use cairn_core::ops::{config_get, config_set_local};
use cairn_core::settings::IdentityProfile;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, card, card_header, framed, hstack, icon, icon_tile, spacer, tr};

pub(crate) struct RepoSettingsPanel {
    repo: Entity<RepoView>,
    name: Entity<InputState>,
    email: Entity<InputState>,
    signing_key: Entity<InputState>,
    sign: bool,
    ssh_format: bool,
    remotes: Vec<(String, String)>,
    new_remote_name: Entity<InputState>,
    new_remote_url: Entity<InputState>,
    profile_name: Entity<InputState>,
}

fn local(git: &cairn_core::Git, key: &str) -> String {
    git.run(["config", "--local", "--get", key]).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub(crate) fn apply_profile(git: &cairn_core::Git, p: &IdentityProfile) -> cairn_core::Result<()> {
    if !p.user_name.is_empty() {
        config_set_local(git, "user.name", &p.user_name)?;
    }
    if !p.email.is_empty() {
        config_set_local(git, "user.email", &p.email)?;
    }
    if !p.signing_key.is_empty() {
        config_set_local(git, "user.signingkey", &p.signing_key)?;
    }
    Ok(())
}

pub(crate) fn auto_apply(git: &cairn_core::Git, profiles: &[IdentityProfile]) {
    if !local(git, "user.email").is_empty() {
        return;
    }
    if let Some(p) = profiles.iter().find(|p| p.matches(git.dir())) {
        let _ = apply_profile(git, p);
    }
}

impl RepoSettingsPanel {
    fn new(repo: Entity<RepoView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let git = repo.read(cx).git();
        let global_name = config_get(&git, "user.name").unwrap_or_default();
        let global_email = config_get(&git, "user.email").unwrap_or_default();
        let mk = |v: String, ph: String, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).default_value(v).placeholder(ph));
        let name = mk(local(&git, "user.name"), global_name, window, cx);
        let email = mk(local(&git, "user.email"), global_email, window, cx);
        let signing_key = mk(local(&git, "user.signingkey"), config_get(&git, "user.signingkey").unwrap_or_else(|| t!("repo_settings.key_placeholder").to_string()), window, cx);
        let sign = config_get(&git, "commit.gpgsign").is_some_and(|v| v == "true");
        let ssh_format = config_get(&git, "gpg.format").is_some_and(|v| v == "ssh");
        let remotes = cairn_core::refs::list_remotes(&git).unwrap_or_default().into_iter().map(|r| (r.name, r.url)).collect();
        let new_remote_name = mk(String::new(), "origin".into(), window, cx);
        let new_remote_url = mk(String::new(), "https://github.com/owner/repo.git".into(), window, cx);
        let profile_name = mk(String::new(), t!("repo_settings.profile_name_placeholder").to_string(), window, cx);
        Self { repo, name, email, signing_key, sign, ssh_format, remotes, new_remote_name, new_remote_url, profile_name }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let git = self.repo.read(cx).git();
        let pairs = [
            ("user.name", self.name.read(cx).value().trim().to_string()),
            ("user.email", self.email.read(cx).value().trim().to_string()),
            ("user.signingkey", self.signing_key.read(cx).value().trim().to_string()),
        ];
        let mut result = Ok(());
        for (k, v) in pairs {
            if v.is_empty() {
                let _ = git.run(["config", "--local", "--unset", k]);
            } else if let Err(e) = config_set_local(&git, k, &v) {
                result = Err(e);
            }
        }
        let _ = config_set_local(&git, "commit.gpgsign", if self.sign { "true" } else { "false" });
        if self.ssh_format {
            let _ = config_set_local(&git, "gpg.format", "ssh");
        } else {
            let _ = git.run(["config", "--local", "--unset", "gpg.format"]);
        }
        match result {
            Ok(()) => window.push_notification(Notification::success(tr("repo_settings.saved")), cx),
            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
        }
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        let name = self.profile_name.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let root = self.repo.read(cx).repo.root.clone();
        let folder = root.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let profile = IdentityProfile {
            name,
            user_name: self.name.read(cx).value().trim().to_string(),
            email: self.email.read(cx).value().trim().to_string(),
            signing_key: self.signing_key.read(cx).value().trim().to_string(),
            folder,
        };
        let mut settings = cairn_core::settings::Settings::load();
        settings.profiles.retain(|p| p.name != profile.name);
        settings.profiles.push(profile);
        let _ = settings.save();
        self.repo.update(cx, |v, cx| {
            v.settings.profiles = settings.profiles.clone();
            cx.notify();
        });
        cx.notify();
    }
}

impl Render for RepoSettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let profiles = self.repo.read(cx).settings.profiles.clone();
        let border = theme.border;
        let row = move |label: SharedString, first: bool| {
            hstack()
                .gap_4()
                .px(px(14.))
                .py(px(10.))
                .when(!first, |d| d.border_t_1().border_color(border))
                .child(div().w(px(150.)).flex_none().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        };
        let toggle_row = move |label: SharedString, first: bool| {
            hstack()
                .gap_4()
                .px(px(14.))
                .py(px(10.))
                .when(!first, |d| d.border_t_1().border_color(border))
                .child(div().flex_1().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        };
        let field = |label: &str, input: &Entity<InputState>, first: bool| row(tr(label), first).child(div().flex_1().child(Input::new(input)));
        let mut profile_row = h_flex().flex_wrap().gap_1p5();
        for p in &profiles {
            let pc = p.clone();
            let repo = self.repo.clone();
            profile_row = profile_row.child(framed(Button::new(SharedString::from(format!("prof-{}", p.name))), Tone::Plain, cx).h(px(28.)).icon(icon(IconName::User)).label(p.name.clone()).tooltip(format!("{} <{}>", p.user_name, p.email)).on_click(cx.listener(move |this, _, window, cx| {
                let git = repo.read(cx).git();
                match apply_profile(&git, &pc) {
                    Ok(()) => {
                        this.name.update(cx, |s, cx| s.set_value(pc.user_name.clone(), window, cx));
                        this.email.update(cx, |s, cx| s.set_value(pc.email.clone(), window, cx));
                        this.signing_key.update(cx, |s, cx| s.set_value(pc.signing_key.clone(), window, cx));
                        window.push_notification(Notification::success(t!("repo_settings.profile_applied", name = pc.name.clone()).to_string()), cx);
                    }
                    Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                }
            })));
        }
        let remote_count = self.remotes.len();
        let mut remotes = card(cx).child(card_header(IconName::Cloud, tr("repo_settings.remotes"), Some(remote_count), cx));
        for (name, url) in self.remotes.clone() {
            let n = name.clone();
            remotes = remotes.child(
                hstack()
                    .gap_3()
                    .px(px(14.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(icon_tile(IconName::Cloud, theme.cyan))
                    .child(v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(name.clone())).child(div().truncate().text_xs().text_color(theme.muted_foreground).font_family(theme.mono_font_family.clone()).child(url)))
                    .child(Button::new(SharedString::from(format!("rm-remote-{name}"))).small().ghost().icon(icon(IconName::Trash)).tooltip(tr("repo_settings.remove_remote")).on_click(cx.listener(move |this, _, window, cx| {
                        let git = this.repo.read(cx).git();
                        match git.run(["remote", "remove", &n]) {
                            Ok(_) => this.remotes.retain(|(x, _)| x != &n),
                            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                        }
                        cx.notify();
                    }))),
            );
        }
        let identity = card(cx)
            .child(card_header(IconName::User, tr("repo_settings.identity"), None, cx))
            .child(div().px(px(14.)).pt(px(10.)).text_xs().text_color(theme.muted_foreground).child(tr("repo_settings.identity_hint")))
            .child(field("repo_settings.name", &self.name, true))
            .child(field("repo_settings.email", &self.email, false))
            .when(!profiles.is_empty(), |d| d.child(row(tr("repo_settings.profiles"), false).child(profile_row)))
            .child(
                row(tr("repo_settings.save_profile"), false)
                    .child(div().flex_1().child(Input::new(&self.profile_name)))
                    .child(framed(Button::new("save-profile"), Tone::Plain, cx).icon(icon(IconName::Save)).label(tr("repo_settings.save_profile_button")).on_click(cx.listener(|this, _, _, cx| this.save_profile(cx)))),
            )
            .child(div().px(px(14.)).pb(px(12.)).text_xs().text_color(theme.muted_foreground).child(tr("repo_settings.profile_hint")));
        let signing = card(cx)
            .child(card_header(IconName::ShieldCheck, tr("repo_settings.signing"), None, cx))
            .child(toggle_row(tr("repo_settings.sign"), true).child(Switch::new("rs-sign").checked(self.sign).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.sign = *v;
                cx.notify();
            }))))
            .child(toggle_row(tr("repo_settings.ssh_format"), false).child(Switch::new("rs-ssh").checked(self.ssh_format).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.ssh_format = *v;
                cx.notify();
            }))))
            .child(field("repo_settings.key", &self.signing_key, false));
        v_flex()
            .w_full()
            .gap_3()
            .child(div().id("rs-scroll").max_h(px(640.)).overflow_y_scroll().child(v_flex().gap_3().child(identity).child(signing).child(remotes.child(
                hstack()
                    .gap_2()
                    .p(px(12.))
                    .child(div().w(px(120.)).child(Input::new(&self.new_remote_name)))
                    .child(div().flex_1().child(Input::new(&self.new_remote_url)))
                    .child(framed(Button::new("add-remote"), Tone::Plain, cx).icon(icon(IconName::Plus)).label(tr("repo_settings.add_remote")).on_click(cx.listener(|this, _, window, cx| {
                        let name = this.new_remote_name.read(cx).value().trim().to_string();
                        let url = this.new_remote_url.read(cx).value().trim().to_string();
                        if name.is_empty() || url.is_empty() {
                            return;
                        }
                        let git = this.repo.read(cx).git();
                        match git.run(["remote", "add", &name, &url]) {
                            Ok(_) => {
                                this.remotes.push((name, url));
                                this.new_remote_name.update(cx, |s, cx| s.set_value("", window, cx));
                                this.new_remote_url.update(cx, |s, cx| s.set_value("", window, cx));
                            }
                            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                        }
                        cx.notify();
                    }))),
            ))))
            .child(
                hstack()
                    .gap_2()
                    .pt_1()
                    .child(spacer())
                    .child(framed(Button::new("rs-close"), Tone::Plain, cx).label(tr("dialog.close")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(framed(Button::new("rs-save"), Tone::Accent, cx).icon(icon(IconName::Check)).label(tr("dialog.save")).on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))),
            )
    }
}

/// Opens the repository settings once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
pub(crate) fn open(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| open_now(repo, window, cx));
}

fn open_now(repo: Entity<RepoView>, window: &mut Window, cx: &mut App) {
    let name = repo.read(cx).name.clone();
    let panel = cx.new(|cx| RepoSettingsPanel::new(repo, window, cx));
    window.open_dialog(cx, move |dialog, _, _| dialog.title(t!("repo_settings.title", name = name.to_string()).to_string()).w(px(680.)).close_button(true).child(panel.clone()));
}
