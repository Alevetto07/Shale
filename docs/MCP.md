# Shale MCP

`shale-mcp` is a small set of tools for a model that is driving a print. Names say what they do. Arguments are JSON. A tool either returns JSON or an error string you can show to a person.

Start it with the Cura resources and the engine on the path the desktop uses:

```
SHALE_CURA_DIR=... SHALE_MATERIALS_DIR=... SHALE_CURAENGINE=... shale-mcp
```

Add `--http 127.0.0.1:7337` to listen on localhost. Any other address is refused.

Projects and "don't suggest this again" are stored under `SHALE_CONFIG_DIR`, or `$HOME/.config/shale` when that is unset.

## Printers and materials

### `list_printers`

Use this to see which machines the catalogue can slice for.

No arguments. Returns an array of `{id, name, manufacturer}`.

### `list_materials`

Use this to pick a filament id for `set_settings`.

```json
{ "printer_id": "bambu_lab_a1" }
```

`printer_id` may be omitted. The current printer is used.

### `list_profiles`

Use this to see nozzle variants and quality profiles.

```json
{ "printer_id": "bambu_lab_a1" }
```

### `list_my_printers`

Use this for the printers on the signed-in Bambu account. Read only.

No arguments. If this process is not signed in, the error is exactly `sign in to Bambu in Shale first`.

### `printer_status`

Use this for one machine: state, the current job, progress, time left, nozzle and bed temperatures, and AMS slots with material, colour, and humidity when the report has them.

```json
{ "id": "01P00C000000000" }
```

The cloud device list does not include temperatures or AMS humidity. A LAN status report includes temperatures and still has no AMS humidity. Those fields are null rather than guessed. This tool does not start, pause, or stop a print.

### `list_print_jobs`

Use this for recent and current jobs. Read only.

No arguments. The same sign-in error as `list_my_printers` when there is no account.

## Models

### `load_model`

Use this to put a model on the bed. STL, 3MF, and the other formats the app imports.

```json
{ "path": "/home/me/bracket.stl", "plate": 1, "ignore_project_settings": false }
```

`plate` is 1-based and matches `list_plates`. Omit it for plate 1.

### `list_plates`

Use this after `load_model` to see which plates a 3MF contained. The result is `{ "plates": [ { "number", "name", "objects" } ] }`. `number` is the same 1-based plate `load_model` and `slice` take. An STL is plate 1.

No arguments.

### `transform_model`

Use this to move, turn, or scale one model.

```json
{ "id": 1, "rotation_deg": [0, 90, 0] }
```

`translation` and `scale` are optional arrays of three numbers. Omitted axes stay as they are.

### `arrange_model`

Use this to centre a model on the bed and drop it down. Rotation and scale are kept.

```json
{ "id": 1 }
```

## Settings

### `get_settings`

Use this to read the resolved plate: printer, quality, temperatures, and the settings a person would see.

```json
{ "printer_id": "bambu_lab_a1" }
```

`printer_id` may be omitted.

### `set_settings`

Use this to change the slice. Only the fields you send are changed.

```json
{
  "adhesion_type": "brim",
  "brim_width": 8,
  "support_structure": "tree",
  "infill_sparse_density": 15,
  "settings": { "draft_shield": "limited" }
}
```

`adhesion_type` is `none`, `skirt`, or `brim`. `support_structure` is `normal` or `tree`. `auto` is refused with `support structure 'auto' is not available on this engine yet. Use normal or tree.`

`settings` is any other Cura key, the same as the CLI `--set KEY=VALUE`. An unknown key is an error. A close key is named: `adhesion` comes back as `Did you mean 'adhesion_type'?`.

`nonplanar_mode` is `off`, `curved-tops`, or `auto`. `nonplanar_max_angle_deg` is greater than 0 and at most 10.

## Suggestions

### `get_suggestions`

Use this for the ranked list of tips and warnings. Warnings come first. The app shows three; this tool returns the full list unless you set `limit`.

```json
{ "limit": 3 }
```

Optional fields, all omitted when you do not know them:

- `slots`: spools the project uses. `{ "index": 0, "filament_type": "PLA" }`
- `device_slots`: spools the connected printer has loaded
- `humidity_percent`: 0 to 100, only when the printer reported it
- `hours_since_dry`: only from spool data
- `tags`: the only tag that changes a suggestion is `hot-use`
- `dismissed`, `global_dismissed`, `shown_once`: ids already hidden
- `nonplanar_mode`: `off`, `auto`, or `curved-tops`
- `multi_material`: true when the printer has an AMS

### `apply_suggestion`

Use this to apply one fix and then hide that suggestion on this project. This is a different tool from `get_suggestions`.

```json
{ "id": "brim_contact:1" }
```

`id` is the suggestion id or the rule id from `get_suggestions`. A suggestion with nothing to change is an error. Moving a model onto another plate is reported in `skipped` and is left to the app.

### `dismiss_suggestion`

Use this to hide a suggestion without changing settings. This is a different tool from `apply_suggestion`.

```json
{ "id": "brim", "forever": false }
```

`forever` true stores the rule for every project. `forever` false hides it on this project only.

## Slicing

### `slice`

Use this to slice the loaded models.

```json
{ "plate": 1, "all_plates": false }
```

`all_plates` true slices every plate of a 3MF. This can take a while. The result is the same summary as `slice_summary`.

### `slice_summary`

Use this to read the last slice: layers, time, filament, warnings, and a short non-planar note.

No arguments. Errors if nothing has been sliced.

### `slice_stats`

Use this for a breakdown of the last slice. Nothing is sliced again.

```json
{ "price_per_kg": 20, "layers": false }
```

Returns time, filament metres, grams, cost, layer count, max and average speed, seconds in walls, infill, supports, travel, and ironing, grams per extruder, and purge waste. `layers` true adds one duration per layer. Omit `price_per_kg` for 20 EUR.

### `slice_variants`

Use this to compare a few setting patches of the same model. The current settings are not changed.

```json
{
  "patches": [
    { "infill_sparse_density": 15 },
    { "infill_sparse_density": 30 },
    { "layer_height": 0.12 },
    { "support_structure": "tree" }
  ],
  "concurrency": 2,
  "price_per_kg": 20
}
```

At most 8 patches. Each patch is sliced in its own engine process and its own temp file. `concurrency` is how many run at once: 1, 2, or 3. Omit it and Shale uses 2, or 3 when the machine has at least four cores and 6 GB free, or 1 when memory is tight.

Each row has `index`, `label`, `print_time_seconds`, `filament_metres`, `grams`, `cost`, `support_seconds`, `warnings`, and `error`. A failed patch sets `error` and the others still come back.

### `apply_variant`

Use this to make one compared patch the current settings.

```json
{ "index": 1 }
```

`index` is the row from `slice_variants`, starting at 0.

### `export_gcode`

Use this to copy the last slice to a path.

```json
{ "path": "/home/me/bracket.gcode" }
```

## Supports and instructions

### `analyse_overhangs`

Use this to see where a model overhangs.

```json
{ "id": 1 }
```

### `propose_supports`

Use this to build a support plan. It does not turn supports on until `apply_supports`.

```json
{ "id": 1 }
```

### `apply_supports`

Use this to keep the proposed supports. Pass region ids to keep a subset.

```json
{ "ids": [1, 2] }
```

Omit `ids` to keep every region.

### `list_instructions`

Use this to read custom G-code instructions on the job.

No arguments.

### `add_instruction`

Use this to add one instruction the slicer will emit.

```json
{ "kind": "pause", "layer": 10 }
```

Kinds: `pause` (layer or height_mm, filament_change, message), `temperature` (layer, nozzle_c, bed_c, wait), `fan` (height_mm, percent), `beep` (layer), `message` (layer, text), `park_and_wait` (layer, x, y, message). An unknown kind is an error.

### `remove_instruction`

Use this to delete one instruction by id.

```json
{ "id": 1 }
```

## Projects

### `list_recent_projects`

Use this to see projects opened or saved recently, newest first.

No arguments. Each entry has `path`, `name`, `printer`, `saved_at_ms`, and `missing`.

### `open_project`

Use this to load a `.shale` project into the session. The models loaded now are replaced.

```json
{ "path": "/home/me/bracket.shale" }
```

### `save_project`

Use this to write the open models as a `.shale` project. Meshes are stored as STL.

```json
{ "path": "/home/me/bracket.shale" }
```

## The same jobs from the terminal

`shale-cli` covers the file-shaped tools:

```
shale projects
shale open bracket.shale
shale save bracket.stl -o bracket.shale
shale stats --gcode bracket.gcode --price 20 --layers
shale compare bracket.stl --patch infill_sparse_density=15 --patch infill_sparse_density=30
shale suggest bracket.stl --json
```

`compare` is `slice_variants`. It does not change a saved project; apply the row you want with `apply_variant` in MCP, or slice again with those settings.
