; Ganchos del instalador NSIS de Alfredo Caja (Tauri v2, bundle.windows.nsis.installerHooks).
; Agrega el acceso directo «Alfredo Caja · Soporte remoto» en el menú Inicio: abre (o avisa a
; la instancia que ya corre) con --soporte, que inicia RustDesk y muestra el ID (T9.9, RF9.15).

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$SMPROGRAMS\Alfredo"
  CreateShortCut "$SMPROGRAMS\Alfredo\Alfredo Caja - Soporte remoto.lnk" "$INSTDIR\Alfredo Caja.exe" "--soporte" "$INSTDIR\Alfredo Caja.exe" 0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$SMPROGRAMS\Alfredo\Alfredo Caja - Soporte remoto.lnk"
!macroend
