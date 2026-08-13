<#
.SYNOPSIS
    Строит матрицу совместимости Vantage Box с версиями sing-box.

.DESCRIPTION
    Скачивает несколько релизов sing-box с GitHub, прогоняет по каждому
    одинаковый набор проб и складывает результат в test-results/:
    compat-matrix.json и compat-matrix.md. В конце печатает диапазон версий,
    выведенный из измерений, — его и стоит записать в SINGBOX_MIN /
    SINGBOX_MAX_EXCLUSIVE (src-tauri/src/clash/client.rs).

    Каждая версия проверяется изолированно: отдельный процесс sing-box,
    нестандартные порты (19090/19080), конфиг без TUN — то есть без прав
    администратора и без вмешательства в сетевой стек. Скрипт ничего не
    останавливает: уже работающий sing-box продолжает работать.

    Скачанные бинарники кэшируются, повторный прогон быстрее.

.PARAMETER Minors
    Сколько последних минорных веток проверять. По умолчанию 6.

.PARAMETER Versions
    Явный список версий через запятую вместо выбора по минорам.

.PARAMETER Cache
    Куда складывать скачанные бинарники. По умолчанию во временную папку.
    Полезно указать другой диск, если на системном мало места.

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
# cargo пишет прогресс в stderr; при ErrorActionPreference=Stop Windows
# PowerShell счёл бы это фатальной ошибкой.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    # Именно example, а не bin: лишний бинарник в пакете сбивает бандлер Tauri,
    # и в инсталлятор попадает не то приложение.
    & cargo run --release --example compat-probe -- @probeArgs 2>&1 |
        ForEach-Object { $_.ToString() } |
        Tee-Object -FilePath $log
    $code = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previousPreference
    Pop-Location
}

Write-Host ''
Write-Host "Лог:      $log"
Write-Host "Матрица:  $(Join-Path $resultsDir 'compat-matrix.md')"
exit $code
