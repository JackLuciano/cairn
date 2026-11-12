use std::sync::Arc;

use cairn_core::diff::{DiffOptions, DiffTarget, FileDiff};
use cairn_core::graph::{self, GraphRow};
use cairn_core::log::{CommitDetails, CommitInfo, LogOptions, LogScope};
use cairn_core::oplog::OpLog;
use cairn_core::patch::LineSelection;
use cairn_core::refs::{Branch, Remote, Stash, Tag};
use cairn_core::repo::InProgress;
use cairn_core::settings::Settings;
use cairn_core::status::RepoStatus;
use cairn_core::{Git, Repository};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::widgets::{Tone, badge, count_chip, framed, hstack, icon, spacer, tr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    History,
    Changes,
    Branches,
    Stack,
    Rebase,
    Absorb,
    Conflicts,
    Blame,
    Bisect,
    FileHistory,
    Compare,
    Recover,
    Stashes,
    Submodules,
    Lfs,
    PullRequests,
}

impl Page {
    /// Pages reached from the "More" tab instead of a tab of their own.
    pub(crate) const MORE: [(Page, &'static str, IconName); 5] = [
        (Page::FileHistory, "page.file_history", IconName::FileClock),
        (Page::Compare, "page.compare", IconName::GitCompareArrows),
        (Page::Recover, "page.recover", IconName::LifeBuoy),
        (Page::Submodules, "page.submodules", IconName::Boxes),
        (Page::Lfs, "page.lfs", IconName::HardDrive),
    ];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectedChange {
    pub path: String,
    pub staged: bool,
    pub untracked: bool,
}

pub(crate) struct Loaded {
    status: RepoStatus,
    commits: Vec<CommitInfo>,
    rows: Vec<GraphRow>,
    branches: Vec<Branch>,
    tags: Vec<Tag>,
    stashes: Vec<Stash>,
    remotes: Vec<Remote>,
    in_progress: InProgress,
    worktrees: Vec<cairn_core::refs::Worktree>,
}

pub(crate) struct OpenRepoEvent(pub std::path::PathBuf);

impl EventEmitter<OpenRepoEvent> for RepoView {}

/// Sent when the History page switches between this branch and all branches, so the choice is saved.
pub(crate) struct HistoryScopeEvent(pub bool);

impl EventEmitter<HistoryScopeEvent> for RepoView {}

fn load_all(repo: &Repository, settings: &Settings, search: Option<String>) -> cairn_core::Result<Loaded> {
    let git = &repo.git;
    let status = cairn_core::status::load_status(git)?;
    let opts = LogOptions {
        scope: if settings.graph.all_branches || !repo.has_commits() { LogScope::AllRefs } else { LogScope::CurrentBranch },
        limit: settings.graph.page_size.max(50),
        first_parent: settings.graph.first_parent,
        include_remotes: settings.graph.show_remotes,
        include_stashes: settings.graph.show_stashes,
        search,
        ..Default::default()
    };
    let commits = cairn_core::log::load_log(git, &opts)?;
    let rows = graph::layout(&commits);
    Ok(Loaded {
        status,
        commits,
        rows,
        branches: cairn_core::refs::list_branches(git).unwrap_or_default(),
        tags: cairn_core::refs::list_tags(git).unwrap_or_default(),
        stashes: cairn_core::refs::list_stashes(git).unwrap_or_default(),
        remotes: cairn_core::refs::list_remotes(git).unwrap_or_default(),
        in_progress: repo.in_progress(),
        worktrees: cairn_core::refs::list_worktrees(git).unwrap_or_default(),
    })
}

pub(crate) struct RepoView {
    pub repo: Arc<Repository>,
    pub name: SharedString,
    pub settings: Settings,
    pub status: RepoStatus,
    pub commits: Vec<CommitInfo>,
    pub rows: Vec<GraphRow>,
    pub branches: Vec<Branch>,
    pub tags: Vec<Tag>,
    pub stashes: Vec<Stash>,
    pub remotes: Vec<Remote>,
    pub worktrees: Vec<cairn_core::refs::Worktree>,
    pub prs: Vec<cairn_core::stack::PullRequest>,
    pub in_progress: InProgress,
    pub page: Page,
    pub loaded_once: bool,
    pub busy: Option<SharedString>,
    pub history_scroll: UniformListScrollHandle,
    pub show_graph: bool,
    pub graph_w: f32,
    pub graph_lane_w: f32,
    pub multi_select: Vec<String>,
    pub last_clicked: Option<usize>,
    pub selected_commit: Option<String>,
    pub details: Option<CommitDetails>,
    pub detail_pushed: Option<(String, bool)>,
    pub detail_file: Option<FileDiff>,
    pub change: Option<SelectedChange>,
    pub change_diff: Option<FileDiff>,
    pub line_selection: LineSelection,
    pub last_line: Option<(usize, usize)>,
    pub summary: Entity<InputState>,
    pub body: Entity<TextareaState>,
    pub amend: bool,
    pub search: Entity<InputState>,
    pub img_mode: u8,
    pub img_amount: f32,
    pub change_pane: crate::diff_view::DiffPane,
    pub commit_pane: crate::diff_view::DiffPane,
    pub page_pane: crate::diff_view::DiffPane,
    pub page_revs: Option<(Option<String>, String)>,
    pub pending_notice: Option<String>,
    pub rebase: crate::pages::rebase::RebaseState,
    pub absorb: crate::pages::absorb::AbsorbState,
    pub conflicts: crate::pages::conflicts::ConflictState,
    pub stack: crate::pages::stack::StackState,
    pub blame: crate::pages::blame::BlameState,
    pub bisect: crate::pages::bisect::BisectPageState,
    pub file_history: crate::pages::file_history::FileHistoryState,
    pub compare: crate::pages::compare::CompareState,
    pub recover: crate::pages::recover::RecoverState,
    pub stash_page: crate::pages::stashes::StashPageState,
    pub submodules: crate::pages::submodules::SubmoduleState,
    pub lfs: crate::pages::lfs::LfsPageState,
    pub pulls: crate::pages::pulls::PullsState,
    focus: FocusHandle,
    watcher: Option<crate::watcher::RepoWatcher>,
    _tasks: Vec<Task<()>>,
}

impl RepoView {
    pub(crate) fn new(repo: Repository, settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name: SharedString = repo.name().into();
        crate::repo_settings::auto_apply(&repo.git, &settings.profiles);
        {
            let git = repo.git.clone();
            let perf = settings.performance.clone();
            cx.background_executor()
                .spawn(async move {
                    if perf.fsmonitor {
                        let _ = git.run(["config", "--local", "core.fsmonitor", "true"]);
                    }
                    if perf.write_commit_graph {
                        let _ = git.run(["commit-graph", "write", "--reachable", "--changed-paths"]);
                    }
                })
                .detach();
        }
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder(tr("changes.summary_placeholder")));
        let body = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10).placeholder(tr("changes.body_placeholder")));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("history.search_placeholder")));
        cx.subscribe(&summary, |_, _, _: &gpui_kit::component::input::InputEvent, cx| cx.notify()).detach();
        cx.subscribe(&search, |this: &mut Self, s, ev: &gpui_kit::component::input::InputEvent, cx| {
            let empty = s.read(cx).value().is_empty();
            if matches!(ev, gpui_kit::component::input::InputEvent::PressEnter { .. }) || (empty && matches!(ev, gpui_kit::component::input::InputEvent::Change)) {
                this.reload(cx);
            }
        })
        .detach();
        let mut view = Self {
            repo: Arc::new(repo),
            name,
            settings,
            status: RepoStatus::default(),
            commits: Vec::new(),
            rows: Vec::new(),
            branches: Vec::new(),
            tags: Vec::new(),
            stashes: Vec::new(),
            remotes: Vec::new(),
            worktrees: Vec::new(),
            prs: Vec::new(),
            in_progress: InProgress::None,
            page: Page::History,
            loaded_once: false,
            busy: None,
            history_scroll: UniformListScrollHandle::new(),
            show_graph: true,
            graph_w: 40.,
            graph_lane_w: 14.,
            multi_select: Vec::new(),
            last_clicked: None,
            selected_commit: None,
            details: None,
            detail_pushed: None,
            detail_file: None,
            change: None,
            change_diff: None,
            line_selection: LineSelection::new(),
            last_line: None,
            summary,
            body,
            amend: false,
            search,
            img_mode: 0,
            img_amount: 0.5,
            change_pane: crate::diff_view::DiffPane::new(cx),
            commit_pane: crate::diff_view::DiffPane::new(cx),
            page_pane: crate::diff_view::DiffPane::new(cx),
            page_revs: None,
            pending_notice: None,
            rebase: Default::default(),
            absorb: Default::default(),
            conflicts: Default::default(),
            stack: Default::default(),
            blame: crate::pages::blame::BlameState::new(window, cx),
            bisect: crate::pages::bisect::BisectPageState::new(window, cx),
            file_history: Default::default(),
            compare: Default::default(),
            recover: Default::default(),
            stash_page: Default::default(),
            submodules: Default::default(),
            lfs: Default::default(),
            pulls: crate::pages::pulls::PullsState::new(window, cx),
            focus: cx.focus_handle(),
            watcher: None,
            _tasks: Vec::new(),
        };
        view.reload(cx);
        view.start_auto_fetch(cx);
        view.start_watching(cx);
        view.start_pr_polling(cx);
        view.start_worktree_cleanup(cx);
        view
    }

    fn start_pr_polling(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            let mut last: std::collections::HashMap<u64, (String, String)> = std::collections::HashMap::new();
            let mut first = true;
            loop {
                let Ok((git, settings)) = this.read_with(cx, |v, _| (v.git(), v.settings.clone())) else { return };
                let wanted = settings.remotes.show_prs_in_graph || settings.notifications.ci_finished || settings.notifications.reviews;
                if wanted {
                    let prs = cx.background_executor().spawn(async move { cairn_core::stack::list_prs(&git).ok() }).await;
                    let Some(prs) = prs else { return };
                    let mut notices = Vec::new();
                    for p in &prs {
                        let done = !p.checks.is_empty() && p.checks.iter().all(|c| matches!(c.state(), "passed" | "failed" | "skipped"));
                        let ci = if done { if p.checks.iter().any(|c| c.state() == "failed") { "failed" } else { "passed" } } else { "running" };
                        let entry = (ci.to_string(), p.review.clone());
                        if let Some(prev) = last.get(&p.number) {
                            if !first && settings.notifications.ci_finished && prev.0 == "running" && ci != "running" {
                                let text = if ci == "passed" {
                                    t!("notify.ci_passed", n = p.number, title = p.title.clone())
                                } else {
                                    t!("notify.ci_failed", n = p.number, title = p.title.clone())
                                };
                                notices.push(text.to_string());
                            }
                            if !first && settings.notifications.reviews && prev.1 != p.review && !p.review.is_empty() {
                                notices.push(t!("notify.review", n = p.number, title = p.title.clone()).to_string());
                            }
                        }
                        last.insert(p.number, entry);
                    }
                    first = false;
                    if this.update(cx, |v, cx| {
                        v.prs = prs;
                        if !notices.is_empty() {
                            v.pending_notice = Some(notices.join("\n"));
                        }
                        cx.notify();
                    }).is_err() {
                        return;
                    }
                }
                let secs = u64::from(settings.remotes.ci_refresh_seconds.max(30));
                cx.background_executor().timer(std::time::Duration::from_secs(secs)).await;
            }
        });
        self._tasks.push(task);
    }

    fn start_watching(&mut self, cx: &mut Context<Self>) {
        if !self.settings.general.watch_files {
            self.watcher = None;
            return;
        }
        let Some(watcher) = crate::watcher::RepoWatcher::start(self.repo.root.clone()) else { return };
        let rx = watcher.rx.clone();
        self.watcher = Some(watcher);
        let task = cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                cx.background_executor().timer(crate::watcher::DEBOUNCE).await;
                while rx.try_recv().is_ok() {}
                let alive = this.update(cx, |this, cx| {
                    if this.busy.is_none() {
                        this.reload(cx);
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        self._tasks.push(task);
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_page(&mut self, page: &str, cx: &mut Context<Self>) {
        self.page = match page {
            "changes" => Page::Changes,
            "branches" => Page::Branches,
            "stack" => Page::Stack,
            "rebase" => Page::Rebase,
            "absorb" => Page::Absorb,
            "conflicts" => Page::Conflicts,
            "blame" => Page::Blame,
            "bisect" => Page::Bisect,
            "file_history" => Page::FileHistory,
            "compare" => Page::Compare,
            "recover" => Page::Recover,
            "stashes" => Page::Stashes,
            "submodules" => Page::Submodules,
            "lfs" => Page::Lfs,
            "pulls" => Page::PullRequests,
            _ => Page::History,
        };
        match self.page {
            Page::Rebase => self.load_rebase(None, cx),
            Page::Absorb => self.load_absorb(cx),
            Page::Conflicts => self.load_conflicts(cx),
            Page::Blame => {
                self.load_blame_files(cx);
                if let Ok(f) = std::env::var("CAIRN_DEBUG_FILE") {
                    self.open_blame(f, cx);
                }
            }
            Page::FileHistory => {
                self.load_blame_files(cx);
                if let Ok(f) = std::env::var("CAIRN_DEBUG_FILE") {
                    self.open_file_history(f, cx);
                }
            }
            Page::Compare => {
                let base = std::env::var("CAIRN_DEBUG_BASE").unwrap_or_default();
                let target = std::env::var("CAIRN_DEBUG_TARGET").unwrap_or_default();
                self.open_compare(base, target, cx);
            }
            Page::Bisect => self.load_bisect(cx),
            Page::Recover => self.load_recover(cx),
            Page::Stashes => self.load_stash_page(cx),
            Page::Submodules => self.load_submodules(cx),
            Page::Lfs => self.load_lfs(cx),
            Page::PullRequests => self.load_pulls(cx),
            _ => {}
        }
        if self.page == Page::Changes {
            let lines = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_LINES").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0) } else { 0 };
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                let _ = this.update(cx, |this, cx| {
                    let first = this.status.unstaged().next().cloned();
                    if let Some(f) = first {
                        this.select_change(SelectedChange { path: f.path.clone(), staged: false, untracked: f.is_untracked() }, cx);
                    }
                });
                if lines == 0 {
                    return;
                }
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                let _ = this.update(cx, |this, cx| {
                    let Some(diff) = this.change_diff.as_ref() else { return };
                    let picked: Vec<(usize, usize)> = diff
                        .hunks
                        .iter()
                        .enumerate()
                        .flat_map(|(h, hunk)| hunk.lines.iter().enumerate().filter(|(_, l)| matches!(l.kind, cairn_core::diff::LineKind::Added | cairn_core::diff::LineKind::Removed)).map(move |(l, _)| (h, l)))
                        .take(lines)
                        .collect();
                    this.line_selection.extend(picked);
                    cx.notify();
                });
            });
            self._tasks.push(task);
        }
        if cfg!(debug_assertions)
            && self.page == Page::History
            && let Ok(file) = std::env::var("CAIRN_DEBUG_FILE")
        {
            let git = self.git();
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                let id = git.run(["log", "-1", "--format=%H", "--", file.as_str()]).map(|s| s.trim().to_string()).unwrap_or_default();
                if id.is_empty() {
                    return;
                }
                let _ = this.update(cx, |this, cx| {
                    this.select_commit(id, cx);
                    this.open_commit_file(file, cx);
                });
            });
            self._tasks.push(task);
        }
        cx.notify();
    }

    pub(crate) fn git(&self) -> Git {
        self.repo.git.clone()
    }

    pub(crate) fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        match page {
            Page::Rebase => self.load_rebase(None, cx),
            Page::Absorb => self.load_absorb(cx),
            Page::Conflicts => self.load_conflicts(cx),
            Page::Stack => self.load_stack(cx),
            Page::Blame | Page::FileHistory => self.load_blame_files(cx),
            Page::Bisect => self.load_bisect(cx),
            Page::Compare => {
                if self.compare.result.is_none() && !self.compare.loading {
                    self.open_compare(String::new(), String::new(), cx);
                }
            }
            Page::Recover => self.load_recover(cx),
            Page::Stashes => self.load_stash_page(cx),
            Page::Submodules => self.load_submodules(cx),
            Page::Lfs => self.load_lfs(cx),
            Page::PullRequests => self.load_pulls(cx),
            _ => {}
        }
        let _ = window;
        cx.notify();
    }

    /// Opens a file or folder in the editor from Settings → Tools, or the system's default program.
    ///
    /// # Arguments
    /// * `path` - the file or folder
    /// * `cx` - the repository view's context
    pub(crate) fn open_in_editor(&self, path: &std::path::Path, cx: &mut Context<Self>) {
        if !crate::platform::open_in_editor(&self.settings.tools.editor, path) {
            cx.open_with_system(path);
        }
    }

    /// Shows one file's diff in the external diff tool.
    ///
    /// # Arguments
    /// * `what` - which states to compare
    /// * `path` - the file
    pub(crate) fn open_difftool(&mut self, what: cairn_core::ops::ToolDiff, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let tool = self.settings.tools.diff_tool.clone();
        let git = self.git();
        cx.spawn_in(window, async move |_, cx| {
            let result = cx.background_executor().spawn(async move { cairn_core::ops::open_difftool(&git, &tool, &what, &path) }).await;
            if let Err(e) = result {
                let _ = cx.update(|window, cx| window.push_notification(Notification::error(friendly_error(&e)).title(tr("tools.diff_failed")), cx));
            }
        })
        .detach();
    }

    /// Opens a conflicted file in the external merge tool.
    ///
    /// # Arguments
    /// * `path` - the conflicted file
    pub(crate) fn open_mergetool(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let tool = self.settings.tools.merge_tool.clone();
        let git = self.git();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { cairn_core::ops::open_mergetool(&git, &tool, &path) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Err(e) = result {
                    window.push_notification(Notification::error(friendly_error(&e)).title(tr("tools.merge_failed")), cx);
                }
                this.conflicts.file = None;
                this.reload(cx);
            });
        })
        .detach();
    }

    /// Asks for a patch file and applies it: mailbox patches become commits, plain diffs change files.
    pub(crate) fn apply_patch_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(tr("tools.apply_patch_prompt")) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(file) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                this.run_op(tr("op.apply_patch"), Some(t!("undo.apply_patch").to_string()), move |git| cairn_core::ops::apply_patch(git, &file), window, cx);
            });
        })
        .detach();
    }

    /// Asks where to save, then writes commits as a patch file.
    ///
    /// # Arguments
    /// * `ids` - the commits to save
    pub(crate) fn save_patch_file(&mut self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let summary = self.commits.iter().find(|c| ids.len() == 1 && c.id == ids[0]).map(|c| c.summary.clone());
        let name = summary
            .map(|s| s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>())
            .map(|s| s.split('-').filter(|p| !p.is_empty()).take(8).collect::<Vec<_>>().join("-"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{}-commits", ids.len()));
        let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| self.repo.root.clone());
        let picked = cx.prompt_for_new_path(&dir, Some(&format!("{name}.patch")));
        let git = self.git();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(dest))) = picked.await else { return };
            let d2 = dest.clone();
            let result = cx.background_executor().spawn(async move { cairn_core::ops::save_patch(&git, &ids, &d2) }).await;
            let _ = cx.update(|window, cx| match result {
                Ok(()) => window.push_notification(Notification::success(t!("tools.patch_saved", path = dest.display().to_string()).to_string()), cx),
                Err(e) => window.push_notification(Notification::error(friendly_error(&e)).title(tr("tools.patch_failed")), cx),
            });
        })
        .detach();
    }

    pub(crate) fn spawn_bg<T, F, A>(&mut self, work: F, apply: A, cx: &mut Context<Self>)
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
        A: FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    {
        let task = cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { work() }).await;
            let _ = this.update(cx, |this, cx| {
                apply(this, result, cx);
                cx.notify();
            });
        });
        self._tasks.push(task);
        self._tasks.retain(|t| !t.is_ready());
    }

    /// Runs `then` once the operation started by `run_op` has finished.
    ///
    /// # Arguments
    /// * `then` - what to do afterwards, such as reloading a page
    pub(crate) fn after_op(&mut self, then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
                match this.read_with(cx, |v, _| v.busy.is_some()) {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(_) => return,
                }
            }
            let _ = this.update(cx, |v, cx| {
                then(v, cx);
                cx.notify();
            });
        });
        self._tasks.push(task);
    }

    pub(crate) fn current_branch(&self) -> Option<&str> {
        self.status.branch.head.as_deref()
    }

    pub(crate) fn diff_options(&self) -> DiffOptions {
        DiffOptions { context: self.settings.diffs.context_lines, ignore_whitespace: self.settings.diffs.ignore_whitespace }
    }

    pub(crate) fn update_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        let reload = settings.graph != self.settings.graph || settings.diffs != self.settings.diffs;
        let watch_changed = settings.general.watch_files != self.settings.general.watch_files;
        self.settings = settings;
        if watch_changed {
            self.start_watching(cx);
        }
        if reload {
            self.reload(cx);
            self.load_change_diff(cx);
        }
        cx.notify();
    }

    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let settings = self.settings.clone();
        let search = Some(self.search.read(cx).value().trim().to_string()).filter(|s| !s.is_empty());
        let task = cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async move { load_all(&repo, &settings, search) }).await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(l) => this.apply_loaded(l, cx),
                    Err(e) => {
                        this.loaded_once = true;
                        log_error(&e.to_string());
                    }
                }
                cx.notify();
            });
        });
        self._tasks.push(task);
        self._tasks.retain(|t| !t.is_ready());
    }

    fn apply_loaded(&mut self, l: Loaded, cx: &mut Context<Self>) {
        let before_behind = self.status.branch.behind;
        let had_conflicts = self.status.conflicted().next().is_some();
        let had_status = self.loaded_once;
        if had_status && l.status.branch.behind > before_behind && l.status.branch.head == self.status.branch.head {
            let n = l.status.branch.behind;
            let notes = self.settings.notifications.clone();
            if notes.fetch_found_commits || (notes.branch_behind && before_behind == 0) {
                self.pending_notice = Some(t!("notify.behind", name = self.name.to_string(), n = n).to_string());
            }
        }
        self.status = l.status;
        self.commits = l.commits;
        let commits = &self.commits;
        self.multi_select.retain(|id| commits.iter().any(|c| &c.id == id));
        if self.multi_select.len() < 2 {
            self.multi_select.clear();
        }
        #[cfg(debug_assertions)]
        if !self.loaded_once
            && let Some(n) = std::env::var("CAIRN_DEBUG_MULTI").ok().and_then(|v| v.parse::<usize>().ok())
        {
            self.multi_select = self.commits.iter().skip(1).take(n).map(|c| c.id.clone()).collect();
        }
        self.rows = l.rows;
        self.branches = l.branches;
        self.tags = l.tags;
        self.stashes = l.stashes;
        self.remotes = l.remotes;
        self.worktrees = l.worktrees;
        self.in_progress = l.in_progress;
        let first_load = !self.loaded_once;
        self.loaded_once = true;
        if first_load && !self.status.is_clean() && self.commits.is_empty() {
            self.page = Page::Changes;
        }
        let has_conflicts = self.status.conflicted().next().is_some();
        if has_conflicts && !had_conflicts && had_status {
            self.page = Page::Conflicts;
        }
        if self.page == Page::Conflicts && has_conflicts && self.conflicts.file.is_none() {
            self.load_conflicts(cx);
        }
        if let Some(sel) = &self.change {
            let still_there = self.status.files.iter().any(|f| {
                f.path == sel.path && if sel.staged { f.staged.is_some() } else { f.unstaged.is_some() }
            });
            if still_there {
                self.load_change_diff(cx);
            } else {
                self.change = None;
                self.change_diff = None;
                self.line_selection.clear();
            }
        }
        if self.selected_commit.is_none()
            && let Some(first) = self.commits.first()
        {
            let id = first.id.clone();
            self.select_commit(id, cx);
        }
    }

    fn start_auto_fetch(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                let minutes = match this.read_with(cx, |this, _| this.settings.git.auto_fetch_minutes) {
                    Ok(m) => m,
                    Err(_) => return,
                };
                let wait = if minutes == 0 { 300 } else { u64::from(minutes) * 60 };
                cx.background_executor().timer(std::time::Duration::from_secs(wait)).await;
                if minutes == 0 {
                    continue;
                }
                let Ok((git, prune, idle)) = this.read_with(cx, |this, _| {
                    (this.git(), this.settings.git.prune_on_fetch, this.busy.is_none())
                }) else {
                    return;
                };
                if !idle || this.read_with(cx, |this, _| this.remotes.is_empty()).unwrap_or(true) {
                    continue;
                }
                let _ = cx
                    .background_executor()
                    .spawn(async move { cairn_core::ops::fetch(&git, None, prune) })
                    .await;
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    return;
                }
            }
        });
        self._tasks.push(task);
    }

    fn start_worktree_cleanup(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(60)).await;
            loop {
                let Ok((git, days, idle)) = this.read_with(cx, |this, _| (this.git(), this.settings.workspace.cleanup_merged_worktrees_days, this.busy.is_none())) else {
                    return;
                };
                if days > 0 && idle {
                    let removed = cx
                        .background_executor()
                        .spawn(async move {
                            let base = ["origin/main", "origin/master", "main", "master"].into_iter().find(|b| git.succeeds(["rev-parse", "--verify", "-q", b]))?;
                            cairn_core::refs::prune_merged_worktrees(&git, base, days).ok()
                        })
                        .await
                        .unwrap_or_default();
                    if !removed.is_empty() {
                        let n = removed.len();
                        if this
                            .update(cx, |this, cx| {
                                this.pending_notice = Some(crate::widgets::plural("worktree.cleaned", n));
                                this.reload(cx);
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                cx.background_executor().timer(std::time::Duration::from_secs(6 * 3600)).await;
            }
        });
        self._tasks.push(task);
    }

    pub(crate) fn run_op<F>(
        &mut self,
        label: SharedString,
        undo_label: Option<String>,
        op: F,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(&Git) -> cairn_core::Result<String> + Send + 'static,
    {
        if self.busy.is_some() {
            window.push_notification(Notification::warning(tr("op.busy")), cx);
            return;
        }
        self.busy = Some(label.clone());
        cx.notify();
        let git = self.git();
        let snapshots = self.settings.checks.snapshots;
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if let (Some(undo), true) = (undo_label, snapshots) {
                        let mut log = OpLog::load(&git);
                        let _ = log.record(&git, &undo);
                    }
                    op(&git)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok(_) => window.push_notification(
                        Notification::success(t!("op.done", what = label.to_string()).to_string()),
                        cx,
                    ),
                    Err(e) => window.push_notification(
                        Notification::error(friendly_error(&e)).title(t!("op.failed", what = label.to_string()).to_string()),
                        cx,
                    ),
                }
                this.reload(cx);
                cx.notify();
            });
        });
        self._tasks.push(task);
        self._tasks.retain(|t| !t.is_ready());
    }

    pub(crate) fn select_commit(&mut self, id: String, cx: &mut Context<Self>) {
        if self.selected_commit.as_deref() == Some(id.as_str()) && self.details.is_some() {
            return;
        }
        self.selected_commit = Some(id.clone());
        self.detail_file = None;
        let git = self.git();
        let upstream = self.status.branch.upstream.clone().or_else(|| self.remotes.first().map(|r| format!("{}/HEAD", r.name)));
        let task = cx.spawn(async move |this, cx| {
            let (details, pushed) = cx
                .background_executor()
                .spawn(async move {
                    let pushed = upstream.as_ref().map(|u| (u.clone(), git.succeeds(["merge-base", "--is-ancestor", &id, u])));
                    (cairn_core::log::commit_details(&git, &id), pushed)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(d) = details
                    && this.selected_commit.as_deref() == Some(d.id.as_str())
                {
                    this.details = Some(d);
                    this.detail_pushed = pushed;
                }
                cx.notify();
            });
        });
        self._tasks.push(task);
        cx.notify();
    }

    pub(crate) fn open_commit_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(id) = self.selected_commit.clone() else { return };
        let git = self.git();
        let opts = self.diff_options();
        let task = cx.spawn(async move |this, cx| {
            let diff = cx
                .background_executor()
                .spawn(async move { cairn_core::diff::file_diff(&git, &DiffTarget::Commit(id), &path, false, opts) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.detail_file = diff.ok();
                cx.notify();
            });
        });
        self._tasks.push(task);
    }

    pub(crate) fn select_change(&mut self, change: SelectedChange, cx: &mut Context<Self>) {
        if self.change.as_ref() != Some(&change) {
            self.line_selection.clear();
            self.last_line = None;
        }
        self.change = Some(change);
        self.load_change_diff(cx);
    }

    pub(crate) fn load_change_diff(&mut self, cx: &mut Context<Self>) {
        let Some(change) = self.change.clone() else { return };
        let git = self.git();
        let opts = self.diff_options();
        let task = cx.spawn(async move |this, cx| {
            let target = if change.staged { DiffTarget::Staged } else { DiffTarget::Unstaged };
            let c = change.clone();
            let diff = cx
                .background_executor()
                .spawn(async move { cairn_core::diff::file_diff(&git, &target, &c.path, c.untracked && !c.staged, opts) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.change.as_ref() == Some(&change) {
                    this.change_diff = diff.ok();
                    let valid: LineSelection = this
                        .line_selection
                        .iter()
                        .copied()
                        .filter(|(h, l)| {
                            this.change_diff
                                .as_ref()
                                .and_then(|d| d.hunks.get(*h))
                                .is_some_and(|hunk| *l < hunk.lines.len())
                        })
                        .collect();
                    this.line_selection = valid;
                }
                cx.notify();
            });
        });
        self._tasks.push(task);
    }

    pub(crate) fn fetch_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fetch(window, cx);
    }

    pub(crate) fn pull_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pull(window, cx);
    }

    pub(crate) fn stash_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stash(window, cx);
    }

    fn handle_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key_char.as_deref() == Some("?") && !window.has_focused_input(cx) {
            crate::palette::open_keymap(self.settings.keyboard.vim_keys, window, cx);
            return;
        }
        if !self.settings.keyboard.vim_keys || window.has_focused_input(cx) || ev.keystroke.modifiers.control || ev.keystroke.modifiers.alt {
            return;
        }
        let key = ev.keystroke.key.as_str();
        match (self.page, key) {
            (Page::History, "j") | (Page::History, "k") => {
                let ix = self.selected_commit.as_ref().and_then(|s| self.commits.iter().position(|c| &c.id == s)).unwrap_or(0);
                let next = if key == "j" { (ix + 1).min(self.commits.len().saturating_sub(1)) } else { ix.saturating_sub(1) };
                if let Some(c) = self.commits.get(next) {
                    let id = c.id.clone();
                    self.history_scroll.scroll_to_item(next, ScrollStrategy::Center);
                    self.select_commit(id, cx);
                }
            }
            (Page::History, "g") => {
                if let Some(c) = self.commits.first() {
                    let id = c.id.clone();
                    self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    self.select_commit(id, cx);
                }
            }
            (Page::History, "G") => {
                if let Some(c) = self.commits.last() {
                    let id = c.id.clone();
                    self.history_scroll.scroll_to_item(self.commits.len() - 1, ScrollStrategy::Bottom);
                    self.select_commit(id, cx);
                }
            }
            (Page::Changes, "j") | (Page::Changes, "k") => {
                let files: Vec<SelectedChange> = self
                    .status
                    .unstaged()
                    .map(|f| SelectedChange { path: f.path.clone(), staged: false, untracked: f.is_untracked() })
                    .chain(self.status.staged().map(|f| SelectedChange { path: f.path.clone(), staged: true, untracked: false }))
                    .collect();
                let ix = self.change.as_ref().and_then(|c| files.iter().position(|f| f == c));
                let next = match (ix, key) {
                    (None, _) => 0,
                    (Some(i), "j") => (i + 1).min(files.len().saturating_sub(1)),
                    (Some(i), _) => i.saturating_sub(1),
                };
                if let Some(f) = files.get(next).cloned() {
                    self.select_change(f, cx);
                }
            }
            (Page::Changes, "s") => {
                if let Some(c) = self.change.clone() {
                    let path = c.path.clone();
                    let staged = c.staged;
                    self.run_op_quiet(tr("op.stage"), move |git| {
                        if staged { cairn_core::ops::unstage_files(git, &[path]) } else { cairn_core::ops::stage_files(git, &[path]) }.map(|_| String::new())
                    }, window, cx);
                }
            }
            (_, "/") => {
                self.open_page(Page::History, window, cx);
                self.search.read(cx).focus_handle(cx).focus(window, cx);
            }
            (_, "f") => self.fetch(window, cx),
            (_, "p") => self.push(false, window, cx),
            (_, "z") => self.open_undo(window, cx),
            (_, "1") => self.open_page(Page::History, window, cx),
            (_, "2") => self.open_page(Page::Changes, window, cx),
            _ => {}
        }
    }

    fn fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remotes.is_empty() {
            window.push_notification(Notification::info(tr("op.no_remote")), cx);
            return;
        }
        let prune = self.settings.git.prune_on_fetch;
        self.run_op(tr("op.fetch"), None, move |git| cairn_core::ops::fetch(git, None, prune), window, cx);
    }

    fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.branch.upstream.is_none() {
            window.push_notification(Notification::info(tr("op.no_upstream_pull")), cx);
            return;
        }
        let mode = self.settings.git.pull_mode;
        let autostash = self.settings.git.autostash;
        self.run_op(tr("op.pull"), Some(t!("undo.pull").to_string()), move |git| cairn_core::ops::pull(git, mode, autostash), window, cx);
    }

    pub(crate) fn push(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.remotes.is_empty() {
            window.push_notification(Notification::info(tr("op.no_remote")), cx);
            return;
        }
        self.guarded(cairn_core::checks::Stage::Push, move |v, window, cx| v.push_now(force, window, cx), window, cx);
    }

    fn push_now(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(branch) = self.current_branch().map(str::to_string) else {
            window.push_notification(Notification::warning(tr("op.detached_push")), cx);
            return;
        };
        let Some(remote) = self.default_remote() else {
            window.push_notification(Notification::info(tr("op.no_remote")), cx);
            return;
        };
        let set_upstream = self.status.branch.upstream.is_none();
        let tags = self.settings.git.push_tags;
        let mode = if force { cairn_core::ops::ForceMode::WithLease } else { cairn_core::ops::ForceMode::None };
        self.run_op(
            tr("op.push"),
            None,
            move |git| cairn_core::ops::push(git, &remote, &branch, set_upstream, mode, tags),
            window,
            cx,
        );
    }

    pub(crate) fn default_remote(&self) -> Option<String> {
        if let Some(up) = &self.status.branch.upstream
            && let Some((remote, _)) = up.split_once('/')
        {
            return Some(remote.to_string());
        }
        self.remotes
            .iter()
            .find(|r| r.name == "origin")
            .or(self.remotes.first())
            .map(|r| r.name.clone())
    }

    fn stash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.is_clean() {
            window.push_notification(Notification::info(tr("op.nothing_to_stash")), cx);
            return;
        }
        self.run_op(
            tr("op.stash"),
            None,
            |git| cairn_core::refs::stash_push(git, None, true).map(|_| String::new()),
            window,
            cx,
        );
    }

    pub(crate) fn checkout(&mut self, target: String, remote: bool, window: &mut Window, cx: &mut Context<Self>) {
        let label = t!("op.checkout", name = target.clone()).to_string();
        self.run_op(
            label.into(),
            Some(t!("undo.checkout", name = target.clone()).to_string()),
            move |git| {
                if remote {
                    cairn_core::refs::checkout_remote(git, &target)
                } else {
                    cairn_core::refs::checkout(git, &target).map(|_| String::new())
                }
            },
            window,
            cx,
        );
    }

    pub(crate) fn open_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::dialogs::undo_history(cx.entity(), window, cx);
    }

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let branch_label: SharedString = self
            .current_branch()
            .map(str::to_string)
            .unwrap_or_else(|| t!("toolbar.detached").to_string())
            .into();
        let ahead = self.status.branch.ahead;
        let behind = self.status.branch.behind;
        let busy = self.busy.clone();
        let local: Vec<Branch> = self.branches.iter().filter(|b| !b.is_remote).cloned().collect();
        let remote_only: Vec<Branch> = self
            .branches
            .iter()
            .filter(|b| b.is_remote && !self.branches.iter().any(|l| !l.is_remote && b.name.ends_with(&format!("/{}", l.name))))
            .cloned()
            .collect();
        let view = cx.entity();

        let branch_button = framed(Button::new("branch-switcher"), Tone::Plain, cx)
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .icon(icon(IconName::GitBranch).text_color(theme.magenta))
            .label(branch_label)
            .dropdown_caret(true)
            .tooltip(tr("toolbar.switch_branch_tip"))
            .dropdown_menu({
                let view = view.clone();
                move |mut menu, _window, _cx| {
                    menu = menu.label(t!("toolbar.local_branches").to_string());
                    for b in &local {
                        let name = b.name.clone();
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(b.name.clone())
                                .checked(b.is_head)
                                .on_click(move |_, window, cx| {
                                    let name = name.clone();
                                    view.update(cx, |v, cx| v.checkout(name, false, window, cx));
                                }),
                        );
                    }
                    if !remote_only.is_empty() {
                        menu = menu.separator().label(t!("toolbar.remote_branches").to_string());
                        for b in remote_only.iter().take(30) {
                            let name = b.name.clone();
                            let view = view.clone();
                            menu = menu.item(PopupMenuItem::new(b.name.clone()).on_click(move |_, window, cx| {
                                let name = name.clone();
                                view.update(cx, |v, cx| v.checkout(name, true, window, cx));
                            }));
                        }
                    }
                    let view_new = view.clone();
                    menu.separator().item(
                        PopupMenuItem::new(t!("toolbar.new_branch").to_string())
                            .icon(icon(IconName::GitBranchPlus))
                            .on_click(move |_, window, cx| {
                                crate::dialogs::new_branch(view_new.clone(), None, window, cx);
                            }),
                    )
                }
            });

        let page_tab = |id: &'static str, page: Page, label: SharedString, count: usize, cx: &mut Context<Self>| {
            let active = self.page == page;
            let theme = cx.theme().clone();
            let fg = if active { theme.foreground } else { theme.muted_foreground };
            let border = if active { theme.primary } else { theme.transparent };
            let tint = if page == Page::Conflicts { theme.red } else { theme.yellow };
            let hover = theme.foreground;
            div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .gap_1p5()
                .h(px(40.))
                .px_3()
                .cursor_pointer()
                .text_sm()
                .whitespace_nowrap()
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(fg)
                .border_b_2()
                .border_color(border)
                .when(!active, |d| d.hover(move |s| s.text_color(hover)))
                .child(label)
                .when(count > 0, |d| d.child(count_chip(count.to_string(), tint.opacity(0.15), tint)))
                .on_click(cx.listener(move |this, _, window, cx| this.open_page(page, window, cx)))
        };
        let change_count = self.status.files.len();
        let conflict_count = self.status.conflicted().count();

        v_flex()
            .w_full()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                hstack()
                    .flex_wrap()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(
                        framed(Button::new("repo-switcher"), Tone::Plain, cx)
                            .icon(icon(IconName::Folder))
                            .label(self.name.clone())
                            .dropdown_caret(true)
                            .tooltip(tr("toolbar.switch_repo_tip"))
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::palette::OpenPalette), cx)),
                    )
                    .child(branch_button)
                    .child(
                        div()
                            .id("command-search")
                            .flex_1()
                            .min_w(px(180.))
                            .max_w(px(420.))
                            .mx_auto()
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary)
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| s.border_color(cx.theme().primary.opacity(0.5)))
                            .child(icon(IconName::Search).small())
                            .child(div().flex_1().min_w_0().truncate().child(tr("toolbar.search_or_run")))
                            .child(crate::widgets::kbd("Ctrl K", cx))
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::palette::OpenPalette), cx)),
                    )
                    .when_some(busy.clone(), |d, label| {
                        d.child(
                            hstack()
                                .gap_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(gpui_kit::component::spinner::Spinner::new().small())
                                .child(label),
                        )
                    })
                    .child(
                        framed(Button::new("undo"), Tone::Plain, cx)
                            .icon(icon(IconName::Undo2))
                            .label(tr("toolbar.undo"))
                            .tooltip(tr("toolbar.undo_tip"))
                            .on_click(cx.listener(|this, _, window, cx| this.open_undo(window, cx))),
                    )
                    .when(self.settings.ai.enabled, |d| {
                        d.child(
                            framed(Button::new("ask-ai"), Tone::Ai, cx)
                                .icon(icon(IconName::Sparkles))
                                .label(tr("toolbar.ask_ai"))
                                .tooltip(tr("toolbar.ask_ai_tip"))
                                .on_click(cx.listener(|_, _, window, cx| crate::assist::open(cx.entity(), window, cx))),
                        )
                    })
                    .child(
                        crate::widgets::framed_if(Button::new("fetch"), Tone::Plain, busy.is_none(), cx)
                            .icon(icon(IconName::RefreshCw))
                            .label(tr("toolbar.fetch"))
                            .tooltip(tr("toolbar.fetch_tip"))
                            .on_click(cx.listener(|this, _, window, cx| this.fetch(window, cx))),
                    )
                    .child(
                        crate::widgets::framed_if(Button::new("pull"), Tone::Plain, busy.is_none(), cx)
                            .icon(icon(IconName::ArrowDownToLine))
                            .label(tr("toolbar.pull"))
                            .when(behind > 0, |b| b.child(count_chip(behind.to_string(), theme.primary.opacity(0.18), theme.primary)))
                            .tooltip(tr("toolbar.pull_tip"))
                            .on_click(cx.listener(|this, _, window, cx| this.pull(window, cx))),
                    )
                    .child({
                        let unpublished = self.status.branch.upstream.is_none() && !self.remotes.is_empty() && self.current_branch().is_some();
                        let tone = if ahead > 0 || unpublished { Tone::Accent } else { Tone::Plain };
                        crate::widgets::framed_if(Button::new("push"), tone, busy.is_none(), cx)
                            .icon(icon(IconName::ArrowUpToLine))
                            .label(tr("toolbar.push"))
                            .when(ahead > 0, |b| b.child(count_chip(ahead.to_string(), gpui_kit::black().opacity(0.18), theme.primary_foreground)))
                            .tooltip(tr("toolbar.push_tip"))
                            .on_click(cx.listener(|this, _, window, cx| this.push(false, window, cx)))
                    })
                    .child(
                        framed(Button::new("more"), Tone::Plain, cx)
                            .px(px(8.))
                            .icon(icon(IconName::Ellipsis))
                            .tooltip(tr("toolbar.more_tip"))
                            .dropdown_menu({
                                let view = view.clone();
                                let root = self.repo.root.clone();
                                move |menu, _, _| {
                                    let root_a = root.clone();
                                    let root_b = root.clone();
                                    let root_c = root.clone();
                                    let v1 = view.clone();
                                    let v2 = view.clone();
                                    let v4 = view.clone();
                                    let v5 = view.clone();
                                    let v6 = view.clone();
                                    let v7 = view.clone();
                                    menu.item(
                                        PopupMenuItem::new(t!("toolbar.stash").to_string())
                                            .icon(icon(IconName::Archive))
                                            .on_click(move |_, window, cx| v4.update(cx, |v, cx| v.stash(window, cx))),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.stash_some").to_string())
                                            .icon(icon(IconName::ArchiveRestore))
                                            .on_click(move |_, window, cx| crate::dialogs::stash_dialog(v5.clone(), Vec::new(), window, cx)),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.apply_patch").to_string())
                                            .icon(icon(IconName::FileInput))
                                            .on_click(move |_, window, cx| v6.update(cx, |v, cx| v.apply_patch_file(window, cx))),
                                    )
                                    .separator()
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.open_folder").to_string())
                                            .icon(icon(IconName::FolderOpen))
                                            .on_click(move |_, _, cx| cx.reveal_path(&root_a)),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.open_terminal").to_string())
                                            .icon(icon(IconName::Terminal))
                                            .on_click(move |_, _, _| crate::platform::open_terminal(&root_b)),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.open_editor").to_string())
                                            .icon(icon(IconName::Code))
                                            .on_click(move |_, _, cx| v7.update(cx, |v, cx| v.open_in_editor(&root_c, cx))),
                                    )
                                    .item({
                                        let v3 = view.clone();
                                        PopupMenuItem::new(t!("palette.repo_settings").to_string())
                                            .icon(icon(IconName::Settings2))
                                            .on_click(move |_, window, cx| crate::repo_settings::open(v3.clone(), window, cx))
                                    })
                                    .separator()
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.force_push").to_string())
                                            .icon(icon(IconName::ArrowUpToLine))
                                            .on_click(move |_, window, cx| {
                                                crate::dialogs::confirm_force_push(v1.clone(), window, cx)
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new(t!("toolbar.refresh").to_string())
                                            .icon(icon(IconName::RotateCw))
                                            .on_click(move |_, _, cx| v2.update(cx, |v, cx| v.reload(cx))),
                                    )
                                }
                            }),
                    ),
            )
            .child(
                hstack()
                    .id("page-tabs")
                    .px_3()
                    .gap_0p5()
                    .border_t_1()
                    .border_color(theme.border)
                    .overflow_x_scroll()
                    .child(page_tab("page-history", Page::History, tr("page.history"), 0, cx))
                    .child(page_tab("page-changes", Page::Changes, tr("page.changes"), change_count, cx))
                    .child(page_tab("page-branches", Page::Branches, tr("page.branches"), 0, cx))
                    .child(page_tab("page-stack", Page::Stack, tr("page.stack"), 0, cx))
                    .child(page_tab("page-rebase", Page::Rebase, tr("page.rebase"), 0, cx))
                    .child(page_tab("page-absorb", Page::Absorb, tr("page.absorb"), 0, cx))
                    .child(page_tab("page-conflicts", Page::Conflicts, tr("page.conflicts"), conflict_count, cx))
                    .child(page_tab("page-blame", Page::Blame, tr("page.blame"), 0, cx))
                    .child(page_tab("page-bisect", Page::Bisect, tr("page.bisect"), 0, cx))
                    .child(page_tab("page-stashes", Page::Stashes, tr("page.stashes"), self.stashes.len(), cx))
                    .child(page_tab("page-pulls", Page::PullRequests, tr("page.pulls"), self.prs.len(), cx))
                    .child({
                        let current = Page::MORE.iter().find(|(p, _, _)| *p == self.page);
                        let active = current.is_some();
                        let label: SharedString = current.map(|(_, l, _)| tr(l)).unwrap_or_else(|| tr("page.more"));
                        let view = view.clone();
                        Button::new("page-more")
                            .ghost()
                            .h(px(40.))
                            .px_3()
                            .rounded_none()
                            .text_sm()
                            .when(active, |b| b.font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).border_b_2().border_color(theme.primary))
                            .when(!active, |b| b.font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground))
                            .label(label)
                            .dropdown_caret(true)
                            .dropdown_menu(move |mut menu, _, _| {
                                for (page, label, ic) in Page::MORE {
                                    let v = view.clone();
                                    menu = menu.item(PopupMenuItem::new(t!(label).to_string()).icon(icon(ic)).on_click(move |_, window, cx| v.update(cx, |v, cx| v.open_page(page, window, cx))));
                                }
                                menu
                            })
                    }),
            )
            .when(self.in_progress != InProgress::None, |d| d.child(self.render_in_progress(window, cx)))
    }

    fn render_in_progress(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (text, cont, abort): (SharedString, Option<&'static str>, &'static str) = match self.in_progress {
            InProgress::Rebase => (tr("progress.rebase"), Some("rebase"), "rebase"),
            InProgress::Merge => (tr("progress.merge"), Some("merge"), "merge"),
            InProgress::CherryPick => (tr("progress.cherry_pick"), Some("cherry-pick"), "cherry-pick"),
            InProgress::Revert => (tr("progress.revert"), Some("revert"), "revert"),
            InProgress::Bisect => (tr("progress.bisect"), None, "bisect"),
            InProgress::None => (SharedString::default(), None, ""),
        };
        let conflicts = self.status.conflicted().count();
        let rebase_dir = self.repo.git_dir.join("rebase-merge");
        let read = |name: &str| std::fs::read_to_string(rebase_dir.join(name)).ok().map(|s| s.trim().to_string());
        let detail = if self.in_progress == InProgress::Rebase {
            match (read("head-name"), read("msgnum"), read("end")) {
                (Some(head), Some(step), Some(end)) => Some(t!("progress.rebase_detail", branch = head.trim_start_matches("refs/heads/").to_string(), step = step, total = end).to_string()),
                _ => None,
            }
        } else {
            None
        };
        let is_rebase = self.in_progress == InProgress::Rebase;
        hstack()
            .flex_wrap()
            .gap(px(10.))
            .px_4()
            .py_2()
            .bg(theme.yellow.opacity(0.12))
            .border_t_1()
            .border_color(theme.border)
            .child(icon(IconName::TriangleAlert).text_color(theme.yellow))
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(text))
            .when_some(detail, |d, t| d.child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(t)))
            .when(conflicts > 0, |d| d.child(badge(crate::widgets::plural("progress.conflicts", conflicts), theme.red)))
            .child(spacer())
            .when(is_rebase, |d| {
                d.child(
                    framed(Button::new("skip-op"), Tone::Plain, cx)
                        .label(tr("progress.skip"))
                        .tooltip(tr("progress.skip_tip"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run_op(tr("progress.skip"), None, |git| git.clone().with_env("GIT_EDITOR", "true").run(["rebase", "--skip"]), window, cx);
                        })),
                )
            })
            .when_some(cont, |d, cmd| {
                d.child(
                    crate::widgets::framed_if(Button::new("continue-op"), Tone::Accent, conflicts == 0, cx)
                        .label(tr("progress.continue"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let cmd = cmd.to_string();
                            this.run_op(tr("progress.continue"), None, move |git| {
                                git.clone().with_env("GIT_EDITOR", "true").run([cmd.as_str(), "--continue"])
                            }, window, cx);
                        })),
                )
            })
            .child(
                framed(Button::new("abort-op"), Tone::Plain, cx)
                    .text_color(theme.red)
                    .label(if abort == "bisect" { tr("progress.stop_bisect") } else { tr("progress.abort") })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let cmd = abort.to_string();
                        this.run_op(tr("progress.abort"), None, move |git| {
                            if cmd == "bisect" { git.run(["bisect", "reset"]) } else { git.run([cmd.as_str(), "--abort"]) }
                        }, window, cx);
                    })),
            )
    }
}

impl Focusable for RepoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RepoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(text) = self.pending_notice.take() {
            window.push_notification(Notification::info(text).title(self.name.clone()).in_app_and_system(), cx);
        }
        let body = if !self.loaded_once {
            div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element()
        } else {
            match self.page {
                Page::History => self.render_history(window, cx).into_any_element(),
                Page::Changes => self.render_changes(window, cx).into_any_element(),
                Page::Branches => self.render_branches(window, cx).into_any_element(),
                Page::Stack => self.render_stack(window, cx).into_any_element(),
                Page::Rebase => self.render_rebase(window, cx).into_any_element(),
                Page::Absorb => self.render_absorb(window, cx).into_any_element(),
                Page::Conflicts => self.render_conflicts(window, cx).into_any_element(),
                Page::Blame => self.render_blame(window, cx).into_any_element(),
                Page::Bisect => self.render_bisect(window, cx).into_any_element(),
                Page::FileHistory => self.render_file_history(window, cx).into_any_element(),
                Page::Compare => self.render_compare(window, cx).into_any_element(),
                Page::Recover => self.render_recover(window, cx).into_any_element(),
                Page::Stashes => self.render_stash_page(window, cx).into_any_element(),
                Page::Submodules => self.render_submodules(window, cx).into_any_element(),
                Page::Lfs => self.render_lfs(window, cx).into_any_element(),
                Page::PullRequests => self.render_pulls(window, cx).into_any_element(),
            }
        };
        v_flex()
            .size_full()
            .key_context("RepoView")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.handle_key(ev, window, cx)))
            .on_action(cx.listener(|this, _: &crate::palette::UndoHistory, window, cx| this.open_undo(window, cx)))
            .child(self.render_toolbar(window, cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

pub(crate) fn friendly_error(e: &cairn_core::Error) -> String {
    match e {
        cairn_core::Error::GitMissing => t!("error.git_missing").to_string(),
        cairn_core::Error::GitCommand { message, .. } => {
            let m = message.to_lowercase();
            if m.contains("authentication failed") || m.contains("could not read username") || m.contains("permission denied (publickey)") {
                t!("error.auth").to_string()
            } else if m.contains("non-fast-forward") || m.contains("fetch first") || m.contains("rejected") {
                t!("error.rejected").to_string()
            } else if m.contains("would be overwritten") {
                t!("error.overwritten").to_string()
            } else if m.contains("could not resolve host") || m.contains("unable to access") {
                t!("error.network").to_string()
            } else if m.contains("conflict") {
                t!("error.conflict").to_string()
            } else {
                message.lines().take(6).collect::<Vec<_>>().join("\n")
            }
        }
        other => other.to_string(),
    }
}

pub(crate) fn log_error(msg: &str) {
    let path = cairn_core::paths::data_dir().join("cairn.log");
    let line = format!("{} {msg}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
