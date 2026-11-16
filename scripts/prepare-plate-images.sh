#!/usr/bin/env bash
# Regenerate bundled ASCII plate JPEGs under apps/desktop/src/field/.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIELD="$ROOT/apps/desktop/src/field"
TMP="${TMPDIR:-/tmp}/shale-plates"
mkdir -p "$TMP"

curl -fsSL -o "$TMP/moon.jpg" "https://upload.wikimedia.org/wikipedia/commons/d/dd/Full_Moon_Luc_Viatour.jpg"
curl -fsSL -o "$TMP/crab.jpg" "https://upload.wikimedia.org/wikipedia/commons/0/00/Crab_Nebula.jpg"
curl -fsSL -o "$TMP/saturn-src.jpg" "https://upload.wikimedia.org/wikipedia/commons/c/c7/Saturn_during_Equinox.jpg"

# Lunar: moon on near-black; paper tone comes from the canvas background only.
ffmpeg -y -hide_banner -loglevel error -i "$TMP/moon.jpg" -vf \
  "scale=640:480:force_original_aspect_ratio=decrease,pad=640:480:(ow-iw)/2:(oh-ih)/2:color=0x111318,crop=616:480:12:0" \
  -q:v 4 "$FIELD/lunar-base.jpg"
ffmpeg -y -hide_banner -loglevel error -i "$FIELD/lunar-base.jpg" -vf \
  "format=gray,geq=lum='lum(X,Y)*if(lt(X,18),0,if(gt(X,W*0.56),0,min(1,max(0,1-(X-0.34*W)/(0.22*W)))))'" \
  -pix_fmt rgb24 -q:v 4 "$FIELD/lunar.jpg"
rm -f "$FIELD/lunar-base.jpg"

ffmpeg -y -hide_banner -loglevel error -i "$TMP/crab.jpg" -vf \
  "scale=420:-1,pad=640:480:175:(oh-ih)/2:color=0x09060E,eq=gamma=1.28:contrast=1.08:saturation=1.2:brightness=-0.02" \
  -q:v 4 "$FIELD/nebula.jpg"

ffmpeg -y -hide_banner -loglevel error -i "$TMP/saturn-src.jpg" -vf \
  "scale=640:-1,crop=640:min(ih\,480),pad=640:480:(ow-iw)/2:(oh-ih)/2:color=0x0C0906,eq=contrast=1.14:gamma=1.05:saturation=1.08" \
  -q:v 4 "$FIELD/saturn.jpg"

echo "Wrote $FIELD/{lunar,nebula,saturn}.jpg"