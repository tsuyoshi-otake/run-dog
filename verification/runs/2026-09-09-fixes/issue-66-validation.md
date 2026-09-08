# Issue #66 修正・検証記録

Outcome: **fixed**

対象: https://github.com/tsuyoshi-otake/run-dog/issues/66

## 問題と維持すべき動作

保存先の文字列比較だけでは、usage store またはその祖先に設置された junction / reparse point を通して、Claude/Codex のユーザーデータへ書き込めた。通常の保存に加え、起動時の世代整理、旧ディレクトリ移行・削除も対象だった。

不変条件は、設定された provider ディレクトリへ store 操作を到達させないこと。正常な保存、原子的な世代公開、旧世代からの復旧、世代数制限、旧保存先からの移行は維持する。

## 修正

`FileUsageStore` の共通ファイル境界で、祖先の no-follow 検証とハンドル保持、最終実体・provider 別名の照合を行う。子ディレクトリの作成も含め、以後の操作は保持した親ハンドルを基準にする。公開・削除も検証済みハンドルを使い、仮ファイルは排他的な新規作成とする。葉の reparse point と複数 hardlink は拒否する。

collector で拒否する前に移行が実行される経路もあるため、provider 設定を store の生成・移行より前に渡す。個々の `GenerationBlobs` メソッドにも境界を適用する。旧ディレクトリの整理では再帰削除を使わない。

この範囲は、単なる永続化直前のパス検査では守れない作成・読込・整理・移行を、同じ境界で扱うために必要だった。

変更ファイル:

- `Cargo.toml`: Windows のハンドル相対 API の機能指定
- `src/windows/mod.rs`: 境界モジュール登録
- `src/windows/usage.rs`: provider 設定を store 初期化前に渡す
- `src/windows/usage_store.rs`: 保存・読込・移行・整理と回帰テスト
- `src/windows/usage_store_path.rs`: 共通境界と DOS 別名の回帰テスト

## 順序付き検証

1. 構文・静的検証
   - `cargo fmt --check`: PASS
   - `cargo clippy --all-targets --all-features -- -D warnings`: PASS
   - `git diff --check`: PASS
2. 原因・代替入力の再現と正常系
   - `cargo test --lib windows::usage_store::tests`: 16 PASS
   - `cargo test --lib component_directory_creation_stays_with_parent_after_dos_alias_retarget -- --nocapture`: 1 PASS
   - store 構築後の root/ancestor junction、provider の別名と大文字小文字差、仮ファイル・公開先の hardlink、旧保存先と整理対象の junction を検証した。provider の sentinel と保存済みデータは不変だった。
   - 検証後の root に `FSCTL_SET_REPARSE_POINT` で junction を挿入した。ハンドル相対の読込・削除・作成は provider に到達せず、列挙も元のディレクトリを対象とした。reparse を除去した後は同じハンドルで保存・読込が成功した。
   - 独立レビューで DOS デバイス別名の再割り当てによる作成先変更が指摘された。隔離した一意の別名で旧方式の絶対パス作成が provider 側へ向かうことを実行確認し、修正後の親ハンドル相対作成が元の安全なディレクトリに留まることを確認した。テストの別名は終了時に解除する。
   - 通常の世代保存・復旧、世代数制限、旧保存先の移行と整理も成功した。
3. リポジトリ全体
   - `cargo test --all-targets --quiet`: 327 PASS、0 FAIL、1 IGNORE
   - ignore は既存の手動用テスト（実際にインストールされた Claude/Codex の JSONL を読むもの）。今回の境界検証は一時フィクスチャで実行した。
   - `cargo build --release`: 既存 `target/release/RunDog.exe` の削除がアクセス拒否になった。
   - `cargo build --release --target-dir target/verification-20260909`: PASS。既存の実行ファイルを変更せず、同じ release profile で別の出力先にビルドした。

## 結論と限界

報告された junction 迂回と、hardlink・検証後の reparse 挿入・DOS 別名再割り当てを含む代替経路で provider のデータが変化しないことを実行確認した。正常な保存・復旧・移行・世代整理は既存テストで維持を確認した。独立レビューは一巡実施し、指摘された作成経路を修正・再検証した。

今回の Windows 環境とローカルの隔離フィクスチャによる検証であり、すべてのファイルシステムや Windows バージョンを実機検証したものではない。別件 #4 の hover 修正について、Windows 11 22H2 実機での UI 確認は未実施。
