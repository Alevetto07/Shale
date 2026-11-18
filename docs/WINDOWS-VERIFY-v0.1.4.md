# Windows verification (v0.1.4)

## Title bar

- **Unit / Rust:** `apps/desktop/src-tauri/src/windows_chrome.rs` tests cover Alt+Space system menu disposition and `GetSystemMenu(..., FALSE)`.
- **Frontend:** Title bar renders SVG minimise / maximise / restore / close icons with `app-region: no-drag` on the button cluster so WebView2 does not swallow clicks inside the drag region.
- **CI (best effort):** `scripts/titlebar-smoke.ps1` on the built `Shale.exe` uses UI Automation to find the three window buttons and toggle maximise. Step is `continue-on-error: true` because headless runners can block foreground interaction; failure does not fail the job but is visible in logs.

## NSIS installer

- **Assets:** Regenerated 150×57 header and 164×314 sidebar BMPs; short English strings in `English.nsh`.
- **CI:** `scripts/installer-screenshots.ps1` sets per-user `LogPixels` to 96 / 120 / 144, runs the setup wizard, captures up to six pages per scale, uploads `dist/installer-screenshots` with the NSIS artefact.