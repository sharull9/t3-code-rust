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
const REPLACED_KEYS: &[&str] =
    &["defaultModelSelection", "sourceControlWriterModelSelection", "providerInstances"];

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
        "sourceControlWritingStyle" => json!({
            "mode": "repo_conventions",
            "customInstructions": "",
            "followChangeRequestTemplates": true,
        }),
        "storageCleanup" => json!({
            "worktreeAfterDays": null,
            "worktreeOnMerge": false,
            "worktreeOnDelete": false,
            "worktreeUnchanged": false,
            "browserArtifactsAfterDays": null,
            "logsAfterDays": null,
        }),
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

#[derive(Debug, thiserror::Error)]
pub enum SettingsUpdateError {
    #[error(transparent)]
    Rpc(#[from] crate::RpcError),
    #[error("Provider instance \"{instance_id}\" was removed on the server. Your changes were not saved.")]
    ProviderRemoved { instance_id: String, settings: ServerSettings },
}

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
            None => default_value(key).is_some_and(|default| {
                // A stored object may omit fields; those read as the default.
                let mut current = default.clone();
                merge(&mut current, &self.value(key));
                current != default
            }),
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

    /// Rebuild whole-object replacements using only the changes made against
    /// this snapshot. Unedited fields come from `latest`, including removals
    /// made by another client while this snapshot was cached.
    pub fn rebase_patch(&self, patch: &Value, latest: &Self) -> Value {
        let mut patch = patch.clone();
        if let Some(entries) = patch.get_mut("projectSettingsOverrides").and_then(Value::as_object_mut) {
            for (project, entry) in entries {
                let before = self.0["projectSettingsOverrides"].get(project);
                let current = latest.0["projectSettingsOverrides"].get(project);
                let mut rebased = rebase_object(before, entry, current);
                if rebased.as_object().is_some_and(Map::is_empty) {
                    rebased = Value::Null;
                }
                *entry = rebased;
            }
        }
        if let Some(instances) = patch.get_mut("providerInstances") {
            let before = self.0.get("providerInstances");
            let current = latest.0.get("providerInstances");
            let mut rebased = rebase_object(before, instances, current);
            // Editing an existing instance must not recreate one deleted on
            // the server. Only IDs absent from the base are intended additions.
            if let Some(entries) = rebased.as_object_mut() {
                entries.retain(|id, _| {
                    before.and_then(|map| map.get(id)).is_none()
                        || current.and_then(|map| map.get(id)).is_some()
                });
            }
            *instances = rebased;
        }
        patch
    }

    /// Reject a save if rebasing would discard an edit to a removed provider.
    /// Cached, unedited entries and already-completed removals are harmless.
    /// The latest snapshot lets the UI report the conflict and show current data.
    pub fn rebase_patch_for_save(
        &self,
        patch: &Value,
        latest: &Self,
    ) -> Result<Value, SettingsUpdateError> {
        if let Some(instances) = patch.get("providerInstances").and_then(Value::as_object) {
            for (id, edited) in instances {
                let original = self.0.get("providerInstances").and_then(|map| map.get(id));
                let current = latest.0.get("providerInstances").and_then(|map| map.get(id));
                if original.is_some() && original != Some(edited) && current.is_none() {
                    return Err(SettingsUpdateError::ProviderRemoved {
                        instance_id: id.clone(),
                        settings: latest.clone(),
                    });
                }
            }
        }
        Ok(self.rebase_patch(patch, latest))
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

/// Apply the difference between two objects to the current object. Missing
/// fields mean deletion; explicit null remains a stored value. Nested objects
/// use the same rule so a provider edit does not resend cached configuration.
fn rebase_object(before: Option<&Value>, after: &Value, current: Option<&Value>) -> Value {
    let mut result = current.and_then(Value::as_object).cloned().unwrap_or_default();
    let before = before.and_then(Value::as_object);
    let after = after.as_object();
    if let Some(before) = before {
        for key in before.keys() {
            if !after.is_some_and(|after| after.contains_key(key)) {
                result.remove(key);
            }
        }
    }
    if let Some(after) = after {
        for (key, value) in after {
            let original = before.and_then(|before| before.get(key));
            if original == Some(value) {
                continue;
            }
            let updated = if value.is_object() && original.is_some_and(Value::is_object)
                && !REPLACED_KEYS.contains(&key.as_str()) {
                rebase_object(original, value, result.get(key))
            } else {
                value.clone()
            };
            result.insert(key.clone(), updated);
        }
    }
    Value::Object(result)
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

    /// Refresh before replacing project entries or the provider map. Callers
    /// must serialize edits so each refresh includes the previous save.
    pub async fn update_settings_from(&self, base: &ServerSettings, patch: Value) -> Result<ServerSettings, SettingsUpdateError> {
        let latest = self.get_settings().await?;
        let patch = base.rebase_patch_for_save(&patch, &latest)?;
        Ok(self.update_settings(patch).await?)
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

    #[test]
    fn rebasing_project_edits_preserves_remote_revocations_and_new_overrides() {
        let base = ServerSettings::from_value(json!({ "projectSettingsOverrides": {
            "p1": { "enableAgentDeviceAccess": true, "defaultRuntimeMode": "auto" }
        } }));
        let latest = ServerSettings::from_value(json!({ "projectSettingsOverrides": {
            "p1": { "defaultRuntimeMode": "approval-required", "defaultAutoPull": true }
        } }));
        let patch = base.set_patch("responseStreamingMode", Some(json!("turn")), Some("p1"));
        assert_eq!(base.rebase_patch(&patch, &latest), json!({ "projectSettingsOverrides": {
            "p1": { "defaultRuntimeMode": "approval-required", "defaultAutoPull": true,
                    "responseStreamingMode": "turn" }
        } }));
        // Clearing all cached overrides still preserves an override added remotely.
        let reset = base.project_patch("p1", &[("enableAgentDeviceAccess", None), ("defaultRuntimeMode", None)]);
        assert_eq!(base.rebase_patch(&reset, &latest), json!({ "projectSettingsOverrides": {
            "p1": { "defaultAutoPull": true }
        } }));
    }

    #[test]
    fn queued_edits_do_not_resend_a_failed_previous_edit() {
        let original = ServerSettings::default();
        let first = original.set_patch("defaultAutoPull", Some(json!(true)), Some("p1"));
        let mut optimistic = original.clone();
        optimistic.apply_patch(&first);
        let second = optimistic.set_patch("sidebarAutoSettleAfterDays", Some(Value::Null), Some("p1"));
        assert_eq!(optimistic.rebase_patch(&second, &original), json!({ "projectSettingsOverrides": {
            "p1": { "sidebarAutoSettleAfterDays": null }
        } }));
        assert_eq!(optimistic.rebase_patch(&second, &optimistic), second);
    }

    #[test]
    fn provider_edits_rebase_only_changed_configuration_and_instance_removals() {
        let base = ServerSettings::from_value(json!({ "providerInstances": {
            "work": { "driver": "codex", "enabled": true, "config": { "binaryPath": "old", "customModels": ["a"] } },
            "remove": { "driver": "claude" }
        } }));
        let latest = ServerSettings::from_value(json!({ "providerInstances": {
            "work": { "driver": "codex", "enabled": false, "config": { "binaryPath": "new", "customModels": ["a"] }, "future": 1 },
            "remove": { "driver": "claude" }, "remote": { "driver": "cursor" }
        } }));
        let mut patch = base.raw().clone();
        patch["providerInstances"]["work"]["config"]["customModels"] = json!(["b"]);
        patch["providerInstances"].as_object_mut().unwrap().remove("remove");
        assert_eq!(base.rebase_patch(&patch, &latest), json!({ "providerInstances": {
            "work": { "driver": "codex", "enabled": false, "config": { "binaryPath": "new", "customModels": ["b"] }, "future": 1 },
            "remote": { "driver": "cursor" }
        } }));
    }

    #[test]
    fn provider_edits_do_not_recreate_instances_removed_remotely() {
        let base = ServerSettings::from_value(json!({ "providerInstances": {
            "removed": { "driver": "codex", "enabled": true, "config": { "binaryPath": "codex" } },
            "kept": { "driver": "claude", "enabled": true }
        } }));
        let mut patch = base.raw().clone();
        patch["providerInstances"]["removed"]["enabled"] = json!(false);
        patch["providerInstances"]["kept"]["enabled"] = json!(false);
        patch["providerInstances"]["new"] = json!({ "driver": "cursor", "enabled": true });
        for latest in [json!({}), json!({ "providerInstances": {} }), json!({ "providerInstances": {
            "kept": { "driver": "claude", "enabled": true, "future": 1 },
            "remote": { "driver": "grok", "enabled": true }
        } })] {
            let latest = ServerSettings::from_value(latest);
            let rebased = base.rebase_patch(&patch, &latest);
            let instances = &rebased["providerInstances"];
            assert!(instances.get("removed").is_none());
            assert_eq!(instances["new"], json!({ "driver": "cursor", "enabled": true }));
            if latest.raw()["providerInstances"].get("kept").is_some() {
                assert_eq!(instances["kept"], json!({ "driver": "claude", "enabled": false, "future": 1 }));
                assert_eq!(instances["remote"], latest.raw()["providerInstances"]["remote"]);
            } else {
                assert!(instances.get("kept").is_none());
            }
        }
    }

    #[test]
    fn provider_save_conflicts_only_when_an_edit_would_be_discarded() {
        let base = ServerSettings::from_value(json!({ "providerInstances": {
            "removed": { "driver": "codex", "enabled": true },
            "kept": { "driver": "claude", "enabled": true }
        } }));
        let latest = ServerSettings::from_value(json!({ "providerInstances": {
            "kept": { "driver": "claude", "enabled": true }
        } }));
        let mut patch = base.raw().clone();
        // An unchanged cached entry does not conflict with its remote removal.
        patch["providerInstances"]["kept"]["enabled"] = json!(false);
        assert!(base.rebase_patch_for_save(&patch, &latest).is_ok());
        patch["providerInstances"]["removed"]["enabled"] = json!(false);
        let error = base.rebase_patch_for_save(&patch, &latest).unwrap_err();
        assert!(error.to_string().contains("Your changes were not saved"));
        assert!(matches!(error, SettingsUpdateError::ProviderRemoved { instance_id, settings }
            if instance_id == "removed" && settings == latest));
        // Removing an already-removed entry is idempotent; new IDs are additions.
        patch["providerInstances"].as_object_mut().unwrap().remove("removed");
        patch["providerInstances"]["new"] = json!({ "driver": "cursor", "enabled": true });
        assert_eq!(base.rebase_patch_for_save(&patch, &latest).unwrap(), patch);
    }

    #[tokio::test]
    async fn provider_conflict_returns_latest_settings_without_sending_a_write() {
        use futures::{SinkExt as _, StreamExt as _};
        use std::time::Duration;
        use tokio::net::TcpListener;
        use tokio_tungstenite::{accept_async, tungstenite::Message};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            accept_async(socket).await.unwrap()
        });
        let connection = crate::Connection {
            rpc: crate::rpc::RpcSession::connect(&format!("ws://{address}")).await.unwrap(),
        };
        let mut server = accept.await.unwrap();
        let base = ServerSettings::from_value(json!({ "providerInstances": {
            "work": { "driver": "codex", "enabled": true }
        } }));
        let mut patch = base.raw().clone();
        patch["providerInstances"]["work"]["enabled"] = json!(false);
        let caller = connection.clone();
        let save = tokio::spawn(async move { caller.update_settings_from(&base, patch).await });
        let frame = tokio::time::timeout(Duration::from_secs(2), server.next()).await.unwrap().unwrap().unwrap();
        let Message::Text(text) = frame else { panic!("expected settings request") };
        let request: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(request["tag"], "server.getSettings");
        let latest = json!({ "providerInstances": {}, "defaultAutoPull": true });
        server.send(Message::Text(json!({
            "_tag": "Exit", "requestId": request["id"],
            "exit": { "_tag": "Success", "value": latest }
        }).to_string().into())).await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(2), save).await.unwrap().unwrap().unwrap_err();
        assert!(matches!(error, SettingsUpdateError::ProviderRemoved { instance_id, settings }
            if instance_id == "work" && settings.raw() == &latest));
        assert!(tokio::time::timeout(Duration::from_millis(100), server.next()).await.is_err(),
            "a conflicted save must not send server.updateSettings");
    }
}
