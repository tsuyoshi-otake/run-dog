# Architecture (current)

This is the short current map. `PLAN.md` is a historical implementation
plan, not the spec. Uninstall is [`UNINSTALL.md`](UNINSTALL.md). Privacy is
[`PRIVACY.md`](PRIVACY.md).

## Process

One Win32 message-loop thread owns the tray icon, flyout, timers, and
settings commits. Update check and vendor limit fetch use short-lived
workers. There is no GUI framework and no resident poller thread.

## Durable locations

| Kind | Location |
| --- | --- |
| Program | `%LOCALAPPDATA%\Programs\RunDog` |
| Settings | `HKCU\Software\SystemExe\RunDog` |
| Startup | `HKCU\...\Run` value `RunDog` |
| Usage state | `%LOCALAPPDATA%\RunDog\usage` (`rundog-usage-state-2`) |
| Process diagnostics | `%LOCALAPPDATA%\RunDog\diagnostics` (64 KiB bounded termination log + active-run marker) |
| Update cache | `%LOCALAPPDATA%\SystemExe\RunDog\updates` |

Claude and Codex homes are read-only inputs except Claude
`.credentials.json`, which OAuth refresh may rewrite. They are never the
usage store and must survive uninstall.

On first startup after upgrading, RunDog moves the legacy
`%LOCALAPPDATA%\SystemExe\RunDog\usage` directory to the current usage-state
location before loading it. The move stays on the same volume and does not copy
the state payloads.

RunDog creates an active-run marker after acquiring the single-instance mutex
and removes it only after a controlled exit. A marker found by the next launch
is recorded as `unclean_previous_run` in `termination.log`; this proves the
prior process did not complete its exit path, but cannot by itself distinguish
a crash, Task Manager termination, power loss, or an OS-forced shutdown.

## Usage ingest

JSONL cursors commit only on a complete newline. Failed persist is not
reported durable. Month rollover must not replay history. Formal model:
`formal/RunDogUsageIngest.tla`. TLC PASS is not a Rust proof.

`UsageCollector` owns cursor retirement. Restore and month rollover enqueue a
single pass; each tick inspects at most four paths using metadata only. A cursor
active this month is preserved even while its file is missing. Older cursors may
be retired when their file is missing, or when it is fully consumed and its mtime
predates the previous month. Unread files, recent files, reparse points and
metadata errors other than NotFound are preserved. No provider file is deleted.
Queues are filtered once per pass, so retirement bookkeeping is O(N).

The optional `active_month` named record in schema 3 persists the last month in
which bytes were consumed; legacy cursors inherit the checkpoint aggregate month.
This protects offsets across restart and allows an interrupted retirement pass
to continue. A resumed retired session is parsed again to rebuild its Codex
cumulative baseline; only events in the requested month contribute to that month.
Claude deduplication keys are retained only for the requested month.

## Updates

Public GitHub Releases only. SHA-256 sidecar is required before Inno
silent install. SignPath Authenticode is pending; see
[`../CODE_SIGNING.md`](../CODE_SIGNING.md) and
[`REPRODUCIBILITY.md`](REPRODUCIBILITY.md).
