//! Durable commit saga shared by the Windows adapter and non-live fakes.
//!
//! A commit either applies as one generation with a unique operation ID, or
//! leaves the previous complete generation observable. Timeout and cancel mark
//! the active operation terminal so a late completion cannot mutate state.

use crate::core::{AppSettings, PendingJournal, SettingsRecord};

/// Logical outcome of one commit attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitStatus {
    Applied,
    Duplicate,
    RejectedStale,
    TimedOut,
    Cancelled,
    Failed,
    Tombstoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitOutcome {
    pub status: CommitStatus,
    pub generation: u64,
    pub last_operation_id: u64,
    pub settings: AppSettings,
}

impl CommitOutcome {
    #[must_use]
    pub const fn succeeded(self) -> bool {
        matches!(self.status, CommitStatus::Applied | CommitStatus::Duplicate)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitRequest {
    pub operation_id: u64,
    pub expected_generation: u64,
    pub settings: AppSettings,
    pub previous: AppSettings,
    pub sync_run_entry: bool,
    pub deadline_millis: u64,
}

/// Storage + gate surface required by the saga. Implementations must not call
/// Win32 from the pure tests that feed an in-memory fake.
pub trait DurableStore {
    fn load_record(&mut self) -> SettingsRecord;
    fn write_record(&mut self, record: SettingsRecord, expected_generation: u64) -> bool;
    fn load_pending(&mut self) -> Option<PendingJournal>;
    fn write_pending(&mut self, journal: &PendingJournal) -> bool;
    fn clear_pending(&mut self) -> bool;
    fn is_tombstoned(&mut self) -> bool;
    fn set_startup(&mut self, enabled: bool) -> bool;
    fn now_millis(&self) -> u64;
    fn is_cancelled(&self, operation_id: u64) -> bool;
    fn mark_timed_out(&mut self, operation_id: u64);
    fn mark_cancelled(&mut self, operation_id: u64);
    fn is_timed_out(&self, operation_id: u64) -> bool;
}

fn terminal_blocked<S: DurableStore>(
    store: &S,
    operation_id: u64,
    deadline_millis: u64,
) -> Option<CommitStatus> {
    if store.is_cancelled(operation_id) {
        return Some(CommitStatus::Cancelled);
    }
    if store.is_timed_out(operation_id) || store.now_millis() >= deadline_millis {
        return Some(CommitStatus::TimedOut);
    }
    None
}

fn snapshot_outcome(record: SettingsRecord, status: CommitStatus) -> CommitOutcome {
    CommitOutcome {
        status,
        generation: record.generation,
        last_operation_id: record.last_operation_id,
        settings: record.settings,
    }
}

/// Executes one user-visible configuration commit.
pub fn execute_commit<S: DurableStore>(store: &mut S, request: CommitRequest) -> CommitOutcome {
    if store.is_tombstoned() {
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::Tombstoned);
    }

    let current = store.load_record();
    if request.operation_id <= current.last_operation_id {
        return snapshot_outcome(current, CommitStatus::Duplicate);
    }
    if request.expected_generation != current.generation {
        return snapshot_outcome(current, CommitStatus::RejectedStale);
    }

    if let Some(status) = terminal_blocked(store, request.operation_id, request.deadline_millis) {
        if status == CommitStatus::TimedOut {
            store.mark_timed_out(request.operation_id);
        }
        return snapshot_outcome(current, status);
    }

    let journal = PendingJournal {
        operation_id: request.operation_id,
        base_generation: request.expected_generation,
        desired: request.settings,
        previous: request.previous,
        sync_run_entry: request.sync_run_entry,
        deadline_millis: request.deadline_millis,
    };
    if !store.write_pending(&journal) {
        return snapshot_outcome(current, CommitStatus::Failed);
    }

    if let Some(status) = terminal_blocked(store, request.operation_id, request.deadline_millis) {
        let _ = store.clear_pending();
        if status == CommitStatus::TimedOut {
            store.mark_timed_out(request.operation_id);
        }
        return snapshot_outcome(current, status);
    }

    let next = SettingsRecord::new(
        request.expected_generation.saturating_add(1),
        request.operation_id,
        request.settings,
    );
    if !store.write_record(next, request.expected_generation) {
        let _ = store.clear_pending();
        return snapshot_outcome(current, CommitStatus::Failed);
    }

    if let Some(status) = terminal_blocked(store, request.operation_id, request.deadline_millis) {
        // Late mutation after timeout/cancel is ignored: roll content back and
        // keep generation monotonic so CAS cursors stay aligned.
        let rolled = SettingsRecord::new(
            next.generation.saturating_add(1),
            request.operation_id,
            request.previous,
        );
        let _ = store.write_record(rolled, next.generation);
        let _ = store.clear_pending();
        if status == CommitStatus::TimedOut {
            store.mark_timed_out(request.operation_id);
        }
        let record = store.load_record();
        return snapshot_outcome(record, status);
    }

    if request.sync_run_entry && !store.set_startup(request.settings.launch_at_startup) {
        let rolled = SettingsRecord::new(
            next.generation.saturating_add(1),
            request.operation_id,
            request.previous,
        );
        let _ = store.write_record(rolled, next.generation);
        let _ = store.clear_pending();
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::Failed);
    }

    if let Some(status) = terminal_blocked(store, request.operation_id, request.deadline_millis) {
        // Deadline expired after durable success: ignore further side effects
        // but do not unwind an already acknowledged generation. Clear journal.
        let _ = store.clear_pending();
        if status == CommitStatus::TimedOut {
            store.mark_timed_out(request.operation_id);
        }
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::Applied);
    }

    let _ = store.clear_pending();
    let record = store.load_record();
    snapshot_outcome(record, CommitStatus::Applied)
}

/// Completes or rolls back a journal left by a crash between settings write and
/// Run sync. Late/cancelled/timed-out journals never mutate further.
pub fn recover_pending<S: DurableStore>(store: &mut S) -> CommitOutcome {
    let Some(journal) = store.load_pending() else {
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::Applied);
    };

    if store.is_tombstoned() {
        let _ = store.clear_pending();
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::Tombstoned);
    }

    if store.is_cancelled(journal.operation_id)
        || store.is_timed_out(journal.operation_id)
        || store.now_millis() >= journal.deadline_millis
    {
        store.mark_timed_out(journal.operation_id);
        // Prefer restoring previous content if the desired generation is visible.
        let current = store.load_record();
        if current.last_operation_id == journal.operation_id && current.settings == journal.desired
        {
            let rolled = SettingsRecord::new(
                current.generation.saturating_add(1),
                journal.operation_id,
                journal.previous,
            );
            let _ = store.write_record(rolled, current.generation);
        }
        let _ = store.clear_pending();
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::TimedOut);
    }

    let current = store.load_record();
    if current.last_operation_id == journal.operation_id && current.settings == journal.desired {
        if journal.sync_run_entry && !store.set_startup(journal.desired.launch_at_startup) {
            let rolled = SettingsRecord::new(
                current.generation.saturating_add(1),
                journal.operation_id,
                journal.previous,
            );
            let _ = store.write_record(rolled, current.generation);
            let _ = store.clear_pending();
            let record = store.load_record();
            return snapshot_outcome(record, CommitStatus::Failed);
        }
        let _ = store.clear_pending();
        let record = store.load_record();
        return snapshot_outcome(record, CommitStatus::Applied);
    }

    // Journal without a matching settings write: discard without mutation.
    let _ = store.clear_pending();
    snapshot_outcome(current, CommitStatus::Failed)
}

/// Gate used by adapters to honour cancel and timeout across saga steps.
#[derive(Clone, Debug, Default)]
pub struct CommitGate {
    cancelled: Option<u64>,
    timed_out: Option<u64>,
    now_millis: u64,
}

impl CommitGate {
    #[must_use]
    pub const fn with_clock(now_millis: u64) -> Self {
        Self {
            cancelled: None,
            timed_out: None,
            now_millis,
        }
    }

    pub fn set_now(&mut self, now_millis: u64) {
        self.now_millis = now_millis;
    }

    pub fn advance(&mut self, millis: u64) {
        self.now_millis = self.now_millis.saturating_add(millis);
    }

    pub fn cancel(&mut self, operation_id: u64) {
        self.cancelled = Some(operation_id);
    }

    pub fn mark_timed_out(&mut self, operation_id: u64) {
        self.timed_out = Some(operation_id);
    }

    #[must_use]
    pub fn now_millis(&self) -> u64 {
        self.now_millis
    }

    #[must_use]
    pub fn is_cancelled(&self, operation_id: u64) -> bool {
        self.cancelled == Some(operation_id)
    }

    #[must_use]
    pub fn is_timed_out(&self, operation_id: u64) -> bool {
        self.timed_out == Some(operation_id)
    }
}
