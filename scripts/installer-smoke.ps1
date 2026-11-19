# Install Shale silently, check what the installer claimed to do, launch it,
# open a project by association, then uninstall and check it is gone.
#
# Nothing here needs admin: the bundle is currentUser, so everything lands in
# %LOCALAPPDATA% and HKCU. The script prints one line per check and stops at
# the first failure, so a partial pass cannot read as a pass.
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$confPath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.conf.json"
$version = (Get-Content $confPath -Raw | ConvertFrom-Json).version
$setup = Join-Path $repoRoot "target\release\bundle\nsis\Shale_${version}_x64-setup.exe"
$project = "C:\Users\Alessio\probe\roundtrip.shale"
$appDir = Join-Path $env:LOCALAPPDATA "Shale"
$exe = Join-Path $appDir "Shale.exe"

function Check($name, $ok) {
  if ($ok) { Write-Host "PASS  $name" } else { Write-Host "FAIL  $name"; throw $name }
}

Write-Host "== install"
Start-Process -FilePath $setup -ArgumentList "/S" -Wait
Start-Sleep -Seconds 3
Check "Shale.exe installed at $exe" (Test-Path $exe)
Check "uninstaller present" (Test-Path (Join-Path $appDir "uninstall.exe"))
Check "notices shipped" (Test-Path (Join-Path $appDir "THIRD-PARTY-NOTICES.md"))
Check "no admin install dir" (-not (Test-Path "C:\Program Files\Shale"))
$desk = Join-Path ([Environment]::GetFolderPath("Desktop")) "Shale.lnk"
Check "no desktop shortcut on silent install ($desk)" (-not (Test-Path $desk))

Write-Host "== associations"
$shaleKey = "HKCU:\Software\Classes\.shale"
Check ".shale registered" (Test-Path $shaleKey)
$progId = (Get-ItemProperty $shaleKey)."(default)"
Check ".shale points at a ProgID ($progId)" ($progId -like "Shale*")
$command = (Get-ItemProperty "HKCU:\Software\Classes\$progId\shell\open\command")."(default)"
Check "open command runs Shale ($command)" ($command -like "*Shale.exe*")
foreach ($ext in @(".3mf", ".stl", ".step", ".stp")) {
  $offer = (Get-ItemProperty "HKCU:\Software\Classes\$ext\OpenWithProgids" -ErrorAction SilentlyContinue)
  Check "$ext offers Shale in Open with" ($null -ne $offer -and $null -ne $offer."Shale project")
}
# Shale must not have stolen the model defaults: whoever owned .stl keeps it.
$stl = (Get-ItemProperty "HKCU:\Software\Classes\.stl")."(default)"
Check ".stl default is not Shale ($stl)" ($stl -ne "Shale project")

Write-Host "== launch"
$proc = Start-Process -FilePath $exe -PassThru
$deadline = (Get-Date).AddSeconds(60)
$window = $null
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 400
  $window = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
  if ($window -and $window.MainWindowTitle -eq "Shale") { break }
}
Check "installed Shale shows its window" ($window -and $window.MainWindowTitle -eq "Shale")

Write-Host "== open a project by association"
$second = Start-Process -FilePath $project -PassThru
# The running window polls for the handover file and deletes it as it reads it,
# so this has to look while it is still there rather than after a fixed wait.
$pending = Join-Path $env:APPDATA "dev.shale.desktop\pending-open.txt"
$seen = $false
for ($i = 0; $i -lt 25; $i++) {
  Start-Sleep -Milliseconds 200
  if (Test-Path $pending) { $seen = $true; break }
}
Check "the second launch handed the path over" $seen
Start-Sleep -Seconds 3
Check "the second process exited" ($second.HasExited)
Check "the first window is still up" (-not $proc.HasExited)

# Close it the polite way: the mutex is released on exit.
[void]$proc.CloseMainWindow()
for ($i = 0; $i -lt 30; $i++) { Start-Sleep -Milliseconds 300; if ($proc.HasExited) { break } }
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }

function Wait-ProcessExit($process, $timeoutSeconds) {
  if (-not $process) { return $true }
  $deadline = (Get-Date).AddSeconds($timeoutSeconds)
  while ((Get-Date) -lt $deadline) {
  $process.Refresh()
  if ($process.HasExited) { return $true }
  Start-Sleep -Milliseconds 250
  }
  return $process.HasExited
  }

function Wait-ShaleGone($timeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($timeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if (-not (Test-Path $exe) -and -not (Get-Process -Name "Shale" -ErrorAction SilentlyContinue)) {
      return $true
    }
    Start-Sleep -Milliseconds 250
  }
  return (-not (Test-Path $exe)) -and -not (Get-Process -Name "Shale" -ErrorAction SilentlyContinue)
}

function Wait-RegistryGone($path, $timeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($timeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    if (-not (Test-Path $path)) { return $true }
    Start-Sleep -Milliseconds 250
  }
  return -not (Test-Path $path)
}

function Wait-OpenWithGone($ext, $timeoutSeconds) {
  $deadline = (Get-Date).AddSeconds($timeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    $offer = (Get-ItemProperty "HKCU:\Software\Classes\$ext\OpenWithProgids" -ErrorAction SilentlyContinue)
    if ($null -eq $offer -or $null -eq $offer."Shale project") { return $true }
    Start-Sleep -Milliseconds 250
  }
  $final = (Get-ItemProperty "HKCU:\Software\Classes\$ext\OpenWithProgids" -ErrorAction SilentlyContinue)
  return ($null -eq $final -or $null -eq $final."Shale project")
}

Write-Host "== uninstall"
Check "Shale.exe is not running" (Wait-ProcessExit $proc 15)
$uninstaller = Join-Path $appDir "uninstall.exe"
$uninst = Start-Process -FilePath $uninstaller -ArgumentList "/S" -PassThru
Check "uninstaller finished" (Wait-ProcessExit $uninst 120)
Check "Shale process and install dir are gone" (Wait-ShaleGone 30)
Check ".shale association removed" (Wait-RegistryGone $shaleKey 15)
foreach ($ext in @(".3mf", ".stl", ".step", ".stp")) {
  Check "$ext no longer offers Shale" (Wait-OpenWithGone $ext 15)
}
Check "Start Menu shortcut removed" (-not (Get-ChildItem "$env:APPDATA\Microsoft\Windows\Start Menu\Programs" -Recurse -Filter "Shale.lnk" -ErrorAction SilentlyContinue))
Check "the project file was left alone" (Test-Path $project)
Write-Host "smoke test passed"
