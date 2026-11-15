# Install Shale silently, check what the installer claimed to do, launch it,
# open a project by association, then uninstall and check it is gone.
#
# Nothing here needs admin: the bundle is currentUser, so everything lands in
# %LOCALAPPDATA% and HKCU. The script prints one line per check and stops at
# the first failure, so a partial pass cannot read as a pass.
$ErrorActionPreference = "Stop"

$setup = "C:\Users\Alessio\Shale\target\release\bundle\nsis\Shale_0.1.1_x64-setup.exe"
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

Write-Host "== uninstall"
Start-Process -FilePath (Join-Path $appDir "uninstall.exe") -ArgumentList "/S" -Wait
Start-Sleep -Seconds 4
Check "app directory removed" (-not (Test-Path $exe))
Check ".shale association removed" (-not (Test-Path $shaleKey))
foreach ($ext in @(".3mf", ".stl", ".step", ".stp")) {
  $offer = (Get-ItemProperty "HKCU:\Software\Classes\$ext\OpenWithProgids" -ErrorAction SilentlyContinue)
  Check "$ext no longer offers Shale" ($null -eq $offer -or $null -eq $offer."Shale project")
}
Check "Start Menu shortcut removed" (-not (Get-ChildItem "$env:APPDATA\Microsoft\Windows\Start Menu\Programs" -Recurse -Filter "Shale.lnk" -ErrorAction SilentlyContinue))
Check "the project file was left alone" (Test-Path $project)
Write-Host "smoke test passed"
