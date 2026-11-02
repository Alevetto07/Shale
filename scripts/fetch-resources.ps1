# Download the Cura definition tree and the fdm_materials library.
# Windows equivalent of fetch-resources.sh. Requires Git for Windows.
# Nothing here is copied into the Shale repository.
#
# Pins:
#   Cura 5.13.0 — resources are LGPL-3.0, see vendor/Cura/LICENSE
#   fdm_materials 82f433b — CC0, see vendor/fdm_materials/LICENSE

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
if (-not $Root) { $Root = (Get-Location).Path }
$Dest = if ($args.Count -ge 1) { $args[0] } else { Join-Path $Root "vendor" }
$CuraTag = "5.13.0"
$CuraCommit = "1fb8a7610460e03ec62da950d1c173a249b23b59"
$MaterialsCommit = "82f433b2cfe0ac36c974443b80c9294f31b79224"

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Error "git was not found. Install Git for Windows, then run this script again."
}

New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$Cura = Join-Path $Dest "Cura"
$Materials = Join-Path $Dest "fdm_materials"

if (-not (Test-Path (Join-Path $Cura ".git"))) {
    git clone --filter=blob:none --sparse --branch $CuraTag --depth 1 `
        https://github.com/Ultimaker/Cura.git $Cura
}

git -C $Cura sparse-checkout set --no-cone `
    /LICENSE `
    /resources/definitions `
    /resources/extruders `
    /resources/quality `
    /resources/variants `
    /resources/intent

$Printer = Join-Path $Cura "resources\definitions\fdmprinter.def.json"
if (-not (Test-Path $Printer)) {
    Write-Error "Cura definitions did not check out. Expected $Printer"
}
if (-not (Test-Path (Join-Path $Cura "LICENSE"))) {
    Write-Error "Cura LICENSE is missing. Refusing to continue without the LGPL text."
}

if (-not (Test-Path (Join-Path $Materials ".git"))) {
    git clone https://github.com/Ultimaker/fdm_materials.git $Materials
}
git -C $Materials checkout $MaterialsCommit

if (-not (Test-Path (Join-Path $Materials "LICENSE"))) {
    Write-Error "fdm_materials LICENSE is missing. Refusing to continue without the CC0 text."
}

@"
Cura $CuraTag $CuraCommit
fdm_materials $MaterialsCommit
"@ | Set-Content -Path (Join-Path $Dest "REVISIONS") -Encoding utf8

Write-Output "Resources ready in $Dest"
Write-Output "  Cura $CuraTag ($CuraCommit)"
Write-Output "  fdm_materials $MaterialsCommit"
Write-Output "Cura 5.13 does not ship a Bambu Lab A1 definition. That printer arrived in a later Cura release."
