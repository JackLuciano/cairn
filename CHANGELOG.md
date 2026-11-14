# Changelog

All notable changes to Cairn are listed here. Each section is shown in the app's "What's new"
window after an update.

## [Unreleased]

### Added

- Show Gravatar images for commit authors in History, with initials when no image is available.
  Cached images are available offline and refresh in the background after 24 hours.
- Use Codex CLI for Cairn's AI features. Settings → AI detects local CLI installations and lets
  you choose Codex CLI, Claude Code, or automatic selection.

### Fixed

- Restore the last window size and maximized state when reopening Cairn.

## [0.5.2] - 2026-11-13

No more pile of "still running" warnings.

### Fixed

- Sync no longer lets a second click start between its steps, and a click while something is
  running shows one short note naming what is running instead of a pile of warnings.

## [0.5.1] - 2026-11-13

Sync the way VS Code does it, discards you can take back, and fewer accidental commits to main.

### Added

- Choose what happens after a commit in Settings → Commits: nothing, push, or sync (pull what's
  new, then push). The commit button says what it will do, and the second button commits only.
- "Ask before syncing" shows how many commits Sync will pull and push before it starts.
- Sync warns when someone rebased or force pushed the branch on the server, since merging it
  would keep two copies of the same commits, and offers a rebase instead.
- "Sync settings…" in the Sync button's menu.
- Discarded changes can be brought back: Cairn keeps a copy of every file before discarding
  it, discarding lines from it, or a hard reset, and lists them in Lost & found → Discarded
  changes with a "Bring back" button.
- Committing straight to `main` offers to put the commit on a new branch named after its
  message instead, with "Don't ask again".
- With nothing staged, the commit button offers to commit every changed file (Settings →
  Commits → When nothing is staged).
- Switching to a branch that is only behind the server fetches it and brings it up to date
  first, so you never start on an old `main`.
- Stop a running push, pull or fetch from its progress toast.
- A prefix for new branch names (such as `jack/`), and spaces typed in branch names become
  dashes.
- Ignored files such as `.env` are copied into new worktrees, so they run straight away
  (Settings → Git behavior → Branches).

- Pick a repository for the activity charts on Home, or see all of them together.

### Changed

- The activity charts on Home count a commit once even when several clones or worktrees of
  the same project contain it.
- Sync stops at the first step that fails instead of pulling or pushing after a failed fetch,
  and says so when there is nothing to pull or push. Commit and push no longer pushes when the
  commit failed.

## [0.5.0] - 2026-11-13

A home that tells you where you stand, a branch picker you can search, and clearer, safer
screens everywhere.

### Added

- Home is now a dashboard: a greeting with what needs you, tiles for repositories with changes,
  pulls waiting and reviews, and every pull request waiting for you across all repositories.
- Activity charts on Home: a six-month contribution calendar, commits per week, and who
  contributed how much in the last 90 days, with commits, share and lines added and removed.
- A searchable branch picker replaces the long branch menu. Branches are grouped into current,
  recent, yours and only on the server, each with its last commit, pull request and how far it
  is behind. Switch, compare, highlight or open a pull request right from it. The Highlight
  button in History uses it too.
- Click any commit of a pull request to see only that commit's changes, and step through the
  commits one by one.
- "Create pull request from…" in a commit's right-click menu, for the branches on that commit.
- Push, pull and fetch show their progress in a corner toast: downloading or uploading, a
  percentage, and whether it finished.
- The diff toolbar shows how many lines changed, switches between unified and split view, word
  highlights and line wrapping, and jumps between changes. A change map along the edge shows
  where the changes are; click it to jump there. Your choices are remembered for every tab.
- Settings for appearance, the graph and diffs show a live preview of History and a diff.
- Repository cards on Home show changes, pulls and pushes waiting.

### Changed

- Pages are grouped into Work and Review, and the less common pages live in a Tools panel that
  says what each one does. The Conflicts tab only appears when there are conflicts, and narrow
  windows show icons only.
- Branches are cards with their last commit, pull request, how far ahead or behind `main` they
  are, and conflict warnings, with All, Active and Merged filters.
- Stashes are cards with the files inside, the branch they came from and how old they are.
  Apply, pop and drop appear when you hover one.
- Reset, force push, discard, deleting a branch and dropping a stash explain what happens, what
  you lose, and whether you can get it back.
- The commit card in History scrolls, shows the full message and the changed files, is readable
  in every theme, and hides when you right-click.
- The branch legend in History lists the branches on screen, fits the window instead of
  scrolling, and puts the rest behind "+N". Narrow windows hide the Author and Branch columns.
- Buttons, tabs and cards react when you press them, and the progress toast slides in. "Reduce
  motion" keeps everything still.
- Built with Rust 1.100.

### Fixed

- Opening a pull request from a branch that only exists on the server no longer tries to push
  it first.
- Dialogs stay solid with the glass window on Windows 11.

## [0.4.0] - 2026-11-13

Ship from Cairn, see every branch at a glance, and a faster, fresher look.

### Added

- History shows which branch every commit belongs to: a Branch column, one color per branch
  from its line to its label, and names for merged branches taken from the merge message.
- Commits that are not on your branch fade out. Pick another branch in the new legend, the
  Highlight menu or by clicking any branch label to highlight that one instead.
- Hover a commit in History for a card with its message, author, branch and labels.
- History loads older commits as you scroll, past the first 500.
- Open a pull request from Cairn: base branch, reviewers, labels and draft mode, with the title
  and description written by AI if you like. A branch that was never pushed is published first.
- AI review of a pull request: Claude drafts comments on the exact lines; untick what you don't
  want and post the rest as one review.
- "Waiting for your review" in each category shows the pull requests that need you across all
  of its repositories.
- Branches and pull requests that would conflict with `main` get a warning that names the files,
  checked in the background without touching your work.
- Files you marked as viewed in a pull request stay marked after a restart, and are unmarked
  when they change again.
- Go to file (Ctrl+P) opens any file's history, line history or the file itself.
- New branch from a GitHub issue, named after the issue and linked to it.
- Six color themes (Graphite, Midnight, Nord, Solarized, Rosé, High contrast) in dark and
  light, a glass window on Windows 11 and a compact toolbar option.
- A status bar with the branch, sync state, last fetch, changes and background work.
- Bug report, feature request and translation forms for GitHub issues, and a pull request
  template.

### Changed

- One Sync button fetches, pulls and pushes whatever is needed; Fetch, Pull and Push are still
  in its menu.
- Pages show skeleton placeholders while loading instead of spinners, and keep what they showed
  while refreshing, so switching pages is instant.
- Restored tabs only load when you open them, so Cairn starts faster with many tabs.
- Staging, unstaging, committing, stashing and switching branches update the screen right away.
- Very large diffs open at once with their first part and a "Show everything" button, instead
  of being hidden.
- Softer cards, glowing main buttons and active tabs, and short fades between pages. "Reduce
  motion" turns the animations off.
- The commit details, file list and pull request list panels can be resized, and keep their
  width.

## [0.3.2] - 2026-11-12

A new look for pull requests.

### Changed

- Each pull request opens with a header card: who opened it, where it goes, a ring that fills as
  reviews, checks and conflicts are sorted out, and tiles for files, commits, lines, checks and
  approvals.
- The Overview shows the description as a card and the discussion as a chat, with reviews and
  pushed commits in between. A side panel says what is still needed and holds the merge button.
- Descriptions and comments are shown as formatted Markdown.
- Files changed shows a file type badge and a size bar for each file.
- When nobody has reviewed a pull request that can be merged, Cairn says so instead of
  "Everything looks good".

### Fixed

- Repositories cloned with a filter (partial clones) were treated as having no remote, so Pull
  requests, Fetch and Push did not work in them.

## [0.3.1] - 2026-11-12

### Fixed

- Pull requests no longer says the repository has no remote when it does. This happened when
  the page opened before Cairn had finished reading the repository.

## [0.3.0] - 2026-11-12

Find anything, undo anything, and a calmer History.

### Added

- File history: every commit that changed a file, followed through renames, with each change.
  Open it from a file's right-click menu, Line history or More → File history.
- Compare: pick two branches, tags or commits to see what is only on each side and how the files
  differ. "Compare with current branch" is in the branch and commit menus.
- Lost & found: commits from deleted branches, resets and dropped stashes, with "Restore as
  branch".
- A Stashes page: look inside a stash, restore a single file from it, apply, restore or delete it.
  Stash only some files, or only what you staged, including single blocks and lines.
- Drag a branch label onto another branch in History to merge or rebase it, or drag commits onto a
  branch to cherry-pick them.
- A Pull requests page: filter by "To review", "Mine" or "Drafts", see at a glance what each pull
  request still needs (reviews, checks, conflicts), read the conversation, commits and checks, go
  through the changed files with a "viewed" tick per file, comment on single lines, approve or
  request changes, check it out locally, mark a draft ready and merge it (squash, merge commit or
  rebase).
- Submodules: see their state, download and update them, add new ones and open them in a tab.
- Git LFS: track file types, download LFS files, lock and unlock files.
- Settings → Tools: your editor, external diff tool and merge tool. "Open in editor", "Open in
  diff tool" and "Open in merge tool" are in the matching menus.
- Ignore an untracked file, its file type or its folder from the Changes list.
- Save commits as a patch file, and apply patch files from the ⋯ menu.
- macOS and Linux builds are compiled on every pull request (not packaged yet).

### Changed

- History opens on the branch you are on. Switch to "All branches" at the top; Cairn remembers it.
- In "All branches", branches that start from the same commit share a lane instead of each getting
  its own line, and the graph never grows wider than its column.
- The sidebar groups branches into folders such as `feat/`, keeps the current branch on top, and
  lets you fold Branches, Worktrees and Stashes. Worktrees take one line each.
- Buttons that can't be used right now, such as Commit with nothing staged, are dimmed and can't
  be clicked.
- Stashes show their own message instead of "On branch: …".

## [0.2.4] - 2026-11-12

### Fixed

- The Branches page scrolls again when a repository has many branches, instead of squeezing
  the lists to fit the window. Absorb & split, Bisect, Ask AI and repository settings got the
  same fix.

## [0.2.3] - 2026-11-12

### Added

- "Copy with messages" copies the selected commits as `hash - message` lines, newest first, like
  `git log --oneline`. A single commit's menu has "Copy hash and message".

### Fixed

- Selecting dozens of commits and copying their hashes no longer freezes the app.
- Opening Cairn from a pinned taskbar icon now lights up that icon instead of adding a second
  one.

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
