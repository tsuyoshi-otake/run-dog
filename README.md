# RunDog

<p align="center">
  <img src="assets/rundog-dog.png" alt="RunDog の犬" width="360">
</p>

<p align="center">通知領域で元気に走る、軽量な CPU モニター。</p>

## タスクバー上の RunDog

実際に RunDog を起動した Windows タスクバーです。右端の通知領域に犬が表示されます。

![RunDog が表示された Windows タスクバー](assets/rundog-taskbar.png)

`RunDog` は、Windows の通知領域で CPU 使用率に応じて犬の 3 フレーム・アニメーションを表示する、低負荷の Rust 製常駐アプリケーションです。ローカルに導入されていた RunCat 2.0.0 の振る舞いを参考にした独立実装であり、RunCat のコードや画像を含みません。

## 機能

- `GetSystemTimes` の累積値差分による全体 CPU 使用率の表示
- CPU 使用率に応じた 5–40 FPS のアニメーション（既定の上限は 20 FPS）
- System / Light / Dark テーマ、10 / 20 / 30 / 40 FPS 上限の右クリックメニュー
- サインイン時の起動、ダブルクリックによる Task Manager 起動、Explorer 再起動後の tray 再登録
- GitHub Releases の stable release を起動時に一度だけ非同期確認し、右クリックメニューから更新
- 単一インスタンス、単一 message-loop thread、GUI framework / polling thread / 常時 I/O なし

配備された `dark-dog-*.ico` は ICO ヘッダーではなく 32×32 ARGB BMP です。RunDog は起動時に検証して `HICON` を一度だけ作成し、Dark 用は原画、Light 用は alpha を保った反転色を使います。アニメーション中にファイルを読み直しません。

## ビルド

Windows の MSVC Rust toolchain で実行します。

```powershell
cargo build --release
```

成果物は `target\release\RunDog.exe` です。実行するとコンソールを表示せず、通知領域に常駐します。終了はアイコンの右クリックメニューから行います。

## 自動更新と配布

RunDog は起動時に一度だけ、GitHub Releases の latest published stable release
を短命なバックグラウンド worker で確認します。常時ポーリングはしません。
新しい版があれば右クリックメニューに `Install RunDog vX.Y.Z` が現れます。
選択すると installer を 16 KiB 単位でディスクへ stream し、SHA-256 sidecar
を照合してから Inno Setup を起動します。開始済みの worker は終了後に保持されず、
通常の tray loop の CPU 使用率には影響しません。

ダウンロードと installer 起動は、未署名の executable を無操作で実行しないよう、
メニューからの明示操作が必要です。検出は自動、導入は one-click です。Inno Setup
は既存の RunDog を閉じ、同じ per-user install path を更新して再起動します。

この仕組みは token を埋め込まないため、配布先の GitHub repository と Release assets
は public である必要があります。private repository の Releases は認証なしの client
から取得できません。Release binary には CI で `RUN_DOG_UPDATE_REPOSITORY` として
実際の `owner/repository` を埋め込みます。

Authenticode の証明書・署名は使用しません。そのため Windows SmartScreen 等の警告が
出る可能性があります。SHA-256 sidecar は転送破損・取り違えを fail-closed にする
検査であり、コード署名の代替や、GitHub repository 自体が侵害された場合の真正性保証
ではありません。

ローカルで unsigned installer を作るには Inno Setup 6 を導入してから次を実行します。

    .\scripts\build-installer.ps1 -Version 0.1.0

作成物は `dist\RunDog-Setup-x64.exe` と
`dist\RunDog-Setup-x64.exe.sha256` です。タグ `vX.Y.Z` を push すると
[release workflow](.github/workflows/release.yml) が同じ asset pair を GitHub Release
へ公開します。asset contract は [installer/README.md](installer/README.md) にあります。

## リソース方針

CPU サンプリングは 2 秒ごと、静止に近い時のアニメーションは 5 FPS（200 ms）です。CPU 使用率により速度が変わったときだけ timer を再設定し、設定はユーザー操作または正常終了時だけ保存します。更新確認は起動時とユーザーによる再確認時だけで、release metadata は最大 1 MiB、checksum は最大 16 KiB、installer は最大 100 MiB として stream 処理します。性能目標と、実機でのみ行う測定手順は [PLAN.md](PLAN.md) と [scripts/measure.ps1](scripts/measure.ps1) に記載しています。数値目標はまだ実機測定で主張していません。

## テスト

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo llvm-cov --all-targets --json --summary-only --output-path target\coverage-summary.json
```

[TESTING.md](TESTING.md) は、C2（condition coverage）、PBT、ISTQB コンポーネントテスト、Fake のみを使う非ライブ結合テストの範囲を定義しています。更新 protocol のテストも fixture の release metadata と checksum manifest だけを使います。テストは HKCU、実トレイ、Task Manager、実 CPU API、ネットワークに触れません。

## 範囲

初期版は、ローカルで解析した RunCat 2.0.0 と同じ CPU 表示・アニメーション・テーマ・速度上限・起動時実行・Task Manager 導線を対象とします。GPU/メモリ可視化、ゲーム、Runner 種別の拡張はこの版の対象外です。

## 謝辞

RunDog は [RunCat365](https://github.com/runcat-dev/RunCat365) を機能上の参考にしています。詳細は [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) を参照してください。
