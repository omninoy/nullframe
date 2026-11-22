!macro NSIS_HOOK_PREINSTALL
  IfFileExists "$INSTDIR\nullframe-proxy.exe" 0 nullframe_proxy_preinstall_done

  DetailPrint "Stopping NullFrame proxy workers before replacing application files"
  nsExec::ExecToStack '"$SYSDIR\taskkill.exe" /F /T /IM "nullframe-proxy.exe"'
  Pop $0
  Pop $1
  Sleep 1000

  ; Removing the old sidecar first prevents NSIS from retaining a same-version
  ; or previously locked executable while updating the main application.
  Delete "$INSTDIR\nullframe-proxy.exe"

  nullframe_proxy_preinstall_done:
!macroend
