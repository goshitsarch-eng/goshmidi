# Platform support and verification

Version 2.0.0-alpha.1 is a prerelease. "Implemented" below means the adapter
exists; it does not establish native operation. The current evidence is from
Linux x64. Windows desktop source and both macOS core targets compile; their
native applications, packages and hardware integrations have not been executed.

| Feature | Linux X11 / Wayland | Windows x64 | macOS arm64 / Intel | Flatpak x64 |
|---|---|---|---|---|
| Dioxus UI, transport, views, preferences | Live acceptance passed on both | Implemented; source checked | Implemented; native check open | Nine-stage installed WebView suite passed |
| MIDI/KAR/RMID/WRK parsing and scheduling | Regression tests passed | Source checked | Core source checked | SDK core tests and installed MID/KAR parser passed |
| FluidSynth audio | Actual PulseAudio output captured | WASAPI/DirectSound adapter; unverified | CoreAudio adapter; unverified | Bundled FluidSynth/SF3 output captured live |
| External MIDI and SysEx | ALSA adapter; no physical-device proof | WinMM adapter; unverified | CoreMIDI adapter; unverified | Device permission supplied; unverified |
| Native Open/Save | X11 portal exercised; Wayland adapter | Native adapter; unverified | Native adapter; unverified | Host FUSE missing; document grants blocked |
| Clipboard | X11 native text verified; Wayland suite passed | Native adapter; unverified | Native adapter; unverified | Installed WebView copy succeeded |
| Printing | Actual X11 native dialog and PDF verified | System WebView print; unverified | System WebView print; unverified | Unverified in sandbox |
| Playlist, compatible settings, atomic writes | Domain and live checks passed | Portable implementation; unverified live | Portable implementation; unverified live | Core tests and WebView dirty guard/per-song writes passed |
| Secondary file activation | Authenticated Unix socket tested | Authenticated local IPC; unverified live | Authenticated local IPC; unverified live | Real secondary process adds bundled KAR |
| Light/Dark/System selection | Live selection passed; OS-driven change open | Implemented; unverified | Implemented; unverified | Live selection passed; OS-driven change open |
| Drop, open URL, reveal file, multiple DPI/monitors | Implemented; live desktop QA open | Implemented; unverified | Implemented; unverified | Unverified |
| Distribution | DEB/archive created; extracted DEB checked | MSI/ZIP scripts; native acceptance open | App ZIP scripts; native acceptance open | GNOME SDK build, installed application and bundle checked |
| Production signing | Linux checksums generated | Signing not configured | Developer ID/notarization not configured | Remote publishing not performed |

Details, recorded evidence and remaining gates are in
[VALIDATION.md](docs/VALIDATION.md), [BUILDING.md](BUILDING.md),
[data and packaging](docs/PLATFORMS.md) and [MIGRATION_AUDIT.md](MIGRATION_AUDIT.md).
