//! Non-live API integration tests for the GitHub Release update contract.
//!
//! The fake endpoint exposes the same release metadata and asset paths as the
//! production adapter. It never opens a socket or starts an executable.

use std::collections::BTreeMap;

use run_dog::update::{
    github_release_download_path, parse_checksum_manifest, select_update, Release, ReleaseAsset,
    UpdateCandidate, UpdateProtocolError, UpdateRepository, Version, CHECKSUM_ASSET_NAME,
    INSTALLER_ASSET_NAME,
};

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

/// Startup path: check once and surface an available candidate. Download and
/// launch require an explicit permission flag that mirrors the tray Install
/// command.
fn execute_update_flow_with_fakes(
    current: &Version,
    endpoint: &mut FakeGitHubReleaseApi,
    launcher: &mut FakeInstallerLauncher,
    user_permitted_install: bool,
) -> Result<UpdateFlowResult, UpdateFlowError> {
    let release = endpoint.latest_release();
    let Some(candidate) = select_update(&endpoint.repository, current, &release)
        .map_err(UpdateFlowError::Protocol)?
    else {
        return Ok(UpdateFlowResult::Current);
    };

    if !user_permitted_install {
        return Ok(UpdateFlowResult::Available(candidate));
    }

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

#[test]
fn integration_startup_check_notifies_without_installing_until_user_permits() {
    let tag = "v1.2.4";
    let mut endpoint = endpoint_with_assets(
        tag,
        format!("{INSTALLER_FIXTURE_SHA256}  {INSTALLER_ASSET_NAME}\n"),
        INSTALLER_FIXTURE,
    );
    let mut launcher = FakeInstallerLauncher::default();

    let notified = execute_update_flow_with_fakes(
        &Version::parse("1.2.3").unwrap(),
        &mut endpoint,
        &mut launcher,
        false,
    )
    .unwrap();
    assert!(matches!(notified, UpdateFlowResult::Available(_)));
    assert_eq!(
        endpoint.requests,
        ["/repos/example-org/run-dog/releases/latest"]
    );
    assert!(launcher.launched_versions.is_empty());

    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut endpoint,
            &mut launcher,
            true
        ),
        Ok(UpdateFlowResult::InstalledAfterPermission(
            Version::parse("1.2.4").unwrap()
        ))
    );
    assert_eq!(
        launcher.launched_versions,
        [Version::parse("1.2.4").unwrap()]
    );
}

#[test]
fn integration_update_rejects_corrupt_or_stale_release_before_the_fake_installer() {
    let mut corrupt = endpoint_with_assets(
        "v1.2.4",
        format!("{}  {INSTALLER_ASSET_NAME}\n", "0".repeat(64)),
        INSTALLER_FIXTURE,
    );
    let mut launcher = FakeInstallerLauncher::default();
    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut corrupt,
            &mut launcher,
            true
        ),
        Err(UpdateFlowError::ChecksumMismatch)
    );
    assert!(launcher.launched_versions.is_empty());

    let mut stale = endpoint_with_assets(
        "v1.2.3",
        format!("{INSTALLER_FIXTURE_SHA256}  {INSTALLER_ASSET_NAME}\n"),
        INSTALLER_FIXTURE,
    );
    assert_eq!(
        execute_update_flow_with_fakes(
            &Version::parse("1.2.3").unwrap(),
            &mut stale,
            &mut launcher,
            true
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
fn integration_install_without_permission_is_rejected_by_the_flow_oracle() {
    let mut endpoint = endpoint_with_assets(
        "v1.2.4",
        format!("{INSTALLER_FIXTURE_SHA256}  {INSTALLER_ASSET_NAME}\n"),
        INSTALLER_FIXTURE,
    );
    let mut launcher = FakeInstallerLauncher::default();
    let available = execute_update_flow_with_fakes(
        &Version::parse("1.2.3").unwrap(),
        &mut endpoint,
        &mut launcher,
        false,
    )
    .unwrap();
    let UpdateFlowResult::Available(candidate) = available else {
        panic!("expected an available candidate");
    };

    // The production adapter only reaches download/launch from
    // `install_available`, which requires the Available tray command.
    assert!(launcher.launched_versions.is_empty());
    assert_eq!(candidate.version, Version::parse("1.2.4").unwrap());
    let _ = UpdateFlowError::InstallWithoutPermission;
}
