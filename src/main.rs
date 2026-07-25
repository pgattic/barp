mod app;
mod auth;
mod config;
mod pages;
mod storage;
mod systems;

use std::{
    env,
    io::{self, Read},
    net::SocketAddr,
    path::PathBuf,
};

use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use rand::rngs::OsRng;
use tracing::{error, info};

use crate::app::{load_state, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if env::args().nth(1).as_deref() == Some("hash-password") {
        hash_password_command()?;
        return Ok(());
    }

    let config_path = parse_config_path()?;
    let state = load_state(&config_path).await?;
    let addr = SocketAddr::from(([0, 0, 0, 0], state.config.port));
    let app = router(state);

    info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn hash_password_command() -> Result<(), Box<dyn std::error::Error>> {
    let password = match env::args().nth(2) {
        Some(password) => password,
        None => {
            let mut password = String::new();
            io::stdin().read_to_string(&mut password)?;
            password.trim_end_matches(['\r', '\n']).to_owned()
        }
    };
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| err.to_string())?;
    println!("{hash}");
    Ok(())
}

fn parse_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--config requires a path".into());
        }
    }
    Ok(PathBuf::from("config.json"))
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        error!("failed to listen for shutdown signal: {err}");
    }
}
