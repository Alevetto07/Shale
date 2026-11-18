# Packages the windowless half of Shale: the CLI and the MCP server.
#
# Both are ordinary Rust binaries with no Tauri and no webview, so a zip of the
# two executables is a complete install; CuraEngine and the Cura resource tree
# are the only things they look for at run time.
param(
    [string]$Out = "dist",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path "crates/shale-cli/Cargo.toml" -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
if (-not $SkipBuild) {
    cargo build --release --locked -p shale-cli -p shale-mcp
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$stage = Join-Path $Out "shale-$version-windows-x64"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item "target/release/shale-cli.exe" (Join-Path $stage "shale.exe")
Copy-Item "target/release/shale-mcp.exe" $stage
Copy-Item "docs/INSTALL-CLI.md" $stage
Copy-Item "LICENSE" $stage
Copy-Item "THIRD-PARTY-NOTICES.md" $stage
Copy-Item "LICENSING.md" $stage

$zip = Join-Path $Out "shale-$version-windows-x64.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip

$size = (Get-Item $zip).Length
Write-Host "packaged $zip ($size bytes)"
