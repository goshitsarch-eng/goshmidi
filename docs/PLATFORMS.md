# Platforms, data and packaging

| Platform | WebView | External MIDI | Synthesizer audio | Artifact |
|---|---|---|---|---|
| Windows x64 MSVC | WebView2 | midir / WinMM | FluidSynth WASAPI, DirectSound, PortAudio | per-user MSI and ZIP |
| macOS Apple Silicon / Intel | WKWebView | midir / CoreMIDI | FluidSynth CoreAudio | app bundle ZIP |
| Linux x64 | WebKitGTK 4.1 | ALSA sequencer | FluidSynth PulseAudio, PipeWire, ALSA, JACK, SDL3 | DEB and tar.gz |
| Flatpak x64 | GNOME runtime WebKitGTK | ALSA sequencer | bundled FluidSynth + GM SoundFont | .flatpak |

This table describes implementation targets. Verified results are separate in
VALIDATION.md. Windows dynamically loads packaged DLLs with restricted search
flags; arbitrary MIDI paths cannot select a DLL. macOS packaging rewrites bundled
Homebrew library dependencies to `@loader_path`, checks upstream license-source
hashes and signs the completed bundle. Linux uses system shared libraries.

## Persistent data

Linux keeps `$XDG_CONFIG_HOME/dmidiplayer/dmidiplayer.conf` (normally
`~/.config/dmidiplayer/dmidiplayer.conf`). Windows and macOS derive standard
config/local-data paths through `directories::BaseDirs`; data remains under the
application's `dmidiplayer` subdirectory. `--portable` places settings in the
working directory, and `--file PATH` selects an explicit portable profile.

The original key-file formats remain readable, including unknown keys, CRLF
playlists and relative playlist entries. New song settings use hashed full-path
names under the application's data directory. Legacy `~/.dmidiplayer/*.cfg`
files are read as a fallback and left intact; their recorded song path protects
against basename collisions. Zero volume/channel levels now reload as silence.
Configuration, song settings and playlist writes use same-directory atomic
replacement. `desktop.json` adds appearance/geometry without changing legacy INI keys.

## Flatpak

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08
flatpak-builder --force-clean --user --install --repo=flatpak-repo \
  build-dir flatpak/com.goshapps.GoshMIDI.yml
flatpak run com.goshapps.GoshMIDI
flatpak build-bundle flatpak-repo gosh-midi.flatpak com.goshapps.GoshMIDI
```

The runtime/SDK and registry source archives are pinned. Build commands run
Cargo offline with the lockfile. File access uses portals; no host filesystem or
network sharing is granted. Audio uses the PulseAudio socket. `--device=all` is
retained for `/dev/snd/seq`: Flatpak has no sequencer-only device permission.
The manifest installs the application, SoundFont/license, examples, icons,
metadata, notices and man page. Native single-instance activation replaces the
old D-Bus-only service.

Install and interact with the built sandbox application before release. A host
binary or successful manifest parse is insufficient. Check chooser/document
portal access, clipboard, print portal, audio, external MIDI, theme and scale.
