<#
.SYNOPSIS
    Прогоняет интеграционный тест Vantage Box против настоящего sing-box.

.DESCRIPTION
    Тест изолирован от рабочей системы:
      * поднимает отдельный процесс sing-box и убивает только его;
      * слушает порты 19090 и 19080, а не стандартные;
      * работает с конфигом БЕЗ TUN — сетевой стек не трогается,
        права администратора не нужны;
      * всё состояние держит во временной папке.

    Скрипт НИЧЕГО не останавливает и не удаляет: уже запущенный sing-box
    продолжает работать. Путь к бинарнику определяется автоматически, в том
    числе по уже работающему процессу — но сам процесс при этом не трогается.

    Результат прогона сохраняется в test-results/.

.PARAMETER SingBox
    Путь к бинарнику sing-box. Если не указан — ищется автоматически.

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

    # Scoop кладёт в PATH не сам бинарник, а шим-обёртку. Запускать её нельзя:
    # тест убивает свой дочерний процесс, а при шиме дочерним окажется он сам —
    # настоящий sing-box остался бы висеть сиротой.
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
            throw "Файл не найден: $Explicit"
        }
        return (Resolve-Path -LiteralPath $Explicit).Path
    }

    if ($env:VANTAGE_BOX_TEST_SINGBOX -and (Test-Path -LiteralPath $env:VANTAGE_BOX_TEST_SINGBOX)) {
        return (Resolve-Path -LiteralPath $env:VANTAGE_BOX_TEST_SINGBOX).Path
    }

    $onPath = Get-Command 'sing-box' -ErrorAction SilentlyContinue
    if ($onPath) { return (Resolve-Shim $onPath.Source) }

    # Бинарник, которым управляет сам Vantage Box.
    $managed = Join-Path $env:APPDATA 'vantage-box\bin\sing-box.exe'
    if (Test-Path -LiteralPath $managed) { return $managed }

    # Последняя попытка: узнать путь у уже работающего процесса.
    # Только читаем свойство — процесс не трогаем.
    try {
        $running = Get-Process -Name 'sing-box' -ErrorAction Stop |
            Where-Object { $_.Path } |
            Select-Object -First 1
        if ($running) { return $running.Path }
    } catch {
        # Процесса нет — не беда, просто идём дальше.
    }

    throw @'
Не удалось найти бинарник sing-box.
Укажите путь явно:  ./scripts/integration-test.ps1 -SingBox "C:\путь\sing-box.exe"
'@
}

$binary = Resolve-SingBox -Explicit $SingBox

# Чистая рабочая папка на каждый прогон: остатки прошлого запуска не должны
# влиять на результат.
if (Test-Path -LiteralPath $workDir) {
    Remove-Item -LiteralPath $workDir -Recurse -Force
}
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null

$stamp = Get-Date -Format 'yyyy-MM-dd_HH-mm-ss'
$log = Join-Path $resultsDir "integration_$stamp.log"
$latest = Join-Path $resultsDir 'latest.log'

$header = @"
Vantage Box — интеграционный тест
время:      $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
sing-box:   $binary
рабочая:    $workDir
порты:      Clash API 19090, mixed 19080 (нестандартные, чтобы не мешать рабочему sing-box)
"@

Write-Host $header
$header | Set-Content -LiteralPath $log -Encoding UTF8

$env:VANTAGE_BOX_TEST_SINGBOX = $binary
$env:VANTAGE_BOX_CONFIG_DIR = $workDir

Push-Location (Join-Path $repo 'src-tauri')
# cargo пишет прогресс в stderr, а при ErrorActionPreference=Stop Windows
# PowerShell считает это фатальной ошибкой ещё до завершения команды.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # --nocapture: весь println! теста должен попасть в лог.
    # --test-threads=1: тест один, но порядок вывода так предсказуем.
    # ToString: cargo пишет прогресс в stderr, и без приведения к строке эти
    # строки попали бы в лог как развёрнутые ErrorRecord'ы.
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

$verdict = if ($code -eq 0) { 'РЕЗУЛЬТАТ: успех' } else { "РЕЗУЛЬТАТ: провал (код $code)" }
$verdict | Tee-Object -FilePath $log -Append | Write-Host

Copy-Item -LiteralPath $log -Destination $latest -Force
Write-Host "Лог: $log"
Write-Host "Он же: $latest"

exit $code
