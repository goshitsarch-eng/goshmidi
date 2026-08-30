// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard
import org.goshapps.goshmidi

FormCard.FormCardPage {
    id: page

    title: KI18n.i18nc("@title:window", "Configure Gosh MIDI Player")

    Component.onDestruction: Config.save()

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Playback")
    }

    FormCard.FormCard {
        FormCard.FormSpinBoxDelegate {
            label: KI18n.i18nc("@label:spinbox", "Percussion MIDI channel")
            from: 1
            to: 16
            value: Config.drumsChannel
            onValueModified: Config.drumsChannel = value
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSpinBoxDelegate {
            label: KI18n.i18nc("@label:spinbox", "Volume drop for non-solo channels (%)")
            from: 0
            to: 100
            value: Config.soloVolumeReduction
            onValueModified: Config.soloVolumeReduction = value
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: KI18n.i18nc("@option:check", "Start playing as soon as a song loads")
            checked: Config.autoPlay
            onToggled: Config.autoPlay = checked
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: KI18n.i18nc("@option:check", "Advance to the next playlist item")
            checked: Config.autoAdvance
            onToggled: Config.autoAdvance = checked
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: KI18n.i18nc("@option:check", "Load and save per-song settings automatically")
            description: KI18n.i18nc("@info:whatsthis",
                "Keeps each song's encoding, pitch, tempo and channel mix in ~/.dmidiplayer.")
            checked: Config.autoSongSettings
            onToggled: Config.autoSongSettings = checked
        }
    }

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Lyrics")
    }

    FormCard.FormCard {
        FormCard.FormSpinBoxDelegate {
            label: KI18n.i18nc("@label:spinbox", "Text size")
            from: 8
            to: 72
            value: Config.lyricsFontSize
            onValueModified: Config.lyricsFontSize = value
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Alignment")
            model: [
                KI18n.i18nc("@item:inlistbox Text alignment", "Left"),
                KI18n.i18nc("@item:inlistbox Text alignment", "Centre"),
                KI18n.i18nc("@item:inlistbox Text alignment", "Right")
            ]
            currentIndex: Config.textAlignment
            onActivated: index => Config.textAlignment = index
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: KI18n.i18nc("@label:chooser", "Words already sung")
            color: Config.pastColor
            onColorChanged: Config.pastColor = color
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: KI18n.i18nc("@label:chooser", "Words being sung")
            color: Config.highlightColor
            onColorChanged: Config.highlightColor = color
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: KI18n.i18nc("@label:chooser", "Words still to come")
            color: Config.futureColor
            onColorChanged: Config.futureColor = color
        }
    }

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Piano")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Colour played notes by")
            model: [
                KI18n.i18nc("@item:inlistbox", "One colour"),
                KI18n.i18nc("@item:inlistbox", "Two colours"),
                KI18n.i18nc("@item:inlistbox", "MIDI channel"),
                KI18n.i18nc("@item:inlistbox", "Scale degree")
            ]
            currentIndex: Config.highlightPalette
            onActivated: index => Config.highlightPalette = index
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormColorDelegate {
            text: KI18n.i18nc("@label:chooser", "Single highlight colour")
            color: Config.singleColor
            onColorChanged: Config.singleColor = color
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: KI18n.i18nc("@option:check", "Shade notes by how hard they are struck")
            checked: Config.velocityColor
            onToggled: Config.velocityColor = checked
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Show note names")
            model: [
                KI18n.i18nc("@item:inlistbox", "Never"),
                KI18n.i18nc("@item:inlistbox", "On each C"),
                KI18n.i18nc("@item:inlistbox", "While played"),
                KI18n.i18nc("@item:inlistbox", "Always")
            ]
            currentIndex: Config.namesVisibility
            onActivated: index => Config.namesVisibility = index
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: KI18n.i18nc("@option:check", "Write the octave as a subscript")
            checked: Config.octaveSubscript
            onToggled: Config.octaveSubscript = checked
        }
    }

    FormCard.FormCard {
        Layout.topMargin: Kirigami.Units.largeSpacing

        FormCard.FormButtonDelegate {
            text: KI18n.i18nc("@action:button", "Restore Defaults")
            icon.name: "edit-undo-symbolic"
            onClicked: Config.restoreDefaults()
        }
    }
}
