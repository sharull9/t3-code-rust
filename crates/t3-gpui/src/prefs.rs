//! Small app-wide preferences that are not tied to a server: favorite and
//! hidden models, and the appearance. Stored beside the drafts as `prefs.json`.

use std::path::PathBuf;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// `"<instanceId>/<model>"` keys, in the order they were starred.
    pub favorite_models: Vec<String>,
    /// `"<instanceId>/<model>"` keys left out of the model picker.
    pub hidden_models: Vec<String>,
    pub light_theme: bool,
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
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
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

static EMPTY: Prefs =
    Prefs { favorite_models: Vec::new(), hidden_models: Vec::new(), light_theme: false };
