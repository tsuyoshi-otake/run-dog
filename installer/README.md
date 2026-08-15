# Inno Setup release contract

The installer is intentionally per-user and unsigned. It installs to
%LOCALAPPDATA%\Programs\RunDog, so updating it does not require elevation.

The updater accepts only the latest published stable GitHub Release containing
these exact assets:

- RunDog-Setup-x64.exe
- RunDog-Setup-x64.exe.sha256

The checksum file must contain one sha256sum-style entry for the installer:

<64 lowercase-or-uppercase hexadecimal characters>  RunDog-Setup-x64.exe

Build it locally with PowerShell:

.\scripts\build-installer.ps1 -Version 0.1.0

ISCC.exe from Inno Setup 6 must be installed. The script builds the Rust
release executable, creates dist\RunDog-Setup-x64.exe, and writes its sidecar
checksum. It deliberately does not sign either artifact.
