use super::{PlatformId, soop};
use anyhow::{Result, bail};
use reqwest::Client;

#[derive(Debug, Clone)]
pub struct LiveBroadcast {
    pub id: String,
    pub channel_name: String,
    pub title: String,
    pub password_required: bool,
    pub(crate) payload: BroadcastPayload,
}

#[derive(Debug, Clone)]
pub(crate) enum BroadcastPayload {
    Soop(soop::live::SoopBroadcast),
}

#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub quality: String,
    pub cdn: String,
    pub host: String,
    pub playlist_url: String,
}

pub struct StreamResolveConfig<'a> {
    pub worker_url: &'a str,
    pub worker_api_key: &'a str,
    pub max_retries: usize,
}

#[derive(Debug, Clone)]
pub enum LiveProbe {
    Offline,
    AuthRequired,
    Live(LiveBroadcast),
}

pub enum LiveSession {
    Soop(soop::live::SoopLiveSession),
}

impl LiveSession {
    pub fn new(platform: PlatformId, client: Client) -> Result<Self> {
        match platform {
            PlatformId::Soop => Ok(Self::Soop(soop::live::SoopLiveSession::new(client))),
        }
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<String> {
        match self {
            Self::Soop(session) => session.login(username, password).await,
        }
    }

    pub async fn probe(&self, account: &str) -> Result<LiveProbe> {
        match self {
            Self::Soop(session) => match session.probe(account).await? {
                soop::live::SoopProbe::Offline => Ok(LiveProbe::Offline),
                soop::live::SoopProbe::AuthRequired => Ok(LiveProbe::AuthRequired),
                soop::live::SoopProbe::Live(value) => Ok(LiveProbe::Live(LiveBroadcast {
                    id: value.bno.clone(),
                    channel_name: value.bj_nick.clone(),
                    title: value.title.clone(),
                    password_required: soop::live::is_password_protected(&value.bpwd),
                    payload: BroadcastPayload::Soop(value),
                })),
            },
        }
    }

    pub async fn resolve_stream(
        &self,
        account: &str,
        broadcast: &LiveBroadcast,
        config: &StreamResolveConfig<'_>,
        password: &str,
    ) -> Result<ResolvedStream> {
        match (self, &broadcast.payload) {
            (Self::Soop(session), BroadcastPayload::Soop(value)) => {
                let stream = session
                    .resolve_stream(account, value, config.worker_url, config.worker_api_key, config.max_retries, password)
                    .await?;
                Ok(ResolvedStream {
                    quality: stream.quality,
                    cdn: stream.cdn,
                    host: stream.host,
                    playlist_url: stream.playlist_url,
                })
            }
        }
    }

    pub fn platform(&self) -> PlatformId {
        match self {
            Self::Soop(_) => PlatformId::Soop,
        }
    }
}

pub fn session_for(
    sessions: &mut std::collections::HashMap<PlatformId, LiveSession>,
    platform: PlatformId,
) -> Result<&mut LiveSession> {
    sessions
        .get_mut(&platform)
        .ok_or_else(|| anyhow::anyhow!("LIVE session is not initialized for {platform}"))
}

pub fn ensure_supported(platform: PlatformId) -> Result<()> {
    if !super::provider(platform).capabilities().live {
        bail!("{platform} LIVE is not supported");
    }
    Ok(())
}
