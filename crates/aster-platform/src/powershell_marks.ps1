$global:__AsterPrompt = $function:prompt
$global:__AsterRunning = $false
$global:__AsterOsc = [string][char]27 + ']'
$global:__AsterSt = [string][char]27 + '\'
if (-not $function:PSConsoleHostReadLine) {
    try { Import-Module PSReadLine -ErrorAction Stop } catch {}
}
if ($function:PSConsoleHostReadLine) {
    $global:__AsterReadLine = $function:PSConsoleHostReadLine
    function global:PSConsoleHostReadLine {
        $line = & $global:__AsterReadLine
        if (-not [string]::IsNullOrWhiteSpace($line)) {
            $global:__AsterRunning = $true
            $global:__AsterError = $global:Error[0]
            [Console]::Write("${global:__AsterOsc}133;C${global:__AsterSt}")
        }
        return $line
    }
}
function global:prompt {
    $ok = $?
    if ($global:__AsterRunning) {
        $global:__AsterRunning = $false
        $exitCode = 0
        if (-not $ok) {
            $errorRecord = $global:Error[0]
            $native = [object]::ReferenceEquals($errorRecord, $global:__AsterError) -or
                ($null -ne $errorRecord -and $errorRecord.Exception.GetType().Name -eq 'NativeCommandExitException')
            $exitCode = if ($native -and $global:LASTEXITCODE) { $global:LASTEXITCODE } else { 1 }
        }
        [Console]::Write("${global:__AsterOsc}133;D;$exitCode${global:__AsterSt}")
    }
    $l = $executionContext.SessionState.Path.CurrentLocation
    if ($l.Provider.Name -eq 'FileSystem') { [Console]::Write("$([char]27)]9;9;`"$($l.ProviderPath)`"$([char]27)\") }
    if ($global:__AsterReadLine) {
        [Console]::Write("${global:__AsterOsc}133;A${global:__AsterSt}")
        $text = & $global:__AsterPrompt
        return "$text${global:__AsterOsc}133;B${global:__AsterSt}"
    }
    & $global:__AsterPrompt
}
