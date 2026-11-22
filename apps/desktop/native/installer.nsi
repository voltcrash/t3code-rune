Unicode true
!include FileFunc.nsh
!include LogicLib.nsh
!include x64.nsh
!include Win/WinError.nsh
!addplugindir /x86-unicode "${PLUGIN_DIR}"
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

!ifdef RELEASE_INSTALL
  VIAddVersionKey /LANG=1033 "InternalName" "T3Code.ReleaseInstaller.${RELEASE_ARCH}"
  !include "${__FILEDIR__}/rune-install-location.nsh"
  !include "${__FILEDIR__}/rune-url-protocol.nsh"
!else
  !define ROOT "$LOCALAPPDATA\Programs\${INSTALL_NAME}"
  !define ACTIVE "${ROOT}\app"
  !define SHORTCUT_NAME "${INSTALL_NAME}"
  !define DISPLAY_NAME "${INSTALL_NAME}"
!endif
!define ARP "Software\Microsoft\Windows\CurrentVersion\Uninstall\${INSTALL_NAME}"
!define LINK "$SMPROGRAMS\${SHORTCUT_NAME}.lnk"
!define DESKTOP_LINK "$DESKTOP\${SHORTCUT_NAME}.lnk"
Var stage
Var previous
Var oldVersion
Var oldDisplayName
!ifdef RELEASE_INSTALL
Var createdStartLink
!endif
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

!ifdef RELEASE_INSTALL
!insertmacro DefineReleaseMigration
!endif

Function .onInit
  !insertmacro LockInstaller
  ${IfNot} ${NATIVE_ARCH}
    !ifdef ALLOW_EMULATED_X64
      ; Windows 11 can run existing x64 installations on Arm through emulation.
      StrCpy $1 0
      System::Call 'kernel32::GetMachineTypeAttributes(i 0x8664, *i .r1)i.r0'
      IntOp $1 $1 & 1
      ${If} $0 != 0
      ${OrIf} $1 == 0
    !endif
        MessageBox MB_OK "Use the ${PRODUCT} installer for this PC’s architecture." /SD IDOK
        Abort
    !ifdef ALLOW_EMULATED_X64
      ${EndIf}
    !endif
  ${EndIf}
  !ifdef RELEASE_INSTALL
    !insertmacro ReadReleaseLocation
    Call RecoverAdoption
    ${IfNot} ${Errors}
      Call RecoverShortcutChange
    ${EndIf}
    ${If} ${Errors}
      MessageBox MB_OK "Could not restore the previous desktop installation. Close the app and retry." /SD IDOK
      Abort
    ${EndIf}
  !endif
FunctionEnd

Function LaunchInstalledApp
  StrCpy $0 0
  StrCpy $1 ""
  System::Call 'kernel32::GetCommandLineW()p.r2'
  System::Call 'shell32::CommandLineToArgvW(p r2, *i .r3)p.r4'
  ${If} $4 != 0
    StrCpy $5 $4
    ${DoWhile} $3 > 1
      IntOp $5 $5 + ${NSIS_PTR_SIZE}
      IntOp $3 $3 - 1
      System::Call '*$5(p .r6)'
      System::Call 'kernel32::CompareStringOrdinal(p r6, i -1, w "--force-run", i -1, i 0)i.r7'
      ${If} $7 == 2
        StrCpy $0 1
      ${Else}
        System::Call 'kernel32::CompareStringOrdinal(p r6, i -1, w "--updated", i -1, i 0)i.r7'
        ${If} $7 == 2
          StrCpy $1 "--updated"
        ${EndIf}
      ${EndIf}
    ${Loop}
    System::Call 'kernel32::LocalFree(p r4)'
  ${EndIf}
  ${If} ${Silent}
  ${AndIf} $0 == 0
    Return
  ${EndIf}

  HideWindow
  StrCpy $2 "${ACTIVE}\${PRODUCT}.exe"
  ; GetCurrentProcessToken is a pseudo-handle. Only a confirmed unelevated caller may launch directly.
  System::Call 'advapi32::GetTokenInformation(p -4, i 20, *i .r3, i 4, *i .r4)i.r0'
  ${If} $0 != 0
  ${AndIf} $3 == 0
    ClearErrors
    Exec '"$2" $1'
    ${IfNot} ${Errors}
      Return
    ${EndIf}
  ${Else}
    StrLen $0 $2
    ${If} $0 < 260
      ExecInExplorer::Exec "$2" "/cmdargs" "$1"
      Pop $0
      ${If} $0 == 1
        Return
      ${EndIf}
    ${EndIf}
  ${EndIf}
  IfSilent 0 +2
    SetErrorLevel 3
  MessageBox MB_OK "${PRODUCT} was installed. Open it from the Start menu." /SD IDOK
FunctionEnd

Section "Install"
  SetRegView 64
  SetShellVarContext current
  StrCpy $INSTDIR "${ROOT}"
  !insertmacro CheckProgramClosed "${ACTIVE}\${PRODUCT}.exe" failed
  !insertmacro CheckProgramClosed "${ACTIVE}\resources\node.exe" failed
  ReadRegStr $oldVersion HKCU "${ARP}" "DisplayVersion"
  ReadRegStr $oldDisplayName HKCU "${ARP}" "DisplayName"
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
  ${If} ${FileExists} "${ACTIVE}\*.*"
    !ifdef RELEASE_INSTALL
      Call RecognizeRelease
      IfErrors failed
    !endif
    ${If} $oldVersion != ""
    ${AndIf} ${FileExists} "${ACTIVE}\Uninstall.exe"
      CreateDirectory "${ROOT}\retired"
      GetTempFileName $previous "${ROOT}\retired"
    ${Else}
      !ifdef RELEASE_INSTALL
        StrCpy $previous "${FORMER_DIR}"
        WriteRegStr HKCU "${RELEASE_LOCATION_KEY}" "InstallLocation" "${ACTIVE}"
        IfErrors failed
        WriteRegStr HKCU "${RELEASE_LOCATION_KEY}" "AdoptionStage" "$stage"
        IfErrors failed
        StrCpy $oldDisplayName "$formerProduct"
      !else
        GetTempFileName $previous "$INSTDIR"
      !endif
    ${EndIf}
    IfErrors failed
    !ifdef RELEASE_INSTALL
      ${If} $previous != "${FORMER_DIR}"
        Delete "$previous"
      ${EndIf}
    !else
      Delete "$previous"
    !endif
    !ifdef RELEASE_INSTALL
      Call RecordShortcutChange
      IfErrors failed
    !endif
    Rename "${ACTIVE}" "$previous"
    IfErrors failed
  ${EndIf}
  Rename "$stage" "${ACTIVE}"
  IfErrors restore
  ClearErrors
  WriteRegStr HKCU "${ARP}" "DisplayName" "${DISPLAY_NAME}"
  WriteRegStr HKCU "${ARP}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${ARP}" "Publisher" "T3 Tools"
  WriteRegStr HKCU "${ARP}" "InstallLocation" "${ACTIVE}"
  WriteRegStr HKCU "${ARP}" "DisplayIcon" "${ACTIVE}\${PRODUCT}.exe"
  WriteRegStr HKCU "${ARP}" "UninstallString" '"${ACTIVE}\Uninstall.exe"'
  WriteRegStr HKCU "${ARP}" "QuietUninstallString" '"${ACTIVE}\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${ARP}" "NoModify" 1
  WriteRegDWORD HKCU "${ARP}" "NoRepair" 1
  IfErrors unpublish
  !ifdef RELEASE_INSTALL
    WriteRegStr HKCU "${RELEASE_LOCATION_KEY}" "InstallLocation" "${ACTIVE}"
    IfErrors unpublish
  !endif
  !ifdef RELEASE_INSTALL
    ${IfNot} ${FileExists} "${LINK}"
      CreateShortcut "${LINK}" "${ACTIVE}\${PRODUCT}.exe"
      IfErrors unpublish
      StrCpy $createdStartLink 1
    ${ElseIf} ${FileExists} "${LINK}\*.*"
      Goto unpublish
    ${EndIf}
  !else
    CreateShortcut "${LINK}" "${ACTIVE}\${PRODUCT}.exe"
  !endif
  ${If} $previous == ""
    CreateShortcut "${DESKTOP_LINK}" "${ACTIVE}\${PRODUCT}.exe"
  ${EndIf}
  IfErrors unpublish
  !ifdef RELEASE_INSTALL
    ${If} $oldDisplayName != ""
    ${AndIf} $oldDisplayName != "${PRODUCT}"
    ${AndIf} ${FileExists} "$DESKTOP\$oldDisplayName.lnk"
      ${If} ${FileExists} "${DESKTOP_LINK}\*.*"
        Goto unpublish
      ${ElseIfNot} ${FileExists} "${DESKTOP_LINK}"
        CreateShortcut "${DESKTOP_LINK}" "${ACTIVE}\${PRODUCT}.exe"
        IfErrors unpublish
      ${EndIf}
    ${EndIf}
    ${If} $previous == "${FORMER_DIR}"
      DeleteRegValue HKCU "${RELEASE_LOCATION_KEY}" "AdoptionStage"
      IfErrors unpublish
    ${EndIf}
    ${If} $oldDisplayName != ""
    ${AndIf} $oldDisplayName != "${PRODUCT}"
      Call RecoverShortcutChange
    ${EndIf}
    Call RetirePreviousRelease
    !insertmacro RegisterRuneProtocol "t3code"
    !insertmacro RegisterRuneProtocol "t3code-dev"
  !endif
  Call CleanRetired
  Goto done
unpublish:
  !ifdef RELEASE_INSTALL
    ${If} $createdStartLink == 1
      Delete "${LINK}"
    ${EndIf}
  !endif
  Rename "${ACTIVE}" "$stage"
  ${If} $oldVersion == ""
    ${If} $previous == ""
      Delete "${LINK}"
      Delete "${DESKTOP_LINK}"
    ${EndIf}
    DeleteRegKey HKCU "${ARP}"
    !ifdef RELEASE_INSTALL
      ${If} $previous == ""
        DeleteRegKey HKCU "${RELEASE_LOCATION_KEY}"
      ${EndIf}
    !endif
  ${Else}
    WriteRegStr HKCU "${ARP}" "DisplayVersion" "$oldVersion"
    WriteRegStr HKCU "${ARP}" "DisplayName" "$oldDisplayName"
    !ifdef RELEASE_INSTALL
      WriteRegStr HKCU "${ARP}" "DisplayIcon" "${ACTIVE}\$oldDisplayName.exe"
    !endif
  ${EndIf}
restore:
  ${If} $previous != ""
    Rename "$previous" "${ACTIVE}"
  ${EndIf}
failed:
  SetOutPath "$TEMP"
  !ifdef RELEASE_INSTALL
    ${If} $previous == "${FORMER_DIR}"
      ReadRegStr $0 HKCU "${RELEASE_LOCATION_KEY}" "AdoptionStage"
      ${If} $0 != ""
        Call RecoverAdoption
        StrCpy $stage ""
      ${EndIf}
    ${EndIf}
    Call RecoverShortcutChange
  !endif
  ${If} $stage != ""
    RMDir /r "$stage"
  ${EndIf}
  MessageBox MB_OK "Could not install ${PRODUCT}. Close the app and retry." /SD IDOK
  SetErrorLevel 2
  Abort
done:
  Call LaunchInstalledApp
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

!ifdef RELEASE_INSTALL
Function un.onInit
  !insertmacro ReadReleaseLocation
FunctionEnd
!endif

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
  ${ElseIf} $INSTDIR != "${ACTIVE}"
    SetErrorLevel 2
    Abort
  ${EndIf}
  !insertmacro LockInstaller
  SetOutPath "$TEMP"
  ${If} $INSTDIR == "${ACTIVE}"
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
  !ifdef RELEASE_INSTALL
    !insertmacro UnregisterRuneProtocol "t3code"
    !insertmacro UnregisterRuneProtocol "t3code-dev"
  !endif
  Call un.CleanRetired
  ${If} $pendingCleanup != ""
    Goto busy
  ${EndIf}
  RMDir "${ROOT}"
  !ifdef RELEASE_INSTALL
    ${IfNot} ${FileExists} "${ACTIVE}\Uninstall.exe"
    ${AndIfNot} ${FileExists} "${FORMER_DIR}\*.*"
      DeleteRegKey HKCU "${RELEASE_LOCATION_KEY}"
    ${EndIf}
  !endif
  Goto done
busy:
  MessageBox MB_OK "Close ${PRODUCT} and retry removal from Windows Installed apps." /SD IDOK
  SetErrorLevel 2
  Abort
done:
SectionEnd
