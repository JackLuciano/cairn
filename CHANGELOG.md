# Changelog

All notable changes to Cairn are listed here. Each section is shown in the app's "What's new"
window after an update.

## [Unreleased]

## [0.2.2] - 2026-11-12

### Fixed

- Cairn now opens where you left it: on the same monitor, at the same position and size, and
  maximized or fullscreen if it was. If that monitor is no longer connected, it opens centered
  on your main screen.

## [0.2.1] - 2026-11-12

A new diff viewer.

### Added

- Syntax highlighting in diffs, a side-by-side layout and long-line wrapping, each with its own
  switch in Settings → Diffs.
- Each block of changes has a menu in the gutter to stage, unstage, select or discard it.

### Changed

- Diffs keep their scroll position when the file reloads.
- Opening a file from a commit in History widens the side panel and wraps long lines, so nothing
  runs off the screen.

### Fixed

- The History header no longer wraps onto a second line in narrow windows.

## [0.2.0] - 2026-11-12

A fresh look that follows the Cairn design, plus a few long-requested extras.

### Changed

- New toolbar: repository and branch buttons, a big "Search or run a command" box, and framed
  Undo, Ask AI, Fetch, Pull and Push buttons with counts.
- The interface now uses the Inter font, bundled with Cairn.
- Taller page tabs with colored counts, and a sidebar that shows each repository's changes and
  the open repository's branches, worktrees and stashes.
- History has column headers, roomier rows, Graph / First parent / Hide remotes toggles and a
  "Pushed" line in the commit details.
- Changes has three columns: files, diff and a commit panel with Ignore whitespace and
  per-commit signing.
- Interactive rebase shows a preview of the result, colored actions and the undo history with
  Restore buttons right next to it.
- The category workspace shows each repository's state, the files in a linked change and a
  "Ship this change" panel with the push order.
- A redesigned Settings window with section icons, cards, theme previews and accent swatches.
- A modern conflict editor: clickable Ours / Base / Theirs cards, a progress bar per conflict,
  Previous / Next, a result card with its status and a bottom bar for whole-file choices.
- Branches, Absorb & split, Bisect, Line history, Ask AI and repository settings were rebuilt
  with cards, icon tiles and the new framed buttons.
- Rounder, taller buttons and inputs across the whole app, and a cleaner title bar.

### Added

- Select several commits in History with Ctrl+click or Shift+click, then copy their hashes,
  cherry-pick them or revert them in one go. They are applied in the order they were made.

- "Check now" in Settings → Updates.
- Your own instructions for AI commit messages in Settings → Commits, plus a shared
  `.cairn/commit-message.md` per repository. "Write message from diff" is always visible and
  explains how to turn AI on when it is off.
- Skip commit during a rebase, with the current step shown in the banner.
- Bisect results can be opened in History, explained by AI or reverted in one click.
- Line history lets you step through older and newer versions of a line.
- Press ? for a keyboard shortcut map, also in the command palette.
- "Add branch on top" on the Stack page.

### Fixed

- The "What's new" window no longer shows an empty section for unreleased changes.

## [0.1.1] - 2026-11-12

### Fixed

- Repository tabs, Search, the sidebar toggle and Settings in the title bar now respond to clicks
  on Windows.

## [0.1.0] - 2026-11-12

The first public release of Cairn.

### Added

- Categories that work like folders: create, rename, reorder and delete them, add repositories
  (one at a time or a whole folder of them), move repositories between categories.
- Home screen with category cards, recently opened repositories and friendly empty states.
- Repository tabs that are restored when Cairn starts.
- History view with a colored commit graph, branch, remote and tag labels, author avatars and
  commit details with changed files and per-file diffs.
- Changes view with staged and unstaged files, line-by-line and block staging, discarding with
  confirmation, word-level diff highlighting, commit and amend, and commit-and-push.
- Fetch, pull (rebase, merge or fast-forward only), push, safe force push, stash, branch
  switching and creation, tags, cherry-pick, revert, merge and reset from the commit menu.
- Undo history: every operation saves an undo point you can go back to.
- Banner for an unfinished rebase, merge, cherry-pick, revert or bisect with Continue and Abort.
- Interactive rebase page: reorder commits with arrows, pick, reword, squash, fixup, drop or stop
  to edit, with fixup! commits placed automatically and stacked branches moved along.
- Absorb: staged fixes go back into the commit that last touched those lines. Split: break a
  commit into several by assigning its blocks to new commits.
- Three-way conflict editor with ours, base and theirs side by side, one-click choices per block,
  a live result preview and an optional AI suggestion. Cairn opens it by itself when a pull,
  merge or rebase stops on a conflict.
- Stacked branches and pull requests through the GitHub CLI: restack, submit the whole stack,
  review state, checks and review comments.
- Workspace for categories: fetch, pull or switch every repository at once, and commit and push
  same-named branches across repositories as one linked change.
- Checks before commit and push from `.cairn/checks.toml`, plus a secret scan that blocks keys
  and tokens from leaving your computer.
- Ask AI: explain a commit, spot what could break, summarize the day, review before push and
  write pull request descriptions. Connects with an Anthropic API key, the Claude Code login on
  this computer, or a custom endpoint.
- Line history: blame with age coloring, and every earlier version of a line.
- Visual bisect: mark commits good or bad, or let a test command decide.
- Command palette (Ctrl+K) for pages, git commands, branches and repositories, plus keyboard
  shortcuts and optional Vim keys.
- Per-repository identity, signing and remotes, with identity profiles that apply by folder.
- Worktrees: create and remove them, and clean up merged ones automatically.
- Image diffs (side by side, swipe and onion skin).
- A question before rewriting history on main or master.
- Settings window with search, live theme preview and about 80 options in 15 categories.
- API keys stored in Windows Credential Manager.
- Automatic refresh when files change, and background fetch.
- Automatic updates in the background, with this "What's new" window after each one.
- English and Hungarian interface.
