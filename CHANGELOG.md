# Changelog

## Unreleased

On `cursor/model-browser-93f6` and `cursor/bambu-parity-93f6`. Not merged to main.

### Models

- Search MakerWorld and Thingiverse, open a model into a new job, and keep a local catalogue of what you have looked at.

### Prepare

- Slice every plate, with the plate named on the progress bar. Sending every plate slices any plate that does not yet have G-code.
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
- Files already on the printer can be listed, downloaded, deleted after a confirmation, and printed again without an upload.

### Projects

- A saved project keeps support boxes, per-object overrides (including brim ears), the AMS spool list, and the tray map from the send dialog.
- A project written before those fields still opens.
