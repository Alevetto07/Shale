# Licensing

Shale is a new program. **Its own source code does not have a licence yet.** Alessio (the repository owner) needs to choose one before the project is distributed. Until that choice is written down here, do not assume the code is open source, and do not copy it into another project.

The pieces Shale runs *with* keep their own licences. They are not relicensed by being used from this repository.

## CuraEngine (AGPL-3.0)

[CuraEngine](https://github.com/Ultimaker/CuraEngine) is the slicer. It is published under the GNU Affero General Public License, version 3.

Shale does not link CuraEngine, does not copy its source, and does not modify it. The desktop app starts whatever `CuraEngine` binary you point it at, as a separate process. The CLI backend uses `CuraEngine slice`. The Arcus backend uses `CuraEngine connect` and speaks the engine's existing socket protocol. No CuraEngine binary is committed to this repository.

The Arcus framing (header `0x2BAD0100`, a protobuf size, an FNV-1a type id of `GetTypeName()`, then the protobuf bytes, with a zero word as keepalive) is reimplemented in Rust from the published protocol. libArcus 4.10, which Cura 5.13 ships, is LGPL-3.0. Later libArcus releases are AGPL-3.0. Shale does not link either library and does not copy its source. `Cura.proto` is not vendored; the field numbers Shale uses are the ones from the 5.13.0 tag.

If you distribute Shale together with a CuraEngine binary, that binary remains under the AGPL-3.0 and you still have to comply with it (including offering its source). That obligation is about CuraEngine, not about Shale's own code.

## Cura printer definitions and profiles (LGPL-3.0)

Printer definitions, extruder trains, quality profiles, variants, and intents come from the `resources/` tree of [UltiMaker Cura](https://github.com/Ultimaker/Cura), which is LGPL-3.0. Shale does not vendor that tree. `scripts/fetch-resources.sh` checks out Cura **5.13.0** (commit `1fb8a7610460e03ec62da950d1c173a249b23b59`) into `vendor/Cura/`, sparse, so the definitions sit next to Cura's own `LICENSE`.

Shale reads those files as data. It does not link Cura, and it does not use Uranium (Cura's Python application framework). Uranium pulls in PyQt6, which is GPL-3.0 / commercial, and Shale is deliberately independent of it. Setting formulas are evaluated by Shale's own Rust subset, not by Uranium and not by the LGPL [CuraFormulaeEngine](https://github.com/Ultimaker/Cura) library. That subset is incomplete; see the README. If it later becomes a burden, linking CuraFormulaeEngine would be an LGPL dependency and would need to be called out here.

## fdm_materials (CC0-1.0)

Material profiles come from [fdm_materials](https://github.com/Ultimaker/fdm_materials), dedicated to the public domain under CC0-1.0. The fetch script pins commit `82f433b2cfe0ac36c974443b80c9294f31b79224` and leaves that repository's `LICENSE` beside the XML files in `vendor/fdm_materials/`.

## Space Grotesk (SIL Open Font License 1.1)

The wordmark is set in Space Grotesk, weight 600. Interface text is IBM Plex Mono. Both are SIL Open Font License 1.1. The font files and the OFL texts are in `apps/desktop/src/fonts/` (`OFL.txt` for Space Grotesk, `IBMPlexMono-OFL.txt` for Plex). They are bundled with the app and are not loaded from a font host at runtime.

## STEP tessellation (Apache-2.0)

Shale tessellates STEP boundary representation itself, instead of asking for an STL. The kernel is [truck](https://github.com/ricosjp/truck): `truck-stepio`, `truck-modeling`, `truck-meshalgo`, and `truck-topology`, all Apache-2.0. The STEP parser underneath (`ruststep`) is Apache-2.0. The constrained Delaunay step uses `spade`, which is MIT OR Apache-2.0. Cargo fetches them. Their source is not copied into this repository. Shale's own code is still unlicensed, as stated at the top of this file.

Open CASCADE (OCCT) was the other candidate. It is LGPL-2.1 with an additional exception. The exception lets a program use an unmodified OCCT under terms other than the LGPL, provided OCCT stays replaceable: if you modify OCCT, those changes must be released under LGPL-2.1 plus the same exception, and a combined binary still has to let a recipient swap in their own OCCT. Shipping Shale linked to OCCT would mean shipping that licence, offering OCCT's source, and keeping the library separable. That is a real distribution obligation, so Shale does not link OCCT. truck covers the STEP files this version tessellates (planes, cylinders, spheres, NURBS shells). Files that only a boolean kernel can rebuild are reported as unsupported rather than sent through OCCT.

## MCP SDK (Apache-2.0)

`crates/shale-mcp` depends on the [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk) crate to speak the Model Context Protocol. That crate is Apache-2.0. Shale does not copy its source; Cargo fetches it as a normal dependency. Shale's own code is still unlicensed, as stated at the top of this file.

## What you need to do before a release

1. Choose a licence for Shale's own code and replace the note at the top of this file.
2. Keep the fetch script's pins, and ship or document the Cura `LICENSE` and the fdm_materials `LICENSE` next to the data.
3. If a CuraEngine binary is bundled, ship it unmodified and comply with the AGPL-3.0 for that binary.
4. If a later version links OpenCASCADE, add the LGPL-2.1 text and the OCCT exception, and keep that library replaceable. The current tessellator does not.
