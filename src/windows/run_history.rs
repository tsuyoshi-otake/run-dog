//! Best-effort process lifecycle diagnostics.
//!
//! A process cannot write after Task Manager, a crash, or power loss kills it.
//! Keep an active-run marker instead and classify a marker found by the next
//! launch as an unclean prior termination.

use std::path::{Path, PathBuf};

use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetSystemTime};

use crate::core::DiagnosticSnapshot;

use super::usage_store_path::PinnedDirectory;

const MARKER_HEADER: &str = "rundog-active-run-1";
const MARKER_NAME: &str = "active-run";
const MARKER_TEMP_NAME: &str = "active-run.tmp";
const LOG_NAME: &str = "termination.log";
const LOG_TEMP_NAME: &str = "termination.log.tmp";
const MAX_MARKER_BYTES: usize = 4 * 1024;
const MAX_LOG_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct RunMarker {
    run_id: String,
    started_utc: String,
    pid: u32,
    version: String,
}

impl RunMarker {
    fn new(started_utc: String, pid: u32, version: String) -> Self {
        let run_id = format!("{started_utc}-{pid}");
        Self {
            run_id,
            started_utc,
            pid,
            version,
        }
    }

    fn encode(&self) -> String {
        format!(
            "{MARKER_HEADER}\nrun_id={}\nstarted_utc={}\npid={}\nversion={}\n",
            self.run_id, self.started_utc, self.pid, self.version
        )
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_MARKER_BYTES {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.lines();
        if lines.next()? != MARKER_HEADER {
            return None;
        }
        let run_id = field(lines.next()?, "run_id")?.to_owned();
        let started_utc = field(lines.next()?, "started_utc")?.to_owned();
        let pid = field(lines.next()?, "pid")?.parse().ok()?;
        let version = field(lines.next()?, "version")?.to_owned();
        if run_id.is_empty()
            || started_utc.is_empty()
            || version.is_empty()
            || lines.next().is_some()
            || !safe_field(&run_id)
            || !safe_field(&started_utc)
            || !safe_field(&version)
        {
            return None;
        }
        Some(Self {
            run_id,
            started_utc,
            pid,
            version,
        })
    }
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.strip_prefix(name)?.strip_prefix('=')
}

fn safe_field(value: &str) -> bool {
    !value.contains(['\r', '\n'])
}

/// Owns the marker for one process run. There is deliberately no `Drop`
/// cleanup: aborts and forced termination must leave the marker behind.
pub(super) struct RunSession {
    root: Option<PathBuf>,
    marker: RunMarker,
}

impl RunSession {
    pub(super) fn start_production() -> Self {
        let marker = RunMarker::new(
            utc_now(),
            std::process::id(),
            env!("CARGO_PKG_VERSION").to_owned(),
        );
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|local| local.join("RunDog").join("diagnostics"));
        Self::start(root, marker)
    }

    fn start(root: Option<PathBuf>, marker: RunMarker) -> Self {
        let Some(root_path) = root.as_deref() else {
            return Self { root: None, marker };
        };
        let Some(directory) = PinnedDirectory::open(root_path, true, &[], false) else {
            return Self { root: None, marker };
        };

        let marker_path = root_path.join(MARKER_NAME);
        if let Some(previous_bytes) = directory.read(&marker_path) {
            if let Some(previous) = RunMarker::decode(&previous_bytes) {
                let prior_run = format!("previous_run={}", previous.run_id);
                append_log_once(
                    &directory,
                    root_path,
                    &format!(
                        "{} event=unclean_previous_run previous_run={} previous_started_utc={} previous_pid={} previous_version={}",
                        marker.started_utc,
                        previous.run_id,
                        previous.started_utc,
                        previous.pid,
                        previous.version
                    ),
                    &prior_run,
                );
            } else {
                append_log(
                    &directory,
                    root_path,
                    &format!(
                        "{} event=unclean_previous_run previous_marker=unreadable",
                        marker.started_utc
                    ),
                );
            }
        }

        let marker_temp = root_path.join(MARKER_TEMP_NAME);
        if !directory.write_atomically(&marker_temp, &marker_path, marker.encode().as_bytes()) {
            return Self { root: None, marker };
        }
        append_log(
            &directory,
            root_path,
            &format!(
                "{} event=start run={} pid={} version={}",
                marker.started_utc, marker.run_id, marker.pid, marker.version
            ),
        );
        Self {
            root: Some(root_path.to_path_buf()),
            marker,
        }
    }

    pub(super) fn finish_clean(self) {
        self.finish("clean_exit", None);
    }

    pub(super) fn finish_error(self, error: &str) {
        self.finish("error_exit", Some(error));
    }

    /// Capture the numeric collector state once at message-loop exit.
    /// A missing diagnostics directory or failed write must not affect shutdown.
    pub(super) fn record_usage_diagnostics(&self, snapshot: DiagnosticSnapshot) {
        let Some(root) = self.root.as_deref() else {
            return;
        };
        let Some(directory) = PinnedDirectory::open(root, false, &[], false) else {
            return;
        };
        append_log(
            &directory,
            root,
            &format!(
                "{} event=usage_diagnostics run={} pid={} snapshot={snapshot:?}",
                utc_now(),
                self.marker.run_id,
                self.marker.pid
            ),
        );
    }

    fn finish(self, event: &str, detail: Option<&str>) {
        let Some(root) = self.root.as_deref() else {
            return;
        };
        let Some(directory) = PinnedDirectory::open(root, false, &[], false) else {
            return;
        };
        let marker_path = root.join(MARKER_NAME);
        let owns_marker = directory
            .read(&marker_path)
            .and_then(|bytes| RunMarker::decode(&bytes))
            .is_some_and(|current| current.run_id == self.marker.run_id);
        if !owns_marker || !directory.remove_file(&marker_path) {
            return;
        }

        let detail = detail
            .map(sanitize_detail)
            .filter(|value| !value.is_empty())
            .map_or_else(String::new, |value| format!(" detail={value}"));
        append_log(
            &directory,
            root,
            &format!(
                "{} event={event} run={} pid={} version={}{}",
                utc_now(),
                self.marker.run_id,
                self.marker.pid,
                self.marker.version,
                detail
            ),
        );
    }
}

fn sanitize_detail(value: &str) -> String {
    value
        .chars()
        .take(256)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn append_log_once(directory: &PinnedDirectory, root: &Path, line: &str, dedupe_key: &str) {
    let log_path = root.join(LOG_NAME);
    let existing = directory.read(&log_path).unwrap_or_default();
    if !existing.split(|byte| *byte == b'\n').any(|candidate| {
        candidate
            .windows(dedupe_key.len())
            .any(|window| window == dedupe_key.as_bytes())
    }) {
        write_log(directory, root, &existing, line);
    }
}

fn append_log(directory: &PinnedDirectory, root: &Path, line: &str) {
    let existing = directory.read(&root.join(LOG_NAME)).unwrap_or_default();
    write_log(directory, root, &existing, line);
}

fn write_log(directory: &PinnedDirectory, root: &Path, existing: &[u8], line: &str) {
    let bytes = bounded_log(existing, line.as_bytes(), MAX_LOG_BYTES);
    let _ = directory.write_atomically(&root.join(LOG_TEMP_NAME), &root.join(LOG_NAME), &bytes);
}

fn bounded_log(existing: &[u8], line: &[u8], limit: usize) -> Vec<u8> {
    let line = if line.len() + 1 > limit {
        &line[line.len().saturating_sub(limit.saturating_sub(1))..]
    } else {
        line
    };
    let keep = limit.saturating_sub(line.len() + 1);
    let start = if existing.len() <= keep {
        0
    } else {
        let candidate = existing.len() - keep;
        existing[candidate..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(existing.len(), |offset| candidate + offset + 1)
    };
    let mut output = Vec::with_capacity((existing.len() - start) + line.len() + 1);
    output.extend_from_slice(&existing[start..]);
    output.extend_from_slice(line);
    output.push(b'\n');
    output
}

fn utc_now() -> String {
    let mut now = SYSTEMTIME::default();
    unsafe { GetSystemTime(&mut now) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
    )
}

#[cfg(test)]
mod tests {
    use super::{bounded_log, RunMarker, RunSession, LOG_NAME, MARKER_NAME, MAX_LOG_BYTES};
    use crate::core::DiagnosticSnapshot;
    use std::{fs, path::PathBuf};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "run-dog-history-{}-{nonce}-{name}",
                std::process::id()
            )))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    fn marker(stamp: &str, pid: u32) -> RunMarker {
        RunMarker::new(stamp.to_owned(), pid, "1.2.3".to_owned())
    }

    #[test]
    fn component_first_launch_creates_marker_and_start_record() {
        let root = TestDirectory::new("first");
        let session = RunSession::start(
            Some(root.0.clone()),
            marker("2026-09-20T00:00:00.000Z", 101),
        );

        assert!(root.0.join(MARKER_NAME).is_file());
        let log = fs::read_to_string(root.0.join(LOG_NAME)).expect("log");
        assert!(log.contains("event=start"));
        assert!(!log.contains("event=unclean_previous_run"));
        drop(session);
    }

    #[test]
    fn component_clean_exit_removes_marker_and_next_launch_is_not_unclean() {
        let root = TestDirectory::new("clean");
        RunSession::start(
            Some(root.0.clone()),
            marker("2026-09-20T00:00:00.000Z", 102),
        )
        .finish_clean();
        assert!(!root.0.join(MARKER_NAME).exists());

        let second = RunSession::start(
            Some(root.0.clone()),
            marker("2026-09-20T00:01:00.000Z", 103),
        );
        let log = fs::read_to_string(root.0.join(LOG_NAME)).expect("log");
        assert!(log.contains("event=clean_exit"));
        assert!(!log.contains("event=unclean_previous_run"));
        drop(second);
    }

    #[test]
    fn component_stale_marker_is_recorded_once_before_new_run() {
        let root = TestDirectory::new("unclean");
        let first = RunSession::start(
            Some(root.0.clone()),
            marker("2026-09-20T00:00:00.000Z", 104),
        );
        drop(first);

        let second = RunSession::start(
            Some(root.0.clone()),
            marker("2026-09-20T00:02:00.000Z", 105),
        );
        let log = fs::read_to_string(root.0.join(LOG_NAME)).expect("log");
        assert_eq!(log.matches("event=unclean_previous_run").count(), 1);
        assert!(log.contains("previous_started_utc=2026-09-20T00:00:00.000Z"));
        assert!(log.contains("previous_pid=104"));
        drop(second);
    }

    #[test]
    fn component_log_is_bounded_and_discards_only_complete_old_lines() {
        let old = (0..2_000)
            .map(|index| format!("old-{index:04}\n"))
            .collect::<String>();
        let output = bounded_log(old.as_bytes(), b"new-terminal-record", 256);

        assert!(output.len() <= 256);
        assert!(output.ends_with(b"new-terminal-record\n"));
        assert!(!output.starts_with(b"\n"));
        assert!(std::str::from_utf8(&output).is_ok());

        let production = bounded_log(&vec![b'x'; MAX_LOG_BYTES], b"last", MAX_LOG_BYTES);
        assert!(production.len() <= MAX_LOG_BYTES);
    }

    #[test]
    fn component_exit_usage_diagnostics_are_numeric_correlated_and_bounded() {
        let root = TestDirectory::new("fake-secret-path");
        let session = RunSession::start(
            Some(root.0.clone()),
            marker("2026-09-20T00:00:00.000Z", 106),
        );
        let old = (0..10_000)
            .map(|index| format!("old-{index:05}\n"))
            .collect::<String>();
        fs::write(root.0.join(LOG_NAME), old).expect("prefill diagnostics log");

        session.record_usage_diagnostics(DiagnosticSnapshot {
            retained_dirs: 8,
            queued_discover_dirs: 3,
            queued_pending_files: 4,
            queued_registrations: 5,
            queued_deferred_files: 6,
            queued_retirements: 7,
            deduped_registration_paths: 9,
            deduped_retired_paths: 10,
            tracked_path_retries: 11,
            stored_file_checkpoints: 12,
            claude_fetch_in_flight: 1,
            codex_fetch_in_flight: 0,
            usage_parse_bytes: u64::MAX,
            ..DiagnosticSnapshot::default()
        });

        let log = fs::read_to_string(root.0.join(LOG_NAME)).expect("diagnostics log");
        assert!(log.len() <= MAX_LOG_BYTES);
        assert_eq!(log.matches("event=usage_diagnostics").count(), 1);
        assert!(log.contains("run=2026-09-20T00:00:00.000Z-106 pid=106"));
        for gauge in [
            "retained_dirs: 8",
            "queued_discover_dirs: 3",
            "queued_pending_files: 4",
            "queued_registrations: 5",
            "queued_deferred_files: 6",
            "queued_retirements: 7",
            "deduped_registration_paths: 9",
            "deduped_retired_paths: 10",
            "tracked_path_retries: 11",
            "stored_file_checkpoints: 12",
            "claude_fetch_in_flight: 1",
            "codex_fetch_in_flight: 0",
        ] {
            assert!(log.contains(gauge), "missing {gauge}");
        }
        assert!(log.contains(&format!("usage_parse_bytes: {}", u64::MAX)));
        assert!(!log.contains("fake-secret-path"));
        assert!(!log.contains("Bearer"));
        assert!(!log.contains("Authorization"));
    }
}
