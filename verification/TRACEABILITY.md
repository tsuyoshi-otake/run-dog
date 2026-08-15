# 要件・オラクル・実装・証跡の対応表

| 要件 / 故障モード | 独立オラクル / TLA Action・Property | 実装対応 | 実装テスト / 証跡 | 現判定 |
| --- | --- | --- | --- | --- |
| 一設定世代は全 field と Run desired state が一括で確定する | `AtomicCommitOracle::submit`; `Commit`, `AtomicCommit` | `app.rs:154–215`; `registry.rs:43–50`, `71–83` | `failure_injection_detects_partial_settings_commit_against_atomic_oracle`; `RunDogCurrentFieldAtomic.cfg` | FAIL |
| UI / API / persistence の成功状態が一致する | `DurableState`; `StartupAgreement` | `ports.rs:48–60`; `windows/mod.rs:182–202` | `api_integration_detects_run_value_then_settings_failure_split_commit`; startup TLC cfg | FAIL |
| field errorで部分更新を晒さない | failure時 oracle は旧 generation を保持 | `registry.rs:47–49` が結果を破棄 | field fault C2, PBT artifact | FAIL |
| 未検証・失敗 candidate は共有 state を変えない | `NoTerminalFailureCommit`; `FailureSafety` | Run entry は save より先 | `resource_exhaustion_*`; current failure TLC cfg | FAIL |
| 古い / 同一 generation は新しい確定を巻戻さない | generation + operation ID; `NoStaleWriteAfterNewer`, `EqualVersionSingleWinner` | durable generation無し | `stale_and_equal_generation_*`; stale/equal TLC cfg | FAIL |
| retry / duplicate は論理結果を一度しか反映しない | operation ID; `NoDuplicateCommit`, `AtMostOnceCommit` | retry ID無し | `retry_and_restart_*`; duplicate TLC cfg | FAIL |
| timeout / cancel 後の遅延成功は無視する | `Timeout`, `Cancel`, `LateSuccessIgnored`, `NoLateCommit` | timeout/cancel API無し | `RunDogCurrentLate.cfg`; 実装テスト N/A | N/A / risk |
| 3 read は単一 committed snapshot である | `BeginSnapshot`, `SnapshotConsistent` | `registry.rs:34–38` の逐次 read | `loader_observes_a_torn_snapshot_*`; snapshot TLC cfg | FAIL |
| delete済み state を古い処理が再生成しない | lifecycle `NoRecreateAfterDelete` | `RegCreateKeyExW` | recreate TLC cfg | FAIL |
| resource exhaustion時に UI は durable 成功を表示しない | commit ack required | `EffectPort::apply` は `void` | `resource_exhaustion_*` | FAIL |
| CPU usage は正しい境界とEMAを守る | closed-form percent / EMA | `core/cpu.rs:43–104` | C2 + `pbt_ema_matches_independent_closed_form_oracle`; mutation | PASS（この範囲） |
| GitHub Release は strictly newer published stable version のみ候補にする | version ordering oracle; `pbt_update_selection_never_downgrades` | `update.rs:164–208` | C2、固定seed 2,048-case PBT、`update_protocol_integration` | PASS（非ライブprotocol範囲） |
| asset URL は設定repository・tag・固定asset名に束縛される | URL/path contract | `update.rs:249–327`; `windows/update.rs:268–306` | URL C2、fake GitHub asset integration、mutation | PASS（非ライブprotocol範囲） |
| checksum不一致・stale release は installer を起動しない | fixed SHA-256 fixture oracle | `update.rs:213–245`; `windows/update.rs:306–367` | `integration_corrupt_or_stale_release_never_reaches_fake_installer` | PASS（fake依存先） |
| exit/cancel 後に新たなinstallerを起動しない | cancellation flag check | `windows/update.rs:128–207`, `401–491` | cancellation component test | PARTIAL（実WinHTTP cancellationは非ライブ未実行） |

「N/A / risk」は property が不要という意味ではない。現製品の public protocol に timeout / cancel / retry ID が無いため、具体的な実装呼出しとしての合否を検査できず、TLA+ で必要な境界を示した状態である。
