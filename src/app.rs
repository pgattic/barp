use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::{Path as AxumPath, State},
    http::{
        header::{self, HeaderMap, HeaderName, HeaderValue},
        StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use rand::{rngs::OsRng, RngCore};
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::{fs, sync::Mutex};
use tower_http::trace::TraceLayer;

use crate::{
    auth::{self, maybe_user, User},
    config::{effective_options, merge_options, Config, EffectiveOptions},
    pages::{content_href, render_browse_page, render_play_page},
    storage::{self, join_checked, save_file_exists, save_path_for_rom, validate_play_path},
    systems::SystemRegistry,
};

#[derive(RustEmbed)]
#[folder = "frontend/"]
struct Assets;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: Arc<Config>,
    pub(crate) users: Arc<HashMap<String, User>>,
    pub(crate) sessions: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) save_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    pub(crate) roms_path: Arc<PathBuf>,
    pub(crate) saves_path: Arc<PathBuf>,
    pub(crate) systems: Arc<SystemRegistry>,
    _session_secret: Arc<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    #[error("unauthorized")]
    Unauthorized,
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
        let status = match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::RangeNotSatisfiable => StatusCode::RANGE_NOT_SATISFIABLE,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = self.to_string();
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

#[derive(Debug, Serialize)]
struct BootstrapResponse {
    username: String,
    display_name: String,
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
            get(storage::get_save).put(storage::put_save),
        )
        .route("/*path", get(content_or_asset))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub(crate) async fn load_state(config_path: &Path) -> Result<AppState, Box<dyn std::error::Error>> {
    let config_text = fs::read_to_string(config_path).await?;
    let config: Config = serde_json::from_str(&config_text)?;

    let roms_path = config.roms_path.canonicalize()?;
    if !roms_path.is_dir() {
        return Err(format!("roms_path is not a directory: {}", roms_path.display()).into());
    }
    fs::create_dir_all(&config.saves_path).await?;
    let saves_path = config.saves_path.canonicalize()?;
    let state_path = config
        .state_path
        .clone()
        .unwrap_or_else(|| config.saves_path.join(".barecade-state"));
    fs::create_dir_all(&state_path).await?;
    let session_secret = load_or_create_secret(&state_path).await?;
    let systems = SystemRegistry::new(&config.system_mappings)?;

    let mut users = HashMap::new();
    for user in &config.users {
        let password_hash = fs::read_to_string(&user.password_hash_file).await?;
        users.insert(
            user.username.clone(),
            User {
                username: user.username.clone(),
                display_name: user.display_name.clone(),
                password_hash: password_hash.trim().to_owned(),
                options: merge_options(&config.default_options, &user.option_overrides),
            },
        );
    }

    Ok(AppState {
        config: Arc::new(config),
        users: Arc::new(users),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        save_locks: Arc::new(Mutex::new(HashMap::new())),
        roms_path: Arc::new(roms_path),
        saves_path: Arc::new(saves_path),
        systems: Arc::new(systems),
        _session_secret: Arc::new(session_secret),
    })
}

async fn load_or_create_secret(state_path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = state_path.join("session-secret");
    if let Ok(secret) = fs::read(&path).await {
        return Ok(secret);
    }
    let mut secret = vec![0_u8; 32];
    OsRng.fill_bytes(&mut secret);
    fs::write(path, &secret).await?;
    Ok(secret)
}

async fn index(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    content_page(&state, &headers, "").await
}

async fn content_or_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Response {
    match Assets::get(&path) {
        Some(asset) => asset_response(&path, asset),
        None => content_page(&state, &headers, &path).await,
    }
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
        display_name: user.display_name.clone(),
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
