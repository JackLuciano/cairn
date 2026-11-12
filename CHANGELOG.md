# Changelog

All notable changes to Cairn are listed here. Each section is shown in the app's "What's new"
window after an update.

## [Unreleased]

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
