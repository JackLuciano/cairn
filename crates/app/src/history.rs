use cairn_core::graph::{GraphRow, LinePart};
use cairn_core::log::{CommitInfo, RefKind};
use cairn_core::settings::{DateFormat, GraphStyle};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use rust_i18n::t;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::diff_view::DiffSource;
use crate::repo_view::RepoView;
use crate::widgets::{avatar, empty_state, full_time, hstack, icon, lane_color, relative_time, spacer, status_letter, toggle_pill, tr};

pub(crate) const ROW_H: f32 = 34.;
const LANE_W: f32 = 14.;
const GRAPH_PAD: f32 = 10.;

#[allow(clippy::too_many_arguments)]
fn paint_graph(row: &GraphRow, is_head: bool, is_merge: bool, style: GraphStyle, bounds: Bounds<Pixels>, colors: &[Hsla], bg: Hsla, window: &mut Window) {
    let top = bounds.top();
    let bottom = bounds.bottom();
    let mid = top + (bottom - top) / 2.;
    let x = |lane: usize| bounds.left() + px(GRAPH_PAD + lane as f32 * LANE_W);
    let color = |i: usize| colors[i % colors.len()];
    for line in &row.lines {
        let (p0, p1) = match line.part {
            LinePart::Through => (point(x(line.from), top), point(x(line.to), bottom)),
            LinePart::Top => (point(x(line.from), top), point(x(line.to), mid)),
            LinePart::Bottom => (point(x(line.from), mid), point(x(line.to), bottom)),
        };
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(p0);
        if p0.x == p1.x {
            path.line_to(p1);
        } else if style == GraphStyle::Angular {
            let bend_y = if line.part == LinePart::Top { p1.y } else { p0.y };
            path.line_to(point(p1.x, bend_y));
            path.line_to(p1);
        } else {
            let cy = (p0.y + p1.y) / 2.;
            path.cubic_bezier_to(p1, point(p0.x, cy), point(p1.x, cy));
        }
        if let Ok(p) = path.build() {
            window.paint_path(p, color(line.color));
        }
    }
    let c = point(x(row.lane), mid);
    let dot_color = color(row.color);
    if is_head {
        let ring = Bounds::centered_at(c, size(px(16.), px(16.)));
        window.paint_quad(fill(ring, dot_color.opacity(0.25)).corner_radii(px(8.)));
    }
    let d = if is_head { 11. } else { 9. };
    let b = Bounds::centered_at(c, size(px(d), px(d)));
    if is_merge {
        window.paint_quad(fill(b, bg).corner_radii(px(d / 2.)).border_widths(px(2.5)).border_color(dot_color));
    } else {
        window.paint_quad(fill(b, dot_color).corner_radii(px(d / 2.)).border_widths(px(1.5)).border_color(bg));
    }
}

impl RepoView {
    /// Handles a click on a commit row: plain click selects it, Ctrl adds or removes it from the
    /// selection, Shift selects the range from the last clicked row.
    ///
    /// # Arguments
    /// * `ix` - row index
    /// * `id` - the commit
    /// * `mods` - keyboard modifiers held during the click
    pub(crate) fn click_commit(&mut self, ix: usize, id: String, mods: Modifiers, cx: &mut Context<Self>) {
        if mods.shift
            && let Some(anchor) = self.last_clicked
        {
            let (a, b) = if anchor <= ix { (anchor, ix) } else { (ix, anchor) };
            self.multi_select = self.commits[a..=b.min(self.commits.len().saturating_sub(1))].iter().map(|c| c.id.clone()).collect();
            cx.notify();
            return;
        }
        if mods.control || mods.platform {
            if self.multi_select.is_empty()
                && let Some(cur) = self.selected_commit.clone()
            {
                self.multi_select.push(cur);
            }
            if let Some(pos) = self.multi_select.iter().position(|c| *c == id) {
                self.multi_select.remove(pos);
            } else {
                self.multi_select.push(id.clone());
            }
            if self.multi_select.len() <= 1 {
                self.multi_select.clear();
            }
            self.last_clicked = Some(ix);
            self.select_commit(id, cx);
            return;
        }
        self.multi_select.clear();
        self.last_clicked = Some(ix);
        self.select_commit(id, cx);
    }

    /// Copies the hashes of the selected commits, oldest first, one per line.
    pub(crate) fn copy_selected_hashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked: std::collections::HashSet<&str> = self.multi_select.iter().map(String::as_str).collect();
        let ordered: Vec<&str> = self.commits.iter().rev().map(|c| c.id.as_str()).filter(|id| picked.contains(id)).collect();
        let n = ordered.len();
        cx.write_to_clipboard(ClipboardItem::new_string(ordered.join("\n")));
        window.push_notification(gpui_kit::component::notification::Notification::success(crate::widgets::plural("history.copied_hashes", n)), cx);
    }

    /// Copies the selected commits as `hash - message` lines, newest first like `git log`.
    ///
    /// # Arguments
    /// * `window` - shows the confirmation
    /// * `cx` - the repository view's context
    pub(crate) fn copy_selected_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked: std::collections::HashSet<&str> = self.multi_select.iter().map(String::as_str).collect();
        let lines: Vec<String> = self.commits.iter().filter(|c| picked.contains(c.id.as_str())).map(|c| format!("{} - {}", c.short_id(7), c.summary)).collect();
        let n = lines.len();
        cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
        window.push_notification(gpui_kit::component::notification::Notification::success(crate::widgets::plural("history.copied_list", n)), cx);
    }

    /// Cherry-picks every selected commit onto the current branch, oldest first.
    pub(crate) fn cherry_pick_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.multi_select.clone();
        let n = ids.len();
        self.multi_select.clear();
        self.run_op(tr("op.cherry_pick"), Some(crate::widgets::plural("undo.cherry_pick_n", n)), move |git| cairn_core::ops::cherry_pick_many(git, &ids), window, cx);
    }

    /// Reverts every selected commit with new commits.
    pub(crate) fn revert_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.multi_select.clone();
        let n = ids.len();
        self.multi_select.clear();
        self.run_op(tr("op.revert"), Some(crate::widgets::plural("undo.revert_n", n)), move |git| cairn_core::ops::revert_many(git, &ids), window, cx);
    }

    fn format_time(&self, ts: i64) -> SharedString {
        match self.settings.graph.date_format {
            DateFormat::Relative => relative_time(ts),
            DateFormat::Local => full_time(ts),
            DateFormat::Iso => chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_default()
                .into(),
        }
    }

    fn render_ref_chips(&self, c: &CommitInfo, cx: &App) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let pr_chips: Vec<AnyElement> = if self.settings.remotes.show_prs_in_graph {
            c.refs
                .iter()
                .filter(|r| r.kind == RefKind::LocalBranch)
                .filter_map(|r| self.prs.iter().find(|p| p.head == r.name))
                .map(|p| {
                    let failed = p.checks.iter().any(|c| c.state() == "failed");
                    let running = p.checks.iter().any(|c| matches!(c.state(), "running" | "queued"));
                    let color = if failed { theme.red } else if running { theme.yellow } else if p.checks.is_empty() { theme.muted_foreground } else { theme.green };
                    hstack()
                        .flex_none()
                        .gap_1()
                        .px(px(6.))
                        .h(px(18.))
                        .rounded(px(4.))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.magenta)
                        .bg(theme.magenta.opacity(0.12))
                        .child(icon(IconName::GitPullRequest).xsmall())
                        .child(format!("#{}", p.number))
                        .child(div().size(px(6.)).rounded_full().bg(color))
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut chips: Vec<AnyElement> = c.refs
            .iter()
            .filter(|r| self.settings.graph.show_tags || r.kind != RefKind::Tag)
            .map(|r| {
                let (color, ic) = match r.kind {
                    RefKind::LocalBranch => (theme.primary, IconName::GitBranch),
                    RefKind::RemoteBranch => (theme.muted_foreground, IconName::Cloud),
                    RefKind::Tag => (theme.yellow, IconName::Tag),
                    RefKind::Stash => (theme.magenta, IconName::Archive),
                    RefKind::Head => (theme.red, IconName::CircleDot),
                };
                let chip = hstack()
                    .flex_none()
                    .gap_1()
                    .px(px(6.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if r.is_head { theme.primary_foreground } else { color })
                    .bg(if r.is_head { color } else { color.opacity(0.14) })
                    .border_1()
                    .border_color(color.opacity(0.45))
                    .child(icon(ic).xsmall())
                    .child(r.name.clone());
                chip.into_any_element()
            })
            .collect();
        chips.extend(pr_chips);
        chips
    }

    fn render_commit_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let commit = self.commits[ix].clone();
        let row = self.rows[ix].clone();
        let theme = cx.theme().clone();
        let selected = if self.multi_select.is_empty() { self.selected_commit.as_deref() == Some(commit.id.as_str()) } else { self.multi_select.contains(&commit.id) };
        let is_head = commit.refs.iter().any(|r| r.is_head);
        let graph_w = self.graph_w;
        let show_graph = self.show_graph;
        let colors: Vec<Hsla> = (0..8).map(|i| lane_color(i, cx)).collect();
        let bg = if selected { theme.list_active } else { theme.background };
        let style = self.settings.graph.style;
        let is_merge = commit.is_merge();
        let id = commit.id.clone();
        let menu_id = commit.id.clone();
        let short = commit.short_id(self.settings.graph.hash_length.clamp(4, 40)).to_string();
        let time = self.format_time(commit.time);
        let view = cx.entity();

        div()
            .id(("commit", ix))
            .h(px(ROW_H))
            .w_full()
            .flex()
            .items_center()
            .bg(bg)
            .border_b_1()
            .border_color(theme.border.opacity(0.4))
            .when(!selected, |d| d.hover(|s| s.bg(cx.theme().list_hover)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| this.click_commit(ix, id.clone(), ev.modifiers(), cx)))
            .pl_3()
            .pr_4()
            .gap(px(10.))
            .when(show_graph, |d| {
                d.child(
                    div()
                        .flex_none()
                        .w(px(graph_w))
                        .h_full()
                        .child(canvas(|_, _, _| {}, move |bounds, _, window, _| {
                            paint_graph(&row, is_head, is_merge, style, bounds, &colors, bg, window)
                        }).size_full()),
                )
            })
            .child(
                hstack()
                    .flex_1()
                    .min_w_0()
                    .gap_1p5()
                    .overflow_hidden()
                    .children(self.render_ref_chips(&commit, cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(theme.foreground)
                            .child(commit.summary.clone()),
                    ),
            )
            .when(self.settings.appearance.show_avatars, |d| d.child(avatar(&commit.author, px(22.), cx)))
            .child(div().flex_none().w(px(110.)).truncate().text_xs().text_color(theme.muted_foreground).child(commit.author.clone()))
            .child(div().flex_none().w(px(76.)).text_right().text_xs().text_color(theme.muted_foreground).child(time))
            .child(div().flex_none().w(px(64.)).text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(short))
            .context_menu(move |menu, _window, cx| {
                let v = view.clone();
                let id = menu_id.clone();
                let selected = v.read(cx).multi_select.clone();
                if selected.len() > 1 && selected.contains(&id) {
                    crate::commit_menu::build_multi(menu, v, selected.len())
                } else {
                    crate::commit_menu::build(menu, v, id)
                }
            })
            .into_any_element()
    }

    pub(crate) fn render_history(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.commits.is_empty() {
            return div()
                .size_full()
                .child(empty_state(IconName::GitCommitHorizontal, tr("history.empty_title"), tr("history.empty_body"), cx))
                .into_any_element();
        }
        let count = self.commits.len();
        let lanes = self.rows.iter().take(80).map(|r| r.width).max().unwrap_or(1).min(12);
        let graph_header_w = (GRAPH_PAD * 2. + (lanes.max(1) - 1) as f32 * LANE_W + 4.).max(52.);
        self.graph_w = graph_header_w;
        let list = uniform_list(
            "history-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range.map(|ix| this.render_commit_row(ix, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.history_scroll)
        .size_full();

        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        hstack()
                            .px_4()
                            .py(px(10.))
                            .gap_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(div().flex_none().whitespace_nowrap().text_sm().font_weight(FontWeight::SEMIBOLD).child(if self.settings.graph.first_parent { tr("history.first_parent_title") } else { tr("history.all_branches") }))
                            .child(div().flex_none().whitespace_nowrap().text_sm().text_color(theme.muted_foreground).child(crate::widgets::plural("history.commits_shown", count)))
                            .child(div().flex_1().min_w(px(120.)).max_w(px(280.)).ml_2().child(gpui_kit::component::input::Input::new(&self.search).prefix(icon(IconName::Search).small().text_color(theme.muted_foreground))))
                            .child(spacer())
                            .child(toggle_pill("toggle-graph", tr("history.graph"), self.show_graph, cx).on_click(cx.listener(|this, _, _, cx| {
                                this.show_graph = !this.show_graph;
                                cx.notify();
                            })))
                            .child(toggle_pill("first-parent", tr("history.first_parent"), self.settings.graph.first_parent, cx).on_click(cx.listener(|this, _, _, cx| {
                                this.settings.graph.first_parent = !this.settings.graph.first_parent;
                                this.reload(cx);
                            })))
                            .child(toggle_pill("hide-remotes", tr("history.hide_remotes"), !self.settings.graph.show_remotes, cx).on_click(cx.listener(|this, _, _, cx| {
                                this.settings.graph.show_remotes = !this.settings.graph.show_remotes;
                                this.reload(cx);
                            }))),
                    )
                    .when(self.multi_select.len() > 1, |d| {
                        let n = self.multi_select.len();
                        d.child(
                            hstack()
                                .flex_wrap()
                                .gap_2()
                                .px_4()
                                .py_2()
                                .bg(theme.primary.opacity(0.10))
                                .border_b_1()
                                .border_color(theme.primary.opacity(0.35))
                                .child(div().size(px(22.)).rounded_full().flex().items_center().justify_center().bg(theme.primary).text_xs().font_weight(FontWeight::BOLD).text_color(theme.primary_foreground).child(n.to_string()))
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(t!("history.selected_n", n = n).to_string()))
                                .child(div().text_xs().text_color(theme.muted_foreground).child(tr("history.multi_hint")))
                                .child(spacer())
                                .child(crate::widgets::framed(Button::new("multi-copy"), crate::widgets::Tone::Plain, cx).icon(icon(IconName::Copy)).label(tr("history.copy_hashes")).on_click(cx.listener(|this, _, window, cx| this.copy_selected_hashes(window, cx))))
                                .child(crate::widgets::framed(Button::new("multi-copy-list"), crate::widgets::Tone::Plain, cx).icon(icon(IconName::ListOrdered)).label(tr("history.copy_list")).on_click(cx.listener(|this, _, window, cx| this.copy_selected_list(window, cx))))
                                .child(crate::widgets::framed(Button::new("multi-pick"), crate::widgets::Tone::Accent, cx).icon(icon(IconName::GitCommitHorizontal)).label(tr("history.cherry_pick_n")).on_click(cx.listener(|this, _, window, cx| this.cherry_pick_selected(window, cx))))
                                .child(crate::widgets::framed(Button::new("multi-revert"), crate::widgets::Tone::Plain, cx).icon(icon(IconName::Undo2)).label(tr("history.revert_n")).on_click(cx.listener(|this, _, window, cx| this.revert_selected(window, cx))))
                                .child(Button::new("multi-clear").small().ghost().icon(icon(IconName::X)).tooltip(tr("history.clear_selection")).on_click(cx.listener(|this, _, _, cx| {
                                    this.multi_select.clear();
                                    cx.notify();
                                }))),
                        )
                    })
                    .child(
                        hstack()
                            .h(px(30.))
                            .pl_3()
                            .pr_4()
                            .gap(px(10.))
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .border_b_1()
                            .border_color(theme.border.opacity(0.6))
                            .when(self.show_graph, |d| d.child(div().w(px(graph_header_w)).whitespace_nowrap().overflow_hidden().child(tr("history.col_graph").to_uppercase())))
                            .child(div().flex_1().child(tr("history.col_description").to_uppercase()))
                            .when(self.settings.appearance.show_avatars, |d| d.child(div().w(px(22.))))
                            .child(div().w(px(110.)).child(tr("history.col_author").to_uppercase()))
                            .child(div().w(px(76.)).text_right().child(tr("history.col_date").to_uppercase()))
                            .child(div().w(px(64.)).child(tr("history.col_commit").to_uppercase())),
                    )
                    .child(div().flex_1().min_h_0().child(list)),
            )
            .child(
                div()
                    .flex_none()
                    .when(self.detail_file.is_none(), |d| d.w(px(380.)))
                    .when(self.detail_file.is_some(), |d| d.w(relative(0.5)).min_w(px(380.)))
                    .h_full()
                    .border_l_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(self.render_commit_details(window, cx)),
            )
            .into_any_element()
    }

    fn render_commit_details(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(d) = self.details.clone() else {
            return div().size_full().child(empty_state(IconName::MousePointerClick, tr("history.pick_title"), tr("history.pick_body"), cx)).into_any_element();
        };
        if let Some(file) = self.detail_file.clone() {
            return v_flex()
                .size_full()
                .child(
                    hstack()
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            Button::new("back-to-commit")
                                .xsmall()
                                .ghost()
                                .icon(icon(IconName::ArrowLeft))
                                .label(tr("history.back"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.detail_file = None;
                                    cx.notify();
                                })),
                        )
                        .child(div().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(file.path.clone())),
                )
                .child(div().flex_1().min_h_0().child(self.render_file_diff(DiffSource::CommitFile, &file, cx)))
                .into_any_element();
        }
        let short = d.id.chars().take(10).collect::<String>();
        let id_copy = d.id.clone();
        let (adds, dels) = d.files.iter().fold((0, 0), |(a, r), f| (a + f.added.unwrap_or(0), r + f.removed.unwrap_or(0)));
        let commit_info = self.commits.iter().find(|c| c.id == d.id).cloned();

        v_flex()
            .id("commit-details")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .when_some(commit_info, |el, c| el.child(h_flex().flex_wrap().gap_1().children(self.render_ref_chips(&c, cx))))
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(d.summary.clone()))
            .when(!d.body.is_empty(), |el| el.child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(d.body.clone())))
            .child(
                v_flex()
                    .gap_1p5()
                    .text_sm()
                    .child(
                        hstack()
                            .gap_2()
                            .child(avatar(&d.author, px(22.), cx))
                            .child(div().font_weight(FontWeight::MEDIUM).child(d.author.clone()))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(full_time(d.author_time))),
                    )
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().w(px(64.)).text_xs().text_color(theme.muted_foreground).child(tr("history.commit")))
                            .child(div().font_family(theme.mono_font_family.clone()).text_xs().child(short))
                            .child(
                                Button::new("copy-hash")
                                    .xsmall()
                                    .ghost()
                                    .icon(icon(IconName::Copy))
                                    .tooltip(tr("history.copy_hash"))
                                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(id_copy.clone()))),
                            ),
                    )
                    .when(!d.parents.is_empty(), |el| {
                        el.child(
                            hstack()
                                .gap_2()
                                .child(div().w(px(64.)).text_xs().text_color(theme.muted_foreground).child(tr("history.parents")))
                                .children(d.parents.iter().map(|p| {
                                    let pid = p.clone();
                                    div()
                                        .id(SharedString::from(format!("parent-{p}")))
                                        .cursor_pointer()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().link)
                                        .child(p.chars().take(7).collect::<String>())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let id = pid.clone();
                                            if let Some(ix) = this.commits.iter().position(|c| c.id == id) {
                                                this.history_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                                            }
                                            this.select_commit(id, cx);
                                        }))
                                })),
                        )
                    })
                    .child({
                        let ahead = self.status.branch.ahead as usize;
                        let (text, color): (String, Hsla) = match &self.detail_pushed {
                            Some((remote, true)) => (t!("history.pushed_yes", remote = remote.clone()).to_string(), theme.green),
                            Some((remote, false)) => (t!("history.pushed_no", n = ahead.max(1), remote = remote.clone()).to_string(), theme.yellow),
                            None => (t!("history.pushed_local").to_string(), theme.muted_foreground),
                        };
                        hstack()
                            .gap_2()
                            .child(div().w(px(64.)).text_xs().text_color(theme.muted_foreground).child(tr("history.pushed")))
                            .child(div().text_xs().text_color(color).child(text))
                    }),
            )
            .child(div().h(px(1.)).bg(theme.border))
            .child(
                hstack()
                    .gap_2()
                    .text_sm()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::widgets::plural("history.files_changed", d.files.len())))
                    .child(spacer())
                    .child(div().text_xs().text_color(theme.green).child(format!("+{adds}")))
                    .child(div().text_xs().text_color(theme.red).child(format!("−{dels}"))),
            )
            .child(
                v_flex().gap_0p5().children(d.files.iter().enumerate().map(|(i, f)| {
                    let path = f.path.clone();
                    let (dir, name) = match f.path.rsplit_once('/') {
                        Some((dir, name)) => (format!("{dir}/"), name.to_string()),
                        None => (String::new(), f.path.clone()),
                    };
                    let theme = cx.theme().clone();
                    div()
                        .id(("detail-file", i))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .py_1()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().list_hover))
                        .child(status_letter(f.status, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .child(div().flex().child(div().text_color(theme.muted_foreground).child(dir)).child(name)),
                        )
                        .when_some(f.added, |el, a| el.child(div().text_xs().text_color(theme.green).child(format!("+{a}"))))
                        .when_some(f.removed, |el, r| el.child(div().text_xs().text_color(theme.red).child(format!("−{r}"))))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_commit_file(path.clone(), cx)))
                })),
            )
            .into_any_element()
    }
}

