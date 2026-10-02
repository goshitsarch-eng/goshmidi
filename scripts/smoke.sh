#!/bin/sh
# Decode every installed example without starting a WebView.
set -eu
script_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
smoke_binary=${SMOKE_BIN:-$script_root/target/debug/dmidiplayer}
smoke_examples=${SMOKE_EXAMPLES:-$script_root/examples}
"$smoke_binary" --help >/dev/null
"$smoke_binary" --version
smoke_count=0
for file in "$smoke_examples"/*.mid "$smoke_examples"/*.kar; do
    test -f "$file" || continue
    env -u DISPLAY -u WAYLAND_DISPLAY "$smoke_binary" --dry-run "$file"
    smoke_count=$((smoke_count+1))
done
test "$smoke_count" -gt 0
smoke_temp=$(mktemp -d)
trap 'rm -rf "$smoke_temp"' EXIT HUP INT TERM
printf 'MThd' > "$smoke_temp/corrupt.mid"
if "$smoke_binary" --dry-run "$smoke_temp/corrupt.mid"; then
    echo 'Corrupt input unexpectedly passed validation' >&2; exit 1
fi
if "$smoke_binary" --dry-run "$smoke_temp/missing.mid"; then
    echo 'Missing input unexpectedly passed validation' >&2; exit 1
fi
printf 'Decoded %s example files and rejected corrupt/missing inputs.\n' "$smoke_count"
