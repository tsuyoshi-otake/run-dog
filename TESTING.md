# テスト戦略

RunDog は実アプリケーションを常駐させずに検証できるよう、`core`、`application`、`windows` adapter を分離している。通常の Rust テストは Fake と一時ファイルを使う。インストーラーの実行試験は独立した `scripts/test-installer.ps1` で行い、クリーンな GitHub-hosted Windows runner に限定する。

## ISTQB コンポーネントテスト

ISTQB のコンポーネントテストとして、各コンポーネントをその外部依存から隔離し、仕様に対する入出力・状態・エラー処理を検証する。

| コンポーネント | 代表的なテスト設計技法 | 隔離する依存 |
| --- | --- | --- |
| `core::cpu` | 同値分割、境界値、C2 | FILETIME/OS CPU API |
| `core::memory` / `core::storage` | 同値分割、境界値、C2、PBT | GlobalMemoryStatusEx / SHGetDiskFreeSpaceExW |
| `core::sparkline` | 境界値、容量超過、非有限値 | Tray/GDI |
| `core::animation` | 同値分割、境界値、C2、PBT | Timer/Tray |
| `core::settings` / `theme` | 有効値・無効値の同値分割、PBT | Registry/OS theme |
| `core::usage` | 同値分割、境界値 | セッション jsonl / OAuth / WinHTTP |
| `application::App` | 状態遷移、決定表、C2 | すべての Win32 副作用 |
| `update` | release/version の同値分割、asset 契約、checksum 境界、C2、PBT | GitHub REST、WinHTTP、file hash、Inno Setup |
| `windows::icons` | 正常/短縮/不正 header のエラー推測 | GDI icon 作成 |
| `windows::cpu` / `tray` / `registry` / `usage` | インターフェース、境界値、コマンド同値分割、jsonl payload | GetSystemTimes、Shell、HKCU、セッションホーム、WinHTTP |

終了基準は、対象コンポーネントの正常系・境界値・異常系・状態遷移がテストされ、C2 の条件表に未到達条件がないこと、PBT の不変条件が 2,048 ケースで満たされることとする。

## C2（condition coverage）

`c2_` で始まるテストは、以下の複合判断の各原子条件を真/偽にする決定表である。

| 判断 | 真/偽を検証する条件 |
| --- | --- |
| CPU 差分 | counter regression、ゼロ total、`idle > kernel`、有効な差分 |
| メモリ使用率 | `total == 0`、`available > total`、0% / 100% / 中間値 |
| 速度制御 | 上昇/下降、ヒステリシス境界、上限の縮小/拡大、同値入力 |
| アプリ状態 | 初回開始/重複開始、pending startup、一致/不一致結果、成功/失敗、終了後イベント、UsageSample の変化/同一 |
| 設定/テーマ | `None`、有効値、無効値、System/Light/Dark の解決 |
| 更新 protocol | draft/prerelease、older/equal/newer version、asset 欠落/重複、cross-repository/tag URL、checksum 正常/不正/重複、latest 404、起動時 worker の重複防止 |
| Win32 converter | 有効/短縮/不正データ、既知/未知のトレイコマンド |

`cargo llvm-cov` を利用できる CI では、この決定表を condition/MC/DC 計測結果と照合する。Stable の通常テストは同じ決定表と PBT を実行する。

## 非ライブ結合テスト

`tests/app_integration.rs` は次の Fake のみを使用する。

- `FakeCpu`: 有限の `SystemTimes` キュー
- `FakeClock`: 単調なメモリ上の時刻
- `FakePlatform`: in-memory settings、tray、scheduler、startup registry、process launcher
- `FakeThemeSource`: 固定テーマ入力

これにより、起動、CPU 変化、アニメーション、テーマ、FPS、起動時実行の commit/rollback、Explorer 再起動、Task Manager effect、利用料スナップショット、終了を結合レベルで検証する。`tests/state_machine_pbt.rs` は同じく非ライブの event sequence PBT である。

更新判定は `src/update.rs` の独立 oracle で検証する。`tests/update_protocol_integration.rs`
は GitHub latest API、release asset、checksum、installer launcher をプロトコル互換の
in-memory fake で接続し、本番と共有する一度限りの起動時自動導入 gate を通して、既定オン時の
verified launch、設定オフ・確認中の設定変更・終了キャンセル時の導入抑止、現在版・確認失敗・
破損/stale artifact の launch 抑止を結合レベルで検証する。Win32 adapter が gate の成功判定を
`install_available` へ接続することも source contract で固定する。GitHub JSON decoder、WinHTTP、
download、SHA-256、ShellExecute、Inno Setup は実呼出ししない。release descriptor、
version、asset URL、checksum manifest の境界は component test と 2,048 ケースの PBT で
固定するため、test 実行がネットワークや installer を起動することはない。`windows::update`
の C2 テストは、active な Checking / Downloading / Launching 中に二つ目の check worker
を受け付けないこと、404（stable release 未公開）を endpoint failure と区別すること、
Inno Setup の無表示引数を固定することを検証する。加えて 8 concurrent claim の
コンポーネントテストで、開始権を得る worker がちょうど 1 件になることを確認する。

更新PBTは固定 seed `0x5EED_2026_0815_0001` を使い、縮小済みの反例は
`verification/evidence/update-pbt-counterexamples.regressions` に保存する。

## 実行

使用量コレクターの起動・再開は、Windows adapter の一時ディレクトリと
ファイルストアを使って検証する。使用量を含まない追記の確定位置が正常終了後も
保存されることを3回の再生成で確認し、未完了行だけは再読込を許容する。
`tick_at` に60秒刻みの時刻を渡すテストでは、30プロジェクトの探索が最後まで
進み、照合と本文読込が共通のファイル件数予算内に収まることを確認する。
File ID取得失敗はadapter境界で注入し、集計・保存済み位置・IDの保持、復旧後の
差分再開を検証する。旧schema 2のID欠落は移行時の再構築として診断し、
schema 3のID欠落だけでは月間集計を破棄しない。

診断の `files_opened` は先頭照合・File ID取得・本文読込・上限値の末尾読込で
成功したopenを数える。`integrity_probe_bytes` は実際に読んだ先頭バイト数、
`usage_parse_bytes` は未完了行や先読みを含む本文の実読込バイト数であり、
確定offsetの増分とは異なる。上限値の末尾読込は別の件数・バイト制限を持つ。

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
```

GitHub Actions の Verify は上記を **独立した step** として実行する。1 つの `pwsh` ブロックへ連結すると、後続 `cargo` が成功したとき先行失敗が `$LASTEXITCODE = 0` に上書きされ得る。ローカル一括実行は `.\scripts\run-verification.ps1 -Stage baseline`（各コマンド後に `Assert-LastExitCode`）を使う。Release は Verify job の成功を `needs` してから publish する。

実機の CPU / memory 測定はテストではなく、Release artifact を対象にした別の手動性能評価として扱う。

## 測定対象・診断・インストーラーの確認

`scripts/measure.ps1` は RunDog の PID を明示的に受け取り、標準の導入先または
このリポジトリの build 出力、製品情報、Cargo.toml と同じバージョンを確認する。
終了した PID、別製品、古い版は測定を開始しない。実行中も終了・PID の再利用を検出する。
`perf-scenarios.ps1 -Mode smoke` の `$PID` は PowerShell 自身を指していたため、
現在は `-ProcessId` が必須。`-Mode list` は常駐プロセスなしで使える。

```powershell
.\scripts\tests\measure-target.Tests.ps1
.\scripts\measure.ps1 -ProcessId <RunDogのPID> -ValidateTargetOnly
.\scripts\perf-scenarios.ps1 -Mode smoke -ProcessId <RunDogのPID> -SmokeSeconds 15
```

UsageCollector が所有するディレクトリ、キュー、重複排除集合、再試行、checkpoint、
provider worker の処理中状態は `DiagnosticSnapshot` の数値として参照できる。
正常なメッセージループ終了時に一度、既存の上限 64 KiB の
`%LOCALAPPDATA%\RunDog\diagnostics\termination.log` へ
`event=usage_diagnostics` として記録する。run ID と PID で終了履歴と照合でき、
セッション本文、パス、認証情報は追加しない。強制終了時の記録は保証しない。

Verify は Rust の静的解析・回帰テスト・依存監査に加え、測定対象の契約と
実インストーラーの新規導入、常駐中の再導入、v1.1.40 からの更新、
ショートカット、二重起動、削除時の所有データと他製品データの境界を確認する。
実行ログは CI artifact に 7 日保存する。詳しい環境制約と手順は
[`installer/README.md`](installer/README.md) を参照。8 時間の常駐試験や実ユーザーの
設定を使う手動試験の代わりにはならない。
