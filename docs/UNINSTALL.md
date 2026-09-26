# Uninstall contract

This is the current uninstall specification. `PLAN.md` is not the current spec.

The disposable GitHub-hosted Windows lifecycle harness is
[`scripts/test-installer.ps1`](../scripts/test-installer.ps1). It verifies the
installer and uninstaller with isolated provider and other-product sentinels;
the harness must never run in a developer's live account.
It saves the normal-exit diagnostics log under the disposable runner account's
`~/tmp/run-dog-installer` before uninstall removes RunDog-owned state.

## Scenario

install → Launch at startup ON → change settings → accumulate usage state →
download an update cache installer → exit → uninstall from Windows Apps.

| Item | Location | Uninstall |
| --- | --- | --- |
| Program files | `%LOCALAPPDATA%\Programs\RunDog` | **DELETE** (Inno default `{app}`) |
| Start Menu shortcut | per-user Start Menu | **DELETE** |
| Desktop shortcut | per-user Desktop (created on every install and upgrade) | **DELETE** |
| Uninstaller / Apps entry | Windows Settings → Apps | **DELETE** |
| Launch at startup | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `RunDog` | **DELETE** |
| Settings | `HKCU\Software\SystemExe\RunDog` (including subkeys) | **DELETE** |
| Usage state | `%LOCALAPPDATA%\RunDog\usage` | **DELETE** |
| Process diagnostics | `%LOCALAPPDATA%\RunDog\diagnostics` | **DELETE** |
| Legacy usage state | `%LOCALAPPDATA%\SystemExe\RunDog\usage` | **DELETE** (migrated on startup) |
| Update cache | `%LOCALAPPDATA%\SystemExe\RunDog\updates` | **DELETE** |
| Claude logs | `%USERPROFILE%\.claude\projects\*.jsonl` (or `%CLAUDE_CONFIG_DIR%`) | **PRESERVE** |
| Claude credentials | `%USERPROFILE%\.claude\.credentials.json` | **PRESERVE** |
| Claude home | `%USERPROFILE%\.claude` or `%CLAUDE_CONFIG_DIR%` | **PRESERVE** |
| Codex sessions | `%USERPROFILE%\.codex\sessions\` (or `%CODEX_HOME%`) | **PRESERVE** |
| Codex credentials | `%USERPROFILE%\.codex\auth.json` | **PRESERVE** |
| Codex home | `%USERPROFILE%\.codex` or `%CODEX_HOME%` | **PRESERVE** |
| Windows Personalize | `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` | **PRESERVE** |

`%LOCALAPPDATA%\SystemExe` is left if it is empty after the RunDog subtree is
removed. Other products under `SystemExe` are not this contract.

## Installer implementation

[`installer/RunDog.iss`](../installer/RunDog.iss):

- `[UninstallDelete]` removes `{localappdata}\RunDog` (usage) and
  `{localappdata}\SystemExe\RunDog` (legacy usage + updates).
- `[Code]` `CurUninstallStepChanged` deletes the `Run` value `RunDog` and
  `HKCU\Software\SystemExe\RunDog`.
- The script must not name Claude or Codex homes, logs, or credential files.

`tests/uninstall_contract.rs` statically checks those rules. The hosted lifecycle
harness checks their live behavior after both a clean install and an upgrade.

## What uninstall must never do

- Change or delete Claude Code logs, credentials, or home.
- Change or delete Codex CLI logs, credentials, or home.
- Write secrets, tokens, or prompts into this document or fixtures.
