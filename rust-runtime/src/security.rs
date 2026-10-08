use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
#[cfg(test)]
use std::{fs, path::Path};
use uuid::Uuid;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod kick_lookup;

/// Internal Unix worker entry, before application bootstrap or CLI logging.
pub fn kick_secret_worker_entry() -> Option<i32> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        kick_lookup::worker_entry()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

pub(crate) async fn read_kick_token_cancellable(
    store: &crate::store::Store,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        kick_lookup::read(store, cancel).await
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            bail!("KICK 인증정보 조회가 취소되었습니다.");
        }
        let store = store.clone();
        tokio::task::spawn_blocking(move || read_kick_token(&store))
            .await
            .map_err(|_| anyhow::anyhow!("KICK 인증정보 조회 작업 실패"))?
    }
}

pub const DPAPI_PREFIX: &str = "dpapi:v1:";
pub const NATIVE_SECRET_PREFIX: &str = "native-secret:v1:";
#[cfg(windows)]
const DPAPI_ENTROPY: &[u8] = b"SOOPLiveDownloader:v1";
#[cfg(test)]
const SECRET_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
    "KICK_SESSION_TOKEN",
];

#[cfg(test)]
fn is_protected(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with(DPAPI_PREFIX) || normalized.starts_with(NATIVE_SECRET_PREFIX)
}

fn kick_secret_lock_file(store: &crate::store::Store) -> Result<std::fs::File> {
    let mut path = store.path().as_os_str().to_os_string();
    path.push(".kick-secret.lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(std::path::PathBuf::from(path))
        .context("인증정보 잠금 파일을 열지 못했습니다.")?;
    Ok(file)
}

pub(crate) fn kick_secret_guard(store: &crate::store::Store) -> Result<std::fs::File> {
    let file = kick_secret_lock_file(store)?;
    fs2::FileExt::try_lock_exclusive(&file)
        .context("다른 프로세스가 인증정보를 사용 중입니다. 잠시 후 재시도하세요.")?;
    Ok(file)
}

fn kick_secret_read_guard(
    store: &crate::store::Store,
    timeout: std::time::Duration,
) -> Result<std::fs::File> {
    let file = kick_secret_lock_file(store)?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match fs2::FileExt::try_lock_shared(&file) {
            Ok(()) => return Ok(file),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                if std::time::Instant::now() >= deadline {
                    bail!("인증정보 잠금 대기 시간이 초과되었습니다. 잠시 후 재시도하세요.");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error).context("인증정보 읽기 잠금 실패"),
        }
    }
}

pub(crate) fn read_kick_token(store: &crate::store::Store) -> Result<String> {
    read_kick_token_with(store, |value| unprotect_secret(value, "KICK_SESSION_TOKEN"))
}

fn read_kick_token_with(
    store: &crate::store::Store,
    unprotect: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let _guard = kick_secret_read_guard(store, std::time::Duration::from_secs(5))?;
    // Keep the same cross-process lock through native lookup, then release before HTTP.
    store.refresh_config_cache()?;
    let reference = store
        .setting_value("KICK_SESSION_TOKEN")?
        .unwrap_or_default();
    unprotect(&reference)
}

/// Read a current provider snapshot while native deletion is excluded.
pub(crate) fn read_provider_settings(
    store: &crate::store::Store,
    keys: &[&str],
    secret_keys: &[&str],
) -> Result<std::collections::BTreeMap<String, String>> {
    read_provider_settings_with(store, keys, secret_keys, unprotect_secret)
}

fn read_provider_settings_with(
    store: &crate::store::Store,
    keys: &[&str],
    secret_keys: &[&str],
    mut unprotect: impl FnMut(&str, &str) -> Result<String>,
) -> Result<std::collections::BTreeMap<String, String>> {
    let _guard = kick_secret_read_guard(store, std::time::Duration::from_secs(5))?;
    store.refresh_config_cache()?;
    let mut values = store.settings_for_keys(keys)?;
    for key in secret_keys {
        let reference = values.get(*key).map(String::as_str).unwrap_or_default();
        let plain = unprotect(reference, key)?;
        values.insert((*key).into(), plain);
    }
    Ok(values)
}

pub fn unprotect_secret(value: &str, name: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }

    let normalized = value.to_ascii_lowercase();
    if normalized.starts_with(DPAPI_PREFIX) {
        let encoded = &value[DPAPI_PREFIX.len()..];
        let cipher = BASE64
            .decode(encoded)
            .with_context(|| format!("{name} DPAPI payload is not valid base64"))?;
        let plain =
            dpapi_unprotect(&cipher).with_context(|| format!("{name} DPAPI decrypt failed"))?;
        return String::from_utf8(plain)
            .with_context(|| format!("{name} DPAPI plaintext is not UTF-8"));
    }

    if normalized.starts_with(NATIVE_SECRET_PREFIX) {
        let reference = parse_native_reference(&value[NATIVE_SECRET_PREFIX.len()..], name)?;
        return native_secret_load(reference)
            .with_context(|| format!("{name} native secret lookup failed"));
    }

    // Compatibility for databases created before protected secret storage.
    // New writes never intentionally fall back to plaintext.
    Ok(value.to_string())
}

/// Return an opaque native reference for deferred cleanup, never plaintext or DPAPI data.
pub(crate) fn native_cleanup_reference(value: &str, name: &str) -> Result<Option<String>> {
    let value = value.trim();
    if value.to_ascii_lowercase().starts_with(NATIVE_SECRET_PREFIX) {
        let reference = parse_native_reference(&value[NATIVE_SECRET_PREFIX.len()..], name)?;
        return Ok(Some(format!("{NATIVE_SECRET_PREFIX}{reference}")));
    }
    Ok(None)
}

/// Remove a referenced native credential before its SQLite reference is cleared.
pub fn delete_protected_secret(value: &str, name: &str) -> Result<()> {
    delete_protected_secret_with(value, name, native_secret_delete)
}

fn delete_protected_secret_with(
    value: &str,
    name: &str,
    delete: impl FnOnce(&str) -> Result<()>,
) -> Result<()> {
    let value = value.trim();
    if value.to_ascii_lowercase().starts_with(NATIVE_SECRET_PREFIX) {
        let reference = parse_native_reference(&value[NATIVE_SECRET_PREFIX.len()..], name)?;
        delete(reference).with_context(|| format!("{name} native secret deletion failed"))?;
    }
    // DPAPI and legacy plaintext have no separate credential-store entry.
    Ok(())
}

pub fn protect_secret(value: &str) -> Result<String> {
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("secret must be a single line");
    }
    if value.is_empty() {
        return Ok(String::new());
    }

    #[cfg(windows)]
    {
        let cipher = dpapi::protect(value.as_bytes()).context("DPAPI encrypt failed")?;
        Ok(format!("{DPAPI_PREFIX}{}", BASE64.encode(cipher)))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // Keep only an opaque reference in SQLite. The secret itself lives in
        // the current user's native credential store.
        let reference = Uuid::new_v4().hyphenated().to_string();
        native_secret_store(&reference, value).context("native secret store failed")?;
        Ok(format!("{NATIVE_SECRET_PREFIX}{reference}"))
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        bail!("native protected secret storage is unsupported on this operating system")
    }
}

/// Persist an opaque cleanup intent before creating a KICK native credential.
pub(crate) fn protect_secret_with_cleanup_intent(
    value: &str,
    retain: impl FnOnce(&str) -> Result<()>,
) -> Result<String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        protect_native_secret_with(value, retain, native_secret_store)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = retain;
        protect_secret(value)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub(crate) fn protect_native_secret_with(
    value: &str,
    retain: impl FnOnce(&str) -> Result<()>,
    store: impl FnOnce(&str, &str) -> Result<()>,
) -> Result<String> {
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("secret must be a single line");
    }
    if value.is_empty() {
        return Ok(String::new());
    }
    let reference = Uuid::new_v4().hyphenated().to_string();
    let protected = format!("{NATIVE_SECRET_PREFIX}{reference}");
    retain(&protected).context("native cleanup intent could not be saved")?;
    store(&reference, value).context("native secret store failed")?;
    Ok(protected)
}

fn parse_native_reference<'a>(value: &'a str, name: &str) -> Result<&'a str> {
    let value = value.trim();
    Uuid::parse_str(value).with_context(|| format!("{name} native secret reference is invalid"))?;
    Ok(value)
}

#[cfg(test)]
pub fn configured_secrets(path: &Path) -> Result<std::collections::BTreeMap<String, bool>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut result = std::collections::BTreeMap::new();
    for key in SECRET_KEYS {
        result.insert((*key).to_string(), false);
    }
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if SECRET_KEYS.contains(&key) {
            result.insert(key.to_string(), !value.trim().is_empty());
        }
    }
    Ok(result)
}

#[cfg(windows)]
fn dpapi_unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    dpapi::unprotect(ciphertext)
}

#[cfg(not(windows))]
fn dpapi_unprotect(_ciphertext: &[u8]) -> Result<Vec<u8>> {
    bail!("DPAPI-protected secrets can only be decrypted by the Windows user that created them")
}

#[cfg(target_os = "linux")]
fn native_secret_store(reference: &str, value: &str) -> Result<()> {
    linux_secret_service::store(reference, value)
}

#[cfg(target_os = "linux")]
fn native_secret_load(reference: &str) -> Result<String> {
    linux_secret_service::load(reference)
}

#[cfg(target_os = "macos")]
fn native_secret_store(reference: &str, value: &str) -> Result<()> {
    macos_keychain::store(reference, value)
}

#[cfg(target_os = "macos")]
fn native_secret_load(reference: &str) -> Result<String> {
    macos_keychain::load(reference)
}

#[cfg(windows)]
fn native_secret_load(_reference: &str) -> Result<String> {
    bail!("Unix native-secret references are not readable on Windows")
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn native_secret_load(_reference: &str) -> Result<String> {
    bail!("native protected secret storage is unsupported on this operating system")
}

#[cfg(target_os = "linux")]
fn native_secret_delete(reference: &str) -> Result<()> {
    linux_secret_service::delete(reference)
}

#[cfg(target_os = "macos")]
fn native_secret_delete(reference: &str) -> Result<()> {
    macos_keychain::delete(reference)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn native_secret_delete(_reference: &str) -> Result<()> {
    bail!("Unix native-secret references must be deleted in their original native credential store")
}

#[cfg(target_os = "linux")]
mod linux_secret_service {
    use anyhow::{Context, Result, bail};
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    const APPLICATION_ATTRIBUTE: &str = "stream-archive";

    fn command() -> Command {
        Command::new("secret-tool")
    }

    fn helper_context(action: &str) -> String {
        format!(
            "Linux Secret Service {action} requires `secret-tool` (libsecret-tools) and an available Secret Service session"
        )
    }

    pub(super) fn store(reference: &str, value: &str) -> Result<()> {
        let mut child = command()
            .args([
                "store",
                "--label=Stream Archive",
                "application",
                APPLICATION_ATTRIBUTE,
                "reference",
                reference,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| helper_context("store"))?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .context("Linux Secret Service helper stdin is unavailable")?;
            stdin
                .write_all(value.as_bytes())
                .context("failed to send secret to Linux Secret Service helper")?;
        }
        drop(child.stdin.take());

        let output = child
            .wait_with_output()
            .context("failed to wait for Linux Secret Service helper")?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "{}{}",
                helper_context("store failed"),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            );
        }
        Ok(())
    }

    pub(super) fn delete(reference: &str) -> Result<()> {
        let _output = command()
            .args([
                "clear",
                "application",
                APPLICATION_ATTRIBUTE,
                "reference",
                reference,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .with_context(|| helper_context("delete"))?;
        // clear can skip locked items; request unlock so verification cannot skip them.
        let mut probe = command()
            .args([
                "search",
                "--all",
                "--unlock",
                "application",
                APPLICATION_ATTRIBUTE,
                "reference",
                reference,
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| helper_context("deletion verification"))?;
        let absent = probe.status.success() && probe.stdout.is_empty() && probe.stderr.is_empty();
        // search may return a secret with item metadata; never expose captured output.
        probe.stdout.fill(0);
        probe.stderr.fill(0);
        if !absent {
            bail!(
                "{}; unlock the credential store and retry",
                helper_context("delete failed")
            );
        }
        Ok(())
    }

    pub(super) fn load(reference: &str) -> Result<String> {
        let output = command()
            .args([
                "lookup",
                "application",
                APPLICATION_ATTRIBUTE,
                "reference",
                reference,
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| helper_context("lookup"))?;

        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!(
                "{}{}",
                helper_context("lookup failed"),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            );
        }

        let mut value = String::from_utf8(output.stdout)
            .context("Linux Secret Service returned non-UTF-8 secret data")?;
        if value.ends_with('\n') {
            value.pop();
            if value.ends_with('\r') {
                value.pop();
            }
        }
        Ok(value)
    }
}

#[cfg(target_os = "macos")]
mod macos_keychain {
    use anyhow::{Context, Result, bail};
    use std::{ffi::c_void, ptr::null_mut};

    const SERVICE: &[u8] = b"io.github.p1k1g.stream-archive";
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecKeychainAddGenericPassword(
            keychain: *mut c_void,
            service_name_length: u32,
            service_name: *const u8,
            account_name_length: u32,
            account_name: *const u8,
            password_length: u32,
            password_data: *const c_void,
            item_ref: *mut *mut c_void,
        ) -> i32;

        fn SecKeychainFindGenericPassword(
            keychain_or_array: *mut c_void,
            service_name_length: u32,
            service_name: *const u8,
            account_name_length: u32,
            account_name: *const u8,
            password_length: *mut u32,
            password_data: *mut *mut c_void,
            item_ref: *mut *mut c_void,
        ) -> i32;

        fn SecKeychainItemFreeContent(attr_list: *mut c_void, data: *mut c_void) -> i32;
        fn SecKeychainItemDelete(item_ref: *mut c_void) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
    }

    fn len_u32(value: usize, field: &str) -> Result<u32> {
        u32::try_from(value).with_context(|| format!("macOS Keychain {field} is too long"))
    }

    pub(super) fn store(reference: &str, value: &str) -> Result<()> {
        let account = reference.as_bytes();
        let secret = value.as_bytes();
        let status = unsafe {
            SecKeychainAddGenericPassword(
                null_mut(),
                len_u32(SERVICE.len(), "service")?,
                SERVICE.as_ptr(),
                len_u32(account.len(), "account")?,
                account.as_ptr(),
                len_u32(secret.len(), "secret")?,
                secret.as_ptr().cast::<c_void>(),
                null_mut(),
            )
        };
        if status != 0 {
            bail!("macOS Keychain store failed with OSStatus {status}");
        }
        Ok(())
    }

    pub(super) fn delete(reference: &str) -> Result<()> {
        let account = reference.as_bytes();
        let mut item_ref: *mut c_void = null_mut();
        let status = unsafe {
            SecKeychainFindGenericPassword(
                null_mut(),
                len_u32(SERVICE.len(), "service")?,
                SERVICE.as_ptr(),
                len_u32(account.len(), "account")?,
                account.as_ptr(),
                null_mut(),
                null_mut(),
                &mut item_ref,
            )
        };
        if status == ERR_SEC_ITEM_NOT_FOUND {
            return Ok(());
        }
        if status != 0 {
            bail!("macOS Keychain deletion lookup failed with OSStatus {status}");
        }
        if item_ref.is_null() {
            bail!("macOS Keychain returned an invalid item reference");
        }
        struct KeychainItem(*mut c_void);
        impl Drop for KeychainItem {
            fn drop(&mut self) {
                unsafe { CFRelease(self.0) };
            }
        }
        let item = KeychainItem(item_ref);
        let status = unsafe { SecKeychainItemDelete(item.0) };
        if status != 0 && status != ERR_SEC_ITEM_NOT_FOUND {
            bail!("macOS Keychain deletion failed with OSStatus {status}");
        }
        Ok(())
    }

    pub(super) fn load(reference: &str) -> Result<String> {
        let account = reference.as_bytes();
        let mut secret_len = 0u32;
        let mut secret_ptr: *mut c_void = null_mut();
        let status = unsafe {
            SecKeychainFindGenericPassword(
                null_mut(),
                len_u32(SERVICE.len(), "service")?,
                SERVICE.as_ptr(),
                len_u32(account.len(), "account")?,
                account.as_ptr(),
                &mut secret_len,
                &mut secret_ptr,
                null_mut(),
            )
        };
        if status == ERR_SEC_ITEM_NOT_FOUND {
            bail!("macOS Keychain item is missing for native secret reference")
        }
        if status != 0 {
            bail!("macOS Keychain lookup failed with OSStatus {status}");
        }
        if secret_ptr.is_null() && secret_len != 0 {
            bail!("macOS Keychain returned an invalid secret buffer");
        }

        struct KeychainContent(*mut c_void);
        impl Drop for KeychainContent {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe {
                        let _ = SecKeychainItemFreeContent(null_mut(), self.0);
                    }
                }
            }
        }

        let content = KeychainContent(secret_ptr);
        let bytes = if secret_len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(content.0.cast::<u8>(), secret_len as usize) }
                .to_vec()
        };
        String::from_utf8(bytes).context("macOS Keychain returned non-UTF-8 secret data")
    }
}

#[cfg(windows)]
mod dpapi {
    use super::DPAPI_ENTROPY;
    use anyhow::{Result, bail};
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{GetLastError, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };

    struct OutBlob(CRYPT_INTEGER_BLOB);

    impl Default for OutBlob {
        fn default() -> Self {
            Self(CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: null_mut(),
            })
        }
    }

    impl Drop for OutBlob {
        fn drop(&mut self) {
            if !self.0.pbData.is_null() {
                unsafe {
                    std::ptr::write_bytes(self.0.pbData, 0, self.0.cbData as usize);
                    LocalFree(self.0.pbData.cast::<c_void>());
                }
            }
        }
    }

    fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(bytes.len())
                .map_err(|_| anyhow::anyhow!("DPAPI input is too large"))?,
            pbData: bytes.as_ptr().cast_mut(),
        })
    }

    unsafe fn blob_to_vec(blob: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if blob.pbData.is_null() || blob.cbData == 0 {
            return Vec::new();
        }
        unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec()
    }

    pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>> {
        let input = blob(plaintext)?;
        let entropy = blob(DPAPI_ENTROPY)?;
        let mut out = OutBlob::default();
        let ok = unsafe {
            CryptProtectData(
                &input,
                null(),
                &entropy,
                null_mut(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out.0,
            )
        };
        if ok == 0 {
            bail!("CryptProtectData failed with Win32 error {}", unsafe {
                GetLastError()
            });
        }
        Ok(unsafe { blob_to_vec(&out.0) })
    }

    pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
        let input = blob(ciphertext)?;
        let entropy = blob(DPAPI_ENTROPY)?;
        let mut out = OutBlob::default();
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                null_mut(),
                &entropy,
                null_mut(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out.0,
            )
        };
        if ok == 0 {
            bail!("CryptUnprotectData failed with Win32 error {}", unsafe {
                GetLastError()
            });
        }
        Ok(unsafe { blob_to_vec(&out.0) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_lookup_waits_for_writer_and_holds_shared_lock_through_unprotection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let owner = crate::store::Store::open(path.clone()).unwrap();
        let observer = crate::store::Store::open_observer(path).unwrap();
        for value in ["old-reference", "replacement-reference", ""] {
            let writer = kick_secret_guard(&observer).unwrap();
            observer
                .sync_settings(
                    &std::collections::BTreeMap::from([(
                        "KICK_SESSION_TOKEN".into(),
                        value.into(),
                    )]),
                    "test-observer",
                )
                .unwrap();
            let reader_owner = owner.clone();
            let reader_observer = observer.clone();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                let result = read_kick_token_with(&reader_owner, |reference| {
                    assert_eq!(reference, value);
                    assert!(kick_secret_guard(&reader_observer).is_err());
                    assert!(
                        kick_secret_read_guard(&reader_observer, std::time::Duration::ZERO).is_ok()
                    );
                    Ok(reference.into())
                });
                done_tx.send(result).unwrap();
            });
            started_rx.recv().unwrap();
            assert!(
                done_rx
                    .recv_timeout(std::time::Duration::from_millis(50))
                    .is_err()
            );
            drop(writer);
            assert_eq!(
                done_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap()
                    .unwrap(),
                value
            );
            reader.join().unwrap();
            assert!(kick_secret_guard(&observer).is_ok());
        }
        assert!(read_kick_token_with(&owner, |_| bail!("native lookup failed")).is_err());
        assert!(kick_secret_guard(&observer).is_ok());
    }

    #[test]
    fn provider_lookup_refreshes_observer_changes_and_locks_through_pair_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let owner = crate::store::Store::open(path.clone()).unwrap();
        let observer = crate::store::Store::open_observer(path).unwrap();
        let keys = [
            "SOOP_USERNAME",
            "SOOP_PASSWORD",
            "CHZZK_NID_AUT",
            "CHZZK_NID_SES",
            "CLOUDFLARE_API_KEY",
        ];
        let secrets = [
            "SOOP_PASSWORD",
            "CHZZK_NID_AUT",
            "CHZZK_NID_SES",
            "CLOUDFLARE_API_KEY",
        ];
        for value in ["old-reference", "replacement-reference", ""] {
            let writer = kick_secret_guard(&observer).unwrap();
            observer
                .sync_settings(
                    &keys
                        .iter()
                        .map(|key| ((*key).into(), value.into()))
                        .collect(),
                    "test",
                )
                .unwrap();
            let reader_owner = owner.clone();
            let reader_observer = observer.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                let result =
                    read_provider_settings_with(&reader_owner, &keys, &secrets, |reference, _| {
                        assert_eq!(reference, value);
                        assert!(kick_secret_guard(&reader_observer).is_err());
                        Ok(reference.into())
                    });
                tx.send(result).unwrap();
            });
            assert!(
                rx.recv_timeout(std::time::Duration::from_millis(50))
                    .is_err()
            );
            drop(writer);
            let snapshot = rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            for key in keys {
                assert_eq!(snapshot[key], value);
            }
            reader.join().unwrap();
        }
        assert!(
            read_provider_settings_with(&owner, &keys, &secrets, |_, _| bail!(
                "test native failure"
            ))
            .is_err()
        );
        assert!(kick_secret_guard(&observer).is_ok());
    }

    #[test]
    fn kick_read_lock_timeout_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().join("test.db")).unwrap();
        let _writer = kick_secret_guard(&store).unwrap();
        assert!(kick_secret_read_guard(&store, std::time::Duration::from_millis(20)).is_err());
    }

    #[test]
    fn cleanup_references_never_include_inline_secrets() {
        for value in ["", "plaintext-token", "dpapi:v1:encrypted"] {
            assert!(native_cleanup_reference(value, "TEST").unwrap().is_none());
        }
        assert!(native_cleanup_reference("native-secret:v1:invalid", "TEST").is_err());
    }

    #[test]
    fn deletion_targets_only_valid_native_references_and_propagates_failures() {
        let reference = "123e4567-e89b-12d3-a456-426614174000";
        let value = format!("{NATIVE_SECRET_PREFIX}{reference}");
        delete_protected_secret_with(&value, "TEST", |actual| {
            assert_eq!(actual, reference);
            Ok(())
        })
        .unwrap();
        assert!(delete_protected_secret_with(&value, "TEST", |_| bail!("locked")).is_err());
        assert!(
            delete_protected_secret_with("native-secret:v1:invalid", "TEST", |_| {
                panic!("invalid references must not reach native deletion")
            })
            .is_err()
        );
        for value in ["", "dpapi:v1:opaque", "legacy-token"] {
            delete_protected_secret_with(value, "TEST", |_| {
                panic!("inline secrets have no native entry")
            })
            .unwrap();
        }
    }

    #[test]
    fn plaintext_legacy_value_is_passed_through() {
        assert_eq!(unprotect_secret("abc", "X").unwrap(), "abc");
    }

    #[test]
    fn protected_prefixes_are_detected_case_insensitively() {
        assert!(is_protected("dpapi:v1:abc"));
        assert!(is_protected("DPAPI:V1:abc"));
        assert!(is_protected(
            "native-secret:v1:123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(is_protected(
            "NATIVE-SECRET:V1:123e4567-e89b-12d3-a456-426614174000"
        ));
    }

    #[test]
    fn native_reference_requires_uuid() {
        assert!(parse_native_reference("not-a-uuid", "TEST").is_err());
        assert_eq!(
            parse_native_reference("123e4567-e89b-12d3-a456-426614174000", "TEST").unwrap(),
            "123e4567-e89b-12d3-a456-426614174000"
        );
    }

    #[test]
    fn configured_status_does_not_expose_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.ini");
        fs::write(
            &path,
            "SOOP_PASSWORD=secret\nCLOUDFLARE_API_KEY=\nCHZZK_NID_AUT=aut\nCHZZK_NID_SES=ses\n",
        )
        .unwrap();
        let status = configured_secrets(&path).unwrap();
        assert!(status["SOOP_PASSWORD"]);
        assert!(!status["CLOUDFLARE_API_KEY"]);
        assert!(status["CHZZK_NID_AUT"]);
        assert!(status["CHZZK_NID_SES"]);
    }

    #[cfg(windows)]
    #[test]
    fn native_dpapi_round_trip() {
        let encrypted = protect_secret("phase3-secret").unwrap();
        assert!(encrypted.starts_with(DPAPI_PREFIX));
        assert_eq!(
            unprotect_secret(&encrypted, "TEST").unwrap(),
            "phase3-secret"
        );
    }
}
