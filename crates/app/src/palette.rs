use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::CairnApp;
use crate::repo_view::Page;
use crate::widgets::{hstack, icon, kbd, tr};

gpui_kit::actions!(
    cairn,
    [
        OpenPalette,
        OpenSettings,
        Fetch,
        Pull,
        Push,
        Stash,
        UndoHistory,
        AskAi,
        NewBranch,
        Refresh,
        NextTab,
        PrevTab,
        CloseTab,
        GoHistory,
        GoChanges,
        GoBranches,
        CommitStaged
    ]
);

/// Opens the keyboard map: every shortcut, plus the Vim keys when they are on.
///
/// # Arguments
/// * `vim` - whether Vim keys are turned on
pub(crate) fn open_keymap(vim: bool, window: &mut Window, cx: &mut App) {
    let keys: Vec<(&'static str, &'static str)> = vec![
        ("Ctrl K", "palette.key_palette"),
        ("Ctrl ,", "palette.key_settings"),
        ("Ctrl 1", "page.history"),
        ("Ctrl 2", "page.changes"),
        ("Ctrl 3", "page.branches"),
        ("Ctrl Enter", "palette.key_commit"),
        ("Ctrl Shift F", "toolbar.fetch"),
        ("Ctrl Shift L", "toolbar.pull"),
        ("Ctrl Shift K", "toolbar.push"),
        ("Ctrl Shift S", "toolbar.stash"),
        ("Ctrl Shift B", "toolbar.new_branch"),
        ("Ctrl Shift A", "toolbar.ask_ai"),
        ("Ctrl Z", "toolbar.undo"),
        ("F5", "toolbar.refresh"),
        ("Ctrl Tab", "palette.key_next_tab"),
        ("Ctrl W", "tabs.close"),
        ("?", "palette.key_map"),
    ];
    let vim_keys: Vec<(&'static str, &'static str)> = vec![
        ("j / k", "palette.vim_move"),
        ("g / G", "palette.vim_ends"),
        ("/", "palette.vim_search"),
        ("s", "palette.vim_stage"),
        ("f", "toolbar.fetch"),
        ("p", "toolbar.push"),
    ];
    window.defer(cx, move |window, cx| {
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme().clone();
            let grid = |list: &[(&'static str, &'static str)], cx: &App| {
                div().grid().grid_cols(2).gap_x_6().gap_y_2().children(list.iter().map(|(k, label)| hstack().gap_3().child(div().w(px(110.)).child(kbd(k, cx))).child(div().text_sm().child(tr(label)))))
            };
            dialog
                .w(px(640.))
                .title(tr("palette.key_map"))
                .child(
                    v_flex()
                        .gap_4()
                        .child(grid(&keys, cx))
                        .when(vim, |d| {
                            d.child(div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(tr("palette.vim_title").to_uppercase()))
                                .child(grid(&vim_keys, cx))
                        })
                        .when(!vim, |d| d.child(div().text_xs().text_color(theme.muted_foreground).child(tr("palette.vim_off_hint")))),
                )
        });
    });
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-k", OpenPalette, None),
        KeyBinding::new("ctrl-shift-p", OpenPalette, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("ctrl-shift-f", Fetch, None),
        KeyBinding::new("ctrl-shift-l", Pull, None),
        KeyBinding::new("ctrl-shift-k", Push, None),
        KeyBinding::new("ctrl-shift-s", Stash, None),
        KeyBinding::new("ctrl-z", UndoHistory, Some("RepoView")),
        KeyBinding::new("ctrl-shift-a", AskAi, None),
        KeyBinding::new("ctrl-shift-b", NewBranch, None),
        KeyBinding::new("f5", Refresh, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("ctrl-1", GoHistory, None),
        KeyBinding::new("ctrl-2", GoChanges, None),
        KeyBinding::new("ctrl-3", GoBranches, None),
        KeyBinding::new("ctrl-enter", CommitStaged, None),
    ]);
}

#[derive(Clone)]
struct Item {
    group: &'static str,
    label: SharedString,
    keys: Option<&'static str>,
    icon: IconName,
    run: Run,
}

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

pub(crate) struct Palette {
    input: Entity<InputState>,
    items: Vec<Item>,
    selected: usize,
    _sub: Subscription,
}

fn fuzzy(text: &str, q: &str) -> Option<usize> {
    let t = text.to_lowercase();
    let q = q.to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    if let Some(i) = t.find(&q) {
        return Some(if i == 0 { 0 } else { 1 });
    }
    let mut chars = t.chars();
    for c in q.chars().filter(|c| !c.is_whitespace()) {
        chars.find(|x| *x == c)?;
    }
    Some(2)
}

impl Palette {
    fn filtered(&self, cx: &App) -> Vec<Item> {
        let q = self.input.read(cx).value().to_string();
        let mut list: Vec<(usize, Item)> = self.items.iter().filter_map(|i| fuzzy(&i.label, &q).map(|s| (s, i.clone()))).collect();
        list.sort_by_key(|(s, _)| *s);
        list.into_iter().map(|(_, i)| i).take(40).collect()
    }

    fn run_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.filtered(cx);
        if let Some(item) = list.get(self.selected).cloned() {
            window.close_dialog(cx);
            (item.run)(window, cx);
        }
    }
}

impl Render for Palette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let list = self.filtered(cx);
        if self.selected >= list.len() {
            self.selected = list.len().saturating_sub(1);
        }
        let mut rows = v_flex().gap_0p5();
        for (i, item) in list.iter().enumerate() {
            let on = i == self.selected;
            rows = rows.child(
                div()
                    .id(("pal", i))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .h(px(34.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.list_active))
                    .when(!on, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(icon(item.icon).small().text_color(theme.muted_foreground))
                    .child(div().w(px(90.)).text_xs().text_color(theme.muted_foreground).child(tr(item.group)))
                    .child(div().flex_1().truncate().text_sm().child(item.label.clone()))
                    .when_some(item.keys, |d, k| d.child(kbd(k, cx)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.selected = i;
                        this.run_selected(window, cx);
                    })),
            );
        }
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
                "enter" => this.run_selected(window, cx),
                _ => {}
            }))
            .child(Input::new(&self.input).prefix(icon(IconName::Search).small()))
            .child(div().id("pal-list").max_h(px(380.)).overflow_y_scroll().child(rows))
            .child(hstack().gap_3().text_xs().text_color(theme.muted_foreground).child(tr("palette.hint")))
    }
}

/// Opens the palette once the caller's update of `app` has finished.
///
/// # Arguments
/// * `app` - the application entity, possibly mid-update
pub(crate) fn open(app: Entity<CairnApp>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(app, window, cx));
}

fn open_now(app: Entity<CairnApp>, window: &mut Window, cx: &mut App) {
    let mut items: Vec<Item> = Vec::new();
    let tab = app.read(cx).active_tab();
    let cmd = |group: &'static str, label: &str, keys: Option<&'static str>, icon: IconName, run: Run| Item { group, label: tr(label), keys, icon, run };
    if let Some(tab) = tab.clone() {
        let pages: [(&str, Page, IconName, Option<&'static str>); 9] = [
            ("page.history", Page::History, IconName::GitCommitHorizontal, Some("Ctrl 1")),
            ("page.changes", Page::Changes, IconName::FileDiff, Some("Ctrl 2")),
            ("page.branches", Page::Branches, IconName::GitBranch, Some("Ctrl 3")),
            ("page.stack", Page::Stack, IconName::Layers, None),
            ("page.rebase", Page::Rebase, IconName::GitPullRequestArrow, None),
            ("page.absorb", Page::Absorb, IconName::Combine, None),
            ("page.conflicts", Page::Conflicts, IconName::GitMerge, None),
            ("page.blame", Page::Blame, IconName::FileClock, None),
            ("page.bisect", Page::Bisect, IconName::Bug, None),
        ];
        for (label, page, ic, keys) in pages {
            let t = tab.clone();
            items.push(cmd("palette.go", label, keys, ic, Rc::new(move |w, cx| t.update(cx, |v, cx| v.open_page(page, w, cx)))));
        }
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.fetch", Some("Ctrl Shift F"), IconName::RefreshCw, Rc::new(move |w, cx| t.update(cx, |v, cx| v.fetch_action(w, cx)))));
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.pull", Some("Ctrl Shift L"), IconName::ArrowDownToLine, Rc::new(move |w, cx| t.update(cx, |v, cx| v.pull_action(w, cx)))));
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.push", Some("Ctrl Shift K"), IconName::ArrowUpToLine, Rc::new(move |w, cx| t.update(cx, |v, cx| v.push(false, w, cx)))));
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.stash", Some("Ctrl Shift S"), IconName::Archive, Rc::new(move |w, cx| t.update(cx, |v, cx| v.stash_action(w, cx)))));
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.new_branch", Some("Ctrl Shift B"), IconName::GitBranchPlus, Rc::new(move |w, cx| crate::dialogs::new_branch(t.clone(), None, w, cx))));
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.undo", Some("Ctrl Z"), IconName::Undo2, Rc::new(move |w, cx| crate::dialogs::undo_history(t.clone(), w, cx))));
        let t = tab.clone();
        items.push(cmd("palette.git", "toolbar.force_push", None, IconName::TriangleAlert, Rc::new(move |w, cx| crate::dialogs::confirm_force_push(t.clone(), w, cx))));
        let t = tab.clone();
        items.push(cmd("palette.ai", "toolbar.ask_ai", Some("Ctrl Shift A"), IconName::Sparkles, Rc::new(move |w, cx| crate::assist::open(t.clone(), w, cx))));
        let t = tab.clone();
        items.push(cmd("palette.repo", "palette.repo_settings", None, IconName::Settings2, Rc::new(move |w, cx| crate::repo_settings::open(t.clone(), w, cx))));
        let branches: Vec<(String, bool)> = tab.read(cx).branches.iter().filter(|b| !b.is_head).map(|b| (b.name.clone(), b.is_remote)).collect();
        for (name, remote) in branches {
            let t = tab.clone();
            let n = name.clone();
            items.push(Item {
                group: "palette.branch",
                label: name.into(),
                keys: None,
                icon: if remote { IconName::Cloud } else { IconName::GitBranch },
                run: Rc::new(move |w, cx| t.update(cx, |v, cx| v.checkout(n.clone(), remote, w, cx))),
            });
        }
    }
    let a = app.clone();
    items.push(cmd("palette.app", "settings.title", Some("Ctrl ,"), IconName::Settings, Rc::new(move |w, cx| {
        let s = a.read(cx).settings.clone();
        crate::settings_dialog::open(a.clone(), s, w, cx)
    })));
    let vim = app.read(cx).settings.keyboard.vim_keys;
    items.push(cmd("palette.app", "palette.key_map", Some("?"), IconName::Keyboard, Rc::new(move |w, cx| open_keymap(vim, w, cx))));
    let reduce_motion = app.read(cx).settings.appearance.reduce_motion;
    items.push(cmd("palette.app", "whats_new.button", None, IconName::Sparkles, Rc::new(move |w, cx| crate::whats_new::open(reduce_motion, w, cx))));
    let a = app.clone();
    items.push(cmd("palette.app", "category.new_title", None, IconName::FolderPlus, Rc::new(move |w, cx| a.update(cx, |this, cx| this.new_category(w, cx)))));
    let a = app.clone();
    items.push(cmd("palette.app", "repo.add_existing", None, IconName::FolderOpen, Rc::new(move |w, cx| a.update(cx, |this, cx| this.add_repositories(None, w, cx)))));
    let a = app.clone();
    items.push(cmd("palette.app", "repo.clone", None, IconName::CloudDownload, Rc::new(move |w, cx| a.update(cx, |this, cx| this.clone_repository(None, w, cx)))));
    let repos: Vec<(std::path::PathBuf, String)> = app.read(cx).library.repos.iter().map(|r| (r.path.clone(), r.name.clone())).collect();
    for (path, name) in repos {
        let a = app.clone();
        items.push(Item { group: "palette.open_repo", label: name.into(), keys: None, icon: IconName::FolderGit2, run: Rc::new(move |w, cx| a.update(cx, |this, cx| this.open_repo(&path, w, cx))) });
    }
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("palette.placeholder")));
    let palette = cx.new(|cx| {
        let sub = cx.subscribe_in(&input, window, |this: &mut Palette, _, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::Change => this.selected = 0,
                InputEvent::PressEnter { .. } => this.run_selected(window, cx),
                _ => {}
            }
            cx.notify();
        });
        Palette { input: input.clone(), items, selected: 0, _sub: sub }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        dialog.w(px(640.)).margin_top(px(80.)).close_button(false).child(palette.clone())
    });
}
