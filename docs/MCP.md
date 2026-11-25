# Shale MCP

`shale-mcp` is a small set of tools for a model that is driving a print. Names say what they do. Arguments are JSON. A tool either returns JSON or an error string you can show to a person.

Start it with the Cura resources and the engine on the path the desktop uses:

```
SHALE_CURA_DIR=... SHALE_MATERIALS_DIR=... SHALE_CURAENGINE=... shale-mcp
```

Add `--http 127.0.0.1:7337` to listen on localhost. Any other address is refused.

One process keeps one job. Printer, settings, models, and non-planar mode stay until `new_job` or `clear_job`. `remove_model` drops one model.

`slice` returns within a couple of seconds. If `status` is `running`, poll `slice_progress`, then read `slice_result`. A cube is under a minute. The aerospike is about 10 minutes, which is longer than a 600 second tool call. To compare layer height or infill, call `compare_slices` (the same tool as `slice_variants`) once. Call `list_plates` before `slice` with a plate number. `arrange_model` keeps rotation and scale.

Projects and "don't suggest this again" are stored under `SHALE_CONFIG_DIR`. When that is unset, they use the desktop app's config directory (`dev.shale.desktop`: `%APPDATA%\dev.shale.desktop` on Windows, `$XDG_CONFIG_HOME/dev.shale.desktop` or `$HOME/.config/dev.shale.desktop` on Linux). That is the folder the window writes `recents.json` into. It is not the catalogue cache.

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

Use this for the printers bound to the signed-in Bambu account. Read only.

No arguments. If this process is not signed in, the error is exactly `sign in to Bambu in Shale first`.

### `printer_status`

Use this for one machine: state, the current job, progress, time left, nozzle and bed temperatures, and AMS slots with material, colour, and humidity when the report has them.

```json
{ "id": "01P00C000000000" }
```

The cloud device list does not include temperatures or AMS humidity. A LAN status report includes temperatures and still has no AMS humidity. Those fields are null rather than guessed. This tool does not start, pause, or stop a print.

### `list_print_jobs`

Use this for recent print tasks on the signed-in account (`GET /v1/user-service/my/tasks`). Read only.

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

Use this to read the resolved plate: printer, quality, temperatures, and the settings a person would see. Process settings embedded in a 3MF are included. A value you set yourself wins over the file.

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

### `new_job` and `clear_job`

Use either one at the start of a task. Both clear models, the printer, settings, and non-planar mode. Without this, the next task inherits the previous job.

No arguments.

### `remove_model`

Use this to drop one model and keep the rest.

```json
{ "id": 1 }
```

### `slice`

Use this to start a slice. It returns within a couple of seconds. A finished slice includes the summary. A running slice needs `slice_progress`, then `slice_result`.

```json
{ "plate": 1, "all_plates": false }
```

Call `list_plates` before `plate`. `all_plates` true slices every plate. A cube is under a minute. The aerospike is about 10 minutes.

### `slice_progress`

Use this while `slice` says `running`. `status` is `running`, `done`, or `failed`. `fraction` is 0 to 1.

No arguments.

### `slice_result`

Use this for the summary once `slice_progress` says `done`: time, filament, layers, warnings.

No arguments. Errors while the slice is still running.

### `compare_slices`

Use this to compare settings in one call, such as 0.12 mm against 0.20 mm, or 15% infill against 30%. Do not call `slice` once per setting. `slice_variants` is the same tool.

```json
{
  "patches": [
    { "layer_height": 0.12 },
    { "layer_height": 0.20 }
  ]
}
```

Up to 8 patches. The current settings are not changed. `apply_variant` makes one row current.

### `slice_summary`

Use this to read the last slice: layers, time, filament, warnings, and a short non-planar note.

No arguments. Errors if nothing has been sliced.

### `slice_stats`

Use this for a breakdown of the last slice. Nothing is sliced again.

```json
{ "price_per_kg": 20, "layers": false }
```

Returns time, filament metres, grams, cost, layer count, max and average speed, seconds in walls, infill, supports, skirt, travel, ironing, and anything else, grams per extruder (only the tools that were selected), and purge waste. `layers` true adds one duration per layer. Omit `price_per_kg` for 20 EUR.

### `slice_variants`

Same tool as `compare_slices`. Use it to compare patches, not to slice one setting at a time. The current settings are not changed.

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

Use this to see projects opened or saved recently, newest first. This is the desktop app's list when `SHALE_CONFIG_DIR` is unset.

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

## Camera

These three tools look at a picture. None of them starts, pauses, or jogs a printer. Pausing, when you have opted in, is a button in the Device pane.

### `get_camera_snapshot`

Use this for one JPEG from the chamber.

```json
{ "host": "192.168.1.42", "model": "bambu_lab_a1", "code": "12345678" }
```

`model` chooses the protocol. `bambu_lab_a1` and the P1 series are JPEG over TLS on port 6000. `bambu_lab_p2s` (and X1, H2) is RTSPS on port 322. This build does not decode H.264, so a P2S stream that is only H.264 returns an error that says so rather than a made-up picture. The access code is not repeated in the answer. The result is an image, plus a short line naming the path and the port.

A wrong access code, LAN-only live view switched off, or a camera switched off on the printer, each come back as an error that says which one it is.

### `camera_status`

Use this to ask whether that camera can be reached, without returning the picture. The same arguments as `get_camera_snapshot`. The answer has `path`, `port`, `reachable`, `jpeg`, `faultCode`, and `fault`. `faultCode` is one of `wrong_access_code`, `lan_live_view_off`, `camera_disabled`, `unreachable`, `cloud_unavailable`, `protocol`, or null.

### `get_failure_status`

Use this to judge one JPEG for spaghetti, a part that has come off the bed, or a blob. The judgement runs on this computer. One frame does not notify: the same finding has to hold for three frames in the desktop watch before it counts. This tool reports that one frame and sets `alert` to false unless you would have asked for a single frame, which you cannot.

```json
{ "path": "/tmp/chamber.jpg", "previous": "/tmp/chamber-before.jpg", "confidence": 0.6, "sensitivity": "medium" }
```

`previous` is optional and is how a detachment is seen. `sensitivity` is `low`, `medium`, or `high`. A path inside the catalog cache is refused. The tool does not pause the printer.

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
