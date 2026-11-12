use cairn_core::diff::FileDiff;
use cairn_core::ops::CommitOptions;
use cairn_core::patch::{self, LineSelection};
use cairn_core::status::FileStatus;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::diff_view::DiffSource;
use crate::repo_view::{RepoView, SelectedChange};
use crate::widgets::{Tone, empty_state, framed, hstack, icon, section_label, spacer, status_letter, tr};

impl RepoView {
    pub(crate) fn apply_selection(&mut self, diff: FileDiff, sel: LineSelection, staged: bool, discard: bool, window: &mut Window, cx: &mut Context<Self>) {
        if sel.is_empty() {
            return;
        }
        let label = if discard {
            tr("op.discard")
        } else if staged {
            tr("op.unstage")
        } else {
            tr("op.stage")
        };
        self.line_selection.clear();
        self.run_op_quiet(label, move |git| {
            if discard {
                patch::discard_lines(git, &diff, &sel)
            } else if staged {
                patch::unstage_lines(git, &diff, &sel)
            } else {
                patch::stage_lines(git, &diff, &sel)
            }
            .map(|_| String::new())
        }, window, cx);
    }

    pub(crate) fn run_op_quiet<F>(&mut self, label: SharedString, op: F, window: &mut Window, cx: &mut Context<Self>)
    where
        F: FnOnce(&cairn_core::Git) -> cairn_core::Result<String> + Send + 'static,
    {
        let git = self.git();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { op(&git) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Err(e) = result {
                    window.push_notification(
                        gpui_kit::component::notification::Notification::error(crate::repo_view::friendly_error(&e))
                            .title(t!("op.failed", what = label.to_string()).to_string()),
                        cx,
                    );
                }
                this.reload(cx);
            });
        });
        task.detach();
    }

    pub(crate) fn confirm_discard_hunk(&mut self, h: usize, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            alert
                .confirm()
                .title(tr("changes.discard_hunk_title"))
                .description(tr("changes.discard_body"))
                .ok_text(tr("changes.discard"))
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        if let Some(diff) = this.change_diff.clone() {
                            let sel = patch::select_hunks(&diff, &[h]);
                            this.apply_selection(diff, sel, false, true, window, cx);
                        }
                    });
                    true
                })
        });
    }

    fn confirm_discard_files(&mut self, files: Vec<FileStatus>, window: &mut Window, cx: &mut Context<Self>) {
        if files.is_empty() {
            return;
        }
        let view = cx.entity();
        let count = files.len();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let files = files.clone();
            alert
                .confirm()
                .title(crate::widgets::plural("changes.discard_files_title", count))
                .description(tr("changes.discard_body"))
                .ok_text(tr("changes.discard"))
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let tracked: Vec<String> = files.iter().filter(|f| !f.is_untracked()).map(|f| f.path.clone()).collect();
                    let untracked: Vec<String> = files.iter().filter(|f| f.is_untracked()).map(|f| f.path.clone()).collect();
                    view.update(cx, |this, cx| {
                        this.run_op(
                            tr("op.discard"),
                            Some(t!("undo.discard").to_string()),
                            move |git| {
                                cairn_core::ops::discard_files(git, &tracked, &untracked).map(|_| String::new())
                            },
                            window,
                            cx,
                        );
                    });
                    true
                })
        });
    }

    fn stage_paths(&mut self, paths: Vec<String>, staged: bool, window: &mut Window, cx: &mut Context<Self>) {
        let label = if staged { tr("op.unstage") } else { tr("op.stage") };
        self.run_op_quiet(label, move |git| {
            if staged {
                cairn_core::ops::unstage_files(git, &paths)
            } else {
                cairn_core::ops::stage_files(git, &paths)
            }
            .map(|_| String::new())
        }, window, cx);
    }

    pub(crate) fn commit(&mut self, push_after: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.summary.read(cx).value().trim().is_empty() {
            window.push_notification(gpui_kit::component::notification::Notification::warning(tr("changes.need_summary")), cx);
            return;
        }
        let amends_pushed = self.amend && self.status.branch.upstream.is_some() && self.status.branch.ahead == 0;
        self.guarded(
            cairn_core::checks::Stage::Commit,
            move |v, window, cx| {
                if amends_pushed {
                    v.protected(move |v, window, cx| v.commit_now(push_after, window, cx), window, cx);
                } else {
                    v.commit_now(push_after, window, cx);
                }
            },
            window,
            cx,
        );
    }

    pub(crate) fn suggest_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.ai.enabled {
            window.push_notification(gpui_kit::component::notification::Notification::info(tr("changes.ai_off_body")).title(tr("changes.ai_off_title")), cx);
            crate::settings_dialog::request_category("ai");
            window.dispatch_action(Box::new(crate::palette::OpenSettings), cx);
            return;
        }
        if self.status.staged().count() == 0 {
            window.push_notification(gpui_kit::component::notification::Notification::warning(tr("changes.nothing_staged")), cx);
            return;
        }
        let git = self.git();
        let settings = self.settings.clone();
        let repo_rules = std::fs::read_to_string(self.repo.root.join(".cairn").join("commit-message.md")).ok();
        self.busy = Some(tr("changes.writing_message"));
        cx.notify();
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let diff = git.run(["diff", "--cached", "--no-ext-diff"]).map_err(|e| e.to_string())?;
                    if diff.trim().is_empty() {
                        return Err(t!("changes.nothing_staged").to_string());
                    }
                    crate::ai::commit_message(&settings, &diff, repo_rules.as_deref())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = None;
                match result {
                    Ok((s, b)) => {
                        this.summary.update(cx, |st, cx| st.set_value(s, window, cx));
                        this.body.update(cx, |st, cx| st.set_value(b, window, cx));
                    }
                    Err(e) => window.push_notification(gpui_kit::component::notification::Notification::error(e).title(tr("ai.failed")), cx),
                }
                cx.notify();
            });
        });
        task.detach();
    }

    fn commit_now(&mut self, push_after: bool, window: &mut Window, cx: &mut Context<Self>) {
        let summary = self.summary.read(cx).value().trim().to_string();
        let body = self.body.read(cx).value().trim().to_string();
        if summary.is_empty() {
            window.push_notification(gpui_kit::component::notification::Notification::warning(tr("changes.need_summary")), cx);
            return;
        }
        if self.status.staged().count() == 0 && !self.amend {
            window.push_notification(gpui_kit::component::notification::Notification::warning(tr("changes.nothing_staged")), cx);
            return;
        }
        let message = if body.is_empty() { summary.clone() } else { format!("{summary}\n\n{body}") };
        let opts = CommitOptions {
            amend: self.amend,
            sign: self.settings.commits.sign,
            no_verify: false,
            allow_empty: false,
        };
        let label = if self.amend { tr("op.amend") } else { tr("op.commit") };
        let undo = if self.amend { t!("undo.amend").to_string() } else { t!("undo.commit", summary = summary.clone()).to_string() };
        self.summary.update(cx, |s, cx| s.set_value("", window, cx));
        self.body.update(cx, |s, cx| s.set_value("", window, cx));
        self.amend = false;
        self.run_op(label, Some(undo), move |git| cairn_core::ops::commit(git, &message, &opts), window, cx);
        if push_after {
            let view = cx.entity();
            let task = cx.spawn_in(window, async move |_, cx| {
                loop {
                    cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
                    let busy = view.read_with(cx, |v, _| v.busy.is_some());
                    if !busy {
                        break;
                    }
                }
                let _ = view.update_in(cx, |v, window, cx| v.push(false, window, cx));
            });
            task.detach();
        }
    }

    fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = !self.amend;
        if self.amend && self.summary.read(cx).value().is_empty() {
            let msg = cairn_core::ops::head_message(&self.repo.git);
            let (s, b) = msg.split_once("\n\n").map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((msg.clone(), String::new()));
            self.summary.update(cx, |st, cx| st.set_value(s, window, cx));
            self.body.update(cx, |st, cx| st.set_value(b, window, cx));
        }
        cx.notify();
    }

    fn render_file_row(&mut self, f: &FileStatus, staged: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let kind = if staged { f.staged } else { f.unstaged };
        let letter = kind.map(|k| k.letter()).unwrap_or('M');
        let selected = self.change.as_ref().is_some_and(|c| c.path == f.path && c.staged == staged);
        let (dir, name) = match f.path.rsplit_once('/') {
            Some((d, n)) => (format!("{d}/"), n.to_string()),
            None => (String::new(), f.path.clone()),
        };
        let change = SelectedChange { path: f.path.clone(), staged, untracked: f.is_untracked() };
        let path = f.path.clone();
        let path_menu = f.path.clone();
        let file = f.clone();
        let root = self.repo.root.clone();
        let view = cx.entity();
        div()
            .id(SharedString::from(format!("{}-{}", if staged { "s" } else { "u" }, f.path)))
            .group("file-row")
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(6.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.list_active))
            .when(!selected, |d| d.hover(|s| s.bg(cx.theme().list_hover)))
            .on_click(cx.listener(move |this, _, _, cx| this.select_change(change.clone(), cx)))
            .child(status_letter(letter, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(div().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                    .when(!dir.is_empty(), |d| d.child(div().truncate().text_size(px(11.)).text_color(theme.muted_foreground).child(dir.trim_end_matches('/').to_string()))),
            )
            .child(
                div()
                    .invisible()
                    .group_hover("file-row", |s| s.visible())
                    .child(
                        Button::new(SharedString::from(format!("toggle-{}-{}", staged, f.path)))
                            .xsmall()
                            .ghost()
                            .icon(icon(if staged { IconName::Minus } else { IconName::Plus }))
                            .tooltip(if staged { tr("changes.unstage_file") } else { tr("changes.stage_file") })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.stage_paths(vec![path.clone()], staged, window, cx);
                            })),
                    ),
            )
            .context_menu(move |menu, _, _| {
                let v1 = view.clone();
                let v2 = view.clone();
                let p1 = path_menu.clone();
                let file = file.clone();
                let root = root.clone();
                let full = root.join(&path_menu);
                menu.item(
                    PopupMenuItem::new(if staged { t!("changes.unstage_file").to_string() } else { t!("changes.stage_file").to_string() })
                        .on_click(move |_, window, cx| {
                            let p = p1.clone();
                            v1.update(cx, |this, cx| this.stage_paths(vec![p], staged, window, cx));
                        }),
                )
                .when(!staged, |m| {
                    m.item(
                        PopupMenuItem::new(t!("changes.discard_file").to_string())
                            .icon(icon(IconName::Trash))
                            .on_click(move |_, window, cx| {
                                let f = file.clone();
                                v2.update(cx, |this, cx| this.confirm_discard_files(vec![f], window, cx));
                            }),
                    )
                })
                .separator()
                .item({
                    let v3 = view.clone();
                    let p3 = path_menu.clone();
                    PopupMenuItem::new(t!("changes.line_history").to_string()).icon(icon(IconName::FileClock)).on_click(move |_, _, cx| {
                        let p = p3.clone();
                        v3.update(cx, |this, cx| this.open_blame(p, cx));
                    })
                })
                .item(
                    PopupMenuItem::new(t!("changes.reveal").to_string())
                        .icon(icon(IconName::FolderOpen))
                        .on_click(move |_, _, cx| cx.reveal_path(&full)),
                )
            })
            .into_any_element()
    }

    fn render_file_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let unstaged: Vec<FileStatus> = self.status.unstaged().cloned().collect();
        let staged: Vec<FileStatus> = self.status.staged().cloned().collect();
        let conflicted: Vec<FileStatus> = self.status.conflicted().cloned().collect();
        let all_unstaged_paths: Vec<String> = unstaged.iter().map(|f| f.path.clone()).collect();
        let all_staged_paths: Vec<String> = staged.iter().map(|f| f.path.clone()).collect();
        let discard_all = unstaged.clone();

        v_flex()
            .id("file-list")
            .size_full()
            .overflow_y_scroll()
            .p_1()
            .when(!conflicted.is_empty(), |d| {
                d.child(section_label(t!("changes.conflicted", n = conflicted.len()).to_string(), cx))
                    .children(conflicted.iter().map(|f| {
                        hstack()
                            .gap_2()
                            .px_2()
                            .h(px(28.))
                            .child(status_letter('!', cx))
                            .child(div().text_sm().child(f.path.clone()))
                            .into_any_element()
                    }))
            })
            .child(
                hstack()
                    .pr_1()
                    .child(section_label(t!("changes.unstaged", n = unstaged.len()).to_string(), cx))
                    .child(spacer())
                    .when(!unstaged.is_empty(), |d| {
                        d.child(
                            Button::new("discard-all")
                                .xsmall()
                                .ghost()
                                .icon(icon(IconName::Trash))
                                .tooltip(tr("changes.discard_all"))
                                .on_click(cx.listener(move |this, _, window, cx| this.confirm_discard_files(discard_all.clone(), window, cx))),
                        )
                        .child(
                            crate::widgets::framed(Button::new("stage-all"), crate::widgets::Tone::Plain, cx)
                                .h(px(26.))
                                .px_2()
                                .text_xs()
                                .icon(icon(IconName::Plus))
                                .label(tr("changes.stage_all"))
                                .on_click(cx.listener(move |this, _, window, cx| this.stage_paths(all_unstaged_paths.clone(), false, window, cx))),
                        )
                    }),
            )
            .when(unstaged.is_empty(), |d| {
                d.child(div().px_2().py_1().text_xs().text_color(theme.muted_foreground).child(tr("changes.no_unstaged")))
            })
            .children(unstaged.iter().map(|f| self.render_file_row(f, false, cx)).collect::<Vec<_>>())
            .child(
                hstack()
                    .pr_1()
                    .mt_2()
                    .child(section_label(t!("changes.staged", n = staged.len()).to_string(), cx))
                    .child(spacer())
                    .when(!staged.is_empty(), |d| {
                        d.child(
                            crate::widgets::framed(Button::new("unstage-all"), crate::widgets::Tone::Plain, cx)
                                .h(px(26.))
                                .px_2()
                                .text_xs()
                                .icon(icon(IconName::Minus))
                                .label(tr("changes.unstage_all"))
                                .on_click(cx.listener(move |this, _, window, cx| this.stage_paths(all_staged_paths.clone(), true, window, cx))),
                        )
                    }),
            )
            .when(staged.is_empty(), |d| {
                d.child(div().px_2().py_1().text_xs().text_color(cx.theme().muted_foreground).child(tr("changes.no_staged")))
            })
            .children(staged.iter().map(|f| self.render_file_row(f, true, cx)).collect::<Vec<_>>())
    }

    fn render_commit_box(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let staged = self.status.staged().count();
        let summary_len = self.summary.read(cx).value().chars().count();
        let limit = self.settings.commits.summary_limit;
        let too_long = self.settings.commits.warn_long_summary && summary_len > limit;
        let can_commit = (staged > 0 || self.amend) && summary_len > 0 && self.busy.is_none();
        let amend = self.amend;
        let sign = self.settings.commits.sign;
        let field_label = |text: SharedString, cx: &App| div().text_xs().text_color(cx.theme().muted_foreground).child(text);
        v_flex()
            .size_full()
            .gap_3()
            .p_4()
            .bg(theme.sidebar)
            .child(
                hstack()
                    .gap_2()
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(tr("changes.commit_title")))
                    .child(spacer())
                    .child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("changes.staged_files", staged))),
            )
            .when(self.settings.commits.ai_messages != cairn_core::settings::AiSuggest::Never, |d| {
                d.child(
                    framed(Button::new("ai-message"), Tone::Ai, cx)
                        .w_full()
                        .h(px(34.))
                        .icon(icon(IconName::Sparkles))
                        .label(tr("changes.write_message"))
                        .tooltip(tr("changes.write_message_tip"))
                        .disabled(self.busy.is_some())
                        .on_click(cx.listener(|this, _, window, cx| this.suggest_message(window, cx))),
                )
            })
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        hstack()
                            .child(field_label(tr("changes.summary_label"), cx))
                            .child(spacer())
                            .child(div().text_xs().text_color(if too_long { theme.red } else { theme.muted_foreground }).child(format!("{summary_len}/{limit}"))),
                    )
                    .child(Input::new(&self.summary)),
            )
            .child(v_flex().gap_1p5().child(field_label(tr("changes.description_label"), cx)).child(Textarea::new(&self.body)))
            .child(
                v_flex()
                    .gap_2()
                    .p(px(10.))
                    .rounded(px(8.))
                    .bg(theme.secondary)
                    .child(
                        Switch::new("amend")
                            .small()
                            .checked(amend)
                            .label(tr("changes.amend"))
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_amend(window, cx))),
                    )
                    .child(
                        Switch::new("sign")
                            .small()
                            .checked(sign)
                            .label(tr("changes.sign_commit"))
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                this.settings.commits.sign = *on;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_1())
            .child(
                hstack()
                    .gap_2()
                    .child(
                        framed(Button::new("commit"), Tone::Accent, cx)
                            .flex_1()
                            .h(px(36.))
                            .icon(icon(IconName::Check))
                            .label(if amend {
                                tr("changes.amend_button")
                            } else {
                                crate::widgets::plural("changes.commit_button", staged).into()
                            })
                            .disabled(!can_commit)
                            .on_click(cx.listener(|this, _, window, cx| this.commit(false, window, cx))),
                    )
                    .child(
                        framed(Button::new("commit-push"), Tone::Plain, cx)
                            .h(px(36.))
                            .label(tr("changes.commit_push"))
                            .disabled(!can_commit)
                            .on_click(cx.listener(|this, _, window, cx| this.commit(true, window, cx))),
                    ),
            )
    }

    pub(crate) fn render_changes(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.status.is_clean() && !self.amend {
            return v_flex()
                .size_full()
                .child(div().flex_1().child(empty_state(IconName::CircleCheckBig, tr("changes.clean_title"), tr("changes.clean_body"), cx)))
                .into_any_element();
        }
        let diff_panel = match (&self.change, self.change_diff.clone()) {
            (Some(change), Some(diff)) => {
                let sel_count = self.line_selection.len();
                let staged = change.staged;
                let untracked = change.untracked;
                v_flex()
                    .size_full()
                    .child(
                        hstack()
                            .gap_2()
                            .px_3()
                            .h(px(36.))
                            .border_b_1()
                            .border_color(theme.border)
                            .child(div().min_w_0().truncate().text_sm().font_family(theme.mono_font_family.clone()).child(change.path.clone()))
                            .child(spacer())
                            .when(sel_count > 0 && !untracked, |d| {
                                d.child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("changes.lines_selected", sel_count)))
                                    .when(!staged, |d| {
                                        d.child(
                                            Button::new("discard-lines")
                                                .xsmall()
                                                .ghost()
                                                .label(tr("changes.discard_lines"))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    let Some(diff) = this.change_diff.clone() else { return };
                                                    let sel = this.line_selection.clone();
                                                    let view = cx.entity();
                                                    window.open_alert_dialog(cx, move |alert, _, _| {
                                                        let view = view.clone();
                                                        let diff = diff.clone();
                                                        let sel = sel.clone();
                                                        alert
                                                            .confirm()
                                                            .title(tr("changes.discard_lines_title"))
                                                            .description(tr("changes.discard_body"))
                                                            .ok_text(tr("changes.discard"))
                                                            .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                                                            .on_ok(move |_, window, cx| {
                                                                let (d, s) = (diff.clone(), sel.clone());
                                                                view.update(cx, |this, cx| this.apply_selection(d, s, false, true, window, cx));
                                                                true
                                                            })
                                                    });
                                                })),
                                        )
                                    })
                                    .child(
                                        Button::new("stage-lines")
                                            .xsmall()
                                            .primary()
                                            .label(if staged { tr("changes.unstage_lines") } else { tr("changes.stage_lines") })
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                let Some(diff) = this.change_diff.clone() else { return };
                                                let sel = this.line_selection.clone();
                                                this.apply_selection(diff, sel, staged, false, window, cx);
                                            })),
                                    )
                            })
                            .when(sel_count == 0 && !untracked, |d| {
                                d.child(div().text_xs().text_color(theme.muted_foreground).child(tr("changes.click_lines_hint")))
                            })
                            .child(
                                crate::widgets::toggle_pill("ignore-ws", tr("changes.ignore_whitespace"), self.settings.diffs.ignore_whitespace, cx).on_click(cx.listener(|this, _, _, cx| {
                                    this.settings.diffs.ignore_whitespace = !this.settings.diffs.ignore_whitespace;
                                    this.line_selection.clear();
                                    this.load_change_diff(cx);
                                })),
                            ),
                    )
                    .child(div().flex_1().min_h_0().child(self.render_file_diff(DiffSource::Change, &diff, cx)))
                    .into_any_element()
            }
            (Some(_), None) => div().size_full().flex().items_center().justify_center().child(gpui_kit::component::spinner::Spinner::new()).into_any_element(),
            _ => empty_state(IconName::FileDiff, tr("changes.pick_title"), tr("changes.pick_body"), cx).into_any_element(),
        };

        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_none()
                    .w(px(280.))
                    .h_full()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(div().flex_1().min_h_0().child(self.render_file_list(cx))),
            )
            .child(div().flex_1().min_w_0().h_full().child(diff_panel))
            .child(
                div()
                    .flex_none()
                    .w(px(330.))
                    .h_full()
                    .border_l_1()
                    .border_color(theme.border)
                    .child(self.render_commit_box(cx)),
            )
            .into_any_element()
    }
}

