<div align="center">

<img src="crates/app/assets/cairn.png" width="96" alt="Cairn logo">

# Cairn

**A fast, friendly git client for people who outgrew the basics.**

Native Rust, GPU-rendered with [GPUI](https://gpui.rs). Every risky operation is one click from undone.

[![CI](https://github.com/JackLuciano/cairn/actions/workflows/ci.yml/badge.svg)](https://github.com/JackLuciano/cairn/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078d4)](#install)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org)
[![Languages](https://img.shields.io/badge/UI-English%20%7C%20Magyar-8b5cf6)](#languages)

[Features](#features) · [Install](#install) · [Shortcuts](#keyboard-shortcuts) · [AI](#ask-ai) · [Build from source](#build-from-source)

<br>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/light/history.png">
  <img src="docs/screenshots/dark/history.png" alt="Cairn's history view with the commit graph" width="100%">
</picture>

</div>

## Why Cairn

Most git clients are either a thin layer over the basics or a wall of buttons. Cairn keeps the
everyday flow calm, and makes the powerful things safe:

- **Nothing is scary.** Cairn saves an undo point before every operation. A bad rebase, reset or
  amend is one click away from being undone.
- **The advanced stuff is built in.** Interactive rebase, absorb, split, stacked pull requests,
  a three-way conflict editor and bisect work without memorizing a single flag.
- **Many repositories, one place.** Group repositories into categories and commit, pull or push
  several of them together.
- **Fast.** A native Rust app drawn on the GPU, no Electron, no web view.

## Features

### Everyday git, done right

<table>
<tr>
<td width="50%"><img src="docs/screenshots/dark/changes.png" alt="Changes view with line staging"></td>
<td width="50%"><img src="docs/screenshots/dark/branches.png" alt="Branches, tags and worktrees"></td>
</tr>
<tr>
<td><b>Line-by-line staging.</b> Click lines in a diff to stage, unstage or discard exactly what
you mean. Word-level highlights show what really changed, image diffs come in side-by-side,
swipe and onion-skin modes.</td>
<td><b>Branches, tags, stashes and worktrees</b> in one tidy page. Fetch, pull, push, safe force
push, cherry-pick, revert, merge and reset are a right-click away.</td>
</tr>
</table>

<img src="docs/screenshots/dark/history-multi.png" alt="Several commits selected in History" width="100%">

**Work on many commits at once.** Ctrl+click or Shift+click commits in History, then copy all
their hashes, cherry-pick them or revert them in one step. Cairn applies them in the order they
were made, whatever order you clicked them in.

**AI commit messages.** "Write message from diff" drafts a summary and description from your
staged changes. Tell it how you like your messages in Settings → Commits, or share rules with
your team in `.cairn/commit-message.md`.

**History that stays readable.** History opens on the branch you are on, so twenty open feature
branches don't turn the graph into a wall of lines. Switch to "All branches" when you want the
whole picture; branches that grow from the same commit share a lane instead of fanning out.

**Drag and drop.** Drag a branch label onto another branch to merge or rebase it, or drag commits
onto a branch to cherry-pick them. Cairn always asks first and tells you if it needs to switch
branches.

### Find anything, undo anything

- **File history.** Every commit that changed a file, followed through renames, with the change
  each one made.
- **Compare.** Pick any two branches, tags or commits and see what is only on each side and how
  the files differ.
- **Lost & found.** Deleted a branch, reset too far or dropped a stash? Cairn lists the commits
  nothing points at any more and brings them back as a branch in one click.
- **Stashes, properly.** Stash only some files, or only the blocks and lines you staged. Look
  inside a stash before applying it and restore single files from it.

### Pull requests, submodules and large files

<img src="docs/screenshots/dark/pull-requests.png" alt="Pull request ready to merge" width="100%">

- **Review pull requests in Cairn.** A ring shows what each pull request still needs (reviews,
  checks, conflicts) and what to do next. Read the conversation, commits and checks, tick off
  files as you review them, comment on a line, approve or request changes, check it out locally
  and merge it. Uses the GitHub CLI.
- **Submodules.** See which are missing or on a different commit, download and update them, add
  new ones and open them as their own tab.
- **Git LFS.** Track file types, download LFS files, and lock and unlock files your team edits.

### Your tools

Open files and repositories in your editor, send a diff to your external diff tool or a conflict
to your merge tool, ignore a file or a whole file type from the Changes list, and save commits as
a patch or apply someone else's. Set your programs in Settings → Tools.

### Rewrite history without fear

<table>
<tr>
<td width="50%"><img src="docs/screenshots/dark/rebase.png" alt="Interactive rebase"></td>
<td width="50%"><img src="docs/screenshots/dark/absorb.png" alt="Absorb and split"></td>
</tr>
<tr>
<td><b>Interactive rebase.</b> Reorder, reword, squash, fix up or drop commits. <code>fixup!</code>
commits land in the right place automatically, and stacked branches move along.</td>
<td><b>Absorb and split.</b> Staged fixes jump into the commit that last touched those lines.
Split a commit that grew too big into several clean ones.</td>
</tr>
</table>

### Conflicts and stacks

<table>
<tr>
<td width="50%"><img src="docs/screenshots/dark/conflicts.png" alt="Three-way conflict editor"></td>
<td width="50%"><img src="docs/screenshots/dark/stack.png" alt="Stacked branches"></td>
</tr>
<tr>
<td><b>Three-way conflict editor.</b> Ours, base and theirs side by side, one-click choices per
block and a live preview of the result. Cairn opens it by itself when a pull or rebase stops.</td>
<td><b>Stacked branches and pull requests.</b> Cairn remembers which branch sits on which, so
amending a lower branch and restacking just works. Submit the whole stack to GitHub at once.</td>
</tr>
</table>

### Many repositories at once

<img src="docs/screenshots/dark/home.png" alt="Category workspace" width="100%">

Categories work like folders for your repositories. Open one to fetch, pull or switch every
repository together. When several repositories are on a branch with the same name, Cairn treats
them as one **linked change** you commit and push in a single step.

### Investigate

<table>
<tr>
<td width="50%"><img src="docs/screenshots/dark/line-history.png" alt="Line history"></td>
<td width="50%"><img src="docs/screenshots/dark/bisect.png" alt="Visual bisect"></td>
</tr>
<tr>
<td><b>Line history.</b> Blame with age coloring, and every earlier version of any line.</td>
<td><b>Visual bisect.</b> Mark commits good or bad, or let a test command decide, and Cairn
narrows down the commit that broke things.</td>
</tr>
</table>

### Ask AI

<img src="docs/screenshots/dark/ask-ai.png" alt="Ask AI" width="100%">

Explain a commit, spot what could break, summarize your day, review before you push, write a
commit message or a pull request description, or get a suggestion for a tricky conflict.

- Connect with an **Anthropic API key**, your **Claude Code** login on this computer, or a
  **custom endpoint**.
- **Off until you turn it on.** Paths you exclude (such as `.env` files) are never sent, and
  anything that looks like a secret is redacted first.
- A monthly spending limit keeps costs predictable.

### Safety net

- **Undo history** for every operation, with uncommitted work stashed safely before going back.
- **Checks before commit and push** from a `.cairn/checks.toml` file in your repository.
- **Secret scan** that stops API keys, tokens and private keys from leaving your computer.
- **Protected main**: Cairn asks before rewriting history on `main` or `master`.

### Made to fit

<table>
<tr>
<td width="50%"><img src="docs/screenshots/dark/palette.png" alt="Command palette"></td>
<td width="50%"><img src="docs/screenshots/dark/settings.png" alt="Settings"></td>
</tr>
<tr>
<td><b>Command palette</b> for every page, git command, branch and repository.</td>
<td><b>About 90 settings</b> with search and live preview: themes, accent colors, graph style,
diff options, identity profiles, signing, AI commit message rules and more.</td>
</tr>
</table>

Cairn updates itself in the background (or when you press "Check now" in Settings → Updates) and
shows what changed after each update:

<img src="docs/screenshots/dark/whats-new.png" alt="What's new popup" width="100%">

## Install

Download `dev.cairn.app-win-Setup.exe` from the
[latest release](https://github.com/JackLuciano/cairn/releases/latest) and run it. It installs for
your user only, no admin rights needed. Updates arrive automatically.

The installer is not code-signed yet, so Windows SmartScreen may ask you to confirm the first time
("More info", then "Run anyway").

**Requirements**

- Windows 10 or 11. macOS and Linux builds are compiled by the "Build (macOS and Linux)" workflow
  but are not packaged or tested yet.
- [Git](https://git-scm.com/download/win) 2.40 or newer on your `PATH`
- Optional: the [GitHub CLI](https://cli.github.com) for pull requests (run `gh auth login` once)

## Keyboard shortcuts

| Action | Keys |
| --- | --- |
| Command palette | <kbd>Ctrl</kbd> <kbd>K</kbd> or <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>P</kbd> |
| Settings | <kbd>Ctrl</kbd> <kbd>,</kbd> |
| History, Changes, Branches | <kbd>Ctrl</kbd> <kbd>1</kbd> · <kbd>2</kbd> · <kbd>3</kbd> |
| Commit staged changes | <kbd>Ctrl</kbd> <kbd>Enter</kbd> |
| Fetch · Pull · Push | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>F</kbd> · <kbd>L</kbd> · <kbd>K</kbd> |
| Stash | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>S</kbd> |
| New branch | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>B</kbd> |
| Ask AI | <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>A</kbd> |
| Undo history | <kbd>Ctrl</kbd> <kbd>Z</kbd> |
| Refresh | <kbd>F5</kbd> |
| Next · previous tab · close tab | <kbd>Ctrl</kbd> <kbd>Tab</kbd> · <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Tab</kbd> · <kbd>Ctrl</kbd> <kbd>W</kbd> |

Optional Vim keys can be turned on in Settings → Keyboard: <kbd>j</kbd> <kbd>k</kbd> move,
<kbd>g</kbd> <kbd>G</kbd> jump to the top and bottom, <kbd>/</kbd> searches, <kbd>s</kbd> stages,
<kbd>f</kbd> fetches and <kbd>p</kbd> pushes.

## Checks before commit and push

Put a `.cairn/checks.toml` file in a repository:

```toml
[[check]]
name = "Lint"
run = "cargo clippy -- -D warnings"
on = ["push"]
blocking = true

[[check]]
name = "Unit tests"
run = "npm test"
on = ["commit", "push"]
blocking = false
```

`on` lists when the check runs: `"commit"`, `"push"` or both (the default is push only). A failing
blocking check stops the commit or push; a non-blocking one only warns.

## Languages

The interface is available in **English** and **Hungarian** (Magyar). Switch in
Settings → General → Language.

## Build from source

You need a recent stable [Rust](https://rustup.rs) toolchain and Git.

```bash
cargo run -p cairn
```

Release build:

```bash
cargo build -p cairn --release
```

Run the tests and lints the same way CI does:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

### Project layout

| Path | What it is |
| --- | --- |
| `crates/core` | The git engine: history, graph layout, status, diffs, line staging, rebase, absorb, conflicts, stacks, checks, undo, settings. No UI, fully tested. |
| `crates/app` | The desktop app: windows, pages and dialogs built on GPUI and GPUI Kit. |
| `crates/app/locales` | English and Hungarian translations. |
| `scripts` | Version bump and the local Windows release script. |

Settings live in `%APPDATA%\Cairn\settings.toml`, categories and open tabs in
`%APPDATA%\Cairn\library.json`. API keys are stored in Windows Credential Manager, never in files.

## Contributing

Issues and pull requests are welcome. Please run clippy and the tests before opening a pull
request, and add an entry under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything users
will notice. Releases are described in [RELEASING.md](RELEASING.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at
your option.
