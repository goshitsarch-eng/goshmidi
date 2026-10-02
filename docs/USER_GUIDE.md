# Gosh MIDI Player user guide

This guide describes the Rust/Dioxus Desktop prerelease, 2.0.0-alpha.1.
See [platform support](../PLATFORM_SUPPORT.md) for executed checks and limits.

## Open and organize songs

Use Open or the playlist's + button to select MID, MIDI, KAR, RMI or WRK files.
Open replaces the playlist; + adds files. Activate a playlist row to load a song.
Use the row's × to remove it, the arrows to move it, Shuffle to reorder it and
Clear to empty the list. Open list and Save list use the compatible `.lst` format,
one file path per line. Unsaved replacement and quit offer Save, Discard and Cancel.
A write failure keeps the app open and shows a recovery choice.

The CLI accepts files and lists, `--file PATH` for a separate configuration,
`--portable` for a local configuration, `--driver NAME --connection PORT` for
output selection, and `--dry-run`/`--dump-info` for headless validation. Run
`dmidiplayer --help` for exact options. Launching a second process with files
for the same profile forwards them to the first window.

## Play and edit

Play toggles pause/resume; Stop releases notes and resets position. Previous and
Next select adjacent songs. The seek slider changes position; −1 bar, +1 bar and
Jump move by musical bars. Loop sets a bar range; Repeat selects off, song or
playlist repetition. Tempo changes speed, Volume changes MIDI level, and Pitch
transposes melodic channels while preserving percussion.

Channels shows mute, solo, program lock, instrument, channel name, level and
activity. Piano monitors notes and lets you audition keys through the selected
output. Its display controls adjust visible channels and unused rows.

Lyrics selects track, event type and text encoding. Follow lyrics tracks the
playback position. Copy places the complete text on the native clipboard;
Save text exports it and Print opens the system WebView print dialog. These
workflows report unavailable clipboard or portal services in the UI.

## Choose MIDI output

Open MIDI Setup. FluidSynth produces sound from an SF2/SF3 SoundFont; packages
include a verified GM SF3. You can choose another accessible SoundFont. External
output uses ALSA on Linux, Windows MIDI on Windows and CoreMIDI on macOS.
Refresh enumerates ports; advanced-port visibility, instrument maps and reset
options control the selected backend. Test note auditions the draft selection;
Apply commits it and Cancel leaves the current selection intact.

Dummy is an explicitly silent output for inspection or a machine without audio.
Physical MIDI, Windows/macOS output and Flatpak external-file portal acceptance
still require the native/device checks listed in the validation record. The old
manual's DLS Synth and SonivoxEAS backends are not provided by this rewrite.

## Settings and appearance

Preferences edits a draft; Apply validates and saves it, while Cancel discards
the draft. It includes playlist behavior, lyrics appearance, piano appearance
and note fonts. Save song settings and Load song settings store/retrieve a mix
for the full song path. Compatible legacy settings are read without overwriting
legacy per-song files. Language keys are retained; this prerelease presents its
interface in English.

The appearance selector offers System, Light and Dark. Window geometry and the
playlist sidebar are saved. Menus show platform shortcut modifiers: Command on
macOS and Control on Windows/Linux. Shortcuts avoid handling ordinary text edits
and dialogs as application commands.

## Missing services and release limits

Linux needs WebKitGTK and a running desktop portal provider; Flatpak also needs
working document portals to grant access to selected external files. The cloud
host cannot test these grants because it has no FUSE device. MIDI devices and
audio services must be available to the selected backend. Use MIDI Setup after
an initialization error rather than assuming silent playback produced sound.

Windows/macOS native builds, installed packages, signing and several desktop
checks remain open. [Validation](VALIDATION.md) states what actually ran;
[building](../BUILDING.md) covers dependencies and package commands. The files
under `docs/{en,cs,de,es,fr,gl,it,ru}/` are preserved historical manuals, with
translation updates pending.
