#!/usr/bin/env bash
# Builds Linux (x86_64, aarch64) and macOS (universal) binaries and packs an installable OpenDeck plugin zip.
#
# Needs: rustup targets, cargo-zigbuild and zig (pip install ziglang).
# The macOS build links IOKit, so SDKROOT must point at a macOS SDK when not building on a Mac.
# Usage: scripts/package.sh            -> dist/opendeck-akp03.plugin.zip
#        SKIP_BUILD=1 scripts/package.sh (reuse binaries already in target/)
set -euo pipefail
cd "$(dirname "$0")/.."

ID=st.lynx.plugins.opendeck-akp03.sdPlugin
NAME=opendeck-akp03
OUT=dist/$ID
rm -rf dist && mkdir -p "$OUT"

if [[ -z "${SKIP_BUILD:-}" ]]; then
	cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28
	cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.28
	cargo zigbuild --release --target universal2-apple-darwin
fi

cp -R assets manifest.json LICENSE 40-opendeck-akp03.rules "$OUT/"
cp target/x86_64-unknown-linux-gnu/release/$NAME "$OUT/$NAME-linux"
cp target/aarch64-unknown-linux-gnu/release/$NAME "$OUT/$NAME-linux-aarch64"
cp target/universal2-apple-darwin/release/$NAME "$OUT/$NAME-mac"
chmod +x "$OUT/$NAME"-*

(cd dist && zip -qr "$NAME.plugin.zip" "$ID")
echo "Built dist/$NAME.plugin.zip"
