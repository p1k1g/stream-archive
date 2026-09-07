use anyhow::{bail, Context, Result};
use atomic_write_file::AtomicWriteFile;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::{fs, io::Write, path::{Path, PathBuf}};

pub const DPAPI_PREFIX: &str = "dpapi:v1:";
const DPAPI_ENTROPY: &[u8] = b"SOOPLiveDownloader:v1";
const SECRET_KEYS: &[&str] = &["SOOP_PASSWORD", "CLOUDFLARE_API_KEY"];

pub fn is_protected(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with(DPAPI_PREFIX)
}

pub fn unprotect_secret(value: &str, name: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if !is_protected(value) {
        return Ok(value.to_string());
    }

    let encoded = &value[DPAPI_PREFIX.len()..];
    let cipher = BASE64
        .decode(encoded)
        .with_context(|| format!("{name} DPAPI payload is not valid base64"))?;
    let plain = platform_unprotect(&cipher)
        .with_context(|| format!("{name} DPAPI decrypt failed"))?;
    String::from_utf8(plain).with_context(|| format!("{name} DPAPI plaintext is not UTF-8"))
}

pub fn protect_secret(value: &str) -> Result<String> {
    if value.contains('\r') || value.contains('\n') || value.contains('\0') {
        bail!("secret must be a single line");
    }
    if value.is_empty() {
        return Ok(String::new());
    }
    let cipher = platform_protect(value.as_bytes()).context("DPAPI encrypt failed")?;
    Ok(format!("{DPAPI_PREFIX}{}", BASE64.encode(cipher)))
}

pub fn configured_secrets(path: &Path) -> Result<std::collections::BTreeMap<String, bool>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut result = std::collections::BTreeMap::new();
    for key in SECRET_KEYS {
        result.insert((*key).to_string(), false);
    }
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue; };
        let key = key.trim();
        if SECRET_KEYS.contains(&key) {
            result.insert(key.to_string(), !value.trim().is_empty());
        }
    }
    Ok(result)
}

pub fn save_protected_secrets(
    path: &Path,
    updates: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    for key in updates.keys() {
        if !SECRET_KEYS.contains(&key.as_str()) {
            bail!("unsupported secret key: {key}");
        }
    }

    let original = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let newline = if original.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized = original.replace("\r\n", "\n").replace('\r', "\n");
    let mut encrypted = std::collections::BTreeMap::new();
    for (key, value) in updates {
        if value.is_empty() {
            continue; // blank from the UI means "leave the existing secret unchanged"
        }
        encrypted.insert(key.clone(), protect_secret(value)?);
    }
    if encrypted.is_empty() {
        return Ok(());
    }

    let mut seen = std::collections::HashSet::new();
    let mut output = Vec::new();
    for raw in normalized.lines() {
        if let Some((key, _)) = raw.split_once('=') {
            let key = key.trim();
            if let Some(value) = encrypted.get(key) {
                output.push(format!("{key}={value}"));
                seen.insert(key.to_string());
                continue;
            }
        }
        output.push(raw.to_string());
    }
    for (key, value) in &encrypted {
        if !seen.contains(key) {
            output.push(format!("{key}={value}"));
        }
    }

    backup_existing(path)?;
    let mut content = output.join(newline);
    content.push_str(newline);
    let mut file = AtomicWriteFile::options()
        .open(path)
        .with_context(|| format!("failed to open atomic writer for {}", path.display()))?;
    file.write_all(content.as_bytes())?;
    file.commit()
        .with_context(|| format!("failed to commit {}", path.display()))?;
    Ok(())
}

fn backup_existing(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let mut backup_name = path.as_os_str().to_os_string();
    backup_name.push(".bak");
    let backup = PathBuf::from(backup_name);
    fs::copy(path, &backup).with_context(|| {
        format!("failed to create backup {} from {}", backup.display(), path.display())
    })?;
    Ok(())
}

#[cfg(windows)]
fn platform_protect(plaintext: &[u8]) -> Result<Vec<u8>> {
    dpapi::protect(plaintext)
}

#[cfg(windows)]
fn platform_unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
    dpapi::unprotect(ciphertext)
}

#[cfg(not(windows))]
fn platform_protect(_plaintext: &[u8]) -> Result<Vec<u8>> {
    bail!("native protected secret storage is currently available on Windows only")
}

#[cfg(not(windows))]
fn platform_unprotect(_ciphertext: &[u8]) -> Result<Vec<u8>> {
    bail!("DPAPI-protected secrets can only be decrypted on Windows")
}

#[cfg(windows)]
mod dpapi {
    use super::DPAPI_ENTROPY;
    use anyhow::{bail, Result};
    use std::{ffi::c_void, ptr::{null, null_mut}};
    use windows_sys::Win32::{
        Foundation::{GetLastError, LocalFree},
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
            CRYPTPROTECT_UI_FORBIDDEN,
        },
    };

    struct OutBlob(CRYPT_INTEGER_BLOB);

    impl Default for OutBlob {
        fn default() -> Self {
            Self(CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() })
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
            cbData: u32::try_from(bytes.len()).map_err(|_| anyhow::anyhow!("DPAPI input is too large"))?,
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
            bail!("CryptProtectData failed with Win32 error {}", unsafe { GetLastError() });
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
            bail!("CryptUnprotectData failed with Win32 error {}", unsafe { GetLastError() });
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
    fn protected_prefix_is_detected_case_insensitively() {
        assert!(is_protected("dpapi:v1:abc"));
        assert!(is_protected("DPAPI:V1:abc"));
    }

    #[test]
    fn configured_status_does_not_expose_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.ini");
        fs::write(&path, "SOOP_PASSWORD=secret\nCLOUDFLARE_API_KEY=\n").unwrap();
        let status = configured_secrets(&path).unwrap();
        assert_eq!(status["SOOP_PASSWORD"], true);
        assert_eq!(status["CLOUDFLARE_API_KEY"], false);
    }

    #[cfg(windows)]
    #[test]
    fn native_dpapi_round_trip() {
        let encrypted = protect_secret("phase3-secret").unwrap();
        assert!(encrypted.starts_with(DPAPI_PREFIX));
        assert_eq!(unprotect_secret(&encrypted, "TEST").unwrap(), "phase3-secret");
    }
}
