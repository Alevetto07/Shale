# Bambu toolhead envelopes

Each `bambu_*_head.json` file is a conservative swept envelope used for travel collision
checks. Fields marked in `needs_measurement` should be verified on a physical printer
before treating collisions as exact.

Set `calibrated` to `false` in a local copy only if you want Shale to skip collision
flattening entirely (not recommended for production prints).