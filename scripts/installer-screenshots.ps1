# Capture NSIS installer pages at 100%, 125% and 150% per-user DPI (LogPixels 96/120/144).
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [string]$OutDir = "dist/installer-screenshots"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Add-Type -AssemblyName System.Windows.Forms
$drawing = [System.Reflection.Assembly]::LoadWithPartialName("System.Drawing")
if (-not $drawing) {
  throw "System.Drawing is unavailable on this runner"
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Drawing;
using System.Drawing.Imaging;
public static class WinCap {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  public static void Save(IntPtr hwnd, string path) {
    ShowWindow(hwnd, 9);
    SetForegroundWindow(hwnd);
    System.Threading.Thread.Sleep(400);
    var r = new RECT();
    GetWindowRect(hwnd, ref r);
    int w = r.Right - r.Left;
    int h = r.Bottom - r.Top;
  var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb);
    using (var g = Graphics.FromImage(bmp)) {
      g.CopyFromScreen(r.Left, r.Top, 0, 0, new Size(w, h));
    }
    bmp.Save(path, ImageFormat.Png);
  }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hWnd, ref RECT lpRect);
}
"@

function Set-LogPixels([int]$value) {
  Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name LogPixels -Value $value -Type DWord -Force
  $null = & RUNDLL32.EXE USER32.DLL,UpdatePerUserSystemParameters 1
  Start-Sleep -Seconds 2
}

function Wait-InstallerWindow([int]$timeoutSec = 45) {
  $deadline = (Get-Date).AddSeconds($timeoutSec)
  while ((Get-Date) -lt $deadline) {
    $proc = Get-Process | Where-Object { $_.MainWindowTitle -match "Shale" -and $_.MainWindowTitle -match "Setup|Install" }
    if ($proc) { return $proc[0] }
    Start-Sleep -Milliseconds 350
  }
  return $null
}

Add-Type -AssemblyName System.Windows.Forms
$scales = @(@{ px = 96; label = "100" }, @{ px = 120; label = "125" }, @{ px = 144; label = "150" })
foreach ($scale in $scales) {
  Set-LogPixels $scale.px
  $setup = Start-Process -FilePath $Installer -PassThru
  $win = Wait-InstallerWindow
  if (-not $win) {
    Write-Warning "Installer window not found at DPI $($scale.label)%"
    if ($setup -and -not $setup.HasExited) { Stop-Process -Id $setup.Id -Force }
    continue
  }
  for ($page = 1; $page -le 6; $page++) {
    $out = Join-Path $OutDir ("installer-$($scale.label)pct-page{0}.png" -f $page)
    [WinCap]::Save($win.MainWindowHandle, $out)
    [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
    Start-Sleep -Milliseconds 800
    if ($win.HasExited) { break }
  }
  if (-not $setup.HasExited) {
    Stop-Process -Id $setup.Id -Force -ErrorAction SilentlyContinue
  }
  Start-Sleep -Seconds 1
}

Write-Host "Wrote installer captures under $OutDir"