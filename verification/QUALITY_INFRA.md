# Phase 5 quality infra

親: #5 / 子: #23。ピン: `2c0494cdd7b69fe71234f5daf6c07fb98a5f22f5`。

実測の表は [`evidence/USAGE_GATES.md`](evidence/USAGE_GATES.md)。Authenticode は #1。全履歴 JSONL scanner はしない。

## いま回したもの

| 手段 | この session | 未実行 |
| --- | --- | --- |
| PBT | usage / checkpoint / JSONL no-panic / バッチ等価。#9 #14 #16 を worktree で再実行 | 分割 event の cent 合計（#10）、RFC3339（#19）、freshness（#20）はこの tree に関数が無い |
| Mutation | `usage_checkpoint.rs` 33 / 28 caught / 5 missed（等価） | usage.rs 全体、windows/usage.rs は時間が大きく **NOT RUN** |
| TLC | `RunDogUsageIngest` MaxIds=2、26 distinct。実装証明ではない | MaxIds≥3 の全探索、settings 3-actor |
| Fuzz | cargo-fuzz 無し。no-panic PBT で代替 | libFuzzer / AFL |
| Coverage | **NOT RUN**（再測定していない） | MC/DC は従来どおり NOT RUN |
| Soak | **NOT RUN** | 8h |

## diagnostics

`RUNDOG_DIAGNOSTICS=1` の明示パスだけ。常時 verbose は付けない。
