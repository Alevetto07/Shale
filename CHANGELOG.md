# Changelog

## Unreleased

On `cursor/model-browser-93f6` and `cursor/bambu-parity-93f6`. Not merged to main.

### Models

- Search MakerWorld and Thingiverse, open a model into a new job, and keep a local catalogue of what you have looked at.

### Prepare

- Slice every plate. Up to four run at once (two is the start), each in its own CuraEngine, and the bar names every plate that is running. Cancel, or closing the window, stops the ones that have not finished. Sending every plate slices any plate that does not yet have G-code.
- Auto orient stands a part on the hull face with the least overhang, then the shortest height, then the widest foot. Quarter turns are still tried. A large mesh only offers its biggest faces. Undo puts the rotation back.
- Orient every part on the plate, with the same undo.
- Drag the cut plane, and lay the cut face down on the bed.
- Per-object overrides for top and bottom layers, support, brim type and width, and seam, on top of the overrides the sheet already had.
- Brim ears: a one-layer disc at each sharp corner of a small foot. Automatic is the default. A round foot grows none.
- Paint support with a brush, or drag a box on the plate. Hold up is a support mesh. Keep out is an anti-overhang mesh.

### Device

- The send dialog maps each sliced tool to an AMS tray or the external spool, and warns when the filament or the bed does not match. The warning does not block the press.
- The preview legend shows time and filament for each feature. History lists sends from this computer beside recent cloud tasks.
- Printer messages decode HMS codes, link the wiki page Bambu Studio uses, and toast a new code. The message table is fetched from Bambu and cached in the app config directory. It is not copied into the repository, and it is not the printer-profile cache.
- A desktop toast for a finished print, a failed print, a pause, and a filament run-out. Each printer has its own switch, on unless you turn it off.
- Files already on the printer can be listed, downloaded, deleted after a confirmation, and printed again without an upload. Reprint of a sliced 3MF reads the plate list in the file, so a file that only has plate 2 does not start plate 1.
- A Bambu send uploads a `.gcode.3mf` (G-code, its MD5, filament and tray map, plate pictures, and the project settings). The plate pictures are an isometric view of the parts, not a flat swatch. The same tray map stays on the start command. The cloud task uploads `plate.gcode.3mf` and carries the same choices.
- The send dialog adds timelapse, bed levelling, flow dynamics calibration, vibration compensation, and use AMS. The starting ticks follow Bambu Studio for the printer on screen. A P1 hides flow calibration. A P2, H2, or X2 sends automatic calibration when those switches are on.

### Projects

- A saved project keeps support boxes, per-object overrides (including brim ears), the AMS spool list, and the tray map from the send dialog.
- A project written before those fields still opens.
