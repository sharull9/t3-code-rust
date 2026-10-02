//! Native pooled subscription quota cards.
use crate::{provider_logo, ui};
use chrono::Utc;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
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
                .min_h(px(36.))
                .when_some(value, |view, value| {
                    view.child(account_bar(account, value, color, cx))
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

fn account_bar(account: &LimitAccount, window: &QuotaWindow, color: Hsla, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let reset = window
        .reset()
        .map(|at| duration((at - Utc::now()).num_seconds()))
        .unwrap_or_else(|| "Reset unknown".into());
    let credits = account
        .limits
        .reset_credits
        .as_ref()
        .filter(|credits| credits.available_count > 0)
        .map(|credits| {
            format!(
                " · {} reset credit{}",
                credits.available_count,
                if credits.available_count == 1 {
                    ""
                } else {
                    "s"
                }
            )
        })
        .unwrap_or_default();
    let detail = format!(
        "{} · {} · Checked {}{}",
        account.plan.as_deref().unwrap_or("Subscription"),
        account.source.as_deref().unwrap_or("Provider instance"),
        account.limits.checked_at,
        account
            .limits
            .reset_credits
            .as_ref()
            .and_then(|credits| credits.next_expires_at.as_ref())
            .map(|at| format!(" · Next credit expires {at}"))
            .unwrap_or_default()
    );
    div()
        .relative()
        .w_full()
        .rounded_md()
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
                .w(relative(window.remaining() as f32 / 100.))
                .bg(color.opacity(0.32)),
        )
        .child(
            v_flex()
                .relative()
                .px_2()
                .py_1()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .font_medium()
                                .child(format!("{}  {:.0}%", account.name, window.remaining())),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .child(format!("↻ {reset}{credits}")),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(detail),
                ),
        )
        .into_any_element()
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
