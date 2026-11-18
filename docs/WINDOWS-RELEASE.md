# Windows release build (v0.1)

Build the NSIS installer on **Windows 10/11** with Visual Studio 2019+ Build Tools, CMake, Node 20+, Rust 1.90+, and Git (`core.longpaths` enabled).

## One-time setup

```powershell
git config --global core.longpaths true
rustup toolchain install 1.90.0
rustup default 1.90.0
```

Install UltiMaker Cura 5.x **or** fetch resources into `vendor/`:

```powershell
.\scripts\fetch-resources.ps1
$env:SHALE_CURA_DIR = "$PWD\vendor\cura"
$env:SHALE_MATERIALS_DIR = "$PWD\vendor\materials"
```

Point at CuraEngine if it is not on `PATH` (typical: `C:\Program Files\UltiMaker Cura 5.x\CuraEngine.exe`):

```powershell
$env:SHALE_CURAENGINE = "C:\Program Files\UltiMaker Cura 5.10\CuraEngine.exe"
```

## Build commands

From the repository root:

```powershell
cd apps\desktop
npm ci
npm run build
npm run tauri build
```

The installer is written to:

`apps\desktop\src-tauri\target\release\bundle\nsis\Shale_0.1.0_x64-setup.exe`

It installs **per user** (`%LOCALAPPDATA%\Shale`), registers `.shale` as Shale's own type, adds Open With entries for model formats without stealing default handlers, and ships `THIRD-PARTY-NOTICES.md` beside `Shale.exe`.

Optional packaging for headless tools:

```powershell
.\scripts\package-cli.ps1
```

## Verify before shipping

```powershell
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
.\scripts\installer-smoke.ps1
```

Smoke test expects the installer path above and checks associations, launch, second-instance handover, and clean uninstall.

Regenerate NSIS bitmaps after editing colours:

```powershell
python scripts/generate-nsis-art.py
```

### Optional CLI/MCP component

Tauri 2's bundled NSIS script has no checkbox component sections without forking the entire installer template. Shale ships headless tools via `scripts/package-cli.ps1` beside the GUI installer instead; see `apps/desktop/src-tauri/windows/installer-hooks.nsh`.