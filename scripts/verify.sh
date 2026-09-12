#!/bin/sh
# Tiered QA gate (packaging.md §2.4, PLAN §0):
#   sh scripts/verify.sh --fast   inner loop: cargo + cmake + pins + identity
#   sh scripts/verify.sh          full pre-commit gate: fast + vendoring
#                                 freshness + flatpak-builder + in-sandbox smoke
# Exit non-zero on any failure. Run from anywhere; always operates on the repo root.
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$ROOT"

step() { printf '\n=== %s ===\n' "$*"; }
fail() { echo "verify FAIL: $*" >&2; exit 1; }

MODE="${1:-}"
FAST=0
if [ "$MODE" = "--fast" ]; then
    FAST=1
elif [ -n "$MODE" ]; then
    echo "usage: sh scripts/verify.sh [--fast]" >&2
    exit 2
fi

# --- fast tier ---

step "cargo build --locked"
cargo build --locked

step "cargo clippy --locked"
cargo clippy --locked -- -D warnings

step "cargo fmt --check"
# `cargo fmt`, never bare `rustfmt`: bare rustfmt parses edition-2015 and misfires on edition-2024 sources (T9 precedent).
cargo fmt --check

step "cargo test --locked"
cargo test --locked

# CMake stays green until T15 retires it (P9/D14). If the host clearly lacks
# the C++ deps, record a loud SKIP (CI always has full deps, so it runs
# strictly there); otherwise configure+build+test strictly in build/.
step "cmake configure + build + ctest"
MISSING=""
for mod in gtk4 libadwaita-1 alsa fluidsynth uchardet glib-2.0; do
    if ! pkg-config --exists "$mod" 2>/dev/null; then
        MISSING="${MISSING:+$MISSING }$mod"
    fi
done
if [ -n "$MISSING" ]; then
    echo "WARNING: cmake tier SKIPPED — host lacks pkg-config modules: $MISSING"
    echo "WARNING: install the *-dev packages or rely on CI for the CMake tier."
else
    cmake -B build -DBUILD_DOCS=OFF -DBUILD_TESTING=ON
    cmake --build build
    ctest --test-dir build --output-on-failure
fi

# Count pins (PLAN §1/U1): 23 actions, 23 menu items, 30 settings keys,
# 6 toast sites — greps against the CURRENT C++ tree until T15 retires it.
step "count pins (C++ parity anchors)"
check_count() { # $1=expected $2=file $3=ERE $4=description
    got="$(grep -cE "$3" "$2")"
    [ "$got" = "$1" ] || fail "pin $4: expected $1, got $got ($2 /$3/)"
    echo "pin ok: $4 = $got"
}
check_count 20 src/ui/window.cpp "^[[:space:]]*add\\(\"" "bindActions add() sites"
check_count 3 src/ui/window.cpp "^[[:space:]]*addRepeat\\(\"" "repeat actions"
check_count 23 src/ui/window.cpp "g_menu_append\\(" "menu entries"
check_count 30 src/app/settings.cpp "g_key_file_set_" "settings keys"
check_count 6 src/ui/window.cpp "^[[:space:]]*toast\\(" "toast sites"
echo "pin ok: total actions = 23 (20 bindActions + 3 repeat)"

# Identity contract (successor of check_identity.cmake; no CMake-dependent
# greps per P9 — checks shipped files only).
step "identity contract"
grep -q 'com.goshapps.GoshMIDI' com.goshapps.GoshMIDI.desktop \
    com.goshapps.GoshMIDI.metainfo.xml \
    || fail "canonical app ID missing from desktop/metainfo"
if grep -q 'net.sourceforge.dmidiplayer' com.goshapps.GoshMIDI.desktop \
    com.goshapps.GoshMIDI.metainfo.xml; then
    fail "legacy net.sourceforge.dmidiplayer ID present in desktop/metainfo"
fi
# NOTE: data/dmidiplayer.gresource.xml keeps the legacy
# "/net/sourceforge/dmidiplayer" prefix (line 3) while the C++ UI lives —
# grandfathered until T15 (P9), must not fail this check.
for icon in icons/dmidiplayer_24x24.png icons/dmidiplayer_32x32.png \
            icons/dmidiplayer_48x48.png icons/dmidiplayer_64x64.png \
            icons/dmidiplayer_128x128.png icons/dmidiplayer.svgz; do
    [ -f "$icon" ] || fail "missing install icon source: $icon"
done
echo "identity ok: canonical ID present, legacy absent, 6 icon sources present"

if [ "$FAST" = "1" ]; then
    echo
    echo "verify --fast PASS"
    exit 0
fi

# --- full tier ---

# Vendoring freshness: regenerate with the VENDORED pinned generator (P4c)
# and diff against the committed file; fail on drift. (Needs network, like
# the flatpak build below.)
step "cargo-sources freshness"
command -v flatpak-builder >/dev/null 2>&1 \
    || fail "flatpak-builder not installed (full gate needs it)"
python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o /tmp/cargo-sources.check.json
diff -u flatpak/cargo-sources.json /tmp/cargo-sources.check.json \
    || fail "flatpak/cargo-sources.json is stale — regenerate with the vendored generator"
rm -f /tmp/cargo-sources.check.json
echo "freshness ok"

# Pre-commit gate per user DoD (P1/D10): full flatpak build + in-sandbox smoke.
# --disable-rofiles-fuse: rofiles-fuse mounts fail (EPERM) inside
# containerized sessions; same flag as the sibling GamesHandler gate.
# Output is equivalent (plain-dir checkout instead of a fuse overlay).
step "flatpak-builder"
flatpak-builder --force-clean --user --disable-rofiles-fuse build-dir flatpak/com.goshapps.GoshMIDI.yml

step "in-sandbox smoke"
# `flatpak build`, not `flatpak-builder --run`: --run always spawns
# rofiles-fuse (no opt-out flag exists) and cannot resolve the
# host-relative script path; --bind-mount delivers smoke.sh into the same
# app sandbox (same pattern as the sibling GamesHandler gate).
flatpak build --bind-mount=/repo="$ROOT" build-dir sh /repo/scripts/smoke.sh

echo
echo "verify FULL PASS"
