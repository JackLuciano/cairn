use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cairn_core::diff::{DiffLine, FileDiff, LineKind};
use cairn_core::patch;
use cairn_core::settings::{DiffHighlight, DiffLayout, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{
    Diff, DiffDecoration, DiffFile, DiffGutterMarker, DiffHoverHighlight, DiffInlineUnit, DiffLinePosition, DiffLineRange, DiffMode,
    DiffParseOptions, DiffSide, DiffState,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::RepoView;
use crate::widgets::{empty_state, icon, tr};

/// The retained state behind one text diff surface.
pub(crate) struct DiffPane {
    state: Entity<DiffState>,
    shown: Option<(FileDiff, u32)>,
    path: Option<SharedString>,
    anchor: Rc<RefCell<Option<DiffLinePosition>>>,
    pub show_all: Option<String>,
    cursor: usize,
}

/// The tooltip of one mark on the change map.
///
/// # Arguments
/// * `n` - which change, from 1
/// * `total` - how many changes the file has
///
/// # Returns
/// Such as "Change 2 of 5".
fn t_change(n: usize, total: usize) -> SharedString {
    rust_i18n::t!("diff.change_n_of", n = n, total = total).to_string().into()
}

/// The beginning of a very large diff, so it appears at once; whole hunks are kept, and a hunk
/// that alone is too long is cut with its line counts fixed up.
///
/// # Arguments
/// * `diff` - the full diff
/// * `limit` - roughly how many lines to keep
///
/// # Returns
/// A diff with at most about `limit` lines.
fn first_lines(diff: &FileDiff, limit: usize) -> FileDiff {
    let mut out = FileDiff { hunks: Vec::new(), ..diff.clone() };
    let mut used = 0;
    for hunk in &diff.hunks {
        if used >= limit {
            break;
        }
        let mut h = hunk.clone();
        if used + h.lines.len() > limit {
            h.lines.truncate(limit - used);
            h.old_count = h.lines.iter().filter(|l| matches!(l.kind, LineKind::Removed | LineKind::Context)).count() as u32;
            h.new_count = h.lines.iter().filter(|l| matches!(l.kind, LineKind::Added | LineKind::Context)).count() as u32;
        }
        used += h.lines.len();
        out.hunks.push(h);
    }
    out
}

impl DiffPane {
    /// Creates an empty pane that shows every supplied patch line.
    ///
    /// # Arguments
    /// * `cx` - app context that owns the new diff state
    ///
    /// # Returns
    /// The pane, with nothing loaded yet.
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            state: cx.new(|cx| DiffState::new(Vec::new(), cx).with_context_lines(None)),
            shown: None,
            path: None,
            anchor: Rc::default(),
            show_all: None,
            cursor: 0,
        }
    }

    fn sync(&mut self, diff: &FileDiff, settings: &Settings, cx: &mut App) -> Option<SharedString> {
        let tab_width = settings.diffs.tab_width.max(1);
        let unit = match settings.diffs.highlight {
            DiffHighlight::Words => Some(DiffInlineUnit::Word),
            DiffHighlight::Lines => None,
        };
        let mode = match settings.diffs.layout {
            DiffLayout::Unified => DiffMode::Unified,
            DiffLayout::Split => DiffMode::Split,
        };
        let changed = self.shown.as_ref().is_none_or(|(d, w)| d != diff || *w != tab_width);
        let state = self.state.clone();
        if changed {
            let files = DiffFile::parse_with(&patch_text(diff, tab_width as usize), DiffParseOptions::new().truncated_hunks(true)).unwrap_or_default();
            let path = files.first().map(|f| f.path().clone());
            let same_file = path.is_some() && path == self.path;
            let anchor = if same_file { self.anchor.borrow().clone() } else { None };
            state.update(cx, |s, cx| {
                s.set_files(files, cx);
                if let Some(anchor) = anchor {
                    s.scroll_to_line(anchor, cx);
                }
            });
            if !same_file {
                self.anchor.borrow_mut().take();
                self.cursor = 0;
            }
            self.path = path;
            self.shown = Some((diff.clone(), tab_width));
        }
        state.update(cx, |s, cx| {
            if s.inline_unit() != unit {
                s.set_inline_unit(unit, cx);
            }
            s.set_mode(mode, cx);
        });
        self.path.clone()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSource {
    Change,
    CommitFile,
    Page,
}

fn expand_tabs(text: &str, width: usize) -> Cow<'_, str> {
    if !text.contains('\t') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + width * 4);
    let mut column = 0;
    for ch in text.chars() {
        if ch == '\t' {
            let n = width - column % width;
            out.extend(std::iter::repeat_n(' ', n));
            column += n;
        } else {
            out.push(ch);
            column += 1;
        }
    }
    Cow::Owned(out)
}

fn patch_text(diff: &FileDiff, tab_width: usize) -> String {
    let old = diff.old_path.as_deref().unwrap_or(&diff.path);
    let mut out = String::new();
    if diff.is_new {
        out.push_str("--- /dev/null\n");
    } else {
        out.push_str(&format!("--- a/{old}\n"));
    }
    if diff.is_deleted {
        out.push_str("+++ /dev/null\n");
    } else {
        out.push_str(&format!("+++ b/{}\n", diff.path));
    }
    for hunk in &diff.hunks {
        out.push_str(&hunk.header());
        out.push('\n');
        for line in &hunk.lines {
            let sign = match line.kind {
                LineKind::Added => '+',
                LineKind::Removed => '-',
                LineKind::Context => ' ',
                LineKind::NoNewline => {
                    out.push_str(&line.text);
                    out.push('\n');
                    continue;
                }
            };
            out.push(sign);
            out.push_str(&expand_tabs(&line.text, tab_width));
            out.push('\n');
        }
    }
    out
}

fn line_position(line: &DiffLine) -> Option<(DiffSide, usize)> {
    match line.kind {
        LineKind::Removed => line.old_no.map(|n| (DiffSide::Original, n as usize)),
        LineKind::Added | LineKind::Context => line.new_no.map(|n| (DiffSide::Modified, n as usize)),
        LineKind::NoNewline => None,
    }
}

fn changed_line_at(diff: &FileDiff, side: DiffSide, line: usize) -> Option<(usize, usize)> {
    diff.hunks.iter().enumerate().find_map(|(h, hunk)| {
        hunk.lines
            .iter()
            .position(|l| matches!(l.kind, LineKind::Added | LineKind::Removed) && line_position(l) == Some((side, line)))
            .map(|l| (h, l))
    })
}

fn hunk_menu_button(h: usize, staged: bool, view: WeakEntity<RepoView>) -> impl IntoElement {
    Button::new(("hunk-menu", h))
        .xsmall()
        .ghost()
        .icon(icon(IconName::Ellipsis))
        .tooltip(tr("changes.hunk_actions"))
        .dropdown_menu(move |menu, _, _| {
            let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
            let menu = menu
                .item(
                    PopupMenuItem::new(if staged { tr("changes.unstage_hunk") } else { tr("changes.stage_hunk") })
                        .icon(icon(if staged { IconName::Minus } else { IconName::Plus }))
                        .on_click(move |_, window, cx| {
                            let _ = v1.update(cx, |this, cx| {
                                let Some(diff) = this.change_diff.clone() else { return };
                                let sel = patch::select_hunks(&diff, &[h]);
                                this.apply_selection(diff, sel, staged, false, window, cx);
                            });
                        }),
                )
                .item(PopupMenuItem::new(tr("changes.select_hunk")).icon(icon(IconName::MousePointerClick)).on_click(move |_, _, cx| {
                    let _ = v2.update(cx, |this, cx| {
                        let Some(diff) = this.change_diff.as_ref() else { return };
                        let sel = patch::select_hunks(diff, &[h]);
                        this.line_selection.extend(sel);
                        this.last_line = None;
                        cx.notify();
                    });
                }));
            if staged {
                menu
            } else {
                menu.separator().item(PopupMenuItem::new(tr("changes.discard_hunk")).icon(icon(IconName::Trash)).on_click(move |_, window, cx| {
                    let _ = v3.update(cx, |this, cx| this.confirm_discard_hunk(h, window, cx));
                }))
            }
        })
}

impl RepoView {
    pub(crate) fn toggle_line(&mut self, h: usize, l: usize, extend: bool, cx: &mut Context<Self>) {
        let Some(diff) = self.change_diff.as_ref() else { return };
        if extend && let Some((lh, ll)) = self.last_line.filter(|(lh, _)| *lh == h) {
            let (a, b) = if ll <= l { (ll, l) } else { (l, ll) };
            for (i, line) in diff.hunks[h].lines.iter().enumerate().take(b + 1).skip(a) {
                if matches!(line.kind, LineKind::Added | LineKind::Removed) {
                    self.line_selection.insert((lh, i));
                }
            }
            self.last_line = Some((h, l));
            cx.notify();
            return;
        }
        if !self.line_selection.remove(&(h, l)) {
            self.line_selection.insert((h, l));
        }
        self.last_line = Some((h, l));
        cx.notify();
    }

    /// Renders one file's diff: images side by side, text through the diff component.
    ///
    /// # Arguments
    /// * `source` - whether this is a working-tree change or a file of a commit
    /// * `diff` - the file diff to show
    /// * `cx` - the repository view's context
    ///
    /// # Returns
    /// The diff surface, or an empty state when there is nothing to show as text.
    /// Shows one file's diff with a toolbar (line counts, unified or split, word highlights,
    /// wrapping, jumping between changes) and a change map beside it.
    ///
    /// # Arguments
    /// * `source` - which surface shows the diff
    /// * `diff` - the file's diff
    ///
    /// # Returns
    /// The diff with its controls.
    pub(crate) fn render_file_diff(&mut self, source: DiffSource, diff: &FileDiff, cx: &mut Context<Self>) -> AnyElement {
        let body = self.render_diff_body(source, diff, cx);
        if diff.is_binary || diff.hunks.is_empty() {
            return body;
        }
        let theme = cx.theme().clone();
        let (added, removed) = diff.stats();
        let total_lines: usize = diff.hunks.iter().map(|h| h.lines.len()).sum::<usize>().max(1);
        let targets: Vec<(usize, Option<DiffLinePosition>, f32, f32, Hsla)> = {
            let path = self.pane(source).path.clone();
            let mut at = 0usize;
            diff.hunks
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let adds = h.lines.iter().any(|l| l.kind == LineKind::Added);
                    let dels = h.lines.iter().any(|l| l.kind == LineKind::Removed);
                    let color = match (adds, dels) {
                        (true, false) => theme.green,
                        (false, true) => theme.red,
                        _ => theme.yellow,
                    };
                    let pos = path.clone().and_then(|p| h.lines.iter().filter(|l| l.kind != LineKind::Context).find_map(line_position).map(|(side, n)| DiffLinePosition::new(p, side, n)));
                    let top = at as f32 / total_lines as f32;
                    let height = h.lines.len() as f32 / total_lines as f32;
                    at += h.lines.len();
                    (i, pos, top, height, color)
                })
                .collect()
        };
        let hunks = targets.len();
        let cursor = self.pane(source).cursor.min(hunks.saturating_sub(1));
        let settings = self.settings.diffs.clone();
        let split = settings.layout == DiffLayout::Split;
        let words = settings.highlight == DiffHighlight::Words;
        let positions: Vec<Option<DiffLinePosition>> = targets.iter().map(|t| t.1.clone()).collect();
        let seg = |id: &'static str, label: SharedString, on: bool, cx: &App| {
            let theme = cx.theme();
            div()
                .id(id)
                .px_2()
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .when(on, |d| d.bg(theme.background).text_color(theme.foreground).shadow_xs())
                .when(!on, |d| d.text_color(theme.muted_foreground).hover(|s| s.text_color(theme.foreground)))
                .child(label)
        };
        let set_prefs = move |f: fn(&mut cairn_core::settings::Settings)| {
            move |this: &mut RepoView, _: &ClickEvent, _: &mut Window, cx: &mut Context<RepoView>| {
                f(&mut this.settings);
                cx.emit(crate::repo_view::DiffPrefsEvent(this.settings.diffs.clone()));
                cx.notify();
            }
        };
        let toolbar = h_flex()
            .flex_none()
            .gap_2()
            .px_3()
            .h(px(36.))
            .border_b_1()
            .border_color(theme.border.opacity(0.7))
            .bg(theme.sidebar)
            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.green).child(format!("+{added}")))
            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(theme.red).child(format!("−{removed}")))
            .child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("diff.changes_n", hunks)))
            .child(div().flex_1())
            .child(
                h_flex()
                    .gap_0p5()
                    .p(px(2.))
                    .rounded(px(8.))
                    .bg(theme.secondary)
                    .child(seg("diff-unified", tr("diff.unified"), !split, cx).on_click(cx.listener(set_prefs(|s| s.diffs.layout = DiffLayout::Unified))))
                    .child(seg("diff-split", tr("diff.split"), split, cx).on_click(cx.listener(set_prefs(|s| s.diffs.layout = DiffLayout::Split)))),
            )
            .child(crate::widgets::toggle_pill("diff-words", tr("diff.words"), words, cx).on_click(cx.listener(set_prefs(|s| s.diffs.highlight = if s.diffs.highlight == DiffHighlight::Words { DiffHighlight::Lines } else { DiffHighlight::Words }))))
            .child(crate::widgets::toggle_pill("diff-wrap", tr("diff.wrap"), settings.wrap_lines, cx).on_click(cx.listener(set_prefs(|s| s.diffs.wrap_lines = !s.diffs.wrap_lines))))
            .child(div().w(px(1.)).h(px(18.)).bg(theme.border))
            .child(Button::new("diff-prev").xsmall().ghost().icon(icon(IconName::ChevronUp)).tooltip(tr("diff.prev_change")).on_click({
                let positions = positions.clone();
                cx.listener(move |this, _, _, cx| this.jump_to_change(source, -1, &positions, cx))
            }))
            .child(div().text_xs().text_color(theme.muted_foreground).child(format!("{}/{}", cursor + 1, hunks)))
            .child(Button::new("diff-next").xsmall().ghost().icon(icon(IconName::ChevronDown)).tooltip(tr("diff.next_change")).on_click({
                let positions = positions.clone();
                cx.listener(move |this, _, _, cx| this.jump_to_change(source, 1, &positions, cx))
            }));
        let mut map = div().relative().flex_none().w(px(12.)).h_full().border_l_1().border_color(theme.border.opacity(0.6)).bg(theme.sidebar.opacity(0.6));
        for (i, pos, top, height, color) in targets {
            let tip: SharedString = t_change(i + 1, hunks);
            map = map.child(
                div()
                    .id(("diff-map", i))
                    .absolute()
                    .left(px(2.))
                    .right(px(2.))
                    .top(relative(top))
                    .h(relative(height.max(0.004)))
                    .min_h(px(3.))
                    .rounded(px(2.))
                    .cursor_pointer()
                    .bg(color.opacity(if i == cursor { 1. } else { 0.65 }))
                    .hover(move |s| s.bg(color))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let pane = this.pane_mut(source);
                        pane.cursor = i;
                        if let Some(pos) = pos.clone() {
                            pane.state.update(cx, |s, cx| s.scroll_to_line(pos, cx));
                        }
                        cx.notify();
                    })),
            );
        }
        v_flex().size_full().child(toolbar).child(h_flex().flex_1().min_h_0().child(div().flex_1().min_w_0().h_full().child(body)).child(map)).into_any_element()
    }

    /// Scrolls a diff to the previous or next change.
    ///
    /// # Arguments
    /// * `source` - which diff surface
    /// * `delta` - `-1` for the previous change, `1` for the next
    /// * `positions` - where each change starts
    fn jump_to_change(&mut self, source: DiffSource, delta: isize, positions: &[Option<DiffLinePosition>], cx: &mut Context<Self>) {
        let pane = self.pane_mut(source);
        let next = (pane.cursor as isize + delta).clamp(0, positions.len() as isize - 1) as usize;
        pane.cursor = next;
        if let Some(pos) = positions.get(next).cloned().flatten() {
            pane.state.update(cx, |s, cx| s.scroll_to_line(pos, cx));
        }
        cx.notify();
    }

    fn pane(&self, source: DiffSource) -> &DiffPane {
        match source {
            DiffSource::Change => &self.change_pane,
            DiffSource::CommitFile => &self.commit_pane,
            DiffSource::Page => &self.page_pane,
        }
    }

    fn pane_mut(&mut self, source: DiffSource) -> &mut DiffPane {
        match source {
            DiffSource::Change => &mut self.change_pane,
            DiffSource::CommitFile => &mut self.commit_pane,
            DiffSource::Page => &mut self.page_pane,
        }
    }

    fn render_diff_body(&mut self, source: DiffSource, diff: &FileDiff, cx: &mut Context<Self>) -> AnyElement {
        if diff.is_binary {
            if let Some((old, new)) = image_pair(source, diff, cx) {
                return render_image_diff(old, new, cx).into_any_element();
            }
            return empty_state(IconName::FileImage, tr("diff.binary_title"), tr("diff.binary_body"), cx).into_any_element();
        }
        if diff.hunks.is_empty() {
            return empty_state(IconName::FileCheck, tr("diff.no_changes_title"), tr("diff.no_changes_body"), cx).into_any_element();
        }
        let total: usize = diff.hunks.iter().map(|h| h.lines.len()).sum();
        let limit_lines = (self.settings.diffs.collapse_larger_than_kb as usize).saturating_mul(1024) / 40;
        let first_part = if limit_lines > 0 { limit_lines.clamp(1500, 6000) } else { 20000 };
        let settings = self.settings.clone();
        let pane = match source {
            DiffSource::Change => &mut self.change_pane,
            DiffSource::CommitFile => &mut self.commit_pane,
            DiffSource::Page => &mut self.page_pane,
        };
        let cut = total > first_part && pane.show_all.as_deref() != Some(diff.path.as_str());
        let shown = if cut { first_lines(diff, first_part) } else { diff.clone() };
        let diff = &shown;
        if cut {
            let view = cx.entity().downgrade();
            let which = source;
            let file = diff.path.clone();
            let theme = cx.theme().clone();
            let banner = h_flex()
                .flex_none()
                .gap_2()
                .px_3()
                .py_1p5()
                .bg(theme.yellow.opacity(0.1))
                .border_b_1()
                .border_color(theme.yellow.opacity(0.3))
                .text_xs()
                .child(icon(IconName::Zap).xsmall().text_color(theme.yellow))
                .child(div().flex_1().child(gpui_kit::SharedString::from(rust_i18n::t!("diff.first_part", shown = first_part, total = total).to_string())))
                .child(Button::new("diff-show-all").xsmall().ghost().label(tr("diff.show_all")).on_click(move |_, _, cx| {
                    let file = file.clone();
                    let _ = view.update(cx, |v, cx| {
                        let pane = match which {
                            DiffSource::Change => &mut v.change_pane,
                            DiffSource::CommitFile => &mut v.commit_pane,
                            DiffSource::Page => &mut v.page_pane,
                        };
                        pane.show_all = Some(file);
                        cx.notify();
                    });
                }));
            let body = self.render_shown_diff(source, diff, settings, cx);
            return v_flex().size_full().child(banner).child(div().flex_1().min_h_0().child(body)).into_any_element();
        }
        self.render_shown_diff(source, diff, settings, cx)
    }

    fn render_shown_diff(&mut self, source: DiffSource, diff: &FileDiff, settings: Settings, cx: &mut Context<Self>) -> AnyElement {
        let pane = match source {
            DiffSource::Change => &mut self.change_pane,
            DiffSource::CommitFile => &mut self.commit_pane,
            DiffSource::Page => &mut self.page_pane,
        };
        let Some(path) = pane.sync(diff, &settings, cx) else {
            return empty_state(IconName::TriangleAlert, tr("diff.unreadable_title"), tr("diff.unreadable_body"), cx).into_any_element();
        };
        let state = pane.state.clone();
        let anchor = pane.anchor.clone();
        let split = settings.diffs.layout == DiffLayout::Split;
        let mut el = Diff::new(&state)
            .size_full()
            .header_visible(false)
            .line_number(settings.diffs.line_numbers)
            .syntax_highlight(settings.diffs.syntax_highlighting)
            .soft_wrap(settings.diffs.wrap_lines || split || source != DiffSource::Change)
            .on_line_hover({
                let anchor = anchor.clone();
                move |pos, _, _| {
                    if let Some(pos) = pos {
                        anchor.replace(Some(pos.clone()));
                    }
                }
            });
        if source == DiffSource::Page && self.page == crate::repo_view::Page::PullRequests {
            let view = cx.entity().downgrade();
            let file = diff.path.clone();
            return el
                .hover_highlight(DiffHoverHighlight::Line)
                .on_line_click(move |pos, _, window, cx| {
                    let new_side = pos.side() == DiffSide::Modified;
                    let line = pos.line() as u32;
                    let file = file.clone();
                    if let Some(view) = view.upgrade() {
                        crate::pages::pulls::comment_on_line(view, file, line, new_side, window, cx);
                    }
                })
                .into_any_element();
        }
        if source != DiffSource::Change {
            return el.into_any_element();
        }
        let theme = cx.theme().clone();
        let staged = self.change.as_ref().is_some_and(|c| c.staged);
        let selected_bg = theme.primary.opacity(0.22);
        let decorations: Vec<DiffDecoration> = self
            .line_selection
            .iter()
            .filter_map(|&(h, l)| diff.hunks.get(h)?.lines.get(l))
            .filter_map(line_position)
            .map(|(side, n)| DiffDecoration::new(DiffLineRange::new(path.clone(), side, n, n)).with_background(selected_bg))
            .collect();
        let mut hunk_at: HashMap<DiffLinePosition, usize> = HashMap::new();
        let markers: Vec<DiffGutterMarker> = diff
            .hunks
            .iter()
            .enumerate()
            .filter_map(|(h, hunk)| {
                let (side, n) = hunk.lines.iter().find_map(line_position)?;
                let pos = DiffLinePosition::new(path.clone(), side, n);
                hunk_at.insert(pos.clone(), h);
                Some(DiffGutterMarker::new(("hunk", h), pos).with_label(tr("changes.hunk_actions")))
            })
            .collect();
        let view = cx.entity().downgrade();
        el = el
            .hover_highlight(DiffHoverHighlight::Line)
            .decorations(decorations)
            .gutter_markers(markers)
            .render_gutter_marker({
                let view = view.clone();
                move |marker, _, _| match hunk_at.get(marker.position()) {
                    Some(&h) => hunk_menu_button(h, staged, view.clone()).into_any_element(),
                    None => div().into_any_element(),
                }
            })
            .on_line_click(move |pos, ev, _, cx| {
                anchor.replace(Some(pos.clone()));
                let shift = ev.modifiers().shift;
                let _ = view.update(cx, |this, cx| {
                    let hit = this.change_diff.as_ref().and_then(|d| changed_line_at(d, pos.side(), pos.line()));
                    if let Some((h, l)) = hit {
                        this.toggle_line(h, l, shift, cx);
                    }
                });
            });
        el.into_any_element()
    }
}

const IMAGE_EXT: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

fn blob_to_file(git: &cairn_core::Git, spec: &str, ext: &str) -> Option<std::path::PathBuf> {
    let bytes = git.run_bytes(["show", spec]).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let dir = cairn_core::paths::data_dir().join("tmp").join("img");
    std::fs::create_dir_all(&dir).ok()?;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in spec.bytes().chain(bytes.iter().copied().take(4096)) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let path = dir.join(format!("{h:016x}.{ext}"));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

fn image_pair(source: DiffSource, diff: &FileDiff, cx: &mut Context<RepoView>) -> Option<(Option<std::path::PathBuf>, Option<std::path::PathBuf>)> {
    let ext = diff.path.rsplit('.').next()?.to_lowercase();
    if !IMAGE_EXT.contains(&ext.as_str()) {
        return None;
    }
    let view = cx.entity().read(cx);
    let git = view.git();
    let old_path = diff.old_path.clone().unwrap_or_else(|| diff.path.clone());
    let pair = match source {
        DiffSource::Change => {
            let staged = view.change.as_ref().is_some_and(|c| c.staged);
            if staged {
                (blob_to_file(&git, &format!("HEAD:{old_path}"), &ext), blob_to_file(&git, &format!(":{}", diff.path), &ext))
            } else {
                let worktree = view.repo.root.join(&diff.path);
                (blob_to_file(&git, &format!(":{old_path}"), &ext), worktree.exists().then_some(worktree))
            }
        }
        DiffSource::CommitFile => {
            let id = view.selected_commit.clone()?;
            (blob_to_file(&git, &format!("{id}^:{old_path}"), &ext), blob_to_file(&git, &format!("{id}:{}", diff.path), &ext))
        }
        DiffSource::Page => {
            let (old, new) = view.page_revs.clone()?;
            (old.and_then(|o| blob_to_file(&git, &format!("{o}:{old_path}"), &ext)), blob_to_file(&git, &format!("{new}:{}", diff.path), &ext))
        }
    };
    Some(pair)
}

fn render_image_diff(old: Option<std::path::PathBuf>, new: Option<std::path::PathBuf>, cx: &mut Context<RepoView>) -> impl IntoElement + use<> {
    let theme = cx.theme().clone();
    let (mode, amount) = {
        let v = cx.entity().read(cx);
        (v.img_mode, v.img_amount)
    };
    let checker = theme.muted;
    let frame = |label: SharedString, path: Option<std::path::PathBuf>, color: Hsla| {
        v_flex()
            .flex_1()
            .min_w(px(200.))
            .gap_1()
            .child(div().text_xs().text_color(color).child(label))
            .child(
                div()
                    .h(px(320.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color.opacity(0.6))
                    .bg(checker)
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .child(match path {
                        Some(p) => img(p).max_w_full().max_h_full().into_any_element(),
                        None => div().text_xs().text_color(theme.muted_foreground).child(tr("diff.image_none")).into_any_element(),
                    }),
            )
    };
    let modes = [("diff.image_side", 0u8), ("diff.image_swipe", 1), ("diff.image_onion", 2)];
    let mut mode_row = h_flex().rounded(px(6.)).border_1().border_color(theme.border).overflow_hidden();
    for (label, m) in modes {
        let b = Button::new(label).xsmall().label(tr(label)).on_click(cx.listener(move |this, _, _, cx| {
            this.img_mode = m;
            cx.notify();
        }));
        mode_row = mode_row.child(if mode == m { b.primary() } else { b.ghost() });
    }
    let mut steps = h_flex().gap_1();
    if mode > 0 {
        for (i, a) in [0.0f32, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
            let b = Button::new(("img-step", i)).xsmall().label(format!("{}%", (a * 100.) as u32)).on_click(cx.listener(move |this, _, _, cx| {
                this.img_amount = a;
                cx.notify();
            }));
            steps = steps.child(if (amount - a).abs() < 0.01 { b.primary() } else { b.ghost() });
        }
    }
    let body = if mode == 0 {
        h_flex().gap_3().flex_wrap().child(frame(tr("diff.image_before"), old, theme.red)).child(frame(tr("diff.image_after"), new, theme.green)).into_any_element()
    } else {
        let layer = |p: Option<std::path::PathBuf>| match p {
            Some(p) => img(p).max_w_full().max_h_full().into_any_element(),
            None => div().into_any_element(),
        };
        let top = if mode == 1 {
            div().absolute().top_0().left_0().h_full().w(relative(amount)).overflow_hidden().flex().items_center().justify_center().child(layer(new.clone())).into_any_element()
        } else {
            div().absolute().inset_0().opacity(amount).flex().items_center().justify_center().child(layer(new.clone())).into_any_element()
        };
        div()
            .relative()
            .h(px(380.))
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(checker)
            .overflow_hidden()
            .child(div().absolute().inset_0().flex().items_center().justify_center().child(layer(old)))
            .child(top)
            .when(mode == 1, |d| d.child(div().absolute().top_0().bottom_0().left(relative(amount)).w(px(2.)).bg(theme.primary)))
            .into_any_element()
    };
    v_flex()
        .id("image-diff")
        .size_full()
        .overflow_y_scroll()
        .p_4()
        .gap_3()
        .child(h_flex().gap_3().items_center().child(mode_row).child(steps))
        .child(body)
}

#[cfg(test)]
mod tests {
    use cairn_core::diff::parse_diff;
    use gpui_kit::component::diff::{DiffFile, DiffSide};

    use super::{changed_line_at, expand_tabs, patch_text};

    const SAMPLE: &str = "diff --git a/src/x.lua b/src/x.lua\nindex 1..2 100644\n--- a/src/x.lua\n+++ b/src/x.lua\n@@ -1,3 +1,4 @@ local function f()\n a\n-b\n+B\n+c\n d\n\\ No newline at end of file\n";

    #[test]
    fn rebuilt_patch_parses_in_the_component() {
        let diff = parse_diff(SAMPLE).remove(0);
        let files = DiffFile::parse(&patch_text(&diff, 4)).expect("patch parses");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path().as_ref(), "src/x.lua");
        assert_eq!(files[0].additions(), 2);
        assert_eq!(files[0].deletions(), 1);
    }

    #[test]
    fn new_and_deleted_files_use_dev_null() {
        let mut diff = parse_diff(SAMPLE).remove(0);
        diff.is_new = true;
        assert!(patch_text(&diff, 4).starts_with("--- /dev/null\n+++ b/src/x.lua\n"));
        diff.is_new = false;
        diff.is_deleted = true;
        assert!(patch_text(&diff, 4).starts_with("--- a/src/x.lua\n+++ /dev/null\n"));
    }

    #[test]
    fn clicked_positions_map_back_to_changed_lines() {
        let diff = parse_diff(SAMPLE).remove(0);
        assert_eq!(changed_line_at(&diff, DiffSide::Original, 2), Some((0, 1)));
        assert_eq!(changed_line_at(&diff, DiffSide::Modified, 2), Some((0, 2)));
        assert_eq!(changed_line_at(&diff, DiffSide::Modified, 3), Some((0, 3)));
        assert_eq!(changed_line_at(&diff, DiffSide::Modified, 1), None);
    }

    #[test]
    fn tabs_expand_to_the_configured_stops() {
        assert_eq!(expand_tabs("\tx", 2), "  x");
        assert_eq!(expand_tabs("ab\tc", 4), "ab  c");
        assert_eq!(expand_tabs("plain", 4), "plain");
    }
}
