# Chamber camera

The Device pane can show the printer's camera. Nothing in this path starts a print or jogs an axis. Pause is a separate control, and it stays off unless you opt in for that printer.

## Which socket

| Machines | Protocol | Port |
| --- | --- | --- |
| A1, A1 mini, P1P, P1S | JPEG over TLS. An 80-byte packet (`bblp` and the LAN access code), then a 16-byte header and a JPEG. | 6000 |
| P2S, X1, X1C, X1E, H2, H2D, H2S | RTSPS, `rtsps://<ip>:322/streaming/live/1`, user `bblp`, the same access code. The picture is H.264. | 322 |

The P2S does not speak the port-6000 JPEG protocol. Shale opens port 322 for it, completes the RTSP handshake, and can pull a JPEG out of RTP when the stream carries one. This build does not decode H.264, so a P2S that only sends H.264 shows that as an error instead of a blank picture presented as a still. LAN-only live view has to be on, or port 322 closes immediately.

An unknown printer name stays on the JPEG socket. A wrong guess fails as a camera error.

## What the pane shows

Start and stop, the frame rate, a snapshot, and fullscreen. A wrong access code, LAN-only live view switched off, and a camera switched off on the printer each have their own message. The access code is not written into that message.

**Try the cloud picture** asks the signed-in account once. Bambu's remote camera is a private relay. There is no documented JPEG URL, so the usual answer is that the cloud picture is not available. The LAN camera is the one that works.

## Failed-print watch

The watch samples a frame every few seconds and looks, on this computer, for spaghetti, a part that has come off the bed, or a blob. It is a small grid detector written for Shale, not a downloaded model. A finding has to clear the confidence threshold and then repeat for the number of frames you set (three, by default) before it notifies.

Pause is off. Opting in is per printer, remembered on this computer, and the pane sends the pause only after that opt-in and a finding that has held. The MCP tools do not pause and do not move a printer.

The weights published with Obico's server are AGPL-3.0 and about 193 MB. They are not in this build, and ONNX Runtime is not linked. The address and the checksum are in `THIRD-PARTY-NOTICES.md` so a later build can download them once and check them. The sample images the detector is checked against are synthetic pictures generated in the test, not photographs.

## Empty plate

**Save empty-plate picture** keeps a JPEG under the app's config directory (`camera-plates/`), not in the catalog cache. The experimental removal path compares a later frame with that picture. A small amount of noise still counts as the same plate. A part does not.
