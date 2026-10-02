//! Headless smoke test: pair with a server and print the live shell stream.
//!
//! cargo run -p t3-client --example shell -- "http://localhost:3773/pair#token=..."

use t3_client::{Connection, PairingLink, ShellState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let link = std::env::args().nth(1).ok_or("usage: shell <pairing-link>")?;
    let http = reqwest::Client::new();
    let credentials =
        t3_client::auth::pair(&http, &PairingLink::parse(&link)?, "t3-client smoke").await?;
    println!("paired with {}", credentials.base_url);
    let connection = Connection::connect(&http, &credentials).await?;
    println!("connected");

    let mut shell = ShellState::default();
    let mut stream = connection.subscribe_shell()?;
    while let Some(item) = stream.next().await {
        match item {
            Ok(item) => {
                shell.apply(item);
                if shell.synchronized {
                    for project in &shell.projects {
                        println!("{} ({})", project.title, project.workspace_root);
                        for thread in shell.project_threads(&project.id) {
                            let status = thread
                                .session
                                .as_ref()
                                .map(|s| format!("{:?}", s.status))
                                .unwrap_or_default();
                            println!("  - {} {status}", thread.title);
                        }
                    }
                    println!("-- seq {} --", shell.sequence);
                }
            }
            Err(error) => eprintln!("skipped item: {error}"),
        }
    }
    Ok(())
}
