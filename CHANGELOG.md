# Changelog

## 2.0.0-alpha.1 — unreleased

- Rebuilt the desktop application with Rust and Dioxus Desktop, separating
  application decisions, presentation, platform services and the MIDI engine.
- Added Windows WebView2/WinMM and macOS WKWebView/CoreMIDI adapters and native
  packaging/CI definitions, alongside Linux WebKitGTK and Flatpak configuration.
- Preserved the file decoders, playback scheduler, playlists, MIDI controls,
  compatible INI settings and 18 preferences; retained the `dmidiplayer` CLI.
- Made Channels and Piano reachable; added actual piano audition, responsive
  views, modal keyboard handling and System/Light/Dark appearance.
- Fixed duplicated karaoke exports, discarded read/write errors, zero-volume
  settings reload, basename collisions, stale load identity and pause/resume races.
- Added atomic persistence, guarded dirty transitions and explicit recovery
  when saving settings during quit fails; committed Quit exits the process.
- Replaced Linux-only printing/clipboard/path assumptions with native adapters;
  added authenticated profile-aware activation and pinned release resources.
- Added a baseline audit, domain regressions and an opt-in real-WebView suite.

Linux checks and artifacts are recorded in [VALIDATION.md](docs/VALIDATION.md).
Native Windows/macOS, installed Flatpak, physical MIDI, production signing and
remote CI execution remain release gates; this is not a production release.
