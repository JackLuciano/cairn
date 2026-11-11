<#
.SYNOPSIS
  Builds the Windows installer and update packages locally with Velopack and publishes them as a
  GitHub Release of JackLuciano/cairn, so a Windows release costs no GitHub Actions minutes.

.EXAMPLE
  ./scripts/release.ps1           # release the version in Cargo.toml
  ./scripts/release.ps1 -DryRun   # build and pack, publish nothing

.NOTES
  Needs: gh (logged in), git, cargo, the .NET SDK (for the vpk tool), a clean checkout of main
  that matches origin/main, and a "## [version]" section in CHANGELOG.md.
#>
param(
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$repo = 'JackLuciano/cairn'
$repoUrl = "https://github.com/$repo"
$appId = 'dev.cairn.app'

function Invoke-Checked([scriptblock]$cmd, [string]$what) {
  & $cmd
  if ($LASTEXITCODE) { throw "$what failed (exit $LASTEXITCODE)" }
}

foreach ($tool in 'gh', 'git', 'cargo', 'dotnet') {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is not installed." }
}
if (-not (Get-Command vpk -ErrorAction SilentlyContinue)) {
  Invoke-Checked { dotnet tool install -g vpk } 'Installing vpk'
  $env:PATH = "$env:PATH;$HOME\.dotnet\tools"
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw 'No version found in Cargo.toml.' }
$tag = "v$version"

$changelog = Get-Content CHANGELOG.md -Raw
$match = [regex]::Match($changelog, "(?ms)^## \[$([regex]::Escape($version))\][^\n]*\n(.*?)(?=^## \[|\z)")
if (-not $match.Success -or -not $match.Groups[1].Value.Trim()) { throw "CHANGELOG.md has no section for [$version]." }
$notes = $match.Groups[1].Value.Trim()

if (-not $DryRun) {
  if ((git branch --show-current) -ne 'main') { throw 'Releases are cut from main. Switch to main first.' }
  if (git status --porcelain) { throw 'Working tree is not clean.' }
  Invoke-Checked { git fetch origin main --tags --quiet } 'git fetch'
  if ((git rev-parse HEAD) -ne (git rev-parse origin/main)) { throw 'Local main differs from origin/main. Pull first.' }
  gh release view $tag --repo $repo *> $null
  if (-not $LASTEXITCODE) { throw "Release $tag already exists. Bump the version with ./scripts/set-version.ps1." }
  $existing = git tag --list $tag
  if ($existing -and (git rev-list -n 1 $tag) -ne (git rev-parse HEAD)) { throw "Tag $tag already exists on a different commit." }
}
Write-Host "Releasing Cairn $tag (Windows, local build)" -ForegroundColor Cyan

Invoke-Checked { cargo build -p cairn --release } 'cargo build'

$out = New-Item -ItemType Directory -Force 'releases'
$stage = Join-Path $out 'stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item 'target/release/cairn.exe' $stage
$notesPath = Join-Path $out 'notes.md'
$notes | Set-Content $notesPath -Encoding utf8NoBOM

vpk download github --repoUrl $repoUrl -o $out *> $null

$packArgs = @(
  'pack', '-u', $appId, '-v', $version, '-p', $stage, '-e', 'cairn.exe',
  '--packTitle', 'Cairn', '--packAuthors', 'The Cairn contributors',
  '--releaseNotes', $notesPath, '-o', $out
)
if (Test-Path 'crates/app/assets/cairn.ico') { $packArgs += @('-i', 'crates/app/assets/cairn.ico') }
Invoke-Checked { vpk @packArgs } 'vpk pack'
Write-Host "Packed into $out" -ForegroundColor Green

if ($DryRun) { Write-Host 'Dry run: nothing published.' -ForegroundColor Yellow; exit 0 }

if (-not (git tag --list $tag)) { Invoke-Checked { git tag -a $tag -m "Cairn $tag" } 'git tag' }
Invoke-Checked { git push origin $tag } 'git push tag'

$token = (gh auth token).Trim()
Invoke-Checked {
  vpk upload github --repoUrl $repoUrl --token $token --publish --releaseName "Cairn $tag" --tag $tag -o $out
} 'vpk upload'
Invoke-Checked { gh release edit $tag --repo $repo --notes-file $notesPath --latest } 'Setting release notes'
Write-Host "Published $repoUrl/releases/tag/$tag" -ForegroundColor Green
