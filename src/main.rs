use std::{
    collections::HashMap,
    env,
    io::{self, Read},
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use argon2::{
    password_hash::{PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use axum::{
    body::Body,
    extract::{Form, Path as AxumPath, Query, State},
    http::{
        header::{self, HeaderMap, HeaderValue},
        StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use bytes::Bytes;
use rand::{rngs::OsRng, RngCore};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
    sync::Mutex,
};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

const SESSION_COOKIE: &str = "barecade_session";

#[derive(RustEmbed)]
#[folder = "frontend/"]
struct Assets;

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    users: Arc<HashMap<String, User>>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
    save_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    roms_path: Arc<PathBuf>,
    saves_path: Arc<PathBuf>,
    _session_secret: Arc<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
struct Config {
    roms_path: PathBuf,
    saves_path: PathBuf,
    #[serde(default)]
    state_path: Option<PathBuf>,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    default_options: Options,
    #[serde(default)]
    users: Vec<UserConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Options {
    #[serde(default)]
    display_filter: Option<DisplayFilter>,
    #[serde(default)]
    integer_scaling: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DisplayFilter {
    Smooth,
    Pixelated,
    None,
}

#[derive(Debug, Deserialize)]
struct UserConfig {
    username: String,
    display_name: String,
    password_hash_file: PathBuf,
    #[serde(default)]
    option_overrides: Options,
}

#[derive(Clone)]
struct User {
    username: String,
    display_name: String,
    password_hash: String,
    options: Options,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    username: String,
    password: String,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NextQuery {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    username: String,
    display_name: String,
}

#[derive(Debug, Serialize)]
struct BrowseEntry {
    name: String,
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
struct BootstrapResponse {
    username: String,
    display_name: String,
    options: EffectiveOptions,
    systems: Vec<SystemInfo>,
}

#[derive(Debug, Serialize)]
struct EffectiveOptions {
    display_filter: DisplayFilter,
    integer_scaling: bool,
}

#[derive(Clone, Debug, Serialize)]
struct SystemInfo {
    folder: &'static str,
    label: &'static str,
    core: &'static str,
}

#[derive(Debug, thiserror::Error)]
enum AppError {
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

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/login", get(login_page).post(login_form))
        .route("/logout", post(logout_form))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/browse", get(browse_root))
        .route("/api/browse/*path", get(browse_path))
        .route("/api/roms/*path", get(get_rom))
        .route("/api/saves/*path", get(get_save).put(put_save))
        .route("/*path", get(content_or_asset))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        error!("failed to listen for shutdown signal: {err}");
    }
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

async fn load_state(config_path: &Path) -> Result<AppState, Box<dyn std::error::Error>> {
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

fn merge_options(defaults: &Options, overrides: &Options) -> Options {
    Options {
        display_filter: overrides
            .display_filter
            .clone()
            .or_else(|| defaults.display_filter.clone()),
        integer_scaling: overrides.integer_scaling.or(defaults.integer_scaling),
    }
}

async fn index(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    content_page(&state, &headers, "").await
}

async fn login_page(Query(query): Query<NextQuery>) -> Html<String> {
    Html(render_login_page(
        query.next.as_deref().unwrap_or("/"),
        None,
    ))
}

async fn login_form(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let next = sanitize_next(form.next.as_deref());
    match authenticate_user(&state, &form.username, &form.password).await {
        Ok(user) => {
            let token = new_token();
            state
                .sessions
                .lock()
                .await
                .insert(token.clone(), user.username);
            session_redirect_response(&token, &next).into_response()
        }
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Html(render_login_page(
                &next,
                Some("Invalid username or password"),
            )),
        )
            .into_response(),
    }
}

async fn logout_form(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = cookie_token(&headers) {
        state.sessions.lock().await.remove(&token);
    }
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_static("barecade_session=; Max-Age=0; HttpOnly; SameSite=Lax; Path=/"),
    );
    (response_headers, Redirect::to("/login")).into_response()
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

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user = state
        .users
        .get(&request.username)
        .ok_or(AppError::Unauthorized)?;
    let parsed_hash = PasswordHash::new(&user.password_hash).map_err(|_| AppError::Unauthorized)?;
    Argon2::default()
        .verify_password(request.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized)?;

    let token = new_token();
    state
        .sessions
        .lock()
        .await
        .insert(token.clone(), user.username.clone());

    let mut headers = HeaderMap::new();
    let cookie = format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/");
    headers.insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    Ok((
        headers,
        Json(LoginResponse {
            username: user.username.clone(),
            display_name: user.display_name.clone(),
        }),
    ))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = cookie_token(&headers) {
        state.sessions.lock().await.remove(&token);
    }
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_static("barecade_session=; Max-Age=0; HttpOnly; SameSite=Lax; Path=/"),
    );
    (response_headers, StatusCode::NO_CONTENT)
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let user = require_user(&state, &headers).await?;
    Ok(Json(BootstrapResponse {
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        options: effective_options(&user.options),
        systems: systems().to_vec(),
    }))
}

async fn browse_root(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    browse_impl(state, headers, "").await
}

async fn browse_path(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    browse_impl(state, headers, &path).await
}

async fn browse_impl(
    state: AppState,
    headers: HeaderMap,
    raw_path: &str,
) -> Result<impl IntoResponse, AppError> {
    require_user(&state, &headers).await?;
    let dir = join_checked(&state.roms_path, raw_path)?;
    let mut entries = fs::read_dir(&dir).await.map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => AppError::NotFound,
        _ => AppError::Internal(err.to_string()),
    })?;

    let mut out = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|err| AppError::Internal(err.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            out.push(BrowseEntry { name, kind: "dir" });
        } else if file_type.is_file() && is_rom_file(&entry.path()) {
            out.push(BrowseEntry { name, kind: "file" });
        }
    }
    out.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Json(out))
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
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return AppError::NotFound.into_response();
        }
        Err(err) => return AppError::Internal(err.to_string()).into_response(),
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

    let system = match validate_play_path(path) {
        Ok(system) => system,
        Err(err) => return err.into_response(),
    };
    let save_path = save_path_for_rom(path);
    let has_save = save_file_exists(state, &user.username, &format!("{save_path}.srm")).await;
    Html(render_play_page(
        path,
        &save_path,
        system.core,
        &effective_options(&user.options),
        has_save,
    ))
    .into_response()
}

async fn get_rom(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    require_user(&state, &headers).await?;
    validate_system_path(&path)?;
    let rom_path = join_checked(&state.roms_path, &path)?;
    if !is_rom_file(&rom_path) {
        return Err(AppError::BadRequest("unrecognized ROM extension".into()));
    }
    stream_file(rom_path, headers).await
}

async fn get_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    let user = require_user(&state, &headers).await?;
    validate_system_path(&path)?;
    validate_save_path(&path)?;
    let save_path = user_save_path(&state, &user.username, &path)?;
    stream_file(save_path, headers).await
}

async fn put_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let user = require_user(&state, &headers).await?;
    validate_system_path(&path)?;
    validate_save_path(&path)?;
    if body.is_empty() {
        return Err(AppError::BadRequest("refusing to write empty save".into()));
    }
    let lock = save_lock(&state, &user.username).await;
    let _guard = lock.lock().await;
    let save_path = user_save_path(&state, &user.username, &path)?;
    if let Some(parent) = save_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| AppError::Internal(err.to_string()))?;
    }
    let tmp = save_path.with_extension(format!(
        "{}.tmp.{}",
        save_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("save"),
        new_token()
    ));
    fs::write(&tmp, &body)
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?;
    fs::rename(&tmp, &save_path)
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_lock(state: &AppState, username: &str) -> Arc<Mutex<()>> {
    let mut locks = state.save_locks.lock().await;
    locks
        .entry(username.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn require_user(state: &AppState, headers: &HeaderMap) -> Result<User, AppError> {
    let token = cookie_token(headers).ok_or(AppError::Unauthorized)?;
    let sessions = state.sessions.lock().await;
    let username = sessions.get(&token).ok_or(AppError::Unauthorized)?;
    state
        .users
        .get(username)
        .cloned()
        .ok_or(AppError::Unauthorized)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE).then(|| value.to_owned())
    })
}

fn new_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

async fn maybe_user(state: &AppState, headers: &HeaderMap) -> Option<User> {
    let token = cookie_token(headers)?;
    let sessions = state.sessions.lock().await;
    let username = sessions.get(&token)?;
    state.users.get(username).cloned()
}

async fn authenticate_user(
    state: &AppState,
    username: &str,
    password: &str,
) -> Result<User, AppError> {
    let user = state.users.get(username).ok_or(AppError::Unauthorized)?;
    let parsed_hash = PasswordHash::new(&user.password_hash).map_err(|_| AppError::Unauthorized)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(user.clone())
}

fn session_redirect_response(token: &str, next: &str) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    let cookie = format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/");
    headers.insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    (headers, Redirect::to(next))
}

fn sanitize_next(next: Option<&str>) -> String {
    let Some(next) = next else {
        return "/".to_string();
    };
    if next.starts_with('/') && !next.contains("://") {
        next.to_string()
    } else {
        "/".to_string()
    }
}

fn validate_play_path(path: &str) -> Result<&'static SystemInfo, AppError> {
    validate_system_path(path)?;
    let rom_path = join_checked(Path::new(""), path)?;
    if !is_rom_file(&rom_path) {
        return Err(AppError::BadRequest("unrecognized ROM extension".into()));
    }
    let first = sanitize_segments(path)?
        .first()
        .cloned()
        .ok_or_else(|| AppError::BadRequest("missing system".into()))?;
    system_for_folder(&first).ok_or(AppError::BadRequest("unknown system".into()))
}

async fn render_browse_page(
    state: &AppState,
    user: &User,
    raw_path: &str,
) -> Result<String, AppError> {
    let dir = join_checked(&state.roms_path, raw_path)?;
    let mut dir_entries = fs::read_dir(&dir).await.map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => AppError::NotFound,
        _ => AppError::Internal(err.to_string()),
    })?;

    let mut out = Vec::new();
    while let Some(entry) = dir_entries
        .next_entry()
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|err| AppError::Internal(err.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            out.push(BrowseLink {
                href: content_href(&join_path(raw_path, &name)),
                label: format!("{name}/"),
                is_dir: true,
            });
        } else if file_type.is_file() && is_rom_file(&entry.path()) {
            out.push(BrowseLink {
                href: content_href(&join_path(raw_path, &name)),
                label: name,
                is_dir: false,
            });
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });

    let mut rows = String::new();
    for entry in out {
        rows.push_str(&format!(
            "<div class=\"row\"><a href=\"{}\">{}</a></div>",
            escape_html(&entry.href),
            escape_html(&entry.label)
        ));
    }

    Ok(format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Barecade</title>
    <style>
      body {{ font-family: sans-serif; margin: 1rem; }}
      .toolbar {{ display: flex; gap: 1rem; align-items: center; flex-wrap: wrap; }}
      .row a {{ display: block; padding: .25rem 0; }}
      .path {{ color: #444; }}
      .path a {{ color: inherit; text-decoration: none; }}
      .path a:hover {{ text-decoration: underline; }}
    </style>
  </head>
  <body>
    <main>
      <div class="toolbar">
        <strong>Barecade</strong>
        <span>{user}</span>
        <form method="post" action="/logout">
          <button type="submit">Log out</button>
        </form>
      </div>
      <h1>Library</h1>
      <p class="path">{crumbs}</p>
      <section>{rows}</section>
    </main>
  </body>
</html>"#,
        user = escape_html(&user.display_name),
        crumbs = path_crumbs(raw_path),
        rows = rows,
    ))
}

fn render_play_page(
    path: &str,
    save_path: &str,
    core: &str,
    options: &EffectiveOptions,
    has_save: bool,
) -> String {
    let filter = match options.display_filter {
        DisplayFilter::Pixelated => "pixelated",
        DisplayFilter::Smooth => "smooth",
        DisplayFilter::None => "none",
    };
    let integer_scaling = if options.integer_scaling { "1" } else { "0" };
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Barecade - {path}</title>
    <style>
      html, body {{ width: 100%; height: 100%; margin: 0; background: #000; overflow: hidden; }}
      #game {{ width: 100vw; height: 100vh; }}
    </style>
  </head>
  <body data-path="{path}" data-save-path="{save_path}" data-core="{core}" data-filter="{filter}" data-integer-scaling="{integer_scaling}" data-has-save="{has_save}">
    <div id="game"></div>
    <script src="/player.js"></script>
  </body>
</html>"#,
        path = escape_html(path),
        save_path = escape_html(save_path),
        core = escape_html(core),
        filter = escape_html(filter),
        integer_scaling = integer_scaling,
        has_save = if has_save { "1" } else { "0" },
    )
}

fn render_login_page(next: &str, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!("<p class=\"error\">{}</p>", escape_html(message)))
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Barecade - Login</title>
    <style>
      body {{ font-family: sans-serif; margin: 1rem; }}
      label {{ display: block; margin: 0.5rem 0; }}
      .error {{ color: #b00020; }}
    </style>
  </head>
  <body>
    <main>
      <h1>Barecade</h1>
      {error_html}
      <form method="post" action="/login">
        <input type="hidden" name="next" value="{next}">
        <label>Username <input name="username" autocomplete="username" required></label>
        <label>Password <input name="password" type="password" autocomplete="current-password" required></label>
        <button type="submit">Log in</button>
      </form>
    </main>
  </body>
</html>"#,
        error_html = error_html,
        next = escape_html(next),
    )
}

#[derive(Debug)]
struct BrowseLink {
    href: String,
    label: String,
    is_dir: bool,
}

fn content_href(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", encode_path(path))
    }
}

fn path_crumbs(path: &str) -> String {
    let mut html = format!("<a href=\"{}\">roms</a>", escape_html(&content_href("")));
    if path.is_empty() {
        return html;
    }

    let mut accumulated = String::new();
    for segment in path.split('/') {
        html.push_str(" / ");
        if !accumulated.is_empty() {
            accumulated.push('/');
        }
        accumulated.push_str(segment);
        html.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            escape_html(&content_href(&accumulated)),
            escape_html(segment)
        ));
    }
    html
}

fn join_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.to_string()
    } else {
        format!("{base}/{child}")
    }
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_system_path(path: &str) -> Result<(), AppError> {
    let segments = sanitize_segments(path)?;
    if let Some(first) = segments.first() {
        if system_for_folder(first).is_none() {
            return Err(AppError::BadRequest(format!(
                "unrecognized system folder: {first}"
            )));
        }
    }
    Ok(())
}

fn join_checked(base: &Path, raw_path: &str) -> Result<PathBuf, AppError> {
    let mut path = base.to_path_buf();
    for segment in sanitize_segments(raw_path)? {
        path.push(segment);
    }
    Ok(path)
}

fn sanitize_segments(raw_path: &str) -> Result<Vec<String>, AppError> {
    if raw_path.contains('\\') {
        return Err(AppError::BadRequest("invalid path separator".into()));
    }
    let path = Path::new(raw_path);
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => return Err(AppError::BadRequest("invalid path".into())),
        }
    }
    Ok(segments)
}

fn validate_save_path(path: &str) -> Result<(), AppError> {
    let name = sanitize_segments(path)?
        .last()
        .cloned()
        .ok_or_else(|| AppError::BadRequest("save path is empty".into()))?;
    if name.ends_with(".srm") || name.contains(".state") {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "save path must end in .srm or .stateN".into(),
        ))
    }
}

fn user_save_path(state: &AppState, username: &str, raw_path: &str) -> Result<PathBuf, AppError> {
    let mut base = (*state.saves_path).clone();
    base.push(username);
    join_checked(&base, raw_path)
}

async fn save_file_exists(state: &AppState, username: &str, raw_path: &str) -> bool {
    match user_save_path(state, username, raw_path) {
        Ok(path) => fs::metadata(path)
            .await
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false),
        Err(_) => false,
    }
}

async fn stream_file(path: PathBuf, headers: HeaderMap) -> Result<Response, AppError> {
    let metadata = fs::metadata(&path).await.map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => AppError::NotFound,
        _ => AppError::Internal(err.to_string()),
    })?;
    if !metadata.is_file() {
        return Err(AppError::NotFound);
    }
    let len = metadata.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(parse_range)
        .transpose()?;
    let (start, end, status) = match range {
        Some((start, end)) => {
            if start >= len {
                return Err(AppError::RangeNotSatisfiable);
            }
            (
                start,
                end.min(len.saturating_sub(1)),
                StatusCode::PARTIAL_CONTENT,
            )
        }
        None => (0, len.saturating_sub(1), StatusCode::OK),
    };
    let bytes_to_read = if len == 0 { 0 } else { end - start + 1 };
    let mut file = fs::File::open(&path)
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?;
    file.seek(SeekFrom::Start(start))
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?;
    let mut buf = vec![0_u8; bytes_to_read as usize];
    file.read_exact(&mut buf)
        .await
        .map_err(|err| AppError::Internal(err.to_string()))?;

    let mut builder = Response::builder().status(status);
    builder = builder.header(header::ACCEPT_RANGES, "bytes");
    builder = builder.header(header::CONTENT_LENGTH, bytes_to_read.to_string());
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    if let Some(mime) = mime_guess::from_path(&path).first() {
        builder = builder.header(header::CONTENT_TYPE, mime.as_ref());
    }
    builder
        .body(Body::from(buf))
        .map_err(|err| AppError::Internal(err.to_string()))
}

fn parse_range(value: &str) -> Result<(u64, u64), AppError> {
    let range = value
        .strip_prefix("bytes=")
        .ok_or_else(|| AppError::BadRequest("unsupported range unit".into()))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| AppError::BadRequest("invalid range".into()))?;
    if start.is_empty() {
        return Err(AppError::BadRequest(
            "suffix ranges are not supported".into(),
        ));
    }
    let start = start
        .parse::<u64>()
        .map_err(|_| AppError::BadRequest("invalid range start".into()))?;
    let end = if end.is_empty() {
        u64::MAX
    } else {
        end.parse::<u64>()
            .map_err(|_| AppError::BadRequest("invalid range end".into()))?
    };
    if end < start {
        return Err(AppError::RangeNotSatisfiable);
    }
    Ok((start, end))
}

fn effective_options(options: &Options) -> EffectiveOptions {
    EffectiveOptions {
        display_filter: options
            .display_filter
            .clone()
            .unwrap_or(DisplayFilter::Smooth),
        integer_scaling: options.integer_scaling.unwrap_or(false),
    }
}

fn system_for_folder(folder: &str) -> Option<&'static SystemInfo> {
    systems().iter().find(|system| system.folder == folder)
}

fn systems() -> &'static [SystemInfo] {
    &[
        SystemInfo {
            folder: "nes",
            label: "NES",
            core: "nes",
        },
        SystemInfo {
            folder: "snes",
            label: "SNES",
            core: "snes",
        },
        SystemInfo {
            folder: "gb",
            label: "Game Boy",
            core: "gb",
        },
        SystemInfo {
            folder: "gbc",
            label: "Game Boy Color",
            core: "gb",
        },
        SystemInfo {
            folder: "gba",
            label: "Game Boy Advance",
            core: "gba",
        },
        SystemInfo {
            folder: "n64",
            label: "Nintendo 64",
            core: "n64",
        },
    ]
}

fn is_rom_file(path: &Path) -> bool {
    let Some(ext) = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
    else {
        return false;
    };
    matches!(
        ext.as_str(),
        "nes"
            | "unif"
            | "sfc"
            | "smc"
            | "fig"
            | "swc"
            | "gb"
            | "gbc"
            | "gba"
            | "n64"
            | "z64"
            | "v64"
    )
}

fn save_path_for_rom(path: &str) -> String {
    Path::new(path)
        .with_extension("")
        .to_string_lossy()
        .into_owned()
}

fn default_port() -> u16 {
    3000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal() {
        assert!(sanitize_segments("../secret").is_err());
        assert!(sanitize_segments("nes/../../secret").is_err());
        assert!(sanitize_segments("/nes/game.nes").is_err());
        assert!(sanitize_segments("nes\\game.nes").is_err());
    }

    #[test]
    fn accepts_nested_normal_paths() {
        assert_eq!(
            sanitize_segments("nes/Mario/game.nes").unwrap(),
            vec!["nes", "Mario", "game.nes"]
        );
    }

    #[test]
    fn recognizes_nintendo_rom_extensions() {
        assert!(is_rom_file(Path::new("game.nes")));
        assert!(is_rom_file(Path::new("game.SFC")));
        assert!(is_rom_file(Path::new("game.z64")));
        assert!(!is_rom_file(Path::new("readme.txt")));
    }

    #[test]
    fn save_path_replaces_the_rom_extension() {
        assert_eq!(
            save_path_for_rom("nes/Super Mario Bros.nes"),
            "nes/Super Mario Bros"
        );
        assert_eq!(
            save_path_for_rom("snes/Game (Rev 1).en.sfc"),
            "snes/Game (Rev 1).en"
        );
    }

    #[test]
    fn content_links_use_the_filesystem_path_directly() {
        assert_eq!(content_href(""), "/");
        assert_eq!(content_href("nes"), "/nes");
        assert_eq!(
            content_href("nes/Super Mario Bros.nes"),
            "/nes/Super%20Mario%20Bros.nes"
        );
    }

    #[test]
    fn path_crumbs_link_each_directory_segment() {
        assert_eq!(path_crumbs(""), "<a href=\"/\">roms</a>");
        assert_eq!(
            path_crumbs("nes"),
            "<a href=\"/\">roms</a> / <a href=\"/nes\">nes</a>"
        );
        assert_eq!(
            path_crumbs("nes/Mario"),
            "<a href=\"/\">roms</a> / <a href=\"/nes\">nes</a> / <a href=\"/nes/Mario\">Mario</a>"
        );
    }

    #[test]
    fn rejects_unknown_system_folder() {
        assert!(validate_system_path("genesis/sonic.bin").is_err());
        assert!(validate_system_path("gba/metroid.gba").is_ok());
    }

    #[test]
    fn parses_simple_ranges() {
        assert_eq!(parse_range("bytes=10-20").unwrap(), (10, 20));
        assert_eq!(parse_range("bytes=10-").unwrap(), (10, u64::MAX));
        assert!(parse_range("items=10-20").is_err());
        assert!(parse_range("bytes=-20").is_err());
    }
}
