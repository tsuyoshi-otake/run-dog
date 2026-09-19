# Verified project rules

- Usage catch-up retries must have an explicit terminal outcome. A permanently unreadable path must not own global finalization or starve hot-file discovery; verify this contract with a component regression, stateful PBT, and TLC liveness properties.
- A restored usage checkpoint can contain thousands of old file registrations. Prioritize current-day discovery, registration, and provider-balanced reads, and reserve read capacity after registration. Verify against a restored backlog and the production usage state so Today does not remain at zero while month totals advance.
- Run Cargo tests and Clippy sequentially against one toolchain in the shared target directory. A mixed-toolchain target produced `E0514` during parallel verification; `cargo clean` followed by sequential tests and Clippy passed.
