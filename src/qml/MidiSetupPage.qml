// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Layouts
import QtQuick.Dialogs as QtDialogs
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard
import org.goshapps.goshmidi

FormCard.FormCardPage {
    id: page

    title: KI18n.i18nc("@title:window", "MIDI Setup")

    property int backendIndex: Math.max(0, Player.backends.indexOf(Player.currentBackend))
    property int portIndex: 0
    property bool showAllPorts: Config.advancedPorts
    property string soundFontPath: Config.soundFont
    property int instrumentMap: Config.instrumentMap
    property int sysexReset: Config.sysexReset

    Component.onCompleted: refresh()

    function refresh(): void {
        Player.refreshPorts(Player.backends[page.backendIndex] ?? "", page.showAllPorts);
        page.portIndex = Math.max(0, Player.portLabels.indexOf(Player.currentPort));
    }

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Output")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Backend")
            description: KI18n.i18nc("@info:whatsthis",
                "ALSA reaches hardware synthesisers and emulators such as Munt or Nuked-SC55. "
                + "FluidSynth plays through a SoundFont of your own.")
            model: Player.backends
            currentIndex: page.backendIndex
            onActivated: index => {
                page.backendIndex = index;
                page.refresh();
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Connection")
            model: Player.portLabels
            currentIndex: page.portIndex
            enabled: Player.portLabels.length > 0
            onActivated: index => {
                page.portIndex = index;
                // A recognised device name picks its own instrument map and reset.
                const profile = Player.deviceProfileFor(index);
                if (profile.instrumentMap !== undefined) {
                    page.instrumentMap = profile.instrumentMap;
                    page.sysexReset = profile.sysexReset;
                }
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormSwitchDelegate {
            text: KI18n.i18nc("@option:check", "List every port, including through and system ports")
            checked: page.showAllPorts
            onToggled: {
                page.showAllPorts = checked;
                page.refresh();
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormButtonDelegate {
            text: KI18n.i18nc("@action:button", "Refresh Port List")
            description: KI18n.i18nc("@info:whatsthis",
                "Start Munt or Nuked-SC55 first, then refresh so their ALSA ports appear.")
            icon.name: "view-refresh-symbolic"
            onClicked: page.refresh()
        }
    }

    FormCard.FormPlaceholderMessageDelegate {
        visible: Player.portLabels.length === 0
        icon.name: "network-disconnect-symbolic"
        text: KI18n.i18nc("@info", "No MIDI destinations found")
        explanation: KI18n.i18nc("@info",
            "Start your synthesiser or emulator, then refresh the list.")
    }

    FormCard.FormHeader {
        title: KI18n.i18nc("@title:group", "Sound")
    }

    FormCard.FormCard {
        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Instrument map")
            model: [
                KI18n.i18nc("@item:inlistbox General MIDI", "GM"),
                KI18n.i18nc("@item:inlistbox Roland GS", "GS (SC-55)"),
                KI18n.i18nc("@item:inlistbox Roland MT-32", "MT-32")
            ]
            currentIndex: page.instrumentMap
            onActivated: index => page.instrumentMap = index
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormComboBoxDelegate {
            text: KI18n.i18nc("@label:listbox", "Reset sent before each song")
            model: [
                KI18n.i18nc("@item:inlistbox", "None"),
                KI18n.i18nc("@item:inlistbox", "GM Reset"),
                KI18n.i18nc("@item:inlistbox", "GS Reset (SC-55)"),
                KI18n.i18nc("@item:inlistbox", "XG Reset"),
                KI18n.i18nc("@item:inlistbox", "MT-32 Reset")
            ]
            currentIndex: page.sysexReset
            onActivated: index => page.sysexReset = index
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormTextFieldDelegate {
            id: soundFontField
            label: KI18n.i18nc("@label:textbox", "FluidSynth SoundFont")
            placeholderText: KI18n.i18nc("@info:placeholder", "Optional .sf2 or .sf3 file")
            text: page.soundFontPath
            onTextChanged: page.soundFontPath = text
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormButtonDelegate {
            text: KI18n.i18nc("@action:button", "Choose SoundFont…")
            icon.name: "document-open-symbolic"
            onClicked: soundFontDialog.open()
        }
    }

    FormCard.FormCard {
        Layout.topMargin: Kirigami.Units.largeSpacing

        FormCard.FormButtonDelegate {
            text: KI18n.i18nc("@action:button", "Play a Test Note")
            description: KI18n.i18nc("@info:whatsthis", "Sends middle C to the selected destination.")
            icon.name: "audio-volume-high-symbolic"
            onClicked: {
                page.apply();
                Player.sendTestNote();
            }
        }

        FormCard.FormDelegateSeparator {}

        FormCard.FormButtonDelegate {
            text: KI18n.i18nc("@action:button", "Apply and Close")
            icon.name: "dialog-ok-apply-symbolic"
            onClicked: {
                page.apply();
                applicationWindow().pageStack.layers.pop();
            }
        }
    }

    function apply(): void {
        Player.applyMidiSetup(Player.backends[page.backendIndex] ?? "",
                              page.portIndex,
                              page.soundFontPath,
                              page.instrumentMap,
                              page.sysexReset,
                              page.showAllPorts);
    }

    QtDialogs.FileDialog {
        id: soundFontDialog
        title: KI18n.i18nc("@title:window", "Choose a SoundFont")
        fileMode: QtDialogs.FileDialog.OpenFile
        nameFilters: [KI18n.i18n("SoundFonts (*.sf2 *.sf3)"), KI18n.i18n("All files (*)")]
        onAccepted: page.soundFontPath = selectedFile.toString().replace(/^file:\/\//, "")
    }
}
