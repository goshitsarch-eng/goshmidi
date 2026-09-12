# Packaging & QA — libcosmic Migration

Owner: Packaging/QA teammate. Phase 1: planning only, no source changes.
Applies to branch `cosmic-migration`, app ID `com.goshapps.GoshMIDI`,
binary `dmidiplayer` (decided, P3 / DECISIONS D3 — no rename; desktop
`Exec=dmidiplayer %F` stays as-is)
(C++17 GTK4/libadwaita → Rust libcosmic).

Grounded in current tree: `CMakeLists.txt` (project `dmidiplayer` 1.7.5,
`GOSH_APP_ID`), `src/main.cpp` (`on_open`, `G_APPLICATION_HANDLES_OPEN`),
`src/ui/window.cpp` (`openFiles` replace semantics, slider ranges,
song-settings schema), `src/midi/sequence.cpp` (`setTempoFactor` reject),
`src/midi/player.cpp` (unclamped setters), `tests/` (`test_sequence.cpp`,
`check_identity.cmake`, `check_single_instance_open.cmake`),
`com.goshapps.GoshMIDI.desktop`, `com.goshapps.GoshMIDI.metainfo.xml`,
`icons/CMakeLists.txt`, `examples/` + `examples/CMakeLists.txt`
(`examples.lst` generation/install), `data/dmidiplayer.gresource.xml`
(legacy prefix), `.github/workflows/linux-build.yml` (triggers), README
build deps.

## 0. QA gates (tiered, P1 — user's DoD kept literally)

- **Inner loop** `scripts/verify.sh --fast`: `cargo build --locked`,
  `cargo clippy --locked -- -D warnings`, `cargo fmt --check`,
  `cargo test --locked`, plus the CMake-retirement gate, the Rust count
  pins, and the identity/desktop/icon/soundfont contract checks (T15
  end-state — the CMake tier is retired, D14/P9).
- **Pre-commit gate** (every task from T1 on, no waivers): full
  `verify.sh` — fast tier plus `flatpak-builder` + in-sandbox
  `scripts/smoke.sh`. Incremental rebuilds keep this cheap after T1;
  the first-build cost is paid once.
- **Release gate** (T15 + every RC): DE matrix (§3.2/P12), fresh-profile
  audible playback with zero host setup, second-`flatpak run`
  single-instance forward (§1.5/P8).
- No task broke CMake CI until T15 (P9). `check_single_instance_open.cmake`
  retired in T10 only **after** its Rust successor test was green
  (sequenced retirement, P1) — there was never a commit with neither
  check. T15 retired the remaining C++ build the same way: every
  CMake-dependent check has a Rust/`verify.sh` successor (D14).

## 1. Flatpak design

### 1.1 Manifest skeleton

New file (Phase 2+): `flatpak/com.goshapps.GoshMIDI.yml`.

```yaml
app-id: com.goshapps.GoshMIDI
runtime: org.freedesktop.Platform
runtime-version: "25.08"
sdk: org.freedesktop.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: dmidiplayer   # decided (P3); desktop Exec already matches, no change
finish-args:
  - --share=ipc
  - --socket=fallback-x11
  - --socket=wayland
  - --device=dri
  - --socket=pulseaudio
  - --device=all            # ALSA sequencer (/dev/snd/seq); see §1.4 and §5
  # No --filesystem=host: file access goes through portals (§1.5).
modules:
  - shared-modules/gl.json  # if needed, Flathub shared-modules style include
  - fluidsynth.json         # build from release tarball if not in SDK/runtime
  - soundfont               # T15: bundled GM SoundFont .sf3 (§5 risk 2, §6)
  - cargo-sources.json      # generated, committed (see §1.3)
  - dmidiplayer.json        # cargo --offline --locked build of this repo
```

### 1.2 Runtime choice: org.freedesktop vs COSMIC

- **Default: `org.freedesktop.Platform` + SDK `25.08`.** Available today on
  Flathub and everywhere; libcosmic apps build and run fine on it (COSMIC
  apps are Wayland clients with no hard dependency on a COSMIC host).
  T0 pack-spike correction: `24.08` ships rustc 1.89, which is below the
  libcosmic MSRV 1.93 (DECISIONS D16); `25.08` ships rustc 1.98.1
  (matching `rust-toolchain.toml`; T0 `rust-ext-25.08.json` evidence pins
  rust 1.98.1 for branch 25.08 — exact SDK point release re-confirmed
  in T1).
- **COSMIC runtime (`org.cosmic.Platform`, if published):** prefer only once
  it is installable from Flathub alongside the freedesktop runtime, and only
  if it demonstrably shrinks the bundle or fixes theming. It must never be
  the *only* supported path, because QA must verify non-COSMIC desktops (§4).
- Decision rule: ship freedesktop runtime; re-evaluate COSMIC runtime when
  it exists and CI can install it. Record the decision and runtime-version
  bump in this doc.

### 1.3 Rust SDK extension and cargo vendoring (incl. git deps)

- Toolchain: `org.freedesktop.Sdk.Extension.rust-stable`. The exact toolchain
  and MSRV are **pinned in T0** (P5) and recorded in DECISIONS; the manifest
  records the effective `rustc --version` expectation and CI logs the actual
  `rustc --version` for reproducibility. Set
  `PATH=/usr/lib/sdk/rust-stable/bin:$PATH` in the cargo build module.
- Every cargo invocation uses `--locked` from the first Rust task on (T1,
  P5). `Cargo.lock` is committed; libcosmic rev and every crate version are
  pinned in T0 and recorded in DECISIONS.
- Crate pins (T0, full table in DECISIONS D16; re-verified 2026-09-12
  via the crates.io API **with a `User-Agent` header** — bare curl
  returns empty, and the two T0 `/tmp` fetch artifacts
  (`alsa-Cargo.toml`, `file_chooser.rs`) contain only `404: Not Found`
  from headerless fetches): libcosmic git rev `7e82198` + pop-os/iced
  submodule `87e1787` (features `about`, `single-instance`,
  `xdg-portal`); `alsa 0.12.1` (2026-07-31), `chardetng 1.0.0`
  (2026-03-30, rust 1.40), `encoding_rs 0.8.41` (2026-09-09, rust
  1.88), `url 2.5.8` (2026-01-05), `tempfile 3.27.0` (2026-03-11,
  dev); MSRV `rust-version = "1.93"`. `fluidsynth` crate **rejected
  as abandonware** (verified: single release `0.0.1`, 2018-08-26)
  → hand-written FluidSynth FFI against `libfluidsynth-dev`
  (bindgen-free, no libclang). No direct rfd/zbus/tokio/dirs/serde.
- flatpak-builder runs the build **offline**, so all crates — including
  **git dependencies** — must be vendored:
  1. `cargo vendor vendor/` (plain crates) or, preferred for Flatpak,
     the **vendored + pinned** `flatpak/flatpak-cargo-generator.py`
     (committed copy of upstream
     `flatpak-builder-tools/cargo/flatpak-cargo-generator.py` at the rev
     recorded in DECISIONS, P4c) run against `Cargo.lock`, producing
     `cargo-sources.json`. **No bare `flatpak-cargo-generator(.py)` from
     PATH** — freshness and reproducibility require the pinned copy.
  2. The generator handles git deps by recording the checkout as
     an archive source; plain `cargo vendor` also materializes git checkouts
     into `vendor/` plus a `.cargo/config.toml` `source` replacement. Either
     way, **git deps are allowed but must resolve to a pinned rev in
     `Cargo.lock`** — no floating branches.
  3. Commit the generated `cargo-sources.json` (and `Cargo.lock`) to the
     repo. The cargo module builds with
     `cargo build --offline --locked --release`.
  4. CI enforces freshness: regenerate into a temp dir with the **vendored**
     script and `diff` against the committed file; fail on drift (see §2.4).
- Regeneration command (documented for maintainers, run on dependency change):
  `python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o
  flatpak/cargo-sources.json`. Generator is pinned at upstream rev
  `1fc32195e3e60fe5c97f0af646dec7a99df5962b`, vendored at `flatpak/`
  (DECISIONS D16).
- **Never `--git-tarballs`** — codeload tarballs exclude submodules and
  would break the libcosmic → pop-os/iced dep chain. Rationale is from
  code reading only (generator `fetch_git_repo` + flatpak-builder
  `builder-git.c` submodule handling); unproven end-to-end until T1 runs
  the generator against a submodule-bearing dep. Use the default
  archive-source mode.
- T0 pack-spike probe evidence: vendoring **mechanism proven on a
  synthetic git+submodule pair only** — toy `cfg-if` https git dep plus
  a local `file://` outer/inner submodule fixture, `cargo vendor` →
  `--offline --locked` with clean `CARGO_HOME` (PASS). The pinned
  libcosmic rev + iced submodule were never vendored or offline-checked
  in T0, and the generator never ran against a submodule-bearing dep;
  real-rev vendoring happens in T1.

### 1.4 Minimal finish-args (audio)

| Arg | Why |
|---|---|
| `--share=ipc` | Required alongside X11/Wayland sockets. |
| `--socket=fallback-x11`, `--socket=wayland` | Display; Wayland-first, X11 fallback for non-COSMIC hosts. |
| `--device=dri` | GPU rendering. |
| `--socket=pulseaudio` | Primary audio path: FluidSynth → PulseAudio/PipeWire. |
| `--device=all` | ALSA sequencer access (`/dev/snd/seq`) for external synths (MT-32/Munt, SC-55, USB MIDI). Narrow if Flatpak ever offers a seq-only device rule. |

Deliberately absent: `--filesystem=*` (portals instead), `--share=network`
(no network need), `--talk-name=org.freedesktop.Flatpak` (no sub-sandbox
spawning). Audio risks: see §5.

### 1.5 Portals for file open + single instance (P2, P8)

- Keep the desktop entry's `Exec=dmidiplayer %F` and the
  `MimeType=audio/midi; audio/x-midi;audio/cakewalk;application/x-riff;`
  lines; Flatpak forwards opened files via document-portal paths.
- In-app picking uses the portal file chooser (libcosmic dialogs route
  through portals under Flatpak automatically; provider matrix in §3.2/P12).
  Never add host-filesystem permissions to work around picker issues — fix
  the portal path instead.
- **Second-Open is REPLACE, not append (P2).** Verified in source:
  `src/main.cpp:48` calls `win->openFiles(paths, true)`, and
  `MainWindow::openFiles` (`src/ui/window.cpp:351-370`) clears the playlist
  when `replacePlaylist` is set, then selects the first new song. The Rust
  port must preserve this observable behavior: a second launch with file
  paths **replaces** the playlist in the running instance. The QA equivalent
  is an integration test that delivers a second open request and asserts the
  playlist was replaced (size equals the new set, first song selected) —
  including a portal-delivered `/run/user/$UID/doc/...` path copy of
  `test.mid` (portal-path replacement test, P2).
- **D-Bus activation + bus name (P8).** The desktop file gains
  `DBusActivatable=true` (**landed T14c** — the only desktop delta vs the
  C++-shipped file); the app owns the well-known bus name
  `com.goshapps.GoshMIDI` (which matches the desktop basename, as required
  for activatability) via `run_single_instance` (DECISIONS D12). A second
  launch — including `flatpak run com.goshapps.GoshMIDI file.mid` while the
  app is already running — forwards its paths over D-Bus and presents the
  running window. Verification: integration test for forward + present, plus
  an RC step running a second `flatpak run` against a live instance and
  asserting replace + present (§3.2).
- **T14c single-instance evidence (A4, incl. `flatpak run` case).**
  Source-verified in the pinned libcosmic rev: first instance serves
  `/com/goshapps/GoshMIDI` and claims `APP_ID` (`dbus_activation.rs:41`);
  second instance proxies `destination(APP_ID)` and calls
  `activate_action("open", args)` / `activate()` (`app/mod.rs:195-258`),
  exiting before GUI init when the call lands. `APP_ID` is
  `com.goshapps.GoshMIDI` (`src/app.rs`, `const APP_ID`), equal to the
  desktop basename — pinned by
  `tests/desktop_integration.rs::single_instance_bus_name_matches_desktop_basename`,
  and the sending half (`action() == "open"` + argv) by
  `...::single_instance_flags_forward_files_as_open_action`. Sandbox bus
  policy probed live (2026-09-12): a sandboxed app CAN own its own app-ID
  name (`RequestName` → `PRIMARY_OWNER`) and CANNOT own a foreign name —
  so **no `--own-name` finish-arg is needed**. Wayland `application_id`
  and `iced.id` are both `APP_ID` (`app/mod.rs:67-70`), matching
  `StartupWMClass`. Remaining live half (needs a display, Phase-3 recipe
  in §3.2): second `flatpak run --file-forwarding … @@ file.mid @@`
  against a running instance → replace + present; `--file-forwarding` is
  required because bare host paths are invisible in both sandboxes, and
  the forwarded document-portal path is valid in the first sandbox only
  because both instances share the per-app document store.
- Portal-delivered paths (`/run/user/.../doc/...`) must be accepted anywhere
  a path is accepted (open flows, playlist entries, SoundFont picker).

### 1.6 Desktop / metainfo / icons reuse

Reuse as-is; the app ID is stable and tested:

- `com.goshapps.GoshMIDI.desktop`: keep `Name`, `Icon=com.goshapps.GoshMIDI`,
  `StartupWMClass=com.goshapps.GoshMIDI`, `MimeType`, `Exec=dmidiplayer %F`
  (already correct — binary name decided, P3, so no `Exec` change). The only
  desktop change in the migration is adding `DBusActivatable=true` (P8,
  **landed T14c**). Pinned line-by-line by
  `tests/desktop_integration.rs::desktop_file_is_cpp_shipped_plus_dbus_activatable`
  (15 lines, exact) plus `verify.sh` field greps and
  `desktop-file-validate` when installed.
- `com.goshapps.GoshMIDI.metainfo.xml` — scope is exactly these three
  (P11, Phase 2+):
  1. Update `<description>` toolkit wording (GTK4/libadwaita → libcosmic/Rust)
     — **landed T14c**.
  2. **Rhythm-claim fix:** line 18 advertised "Piano, MIDI Channels, Rhythm,
     and Lyrics (Karaoke) views", but the code implements rhythm as dashboard
     lamps, not a switcher page (DECISIONS D5) — **landed T14c** as
     "Piano, MIDI Channels, and Lyrics (Karaoke) views, plus dashboard
     rhythm lamps that follow the meter". `README.md:38` carried the same
     doc bug (ux.md §3.5) and got the same fix.
  3. Add a `<release>` entry at the first libcosmic release — **T15's**.
  Out of scope: screenshot refresh, new `content_rating`/categories — defer.
  Keep `<id>`, license, existing screenshots/releases otherwise.
  `appstreamcli validate` passes (2026-09-12, appstream 1.1.3; sole note is
  the pedantic grandfathered `cid-contains-uppercase-letter` — the app ID is
  decided, P3/D3); `verify.sh` runs it strictly when installed.
- `icons/`: the `dmidiplayer_*` sources install renamed to the app ID
  under `icons/hicolor/<size>/apps` (+ scalable `.svgz`) — the retired
  `icons/CMakeLists.txt` pattern, now performed by the Flatpak module.
  The `check_identity.cmake` successor in `scripts/verify.sh` asserts the
  canonical ID present, legacy `net.sourceforge.dmidiplayer` absent, and
  installed icon filenames — without depending on CMake (P9, §2.4).
- **T14c symbolic-icon hicolor fallback** (`icons/fallback/`, 29 SVGs).
  Why: icon lookup (`cosmic-freedesktop-icons`, pinned via libcosmic)
  searches requested-theme → Inherits → `Cosmic` → `hicolor` → `gnome` →
  `Yaru` → pixmaps, and the in-sandbox default theme is always `Cosmic`
  (fresh sandboxed cosmic-config). The freedesktop runtime ships only
  `hicolor`, and real `flatpak run` mounts host icons at
  `/run/host/share/icons` — so on any host without a `Cosmic`/`Pop` theme
  (GNOME, KDE, minimal COSMIC) every `*-symbolic` lookup previously fell
  through to an empty-SVG render (invisible buttons). Verified matrix:
  `flatpak build` shell (no host mounts) resolves nothing; host Adwaita
  and BaseApp `Cosmic`/`Pop` cover all names when mounted, except
  `channel-secure-symbolic`, which only `Pop` (and Adwaita) ship — the
  `ux.md §8`-predicted fallback-review case. Fix: vendor the 29 looked-up
  names (20 app `from_name` literals + 9 libcosmic-widget-embedded:
  dropdown `pan-down`, segmented `go-next/go-previous/window-close`,
  menu `object-select/pan-end`, CSD `window-minimize/maximize/restore/close`)
  from pinned upstreams (28 × `pop-os/cosmic-icons@343c007`,
  1 × `pop-os/icon-theme@1a575a8` for `channel-secure`; all CC-BY-SA-4.0,
  see `icons/fallback/SOURCES.md` for revs, hashes, attribution) and
  install them to `hicolor/scalable/{actions,status}/`, which the runtime
  hicolor `index.theme` lists — so the borrowed-index mechanism resolves
  them without shipping an index. Themed hosts are unaffected (requested
  theme is searched first). Self-maintaining: `verify.sh` fails if any
  `src/` `*-symbolic` literal lacks a fallback file, and `smoke.sh`
  asserts the installed set in-sandbox.

### 1.7 Examples install (P6)

- `examples/CMakeLists.txt` **generates** `examples.lst` at build time (from
  the shipped `*.mid`/`*.kar`) and **installs** the example files + `.lst`
  under `<datadir>/dmidiplayer`. The Flatpak module must replicate both:
  generate `examples.lst` during the build and install it with the example
  files to `/app/share/dmidiplayer/` — this is the first entry of the F1
  first-launch fallback chain (arch §4.1).
- In-sandbox assertion (`scripts/smoke.sh`): the installed `examples.lst`
  exists, every listed file exists beside it, and the pinned fixtures
  (`twinkle.kar`, `test.mid`, `Negra_Sombra.kar`) parse headlessly.

## 2. Test plan

### 2.1 Unit tests (Rust, `cargo test`)

State/Message update tests for the Elm core — no GUI, no audio device:

- `update()` transitions: Play/Pause/Stop/Next/Prev, Seek/BarJump,
  Shuffle/Repeat/AutoAdvance toggles.
- **Range semantics — phantom clamps fixed (P10a).** Verified core truth:
  `Sequence::setTempoFactor` **rejects** factors outside 0.1–10.0
  (ignores the call, no clamp — `src/midi/sequence.cpp:245-251`), and
  `setPitchShift`/`setVolumeFactor` (`src/midi/player.cpp:161-171`) apply
  unclamped. The UI ranges — pitch spin -12..+12, tempo scale 50–200,
  volume scale 0–200 (`src/ui/window.cpp:1669-1707`) — are enforced at the
  widget boundary only. Rust `update()` tests must assert the same boundary
  semantics: an out-of-range tempo factor leaves state unchanged
  (validate-and-ignore, A7 — never a silent clamp); pitch/volume messages
  carry widget-validated values. No `update()` test may assert a clamp the
  C++ core does not perform.
- Reducer parity with today's `test_sequence.cpp` assertions, ported to Rust
  fixtures in `examples/`: load `twinkle.kar` (events, ticks, bars, lyrics),
  `test.mid` (division, channel usage, beats, bar jump),
  `Negra_Sombra.kar` (karaoke text); playlist add/select/shuffle/save/load
  round-trip; supported-extension filter (`MID` case-insensitive, `WRK`,
  `RMI` in; `WAV` out); SysEx GM/MT-32 reset framing + settle time;
  concatenated-SysEx parse; companion `.syx` discovery + load; instrument
  map names and `inferDeviceProfile` cases (Munt/MT-32 incl. GM-Emulation
  exception, SC-55, Nuked Virtual SC55, RT-55).
- **Missing-case sweep (P10b):** song-settings save→load round-trip (Global +
  per-channel groups, auto on/off triggers); GKeyFile escaping, corrupt-file
  keeps defaults, unknown keys kept, dir creation (A11); portal-path playlist
  entries (save/load/forward with `/run/.../doc/...` paths); U10 lyric
  static+live concatenation **preserved incl. duplication quirk**; loop ×
  repeat interplay (loop bars × repeat song/list/off, end-of-song policy);
  A2 sparse-fixture latency (command polling + wakeup, stop-during-long-sleep).
- Malformed/edge inputs: empty file, truncated SMF, unknown extension,
  missing examples dir → clean `Err`, no panic.

### 2.2 Integration tests (messages/state flows)

Drive the app through public messages/state, not internals:

- Open-file flow: `Open(paths)` → playlist populated → first song selected →
  state `Stopped→Playing` on Play; second `Open` while running **replaces**
  the playlist per the single-instance contract (§1.5/P2 — never appends),
  including a portal-style `/run/user/$UID/doc/...` path copy of `test.mid`
  (portal-path replacement test).
- Playlist flow: add `examples/*.mid *.kar`, reorder, shuffle (size stable),
  save → load round-trip equals, next/prev at ends with repeat on/off,
  auto-advance on song end.
- Settings flow: select ALSA port / FluidSynth + SoundFont path → persisted
  config → reload applies; invalid SoundFont path → error state, previous
  backend kept.
- Loop + repeat flow: loop bars set × repeat song/list/off → end-of-song and
  loop re-jump behavior matches §2.1 semantics.
- No-audio-device runs: all of the above pass with a dummy/null MIDI sink
  and no ALSA hardware (CI has no sound card).

### 2.3 Headless smoke script + `--dry-run`/`--dump-info` contract (P10c)

`scripts/smoke.sh` (new in Phase 2): runs the built binary with
`--help`, loads each `examples/*.mid|*.kar` headlessly, asserts exit 0
and expected stdout markers, and fails on stderr
`panic`/`thread .* panicked`. Must work with no display (unset
`DISPLAY`/`WAYLAND_DISPLAY` — no `xvfb-run` needed for the headless flags)
and inside `flatpak run` with no display. Also performs the P6 in-sandbox
examples assertion (§1.7).

`main.rs` headless contract (both flags **exit before any GUI/toolkit
initialization**, so they never touch a display or GPU):

- `dmidiplayer --dry-run <files...>`: parse + decode each file, print one
  marker line per file (`file`, `events`, `ticks`, `duration`, `division`,
  `bars`), exit 0. Any unloadable file → `error: <file>: <reason>` line and
  non-zero exit, no panic.
- `dmidiplayer --dump-info <file>`: print the full marker set for one file
  (same fields, single file), exit 0; same error contract on failure.
- `smoke.sh` asserts: exit codes, marker presence per example file, error
  contract on a corrupt fixture, and zero `panic` on stderr. T14c additions:
  `--help` under `XDG_CURRENT_DESKTOP=GNOME` / `=KDE` / unset (headless boot
  must not depend on a COSMIC session — §3.2 inputs), and, inside the sandbox
  only, the installed desktop (`DBusActivatable=true`), metainfo (rhythm fix),
  all six hicolor app-icon sizes, and the fallback inventory (exact when the
  `/repo` bind-mount is visible, else count ≥ 29 + spot names).

### 2.4 `scripts/verify.sh` (tiered QA gate, P1 + P9)

One script, zero arguments for the full gate, `--fast` for the inner loop;
exit non-zero on any failure:

```sh
#!/bin/sh
set -eu
# --- fast tier (also runs as the first half of the full gate) ---
cargo build --locked
cargo clippy --locked -- -D warnings
cargo fmt --check
cargo test --locked
# CMake/C++ retirement gate (P9/D14, T15): every path below is gone.
for retired in CMakeLists.txt cmake docs/CMakeLists.txt \
    examples/CMakeLists.txt icons/CMakeLists.txt \
    tests/check_identity.cmake tests/test_sequence.cpp \
    data/dmidiplayer.gresource.xml src/main.cpp src/midi \
    src/ui/window.cpp src/ui/window.hpp src/app/settings.cpp \
    src/app/settings.hpp src/app/playlist.cpp src/app/playlist.hpp \
    src/app/instruments.hpp .github/workflows/linux-build.yml; do
    [ ! -e "$retired" ]  # fail on resurrection
done
# Count pins, Rust successors (PLAN §1/U1):
grep -q 'pub const ALL: \[MenuAction; 23\]' src/ui/menu.rs      # 23 actions+menu
grep -q 'const KNOWN_KEYS: \[&str; 30\]' src/config/settings.rs # 30 keys
grep -q 'pub const ACCELERATORS: \[Accelerator; 9\]' src/state/keys.rs
grep -q 'Message::Toast(text) => self.toast(text)' src/app.rs   # toast funnel
[ "${1:-}" = "--fast" ] && exit 0
# Identity/metadata contract (successor of check_identity.cmake):
# NOTE: no CMake-dependent greps (P9) — check shipped files, not build inputs.
grep -q 'com.goshapps.GoshMIDI' com.goshapps.GoshMIDI.desktop \
  com.goshapps.GoshMIDI.metainfo.xml
! grep -q 'net.sourceforge.dmidiplayer' com.goshapps.GoshMIDI.desktop \
  com.goshapps.GoshMIDI.metainfo.xml
# Installed-icon check reads the install manifest / datadir, never the
# retired icons/CMakeLists.txt. The legacy gresource prefix
# ("/net/sourceforge/dmidiplayer") is resolved by removal with its file.
# Vendoring freshness (vendored + pinned generator, P4c):
python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o /tmp/cargo-sources.check.json
diff -u flatpak/cargo-sources.json /tmp/cargo-sources.check.json
# Flatpak build + in-sandbox smoke (pre-commit gate per user DoD, P1):
flatpak-builder --force-clean --user build-dir flatpak/com.goshapps.GoshMIDI.yml
flatpak build --bind-mount=/repo=$PWD build-dir sh /repo/scripts/smoke.sh
```

`check_single_instance_open.cmake` ran in the CMake tier until T10 retired
it after the Rust successor (forward + replace + present) went green; the
file itself is deleted with the T15 retirement.
T14c additions to the `--fast` tier: the desktop-integration contract
(`DBusActivatable` + compat-field greps, metainfo wording pins), the
symbolic-icon fallback inventory (every `src/` literal resolves to a file in
`icons/fallback/`), and the freedesktop validators — `desktop-file-validate`
and `appstreamcli validate` run strictly when installed, loud-SKIP otherwise
(CI images without them rely on `tests/desktop_integration.rs` instead).
T15 additions: the retirement gate + Rust count pins above, the metainfo
`<release>` pin (shipped `Cargo.toml` version must have an entry), the
bundled-SoundFont manifest pin (code-side `BUNDLED_SOUNDFONT_NAME` installed
from sha256-pinned sources — font + MIT license, both pinned in `verify.sh`
and `desktop_integration.rs`), and the in-sandbox SoundFont presence/magic
assertion in `smoke.sh`.

## 3. CI

### 3.1 Workflow (`cosmic-build.yml`; the CMake job is retired)

The CMake job (`linux-build.yml`) ran untouched during migration next to
the parallel `cosmic` job; T15 deleted it with the C++ retirement, so
`cosmic-build.yml` is the whole CI:

1. **Triggers (P4a).** `cosmic-migration` is listed on `push`/`pull_request`
   alongside `devel` and `master` (verified) — keep `devel` for the later
   merge.
2. `actions/checkout`, install Rust stable pinned per T0/P5
   (`dtolnay/rust-toolchain` at the pinned toolchain + clippy).
3. Install system deps (P4b). The `cosmic`
   job needs this full enumerated Debian set (**preliminary — derived,
   not build-logged; finalizes when the T1 CI install goes green**):
   `pkg-config cmake g++ libasound2-dev libfluidsynth-dev libwayland-dev
   libxkbcommon-dev libfontconfig-dev libfreetype-dev libexpat1-dev` plus
   generator tooling `python3 python3-aiohttp python3-tomlkit`. No
   `libuchardet-dev` (the Rust port uses chardetng/encoding_rs). No
   `libvulkan-dev`/`libgl1-mesa-dev`/X11 headers — no wgpu initially. No
   `libclang` (bindgen-free: hand-written FluidSynth FFI, D16).
   Derivation sources: libcosmic README "Dependencies" section
   (`https://github.com/pop-os/libcosmic` — `cargo cmake just
   libexpat1-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev
   pkgconf`) + libcosmic CI workflow
   (`https://github.com/pop-os/libcosmic/tree/master/.github/workflows`
   — `libxkbcommon-dev libwayland-dev`) + `alsa-sys` pkg-config need
   (`libasound2-dev`) + FluidSynth headers (`libfluidsynth-dev`).
   No apt/dpkg or clean-room build log exists for the exact set yet.
   NOTE: package names are `libfontconfig-dev`/`libfreetype-dev` on current
   Debian/Ubuntu — the old `libfontconfig1-dev`/`libfreetype6-dev` names are
   wrong here. Trim or extend from actual link errors in T1 and record the
   final list here.
4. Cache `~/.cargo` keyed on `Cargo.lock`.
5. Run `sh scripts/verify.sh` (fast tier, identity check, vendoring-freshness
   diff with the vendored generator).
6. Flatpak job (separate, needs Flatpak action): install SDK
   (`org.freedesktop.Sdk//25.08`, rust-stable extension), `flatpak-builder
   --force-clean`, then in-sandbox `scripts/smoke.sh` (incl. P6 examples
   assertion and headless-flag checks with no display).

### 3.2 Desktop + portal-provider verification matrix (P12)

File-chooser and open-file flows differ per portal backend, so every release
candidate is verified against this provider matrix (T0 pins the
`dialog::file_chooser` provider/features; failures here are RC blockers):

| Host DE / session | Portal backend | Expected chooser path |
|---|---|---|
| COSMIC (Wayland) | `xdg-desktop-portal-cosmic` (else `-gtk` fallback) | libcosmic chooser → portal |
| GNOME Wayland | `xdg-desktop-portal-gnome`/`-gtk` | libcosmic chooser → portal |
| GNOME X11 | `xdg-desktop-portal-gnome`/`-gtk` | libcosmic chooser → portal |
| KDE Plasma Wayland | `xdg-desktop-portal-kde` | libcosmic chooser → portal |
| KDE Plasma X11 | `xdg-desktop-portal-kde` | libcosmic chooser → portal |
| No portal (minimal WM / backend missing) | none | non-portal fallback (below) |

Per matrix row: app launches and renders, opens a `.mid` from the file
manager (MIME → single instance → **replace**), the in-app file-chooser
works, and a second `flatpak run` with a file forwards into the running
instance (P8). Record DE, compositor, portal backend versions, and `flatpak
--version` in the release checklist.

**Non-portal fallback:** dev/system builds (`cargo run`, unpackaged install)
use native dialogs with direct filesystem access. Sandboxed builds with no
portal backend keep working on already-granted + bundled paths (examples,
persisted picks); a new pick that needs a portal shows a guidance error
("install a portal backend / run outside the sandbox") — never a silent
failure, and never a `--filesystem=host` escape hatch.

Headless CI clips (run everywhere): `flatpak run com.goshapps.GoshMIDI
--help`, `--dry-run` over the examples, and the in-sandbox smoke script
from §2.3 — all with no display.

### T14c matrix inputs + Phase-3 manual recipes (clipboard, single-instance)

Verified headless in T14c (evidence above, all recorded 2026-09-12):

- Headless boot is DE-independent: `smoke.sh` §1b runs `--help` under
  `XDG_CURRENT_DESKTOP=GNOME` / `=KDE` / unset with no display (all exit 0
  with the usage marker, zero `panic`). This pins the CLI/boot path only —
  per-row GUI rendering stays a T15 live run.
- Icon resolution per row: COSMIC hosts mount `Cosmic`/`Pop` via
  `/run/host/share/icons` (themed icons win); GNOME/KDE/Adwaita-only hosts
  have no theme in the lookup chain (Adwaita is never consulted — chain is
  requested → Inherits → `Cosmic` → `hicolor` → `gnome` → `Yaru`), so the
  T14c hicolor fallback (§1.6) is what renders there. T15 live check per
  row: every toolbar/prefs/dialog icon visible (no empty buttons),
  especially `channel-secure` (lock) and the CSD window controls.
- Single-instance static halves: bus-name == desktop-basename invariant,
  `open`-action sending shape, and sandbox own-ID permission are pinned
  (§1.5); only the live double-run below remains.
- Clipboard path (with arch): lyrics Copy is
  `Message::LyricsCopy → iced::clipboard::write(full_text())` (`app.rs`,
  same full-buffer text as C++ `window.cpp:1811-1821`). Delivery is
  **compositor-mediated, not portal-mediated**: Wayland data-device over
  the existing `--socket=wayland` connection, X11 clipboard over
  `--socket=fallback-x11` — no finish-arg or permission change needed, and
  none was made. Nothing about the write is headless-observable (the
  `Task` is runtime-opaque; `full_text()` content is pinned by the lyrics
  tests), so no automated check was added — the recipe below is the check.

Phase-3 manual recipe (run once per §3.2 row, on the release candidate):

1. `flatpak run com.goshapps.GoshMIDI` → window renders, all icons visible.
2. Lyrics view → load `twinkle.kar` → Copy → paste into a host app
   (e.g. Text Editor) → full lyrics text arrives, sandbox-external paste
   works.
3. Second launch while running:
   `flatpak run --file-forwarding com.goshapps.GoshMIDI @@ /host/path/song.mid @@`
   → exits 0 immediately, running instance's playlist is REPLACED (size 1,
   first song selected) and its window presents. Bare second
   `flatpak run com.goshapps.GoshMIDI` → present-only, playlist untouched.
4. `COSMIC_SINGLE_INSTANCE=false flatpak run com.goshapps.GoshMIDI` →
   standalone second window (escape hatch).
5. MIME launch: open a `.mid` from the host file manager → routes into the
   running instance (replace + present), same as step 3.

## 4. Rollout checklist (first libcosmic Flatpak release)

- [ ] `flatpak/com.goshapps.GoshMIDI.yml` + committed `cargo-sources.json`
      (vendored + pinned generator, P4c/P5).
- [ ] `scripts/verify.sh` (+`--fast`) + `scripts/smoke.sh` green locally
      and in CI (tiered gates, P1).
- [x] CI triggers include `cosmic-migration` + full system deps (P4a/P4b;
      T15: CMake job retired, `cosmic-build.yml` is the CI).
- [x] Binary `dmidiplayer` end to end: manifest `command`, `Exec`, `--help`
      usage line, smoke test (P3; exact-line `--help`/`-v` pins in
      `tests/desktop_integration.rs`, T14c).
- [x] `DBusActivatable=true` + bus name owned (P8, T14c: desktop landed,
      bus-name == basename + `open`-action halves pinned, sandbox own-ID
      probed; live second-`flatpak run` forward is a Phase-3 recipe, §3.2).
- [ ] Examples generated + installed + asserted in-sandbox (P6).
- [x] Metainfo: toolkit wording + Rhythm-claim fix (P11, T14c) +
      `<release>` entry for the shipped version (T15).
- [x] Symbolic-icon hicolor fallback vendored + installed + inventoried
      (T14c, §1.6; per-row visibility is a T15 live check).
- [ ] `.flatpak` bundle verified on the §3.2 provider matrix (P12).
- [ ] Sandbox data-migration story + stale-SoundFont UX shipped (P7, §6;
      T15-S2: fixtures + bundled fallback landed, re-prompt UX in S3).
- [x] SoundFont story documented in-app and README (T15, §5 risk 2:
      bundled `FluidR3Mono_GM.sf3` + host-install advice).
- [x] CMake retired, CMake-dependent checks replaced, gresource
      grandfathering resolved by removal (P9, T15).

## 5. Risks

1. **ALSA sequencer in the sandbox.** `--socket=pulseaudio` covers
   FluidSynth audio, but external synths (Munt, Nuked-SC55, USB MIDI) need
   ALSA sequencer (`/dev/snd/seq`), which requires the broad `--device=all`.
   There is no narrower Flatpak permission for seq-only access. Mitigation:
   keep `--device=all` with a comment, document why, and re-check each
   runtime bump; QA must cover "external port listed + note plays" on a host
   with a sequencer client running.
2. **FluidSynth SoundFont access.** The host's
   `/usr/share/sounds/sf2/*.sf2` (e.g. `fluid-soundfont-gm`) is **not**
   visible in the sandbox, so a fresh install has no GM SoundFont and the
   README's "install fluid-soundfont-gm" advice stops working. Mitigations
   (T15, §6): bundled fallback `FluidR3Mono_GM.sf3` v2.312 (MIT,
   14,563,174 bytes — the T6-recorded `.sf3` substitution, full
   provenance in the manifest comments); sandbox-aware SF search paths
   (bundled file first, then host paths — never host `/usr/share`
   in-sandbox); portal picker with persisted access; re-prompt UX when the
   configured SoundFont is stale (S3). QA: fresh profile → audible
   FluidSynth playback with zero host setup (S4 gate).
3. **Vendoring drift / git-dep availability.** Any `Cargo.toml`/`Cargo.lock`
   change without regenerating `cargo-sources.json` breaks the offline
   Flatpak build; yanked crates or force-pushed git revs break it later.
   Mitigation: `--locked` from T1 + CI freshness diff with the vendored
   generator (§2.4), pinned git revs (P5), prefer crates.io releases over
   git deps.
4. **COSMIC runtime availability.** If a COSMIC runtime never ships on
   Flathub, COSMIC-specific theming/integration can lag; the freedesktop
   default (§1.2) keeps us shippable regardless.
5. **Portal behavior variance across DEs.** File-chooser and open-file flows
   differ on COSMIC/GNOME/KDE/X11. Mitigation: §3.2 provider matrix + portal
   fallback on every RC (P12).
6. **Stranded host-side user data.** Config, song settings, playlists, and
   SoundFont paths from native installs live outside the sandbox and may
   reference inaccessible absolute paths. Mitigation: §6 migration story +
   re-prompt UX; never auto-delete or silently drop user data (P7).

## 6. Sandbox data-migration story (P7, lands in T15)

What a native install leaves behind (all outside the sandbox):

- Global config `$XDG_CONFIG_HOME/dmidiplayer/dmidiplayer.conf` (30 keys,
  incl. `soundFont`, `lastPlayList`, `lastDirectory`, `recentFiles`).
- Song settings `~/.dmidiplayer/<song>.cfg` (Global + per-channel groups).
- Playlists (`.lst`) and MIDI files referenced by absolute host paths.
- The configured SoundFont as an absolute host path (stale in-sandbox).

Story (UX-led, no silent auto-copy across the sandbox boundary):

- First sandboxed run starts fresh; the app detects stale entries (config
  values, `.cfg` paths, playlist entries pointing at unreadable absolute
  paths) and offers per-path **re-prompt UX** ("re-locate" via the portal
  picker) instead of silently dropping them.
- **Stale absolute `soundFont` → re-prompt UX (S3):** on load, an unreadable
  configured SoundFont falls back to the bundled SF (with a toast naming the
  stale path) and offers the D11 chooser; the driver fallback order is
  otherwise unchanged. S2 lands the fixture shapes + the bundled file this
  contract builds on; S3 wires the UX against them.
- **Sandbox-aware SF search paths:** bundled
  `/app/share/soundfonts/FluidR3Mono_GM.sf3` first (P7 — host `/usr` is
  invisible in-sandbox), then the 8 host paths in C++ order
  (`output_fluid.rs::default_sound_font_paths`).
- **Bundled-SF choice (T15, per the T6 criteria — license must allow
  redistribution via Flathub alongside GPL-3.0-or-later code, size minimal
  for full GM coverage):** `FluidR3Mono_GM.sf3` v2.312 (2016-12-05) —
  the T6-recorded `.sf3` substitution for the 141.5 MiB `FluidR3_GM.sf2`.
  License **MIT** (`FluidR3Mono_License.md`: Frank Wen 2000-2002/2008 +
  Michael Cowgill 2014-16 mono version, full COPYING text; a `type: file`
  source installs it next to the font in-sandbox). Size **14,563,174
  bytes (~13.9 MiB)**. Upstream LibreScore/sf3 at pinned commit
  `3a6eda8` (v0.8.0), sha256
  `cfcd66d8…2794ec2` (two identical downloads, 2026-09-12; the `.wasm`
  suffix is upstream's web-compression alias for the same `.sf3` bytes —
  `RIFF....sfbk` magic verified). Decode proven pre-merge: host
  FluidSynth 2.6.0 (the manifest pin) renders `examples/test.mid`
  through it to 121,408 non-silent stereo frames. Full provenance in the
  `soundfont` module comments; the code↔manifest bond is pinned by
  `desktop_integration::bundled_soundfont_manifest_matches_code` and the
  `verify.sh` soundfont pin, in-sandbox presence/magic by `smoke.sh`.
- QA: scripted stale-path fixtures (`tests/fixtures/stale/`: config +
  `.cfg` + `.lst` with dead absolute paths, pinned by `tests/stale_flows.rs`
  — verbatim load, no drops, no panics) → S3 re-prompt UX on every stale
  path → S4 gate: fresh profile plays audibly with zero host setup.
