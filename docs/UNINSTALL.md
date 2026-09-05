# Uninstall contract

This is the current uninstall specification. `PLAN.md` is not the current spec.

Real-machine uninstall was **NOT RUN** for this revision. The installer script
and this document are the contract. Fail-first live uninstall remains a
follow-up.

## Scenario

install → Launch at startup ON → change settings → accumulate usage state →
download an update cache installer → exit → uninstall from Windows Apps.

| Item | Location | Uninstall |
| --- | --- | --- |
| Program files | `%LOCALAPPDATA%\Programs\RunDog` | **DELETE** (Inno default `{app}`) |
| Start Menu shortcut | per-user Start Menu | **DELETE** |
| Desktop shortcut | per-user Desktop (if the task was selected) | **DELETE** |
| Uninstaller / Apps entry | Windows Settings → Apps | **DELETE** |
| Launch at startup | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `RunDog` | **DELETE** |
| Settings | `HKCU\Software\SystemExe\RunDog` (including subkeys) | **DELETE** |
| Usage state | `%LOCALAPPDATA%\SystemExe\RunDog\usage` | **DELETE** |
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

- `[UninstallDelete]` removes `{localappdata}\SystemExe\RunDog` (usage + updates).
- `[Code]` `CurUninstallStepChanged` deletes the `Run` value `RunDog` and
  `HKCU\Software\SystemExe\RunDog`.
- The script must not name Claude or Codex homes, logs, or credential files.

`tests/uninstall_contract.rs` statically checks those rules. It does not run
Inno and does not uninstall a live copy.

## What uninstall must never do

- Change or delete Claude Code logs, credentials, or home.
- Change or delete Codex CLI logs, credentials, or home.
- Write secrets, tokens, or prompts into this document or fixtures.
