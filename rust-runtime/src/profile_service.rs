//! Bounded, public channel avatars. Provider URL mechanics stay behind providers.
use crate::support::platform::{PlatformId, provider};
use anyhow::{Result, bail};
use image::{ImageReader, imageops::FilterType};
use std::{io::Cursor, time::Duration};
use url::Url;

/// SOOP sample: 200x200; larger CHZZK images are cropped/downscaled to this ceiling.
pub const PROFILE_SIDE: u32 = 200;
const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_METADATA: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct ProfileImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn validate_image_url(platform: PlatformId, url: &Url) -> Result<()> {
    let allowed = match platform {
        PlatformId::Soop => url.host_str() == Some("stimg.sooplive.com"),
        PlatformId::Chzzk => url.host_str() == Some("nng-phinf.pstatic.net"),
    };
    if !allowed
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        bail!("허용되지 않은 프로필 이미지 주소입니다.");
    }
    Ok(())
}

async fn bounded_body(response: reqwest::Response, maximum: usize) -> Result<Vec<u8>> {
    let mut response = response.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > maximum as u64)
    {
        bail!("프로필 응답이 너무 큽니다.");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > maximum.saturating_sub(bytes.len()) {
            bail!("프로필 응답이 너무 큽니다.");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn load(platform: PlatformId, account: &str) -> Result<ProfileImage> {
    // Bound the entire operation, including metadata, download and blocking decode.
    tokio::time::timeout(Duration::from_secs(12), load_inner(platform, account)).await?
}

async fn load_inner(platform: PlatformId, account: &str) -> Result<ProfileImage> {
    let account = account.trim();
    let provider = provider(platform);
    provider.validate_account(account)?;
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36",
        )
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let metadata = if platform == PlatformId::Chzzk {
        let bytes = bounded_body(
            provider
                .channel_lookup_request(&client, account)
                .send()
                .await?,
            MAX_METADATA,
        )
        .await?;
        Some(serde_json::from_slice(&bytes)?)
    } else {
        None
    };
    let url = provider.profile_image_url(account, metadata.as_ref())?;
    validate_image_url(platform, &url)?;
    let bytes = bounded_body(client.get(url).send().await?, MAX_BODY).await?;
    tokio::task::spawn_blocking(move || decode(&bytes)).await?
}

fn decode(bytes: &[u8]) -> Result<ProfileImage> {
    if bytes.len() > MAX_BODY {
        bail!("프로필 이미지가 너무 큽니다.");
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Gif
                | image::ImageFormat::WebP
        )
    ) {
        bail!("지원하지 않는 프로필 이미지 형식입니다.");
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    // GIF/WebP animations intentionally use the first frame only.
    let image = reader.decode()?;
    let side = image.width().min(image.height()).min(PROFILE_SIDE);
    if side == 0 {
        bail!("빈 프로필 이미지입니다.");
    }
    let image = image
        .resize_to_fill(side, side, FilterType::Triangle)
        .into_rgba8();
    Ok(ProfileImage {
        width: side,
        height: side,
        rgba: image.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat};

    #[tokio::test]
    async fn rejects_declared_and_streamed_body_over_limit() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        for payload in [
            "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\na\r\n0123456789\r\na\r\n0123456789\r\n0\r\n\r\n",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                socket.write_all(payload.as_bytes()).await.unwrap();
            });
            let response = reqwest::Client::new()
                .get(format!("http://{address}"))
                .send()
                .await
                .unwrap();
            assert!(bounded_body(response, 16).await.is_err());
            server.await.unwrap();
        }
    }

    #[test]
    fn decodes_gif_and_webp_by_content() {
        for format in [ImageFormat::Gif, ImageFormat::WebP, ImageFormat::Jpeg] {
            let mut bytes = Cursor::new(Vec::new());
            DynamicImage::new_rgb8(200, 200)
                .write_to(&mut bytes, format)
                .unwrap();
            assert_eq!(decode(bytes.get_ref()).unwrap().width, 200);
        }
    }

    #[test]
    fn crop_downscale_and_no_upscale() {
        for (width, height, expected) in [(1002, 1025, 200), (200, 200, 200), (24, 40, 24)] {
            let mut bytes = Cursor::new(Vec::new());
            DynamicImage::new_rgba8(width, height)
                .write_to(&mut bytes, ImageFormat::Png)
                .unwrap();
            let result = decode(bytes.get_ref()).unwrap();
            assert_eq!((result.width, result.height), (expected, expected));
            assert_eq!(result.rgba.len(), (expected * expected * 4) as usize);
        }
    }

    #[test]
    fn rejects_corruption_large_dimensions_and_unsupported_formats() {
        assert!(decode(b"not an image").is_err());
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(4097, 2)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        assert!(decode(bytes.get_ref()).is_err());
        assert!(decode(b"BM\0\0\0\0\0\0\0\0\0\0\0\0").is_err());
        assert!(decode(&vec![0; MAX_BODY + 1]).is_err());
    }

    #[test]
    fn restricts_public_provider_urls() {
        for raw in [
            "http://stimg.sooplive.com/a",
            "https://localhost/a",
            "https://stimg.sooplive.com.evil.test/a",
            "https://user@stimg.sooplive.com/a",
            "https://stimg.sooplive.com:8443/a",
        ] {
            assert!(validate_image_url(PlatformId::Soop, &Url::parse(raw).unwrap()).is_err());
        }
        assert!(
            validate_image_url(
                PlatformId::Soop,
                &Url::parse("https://stimg.sooplive.com/LOGO/10/1004ysus/1004ysus.jpg").unwrap()
            )
            .is_ok()
        );
        assert!(
            validate_image_url(
                PlatformId::Chzzk,
                &Url::parse("https://nng-phinf.pstatic.net/a.png").unwrap()
            )
            .is_ok()
        );
    }

    #[test]
    fn chzzk_profile_requires_matching_identity() {
        let account = "0123456789abcdef0123456789abcdef";
        let mut value = serde_json::json!({"code":200,"content":{"channelId":account,"channelName":"테스트","channelImageUrl":"https://nng-phinf.pstatic.net/a.png"}});
        assert!(
            provider(PlatformId::Chzzk)
                .profile_image_url(account, Some(&value))
                .is_ok()
        );
        value["content"]["channelId"] = "ffffffffffffffffffffffffffffffff".into();
        assert!(
            provider(PlatformId::Chzzk)
                .profile_image_url(account, Some(&value))
                .is_err()
        );
    }
}
