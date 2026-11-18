## Shale v0.1.4

Installer polish, Windows title-bar controls, grouped Settings, and a skippable first-run setup wizard.

### Assets

- **Windows desktop:** `Shale_0.1.4_x64-setup.exe`
- **Windows headless:** `shale-0.1.4-windows-x64.zip` (`shale.exe`, `shale-mcp.exe`)
- **Linux headless:** `shale-0.1.4-linux-x64.tar.gz` (`shale`, `shale-mcp`)

### Highlights

- NSIS branding tuned for 100–150% display scaling; shorter installer copy.
- Custom title bar uses Win11-style SVG window glyphs with correct no-drag hit targets.
- Start-screen **Settings** is a searchable sidebar (General, Appearance, Printers, Profiles, Slicing, Bambu account, Import, Advanced, About).
- First-run wizard: theme, printer, slicer import scan, Bambu login — all skippable; re-run from Settings.

### Known limitations

- Slicer import reports detected JSON presets; deep preset conversion from Orca/Prusa/Cura formats is not automatic in this release.
- Installer screenshots in the PR are mock or CI captures; validate on your own display scale before tagging.