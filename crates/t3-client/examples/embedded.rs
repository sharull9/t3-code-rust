//! Embed the T3 server: spawn the self-contained `t3` executable, hand it a
//! bootstrap envelope on stdin, then log in with the bootstrap token.
//!
//! This is the handoff the Electron desktop app uses (`--bootstrap-fd`), see
//! `apps/desktop/src/backend/DesktopBackendConfiguration.ts` in t3code.
//!
//! cargo run -p t3-client --example embedded -- <path to t3(.exe)> [data dir] [port]

use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::json;
use t3_client::{Connection, Credentials, PairingLink, ShellState};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let exe = args.next().ok_or("usage: embedded <t3 executable> [data dir] [port]")?;
    let home = args.next().unwrap_or_else(|| {
        std::env::temp_dir().join("t3-embedded-home").to_string_lossy().into_owned()
    });
    let port: u16 = args.next().map(|p| p.parse()).transpose()?.unwrap_or(3898);

    // A reusable secret only this process knows; the server trusts it as a
    // desktop handoff and exchanges it for bearer tokens.
    let bootstrap_token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let envelope = json!({
        "mode": "desktop",
        "noBrowser": true,
        "port": port,
        "host": "127.0.0.1",
        "t3Home": home,
        "desktopBootstrapToken": bootstrap_token,
        "tailscaleServeEnabled": false,
        "tailscaleServePort": 443,
    });

    let started = Instant::now();
    let mut child = Command::new(&exe)
        .args(["--bootstrap-fd", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin.write_all(format!("{envelope}\n").as_bytes()).await?;
    stdin.shutdown().await?;
    drop(stdin);
    println!("spawned {exe} (pid {:?}), data in {home}", child.id());

    let base = format!("http://127.0.0.1:{port}/");
    let http = reqwest::Client::new();
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(format!("server exited early: {status}").into());
        }
        let probe = http
            .get(format!("{base}.well-known/t3/environment"))
            .timeout(Duration::from_secs(2))
            .send()
            .await;
        if probe.is_ok_and(|r| r.status().is_success()) {
            break;
        }
        if started.elapsed() > Duration::from_secs(60) {
            return Err("server did not become ready within 60s".into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    println!("server ready after {} ms", started.elapsed().as_millis());

    // The bootstrap token goes through the same exchange as a pairing token.
    let link = PairingLink::parse(&format!("{base} {bootstrap_token}"))?;
    let credentials: Credentials = t3_client::auth::pair(&http, &link, "T3 GPUI (embedded)").await?;
    println!("logged in, scopes: {}", credentials.scope);
    let again = t3_client::auth::pair(&http, &link, "T3 GPUI (embedded)").await;
    println!("bootstrap token reusable: {}", again.is_ok());

    let connection = Connection::connect(&http, &credentials).await?;
    let mut stream = connection.subscribe_shell()?;
    let mut shell = ShellState::default();
    while let Some(item) = stream.next().await {
        shell.apply(item?);
        if shell.synchronized {
            break;
        }
    }
    println!(
        "shell synchronized: {} projects, {} threads (seq {})",
        shell.projects.len(),
        shell.threads.len(),
        shell.sequence
    );

    child.kill().await?;
    println!("server stopped");
    Ok(())
}
