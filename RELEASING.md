# Releasing Cairn

Cairn is released from this public repository. The installer and the update packages are built
**on your own Windows machine** with [Velopack](https://velopack.io) and uploaded to GitHub
Releases, so a Windows release uses no GitHub Actions minutes. Installed copies of Cairn check
these releases, download updates in the background, and install them when Cairn closes (or right
away with the "Restart to update" button).

**Nothing is published until you run the release script yourself.**

## One-time setup

1. Install the [.NET SDK](https://dotnet.microsoft.com/download) (8 or newer). The release
   script installs the `vpk` tool with it on first run.
2. Install the [GitHub CLI](https://cli.github.com/) and log in: `gh auth login`.

## Cutting a release

```powershell
./scripts/set-version.ps1 0.2.0
```

1. Add a `## [0.2.0] - YYYY-MM-DD` section at the top of `CHANGELOG.md`. It becomes the GitHub
   release notes and the "What's new" window users see after updating.
2. Merge the version bump and changelog into `main` through a pull request.
3. On the updated `main`, run:

```powershell
git pull origin main
```

```powershell
./scripts/release.ps1
```

The script checks that the changelog section exists and that your checkout matches
`origin/main`, builds a release binary, packs `CairnSetup.exe` plus full and delta update
packages, tags the commit, and publishes the release.

Try it without publishing:

```powershell
./scripts/release.ps1 -DryRun
```

## What users get

| File | Purpose |
| --- | --- |
| `dev.cairn.app-win-Setup.exe` | Installer. Per-user, no admin rights needed. |
| `*-full.nupkg`, `*-delta.nupkg`, `releases.win.json` | Used by the built-in updater. Not meant for manual download. |

The installers are not code-signed yet, so Windows SmartScreen may warn on first install.

## Rules

- The app ID `dev.cairn.app` and the binary name `cairn.exe` must never change after the first
  release: changing them orphans users' installs and breaks updates.
- Release notes come only from `CHANGELOG.md`.
