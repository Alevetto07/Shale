#!/usr/bin/env bash
# Slice repo test models with --nonplanar auto and emit a markdown table.
set -euo pipefail
cd "$(dirname "$0")/.."
export SHALE_CURA_DIR="${SHALE_CURA_DIR:-$PWD/vendor/Cura}"
export SHALE_MATERIALS_DIR="${SHALE_MATERIALS_DIR:-$PWD/vendor/fdm_materials}"
export SHALE_ORCA_DIR="${SHALE_ORCA_DIR:-$PWD/vendor/BambuStudio/resources/profiles}"
export SHALE_CURAENGINE="${SHALE_CURAENGINE:-$PWD/vendor/curaengine/CuraEngine}"
export SHALE_AUTO_PLAN_JSON=1

ROOT="$PWD"
CLI=(cargo run -q -p shale-cli --)

echo "| model | features | surfaces | max tilt ° | slice time |"
echo "|-------|----------|----------|------------|------------|"

run() {
  local name="$1"
  local path="$2"
  local gcode="/tmp/shale-auto-${name}.gcode"
  local out
  out=$("${CLI[@]}" slice "$path" \
    --printer bambu_lab_a1 \
    --variant "0.4 mm" \
    --quality 0_20mm_standard_bbl_a1 \
    --material bambu_lab_pla_basic \
    --nonplanar auto \
    --max-angle 10 \
    -o "$gcode" 2>&1)
  local time_s
  time_s=$(echo "$out" | awk '/^time /{printf "%s %s", $3, $4}')
  local json
  json=$(echo "$out" | awk '/^\{/{found=1} found' | jq -s 'add // {}' 2>/dev/null || echo "{}")
  local feats surfs slope
  feats=$(echo "$json" | jq -r '.detection.features | length // "?"')
  surfs=$(echo "$json" | jq -r '.enabled_surface_count // "?"')
  slope=$(echo "$json" | jq -r '[.surfaces[]? | select(.enabled) | .tilt_deg] | if length>0 then max else 0 end')
  echo "| $name | $feats | $surfs | $slope | $time_s |"
}

run bridge "$ROOT/examples/bridge.stl"
run overhang "$ROOT/examples/overhang-shelf.stl"
run t-wing "$ROOT/examples/t-wing.stl"
run cube "$ROOT/examples/cube.stl"