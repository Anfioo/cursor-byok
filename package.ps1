# Builds the Scoop plugin package for WingHarness: cursor-server, the web
# console, and the plugin metadata in packaging/, zipped so a bucket manifest
# can install it.
#
#   pwsh ./package.ps1                                   # -> dist/
#   pwsh ./package.ps1 -OutDir M:\AnfiooScoopBuckets\dist # -> the bucket
#
# The zip is flat: plugin.json, start.cmd, stop.cmd, init-env.ps1,
# cursor-server.exe and dist/ at the root.

param(
    [string] $OutDir = (Join-Path $PSScriptRoot 'dist')
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

# The server crate owns the version; the packaged plugin.json is stamped with
# it, and the bucket manifest repeats it.
$cargo = Get-Content -Raw 'server\Cargo.toml'
$version = [regex]::Match($cargo, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version) { throw 'could not read the version from server\Cargo.toml' }

Write-Host "==> [1/4] building cursor-server $version"
cargo build --release --package cursor-server --bin cursor-server
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

Write-Host '==> [2/4] building the web console'
npm --prefix apps/desktop run build
if ($LASTEXITCODE -ne 0) { throw 'the web console build failed' }

Write-Host '==> [3/4] staging the plugin'
$stage = Join-Path $PSScriptRoot 'output'
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage | Out-Null

Copy-Item 'packaging\*' $stage -Recurse
Copy-Item 'target\release\cursor-server.exe' $stage
Copy-Item -Recurse 'apps\desktop\dist' (Join-Path $stage 'dist')

$manifest = Join-Path $stage 'plugin.json'
$json = Get-Content -Raw $manifest
$json = [regex]::Replace($json, '"version"\s*:\s*"[^"]*"', '"version": "' + $version + '"')
Set-Content -Path $manifest -Value $json -Encoding utf8NoBOM

Write-Host '==> [4/4] zipping'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$zip = Join-Path $OutDir "cursor-proxy-$version.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $stage,
    $zip,
    [System.IO.Compression.CompressionLevel]::Optimal,
    $false
)

$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
$url = 'file:///' + ($zip -replace '\\', '/')

Write-Host ''
Write-Host "zip:  $zip"
Write-Host "hash: $hash"
Write-Host ''
Write-Host 'bucket manifest:'
Write-Host ('  "version": "' + $version + '"')
Write-Host ('  "url": "' + $url + '"')
Write-Host ('  "hash": "' + $hash + '"')
