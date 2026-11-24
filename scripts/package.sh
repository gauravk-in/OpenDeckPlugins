#!/usr/bin/env bash
# Builds all binaries and packs an installable OpenDeck plugin zip.
#
# Needs: rustup targets, cargo-zigbuild and zig (pip install ziglang) for cross builds.
# Usage: scripts/package.sh            -> dist/opendeck-weblink.zip
#        SKIP_BUILD=1 scripts/package.sh (reuse binaries already in target/)
set -euo pipefail
cd "$(dirname "$0")/.."

UUID=io.github.gauravk-in.weblink
OUT=dist/$UUID.sdPlugin
rm -rf dist && mkdir -p "$OUT/bin"

if [[ -z "${SKIP_BUILD:-}" ]]; then
	cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.28
	cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.28
	cargo zigbuild --release --target universal2-apple-darwin
fi

cp -R assets/. "$OUT/"
cp target/universal2-apple-darwin/release/opendeck-weblink "$OUT/bin/opendeck-weblink-mac"
cp target/x86_64-unknown-linux-gnu/release/opendeck-weblink "$OUT/bin/opendeck-weblink-linux-x86_64"
cp target/aarch64-unknown-linux-gnu/release/opendeck-weblink "$OUT/bin/opendeck-weblink-linux-aarch64"
chmod +x "$OUT"/bin/*
cp LICENSE "$OUT/"

(cd dist && zip -qr "opendeck-weblink.zip" "$UUID.sdPlugin")
echo "Built dist/opendeck-weblink.zip"
