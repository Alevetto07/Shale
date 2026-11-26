# Experimental print removal (A1 only)

**Untested on real hardware.** This has not been run on a printer. The first time you try it, stay with the machine and be ready to switch it off. It is off unless you turn it on, and the Device pane only offers it for the Bambu Lab A1.

The A1 mini, the P2S, and every other machine do not show the control. The generator still takes a bed description, so a later machine can be tried in a test with `allow_other_machines`. The window never sets that flag.

## What it does

1. The print finishes.
2. The bed heater is switched off and the printer waits until the bed is at or below the temperature you set. The default is 28 °C, which is about where a PEI plate lets go of a part.
3. The nozzle parks above the back of the plate, drops to 1 mm above the surface behind the part, and the bed moves so the part is pushed off the front. The move is slow, and it is repeated across the part. The travel height follows the part height from the slice, and it stays between 10 mm and 40 mm.
4. A camera frame is compared with the empty-plate picture. If the plate matches, the next queued job may start. If it does not, the queue stops and the pane says so.

Two ways to send the motion, chosen in the queue:

* **Append the sweep to the file.** The routine is written onto the end of the copy that is queued. The original slice is left as it was.
* **Send the sweep after the bed cools.** The lines are published as `gcode_line` only once the queue is in the sweeping state, which it reaches only after the bed report is cool enough, and only if you turned this mode on before the job.

Neither path runs when the setting is Off. A finish then waits for you to say the plate is clear, as before.

## What it refuses

The part is refused, with a reason, when any of these are true:

* The machine is not an A1.
* The part is taller than 25 mm. The nozzle only pushes the base, and a taller part can tip into the toolhead.
* The part is under 2 mm tall, or under 15 mm in its shorter direction. There is nothing solid to push.
* The part is wider than 180 mm. A sweep that wide has nowhere safe to stand beside it.
* The part is not sitting on the plate, or it does not fit the printable area.
* Adhesion is `none` or empty. A part that was printed with no brim and no skirt is the one most likely to have come loose in a way the sweep cannot predict. A brim or a skirt is accepted.

## An example

A part from x 100–140 mm, y 80–130 mm, z 0–12 mm, with a brim, on the A1's 256 mm plate (origin at the front left), cooling to 28 °C:

```
; Shale experimental print removal for the A1
; Untested on real hardware. Stay with the printer the first time and be ready to switch it off.
; The nozzle parks above the back of the plate, drops just above the surface behind the part, and the bed moves so the part is pushed off the front.
M140 S0
M190 S28
G90
G1 Z16.0 F300
; pass 1 of 3
G1 X108.0 Y138.0 Z16.0 F3000
G1 Z1.0 F300
G1 Y8.0 F600
G1 Z16.0 F300
; pass 2 of 3
G1 X120.0 Y138.0 Z16.0 F3000
G1 Z1.0 F300
G1 Y8.0 F600
G1 Z16.0 F300
; pass 3 of 3
G1 X132.0 Y138.0 Z16.0 F3000
G1 Z1.0 F300
G1 Y8.0 F600
G1 Z16.0 F300
; park above the back of the plate
G1 X120.0 Y248.0 Z16.0 F3000
```

Feeds are in mm/min: 300 to lift, 3000 to travel, 600 to sweep. `M190` waits. That wait is inside the file when the sweep is appended, and it is the queue's own wait when the lines are sent after the print.

A profile whose origin is the centre of the bed uses that origin, so the lines match the slice. The A1's stock definition is front-left.

## What you should do on a real A1

Do this with the plate empty of anything you care about, the door area clear, and a hand near the power switch.

1. Leave removal off and print one small part the usual way. Confirm pause, resume, and stop still do what they say.
2. Photograph the empty plate with **Save empty-plate picture** while the camera is running and the plate really is empty.
3. Slice a part that is about the size of the example above, with a brim, and turn removal on only after reading the warning. Queue that one job. Do not queue a second job the first time.
4. Watch the bed cool. Watch the nozzle. If it comes down somewhere other than behind the part, switch the printer off.
5. Only after one sweep has finished in front of you, try a second queued job and see whether a clear plate starts it and a part left on the plate stops it.

Do not try this with a tall part, a part that fills the plate, or a material you know barely sticks. The generator refuses some of those, and it should. If it does not refuse one that looks dangerous, leave the setting off.
