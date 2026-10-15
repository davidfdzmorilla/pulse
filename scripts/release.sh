#!/bin/bash
# Publishes a new version of Pulse on GitHub, for the app's updater to find.
#   scripts/release.sh [patch|minor|major]   (patch by default: 0.1.0 → 0.1.1)
# Raises the version in crates/app/Cargo.toml, runs the tests, builds and signs
# Pulse.app (scripts/bundle.sh), commits "Pulse X.Y.Z", tags vX.Y.Z, pushes, and
# creates the GitHub release with Pulse-X.Y.Z.zip and the commits since the last
# version as notes.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
part=${1:-patch}

if [ -n "$(git status --porcelain)" ]; then
    echo "Hay cambios sin guardar en git: haz commit antes de publicar." >&2
    exit 1
fi
if ! security find-identity -p codesigning | grep -q '"Pulse Local Signing"' && [ "${PULSE_ALLOW_ADHOC:-}" != 1 ]; then
    echo "Falta el certificado «Pulse Local Signing»: la versión saldría con firma ad hoc." >&2
    echo "(PULSE_ALLOW_ADHOC=1 para publicarla igualmente.)" >&2
    exit 1
fi

current=$(grep -m1 '^version' crates/app/Cargo.toml | cut -d'"' -f2)
IFS=. read -r major minor patch <<< "$current"
case "$part" in
    major) version="$((major + 1)).0.0" ;;
    minor) version="$major.$((minor + 1)).0" ;;
    patch) version="$major.$minor.$((patch + 1))" ;;
    *) echo "Uso: scripts/release.sh [patch|minor|major]" >&2; exit 1 ;;
esac
echo "Pulse $current → $version"

previous_tag=$(git describe --tags --abbrev=0 2>/dev/null || true)
if [ -n "$previous_tag" ]; then
    notes=$(git log --no-merges --pretty='- %s' "$previous_tag"..HEAD)
else
    notes="Primera versión publicada."
fi

sed -i '' "1,/^version = /s/^version = \".*\"/version = \"$version\"/" crates/app/Cargo.toml
cargo test --release --workspace --quiet
scripts/bundle.sh
zip="target/bundle/Pulse-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent target/bundle/Pulse.app "$zip"

git add crates/app/Cargo.toml Cargo.lock
git commit --quiet -m "Pulse $version"
git tag "v$version"
git push --quiet origin HEAD "v$version"
gh release create "v$version" "$zip" --title "Pulse $version" --notes "$notes"
echo "Publicada Pulse $version"
