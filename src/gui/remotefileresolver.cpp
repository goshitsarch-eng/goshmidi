/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "remotefileresolver.hpp"

#include "../midi/events.hpp"

#include <KIO/FileCopyJob>
#include <KIO/StatJob>
#include <KLocalizedString>

#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStringList>

namespace dmidi {
namespace {

// Same convention the player uses locally: a SysEx dump named after the song,
// otherwise a shared Folder.syx sitting beside it.
QStringList companionSysexNames(const QString& midiFileName)
{
    const QString base = QFileInfo(midiFileName).completeBaseName();
    QStringList names;
    if (!base.isEmpty())
        names << base + QStringLiteral(".syx");
    names << QStringLiteral("Folder.syx");
    return names;
}

} // namespace

RemoteFileResolver::RemoteFileResolver(QObject* parent)
    : QObject(parent)
    , m_cache(QDir::tempPath() + QStringLiteral("/goshmidi-remote-XXXXXX"))
{
    m_cache.setAutoRemove(true);
}

RemoteFileResolver::~RemoteFileResolver()
{
    cancel();
}

QString RemoteFileResolver::locatorFromUrl(const QUrl& url)
{
    if (url.isEmpty())
        return {};
    if (url.isLocalFile())
        return url.toLocalFile();
    if (url.scheme().isEmpty())
        return url.path();
    return url.toString();
}

QUrl RemoteFileResolver::urlFromLocator(const QString& locator)
{
    if (locator.isEmpty())
        return {};
    if (isRemote(locator))
        return QUrl(locator);
    return QUrl::fromLocalFile(locator);
}

bool RemoteFileResolver::isRemote(const QString& locator)
{
    return dmidi::isRemoteLocator(locator.toStdString());
}

QString RemoteFileResolver::displayName(const QString& locator)
{
    const QString name = QString::fromStdString(dmidi::locatorFileName(locator.toStdString()));
    return name.isEmpty() ? locator : QUrl::fromPercentEncoding(name.toUtf8());
}

QString RemoteFileResolver::localPathFor(const QString& locator) const
{
    if (locator.isEmpty())
        return {};
    if (!isRemote(locator))
        return locator;
    const QString cached = m_resolved.value(locator);
    return QFileInfo::exists(cached) ? cached : QString();
}

QString RemoteFileResolver::cacheDirFor(const QString& locator)
{
    // One directory per locator, so a companion .syx can sit next to the song
    // under its real name without two shares colliding.
    const QByteArray digest =
        QCryptographicHash::hash(locator.toUtf8(), QCryptographicHash::Sha1).toHex().left(16);
    const QString dir = m_cache.filePath(QString::fromLatin1(digest));
    QDir().mkpath(dir);
    return dir;
}

void RemoteFileResolver::resolve(const QString& locator)
{
    cancel();
    if (locator.isEmpty()) {
        Q_EMIT failed(locator, i18n("No file to open."));
        return;
    }

    if (!isRemote(locator)) {
        Q_EMIT resolved(locator, locator);
        return;
    }

    const QString cached = localPathFor(locator);
    if (!cached.isEmpty()) {
        Q_EMIT resolved(locator, cached);
        return;
    }

    const QUrl url(locator);
    if (!url.isValid()) {
        Q_EMIT failed(locator, i18n("“%1” is not a valid address.", locator));
        return;
    }

    m_pending = locator;

    // A share that is already mounted locally (or a URL such as desktop:/ that
    // merely points at one) needs no copy at all.
    auto* statJob = KIO::mostLocalUrl(url, KIO::HideProgressInfo);
    m_job = statJob;
    connect(statJob, &KJob::result, this, [this, statJob, url] {
        m_job.clear();
        if (m_pending.isEmpty())
            return;
        if (!statJob->error()) {
            const QUrl local = statJob->mostLocalUrl();
            if (local.isLocalFile() && QFileInfo::exists(local.toLocalFile())) {
                finish(local.toLocalFile());
                return;
            }
        }
        startCopy(url);
    });
}

void RemoteFileResolver::startCopy(const QUrl& source)
{
    const QString locator = m_pending;
    QString name = QUrl::fromPercentEncoding(source.fileName().toUtf8());
    if (name.isEmpty())
        name = QStringLiteral("download.mid");
    const QString target = cacheDirFor(locator) + QLatin1Char('/') + name;

    auto* job = KIO::file_copy(source,
                               QUrl::fromLocalFile(target),
                               -1,
                               KIO::Overwrite | KIO::HideProgressInfo);
    m_job = job;
    connect(job, &KJob::percentChanged, this, [this, locator](KJob*, unsigned long percent) {
        if (m_pending == locator)
            Q_EMIT progress(locator, static_cast<int>(percent));
    });
    connect(job, &KJob::result, this, [this, job, source, target] {
        m_job.clear();
        if (m_pending.isEmpty())
            return;
        if (job->error()) {
            fail(job->errorString());
            return;
        }
        fetchCompanionSysex(source, target, 0);
    });
}

void RemoteFileResolver::fetchCompanionSysex(const QUrl& source, const QString& localPath,
                                             int candidate)
{
    // Best effort: the song plays with or without its SysEx dump, so a failure
    // here simply means the share did not carry one.
    const QStringList candidates = companionSysexNames(QFileInfo(localPath).fileName());
    if (candidate >= candidates.size()) {
        finish(localPath);
        return;
    }

    const QString locator = m_pending;
    const QString name = candidates.at(candidate);
    QUrl remote = source.adjusted(QUrl::RemoveFilename);
    remote.setPath(remote.path() + name);
    const QString target = QFileInfo(localPath).absolutePath() + QLatin1Char('/') + name;

    auto* job = KIO::file_copy(remote,
                               QUrl::fromLocalFile(target),
                               -1,
                               KIO::Overwrite | KIO::HideProgressInfo);
    m_job = job;
    connect(job, &KJob::result, this, [this, job, source, localPath, target, locator, candidate] {
        m_job.clear();
        if (m_pending != locator)
            return;
        if (job->error()) {
            QFile::remove(target);
            fetchCompanionSysex(source, localPath, candidate + 1);
            return;
        }
        // Found one; the player picks it up from beside the song.
        finish(localPath);
    });
}

void RemoteFileResolver::finish(const QString& localPath)
{
    const QString locator = m_pending;
    m_pending.clear();
    if (locator.isEmpty())
        return;
    m_resolved.insert(locator, localPath);
    Q_EMIT resolved(locator, localPath);
}

void RemoteFileResolver::fail(const QString& message)
{
    const QString locator = m_pending;
    m_pending.clear();
    if (locator.isEmpty())
        return;
    Q_EMIT failed(locator, message);
}

void RemoteFileResolver::cancel()
{
    m_pending.clear();
    if (m_job) {
        m_job->kill();
        m_job.clear();
    }
}

} // namespace dmidi
