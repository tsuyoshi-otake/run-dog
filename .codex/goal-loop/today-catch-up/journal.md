# Goal-loop journal

## Iteration 1

- Implemented bounded registration/identity/read retries with an explicit terminal outcome after three failures and a ten-minute cooldown.
- Kept hot-file restat and discovery active during catch-up so a permanently unreadable file cannot starve current-day usage.
- Added component regressions for a permanently failing path and for a new session appearing while catch-up contains a terminal path.
- Added a stateful PBT command model covering discovery, registration, permanent failure, append, scan, commit, rollover, and restart.
- Added a TLC collector model for terminal catch-up, exact-once counting, hot-log liveness, and restart preservation.

### Failure signals and corrections

- TLC first found that the modeled blocked file could take the successful registration action. Restricted `CollectorRegister` to the healthy file, matching the modeled permanent identity-failure contract.
- A factored collector-only initialization left legacy module variables undefined in TLC. Restored the complete module initialization and isolated collector properties in `formal/RunDogUsageCatchUp.cfg`.
- The default stable toolchain is Rust 1.96 while this repository requires 1.97. Verification used `rustup run nightly cargo test --workspace --locked` without installing packages.
- Nightly Clippy is unavailable because the `clippy` component is not installed. No toolchain mutation was performed; this remains a reported verification limitation.

### Passing evidence

- `cargo fmt --check`: pass.
- `git diff --check`: pass.
- `rustup run nightly cargo test --workspace --locked`: pass; 296 library tests (295 passed, 1 ignored) plus all integration groups passed.
- TLC pass mode: pass; 1,386 generated states, 394 distinct states, depth 20, no invariant or temporal-property violation.
- Test and TLC runner survivor check: pending final verification.
- Publication audit: complete; five reportable findings recorded separately in the Codex Security report.

## Iteration 2

- Luna Max independent verification rejected iteration 1 because metadata failures could drop pending paths without reaching retry/terminal state, the PBT did not exercise the production collector, rollover could restore stale Today state, and the new-session regression did not assert Today.
- Routed metadata/stat failures through the same bounded retry state machine and retained nonterminal retry paths in the pending queue.
- Added a Stateful PBT that drives the production `UsageCollector` through append, replay, tick, drain, and restart sequences, checking exact Month input and Today cost totals.
- Reset committed Today state at rollover in both the Rust command model and TLA+ model, and added `RolloverPreventsStaleTodayRestore`.
- Strengthened component regressions to assert terminal state, cooldown-based recovery, resumed exact totals, and nonzero Today usage.

### Passing evidence

- `cargo fmt --check` and `git diff --check`: pass.
- Focused production Stateful PBT: pass (1 test, 296 filtered).
- `rustup run nightly cargo test --all-targets --all-features --locked`: pass; 297 library tests (296 passed, 1 ignored) and all integration groups passed.
- `rustup run 1.97.1 cargo clippy --all-targets --all-features --locked -- -D warnings`: pass.
- Catch-up TLC: pass; 1,219 generated states, 340 distinct states, depth 20, no error.
- Legacy TLC: pass; 14,619,733 generated states, 1,091,712 distinct states, depth 24, no error.
- Final repository-related Cargo/Rust/Java/TKW survivor check: zero processes.
