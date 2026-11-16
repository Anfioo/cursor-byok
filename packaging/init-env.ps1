# Called by start.cmd when .env is missing, or when it exists without a
# CURSOR_EXTERNAL_API_KEY. It makes sure that one secret is present and leaves
# everything else in the file alone.
#
# The file lives in the plugin's persist folder (<scoop>\persist\cursor-proxy),
# not next to the binary: Scoop replaces the install folder on every update.
# The value is plain text on purpose: the app's config window edits this same
# file, the file is the source of truth, and cursor-server reads it as-is.
# Generating it is only about never shipping a guessable key -- cursor-server
# seeds the key it finds here into its own database on every start.
#
# ASCII only, deliberately: Windows PowerShell 5.1 reads a BOM-less script as
# ANSI, so non-ASCII text in here would come out as mojibake.

$ErrorActionPreference = 'Stop'

$folder = if ($env:PLUGIN_PERSIST) {
    $env:PLUGIN_PERSIST
} else {
    Split-Path -Parent $MyInvocation.MyCommand.Path
}
if (-not (Test-Path -LiteralPath $folder)) {
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
}
$target = Join-Path $folder '.env'

# Shipped as-is on the very first start; afterwards the app's config window is
# what keeps the file up to date. "%KEY%=" is filled in below.
$template = @'
# cursor-byok settings. Written by init-env.ps1 on the first start, then edited
# by the app's config window -- this file is the source of truth, the server
# reads it at start.
#
# One setting per line. Never put a comment after a value on the same line:
# everything following "=" is the value.

# Address the server listens on, port included. 0.0.0.0 opens it to the network.
CURSOR_LISTEN_ADDR=127.0.0.1:24680

# Web console static files, relative to the plugin folder.
CURSOR_CONSOLE_DIR=dist

# Key external callers must send. Filled in below on the first start.
CURSOR_EXTERNAL_API_KEY=

# SQLite file holding channels, models and call history, relative to this folder.
CURSOR_DATABASE_URL=sqlite://./data/cursor-byok.db

# error | warn | info | debug | trace
RUST_LOG=info

# CURSOR_PROVIDER_TIMEOUT_SECONDS is left out on purpose: the server has its own
# default. Add it here (or set it in the plugin settings window) to override it.
'@

# `Length` characters from an alphabet without quotes, spaces or "#", so a value
# never needs escaping no matter who parses the file.
function New-Secret([int] $Length) {
    $alphabet = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789'
    $bytes = New-Object byte[] $Length
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $chars = foreach ($byte in $bytes) { $alphabet[$byte % $alphabet.Length] }
    return (-join $chars)
}

# The raw value of `Name`, or an empty string when the line is absent, blank or
# commented out.
function Get-Value([string] $Text, [string] $Name) {
    $match = [regex]::Match($Text, '(?m)^\s*' + [regex]::Escape($Name) + '\s*=(.*)$')
    if ($match.Success) { return $match.Groups[1].Value.Trim() }
    return ''
}

# `Name` set to `Value`: the existing line is rewritten, a missing one appended.
function Set-Value([string] $Text, [string] $Name, [string] $Value) {
    $line = $Name + '=' + $Value
    $pattern = '(?m)^\s*' + [regex]::Escape($Name) + '\s*=.*$'
    if ([regex]::IsMatch($Text, $pattern)) {
        return [regex]::Replace($Text, $pattern, $line)
    }
    return $Text.TrimEnd([char[]]"`r`n") + "`r`n" + $line + "`r`n"
}

$fresh = -not (Test-Path -LiteralPath $target)
$text = if ($fresh) { $template } else { [System.IO.File]::ReadAllText($target) }
$changed = $fresh

if (Get-Value $text 'CURSOR_EXTERNAL_API_KEY') {
    Write-Host '[cursor-proxy] CURSOR_EXTERNAL_API_KEY is already set, kept as it is.'
} else {
    $text = Set-Value $text 'CURSOR_EXTERNAL_API_KEY' (New-Secret 48)
    $changed = $true
}

if ($changed) {
    # No BOM, and the app's reader trims CR: both endings work everywhere.
    [System.IO.File]::WriteAllText($target, $text, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host ('[cursor-proxy] wrote ' + $target)
    Write-Host ('[cursor-proxy] CURSOR_EXTERNAL_API_KEY = ' + (Get-Value $text 'CURSOR_EXTERNAL_API_KEY'))
    Write-Host '[cursor-proxy] It is editable in the plugin settings window.'
} else {
    Write-Host '[cursor-proxy] .env already has the key, left untouched.'
}

exit 0
