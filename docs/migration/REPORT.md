# Migration REPORT — Gosh MIDI Player 1.8.0 (libcosmic/Rust)

Status: **S4 sign-off recorded (2026-09-12)** — CONDITIONAL: the automated
gates are green and the live forward/synth probes pass, but the release is
**not shippable** until the §4 blocker slice lands (`flatpak build-export`
fails: no D-Bus service file for the `DBusActivatable=true` desktop entry —
no install, no bundle, no activation). X11 needs real-DE adjudication (§3
S4), and the §4 manual steps (human-audible, on-console DE runs) remain.

## 0. Release + environment

- Version 1.8.0, 2026-09-12 — first libcosmic/Rust release (GTK4/C++ retired).
- Branch `cosmic-migration`; app ID `com.goshapps.GoshMIDI`; command `dmidiplayer`.
- Toolchain: rustc 1.98.1 (`rust-toolchain.toml`), libcosmic rev `7e82198`
  (D16), freedesktop runtime/SDK 25.08, FluidSynth 2.6.0 (flatpak module).
- Bundled SoundFont: `FluidR3Mono_GM.sf3` v2.312, MIT, 14,563,174 bytes
  (LibreScore/sf3 @ `3a6eda8`, sha256 `cfcd66d8…2794ec2` — S2 §3).
- S4 probe host: NixOS + COSMIC (`cosmic-comp` live, `xdg-desktop-portal`
  1.20.4 + `portal-cosmic` 1.2.0 + `portal-gtk` 1.15.3, pipewire 1.6.6 +
  pipewire-pulse), flatpak 1.18.2 + flatpak-builder, freedesktop 25.08
  runtime installed; probes ran from a distrobox container sharing the
  session bus, with GUI clients on a nested `weston --backend=headless`
  compositor (never on the live desktop).

## 1. Parity counts (PLAN §1/U1 — all pinned)

| Count | Value | Pins |
|---|---|---|
| Actions | 23 | `MenuAction::ALL: [MenuAction; 23]` + unit `inventory_is_23_entries_in_cpp_section_order` + `keys_flows::menu_maps_all_23_entries_to_documented_messages` |
| Menu entries | 23 | same inventory (actions+menu unified in Rust) |
| Settings keys | 30 | `KNOWN_KEYS: [&str; 30]` + unit `KNOWN_KEYS.len() == 30` |
| Toast trigger sites | 6 | `Message::Toast` funnel pin + per-path tests (`open`/`playlist`/`lyrics`/`dialogs`/`keys` flows + `app.rs` error→toast units) |
| Accelerators | 9 | `ACCELERATORS: [Accelerator; 9]` + unit + `keys_flows` |

T15 retired the C++ greps; the Rust successors above are enforced by both
`cargo test` and the `verify.sh` count-pins step (S2, re-verified S4).

## 2. Gates

- `sh scripts/verify.sh --fast`: **PASS** (S4, 2026-09-12 — 559 lib +
  10 bin + 103 integration-file tests, 0 failed, +1 ignored doctest;
  retirement gate, all 4 count pins, identity/desktop/icon/soundfont
  contracts ok; `desktop-file-validate` + `appstreamcli validate`
  strict-ok, sole note the grandfathered `cid-contains-uppercase-letter`).
- `sh scripts/verify.sh` (full: vendoring freshness + flatpak-builder +
  in-sandbox smoke): **PASS** (S4, 2026-09-12 — exactly one full run;
  freshness diff clean, builder exit 0, smoke: P6 10/10 examples resolve,
  bundled SoundFont 14,563,174 bytes + sfbk magic + MIT license,
  `--help`/DE-marker/`--dry-run`(10)/`--dump-info`(10)/missing-file all
  green, zero panics).
- S4 release gate (DE matrix, fresh-profile audible playback, second-run
  forward): **PARTIAL — see §3 S4** (live Wayland launch + forward +
  in-sandbox synth render pass; install/activation blocked on the §4
  service-file slice; GNOME/KDE + human-audible remain manual).

## 3. Slice inputs

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
- Count pins replaced with Rust successors (table above); toast sites pin
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

### S4 — release sign-off (this slice; no repo code touched)

Parity final pass: the §1 pins re-verified in the S4 `--fast` run (all 4
`check_pin` oks) over the green suite above — 23/23/30/6/toast-funnel
plus 9 accelerators hold at sign-off.

DE matrix (packaging §3.2; GUI clients run offscreen on nested
`weston --backend=headless`, socket `wayland-s4` — never on the live
desktop; paired `flatpak build` sandboxes, since install is §4-blocked):

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
— a human must confirm audible playback on the RC (§4 recipe).

Second-run single-instance forward: **PASS (build-shell pair; literal
`flatpak run` form awaits the §4 slice).** With the RC owning the bus
name, a second sandboxed instance launched **displayless**
(`DISPLAY`/`WAYLAND_DISPLAY` unset) with a song arg exited 0 in 66 ms
and 82 ms across two runs (empty stderr) — the forward landed and the
sender exited before GUI init. Receipt traced: with `/songs` mounted in
both sandboxes, the receiver thread ran
`openat("/songs/test.mid", O_RDONLY)` after the forward. Replace-on-
receive is pinned by the green integration suite (incl. the portal-path
replacement test); window-present is not headless-observable → manual.

## 4. Blockers + remaining manual steps

**Blocker (own slice + full gate, NOT fixed in S4 — code freeze held).**
`flatpak build-export` on the RC fails: `error: Desktop file D-Bus
activatable, but service file not exported:
…/build-dir/export/share/dbus-1/services/com.goshapps.GoshMIDI.service`.
The desktop file claims `DBusActivatable=true` (P8, T14c) but nothing
ships the D-Bus service file — so the app can neither be installed nor
bundled nor D-Bus-activated. First caught by S4 because `verify.sh`
exercises `flatpak build` only, never export/install. Fix shape: the
manifest installs `share/dbus-1/services/com.goshapps.GoshMIDI.service`
(`[D-BUS Service] Name=… Exec=/app/bin/dmidiplayer`), plus an
export/install assertion (smoke or a `build-export` step in the full
gate) so this can never regress silently.

Remaining manual steps (recipes in packaging.md §3.2 + §14-phase-3 notes;
all need a human on consoles S4 could not reach):

1. Fresh-profile **audible** playback on the installed RC: new user (or
   empty `~/.var/app/com.goshapps.GoshMIDI`), launch, press Play on a
   bundled example — expect GM audio with zero host setup. (Machine
   halves already pass — §3 S4.)
2. On-console DE runs: COSMIC visual pass (icons incl. `channel-secure`
   + CSD controls, §15.1 light/dark dash values per the ux §14 recipe),
   then GNOME Wayland+X11 and KDE Wayland+X11 (launch + render + chooser
   + MIME + clipboard paste-out + second-`flatpak run` replace + present
   + `COSMIC_SINGLE_INSTANCE=false` escape). The X11 rows also adjudicate
   the nested-Xwayland softbuffer panic (§3 S4).
3. `.flatpak` bundle build + install from bundle on the §3.2 provider
   matrix (blocked on the service-file slice above).
4. Stale-path re-prompt click-through (packaging §6): config with a dead
   absolute `soundFont` → toast + bundled fallback + D3 warning + D11
   re-locate; dead playlist/`.cfg` paths → re-prompt offers, nothing
   silently dropped.
