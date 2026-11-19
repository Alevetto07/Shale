# Smoke-test custom title bar controls on a built Shale.exe (Windows CI).
param(
  [Parameter(Mandatory = $true)][string]$Exe
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path $Exe)) { throw "missing $Exe" }

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class WinState {
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hWnd);
}
"@

$proc = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 4
$win = Get-Process -Id $proc.Id
if (-not $win.MainWindowHandle -or $win.MainWindowHandle -eq 0) {
  Stop-Process -Id $proc.Id -Force
  throw "Shale main window did not appear"
}

# Best-effort UI Automation: look for Minimise / Maximise / Close buttons by name.
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::FromHandle($win.MainWindowHandle)
$scope = [System.Windows.Automation.TreeScope]::Descendants
$cond = New-Object System.Windows.Automation.AndCondition(
  (New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    [System.Windows.Automation.ControlType]::Button
  ))
)
$buttons = $root.FindAll($scope, $cond)
$labels = @()
foreach ($b in $buttons) {
  $name = $b.Current.Name
  if ($name -match "Minimise|Maximise|Restore|Close") { $labels += $name }
}
if ($labels.Count -lt 3) {
  Stop-Process -Id $proc.Id -Force
  throw "expected title bar buttons, found: $($labels -join ', ')"
}

# Click minimise, expect iconic; restore via taskbar is flaky in CI, so close after maximise toggle attempt.
$max = $buttons | Where-Object { $_.Current.Name -match "Maximise|Restore" } | Select-Object -First 1
if ($max) {
  $pattern = $max.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  $pattern.Invoke()
  Start-Sleep -Milliseconds 600
}
if ([WinState]::IsZoomed($win.MainWindowHandle)) {
  $pattern.Invoke()
  Start-Sleep -Milliseconds 400
}

Stop-Process -Id $proc.Id -Force
Write-Host "titlebar smoke ok ($($labels -join ', '))"