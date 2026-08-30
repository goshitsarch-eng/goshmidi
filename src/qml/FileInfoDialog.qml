// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

Kirigami.PromptDialog {
    id: dialog

    title: KI18n.i18nc("@title:window", "File Information")
    standardButtons: Kirigami.Dialog.Ok

    property string details: ""

    onOpened: details = Player.fileInformation()

    preferredWidth: Kirigami.Units.gridUnit * 26

    Kirigami.SelectableLabel {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        text: dialog.details
    }
}
