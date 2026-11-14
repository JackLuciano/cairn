use cairn_core::graph::{GraphRow, LinePart};
use cairn_core::log::{CommitDetails, CommitInfo, RefKind};
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
use crate::widgets::{commit_author_avatar, empty_state, full_time, hstack, icon, lane_color, relative_time, spacer, status_letter, toggle_pill, tr};

pub(crate) const ROW_H: f32 = 34.;
const LANE_W: f32 = 14.;
const MIN_LANE_W: f32 = 6.;
const MAX_GRAPH_W: f32 = 200.;
const GRAPH_PAD: f32 = 10.;
const DIM_ALPHA: f32 = 0.4;
const TEXT_DIM_ALPHA: f32 = 0.72;
const BRANCH_COL_W: f32 = 150.;

const LEGEND_MAX: usize = 8;
const MIN_DESC_W: f32 = 240.;

const CARD_W: f32 = 420.;
const CARD_FILES: usize = 6;
const CARD_BODY_LINES: usize = 6;
const CARD_CACHE_MAX: usize = 400;

type DetailsCache = std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<CommitDetails>>>;

static CARD_DETAILS: std::sync::LazyLock<DetailsCache> = std::sync::LazyLock::new(Default::default);

thread_local! {
    static CARD_HIDDEN_FOR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// What a commit's hover card knows before its message and files have loaded.
#[derive(Clone)]
struct CardInfo {
    id: String,
    summary: String,
    author: String,
    email: String,
    view: WeakEntity<RepoView>,
    time: i64,
    hash: String,
    parents: Vec<String>,
    branch: Option<String>,
    branch_color: Option<Hsla>,
    refs: Vec<(String, IconName, Hsla)>,
    merge: bool,
    on_branch: bool,
    git: cairn_core::Git,
}

/// The card shown when hovering a commit: author, full message, branch, labels and the files it
/// changed. The message and files load in the background and are cached for the next hover.
struct CommitCard {
    info: CardInfo,
    details: Option<std::sync::Arc<CommitDetails>>,
    failed: bool,
    _load: Option<Task<()>>,
}

impl CommitCard {
    /// Creates the card and starts loading the commit's message and files unless they are cached.
    ///
    /// # Arguments
    /// * `info` - what the history row already knows about the commit
    ///
    /// # Returns
    /// The card.
    fn new(info: CardInfo, cx: &mut Context<Self>) -> Self {
        let cached = CARD_DETAILS.lock().ok().and_then(|m| m.get(&info.id).cloned());
        let load = cached.is_none().then(|| {
            let git = info.git.clone();
            let id = info.id.clone();
            cx.spawn(async move |this, cx| {
                let key = id.clone();
                let details = cx.background_executor().spawn(async move { cairn_core::log::commit_details(&git, &id).ok().map(std::sync::Arc::new) }).await;
                if let (Some(d), Ok(mut cache)) = (&details, CARD_DETAILS.lock()) {
                    if cache.len() >= CARD_CACHE_MAX {
                        cache.clear();
                    }
                    cache.insert(key, d.clone());
                }
                let _ = this.update(cx, |card, cx| {
                    card.failed = details.is_none();
                    card.details = details;
                    cx.notify();
                });
            })
        });
        Self { info, details: cached, failed: false, _load: load }
    }

    fn render_files(&self, d: &CommitDetails, cx: &App) -> Div {
        let theme = cx.theme();
        let added: u32 = d.files.iter().filter_map(|f| f.added).sum();
        let removed: u32 = d.files.iter().filter_map(|f| f.removed).sum();
        let count = d.files.len();
        let title = if count == 1 { t!("history.card_file_one").to_string() } else { t!("history.card_files", count = count).to_string() };
        let total = (added + removed).max(1) as f32;
        let mut col = v_flex()
            .gap_1()
            .child(
                hstack()
                    .gap_2()
                    .text_xs()
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(title))
                    .child(spacer())
                    .child(div().font_family(theme.mono_font_family.clone()).text_color(theme.green).child(format!("+{added}")))
                    .child(div().font_family(theme.mono_font_family.clone()).text_color(theme.red).child(format!("−{removed}")))
                    .child(
                        h_flex()
                            .w(px(48.))
                            .h(px(5.))
                            .rounded(px(3.))
                            .overflow_hidden()
                            .bg(theme.border)
                            .child(div().h_full().w(relative(added as f32 / total)).bg(theme.green))
                            .child(div().h_full().w(relative(removed as f32 / total)).bg(theme.red)),
                    ),
            );
        for f in d.files.iter().take(CARD_FILES) {
            let (dir, name) = match f.path.rsplit_once('/') {
                Some((dir, name)) => (format!("{dir}/"), name.to_string()),
                None => (String::new(), f.path.clone()),
            };
            col = col.child(
                hstack()
                    .gap_2()
                    .h(px(22.))
                    .text_xs()
                    .child(status_letter(f.status, cx))
                    .child(h_flex().flex_1().min_w_0().overflow_hidden().child(div().flex_shrink(1.).min_w_0().truncate().text_color(theme.muted_foreground).child(dir)).child(div().flex_none().text_color(theme.foreground).child(name)))
                    .when_some(f.added.filter(|n| *n > 0), |d, n| d.child(div().flex_none().font_family(theme.mono_font_family.clone()).text_color(theme.green).child(format!("+{n}"))))
                    .when_some(f.removed.filter(|n| *n > 0), |d, n| d.child(div().flex_none().font_family(theme.mono_font_family.clone()).text_color(theme.red).child(format!("−{n}")))),
            );
        }
        if count > CARD_FILES {
            col = col.child(div().pl(px(26.)).text_xs().text_color(theme.muted_foreground).child(t!("history.card_more_files", count = count - CARD_FILES).to_string()));
        }
        col
    }
}

impl Render for CommitCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if CARD_HIDDEN_FOR.with(|h| h.borrow().as_deref() == Some(self.info.id.as_str())) {
            return div().into_any_element();
        }
        let theme = cx.theme().clone();
        let info = &self.info;
        let (show_avatars, load_gravatar) = info.view.upgrade().map(|view| {
            let appearance = &view.read(cx).settings.appearance;
            (appearance.show_avatars, appearance.load_gravatar)
        }).unwrap_or_default();
        let color = info.branch_color.unwrap_or(theme.primary);
        let mono = theme.mono_font_family.clone();
        let body = self.details.as_ref().map(|d| {
            let lines: Vec<&str> = d.body.lines().collect();
            let mut text = lines.iter().take(CARD_BODY_LINES).copied().collect::<Vec<_>>().join("\n");
            if lines.len() > CARD_BODY_LINES {
                text.push_str("\n…");
            }
            text
        });
        let signed = self.details.as_ref().is_some_and(|d| d.signature == cairn_core::log::SignatureStatus::Good);
        let chips = h_flex()
            .flex_wrap()
            .gap_1p5()
            .items_center()
            .when_some(info.branch.clone(), |d, b| {
                d.child(
                    hstack()
                        .gap_1p5()
                        .px_2()
                        .h(px(22.))
                        .rounded(px(11.))
                        .border_1()
                        .border_color(color.opacity(0.5))
                        .bg(color.opacity(0.16))
                        .text_xs()
                        .font_family(mono.clone())
                        .text_color(color)
                        .child(div().size(px(7.)).rounded_full().bg(color))
                        .child(b),
                )
            })
            .child(if info.on_branch { crate::widgets::badge(tr("history.card_on_branch"), theme.green) } else { crate::widgets::badge(tr("history.card_off_branch"), theme.yellow) })
            .when(info.merge, |d| d.child(crate::widgets::badge(tr("history.card_merge"), theme.magenta)))
            .when(signed, |d| d.child(crate::widgets::badge(tr("history.card_signed"), theme.green)));
        let labels: Vec<&(String, IconName, Hsla)> = info.refs.iter().filter(|(n, _, _)| Some(n) != info.branch.as_ref()).collect();
        let more_labels = labels.len().saturating_sub(8);
        let relative = relative_time(info.time);
        let exact = full_time(info.time);
        let parents = info.parents.iter().map(|p| p.chars().take(7).collect::<String>()).collect::<Vec<_>>().join(" ");

        v_flex()
            .w(px(CARD_W))
            .rounded(px(14.))
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .font_family(theme.font_family.clone())
            .shadow(crate::widgets::soft_shadow(cx))
            .child(
                v_flex()
                    .gap_2p5()
                    .px_4()
                    .pt_3()
                    .pb_3()
                    .child(
                        hstack()
                            .gap_2p5()
                            .when(show_avatars, |d| d.child(commit_author_avatar(&info.author, &info.email, px(32.), load_gravatar, cx)))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(info.author.clone()))
                                    .child(div().text_xs().text_color(theme.muted_foreground).truncate().child(info.email.clone())),
                            )
                            .child(
                                v_flex()
                                    .flex_none()
                                    .items_end()
                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(relative.clone()))
                                    .when(!exact.starts_with(relative.as_ref()), |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(exact.clone()))),
                            ),
                    )
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).line_height(rems(1.35)).whitespace_normal().child(info.summary.clone()))
                    .when_some(body.filter(|b| !b.is_empty()), |d, b| {
                        d.child(div().pl_2p5().border_l_2().border_color(theme.border).text_xs().line_height(rems(1.25)).text_color(theme.muted_foreground).whitespace_normal().child(b))
                    })
                    .child(chips)
                    .when(!labels.is_empty(), |d| {
                        d.child(
                            h_flex()
                                .flex_wrap()
                                .gap_1()
                                .children(labels.iter().take(8).map(|(name, ic, c)| {
                                    hstack()
                                        .flex_none()
                                        .gap_1()
                                        .px(px(6.))
                                        .h(px(18.))
                                        .rounded(px(4.))
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(*c)
                                        .bg(c.opacity(0.12))
                                        .border_1()
                                        .border_color(c.opacity(0.35))
                                        .child(icon(*ic).xsmall())
                                        .child(name.clone())
                                }))
                                .when(more_labels > 0, |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(format!("+{more_labels}")))),
                        )
                    }),
            )
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border.opacity(0.6))
                    .map(|d| match &self.details {
                        Some(details) => d.child(self.render_files(details, cx)),
                        None if self.failed => d.child(div().text_xs().text_color(theme.muted_foreground).child(tr("history.card_no_details"))),
                        None => d.child(crate::skeleton::card_files(cx)),
                    }),
            )
            .child(
                hstack()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(theme.border.opacity(0.6))
                    .bg(theme.muted.opacity(0.35))
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(icon(IconName::GitCommitHorizontal).xsmall())
                    .child(div().font_family(mono.clone()).text_color(theme.foreground).child(info.hash.clone()))
                    .when(!parents.is_empty(), |d| d.child(div().font_family(mono.clone()).child(format!("← {parents}"))))
                    .child(spacer())
                    .child(div().truncate().child(tr("history.card_hint"))),
            )
            .into_any_element()
    }
}

/// Everything derived from the loaded commits for drawing History: lanes, branch names and
/// colors. Worked out off the UI thread so large histories stay smooth.
pub(crate) struct HistoryLayout {
    pub rows: Vec<GraphRow>,
    pub owners: Vec<Option<String>>,
    pub palette: Vec<usize>,
    pub owner_colors: std::collections::HashMap<String, usize>,
}

impl HistoryLayout {
    /// Lays out the graph for the given commits.
    ///
    /// # Arguments
    /// * `commits` - the loaded history, newest first
    /// * `focus` - the branch to highlight; the checked-out one when `None`
    /// * `searching` - whether the list is filtered by a search
    ///
    /// # Returns
    /// The finished layout.
    pub(crate) fn compute(commits: &[CommitInfo], focus: Option<&str>, searching: bool) -> Self {
        let hot = crate::repo_view::focus_hot(commits, focus, searching);
        let owners = if searching { vec![None; commits.len()] } else { cairn_core::graph::branch_owners(commits) };
        let (palette, owner_colors) = cairn_core::graph::owner_colors(&owners);
        let rows = cairn_core::graph::layout_focused(commits, hot.as_deref(), (!searching).then_some(palette.as_slice()));
        Self { rows, owners, palette, owner_colors }
    }

    /// Puts the layout on the view, unless the commits changed in the meantime.
    pub(crate) fn apply(self, view: &mut RepoView) {
        if self.rows.len() != view.commits.len() {
            return;
        }
        view.rows = self.rows;
        view.owners = self.owners;
        view.palette = self.palette;
        view.owner_colors = self.owner_colors;
    }
}

/// Works out lane spacing and column width so the whole graph fits a bounded column.
///
/// # Arguments
/// * `lanes` - the widest row's lane count
///
/// # Returns
/// `(lane spacing, column width)` in pixels.
fn graph_metrics(lanes: usize) -> (f32, f32) {
    let gaps = lanes.saturating_sub(1) as f32;
    let lane_w = if gaps == 0. { LANE_W } else { ((MAX_GRAPH_W - GRAPH_PAD * 2. - 4.) / gaps).clamp(MIN_LANE_W, LANE_W) };
    (lane_w, (GRAPH_PAD * 2. + gaps * lane_w + 4.).clamp(52., MAX_GRAPH_W))
}

#[allow(clippy::too_many_arguments)]
fn paint_graph(row: &GraphRow, is_head: bool, is_merge: bool, style: GraphStyle, lane_w: f32, bounds: Bounds<Pixels>, colors: &[Hsla], bg: Hsla, window: &mut Window) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| paint_graph_clipped(row, is_head, is_merge, style, lane_w, bounds, colors, bg, window));
}

#[allow(clippy::too_many_arguments)]
fn paint_graph_clipped(row: &GraphRow, is_head: bool, is_merge: bool, style: GraphStyle, lane_w: f32, bounds: Bounds<Pixels>, colors: &[Hsla], bg: Hsla, window: &mut Window) {
    let top = bounds.top();
    let bottom = bounds.bottom();
    let mid = top + (bottom - top) / 2.;
    let x = |lane: usize| bounds.left() + px(GRAPH_PAD + lane as f32 * lane_w);
    let color = |i: usize| colors[i % colors.len()];
    let faded = |c: Hsla, dim: bool| if dim { c.opacity(DIM_ALPHA) } else { c };
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
            window.paint_path(p, faded(color(line.color), line.dim));
        }
    }
    let c = point(x(row.lane), mid);
    let dot_color = faded(color(row.color), row.dim);
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

    /// Highlights one branch's commits in the history and fades the rest.
    ///
    /// # Arguments
    /// * `name` - the branch to highlight, or `None` for the checked-out branch; picking the
    ///   highlighted branch again goes back to the checked-out branch
    pub(crate) fn set_focus_branch(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.focus_branch = if name.is_some() && name == self.focus_branch { None } else { name };
        if self.focus_branch.as_deref().is_some_and(|n| self.current_branch() == Some(n)) {
            self.focus_branch = None;
        }
        let searching = !self.search.read(cx).value().trim().is_empty();
        let commits = self.commits.clone();
        let focus = self.focus_branch.clone();
        cx.notify();
        self.spawn_bg(move || HistoryLayout::compute(&commits, focus.as_deref(), searching), |this, laid, _| laid.apply(this), cx);
    }

    /// The color a branch is drawn in, matching its line in the graph.
    ///
    /// # Arguments
    /// * `name` - a local or remote branch name
    ///
    /// # Returns
    /// The branch's lane color, or `None` when it owns no loaded commit.
    pub(crate) fn branch_color(&self, name: &str, cx: &App) -> Option<Hsla> {
        let ix = self.owner_colors.get(name).or_else(|| name.split_once('/').and_then(|(_, b)| self.owner_colors.get(b))).or_else(|| {
            let remote = self.remotes.iter().map(|r| format!("{}/{name}", r.name)).find(|n| self.owner_colors.contains_key(n))?;
            self.owner_colors.get(&remote)
        })?;
        Some(lane_color(*ix, cx))
    }

    /// Whether the history has commits of more than one branch, so the branch column helps.
    fn shows_branch_column(&self) -> bool {
        let mut names = self.owners.iter().flatten();
        let first = names.next();
        first.is_some() && names.any(|n| Some(n) != first)
    }

    /// Which optional columns fit next to a readable commit message at the list's current width.
    ///
    /// # Returns
    /// `(branch column, author column)`.
    fn fitting_columns(&self) -> (bool, bool) {
        let branch_wanted = self.shows_branch_column();
        let width = self.history_width.get();
        if width <= 0. {
            return (branch_wanted, true);
        }
        let graph = if self.show_graph { self.graph_w + 10. } else { 0. };
        let free = width - 28. - graph - 86. - 74.;
        let author = 120. + if self.settings.appearance.show_avatars { 32. } else { 0. };
        let branch = BRANCH_COL_W + 10.;
        let show_branch = branch_wanted && free - branch >= MIN_DESC_W;
        let show_author = free - author - if show_branch { branch } else { 0. } >= MIN_DESC_W;
        (show_branch, show_author)
    }

    /// Loads another page of history once the list is scrolled near its end.
    ///
    /// # Arguments
    /// * `last_visible` - index of the last row being drawn
    fn maybe_load_more(&mut self, last_visible: usize, cx: &mut Context<Self>) {
        let page = self.settings.graph.page_size.max(50);
        let limit = self.history_limit.max(page);
        if self.loading_more || self.commits.len() < limit || last_visible + 20 < self.commits.len() {
            return;
        }
        self.history_limit = limit + page;
        self.loading_more = true;
        let git = self.git();
        let settings = self.settings.clone();
        let have = self.commits.clone();
        let focus = self.focus_branch.clone();
        let opts = cairn_core::log::LogOptions {
            scope: if settings.graph.all_branches { cairn_core::log::LogScope::AllRefs } else { cairn_core::log::LogScope::CurrentBranch },
            limit: page,
            skip: have.len(),
            first_parent: settings.graph.first_parent,
            include_remotes: settings.graph.show_remotes,
            include_stashes: settings.graph.show_stashes,
            ..Default::default()
        };
        self.spawn_bg(
            move || {
                let more = cairn_core::log::load_log(&git, &opts).unwrap_or_default();
                let mut all = have;
                let known: std::collections::HashSet<String> = all.iter().map(|c| c.id.clone()).collect();
                all.extend(more.into_iter().filter(|c| !known.contains(&c.id)));
                let laid = HistoryLayout::compute(&all, focus.as_deref(), false);
                (all, laid)
            },
            |this, (all, laid), _| {
                this.loading_more = false;
                if all.len() >= this.commits.len() {
                    this.commits = all;
                    laid.apply(this);
                }
            },
            cx,
        );
    }

    /// Commits that arrived since the last load, shown with a short slide-in.
    ///
    /// # Arguments
    /// * `before` - the commit ids shown until now
    pub(crate) fn mark_fresh(&mut self, before: &std::collections::HashSet<String>, cx: &mut Context<Self>) {
        if before.is_empty() || self.settings.appearance.reduce_motion {
            return;
        }
        self.fresh = self.commits.iter().take(50).filter(|c| !before.contains(&c.id)).map(|c| c.id.clone()).collect();
        if self.fresh.is_empty() {
            return;
        }
        self.spawn_bg(|| std::thread::sleep(std::time::Duration::from_millis(1200)), |this, _, _| this.fresh.clear(), cx);
    }

    fn render_legend(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shows_branch_column() {
            return None;
        }
        let theme = cx.theme().clone();
        let range = if self.history_visible.is_empty() { 0..self.owners.len().min(30) } else { self.history_visible.start..self.history_visible.end.min(self.owners.len()) };
        let mut names: Vec<(String, usize)> = Vec::new();
        for owner in self.owners[range].iter().flatten() {
            if !names.iter().any(|(n, _)| n == owner)
                && let Some(ix) = self.owner_colors.get(owner)
            {
                names.push((owner.clone(), *ix));
            }
        }
        let labelled: Vec<(String, String, bool, usize)> = names
            .into_iter()
            .map(|(name, ix)| {
                let remote = self.remotes.iter().find_map(|r| name.strip_prefix(&format!("{}/", r.name)).map(str::to_string));
                let is_remote = remote.is_some();
                (remote.unwrap_or_else(|| name.clone()), name, is_remote, ix)
            })
            .collect();
        let width = self.history_width.get();
        let mut budget = if width > 0. { width - 32. - 80. - 44. } else { f32::MAX };
        let mut fits = 0;
        for (short, _, is_remote, _) in labelled.iter().take(LEGEND_MAX) {
            let chip = (34. + short.chars().count() as f32 * 7.2 + if *is_remote { 16. } else { 0. }).min(240.) + 6.;
            if chip > budget && fits > 0 {
                break;
            }
            budget -= chip;
            fits += 1;
        }
        let hidden = SharedString::from(labelled.iter().skip(fits).map(|(_, n, _, _)| n.as_str()).collect::<Vec<_>>().join("\n"));
        let total = labelled.len();
        let focus = self.focus_branch.clone().or_else(|| self.current_branch().map(str::to_string));
        let mut row = hstack()
            .gap_1p5()
            .px_4()
            .py(px(6.))
            .border_b_1()
            .border_color(theme.border.opacity(0.6))
            .w_full()
            .min_w_0()
            .flex_none()
            .overflow_hidden()
            .child(div().flex_none().text_xs().text_color(theme.muted_foreground).mr_1().child(tr("history.legend_in_view")));
        for (k, (short, name, is_remote, ix)) in labelled.into_iter().take(fits).enumerate() {
            let color = lane_color(ix, cx);
            let on = focus.as_deref() == Some(name.as_str());
            let pick = name.clone();
            let full = SharedString::from(name);
            row = row.child(
                hstack()
                    .id(("legend", k))
                    .flex_shrink(1.)
                    .min_w(px(48.))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx))
                    .gap_1p5()
                    .h(px(22.))
                    .px_2()
                    .rounded(px(11.))
                    .cursor_pointer()
                    .text_xs()
                    .font_family(theme.mono_font_family.clone())
                    .border_1()
                    .border_color(if on { color } else { color.opacity(0.3) })
                    .bg(color.opacity(if on { 0.2 } else { 0.07 }))
                    .text_color(if on { theme.foreground } else { theme.muted_foreground })
                    .when(on, |d| d.shadow(crate::widgets::glow(color, 8.)))
                    .hover(move |s| s.bg(color.opacity(0.16)).text_color(color))
                    .active(|s| s.opacity(crate::widgets::PRESSED))
                    .max_w(px(240.))
                    .child(div().flex_none().size(px(7.)).rounded_full().bg(color))
                    .when(is_remote, |d| d.child(icon(IconName::Cloud).xsmall().flex_none()))
                    .child(div().min_w_0().truncate().child(short))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.focus_branch = None;
                        this.set_focus_branch(Some(pick.clone()), cx);
                    })),
            );
        }
        if total > fits {
            row = row.child(
                div()
                    .id("legend-more")
                    .flex_none()
                    .px_1p5()
                    .rounded(px(8.))
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.muted))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(hidden.clone()).build(window, cx))
                    .child(format!("+{}", total - fits)),
            );
        }
        Some(row.into_any_element())
    }

    fn render_ref_chips(&self, c: &CommitInfo, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let focus = self.focus_branch.clone();
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
                let color = match r.kind {
                    RefKind::LocalBranch | RefKind::RemoteBranch => self.branch_color(&r.name, cx).unwrap_or(color),
                    _ => color,
                };
                let focused = focus.as_deref() == Some(r.name.as_str());
                let chip = hstack()
                    .id(SharedString::from(format!("chip-{}-{}", c.id, r.name)))
                    .flex_none()
                    .gap_1()
                    .px(px(6.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if r.is_head || focused { theme.primary_foreground } else { color })
                    .bg(if r.is_head { color } else if focused { theme.magenta } else { color.opacity(0.14) })
                    .border_1()
                    .border_color(if focused { theme.magenta } else { color.opacity(0.45) })
                    .child(icon(ic).xsmall())
                    .child(r.name.clone());
                let name = r.name.clone();
                let chip = if matches!(r.kind, RefKind::LocalBranch | RefKind::RemoteBranch) {
                    let pick = name.clone();
                    chip.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_focus_branch(Some(pick.clone()), cx);
                    }))
                } else {
                    chip
                };
                if r.kind == RefKind::LocalBranch {
                    chip.on_drag(crate::drag::DragBranch { name: name.clone() }, move |_, _, _, cx| crate::drag::DragPreview::new(name.clone(), IconName::GitBranch, cx))
                        .into_any_element()
                } else {
                    chip.into_any_element()
                }
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
        let lane_w = self.graph_lane_w;
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
        let faded = row.dim && !selected;
        let (show_branch, show_author) = self.fitting_columns();
        let branch_col = show_branch.then(|| self.owners.get(ix).cloned().flatten());
        let continues = ix > 0 && self.owners.get(ix - 1) == self.owners.get(ix) && !commit.refs.iter().any(|r| matches!(r.kind, RefKind::LocalBranch | RefKind::RemoteBranch));
        let text = if faded { theme.foreground.opacity(TEXT_DIM_ALPHA) } else { theme.foreground };
        let muted = if faded { theme.muted_foreground.opacity(TEXT_DIM_ALPHA + 0.1) } else { theme.muted_foreground };

        let card = self.card_info(ix, cx);
        let fresh = self.fresh.contains(&commit.id);
        let row_el = div()
            .id(("commit", ix))
            .h(px(ROW_H))
            .w_full()
            .flex()
            .items_center()
            .bg(bg)
            .when(selected, |d| d.bg(linear_gradient(90., linear_color_stop(theme.primary.opacity(0.22), 0.), linear_color_stop(bg, 0.45))).border_l_2().border_color(theme.primary))
            .tooltip(move |_, cx| cx.new(|cx| CommitCard::new(card.clone(), cx)).into())
            .on_mouse_down(MouseButton::Right, {
                let id = commit.id.clone();
                move |_, window, _| {
                    CARD_HIDDEN_FOR.with(|h| *h.borrow_mut() = Some(id.clone()));
                    window.refresh();
                }
            })
            .on_hover(|hovered, _, _| {
                if *hovered {
                    CARD_HIDDEN_FOR.with(|h| h.borrow_mut().take());
                }
            })
            .border_b_1()
            .border_color(theme.border.opacity(0.4))
            .when(!selected, |d| d.hover(|s| s.bg(cx.theme().list_hover)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| this.click_commit(ix, id.clone(), ev.modifiers(), cx)))
            .on_drag(
                crate::drag::DragCommits { ids: if self.multi_select.contains(&commit.id) { self.multi_select.clone() } else { vec![commit.id.clone()] } },
                |d, _, _, cx| crate::drag::DragPreview::new(crate::widgets::plural("drag.commits", d.ids.len()), IconName::GitCommitHorizontal, cx),
            )
            .drag_over::<crate::drag::DragBranch>(move |s, _, _, cx| s.bg(cx.theme().primary.opacity(0.16)))
            .drag_over::<crate::drag::DragCommits>(move |s, _, _, cx| s.bg(cx.theme().primary.opacity(0.16)))
            .on_drop(cx.listener({
                let target = commit.id.clone();
                move |this, d: &crate::drag::DragBranch, window, cx| this.drop_branch(d.name.clone(), target.clone(), window, cx)
            }))
            .on_drop(cx.listener({
                let target = commit.id.clone();
                move |this, d: &crate::drag::DragCommits, window, cx| this.drop_commits(d.ids.clone(), target.clone(), window, cx)
            }))
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
                            paint_graph(&row, is_head, is_merge, style, lane_w, bounds, &colors, bg, window)
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
                            .text_color(text)
                            .child(commit.summary.clone()),
                    ),
            )
            .when_some(branch_col, |d, owner| {
                let color = owner.as_deref().and_then(|o| self.branch_color(o, cx)).unwrap_or(theme.muted_foreground);
                let color = if faded { color.opacity(TEXT_DIM_ALPHA + 0.1) } else { color };
                d.child(
                    hstack()
                        .flex_none()
                        .w(px(BRANCH_COL_W))
                        .gap_1p5()
                        .overflow_hidden()
                        .child(div().flex_none().w(px(3.)).h(px(16.)).rounded(px(2.)).bg(color.opacity(if continues { 0.45 } else { 1. })))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .font_family(theme.mono_font_family.clone())
                                .text_color(if continues { color.opacity(0.6) } else { color })
                                .when(!continues, |d| d.font_weight(FontWeight::SEMIBOLD))
                                .child(owner.unwrap_or_else(|| "·".into())),
                        ),
                )
            })
            .when(show_author && self.settings.appearance.show_avatars, |d| d.child(div().flex_none().when(faded, |d| d.opacity(TEXT_DIM_ALPHA)).child(commit_author_avatar(&commit.author, &commit.email, px(22.), self.settings.appearance.load_gravatar, cx))))
            .when(show_author, |d| d.child(div().flex_none().w(px(110.)).truncate().text_xs().text_color(muted).child(commit.author.clone())))
            .child(div().flex_none().w(px(76.)).text_right().text_xs().text_color(muted).child(time))
            .child(div().flex_none().w(px(64.)).text_xs().font_family(theme.mono_font_family.clone()).text_color(muted).child(short))
            .context_menu(move |menu, _window, cx| {
                let v = view.clone();
                let id = menu_id.clone();
                let selected = v.read(cx).multi_select.clone();
                if selected.len() > 1 && selected.contains(&id) {
                    crate::commit_menu::build_multi(menu, v, selected.len())
                } else {
                    let heads = v.read(cx).pr_heads(&id);
                    crate::commit_menu::build(menu, v, id, heads)
                }
            });
        if fresh {
            let tint = theme.primary;
            row_el
                .with_animation(SharedString::from(format!("fresh-{}", commit.id)), Animation::new(std::time::Duration::from_millis(520)).with_easing(ease_out_quint()), move |d, t| d.opacity(0.2 + 0.8 * t).pl(px(12. + 18. * (1. - t))).bg(tint.opacity(0.18 * (1. - t))))
                .into_any_element()
        } else {
            row_el.into_any_element()
        }
    }

    /// Branches pointing at a commit that a pull request could be opened from: not the main
    /// branch, and without an open pull request yet. A local branch wins over its remote copy.
    ///
    /// # Arguments
    /// * `id` - the commit's hash
    ///
    /// # Returns
    /// `(ref, name shown)` pairs.
    pub(crate) fn pr_heads(&self, id: &str) -> Vec<(String, String)> {
        let Some(commit) = self.commits.iter().find(|c| c.id == id) else {
            return Vec::new();
        };
        let trunk = self.trunk.as_deref().map(|t| self.remotes.iter().find_map(|r| t.strip_prefix(&format!("{}/", r.name))).unwrap_or(t)).unwrap_or("main");
        let mut heads: Vec<(String, String)> = Vec::new();
        for r in commit.refs.iter().filter(|r| r.kind == RefKind::LocalBranch) {
            heads.push((r.name.clone(), r.name.clone()));
        }
        for r in commit.refs.iter().filter(|r| r.kind == RefKind::RemoteBranch) {
            let short = self.remotes.iter().find_map(|rm| r.name.strip_prefix(&format!("{}/", rm.name))).unwrap_or(&r.name).to_string();
            if short != "HEAD" && !heads.iter().any(|(_, n)| *n == short) {
                heads.push((r.name.clone(), short));
            }
        }
        heads.retain(|(_, n)| n != trunk && !matches!(n.as_str(), "main" | "master") && !self.prs.iter().any(|p| &p.head == n));
        heads.truncate(3);
        heads
    }

    /// Gathers what the hover card of a history row shows.
    ///
    /// # Arguments
    /// * `ix` - the row's index
    ///
    /// # Returns
    /// The card's data.
    fn card_info(&self, ix: usize, cx: &Context<Self>) -> CardInfo {
        let commit = &self.commits[ix];
        let theme = cx.theme();
        let owner = self.owners.get(ix).cloned().flatten();
        CardInfo {
            id: commit.id.clone(),
            summary: commit.summary.clone(),
            author: commit.author.clone(),
            email: commit.email.clone(),
            view: cx.entity().downgrade(),
            time: commit.time,
            hash: commit.id.chars().take(10).collect(),
            parents: commit.parents.clone(),
            branch_color: owner.as_deref().and_then(|o| self.branch_color(o, cx)),
            branch: owner,
            refs: commit
                .refs
                .iter()
                .map(|r| {
                    let (fallback, ic) = match r.kind {
                        RefKind::LocalBranch => (theme.primary, IconName::GitBranch),
                        RefKind::RemoteBranch => (theme.muted_foreground, IconName::Cloud),
                        RefKind::Tag => (theme.yellow, IconName::Tag),
                        RefKind::Stash => (theme.magenta, IconName::Archive),
                        RefKind::Head => (theme.red, IconName::CircleDot),
                    };
                    let color = match r.kind {
                        RefKind::LocalBranch | RefKind::RemoteBranch => self.branch_color(&r.name, cx).unwrap_or(fallback),
                        _ => fallback,
                    };
                    (r.name.clone(), ic, color)
                })
                .collect(),
            merge: commit.is_merge(),
            on_branch: !self.rows.get(ix).is_some_and(|r| r.dim),
            git: self.git(),
        }
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
        let lanes = self.rows.iter().map(|r| r.width).max().unwrap_or(1).max(1);
        let (lane_w, graph_header_w) = graph_metrics(lanes);
        self.graph_w = graph_header_w;
        self.graph_lane_w = lane_w;
        let list = uniform_list(
            "history-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                if this.history_visible != range {
                    this.history_visible = range.clone();
                    cx.notify();
                }
                this.maybe_load_more(range.end, cx);
                range.map(|ix| this.render_commit_row(ix, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.history_scroll)
        .size_full();
        let legend = self.render_legend(cx);
        let debug_card = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_CARD").ok().and_then(|v| v.parse::<usize>().ok()).filter(|ix| *ix < count) } else { None };
        let debug_card = debug_card.map(|ix| {
            let info = self.card_info(ix, cx);
            window.use_keyed_state(("debug-card", ix), cx, move |_, cx| CommitCard::new(info, cx))
        });

        let (show_branch, show_author) = self.fitting_columns();
        let measured = self.history_width.clone();
        let me = cx.entity_id();
        let main = v_flex()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
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
                    .when_some(debug_card, |d, card| d.child(deferred(div().absolute().top(px(150.)).left(px(260.)).child(card))))
                    .child(
                        hstack()
                            .flex_wrap()
                            .px_4()
                            .py(px(10.))
                            .gap_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .child({
                                let all = self.settings.graph.all_branches;
                                let segment = |id: &'static str, label: SharedString, on: bool, value: bool, cx: &mut Context<Self>| {
                                    let theme = cx.theme().clone();
                                    div()
                                        .id(id)
                                        .flex()
                                        .items_center()
                                        .h(px(26.))
                                        .px(px(12.))
                                        .rounded(px(6.))
                                        .text_sm()
                                        .whitespace_nowrap()
                                        .cursor_pointer()
                                        .when(on, |d| d.bg(theme.background).text_color(theme.foreground).font_weight(FontWeight::SEMIBOLD).shadow_xs())
                                        .when(!on, |d| d.text_color(theme.muted_foreground).hover(|s| s.text_color(theme.foreground)))
                                        .child(label)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if this.settings.graph.all_branches != value {
                                                this.settings.graph.all_branches = value;
                                                cx.emit(crate::repo_view::HistoryScopeEvent(value));
                                                this.reload(cx);
                                            }
                                        }))
                                };
                                hstack()
                                    .flex_none()
                                    .p(px(2.))
                                    .gap(px(2.))
                                    .rounded(px(8.))
                                    .bg(theme.secondary)
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(segment("scope-branch", tr("history.this_branch"), !all, false, cx))
                                    .child(segment("scope-all", tr("history.all_branches"), all, true, cx))
                            })
                            .child(div().flex_none().whitespace_nowrap().text_sm().text_color(theme.muted_foreground).child(crate::widgets::plural("history.commits_shown", count)))
                            .when(self.loading_more, |d| d.child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(tr("history.loading_more"))))
                            .when(self.settings.graph.all_branches, |d| {
                                let view = cx.entity();
                                let current = self.current_branch().map(str::to_string);
                                let shown = self.focus_branch.clone().or(current.clone()).unwrap_or_else(|| "HEAD".into());
                                let other = self.focus_branch.is_some();
                                d.child(
                                    crate::widgets::framed(Button::new("history-focus"), if other { crate::widgets::Tone::Accent } else { crate::widgets::Tone::Plain }, cx)
                                        .h(px(28.))
                                        .text_xs()
                                        .icon(icon(IconName::Highlighter))
                                        .label(SharedString::from(t!("history.highlight", name = shown).to_string()))
                                        .tooltip(tr("history.highlight_hint"))
                                        .dropdown_caret(true)
                                        .on_click(move |_, window, cx| crate::branch_picker::open(view.clone(), crate::branch_picker::Mode::Highlight, window, cx)),
                                )
                                .when(other, |d| {
                                    d.child(Button::new("history-focus-clear").xsmall().ghost().icon(icon(IconName::X)).tooltip(tr("history.highlight_clear")).on_click(cx.listener(|this, _, _, cx| this.set_focus_branch(None, cx))))
                                })
                            })
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
                            .when(self.settings.graph.all_branches, |d| {
                                d.child(toggle_pill("hide-remotes", tr("history.hide_remotes"), !self.settings.graph.show_remotes, cx).on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.graph.show_remotes = !this.settings.graph.show_remotes;
                                    this.reload(cx);
                                })))
                            }),
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
                    .children(legend)
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
                            .when(show_branch, |d| d.child(div().w(px(BRANCH_COL_W)).child(tr("history.col_branch").to_uppercase())))
                            .when(show_author && self.settings.appearance.show_avatars, |d| d.child(div().w(px(22.))))
                            .when(show_author, |d| d.child(div().w(px(110.)).child(tr("history.col_author").to_uppercase())))
                            .child(div().w(px(76.)).text_right().child(tr("history.col_date").to_uppercase()))
                            .child(div().w(px(64.)).child(tr("history.col_commit").to_uppercase())),
                    )
                    .child(div().flex_1().min_h_0().child(list));
        let details = div().size_full().border_l_1().border_color(theme.border).bg(theme.sidebar).child(self.render_commit_details(window, cx));
        if self.detail_file.is_some() {
            return h_flex().size_full().child(main).child(div().flex_none().w(relative(0.5)).min_w(px(380.)).h_full().child(details)).into_any_element();
        }
        let view = cx.entity();
        gpui_kit::component::resizable::h_resizable("history-split")
            .child(gpui_kit::component::resizable::resizable_panel().child(main.size_full()))
            .child(gpui_kit::component::resizable::resizable_panel().size(px(self.settings.appearance.details_width.clamp(300., 900.))).size_range(px(300.)..px(900.)).child(details))
            .on_resize(move |state, _, cx| {
                if let Some(w) = state.read(cx).sizes().get(1).copied() {
                    view.update(cx, |v, cx| v.remember_panel(Some(f32::from(w)), None, cx));
                }
            })
            .into_any_element()
    }

    fn render_commit_details(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let Some(d) = self.details.clone() else {
            if self.selected_commit.is_some() {
                return crate::skeleton::details(cx).into_any_element();
            }
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
                            .when(self.settings.appearance.show_avatars, |row| row.child(commit_author_avatar(&d.author, &d.email, px(22.), self.settings.appearance.load_gravatar, cx)))
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
                        .context_menu({
                            let view = cx.entity();
                            let path = f.path.clone();
                            let id = d.id.clone();
                            let parent = d.parents.first().cloned().unwrap_or_else(|| "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into());
                            move |menu, _, _| {
                                let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
                                let (p1, p2, p3) = (path.clone(), path.clone(), path.clone());
                                let (id, parent) = (id.clone(), parent.clone());
                                menu.item(gpui_kit::component::menu::PopupMenuItem::new(t!("actions.file_history").to_string()).icon(icon(IconName::ScrollText)).on_click(move |_, _, cx| {
                                    let p = p1.clone();
                                    v1.update(cx, |v, cx| v.open_file_history(p, cx));
                                }))
                                .item(gpui_kit::component::menu::PopupMenuItem::new(t!("changes.line_history").to_string()).icon(icon(IconName::FileClock)).on_click(move |_, _, cx| {
                                    let p = p2.clone();
                                    v2.update(cx, |v, cx| v.open_blame(p, cx));
                                }))
                                .item(gpui_kit::component::menu::PopupMenuItem::new(t!("actions.open_difftool").to_string()).icon(icon(IconName::ArrowLeftRight)).on_click(move |_, window, cx| {
                                    let what = cairn_core::ops::ToolDiff::Revisions(parent.clone(), id.clone());
                                    let p = p3.clone();
                                    v3.update(cx, |v, cx| v.open_difftool(what, p, window, cx));
                                }))
                            }
                        })
                })),
            )
            .into_any_element()
    }
}

