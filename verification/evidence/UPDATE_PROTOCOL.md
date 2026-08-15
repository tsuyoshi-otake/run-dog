# GitHub Release update protocol — verification evidence

Scope: `src/update.rs`, the non-live update contract, and isolated Windows
adapter components. No test opens a network connection or starts an installer.

## Reproduction

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo +nightly llvm-cov --all-targets --json --summary-only --branch `
  --output-path target\coverage-branch-summary.json
cargo mutants --file src/update.rs --output target\mutants-update-full-final-03 `
  --jobs 4 --timeout 120
```

## Results

- `cargo test --all-targets`: 44 unit/component, 5 existing app integration,
  12 persistence-protocol (1 expected-negative test ignored), 1 state-machine
  PBT, and 2 startup-update fake-API integration tests passed.
- The startup-update component tests exercise the state gate, the GitHub latest
  404/no-stable distinction, and the fixed Inno silent parameters. They do not
  execute WinHTTP or ShellExecute.
- Update PBT: 2,048 deterministic cases; seed `0x5EED_2026_0815_0001`.
  Minimized failures are persisted in `update-pbt-counterexamples.regressions`.
- `cargo-mutants 27.1.0`: 51 generated; 45 caught; 0 survived; 6 unviable;
  0 timeout. The viable-mutant score is 100.0%. The six unviable mutations
  fail to compile because the substituted return type has no `Default`; none
  is classified as equivalent.
- `cargo-llvm-cov 0.8.7` with nightly Rust 1.99.0-nightly branch mode:
  `src/update.rs` 313/335 lines, 30/32 functions, 51/58 branches. Branch
  coverage is not reported as C2 or MC/DC.

## Outside the non-live test scope

- A live GitHub Release request/download.
- `ShellExecuteW` of an installer.
- Runtime ShellExecuteW of an installer. Inno Setup 6.7.3 did compile the
  local v0.1.1 artifact separately; that verifies packaging only, not the
  runtime auto-update path.
- MC/DC: `cargo-llvm-cov 0.8.7 --mcdc` is incompatible with the installed
  nightly compiler's `coverage-options` spelling.
