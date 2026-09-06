# Requires -Version 5.1
# Manual Authenticode check. Not wired into Verify/Release while SignPath
# approval is pending. After approval, Release should call this and fail
# closed on anything other than Valid.

param(
    [Parameter(Mandatory = $true)]
    [string] $Path
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path -LiteralPath $Path)) {
    throw "Installer not found: $Path"
}

$signature = Get-AuthenticodeSignature -LiteralPath $Path
Write-Output ("Status={0}" -f $signature.Status)
if ($signature.SignerCertificate) {
    Write-Output ("Subject={0}" -f $signature.SignerCertificate.Subject)
}

if ($signature.Status -ne "Valid") {
    throw "Authenticode status is $($signature.Status). Expected Valid after SignPath."
}
