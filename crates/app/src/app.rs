use std::path::{Path, PathBuf};

use cairn_core::library::{Library, RepoEntry};
use cairn_core::settings::{Settings, StartupMode};
use cairn_core::Repository;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;
use uuid::Uuid;

use crate::repo_view::RepoView;
use crate::widgets::{count_chip, empty_state, hstack, icon, section_label, spacer, tr};

pub(crate) struct CairnApp {
    pub library: Library,
    pub settings: Settings,
    pub tabs: Vec<Entity<RepoView>>,
    pub active: Option<usize>,
    pub open_category: Option<Option<Uuid>>,
    pub sidebar_open: bool,
    pub update_ready: Option<(String, std::sync::Arc<velopack::UpdateInfo>)>,
    workspace: Option<(Uuid, Entity<crate::workspace_view::WorkspacePanel>)>,
    side_toggled: std::collections::HashSet<String>,
    focus: FocusHandle,
    pub(crate) home: crate::home::HomeData,
}

fn read_branch(path: &Path) -> Option<String> {
    let head = path.join(".git").join("HEAD");
    let text = std::fs::read_to_string(&head).ok().or_else(|| {
        let gitfile = std::fs::read_to_string(path.join(".git")).ok()?;
        let dir = gitfile.trim().strip_prefix("gitdir: ")?;
        std::fs::read_to_string(Path::new(dir).join("HEAD")).ok()
    })?;
    let text = text.trim();
    Some(text.strip_prefix("ref: refs/heads/").map(str::to_string).unwrap_or_else(|| text.chars().take(7).collect()))
}

impl CairnApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = Library::load();
        let settings = Settings::load();
        let mut app = Self {
            library,
            settings,
            tabs: Vec::new(),
            active: None,
            open_category: None,
            sidebar_open: true,
            update_ready: None,
            workspace: None,
            side_toggled: if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_SIDE").map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default() } else { Default::default() },
            focus: cx.focus_handle(),
            home: Default::default(),
        };
        cairn_core::git::set_command_logging(app.settings.advanced.log_git_commands);
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            crate::window_state::capture(window, cx);
            let Some(app) = weak.upgrade() else { return true };
            let (confirm, busy) = {
                let a = app.read(cx);
                (a.settings.general.confirm_quit_during_operation, a.tabs.iter().any(|t| t.read(cx).busy.is_some()))
            };
            if !(confirm && busy) {
                return true;
            }
            window.open_alert_dialog(cx, |alert, _, _| {
                alert
                    .confirm()
                    .title(tr("quit.title"))
                    .description(tr("quit.body"))
                    .ok_text(tr("quit.ok"))
                    .ok_variant(ButtonVariant::Danger)
                    .on_ok(|_, _, cx| {
                        cx.quit();
                        true
                    })
            });
            false
        });
        crate::updater::start(&mut app, window, cx);
        crate::whats_new::show_after_update(&mut app, window, cx);
        cx.on_app_quit(|this, _| {
            let ready = this.update_ready.clone();
            async move {
                if let Some((_, info)) = ready {
                    crate::updater::apply_on_exit(&info);
                }
            }
        })
        .detach();
        if app.settings.general.startup == StartupMode::RestoreTabs {
            let tabs = app.library.open_tabs.clone();
            let active = app.library.active_tab.clone();
            for path in tabs {
                app.open_repo_inner(&path, false, window, cx);
            }
            if let Some(active) = active
                && let Some(ix) = app.tabs.iter().position(|t| same(&t.read(cx).repo.root, &active))
            {
                app.active = Some(ix);
            }
        }
        #[cfg(debug_assertions)]
        cx.defer_in(window, |this, window, cx| this.debug_hooks(window, cx));
        app
    }

    #[cfg(debug_assertions)]
    fn debug_hooks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(path) = std::env::var("CAIRN_DEBUG_OPEN") {
            self.open_repo(Path::new(&path), window, cx);
        }
        if std::env::var("CAIRN_DEBUG_HOME").is_ok() {
            self.go_home(None, cx);
        }
        if let Ok(done) = std::env::var("CAIRN_DEBUG_LIVE")
            && let Some(tab) = self.active_tab()
        {
            tab.update(cx, |v, cx| {
                v.live_op = Some(crate::live_op::LiveOp { label: tr("op.push"), phase: "Writing objects".into(), done: done.parse().unwrap_or(0.62), finished: None });
                v.live_cancel = Some(cairn_core::git::CancelHandle::default());
                cx.notify();
            });
        }
        if let Ok(page) = std::env::var("CAIRN_DEBUG_PAGE")
            && let Some(tab) = self.active.and_then(|i| self.tabs.get(i)).cloned()
        {
            tab.update(cx, |v, cx| v.debug_page(&page, cx));
        }
        if let Ok(section) = std::env::var("CAIRN_DEBUG_SETTINGS") {
            if section != "1" {
                crate::settings_dialog::request_category(Box::leak(section.into_boxed_str()));
            }
            crate::settings_dialog::open(cx.entity(), self.settings.clone(), window, cx);
        }
        match std::env::var("CAIRN_DEBUG_DIALOG").as_deref() {
            Ok("palette") => crate::palette::open(cx.entity(), window, cx),
            Ok("whats_new") => crate::whats_new::debug_open(window, cx),
            Ok("assist") => {
                if let Some(tab) = self.active_tab() {
                    crate::assist::open(tab, window, cx);
                }
            }
            Ok("repo_settings") => {
                if let Some(tab) = self.active_tab() {
                    crate::repo_settings::open(tab, window, cx);
                }
            }
            Ok("new_pr") => {
                if let Some(tab) = self.active_tab() {
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                        let _ = cx.update(|window, cx| crate::new_pr::open(tab, window, cx));
                    })
                    .detach();
                }
            }
            Ok("issue_branch") => {
                if let Some(tab) = self.active_tab() {
                    crate::issue_branch::open(tab, window, cx);
                }
            }
            Ok(kind @ ("force_push" | "reset_hard")) => {
                let kind = kind.to_string();
                if let Some(tab) = self.active_tab() {
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                        let _ = cx.update(|window, cx| {
                            if kind == "force_push" {
                                crate::dialogs::confirm_force_push(tab, window, cx);
                            } else if let Some(id) = tab.read(cx).commits.get(3).map(|c| c.id.clone()) {
                                crate::dialogs::confirm_reset(tab, id, cairn_core::ops::ResetMode::Hard, window, cx);
                            }
                        });
                    })
                    .detach();
                }
            }
            Ok(kind @ ("commit_all" | "commit_trunk")) => {
                let kind = kind.to_string();
                if let Some(tab) = self.active_tab() {
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                        let _ = cx.update(|window, cx| {
                            tab.update(cx, |v, cx| v.summary.update(cx, |s, cx| s.set_value("feat(ui): city search box", window, cx)));
                            let plan = crate::changes::CommitPlan::default();
                            if kind == "commit_all" {
                                crate::commit_guard::ask_commit_all(tab, plan, 3, window, cx);
                            } else {
                                crate::commit_guard::ask_on_trunk(tab, plan, window, cx);
                            }
                        });
                    })
                    .detach();
                }
            }
            Ok(kind @ ("sync_confirm" | "sync_rewritten")) => {
                let kind = kind.to_string();
                if let Some(tab) = self.active_tab() {
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                        let plan = cairn_core::ops::SyncPlan { ahead: 2, behind: 3, rewritten: 2 };
                        let _ = cx.update(|window, cx| {
                            if kind == "sync_confirm" {
                                crate::sync::confirm_sync(tab, plan, window, cx);
                            } else {
                                crate::sync::warn_rewritten(tab, plan, window, cx);
                            }
                        });
                    })
                    .detach();
                }
            }
            Ok("branch_picker") => {
                if let Some(tab) = self.active_tab() {
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
                        let mode = if std::env::var("CAIRN_DEBUG_PICKER_HIGHLIGHT").is_ok() { crate::branch_picker::Mode::Highlight } else { crate::branch_picker::Mode::Switch };
                        let _ = cx.update(|window, cx| crate::branch_picker::open(tab, mode, window, cx));
                    })
                    .detach();
                }
            }
            Ok("goto_file") => {
                if let Some(tab) = self.active_tab() {
                    crate::goto_file::open(tab, window, cx);
                }
            }
            Ok("workspace") => {
                self.active = None;
                self.open_category = self.library.categories.first().map(|c| Some(c.id));
            }
            _ => {}
        }
    }

    pub(crate) fn active_tab(&self) -> Option<Entity<RepoView>> {
        self.active.and_then(|i| self.tabs.get(i)).cloned()
    }

    fn with_tab(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut RepoView, &mut Window, &mut Context<RepoView>)) {
        if let Some(tab) = self.active_tab() {
            tab.update(cx, |v, cx| f(v, window, cx));
        }
    }

    pub(crate) fn save_library(&mut self, cx: &mut Context<Self>) {
        self.library.open_tabs = self.tabs.iter().map(|t| t.read(cx).repo.root.clone()).collect();
        self.library.active_tab = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.read(cx).repo.root.clone());
        if let Err(e) = self.library.save() {
            crate::repo_view::log_error(&format!("saving library: {e}"));
        }
    }

    pub(crate) fn apply_settings(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        let language_changed = settings.general.language != self.settings.general.language;
        cairn_core::git::set_command_logging(settings.advanced.log_git_commands);
        self.settings = settings.clone();
        if let Err(e) = settings.save() {
            window.push_notification(Notification::error(e.to_string()), cx);
        }
        if language_changed {
            rust_i18n::set_locale(settings.general.language.code());
            gpui_kit::component::set_locale(settings.general.language.code());
        }
        crate::theme::apply(&settings, Some(window), cx);
        for tab in &self.tabs {
            let s = settings.clone();
            tab.update(cx, |v, cx| v.update_settings(s, cx));
        }
        cx.notify();
    }

    fn open_repo_inner(&mut self, path: &Path, notify_errors: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(ix) = self.tabs.iter().position(|t| same(&t.read(cx).repo.root, path)) {
            self.active = Some(ix);
            return true;
        }
        match Repository::open(path) {
            Ok(repo) => {
                if self.library.repo(&repo.root).is_none() {
                    self.library.add_repo(&repo.root, None);
                }
                self.library.open_tab(&repo.root);
                let settings = self.settings.clone();
                let view = cx.new(|cx| RepoView::new(repo, settings, window, cx));
                cx.subscribe_in(&view, window, |this, _, ev: &crate::repo_view::OpenRepoEvent, window, cx| {
                    let path = ev.0.clone();
                    let task = cx.spawn_in(window, async move |this, cx| {
                        for _ in 0..40 {
                            if path.join(".git").exists() {
                                break;
                            }
                            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
                        }
                        let _ = this.update_in(cx, |this, window, cx| this.open_repo(&path, window, cx));
                    });
                    task.detach();
                    let _ = this;
                })
                .detach();
                cx.subscribe(&view, |this, _, ev: &crate::repo_view::PanelWidthEvent, cx| {
                    if let Some(w) = ev.details {
                        this.settings.appearance.details_width = w;
                    }
                    if let Some(w) = ev.list {
                        this.settings.appearance.list_width = w;
                    }
                    let s = this.settings.clone();
                    for tab in &this.tabs {
                        tab.update(cx, |v, _| {
                            v.settings.appearance.details_width = s.appearance.details_width;
                            v.settings.appearance.list_width = s.appearance.list_width;
                        });
                    }
                    if let Err(e) = this.settings.save() {
                        crate::repo_view::log_error(&format!("saving settings: {e}"));
                    }
                })
                .detach();
                cx.subscribe(&view, |this, _, ev: &crate::repo_view::HistoryScopeEvent, _| {
                    this.settings.graph.all_branches = ev.0;
                    if let Err(e) = this.settings.save() {
                        crate::repo_view::log_error(&format!("saving settings: {e}"));
                    }
                })
                .detach();
                cx.subscribe(&view, |this, _, ev: &crate::repo_view::DiffPrefsEvent, cx| {
                    this.settings.diffs = ev.0.clone();
                    if let Err(e) = this.settings.save() {
                        crate::repo_view::log_error(&format!("saving settings: {e}"));
                    }
                    let diffs = ev.0.clone();
                    for tab in this.tabs.clone() {
                        tab.update(cx, |v, cx| {
                            v.settings.diffs = diffs.clone();
                            cx.notify();
                        });
                    }
                })
                .detach();
                cx.subscribe(&view, |this, _, ev: &crate::repo_view::PrefsEvent, cx| {
                    (ev.0)(&mut this.settings);
                    if let Err(e) = this.settings.save() {
                        crate::repo_view::log_error(&format!("saving settings: {e}"));
                    }
                    for tab in this.tabs.clone() {
                        let change = ev.0.clone();
                        tab.update(cx, |v, cx| {
                            change(&mut v.settings);
                            cx.notify();
                        });
                    }
                })
                .detach();
                self.tabs.push(view);
                self.active = Some(self.tabs.len() - 1);
                true
            }
            Err(e) => {
                if notify_errors {
                    window.push_notification(
                        Notification::error(crate::repo_view::friendly_error(&e)).title(tr("repo.open_failed")),
                        cx,
                    );
                }
                false
            }
        }
    }

    pub(crate) fn open_repo(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_repo_inner(path, true, window, cx) {
            self.save_library(cx);
        }
        cx.notify();
    }

    fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);
        self.active = match self.active {
            Some(a) if a == ix => {
                if self.tabs.is_empty() {
                    None
                } else {
                    Some(ix.saturating_sub(1).min(self.tabs.len() - 1))
                }
            }
            Some(a) if a > ix => Some(a - 1),
            other => other,
        };
        self.save_library(cx);
        cx.notify();
    }

    fn go_home(&mut self, category: Option<Option<Uuid>>, cx: &mut Context<Self>) {
        self.active = None;
        self.open_category = category;
        cx.notify();
    }

    /// Opens a repository on one of its pull requests.
    ///
    /// # Arguments
    /// * `path` - the repository
    /// * `number` - the pull request
    pub(crate) fn open_pr_in(&mut self, path: &Path, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.open_repo(path, window, cx);
        if let Some(tab) = self.active_tab() {
            tab.update(cx, |v, cx| {
                v.page = crate::repo_view::Page::PullRequests;
                v.pulls.selected = Some(number);
                v.pulls.loaded = false;
                v.load_pulls(cx);
            });
        }
    }

    pub(crate) fn add_repositories(&mut self, category: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some(tr("repo.add_prompt")),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                let mut added = 0;
                for path in paths {
                    let found = Library::scan_folder(&path);
                    if found.is_empty() {
                        window.push_notification(
                            Notification::warning(t!("repo.not_a_repo", path = path.display().to_string()).to_string()),
                            cx,
                        );
                    }
                    for repo in found {
                        let root = Repository::open(&repo).map(|r| r.root).unwrap_or(repo);
                        this.library.add_repo(&root, category);
                        added += 1;
                    }
                }
                if added > 0 {
                    window.push_notification(Notification::success(crate::widgets::plural("repo.added", added)), cx);
                }
                this.save_library(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn clone_repository(&mut self, category: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let base = self.settings.general.clone_dir.clone();
        crate::dialogs::text_prompt(
            tr("repo.clone_title"),
            t!("repo.clone_body", dir = base.clone()).to_string().into(),
            "https://github.com/owner/repo.git".into(),
            String::new(),
            tr("repo.clone"),
            window,
            cx,
            move |url, window, cx| {
                let name = url
                    .trim_end_matches('/')
                    .rsplit(['/', ':'])
                    .next()
                    .unwrap_or("repo")
                    .trim_end_matches(".git")
                    .to_string();
                let dest = PathBuf::from(&base).join(&name);
                window.push_notification(Notification::info(t!("repo.cloning", name = name.clone()).to_string()), cx);
                let view = view.clone();
                window
                    .spawn(cx, async move |cx| {
                        let dest2 = dest.clone();
                        let result = cx
                            .background_executor()
                            .spawn(async move { Repository::clone(&url, &dest2) })
                            .await;
                        let _ = view.update_in(cx, |this, window, cx| match result {
                            Ok(repo) => {
                                this.library.add_repo(&repo.root, category);
                                this.open_repo(&repo.root, window, cx);
                            }
                            Err(e) => window.push_notification(
                                Notification::error(crate::repo_view::friendly_error(&e)).title(tr("repo.clone_failed")),
                                cx,
                            ),
                        });
                    })
                    .detach();
            },
        );
    }

    pub(crate) fn init_repository(&mut self, category: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("repo.init_prompt")),
        });
        let branch = self.settings.git.default_branch.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |this, window, cx| match Repository::init(&path, &branch) {
                Ok(repo) => {
                    this.library.add_repo(&repo.root, category);
                    this.open_repo(&repo.root, window, cx);
                }
                Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)), cx),
            });
        })
        .detach();
    }

    pub(crate) fn new_category(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        crate::dialogs::text_prompt(
            tr("category.new_title"),
            tr("category.new_body"),
            tr("category.name_placeholder"),
            String::new(),
            tr("category.create"),
            window,
            cx,
            move |name, _, cx| {
                view.update(cx, |this, cx| {
                    let id = this.library.create_category(&name);
                    this.open_category = Some(Some(id));
                    this.active = None;
                    this.save_library(cx);
                    cx.notify();
                });
            },
        );
    }

    fn rename_category(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.library.categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
        let view = cx.entity();
        crate::dialogs::text_prompt(
            tr("category.rename_title"),
            SharedString::default(),
            tr("category.name_placeholder"),
            current,
            tr("dialog.save"),
            window,
            cx,
            move |name, _, cx| {
                view.update(cx, |this, cx| {
                    this.library.rename_category(id, &name);
                    this.save_library(cx);
                    cx.notify();
                });
            },
        );
    }

    fn delete_category(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.library.categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title(t!("category.delete_title", name = name.clone()).to_string())
                .description(tr("category.delete_body"))
                .ok_text(tr("category.delete"))
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.library.delete_category(id);
                        if this.open_category == Some(Some(id)) {
                            this.open_category = None;
                        }
                        this.save_library(cx);
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn remove_repo(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.library.repo(&path).map(|r| r.name.clone()).unwrap_or_default();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let path = path.clone();
            alert
                .confirm()
                .title(t!("repo.remove_title", name = name.clone()).to_string())
                .description(tr("repo.remove_body"))
                .ok_text(tr("repo.remove"))
                .on_ok(move |_, _, cx| {
                    let path = path.clone();
                    view.update(cx, |this, cx| {
                        if let Some(ix) = this.tabs.iter().position(|t| same(&t.read(cx).repo.root, &path)) {
                            this.close_tab(ix, cx);
                        }
                        this.library.remove_repo(&path);
                        this.save_library(cx);
                        cx.notify();
                    });
                    true
                })
        });
    }

    fn rename_repo(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.library.repo(&path).map(|r| r.name.clone()).unwrap_or_default();
        let view = cx.entity();
        crate::dialogs::text_prompt(
            tr("repo.rename_title"),
            tr("repo.rename_body"),
            tr("repo.name_placeholder"),
            current,
            tr("dialog.save"),
            window,
            cx,
            move |name, _, cx| {
                let path = path.clone();
                view.update(cx, |this, cx| {
                    this.library.rename_repo(&path, &name);
                    this.save_library(cx);
                    cx.notify();
                });
            },
        );
    }

    fn repo_menu(&self, menu: PopupMenu, entry: &RepoEntry, view: Entity<Self>) -> PopupMenu {
        let path = entry.path.clone();
        let mut move_menu = menu;
        let (v_open, p_open) = (view.clone(), path.clone());
        move_menu = move_menu.item(PopupMenuItem::new(t!("repo.open").to_string()).icon(icon(IconName::FolderOpen)).on_click(move |_, window, cx| {
            let p = p_open.clone();
            v_open.update(cx, |this, cx| this.open_repo(&p, window, cx));
        }));
        let reveal = path.clone();
        move_menu = move_menu.item(
            PopupMenuItem::new(t!("repo.reveal").to_string())
                .icon(icon(IconName::ExternalLink))
                .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
        );
        move_menu = move_menu.separator().label(t!("repo.move_to").to_string());
        for c in &self.library.categories {
            let (v, p, id) = (view.clone(), path.clone(), c.id);
            move_menu = move_menu.item(
                PopupMenuItem::new(c.name.clone())
                    .icon(icon(IconName::Folder))
                    .checked(entry.category == Some(id))
                    .on_click(move |_, _, cx| {
                        let p = p.clone();
                        v.update(cx, |this, cx| {
                            this.library.set_repo_category(&p, Some(id));
                            this.save_library(cx);
                            cx.notify();
                        });
                    }),
            );
        }
        let (v_none, p_none) = (view.clone(), path.clone());
        move_menu = move_menu.item(
            PopupMenuItem::new(t!("repo.no_category").to_string())
                .checked(entry.category.is_none())
                .on_click(move |_, _, cx| {
                    let p = p_none.clone();
                    v_none.update(cx, |this, cx| {
                        this.library.set_repo_category(&p, None);
                        this.save_library(cx);
                        cx.notify();
                    });
                }),
        );
        let (v_ren, p_ren) = (view.clone(), path.clone());
        let (v_rm, p_rm) = (view.clone(), path.clone());
        move_menu
            .separator()
            .item(PopupMenuItem::new(t!("repo.rename").to_string()).icon(icon(IconName::Pencil)).on_click(move |_, window, cx| {
                let p = p_ren.clone();
                v_ren.update(cx, |this, cx| this.rename_repo(p, window, cx));
            }))
            .item(PopupMenuItem::new(t!("repo.remove").to_string()).icon(icon(IconName::Trash)).on_click(move |_, window, cx| {
                let p = p_rm.clone();
                v_rm.update(cx, |this, cx| this.remove_repo(p, window, cx));
            }))
    }

    fn render_sidebar_repo(&self, entry: &RepoEntry, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let active_path = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.read(cx).repo.root.clone());
        let is_active = active_path.as_deref().is_some_and(|a| same(a, &entry.path));
        let is_open = self.tabs.iter().any(|t| same(&t.read(cx).repo.root, &entry.path));
        let missing = !entry.path.exists();
        let path = entry.path.clone();
        let branch = read_branch(&entry.path).unwrap_or_default();
        let view = cx.entity();
        let entry_menu = entry.clone();
        let (changed, ahead, conflicts) = self
            .tabs
            .iter()
            .find(|t| same(&t.read(cx).repo.root, &entry.path))
            .map(|t| {
                let s = &t.read(cx).status;
                (s.files.len(), s.branch.ahead as usize, s.conflicted().count())
            })
            .unwrap_or_default();
        div()
            .id(SharedString::from(format!("side-repo-{}", entry.path.display())))
            .flex()
            .items_center()
            .gap(px(10.))
            .pl_5()
            .pr_2()
            .py(px(6.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(is_active, |d| d.bg(theme.list_active))
            .when(!is_active, |d| d.hover(|s| s.bg(cx.theme().list_hover)))
            .child(icon(IconName::FolderGit2).small().text_color(if is_open { theme.primary } else { theme.muted_foreground }))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().text_color(if missing { theme.muted_foreground } else { theme.foreground }).child(entry.name.clone()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.muted_foreground)
                            .child(if missing { t!("repo.missing").to_string() } else { branch }),
                    ),
            )
            .when(conflicts > 0, |d| d.child(count_chip(conflicts.to_string(), theme.red.opacity(0.15), theme.red)))
            .when(changed > 0 && conflicts == 0, |d| d.child(count_chip(changed.to_string(), theme.yellow.opacity(0.15), theme.yellow)))
            .when(ahead > 0, |d| d.child(count_chip(format!("↑{ahead}"), theme.primary.opacity(0.15), theme.primary)))
            .on_click(cx.listener(move |this, _, window, cx| this.open_repo(&path, window, cx)))
            .context_menu(move |menu, _, cx| view.read(cx).repo_menu(menu, &entry_menu, view.clone()))
            .into_any_element()
    }

    fn render_repo_sections(&self, tab: Entity<RepoView>, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (branches, worktrees, stashes) = {
            let v = tab.read(cx);
            let local: Vec<cairn_core::refs::Branch> = v.branches.iter().filter(|b| !b.is_remote).cloned().collect();
            let worktrees: Vec<cairn_core::refs::Worktree> = v.worktrees.iter().filter(|w| !w.is_main).cloned().collect();
            (local, worktrees, v.stashes.clone())
        };
        let toggled = self.side_toggled.clone();
        let is_open = |key: &str, default_open: bool| default_open != toggled.contains(key);
        let row = |id: SharedString, indent: bool, cx: &App| {
            let hover = cx.theme().list_hover;
            div().id(id).flex().items_center().gap(px(8.)).when(indent, |d| d.pl(px(22.))).when(!indent, |d| d.pl_2()).pr_2().h(px(26.)).rounded(px(6.)).cursor_pointer().hover(move |s| s.bg(hover))
        };
        let header = |key: &'static str, label: SharedString, count: usize, open: bool, cx: &mut Context<Self>| {
            let theme = cx.theme().clone();
            div()
                .id(key)
                .flex()
                .items_center()
                .gap_1p5()
                .px_2()
                .h(px(26.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.list_hover))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())
                .child(div().flex_1().child(label.to_uppercase()))
                .child(count_chip(count.to_string(), theme.secondary, theme.muted_foreground))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.side_toggled.remove(key) {
                        this.side_toggled.insert(key.to_string());
                    }
                    cx.notify();
                }))
        };
        let branch_meta = |b: &cairn_core::refs::Branch| match (b.ahead, b.behind) {
            (0, 0) => crate::widgets::relative_time(b.last_commit_time).to_string(),
            (a, 0) => format!("↑{a}"),
            (0, r) => format!("↓{r}"),
            (a, r) => format!("↑{a} ↓{r}"),
        };
        let branch_row = |b: &cairn_core::refs::Branch, label: String, indent: bool, cx: &mut Context<Self>| {
            let theme = cx.theme().clone();
            let name = b.name.clone();
            let t = tab.clone();
            let lane = tab.read(cx).branch_color(&b.name, cx);
            row(SharedString::from(format!("side-branch-{}", b.name)), indent, cx)
                .when(b.is_head, |d| d.bg(theme.list_active))
                .child(icon(IconName::GitBranch).xsmall().text_color(lane.unwrap_or(if b.is_head { theme.magenta } else { theme.muted_foreground })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .font_weight(if b.is_head { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(if b.is_head { theme.foreground } else { theme.muted_foreground })
                        .child(label),
                )
                .when(tab.read(cx).clashes.contains_key(&b.name), |d| d.child(icon(IconName::TriangleAlert).xsmall().text_color(theme.red)))
                .child(div().flex_none().text_size(px(11.)).text_color(theme.muted_foreground.opacity(0.8)).child(branch_meta(b)))
                .on_click(move |_, window, cx| {
                    let name = name.clone();
                    t.update(cx, |v, cx| v.checkout(name, false, window, cx));
                })
        };

        let mut col = v_flex().gap(px(10.)).pt_3().pb_2();
        let branches_open = is_open("sec:branches", true);
        let mut branch_list = v_flex().gap(px(1.)).child(header("sec:branches", tr("sidebar.branches"), branches.len(), branches_open, cx));
        if branches_open && !tab.read(cx).loaded_once {
            branch_list = branch_list.child(crate::skeleton::sidebar(5, cx));
        }
        if branches_open {
            if let Some(cur) = branches.iter().find(|b| b.is_head) {
                branch_list = branch_list.child(branch_row(cur, cur.name.clone(), false, cx));
            }
            let mut groups: Vec<(String, Vec<&cairn_core::refs::Branch>)> = Vec::new();
            let mut loose: Vec<&cairn_core::refs::Branch> = Vec::new();
            for b in branches.iter().filter(|b| !b.is_head) {
                match b.name.split_once('/') {
                    Some((prefix, _)) => match groups.iter_mut().find(|(p, _)| p == prefix) {
                        Some((_, list)) => list.push(b),
                        None => groups.push((prefix.to_string(), vec![b])),
                    },
                    None => loose.push(b),
                }
            }
            for (_, members) in groups.iter().filter(|(_, m)| m.len() == 1) {
                loose.push(members[0]);
            }
            groups.retain(|(_, m)| m.len() > 1);
            loose.sort_by_key(|b| b.name.to_lowercase());
            for b in loose {
                branch_list = branch_list.child(branch_row(b, b.name.clone(), false, cx));
            }
            for (prefix, members) in &groups {
                let key = format!("grp:{prefix}");
                let open = is_open(&key, false);
                let k2 = key.clone();
                branch_list = branch_list.child(
                    row(SharedString::from(format!("side-group-{prefix}")), false, cx)
                        .child(icon(if open { IconName::FolderOpen } else { IconName::Folder }).xsmall().text_color(theme.yellow))
                        .child(div().flex_1().min_w_0().truncate().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.foreground).child(format!("{prefix}/")))
                        .child(div().flex_none().text_size(px(11.)).text_color(theme.muted_foreground).child(members.len().to_string()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.side_toggled.remove(&k2) {
                                this.side_toggled.insert(k2.clone());
                            }
                            cx.notify();
                        })),
                );
                if open {
                    let more_key = format!("more:{prefix}");
                    let all = toggled.contains(&more_key);
                    let shown = if all { members.len() } else { members.len().min(8) };
                    for b in members.iter().take(shown) {
                        let short = b.name.split_once('/').map(|(_, r)| r.to_string()).unwrap_or_else(|| b.name.clone());
                        branch_list = branch_list.child(branch_row(b, short, true, cx));
                    }
                    if members.len() > shown {
                        branch_list = branch_list.child(
                            row(SharedString::from(more_key.clone()), true, cx)
                                .text_xs()
                                .text_color(theme.link)
                                .child(t!("sidebar.show_all", n = members.len()).to_string())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.side_toggled.insert(more_key.clone());
                                    cx.notify();
                                })),
                        );
                    }
                }
            }
        }
        col = col.child(branch_list);
        if !worktrees.is_empty() {
            let open = is_open("sec:worktrees", worktrees.len() <= 3);
            let mut list = v_flex().gap(px(1.)).child(header("sec:worktrees", tr("sidebar.worktrees"), worktrees.len(), open, cx));
            if open {
                for w in &worktrees {
                    let path = w.path.clone();
                    let label = w.branch.clone().unwrap_or_else(|| w.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    let full: SharedString = crate::widgets::display_path(&w.path).into();
                    list = list.child(
                        row(SharedString::from(format!("side-wt-{}", w.path.display())), false, cx)
                            .child(icon(IconName::Layers).xsmall().text_color(theme.cyan))
                            .child(div().flex_1().min_w_0().truncate().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(label))
                            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx))
                            .on_click(cx.listener(move |this, _, window, cx| this.open_repo(&path, window, cx))),
                    );
                }
            }
            col = col.child(list);
        }
        if !stashes.is_empty() {
            let open = is_open("sec:stashes", stashes.len() <= 3);
            let mut list = v_flex().gap(px(1.)).child(header("sec:stashes", tr("sidebar.stashes"), stashes.len(), open, cx));
            if open {
                for s in stashes.iter().take(6) {
                    let t = tab.clone();
                    list = list.child(
                        row(SharedString::from(format!("side-stash-{}", s.index)), false, cx)
                            .child(icon(IconName::Archive).xsmall().text_color(theme.magenta))
                            .child(div().flex_1().min_w_0().truncate().text_xs().text_color(theme.muted_foreground).child(cairn_core::refs::stash_title(&s.message).0))
                            .on_click(move |_, window, cx| t.update(cx, |v, cx| v.open_page(crate::repo_view::Page::Stashes, window, cx))),
                    );
                }
            }
            col = col.child(list);
        }
        col.border_t_1().border_color(theme.border).mt_3()
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let categories = self.library.categories.clone();
        let uncategorized: Vec<RepoEntry> = self.library.repos_in(None).into_iter().cloned().collect();
        let home_active = self.active.is_none() && self.open_category.is_none();
        let view = cx.entity();

        let mut list = v_flex().gap_0p5();
        for c in &categories {
            let repos: Vec<RepoEntry> = self.library.repos_in(Some(c.id)).into_iter().cloned().collect();
            let id = c.id;
            let collapsed = c.collapsed;
            let open = self.active.is_none() && self.open_category == Some(Some(id));
            let v = view.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("cat-{id}")))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(open, |d| d.bg(theme.list_active))
                    .when(!open, |d| d.hover(|s| s.bg(cx.theme().list_hover)))
                    .child(
                        div()
                            .id(SharedString::from(format!("cat-toggle-{id}")))
                            .child(icon(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown }).xsmall().text_color(theme.muted_foreground))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.library.toggle_collapsed(id);
                                this.save_library(cx);
                                cx.notify();
                            })),
                    )
                    .child(icon(if collapsed { IconName::Folder } else { IconName::FolderOpen }).small().text_color(theme.yellow))
                    .child(div().flex_1().min_w_0().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(c.name.clone()))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(repos.len().to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| this.go_home(Some(Some(id)), cx)))
                    .context_menu(move |menu, _, _| category_menu(menu, v.clone(), id)),
            );
            if !collapsed {
                for r in &repos {
                    list = list.child(self.render_sidebar_repo(r, cx));
                }
                if repos.is_empty() {
                    list = list.child(div().pl_8().py_1().text_xs().text_color(theme.muted_foreground).child(tr("category.empty_hint")));
                }
            }
        }

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .p_2()
                    .gap_0p5()
                    .child(
                        div()
                            .id("side-home")
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_1p5()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .when(home_active, |d| d.bg(theme.list_active))
                            .when(!home_active, |d| d.hover(|s| s.bg(cx.theme().list_hover)))
                            .child(icon(IconName::House).small())
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(tr("sidebar.home")))
                            .on_click(cx.listener(|this, _, _, cx| this.go_home(None, cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(
                        hstack()
                            .child(section_label(tr("sidebar.categories"), cx))
                            .child(spacer())
                            .child(
                                Button::new("side-new-category")
                                    .xsmall()
                                    .ghost()
                                    .icon(icon(IconName::FolderPlus))
                                    .tooltip(tr("category.new_title"))
                                    .on_click(cx.listener(|this, _, window, cx| this.new_category(window, cx))),
                            ),
                    )
                    .when(categories.is_empty(), |d| {
                        d.child(div().px_2().pb_2().text_xs().text_color(theme.muted_foreground).child(tr("sidebar.no_categories")))
                    })
                    .child(list)
                    .when(!uncategorized.is_empty(), |d| {
                        let mut el = d.child(section_label(tr("sidebar.other_repos"), cx));
                        for r in &uncategorized {
                            el = el.child(self.render_sidebar_repo(r, cx));
                        }
                        el
                    })
                    .when_some(self.active_tab(), |d, tab| d.child(self.render_repo_sections(tab, cx))),
            )
            .child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(add_repo_button("side-add", None, view.clone(), cx)),
            )
    }

    fn render_title_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let home_active = self.active.is_none();
        let mut tabs = h_flex().h_full().items_end().gap_0p5().ml_2();
        tabs = tabs.child(
            div()
                .id("tab-home")
                .occlude()
                .h(px(30.))
                .px_3()
                .flex()
                .items_center()
                .gap_1p5()
                .rounded_t(px(6.))
                .cursor_pointer()
                .text_sm()
                .when(home_active, |d| d.bg(theme.background).text_color(theme.foreground))
                .when(!home_active, |d| d.text_color(theme.muted_foreground).hover(|s| s.bg(cx.theme().list_hover)))
                .child(icon(IconName::House).small())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.active = None;
                    cx.notify();
                })),
        );
        for (ix, tab) in self.tabs.iter().enumerate() {
            let active = self.active == Some(ix);
            let (name, busy) = {
                let v = tab.read(cx);
                let name = self.library.repo(&v.repo.root).map(|r| r.name.clone()).unwrap_or_else(|| v.name.to_string());
                (name, v.busy.is_some())
            };
            tabs = tabs.child(
                div()
                    .id(("tab", ix))
                    .group("tab")
                    .occlude()
                    .h(px(30.))
                    .pl_3()
                    .pr_1()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .rounded_t(px(6.))
                    .cursor_pointer()
                    .text_sm()
                    .max_w(px(220.))
                    .when(active, |d| d.bg(theme.background).text_color(theme.foreground))
                    .when(!active, |d| d.text_color(theme.muted_foreground).hover(|s| s.bg(cx.theme().list_hover)))
                    .child(icon(IconName::FolderGit2).small().text_color(if active { theme.primary } else { theme.muted_foreground }))
                    .child(div().truncate().child(name))
                    .when(busy, |d| d.child(gpui_kit::component::spinner::Spinner::new().xsmall()))
                    .child(
                        Button::new(("close-tab", ix))
                            .xsmall()
                            .ghost()
                            .icon(icon(IconName::X))
                            .tooltip(tr("tabs.close"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_tab(ix, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active = Some(ix);
                        this.save_library(cx);
                        cx.notify();
                    })),
            );
        }

        let maximized = window.is_maximized();
        let control = |id: &'static str, glyph: IconName, area: WindowControlArea, close: bool, cx: &App| {
            let theme = cx.theme();
            div()
                .id(id)
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .hover(|s| if close { s.bg(rgb(0xc42b1c)).text_color(rgb(0xffffff)) } else { s.bg(theme.list_hover).text_color(theme.foreground) })
                .window_control_area(area)
                .child(icon(glyph).small())
        };
        h_flex()
            .id("cairn-title-bar")
            .flex_none()
            .w_full()
            .h(px(36.))
            .items_center()
            .bg(theme.title_bar)
            .border_b_1()
            .border_color(theme.title_bar_border)
                .child(
                    hstack()
                        .id("title-drag-logo")
                        .h_full()
                        .gap_2()
                        .pl_3()
                        .pr_1()
                        .window_control_area(WindowControlArea::Drag)
                        .child(logo(px(18.), cx))
                        .child(div().text_sm().font_weight(FontWeight::BOLD).child("Cairn")),
                )
                .child(tabs)
                .child(div().id("title-drag").flex_1().h_full().window_control_area(WindowControlArea::Drag))
                .child(
                    hstack()
                        .id("titlebar-actions")
                        .occlude()
                        .gap(px(6.))
                        .pr_2()
                        .when_some(self.update_ready.as_ref().map(|(v, _)| v.clone()), |d, v| d.child(crate::updater::update_pill(&v, cx)))
                        .when(self.active.is_none(), |d| {
                            d.child(
                                crate::widgets::framed(Button::new("open-palette"), crate::widgets::Tone::Plain, cx)
                                    .h(px(28.))
                                    .rounded(px(7.))
                                    .text_color(theme.muted_foreground)
                                    .icon(icon(IconName::Search))
                                    .label(tr("palette.button"))
                                    .tooltip(tr("palette.tooltip"))
                                    .on_click(cx.listener(|_, _, window, cx| crate::palette::open(cx.entity(), window, cx))),
                            )
                        })
                        .child(
                            crate::widgets::framed(Button::new("toggle-sidebar"), crate::widgets::Tone::Plain, cx)
                                .size(px(28.))
                                .px_0()
                                .rounded(px(7.))
                                .icon(icon(if self.sidebar_open { IconName::PanelLeftClose } else { IconName::PanelLeftOpen }).text_color(theme.muted_foreground))
                                .tooltip(tr("tabs.toggle_sidebar"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sidebar_open = !this.sidebar_open;
                                    cx.notify();
                                })),
                        )
                        .child(
                            crate::widgets::framed(Button::new("open-settings"), crate::widgets::Tone::Plain, cx)
                                .size(px(28.))
                                .px_0()
                                .rounded(px(7.))
                                .icon(icon(IconName::Settings).text_color(theme.muted_foreground))
                                .tooltip(tr("settings.title"))
                                .on_click(cx.listener(|this, _, window, cx| crate::settings_dialog::open(cx.entity(), this.settings.clone(), window, cx))),
                        ),
                )
                .child(control("win-min", IconName::WindowMinimize, WindowControlArea::Min, false, cx))
                .child(control("win-max", if maximized { IconName::WindowRestore } else { IconName::WindowMaximize }, WindowControlArea::Max, false, cx))
                .child(control("win-close", IconName::WindowClose, WindowControlArea::Close, true, cx))
    }

    fn render_home(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        if self.library.repos.is_empty() && self.library.categories.is_empty() {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_4()
                .child(logo(px(56.), cx))
                .child(div().text_2xl().font_weight(FontWeight::BOLD).child(tr("home.welcome_title")))
                .child(div().max_w(px(520.)).text_center().text_color(theme.muted_foreground).child(tr("home.welcome_body")))
                .child(
                    h_flex()
                        .gap_2()
                        .child(add_repo_button("welcome-add", None, view.clone(), cx))
                        .child(
                            Button::new("welcome-category")
                                .outline()
                                .icon(icon(IconName::FolderPlus))
                                .label(tr("category.new_title"))
                                .on_click(cx.listener(|this, _, window, cx| this.new_category(window, cx))),
                        ),
                )
                .into_any_element();
        }

        let opened = self.open_category;
        let (title, repos, category): (SharedString, Vec<RepoEntry>, Option<Uuid>) = match opened {
            Some(Some(id)) => {
                let name = self.library.categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
                (name.into(), self.library.repos_in(Some(id)).into_iter().cloned().collect(), Some(id))
            }
            Some(None) => (tr("sidebar.other_repos"), self.library.repos_in(None).into_iter().cloned().collect(), None),
            None => (SharedString::default(), Vec::new(), None),
        };

        let header = hstack()
            .gap_3()
            .child(
                hstack()
                    .gap_2()
                    .when(opened.is_some(), |d| {
                        d.child(
                            Button::new("home-back")
                                .small()
                                .ghost()
                                .icon(icon(IconName::ArrowLeft))
                                .tooltip(tr("home.back"))
                                .on_click(cx.listener(|this, _, _, cx| this.go_home(None, cx))),
                        )
                        .child(icon(IconName::FolderOpen).text_color(theme.yellow))
                    })
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child(title)),
            )
            .child(spacer())
            .when(opened.is_none(), |d| {
                d.child(
                    Button::new("home-new-category")
                        .small()
                        .outline()
                        .icon(icon(IconName::FolderPlus))
                        .label(tr("category.new_title"))
                        .on_click(cx.listener(|this, _, window, cx| this.new_category(window, cx))),
                )
            })
            .when_some(opened.and_then(|c| c), |d, id| {
                let v = view.clone();
                d.child(
                    Button::new("cat-actions")
                        .small()
                        .ghost()
                        .icon(icon(IconName::Ellipsis))
                        .dropdown_menu(move |menu, _, _| category_menu(menu, v.clone(), id)),
                )
            })
            .child(add_repo_button("home-add", category, view.clone(), cx));

        let body = if opened.is_none() {
            let mut grid = div().w_full().grid().grid_cols(3).gap_3();
            for c in self.library.categories.clone() {
                let count = self.library.repos_in(Some(c.id)).len();
                let names: Vec<String> = self.library.repos_in(Some(c.id)).iter().take(4).map(|r| r.name.clone()).collect();
                let id = c.id;
                let v = view.clone();
                grid = grid.child(
                    v_flex()
                        .id(SharedString::from(format!("home-cat-{id}")))
                        .gap_2()
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.sidebar)
                        .cursor_pointer()
                        .hover(|s| s.border_color(cx.theme().primary.opacity(0.6)))
                        .child(
                            hstack()
                                .gap_2()
                                .child(icon(IconName::Folder).text_color(theme.yellow))
                                .child(div().flex_1().truncate().font_weight(FontWeight::SEMIBOLD).child(c.name.clone()))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("home.repo_count", count))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(if names.is_empty() { t!("category.empty_hint").to_string() } else { names.join(" · ") }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.go_home(Some(Some(id)), cx)))
                        .context_menu(move |menu, _, _| category_menu(menu, v.clone(), id)),
                );
            }
            let others = self.library.repos_in(None).len();
            if others > 0 {
                grid = grid.child(
                    v_flex()
                        .id("home-cat-none")
                        .gap_2()
                        .p_4()
                        .rounded(px(10.))
                        .border_1()
                        .border_dashed()
                        .border_color(theme.border)
                        .cursor_pointer()
                        .hover(|s| s.border_color(cx.theme().primary.opacity(0.6)))
                        .child(
                            hstack()
                                .gap_2()
                                .child(icon(IconName::Inbox).text_color(theme.muted_foreground))
                                .child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child(tr("sidebar.other_repos")))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("home.repo_count", others))),
                        )
                        .child(div().text_xs().text_color(theme.muted_foreground).child(tr("home.other_hint")))
                        .on_click(cx.listener(|this, _, _, cx| this.go_home(Some(None), cx))),
                );
            }
            let recent: Vec<RepoEntry> = {
                let mut r: Vec<RepoEntry> = self.library.repos.iter().filter(|r| r.last_opened > 0).cloned().collect();
                r.sort_by_key(|r| -r.last_opened);
                r.truncate(6);
                r
            };
            let dashboard = self.render_dashboard(cx);
            v_flex()
                .gap_5()
                .child(dashboard)
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(tr("home.categories")))
                .child(grid)
                .when(!recent.is_empty(), |d| {
                    d.child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(tr("home.recent")))
                        .child(self.repo_grid(&recent, cx))
                })
                .into_any_element()
        } else if repos.is_empty() {
            empty_state(IconName::FolderPlus, tr("category.empty_title"), tr("category.empty_body"), cx).into_any_element()
        } else {
            let panel = self.workspace.as_ref().filter(|(id, _)| Some(*id) == category).map(|(_, p)| p.clone());
            v_flex()
                .gap_5()
                .when_some(panel, |d, p| d.child(p))
                .child(self.repo_grid(&repos, cx))
                .into_any_element()
        };

        v_flex()
            .id("home-scroll")
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .gap_5()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn repo_grid(&self, repos: &[RepoEntry], cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        let mut grid = div().w_full().grid().grid_cols(3).gap_3();
        for r in repos {
            let path = r.path.clone();
            let entry = r.clone();
            let v = view.clone();
            let missing = !r.path.exists();
            let branch = read_branch(&r.path).unwrap_or_default();
            grid = grid.child(
                v_flex()
                    .id(SharedString::from(format!("card-{}", r.path.display())))
                    .gap_1p5()
                    .p_4()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .cursor_pointer()
                    .hover(|s| s.border_color(cx.theme().primary.opacity(0.6)))
                    .child(
                        hstack()
                            .gap_2()
                            .child(icon(IconName::FolderGit2).text_color(theme.primary))
                            .child(div().flex_1().truncate().font_weight(FontWeight::SEMIBOLD).child(r.name.clone())),
                    )
                    .child(
                        hstack()
                            .gap_1()
                            .text_xs()
                            .text_color(if missing { theme.red } else { theme.muted_foreground })
                            .child(icon(if missing { IconName::TriangleAlert } else { IconName::GitBranch }).xsmall())
                            .child(if missing { t!("repo.missing").to_string() } else { branch }),
                    )
                    .child(div().text_xs().truncate().text_color(theme.muted_foreground).child(crate::widgets::display_path(&r.path)))
                    .children(self.repo_status_chips(&r.path, cx).map(|c| c.pt_1()))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_repo(&path, window, cx)))
                    .context_menu(move |menu, _, cx| v.read(cx).repo_menu(menu, &entry, v.clone())),
            );
        }
        grid
    }
}

fn category_menu(menu: PopupMenu, view: Entity<CairnApp>, id: Uuid) -> PopupMenu {
    let (v1, v2, v3, v4, v5, v6, v7) = (view.clone(), view.clone(), view.clone(), view.clone(), view.clone(), view.clone(), view.clone());
    menu.item(PopupMenuItem::new(t!("category.open").to_string()).icon(icon(IconName::FolderOpen)).on_click(move |_, _, cx| {
        v1.update(cx, |this, cx| this.go_home(Some(Some(id)), cx));
    }))
    .item(PopupMenuItem::new(t!("repo.add_existing").to_string()).icon(icon(IconName::FolderPlus)).on_click(move |_, window, cx| {
        v2.update(cx, |this, cx| this.add_repositories(Some(id), window, cx));
    }))
    .item(PopupMenuItem::new(t!("repo.clone").to_string()).icon(icon(IconName::CloudDownload)).on_click(move |_, window, cx| {
        v3.update(cx, |this, cx| this.clone_repository(Some(id), window, cx));
    }))
    .separator()
    .item(PopupMenuItem::new(t!("category.rename").to_string()).icon(icon(IconName::Pencil)).on_click(move |_, window, cx| {
        v4.update(cx, |this, cx| this.rename_category(id, window, cx));
    }))
    .item(PopupMenuItem::new(t!("category.move_up").to_string()).icon(icon(IconName::ArrowUp)).on_click(move |_, _, cx| {
        v5.update(cx, |this, cx| {
            this.library.move_category(id, -1);
            this.save_library(cx);
            cx.notify();
        });
    }))
    .item(PopupMenuItem::new(t!("category.move_down").to_string()).icon(icon(IconName::ArrowDown)).on_click(move |_, _, cx| {
        v6.update(cx, |this, cx| {
            this.library.move_category(id, 1);
            this.save_library(cx);
            cx.notify();
        });
    }))
    .separator()
    .item(PopupMenuItem::new(t!("category.delete").to_string()).icon(icon(IconName::Trash)).on_click(move |_, window, cx| {
        v7.update(cx, |this, cx| this.delete_category(id, window, cx));
    }))
}

fn add_repo_button(id: &'static str, category: Option<Uuid>, view: Entity<CairnApp>, cx: &mut Context<CairnApp>) -> impl IntoElement {
    let tone = if id == "side-add" { crate::widgets::Tone::Plain } else { crate::widgets::Tone::Accent };
    crate::widgets::framed(Button::new(id), tone, cx)
        .when(id == "side-add", |b| b.w_full().text_color(cx.theme().muted_foreground))
        .icon(icon(IconName::Plus))
        .label(tr("repo.add"))
        .dropdown_menu(move |menu, _, _| {
            let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
            menu.item(PopupMenuItem::new(t!("repo.add_existing").to_string()).icon(icon(IconName::FolderOpen)).on_click(move |_, window, cx| {
                v1.update(cx, |this, cx| this.add_repositories(category, window, cx));
            }))
            .item(PopupMenuItem::new(t!("repo.clone").to_string()).icon(icon(IconName::CloudDownload)).on_click(move |_, window, cx| {
                v2.update(cx, |this, cx| this.clone_repository(category, window, cx));
            }))
            .item(PopupMenuItem::new(t!("repo.create_new").to_string()).icon(icon(IconName::FolderPlus)).on_click(move |_, window, cx| {
                v3.update(cx, |this, cx| this.init_repository(category, window, cx));
            }))
        })
}

pub(crate) fn logo(size: Pixels, cx: &App) -> impl IntoElement {
    let c = cx.theme().primary;
    canvas(|_, _, _| {}, move |bounds, _, window, _| {
        let w = bounds.size.width;
        let h = bounds.size.height;
        let stone = |cx_: f32, cy: f32, rx: f32, ry: f32| {
            Bounds::centered_at(point(bounds.left() + w * cx_, bounds.top() + h * cy), gpui_kit::size(w * rx * 2., h * ry * 2.))
        };
        window.paint_quad(fill(stone(0.5, 0.8, 0.36, 0.12), c.opacity(0.45)).corner_radii(h * 0.12));
        window.paint_quad(fill(stone(0.5, 0.54, 0.25, 0.11), c.opacity(0.7)).corner_radii(h * 0.11));
        window.paint_quad(fill(stone(0.5, 0.3, 0.16, 0.10), c).corner_radii(h * 0.10));
    })
    .size(size)
}

pub(crate) fn same(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

impl Focusable for CairnApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CairnApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if let (None, Some(Some(id))) = (self.active, self.open_category) {
            let repos: Vec<(PathBuf, String)> = self.library.repos_in(Some(id)).iter().filter(|r| r.path.exists()).map(|r| (r.path.clone(), r.name.clone())).collect();
            let stale = self.workspace.as_ref().is_none_or(|(wid, p)| *wid != id || p.read(cx).paths.len() != repos.len());
            if repos.len() >= 2 && stale {
                let settings = self.settings.clone();
                let panel = cx.new(|cx| crate::workspace_view::WorkspacePanel::new(repos, settings, window, cx));
                cx.subscribe_in(&panel, window, |this, _, ev: &crate::workspace_view::OpenPrEvent, window, cx| this.open_pr_in(&ev.0.clone(), ev.1, window, cx))
                .detach();
                self.workspace = Some((id, panel));
            }
        }
        let main = match self.active.and_then(|i| self.tabs.get(i)).cloned() {
            Some(tab) => tab.into_any_element(),
            None => self.render_home(cx).into_any_element(),
        };
        let _ = window;
        use crate::palette as p;
        v_flex()
            .size_full()
            .key_context("CairnApp")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &p::OpenPalette, window, cx| p::open(cx.entity(), window, cx)))
            .on_action(cx.listener(|this, _: &p::OpenSettings, window, cx| crate::settings_dialog::open(cx.entity(), this.settings.clone(), window, cx)))
            .on_action(cx.listener(|this, _: &p::Fetch, window, cx| this.with_tab(window, cx, |v, w, cx| v.fetch_action(w, cx))))
            .on_action(cx.listener(|this, _: &p::Pull, window, cx| this.with_tab(window, cx, |v, w, cx| v.pull_action(w, cx))))
            .on_action(cx.listener(|this, _: &p::Push, window, cx| this.with_tab(window, cx, |v, w, cx| v.push(false, w, cx))))
            .on_action(cx.listener(|this, _: &p::Stash, window, cx| this.with_tab(window, cx, |v, w, cx| v.stash_action(w, cx))))
            .on_action(cx.listener(|this, _: &p::AskAi, window, cx| {
                if let Some(t) = this.active_tab() {
                    crate::assist::open(t, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &p::NewBranch, window, cx| {
                if let Some(t) = this.active_tab() {
                    crate::dialogs::new_branch(t, None, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &p::GoToFile, window, cx| {
                if let Some(t) = this.active_tab() {
                    crate::goto_file::open(t, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &p::Refresh, window, cx| this.with_tab(window, cx, |v, _, cx| v.reload(cx))))
            .on_action(cx.listener(|this, _: &p::NextTab, _, cx| {
                if !this.tabs.is_empty() {
                    this.active = Some(this.active.map(|a| (a + 1) % this.tabs.len()).unwrap_or(0));
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &p::PrevTab, _, cx| {
                if !this.tabs.is_empty() {
                    let n = this.tabs.len();
                    this.active = Some(this.active.map(|a| (a + n - 1) % n).unwrap_or(0));
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &p::CloseTab, _, cx| {
                if let Some(a) = this.active {
                    this.close_tab(a, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &p::GoHistory, window, cx| this.with_tab(window, cx, |v, w, cx| v.open_page(crate::repo_view::Page::History, w, cx))))
            .on_action(cx.listener(|this, _: &p::GoChanges, window, cx| this.with_tab(window, cx, |v, w, cx| v.open_page(crate::repo_view::Page::Changes, w, cx))))
            .on_action(cx.listener(|this, _: &p::GoBranches, window, cx| this.with_tab(window, cx, |v, w, cx| v.open_page(crate::repo_view::Page::Branches, w, cx))))
            .on_action(cx.listener(|this, _: &p::CommitStaged, window, cx| this.with_tab(window, cx, |v, w, cx| {
                if v.page == crate::repo_view::Page::Changes {
                    let after = if v.amend { cairn_core::settings::AfterCommit::Nothing } else { v.settings.commits.after_commit };
                    v.commit(after, w, cx);
                }
            })))
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(self.render_title_bar(window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_open, |d| d.child(div().flex_none().w(px(260.)).h_full().child(self.render_sidebar(cx))))
                    .child(div().flex_1().min_w_0().h_full().child(main)),
            )
    }
}
