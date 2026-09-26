# Inno Setup release contract

The installer is per-user. It installs to %LOCALAPPDATA%\Programs\RunDog, so
updating it does not require elevation. Authenticode signing via SignPath
Foundation is pending; see [CODE_SIGNING.md](../CODE_SIGNING.md). Until then
release assets remain unsigned.

Every installation creates RunDog shortcuts in the current user's Start Menu
and on their desktop, including silent installs and upgrades. No optional task
selection is required. Uninstall removes both shortcuts.

The updater accepts only the latest published stable GitHub Release containing
these exact assets:

- RunDog-Setup-x64.exe
- RunDog-Setup-x64.exe.sha256

The checksum file must contain one sha256sum-style entry for the installer:

<64 lowercase-or-uppercase hexadecimal characters>  RunDog-Setup-x64.exe

At application startup, RunDog checks this endpoint once. A strictly newer
release that satisfies this asset contract is downloaded, hash-checked, and
started with Inno Setup's silent arguments. There is no resident update polling
worker; a GitHub latest-endpoint 404 means no published stable release exists.

Build it locally with PowerShell:

.\scripts\build-installer.ps1 -Version 1.0.0

ISCC.exe from Inno Setup 6 must be installed. The script builds the Rust
release executable, creates dist\RunDog-Setup-x64.exe, and writes its sidecar
checksum. It deliberately does not sign either artifact.

Downloaded installers live in `%LOCALAPPDATA%\SystemExe\RunDog\updates` as
`RunDog-Setup-<version>.exe`. The updater keeps at most two completed
installers and deletes leftover `.part` / `.failed` files. It never deletes
the installer it is about to launch.

Usage state lives in `%LOCALAPPDATA%\RunDog\usage`. On the first startup after
upgrading, RunDog moves the legacy `%LOCALAPPDATA%\SystemExe\RunDog\usage`
directory there without copying its contents.

Uninstall (Windows Settings → Apps → RunDog) removes `{app}`, shortcuts, the
HKCU Run value `RunDog`, `HKCU\Software\SystemExe\RunDog`,
`%LOCALAPPDATA%\RunDog`, and `%LOCALAPPDATA%\SystemExe\RunDog`. It must not name
or touch Claude or Codex homes. The table is
[docs/UNINSTALL.md](../docs/UNINSTALL.md).

## Hosted lifecycle verification

`scripts/test-installer.ps1` runs only under the disposable `runneradmin` account
on a GitHub-hosted Windows Actions runner. It refuses a profile with any existing
RunDog installation, process, data, shortcuts, startup value, or uninstall entry.
The workflow must supply two already-built installer files: the current build and
an older published baseline. For example, with the current build in `dist` and
the verified baseline under the runner's `~/tmp`:

```powershell
./scripts/test-installer.ps1 `
  -CurrentInstaller ./dist/RunDog-Setup-x64.exe -CurrentVersion 1.1.41 `
  -BaselineInstaller (Join-Path $env:USERPROFILE 'tmp/run-dog-installer/baseline/RunDog-Setup-x64.exe') -BaselineVersion 1.1.40 `
  -EvidenceDirectory (Join-Path $env:USERPROFILE 'tmp/run-dog-installer/evidence')
```

The caller must authenticate the baseline installer before invoking the harness
(release sidecar checksum and GitHub asset digest). The harness never downloads
an installer. It performs clean-current install, duplicate shortcut launch,
resident reinstall, uninstall, clean-baseline install with tasks deselected and
no desktop shortcut, resident upgrade that adds the desktop shortcut, and a
second uninstall. It requests normal RunDog exit before each uninstall and saves
the resulting `usage_diagnostics` and `clean_exit` records. Each stage has a
deadline; Inno logs, termination logs, `lifecycle.log`, and `summary.json` are
saved under the runner account's `~/tmp/run-dog-installer`. The runner job should
upload the evidence directory even when the harness fails.

The harness seeds the application's documented `SettingsRecord` with automatic
update **installation** off in the disposable HKCU hive. RunDog still makes its
normal one-shot release check at startup; there is no production setting that
disables that fetch. No provider credentials or real usage data are required.
