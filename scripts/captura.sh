#!/bin/bash
# Screenshot of Pulse in a given state, for reviewing the interface.
#   scripts/captura.sh <out-dir> <name> "<scene>" [WxH] [project]
# Builds nothing: run `cargo build --release` first. The scene is PULSE_SCENE
# (see crates/app/src/scene.rs). Writes <out-dir>/<name>.png (and the raw .ppm).
# Pulse opens a window for a few seconds; it neither opens the last project nor
# saves settings or recent projects in this mode.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=$1; name=$2; scene=${3:-}; size=${4:-1440x900}; project=${5:-}
mkdir -p "$out"
rm -f "$out/$name.ppm" "$out/$name.png"
PULSE_SCREENSHOT="$out/$name.ppm" PULSE_SCENE="$scene" PULSE_SIZE="$size" \
    timeout 60 "$root/target/release/pulse" ${project:+"$project"} >/dev/null 2>"$out/$name.log" || true
python3 -I "$root/scripts/ppm.py" "$out/$name.ppm" "$out/$name.png"
echo "$out/$name.png"
