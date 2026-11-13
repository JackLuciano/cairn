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

## Checks on pull requests

Nothing runs on its own when a pull request is opened or updated. After reviewing a pull
request, add a label to start its checks:

| Label | Runs |
| --- | --- |
| `run-ci` | Clippy and tests on Windows |
| `run-builds` | The macOS and Linux builds |

The results appear in the pull request's checks. To run them again after new commits, remove
the label and add it again. On `main`, start them from **Actions → Run workflow**.

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

## Signed releases (GitHub Actions + SignPath)

Signed Windows releases are built on GitHub, because SignPath only signs files it can trace back
to a GitHub Actions build of this repository. After the version bump and changelog are merged,
run **Actions → Release (Windows) → Run workflow** on `main`. The workflow builds `cairn.exe`,
sends it to SignPath, packs the installer with Velopack, sends the installer to SignPath, tags
the commit, publishes the release and starts the macOS and Linux builds. Each signing request
waits until an approver approves it on signpath.io.

Without the SignPath settings below, the same workflow publishes an unsigned release.

One-time SignPath setup (after SignPath Foundation accepts the project):

1. In SignPath, create two artifact configurations with the slugs `app` and `installer`, from
   [`.signpath/artifact-configurations`](.signpath/artifact-configurations).
2. Create a CI user with submitter rights on the release signing policy and copy its API token.
3. In GitHub → Settings → Secrets and variables → Actions, add:
   - the secret `SIGNPATH_API_TOKEN`
   - the variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG` and
     `SIGNPATH_POLICY_SLUG`
4. Install the SignPath GitHub App on the repository, if SignPath asks for it.

The local `./scripts/release.ps1` still works, but its releases are unsigned.

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
