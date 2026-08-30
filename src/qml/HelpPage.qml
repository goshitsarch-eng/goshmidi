// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import org.kde.ki18n
import org.kde.kirigamiaddons.formcard as FormCard

FormCard.FormCardPage {
    id: page

    title: KI18n.i18nc("@title:window", "Handbook")

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Playing songs")
    }

    FormCard.FormCard {
        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "Supported files")
            description: KI18n.i18nc("@info", "MIDI (.mid, .midi), karaoke (.kar), RIFF MIDI (.rmi) "
                + "and Cakewalk (.wrk). Playlists are plain .lst text files with one path or "
                + "address per line.")
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "Network shares")
            description: KI18n.i18nc("@info", "Pick a file from a share in the open dialog, or use "
                + "Open Location to type an address such as smb://server/music/song.mid. "
                + "The file is fetched in the background and played here; a companion SysEx "
                + "dump beside it is picked up too.")
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "Adjusting a song")
            description: KI18n.i18nc("@info", "Transpose by up to twelve semitones — percussion is "
                + "left alone. Tempo runs from half to double speed and volume from silence to "
                + "200%. Loop between two bars, or jump straight to one.")
        }
    }

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "MIDI output")
    }

    FormCard.FormCard {
        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "Hardware and emulators")
            description: KI18n.i18nc("@info", "Start Munt (mt32emu-qt) or Nuked-SC55 before "
                + "connecting, then choose ALSA in MIDI Setup and pick the port. Recognised "
                + "device names select their own instrument map and reset message.")
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "FluidSynth")
            description: KI18n.i18nc("@info", "For sound without external hardware, install a General "
                + "MIDI SoundFont and choose it in MIDI Setup.")
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "SysEx dumps")
            description: KI18n.i18nc("@info", "A .syx file named after the song, or Folder.syx in the "
                + "same directory, is sent before playback starts.")
        }
    }

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Where settings live")
    }

    FormCard.FormCard {
        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "Configuration")
            description: "~/.config/dmidiplayer/dmidiplayer.conf"
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextDelegate {
            text: KI18n.i18nc("@label", "Per-song settings")
            description: "~/.dmidiplayer/<song>.cfg"
        }
    }
}
