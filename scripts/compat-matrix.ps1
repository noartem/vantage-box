<#
.SYNOPSIS
    Builds a compatibility matrix of Vantage Box across sing-box versions.

.DESCRIPTION
    Downloads several sing-box releases from GitHub, runs the same probe set
    against each, and writes the result to test-results/: compat-matrix.json
    and compat-matrix.md. At the end it prints the version range derived from
    the measurements — that is what should be written to SINGBOX_MIN /
    SINGBOX_MAX_EXCLUSIVE (src-tauri/src/clash/client.rs).

    Each version is checked in isolation: a separate sing-box process,
    non-standard ports (19090/19080), a config without TUN — i.e. no admin
    privileges and no network-stack interference. The script stops nothing: an
    already-running sing-box keeps running.

    Downloaded binaries are cached, so a repeat run is faster.

.PARAMETER Minors
    How many latest minor branches to check. Default 6.

.PARAMETER Versions
    An explicit comma-separated list of versions instead of selecting by minor.

.PARAMETER Cache
    Where to put the downloaded binaries. Default is a temp folder.
    Useful to point at another drive if the system one is low on space.

.EXAMPLE
    ./scripts/compat-matrix.ps1

.EXAMPLE
    ./scripts/compat-matrix.ps1 -Versions 1.11.15,1.12.9,1.13.16
#>
[CmdletBinding()]
param(
    [int]$Minors = 6,
    [string]$Versions,
    [string]$Cache
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$resultsDir = Join-Path $repo 'test-results'
New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null

$stamp = Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'
$log = Join-Path $resultsDir "compat_$stamp.log"

$probeArgs = @('--out', $resultsDir)
if ($Cache) { $probeArgs += @('--cache', $Cache) }
if ($Versions) {
    $probeArgs += @('--versions', $Versions)
} else {
    $probeArgs += @('--minors', "$Minors")
}

Push-Location (Join-Path $repo 'src-tauri')
# cargo writes progress to stderr; with ErrorActionPreference=Stop Windows
# PowerShell would treat that as a fatal error.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # An example, not a bin: an extra binary in the package trips the Tauri
    # bundler, and the wrong app ends up in the installer.
    & cargo run --release --example compat-probe -- @probeArgs 2>&1 |
        ForEach-Object { $_.ToString() } |
        Tee-Object -FilePath $log
    $code = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previousPreference
    Pop-Location
}

Write-Host ''
Write-Host "Log:      $log"
Write-Host "Matrix:   $(Join-Path $resultsDir 'compat-matrix.md')"
exit $code