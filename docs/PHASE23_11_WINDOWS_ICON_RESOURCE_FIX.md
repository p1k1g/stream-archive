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

Phase 23.10 generated icon frames and then mutated the linked executable with a custom `UpdateResource` path. Manual RC showed that Windows could enumerate the resource but decoded only the upper portion of the icon. A first Phase 23.11 attempt switched the frame payload to hand-written DIB data; the new package verifier correctly rejected it because the associated icon still had a blank lower half.

The subsequent strict Rust PNG decode exposed the deeper root cause: the repository's previous `rust-gui/assets/stream-archive-icon.png` had a corrupt DEFLATE stream (`DistanceTooFarBack`). Tolerant decoders could display enough of it for development, but Windows icon generation/runtime branding could decode it incompletely. Phase 23.11 replaces that damaged file with a valid re-encoded copy of the same selected artwork.

The fix also removes custom post-link PE resource mutation entirely.

The Rust GUI build now:

- reads the canonical `assets/stream-archive-icon.png`;
- creates 16, 24, 32, 48, 64, 128 and 256 pixel PNG-backed ICO frames in `OUT_DIR`;
- links that generated ICO through `winresource` / the standard Windows resource compiler path;
- sets the Stream Archive product/file metadata in the same resource build.

Slint continues to embed the canonical source image for the runtime window icon. The portable ZIP still has no standalone PNG/ICO runtime dependency.

## Regression protection

`Verify-WindowsPackage.ps1` extracts the packaged executable's associated Windows icon and rejects a blank/cropped lower half. Packaging contracts also reject a return to the post-link `Set-WindowsExecutableIcon.ps1` path and require the standard resource-compiler integration.

The previous failing CI is intentionally useful evidence: the verifier caught the malformed icon before an artifact could be accepted.

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
