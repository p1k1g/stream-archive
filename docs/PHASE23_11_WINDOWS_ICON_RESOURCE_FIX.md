# Phase 23.11 — Windows Icon Resource Fix

Phase 23.11 is a narrow release-candidate hotfix for the Windows application icon defect found during manual validation of the Phase 23.10 release artifact.

Baseline main commit: `1d1af4cc0f862aafae1fa984405e777e26ab0008`.

## Manual RC finding

Phase 23.10 correctly embedded an icon resource and passed package/CI checks, but the actual Windows release artifact rendered the icon incorrectly:

- Explorer showed only the upper portion of the icon.
- The title-bar/taskbar icon was incomplete.
- Inspection of the embedded 256x256 `RT_ICON` resource showed that the lower half of the image was blank/cropped.

The canonical source PNG itself is intact. The defect was in the generated ICO/PE resource payload.

## Root cause and fix

Phase 23.10 generated each ICO frame as a PNG payload and then inserted those bytes directly as `RT_ICON` resources.

Phase 23.11 replaces those payloads with native 32-bpp Windows DIB icon frames:

- BITMAPINFOHEADER
- bottom-up BGRA XOR bitmap
- 1-bpp AND mask
- 16, 24, 32, 48, 64, 128 and 256 pixel entries

This format matches the native `RT_ICON` resource representation and avoids the cropped rendering observed with the previous embedded PNG payloads.

The Slint window icon continues to use the canonical embedded source artwork. No renderer, provider, runtime, database or package-layout behavior changes in this phase.

## Regression protection

`Set-WindowsExecutableIcon.ps1` now:

- validates all seven ICO entries;
- requires BITMAPINFOHEADER-backed DIB frames;
- embeds the generated frames into `StreamArchive.exe`;
- extracts the associated icon again after embedding;
- rejects an icon whose lower half is effectively blank/cropped.

`Verify-WindowsPackage.ps1` independently performs the same lower-half visibility check against the packaged `StreamArchive.exe`.

Packaging guards prevent the PE icon path from returning to PNG-backed `RT_ICON` payloads without an explicit contract change.

## Scope boundary

This phase does not change:

- FemtoVG/software renderer selection;
- LIVE/VOD behavior;
- yt-dlp/FFmpeg/Streamlink discovery;
- SQLite schema or stored data;
- Queue/History/backup formats;
- Windows Native-only package layout;
- Linux/macOS CLI/headless packaging;
- version `0.5.2`;
- tags or GitHub Releases.

## Required validation

Automated:

- RuntimeContracts
- Windows package build
- Windows package verification
- Windows release archive verification
- existing Windows/Linux/macOS core gates

Manual after a fresh Windows artifact:

- Explorer large/small icon
- title-bar icon
- running taskbar icon
- taskbar pin/unpin/re-pin
- optional Windows icon-cache refresh if an old icon remains cached

The phase may create a PR and reach Ready for review, but must not merge itself or publish `v0.5.2`.
