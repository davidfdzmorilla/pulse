#!/bin/bash
# Publishes a new version of Pulse on GitHub, for the app's updater to find.
#   scripts/release.sh [patch|minor|major]   (patch by default: 0.1.3 → 0.1.4)
# Only from main, up to date with GitHub and with nothing uncommitted: work
# reaches main through pull requests. The version comes from the last tag
# (v1.2.3); nothing is committed: the new tag marks the version. Runs the tests,
# builds and signs Pulse.app (scripts/bundle.sh), tags, and creates the GitHub
# release with Pulse-X.Y.Z.zip and the titles merged since the last version
# as its notes.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
part=${1:-patch}

if [ "$(git branch --show-current)" != main ]; then
    echo "Se publica desde main: git switch main && git pull" >&2
    exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
    echo "Hay cambios sin guardar en git." >&2
    exit 1
fi
git fetch --quiet --tags origin
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then
    echo "main no está al día con GitHub: git pull" >&2
    exit 1
fi
if ! security find-identity -p codesigning | grep -q '"Pulse Local Signing"' && [ "${PULSE_ALLOW_ADHOC:-}" != 1 ]; then
    echo "Falta el certificado «Pulse Local Signing»: la versión saldría con firma ad hoc." >&2
    echo "(PULSE_ALLOW_ADHOC=1 para publicarla igualmente.)" >&2
    exit 1
fi

previous_tag=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)
if [ -n "$previous_tag" ] && [ "$(git rev-list -n1 "$previous_tag")" = "$(git rev-parse HEAD)" ]; then
    echo "No hay cambios desde $previous_tag." >&2
    exit 1
fi
current=${previous_tag#v}
current=${current:-0.0.0}
IFS=. read -r major minor patch <<< "$current"
case "$part" in
    major) version="$((major + 1)).0.0" ;;
    minor) version="$major.$((minor + 1)).0" ;;
    patch) version="$major.$minor.$((patch + 1))" ;;
    *) echo "Uso: scripts/release.sh [patch|minor|major]" >&2; exit 1 ;;
esac
echo "Pulse $current → $version"

# One line per merged pull request; the old "Pulse X.Y.Z" commits and the PR
# numbers GitHub adds to titles are left out.
range=${previous_tag:+$previous_tag..}HEAD
notes=$(git log --no-merges --pretty='%s' "$range" | grep -v -E '^Pulse [0-9]+\.[0-9]+\.[0-9]+$' | sed -E 's/ \(#[0-9]+\)$//; s/^/- /' || true)
notes=${notes:-"- Mejoras internas"}

export PULSE_VERSION=$version
cargo test --release --workspace --quiet
scripts/bundle.sh
out=${PULSE_BUNDLE_DIR:-$HOME/Library/Caches/Pulse/bundle}
codesign --verify --strict "$out/Pulse.app"
zip="$out/Pulse-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent "$out/Pulse.app" "$zip"

git tag -a "v$version" -m "Pulse $version"
git push --quiet origin "v$version"
gh release create "v$version" "$zip" --title "Pulse $version" --notes "$notes"
echo "Publicada Pulse $version"
