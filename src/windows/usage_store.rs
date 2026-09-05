//! LOCALAPPDATA usage durable store. Never writes into Claude or Codex trees.

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use crate::core::{
    load_usage_state, persist_usage_state, usage_store_root_is_forbidden, GenerationBlobs,
    LoadStatus, PersistStatus, UsageState,
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
        Some(
            PathBuf::from(local)
                .join("SystemExe")
                .join("RunDog")
                .join("usage"),
        )
    }

    #[must_use]
    pub fn production() -> Option<Self> {
        Some(Self::at(Self::production_root()?))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn persist(&mut self, state: &UsageState) -> PersistStatus {
        persist_usage_state(self, state)
    }

    #[must_use]
    pub fn load(&self) -> LoadStatus {
        load_usage_state(self)
    }

    #[must_use]
    pub fn refuses_provider_roots(&self, claude_dir: &Path, codex_home: &Path) -> bool {
        usage_store_root_is_forbidden(&self.root, &[claude_dir, codex_home])
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

fn blob_path(root: &Path, generation: u64) -> PathBuf {
    root.join(format!("{BLOB_PREFIX}{generation:016}{BLOB_SUFFIX}"))
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
    if final_path.exists() && fs::remove_file(final_path).is_err() {
        let _ = fs::remove_file(tmp);
        return false;
    }
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
            schema_version: 1,
            aggregate: UsageAggregate {
                month_start: 20_260_901,
                today: 20_260_905,
                last_collected_ms: 0,
                catch_up_done: false,
                snapshot: UsageSnapshot::default(),
            },
            cursors: Vec::new(),
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
        let Some(store) = FileUsageStore::production() else {
            return;
        };
        assert!(store
            .root()
            .ends_with(std::path::Path::new("SystemExe\\RunDog\\usage")));
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
