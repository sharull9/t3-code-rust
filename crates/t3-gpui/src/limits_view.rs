//! Native pooled subscription quota cards.
use crate::{provider_logo, ui};
use chrono::Utc;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::quotas::{LimitAccount, LimitPool, LimitsReport, QuotaWindow};

pub fn render(report: &LimitsReport, connected: bool, wide: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    v_flex().id("usage-limits-content").test_support().w_full().gap_6()
        .when(!connected, |view| view.child(div().text_sm().text_color(theme.muted_foreground)
            .child("Offline · Showing the last reported limits. Reconnect to refresh.")))
        .children(report.notices.iter().map(|notice| div().text_sm().text_color(theme.warning).child(notice.clone())))
        .children(report.pools().into_iter().enumerate().map(|(index, pool)| {
            let color = if matches!(pool.driver.as_str(), "claude" | "claudeAgent") { ui::hex(0xd97757) } else { theme.foreground };
            v_flex().id(("limits-provider", index)).gap_3()
                .child(h_flex().gap_2().items_center()
                    .child(provider_logo::logo(&pool.driver, px(18.), color))
                    .child(div().text_sm().font_semibold().child(ui::provider_label(Some(&pool.driver)))))
                .children(pool.windows.iter().enumerate().map(|(row, window)| render_window(&pool, window, index, row, wide, cx)))
                .into_any_element()
        }))
        .when(report.accounts.is_empty(), |view| view.child(v_flex().py_8().gap_2()
            .child(div().text_sm().font_medium().child("No subscription limits reported"))
            .child(div().text_sm().text_color(theme.muted_foreground).child(if connected {
                "Limits appear when a signed-in provider or configured quota source reports them. API cost and token history are available in the other tabs."
            } else { "Connect to a server to read your provider subscription limits." }))))
        .children(report.external_links.iter().enumerate().map(|(index, link)| {
            let url = link.url.clone();
            Button::new(("limits-external", index)).ghost().small().label(link.label.clone())
                .on_click(move |_, _, cx| { if url::Url::parse(&url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) { cx.open_url(&url); } })
        }))
        .child(div().text_xs().text_color(theme.muted_foreground)
            .child(format!("Updated {} · Limits refresh every 5 minutes while this tab is open.",
                report.accounts.iter().filter_map(|a| chrono::DateTime::parse_from_rfc3339(&a.limits.checked_at).ok())
                    .max().map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_else(|| "—".into()))))
        .into_any_element()
}

fn render_window(
    pool: &LimitPool,
    window: &QuotaWindow,
    provider: usize,
    row: usize,
    wide: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let now = Utc::now();
    let color = if matches!(pool.driver.as_str(), "claude" | "claudeAgent") {
        ui::hex(0xd97757)
    } else {
        theme.foreground
    };
    let next_reset = pool
        .members(window)
        .filter_map(|(_, w)| w.reset().map(|at| (at, w.used_percent)))
        .filter(|(at, _)| *at > now)
        .min_by_key(|(at, _)| *at);
    let count = pool.members(window).count().max(1);
    let stat = v_flex()
        .gap_1()
        .when(wide, |view| view.w(px(170.)).flex_shrink_0())
        .child(div().text_sm().font_medium().child(window.label.clone()))
        .child(
            h_flex()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .text_size(px(28.))
                        .font_semibold()
                        .child(format!("{:.0}%", pool.remaining(window))),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("left"),
                ),
        )
        .when_some(pool.pace(window, now), |view, pace| {
            view.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(pace),
            )
        })
        .when_some(next_reset, |view, (at, used)| {
            view.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "+{:.0}% in {}",
                        used / count as f64,
                        duration((at - now).num_seconds())
                    )),
            )
        });
    let bars = div()
        .flex()
        .flex_wrap()
        .flex_1()
        .w_full()
        .min_w_0()
        .gap_2()
        .children(pool.accounts.iter().enumerate().map(|(index, account)| {
            let value = account
                .limits
                .windows
                .iter()
                .find(|w| w.id == window.id && w.kind == window.kind);
            div()
                .id(format!("quota-{provider}-{row}-{index}"))
                .test_support()
                .flex_1()
                .min_w(px(180.))
                .min_h(px(30.))
                .when_some(value, |view, value| {
                    let id = format!("quota-hover-{provider}-{row}-{index}");
                    view.child(account_bar(id, &pool.driver, account, value, count, color, cx))
                })
        }));
    div()
        .id(format!("quota-card-{provider}-{row}"))
        .test_support()
        .flex()
        .items_center()
        .gap_4()
        .when(!wide, |view| view.flex_col().items_start())
        .p_4()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .child(stat)
        .child(bars)
        .into_any_element()
}

/// One account's share of a window: a solid bar of what's left over a
/// hatched track, the name and percentage beneath it, and the details in a
/// card on hover.
fn account_bar(
    id: String,
    driver: &str,
    account: &LimitAccount,
    window: &QuotaWindow,
    pool_size: usize,
    color: Hsla,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let remaining = window.remaining();
    // Opaque, so the track's hatching doesn't show through what's left.
    let fill = theme.muted.blend(color.opacity(0.45));
    let bar = v_flex()
        .w_full()
        .gap_1()
        .cursor_default()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(10.))
                .rounded_sm()
                .overflow_hidden()
                .bg(theme.muted)
                .child(
                    svg()
                        .data(include_bytes!("../assets/quota-track.svg"))
                        .absolute()
                        .size_full()
                        .text_color(color.opacity(0.18)),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(relative(remaining as f32 / 100.))
                        .bg(fill),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .text_xs()
                .child(div().flex_1().min_w_0().truncate().font_medium().child(account.name.clone()))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(theme.muted_foreground)
                        .child(format!("{remaining:.0}%")),
                ),
        );
    let details = BarDetails::new(driver, account, window, pool_size, color);
    HoverCard::new(SharedString::from(id))
        .anchor(Anchor::TopLeft)
        .open_delay(std::time::Duration::from_millis(150))
        .trigger(bar)
        .content(move |_, _, cx| details.render(cx))
        .into_any_element()
}

/// What the hover card shows, captured when the bar renders.
struct BarDetails {
    driver: String,
    color: Hsla,
    name: String,
    source: Option<String>,
    plan: Option<String>,
    window: String,
    remaining: f64,
    reset: Option<chrono::DateTime<Utc>>,
    /// The pool's share this window gives back on reset.
    restores: f64,
    credits: Option<(u64, Option<String>)>,
    checked_at: String,
}

impl BarDetails {
    fn new(
        driver: &str,
        account: &LimitAccount,
        window: &QuotaWindow,
        pool_size: usize,
        color: Hsla,
    ) -> Self {
        Self {
            driver: driver.to_owned(),
            color,
            name: account.name.clone(),
            source: account.source.clone(),
            plan: account.plan.clone(),
            window: window.label.clone(),
            remaining: window.remaining(),
            reset: window.reset(),
            restores: window.used_percent / pool_size.max(1) as f64,
            credits: account
                .limits
                .reset_credits
                .as_ref()
                .filter(|credits| credits.available_count > 0)
                .map(|credits| (credits.available_count, credits.next_expires_at.clone())),
            checked_at: account.limits.checked_at.clone(),
        }
    }

    fn render(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let now = Utc::now();
        let row = |label: &'static str, value: String| {
            h_flex()
                .gap_3()
                .text_xs()
                .child(div().w(px(72.)).flex_shrink_0().text_color(theme.muted_foreground).child(label))
                .child(div().flex_1().min_w_0().child(value))
        };
        let resets = match self.reset {
            Some(at) if at > now => format!(
                "{} · in {}",
                at.with_timezone(&chrono::Local).format("%-m/%-d %-I:%M %P"),
                duration((at - now).num_seconds())
            ),
            Some(_) => "Reset due".into(),
            None => "Unknown".into(),
        };
        let checked = chrono::DateTime::parse_from_rfc3339(&self.checked_at)
            .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_else(|_| "—".into());
        let credits = self.credits.as_ref().map(|(count, expires)| {
            let expires = expires
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| format!(" · expires in {}", duration((at.to_utc() - now).num_seconds())))
                .unwrap_or_default();
            format!("{count} banked{expires}")
        });
        v_flex()
            .w(px(280.))
            .gap_3()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(provider_logo::logo(&self.driver, px(16.), self.color))
                            .child(div().min_w_0().truncate().text_sm().font_semibold().child(self.name.clone())),
                    )
                    .children(self.source.clone().map(|source| {
                        div().text_xs().text_color(theme.muted_foreground).child(source)
                    })),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .pt_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(row("Plan", self.plan.clone().unwrap_or_else(|| "Subscription".into())))
                    .child(row("Window", self.window.clone())),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .pt_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(row("Left", format!("{:.0}%", self.remaining)))
                    .child(row("Resets", resets))
                    .child(row("Restores", format!("+{:.0}% of pool", self.restores))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().min_w_0().truncate().child(credits.unwrap_or_else(|| "No banked resets".into())))
                    .child(format!("Checked {checked}")),
            )
            .into_any_element()
    }
}

fn duration(seconds: i64) -> String {
    if seconds <= 0 {
        return "Reset due".into();
    }
    let minutes = (seconds + 59) / 60;
    if minutes >= 1440 {
        format!("{}d {}h", minutes / 1440, minutes % 1440 / 60)
    } else if minutes >= 60 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    }
}
