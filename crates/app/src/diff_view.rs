use cairn_core::diff::{FileDiff, Hunk, LineKind, word_diff};
use cairn_core::patch::LineSelection;
use cairn_core::settings::{DiffHighlight, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::RepoView;
use crate::widgets::{empty_state, tr};

const LINE_H: f32 = 22.;

#[derive(Clone, Copy)]
enum Row {
    Header(usize),
    Line(usize, usize),
}

fn flatten(diff: &FileDiff) -> Vec<Row> {
    let mut rows = Vec::new();
    for (h, hunk) in diff.hunks.iter().enumerate() {
        rows.push(Row::Header(h));
        for l in 0..hunk.lines.len() {
            rows.push(Row::Line(h, l));
        }
    }
    rows
}

fn paired_line(hunk: &Hunk, l: usize) -> Option<usize> {
    let kind = hunk.lines[l].kind;
    if !matches!(kind, LineKind::Added | LineKind::Removed) {
        return None;
    }
    let mut start = l;
    while start > 0 && matches!(hunk.lines[start - 1].kind, LineKind::Added | LineKind::Removed) {
        start -= 1;
    }
    let mut end = l;
    while end + 1 < hunk.lines.len() && matches!(hunk.lines[end + 1].kind, LineKind::Added | LineKind::Removed) {
        end += 1;
    }
    let removed: Vec<usize> = (start..=end).filter(|&i| hunk.lines[i].kind == LineKind::Removed).collect();
    let added: Vec<usize> = (start..=end).filter(|&i| hunk.lines[i].kind == LineKind::Added).collect();
    if removed.len() != added.len() {
        return None;
    }
    match kind {
        LineKind::Removed => removed.iter().position(|&i| i == l).map(|p| added[p]),
        _ => added.iter().position(|&i| i == l).map(|p| removed[p]),
    }
}

fn byte_ranges(spans: &[cairn_core::diff::WordSpan]) -> Vec<std::ops::Range<usize>> {
    let mut pos = 0;
    let mut out = Vec::new();
    for s in spans {
        let end = pos + s.text.len();
        if s.changed && !s.text.trim().is_empty() {
            out.push(pos..end);
        }
        pos = end;
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSource {
    Change,
    CommitFile,
}

impl RepoView {
    fn diff_for(&self, source: DiffSource) -> Option<&FileDiff> {
        match source {
            DiffSource::Change => self.change_diff.as_ref(),
            DiffSource::CommitFile => self.detail_file.as_ref(),
        }
    }

    fn render_diff_row(&mut self, source: DiffSource, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let Some(diff) = self.diff_for(source) else { return div().into_any_element() };
        let theme = cx.theme().clone();
        let mono = theme.mono_font_family.clone();
        let selectable = source == DiffSource::Change && !diff.is_binary;
        match row {
            Row::Header(h) => {
                let hunk = &diff.hunks[h];
                let header = hunk.header();
                let staged = self.change.as_ref().is_some_and(|c| c.staged);
                h_flex()
                    .id(("hunk", h))
                    .h(px(LINE_H))
                    .px_3()
                    .gap_2()
                    .items_center()
                    .bg(theme.muted)
                    .border_y_1()
                    .border_color(theme.border)
                    .text_xs()
                    .font_family(mono)
                    .text_color(theme.primary)
                    .child(div().flex_1().truncate().child(header))
                    .when(selectable, |d| {
                        d.child(crate::changes::hunk_buttons(h, staged, cx))
                    })
                    .into_any_element()
            }
            Row::Line(h, l) => {
                let hunk = &diff.hunks[h];
                let line = &hunk.lines[l];
                let selected = self.line_selection.contains(&(h, l));
                let (sign, bg, sign_color) = match line.kind {
                    LineKind::Added => ("+", theme.green.opacity(0.12), theme.green),
                    LineKind::Removed => ("−", theme.red.opacity(0.12), theme.red),
                    LineKind::Context => (" ", theme.transparent, theme.muted_foreground),
                    LineKind::NoNewline => ("", theme.transparent, theme.muted_foreground),
                };
                let changed_line = matches!(line.kind, LineKind::Added | LineKind::Removed);
                let bg = if selected { theme.primary.opacity(0.22) } else { bg };
                let text: SharedString = line.text.replace('\t', &" ".repeat(self.settings.diffs.tab_width as usize)).into();
                let mut highlights = Vec::new();
                if self.settings.diffs.highlight == DiffHighlight::Words
                    && line.text.len() < 2000
                    && let Some(p) = paired_line(hunk, l)
                {
                    let other = &hunk.lines[p];
                    let (old, new) = if line.kind == LineKind::Removed {
                        (line.text.as_str(), other.text.as_str())
                    } else {
                        (other.text.as_str(), line.text.as_str())
                    };
                    let (a, b) = word_diff(old, new);
                    let spans = if line.kind == LineKind::Removed { a } else { b };
                    let strong = if line.kind == LineKind::Removed { theme.red.opacity(0.35) } else { theme.green.opacity(0.35) };
                    if !line.text.contains('\t') {
                        for r in byte_ranges(&spans) {
                            highlights.push((r, HighlightStyle { background_color: Some(strong), ..Default::default() }));
                        }
                    }
                }
                let styled = StyledText::new(text).with_highlights(highlights);
                let num = |n: Option<u32>| n.map(|v| v.to_string()).unwrap_or_default();
                let show_numbers = self.settings.diffs.line_numbers;
                let el = h_flex()
                    .id(("line", h * 100_000 + l))
                    .h(px(LINE_H))
                    .w_full()
                    .bg(bg)
                    .text_xs()
                    .font_family(mono)
                    .when(show_numbers, |d| {
                        d.child(div().flex_none().w(px(44.)).pr_2().text_right().text_color(theme.muted_foreground.opacity(0.7)).child(num(line.old_no)))
                            .child(div().flex_none().w(px(44.)).pr_2().text_right().text_color(theme.muted_foreground.opacity(0.7)).child(num(line.new_no)))
                    })
                    .child(div().flex_none().w(px(16.)).text_color(sign_color).child(sign))
                    .child(div().flex_1().whitespace_nowrap().child(styled));
                if selectable && changed_line {
                    el.cursor_pointer()
                        .hover(|s| s.bg(theme.primary.opacity(0.10)))
                        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                            this.toggle_line(h, l, ev.modifiers().shift, cx);
                        }))
                        .into_any_element()
                } else {
                    el.into_any_element()
                }
            }
        }
    }

    pub(crate) fn toggle_line(&mut self, h: usize, l: usize, extend: bool, cx: &mut Context<Self>) {
        let Some(diff) = self.change_diff.as_ref() else { return };
        if extend && let Some((lh, ll)) = self.last_line.filter(|(lh, _)| *lh == h) {
            let (a, b) = if ll <= l { (ll, l) } else { (l, ll) };
            for i in a..=b {
                if matches!(diff.hunks[h].lines[i].kind, LineKind::Added | LineKind::Removed) {
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
        let b = gpui_kit::component::button::Button::new(label).xsmall().label(tr(label)).on_click(cx.listener(move |this, _, _, cx| {
            this.img_mode = m;
            cx.notify();
        }));
        use gpui_kit::component::button::ButtonVariants as _;
        mode_row = mode_row.child(if mode == m { b.primary() } else { b.ghost() });
    }
    let mut steps = h_flex().gap_1();
    if mode > 0 {
        for (i, a) in [0.0f32, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
            use gpui_kit::component::button::ButtonVariants as _;
            let b = gpui_kit::component::button::Button::new(("img-step", i)).xsmall().label(format!("{}%", (a * 100.) as u32)).on_click(cx.listener(move |this, _, _, cx| {
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

pub(crate) fn render_file_diff(
    id: &'static str,
    diff: &FileDiff,
    _selection: Option<&LineSelection>,
    settings: &Settings,
    _window: &mut Window,
    cx: &mut Context<RepoView>,
) -> AnyElement {
    let _ = settings;
    let source = if id == "change-diff" { DiffSource::Change } else { DiffSource::CommitFile };
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
    let limit_lines = (settings.diffs.collapse_larger_than_kb as usize).saturating_mul(1024) / 40;
    if limit_lines > 0 && total > limit_lines.max(2000) {
        return empty_state(IconName::TriangleAlert, tr("diff.too_large_title"), tr("diff.too_large_body"), cx).into_any_element();
    }
    let rows = flatten(diff);
    let count = rows.len();
    uniform_list(
        id,
        count,
        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
            range.map(|ix| this.render_diff_row(source, rows[ix], cx)).collect::<Vec<_>>()
        }),
    )
    .size_full()
    .into_any_element()
}
