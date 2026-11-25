#!/usr/bin/env bash
# Builds all binaries and packs an installable OpenDeck plugin zip.
#
# Needs: rustup targets, cargo-zigbuild and zig (pip install ziglang) for cross builds.
# Usage: scripts/package.sh            -> dist/opendeck-homeassistant.zip
#        SKIP_BUILD=1 scripts/package.sh (reuse binaries already in target/)
set -euo pipefail
cd "$(dirname "$0")/.."

UUID=io.github.gauravk-in.homeassistant
NAME=opendeck-homeassistant
OUT=dist/$UUID.sdPlugin
rm -rf dist && mkdir -p "$OUT/bin"

if [[ -z "${SKIP_BUILD:-}" ]]; then
	cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28
	cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.28
	cargo zigbuild --release --target universal2-apple-darwin
fi

cp -R assets/. "$OUT/"
cp target/universal2-apple-darwin/release/$NAME "$OUT/bin/$NAME-mac"
cp target/x86_64-unknown-linux-gnu/release/$NAME "$OUT/bin/$NAME-linux-x86_64"
cp target/aarch64-unknown-linux-gnu/release/$NAME "$OUT/bin/$NAME-linux-aarch64"
chmod +x "$OUT"/bin/*
cp LICENSE "$OUT/"

(cd dist && zip -qr "$NAME.zip" "$UUID.sdPlugin")
echo "Built dist/$NAME.zip"
