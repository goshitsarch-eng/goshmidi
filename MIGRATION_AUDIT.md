# Gosh MIDI Player migration audit

Baseline: commit `7a3e275439ef59f45aa887fc6dfb523c85fa5ac5` on selected `Main` ref;
application version 1.8.0. This inventory records source behavior, not promises
from the previous README. Status is updated as the migration is verified.

## Evidence and scope

The original application was built and launched before rewriting. Its fast
verification gate passed 675 unit/integration tests; one documentation test is
ignored. The existing smoke script passed for ten MID/KAR examples. The script
only checks argument/file-presence markers: it **does not decode MIDI**.

Live Linux checks used a private D-Bus session, Weston, a virtual display, and
Dummy MIDI output. `test.mid` loaded, displayed decoded metadata, and reached
Finished. The main transport, lyrics filters, menu, and Preferences were
inspected on the running app. Captures and an immutable source/binary reference
are retained outside the checkout at `/workspace/goshmidi-environment/reference`.
No Windows, macOS, physical MIDI device, desktop portal provider, or installed
Flatpak was available for baseline interaction. Source/tests are evidence for
those unexercised flows; they are not substitutes for live platform verification.

## Existing architecture

* Rust edition 2024, pinned Rust 1.98.1, crate/binary name `dmidiplayer`.
* Primary frontend: libcosmic/iced, fetched from a Git revision with multiple
  forked Git dependencies (winit, softbuffer, clipboard, accessibility).
* `src/main.rs`: hand-written CLI parser; launches a libcosmic single-instance
  app. Unknown options and some incomplete options are silently ignored.
* `src/app.rs`: 6,685 lines including tests; lifecycle, UI models, command
  handling, async dialogs, persistence, output fallback, and engine event routing
  are coupled to `cosmic::Application` and `Task<Message>`.
* `src/message.rs`: commands and shared types depend on iced keyboard/color
  picker types, coupling otherwise portable application logic to the toolkit.
* `src/core/`: event model, SMF/RMID/WRK parsers, charset decoding, instrument
  tables, SysEx handling, playback scheduler, outputs. The scheduler runs on a
  dedicated thread and receives owned commands. Notes/transport are lossless;
  clock updates have a bounded queue. This is useful behavior to retain.
* `src/state/`: playlist, playback, channels, lyrics, piano, MIDI configuration,
  dialog plans. Most of this is already portable and unit tested; keyboard
  handling depends on iced. GUI state mirrors engine messages.
* `src/config/`: GKeyFile-compatible parser/writer, per-song settings, path
  helpers. Unknown settings/groups survive a load/save. No data conversion to
  JSON/TOML is necessary for compatibility.
* `src/ui/`: widget construction and presentation; large files with many
  historical migration comments and source-shape tests.
* `tests/`: 12 integration targets. Most drive `App::init/update` directly and
  drop asynchronous Tasks; this misses missing UI entry points and live dialog
  failures. A passing suite does not establish every visible workflow works.

## Formats and persistence

* Read: `.mid`, `.midi`, `.kar` (SMF); `.rmi` (RIFF RMID including INFO);
  `.wrk` (Cakewalk); companion `.syx` (same basename, then `Folder.syx`).
* Playlists: `.lst`, one path per line; blank lines ignored; CRLF accepted;
  relative paths resolve against the playlist's directory; missing songs remain
  represented. Current save writes absolute paths. No unsaved-change prompt.
* Global settings: `$XDG_CONFIG_HOME/dmidiplayer/dmidiplayer.conf`, falling back
  to `~/.config/dmidiplayer/dmidiplayer.conf`. Portable `-p`/`-f` are opt-in.
* Song mix settings: `~/.dmidiplayer/<song filename>.cfg`; includes file,
  encoding, tempo, volume, pitch, and channel name/mute/solo/lock/program/level.
* Thirty known global keys: output connection/backend, SoundFont, last folder,
  playlist, language, percussion channel, solo reduction, autoplay/advance/song
  settings, advanced ports, reset/profile, piano display, lyrics colors/font/
  alignment, window size, sidebar visibility, repeat policy, ten recent files.
* MIDI Apply, Preferences close, and window close save global settings. Loading
  a song changes recent paths in memory. Per-song write errors are discarded.

## Platform and dependency audit

ALSA is an unconditional dependency; FluidSynth is unconditionally linked with
`#[link(name = "fluidsynth")]`. Audio driver candidates are all Linux-oriented.
Default SoundFont and example discovery hard-code `/app` and `/usr` directories.
Home resolution only understands HOME/XDG and falls back to `/`. Opening URLs
and printing use `Command::new("xdg-open")`; the latter opens an exported text
file and is not a print dialog. Dialogs, clipboard, file drop, D-Bus activation,
window controls, fonts, and icons all depend on libcosmic/Linux infrastructure.
There is no application networking beyond opening browser links. No shell
interpreters are needed by MIDI parsing or playback.

Packaging is a freedesktop 25.08 Flatpak with FluidSynth and an MIT GM SF3.
It requests `--device=all` for ALSA sequencer access. CI only runs Ubuntu;
there is no Windows installer or macOS bundle. A branch workflow hard-codes
historic branch names. Existing docs largely describe an earlier GTK/C++ port
and a libcosmic migration and must not be treated as current product docs.

## Feature inventory

`WORKING` below means source plus relevant automated or live baseline evidence;
hardware/platform-dependent live checks remain explicitly distinguished.

| Feature | Baseline | Source | Expected useful behavior / defects | Migration | Platform concerns |
|---|---|---|---|---|---|
| MID/MIDI/KAR decoding | WORKING | core/smf.rs, sequence.rs | Tempo, SMPTE, text, events, charset; malformed input must report an error | Rebuilt; Linux controls/domain verified | Portable |
| RMI/WRK decoding | WORKING (tests) | core/rmi.rs, wrk.rs | RIFF INFO, Cakewalk track offsets | Retained; parser regression tests pass | Portable |
| Open files / multi-select | PARTIAL | app.rs:587,1187 | Replace list, load first, autoplay preference; initial-folder builder ineffective | Rebuilt; Linux controls/domain verified | Native dialogs, portals |
| Add songs | WORKING (tests) | ui/sidebar.rs, state/playlist.rs | Append supported files without silently replacing list | Rebuilt; Linux controls/domain verified | Unicode paths |
| Drag/drop | PARTIAL | app.rs:613,2176 | Native files and portal transfers replace list; Linux-specific mechanisms | Implemented; native drag unverified | Test each OS |
| Playlist select / activate | WORKING | app.rs:2141,2147 | Select loads; activate starts playback | Rebuilt; Linux controls/domain verified | Keyboard and pointer |
| Remove / move / clear / shuffle | WORKING (tests) | state/playlist.rs | Preserve selected identity on reorder; dirty edits can be lost on exit | Rebuilt; Linux controls/domain verified | Portable |
| Playlist load/save | WORKING (tests) | state/playlist.rs, app.rs:1206 | Relative inputs, absolute outputs, CRLF; error details lost in bool API | Rebuilt; Linux controls/domain verified | Atomic writes, Unicode |
| Play/pause/resume/stop | WORKING | app.rs:683, core/player.rs | Stop silences notes, resets time; commands bounded by echo quantum | Rebuilt; Linux controls/domain verified | Dedicated engine thread |
| Previous/next / auto advance | WORKING (tests) | app.rs:717,738 | No manual wrap; repeat-list wraps; preserve autosave intent | Rebuilt; Linux controls/domain verified | Portable |
| Seek / bar steps / jump | WORKING (tests) | core/player.rs, state/dialogs.rs | Consistent position and playback continuation | Rebuilt; Linux controls/domain verified | Responsive controls |
| Loop and repeat policies | WORKING (tests) | core/player.rs, state/playback.rs | Validate ordered bar range; old loop confirms reversed bounds | Rebuilt; Linux controls/domain verified | Portable |
| Tempo / volume / transpose | WORKING | state/playback.rs | 50–200%, 0–200%, ±12 semitones; percussion not transposed | Rebuilt; Linux controls/domain verified | Portable |
| Channel mixer | BROKEN entry point | ui/channels.rs, app.rs:2624 | 16 channels, mute/solo/lock/program/name/level/meters; no UI emits ViewSelected | Rebuilt; Linux controls/domain verified | Must expose a reachable tab |
| GM/GS/MT-32 profiles | WORKING (tests) | core/instruments.rs | Names/reset suggested from output port; full tables | Rebuilt; Linux controls/domain verified | Portable |
| Lyrics/text filters and charset | WORKING | state/lyrics.rs, ui/lyrics.rs | Track/type/encoding selection; live lyric accumulation | Rebuilt; Linux controls/domain verified | Unicode clipboard/font |
| Lyrics copy/save | WORKING (tests) | app.rs:2263 | Copy decoded text; save actual text with error reporting | Rebuilt; Linux controls/domain verified | System clipboard/dialog |
| Lyrics print | PARTIAL | app.rs:343,1346 | Opens a temporary text file instead of printing | Rebuilt; Linux controls/domain verified | WebView/system print |
| Lyrics font/color/alignment | WORKING (tests) | state/lyrics.rs | Live preferences; preserve GDK/Pango-compatible strings | Rebuilt; Linux controls/domain verified | CSS presentation conversion |
| Piano monitor/audition | BROKEN entry point | ui/piano.rs, state/piano.rs | Note matrix, range/tighten, show-all/hide-unused; no reachable view switch | Rebuilt; Linux controls/domain verified | Pointer capture + release |
| Piano display preferences | PARTIAL | ui/prefs.rs, ui/piano.rs | Velocity tint, palette, names; octave subscript is a carried no-op | Rebuilt; Linux controls/domain verified | Responsive SVG/HTML |
| MIDI setup/refresh/test/apply | PARTIAL | app.rs:935,1016 | Explicit errors/fallback; enumeration/test not hardware-verified | Rebuilt; Linux controls/domain verified | WinMM/CoreMIDI/ALSA |
| FluidSynth + SF2/SF3 | WORKING offline | core/output_fluid.rs | Non-silent offline rendering verified; native audio not available | Real SF3 audio captured; nonzero output | Runtime library + audio drivers |
| ALSA external MIDI | UNKNOWN hardware | core/output_alsa.rs | Port filtering, advanced ports, all MIDI messages including SysEx | Retained; physical device unverified | Linux only; native equivalents elsewhere |
| Dummy output | WORKING | core/output.rs | Useful explicit silent/testing backend | Rebuilt; Linux controls/domain verified | All OS |
| Companion SysEx/reset | WORKING (tests) | core/sysex.rs, player.rs | GM/GS/XG/MT-32, paced dumps, responsive stop | Retained + tested; hardware unverified | Native MIDI must retain SysEx |
| Preferences commit/cancel | PARTIAL | app.rs:1628,2521 | Close commits; no Cancel; tall dialog/menu can obscure controls | Rebuilt; Linux controls/domain verified | Modal focus/fit |
| Song settings load/save/auto | PARTIAL | config/song.rs, app.rs:1066 | Compatible mixes; silent write errors and unsanitized names | Rebuilt; Linux controls/domain verified | Safe paths + legacy lookup |
| Recent files / saved sidebar | PARTIAL | config/settings.rs | MRU-10 preserved; no reachable sidebar visibility toggle | Rebuilt; Linux controls/domain verified | Portable |
| Window size persistence | WORKING (tests) | app.rs:2546, state/playlist.rs | Actual size on close; maximization not saved | Size/position/maximized implemented + tests | Per-platform lifecycle |
| Menus / nine shortcuts | PARTIAL | ui/menu.rs, state/keys.rs | 23 menu actions; Ctrl conventions only; modal focus can escape | Rebuilt; Linux controls/domain verified | Native macOS menu + Command |
| Help/about/browser links | WORKING (source/live shell) | state/dialogs.rs, app.rs:365 | Attribution, help, source/search links; xdg-open only | Dialogs verified; browser/reveal unverified | System URL opener |
| CLI help/version/portable | WORKING | main.rs | Preserve command name and flags; reject malformed arguments | Rebuilt; Linux controls/domain verified | OsString paths |
| CLI dry-run/dump-info | BROKEN validation claim | main.rs:126 | Currently only lists file paths; must decode and fail bad inputs | Rebuilt; Linux controls/domain verified | Headless all targets |
| Single-instance/file activation | PARTIAL | app.rs:2725, main.rs | D-Bus only; forward files and focus existing window | Authenticated IPC tested; Linux Unix socket | Local IPC/native open events |
| Follow System/Light/Dark | NOT IMPLEMENTED choice | ui/theme.rs | Old toolkit chooses theme; no persisted explicit user choice | Rebuilt; Linux controls/domain verified | Live OS theme |
| Language / notesFont keys | DEAD application behavior | config/settings.rs | Persisted, no applied translation or notes font | Language preserved; notesFont now applied | Keep data; don't advertise fake functionality |
| Windows/macOS packages | NOT IMPLEMENTED | .github/workflows | Native app, installer/bundle and platform QA required | Packagers + CI implemented; native acceptance blocked | OS runners |
| Flatpak release | UNKNOWN current instance | flatpak/*.yml | Must build/install/run inside sandbox; host success insufficient | SDK build, installed parser/WebView and bundle checked | GNOME/WebKit runtime and portals |

## Confirmed defects to address

1. Channels and Piano exist but are unreachable: production source has no
   producer of `Message::ViewSelected`, only its update handler and a unit test.
2. Direct X11 launch panics in pinned softbuffer for a transparent depth-32
   visual. Wayland launch works. The replacement must be checked on both.
3. Dry-run claims validation without parsing, and missing inputs exit success.
4. Silent settings/song-setting writes lose user data without diagnosis.
5. Preferences can overrun a short window; the menu can remain over its modal.
   A keyboard focus trap and explicit Apply/Cancel are needed.
6. “Print” delegates to a text viewer; it does not implement printing.
7. No sidebar toggle or reachable view switcher; huge empty default song area.
8. Home, soundfont discovery, openers, outputs, and single-instance lifecycle
   encode Linux assumptions. No non-Linux CI or release artifacts exist.
9. Loop bounds can be reversed; failures loading songs can leave stale UI
   metadata associated with the newly selected playlist entry.

## Migration rules and acceptance ledger

Retain the portable parsers, engine protocol, instrument tables, compatible
settings, and useful state helpers. Replace frontend-specific application code
with an explicit command/effect model. Do not introduce a second format parser.
Changes to buggy behavior above are intentional improvements, not parity loss.
Keep version 1.8.0 data readable; the rewrite is `2.0.0-alpha.1` until all
platform/release acceptance checks pass. No stable release is claimed by a
compiling shell. Every platform result must distinguish source implementation,
compile/test evidence, live UI evidence, installed package evidence, and unknown.

For every inventory row record: identified, old behavior documented,
reimplemented, domain tested, UI tested, Windows verified, macOS verified,
Linux verified, Flatpak verified, and documentation updated. Remaining external
blockers must be named, not turned into checked boxes.

## Rewrite acceptance matrix (2026-10-02)

All inventory features were identified and their baseline behavior documented
before the rewrite. The table above retains baseline source locations at the
recorded commit; the original source/binary is preserved outside the checkout
and in Git history. The obsolete libcosmic frontend, controller/messages,
keyboard adapter, D-Bus-only service and toolkit-specific tests have been
retired from the canonical tree after Linux domain/live parity checks.

`Pass` means executed relevant checks; `Source` means implemented but not live
verified. `Compile` is a source compile check, not native acceptance. `Blocked`
requires external runtime/access. Current documentation covers every row.

| Feature | Reimplemented | Domain | Linux UI/native | Windows | macOS | Flatpak | Docs |
|---|---|---|---|---|---|---|---|
| MID/MIDI/KAR decoding | Yes | Pass | Pass | Compile only | Core compile only | SDK + installed pass | Updated |
| RMI/WRK decoding | Yes | Pass | Pass | Compile only | Core compile only | SDK tests pass | Updated |
| Open files / multi-select | Yes | Pass | Pass | Compile only | Core compile only | Core pass; portal blocked | Updated |
| Add songs | Yes | Pass | Pass | Compile only | Core compile only | Core pass; portal blocked | Updated |
| Drag/drop | Yes | Pass | Source; drop unverified | Compile only | Core compile only | Native unverified | Updated |
| Playlist select / activate | Yes | Pass | Pass | Compile only | Core compile only | Core pass | Updated |
| Remove / move / clear / shuffle | Yes | Pass | Pass | Compile only | Core compile only | Core pass; clear guard live | Updated |
| Playlist load/save | Yes | Pass | Pass | Compile only | Core compile only | Core pass; portal blocked | Updated |
| Play/pause/resume/stop | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Previous/next / auto advance | Yes | Pass | Pass | Compile only | Core compile only | Core pass | Updated |
| Seek / bar steps / jump | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Loop and repeat policies | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Tempo / volume / transpose | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Channel mixer | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| GM/GS/MT-32 profiles | Yes | Pass | Pass | Compile only | Core compile only | Core pass; setup live | Updated |
| Lyrics/text filters and charset | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Lyrics copy/save | Yes | Pass | Pass | Compile only | Core compile only | Copy pass; portal blocked | Updated |
| Lyrics print | Yes | Pass | Pass | Compile only | Core compile only | Native unverified | Updated |
| Lyrics font/color/alignment | Yes | Pass | Pass | Compile only | Core compile only | Live preferences pass | Updated |
| Piano monitor/audition | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Piano display preferences | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| MIDI setup/refresh/test/apply | Yes | Pass | Pass | Compile only | Core compile only | Dummy live pass | Updated |
| FluidSynth + SF2/SF3 | Yes | Pass | Pass | Compile only | Core compile only | Bundled SF3 audio pass | Updated |
| ALSA external MIDI | Yes | Pass | No device | Compile only | Core compile only | No device | Updated |
| Dummy output | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Companion SysEx/reset | Yes | Pass | Device unverified | Compile only | Core compile only | SDK pass; device unverified | Updated |
| Preferences commit/cancel | Yes | Pass | Pass | Compile only | Core compile only | Live pass | Updated |
| Song settings load/save/auto | Yes | Pass | Pass | Compile only | Core compile only | Live save/load pass | Updated |
| Recent files / saved sidebar | Yes | Pass | Pass | Compile only | Core compile only | Core + live sidebar pass | Updated |
| Window size persistence | Yes | Pass | Pass | Compile only | Core compile only | Core pass | Updated |
| Menus / nine shortcuts | Yes | Pass | Pass | Compile only | Core compile only | Menus live; shortcuts open | Updated |
| Help/about/browser links | Yes | Pass | Dialogs pass; opener unverified | Compile only | Core compile only | Dialogs pass; opener open | Updated |
| CLI help/version/portable | Yes | Pass | Pass | Compile only | Core compile only | Installed CLI pass | Updated |
| CLI dry-run/dump-info | Yes | Pass | Pass | Compile only | Core compile only | Installed pass | Updated |
| Single-instance/file activation | Yes | Pass | Pass | Compile only | Core compile only | Installed activation pass | Updated |
| Follow System/Light/Dark | Yes | Pass | Pass | Compile only | Core compile only | Live selection pass | Updated |
| Language / notesFont keys | Font applied; language key retained | Pass | Font pass; no translation advertised | Compile only | Core compile only | Live font; language retained | Updated |
| Windows/macOS packages | Yes | N/A | N/A | Compile only | Core compile only | N/A | Updated |
| Flatpak release | Yes | Sources checked | N/A | Compile only | Core compile only | Built, installed, bundled | Updated |

## Additional migration fixes

- Rapid Pause/Resume could lose the queued resume while the scheduler performed
  its note-release postlude. The engine now preserves that command; a real-engine
  controller regression test catches it.
- Late clock events after Stop no longer resurrect a nonzero position.
- Saved master/channel zero levels now reload as silence instead of being ignored.
- Full song paths prevent settings basename collisions and preserve non-UTF-8
  file identity; legacy files are read without overwriting them.
- Static lyric exports no longer duplicate sung text; KAR syllables join with
  proper line/verse breaks. Invalid preference colors keep the draft open.
- All four piano palettes, note font and octave names are applied; audition keys
  light immediately and release on pointer/focus/modal changes. A stale timer
  cannot release a newer audition.
- Failed preference writes prevent closing, and missing portal/clipboard providers
  report visible errors. Seatless Wayland clipboard access is guarded.
- Confirmed Quit changes the host window policy to close and exits, rather than
  leaving a hidden process under the temporary dirty/save guard policy.
- Linux activation uses a private Unix socket. Dioxus's separate localhost
  edit stream requires network sharing for WebKit's nested Flatpak helper;
  installed-app A/B tests established this requirement.

Linux X11 and Wayland WebView suites, native chooser/save/clipboard/PDF,
nonzero FluidSynth sound and extracted DEB checks passed. The 338 ordinary
Rust tests pass; a separate explicit live-audio check also passed. Remaining
Windows/macOS native UI/install, physical MIDI, signing, drop/opener/scale/OS
theme events and Flatpak document portals/device checks remain open. See
[docs/VALIDATION.md](docs/VALIDATION.md) for evidence and blockers. The rewrite
is intentionally not labeled a completed production migration.
