use cairn_core::log::{CommitInfo, CommitQuery};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, card, commit_row, empty_state, framed_if, hstack, icon, page_heading, spacer, toggle_pill, tr};

/// The fields and results of the Search commits page.
pub(crate) struct SearchState {
    pub message: Entity<InputState>,
    pub author: Entity<InputState>,
    pub path: Entity<InputState>,
    pub code: Entity<InputState>,
    pub since: Entity<InputState>,
    pub regex: bool,
    pub this_branch: bool,
    pub loading: bool,
    pub results: Option<Result<Vec<CommitInfo>, String>>,
    _subs: Vec<Subscription>,
}

impl SearchState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<RepoView>) -> Self {
        let field = |key: &str, window: &mut Window, cx: &mut Context<RepoView>| cx.new(|cx| InputState::new(window, cx).placeholder(tr(key)));
        let message = field("search.message_placeholder", window, cx);
        let author = field("search.author_placeholder", window, cx);
        let path = field("search.path_placeholder", window, cx);
        let code = field("search.code_placeholder", window, cx);
        let since = field("search.since_placeholder", window, cx);
        let subs = [&message, &author, &path, &code, &since]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this: &mut RepoView, _, ev: &InputEvent, cx| match ev {
                    InputEvent::PressEnter { .. } => this.run_search(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                })
            })
            .collect();
        Self { message, author, path, code, since, regex: false, this_branch: false, loading: false, results: None, _subs: subs }
    }

    fn query(&self, cx: &App) -> CommitQuery {
        let v = |s: &Entity<InputState>| s.read(cx).value().to_string();
        CommitQuery { message: v(&self.message), author: v(&self.author), path: v(&self.path), code: v(&self.code), code_regex: self.regex, since: v(&self.since), until: String::new(), branch: self.this_branch.then(|| "HEAD".to_string()), limit: 300 }
    }
}

impl RepoView {
    /// Opens the Search commits page with the message field filled in, such as from the History
    /// search box.
    ///
    /// # Arguments
    /// * `text` - what to look for in commit messages
    pub(crate) fn open_search(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.page = Page::Search;
        if !text.trim().is_empty() {
            self.search_page.message.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        if !self.search_page.query(cx).is_empty() {
            self.run_search(cx);
        }
        self.search_page.message.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let q = self.search_page.query(cx);
        if q.is_empty() {
            self.search_page.results = None;
            cx.notify();
            return;
        }
        self.search_page.loading = true;
        let git = self.git();
        self.spawn_bg(
            move || cairn_core::log::search(&git, &q).map_err(|e| crate::repo_view::friendly_error(&e)),
            |this, found, _| {
                this.search_page.loading = false;
                this.search_page.results = Some(found);
            },
            cx,
        );
        cx.notify();
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = &self.search_page;
        for input in [&s.message, &s.author, &s.path, &s.code, &s.since] {
            input.update(cx, |st, cx| st.set_value("", window, cx));
        }
        self.search_page.results = None;
        cx.notify();
    }

    pub(crate) fn render_search(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let s = &self.search_page;
        let has_query = !s.query(cx).is_empty();
        let field = |label: SharedString, hint: Option<SharedString>, input: &Entity<InputState>, ic: IconName, cx: &App| {
            v_flex()
                .gap_1()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(label))
                .child(Input::new(input).prefix(icon(ic).small().text_color(cx.theme().muted_foreground)))
                .when_some(hint, |d, h| d.child(div().text_xs().text_color(cx.theme().muted_foreground).whitespace_normal().child(h)))
        };
        let form = card(cx).child(
            v_flex()
                .p_4()
                .gap_3()
                .child(field(tr("search.message"), None, &s.message, IconName::MessageSquare, cx))
                .child(field(tr("search.author"), None, &s.author, IconName::UserRound, cx))
                .child(field(tr("search.path"), Some(tr("search.path_hint")), &s.path, IconName::FileSearch, cx))
                .child(field(tr("search.code"), Some(tr("search.code_hint")), &s.code, IconName::SearchCode, cx))
                .child(field(tr("search.since"), Some(tr("search.since_hint")), &s.since, IconName::CalendarClock, cx))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_1p5()
                        .child(toggle_pill("search-regex", tr("search.regex"), s.regex, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.search_page.regex = !this.search_page.regex;
                            this.run_search(cx);
                        })))
                        .child(toggle_pill("search-branch", tr("search.this_branch"), s.this_branch, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.search_page.this_branch = !this.search_page.this_branch;
                            this.run_search(cx);
                        }))),
                )
                .child(
                    hstack()
                        .gap_2()
                        .pt_1()
                        .child(Button::new("search-clear").ghost().small().label(tr("search.clear")).on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))))
                        .child(spacer())
                        .child(framed_if(Button::new("search-go"), Tone::Accent, has_query && !s.loading, cx).icon(icon(IconName::Search)).label(tr("search.go")).loading(s.loading).on_click(cx.listener(|this, _, _, cx| this.run_search(cx)))),
                ),
        );
        let results = match &s.results {
            None => empty_state(IconName::TextSearch, tr("search.start_title"), tr("search.start_body"), cx).into_any_element(),
            Some(Err(e)) => empty_state(IconName::TriangleAlert, tr("search.failed"), e.clone(), cx).into_any_element(),
            Some(Ok(list)) if list.is_empty() => empty_state(IconName::SearchX, tr("search.none_title"), tr("search.none_body"), cx).into_any_element(),
            Some(Ok(list)) => {
                let mut rows = v_flex().gap_0p5().p_2();
                for (i, c) in list.iter().enumerate() {
                    let id = c.id.clone();
                    let selected = self.selected_commit.as_deref() == Some(c.id.as_str());
                    rows = rows.child(commit_row(("search-hit", i), &c.id, &c.summary, &c.author, c.time, selected, cx).on_click(cx.listener(move |this, _, _, cx| {
                        this.page = Page::History;
                        if let Some(ix) = this.commits.iter().position(|x| x.id == id) {
                            this.history_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                        }
                        this.select_commit(id.clone(), cx);
                    })));
                }
                v_flex()
                    .size_full()
                    .child(
                        hstack()
                            .flex_none()
                            .px_4()
                            .h(px(40.))
                            .border_b_1()
                            .border_color(theme.border)
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(crate::widgets::plural("search.found", list.len())))
                            .child(spacer())
                            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("search.click_hint"))),
                    )
                    .child(div().id("search-hits").flex_1().min_h_0().overflow_y_scroll().child(rows))
                    .into_any_element()
            }
        };
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .id("search-form")
                    .flex_none()
                    .w(px(380.))
                    .h_full()
                    .overflow_y_scroll()
                    .p_5()
                    .gap_4()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(page_heading(tr("page.search"), tr("search.subtitle"), cx))
                    .child(form),
            )
            .child(div().flex_1().min_w_0().h_full().child(results))
    }
}
