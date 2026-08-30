/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "pianoview.hpp"

#include "appconfig.hpp"

#include <QPainter>

#include <algorithm>

namespace dmidi {
namespace {

bool isBlackKey(int note)
{
    switch (note % 12) {
    case 1:
    case 3:
    case 6:
    case 8:
    case 10:
        return true;
    default:
        return false;
    }
}

QString noteName(int note, bool octaveSubscript)
{
    static const char* const kNames[] = {"C", "C♯", "D", "D♯", "E", "F",
                                         "F♯", "G", "G♯", "A", "A♯", "B"};
    static const QChar kSubscripts[] = {u'₀', u'₁', u'₂', u'₃', u'₄',
                                        u'₅', u'₆', u'₇', u'₈', u'₉'};
    const int octave = note / 12 - 1;
    const QString name = QString::fromUtf8(kNames[note % 12]);
    if (octaveSubscript && octave >= 0 && octave <= 9)
        return name + kSubscripts[octave];
    return name + QString::number(octave);
}

// The per-channel palette, echoing the colours the previous release used.
const QColor& channelPalette(int channel)
{
    static const QColor kPalette[kMidiChannels] = {
        QColor(0xe0, 0x1b, 0x24), QColor(0xff, 0x78, 0x00), QColor(0xf6, 0xd3, 0x2d),
        QColor(0x33, 0xd1, 0x7a), QColor(0x35, 0x84, 0xe4), QColor(0x91, 0x41, 0xac),
        QColor(0xc0, 0x61, 0xcb), QColor(0x62, 0xa0, 0xea), QColor(0x26, 0xa2, 0x69),
        QColor(0xe5, 0xa5, 0x0a), QColor(0xc0, 0x1c, 0x28), QColor(0x1c, 0x71, 0xd8),
        QColor(0x61, 0x35, 0x83), QColor(0x9a, 0x99, 0x96), QColor(0x77, 0x76, 0x7b),
        QColor(0xf6, 0x61, 0x51),
    };
    return kPalette[channel & 15];
}

} // namespace

PianoView::PianoView(QQuickItem* parent)
    : QQuickPaintedItem(parent)
{
    setFlag(ItemHasContents, true);
    connect(this, &PianoView::paletteChanged, this, [this] { update(); });
    connect(AppConfig::instance(), &AppConfig::changed, this, [this] { update(); });
}

void PianoView::setController(PlayerController* controller)
{
    if (m_controller == controller)
        return;
    if (m_controller)
        m_controller->disconnect(this);
    m_controller = controller;
    if (m_controller) {
        connect(m_controller, &PlayerController::noteOn, this, &PianoView::noteOn);
        connect(m_controller, &PlayerController::noteOff, this, &PianoView::noteOff);
        connect(m_controller, &PlayerController::notesCleared, this, &PianoView::clearNotes);
        connect(m_controller, &PlayerController::songChanged, this, [this] {
            clearNotes();
            relayout();
        });
    }
    Q_EMIT controllerChanged();
    relayout();
}

void PianoView::setChannels(ChannelModel* channels)
{
    if (m_channels == channels)
        return;
    if (m_channels)
        m_channels->disconnect(this);
    m_channels = channels;
    if (m_channels) {
        connect(m_channels, &ChannelModel::pianoVisibilityChanged, this, &PianoView::relayout);
        connect(m_channels, &ChannelModel::modelReset, this, &PianoView::relayout);
    }
    Q_EMIT channelsChanged();
    relayout();
}

void PianoView::setTightenKeys(bool tighten)
{
    if (m_tighten == tighten)
        return;
    m_tighten = tighten;
    Q_EMIT tightenKeysChanged();
    update();
}

void PianoView::setRowHeight(qreal height)
{
    const qreal value = std::max<qreal>(32, height);
    if (qFuzzyCompare(m_rowHeight, value))
        return;
    m_rowHeight = value;
    Q_EMIT rowHeightChanged();
    relayout();
}

QList<int> PianoView::rowChannels() const
{
    QList<int> rows;
    if (!m_channels)
        return rows;
    for (int channel = 0; channel < kMidiChannels; ++channel) {
        const auto& info = m_channels->channel(channel);
        if (info.used && info.pianoVisible)
            rows << channel;
    }
    return rows;
}

int PianoView::visibleRows() const
{
    return std::max(1, int(rowChannels().size()));
}

void PianoView::relayout()
{
    setImplicitHeight(visibleRows() * m_rowHeight);
    Q_EMIT layoutChanged();
    update();
}

PianoView::Range PianoView::range() const
{
    Range r;
    if (m_tighten && m_controller && m_controller->hasSong()) {
        r.lowest = std::max(0, m_controller->lowestNote() - 2);
        r.highest = std::min(127, m_controller->highestNote() + 2);
        if (r.highest <= r.lowest) {
            r.lowest = 21;
            r.highest = 108;
        }
    }
    r.whiteKeys = 0;
    for (int note = r.lowest; note <= r.highest; ++note) {
        if (!isBlackKey(note))
            ++r.whiteKeys;
    }
    r.whiteKeys = std::max(1, r.whiteKeys);
    return r;
}

QColor PianoView::noteColor(int channel, int velocity) const
{
    auto* config = AppConfig::instance();
    QColor color = config->highlightPalette() == 0 ? config->singleColor()
                                                   : channelPalette(channel);
    if (config->velocityColor()) {
        const double t = 0.45 + 0.55 * (std::clamp(velocity, 0, 127) / 127.0);
        color = QColor::fromRgbF(color.redF() * t, color.greenF() * t, color.blueF() * t);
    }
    return color;
}

void PianoView::paint(QPainter* painter)
{
    painter->setRenderHint(QPainter::Antialiasing, true);
    painter->fillRect(boundingRect(), m_background);

    const QList<int> rows = rowChannels();
    if (rows.isEmpty())
        return;

    auto* config = AppConfig::instance();
    const Range keys = range();
    const qreal margin = 4;
    const qreal whiteWidth = std::max<qreal>(2.0, (width() - margin * 2) / keys.whiteKeys);
    const qreal labelStrip = 18;
    painter->setFont(m_font);

    for (int row = 0; row < rows.size(); ++row) {
        const int channel = rows.at(row);
        const qreal top = row * m_rowHeight;
        const qreal keyTop = top + 8;
        const qreal keyHeight = std::max<qreal>(16, m_rowHeight - labelStrip - 11);

        // White keys first, black ones on top of them.
        qreal x = margin;
        for (int note = keys.lowest; note <= keys.highest; ++note) {
            if (isBlackKey(note))
                continue;
            const bool lit = m_notesOn[channel][note];
            const QRectF key(x, keyTop, whiteWidth - 1, keyHeight);
            painter->setBrush(lit ? noteColor(channel, m_velocity[channel][note]) : m_whiteKey);
            painter->setPen(QPen(m_keyBorder, 1));
            painter->drawRoundedRect(key, 2, 2);

            const int visibility = config->namesVisibility();
            const bool showName = visibility == 3 || (visibility == 1 && note % 12 == 0)
                || (visibility == 2 && lit);
            if (showName && whiteWidth > 9) {
                painter->setPen(m_label);
                painter->drawText(QRectF(x, keyTop + keyHeight - 15, whiteWidth, 14),
                                  Qt::AlignHCenter | Qt::AlignVCenter,
                                  noteName(note, config->octaveSubscript()));
            }
            x += whiteWidth;
        }

        x = margin;
        painter->setPen(Qt::NoPen);
        for (int note = keys.lowest; note <= keys.highest; ++note) {
            if (!isBlackKey(note)) {
                x += whiteWidth;
                continue;
            }
            const bool lit = m_notesOn[channel][note];
            const QRectF key(x - whiteWidth * 0.32, keyTop, whiteWidth * 0.64, keyHeight * 0.62);
            painter->setBrush(lit ? noteColor(channel, m_velocity[channel][note]) : m_blackKey);
            painter->drawRoundedRect(key, 2, 2);
        }

        painter->setPen(m_channelLabel);
        const QString name = m_channels ? m_channels->channel(channel).name : QString();
        const QString caption = name.isEmpty()
            ? QStringLiteral("%1").arg(channel + 1)
            : QStringLiteral("%1 · %2").arg(channel + 1).arg(name);
        painter->drawText(QRectF(margin, top + m_rowHeight - labelStrip, width() - margin * 2,
                                 labelStrip),
                          Qt::AlignLeft | Qt::AlignVCenter, caption);
    }
}

int PianoView::channelAt(qreal y) const
{
    const QList<int> rows = rowChannels();
    if (rows.isEmpty() || m_rowHeight <= 0)
        return -1;
    const int row = int(y / m_rowHeight);
    if (row < 0 || row >= rows.size())
        return -1;
    return rows.at(row);
}

int PianoView::noteAt(qreal x, qreal y) const
{
    if (channelAt(y) < 0)
        return -1;

    const Range keys = range();
    const qreal margin = 4;
    const qreal whiteWidth = std::max<qreal>(2.0, (width() - margin * 2) / keys.whiteKeys);
    const qreal keyTop = int(y / m_rowHeight) * m_rowHeight + 8;
    const qreal keyHeight = std::max<qreal>(16, m_rowHeight - 29);

    // Black keys sit above the white ones, so test them first.
    if (y - keyTop <= keyHeight * 0.62) {
        qreal cursor = margin;
        for (int note = keys.lowest; note <= keys.highest; ++note) {
            if (!isBlackKey(note)) {
                cursor += whiteWidth;
                continue;
            }
            const qreal left = cursor - whiteWidth * 0.32;
            if (x >= left && x <= left + whiteWidth * 0.64)
                return note;
        }
    }

    const int index = int((x - margin) / whiteWidth);
    int seen = 0;
    for (int note = keys.lowest; note <= keys.highest; ++note) {
        if (isBlackKey(note))
            continue;
        if (seen == index)
            return note;
        ++seen;
    }
    return -1;
}

void PianoView::noteOn(int channel, int note, int velocity)
{
    if (channel < 0 || channel >= kMidiChannels || note < 0 || note > 127)
        return;
    m_notesOn[channel][note] = velocity > 0;
    m_velocity[channel][note] = velocity;
    update();
}

void PianoView::noteOff(int channel, int note)
{
    if (channel < 0 || channel >= kMidiChannels || note < 0 || note > 127)
        return;
    m_notesOn[channel][note] = false;
    update();
}

void PianoView::clearNotes()
{
    std::fill(&m_notesOn[0][0], &m_notesOn[0][0] + kMidiChannels * 128, false);
    update();
}

} // namespace dmidi
