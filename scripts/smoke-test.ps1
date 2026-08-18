<#
.SYNOPSIS
    Checks that the app starts and brings up the tray, hotkeys, and window.

.DESCRIPTION
    "The process didn't crash" says nothing about the tray icon or global hotkeys:
    they silently fail if something goes wrong. On startup the app prints a line like

        vantage-box startup tray=ok hotkeys=ok window=shown settings=ok

    and this script checks exactly that.

    The app runs for a few seconds and then stops. The sing-box service is not
    touched: Vantage Box does not start it itself.

    Note: during the run the app occupies the global hotkeys from settings.json
    (by default Ctrl+Alt+P and Ctrl+Alt+O). After it stops they are released.

    A debug build pulls the frontend from the Vite dev server (devUrl in
    tauri.conf.json), so the script starts it on its own when needed and stops it
    after the run. A release build uses the already-bundled files and needs no
    server.

.PARAMETER Seconds
    How long to wait for the self-test to finish. Default 40.

.PARAMETER Release
    Check the release build (with the bundled frontend) instead of debug.
#>
[CmdletBinding()]
param(
    [int]$Seconds = 40,
    [switch]$Release
)

$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$resultsDir = Join-Path $repo 'test-results'
New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null

$profile_ = if ($Release) { 'release' } else { 'debug' }

# target-dir may be overridden in .cargo/config.toml.
$targetRoot = Join-Path $repo 'src-tauri\target'
$override = Join-Path $repo 'src-tauri\.cargo\config.toml'
if (Test-Path -LiteralPath $override) {
    $line = Get-Content -LiteralPath $override | Where-Object { $_ -match 'target-dir' }
    if ($line -match '=\s*"([^"]+)"') { $targetRoot = $Matches[1] }
}

$exe = Join-Path $targetRoot "$profile_\vantage-box.exe"
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Binary not found: $exe. Build it for the '$profile_' profile first."
}

function Test-Port {
    param([int]$Port)
    try {
        $client = [System.Net.Sockets.TcpClient]::new()
        $client.Connect('127.0.0.1', $Port)
        $client.Close()
        return $true
    } catch {
        return $false
    }
}

# A debug build fetches the frontend from the dev server. Without it the window
# comes up empty — a silent failure: the process is alive, the UI is not.
$vite = $null
if (-not $Release -and -not (Test-Port -Port 1420)) {
    Write-Host 'starting the Vite dev server on 1420...'
    $vite = Start-Process -PassThru -WindowStyle Hidden -FilePath 'npm.cmd' `
        -ArgumentList 'run', 'dev' -WorkingDirectory $repo

    $deadline = (Get-Date).AddSeconds(60)
    while (-not (Test-Port -Port 1420)) {
        if ((Get-Date) -gt $deadline) {
            & taskkill /T /F /PID $vite.Id 2>&1 | Out-Null
            throw 'Vite dev server did not come up within 60s'
        }
        Start-Sleep -Milliseconds 300
    }
}

$stamp = Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'
$log = Join-Path $resultsDir "smoke_$stamp.log"
$out = Join-Path $env:TEMP 'vantage-box-smoke-out.log'
$err = Join-Path $env:TEMP 'vantage-box-smoke-err.log'

"Vantage Box — smoke test`ntime:    $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`nbinary: $exe" |
    Tee-Object -FilePath $log | Write-Host

# --self-test makes the app open the popup, wait for a signal from its webview,
# and exit on its own.
$process = Start-Process -PassThru -FilePath $exe -ArgumentList '--self-test' `
    -RedirectStandardOutput $out -RedirectStandardError $err

$exited = $process.WaitForExit($Seconds * 1000)
# The app must exit on its own: if it hangs, the self-test didn't reach the end,
# and that is a result too.
$alive = -not $exited
if ($alive) {
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit(5000) | Out-Null
}

# Only stop the server we started ourselves.
if ($vite) {
    & taskkill /T /F /PID $vite.Id 2>&1 | Out-Null
}

$output = @()
foreach ($file in @($out, $err)) {
    if (Test-Path -LiteralPath $file) {
        $output += Get-Content -LiteralPath $file
    }
}

$startup = $output | Where-Object { $_ -match '^vantage-box startup ' } | Select-Object -First 1
$selftest = $output | Where-Object { $_ -match '^vantage-box selftest ' } | Select-Object -First 1
$problems = $output | Where-Object { $_ -match 'hotkey problem|selftest:' }

$report = @()
$report += "exited on its own within ${Seconds}s: $(-not $alive)"
$report += "startup line:    $(if ($startup) { $startup } else { '<not found>' })"
$report += "self-test:        $(if ($selftest) { $selftest } else { '<not found>' })"
if ($problems) { $report += $problems }
if ($output -and -not $startup) { $report += '--- full output ---'; $report += $output }

$report | Tee-Object -FilePath $log -Append | Write-Host

$failures = @()
if ($alive) { $failures += "the app did not finish its self-test within ${Seconds}s" }
if (-not $startup) { $failures += 'the app did not print the startup line' }
else {
    if ($startup -notmatch 'tray=(ok|off)') { $failures += 'the tray did not come up' }
    if ($startup -notmatch 'hotkeys=ok') { $failures += 'hotkeys are taken or misconfigured' }
    if ($startup -notmatch 'settings=ok') { $failures += 'settings.json was not read' }
}
if (-not $selftest) { $failures += 'the self-test did not report' }
elseif ($selftest -notmatch 'popup=ok') { $failures += 'the popup did not open' }

if ($failures.Count -eq 0) {
    'RESULT: success' | Tee-Object -FilePath $log -Append | Write-Host
    $code = 0
} else {
    (@('RESULT: failure') + ($failures | ForEach-Object { "  - $_" })) |
        Tee-Object -FilePath $log -Append | Write-Host
    $code = 1
}

Copy-Item -LiteralPath $log -Destination (Join-Path $resultsDir 'smoke-latest.log') -Force
Write-Host "Log: $log"
exit $code