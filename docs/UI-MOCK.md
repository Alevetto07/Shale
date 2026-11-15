# Mocked UI runs (no Tauri)

For layout review and screenshots without the Rust shell:

```bash
cd apps/desktop
VITE_SHALE_MOCK=1 npm run dev
# http://127.0.0.1:1420/?mock=workspace-preview&pane=prepare
```

Query flags:

| param | values |
| --- | --- |
| `mock` | `home`, `home-empty`, `workspace-prepare`, `workspace-preview`, `device-lan`, `device-cloud-in`, `device-cloud-out` |
| `pane` | `home`, `prepare`, `preview`, `device` — initial tab after boot |
| `cloud` | `in` / `out` |
| `recents` | `0` to hide recent list on Home |

Automated captures (Playwright + headless Chromium):

```bash
node scripts/ui-screenshots.mjs --out docs/screens/after --width 1280 --height 800
node scripts/ui-screenshots.mjs --out docs/screens/after --width 1920 --height 1080
```

Screenshots land under `docs/screens/` with manifests. Requires `npm install` in `apps/desktop` (adds `playwright` devDependency).