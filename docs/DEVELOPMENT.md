# Development

The canonical entry point is `src/main.rs`. `application::Controller` owns
application decisions and the engine lifecycle. `commands` is the shared menu,
shortcut and action inventory; `frontend` holds Dioxus components/CSS; `platform`
contains native dialogs, clipboard, activation, MIDI and resource adapters.
Portable parsers, scheduling and compatible settings remain in `core`, `state`
and `config`. See ARCHITECTURE.md.

## Daily checks

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
sh scripts/smoke.sh
```

`--no-default-features` builds the domain and headless CLI without Dioxus or a
WebView. Linux still needs ALSA development files for native MIDI. Tests use
real scheduler events and isolated settings; most do not need audio hardware.
One explicit audio test is ignored in the ordinary suite:

```sh
GOSH_MIDI_TEST_SOUNDFONT=/absolute/path/GM.sf3 \
  cargo test --test runtime_audio -- --ignored --nocapture
```

A real audio server is required. Capture its monitor output when testing sound;
receiving MIDI events alone does not establish that audio was produced.

The `ui-test` feature adds a finite acceptance harness. `GOSH_MIDI_UI_REPORT`
selects its JSON report path. Use a fresh profile on every run; saved mute/solo
and preferences deliberately persist and change initial control values.
`scripts/desktop-qa.py` handles profiles and deadlines. Native chooser, PDF,
clipboard, activation and external-device checks are additional checks.

## Dependencies and Flatpak sources

Cargo.lock contains registry packages only. After changing dependencies, run:

```sh
python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
```

The standard-library-only generator uses Cargo checksums and rejects Git
sources. CI compares regenerated sources byte for byte. Resource fetching
checks SHA-256 before replacing any file and preserves TLS verification.

## Troubleshooting

- Missing WebKitGTK: install the 4.1 development/runtime packages; GTK4 or
  WebKitGTK 6.0 alone does not provide Dioxus's Linux host dependencies.
- File chooser unavailable: run in a session D-Bus and install/configure your
  desktop's portal backend. The application reports a missing provider.
- Silent output: Dummy is intentionally silent. Check MIDI Setup, SoundFont,
  the selected native audio driver and the desktop audio server.
- Another profile is not responding: close that profile's process. OS locks,
  rather than a stale PID file, determine ownership; crashed instances can be reopened.
- A Linux cross-check cannot validate WKWebView, CoreMIDI, WinMM, MSI or signing.
  Use the corresponding native CI runner and installed-package checklist.

In this managed cloud workspace, source
`/workspace/goshmidi-environment/activate.sh` before Cargo/native commands.
`dioxus-desktop.sh` starts a private X11 desktop and session D-Bus using the
extracted system libraries. `dioxus-wayland.sh` is the Wayland QA counterpart.
These helpers and logs live outside the repository and are not release assets.
