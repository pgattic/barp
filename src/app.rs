use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{ensure, Context};
use argon2::password_hash::PasswordHash;
use axum::{
    extract::{DefaultBodyLimit, Path as AxumPath, State},
    http::{
        header::{self, HeaderMap, HeaderName, HeaderValue},
        StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::{fs, sync::Mutex};
use tower_http::trace::{DefaultMakeSpan, DefaultOnFailure, DefaultOnResponse, TraceLayer};
use tracing::{error, info, warn, Level};

use crate::{
    auth::{self, maybe_user, LoginLimiter, User},
    config::{
        effective_options, merge_options, Config, EffectiveOptions, PasswordHashSource, UserConfig,
    },
    pages::{content_href, normalize_content_path, render_browse_page, render_play_page},
    storage::{self, join_checked, save_file_exists, save_path_for_rom, validate_play_path},
    systems::SystemRegistry,
};

#[derive(RustEmbed)]
#[folder = "frontend/"]
#[exclude = "emulatorjs/*"]
struct Assets;

/// Axum caps request bodies at 2 MiB by default, which silently rejects save
/// states from anything beefier than an 8-bit core (mGBA states run several
/// megabytes; N64/PSP are larger still).
const MAX_SAVE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: Arc<Config>,
    pub(crate) users: Arc<HashMap<String, User>>,
    pub(crate) sessions: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) login_limiter: Arc<LoginLimiter>,
    pub(crate) save_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    pub(crate) roms_path: Arc<PathBuf>,
    pub(crate) saves_path: Arc<PathBuf>,
    pub(crate) emulatorjs_path: Arc<PathBuf>,
    pub(crate) systems: Arc<SystemRegistry>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("too many requests")]
    TooManyRequests,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
    #[error("range not satisfiable")]
    RangeNotSatisfiable,
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::RangeNotSatisfiable => StatusCode::RANGE_NOT_SATISFIABLE,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = match &self {
            AppError::Internal(_) => "internal server error".to_string(),
            _ => self.to_string(),
        };
        match &self {
            AppError::Internal(detail) => {
                error!(status = status.as_u16(), error = %detail, "request failed");
            }
            AppError::BadRequest(detail) => {
                warn!(status = status.as_u16(), error = %detail, "invalid request");
            }
            _ => {}
        }
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

#[derive(Debug, Serialize)]
struct BootstrapResponse {
    username: String,
    options: EffectiveOptions,
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/login", get(auth::login_page).post(auth::login_form))
        .route("/logout", post(auth::logout_form))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/login", post(auth::login))
        .route("/api/logout", post(auth::logout))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/browse", get(storage::browse_root))
        .route("/api/browse/*path", get(storage::browse_path))
        .route("/api/roms/*path", get(storage::get_rom))
        .route(
            "/api/saves/*path",
            get(storage::get_save)
                .put(storage::put_save)
                .layer(DefaultBodyLimit::max(MAX_SAVE_BYTES)),
        )
        .route("/emulatorjs/data/*path", get(serve_emulatorjs))
        .route("/*path", get(content_or_asset))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::DEBUG))
                .on_response(DefaultOnResponse::new().level(Level::DEBUG))
                .on_failure(DefaultOnFailure::new().level(Level::ERROR)),
        )
        .with_state(state)
}

pub(crate) async fn load_state(config_path: &Path) -> anyhow::Result<AppState> {
    let config_text = fs::read_to_string(config_path)
        .await
        .with_context(|| format!("failed to read config file {}", config_path.display()))?;
    let config: Config = serde_json::from_str(&config_text)
        .with_context(|| format!("invalid JSON configuration in {}", config_path.display()))?;
    ensure!(
        !config.users.is_empty(),
        "configuration must contain at least one user"
    );

    let roms_path = config.roms_path.canonicalize().with_context(|| {
        format!(
            "roms_path could not be resolved ({})",
            config.roms_path.display()
        )
    })?;
    ensure!(
        roms_path.is_dir(),
        "roms_path is not a directory: {}",
        roms_path.display()
    );
    let _roms = fs::read_dir(&roms_path)
        .await
        .with_context(|| format!("roms_path is not readable: {}", roms_path.display()))?;

    fs::create_dir_all(&config.saves_path)
        .await
        .with_context(|| {
            format!(
                "failed to create saves_path {}",
                config.saves_path.display()
            )
        })?;
    let saves_path = config.saves_path.canonicalize().with_context(|| {
        format!(
            "saves_path could not be resolved ({})",
            config.saves_path.display()
        )
    })?;
    verify_saves_writable(&saves_path).await?;

    let emulatorjs_path = validate_emulatorjs_path(&config.emulatorjs_path)?;
    let systems = SystemRegistry::new(&emulatorjs_path, &config.system_mappings)
        .context("failed to load EmulatorJS system registry")?;
    validate_rom_folders(&roms_path, &systems).await?;

    let mut users = HashMap::new();
    for (username, user) in &config.users {
        validate_username(username)?;
        let password_hash = load_password_hash(username, user).await?;
        users.insert(
            username.clone(),
            User {
                username: username.clone(),
                password_hash,
                options: merge_options(&config.default_options, &user.option_overrides),
            },
        );
    }

    let options = effective_options(&config.default_options);
    info!(
        roms_path = %roms_path.display(),
        saves_path = %saves_path.display(),
        emulatorjs_path = %emulatorjs_path.display(),
        port = config.port,
        users = users.len(),
        systems = systems.len(),
        shader = %options.shader,
        smooth = options.smooth,
        integer_scale = options.integer_scale,
        "configuration validated"
    );

    Ok(AppState {
        config: Arc::new(config),
        users: Arc::new(users),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        login_limiter: Arc::new(LoginLimiter::new()),
        save_locks: Arc::new(Mutex::new(HashMap::new())),
        roms_path: Arc::new(roms_path),
        saves_path: Arc::new(saves_path),
        emulatorjs_path: Arc::new(emulatorjs_path),
        systems: Arc::new(systems),
    })
}

async fn load_password_hash(username: &str, user: &UserConfig) -> anyhow::Result<String> {
    let raw = match user.password_hash_source(username)? {
        PasswordHashSource::Inline(hash) => hash,
        PasswordHashSource::File(path) => fs::read_to_string(&path)
            .await
            .with_context(|| {
                format!(
                    "failed to read password hash for user {username} from {}",
                    path.display()
                )
            })?
            .trim()
            .to_owned(),
    };
    ensure!(
        !raw.is_empty(),
        "password hash for user {username} must not be empty"
    );
    let parsed_hash = PasswordHash::new(&raw)
        .map_err(|err| anyhow::anyhow!("invalid password hash for user {username}: {err}"))?;
    ensure!(
        parsed_hash.algorithm.as_str().starts_with("argon2"),
        "password hash for user {username} is not an Argon2 hash"
    );
    Ok(raw)
}

async fn verify_saves_writable(path: &Path) -> anyhow::Result<()> {
    let probe = path.join(format!(".barp-write-test-{}", auth::new_token()));
    fs::write(&probe, b"ok")
        .await
        .with_context(|| format!("saves_path is not writable: {}", path.display()))?;
    fs::remove_file(&probe).await.with_context(|| {
        format!(
            "saves_path write probe could not be removed: {}",
            probe.display()
        )
    })?;
    info!(saves_path = %path.display(), "save directory is writable");
    Ok(())
}

async fn validate_rom_folders(path: &Path, systems: &SystemRegistry) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(path)
        .await
        .with_context(|| format!("failed to inspect roms_path {}", path.display()))?;
    let mut recognized = 0_usize;
    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| format!("failed to inspect roms_path {}", path.display()))?
    {
        let file_type = entry
            .file_type()
            .await
            .with_context(|| format!("failed to inspect ROM entry {}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().to_string();
        if folder.starts_with('.') {
            continue;
        }
        if let Some(system) = systems.for_folder(&folder) {
            recognized += 1;
            tracing::debug!(%folder, core = %system.core, "recognized ROM folder");
        } else {
            warn!(
                %folder,
                "ROM folder has no system mapping; hiding from browser"
            );
        }
    }
    info!(recognized, "ROM folders inspected");
    Ok(())
}

fn validate_username(username: &str) -> anyhow::Result<()> {
    ensure!(!username.is_empty(), "usernames must not be empty");
    let path = Path::new(username);
    ensure!(
        path.components().count() == 1
            && path
                .components()
                .next()
                .is_some_and(|component| matches!(component, std::path::Component::Normal(_))),
        "invalid username {username:?}: usernames must be a single path-safe component"
    );
    Ok(())
}

fn validate_emulatorjs_path(path: &Path) -> anyhow::Result<PathBuf> {
    let emulatorjs_path = path.canonicalize().map_err(|err| {
        anyhow::anyhow!(
            "emulatorjs_path could not be resolved ({}): {err}",
            path.display()
        )
    })?;
    if !emulatorjs_path.is_dir() {
        return Err(anyhow::anyhow!(
            "emulatorjs_path is not a directory: {}",
            emulatorjs_path.display()
        ));
    }
    if !emulatorjs_path.join("loader.js").is_file() {
        return Err(anyhow::anyhow!(
            "emulatorjs_path is missing loader.js: {}",
            emulatorjs_path.display()
        ));
    }
    if !emulatorjs_path.join("cores/cores.json").is_file() {
        return Err(anyhow::anyhow!(
            "emulatorjs_path is missing cores/cores.json: {}",
            emulatorjs_path.display()
        ));
    }
    Ok(emulatorjs_path)
}

async fn serve_emulatorjs(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    let file = join_checked(&state.emulatorjs_path, &path)?;
    storage::stream_file(file, headers).await
}

async fn index(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    content_page(&state, &headers, "").await
}

async fn content_or_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Response {
    if let Some(asset) = Assets::get(&path) {
        return asset_response(&path, asset);
    }

    let normalized = normalize_content_path(&path);
    if path.ends_with('/') && path != normalized && !normalized.is_empty() {
        return Redirect::to(&content_href(&normalized)).into_response();
    }

    content_page(&state, &headers, &normalized).await
}

fn asset_response(path: &str, asset: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).unwrap(),
    );
    (headers, asset.data).into_response()
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let user = auth::require_user(&state, &headers).await?;
    Ok(Json(BootstrapResponse {
        username: user.username.clone(),
        options: effective_options(&user.options),
    }))
}

async fn content_page(state: &AppState, headers: &HeaderMap, path: &str) -> Response {
    let Some(user) = maybe_user(state, headers).await else {
        let next = content_href(path);
        return Redirect::to(&format!("/login?next={}", urlencoding::encode(&next)))
            .into_response();
    };

    let target = match join_checked(&state.roms_path, path) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    let metadata = match fs::metadata(&target).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return AppError::NotFound.into_response();
        }
        Err(err) => return AppError::from(err).into_response(),
    };

    if metadata.is_dir() {
        return match render_browse_page(state, &user, path).await {
            Ok(html) => Html(html).into_response(),
            Err(err) => err.into_response(),
        };
    }
    if !metadata.is_file() {
        return AppError::NotFound.into_response();
    }

    let system = match validate_play_path(&state.systems, path) {
        Ok(system) => system,
        Err(err) => return err.into_response(),
    };
    let save_path = save_path_for_rom(path);
    let has_save = save_file_exists(state, &user.username, &format!("{save_path}.srm")).await;
    let mut response = Html(render_play_page(
        path,
        &save_path,
        &system.core,
        &effective_options(&user.options),
        has_save,
        system.threads,
    ))
    .into_response();
    if system.threads {
        response.headers_mut().insert(
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("cross-origin-embedder-policy"),
            HeaderValue::from_static("require-corp"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_must_be_single_safe_path_components() {
        assert!(validate_username("player1").is_ok());
        assert!(validate_username("").is_err());
        assert!(validate_username("../outside").is_err());
        assert!(validate_username("/absolute").is_err());
        assert!(validate_username("nested/player").is_err());
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_details_to_clients() {
        let response = AppError::Internal("/secret/path: permission denied".into()).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("internal server error"));
        assert!(!text.contains("/secret/path"));
    }
}
