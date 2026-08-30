// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.kde.kirigami.delegates as KirigamiDelegates
import org.goshapps.goshmidi

Kirigami.ScrollablePage {
    id: page

    title: Player.playlist.fileName.length > 0
        ? KI18n.i18nc("@title <playlist file name>", "Playlist — %1", Player.playlist.fileName)
        : KI18n.i18nc("@title", "Playlist")

    visible: Config.playlistVisible

    Kirigami.ColumnView.fillWidth: false

    actions: [
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Add Files…")
            icon.name: "list-add-symbolic"
            onTriggered: applicationWindow().openFiles()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Shuffle")
            icon.name: "media-playlist-shuffle-symbolic"
            enabled: Player.playlist.count > 1
            onTriggered: Player.shufflePlaylist()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Open Playlist…")
            icon.name: "document-open-symbolic"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: applicationWindow().openPlaylistFile()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Save Playlist As…")
            icon.name: "document-save-as-symbolic"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            enabled: Player.playlist.count > 0
            onTriggered: applicationWindow().savePlaylistFile()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:intoolbar", "Clear")
            icon.name: "edit-clear-all-symbolic"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            enabled: Player.playlist.count > 0
            onTriggered: Player.clearPlaylist()
        }
    ]

    ListView {
        id: playlistView

        model: Player.playlist
        currentIndex: Player.playlist.currentIndex
        clip: true
        reuseItems: true

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: playlistView.count === 0
            icon.name: "view-media-playlist-symbolic"
            text: KI18n.i18nc("@info", "No songs yet")
            explanation: KI18n.i18nc("@info", "Add MIDI files, or open one from a network share.")

            helpfulAction: Kirigami.Action {
                text: KI18n.i18nc("@action:button", "Add Files…")
                icon.name: "list-add-symbolic"
                onTriggered: applicationWindow().openFiles()
            }
        }

        delegate: QQC2.ItemDelegate {
            id: delegate

            required property int index
            required property string name
            required property string location
            required property bool remote
            required property bool current

            width: ListView.view.width
            highlighted: delegate.current

            onClicked: Player.playIndex(delegate.index)

            contentItem: RowLayout {
                spacing: Kirigami.Units.smallSpacing

                Kirigami.Icon {
                    source: delegate.current && Player.playing
                        ? "media-playback-start-symbolic"
                        : (delegate.remote ? "folder-network-symbolic" : "audio-midi")
                    implicitWidth: Kirigami.Units.iconSizes.small
                    implicitHeight: Kirigami.Units.iconSizes.small
                }

                KirigamiDelegates.TitleSubtitle {
                    Layout.fillWidth: true
                    title: delegate.name
                    subtitle: delegate.location
                    selected: delegate.highlighted || delegate.down
                    font.bold: delegate.current
                }
            }

            QQC2.ToolTip.visible: hovered
            QQC2.ToolTip.text: delegate.location

            // Right-click menu, the way a KDE list behaves.
            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: contextMenu.popup()
            }

            QQC2.Menu {
                id: contextMenu

                QQC2.MenuItem {
                    text: KI18n.i18nc("@action:inmenu", "Play")
                    icon.name: "media-playback-start-symbolic"
                    onTriggered: Player.playIndex(delegate.index)
                }
                QQC2.MenuSeparator {}
                QQC2.MenuItem {
                    text: KI18n.i18nc("@action:inmenu", "Move Up")
                    icon.name: "arrow-up-symbolic"
                    enabled: delegate.index > 0
                    onTriggered: Player.moveUp(delegate.index)
                }
                QQC2.MenuItem {
                    text: KI18n.i18nc("@action:inmenu", "Move Down")
                    icon.name: "arrow-down-symbolic"
                    enabled: delegate.index < Player.playlist.count - 1
                    onTriggered: Player.moveDown(delegate.index)
                }
                QQC2.MenuSeparator {}
                QQC2.MenuItem {
                    text: KI18n.i18nc("@action:inmenu", "Remove From Playlist")
                    icon.name: "list-remove-symbolic"
                    onTriggered: Player.removeAt(delegate.index)
                }
            }
        }
    }

    footer: QQC2.ToolBar {
        contentItem: RowLayout {
            spacing: 0

            QQC2.ToolButton {
                icon.name: "arrow-up-symbolic"
                enabled: Player.playlist.currentIndex > 0
                onClicked: Player.moveUp(Player.playlist.currentIndex)
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Move the selected song up")
            }
            QQC2.ToolButton {
                icon.name: "arrow-down-symbolic"
                enabled: Player.playlist.currentIndex >= 0
                    && Player.playlist.currentIndex < Player.playlist.count - 1
                onClicked: Player.moveDown(Player.playlist.currentIndex)
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Move the selected song down")
            }
            QQC2.ToolButton {
                icon.name: "list-remove-symbolic"
                enabled: Player.playlist.currentIndex >= 0
                onClicked: Player.removeAt(Player.playlist.currentIndex)
                QQC2.ToolTip.visible: hovered
                QQC2.ToolTip.text: KI18n.i18nc("@info:tooltip", "Remove the selected song")
            }

            Item {
                Layout.fillWidth: true
            }

            QQC2.Label {
                opacity: 0.7
                font: Kirigami.Theme.smallFont
                text: KI18n.i18ncp("@info:status", "%1 song", "%1 songs", Player.playlist.count)
            }
        }
    }
}
