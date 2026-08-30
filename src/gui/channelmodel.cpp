/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "channelmodel.hpp"

#include <algorithm>

namespace dmidi {

ChannelModel::ChannelModel(QObject* parent)
    : QAbstractListModel(parent)
{
}

int ChannelModel::rowCount(const QModelIndex& parent) const
{
    return parent.isValid() ? 0 : kMidiChannels;
}

QVariant ChannelModel::data(const QModelIndex& index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= kMidiChannels)
        return {};
    const Channel& ch = m_channels[index.row()];
    switch (role) {
    case ChannelRole:
        return index.row() + 1;
    case UsedRole:
        return ch.used;
    case Qt::DisplayRole:
    case NameRole:
        return ch.name;
    case MutedRole:
        return ch.muted;
    case SoloRole:
        return ch.solo;
    case LockedRole:
        return ch.locked;
    case PatchRole:
        return ch.patch;
    case LevelRole:
        return ch.level;
    case VolumeRole:
        return ch.volume;
    case PianoVisibleRole:
        return ch.pianoVisible;
    default:
        return {};
    }
}

bool ChannelModel::setData(const QModelIndex& index, const QVariant& value, int role)
{
    if (!index.isValid() || index.row() < 0 || index.row() >= kMidiChannels)
        return false;
    const int row = index.row();
    Channel& ch = m_channels[row];

    switch (role) {
    case NameRole: {
        const QString name = value.toString();
        if (ch.name == name)
            return false;
        ch.name = name;
        emitChanged(row, {NameRole});
        return true;
    }
    case MutedRole:
        if (ch.muted == value.toBool())
            return false;
        ch.muted = value.toBool();
        emitChanged(row, {MutedRole});
        Q_EMIT mixChanged();
        return true;
    case SoloRole:
        if (ch.solo == value.toBool())
            return false;
        ch.solo = value.toBool();
        emitChanged(row, {SoloRole});
        Q_EMIT mixChanged();
        return true;
    case LockedRole:
        if (ch.locked == value.toBool())
            return false;
        ch.locked = value.toBool();
        emitChanged(row, {LockedRole});
        Q_EMIT lockRequested(row, ch.locked);
        return true;
    case PatchRole: {
        const int patch = clampMidi(value.toInt());
        if (ch.patch == patch)
            return false;
        ch.patch = patch;
        emitChanged(row, {PatchRole});
        Q_EMIT patchRequested(row, patch);
        return true;
    }
    case VolumeRole: {
        const int volume = std::clamp(value.toInt(), 0, 200);
        if (ch.volume == volume)
            return false;
        ch.volume = volume;
        emitChanged(row, {VolumeRole});
        Q_EMIT mixChanged();
        return true;
    }
    case PianoVisibleRole:
        if (ch.pianoVisible == value.toBool())
            return false;
        ch.pianoVisible = value.toBool();
        emitChanged(row, {PianoVisibleRole});
        Q_EMIT pianoVisibilityChanged();
        return true;
    default:
        return false;
    }
}

Qt::ItemFlags ChannelModel::flags(const QModelIndex& index) const
{
    if (!index.isValid())
        return Qt::NoItemFlags;
    return QAbstractListModel::flags(index) | Qt::ItemIsEditable;
}

QHash<int, QByteArray> ChannelModel::roleNames() const
{
    return {
        {ChannelRole, "channel"},
        {UsedRole, "used"},
        {NameRole, "name"},
        {MutedRole, "muted"},
        {SoloRole, "solo"},
        {LockedRole, "locked"},
        {PatchRole, "patch"},
        {LevelRole, "level"},
        {VolumeRole, "volume"},
        {PianoVisibleRole, "pianoVisible"},
    };
}

bool ChannelModel::anySolo() const
{
    return std::any_of(std::begin(m_channels), std::end(m_channels), [](const Channel& ch) {
        return ch.solo;
    });
}

void ChannelModel::emitChanged(int row, const QList<int>& roles)
{
    Q_EMIT dataChanged(index(row), index(row), roles);
}

void ChannelModel::resetForSong(const std::function<bool(int)>& used,
                                const std::function<QString(int)>& label)
{
    beginResetModel();
    for (int i = 0; i < kMidiChannels; ++i) {
        Channel& ch = m_channels[i];
        ch.used = used(i);
        ch.name = ch.used ? label(i) : QString();
        ch.muted = false;
        ch.solo = false;
        ch.locked = false;
        ch.patch = 0;
        ch.level = 0;
        ch.volume = 100;
        ch.pianoVisible = true;
    }
    endResetModel();
    Q_EMIT pianoVisibilityChanged();
}

void ChannelModel::setPatchSilently(int channel, int patch)
{
    if (channel < 0 || channel >= kMidiChannels)
        return;
    const int value = clampMidi(patch);
    if (m_channels[channel].patch == value)
        return;
    m_channels[channel].patch = value;
    emitChanged(channel, {PatchRole});
}

void ChannelModel::bumpLevel(int channel, double level)
{
    if (channel < 0 || channel >= kMidiChannels)
        return;
    m_channels[channel].level = std::max(m_channels[channel].level, level);
}

void ChannelModel::decayLevels(double factor)
{
    for (int i = 0; i < kMidiChannels; ++i) {
        if (m_channels[i].level <= 0.001) {
            if (m_channels[i].level != 0) {
                m_channels[i].level = 0;
                emitChanged(i, {LevelRole});
            }
            continue;
        }
        m_channels[i].level *= factor;
        emitChanged(i, {LevelRole});
    }
}

void ChannelModel::setName(int row, const QString& name)
{
    setData(index(row), name, NameRole);
}

void ChannelModel::setMuted(int row, bool muted)
{
    setData(index(row), muted, MutedRole);
}

void ChannelModel::setSolo(int row, bool solo)
{
    setData(index(row), solo, SoloRole);
}

void ChannelModel::setLocked(int row, bool locked)
{
    setData(index(row), locked, LockedRole);
}

void ChannelModel::setPatch(int row, int patch)
{
    setData(index(row), patch, PatchRole);
}

void ChannelModel::setVolume(int row, int volume)
{
    setData(index(row), volume, VolumeRole);
}

void ChannelModel::setPianoVisible(int row, bool visible)
{
    setData(index(row), visible, PianoVisibleRole);
}

void ChannelModel::showOnlyUsedChannels()
{
    for (int i = 0; i < kMidiChannels; ++i) {
        if (m_channels[i].pianoVisible != m_channels[i].used) {
            m_channels[i].pianoVisible = m_channels[i].used;
            emitChanged(i, {PianoVisibleRole});
        }
    }
    Q_EMIT pianoVisibilityChanged();
}

void ChannelModel::showAllChannels()
{
    for (int i = 0; i < kMidiChannels; ++i) {
        if (!m_channels[i].pianoVisible) {
            m_channels[i].pianoVisible = true;
            emitChanged(i, {PianoVisibleRole});
        }
    }
    Q_EMIT pianoVisibilityChanged();
}

} // namespace dmidi
