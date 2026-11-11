use cairn_core::graph::{GraphRow, LinePart};
use cairn_core::log::{CommitInfo, RefKind};
use cairn_core::settings::{DateFormat, GraphStyle};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::diff_view::render_file_diff;
use crate::repo_view::RepoView;
use crate::widgets::{avatar, empty_state, full_time, hstack, icon, lane_color, relative_time, spacer, status_letter, tr};

pub(crate) const ROW_H: f32 = 30.;
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
        let selected = self.selected_commit.as_deref() == Some(commit.id.as_str());
        let is_head = commit.refs.iter().any(|r| r.is_head);
        let lanes = self.rows.iter().skip(ix.saturating_sub(40)).take(80).map(|r| r.width).max().unwrap_or(1).min(12);
        let graph_w = GRAPH_PAD * 2. + (lanes.max(1) - 1) as f32 * LANE_W + 4.;
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
            .on_click(cx.listener(move |this, _, _, cx| this.select_commit(id.clone(), cx)))
            .child(
                div()
                    .flex_none()
                    .w(px(graph_w))
                    .h_full()
                    .child(canvas(|_, _, _| {}, move |bounds, _, window, _| {
                        paint_graph(&row, is_head, is_merge, style, bounds, &colors, bg, window)
                    }).size_full()),
            )
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
            .child(
                hstack()
                    .flex_none()
                    .gap_2()
                    .px_3()
                    .when(self.settings.appearance.show_avatars, |d| d.child(avatar(&commit.author, px(20.), cx)))
                    .child(div().w(px(110.)).truncate().text_xs().text_color(theme.muted_foreground).child(commit.author.clone()))
                    .child(div().w(px(76.)).text_right().text_xs().text_color(theme.muted_foreground).child(time))
                    .child(div().w(px(64.)).text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(short)),
            )
            .context_menu(move |menu, _window, _cx| {
                let v = view.clone();
                let id = menu_id.clone();
                crate::commit_menu::build(menu, v, id)
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
                            .h(px(28.))
                            .px_3()
                            .gap_2()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .border_b_1()
                            .border_color(theme.border)
                            .child(crate::widgets::plural("history.commits_shown", count))
                            .child(div().w(px(260.)).child(gpui_kit::component::input::Input::new(&self.search).xsmall().prefix(icon(IconName::Search).xsmall())))
                            .child(spacer())
                            .child(
                                Button::new("first-parent")
                                    .xsmall()
                                    .ghost()
                                    .label(tr("history.first_parent"))
                                    .selected(self.settings.graph.first_parent)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.graph.first_parent = !this.settings.graph.first_parent;
                                        this.reload(cx);
                                    })),
                            )
                            .child(
                                Button::new("show-remotes")
                                    .xsmall()
                                    .ghost()
                                    .label(tr("history.show_remotes"))
                                    .selected(self.settings.graph.show_remotes)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.graph.show_remotes = !this.settings.graph.show_remotes;
                                        this.reload(cx);
                                    })),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(list)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(380.))
                    .h_full()
                    .border_l_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(self.render_commit_details(window, cx)),
            )
            .into_any_element()
    }

    fn render_commit_details(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .child(div().flex_1().min_h_0().child(render_file_diff("commit-file-diff", &file, None, &self.settings, window, cx)))
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

