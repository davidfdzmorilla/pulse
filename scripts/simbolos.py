#!/usr/bin/env python3
"""Reads a Pulse crash report from macOS (a .ips file: Consola → Informes de
fallos, or ~/Library/Logs/DiagnosticReports/pulse-*.ips) back to the code:
each frame of Pulse's own code with its function and file:line.

    python3 scripts/simbolos.py pulse-2026-10-21-101500.ips

It needs that version's debugging symbols (pulse.dSYM): the ones scripts/release.sh
keeps in ~/Library/Caches/Pulse/simbolos/<version>, or else they are downloaded
from the release (Pulse-<version>-simbolos.zip) and kept there. PULSE_SIMBOLOS
names another folder to look in (a pulse.dSYM, or folders of them).
"""
import json
import re
import os
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

REPOSITORY = "davidfdzmorilla/pulse"
CACHE = os.path.expanduser("~/Library/Caches/Pulse/simbolos")
# Where arm64 executables are loaded before the system slides them.
TEXT = 0x100000000


def read_report(path):
    with open(path, encoding="utf-8") as f:
        text = f.read()
    # A header line of JSON, then the report's JSON.
    header, _, body = text.partition("\n")
    return json.loads(header), json.loads(body)


def uuid_of(dsym):
    out = subprocess.run(["dwarfdump", "--uuid", dsym], capture_output=True, text=True).stdout
    return {line.split()[1].upper() for line in out.splitlines() if line.startswith("UUID:")}


def dsyms_in(folder):
    for root, dirs, _ in os.walk(folder):
        for d in dirs:
            if d.endswith(".dSYM"):
                yield os.path.join(root, d)


def find_dsym(uuid, version):
    places = [p for p in [os.environ.get("PULSE_SIMBOLOS"), CACHE] if p]
    for place in places:
        candidates = [place] if place.endswith(".dSYM") else list(dsyms_in(place))
        for dsym in candidates:
            if uuid in uuid_of(dsym):
                return dsym
    if not version:
        return None
    # Not here: that version's, from its release.
    url = f"https://github.com/{REPOSITORY}/releases/download/v{version}/Pulse-{version}-simbolos.zip"
    folder = os.path.join(CACHE, version)
    os.makedirs(folder, exist_ok=True)
    print(f"Descargando los símbolos de Pulse {version}…", file=sys.stderr)
    try:
        with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as tmp:
            with urllib.request.urlopen(url) as response:
                tmp.write(response.read())
        with zipfile.ZipFile(tmp.name) as z:
            z.extractall(folder)
        os.unlink(tmp.name)
    except Exception as e:  # noqa: BLE001 - said, and the frames go unread
        print(f"No se pudieron descargar ({e}): {url}", file=sys.stderr)
        return None
    for dsym in dsyms_in(folder):
        if uuid in uuid_of(dsym):
            return dsym
    return None


def readable(symbol):
    """A Rust symbol's names in order ("pulse::midi_map::Glide::retain…"), from
    its mangled form ("_RINvMs_NtCs…5pulse8midi_map5Glide…"): each name is its
    length and itself; crate hashes (Cs…_) are skipped. Not a full demangler,
    but enough to see what it is."""
    if not symbol.startswith("_R"):
        return symbol
    names, i = [], 2
    while i < len(symbol):
        # A crate's disambiguator (Cs…_), a back reference (B…_) or another
        # disambiguator (s…_): up to its "_".
        back = re.match(r"(Cs|B|s)[0-9A-Za-z]*_", symbol[i:])
        if back and (symbol[i] != "s" or i > 0 and not symbol[i - 1].isdigit()):
            i += back.end()
            continue
        if symbol[i].isdigit():
            j = i
            while j < len(symbol) and symbol[j].isdigit():
                j += 1
            n = int(symbol[i:j])
            name = symbol[j:j + n]
            if n and len(name) == n and name[0].isalpha() and all(c.isalnum() or c == "_" for c in name):
                names.append(name)
                i = j + n
                continue
            i = j
            continue
        i += 1
    # The same name twice in a row says nothing more.
    shown = [n for k, n in enumerate(names) if k == 0 or n != names[k - 1]]
    return "::".join(shown) or symbol


def pretty(line):
    """atos's line with the symbol made readable: "name (in pulse-…) (file:line)"."""
    symbol, _, rest = line.partition(" (in ")
    place = rest.rpartition(") (")[2].rstrip(")") if ") (" in rest else ""
    return f"{readable(symbol)}  {place}".rstrip()


def dwarf(dsym):
    folder = os.path.join(dsym, "Contents", "Resources", "DWARF")
    return os.path.join(folder, os.listdir(folder)[0])


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)
    header, report = read_report(sys.argv[1])
    version = header.get("app_version") or report.get("bundleInfo", {}).get("CFBundleShortVersionString")
    images = report.get("usedImages", [])
    ours = [i for i, image in enumerate(images) if image.get("name") == "pulse"]
    if not ours:
        sys.exit("El informe no tiene código de Pulse")
    index = ours[0]
    uuid = images[index]["uuid"].upper()
    dsym = find_dsym(uuid, version)
    if not dsym:
        sys.exit(f"No están los símbolos de este pulse (Pulse {version}, UUID {uuid})")
    exception = report.get("exception", {})
    print(f"Pulse {version} · {header.get('os_version', '')} · {exception.get('type', '')} {exception.get('signal', '')}".strip())
    threads = report.get("threads", [])
    # The thread that failed first.
    threads = sorted(enumerate(threads), key=lambda t: not t[1].get("triggered"))
    for n, thread in threads:
        frames = thread.get("frames", [])
        addresses = [hex(TEXT + f["imageOffset"]) for f in frames if f.get("imageIndex") == index]
        if not addresses:
            continue
        # -i: each address's functions inlined into each other, innermost
        # first, a blank line after each address.
        out = subprocess.run(["atos", "-i", "-o", dwarf(dsym), "-arch", "arm64", "-l", hex(TEXT), *addresses], capture_output=True, text=True).stdout
        chains = [block.splitlines() for block in out.strip("\n").split("\n\n")]
        title = thread.get("name") or thread.get("queue") or f"hilo {n}"
        print(f"\n{'► ' if thread.get('triggered') else ''}{title}")
        chains = iter(chains)
        for frame in frames:
            if frame.get("imageIndex") == index:
                # The function the frame is in first, then what was inlined into it.
                for k, line in enumerate(reversed(next(chains, ["?"]))):
                    print(f"  {'  ↳ ' if k else ''}{pretty(line)}")
            else:
                print(f"  ({images[frame.get('imageIndex', 0)].get('name', '?')})")


if __name__ == "__main__":
    main()
