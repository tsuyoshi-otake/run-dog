param(
    [ValidateRange(1, 100000)]
    [int]$Cases = 1024
)

$ErrorActionPreference = 'Stop'
$env:RUN_DOG_FUZZ_CASES = [string]$Cases
Write-Output "cargo-fuzz / libFuzzer: NOT USED (not installed on this toolchain)"
Write-Output "Deterministic bounded fuzz: seed 0x5EED_2026_0905_0003 cases=$Cases"
cargo test --offline --lib bounded
if ($LASTEXITCODE -ne 0) {
    throw 'bounded ingest fuzz failed'
}
