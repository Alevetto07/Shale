; Shale installs per user and claims only its own file type.
;
; Optional CLI/MCP: Tauri 2's bundled NSIS template has no component sections, so
; we do not ship shale-cli/shale-mcp as a tick-box here. Package them with
; scripts/package-cli.ps1 and document the zip beside the GUI installer.
;
; .shale is ours, so the bundle registers it as the default handler. Model
; files are somebody else's: Bambu Studio or Cura is probably the default, and
; an installer that quietly takes .stl away from it is rude. So Shale only
; offers itself in "Open with" for those, and takes the entry back on uninstall.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\.3mf\OpenWithProgids" "Shale project" ""
  WriteRegStr HKCU "Software\Classes\.stl\OpenWithProgids" "Shale project" ""
  WriteRegStr HKCU "Software\Classes\.step\OpenWithProgids" "Shale project" ""
  WriteRegStr HKCU "Software\Classes\.stp\OpenWithProgids" "Shale project" ""
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Let a polite close finish before files are removed; taskkill is the last resort.
  nsExec::ExecToLog 'powershell -NoProfile -Command "$p = Get-Process Shale -ErrorAction SilentlyContinue; if ($p) { $p | ForEach-Object { $_.CloseMainWindow() | Out-Null }; $deadline=(Get-Date).AddSeconds(20); while ((Get-Date) -lt $deadline -and (Get-Process Shale -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 250 } }"'
  Sleep 500
  nsExec::ExecToLog 'taskkill /IM Shale.exe /F /T'
  Sleep 1000
  DeleteRegValue HKCU "Software\Classes\.3mf\OpenWithProgids" "Shale project"
  DeleteRegValue HKCU "Software\Classes\.stl\OpenWithProgids" "Shale project"
  DeleteRegValue HKCU "Software\Classes\.step\OpenWithProgids" "Shale project"
  DeleteRegValue HKCU "Software\Classes\.stp\OpenWithProgids" "Shale project"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The bundle's own association cleanup runs after the pre-uninstall hook and
  ; leaves an empty .shale key behind, so the key goes last: Shale was the only
  ; owner of that extension.
  DeleteRegKey HKCU "Software\Classes\.shale"
  ; Parsed catalogue cache lives in roaming app data, not beside the binary.
  RMDir /r "$APPDATA\Shale\catalog-cache"
  ; Earlier builds used this folder; remove it if nothing else is there.
  RMDir /r "$LOCALAPPDATA\Shale\cache"
  RMDir "$LOCALAPPDATA\Shale"
!macroend
