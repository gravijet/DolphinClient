; DolphinClient Launcher — Windows-Installer (NSIS).
; Baubar von Linux aus: makensis -DVERSION=x.y.z -DBINDIR=<dir> -DOUTFILE=<exe> launcher-installer.nsi
;
; Installiert pro Benutzer (kein Admin nötig) nach
;   %LOCALAPPDATA%\Programs\DolphinClient
; mit Startmenü- + Desktop-Verknüpfung, Uninstaller und Systemsteuerungs-Eintrag.
; Nach der Installation startet der Launcher automatisch — auch beim stillen
; Selbst-Update (/S), das der Launcher selbst anstößt.

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef BINDIR
  !define BINDIR "../launcher-native/target/x86_64-pc-windows-gnu/release"
!endif
!ifndef OUTFILE
  !define OUTFILE "DolphinClient-Setup-${VERSION}.exe"
!endif

!define APPNAME "DolphinClient"
!define EXE "dolphinclient-launcher.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

Name "${APPNAME} Launcher ${VERSION}"
OutFile "${OUTFILE}"
Unicode true
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
Icon "../assets/brand/dolphin.ico"
UninstallIcon "../assets/brand/dolphin.ico"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  ; Beim stillen Selbst-Update beendet sich der alte Launcher gerade erst —
  ; kurz warten, damit die .exe nicht mehr gesperrt ist.
  IfSilent 0 +2
    Sleep 1500

  SetOutPath "$INSTDIR"
  File "${BINDIR}/${EXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Verknüpfungen.
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${EXE}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME} deinstallieren.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXE}"

  ; Eintrag in "Apps & Features".
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME} Launcher"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "DolphinClient"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; Launcher starten (normal wie still — nach einem Selbst-Update fühlt sich
  ; das wie ein nahtloser Neustart an).
  Exec '"$INSTDIR\${EXE}"'
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME} deinstallieren.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
SectionEnd
