use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use cairn_core::people::Person;
use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::RepoView;
use crate::widgets::{avatar, hstack, icon, tr};

impl RepoView {
    /// Lists the repository's contributors and lets the user pick co-authors for the commit.
    /// The picked people are written as `Co-authored-by` lines at the end of the description.
    pub(crate) fn open_coauthors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let git = self.git();
        let view = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let people = cx.background_executor().spawn(async move { cairn_core::people::contributors(&git) }).await;
            let _ = cx.update(|window, cx| match people {
                Ok(p) if !p.is_empty() => show(view, p, window, cx),
                Ok(_) => window.push_notification(Notification::info(tr("coauthors.none")), cx),
                Err(e) => window.push_notification(Notification::error(crate::repo_view::friendly_error(&e)), cx),
            });
        })
        .detach();
    }
}

fn show(view: Entity<RepoView>, people: Vec<Person>, window: &mut Window, cx: &mut App) {
    let body = view.read(cx).body.read(cx).value().to_string();
    let named = cairn_core::people::co_author_emails(&body);
    let picked: Rc<RefCell<BTreeSet<String>>> = Rc::new(RefCell::new(people.iter().map(|p| p.email.to_lowercase()).filter(|e| named.contains(e)).collect()));
    let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("coauthors.search")));
    cx.subscribe(&search, |_, _: &InputEvent, cx| cx.refresh_windows()).detach();
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            search.focus_handle(cx).focus(window, cx);
        }
        let theme = cx.theme().clone();
        let q = search.read(cx).value().trim().to_lowercase();
        let mut list = v_flex().gap_0p5();
        let shown: Vec<(usize, &Person)> = people.iter().enumerate().filter(|(_, p)| q.is_empty() || p.name.to_lowercase().contains(&q) || p.email.to_lowercase().contains(&q)).take(80).collect();
        for (i, p) in &shown {
            let key = p.email.to_lowercase();
            let on = picked.borrow().contains(&key);
            let picked = picked.clone();
            list = list.child(
                hstack()
                    .id(("coauthor", *i))
                    .gap_2p5()
                    .px_2()
                    .py_1p5()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.primary.opacity(0.1)))
                    .hover(|s| s.bg(theme.list_hover))
                    .child(Checkbox::new(("coauthor-pick", *i)).checked(on))
                    .child(avatar(&p.name, px(24.), cx))
                    .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(p.name.clone())).child(div().text_xs().truncate().text_color(theme.muted_foreground).child(p.email.clone())))
                    .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("coauthors.commits", p.commits as usize)))
                    .on_click(move |_, window, _| {
                        let mut set = picked.borrow_mut();
                        if !set.remove(&key) {
                            set.insert(key.clone());
                        }
                        window.refresh();
                    }),
            );
        }
        if shown.is_empty() {
            list = list.child(div().px_2().py_4().text_sm().text_color(theme.muted_foreground).child(tr("coauthors.no_match")));
        }
        let count = picked.borrow().len();
        let (view, people, picked) = (view.clone(), people.clone(), picked.clone());
        dialog
            .title(tr("coauthors.title"))
            .w(px(520.))
            .bg(theme.popover.opacity(1.))
            .child(
                v_flex()
                    .gap_3()
                    .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(tr("coauthors.body")))
                    .child(Input::new(&search).prefix(icon(IconName::Search).small()))
                    .child(div().id("coauthor-list").h(px(320.)).overflow_y_scroll().rounded(px(8.)).border_1().border_color(theme.border).p_1().child(list)),
            )
            .footer(crate::dialogs::footer(if count == 0 { tr("coauthors.ok_zero") } else { crate::widgets::plural("coauthors.ok", count).into() }, false, move |window, cx| {
                let chosen: Vec<Person> = people.iter().filter(|p| picked.borrow().contains(&p.email.to_lowercase())).cloned().collect();
                view.update(cx, |v, cx| {
                    let text = cairn_core::people::set_co_authors(&v.body.read(cx).value(), &chosen);
                    v.body.update(cx, |s, cx| s.set_value(text, window, cx));
                    cx.notify();
                });
                window.close_dialog(cx);
            }))
    });
}
