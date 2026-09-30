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

Phase 23.10 generated icon frames and then mutated the linked executable with a custom `UpdateResource` path. Manual RC showed that Windows could enumerate the resource but decoded only the upper portion of the icon. A first Phase 23.11 attempt switched the frame payload to hand-written DIB data; the package verifier correctly rejected it because the associated icon still had a blank lower half.

A later attempt moved icon generation into `build.rs`, which exposed that the repository PNG used by the experimental path was not reliably decodable in CI (`UnexpectedEof`). Phase 23.11 therefore removes all custom runtime/build-time image conversion.

The final fix uses two verified canonical assets produced from the same selected artwork:

- `rust-gui/assets/stream-archive-icon.png` for the Slint Window icon;
- `rust-gui/assets/stream-archive.ico` containing 16, 24, 32, 48, 64, 128 and 256 pixel Windows icon entries.

`build.rs` links the checked-in ICO through `winresource` / the standard Windows resource compiler path. There is no post-link `UpdateResource` mutation and no image decoder/generator dependency in the build script.

The portable ZIP still has no standalone PNG/ICO runtime dependency: both assets are build inputs only.

## Regression protection

`Verify-WindowsPackage.ps1` extracts the packaged executable's associated Windows icon and rejects a blank/cropped lower half. Packaging contracts reject a return to post-link icon mutation or build-time image regeneration and require the canonical checked-in ICO plus `winresource`.

The previous failing CI runs are useful evidence that the verifier/build boundary now fails closed instead of accepting a visually broken icon.

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
