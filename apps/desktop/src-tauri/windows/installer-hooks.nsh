; Shale installs per user and claims only its own file type.
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
  ; The bundle's own association cleanup leaves an empty .shale key behind, so
  ; the key itself goes too: Shale was the only owner of that extension.
  DeleteRegKey HKCU "Software\Classes\.shale"
  DeleteRegValue HKCU "Software\Classes\.3mf\OpenWithProgids" "Shale project"
  DeleteRegValue HKCU "Software\Classes\.stl\OpenWithProgids" "Shale project"
  DeleteRegValue HKCU "Software\Classes\.step\OpenWithProgids" "Shale project"
  DeleteRegValue HKCU "Software\Classes\.stp\OpenWithProgids" "Shale project"
!macroend
