# Website demo data honesty

## Toolpath JSON (`public/demo/demo-slice.json`)

- **Model:** `examples/t-wing.stl` (Shale repo examples, AGPL-3.0-or-later).
- **Generator:** `scripts/slice-stl-to-json.mjs` — 2.5D triangle plane-cut + simple infill hatch.
- **Not CuraEngine** on the cloud agent VM.

### CuraEngine attempts on the agent VM

1. **UltiMaker CuraEngine 5.13.0 source** — `cmake` failed: missing Conan package `standardprojectsettings` (full submodule/conan setup not completed).
2. **`libstdc++-14-dev`** installed after initial linker failure; still blocked on CMake deps.
3. **UltiMaker Cura AppImage** — download started (`5.10.2` Linux x64); extract did not finish in time for this commit.

When CuraEngine is available, prefer: `shale-cli slice examples/t-wing.stl ...` and a G-code → JSON trimmer, then update `source` and CLI/MCP copy to **recorded on Linux**.

## CLI / MCP transcripts

- Marked **illustrative** in JSON `label` fields and terminal chrome.
- Layer counts align with `demo-slice.json` (~119 layers).

## 3DBenchy

Creative Tools’ 3DBenchy is **CC BY-ND 4.0** (no derivatives). We did not slice or redistribute it; t-wing is used instead.