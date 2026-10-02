//! Read-only health check using a saved GPUI credential file. Prints no tokens,
//! project names or message text.
//! cargo run -p t3-client --example session -- <credentials.json> [workspace-root]
use std::time::Duration;
use t3_client::{Connection, Credentials, ShellState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::args().nth(1).ok_or("usage: session <credentials.json> [workspace-root]")?;
    let workspace = std::env::args().nth(2);
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
        if let Some(cwd) = workspace {
            use t3_client::{WorkspaceRequest, WorkspaceResponse};
            for request in [
                WorkspaceRequest::BrowseDirectories { partial_path: "~".into(), cwd: None },
                WorkspaceRequest::ListDirectory { cwd: cwd.clone(), directory_path: None },
                WorkspaceRequest::ReadFile { cwd: cwd.clone(), relative_path: "README.md".into() },
                WorkspaceRequest::GitStatus { cwd: cwd.clone() },
                WorkspaceRequest::ListRefs { cwd: cwd.clone() },
                WorkspaceRequest::DiffPreview { cwd },
            ] {
                let result = request.execute(&connection).await?;
                let kind = match result {
                    WorkspaceResponse::BrowseDirectories(_) => "folder browser",
                    WorkspaceResponse::Directory(_) => "file listing",
                    WorkspaceResponse::File(_) => "file preview",
                    WorkspaceResponse::GitStatus(_) => "git status",
                    WorkspaceResponse::Refs(_) => "branch listing",
                    WorkspaceResponse::DiffPreview(_) => "diff preview",
                    _ => "other",
                };
                println!("workspace decoded: {kind}");
            }
        }
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
