use cairn_core::compare::Comparison;
use cairn_core::diff::FileDiff;
use cairn_core::log::CommitInfo;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::diff_view::DiffSource;
use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, commit_row, count_chip, empty_state, file_row, framed, hstack, icon, spacer, toggle_pill, tr};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CompareTab {
    #[default]
    Files,
    Ahead,
    Behind,
}

#[derive(Default)]
pub(crate) struct CompareState {
    pub base: String,
    pub target: String,
    pub result: Option<Comparison>,
    pub error: Option<String>,
    pub loading: bool,
    pub tab: CompareTab,
    pub file: Option<String>,
    pub diff: Option<FileDiff>,
}

/// Fills a menu with every branch and tag; picking one calls `pick` with its name.
///
/// # Arguments
/// * `menu` - the menu to fill
/// * `view` - the repository view
/// * `pick` - what to do with the chosen name
///
/// # Returns
/// The filled menu.
pub(crate) fn ref_menu(mut menu: PopupMenu, view: &Entity<RepoView>, pick: impl Fn(&mut RepoView, String, &mut Context<RepoView>) + Clone + 'static, cx: &App) -> PopupMenu {
    let v = view.read(cx);
    let local: Vec<String> = v.branches.iter().filter(|b| !b.is_remote).map(|b| b.name.clone()).collect();
    let remote: Vec<String> = v.branches.iter().filter(|b| b.is_remote).map(|b| b.name.clone()).take(40).collect();
    let tags: Vec<String> = v.tags.iter().map(|t| t.name.clone()).take(40).collect();
    let add = |menu: PopupMenu, name: String, ic: IconName| {
        let view = view.clone();
        let pick = pick.clone();
        menu.item(PopupMenuItem::new(name.clone()).icon(icon(ic)).on_click(move |_, _, cx| {
            let name = name.clone();
            let pick = pick.clone();
            view.update(cx, |v, cx| pick(v, name, cx));
        }))
    };
    menu = add(menu, "HEAD".into(), IconName::CircleDot);
    menu = menu.separator().label(t!("toolbar.local_branches").to_string());
    for b in local {
        menu = add(menu, b, IconName::GitBranch);
    }
    if !remote.is_empty() {
        menu = menu.separator().label(t!("toolbar.remote_branches").to_string());
        for b in remote {
            menu = add(menu, b, IconName::Cloud);
        }
    }
    if !tags.is_empty() {
        menu = menu.separator().label(t!("branches.tags").to_string());
        for tg in tags {
            menu = add(menu, tg, IconName::Tag);
        }
    }
    menu
}

impl RepoView {
    /// Compares two branches, tags or commits and shows the result on the Compare page.
    ///
    /// # Arguments
    /// * `base` - what to compare against; the main branch when empty
    /// * `target` - what to inspect; the current branch when empty
    pub(crate) fn open_compare(&mut self, base: String, target: String, cx: &mut Context<Self>) {
        self.page = Page::Compare;
        let target = if target.is_empty() { self.current_branch().map(str::to_string).unwrap_or_else(|| "HEAD".into()) } else { target };
        let tab = self.compare.tab;
        self.compare = CompareState { base: base.clone(), target: target.clone(), loading: true, tab, ..Default::default() };
        let repo = self.repo.clone();
        let current = self.current_branch().map(str::to_string);
        self.spawn_bg(
            move || {
                let base = if base.is_empty() {
                    let trunk = repo.trunk().unwrap_or_else(|| "HEAD".into());
                    let local = trunk.split_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| trunk.clone());
                    if current.as_deref() == Some(local.as_str()) || current.as_deref() == Some(trunk.as_str()) {
                        repo.upstream_or_default().filter(|u| *u != target).unwrap_or(trunk)
                    } else {
                        trunk
                    }
                } else {
                    base
                };
                let result = cairn_core::compare::compare(&repo.git, &base, &target, 1000);
                (base, result)
            },
            |this, (base, result), cx| {
                this.compare.loading = false;
                this.compare.base = base;
                match result {
                    Ok(r) => {
                        let first = r.files.first().map(|f| f.path.clone());
                        this.compare.result = Some(r);
                        if let Some(path) = first.filter(|_| this.compare.tab == CompareTab::Files) {
                            this.open_compare_file(path, cx);
                        }
                    }
                    Err(e) => this.compare.error = Some(crate::repo_view::friendly_error(&e)),
                }
            },
            cx,
        );
        cx.notify();
    }

    fn open_compare_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(r) = self.compare.result.clone() else { return };
        let old_path = r.files.iter().find(|f| f.path == path).and_then(|f| f.old_path.clone());
        let from = r.merge_base.clone().unwrap_or_else(|| r.base.clone());
        self.compare.file = Some(path.clone());
        self.compare.diff = None;
        self.page_revs = Some((Some(from.clone()), r.target.clone()));
        let git = self.git();
        let opts = self.diff_options();
        let want = path.clone();
        self.spawn_bg(
            move || cairn_core::diff::revision_file_diff(&git, Some(&from), &r.target, &path, old_path.as_deref(), opts).ok(),
            move |this, diff, _| {
                if this.compare.file.as_deref() == Some(want.as_str()) {
                    this.compare.diff = diff;
                }
            },
            cx,
        );
        cx.notify();
    }

    fn compare_commit_list(&mut self, commits: &[CommitInfo], cx: &mut Context<Self>) -> AnyElement {
        if commits.is_empty() {
            return empty_state(IconName::CircleCheck, tr("compare.no_commits_title"), tr("compare.no_commits_body"), cx).into_any_element();
        }
        let mut list = v_flex().gap_0p5().p_2();
        for (i, c) in commits.iter().enumerate() {
            let id = c.id.clone();
            let selected = self.selected_commit.as_deref() == Some(c.id.as_str());
            list = list.child(commit_row(("cmp-commit", i), &c.id, &c.summary, &c.author, c.time, selected, cx).on_click(cx.listener(move |this, _, _, cx| {
                this.page = Page::History;
                if let Some(ix) = this.commits.iter().position(|x| x.id == id) {
                    this.history_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                }
                this.select_commit(id.clone(), cx);
            })));
        }
        div().id("cmp-commits").size_full().overflow_y_scroll().child(list).into_any_element()
    }

    pub(crate) fn render_compare(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let view = cx.entity();
        let picker = |id: &'static str, label: SharedString, value: String, is_base: bool, cx: &mut Context<Self>| {
            let view = view.clone();
            v_flex()
                .gap_1()
                .child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(label.to_uppercase()))
                .child(
                    framed(Button::new(id), Tone::Plain, cx)
                        .min_w(px(180.))
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .icon(icon(IconName::GitBranch).text_color(cx.theme().magenta))
                        .label(if value.is_empty() { tr("compare.pick") } else { SharedString::from(value) })
                        .dropdown_caret(true)
                        .dropdown_menu(move |menu, _, cx| {
                            ref_menu(menu, &view, move |v, name, cx| {
                                let (base, target) = if is_base { (name, v.compare.target.clone()) } else { (v.compare.base.clone(), name) };
                                v.open_compare(base, target, cx);
                            }, cx)
                        }),
                )
        };
        let base = self.compare.base.clone();
        let target = self.compare.target.clone();
        let header = hstack()
            .flex_none()
            .items_end()
            .flex_wrap()
            .gap_3()
            .px_5()
            .py_4()
            .border_b_1()
            .border_color(theme.border)
            .child(picker("cmp-base", tr("compare.base"), base.clone(), true, cx))
            .child(
                Button::new("cmp-swap").small().ghost().icon(icon(IconName::ArrowLeftRight)).tooltip(tr("compare.swap")).on_click(cx.listener(move |this, _, _, cx| {
                    let (b, t) = (this.compare.base.clone(), this.compare.target.clone());
                    this.open_compare(t, b, cx);
                })),
            )
            .child(picker("cmp-target", tr("compare.target"), target.clone(), false, cx))
            .child(spacer())
            .child(div().max_w(px(360.)).text_xs().text_color(theme.muted_foreground).child(tr("compare.explain")));

        let body = if self.compare.loading {
            div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element()
        } else if let Some(err) = self.compare.error.clone() {
            empty_state(IconName::TriangleAlert, tr("compare.failed"), err, cx).into_any_element()
        } else if let Some(r) = self.compare.result.clone() {
            let tab = self.compare.tab;
            let tabs = hstack()
                .gap_2()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(toggle_pill("cmp-tab-files", t!("compare.files", n = r.files.len()).to_string(), tab == CompareTab::Files, cx).on_click(cx.listener(|this, _, _, cx| {
                    this.compare.tab = CompareTab::Files;
                    cx.notify();
                })))
                .child(toggle_pill("cmp-tab-ahead", t!("compare.only_on", name = r.target.clone(), n = r.ahead.len()).to_string(), tab == CompareTab::Ahead, cx).on_click(cx.listener(|this, _, _, cx| {
                    this.compare.tab = CompareTab::Ahead;
                    cx.notify();
                })))
                .child(toggle_pill("cmp-tab-behind", t!("compare.only_on", name = r.base.clone(), n = r.behind.len()).to_string(), tab == CompareTab::Behind, cx).on_click(cx.listener(|this, _, _, cx| {
                    this.compare.tab = CompareTab::Behind;
                    cx.notify();
                })));
            let content = match tab {
                CompareTab::Ahead => self.compare_commit_list(&r.ahead, cx),
                CompareTab::Behind => self.compare_commit_list(&r.behind, cx),
                CompareTab::Files if r.files.is_empty() => empty_state(IconName::FileCheck, tr("compare.no_files_title"), tr("compare.no_files_body"), cx).into_any_element(),
                CompareTab::Files => {
                    let mut files = v_flex().gap_0p5().p_1();
                    for (i, f) in r.files.iter().enumerate() {
                        let path = f.path.clone();
                        let on = self.compare.file.as_deref() == Some(f.path.as_str());
                        let stats = f.added.zip(f.removed);
                        files = files.child(file_row(("cmp-file", i), f.status, &f.path, stats, on, cx).on_click(cx.listener(move |this, _, _, cx| this.open_compare_file(path.clone(), cx))));
                    }
                    let diff = match (&self.compare.file, self.compare.diff.clone()) {
                        (Some(_), Some(d)) => self.render_file_diff(DiffSource::Page, &d, cx),
                        (Some(_), None) => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
                        _ => empty_state(IconName::FileDiff, tr("changes.pick_title"), tr("compare.pick_file"), cx).into_any_element(),
                    };
                    h_flex()
                        .size_full()
                        .child(div().id("cmp-files").flex_none().w(px(300.)).h_full().overflow_y_scroll().bg(theme.sidebar).border_r_1().border_color(theme.border).child(files))
                        .child(div().flex_1().min_w_0().h_full().child(diff))
                        .into_any_element()
                }
            };
            v_flex()
                .size_full()
                .child(
                    tabs.child(spacer()).when(r.merge_base.is_none(), |d| d.child(count_chip(tr("compare.unrelated"), theme.yellow.opacity(0.15), theme.yellow))),
                )
                .child(div().flex_1().min_h_0().child(content))
                .into_any_element()
        } else {
            empty_state(IconName::GitCompareArrows, tr("compare.pick_title"), tr("compare.pick_body"), cx).into_any_element()
        };
        v_flex().size_full().child(header).child(div().flex_1().min_h_0().child(body))
    }
}
