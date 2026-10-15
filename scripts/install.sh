#!/bin/bash
# Installs target/bundle/Pulse.app (built by scripts/bundle.sh) in ~/Applications,
# keeping the version there as "Pulse (anterior).app". Only needed the first time:
# afterwards Pulse updates itself from GitHub.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
app="$root/target/bundle/Pulse.app"
dest="$HOME/Applications"
[ -d "$app" ] || { echo "Primero: scripts/bundle.sh" >&2; exit 1; }
mkdir -p "$dest"
if [ -d "$dest/Pulse.app" ]; then
    rm -rf "$dest/Pulse (anterior).app"
    mv "$dest/Pulse.app" "$dest/Pulse (anterior).app"
fi
ditto "$app" "$dest/Pulse.app"
echo "Instalado en $dest/Pulse.app"
