use cairn_core::submodule::{self, Submodule, SubmoduleState as State};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{OpenRepoEvent, RepoView};
use crate::widgets::{Tone, card, card_header, count_chip, framed, hstack, icon, icon_tile, page_heading, spacer, tr};

#[derive(Default)]
pub(crate) struct SubmoduleState {
    pub loading: bool,
    pub loaded: bool,
    pub list: Vec<Submodule>,
    pub error: Option<String>,
}

impl RepoView {
    pub(crate) fn load_submodules(&mut self, cx: &mut Context<Self>) {
        if self.submodules.loading {
            return;
        }
        self.submodules.loading = true;
        let git = self.git();
        self.spawn_bg(move || submodule::list(&git).map_err(|e| crate::repo_view::friendly_error(&e)), |this, result, _| {
            this.submodules.loading = false;
            this.submodules.loaded = true;
            match result {
                Ok(list) => {
                    this.submodules.list = list;
                    this.submodules.error = None;
                }
                Err(e) => this.submodules.error = Some(e),
            }
        }, cx);
        cx.notify();
    }

    fn submodule_op(&mut self, label: SharedString, op: impl FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(label, None, op, window, cx);
        self.after_op(|v, cx| {
            v.submodules.loaded = false;
            v.load_submodules(cx);
        }, cx);
    }

    fn add_submodule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        crate::dialogs::text_prompt(tr("submodules.add_title"), tr("submodules.add_body"), tr("submodules.url_placeholder"), String::new(), tr("submodules.add"), window, cx, move |url, window, cx| {
            let path = submodule::folder_for(&url);
            view.update(cx, |v, cx| v.submodule_op(tr("op.add_submodule"), move |git| submodule::add(git, &url, &path), window, cx));
        });
    }

    pub(crate) fn render_submodules(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if !self.submodules.loaded && !self.submodules.loading {
            self.load_submodules(cx);
        }
        let list = self.submodules.list.clone();
        let busy = self.busy.is_some();
        let mut col = v_flex().id("submodules-scroll").size_full().overflow_y_scroll().p_5().gap_4();
        col = col.child(
            hstack()
                .flex_none()
                .items_start()
                .gap_2()
                .child(page_heading(tr("page.submodules"), tr("submodules.subtitle"), cx))
                .child(spacer())
                .when(!list.is_empty(), |d| {
                    d.child(crate::widgets::framed_if(Button::new("sub-sync"), Tone::Plain, !busy, cx).icon(icon(IconName::Link)).label(tr("submodules.sync")).tooltip(tr("submodules.sync_tip")).on_click(cx.listener(|this, _, window, cx| this.submodule_op(tr("submodules.sync"), submodule::sync, window, cx))))
                        .child(crate::widgets::framed_if(Button::new("sub-update-all"), Tone::Plain, !busy, cx).icon(icon(IconName::RefreshCw)).label(tr("submodules.update_all")).on_click(cx.listener(|this, _, window, cx| this.submodule_op(tr("op.update_submodules"), |git| submodule::update(git, None), window, cx))))
                })
                .child(crate::widgets::framed_if(Button::new("sub-add"), Tone::Accent, !busy, cx).icon(icon(IconName::Plus)).label(tr("submodules.add")).on_click(cx.listener(|this, _, window, cx| this.add_submodule(window, cx)))),
        );
        if let Some(e) = self.submodules.error.clone() {
            col = col.child(div().p_3().rounded(px(8.)).bg(theme.red.opacity(0.1)).text_sm().text_color(theme.red).child(e));
        }
        if self.submodules.loading && !self.submodules.loaded {
            return col.child(div().py_6().flex().justify_center().child(gpui_kit::component::spinner::Spinner::new())).into_any_element();
        }
        if list.is_empty() {
            return col
                .child(
                    card(cx).child(
                        v_flex()
                            .items_center()
                            .gap_2()
                            .p_6()
                            .child(icon_tile(IconName::Boxes, theme.primary).size(px(44.)).rounded(px(12.)))
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("submodules.empty_title")))
                            .child(div().max_w(px(460.)).text_sm().text_center().text_color(theme.muted_foreground).child(tr("submodules.empty_body"))),
                    ),
                )
                .into_any_element();
        }
        let mut c = card(cx).child(card_header(IconName::Boxes, tr("page.submodules"), Some(list.len()), cx));
        for (i, s) in list.iter().enumerate() {
            let (label, color) = match s.state {
                State::UpToDate => (tr("submodules.up_to_date"), theme.green),
                State::NotInitialized => (tr("submodules.not_initialized"), theme.yellow),
                State::Changed => (tr("submodules.changed"), theme.blue),
                State::Conflicted => (tr("submodules.conflicted"), theme.red),
            };
            let path = s.path.clone();
            let open_path = self.repo.root.join(&s.path);
            let initialized = s.state != State::NotInitialized;
            c = c.child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .px(px(14.))
                    .py(px(10.))
                    .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                    .hover(|st| st.bg(theme.list_hover))
                    .child(icon_tile(IconName::Package, color))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(hstack().gap_2().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).font_family(theme.mono_font_family.clone()).child(s.path.clone())).child(count_chip(label, color.opacity(0.16), color)))
                            .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(format!(
                                "{}{}{}",
                                s.commit.chars().take(7).collect::<String>(),
                                if s.describe.is_empty() { String::new() } else { format!(" ({})", s.describe) },
                                if s.url.is_empty() { String::new() } else { format!(" · {}", s.url) }
                            ))),
                    )
                    .when(s.state != State::UpToDate, |d| {
                        d.child(crate::widgets::framed_if(Button::new(("sub-update", i)), Tone::Plain, !busy, cx).h(px(28.)).icon(icon(IconName::Download)).label(if initialized { tr("submodules.update") } else { tr("submodules.init") }).on_click(cx.listener(move |this, _, window, cx| {
                            let p = path.clone();
                            this.submodule_op(t!("op.update_submodule", path = p.clone()).to_string().into(), move |git| submodule::update(git, Some(&p)), window, cx);
                        })))
                    })
                    .when(initialized, |d| {
                        d.child(framed(Button::new(("sub-open", i)), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::FolderOpen)).label(tr("repo.open")).on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenRepoEvent(open_path.clone())))))
                    }),
            );
        }
        col.child(c).into_any_element()
    }
}
