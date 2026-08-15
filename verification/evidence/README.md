# 検証証跡

* `pbt-counterexamples.regressions` は Proptest が縮小した原子性反例と replay token である。`scripts/run-verification.ps1 -Stage pbt-counterexample` は、その反例が実際に出力された場合だけ成功する。
* `branch-coverage.json` は `cargo +nightly llvm-cov --branch --all-targets --json` の機械出力である。ディレクトリ移動前の build artifact 由来で、旧 root のゼロ計測 source path が含まれる。監査で使う集計は現在の root `C:\\Codes\\tsuyoshi-otake\\run-dog` の `src/core` と `src/application` に限る。
* `mutants-shard-*` は初回57 mutantの shard 実行、`mutants-cpu-rerun` は初回 survivor を判別するテスト追加後の CPU 再実行である。`mutants/mutants.out` は中断した全件試行であり、結論の母集団には用いていない。
