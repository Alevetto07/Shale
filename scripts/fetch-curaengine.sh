#!/usr/bin/env bash
# Obtain CuraEngine 5.13.0 to match scripts/fetch-resources.sh (Cura 5.13.0).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/vendor/curaengine}"
TAG="5.13.0"
APPIMAGE="$ROOT/vendor/UltiMaker-Cura-${TAG}-linux-X64.AppImage"
URL="https://github.com/Ultimaker/Cura/releases/download/${TAG}/UltiMaker-Cura-${TAG}-linux-X64.AppImage"

mkdir -p "$(dirname "$APPIMAGE")"
if [[ ! -f "$APPIMAGE" ]]; then
  echo "Downloading UltiMaker Cura ${TAG} AppImage…"
  curl -fsSL -o "$APPIMAGE" "$URL"
  chmod +x "$APPIMAGE"
fi

EXTRACT="$ROOT/vendor/cura-appimage-root"
rm -rf "$EXTRACT"
mkdir -p "$EXTRACT"
cd "$EXTRACT"
"$APPIMAGE" --appimage-extract >/dev/null 2>&1

ENGINE="$(find "$EXTRACT/squashfs-root" -type f -name 'CuraEngine' 2>/dev/null | head -1)"
if [[ -z "$ENGINE" ]]; then
  echo "CuraEngine binary not found inside AppImage" >&2
  exit 1
fi

LIBDIR="$EXTRACT/squashfs-root"
mkdir -p "$DEST"
install -m 755 "$ENGINE" "$DEST/CuraEngine.bin"

# The AppImage ships a relative ELF interpreter (lib64/ld-linux-x86-64.so.2); run via the system loader.
cat > "$DEST/CuraEngine" <<WRAP
#!/bin/bash
set -euo pipefail
export LD_LIBRARY_PATH="${LIBDIR}:\${LD_LIBRARY_PATH:-}"
exec /lib64/ld-linux-x86-64.so.2 "${DEST}/CuraEngine.bin" "\$@"
WRAP
chmod 755 "$DEST/CuraEngine"

if command -v patchelf >/dev/null 2>&1; then
  patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 "$DEST/CuraEngine.bin" || true
fi

echo "CuraEngine installed at $DEST/CuraEngine"
echo "  export SHALE_CURAENGINE=$DEST/CuraEngine"
echo "  export SHALE_CURAENGINE_LIB_DIR=$LIBDIR"