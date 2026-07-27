use std::{
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{
        header::{self, HeaderMap},
        StatusCode,
    },
    response::Response,
    Json,
};
use bytes::Bytes;
use serde::Serialize;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
    sync::Mutex,
};
use tokio_util::io::ReaderStream;
use tracing::info;

use crate::{
    app::{AppError, AppState},
    auth::{new_token, require_user},
    systems::{System, SystemRegistry},
};

#[derive(Debug, Serialize)]
struct BrowseEntry {
    name: String,
    #[serde(rename = "type")]
    kind: &'static str,
}

pub(crate) async fn browse_root(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl axum::response::IntoResponse, AppError> {
    browse_impl(state, headers, "").await
}

pub(crate) async fn browse_path(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<impl axum::response::IntoResponse, AppError> {
    browse_impl(state, headers, &path).await
}

async fn browse_impl(
    state: AppState,
    headers: HeaderMap,
    raw_path: &str,
) -> Result<impl axum::response::IntoResponse, AppError> {
    require_user(&state, &headers).await?;
    let dir = join_checked(&state.roms_path, raw_path)?;
    let mut entries = fs::read_dir(&dir).await.map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => AppError::NotFound,
        _ => err.into(),
    })?;

    let mut out = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            out.push(BrowseEntry { name, kind: "dir" });
        } else if file_type.is_file()
            && state
                .systems
                .for_path(raw_path)
                .is_some_and(|system| state.systems.supports_file(system, &entry.path()))
        {
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

pub(crate) async fn get_rom(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    require_user(&state, &headers).await?;
    let system = validate_system_path(&state.systems, &path)?;
    let rom_path = join_checked(&state.roms_path, &path)?;
    if !state.systems.supports_file(system, &rom_path) {
        return Err(AppError::BadRequest("unrecognized ROM extension".into()));
    }
    stream_file(rom_path, headers).await
}

pub(crate) async fn get_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
) -> Result<Response, AppError> {
    let user = require_user(&state, &headers).await?;
    validate_system_path(&state.systems, &path)?;
    validate_save_path(&path)?;
    let save_path = user_save_path(&state, &user.username, &path)?;
    stream_file(save_path, headers).await
}

pub(crate) async fn put_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
    body: Bytes,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let user = require_user(&state, &headers).await?;
    validate_system_path(&state.systems, &path)?;
    validate_save_path(&path)?;
    if body.is_empty() {
        return Err(AppError::BadRequest("refusing to write empty save".into()));
    }
    let lock = save_lock(&state, &user.username).await;
    let _guard = lock.lock().await;
    let save_path = user_save_path(&state, &user.username, &path)?;
    if let Some(parent) = save_path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            AppError::Internal(format!(
                "failed to create save directory {}: {err}",
                parent.display()
            ))
        })?;
    }
    let tmp = save_path.with_extension(format!(
        "{}.tmp.{}",
        save_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("save"),
        new_token()
    ));
    fs::write(&tmp, &body).await.map_err(|err| {
        AppError::Internal(format!(
            "failed to write temporary save file {}: {err}",
            tmp.display()
        ))
    })?;
    if let Err(err) = fs::rename(&tmp, &save_path).await {
        let _ = fs::remove_file(&tmp).await;
        return Err(AppError::Internal(format!(
            "failed to commit save file {}: {err}",
            save_path.display()
        )));
    }
    info!(
        username = %user.username,
        save = %path,
        bytes = body.len(),
        "save stored"
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn save_lock(state: &AppState, username: &str) -> Arc<Mutex<()>> {
    let mut locks = state.save_locks.lock().await;
    locks
        .entry(username.to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

pub(crate) fn validate_play_path<'a>(
    registry: &'a SystemRegistry,
    path: &str,
) -> Result<&'a System, AppError> {
    let system = validate_system_path(registry, path)?;
    let rom_path = join_checked(Path::new(""), path)?;
    if !registry.supports_file(system, &rom_path) {
        return Err(AppError::BadRequest("unrecognized ROM extension".into()));
    }
    Ok(system)
}

pub(crate) fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn validate_system_path<'a>(
    registry: &'a SystemRegistry,
    path: &str,
) -> Result<&'a System, AppError> {
    let segments = sanitize_segments(path)?;
    let first = segments
        .first()
        .ok_or_else(|| AppError::BadRequest("missing system".into()))?;
    registry
        .for_folder(first)
        .ok_or_else(|| AppError::BadRequest(format!("unrecognized system folder: {first}")))
}

pub(crate) fn join_checked(base: &Path, raw_path: &str) -> Result<PathBuf, AppError> {
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

pub(crate) async fn save_file_exists(state: &AppState, username: &str, raw_path: &str) -> bool {
    match user_save_path(state, username, raw_path) {
        Ok(path) => fs::metadata(path)
            .await
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub(crate) async fn stream_file(path: PathBuf, headers: HeaderMap) -> Result<Response, AppError> {
    let metadata = fs::metadata(&path).await.map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => AppError::NotFound,
        _ => err.into(),
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
    let mut file = fs::File::open(&path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    let body = Body::from_stream(ReaderStream::new(file.take(bytes_to_read)));

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
        .body(body)
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
        return Err(AppError::BadRequest(
            "range end precedes range start".into(),
        ));
    }
    Ok((start, end))
}

pub(crate) fn save_path_for_rom(path: &str) -> String {
    Path::new(path)
        .with_extension("")
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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
    fn rejects_unknown_system_folder() {
        let emulatorjs_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/emulatorjs/data");
        let registry = SystemRegistry::new(&emulatorjs_path, &HashMap::new()).unwrap();
        assert!(validate_system_path(&registry, "unknown/sonic.bin").is_err());
        assert!(validate_system_path(&registry, "genesis/sonic.bin").is_ok());
    }

    #[test]
    fn parses_simple_ranges() {
        assert_eq!(parse_range("bytes=10-20").unwrap(), (10, 20));
        assert_eq!(parse_range("bytes=10-").unwrap(), (10, u64::MAX));
        assert!(parse_range("items=10-20").is_err());
        assert!(parse_range("bytes=-20").is_err());
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
}
