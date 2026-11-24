#!/usr/bin/env bash
# Builds all binaries and packs an installable OpenDeck plugin zip.
#
# Needs: rustup targets, cargo-zigbuild and zig (pip install ziglang) for cross builds, and a macOS SDK
# in SDKROOT (the mac binary links AppKit for media keys), e.g. MacOSX13.3.sdk from
# https://github.com/joseluisq/macosx-sdks. On a Mac, SDKROOT is found with xcrun.
# Usage: scripts/package.sh            -> dist/opendeck-mediaknob.zip
#        SKIP_BUILD=1 scripts/package.sh (reuse binaries already in target/)
set -euo pipefail
cd "$(dirname "$0")/.."

UUID=io.github.gauravk-in.mediaknob
NAME=opendeck-mediaknob
OUT=dist/$UUID.sdPlugin
rm -rf dist && mkdir -p "$OUT/bin"

if [[ -z "${SKIP_BUILD:-}" ]]; then
	if [[ -z "${SDKROOT:-}" ]]; then
		SDKROOT=$(xcrun --sdk macosx --show-sdk-path 2>/dev/null) || { echo "Set SDKROOT to a macOS SDK" >&2; exit 1; }
		export SDKROOT
	fi
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
