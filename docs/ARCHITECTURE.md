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
| Usage state | `%LOCALAPPDATA%\SystemExe\RunDog\usage` (`rundog-usage-state-1`) |
| Update cache | `%LOCALAPPDATA%\SystemExe\RunDog\updates` |

Claude and Codex homes are read-only inputs except Claude
`.credentials.json`, which OAuth refresh may rewrite. They are never the
usage store and must survive uninstall.

## Usage ingest

JSONL cursors commit only on a complete newline. Failed persist is not
reported durable. Month rollover must not replay history. Formal model:
`formal/RunDogUsageIngest.tla`. TLC PASS is not a Rust proof.

## Updates

Public GitHub Releases only. SHA-256 sidecar is required before Inno
silent install. SignPath Authenticode is pending; see
[`../CODE_SIGNING.md`](../CODE_SIGNING.md) and
[`REPRODUCIBILITY.md`](REPRODUCIBILITY.md).
