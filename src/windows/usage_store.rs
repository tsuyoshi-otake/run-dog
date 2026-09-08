//! LOCALAPPDATA usage durable store. Never writes into Claude or Codex trees.

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use crate::core::{
    load_usage_state, persist_usage_state, usage_store_root_is_forbidden, GenerationBlobs,
    LoadStatus, PersistStatus, UsageState, MAX_PRIOR_GENERATIONS,
};

const CURRENT_HEADER: &str = "rundog-usage-current-1";
const CURRENT_NAME: &str = "current";
const BLOB_PREFIX: &str = "g";
const BLOB_SUFFIX: &str = ".state";

#[derive(Clone, Debug)]
pub struct FileUsageStore {
    root: PathBuf,
}

impl FileUsageStore {
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn production_root() -> Option<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(local).join("RunDog").join("usage"))
    }

    #[must_use]
    pub fn production() -> Option<Self> {
        let current = Self::production_root()?;
        let local = std::env::var_os("LOCALAPPDATA")?;
        let legacy = PathBuf::from(local)
            .join("SystemExe")
            .join("RunDog")
            .join("usage");
        Some(Self::at(select_production_root(&legacy, &current)))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn persist(&mut self, state: &UsageState) -> PersistStatus {
        let status = persist_usage_state(self, state);
        if let PersistStatus::Applied { generation } = status {
            self.cleanup_after_publish(generation);
        }
        status
    }

    #[must_use]
    pub fn load(&self) -> LoadStatus {
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
        usage_store_root_is_forbidden(&self.root, &[claude_dir, codex_home])
    }

    fn cleanup_after_publish(&self, generation: u64) {
        let oldest_retained = generation.saturating_sub(MAX_PRIOR_GENERATIONS);
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let old_generation =
                parse_blob_generation(&name).is_some_and(|candidate| candidate < oldest_retained);
            if old_generation || name.ends_with(".tmp") {
                let _ = fs::remove_file(path);
            }
        }
    }
}

impl GenerationBlobs for FileUsageStore {
    fn write_blob(&mut self, generation: u64, bytes: &[u8]) -> bool {
        if !ensure_root(&self.root) {
            return false;
        }
        let final_path = blob_path(&self.root, generation);
        let tmp = final_path.with_extension("state.tmp");
        write_atomically(&tmp, &final_path, bytes)
    }

    fn read_blob(&self, generation: u64) -> Option<Vec<u8>> {
        fs::read(blob_path(&self.root, generation)).ok()
    }

    fn write_current(&mut self, generation: u64) -> bool {
        if !ensure_root(&self.root) {
            return false;
        }
        let payload = format!("{CURRENT_HEADER}\ngeneration={generation}\n");
        let final_path = self.root.join(CURRENT_NAME);
        let tmp = self.root.join("current.tmp");
        write_atomically(&tmp, &final_path, payload.as_bytes())
    }

    fn read_current(&self) -> Option<u64> {
        let text = fs::read_to_string(self.root.join(CURRENT_NAME)).ok()?;
        parse_current(&text)
    }

    fn highest_blob_generation(&self) -> Option<u64> {
        let entries = fs::read_dir(&self.root).ok()?;
        entries
            .filter_map(|entry| {
                let name = entry.ok()?.file_name();
                let name = name.to_str()?;
                let stem = name.strip_prefix(BLOB_PREFIX)?.strip_suffix(BLOB_SUFFIX)?;
                stem.parse().ok()
            })
            .max()
    }
}

fn ensure_root(root: &Path) -> bool {
    fs::create_dir_all(root).is_ok() && root.is_dir()
}

fn select_production_root(legacy: &Path, current: &Path) -> PathBuf {
    if real_directory(current) {
        if root_has_valid_state(current) {
            remove_real_directory(legacy);
            return current.to_path_buf();
        }
        if directory_is_empty(current) && real_directory(legacy) {
            let _ = fs::remove_dir(current);
            if move_directory(legacy, current) {
                return current.to_path_buf();
            }
        }
        if root_has_valid_state(legacy) {
            return legacy.to_path_buf();
        }
        return current.to_path_buf();
    }
    if real_directory(legacy) {
        if move_directory(legacy, current) {
            return current.to_path_buf();
        }
        return legacy.to_path_buf();
    }
    current.to_path_buf()
}

fn root_has_valid_state(root: &Path) -> bool {
    matches!(
        load_usage_state(&FileUsageStore::at(root.to_path_buf())),
        LoadStatus::Loaded(_) | LoadStatus::RecoveredPrior { .. }
    )
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn directory_is_empty(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

fn move_directory(from: &Path, to: &Path) -> bool {
    let Some(parent) = to.parent() else {
        return false;
    };
    fs::create_dir_all(parent).is_ok() && fs::rename(from, to).is_ok()
}

fn remove_real_directory(path: &Path) {
    if real_directory(path) {
        let _ = fs::remove_dir_all(path);
    }
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

fn write_atomically(tmp: &Path, final_path: &Path, bytes: &[u8]) -> bool {
    let _ = fs::remove_file(tmp);
    let Ok(mut file) = File::create(tmp) else {
        return false;
    };
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        let _ = fs::remove_file(tmp);
        return false;
    }
    drop(file);
    // On Windows rename replaces an existing file without a delete/publication
    // gap. A failed replacement leaves the previous current pointer intact.
    if fs::rename(tmp, final_path).is_ok() {
        return true;
    }
    let _ = fs::remove_file(tmp);
    false
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

        assert_eq!(super::select_production_root(&legacy, &current), current);
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

        assert_eq!(super::select_production_root(&legacy, &current), current);
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
