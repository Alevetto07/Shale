#!/usr/bin/env bash
# Download the Cura definition/profile tree and the fdm_materials library
# that Shale reads at runtime. Nothing here is copied into git.
#
# Pins:
#   Cura 5.13.0 (stable) — resources are LGPL-3.0, see vendor/Cura/LICENSE
#   fdm_materials 82f433b — CC0, see vendor/fdm_materials/LICENSE
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/vendor}"
CURA_TAG="5.13.0"
CURA_COMMIT="1fb8a7610460e03ec62da950d1c173a249b23b59"
MATERIALS_COMMIT="82f433b2cfe0ac36c974443b80c9294f31b79224"

mkdir -p "$DEST"

if [[ ! -d "$DEST/Cura/.git" ]]; then
  git clone --filter=blob:none --sparse --branch "$CURA_TAG" --depth 1 \
    https://github.com/Ultimaker/Cura.git "$DEST/Cura"
fi

# Cone mode cannot list a single file such as LICENSE. Non-cone paths with a
# leading slash check out that file next to the resource directories.
git -C "$DEST/Cura" sparse-checkout set --no-cone \
  /LICENSE \
  /resources/definitions \
  /resources/extruders \
  /resources/quality \
  /resources/variants \
  /resources/intent

if [[ ! -f "$DEST/Cura/resources/definitions/fdmprinter.def.json" ]]; then
  echo "Cura definitions did not check out. Expected fdmprinter.def.json." >&2
  exit 1
fi
if [[ ! -f "$DEST/Cura/LICENSE" ]]; then
  echo "Cura LICENSE is missing. Refusing to continue without the LGPL text." >&2
  exit 1
fi

if [[ ! -d "$DEST/fdm_materials/.git" ]]; then
  git clone https://github.com/Ultimaker/fdm_materials.git "$DEST/fdm_materials"
fi
git -C "$DEST/fdm_materials" checkout "$MATERIALS_COMMIT"

if [[ ! -f "$DEST/fdm_materials/LICENSE" ]]; then
  echo "fdm_materials LICENSE is missing. Refusing to continue without the CC0 text." >&2
  exit 1
fi

cat > "$DEST/REVISIONS" <<EOF
Cura ${CURA_TAG} ${CURA_COMMIT}
fdm_materials ${MATERIALS_COMMIT}
EOF

echo "Resources ready in $DEST"
echo "  Cura ${CURA_TAG} (${CURA_COMMIT})"
echo "  fdm_materials ${MATERIALS_COMMIT}"
