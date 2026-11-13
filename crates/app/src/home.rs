use std::collections::HashMap;
use std::path::PathBuf;

use cairn_core::insights::CommitStat;
use cairn_core::stack::QueuedPr;
use cairn_core::workspace::RepoSummary;
use chrono::Timelike;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::app::CairnApp;
use crate::widgets::{hstack, icon, relative_time, spacer, tr};

const REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// What the Home dashboard knows about every repository in the library.
#[derive(Default)]
pub(crate) struct HomeData {
    pub summaries: HashMap<PathBuf, RepoSummary>,
    pub queue: Option<Result<Vec<(PathBuf, QueuedPr)>, String>>,
    pub activity: Vec<CommitStat>,
    pub lines: Option<HashMap<String, (u64, u64)>>,
    pub lines_loading: bool,
    pub me: Option<String>,
    pub loading: bool,
    pub loaded: Option<std::time::Instant>,
}

const CALENDAR_WEEKS: i64 = 26;
const BAR_WEEKS: i64 = 12;
const CONTRIBUTOR_DAYS: i64 = 90;
const TOP_CONTRIBUTORS: usize = 6;
const DAY: i64 = 86_400;

/// Rounds a line count for display, such as `3.1k`.
///
/// # Arguments
/// * `n` - the count
///
/// # Returns
/// The short form.
fn short_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.)
    } else if n >= 10_000 {
        format!("{}k", n / 1000)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.)
    } else {
        n.to_string()
    }
}

impl CairnApp {
    /// Reads the status of every repository and the review queue in the background, at most
    /// once a minute.
    pub(crate) fn load_home(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.home.loading || (!force && self.home.loaded.is_some_and(|t| t.elapsed() < REFRESH_AFTER)) {
            return;
        }
        self.home.loading = true;
        let paths: Vec<PathBuf> = self.library.repos.iter().map(|r| r.path.clone()).filter(|p| p.exists()).collect();
        let mock = cfg!(debug_assertions) && std::env::var("CAIRN_DEBUG_MOCK_PRS").is_ok();
        let queue_paths = paths.clone();
        cx.spawn(async move |this, cx| {
            let queue = cx.background_executor().spawn(async move { if mock { Ok(crate::workspace_view::mock_queue(&queue_paths)) } else { crate::workspace_view::load_queue(&queue_paths) } }).await;
            let _ = this.update(cx, |this, cx| {
                this.home.queue = Some(queue);
                cx.notify();
            });
        })
        .detach();
        if force || (self.home.lines.is_none() && !self.home.lines_loading) {
            self.home.lines_loading = true;
            let line_paths = paths.clone();
            cx.spawn(async move |this, cx| {
                let lines = cx
                    .background_executor()
                    .spawn(async move {
                        let stats: Vec<CommitStat> = std::thread::scope(|s| {
                            let handles: Vec<_> = line_paths.iter().map(|p| s.spawn(move || cairn_core::insights::activity(&cairn_core::Git::new(p), CONTRIBUTOR_DAYS as u32, true))).collect();
                            handles.into_iter().filter_map(|h| h.join().ok()).flatten().collect()
                        });
                        let mut by: HashMap<String, (u64, u64)> = HashMap::new();
                        for c in stats {
                            let e = by.entry(c.author.to_lowercase()).or_default();
                            e.0 += c.added as u64;
                            e.1 += c.removed as u64;
                        }
                        by
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.home.lines_loading = false;
                    this.home.lines = Some(lines);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.spawn(async move |this, cx| {
            let (summaries, activity, me) = cx
                .background_executor()
                .spawn(async move {
                    let days = (CALENDAR_WEEKS * 7 + 7) as u32;
                    let (summaries, activity): (Vec<RepoSummary>, Vec<Vec<CommitStat>>) = std::thread::scope(|s| {
                        let handles: Vec<_> = paths.iter().map(|p| s.spawn(move || (cairn_core::workspace::summarize(p), cairn_core::insights::activity(&cairn_core::Git::new(p), days, false)))).collect();
                        handles.into_iter().filter_map(|h| h.join().ok()).unzip()
                    });
                    let me = paths.first().and_then(|p| cairn_core::Git::new(p).run(["config", "user.name"]).ok()).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
                    (summaries, activity.into_iter().flatten().collect::<Vec<_>>(), me)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.home.loading = false;
                this.home.loaded = Some(std::time::Instant::now());
                this.home.summaries = summaries.into_iter().map(|s| (s.path.clone(), s)).collect();
                this.home.activity = activity;
                this.home.me = me;
                cx.notify();
            });
        })
        .detach();
    }

    /// The top of Home: a greeting, a one-line summary, stat tiles and the review queue.
    ///
    /// # Returns
    /// The dashboard.
    pub(crate) fn render_dashboard(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.load_home(false, cx);
        let theme = cx.theme().clone();
        let repos = self.library.repos.len();
        let changed = self.home.summaries.values().filter(|s| s.changed > 0).count();
        let behind = self.home.summaries.values().filter(|s| s.behind > 0).count();
        let unpushed = self.home.summaries.values().filter(|s| s.ahead > 0).count();
        let queue: Vec<(PathBuf, QueuedPr)> = self.home.queue.as_ref().and_then(|q| q.as_ref().ok()).cloned().unwrap_or_default();
        let waiting = queue.iter().filter(|(_, q)| !q.mine).count();
        let hour = chrono::Local::now().hour();
        let greeting = match hour {
            5..=11 => tr("home.good_morning"),
            12..=17 => tr("home.good_afternoon"),
            _ => tr("home.good_evening"),
        };
        let loaded = self.home.loaded.is_some();
        let summary: SharedString = if !loaded {
            tr("home.checking")
        } else if changed + behind + waiting + unpushed == 0 {
            tr("home.all_calm")
        } else {
            let mut parts: Vec<String> = Vec::new();
            if waiting > 0 {
                parts.push(crate::widgets::plural("home.sum_reviews", waiting));
            }
            if changed > 0 {
                parts.push(crate::widgets::plural("home.sum_changed", changed));
            }
            if behind > 0 {
                parts.push(crate::widgets::plural("home.sum_behind", behind));
            }
            if unpushed > 0 {
                parts.push(crate::widgets::plural("home.sum_unpushed", unpushed));
            }
            parts.join(" · ").into()
        };
        let tile = |id: &'static str, ic: IconName, n: Option<usize>, label: SharedString, color: Hsla, cx: &App| {
            let theme = cx.theme();
            h_flex()
                .id(id)
                .flex_1()
                .min_w(px(160.))
                .gap_3()
                .p_4()
                .rounded(px(14.))
                .border_1()
                .border_color(theme.border.opacity(0.8))
                .bg(linear_gradient(135., linear_color_stop(color.opacity(0.12), 0.), linear_color_stop(theme.sidebar, 0.7)))
                .child(div().flex_none().size(px(38.)).rounded(px(11.)).flex().items_center().justify_center().bg(color.opacity(0.16)).border_1().border_color(color.opacity(0.4)).child(icon(ic).text_color(color)))
                .child(
                    v_flex()
                        .child(div().text_2xl().font_weight(FontWeight::BOLD).line_height(rems(1.6)).child(n.map(|n| n.to_string()).unwrap_or_else(|| "–".into())))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(label)),
                )
        };
        let tiles = h_flex()
            .flex_wrap()
            .gap_3()
            .child(tile("home-t-repos", IconName::FolderGit2, Some(repos), tr("home.tile_repos"), theme.primary, cx))
            .child(tile("home-t-changed", IconName::FilePen, loaded.then_some(changed), tr("home.tile_changed"), theme.yellow, cx))
            .child(tile("home-t-behind", IconName::ArrowDownToLine, loaded.then_some(behind), tr("home.tile_behind"), theme.cyan, cx))
            .child(tile("home-t-reviews", IconName::Inbox, self.home.queue.as_ref().map(|_| waiting), tr("home.tile_reviews"), theme.magenta, cx));
        let mut col = v_flex()
            .gap_4()
            .child(
                hstack()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_2xl().font_weight(FontWeight::BOLD).child(greeting))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(summary)),
                    )
                    .when(self.home.loading, |d| d.child(gpui_kit::component::spinner::Spinner::new().small()))
                    .child(
                        gpui_kit::component::button::Button::new("home-refresh")
                            .small()
                            .outline()
                            .icon(icon(IconName::RefreshCw))
                            .tooltip(tr("toolbar.refresh"))
                            .on_click(cx.listener(|this, _, _, cx| this.load_home(true, cx))),
                    ),
            )
            .child(tiles);
        if !queue.is_empty() {
            let mut list = h_flex().flex_wrap().gap_2();
            for (i, (path, q)) in queue.iter().enumerate() {
                let name = self.library.repos.iter().find(|r| &r.path == path).map(|r| r.name.clone()).unwrap_or_default();
                let accent = if q.mine { theme.blue } else { theme.magenta };
                let created = chrono::DateTime::parse_from_rfc3339(&q.created).map(|d| d.timestamp()).unwrap_or(0);
                let (path, number) = (path.clone(), q.number);
                list = list.child(
                    h_flex()
                        .id(("home-queue", i))
                        .w(px(380.))
                        .gap_2p5()
                        .p_3()
                        .rounded(px(12.))
                        .cursor_pointer()
                        .border_1()
                        .border_color(theme.border.opacity(0.8))
                        .bg(theme.sidebar)
                        .hover(|s| s.border_color(accent.opacity(0.6)))
                        .active(|s| s.opacity(crate::widgets::PRESSED))
                        .child(crate::widgets::avatar(&q.author, px(30.), cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(hstack().gap_1p5().text_xs().text_color(theme.muted_foreground).child(icon(IconName::FolderGit2).xsmall()).child(div().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(name)).child(format!("#{}", q.number)).child(spacer()).child(relative_time(created)))
                                .child(div().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(q.title.clone()))
                                .child(hstack().gap_1p5().child(crate::widgets::badge(if q.mine { tr("queue.yours") } else { tr("pulls.your_turn") }, accent)).child(div().text_xs().text_color(theme.muted_foreground).child(q.author.clone()))),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| this.open_pr_in(&path, number, window, cx))),
                );
            }
            col = col.child(hstack().gap_2().pt_1().child(icon(IconName::Inbox).small().text_color(theme.magenta)).child(div().text_base().font_weight(FontWeight::BOLD).child(tr("home.waiting_title")))).child(list);
        }
        col.children(self.render_activity(cx)).into_any_element()
    }

    /// Activity across every repository: a contribution calendar, commits per week and who
    /// contributed how much.
    ///
    /// # Returns
    /// The section, or `None` before the history has loaded or when it is empty.
    pub(crate) fn render_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use chrono::Datelike;
        if self.home.activity.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let today = chrono::Local::now().date_naive();
        let this_monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
        let first_monday = this_monday - chrono::Duration::weeks(CALENDAR_WEEKS - 1);
        let mut days = vec![0u32; (CALENDAR_WEEKS * 7) as usize];
        let now = chrono::Utc::now().timestamp();
        for c in &self.home.activity {
            let Some(date) = chrono::DateTime::from_timestamp(c.time, 0).map(|d| d.with_timezone(&chrono::Local).date_naive()) else { continue };
            let i = (date - first_monday).num_days();
            if (0..CALENDAR_WEEKS * 7).contains(&i) {
                days[i as usize] += 1;
            }
        }
        let total: u32 = days.iter().sum();
        let busiest = days.iter().copied().max().unwrap_or(0).max(1);
        let level = |n: u32| -> f32 {
            if n == 0 {
                0.
            } else {
                match (n as f32 / busiest as f32 * 4.).ceil() as u32 {
                    0 | 1 => 0.3,
                    2 => 0.5,
                    3 => 0.75,
                    _ => 1.,
                }
            }
        };
        let cell = |id: usize, n: u32, date: chrono::NaiveDate, future: bool, cx: &App| {
            let theme = cx.theme();
            let tip: SharedString = crate::widgets::plural_with("home.cell_tip", n as usize, &date.format("%Y-%m-%d").to_string()).into();
            div()
                .id(("cal", id))
                .size(px(12.))
                .rounded(px(3.))
                .when(future, |d| d.invisible())
                .bg(if n == 0 { theme.muted.opacity(0.7) } else { theme.green.opacity(level(n)) })
                .hover(|s| s.border_1().border_color(theme.foreground.opacity(0.6)))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
        };
        let mut months = h_flex().gap(px(3.)).pl(px(30.)).h(px(14.));
        let mut grid = h_flex().gap(px(3.));
        let mut last_month = 0;
        for w in 0..CALENDAR_WEEKS {
            let monday = first_monday + chrono::Duration::weeks(w);
            let label = (monday.month() != last_month).then(|| monday.format("%b").to_string());
            last_month = monday.month();
            months = months.child(div().w(px(12.)).whitespace_nowrap().text_size(px(10.)).text_color(theme.muted_foreground).children(label));
            let mut col = v_flex().gap(px(3.));
            for d in 0..7 {
                let date = monday + chrono::Duration::days(d);
                let i = (w * 7 + d) as usize;
                col = col.child(cell(i, days[i], date, date > today, cx));
            }
            grid = grid.child(col);
        }
        let day_labels = v_flex().gap(px(3.)).w(px(27.)).children(["", "Mon", "", "Wed", "", "Fri", ""].map(|l| div().h(px(12.)).text_size(px(10.)).line_height(px(12.)).text_color(theme.muted_foreground).child(l)));
        let legend = hstack()
            .gap(px(3.))
            .text_size(px(10.))
            .text_color(theme.muted_foreground)
            .child(div().mr_1().child(tr("home.less")))
            .child(div().size(px(10.)).rounded(px(2.)).bg(theme.muted.opacity(0.7)))
            .children([0.3, 0.5, 0.75, 1.].map(|o| div().size(px(10.)).rounded(px(2.)).bg(theme.green.opacity(o))))
            .child(div().ml_1().child(tr("home.more")));
        let card = |cx: &App| v_flex().gap_3().p_4().rounded(px(14.)).border_1().border_color(cx.theme().border.opacity(0.8)).bg(cx.theme().sidebar);
        let calendar = card(cx)
            .flex_none()
            .child(hstack().gap_2().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("home.calendar_title"))).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(crate::widgets::plural("home.commits_total", total as usize))))
            .child(v_flex().gap_1().child(months).child(h_flex().child(day_labels).child(grid)))
            .child(hstack().child(spacer()).child(legend));

        let mut weeks = vec![0u32; BAR_WEEKS as usize];
        let bar_start = this_monday - chrono::Duration::weeks(BAR_WEEKS - 1);
        for c in &self.home.activity {
            let Some(date) = chrono::DateTime::from_timestamp(c.time, 0).map(|d| d.with_timezone(&chrono::Local).date_naive()) else { continue };
            let i = (date - bar_start).num_days() / 7;
            if (0..BAR_WEEKS).contains(&i) && date >= bar_start {
                weeks[i as usize] += 1;
            }
        }
        let top = weeks.iter().copied().max().unwrap_or(0).max(1);
        let mut bars = h_flex().items_end().gap(px(6.)).h(px(120.)).border_b_1().border_color(theme.border);
        for (i, n) in weeks.iter().copied().enumerate() {
            let start = bar_start + chrono::Duration::weeks(i as i64);
            let tip: SharedString = crate::widgets::plural_with("home.week_tip", n as usize, &start.format("%Y-%m-%d").to_string()).into();
            let primary = theme.primary;
            bars = bars.child(
                div()
                    .id(("week-bar", i))
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .cursor_default()
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
                    .child(
                        div()
                            .w_full()
                            .h(relative(n as f32 / top as f32))
                            .min_h(px(if n > 0 { 3. } else { 0. }))
                            .rounded_t(px(4.))
                            .bg(primary.opacity(if i + 1 == BAR_WEEKS as usize { 1. } else { 0.75 }))
                            .hover(move |s| s.bg(primary)),
                    ),
            );
        }
        let this_week = weeks.last().copied().unwrap_or(0);
        let prev_week = weeks.get(weeks.len().saturating_sub(2)).copied().unwrap_or(0);
        let trend: SharedString = if prev_week == 0 { crate::widgets::plural("home.this_week", this_week as usize).into() } else { t!("home.vs_last_week", this = this_week, last = prev_week).to_string().into() };
        let weekly = card(cx)
            .flex_1()
            .min_w(px(300.))
            .child(hstack().gap_2().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("home.weekly_title"))).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(trend)))
            .child(hstack().text_size(px(10.)).text_color(theme.muted_foreground).child(crate::widgets::plural("home.peak", top as usize)))
            .child(bars)
            .child(hstack().text_size(px(10.)).text_color(theme.muted_foreground).child(bar_start.format("%b %d").to_string()).child(spacer()).child(tr("home.this_week_label")));

        let since = now - CONTRIBUTOR_DAYS * DAY;
        let mut people: Vec<(String, u32, u64, u64)> = Vec::new();
        for c in self.home.activity.iter().filter(|c| c.time >= since) {
            match people.iter_mut().find(|p| p.0.eq_ignore_ascii_case(&c.author)) {
                Some(p) => {
                    p.1 += 1;
                    p.2 += c.added as u64;
                    p.3 += c.removed as u64;
                }
                None => people.push((c.author.clone(), 1, c.added as u64, c.removed as u64)),
            }
        }
        people.sort_by_key(|p| std::cmp::Reverse(p.1));
        let team_total: u32 = people.iter().map(|p| p.1).sum();
        let mut rows = div().w_full().grid().grid_cols(2).gap_x_6().gap_y_3();
        for (name, commits, added, removed) in people.iter().take(TOP_CONTRIBUTORS) {
            let you = self.home.me.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(name));
            let share = *commits as f32 / team_total.max(1) as f32 * 100.;
            rows = rows.child(
                v_flex()
                    .gap_1p5()
                    .child(
                        hstack()
                            .gap_2()
                            .child(crate::widgets::avatar(name, px(24.), cx))
                            .child(div().min_w_0().truncate().text_sm().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .when(you, |d| d.child(crate::widgets::badge(tr("home.you"), theme.primary)))
                            .child(spacer())
                            .child(div().text_xs().text_color(theme.foreground).child(crate::widgets::plural("home.commits_n", *commits as usize)))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(format!("{share:.0}%"))),
                    )
                    .child(h_flex().h(px(6.)).w_full().rounded(px(3.)).bg(theme.muted.opacity(0.7)).child(div().h_full().rounded(px(3.)).w(relative(share / 100.)).bg(if you { theme.primary } else { theme.primary.opacity(0.6) })))
                    .child(hstack().gap_2().text_xs().text_color(theme.muted_foreground).child(match self.home.lines.as_ref().map(|l| l.get(&name.to_lowercase()).copied().unwrap_or((*added, *removed))) {
                        Some((a, r)) => t!("home.lines", added = short_count(a), removed = short_count(r)).to_string(),
                        None => t!("home.counting_lines").to_string(),
                    })),
            );
        }
        let contributors = card(cx)
            .w_full()
            .child(hstack().gap_2().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(tr("home.contributors_title"))).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(t!("home.contributors_sub", people = people.len(), days = CONTRIBUTOR_DAYS).to_string())))
            .child(rows);

        Some(
            v_flex()
                .gap_3()
                .child(hstack().gap_2().pt_1().child(icon(IconName::Activity).small().text_color(theme.green)).child(div().text_base().font_weight(FontWeight::BOLD).child(tr("home.activity_title"))).child(div().text_xs().text_color(theme.muted_foreground).child(tr("home.activity_sub"))))
                .child(h_flex().flex_wrap().items_stretch().gap_3().child(calendar).child(weekly))
                .when(!people.is_empty(), |d| d.child(contributors))
                .into_any_element(),
        )
    }

    /// The live status line of a repository card on Home.
    ///
    /// # Arguments
    /// * `path` - the repository
    ///
    /// # Returns
    /// Chips for changes, commits to pull and to push, or "Up to date".
    pub(crate) fn repo_status_chips(&self, path: &PathBuf, cx: &App) -> Option<Div> {
        let s = self.home.summaries.get(path)?;
        let theme = cx.theme();
        let chip = |text: String, c: Hsla| div().flex_none().px(px(7.)).h(px(20.)).flex().items_center().rounded(px(10.)).bg(c.opacity(0.14)).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(c).child(text);
        let mut row = h_flex().flex_wrap().gap_1p5();
        if s.error.is_some() {
            return Some(row.child(chip(t!("workspace.error").to_string(), theme.red)));
        }
        if s.changed > 0 {
            row = row.child(chip(crate::widgets::plural("workspace.changed_files", s.changed), theme.yellow));
        }
        if s.behind > 0 {
            row = row.child(chip(t!("branch_picker.to_pull", n = s.behind).to_string(), theme.cyan));
        }
        if s.ahead > 0 {
            row = row.child(chip(t!("branch_picker.to_push", n = s.ahead).to_string(), theme.blue));
        }
        if s.changed + s.behind as usize + s.ahead as usize == 0 {
            row = row.child(hstack().gap_1().text_xs().text_color(theme.green).child(icon(IconName::CircleCheck).xsmall()).child(tr("workspace.up_to_date")));
        }
        Some(row)
    }
}
