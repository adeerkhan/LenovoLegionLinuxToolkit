#!/usr/bin/env sh
# Regenerate packaging/flatpak/cargo-sources.json from Cargo.lock.
#
# Flatpak builds run without network access, so every crate the build needs must
# be listed as a source. The generator is the one Flathub publishes; it wants
# `aiohttp` (`python3 -m pip install --user aiohttp`).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
tool="${FLATPAK_CARGO_GENERATOR:-$here/flatpak-cargo-generator.py}"

if [ ! -f "$tool" ]; then
    echo "fetching flatpak-cargo-generator.py ..."
    curl -sSL -o "$tool" \
        https://raw.githubusercontent.com/flathub/flatpak-builder-tools/master/cargo/flatpak_cargo_generator/flatpak-cargo-generator.py
    chmod +x "$tool"
fi

python3 "$tool" "$root/Cargo.lock" -o "$here/cargo-sources.json"
echo "wrote $here/cargo-sources.json"
