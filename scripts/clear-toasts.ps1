# Dismiss whatever is sitting in the notification corner.
#
# OneDrive's "storage is full" toast is a critical notification: it does not
# time out, and it sits exactly where Shale's job card is. Turning toasts off
# stops new ones; this clears the one already on screen.
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class Keys {
  [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
}
'@
# Win+N opens the notification centre.
[Keys]::keybd_event(0x5B, 0, 0, [UIntPtr]::Zero)
[Keys]::keybd_event(0x4E, 0, 0, [UIntPtr]::Zero)
[Keys]::keybd_event(0x4E, 0, 2, [UIntPtr]::Zero)
[Keys]::keybd_event(0x5B, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 1200

$root = [System.Windows.Automation.AutomationElement]::RootElement
$condition = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Button)
$buttons = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
$cleared = $false
foreach ($button in $buttons) {
  $name = $button.Current.Name
  if ($name -match "Clear all|Cancella tutto") {
    $pattern = $null
    if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
      $pattern.Invoke()
      $cleared = $true
      break
    }
  }
}
Write-Host ("cleared=" + $cleared)
Start-Sleep -Milliseconds 600
[Keys]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero)
[Keys]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 600
