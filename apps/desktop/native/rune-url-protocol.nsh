!macro ReadProtocolValue result key name
  ClearErrors
  ReadRegStr ${result} HKCU "Software\Classes\${key}" "${name}"
  ${If} ${Errors}
    ClearErrors
    ReadRegStr ${result} HKCR "${key}" "${name}"
  ${EndIf}
!macroend

!macro RegisterRuneProtocol scheme
  !insertmacro ReadProtocolValue $2 "${scheme}\shell\open\command" "DelegateExecute"
  ${If} ${Errors}
    !insertmacro ReadProtocolValue $0 "${scheme}\shell\open\command" ""
    !insertmacro ReadProtocolValue $1 "${scheme}" "URL Protocol"
    ${If} ${Errors}
    ${AndIf} $0 == ""
    ${OrIf} $0 == '"${ACTIVE}\${PRODUCT}.exe" "%1"'
    ${OrIf} $0 == '"${ACTIVE}\$oldDisplayName.exe" "%1"'
      ClearErrors
      WriteRegStr HKCU "Software\Classes\${scheme}\shell\open\command" "" '"${ACTIVE}\${PRODUCT}.exe" "%1"'
      ${IfNot} ${Errors}
        WriteRegStr HKCU "Software\Classes\${scheme}" "" "URL:T3 Code"
        WriteRegStr HKCU "Software\Classes\${scheme}" "URL Protocol" ""
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro UnregisterRuneProtocol scheme
  !insertmacro ReadProtocolValue $1 "${scheme}\shell\open\command" "DelegateExecute"
  ${If} ${Errors}
    ReadRegStr $0 HKCU "Software\Classes\${scheme}\shell\open\command" ""
    ${If} $0 == '"${ACTIVE}\${PRODUCT}.exe" "%1"'
      DeleteRegValue HKCU "Software\Classes\${scheme}\shell\open\command" ""
      DeleteRegKey /ifempty HKCU "Software\Classes\${scheme}\shell\open\command"
      DeleteRegKey /ifempty HKCU "Software\Classes\${scheme}\shell\open"
      DeleteRegKey /ifempty HKCU "Software\Classes\${scheme}\shell"
      ClearErrors
      EnumRegKey $1 HKCU "Software\Classes\${scheme}" 0
      ${If} ${Errors}
        DeleteRegValue HKCU "Software\Classes\${scheme}" "URL Protocol"
        ReadRegStr $0 HKCU "Software\Classes\${scheme}" ""
        ${If} $0 == "URL:T3 Code"
          DeleteRegValue HKCU "Software\Classes\${scheme}" ""
        ${EndIf}
        DeleteRegKey /ifempty HKCU "Software\Classes\${scheme}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
