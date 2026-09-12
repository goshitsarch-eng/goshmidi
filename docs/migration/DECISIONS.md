# Migration decisions

Format: question, options considered, choice, why. Priority order for
tie-breaks: feature parity > Flatpak sandbox correctness > accessibility >
simplicity/maintainability > COSMIC conventions.

## D1. Rewrite vs FFI bridge for `dmidi-core`

- Options: (a) clean-room Rust port of core algorithms 1:1; (b) `cxx`/FFI
  bridge over the C++ `dmidi-core` static library.
- Choice: (a) rewrite.
- Why: `player.cpp` is coupled to GLib (`g_idle_add`) per callback,
  `sequence.cpp` to GLib + uchardet, and the playback cursor aliases
  `MidiEvent*` into a `std::vector` — all unsafe/awkward across FFI. One
  language and one async model also closes the current benign data races.
  Parity is guarded by golden tests against the C++ implementation.

## D2. Flatpak runtime

- Options: (a) `org.freedesktop.Platform` 24.08; (b) COSMIC runtime.
- Choice: (a), re-evaluate if a COSMIC runtime ships on Flathub.
- Why: shippable today everywhere; QA must verify non-COSMIC desktops, which
  a COSMIC-only runtime would undermine.

## D3. Binary and config identity

- Options: (a) keep `dmidiplayer` command + `~/.config/dmidiplayer` +
  `~/.dmidiplayer` paths; (b) rename to goshmidi everywhere.
- Choice: (a).
- Why: README contract + existing user configs/song settings. Tested by the
  identity check (successor of `check_identity.cmake`).

## D4. Settings format

- Options: (a) GKeyFile-compatible INI preserved (unknown keys kept);
  (b) migrate to TOML/JSON.
- Choice: (a).
- Why: existing user configs and per-song files keep working (parity first).

## D5. Rhythm "view"

- Options: (a) 3 switcher pages (lyrics/channels/piano), rhythm as dashboard
  lamps per code; (b) add a 4th rhythm page per README wording.
- Choice: (a).
- Why: code is the source of truth; README wording is a doc bug to fix
  separately. No invented scope.

## D6. Print lyrics

- Options: (a) save + `xdg-open` and/or ashpd print portal; (b) drop Print.
- Choice: (a).
- Why: parity — a visible entry must keep working through the closest
  faithful path, tracked as a gap, never silently removed.

## D7. Single instance mechanism

- Options: (a) D-Bus well-known-name claim + forward argv files;
  (b) portal `open_uri` only.
- Choice: (a), with portal-delivered paths accepted everywhere.
- Why: preserves the observable `HANDLES_OPEN` contract including CLI and
  MIME launches outside a portal context.

## D9. Persistence backend — U13 contradiction (ux `cosmic-config` vs arch GKeyFile INI)

- Options: (a) GKeyFile-compatible INI at existing paths; (b) `cosmic-config`;
  (c) dual-write/migrate-once.
- Choice: (a), already D4 — ux.md corrected to match.
- Why: parity (existing configs keep working) outranks COSMIC conventions
  (priority 5). No migration code, no data-loss window.

## D10. Devil P1 — per-task Flatpak gate vs tiered gates

- Options: (a) keep user's literal DoD (flatpak-builder + smoke per task);
  (b) tier: cargo-only per task, Flatpak at release.
- Choice: (a) with a `--fast` inner loop; full gate before every commit.
- Why: the user's DoD is explicit; weakening it silently is not the lead's
  call. Incremental flatpak rebuilds make (a) affordable after T1.

## D11. Bug-compat rulings (devil U2/U3/U10/U12/U14, A3/A8/A14)

- FIX (clear contrary intent or correctness trap, small + tested): save window
  size + repeatMode on close-request (U2); menu Loop forces toggle ON then
  opens dialog (U3); piano clicks map in drawn range (U12); prefs close
  applies solo% (U14); UI syncs to engine mix state on song load (A8).
- PRESERVE (observable behavior, tests pin it): lyric static+live duplication
  (U10); audition bypassing mute/pitch + idle-only note-off (A3);
  remove/shuffle-not-adjusting-index + load-keeping-nonexistent (A14).
- Why: parity first — preserve what users see, fix only data-loss/trap bugs
  whose intent is evident in the code.

## D12. Single instance — A4 (hand-rolled D-Bus vs `run_single_instance`)

- Options: (a) zbus hand-roll (arch §2.5); (b) `cosmic::app::run_single_instance`.
- Choice: (b); amends D7's mechanism, keeps its contract (forward + present,
  replace semantics per P2). `PianoVisible` (A3) dropped — no C++ surface.
- Why: same observable contract, less code, COSMIC-idiomatic; T0 verifies.

## D13. i18n scope (U15) and notesFont wiring (U6)

- Choice: i18n explicitly deferred (baseline English-only, `language`
  write-only — no regression possible); `notesFont` stays a headless carried
  key, wiring dropped as not parity-safe.
- Why: no invented scope; parity of the shipped behavior.

## D14. CMake end-state (P9)

- Choice: CMake/ctest stays green through T14; T15 retires the C++ build,
  replaces CMake-dependent checks with Rust/`verify.sh` successors, and
  records the legacy gresource-prefix grandfathering.
- Why: bisected history stays green; retirement is explicit, not rot.

## D8. Task granularity and DoD gating

- Options: (a) walking skeleton first so full DoD (incl. Flatpak + smoke)
  applies from T1; (b) core-first, Flatpak later with DoD waivers.
- Choice: (a).
- Why: unwaived DoD keeps every commit shippable and reviewable; waivers
  would hide sandbox regressions until late.
