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
- Note: version amended by D16 — runtime is now 25.08 (24.08 rustc 1.89 is
  below libcosmic MSRV 1.93). The freedesktop-over-COSMIC choice stands.

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

## D8. Task granularity and DoD gating

- Options: (a) walking skeleton first so full DoD (incl. Flatpak + smoke)
  applies from T1; (b) core-first, Flatpak later with DoD waivers.
- Choice: (a).
- Why: unwaived DoD keeps every commit shippable and reviewable; waivers
  would hide sandbox regressions until late.

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

## D15. lyricsFont dead-CSS ruling (T0 arch spike report §5, empirical GTK probe)

- Question: C++ `font: Sans 16` is invalid CSS (GtkCssProvider drops it —
  empirically probed), so the default `lyricsFont` never applies; only
  `16pt Sans`-shaped input works. FIX (parse + apply per T0 font spec) or
  PRESERVE (store-only quirk)?
- Choice: FIX — parse per spec (trailing size, px/pt rules, keyword set,
  family remainder, Sans fallback) and apply; default `Sans 16` renders as
  16pt Sans.
- Why: D11 framework — the ux D2 Preferences row's evident intent is a working font setting;
  a no-op default is a trap, and the quirk is a GTK-CSS accident, not
  behavior. Tested per spec incl. divergent `12px` vs `12pt` cases.

## D16. T0 pins (arch probe `cargo check` green; vendoring mechanism proven on synthetic probe)

- libcosmic git rev `7e82198ab1aba712640b0167e58ad37bf03012ca` (master
  2026-09-11); pop-os/iced submodule `87e1787fbb9b924576e6f6ea2ef2f45d0729e63d`
  (iced 0.14.0, iced_widget 0.14.2); features `about` (non-default, required),
  `single-instance`, `xdg-portal` + defaults. NEVER `rfd` + `xdg-portal`
  together (duplicate definitions, source-verified).
- MSRV/`rust-version = "1.93"` (libcosmic floor); CI/local toolchain 1.98.1
  (`rust-toolchain.toml`); Flatpak runtime **25.08** (24.08 rustc 1.89 is
  below the floor — amends D2's version and packaging.md §1.2).
- Crates: `alsa 0.12`, `chardetng 1`, `encoding_rs 0.8`, `url 2.5`,
  `tempfile` (dev); hand-written FluidSynth FFI (crate rejected as
  abandonware); hand-rolled GKeyFile INI; ashpd via `cosmic::dialog::ashpd`
  re-export; no direct rfd/zbus/tokio/dirs/serde. `--locked` from T1.
- flatpak-cargo-generator pinned rev `1fc32195e3e60fe5c97f0af646dec7a99df5962b`,
  vendored at `flatpak/`; regen `python3 flatpak/flatpak-cargo-generator.py
  Cargo.lock -o flatpak/cargo-sources.json`. Never `--git-tarballs`
  (submodules). Mechanism proven on a synthetic git+submodule probe build;
  real-rev (libcosmic+iced) vendoring + offline check happen in T1.
- Widget verdicts: all cited paths compile/source-verified incl. `spin_button`
  (no `number_input`), `color_picker`, `text::Rich/Span` (no `rich_text`
  module, though `iced::widget::rich_text()`/`span()` helper fns exist),
  `menu::Tree` (no `MenuTree`), `nav_bar::Model`, tooltip CLEARED,
  `cosmic::app::toaster` MISSING (use `widget::toaster`); iced checkbox is
  1-arg `checkbox(bool).label()`.
- `run_single_instance` verified (`Flags: CosmicFlags`, `Message:
  Clone+Debug+Send+'static`, `dbus_activation` hook,
  `COSMIC_SINGLE_INSTANCE=false` escape); file_chooser portal verified
  (`FileFilter` glob/mimetype,
  `Error::Cancelled`, `Url` results; open-dialog initial dir/file_name are
  dead upstream — D1 `lastDirectory` wiring blocked, save names OK).
- DnD: X11 `FileDropped` subscription + Wayland `dnd_destination`
  (`text/uri-list` parsed manually, sandboxed `file_transfer`); portal
  chooser stays primary; runtime-confirm COSMIC Wayland drags in T10.
- Parsers: color = hand-rolled `gdk_rgba_parse` parity (~80 lines, NOT
  css-color); font = T0 empirical spec. Both with rejection tests.

## D17. D2 color rows — `color_picker` vs text-only (T0 verdict was "evaluate", T11 needs a ruling)

- Options: (a) `widget::color_picker` bound to the text field (text stays
  source of truth, `gdk_rgba_parse` parity); (b) text + swatch only.
- Choice: (a), with fallback to (b) if T11 integration misbehaves.
- Why: COSMIC-idiomatic picker with a parity-safe text path; the probe
  compiled `ColorPickerModel::new`, so (a) is worth attempting first.

## D18. Rust workspace location during migration

- Options: (a) repo root (`Cargo.toml` + `src/*.rs` alongside C++ until T15);
  (b) `rust/` subdirectory, moved at T15.
- Choice: (a).
- Why: end-state layout from day one, no heroic move at T15. No interference:
  CMakeLists lists C++ sources explicitly (ignores `.rs`), cargo ignores
  `.cpp`/`.cmake`, `tests/` and `examples/` coexist the same way. T15 deletes
  the C++ files; Rust paths never move.

## D19. Engine protocol ownership (T7 duplication vs message.rs T1 stubs)

- Options: (a) `core::player` owns `EngineCommand` + `PlayerEvent`, `message.rs`
  re-exports them in T9; (b) `message.rs` owns them, core imports app layer;
  (c) bridge `From` conversions between parallel definitions.
- Choice: (a).
- Why: the dependency rule forbids core importing app-layer types, and the
  engine thread needs the types in `core`. Re-export kills drift with no
  bridge code. `PreviewNote/Off` stay app-only (A3 bypass); `NoteOff` gains
  `vel` (C++ parity) when message.rs adopts core's types in T9.
