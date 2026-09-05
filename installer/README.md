# Inno Setup release contract

The installer is per-user. It installs to %LOCALAPPDATA%\Programs\RunDog, so
updating it does not require elevation. Authenticode signing via SignPath
Foundation is pending; see [CODE_SIGNING.md](../CODE_SIGNING.md). Until then
release assets remain unsigned.

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

Uninstall (Windows Settings → Apps → RunDog) removes `{app}`, shortcuts, the
HKCU Run value `RunDog`, `HKCU\Software\SystemExe\RunDog`, and
`%LOCALAPPDATA%\SystemExe\RunDog`. It must not name or touch Claude or Codex
homes. The table is [docs/UNINSTALL.md](../docs/UNINSTALL.md). Live uninstall
was not run for this contract revision.
