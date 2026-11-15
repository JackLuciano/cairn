use std::collections::{BTreeSet, HashMap};

use cairn_core::backport::{self, Outcome, Source};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::RepoView;
use crate::widgets::{Tone, badge, framed_if, hstack, icon, spacer, tr};

#[derive(Clone)]
enum Status {
    Running,
    Done(Outcome),
}

/// "Backport": copies a pull request onto release branches, one new pull request per branch.
pub(crate) struct BackportPanel {
    repo: Entity<RepoView>,
    number: u64,
    title: String,
    src: Option<Result<Source, String>>,
    targets: Vec<(String, bool)>,
    picked: BTreeSet<String>,
    status: HashMap<String, Status>,
    running: bool,
}

impl BackportPanel {
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(src)) = self.src.clone() else { return };
        let targets: Vec<String> = self.picked.iter().cloned().collect();
        if targets.is_empty() {
            return;
        }
        let (git, remote, dir, mock) = {
            let v = self.repo.read(cx);
            (v.git(), v.default_remote().unwrap_or_else(|| "origin".into()), v.worktree_base().join("backports"), v.pulls.mock)
        };
        self.running = true;
        for t in &targets {
            self.status.insert(t.clone(), Status::Running);
        }
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            for (i, target) in targets.into_iter().enumerate() {
                let outcome = if mock {
                    cx.background_executor().timer(std::time::Duration::from_millis(700)).await;
                    if i == 1 { Outcome::Conflict { worktree: dir.join("demo") } } else { Outcome::Opened { number: 60 + i as u64, url: format!("https://github.com/aurora/aurora/pull/{}", 60 + i) } }
                } else {
                    let (git, remote, src, dir, t) = (git.clone(), remote.clone(), src.clone(), dir.clone(), target.clone());
                    cx.background_executor().spawn(async move { backport::run(&git, &remote, &src, &t, &dir) }).await
                };
                let open = if let Outcome::Conflict { worktree } = &outcome { Some(worktree.clone()) } else { None };
                if this
                    .update(cx, |p, cx| {
                        p.status.insert(target.clone(), Status::Done(outcome));
                        if let Some(path) = open.filter(|_| !mock) {
                            p.repo.update(cx, |_, cx| cx.emit(crate::repo_view::OpenRepoEvent(path)));
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = this.update(cx, |p, cx| {
                p.running = false;
                p.repo.update(cx, |v, cx| {
                    v.pulls.loaded = false;
                    v.load_pulls(cx);
                });
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for BackportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut list = v_flex().gap_1();
        match &self.src {
            None => list = list.child(crate::skeleton::list(3, cx)),
            Some(Err(e)) => list = list.child(div().p_3().rounded(px(8.)).bg(theme.red.opacity(0.1)).text_sm().text_color(theme.red).child(e.clone())),
            Some(Ok(_)) if self.targets.is_empty() => list = list.child(div().p_3().text_sm().text_color(theme.muted_foreground).child(tr("backport.no_targets"))),
            Some(Ok(_)) => {}
        }
        for (i, (name, release)) in self.targets.clone().into_iter().enumerate() {
            let on = self.picked.contains(&name);
            let status = self.status.get(&name).cloned();
            let n = name.clone();
            let right: AnyElement = match status {
                None => div().into_any_element(),
                Some(Status::Running) => hstack().gap_1().text_xs().text_color(theme.muted_foreground).child(icon(IconName::LoaderCircle).xsmall()).child(tr("backport.working")).into_any_element(),
                Some(Status::Done(Outcome::Opened { number, url })) => Button::new(("bp-open", i)).xsmall().ghost().icon(icon(IconName::ExternalLink)).label(t!("backport.opened", n = number).to_string()).on_click(move |_, _, cx| cx.open_url(&url)).into_any_element(),
                Some(Status::Done(Outcome::Conflict { .. })) => badge(tr("backport.conflict"), theme.yellow).into_any_element(),
                Some(Status::Done(Outcome::AlreadyThere)) => badge(tr("backport.already"), theme.green).into_any_element(),
                Some(Status::Done(Outcome::Failed(e))) => div().max_w(px(260.)).truncate().text_xs().text_color(theme.red).child(e).into_any_element(),
            };
            list = list.child(
                hstack()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(6.))
                    .hover(|s| s.bg(theme.list_hover))
                    .child(Checkbox::new(("bp-pick", i)).checked(on).disabled(self.running).on_click(cx.listener(move |p, v: &bool, _, cx| {
                        if *v {
                            p.picked.insert(n.clone());
                        } else {
                            p.picked.remove(&n);
                        }
                        cx.notify();
                    })))
                    .child(icon(IconName::GitBranch).xsmall().text_color(theme.muted_foreground))
                    .child(div().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(name.clone()))
                    .when(release, |d| d.child(badge(tr("backport.release"), theme.blue)))
                    .child(spacer())
                    .child(right),
            );
        }
        let conflict = self.status.values().any(|s| matches!(s, Status::Done(Outcome::Conflict { .. })));
        let finished = !self.status.is_empty() && !self.running;
        let count = self.picked.len();
        v_flex()
            .gap_3()
            .child(div().text_sm().text_color(theme.muted_foreground).whitespace_normal().child(t!("backport.body", n = self.number, title = self.title.clone()).to_string()))
            .child(div().id("bp-list").max_h(px(340.)).overflow_y_scroll().rounded(px(8.)).border_1().border_color(theme.border).p_1().child(list))
            .when(conflict, |d| d.child(hstack().w_full().items_start().gap_2().p_2p5().rounded(px(8.)).bg(theme.yellow.opacity(0.1)).text_sm().text_color(theme.yellow).child(div().flex_none().pt(px(2.)).child(icon(IconName::TriangleAlert).small())).child(div().flex_1().min_w_0().whitespace_normal().child(tr("backport.conflict_body")))))
            .child(hstack().w_full().items_start().gap_2().text_xs().text_color(theme.muted_foreground).child(div().flex_1().min_w_0().whitespace_normal().child(tr("backport.safe"))))
            .child(
                hstack()
                    .gap_2()
                    .child(spacer())
                    .child(Button::new("bp-close").ghost().label(if finished { tr("dialog.close") } else { tr("dialog.cancel") }).on_click(|_, window, cx| window.close_dialog(cx)))
                    .when(!finished, |d| {
                        d.child(framed_if(Button::new("bp-go"), Tone::Accent, count > 0 && !self.running && matches!(self.src, Some(Ok(_))), cx).icon(icon(IconName::GitPullRequestCreate)).label(crate::widgets::plural("backport.go", count)).loading(self.running).on_click(cx.listener(|p, _, window, cx| p.start(window, cx))))
                    }),
            )
    }
}

/// Opens the backport dialog for a pull request once the caller's update of `repo` has finished.
///
/// # Arguments
/// * `repo` - the repository view, possibly mid-update
/// * `number` - the pull request
/// * `title` - its title
/// * `skip` - its head and base branches, left out of the list
pub(crate) fn open(repo: Entity<RepoView>, number: u64, title: String, skip: Vec<String>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let (git, remote, mock) = {
            let v = repo.read(cx);
            (v.git(), v.default_remote().unwrap_or_else(|| "origin".into()), v.pulls.mock)
        };
        let panel = cx.new(|cx| {
            cx.spawn(async move |this, cx| {
                let loaded = if mock {
                    let targets = ["release/2.4", "release/2.3", "release/2.2", "develop"].iter().map(|b| (b.to_string(), b.starts_with("release"))).collect();
                    (Ok(Source { number, title: String::new(), commits: vec!["0".into()] }), targets)
                } else {
                    cx.background_executor()
                        .spawn(async move {
                            let skip: Vec<&str> = skip.iter().map(String::as_str).collect();
                            (backport::source(&git, number).map_err(|e| crate::repo_view::friendly_error(&e)), backport::targets(&git, &remote, &skip))
                        })
                        .await
                };
                let _ = this.update(cx, |p: &mut BackportPanel, cx| {
                    p.src = Some(loaded.0);
                    p.targets = loaded.1;
                    if mock {
                        p.picked = p.targets.iter().filter(|(_, r)| *r).take(2).map(|(b, _)| b.clone()).collect();
                    }
                    cx.notify();
                });
            })
            .detach();
            BackportPanel { repo, number, title, src: None, targets: Vec::new(), picked: BTreeSet::new(), status: HashMap::new(), running: false }
        });
        window.open_dialog(cx, move |dialog, _, cx| dialog.title(tr("backport.title")).w(px(620.)).bg(cx.theme().popover.opacity(1.)).close_button(true).child(panel.clone()));
    });
}
