# Chamber camera

The Device pane can show the printer's camera. Nothing in this path starts a print or jogs an axis. Pause is a separate control, and it stays off unless you opt in for that printer.

## Which socket

| Machines | Protocol | Port |
| --- | --- | --- |
| A1, A1 mini, P1P, P1S | JPEG over TLS. An 80-byte packet (`bblp` and the LAN access code), then a 16-byte header and a JPEG. | 6000 |
| P2S, X1, X1C, X1E, H2, H2D, H2S | RTSPS, `rtsps://<ip>:322/streaming/live/1`, user `bblp`, the same access code. The picture is H.264. | 322 |

The P2S does not speak the port-6000 JPEG protocol. Shale opens port 322, completes the RTSP handshake, and decodes the H.264 access units with Cisco's official OpenH264 binary. The first time it is needed, Shale downloads that binary for this computer, checks its SHA-256 against the sums the OpenH264 bindings already trust, and stores it in the app data directory. `get_camera_snapshot` does the same download when the library is missing, so the tool does not depend on the app having installed it first. `camera_status` uses a library that is already there and does not start a download. It is not part of the installer. Each decoded picture is stored as a JPEG, so the overlay, the failure watch, a snapshot, and the plate check use the same path as an A1 frame. A packet that is not a picture yet is skipped. LAN-only live view has to be on, or port 322 closes immediately.

The camera panel credits the codec with the sentence Cisco's binary licence requires: "OpenH264 Video Codec provided by Cisco Systems, Inc." The `openh264-source` build feature compiles OpenH264 from source instead. Leave it off in a release build. Cisco's patent promise does not cover that build. See THIRD-PARTY-NOTICES.md.

An unknown printer name stays on the JPEG socket. A wrong guess fails as a camera error.

## What the pane shows

Start and stop, the frame rate, a snapshot, and fullscreen. A wrong access code, LAN-only live view switched off, and a camera switched off on the printer each have their own message. The access code is not written into that message.

**Try the cloud picture** asks the signed-in account once. Bambu's remote camera is a private relay. There is no documented JPEG URL, so the usual answer is that the cloud picture is not available. The LAN camera is the one that works.

## Failed-print watch

The watch samples a frame every few seconds. The default is a small grid detector written for Shale: spaghetti, a part that has come off the bed, or a blob. A finding has to clear the confidence threshold and then repeat for the number of frames you set (three, by default) before it notifies.

The detector control can switch to the Obico failure model. That file is about 193 MB, AGPL-3.0, and it is not in the repository. **Download the Obico failure model** fetches it into the app data directory, shows how many bytes have arrived and how many are expected, and keeps it only when the SHA-256 matches. The OpenH264 download shows the same byte counts. Until that finishes, and whenever you press **Use the built-in detector**, the grid detector is what runs. The model’s only class is `failure`. A frame it marks is treated as spaghetti for the notice and the optional pause.

Pause is off. Opting in is per printer, remembered on this computer, and the pane sends the pause only after that opt-in and a finding that has held. The last few frames before a finding that holds are written under the app data directory (`failure-frames/`), capped at eight warnings and six frames each, and older ones are deleted. The queue shows that history again after a restart. `SHALE_CACHE_DIR` overrides the directory, which is how tests stay off the real app data folder. The frames are not written into the queued file. Changing sensitivity shows a generated test frame and what the detector would say about it. The MCP tools do not pause and do not move a printer. They do not download the Obico model. `get_camera_snapshot` does download Cisco's library when it is missing.

The address and the checksum are in `THIRD-PARTY-NOTICES.md`. Tests use a loopback file and never request the release. The sample images the grid detector is checked against are synthetic pictures generated in the test, not photographs.

## Timelapse

**Record this print** is off until you turn it on for the job in front of you. Choose a folder first. While it is on, Shale keeps one frame each time the printer reports a new layer, and otherwise one frame on the interval you set. **Save the timelapse** writes a JPEG sequence and an animated preview into that folder. When Cisco's OpenH264 library is already installed, it also writes an MP4. The save does not download the library, and it does not move the printer. At most 180 frames are kept. The catalogue cache is not a valid folder.

## Empty plate

**Save empty-plate picture** keeps a JPEG under the app's config directory (`camera-plates/`), not in the catalog cache. The experimental removal path compares a later frame with that picture. A small amount of noise still counts as the same plate. A part does not.
