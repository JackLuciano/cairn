<#
.SYNOPSIS
  Sets the Cairn version in Cargo.toml and refreshes Cargo.lock.

.EXAMPLE
  ./scripts/set-version.ps1 0.2.0
#>
param(
  [Parameter(Mandatory = $true)][string]$Version
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$') { throw "Not a semantic version: $Version" }
$text = Get-Content Cargo.toml -Raw
$updated = [regex]::Replace($text, '(?m)^version = "[^"]+"', "version = `"$Version`"", 1)
Set-Content Cargo.toml $updated -NoNewline -Encoding utf8NoBOM
cargo update -w --offline *> $null
Write-Host "Version set to $Version. Add a '## [$Version] - $(Get-Date -Format yyyy-MM-dd)' section to CHANGELOG.md."
