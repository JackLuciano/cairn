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
use crate::widgets::{hstack, icon, section_label, spacer, tr};

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
        let field = |label: &str, input: &Entity<InputState>| hstack().gap_3().child(div().w(px(140.)).text_sm().child(tr(label))).child(div().flex_1().child(Input::new(input).small()));
        let mut profile_row = h_flex().flex_wrap().gap_1();
        for p in &profiles {
            let pc = p.clone();
            let repo = self.repo.clone();
            profile_row = profile_row.child(Button::new(SharedString::from(format!("prof-{}", p.name))).small().outline().label(p.name.clone()).tooltip(format!("{} <{}>", p.user_name, p.email)).on_click(cx.listener(move |this, _, window, cx| {
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
        let mut remotes = v_flex().gap_1();
        for (name, url) in self.remotes.clone() {
            let n = name.clone();
            remotes = remotes.child(
                hstack()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .bg(theme.sidebar)
                    .child(icon(IconName::Cloud).small().text_color(theme.muted_foreground))
                    .child(div().w(px(90.)).text_sm().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                    .child(div().flex_1().truncate().text_xs().font_family(theme.mono_font_family.clone()).child(url))
                    .child(Button::new(SharedString::from(format!("rm-remote-{name}"))).xsmall().ghost().icon(icon(IconName::Trash)).tooltip(tr("repo_settings.remove_remote")).on_click(cx.listener(move |this, _, window, cx| {
                        let git = this.repo.read(cx).git();
                        match git.run(["remote", "remove", &n]) {
                            Ok(_) => this.remotes.retain(|(x, _)| x != &n),
                            Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                        }
                        cx.notify();
                    }))),
            );
        }
        v_flex()
            .w_full()
            .gap_3()
            .child(section_label(tr("repo_settings.identity"), cx).px_0())
            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("repo_settings.identity_hint")))
            .child(field("repo_settings.name", &self.name))
            .child(field("repo_settings.email", &self.email))
            .when(!profiles.is_empty(), |d| d.child(hstack().gap_2().child(div().w(px(140.)).text_sm().child(tr("repo_settings.profiles"))).child(profile_row)))
            .child(hstack().gap_2().child(div().w(px(140.)).text_sm().child(tr("repo_settings.save_profile"))).child(div().flex_1().child(Input::new(&self.profile_name).small())).child(Button::new("save-profile").small().outline().label(tr("repo_settings.save_profile_button")).on_click(cx.listener(|this, _, _, cx| this.save_profile(cx)))))
            .child(div().text_xs().text_color(theme.muted_foreground).child(tr("repo_settings.profile_hint")))
            .child(section_label(tr("repo_settings.signing"), cx).px_0())
            .child(Switch::new("rs-sign").checked(self.sign).label(tr("repo_settings.sign")).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.sign = *v;
                cx.notify();
            })))
            .child(Switch::new("rs-ssh").checked(self.ssh_format).label(tr("repo_settings.ssh_format")).on_click(cx.listener(|this, v: &bool, _, cx| {
                this.ssh_format = *v;
                cx.notify();
            })))
            .child(field("repo_settings.key", &self.signing_key))
            .child(section_label(tr("repo_settings.remotes"), cx).px_0())
            .child(remotes)
            .child(
                hstack()
                    .gap_2()
                    .child(div().w(px(110.)).child(Input::new(&self.new_remote_name).small()))
                    .child(div().flex_1().child(Input::new(&self.new_remote_url).small()))
                    .child(Button::new("add-remote").small().outline().icon(icon(IconName::Plus)).label(tr("repo_settings.add_remote")).on_click(cx.listener(|this, _, window, cx| {
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
            )
            .child(
                hstack()
                    .pt_2()
                    .child(spacer())
                    .child(Button::new("rs-close").ghost().label(tr("dialog.close")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("rs-save").primary().label(tr("dialog.save")).on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))),
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
