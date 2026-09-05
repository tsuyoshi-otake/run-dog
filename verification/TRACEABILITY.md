# 要件・オラクル・実装・証跡の対応表

| 要件 / 故障モード | 独立オラクル / TLA Action・Property | 実装対応 | 実装テスト / 証跡 | 現判定 |
| --- | --- | --- | --- | --- |
| 一設定世代は全 field と Run desired state が一括で確定する | `AtomicCommitOracle` / `Commit` | `SettingsRecord` v2 + `execute_commit` | persistence PBT / live hive | PASS |
| UI / API / persistence の成功状態が一致する | `StartupAgreement` | settings 先行 → Run sync → rollback | `run_failure_rolls_settings_*` | PASS |
| field errorで部分更新を晒さない | failure 時旧 generation 保持 | 単一 payload + journal | `pbt_faulted_settings_write_*` | PASS |
| 未検証・失敗 candidate は共有 state を変えない | `FailureSafety` | pending journal + rollback | resource / run failure tests | PASS |
| 古い generation は新しい確定を巻戻さない | `NoStaleWriteAfterNewer` | expected_generation CAS | stale / live hive | PASS |
| snapshot は単一 committed record | `SnapshotConsistent` | 単一 `SettingsRecord` read | loader / live reload | PASS |
| resource exhaustion 時 UI は durable 成功を表示しない | commit ack | `SettingsCommitFinished` | `resource_exhaustion_*` | PASS |
| retry / duplicate は一度しか反映しない | `AtMostOnceCommit` | `last_operation_id` | `duplicate_operation_id_*` / live hive | PASS |
| timeout / cancel 後の遅延成功は無視する | `LateSuccessIgnored` | `CommitGate` + deadline | `timeout_*` / `cancel_*` | PASS |
| crash 後は journal から回復または rollback | recovery saga | `recover_pending` on startup | crash recovery tests / live hive | PASS |
| delete 済み state を古い処理が再生成しない | `NoRecreateAfterDelete` | `Lifecycle=tombstoned` | tombstone tests / live hive | PASS |
| 実 Registry でも同じ契約 | live hive oracle | `RegistryStore` + isolated HKCU | `registry_hive_integration` | PASS |
| 起動時更新は通知のみ、Install は明示許可後 | permission oracle | `check_for_updates` + tray Install | update integration | PASS |
| exit/cancel 後に新たな installer を起動しない | launch-gate | cancel + launch_gate | update component tests | PASS（非ライブ） |
| CPU usage 境界と EMA | closed-form oracle | `core/cpu.rs` | C2 + EMA PBT | PASS |
| GitHub Release は strictly newer stable のみ | version oracle | `src/update.rs` | C2 + PBT | PASS |
| Codex `token_count` の同一 snapshot / rate-limit 再通知は二重計上しない | `NoDoubleCount` / `decide_codex_event` | `src/core/codex_usage.rs` + collector watermark | `component_codex_identical_snapshot_*` / `component_codex_rate_limit_only_*` | REPRODUCED → 修正 |
| Codex 累積 `total` は last だけ加算 | independent last/total fixture | `decide_codex_event` | `component_codex_cumulative_usage_*` | REPRODUCED → 修正 |
| replay / resume 再通知は不変 | `ReplayInvariance` | previous `total` watermark | `component_codex_replay_*` / `component_codex_resume_*` / PBT | REPRODUCED → 修正 |
| fork / subagent の inherited snapshot は baseline | ccusage / openai/codex#18023 | `IgnoreInheritedBaseline` | `component_codex_fork_*` | REPRODUCED → 修正 |
| プロセス再起動後も続きの turn だけ加算 | `RestartEquivalence` | checkpoint optional `file=` totals | `component_codex_process_restart_*` / PBT | REPRODUCED → 修正 |
| 同一 token tuple でも `total` が進む別 request は加算 | 非 tuple-dedupe oracle | `total` 比較のみ | `component_codex_same_tuple_*` | REPRODUCED（仕様通り二重ではない） |
| `total` 欠落時は replay 保護を自称しない | incomplete identity | count `last` only | `component_codex_missing_total_*` | UNCERTAIN（書き換えない） |
| 遅延・逆順の小さい `total` を replay として落とす | stale-snapshot oracle | なし（reset と区別不能） | `component_codex_out_of_order_*` | NOT_REPRODUCED as defect |

単一インスタンス mutex と generation / operation ID CAS により、通常経路の多数プロセス競合は抑止する。実 OS crash の瞬間耐久性は Registry の非揮発書込みに依存し、journal 回復で観測可能な分裂を解消する。
