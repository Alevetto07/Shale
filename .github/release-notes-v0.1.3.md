## Shale v0.1.3

First release candidate for **main**: a Bambu-focused slicer using **unmodified CuraEngine 5.13**, with a Tauri desktop app, CLI, and MCP server sharing one Rust core.

### Downloads

- **Windows desktop:** `Shale_0.1.3_x64-setup.exe`
- **Windows headless:** `shale-0.1.3-windows-x64.zip` (`shale.exe`, `shale-mcp.exe`)
- **Linux headless:** `shale-0.1.3-linux-x64.tar.gz` (`shale`, `shale-mcp`)

Verify downloads with `SHA256SUMS.txt`.

### Highlights

- Bambu Lab printers from Bambu Studio profile data; default **A1** with machine start/end G-code preserved
- Desktop: prepare, slice, layer preview, device view (LAN MQTT/FTPS), project tabs and multi-plate jobs
- CLI: `info`, `printers`, `settings`, `slice` with friendly quality names and absolute paths for CuraEngine
- MCP: same job API for agents (stdio or loopback HTTP)
- Windows installer checks (GUI subsystem PE), catalogue performance fixes, and UI polish from the v0.1 PR stack

### Runtime requirements

Install **CuraEngine 5.13** and Cura/Bambu profile data separately (or run `scripts/fetch-resources.sh` from source). Shale does not bundle the engine in these assets.

### Licence

Shale is **AGPL-3.0-or-later**. CuraEngine and Bambu Studio profiles remain **AGPL-3.0** when you supply them at runtime. See `LICENSE`, `README.md`, and `LICENSING.md`.