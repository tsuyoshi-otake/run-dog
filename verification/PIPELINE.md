# 検証パイプライン

`scripts/run-verification.ps1` は CI runner でそのまま使える stage runner である。実 Registry、tray、CPU API は呼ばない。

| stage | CI の終了条件 | 推奨頻度 |
| --- | --- | --- |
| `baseline` | format、clippy、全通常テストが成功 | 各変更。CI Verify は同等コマンドを独立 step で実行し、Release は Verify 成功を `needs` する |
| `pbt-counterexample` | 原子性の**期待された反例**が縮小値付きで出る | 各変更 |
| `coverage` | branch JSON が生成され、通常テストが成功 | 各変更または nightly |
| `tlc` | 2 actor の参照モデルが PASS | 各変更 |
| `mutation` | mutation job が完走。score / survivor を artifact として判定 | nightly / release candidate |

```powershell
.\scripts\run-verification.ps1 -Stage baseline
.\scripts\run-verification.ps1 -Stage pbt-counterexample
.\scripts\run-verification.ps1 -Stage coverage

$env:RUN_DOG_JAVA = 'C:\\path\\to\\java.exe'
$env:RUN_DOG_TLA2TOOLS = 'C:\\path\\to\\tla2tools.jar'
.\scripts\run-verification.ps1 -Stage tlc

# Usage ingest and scheduling reference models, including fault counterexamples.
.\scripts\run-usage-ingest-tlc.ps1 -Model all -Mode all
```

`RunDogUsageScheduling.tla` は、古い pending が旧64件相当の探索範囲を超える場合と、pending 読み取りが通常の byte budget を使い切る間に当日ファイルへ追記される場合を有限化する。修正版では当日イベントの最初の tick での観測、古いファイルの進捗、1 tick あたり最大2ファイルの読み取りを確認する。旧 prefix 探索と hot restat 予約なしの設定は、どちらも `TodayWithinOneTick` の反例を出す必要がある。前提は2つの file-work 枠、期限到来済みの restat/retry、1回の読み取りに収まる当日イベントである。TLC の PASS はこの参照モデルの到達状態についての結果であり、Rust 実装との対応は `src/windows/usage.rs` の2つの component regression で確認する。

`pbt-counterexample` は現行実装の適合テストではない。最小反例が現れることを確認して 0 で終わる負の検証 stage であり、反例が消えた場合は「修正された」か「probe が壊れた」かを人が判別する。修正後には期待値を反転し、通常の green PBT に昇格させる。

TLC の 3 actor 全探索、全 `RunDogCurrent*.cfg` の反例探索、及び全 mutation は探索量・実行時間が大きいため release candidate で必須、通常 pull request では scheduled job とする。現行プロトコル model の FAIL は既知 finding の再現であって、参照仕様の PASS と混同してはならない。
