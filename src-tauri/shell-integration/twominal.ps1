# Twominal shell integration. This file only emits semantic terminal markers.
if ($global:TwominalShellIntegrationActive) {
    Remove-Item Env:TWOMINAL_SHELL_INTEGRATION_NONCE -ErrorAction SilentlyContinue
    Remove-Item Env:TWOMINAL_INTEGRATION_SCRIPT -ErrorAction SilentlyContinue
    return
}
$global:TwominalShellIntegrationActive = $true
$global:TwominalIntegrationNonce = $env:TWOMINAL_SHELL_INTEGRATION_NONCE
Remove-Item Env:TWOMINAL_SHELL_INTEGRATION_NONCE -ErrorAction SilentlyContinue
Remove-Item Env:TWOMINAL_INTEGRATION_SCRIPT -ErrorAction SilentlyContinue

foreach ($profilePath in @(
    $PROFILE.AllUsersAllHosts,
    $PROFILE.AllUsersCurrentHost,
    $PROFILE.CurrentUserAllHosts,
    $PROFILE.CurrentUserCurrentHost
)) {
    if ($profilePath -and (Test-Path -LiteralPath $profilePath -PathType Leaf)) {
        . $profilePath
    }
}

$global:TwominalOriginalPrompt = $function:prompt

function global:prompt {
    $commandStatus = if ($?) { 0 } else { 1 }
    $escape = [char]27
    $bell = [char]7
    $nonce = $global:TwominalIntegrationNonce
    [Console]::Write("${escape}]133;D;${commandStatus};${nonce}${bell}${escape}]133;A;${nonce}${bell}")
    $cwdBytes = [Text.Encoding]::UTF8.GetBytes((Get-Location).Path)
    $cwdHex = -join ($cwdBytes | ForEach-Object { $_.ToString("x2") })
    [Console]::Write("${escape}]133;P;CwdHex=${cwdHex};${nonce}${bell}")

    $promptValue = & $global:TwominalOriginalPrompt
    return "${promptValue}${escape}]133;B;${nonce}${bell}"
}
