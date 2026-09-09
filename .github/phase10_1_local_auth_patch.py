from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    s = p.read_text(encoding='utf-8')
    if s.count(old) < count:
        raise SystemExit(f'patch target not found: {path}\n{old[:240]}')
    p.write_text(s.replace(old, new, count), encoding='utf-8', newline='\n')

# Backend password validation + direct-loopback bypass.
auth = 'rust-web/src/auth.rs'
replace(auth,
'''    pub(crate) fn authorize_session(&self, headers: &HeaderMap) -> ApiResult<()> {''',
'''    pub(crate) fn local_bypass_allowed(&self, headers: &HeaderMap, bind: &str) -> Result<bool> {
        if !self.configured()? {
            return Ok(false);
        }
        let loopback_bind = bind.starts_with("127.0.0.1:")
            || bind.starts_with("[::1]:")
            || bind.starts_with("localhost:");
        if !loopback_bind {
            return Ok(false);
        }
        if [
            "forwarded",
            "x-forwarded-for",
            "x-forwarded-host",
            "x-forwarded-proto",
            "x-real-ip",
        ]
        .iter()
        .any(|name| headers.contains_key(*name))
        {
            return Ok(false);
        }
        let host = headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        Ok(host == "localhost"
            || host.starts_with("localhost:")
            || host == "127.0.0.1"
            || host.starts_with("127.0.0.1:")
            || host == "[::1]"
            || host.starts_with("[::1]:"))
    }

    pub(crate) fn authorize_session(&self, headers: &HeaderMap) -> ApiResult<()> {''')

replace(auth,
'''    let configured = state.auth.configured().map_err(internal)?;
    let session = state
        .auth
        .session_from_headers(&headers)
        .map_err(internal)?;''',
'''    let configured = state.auth.configured().map_err(internal)?;
    if configured
        && state
            .auth
            .local_bypass_allowed(&headers, &state.bind)
            .map_err(internal)?
    {
        return Ok(Json(json!({
            "configured": true,
            "authenticated": true,
            "username": "local",
            "csrf_token": "local-direct",
            "expires_at": Value::Null,
            "session_hours": state.auth.session_hours,
            "local_bypass": true
        })));
    }
    let session = state
        .auth
        .session_from_headers(&headers)
        .map_err(internal)?;''')

replace(auth,
'''            "session_hours": state.auth.session_hours
        }),''',
'''            "session_hours": state.auth.session_hours,
            "local_bypass": false
        }),''')
replace(auth,
'''            "session_hours": state.auth.session_hours
        }),
    }))''',
'''            "session_hours": state.auth.session_hours,
            "local_bypass": false
        }),
    }))''')

replace(auth,
'''    if !(12..=128).contains(&value.len()) {
        bail!("password must be 12 to 128 characters");
    }''',
'''    if !(4..=128).contains(&value.len()) {
        bail!("password must be 4 to 128 characters");
    }''')

replace(auth,
'''    fn cookie_parser_extracts_named_cookie() {''',
'''    fn password_validation_accepts_four_characters() {
        assert!(validate_password("1234").is_ok());
        assert!(validate_password("123").is_err());
    }

    #[test]
    fn cookie_parser_extracts_named_cookie() {''')

# Protected API authorization recognizes direct-local mode.
main = 'rust-web/src/main.rs'
replace(main,
'''fn authorize(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    let supplied = headers''',
'''fn authorize(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if state
        .auth
        .local_bypass_allowed(headers, &state.bind)
        .map_err(internal_error)?
    {
        return Ok(());
    }
    let supplied = headers''')

# Browser constraints and local-bypass label.
p10 = 'rust-web/web/phase10.js'
replace(p10, 'minlength="12" maxlength="128" required></label><label>비밀번호 확인<input name="confirm" type="password" autocomplete="new-password" minlength="12" maxlength="128" required></label><p class="hint">비밀번호는 12자 이상이어야 합니다.', 'minlength="4" maxlength="128" required></label><label>비밀번호 확인<input name="confirm" type="password" autocomplete="new-password" minlength="4" maxlength="128" required></label><p class="hint">비밀번호는 4자 이상이어야 합니다.')
replace(p10, 'autocomplete="new-password" minlength="12" maxlength="128" required></label><label>새 비밀번호 확인<input name="confirm" type="password" autocomplete="new-password" minlength="12" maxlength="128" required>', 'autocomplete="new-password" minlength="4" maxlength="128" required></label><label>새 비밀번호 확인<input name="confirm" type="password" autocomplete="new-password" minlength="4" maxlength="128" required>')
replace(p10,
'''  if(authState?.authenticated){
    const label=document.createElement('span');label.className='p10-user';label.textContent=`${authState.username} 로그인`;''',
'''  if(authState?.authenticated){
    if(authState.local_bypass){const label=document.createElement('span');label.className='p10-user';label.textContent='로컬 접속 · 로그인 생략';authBar.append(label);return}
    const label=document.createElement('span');label.className='p10-user';label.textContent=`${authState.username} 로그인`;''')
