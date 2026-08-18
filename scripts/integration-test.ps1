<#
.SYNOPSIS
    Runs the Vantage Box integration test against a real sing-box.

.DESCRIPTION
    The test is isolated from the working system:
      * it starts a separate sing-box process and kills only it;
      * it listens on ports 19090 and 19080, not the standard ones;
      * it works with a config WITHOUT TUN — the network stack is not touched,
        no administrator privileges are needed;
      * all state is kept in a temp folder.

    The script does NOT stop or delete anything: an already-running sing-box
    keeps running. The binary path is detected automatically, including from an
    already-running process — but the process itself is never touched.

    The run result is saved to test-results/.

.PARAMETER SingBox
    Path to the sing-box binary. If not specified, it is found automatically.

.EXAMPLE
    ./scripts/integration-test.ps1
#>
[CmdletBinding()]
param(
    [string]$SingBox
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$resultsDir = Join-Path $repo 'test-results'
$workDir = Join-Path $env:TEMP 'vantage-box-integration'

function Resolve-Shim {
    param([string]$Path)

    # Scoop puts a shim wrapper in PATH rather than the binary itself. Running it is unsafe:
    # the test kills its own child process, and with a shim the child would be the shim itself —
    # the real sing-box would be left orphaned.
    $shim = [System.IO.Path]::ChangeExtension($Path, '.shim')
    if (Test-Path -LiteralPath $shim) {
        $target = (Get-Content -LiteralPath $shim | Where-Object { $_ -match '^\s*path\s*=' } | Select-Object -First 1)
        if ($target -match '=\s*"?([^"]+)"?\s*$') {
            $real = $Matches[1].Trim()
            if (Test-Path -LiteralPath $real) { return (Resolve-Path -LiteralPath $real).Path }
        }
    }
    return $Path
}

function Resolve-SingBox {
    param([string]$Explicit)

    if ($Explicit) {
        if (-not (Test-Path -LiteralPath $Explicit)) {
            throw "File not found: $Explicit"
        }
        return (Resolve-Path -LiteralPath $Explicit).Path
    }

    if ($env:VANTAGE_BOX_TEST_SINGBOX -and (Test-Path -LiteralPath $env:VANTAGE_BOX_TEST_SINGBOX)) {
        return (Resolve-Path -LiteralPath $env:VANTAGE_BOX_TEST_SINGBOX).Path
    }

    $onPath = Get-Command 'sing-box' -ErrorAction SilentlyContinue
    if ($onPath) { return (Resolve-Shim $onPath.Source) }

    # The binary managed by Vantage Box itself.
    $managed = Join-Path $env:APPDATA 'vantage-box\bin\sing-box.exe'
    if (Test-Path -LiteralPath $managed) { return $managed }

    # Last resort: get the path from an already-running process.
    # We only read the property — the process is never touched.
    try {
        $running = Get-Process -Name 'sing-box' -ErrorAction Stop |
            Where-Object { $_.Path } |
            Select-Object -First 1
        if ($running) { return $running.Path }
    } catch {
        # No process — not a problem, just keep going.
    }

    throw @'
Could not find the sing-box binary.
Specify the path explicitly:  ./scripts/integration-test.ps1 -SingBox "C:\path\sing-box.exe"
'@
}

$binary = Resolve-SingBox -Explicit $SingBox

# A clean work folder for every run: leftovers from a previous run must not
# affect the result.
if (Test-Path -LiteralPath $workDir) {
    Remove-Item -LiteralPath $workDir -Recurse -Force
}
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null

$stamp = Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'
$log = Join-Path $resultsDir "integration_$stamp.log"
$latest = Join-Path $resultsDir 'latest.log'

$header = @"
Vantage Box — integration test
time:       $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
sing-box:   $binary
work dir:   $workDir
ports:      Clash API 19090, mixed 19080 (non-standard, so they don't clash with the working sing-box)
"@

Write-Host $header
$header | Set-Content -LiteralPath $log -Encoding UTF8

$env:VANTAGE_BOX_TEST_SINGBOX = $binary
$env:VANTAGE_BOX_CONFIG_DIR = $workDir

Push-Location (Join-Path $repo 'src-tauri')
# cargo writes progress to stderr, and with ErrorActionPreference=Stop Windows
# PowerShell treats that as a fatal error before the command even finishes.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # --nocapture: all the test's println! output must reach the log.
    # --test-threads=1: there's only one test, but this makes output order predictable.
    # ToString: cargo writes progress to stderr, and without casting to a string these
    # lines would end up in the log as expanded ErrorRecords.
    & cargo test --test live_singbox -- --nocapture --test-threads=1 2>&1 |
        ForEach-Object { $_.ToString() } |
        Tee-Object -FilePath $log -Append
    $code = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previousPreference
    Pop-Location
    Remove-Item Env:\VANTAGE_BOX_TEST_SINGBOX -ErrorAction SilentlyContinue
    Remove-Item Env:\VANTAGE_BOX_CONFIG_DIR -ErrorAction SilentlyContinue
}

$verdict = if ($code -eq 0) { 'RESULT: success' } else { "RESULT: failure (code $code)" }
$verdict | Tee-Object -FilePath $log -Append | Write-Host

Copy-Item -LiteralPath $log -Destination $latest -Force
Write-Host "Log: $log"
Write-Host "Same: $latest"

exit $code