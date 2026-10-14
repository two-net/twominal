# Twominal's shell integration for PowerShell (FISH-04, FISH-05, SW-02).
#
# Twominal starts PowerShell with -NoExit -EncodedCommand and this script, which PowerShell
# runs after the user's profiles. It needs no file, and no file of the user's is created,
# changed or deleted. It wraps the prompt function, so that before each prompt it reports to
# Twominal, and then the user's own prompt runs as before.
#
# It works in Windows PowerShell 5.1 and in PowerShell 7 or later.
#
# A report is ESC ] 2222 ; nonce ; key ; value BEL, its value written with every byte outside
# ! to ~, and every % and ;, as %XX. DESIGN.md §7 lists the reports.

# UTF-8 for console input and output (SW-02). Twominal sends this part even with shell
# integration off.
[Console]::InputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$global:OutputEncoding = [Console]::OutputEncoding

# Kept, but not in the environment: no program the user runs can read it.
$global:__twominal_nonce = $env:twominal_nonce
Remove-Item Env:\twominal_nonce -ErrorAction SilentlyContinue

if ($global:__twominal_nonce) {
    # What each report last said, so that only a change is said again.
    $global:__twominal_said = @{}
    $global:__twominal_prompt = $function:prompt

    # Returns $value, encoded.
    function global:__twominal_encode([string] $value) {
        if ($value -cnotmatch '[^!-~]|[%;]') {
            return $value
        }
        $encoded = [System.Text.StringBuilder]::new()
        foreach ($byte in [System.Text.Encoding]::UTF8.GetBytes($value)) {
            if ($byte -ge 0x21 -and $byte -le 0x7E -and $byte -ne 0x25 -and $byte -ne 0x3B) {
                [void] $encoded.Append([char] $byte)
            } else {
                [void] $encoded.Append(('%{0:X2}' -f $byte))
            }
        }
        $encoded.ToString()
    }

    # Returns the names, each encoded, separated by spaces.
    function global:__twominal_names($names) {
        $names = @(@($names) -ne $null)
        if (-not ($names -cmatch '[^!-~]|[%;]')) {
            return $names -join ' '
        }
        @($names | ForEach-Object { __twominal_encode $_ }) -join ' '
    }

    # Writes the report $key, with the encoded $value when there is one, unless it says what
    # it said last time.
    function global:__twominal_say([string] $key, [string] $value) {
        $escape = [char] 27
        $bell = [char] 7
        if ($PSBoundParameters.ContainsKey('value')) {
            if ($global:__twominal_said.ContainsKey($key) -and $global:__twominal_said[$key] -ceq $value) {
                return
            }
            $global:__twominal_said[$key] = $value
            [Console]::Write("$escape]2222;$global:__twominal_nonce;$key;$value$bell")
        } else {
            [Console]::Write("$escape]2222;$global:__twominal_nonce;$key$bell")
        }
    }

    function global:__twominal_cmdlets {
        Set-StrictMode -Off
        __twominal_say cmdlets (__twominal_names (Get-Command -CommandType Cmdlet).Name)
    }

    # Before each prompt: what changed, then that the shell is ready for a command. The user's
    # prompt runs first, with $? as the last command left it, and its text is returned as it
    # was.
    function global:prompt {
        $succeeded = $global:?
        Set-StrictMode -Off
        if (-not $succeeded) {
            Write-Error 'failed' -ErrorAction Ignore
        }
        $text = $global:__twominal_prompt.Invoke()

        if ($PWD.Provider.Name -eq 'FileSystem') {
            __twominal_say cwd (__twominal_encode $PWD.ProviderPath)
        }
        __twominal_say path (__twominal_encode $env:PATH)
        __twominal_say aliases (__twominal_names (Get-ChildItem Alias:).Name)
        __twominal_say functions (__twominal_names ((Get-ChildItem Function:).Name -notlike '__twominal_*'))
        if (-not $global:__twominal_said.ContainsKey('cmdlets') -and -not (Get-Module PSReadLine)) {
            __twominal_cmdlets
        }
        __twominal_say ready
        $text
    }

    # Listing the cmdlets takes a tenth of a second or more, so it waits until PowerShell is
    # idle after the first prompt. Only PSReadLine says when that is. Without it, they are
    # listed at the first prompt.
    if (Get-Module PSReadLine) {
        $null = Register-EngineEvent -SourceIdentifier PowerShell.OnIdle -MaxTriggerCount 1 -Action {
            __twominal_cmdlets
        }
    }
}
