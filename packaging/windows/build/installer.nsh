; Kairo installer additions (prompt 102), included by electron-builder's NSIS
; templates (`nsis.include`). LogicLib, `${Silent}` and BUILD_RESOURCES_DIR come
; from those templates.
;
; WebView2: web pages, Slido and PowerPoint import use the Microsoft Edge
; WebView2 runtime (DECISIONS, "Replace CEF with gpui-webview"). Windows 11
; ships it; some Windows 10 installs lack it. When neither the per-machine nor
; the per-user runtime is registered, run Microsoft's Evergreen bootstrapper,
; which package.ps1 downloads and checks for Microsoft's signature. Run without
; elevation, it installs the runtime for the current user. Kairo itself still
; installs if this fails; the web features then report the missing runtime.

!ifndef BUILD_UNINSTALLER

!define KAIRO_WEBVIEW2_CLIENT "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

; Sets OUT to the registered runtime version, or "" when none is installed.
!macro kairoWebView2Version OUT
  ReadRegStr ${OUT} HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${KAIRO_WEBVIEW2_CLIENT}" "pv"
  ${If} ${OUT} == "0.0.0.0"
    StrCpy ${OUT} ""
  ${EndIf}
  ${If} ${OUT} == ""
    ReadRegStr ${OUT} HKCU "Software\Microsoft\EdgeUpdate\Clients\${KAIRO_WEBVIEW2_CLIENT}" "pv"
    ${If} ${OUT} == "0.0.0.0"
      StrCpy ${OUT} ""
    ${EndIf}
  ${EndIf}
!macroend

!macro customInstall
  !insertmacro kairoWebView2Version $0
  ${If} $0 == ""
    DetailPrint "Installing the Microsoft Edge WebView2 Runtime"
    File "/oname=$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" "${BUILD_RESOURCES_DIR}\MicrosoftEdgeWebview2Setup.exe"
    ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
    Delete "$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe"
    ${If} $1 != 0
      DetailPrint "WebView2 Runtime setup failed with exit code $1"
      MessageBox MB_OK|MB_ICONINFORMATION "Kairo is installed, but the Microsoft Edge WebView2 Runtime could not be installed (error $1).$\r$\n$\r$\nWeb pages, Slido and PowerPoint import need it. Install it from https://developer.microsoft.com/microsoft-edge/webview2/ and then restart Kairo." /SD IDOK
    ${EndIf}
  ${EndIf}
!macroend

!endif
