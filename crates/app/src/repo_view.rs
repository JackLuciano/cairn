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
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
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
    /// Pages reached from the "Tools" panel instead of a tab of their own, each with a one-line
    /// explanation for people who do not know the git term.
    pub(crate) const TOOLS: [(Page, &'static str, IconName, &'static str); 10] = [
        (Page::Rebase, "page.rebase", IconName::GitPullRequestArrow, "page_bar.rebase_desc"),
        (Page::Absorb, "page.absorb", IconName::Scissors, "page_bar.absorb_desc"),
        (Page::Conflicts, "page.conflicts", IconName::TriangleAlert, "page_bar.conflicts_desc"),
        (Page::Compare, "page.compare", IconName::GitCompareArrows, "page_bar.compare_desc"),
        (Page::Blame, "page.blame", IconName::TextSearch, "page_bar.blame_desc"),
        (Page::FileHistory, "page.file_history", IconName::FileClock, "page_bar.file_history_desc"),
        (Page::Bisect, "page.bisect", IconName::Crosshair, "page_bar.bisect_desc"),
        (Page::Recover, "page.recover", IconName::LifeBuoy, "page_bar.recover_desc"),
        (Page::Submodules, "page.submodules", IconName::Boxes, "page_bar.submodules_desc"),
        (Page::Lfs, "page.lfs", IconName::HardDrive, "page_bar.lfs_desc"),
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
    owners: Vec<Option<String>>,
    palette: Vec<usize>,
    owner_colors: std::collections::HashMap<String, usize>,
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

/// Sent when the diff toolbar changes how diffs look, so the choice is saved.
pub(crate) struct DiffPrefsEvent(pub cairn_core::settings::Diffs);

impl EventEmitter<DiffPrefsEvent> for RepoView {}

/// Sent when a side panel was resized, so the width is saved: the commit details panel, the file
/// list on the Changes and Pull requests pages.
pub(crate) struct PanelWidthEvent {
    pub details: Option<f32>,
    pub list: Option<f32>,
}

impl EventEmitter<PanelWidthEvent> for RepoView {}

/// Works out which loaded commits belong to the highlighted branch.
///
/// # Arguments
/// * `commits` - the loaded history, newest first
/// * `focus` - the branch to highlight; the checked-out commit when `None` or not loaded
/// * `searching` - whether the history is filtered by a search, which breaks the parent chain
///
/// # Returns
/// One flag per commit, or `None` to show every commit at full strength.
pub(crate) fn focus_hot(commits: &[CommitInfo], focus: Option<&str>, searching: bool) -> Option<Vec<bool>> {
    if searching {
        return None;
    }
    let named = focus.and_then(|name| commits.iter().find(|c| c.refs.iter().any(|r| r.name == name && r.kind != cairn_core::log::RefKind::Stash)));
    let tip = named.or_else(|| commits.iter().find(|c| c.refs.iter().any(|r| r.is_head)))?;
    graph::reachable(commits, &tip.id)
}

fn load_all(repo: &Repository, settings: &Settings, search: Option<String>, limit: usize, focus: Option<String>) -> cairn_core::Result<Loaded> {
    let git = &repo.git;
    let status = cairn_core::status::load_status(git)?;
    let searching = search.is_some();
    let opts = LogOptions {
        scope: if settings.graph.all_branches || !repo.has_commits() { LogScope::AllRefs } else { LogScope::CurrentBranch },
        limit: limit.max(settings.graph.page_size.max(50)),
        first_parent: settings.graph.first_parent,
        include_remotes: settings.graph.show_remotes,
        include_stashes: settings.graph.show_stashes,
        search,
        ..Default::default()
    };
    let commits = cairn_core::log::load_log(git, &opts)?;
    let laid = crate::history::HistoryLayout::compute(&commits, focus.as_deref(), searching);
    Ok(Loaded {
        status,
        commits,
        rows: laid.rows,
        owners: laid.owners,
        palette: laid.palette,
        owner_colors: laid.owner_colors,
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
    pub owners: Vec<Option<String>>,
    pub palette: Vec<usize>,
    pub owner_colors: std::collections::HashMap<String, usize>,
    pub branches: Vec<Branch>,
    pub tags: Vec<Tag>,
    pub stashes: Vec<Stash>,
    pub remotes: Vec<Remote>,
    pub worktrees: Vec<cairn_core::refs::Worktree>,
    pub prs: Vec<cairn_core::stack::PullRequest>,
    pub in_progress: InProgress,
    pub page: Page,
    pub loaded_once: bool,
    pub started: bool,
    pub busy: Option<SharedString>,
    pub history_scroll: UniformListScrollHandle,
    pub show_graph: bool,
    pub graph_w: f32,
    pub graph_lane_w: f32,
    pub focus_branch: Option<String>,
    pub fresh: std::collections::HashSet<String>,
    pub trunk: Option<String>,
    pub clashes: std::collections::HashMap<String, Vec<String>>,
    conflict_sig: String,
    pub history_limit: usize,
    pub loading_more: bool,
    pub history_visible: std::ops::Range<usize>,
    pub history_width: std::rc::Rc<std::cell::Cell<f32>>,
    pub page_bar_width: std::rc::Rc<std::cell::Cell<f32>>,
    pub branch_stats: (String, std::collections::HashMap<String, (u32, u32)>),
    pub branch_stats_loading: bool,
    pub branch_filter: crate::pages::branches::BranchFilter,
    pub live_op: Option<crate::live_op::LiveOp>,
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
    pub(crate) _tasks: Vec<Task<()>>,
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
            owners: Vec::new(),
            palette: Vec::new(),
            owner_colors: Default::default(),
            branches: Vec::new(),
            tags: Vec::new(),
            stashes: Vec::new(),
            remotes: Vec::new(),
            worktrees: Vec::new(),
            prs: Vec::new(),
            in_progress: InProgress::None,
            page: Page::History,
            loaded_once: false,
            started: false,
            busy: None,
            history_scroll: UniformListScrollHandle::new(),
            show_graph: true,
            graph_w: 40.,
            graph_lane_w: 14.,
            focus_branch: None,
            fresh: Default::default(),
            trunk: None,
            clashes: Default::default(),
            conflict_sig: String::new(),
            history_limit: 0,
            loading_more: false,
            history_visible: 0..0,
            history_width: Default::default(),
            page_bar_width: Default::default(),
            branch_stats: Default::default(),
            branch_stats_loading: false,
            branch_filter: Default::default(),
            live_op: None,
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
        #[cfg(debug_assertions)]
        if let Ok(f) = std::env::var("CAIRN_DEBUG_FOCUS") {
            view.focus_branch = Some(f);
        }
        view
    }

    /// Remembers a resized panel width for this session and asks the app to save it.
    ///
    /// # Arguments
    /// * `details` - the new commit details panel width
    /// * `list` - the new file list width
    pub(crate) fn remember_panel(&mut self, details: Option<f32>, list: Option<f32>, cx: &mut Context<Self>) {
        if let Some(w) = details {
            self.settings.appearance.details_width = w;
        }
        if let Some(w) = list {
            self.settings.appearance.list_width = w;
        }
        cx.emit(PanelWidthEvent { details, list });
    }

    /// Starts loading the repository and its background work the first time its tab is shown,
    /// so restoring many tabs at startup only loads the one on screen.
    pub(crate) fn wake(&mut self, cx: &mut Context<Self>) {
        if self.started {
            return;
        }
        self.started = true;
        self.reload(cx);
        self.start_auto_fetch(cx);
        self.start_watching(cx);
        self.start_pr_polling(cx);
        self.start_worktree_cleanup(cx);
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
        if !self.started {
            return;
        }
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
        let limit = self.history_limit;
        let focus = self.focus_branch.clone();
        let task = cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async move { load_all(&repo, &settings, search, limit, focus) }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
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
        let before: std::collections::HashSet<String> = if self.loaded_once { self.commits.iter().take(50).map(|c| c.id.clone()).collect() } else { Default::default() };
        self.commits = l.commits;
        self.mark_fresh(&before, cx);
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
        self.owners = l.owners;
        self.palette = l.palette;
        self.owner_colors = l.owner_colors;
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
        self.check_conflicts(cx);
    }

    /// Finds branches and pull requests that would conflict with the branch they go into, in the
    /// background and only when a branch tip moved since the last check.
    pub(crate) fn check_conflicts(&mut self, cx: &mut Context<Self>) {
        let mut local: Vec<&Branch> = self.branches.iter().filter(|b| !b.is_remote).collect();
        local.sort_by_key(|b| std::cmp::Reverse(b.last_commit_time));
        let local: Vec<(String, String)> = local.into_iter().take(40).map(|b| (b.name.clone(), b.target.clone())).collect();
        let prs: Vec<(u64, String, String)> = self.prs.iter().chain(self.pulls.list.iter()).map(|p| (p.number, p.head.clone(), p.base.clone())).collect();
        let remote_tips: String = self.branches.iter().filter(|b| b.is_remote).map(|b| b.target.as_str()).collect::<Vec<_>>().join(",");
        let sig = format!("{local:?}{prs:?}{remote_tips}");
        if sig == self.conflict_sig {
            return;
        }
        self.conflict_sig = sig;
        let git = self.git();
        self.spawn_bg(
            move || {
                let trunk = cairn_core::conflicts::default_trunk(&git);
                let mut found = std::collections::HashMap::new();
                if let Some(trunk) = trunk.as_deref() {
                    let trunk_short = trunk.split_once('/').map(|(_, b)| b).unwrap_or(trunk);
                    for (name, tip) in &local {
                        if name == trunk_short || name == trunk || git.succeeds(["merge-base", "--is-ancestor", tip, trunk]) {
                            continue;
                        }
                        if let Some(files) = cairn_core::conflicts::merge_conflicts(&git, trunk, tip).filter(|f| !f.is_empty()) {
                            found.insert(name.clone(), files);
                        }
                    }
                }
                let mut seen = std::collections::HashSet::new();
                for (number, head, base) in prs {
                    if !seen.insert(number) {
                        continue;
                    }
                    let (h, b) = (format!("origin/{head}"), format!("origin/{base}"));
                    if let Some(files) = cairn_core::conflicts::merge_conflicts(&git, &b, &h).filter(|f| !f.is_empty()) {
                        found.insert(format!("#{number}"), files);
                    }
                }
                (trunk, found)
            },
            |this, (trunk, mut found), _| {
                this.trunk = trunk.map(|t| t.split_once('/').filter(|(r, _)| this.remotes.iter().any(|x| x.name == *r)).map(|(_, b)| b.to_string()).unwrap_or(t));
                if this.pulls.mock {
                    found.insert("#45".into(), vec!["src/screens/Settings.tsx".into()]);
                }
                this.clashes = found;
            },
            cx,
        );
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
        self.live_op = None;
        cx.notify();
        let progress: crate::live_op::SharedProgress = Default::default();
        let sink = progress.clone();
        let git = self.git().with_progress(move |phase, done| {
            if let Ok(mut p) = sink.lock() {
                *p = Some((phase.to_string(), done));
            }
        });
        self.watch_progress(label.clone(), progress, cx);
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
                let shown = this.finish_live_op(result.is_ok(), cx);
                match result {
                    Ok(_) if shown => {}
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

    /// One button for staying in step with the remote: fetches, pulls when the remote has new
    /// commits and pushes when this branch has commits the remote lacks. Publishes a branch that
    /// was never pushed.
    pub(crate) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remotes.is_empty() {
            window.push_notification(Notification::info(tr("op.no_remote")), cx);
            return;
        }
        if self.status.branch.upstream.is_none() && self.current_branch().is_some() {
            self.push(false, window, cx);
            return;
        }
        self.fetch(window, cx);
        let git = self.git();
        cx.spawn_in(window, async move |this, cx| {
            let idle = async |this: &WeakEntity<Self>, cx: &mut AsyncWindowContext| loop {
                cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
                match this.read_with(cx, |v, _| v.busy.is_some()) {
                    Ok(true) => continue,
                    Ok(false) => return true,
                    Err(_) => return false,
                }
            };
            if !idle(&this, cx).await {
                return;
            }
            let g = git.clone();
            let count = |range: &'static str| {
                let g = g.clone();
                move || g.run(["rev-list", "--count", range]).ok().and_then(|n| n.trim().parse::<u32>().ok()).unwrap_or(0)
            };
            let behind_of = count("HEAD..@{upstream}");
            let behind = cx.background_executor().spawn(async move { behind_of() }).await;
            if behind > 0 {
                let _ = this.update_in(cx, |v, window, cx| v.pull(window, cx));
                if !idle(&this, cx).await {
                    return;
                }
            }
            let ahead_of = count("@{upstream}..HEAD");
            let ahead = cx.background_executor().spawn(async move { ahead_of() }).await;
            if ahead > 0 {
                let _ = this.update_in(cx, |v, window, cx| v.push(false, window, cx));
            }
        })
        .detach();
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
        if self.busy.is_none() {
            self.status.assume_stashed();
            self.change = None;
            self.change_diff = None;
            cx.notify();
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
        let local_name = if remote { target.split_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| target.clone()) } else { target.clone() };
        if self.busy.is_none() && (self.branches.iter().any(|b| !b.is_remote && b.name == local_name) || remote) {
            self.status.branch.head = Some(local_name.clone());
            for b in &mut self.branches {
                b.is_head = !b.is_remote && b.name == local_name;
            }
            cx.notify();
        }
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
        let compact = self.settings.appearance.compact_toolbar;
        let view = cx.entity();

        let branch_button = framed(Button::new("branch-switcher"), Tone::Plain, cx)
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .icon(icon(IconName::GitBranch).text_color(theme.magenta))
            .label(branch_label)
            .dropdown_caret(true)
            .tooltip(tr("toolbar.switch_branch_tip"))
            .on_click({
                let view = view.clone();
                move |_, window, cx| crate::branch_picker::open(view.clone(), crate::branch_picker::Mode::Switch, window, cx)
            });

        let bar_w = self.page_bar_width.get();
        let icons_only = bar_w > 0. && bar_w < 760.;
        let page_tab = |id: &'static str, page: Page, ic: IconName, label: SharedString, count: usize, cx: &mut Context<Self>| {
            let active = self.page == page;
            let theme = cx.theme().clone();
            let fg = if active { theme.foreground } else { theme.muted_foreground };
            let tint = if page == Page::Conflicts { theme.red } else { theme.yellow };
            let hover = theme.foreground;
            let hover_bg = theme.list_hover;
            let tip = label.clone();
            div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .gap_1p5()
                .h(px(30.))
                .px_2p5()
                .rounded(px(8.))
                .cursor_pointer()
                .text_sm()
                .whitespace_nowrap()
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(fg)
                .when(active, |d| d.bg(theme.background).border_1().border_color(theme.primary.opacity(0.45)).shadow(crate::widgets::glow(theme.primary, 6.)))
                .when(!active, |d| d.border_1().border_color(theme.transparent).hover(move |s| s.text_color(hover).bg(hover_bg)))
                .active(|s| s.opacity(crate::widgets::PRESSED))
                .child(icon(ic).small().text_color(if active { theme.primary } else { fg }))
                .when(!icons_only, |d| d.child(label))
                .when(icons_only, |d| d.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)))
                .when(count > 0, |d| d.child(count_chip(count.to_string(), tint.opacity(0.15), tint)))
                .on_click(cx.listener(move |this, _, window, cx| this.open_page(page, window, cx)))
        };
        let group = |label: SharedString, cx: &App| {
            let theme = cx.theme();
            hstack()
                .flex_none()
                .gap_0p5()
                .p(px(3.))
                .rounded(px(10.))
                .bg(theme.secondary.opacity(0.6))
                .border_1()
                .border_color(theme.border.opacity(0.6))
                .when(!icons_only, |d| d.child(div().px_1p5().text_size(px(10.)).font_weight(FontWeight::BOLD).text_color(theme.muted_foreground.opacity(0.8)).child(label.to_uppercase())))
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
                    .child({
                        let b = framed(Button::new("undo"), Tone::Plain, cx).icon(icon(IconName::Undo2)).tooltip(tr("toolbar.undo_tip")).on_click(cx.listener(|this, _, window, cx| this.open_undo(window, cx)));
                        if compact { b.px(px(8.)) } else { b.label(tr("toolbar.undo")) }
                    })
                    .when(self.settings.ai.enabled, |d| {
                        let b = framed(Button::new("ask-ai"), Tone::Ai, cx).icon(icon(IconName::Sparkles)).tooltip(tr("toolbar.ask_ai_tip")).on_click(cx.listener(|_, _, window, cx| crate::assist::open(cx.entity(), window, cx)));
                        d.child(if compact { b.px(px(8.)) } else { b.label(tr("toolbar.ask_ai")) })
                    })
                    .child({
                        let unpublished = self.status.branch.upstream.is_none() && !self.remotes.is_empty() && self.current_branch().is_some();
                        let pending = ahead > 0 || behind > 0 || unpublished;
                        let tone = if pending { Tone::Accent } else { Tone::Plain };
                        let chip_fg = if pending { theme.primary_foreground } else { theme.muted_foreground };
                        let tip = if unpublished { tr("toolbar.sync_publish_tip") } else if pending { t!("toolbar.sync_pending_tip", down = behind, up = ahead).to_string().into() } else { tr("toolbar.sync_tip") };
                        let sync = crate::widgets::framed_if(Button::new("sync"), tone, busy.is_none(), cx)
                            .icon(icon(IconName::RefreshCw))
                            .when(!compact, |b| b.label(if unpublished { tr("toolbar.publish") } else { tr("toolbar.sync") }))
                            .when(behind > 0, |b| b.child(count_chip(format!("↓{behind}"), gpui_kit::black().opacity(0.18), chip_fg)))
                            .when(ahead > 0, |b| b.child(count_chip(format!("↑{ahead}"), gpui_kit::black().opacity(0.18), chip_fg)))
                            .tooltip(tip)
                            .rounded_r(px(0.))
                            .on_click(cx.listener(|this, _, window, cx| this.sync(window, cx)));
                        let caret = framed(Button::new("sync-menu"), tone, cx)
                            .px(px(6.))
                            .rounded_l(px(0.))
                            .border_l_0()
                            .icon(icon(IconName::ChevronDown).xsmall())
                            .tooltip(tr("toolbar.sync_menu_tip"))
                            .dropdown_menu({
                                let view = view.clone();
                                move |menu, _, _| {
                                    let (a, b, c, d) = (view.clone(), view.clone(), view.clone(), view.clone());
                                    menu.item(PopupMenuItem::new(t!("toolbar.fetch").to_string()).icon(icon(IconName::RefreshCw)).on_click(move |_, window, cx| a.update(cx, |v, cx| v.fetch(window, cx))))
                                        .item(PopupMenuItem::new(t!("toolbar.pull").to_string()).icon(icon(IconName::ArrowDownToLine)).on_click(move |_, window, cx| b.update(cx, |v, cx| v.pull(window, cx))))
                                        .item(PopupMenuItem::new(t!("toolbar.push").to_string()).icon(icon(IconName::ArrowUpToLine)).on_click(move |_, window, cx| c.update(cx, |v, cx| v.push(false, window, cx))))
                                        .separator()
                                        .item(PopupMenuItem::new(t!("toolbar.force_push").to_string()).icon(icon(IconName::TriangleAlert)).on_click(move |_, window, cx| crate::dialogs::confirm_force_push(d.clone(), window, cx)))
                                }
                            });
                        h_flex().child(sync).child(caret)
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
            .child({
                let measured = self.page_bar_width.clone();
                let me = cx.entity_id();
                let tool = Page::TOOLS.iter().find(|(p, ..)| *p == self.page).copied();
                let tools_label: SharedString = tool.map(|(_, l, ..)| tr(l)).unwrap_or_else(|| tr("page_bar.tools"));
                let tools_icon = tool.map(|(_, _, ic, _)| ic).unwrap_or(IconName::Wrench);
                let current_page = self.page;
                let tools_view = view.clone();
                let tools = gpui_kit::component::popover::Popover::new("page-tools")
                    .anchor(Anchor::TopRight)
                    .when(cfg!(debug_assertions) && std::env::var("CAIRN_DEBUG_TOOLS").is_ok(), |p| p.open(true))
                    .trigger(
                        Button::new("page-tools-btn")
                            .ghost()
                            .h(px(30.))
                            .px_2p5()
                            .rounded(px(8.))
                            .text_sm()
                            .icon(icon(tools_icon).small())
                            .when(tool.is_some(), |b| b.font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).border_1().border_color(theme.primary.opacity(0.45)).bg(theme.background))
                            .when(tool.is_none(), |b| b.font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground))
                            .when(!icons_only || tool.is_some(), |b| b.label(tools_label))
                            .when(conflict_count > 0 && tool.is_none(), |b| b.child(count_chip(conflict_count.to_string(), theme.red.opacity(0.15), theme.red)))
                            .dropdown_caret(true),
                    )
                    .content(move |_, _, cx| {
                        let theme = cx.theme().clone();
                        let mut grid = h_flex().flex_wrap().gap_2().w(px(560.));
                        for (page, label, ic, desc) in Page::TOOLS {
                            let on = current_page == page;
                            let v = tools_view.clone();
                            let tint = if page == Page::Conflicts && conflict_count > 0 { theme.red } else { theme.primary };
                            grid = grid.child(
                                h_flex()
                                    .id(SharedString::from(format!("tool-{label}")))
                                    .w(px(274.))
                                    .gap_2p5()
                                    .p_2p5()
                                    .rounded(px(10.))
                                    .cursor_pointer()
                                    .border_1()
                                    .border_color(if on { tint.opacity(0.55) } else { theme.border.opacity(0.6) })
                                    .when(on, |d| d.bg(tint.opacity(0.1)))
                                    .hover(|s| s.bg(theme.list_hover).border_color(tint.opacity(0.45)))
                                    .active(|s| s.opacity(crate::widgets::PRESSED))
                                    .child(div().flex_none().size(px(32.)).rounded(px(9.)).flex().items_center().justify_center().bg(tint.opacity(0.14)).child(icon(ic).small().text_color(tint)))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(hstack().gap_1p5().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr(label))).when(page == Page::Conflicts && conflict_count > 0, |d| d.child(count_chip(conflict_count.to_string(), theme.red.opacity(0.15), theme.red))))
                                            .child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(tr(desc))),
                                    )
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        v.update(cx, |v, cx| v.open_page(page, window, cx));
                                        cx.emit(DismissEvent);
                                    })),
                            );
                        }
                        v_flex().gap_2().p_1().child(div().text_xs().font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(tr("page_bar.tools_title").to_uppercase())).child(grid)
                    });
                hstack()
                    .relative()
                    .gap_2()
                    .px_3()
                    .py(px(6.))
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                let w = f32::from(bounds.size.width);
                                if (measured.get() - w).abs() > 0.5 {
                                    measured.set(w);
                                    cx.notify(me);
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        group(tr("page_bar.work"), cx)
                            .child(page_tab("page-history", Page::History, IconName::GitCommitHorizontal, tr("page.history"), 0, cx))
                            .child(page_tab("page-changes", Page::Changes, IconName::FilePen, tr("page.changes"), change_count, cx))
                            .child(page_tab("page-branches", Page::Branches, IconName::GitBranch, tr("page.branches"), 0, cx))
                            .child(page_tab("page-stashes", Page::Stashes, IconName::Archive, tr("page.stashes"), self.stashes.len(), cx)),
                    )
                    .child(
                        group(tr("page_bar.review"), cx)
                            .child(page_tab("page-pulls", Page::PullRequests, IconName::GitPullRequest, tr("page.pulls"), self.prs.len(), cx))
                            .child(page_tab("page-stack", Page::Stack, IconName::Layers, tr("page.stack"), 0, cx)),
                    )
                    .when(conflict_count > 0, |d| d.child(page_tab("page-conflicts", Page::Conflicts, IconName::TriangleAlert, tr("page.conflicts"), conflict_count, cx)))
                    .child(spacer())
                    .child(tools)
            })
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

impl RepoView {
    /// The thin bar along the bottom: branch and sync state, last fetch, changes, stashes,
    /// warnings and what is running in the background.
    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let item = |ic: IconName, text: SharedString, color: Hsla| hstack().flex_none().gap_1().text_color(color).child(icon(ic).xsmall()).child(text);
        let fetched = std::fs::metadata(self.repo.git_dir.join("FETCH_HEAD"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| t!("status_bar.fetched", when = crate::widgets::relative_time(d.as_secs() as i64).to_string()).to_string())
            .unwrap_or_else(|| t!("status_bar.never_fetched").to_string());
        let ahead = self.status.branch.ahead;
        let behind = self.status.branch.behind;
        let sync_text: SharedString = match (self.status.branch.upstream.as_ref(), ahead, behind) {
            (None, _, _) => tr("status_bar.no_upstream"),
            (Some(_), 0, 0) => tr("status_bar.in_sync"),
            (Some(_), a, b) => format!("↑{a} ↓{b}").into(),
        };
        let changes = self.status.files.len();
        let clash = self.current_branch().is_some_and(|b| self.clashes.contains_key(b));
        let muted = theme.muted_foreground;
        h_flex()
            .flex_none()
            .h(px(26.))
            .px_3()
            .gap_4()
            .items_center()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .text_size(px(11.5))
            .text_color(muted)
            .child(item(IconName::GitBranch, self.current_branch().map(str::to_string).unwrap_or_else(|| t!("toolbar.detached").to_string()).into(), theme.foreground).font_family(theme.mono_font_family.clone()))
            .child(item(IconName::ArrowDownUp, sync_text, if ahead + behind > 0 { theme.primary } else { muted }))
            .child(item(IconName::Clock, fetched.into(), muted))
            .child(item(IconName::FilePen, crate::widgets::plural("status_bar.changes", changes).into(), if changes > 0 { theme.yellow } else { muted }))
            .when(!self.stashes.is_empty(), |d| d.child(item(IconName::Archive, crate::widgets::plural("status_bar.stashes", self.stashes.len()).into(), muted)))
            .when(clash, |d| d.child(item(IconName::TriangleAlert, t!("conflicts_early.with", base = self.trunk.clone().unwrap_or_else(|| "main".into())).to_string().into(), theme.red)))
            .child(div().flex_1())
            .when_some(self.busy.clone(), |d, b| d.child(hstack().gap_1p5().child(gpui_kit::component::spinner::Spinner::new().xsmall()).child(b)))
            .when(self.loading_more, |d| d.child(tr("history.loading_more")))
            .child(crate::widgets::plural("status_bar.commits", self.commits.len()))
    }
}

impl Focusable for RepoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RepoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.wake(cx);
        if let Some(text) = self.pending_notice.take() {
            window.push_notification(Notification::info(text).title(self.name.clone()).in_app_and_system(), cx);
        }
        let forced = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_SKELETON").ok() } else { None };
        let body = if let Some(kind) = forced {
            match kind.as_str() {
                "changes" => crate::skeleton::changes(cx).into_any_element(),
                "pulls" => h_flex()
                    .size_full()
                    .child(div().flex_none().w(px(340.)).h_full().bg(cx.theme().sidebar).border_r_1().border_color(cx.theme().border).child(crate::skeleton::pr_list(5, cx)))
                    .child(div().flex_1().min_w_0().h_full().child(crate::skeleton::pr_detail(cx)))
                    .into_any_element(),
                _ => crate::skeleton::history(self.settings.appearance.show_avatars, cx).into_any_element(),
            }
        } else if !self.loaded_once {
            match self.page {
                Page::Changes => crate::skeleton::changes(cx).into_any_element(),
                _ => crate::skeleton::history(self.settings.appearance.show_avatars, cx).into_any_element(),
            }
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
            .relative()
            .size_full()
            .key_context("RepoView")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.handle_key(ev, window, cx)))
            .on_action(cx.listener(|this, _: &crate::palette::UndoHistory, window, cx| this.open_undo(window, cx)))
            .child(self.render_toolbar(window, cx))
            .child({
                let page = div().size_full().child(body);
                if self.settings.appearance.reduce_motion {
                    div().flex_1().min_h_0().child(page).into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(page.with_animation(SharedString::from(format!("page-{:?}", self.page)), Animation::new(std::time::Duration::from_millis(160)).with_easing(ease_out_quint()), |d, t| d.opacity(0.35 + 0.65 * t)))
                        .into_any_element()
                }
            })
            .child(self.render_status_bar(cx))
            .children(self.render_live_op(cx))
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
