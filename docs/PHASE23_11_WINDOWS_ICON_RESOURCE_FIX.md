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

The follow-up investigation exposed two separate problems in the experimental fixes:

- the original repository PNG used by the first build-time generation attempt was damaged and failed strict PNG decoding with `UnexpectedEof`;
- a later `winresource` attempt referenced `assets/stream-archive.ico` as a path relative to the generated `resource.rc`, so `rc.exe` could not find the file.

The final Phase 23.11 path removes custom post-link PE mutation and keeps one validated canonical artwork source:

- `rust-gui/assets/stream-archive-icon.png` is the canonical source for both Slint runtime branding and Windows icon generation;
- `build.rs` decodes that validated PNG and generates 16, 24, 32, 48, 64, 128 and 256 pixel ICO frames into Cargo `OUT_DIR`;
- the generated ICO path is absolute and is handed to `winresource`, which uses the standard Windows resource compiler/linker path;
- the generated ICO is a build intermediate only and is not copied into the portable package.

The portable ZIP therefore still has no standalone PNG/ICO runtime dependency.

## Regression protection

`Verify-WindowsPackage.ps1` extracts the packaged executable's associated Windows icon and rejects a blank/cropped lower half. Packaging contracts prevent a return to post-link `UpdateResource` mutation, require the canonical validated PNG, require all seven icon sizes, require `OUT_DIR` generation, and require `winresource` resource linking.

The earlier failing CI runs are intentionally useful evidence: malformed/truncated resources and invalid paths now fail closed before a release artifact can be accepted.

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
