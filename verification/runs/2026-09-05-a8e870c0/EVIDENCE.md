# 独立レビューと証拠分類

調査ピン: `a8e870c0f1cad7cf4e431f6d563c8670c5626868`（2026-09-05、`git fetch origin` 後）。

重大度と証拠状態は別軸。既存 PR の「直した」記述は現行 main に無い限り未統合として扱う。

## 既存 Issue / PR 対応

| 対象 | 現行 main での独立判定 | 証拠状態 | 既存成果物 |
| --- | --- | --- | --- |
| #6 / PR #8 CI 連結失敗飲み | `.github/workflows/verify.yml` L21-27 と `release.yml` L20-25 が 1 pwsh ブロック。`scripts/build-installer.ps1` L39 は `cargo build --release` の `$LASTEXITCODE` 未検査。ローカルで `exit 1` の後 `exit 0` が `$LASTEXITCODE=0` | REPRODUCED | PR #8 は `2c0494c` 起点。現行 main 未取り込み。Verify check は FAILURE（当時）。採否は再 base 後に判断 |
| #7 / PR #9 reasoning 二重加算 | `src/windows/usage.rs` L1382-1385 が `output_tokens.saturating_add(reasoning_output_tokens)` | OBSERVED_STATIC | PR #9 OPEN。main 残 |
| #7 / PR #10 nanos 丸め | 現行はイベント単位 `cost_cents` 経路（checkpoint v3）。分割丸めの fail-first は本セッション未実行 | NEEDS_RUNTIME_REPRO | PR #10 OPEN |
| #7 / PR #11 long-context 二重加算 | `TokenUsage::processed_input_tokens` が通常 bucket と `long_context_*` を両方足す（`src/core/usage.rs` L22-29） | OBSERVED_STATIC | PR #11 OPEN |
| #7 / PR #12 未知モデル Silent Loss | 現行 apply 経路の `cost_cents is None` でイベント全体を落とすかは本セッションで未トレース完了 | NEEDS_RUNTIME_REPRO | PR #12 OPEN |
| #7 cache write | 実 JSONL 形状の固定サンプルなし。料金を捏造しない | NEEDS_CONTRACT | 修正なしが妥当 |
| #7 service tier | `service_tier` を要求と処理で混同している静的証拠なし | NOT_REPRODUCED | 欠陥として未採用 |
| #13 / PR #14 Codex SM | 現行は行ごとの `last_token_usage` 加算。snapshot/replay の fail-first は未実行 | OBSERVED_STATIC（加算） / NEEDS_RUNTIME_REPRO（replay 一式） | PR #14 CONFLICTING。位置依存列拡張のまま独立 merge 禁止 |
| #15 / PR #16 不完全 JSONL | `read_appended` は改行無しで `skip_incomplete_line`。改行無しでも `new_offset = offset + scanned`。不完全 prefix が committed progress になり得る | OBSERVED_STATIC | PR #16 CONFLICTING。vNext store へ移植。独立 merge 禁止 |
| #15 / PR #18 旧月 dir | 探索範囲の静的確認は未完了。毎 tick 全再帰は現行にも無い | NEEDS_RUNTIME_REPRO | PR #18 CONFLICTING。再検証してから |
| #17 / PR #19 RFC3339 | `parse_timestamp` は壁時計を UTC 秒として合成。offset / `Z` を読まない（`src/windows/usage.rs` L1848-1863） | OBSERVED_STATIC | PR #19 OPEN |
| #17 / PR #20 freshness | `LimitWindow::effective` は reset 後を `used_tenths: 0` にする（`src/core/usage.rs` L57-67）。表示が 0% になるかは flyout 配線の実行が必要 | OBSERVED_STATIC（effective） / NEEDS_RUNTIME_REPRO（UI） | PR #20 OPEN。#26 マージ後に差分再評価 |
| #17 / PR #21 volume | `read_storage_status` が `"C:\\"` 固定（`src/windows/storage.rs` L8-16） | OBSERVED_STATIC | PR #21 OPEN |
| #17 / PR #22 flyout | 現行 `position_near_icon` のプライマリ clamp は本セッションで未完読 | NEEDS_RUNTIME_REPRO | PR #22 CONFLICTING（#26 と flyout 衝突の可能性） |
| #23 / PR #24 quality | 現行 tree に AGENTS.md / DiagnosticRing / RunDogUsageIngest.tla は無い（PR #24 側）。本ピンでの PBT/mutation/TLC/fuzz は未実行 | NOT RUN | PR #24 CONFLICTING。資産再利用。Replay 等を足してから gate 化 |
| #25 / PR #26 Fable / Banked | `origin/main` にマージ済み（`2b9ace9` + release `a8e870c`） | 取り込み済み | 重複 merge するな |
| #1 Authenticode | usage / CI に混ぜない | REJECTED（本プログラムのブロッカーとしては） | #1 のまま |
| #2 / #3 | CLOSED。資格情報削除やピン変更はしない | REJECTED（再オープンしない） | — |
| #4 22H2 hover | 本セッション未再現 | NEEDS_RUNTIME_REPRO | OS 互換。usage PR に混ぜない |

## 現行 main で新たに確定した静的欠陥（既存 PR が主対象でない）

| ID | 内容 | 重大度（仮） | 証拠状態 | メモ |
| --- | --- | --- | --- | --- |
| MSG-COLLISION | `WM_SHOW_TRAY_MENU` と `UPDATE_REQUEST_EXIT_MESSAGE` がどちらも `0x8000 + 2`。WndProc は tray menu を先に見て return する | 高（更新インストールが Exit を投稿してもメニューが開く） | OBSERVED_STATIC | 定数を 1 個ずらして終わりにしない。`messages.rs` 一元化 + 一意性テスト。子 Issue 新設 |
| REG-8K | `write_string` に 8KiB 上限なし。`read_string` は `length > 8192` で None。save 成功後に load 不能 → Silent Loss / 空 rebuild | 高 | OBSERVED_STATIC | 子 Issue 新設。byte サイズを oracle に |
| REG-HANDLE | `load_usage_checkpoint_at` は `open_key` のあと `read_string(...)?`。失敗時 `close_key` しない | 中 | OBSERVED_STATIC | 同上 |
| CATCHUP-PERSIST | `persist_checkpoint_if_needed` は `self.catch_up` 中に return。途中 resume 無し | 高 | OBSERVED_STATIC | Phase 3 store。#15 に接続 |
| MONTH-ROLLOVER | `apply_checkpoint` は `month_start` 不一致で checkpoint 全体を捨てる | 高 | OBSERVED_STATIC | 月跨ぎだけで履歴 replay し得る。#15/#13 に接続。独立 Issue は作らず本文更新 |

## 棄却 / 非再現

| 仮説 | 状態 | 理由 |
| --- | --- | --- |
| 全履歴 JSONL を毎 tick 再帰スキャンする | REJECTED | 契約違反。hot / bounded discover のみ |
| Banked Reset を第三の reset 時刻として扱う | REJECTED | #26 実装方針と一致。現行も回数 |
| Authenticode を usage PR に混ぜる | REJECTED | #1 |
| Claude/Codex ユーザーデータを直接消す / 書き換える | REJECTED | 不変条件 |
| service tier 欠陥 | NOT_REPRODUCED | 現行静的読取では要求と処理の混同なし |
| cache-write 料金 | NEEDS_CONTRACT | 実 fixture なし |

## 完成度改善（欠陥ではない）

- required status checks / main 保護が無い（`gh api .../protection` → 404）。単独 maintainer を壊さない範囲で ruleset を試す
- Verify が 1 job 内の連結 step。失敗単位を step 分割すれば十分（独立 job は過剰になり得る）
- 診断に prompt / token を出さない契約のテストが現行に無い

## 未実行（NOT RUN）

- GHA 上での fmt-only 失敗 e2e
- タグ Release の `needs: verify` e2e
- installer 実機インストール / アンインストール
- 8h soak / cargo-fuzz / llvm-cov / TLC / cargo mutants（本ピン）
- 実機 tray / マルチモニタ / RDP / DPI 切替
- 実 Claude/Codex JSONL のライブ走査（秘密を Issue/fixture に入れない）
