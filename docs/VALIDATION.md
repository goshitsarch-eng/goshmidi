# Validation record — 2026-10-02

The rewrite is a prerelease, not a completed cross-platform production release.
This record separates executed checks from implementation and CI definitions.

| Check | Result | Evidence / limit |
|---|---|---|
| Original 1.8.0 baseline | Passed | Built/launched before rewrite; 675 tests, ten example smoke inputs; baseline captured in MIGRATION_AUDIT.md |
| Linux Rust regressions | Passed | 319 library + 2 CLI + 17 real-engine controller tests (338 total); separate audio test is normally ignored |
| Format / clippy | Passed | `cargo fmt --check`; release `cargo clippy --all-targets --all-features -- -D warnings` |
| Dependency advisory scanner | Unavailable | `cargo-audit` is not installed; a RustSec advisory scan remains a release check |
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
| Flatpak SDK build and core | Passed | Actual GNOME 50 SDK, offline Cargo build and 338 core tests; bundled FluidSynth 2.6, verified SoundFont and Rust license inventory |
| Installed Flatpak parser | Passed | Real installed sandbox decodes MID and emits KAR metadata; standalone bundle created |
| Installed Flatpak WebView | Passed | Opt-in QA variant passes all nine stages, including clipboard and per-song writes; required network namespace sharing confirmed by A/B testing |
| Normal installed Flatpak GUI/audio/activation | Passed | Release without QA code renders; real secondary process adds KAR; quit Cancel keeps window open, Discard exits; bundled FluidSynth/SF3 capture: 504,174 signed-16 samples, 377,428 nonzero, peak 5,680 |
| Flatpak document portals | Blocked by host | Cloud host has no `/dev/fuse`; chooser/export grants and sandbox printing still require a capable desktop |
| GitHub CI execution | Blocked by account | Actual push/PR workflows were triggered; every build job failed before steps ran. GitHub annotation: "The job was not started because your account is locked due to a billing issue." |
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
The installed Flatpak screenshot is `docs/screenshots/flatpak-installed.png`.

The environment configuration draft adds api.github.com and dl.flathub.org.
Saving a draft alone does not activate policy. Both endpoints became reachable
during final checks. The native matrix could be triggered, but GitHub's account
billing lock prevents runners from starting. Installed Flatpak checks ran
locally using official GNOME runtime/SDK 50 and Rust SDK extension 25.08.
Draft PR: https://github.com/goshitsarch-eng/goshmidi/pull/8.
Blocked native matrix: https://github.com/goshitsarch-eng/goshmidi/actions/runs/36954692212.

Flatpak evidence includes `flatpak-build-final.log`, `flatpak-permission-build.log`,
`flatpak-app-install.log` and `flatpak-qa-confirmed/report.json` in the task evidence
directory. Dioxus's authenticated localhost edit stream requires `--share=network`
for WebKit's nested helper; without it the installed UI remained blank. Flatpak's
permission also allows remote networking, because it cannot limit sharing to
loopback. The manifest documents this measured requirement. Normal bundles exclude
the opt-in QA code. `flatpak-normal-final.png`, `flatpak-activation-final.png` and
`flatpak-audio/report.json` record the normal installed release. Its app commit is
`940ca8fe5688998337ae78fbcaab97c89f9ec8e065881c1196c4d65f94908b74`.
Native host printing does not establish that feature inside Flatpak.
