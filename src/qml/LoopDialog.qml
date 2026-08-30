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

    title: KI18n.i18nc("@title:window", "Loop Between Bars")
    standardButtons: Kirigami.Dialog.NoButton

    onOpened: {
        fromBar.value = Player.loopStart;
        toBar.value = Player.loopEnd;
    }

    customFooterActions: [
        Kirigami.Action {
            text: KI18n.i18nc("@action:button", "Loop")
            icon.name: "media-playlist-repeat-symbolic"
            onTriggered: {
                Player.setLoopRange(fromBar.value, toBar.value);
                Player.loopEnabled = true;
                dialog.close();
            }
        },
        Kirigami.Action {
            text: KI18n.i18nc("@action:button Stop looping", "Turn Off")
            icon.name: "dialog-cancel-symbolic"
            onTriggered: {
                Player.loopEnabled = false;
                dialog.close();
            }
        }
    ]

    GridLayout {
        columns: 2
        columnSpacing: Kirigami.Units.largeSpacing
        rowSpacing: Kirigami.Units.smallSpacing

        QQC2.Label {
            text: KI18n.i18nc("@label:spinbox", "From bar:")
        }

        QQC2.SpinBox {
            id: fromBar
            from: 1
            to: Math.max(1, Player.lastBar)
            editable: true
        }

        QQC2.Label {
            text: KI18n.i18nc("@label:spinbox", "To bar:")
        }

        QQC2.SpinBox {
            id: toBar
            from: fromBar.value
            to: Math.max(1, Player.lastBar)
            editable: true
        }
    }
}
