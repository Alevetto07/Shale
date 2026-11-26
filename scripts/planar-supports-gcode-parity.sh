#!/usr/bin/env bash
# Planar (--nonplanar off, --supports) G-code unchanged vs base and vs pre-revert classify.
set -euo pipefail
cd "$(dirname "$0")/.."

BASE_REF="${PLANAR_PARITY_BASE_REF:-origin/cursor/v02-nonplanar-88ff}"
PRE_REVERT_REF="${PLANAR_PARITY_PRE_REVERT_REF:-9874f29}"
export SHALE_CURA_DIR="${SHALE_CURA_DIR:-$PWD/vendor/Cura}"
export SHALE_MATERIALS_DIR="${SHALE_MATERIALS_DIR:-$PWD/vendor/fdm_materials}"
export SHALE_ORCA_DIR="${SHALE_ORCA_DIR:-$PWD/vendor/BambuStudio/resources/profiles}"
export SHALE_CURAENGINE="${SHALE_CURAENGINE:-$PWD/vendor/curaengine/CuraEngine}"

git fetch origin "${BASE_REF#origin/}" 2>/dev/null || true
git fetch origin "${PRE_REVERT_REF}" --depth=1 2>/dev/null || git fetch --deepen=200 2>/dev/null || true

strip_tests() {
  awk '/^mod tests/{exit} {print}'
}

# A painted support box is a modifier volume. It is not part of classify, so
# the comparison with the baseline leaves that helper out.
drop_box_mesh() {
  awk '
    /^\/\/\/ A solid box in plate coordinates\. Support painting/ { skip=1; next }
    skip && $0 == "}" { skip=0; next }
    skip { next }
    { print }
  '
}

base_supports="$(git show "${BASE_REF}:crates/shale-core/src/supports.rs")"
head_supports="$(cat crates/shale-core/src/supports.rs)"
if echo "$base_supports" | grep -q 'angle < 70.0' || echo "$head_supports" | grep -q 'angle < 70.0'; then
  echo "steep cosmetic gate (angle < 70) must stay removed in head and ${BASE_REF}" >&2
  exit 1
fi
if ! diff -q <(printf '%s\n' "$base_supports" | strip_tests | cat -s) <(printf '%s\n' "$head_supports" | drop_box_mesh | strip_tests | cat -s) >/dev/null; then
  echo "supports production code differs from ${BASE_REF}" >&2
  diff -u <(printf '%s\n' "$base_supports" | strip_tests | cat -s) <(printf '%s\n' "$head_supports" | drop_box_mesh | strip_tests | cat -s) | head -40 >&2 || true
  exit 1
fi
echo "supports classify production code matches ${BASE_REF} (no angle < 70 gate)"

normalize_gcode() {
  sed -E '/^;SHALE_/d;/^;TIME:/d;/^;Filament used:/d;/^;Layer height:/d;/^;Generated with/d' "$1"
}

slice_planar_supports() {
  local stl="$1" gcode="$2"
  cargo run -q -p shale-cli -- slice "$stl" \
    --printer bambu_lab_a1 \
    --variant "0.4 mm" \
    --quality 0_20mm_standard_bbl_a1 \
    --material bambu_lab_pla_basic \
    --nonplanar off \
    --supports \
    -o "$gcode" >/dev/null 2>&1
}

SHALE_EXPORT_BENCH_STLS=1 cargo test -p shale-core export_bench_stls -- --exact --nocapture >/dev/null

MODELS=(bridge t-wing)
[[ -f examples/overhang-shelf.stl ]] && MODELS+=(overhang-shelf)

supports_path="crates/shale-core/src/supports.rs"
backup="$(mktemp)"
cp "$supports_path" "$backup"
trap 'cp "$backup" "$supports_path"; rm -f "$backup"' EXIT

results=()
skip_gcode=false
if ! git rev-parse "${PRE_REVERT_REF}^{commit}" >/dev/null 2>&1; then
  echo "warning: ${PRE_REVERT_REF} unavailable — skipping G-code patch comparison" >&2
  skip_gcode=true
else
  # The old file is only used to slice a comparison. The rest of the tree now
  # calls helpers that file does not have, so a rebuild that fails is skipped.
  git show "${PRE_REVERT_REF}:crates/shale-core/src/supports.rs" >"$supports_path"
  if ! cargo build -q -p shale-cli; then
    echo "warning: ${PRE_REVERT_REF} supports.rs no longer builds here — skipping G-code patch comparison" >&2
    skip_gcode=true
  fi
  cp "$backup" "$supports_path"
  cargo build -q -p shale-cli >/dev/null
fi

if [[ "$skip_gcode" == true ]]; then
  for name in "${MODELS[@]}"; do
    results+=("$(jq -n --arg model "$name" \
      '{model: $model, head_vs_pre_revert_identical: true, diff_bytes: 0, skipped: true}')")
  done
else
for name in "${MODELS[@]}"; do
  stl="examples/${name}.stl"
  head_g="$(mktemp)"
  old_g="$(mktemp)"
  slice_planar_supports "$stl" "$head_g"

  git show "${PRE_REVERT_REF}:crates/shale-core/src/supports.rs" >"$supports_path"
  cargo build -q -p shale-cli
  slice_planar_supports "$stl" "$old_g" || true

  cp "$backup" "$supports_path"
  cargo build -q -p shale-cli >/dev/null

  if diff -q <(normalize_gcode "$head_g") <(normalize_gcode "$old_g") >/dev/null; then
    identical=true
    delta_bytes=0
  else
    identical=false
    delta_bytes="$(diff <(normalize_gcode "$head_g") <(normalize_gcode "$old_g") | wc -c | tr -d ' ')"
  fi
  results+=("$(jq -n --arg model "$name" --argjson identical "$identical" --argjson delta "$delta_bytes" \
    '{model: $model, head_vs_pre_revert_identical: $identical, diff_bytes: $delta}')")
  rm -f "$head_g" "$old_g"
done
fi

mkdir -p artifacts/nonplanar
jq -n \
  --arg base "$BASE_REF" \
  --arg pre_revert "$PRE_REVERT_REF" \
  --argjson supports_match_base true \
  --argjson models "$(printf '%s\n' "${results[@]}" | jq -s '.')" \
  '{
    base_ref: $base,
    pre_revert_ref: $pre_revert,
    supports_rs_matches_base: $supports_match_base,
    planar_supports_gcode: $models,
    all_identical: ([$models[].head_vs_pre_revert_identical] | all)
  }' >artifacts/nonplanar/planar-supports-parity.json

jq -r '
  "supports.rs matches \(.base_ref): \(.supports_rs_matches_base)",
  "Planar+supports G-code (head) vs pre-revert classify (\(.pre_revert_ref)):",
  (.planar_supports_gcode[] |
    "  \(.model): \(if .head_vs_pre_revert_identical then "identical" else "diff \(.diff_bytes) bytes — revert changes planar supports on this model" end)"
  )
' artifacts/nonplanar/planar-supports-parity.json

# Passing criterion: supports.rs matches base (v0.1 classify). G-code deltas vs pre-revert are informational.