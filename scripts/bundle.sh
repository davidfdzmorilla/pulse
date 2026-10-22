#!/bin/bash
# Builds Pulse.app in ~/Library/Caches/Pulse/bundle (PULSE_BUNDLE_DIR): the release
# binary, its icon and Info.plist (with the version, see below),
# signed with the "Pulse Local Signing" certificate if the keychain has it (else
# ad hoc). Not inside the repository: a folder synced with iCloud gets Finder and
# sync attributes on the bundle, and codesign refuses to sign it then.
# Usage: scripts/bundle.sh
set -euo pipefail
# With pipefail, never `cmd | grep -q`: grep stops at the first match, cmd dies
# of SIGPIPE and the pipe fails although it matched. `grep … >/dev/null` reads it all.
root=$(cd "$(dirname "$0")/.." && pwd)
# The version: PULSE_VERSION (set by scripts/release.sh), else git's (the last
# release and how far past it, as the app names itself: see build.rs).
described=$(git -C "$root" describe --tags --match 'v[0-9]*' 2>/dev/null || true)
version=${PULSE_VERSION:-${described#v}}
version=${version:-$(grep -m1 '^version' "$root/crates/app/Cargo.toml" | cut -d'"' -f2)}
export PULSE_VERSION=$version
identity=${PULSE_SIGN_IDENTITY:-Pulse Local Signing}

# The dependencies Cargo.lock names, not newer ones; the published build, with
# its debugging symbols apart (Cargo.toml's dist profile). The paths of this
# Mac (the repository, Cargo's registry) are written as relative ones, so the
# binary doesn't carry them and builds the same in any folder.
remap=$(printf '%s\x1f%s' "--remap-path-prefix=$root=." "--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=~/.cargo")
export CARGO_ENCODED_RUSTFLAGS="$remap"
cargo build --profile dist --locked --manifest-path "$root/Cargo.toml" -p app
# Where Cargo built it (CARGO_TARGET_DIR or the config may move it from target/).
target=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
built="$target/dist/pulse"
# The one just built, with this version, not an older one left there.
if [ "$("$built" --version)" != "$version" ]; then
    echo "$built no es Pulse $version: dice «$("$built" --version)»" >&2
    exit 1
fi
out=${PULSE_BUNDLE_DIR:-$HOME/Library/Caches/Pulse/bundle}
mkdir -p "$out"
app="$out/Pulse.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$built" "$app/Contents/MacOS/pulse"
# The symbols beside the app, not in it (scripts/release.sh publishes them).
rm -rf "$out/pulse.dSYM"
cp -RL "$built.dSYM" "$out/pulse.dSYM"  # (a link into target/dist/deps)
if [ "$(dwarfdump --uuid "$app/Contents/MacOS/pulse" | cut -d' ' -f2)" != "$(dwarfdump --uuid "$out/pulse.dSYM" | cut -d' ' -f2)" ]; then
    echo "Los símbolos no son de este pulse" >&2
    exit 1
fi
# Without its debugging symbols (they are in the dSYM): smaller, and none of
# the paths the object files had. The UUID stays, so the dSYM still fits.
strip -S "$app/Contents/MacOS/pulse"
if strings "$app/Contents/MacOS/pulse" | grep -F "$HOME/" >/dev/null; then
    echo "El binario aún lleva rutas de $HOME:" >&2
    strings "$app/Contents/MacOS/pulse" | grep -F "$HOME/" | head -5 >&2
    exit 1
fi
cp "$root/packaging/Pulse.icns" "$app/Contents/Resources/Pulse.icns"
cp "$root/LICENSE" "$app/Contents/Resources/LICENSE.txt"
cp "$root/packaging/THIRD_PARTY_NOTICES.txt" "$app/Contents/Resources/THIRD_PARTY_NOTICES.txt"
sed "s/@VERSION@/$version/g" "$root/packaging/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
xattr -cr "$app"

if security find-identity -p codesigning | grep "\"$identity\"" >/dev/null; then
    codesign --force --sign "$identity" "$app"
    echo "Pulse ${version} firmado con «${identity}»: $app"
else
    codesign --force --sign - "$app"
    echo "Pulse ${version} con firma ad hoc (no está el certificado «${identity}»): $app"
fi
