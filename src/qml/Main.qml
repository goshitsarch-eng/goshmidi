// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Dialogs as QtDialogs
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

Kirigami.ApplicationWindow {
    id: root

    readonly property string appName: KI18n.i18nc("@title The application name", "Gosh MIDI Player")

    readonly property var midiNameFilters: [
        KI18n.i18n("All supported files (*.mid *.midi *.kar *.rmi *.wrk *.lst)"),
        KI18n.i18n("MIDI files (*.mid *.midi)"),
        KI18n.i18n("Karaoke files (*.kar)"),
        KI18n.i18n("RIFF MIDI files (*.rmi)"),
        KI18n.i18n("Cakewalk files (*.wrk)"),
        KI18n.i18n("Playlists (*.lst)"),
        KI18n.i18n("All files (*)")
    ]

    title: Player.hasSong
        ? KI18n.i18nc("@title:window <song> — <application>", "%1 — %2", Player.songTitle, appName)
        : appName

    width: Config.windowWidth
    height: Config.windowHeight
    minimumWidth: Kirigami.Units.gridUnit * 30
    minimumHeight: Kirigami.Units.gridUnit * 22

    onClosing: Player.persistWindowState(root.width, root.height)

    // Space toggles playback, but not while a name is being typed into a text
    // field — there it has to reach the field as a space.
    readonly property bool editingText: {
        const item = root.activeFocusItem;
        // instanceof has already established that readOnly exists here; the
        // linter simply does not narrow the type of activeFocusItem.
        // qmllint disable missing-property
        return (item instanceof TextInput || item instanceof TextEdit) && !item.readOnly;
        // qmllint enable missing-property
    }

    Shortcut {
        sequences: ["Space", "Media Play"]
        enabled: !root.editingText
        onActivated: Player.playing ? Player.pause() : Player.play()
    }

    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.ToolBar
    pageStack.defaultColumnWidth: Kirigami.Units.gridUnit * 16
    pageStack.initialPage: [playlistPageComponent, playerPageComponent]

    // Everything the drawer, the toolbar and the shortcuts share.
    function openFiles(): void { openFileDialog.open(); }
    function openLocation(): void { locationDialog.open(); }
    function openPlaylistFile(): void { openPlaylistDialog.open(); }
    function savePlaylistFile(): void { savePlaylistDialog.open(); }
    function saveLyricsFile(): void { saveLyricsDialog.open(); }
    function showFileInformation(): void { fileInfoDialog.open(); }
    function showJumpToBar(): void { jumpDialog.open(); }
    function showLoopRange(): void { loopDialog.open(); }
    function showMidiSetup(): void { pageStack.layers.push(midiSetupComponent); }
    function showSettings(): void { pageStack.layers.push(settingsComponent); }
    function showHelp(): void { pageStack.layers.push(helpComponent); }
    function showAbout(): void { pageStack.layers.push(aboutComponent); }

    globalDrawer: Kirigami.GlobalDrawer {
        isMenu: true
        title: root.appName
        titleIcon: "com.goshapps.GoshMIDI"

        actions: [
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Open Files…")
                icon.name: "document-open-symbolic"
                shortcut: StandardKey.Open
                onTriggered: root.openFiles()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu Open a file from a network share", "Open Location…")
                icon.name: "folder-network-symbolic"
                shortcut: "Ctrl+L"
                onTriggered: root.openLocation()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "File Information")
                icon.name: "documentinfo-symbolic"
                enabled: Player.hasSong
                onTriggered: root.showFileInformation()
            },
            Kirigami.Action {
                separator: true
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Load Song Settings")
                icon.name: "document-revert-symbolic"
                enabled: Player.hasSong
                onTriggered: Player.loadSongSettings()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Save Song Settings")
                icon.name: "document-save-symbolic"
                enabled: Player.hasSong
                onTriggered: Player.saveSongSettings()
            },
            Kirigami.Action {
                separator: true
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Jump to Bar…")
                icon.name: "go-jump-symbolic"
                enabled: Player.hasSong
                onTriggered: root.showJumpToBar()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Loop Between Bars…")
                icon.name: "media-playlist-repeat-symbolic"
                enabled: Player.hasSong
                onTriggered: root.showLoopRange()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Repeat")
                icon.name: "media-repeat-all-symbolic"

                Kirigami.Action {
                    text: KI18n.i18nc("@action:inmenu Repeat nothing", "Off")
                    checkable: true
                    checked: Player.repeatMode === 0
                    onTriggered: Player.repeatMode = 0
                }
                Kirigami.Action {
                    text: KI18n.i18nc("@action:inmenu", "Current Song")
                    checkable: true
                    checked: Player.repeatMode === 1
                    onTriggered: Player.repeatMode = 1
                }
                Kirigami.Action {
                    text: KI18n.i18nc("@action:inmenu", "Whole Playlist")
                    checkable: true
                    checked: Player.repeatMode === 2
                    onTriggered: Player.repeatMode = 2
                }
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Show Playlist")
                icon.name: "view-media-playlist-symbolic"
                checkable: true
                checked: Config.playlistVisible
                shortcut: "Ctrl+P"
                onTriggered: {
                    Config.playlistVisible = !Config.playlistVisible;
                    Config.save();
                }
            },
            Kirigami.Action {
                separator: true
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "MIDI Setup…")
                icon.name: "audio-card-symbolic"
                onTriggered: root.showMidiSetup()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Configure…")
                icon.name: "settings-configure-symbolic"
                shortcut: StandardKey.Preferences
                onTriggered: root.showSettings()
            },
            Kirigami.Action {
                separator: true
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Handbook")
                icon.name: "help-contents-symbolic"
                shortcut: StandardKey.HelpContents
                onTriggered: root.showHelp()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "About Gosh MIDI Player")
                icon.name: "help-about-symbolic"
                onTriggered: root.showAbout()
            },
            Kirigami.Action {
                text: KI18n.i18nc("@action:inmenu", "Quit")
                icon.name: "application-exit-symbolic"
                shortcut: StandardKey.Quit
                onTriggered: root.close()
            }
        ]
    }

    Component {
        id: playlistPageComponent
        PlaylistPage {}
    }

    Component {
        id: playerPageComponent
        PlayerPage {}
    }

    Component {
        id: midiSetupComponent
        MidiSetupPage {}
    }

    Component {
        id: settingsComponent
        SettingsPage {}
    }

    Component {
        id: helpComponent
        HelpPage {}
    }

    Component {
        id: aboutComponent
        Kirigami.AboutPage {
            aboutData: Player.aboutData
        }
    }

    JumpToBarDialog {
        id: jumpDialog
    }

    LoopDialog {
        id: loopDialog
    }

    FileInfoDialog {
        id: fileInfoDialog
    }

    OpenLocationDialog {
        id: locationDialog
    }

    QtDialogs.FileDialog {
        id: openFileDialog
        title: KI18n.i18nc("@title:window", "Open MIDI Files")
        fileMode: QtDialogs.FileDialog.OpenFiles
        nameFilters: root.midiNameFilters
        onAccepted: Player.openUrls(selectedFiles, true)
    }

    QtDialogs.FileDialog {
        id: openPlaylistDialog
        title: KI18n.i18nc("@title:window", "Open Playlist")
        fileMode: QtDialogs.FileDialog.OpenFile
        nameFilters: [KI18n.i18n("Playlists (*.lst)"), KI18n.i18n("All files (*)")]
        onAccepted: Player.loadPlaylist(selectedFile)
    }

    QtDialogs.FileDialog {
        id: savePlaylistDialog
        title: KI18n.i18nc("@title:window", "Save Playlist")
        fileMode: QtDialogs.FileDialog.SaveFile
        defaultSuffix: "lst"
        nameFilters: [KI18n.i18n("Playlists (*.lst)")]
        onAccepted: Player.savePlaylist(selectedFile)
    }

    QtDialogs.FileDialog {
        id: saveLyricsDialog
        title: KI18n.i18nc("@title:window", "Save Lyrics")
        fileMode: QtDialogs.FileDialog.SaveFile
        defaultSuffix: "txt"
        nameFilters: [KI18n.i18n("Text files (*.txt)"), KI18n.i18n("All files (*)")]
        onAccepted: Player.saveLyrics(selectedFile)
    }
}
