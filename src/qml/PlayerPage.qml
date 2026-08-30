// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

Kirigami.Page {
    id: page

    Kirigami.ColumnView.fillWidth: true

    title: Player.hasSong ? Player.songTitle : KI18n.i18nc("@title", "Gosh MIDI Player")

    padding: 0
    topPadding: 0
    bottomPadding: 0

    actions: [
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Open")
            icon.name: "document-open-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            onTriggered: applicationWindow().openFiles()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Previous")
            icon.name: "media-skip-backward-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: Player.playlist.count > 1
            shortcut: "Ctrl+Left"
            onTriggered: Player.previous()
        },
        Kirigami.Action {
            text: Player.playing ? KI18n.i18nc("@action:intoolbar", "Pause") : KI18n.i18nc("@action:intoolbar", "Play")
            icon.name: Player.playing ? "media-playback-pause-symbolic" : "media-playback-start-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: Player.hasSong || Player.playlist.count > 0
            onTriggered: Player.playing ? Player.pause() : Player.play()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Stop")
            icon.name: "media-playback-stop-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: Player.hasSong
            shortcut: "Ctrl+S"
            onTriggered: Player.stop()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Next")
            icon.name: "media-skip-forward-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: Player.playlist.count > 1
            shortcut: "Ctrl+Right"
            onTriggered: Player.next()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Back One Bar")
            icon.name: "media-seek-backward-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: Player.hasSong
            onTriggered: Player.rewindOneBar()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Forward One Bar")
            icon.name: "media-seek-forward-symbolic"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: Player.hasSong
            onTriggered: Player.forwardOneBar()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Loop")
            icon.name: "media-playlist-repeat-symbolic"
            checkable: true
            checked: Player.loopEnabled
            enabled: Player.hasSong
            onTriggered: {
                if (Player.loopEnabled) {
                    Player.loopEnabled = false;
                } else {
                    applicationWindow().showLoopRange();
                }
            }
        }
    ]

    Connections {
        target: Player

        function onMessage(text: string): void {
            notice.text = text;
            notice.type = Kirigami.MessageType.Information;
            notice.visible = true;
        }

        function onErrorMessage(text: string): void {
            notice.text = text;
            notice.type = Kirigami.MessageType.Error;
            notice.visible = true;
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Kirigami.InlineMessage {
            id: notice

            Layout.fillWidth: true
            position: Kirigami.InlineMessage.Position.Header
            showCloseButton: true
            visible: false
            type: Kirigami.MessageType.Information

            Timer {
                running: notice.visible && notice.type !== Kirigami.MessageType.Error
                interval: 6000
                onTriggered: notice.visible = false
            }
        }

        // Fetching a song off a network share can take a moment; say so.
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            position: Kirigami.InlineMessage.Position.Header
            visible: Player.busy
            type: Kirigami.MessageType.Information
            text: Player.busyProgress >= 0
                ? KI18n.i18nc("@info %1 is a message, %2 a percentage", "%1 (%2%)", Player.busyText, Player.busyProgress)
                : Player.busyText

            actions: [
                Kirigami.Action {
                    text: KI18n.i18nc("@action:button", "Cancel")
                    icon.name: "dialog-cancel-symbolic"
                    onTriggered: Player.cancelPendingLoad()
                }
            ]
        }

        // --- Now playing -----------------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: dashboard.implicitHeight + Kirigami.Units.largeSpacing * 2
            color: Kirigami.Theme.backgroundColor

            Kirigami.Theme.colorSet: Kirigami.Theme.Header
            Kirigami.Theme.inherit: false

            Kirigami.Separator {
                anchors.bottom: parent.bottom
                anchors.left: parent.left
                anchors.right: parent.right
            }

            RowLayout {
                id: dashboard

                anchors.fill: parent
                anchors.margins: Kirigami.Units.largeSpacing
                spacing: Kirigami.Units.largeSpacing * 2

                QQC2.Label {
                    text: Player.timeText
                    font.family: "monospace"
                    font.pointSize: Kirigami.Theme.defaultFont.pointSize * 2
                    font.weight: Font.Light
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Kirigami.Icon {
                            source: Player.songIsRemote ? "folder-network-symbolic" : "audio-midi"
                            implicitWidth: Kirigami.Units.iconSizes.small
                            implicitHeight: Kirigami.Units.iconSizes.small
                            visible: Player.hasSong
                        }

                        Kirigami.Heading {
                            Layout.fillWidth: true
                            level: 4
                            elide: Text.ElideMiddle
                            text: Player.hasSong ? Player.songTitle : KI18n.i18nc("@info", "No file loaded")
                        }
                    }

                    QQC2.Label {
                        Layout.fillWidth: true
                        visible: Player.hasSong
                        elide: Text.ElideMiddle
                        opacity: 0.7
                        font: Kirigami.Theme.smallFont
                        text: Player.songLocation
                    }

                    RhythmLamps {
                        Layout.fillWidth: true
                        beats: Player.beatsPerBar
                        activeBeat: Player.currentBeat
                        running: Player.playing
                    }
                }

                GridLayout {
                    columns: 2
                    rowSpacing: Kirigami.Units.smallSpacing
                    columnSpacing: Kirigami.Units.largeSpacing

                    QQC2.Label {
                        text: KI18n.i18nc("@label:textbox", "Position:")
                        opacity: 0.7
                        Layout.alignment: Qt.AlignRight
                    }
                    QQC2.Label {
                        text: Player.barBeatText
                        font.family: "monospace"
                    }

                    QQC2.Label {
                        text: KI18n.i18nc("@label:textbox", "Tempo:")
                        opacity: 0.7
                        Layout.alignment: Qt.AlignRight
                    }
                    QQC2.Label {
                        text: Player.bpmText
                    }

                    QQC2.Label {
                        text: KI18n.i18nc("@label:textbox", "Length:")
                        opacity: 0.7
                        Layout.alignment: Qt.AlignRight
                    }
                    QQC2.Label {
                        text: Player.hasSong ? Player.durationText : "—"
                    }
                }
            }
        }

        // --- Seek bar --------------------------------------------------------
        QQC2.Slider {
            id: seekSlider

            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.topMargin: Kirigami.Units.smallSpacing

            enabled: Player.hasSong
            from: 0
            to: 1
            value: Player.position

            onMoved: Player.position = value

            QQC2.ToolTip.visible: pressed
            QQC2.ToolTip.text: Player.timeText
        }

        // --- Tempo, volume, transposition ------------------------------------
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.largeSpacing

            QQC2.ToolButton {
                text: KI18n.i18nc("@action:button Reset the tempo to 100%", "Tempo")
                icon.name: "chronometer-reset-symbolic"
                onClicked: Player.tempoPercent = 100
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Reset the tempo to the score's own")
            }

            QQC2.Slider {
                Layout.fillWidth: true
                from: 50
                to: 200
                stepSize: 1
                value: Player.tempoPercent
                onMoved: Player.tempoPercent = value
                QQC2.ToolTip.visible: hovered || pressed
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Speed: %1%", Math.round(value))
            }

            QQC2.ToolButton {
                text: KI18n.i18nc("@action:button Reset the volume to 100%", "Volume")
                icon.name: "audio-volume-high-symbolic"
                onClicked: Player.volumePercent = 100
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Reset the volume to 100%")
            }

            QQC2.Slider {
                Layout.fillWidth: true
                from: 0
                to: 200
                stepSize: 1
                value: Player.volumePercent
                onMoved: Player.volumePercent = value
                QQC2.ToolTip.visible: hovered || pressed
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Volume: %1%", Math.round(value))
            }

            QQC2.Label {
                text: KI18n.i18nc("@label:spinbox Transpose the song", "Pitch:")
            }

            QQC2.SpinBox {
                from: -12
                to: 12
                value: Player.pitch
                editable: true
                onValueModified: Player.pitch = value
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Transpose by semitones; percussion is left alone")
            }
        }

        // --- Views -----------------------------------------------------------
        QQC2.TabBar {
            id: viewTabs

            Layout.fillWidth: true

            QQC2.TabButton {
                text: KI18n.i18nc("@title:tab", "Lyrics")
                icon.name: "view-media-lyrics-symbolic"
            }
            QQC2.TabButton {
                text: KI18n.i18nc("@title:tab", "Channels")
                icon.name: "audio-speakers-symbolic"
            }
            QQC2.TabButton {
                text: KI18n.i18nc("@title:tab", "Piano")
                icon.name: "audio-midi"
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: viewTabs.currentIndex

            LyricsView {}
            ChannelsView {}
            PianoBoard {}
        }
    }

    footer: QQC2.ToolBar {
        contentItem: RowLayout {
            spacing: Kirigami.Units.largeSpacing

            QQC2.Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: Player.statusText
            }

            QQC2.Label {
                elide: Text.ElideRight
                opacity: 0.7
                font: Kirigami.Theme.smallFont
                text: Player.outputSummary
            }
        }
    }
}
