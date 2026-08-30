Gosh MIDI Player
================

A MIDI file player for the Plasma desktop, built with **Qt 6** and
**[Kirigami](https://develop.kde.org/frameworks/kirigami/)**. This is a fork of
**[dmidiplayer](https://sourceforge.net/p/dmidiplayer/)** (Drumstick MIDI File
Player) by Pedro López-Cabanillas. The command and compatible settings paths
remain `dmidiplayer`, `~/.config/dmidiplayer`, and `~/.dmidiplayer`; Gosh Apps
releases use the permanent application ID `com.goshapps.GoshMIDI`.

It reads **.MID** / **.MIDI** (Standard MIDI Files), **.KAR** (Karaoke),
**.RMI** (RIFF RMID), and **.WRK** (Cakewalk) files, and sends MIDI events to
ALSA sequencer ports or to an embedded FluidSynth backend.

The interface follows the system colour scheme in **light and dark**, uses the
Plasma widget style through `org.kde.desktop`, and is convergent: on a wide
window the playlist sits beside the player, on a narrow one it becomes its own
page.

![App Screenshot](https://p.kagi.com/proxy/screenshot.png?c=IVBIz-djrqhBdoI1kAYWuIGdBLzVdwgu0RO3DCOd_o3wHzM7zafCPDhPaFUe6FuRQx3eskzbUvHhtBSisKAaDPlR2o8sJArato3jLu0QNpAYAUg84hhh9hbq0oz6KTK3)


Original project
----------------

* Homepage: [dmidiplayer.sourceforge.io](https://dmidiplayer.sourceforge.io/)
* SourceForge: [sourceforge.net/p/dmidiplayer](https://sourceforge.net/p/dmidiplayer/)
* GitHub (upstream): [pedrolcl/dmidiplayer](https://github.com/pedrolcl/dmidiplayer)

Key features
------------

* MIDI output to ALSA hardware/soft-synth ports (MT-32, SC-55, USB MIDI, Munt),
  FluidSynth with your own SoundFont, or a dummy sink
* **Plays files that live on network shares** — Samba (`smb://`), SSH
  (`sftp://`, `fish://`), WebDAV (`dav://`) and anything else KIO reaches, either
  picked in the file dialog or typed into **Open Location** (Ctrl+L)
* Instrument maps and SysEx resets for GM, GS (SC-55), XG, and MT-32
* Automatic companion `.syx` loading (same name as the MIDI file, or
  `Folder.syx`), including from remote shares
* Transpose song tonality between -12 and +12 semitones (percussion excluded)
* Change MIDI volume (CC7) from 0% to 200%
* Scale song speed between 50% and 200% tempo
* Lyrics, piano, MIDI channels, and rhythm views
* Playlists (`.lst`): add/remove/reorder/shuffle, save/load, next/previous,
  auto-advance, repeat song or whole list
* One window per session: opening a file from the file manager hands it to the
  running instance over D-Bus

Build requirements
------------------

* C++20 compiler
* Qt 6.5+ (Base, Declarative, Qt5Compat)
* KDE Frameworks 6.21+ (Kirigami, Kirigami Add-ons, CoreAddons, I18n,
  DBusAddons, KIO, IconThemes, ColorScheme) and Extra CMake Modules
* ALSA, FluidSynth, uchardet
* CMake 3.20+

On a distribution that ships KDE Frameworks 6:

```
$ sudo apt install cmake g++ pkg-config extra-cmake-modules \
        qt6-base-dev qt6-declarative-dev qt6-5compat-dev \
        kf6-kirigami-dev kf6-kirigami-addons-dev \
        libkf6coreaddons-dev libkf6i18n-dev libkf6kio-dev \
        libkf6iconthemes-dev libkf6colorscheme-dev \
        libasound2-dev libfluidsynth-dev libuchardet-dev
$ cmake -S . -B build -DCMAKE_BUILD_TYPE=RelWithDebInfo
$ cmake --build build
$ cmake --install build
```

Ubuntu 24.04 has no KF6 of its own; the CI workflow in
`.github/workflows/linux-build.yml` shows how to take Qt 6 and KF6 from the KDE
neon archive, which targets that same release.

Run `dmidiplayer --help` for command-line options (`-d`/`--driver`,
`-c`/`--connection`, portable settings, and files that become a temporary
playlist).

Playing from a network share
----------------------------

Pick the file from a share in the open dialog, or press **Ctrl+L** and type an
address such as `smb://server/music/song.mid`. If the share is already mounted,
the file is played in place; otherwise it is fetched in the background — with a
progress message you can cancel — and a companion `.syx` beside it comes along.
Playlists may hold remote addresses alongside local paths.

Choosing a synthesiser
----------------------

For sound without an external synth, install a GM SoundFont (for example
`fluid-soundfont-gm`) and select the **FluidSynth** backend in MIDI Setup.
To use your own module (Roland MT-32 / [Munt](https://github.com/munt/munt),
SC-55 / [Nuked-SC55](https://github.com/PanykSystem/Nuked-SC55-GUI-Float), USB MIDI):

1. Start the emulator so it appears as an ALSA sequencer port
   (`mt32emu-qt` → `MT-32:Standard`; Nuked-SC55 → `Virtual SC55` / similar).
2. Open **MIDI Setup**, choose **ALSA**, pick that port, and Apply.
   Named ports select the matching instrument map and SysEx reset.
3. Optionally send a companion `.syx` dump by placing it next to the `.mid` file.

Both emulators must be running **before** you connect; use **Refresh Port List**
if you started them after opening MIDI Setup.
