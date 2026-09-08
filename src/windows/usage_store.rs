//! LOCALAPPDATA usage durable store. Never writes into Claude or Codex trees.

use std::path::{Path, PathBuf};

use super::usage_store_path::{forbidden, PinnedDirectory};
use crate::core::{
    load_usage_state, persist_usage_state, GenerationBlobs, LoadStatus, PersistStatus, UsageState,
    MAX_PRIOR_GENERATIONS,
};

const CURRENT_HEADER: &str = "rundog-usage-current-1";
const CURRENT_NAME: &str = "current";
const BLOB_PREFIX: &str = "g";
const BLOB_SUFFIX: &str = ".state";

#[derive(Clone, Debug)]
pub struct FileUsageStore {
    root: PathBuf,
    denied: Vec<PathBuf>,
}

impl FileUsageStore {
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self {
            root,
            denied: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_provider_roots(mut self, claude: &Path, codex: &Path) -> Self {
        self.denied = vec![claude.to_path_buf(), codex.to_path_buf()];
        self
    }

    #[must_use]
    pub fn production_root() -> Option<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(local).join("RunDog").join("usage"))
    }

    #[must_use]
    pub fn production(claude: &Path, codex: &Path) -> Option<Self> {
        let current = Self::production_root()?;
        let local = std::env::var_os("LOCALAPPDATA")?;
        let legacy = PathBuf::from(local)
            .join("SystemExe")
            .join("RunDog")
            .join("usage");
        let denied = vec![claude.to_path_buf(), codex.to_path_buf()];
        Some(Self {
            root: select_production_root(&legacy, &current, &denied),
            denied,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn persist(&mut self, state: &UsageState) -> PersistStatus {
        let Some(_guard) = PinnedDirectory::open(&self.root, true, &self.denied, false) else {
            return PersistStatus::Failed;
        };
        let status = persist_usage_state(self, state);
        if let PersistStatus::Applied { generation } = status {
            self.cleanup_after_publish(generation);
        }
        status
    }

    #[must_use]
    pub fn load(&self) -> LoadStatus {
        let Some(_guard) = PinnedDirectory::open(&self.root, false, &self.denied, false) else {
            return LoadStatus::Missing;
        };
        let status = load_usage_state(self);
        match &status {
            LoadStatus::Loaded(state) | LoadStatus::RecoveredPrior { state, .. } => {
                self.cleanup_after_publish(state.generation);
            }
            LoadStatus::Missing => {}
        }
        status
    }

    #[must_use]
    pub fn refuses_provider_roots(&self, claude_dir: &Path, codex_home: &Path) -> bool {
        forbidden(
            &self.root,
            &[claude_dir.to_path_buf(), codex_home.to_path_buf()],
        )
    }

    fn cleanup_after_publish(&self, generation: u64) {
        let Some(guard) = PinnedDirectory::open(&self.root, false, &self.denied, false) else {
            return;
        };
        let oldest_retained = generation.saturating_sub(MAX_PRIOR_GENERATIONS);
        let Some(entries) = guard.entries() else {
            return;
        };
        for path in entries {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let old_generation =
                parse_blob_generation(name).is_some_and(|candidate| candidate < oldest_retained);
            if old_generation || name.ends_with(".tmp") {
                let _ = guard.remove_file(&path);
            }
        }
    }
}

impl GenerationBlobs for FileUsageStore {
    fn write_blob(&mut self, generation: u64, bytes: &[u8]) -> bool {
        let Some(guard) = PinnedDirectory::open(&self.root, true, &self.denied, false) else {
            return false;
        };
        let final_path = blob_path(&self.root, generation);
        let tmp = final_path.with_extension("state.tmp");
        guard.write_atomically(&tmp, &final_path, bytes)
    }

    fn read_blob(&self, generation: u64) -> Option<Vec<u8>> {
        let guard = PinnedDirectory::open(&self.root, false, &self.denied, false)?;
        guard.read(&blob_path(&self.root, generation))
    }

    fn write_current(&mut self, generation: u64) -> bool {
        let Some(guard) = PinnedDirectory::open(&self.root, true, &self.denied, false) else {
            return false;
        };
        let payload = format!("{CURRENT_HEADER}\ngeneration={generation}\n");
        let final_path = self.root.join(CURRENT_NAME);
        let tmp = self.root.join("current.tmp");
        guard.write_atomically(&tmp, &final_path, payload.as_bytes())
    }

    fn read_current(&self) -> Option<u64> {
        let guard = PinnedDirectory::open(&self.root, false, &self.denied, false)?;
        let text = String::from_utf8(guard.read(&self.root.join(CURRENT_NAME))?).ok()?;
        parse_current(&text)
    }

    fn highest_blob_generation(&self) -> Option<u64> {
        let guard = PinnedDirectory::open(&self.root, false, &self.denied, false)?;
        let entries = guard.entries()?;
        entries
            .into_iter()
            .filter_map(|entry| {
                let name = entry.file_name()?;
                let name = name.to_str()?;
                let stem = name.strip_prefix(BLOB_PREFIX)?.strip_suffix(BLOB_SUFFIX)?;
                stem.parse().ok()
            })
            .max()
    }
}

fn select_production_root(legacy: &Path, current: &Path, denied: &[PathBuf]) -> PathBuf {
    if real_directory(current, denied) {
        if root_has_valid_state(current, denied) {
            remove_real_directory(legacy, denied);
            return current.to_path_buf();
        }
        if directory_is_empty(current, denied) && real_directory(legacy, denied) {
            let _ = remove_empty_directory(current, denied);
            if move_directory(legacy, current, denied) {
                return current.to_path_buf();
            }
        }
        if root_has_valid_state(legacy, denied) {
            return legacy.to_path_buf();
        }
        return current.to_path_buf();
    }
    if real_directory(legacy, denied) {
        if move_directory(legacy, current, denied) {
            return current.to_path_buf();
        }
        return legacy.to_path_buf();
    }
    current.to_path_buf()
}

fn root_has_valid_state(root: &Path, denied: &[PathBuf]) -> bool {
    matches!(
        load_usage_state(&FileUsageStore {
            root: root.to_path_buf(),
            denied: denied.to_vec()
        }),
        LoadStatus::Loaded(_) | LoadStatus::RecoveredPrior { .. }
    )
}

fn real_directory(path: &Path, denied: &[PathBuf]) -> bool {
    PinnedDirectory::open(path, false, denied, false).is_some()
}

fn directory_is_empty(path: &Path, denied: &[PathBuf]) -> bool {
    let Some(guard) = PinnedDirectory::open(path, false, denied, false) else {
        return false;
    };
    guard.entries().is_some_and(|entries| entries.is_empty())
}

fn remove_empty_directory(path: &Path, denied: &[PathBuf]) -> bool {
    PinnedDirectory::open(path, false, denied, true).is_some_and(|guard| guard.remove_empty())
}

fn move_directory(from: &Path, to: &Path, denied: &[PathBuf]) -> bool {
    if forbidden(to, denied) {
        return false;
    }
    let Some(parent) = to.parent() else {
        return false;
    };
    let Some(source) = PinnedDirectory::open(from, false, denied, true) else {
        return false;
    };
    let Some(parent) = PinnedDirectory::open(parent, true, denied, false) else {
        return false;
    };
    source.rename_to(to, &parent)
}

fn remove_real_directory(path: &Path, denied: &[PathBuf]) {
    let Some(guard) = PinnedDirectory::open(path, false, denied, true) else {
        return;
    };
    let Some(entries) = guard.entries() else {
        return;
    };
    // Never recurse into attacker-supplied directories or junctions. Only
    // remove store-owned regular files; leave unknown contents untouched.
    for entry in entries {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == CURRENT_NAME || parse_blob_generation(name).is_some() || name.ends_with(".tmp") {
            let _ = guard.remove_file(&entry);
        }
    }
    let _ = guard.remove_empty();
}

fn blob_path(root: &Path, generation: u64) -> PathBuf {
    root.join(format!("{BLOB_PREFIX}{generation:016}{BLOB_SUFFIX}"))
}

fn parse_blob_generation(name: &str) -> Option<u64> {
    name.strip_prefix(BLOB_PREFIX)?
        .strip_suffix(BLOB_SUFFIX)?
        .parse()
        .ok()
}

fn parse_current(text: &str) -> Option<u64> {
    let mut lines = text.lines();
    if lines.next()? != CURRENT_HEADER {
        return None;
    }
    let value = lines.next()?.strip_prefix("generation=")?;
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_current, FileUsageStore};
    use crate::core::{PersistStatus, UsageAggregate, UsageSnapshot, UsageState};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rundog-usage-store-{label}-{nanos}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn sample() -> UsageState {
        UsageState {
            generation: 0,
            schema_version: crate::core::USAGE_STATE_SCHEMA_VERSION,
            aggregate: UsageAggregate {
                month_start: 20_260_901,
                today: 20_260_905,
                last_collected_ms: 0,
                catch_up_done: false,
                snapshot: UsageSnapshot::default(),
            },
            cursors: Vec::new(),
            claude_keys: std::collections::HashSet::new(),
        }
    }

    fn junction(link: &std::path::Path, target: &std::path::Path) {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:RUNDOG_TEST_LINK -Target $env:RUNDOG_TEST_TARGET | Out-Null"])
            .env("RUNDOG_TEST_LINK", link).env("RUNDOG_TEST_TARGET", target)
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .output().expect("create isolated junction");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn component_store_refuses_junction_root_and_ancestor_after_construction() {
        use crate::core::GenerationBlobs;
        for ancestor in [false, true] {
            let base = temp_root("junction");
            let provider = base.join(".claude");
            let target = provider.join("usage");
            let mut fixture = FileUsageStore::at(target.clone());
            assert!(matches!(
                fixture.persist(&sample()),
                PersistStatus::Applied { .. }
            ));
            fs::write(target.join("abandoned.tmp"), b"provider-sentinel").unwrap();
            let before_current = fs::read(target.join("current")).unwrap();
            let before_blob = fs::read(super::blob_path(&target, 1)).unwrap();
            let alias = base.join("alias");
            let root = if ancestor {
                alias.join("usage")
            } else {
                alias.clone()
            };
            let mut store =
                FileUsageStore::at(root).with_provider_roots(&provider, &base.join(".codex"));
            junction(&alias, if ancestor { &provider } else { &target });
            assert_eq!(store.persist(&sample()), PersistStatus::Failed);
            assert_eq!(store.load(), crate::core::LoadStatus::Missing);
            assert!(!store.write_blob(2, b"must-not-write"));
            assert!(!store.write_current(2));
            assert_eq!(store.read_current(), None);
            assert_eq!(store.read_blob(1), None);
            assert_eq!(fs::read(target.join("current")).unwrap(), before_current);
            assert_eq!(fs::read(super::blob_path(&target, 1)).unwrap(), before_blob);
            assert_eq!(
                fs::read(target.join("abandoned.tmp")).unwrap(),
                b"provider-sentinel"
            );
            assert_eq!(fs::read_dir(&target).unwrap().count(), 3);
            fs::remove_dir(&alias).unwrap();
            fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn component_store_pins_ancestors_against_replacement_and_writable_handles() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::{
            Foundation::GENERIC_WRITE,
            Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            },
        };
        let base = temp_root("pinned");
        let root = base.join("parent").join("usage");
        let guard = super::PinnedDirectory::open(&root, true, &[], false).unwrap();
        assert!(fs::rename(&root, base.join("replaced")).is_err());
        assert!(fs::rename(root.parent().unwrap(), base.join("moved-parent")).is_err());
        let writable = || {
            fs::OpenOptions::new()
                .access_mode(GENERIC_WRITE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(root.parent().unwrap())
        };
        assert!(writable().is_err());
        drop(guard);
        let writer = writable().unwrap();
        assert!(super::PinnedDirectory::open(&root, false, &[], false).is_none());
        drop(writer);
        fs::rename(&root, base.join("replaced")).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_store_handle_relative_io_survives_root_reparse_insertion() {
        use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
        use windows_sys::Win32::{
            Foundation::GENERIC_WRITE,
            Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
                FILE_SHARE_READ, FILE_SHARE_WRITE,
            },
            System::IO::DeviceIoControl,
        };
        let base = temp_root("root-reparse-race");
        let root = base.join("store");
        let provider = base.join("provider");
        fs::create_dir_all(&provider).unwrap();
        fs::write(provider.join("sentinel.tmp"), b"untouched").unwrap();
        let guard =
            super::PinnedDirectory::open(&root, true, std::slice::from_ref(&provider), false)
                .unwrap();
        let writer = fs::OpenOptions::new()
            .access_mode(GENERIC_WRITE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&root)
            .unwrap();
        let target: Vec<_> = std::ffi::OsString::from(format!(r"\??\{}", provider.display()))
            .encode_wide()
            .collect();
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&0xA0000003_u32.to_le_bytes());
        buffer.extend_from_slice(&((8 + (target.len() + 2) * 2) as u16).to_le_bytes());
        buffer.extend_from_slice(&0_u16.to_le_bytes());
        buffer.extend_from_slice(&0_u16.to_le_bytes());
        buffer.extend_from_slice(&((target.len() * 2) as u16).to_le_bytes());
        buffer.extend_from_slice(&(((target.len() + 1) * 2) as u16).to_le_bytes());
        buffer.extend_from_slice(&0_u16.to_le_bytes());
        for ch in target {
            buffer.extend_from_slice(&ch.to_le_bytes());
        }
        buffer.extend_from_slice(&[0; 4]);
        let mut returned = 0;
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    writer.as_raw_handle(),
                    0x000900A4,
                    buffer.as_ptr().cast(),
                    buffer.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
        assert_eq!(fs::read(root.join("sentinel.tmp")).unwrap(), b"untouched");
        assert!(!guard.remove_file(&root.join("sentinel.tmp")));
        assert!(!guard.write_atomically(
            &root.join("current.tmp"),
            &root.join("current"),
            b"store-only"
        ));
        assert!(guard.read(&root.join("sentinel.tmp")).is_none());
        assert!(guard.entries().unwrap().is_empty());
        assert_eq!(fs::read_dir(&provider).unwrap().count(), 1);
        assert_eq!(
            fs::read(provider.join("sentinel.tmp")).unwrap(),
            b"untouched"
        );
        let delete = [3_u8, 0, 0, 0xA0, 0, 0, 0, 0];
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    writer.as_raw_handle(),
                    0x000900AC,
                    delete.as_ptr().cast(),
                    delete.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        drop(writer);
        assert!(guard.write_atomically(
            &root.join("current.tmp"),
            &root.join("current"),
            b"store-only"
        ));
        assert_eq!(guard.read(&root.join("current")).unwrap(), b"store-only");
        drop(guard);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_store_never_truncates_temporary_or_published_hardlinks() {
        use crate::core::GenerationBlobs;
        let base = temp_root("leaf-hardlinks");
        let root = base.join("store");
        let provider = base.join(".codex");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&provider).unwrap();
        let target = provider.join("sentinel");
        fs::write(&target, b"provider-sentinel").unwrap();
        let mut store =
            FileUsageStore::at(root.clone()).with_provider_roots(&base.join(".claude"), &provider);
        for name in [
            "current.tmp",
            "current",
            "g0000000000000001.state.tmp",
            "g0000000000000001.state",
        ] {
            let link = root.join(name);
            fs::hard_link(&target, &link).unwrap();
            let applied = if name.starts_with("current") {
                store.write_current(1)
            } else {
                store.write_blob(1, b"replacement")
            };
            assert!(!applied, "accepted hardlink {name}");
            assert_eq!(fs::read(&target).unwrap(), b"provider-sentinel");
            assert!(link.exists());
            fs::remove_file(link).unwrap();
        }
        assert!(matches!(
            store.persist(&sample()),
            PersistStatus::Applied { .. }
        ));
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_store_checks_custom_provider_aliases_and_case_before_creation() {
        let base = temp_root("provider-alias");
        let provider = base.join("custom-provider");
        fs::create_dir_all(&provider).unwrap();
        let alias = base.join("provider-alias");
        junction(&alias, &provider);
        for denied in [
            alias.clone(),
            std::path::PathBuf::from(provider.to_str().unwrap().to_uppercase()),
        ] {
            let mut store = FileUsageStore::at(provider.join("must-not-create"))
                .with_provider_roots(&denied, &base.join(".codex"));
            assert_eq!(store.persist(&sample()), PersistStatus::Failed);
            assert!(!provider.join("must-not-create").exists());
        }
        fs::remove_dir(alias).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_store_migration_and_cleanup_refuse_provider_junctions() {
        let base = temp_root("migration-junction");
        let provider = base.join(".claude");
        let legacy = base.join("legacy");
        let current = base.join("current-store");
        let mut provider_fixture = FileUsageStore::at(provider.clone());
        assert!(matches!(
            provider_fixture.persist(&sample()),
            PersistStatus::Applied { .. }
        ));
        let before = fs::read(provider.join("current")).unwrap();
        let denied = [provider.clone()];
        junction(&legacy, &provider);
        assert_eq!(
            super::select_production_root(&legacy, &current, &denied),
            current
        );
        assert!(!current.exists());
        let mut store = FileUsageStore::at(current.clone());
        assert!(matches!(
            store.persist(&sample()),
            PersistStatus::Applied { .. }
        ));
        assert_eq!(
            super::select_production_root(&legacy, &current, &denied),
            current
        );
        assert_eq!(fs::read(provider.join("current")).unwrap(), before);
        assert!(legacy.exists());
        fs::remove_dir(&legacy).unwrap();
        // A configured provider root is also refused without any junction.
        assert_eq!(
            super::select_production_root(&provider, &base.join("new-store"), &denied),
            base.join("new-store")
        );
        assert!(provider.exists());
        assert!(!base.join("new-store").exists());
        // Nested junctions in a stale legacy directory are never traversed.
        fs::create_dir(&legacy).unwrap();
        junction(&legacy.join("nested"), &provider);
        assert_eq!(
            super::select_production_root(&legacy, &current, &denied),
            current
        );
        assert_eq!(fs::read(provider.join("current")).unwrap(), before);
        fs::remove_dir(legacy.join("nested")).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_recovery_then_save_preserves_generation_and_latest_state() {
        for corrupt in [false, true] {
            let root = temp_root("recovery-save");
            let mut store = FileUsageStore::at(root.clone());
            let mut state = sample();
            for tokens in [100, 200] {
                state.aggregate.snapshot.claude.month_input_tokens = tokens;
                assert!(matches!(
                    store.persist(&state),
                    PersistStatus::Applied { .. }
                ));
            }
            if corrupt {
                fs::write(root.join("current"), b"broken").unwrap();
            } else {
                fs::remove_file(root.join("current")).unwrap();
            }
            let crate::core::LoadStatus::RecoveredPrior { mut state, .. } = store.load() else {
                panic!("expected recovery");
            };
            assert_eq!(state.generation, 2);
            state.aggregate.snapshot.claude.month_input_tokens = 300;
            assert_eq!(
                store.persist(&state),
                PersistStatus::Applied { generation: 3 }
            );
            fs::remove_file(root.join("current")).unwrap();
            let crate::core::LoadStatus::RecoveredPrior { state, .. } = store.load() else {
                panic!("expected second recovery");
            };
            assert_eq!(state.generation, 3);
            assert_eq!(state.aggregate.snapshot.claude.month_input_tokens, 300);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn component_file_store_persist_and_load_catch_up_in_progress() {
        let root = temp_root("roundtrip");
        let mut store = FileUsageStore::at(root.clone());
        assert_eq!(
            store.persist(&sample()),
            PersistStatus::Applied { generation: 1 }
        );
        match store.load() {
            crate::core::LoadStatus::Loaded(state) => {
                assert_eq!(state.generation, 1);
                assert!(!state.aggregate.catch_up_done);
            }
            other => panic!("expected loaded, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn component_file_store_recovers_prior_when_latest_blob_is_gone() {
        let root = temp_root("recover");
        let mut store = FileUsageStore::at(root.clone());
        assert!(matches!(
            store.persist(&sample()),
            PersistStatus::Applied { generation: 1 }
        ));
        let mut second = sample();
        second.aggregate.catch_up_done = true;
        assert!(matches!(
            store.persist(&second),
            PersistStatus::Applied { generation: 2 }
        ));
        let latest = root.join("g0000000000000002.state");
        fs::remove_file(latest).expect("remove latest");
        match store.load() {
            crate::core::LoadStatus::RecoveredPrior { state, requested } => {
                assert_eq!(requested, Some(2));
                assert_eq!(state.generation, 1);
                assert!(!state.aggregate.catch_up_done);
            }
            other => panic!("expected recovered prior, got {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn component_file_store_bounds_generations_and_keeps_recovery_window() {
        let root = temp_root("retention");
        let mut store = FileUsageStore::at(root.clone());
        let state = sample();
        for generation in 1..=100 {
            assert_eq!(store.persist(&state), PersistStatus::Applied { generation });
            if generation == 1 {
                fs::write(root.join("abandoned.tmp"), b"partial").unwrap();
            }
        }

        let mut generations = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| super::parse_blob_generation(entry.file_name().to_str()?))
            .collect::<Vec<_>>();
        generations.sort_unstable();
        assert_eq!(generations, (92..=100).collect::<Vec<_>>());
        assert!(!root.join("abandoned.tmp").exists());

        fs::write(root.join("g0000000000000100.state"), b"truncated").unwrap();
        let crate::core::LoadStatus::RecoveredPrior { state, requested } = store.load() else {
            panic!("expected prior generation recovery");
        };
        assert_eq!(requested, Some(100));
        assert_eq!(state.generation, 99);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn component_file_store_prunes_stale_generations_on_load() {
        let root = temp_root("startup-retention");
        fs::create_dir_all(&root).unwrap();
        for generation in 1..=100 {
            let mut state = sample();
            state.generation = generation;
            fs::write(
                root.join(format!("g{generation:016}.state")),
                state.encode(),
            )
            .unwrap();
        }
        fs::write(
            root.join("current"),
            b"rundog-usage-current-1\ngeneration=100\n",
        )
        .unwrap();
        fs::write(root.join("abandoned.state.tmp"), b"partial").unwrap();

        let store = FileUsageStore::at(root.clone());
        let crate::core::LoadStatus::Loaded(state) = store.load() else {
            panic!("expected current generation");
        };
        assert_eq!(state.generation, 100);

        let mut generations = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| super::parse_blob_generation(entry.file_name().to_str()?))
            .collect::<Vec<_>>();
        generations.sort_unstable();
        assert_eq!(generations, (92..=100).collect::<Vec<_>>());
        assert!(!root.join("abandoned.state.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn component_legacy_usage_directory_moves_to_product_root() {
        let base = temp_root("legacy-migration");
        let legacy = base.join("SystemExe").join("RunDog").join("usage");
        let current = base.join("RunDog").join("usage");
        fs::create_dir_all(&legacy).unwrap();
        for generation in 1..=100 {
            let mut state = sample();
            state.generation = generation;
            state.aggregate.snapshot.codex.month_input_tokens = 42;
            fs::write(
                legacy.join(format!("g{generation:016}.state")),
                state.encode(),
            )
            .unwrap();
        }
        fs::write(
            legacy.join("current"),
            b"rundog-usage-current-1\ngeneration=100\n",
        )
        .unwrap();

        assert_eq!(
            super::select_production_root(&legacy, &current, &[]),
            current
        );
        assert!(!legacy.exists());
        let crate::core::LoadStatus::Loaded(state) = FileUsageStore::at(current.clone()).load()
        else {
            panic!("expected migrated state");
        };
        assert_eq!(state.generation, 100);
        assert_eq!(state.aggregate.snapshot.codex.month_input_tokens, 42);
        let state_file_count = fs::read_dir(&current)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                super::parse_blob_generation(&entry.file_name().to_string_lossy()).is_some()
            })
            .count();
        assert_eq!(state_file_count, 9);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_valid_product_root_removes_stale_legacy_root() {
        let base = temp_root("legacy-conflict");
        let legacy = base.join("SystemExe").join("RunDog").join("usage");
        let current = base.join("RunDog").join("usage");
        let mut legacy_store = FileUsageStore::at(legacy.clone());
        let mut legacy_state = sample();
        legacy_state.aggregate.snapshot.codex.month_input_tokens = 11;
        assert!(matches!(
            legacy_store.persist(&legacy_state),
            PersistStatus::Applied { .. }
        ));
        let mut current_store = FileUsageStore::at(current.clone());
        let mut current_state = sample();
        current_state.aggregate.snapshot.codex.month_input_tokens = 99;
        assert!(matches!(
            current_store.persist(&current_state),
            PersistStatus::Applied { .. }
        ));

        assert_eq!(
            super::select_production_root(&legacy, &current, &[]),
            current
        );
        assert!(!legacy.exists());
        let crate::core::LoadStatus::Loaded(state) = FileUsageStore::at(current.clone()).load()
        else {
            panic!("expected product state");
        };
        assert_eq!(state.aggregate.snapshot.codex.month_input_tokens, 99);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn component_file_store_failed_root_is_not_reported_durable() {
        let root = temp_root("not-dir");
        fs::write(&root, b"not-a-directory").expect("file");
        let mut store = FileUsageStore::at(root.clone());
        assert_eq!(store.persist(&sample()), PersistStatus::Failed);
        assert!(matches!(store.load(), crate::core::LoadStatus::Missing));
        let _ = fs::remove_file(root);
    }

    #[test]
    fn component_production_root_is_under_localappdata_not_provider_home() {
        let Some(root) = FileUsageStore::production_root() else {
            return;
        };
        assert!(root.ends_with(std::path::Path::new("RunDog\\usage")));
        assert!(!root.ends_with(std::path::Path::new("SystemExe\\RunDog\\usage")));
        let store = FileUsageStore::at(root);
        assert!(!store.refuses_provider_roots(
            std::path::Path::new(r"C:\Users\me\.claude"),
            std::path::Path::new(r"C:\Users\me\.codex")
        ));
    }

    #[test]
    fn component_current_pointer_rejects_incomplete_record() {
        assert!(parse_current("rundog-usage-current-1\n").is_none());
        assert!(parse_current("generation=1\n").is_none());
        assert_eq!(
            parse_current("rundog-usage-current-1\ngeneration=4\n"),
            Some(4)
        );
    }
}
