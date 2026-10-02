//! Small app-wide preferences that are not tied to a server: favorite models
//! and the appearance. Stored beside the drafts as `prefs.json`.

use std::path::PathBuf;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// `"<instanceId>/<model>"` keys, in the order they were starred.
    pub favorite_models: Vec<String>,
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
        if let Some(path) = path() {
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
}

static EMPTY: Prefs = Prefs { favorite_models: Vec::new(), light_theme: false };
