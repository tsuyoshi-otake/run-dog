//! Windows-only GitHub Releases updater.
//!
//! Network work is performed only in short-lived worker threads. Release
//! metadata is capped in memory; installer bytes are streamed to disk and
//! hashed before the Inno Setup executable is started.

use std::{
    ffi::c_void,
    fs::{self, File},
    io::{Read, Write},
    mem::size_of_val,
    path::{Path, PathBuf},
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND},
    Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
        WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE,
        WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
        WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE,
    },
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::update::{
    github_release_download_path, parse_checksum_manifest, select_update, Release, ReleaseAsset,
    UpdateCandidate, UpdateRepository, Version, CHECKSUM_ASSET_NAME, INSTALLER_ASSET_NAME,
};

/// Reserved application message used only after a verified installer has been
/// handed to ShellExecute. The main thread then removes the tray icon before
/// Inno Setup replaces the executable.
pub const UPDATE_REQUEST_EXIT_MESSAGE: u32 = 0x8000 + 2;
/// Posted when an update check worker reaches a terminal menu state.
pub const UPDATE_CHECK_DONE_MESSAGE: u32 = 0x8000 + 4;

const API_HOST: &str = "api.github.com";
const RELEASE_HOST: &str = "github.com";
const API_HEADERS: &str =
    "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n";
const MAX_RELEASE_METADATA_BYTES: usize = 1_024 * 1_024;
const MAX_CHECKSUM_BYTES: usize = 16 * 1_024;
const MAX_INSTALLER_BYTES: usize = 100 * 1_024 * 1_024;
const NETWORK_TIMEOUT_MS: i32 = 15_000;
const NETWORK_RECEIVE_TIMEOUT_MS: i32 = 30_000;
const NETWORK_BUFFER_BYTES: usize = 16 * 1_024;
const HASH_BUFFER_BYTES: usize = 32 * 1_024;
const INNO_SILENT_PARAMETERS: &str = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS";
const MAX_REDIRECTS: u8 = 5;
const TLS_PROTOCOLS: u32 =
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;

/// Values displayed by the tray menu. This type contains no Win32 handles and
/// can be copied without retaining a network worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateMenuState {
    Idle,
    Checking,
    Current,
    Available { version: String },
    Downloading { version: String },
    Launching,
    Failed,
}

#[derive(Clone, Debug)]
enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(UpdateCandidate),
    Downloading(UpdateCandidate),
    Launching,
    Failed,
}

impl From<&UpdateState> for UpdateMenuState {
    fn from(value: &UpdateState) -> Self {
        match value {
            UpdateState::Idle => Self::Idle,
            UpdateState::Checking => Self::Checking,
            UpdateState::Current => Self::Current,
            UpdateState::Available(candidate) => Self::Available {
                version: candidate.version.to_string(),
            },
            UpdateState::Downloading(candidate) => Self::Downloading {
                version: candidate.version.to_string(),
            },
            UpdateState::Launching => Self::Launching,
            UpdateState::Failed => Self::Failed,
        }
    }
}

/// Owns no persistent network resources. Each operation opens its own WinHTTP
/// handles and releases them before the worker exits.
pub struct UpdateController {
    state: Arc<Mutex<UpdateState>>,
    repository: UpdateRepository,
    current_version: Version,
    cancelled: Arc<AtomicBool>,
    /// Serializes the final cancellation check with `ShellExecuteW`. Once
    /// `cancel` returns, no worker can launch a new installer.
    launch_gate: Arc<Mutex<()>>,
}

impl UpdateController {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(UpdateState::Idle)),
            repository: UpdateRepository::from_build_config(),
            current_version: Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("Cargo package version must be MAJOR.MINOR.PATCH"),
            cancelled: Arc::new(AtomicBool::new(false)),
            launch_gate: Arc::new(Mutex::new(())),
        }
    }

    #[must_use]
    pub fn menu_state(&self) -> UpdateMenuState {
        UpdateMenuState::from(&*lock_state(&self.state))
    }

    /// Starts an asynchronous check if no check or install is already active.
    ///
    /// `notify_always` is true for a user-initiated Check. Startup checks only
    /// surface a balloon when a newer release is actually available.
    pub fn check_for_updates(&self, hwnd: HWND, notify_always: bool) {
        if !begin_check(&self.state) {
            return;
        }

        let state = Arc::clone(&self.state);
        let repository = self.repository.clone();
        let current_version = self.current_version.clone();
        let cancelled = Arc::clone(&self.cancelled);
        let hwnd_bits = hwnd as usize;
        let notify = usize::from(notify_always);
        let worker = thread::Builder::new()
            .name("run-dog-update-check".to_owned())
            .spawn(move || {
                let next_state = if cancelled.load(Ordering::Acquire) {
                    return;
                } else {
                    match fetch_latest_release(&repository, &current_version) {
                        Ok(Some(candidate)) => UpdateState::Available(candidate),
                        Ok(None) => UpdateState::Current,
                        Err(()) => UpdateState::Failed,
                    }
                };

                if !cancelled.load(Ordering::Acquire) {
                    *lock_state(&state) = next_state;
                    let _ = unsafe {
                        PostMessageW(hwnd_bits as HWND, UPDATE_CHECK_DONE_MESSAGE, notify, 0)
                    };
                }
            });

        if worker.is_err() {
            *lock_state(&self.state) = UpdateState::Failed;
            if notify_always && !hwnd.is_null() {
                let _ = unsafe { PostMessageW(hwnd, UPDATE_CHECK_DONE_MESSAGE, 1, 0) };
            }
        }
    }

    /// Streams, verifies, and starts the available installer without blocking
    /// the message-loop thread. Called only after the user selects Install in
    /// the tray menu. A successful launch requests orderly shutdown.
    pub fn install_available(&self, hwnd: HWND) {
        let candidate = {
            let mut state = lock_state(&self.state);
            let UpdateState::Available(candidate) = &*state else {
                return;
            };
            let candidate = candidate.clone();
            *state = UpdateState::Downloading(candidate.clone());
            candidate
        };

        let state = Arc::clone(&self.state);
        let repository = self.repository.clone();
        let cancelled = Arc::clone(&self.cancelled);
        let launch_gate = Arc::clone(&self.launch_gate);
        let hwnd = hwnd as usize;
        let worker = thread::Builder::new()
            .name("run-dog-update-install".to_owned())
            .spawn(move || {
                install_candidate(
                    &state,
                    &repository,
                    &candidate,
                    &cancelled,
                    &launch_gate,
                    hwnd as HWND,
                );
            });

        if worker.is_err() {
            *lock_state(&self.state) = UpdateState::Failed;
        }
    }

    /// Prevents a worker from launching an installer after the user exits.
    pub fn cancel(&self) {
        let _launch_gate = lock_launch_gate(&self.launch_gate);
        self.cancelled.store(true, Ordering::Release);
    }
}

fn lock_state(state: &Mutex<UpdateState>) -> MutexGuard<'_, UpdateState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_launch_gate(gate: &Mutex<()>) -> MutexGuard<'_, ()> {
    gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn begin_check(state: &Mutex<UpdateState>) -> bool {
    let mut state = lock_state(state);
    if matches!(
        *state,
        UpdateState::Checking | UpdateState::Downloading(_) | UpdateState::Launching
    ) {
        return false;
    }
    *state = UpdateState::Checking;
    true
}

fn install_candidate(
    state: &Mutex<UpdateState>,
    repository: &UpdateRepository,
    candidate: &UpdateCandidate,
    cancelled: &AtomicBool,
    launch_gate: &Mutex<()>,
    hwnd: HWND,
) {
    if cancelled.load(Ordering::Acquire) {
        return;
    }

    match download_verify_and_launch(repository, candidate, cancelled, launch_gate) {
        Ok(()) if !cancelled.load(Ordering::Acquire) => {
            *lock_state(state) = UpdateState::Launching;
            let _ = unsafe { PostMessageW(hwnd, UPDATE_REQUEST_EXIT_MESSAGE, 0, 0) };
        }
        Ok(()) | Err(()) => {
            if !cancelled.load(Ordering::Acquire) {
                *lock_state(state) = UpdateState::Failed;
            }
        }
    }
}

fn fetch_latest_release(
    repository: &UpdateRepository,
    current_version: &Version,
) -> Result<Option<UpdateCandidate>, ()> {
    let path = format!("/repos/{}/releases/latest", repository.as_str());
    let (status, response) =
        https_get(API_HOST, &path, API_HEADERS, MAX_RELEASE_METADATA_BYTES).map_err(|_| ())?;

    if !latest_release_body_is_available(status)? {
        return Ok(None);
    }

    let release: GitHubRelease = serde_json::from_slice(&response).map_err(|_| ())?;
    let release = Release {
        tag_name: release.tag_name,
        draft: release.draft,
        prerelease: release.prerelease,
        assets: release
            .assets
            .into_iter()
            .map(|asset| ReleaseAsset {
                name: asset.name,
                browser_download_url: asset.browser_download_url,
            })
            .collect(),
    };
    select_update(repository, current_version, &release).map_err(|_| ())
}

/// Maps GitHub's latest-release response into the update protocol's terminal
/// states. A repository with only pre-releases returns 404 from this endpoint.
fn latest_release_body_is_available(status: u32) -> Result<bool, ()> {
    match status {
        200 => Ok(true),
        404 => Ok(false),
        _ => Err(()),
    }
}

fn download_verify_and_launch(
    repository: &UpdateRepository,
    candidate: &UpdateCandidate,
    cancelled: &AtomicBool,
    launch_gate: &Mutex<()>,
) -> Result<(), ()> {
    let checksum_path =
        github_release_download_path(repository, &candidate.checksum_url, CHECKSUM_ASSET_NAME)
            .map_err(|_| ())?;
    let (status, checksum) =
        https_get(RELEASE_HOST, &checksum_path, "", MAX_CHECKSUM_BYTES).map_err(|_| ())?;
    if status != 200 {
        return Err(());
    }
    let checksum = std::str::from_utf8(&checksum).map_err(|_| ())?;
    let expected_hash = parse_checksum_manifest(checksum).map_err(|_| ())?;

    if cancelled.load(Ordering::Acquire) {
        return Err(());
    }

    let installer_path =
        download_installer(repository, candidate, &expected_hash, cancelled).map_err(|_| ())?;
    if cancelled.load(Ordering::Acquire) {
        let _ = fs::remove_file(&installer_path);
        return Err(());
    }

    launch_installer_if_not_cancelled(&installer_path, cancelled, launch_gate).map_err(|_| ())
}

fn launch_installer_if_not_cancelled(
    installer_path: &Path,
    cancelled: &AtomicBool,
    launch_gate: &Mutex<()>,
) -> Result<(), String> {
    let _launch_gate = lock_launch_gate(launch_gate);
    if cancelled.load(Ordering::Acquire) {
        return Err("update cancelled".to_owned());
    }
    launch_installer(installer_path)
}

fn download_installer(
    repository: &UpdateRepository,
    candidate: &UpdateCandidate,
    expected_hash: &str,
    cancelled: &AtomicBool,
) -> Result<PathBuf, String> {
    let asset_path =
        github_release_download_path(repository, &candidate.installer_url, INSTALLER_ASSET_NAME)
            .map_err(|error| error.to_string())?;
    let directory = update_directory()?;
    let installer_path = directory.join(format!("RunDog-Setup-{}.exe", candidate.version));
    let partial_path = installer_path.with_extension("exe.part");
    let _ = fs::remove_file(&partial_path);

    let download_result = (|| {
        let mut file = File::create(&partial_path).map_err(|error| error.to_string())?;
        let status = https_request_to_writer(
            "GET",
            RELEASE_HOST,
            &asset_path,
            "",
            None,
            MAX_INSTALLER_BYTES,
            &mut file,
            Some(cancelled),
        )?;
        file.sync_all().map_err(|error| error.to_string())?;
        if status != 200 {
            return Err(format!("installer download returned HTTP {status}"));
        }
        Ok(())
    })();
    if let Err(error) = download_result {
        let _ = fs::remove_file(&partial_path);
        return Err(error);
    }

    if cancelled.load(Ordering::Acquire) {
        let _ = fs::remove_file(&partial_path);
        return Err("update cancelled".to_owned());
    }

    let actual_hash = sha256_file(&partial_path)?;
    if !expected_hash.eq_ignore_ascii_case(&actual_hash) {
        let _ = fs::remove_file(&partial_path);
        return Err("installer checksum did not match".to_owned());
    }

    if installer_path.exists() {
        fs::remove_file(&installer_path).map_err(|error| error.to_string())?;
    }
    fs::rename(&partial_path, &installer_path).map_err(|error| error.to_string())?;
    Ok(installer_path)
}

fn update_directory() -> Result<PathBuf, String> {
    let local_app_data =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA is unavailable".to_owned())?;
    let directory = PathBuf::from(local_app_data)
        .join("SystemExe")
        .join("RunDog")
        .join("updates");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    sha256_reader(&mut file)
}

fn sha256_reader(reader: &mut impl Read) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn launch_installer(path: &Path) -> Result<(), String> {
    let updates = update_directory()?;
    let canonical = fs::canonicalize(path).map_err(|error| error.to_string())?;
    let root = fs::canonicalize(&updates).map_err(|error| error.to_string())?;
    if !canonical.starts_with(root) {
        return Err("installer path escaped the update directory".to_owned());
    }
    super::launch_detached(&canonical, INNO_SILENT_PARAMETERS)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RedirectPolicy {
    Deny,
    GitHubAssets,
}

fn redirect_policy_for_host(host: &str) -> RedirectPolicy {
    if is_github_download_host(host) {
        RedirectPolicy::GitHubAssets
    } else {
        RedirectPolicy::Deny
    }
}

#[must_use]
pub(super) fn is_github_download_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "github.com" || host == "api.github.com" || is_github_release_cdn_host(&host)
}

#[must_use]
pub(super) fn is_github_release_cdn_host(host: &str) -> bool {
    matches!(
        host,
        "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "github-releases.githubusercontent.com"
    )
}

#[must_use]
pub(super) fn github_redirect_allowed(from: &str, to: &str) -> bool {
    let from = from.to_ascii_lowercase();
    let to = to.to_ascii_lowercase();
    if from == "api.github.com" {
        return to == "api.github.com";
    }
    if from == "github.com" {
        return to == "github.com" || is_github_release_cdn_host(&to);
    }
    is_github_release_cdn_host(&from) && to == from
}

#[must_use]
fn is_redirect_status(status: u32) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Parses `Location` as HTTPS only. Relative `/path` stays on the current host.
#[must_use]
pub(super) fn parse_https_location(location: &str, current_host: &str) -> Option<(String, String)> {
    let location = location.trim();
    if let Some(hash) = location.find('#') {
        return parse_https_location(&location[..hash], current_host);
    }
    if location.starts_with('/') {
        if location
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, b'\\' | 0))
        {
            return None;
        }
        return Some((current_host.to_ascii_lowercase(), location.to_owned()));
    }
    let rest = location.strip_prefix("https://")?;
    if rest.contains('\\') || rest.contains('@') || rest.contains('\0') {
        return None;
    }
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = if let Some((host, port)) = host_port.split_once(':') {
        if port != "443" {
            return None;
        }
        host
    } else {
        host_port
    };
    if host.is_empty()
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return None;
    }
    Some((host.to_ascii_lowercase(), format!("/{path}")))
}

pub(super) fn https_get(
    host: &str,
    path: &str,
    headers: &str,
    maximum_bytes: usize,
) -> Result<(u32, Vec<u8>), String> {
    https_bytes("GET", host, path, headers, None, maximum_bytes)
}

pub(super) fn https_post(
    host: &str,
    path: &str,
    headers: &str,
    payload: &[u8],
    maximum_bytes: usize,
) -> Result<(u32, Vec<u8>), String> {
    https_bytes("POST", host, path, headers, Some(payload), maximum_bytes)
}

fn https_bytes(
    verb: &str,
    host: &str,
    path: &str,
    headers: &str,
    payload: Option<&[u8]>,
    maximum_bytes: usize,
) -> Result<(u32, Vec<u8>), String> {
    let mut body = Vec::new();
    let status = https_request_to_writer(
        verb,
        host,
        path,
        headers,
        payload,
        maximum_bytes,
        &mut body,
        None,
    )?;
    Ok((status, body))
}

#[allow(clippy::too_many_arguments)]
fn https_request_to_writer(
    verb: &str,
    host: &str,
    path: &str,
    headers: &str,
    payload: Option<&[u8]>,
    maximum_bytes: usize,
    writer: &mut impl Write,
    cancelled: Option<&AtomicBool>,
) -> Result<u32, String> {
    let policy = redirect_policy_for_host(host);
    let mut host = host.to_owned();
    let mut path = path.to_owned();
    let mut headers = headers.to_owned();
    let mut verb = verb.to_owned();
    let mut payload = payload.map(Vec::from);

    for _ in 0..=MAX_REDIRECTS {
        if is_cancelled(cancelled) {
            return Err("update cancelled".to_owned());
        }

        let agent = wide(&format!("RunDog/{}", env!("CARGO_PKG_VERSION")));
        let session = HttpHandle::new("WinHttpOpen", unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        })?;
        if unsafe {
            WinHttpSetTimeouts(
                session.0,
                NETWORK_TIMEOUT_MS,
                NETWORK_TIMEOUT_MS,
                NETWORK_TIMEOUT_MS,
                NETWORK_RECEIVE_TIMEOUT_MS,
            )
        } == 0
        {
            return Err(last_error("WinHttpSetTimeouts"));
        }
        apply_tls12_plus(&session)?;

        let host_wide = wide(&host);
        let connection = HttpHandle::new("WinHttpConnect", unsafe {
            WinHttpConnect(session.0, host_wide.as_ptr(), 443, 0)
        })?;
        let verb_wide = wide(&verb);
        let path_wide = wide(&path);
        let request = HttpHandle::new("WinHttpOpenRequest", unsafe {
            WinHttpOpenRequest(
                connection.0,
                verb_wide.as_ptr(),
                path_wide.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        disable_automatic_redirects(&request)?;
        let headers_wide = wide(&headers);
        let (headers_pointer, headers_length) = if headers.is_empty() {
            (ptr::null(), 0)
        } else {
            (headers_wide.as_ptr(), (headers_wide.len() - 1) as u32)
        };
        let (optional, optional_len, total_len) = match payload.as_deref() {
            Some(bytes) if !bytes.is_empty() => (
                bytes.as_ptr().cast::<c_void>(),
                bytes.len() as u32,
                bytes.len() as u32,
            ),
            _ => (ptr::null(), 0, 0),
        };
        if unsafe {
            WinHttpSendRequest(
                request.0,
                headers_pointer,
                headers_length,
                optional.cast_mut(),
                optional_len,
                total_len,
                0,
            )
        } == 0
        {
            return Err(last_error("WinHttpSendRequest"));
        }
        if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
            return Err(last_error("WinHttpReceiveResponse"));
        }

        let status = http_status(&request)?;
        if is_redirect_status(status) {
            if policy != RedirectPolicy::GitHubAssets {
                return Err("HTTPS redirect refused for this host".to_owned());
            }
            let location = response_location(&request)?;
            let (next_host, next_path) = parse_https_location(&location, &host)
                .ok_or_else(|| "redirect Location is not a valid HTTPS URL".to_owned())?;
            if !github_redirect_allowed(&host, &next_host) {
                return Err("redirect left the GitHub download allowlist".to_owned());
            }
            if !next_host.eq_ignore_ascii_case(&host) {
                headers.clear();
            }
            if status == 303 {
                verb = "GET".to_owned();
                payload = None;
            }
            host = next_host;
            path = next_path;
            continue;
        }
        if status != 200 {
            return Ok(status);
        }

        let mut bytes_written = 0_usize;
        let mut buffer = [0_u8; NETWORK_BUFFER_BYTES];
        loop {
            if is_cancelled(cancelled) {
                return Err("update cancelled".to_owned());
            }
            let mut available = 0_u32;
            if unsafe { WinHttpQueryDataAvailable(request.0, &mut available) } == 0 {
                return Err(last_error("WinHttpQueryDataAvailable"));
            }
            if available == 0 {
                break;
            }

            let requested = available.min(NETWORK_BUFFER_BYTES as u32);
            let mut bytes_read = 0_u32;
            if unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast::<c_void>(),
                    requested,
                    &mut bytes_read,
                )
            } == 0
            {
                return Err(last_error("WinHttpReadData"));
            }
            if bytes_read == 0 {
                return Err("WinHttpReadData returned an empty non-terminal chunk".to_owned());
            }
            let bytes_read = bytes_read as usize;
            if bytes_read > maximum_bytes.saturating_sub(bytes_written) {
                return Err("HTTP response exceeded its size limit".to_owned());
            }
            writer
                .write_all(&buffer[..bytes_read])
                .map_err(|error| error.to_string())?;
            bytes_written += bytes_read;
        }

        return Ok(status);
    }

    Err("HTTPS redirect limit exceeded".to_owned())
}

fn apply_tls12_plus(session: &HttpHandle) -> Result<(), String> {
    let mut protocols = TLS_PROTOCOLS;
    if unsafe {
        WinHttpSetOption(
            session.0,
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            (&raw mut protocols).cast::<c_void>(),
            std::mem::size_of_val(&protocols) as u32,
        )
    } == 0
    {
        protocols = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
        if unsafe {
            WinHttpSetOption(
                session.0,
                WINHTTP_OPTION_SECURE_PROTOCOLS,
                (&raw mut protocols).cast::<c_void>(),
                std::mem::size_of_val(&protocols) as u32,
            )
        } == 0
        {
            return Err(last_error("WinHttpSetOption(SECURE_PROTOCOLS)"));
        }
    }
    Ok(())
}

fn disable_automatic_redirects(request: &HttpHandle) -> Result<(), String> {
    let mut disable = WINHTTP_DISABLE_REDIRECTS;
    if unsafe {
        WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_DISABLE_FEATURE,
            (&raw mut disable).cast::<c_void>(),
            std::mem::size_of_val(&disable) as u32,
        )
    } == 0
    {
        return Err(last_error("WinHttpSetOption(DISABLE_REDIRECTS)"));
    }
    Ok(())
}

fn response_location(request: &HttpHandle) -> Result<String, String> {
    let mut buffer = [0_u16; 2_048];
    let mut size = (buffer.len() * 2) as u32;
    let mut index = 0_u32;
    if unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_LOCATION,
            ptr::null(),
            buffer.as_mut_ptr().cast::<c_void>(),
            &mut size,
            &mut index,
        )
    } == 0
    {
        return Err(last_error("WinHttpQueryHeaders(LOCATION)"));
    }
    let units = (size as usize) / 2;
    let terminator = buffer[..units.min(buffer.len())]
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.min(buffer.len()));
    String::from_utf16(&buffer[..terminator])
        .map_err(|_| "Location header is not UTF-16".to_owned())
}

fn is_cancelled(cancelled: Option<&AtomicBool>) -> bool {
    cancelled.is_some_and(|value| value.load(Ordering::Acquire))
}

fn http_status(request: &HttpHandle) -> Result<u32, String> {
    let mut status = 0_u32;
    let mut size = size_of_val(&status) as u32;
    let mut index = 0_u32;
    if unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            (&mut status as *mut u32).cast::<c_void>(),
            &mut size,
            &mut index,
        )
    } == 0
    {
        return Err(last_error("WinHttpQueryHeaders"));
    }
    Ok(status)
}

struct HttpHandle(*mut c_void);

impl HttpHandle {
    fn new(operation: &str, handle: *mut c_void) -> Result<Self, String> {
        if handle.is_null() {
            Err(last_error(operation))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[must_use]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[must_use]
fn last_error(operation: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("{operation} failed with Win32 error {code}")
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
    };

    use crate::update::{UpdateCandidate, Version};

    use super::{
        begin_check, github_redirect_allowed, is_cancelled, is_github_download_host,
        latest_release_body_is_available, parse_https_location, sha256_reader, GitHubRelease,
        UpdateState, INNO_SILENT_PARAMETERS,
    };

    fn candidate() -> UpdateCandidate {
        UpdateCandidate {
            version: Version::parse("1.2.4").unwrap(),
            installer_url: "https://github.com/example/run-dog/releases/download/v1.2.4/RunDog-Setup-x64.exe".to_owned(),
            checksum_url: "https://github.com/example/run-dog/releases/download/v1.2.4/RunDog-Setup-x64.exe.sha256".to_owned(),
        }
    }

    #[test]
    fn component_github_release_decoder_requires_named_download_assets() {
        let release: GitHubRelease = serde_json::from_str(
            r#"{
                "tag_name": "v1.2.3",
                "draft": false,
                "prerelease": false,
                "assets": [{
                    "name": "RunDog-Setup-x64.exe",
                    "browser_download_url": "https://github.com/example/run-dog/releases/download/v1.2.3/RunDog-Setup-x64.exe"
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(release.tag_name, "v1.2.3");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "RunDog-Setup-x64.exe");
    }

    #[test]
    fn component_sha256_reader_matches_the_published_abc_test_vector() {
        let mut reader = std::io::Cursor::new(b"abc");
        assert_eq!(
            sha256_reader(&mut reader),
            Ok("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned())
        );
    }

    #[test]
    fn component_cancellation_classifier_covers_absent_false_and_true_states() {
        let cancelled = AtomicBool::new(false);
        assert!(!is_cancelled(None));
        assert!(!is_cancelled(Some(&cancelled)));
        cancelled.store(true, std::sync::atomic::Ordering::Release);
        assert!(is_cancelled(Some(&cancelled)));
    }

    #[test]
    fn c2_startup_update_gate_rejects_each_active_state_and_allows_terminal_rechecks() {
        for state in [
            UpdateState::Idle,
            UpdateState::Current,
            UpdateState::Available(candidate()),
            UpdateState::Failed,
        ] {
            let state = Mutex::new(state);
            assert!(begin_check(&state));
            assert!(matches!(*state.lock().unwrap(), UpdateState::Checking));
        }

        for state in [
            UpdateState::Checking,
            UpdateState::Downloading(candidate()),
            UpdateState::Launching,
        ] {
            let state = Mutex::new(state);
            assert!(!begin_check(&state));
        }
    }

    #[test]
    fn component_startup_update_gate_allows_exactly_one_concurrent_claim() {
        let state = Arc::new(Mutex::new(UpdateState::Idle));
        let successful_claims = Arc::new(AtomicUsize::new(0));

        thread::scope(|scope| {
            for _ in 0..8 {
                let state = Arc::clone(&state);
                let successful_claims = Arc::clone(&successful_claims);
                scope.spawn(move || {
                    if begin_check(&state) {
                        successful_claims.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });

        assert_eq!(successful_claims.load(Ordering::Relaxed), 1);
        assert!(matches!(*state.lock().unwrap(), UpdateState::Checking));
    }

    #[test]
    fn component_inno_parameters_suppress_prompts_and_close_the_old_process() {
        assert_eq!(
            INNO_SILENT_PARAMETERS
                .split_ascii_whitespace()
                .collect::<Vec<_>>(),
            [
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
            ]
        );
    }

    #[test]
    fn c2_latest_release_status_distinguishes_release_absence_from_endpoint_failure() {
        assert_eq!(latest_release_body_is_available(200), Ok(true));
        assert_eq!(latest_release_body_is_available(404), Ok(false));
        assert_eq!(latest_release_body_is_available(401), Err(()));
        assert_eq!(latest_release_body_is_available(500), Err(()));
    }

    #[test]
    fn c2_github_redirect_parser_accepts_https_cdn_and_rejects_http_or_credentials() {
        assert_eq!(
            parse_https_location(
                "https://objects.githubusercontent.com/github-production-release-asset/RunDog-Setup-x64.exe",
                "github.com",
            ),
            Some((
                "objects.githubusercontent.com".to_owned(),
                "/github-production-release-asset/RunDog-Setup-x64.exe".to_owned()
            ))
        );
        assert_eq!(
            parse_https_location(
                "/releases/download/v1.2.4/RunDog-Setup-x64.exe",
                "github.com"
            ),
            Some((
                "github.com".to_owned(),
                "/releases/download/v1.2.4/RunDog-Setup-x64.exe".to_owned()
            ))
        );
        assert!(parse_https_location("http://github.com/evil", "github.com").is_none());
        assert!(parse_https_location("https://evil@github.com/asset", "github.com").is_none());
        assert!(parse_https_location("https://example.invalid/asset", "github.com").is_some());
        assert!(github_redirect_allowed(
            "github.com",
            "objects.githubusercontent.com"
        ));
        assert!(github_redirect_allowed(
            "github.com",
            "release-assets.githubusercontent.com"
        ));
        assert!(!github_redirect_allowed(
            "github.com",
            "raw.githubusercontent.com"
        ));
        assert!(!github_redirect_allowed(
            "objects.githubusercontent.com",
            "github.com"
        ));
        assert!(!github_redirect_allowed(
            "api.github.com",
            "objects.githubusercontent.com"
        ));
        assert!(is_github_download_host(
            "release-assets.githubusercontent.com"
        ));
        assert!(!is_github_download_host("raw.githubusercontent.com"));
        assert!(!is_github_download_host(
            "githubusercontent.com.evil.example"
        ));
    }
}
