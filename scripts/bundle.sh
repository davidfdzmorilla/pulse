#!/bin/bash
# Builds target/bundle/Pulse.app: the release binary, its icon and Info.plist (with
# the version from crates/app/Cargo.toml), signed with the "Pulse Local Signing"
# certificate if the keychain has it (else ad hoc).
# Usage: scripts/bundle.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
version=$(grep -m1 '^version' "$root/crates/app/Cargo.toml" | cut -d'"' -f2)
identity=${PULSE_SIGN_IDENTITY:-Pulse Local Signing}

cargo build --release --manifest-path "$root/Cargo.toml" -p app
app="$root/target/bundle/Pulse.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/target/release/pulse" "$app/Contents/MacOS/pulse"
cp "$root/packaging/Pulse.icns" "$app/Contents/Resources/Pulse.icns"
sed "s/@VERSION@/$version/g" "$root/packaging/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

if security find-identity -p codesigning | grep -q "\"$identity\""; then
    codesign --force --sign "$identity" "$app"
    echo "Pulse ${version} firmado con «${identity}»: $app"
else
    codesign --force --sign - "$app"
    echo "Pulse ${version} con firma ad hoc (no está el certificado «${identity}»): $app"
fi
