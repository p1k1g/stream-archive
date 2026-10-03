# Third-party notices

Stream Archive uses external media tools but does not bundle them in the Windows, Linux, or macOS release packages. Users install or configure these tools separately, and each tool remains governed by its own license and distribution terms.

## Streamlink

- Project: https://github.com/streamlink/streamlink
- License: BSD 2-Clause
- Role in Stream Archive: LIVE stream handling and CHZZK media extraction

Streamlink is executed as an external program. Stream Archive does not incorporate Streamlink source code into its own binaries.

## FFmpeg

- Project: https://ffmpeg.org/
- License information: https://ffmpeg.org/legal.html
- Role in Stream Archive: media remuxing/finalization and related media processing

Most FFmpeg source is licensed under LGPL 2.1 or later. A build configured with GPL components can instead be distributed under GPL terms. The exact license obligations therefore depend on the FFmpeg build installed by the user.

FFmpeg is executed as an external program and is not bundled with Stream Archive.

## yt-dlp

- Project: https://github.com/yt-dlp/yt-dlp
- Role in Stream Archive: SOOP VOD metadata/download flow

The yt-dlp source repository is licensed under the Unlicense. Some official release executables include third-party components and are distributed under additional terms; notably, PyInstaller-bundled executables contain GPLv3+ licensed code. Refer to the yt-dlp project's own licensing documentation for the specific artifact you use.

yt-dlp is executed as an external program and is not bundled with Stream Archive.

## Independence

Stream Archive is an independent project and is not affiliated with, endorsed by, sponsored by, or officially associated with SOOP, NAVER, CHZZK, Streamlink, FFmpeg, or yt-dlp.

Product names, service names, trademarks, and logos belong to their respective owners.

## Redistribution policy

Stream Archive release packages intentionally do not redistribute Streamlink, FFmpeg, or yt-dlp binaries. If a future release begins bundling any third-party executable or library, the release process must be updated to include the applicable license texts, attribution notices, source-offer obligations, and artifact-specific compliance checks before distribution.

## 플랫폼 식별 이미지

LIVE의 오프라인 표시에는 각 서비스가 공개 웹사이트에서 제공하는 다음 식별 이미지를 사용합니다. 원본 전체를 비율 유지하여 표시하며 방송 썸네일과 구분합니다.

- SOOP: `rust-gui/assets/platform-soop.png` — https://res.sooplive.com/images/mobile/afreeca_mobile.png (SOOP 웹사이트 `apple-touch-icon`)
- CHZZK: `rust-gui/assets/platform-chzzk.png` — https://ssl.pstatic.net/static/nng/glive/icon/favicon.png (CHZZK 웹사이트 favicon)

명칭·상표·로고의 권리는 해당 권리자에게 귀속됩니다. 프로젝트의 코드 라이선스는 해당 상표에 대한 권리를 부여하지 않습니다. Stream Archive는 해당 서비스와 제휴·승인 관계가 없습니다.
