//! Release-update protocol independent of Win32 and HTTP.
//!
//! This module accepts only published stable vMAJOR.MINOR.PATCH releases and
//! the two fixed asset names produced by the release pipeline. The platform
//! adapter supplies decoded GitHub data and performs the actual I/O.

use std::{cmp::Ordering, fmt};

pub const DEFAULT_REPOSITORY: &str = "tsuyoshi-otake/run-dog";
pub const INSTALLER_ASSET_NAME: &str = "RunDog-Setup-x64.exe";
pub const CHECKSUM_ASSET_NAME: &str = "RunDog-Setup-x64.exe.sha256";

/// Wall-clock time between automatic update checks while RunDog runs.
pub const UPDATE_CHECK_INTERVAL_MS: u64 = 60 * 60 * 1_000;
/// Upper bound of the random delay added to every automatic check, so PCs
/// behind one address do not query GitHub in the same second.
pub const UPDATE_CHECK_MAX_JITTER_MS: u64 = 5 * 60 * 1_000;
/// First retry after a failed check or install; doubles per consecutive
/// failure up to `UPDATE_CHECK_INTERVAL_MS`.
pub const UPDATE_RETRY_BASE_MS: u64 = 5 * 60 * 1_000;
/// Longest server-requested wait honored before checking again.
pub const UPDATE_SERVER_RETRY_MAX_MS: u64 = 6 * 60 * 60 * 1_000;

/// Who started an update check. Only automatic checks install by themselves;
/// a manual check reports its result and leaves Install to the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateCheckOrigin {
    Automatic,
    Manual,
}

/// Terminal result of one update check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateCheckOutcome {
    Current,
    Available(Version),
    /// `retry_not_before_ms` is GitHub's `Retry-After` / rate-limit reset as
    /// Unix milliseconds, when the response carried one.
    Failed {
        retry_not_before_ms: Option<u64>,
    },
}

/// What the adapter does after a check completes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateCheckDecision {
    /// Start the verified download-and-install path.
    pub install: bool,
    /// Show the result balloon. Manual checks report every result; automatic
    /// checks announce each newly available version once.
    pub announce: bool,
}

/// Wall-clock policy for update checks while RunDog runs.
///
/// The first automatic check is due at startup. Every completion (Current,
/// Available, or Failed; automatic or manual) reschedules: success waits the
/// interval, a failed check or install backs off from `UPDATE_RETRY_BASE_MS`
/// and never retries before a server-provided time. Each delay adds the
/// caller's jitter, capped at `UPDATE_CHECK_MAX_JITTER_MS`. The setting is
/// read at completion, so turning auto-update off while a request is in flight
/// prevents that install. After `cancel` (exit) nothing is due and nothing
/// installs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpdateSchedule {
    next_due_ms: u64,
    /// When `next_due_ms` was set; a clock now earlier than this moved
    /// backwards, and the check is due instead of waiting out the jump.
    scheduled_at_ms: u64,
    in_flight: Option<UpdateCheckOrigin>,
    consecutive_failures: u32,
    announced: Option<Version>,
    cancelled: bool,
}

impl UpdateSchedule {
    /// Whether the adapter should start an automatic check now.
    #[must_use]
    pub fn is_due(&self, now_ms: u64) -> bool {
        !self.cancelled
            && self.in_flight.is_none()
            && (now_ms >= self.next_due_ms || now_ms < self.scheduled_at_ms)
    }

    /// Records a check the adapter actually started.
    pub fn begin(&mut self, origin: UpdateCheckOrigin) {
        if !self.cancelled {
            self.in_flight = Some(origin);
        }
    }

    /// Whether a started check still waits for `complete`. A result arriving
    /// without one (a duplicate or late completion) must be ignored.
    #[must_use]
    pub fn awaiting_result(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Consumes the in-flight check, reschedules, and decides install and
    /// announcement.
    #[must_use]
    pub fn complete(
        &mut self,
        now_ms: u64,
        outcome: UpdateCheckOutcome,
        setting_enabled: bool,
        jitter_ms: u64,
    ) -> UpdateCheckDecision {
        let origin = self.in_flight.take();
        if self.cancelled {
            return UpdateCheckDecision::default();
        }
        let manual = origin == Some(UpdateCheckOrigin::Manual);
        match outcome {
            UpdateCheckOutcome::Current => {
                self.succeeded(now_ms, jitter_ms);
                UpdateCheckDecision {
                    install: false,
                    announce: manual,
                }
            }
            UpdateCheckOutcome::Available(version) => {
                self.succeeded(now_ms, jitter_ms);
                let new_version = self.announced.as_ref() != Some(&version);
                self.announced = Some(version);
                UpdateCheckDecision {
                    install: origin == Some(UpdateCheckOrigin::Automatic) && setting_enabled,
                    announce: manual || new_version,
                }
            }
            UpdateCheckOutcome::Failed {
                retry_not_before_ms,
            } => {
                self.failed(now_ms, retry_not_before_ms, jitter_ms);
                UpdateCheckDecision {
                    install: false,
                    announce: manual,
                }
            }
        }
    }

    /// A verified install could not be started; retry the check with backoff.
    pub fn install_failed(&mut self, now_ms: u64, jitter_ms: u64) {
        if !self.cancelled {
            self.failed(now_ms, None, jitter_ms);
        }
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.in_flight = None;
    }

    fn succeeded(&mut self, now_ms: u64, jitter_ms: u64) {
        self.consecutive_failures = 0;
        self.schedule(
            now_ms,
            now_ms.saturating_add(UPDATE_CHECK_INTERVAL_MS),
            jitter_ms,
        );
    }

    fn failed(&mut self, now_ms: u64, retry_not_before_ms: Option<u64>, jitter_ms: u64) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let doublings = (self.consecutive_failures - 1).min(16);
        let backoff = UPDATE_RETRY_BASE_MS
            .saturating_mul(1 << doublings)
            .min(UPDATE_CHECK_INTERVAL_MS);
        let retry_at = now_ms
            .saturating_add(backoff)
            .max(retry_not_before_ms.unwrap_or(0));
        self.schedule(now_ms, retry_at, jitter_ms);
    }

    fn schedule(&mut self, now_ms: u64, due_ms: u64, jitter_ms: u64) {
        self.scheduled_at_ms = now_ms;
        self.next_due_ms = due_ms.saturating_add(jitter_ms.min(UPDATE_CHECK_MAX_JITTER_MS));
    }
}

/// GitHub's earliest retry time for a rate-limited API response, as Unix
/// milliseconds. Only 403/429 count: `Retry-After` seconds win, otherwise an
/// exhausted quota (`X-RateLimit-Remaining: 0`) waits for
/// `X-RateLimit-Reset`. The wait is capped at `UPDATE_SERVER_RETRY_MAX_MS`.
#[must_use]
pub fn github_retry_not_before_ms(
    now_ms: u64,
    status: u32,
    retry_after: Option<&str>,
    ratelimit_remaining: Option<&str>,
    ratelimit_reset: Option<&str>,
) -> Option<u64> {
    if !matches!(status, 403 | 429) {
        return None;
    }
    let latest = now_ms.saturating_add(UPDATE_SERVER_RETRY_MAX_MS);
    let seconds = |value: Option<&str>| value.and_then(|value| value.trim().parse::<u64>().ok());
    if let Some(wait) = seconds(retry_after) {
        return Some(
            now_ms
                .saturating_add(wait.saturating_mul(1_000))
                .min(latest),
        );
    }
    if seconds(ratelimit_remaining) != Some(0) {
        return None;
    }
    let reset_ms = seconds(ratelimit_reset)?.saturating_mul(1_000);
    (reset_ms > now_ms).then_some(reset_ms.min(latest))
}

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

#[cfg(test)]
mod schedule_tests {
    use super::{
        github_retry_not_before_ms, UpdateCheckDecision, UpdateCheckOrigin, UpdateCheckOutcome,
        UpdateSchedule, Version, UPDATE_CHECK_INTERVAL_MS, UPDATE_CHECK_MAX_JITTER_MS,
        UPDATE_RETRY_BASE_MS, UPDATE_SERVER_RETRY_MAX_MS,
    };
    use proptest::{
        prelude::*,
        test_runner::{Config as ProptestConfig, FileFailurePersistence, RngSeed},
    };

    const T0: u64 = 1_793_410_000_000;
    const MINUTE: u64 = 60_000;

    fn version(value: &str) -> Version {
        Version::parse(value).unwrap()
    }

    fn available(value: &str) -> UpdateCheckOutcome {
        UpdateCheckOutcome::Available(version(value))
    }

    fn failed() -> UpdateCheckOutcome {
        UpdateCheckOutcome::Failed {
            retry_not_before_ms: None,
        }
    }

    fn run(
        schedule: &mut UpdateSchedule,
        origin: UpdateCheckOrigin,
        now_ms: u64,
        outcome: UpdateCheckOutcome,
        setting_enabled: bool,
    ) -> UpdateCheckDecision {
        schedule.begin(origin);
        schedule.complete(now_ms, outcome, setting_enabled, 0)
    }

    #[test]
    fn c2_schedule_first_check_is_due_at_startup_and_not_while_in_flight() {
        let mut schedule = UpdateSchedule::default();
        assert!(schedule.is_due(T0));
        assert!(!schedule.awaiting_result());
        schedule.begin(UpdateCheckOrigin::Automatic);
        assert!(schedule.awaiting_result());
        assert!(!schedule.is_due(T0));
        assert!(!schedule.is_due(T0 + 10 * UPDATE_CHECK_INTERVAL_MS));

        let _ = schedule.complete(T0, UpdateCheckOutcome::Current, true, 0);
        assert!(!schedule.awaiting_result());
        schedule.begin(UpdateCheckOrigin::Manual);
        schedule.cancel();
        assert!(!schedule.awaiting_result());
    }

    #[test]
    fn c2_schedule_rechecks_after_the_interval_plus_jitter_while_running() {
        let mut schedule = UpdateSchedule::default();
        schedule.begin(UpdateCheckOrigin::Automatic);
        let _ = schedule.complete(T0, UpdateCheckOutcome::Current, true, 1_234);

        assert!(!schedule.is_due(T0 + UPDATE_CHECK_INTERVAL_MS + 1_233));
        assert!(schedule.is_due(T0 + UPDATE_CHECK_INTERVAL_MS + 1_234));

        // Jitter is bounded even if the adapter passes a larger value.
        schedule.begin(UpdateCheckOrigin::Automatic);
        let _ = schedule.complete(T0, UpdateCheckOutcome::Current, true, u64::MAX);
        assert!(schedule.is_due(T0 + UPDATE_CHECK_INTERVAL_MS + UPDATE_CHECK_MAX_JITTER_MS));
    }

    #[test]
    fn c2_schedule_backs_off_after_failures_caps_at_the_interval_and_resets() {
        let mut schedule = UpdateSchedule::default();
        let mut now = T0;
        for minutes in [5, 10, 20, 40, 60, 60, 60] {
            let _ = run(
                &mut schedule,
                UpdateCheckOrigin::Automatic,
                now,
                failed(),
                true,
            );
            assert!(
                !schedule.is_due(now + minutes * MINUTE - 1),
                "{minutes} min"
            );
            assert!(schedule.is_due(now + minutes * MINUTE), "{minutes} min");
            now += minutes * MINUTE;
        }
        assert_eq!(UPDATE_RETRY_BASE_MS, 5 * MINUTE);
        assert_eq!(UPDATE_CHECK_INTERVAL_MS, 60 * MINUTE);

        let _ = run(
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            now,
            UpdateCheckOutcome::Current,
            true,
        );
        let _ = run(
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            now,
            failed(),
            true,
        );
        assert!(
            schedule.is_due(now + 5 * MINUTE),
            "success resets the backoff"
        );
    }

    #[test]
    fn c2_schedule_waits_for_a_server_retry_time_but_never_less_than_backoff() {
        let mut schedule = UpdateSchedule::default();
        schedule.begin(UpdateCheckOrigin::Automatic);
        let _ = schedule.complete(
            T0,
            UpdateCheckOutcome::Failed {
                retry_not_before_ms: Some(T0 + 50 * MINUTE),
            },
            true,
            0,
        );
        assert!(!schedule.is_due(T0 + 50 * MINUTE - 1));
        assert!(schedule.is_due(T0 + 50 * MINUTE));

        schedule.begin(UpdateCheckOrigin::Automatic);
        let _ = schedule.complete(
            T0,
            UpdateCheckOutcome::Failed {
                retry_not_before_ms: Some(T0 + MINUTE),
            },
            true,
            0,
        );
        assert!(
            !schedule.is_due(T0 + 10 * MINUTE - 1),
            "second failure backs off 10 min"
        );
        assert!(schedule.is_due(T0 + 10 * MINUTE));
    }

    #[test]
    fn c2_schedule_installs_only_automatic_available_updates_with_the_setting_on() {
        let mut schedule = UpdateSchedule::default();
        assert_eq!(
            run(
                &mut schedule,
                UpdateCheckOrigin::Automatic,
                T0,
                available("1.2.4"),
                true
            ),
            UpdateCheckDecision {
                install: true,
                announce: true
            }
        );
        assert_eq!(
            run(
                &mut schedule,
                UpdateCheckOrigin::Manual,
                T0,
                available("1.2.4"),
                true
            ),
            UpdateCheckDecision {
                install: false,
                announce: true
            },
            "a manual check keeps the Install menu item"
        );

        let mut off = UpdateSchedule::default();
        assert_eq!(
            run(
                &mut off,
                UpdateCheckOrigin::Automatic,
                T0,
                available("1.2.4"),
                false
            ),
            UpdateCheckDecision {
                install: false,
                announce: true
            }
        );
        assert_eq!(
            run(
                &mut off,
                UpdateCheckOrigin::Automatic,
                T0,
                available("1.2.4"),
                false
            ),
            UpdateCheckDecision::default(),
            "the same version is announced once, not every hour"
        );
        assert_eq!(
            run(
                &mut off,
                UpdateCheckOrigin::Automatic,
                T0,
                available("1.2.5"),
                false
            ),
            UpdateCheckDecision {
                install: false,
                announce: true
            }
        );
    }

    #[test]
    fn c2_schedule_reports_every_manual_result_and_no_automatic_non_update() {
        let mut schedule = UpdateSchedule::default();
        for outcome in [UpdateCheckOutcome::Current, failed()] {
            assert_eq!(
                run(
                    &mut schedule,
                    UpdateCheckOrigin::Automatic,
                    T0,
                    outcome.clone(),
                    true
                ),
                UpdateCheckDecision::default()
            );
            assert_eq!(
                run(&mut schedule, UpdateCheckOrigin::Manual, T0, outcome, true),
                UpdateCheckDecision {
                    install: false,
                    announce: true
                }
            );
        }
    }

    #[test]
    fn c2_schedule_retries_a_failed_install_with_backoff() {
        let mut schedule = UpdateSchedule::default();
        let decision = run(
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            T0,
            available("1.2.4"),
            true,
        );
        assert!(decision.install);
        assert!(!schedule.is_due(T0 + 30 * MINUTE));

        schedule.install_failed(T0 + MINUTE, 0);
        assert!(!schedule.is_due(T0 + 6 * MINUTE - 1));
        assert!(schedule.is_due(T0 + 6 * MINUTE));

        assert!(
            run(
                &mut schedule,
                UpdateCheckOrigin::Automatic,
                T0 + 6 * MINUTE,
                available("1.2.4"),
                true
            )
            .install,
            "the retry installs the same version again"
        );
    }

    #[test]
    fn c2_schedule_after_exit_never_checks_or_installs() {
        let mut schedule = UpdateSchedule::default();
        schedule.begin(UpdateCheckOrigin::Automatic);
        schedule.cancel();
        assert_eq!(
            schedule.complete(T0, available("1.2.4"), true, 0),
            UpdateCheckDecision::default()
        );
        assert!(!schedule.is_due(T0 + 10 * UPDATE_CHECK_INTERVAL_MS));
        schedule.install_failed(T0, 0);
        schedule.begin(UpdateCheckOrigin::Automatic);
        assert!(!schedule.is_due(T0 + 10 * UPDATE_CHECK_INTERVAL_MS));
        assert_eq!(
            schedule.complete(T0, available("1.2.4"), true, 0),
            UpdateCheckDecision::default()
        );
    }

    #[test]
    fn c2_schedule_treats_a_clock_moved_backwards_as_due() {
        let mut schedule = UpdateSchedule::default();
        let _ = run(
            &mut schedule,
            UpdateCheckOrigin::Automatic,
            T0,
            UpdateCheckOutcome::Current,
            true,
        );
        assert!(!schedule.is_due(T0 + MINUTE));
        assert!(schedule.is_due(T0 - 1));
    }

    #[test]
    fn c2_github_retry_hint_honors_retry_after_and_exhausted_rate_limit_reset() {
        let now = T0;
        assert_eq!(
            github_retry_not_before_ms(now, 403, Some("120"), None, None),
            Some(now + 120_000)
        );
        assert_eq!(
            github_retry_not_before_ms(now, 429, Some(" 7 "), Some("0"), Some("1793413478")),
            Some(now + 7_000),
            "Retry-After wins over the reset time"
        );
        assert_eq!(
            github_retry_not_before_ms(now, 403, None, Some("0"), Some("1793413478")),
            Some(1_793_413_478_000)
        );
        assert_eq!(
            github_retry_not_before_ms(now, 403, None, Some("5"), Some("1793413478")),
            None,
            "a 403 with quota left is not a rate limit"
        );
        assert_eq!(
            github_retry_not_before_ms(now, 403, None, Some("0"), Some("1000")),
            None,
            "a reset in the past adds no wait"
        );
        assert_eq!(
            github_retry_not_before_ms(now, 200, Some("120"), Some("0"), None),
            None
        );
        assert_eq!(
            github_retry_not_before_ms(now, 503, Some("120"), None, None),
            None
        );
        assert_eq!(
            github_retry_not_before_ms(now, 429, Some("soon"), None, None),
            None
        );
        assert_eq!(
            github_retry_not_before_ms(now, 429, Some("999999999"), None, None),
            Some(now + UPDATE_SERVER_RETRY_MAX_MS),
            "a server wait is bounded"
        );
    }

    #[derive(Clone, Debug)]
    enum Step {
        Check {
            manual: bool,
            outcome: u8,
            setting: bool,
            jitter: u64,
            elapsed: u64,
        },
        InstallFailed {
            elapsed: u64,
        },
        Exit,
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            8 => (
                any::<bool>(),
                0_u8..4,
                any::<bool>(),
                any::<u64>(),
                0_u64..3 * UPDATE_CHECK_INTERVAL_MS,
            )
                .prop_map(|(manual, outcome, setting, jitter, elapsed)| Step::Check {
                    manual,
                    outcome,
                    setting,
                    jitter,
                    elapsed,
                }),
            2 => (0_u64..UPDATE_CHECK_INTERVAL_MS)
                .prop_map(|elapsed| Step::InstallFailed { elapsed }),
            1 => Just(Step::Exit),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 1_024,
            rng_seed: RngSeed::Fixed(0x5EED_2026_1031_0110),
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "verification/evidence/update-schedule-pbt-counterexamples.regressions",
            ))),
            .. ProptestConfig::default()
        })]

        /// Stateful model check: every completion leaves the schedule idle with
        /// a bounded next check, and only automatic, enabled, available checks
        /// before exit may install.
        #[test]
        fn pbt_schedule_always_reschedules_and_installs_only_when_allowed(
            steps in proptest::collection::vec(step(), 1..40)
        ) {
            let mut schedule = UpdateSchedule::default();
            let mut now = T0;
            let mut exited = false;
            for step in steps {
                match step {
                    Step::Check { manual, outcome, setting, jitter, elapsed } => {
                        now += elapsed;
                        let origin = if manual {
                            UpdateCheckOrigin::Manual
                        } else {
                            UpdateCheckOrigin::Automatic
                        };
                        let outcome = match outcome {
                            0 => UpdateCheckOutcome::Current,
                            1 => available("1.2.4"),
                            2 => failed(),
                            _ => UpdateCheckOutcome::Failed {
                                retry_not_before_ms: Some(now + 7 * MINUTE),
                            },
                        };
                        let is_available = matches!(outcome, UpdateCheckOutcome::Available(_));
                        schedule.begin(origin);
                        let decision = schedule.complete(now, outcome, setting, jitter);
                        prop_assert_eq!(
                            decision.install,
                            !exited && !manual && setting && is_available
                        );
                        if exited {
                            prop_assert_eq!(decision, UpdateCheckDecision::default());
                            prop_assert!(!schedule.is_due(now + 10 * UPDATE_CHECK_INTERVAL_MS));
                        } else {
                            prop_assert!(
                                schedule.is_due(
                                    now + UPDATE_CHECK_INTERVAL_MS + UPDATE_CHECK_MAX_JITTER_MS
                                ),
                                "next check is never later than interval + jitter"
                            );
                        }
                    }
                    Step::InstallFailed { elapsed } => {
                        now += elapsed;
                        schedule.install_failed(now, 0);
                        if !exited {
                            prop_assert!(schedule.is_due(now + UPDATE_CHECK_INTERVAL_MS));
                        }
                    }
                    Step::Exit => {
                        schedule.cancel();
                        exited = true;
                    }
                }
            }
        }
    }
}
