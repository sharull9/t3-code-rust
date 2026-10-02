//! Subscription quotas from provider snapshots and `usageLimitSourcesUpdated`.
//! Mirrors the account deduplication and equal-share pooling in upstream
//! `packages/shared/src/usageLimits.ts`; percentages describe quota remaining.
use crate::types::ServerConfig;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProviderAuth {
    pub email: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub window_duration_mins: Option<u64>,
}

impl QuotaWindow {
    pub fn remaining(&self) -> f64 {
        100. - self.used_percent.clamp(0., 100.)
    }
    pub fn reset(&self) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(self.resets_at.as_deref()?)
            .ok()
            .map(|at| at.with_timezone(&Utc))
    }
    pub fn elapsed_share(&self, now: DateTime<Utc>) -> Option<f64> {
        let duration = self.window_duration_mins? as f64 * 60.;
        if duration <= 0. {
            return None;
        }
        Some((1. - (self.reset()? - now).num_seconds() as f64 / duration).clamp(0., 1.))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredits {
    pub available_count: u64,
    pub next_expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuotaUnavailable {
    pub reason: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalUsage {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimits {
    pub checked_at: String,
    #[serde(default, deserialize_with = "compatible_array")]
    pub windows: Vec<QuotaWindow>,
    pub credential_fingerprint: Option<String>,
    pub reset_credits: Option<ResetCredits>,
    pub unavailable: Option<QuotaUnavailable>,
    pub external_usage: Option<ExternalUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAccount {
    pub id: String,
    pub driver: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub usage_limits: UsageLimits,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitSource {
    pub id: String,
    pub label: String,
    #[serde(default, deserialize_with = "compatible_array")]
    pub accounts: Vec<SourceAccount>,
    pub error: Option<String>,
}

fn compatible_array<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(Vec::<Value>::deserialize(deserializer)?
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

/// Apply full snapshots and partial events without erasing quota-source state.
pub fn apply_config_event(
    current: &mut Option<ServerConfig>,
    event: Value,
) -> Result<bool, serde_json::Error> {
    match event["type"].as_str() {
        Some("snapshot") => {
            let mut next: ServerConfig = serde_json::from_value(event["config"].clone())?;
            let carries_sources = next
                .environment
                .as_ref()
                .is_some_and(|env| env.capabilities["usageLimitSources"] == true);
            if carries_sources && let Some(previous) = current.as_ref() {
                next.usage_limit_sources = previous.usage_limit_sources.clone();
            }
            *current = Some(next);
        }
        Some("providerStatuses") => {
            let providers = serde_json::from_value(event["payload"]["providers"].clone())?;
            current.get_or_insert_with(ServerConfig::default).providers = providers;
        }
        Some("usageLimitSourcesUpdated") => {
            let sources = serde_json::from_value(event["payload"]["sources"].clone())?;
            current
                .get_or_insert_with(ServerConfig::default)
                .usage_limit_sources = sources;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

#[derive(Debug, Clone)]
pub struct LimitAccount {
    pub key: String,
    pub driver: String,
    pub name: String,
    pub plan: Option<String>,
    pub source: Option<String>,
    pub limits: UsageLimits,
}

#[derive(Debug, Clone, Default)]
pub struct LimitsReport {
    pub accounts: Vec<LimitAccount>,
    pub notices: Vec<String>,
    pub external_links: Vec<ExternalUsage>,
}

impl LimitsReport {
    pub fn from_config(config: &ServerConfig) -> Self {
        let mut report = Self::default();
        for provider in &config.providers {
            if !provider.enabled
                || !provider.installed
                || provider.availability.as_deref() == Some("unavailable")
            {
                continue;
            }
            let Some(limits) = &provider.usage_limits else {
                continue;
            };
            let name = provider
                .display_name
                .clone()
                .unwrap_or_else(|| provider.instance_id.clone());
            report.insert(
                &provider.driver,
                provider.auth.email.as_deref(),
                &provider.instance_id,
                name,
                provider.auth.label.clone(),
                None,
                limits.clone(),
            );
        }
        for source in &config.usage_limit_sources {
            if let Some(error) = &source.error {
                report.notices.push(format!("{}: {error}", source.label));
            } else if source.accounts.is_empty() {
                report
                    .notices
                    .push(format!("{}: No accounts reported.", source.label));
            }
            for account in &source.accounts {
                // Keep addresses out of the default UI, as upstream does.
                let name = account
                    .email
                    .as_deref()
                    .map(redacted_name)
                    .unwrap_or_else(|| account.id.trim_end_matches(".json").to_owned());
                report.insert(
                    &account.driver,
                    account.email.as_deref(),
                    &format!("{}:{}", source.id, account.id),
                    name,
                    account.plan.clone(),
                    Some(source.label.clone()),
                    account.usage_limits.clone(),
                );
            }
        }
        report
    }

    fn insert(
        &mut self,
        driver: &str,
        email: Option<&str>,
        fallback: &str,
        name: String,
        plan: Option<String>,
        source: Option<String>,
        limits: UsageLimits,
    ) {
        if let Some(link) = &limits.external_usage
            && !self.external_links.iter().any(|seen| seen.url == link.url)
        {
            self.external_links.push(link.clone());
        }
        if let Some(unavailable) = &limits.unavailable {
            if unavailable.reason != "unsupported" {
                self.notices.push(format!(
                    "{name}: {}",
                    unavailable
                        .message
                        .as_deref()
                        .unwrap_or("Could not read limits.")
                ));
            }
            return;
        }
        if limits.windows.is_empty() {
            self.notices.push(format!("{name}: No limits reported."));
            return;
        }
        let identity = email
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .or_else(|| {
                limits
                    .credential_fingerprint
                    .as_ref()
                    .map(|s| format!("credential:{s}"))
            })
            .unwrap_or_else(|| fallback.to_owned());
        let key = format!("{driver}:{identity}");
        if let Some(existing) = self.accounts.iter_mut().find(|account| account.key == key) {
            let next_at = DateTime::parse_from_rfc3339(&limits.checked_at).ok();
            let old_at = DateTime::parse_from_rfc3339(&existing.limits.checked_at).ok();
            if next_at > old_at {
                let credits = limits
                    .reset_credits
                    .clone()
                    .or(existing.limits.reset_credits.clone());
                existing.limits = limits;
                existing.limits.reset_credits = credits;
            } else if existing.limits.reset_credits.is_none() {
                existing.limits.reset_credits = limits.reset_credits;
            }
            if existing.source.is_some() && source.is_none() {
                existing.name = name;
                existing.source = None;
            }
            if existing.plan.is_none() {
                existing.plan = plan;
            }
        } else {
            self.accounts.push(LimitAccount {
                key,
                driver: driver.into(),
                name,
                plan,
                source,
                limits,
            });
        }
    }

    pub fn pools(&self) -> Vec<LimitPool> {
        let mut pools: Vec<LimitPool> = Vec::new();
        for account in &self.accounts {
            let index = pools
                .iter()
                .position(|pool| pool.driver == account.driver)
                .unwrap_or_else(|| {
                    pools.push(LimitPool {
                        driver: account.driver.clone(),
                        accounts: Vec::new(),
                        windows: Vec::new(),
                    });
                    pools.len() - 1
                });
            pools[index].accounts.push(account.clone());
        }
        for pool in &mut pools {
            let order_window = pool
                .accounts
                .iter()
                .flat_map(|account| &account.limits.windows)
                .min_by_key(|window| kind_order(&window.kind))
                .map(|window| (window.kind.clone(), window.id.clone()));
            pool.accounts.sort_by_key(|account| {
                let reset = order_window
                    .as_ref()
                    .and_then(|(kind, id)| {
                        account
                            .limits
                            .windows
                            .iter()
                            .find(|window| window.kind == *kind && window.id == *id)
                    })
                    .and_then(QuotaWindow::reset);
                (
                    reset.is_none(),
                    reset,
                    account.name.to_lowercase(),
                    account.key.clone(),
                )
            });
            for account in &pool.accounts {
                for window in &account.limits.windows {
                    if !pool
                        .windows
                        .iter()
                        .any(|seen| seen.id == window.id && seen.kind == window.kind)
                    {
                        pool.windows.push(window.clone());
                    }
                }
            }
            pool.windows.sort_by_key(|window| kind_order(&window.kind));
            if pool.driver == "cursor"
                && pool.windows.iter().any(|w| w.id == "autoPercentUsed")
                && pool.windows.iter().any(|w| w.id == "apiPercentUsed")
            {
                pool.windows.retain(|w| w.id != "totalPercentUsed");
            }
        }
        pools
    }
}

pub struct LimitPool {
    pub driver: String,
    pub accounts: Vec<LimitAccount>,
    pub windows: Vec<QuotaWindow>,
}

impl LimitPool {
    pub fn members<'a>(
        &'a self,
        window: &'a QuotaWindow,
    ) -> impl Iterator<Item = (&'a LimitAccount, &'a QuotaWindow)> {
        self.accounts.iter().filter_map(move |account| {
            account
                .limits
                .windows
                .iter()
                .find(|candidate| candidate.kind == window.kind && candidate.id == window.id)
                .map(|w| (account, w))
        })
    }
    pub fn remaining(&self, window: &QuotaWindow) -> f64 {
        let values: Vec<_> = self.members(window).map(|(_, w)| w.remaining()).collect();
        (values.iter().sum::<f64>() / values.len().max(1) as f64).round()
    }
    pub fn pace(&self, window: &QuotaWindow, now: DateTime<Utc>) -> Option<&'static str> {
        let timed: Vec<_> = self
            .members(window)
            .filter_map(|(_, w)| {
                w.elapsed_share(now)
                    .map(|elapsed| w.used_percent - elapsed * 100.)
            })
            .collect();
        if timed.is_empty() {
            return None;
        }
        let gap = timed.iter().sum::<f64>() / timed.len() as f64;
        Some(if gap > 5. {
            "Ahead of pace"
        } else if gap < -5. {
            "Under pace"
        } else {
            "On pace"
        })
    }
}

fn kind_order(kind: &str) -> u8 {
    match kind {
        "session" => 0,
        "weekly" => 1,
        "monthly" => 2,
        _ => 3,
    }
}
fn redacted_name(email: &str) -> String {
    let mut parts = email.split('@');
    let first = parts.next().and_then(|s| s.chars().next()).unwrap_or('?');
    let domain = parts.next().and_then(|s| s.chars().next()).unwrap_or('?');
    format!("{}{}", first.to_uppercase(), domain.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> ServerConfig {
        serde_json::from_value(json!({
            "providers": [
                { "instanceId": "codex-a", "driver": "codex", "displayName": "Work", "enabled": true, "installed": true,
                  "auth": { "email": "person@example.com" },
                  "usageLimits": { "checkedAt": "2026-10-02T09:00:00Z", "resetCredits": { "availableCount": 1 },
                    "windows": [{"id": "primary", "kind": "session", "label": "Session", "usedPercent": 10}] } },
                { "instanceId": "disabled", "driver": "codex", "enabled": false, "installed": true,
                  "usageLimits": { "checkedAt": "2026-10-02T09:00:00Z", "windows": [{"id": "primary", "kind": "session", "label": "Session", "usedPercent": 100}] } }
            ],
            "usageLimitSources": [{ "id": "hub", "label": "Proxy", "accounts": [
                { "id": "a.json", "driver": "codex", "email": "PERSON@EXAMPLE.COM", "plan": "Pro",
                  "usageLimits": { "checkedAt": "2026-10-02T10:00:00Z", "windows": [
                    {"id": "primary", "kind": "session", "label": "Session", "usedPercent": 47,
                     "resetsAt": "2026-10-02T11:00:00Z", "windowDurationMins": 300 },
                    {"id": "weekly", "kind": "weekly", "label": "Weekly", "usedPercent": 15},
                    {"id": "future-invalid"}
                  ] } },
                { "id": "b.json", "driver": "codex", "email": "other@example.com",
                  "usageLimits": { "checkedAt": "2026-10-02T10:00:00Z", "windows": [
                    {"id": "primary", "kind": "session", "label": "Session", "usedPercent": 52},
                    {"id": "weekly", "kind": "weekly", "label": "Weekly", "usedPercent": 8},
                    {"id": "primary", "kind": "monthly", "label": "Monthly", "usedPercent": 100}
                  ] } }
            ] }]
        })).unwrap()
    }

    #[test]
    fn native_and_source_accounts_deduplicate_and_pool_only_matching_windows() {
        let report = LimitsReport::from_config(&config());
        assert_eq!(report.accounts.len(), 2);
        let work = report.accounts.iter().find(|a| a.name == "Work").unwrap();
        assert_eq!(work.limits.windows.len(), 2); // Invalid future entry skipped.
        assert_eq!(
            work.limits.reset_credits.as_ref().unwrap().available_count,
            1
        );
        assert_eq!(work.plan.as_deref(), Some("Pro"));
        assert!(work.source.is_none());
        assert_eq!(report.accounts[1].name, "OE");
        let pools = report.pools();
        let pool = &pools[0];
        assert_eq!(
            pool.windows
                .iter()
                .map(|w| w.kind.as_str())
                .collect::<Vec<_>>(),
            ["session", "weekly", "monthly"]
        );
        assert_eq!(pool.remaining(&pool.windows[0]), 51.);
        assert_eq!(pool.remaining(&pool.windows[1]), 89.);
        assert_eq!(pool.remaining(&pool.windows[2]), 0.);
        // Only the timed account influences pace; the clockless one cannot skew it.
        assert_eq!(
            pool.pace(&pool.windows[0], "2026-10-02T10:00:00Z".parse().unwrap()),
            Some("Under pace")
        );
        assert_eq!(pool.accounts[0].name, "Work");
    }

    #[test]
    fn partial_events_keep_sources_and_removals_and_legacy_snapshots_clear_them() {
        let mut current = Some(config());
        apply_config_event(
            &mut current,
            json!({ "type": "providerStatuses", "payload": { "providers": [] } }),
        )
        .unwrap();
        assert!(current.as_ref().unwrap().providers.is_empty());
        assert_eq!(current.as_ref().unwrap().usage_limit_sources.len(), 1);
        apply_config_event(&mut current, json!({"type":"snapshot", "config": {
            "environment": { "environmentId": "test", "capabilities": { "usageLimitSources": true } }, "providers": []
        }})).unwrap();
        assert_eq!(current.as_ref().unwrap().usage_limit_sources.len(), 1);
        assert!(!apply_config_event(&mut current, json!({ "type": "futureEvent" })).unwrap());
        assert!(
            apply_config_event(
                &mut current,
                json!({"type":"providerStatuses", "payload":{"providers":42}})
            )
            .is_err()
        );
        assert_eq!(current.as_ref().unwrap().usage_limit_sources.len(), 1);
        apply_config_event(
            &mut current,
            json!({"type":"usageLimitSourcesUpdated", "payload":{"sources":[]}}),
        )
        .unwrap();
        assert!(current.as_ref().unwrap().usage_limit_sources.is_empty());
        current = Some(config());
        apply_config_event(
            &mut current,
            json!({"type":"snapshot", "config":{"providers":[]}}),
        )
        .unwrap();
        assert!(current.as_ref().unwrap().usage_limit_sources.is_empty());
    }

    #[test]
    fn unavailable_probes_and_external_usage_are_visible_without_inventing_quota() {
        let mut config = config();
        config.usage_limit_sources[0].error = Some("Offline proxy".into());
        let limits = config.providers[0].usage_limits.as_mut().unwrap();
        limits.unavailable = Some(QuotaUnavailable {
            reason: "probeFailed".into(),
            message: Some("Sign in again".into()),
        });
        limits.external_usage = Some(ExternalUsage {
            label: "View usage".into(),
            url: "https://example.com/usage".into(),
        });
        let report = LimitsReport::from_config(&config);
        assert_eq!(report.accounts.len(), 2); // Hub reports still render.
        assert!(report.notices.iter().any(|n| n.contains("Sign in again")));
        assert!(report.notices.iter().any(|n| n.contains("Offline proxy")));
        assert_eq!(report.external_links.len(), 1);
    }
}
