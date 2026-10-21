#!/bin/bash
# Publishes a new version of Pulse on GitHub, for the app's updater to find.
#   scripts/release.sh [patch|minor|major]   (patch by default: 0.1.3 → 0.1.4)
# Only from main, up to date with GitHub and with nothing uncommitted: work
# reaches main through pull requests. The version comes from the last tag
# (v1.2.3); nothing is committed: the new tag marks the version. Runs the tests,
# builds and signs Pulse.app (scripts/bundle.sh) and publishes it all at once:
# the release is made as a draft with Pulse-X.Y.Z.zip and the titles merged
# since the last version as its notes, the zip is downloaded back and checked
# (its SHA-256, its signature and who signed it, its version, the binary's),
# and only then is it published, which makes the tag. If anything fails
# before, or it is stopped (Ctrl-C), the draft goes and nothing is left: no
# tag without a release, no number skipped.
set -Eeuo pipefail
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
# GitHub's tags win over old local ones (after the history was rewritten).
git fetch --quiet --tags --force origin
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
latest=$(gh release view --json tagName --jq .tagName 2>/dev/null || true)
if [ -n "$latest" ] && [ "$previous_tag" != "$latest" ]; then
    echo "La última versión publicada es $latest, pero main viene de ${previous_tag:-ninguna}: ¿falta git pull?" >&2
    exit 1
fi
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
cargo test --release --locked --workspace --quiet
scripts/bundle.sh
out=${PULSE_BUNDLE_DIR:-$HOME/Library/Caches/Pulse/bundle}
# Signed, and by the certificate the installed copies trust (an ad hoc
# signature only if asked for: they would all refuse it).
checked() {
    codesign --verify --strict "$1"
    if [ "${PULSE_ALLOW_ADHOC:-}" != 1 ] && ! codesign -dvv "$1" 2>&1 | grep -q '^Authority=Pulse Local Signing$'; then
        echo "$1 no está firmado con «Pulse Local Signing»" >&2
        return 1
    fi
    [ "$("$1/Contents/MacOS/pulse" --version)" = "$version" ]
}
checked "$out/Pulse.app"
# Without the extended attributes macOS adds (they come out as ._ files
# anywhere else).
zip="$out/Pulse-$version.zip"
rm -f "$zip"
ditto -c -k --norsrc --noextattr --keepParent "$out/Pulse.app" "$zip"
# Its debugging symbols, published beside it and kept here, to read crash
# reports of this version back to the line (scripts/simbolos.py).
symbols="$out/Pulse-$version-simbolos.zip"
rm -f "$symbols"
ditto -c -k --norsrc --noextattr --keepParent "$out/pulse.dSYM" "$symbols"
kept="$HOME/Library/Caches/Pulse/simbolos/$version"
mkdir -p "$kept"
rm -rf "$kept/pulse.dSYM"
cp -R "$out/pulse.dSYM" "$kept/"

# A draft first: nothing public, no tag yet. From before it is made, anything
# that fails or stops the script takes it away again (also a create that left
# it half uploaded, or Ctrl-C).
tag="v$version"
check=$(mktemp -d)
discard() {
    trap - ERR INT TERM
    echo "No se publicó: se quita el borrador." >&2
    gh release delete "$tag" --yes --cleanup-tag >/dev/null 2>&1 || true
    rm -rf "$check"
}
trap discard ERR
trap 'discard; exit 130' INT TERM
gh release create "$tag" "$zip" "$symbols" --draft --target "$(git rev-parse HEAD)" --title "Pulse $version" --notes "$notes"
# What GitHub has is what was built: the same bytes, signed, the version.
gh release download "$tag" --pattern "Pulse-$version.zip" --dir "$check"
[ "$(shasum -a 256 "$check/Pulse-$version.zip" | cut -d' ' -f1)" = "$(shasum -a 256 "$zip" | cut -d' ' -f1)" ]
ditto -x -k "$check/Pulse-$version.zip" "$check"
checked "$check/Pulse.app"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$check/Pulse.app/Contents/Info.plist")" = "$version" ]
gh release edit "$tag" --draft=false --latest
trap - ERR INT TERM
rm -rf "$check"
git fetch --quiet --tags --force origin
echo "Publicada Pulse $version"
