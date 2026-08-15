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
        WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
        WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    },
    UI::{
        Shell::ShellExecuteW,
        WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL},
    },
};

use crate::update::{
    github_release_download_path, parse_checksum_manifest, select_update, Release, ReleaseAsset,
    UpdateCandidate, UpdateRepository, Version, CHECKSUM_ASSET_NAME, INSTALLER_ASSET_NAME,
};

/// Reserved application message used only after a verified installer has been
/// handed to ShellExecute. The main thread then removes the tray icon before
/// Inno Setup replaces the executable.
pub const UPDATE_REQUEST_EXIT_MESSAGE: u32 = 0x8000 + 2;

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
        }
    }

    #[must_use]
    pub fn menu_state(&self) -> UpdateMenuState {
        UpdateMenuState::from(&*lock_state(&self.state))
    }

    /// Starts an asynchronous check if no check or install is already active.
    pub fn check_for_updates(&self) {
        {
            let mut state = lock_state(&self.state);
            if matches!(
                *state,
                UpdateState::Checking | UpdateState::Downloading(_) | UpdateState::Launching
            ) {
                return;
            }
            *state = UpdateState::Checking;
        }

        let state = Arc::clone(&self.state);
        let repository = self.repository.clone();
        let current_version = self.current_version.clone();
        let cancelled = Arc::clone(&self.cancelled);
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
                }
            });

        if worker.is_err() {
            *lock_state(&self.state) = UpdateState::Failed;
        }
    }

    /// Streams, verifies, and starts the available installer without blocking
    /// the message-loop thread. A successful launch requests orderly shutdown.
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
        let hwnd = hwnd as usize;
        let worker = thread::Builder::new()
            .name("run-dog-update-install".to_owned())
            .spawn(move || {
                if cancelled.load(Ordering::Acquire) {
                    return;
                }

                match download_verify_and_launch(&repository, &candidate, &cancelled) {
                    Ok(()) if !cancelled.load(Ordering::Acquire) => {
                        *lock_state(&state) = UpdateState::Launching;
                        let _ = unsafe {
                            PostMessageW(hwnd as HWND, UPDATE_REQUEST_EXIT_MESSAGE, 0, 0)
                        };
                    }
                    Ok(()) | Err(()) => {
                        if !cancelled.load(Ordering::Acquire) {
                            *lock_state(&state) = UpdateState::Failed;
                        }
                    }
                }
            });

        if worker.is_err() {
            *lock_state(&self.state) = UpdateState::Failed;
        }
    }

    /// Prevents a worker from launching an installer after the user exits.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

fn lock_state(state: &Mutex<UpdateState>) -> MutexGuard<'_, UpdateState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn fetch_latest_release(
    repository: &UpdateRepository,
    current_version: &Version,
) -> Result<Option<UpdateCandidate>, ()> {
    let path = format!("/repos/{}/releases/latest", repository.as_str());
    let (status, response) =
        https_get(API_HOST, &path, API_HEADERS, MAX_RELEASE_METADATA_BYTES).map_err(|_| ())?;

    if status != 200 {
        return Err(());
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

fn download_verify_and_launch(
    repository: &UpdateRepository,
    candidate: &UpdateCandidate,
    cancelled: &AtomicBool,
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

    launch_installer(&installer_path).map_err(|_| ())
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
            RELEASE_HOST,
            &asset_path,
            "",
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
    let path = path
        .to_str()
        .ok_or_else(|| "installer path is not valid UTF-16 input".to_owned())?;
    let operation = wide("open");
    let installer = wide(path);
    let parameters = wide("/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS");
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            installer.as_ptr(),
            parameters.as_ptr(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize <= 32 {
        return Err(format!(
            "ShellExecuteW failed with code {}",
            result as isize
        ));
    }

    Ok(())
}

fn https_get(
    host: &str,
    path: &str,
    headers: &str,
    maximum_bytes: usize,
) -> Result<(u32, Vec<u8>), String> {
    let mut body = Vec::new();
    let status = https_request_to_writer(host, path, headers, maximum_bytes, &mut body, None)?;
    Ok((status, body))
}

fn https_request_to_writer(
    host: &str,
    path: &str,
    headers: &str,
    maximum_bytes: usize,
    writer: &mut impl Write,
    cancelled: Option<&AtomicBool>,
) -> Result<u32, String> {
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

    let host_wide = wide(host);
    let connection = HttpHandle::new("WinHttpConnect", unsafe {
        WinHttpConnect(session.0, host_wide.as_ptr(), 443, 0)
    })?;
    let verb = wide("GET");
    let path_wide = wide(path);
    let request = HttpHandle::new("WinHttpOpenRequest", unsafe {
        WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            path_wide.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    })?;
    let headers_wide = wide(headers);
    let (headers_pointer, headers_length) = if headers.is_empty() {
        (ptr::null(), 0)
    } else {
        (headers_wide.as_ptr(), (headers_wide.len() - 1) as u32)
    };
    if unsafe {
        WinHttpSendRequest(
            request.0,
            headers_pointer,
            headers_length,
            ptr::null(),
            0,
            0,
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

    Ok(status)
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
    use std::sync::atomic::AtomicBool;

    use super::{is_cancelled, sha256_reader, GitHubRelease};

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
}
