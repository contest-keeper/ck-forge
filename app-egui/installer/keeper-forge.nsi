; The Keeper Forge installer. Per machine, so the elevated helper (ck-forge.exe) sits in a folder only
; administrators can write: it is started with administrator rights.
;
; makensis /DVERSION=0.2.0-preview.14 /DAPP=...\ck-forge-egui.exe /DHELPER=...\ck-forge.exe
;          /DLICENCE=...\LICENSE /DTHIRDPARTY=...\THIRD-PARTY-LICENSES.txt /DOUTFILE=...\setup.exe keeper-forge.nsi
;
; It replaces the installer of the Tauri window and the "Keeper Forge New" preview. The user's data
; (profiles in %APPDATA%\org.contest-keeper.forge, the sign-in in the credential vault) is not touched.
;
; /S installs without asking (the egui updater). The Tauri updater passes /P /UPDATE /R /ARGS:
; /P shows only the progress, /R starts the program afterwards.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !error "give the version: /DVERSION=x.y.z"
!endif

!define UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall"
; Not "Keeper Forge": an entry of that name is how a Tauri install is recognised.
!define UNINSTKEY "${UNINSTALL}\KeeperForge"
; The Tauri installer keeps its folder in the default value of this key; this one writes it too.
!define DIRKEY "Software\Contest Keeper\Keeper Forge"
!define TAURIKEY "${UNINSTALL}\Keeper Forge"
!define PREVIEWKEY "Software\Contest Keeper\Keeper Forge New"

Name "Keeper Forge ${VERSION}"
OutFile "${OUTFILE}"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

Var Passive
Var Restart
Var TauriDir
Var PreviewDir
Var HadDesktop
Var Busy

; Relative paths here are read from the folder of this script.
!define MUI_ICON "keeper-forge.ico"
!define MUI_UNICON "keeper-forge.ico"

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_LICENSE "${LICENCE}"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ; The Start menu and desktop of every user, not of the administrator who typed the password.
  SetShellVarContext all
  ; The Tauri installer wrote the 64-bit registry.
  SetRegView 64

  ${GetParameters} $R0
  StrCpy $Passive 0
  ClearErrors
  ${GetOptions} $R0 "/P" $R1
  ${IfNot} ${Errors}
    StrCpy $Passive 1
    SetAutoClose true
  ${EndIf}
  StrCpy $Restart 0
  ClearErrors
  ${GetOptions} $R0 "/R" $R1
  ${IfNot} ${Errors}
    StrCpy $Restart 1
  ${EndIf}

  ReadRegStr $R1 HKLM "${DIRKEY}" ""
  StrCpy $TauriDir ""
  ReadRegStr $R2 HKLM "${TAURIKEY}" "UninstallString"
  ${If} $R2 != ""
    StrCpy $TauriDir $R1
    ${If} $TauriDir == ""
      StrCpy $TauriDir "$PROGRAMFILES64\Keeper Forge"
    ${EndIf}
  ${EndIf}
  ; Always here, whatever /D= or the registry say: ck-forge.exe is started with administrator
  ; rights, so a standard user must not be able to write to its folder and swap it.
  StrCpy $INSTDIR "$PROGRAMFILES64\Keeper Forge"

  ; The preview installer wrote the 32-bit registry.
  SetRegView 32
  ReadRegStr $PreviewDir HKLM "${PREVIEWKEY}" "InstallDir"
  SetRegView 64
FunctionEnd

Function SkipIfPassive
  ${If} $Passive = 1
    Abort
  ${EndIf}
FunctionEnd

; Sets $Busy when a program that has to be replaced is running: Windows will not open it for writing.
!macro CheckFree FILE
  ${If} ${FileExists} "${FILE}"
    ClearErrors
    FileOpen $R9 "${FILE}" a
    ${If} ${Errors}
      StrCpy $Busy 1
    ${Else}
      FileClose $R9
    ${EndIf}
  ${EndIf}
!macroend

; Waits for the programs to close, before anything is changed. Nothing is killed here: ck-forge.exe
; may be in the middle of writing a stick.
Function WaitUntilClosed
  StrCpy $R8 0
  check:
  StrCpy $Busy 0
  !insertmacro CheckFree "$INSTDIR\keeper-forge.exe"
  !insertmacro CheckFree "$INSTDIR\ck-forge.exe"
  ${If} $TauriDir != ""
    !insertmacro CheckFree "$TauriDir\ck-forge-app.exe"
    !insertmacro CheckFree "$TauriDir\ck-forge.exe"
  ${EndIf}
  ${If} $PreviewDir != ""
    !insertmacro CheckFree "$PreviewDir\keeper-forge.exe"
    !insertmacro CheckFree "$PreviewDir\ck-forge.exe"
  ${EndIf}
  ${If} $Busy = 0
    Return
  ${EndIf}
  ; An updater starts the installer while the window is still closing.
  IntOp $R8 $R8 + 1
  ${If} $R8 < 20
    Sleep 500
    Goto check
  ${EndIf}
  ${If} $Passive = 0
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Keeper Forge is still running. Close it, and wait for any stick being written, then retry." /SD IDCANCEL IDRETRY check
  ${EndIf}
  Abort "Keeper Forge is still running: nothing was changed."
FunctionEnd

; In Program Files, the Tauri window's own uninstaller, silently: it removes its files, shortcuts and
; entry. It deletes application data only when its "delete app data" box was ticked, which a silent
; run never shows.
Function RemoveTauri
  ${If} $TauriDir == ""
    Return
  ${EndIf}
  ; Nothing from any other folder is run: this installer is elevated, and a standard user who can
  ; write there could swap uninstall.exe. Only the exact default passes (no "..", no look-alike).
  ; The known files are deleted by name instead; the shortcuts are overwritten by the new ones.
  ${If} $TauriDir != "$PROGRAMFILES64\Keeper Forge"
    ClearErrors
    Delete "$TauriDir\ck-forge-app.exe"
    Delete "$TauriDir\ck-forge.exe"
    Delete "$TauriDir\WebView2Loader.dll"
    ${If} ${Errors}
      Abort "The earlier Keeper Forge in $TauriDir could not be removed. Restart Windows, then run this installer again."
    ${EndIf}
    Delete "$TauriDir\uninstall.exe"
    ${If} ${Errors}
      Abort "Delete $TauriDir\uninstall.exe, then run this installer again."
    ${EndIf}
    ; Only when empty: anything else in the folder stays.
    RMDir "$TauriDir"
  ; CheckTauri has made sure that without an uninstaller there is no program left either.
  ${ElseIf} ${FileExists} "$TauriDir\uninstall.exe"
    ClearErrors
    StrCpy $0 1
    ; _?= makes ExecWait wait for the uninstaller itself; it must come last and unquoted.
    ExecWait '"$TauriDir\uninstall.exe" /S _?=$TauriDir' $0
    ${If} ${Errors}
    ${OrIf} $0 <> 0
      Abort "The earlier Keeper Forge could not be removed (code $0)."
    ${EndIf}
    ${If} ${FileExists} "$TauriDir\ck-forge-app.exe"
      Abort "The earlier Keeper Forge in $TauriDir could not be removed. Restart Windows, then run this installer again."
    ${EndIf}
    ; Run with _?= it cannot delete itself.
    Delete "$TauriDir\uninstall.exe"
    RMDir "$TauriDir"
  ${EndIf}
  ; Left over only when its uninstaller and its program were already gone.
  DeleteRegKey HKLM "${TAURIKEY}"
FunctionEnd

; A program without its uninstaller cannot be removed cleanly, and forgetting its entry would leave
; it behind for good. Stops before anything is changed.
Function CheckTauri
  ${If} $TauriDir == ""
  ${OrIf} ${FileExists} "$TauriDir\uninstall.exe"
    Return
  ${EndIf}
  ${If} ${FileExists} "$TauriDir\ck-forge-app.exe"
    StrCpy $R0 "The earlier Keeper Forge has no uninstaller. Delete $TauriDir\ck-forge-app.exe (your profiles are not kept there), then run this installer again."
    ; A passive run shows the same line on its progress page and asks nothing.
    ${If} $Passive = 0
      MessageBox MB_ICONSTOP $R0 /SD IDOK
    ${EndIf}
    Abort $R0
  ${EndIf}
FunctionEnd

; Only the files the preview installed, so nothing else in its folder is lost.
Function RemovePreview
  ${If} $PreviewDir == ""
    Return
  ${EndIf}
  ClearErrors
  Delete "$PreviewDir\keeper-forge.exe"
  Delete "$PreviewDir\ck-forge.exe"
  Delete "$PreviewDir\LICENSE.txt"
  Delete "$PreviewDir\THIRD-PARTY-LICENSES.txt"
  ; Its uninstaller and entry stay while a file does, so the preview can still be removed.
  ${If} ${Errors}
    DetailPrint "The preview in $PreviewDir could not be removed: remove it in Windows Settings."
    Return
  ${EndIf}
  Delete "$PreviewDir\uninstall.exe"
  ${If} ${Errors}
    DetailPrint "The preview in $PreviewDir could not be removed: remove it in Windows Settings."
    Return
  ${EndIf}
  RMDir "$PreviewDir"
  Delete "$SMPROGRAMS\Keeper Forge New.lnk"
  SetRegView 32
  DeleteRegKey HKLM "${PREVIEWKEY}"
  DeleteRegKey HKLM "${UNINSTALL}\KeeperForgeNew"
  SetRegView 64
FunctionEnd

; Moves one staged file into place.
!macro Activate NAME
  ClearErrors
  Delete "$INSTDIR\${NAME}"
  Rename "$INSTDIR\staging\${NAME}" "$INSTDIR\${NAME}"
  ${If} ${Errors}
    Abort "Could not put ${NAME} in place: run this installer again."
  ${EndIf}
!macroend

Section "Install"
  Call CheckTauri
  Call WaitUntilClosed

  ; Everything new is written out before anything old is removed, so a failed extraction (a full
  ; disk, an antivirus) leaves the earlier install working.
  ClearErrors
  SetOutPath "$INSTDIR\staging"
  File /oname=keeper-forge.exe "${APP}"
  File /oname=ck-forge.exe "${HELPER}"
  File /oname=LICENSE.txt "${LICENCE}"
  File /oname=THIRD-PARTY-LICENSES.txt "${THIRDPARTY}"
  File "keeper-forge.ico"
  WriteUninstaller "$INSTDIR\staging\uninstall.exe"
  ${If} ${Errors}
    Abort "Could not unpack Keeper Forge: nothing was removed."
  ${EndIf}
  ; Windows does not delete the folder a program is working in.
  SetOutPath "$INSTDIR"

  ; Looked at before the Tauri uninstaller deletes it.
  StrCpy $HadDesktop 0
  ${If} ${FileExists} "$DESKTOP\Keeper Forge.lnk"
    StrCpy $HadDesktop 1
  ${EndIf}
  Call RemoveTauri
  Call RemovePreview

  !insertmacro Activate "keeper-forge.exe"
  !insertmacro Activate "ck-forge.exe"
  !insertmacro Activate "LICENSE.txt"
  !insertmacro Activate "THIRD-PARTY-LICENSES.txt"
  !insertmacro Activate "keeper-forge.ico"
  !insertmacro Activate "uninstall.exe"
  RMDir "$INSTDIR\staging"
  CreateShortCut "$SMPROGRAMS\Keeper Forge.lnk" "$INSTDIR\keeper-forge.exe" "" "$INSTDIR\keeper-forge.ico"
  ${If} $HadDesktop = 1
    CreateShortCut "$DESKTOP\Keeper Forge.lnk" "$INSTDIR\keeper-forge.exe" "" "$INSTDIR\keeper-forge.ico"
  ${EndIf}
  WriteRegStr HKLM "${DIRKEY}" "" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayName" "Keeper Forge"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\keeper-forge.ico"
  WriteRegStr HKLM "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTKEY}" "Publisher" "Contest Keeper"
  WriteRegStr HKLM "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Function .onInstSuccess
  ${If} $Passive = 0
  ${AndIfNot} ${Silent}
    Return
  ${EndIf}
  ; The preview's updater passes only /S and then starts the program at its old path, which is gone.
  ${If} $Restart = 1
  ${OrIf} $PreviewDir != ""
    ; Through Explorer, so the window does not run as administrator.
    ; shortcut: the arguments after /ARGS are dropped, pass them on if the window ever takes any.
    Exec '"$WINDIR\explorer.exe" "$INSTDIR\keeper-forge.exe"'
  ${EndIf}
FunctionEnd

Function un.onInit
  SetShellVarContext all
  SetRegView 64
FunctionEnd

Section "Uninstall"
  StrCpy $Busy 0
  !insertmacro CheckFree "$INSTDIR\keeper-forge.exe"
  !insertmacro CheckFree "$INSTDIR\ck-forge.exe"
  ${If} $Busy = 1
    Abort "Keeper Forge is still running: close it and uninstall again."
  ${EndIf}
  ClearErrors
  Delete "$INSTDIR\keeper-forge.exe"
  Delete "$INSTDIR\ck-forge.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt"
  Delete "$INSTDIR\keeper-forge.ico"
  ; Left by an install that failed half way.
  Delete "$INSTDIR\staging\keeper-forge.exe"
  Delete "$INSTDIR\staging\ck-forge.exe"
  Delete "$INSTDIR\staging\LICENSE.txt"
  Delete "$INSTDIR\staging\THIRD-PARTY-LICENSES.txt"
  Delete "$INSTDIR\staging\keeper-forge.ico"
  Delete "$INSTDIR\staging\uninstall.exe"
  ; The uninstaller and the entry stay while a file does, so it can be tried again.
  ${If} ${Errors}
    Abort "Some files could not be deleted: restart Windows and uninstall again."
  ${EndIf}
  Delete "$INSTDIR\uninstall.exe"
  ${If} ${Errors}
    Abort "The uninstaller could not be deleted: restart Windows and uninstall again."
  ${EndIf}
  RMDir "$INSTDIR\staging"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Keeper Forge.lnk"
  Delete "$DESKTOP\Keeper Forge.lnk"
  DeleteRegKey HKLM "${DIRKEY}"
  DeleteRegKey /ifempty HKLM "Software\Contest Keeper"
  DeleteRegKey HKLM "${UNINSTKEY}"
SectionEnd
