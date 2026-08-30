/*
    Gosh MIDI Player — Qt6/Kirigami

    Makes files that live on network shares playable in-app.

    The file dialog, drag & drop, the D-Bus "Open" call and the command line all
    hand over QUrls. Anything a KIO worker can reach — smb://, sftp://, fish://,
    nfs://, dav://, mtp://, google-drive:// … — has no local path, so it has to
    be resolved before the MIDI parser can touch it: either to the local file it
    already maps to, or to a copy fetched into this session's cache.
*/

#pragma once

#include <QHash>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QTemporaryDir>
#include <QUrl>

class KJob;

namespace dmidi {

class RemoteFileResolver : public QObject
{
    Q_OBJECT

public:
    explicit RemoteFileResolver(QObject* parent = nullptr);
    ~RemoteFileResolver() override;

    // Playlists, settings and the MIDI engine speak "locators": a local
    // filesystem path, or the string form of a remote URL.
    static QString locatorFromUrl(const QUrl& url);
    static QUrl urlFromLocator(const QString& locator);
    static bool isRemote(const QString& locator);
    static QString displayName(const QString& locator);

    // Local path for a locator that needs no fetching (or was already
    // fetched), otherwise an empty string.
    QString localPathFor(const QString& locator) const;

    // Resolves locator to a local path, fetching it if needed. resolved() or
    // failed() is always emitted, possibly before this call returns when the
    // answer is already known.
    void resolve(const QString& locator);

    // Abandons the in-flight fetch, if any.
    void cancel();
    bool busy() const { return !m_pending.isEmpty(); }
    QString pending() const { return m_pending; }

Q_SIGNALS:
    void resolved(const QString& locator, const QString& localPath);
    void failed(const QString& locator, const QString& message);
    void progress(const QString& locator, int percent);

private:
    void startCopy(const QUrl& source);
    void fetchCompanionSysex(const QUrl& source, const QString& localPath, int candidate);
    void finish(const QString& localPath);
    void fail(const QString& message);
    QString cacheDirFor(const QString& locator);

    QTemporaryDir m_cache;
    QHash<QString, QString> m_resolved;
    QString m_pending;
    QPointer<KJob> m_job;
};

} // namespace dmidi
