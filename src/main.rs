mod app;
mod auth;
mod config;
mod pages;
mod storage;
mod systems;

use std::{
    io::{self, IsTerminal, Read},
    net::SocketAddr,
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use clap::{Parser, Subcommand};
use rand::rngs::OsRng;
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

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the BARP web server (the default command).
    Serve,
    /// Generate an Argon2 password hash.
    HashPassword {
        /// Password to hash. Reads from stdin when omitted.
        password: Option<String>,
    },
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
        .with_ansi(io::stderr().is_terminal())
        .with_target(false)
        .init();
}

async fn run(cli: Cli) -> Result<()> {
    if let Some(Command::HashPassword { password }) = cli.command {
        hash_password_command(password)?;
        return Ok(());
    }

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

fn hash_password_command(argument: Option<String>) -> Result<()> {
    let password = match argument {
        Some(password) => password,
        None => {
            let mut password = String::new();
            io::stdin()
                .read_to_string(&mut password)
                .context("failed to read password from stdin")?;
            password.trim_end_matches(['\r', '\n']).to_owned()
        }
    };
    if password.is_empty() {
        bail!("password must not be empty");
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| anyhow::anyhow!("failed to hash password: {err}"))?;
    println!("{hash}");
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
    fn server_is_the_default_command() {
        let cli = Cli::try_parse_from(["barp", "--config", "/tmp/barp.json"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.config, PathBuf::from("/tmp/barp.json"));
    }

    #[test]
    fn parses_hash_password_subcommand() {
        let cli = Cli::try_parse_from(["barp", "hash-password", "secret"]).unwrap();
        match cli.command {
            Some(Command::HashPassword { password }) => {
                assert_eq!(password.as_deref(), Some("secret"));
            }
            _ => panic!("expected hash-password command"),
        }
    }
}
