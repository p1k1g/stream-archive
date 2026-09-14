use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
#[cfg(test)]
use std::{fs, path::Path};

pub const DPAPI_PREFIX: &str = "dpapi:v1:";
#[cfg(windows)]
const DPAPI_ENTROPY: &[u8] = b"SOOPLiveDownloader:v1";
#[cfg(test)]
const SECRET_KEYS: &[&str] = &[
    "SOOP_PASSWORD",
    "CLOUDFLARE_API_KEY",
    "CHZZK_NID_AUT",
    "CHZZK_NID_SES",
];

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
    let plain =
        platform_unprotect(&cipher).with_context(|| format!("{name} DPAPI decrypt failed"))?;
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
    fn protected_prefix_is_detected_case_insensitively() {
        assert!(is_protected("dpapi:v1:abc"));
        assert!(is_protected("DPAPI:V1:abc"));
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
        assert_eq!(status["SOOP_PASSWORD"], true);
        assert_eq!(status["CLOUDFLARE_API_KEY"], false);
        assert_eq!(status["CHZZK_NID_AUT"], true);
        assert_eq!(status["CHZZK_NID_SES"], true);
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
