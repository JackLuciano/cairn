use std::collections::{HashMap, HashSet};

use cairn_core::tree::{self, FileContent, TreeEntry};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, empty_state, framed, hstack, icon, page_heading, spacer, tr};

const MAX_LINES: usize = 4000;

pub(crate) struct BrowseState {
    pub rev: Option<String>,
    pub label: String,
    pub dirs: HashMap<String, Vec<TreeEntry>>,
    pub open: HashSet<String>,
    pub loading: HashSet<String>,
    pub error: Option<String>,
    pub file: Option<String>,
    pub content: Option<FileContent>,
    pub content_loading: bool,
    pub input: Entity<InputState>,
    _sub: Subscription,
}

impl BrowseState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("browse.rev_placeholder")));
        let sub = cx.subscribe(&input, |this: &mut RepoView, s, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                let rev = s.read(cx).value().trim().to_string();
                if !rev.is_empty() {
                    this.open_browse(rev, cx);
                }
            }
        });
        Self { rev: None, label: String::new(), dirs: HashMap::new(), open: HashSet::new(), loading: HashSet::new(), error: None, file: None, content: None, content_loading: false, input, _sub: sub }
    }
}

impl RepoView {
    /// Shows the files of a commit, branch or tag on the Browse page.
    ///
    /// # Arguments
    /// * `rev` - what to browse, such as a commit hash or `main`
    pub(crate) fn open_browse(&mut self, rev: String, cx: &mut Context<Self>) {
        self.page = Page::Browse;
        let git = self.git();
        let typed = rev.clone();
        self.spawn_bg(
            move || tree::resolve_commit(&git, &typed).map(|id| (id.clone(), tree::list_dir(&git, &id, ""))),
            move |this, found, cx| match found {
                Some((id, root)) => {
                    let label = if rev.len() >= 7 && id.starts_with(&rev) { id.chars().take(10).collect() } else { rev.clone() };
                    let same = this.browse.rev.as_deref() == Some(id.as_str());
                    this.browse.rev = Some(id);
                    this.browse.label = label;
                    this.browse.error = None;
                    if !same {
                        this.browse.dirs.clear();
                        this.browse.loading.clear();
                        let keep = this.browse.file.take();
                        this.browse.content = None;
                        if let Some(f) = keep {
                            this.open_browse_file(f, cx);
                        }
                    }
                    match root {
                        Ok(entries) => {
                            this.browse.dirs.insert(String::new(), entries);
                        }
                        Err(e) => this.browse.error = Some(crate::repo_view::friendly_error(&e)),
                    }
                }
                None => this.browse.error = Some(t!("browse.not_found", rev = rev.clone()).to_string()),
            },
            cx,
        );
        cx.notify();
    }

    fn toggle_browse_dir(&mut self, dir: String, cx: &mut Context<Self>) {
        if !self.browse.open.remove(&dir) {
            self.browse.open.insert(dir.clone());
            if !self.browse.dirs.contains_key(&dir) && self.browse.loading.insert(dir.clone()) {
                let Some(rev) = self.browse.rev.clone() else { return };
                let git = self.git();
                let d = dir.clone();
                self.spawn_bg(
                    move || tree::list_dir(&git, &rev, &d),
                    move |this, entries, _| {
                        this.browse.loading.remove(&dir);
                        this.browse.dirs.insert(dir.clone(), entries.unwrap_or_default());
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    fn open_browse_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(rev) = self.browse.rev.clone() else { return };
        self.browse.file = Some(path.clone());
        self.browse.content = None;
        self.browse.content_loading = true;
        let git = self.git();
        let p = path.clone();
        self.spawn_bg(
            move || tree::read_file(&git, &rev, &p),
            move |this, content, _| {
                if this.browse.file.as_deref() != Some(path.as_str()) {
                    return;
                }
                this.browse.content_loading = false;
                this.browse.content = Some(content.unwrap_or(FileContent::Binary(0)));
            },
            cx,
        );
        cx.notify();
    }

    fn save_browse_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(rev), Some(path)) = (self.browse.rev.clone(), self.browse.file.clone()) else { return };
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| self.repo.root.clone());
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        let git = self.git();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(dest))) = picked.await else { return };
            let d2 = dest.clone();
            let result = cx.background_executor().spawn(async move { tree::save_file(&git, &rev, &path, &d2) }).await;
            let _ = cx.update(|window, cx| match result {
                Ok(()) => window.push_notification(Notification::success(t!("browse.saved", path = dest.display().to_string()).to_string()), cx),
                Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)).title(tr("browse.save_failed")), cx),
            });
        })
        .detach();
    }

    fn browse_rows(&self, dir: &str, depth: usize, out: &mut Vec<(usize, TreeEntry)>) {
        let Some(entries) = self.browse.dirs.get(dir) else { return };
        for e in entries {
            out.push((depth, e.clone()));
            if e.is_dir && self.browse.open.contains(&e.path) {
                self.browse_rows(&e.path, depth + 1, out);
            }
        }
    }

    fn render_browse_file(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(path) = self.browse.file.clone() else {
            return empty_state(IconName::FileText, tr("browse.pick_title"), tr("browse.pick_body"), cx).into_any_element();
        };
        let header = hstack()
            .flex_none()
            .gap_2()
            .px_4()
            .py_2p5()
            .border_b_1()
            .border_color(theme.border)
            .child(icon(IconName::FileText).small().text_color(theme.muted_foreground))
            .child(div().flex_1().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(path.clone()))
            .child({
                let p = path.clone();
                framed(Button::new("browse-history"), Tone::Plain, cx).small().icon(icon(IconName::FileClock)).label(tr("page.file_history")).on_click(cx.listener(move |this, _, window, cx| {
                    this.open_page(Page::FileHistory, window, cx);
                    this.open_file_history(p.clone(), cx);
                }))
            })
            .child(framed(Button::new("browse-save"), Tone::Plain, cx).small().icon(icon(IconName::Save)).label(tr("browse.save")).on_click(cx.listener(|this, _, window, cx| this.save_browse_file(window, cx))));
        let body = match &self.browse.content {
            _ if self.browse.content_loading => crate::skeleton::list(6, cx).into_any_element(),
            Some(FileContent::Text(text)) => {
                let total = text.lines().count();
                let width = total.to_string().len();
                let mut lines = v_flex().py_2().font_family(theme.mono_font_family.clone()).text_size(px(12.));
                for (i, l) in text.lines().take(MAX_LINES).enumerate() {
                    lines = lines.child(h_flex().px_3().child(div().flex_none().w(px(width as f32 * 8. + 12.)).text_color(theme.muted_foreground.opacity(0.7)).child(format!("{}", i + 1))).child(div().flex_1().min_w_0().whitespace_normal().child(l.replace('\t', "    "))));
                }
                if total > MAX_LINES {
                    lines = lines.child(div().px_3().py_2().text_xs().text_color(theme.muted_foreground).child(t!("browse.truncated", n = MAX_LINES, total = total).to_string()));
                }
                div().id("browse-text").flex_1().min_h_0().overflow_scroll().child(lines).into_any_element()
            }
            Some(FileContent::Binary(size)) => empty_state(IconName::File, tr("browse.binary_title"), t!("browse.binary_body", size = format_size(*size)).to_string(), cx).into_any_element(),
            None => div().into_any_element(),
        };
        v_flex().size_full().child(header).child(body).into_any_element()
    }

    pub(crate) fn render_browse(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.browse.rev.is_none() && self.browse.error.is_none() && self.loaded_once {
            let start = self.selected_commit.clone().unwrap_or_else(|| "HEAD".into());
            self.open_browse(start, cx);
        }
        let theme = cx.theme().clone();
        let subtitle = match &self.browse.rev {
            Some(_) => t!("browse.subtitle", rev = self.browse.label.clone()).to_string(),
            None => tr("browse.subtitle_none").to_string(),
        };
        let current = self.current_branch().map(str::to_string);
        let header = hstack()
            .flex_none()
            .gap_2()
            .px_5()
            .pt_5()
            .pb_3()
            .child(page_heading(tr("page.browse"), subtitle, cx))
            .child(spacer())
            .child(div().w(px(260.)).child(Input::new(&self.browse.input).small().prefix(icon(IconName::Search).small())))
            .when_some(current, |d, b| {
                d.child(framed(Button::new("browse-current"), Tone::Plain, cx).icon(icon(IconName::GitBranch)).label(b.clone()).tooltip(tr("browse.current_tip")).on_click(cx.listener(move |this, _, _, cx| this.open_browse(b.clone(), cx))))
            });
        if let Some(e) = self.browse.error.clone() {
            return v_flex().size_full().child(header).child(empty_state(IconName::FolderTree, tr("browse.error_title"), e, cx)).into_any_element();
        }
        let mut rows = Vec::new();
        self.browse_rows("", 0, &mut rows);
        let mut list = v_flex().gap_px().p_2();
        if rows.is_empty() {
            list = list.child(crate::skeleton::sidebar(8, cx));
        }
        for (i, (depth, e)) in rows.into_iter().enumerate() {
            let selected = self.browse.file.as_deref() == Some(e.path.as_str());
            let open = self.browse.open.contains(&e.path);
            let loading = self.browse.loading.contains(&e.path);
            let path = e.path.clone();
            let is_dir = e.is_dir;
            list = list.child(
                h_flex()
                    .id(("browse-row", i))
                    .gap_1p5()
                    .pl(px(8. + depth as f32 * 14.))
                    .pr_2()
                    .h(px(26.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(theme.list_active))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(div().flex_none().w(px(12.)).when(is_dir, |d| d.child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall().text_color(theme.muted_foreground))))
                    .child(icon(if is_dir { if open { IconName::FolderOpen } else { IconName::Folder } } else { IconName::File }).xsmall().text_color(if is_dir { theme.yellow } else { theme.muted_foreground }))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(e.name.clone()))
                    .when(loading, |d| d.child(gpui_kit::component::spinner::Spinner::new().xsmall()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if is_dir {
                            this.toggle_browse_dir(path.clone(), cx);
                        } else {
                            this.open_browse_file(path.clone(), cx);
                        }
                    })),
            );
        }
        v_flex()
            .size_full()
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().id("browse-tree").flex_none().w(px(320.)).h_full().overflow_y_scroll().border_r_1().border_color(theme.border).bg(theme.sidebar).child(list))
                    .child(div().flex_1().min_w_0().h_full().child(self.render_browse_file(cx))),
            )
            .into_any_element()
    }
}

fn format_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.),
        b => format!("{b} B"),
    }
}
