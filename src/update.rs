//! Release-update protocol independent of Win32 and HTTP.
//!
//! This module accepts only published stable vMAJOR.MINOR.PATCH releases and
//! the two fixed asset names produced by the release pipeline. The platform
//! adapter supplies decoded GitHub data and performs the actual I/O.

use std::{cmp::Ordering, fmt};

pub const DEFAULT_REPOSITORY: &str = "systemexe-research-and-development/run-dog";
pub const INSTALLER_ASSET_NAME: &str = "RunDog-Setup-x64.exe";
pub const CHECKSUM_ASSET_NAME: &str = "RunDog-Setup-x64.exe.sha256";

/// A GitHub owner/repository slug accepted by the update protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateRepository(String);

impl UpdateRepository {
    /// Validates a release repository identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, UpdateProtocolError> {
        let value = value.into();
        if is_valid_repository(&value) {
            Ok(Self(value))
        } else {
            Err(UpdateProtocolError::InvalidRepository)
        }
    }

    /// Resolves the repository embedded at release-build time.
    ///
    /// The fallback keeps local developer builds usable. CI sets
    /// RUN_DOG_UPDATE_REPOSITORY to github.repository, avoiding a hard-coded
    /// fork or checkout path in distributed binaries.
    #[must_use]
    pub fn from_build_config() -> Self {
        let configured = option_env!("RUN_DOG_UPDATE_REPOSITORY").unwrap_or(DEFAULT_REPOSITORY);
        Self::new(configured).unwrap_or_else(|_| {
            Self::new(DEFAULT_REPOSITORY)
                .expect("the built-in RunDog update repository must be valid")
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable three-component release version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Version([u32; 3]);

impl Version {
    /// Parses 1.2.3 or the GitHub tag spelling v1.2.3.
    pub fn parse(value: &str) -> Result<Self, UpdateProtocolError> {
        let value = value.strip_prefix('v').unwrap_or(value);
        let mut parts = value.split('.');
        let mut parsed = [0_u32; 3];

        for slot in &mut parsed {
            let Some(part) = parts.next() else {
                return Err(UpdateProtocolError::InvalidVersion);
            };
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(UpdateProtocolError::InvalidVersion);
            }
            *slot = part
                .parse()
                .map_err(|_| UpdateProtocolError::InvalidVersion)?;
        }

        if parts.next().is_some() {
            return Err(UpdateProtocolError::InvalidVersion);
        }

        Ok(Self(parsed))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

/// The small part of GitHub release metadata needed by the update protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

/// A named browser-download asset in a GitHub release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// An update that passed release metadata validation but has not been
/// downloaded or launched yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateCandidate {
    pub version: Version,
    pub installer_url: String,
    pub checksum_url: String,
}

/// Protocol failures intentionally have no permissive fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateProtocolError {
    InvalidRepository,
    InvalidVersion,
    MissingInstallerAsset,
    MissingChecksumAsset,
    DuplicateInstallerAsset,
    DuplicateChecksumAsset,
    InvalidAssetUrl,
    InvalidChecksum,
    MissingChecksumEntry,
    DuplicateChecksumEntry,
}

impl fmt::Display for UpdateProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRepository => "invalid update repository",
            Self::InvalidVersion => "release version is not a stable MAJOR.MINOR.PATCH version",
            Self::MissingInstallerAsset => "release has no RunDog installer asset",
            Self::MissingChecksumAsset => "release has no installer checksum asset",
            Self::DuplicateInstallerAsset => "release has duplicate RunDog installer assets",
            Self::DuplicateChecksumAsset => "release has duplicate installer checksum assets",
            Self::InvalidAssetUrl => {
                "release asset URL is outside the configured GitHub repository"
            }
            Self::InvalidChecksum => "release checksum is invalid",
            Self::MissingChecksumEntry => "checksum file has no installer entry",
            Self::DuplicateChecksumEntry => "checksum file has duplicate installer entries",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for UpdateProtocolError {}

/// Selects only a strictly newer, published stable release with the required
/// pair of same-repository assets.
pub fn select_update(
    repository: &UpdateRepository,
    current_version: &Version,
    release: &Release,
) -> Result<Option<UpdateCandidate>, UpdateProtocolError> {
    if release.draft || release.prerelease {
        return Ok(None);
    }

    let version = Version::parse(&release.tag_name)?;
    if version <= *current_version {
        return Ok(None);
    }

    let installer = required_asset(
        &release.assets,
        INSTALLER_ASSET_NAME,
        UpdateProtocolError::MissingInstallerAsset,
        UpdateProtocolError::DuplicateInstallerAsset,
    )?;
    let checksum = required_asset(
        &release.assets,
        CHECKSUM_ASSET_NAME,
        UpdateProtocolError::MissingChecksumAsset,
        UpdateProtocolError::DuplicateChecksumAsset,
    )?;

    validate_release_asset_url(
        repository,
        &installer.browser_download_url,
        &release.tag_name,
        INSTALLER_ASSET_NAME,
    )?;
    validate_release_asset_url(
        repository,
        &checksum.browser_download_url,
        &release.tag_name,
        CHECKSUM_ASSET_NAME,
    )?;

    Ok(Some(UpdateCandidate {
        version,
        installer_url: installer.browser_download_url.clone(),
        checksum_url: checksum.browser_download_url.clone(),
    }))
}

/// Extracts the SHA-256 entry for the fixed installer name from a conventional
/// sha256sum-style manifest.
pub fn parse_checksum_manifest(manifest: &str) -> Result<String, UpdateProtocolError> {
    let mut checksum = None;

    for line in manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let mut fields = line.split_ascii_whitespace();
        let Some(digest) = fields.next() else {
            continue;
        };
        let Some(raw_filename) = fields.next() else {
            continue;
        };
        let filename = raw_filename.strip_prefix('*').unwrap_or(raw_filename);

        if filename != INSTALLER_ASSET_NAME {
            continue;
        }
        if fields.next().is_some() {
            return Err(UpdateProtocolError::InvalidChecksum);
        }
        if !is_sha256(digest) {
            return Err(UpdateProtocolError::InvalidChecksum);
        }
        if checksum.replace(digest.to_ascii_lowercase()).is_some() {
            return Err(UpdateProtocolError::DuplicateChecksumEntry);
        }
    }

    checksum.ok_or(UpdateProtocolError::MissingChecksumEntry)
}

/// Rechecks an API-supplied asset URL before it is handed to the network
/// adapter. The adapter then uses only this GitHub release-download route.
pub fn github_release_download_path(
    repository: &UpdateRepository,
    url: &str,
    asset_name: &str,
) -> Result<String, UpdateProtocolError> {
    validate_asset_url(repository, url, asset_name)?;
    url.strip_prefix("https://github.com")
        .map(str::to_owned)
        .ok_or(UpdateProtocolError::InvalidAssetUrl)
}

fn is_valid_repository(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !repository.is_empty()
        && !repository.contains('/')
        && owner
            .bytes()
            .chain(repository.bytes())
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn required_asset<'a>(
    assets: &'a [ReleaseAsset],
    name: &str,
    missing_error: UpdateProtocolError,
    duplicate_error: UpdateProtocolError,
) -> Result<&'a ReleaseAsset, UpdateProtocolError> {
    let mut matching_assets = assets.iter().filter(|asset| asset.name == name);
    let asset = matching_assets.next().ok_or(missing_error)?;
    if matching_assets.next().is_some() {
        return Err(duplicate_error);
    }
    Ok(asset)
}

fn validate_release_asset_url(
    repository: &UpdateRepository,
    url: &str,
    tag_name: &str,
    asset_name: &str,
) -> Result<(), UpdateProtocolError> {
    validate_asset_url(repository, url, asset_name)?;
    let expected = format!(
        "https://github.com/{}/releases/download/{tag_name}/{asset_name}",
        repository.as_str()
    );
    if url != expected {
        return Err(UpdateProtocolError::InvalidAssetUrl);
    }
    Ok(())
}

fn validate_asset_url(
    repository: &UpdateRepository,
    url: &str,
    asset_name: &str,
) -> Result<(), UpdateProtocolError> {
    let prefix = format!(
        "https://github.com/{}/releases/download/",
        repository.as_str()
    );
    let Some(path) = url.strip_prefix(&prefix) else {
        return Err(UpdateProtocolError::InvalidAssetUrl);
    };
    let expected_suffix = format!("/{asset_name}");

    if path.is_empty()
        || !path.ends_with(&expected_suffix)
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
    {
        return Err(UpdateProtocolError::InvalidAssetUrl);
    }

    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{
        github_release_download_path, parse_checksum_manifest, select_update, Release,
        ReleaseAsset, UpdateProtocolError, UpdateRepository, Version, CHECKSUM_ASSET_NAME,
        INSTALLER_ASSET_NAME,
    };
    use proptest::{
        prelude::*,
        test_runner::{Config as ProptestConfig, FileFailurePersistence, RngSeed},
    };

    fn repository() -> UpdateRepository {
        UpdateRepository::new("example-org/run-dog").expect("valid test repository")
    }

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft: false,
            prerelease: false,
            assets: vec![
                ReleaseAsset {
                    name: INSTALLER_ASSET_NAME.to_owned(),
                    browser_download_url: format!(
                        "https://github.com/example-org/run-dog/releases/download/{tag}/{INSTALLER_ASSET_NAME}"
                    ),
                },
                ReleaseAsset {
                    name: CHECKSUM_ASSET_NAME.to_owned(),
                    browser_download_url: format!(
                        "https://github.com/example-org/run-dog/releases/download/{tag}/{CHECKSUM_ASSET_NAME}"
                    ),
                },
            ],
        }
    }

    #[test]
    fn c2_release_selection_accepts_only_a_strictly_newer_complete_stable_release() {
        let current = Version::parse("1.2.3").unwrap();

        assert!(select_update(&repository(), &current, &release("v1.2.4"))
            .unwrap()
            .is_some());
        assert!(select_update(&repository(), &current, &release("v1.2.3"))
            .unwrap()
            .is_none());
        assert!(select_update(&repository(), &current, &release("v1.2.2"))
            .unwrap()
            .is_none());

        let mut prerelease = release("v1.2.4");
        prerelease.prerelease = true;
        assert!(select_update(&repository(), &current, &prerelease)
            .unwrap()
            .is_none());

        let mut missing_asset = release("v1.2.4");
        missing_asset.assets.pop();
        assert_eq!(
            select_update(&repository(), &current, &missing_asset),
            Err(UpdateProtocolError::MissingChecksumAsset)
        );

        let mut duplicate_asset = release("v1.2.4");
        duplicate_asset
            .assets
            .push(duplicate_asset.assets[0].clone());
        assert_eq!(
            select_update(&repository(), &current, &duplicate_asset),
            Err(UpdateProtocolError::DuplicateInstallerAsset)
        );
    }

    #[test]
    fn c2_release_selection_rejects_cross_repository_and_non_stable_inputs() {
        let current = Version::parse("1.2.3").unwrap();
        let mut cross_repository = release("v1.2.4");
        cross_repository.assets[0].browser_download_url =
            "https://example.invalid/RunDog-Setup-x64.exe".to_owned();
        assert_eq!(
            select_update(&repository(), &current, &cross_repository),
            Err(UpdateProtocolError::InvalidAssetUrl)
        );
        assert_eq!(
            Version::parse("v1.2.3-rc.1"),
            Err(UpdateProtocolError::InvalidVersion)
        );
        for invalid_version in ["1..3", "1.02.3", "1.a.3", "1.2", "1.2.3.4"] {
            assert_eq!(
                Version::parse(invalid_version),
                Err(UpdateProtocolError::InvalidVersion)
            );
        }
        assert_eq!(
            UpdateRepository::new("owner/too/many"),
            Err(UpdateProtocolError::InvalidRepository)
        );
        for invalid_repository in [
            "owner",
            "/repo",
            "owner/",
            "owner/repo name",
            "owner/repo\\name",
        ] {
            assert_eq!(
                UpdateRepository::new(invalid_repository),
                Err(UpdateProtocolError::InvalidRepository)
            );
        }
        assert_eq!(
            UpdateRepository::new("owner_1/repo.2").map(|value| value.as_str().to_owned()),
            Ok("owner_1/repo.2".to_owned())
        );

        let mut mismatched_tag = release("v1.2.4");
        mismatched_tag.assets[0].browser_download_url =
            "https://github.com/example-org/run-dog/releases/download/v9.9.9/RunDog-Setup-x64.exe"
                .to_owned();
        assert_eq!(
            select_update(&repository(), &current, &mismatched_tag),
            Err(UpdateProtocolError::InvalidAssetUrl)
        );
    }

    #[test]
    fn c2_checksum_manifest_accepts_one_installer_hash_and_rejects_ambiguous_input() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_checksum_manifest(&format!("{hash}  {INSTALLER_ASSET_NAME}\n")),
            Ok(hash.clone())
        );
        assert_eq!(
            parse_checksum_manifest(&format!("{hash} *{INSTALLER_ASSET_NAME}\n")),
            Ok(hash.clone())
        );
        assert_eq!(
            parse_checksum_manifest(&format!("not-a-hash  {INSTALLER_ASSET_NAME}\n")),
            Err(UpdateProtocolError::InvalidChecksum)
        );
        assert_eq!(
            parse_checksum_manifest(&format!("{}  {INSTALLER_ASSET_NAME}\n", "g".repeat(64))),
            Err(UpdateProtocolError::InvalidChecksum)
        );
        assert_eq!(
            parse_checksum_manifest(&format!(
                "{hash}  {INSTALLER_ASSET_NAME}\n{hash}  {INSTALLER_ASSET_NAME}\n"
            )),
            Err(UpdateProtocolError::DuplicateChecksumEntry)
        );
    }

    #[test]
    fn component_release_asset_path_preserves_repository_tag_and_name() {
        let tag = "v1.2.4";
        let url = format!(
            "https://github.com/example-org/run-dog/releases/download/{tag}/{INSTALLER_ASSET_NAME}"
        );
        assert_eq!(
            super::github_release_download_path(&repository(), &url, INSTALLER_ASSET_NAME),
            Ok(format!(
                "/example-org/run-dog/releases/download/{tag}/{INSTALLER_ASSET_NAME}"
            ))
        );
    }

    #[test]
    fn c2_release_asset_path_rejects_wrong_repository_suffix_and_escape_characters() {
        let repository = repository();
        for invalid_url in [
            format!(
                "https://github.com/other-org/run-dog/releases/download/v1.2.4/{INSTALLER_ASSET_NAME}"
            ),
            "https://github.com/example-org/run-dog/releases/download/v1.2.4/not-an-installer.exe"
                .to_owned(),
            format!(
                "https://github.com/example-org/run-dog/releases/download/v1.2.4\\escape/{INSTALLER_ASSET_NAME}"
            ),
        ] {
            assert_eq!(
                github_release_download_path(&repository, &invalid_url, INSTALLER_ASSET_NAME),
                Err(UpdateProtocolError::InvalidAssetUrl)
            );
        }
    }

    #[test]
    fn component_display_contract_preserves_versions_and_protocol_diagnostics() {
        assert_eq!(Version::parse("v1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(
            UpdateProtocolError::InvalidAssetUrl.to_string(),
            "release asset URL is outside the configured GitHub repository"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 2_048,
            rng_seed: RngSeed::Fixed(0x5EED_2026_0815_0001),
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "verification/evidence/update-pbt-counterexamples.regressions",
            ))),
            .. ProptestConfig::default()
        })]

        #[test]
        fn pbt_update_selection_never_downgrades(
            current_major in 0_u16..100,
            current_minor in 0_u16..100,
            current_patch in 0_u16..100,
            release_major in 0_u16..100,
            release_minor in 0_u16..100,
            release_patch in 0_u16..100,
        ) {
            let current = Version::parse(&format!(
                "{current_major}.{current_minor}.{current_patch}"
            )).unwrap();
            let tag = format!("v{release_major}.{release_minor}.{release_patch}");
            let selection = select_update(&repository(), &current, &release(&tag)).unwrap();
            let release_version = Version::parse(&tag).unwrap();

            prop_assert_eq!(selection.is_some(), release_version > current);
        }
    }
}
