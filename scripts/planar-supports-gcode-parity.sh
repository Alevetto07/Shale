#!/usr/bin/env bash
# Confirm planar (--nonplanar off) G-code with supports matches base after supports::classify revert.
set -euo pipefail
cd "$(dirname "$0")/.."

BASE_REF="${PLANAR_PARITY_BASE_REF:-origin/cursor/v02-nonplanar-88ff}"
export SHALE_CURA_DIR="${SHALE_CURA_DIR:-$PWD/vendor/Cura}"
export SHALE_MATERIALS_DIR="${SHALE_MATERIALS_DIR:-$PWD/vendor/fdm_materials}"
export SHALE_ORCA_DIR="${SHALE_ORCA_DIR:-$PWD/vendor/BambuStudio/resources/profiles}"
export SHALE_CURAENGINE="${SHALE_CURAENGINE:-$PWD/vendor/curaengine/CuraEngine}"

git fetch origin "${BASE_REF#origin/}" 2>/dev/null || true

normalize_gcode() {
  sed -E '/^;SHALE_/d;/^;TIME:/d;/^;Filament used:/d;/^;Layer height:/d;/^;Generated with/d' "$1"
}

slice_planar_supports() {
  local root="$1" stl="$2" gcode="$3"
  (
    cd "$root"
    cargo run -q -p shale-cli -- slice "$stl" \
      --printer bambu_lab_a1 \
      --variant "0.4 mm" \
      --quality 0_20mm_standard_bbl_a1 \
      --material bambu_lab_pla_basic \
      --nonplanar off \
      --supports \
      -o "$gcode" >/dev/null 2>&1
  )
}

MODELS=(overhang-shelf bridge t-wing)

ensure_stls() {
  local root="$1"
  (
    cd "$root"
    SHALE_EXPORT_BENCH_STLS=1 cargo test -p shale-core export_bench_stls -- --exact --nocapture >/dev/null
  )
}

WT="$(mktemp -d)"
cleanup() { git worktree remove -f "$WT" 2>/dev/null || rm -rf "$WT"; }
trap cleanup EXIT

git worktree add -f "$WT" "$BASE_REF" >/dev/null
ensure_stls "$PWD"
ensure_stls "$WT"

results=()
for name in "${MODELS[@]}"; do
  stl_rel="examples/${name}.stl"
  head_g="$(mktemp)"
  base_g="$(mktemp)"
  slice_planar_supports "$PWD" "$PWD/$stl_rel" "$head_g"
  slice_planar_supports "$WT" "$WT/$stl_rel" "$base_g"

  if diff -q <(normalize_gcode "$head_g") <(normalize_gcode "$base_g") >/dev/null; then
    identical=true
    delta_bytes=0
  else
    identical=false
    delta_bytes="$(diff <(normalize_gcode "$head_g") <(normalize_gcode "$base_g") | wc -c | tr -d ' ')"
  fi
  results+=("$(jq -n --arg model "$name" --argjson identical "$identical" --argjson delta "$delta_bytes" \
    '{model: $model, identical: $identical, diff_bytes: $delta}')")
  rm -f "$head_g" "$base_g"
done

mkdir -p artifacts/nonplanar
jq -n --arg base "$BASE_REF" --argjson models "$(printf '%s\n' "${results[@]}" | jq -s '.')" \
  '{base_ref: $base, planar_supports_gcode: $models, all_identical: ([$models[].identical] | all)}' \
  >artifacts/nonplanar/planar-supports-parity.json

jq -r '
  "Planar G-code parity vs \(.base_ref) (--nonplanar off, --supports):",
  (.planar_supports_gcode[] | "  \(.model): \(if .identical then "identical" else "DIFF \(.diff_bytes) bytes" end)"),
  "Overall: \(if .all_identical then "PASS — no planar support output change" else "FAIL" end)"
' artifacts/nonplanar/planar-supports-parity.json

if ! jq -e '.all_identical' artifacts/nonplanar/planar-supports-parity.json >/dev/null; then
  exit 1
fi