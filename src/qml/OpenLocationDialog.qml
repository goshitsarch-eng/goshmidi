// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

// Opens a song straight from an address, so a share reachable by KIO can be
// played even when it is not mounted anywhere in the filesystem.
Kirigami.PromptDialog {
    id: dialog

    title: KI18n.i18nc("@title:window", "Open Location")
    standardButtons: Kirigami.Dialog.NoButton
    preferredWidth: Kirigami.Units.gridUnit * 26

    onOpened: {
        address.clear();
        address.forceActiveFocus();
    }

    customFooterActions: [
        Kirigami.Action {
            text: KI18n.i18nc("@action:button", "Open")
            icon.name: "document-open-symbolic"
            enabled: address.text.length > 0
            onTriggered: dialog.accept()
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:button", "Cancel")
            icon.name: "dialog-cancel-symbolic"
            onTriggered: dialog.close()
        }
    ]

    function accept(): void {
        Player.openUrls([address.text], true);
        dialog.close();
    }

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: KI18n.i18nc("@info", "Enter the address of a MIDI file or playlist.")
        }

        QQC2.TextField {
            id: address
            Layout.fillWidth: true
            placeholderText: "smb://server/share/song.mid"
            onAccepted: dialog.accept()
        }

        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            opacity: 0.7
            font: Kirigami.Theme.smallFont
            text: KI18n.i18nc("@info", "Windows shares (smb://), SSH (sftp://, fish://), "
                + "WebDAV (dav://) and any other address your system can reach are supported.")
        }
    }
}
