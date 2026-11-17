#!/usr/bin/env bash
# End-to-end planar vs `--nonplanar auto` slices; writes JSON + markdown under artifacts/nonplanar/.
set -euo pipefail
cd "$(dirname "$0")/.."

export SHALE_CURA_DIR="${SHALE_CURA_DIR:-$PWD/vendor/Cura}"
export SHALE_MATERIALS_DIR="${SHALE_MATERIALS_DIR:-$PWD/vendor/fdm_materials}"
export SHALE_ORCA_DIR="${SHALE_ORCA_DIR:-$PWD/vendor/BambuStudio/resources/profiles}"
export SHALE_CURAENGINE="${SHALE_CURAENGINE:-$PWD/vendor/curaengine/CuraEngine}"
export SHALE_BENCH_SUMMARY=1

ROOT="$PWD"
ARTIFACTS="$ROOT/artifacts/nonplanar"
mkdir -p "$ARTIFACTS"

if [[ ! -f "$ROOT/examples/dome.stl" ]] || [[ ! -f "$ROOT/examples/chevron-slab.stl" ]]; then
  SHALE_EXPORT_BENCH_STLS=1 cargo test -p shale-core export_bench_stls -- --exact --nocapture
fi

CLI=(cargo run -q -p shale-cli --)

SLICE_ARGS=(
  --printer bambu_lab_a1
  --variant "0.4 mm"
  --quality 0_20mm_standard_bbl_a1
  --material bambu_lab_pla_basic
  --max-angle 10
)

parse_seconds() {
  local block="$1"
  echo "$block" | awk '/bench_summary/{sub(/^.*bench_summary /,""); print}' | tail -1 | jq '.print_time_seconds // 0'
}

slice_model() {
  local mode="$1"
  local path="$2"
  local gcode="$3"
  local extra=()
  if [[ "$mode" == "auto" ]]; then
    export SHALE_AUTO_PLAN_JSON=1
    extra=(--nonplanar auto)
  else
    unset SHALE_AUTO_PLAN_JSON
    extra=(--nonplanar off)
  fi
  "${CLI[@]}" slice "$path" "${SLICE_ARGS[@]}" "${extra[@]}" -o "$gcode" 2>&1
}

metrics_from_auto_json() {
  local json="${1:-{}}"
  jq -c '
    def oh:
      [.surfaces[]? | select(.enabled) |
        select(.kind | test("bridge|cantilever|overhang"; "i")) |
        {
          kind: (.kind | gsub("_tilt_band"; "") | gsub("_"; " ")),
          before: .overhang_angle_before_deg,
          after: .overhang_angle_after_deg
        }
      ];
    {
      features: (.detection.features | length),
      surfaces_enabled: (.enabled_surface_count // 0),
      max_tilt_deg: ([.surfaces[]? | select(.enabled) | .tilt_deg] | if length>0 then max else 0 end),
      fallback_planar: (.fallback_to_planar // false),
      underside_bands: oh,
      underside_summary: (
        oh | map(
          if .before != null and .after != null then
            "\(.kind) \(.before | . * 10 | round / 10)°→\(.after | . * 10 | round / 10)°"
          else empty end
        ) | join("; ")
      ),
      notes: (.notes // [])
    }
  ' <<<"$json"
}

emit_row() {
  local name="$1"
  local path="$2"
  local tag="$3"
  local planar_out auto_out
  local planar_g="/tmp/shale-bench-${tag}-planar.gcode"
  local auto_g="/tmp/shale-bench-${tag}-auto.gcode"

  echo "== $name (planar) ==" >&2
  planar_out=$(slice_model off "$path" "$planar_g")
  echo "== $name (auto) ==" >&2
  auto_out=$(slice_model auto "$path" "$auto_g")

  local ps as
  ps=$(parse_seconds "$planar_out")
  as=$(parse_seconds "$auto_out")

  local auto_json
  auto_json=$(echo "$auto_out" | awk '/^\{/{found=1} found' | jq -s 'add // {}')

  local meta underside
  meta=$(metrics_from_auto_json "$auto_json")
  underside=$(echo "$meta" | jq -r '.underside_summary')

  jq -n \
    --arg name "$name" \
    --arg path "$path" \
    --argjson planar_s "$ps" \
    --argjson auto_s "$as" \
    --argjson auto_plan "$auto_json" \
    --arg underside "$underside" \
    --argjson meta "$meta" \
    '{
      model: $name,
      stl: $path,
      features: $meta.features,
      surfaces_enabled: $meta.surfaces_enabled,
      max_tilt_deg: $meta.max_tilt_deg,
      underside_overhang: (if $underside == "" then null else $underside end),
      underside_bands: $meta.underside_bands,
      fallback_planar: $meta.fallback_planar,
      notes: $meta.notes,
      slice_time_planar_s: $planar_s,
      slice_time_auto_s: $auto_s,
      slice_time_delta_s: ($auto_s - $planar_s)
    }'
}

MODELS=(
  "dome|$ROOT/examples/dome.stl"
  "chevron|$ROOT/examples/chevron-slab.stl"
  "t-wing|$ROOT/examples/t-wing.stl"
  "bridge|$ROOT/examples/bridge.stl"
  "overhang|$ROOT/examples/overhang-shelf.stl"
)

ROWS=()
for entry in "${MODELS[@]}"; do
  IFS='|' read -r name path <<<"$entry"
  ROWS+=("$(emit_row "$name" "$path" "$name")")
done

REPORT="$ARTIFACTS/bench-e2e-report.json"
printf '%s\n' "${ROWS[@]}" | jq -s '.' >"$REPORT"

jq -r '
  ["| model | features | surfaces | max tilt ° | underside overhang (before→after) | slice planar | slice auto | Δ | helps? |",
   "|-------|----------|----------|------------|-----------------------------------|--------------|------------|---|--------|"],
  (.[] |
    . as $r |
    ($r.slice_time_planar_s | if . > 0 then "\(. | . * 10 | round / 10)s" else "—" end) as $pt |
    ($r.slice_time_auto_s | if . > 0 then "\(. | . * 10 | round / 10)s" else "—" end) as $at |
    ($r.slice_time_delta_s | if ($r.slice_time_planar_s > 0 and $r.slice_time_auto_s > 0) then
      (if . >= 0 then "+" else "" end) + "\(. | . * 10 | round / 10)s"
    else "—" end) as $dt |
    (if $r.fallback_planar or $r.surfaces_enabled == 0 then
      if ($r.notes | length) > 0 then "no — " + ($r.notes[0] // "planar only") else "no — no bands enabled" end
    elif ($r.underside_overhang != null) then
      "yes — underside " + $r.underside_overhang
    else
      "yes — top/slope bands (tilt " + ($r.max_tilt_deg | tostring) + "°)"
    end) as $help |
    "| \($r.model) | \($r.features) | \($r.surfaces_enabled) | \($r.max_tilt_deg) | \($r.underside_overhang // "—") | \($pt) | \($at) | \($dt) | \($help) |"
  )
' "$REPORT" | tee "$ARTIFACTS/bench-e2e-table.md"

echo "Wrote $REPORT and $ARTIFACTS/bench-e2e-table.md" >&2