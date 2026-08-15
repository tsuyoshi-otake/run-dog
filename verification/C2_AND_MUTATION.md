# C2 とミューテーション分析

## C2（原子条件）

「C2」はここでは各原子条件が真・偽を少なくとも一度支配する condition coverage として測定した。native MC/DC は `cargo-llvm-cov 0.8.7 --mcdc` が nightly `rustc 1.99.0-nightly` の受理値 `condition` と不整合で失敗したため、PASS と称していない。機械採取した branch coverage は [`evidence/branch-coverage.json`](evidence/branch-coverage.json) を参照。

| 対象条件 | false を固定する例 | true を固定する例 | テスト |
| --- | --- | --- | --- |
| `current.idle.checked_sub(previous.idle)` | monotonic idle | idle counter regression | `c2_cpu_delta_validation_exercises_every_boolean_condition` |
| kernel / user `checked_sub` | monotonic kernel / user | kernel / user counter regression | 同上 |
| `kernel.checked_add(user)` | 通常の有限差分 | `u64::MAX + 1` overflow | 同上 |
| `total == 0` | positive interval | unchanged counter | 同上 |
| `idle > kernel` | `idle == kernel`（0% valid）と normal busy | idle が kernel を超える | 同上 |
| `pending_startup_change.is_some()` | 未 pending toggle | toggle 二重送信 | `app::c2_startup_results_cover_busy_mismatch_failure_and_success` |
| `pending_startup_change != Some(enabled)` | matching success / failure | mismatched result | 同上 |
| setting key open | normal open | `OpenSettingsKey` fault | `c2_each_persistence_write_has_explicit_success_and_failure_outcomes` |
| Theme / Fps / startup field write | write success | 各 field で一度ずつ failure | 同上 |
| Run key open / write | success | open failure、後段 startup field failure | `c2_startup_key_success_failure_and_later_settings_failure_are_distinct` |
| update version / release state | strictly newer stable | draft / prerelease / equal / older / malformed tag | `update::c2_release_selection_*` |
| update asset binding | exact repository/tag/asset path | missing / duplicate asset、wrong repository / tag / suffix / backslash | `update::c2_release_asset_path_*` |
| update checksum | one 64-hex entry | non-hex / wrong-length / duplicate / extra data | `update::c2_checksum_manifest_*` |
| 起動時更新 gate | Idle / Current / Available / Failed からの開始 | Checking / Downloading / Launching 中の二重開始。8 concurrent claim は winner 1 件 | `windows::update::c2_startup_update_gate_*`; `component_startup_update_gate_allows_exactly_one_concurrent_claim` |
| latest endpoint status | 200（body を decode） | 404（stable 未公開）、401 / 500（失敗） | `windows::update::c2_latest_release_status_*` |

範囲 `src/core` + `src/application` の branch coverage は **47/50 (94.0%)**、line coverage は **585/600 (97.5%)**。移動前の生成 target に由来するゼロ計測の旧パスは JSON に残るため、現在の root `C:\\Codes\\tsuyoshi-otake\\run-dog` に限定して集計した。未カバー3 branch は主に Windows 非ライブ境界ではなく、`ports.rs` の loop / pattern と animation の補助分岐である。数値を C2 / MC/DC と読み替えてはいけない。

### GitHub Release 更新protocolの追加計測

`src/update.rs` の stable release 選択、asset URL binding、checksum manifest は固定seed
`0x5EED_2026_0815_0001` の2,048-case PBT、C2決定表、および非ライブ GitHub/installer
結合testで検査した。nightly `cargo llvm-cov --branch` の機械採取値は **51/58 branch
(87.9%)**、line **313/335 (93.4%)** だった。これは `#[cfg(test)]` を含むLLVM branch
集計であり、C2/MC/DCの達成率ではない。`--mcdc` は `cargo-llvm-cov 0.8.7` が発行する
`-Z coverage-options=mcdc` と、導入済みnightlyが受理する `condition` の不整合により
**NOT RUN**。したがってMC/DC達成とは主張しない。

### v1.0.0 の起動時更新確認（通知のみ）

2026-08-15 に `cargo +nightly llvm-cov 0.8.7 --all-targets --branch` を現行コードで
実行した。`src/windows/update.rs` は **120/579 lines、16/57 functions、4/84 branches**
である。これは adapter の WinHTTP / file / ShellExecute を非ライブ方針で隔離しているためで、
この数値を C2 達成率として扱わない。追加した C2 決定表は上記の state gate と latest status
の純粋条件を対象に全行を通したが、live download、hash、installer launch、キャンセル競合の
網羅を意味しない。

起動時は stable release の有無を確認してメニューへ通知するだけであり、Install はユーザー
操作後にだけ進む。従って以下の 100.0% mutant score は `src/update.rs` を含む従来の対象
範囲だけの値であり、Windows update adapter の mutation score としては **NOT RUN** である。

## ミューテーション

対象は `src/application/app.rs`, `src/core/cpu.rs`, `src/core/settings.rs`。`cargo-mutants 27.1.0` を4 shardで隔離実行し、初回の survivor に対する独立EMAテスト・PBTを追加後、CPUを再実行した。

| 範囲 | mutant | caught | survived | unviable | timeout |
| --- | ---:| ---:| ---:| ---:| ---:|
| `application/app.rs` | 29 | 18 | 0 | 11 | 0 |
| `core/settings.rs` | 1 | 1 | 0 | 0 | 0 |
| `core/cpu.rs`（再実行） | 27 | 23 | 0 | 4 | 0 |
| `update.rs`（最終再実行） | 51 | 45 | 0 | 6 | 0 |
| **合計** | **108** | **87** | **0** | **21** | **0** |

実行可能 mutant score は **87 / (87 + 0) = 100.0%**。unviable 21件は、mutator が `Default::default()` を返して対象戻り値型に `Default` が無いためビルド不能になったもの。これは「テストが検出した」でも「等価 mutant」でもない。

初回に survived だった6件は全て非等価である。次の判別入力を追加し、CPU再実行で全て caught になった。

| 初回 survivor | 非等価の根拠 | 追加 oracle |
| --- | --- | --- |
| `idle > kernel` → `idle >= kernel` | `idle == kernel` は valid な0%だが mutant は `None` | 0% boundary example |
| EMA `+` → `-` | raw 20→80、alpha=.25 は35、mutantは5 | closed-form EMA example / PBT |
| EMA `*` → `+` | 同入力で35ではなく clamp後80.25等 | 同上 |
| EMA `(raw-smoothed)` の `-` → `/` | 同入力で21 | 同上 |
| EMA `(raw-smoothed)` の `-` → `+` | 同入力で45 | 同上 |
| `latest` → `None` | 最新EMA値35が観測不能 | `latest()` assertion |

equivalent mutant は0件と判定した。検証していない `windows/registry.rs` の mutation はこの score の母集団に含めていない。そこは実装が Windows-only で、今回の安全性 finding は protocol adapter とTLA+で評価した。修正後は test hive を用いた別 mutation / integration job が必要である。
