#!/usr/bin/env sh
# Packages the windowless half of Shale for a machine with no desktop.
#
# On Debian aarch64 the binaries are built natively (see docs/INSTALL-CLI.md);
# cross-compiling from Windows is possible but needs a linker for the target,
# so the honest path is to build where it runs or use `cross`.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/shale-cli/Cargo.toml | head -n 1)
target=${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
case "$target" in
    x86_64-unknown-linux-gnu) bundle=linux-x64 ;;
    aarch64-unknown-linux-gnu) bundle=linux-arm64 ;;
    *) bundle="$target" ;;
esac
stage="dist/shale-$version-$bundle"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
    cargo build --release --locked -p shale-cli -p shale-mcp --target "$target"
fi

rm -rf "$stage"
mkdir -p "$stage"
cp "target/$target/release/shale-cli" "$stage/shale"
cp "target/$target/release/shale-mcp" "$stage/"
cp docs/INSTALL-CLI.md THIRD-PARTY-NOTICES.md LICENSING.md "$stage/"

tar -C dist -czf "dist/shale-$version-$bundle.tar.gz" "shale-$version-$bundle"
echo "packaged dist/shale-$version-$bundle.tar.gz"
