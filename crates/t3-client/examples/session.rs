//! Read-only health check using a saved GPUI credential file. Prints no tokens,
//! project names or message text.
//! cargo run -p t3-client --example session -- <credentials.json>
use std::time::Duration;
use t3_client::{Connection, Credentials, ShellState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: session <credentials.json>")?;
    let credentials: Credentials = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    if credentials.is_expired() {
        return Err("saved credentials expired; pair again in the app".into());
    }
    tokio::time::timeout(Duration::from_secs(15), async {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()?;
        let connection = Connection::connect(&http, &credentials).await?;
        let config = connection.server_config().await?;
        println!(
            "config decoded: {} providers, {} models",
            config.providers.len(),
            config.providers.iter().map(|p| p.models.len()).sum::<usize>()
        );
        let archived = connection.archived_shell().await?;
        println!(
            "archive decoded: {} projects, {} threads",
            archived.projects.len(),
            archived.threads.len()
        );
        let mut shell = ShellState::default();
        let mut stream = connection.subscribe_shell()?;
        while let Some(item) = stream.next().await {
            shell.apply(item?);
            if shell.synchronized {
                println!(
                    "shell synchronized: {} projects, {} threads",
                    shell.projects.len(),
                    shell.threads.len()
                );
                return Ok::<(), Box<dyn std::error::Error>>(());
            }
        }
        Err("shell stream ended before synchronization".into())
    })
    .await??;
    Ok(())
}
