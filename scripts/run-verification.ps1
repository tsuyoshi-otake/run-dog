param(
    [ValidateSet('baseline', 'pbt-counterexample', 'coverage', 'mutation', 'tlc')]
    [string]$Stage = 'baseline'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repo

function Assert-LastExitCode([string]$step) {
    if ($LASTEXITCODE -ne 0) {
        throw "$step failed with exit code $LASTEXITCODE."
    }
}

switch ($Stage) {
    'baseline' {
        cargo fmt --check
        Assert-LastExitCode 'cargo fmt --check'
        cargo clippy --all-targets --all-features -- -D warnings
        Assert-LastExitCode 'cargo clippy'
        cargo test --all-targets
        Assert-LastExitCode 'cargo test'
    }
    'pbt-counterexample' {
        # This is a negative property: a non-zero test exit is expected only
        # when Proptest has actually produced the saved minimal counterexample.
        $env:PROPTEST_RNG_SEED = '20260815'
        $previousErrorActionPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $probeOutput = & cargo test --test persistence_protocol_verification `
            pbt_faulted_settings_write_must_not_expose_a_partial_generation -- --ignored
        $probeExitCode = $LASTEXITCODE
        $ErrorActionPreference = $previousErrorActionPreference
        $probeText = $probeOutput | Out-String
        $probeOutput | Write-Output

        if ($probeExitCode -eq 0 -or $probeText -notmatch 'minimal failing input:') {
            throw 'The atomicity counterexample was not demonstrated as expected.'
        }

        if (-not (Test-Path 'verification/evidence/pbt-counterexamples.regressions')) {
            throw 'The Proptest regression artifact was not saved.'
        }

        Write-Output 'Expected atomicity counterexample reproduced and saved.'
        # Avoid leaking cargo's expected non-zero status to a CI caller.
        $global:LASTEXITCODE = 0
    }
    'coverage' {
        # `cargo-llvm-cov 0.8.7` currently maps --mcdc to an obsolete nightly
        # spelling.  Branch data is machine-collected; C2 is audited in the
        # accompanying condition matrix.
        cargo +nightly llvm-cov --branch --all-targets --json `
            --output-path verification/evidence/branch-coverage.json
        Assert-LastExitCode 'cargo llvm-cov'
    }
    'mutation' {
        cargo mutants --file src/core/cpu.rs --file src/core/settings.rs `
            --file src/application/app.rs --output verification/evidence/mutants
        Assert-LastExitCode 'cargo mutants'
    }
    'tlc' {
        if ([string]::IsNullOrWhiteSpace($env:RUN_DOG_JAVA) -or
            [string]::IsNullOrWhiteSpace($env:RUN_DOG_TLA2TOOLS)) {
            throw 'Set RUN_DOG_JAVA and RUN_DOG_TLA2TOOLS before running TLC.'
        }

        $formal = Join-Path $repo 'formal'
        $config = Join-Path $formal 'RunDogProtocolTwoActors.cfg'
        $spec = Join-Path $formal 'RunDogProtocol.tla'
        $metadata = Join-Path ([IO.Path]::GetTempPath()) ('rundog-tlc-' + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $metadata | Out-Null

        & "$env:RUN_DOG_JAVA" -XX:+UseParallelGC -cp "$env:RUN_DOG_TLA2TOOLS" tlc2.TLC `
            -workers 1 -metadir $metadata -config $config $spec
        Assert-LastExitCode 'TLC reference model'
    }
}
