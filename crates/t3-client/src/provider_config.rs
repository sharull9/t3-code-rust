//! Provider instance configuration in server settings.
//!
//! Instances live in `providerInstances` (id to an envelope of `driver`,
//! `enabled`, `displayName`, `accentColor`, `environment` and a driver-specific
//! `config` blob). The built-in "default slot" of each driver (instance id =
//! driver) can still be stored only in the legacy `providers.<driver>` object;
//! such a slot is shown as a synthesized envelope and, once edited, written to
//! `providerInstances` (and the legacy object reset), as upstream's web UI does.
//!
//! `providerInstances` is a whole-map replacement on the server: every write
//! resends the full map. Edits therefore start from the stored envelope and
//! change only the touched key, so unknown fields (a newer server's, a fork's
//! driver) survive. Driver field lists mirror `packages/contracts/src/settings.ts`.

use serde_json::{Map, Value, json};

use crate::ServerSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Text,
    Password,
    Switch,
    /// `(stored value, label)`; the first entry is the default and is stored
    /// as an omitted key.
    Select(&'static [(&'static str, &'static str)]),
}

/// One field of a driver's `config` blob that the form shows.
#[derive(Debug)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub placeholder: &'static str,
    pub control: Control,
    /// `clearWhenEmpty: "persist"`: an empty value is stored, not omitted.
    pub persist_empty: bool,
    /// The legacy schema's decoded default (for resetting a default slot).
    pub default: &'static str,
}

#[derive(Debug)]
pub struct Driver {
    pub kind: &'static str,
    pub label: &'static str,
    pub badge: Option<&'static str>,
    pub default_enabled: bool,
    pub fields: &'static [Field],
}

impl Driver {
    /// Antigravity has no custom model list.
    pub fn has_custom_models(&self) -> bool {
        self.kind != "antigravity"
    }
}

const fn text(
    key: &'static str,
    label: &'static str,
    description: &'static str,
    placeholder: &'static str,
    default: &'static str,
) -> Field {
    Field {
        key,
        label,
        description,
        placeholder,
        control: Control::Text,
        persist_empty: false,
        default,
    }
}

const ANTIGRAVITY_AUTH: &[(&str, &str)] = &[
    ("oauth-personal", "Google account"),
    ("oauth-business", "Gemini Enterprise"),
    ("gemini-api-key", "Gemini API key"),
    ("agent-platform", "Agent Platform (Vertex AI)"),
];

pub const DRIVERS: &[Driver] = &[
    Driver {
        kind: "codex",
        label: "Codex",
        badge: None,
        default_enabled: true,
        fields: &[
            text("binaryPath", "Binary path", "Path to the Codex binary used by this instance.", "codex", "codex"),
            text("homePath", "CODEX_HOME path", "Custom Codex home and config directory.", "~/.codex", ""),
            text(
                "shadowHomePath",
                "Shadow home path",
                "Account-specific Codex home. Keeps auth.json separate while sharing state from CODEX_HOME.",
                "~/.codex-t3/personal",
                "",
            ),
            text(
                "launchArgs",
                "Launch arguments",
                "Additional CLI arguments passed to codex app-server on session start.",
                "",
                "",
            ),
        ],
    },
    Driver {
        kind: "claudeAgent",
        label: "Claude",
        badge: None,
        default_enabled: true,
        fields: &[
            text("binaryPath", "Binary path", "Path to the Claude binary used by this instance.", "claude", "claude"),
            text(
                "homePath",
                "CLAUDE_CONFIG_DIR path",
                "Custom Claude home and config directory. Keeps .claude.json and .claude separate.",
                "~/.claude",
                "",
            ),
            text(
                "autoCompactWindow",
                "Auto-compact after",
                "Compact after 100,000 to 1,000,000 tokens. Leave empty to use Claude's default.",
                "e.g. 300000",
                "",
            ),
            text(
                "launchArgs",
                "Launch arguments",
                "Additional CLI arguments passed on session start.",
                "e.g. --chrome",
                "",
            ),
        ],
    },
    Driver {
        kind: "cursor",
        label: "Cursor",
        badge: Some("Early Access"),
        default_enabled: false,
        fields: &[
            text("binaryPath", "Binary path", "Path to the Cursor agent binary.", "cursor-agent", "cursor-agent"),
            text(
                "apiEndpoint",
                "API endpoint",
                "Override the Cursor API endpoint for this instance.",
                "https://...",
                "",
            ),
        ],
    },
    Driver {
        kind: "grok",
        label: "Grok",
        badge: Some("Early Access"),
        default_enabled: false,
        fields: &[text("binaryPath", "Binary path", "Path to the Grok CLI binary.", "grok", "grok")],
    },
    Driver {
        kind: "opencode",
        label: "OpenCode",
        badge: None,
        default_enabled: false,
        fields: &[
            text("binaryPath", "Binary path", "Path to the OpenCode binary.", "opencode", "opencode"),
            text(
                "serverUrl",
                "Server URL",
                "Leave blank to let T3 Code spawn the server when needed.",
                "http://127.0.0.1:4096",
                "",
            ),
            Field {
                key: "serverPassword",
                label: "Server password",
                description: "Stored in plain text on disk.",
                placeholder: "Optional",
                control: Control::Password,
                persist_empty: false,
                default: "",
            },
        ],
    },
    Driver {
        kind: "antigravity",
        label: "Antigravity",
        badge: None,
        default_enabled: false,
        fields: &[
            Field {
                key: "authMethod",
                label: "Sign-in method",
                description: "Google accounts use your subscription; API keys and Agent Platform bill usage.",
                placeholder: "",
                control: Control::Select(ANTIGRAVITY_AUTH),
                persist_empty: false,
                default: "oauth-personal",
            },
            Field {
                key: "apiKey",
                label: "API key",
                description: "Gemini or Vertex AI express key. Stored in plain text.",
                placeholder: "Optional",
                control: Control::Password,
                persist_empty: false,
                default: "",
            },
            text(
                "gcpProject",
                "GCP project",
                "Required for Gemini Enterprise. Agent Platform uses it when no API key is set.",
                "my-project-id",
                "",
            ),
            text(
                "gcpLocation",
                "GCP location",
                "Region for Gemini Enterprise or Agent Platform.",
                "us-central1",
                "",
            ),
            Field {
                key: "binaryPath",
                label: "Binary path",
                description: "Custom ACP executable. Leave empty to select automatically.",
                placeholder: "Automatic",
                control: Control::Text,
                persist_empty: true,
                default: "",
            },
        ],
    },
];

pub fn driver(kind: &str) -> Option<&'static Driver> {
    DRIVERS.iter().find(|driver| driver.kind == kind)
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(String),
    Bool(bool),
}

/// A configured instance: its id, driver, whether it is its driver's default
/// slot and the envelope as stored (or synthesized from the legacy object).
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceConfig {
    pub id: String,
    pub driver: String,
    pub is_default: bool,
    /// Synthesized from `providers.<driver>`; nothing is stored in
    /// `providerInstances` for it yet.
    pub legacy: bool,
    pub envelope: Value,
}

impl InstanceConfig {
    fn config(&self) -> Option<&Map<String, Value>> {
        self.envelope.get("config")?.as_object()
    }

    /// An explicit `false` on the envelope or in the config wins; otherwise
    /// the envelope, then the config, then the driver's default.
    pub fn enabled(&self) -> bool {
        let envelope = self.envelope.get("enabled").and_then(Value::as_bool);
        let config = self.config().and_then(|config| config.get("enabled")).and_then(Value::as_bool);
        if envelope == Some(false) || config == Some(false) {
            return false;
        }
        envelope
            .or(config)
            .unwrap_or_else(|| driver(&self.driver).is_none_or(|driver| driver.default_enabled))
    }

    pub fn display_name(&self) -> Option<&str> {
        self.envelope.get("displayName")?.as_str().map(str::trim).filter(|name| !name.is_empty())
    }

    pub fn text_value(&self, key: &str) -> String {
        self.config()
            .and_then(|config| config.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    pub fn bool_value(&self, key: &str) -> bool {
        self.config().and_then(|config| config.get(key)).and_then(Value::as_bool).unwrap_or(false)
    }

    /// Slugs of the custom models (entries are a slug string or an object).
    pub fn custom_models(&self) -> Vec<String> {
        self.config()
            .and_then(|config| config.get("customModels"))
            .and_then(Value::as_array)
            .map(|models| models.iter().filter_map(custom_model_slug).map(str::to_owned).collect())
            .unwrap_or_default()
    }
}

fn custom_model_slug(entry: &Value) -> Option<&str> {
    match entry {
        Value::String(slug) => Some(slug.as_str()),
        other => other.get("slug").and_then(Value::as_str),
    }
    .map(str::trim)
    .filter(|slug| !slug.is_empty())
}

fn config_mut(envelope: &mut Value) -> &mut Map<String, Value> {
    let object = envelope.as_object_mut().expect("an instance envelope is an object");
    let config = object.entry("config").or_insert_with(|| Value::Object(Map::new()));
    if !config.is_object() {
        *config = Value::Object(Map::new());
    }
    config.as_object_mut().expect("just made an object")
}

/// A config left empty is dropped, like the `undefined` upstream stores.
fn drop_empty_config(envelope: &mut Value) {
    if let Some(object) = envelope.as_object_mut()
        && object.get("config").and_then(Value::as_object).is_some_and(Map::is_empty)
    {
        object.remove("config");
    }
}

pub fn set_enabled(envelope: &mut Value, enabled: bool) {
    if let Some(object) = envelope.as_object_mut() {
        object.insert("enabled".to_owned(), Value::Bool(enabled));
    }
}

/// Blank clears the name.
pub fn set_display_name(envelope: &mut Value, name: &str) {
    let Some(object) = envelope.as_object_mut() else { return };
    let name = name.trim();
    if name.is_empty() {
        object.remove("displayName");
    } else {
        object.insert("displayName".to_owned(), json!(name));
    }
}

/// Writes one form field: a blank text value (or a boolean equal to its
/// default) is omitted unless the field persists empties. Other config keys
/// are untouched.
pub fn set_field(envelope: &mut Value, field: &Field, value: &FieldValue) {
    let config = config_mut(envelope);
    match value {
        FieldValue::Bool(value) => {
            if !field.persist_empty && !*value {
                config.remove(field.key);
            } else {
                config.insert(field.key.to_owned(), Value::Bool(*value));
            }
        }
        FieldValue::Text(value) => {
            if !field.persist_empty && value.trim().is_empty() {
                config.remove(field.key);
            } else {
                config.insert(field.key.to_owned(), Value::String(value.clone()));
            }
        }
    }
    drop_empty_config(envelope);
}

/// Adds a custom model (a bare slug string, as upstream stores one without a
/// name or capabilities). `false` when blank or already present.
pub fn add_custom_model(envelope: &mut Value, slug: &str) -> bool {
    let slug = slug.trim();
    if slug.is_empty() {
        return false;
    }
    let config = config_mut(envelope);
    let models = config.entry("customModels").or_insert_with(|| json!([]));
    if !models.is_array() {
        *models = json!([]);
    }
    let models = models.as_array_mut().expect("just made an array");
    if models.iter().any(|entry| custom_model_slug(entry) == Some(slug)) {
        return false;
    }
    models.push(json!(slug));
    true
}

/// Removes a custom model, keeping the other entries (with their names and
/// capabilities) as stored. The empty list stays: it records a deliberate clear.
pub fn remove_custom_model(envelope: &mut Value, slug: &str) {
    let config = config_mut(envelope);
    if let Some(models) = config.get_mut("customModels").and_then(Value::as_array_mut) {
        models.retain(|entry| custom_model_slug(entry) != Some(slug));
    }
}

/// The legacy `providers.<driver>` values a default slot is reset to after its
/// config moves to `providerInstances`.
fn legacy_defaults(driver: &Driver) -> Value {
    let mut object = Map::new();
    object.insert("enabled".to_owned(), Value::Bool(driver.default_enabled));
    for field in driver.fields {
        match field.control {
            Control::Switch => object.insert(field.key.to_owned(), Value::Bool(false)),
            _ => object.insert(field.key.to_owned(), Value::String(field.default.to_owned())),
        };
    }
    object.insert("customModels".to_owned(), json!([]));
    Value::Object(object)
}

/// Checks an instance id against the server's slug rules.
pub fn validate_instance_id(id: &str, existing: &[InstanceConfig]) -> Result<(), String> {
    if id.is_empty() {
        return Err("Instance ID is required.".to_owned());
    }
    if id.chars().count() > 64 {
        return Err("Instance ID must be 64 characters or fewer.".to_owned());
    }
    let mut chars = id.chars();
    let valid = chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(
            "Instance ID must start with a letter and use only letters, digits, '-', or '_'."
                .to_owned(),
        );
    }
    if existing.iter().any(|instance| instance.id == id) {
        return Err(format!("An instance named '{id}' already exists."));
    }
    Ok(())
}

/// `{driver}_{label slug}`, or empty for a blank label.
pub fn derive_instance_id(driver: &str, label: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;
    for c in label.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending_separator && !slug.is_empty() {
                slug.push('_');
            }
            pending_separator = false;
            slug.push(c);
        } else {
            pending_separator = true;
        }
    }
    slug.truncate(48);
    if slug.is_empty() { String::new() } else { format!("{driver}_{slug}") }
}

impl ServerSettings {
    fn explicit_instances(&self) -> Map<String, Value> {
        self.raw().get("providerInstances").and_then(Value::as_object).cloned().unwrap_or_default()
    }

    /// Every configured instance: each known driver's default slot (explicit,
    /// or synthesized from the legacy object), then the other explicit ones.
    pub fn provider_instances(&self) -> Vec<InstanceConfig> {
        let explicit = self.explicit_instances();
        let legacy = self.raw().get("providers").and_then(Value::as_object);
        let mut instances = Vec::new();
        for driver in DRIVERS {
            if let Some(envelope) = explicit.get(driver.kind).filter(|value| value.is_object()) {
                instances.push(InstanceConfig {
                    id: driver.kind.to_owned(),
                    driver: envelope
                        .get("driver")
                        .and_then(Value::as_str)
                        .unwrap_or(driver.kind)
                        .to_owned(),
                    is_default: true,
                    legacy: false,
                    envelope: envelope.clone(),
                });
            } else if let Some(blob) = legacy.and_then(|legacy| legacy.get(driver.kind))
                && let Some(blob) = blob.as_object()
            {
                let mut config = blob.clone();
                let enabled = config.remove("enabled");
                let mut envelope = Map::new();
                envelope.insert("driver".to_owned(), json!(driver.kind));
                if let Some(enabled) = enabled.filter(Value::is_boolean) {
                    envelope.insert("enabled".to_owned(), enabled);
                }
                envelope.insert("config".to_owned(), Value::Object(config));
                instances.push(InstanceConfig {
                    id: driver.kind.to_owned(),
                    driver: driver.kind.to_owned(),
                    is_default: true,
                    legacy: true,
                    envelope: Value::Object(envelope),
                });
            }
        }
        for (id, envelope) in &explicit {
            if instances.iter().any(|instance| &instance.id == id) {
                continue;
            }
            let Some(driver) = envelope.get("driver").and_then(Value::as_str) else { continue };
            instances.push(InstanceConfig {
                id: id.clone(),
                driver: driver.to_owned(),
                is_default: false,
                legacy: false,
                envelope: envelope.clone(),
            });
        }
        instances
    }

    pub fn provider_instance(&self, id: &str) -> Option<InstanceConfig> {
        self.provider_instances().into_iter().find(|instance| instance.id == id)
    }

    /// The patch storing `envelope` for `instance`: the full `providerInstances`
    /// map with that entry replaced, plus, for a default slot still held in the
    /// legacy object, that object reset (the envelope now owns the config).
    pub fn instance_update_patch(&self, instance: &InstanceConfig, envelope: Value) -> Value {
        let mut map = self.explicit_instances();
        map.insert(instance.id.clone(), envelope);
        let mut patch = Map::new();
        patch.insert("providerInstances".to_owned(), Value::Object(map));
        if instance.legacy
            && let Some(driver) = driver(&instance.driver)
        {
            patch.insert("providers".to_owned(), json!({ driver.kind: legacy_defaults(driver) }));
        }
        Value::Object(patch)
    }

    /// The patch adding a new instance (enabled, with an optional name). Codex
    /// instances start with `setupMode: "existing"`, as upstream creates them.
    pub fn instance_add_patch(
        &self,
        id: &str,
        driver: &str,
        display_name: &str,
    ) -> Result<Value, String> {
        validate_instance_id(id, &self.provider_instances())?;
        let mut envelope = json!({ "driver": driver, "enabled": true });
        set_display_name(&mut envelope, display_name);
        if driver == "codex" {
            envelope["config"] = json!({ "setupMode": "existing" });
        }
        let mut map = self.explicit_instances();
        map.insert(id.to_owned(), envelope);
        Ok(json!({ "providerInstances": map }))
    }

    /// The patch removing a custom (non-default) instance.
    pub fn instance_remove_patch(&self, id: &str) -> Value {
        let mut map = self.explicit_instances();
        map.remove(id);
        json!({ "providerInstances": map })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ServerSettings {
        ServerSettings::from_value(json!({
            "providers": {
                "codex": { "enabled": true, "binaryPath": "codex", "homePath": "", "customModels": [] },
                "grok": { "enabled": false, "binaryPath": "grok", "customModels": [] }
            },
            "providerInstances": {
                "codex_work": {
                    "driver": "codex", "enabled": true, "displayName": "Work",
                    "futureEnvelopeField": 7,
                    "config": { "homePath": "~/w", "futureConfigField": [1, 2],
                                "customModels": ["a", { "slug": "b", "name": "B" }] }
                },
                "fork": { "driver": "someFork", "config": { "x": 1 } }
            },
            "somethingNew": true
        }))
    }

    fn field(driver: &str, key: &str) -> &'static Field {
        driver_fields(driver).iter().find(|field| field.key == key).unwrap()
    }
    fn driver_fields(kind: &str) -> &'static [Field] {
        driver(kind).unwrap().fields
    }

    #[test]
    fn lists_default_slots_then_explicit_instances() {
        let ids: Vec<_> = settings().provider_instances().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, ["codex", "grok", "codex_work", "fork"]);
        let codex = settings().provider_instance("codex").unwrap();
        assert!(codex.is_default && codex.legacy && codex.enabled());
        assert_eq!(codex.text_value("binaryPath"), "codex");
        assert!(codex.envelope["config"].get("enabled").is_none());
        assert!(!settings().provider_instance("grok").unwrap().enabled());
    }

    #[test]
    fn enable_toggle_patch_resends_the_map_and_keeps_unknown_fields() {
        let settings = settings();
        let work = settings.provider_instance("codex_work").unwrap();
        let mut envelope = work.envelope.clone();
        set_enabled(&mut envelope, false);
        let patch = settings.instance_update_patch(&work, envelope);
        let map = &patch["providerInstances"];
        assert_eq!(map["codex_work"]["enabled"], false);
        assert_eq!(map["codex_work"]["futureEnvelopeField"], 7);
        assert_eq!(map["codex_work"]["config"]["futureConfigField"], json!([1, 2]));
        assert_eq!(map["fork"], settings.raw()["providerInstances"]["fork"]);
        assert!(patch.get("providers").is_none());
        assert!(patch.get("somethingNew").is_none());
    }

    #[test]
    fn editing_a_legacy_default_slot_moves_it_and_resets_the_legacy_object() {
        let settings = settings();
        let grok = settings.provider_instance("grok").unwrap();
        let mut envelope = grok.envelope.clone();
        set_enabled(&mut envelope, true);
        let patch = settings.instance_update_patch(&grok, envelope);
        assert_eq!(
            patch["providerInstances"]["grok"],
            json!({ "driver": "grok", "enabled": true,
                    "config": { "binaryPath": "grok", "customModels": [] } })
        );
        assert_eq!(patch["providers"]["grok"]["enabled"], false);
        assert_eq!(patch["providers"]["grok"]["binaryPath"], "grok");
        // The other instances are carried along.
        assert!(patch["providerInstances"]["codex_work"].is_object());
    }

    #[test]
    fn field_edit_writes_config_and_blank_omits() {
        let settings = settings();
        let work = settings.provider_instance("codex_work").unwrap();
        let mut envelope = work.envelope.clone();
        set_field(&mut envelope, field("codex", "binaryPath"), &FieldValue::Text("/opt/codex".into()));
        let patch = settings.instance_update_patch(&work, envelope.clone());
        let config = &patch["providerInstances"]["codex_work"]["config"];
        assert_eq!(config["binaryPath"], "/opt/codex");
        assert_eq!(config["homePath"], "~/w");
        assert_eq!(config["futureConfigField"], json!([1, 2]));
        set_field(&mut envelope, field("codex", "homePath"), &FieldValue::Text("  ".into()));
        assert!(envelope["config"].get("homePath").is_none());
        // Persisting fields keep an empty value.
        set_field(&mut envelope, field("antigravity", "binaryPath"), &FieldValue::Text(String::new()));
        assert_eq!(envelope["config"]["binaryPath"], "");
    }

    #[test]
    fn emptying_the_config_drops_it() {
        let mut envelope = json!({ "driver": "grok", "config": { "binaryPath": "x" } });
        set_field(&mut envelope, field("grok", "binaryPath"), &FieldValue::Text(String::new()));
        assert_eq!(envelope, json!({ "driver": "grok" }));
    }

    #[test]
    fn custom_models_add_and_remove_keep_other_entries() {
        let settings = settings();
        let work = settings.provider_instance("codex_work").unwrap();
        assert_eq!(work.custom_models(), ["a", "b"]);
        let mut envelope = work.envelope.clone();
        assert!(add_custom_model(&mut envelope, " gpt-x "));
        assert!(!add_custom_model(&mut envelope, "a"));
        assert!(!add_custom_model(&mut envelope, "  "));
        let patch = settings.instance_update_patch(&work, envelope.clone());
        assert_eq!(
            patch["providerInstances"]["codex_work"]["config"]["customModels"],
            json!(["a", { "slug": "b", "name": "B" }, "gpt-x"])
        );
        remove_custom_model(&mut envelope, "b");
        remove_custom_model(&mut envelope, "a");
        remove_custom_model(&mut envelope, "gpt-x");
        assert_eq!(envelope["config"]["customModels"], json!([]));
        assert_eq!(envelope["config"]["homePath"], "~/w");
    }

    #[test]
    fn add_and_remove_instance_patches() {
        let settings = settings();
        let patch = settings.instance_add_patch("codex_home", "codex", "Home").unwrap();
        assert_eq!(
            patch["providerInstances"]["codex_home"],
            json!({ "driver": "codex", "enabled": true, "displayName": "Home",
                    "config": { "setupMode": "existing" } })
        );
        assert!(patch["providerInstances"]["fork"].is_object());
        let claude = settings.instance_add_patch("claude_b", "claudeAgent", "").unwrap();
        assert_eq!(claude["providerInstances"]["claude_b"], json!({ "driver": "claudeAgent", "enabled": true }));
        assert!(settings.instance_add_patch("codex_work", "codex", "").is_err());
        assert!(settings.instance_add_patch("1bad", "codex", "").is_err());
        let removed = settings.instance_remove_patch("codex_work");
        assert!(removed["providerInstances"].get("codex_work").is_none());
        assert!(removed["providerInstances"]["fork"].is_object());
    }

    #[test]
    fn instance_ids_derive_from_labels() {
        assert_eq!(derive_instance_id("codex", "My Work!"), "codex_my_work");
        assert_eq!(derive_instance_id("codex", "  "), "");
    }

    #[test]
    fn explicit_false_wins_over_enabled_resolution() {
        let instance = |envelope: Value| InstanceConfig {
            id: "x".into(),
            driver: "grok".into(),
            is_default: false,
            legacy: false,
            envelope,
        };
        assert!(!instance(json!({ "driver": "grok" })).enabled());
        assert!(instance(json!({ "driver": "grok", "enabled": true })).enabled());
        assert!(!instance(json!({ "driver": "grok", "enabled": true, "config": { "enabled": false } })).enabled());
    }

    #[test]
    fn applying_the_update_patch_replaces_the_instance_map() {
        let mut settings = settings();
        let patch = settings.instance_remove_patch("codex_work");
        settings.apply_patch(&patch);
        assert!(settings.provider_instance("codex_work").is_none());
        assert!(settings.provider_instance("fork").is_some());
    }
}
