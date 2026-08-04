mod app;
mod auth;
mod config;
mod pages;
mod storage;
mod systems;

use std::{io::IsTerminal, net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::app::{load_state, router};

#[derive(Debug, Parser)]
#[command(version, about = "Boring Ahh ROM Player")]
struct Cli {
    /// Path to BARP's JSON configuration.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        default_value = "config.json"
    )]
    config: PathBuf,
}

#[tokio::main]
async fn main() {
    init_tracing();
    let cli = Cli::parse();

    if let Err(err) = run(cli).await {
        error!(error = %format!("{err:#}"), "BARP stopped with an error");
        std::process::exit(1);
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("barp=info,tower_http=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .init();
}

async fn run(cli: Cli) -> Result<()> {
    info!(config = %cli.config.display(), "loading configuration");
    let state = load_state(&cli.config).await?;
    let addr = SocketAddr::from(([0, 0, 0, 0], state.config.port));
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind HTTP listener to {addr}"))?;
    info!(address = %addr, "BARP is ready");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("HTTP server failed")?;
    info!("BARP stopped cleanly");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(err) = result {
                            warn!(%err, "failed to listen for Ctrl-C");
                        }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(err) => {
                warn!(%err, "failed to listen for SIGTERM; waiting for Ctrl-C");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }

    #[cfg(not(unix))]
    if let Err(err) = tokio::signal::ctrl_c().await {
        warn!(%err, "failed to listen for Ctrl-C");
    }

    info!("shutdown requested");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_flag() {
        let cli = Cli::try_parse_from(["barp", "--config", "/tmp/barp.json"]).unwrap();
        assert_eq!(cli.config, PathBuf::from("/tmp/barp.json"));
    }
}
