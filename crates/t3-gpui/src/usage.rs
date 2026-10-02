//! Usage page: tokens and estimated API cost across the agents' local logs
//! (`server.getUsageSummary`), opened from the sidebar footer.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::AreaChart;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use t3_client::usage::DayUsage;
use t3_client::{UsageReport, UsageSummary, UsageWindow};

use crate::ui::{self, icon};

/// Reopening the page within this long reuses the last read.
const FRESH_FOR: Duration = Duration::from_secs(60);
const RANGES: [u32; 3] = [7, 30, 90];
const DEFAULT_RANGE: u32 = 30;
const COLUMN_WIDTH: Pixels = px(120.);
const Y_TICKS: usize = 5;

pub enum UsageEvent {
    Load {
        request_id: u64,
        window: UsageWindow,
    },
    LoadLimits {
        request_id: u64,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Metric {
    Cost,
    Tokens,
    Limits,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Breakdown {
    Model,
    Day,
}

pub struct UsageView {
    active: bool,
    limits_wide: bool,
    quota_config: t3_client::ServerConfig,
    limits: t3_client::quotas::LimitsReport,
    limits_pending: Option<u64>,
    limits_loaded_at: Option<Instant>,
    limits_error: Option<String>,
    connected: bool,
    days: u32,
    metric: Metric,
    breakdown: Breakdown,
    next_request: u64,
    pending: Option<u64>,
    report: Option<UsageReport>,
    /// The range `report` covers; a different one shows the skeleton.
    report_days: u32,
    loaded_at: Option<Instant>,
    error: Option<String>,
}

impl EventEmitter<UsageEvent> for UsageView {}

impl UsageView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.active && this.metric == Metric::Limits {
                            this.ensure_fresh(cx);
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
            }
        })
        .detach();
        Self {
            active: false,
            limits_wide: false,
            quota_config: t3_client::ServerConfig::default(),
            limits: t3_client::quotas::LimitsReport::default(),
            limits_pending: None,
            limits_loaded_at: None,
            limits_error: None,
            connected: false,
            days: DEFAULT_RANGE,
            metric: Metric::Cost,
            breakdown: Breakdown::Model,
            next_request: 0,
            pending: None,
            report: None,
            report_days: DEFAULT_RANGE,
            loaded_at: None,
            error: None,
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn set_config(&mut self, config: t3_client::ServerConfig, cx: &mut Context<Self>) {
        self.quota_config = config;
        self.update_limits(cx);
    }

    pub fn set_providers(
        &mut self,
        providers: Vec<t3_client::ServerProvider>,
        cx: &mut Context<Self>,
    ) {
        self.quota_config.providers = providers;
        self.update_limits(cx);
    }

    fn update_limits(&mut self, cx: &mut Context<Self>) {
        self.limits = t3_client::quotas::LimitsReport::from_config(&self.quota_config);
        cx.notify();
    }

    pub fn finish_limits(
        &mut self,
        request_id: u64,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        if self.limits_pending != Some(request_id) {
            return;
        }
        self.limits_pending = None;
        // Throttle failed automatic probes too; manual refresh can always retry.
        self.limits_loaded_at = Some(Instant::now());
        self.limits_error = result.err();
        cx.notify();
    }

    pub fn set_connected(&mut self, connected: bool, cx: &mut Context<Self>) {
        if self.connected != connected {
            self.connected = connected;
            if !connected {
                self.pending = None;
                self.limits_pending = None;
            }
            cx.notify();
        }
    }

    /// Forgets the previous server's numbers.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.quota_config = t3_client::ServerConfig::default();
        self.limits = t3_client::quotas::LimitsReport::default();
        self.limits_pending = None;
        self.limits_loaded_at = None;
        self.limits_error = None;
        self.pending = None;
        self.report = None;
        self.loaded_at = None;
        self.error = None;
        cx.notify();
    }

    /// Loads when there is nothing recent to show for the selected range.
    pub fn ensure_fresh(&mut self, cx: &mut Context<Self>) {
        if self.metric == Metric::Limits {
            if self.limits_pending.is_none()
                && self
                    .limits_loaded_at
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(300))
            {
                self.refresh(cx);
            }
            return;
        }
        let fresh = self.report.is_some()
            && self.report_days == self.days
            && self.loaded_at.is_some_and(|at| at.elapsed() < FRESH_FOR);
        if !fresh && self.pending.is_none() {
            self.refresh(cx);
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.connected {
            return;
        }
        if self.metric == Metric::Limits {
            if self.limits_pending.is_some() {
                return;
            }
            self.next_request += 1;
            self.limits_pending = Some(self.next_request);
            self.limits_error = None;
            cx.emit(UsageEvent::LoadLimits {
                request_id: self.next_request,
            });
            cx.notify();
            return;
        }
        if self.pending.is_some() {
            return;
        }
        self.next_request += 1;
        let request_id = self.next_request;
        self.pending = Some(request_id);
        self.error = None;
        let time_zone = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into());
        let today = chrono::Local::now().date_naive();
        let window = UsageWindow::last_days(today, self.days, time_zone);
        cx.emit(UsageEvent::Load { request_id, window });
        cx.notify();
    }

    pub fn finish(
        &mut self,
        request_id: u64,
        result: Result<UsageSummary, String>,
        cx: &mut Context<Self>,
    ) {
        if self.pending != Some(request_id) {
            return;
        }
        self.pending = None;
        match result {
            Ok(summary) => {
                self.report = Some(UsageReport::from_summary(&summary));
                self.report_days = self.days;
                self.loaded_at = Some(Instant::now());
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn set_days(&mut self, days: u32, cx: &mut Context<Self>) {
        if self.days != days {
            self.days = days;
            // A load for the old range is no longer wanted.
            self.pending = None;
            self.refresh(cx);
        }
    }
}

impl Render for UsageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let report = self
            .report
            .as_ref()
            .filter(|_| self.report_days == self.days);
        let body = if self.metric == Metric::Limits {
            crate::limits_view::render(&self.limits, self.connected, self.limits_wide, cx)
        } else {
            match report {
                Some(report) => self.render_report(report, cx).into_any_element(),
                None if self.pending.is_some() || (self.connected && self.error.is_none()) => {
                    render_skeleton(cx).into_any_element()
                }
                None => self.render_unavailable(cx).into_any_element(),
            }
        };

        let entity = cx.entity().downgrade();
        div()
            .id("usage-page")
            .size_full()
            .flex()
            .flex_col()
            .on_prepaint(move |bounds, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    let wide = bounds.size.width >= px(740.);
                    if this.limits_wide != wide {
                        this.limits_wide = wide;
                        cx.notify();
                    }
                });
            })
            .test_support()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(1080.))
                    .mx_auto()
                    .px_6()
                    .pt_4()
                    .pb_4()
                    .flex_shrink_0()
                    .child(self.render_toolbar(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(1080.))
                            .mx_auto()
                            .px_6()
                            .pb_8()
                            .gap_6()
                            .when_some(
                                self.limits_error
                                    .clone()
                                    .filter(|_| self.metric == Metric::Limits),
                                |page, error| {
                                    page.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().danger)
                                            .child(format!("Could not refresh limits: {error}")),
                                    )
                                },
                            )
                            .when_some(
                                self.error
                                    .clone()
                                    .filter(|_| report.is_some() && self.metric != Metric::Limits),
                                |page, error| {
                                    page.child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().danger)
                                            .child(format!("Could not refresh usage: {error}")),
                                    )
                                },
                            )
                            .child(body),
                    ),
            )
    }
}

impl UsageView {
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let range = TabBar::new("usage-range")
            .segmented()
            .small()
            .selected_index(
                RANGES
                    .iter()
                    .position(|days| *days == self.days)
                    .unwrap_or(1),
            )
            .children(
                RANGES
                    .iter()
                    .map(|days| Tab::new().label(format!("{days}d"))),
            )
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(days) = RANGES.get(*index) {
                    this.set_days(*days, cx);
                }
            }));
        let metric = TabBar::new("usage-metric")
            .segmented()
            .small()
            .selected_index(match self.metric {
                Metric::Cost => 0,
                Metric::Tokens => 1,
                Metric::Limits => 2,
            })
            .child(Tab::new().label("Cost"))
            .child(Tab::new().label("Tokens"))
            .child(Tab::new().label("Limits"))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                this.metric = match *index {
                    0 => Metric::Cost,
                    1 => Metric::Tokens,
                    _ => Metric::Limits,
                };
                this.ensure_fresh(cx);
                cx.notify();
            }));
        let refreshing = if self.metric == Metric::Limits {
            self.limits_pending.is_some()
        } else {
            self.pending.is_some()
        };

        h_flex()
            .flex_wrap()
            .gap_3()
            .items_center()
            // The title bar already names the page.
            .child(
                div()
                    .flex_1()
                    .when(refreshing && self.report.is_some(), |slot| {
                        slot.child(ui::loader(
                            "usage-refreshing",
                            gpui_kit::component::Size::XSmall,
                        ))
                    }),
            )
            .child(metric)
            .when(self.metric != Metric::Limits, |row| row.child(range))
            .child(
                Button::new("usage-refresh")
                    .ghost()
                    .small()
                    .icon(icon(IconName::RefreshCw))
                    .tooltip("Refresh usage")
                    .disabled(refreshing || !self.connected)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .text_color(theme.foreground)
    }

    fn render_unavailable(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let message = match (&self.error, self.connected) {
            (_, false) => "Connect to a server to see usage.".to_owned(),
            (Some(error), true) => format!("Could not read usage: {error}"),
            (None, true) => "No usage yet.".to_owned(),
        };
        v_flex()
            .py_16()
            .gap_3()
            .items_center()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(icon(IconName::ChartNoAxesColumn).large())
            .child(message)
            .when(self.connected, |empty| {
                empty.child(
                    Button::new("usage-retry")
                        .small()
                        .label("Try again")
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                )
            })
    }

    fn render_report(&self, report: &UsageReport, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let metric = self.metric;
        debug_assert!(metric != Metric::Limits);
        let (headline, subtitle, chart_title) = match metric {
            Metric::Cost => (
                format_usd(report.cost_usd),
                format!("{} · API estimate", plural(report.sessions, "session")),
                "Daily cost",
            ),
            Metric::Tokens => (
                format_tokens(report.tokens()),
                plural(report.sessions, "session"),
                "Daily processed tokens",
            ),
            Metric::Limits => unreachable!(),
        };

        let providers = report.providers.iter().enumerate().map(|(ix, usage)| {
            let provider = Provider::of(&usage.provider);
            let color = provider.color(cx);
            let (value, detail) = match metric {
                Metric::Cost => (
                    format_usd(usage.cost_usd),
                    format!(
                        "{} of cost · {} tokens",
                        format_share(share(usage.cost_usd, report.cost_usd)),
                        format_tokens(usage.tokens)
                    ),
                ),
                Metric::Tokens => (
                    format_tokens(usage.tokens),
                    format!(
                        "{} of tokens · {}",
                        format_share(share(usage.tokens as f64, report.tokens() as f64)),
                        format_usd(usage.cost_usd)
                    ),
                ),
                Metric::Limits => unreachable!(),
            };
            v_flex()
                .id(("usage-provider", ix))
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().size_1p5().flex_shrink_0().rounded_full().bg(color))
                        .child(crate::provider_logo::logo(&usage.provider, px(14.), color))
                        .child(div().text_sm().font_medium().child(provider.name.clone()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(plural(usage.sessions, "session")),
                        )
                        .child(div().text_sm().font_semibold().child(value)),
                )
                .child(
                    div()
                        .pl_3p5()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                )
        });

        let overview = h_flex()
            .flex_wrap()
            .items_start()
            .gap_x_10()
            .gap_y_6()
            .child(
                v_flex()
                    .w(px(300.))
                    .flex_shrink_0()
                    .gap_4()
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .id("usage-headline")
                                    .text_size(px(36.))
                                    .line_height(px(42.))
                                    .font_bold()
                                    .child(headline),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(subtitle),
                            ),
                    )
                    .children(providers),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(320.))
                    .gap_3()
                    .child(div().text_sm().font_semibold().child(chart_title))
                    .child(self.render_chart(report, cx)),
            );

        let totals = [
            ("Processed tokens", format_tokens(report.tokens())),
            (
                "Cached input",
                format_tokens(report.totals.cached_input_tokens),
            ),
            (
                "Uncached input",
                format_tokens(report.totals.uncached_input_tokens),
            ),
            ("Output", format_tokens(report.totals.output_tokens)),
            ("Cache savings", format_usd(report.cache_savings_usd)),
        ];

        v_flex()
            .gap_8()
            .child(overview)
            .child(section(
                "Totals",
                h_flex()
                    .flex_wrap()
                    .gap_y_4()
                    .children(totals.into_iter().map(|(label, value)| {
                        stat(label, div().text_lg().font_semibold().child(value), cx)
                    })),
                cx,
            ))
            .child(self.render_breakdown(report, cx))
    }

    fn render_chart(&self, report: &UsageReport, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        if report.providers.is_empty() || report.days.is_empty() {
            return div()
                .h(px(220.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_lg()
                .bg(theme.secondary)
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No usage in this period")
                .into_any_element();
        }
        let metric = self.metric;
        let peak = report
            .days
            .iter()
            .flat_map(|day| day.by_provider.iter())
            .map(|(cost, tokens)| match metric {
                Metric::Cost => *cost,
                Metric::Tokens => *tokens as f64,
                Metric::Limits => unreachable!(),
            })
            .fold(0., f64::max);
        // Round ticks ($25, $50, …) rather than quarters of the peak.
        let step = nice_step(peak / (Y_TICKS - 1) as f64);
        let mut chart = AreaChart::new(report.days.clone())
            .id("usage-chart")
            .x(|day: &DayUsage| SharedString::from(chart_day(&day.day)))
            .x_tick_count(3)
            .grid_dashed(false)
            .y_axis(true)
            .y_tick_count(Y_TICKS)
            .y_domain(0., step * (Y_TICKS - 1) as f64)
            // No headroom, so the top tick reads the round domain maximum.
            .y_padding(0., 0.)
            .y_tick_format(move |value| match metric {
                Metric::Cost => format_usd(value),
                Metric::Tokens => format_tokens(value.max(0.).round() as u64),
                Metric::Limits => unreachable!(),
            })
            .tooltip_title(|day: &DayUsage| SharedString::from(table_day(&day.day)))
            .tooltip_value(move |_, _, value| match metric {
                Metric::Cost => format_usd(value).into(),
                Metric::Tokens => format_tokens(value.max(0.).round() as u64).into(),
                Metric::Limits => unreachable!(),
            });
        for (ix, usage) in report.providers.iter().enumerate() {
            let provider = Provider::of(&usage.provider);
            let color = provider.color(cx);
            chart = chart
                .y(move |day: &DayUsage| {
                    let (cost, tokens) = day.by_provider.get(ix).copied().unwrap_or_default();
                    match metric {
                        Metric::Cost => cost,
                        Metric::Tokens => tokens as f64,
                        Metric::Limits => unreachable!(),
                    }
                })
                .stroke(color)
                .fill(color.opacity(0.08))
                .natural()
                .name(provider.name);
        }
        div().h(px(240.)).w_full().child(chart).into_any_element()
    }

    fn render_breakdown(&self, report: &UsageReport, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let toggle = TabBar::new("usage-breakdown")
            .segmented()
            .small()
            .selected_index(if self.breakdown == Breakdown::Model {
                0
            } else {
                1
            })
            .child(Tab::new().label("Model"))
            .child(Tab::new().label("Day"))
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                this.breakdown = if *index == 0 {
                    Breakdown::Model
                } else {
                    Breakdown::Day
                };
                cx.notify();
            }));

        let total_cost = report.cost_usd;
        let rows: Vec<(AnyElement, f64, u64)> = match self.breakdown {
            Breakdown::Model => {
                let mut models: Vec<_> = report.models.iter().collect();
                if self.metric == Metric::Tokens {
                    models.sort_by_key(|model| std::cmp::Reverse(model.tokens));
                }
                models
                    .into_iter()
                    .map(|model| {
                        let provider = Provider::of(&model.provider);
                        let label = h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(crate::provider_logo::logo(
                                &model.provider,
                                px(14.),
                                provider.color(cx),
                            ))
                            .child(div().min_w_0().truncate().child(model.model.clone()));
                        (label.into_any_element(), model.cost_usd, model.tokens)
                    })
                    .collect()
            }
            Breakdown::Day => report
                .days
                .iter()
                .rev()
                .filter(|day| day.tokens > 0 || day.cost_usd > 0.)
                .map(|day| {
                    (
                        div().child(table_day(&day.day)).into_any_element(),
                        day.cost_usd,
                        day.tokens,
                    )
                })
                .collect(),
        };
        let empty = rows.is_empty();
        let label_header = if self.breakdown == Breakdown::Model {
            "Model"
        } else {
            "Day"
        };

        let header = h_flex()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().flex_1().child(label_header))
            .child(column("Cost"))
            .child(column("Share"))
            .child(column("Tokens"));
        let table = v_flex()
            .id("usage-breakdown-table")
            .child(header)
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(|(ix, (label, cost, tokens))| {
                        h_flex()
                            .id(("usage-row", ix))
                            .py_2()
                            .border_b_1()
                            .border_color(theme.border.opacity(0.6))
                            .text_sm()
                            .child(div().flex_1().min_w_0().child(label))
                            .child(column(format_usd(cost)))
                            .child(
                                column(format_share(share(cost, total_cost)))
                                    .text_color(theme.muted_foreground),
                            )
                            .child(column(format_tokens(tokens)).text_color(theme.muted_foreground))
                    }),
            )
            .when(empty, |table| {
                table.child(
                    div()
                        .py_4()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No usage in this period"),
                )
            });

        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child("Breakdown"))
                    .child(toggle),
            )
            .child(table)
    }
}

/// The page's shape while the first read runs: the same layout, in skeletons.
fn render_skeleton(cx: &App) -> impl IntoElement {
    let bar = |w: f32, h: f32| Skeleton::new().w(px(w)).h(px(h)).rounded_md();
    let provider = |ix: usize| {
        v_flex()
            .id(("usage-provider-skeleton", ix))
            .gap_1p5()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Skeleton::new().size_2().rounded_full())
                    .child(Skeleton::new().size_4().rounded_full())
                    .child(bar(80., 14.))
                    .child(div().flex_1())
                    .child(bar(56., 14.)),
            )
            .child(bar(144., 16.))
    };
    let totals = [
        "Processed tokens",
        "Cached input",
        "Uncached input",
        "Output",
        "Cache savings",
    ];

    v_flex()
        .id("usage-skeleton")
        .gap_8()
        .child(
            h_flex()
                .flex_wrap()
                .items_start()
                .gap_x_10()
                .gap_y_6()
                .child(
                    v_flex()
                        .w(px(300.))
                        .flex_shrink_0()
                        .gap_4()
                        .child(v_flex().gap_1().child(bar(144., 40.)).child(bar(128., 16.)))
                        .children((0..6).map(provider)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(320.))
                        .gap_3()
                        .child(bar(96., 20.))
                        .child(Skeleton::new().w_full().h(px(224.)).rounded_lg())
                        .child(Skeleton::new().w_full().h(px(16.)).rounded_md()),
                ),
        )
        .child(section(
            "Totals",
            h_flex().flex_wrap().gap_y_4().children(
                totals
                    .into_iter()
                    .map(|label| stat(label, bar(64., 24.), cx)),
            ),
            cx,
        ))
        .child(
            v_flex()
                .gap_3()
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().font_semibold().child("Breakdown"))
                        .child(bar(112., 28.)),
                )
                .child(Skeleton::new().w_full().h(px(176.)).rounded_lg()),
        )
}

fn section(title: &'static str, body: impl IntoElement, _: &App) -> impl IntoElement {
    v_flex()
        .gap_3()
        .child(div().text_sm().font_semibold().child(title))
        .child(body)
}

fn stat(label: &'static str, value: impl IntoElement, cx: &App) -> impl IntoElement {
    v_flex()
        .w(relative(0.2))
        .min_w(COLUMN_WIDTH)
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(value)
}

fn column(text: impl Into<SharedString>) -> Div {
    div()
        .w(COLUMN_WIDTH)
        .flex_shrink_0()
        .text_right()
        .child(text.into())
}

/// How the page names and colors each agent the server scans.
struct Provider {
    name: SharedString,
    color: Option<u32>,
}

impl Provider {
    fn of(provider: &str) -> Self {
        let (name, color): (&str, Option<u32>) = match provider {
            "claude" => ("Claude Code", Some(0xd97757)),
            // Codex draws in the foreground color, as the web app does.
            "codex" => ("Codex", None),
            "opencode" => ("OpenCode", Some(0x5ba4cf)),
            "antigravity" => ("Antigravity", Some(0x9a86f0)),
            "cursor" => ("Cursor", Some(0x9a9aa3)),
            "grok" => ("Grok", Some(0xe0a84a)),
            other => {
                let mut chars = other.chars();
                let name: String = chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default();
                return Self {
                    name: name.into(),
                    color: Some(0x8b8b93),
                };
            }
        };
        Self {
            name: name.into(),
            color,
        }
    }

    fn color(&self, cx: &App) -> Hsla {
        self.color.map(ui::hex).unwrap_or(cx.theme().foreground)
    }
}

/// The smallest 1, 2, 2.5 or 5 times a power of ten at or above `raw`.
fn nice_step(raw: f64) -> f64 {
    if raw <= 0. || !raw.is_finite() {
        return 1.;
    }
    let magnitude = 10f64.powf(raw.log10().floor());
    [1., 2., 2.5, 5., 10.]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|step| *step >= raw * (1. - 1e-9))
        .unwrap_or(10. * magnitude)
}

fn plural(count: u64, noun: &str) -> String {
    format!(
        "{} {noun}{}",
        group_thousands(count),
        if count == 1 { "" } else { "s" }
    )
}

fn share(part: f64, total: f64) -> f64 {
    if total > 0. { part / total } else { 0. }
}

fn format_share(share: f64) -> String {
    if share > 0. && share < 0.001 {
        "<0.1%".into()
    } else {
        format!("{:.1}%", share * 100.)
    }
}

/// Three significant figures with a K/M/B suffix: 6.27B, 879M, 65.4M.
fn format_tokens(tokens: u64) -> String {
    let value = tokens as f64;
    let (scaled, suffix) = match value {
        v if v >= 1e9 => (v / 1e9, "B"),
        v if v >= 1e6 => (v / 1e6, "M"),
        v if v >= 1e3 => (v / 1e3, "K"),
        _ => return tokens.to_string(),
    };
    let digits = if scaled >= 100. {
        0
    } else if scaled >= 10. {
        1
    } else {
        2
    };
    format!("{scaled:.digits$}{suffix}")
}

/// Dollars with thousands separators: $1,764.90.
fn format_usd(amount: f64) -> String {
    let cents = (amount.abs() * 100.).round() as u64;
    let sign = if amount < 0. && cents > 0 { "-" } else { "" };
    format!("{sign}${}.{:02}", group_thousands(cents / 100), cents % 100)
}

fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, digit) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

fn parse_day(day: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

/// "SEP 3", the chart's axis labels.
fn chart_day(day: &str) -> String {
    parse_day(day).map_or_else(
        || day.to_owned(),
        |d| d.format("%b %-d").to_string().to_uppercase(),
    )
}

/// "Oct 2, 2026", for the day table and tooltips.
fn table_day(day: &str) -> String {
    parse_day(day).map_or_else(|| day.to_owned(), |d| d.format("%b %-d, %Y").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::TestAppContext;
    use gpui_kit::test::TestWindowExt as _;
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};

    fn quota_config() -> t3_client::ServerConfig {
        serde_json::from_value(json!({ "providers": [
            { "instanceId":"codex-a", "driver":"codex", "displayName":"Work", "enabled":true, "installed":true,
              "usageLimits": { "checkedAt":"2026-10-02T10:00:00Z", "windows":[
                {"id":"primary","kind":"session","label":"Session","usedPercent":47,"resetsAt":"2026-10-02T15:00:00Z"}
              ] } },
            { "instanceId":"codex-b", "driver":"codex", "displayName":"Personal", "enabled":true, "installed":true,
              "usageLimits": { "checkedAt":"2026-10-02T10:00:00Z", "windows":[
                {"id":"primary","kind":"session","label":"Session","usedPercent":52}
              ] } }
        ] })).unwrap()
    }

    #[gpui_kit::test]
    fn limits_navigation_keeps_history_controls_and_handles_refresh_and_offline(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(420.), px(480.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(UsageView::new),
            )
            .unwrap()
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&page, move |_, event: &UsageEvent, _| {
                captured.borrow_mut().push(match event {
                    UsageEvent::Load { request_id, .. } => ("history", *request_id),
                    UsageEvent::LoadLimits { request_id } => ("limits", *request_id),
                });
            })
        });
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| {
                page.set_connected(true, cx);
                page.set_config(quota_config(), cx);
            });
            window.render_frame(cx);
            window.within("usage-range").click(0_usize, cx);
            window.render_frame(cx);
            window.within("usage-metric").click(1_usize, cx);
            window.render_frame(cx);
            window.within("usage-metric").click(2_usize, cx);
            window.render_frame(cx);
            assert!(window.try_find("usage-limits-content").is_some());
            let card = window.find("quota-card-0-0").bounds();
            let page_bounds = window.find("usage-page").bounds();
            assert!(card.right() <= page_bounds.right());
            for id in ["quota-0-0-0", "quota-0-0-1"] {
                let bar = window.find(id).bounds();
                assert!(bar.right() <= page_bounds.right());
                assert!(bar.size.width > px(0.));
            }
            window.click("usage-refresh", cx); // Pending check cannot duplicate a probe.
        })
        .unwrap();
        assert_eq!(
            events
                .borrow()
                .iter()
                .map(|(kind, _)| *kind)
                .collect::<Vec<_>>(),
            ["history", "limits"]
        );
        let request = events.borrow()[1].1;
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| page.finish_limits(request + 1, Ok(()), cx));
            assert_eq!(page.read(cx).limits_pending, Some(request));
            page.update(cx, |page, cx| {
                page.finish_limits(request, Err("Probe failed".into()), cx)
            });
            assert_eq!(page.read(cx).limits.accounts.len(), 2);
            window.render_frame(cx);
            window.click("usage-refresh", cx);
        })
        .unwrap();
        assert_eq!(events.borrow().len(), 3);
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| page.set_connected(false, cx));
            window.render_frame(cx);
            window.click("usage-refresh", cx);
            assert_eq!(page.read(cx).limits.accounts.len(), 2);
            window.within("usage-metric").click(1_usize, cx);
        })
        .unwrap();
        // Let GPUI deliver the entity notification before inspecting the new frame.
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(page.read(cx).metric == Metric::Tokens);
            assert!(
                window
                    .within("usage-range")
                    .find(0_usize)
                    .bounds()
                    .size
                    .width
                    > px(0.)
            );
            assert!(window.try_find("usage-limits-content").is_none());
            assert_eq!(page.read(cx).days, 7);
            assert!(page.read(cx).metric == Metric::Tokens);
            page.update(cx, |page, cx| page.reset(cx));
            assert!(page.read(cx).limits.accounts.is_empty());
            // Completion from the previous server must not revive its state.
            page.update(cx, |page, cx| page.finish_limits(request + 1, Ok(()), cx));
            assert!(page.read(cx).limits_loaded_at.is_none());
        })
        .unwrap();
        assert_eq!(events.borrow().len(), 3);
    }

    #[gpui_kit::test]
    fn wide_limits_keep_account_columns_aligned_when_one_window_is_missing(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1100.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(UsageView::new),
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| {
                let mut config = quota_config();
                config.providers[0]
                    .usage_limits
                    .as_mut()
                    .unwrap()
                    .windows
                    .push(
                        serde_json::from_value(json!({
                            "id":"weekly", "kind":"weekly", "label":"Weekly", "usedPercent":15
                        }))
                        .unwrap(),
                    );
                page.metric = Metric::Limits;
                page.set_config(config, cx);
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(page.read(cx).limits_wide);
            let session_a = window.find("quota-0-0-0").bounds();
            let session_b = window.find("quota-0-0-1").bounds();
            let weekly_a = window.find("quota-0-1-0").bounds();
            let weekly_gap = window.find("quota-0-1-1").bounds();
            assert_eq!(session_a.origin.y, session_b.origin.y);
            assert!(session_a.right() <= session_b.origin.x);
            assert_eq!(session_a.origin.x, weekly_a.origin.x);
            assert_eq!(session_b.origin.x, weekly_gap.origin.x);
            assert_eq!(session_a.size.width, weekly_a.size.width);
            assert!(session_b.right() <= window.find("usage-page").bounds().right());
        })
        .unwrap();
    }

    #[test]
    fn numbers_read_like_the_web_usage_page() {
        assert_eq!(format_tokens(6_270_000_000), "6.27B");
        assert_eq!(format_tokens(879_000_000), "879M");
        assert_eq!(format_tokens(65_400_000), "65.4M");
        assert_eq!(format_tokens(950), "950");
        assert_eq!(format_usd(1764.9), "$1,764.90");
        assert_eq!(format_usd(11_951.594), "$11,951.59");
        assert_eq!(format_usd(0.), "$0.00");
        assert_eq!(format_share(0.417), "41.7%");
        assert_eq!(format_share(0.0004), "<0.1%");
        assert_eq!(plural(1, "session"), "1 session");
        assert_eq!(plural(1688, "session"), "1,688 sessions");
        assert_eq!(chart_day("2026-09-03"), "SEP 3");
        assert_eq!(nice_step(92.06 / 4.), 25.);
        assert_eq!(nice_step(400e6 / 4.), 100e6);
        assert_eq!(nice_step(0.), 1.);
        assert_eq!(table_day("2026-10-02"), "Oct 2, 2026");
    }
}
