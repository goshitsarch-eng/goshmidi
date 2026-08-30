// SPDX-FileCopyrightText: 2006-2026 Pedro López-Cabanillas and contributors
// SPDX-License-Identifier: GPL-3.0-or-later

import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// One lamp per beat in the bar; the downbeat is picked out in the accent colour.
RowLayout {
    id: lamps

    property int beats: 4
    property int activeBeat: 1
    property bool running: false

    // Each entry is a lamp's state, so the delegates need nothing from this
    // scope beyond their own model data.
    readonly property var lampStates: {
        const states = [];
        for (let beat = 1; beat <= Math.max(1, lamps.beats); ++beat) {
            states.push(!lamps.running || lamps.activeBeat !== beat
                ? "off"
                : (beat === 1 ? "downbeat" : "beat"));
        }
        return states;
    }

    spacing: Kirigami.Units.smallSpacing

    Repeater {
        model: lamps.lampStates

        Rectangle {
            required property string modelData

            implicitWidth: Kirigami.Units.gridUnit * 0.75
            implicitHeight: Kirigami.Units.smallSpacing * 1.5
            radius: height / 2

            color: switch (modelData) {
                case "downbeat": return Kirigami.Theme.negativeTextColor;
                case "beat": return Kirigami.Theme.highlightColor;
                default: return Qt.alpha(Kirigami.Theme.textColor, 0.15);
            }

            Behavior on color {
                ColorAnimation {
                    duration: Kirigami.Units.shortDuration
                }
            }
        }
    }

    Item {
        Layout.fillWidth: true
    }
}
