from pathlib import Path


def read(path):
    return Path(path).read_text(encoding='utf-8')


def write(path, text):
    Path(path).write_text(text, encoding='utf-8', newline='\n')


def rep(text, old, new, label):
    if old not in text:
        raise SystemExit(f'patch target not found: {label}')
    return text.replace(old, new, 1)

# 1) Add a read-only session authorization path that validates the session
# cookie but intentionally does not require CSRF. Mutating REST APIs continue
# to use authorize_session()/require_session(), which still enforce CSRF.
p='rust-web/src/auth.rs'
s=read(p)
old='''    pub(crate) fn authorize_session(&self, headers: &HeaderMap) -> ApiResult<()> {
        let session = self
            .session_from_headers(headers)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "login required".to_string()))?;
        let supplied = headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if supplied.is_empty()
            || !constant_time_eq(supplied.as_bytes(), session.csrf_token.as_bytes())
        {
            return Err((StatusCode::FORBIDDEN, "invalid CSRF token".to_string()));
        }
        Ok(())
    }
'''
new='''    pub(crate) fn authorize_session_readonly(&self, headers: &HeaderMap) -> ApiResult<()> {
        self.session_from_headers(headers)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .ok_or((StatusCode::UNAUTHORIZED, "login required".to_string()))?;
        Ok(())
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
        if supplied.is_empty()
            || !constant_time_eq(supplied.as_bytes(), session.csrf_token.as_bytes())
        {
            return Err((StatusCode::FORBIDDEN, "invalid CSRF token".to_string()));
        }
        Ok(())
    }
'''
s=rep(s, old, new, 'readonly session auth')

# Add a Windows test proving read-only auth needs only the cookie and observes
# server-side session revocation, while mutating auth still requires CSRF.
needle='''    #[cfg(windows)]
    #[test]
    fn password_hash_round_trip() {
        let encoded = hash_password("phase10-test-password").unwrap();
        assert!(encoded.starts_with("pbkdf2-sha256$v1$"));
        assert!(verify_password("phase10-test-password", &encoded).unwrap());
        assert!(!verify_password("wrong-password-value", &encoded).unwrap());
    }
'''
replacement=needle+'''
    #[cfg(windows)]
    #[test]
    fn readonly_session_auth_skips_csrf_and_observes_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let auth = AuthManager::open(dir.path().join("auth.db")).unwrap();
        let now = Utc::now().to_rfc3339();
        let conn = auth.conn().unwrap();
        conn.execute(
            "INSERT INTO auth_users(username,password_hash,created_at,password_changed_at) VALUES(?1,?2,?3,?3)",
            params!["admin", "unused-test-hash", now],
        )
        .unwrap();
        let user_id = conn.last_insert_rowid();
        drop(conn);

        let request_headers = HeaderMap::new();
        let (session, cookie) = auth.create_session(user_id, "admin", &request_headers).unwrap();
        let cookie_pair = cookie.split(';').next().unwrap();
        let mut stream_headers = HeaderMap::new();
        stream_headers.insert(COOKIE, HeaderValue::from_str(cookie_pair).unwrap());

        assert!(auth.authorize_session_readonly(&stream_headers).is_ok());
        assert_eq!(
            auth.authorize_session(&stream_headers).unwrap_err().0,
            StatusCode::FORBIDDEN
        );

        let conn = auth.conn().unwrap();
        conn.execute(
            "DELETE FROM auth_sessions WHERE session_id=?1",
            params![session.session_id],
        )
        .unwrap();
        drop(conn);
        assert_eq!(
            auth.authorize_session_readonly(&stream_headers)
                .unwrap_err()
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
'''
s=rep(s, needle, replacement, 'readonly auth unit test')
write(p,s)

# 2) SSE authorization: local bypass or cookie-only read authorization.
# Revalidate cookie sessions every 5 seconds and terminate the stream when the
# session expires/is deleted. Local direct bypass has no session to revoke.
p='rust-web/src/realtime.rs'
s=read(p)
s=rep(s,
'''const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const LOG_LINES: usize = 160;

fn authorize_stream(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(());
    }
    // Native EventSource cannot attach the recovery Bearer header. Session-cookie
    // clients use SSE; recovery-token mode intentionally remains on REST polling.
    state.auth.authorize_session(headers)
}
''',
'''const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const SESSION_REVALIDATE_INTERVAL: Duration = Duration::from_secs(5);
const LOG_LINES: usize = 160;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamAuth {
    LocalBypass,
    Session,
}

fn authorize_stream(headers: &HeaderMap, state: &AppState) -> ApiResult<StreamAuth> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(StreamAuth::LocalBypass);
    }
    // SSE is a read-only GET. Native EventSource cannot attach X-CSRF-Token,
    // so validate the HttpOnly session cookie without weakening CSRF checks on
    // any mutating API. Recovery Bearer mode remains on REST polling because
    // native EventSource also cannot attach Authorization.
    state.auth.authorize_session_readonly(headers)?;
    Ok(StreamAuth::Session)
}
''','stream auth mode')

s=rep(s,
'''    authorize_stream(&headers, &state)?;

    let mut log_events = state.logs.subscribe();
    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(8);
    tokio::spawn(async move {
        let mut tick = interval(SNAPSHOT_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                event = log_events.recv() => match event {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            if sender.send(Ok(snapshot_event(&state).await)).await.is_err() {
                break;
            }
        }
    });
''',
'''    let auth_mode = authorize_stream(&headers, &state)?;
    let session_headers = (auth_mode == StreamAuth::Session).then_some(headers.clone());

    let mut log_events = state.logs.subscribe();
    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(8);
    tokio::spawn(async move {
        let mut tick = interval(SNAPSHOT_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut revalidate = interval(SESSION_REVALIDATE_INTERVAL);
        revalidate.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // The initial request was already authenticated above. Do not spend the
        // first loop iteration re-reading SQLite solely because interval() ticks
        // immediately on creation.
        revalidate.tick().await;
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = revalidate.tick(), if session_headers.is_some() => {
                    let Some(ref headers) = session_headers else { continue };
                    if state.auth.authorize_session_readonly(headers).is_err() {
                        break;
                    }
                    continue;
                }
                event = log_events.recv() => match event {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            if sender.send(Ok(snapshot_event(&state).await)).await.is_err() {
                break;
            }
        }
    });
''','periodic session revalidation')

# Add lightweight auth-mode tests that don't need a full AppState.
s=rep(s,
'''    #[tokio::test]
    async fn log_buffer_wakes_realtime_subscribers() {
        let logs = LogBuffer::new();
        let mut receiver = logs.subscribe();
        logs.push("realtime-test").await;
        assert!(receiver.recv().await.is_ok());
        assert_eq!(logs.tail(1).await, vec!["realtime-test"]);
    }
''',
'''    #[tokio::test]
    async fn log_buffer_wakes_realtime_subscribers() {
        let logs = LogBuffer::new();
        let mut receiver = logs.subscribe();
        logs.push("realtime-test").await;
        assert!(receiver.recv().await.is_ok());
        assert_eq!(logs.tail(1).await, vec!["realtime-test"]);
    }

    #[test]
    fn session_revalidation_is_shorter_than_keep_alive_window() {
        assert!(SESSION_REVALIDATE_INTERVAL < Duration::from_secs(15));
    }
''','realtime interval test')
write(p,s)

# Static sanity.
auth=read('rust-web/src/auth.rs')
rt=read('rust-web/src/realtime.rs')
assert 'authorize_session_readonly' in auth
assert 'SESSION_REVALIDATE_INTERVAL' in rt
assert 'state.auth.authorize_session_readonly(headers)' in rt
assert 'state.auth.authorize_session(headers)' not in rt
assert 'StreamAuth::Session' in rt
print('Phase 11 Codex review patch sanity: PASS')
