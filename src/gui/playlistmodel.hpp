/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#pragma once

#include "../app/playlist.hpp"

#include <QAbstractListModel>
#include <QQmlEngine>

namespace dmidi {

class PlaylistModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("The playlist model is owned by Player.")

    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
    Q_PROPERTY(int currentIndex READ currentIndex NOTIFY currentIndexChanged)
    Q_PROPERTY(QString fileName READ fileName NOTIFY fileNameChanged)
    Q_PROPERTY(bool dirty READ dirty NOTIFY countChanged)

public:
    enum Role {
        NameRole = Qt::UserRole + 1,
        LocatorRole,
        LocationRole,
        RemoteRole,
        CurrentRole,
    };
    Q_ENUM(Role)

    explicit PlaylistModel(Playlist* playlist, QObject* parent = nullptr);

    int rowCount(const QModelIndex& parent = {}) const override;
    QVariant data(const QModelIndex& index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    int currentIndex() const { return m_playlist->currentIndex(); }
    QString fileName() const;
    bool dirty() const { return m_playlist->dirty(); }

    // Any change made through the Playlist object directly must be announced
    // with one of these so the view stays in step.
    void reload();
    void notifyCurrentChanged();

Q_SIGNALS:
    void countChanged();
    void currentIndexChanged();
    void fileNameChanged();

private:
    Playlist* m_playlist{};
};

} // namespace dmidi
