// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.ki18n
import org.kde.kirigami as Kirigami
import org.goshapps.goshmidi

Kirigami.PromptDialog {
    id: dialog

    title: KI18n.i18nc("@title:window", "Jump to Bar")
    standardButtons: Kirigami.Dialog.NoButton

    onOpened: bar.value = 1

    customFooterActions: [
        Kirigami.Action {
            text: KI18n.i18nc("@action:button", "Jump")
            icon.name: "go-jump-symbolic"
            onTriggered: {
                Player.jumpToBar(bar.value);
                dialog.close();
            }
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:button", "Cancel")
            icon.name: "dialog-cancel-symbolic"
            onTriggered: dialog.close()
        }
    ]

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            text: KI18n.i18ncp("@label:spinbox", "This song has %1 bar.", "This song has %1 bars.",
                         Player.lastBar)
        }

        QQC2.SpinBox {
            id: bar
            Layout.fillWidth: true
            from: 1
            to: Math.max(1, Player.lastBar)
            editable: true
            value: 1
        }
    }
}
