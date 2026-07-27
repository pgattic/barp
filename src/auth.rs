use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use axum::{
    extract::{Form, Query, State},
    http::{
        header::{self, HeaderMap, HeaderValue},
        StatusCode,
    },
    response::{Html, IntoResponse, Redirect},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    app::{AppError, AppState},
    config::Options,
    pages::{content_href, render_login_page},
};

const SESSION_COOKIE: &str = "barp_session";

#[derive(Clone)]
pub(crate) struct User {
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) password_hash: String,
    pub(crate) options: Options,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginForm {
    username: String,
    password: String,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NextQuery {
    #[serde(default)]
    pub(crate) next: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    username: String,
    display_name: String,
}

pub(crate) async fn login_page(Query(query): Query<NextQuery>) -> Html<String> {
    Html(render_login_page(
        query.next.as_deref().unwrap_or("/"),
        None,
    ))
}

pub(crate) async fn login_form(
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

pub(crate) async fn logout_form(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(username) = remove_session(&state, &headers).await {
        info!(%username, "user logged out");
    }
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::SET_COOKIE, expired_cookie());
    (response_headers, Redirect::to("/login")).into_response()
}

pub(crate) async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user = authenticate_user(&state, &request.username, &request.password).await?;
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
            username: user.username,
            display_name: user.display_name,
        }),
    ))
}

pub(crate) async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(username) = remove_session(&state, &headers).await {
        info!(%username, "user logged out");
    }
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::SET_COOKIE, expired_cookie());
    (response_headers, StatusCode::NO_CONTENT)
}

pub(crate) async fn require_user(state: &AppState, headers: &HeaderMap) -> Result<User, AppError> {
    let token = cookie_token(headers).ok_or(AppError::Unauthorized)?;
    let sessions = state.sessions.lock().await;
    let username = sessions.get(&token).ok_or(AppError::Unauthorized)?;
    state
        .users
        .get(username)
        .cloned()
        .ok_or(AppError::Unauthorized)
}

pub(crate) async fn maybe_user(state: &AppState, headers: &HeaderMap) -> Option<User> {
    let token = cookie_token(headers)?;
    let sessions = state.sessions.lock().await;
    let username = sessions.get(&token)?;
    state.users.get(username).cloned()
}

pub(crate) fn new_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

async fn authenticate_user(
    state: &AppState,
    username: &str,
    password: &str,
) -> Result<User, AppError> {
    let Some(user) = state.users.get(username) else {
        warn!(%username, "login failed");
        return Err(AppError::Unauthorized);
    };
    let parsed_hash = PasswordHash::new(&user.password_hash).map_err(|err| {
        AppError::Internal(format!(
            "configured password hash for user {username} became invalid: {err}"
        ))
    })?;
    if Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_err()
    {
        warn!(%username, "login failed");
        return Err(AppError::Unauthorized);
    }
    info!(%username, "user logged in");
    Ok(user.clone())
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE).then(|| value.to_owned())
    })
}

async fn remove_session(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let token = cookie_token(headers)?;
    state.sessions.lock().await.remove(&token)
}

fn expired_cookie() -> HeaderValue {
    HeaderValue::from_static("barp_session=; Max-Age=0; HttpOnly; SameSite=Lax; Path=/")
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
        content_href("")
    }
}
