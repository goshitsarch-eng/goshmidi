# Cross-platform architecture

This design follows the audited MIDI workflows in MIGRATION_AUDIT.md. It keeps
the existing event formats and real-time scheduler while replacing the
libcosmic application/frontend. Dioxus components do not own MIDI parsing,
persistent files, native handles, or external-process logic.

## Layers

* **core**: MIDI events, SMF/KAR/RMID/WRK parsing, charset conversion, instrument
  maps, SysEx and scheduling. A dedicated engine thread owns the active sequence
  and output. Commands and events remain owned messages with bounded clock
  updates. There are no UI dependencies here.
* **state/config**: portable playlist, mix, lyrics, piano, playback, and
  backward-compatible key-file settings. Paths come from the platform layer.
* **application/commands**: one typed command inventory and a toolkit-independent
  application controller. Menu, toolbar, shortcut, context menu, and drop events
  converge here. The controller returns effects for operations outside domain
  state (dialogs, clipboard, print, URLs, quit). Dialog edits are scratch state
  with explicit Apply/Cancel; committing merges owned preference keys only.
* **services**: background work for file operations/dialogs and lossless engine
  events. A main-thread Dioxus signal holds presentation state; no
  Arc<Mutex<Everything>> or widget-owned domain model is used.
* **platform**: centralized native MIDI, runtime FluidSynth loading, standard
  per-OS paths, system dialogs, external open/reveal, and lifecycle adapters.
  ALSA remains a Linux adapter; WinMM/CoreMIDI use midir. FluidSynth loads a
  vetted system/bundled library so absence reports an output error rather than
  preventing the app from launching. Full SF2/SF3 capability is retained.
* **frontend**: Dioxus Desktop, shared Rust components and embedded CSS and image assets.
  The host uses WebView2 on Windows, WKWebView on macOS, and WebKitGTK on Linux;
  GTK is the Linux WebView host, not an application-widget implementation.

## Data and errors

Global and per-song key-file formats remain compatible. Linux's existing
configuration location remains valid. Windows/macOS use their normal app-data
locations. New per-song data uses standard app-data directories and reads legacy
Linux files before migration; originals must not be silently rewritten.
Atomic same-directory replacements protect configuration/playlist writes. Linux activation uses authenticated Unix sockets across Flatpak network namespaces; Windows/macOS use authenticated loopback connections.
Expected I/O/parse/native errors become visible messages and structured logs.

## Desktop behavior

Native window controls and menus coexist with reachable Lyrics, Channels, and
Piano tabs and a collapsible playlist. Light, Dark, and Follow System share
accessible semantic colors; Follow System is the initial choice. All icon
controls have labels/tooltips. Native dialogs use portals on Linux/Flatpak.
Printing is a WebView/system print action; file parsing remains Rust-only.
Commands use Ctrl on Linux/Windows and Command on macOS, ignore text-input
editing shortcuts, and are scoped while modal dialogs are open.

## Validation and releases

Retain meaningful existing parser/scheduler/state tests and add migration tests
for controller effects, data safety, CLI decoding, malformed input, Unicode,
CRLF, and platform path resolution. Live UI checks must verify real playback,
all command entry points, dialogs, themes, resizing, and persistence. Native
OS CI builds/tests and package QA are separate evidence from local Linux work.
The prerelease packaging matrix includes Windows MSI, macOS app/archive,
Linux DEB/tarball, and Flatpak. Signing/notarization are optional credential-bound
release steps; unsigned local builds must work without certificates.
