/*
    Gosh MIDI Player — Qt6/Kirigami

    The sixteen MIDI channels of the loaded song: name, mute/solo/lock state,
    patch, level meter and per-channel volume.
*/

#pragma once

#include "../midi/events.hpp"

#include <QAbstractListModel>
#include <QQmlEngine>
#include <QString>

#include <functional>

namespace dmidi {

class ChannelModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("The channel model is owned by Player.")

public:
    enum Role {
        ChannelRole = Qt::UserRole + 1,
        UsedRole,
        NameRole,
        MutedRole,
        SoloRole,
        LockedRole,
        PatchRole,
        LevelRole,
        VolumeRole,
        PianoVisibleRole,
    };
    Q_ENUM(Role)

    explicit ChannelModel(QObject* parent = nullptr);

    int rowCount(const QModelIndex& parent = {}) const override;
    QVariant data(const QModelIndex& index, int role) const override;
    bool setData(const QModelIndex& index, const QVariant& value, int role) override;
    Qt::ItemFlags flags(const QModelIndex& index) const override;
    QHash<int, QByteArray> roleNames() const override;

    struct Channel {
        bool used{};
        QString name;
        bool muted{};
        bool solo{};
        bool locked{};
        int patch{};
        double level{};
        int volume{100};
        bool pianoVisible{true};
    };

    const Channel& channel(int index) const { return m_channels[index]; }
    bool anySolo() const;

    // Called from QML; each mirrors one editable role.
    Q_INVOKABLE void setName(int row, const QString& name);
    Q_INVOKABLE void setMuted(int row, bool muted);
    Q_INVOKABLE void setSolo(int row, bool solo);
    Q_INVOKABLE void setLocked(int row, bool locked);
    Q_INVOKABLE void setPatch(int row, int patch);
    Q_INVOKABLE void setVolume(int row, int volume);
    Q_INVOKABLE void setPianoVisible(int row, bool visible);

    // Called by the controller as the song loads and plays.
    void resetForSong(const std::function<bool(int)>& used,
                      const std::function<QString(int)>& label);
    void setPatchSilently(int channel, int patch);
    void bumpLevel(int channel, double level);
    void decayLevels(double factor);

    // Piano keyboard visibility shortcuts, driven from the piano toolbar.
    Q_INVOKABLE void showOnlyUsedChannels();
    Q_INVOKABLE void showAllChannels();

Q_SIGNALS:
    // Emitted when the user (not playback) changes something the player has to
    // act on: mute, solo, lock, patch or volume.
    void mixChanged();
    void patchRequested(int channel, int patch);
    void lockRequested(int channel, bool locked);
    void pianoVisibilityChanged();

private:
    void emitChanged(int row, const QList<int>& roles);

    Channel m_channels[kMidiChannels];
};

} // namespace dmidi
