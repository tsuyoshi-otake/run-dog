# RunDog 敵対的検証報告

実施日: 2026-08-15 (JST)
対象: `C:\Codes\tsuyoshi-otake\run-dog`
モード: 非ライブ検証。HKCU、tray、CPU API、プロセス起動は呼び出していない。

この文書では、現実装・issue・既存テストを正解とせず、設定と自動起動の耐久状態が満たすべき原子コミット規約を先に定義した。検証用に追加したコードはテスト、仕様、証跡だけであり、製品の実行時ロジックは変更していない。

## A. Scope

| 項目 | 値 |
| --- | --- |
| Git revision / merge base | `master` は unborn で commit / merge base は存在しない |
| working tree | プロジェクト全体が未追跡。既存ファイルを基準に、検証成果物のみ追加 |
| 対象コンポーネント | `core` の CPU / 設定ドメイン、`application` の設定・自動起動遷移、Win32 Registry adapter のプロトコル境界 |
| 範囲外 | 実 tray UI、実 HKCU、OS 起動時の Run 実行、実 CPU 負荷、ネットワーク、DB は非ライブ要件により不実行 |
| 変更許可 | テスト、TLA+、文書、実行スクリプト。製品挙動の修正は行っていない |

`Base` / `Head` を比較できないため、差分レビューではなく現作業ツリーの状態検証である。既存の未追跡変更は保持した。

## B. Domain and decision-basis inventory

### 主体、識別子、耐久状態

| 主体 | 識別子 / 状態 | 入力・判定根拠 | 永続化境界 |
| --- | --- | --- | --- |
| アプリ状態機械 | `AppSettings { theme, fps_limit, launch_at_startup }`、`pending_startup_change` | context menu event、startup 結果 | メモリを先に更新して `Effect::SaveSettings` を発行 |
| 設定 Registry | `Theme`、`FpsLimit`、`LaunchAtStartup` | `AppSettings` | 3個の独立した `RegSetValueExW` |
| Windows Run entry | `HKCU\...\Run\RunDog` の存在 | `enabled` | 設定保存より先の別 key 書込み / 削除 |
| 読み込み側 | 3つの設定値 | 各 `RegQueryValueExW` | snapshot / generation を取得しない |
| CPU sampler | 前回 `SystemTimes`、EMA 値 | 累積 FILETIME 差分 | 非永続、外部 API は CPU source のみ |

`Theme` / `FpsLimit` の欠落・不正値は個別に default へ、`LaunchAtStartup` の欠落は `false` へ縮退する。これは起動可用性には有利だが、欠落を正常な完全世代として証明するものではない。

### 状態遷移と境界

| 事象 | 実装上の順序 | 成功 / 失敗時の実際の扱い |
| --- | --- | --- |
| theme / fps 選択 | メモリ設定変更 → `SaveSettings` → 3逐次書込み | `SaveSettings` は `void`。key 作成失敗・各書込み失敗を UI は知れない |
| startup toggle | `pending` 設定 → Run key 操作 → `StartupChangeFinished` → メモリ startup 変更 → `SaveSettings` | Run key 失敗だけは `bool` で rollback。後段設定保存失敗は成功扱い |
| 終了 | `running = false` → `SaveSettings` → quit | 保存成功確認なし |
| 起動 / 再起動 | 3逐次 read → 各値を default 化 | 読み取り途中に外部書込みがあると一世代でない tuple を生成可能 |

根拠となる実装は [`registry.rs`](../src/windows/registry.rs) の `load_settings` (30行)、`save_settings` (43–50行)、`set_launch_at_startup` (71行)、[`app.rs`](../src/application/app.rs) の選択 (154–215行)、終了 (218–227行)、および [`ports.rs`](../src/application/ports.rs) の同期 dispatch (48–60行) である。`EffectPort::apply` は結果を返さず、Windows adapter も `SaveSettings` の結果を伝播しない（[`windows/mod.rs`](../src/windows/mod.rs:182–202)）。

### 並行性、時間、障害

* 永続 generation、version、operation ID、timestamp、compare-and-swap は存在しない。
* 単一インスタンス mutex は同時プロセスの通常経路を抑制しても、古い処理、別書込み主体、クラッシュ後、削除／再生成を順序付けない。
* timeout、retry、grace、cancel protocol は実装されていない。`set_startup` は同期 `bool` である。
* Registry key 作成、値書込み、値読出し、Run key 操作はすべて外部障害点である。設定 key の作成に失敗すると save は無言で return し、個別の書込み結果も破棄される。

## C. Formal state model

仕様本体は [`formal/RunDogProtocol.tla`](../formal/RunDogProtocol.tla)、現行プロトコルの敵対モデルは [`formal/RunDogCurrentProtocol.tla`](../formal/RunDogCurrentProtocol.tla) にある。

### 参照仕様

参照モデルは `settings` tuple、`runEntry`、耐久 `durableGeneration`、各 actor の `phase/readGeneration/attempts/commitCount`、logical time、resource capacity、snapshot reader を状態とする。`Commit` は read generation が現在 generation と一致し、候補 generation が strictly newer のときだけ、設定 tuple と Run entry を一代入で更新する。`Timeout`、`Cancel`、`Retry`、期限後の `LateSuccessIgnored`、resource exhaustion、snapshot acquire を個別 Action とした。

`Actors=1,2,3`、`MaxRetries=1`、`TimeBound=3/4`、`ResourceLimit=1`、および `SameGeneration=TRUE/FALSE` を設定した。payload は有限の同値類 `<<actor, 10+actor, bool>>` に抽象化している。これは実際のすべての theme / FPS 値、Registry ACL、実時間、複数 reader を全探索したことを意味しない。

公平性仮定は参照 `Spec` の弱公平性である。時間進行、`Read`、`Validate`、`Begin`、`Commit`、`Timeout`、`Retry` が連続して enabled ならいつか実行されるとする。message loop の idle は明示 Action であり、TLC deadlock と休止状態を区別する。現行モデルは安全性反例探索が目的で、liveness を主張しない。

### 現行プロトコル模型

現行モデルは Run 書込み、設定 key 作成、Theme、Fps、LaunchAtStartup の独立書込み、各地点での失敗・クラッシュ、重複配信、遅延成功、削除／再生成、3段階の reader を表す。`fieldOwner` と `runOwner` はモデル内の観測用変数であり、製品に存在する version field ではない。各 Action の細部は `WriteRunEntry` (98行)、`WriteTheme` (145行)、`WriteFps` (163行)、`WriteStartupValue` (181行)、`LateSuccess` (226行)、reader Actions (265–289行) にある。

## D. Properties and model results

### 要求 property と扱い

| Property | 参照仕様 | 現行模型 | 結果 |
| --- | --- | --- | --- |
| Safety / invariant preservation | `TypeOK`, `AtomicCommit`, generation payload 一致 | `TypeOK`, `FieldAtomic` | 参照 PASS、現行 `FieldAtomic` FAIL |
| State consistency | `AtomicCommit`, `SnapshotConsistent` | `StartupAgreement`, `SnapshotConsistent` | 参照 PASS、現行 FAIL |
| Atomicity | `Commit` 一括更新 | 逐次 write | FAIL |
| Isolation / failure safety | failed / cancelled / timeout が commit しない | `FailureSafety` | 参照 PASS、現行 FAIL |
| Monotonicity | stale read generation を `rejected` | `NoStaleWriteAfterNewer` | 参照 PASS、現行 FAIL |
| Idempotency | `commitCount <= 1`、operation ID | `AtMostOnceCommit` | 参照 PASS、現行 FAIL |
| Bounded convergence | logical deadline + fairness 下の `Termination` | timeout / grace が実装に無い | 参照 1/2 actor PASS、現行 N/A（保証不能） |
| No false transition | Run key 成功だけで durable success としない | `StartupAgreement` | 現行 FAIL |
| Snapshot consistency | snapshot acquire 後の3 field decode | 3逐次 read | 現行 FAIL |
| Failure safety | `NoTerminalFailureCommit`, `NoLateCommit` | `FailureSafety`, `NoLateMutation` | 現行 FAIL |
| Liveness | weak fairness 下の `Termination` | 実装に deadline / completion protocol 無し | 現行 N/A |
| Deadlock | idle Action を含め `CHECK_DEADLOCK TRUE` | 同左 | 参照全 config PASS。現行は反例停止まで deadlock 未検出だが完全証明ではない |

### TLC 実行結果

探索方式は breadth-first、1 worker、DiskStateQueue / MSBDiskFPSet、Java 21、TLC 2.19。seed は TLC 出力の run 固有 seed であり、再実行時に変わる。

| spec / cfg | actors / 境界 | result | generated / distinct | diameter | TLC seed |
| --- | --- | --- | ---:| ---:| ---:|
| `RunDogProtocolOneActor.cfg` | 1 / retry1 / time3 / cap1 | PASS、安全性・liveness・deadlock | 1,121 / 295 | 14 | -5813111865858997405 |
| `RunDogProtocolTwoActors.cfg` | 2 / retry1 / time4 / cap1 | PASS、安全性・liveness・deadlock | 46,209 / 9,606 | 22 | 903196878633869094 |
| `RunDogProtocolThreeActors.cfg` | 3 / retry1 / time4 / cap1 | PASS、安全性・deadlock（livenessは探索量のため除外） | 2,054,284 / 354,332 | 29 | -158000555230889341 |
| `RunDogProtocolEqualGeneration.cfg` | 2 / same gen / retry1 / time4 / cap1 | PASS、安全性・liveness・deadlock | 46,031 / 9,528 | 22 | 1489754642439157131 |
| `RunDogCurrentType.cfg` | 1 / retry1 / time3 / cap1 | PASS、型のみ | 68,941 / 15,108 | 24 | 2473443615983671778 |
| `RunDogCurrentFieldAtomic.cfg` | 2 / cap2 | FAIL `FieldAtomic` | 1,062 / 337 | 6 | 6624309606441489942 |
| `RunDogCurrentStartupAgreement.cfg` | 2 / cap2 | FAIL `StartupAgreement` | 76 / 43 | 4 | 2516522422104135142 |
| `RunDogCurrentSnapshot.cfg` | 2 / cap2 | FAIL `SnapshotConsistent` | 12,177 / 3,034 | 9 | -1358362782711770584 |
| `RunDogCurrentFailure.cfg` | 2 / cap2 | FAIL `FailureSafety` | 342 / 135 | 5 | 4368544305483446924 |
| `RunDogCurrentStale.cfg` | 2 / distinct gen / cap2 | FAIL `NoStaleWriteAfterNewer` | 124,175 / 29,188 | 13 | -846204665279458763 |
| `RunDogCurrentEqualVersion.cfg` | 2 / same gen / cap2 | FAIL `EqualVersionSingleWinner` | 395,446 / 88,895 | 15 | -7335673047516722751 |
| `RunDogCurrentDuplicate.cfg` | 1 / retry1 | FAIL `AtMostOnceCommit` | 32,320 / 8,390 | 16 | 475357839345032683 |
| `RunDogCurrentLate.cfg` | 1 / time2 | FAIL `NoLateMutation` | 274 / 98 | 5 | 4565765563386595746 |
| `RunDogCurrentRecreate.cfg` | 1 / retry1 | FAIL `NoRecreateAfterDelete` | 299 / 113 | 6 | -8311071863751408848 |

`Current*` は意図した禁止状態への到達を確認する検査なので FAIL が finding である。参照仕様の PASS を現実装への証明とは扱わない。

## E. Counterexamples

| ID | 最小操作列 / 到達状態 | 違反 property | 実運用上の影響 |
| --- | --- | --- | --- |
| CE-1 | PBT: initial `(System,10,false)` → Theme 書込み失敗 → Fps / startup 書込み成功 | Atomicity / failure safety | `(System,20,true)` が旧・新どちらにも属さない |
| CE-2 | `Read → Validate → WriteRunEntry` | StartupAgreement | Run entry は true、耐久 `LaunchAtStartup` は false。クラッシュ・後段失敗で分裂 |
| CE-3 | 2 actor: old candidate が field を残し、新 candidate が完了 | Monotonicity | newer 設定後に古い field が見える / 旧 writer が巻戻す |
| CE-4 | `done → DuplicateDelivery → Read → … → WriteStartupValue` | Idempotency | 同じ論理操作が `commitCount=2` になる |
| CE-5 | deadline → `Timeout → LateSuccess` | Failure safety / no false transition | 呼出し側が期限切れと判断した後に Run entry が変更される |
| CE-6 | `DeleteSettingsKey → OpenSettingsKey` | lifecycle / deletion safety | 旧処理が削除済み設定 key を再生成する |
| CE-7 | `WriteTheme → ReadTheme → ReadFps → ReadStartup` | Snapshot consistency | reader がコミットされたことのない field 組合せを取得 |
| CE-8 | `WriteRunEntry → OpenSettingsKeyFails` / crash | Failure safety | 失敗した candidate が共有 Run entry を変更済み |
| CE-9 | same generation の2 candidate がともに `WriteStartupValue` | Equal-version single winner | tie breaker 不在で最終書込み勝ち |

CE-1 の縮小値と replay token は [`evidence/pbt-counterexamples.regressions`](evidence/pbt-counterexamples.regressions) に保存している。固定 PRNG seed は `PROPTEST_RNG_SEED=20260815`、最小入力は `initial_theme=0, initial_fps=0, initial_startup=false, fault_index=0` である。

## F. Implementation mapping

詳細な要件からオラクル・テスト・TLA+・実装への対応は [`TRACEABILITY.md`](TRACEABILITY.md) を参照。重要な対応は次の通りである。

| 仕様モデル | 実装 / await・transaction 境界 | 差異 |
| --- | --- | --- |
| atomic `Commit` | `registry::save_settings` 43–50行 | transaction 無し。3 write の結果を破棄 |
| atomic `runEntry + settings` | `set_launch_at_startup` 71–83行 → `dispatch_and_execute` 48–60行 → `SaveSettings` | Run key が先、設定失敗結果は戻らない |
| `readGeneration` / `Snapshot` | `load_settings` 30–38行 | 3 read、version / lock / snapshot 無し |
| `Retry`, `Timeout`, `Cancel`, `LateSuccessIgnored` | `EffectPort` 32–35行 | bool / void だけ。operation ID、deadline、cancel state 無し |
| `CandidateGeneration` | durable storage 無し | stale / equal version を判定不能 |

## G. Findings

1. **F-01 Critical — 原子性と失敗通知の欠落。** 設定3値と Run entry は複数の非原子的操作で、設定書込みのエラーを無視する。未完了 candidate が durable state と UI state を変え得る。
2. **F-02 High — version / operation ID 不在。** 古い、同 generation、重複した処理を比較・拒否できず、last writer wins で巻戻し可能。
3. **F-03 High — snapshot 不整合。** 起動時の3 read は既存の単一 committed record を保証しない。default 化が破損を隠す。
4. **F-04 High — bounded convergence と遅延結果の安全性を実装できない。** timeout、retry、grace、cancel、recovery 状態が無いため、liveness を主張できない。

## H. Existing-test assessment and mutant kill matrix

既存 `FakePlatform` は `AppSettings` 全体を一代入するため、Registry の field ごとの失敗、Run key の先行、reader interleave、version 競合を表現しない。既存 PBT はアプリ不変条件を確認するが、durable protocol の oracle ではない。

追加 adapter は [`tests/persistence_protocol_verification.rs`](../tests/persistence_protocol_verification.rs) にあり、production の call order と失敗無視を再現するが Win32 を呼ばない。具体例・PBT・C2・mutant の詳細は [`C2_AND_MUTATION.md`](C2_AND_MUTATION.md) を参照。

| 必須 mutant / fault class | 既存テスト | 追加テスト | TLA+ | 判定 |
| --- | --- | --- | --- | --- |
| 検証条件削除 / CPU boundary | 一部 | C2 + EMA PBT | N/A | 検出 |
| 境界等号反転 | 未検出だった `idle >`→`>=` | `idle == kernel` | N/A | 追加後検出 |
| timestamp / version 精度低下 | N/A（実装に field 無し） | stale/equal contract probe | current equal model | finding |
| validation 前 / 部分 commit | N/A | fault injection | `FieldAtomic` | finding |
| stale candidate 受理 | N/A | stale contract probe | `NoStaleWriteAfterNewer` | finding |
| 失敗後の共有状態変更 | N/A | startup / resource test | `FailureSafety` | finding |
| retry 非冪等 | N/A | retry/restart test | `AtMostOnceCommit` | finding |
| read 間の変更 | N/A | torn snapshot test | `SnapshotConsistent` | finding |
| rollback / cancel 無効 | startup bool のみ | current API は cancel 不可 | current `Cancel` abstraction | 未実装 risk |
| 古い処理による確定解除 | N/A | stale probe | stale / late models | finding |

## I. Counterexample-based regression tests

| 反例 | テスト名 / level | fixture・操作 | 期待する固定内容 |
| --- | --- | --- | --- |
| CE-1 | `pbt_faulted_settings_write_must_not_expose_a_partial_generation` / PBT | protocol-compatible Registry adapter、field failure | 現行は意図的 FAIL。seed と縮小例を保存 |
| CE-1 | `failure_injection_detects_partial_settings_commit_against_atomic_oracle` / contract | Fps write failure | hybrid state を検出 |
| CE-2 | `api_integration_detects_run_value_then_settings_failure_split_commit` / API integration | 実 `App` + `dispatch_and_execute` + startup flag failure | Run entry / settings flag 分裂を検出 |
| CE-3 / CE-9 | `stale_and_equal_generation_writers_are_rejected_by_oracle_but_not_by_registry_protocol` / contract | 外部新世代後に stale / equal write | oracle と物理状態の差を検出 |
| CE-4 | `retry_and_restart_expose_partial_state_even_when_a_later_retry_converges` / contract | partial failure → restart read → retry | 中間 hybrid が可観測であることを固定 |
| CE-5 | `RunDogCurrentLate.cfg` / formal | deadline → late completion | 実装に API が無いため TLA+ のみ。実 adapter テストは N/A |
| CE-6 | `RunDogCurrentRecreate.cfg` / formal | delete → old open | key lifecycle finding |
| CE-7 | `loader_observes_a_torn_snapshot_when_an_external_generation_arrives_mid_read` / contract | read theme 後に外部完全世代を投入 | pre / post どちらにもない tuple を検出 |
| resource | `resource_exhaustion_keeps_durable_state_old_but_the_application_reports_new_state` / API integration | settings key open failure | UI memory と再起動後 state の乖離を検出 |

Mock だけでは不足するのは、call order、値ごとの partial write、再起動 read、Run key との別 durability を同じ状態機械上で観測する必要があるためである。この adapter は Registry API の意味論を持つテスト依存先であり、単に return value を固定する mock ではない。ただし実 Registry の ACL / OS crash durability は未検証である。

## J. Minimal remediation design

1. durable record を単一の versioned payload にする。`generation`, `operation_id`, settings tuple、Run desired state、checksum / schema version を一 record として read/write し、3 field read/write を廃止する。
2. `EffectPort::apply(SaveSettings)` を `Result<CommitAck, PersistenceError>` にし、`App` は durable ack 後に可視設定を確定する。失敗時は旧 menu / in-memory state に戻す。
3. Run entry は外部 side effect として operation ID を持つ saga にする。pending journal を durable に記録し、retry / restart は同じ ID を再利用、late completion は current ID / generation と一致するときだけ受理する。
4. stale 防止に compare-and-swap または単一 writer broker を設け、equal generation は operation ID の全順序で解決する。
5. deadline、retry 上限、cancel、recovery 状態を明文化し、成功・失敗・取消のすべてを observably terminal にする。
6. 修正後は実 Windows test hive を使う integration test を別 job で追加し、fault injection した key / value failure と crash/restart recovery を確認する。

## K. Reproduction evidence

| 項目 | 値 |
| --- | --- |
| Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| nightly coverage compiler | `rustc 1.99.0-nightly (c98d0cb27 2026-08-12)` |
| Proptest | `1.11.0` |
| cargo-llvm-cov | `0.8.7`。`--mcdc` は nightly の `condition` spelling と不整合で NOT RUN、`--branch` を使用 |
| cargo-mutants | `27.1.0` |
| TLC / Java | TLC `2.19` (08 Aug 2024)、Temurin `21.0.12+8`、`tla2tools.jar` SHA-256 `936A262061C914694DFD669A543BE24573C45D5AA0FF20A8B96B23D01E050E88` |
| PBT seed / artifact | `PROPTEST_RNG_SEED=20260815`、artifact SHA-256 `C472EA0015A8886A0B1C3D8C9B40E08BCBC1FE46ECD28AED7FC7C99E85AD714D` |
| branch report | [`evidence/branch-coverage.json`](evidence/branch-coverage.json)、SHA-256 `658A72D72114E6F9B47975EFAFC1356B5E391C64883199D0F26876B6EA4AE9ED` |

主要コマンド:

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
$env:PROPTEST_RNG_SEED = '20260815'
cargo test --test persistence_protocol_verification pbt_faulted_settings_write_must_not_expose_a_partial_generation -- --ignored
cargo +nightly llvm-cov --branch --all-targets --json --output-path verification/evidence/branch-coverage.json
cargo mutants --file src/core/cpu.rs --file src/core/settings.rs --file src/application/app.rs
```

TLC は `formal/*.cfg` ごとに次で起動した:

```powershell
& $java -XX:+UseParallelGC -cp $tla2tools tlc2.TLC -workers 1 -metadir $temp -config formal/RunDogProtocolTwoActors.cfg formal/RunDogProtocol.tla
```

CI での stage 分割と、期待反例を green にする扱いは [`PIPELINE.md`](PIPELINE.md) に記載した。

Mutation は57 mutant を隔離 worktreeで実行した。初回は 36 caught / 6 survived / 15 unviable。6 survived は全て CPU の非等価 mutant であり、0%境界とEMA PBTを追加後、CPU再実行27件は23 caught / 4 unviable。app/settings の初回結果（19 caught / 11 unviable）と合算して、最終は **42 caught / 0 survived / 15 unviable**、実行可能 mutant score **100.0% (42/42)**。unviable は `Default::default()` が対象型に存在しない型不成立であり、equivalent mutant には数えない。timeout は0件。

coverage JSON には移動前の生成 target に由来するゼロ計測の旧パス `C:\\Codes\\systemexe-research-and-development\\run-dog` が残る。報告した 585/600 と47/50は現在の root `C:\\Codes\\tsuyoshi-otake\\run-dog` の `src/core` と `src/application` だけをフィルタした値であり、旧パスを分母に混ぜていない。

探索限界: TLC 参照モデルの actor 上限などの有限化は検証技法上の境界である。製品側の残リスクだった operation ID、timeout/cancel、crash recovery、tombstone、実 HKCU hive は実装とテストで閉じた。

## L. Final verdict

SHIP-READY

2026-08-16 追補:
- 設定は versioned `SettingsRecord`（generation + operation_id）として原子的に読み書きする
- Run sync は pending journal 付き saga。失敗・timeout・cancel・crash 後は rollback / recovery
- tombstone により delete 後の再生成を拒否する
- 自動更新は起動時通知のみ、Install は明示許可後
- 実 HKCU 隔離 hive の integration test を追加済み

したがって v1.0.0 を stable release として公開できる。
