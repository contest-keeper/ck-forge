; The installer of the egui window. Per machine, so the elevated helper (ck-forge.exe) sits in a
; folder only administrators can write: it is started with administrator rights.
;
; makensis /DVERSION=0.2.0-preview.14 /DAPP=...\ck-forge-egui.exe /DHELPER=...\ck-forge.exe
;          /DLICENCE=...\LICENSE /DTHIRDPARTY=...\THIRD-PARTY-LICENSES.txt /DOUTFILE=...\setup.exe keeper-forge.nsi
;
; It installs into its own folder and its own uninstall entry, so it can sit next to the installer of
; the Tauri window until that one is removed. /S installs without asking (the updater uses it).

Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !error "give the version: /DVERSION=x.y.z"
!endif

Name "Keeper Forge ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Keeper Forge New"
InstallDirRegKey HKLM "Software\Contest Keeper\Keeper Forge New" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!insertmacro MUI_PAGE_LICENSE "${LICENCE}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  ; The shortcut goes in the Start menu of every user, not of the administrator who typed the password.
  SetShellVarContext all
  SetOutPath "$INSTDIR"
  ; A running copy cannot be replaced: the updater closes it first, and a person is asked to.
  File /oname=keeper-forge.exe "${APP}"
  File /oname=ck-forge.exe "${HELPER}"
  File /oname=LICENSE.txt "${LICENCE}"
  File /oname=THIRD-PARTY-LICENSES.txt "${THIRDPARTY}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortCut "$SMPROGRAMS\Keeper Forge New.lnk" "$INSTDIR\keeper-forge.exe"
  WriteRegStr HKLM "Software\Contest Keeper\Keeper Forge New" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew" "DisplayName" "Keeper Forge (new window)"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew" "Publisher" "Contest Keeper"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  Delete "$INSTDIR\keeper-forge.exe"
  Delete "$INSTDIR\ck-forge.exe"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Keeper Forge New.lnk"
  DeleteRegKey HKLM "Software\Contest Keeper\Keeper Forge New"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\KeeperForgeNew"
SectionEnd
