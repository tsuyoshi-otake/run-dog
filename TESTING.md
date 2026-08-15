# テスト戦略

RunDog は実アプリケーションを常駐させずに検証できるよう、`core`、`application`、`windows` adapter を分離している。テスト実行時に HKCU、タスクトレイ、Task Manager、実 CPU カウンター、実時計、ネットワークへ触れるテストはない。

## ISTQB コンポーネントテスト

ISTQB のコンポーネントテストとして、各コンポーネントをその外部依存から隔離し、仕様に対する入出力・状態・エラー処理を検証する。

| コンポーネント | 代表的なテスト設計技法 | 隔離する依存 |
| --- | --- | --- |
| `core::cpu` | 同値分割、境界値、C2 | FILETIME/OS CPU API |
| `core::animation` | 同値分割、境界値、C2、PBT | Timer/Tray |
| `core::settings` / `theme` | 有効値・無効値の同値分割、PBT | Registry/OS theme |
| `application::App` | 状態遷移、決定表、C2 | すべての Win32 副作用 |
| `update` | release/version の同値分割、asset 契約、checksum 境界、C2、PBT | GitHub REST、WinHTTP、file hash、Inno Setup |
| `windows::icons` | 正常/短縮/不正 header のエラー推測 | GDI icon 作成 |
| `windows::cpu` / `tray` / `registry` | インターフェース、境界値、コマンド同値分割 | GetSystemTimes、Shell、HKCU |

終了基準は、対象コンポーネントの正常系・境界値・異常系・状態遷移がテストされ、C2 の条件表に未到達条件がないこと、PBT の不変条件が 2,048 ケースで満たされることとする。

## C2（condition coverage）

`c2_` で始まるテストは、以下の複合判断の各原子条件を真/偽にする決定表である。

| 判断 | 真/偽を検証する条件 |
| --- | --- |
| CPU 差分 | counter regression、ゼロ total、`idle > kernel`、有効な差分 |
| 速度制御 | 上昇/下降、ヒステリシス境界、上限の縮小/拡大、同値入力 |
| アプリ状態 | 初回開始/重複開始、pending startup、一致/不一致結果、成功/失敗、終了後イベント |
| 設定/テーマ | `None`、有効値、無効値、System/Light/Dark の解決 |
| 更新 protocol | draft/prerelease、older/equal/newer version、asset 欠落/重複、cross-repository/tag URL、checksum 正常/不正/重複 |
| Win32 converter | 有効/短縮/不正データ、既知/未知のトレイコマンド |

`cargo llvm-cov` を利用できる CI では、この決定表を condition/MC/DC 計測結果と照合する。Stable の通常テストは同じ決定表と PBT を実行する。

## 非ライブ結合テスト

`tests/app_integration.rs` は次の Fake のみを使用する。

- `FakeCpu`: 有限の `SystemTimes` キュー
- `FakeClock`: 単調なメモリ上の時刻
- `FakePlatform`: in-memory settings、tray、scheduler、startup registry、process launcher
- `FakeThemeSource`: 固定テーマ入力

これにより、起動、CPU 変化、アニメーション、テーマ、FPS、起動時実行の commit/rollback、Explorer 再起動、Task Manager effect、終了を結合レベルで検証する。`tests/state_machine_pbt.rs` は同じく非ライブの event sequence PBT である。

更新判定は `src/update.rs` の独立 oracle で検証する。`tests/update_protocol_integration.rs`
は GitHub latest API、release asset、checksum、installer launcher をプロトコル互換の
in-memory fake で接続し、新版の verified launch と破損/stale artifact の launch 抑止を
結合レベルで検証する。GitHub JSON decoder、WinHTTP、download、SHA-256、ShellExecute、
Inno Setup は実呼出ししない。release descriptor、version、asset URL、checksum manifest
の境界は component test と 2,048 ケースの PBT で固定するため、test 実行がネットワークや
installer を起動することはない。

更新PBTは固定 seed `0x5EED_2026_0815_0001` を使い、縮小済みの反例は
`verification/evidence/update-pbt-counterexamples.regressions` に保存する。

## 実行

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo build --release
```

実機の CPU / memory 測定はテストではなく、Release artifact を対象にした別の手動性能評価として扱う。
