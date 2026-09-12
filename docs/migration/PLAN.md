# Migration PLAN — Gosh MIDI Player (GTK4/C++17 → libcosmic/Rust)

- App: Gosh MIDI Player, ID `com.goshapps.GoshMIDI`, command `dmidiplayer`
  (adapted from template `[APP NAME]` / `[LANGUAGE]` → Gosh MIDI Player / C++17).
- Sources: [ux.md](ux.md), [architecture.md](architecture.md),
  [packaging.md](packaging.md), devil's-advocate review (OBJECT, 43 findings,
  all lead-verified against source) — corrections folded into T0 and the
  contract table below; owner doc fixes land before the Phase 1 commit.
- Branch: `cosmic-migration`.

## 0. Gates (tiered, per devil P1 — user's DoD kept literally)

- **Inner loop** `scripts/verify.sh --fast`: `cargo build --locked`,
  `cargo clippy --locked -- -D warnings`, `cargo test --locked`, CMake/ctest
  green (until T15 retires CMake).
- **Pre-commit gate** (every task, no waivers): full `verify.sh` — fast tier
  plus `flatpak-builder` + in-sandbox smoke. Incremental flatpak rebuilds keep
  this cheap after T1; first build cost is paid once.
- **Release gate** (T15 + every RC): DE matrix (COSMIC, GNOME Wayland+X11, KDE
  Wayland+X11), fresh-profile audible playback, second-`flatpak run`
  single-instance forward.
- No task breaks CMake CI until T15 retires it; `check_single_instance_open`
  retirement is sequenced in T10 with a Rust successor test first.

## 1. Feature parity checklist

Ticked only when verified in the running Flatpak (Phase 3 is the final pass).
Counts are devil-corrected (U1): **23 actions, 23 menu items, 30 settings
keys, 6 toast sites**. CI pins all four (grep/test).

### Windows / dialogs
- [ ] W1 main window: size + repeatMode persist (1100x720), title with file, single-instance
      open/present (**replace** playlist, P2), DnD replace-playlist (or T0
      evidence + chooser parity, U9)
- [ ] D1 open files: 6 filters, multi-select, `.lst`-vs-songs branching
- [ ] D2 preferences: all 17 rows, commit-on-close (**fixed**: solo% applies
      at close too, U14) + save
- [ ] D3 MIDI setup: field order, refill/profile/test/apply, hint strings
- [ ] D4 about: all strings, credit section, 3 links
- [ ] D5 help: full `kHelpText` copy
- [ ] D6 file info: all fields + copyright lines + empty state
- [ ] D7 jump: range 1..lastBar, pause-jump-resume
- [ ] D8 loop: from/to seeds, toggle pairing, cancel-resets-toggle; menu
      Loop forces toggle ON then opens dialog (U3 fix, silent no-op banned)
- [ ] D9/D10 playlist open/save: names, filters, no-autoplay-on-open-list
- [ ] D11 SoundFont chooser: sf2/sf3 filters, deferred commit
- [ ] D12 save lyrics / D13 print lyrics (portal/export path, gap tracked)
- [ ] Toasts: all 6 trigger sites (U1d)

### Widgets / views
- [ ] Sidebar: basename rows, click-select vs double-click/Enter-play (U11),
      row-activated play, 8 tools in order
- [ ] Menu: all 4 sections / 23 entries (U1b, incl. 2 external links)
- [ ] Header: 8 transport buttons + loop toggle
- [ ] Dashboard: time format, live tempo/volume, pitch -12..+12, lamps, bar:beat
- [ ] Sliders: pos seek guard, tempo 50–200, volume 0–200, reset buttons
- [ ] Switcher: 3 pages (lyrics/channels/piano) — rhythm stays dashboard lamps
- [ ] Lyrics: 3 dropdowns + preselects, copy/save/print, 3 colors, autoscroll,
      static+live concatenation **preserved incl. duplication quirk** (U10)
- [ ] Channels: 16 used-only rows, 8 fields, solo/mute formula, patch reselect
- [ ] Piano: tighten/show/hide, palette/velocity/names, click audition;
      clicks map in the **drawn** range (U12 fix); audition bypasses
      mute/pitch with idle-only note-off (A3); no per-channel visibility
      toggle (no C++ surface — new scope banned)
- [ ] Status line: 5 states + transitions

### Input / data / integration
- [ ] All 23 actions (U1a) + all 9 accelerators kept
- [ ] All 30 `AppSettings` keys round-trip (U1c, incl. 4 headless keys)
- [ ] Per-song `.cfg` schema + manual/auto triggers; `.lst` format unchanged;
      playlist index semantics **preserved** (remove/shuffle don't adjust,
      move does; load keeps nonexistent paths — A14)
- [ ] F1–F16 flows end to end; style.css mapped in light + dark (dead
      `.channel-meter` entry removed, U16)
- [ ] Desktop file (+`DBusActivatable=true`, P8), MIME, CLI flags/messages,
      `dmidiplayer` command compat, icon names, clipboard copy,
      single-instance via `run_single_instance` (A4) incl. `flatpak run` case

### Engine behavior (1:1)
- [ ] File support + loading-error strings; scheduling math; tempo setter
      **rejects** out-of-range (no clamp, A7/P10a); `div==0 → 96` (A7)
- [ ] Start sequence (reset → companion → controllers → volumes), resume skip;
      `sendVolumeEvents` also resets `m_volume[ch]=100` (A9)
- [ ] Mixing rules (CC7 floor, drums pitch exclusion, mute/lock/patch);
      `ResetPosition` resets beat cursor (A1), distinct from seek
- [ ] Command polling in echo loop + wakeup; stop-during-long-sleep specified
      + sparse-fixture latency test (A2)
- [ ] Backpressure per variant (coalesced `Time`, lossless notes — A13)
- [ ] Loop re-jump; end-of-song policy; ALSA enum filter (incl. `PORT` type +
      `type==0` passthrough, advanced-list fallback, unknown→ALSA, -d/-c
      both-or-neither — A6); driver fallback order; default-SF search;
      test-note ch1 middle C 400 ms; Rust `open` returns `Result` with the
      no-SF+no-ALSA path tested (A15)
- [ ] Charset: normalization spec (ISO-8859-1≠windows-1252 in 0x80–0x9F,
      uchardet-name mapping, off-list dropdown fallback — A10) + golden tests
- [ ] GKeyFile: escaping, corrupt-file keeps defaults, dir creation, unknown
      keys kept (A11); INI wins over cosmic-config (U13 → DECISIONS D4/D9)
- [ ] Font strings: family/size/weight parse + malformed fallback (U6);
      `notesFont` stays headless (wiring dropped)
- [ ] Colors: `gdk_rgba_parse` parity (named/rgb()/#rgb/#rrggbb — U5), invalid
      rejected + tested; `widget::color_picker` evaluated in T0

### Accessibility (U7 — was absent; now required)
- [ ] Baseline stated: no a11y regression by fiat; acceptance = every action
      keyboard-reachable, accessible names set, focus visible, tooltip story
      resolved (verify iced/libcosmic tooltip in T0) or gap-tracked like D13
- [ ] i18n explicitly deferred: baseline is English-only, `language` stays
      write-only (U15) — no regression possible

## 2. Risk list

1. ALSA sequencer needs `--device=all` (no seq-only permission). Doc + recheck
   per runtime bump; QA covers external-port playback.
2. Host SoundFonts invisible in sandbox → bundled fallback GM SF (T6 selects:
   license+size evidence) + portal picker + re-prompt UX for stale absolute
   paths + sandbox-aware search paths (P7). Fresh profile must play audibly
   with zero host setup.
3. Vendoring drift / git-dep pins → CI freshness diff; prefer crates.io;
   `--locked` from the first Rust task (P5).
4. Print (no iced backend) → save + xdg-open and/or ashpd print portal.
5. System file-drop depends on T0 `dnd_*` spike → portal choosers primary.
6. Player-thread races must not survive → message-passing only (arch §2.4).
7. Binding API drift (`alsa`, FluidSynth, chardetng, encoding_rs, ashpd) →
   versions pinned in T0, wrapped in traits, Dummy keeps CI green.
8. Codec-name mapping (A10) must preserve the 14-codec UI list.
9. COSMIC runtime may never ship → freedesktop default; never the only path.
10. Portal variance across DEs → provider matrix (P12) + GNOME/KDE matrix RCs.
11. Existing user data outside the sandbox (config/song `.cfg`/playlists) →
    T15 migration story + inaccessible-path UX (P7).
12. Full flatpak gate per task is slow on first build → paid once in T1,
    incremental after; `--fast` for iteration (P1 ruling).

## 3. Ordered task list (buildable after every task)

Ownership: ux = view/widget modules + ux.md; arch = core/state/config +
architecture.md; pack = flatpak/ tests/ scripts/ + packaging.md;
lead = PLAN/DECISIONS/REPORT + glue.

- [x] T0 Pin + API spike (arch+pack+ux, blocks all UI tasks): pin libcosmic — DONE (both spikes green, pins in D16, parser specs in arch §2.6, devil SIGN-OFF, no repo code touched)
      rev, toolchain/MSRV, every crate (P5); compile-verify each cited widget
      path (`spin_button` not `number_input`, `color_picker`, about/dialog/
      menu/nav/settings/tab/segmented/text_editor/toaster/toggler/dropdown/
      progress_bar, iced checkbox/slider/canvas/container/row/column/stack/
      rich_text, at-risk `tooltip`); verify `run_single_instance` +
      `dbus_activation`; `dialog::file_chooser` provider/features + Url→
      PathBuf + cancel (A5/P12); `dnd_*` OS file-drop verdict (U9); color
      parser + font parser specs (U5/U6). Output: pinned pins + verdicts in
      DECISIONS; owner docs corrected (§5).
- [x] T1 Walking skeleton (arch+pack, lead commits): Rust workspace per arch — DONE (trio 9/9, flatpak-builder exit 0, in-sandbox smoke PASS, full verify.sh PASS, devil SIGN-OFF)
      §5 (stubs), CLI incl. headless `--dry-run/--dump-info` exiting before
      GUI (P10c) + `-p/-f/-d/-c` (both-or-neither, A6d), app shell window,
      `run_single_instance` wiring, `scripts/verify.sh` (+`--fast`, incl.
      count-pin greps for 23 actions / 23 menu entries / 30 settings keys /
      6 toast sites) + `scripts/smoke.sh`,
      `flatpak/com.goshapps.GoshMIDI.yml` (offline cargo
      commands, examples generation/install — P6) + `cargo-sources.json`
      (vendored generator, P4c), CI `cosmic` job with fixed triggers +
      full system deps (P4). Binary name `dmidiplayer` (P3/D3). Full DoD
      from here on.
- [x] T2 `core::event` (arch): MidiEvent/Kind/TextType/constants/factories, — DONE (18 tests, suite 27/27, devil differential SIGN-OFF 90/90 paths + byte-identical semantics)
      extension filter; unit tests incl. case-insensitive ext matrix.
- [x] T3 SMF decode + `Sequence` store/timing/nav (arch): `smf.rs`, ticks↔µs, — DONE (24+ tests, suite 51+, C++ differential goldens, raw-byte label fix, devil SIGN-OFF)
      beats, bar nav, tempo reject (A7), `div==0→96`; golden tests vs C++.
- [x] T4 RMI/WRK + text/charset (arch): `rmi.rs`, `wrk.rs` (banked SysEx, — DONE (23 tests, suite 76+, C++ differential + codec sweep, deviations devil-ruled, SIGN-OFF)
      track offsets), chardetng/encoding_rs + normalization spec (A10),
      14 codecs, KAR headers, off-list dropdown rule; goldens + lyric-decode
      goldens across fixtures/charsets.
- [x] T5 SysEx/instruments/output-trait (arch): resets/pacing, `.syx` parse, — DONE (36 tests, suite 113/113, 3015-vector differential fuzz clean, devil SIGN-OFF; 2 doc nits carried to T6)
      companion lookup, patch tables, `inferDeviceProfile`, `MidiOutput`
      trait (failable `open`, A15) + Dummy + Manager (close-previous-first,
      unknown→ALSA — A6c); unit tests.
- [x] T6 ALSA + FluidSynth backends (arch): port enum + exact filter (A6a), — DONE (21 tests, suite 134/134, live-hardware + FFI-header verified, fluidsynth 2.6.0 manifest module, FULL PASS, devil SIGN-OFF ×2; port-parse + sdl2-string nits carried)
      advanced-list fallback (A6b), subscribe, driver fallback order, gain
      0.6, bend remap, sandbox-aware SF search paths + bundled-SF selection
      (license/size evidence, P7), `lastError`; hardware tests gated,
      Dummy-backed + no-SF tests always green.
- [x] T7 Player engine (arch): scheduler (absolute/incremental, echo polling — DONE (41 tests, suite 175/175, fake-clock + threaded, idle-Stop fix, devil SIGN-OFF, D19 protocol ownership)
      + wakeup — A2), start sequence, mixing rules, `ResetPosition` (A1),
      command channel + `PlayerEvent` stream with per-variant backpressure
      (A13); deterministic scheduler tests (fake clock) + sparse-latency +
      flood tests.
- [x] T8 Config + playlist state (arch): GKeyFile-compat reader/writer — DONE (42 tests, suite 217/217, GLib bidirectional byte-identical, devil SIGN-OFF; ensure_data_dir obligation + 2 doc nits carried to T9)
      (escaping, corrupt→defaults, dir creation, unknown keys — A11), all
      30 keys + MRU-10 + portable mode, song-settings schema/apply,
      save-on-close-request (**fix**, U2) + save-point truth table, `.lst`
      load/save with A14 semantics; fixture round-trip tests.
- [x] T9 App shell + transport (arch+ux): `Message`/`update`/`view`/tasks/ — DONE (suite 311/311, 69 variants tested, F1/F2 fixed, FULL PASS, devil substance SIGN-OFF; fmt-leg OBJECT adjudicated: cargo fmt --check exit 0)
      subscriptions, header transport, dashboard, 3 sliders, status line,
      toasts; commit-on-close prefs (U14); load-time UI↔engine sync (**fix**,
      A8); unit tests per Message variant; repeat/finish policy.
- [ ] T10 Playlist + file flows (ux+arch): sidebar (U11 select/activate) + 8
      tools, D1/D9/D10 via pinned chooser (Url→PathBuf, cancel — A5),
      single-instance forward + present + `COSMIC_SINGLE_INSTANCE=false`
      escape (A4), portal paths incl. replace-asserting integration test
      (P2), DnD per T0 verdict; retire `check_single_instance_open.cmake`
      only after Rust successor is green (P1).
- [ ] T11 Lyrics (ux): page + filters + live tail (U10 concatenation
      preserved), copy/save/print routing, D12/D13, prefs Lyrics page
      (color picker per T0, font spec U6); integration tests.
- [ ] T12 Channels + prefs (ux): 16 rows + solo/mute formula + meters +
      patch relabel, prefs General/Piano pages (`spin_button`s, U4);
      integration tests.
- [ ] T13 Piano + remaining dialogs (ux): canvas program + drawn-range clicks
      (U12) + A3 audition rules, D3 MIDI setup + D6/D7/D8/D5/D4, song-settings
      manual actions; integration tests.
- [ ] T14 Shortcuts + theming + integration (ux): 9 accelerators, 23-item menu
      (U1b), style mapping light/dark, desktop/MIME/icons/clipboard, a11y
      acceptance (U7), metainfo description + Rhythm-claim fix (U8/P11).
- [ ] T15 Release hardening (pack, devil leads): full Message-variant unit
      coverage audit, P10b missing-case sweep (song-settings round-trip,
      escaping/corrupt, portal entries, loop+repeat interplay), CMake
      retirement (P9: remove C++ build + CMake-dependent greps, grandfather
      note for the legacy gresource prefix), sandbox data-migration story +
      stale-SoundFont UX (P7), metainfo `<release>`, GNOME/KDE matrix,
      REPORT.md inputs. Gates Phase 3.

## 4. How tasks stay buildable

New modules land behind the existing shell: core grows as pure-Rust tested
code, views land one page per task rendering from `Option<LoadedSong>` empty
states. `verify.sh` (fast) runs continuously, full gate + Flatpak before every
task commit; failures become new tasks, never carried. CMake/ctest stays green
until T15.

## 5. Owner doc corrections (Phase 1 exit requirement)

- ux.md: U1 counts; U2/U3/U12/U14 behavior rulings; U4 `spin_button`; U5
  color picker + `gdk_rgba_parse` parity; U6 font spec, drop notesFont wiring;
  U7 a11y section; U8 metainfo scope; U9 T0 spike ref; U10 preserve + test;
  U11 select/activate; U13 INI (match arch); U15 i18n deferred; U16 dead CSS.
- architecture.md: A1 `ResetPosition`; A2 echo polling + wakeup; A3 audition
  rules, drop `PianoVisible`; A4 rewrite §2.5 on `run_single_instance`; A5
  chooser provider + Url→PathBuf; A6 four ALSA pins; A7 reject wording +
  div==0; A8 UI-sync fix; A9 volume reset; A10 charset spec; A11 GKeyFile
  teeth; A12 field count; A13 backpressure; A14 preserve; A15 failable open.
- packaging.md: P1 tiered gates + CMake sequencing; P2 replace; P3
  `dmidiplayer`; P4 triggers/deps/vendored generator; P5 pins (with T0); P6
  examples generation/install; P7 migration story + SF criteria; P8
  DBusActivatable + flatpak-run case; P9 CMake end-state; P10 clamps/cases/
  headless flags; P11 metainfo scope; P12 provider matrix.
- Each owner applies only their list, then devil re-reviews all three + PLAN.
