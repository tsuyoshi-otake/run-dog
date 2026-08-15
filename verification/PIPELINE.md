# 検証パイプライン

`scripts/run-verification.ps1` は CI runner でそのまま使える stage runner である。実 Registry、tray、CPU API は呼ばない。

| stage | CI の終了条件 | 推奨頻度 |
| --- | --- | --- |
| `baseline` | format、clippy、全通常テストが成功 | 各変更 |
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
```

`pbt-counterexample` は現行実装の適合テストではない。最小反例が現れることを確認して 0 で終わる負の検証 stage であり、反例が消えた場合は「修正された」か「probe が壊れた」かを人が判別する。修正後には期待値を反転し、通常の green PBT に昇格させる。

TLC の 3 actor 全探索、全 `RunDogCurrent*.cfg` の反例探索、及び全 mutation は探索量・実行時間が大きいため release candidate で必須、通常 pull request では scheduled job とする。現行プロトコル model の FAIL は既知 finding の再現であって、参照仕様の PASS と混同してはならない。
