use super::{AppState, ApiError, ApiResult};
use crate::security::{protect_secret, unprotect_secret};
use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::{
        header::{COOKIE, SET_COOKIE, USER_AGENT},
        HeaderMap, HeaderValue, StatusCode,
    },
    Json,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};
use uuid::Uuid;

const SESSION_COOKIE: &str = "soop_session";
const CSRF_HEADER: &str = "x-csrf-token";
const PBKDF2_ITERATIONS: u64 = 310_000;
const LOGIN_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOGIN_BLOCK: Duration = Duration::from_secs(15 * 60);
const LOGIN_MAX_FAILURES: u32 = 5;

#[derive(Clone)]
pub(crate) struct AuthManager {
    db_path: PathBuf,
    failures: std::sync::Arc<Mutex<HashMap<String, FailureState>>>,
    session_hours: i64,
}

#[derive(Debug)]
struct FailureState {
    window_started: Instant,
    failures: u32,
    blocked_until: Option<Instant>,
}

#[derive(Debug, Clone)]
struct SessionIdentity {
    session_id: String,
    user_id: i64,
    username: String,
    csrf_token: String,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetupRequest {
    username: String,
    password: String,
    password_confirm: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
    new_password_confirm: String,
}

impl AuthManager {
    pub(crate) fn open(db_path: PathBuf) -> Result<Self> {
        let session_hours = env::var("SOOP_SESSION_HOURS")
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(12)
            .clamp(1, 168);
        let manager = Self {
            db_path,
            failures: std::sync::Arc::new(Mutex::new(HashMap::new())),
            session_hours,
        };
        manager.initialize()?;
        Ok(manager)
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("failed to open auth database {}", self.db_path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        Ok(conn)
    }

    fn initialize(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS auth_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL UNIQUE COLLATE NOCASE,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                password_changed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS auth_sessions (
                session_id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                secret_protected TEXT NOT NULL,
                csrf_token TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                user_agent TEXT,
                FOREIGN KEY(user_id) REFERENCES auth_users(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS ix_auth_sessions_user ON auth_sessions(user_id);
            CREATE INDEX IF NOT EXISTS ix_auth_sessions_expiry ON auth_sessions(expires_at);

            CREATE TABLE IF NOT EXISTS auth_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event TEXT NOT NULL,
                username TEXT,
                success INTEGER NOT NULL,
                detail TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS ix_auth_audit_created ON auth_audit(created_at DESC);
            "#,
        )?;
        self.cleanup_expired()?;
        Ok(())
    }

    fn configured(&self) -> Result<bool> {
        let conn = self.conn()?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM auth_users", [], |row| row.get(0))?;
        Ok(count > 0)
    }

    fn cleanup_expired(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM auth_sessions WHERE expires_at <= ?1",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn audit(&self, event: &str, username: Option<&str>, success: bool, detail: Option<&str>) {
        if let Ok(conn) = self.conn() {
            let _ = conn.execute(
                "INSERT INTO auth_audit(event,username,success,detail,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![event, username, i64::from(success), detail, Utc::now().to_rfc3339()],
            );
        }
    }

    fn create_session(&self, user_id: i64, username: &str, headers: &HeaderMap) -> Result<(SessionIdentity, String)> {
        self.cleanup_expired()?;
        let session_id = random_token();
        let secret = random_token();
        let protected = protect_secret(&secret).context("failed to protect session secret")?;
        let csrf_token = random_token();
        let now = Utc::now();
        let expires = now + ChronoDuration::hours(self.session_hours);
        let user_agent = headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .chars()
            .take(512)
            .collect::<String>();
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO auth_sessions(session_id,user_id,secret_protected,csrf_token,created_at,expires_at,last_seen_at,user_agent) VALUES(?1,?2,?3,?4,?5,?6,?5,?7)",
            params![session_id, user_id, protected, csrf_token, now.to_rfc3339(), expires.to_rfc3339(), user_agent],
        )?;
        let identity = SessionIdentity {
            session_id: session_id.clone(),
            user_id,
            username: username.to_string(),
            csrf_token,
            expires_at: expires.to_rfc3339(),
        };
        let cookie = session_cookie(&format!("{session_id}.{secret}"), self.session_hours, request_is_https(headers));
        Ok((identity, cookie))
    }

    fn session_from_headers(&self, headers: &HeaderMap) -> Result<Option<SessionIdentity>> {
        let Some(value) = cookie_value(headers, SESSION_COOKIE) else { return Ok(None); };
        let Some((session_id, secret)) = value.split_once('.') else { return Ok(None); };
        if session_id.len() > 160 || secret.len() > 160 || session_id.is_empty() || secret.is_empty() {
            return Ok(None);
        }

        let conn = self.conn()?;
        let row = conn
            .query_row(
                r#"SELECT s.user_id,u.username,s.secret_protected,s.csrf_token,s.expires_at
                   FROM auth_sessions s JOIN auth_users u ON u.id=s.user_id
                   WHERE s.session_id=?1"#,
                params![session_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((user_id, username, protected, csrf_token, expires_at)) = row else { return Ok(None); };

        let expired = DateTime::parse_from_rfc3339(&expires_at)
            .map(|value| value.with_timezone(&Utc) <= Utc::now())
            .unwrap_or(true);
        if expired {
            let _ = conn.execute("DELETE FROM auth_sessions WHERE session_id=?1", params![session_id]);
            return Ok(None);
        }

        let expected = match unprotect_secret(&protected, "AUTH_SESSION") {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        if !constant_time_eq(secret.as_bytes(), expected.as_bytes()) {
            return Ok(None);
        }

        let _ = conn.execute(
            "UPDATE auth_sessions SET last_seen_at=?2 WHERE session_id=?1",
            params![session_id, Utc::now().to_rfc3339()],
        );
        Ok(Some(SessionIdentity {
            session_id: session_id.to_string(),
            user_id,
            username,
            csrf_token,
            expires_at,
        }))
    }

    pub(crate) fn authorize_session(&self, headers: &HeaderMap) -> ApiResult<()> {
        let session = self
            .session_from_headers(headers)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "login required".to_string()))?;
        let supplied = headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if supplied.is_empty() || !constant_time_eq(supplied.as_bytes(), session.csrf_token.as_bytes()) {
            return Err((StatusCode::FORBIDDEN, "invalid CSRF token".to_string()));
        }
        Ok(())
    }

    fn require_session(&self, headers: &HeaderMap) -> ApiResult<SessionIdentity> {
        let session = self
            .session_from_headers(headers)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "login required".to_string()))?;
        let supplied = headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if supplied.is_empty() || !constant_time_eq(supplied.as_bytes(), session.csrf_token.as_bytes()) {
            return Err((StatusCode::FORBIDDEN, "invalid CSRF token".to_string()));
        }
        Ok(session)
    }

    fn login_blocked(&self, key: &str) -> bool {
        let Ok(mut failures) = self.failures.lock() else { return true; };
        let now = Instant::now();
        let Some(state) = failures.get_mut(key) else { return false; };
        if let Some(until) = state.blocked_until {
            if now < until {
                return true;
            }
            state.blocked_until = None;
            state.failures = 0;
            state.window_started = now;
        }
        if now.duration_since(state.window_started) > LOGIN_WINDOW {
            state.failures = 0;
            state.window_started = now;
        }
        false
    }

    fn record_login_failure(&self, key: &str) {
        let Ok(mut failures) = self.failures.lock() else { return; };
        let now = Instant::now();
        let state = failures.entry(key.to_string()).or_insert(FailureState {
            window_started: now,
            failures: 0,
            blocked_until: None,
        });
        if now.duration_since(state.window_started) > LOGIN_WINDOW {
            state.window_started = now;
            state.failures = 0;
            state.blocked_until = None;
        }
        state.failures = state.failures.saturating_add(1);
        if state.failures >= LOGIN_MAX_FAILURES {
            state.blocked_until = Some(now + LOGIN_BLOCK);
        }
    }

    fn clear_login_failures(&self, key: &str) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.remove(key);
        }
    }
}

pub(crate) async fn api_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let configured = state.auth.configured().map_err(internal)?;
    let session = state.auth.session_from_headers(&headers).map_err(internal)?;
    Ok(Json(match session {
        Some(session) => json!({
            "configured": configured,
            "authenticated": true,
            "username": session.username,
            "csrf_token": session.csrf_token,
            "expires_at": session.expires_at,
            "session_hours": state.auth.session_hours
        }),
        None => json!({
            "configured": configured,
            "authenticated": false,
            "username": Value::Null,
            "csrf_token": Value::Null,
            "expires_at": Value::Null,
            "session_hours": state.auth.session_hours
        }),
    }))
}

pub(crate) async fn api_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    if state.auth.configured().map_err(internal)? {
        return Err((StatusCode::CONFLICT, "administrator account is already configured".into()));
    }
    validate_username(&body.username).map_err(bad_request)?;
    validate_password(&body.password).map_err(bad_request)?;
    if body.password != body.password_confirm {
        return Err((StatusCode::BAD_REQUEST, "password confirmation does not match".into()));
    }
    let username = body.username.trim();
    let password_hash = hash_password(&body.password).map_err(internal)?;
    let now = Utc::now().to_rfc3339();
    let conn = state.auth.conn().map_err(internal)?;
    if conn.query_row("SELECT COUNT(*) FROM auth_users", [], |row| row.get::<_, i64>(0)).map_err(internal)? > 0 {
        return Err((StatusCode::CONFLICT, "administrator account is already configured".into()));
    }
    conn.execute(
        "INSERT INTO auth_users(username,password_hash,created_at,password_changed_at) VALUES(?1,?2,?3,?3)",
        params![username, password_hash, now],
    )
    .map_err(internal)?;
    let user_id = conn.last_insert_rowid();
    drop(conn);
    let (session, cookie) = state.auth.create_session(user_id, username, &headers).map_err(internal)?;
    state.auth.audit("setup", Some(username), true, Some("first administrator created"));
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, HeaderValue::from_str(&cookie).map_err(internal)?);
    Ok((response_headers, Json(session_json(&session))))
}

pub(crate) async fn api_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    if !state.auth.configured().map_err(internal)? {
        return Err((StatusCode::PRECONDITION_REQUIRED, "administrator account is not configured".into()));
    }
    let username = body.username.trim();
    let key = login_failure_key(username, &headers);
    if state.auth.login_blocked(&key) {
        state.auth.audit("login", Some(username), false, Some("rate limited"));
        return Err((StatusCode::TOO_MANY_REQUESTS, "too many failed login attempts; try again later".into()));
    }
    let conn = state.auth.conn().map_err(internal)?;
    let row = conn
        .query_row(
            "SELECT id,username,password_hash FROM auth_users WHERE username=?1 COLLATE NOCASE",
            params![username],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        )
        .optional()
        .map_err(internal)?;
    drop(conn);

    let valid = row
        .as_ref()
        .is_some_and(|(_, _, hash)| verify_password(&body.password, hash).unwrap_or(false));
    if !valid {
        state.auth.record_login_failure(&key);
        state.auth.audit("login", Some(username), false, Some("invalid credentials"));
        return Err((StatusCode::UNAUTHORIZED, "invalid username or password".into()));
    }
    let (user_id, canonical_username, _) = row.unwrap();
    state.auth.clear_login_failures(&key);
    let (session, cookie) = state.auth.create_session(user_id, &canonical_username, &headers).map_err(internal)?;
    state.auth.audit("login", Some(&canonical_username), true, None);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, HeaderValue::from_str(&cookie).map_err(internal)?);
    Ok((response_headers, Json(session_json(&session))))
}

pub(crate) async fn api_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let session = state.auth.require_session(&headers)?;
    let conn = state.auth.conn().map_err(internal)?;
    conn.execute("DELETE FROM auth_sessions WHERE session_id=?1", params![session.session_id]).map_err(internal)?;
    state.auth.audit("logout", Some(&session.username), true, None);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, HeaderValue::from_str(&clear_session_cookie(request_is_https(&headers))).map_err(internal)?);
    Ok((response_headers, Json(json!({"ok": true}))))
}

pub(crate) async fn api_logout_all(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let session = state.auth.require_session(&headers)?;
    let conn = state.auth.conn().map_err(internal)?;
    conn.execute("DELETE FROM auth_sessions WHERE user_id=?1", params![session.user_id]).map_err(internal)?;
    state.auth.audit("logout_all", Some(&session.username), true, None);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, HeaderValue::from_str(&clear_session_cookie(request_is_https(&headers))).map_err(internal)?);
    Ok((response_headers, Json(json!({"ok": true}))))
}

pub(crate) async fn api_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let session = state.auth.require_session(&headers)?;
    validate_password(&body.new_password).map_err(bad_request)?;
    if body.new_password != body.new_password_confirm {
        return Err((StatusCode::BAD_REQUEST, "new password confirmation does not match".into()));
    }
    let conn = state.auth.conn().map_err(internal)?;
    let current_hash: String = conn
        .query_row("SELECT password_hash FROM auth_users WHERE id=?1", params![session.user_id], |row| row.get(0))
        .map_err(internal)?;
    if !verify_password(&body.current_password, &current_hash).map_err(internal)? {
        state.auth.audit("password_change", Some(&session.username), false, Some("current password mismatch"));
        return Err((StatusCode::UNAUTHORIZED, "current password is incorrect".into()));
    }
    let new_hash = hash_password(&body.new_password).map_err(internal)?;
    conn.execute(
        "UPDATE auth_users SET password_hash=?2,password_changed_at=?3 WHERE id=?1",
        params![session.user_id, new_hash, Utc::now().to_rfc3339()],
    )
    .map_err(internal)?;
    conn.execute("DELETE FROM auth_sessions WHERE user_id=?1", params![session.user_id]).map_err(internal)?;
    drop(conn);
    let (new_session, cookie) = state.auth.create_session(session.user_id, &session.username, &headers).map_err(internal)?;
    state.auth.audit("password_change", Some(&session.username), true, Some("all previous sessions invalidated"));
    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, HeaderValue::from_str(&cookie).map_err(internal)?);
    Ok((response_headers, Json(session_json(&new_session))))
}

fn session_json(session: &SessionIdentity) -> Value {
    json!({
        "authenticated": true,
        "username": session.username,
        "csrf_token": session.csrf_token,
        "expires_at": session.expires_at
    })
}

fn validate_username(value: &str) -> Result<()> {
    let value = value.trim();
    if !(3..=32).contains(&value.len()) {
        bail!("username must be 3 to 32 characters");
    }
    if !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')) {
        bail!("username may contain only letters, numbers, dot, underscore and hyphen");
    }
    Ok(())
}

fn validate_password(value: &str) -> Result<()> {
    if !(12..=128).contains(&value.len()) {
        bail!("password must be 12 to 128 characters");
    }
    if value.contains('\0') || value.contains('\r') || value.contains('\n') {
        bail!("password must be a single line");
    }
    Ok(())
}

fn random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn login_failure_key(username: &str, headers: &HeaderMap) -> String {
    let client = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|value| value.to_str().ok()))
        .unwrap_or("local")
        .trim();
    format!("{}|{}", username.trim().to_ascii_lowercase(), client)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for raw in headers.get_all(COOKIE) {
        let Ok(text) = raw.to_str() else { continue; };
        for part in text.split(';') {
            let Some((key, value)) = part.trim().split_once('=') else { continue; };
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn request_is_https(headers: &HeaderMap) -> bool {
    if headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|part| part.trim().eq_ignore_ascii_case("https")))
    {
        return true;
    }
    headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().split(';').any(|part| part.trim() == "proto=https"))
}

fn session_cookie(value: &str, hours: i64, secure: bool) -> String {
    let max_age = hours.saturating_mul(3600);
    format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn clear_session_cookie(secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn hash_password(password: &str) -> Result<String> {
    validate_password(password)?;
    let salt = Uuid::new_v4().as_bytes().to_vec();
    let derived = pbkdf2_sha256(password.as_bytes(), &salt, PBKDF2_ITERATIONS, 32)?;
    Ok(format!(
        "pbkdf2-sha256$v1${PBKDF2_ITERATIONS}${}${}",
        hex_encode(&salt),
        hex_encode(&derived)
    ))
}

fn verify_password(password: &str, encoded: &str) -> Result<bool> {
    let parts: Vec<&str> = encoded.split('$').collect();
    if parts.len() != 5 || parts[0] != "pbkdf2-sha256" || parts[1] != "v1" {
        return Ok(false);
    }
    let iterations = parts[2].parse::<u64>().context("invalid password hash iterations")?;
    if !(100_000..=2_000_000).contains(&iterations) {
        return Ok(false);
    }
    let salt = hex_decode(parts[3])?;
    let expected = hex_decode(parts[4])?;
    if salt.len() < 16 || expected.len() != 32 {
        return Ok(false);
    }
    let derived = pbkdf2_sha256(password.as_bytes(), &salt, iterations, expected.len())?;
    Ok(constant_time_eq(&derived, &expected))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        bail!("invalid hex length");
    }
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        output.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Ok(output)
}

fn hex_value(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => bail!("invalid hex character"),
    }
}

fn internal(err: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn bad_request(err: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, err.to_string())
}

#[cfg(windows)]
fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u64, output_len: usize) -> Result<Vec<u8>> {
    use std::ptr::null;
    use windows_sys::Win32::Security::Cryptography::{
        BCryptCloseAlgorithmProvider, BCryptDeriveKeyPBKDF2, BCryptOpenAlgorithmProvider,
        BCRYPT_ALG_HANDLE, BCRYPT_ALG_HANDLE_HMAC_FLAG, BCRYPT_SHA256_ALGORITHM,
    };

    let mut algorithm: BCRYPT_ALG_HANDLE = 0 as _;
    let status = unsafe {
        BCryptOpenAlgorithmProvider(
            &mut algorithm,
            BCRYPT_SHA256_ALGORITHM,
            null(),
            BCRYPT_ALG_HANDLE_HMAC_FLAG,
        )
    };
    if status != 0 {
        bail!("BCryptOpenAlgorithmProvider(SHA256/HMAC) failed: 0x{:08x}", status as u32);
    }

    struct Algorithm(BCRYPT_ALG_HANDLE);
    impl Drop for Algorithm {
        fn drop(&mut self) {
            unsafe { BCryptCloseAlgorithmProvider(self.0, 0); }
        }
    }
    let algorithm = Algorithm(algorithm);
    let mut output = vec![0u8; output_len];
    let password_len = u32::try_from(password.len()).context("password is too long")?;
    let salt_len = u32::try_from(salt.len()).context("salt is too long")?;
    let output_len_u32 = u32::try_from(output.len()).context("derived key is too long")?;
    let status = unsafe {
        BCryptDeriveKeyPBKDF2(
            algorithm.0,
            password.as_ptr(),
            password_len,
            salt.as_ptr(),
            salt_len,
            iterations,
            output.as_mut_ptr(),
            output_len_u32,
            0,
        )
    };
    if status != 0 {
        bail!("BCryptDeriveKeyPBKDF2 failed: 0x{:08x}", status as u32);
    }
    Ok(output)
}

#[cfg(not(windows))]
fn pbkdf2_sha256(_password: &[u8], _salt: &[u8], _iterations: u64, _output_len: usize) -> Result<Vec<u8>> {
    bail!("browser password authentication is currently available on Windows only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_validation_accepts_expected_characters() {
        assert!(validate_username("admin_01").is_ok());
        assert!(validate_username("ab").is_err());
        assert!(validate_username("admin name").is_err());
    }

    #[test]
    fn cookie_parser_extracts_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("x=1; soop_session=abc.def; y=2"));
        assert_eq!(cookie_value(&headers, SESSION_COOKIE).as_deref(), Some("abc.def"));
    }

    #[test]
    fn constant_time_compare_matches_equal_values() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[cfg(windows)]
    #[test]
    fn password_hash_round_trip() {
        let encoded = hash_password("phase10-test-password").unwrap();
        assert!(encoded.starts_with("pbkdf2-sha256$v1$"));
        assert!(verify_password("phase10-test-password", &encoded).unwrap());
        assert!(!verify_password("wrong-password-value", &encoded).unwrap());
    }
}
