param(
    [Parameter(Mandatory, ParameterSetName='Update')][int]$OwnerId,
    [Parameter(Mandatory, ParameterSetName='Verify')][string]$Relaunched
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Windows.Forms
function Find-Control($property, $value, [switch]$TextValue) {
    $condition = [Windows.Automation.PropertyCondition]::new($property, $value)
    if ($TextValue) {
        $valueCondition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.ValuePattern]::ValueProperty, $value)
        $condition = [Windows.Automation.OrCondition]::new($condition, $valueCondition)
    }
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while (!$owner.HasExited -and $timer.Elapsed.TotalSeconds -lt 75) {
        $owner.Refresh()
        if ($owner.MainWindowHandle -ne [IntPtr]::Zero) {
            $window = [Windows.Automation.AutomationElement]::FromHandle($owner.MainWindowHandle)
            $control = $window.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
            if ($control -and $control.Current.IsEnabled) { return $control }
        }
        Start-Sleep -Milliseconds 200
    }
    throw "Could not find $value in the Rune window"
}
$nameProperty = [Windows.Automation.AutomationElement]::NameProperty
$id = [Windows.Automation.AutomationElement]::AutomationIdProperty
if ($Relaunched) {
    $name = [IO.Path]::GetFileNameWithoutExtension($Relaunched)
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        foreach ($app in [Diagnostics.Process]::GetProcessesByName($name)) {
            if ($app.MainModule.FileName -ieq $Relaunched -and $app.MainWindowHandle -ne [IntPtr]::Zero) {
                $owner = $app
                break
            }
            $app.Dispose()
        }
        if ($owner) { break }
        Start-Sleep -Milliseconds 250
    } while ($timer.Elapsed.TotalSeconds -lt 90)
    if (!$owner) { throw "The updated installation did not reopen its window: $Relaunched" }
    $thread = Find-Control $id 'launch-check-thread'
    $thread.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
    $null = Find-Control $nameProperty 'The release opens and shows this thread.' -TextValue
    return
}
$owner = [Diagnostics.Process]::GetProcessById($OwnerId)
$null = Find-Control $id 'launch-check-thread'
$shell = New-Object -ComObject WScript.Shell
if (!$shell.AppActivate($OwnerId)) { throw 'Could not activate the installed Rune window' }
[Windows.Forms.SendKeys]::SendWait('^k')
$null = Find-Control $id 'palette-query'
[Windows.Forms.SendKeys]::SendWait('Check for Updates')
$null = Find-Control $nameProperty ('Check for Updates' + [char]0x2026)
[Windows.Forms.SendKeys]::SendWait('{ENTER}')
foreach ($action in @('Download', 'Restart')) {
    $control = Find-Control $nameProperty $action
    $control.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}
