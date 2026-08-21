; NSIS installer hooks for vantage-box.
;
; Registers the `vantage://` URI scheme for the current user. The bundle uses
; `installMode: currentUser`, so we write to HKCU (no admin elevation) — this
; mirrors the per-user install and keeps the scheme private to the user.
;
; The OS launches us as `vantage-box.exe "uri" "<url>"`. When the app is already
; running, `tauri-plugin-single-instance` forwards that second launch's argv to
; the first instance; on a cold start, `setup()` finds the URI in argv. Both
; paths funnel into `uri::dispatch`.
;
; `$\"` is the NSIS escape for a literal double quote, so the registry value
; ends up as:  "C:\...\vantage-box.exe" "uri" "%1"
; The path is quoted in case $INSTDIR contains spaces.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\vantage" "" "URL:Vantage Box"
  WriteRegStr HKCU "Software\Classes\vantage" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\vantage\shell\open\command" "" "$\"$INSTDIR\vantage-box.exe$\" $\"uri$\" $\"%1$\""
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\vantage"
!macroend