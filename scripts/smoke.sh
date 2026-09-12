#!/bin/sh
# Headless smoke test for dmidiplayer (Rust/libcosmic port).
#
# Runs --help / --dry-run / --dump-info, asserts exit 0 plus stdout markers,
# and fails on any panic on stderr. Works with no display (proves it by
# running the binary under `env -u DISPLAY -u WAYLAND_DISPLAY`) and inside
# `flatpak run` / `flatpak-builder --run` (prefers /app install paths).
#
# Env overrides: SMOKE_BIN (binary path), SMOKE_EXAMPLES (examples dir).
set -eu

fail() { echo "smoke FAIL: $*" >&2; exit 1; }
pass() { echo "smoke ok: $*"; }

case "$0" in
    */*) ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)" ;;
    *) ROOT="$(pwd)" ;;
esac

# --- binary resolution: sandbox install first, then local targets, then PATH.
BIN="${SMOKE_BIN:-}"
if [ -z "$BIN" ]; then
    for c in /app/bin/dmidiplayer \
             "$ROOT/target/release/dmidiplayer" \
             "$ROOT/target/debug/dmidiplayer"; do
        if [ -x "$c" ]; then BIN="$c"; break; fi
    done
    if [ -z "$BIN" ] && command -v dmidiplayer >/dev/null 2>&1; then
        BIN="dmidiplayer"
    fi
fi
[ -n "$BIN" ] || fail "no dmidiplayer binary found (SMOKE_BIN unset, no /app or target build)"

# --- examples resolution: sandbox install (P6) first, then repo examples.
EXDIR="${SMOKE_EXAMPLES:-}"
if [ -z "$EXDIR" ]; then
    if [ -f /app/share/dmidiplayer/examples.lst ]; then
        EXDIR="/app/share/dmidiplayer"
    else
        EXDIR="$ROOT/examples"
    fi
fi
[ -d "$EXDIR" ] || fail "examples dir not found: $EXDIR"

# P6 in-sandbox assertion: installed examples.lst exists and every entry
# resolves beside it.
if [ "$EXDIR" = "/app/share/dmidiplayer" ]; then
    [ -f "$EXDIR/examples.lst" ] || fail "missing /app/share/dmidiplayer/examples.lst"
    while IFS= read -r entry || [ -n "$entry" ]; do
        [ -n "$entry" ] || continue
        [ -e "$EXDIR/$entry" ] || fail "examples.lst entry missing from install: $entry"
    done < "$EXDIR/examples.lst"
    pass "P6 examples install ($(wc -l < "$EXDIR/examples.lst" | tr -d ' ') entries resolve)"
fi

# T14c in-sandbox assertions: installed desktop/metainfo/icons identity.
# Only meaningful inside the sandbox (host runs test the repo files via
# verify.sh + the desktop_integration test instead).
if [ -d /app/share/applications ]; then
    DESK=/app/share/applications/com.goshapps.GoshMIDI.desktop
    META=/app/share/metainfo/com.goshapps.GoshMIDI.metainfo.xml
    [ -f "$DESK" ] || fail "installed desktop file missing: $DESK"
    [ -f "$META" ] || fail "installed metainfo missing: $META"
    grep -q '^DBusActivatable=true$' "$DESK" \
        || fail "installed desktop lacks DBusActivatable=true"
    grep -q 'rhythm lamps' "$META" \
        || fail "installed metainfo lacks the T14c rhythm-claim fix"
    for s in 24x24 32x32 48x48 64x64 128x128; do
        [ -f "/app/share/icons/hicolor/$s/apps/com.goshapps.GoshMIDI.png" ] \
            || fail "installed app icon missing: hicolor/$s/apps"
    done
    [ -f /app/share/icons/hicolor/scalable/apps/com.goshapps.GoshMIDI.svgz ] \
        || fail "installed app icon missing: hicolor/scalable/apps"
    # Fallback inventory: exact when the /repo bind-mount is visible
    # (`flatpak build --bind-mount`, the verify.sh full-gate path), else a
    # count + spot-name sanity check (plain `flatpak run` has no /repo).
    if [ -d /repo/src ]; then
        missing=""
        for name in $(grep -rho '"[a-z0-9-]*-symbolic"' /repo/src --include='*.rs' \
            | tr -d '"' | sort -u); do
            if [ ! -f "/app/share/icons/hicolor/scalable/actions/$name.svg" ] \
                && [ ! -f "/app/share/icons/hicolor/scalable/status/$name.svg" ]; then
                missing="${missing:+$missing }$name"
            fi
        done
        [ -z "$missing" ] || fail "installed fallback missing for: $missing"
    else
        count="$(find /app/share/icons/hicolor/scalable/actions \
            /app/share/icons/hicolor/scalable/status -name '*-symbolic.svg' \
            2>/dev/null | wc -l | tr -d ' ')"
        [ "$count" -ge 29 ] || fail "only $count fallback icons installed (want >= 29)"
        for spot in media-playback-start-symbolic channel-secure-symbolic \
            open-menu-symbolic window-close-symbolic pan-down-symbolic; do
            find /app/share/icons/hicolor/scalable -name "$spot.svg" \
                | grep -q . || fail "installed fallback missing spot name: $spot"
        done
    fi
    pass "installed desktop/metainfo/app-icons/fallback inventory"
fi

# Collect .mid/.kar fixtures (sh has no nullglob; filter non-matches).
set --
for pat in "$EXDIR"/*.mid "$EXDIR"/*.kar; do
    [ -e "$pat" ] || continue
    set -- "$@" "$pat"
done
[ "$#" -gt 0 ] || fail "no .mid/.kar examples in $EXDIR"
pass "fixtures: $# files in $EXDIR"

OUT="$(mktemp)"; ERR="$(mktemp)"
trap 'rm -f "$OUT" "$ERR"' EXIT

# Always run headless: no DISPLAY/WAYLAND_DISPLAY may leak through.
run_bin() { env -u DISPLAY -u WAYLAND_DISPLAY "$BIN" "$@"; }

no_panic() { # $1 = label; scans $ERR
    if grep -qi 'panic' "$ERR"; then
        echo "--- stderr ($1) ---" >&2; cat "$ERR" >&2
        fail "panic on stderr during $1"
    fi
}

# 1. --help exits 0 with the usage marker.
run_bin --help >"$OUT" 2>"$ERR" || fail "--help exited $?"
grep -q 'Usage: dmidiplayer' "$OUT" || { cat "$OUT" >&2; fail "--help missing usage marker"; }
no_panic "--help"
pass "--help exit 0 + usage marker"

# 1b. T14c: headless boot must not depend on a COSMIC session — the same
# --help passes under GNOME/KDE/unset desktop markers (packaging.md §3.2
# inputs; full GUI matrix stays a T15 live run).
for de in GNOME KDE; do
    env -u DISPLAY -u WAYLAND_DISPLAY XDG_CURRENT_DESKTOP="$de" "$BIN" --help \
        >"$OUT" 2>"$ERR" || fail "--help under XDG_CURRENT_DESKTOP=$de exited $?"
    grep -q 'Usage: dmidiplayer' "$OUT" \
        || fail "--help under XDG_CURRENT_DESKTOP=$de missing usage marker"
    no_panic "--help ($de)"
done
env -u DISPLAY -u WAYLAND_DISPLAY -u XDG_CURRENT_DESKTOP "$BIN" --help \
    >"$OUT" 2>"$ERR" || fail "--help with no XDG_CURRENT_DESKTOP exited $?"
grep -q 'Usage: dmidiplayer' "$OUT" \
    || fail "--help with no XDG_CURRENT_DESKTOP missing usage marker"
no_panic "--help (no DE)"
pass "--help under GNOME/KDE/unset DE markers"

# 2. --dry-run over every example: exit 0, per-file + OK markers.
run_bin --dry-run "$@" >"$OUT" 2>"$ERR" || fail "--dry-run exited $?"
grep -q 'dry-run: OK (GUI skipped)' "$OUT" || { cat "$OUT" >&2; fail "--dry-run missing OK marker"; }
for f in "$@"; do
    base="$(basename "$f")"
    grep -qF "dry-run: file " "$OUT" && grep -qF "$base" "$OUT" \
        || fail "--dry-run missing file marker for $base"
done
no_panic "--dry-run"
pass "--dry-run exit 0 + markers for $# files"

# 3. --dump-info per example file: exit 0 + OK marker each.
for f in "$@"; do
    run_bin --dump-info "$f" >"$OUT" 2>"$ERR" || fail "--dump-info $f exited $?"
    grep -q 'dry-run: OK (GUI skipped)' "$OUT" || fail "--dump-info $f missing OK marker"
    no_panic "--dump-info $f"
done
pass "--dump-info exit 0 + markers for $# files"

# 4. Missing input is reported on stderr without panicking. T1 reports
# `File not found` and exits 0; T3 (real decode) tightens this to the
# `error: <file>: <reason>` + non-zero contract, so no exit code is pinned.
run_bin --dry-run "$EXDIR/no-such-file-12345.mid" >"$OUT" 2>"$ERR" || true
grep -q 'File not found' "$ERR" || { cat "$ERR" >&2; fail "missing file not reported on stderr"; }
no_panic "missing-file case"
pass "missing file reported, no panic"

echo "smoke PASS: $BIN"
