//! Non-live API integration tests for the GitHub Release update contract.
//!
//! The fake endpoint exposes the same release metadata and asset paths as the
//! production adapter. It never opens a socket or starts an executable.

use std::collections::BTreeMap;

use run_dog::update::{
    github_release_download_path, parse_checksum_manifest, select_update, Release, ReleaseAsset,
    UpdateCandidate, UpdateCheckOrigin, UpdateCheckOutcome, UpdateProtocolError, UpdateRepository,
    UpdateSchedule, Version, CHECKSUM_ASSET_NAME, INSTALLER_ASSET_NAME, UPDATE_CHECK_INTERVAL_MS,
    UPDATE_RETRY_BASE_MS,
};

const WINDOWS_ADAPTER: &str = include_str!("../src/windows/mod.rs");

const REPOSITORY: &str = "example-org/run-dog";
const INSTALLER_FIXTURE: &[u8] = b"abc";
const INSTALLER_FIXTURE_SHA256: &str =
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[derive(Clone, Debug, Eq, PartialEq)]
enum UpdateFlowResult {
    Current,
    Available(UpdateCandidate),
    InstalledAfterPermission(Version),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UpdateFlowError {
    Protocol(UpdateProtocolError),
    MissingAsset,
    InvalidChecksumEncoding,
    UnexpectedFixturePayload,
    ChecksumMismatch,
    InstallWithoutPermission,
}

/// Protocol-compatible in-memory GitHub Releases endpoint and asset store.
struct FakeGitHubReleaseApi {
    repository: UpdateRepository,
    release: Release,
    assets: BTreeMap<String, Vec<u8>>,
    requests: Vec<String>,
}

impl FakeGitHubReleaseApi {
    fn new(release: Release) -> Self {
        Self {
            repository: UpdateRepository::new(REPOSITORY).unwrap(),
            release,
            assets: BTreeMap::new(),
            requests: Vec::new(),
        }
    }

    fn latest_release(&mut self) -> Release {
        self.requests
            .push(format!("/repos/{REPOSITORY}/releases/latest"));
        self.release.clone()
    }

    fn add_asset(&mut self, tag: &str, name: &str, bytes: impl Into<Vec<u8>>) {
        self.assets.insert(
            format!("/{REPOSITORY}/releases/download/{tag}/{name}"),
            bytes.into(),
        );
    }

    fn download(&mut self, url: &str, asset_name: &str) -> Result<Vec<u8>, UpdateFlowError> {
        let path = github_release_download_path(&self.repository, url, asset_name)
            .map_err(UpdateFlowError::Protocol)?;
        self.requests.push(path.clone());
        self.assets
            .get(&path)
            .cloned()
            .ok_or(UpdateFlowError::MissingAsset)
    }
}

#[derive(Default)]
struct FakeInstallerLauncher {
    launched_versions: Vec<Version>,
}

impl FakeInstallerLauncher {
    fn launch_after_permission(&mut self, candidate: &UpdateCandidate) {
        self.launched_versions.push(candidate.version.clone());
    }
}

/// Independent fixture oracle. `abc` has this SHA-256 by the published
/// SHA-256 test vector; anything else is treated as a transfer failure.
fn fixture_sha256(payload: &[u8]) -> Option<&'static str> {
    (payload == INSTALLER_FIXTURE).then_some(INSTALLER_FIXTURE_SHA256)
}

/// Wall-clock start of every scenario (2026-10-31T00:00:00Z).
const NOW: u64 = 1_793_404_800_000;

/// One check through the shared update policy plus protocol-compatible fake
/// I/O. This follows the same `UpdateSchedule` calls as the Win32 adapter:
/// `begin` when a check starts, `complete` on UPDATE_CHECK_DONE, and
/// `install_failed` on UPDATE_INSTALL_FAILED.
fn execute_update_flow_with_fakes(
    current: &Version,
    endpoint: &mut FakeGitHubReleaseApi,
    launcher: &mut FakeInstallerLauncher,
    schedule: &mut UpdateSchedule,
    origin: UpdateCheckOrigin,
    now_ms: u64,
    setting_enabled: bool,
) -> Result<UpdateFlowResult, UpdateFlowError> {
    schedule.begin(origin);
    let release = endpoint.latest_release();
    let candidate = match select_update(&endpoint.repository, current, &release) {
        Ok(candidate) => candidate,
        Err(error) => {
            let _ = schedule.complete(
                now_ms,
                UpdateCheckOutcome::Failed {
                    retry_not_before_ms: None,
                },
                setting_enabled,
                0,
            );
            return Err(UpdateFlowError::Protocol(error));
        }
    };
    let Some(candidate) = candidate else {
        let _ = schedule.complete(now_ms, UpdateCheckOutcome::Current, setting_enabled, 0);
        return Ok(UpdateFlowResult::Current);
    };

    let decision = schedule.complete(
        now_ms,
        UpdateCheckOutcome::Available(candidate.version.clone()),
        setting_enabled,
        0,
    );
    if !decision.install {
        return Ok(UpdateFlowResult::Available(candidate));
    }

    install_with_fakes(endpoint, launcher, candidate).inspect_err(|_| {
        schedule.install_failed(now_ms, 0);
    })
}

fn install_with_fakes(
    endpoint: &mut FakeGitHubReleaseApi,
    launcher: &mut FakeInstallerLauncher,
    candidate: UpdateCandidate,
) -> Result<UpdateFlowResult, UpdateFlowError> {
    let checksum = endpoint.download(&candidate.checksum_url, CHECKSUM_ASSET_NAME)?;
    let checksum =
        std::str::from_utf8(&checksum).map_err(|_| UpdateFlowError::InvalidChecksumEncoding)?;
    let expected_hash = parse_checksum_manifest(checksum).map_err(UpdateFlowError::Protocol)?;

    let installer = endpoint.download(&candidate.installer_url, INSTALLER_ASSET_NAME)?;
    let actual_hash =
        fixture_sha256(&installer).ok_or(UpdateFlowError::UnexpectedFixturePayload)?;
    if expected_hash != actual_hash {
        return Err(UpdateFlowError::ChecksumMismatch);
    }

    launcher.launch_after_permission(&candidate);
    Ok(UpdateFlowResult::InstalledAfterPermission(
        candidate.version,
    ))
}

fn release(tag: &str) -> Release {
    Release {
        tag_name: tag.to_owned(),
        draft: false,
        prerelease: false,
        assets: [INSTALLER_ASSET_NAME, CHECKSUM_ASSET_NAME]
            .into_iter()
            .map(|name| ReleaseAsset {
                name: name.to_owned(),
                browser_download_url: format!(
                    "https://github.com/{REPOSITORY}/releases/download/{tag}/{name}"
                ),
            })
            .collect(),
    }
}

fn endpoint_with_assets(tag: &str, checksum: String, installer: &[u8]) -> FakeGitHubReleaseApi {
    let mut endpoint = FakeGitHubReleaseApi::new(release(tag));
    endpoint.add_asset(tag, CHECKSUM_ASSET_NAME, checksum);
    endpoint.add_asset(tag, INSTALLER_ASSET_NAME, installer);
    endpoint
}

fn candidate(tag: &str) -> UpdateCandidate {
    UpdateCandidate {
        version: Version::parse(tag.trim_start_matches('v')).unwrap(),
        installer_url: format!(
            "https://github.com/{REPOSITORY}/releases/download/{tag}/{INSTALLER_ASSET_NAME}"
        ),
        checksum_url: format!(
            "https://github.com/{REPOSITORY}/releases/download/{tag}/{CHECKSUM_ASSET_NAME}"
        ),
    }
}

fn verified_endpoint(tag: &str) -> FakeGitHubReleaseApi {
    endpoint_with_assets(
        tag,
        format!("{INSTALLER_FIXTURE_SHA256}  {INSTALLER_ASSET_NAME}\n"),
        INSTALLER_FIXTURE,
    )
}

#[test]
fn integration_automatic_update_verifies_and_installs_while_manual_check_only_offers() {
    let mut endpoint = verified_endpoint("v1.2.4");
    let mut launcher = FakeInstallerLauncher::default();
    let mut schedule = UpdateSchedule::default();

    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut endpoint,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            NOW,
            true,
        ),
        Ok(UpdateFlowResult::InstalledAfterPermission(
            Version::parse("1.2.4").unwrap()
        ))
    );
    assert_eq!(
        endpoint.requests,
        [
            "/repos/example-org/run-dog/releases/latest",
            "/example-org/run-dog/releases/download/v1.2.4/RunDog-Setup-x64.exe.sha256",
            "/example-org/run-dog/releases/download/v1.2.4/RunDog-Setup-x64.exe",
        ]
    );
    assert_eq!(launcher.launched_versions.len(), 1);

    // A manual check never inherits install permission; it offers Install.
    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut endpoint,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Manual,
            NOW,
            true,
        ),
        Ok(UpdateFlowResult::Available(candidate("v1.2.4")))
    );
    assert_eq!(launcher.launched_versions.len(), 1);
}

#[test]
fn integration_release_published_while_running_installs_on_a_later_automatic_check() {
    let current = Version::parse("1.2.3").unwrap();
    let mut launcher = FakeInstallerLauncher::default();
    let mut schedule = UpdateSchedule::default();

    // Startup check: the release does not exist yet.
    let mut before_release = verified_endpoint("v1.2.3");
    assert!(schedule.is_due(NOW));
    assert_eq!(
        execute_update_flow_with_fakes(
            &current,
            &mut before_release,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            NOW,
            true,
        ),
        Ok(UpdateFlowResult::Current)
    );
    assert!(!schedule.is_due(NOW + UPDATE_CHECK_INTERVAL_MS - 1));

    // v1.2.4 is published while RunDog keeps running; the next due check
    // installs it without a restart.
    let later = NOW + UPDATE_CHECK_INTERVAL_MS;
    assert!(schedule.is_due(later));
    let mut after_release = verified_endpoint("v1.2.4");
    assert_eq!(
        execute_update_flow_with_fakes(
            &current,
            &mut after_release,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            later,
            true,
        ),
        Ok(UpdateFlowResult::InstalledAfterPermission(
            Version::parse("1.2.4").unwrap()
        ))
    );
    assert_eq!(launcher.launched_versions.len(), 1);
}

#[test]
fn integration_update_rejects_corrupt_or_stale_release_and_retries_with_backoff() {
    let mut corrupt = endpoint_with_assets(
        "v1.2.4",
        format!("{}  {INSTALLER_ASSET_NAME}\n", "0".repeat(64)),
        INSTALLER_FIXTURE,
    );
    let mut launcher = FakeInstallerLauncher::default();
    let mut schedule = UpdateSchedule::default();
    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut corrupt,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            NOW,
            true,
        ),
        Err(UpdateFlowError::ChecksumMismatch)
    );
    assert!(launcher.launched_versions.is_empty());
    // A failed install is retried after the backoff, not left failed.
    assert!(!schedule.is_due(NOW + UPDATE_RETRY_BASE_MS - 1));
    assert!(schedule.is_due(NOW + UPDATE_RETRY_BASE_MS));

    let mut failed_check = FakeGitHubReleaseApi::new(release("not-a-version"));
    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut failed_check,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            NOW,
            true,
        ),
        Err(UpdateFlowError::Protocol(
            UpdateProtocolError::InvalidVersion
        ))
    );
    assert!(launcher.launched_versions.is_empty());
    assert!(!schedule.is_due(NOW));

    let mut stale = verified_endpoint("v1.2.3");
    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut stale,
            &mut launcher,
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            NOW,
            true,
        ),
        Ok(UpdateFlowResult::Current)
    );
    assert_eq!(
        stale.requests,
        ["/repos/example-org/run-dog/releases/latest"]
    );
    assert!(launcher.launched_versions.is_empty());
}

#[test]
fn integration_disabled_setting_and_exit_never_install() {
    let mut endpoint = verified_endpoint("v1.2.4");
    let mut launcher = FakeInstallerLauncher::default();
    let mut schedule = UpdateSchedule::default();
    let available = execute_update_flow_with_fakes(
        &Version::parse("1.2.3").unwrap(),
        &mut endpoint,
        &mut launcher,
        &mut schedule,
        UpdateCheckOrigin::Automatic,
        NOW,
        false,
    )
    .unwrap();
    let UpdateFlowResult::Available(candidate) = available else {
        panic!("expected an available candidate");
    };

    assert!(launcher.launched_versions.is_empty());
    assert_eq!(candidate.version, Version::parse("1.2.4").unwrap());

    schedule.cancel();
    assert!(!schedule.is_due(NOW + 10 * UPDATE_CHECK_INTERVAL_MS));
    let available = execute_update_flow_with_fakes(
        &Version::parse("1.2.3").unwrap(),
        &mut endpoint,
        &mut launcher,
        &mut schedule,
        UpdateCheckOrigin::Automatic,
        NOW,
        true,
    )
    .unwrap();
    assert!(matches!(available, UpdateFlowResult::Available(_)));
    assert!(launcher.launched_versions.is_empty());
    let _ = UpdateFlowError::InstallWithoutPermission;
}

#[test]
fn integration_windows_adapter_routes_the_schedule_to_checks_and_installer() {
    for wiring in [
        "UPDATE_TIMER_ID => context.tick_update(),",
        "self.update_schedule.begin(UpdateCheckOrigin::Automatic);",
        "context.update_schedule.begin(UpdateCheckOrigin::Manual);",
        "let decision = self.update_schedule.complete(",
        "self.updater.install_available(self.platform.hwnd);",
        ".install_failed(unix_now_ms(), update_check_jitter_ms());",
        "self.update_schedule.cancel();",
    ] {
        assert!(WINDOWS_ADAPTER.contains(wiring), "{wiring}");
    }
}
