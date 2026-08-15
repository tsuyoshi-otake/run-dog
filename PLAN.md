# RunDog 実装計画

## 1. 目的と確定方針

RunCat の挙動を参考に、CPU 使用率に応じて Windows の通知領域で犬が走る Rust 製アプリケーション `RunDog` を作る。

- 同等機能の基準は、この PC で実行中の **RunCat v2.0.0** とする。
- 現行 RunCat 365 v3.6.0 の追加機能は、初期リリース後の候補として分離する。
- Windows 10 19041 以降、まず x86_64 を対象にする。
- UI フレームワークや非同期ランタイムを使わず、Rust から Win32 API を直接呼ぶ。
- 実行時スレッドは原則 1 本、待機中はブロッキング・メッセージループとする。
- 自動テストは C2、PBT、非ライブ結合テストで構成し、実レジストリや実タスクトレイへ副作用を出さない。
- Apache-2.0 の条件と RunCat への帰属を守りつつ、名称、犬画像、UI 文言は RunDog 独自のものにする。

## 2. 調査済み事実

### インストール済み RunCat

| 項目 | 確認値 |
| --- | --- |
| 実行ファイル | `C:\Apps\RunCat-x64\RunCat.exe` |
| File/Product Version | `2.0.0.0` / `2.0.0` |
| Product/Company | `RunCat` / `Takuto Nakamura` |
| SHA-256 | `9F739A3D5D89CB6624FE17EC6C5DC6AA3CBF4AB8C851DCDB020904BEC6B5FA15` |
| 署名 | Authenticode 未署名 |
| 起動方法 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` の `RunCat` |
| 実装基盤 | .NET 6、Windows Forms、System.Drawing、PerformanceCounter |
| 一致する上流 | タグ `2.0`、commit `09a33dabc6ed3c35b33a647fe986d3268273e5a0` |

ファイルの作成日時、バージョン、上流タグのリリース日時が一致しているため、実機比較にはタグ `2.0` のソースを使う。

### 上流リポジトリ

- 正式な移管先: <https://github.com/runcat-dev/RunCat365>
- 参照用クローン: `C:\Codes\runcat-dev\RunCat365`
- ライセンス: Apache License 2.0
- クローン時の main: `03b6e2b288c2df5df2433398f5547857bb4d0e2f`
- 現行プロジェクト版: 3.6.0、C# / Win32 / .NET 9

### RunCat v2.0.0 の機能

1. 多重起動防止。
2. 通知領域アイコンのフレームアニメーション。
3. CPU 使用率の 3 秒周期取得。
4. CPU 使用率に連動したアニメーション速度。
5. CPU 使用率のツールチップ表示。
6. Runner 選択（Cat、Parrot、Horse）。
7. System、Light、Dark のテーマ選択とシステムテーマ変更追従。
8. Default、10、20、30、40 相当の速度上限。
9. Windows ログイン時の自動起動切り替え。
10. ダブルクリックによる Task Manager 起動。
11. 設定保存と終了。

RunDog では Runner 選択を犬の 3 フレームへ置き換える。Runner の複数種類は製品目的上不要なので初期範囲外とする。

### 現行 v3.6.0 の追加機能

GPU/メモリを速度源にする機能、CPU/GPU/メモリ/温度/ストレージ/ネットワーク表示、最大 10/20/30/40 fps、カスタム Runner、多言語、Endless Game などが追加されている。これらは v2.0.0 同等版の性能目標を達成してから個別に採否を決める。

### 配置済み画像

| ファイル | 実体 | 寸法 | Pixel Format |
| --- | --- | --- | --- |
| `dark-dog-1.ico` | BMP | 32 x 32 | 32 bpp ARGB |
| `dark-dog-2.ico` | BMP | 32 x 32 | 32 bpp ARGB |
| `dark-dog-3.ico` | BMP | 32 x 32 | 32 bpp ARGB |

3 ファイルとも内容は異なる有効なアニメーションフレームだが、先頭は ICO ヘッダーではなく `BM` であり、拡張子だけが `.ico` になっている。実装時に正規 ICO へ変換し、元ファイルは素材として保持する。また Dark 用しかないため、Light 用は決定的なパレット変換で生成して目視確認する。

### RunCat の暫定性能ベースライン

同一 PC、論理 CPU 4 個、実行中の v2.0.0 を 30 秒、1 秒間隔で測定した。

| 指標 | 測定値 |
| --- | ---: |
| 1 コア基準 CPU 平均 | 1.2532% |
| マシン全体 CPU 平均 | 0.3133% |
| マシン全体 CPU 最大 | 1.5409% |
| Working Set 平均 | 44.590 MiB |
| Private Bytes | 24.891 MiB |
| Handles | 450 |
| Threads | 6 |

これは短時間の暫定値であり、最終比較ではウォームアップ後 10 分以上、同じ表示速度と負荷条件で再測定する。

## 3. 初期リリースの機能範囲

### 必須

- `Local\SystemExe.RunDog` の名前付き Mutex による多重起動防止。
- 3 フレームの犬アニメーションを通知領域へ表示。
- CPU 使用率を 0.0〜100.0% に正規化し、ツールチップへ表示。
- CPU 使用率に応じたアニメーション速度変更。
- 最大速度 10/20/30/40 fps の選択。省リソース優先の初期値は 20 fps。
- System/Light/Dark テーマ。System は OS の変更通知に追従。
- Windows ログイン時の自動起動切り替え。
- ダブルクリックで `taskmgr.exe` を直接起動。
- 設定保存、終了、Explorer 再起動後の通知アイコン復元。
- 日本語のメニュー文言。文字列は将来の多言語化に備えて一か所へ集約。

### 初期範囲外

- RunCat の Cat/Parrot/Horse 画像および同一 UI の複製。
- GPU/メモリをアニメーション速度源にする機能。
- 温度、ディスク、ネットワークの常時取得。
- カスタム Runner 編集画面。
- Endless Game。
- Store/MSIX 配布、アップデータ、ARM64。

## 4. 実装アーキテクチャ

### 構成

単一 Cargo package に、OS 非依存ライブラリと Windows バイナリを分ける。

```text
src/
  lib.rs
  core/
    cpu.rs            # FILETIME 差分、平滑化、0..100 クランプ
    animation.rs      # 負荷→fps/interval、ヒステリシス、フレーム循環
    settings.rs       # 設定モデル、既定値、検証
    theme.rs          # System/Light/Dark の解決
  application/
    app.rs            # Event を State と Effect へ変換する状態機械
    event.rs
    effect.rs
    ports.rs          # CPU、時計、Tray、設定、起動、Theme の境界
  windows/
    message_window.rs # 非表示ウィンドウと GetMessageW ループ
    tray.rs           # Shell_NotifyIconW、メニュー、TaskbarCreated
    cpu.rs            # GetSystemTimes
    registry.rs       # 設定、テーマ、自動起動
    icon.rs           # 埋め込みリソースと HICON の所有権
    shell.rs          # ShellExecuteW(taskmgr.exe)
  main.rs
tests/
  app_integration.rs  # すべて偽 Port を使う非ライブ結合テスト
  state_machine_pbt.rs
tools/
  icon_pack/          # BMP 検証、Light 版生成、正規 ICO 化
assets/
  source/             # ユーザー配置の原本
  generated/          # 検証済み ICO
scripts/
  measure.ps1         # 性能測定。自動テストとは分離
```

処理の中心は次の一方向にする。

```text
Win32 callback -> Event -> App::dispatch -> State + Effect -> Port adapter
```

`App::dispatch` から Win32 API を直接呼ばない。この境界により、結合レベルでも OS を動かさずに振る舞いを再現できる。

### 依存関係

- 実行時依存は原則 `windows-sys` のみ。
- `tokio`、`winit`、`tray-icon`、GUI フレームワーク、`serde` は使わない。
- PBT の dev-dependency に `proptest` を使う。
- モックライブラリは使わず、テスト専用の小さな手書き Fake を使う。
- アイコン変換に必要な依存はビルド/ツール側だけに閉じ込め、実行ファイルへリンクしない。

### Win32 実装

- `#![windows_subsystem = "windows"]` でコンソールを表示しない。
- 非表示ウィンドウ 1 個と `GetMessageW` によるブロッキングループを使う。
- `Shell_NotifyIconW` の `NIM_ADD/MODIFY/DELETE` と Version 4 を使用する。
- `TaskbarCreated` を受けて Explorer 再起動後にアイコンを再登録する。
- `SetCoalescableTimer` または通常の `SetTimer` を使い、専用スレッドを作らない。
- HICON は起動時に全フレームを 1 回だけロードし、終了時に 1 回だけ破棄する。
- Task Manager は PowerShell を経由せず `ShellExecuteW` で直接起動する。
- RunCat の Mutex、レジストリ値、設定ファイルは変更しない。

## 5. 省リソース設計

1. CPU 使用率は PDH/PerformanceCounter ではなく `GetSystemTimes` の FILETIME 差分から計算する。
2. サンプリング周期は 2 秒を初期値とし、初回値は表示に使わない。
3. 負荷は指数移動平均で平滑化し、速度帯にはヒステリシスを持たせる。
4. 速度は 5 fps から選択上限までの少数の帯へ量子化する。
5. 初期上限を 20 fps とし、40 fps はユーザーが明示的に選んだ場合だけ使う。
6. 速度帯が変わらない限りアニメーション Timer を再設定しない。
7. Tray の `NIM_MODIFY` は、フレームまたは実際に表示されるツールチップ文字列が変わる場合だけ呼ぶ。
8. 設定は `HKCU\Software\SystemExe\RunDog` に小さな値として保存し、常駐パーサーを持たない。
9. ログはデバッグビルドのみ。Release で常時 I/O を行わない。
10. Release は LTO、`codegen-units = 1`、`panic = "abort"`、シンボル除去を使う。CPU 優先で最適化し、見かけ上の Working Set を強制的に trim しない。

## 6. テスト戦略

### 6.1 C2（条件網羅）

- 対象は `core` と `application` の全条件、および Windows adapter から抽出した全判断ロジック。
- 複合条件は原子条件ごとの真/偽が最低 1 回現れる決定表をテストへ併記する。
- Nightly の LLVM condition/MC/DC instrumentation と `cargo-llvm-cov` を CI の補助ゲートとして使う。
- `core` と `application` は testable conditions 100% を必須とする。
- 生の FFI 呼び出しだけを数値ゲートから除外できるが、除外理由を coverage manifest に記録する。条件分岐を FFI glue 内へ残して除外することは禁止する。
- Stable の通常テストと Release ビルドは Nightly に依存させない。

### 6.2 PBT

`proptest` で少なくとも次を検証する。

- FILETIME の差分を任意倍率にしても CPU 比率が同じ。
- ゼロ差分、wrap/逆行相当、idle が total を超える入力でも panic せず 0.0〜100.0% に収まる。
- CPU 負荷が増えたとき、同じ状態条件では fps が低下しない。
- 生成される interval は常に正で、選択した最大 fps を超えない。
- ヒステリシス境界付近の微小揺れで速度帯が連続反転しない。
- 任意の正のフレーム数で index が範囲外にならず、N 回で一周する。
- 設定の encode/decode が round-trip し、不明値や欠損値は安全な既定値になる。
- System/Light/Dark の優先順位が常に一意に解決される。
- CPU ツールチップが Win32 の UTF-16 長制限を超えない。
- 任意の Event 列でも「Tray は最大 1 個」「終了後に更新 Effect を出さない」「Timer interval は正」を保つ。

CI は固定 regression seed を保存し、通常ケース数 2,048 を初期値とする。既知の境界値は PBT 任せにせず通常の例示テストにも固定する。

### 6.3 非ライブ結合テスト

`tests/app_integration.rs` で本物の `App`、状態機械、設定検証、速度計算を接続し、境界だけを Fake にする。

Fake 対象:

- `CpuSource`
- `Clock/Scheduler`
- `TrayPort`
- `SettingsStore`
- `StartupPort`
- `ThemeSource`
- `ProcessLauncher`

必須シナリオ:

1. 既定設定から起動し、Tray 登録と 2 種類の Timer 設定を行う。
2. 保存済みテーマ/fps を復元し、初期フレームへ反映する。
3. CPU サンプル列から速度帯、Timer 更新、ツールチップ更新が正しい順に出る。
4. 同じ速度帯では Timer 再設定を出さない。
5. ThemeChanged で正しいフレーム集合へ切り替え、設定を保存する。
6. 自動起動の成功、失敗、拒否時にメニュー状態を正しく commit/rollback する。
7. ダブルクリックで `taskmgr.exe` の起動 Effect を 1 回だけ出す。
8. `TaskbarCreated` で Tray を再登録する。
9. 不正設定から安全に既定値へ復旧する。
10. 終了時に Timer 停止、Tray 削除、設定保存、リソース解放を順序どおり行う。

禁止事項:

- 実 `HKCU` の読み書き。
- 実通知領域へのアイコン追加。
- 実 Task Manager の起動。
- 実 CPU 値や時刻への依存。
- ネットワーク、Store、外部サービスへの接続。
- テスト間で共有されるグローバル状態。

Win32 adapter はコンパイル検証と、構造体変換などの純粋関数テストを行う。実 OS 操作は自動テストに含めず、リリース候補の手動スモーク確認として明確に分離する。

### 6.4 通常の品質ゲート

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets`
- C2/MC/DC coverage job
- PBT job
- x86_64-pc-windows-msvc Release build
- `unsafe_op_in_unsafe_fn = deny`。unsafe は `windows` module 内に限定し、安全条件をコメントする。

## 7. 性能測定ゲート

性能測定は非ライブ結合テストとは別の、明示実行するベンチマーク工程とする。CI の機能テストへ混ぜない。

### 測定方法

1. Release バイナリを起動して 60 秒ウォームアップ。
2. 1 秒間隔で 10 分以上測定。
3. RunCat と RunDog を同じ PC、同じ時間帯、同じアニメーション上限で別々に測定。
4. CPU は「1 コア基準」と「マシン全体」の両方を記録。
5. Working Set、Private Bytes、Handles、Threads、GDI/User objects を記録。
6. 低負荷時、20 fps 上限時、テーマ変更連打後、8 時間 soak 後を分ける。
7. 外れ値だけでなく平均、P95、最大、開始/終了差を保存する。

### 初期合格予算

同じ 4 論理 CPU の PC、アイドル、20 fps 上限で次を目標にする。

| 指標 | RunDog 合格予算 |
| --- | ---: |
| マシン全体 CPU 平均 | 0.10% 以下、かつ RunCat 比 50% 以上削減 |
| マシン全体 CPU P95 | 0.25% 以下 |
| Working Set | 12 MiB 以下 |
| Private Bytes | 8 MiB 以下 |
| Threads | 2 以下 |
| Handles | 100 以下 |
| 8 時間後の Private Bytes 増加 | 1 MiB 未満 |
| 8 時間後の Handle 増加 | 継続的増加なし |

環境ノイズで絶対値を満たさない場合でも相対 50% 削減は維持する。未達時は ETW/WPA または同等の profiler で Tray 更新、Timer wakeup、設定 I/O、GDI resource の順に原因を分解し、根拠なしに閾値だけを緩和しない。

## 8. 実装フェーズと完了条件

### Phase 0: リポジトリと法務の土台

- このディレクトリを Cargo/Git リポジトリ化する。
- 自動更新の GitHub Releases を同梱 token なしで参照するため、初期配布では `systemexe-research-and-development` 配下の source/release GitHub リポジトリを public とする。private source と public release repository を分離する配布は、cross-repository publish credential を要するためこの初期 workflow の対象外とする。
- Rust toolchain、formatter、lint、Release profile を固定する。
- `LICENSE`、`NOTICE` または `THIRD_PARTY_NOTICES.md` に RunCat365 と Apache-2.0 の帰属を記載する。
- 画像原本を `assets/source` へ整理し、正規 ICO と Light 版を生成する。

完了条件: 空の RunDog が単一 EXE として起動・終了し、画像検証が自動化されている。

### Phase 1: OS 非依存 core/application

- CPU 差分計算、平滑化、速度帯、ヒステリシス、フレーム循環を実装。
- Event/State/Effect と Port を実装。
- 設定既定値と検証を実装。
- C2 の決定表と PBT を先に作成する。

完了条件: Win32 API なしで全状態遷移を再現でき、C2/PBT が通る。

### Phase 2: Win32 adapter と機能同等

- message window、Tray、context menu、CPU、theme、registry、Task Manager 起動を実装。
- Explorer 再起動、DPI、終了時リソース解放を処理。
- RunCat のレジストリ値や Mutex と競合しないことを確認。

完了条件: 必須機能チェックリストを満たし、unsafe 境界が Windows module に限定される。

### Phase 3: 非ライブ結合テスト

- 手書き Fake の TestRig を実装。
- 必須 10 シナリオと失敗経路を実装。
- Event 列 PBT を追加。
- C2 coverage の不足条件をゼロにする。

完了条件: OS 副作用なしで結合テスト、C2、PBT、lint がすべて成功する。

### Phase 4: 計測と最適化

- `scripts/measure.ps1` を作り、RunCat/RunDog を同じ条件で測る。
- Timer、Tray 更新、メモリ/Handle 所有権を profiler で確認。
- 性能予算を満たすまで量子化、更新抑制、所有権を調整する。

完了条件: 生データ付きの比較結果が保存され、全性能予算を満たす。

### Phase 5: 配布準備

- x64 Release 単一 EXE、README、設定場所、アンインストール手順を用意。
- SHA-256 を生成。
- private repository の CI で全ゲートを実行。
- 手動スモーク確認は自動テスト結果と混ぜず、別チェックリストへ記録する。

完了条件: 新規 Windows ユーザー環境で展開可能な Release artifact と再現可能な検証記録がある。

## 9. 主なリスクと対策

| リスク | 対策 |
| --- | --- |
| `.ico` が実際は BMP | 原本を保持し、変換ツールで header/dimension/alpha を検証して正規 ICO を生成する |
| Light 用画像がない | 決定的パレット変換、golden hash、目視確認を行う |
| Tray 更新が CPU の支配要因になる | 初期 20 fps、速度帯量子化、ヒステリシス、変更時のみ NIM_MODIFY |
| Win32 FFI がテスト不能になる | 判断ロジックを core/application へ置き、adapter を薄くする |
| C2 と単なる branch coverage を混同する | 原子条件の決定表を正本とし、condition/MC/DC report は補助証跡にする |
| 自動起動テストがユーザー環境を壊す | 結合テストは Fake のみ。実 HKCU を自動テストから禁止する |
| RunCat の商標・画像と混同される | RunDog 独自名称/犬画像を使用し、RunCat は inspiration/attribution としてのみ記載する |
| 現行 v3.6.0 の機能まで scope が膨らむ | 初期基準を実機 v2.0.0 に固定し、追加機能を性能達成後の別 milestone にする |

## 10. Definition of Done

- v2.0.0 基準の必須機能がすべて動作する。
- RunCat の実データ、レジストリ、プロセスへ変更を加えていない。
- `core` と `application` の C2 条件網羅が 100%。
- 指定 PBT と非ライブ結合テストが安定して成功する。
- 自動テストは実 OS 副作用、時刻、CPU 値、ネットワークに依存しない。
- Release の CPU、メモリ、Thread、Handle が性能予算内。
- 8 時間 soak で資源リークがない。
- Apache-2.0 帰属、独自画像、README、ビルド/テスト/計測手順が揃っている。
- x86_64 Release artifact の SHA-256 と検証記録がある。
