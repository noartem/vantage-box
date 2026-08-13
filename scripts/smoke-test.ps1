<#
.SYNOPSIS
    Проверяет, что приложение стартует и поднимает трей, хоткеи и окно.

.DESCRIPTION
    «Процесс не упал» ничего не говорит про иконку в трее и глобальные хоткеи:
    они молча не работают, если что-то пошло не так. Приложение при старте
    печатает строку вида

        vantage-box startup tray=ok hotkeys=ok window=shown settings=ok

    и скрипт проверяет именно её.

    Приложение запускается на несколько секунд и останавливается. Сервис
    sing-box при этом не трогается: Vantage Box не запускает его сам.

    Внимание: на время прогона приложение занимает глобальные хоткеи из
    settings.json (по умолчанию Ctrl+Alt+P и Ctrl+Alt+O). После остановки они
    освобождаются.

    Debug-сборка берёт фронтенд с dev-сервера Vite (devUrl в tauri.conf.json),
    поэтому скрипт при необходимости поднимает его сам и гасит после прогона.
    Release-сборка использует уже собранные файлы, и сервер ей не нужен.

.PARAMETER Seconds
    Сколько ждать завершения самопроверки. По умолчанию 40.

.PARAMETER Release
    Проверять release-сборку (со встроенным фронтендом) вместо debug.
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

# target-dir может быть переопределён в .cargo/config.toml.
$targetRoot = Join-Path $repo 'src-tauri\target'
$override = Join-Path $repo 'src-tauri\.cargo\config.toml'
if (Test-Path -LiteralPath $override) {
    $line = Get-Content -LiteralPath $override | Where-Object { $_ -match 'target-dir' }
    if ($line -match '=\s*"([^"]+)"') { $targetRoot = $Matches[1] }
}

$exe = Join-Path $targetRoot "$profile_\vantage-box.exe"
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Бинарник не найден: $exe. Сначала соберите его для профиля '$profile_'."
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

# Debug-сборка ходит за фронтендом на dev-сервер. Без него окно оказывается
# пустым, и это молчаливый отказ: процесс жив, UI нет.
$vite = $null
if (-not $Release -and -not (Test-Port -Port 1420)) {
    Write-Host 'поднимаю dev-сервер Vite на 1420…'
    $vite = Start-Process -PassThru -WindowStyle Hidden -FilePath 'npm.cmd' `
        -ArgumentList 'run', 'dev' -WorkingDirectory $repo

    $deadline = (Get-Date).AddSeconds(60)
    while (-not (Test-Port -Port 1420)) {
        if ((Get-Date) -gt $deadline) {
            & taskkill /T /F /PID $vite.Id 2>&1 | Out-Null
            throw 'dev-сервер Vite не поднялся за 60 с'
        }
        Start-Sleep -Milliseconds 300
    }
}

$stamp = Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'
$log = Join-Path $resultsDir "smoke_$stamp.log"
$out = Join-Path $env:TEMP 'vantage-box-smoke-out.log'
$err = Join-Path $env:TEMP 'vantage-box-smoke-err.log'

"Vantage Box — smoke-тест`nвремя:    $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`nбинарник: $exe" |
    Tee-Object -FilePath $log | Write-Host

# --self-test заставляет приложение открыть попап, дождаться сигнала от его
# webview и выйти самостоятельно.
$process = Start-Process -PassThru -FilePath $exe -ArgumentList '--self-test' `
    -RedirectStandardOutput $out -RedirectStandardError $err

$exited = $process.WaitForExit($Seconds * 1000)
# Приложение обязано завершиться само: если оно висит, самопроверка не дошла
# до конца, и это тоже результат.
$alive = -not $exited
if ($alive) {
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit(5000) | Out-Null
}

# Гасим только тот сервер, который подняли сами.
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
$report += "завершилось само за ${Seconds}с: $(-not $alive)"
$report += "строка старта:    $(if ($startup) { $startup } else { '<не найдена>' })"
$report += "самопроверка:     $(if ($selftest) { $selftest } else { '<не найдена>' })"
if ($problems) { $report += $problems }
if ($output -and -not $startup) { $report += '--- весь вывод ---'; $report += $output }

$report | Tee-Object -FilePath $log -Append | Write-Host

$failures = @()
if ($alive) { $failures += "приложение не завершило самопроверку за ${Seconds}с" }
if (-not $startup) { $failures += 'приложение не напечатало строку старта' }
else {
    if ($startup -notmatch 'tray=(ok|off)') { $failures += 'трей не поднялся' }
    if ($startup -notmatch 'hotkeys=ok') { $failures += 'хоткеи заняты или заданы неверно' }
    if ($startup -notmatch 'settings=ok') { $failures += 'settings.json не прочитался' }
}
if (-not $selftest) { $failures += 'самопроверка не отчиталась' }
elseif ($selftest -notmatch 'popup=ok') { $failures += 'попап не открылся' }

if ($failures.Count -eq 0) {
    'РЕЗУЛЬТАТ: успех' | Tee-Object -FilePath $log -Append | Write-Host
    $code = 0
} else {
    (@('РЕЗУЛЬТАТ: провал') + ($failures | ForEach-Object { "  - $_" })) |
        Tee-Object -FilePath $log -Append | Write-Host
    $code = 1
}

Copy-Item -LiteralPath $log -Destination (Join-Path $resultsDir 'smoke-latest.log') -Force
Write-Host "Лог: $log"
exit $code
