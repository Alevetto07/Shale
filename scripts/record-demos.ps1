# Record one short video per demo scenario.
#
# Each scenario is a state the app starts in (`Shale.exe --demo <scenario>`),
# so the recording is the same every time: wait for the window, let it settle,
# record six seconds of it, close it. Nothing here touches the printer or the
# network; the demo scenarios do not either.
#
# Nothing but Shale may be in frame. Toasts are turned off globally and for
# OneDrive itself, any toast already up is closed, and a watcher keeps closing
# them for the whole run. The window is parked on the work area (screen minus
# taskbar) and kept topmost, and the capture is exactly that rectangle, so no
# other window and no taskbar can show through. The pointer is not drawn.
param(
  [string]$Exe = "C:\Users\Alessio\Shale\target\release\Shale.exe",
  [string]$Out = "C:\Users\Alessio\Shale-videos",
  [string[]]$Only = @(),
  [int]$Seconds = 6
)

$ErrorActionPreference = "Stop"

# A demo that slices needs the engine and the profile library, the same as the
# app does when a person presses Slice.
$env:SHALE_CURA_DIR = "C:\Program Files\UltiMaker Cura 5.13.0\share\cura\resources"
$env:SHALE_MATERIALS_DIR = "C:\Program Files\UltiMaker Cura 5.13.0\share\cura\resources\materials"
$env:SHALE_CURAENGINE = "C:\Program Files\UltiMaker Cura 5.13.0\CuraEngine.exe"

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ShaleCapture {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint action, uint param, ref RECT rect, uint winIni);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int index);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string title);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr param);
  public struct RECT { public int Left, Top, Right, Bottom; }

  // The work area is the screen minus the taskbar, so a window parked on it
  // covers everything a person could otherwise see behind the app.
  public static RECT WorkArea() {
    RECT rect = new RECT();
    SystemParametersInfo(0x0030, 0, ref rect, 0);
    return rect;
  }

  // A toast is a XAML island owned by the shell, in the bottom right corner.
  // Closing it is the only way to get rid of one that is already up.
  public static int CloseToasts() {
    RECT screen = new RECT();
    SystemParametersInfo(0x0030, 0, ref screen, 0);
    int closed = 0;
    EnumWindows(delegate(IntPtr hWnd, IntPtr param) {
      if (!IsWindowVisible(hWnd)) return true;
      StringBuilder name = new StringBuilder(256);
      GetClassName(hWnd, name, name.Capacity);
      string cls = name.ToString();
      // A shell toast, or any always-on-top popup parked in the corner (chat
      // apps draw their own). Shale itself is topmost too, so it is skipped.
      bool shellToast = cls == "Windows.UI.Core.CoreWindow" || cls == "XamlExplorerHostIslandWindow";
      bool topmost = (GetWindowLong(hWnd, -20) & 0x00000008) != 0;
      if (!shellToast && !topmost) return true;
      StringBuilder title = new StringBuilder(256);
      GetWindowText(hWnd, title, title.Capacity);
      if (title.ToString() == "Shale") return true;
      RECT rect;
      if (!GetWindowRect(hWnd, out rect)) return true;
      int width = rect.Right - rect.Left;
      int height = rect.Bottom - rect.Top;
      if (width < 200 || width > 900 || height < 60 || height > 600) return true;
      if (rect.Left < screen.Right - 1000) return true;
      if (rect.Top < screen.Bottom - 800) return true;
      SendMessage(hWnd, 0x0010, IntPtr.Zero, IntPtr.Zero);
      closed++;
      return true;
    }, IntPtr.Zero);
    return closed;
  }
}
'@
[void][ShaleCapture]::SetProcessDPIAware()

$ffmpeg = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\Gyan.FFmpeg_*\ffmpeg-*\bin\ffmpeg.exe" |
  Select-Object -First 1 -ExpandProperty FullName
if (-not $ffmpeg) { throw "ffmpeg not found under WinGet packages" }
if (-not (Test-Path $Exe)) { throw "no Shale at $Exe" }
New-Item -ItemType Directory -Force -Path $Out | Out-Null


# Toasts photobombed every earlier take. Turn them off globally and for OneDrive
# itself, and put both values back afterwards, whatever they were.
$toastKey = "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Notifications\Settings"
$oneDriveKey = Join-Path $toastKey "Microsoft.SkyDrive.Desktop"
$toastWas = (Get-ItemProperty -Path $toastKey -Name NOC_GLOBAL_SETTING_TOASTS_ENABLED -ErrorAction SilentlyContinue).NOC_GLOBAL_SETTING_TOASTS_ENABLED
Set-ItemProperty -Path $toastKey -Name NOC_GLOBAL_SETTING_TOASTS_ENABLED -Value 0 -Type DWord
$appWas = $null
if (Test-Path $oneDriveKey) {
  $appWas = (Get-ItemProperty -Path $oneDriveKey -Name Enabled -ErrorAction SilentlyContinue).Enabled
  Set-ItemProperty -Path $oneDriveKey -Name Enabled -Value 0 -Type DWord
}

# A watcher for the whole run: whatever the shell puts in the corner goes away
# before it can be filmed.
$watcher = Start-Job -ScriptBlock {
  Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ToastWatch {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int index);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint action, uint param, ref RECT rect, uint winIni);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr param);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static int Close() {
    RECT screen = new RECT();
    SystemParametersInfo(0x0030, 0, ref screen, 0);
    int closed = 0;
    EnumWindows(delegate(IntPtr hWnd, IntPtr param) {
      if (!IsWindowVisible(hWnd)) return true;
      StringBuilder name = new StringBuilder(256);
      GetClassName(hWnd, name, name.Capacity);
      string cls = name.ToString();
      // A shell toast, or any always-on-top popup parked in the corner (chat
      // apps draw their own). Shale itself is topmost too, so it is skipped.
      bool shellToast = cls == "Windows.UI.Core.CoreWindow" || cls == "XamlExplorerHostIslandWindow";
      bool topmost = (GetWindowLong(hWnd, -20) & 0x00000008) != 0;
      if (!shellToast && !topmost) return true;
      StringBuilder title = new StringBuilder(256);
      GetWindowText(hWnd, title, title.Capacity);
      if (title.ToString() == "Shale") return true;
      RECT rect;
      if (!GetWindowRect(hWnd, out rect)) return true;
      int width = rect.Right - rect.Left;
      int height = rect.Bottom - rect.Top;
      if (width < 200 || width > 900 || height < 60 || height > 600) return true;
      if (rect.Left < screen.Right - 1000) return true;
      if (rect.Top < screen.Bottom - 800) return true;
      SendMessage(hWnd, 0x0010, IntPtr.Zero, IntPtr.Zero);
      closed++;
      return true;
    }, IntPtr.Zero);
    return closed;
  }
}
'@
  while ($true) {
    [void][ToastWatch]::Close()
    Start-Sleep -Milliseconds 250
  }
}

# Two presets so the sheet has something to show, written before the app starts
# in the shape the app itself writes.
$presetDir = Join-Path $env:APPDATA "dev.shale.desktop\presets"
New-Item -ItemType Directory -Force -Path $presetDir | Out-Null
# A preset is the eight fields of the sheet and nothing else: a file missing a
# key is not a preset, and the sheet then lists none of them.
$seed = @{
  "draft-fast.json" = @{ name = "draft fast"; values = @{ layerHeight = "0.28"; walls = "2"; infill = "10"; pattern = "grid"; supports = "none"; nozzle = "0.4"; bed = "60"; speed = "150" } }
  "strong-012.json" = @{ name = "strong 0.12"; values = @{ layerHeight = "0.12"; walls = "4"; infill = "35"; pattern = "gyroid"; supports = "tree"; nozzle = "0.4"; bed = "60"; speed = "60" } }
  "display-02.json" = @{ name = "display 0.20"; values = @{ layerHeight = "0.20"; walls = "3"; infill = "15"; pattern = "cubic"; supports = "none"; nozzle = "0.4"; bed = "60"; speed = "100" } }
}

$utf8 = New-Object System.Text.UTF8Encoding $false
foreach ($file in $seed.Keys) {
  $path = Join-Path $presetDir $file
  # A byte-order mark makes serde_json refuse the file, and the sheet then
  # shows no presets at all.
  [System.IO.File]::WriteAllText($path, ($seed[$file] | ConvertTo-Json -Depth 5), $utf8)
}

# How long each scenario needs before it is worth filming: an import or a slice
# at startup takes real seconds, and a video of a spinner is not a demo.
$settle = @{
  "start"   = 20
  "slice"   = 12
  "preview" = 18
  "multi"   = 26
  "queue"   = 18
  "cost"    = 18
  "presets" = 20
}

$order = @("start", "slice", "preview", "multi", "queue", "cost", "presets")
if ($Only.Count -gt 0) { $order = $order | Where-Object { $Only -contains $_ } }

$index = @{ start = 1; slice = 2; preview = 3; multi = 4; queue = 5; cost = 6; presets = 7 }
try {
  foreach ($scenario in $order) {
    $name = "{0:D2}-{1}.mp4" -f $index[$scenario], $scenario
    $target = Join-Path $Out $name

    Write-Host "== $scenario"
    # The scenarios are driven by the same environment the app already reads, so
    # a recording does not depend on the argument parser being right.
    $env:SHALE_DEMO = $scenario
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
    $env:SHALE_DEMO_MODEL = "C:\Users\Alessio\Shale\examples\cube.stl"
    $env:SHALE_DEMO_SLICE = $null
    $env:SHALE_DEMO_SUPPORTS = $null
    if ($scenario -ne "start") {
      if ($scenario -in @("preview", "multi", "queue", "cost", "presets")) {
        $env:SHALE_DEMO_SLICE = "1"
      }
      if ($scenario -eq "multi") { $env:SHALE_DEMO_SUPPORTS = "1" }
    } else {
      $env:SHALE_DEMO_MODEL = $null
    }
    # A throwaway webview profile per scenario: the Device pane reads the
    # remembered printer from localStorage, so a clip must start with none, and
    # a profile left locked by the previous run would stop the next one opening.
    $env:WEBVIEW2_USER_DATA_FOLDER = "C:\Users\Alessio\probe\shale-demo-profile-$scenario"
    if (Test-Path $env:WEBVIEW2_USER_DATA_FOLDER) { Remove-Item -Recurse -Force $env:WEBVIEW2_USER_DATA_FOLDER }
    $proc = Start-Process -FilePath $Exe -PassThru

    # Wait for a window actually titled Shale: the process exists before the
    # webview has drawn anything.
    $deadline = (Get-Date).AddSeconds(90)
    $window = $null
    while ((Get-Date) -lt $deadline) {
      Start-Sleep -Milliseconds 400
      $window = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
      if ($window -and $window.MainWindowTitle -eq "Shale") { break }
      if ($proc.HasExited) { throw "Shale exited before showing a window ($scenario)" }
    }
    if (-not $window -or $window.MainWindowTitle -ne "Shale") {
      throw "no Shale window after 90 s ($scenario)"
    }

    Start-Sleep -Seconds $settle[$scenario]
    # The tab refuses to open before a slice exists, and a slow plate is still
    # slicing when the app settles, so the click is retried until it takes.
    # The slice clip is about pressing Slice: the plate is empty on purpose, and
    # the take is longer so the toolpath has time to appear.
    if ($scenario -eq "slice") {
      $click = & python "C:\Users\Alessio\Shale\scripts\demo-tab.py" SLICE 2>&1
      Write-Host ("   slice {0}" -f $click)
      Start-Sleep -Milliseconds 1500
    }
    if ($scenario -in @("preview", "multi")) {
      for ($try = 0; $try -lt 12; $try++) {
        $click = & python "C:\Users\Alessio\Shale\scripts\demo-tab.py" PREVIEW 2>&1
        Write-Host ("   tab {0}" -f $click)
        if ("$click".Trim() -eq "active") { break }
        Start-Sleep -Seconds 5
      }
      Start-Sleep -Milliseconds 1200
    }

    # Park it on the work area and keep it above everything else, so the capture
    # rectangle holds the app and nothing behind it.
    $work = [ShaleCapture]::WorkArea()
    $workW = $work.Right - $work.Left
    $workH = $work.Bottom - $work.Top
    [void][ShaleCapture]::SetWindowPos($window.MainWindowHandle, [IntPtr](-1), $work.Left, $work.Top, $workW, $workH, 0x0040)
    Start-Sleep -Milliseconds 1400
    # The pointer would otherwise sit somewhere over the window.
    [void][ShaleCapture]::SetCursorPos($work.Left - 400, $work.Bottom + 200)
    [void][ShaleCapture]::CloseToasts()
    Start-Sleep -Milliseconds 500
    [void][ShaleCapture]::CloseToasts()

    # The taskbar is topmost and would sit in the bottom strip of a full-screen
    # window, so the capture stops above it.
    $tray = [ShaleCapture]::FindWindow("Shell_TrayWnd", $null)
    $trayH = 0
    if ($tray -ne [IntPtr]::Zero) {
      $trayRect = New-Object ShaleCapture+RECT
      [void][ShaleCapture]::GetWindowRect($tray, [ref]$trayRect)
      $trayH = $trayRect.Bottom - $trayRect.Top
    }
    $takeSeconds = $Seconds
    if ($scenario -eq "slice") { $takeSeconds = 10 }
    $rect = New-Object ShaleCapture+RECT
    [void][ShaleCapture]::GetWindowRect($window.MainWindowHandle, [ref]$rect)
    $x = [Math]::Max($work.Left, $rect.Left)
    $y = [Math]::Max($work.Top, $rect.Top)
    $w = [Math]::Min($work.Right, $rect.Right) - $x
    $h = [Math]::Min($work.Bottom, $rect.Bottom) - $y - $trayH
    if ($w % 2 -ne 0) { $w-- }
    if ($h % 2 -ne 0) { $h-- }
    Write-Host ("   crop {0},{1} {2}x{3}" -f $x, $y, $w, $h)

    & $ffmpeg -y -hide_banner -loglevel error `
      -f gdigrab -framerate 30 -draw_mouse 0 -offset_x $x -offset_y $y -video_size "${w}x${h}" -i desktop `
      -t $takeSeconds `
      -vf "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2" `
      -c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -movflags +faststart -an `
      $target
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed for $scenario" }

    $size = (Get-Item $target).Length
    Write-Host ("   {0}  {1:N0} bytes" -f $name, $size)

    # Close the window politely; the app's single-instance mutex is released on
    # exit, and a hard kill leaves WebView2 profiles behind.
    if (-not $proc.HasExited) { [void]$proc.CloseMainWindow() }
    for ($i = 0; $i -lt 20; $i++) {
      Start-Sleep -Milliseconds 300
      if ($proc.HasExited) { break }
    }
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
    Start-Sleep -Milliseconds 800
  }
} finally {
  Stop-Job $watcher -ErrorAction SilentlyContinue
  Remove-Job $watcher -Force -ErrorAction SilentlyContinue
  if ($null -eq $toastWas) {
    Remove-ItemProperty -Path $toastKey -Name NOC_GLOBAL_SETTING_TOASTS_ENABLED -ErrorAction SilentlyContinue
  } else {
    Set-ItemProperty -Path $toastKey -Name NOC_GLOBAL_SETTING_TOASTS_ENABLED -Value $toastWas -Type DWord
  }
  if ($null -eq $appWas) {
    Remove-ItemProperty -Path $oneDriveKey -Name Enabled -ErrorAction SilentlyContinue
  } else {
    Set-ItemProperty -Path $oneDriveKey -Name Enabled -Value $appWas -Type DWord
  }
}
Write-Host "done: $Out"
