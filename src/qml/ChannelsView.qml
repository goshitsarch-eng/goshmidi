// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

QQC2.ScrollView {
    id: view

    clip: true

    ListView {
        id: channelList

        model: Player.channels
        spacing: 0
        reuseItems: true

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: !Player.hasSong
            icon.name: "audio-speakers-symbolic"
            text: KI18n.i18nc("@info", "No song loaded")
            explanation: KI18n.i18nc("@info", "The channels a song uses appear here while it plays.")
        }

        delegate: QQC2.ItemDelegate {
            id: delegate

            required property int index
            required property int channel
            required property bool used
            required property string name
            required property bool muted
            required property bool solo
            required property bool locked
            required property int patch
            required property real level
            required property int volume
            required property bool pianoVisible

            width: ListView.view.width
            visible: delegate.used
            height: delegate.used ? implicitHeight : 0
            hoverEnabled: false
            down: false

            contentItem: RowLayout {
                spacing: Kirigami.Units.smallSpacing

                QQC2.Label {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 1.5
                    horizontalAlignment: Text.AlignRight
                    text: delegate.channel
                    font.family: "monospace"
                    opacity: 0.7
                }

                QQC2.TextField {
                    Layout.fillWidth: true
                    Layout.minimumWidth: Kirigami.Units.gridUnit * 5
                    text: delegate.name
                    onEditingFinished: Player.channels.setName(delegate.index, text)
                }

                QQC2.Button {
                    text: KI18n.i18nc("@action:button Mute this channel", "M")
                    checkable: true
                    checked: delegate.muted
                    implicitWidth: Kirigami.Units.gridUnit * 2
                    onToggled: Player.channels.setMuted(delegate.index, checked)
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Mute channel %1", delegate.channel)
                }

                QQC2.Button {
                    text: KI18n.i18nc("@action:button Solo this channel", "S")
                    checkable: true
                    checked: delegate.solo
                    implicitWidth: Kirigami.Units.gridUnit * 2
                    onToggled: Player.channels.setSolo(delegate.index, checked)
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Play channel %1 alone", delegate.channel)
                }

                QQC2.ProgressBar {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 4
                    from: 0
                    to: 1
                    value: delegate.level
                }

                QQC2.Slider {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 6
                    from: 0
                    to: 200
                    stepSize: 1
                    value: delegate.volume
                    onMoved: Player.channels.setVolume(delegate.index, value)
                    QQC2.ToolTip.visible: hovered || pressed
                    QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Level: %1%", Math.round(value))
                }

                QQC2.ToolButton {
                    icon.name: delegate.locked ? "lock-symbolic" : "unlock-symbolic"
                    checkable: true
                    checked: delegate.locked
                    onToggled: Player.channels.setLocked(delegate.index, checked)
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip",
                        "Keep this instrument, ignoring the song's own program changes")
                }

                QQC2.ComboBox {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 10
                    model: Player.patchNames
                    currentIndex: delegate.patch
                    onActivated: Player.channels.setPatch(delegate.index, currentIndex)
                }

                QQC2.CheckBox {
                    checked: delegate.pianoVisible
                    onToggled: Player.channels.setPianoVisible(delegate.index, checked)
                    QQC2.ToolTip.visible: hovered
                    QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Show this channel on the piano")
                }
            }
        }
    }
}
