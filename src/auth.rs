use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use axum::{
    extract::{ConnectInfo, Form, Query, State},
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
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{
    app::{AppError, AppState},
    config::Options,
    pages::{content_href, render_login_page},
};

const SESSION_COOKIE: &str = "barp_session";
/// Keep the browser cookie for a year. The server still forgets sessions on
/// restart; this only stops the browser from dropping the cookie early.
const SESSION_MAX_AGE_SECS: u64 = 60 * 60 * 24 * 365;
const LOGIN_MAX_FAILURES: usize = 5;
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub(crate) struct User {
    pub(crate) username: String,
    pub(crate) password_hash: String,
    pub(crate) options: Options,
}

/// Tracks failed logins by client IP and username.
pub(crate) struct LoginLimiter {
    max_failures: usize,
    window: Duration,
    failures: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl LoginLimiter {
    pub(crate) fn new() -> Self {
        Self {
            max_failures: LOGIN_MAX_FAILURES,
            window: LOGIN_WINDOW,
            failures: Mutex::new(HashMap::new()),
        }
    }

    async fn is_limited(&self, ip: &str, username: &str) -> bool {
        let mut failures = self.failures.lock().await;
        let now = Instant::now();
        self.bucket_limited(&mut failures, &ip_key(ip), now)
            || self.bucket_limited(&mut failures, &user_key(username), now)
    }

    async fn record_failure(&self, ip: &str, username: &str) {
        let mut failures = self.failures.lock().await;
        let now = Instant::now();
        self.push_failure(&mut failures, ip_key(ip), now);
        if !username.is_empty() {
            self.push_failure(&mut failures, user_key(username), now);
        }
    }

    async fn clear(&self, ip: &str, username: &str) {
        let mut failures = self.failures.lock().await;
        failures.remove(&ip_key(ip));
        failures.remove(&user_key(username));
    }

    fn bucket_limited(
        &self,
        failures: &mut HashMap<String, VecDeque<Instant>>,
        key: &str,
        now: Instant,
    ) -> bool {
        let Some(times) = failures.get_mut(key) else {
            return false;
        };
        prune_window(times, now, self.window);
        if times.is_empty() {
            failures.remove(key);
            return false;
        }
        times.len() >= self.max_failures
    }

    fn push_failure(
        &self,
        failures: &mut HashMap<String, VecDeque<Instant>>,
        key: String,
        now: Instant,
    ) {
        let times = failures.entry(key).or_default();
        prune_window(times, now, self.window);
        times.push_back(now);
    }
}

fn prune_window(times: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while times
        .front()
        .is_some_and(|stamp| now.saturating_duration_since(*stamp) > window)
    {
        times.pop_front();
    }
}

fn ip_key(ip: &str) -> String {
    format!("ip:{ip}")
}

fn user_key(username: &str) -> String {
    format!("user:{}", username.to_lowercase())
}

fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> String {
    // nginx appends the connecting client with $proxy_add_x_forwarded_for, so
    // the rightmost hop is the address our trusted proxy saw.
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(ip) = xff
            .split(',')
            .map(str::trim)
            .rfind(|part| !part.is_empty())
        {
            return ip.to_owned();
        }
    }
    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        return real_ip.to_owned();
    }
    peer.ip().to_string()
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
}

pub(crate) async fn login_page(Query(query): Query<NextQuery>) -> Html<String> {
    Html(render_login_page(
        query.next.as_deref().unwrap_or("/"),
        None,
    ))
}

pub(crate) async fn login_form(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let next = sanitize_next(form.next.as_deref());
    let ip = client_ip(&headers, peer);
    match attempt_login(&state, &ip, &form.username, &form.password).await {
        Ok(user) => {
            let token = new_token();
            state
                .sessions
                .lock()
                .await
                .insert(token.clone(), user.username);
            session_redirect_response(&token, &next).into_response()
        }
        Err(AppError::TooManyRequests) => (
            StatusCode::TOO_MANY_REQUESTS,
            Html(render_login_page(
                &next,
                Some("Too many failed login attempts. Try again in a few minutes."),
            )),
        )
            .into_response(),
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let ip = client_ip(&headers, peer);
    let user = attempt_login(&state, &ip, &request.username, &request.password).await?;
    let token = new_token();
    state
        .sessions
        .lock()
        .await
        .insert(token.clone(), user.username.clone());

    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, session_cookie_value(&token));
    Ok((
        headers,
        Json(LoginResponse {
            username: user.username,
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

async fn attempt_login(
    state: &AppState,
    ip: &str,
    username: &str,
    password: &str,
) -> Result<User, AppError> {
    if state.login_limiter.is_limited(ip, username).await {
        warn!(%ip, %username, "login rate limited");
        return Err(AppError::TooManyRequests);
    }

    match authenticate_user(state, username, password).await {
        Ok(user) => {
            state.login_limiter.clear(ip, username).await;
            Ok(user)
        }
        Err(AppError::Unauthorized) => {
            state.login_limiter.record_failure(ip, username).await;
            Err(AppError::Unauthorized)
        }
        Err(err) => Err(err),
    }
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

fn session_cookie_value(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}; Max-Age={SESSION_MAX_AGE_SECS}; HttpOnly; SameSite=Lax; Path=/"
    ))
    .expect("session cookie is ASCII")
}

fn session_redirect_response(token: &str, next: &str) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, session_cookie_value(token));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limits_after_too_many_failures_for_ip_or_username() {
        let limiter = LoginLimiter {
            max_failures: 3,
            window: Duration::from_secs(60),
            failures: Mutex::new(HashMap::new()),
        };

        for _ in 0..3 {
            assert!(!limiter.is_limited("1.2.3.4", "alice").await);
            limiter.record_failure("1.2.3.4", "alice").await;
        }
        assert!(limiter.is_limited("1.2.3.4", "bob").await);
        assert!(limiter.is_limited("9.9.9.9", "alice").await);

        limiter.clear("1.2.3.4", "alice").await;
        assert!(!limiter.is_limited("1.2.3.4", "bob").await);
        assert!(!limiter.is_limited("9.9.9.9", "alice").await);
    }

    #[test]
    fn prefers_rightmost_forwarded_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.1.1.1, 10.0.0.5"),
        );
        let peer = "127.0.0.1:3000".parse().unwrap();
        assert_eq!(client_ip(&headers, peer), "10.0.0.5");
    }
}
