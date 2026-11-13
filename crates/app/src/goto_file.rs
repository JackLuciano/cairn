use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::RepoView;
use crate::widgets::{hstack, icon, kbd, tr};

const SHOWN: usize = 60;

/// What happens to the picked file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    FileHistory,
    LineHistory,
    Editor,
}

/// "Go to file": type part of a name to find any tracked file, then open its history, its line
/// history or the file itself.
pub(crate) struct GotoFile {
    repo: Entity<RepoView>,
    input: Entity<InputState>,
    files: Option<Vec<String>>,
    selected: usize,
    _sub: Subscription,
}

/// Ranks a path for a query: name matches beat folder matches, and earlier matches beat later.
///
/// # Arguments
/// * `path` - a repository-relative path
/// * `q` - what was typed
///
/// # Returns
/// A rank, lower is better, or `None` when the path does not match.
fn rank(path: &str, q: &str) -> Option<usize> {
    let name = path.rsplit('/').next().unwrap_or(path);
    crate::palette::fuzzy(name, q).map(|s| s * 2).or_else(|| crate::palette::fuzzy(path, q).map(|s| 10 + s))
}

impl GotoFile {
    fn filtered(&self, cx: &App) -> Vec<String> {
        let Some(files) = &self.files else { return Vec::new() };
        let q = self.input.read(cx).value().trim().to_string();
        if q.is_empty() {
            return files.iter().take(SHOWN).cloned().collect();
        }
        let mut hits: Vec<(usize, usize, &String)> = files.iter().filter_map(|f| rank(f, &q).map(|r| (r, f.len(), f))).collect();
        hits.sort();
        hits.into_iter().take(SHOWN).map(|(_, _, f)| f.clone()).collect()
    }

    fn open(&mut self, how: Open, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.filtered(cx).get(self.selected).cloned() else { return };
        window.close_dialog(cx);
        self.repo.update(cx, |v, cx| match how {
            Open::FileHistory => {
                v.load_blame_files(cx);
                v.open_file_history(path, cx);
            }
            Open::LineHistory => v.open_blame(path, cx),
            Open::Editor => {
                let full = v.repo.root.join(&path);
                v.open_in_editor(&full, cx);
            }
        });
    }
}

impl Render for GotoFile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let list = self.filtered(cx);
        if self.selected >= list.len() {
            self.selected = list.len().saturating_sub(1);
        }
        let mut rows = v_flex().gap_0p5();
        match &self.files {
            None => rows = rows.child(crate::skeleton::sidebar(8, cx)),
            Some(_) if list.is_empty() => rows = rows.child(div().px_3().py_4().text_sm().text_color(theme.muted_foreground).child(tr("goto_file.none"))),
            Some(_) => {}
        }
        for (i, path) in list.iter().enumerate() {
            let on = i == self.selected;
            let (dir, name) = match path.rsplit_once('/') {
                Some((d, n)) => (format!("{d}/"), n.to_string()),
                None => (String::new(), path.clone()),
            };
            rows = rows.child(
                h_flex()
                    .id(("goto", i))
                    .gap_2()
                    .px_3()
                    .h(px(34.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(icon(IconName::File).small().text_color(theme.muted_foreground))
                    .child(h_flex().flex_1().min_w_0().gap_1p5().child(div().flex_none().text_sm().font_weight(FontWeight::MEDIUM).child(name)).child(div().min_w_0().truncate().text_xs().text_color(theme.muted_foreground).child(dir)))
                    .when(on, |d| {
                        d.child(Button::new(("goto-lines", i)).xsmall().ghost().icon(icon(IconName::FileClock)).tooltip(tr("changes.line_history")).on_click(cx.listener(|this, _, window, cx| this.open(Open::LineHistory, window, cx))))
                            .child(Button::new(("goto-edit", i)).xsmall().ghost().icon(icon(IconName::Code)).tooltip(tr("actions.open_editor")).on_click(cx.listener(|this, _, window, cx| this.open(Open::Editor, window, cx))))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = i;
                        this.open(Open::FileHistory, window, cx);
                    })),
            );
        }
        let total = self.files.as_ref().map(Vec::len).unwrap_or(0);
        v_flex()
            .w_full()
            .gap_2()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| match ev.keystroke.key.as_str() {
                "down" => {
                    this.selected += 1;
                    cx.notify();
                }
                "up" => {
                    this.selected = this.selected.saturating_sub(1);
                    cx.notify();
                }
                "enter" if ev.keystroke.modifiers.control => this.open(Open::LineHistory, window, cx),
                "enter" if ev.keystroke.modifiers.shift => this.open(Open::Editor, window, cx),
                _ => {}
            }))
            .child(Input::new(&self.input).prefix(icon(IconName::FileSearch).small()))
            .child(div().id("goto-list").max_h(px(400.)).overflow_y_scroll().child(rows))
            .child(
                hstack()
                    .gap_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(hstack().gap_1().child(kbd("Enter", cx)).child(tr("actions.file_history")))
                    .child(hstack().gap_1().child(kbd("Ctrl Enter", cx)).child(tr("changes.line_history")))
                    .child(hstack().gap_1().child(kbd("Shift Enter", cx)).child(tr("actions.open_editor")))
                    .child(div().flex_1())
                    .child(crate::widgets::plural("goto_file.count", total)),
            )
    }
}

/// Opens "Go to file" once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
pub(crate) fn open(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, window, cx));
}

fn open_now(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("goto_file.placeholder")));
    let git = repo.read(cx).git();
    let panel = cx.new(|cx| {
        let sub = cx.subscribe_in(&input, window, |this: &mut GotoFile, _, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::Change => this.selected = 0,
                InputEvent::PressEnter { .. } => this.open(Open::FileHistory, window, cx),
                _ => {}
            }
            cx.notify();
        });
        cx.spawn(async move |this, cx| {
            let files = cx.background_executor().spawn(async move { git.run(["ls-files", "-z"]).map(|o| o.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default() }).await;
            let _ = this.update(cx, |this: &mut GotoFile, cx| {
                this.files = Some(files);
                cx.notify();
            });
        })
        .detach();
        GotoFile { repo, input: input.clone(), files: None, selected: 0, _sub: sub }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        dialog.w(px(680.)).margin_top(px(80.)).close_button(false).child(panel.clone())
    });
}
