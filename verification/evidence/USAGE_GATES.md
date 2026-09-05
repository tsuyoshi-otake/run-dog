# Usage quality gates（実測 2026-09-05）

ピン: `2c0494cdd7b69fe71234f5daf6c07fb98a5f22f5`。TLC PASS は Rust 実装の証明ではない。

## PBT（この increment / #24 上）

| テスト | 結果 |
| --- | --- |
| `pbt_unknown_model_keeps_token_totals_without_inventing_cost` | PASS（core。collector が未知モデルを捨てる件は #12） |
| `pbt_same_logical_tokens_same_known_model_cost` | PASS |
| `pbt_checkpoint_round_trip_and_unknown_lines_do_not_change_known_fields` | PASS |
| `pbt_checkpoint_decode_never_panics_on_arbitrary_payload` | PASS |
| `pbt_jsonl_parsers_never_panic_on_arbitrary_lines` | PASS |
| `pbt_same_events_different_write_batches_same_cost` | PASS（事前に書いた 1 ファイル vs 2 ファイル） |
| `pbt_updater_metadata_parsers_never_panic` | PASS |

回帰ファイル: `verification/evidence/usage-pbt-counterexamples.regressions`（縮小反例なし）。

最初の定式（idle 後 append）は batch 100c vs 50c で落ちた。原因は `STAT_COOLDOWN` 30s で tail を見ないこと。契約を「scan 前に全ファイルが存在する」に直した。idle 後 append の即時可視は **NEEDS_CONTRACT**（#16 の `test_force_restat` 経路）。

## PBT（未マージ PR を worktree で再実行）

| PR | テスト | 結果 |
| --- | --- | --- |
| #14 | `pbt_unchanged_total_is_never_counted_twice` | PASS |
| #14 | `pbt_replay_invariance_after_advancing_session` | PASS |
| #14 | `pbt_restart_equivalence_split_after_first_turn` | PASS |
| #16 | `pbt_reader_prefix_without_newline_never_advances` | PASS |
| #9 | `pbt_codex_output_equals_output_tokens_not_output_plus_reasoning` | PASS |

## Mutation（`cargo-mutants 27.1.0`、`src/core/usage_checkpoint.rs`）

- 33 mutants、28 caught、5 missed、timeout 0
- スコアだけ見ない。killed の代表:
  - `decode -> None` / `Some(Default)`
  - `delete match arm HEADER`
  - `catch_up_done != 0` を `==` に反転
  - `delete !`（`catch_up_done=false` を通す）
  - file prefix `'c'` / `'x'` 削除
  - snapshot の cents / tokens field 削除
  - `parse_file_line -> None` および偽タプル
- missed（等価 / 契約外）:
  - `HEADER_V1\|V2` 腕削除 → `_ => None` と等価
  - `file_key_sort_key` 置換 → encode の並びだけ。decode は順序非依存

未実行（関数がこの tree に無い、または母集団外）:

| 仮説 mutant | 状態 |
| --- | --- |
| remove Codex snapshot dedupe | #14 の PBT が代替。この tree では **NOT RUN** |
| remove snapshot delta | 同上 |
| restore reasoning double-add | #9。main はまだ加算する。**NOT RUN**（意図的に #9 をやり直さない） |
| ignore cache-write | 現行 parser は 5m/1h を読む。欠陥としては **NOT_REPRODUCED** |
| ignore service-tier | 欠陥ではない（#7）。**NOT_REPRODUCED** |
| premature partial offset commit | #16。main の `skip_incomplete_line` は改行無しでも進む。**REPRODUCED**（静的）/ この tree では未修正 |
| long-context double display | #11。**NOT RUN** |
| drop unknown model | core PBT は tokens を保持。collector は main で drop。#12 |
| ignore timezone offset | #19。**NOT RUN**（この tree） |
| treat stale rate-limit as current | #20。**NOT RUN**（この tree） |
| checkpoint generation mismatch | **caught**（HEADER 削除・v1/v2 reject） |

## TLC

- ツール: TLC2 `2026.09.04.170753` (rev `b123b22`)、`tla2tools` v1.8.0、Temurin 21.0.12
- モデル: `formal/RunDogUsageIngest.tla` + `RunDogUsageIngest.cfg`
- 定数: `MaxIds = 2`
- 生成 100 / 相異 26 / 深さ 6 / temporal 3 branches
- 不変条件: TypeOK, AtMostOnceContribution, NoPrematureCommit, CheckpointConsistency, AcceptedAgreesContribution
- 進捗: ProducerProgress, AcceptedProgress（WF on ReadNext / Complete / Producer）
- 反例: なし
- Idle は明示 stutter。デッドロック無し
- **Rust collector の証明ではない。** main の premature offset はこの spec に含まれない（仕様側は禁止）

## Fuzz

`cargo-fuzz` / `fuzz/` 無し。**libFuzzer NOT RUN**。代替は上記 arbitrary-line / checkpoint / Version / checksum の no-panic PBT。

## Coverage

`cargo-llvm-cov 0.8.7` は入っている。この session では `+nightly llvm-cov --branch --all-targets` を回していない。**NOT RUN**。MC/DC は既存文書どおり **NOT RUN**。`C2_AND_MUTATION.md` の 94% branch を再測定していない。

## Diagnostics

`RUNDOG_DIAGNOSTICS=1` のときだけ `UsageCollector` が ring に `UsageTick` を書く。既定は空。path / token / JSONL 本文は型に無い。

## Performance

8h soak **NOT RUN**。
