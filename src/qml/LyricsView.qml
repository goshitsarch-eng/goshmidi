// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

ColumnLayout {
    id: view

    spacing: 0

    QQC2.ToolBar {
        Layout.fillWidth: true

        contentItem: RowLayout {
            spacing: Kirigami.Units.smallSpacing

            QQC2.Label {
                text: KI18n.i18nc("@label:listbox", "Track:")
            }

            QQC2.ComboBox {
                Layout.preferredWidth: Kirigami.Units.gridUnit * 12
                model: Player.lyricTracks
                currentIndex: Player.lyricTrackIndex
                onActivated: Player.lyricTrackIndex = currentIndex
            }

            QQC2.Label {
                text: KI18n.i18nc("@label:listbox Kind of text event", "Type:")
            }

            QQC2.ComboBox {
                Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                model: Player.lyricTypes
                currentIndex: Player.lyricTypeIndex
                onActivated: Player.lyricTypeIndex = currentIndex
            }

            QQC2.Label {
                text: KI18n.i18nc("@label:listbox Character encoding", "Encoding:")
            }

            QQC2.ComboBox {
                Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                model: Player.encodings
                currentIndex: Player.encodingIndex
                onActivated: Player.encodingIndex = currentIndex
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Change this if the lyrics look garbled")
            }

            Item {
                Layout.fillWidth: true
            }

            QQC2.ToolButton {
                icon.name: "edit-copy-symbolic"
                enabled: Player.plainLyrics.length > 0
                onClicked: Player.copyLyricsToClipboard()
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Copy the lyrics")
            }

            QQC2.ToolButton {
                icon.name: "document-save-symbolic"
                enabled: Player.plainLyrics.length > 0
                onClicked: applicationWindow().saveLyricsFile()
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Save the lyrics to a text file")
            }
        }
    }

    Flickable {
        id: flick

        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true

        boundsBehavior: Flickable.StopAtBounds
        QQC2.ScrollBar.vertical: QQC2.ScrollBar {}

        // The attached form is what lets a TextArea size and scroll correctly
        // inside a Flickable.
        QQC2.TextArea.flickable: QQC2.TextArea {
            id: lyrics

            readOnly: true
            wrapMode: TextEdit.Wrap
            textFormat: TextEdit.RichText
            background: null
            selectByMouse: true
            text: Player.lyricsText
            font.pointSize: Config.lyricsFontSize
            horizontalAlignment: switch (Config.textAlignment) {
                case 1: return TextEdit.AlignHCenter;
                case 2: return TextEdit.AlignRight;
                default: return TextEdit.AlignLeft;
            }
        }

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: Player.plainLyrics.length === 0
            icon.name: "view-media-lyrics-symbolic"
            text: Player.hasSong
                ? KI18n.i18nc("@info", "This song has no lyrics")
                : KI18n.i18nc("@info", "No song loaded")
            explanation: Player.hasSong
                ? KI18n.i18nc("@info", "Try another track or text type above.")
                : ""
        }
    }

    // Keep the line being sung in view.
    Connections {
        target: Player

        function onLyricsAdvanced(characterOffset: int): void {
            if (flick.contentHeight <= flick.height) {
                return;
            }
            const rectangle = lyrics.positionToRectangle(characterOffset);
            const target = rectangle.y - flick.height / 2;
            flick.contentY = Math.max(0, Math.min(target, flick.contentHeight - flick.height));
        }
    }
}
