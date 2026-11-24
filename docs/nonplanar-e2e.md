# Non-planar end-to-end matrix

`scripts/nonplanar-e2e.ps1` is the acceptance harness for the non-planar
pipeline: it slices every fixture part on every supported Bambu printer in both
`--nonplanar off` and `--nonplanar auto` mode, and records what each run
produced. Everything it writes lands in `%TEMP%\shale-v02\e2e\`, never in the
repository.

## What it does

1. Builds a **release** `shale-cli` through the build-slot wrapper
   (`%TEMP%\shale-v02\cargo-slot.ps1`, which serialises cargo across the
   parallel agent shells). `-SkipBuild` reuses an existing binary.
2. Materialises the four fixtures into `%TEMP%\shale-v02\e2e\inputs\`:

   | part                 | source                                                    |
   |----------------------|-----------------------------------------------------------|
   | `t-wing`             | `examples/t-wing.stl` (committed)                          |
   | `bridge-overhang`    | `examples/bridge-overhang.stl` (committed; `scripts/make_bridge_part.py` regenerates it byte-for-byte) |
   | `dome`               | exported by `tests/export_bench_stls.rs` (`SHALE_EXPORT_BENCH_STLS=1`) |
   | `pillar-beside-dome` | exported by `tests/nonplanar_auto_twing.rs` (`SHALE_E2E_INPUT_DIR=<dir>`) |

   Generated STLs are copied out and deleted from `examples/` again; only
   `bridge-overhang.stl` is a committed fixture.
3. Slices `4 parts x 4 printers x {off, auto}` =
   `bambu_lab_a1`, `bambu_lab_a1_mini`, `bambu_lab_p1s`,
   `bambu_lab_x1_carbon` (all four carry a head envelope in
   `crates/shale-nonplanar/src/envelope.rs`), at `--max-angle 10`.
4. Writes `summary.csv` and one log per slice.

## Running it

From the repository root, in PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\nonplanar-e2e.ps1
```

Useful switches:

```powershell
# recycle the release binary, only re-slice the two auto fixtures
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\nonplanar-e2e.ps1 -SkipBuild -Parts t-wing,bridge-overhang

# a single printer while iterating
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\nonplanar-e2e.ps1 -Printers bambu_lab_a1
```

The script needs the vendored resource trees (`vendor\Cura`,
`vendor\fdm_materials`, `vendor\BambuStudio\resources\profiles`,
`vendor\curaengine\CuraEngine.exe`) and sets `SHALE_CURA_DIR`,
`SHALE_MATERIALS_DIR`, `SHALE_ORCA_DIR`, `SHALE_CURAENGINE` and
`SHALE_BENCH_SUMMARY=1` itself. It also pins `CARGO_TARGET_DIR` to
`%TEMP%\shale-v02\target-e2e` when the caller has not set one, so it never
contends for `target/` with an interactive session. The release build takes a
few minutes; the sixteen slices take roughly a minute each.

Every slice runs under a watchdog: after `-SliceTimeoutSec` (default 300) the
process tree the script itself started (`shale-cli` and its `CuraEngine` child)
is killed and the row records `exit=timeout`, so a single hanging slice cannot
cost the rest of the matrix.

The fixtures are materialised without touching the repository: the dome export
writes `examples\dome.stl`, `examples\chevron-slab.stl` and
`examples\overhang-shelf.stl`, so the script byte-copies whatever was there
first and restores it afterwards (deleting only files it created).

## Output

```
%TEMP%\shale-v02\e2e\
  inputs\*.stl
  <part>-<printer>-<mode>.gcode
  logs\<part>-<printer>-<mode>.log
  summary.csv
```

`summary.csv` columns:

| column            | meaning                                                            |
|-------------------|--------------------------------------------------------------------|
| `part`            | fixture name                                                       |
| `printer`         | printer definition id                                              |
| `mode`            | `off` or `auto`                                                    |
| `exit`            | CLI exit code (`0` = G-code written)                               |
| `print_time_s`    | estimate from the `bench_summary` JSON line                        |
| `layers`          | layer count                                                        |
| `filament_m`      | filament used, metres                                              |
| `nonplanar`       | the `nonplanar …` line: `islands`, `flattened`, `equiv`, `collisions_fixed` |
| `nonplanar_safety`| the `nonplanar_safety …` line: `ok`, `violations`, `max_slope`      |
| `fallback`        | `fallback_planar=` from the `auto surfaces=…` line                  |
| `wall_s`          | wall-clock seconds for that slice                                  |

## Reading a row

* `equiv=true` — the planar part of the print is unchanged by the pipeline;
  a `false` here is a regression, not a quality setting.
* `nonplanar_safety ok=false` or `violations>0` — the head envelope or the
  10° clearance was violated; the slice should also have fallen back to planar
  (`fallback=true`).
* `auto` with `fallback=true` and a `filament_m` identical to the `off` row
  means the planner deliberately declined: either no surface qualified or the
  only candidates were flat undersides that a tilt cannot improve (see
  `auto/plan.rs`, the `89°` rule). That is a decision, not a failure.
* `mode=auto` must never print above the `off` row's `print_time_s` by more
  than a rounding step, and its G-code must stay inside the same model
  envelope.

Results are expected to move while the three M1 workstreams merge; re-run the
harness after every merge and diff the CSV.
