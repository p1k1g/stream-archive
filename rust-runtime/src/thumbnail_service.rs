//! Bounded, public LIVE snapshots. Provider URL mechanics stay behind providers.
use crate::support::platform::PlatformId;
use anyhow::{Result, bail};
use image::{ImageReader, imageops::FilterType};
use std::{io::Cursor, time::Duration};
use url::Url;

const MAX_WIDTH: u32 = 480;
const MAX_HEIGHT: u32 = 270;
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ThumbnailImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn validate_image_url(platform: PlatformId, url: &Url) -> Result<()> {
    let allowed = crate::support::platform::live::validate_thumbnail_url(platform, url);
    if !allowed
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        bail!("허용되지 않은 LIVE 썸네일 이미지 주소입니다.");
    }
    Ok(())
}

async fn bounded_body(response: reqwest::Response, maximum: usize) -> Result<Vec<u8>> {
    let mut response = response.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > maximum as u64)
    {
        bail!("LIVE 썸네일 응답이 너무 큽니다.");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > maximum.saturating_sub(bytes.len()) {
            bail!("LIVE 썸네일 응답이 너무 큽니다.");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn load(platform: PlatformId, url: &str) -> Result<ThumbnailImage> {
    tokio::time::timeout(Duration::from_secs(12), load_inner(platform, url, false)).await?
}

pub async fn load_vod(platform: PlatformId, url: &str) -> Result<ThumbnailImage> {
    tokio::time::timeout(Duration::from_secs(12), load_inner(platform, url, true)).await?
}

fn validate_vod_image_url(platform: PlatformId, url: &Url) -> Result<()> {
    if !crate::support::platform::vod::validate_thumbnail_url(platform, url)
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        bail!("허용되지 않은 VOD 썸네일 이미지 주소입니다.");
    }
    Ok(())
}

async fn load_inner(platform: PlatformId, url: &str, vod: bool) -> Result<ThumbnailImage> {
    let mut url = Url::parse(url)?;
    if vod {
        // Legacy provider thumbnails may use HTTP; never transmit over HTTP.
        if url.scheme() == "http" {
            url.set_scheme("https")
                .map_err(|_| anyhow::anyhow!("잘못된 썸네일 주소입니다."))?;
        }
        validate_vod_image_url(platform, &url)?;
    } else {
        validate_image_url(platform, &url)?;
    }
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36",
        )
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let bytes = bounded_body(
        client
            .get(url)
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .send()
            .await?,
        MAX_BODY,
    )
    .await?;
    tokio::task::spawn_blocking(move || decode(&bytes)).await?
}

fn decode(bytes: &[u8]) -> Result<ThumbnailImage> {
    if bytes.len() > MAX_BODY {
        bail!("LIVE 썸네일 이미지가 너무 큽니다.");
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
        bail!("지원하지 않는 LIVE 썸네일 이미지 형식입니다.");
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    // GIF/WebP animations intentionally use the first frame only.
    let image = reader.decode()?;
    if image.width() == 0 || image.height() == 0 {
        bail!("빈 LIVE 썸네일 이미지입니다.");
    }
    // Fit the whole image, preserving aspect ratio and never upscaling.
    let image = image
        .resize(
            image.width().min(MAX_WIDTH),
            image.height().min(MAX_HEIGHT),
            FilterType::Triangle,
        )
        .into_rgba8();
    Ok(ThumbnailImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat};

    #[test]
    fn vod_images_are_limited_to_provider_cdns_without_credentials_or_custom_ports() {
        for (platform, url) in [
            (
                PlatformId::Soop,
                "https://videoimg.sooplive.com/thumb.php?id=123",
            ),
            (PlatformId::Chzzk, "https://video-phinf.pstatic.net/a.jpg"),
            (PlatformId::Chzzk, "https://nng-phinf.pstatic.net/a.jpg"),
        ] {
            assert!(validate_vod_image_url(platform, &Url::parse(url).unwrap()).is_ok());
        }
        for url in [
            "https://localhost/a",
            "https://videoimg.sooplive.com.evil.test/a",
            "https://user:secret@videoimg.sooplive.com/a",
            "https://videoimg.sooplive.com:444/a",
            "http://videoimg.sooplive.com/a",
        ] {
            assert!(validate_vod_image_url(PlatformId::Soop, &Url::parse(url).unwrap()).is_err());
        }
        assert!(
            validate_vod_image_url(
                PlatformId::Chzzk,
                &Url::parse("https://videoimg.sooplive.com/a").unwrap()
            )
            .is_err()
        );
    }

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
            let response = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap()
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
    fn fit_downscale_and_no_upscale() {
        for (width, height, expected) in [
            (1920, 1080, (480, 270)),
            (200, 200, (200, 200)),
            (24, 40, (24, 40)),
        ] {
            let mut bytes = Cursor::new(Vec::new());
            DynamicImage::new_rgba8(width, height)
                .write_to(&mut bytes, ImageFormat::Png)
                .unwrap();
            let result = decode(bytes.get_ref()).unwrap();
            assert_eq!((result.width, result.height), expected);
            assert_eq!(result.rgba.len(), (expected.0 * expected.1 * 4) as usize);
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
            "http://liveimg.sooplive.com/a",
            "https://localhost/a",
            "https://liveimg.sooplive.com.evil.test/a",
            "https://user@liveimg.sooplive.com/a",
            "https://liveimg.sooplive.com:8443/a",
        ] {
            assert!(validate_image_url(PlatformId::Soop, &Url::parse(raw).unwrap()).is_err());
        }
        assert!(
            validate_image_url(
                PlatformId::Soop,
                &Url::parse("https://liveimg.sooplive.com/m/123").unwrap()
            )
            .is_ok()
        );
        assert!(
            validate_image_url(
                PlatformId::Chzzk,
                &Url::parse("https://livecloud-thumb.akamaized.net/chzzk/a.jpg").unwrap()
            )
            .is_ok()
        );
    }
}
