use crate::{security::unprotect_secret, store};
use anyhow::Result;

use super::super::live::HttpCookie;

pub const NID_AUT_KEY: &str = "CHZZK_NID_AUT";
pub const NID_SES_KEY: &str = "CHZZK_NID_SES";

#[derive(Debug, Clone, Default)]
pub struct ChzzkAuth {
    nid_aut: String,
    nid_ses: String,
}

impl ChzzkAuth {
    pub fn load() -> Result<Self> {
        let db = store::global()?;
        let nid_aut = unprotect_secret(
            &db.setting_value(NID_AUT_KEY)?.unwrap_or_default(),
            NID_AUT_KEY,
        )?;
        let nid_ses = unprotect_secret(
            &db.setting_value(NID_SES_KEY)?.unwrap_or_default(),
            NID_SES_KEY,
        )?;
        Ok(Self { nid_aut, nid_ses })
    }

    pub fn configured(&self) -> bool {
        !self.nid_aut.is_empty() && !self.nid_ses.is_empty()
    }

    pub fn partial(&self) -> bool {
        self.nid_aut.is_empty() != self.nid_ses.is_empty()
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
    fn from_plain(nid_aut: &str, nid_ses: &str) -> Self {
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
    fn requires_both_naver_cookies_for_complete_auth() {
        assert!(!ChzzkAuth::from_plain("", "").configured());
        assert!(ChzzkAuth::from_plain("aut", "").partial());
        assert!(ChzzkAuth::from_plain("", "ses").partial());
        assert!(ChzzkAuth::from_plain("aut", "ses").configured());
    }

    #[test]
    fn builds_cookie_header_and_streamlink_cookie_list() {
        let auth = ChzzkAuth::from_plain("aut", "ses");
        assert_eq!(auth.cookie_header().as_deref(), Some("NID_AUT=aut; NID_SES=ses"));
        let cookies = auth.streamlink_cookies();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "NID_AUT");
        assert_eq!(cookies[1].name, "NID_SES");
    }
}
