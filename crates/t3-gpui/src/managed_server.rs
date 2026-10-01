//! Owns a locally managed T3 server process and its in-memory credentials.
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;
use t3_client::{Credentials, PairingLink};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use url::Url;

const READY_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const CLIENT_LABEL: &str = "T3 GPUI (managed)";

pub struct ManagedServer {
    child: Child,
    bootstrap_token: String,
    credentials: Credentials,
    base_url: Url,
}

impl ManagedServer {
    /// Start the bundled server with a private desktop bootstrap token and
    /// exchange that token for the initial in-memory credentials.
    pub async fn start(executable: PathBuf, http: &reqwest::Client) -> Result<Self, String> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| "could not locate the local application data directory".to_owned())?
            .join("t3-gpui")
            .join("server");
        Self::start_in_data_dir(executable, http, data_dir).await
    }

    async fn start_in_data_dir(
        executable: PathBuf,
        http: &reqwest::Client,
        data_dir: PathBuf,
    ) -> Result<Self, String> {
        let executable_metadata = std::fs::metadata(&executable).map_err(|error| {
            format!("managed server executable {} is unavailable: {error}", executable.display())
        })?;
        if !executable_metadata.is_file() {
            return Err(format!(
                "managed server executable {} is not a file",
                executable.display()
            ));
        }
        std::fs::create_dir_all(&data_dir)
            .map_err(|error| format!("create managed server data directory: {error}"))?;
        let port =
            loopback_port().map_err(|error| format!("allocate managed server port: {error}"))?;
        let base_url = Url::parse(&format!("http://127.0.0.1:{port}/"))
            .map_err(|error| format!("construct managed server URL: {error}"))?;
        let bootstrap_token = format!("{}{}", t3_client::new_id(), t3_client::new_id());
        let envelope = json!({
            "mode": "desktop",
            "noBrowser": true,
            "port": port,
            "host": "127.0.0.1",
            "t3Home": data_dir,
            "desktopBootstrapToken": bootstrap_token,
            "tailscaleServeEnabled": false,
            "tailscaleServePort": 443,
        });

        let mut command = Command::new(&executable);
        command
            .args(["--bootstrap-fd", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // Keep server diagnostics bounded by discarding them. The process
            // never receives application credentials on its command line.
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = command
            .spawn()
            .map_err(|error| format!("start managed server {}: {error}", executable.display()))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "managed server stdin was not available".to_owned())?;
        let encoded = serde_json::to_vec(&envelope)
            .map_err(|error| format!("encode managed server bootstrap: {error}"))?;
        stdin
            .write_all(&encoded)
            .await
            .map_err(|error| format!("send managed server bootstrap: {error}"))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| format!("finish managed server bootstrap: {error}"))?;
        stdin
            .shutdown()
            .await
            .map_err(|error| format!("close managed server bootstrap input: {error}"))?;
        drop(stdin);

        wait_until_ready(&mut child, http, &base_url).await?;
        let pairing = PairingLink::parse(&format!("{base_url} {bootstrap_token}"))
            .map_err(|error| format!("parse managed server credentials: {error}"))?;
        let credentials = t3_client::auth::pair(http, &pairing, CLIENT_LABEL)
            .await
            .map_err(|error| format!("authenticate with managed server: {error}"))?;

        Ok(Self { child, bootstrap_token, credentials, base_url })
    }

    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Check whether the owned child process has exited.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        self.child.try_wait().map_err(|error| format!("check managed server process: {error}"))
    }

    /// Re-exchange the in-memory bootstrap secret if the access token expires.
    /// The secret and resulting credentials are never written to disk.
    pub async fn reauthenticate(&mut self, http: &reqwest::Client) -> Result<(), String> {
        let pairing = PairingLink::parse(&format!("{} {}", self.base_url, self.bootstrap_token))
            .map_err(|error| format!("parse managed server credentials: {error}"))?;
        let credentials = t3_client::auth::pair(http, &pairing, CLIENT_LABEL)
            .await
            .map_err(|error| format!("reauthenticate with managed server: {error}"))?;
        self.credentials = credentials;
        Ok(())
    }

    /// Stop this server process and wait for its exit. Dropping the wrapper
    /// also kills the child through `kill_on_drop`.
    pub async fn shutdown(mut self) -> Result<(), String> {
        if self.try_wait()?.is_none() {
            self.child.kill().await.map_err(|error| format!("stop managed server: {error}"))?;
        }
        self.child
            .wait()
            .await
            .map(|_| ())
            .map_err(|error| format!("wait for managed server shutdown: {error}"))
    }
}

fn loopback_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_until_ready(
    child: &mut Child,
    http: &reqwest::Client,
    base_url: &Url,
) -> Result<(), String> {
    let started = Instant::now();
    let probe_url = base_url
        .join(".well-known/t3/environment")
        .map_err(|error| format!("construct managed server health URL: {error}"))?;
    loop {
        if let Some(status) =
            child.try_wait().map_err(|error| format!("check managed server startup: {error}"))?
        {
            return Err(format!("managed server exited before becoming ready: {status}"));
        }
        if started.elapsed() >= READY_TIMEOUT {
            return Err("managed server did not become ready within 60 seconds".into());
        }
        let ready = http
            .get(probe_url.clone())
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if ready {
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

    #[tokio::test]
    async fn missing_executable_reports_startup_error_without_running_a_server() {
        let dir = isolated_dir();
        let http = reqwest::Client::new();
        let result = ManagedServer::start_in_data_dir(
            dir.join("missing-server-executable"),
            &http,
            dir.clone(),
        )
        .await;
        let error = match result {
            Ok(_) => panic!("missing server executable unexpectedly started"),
            Err(error) => error,
        };
        assert!(error.contains("executable"));
        assert!(!dir.exists(), "missing executable must fail before creating server data");
    }
}
