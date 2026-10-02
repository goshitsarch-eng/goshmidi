% DMIDIPLAYER(1) Gosh MIDI Player 2.0.0-alpha.1
% Gosh MIDI Player contributors; original dmidiplayer by Pedro López-Cabanillas

# NAME

dmidiplayer — MIDI and karaoke player

# SYNOPSIS

`dmidiplayer [OPTIONS] [MIDI_FILES_OR_PLAYLISTS]`

# DESCRIPTION

Gosh MIDI Player uses Rust and Dioxus Desktop. It reads MID, MIDI, KAR, RMI and
WRK files, with FluidSynth or native external MIDI output. Linux uses ALSA;
Windows and macOS use the native MIDI backend. A `.lst` playlist contains one
path per line; relative entries resolve against that playlist's directory.

# OPTIONS

`-p, --portable`
: Settings in the working directory.

`-f, --file PATH`
: Explicit portable settings file.

`-d, --driver BACKEND` and `-c, --connection PORT`
: Select output together. Backends: ALSA on Linux; MIDI on Windows/macOS;
  FluidSynth or Dummy on all targets. Dummy is intentionally silent.

`--dry-run`
: Decode inputs without a GUI. Invalid/missing inputs return failure.

`--dump-info`
: Decode inputs and print JSON metadata without a GUI.

`-h, --help`; `-v, --version`
: Show help or version.

`--`
: Treat subsequent arguments as paths, including names beginning with `-`.

# DATA

Legacy key-file settings and song mixes remain readable. Linux configuration is
normally `~/.config/dmidiplayer/dmidiplayer.conf`; native Windows/macOS app-data
locations are used there. New per-song settings use hashed full-path names in
the application's data directory and read `~/.dmidiplayer/*.cfg` as a fallback.

# SHORTCUTS

Control on Linux/Windows, Command on macOS: O opens, Q quits, P pauses/resumes,
S stops, Left/Right changes song, comma opens preferences. Space plays/pauses;
F1 opens help. Editing controls and modal dialogs scope application shortcuts;
Escape cancels a modal. Native menus provide editing actions.

# STATUS

This version is a prerelease. Consult the repository's validation record for
native-platform and installed-package verification; compilation alone is not
cross-platform acceptance.

# LICENSE

GPL-3.0-or-later. See LICENSE and THIRD_PARTY_NOTICES.md.
