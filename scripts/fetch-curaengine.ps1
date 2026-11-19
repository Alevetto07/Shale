# Obtain CuraEngine 5.13.0 on Windows (matches fetch-resources.ps1 / Cura 5.13.0).
# Extracts CuraEngine.exe and bundled DLLs from the official installer via 7-Zip.
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) { $Root = (Get-Location).Path }
$Dest = if ($args.Count -ge 1) { $args[0] } else { Join-Path $Root "vendor\curaengine" }
$Tag = "5.13.0"
$Installer = Join-Path $Root "vendor\UltiMaker-Cura-$Tag-win64-X64.exe"
$Url = "https://github.com/Ultimaker/Cura/releases/download/$Tag/UltiMaker-Cura-$Tag-win64-X64.exe"
$Extract = Join-Path $Root "vendor\cura-win-extract"

New-Item -ItemType Directory -Force -Path (Split-Path $Installer) | Out-Null
if (-not (Test-Path $Installer)) {
    Write-Host "Downloading UltiMaker Cura $Tag Windows installer…"
    Invoke-WebRequest -Uri $Url -OutFile $Installer -UseBasicParsing
}

if (-not (Get-Command 7z -ErrorAction SilentlyContinue)) {
    $SevenZip = "${env:ProgramFiles}\7-Zip\7z.exe"
    if (-not (Test-Path $SevenZip)) {
        Write-Error "7-Zip (7z) is required to extract the Cura installer."
    }
    $SevenZipCmd = $SevenZip
} else {
    $SevenZipCmd = "7z"
}

if (Test-Path $Extract) {
    Remove-Item -Recurse -Force $Extract
}
New-Item -ItemType Directory -Force -Path $Extract | Out-Null
& $SevenZipCmd x $Installer "-o$Extract" -y | Out-Null

$Engine = Get-ChildItem -Path $Extract -Recurse -Filter "CuraEngine.exe" -ErrorAction SilentlyContinue |
    Select-Object -First 1
if (-not $Engine) {
    Write-Error "CuraEngine.exe not found inside extracted installer"
}

$LibDir = $Engine.Directory.FullName
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$Target = Join-Path $Dest "CuraEngine.exe"
Copy-Item -Force $Engine.FullName $Target
Get-ChildItem -Path $LibDir -Filter "*.dll" -ErrorAction SilentlyContinue |
    ForEach-Object { Copy-Item -Force $_.FullName $Dest }

Write-Host "CuraEngine installed at $Target"
Write-Host "  set SHALE_CURAENGINE=$Target"
Write-Host "  set SHALE_CURAENGINE_LIB_DIR=$Dest"