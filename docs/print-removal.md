# Camera print removal

This is a design note. The code in `crates/shale-core/src/removal.rs` is a stub. It does not open a camera and it does not send G-code.

## When it should run

A later version can clear a finished print without someone standing at the machine:

1. The user arms the feature. The default is off (`SHALE_PRINT_REMOVAL` unset).
2. Shale watches a snapshot URL the user already uses for that printer. Moonraker serves `webcam/?action=snapshot` (or the URL in its webcam config). OctoPrint serves the same shape at `/webcam/?action=snapshot`. Both are read-only GETs.
3. Two checks, both required: the job Moonraker or OctoPrint reports is complete, and the bed image no longer matches the empty-bed picture taken before the print. A print that failed or was cancelled does not qualify.
4. Only then would Shale offer the user's removal sequence: a short list of moves they wrote (park, sweep, present). The sequence is data on the printer profile, not something Shale invents.

## What this stub does

`plan` returns the gate, the sequence, and `will_move_printer: false`. Arming the flag changes the note. It still does not connect. A future runner must refuse to act while that flag is false.

## Left for the real version

* Snapshot fetch, and a stored empty-bed frame per printer.
* A small difference check (downsampled greyscale), not a model of the part.
* Confirmation in the window before the first automatic run.
* The actual send path, whichever of Moonraker `printer.gcode.script` or OctoPrint's API the user picked, with a timeout and a stop if the head is not where the sequence expects.
