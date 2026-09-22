use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
#[cfg(test)]
use std::{fs, path::Path};
use uuid::Uuid;

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
];

#[cfg(test)]
fn is_protected(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.starts_with(DPAPI_PREFIX) || normalized.starts_with(NATIVE_SECRET_PREFIX)
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
