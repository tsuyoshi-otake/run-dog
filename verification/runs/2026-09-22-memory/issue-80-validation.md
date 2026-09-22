# Issue #80: resource ownership and usage retention

## Acceptance rubric

| Criterion | Verify | Expect | Result |
| --- | --- | --- | --- |
| Win32 failure paths release owned objects | `cargo test --lib failed_` | 64 failed region transfers retain zero GDI resources; failed menu attachment destroys each child | PASS |
| Successful transfers retain their intended owners | `cargo test --lib successful_` | Window keeps its region; attached submenu remains valid until parent destruction | PASS |
| Cursor retirement is bounded and durable | `cargo test --lib retirement` and `cargo test --lib deleted_cursors` | At most four paths per tick; missing current-month cursors survive; expired cursors disappear from memory and a persisted restart | PASS |
| Resuming sessions cannot replay counted usage | `cargo test --lib retired_` and `cargo test --lib legacy_checkpoint` | New bytes count once; cumulative baseline and legacy offsets survive | PASS |
| Cold queues do not resurrect retired cursors | `cargo test --lib retirement_does_not_reregister` | Cold registration is skipped; a live append may resume during the sweep | PASS |
| Durable metadata is compatible and validated | `cargo test --lib cursor_activity_month` and full property suite | Optional named records roundtrip; invalid, duplicate and orphan records fail decoding | PASS |
| Existing behavior remains valid | `cargo test --all-targets --locked` | Zero failures | 357 PASS, 1 existing manual test ignored |
| Static checks and release build | `cargo fmt --check`; `cargo clippy --all-targets --all-features --locked -- -D warnings`; `cargo build --release --locked` | All exit 0 | PASS |
| Dependency safety | `cargo audit` | No advisory findings | PASS, 84 locked dependencies; no dependency changes |

All Cargo commands used one pinned toolchain (1.97.1) and a shared target
directory, sequentially. Native test runner processes were checked after exit.
The GDI resource assertion runs its own child test process, avoiding interference
from concurrent GUI tests. The first per-handle assertion was replaced because
Windows caches deleted region handles; process GDI counts are the verified oracle.

The initial main CI run exposed an existing nondeterministic same-size rewrite
fixture: identical file IDs and 64-byte prefixes depend on a changed millisecond
mtime, but two fast writes can have equal timestamps. The test now explicitly
sets a different `FileTimes` value and asserts `SameSizeRewriteHint`. Production
rewrite detection is unchanged. The complete local suite passed again (357,
with the same one manual test ignored); the first release run was cancelled
before publication so the corrected fixture is included in the final release.

## Findings and fixes

The baseline deleted-file probe retained 64 cursors after month rollover and
durable encode/decode. Failed window-region transfers retained 64 GDI objects;
failed submenu attachment left a child menu unattached to the destroyed parent.
The fixes assign cleanup to the API caller when ownership transfer fails and to
UsageCollector for cursor retirement. Current-month continuation state is never
retired; fully consumed old files have a full prior-month mtime grace period.
Retirement deletes bookkeeping only, never provider files.

The installed 1.1.32 baseline was sampled for 301.5 seconds: private memory
39.160 to 39.340 MiB (maximum 39.395), handles 402 to 402 (maximum 418), GDI 31
throughout, USER 26 to 26 (maximum 28). This did not show a sustained live leak,
but did not cover the reproduced exceptional paths or months of accumulation.

## Required scope and architecture (IRV)

A. The required responsibilities were UsageCollector lifetime/state management,
UsageState serialization, and Win32 ownership transfer in flyout/tray. Regression
tests live beside these owners; usage_ingest_pbt and usage_durable fixtures gained
the optional field. Architecture documentation states the retention contract.

B. Scope expanded from memory ownership to the durable cursor contract because
in-memory-only pruning would reload retired cursors after restart. Release scope
also required version metadata and the mandated privacy/history audit. No other
application responsibility was refactored.

C. UsageCollector remains the sole owner of retirement and live continuation
state; UsageState owns optional on-disk metadata. The field uses existing named
schema-3 records and defaults conservatively for older checkpoints. No new
dependency or infrastructure knowledge was added to core policy. Cleanup is a
behavioral retention change, not an externally equivalent refactor.

D. Future cursor-retention changes can start with this contract and the colocated
regressions without rediscovering scheduler, checkpoint or GDI failure behavior.
They must understand the active-month invariant, the prior-month mtime grace,
and cumulative baseline reconstruction on resumed sessions.

## Release privacy audit

The four required checks ran in parallel: tracked content, author/committer
identities, GitHub visibility/tags/releases, and LICENSE. The repository was
already public, with 36 tags and latest release v1.1.33; MIT LICENSE exists.
Existing author/committer identities use the project noreply identity.
The fresh remote mirror additionally exposes GitHub-managed pull-request refs
with older personal author email/name values; these are separate from the
current branch and tag histories. They are included in the local sanitized
mirror, but GitHub does not permit pushing those managed refs. Server-side cache
and closed-PR reference removal requires GitHub Support; a branch/tag rewrite
does not certify their erasure.

Actual findings: personal publisher metadata in build.rs and personal machine
paths in historical verification evidence. Current files were anonymized and
the user explicitly approved backup plus rewriting affected branches and tags.
UTF-16 evidence is included in the history scan. Historical coverage reports also
contained the same local username. No token/private-key patterns were found.

Reviewed intentional matches: public repository/maintainer URLs and project
publisher/registry identifiers; public MIT attribution; the stable installer
AppId; the public Claude OAuth client ID; synthetic test paths (a, me, example)
and the adversarial URL userinfo fixture; project noreply and Cursor service
attribution addresses. These are not private credentials or accidental local
identity disclosure. A generated script UUID in old evidence was anonymized.

## Limits

Short process measurements do not prove absence of all leaks. This change is
verified on Windows with deterministic failure, restore and continuation cases;
it is not a multi-day soak or a proof for every filesystem and Windows version.
