use std::collections::HashSet;
use std::path::PathBuf;

use cairn_core::settings::Settings;
use cairn_core::workspace::{self, RepoOutcome, RepoSummary};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::widgets::{badge, hstack, icon, section_label, spacer, tr};

pub(crate) struct WorkspacePanel {
    pub paths: Vec<PathBuf>,
    names: Vec<(PathBuf, String)>,
    settings: Settings,
    summaries: Vec<RepoSummary>,
    loading: bool,
    busy: Option<SharedString>,
    linked: Option<String>,
    include: HashSet<PathBuf>,
    order: Vec<PathBuf>,
    message: Entity<InputState>,
    open_prs: bool,
}

impl WorkspacePanel {
    pub(crate) fn new(repos: Vec<(PathBuf, String)>, settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| InputState::new(window, cx).placeholder(tr("workspace.message_placeholder")));
        let mut panel = Self {
            paths: repos.iter().map(|(p, _)| p.clone()).collect(),
            names: repos,
            settings,
            summaries: Vec::new(),
            loading: false,
            busy: None,
            linked: None,
            include: HashSet::new(),
            order: Vec::new(),
            message,
            open_prs: true,
        };
        panel.refresh(cx);
        panel
    }

    fn name_of(&self, p: &PathBuf) -> String {
        self.names.iter().find(|(x, _)| x == p).map(|(_, n)| n.clone()).unwrap_or_else(|| p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let paths = self.paths.clone();
        self.loading = true;
        cx.spawn(async move |this, cx| {
            let summaries = cx
                .background_executor()
                .spawn(async move {
                    std::thread::scope(|s| {
                        let handles: Vec<_> = paths.iter().map(|p| s.spawn(move || workspace::summarize(p))).collect();
                        handles.into_iter().filter_map(|h| h.join().ok()).collect::<Vec<_>>()
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.summaries = summaries;
                let links = workspace::linked_branches(&this.summaries, &this.settings.git.default_branch);
                if this.linked.as_ref().is_none_or(|l| !links.iter().any(|(b, _)| b == l)) {
                    this.linked = links.first().map(|(b, _)| b.clone());
                    this.reset_linked();
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reset_linked(&mut self) {
        let Some(branch) = self.linked.clone() else {
            self.include.clear();
            self.order.clear();
            return;
        };
        let members: Vec<PathBuf> = self.summaries.iter().filter(|s| s.branch.as_deref() == Some(branch.as_str())).map(|s| s.path.clone()).collect();
        self.include = members.iter().cloned().collect();
        self.order = members;
    }

    fn run_batch<F>(&mut self, label: SharedString, op: F, window: &mut Window, cx: &mut Context<Self>)
    where
        F: FnOnce() -> Vec<RepoOutcome> + Send + 'static,
    {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(label.clone());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let outcomes = cx.background_executor().spawn(async move { op() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                let failed: Vec<String> = outcomes.iter().filter(|o| !o.ok).map(|o| format!("{}: {}", this.name_of(&o.path), o.message.lines().next().unwrap_or_default())).collect();
                if failed.is_empty() {
                    window.push_notification(Notification::success(if outcomes.len() == 1 { t!("workspace.done_one", what = label.to_string()).to_string() } else { t!("workspace.done", what = label.to_string(), n = outcomes.len()).to_string() }), cx);
                } else {
                    window.push_notification(Notification::warning(failed.join("\n")).title(t!("workspace.partly", what = label.to_string()).to_string()), cx);
                }
                this.refresh(cx);
            });
        })
        .detach();
    }

    fn ship(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_string();
        if message.is_empty() {
            window.push_notification(Notification::warning(tr("changes.need_summary")), cx);
            return;
        }
        let order: Vec<PathBuf> = self.order.iter().filter(|p| self.include.contains(*p)).cloned().collect();
        let sign = self.settings.commits.sign;
        let stop = self.settings.workspace.all_or_nothing_push;
        let scan = self.settings.checks.secret_scan && self.settings.checks.block_on_secret;
        let open_prs = self.open_prs;
        let draft = self.settings.remotes.draft_prs;
        let base = self.settings.git.default_branch.clone();
        let msg2 = message.clone();
        self.message.update(cx, |s, cx| s.set_value("", window, cx));
        self.run_batch(tr("workspace.ship"), move || {
            let mut out = workspace::commit_all(&order, &message, sign);
            if out.iter().any(|o| !o.ok) && stop {
                return out;
            }
            if scan {
                for p in &order {
                    let git = cairn_core::Git::new(p);
                    let up = cairn_core::status::load_status(&git).ok().and_then(|s| s.branch.upstream);
                    let found = cairn_core::checks::scan_push(&git, up.as_deref()).unwrap_or_default();
                    if !found.is_empty() {
                        out.push(RepoOutcome { path: p.clone(), ok: false, message: format!("{} possible secrets found, nothing was pushed", found.len()) });
                        return out;
                    }
                }
            }
            out.extend(workspace::push_all(&order, stop));
            if open_prs && out.iter().all(|o| o.ok) {
                for p in &order {
                    let git = cairn_core::Git::new(p);
                    if let Err(e) = cairn_core::stack::create_pr(&git, &base, &msg2, "", draft)
                        && !e.to_string().contains("already exists")
                    {
                        out.push(RepoOutcome { path: p.clone(), ok: false, message: e.to_string() });
                    }
                }
            }
            out
        }, window, cx);
    }
}

impl Render for WorkspacePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let busy = self.busy.clone();
        let mut grid = div().w_full().grid().grid_cols(3).gap_2();
        for s in &self.summaries {
            let name = self.name_of(&s.path);
            let (state, color) = if s.error.is_some() {
                (tr("workspace.error"), theme.red)
            } else if s.changed > 0 {
                (t!("workspace.changed", n = s.changed).to_string().into(), theme.yellow)
            } else if s.behind > 0 {
                (t!("workspace.behind", n = s.behind).to_string().into(), theme.cyan)
            } else if s.ahead > 0 {
                (t!("workspace.ahead", n = s.ahead).to_string().into(), theme.blue)
            } else {
                (tr("workspace.clean"), theme.green)
            };
            let linked = self.linked.is_some() && s.branch == self.linked;
            grid = grid.child(
                v_flex()
                    .gap_1()
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if linked { theme.magenta.opacity(0.6) } else { theme.border })
                    .bg(theme.sidebar)
                    .child(hstack().gap_2().child(icon(IconName::FolderGit2).small().text_color(if linked { theme.magenta } else { theme.muted_foreground })).child(div().flex_1().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(name)).child(badge(state, color)))
                    .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(if linked { theme.magenta } else { theme.muted_foreground }).child(s.branch.clone().unwrap_or_else(|| t!("toolbar.detached").to_string()))),
            );
        }
        let links = workspace::linked_branches(&self.summaries, &self.settings.git.default_branch);
        let mut linked_section = v_flex().gap_2();
        if links.is_empty() {
            linked_section = linked_section.child(div().text_sm().text_color(theme.muted_foreground).child(tr("workspace.no_links")));
        } else {
            let mut chooser = h_flex().flex_wrap().gap_1();
            for (b, members) in &links {
                let on = self.linked.as_deref() == Some(b.as_str());
                let name = b.clone();
                let btn = Button::new(SharedString::from(format!("link-{b}"))).small().label(format!("{b} · {}", members.len())).on_click(cx.listener(move |this, _, _, cx| {
                    this.linked = Some(name.clone());
                    this.reset_linked();
                    cx.notify();
                }));
                chooser = chooser.child(if on { btn.primary() } else { btn.outline() });
            }
            linked_section = linked_section.child(chooser);
            let order = self.order.clone();
            for (i, p) in order.iter().enumerate() {
                let s = self.summaries.iter().find(|s| &s.path == p).cloned().unwrap_or_default();
                let included = self.include.contains(p);
                let path = p.clone();
                let n = order.len();
                linked_section = linked_section.child(
                    hstack()
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .rounded(px(6.))
                        .bg(theme.sidebar)
                        .child(div().w(px(18.)).text_xs().text_color(theme.muted_foreground).child(format!("{}.", i + 1)))
                        .child(Switch::new(SharedString::from(format!("inc-{}", p.display()))).checked(included).on_click(cx.listener(move |this, v: &bool, _, cx| {
                            if *v {
                                this.include.insert(path.clone());
                            } else {
                                this.include.remove(&path);
                            }
                            cx.notify();
                        })))
                        .child(div().flex_1().text_sm().child(self.name_of(p)))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(t!("workspace.repo_state", changed = s.changed, ahead = s.ahead).to_string()))
                        .child(Button::new(("ord-up", i)).xsmall().ghost().icon(icon(IconName::ChevronUp)).disabled(i == 0).on_click(cx.listener(move |this, _, _, cx| {
                            this.order.swap(i, i - 1);
                            cx.notify();
                        })))
                        .child(Button::new(("ord-down", i)).xsmall().ghost().icon(icon(IconName::ChevronDown)).disabled(i + 1 >= n).on_click(cx.listener(move |this, _, _, cx| {
                            this.order.swap(i, i + 1);
                            cx.notify();
                        }))),
                );
            }
            let count = self.order.iter().filter(|p| self.include.contains(*p)).count();
            linked_section = linked_section
                .child(div().text_xs().text_color(theme.muted_foreground).child(tr("workspace.order_hint")))
                .child(Input::new(&self.message))
                .child(
                    hstack()
                        .gap_3()
                        .child(Switch::new("ws-open-prs").checked(self.open_prs).label(tr("workspace.open_prs")).on_click(cx.listener(|this, v: &bool, _, cx| {
                            this.open_prs = *v;
                            cx.notify();
                        })))
                        .child(spacer())
                        .child(Button::new("ws-ship").small().primary().icon(icon(IconName::ArrowUpToLine)).label(crate::widgets::plural("workspace.ship_n", count)).disabled(count == 0 || busy.is_some()).on_click(cx.listener(|this, _, window, cx| this.ship(window, cx)))),
                );
        }
        let paths = self.paths.clone();
        let (p1, p2, p3) = (paths.clone(), paths.clone(), paths.clone());
        let mode = self.settings.git.pull_mode;
        let prune = self.settings.git.prune_on_fetch;
        v_flex()
            .gap_3()
            .p_4()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .child(
                hstack()
                    .gap_2()
                    .child(icon(IconName::Layers).text_color(theme.magenta))
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("workspace.title")))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(t!("workspace.subtitle", n = paths.len()).to_string()))
                    .child(spacer())
                    .when_some(busy.clone(), |d, b| d.child(hstack().gap_1().text_xs().text_color(theme.muted_foreground).child(gpui_kit::component::spinner::Spinner::new().xsmall()).child(b)))
                    .child(Button::new("ws-refresh").small().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| this.refresh(cx))))
                    .child(Button::new("ws-fetch").small().outline().label(tr("workspace.fetch_all")).disabled(busy.is_some()).on_click(cx.listener(move |this, _, window, cx| {
                        let p = p1.clone();
                        this.run_batch(tr("workspace.fetch_all"), move || workspace::fetch_all(&p, prune), window, cx);
                    })))
                    .child(Button::new("ws-pull").small().outline().label(tr("workspace.pull_clean")).disabled(busy.is_some()).on_click(cx.listener(move |this, _, window, cx| {
                        let p = p2.clone();
                        this.run_batch(tr("workspace.pull_clean"), move || workspace::pull_clean(&p, mode), window, cx);
                    })))
                    .child(Button::new("ws-switch").small().outline().label(tr("workspace.switch_all")).disabled(busy.is_some()).on_click(cx.listener(move |_, _, window, cx| {
                        let p = p3.clone();
                        let view = cx.entity();
                        crate::dialogs::text_prompt(tr("workspace.switch_title"), tr("workspace.switch_body"), tr("branch.name_placeholder"), String::new(), tr("workspace.switch_ok"), window, cx, move |branch, window, cx| {
                            let p = p.clone();
                            view.update(cx, |this, cx| this.run_batch(tr("workspace.switch_all"), move || workspace::switch_all(&p, &branch, true), window, cx));
                        });
                    }))),
            )
            .when(self.loading && self.summaries.is_empty(), |d| d.child(gpui_kit::component::spinner::Spinner::new()))
            .child(grid)
            .child(section_label(tr("workspace.linked_title"), cx).px_0())
            .child(linked_section)
    }
}
