# Goal: RunDog continues collecting Today usage when catch-up encounters permanently unreadable files
Workdir: C:\Codes\tsuyoshi-otake\run-dog
Max iterations: 8

## Criteria
- C1: A permanently unreadable registration cannot keep the collector in catch-up forever or block healthy files. Verify: run the focused Rust usage tests containing the new failure regression. Expect: tests exit 0 and assert an explicit terminal outcome plus healthy-file progress.
- C2: Appends and new sessions created during catch-up are eventually observed and counted once in Today and Month. Verify: run the focused Rust usage tests containing the new live-progress regressions. Expect: tests exit 0 with nonzero Today and exact-once Month totals.
- C3: Restart, day rollover, and retry sequences preserve committed cursors and never double count. Verify: run the usage Stateful PBT suite. Expect: all generated sequences pass and counterexamples are persisted under verification/evidence if found.
- C4: The formal model proves NoDoubleCount, NoLostHealthyProgress, CatchUpHasTerminalOutcome, HotLogEventuallyObserved, and RestartPreservesCommittedState under documented fairness assumptions. Verify: run TLC against the catch-up model configuration. Expect: model checking completes with no invariant or temporal-property violation.
- C5: Repository quality gates pass. Verify: run cargo fmt --check, cargo clippy --all-targets --all-features -- -D warnings, and cargo test --all-targets --all-features. Expect: every command exits 0 and no repository-related test process survives.
- C6: Pre-publication audit covers tracked-file secrets/PII-like strings, commit authors, GitHub visibility and tags, and LICENSE presence. Verify: inspect git-tracked content with offline searches, git history metadata, gh repo view/API output, git/GitHub tags, and the tracked LICENSE file. Expect: no unaddressed secret or unintended personal-data disclosure; visibility and license status are explicitly reported.
