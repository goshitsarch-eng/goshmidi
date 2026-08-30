// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

ColumnLayout {
    id: board

    spacing: 0

    QQC2.ToolBar {
        Layout.fillWidth: true

        contentItem: RowLayout {
            spacing: Kirigami.Units.smallSpacing

            QQC2.CheckBox {
                id: tighten
                text: KI18n.i18nc("@option:check Narrow the keyboard to the notes actually used", "Fit to song range")
                enabled: Player.hasSong
            }

            QQC2.ToolButton {
                text: KI18n.i18nc("@action:button", "Show All")
                icon.name: "view-visible-symbolic"
                onClicked: Player.channels.showAllChannels()
            }

            QQC2.ToolButton {
                text: KI18n.i18nc("@action:button", "Hide Unused")
                icon.name: "view-hidden-symbolic"
                onClicked: Player.channels.showOnlyUsedChannels()
            }

            Item {
                Layout.fillWidth: true
            }

            QQC2.Label {
                opacity: 0.7
                font: Kirigami.Theme.smallFont
                text: KI18n.i18nc("@info:status", "Click a key to play it")
            }
        }
    }

    QQC2.ScrollView {
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true

        contentWidth: availableWidth

        PianoView {
            id: piano

            width: parent.width
            implicitWidth: parent.width

            controller: Player
            channels: Player.channels
            tightenKeys: tighten.checked
            rowHeight: Kirigami.Units.gridUnit * 4

            // Follows the active colour scheme, light or dark.
            Kirigami.Theme.colorSet: Kirigami.Theme.View
            Kirigami.Theme.inherit: false

            backgroundColor: Kirigami.Theme.backgroundColor
            whiteKeyColor: Kirigami.ColorUtils.tintWithAlpha(
                Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.04)
            blackKeyColor: Kirigami.ColorUtils.tintWithAlpha(
                Kirigami.Theme.backgroundColor, Kirigami.Theme.textColor, 0.82)
            keyBorderColor: Qt.alpha(Kirigami.Theme.textColor, 0.35)
            labelColor: Qt.alpha(Kirigami.Theme.textColor, 0.65)
            channelLabelColor: Qt.alpha(Kirigami.Theme.textColor, 0.8)
            labelFont: Kirigami.Theme.smallFont

            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton

                onPressed: mouse => {
                    const channel = piano.channelAt(mouse.y);
                    const note = piano.noteAt(mouse.x, mouse.y);
                    if (channel >= 0 && note >= 0) {
                        Player.playNote(channel, note, 80);
                    }
                }

                onReleased: Player.releaseNotes()
                onCanceled: Player.releaseNotes()
            }
        }
    }
}
