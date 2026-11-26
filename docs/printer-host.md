# The host

Shale used to stop at the file. This is the half that owns the machine: it
connects to the printer on the LAN, reads what it says, uploads the plate, and —
on a press — starts it.

## What the printer speaks

Bambu's LAN interface is small and undocumented but stable. Shale talks to it
directly; nothing here is Bambu Studio code.

| | |
|---|---|
| Status | MQTT 3.1.1 over TLS, port **8883**, user `bblp`, password = the printer's 8-character LAN access code |
| Reports | subscribe `device/<serial>/report` — one JSON `print` object per state change, plus a heartbeat |
| Commands | publish `device/<serial>/request` |
| Upload | implicit FTPS, port **990**, same credentials, `PBSZ 0` + `PROT P`, passive data channel |
| Start | `{"print":{"command":"gcode_file","param":"/cache/<name>.gcode","bed_type":"textured_plate"}}` |
| Control | `pause`, `resume`, `stop` — same envelope |

The certificate is self-signed, so the client accepts any certificate: the
access code is the credential that authorises, and it never leaves the LAN.

## What the window shows

The Device pane is live. It polls `host_status` once a second while it is open;
the link itself runs on its own thread and reconnects without being asked.

- state (`IDLE`, `PREPARE`, `RUNNING`, `PAUSE`, `FINISH`, `FAILED`), percentage,
  layer `n/m`, remaining minutes, nozzle and bed actual/target
- the file the machine says it is printing
- **send & print** — uploads the plate that was just sliced, then sends the
  start command
- **pause**, **resume**, **stop**

## Where the safety line is now

The old rule in this repository was `will_move_printer: false` and "do not move
the printer". This change reverses it deliberately, and the reversal is written
here rather than buried in a diff:

- The host **never** starts a print by itself. Every actuation is a press in the
  window, and the send button asks first, naming the layer count, the predicted
  time and the plate it is about to use.
- There is no scheduler, no queue, no "print all plates", no auto-retry.
- Upload and status read are harmless: they write a file and read reports.
- Credentials live in this profile's webview storage (or `SHALE_PRINTER_HOST`,
  `SHALE_PRINTER_SERIAL`, `SHALE_PRINTER_CODE` when launched from a shell).
  They are never written to the repository, a log line, or a test.

## How it was proved without the printer

The real machine was not on the network, so the protocol was proved against a
localhost server that speaks it — TLS, MQTT, FTPS, reports — and transcribes
everything it is told. It is a test harness, kept outside this repository.
`cargo run -p shale-host --example prove_fake` does the whole round trip:

```
1. connected=true state=IDLE nozzle=26.1 bed=25.2 reports=1
2. uploaded 65 bytes as /cache/host-proof.gcode
3. command {"print":{"bed_type":"textured_plate","command":"gcode_file",
                     "param":"/cache/host-proof.gcode","sequence_id":"1","use_ams":false}}
4. state=RUNNING percent=6 layer=2/100 file=host-proof.gcode nozzle=220/220
5. after pause: state=PAUSE percent=7
```

and the fake's transcript shows the bytes arriving intact (`sha256 5f4341af…`,
first line `; Shale host proof`), the command parsed and accepted, and the pause
acknowledged.


## Two bugs this found, both real

- **FTP ordering.** Opening the data connection before `STOR` deadlocks against
  a server that accepts only after the command, which is how real FTP servers
  behave. The client now asks first, reads `150`, then connects.
- **TLS shutdown.** Closing the data connection straight after the last byte
  resets it while the server's final records are still unread, and the upload is
  lost. The client now sends `close_notify` and drains the stream first. A real
  printer would have eaten every upload this way.

## Printer messages

`print.hms` is a list of `{attr, code}` pairs. Shale shows each one as the four
groups Bambu prints (`0300-0100-0001-0007`) and links
`https://wiki.bambulab.com/en/x1/troubleshooting/hmscode/0300_0100_0001_0007`,
the same URL Bambu Studio builds.

The sentence comes from Bambu's table. On startup Shale asks
`https://e.bambulab.com/query.php?lang=en` — Studio's own call — and stores the
answer at `hms/hms_en.json` in the app config directory. That folder is not the
printer-profile cache. A later start sends the cached version and keeps the
file when the server says it is current. If the request fails, the sentences
already cached are used, and a code the table does not name falls back to a
short British-English line that names the module and the code.

The table itself is not in this repository. Bambu Studio is AGPL-3.0, and the
JSON ships inside that program, but the live endpoint does not publish a
separate licence for the message text. Copying the file in would be a guess
about that licence, so Shale fetches it instead.

## Files on the printer

The same FTPS session lists the card, downloads a timelapse or a sliced file,
deletes after a confirmation, and starts a file that is already there. A
`.gcode` uses `gcode_file`. A `.gcode.3mf` is read first: the plate comes from
`slice_info.config` and the `Metadata/plate_N.gcode` entries, lowest index
first, so a file that only contains plate 2 starts plate 2. Reprint does not
upload.

A Bambu send uploads a `.gcode.3mf` in the layout Bambu Studio writes:
`Metadata/plate_1.gcode` and its uppercase `.md5`, `slice_info.config` (filament
identity and the tray map), plate thumbnails, `model_settings.config`, and
`project_settings.config`. The thumbnails are an isometric view of the
meshes on that plate. An empty plate is a picture of the bed. The same `ams_mapping` array is on the `project_file`
command. The dialog's timelapse, bed levelling, flow dynamics, vibration
compensation, and use-AMS switches follow Studio's defaults for the printer.
A P1 has no flow calibration. A P2, H2, or X2 sends automatic calibration
(`auto_bed_leveling` / `auto_flow_cali` of 2) when those switches are on.
A printer that is not a Bambu machine still receives plain G-code.

## Known gaps

- **One plate per upload.** Each send is its own archive, with that plate as
  `Metadata/plate_1.gcode`. A 3MF already on the printer can still name another
  plate, and reprint uses the one in the file.
- **No camera.** The chamber stream is a separate protocol (`bambu:///local/…`
  on X1-class, JPEG on 6000 for the A1/P1) and is not implemented.
- **No firmware or account calls.** Those go through Bambu's cloud, not the LAN.
- **One printer at a time.** A second connect closes the first.
