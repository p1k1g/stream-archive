# Phase 21.5 follow-up — Native VOD UI polish

This follow-up keeps the Phase 21.5 VOD behavior unchanged and aligns the native VOD page with the compact layout conventions introduced during Phase 21.4.

## UI-only scope

- Match the LIVE/Channels top action bar pattern.
- Keep URL input and status context compact and fixed-height where practical.
- Keep metadata, quality/part selectors, progress, and download controls inside the page scroll area.
- Preserve resize/maximize behavior and avoid vertical stretch creating oversized cards.
- Do not change VOD analyze/download/cancel contracts, provider behavior, persistence, process ownership, or Settings authority.

## Manual smoke check

1. Resize/maximize/restore the native window on the VOD page.
2. Confirm SOOP and CHZZK Analyze still reach `READY`.
3. Confirm quality and part selection remain usable without oversized blank spacing.
4. Confirm Browse, Merge, Download, progress, Cancel, and restart still work.
5. Confirm LIVE/Channels/Settings layouts remain unchanged.
