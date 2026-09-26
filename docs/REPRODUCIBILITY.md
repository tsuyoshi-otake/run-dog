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
- Verify and Release both use Inno Setup **6.7.1** (released 2026-02-17)
  through Chocolatey. The lifecycle gate and published package therefore
  use the same compiler version and `scripts/build-installer.ps1`.
- CI pins `cargo-audit` **0.22.2**. Only its executable is cached, under an
  exact OS/architecture/Rust-toolchain/tool-version key. Pull requests may
  restore that cache; only trusted push/manual runs may save it. Every run
  verifies the executable version, fetches current advisories, and audits
  `Cargo.lock`; neither advisory data nor audit results are cached.
- New cache/artifact actions are pinned by commit to releases older than
  seven days. Changing dependency or tooling pins requires the same minimum
  release age; the lockfile remains mandatory.

## What is deliberately not in CI

- `cargo-deny` and a separate advisory deny list. `cargo audit` already runs
  as a blocking check in Verify, including the reusable Release gate.
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
cargo build --locked --release
.\scripts\build-installer.ps1 -Version 1.1.41
```

Inputs: this repo at a known commit, `Cargo.lock`, Inno Setup 6, MSVC
Rust ≥ 1.97. Output: `dist\RunDog-Setup-x64.exe` and
`dist\RunDog-Setup-x64.exe.sha256`. Bit-identical to CI is not claimed
(LTO, path, SDK). The sidecar hash is the distribution contract.
