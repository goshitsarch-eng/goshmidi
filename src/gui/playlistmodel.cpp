/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "playlistmodel.hpp"

#include "../midi/events.hpp"
#include "remotefileresolver.hpp"

#include <QFileInfo>
#include <QUrl>

namespace dmidi {

PlaylistModel::PlaylistModel(Playlist* playlist, QObject* parent)
    : QAbstractListModel(parent)
    , m_playlist(playlist)
{
}

int PlaylistModel::rowCount(const QModelIndex& parent) const
{
    return parent.isValid() ? 0 : m_playlist->size();
}

QVariant PlaylistModel::data(const QModelIndex& index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_playlist->size())
        return {};

    const QString locator = QString::fromStdString(m_playlist->items().at(index.row()));
    switch (role) {
    case Qt::DisplayRole:
    case NameRole:
        return RemoteFileResolver::displayName(locator);
    case LocatorRole:
        return locator;
    case LocationRole: {
        if (RemoteFileResolver::isRemote(locator)) {
            const QUrl url(locator);
            QString host = url.host();
            if (host.isEmpty())
                host = url.scheme();
            const QString dir = QFileInfo(url.path()).path();
            return dir.isEmpty() || dir == QLatin1String("/")
                ? host
                : QStringLiteral("%1:%2").arg(host, dir);
        }
        return QFileInfo(locator).absolutePath();
    }
    case RemoteRole:
        return RemoteFileResolver::isRemote(locator);
    case CurrentRole:
        return index.row() == m_playlist->currentIndex();
    default:
        return {};
    }
}

QHash<int, QByteArray> PlaylistModel::roleNames() const
{
    return {
        {NameRole, "name"},
        {LocatorRole, "locator"},
        {LocationRole, "location"},
        {RemoteRole, "remote"},
        {CurrentRole, "current"},
    };
}

QString PlaylistModel::fileName() const
{
    const QString file = QString::fromStdString(m_playlist->fileName());
    return file.isEmpty() ? QString() : QFileInfo(file).fileName();
}

void PlaylistModel::reload()
{
    beginResetModel();
    endResetModel();
    Q_EMIT countChanged();
    Q_EMIT currentIndexChanged();
    Q_EMIT fileNameChanged();
}

void PlaylistModel::notifyCurrentChanged()
{
    if (m_playlist->size() > 0) {
        Q_EMIT dataChanged(index(0), index(m_playlist->size() - 1), {CurrentRole});
    }
    Q_EMIT currentIndexChanged();
}

} // namespace dmidi
