use std::time::Duration;

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

const WIDTHS: [f32; 11] = [0.62, 0.48, 0.71, 0.39, 0.56, 0.67, 0.44, 0.52, 0.75, 0.35, 0.6];

/// The color placeholder shapes are filled with.
#[derive(Clone, Copy)]
struct Ink(Hsla);

impl Ink {
    fn new(cx: &App) -> Self {
        let t = cx.theme();
        Self(t.foreground.opacity(if t.is_dark() { 0.1 } else { 0.08 }))
    }

    /// One rounded placeholder bar.
    ///
    /// # Arguments
    /// * `w` - width, absolute or relative
    /// * `h` - height in pixels
    ///
    /// # Returns
    /// The bar.
    fn bar(self, w: impl Into<DefiniteLength>, h: f32) -> Div {
        let w: DefiniteLength = w.into();
        div().flex_none().w(w).h(px(h)).rounded(px(h.min(8.) / 2. + 2.)).bg(self.0)
    }

    fn circle(self, size: f32) -> Div {
        div().size(px(size)).flex_none().rounded_full().bg(self.0)
    }
}

/// Lets the whole placeholder breathe in one animation, so every shape pulses together.
///
/// # Arguments
/// * `id` - animation id, unique per placeholder kind
/// * `body` - the placeholder layout
///
/// # Returns
/// The animated placeholder.
fn pulse(id: &'static str, body: Div) -> AnimationElement<Div> {
    body.with_animation(id, Animation::new(Duration::from_millis(1400)).repeat().with_easing(bounce(ease_in_out)), |this, delta| this.opacity(1.0 - delta * 0.5))
}

/// A pseudo-random but stable width for placeholder row `i`.
///
/// # Arguments
/// * `i` - row index
///
/// # Returns
/// A fraction of the available width.
fn width(i: usize) -> f32 {
    WIDTHS[(i * 7 + 3) % WIDTHS.len()]
}

fn details_body(k: Ink) -> Div {
    let mut files = v_flex().gap_2();
    for i in 0..6 {
        files = files.child(h_flex().gap_2().child(k.bar(px(14.), 14.)).child(k.bar(relative(width(i + 2)), 9.)).child(div().flex_1()).child(k.bar(px(34.), 9.)));
    }
    v_flex()
        .size_full()
        .p_4()
        .gap_3()
        .child(h_flex().gap_1().child(k.bar(px(110.), 18.)).child(k.bar(px(130.), 18.)))
        .child(k.bar(relative(0.85), 16.))
        .child(k.bar(relative(0.6), 10.))
        .child(h_flex().gap_2().child(k.circle(22.)).child(k.bar(px(120.), 10.)).child(k.bar(px(90.), 9.)))
        .child(k.bar(relative(0.5), 9.))
        .child(k.bar(relative(0.4), 9.))
        .child(div().h(px(4.)))
        .child(files)
}

fn diff_body(k: Ink, cx: &App) -> Div {
    let t = cx.theme();
    let mut lines = v_flex().gap(px(9.)).p_4();
    for i in 0..22 {
        let indent = [0., 16., 32., 16., 32., 48., 32., 16.][i % 8];
        let tint = match i % 9 {
            3 | 4 => Some(t.green.opacity(0.1)),
            6 => Some(t.red.opacity(0.1)),
            _ => None,
        };
        lines = lines.child(
            h_flex()
                .gap_3()
                .py(px(2.))
                .when_some(tint, |d, c| d.bg(c).rounded(px(3.)))
                .child(k.bar(px(22.), 8.))
                .child(div().flex_none().w(px(indent)))
                .child(k.bar(relative(width(i) * 0.8), 9.)),
        );
    }
    v_flex().size_full().child(h_flex().h(px(46.)).px_4().gap_3().border_b_1().border_color(t.border).child(k.bar(px(30.), 18.)).child(k.bar(px(240.), 12.)).child(k.bar(px(70.), 10.))).child(lines)
}

/// Placeholder for the History page while the first commits load: graph, rows and the details
/// panel, shaped like the real page so nothing jumps when it arrives.
///
/// # Arguments
/// * `avatars` - whether rows show author avatars
///
/// # Returns
/// The placeholder page.
pub(crate) fn history(avatars: bool, cx: &App) -> AnimationElement<Div> {
    let t = cx.theme();
    let k = Ink::new(cx);
    let row_h = crate::history::ROW_H;
    let mut rows = v_flex().flex_1().min_h_0().overflow_hidden();
    for i in 0..18 {
        let side = (3..7).contains(&i);
        rows = rows.child(
            h_flex()
                .h(px(row_h))
                .items_center()
                .pl_3()
                .pr_4()
                .gap(px(10.))
                .border_b_1()
                .border_color(t.border.opacity(0.4))
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px(52.))
                        .h_full()
                        .child(div().absolute().left(px(13.5)).top_0().bottom_0().w(px(2.)).bg(k.0))
                        .when(side, |d| d.child(div().absolute().left(px(27.5)).top_0().bottom_0().w(px(2.)).bg(k.0)))
                        .child(div().absolute().left(px(if side { 24. } else { 10. })).top(px(row_h / 2. - 4.5)).child(k.circle(9.))),
                )
                .child(h_flex().flex_1().min_w_0().gap_2().when(i % 5 == 0, |d| d.child(k.bar(px(78.), 16.))).child(k.bar(relative(width(i)), 10.)))
                .when(avatars, |d| d.child(k.circle(22.)))
                .child(k.bar(px(90.), 9.))
                .child(k.bar(px(60.), 9.))
                .child(k.bar(px(52.), 9.)),
        );
    }
    pulse(
        "skeleton-history",
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(h_flex().px_4().py(px(10.)).gap_2().border_b_1().border_color(t.border).child(k.bar(px(190.), 30.)).child(k.bar(px(80.), 12.)).child(k.bar(px(220.), 28.)))
                    .child(h_flex().h(px(30.)).pl_3().pr_4().gap(px(10.)).border_b_1().border_color(t.border.opacity(0.6)).child(k.bar(px(44.), 8.)).child(k.bar(px(80.), 8.)))
                    .child(rows),
            )
            .child(div().flex_none().w(px(380.)).h_full().border_l_1().border_color(t.border).bg(t.sidebar).child(details_body(k))),
    )
}

/// Placeholder for the message and changed files of a commit's hover card.
///
/// # Returns
/// The animated placeholder.
pub(crate) fn card_files(cx: &App) -> AnimationElement<Div> {
    let k = Ink::new(cx);
    let mut col = v_flex().gap_2().child(k.bar(relative(0.8), 9.)).child(k.bar(relative(0.55), 9.)).child(div().h(px(2.)));
    for w in WIDTHS.iter().take(3) {
        col = col.child(h_flex().gap_2().child(k.bar(px(18.), 18.)).child(k.bar(relative(w * 0.8), 10.)));
    }
    pulse("skeleton-card", col)
}

/// Placeholder for a commit's details panel.
///
/// # Returns
/// Chips, title, author lines and a file list.
pub(crate) fn details(cx: &App) -> AnimationElement<Div> {
    pulse("skeleton-details", details_body(Ink::new(cx)))
}

/// Placeholder for the Changes page: file lists on the left, a diff on the right.
///
/// # Returns
/// The placeholder page.
pub(crate) fn changes(cx: &App) -> AnimationElement<Div> {
    let t = cx.theme();
    let k = Ink::new(cx);
    let mut list = v_flex().gap_2p5().p_3();
    for i in 0..9 {
        if i == 0 || i == 5 {
            list = list.child(k.bar(px(90.), 9.).mt_2());
        }
        list = list.child(h_flex().gap_2().child(k.bar(px(14.), 14.)).child(k.bar(relative(width(i)), 10.)));
    }
    pulse(
        "skeleton-changes",
        h_flex()
            .size_full()
            .child(v_flex().flex_none().w(px(340.)).h_full().border_r_1().border_color(t.border).bg(t.sidebar).child(list))
            .child(div().flex_1().min_w_0().h_full().child(diff_body(k, cx))),
    )
}

/// Placeholder for a diff: a file header and code-shaped lines.
///
/// # Returns
/// The placeholder diff.
pub(crate) fn diff(cx: &App) -> AnimationElement<Div> {
    pulse("skeleton-diff", diff_body(Ink::new(cx), cx))
}

/// Placeholder rows for a list page such as Branches, Stashes or Lost & found.
///
/// # Arguments
/// * `n` - how many rows
///
/// # Returns
/// A card of placeholder rows.
pub(crate) fn list(n: usize, cx: &App) -> AnimationElement<Div> {
    let t = cx.theme();
    let k = Ink::new(cx);
    let mut card = crate::widgets::card(cx).child(h_flex().px_4().py_3().gap_2().border_b_1().border_color(t.border).child(k.bar(px(16.), 16.)).child(k.bar(px(140.), 12.)));
    for i in 0..n {
        card = card.child(
            h_flex()
                .px_4()
                .py(px(11.))
                .gap_3()
                .border_b_1()
                .border_color(t.border.opacity(0.6))
                .child(k.circle(26.))
                .child(v_flex().flex_1().gap_1p5().child(k.bar(relative(width(i)), 11.)).child(k.bar(relative(width(i + 4) * 0.5), 8.)))
                .child(k.bar(px(64.), 22.)),
        );
    }
    pulse("skeleton-list", v_flex().size_full().p_4().gap_3().child(card))
}

/// Placeholder for the pull request list.
///
/// # Arguments
/// * `n` - how many cards
///
/// # Returns
/// Placeholder cards shaped like pull requests.
pub(crate) fn pr_list(n: usize, cx: &App) -> AnimationElement<Div> {
    let t = cx.theme();
    let k = Ink::new(cx);
    let mut col = v_flex().gap_1p5().p_2();
    for i in 0..n {
        col = col.child(
            v_flex()
                .gap_2()
                .pl(px(14.))
                .pr_3()
                .py(px(10.))
                .rounded(px(12.))
                .border_1()
                .border_color(t.border.opacity(0.6))
                .child(h_flex().gap_2().child(k.circle(18.)).child(k.bar(px(70.), 9.)).child(div().flex_1()).child(k.bar(px(40.), 8.)))
                .child(k.bar(relative((width(i) + 0.2).min(0.95)), 12.))
                .child(h_flex().gap_1p5().child(k.bar(px(70.), 18.)).child(div().flex_1()).child(k.bar(px(50.), 9.))),
        );
    }
    pulse("skeleton-pr-list", v_flex().size_full().child(h_flex().px_3().pt_3().pb_2().gap_2().child(k.bar(px(110.), 16.))).child(h_flex().px_3().pb_3().gap_1p5().children((0..4).map(|_| k.bar(px(64.), 22.)))).child(col))
}

/// Placeholder for an open pull request: the header card, the tab strip and the conversation.
///
/// # Returns
/// The placeholder detail view.
pub(crate) fn pr_detail(cx: &App) -> AnimationElement<Div> {
    let t = cx.theme();
    let k = Ink::new(cx);
    let mut talk = v_flex().gap_3().px_4();
    for i in 0..4 {
        let mine = i % 2 == 1;
        talk = talk.child(
            h_flex()
                .gap_2()
                .items_start()
                .when(mine, |d| d.flex_row_reverse())
                .child(k.circle(28.))
                .child(v_flex().w(relative(0.55)).gap_1p5().p_3().rounded(px(12.)).border_1().border_color(t.border).child(k.bar(px(110.), 9.)).child(k.bar(relative(0.95), 9.)).child(k.bar(relative(width(i)), 9.))),
        );
    }
    pulse(
        "skeleton-pr-detail",
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .m_4()
                    .mb_3()
                    .p_5()
                    .gap_4()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.sidebar)
                    .child(
                        h_flex()
                            .gap_4()
                            .items_start()
                            .child(k.circle(48.))
                            .child(v_flex().flex_1().gap_2().child(k.bar(px(220.), 10.)).child(k.bar(relative(0.7), 22.)).child(h_flex().gap_2().child(k.bar(px(120.), 22.)).child(k.bar(px(90.), 22.))))
                            .child(k.circle(76.)),
                    )
                    .child(h_flex().gap_2p5().children((0..5).map(|_| k.bar(px(112.), 52.)))),
            )
            .child(h_flex().px_4().pb_3().child(k.bar(px(470.), 36.)))
            .child(talk),
    )
}

/// Placeholder rows for the sidebar's branch list.
///
/// # Arguments
/// * `n` - how many rows
///
/// # Returns
/// Placeholder rows.
pub(crate) fn sidebar(n: usize, cx: &App) -> AnimationElement<Div> {
    let k = Ink::new(cx);
    let mut col = v_flex().gap_2p5().px_2().py_2();
    for i in 0..n {
        col = col.child(h_flex().gap_2().child(k.bar(px(12.), 12.)).child(k.bar(relative(width(i) + 0.15), 9.)));
    }
    pulse("skeleton-sidebar", col)
}
