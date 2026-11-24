#!/usr/bin/env bash
# Export printable STLs and preview images. Needs OpenSCAD 2021.01+ (xvfb-run on headless Linux).
set -euo pipefail
cd "$(dirname "$0")"
EXTRA=("$@")   # e.g. -D s6_w=118.2
mkdir -p stl images
SCAD=${OPENSCAD:-openscad}
run() { if [ -z "${DISPLAY:-}" ] && command -v xvfb-run >/dev/null; then xvfb-run -a "$SCAD" "$@"; else "$SCAD" "$@"; fi; }
for p in body lid dock dock_pad; do
  echo "STL: $p"; "$SCAD" -q -o "stl/$p.stl" -D "part=\"$p\"" ${EXTRA[@]+"${EXTRA[@]}"} homedeck.scad
done
img() { run -q -o "images/$1.png" -D "part=\"$2\"" --imgsize=1600,1200 --colorscheme=Tomorrow --viewall --autocenter --camera="$3" "${@:4}" ${EXTRA[@]+"${EXTRA[@]}"} homedeck.scad; }
img assembly assembly 0,0,0,62,0,28,0
img layout layout 0,0,0,0,0,0,0 --projection=o
img body body 0,0,0,235,0,30,0
img lid lid 0,0,0,55,0,25,0
img dock dock 0,0,0,55,0,210,0
img section section 0,0,0,90,0,90,0 --projection=o
echo done
