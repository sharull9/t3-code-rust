//! Small app-wide preferences that are not tied to a server: favorite and
//! hidden models, and the appearance. Stored beside the drafts as `prefs.json`.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

/// Which color scheme the app uses; `System` follows the OS appearance.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// How wide the conversation column may grow.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatWidth {
    #[default]
    Comfortable,
    Wide,
    Full,
}

pub const INTERFACE_FONT_SIZE: RangeInclusive<u32> = 12..=20;
pub const PROMPT_FONT_SIZE: RangeInclusive<u32> = 12..=20;
pub const CODE_FONT_SIZE: RangeInclusive<u32> = 10..=18;
pub const DEFAULT_INTERFACE_FONT_SIZE: u32 = 16;
pub const DEFAULT_PROMPT_FONT_SIZE: u32 = 14;
pub const DEFAULT_CODE_FONT_SIZE: u32 = 13;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// `"<instanceId>/<model>"` keys, in the order they were starred.
    pub favorite_models: Vec<String>,
    /// Editor ID (`EditorId`) the info panel's "Open in" button launches.
    pub preferred_editor: Option<String>,
    /// Whether the info panel was showing when the app last closed.
    pub info_panel_open: bool,
    /// `"<instanceId>/<model>"` keys left out of the model picker.
    pub hidden_models: Vec<String>,
    pub theme: ThemeMode,
    /// Pre-`theme` boolean; read only to migrate old files, never written.
    #[serde(rename = "lightTheme", skip_serializing)]
    pub(crate) legacy_light_theme: Option<bool>,
    pub font_size_interface: u32,
    pub font_size_prompt: u32,
    pub font_size_code: u32,
    pub chat_width: ChatWidth,
    pub confirm_thread_archive: bool,
    /// Shortcut overrides for commands the server has no keybinding id for,
    /// by `keymap::Command::id`, in upstream syntax (`mod+shift+l`).
    pub keybindings: BTreeMap<String, Vec<String>>,
}

const DEFAULT: Prefs = Prefs {
    favorite_models: Vec::new(),
    preferred_editor: None,
    info_panel_open: false,
    hidden_models: Vec::new(),
    theme: ThemeMode::System,
    legacy_light_theme: None,
    font_size_interface: DEFAULT_INTERFACE_FONT_SIZE,
    font_size_prompt: DEFAULT_PROMPT_FONT_SIZE,
    font_size_code: DEFAULT_CODE_FONT_SIZE,
    chat_width: ChatWidth::Comfortable,
    confirm_thread_archive: false,
    keybindings: BTreeMap::new(),
};

impl Default for Prefs {
    fn default() -> Self {
        DEFAULT
    }
}

impl Global for Prefs {}

fn path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| directory.join("t3-gpui").join("prefs.json"))
}

pub fn favorite_key(instance_id: &str, model: &str) -> String {
    format!("{instance_id}/{model}")
}

impl Prefs {
    /// Reads the saved preferences; a missing or unreadable file gives defaults.
    pub fn load() -> Self {
        path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| Self::parse(&bytes))
            .unwrap_or_default()
    }

    /// Parses saved preferences, migrating the old `lightTheme` boolean.
    fn parse(bytes: &[u8]) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let has_theme = value.get("theme").is_some();
        let mut prefs: Self = serde_json::from_value(value).ok()?;
        if !has_theme && let Some(light) = prefs.legacy_light_theme {
            prefs.theme = if light { ThemeMode::Light } else { ThemeMode::Dark };
        }
        prefs.legacy_light_theme = None;
        prefs.font_size_interface = clamp(prefs.font_size_interface, INTERFACE_FONT_SIZE);
        prefs.font_size_prompt = clamp(prefs.font_size_prompt, PROMPT_FONT_SIZE);
        prefs.font_size_code = clamp(prefs.font_size_code, CODE_FONT_SIZE);
        Some(prefs)
    }

    /// Resets everything the Appearance page controls.
    pub fn reset_appearance(&mut self) {
        let defaults = Self::default();
        self.theme = defaults.theme;
        self.font_size_interface = defaults.font_size_interface;
        self.font_size_prompt = defaults.font_size_prompt;
        self.font_size_code = defaults.font_size_code;
        self.chat_width = defaults.chat_width;
        self.confirm_thread_archive = defaults.confirm_thread_archive;
    }

    pub fn appearance_modified(&self) -> bool {
        let defaults = Self::default();
        self.theme != defaults.theme
            || self.font_size_interface != defaults.font_size_interface
            || self.font_size_prompt != defaults.font_size_prompt
            || self.font_size_code != defaults.font_size_code
            || self.chat_width != defaults.chat_width
            || self.confirm_thread_archive != defaults.confirm_thread_archive
    }

    pub fn global(cx: &App) -> &Self {
        cx.try_global::<Self>().unwrap_or(&EMPTY)
    }

    /// Applies `change` to the global preferences and writes them to disk.
    pub fn update(cx: &mut App, change: impl FnOnce(&mut Self)) {
        let mut prefs = Self::global(cx).clone();
        change(&mut prefs);
        // Tests change preferences without touching the user's saved file.
        if let Some(path) = path().filter(|_| !cfg!(test)) {
            let saved = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(&path, serde_json::to_vec_pretty(&prefs)?));
            if let Err(error) = saved {
                eprintln!("could not save preferences: {error}");
            }
        }
        cx.set_global(prefs);
    }

    pub fn toggle_favorite(cx: &mut App, instance_id: &str, model: &str) {
        let key = favorite_key(instance_id, model);
        Self::update(cx, |prefs| {
            if let Some(index) = prefs.favorite_models.iter().position(|k| *k == key) {
                prefs.favorite_models.remove(index);
            } else {
                prefs.favorite_models.push(key);
            }
        });
    }

    pub fn is_hidden(&self, instance_id: &str, model: &str) -> bool {
        self.hidden_models.contains(&favorite_key(instance_id, model))
    }

    pub fn set_model_hidden(cx: &mut App, instance_id: &str, model: &str, hidden: bool) {
        Self::set_models_hidden(cx, instance_id, [model], hidden);
    }

    /// Shows or hides several models of one instance in a single save.
    pub fn set_models_hidden<'a>(
        cx: &mut App,
        instance_id: &str,
        models: impl IntoIterator<Item = &'a str>,
        hidden: bool,
    ) {
        let keys: Vec<String> = models.into_iter().map(|model| favorite_key(instance_id, model)).collect();
        Self::update(cx, |prefs| {
            prefs.hidden_models.retain(|key| !keys.contains(key));
            if hidden {
                prefs.hidden_models.extend(keys);
            }
        });
    }
}

static EMPTY: Prefs = DEFAULT;

fn clamp(value: u32, range: RangeInclusive<u32>) -> u32 {
    value.clamp(*range.start(), *range.end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_light_theme_boolean_migrates() {
        let light = Prefs::parse(br#"{"lightTheme":true,"favoriteModels":["a/b"]}"#).unwrap();
        assert_eq!(light.theme, ThemeMode::Light);
        assert_eq!(light.favorite_models, ["a/b"]);
        let dark = Prefs::parse(br#"{"lightTheme":false}"#).unwrap();
        assert_eq!(dark.theme, ThemeMode::Dark);
        // An explicit theme wins over the legacy flag; no file means System.
        let explicit = Prefs::parse(br#"{"theme":"system","lightTheme":true}"#).unwrap();
        assert_eq!(explicit.theme, ThemeMode::System);
        assert_eq!(Prefs::parse(b"{}").unwrap().theme, ThemeMode::System);
        let saved = serde_json::to_string(&light).unwrap();
        assert!(!saved.contains("lightTheme") && saved.contains(r#""theme":"light""#));
    }

    #[test]
    fn font_sizes_are_clamped_and_reset_restores_defaults() {
        let mut prefs = Prefs::parse(br#"{"fontSizeCode":99,"fontSizeInterface":1}"#).unwrap();
        assert_eq!((prefs.font_size_code, prefs.font_size_interface), (18, 12));
        prefs.chat_width = ChatWidth::Full;
        prefs.favorite_models.push("a/b".into());
        assert!(prefs.appearance_modified());
        prefs.reset_appearance();
        assert!(!prefs.appearance_modified());
        assert_eq!(prefs.favorite_models, ["a/b"]);
    }
}
