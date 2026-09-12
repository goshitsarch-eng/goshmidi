#!/bin/sh
# Tiered QA gate (packaging.md §2.4, PLAN §0):
#   sh scripts/verify.sh --fast   inner loop: cargo + retirement + pins + identity
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

# CMake/C++ retirement gate (P9/D14, T15): the C++ build is gone and must
# stay gone. Mirrors `tests/desktop_integration.rs::cmake_build_is_retired`
# so both `cargo test` and this gate catch a resurrection.
step "cmake retirement"
for retired in CMakeLists.txt cmake docs/CMakeLists.txt examples/CMakeLists.txt \
    icons/CMakeLists.txt tests/check_identity.cmake tests/test_sequence.cpp \
    data/dmidiplayer.gresource.xml src/main.cpp src/midi src/ui/window.cpp \
    src/ui/window.hpp src/app/settings.cpp src/app/settings.hpp \
    src/app/playlist.cpp src/app/playlist.hpp src/app/instruments.hpp \
    .github/workflows/linux-build.yml; do
    [ ! -e "$retired" ] || fail "retired C++/CMake path resurrected: $retired"
done
echo "retirement ok: C++ build gone (legacy gresource prefix removed with it)"

# Count pins (PLAN §1/U1): 23 actions, 23 menu items, 30 settings keys,
# 6 toast sites — T15 Rust successors of the retired C++ greps. Each pin is
# a declaration-shape grep backed by an exact unit/integration test (named):
step "count pins (Rust parity anchors)"
check_pin() { # $1=file $2=ERE $3=description
    grep -qE "$2" "$1" || fail "pin $3 missing ($1 /$2/)"
    echo "pin ok: $3"
}
# Actions + menu are one inventory in Rust (MenuAction::ALL covers both the
# 20 bindActions + 3 repeat actions and the 23 g_menu_append entries):
# unit `menu::inventory_is_23_entries_in_cpp_section_order` + flows
# `keys_flows::menu_maps_all_23_entries_to_documented_messages`.
check_pin src/ui/menu.rs "pub const ALL: \[MenuAction; 23\]" "23 actions/menu entries"
# 30 settings keys: unit `settings::...KNOWN_KEYS.len() == 30` (settings.rs).
check_pin src/config/settings.rs "const KNOWN_KEYS: \[&str; 30\]" "30 settings keys"
# 9 accelerators (PLAN §1 input contract): unit `keys::...ACCELERATORS`.
check_pin src/state/keys.rs "pub const ACCELERATORS: \[Accelerator; 9\]" "9 accelerators"
# 6 toast trigger sites have no naive count in Rust (errors route through
# one Toast funnel + per-path arms, not 6 window.cpp toast() calls); the
# parity lives in tests, and the gate pins the funnel they all share:
# `open/d1_error_leaves_playlist_untouched`, `playlist_flows` error halves,
# `lyrics_flows` saved/printed routing, `dialogs_flows` D3/chooser routing,
# `keys_flows` opener routing, + the `app.rs` error→toast unit tests.
check_pin src/app.rs "Message::Toast\(text\) => self\.toast\(text\)" "toast funnel"

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
# NOTE (T15): the grandfathered legacy prefix
# ("/net/sourceforge/dmidiplayer" in data/dmidiplayer.gresource.xml) is
# resolved by removal — the file is gone (retirement gate above), so no
# grandfather clause remains.
for icon in icons/dmidiplayer_24x24.png icons/dmidiplayer_32x32.png \
            icons/dmidiplayer_48x48.png icons/dmidiplayer_64x64.png \
            icons/dmidiplayer_128x128.png icons/dmidiplayer.svgz; do
    [ -f "$icon" ] || fail "missing install icon source: $icon"
done
echo "identity ok: canonical ID present, legacy absent, 6 icon sources present"

# Desktop integration contract (T14c, packaging.md §1.5/P8 + §1.6):
# the ONLY desktop change vs the C++-shipped file is DBusActivatable=true;
# everything else stays byte-identical for compat (P3).
step "desktop integration contract"
grep -q '^DBusActivatable=true$' com.goshapps.GoshMIDI.desktop \
    || fail "desktop missing DBusActivatable=true (P8)"
grep -q '^Exec=dmidiplayer %F$' com.goshapps.GoshMIDI.desktop \
    || fail "desktop Exec drifted (must stay 'dmidiplayer %F', P3)"
grep -q '^Icon=com.goshapps.GoshMIDI$' com.goshapps.GoshMIDI.desktop \
    || fail "desktop Icon drifted"
grep -q '^StartupWMClass=com.goshapps.GoshMIDI$' com.goshapps.GoshMIDI.desktop \
    || fail "desktop StartupWMClass drifted"
grep -q '^MimeType=audio/midi;audio/x-midi;audio/cakewalk;application/x-riff;$' \
    com.goshapps.GoshMIDI.desktop \
    || fail "desktop MimeType drifted"
# Metainfo wording (P11 minus <release>, which is T15's): toolkit + rhythm.
grep -q 'libcosmic/Rust' com.goshapps.GoshMIDI.metainfo.xml \
    || fail "metainfo toolkit wording not updated (P11)"
grep -q 'rhythm lamps' com.goshapps.GoshMIDI.metainfo.xml \
    || fail "metainfo rhythm-claim fix missing (U8/P11)"
if grep -q 'Rhythm, and Lyrics' com.goshapps.GoshMIDI.metainfo.xml \
    || grep -q 'GTK4/libadwaita' com.goshapps.GoshMIDI.metainfo.xml; then
    fail "metainfo still carries the pre-T14c description wording"
fi
# Metainfo <release> (P11, T15's): the shipped crate version must have an
# entry — a version bump without a release note fails here.
VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml)"
[ -n "$VERSION" ] || fail "cannot read version from Cargo.toml"
grep -q "<release version=\"$VERSION\"" com.goshapps.GoshMIDI.metainfo.xml \
    || fail "metainfo lacks a <release> entry for shipped version $VERSION"
echo "desktop ok: DBusActivatable + compat fields + metainfo wording + release $VERSION"

# Symbolic-icon fallback inventory (T14c, packaging.md §1.6): every
# *-symbolic name referenced in src/ must have a vendored hicolor fallback
# (icons/fallback/SOURCES.md), and the manifest must install the set.
step "icon fallback inventory"
[ -f icons/fallback/SOURCES.md ] || fail "icons/fallback/SOURCES.md missing"
grep -q 'icons/fallback/actions/' flatpak/com.goshapps.GoshMIDI.yml \
    || fail "manifest does not install icons/fallback/actions/"
grep -q 'icons/fallback/status/' flatpak/com.goshapps.GoshMIDI.yml \
    || fail "manifest does not install icons/fallback/status/"
missing_icons=""
for name in $(grep -rho '"[a-z0-9-]*-symbolic"' src/ --include='*.rs' \
    | tr -d '"' | sort -u); do
    if [ ! -f "icons/fallback/actions/$name.svg" ] \
        && [ ! -f "icons/fallback/status/$name.svg" ]; then
        missing_icons="${missing_icons:+$missing_icons }$name"
    fi
done
[ -z "$missing_icons" ] \
    || fail "no hicolor fallback for: $missing_icons"
echo "icons ok: every src-referenced symbolic name has a fallback"

# Bundled SoundFont manifest pin (T15, P7): the manifest's `soundfont`
# module must install the exact file the engine searches first
# (output_fluid.rs BUNDLED_SOUNDFONT_NAME) into /app/share/soundfonts/
# from a sha256-pinned source. (In-sandbox presence is asserted by
# smoke.sh; the code↔manifest bond is also pinned by
# `desktop_integration::bundled_soundfont_manifest_matches_code`.)
step "bundled soundfont manifest pin"
SF_NAME="$(sed -n 's/^pub const BUNDLED_SOUNDFONT_NAME: &str = "\(.*\)";$/\1/p' \
    src/core/output_fluid.rs)"
[ -n "$SF_NAME" ] || fail "cannot read BUNDLED_SOUNDFONT_NAME from output_fluid.rs"
grep -qF "$SF_NAME" flatpak/com.goshapps.GoshMIDI.yml \
    || fail "manifest does not install the bundled SoundFont $SF_NAME"
grep -q '/app/share/soundfonts/' flatpak/com.goshapps.GoshMIDI.yml \
    || fail "manifest does not install into /app/share/soundfonts/"
grep -q 'sha256: cfcd66d89e8386823400eca64934b14fbea7bf48ba1f00d21189af1262794ec2' \
    flatpak/com.goshapps.GoshMIDI.yml \
    || fail "manifest soundfont sha256 drifted (re-verify the pinned file)"
grep -q 'sha256: c21ed6be2fea862673953977c616befc28d1b20dfa60349e4461b97c759db6ca' \
    flatpak/com.goshapps.GoshMIDI.yml \
    || fail "manifest soundfont license sha256 drifted (re-verify the pinned file)"
echo "soundfont ok: manifest installs $SF_NAME (font + license sha256-pinned)"

# Freedesktop validators when installed (strict; CI images without them
# rely on the committed desktop_integration test + T15 matrix instead).
if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate com.goshapps.GoshMIDI.desktop \
        || fail "desktop-file-validate failed"
    echo "desktop-file-validate ok"
else
    echo "WARNING: desktop-file-validate absent — skipped (install desktop-file-utils)"
fi
if command -v appstreamcli >/dev/null 2>&1; then
    appstreamcli validate com.goshapps.GoshMIDI.metainfo.xml \
        || fail "appstreamcli validate failed"
    echo "appstreamcli validate ok"
else
    echo "WARNING: appstreamcli absent — skipped (install appstream)"
fi

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
