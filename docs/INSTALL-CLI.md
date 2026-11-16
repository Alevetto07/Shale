# Shale without the window

Two small binaries, no Tauri, no webview, no Node:

| Binary | What it does |
| --- | --- |
| `shale` (`shale.exe` on Windows) | `info`, `printers`, `settings`, `slice` — a terminal slicer |
| `shale-mcp` | MCP server over stdio (default) or `--http 127.0.0.1:PORT` |

Both drive the same `shale-core` the desktop app uses, and both hand the mesh to
an **unmodified CuraEngine** process. They do not bundle CuraEngine: it is a
separate program, and Shale looks for it rather than linking it.

## What they need at run time

1. **CuraEngine** — the binary.
2. **The Cura resource tree** — printer definitions, quality profiles and
   materials. This is what makes the printer list non-empty.

Both can be pointed at explicitly, which is the reliable way on a server.

Bambu printers use the Orca/Bambu profile tree (`SHALE_ORCA_DIR` when you need to
override paths). Shale auto-detects a local checkout or an installed Bambu Studio
when the variable is unset.

### Linux and macOS

```sh
export SHALE_CURAENGINE=/usr/bin/CuraEngine
export SHALE_CURA_DIR=/usr/share/cura/resources
export SHALE_MATERIALS_DIR=/usr/share/cura/resources/materials
export SHALE_ORCA_DIR=/path/to/BambuStudio/resources/profiles
# Optional when DLLs or shared libraries sit beside the engine binary:
export SHALE_CURAENGINE_LIB_DIR=/usr/lib/CuraEngine
```

### Windows (PowerShell)

```powershell
$env:SHALE_CURAENGINE = "C:\Program Files\UltiMaker Cura 5.11\CuraEngine.exe"
$env:SHALE_CURAENGINE_LIB_DIR = "C:\Program Files\UltiMaker Cura 5.11"
$env:SHALE_CURA_DIR = "C:\path\to\Shale\Cura"
$env:SHALE_MATERIALS_DIR = "C:\path\to\Shale\fdm_materials"
$env:SHALE_ORCA_DIR = "C:\path\to\Shale\vendor\BambuStudio\resources\profiles"
```

Run `shale info` after setting these; it lists which variables are still missing
and whether CuraEngine responds.

If the variables are unset, Shale looks next to the working directory and the
executable (a `Cura/` folder with `fdm_materials/` beside it), then at an
installed UltiMaker Cura 5.x. `SHALE_ORCA_DIR` falls back to a local
`vendor/BambuStudio/resources/profiles` checkout, or to an installed Bambu
Studio. The error names every path it tried, so a wrong path is a message, not a
mystery.

## Windows

Unzip `shale-0.1.1-windows-x64.zip` anywhere and run from that folder:

```powershell
.\shale.exe info
.\shale.exe printers --search a1
.\shale.exe slice .\cube.stl --printer bambu_lab_a1 --layer 0.2 --infill 20 -o .\cube.gcode
```

CuraEngine and the resource tree come from an installed UltiMaker Cura 5.x, or
from `scripts/fetch-resources.ps1` in the repository. To use a copy that is not
installed, set all five variables above (including `SHALE_ORCA_DIR`).

## Debian 13 aarch64 (Arduino UNO Q and similar)

Rust builds the binaries natively; CuraEngine is the part Debian packages:

```sh
sudo apt update
sudo apt install -y cura-engine cura-resources   # provides /usr/bin/CuraEngine and /usr/share/cura/resources
```

If the distribution has no `cura-engine` package, build it from source
(UltiMaker/CuraEngine, `cmake -B build && cmake --build build --release`) or
copy a static build, then point `SHALE_CURAENGINE` at it.

Build Shale on the board itself (a release build takes a few minutes on four
cores; there is no cross toolchain to fight):

```sh
sudo apt install -y build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
git clone <repo> shale && cd shale
cargo build --release --locked -p shale-cli -p shale-mcp
```

Cross-compiling from an x86 machine is possible with `cross` and needs a linker
for `aarch64-unknown-linux-gnu`; `scripts/package-cli.sh` honours `TARGET`:

```sh
TARGET=aarch64-unknown-linux-gnu ./scripts/package-cli.sh
```

Install:

```sh
sudo install -m755 target/release/shale-cli target/release/shale-mcp /usr/local/bin/shale /usr/local/bin/
sudo ln -sf /usr/local/bin/shale-cli /usr/local/bin/shale 2>/dev/null || cp target/release/shale-cli /usr/local/bin/shale
shale info
```

## Using the MCP server

The server speaks MCP over stdio, which is what desktop clients expect. A client
entry looks like this (paths adjusted):

```json
{
  "mcpServers": {
    "shale": {
      "command": "/usr/local/bin/shale-mcp",
      "env": {
        "SHALE_CURAENGINE": "/usr/bin/CuraEngine",
        "SHALE_CURA_DIR": "/usr/share/cura/resources",
        "SHALE_MATERIALS_DIR": "/usr/share/cura/resources/materials",
        "SHALE_ORCA_DIR": "/path/to/BambuStudio/resources/profiles"
      }
    }
  }
}
```

`shale-mcp --http 127.0.0.1:8766` serves streamable HTTP instead, and refuses
any address that is not loopback: this server starts prints, so it is not
something to expose.

Seventeen tools are exposed — `list_printers`, `list_materials`, `list_profiles`,
`set_settings`, `load_model`, `transform_model`, `arrange_model`, `slice`,
`slice_summary`, `export_gcode`, the support pair (`analyse_overhangs`,
`propose_supports`, `apply_supports`) and the instruction trio
(`add_instruction`, `list_instructions`, `remove_instruction`).

`python scripts/mcp-smoke.py /usr/local/bin/shale-mcp` performs the initialize
handshake and lists the tools, so a package can be checked before it ships.

## Checking an install

```sh
shale info          # engine path, engine ok, resource paths, printer count
shale printers      # should not be empty
shale settings --printer creality_ender3 | head
```

`info` printing `printers 0` means the resource tree was found but has no
definitions; `cura not found` means the tree itself is missing.

## Packaging

```powershell
pwsh scripts/package-cli.ps1                 # dist/shale-<version>-windows-x64.zip
```

```sh
./scripts/package-cli.sh                     # dist/shale-<version>-linux-x64.tar.gz
```

Both stage the two binaries with this file, `THIRD-PARTY-NOTICES.md` and
`LICENSING.md`, so a zip is a complete, self-describing install.
