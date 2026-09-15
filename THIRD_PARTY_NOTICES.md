# Third-party notices

Stream Archive uses external media tools but does not bundle them in the portable package. Users install or configure these tools separately, and each tool remains governed by its own license and distribution terms.

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

The Stream Archive portable package intentionally does not redistribute Streamlink, FFmpeg, or yt-dlp binaries. If a future release begins bundling any third-party executable or library, the release process must be updated to include the applicable license texts, attribution notices, source-offer obligations, and artifact-specific compliance checks before distribution.
