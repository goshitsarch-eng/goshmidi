# Migration REPORT — Gosh MIDI Player 1.8.0 (libcosmic/Rust)

Status: **FINAL (2026-09-12)** — automated project DoD HOLDS on HEAD
`85dd3c3` (`verify.sh --fast` + full `verify.sh` re-run PASS in the final
slice, install + bundle + bundle-install + installed-app headless run all
executed and green). Release gate: every machine-verifiable half PASSES;
the human-on-console rows (§7) remain — human-audible playback, on-console
DE matrix (incl. X11 adjudication — still environment-impossible here),
stale re-prompt click-through, literal second-`flatpak run` on a live
desktop. Prior S4/S5 sign-off record is preserved in §5–§7.

## 0. Release + environment

- Version 1.8.0, 2026-09-12 — first libcosmic/Rust release (GTK4/C++ retired).
- Branch `cosmic-migration`; app ID `com.goshapps.GoshMIDI`; command `dmidiplayer`.
- Toolchain: rustc 1.98.1 (`rust-toolchain.toml`), libcosmic rev `7e82198`
  (D16), freedesktop runtime/SDK 25.08, FluidSynth 2.6.0 (flatpak module).
- Bundled SoundFont: `FluidR3Mono_GM.sf3` v2.312, MIT, 14,563,174 bytes
  (LibreScore/sf3 @ `3a6eda8`, sha256 `cfcd66d8…2794ec2` — S2 §6).
- Final-slice probe host (re-observed S6): flatpak 1.18.2 +
  flatpak-builder 1.4.10, freedesktop 25.08 runtime installed, rustc/cargo
  1.98.1, ALSA 1.2.16.1, FluidSynth 2.6.0; `gnome-shell`/`plasmashell`/
  `kwin`/`mutter`/`cosmic-comp` all absent, `/usr/share/wayland-sessions`
  = `sway.desktop` only, `/usr/share/xsessions` empty. (S4 ran on NixOS +
  COSMIC with a nested headless weston; that record stands in §6.)
- Release bundle (S6-measured): single-file `.flatpak` = 20,916,376 bytes.

## 1. What was built

A clean-room Rust port of the Gosh MIDI Player (ex GTK4/C++17) on
libcosmic, same app ID (`com.goshapps.GoshMIDI`) and command
(`dmidiplayer`), same config paths (`~/.config/dmidiplayer`,
`~/.dmidiplayer`), released as a freedesktop-25.08 Flatpak (1.8.0).

- **Core engine** (`src/core/`): SMF/KAR/RMI/WRK decode, `Sequence`
  store/timing/navigation, charset pipeline (chardetng/encoding_rs, 14
  codecs), SysEx/instrument/device-profile tables, `MidiOutput` trait +
  Dummy/Manager, ALSA + hand-written-FFI FluidSynth backends, threaded
  player scheduler with per-variant backpressure (`EngineCommand` /
  `PlayerEvent`), GKeyFile-compatible INI config (30 keys + MRU-10 +
  portable mode), song-settings schema, `.lst` playlist state.
- **App shell + views** (`src/app.rs`, `src/state/`, `src/ui/`): Elm
  `Message`/`update`/`view`, header transport, dashboard, 3 sliders,
  status line, toasts; sidebar playlist + 8 tools; lyrics/channels/piano
  switcher pages; preferences, MIDI setup, file info, jump, loop, help,
  about, lyrics save/print, SoundFont chooser, playlist open/save.
- **Desktop integration**: desktop file (only delta vs C++ ship:
  `DBusActivatable=true`, P8) + D-Bus service file, metainfo (toolkit
  wording, rhythm-claim fix, 1.8.0 `<release>`), 6 hicolor app icons, 29
  vendored symbolic-icon hicolor fallbacks (T14c §1.6), man page,
  single-instance forward + present via `run_single_instance` with
  `COSMIC_SINGLE_INSTANCE=false` escape.
- **Packaging/QA**: `flatpak/com.goshapps.GoshMIDI.yml` (fluidsynth 2.6.0
  + bundled-SF + cargo modules), vendored pinned cargo generator +
  committed `cargo-sources.json`, `scripts/verify.sh` (tiered gate) +
  `scripts/smoke.sh` (headless + in-sandbox), `cosmic-build.yml` CI
  (CMake job retired, D14/P9), 10 shipped examples with generated
  `examples.lst` (P6).

### Parity counts (PLAN §1/U1 — all pinned)

| Count | Value | Pins |
|---|---|---|
| Actions | 23 | `MenuAction::ALL: [MenuAction; 23]` + unit `inventory_is_23_entries_in_cpp_section_order` + `keys_flows::menu_maps_all_23_entries_to_documented_messages` |
| Menu entries | 23 | same inventory (actions+menu unified in Rust) |
| Settings keys | 30 | `KNOWN_KEYS: [&str; 30]` + unit `KNOWN_KEYS.len() == 30` |
| Toast trigger sites | 6 | `Message::Toast` funnel pin + per-path tests (`open`/`playlist`/`lyrics`/`dialogs`/`keys` flows + `app.rs` error→toast units) |
| Accelerators | 9 | `ACCELERATORS: [Accelerator; 9]` + unit + `keys_flows` |

T15 retired the C++ greps; the Rust successors above are enforced by both
`cargo test` and the `verify.sh` count-pins step (S2, re-verified S6).

## 2. Deviations from the original app + why

Rule applied throughout (DECISIONS D11): preserve observable behavior,
fix only data-loss/trap bugs whose contrary intent is evident in the
code. Every item below is test-pinned.

### Deliberate fixes (C++ behavior NOT kept)

- Window size + repeatMode now save on close-request (U2 — C++ lost them).
- Menu Loop forces the loop toggle ON, then opens the dialog (U3 — C++
  silently no-op'd when the toggle was off; silent no-ops banned).
- Piano clicks map in the drawn range (U12 — C++ mapped the wrong range).
- Prefs close applies solo% like every other row (U14 — C++ dropped it).
- UI re-syncs to engine mix state on song load (A8 — C++ left stale
  channel/patch display).
- Lyrics filter selections hold (T11 — C++ `refreshLyrics` reset both
  filters on every user change, so the dropdowns could never hold a
  selection).
- `lyricsFont` default `Sans 16` is parsed and applied per the T0 font
  spec (D15 — invalid GTK CSS the old provider dropped, so the setting
  was a no-op by accident, not behavior).
- MIDI Setup Apply stops the engine only when the reseat plan actually
  sends something; end state identical (D21 — C++ stopped unconditionally,
  interrupting playback for no-change Applies).
- Out-of-range piano clicks emit silence (D21 — C++ auditioned a phantom
  wrong note).
- Window/WM title follows the header title on boot/song-load/forward
  (S5 W1 fix — the title update existed only in the header).

### Preserved quirks (kept 1:1, tests pin them)

- Lyrics static+live concatenation incl. the duplication quirk (U10).
- Piano audition bypassing mute/pitch, idle-only note-off (A3).
- Playlist index semantics: remove/shuffle don't adjust the index, move
  does; `.lst` load keeps nonexistent paths verbatim (A14).
- Tempo setter rejects out-of-range (no clamp), `div==0 → 96` (A7);
  `sendVolumeEvents` resets `m_volume[ch]=100` (A9).

### Platform adaptations (no C++ equivalent possible)

- Print lyrics = temp `.txt` export + `xdg-open` (D6/D13 — iced has no
  print backend; in-sandbox this routes through the OpenURI portal; C++
  print-dialog success was likewise silent). Ashpd `PrintProxy` rejected:
  PDF-oriented flow, varies per DE, and the app ships no PDF writer.
- Rhythm stays dashboard lamps, not a 4th switcher page (D5 — code truth;
  metainfo + README wording fixed instead).
- Single instance via `run_single_instance` (D7/D12 — same observable
  contract: forward + present + replace, incl. the `flatpak run` case).
- Freedesktop runtime 25.08, never COSMIC-only (D2 — shippable everywhere;
  24.08's rustc 1.89 is below the 1.93 MSRV floor, D16).
- Settings stay GKeyFile-compatible INI at the existing paths (D4/D9 —
  no `cosmic-config` migration, no data-loss window; unknown keys kept).
- Bundled `FluidR3Mono_GM.sf3` searched first + stale-path re-prompt UX
  (P7/§6 — host `/usr/share` SoundFonts are invisible in the sandbox; a
  dead absolute `soundFont` falls back to bundled with a toast naming
  the stale path, D3 warning, D11 re-locate).
- Exactly the 9 C++ accelerators, no invented bindings (D22 — 12/23
  actions keyboard-reachable: 9 accels + Tab-Enter on the Rew/Fwd/Loop
  button twins; the 11 menu-only actions are a tracked gap, like D13).
- Dashboard `.dash-value` per-mode ink (D23 — theme accent in dark
  (10.45:1 AAA), C++ `#62a0ea` in light (6.96:1 AA); the light accent
  measured 2.12:1 and fails AA on the explicit dark card).
- Symbolic icons: 29-name hicolor fallback vendored from pinned upstreams
  (T14c — in-sandbox theme is always `Cosmic`, which the freedesktop
  runtime doesn't ship; without the fallback, GNOME/KDE hosts render
  empty buttons).
- i18n explicitly deferred (D13 — baseline English-only, `language`
  stays write-only); `notesFont` stays a headless carried key (U6/D13).

## 3. Known limitations

1. **Manual release rows outstanding** (§7): human-audible fresh-profile
   playback, on-console COSMIC visual pass, GNOME/KDE Wayland+X11 runs,
   stale re-prompt click-through, literal second-`flatpak run` on a live
   desktop. All machine-verifiable halves pass (§5).
2. **X11 needs real-DE adjudication**: the nested-Xwayland softbuffer
   panic (`iced_tiny_skia`, `Visual 0x6d does not use softbuffer's pixel
   format`) reproduces identically sandboxed and unsandboxed, with DRI
   and `LIBGL_ALWAYS_SOFTWARE=1` — not sandbox-caused, but unresolved;
   still environment-impossible here (no GNOME/KDE/Xorg, re-confirmed S6).
3. 11/23 actions are pointer-only via the menu widget (D22 gap, tracked).
4. English-only UI; `language` key write-only (D13).
5. `notesFont` carried but unwired (D13/U6).
6. ALSA sequencer needs the broad `--device=all` (no seq-only Flatpak
   permission exists); re-check each runtime bump (risk 1).
7. Print is temp-file + viewer-open, not an in-app print dialog (D6).
8. No-portal hosts (minimal WM): new picks needing a portal show a
   guidance error; bundled + already-granted paths keep working (P12).
9. Open-dialog initial dir/file-name are dead upstream in the pinned
   chooser (D16) — `lastDirectory` wiring blocked; save names work.
10. CI (`cosmic-build.yml`) runs on merge — S6 gates are local runs.

## 4. Build / run / install instructions (Flatpak)

Prerequisites (packaging.md §3.1; present on the S6 probe host: ALSA
1.2.16.1, FluidSynth 2.6.0, `aiohttp`+`tomlkit` for the generator):

```
$ sudo apt install pkg-config cmake g++ libasound2-dev libfluidsynth-dev \
        libwayland-dev libxkbcommon-dev libfontconfig-dev libfreetype-dev \
        libexpat1-dev python3 python3-aiohttp python3-tomlkit
```

(documented prereq, not re-run on the S6 host — deps were probed present
via `pkg-config`/import instead. Rust 1.93+ via `rust-toolchain.toml`.)

Inner loop — **ran S6, PASS**:

```
$ cargo build --locked
$ sh scripts/verify.sh --fast     # build, clippy, fmt, tests, pins
```

Full gate — **ran S6, PASS** (freshness + builder + in-sandbox smoke +
build-export, ending `verify FULL PASS`):

```
$ sh scripts/verify.sh
```

Headless checks against a local build — **ran S6, PASS** (10/10 files):

```
$ ./target/debug/dmidiplayer --help
$ ./target/debug/dmidiplayer --dry-run examples/test.mid
$ ./target/debug/dmidiplayer --dump-info examples/test.mid
$ sh scripts/smoke.sh
```

Flatpak build + in-sandbox smoke (the full gate's core steps) — **ran S6
inside `verify.sh`, PASS**:

```
$ flatpak-builder --force-clean --user --disable-rofiles-fuse build-dir \
    flatpak/com.goshapps.GoshMIDI.yml
$ flatpak build --bind-mount=/repo="$PWD" build-dir sh /repo/scripts/smoke.sh
```

Install from a local repo — **ran S6 end to end, PASS** (installed,
headless `flatpak run --help` green, then uninstalled without trace):

```
$ flatpak build-export /tmp/goshmidi-repo build-dir
$ flatpak --user remote-add --no-gpg-verify goshmidi-local /tmp/goshmidi-repo
$ flatpak --user install -y goshmidi-local com.goshapps.GoshMIDI
$ flatpak run com.goshapps.GoshMIDI [files...]          # GUI launch
$ flatpak run com.goshapps.GoshMIDI --help              # headless check
```

Single-file bundle — **ran S6, PASS** (20,916,376-byte bundle built,
installed FROM the bundle file, and `flatpak run --dry-run
/app/share/dmidiplayer/test.mid` returned OK inside the installed app):

```
$ flatpak build-bundle /tmp/goshmidi-repo /tmp/goshmidi.flatpak com.goshapps.GoshMIDI
$ flatpak --user install -y /tmp/goshmidi.flatpak
```

Second-instance forward on a live desktop (packaging.md §3.2 recipe —
forward halves verified offscreen in S4/S5 on this exact tree; the
literal form below stays a §7 manual on a real DE):

```
$ flatpak run --file-forwarding com.goshapps.GoshMIDI @@ /host/path/song.mid @@
```

Regenerate vendored sources after any dependency change (generator
`--help` probed S6; freshness diff enforced by the full gate):

```
$ python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
```

Note: S6 install cycles printed a `.../exports/share/icons/hicolor/
index.theme: Permission denied` warning from `cp` — a pre-existing
environment quirk of the shared exports dir; every install/uninstall
still reported complete and the installed app ran correctly.

## 5. Gates

- `sh scripts/verify.sh --fast`: **PASS** (S6 final slice, 2026-09-12 —
  560 lib + 10 bin + 105 integration-file tests, 0 failed, +1 ignored
  doctest; retirement gate, all 4 count pins, identity/desktop/icon/
  soundfont/service+man contracts ok; `desktop-file-validate` +
  `appstreamcli validate` strict-ok).
- `sh scripts/verify.sh` (full: vendoring freshness + flatpak-builder +
  in-sandbox smoke + build-export): **PASS** (S6, 2026-09-12 — freshness
  diff clean, builder exit 0, smoke: P6 10/10 examples resolve, bundled
  SoundFont 14,563,174 bytes + sfbk magic + MIT license, D-Bus service
  file + man page, `--help`/DE-markers/`--dry-run`(10)/`--dump-info`(10)/
  missing-file all green, zero panics; build-export commit `86418847…`,
  `verify FULL PASS`).
- S6 install/bundle verification (new — closes the installability half
  of old §4 step 3): build-export → user remote-add → install →
  displayless `flatpak run --help` green → `build-bundle` (20,916,376
  bytes) → uninstall → install FROM the bundle file → displayless
  `flatpak run --dry-run /app/share/dmidiplayer/test.mid` OK → full
  cleanup (no install, remote, or app data remains).
- S6 release gate: **all machine-verifiable halves PASS** (offscreen
  Wayland launch + forward + synth render per S4/S5 on this exact tree,
  plus the S6 install/bundle/install-from-bundle runs above). Human rows
  remain — see §7.
- Prior runs (record): S4 `--fast` PASS (559+10+103) + full PASS (one
  run); S5 `--fast` PASS (560+10+105) + full PASS (two runs, one on the
  final tree). Detail in §6.

## 6. Slice inputs

### S0 — drift triage (landed: `2cb6426`)

- Committed test-only drift (settings/song round-trip pins, loop×repeat
  engine legs, `finish_action` doc); held `theme.rs` for S3 (cites ux §15.1,
  which does not exist yet).

### S1 — arch coverage + P10b close-out (landed: `4dadd91`)

- Portal `.lst` legs, loop×repeat note, Message-variant audit
  (per commit title; details: commit + arch owner).

### S2 — pack retirement + metainfo + migration story (landed: `8f3ef37`)

- CMake/C++ retired (D14/P9): 30 tracked paths deleted (all `CMakeLists.txt`,
  `cmake/`, C++ sources, `check_identity.cmake`, `test_sequence.cpp`,
  `data/dmidiplayer.gresource.xml`, `linux-build.yml`); legacy gresource
  prefix resolved by removal. `data/style.css` kept as the `theme.rs`
  mapping reference (no build consumer).
- Out of scope, carried to S3 (S0 hold stands): `theme.rs` per-mode
  contrast + `warning_text` cite ux §15.1, which does not exist yet (ux.md
  ends at §14) — reverted from this slice; S3 lands them with §15.1.
- Count pins replaced with Rust successors (§1 table); toast sites pin
  the `Message::Toast` funnel + named tests (no naive count exists in Rust).
- Metainfo `<release version="1.8.0" date="2026-09-12">` (P11); T14c
  toolkit + rhythm wording re-verified by the same test file.
- Flatpak `soundfont` module: pinned `type: file` sources (font + MIT
  license) installing to `/app/share/soundfonts/`; engine searches the
  bundled file first (`BUNDLED_SOUNDFONT_NAME`). Decode proven pre-merge
  (FluidSynth 2.6.0 renders `test.mid` → 121,408 non-silent frames);
  in-sandbox presence/size/magic/license asserted by `smoke.sh`.
- Stale-path fixtures (`tests/fixtures/stale/` + `tests/stale_flows.rs`):
  dead-absolute config/`.cfg`/`.lst` load verbatim with no drops or panics
  — the shapes S3's re-prompt UX builds against.
- Version 1.8.0 end to end (`Cargo.toml`/`Cargo.lock`, about dialog pins,
  man page header, `ChangeLog`); README build section rewritten for
  cargo/Flatpak with the bundled-SF story.
- New/updated tests: `stale_flows.rs` (4), `desktop_integration.rs` (+3:
  retirement, release, soundfont bond — bond pins both font + license
  sha256, like `verify.sh`), `output_fluid.rs` search-path pin
  updated, version pins bumped. Each observed failing before its fix
  (stale pins passed on landing — they pin new fixtures over unchanged
  verbatim-load behavior; license pin red-probed with a bogus hash).

### S3 — stale-SF wiring + theming (landed: `efce8d2`)

- ux §15.1 (new spec) + DECISIONS D23: `.dash-value` per-mode ink (dark
  theme accent 10.45:1 AAA kept; light C++ `#62a0ea` 6.96:1 AA chosen;
  light accent 2.12:1 rejected) + `warning_text` D3-hint ink (theme
  warning base, exactly one surface).
- `theme.rs`: the S0-held diff landed against its spec (`dash_value` /
  `warning_text` + `dash_value_contrast_pins` + per-mode pin).
- Stale-SoundFont re-prompt (packaging §6 contract): `midi.rs`
  `is_stale_soundfont` probe + `stale_soundfont_hint`/`_toast` strings;
  `app.rs::startup_select_output` keeps the stale value verbatim, sends
  the engine `find_default_sound_font()` (in-sandbox: bundled SF),
  pushes one toast naming stale path + fallback + MIDI Setup; D3 status
  priority becomes test result > stale warning > refill hint > empty
  (`status_warning` VM flag, `warning_text` style); D11 Browse… stays the
  re-locate path (deferred commit, no auto-modal). Driver order unchanged.
- Tests: `stale_flows.rs` 4→5 (new `stale_soundfont_fixture_triggers_-
  reprompt_contract` bond over the S2 fixtures), `midi::stale_*` units,
  `builder_stale_status_priority`, `app.rs` startup pair; suite delta
  S2→S4: 551+10+102 → 559+10+103, 0 failed (S4-observed).
- S4 verified the landmarks in-tree (`midi.rs:150-167`,
  `theme.rs:97-108,328`, `app.rs:502-525`, `midi_setup.rs:194,323`).

### S4 — release sign-off (landed: `f738a9d`; no repo code touched)

Parity final pass: the §1 pins re-verified in the S4 `--fast` run (all 4
`check_pin` oks) over the green suite above — 23/23/30/6/toast-funnel
plus 9 accelerators hold at sign-off.

DE matrix (packaging §3.2; GUI clients run offscreen on nested
`weston --backend=headless`, socket `wayland-s4` — never on the live
desktop; paired `flatpak build` sandboxes, since install was §7-blocked
at the time):

| Row | Result |
|---|---|
| COSMIC Wayland | **PASS (offscreen).** RC GUI boots fresh-profile (no `~/.config/dmidiplayer`, no `~/.dmidiplayer`, no app config under `~/.var`), stays healthy 12 s+, zero panics, owns `com.goshapps.GoshMIDI` on the session bus, FluidSynth selects the PulseAudio driver in-sandbox; `WAYLAND_DEBUG` shows `wl_surface` commits + frame callbacks (render path proven at protocol level). Pixels uncapturable here (`weston-screenshooter` hangs, `grim` reports no capture protocol, no `weston-capture` tool) — visual confirm stays manual. |
| GNOME Wayland / X11, KDE Wayland / X11 | **ENVIRONMENT-IMPOSSIBLE** (command-output evidence, not a pass claim): `gnome-shell`, `plasmashell`, `kwin`, `mutter` all absent from `PATH`; `/usr/share/wayland-sessions` holds only `sway.desktop`, `/usr/share/xsessions` is empty. Automated halves that do run everywhere: smoke §1b `--help` under `GNOME`/`KDE`/unset DE markers (green in the S4 full run) + the portal-path integration tests. |
| X11 path (nested Xwayland `:99`, neither GNOME nor KDE) | **FAIL — needs real-DE adjudication, not counted as a release verdict.** Both the sandboxed RC and the unsandboxed `target/debug` binary panic identically in vendored `iced_tiny_skia`/`softbuffer` (`Visual 0x6d does not use softbuffer's pixel format`), with DRI and with `LIBGL_ALWAYS_SOFTWARE=1` — so it is not sandbox-caused; it is the CPU-fallback renderer vs this nested X server (or renderer selection generally). A real GNOME-X11/KDE-X11/Xorg smoke launch must adjudicate before release; if it reproduces there it is a blocker. |

Fresh-profile audible playback (zero host setup): **machine halves PASS,
human-audible stays manual.** (1) In-sandbox synth render, no host files:
sandbox `fluidsynth` 2.6.0 + `/app/share/soundfonts/FluidR3Mono_GM.sf3` +
`/app/share/dmidiplayer/test.mid` → 242,624 stereo frames @44.1 kHz,
404,581/485,248 nonzero samples, peak 1937 — non-silent GM audio from
bundled bytes only. (2) Live routing: the offscreen RC boot initialized
the in-sandbox PulseAudio driver against host pipewire-pulse
(`pipewire-0` + `pulse/native` sockets present, `--socket=pulseaudio` in
the manifest). No sound was emitted during S4 (shared machine, by design)
— a human must confirm audible playback on the RC (§7 recipe).

Second-run single-instance forward: **PASS (build-shell pair; literal
`flatpak run` form awaits the §7 slice).** With the RC owning the bus
name, a second sandboxed instance launched **displayless**
(`DISPLAY`/`WAYLAND_DISPLAY` unset) with a song arg exited 0 in 66 ms
and 82 ms across two runs (empty stderr) — the forward landed and the
sender exited before GUI init. Receipt traced: with `/songs` mounted in
both sandboxes, the receiver thread ran
`openat("/songs/test.mid", O_RDONLY)` after the forward. Replace-on-
receive is pinned by the green integration suite (incl. the portal-path
replacement test); window-present is not headless-observable → manual.

### S5 — Phase-3 blocker slice (landed: `85dd3c3`)

- [pack BLOCKER] D-Bus service file lands: new
  `com.goshapps.GoshMIDI.service` (`[D-BUS Service]
  Name=com.goshapps.GoshMIDI Exec=/app/bin/dmidiplayer`) installed by the
  manifest to `/app/share/dbus-1/services/`. Pre-fix repro on the S4 tree:
  `flatpak build-export` fails with the exact §7 error; post-fix the
  export commits clean
  (`Exporting share/dbus-1/services/com.goshapps.GoshMIDI.service`).
  Regression pins: `desktop_integration::dbus_service_file_-
  installed_for_activatable_desktop` (observed failing first), `verify.sh`
  `--fast` service pins, `smoke.sh` in-sandbox asserts, and a new
  full-gate `build-export` installability step. (Export logs "Not
  exporting … non-allowed export filename" for the T14c symbolic
  fallbacks — pre-existing and intended: they serve in-sandbox lookup
  only, asserted by smoke in `/app`.)
- [ux minor] W1 title fixed: `init` + `on_opened` now call
  `set_window_title` alongside `set_header_title` (`src/app.rs`;
  `on_opened` returns the title `Task`, propagated at the
  `PlayerEvent::Opened` arm). Pinned by
  `app::tests::opened_song_sets_header_and_window_title_with_file`
  (observed failing first: `""` vs expected) and protocol-verified live
  on nested headless weston (offscreen, fresh profile, song arg):
  `xdg_toplevel.set_title("Gosh MIDI Player")` at boot then
  `set_title("Gosh MIDI Player — test.mid")` on song load, zero panics.
- [pack minor] Man page ships: the manifest installs `docs/dmidiplayer.1`
  to `/app/share/man/man1/` (the retired `docs/CMakeLists.txt` installed
  it in both branches). Pinned by
  `desktop_integration::man_page_installed_to_man1` (observed failing
  first) + `verify.sh --fast` pin + `smoke.sh` in-sandbox assert.
- [release/lead carried] X11: still no verdict possible here —
  re-observed `gnome-shell`/`plasmashell`/`kwin`/`mutter` absent,
  `/usr/share/wayland-sessions` = `sway.desktop` only,
  `/usr/share/xsessions` empty. The nested-Xwayland softbuffer panic (§6
  S4) still awaits real-DE adjudication per §7 step 2.
- Suite delta S4→S5: 559+10+103 → 560+10+105, 0 failed.

### S6 — final REPORT slice (this commit; REPORT.md only)

- Re-ran both gates on the final tree (§5), executed the full
  install/bundle/install-from-bundle cycle (§4 — all green, cleaned up
  without trace), re-probed the DE environment (§0 — still no GNOME/KDE/
  Xorg), and wrote the project-DoD REPORT sections (§1–§4). No repo code
  touched; suite unchanged at 560+10+105, 0 failed.

## 7. Blockers + remaining manual steps

**Blocker — FIXED in S5 (was: S4 RC `flatpak build-export` fails: `error:
Desktop file D-Bus activatable, but service file not exported:
…/build-dir/export/share/dbus-1/services/com.goshapps.GoshMIDI.service`).**
Root cause as diagnosed in S4: the desktop file claims
`DBusActivatable=true` (P8, T14c) but nothing shipped the D-Bus service
file, and `verify.sh` exercised `flatpak build` only, never export. Fix
(§6 S5): the manifest installs
`share/dbus-1/services/com.goshapps.GoshMIDI.service`
(`[D-BUS Service] Name=com.goshapps.GoshMIDI Exec=/app/bin/dmidiplayer`,
flatpak rewrites `Exec` to the `flatpak run` wrapper at install) plus
three regression pins — `desktop_integration.rs` bond tests, `verify.sh`
`--fast` pins, `smoke.sh` in-sandbox asserts — and a `flatpak
build-export` installability step in the full gate so this can never
regress silently. Install/bundle/activation are unblocked (export commits
clean, §5; install + bundle + install-from-bundle + installed-app
headless run all executed green in S6).

Remaining manual steps (recipes in packaging.md §3.2 + §14-phase-3 notes;
all need a human on consoles S4–S6 could not reach):

1. Fresh-profile **audible** playback on the installed RC: new user (or
   empty `~/.var/app/com.goshapps.GoshMIDI`), launch, press Play on a
   bundled example — expect GM audio with zero host setup. (Machine
   halves already pass — §6 S4.)
2. On-console DE runs: COSMIC visual pass (icons incl. `channel-secure`
   + CSD controls, §15.1 light/dark dash values per the ux §14 recipe),
   then GNOME Wayland+X11 and KDE Wayland+X11 (launch + render + chooser
   + MIME + clipboard paste-out + second-`flatpak run` replace + present
   + `COSMIC_SINGLE_INSTANCE=false` escape). The X11 rows also adjudicate
   the nested-Xwayland softbuffer panic (§6 S4).
3. Stale-path re-prompt click-through (packaging §6): config with a dead
   absolute `soundFont` → toast + bundled fallback + D3 warning + D11
   re-locate; dead playlist/`.cfg` paths → re-prompt offers, nothing
   silently dropped.
4. (Old step 3 install/bundle half — CLOSED in S6: export + install +
   bundle + install-from-bundle + installed-app headless run all executed
   green. Re-run on the RC host only if its flatpak/runtime versions
   differ.)
