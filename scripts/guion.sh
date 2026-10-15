#!/bin/bash
# Runs Pulse with a test script (see crates/app/src/script.rs) and turns its
# screenshots into PNG.
#   scripts/guion.sh <guion.txt> [WxH] [project]
# Builds nothing: run `cargo build --release` first. Writes to <guion>.out/:
# log.txt (each step, status messages, errors), the screenshots, widget lists and
# project dumps the script asks for. Settings and recent projects are left alone.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
script=$1; size=${2:-1440x900}; project=${3:-}
out=${script%.txt}.out
rm -rf "$out"; mkdir -p "$out"
PULSE_SIZE=$size PULSE_SCRIPT=$script PULSE_SCRIPT_OUT=$out \
    timeout "${TIMEOUT:-300}" "$root/target/release/pulse" ${project:+"$project"} >"$out/stdout.txt" 2>"$out/stderr.txt"
echo "salida: $?"
for f in "$out"/*.ppm; do
    [ -e "$f" ] || continue
    python3 -I "$root/scripts/ppm.py" "$f" "${f%.ppm}.png" && rm "$f"
done
grep -E "ERROR|estado|sin respuesta|no llegó|fin del guion" "$out/log.txt"
