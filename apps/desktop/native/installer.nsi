Unicode true
!include FileFunc.nsh
!include LogicLib.nsh
!include x64.nsh
!include Win/WinError.nsh
!define /ifndef ERROR_DIR_NOT_EMPTY 145

Name "${PRODUCT}"
OutFile "${ARTIFACT}"
Icon "${ICON}"
UninstallIcon "${ICON}"
RequestExecutionLevel user
CRCCheck force
AllowSkipFiles off
SetCompressor lzma
!ifdef SIGN_UNINSTALLER
!uninstfinalize 'node scripts/lib/rune-windows.mjs "%1"' = 0
!endif
AutoCloseWindow true
ShowInstDetails nevershow
ShowUninstDetails nevershow
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

VIProductVersion "${NUMERIC_VERSION}.0"
VIAddVersionKey /LANG=1033 "CompanyName" "T3 Tools"
VIAddVersionKey /LANG=1033 "ProductName" "${PRODUCT}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "FileDescription" "${PRODUCT} Installer"
VIAddVersionKey /LANG=1033 "LegalCopyright" "T3 Tools"

!define ROOT "$LOCALAPPDATA\Programs\${INSTALL_NAME}"
!define ARP "Software\Microsoft\Windows\CurrentVersion\Uninstall\${INSTALL_NAME}"
!define LINK "$SMPROGRAMS\${INSTALL_NAME}.lnk"
!define DESKTOP_LINK "$DESKTOP\${INSTALL_NAME}.lnk"
Var stage
Var previous
Var oldVersion
Var pendingCleanup

!macro LockInstaller
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Local\${INSTALL_NAME}-Installer")p.r0 ?e'
  Pop $1
  ${If} $0 == 0
  ${OrIf} $1 == 183
    MessageBox MB_OK "Another ${PRODUCT} installation is in progress." /SD IDOK
    Abort
  ${EndIf}
!macroend

!macro CheckProgramClosed file errorLabel
  ${If} ${FileExists} "${file}"
    ClearErrors
    FileOpen $0 "${file}" a
    IfErrors ${errorLabel}
    FileClose $0
  ${EndIf}
!macroend

!macro DefineCleanup prefix
Function ${prefix}CleanRetired
  StrCpy $pendingCleanup ""
  FindFirst $2 $3 "${ROOT}\retired\*"
  ${DoWhile} $3 != ""
    ${If} $3 != "."
    ${AndIf} $3 != ".."
    ${AndIf} ${FileExists} "${ROOT}\retired\$3\Uninstall.exe"
      StrCpy $4 "${ROOT}\retired\$3"
      ClearErrors
      ExecWait '"$4\Uninstall.exe" /S /Payload _?=$4' $5
      ${If} ${Errors}
      ${OrIf} $5 != 0
        StrCpy $pendingCleanup "$4"
      ${Else}
        Delete "$4\Uninstall.exe"
        ${If} ${FileExists} "$4\Uninstall.exe"
          StrCpy $pendingCleanup "$4"
        ${EndIf}
        RMDir "$4"
      ${EndIf}
    ${EndIf}
    FindNext $2 $3
  ${Loop}
  FindClose $2
  RMDir "${ROOT}\retired"
  ${If} $pendingCleanup != ""
    WriteRegStr HKCU "${ARP}.Cleanup" "DisplayName" "Finish removing ${INSTALL_NAME}"
    WriteRegStr HKCU "${ARP}.Cleanup" "Publisher" "T3 Tools"
    WriteRegStr HKCU "${ARP}.Cleanup" "UninstallString" '"$pendingCleanup\Uninstall.exe"'
    WriteRegStr HKCU "${ARP}.Cleanup" "QuietUninstallString" '"$pendingCleanup\Uninstall.exe" /S'
  ${Else}
    DeleteRegKey HKCU "${ARP}.Cleanup"
  ${EndIf}
FunctionEnd
!macroend
!insertmacro DefineCleanup ""
!insertmacro DefineCleanup "un."

Function .onInit
  !insertmacro LockInstaller
  ${IfNot} ${NATIVE_ARCH}
    MessageBox MB_OK "Use the ${PRODUCT} installer for this PC’s architecture." /SD IDOK
    Abort
  ${EndIf}
FunctionEnd

Section "Install"
  SetRegView 64
  SetShellVarContext current
  StrCpy $INSTDIR "${ROOT}"
  !insertmacro CheckProgramClosed "$INSTDIR\app\${PRODUCT}.exe" failed
  !insertmacro CheckProgramClosed "$INSTDIR\app\resources\node.exe" failed
  ReadRegStr $oldVersion HKCU "${ARP}" "DisplayVersion"
  ClearErrors
  CreateDirectory "$INSTDIR"
  GetTempFileName $stage "$INSTDIR"
  IfErrors failed
  Delete "$stage"
  SetOutPath "$stage"
  File /r "${PAYLOAD}/*"
  WriteUninstaller "$stage\Uninstall.exe"
  IfErrors failed
  SetOutPath "$INSTDIR"
  ${If} ${FileExists} "$INSTDIR\app\*.*"
    ${If} $oldVersion != ""
    ${AndIf} ${FileExists} "$INSTDIR\app\Uninstall.exe"
      CreateDirectory "${ROOT}\retired"
      GetTempFileName $previous "${ROOT}\retired"
    ${Else}
      GetTempFileName $previous "$INSTDIR"
    ${EndIf}
    IfErrors failed
    Delete "$previous"
    Rename "$INSTDIR\app" "$previous"
    IfErrors failed
  ${EndIf}
  Rename "$stage" "$INSTDIR\app"
  IfErrors restore
  ClearErrors
  CreateShortcut "${LINK}" "$INSTDIR\app\${PRODUCT}.exe"
  ${If} $previous == ""
    CreateShortcut "${DESKTOP_LINK}" "$INSTDIR\app\${PRODUCT}.exe"
  ${EndIf}
  WriteRegStr HKCU "${ARP}" "DisplayName" "${INSTALL_NAME}"
  WriteRegStr HKCU "${ARP}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${ARP}" "Publisher" "T3 Tools"
  WriteRegStr HKCU "${ARP}" "InstallLocation" "$INSTDIR\app"
  WriteRegStr HKCU "${ARP}" "DisplayIcon" "$INSTDIR\app\${PRODUCT}.exe"
  WriteRegStr HKCU "${ARP}" "UninstallString" '"$INSTDIR\app\Uninstall.exe"'
  WriteRegStr HKCU "${ARP}" "QuietUninstallString" '"$INSTDIR\app\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${ARP}" "NoModify" 1
  WriteRegDWORD HKCU "${ARP}" "NoRepair" 1
  IfErrors unpublish
  Call CleanRetired
  Goto done
unpublish:
  Rename "$INSTDIR\app" "$stage"
  ${If} $previous == ""
    Delete "${LINK}"
    Delete "${DESKTOP_LINK}"
    DeleteRegKey HKCU "${ARP}"
  ${Else}
    WriteRegStr HKCU "${ARP}" "DisplayVersion" "$oldVersion"
  ${EndIf}
restore:
  ${If} $previous != ""
    Rename "$previous" "$INSTDIR\app"
  ${EndIf}
failed:
  SetOutPath "$TEMP"
  ${If} $stage != ""
    RMDir /r "$stage"
  ${EndIf}
  MessageBox MB_OK "Could not install ${PRODUCT}. Close the app and retry." /SD IDOK
  SetErrorLevel 2
  Abort
done:
SectionEnd

Function un.RemovePayloadDirectory
  Pop $0
  System::Call 'kernel32::RemoveDirectoryW(w r0) i.r1 ?e'
  Pop $2
  ${If} $1 == 0
  ${AndIf} $2 != ${ERROR_DIR_NOT_EMPTY}
  ${AndIf} $2 != ${ERROR_FILE_NOT_FOUND}
  ${AndIf} $2 != ${ERROR_PATH_NOT_FOUND}
    SetErrorLevel 2
    Abort
  ${EndIf}
FunctionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext current
  ${GetParent} "$INSTDIR" $0
  ${If} $0 == "${ROOT}\retired"
    ${GetParameters} $1
    ClearErrors
    ${GetOptions} "$1" "/Payload" $2
    ${IfNot} ${Errors}
      ClearErrors
      !include "${REMOVALS}"
      SetErrorLevel 0
      Goto done
    ${EndIf}
  ${ElseIf} $INSTDIR != "${ROOT}\app"
    SetErrorLevel 2
    Abort
  ${EndIf}
  !insertmacro LockInstaller
  SetOutPath "$TEMP"
  ${If} $INSTDIR == "${ROOT}\app"
    !insertmacro CheckProgramClosed "$INSTDIR\${PRODUCT}.exe" busy
    !insertmacro CheckProgramClosed "$INSTDIR\resources\node.exe" busy
    ClearErrors
    CreateDirectory "${ROOT}\retired"
    GetTempFileName $previous "${ROOT}\retired"
    IfErrors busy
    Delete "$previous"
    Rename "$INSTDIR" "$previous"
    IfErrors busy
    Delete "${LINK}"
    Delete "${DESKTOP_LINK}"
    DeleteRegKey HKCU "${ARP}"
  ${EndIf}
  Call un.CleanRetired
  ${If} $pendingCleanup != ""
    Goto busy
  ${EndIf}
  RMDir "${ROOT}"
  Goto done
busy:
  MessageBox MB_OK "Close ${PRODUCT} and retry removal from Windows Installed apps." /SD IDOK
  SetErrorLevel 2
  Abort
done:
SectionEnd
