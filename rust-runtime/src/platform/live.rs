use super::{PlatformId, chzzk, kick, soop};
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

impl LiveBroadcast {
    /// Metadata from the existing provider probe; no additional API request.
    pub fn thumbnail_url(&self) -> Option<String> {
        match &self.payload {
            BroadcastPayload::Soop(value) => soop::live::thumbnail_url(&value.bno),
            BroadcastPayload::Chzzk(value) => value.thumbnail_url.clone(),
            BroadcastPayload::Kick(value) => value.thumbnail_url.clone(),
        }
    }
}

pub(crate) fn validate_thumbnail_url(platform: PlatformId, url: &url::Url) -> bool {
    match platform {
        PlatformId::Soop => soop::live::valid_thumbnail_url(url),
        PlatformId::Chzzk => chzzk::live::valid_thumbnail_url(url),
        PlatformId::Kick => kick::live::valid_thumbnail_url(url),
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BroadcastPayload {
    Soop(soop::live::SoopBroadcast),
    Chzzk(chzzk::live::ChzzkBroadcast),
    Kick(kick::live::KickBroadcast),
}

#[derive(Debug, Clone)]
pub struct HttpCookie {
    pub domain: String,
    pub name: String,
    pub value: String,
    pub secure: bool,
}

#[derive(Debug, Clone)]
pub enum StreamInput {
    DirectHls(String),
    PluginUrl {
        url: String,
        cookies: Vec<HttpCookie>,
        start_at_zero: bool,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub quality: String,
    pub cdn: String,
    pub host: String,
    pub input: StreamInput,
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
    Chzzk(chzzk::live::ChzzkLiveSession),
    Kick(kick::live::KickLiveSession),
}

impl LiveSession {
    pub fn new(platform: PlatformId, client: Client) -> Result<Self> {
        match platform {
            PlatformId::Soop => Ok(Self::Soop(soop::live::SoopLiveSession::new(client))),
            PlatformId::Chzzk => Ok(Self::Chzzk(chzzk::live::ChzzkLiveSession::new(client))),
            PlatformId::Kick => Ok(Self::Kick(kick::live::KickLiveSession::new(client))),
        }
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<String> {
        match self {
            Self::Soop(session) => session.login(username, password).await,
            Self::Chzzk(_) => bail!("CHZZK는 ID/PW 로그인을 사용하지 않습니다."),
            Self::Kick(_) => bail!("KICK은 공개 LIVE만 지원하며 ID/PW 로그인을 사용하지 않습니다."),
        }
    }

    pub async fn recover_auth(&mut self, username: &str, password: &str) -> Result<String> {
        match self {
            Self::Soop(session) => session.login(username, password).await,
            Self::Chzzk(session) => session.recover_auth(),
            Self::Kick(session) => session.recover_auth(),
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
            Self::Chzzk(session) => match session.probe(account).await? {
                chzzk::live::ChzzkProbe::Offline => Ok(LiveProbe::Offline),
                chzzk::live::ChzzkProbe::AuthRequired => Ok(LiveProbe::AuthRequired),
                chzzk::live::ChzzkProbe::Live(value) => Ok(LiveProbe::Live(LiveBroadcast {
                    id: value.live_id.clone(),
                    channel_name: value.channel_name.clone(),
                    title: value.title.clone(),
                    password_required: false,
                    payload: BroadcastPayload::Chzzk(value),
                })),
            },
            Self::Kick(session) => match session.probe(account).await? {
                kick::live::KickProbe::Offline => Ok(LiveProbe::Offline),
                kick::live::KickProbe::Live(value) => Ok(LiveProbe::Live(LiveBroadcast {
                    id: value.live_id.clone(),
                    channel_name: value.channel_name.clone(),
                    title: value.title.clone(),
                    password_required: false,
                    payload: BroadcastPayload::Kick(value),
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
                    .resolve_stream(
                        account,
                        value,
                        config.worker_url,
                        config.worker_api_key,
                        config.max_retries,
                        password,
                    )
                    .await?;
                Ok(ResolvedStream {
                    quality: stream.quality,
                    cdn: stream.cdn,
                    host: stream.host,
                    input: StreamInput::DirectHls(stream.playlist_url),
                })
            }
            (Self::Chzzk(session), BroadcastPayload::Chzzk(value)) => {
                let stream = session.resolve_stream(account, value).await?;
                Ok(ResolvedStream {
                    quality: stream.quality,
                    cdn: "streamlink-plugin".into(),
                    host: "chzzk.naver.com".into(),
                    input: stream.input,
                })
            }
            (Self::Kick(session), BroadcastPayload::Kick(value)) => Ok(ResolvedStream {
                quality: "best".into(),
                cdn: "streamlink-plugin".into(),
                host: "kick.com".into(),
                input: session.resolve_stream(account, value)?,
            }),
            _ => bail!("LIVE 세션과 방송 payload 플랫폼이 일치하지 않습니다."),
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
