!define RELEASE_LOCATION_KEY "Software\T3 Tools\Rune Desktop Installer"
!define FORMER_LOCATION_KEY "Software\e9197887-efb3-55e0-985e-d6d3b5dd594a"
!define FORMER_ARP "Software\Microsoft\Windows\CurrentVersion\Uninstall\e9197887-efb3-55e0-985e-d6d3b5dd594a"
!define ROOT "$releaseState"
!define ACTIVE "$releaseApp"
!define SHORTCUT_NAME "${PRODUCT}"
!define DISPLAY_NAME "${PRODUCT}"
!define FORMER_DIR "${ROOT}\previous-desktop"
Var releaseApp
Var releaseState
Var formerProduct

!macro ReadReleaseLocation
  SetRegView 64
  ReadRegStr $releaseApp HKCU "${RELEASE_LOCATION_KEY}" "InstallLocation"
  ${If} $releaseApp == ""
    ReadRegStr $releaseApp HKCU "${FORMER_LOCATION_KEY}" "InstallLocation"
  ${EndIf}
  ${If} $releaseApp == ""
    StrCpy $releaseApp "$LOCALAPPDATA\Programs\t3code"
  ${EndIf}
  StrCpy $releaseState "$releaseApp.rune-install"
!macroend

!macro ValidateProductName value
  ${If} ${value} != "T3 Code (Alpha)"
  ${AndIf} ${value} != "T3 Code (Beta)"
  ${AndIf} ${value} != "T3 Code (Nightly)"
  ${AndIf} ${value} != "T3 Code"
    StrCpy ${value} ""
  ${EndIf}
!macroend

!define SHORTCUT_CHANGE "${RELEASE_LOCATION_KEY}\Shortcuts"

!macro DefineReleaseMigration
Function ReadFormerProduct
  ReadRegStr $formerProduct HKCU "${FORMER_LOCATION_KEY}" "ShortcutName"
  !insertmacro ValidateProductName $formerProduct
FunctionEnd

Function RecordShortcutChange
  ${If} $oldDisplayName == ""
  ${OrIf} $oldDisplayName == "${PRODUCT}"
    ClearErrors
    Return
  ${EndIf}
  StrCpy $0 0
  ${IfNot} ${FileExists} "${LINK}"
    IntOp $0 $0 | 1
  ${EndIf}
  ${IfNot} ${FileExists} "${DESKTOP_LINK}"
    IntOp $0 $0 | 2
  ${EndIf}
  ClearErrors
  WriteRegDWORD HKCU "${SHORTCUT_CHANGE}" "AbsentLinks" $0
  WriteRegStr HKCU "${SHORTCUT_CHANGE}" "Target" "${PRODUCT}"
  WriteRegStr HKCU "${SHORTCUT_CHANGE}" "Previous" "$oldDisplayName"
FunctionEnd

Function RecoverShortcutChange
  ReadRegStr $3 HKCU "${SHORTCUT_CHANGE}" "Previous"
  ReadRegStr $4 HKCU "${SHORTCUT_CHANGE}" "Target"
  !insertmacro ValidateProductName $3
  !insertmacro ValidateProductName $4
  ${If} $3 == ""
  ${OrIf} $4 == ""
  ${OrIf} $3 == $4
    ClearErrors
    Return
  ${EndIf}
  ReadRegDWORD $5 HKCU "${SHORTCUT_CHANGE}" "AbsentLinks"
  IfErrors failed
  ${If} ${FileExists} "${ACTIVE}\$3.exe"
    IntOp $0 $5 & 1
    ${If} $0 != 0
    ${AndIf} ${FileExists} "$SMPROGRAMS\$4.lnk"
      Delete "$SMPROGRAMS\$4.lnk"
    ${EndIf}
    IntOp $0 $5 & 2
    ${If} $0 != 0
    ${AndIf} ${FileExists} "$DESKTOP\$4.lnk"
      Delete "$DESKTOP\$4.lnk"
    ${EndIf}
  ${ElseIf} ${FileExists} "${ACTIVE}\$4.exe"
    ${IfNot} ${FileExists} "$SMPROGRAMS\$4.lnk"
      CreateShortcut "$SMPROGRAMS\$4.lnk" "${ACTIVE}\$4.exe"
      IfErrors failed
    ${EndIf}
    ${If} ${FileExists} "$DESKTOP\$3.lnk"
      ${IfNot} ${FileExists} "$DESKTOP\$4.lnk"
        CreateShortcut "$DESKTOP\$4.lnk" "${ACTIVE}\$4.exe"
        IfErrors failed
      ${EndIf}
      ${If} ${FileExists} "$DESKTOP\$4.lnk\*.*"
        Goto failed
      ${EndIf}
      Delete "$DESKTOP\$3.lnk"
    ${EndIf}
    ${If} ${FileExists} "$SMPROGRAMS\$3.lnk"
    ${AndIfNot} ${FileExists} "$SMPROGRAMS\$4.lnk\*.*"
      Delete "$SMPROGRAMS\$3.lnk"
    ${EndIf}
  ${Else}
    Goto failed
  ${EndIf}
  IfErrors failed
  DeleteRegKey HKCU "${SHORTCUT_CHANGE}"
  Return
failed:
  SetErrors
FunctionEnd

Function RecognizeRelease
  ClearErrors
  ReadRegStr $0 HKCU "${RELEASE_LOCATION_KEY}" "InstallLocation"
  StrCpy $formerProduct ""
  ${If} $0 == "${ACTIVE}"
  ${AndIf} $oldVersion != ""
  ${AndIf} ${FileExists} "${ACTIVE}\Uninstall.exe"
    !insertmacro ValidateProductName $oldDisplayName
    ${If} $oldDisplayName == ""
      SetErrors
      Return
    ${EndIf}
    ClearErrors
    Return
  ${EndIf}
  ReadRegStr $0 HKCU "${FORMER_LOCATION_KEY}" "InstallLocation"
  ReadRegStr $1 HKCU "${FORMER_ARP}" "Publisher"
  Call ReadFormerProduct
  ${If} $0 != "${ACTIVE}"
  ${OrIf} $1 != "T3 Tools"
  ${OrIfNot} ${FileExists} "${ACTIVE}\resources\app.asar"
    SetErrors
    Return
  ${EndIf}
  ${If} $formerProduct != ""
  ${AndIf} $oldVersion == ""
  ${AndIf} ${FileExists} "${ACTIVE}\$formerProduct.exe"
  ${AndIf} ${FileExists} "${ACTIVE}\Uninstall $formerProduct.exe"
  ${AndIfNot} ${FileExists} "${FORMER_DIR}"
    !insertmacro CheckProgramClosed "${ACTIVE}\$formerProduct.exe" locked
    ClearErrors
    Return
  ${EndIf}
locked:
  SetErrors
FunctionEnd

Function RecoverAdoption
  ReadRegStr $7 HKCU "${RELEASE_LOCATION_KEY}" "AdoptionStage"
  ${If} $7 == ""
    ClearErrors
    Return
  ${EndIf}
  ${GetParent} "$7" $0
  ${If} $0 != "${ROOT}"
    Goto incomplete
  ${EndIf}
  ClearErrors
  ${If} ${FileExists} "${FORMER_DIR}\*.*"
    ${If} ${FileExists} "${ACTIVE}\*.*"
      ${IfNot} ${FileExists} "${ACTIVE}\Uninstall.exe"
      ${OrIfNot} ${FileExists} "${ACTIVE}\resources\rune-projects.mjs"
      ${OrIf} ${FileExists} "$7\*.*"
        Goto incomplete
      ${EndIf}
      Rename "${ACTIVE}" "$7"
      IfErrors incomplete
    ${EndIf}
    Rename "${FORMER_DIR}" "${ACTIVE}"
    IfErrors incomplete
  ${EndIf}
  ${IfNot} ${FileExists} "${ACTIVE}\resources\app.asar"
    Goto incomplete
  ${EndIf}
  DeleteRegKey HKCU "${ARP}"
  ${If} ${FileExists} "$7\Uninstall.exe"
  ${AndIf} ${FileExists} "$7\resources\rune-projects.mjs"
    ClearErrors
    CreateDirectory "${ROOT}\retired"
    GetTempFileName $6 "${ROOT}\retired"
    IfErrors incomplete
    Delete "$6"
    Rename "$7" "$6"
    IfErrors incomplete
  ${EndIf}
  ClearErrors
  DeleteRegValue HKCU "${RELEASE_LOCATION_KEY}" "AdoptionStage"
  Return
incomplete:
  SetErrors
FunctionEnd

Function RetirePreviousRelease
  ReadRegStr $0 HKCU "${FORMER_LOCATION_KEY}" "InstallLocation"
  ${If} $0 != "${ACTIVE}"
  ${AndIf} $0 != "${FORMER_DIR}"
    Return
  ${EndIf}
  ReadRegStr $1 HKCU "${FORMER_ARP}" "Publisher"
  Call ReadFormerProduct
  ${If} $1 != "T3 Tools"
  ${OrIf} $formerProduct == ""
    Return
  ${EndIf}
  StrCpy $8 "${FORMER_DIR}\Uninstall $formerProduct.exe"
  ${IfNot} ${FileExists} "$8"
    Return
  ${EndIf}
  ; The original uninstaller reads this location even when invoked with _?=.
  ClearErrors
  WriteRegStr HKCU "${FORMER_LOCATION_KEY}" "InstallLocation" "${FORMER_DIR}"
  WriteRegStr HKCU "${FORMER_ARP}" "UninstallString" '"$8" /currentuser --updated --keep-shortcuts'
  WriteRegStr HKCU "${FORMER_ARP}" "QuietUninstallString" '"$8" /currentuser /S --updated --keep-shortcuts'
  WriteRegStr HKCU "${FORMER_ARP}" "DisplayName" "Finish removing the previous $formerProduct installation"
  IfErrors done
  System::Call 'advapi32::GetTokenInformation(p -4, i 20, *i .r3, i 4, *i .r4)i.r0'
  ${If} $0 != 0
  ${AndIf} $3 == 0
    InitPluginsDir
    CopyFiles /SILENT "$8" "$PLUGINSDIR\previous-desktop.exe"
    IfErrors done
    ExecWait '"$PLUGINSDIR\previous-desktop.exe" /currentuser /S --updated --keep-shortcuts _?=${FORMER_DIR}' $3
    IfErrors done
    ${If} $3 == 0
      Delete "$8"
      RMDir "${FORMER_DIR}"
    ${EndIf}
  ${Else}
    ExecInExplorer::Exec "$8" "/cmdargs" "/currentuser /S --updated --keep-shortcuts"
    Pop $0
  ${EndIf}
done:
FunctionEnd
!macroend
