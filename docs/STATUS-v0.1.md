# Shale v0.1 — Batch 1 status

Branch `cursor/deep-field-75ee`, seven commits on top of `0b2e3e4`. Nothing was
pushed, merged, tagged or published; no PR was opened or changed. The A1 at
192.168.1.11 was never contacted and no network was scanned.

Workspace at `96b484c`: **157 passed, 0 failed, 2 ignored**. Both ignored tests
are stopwatches (`boolean_timings_…`, `a_curved_part_does_not_become_a_chain`),
not checks.

## Items

| # | Item | Commit | State |
|---|---|---|---|
| 1 | Overnight queue never starts itself | `7339374` | done |
| 2 | A1's own start and end G-code | `6ab5eb2` | done |
| 3 | Calibration: the end sequence is not a layer | `3fdc256` | done |
| 4 | Boolean timings at 8k / 33k / 80k | `33321d4` | measured where it finishes — see the open defect below |
| 5 | Docs reconciliation | `cd93b1b` (+ in `6ab5eb2`, `6bc006c`) | done |
| 6 | Theme leftovers | `6bc006c` | done |
| 7 | Keyring for the access code | `96b484c` | done; TLS pinning not started |
| 8 | Fake printer + wiring tests | — | **not done** |

### 1. The queue (`7339374`)

`shale_core::queue` is a five-state machine — `idle → armed → printing →
waiting_for_plate`, with `stopped` on a failure. `observe` accepts machine
reports and has no way to start a job. `queue_start_next` refuses unless the
state says a press authorised it, and the refusal names the reason ("waiting for
a plate-cleared press"), because a finished print leaves its part on the bed.
The old "advance when the previous job finishes" effect is gone from `Tabs.tsx`.

Proved through the command layer, not just the state machine: a probe imported
two jobs, sent a running report, and was refused; one press started exactly one.

### 2. The A1 sequence (`6ab5eb2`)

The A1 profile keeps `machine_start_gcode` at zero length; the real text lives in
`… template machine_start_gcode.json` and its end counterpart, as lists of lines.
They are read per nozzle, carried on the catalog, and rendered with the values
the slice resolved. Bambu's names are translated (`nozzle_temperature` ← Cura's
`material_print_temperature`, `bed_temperature_initial_layer_single`,
`filament_type` from the Orca profile, purge/calibration rate from the filament's
volumetric limit). An unknown name fails the slice and names itself; `render`
refuses to return text with a `{` left in it.

A Bambu printer is now refused a file that does not open with `;===== machine:`,
so a Cura-started file cannot be sent to it. Fixtures from the A1's own templates
are committed as `tests/fixtures/bambu-a1/start.txt` and `end.txt`; four tests
assert the banner, the AMS handover, the bed scan, 215/210/60 °C, `H0.4`, and the
`M400` → `M18 X Y Z` tail. In the running window, a sliced bracket reported
`machine_gcode: true`.

### 3. Calibration (`3fdc256`)

The layer-Z pass read `G1 E-2 Z0.2` from the end sequence as a layer at 0.2 mm
and dropped the top band to 0 °C. Z is read only before the last extrusion, with
a regression test whose end sequence carries no Z.

### 4. The boolean (`33321d4`)

The ladder Batch 1 asked for cannot be taken, and that is the finding. A lattice
sphere at 3 000 / 4 940 / 7 820 triangles builds in 575 ms / 1 653 ms / 5 090 ms,
at depths 1 696 / 2 700 / 4 264 — quadratic in the count, linear in depth,
because on a convex body every candidate split plane is a face plane and each
node peels one face. The thirty-thousand rung does not finish in seven minutes.

`BOOLEAN_CAP` therefore drops from 200 000 to 12 000: above it a convex part
would sit on a spinner for minutes rather than return. The concave case the cap
was raised for is untouched — a plate with a bore (2 554 triangles) is 57 ms and
the 1946-triangle bracket is 0.01 s.

### 5. Docs (`cd93b1b`, plus `6ab5eb2`, `6bc006c`)

`LICENSING.md` names every library the host brought in (rustls, ring,
rustls-pki-types, untrusted, windows-sys) and carries the csg.js MIT attribution,
which is an obligation and was only a comment. `HANDOVER.md` keeps its text and
gains a note saying which of it is out of date. README's Bambu paragraph,
`docs/pr-8-note.md` and `docs/bambu-parity.md` match the code.

### 6. Theme (`6bc006c`)

Measured in the window, not by eye: checked menu rows are a 2 px
`rgb(185, 179, 255)` rule and the page contains zero `✓` characters; the speed
legend is five stops in one hue with 1 px gaps (a gapless ramp read as one wash);
the settings footnote gradient is flat; the Device warning uses the accent; the
active plate chip was ink-behind/paper-in-front — a white tile on this background
— and is now a 10 % accent tint with an accent underline.

OS file dialogs and the title bar are not themed. They are the system's.

### 7. The keyring (`96b484c`)

The access code is a password on the printer's MQTT connection and was kept in
`localStorage`. It is in the Windows Credential Manager now (`Shale/<key>`, via
`CredWriteW`/`CredReadW`/`CredDeleteW`), with `secret_set`/`secret_get`/
`secret_clear`; a code left in the old place is moved once and deleted. Proved
against the real vault (`cargo test -p shale-desktop --locked secret`: round trip,
read-back, delete, absent-after-delete). Nothing logs it.

## Open

- **Item 8 was not done.** `fake_printer.py` is nowhere on this machine — not in
  the repo, not in any scratch directory — so it has to be rewritten (MQTT over
  TLS on 8883, implicit FTPS on 990), not committed. The host, queue and Boolean
  wiring it was meant to test was instead driven through the live app by probes,
  which is weaker: it does not run in CI.
- **TOFU TLS pinning is not implemented.** The host trusts the system roots.
  A printer presents a self-signed certificate, so nothing pins the fingerprint
  the first time it is seen.
- **The Boolean wall in item 4 is real and unfixed.** It is now bounded by a cap
  and described in `docs/verification.md`; the fix is an iterative build or a
  kernel that chooses its own planes, not a bigger stack.
- **`docs/STATUS-v0.1.md` is the only status page**; `HANDOVER.md` still
  describes `38729ab` by design, with a pointer.
- **Nothing is pushed.** `git push` cannot read a Username in this shell
  (`terminal prompts disabled`), and Batch 1 forbids pushing anyway. The remote
  tip is `0b2e3e4`; these seven commits are local.
- **PR #8's body on GitHub does not carry the new note** — the local
  `docs/pr-8-note.md` does.
- **Not proved on hardware.** Every host claim — MQTT, FTPS upload, the
  `;===== machine:` gate end to end — is proved against local fixtures or the
  old fake printer, never against an A1.
