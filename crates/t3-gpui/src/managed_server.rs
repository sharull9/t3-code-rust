//! Runs T3 on this machine against the shared T3 home (`~/.t3`), so the GPUI
//! app and the T3 Code desktop app see the same projects and threads.
//!
//! Only one server may own a T3 home's database. If one is already running
//! there (usually the desktop app's), this attaches to it by minting a pairing
//! credential through the server CLI. Otherwise it starts and owns a server
//! child with a private desktop bootstrap token. Credentials stay in memory.
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use t3_client::{Credentials, PairingLink};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use url::Url;

const READY_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const CLI_TIMEOUT: Duration = Duration::from_secs(60);
const CLIENT_LABEL: &str = "T3 GPUI (local)";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// How to invoke the T3 server: a standalone `t3` binary, or the desktop
/// app's Electron executable running its bundled server as Node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launcher {
    program: PathBuf,
    entry: Option<PathBuf>,
}

impl Launcher {
    /// Interpret a user-chosen file. A T3 Code desktop executable runs its
    /// bundled server; anything else is treated as a standalone `t3` binary.
    pub fn from_path(path: PathBuf) -> Result<Self, String> {
        let metadata = std::fs::metadata(&path).map_err(|error| {
            format!("T3 server executable {} is unavailable: {error}", path.display())
        })?;
        if !metadata.is_file() {
            return Err(format!("T3 server executable {} is not a file", path.display()));
        }
        let entry = path.parent().and_then(desktop_server_entry);
        Ok(Self { program: path, entry })
    }

    /// Find an installed server: a standalone install under
    /// `<t3 home>/runtime/versions/<version>`, then the T3 Code desktop app.
    pub fn discover(t3_home: &Path) -> Option<Self> {
        standalone_installs(t3_home)
            .into_iter()
            .next()
            .map(|program| Self { program, entry: None })
            .or_else(desktop_install)
    }

    pub fn describe(&self) -> String {
        match &self.entry {
            Some(_) => format!("{} (bundled server)", self.program.display()),
            None => self.program.display().to_string(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(entry) = &self.entry {
            command.env("ELECTRON_RUN_AS_NODE", "1").arg(entry);
        }
        if let Some(home) = dirs::home_dir() {
            command.current_dir(home);
        }
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
}

/// `$T3CODE_HOME`, else `~/.t3`: the home the T3 CLI and desktop app use.
pub fn default_t3_home() -> Option<PathBuf> {
    match std::env::var_os("T3CODE_HOME") {
        Some(home) if !home.is_empty() => Some(PathBuf::from(home)),
        _ => Some(dirs::home_dir()?.join(".t3")),
    }
}

fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// Standalone installs, newest first by modification time.
fn standalone_installs(t3_home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(t3_home.join("runtime").join("versions")) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path().join(exe_name("t3")))
        .filter(|path| path.is_file())
        .map(|path| {
            let modified = path
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, path)
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, path)| path).collect()
}

/// The bundled server entry next to a desktop executable, if this is one.
fn desktop_server_entry(app_dir: &Path) -> Option<PathBuf> {
    let resources = app_dir.join("resources");
    // Packaged Windows ships the server tree as a sidecar asar; elsewhere it
    // lives in the app asar. Electron-as-Node reads both in place.
    ["server.asar", "app.asar"]
        .iter()
        .map(|archive| resources.join(archive))
        .find(|archive| archive.exists())
        .map(|archive| archive.join("apps").join("server").join("dist").join("bin.mjs"))
}

#[cfg(windows)]
fn desktop_install() -> Option<Launcher> {
    let app_dir = dirs::data_local_dir()?.join("Programs").join("t3code");
    let entry = desktop_server_entry(&app_dir)?;
    let program = std::fs::read_dir(&app_dir).ok()?.flatten().map(|e| e.path()).find(|path| {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        name.starts_with("T3 Code") && name.ends_with(".exe")
    })?;
    Some(Launcher { program, entry: Some(entry) })
}

#[cfg(not(windows))]
fn desktop_install() -> Option<Launcher> {
    None
}

/// What a live server records next to its database.
#[derive(Deserialize)]
struct RuntimeState {
    origin: Url,
}

enum Owner {
    /// A server this app started; dropping it kills the process.
    Child(Child),
    /// A server someone else runs on the same T3 home.
    Attached,
}

pub struct ManagedServer {
    owner: Owner,
    launcher: Launcher,
    t3_home: PathBuf,
    /// Reusable secret for an owned child; attached servers mint a fresh
    /// pairing credential instead.
    bootstrap_token: Option<String>,
    credentials: Credentials,
    base_url: Url,
}

impl ManagedServer {
    /// Use the server already running on the shared T3 home, or start one.
    /// `executable` overrides discovery.
    pub async fn start(executable: Option<PathBuf>, http: &reqwest::Client) -> Result<Self, String> {
        let t3_home =
            default_t3_home().ok_or_else(|| "could not locate the home directory".to_owned())?;
        let launcher = match executable {
            Some(path) => Launcher::from_path(path)?,
            None => Launcher::discover(&t3_home).ok_or_else(|| {
                format!(
                    "no T3 server found. Install T3 Code or the t3 CLI, or choose an executable. \
                     Looked in {} and the T3 Code install folder.",
                    t3_home.join("runtime").join("versions").display()
                )
            })?,
        };
        if let Some(origin) = running_server(http, &t3_home).await {
            return Self::attach(launcher, t3_home, origin, http).await;
        }
        Self::spawn(launcher, t3_home, http).await
    }

    async fn attach(
        launcher: Launcher,
        t3_home: PathBuf,
        base_url: Url,
        http: &reqwest::Client,
    ) -> Result<Self, String> {
        let credentials = mint_and_pair(&launcher, &t3_home, &base_url, http).await?;
        Ok(Self { owner: Owner::Attached, launcher, t3_home, bootstrap_token: None, credentials, base_url })
    }

    async fn spawn(launcher: Launcher, t3_home: PathBuf, http: &reqwest::Client) -> Result<Self, String> {
        std::fs::create_dir_all(&t3_home)
            .map_err(|error| format!("create T3 home {}: {error}", t3_home.display()))?;
        let port = loopback_port().map_err(|error| format!("allocate local server port: {error}"))?;
        let base_url = Url::parse(&format!("http://127.0.0.1:{port}/"))
            .map_err(|error| format!("construct local server URL: {error}"))?;
        let bootstrap_token = format!("{}{}", t3_client::new_id(), t3_client::new_id());
        let envelope = json!({
            "mode": "desktop",
            "noBrowser": true,
            "port": port,
            "host": "127.0.0.1",
            "t3Home": t3_home,
            "desktopBootstrapToken": bootstrap_token,
            "tailscaleServeEnabled": false,
            "tailscaleServePort": 443,
        });

        let log_path = log_path();
        let stderr = log_path
            .as_deref()
            .and_then(|path| {
                path.parent().map(std::fs::create_dir_all);
                std::fs::File::create(path).ok()
            })
            .map_or_else(Stdio::null, Stdio::from);
        let mut command = launcher.command();
        command
            .args(["--bootstrap-fd", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // The token travels only through stdin, never the command line.
            .stderr(stderr)
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|error| format!("start local server {}: {error}", launcher.describe()))?;
        let mut stdin =
            child.stdin.take().ok_or_else(|| "local server stdin was not available".to_owned())?;
        let mut encoded = serde_json::to_vec(&envelope)
            .map_err(|error| format!("encode local server bootstrap: {error}"))?;
        encoded.push(b'\n');
        stdin
            .write_all(&encoded)
            .await
            .map_err(|error| format!("send local server bootstrap: {error}"))?;
        stdin
            .shutdown()
            .await
            .map_err(|error| format!("close local server bootstrap input: {error}"))?;
        drop(stdin);

        if let Err(error) = wait_until_ready(&mut child, http, &base_url).await {
            return Err(match &log_path {
                Some(path) => format!("{error}. Server log: {}", path.display()),
                None => error,
            });
        }
        let pairing = PairingLink::parse(&format!("{base_url} {bootstrap_token}"))
            .map_err(|error| format!("parse local server credentials: {error}"))?;
        let credentials = t3_client::auth::pair(http, &pairing, CLIENT_LABEL)
            .await
            .map_err(|error| format!("authenticate with local server: {error}"))?;

        Ok(Self {
            owner: Owner::Child(child),
            launcher,
            t3_home,
            bootstrap_token: Some(bootstrap_token),
            credentials,
            base_url,
        })
    }

    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Why the server is gone, if it is. An owned child is checked directly;
    /// an attached server is probed, since its owner may have quit.
    pub async fn exited(&mut self, http: &reqwest::Client) -> Option<String> {
        match &mut self.owner {
            Owner::Child(child) => match child.try_wait() {
                Ok(Some(status)) => Some(format!("Local server exited ({status})")),
                Ok(None) => None,
                Err(error) => Some(format!("Could not check the local server: {error}")),
            },
            Owner::Attached => (!is_ready(http, &self.base_url).await)
                .then(|| format!("The T3 server at {} stopped", self.base_url)),
        }
    }

    /// Get fresh credentials after the access token expires.
    pub async fn reauthenticate(&mut self, http: &reqwest::Client) -> Result<(), String> {
        self.credentials = match &self.bootstrap_token {
            Some(token) => {
                let pairing = PairingLink::parse(&format!("{} {token}", self.base_url))
                    .map_err(|error| format!("parse local server credentials: {error}"))?;
                t3_client::auth::pair(http, &pairing, CLIENT_LABEL)
                    .await
                    .map_err(|error| format!("reauthenticate with local server: {error}"))?
            }
            None => mint_and_pair(&self.launcher, &self.t3_home, &self.base_url, http).await?,
        };
        Ok(())
    }

    /// Stop an owned server and wait for it to exit. An attached server
    /// belongs to someone else and keeps running.
    pub async fn shutdown(mut self) -> Result<(), String> {
        let Owner::Child(child) = &mut self.owner else {
            return Ok(());
        };
        if child.try_wait().map_err(|error| format!("check local server: {error}"))?.is_none() {
            child.kill().await.map_err(|error| format!("stop local server: {error}"))?;
        }
        child
            .wait()
            .await
            .map(|_| ())
            .map_err(|error| format!("wait for local server shutdown: {error}"))
    }
}

fn log_path() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("t3-gpui").join("logs").join("server.log"))
}

/// The origin of a live server on this T3 home, from the runtime file it
/// writes next to its database, confirmed by a health probe.
async fn running_server(http: &reqwest::Client, t3_home: &Path) -> Option<Url> {
    let path = t3_home.join("userdata").join("server-runtime.json");
    let state: RuntimeState = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    is_ready(http, &state.origin).await.then_some(state.origin)
}

/// Ask the server CLI for a one-time pairing credential on `t3_home`, then
/// exchange it at `base_url`.
async fn mint_and_pair(
    launcher: &Launcher,
    t3_home: &Path,
    base_url: &Url,
    http: &reqwest::Client,
) -> Result<Credentials, String> {
    #[derive(Deserialize)]
    struct Issued {
        credential: String,
    }
    let mut command = launcher.command();
    command
        .args(["auth", "pairing", "create", "--json", "--ttl", "5m", "--label", CLIENT_LABEL])
        .arg("--base-dir")
        .arg(t3_home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(CLI_TIMEOUT, command.output())
        .await
        .map_err(|_| "T3 CLI did not issue a pairing credential in time".to_owned())?
        .map_err(|error| format!("run T3 CLI {}: {error}", launcher.describe()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().find(|line| !line.trim().is_empty()).unwrap_or("no output");
        return Err(format!("T3 CLI could not issue a pairing credential ({}): {detail}", output.status));
    }
    let issued: Issued = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("read pairing credential from T3 CLI: {error}"))?;
    let pairing = PairingLink::parse(&format!("{base_url} {}", issued.credential))
        .map_err(|error| format!("parse pairing credential: {error}"))?;
    t3_client::auth::pair(http, &pairing, CLIENT_LABEL)
        .await
        .map_err(|error| format!("authenticate with the T3 server at {base_url}: {error}"))
}

fn loopback_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn is_ready(http: &reqwest::Client, base_url: &Url) -> bool {
    let Ok(probe_url) = base_url.join(".well-known/t3/environment") else {
        return false;
    };
    http.get(probe_url)
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

async fn wait_until_ready(
    child: &mut Child,
    http: &reqwest::Client,
    base_url: &Url,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        if let Some(status) =
            child.try_wait().map_err(|error| format!("check local server startup: {error}"))?
        {
            return Err(format!("local server exited before becoming ready: {status}"));
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err("local server did not become ready within 60 seconds".into());
        }
        if is_ready(http, base_url).await {
            return Ok(());
        }
        tokio::time::sleep(PROBE_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn isolated_dir() -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("t3-gpui-managed-server-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn missing_executable_is_rejected_before_anything_runs() {
        let dir = isolated_dir();
        let error = Launcher::from_path(dir.join("missing-server-executable")).unwrap_err();
        assert!(error.contains("unavailable"));
        assert!(!dir.exists());
    }

    #[test]
    fn standalone_installs_prefer_the_newest_version_and_skip_staging() {
        let home = isolated_dir();
        let versions = home.join("runtime").join("versions");
        for version in ["0.1.0", ".staging-abc", "0.2.0"] {
            std::fs::create_dir_all(versions.join(version)).unwrap();
            std::fs::write(versions.join(version).join(exe_name("t3")), b"").unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::create_dir_all(versions.join("empty")).unwrap();
        let found = standalone_installs(&home);
        assert_eq!(
            found,
            [versions.join("0.2.0").join(exe_name("t3")), versions.join("0.1.0").join(exe_name("t3"))]
        );
        assert_eq!(Launcher::discover(&home).unwrap().program, found[0]);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn desktop_executables_run_their_bundled_server() {
        let dir = isolated_dir();
        std::fs::create_dir_all(dir.join("resources").join("server.asar")).unwrap();
        let program = dir.join("T3 Code.exe");
        std::fs::write(&program, b"").unwrap();
        let launcher = Launcher::from_path(program.clone()).unwrap();
        assert_eq!(launcher.program, program);
        assert_eq!(
            launcher.entry,
            Some(dir.join("resources/server.asar/apps/server/dist/bin.mjs"))
        );

        let standalone = dir.join("bin").join(exe_name("t3"));
        std::fs::create_dir_all(standalone.parent().unwrap()).unwrap();
        std::fs::write(&standalone, b"").unwrap();
        assert_eq!(Launcher::from_path(standalone).unwrap().entry, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Needs an installed T3 server. Starts one on a throwaway home, then
    /// attaches a second client to it the way the app attaches to T3 Code.
    #[tokio::test]
    #[ignore = "starts the installed T3 server"]
    async fn starts_a_server_then_attaches_to_it() {
        let home = isolated_dir();
        let launcher = Launcher::discover(&default_t3_home().unwrap())
            .expect("an installed T3 server");
        let http = reqwest::Client::new();
        let owned = ManagedServer::spawn(launcher.clone(), home.clone(), &http).await.unwrap();
        let origin = running_server(&http, &home).await.expect("runtime file for the new server");
        assert_eq!(origin.port(), owned.base_url().port());

        let mut attached = ManagedServer::attach(launcher, home.clone(), origin, &http).await.unwrap();
        assert!(attached.exited(&http).await.is_none());
        attached.reauthenticate(&http).await.unwrap();
        t3_client::Connection::connect(&http, attached.credentials()).await.unwrap();

        owned.shutdown().await.unwrap();
        assert!(attached.exited(&http).await.is_some());
        let _ = std::fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn stale_runtime_files_are_not_treated_as_running_servers() {
        let home = isolated_dir();
        std::fs::create_dir_all(home.join("userdata")).unwrap();
        let port = loopback_port().unwrap();
        std::fs::write(
            home.join("userdata").join("server-runtime.json"),
            format!(r#"{{"version":1,"pid":1,"port":{port},"origin":"http://127.0.0.1:{port}"}}"#),
        )
        .unwrap();
        assert!(running_server(&reqwest::Client::new(), &home).await.is_none());
        std::fs::remove_dir_all(home).unwrap();
    }
}
