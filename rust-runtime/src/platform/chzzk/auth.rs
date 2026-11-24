use crate::{
    security::read_provider_settings,
    store::{self, Store},
};
use anyhow::Result;
use std::{
    collections::BTreeMap,
    fmt,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use super::super::live::HttpCookie;

pub const NID_AUT_KEY: &str = "CHZZK_NID_AUT";
pub const NID_SES_KEY: &str = "CHZZK_NID_SES";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChzzkAuthState {
    Missing,
    Partial,
    Configured,
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct ChzzkAuth {
    nid_aut: String,
    nid_ses: String,
}

impl fmt::Debug for ChzzkAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChzzkAuth")
            .field("state", &self.state())
            .finish()
    }
}

struct CachedChzzkAuth {
    db_path: PathBuf,
    raw_nid_aut: String,
    raw_nid_ses: String,
    auth: ChzzkAuth,
}

static CHZZK_AUTH_CACHE: OnceLock<Mutex<Option<CachedChzzkAuth>>> = OnceLock::new();

fn auth_cache() -> &'static Mutex<Option<CachedChzzkAuth>> {
    CHZZK_AUTH_CACHE.get_or_init(|| Mutex::new(None))
}

fn read_raw_auth_refs(db: &Store) -> Result<(String, String)> {
    let mut raw = db.settings_for_keys(&[NID_AUT_KEY, NID_SES_KEY])?;
    let raw_aut = raw.remove(NID_AUT_KEY).unwrap_or_default();
    let raw_ses = raw.remove(NID_SES_KEY).unwrap_or_default();
    Ok((raw_aut, raw_ses))
}

impl ChzzkAuth {
    // Cache decrypted credentials keyed by (db_path, raw stored references) so
    // repeated LIVE probes avoid redundant OS secret-store / DPAPI decryptions
    // while still picking up in-process or cross-process settings edits immediately.
    pub fn load() -> Result<Self> {
        let db = store::global()?;
        Self::load_from_store(&db)
    }

    pub(crate) fn load_from_store(db: &Store) -> Result<Self> {
        Self::load_from_store_with(db, |store| {
            read_provider_settings(
                store,
                &[NID_AUT_KEY, NID_SES_KEY],
                &[NID_AUT_KEY, NID_SES_KEY],
            )
        })
    }

    fn load_from_store_with(
        db: &Store,
        loader: impl FnOnce(&Store) -> Result<BTreeMap<String, String>>,
    ) -> Result<Self> {
        db.refresh_config_cache_if_changed()?;
        let (raw_aut, raw_ses) = read_raw_auth_refs(db)?;
        let db_path = db.path();
        {
            let guard = auth_cache().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = guard.as_ref()
                && cached.db_path == db_path
                && cached.raw_nid_aut == raw_aut
                && cached.raw_nid_ses == raw_ses
            {
                return Ok(cached.auth.clone());
            }
        }

        let mut settings = loader(db)?;
        let nid_aut = settings.remove(NID_AUT_KEY).unwrap_or_default();
        let nid_ses = settings.remove(NID_SES_KEY).unwrap_or_default();
        let auth = Self { nid_aut, nid_ses };

        // Cache only if the raw references did not change mid-load.
        if let Ok((after_aut, after_ses)) = read_raw_auth_refs(db)
            && after_aut == raw_aut
            && after_ses == raw_ses
        {
            let mut guard = auth_cache().lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(CachedChzzkAuth {
                db_path: db_path.to_path_buf(),
                raw_nid_aut: raw_aut,
                raw_nid_ses: raw_ses,
                auth: auth.clone(),
            });
        }
        Ok(auth)
    }

    pub fn state(&self) -> ChzzkAuthState {
        match (self.nid_aut.is_empty(), self.nid_ses.is_empty()) {
            (true, true) => ChzzkAuthState::Missing,
            (false, false) => ChzzkAuthState::Configured,
            _ => ChzzkAuthState::Partial,
        }
    }

    pub fn configured(&self) -> bool {
        self.state() == ChzzkAuthState::Configured
    }

    pub fn partial(&self) -> bool {
        self.state() == ChzzkAuthState::Partial
    }

    pub fn cookie_header(&self) -> Option<String> {
        let mut values = Vec::new();
        if !self.nid_aut.is_empty() {
            values.push(format!("NID_AUT={}", self.nid_aut));
        }
        if !self.nid_ses.is_empty() {
            values.push(format!("NID_SES={}", self.nid_ses));
        }
        (!values.is_empty()).then(|| values.join("; "))
    }

    pub fn streamlink_cookies(&self) -> Vec<HttpCookie> {
        let mut cookies = Vec::new();
        if !self.nid_aut.is_empty() {
            cookies.push(HttpCookie {
                domain: ".naver.com".into(),
                name: "NID_AUT".into(),
                value: self.nid_aut.clone(),
                secure: true,
            });
        }
        if !self.nid_ses.is_empty() {
            cookies.push(HttpCookie {
                domain: ".naver.com".into(),
                name: "NID_SES".into(),
                value: self.nid_ses.clone(),
                secure: true,
            });
        }
        cookies
    }

    #[cfg(test)]
    pub(crate) fn from_plain(nid_aut: &str, nid_ses: &str) -> Self {
        Self {
            nid_aut: nid_aut.into(),
            nid_ses: nid_ses.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_auth_state() {
        assert_eq!(
            ChzzkAuth::from_plain("", "").state(),
            ChzzkAuthState::Missing
        );
        assert_eq!(
            ChzzkAuth::from_plain("aut", "").state(),
            ChzzkAuthState::Partial
        );
        assert_eq!(
            ChzzkAuth::from_plain("", "ses").state(),
            ChzzkAuthState::Partial
        );
        assert_eq!(
            ChzzkAuth::from_plain("aut", "ses").state(),
            ChzzkAuthState::Configured
        );
    }

    #[test]
    fn requires_both_naver_cookies_for_complete_auth() {
        assert!(!ChzzkAuth::from_plain("", "").configured());
        assert!(ChzzkAuth::from_plain("aut", "").partial());
        assert!(ChzzkAuth::from_plain("", "ses").partial());
        assert!(ChzzkAuth::from_plain("aut", "ses").configured());
    }

    #[test]
    fn builds_cookie_header_and_streamlink_cookie_list() {
        let auth = ChzzkAuth::from_plain("aut", "ses");
        assert_eq!(
            auth.cookie_header().as_deref(),
            Some("NID_AUT=aut; NID_SES=ses")
        );
        let cookies = auth.streamlink_cookies();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "NID_AUT");
        assert_eq!(cookies[1].name, "NID_SES");
        let debug = format!("{auth:?}");
        assert!(!debug.contains("aut"));
        assert!(!debug.contains("ses"));
    }

    #[test]
    fn caches_loaded_auth_until_raw_settings_or_external_db_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("stream-archive.db");
        let store = Store::open(db_path.clone()).unwrap();
        let calls = AtomicUsize::new(0);
        let load = |s: &Store| {
            ChzzkAuth::load_from_store_with(s, |inner| {
                calls.fetch_add(1, Ordering::SeqCst);
                inner.settings_for_keys(&[NID_AUT_KEY, NID_SES_KEY])
            })
        };

        store
            .sync_settings(
                &BTreeMap::from([
                    (NID_AUT_KEY.into(), "aut-v1".into()),
                    (NID_SES_KEY.into(), "ses-v1".into()),
                ]),
                "test",
            )
            .unwrap();
        assert_eq!(
            load(&store).unwrap(),
            ChzzkAuth::from_plain("aut-v1", "ses-v1")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Unchanged raw refs hit the in-memory cache without re-running decryption.
        assert_eq!(
            load(&store).unwrap(),
            ChzzkAuth::from_plain("aut-v1", "ses-v1")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // In-process update invalidates immediately.
        store
            .sync_settings(
                &BTreeMap::from([
                    (NID_AUT_KEY.into(), "aut-v2".into()),
                    (NID_SES_KEY.into(), "ses-v2".into()),
                ]),
                "test",
            )
            .unwrap();
        assert_eq!(
            load(&store).unwrap(),
            ChzzkAuth::from_plain("aut-v2", "ses-v2")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // External connection write invalidates via PRAGMA data_version.
        let other = Store::open_observer(db_path).unwrap();
        other
            .sync_settings(
                &BTreeMap::from([
                    (NID_AUT_KEY.into(), "".into()),
                    (NID_SES_KEY.into(), "".into()),
                ]),
                "external",
            )
            .unwrap();
        assert_eq!(load(&store).unwrap(), ChzzkAuth::from_plain("", ""));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
