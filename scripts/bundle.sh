#!/bin/bash
# Builds Pulse.app in ~/Library/Caches/Pulse/bundle (PULSE_BUNDLE_DIR): the release
# binary, its icon and Info.plist (with the version, see below),
# signed with the "Pulse Local Signing" certificate if the keychain has it (else
# ad hoc). Not inside the repository: a folder synced with iCloud gets Finder and
# sync attributes on the bundle, and codesign refuses to sign it then.
# Usage: scripts/bundle.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
# The version: PULSE_VERSION (set by scripts/release.sh), else Cargo's.
version=${PULSE_VERSION:-$(grep -m1 '^version' "$root/crates/app/Cargo.toml" | cut -d'"' -f2)}
export PULSE_VERSION=$version
identity=${PULSE_SIGN_IDENTITY:-Pulse Local Signing}

cargo build --release --manifest-path "$root/Cargo.toml" -p app
out=${PULSE_BUNDLE_DIR:-$HOME/Library/Caches/Pulse/bundle}
mkdir -p "$out"
app="$out/Pulse.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/release/pulse" "$app/Contents/MacOS/pulse"
cp "$root/packaging/Pulse.icns" "$app/Contents/Resources/Pulse.icns"
cp "$root/LICENSE" "$app/Contents/Resources/LICENSE.txt"
cp "$root/packaging/THIRD_PARTY_NOTICES.txt" "$app/Contents/Resources/THIRD_PARTY_NOTICES.txt"
sed "s/@VERSION@/$version/g" "$root/packaging/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
xattr -cr "$app"

if security find-identity -p codesigning | grep -q "\"$identity\""; then
    codesign --force --sign "$identity" "$app"
    echo "Pulse ${version} firmado con «${identity}»: $app"
else
    codesign --force --sign - "$app"
    echo "Pulse ${version} con firma ad hoc (no está el certificado «${identity}»): $app"
fi
