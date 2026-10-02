//! Token usage and API-cost estimates (`server.getUsageSummary`).
//!
//! The server scans each provider's local session logs (Claude Code, Codex,
//! OpenCode, …) and returns per-day, per-model buckets. [`UsageReport`] folds
//! those into the totals the usage page shows, the same way the web app's
//! usage page does for a single environment.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Connection, RpcError};

/// A provider's log scan can take a while on a large history.
const USAGE_TIMEOUT: Duration = Duration::from_secs(180);

/// `UsageWindow` in the contract: an inclusive range of local days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub since_day: String,
    pub until_day: String,
    pub time_zone: String,
}

impl UsageWindow {
    /// The last `days` days, today included, in `time_zone`.
    pub fn last_days(today: chrono::NaiveDate, days: u32, time_zone: impl Into<String>) -> Self {
        let since = today - chrono::Days::new(u64::from(days.max(1) - 1));
        Self {
            since_day: since.format("%Y-%m-%d").to_string(),
            until_day: today.format("%Y-%m-%d").to_string(),
            time_zone: time_zone.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    #[serde(default)]
    pub uncached_input_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
}

impl UsageTotals {
    /// Processed tokens, as the web app counts them: reasoning is already
    /// part of output.
    pub fn processed(&self) -> u64 {
        self.uncached_input_tokens
            + self.cached_input_tokens
            + self.cache_creation_tokens
            + self.output_tokens
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    pub day: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub totals: UsageTotals,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub cache_savings_usd: f64,
    #[serde(default)]
    pub sessions: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSourceFingerprint {
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSource {
    pub fingerprint: UsageSourceFingerprint,
    pub status: String,
    #[serde(default)]
    pub distinct_sessions: u64,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    #[serde(default)]
    pub contract_version: f64,
    pub since_day: String,
    pub until_day: String,
    #[serde(default)]
    pub buckets: Vec<UsageBucket>,
    #[serde(default)]
    pub sources: Vec<UsageSource>,
}

impl Connection {
    /// Refreshes model pricing (best effort), then reads the usage summary.
    pub async fn usage_summary(&self, window: &UsageWindow) -> Result<UsageSummary, RpcError> {
        // Stale or missing rates only make costs less precise; a failure here
        // must not hide the token counts.
        let _ = self.rpc().call::<serde_json::Value>("server.refreshUsageRates", json!({})).await;
        self.rpc()
            .call_with_timeout("server.getUsageSummary", json!(window), USAGE_TIMEOUT)
            .await
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderUsage {
    pub provider: String,
    pub cost_usd: f64,
    pub tokens: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub cost_usd: f64,
    pub tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayUsage {
    pub day: String,
    pub cost_usd: f64,
    pub tokens: u64,
    /// Per provider, in the report's `providers` order.
    pub by_provider: Vec<(f64, u64)>,
}

/// One environment's usage, folded for display.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageReport {
    pub cost_usd: f64,
    pub totals: UsageTotals,
    pub cache_savings_usd: f64,
    pub sessions: u64,
    /// Most sessions first.
    pub providers: Vec<ProviderUsage>,
    /// Highest cost first.
    pub models: Vec<ModelUsage>,
    /// Every day of the window, oldest first, zero-filled.
    pub days: Vec<DayUsage>,
}

impl UsageReport {
    pub fn tokens(&self) -> u64 {
        self.totals.processed()
    }

    pub fn from_summary(summary: &UsageSummary) -> Self {
        let mut report = Self::default();
        let mut providers: HashMap<&str, ProviderUsage> = HashMap::new();
        for source in summary.sources.iter().filter(|source| source.status != "missing") {
            let provider = source.fingerprint.provider.as_str();
            let entry = providers.entry(provider).or_insert_with(|| ProviderUsage {
                provider: provider.to_owned(),
                ..Default::default()
            });
            entry.sessions += source.distinct_sessions;
            report.sessions += source.distinct_sessions;
        }

        let mut models: HashMap<(&str, &str), ModelUsage> = HashMap::new();
        let mut days: BTreeMap<&str, HashMap<&str, (f64, u64)>> = BTreeMap::new();
        for bucket in &summary.buckets {
            let tokens = bucket.totals.processed();
            report.cost_usd += bucket.cost_usd;
            report.cache_savings_usd += bucket.cache_savings_usd;
            report.totals.uncached_input_tokens += bucket.totals.uncached_input_tokens;
            report.totals.cached_input_tokens += bucket.totals.cached_input_tokens;
            report.totals.cache_creation_tokens += bucket.totals.cache_creation_tokens;
            report.totals.output_tokens += bucket.totals.output_tokens;
            report.totals.reasoning_tokens += bucket.totals.reasoning_tokens;

            let provider = providers.entry(&bucket.provider).or_insert_with(|| ProviderUsage {
                provider: bucket.provider.clone(),
                ..Default::default()
            });
            provider.cost_usd += bucket.cost_usd;
            provider.tokens += tokens;

            let model = models.entry((&bucket.provider, &bucket.model)).or_insert_with(|| {
                ModelUsage {
                    provider: bucket.provider.clone(),
                    model: bucket.model.clone(),
                    ..Default::default()
                }
            });
            model.cost_usd += bucket.cost_usd;
            model.tokens += tokens;

            let day = days.entry(&bucket.day).or_default().entry(&bucket.provider).or_default();
            day.0 += bucket.cost_usd;
            day.1 += tokens;
        }

        // A scanned but unused agent (no sessions, nothing logged) is noise.
        report.providers = providers
            .into_values()
            .filter(|p| p.sessions > 0 || p.tokens > 0 || p.cost_usd > 0.)
            .collect();
        report.providers.sort_by(|a, b| {
            b.sessions
                .cmp(&a.sessions)
                .then_with(|| b.tokens.cmp(&a.tokens))
                .then_with(|| a.provider.cmp(&b.provider))
        });
        report.models = models.into_values().collect();
        report.models.sort_by(|a, b| {
            b.cost_usd
                .total_cmp(&a.cost_usd)
                .then_with(|| b.tokens.cmp(&a.tokens))
                .then_with(|| a.model.cmp(&b.model))
        });

        let mut day = chrono::NaiveDate::parse_from_str(&summary.since_day, "%Y-%m-%d").ok();
        let until = chrono::NaiveDate::parse_from_str(&summary.until_day, "%Y-%m-%d").ok();
        let mut labels: Vec<String> = Vec::new();
        while let (Some(current), Some(until)) = (day, until) {
            if current > until || labels.len() > 366 {
                break;
            }
            labels.push(current.format("%Y-%m-%d").to_string());
            day = current.succ_opt();
        }
        // Buckets outside the requested window still count, at their own date.
        for key in days.keys() {
            if !labels.iter().any(|label| label == key) {
                labels.push((*key).to_owned());
            }
        }
        labels.sort();
        report.days = labels
            .into_iter()
            .map(|label| {
                let by_day = days.get(label.as_str());
                let by_provider: Vec<(f64, u64)> = report
                    .providers
                    .iter()
                    .map(|provider| {
                        by_day
                            .and_then(|map| map.get(provider.provider.as_str()))
                            .copied()
                            .unwrap_or_default()
                    })
                    .collect();
                DayUsage {
                    cost_usd: by_provider.iter().map(|(cost, _)| cost).sum(),
                    tokens: by_provider.iter().map(|(_, tokens)| tokens).sum(),
                    day: label,
                    by_provider,
                }
            })
            .collect();
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> UsageSummary {
        serde_json::from_value(json!({
            "contractVersion": 4, "readAt": "2026-10-02T00:00:00Z", "timeZone": "UTC",
            "sinceDay": "2026-09-30", "untilDay": "2026-10-02",
            "buckets": [
                { "day": "2026-09-30", "provider": "claude", "model": "claude-opus-5",
                  "totals": { "uncachedInputTokens": 10, "cachedInputTokens": 100, "cacheCreationTokens": 5, "outputTokens": 20, "reasoningTokens": 4 },
                  "costUsd": 2.0, "cacheSavingsUsd": 1.5, "costSource": "modelPriced", "records": 3, "unpricedRecords": 0, "sessions": 1 },
                { "day": "2026-10-02", "provider": "codex", "model": "gpt-6",
                  "totals": { "uncachedInputTokens": 1, "cachedInputTokens": 2, "cacheCreationTokens": 0, "outputTokens": 3, "reasoningTokens": 0 },
                  "costUsd": 0.5, "cacheSavingsUsd": 0.25, "costSource": "providerReported", "records": 1, "unpricedRecords": 0, "sessions": 1 },
                { "day": "2026-10-02", "provider": "claude", "model": "claude-opus-5",
                  "totals": { "uncachedInputTokens": 1, "cachedInputTokens": 0, "cacheCreationTokens": 0, "outputTokens": 1, "reasoningTokens": 0 },
                  "costUsd": 1.0, "cacheSavingsUsd": 0.0, "costSource": "modelPriced", "records": 1, "unpricedRecords": 0, "sessions": 1 }
            ],
            "sources": [
                { "fingerprint": { "hostId": "h", "provider": "claude", "resolvedHomePath": "~/.claude", "volumeId": null }, "status": "ok", "scannedFiles": 3, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 2 },
                { "fingerprint": { "hostId": "h", "provider": "codex", "resolvedHomePath": "~/.codex", "volumeId": null }, "status": "ok", "scannedFiles": 1, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 5 },
                { "fingerprint": { "hostId": "h", "provider": "grok", "resolvedHomePath": "~/.grok", "volumeId": null }, "status": "missing", "scannedFiles": 0, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 9 },
                { "fingerprint": { "hostId": "h", "provider": "cursor", "resolvedHomePath": "~/.cursor", "volumeId": null }, "status": "ok", "scannedFiles": 0, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 0 }
            ],
            "pricing": { "status": "fresh", "source": "models.dev", "fetchedAt": null, "knownModels": 10 },
            "scanDurationMs": 12
        }))
        .unwrap()
    }

    #[test]
    fn window_spans_inclusive_days() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
        let window = UsageWindow::last_days(today, 30, "Asia/Kolkata");
        assert_eq!(window.since_day, "2026-09-03");
        assert_eq!(window.until_day, "2026-10-02");
        assert_eq!(
            serde_json::to_value(&window).unwrap(),
            json!({ "sinceDay": "2026-09-03", "untilDay": "2026-10-02", "timeZone": "Asia/Kolkata" })
        );
        assert_eq!(UsageWindow::last_days(today, 1, "UTC").since_day, "2026-10-02");
    }

    #[test]
    fn report_folds_buckets_by_provider_model_and_day() {
        let report = UsageReport::from_summary(&summary());
        assert_eq!(report.tokens(), 135 + 6 + 2);
        assert_eq!(report.totals.cached_input_tokens, 102);
        assert!((report.cost_usd - 3.5).abs() < 1e-9);
        assert!((report.cache_savings_usd - 1.75).abs() < 1e-9);
        // Missing sources and unused agents don't count toward sessions or providers.
        assert_eq!(report.sessions, 7);
        let providers: Vec<_> = report.providers.iter().map(|p| p.provider.as_str()).collect();
        assert_eq!(providers, ["codex", "claude"]);
        assert_eq!(report.providers[1].tokens, 137);
        assert_eq!(report.models[0].model, "claude-opus-5");
        assert!((report.models[0].cost_usd - 3.0).abs() < 1e-9);
        let days: Vec<_> = report.days.iter().map(|d| d.day.as_str()).collect();
        assert_eq!(days, ["2026-09-30", "2026-10-01", "2026-10-02"]);
        assert_eq!(report.days[1].tokens, 0);
        assert_eq!(report.days[2].by_provider, vec![(0.5, 6), (1.0, 2)]);
    }
}
