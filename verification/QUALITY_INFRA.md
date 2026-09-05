# Phase 5 quality infra（段階メモ）

親: #5 / 子: #23。ピン: `2c0494cdd7b69fe71234f5daf6c07fb98a5f22f5`。

既存の `formal/` と `verification/C2_AND_MUTATION.md` を再利用する。新しい全履歴スキャンや Authenticode はここに含めない。

## いまあるもの

| 手段 | 現状 | 次の対象（未実行は NOT RUN） |
| --- | --- | --- |
| PBT | settings / CPU / update / storage ほか。seed 付き update 反例あり | usage: RFC3339 bucket、limit freshness、JSONL cursor 不変条件 |
| C2 / mutation | `verification/C2_AND_MUTATION.md`。usage adapter は母集団外 | `core/usage.rs` の freshness / timestamp を独立 shard |
| TLC | `formal/RunDogProtocol.tla`（settings commit） | usage SM は未モデル。**NOT RUN**。勝手に巨大 spec を足さない |
| fuzz | なし | jsonl 1 行パーサ（本文を残さない）。**NOT RUN** |

## diagnostics

`core::diagnostics::DiagnosticRing` は kind + 計数 + UTC ms のみ。path / token / header / JSONL 本文は型で持てない。この increment では collector へ未配線（配線は次 PR）。
