use cairn_core::lfs::{self, LfsStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, card, card_header, count_chip, framed, hstack, icon, icon_tile, page_heading, spacer, tr};

#[derive(Default)]
pub(crate) struct LfsPageState {
    pub loading: bool,
    pub loaded: bool,
    pub status: LfsStatus,
}

impl RepoView {
    pub(crate) fn load_lfs(&mut self, cx: &mut Context<Self>) {
        if self.lfs.loading {
            return;
        }
        self.lfs.loading = true;
        let git = self.git();
        self.spawn_bg(move || {
            let with_locks = cairn_core::refs::list_remotes(&git).map(|r| !r.is_empty()).unwrap_or(false);
            lfs::status(&git, with_locks).unwrap_or_default()
        }, |this, status, _| {
            this.lfs.loading = false;
            this.lfs.loaded = true;
            this.lfs.status = status;
        }, cx);
        cx.notify();
    }

    fn lfs_op(&mut self, label: SharedString, op: impl FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(label, None, op, window, cx);
        self.after_op(|v, cx| {
            v.lfs.loaded = false;
            v.load_lfs(cx);
        }, cx);
    }

    fn lfs_track_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        crate::dialogs::text_prompt(tr("lfs.track_title"), tr("lfs.track_body"), SharedString::from("*.psd"), String::new(), tr("lfs.track"), window, cx, move |pattern, window, cx| {
            view.update(cx, |v, cx| v.lfs_op(tr("op.lfs_track"), move |git| lfs::track(git, &pattern), window, cx));
        });
    }

    pub(crate) fn render_lfs(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if !self.lfs.loaded && !self.lfs.loading {
            self.load_lfs(cx);
        }
        let busy = self.busy.is_some();
        let st = self.lfs.status.clone();
        let mut col = v_flex().id("lfs-scroll").size_full().overflow_y_scroll().p_5().gap_4();
        col = col.child(
            hstack()
                .flex_none()
                .items_start()
                .gap_2()
                .child(page_heading(tr("page.lfs"), st.version.clone().unwrap_or_else(|| t!("lfs.subtitle").to_string()), cx))
                .child(spacer())
                .child(Button::new("lfs-refresh").small().ghost().icon(icon(IconName::RefreshCw)).tooltip(tr("toolbar.refresh")).on_click(cx.listener(|this, _, _, cx| {
                    this.lfs.loaded = false;
                    this.load_lfs(cx);
                })))
                .when(st.version.is_some() && !st.patterns.is_empty(), |d| {
                    d.child(crate::widgets::framed_if(Button::new("lfs-pull"), Tone::Plain, !busy, cx).icon(icon(IconName::Download)).label(tr("lfs.pull")).on_click(cx.listener(|this, _, window, cx| this.lfs_op(tr("lfs.pull"), lfs::pull, window, cx))))
                })
                .when(st.version.is_some(), |d| d.child(crate::widgets::framed_if(Button::new("lfs-track"), Tone::Accent, !busy, cx).icon(icon(IconName::Plus)).label(tr("lfs.track")).on_click(cx.listener(|this, _, window, cx| this.lfs_track_prompt(window, cx))))),
        );
        if self.lfs.loading && !self.lfs.loaded {
            return col.child(crate::skeleton::list(4, cx)).into_any_element();
        }
        if st.version.is_none() {
            return col
                .child(
                    card(cx).child(
                        v_flex()
                            .items_center()
                            .gap_2()
                            .p_6()
                            .child(icon_tile(IconName::HardDrive, theme.yellow).size(px(44.)).rounded(px(12.)))
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("lfs.missing_title")))
                            .child(div().max_w(px(460.)).text_sm().text_center().text_color(theme.muted_foreground).child(tr("lfs.missing_body")))
                            .child(framed(Button::new("lfs-install"), Tone::Plain, cx).icon(icon(IconName::ExternalLink)).label(tr("lfs.get")).on_click(|_, _, cx| cx.open_url("https://git-lfs.com"))),
                    ),
                )
                .into_any_element();
        }
        let mut patterns = card(cx).child(card_header(IconName::Shapes, tr("lfs.patterns"), Some(st.patterns.len()), cx));
        if st.patterns.is_empty() {
            patterns = patterns.child(div().px(px(14.)).py_4().text_sm().text_color(theme.muted_foreground).child(tr("lfs.no_patterns")));
        }
        for (i, p) in st.patterns.iter().enumerate() {
            let pattern = p.clone();
            patterns = patterns.child(
                hstack()
                    .gap_3()
                    .px(px(14.))
                    .py(px(8.))
                    .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                    .child(div().flex_1().text_sm().font_family(theme.mono_font_family.clone()).child(p.clone()))
                    .child(Button::new(("lfs-untrack", i)).small().ghost().label(tr("lfs.untrack")).disabled(busy).on_click(cx.listener(move |this, _, window, cx| {
                        let p = pattern.clone();
                        this.lfs_op(tr("op.lfs_untrack"), move |git| lfs::untrack(git, &p), window, cx);
                    }))),
            );
        }
        col = col.child(patterns);
        if st.locks_error.is_some() || !st.locks.is_empty() {
            let mut locks = card(cx).child(card_header(IconName::Lock, tr("lfs.locks"), Some(st.locks.len()), cx));
            if let Some(e) = st.locks_error.clone() {
                locks = locks.child(div().px(px(14.)).py_3().text_xs().text_color(theme.muted_foreground).child(t!("lfs.locks_unavailable", error = e).to_string()));
            }
            for (i, l) in st.locks.iter().enumerate() {
                let path = l.path.clone();
                locks = locks.child(
                    hstack()
                        .gap_3()
                        .px(px(14.))
                        .py(px(8.))
                        .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                        .child(icon(IconName::Lock).small().text_color(theme.yellow))
                        .child(v_flex().flex_1().min_w_0().child(div().truncate().text_sm().child(l.path.clone())).child(div().text_xs().text_color(theme.muted_foreground).child(t!("lfs.locked_by", who = l.owner.clone()).to_string())))
                        .child(Button::new(("lfs-unlock", i)).small().ghost().icon(icon(IconName::LockOpen)).label(tr("lfs.unlock")).disabled(busy).on_click(cx.listener(move |this, _, window, cx| {
                            let p = path.clone();
                            this.lfs_op(tr("op.lfs_unlock"), move |git| lfs::unlock(git, &p, false), window, cx);
                        }))),
                );
            }
            col = col.child(locks);
        }
        let missing = st.files.iter().filter(|f| !f.downloaded).count();
        let mut files = card(cx).child(card_header(IconName::Files, tr("lfs.files"), Some(st.files.len()), cx).when(missing > 0, |d| d.child(count_chip(t!("lfs.not_downloaded", n = missing).to_string(), theme.yellow.opacity(0.15), theme.yellow))));
        if st.files.is_empty() {
            files = files.child(div().px(px(14.)).py_4().text_sm().text_color(theme.muted_foreground).child(tr("lfs.no_files")));
        }
        for (i, f) in st.files.iter().take(500).enumerate() {
            let path = f.path.clone();
            let locked = st.locks.iter().any(|l| l.path == f.path);
            files = files.child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .px(px(14.))
                    .py(px(7.))
                    .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                    .child(icon(if f.downloaded { IconName::CircleCheck } else { IconName::CloudDownload }).small().text_color(if f.downloaded { theme.green } else { theme.yellow }))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(f.path.clone()))
                    .child(div().text_xs().font_family(theme.mono_font_family.clone()).text_color(theme.muted_foreground).child(f.oid.chars().take(10).collect::<String>()))
                    .when(!locked && !self.remotes.is_empty(), |d| {
                        d.child(Button::new(("lfs-lock", i)).small().ghost().icon(icon(IconName::Lock)).tooltip(tr("lfs.lock")).disabled(busy).on_click(cx.listener(move |this, _, window, cx| {
                            let p = path.clone();
                            this.lfs_op(tr("op.lfs_lock"), move |git| lfs::lock(git, &p), window, cx);
                        })))
                    }),
            );
        }
        col.child(files).into_any_element()
    }
}
