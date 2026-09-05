# RunDog architecture

Layering is the contract. Do not invert it to ship a feature faster.

## Layers

- `src/core`: pure domain. No Win32, no filesystem, no network, no clocks that cannot be injected. Time enters as `u64` milliseconds.
- `src/application`: state machine and ports. Side effects are values, not hidden calls.
- `src/windows`: OS adapter. Platform types stay here so tests can exercise `core` / `application` without a live tray, hive, or CPU API.
- `formal/` and `verification/`: oracles and evidence. Reuse them. Do not invent a second source of truth.

## Change sequence

1. Search existing issues. Do not duplicate.
2. Write the contract and the oracle first.
3. Fail-first: a test that the current behavior violates the contract.
4. Fix only what is **REPRODUCED**. Leave **NOT_REPRODUCED** / **NEEDS_CONTRACT** / **UNCERTAIN** named.
5. Prefer a small independent PR. Do not stack unrelated hardening.

## Usage / Windows boundaries

- Internal instants are UTC. Civil Today / Month buckets use local TZ.
- Do not display a stale rate-limit percentage as current. After `reset_at`, a failed refresh is Expired / Unknown, not 0%.
- Do not hardcode `C:\`. Resolve the system volume from the Windows directory.
- Flyout clamp uses the work area of the monitor that owns the tray icon or cursor, not primary `SM_CXSCREEN`.
- Do not add a full-history JSONL recursive scan on every tick.

## Diagnostics and secrets

- Diagnostic events may store kinds, counts, and UTC instants.
- Forbidden in diagnostics, logs, and checkpoints: tokens, cookies, Authorization headers, JSONL message bodies, credential file contents, absolute home paths that embed a username when a relative key suffices.
- Authenticode / signed updates stay on issue #1. Do not fold them into a usage PR.

## Verification vocabulary

Use `PASS` / `REPRODUCED` / `NOT_REPRODUCED` / `NEEDS_CONTRACT` / `UNCERTAIN` / `NOT RUN`. Never write “all fixed”.
