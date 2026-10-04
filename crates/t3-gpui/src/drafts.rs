//! Persistent user-authored drafts, isolated by server and environment.
//!
//! The store contains only composer text and question answers. It deliberately
//! has no fields for pairing links, tokens, or other connection credentials.
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use t3_client::pending::AnswerDraft;
use url::Url;

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DraftStore {
    /// Keys are normalized server URLs. Each server keeps separate named
    /// environments so switching environments cannot surface another one's text.
    pub servers: HashMap<String, HashMap<String, EnvironmentDrafts>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvironmentDrafts {
    /// Composer text by thread ID, including draft threads' IDs.
    pub thread_text: HashMap<String, String>,
    /// thread ID -> request ID -> question ID -> answer draft.
    pub question_answers: HashMap<String, HashMap<String, HashMap<String, AnswerDraft>>>,
    /// Threads started locally but not yet created on the server. A draft
    /// becomes a real thread, with the same ID, when its first message is sent.
    pub new_threads: Vec<DraftThread>,
}

/// A new thread's settings before it exists on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftThread {
    pub id: String,
    pub project_id: String,
    pub model_selection: serde_json::Value,
    pub runtime_mode: String,
    pub interaction_mode: String,
    /// Start the thread in a new worktree instead of the project checkout.
    #[serde(default)]
    pub new_worktree: bool,
    /// The branch a new worktree starts from; the checked-out one when unset.
    #[serde(default)]
    pub base_branch: Option<String>,
}

pub fn default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| directory.join("t3-gpui").join("drafts.json"))
}

impl DraftStore {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        serde_json::from_slice(&contents).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid draft store: {error}"))
        })
    }

    /// Write a complete replacement beside the destination, flush it, then
    /// rename it over the old file. A failed write leaves the last good file
    /// untouched.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let parent =
            path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::other(format!("serialize draft store: {error}")))?;
        let (temporary, mut file) = create_temp_file(path)?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)?;
            // Make the directory entry durable where the platform supports it.
            if let Ok(directory) = File::open(parent) {
                let _ = directory.sync_all();
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn environment(&self, server_url: &str, environment: &str) -> Option<&EnvironmentDrafts> {
        self.servers.get(&normalize_server_url(server_url))?.get(environment)
    }

    pub fn environment_mut(
        &mut self,
        server_url: &str,
        environment: &str,
    ) -> &mut EnvironmentDrafts {
        self.servers
            .entry(normalize_server_url(server_url))
            .or_default()
            .entry(environment.to_owned())
            .or_default()
    }
}

fn create_temp_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let parent =
        path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    for _ in 0..32 {
        let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), id));
        match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not allocate temporary draft file"))
}

/// Normalize the origin's scheme and host, remove default ports and trailing
/// slashes, and discard any URL user-info so credentials never become keys.
pub fn normalize_server_url(value: &str) -> String {
    let value = value.trim();
    let Ok(mut url) = Url::parse(value) else {
        let Some((scheme, remainder)) = value.split_once("://") else {
            return value
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_owned();
        };
        let remainder = remainder.split(['?', '#']).next().unwrap_or_default();
        let authority_end = remainder.find('/').unwrap_or(remainder.len());
        let authority = remainder[..authority_end].rsplit('@').next().unwrap_or_default();
        return format!(
            "{}://{}{}",
            scheme.to_ascii_lowercase(),
            authority.to_ascii_lowercase(),
            remainder[authority_end..].trim_end_matches('/')
        );
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string().trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("t3-gpui-drafts-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn drafts_round_trip_and_are_isolated_by_server_and_environment() {
        let dir = temp_dir();
        let path = dir.join("drafts.json");
        let mut store = DraftStore::default();
        store
            .environment_mut("HTTPS://Example.COM:443/", "prod")
            .thread_text
            .insert("thread-a".into(), "unfinished message".into());
        store
            .environment_mut("https://example.com", "dev")
            .question_answers
            .entry("thread-a".into())
            .or_default()
            .entry("request-a".into())
            .or_default()
            .insert(
                "question-a".into(),
                AnswerDraft { selected: vec!["one".into()], custom: "notes".into() },
            );
        store
            .environment_mut("https://other.example", "prod")
            .thread_text
            .insert("thread-a".into(), "separate server".into());
        store.environment_mut("https://example.com", "prod").new_threads.push(DraftThread {
            id: "draft-1".into(),
            project_id: "project-1".into(),
            model_selection: serde_json::json!({ "instanceId": "codex", "model": "gpt" }),
            runtime_mode: "full-access".into(),
            interaction_mode: "default".into(),
            new_worktree: false,
            base_branch: None,
        });
        store.save(&path).unwrap();
        store
            .environment_mut("https://example.com", "prod")
            .thread_text
            .insert("thread-a".into(), "updated message".into());
        store.save(&path).unwrap();

        let loaded = DraftStore::load(&path).unwrap();
        assert_eq!(loaded, store);
        assert_eq!(
            loaded.environment("https://EXAMPLE.com/", "prod").unwrap().thread_text["thread-a"],
            "updated message"
        );
        assert!(loaded.environment("https://example.com", "dev").unwrap().thread_text.is_empty());
        assert_eq!(loaded.servers.len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_loads_empty_and_malformed_file_is_reported() {
        let dir = temp_dir();
        let path = dir.join("drafts.json");
        assert_eq!(DraftStore::load(&path).unwrap(), DraftStore::default());
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, b"not json").unwrap();
        assert_eq!(DraftStore::load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn url_normalization_discards_user_info_and_default_port() {
        assert_eq!(
            normalize_server_url(" HTTPS://name:secret@Example.COM:443///?token=no#fragment "),
            "https://example.com"
        );
        assert_eq!(
            normalize_server_url("http://EXAMPLE.com:80/api/?token=no#fragment"),
            "http://example.com/api"
        );
    }
}
