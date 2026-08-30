/*
    Gosh MIDI Player — Qt6/Kirigami

    One keyboard row per audible MIDI channel, lit as the song plays. Painted in
    C++ because a QML item per key would mean well over a thousand of them.
*/

#pragma once

#include "channelmodel.hpp"
#include "playercontroller.hpp"

#include <QColor>
#include <QFont>
#include <QQmlEngine>
#include <QQuickPaintedItem>

namespace dmidi {

class PianoView : public QQuickPaintedItem
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(dmidi::PlayerController* controller READ controller WRITE setController NOTIFY controllerChanged)
    Q_PROPERTY(dmidi::ChannelModel* channels READ channels WRITE setChannels NOTIFY channelsChanged)
    Q_PROPERTY(bool tightenKeys READ tightenKeys WRITE setTightenKeys NOTIFY tightenKeysChanged)
    Q_PROPERTY(qreal rowHeight READ rowHeight WRITE setRowHeight NOTIFY rowHeightChanged)
    Q_PROPERTY(int visibleRows READ visibleRows NOTIFY layoutChanged)

    // Colours come from Kirigami.Theme so the keyboard follows the colour
    // scheme in both light and dark.
    Q_PROPERTY(QColor backgroundColor MEMBER m_background NOTIFY paletteChanged)
    Q_PROPERTY(QColor whiteKeyColor MEMBER m_whiteKey NOTIFY paletteChanged)
    Q_PROPERTY(QColor blackKeyColor MEMBER m_blackKey NOTIFY paletteChanged)
    Q_PROPERTY(QColor keyBorderColor MEMBER m_keyBorder NOTIFY paletteChanged)
    Q_PROPERTY(QColor labelColor MEMBER m_label NOTIFY paletteChanged)
    Q_PROPERTY(QColor channelLabelColor MEMBER m_channelLabel NOTIFY paletteChanged)
    Q_PROPERTY(QFont labelFont MEMBER m_font NOTIFY paletteChanged)

public:
    explicit PianoView(QQuickItem* parent = nullptr);

    void paint(QPainter* painter) override;

    PlayerController* controller() const { return m_controller; }
    void setController(PlayerController* controller);
    ChannelModel* channels() const { return m_channels; }
    void setChannels(ChannelModel* channels);
    bool tightenKeys() const { return m_tighten; }
    void setTightenKeys(bool tighten);
    qreal rowHeight() const { return m_rowHeight; }
    void setRowHeight(qreal height);
    int visibleRows() const;

    // Hit testing for the QML mouse handler.
    Q_INVOKABLE int channelAt(qreal y) const;
    Q_INVOKABLE int noteAt(qreal x, qreal y) const;

Q_SIGNALS:
    void controllerChanged();
    void channelsChanged();
    void tightenKeysChanged();
    void rowHeightChanged();
    void layoutChanged();
    void paletteChanged();

private:
    struct Range {
        int lowest{21};
        int highest{108};
        int whiteKeys{52};
    };

    void noteOn(int channel, int note, int velocity);
    void noteOff(int channel, int note);
    void clearNotes();
    void relayout();
    QList<int> rowChannels() const;
    Range range() const;
    QColor noteColor(int channel, int velocity) const;

    PlayerController* m_controller{};
    ChannelModel* m_channels{};
    bool m_tighten{};
    qreal m_rowHeight{72};

    QColor m_background{QColor(0x24, 0x1f, 0x31)};
    QColor m_whiteKey{QColor(0xf2, 0xf2, 0xef)};
    QColor m_blackKey{QColor(0x1a, 0x1a, 0x1f)};
    QColor m_keyBorder{QColor(0x33, 0x33, 0x33)};
    QColor m_label{QColor(0x1a, 0x1a, 0x1a)};
    QColor m_channelLabel{QColor(0xe6, 0xe6, 0xe6)};
    QFont m_font;

    bool m_notesOn[kMidiChannels][128]{};
    int m_velocity[kMidiChannels][128]{};
};

} // namespace dmidi
