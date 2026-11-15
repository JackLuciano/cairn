use std::hash::{Hash, Hasher};

use cairn_core::away::AwaySummary;
use cairn_core::overlap::Overlap;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::repo_view::{Page, RepoView};
use crate::widgets::{Tone, avatar, framed, framed_if, hstack, icon, relative_time, tr};

const AWAY_AFTER_SECS: i64 = 2 * 3600;

/// What Cairn knows about other people's work: what landed while the user was away, and who
/// else is changing the same files.
#[derive(Default)]
pub(crate) struct Awareness {
    pub away: Option<AwaySummary>,
    pub away_ai: Option<Result<String, String>>,
    pub away_ai_running: bool,
    pub away_tip: String,
    pub away_checked: bool,
    pub away_done: bool,
    pub overlaps: Vec<Overlap>,
    pub overlap_sig: u64,
}

fn names(authors: &[(String, usize)]) -> String {
    match authors.len() {
        0 => String::new(),
        1 => authors[0].0.clone(),
        2 => t!("away.two", a = authors[0].0.clone(), b = authors[1].0.clone()).to_string(),
        3 => t!("away.many_one", a = authors[0].0.clone(), b = authors[1].0.clone()).to_string(),
        n => t!("away.many", a = authors[0].0.clone(), b = authors[1].0.clone(), n = n - 2).to_string(),
    }
}

fn span(secs: i64) -> String {
    let hours = secs / 3600;
    if hours < 48 { crate::widgets::plural("away.hours", hours.max(1) as usize) } else { crate::widgets::plural("away.days", (hours / 24) as usize) }
}

impl RepoView {
    /// Runs after every reload: looks again for overlapping work when the user's files or the
    /// server's branches changed, and checks what landed while the user was away.
    pub(crate) fn after_loaded(&mut self, cx: &mut Context<Self>) {
        let remote_tips: Vec<String> = self.branches.iter().filter(|b| b.is_remote).map(|b| b.target.clone()).collect();
        let trunk_tip = self.trunk.as_ref().and_then(|t| self.branches.iter().find(|b| b.is_remote && b.name.split_once('/').map(|(_, n)| n) == Some(t.as_str()))).map(|b| b.target.clone()).unwrap_or_default();
        if self.settings.notifications.away && !self.aware.away_done && (!self.aware.away_checked || (!trunk_tip.is_empty() && trunk_tip != self.aware.away_tip)) {
            self.aware.away_checked = true;
            self.aware.away_tip = trunk_tip;
            self.load_away(cx);
        }
        if !self.settings.notifications.overlaps {
            self.aware.overlaps.clear();
            return;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.status.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().hash(&mut h);
        self.status.branch.head.hash(&mut h);
        remote_tips.hash(&mut h);
        let sig = h.finish();
        if sig == self.aware.overlap_sig {
            return;
        }
        self.aware.overlap_sig = sig;
        let git = self.git();
        self.spawn_bg(
            move || {
                let trunk = cairn_core::conflicts::default_trunk(&git)?;
                let mine = cairn_core::overlap::my_files(&git, &trunk);
                cairn_core::overlap::find(&git, &trunk, &mine).ok()
            },
            |this, found, cx| {
                this.aware.overlaps = found.unwrap_or_default();
                cx.notify();
            },
            cx,
        );
    }

    fn load_away(&mut self, cx: &mut Context<Self>) {
        let git = self.git();
        let root = self.repo.root.clone();
        let debug_hours = if cfg!(debug_assertions) { std::env::var("CAIRN_DEBUG_AWAY_HOURS").ok().and_then(|v| v.parse::<i64>().ok()) } else { None };
        self.spawn_bg(
            move || {
                let now = chrono::Utc::now().timestamp();
                let since = match debug_hours.map(|h| now - h * 3600).or_else(|| cairn_core::away::last_seen(&root)) {
                    Some(s) => s,
                    None => {
                        cairn_core::away::mark_seen(&root, now);
                        return None;
                    }
                };
                if now - since < AWAY_AFTER_SECS {
                    return None;
                }
                let trunk = cairn_core::conflicts::default_trunk(&git)?;
                let summary = cairn_core::away::summarize(&git, &trunk, since).ok()?;
                if summary.is_empty() {
                    cairn_core::away::mark_seen(&root, now);
                    return None;
                }
                Some(summary)
            },
            |this, summary, cx| {
                this.aware.away = summary;
                cx.notify();
            },
            cx,
        );
    }

    fn dismiss_away(&mut self, cx: &mut Context<Self>) {
        cairn_core::away::mark_seen(&self.repo.root, chrono::Utc::now().timestamp());
        self.aware.away = None;
        self.aware.away_ai = None;
        self.aware.away_done = true;
        cx.notify();
    }

    fn summarize_away(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.aware.away.clone() else { return };
        let settings = self.settings.clone();
        let git = self.git();
        self.aware.away_ai_running = true;
        self.spawn_bg(
            move || {
                let mut context: String = s.commits.iter().take(60).map(|c| format!("- {} ({})\n", c.summary, c.author)).collect();
                if !s.overlap.is_empty() {
                    context.push_str(&format!("\nFiles the developer is also changing: {}\n", s.overlap.join(", ")));
                }
                if let Some(first) = s.commits.last() {
                    let stat = git.run(["diff", "--stat", &format!("{}~1..{}", first.id, s.trunk)]).unwrap_or_default();
                    context.push_str(&format!("\n{}", stat.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")));
                }
                crate::ai::explain(&settings, crate::ai::Question::SinceAway, &context)
            },
            |this, answer, cx| {
                this.aware.away_ai_running = false;
                this.aware.away_ai = Some(answer);
                cx.notify();
            },
            cx,
        );
        cx.notify();
    }

    /// The "While you were away" card at the top of History.
    ///
    /// # Returns
    /// The card, or nothing when there is nothing new.
    pub(crate) fn render_away_card(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = self.aware.away.clone()?;
        let theme = cx.theme().clone();
        let now = chrono::Utc::now().timestamp();
        let trunk = s.trunk.rsplit_once('/').map(|(_, b)| b.to_string()).unwrap_or_else(|| s.trunk.clone());
        let mut line = t!("away.landed", n = s.commits.len(), who = names(&s.authors), branch = trunk.clone()).to_string();
        if s.commits.len() == 1 {
            line = t!("away.landed_one", who = names(&s.authors), branch = trunk).to_string();
        }
        if !s.prs.is_empty() {
            line = format!("{line} · {}", crate::widgets::plural("away.prs", s.prs.len()));
        }
        let mut rows = v_flex().gap_0p5();
        for (i, c) in s.commits.iter().take(4).enumerate() {
            let id = c.id.clone();
            rows = rows.child(
                hstack()
                    .id(("away-commit", i))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.list_hover))
                    .child(avatar(&c.author, px(18.), cx))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(c.summary.clone()))
                    .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(relative_time(c.time)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ix) = this.commits.iter().position(|x| x.id == id) {
                            this.history_scroll.scroll_to_item(ix, ScrollStrategy::Center);
                        }
                        this.select_commit(id.clone(), cx);
                    })),
            );
        }
        if s.commits.len() > 4 {
            rows = rows.child(div().px_2().text_xs().text_color(theme.muted_foreground).child(t!("safe.and_more", n = s.commits.len() - 4).to_string()));
        }
        let ai_on = self.settings.ai.enabled;
        Some(
            v_flex()
                .mx_4()
                .mt_3()
                .gap_2()
                .p_3()
                .rounded(px(12.))
                .border_1()
                .border_color(theme.blue.opacity(0.35))
                .bg(linear_gradient(110., linear_color_stop(theme.blue.opacity(0.12), 0.), linear_color_stop(theme.sidebar, 0.85)))
                .child(
                    hstack()
                        .gap_2()
                        .child(div().flex_none().size(px(30.)).rounded(px(8.)).bg(theme.blue.opacity(0.16)).flex().items_center().justify_center().child(icon(IconName::Coffee).small().text_color(theme.blue)))
                        .child(v_flex().flex_1().min_w_0().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(t!("away.title", time = span(now - s.since)).to_string())).child(div().text_xs().text_color(theme.muted_foreground).whitespace_normal().child(line)))
                        .when(ai_on, |d| {
                            d.child(framed_if(Button::new("away-ai"), Tone::Ai, !self.aware.away_ai_running && self.aware.away_ai.is_none(), cx).h(px(26.)).icon(icon(IconName::Sparkles)).label(tr("away.summarize")).loading(self.aware.away_ai_running).on_click(cx.listener(|this, _, _, cx| this.summarize_away(cx))))
                        })
                        .child(framed(Button::new("away-ok"), Tone::Plain, cx).h(px(26.)).icon(icon(IconName::Check)).label(tr("away.got_it")).on_click(cx.listener(|this, _, _, cx| this.dismiss_away(cx)))),
                )
                .when(!s.overlap.is_empty(), |d| {
                    d.child(
                        hstack()
                            .w_full()
                            .items_start()
                            .gap_2()
                            .px_2()
                            .py_1p5()
                            .rounded(px(8.))
                            .bg(theme.yellow.opacity(0.1))
                            .text_sm()
                            .text_color(theme.yellow)
                            .child(div().flex_none().pt(px(2.)).child(icon(IconName::TriangleAlert).small()))
                            .child(div().flex_1().min_w_0().whitespace_normal().child(t!("away.overlap", files = s.overlap.iter().take(4).map(|f| f.rsplit('/').next().unwrap_or(f)).collect::<Vec<_>>().join(", ")).to_string()))
                            .child(Button::new("away-sync").xsmall().ghost().label(tr("away.sync")).on_click(cx.listener(|this, _, window, cx| this.sync(window, cx)))),
                    )
                })
                .when_some(self.aware.away_ai.clone(), |d, a| {
                    d.child(match a {
                        Ok(text) => div().px_2().py_1p5().rounded(px(8.)).bg(theme.magenta.opacity(0.08)).text_sm().whitespace_normal().child(crate::pages::pulls::markdown("away-ai-text", &text).unwrap_or_else(|| div().into_any_element())).into_any_element(),
                        Err(e) => div().px_2().text_sm().text_color(theme.red).child(e).into_any_element(),
                    })
                })
                .child(rows)
                .into_any_element(),
        )
    }

    /// The other branch that changes `path` too, if any.
    ///
    /// # Arguments
    /// * `path` - a file the user is changing
    ///
    /// # Returns
    /// The first overlapping branch.
    pub(crate) fn overlap_for(&self, path: &str) -> Option<&Overlap> {
        self.aware.overlaps.iter().find(|o| o.files.iter().any(|f| f == path))
    }

    /// The warning above the Changes list when someone else is changing the same files.
    ///
    /// # Returns
    /// The banner, or nothing without overlaps.
    pub(crate) fn render_overlap_banner(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let first = self.aware.overlaps.first()?.clone();
        let theme = cx.theme().clone();
        let more = self.aware.overlaps.len() - 1;
        let file = first.files.first().map(|f| f.rsplit('/').next().unwrap_or(f).to_string()).unwrap_or_default();
        let text = if first.files.len() > 1 {
            t!("overlap.many_files", who = first.author.clone(), file = file, n = first.files.len() - 1, branch = first.branch.clone()).to_string()
        } else {
            t!("overlap.one_file", who = first.author.clone(), file = file, branch = first.branch.clone()).to_string()
        };
        let branch = first.branch.clone();
        Some(
            v_flex()
                .m_1()
                .mb_2()
                .gap_1p5()
                .p_2p5()
                .rounded(px(10.))
                .border_1()
                .border_color(theme.yellow.opacity(0.4))
                .bg(theme.yellow.opacity(0.08))
                .child(hstack().items_start().gap_2().child(div().flex_none().pt(px(1.)).child(avatar(&first.author, px(20.), cx))).child(div().flex_1().min_w_0().text_sm().whitespace_normal().child(text)))
                .child(
                    h_flex()
                        .gap_1()
                        .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(if more > 0 { crate::widgets::plural("overlap.more", more) } else { tr("overlap.hint").to_string() }))
                        .child(Button::new("overlap-compare").xsmall().ghost().icon(icon(IconName::GitCompareArrows)).label(tr("overlap.compare")).on_click(cx.listener(move |this, _, _, cx| {
                            this.page = Page::Compare;
                            this.open_compare("HEAD".into(), branch.clone(), cx);
                        }))),
                )
                .into_any_element(),
        )
    }
}
