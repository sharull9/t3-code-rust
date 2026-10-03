//! Server settings (`server.getSettings` / `server.updateSettings`).
//!
//! The schema is large and grows between server releases, so [`ServerSettings`]
//! keeps the raw JSON object and offers typed reads for the fields this client
//! edits. Unknown fields survive a round trip, and a patch only ever names the
//! keys the user changed. Defaults and the project-scoped key list mirror
//! `packages/contracts/src/settings.ts` upstream.

use serde_json::{Map, Value, json};

/// Keys a project may override (`PROJECT_SCOPED_SERVER_SETTING_KEYS`).
pub const PROJECT_SCOPED_KEYS: &[&str] = &[
    "worktreeCleanup",
    "defaultModelSelection",
    "defaultRuntimeMode",
    "defaultThreadEnvMode",
    "newWorktreesStartFromOrigin",
    "worktreeSubmodules",
    "defaultAutoPull",
    "defaultProjectScripts",
    "enableAgentBrowserAccess",
    "enableAgentDeviceAccess",
    "textGenerationModelSelection",
    "sourceControlWriterModelSelection",
    "sourceControlWritingStyle",
    "pullRequestMergeMethod",
    "sidebarAutoSettleOnMerge",
    "sidebarAutoSettleAfterDays",
    "continueThreadsAfterServerUpdate",
    "responseStreamingMode",
];

/// Project-scoped keys whose environment type is nullable, so `null` is a real
/// stored override (`isNullableProjectSettingsOverride`).
const NULLABLE_PROJECT_KEYS: &[&str] = &[
    "defaultModelSelection",
    "sourceControlWriterModelSelection",
    "pullRequestMergeMethod",
    "sidebarAutoSettleAfterDays",
];

/// Patch keys whose value replaces the stored one instead of merging into it.
const REPLACED_KEYS: &[&str] = &["defaultModelSelection", "sourceControlWriterModelSelection"];

pub fn is_project_scoped(key: &str) -> bool {
    PROJECT_SCOPED_KEYS.contains(&key)
}

pub fn is_nullable_project_override(key: &str) -> bool {
    NULLABLE_PROJECT_KEYS.contains(&key)
}

/// The decoding default for a key; `None` when this client does not know one.
pub fn default_value(key: &str) -> Option<Value> {
    Some(match key {
        "defaultModelSelection"
        | "defaultThreadEnvMode"
        | "worktreeSubmodules"
        | "sourceControlWriterModelSelection"
        | "pullRequestMergeMethod"
        | "worktreeCleanup" => Value::Null,
        "defaultRuntimeMode" => json!("full-access"),
        "responseStreamingMode" => json!("paragraph"),
        "sidebarAutoSettleAfterDays" => json!(3),
        "sidebarAutoSettleOnMerge" | "newWorktreesStartFromOrigin" | "enableProviderUpdateChecks"
        | "enableAgentBrowserAccess" => json!(true),
        "continueThreadsAfterServerUpdate" | "defaultAutoPull" | "enableAgentDeviceAccess" => {
            json!(false)
        }
        _ => return None,
    })
}

/// A key's value at some scope and whether a project override supplies it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedValue {
    pub value: Value,
    pub overridden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerSettings(Value);

impl Default for ServerSettings {
    fn default() -> Self {
        Self(Value::Object(Map::new()))
    }
}

impl ServerSettings {
    /// A non-object (a malformed reply) reads as empty so every key falls back
    /// to its default.
    pub fn from_value(value: Value) -> Self {
        if value.is_object() { Self(value) } else { Self::default() }
    }

    pub fn raw(&self) -> &Value {
        &self.0
    }

    /// The environment-level value, with the schema default for an absent key.
    pub fn value(&self, key: &str) -> Value {
        self.0.get(key).cloned().or_else(|| default_value(key)).unwrap_or(Value::Null)
    }

    pub fn project_overrides(&self, project_id: &str) -> Option<&Map<String, Value>> {
        self.0.get("projectSettingsOverrides")?.get(project_id)?.as_object()
    }

    /// `key` as seen from `project` (`None` is the environment). An absent
    /// override inherits the environment value.
    pub fn scoped(&self, key: &str, project: Option<&str>) -> ScopedValue {
        if let Some(project) = project.filter(|_| is_project_scoped(key))
            && let Some(value) = self.project_overrides(project).and_then(|entry| entry.get(key))
        {
            return ScopedValue { value: value.clone(), overridden: true };
        }
        ScopedValue { value: self.value(key), overridden: false }
    }

    /// Whether `key` differs from what "Restore defaults" would leave: the
    /// built-in default at the environment, any override at a project.
    pub fn is_modified(&self, key: &str, project: Option<&str>) -> bool {
        match project {
            Some(project) if is_project_scoped(key) => {
                self.project_overrides(project).is_some_and(|entry| entry.contains_key(key))
            }
            Some(_) => false,
            None => default_value(key).is_some_and(|default| self.value(key) != default),
        }
    }

    // Typed reads for the fields the General page edits.

    pub fn default_model_selection(&self) -> Option<Value> {
        Some(self.value("defaultModelSelection")).filter(|value| !value.is_null())
    }
    pub fn default_runtime_mode(&self) -> String {
        self.str_value("defaultRuntimeMode")
    }
    pub fn default_thread_env_mode(&self) -> Option<String> {
        self.value("defaultThreadEnvMode").as_str().map(str::to_owned)
    }
    pub fn worktree_submodules(&self) -> Option<String> {
        self.value("worktreeSubmodules").as_str().map(str::to_owned)
    }
    pub fn new_worktrees_start_from_origin(&self) -> bool {
        self.value("newWorktreesStartFromOrigin").as_bool().unwrap_or(true)
    }
    pub fn sidebar_auto_settle_on_merge(&self) -> bool {
        self.value("sidebarAutoSettleOnMerge").as_bool().unwrap_or(true)
    }
    /// `None` means never auto-settle.
    pub fn sidebar_auto_settle_after_days(&self) -> Option<u32> {
        self.value("sidebarAutoSettleAfterDays").as_u64().map(|days| days as u32)
    }
    pub fn response_streaming_mode(&self) -> String {
        self.str_value("responseStreamingMode")
    }
    pub fn continue_threads_after_server_update(&self) -> bool {
        self.value("continueThreadsAfterServerUpdate").as_bool().unwrap_or(false)
    }
    fn str_value(&self, key: &str) -> String {
        self.value(key).as_str().unwrap_or_default().to_owned()
    }

    /// A patch changing environment keys. Absent keys are left alone.
    pub fn environment_patch(edits: &[(&str, Value)]) -> Value {
        Value::Object(edits.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect())
    }

    /// A patch writing project overrides. Each `Some` value sets an override,
    /// each `None` removes it. `null` for a key that is not nullable also
    /// removes it, so an "Inherit" menu item and the row's reset agree. The
    /// server replaces a project's whole entry, so the patch resends the
    /// current entry with the edits applied; an empty entry is `null`, which
    /// removes it.
    pub fn project_patch(&self, project_id: &str, edits: &[(&str, Option<Value>)]) -> Value {
        let mut entry = self.project_overrides(project_id).cloned().unwrap_or_default();
        for (key, value) in edits {
            match value {
                Some(Value::Null) if !is_nullable_project_override(key) => entry.remove(*key),
                Some(value) => entry.insert((*key).to_owned(), value.clone()),
                None => entry.remove(*key),
            };
        }
        let entry = if entry.is_empty() { Value::Null } else { Value::Object(entry) };
        json!({ "projectSettingsOverrides": { project_id: entry } })
    }

    /// The patch for one key at a scope: an environment write, or a project
    /// override (`None` clears it).
    pub fn set_patch(&self, key: &str, value: Option<Value>, project: Option<&str>) -> Value {
        match project.filter(|_| is_project_scoped(key)) {
            Some(project) => self.project_patch(project, &[(key, value)]),
            None => Self::environment_patch(&[(key, value.unwrap_or(Value::Null))]),
        }
    }

    /// One patch that puts every modified key of `keys` back to its default at
    /// the scope (clearing overrides at a project). `None` when nothing differs.
    pub fn reset_patch(&self, keys: &[&str], project: Option<&str>) -> Option<Value> {
        let modified: Vec<&str> =
            keys.iter().copied().filter(|key| self.is_modified(key, project)).collect();
        if modified.is_empty() {
            return None;
        }
        Some(match project {
            Some(project) => {
                let edits: Vec<(&str, Option<Value>)> =
                    modified.into_iter().map(|key| (key, None)).collect();
                self.project_patch(project, &edits)
            }
            None => {
                let edits: Vec<(&str, Value)> = modified
                    .into_iter()
                    .filter_map(|key| default_value(key).map(|default| (key, default)))
                    .collect();
                Self::environment_patch(&edits)
            }
        })
    }

    /// Applies a patch the way the server does, for showing a pending save
    /// before the server answers. Objects merge key by key except the model
    /// selections, which replace; a project's entry replaces whole and `null`
    /// removes it.
    pub fn apply_patch(&mut self, patch: &Value) {
        let Some(patch) = patch.as_object() else { return };
        let Some(settings) = self.0.as_object_mut() else { return };
        for (key, value) in patch {
            if key == "projectSettingsOverrides" {
                let overrides = settings
                    .entry(key.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let (Some(overrides), Some(entries)) =
                    (overrides.as_object_mut(), value.as_object())
                {
                    for (project, entry) in entries {
                        if entry.is_null() {
                            overrides.remove(project);
                        } else {
                            overrides.insert(project.clone(), entry.clone());
                        }
                    }
                }
            } else if REPLACED_KEYS.contains(&key.as_str()) {
                settings.insert(key.clone(), value.clone());
            } else {
                merge(settings.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
    }
}

fn merge(target: &mut Value, patch: &Value) {
    match (target.as_object_mut(), patch.as_object()) {
        (Some(target), Some(patch)) => {
            for (key, value) in patch {
                merge(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        _ => *target = patch.clone(),
    }
}

impl crate::Connection {
    pub async fn get_settings(&self) -> Result<ServerSettings, crate::RpcError> {
        let value = self.rpc().call::<Value>("server.getSettings", json!({})).await?;
        Ok(ServerSettings::from_value(value))
    }

    /// Sends a patch (see [`ServerSettings::set_patch`]) and returns the
    /// settings the server stored.
    pub async fn update_settings(&self, patch: Value) -> Result<ServerSettings, crate::RpcError> {
        let value = self.rpc().call::<Value>("server.updateSettings", json!({ "patch": patch })).await?;
        Ok(ServerSettings::from_value(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ServerSettings {
        ServerSettings::from_value(json!({
            "defaultRuntimeMode": "approval-required",
            "sidebarAutoSettleAfterDays": null,
            "somethingNew": { "kept": true },
            "projectSettingsOverrides": {
                "p1": { "defaultRuntimeMode": "auto", "defaultModelSelection": null }
            }
        }))
    }

    #[test]
    fn absent_keys_decode_to_schema_defaults_and_unknown_fields_survive() {
        let settings = settings();
        assert_eq!(settings.default_runtime_mode(), "approval-required");
        assert_eq!(settings.sidebar_auto_settle_after_days(), None);
        assert!(settings.sidebar_auto_settle_on_merge());
        assert_eq!(settings.response_streaming_mode(), "paragraph");
        assert_eq!(settings.raw()["somethingNew"]["kept"], true);
        assert_eq!(ServerSettings::from_value(json!(42)).default_runtime_mode(), "full-access");
        assert_eq!(ServerSettings::default().sidebar_auto_settle_after_days(), Some(3));
    }

    #[test]
    fn scoped_reads_inherit_unless_a_project_overrides() {
        let settings = settings();
        let own = settings.scoped("defaultRuntimeMode", Some("p1"));
        assert_eq!((own.value, own.overridden), (json!("auto"), true));
        let inherited = settings.scoped("newWorktreesStartFromOrigin", Some("p1"));
        assert_eq!((inherited.value, inherited.overridden), (json!(true), false));
        assert!(settings.scoped("defaultModelSelection", Some("p1")).overridden);
        assert!(!settings.scoped("defaultRuntimeMode", Some("p2")).overridden);
        assert!(settings.is_modified("defaultRuntimeMode", None));
        assert!(!settings.is_modified("responseStreamingMode", None));
        assert!(settings.is_modified("defaultRuntimeMode", Some("p1")));
        assert!(!settings.is_modified("responseStreamingMode", Some("p1")));
    }

    #[test]
    fn environment_and_project_patches_have_contract_shape() {
        let settings = settings();
        assert_eq!(
            settings.set_patch("responseStreamingMode", Some(json!("turn")), None),
            json!({ "responseStreamingMode": "turn" })
        );
        // The project's whole entry is resent with the edit applied.
        assert_eq!(
            settings.set_patch("responseStreamingMode", Some(json!("turn")), Some("p1")),
            json!({ "projectSettingsOverrides": { "p1": {
                "defaultRuntimeMode": "auto", "defaultModelSelection": null,
                "responseStreamingMode": "turn"
            } } })
        );
        // Not project scoped: always an environment write.
        assert_eq!(
            settings.set_patch("addProjectBaseDirectory", Some(json!("~/")), Some("p1")),
            json!({ "addProjectBaseDirectory": "~/" })
        );
    }

    #[test]
    fn null_clears_non_nullable_overrides_but_is_stored_for_nullable_ones() {
        let settings = settings();
        let cleared = settings.project_patch("p1", &[("defaultRuntimeMode", Some(Value::Null))]);
        assert_eq!(
            cleared,
            json!({ "projectSettingsOverrides": { "p1": { "defaultModelSelection": null } } })
        );
        let stored = settings.project_patch("p1", &[("sidebarAutoSettleAfterDays", Some(Value::Null))]);
        assert_eq!(stored["projectSettingsOverrides"]["p1"]["sidebarAutoSettleAfterDays"], Value::Null);
        assert!(stored["projectSettingsOverrides"]["p1"].get("sidebarAutoSettleAfterDays").is_some());
        // Removing the last override removes the entry.
        let last = ServerSettings::from_value(json!({
            "projectSettingsOverrides": { "p2": { "defaultRuntimeMode": "auto" } }
        }));
        assert_eq!(
            last.project_patch("p2", &[("defaultRuntimeMode", None)]),
            json!({ "projectSettingsOverrides": { "p2": null } })
        );
    }

    #[test]
    fn reset_patch_restores_environment_defaults_or_clears_overrides() {
        let settings = settings();
        let keys = ["defaultRuntimeMode", "sidebarAutoSettleAfterDays", "responseStreamingMode"];
        assert_eq!(
            settings.reset_patch(&keys, None),
            Some(json!({ "defaultRuntimeMode": "full-access", "sidebarAutoSettleAfterDays": 3 }))
        );
        assert_eq!(
            settings.reset_patch(&keys, Some("p1")),
            Some(json!({ "projectSettingsOverrides": { "p1": { "defaultModelSelection": null } } }))
        );
        assert_eq!(settings.reset_patch(&["responseStreamingMode"], None), None);
        assert_eq!(settings.reset_patch(&["responseStreamingMode"], Some("p1")), None);
    }

    #[test]
    fn applying_a_patch_matches_server_merge_rules() {
        let mut settings = settings();
        settings.apply_patch(&json!({
            "defaultRuntimeMode": "full-access",
            "defaultModelSelection": { "instanceId": "codex", "model": "m" },
            "projectSettingsOverrides": { "p1": null, "p3": { "defaultAutoPull": true } },
        }));
        assert_eq!(settings.default_runtime_mode(), "full-access");
        assert_eq!(settings.default_model_selection().unwrap()["model"], "m");
        assert!(settings.project_overrides("p1").is_none());
        assert!(settings.project_overrides("p3").is_some());
        assert_eq!(settings.raw()["somethingNew"]["kept"], true);
        settings.apply_patch(&json!({ "storageCleanup": { "logsAfterDays": 7 } }));
        settings.apply_patch(&json!({ "storageCleanup": { "worktreeOnMerge": true } }));
        assert_eq!(settings.raw()["storageCleanup"], json!({ "logsAfterDays": 7, "worktreeOnMerge": true }));
    }
}
