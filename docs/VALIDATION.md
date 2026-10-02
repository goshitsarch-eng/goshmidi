# Validation record — 2026-10-02

The rewrite is a prerelease, not a completed cross-platform production release.
This record separates executed checks from implementation and CI definitions.

| Check | Result | Evidence / limit |
|---|---|---|
| Original 1.8.0 baseline | Passed | Built/launched before rewrite; 675 tests, ten example smoke inputs; baseline captured in MIGRATION_AUDIT.md |
| Linux Rust regressions | Passed | 319 library + 2 CLI + 17 real-engine controller tests (338 total); separate audio test is normally ignored |
| Format / clippy | Passed | `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings` |
| CLI parser smoke | Passed | Ten MID/KAR fixtures decode; corrupt/missing inputs fail; headless metadata works |
| Actual X11 WebView | Passed | Fresh-profile acceptance harness, all nine stages |
| Actual Wayland WebView | Passed | Same nine stages with GDK Wayland on a private Weston compositor with an input seat |
| Native portal Open | Passed | Ctrl+O opens GTK portal provider; selected real KAR; dirty replacement prompts Save/Discard/Cancel |
| Native playlists | Passed | Secondary launch adds a song; native save writes two entries; clear/reopen restores list |
| Failed save during quit | Passed live | Read-only disposable profile produces visible error and recovery dialog; Cancel keeps the window open; explicit Quit without saving exits |
| Clipboard / text save | Passed | Native X11 clipboard and portal text-save output identical, 367-byte karaoke text |
| Printing | Passed | Actual GTK print dialog; Print to File produces a one-page PDF; extracted text contains both verses |
| FluidSynth runtime sound | Passed | Real Rust engine + verified SF3 + PulseAudio monitor: 498,532 signed-16 samples, 204,475 nonzero, peak 5,520 |
| Local activation protocol | Passed | Authentication rejection, forwarded Unicode paths, separate profiles and stale-endpoint recovery; Linux uses Unix sockets |
| Linux release / packages | Passed | Optimized build; DEB and tar.gz created; checksums verified; extracted DEB executable decodes all ten installed examples and launches its real WebView; system APT installation was not performed |
| Windows desktop source | Passed compile check | `cargo check --all-targets --target x86_64-pc-windows-msvc`; SDK icon/version resources require a native build; no native link/install/UI proof |
| macOS core, both architectures | Passed compile checks | `--no-default-features --lib` for arm64 and Intel; no WKWebView/CoreMIDI/native package proof |
| macOS desktop cross-check | Blocked | Linux lacks Apple's Objective-C compiler/SDK; compiler rejects `-arch`/deployment flags |
| Flatpak source/manifest | Checked | Valid YAML; regenerated Cargo sources identical; libxdo and FluidSynth/SoundFont pins supplied |
| Installed Flatpak | In progress | Initial Flathub 403 resolved during final checks; official SDK/runtime metadata now reachable; installed sandbox results pending |
| GitHub CI execution | In progress | Initial API 403 resolved during final checks; authenticated repository access now available; native workflow results pending |
| Signing / notarization | Open release gate | macOS ad-hoc packaging only; production Apple/Windows identities and signing flow unavailable |
| Physical external MIDI | Unverified | No ALSA sequencer or physical synthesizer; WinMM/CoreMIDI hardware unavailable |
| Native drop / opener / reveal | Unverified live | Native/Dioxus adapters implemented; no full file-manager/browser/device desktop available |
| OS-driven theme change / scale | Unverified live | Explicit Light/Dark/System selection tested; real OS preference changes and multi-monitor scale need native desktop QA |

The WebView suite actually operates Play/Pause/Stop, bar/seek/loop/repeat,
all views, channel edits, piano audition, lyric filters/export, all 18 preferences,
MIDI setup/refresh/test/apply, Help/About/File Info, song settings, themes and
sidebar/dirty confirmation. Native dialogs are tested separately; the DOM suite
does not fake them. A reused profile caused an initial mixer assertion failure
because it already had saved toggles; the harness uses fresh profiles thereafter.

A headless Wayland backend with no input seat could not supply a clipboard.
The adapter now reports that condition, and the complete Wayland run uses an
input-capable compositor. This is distinct from a normal Wayland desktop.

## Reproducible evidence

`tests/application_contracts.rs`, `tests/runtime_audio.rs`, `tests/desktop_qa.js`,
`scripts/desktop-qa.py`, `scripts/smoke.sh`, and the desktop workflow provide
repeatable checks. Actual screenshots are in `docs/screenshots/`; they are not
mockups. Task logs, JSON WebView reports, PCM capture, printed PDF, exported
lyrics, baseline source/binary and extracted DEB installation are retained
outside the checkout in `/workspace/goshmidi-environment/`.

The environment configuration draft adds api.github.com and dl.flathub.org.
Saving a draft alone does not activate policy. Both endpoints became reachable
during final checks; native CI and installed Flatpak results are being recorded.
