# Reproducibility and SignPath verification

Do not treat `PLAN.md` as the current build spec.

## What is pinned today

- `Cargo.toml` `rust-version = "1.97"` (MSRV).
- `Cargo.lock` is committed. Release and Verify use that lockfile.
- CI (`.github/workflows/verify.yml` and `release.yml`) installs
  `dtolnay/rust-toolchain@stable` plus rustfmt/clippy. Local rustc on the
  maintainer machine is currently 1.97.1; GitHub `stable` may be newer.
  Keep both green. Do **not** add `rust-toolchain.toml` that forces CI
  onto an older pin and fights `stable`.
- Inno Setup **6** via `choco install innosetup` on `windows-latest`.
  Exact Chocolatey version pins are **deferred**: a disappearing package
  version turns Release red without helping users. The script contract is
  Inno 6 + `scripts/build-installer.ps1`.

## What is deliberately not in CI

- `cargo-deny` / `cargo audit` / advisory deny lists. Adding them now
  would create a second red path on unrelated crate advisories. Revisit
  after SignPath, as a non-blocking report job first.
- Authenticode verification. SignPath Foundation approval is still
  pending, so current Release assets are unsigned. A hard
  `Get-AuthenticodeSignature` gate would fail every tag.

## After SignPath approval

1. Keep the SHA-256 sidecar as the transport/integrity check. Signing does
   not replace it.
2. Add a Release-job step that runs
   [`../scripts/verify-authenticode.ps1`](../scripts/verify-authenticode.ps1)
   against `dist\RunDog-Setup-x64.exe` and requires `Status = Valid` and
   publisher metadata that matches `CODE_SIGNING.md` (`RunDog` /
   SignPath Foundation). Until then the script is **manual only**.
3. Do not submit PR or local `cargo build` binaries for signing.

## How to reproduce an unsigned installer locally

```powershell
cargo build --release
.\scripts\build-installer.ps1 -Version 1.1.21
```

Inputs: this repo at a known commit, `Cargo.lock`, Inno Setup 6, MSVC
Rust ≥ 1.97. Output: `dist\RunDog-Setup-x64.exe` and
`dist\RunDog-Setup-x64.exe.sha256`. Bit-identical to CI is not claimed
(LTO, path, SDK). The sidecar hash is the distribution contract.
