/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "playercontroller.hpp"

#include "../app/instruments.hpp"
#include "../app/settings.hpp"
#include "appconfig.hpp"

#include <KLocalizedString>

#include <QClipboard>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QSettings>
#include <QStandardPaths>
#include <QTextStream>
#include <QTimer>
#include <QVariantMap>

#include <algorithm>

namespace dmidi {
namespace {

AppSettings& st()
{
    return AppSettings::instance();
}

QString clockText(std::chrono::milliseconds ms)
{
    const qint64 total = ms.count() / 1000;
    return QStringLiteral("%1:%2:%3")
        .arg(total / 3600, 2, 10, QLatin1Char('0'))
        .arg((total / 60) % 60, 2, 10, QLatin1Char('0'))
        .arg(total % 60, 2, 10, QLatin1Char('0'));
}

QString escaped(const QString& text)
{
    return text.toHtmlEscaped();
}

// Karaoke line markers: '/' starts a new line and '\\' a new verse. They can sit
// anywhere in a syllable — "\\Twin", "/How ", "are./ " are all real — so the
// text is scanned character by character.
enum class LyricBreak { None, Line, Verse };

LyricBreak strongest(LyricBreak a, LyricBreak b)
{
    return a > b ? a : b;
}

const char* const kSongSettingsGroupFormat = "MIDI Channel %1";

QString songSettingsGroup(int channel)
{
    // Matches the group names earlier releases wrote, two-space padded for
    // single digits, so existing per-song .cfg files keep loading.
    return QString::fromLatin1(kSongSettingsGroupFormat)
        .arg(channel + 1, 2, 10, QLatin1Char(' '));
}

} // namespace

PlayerController::PlayerController()
    : m_status(i18n("Ready"))
{
    m_repeat = std::clamp(st().repeatMode, 0, 2);
    refreshPatchNames();
    wirePlayer();

    connect(&m_channelModel, &ChannelModel::mixChanged, this, &PlayerController::applyMix);
    connect(&m_channelModel, &ChannelModel::patchRequested, this, [this](int channel, int patch) {
        m_player.setPatch(channel, patch);
    });
    connect(&m_channelModel, &ChannelModel::lockRequested, this, [this](int channel, bool locked) {
        m_player.setLocked(channel, locked);
    });

    connect(&m_resolver, &RemoteFileResolver::resolved, this,
            [this](const QString& locator, const QString& localPath) {
                setBusy(false);
                loadResolvedFile(locator, localPath, m_pendingAutoPlay);
            });
    connect(&m_resolver, &RemoteFileResolver::failed, this,
            [this](const QString& locator, const QString& reason) {
                setBusy(false);
                setStatus(i18n("Stopped"));
                Q_EMIT errorMessage(i18n("Could not open %1: %2",
                                         RemoteFileResolver::displayName(locator), reason));
            });
    connect(&m_resolver, &RemoteFileResolver::progress, this,
            [this](const QString& locator, int percent) {
                setBusy(true, i18n("Fetching %1…", RemoteFileResolver::displayName(locator)),
                        percent);
            });

    m_meterTimer.setInterval(50);
    connect(&m_meterTimer, &QTimer::timeout, this, [this] { m_channelModel.decayLevels(0.82); });
    m_meterTimer.start();

    restoreOutput();
    loadInitialPlaylist();
}

PlayerController::~PlayerController()
{
    m_player.stop();
    st().repeatMode = m_repeat;
    st().save();
}

PlayerController* PlayerController::instance()
{
    static PlayerController controller;
    return &controller;
}

PlayerController* PlayerController::create(QQmlEngine*, QJSEngine*)
{
    auto* controller = instance();
    QQmlEngine::setObjectOwnership(controller, QQmlEngine::CppOwnership);
    return controller;
}

// --- player callbacks -------------------------------------------------------

void PlayerController::wirePlayer()
{
    m_player.onTime = [this](std::chrono::milliseconds ms, int64_t ticks) {
        m_timeText = clockText(ms);
        const int length = m_player.song().songLengthTicks();
        if (!m_seeking && length > 0)
            m_position = std::clamp(double(ticks) / length, 0.0, 1.0);
        Q_EMIT positionChanged();
    };
    m_player.onBeat = [this](int bar, int beat, int max) {
        m_barBeat = QStringLiteral("%1:%2").arg(bar).arg(beat);
        m_beat = beat;
        m_beatsPerBar = std::max(1, max);
        Q_EMIT beatChanged();
    };
    m_player.onNoteOn = [this](int channel, int note, int velocity) {
        m_channelModel.bumpLevel(channel, velocity / 127.0);
        Q_EMIT noteOn(channel, note, velocity);
    };
    m_player.onNoteOff = [this](int channel, int note, int) { Q_EMIT noteOff(channel, note); };
    m_player.onText = [this](int track, int type, int64_t ticks, const std::vector<uint8_t>&) {
        if (m_lyricTrackIndex > 0 && track != m_lyricTrackIndex)
            return;
        if (m_lyricTypeIndex > 0 && type != m_lyricTypeIndex)
            return;
        const int cursor = lyricCursorAt(ticks);
        if (cursor == m_lyricsCursor)
            return;
        m_lyricsCursor = cursor;
        rebuildLyrics();
        Q_EMIT lyricsAdvanced(m_lyricsCursor);
    };
    m_player.onProgram = [this](int channel, int program) {
        m_channelModel.setPatchSilently(channel, program);
    };
    m_player.onTempo = [this](double tempo) {
        m_bpmText = i18nc("@info beats per minute", "%1 bpm",
                          QString::number(tempoToBpm(tempo), 'f', 1));
        Q_EMIT tempoChanged();
    };
    m_player.onStarted = [this] {
        setStatus(i18n("Playing"));
        Q_EMIT playbackStateChanged();
    };
    m_player.onStopped = [this] {
        setStatus(i18n("Stopped"));
        Q_EMIT playbackStateChanged();
    };
    m_player.onFinished = [this] { handleFinished(); };
}

void PlayerController::handleFinished()
{
    setStatus(i18n("Finished"));
    Q_EMIT playbackStateChanged();

    if (m_repeat == 1) {
        m_player.resetPosition();
        play();
        return;
    }
    if (st().autoSongSettings)
        saveSongSettings();
    if (m_repeat == 2) {
        if (!m_playlist.selectNext())
            m_playlist.selectFirst();
        m_playlistModel.notifyCurrentChanged();
        loadCurrent(true);
        return;
    }
    if (st().autoAdvance && !m_playlist.atLast())
        next();
}

// --- transport --------------------------------------------------------------

bool PlayerController::isPlaying() const { return m_player.isRunning(); }
bool PlayerController::isPaused() const { return m_player.isPaused(); }
bool PlayerController::hasSong() const { return !m_player.song().empty(); }
int PlayerController::lastBar() const { return std::max(1, m_player.song().lastBar()); }
int PlayerController::lowestNote() const { return m_player.song().lowestNote(); }
int PlayerController::highestNote() const { return m_player.song().highestNote(); }
bool PlayerController::loopEnabled() const { return m_player.loopEnabled(); }
int PlayerController::loopStart() const { return m_player.loopStart(); }
int PlayerController::loopEnd() const { return m_player.loopEnd(); }

QString PlayerController::durationText() const
{
    return QString::fromStdString(m_player.song().durationString());
}

void PlayerController::play()
{
    if (m_player.song().empty()) {
        if (!m_playlist.empty())
            loadCurrent(true);
        return;
    }
    m_player.setOutput(m_outputs.current());
    m_player.play();
    setStatus(i18n("Playing"));
    Q_EMIT playbackStateChanged();
}

void PlayerController::pause()
{
    if (m_player.isRunning()) {
        m_player.pause();
        setStatus(i18n("Paused"));
        Q_EMIT playbackStateChanged();
    } else if (!m_player.song().empty()) {
        play();
    }
}

void PlayerController::stop()
{
    m_player.stop();
    m_player.resetPosition();
    m_position = 0;
    m_timeText = QStringLiteral("00:00:00");
    m_lyricsCursor = 0;
    rebuildLyrics();
    Q_EMIT notesCleared();
    Q_EMIT positionChanged();
    setStatus(i18n("Stopped"));
    Q_EMIT playbackStateChanged();
}

void PlayerController::next()
{
    if (st().autoSongSettings)
        saveSongSettings();
    if (m_playlist.selectNext()) {
        m_playlistModel.notifyCurrentChanged();
        loadCurrent(true);
    }
}

void PlayerController::previous()
{
    if (m_playlist.selectPrev()) {
        m_playlistModel.notifyCurrentChanged();
        loadCurrent(true);
    }
}

void PlayerController::forwardOneBar() { m_player.beatForward(); }
void PlayerController::rewindOneBar() { m_player.beatBackward(); }

void PlayerController::jumpToBar(int bar)
{
    if (m_player.song().empty())
        return;
    const bool wasRunning = m_player.isRunning();
    if (wasRunning)
        m_player.pause();
    m_player.jumpToBar(std::clamp(bar, 1, lastBar()));
    if (wasRunning)
        play();
}

void PlayerController::setPosition(qreal fraction)
{
    const int length = m_player.song().songLengthTicks();
    if (length <= 0)
        return;
    m_seeking = true;
    m_position = std::clamp(double(fraction), 0.0, 1.0);
    m_player.setPosition(static_cast<int64_t>(m_position * length));
    m_seeking = false;
    Q_EMIT positionChanged();
}

void PlayerController::setTempoPercent(int percent)
{
    const int value = std::clamp(percent, 50, 200);
    if (m_tempoPercent == value)
        return;
    m_tempoPercent = value;
    m_player.song().setTempoFactor(value / 100.0);
    m_bpmText = i18nc("@info beats per minute", "%1 bpm",
                      QString::number(m_player.currentBpm(), 'f', 1));
    Q_EMIT tempoChanged();
}

void PlayerController::setVolumePercent(int percent)
{
    const int value = std::clamp(percent, 0, 200);
    if (m_volumePercent == value)
        return;
    m_volumePercent = value;
    m_player.setVolumeFactor(value);
    Q_EMIT volumeChanged();
}

void PlayerController::setPitch(int semitones)
{
    const int value = std::clamp(semitones, -12, 12);
    if (m_pitch == value)
        return;
    m_pitch = value;
    m_player.setPitchShift(value);
    Q_EMIT pitchChanged();
}

void PlayerController::setLoopEnabled(bool enabled)
{
    if (m_player.loopEnabled() == enabled)
        return;
    m_player.setLoop(enabled);
    Q_EMIT loopChanged();
}

void PlayerController::setLoopRange(int fromBar, int toBar)
{
    const int last = lastBar();
    const int from = std::clamp(fromBar, 1, last);
    const int to = std::clamp(toBar, from, last);
    m_player.setLoop(from, to);
    Q_EMIT loopChanged();
}

void PlayerController::setRepeatMode(int mode)
{
    const int value = std::clamp(mode, 0, 2);
    if (m_repeat == value)
        return;
    m_repeat = value;
    st().repeatMode = value;
    Q_EMIT repeatModeChanged();
}

void PlayerController::setStatus(const QString& text)
{
    if (m_status == text)
        return;
    m_status = text;
    Q_EMIT statusChanged();
}

void PlayerController::setBusy(bool busy, const QString& text, int progress)
{
    if (m_busy == busy && m_busyText == text && m_busyProgress == progress)
        return;
    m_busy = busy;
    m_busyText = busy ? text : QString();
    m_busyProgress = busy ? progress : -1;
    Q_EMIT busyChanged();
}

// --- opening files ----------------------------------------------------------

void PlayerController::openUrls(const QList<QUrl>& urls, bool replacePlaylist)
{
    QStringList locators;
    locators.reserve(urls.size());
    for (const QUrl& url : urls) {
        const QString locator = RemoteFileResolver::locatorFromUrl(url);
        if (!locator.isEmpty())
            locators << locator;
    }
    openLocators(locators, replacePlaylist);
}

void PlayerController::addUrls(const QList<QUrl>& urls)
{
    openUrls(urls, false);
}

void PlayerController::openLocators(const QStringList& locators, bool replacePlaylist)
{
    if (locators.isEmpty())
        return;

    if (locators.size() == 1 && isPlaylistFile(locators.first().toStdString())) {
        loadPlaylist(RemoteFileResolver::urlFromLocator(locators.first()));
        return;
    }

    if (replacePlaylist)
        m_playlist.clear();

    int added = 0;
    for (const QString& locator : locators) {
        const auto before = m_playlist.size();
        m_playlist.add(locator.toStdString());
        if (m_playlist.size() > before)
            ++added;
    }
    if (added == 0) {
        m_playlistModel.reload();
        Q_EMIT errorMessage(i18n("No supported MIDI files in that selection."));
        return;
    }
    if (replacePlaylist)
        m_playlist.selectFirst();
    m_playlistModel.reload();

    // Adding to a playlist that is already playing must not interrupt it.
    if (replacePlaylist || !hasSong())
        loadCurrent(st().autoPlay);
}

void PlayerController::playIndex(int index)
{
    if (index < 0 || index >= m_playlist.size())
        return;
    m_playlist.setCurrentIndex(index);
    m_playlistModel.notifyCurrentChanged();
    loadCurrent(true);
}

void PlayerController::loadCurrent(bool autoPlay)
{
    const std::string locator = m_playlist.current();
    if (locator.empty())
        return;

    m_player.stop();
    Q_EMIT notesCleared();
    Q_EMIT playbackStateChanged();

    const QString qlocator = QString::fromStdString(locator);
    m_pendingAutoPlay = autoPlay;

    if (RemoteFileResolver::isRemote(qlocator)
        && m_resolver.localPathFor(qlocator).isEmpty()) {
        setBusy(true, i18n("Opening %1…", RemoteFileResolver::displayName(qlocator)), -1);
        setStatus(i18n("Opening…"));
    }
    m_resolver.resolve(qlocator);
}

void PlayerController::cancelPendingLoad()
{
    m_resolver.cancel();
    setBusy(false);
    setStatus(i18n("Stopped"));
}

void PlayerController::loadResolvedFile(const QString& locator, const QString& localPath,
                                        bool autoPlay)
{
    if (!m_player.loadFile(localPath.toStdString())) {
        Q_EMIT errorMessage(i18n("Could not load %1.",
                                 RemoteFileResolver::displayName(locator)));
        setStatus(i18n("Stopped"));
        return;
    }
    if (m_player.song().errorsCount() > 0)
        Q_EMIT message(i18n("“%1” is non-standard or damaged; playing what could be read.",
                            RemoteFileResolver::displayName(locator)));

    m_songLocator = locator;
    m_songIsRemote = RemoteFileResolver::isRemote(locator);
    st().addRecent(locator.toStdString());
    if (!m_songIsRemote)
        st().lastDirectory = QFileInfo(localPath).absolutePath().toStdString();

    refreshSongInfo();
    if (st().autoSongSettings)
        loadSongSettings();

    if (autoPlay)
        play();
    else
        setStatus(i18n("Stopped"));
}

void PlayerController::refreshSongInfo()
{
    auto& song = m_player.song();
    m_songTitle = QString::fromStdString(song.currentFile());

    if (m_songIsRemote) {
        const QUrl url(m_songLocator);
        m_songLocation = url.host().isEmpty() ? url.scheme() : url.host();
    } else {
        m_songLocation = QFileInfo(m_songLocator).absolutePath();
    }

    // Loading a song resets the sequence's tempo factor, so put the speed the
    // user chose back on the new song rather than letting the slider lie.
    song.setTempoFactor(m_tempoPercent / 100.0);

    m_position = 0;
    m_timeText = QStringLiteral("00:00:00");
    m_barBeat = QStringLiteral("1:1");
    m_beat = 1;
    m_bpmText = i18nc("@info beats per minute", "%1 bpm",
                      QString::number(m_player.currentBpm(), 'f', 1));

    m_channelModel.resetForSong(
        [&song](int channel) { return song.channelUsed(channel); },
        [&song](int channel) { return QString::fromStdString(song.channelLabel(channel)); });

    m_lyricsCursor = 0;
    rebuildLyrics();

    Q_EMIT songChanged();
    Q_EMIT positionChanged();
    Q_EMIT beatChanged();
    Q_EMIT tempoChanged();
    Q_EMIT loopChanged();
}

// --- playlist management ----------------------------------------------------

void PlayerController::removeAt(int index)
{
    if (index < 0 || index >= m_playlist.size())
        return;
    m_playlist.removeAt(index);
    m_playlistModel.reload();
}

void PlayerController::moveUp(int index)
{
    m_playlist.moveUp(index);
    m_playlistModel.reload();
}

void PlayerController::moveDown(int index)
{
    m_playlist.moveDown(index);
    m_playlistModel.reload();
}

void PlayerController::shufflePlaylist()
{
    m_playlist.shuffle();
    m_playlistModel.reload();
}

void PlayerController::clearPlaylist()
{
    m_playlist.clear();
    m_playlistModel.reload();
}

void PlayerController::loadPlaylist(const QUrl& url)
{
    const QString locator = RemoteFileResolver::locatorFromUrl(url);
    if (locator.isEmpty())
        return;
    if (RemoteFileResolver::isRemote(locator)) {
        Q_EMIT errorMessage(i18n("Playlists can only be opened from local storage."));
        return;
    }
    if (!m_playlist.load(locator.toStdString())) {
        Q_EMIT errorMessage(i18n("Could not read the playlist %1.", QFileInfo(locator).fileName()));
        return;
    }
    st().lastPlayList = locator.toStdString();
    m_playlistModel.reload();
    if (!m_playlist.empty())
        loadCurrent(st().autoPlay);
}

void PlayerController::savePlaylist(const QUrl& url)
{
    QString locator = RemoteFileResolver::locatorFromUrl(url);
    if (locator.isEmpty())
        return;
    if (RemoteFileResolver::isRemote(locator)) {
        Q_EMIT errorMessage(i18n("Playlists can only be saved to local storage."));
        return;
    }
    if (!locator.endsWith(QLatin1String(".lst"), Qt::CaseInsensitive))
        locator += QStringLiteral(".lst");
    if (!m_playlist.save(locator.toStdString())) {
        Q_EMIT errorMessage(i18n("Could not write the playlist %1.", QFileInfo(locator).fileName()));
        return;
    }
    st().lastPlayList = locator.toStdString();
    m_playlistModel.reload();
    Q_EMIT message(i18n("Playlist saved as %1.", QFileInfo(locator).fileName()));
}

void PlayerController::loadInitialPlaylist()
{
    if (!st().lastPlayList.empty())
        m_playlist.load(st().lastPlayList);

    if (m_playlist.empty()) {
        QStringList candidates;
#ifdef DATADIR
        candidates << QStringLiteral(DATADIR "/dmidiplayer/examples.lst");
#endif
        candidates << QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation)
                          + QStringLiteral("/dmidiplayer/examples.lst")
                   << QStringLiteral("/usr/share/dmidiplayer/examples.lst")
                   << QDir::current().filePath(QStringLiteral("examples/examples.lst"));
        for (const QString& candidate : std::as_const(candidates)) {
            if (QFileInfo::exists(candidate) && m_playlist.load(candidate.toStdString()))
                break;
        }
    }
    m_playlistModel.reload();
}

// --- lyrics -----------------------------------------------------------------

QStringList PlayerController::lyricTypes() const
{
    return {
        i18n("All types"),   i18n("Text"),        i18n("Copyright"),   i18n("Track name"),
        i18n("Instrument"),  i18n("Lyrics"),      i18n("Marker"),      i18n("Cue point"),
        i18n("KAR type"),    i18n("KAR version"), i18n("KAR info"),    i18n("KAR language"),
        i18n("KAR titles"),  i18n("KAR other"),
    };
}

void PlayerController::setLyricTrackIndex(int index)
{
    if (m_lyricTrackIndex == index)
        return;
    m_lyricTrackIndex = index;
    m_lyricsCursor = 0;
    rebuildLyrics();
}

void PlayerController::setLyricTypeIndex(int index)
{
    if (m_lyricTypeIndex == index)
        return;
    m_lyricTypeIndex = index;
    m_lyricsCursor = 0;
    rebuildLyrics();
}

void PlayerController::setEncodingIndex(int index)
{
    if (index < 0 || index >= m_encodings.size() || m_encodingIndex == index)
        return;
    m_encodingIndex = index;
    m_player.song().setCurrentCharset(m_encodings.at(index).toStdString());
    rebuildLyrics();
}

int PlayerController::lyricCursorAt(int64_t ticks) const
{
    int cursor = 0;
    for (const auto& span : m_lyricSpans) {
        if (span.tick > ticks)
            break;
        cursor = span.end;
    }
    return cursor;
}

void PlayerController::rebuildLyrics()
{
    auto& song = m_player.song();

    // Track and encoding choices follow the song unless the user overrode them.
    QStringList tracks{i18n("All tracks")};
    for (int track = 1; track <= song.getNumTracks(); ++track) {
        const QString name = QString::fromStdString(song.trackName(track));
        tracks << (name.isEmpty() ? i18n("Track %1", track)
                                  : i18nc("@item track number and name", "Track %1 — %2", track, name));
    }
    if (tracks != m_lyricTracks) {
        m_lyricTracks = tracks;
        const int best = song.trackMaxPoints();
        m_lyricTrackIndex = best > 0 && best < tracks.size() ? best : 0;
        int bestType = song.typeMaxPoints();
        m_lyricTypeIndex = bestType > 0 ? bestType : 5;
    }
    if (m_lyricTrackIndex >= m_lyricTracks.size())
        m_lyricTrackIndex = 0;

    QStringList encodings;
    for (const auto& name : Sequence::extraCodecNames())
        encodings << QString::fromStdString(name);
    const QString charset = QString::fromStdString(song.currentCharset());
    if (!charset.isEmpty() && !encodings.contains(charset, Qt::CaseInsensitive))
        encodings.prepend(charset);
    m_encodings = encodings;
    m_encodingIndex = std::max(0, int(m_encodings.indexOf(charset)));

    // Collect the visible text, then colour it in three ranges: what has been
    // sung, the word on screen now, and what is still to come.
    QString plain;
    m_lyricSpans.clear();
    const int track = m_lyricTrackIndex;
    const auto type = static_cast<TextType>(m_lyricTypeIndex);
    auto pending = LyricBreak::None;
    for (const auto& record : song.textEvents()) {
        if (track > 0 && record.track != track)
            continue;
        if (m_lyricTypeIndex > 0 && record.type != type)
            continue;

        const QString piece = QString::fromStdString(song.decodeText(record.text));
        if (record.type == TextType::Lyric || record.type == TextType::Text) {
            for (const QChar character : piece) {
                if (character == QLatin1Char('/')) {
                    pending = strongest(pending, LyricBreak::Line);
                    continue;
                }
                if (character == QLatin1Char('\\')) {
                    pending = strongest(pending, LyricBreak::Verse);
                    continue;
                }
                if (pending != LyricBreak::None) {
                    // Swallow the spacing around a marker so lines start on a
                    // word, and hold the break until real text arrives — a
                    // marker at the end of one syllable and the start of the
                    // next must still yield a single break.
                    if (character.isSpace())
                        continue;
                    if (!plain.isEmpty()) {
                        plain += pending == LyricBreak::Verse ? QStringLiteral("\n\n")
                                                             : QStringLiteral("\n");
                    }
                    pending = LyricBreak::None;
                }
                plain += character;
            }
        } else {
            plain += piece;
            if (!piece.isEmpty() && !piece.endsWith(QLatin1Char('\n')))
                plain += QLatin1Char('\n');
            pending = LyricBreak::None;
        }
        m_lyricSpans.push_back({record.tick, int(plain.size())});
    }
    m_lyricsPlain = plain;
    m_lyricsCursor = std::clamp(m_lyricsCursor, 0, int(plain.size()));

    auto* config = AppConfig::instance();
    const QString past = config->pastColor().name();
    const QString future = config->futureColor().name();
    const QString highlight = config->highlightColor().name();
    const int cursor = m_lyricsCursor;
    // Everything from the start of the current line up to the cursor reads as
    // "being sung right now".
    const int windowStart = int(plain.lastIndexOf(QLatin1Char('\n'), std::max(0, cursor - 1))) + 1;

    QString rich;
    rich.reserve(plain.size() * 2 + 128);
    rich += QStringLiteral("<span style=\"color:%1;\">").arg(past);
    rich += escaped(plain.left(windowStart)).replace(QLatin1Char('\n'), QStringLiteral("<br/>"));
    rich += QStringLiteral("</span><span style=\"color:%1;font-weight:bold;\">").arg(highlight);
    rich += escaped(plain.mid(windowStart, cursor - windowStart))
                .replace(QLatin1Char('\n'), QStringLiteral("<br/>"));
    rich += QStringLiteral("</span><span style=\"color:%1;\">").arg(future);
    rich += escaped(plain.mid(cursor)).replace(QLatin1Char('\n'), QStringLiteral("<br/>"));
    rich += QStringLiteral("</span>");
    m_lyricsRich = rich;

    Q_EMIT lyricsChanged();
}

void PlayerController::copyLyricsToClipboard()
{
    if (auto* clipboard = QGuiApplication::clipboard())
        clipboard->setText(m_lyricsPlain);
    Q_EMIT message(i18n("Lyrics copied to the clipboard."));
}

void PlayerController::saveLyrics(const QUrl& url)
{
    const QString path = RemoteFileResolver::locatorFromUrl(url);
    if (path.isEmpty() || RemoteFileResolver::isRemote(path)) {
        Q_EMIT errorMessage(i18n("Lyrics can only be saved to local storage."));
        return;
    }
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) {
        Q_EMIT errorMessage(i18n("Could not write %1.", QFileInfo(path).fileName()));
        return;
    }
    QTextStream out(&file);
    out << m_lyricsPlain;
    Q_EMIT message(i18n("Lyrics saved as %1.", QFileInfo(path).fileName()));
}

// --- channels ---------------------------------------------------------------

void PlayerController::applyMix()
{
    const bool anySolo = m_channelModel.anySolo();
    const double reduction = (100 - std::clamp(st().soloVolumeReduction, 0, 100)) / 100.0;
    for (int i = 0; i < kMidiChannels; ++i) {
        const auto& channel = m_channelModel.channel(i);
        m_player.setMuted(i, channel.muted);
        double factor = channel.volume / 100.0;
        if (anySolo && !channel.solo)
            factor *= reduction;
        m_player.setVolume(i, factor);
    }
}

void PlayerController::refreshPatchNames()
{
    const int map = std::clamp(st().instrumentMap, 0, 2);
    QStringList names;
    names.reserve(128);
    for (int patch = 0; patch < 128; ++patch)
        names << QString::fromUtf8(patchName(map, patch));
    if (names == m_patchNames)
        return;
    m_patchNames = names;
    Q_EMIT patchNamesChanged();
}

void PlayerController::playNote(int channel, int note, int velocity)
{
    if (auto* out = m_outputs.current()) {
        out->sendNoteOn(channel, note, velocity);
        m_channelModel.bumpLevel(channel, velocity / 127.0);
        Q_EMIT noteOn(channel, note, velocity);
    }
}

void PlayerController::releaseNotes()
{
    if (m_player.isRunning())
        return;
    if (auto* out = m_outputs.current()) {
        for (int channel = 0; channel < kMidiChannels; ++channel)
            out->sendController(channel, kCcAllNotesOff, 0);
    }
    Q_EMIT notesCleared();
}

// --- MIDI output ------------------------------------------------------------

QStringList PlayerController::backends() const
{
    QStringList names;
    for (const auto& name : m_outputs.backendNames())
        names << QString::fromStdString(name);
    return names;
}

QString PlayerController::currentBackend() const
{
    auto* out = m_outputs.current();
    return out ? QString::fromStdString(out->backendName()) : QString();
}

QString PlayerController::currentPort() const
{
    auto* out = m_outputs.current();
    return out ? QString::fromStdString(out->currentPort()) : QString();
}

void PlayerController::refreshPorts(const QString& backend, bool advanced)
{
    m_ports.clear();
    m_portLabels.clear();
    if (auto* out = m_outputs.find(backend.toStdString())) {
        m_ports = out->ports(advanced);
        for (const auto& port : m_ports)
            m_portLabels << QString::fromStdString(port.label);
    }
    Q_EMIT portsChanged();
}

QString PlayerController::portIdAt(int index) const
{
    if (index < 0 || index >= int(m_ports.size()))
        return {};
    return QString::fromStdString(m_ports[index].id);
}

QVariantMap PlayerController::deviceProfileFor(int portIndex) const
{
    QVariantMap profile;
    if (portIndex < 0 || portIndex >= int(m_ports.size()))
        return profile;
    const auto inferred = inferDeviceProfile(m_ports[portIndex].label);
    profile.insert(QStringLiteral("instrumentMap"), inferred.first);
    profile.insert(QStringLiteral("sysexReset"), inferred.second);
    return profile;
}

void PlayerController::connectOutput(const QString& backend, const QString& portId)
{
    m_player.stop();
    Q_EMIT playbackStateChanged();

    if (!m_outputs.select(backend.toStdString(), portId.toStdString())) {
        Q_EMIT errorMessage(i18n("Could not open the MIDI output %1.", backend));
        return;
    }
    m_player.setOutput(m_outputs.current());

    auto* out = m_outputs.current();
    st().lastOutputBackend = backend.toStdString();
    st().lastOutputConnection =
        portId.isEmpty() && out ? out->currentPort() : portId.toStdString();
    st().save();

    if (out && !out->lastError().empty()) {
        Q_EMIT errorMessage(QString::fromStdString(out->lastError()));
        m_outputSummary = QString::fromStdString(out->lastError());
    } else if (out) {
        const QString port = QString::fromStdString(out->currentPort());
        m_outputSummary = port.isEmpty()
            ? i18n("MIDI output: %1", QString::fromStdString(out->backendName()))
            : i18n("MIDI output: %1 — %2", QString::fromStdString(out->backendName()), port);
        Q_EMIT message(m_outputSummary);
    }
    Q_EMIT outputChanged();
}

void PlayerController::restoreOutput()
{
    if (!st().soundFont.empty())
        m_outputs.setSoundFont(st().soundFont);

    const std::string backend =
        st().lastOutputBackend.empty() ? "FluidSynth" : st().lastOutputBackend;
    if (!m_outputs.select(backend, st().lastOutputConnection)) {
        if (backend != "FluidSynth")
            m_outputs.select("FluidSynth", "fluidsynth");
        if (!m_outputs.current())
            m_outputs.select("Dummy", "dummy");
    }
    m_player.setOutput(m_outputs.current());
    m_player.setDrumsChannel(std::clamp(st().drumsChannel, 1, 16) - 1);
    m_player.setSysexReset(st().sysexReset);

    if (auto* out = m_outputs.current()) {
        const QString port = QString::fromStdString(out->currentPort());
        m_outputSummary = port.isEmpty()
            ? i18n("MIDI output: %1", QString::fromStdString(out->backendName()))
            : i18n("MIDI output: %1 — %2", QString::fromStdString(out->backendName()), port);
    }
    Q_EMIT outputChanged();
}

void PlayerController::applyMidiSetup(const QString& backend, int portIndex,
                                      const QString& soundFont, int instrumentMap, int sysexReset,
                                      bool advancedPorts)
{
    st().soundFont = soundFont.toStdString();
    st().advancedPorts = advancedPorts;
    st().instrumentMap = std::clamp(instrumentMap, 0, 2);
    st().sysexReset = std::clamp(sysexReset, 0, 4);
    m_player.setSysexReset(st().sysexReset);
    m_outputs.setSoundFont(st().soundFont);
    connectOutput(backend, portIdAt(portIndex));
    refreshPatchNames();
    st().save();
    AppConfig::instance()->notifyChanged();
}

void PlayerController::sendTestNote()
{
    auto* out = m_outputs.current();
    if (!out) {
        Q_EMIT errorMessage(i18n("No MIDI output is connected."));
        return;
    }
    out->sendNoteOn(0, 60, 96);
    QTimer::singleShot(400, this, [this] {
        if (auto* out = m_outputs.current())
            out->sendNoteOff(0, 60, 0);
    });
    Q_EMIT message(i18n("Sent middle C on channel 1."));
}

// --- per-song settings ------------------------------------------------------

QString PlayerController::songSettingsName() const
{
    return QString::fromStdString(m_player.song().currentFile());
}

void PlayerController::saveSongSettings()
{
    const QString name = songSettingsName();
    if (name.isEmpty())
        return;

    QSettings file(QString::fromStdString(AppSettings::songSettingsPath(name.toStdString())),
                   QSettings::IniFormat);
    file.beginGroup(QStringLiteral("Global"));
    file.setValue(QStringLiteral("file"),
                  QString::fromStdString(m_player.song().currentFullFileName()));
    file.setValue(QStringLiteral("encoding"),
                  QString::fromStdString(m_player.song().currentCharset()));
    file.setValue(QStringLiteral("volume"), m_volumePercent);
    file.setValue(QStringLiteral("pitch"), m_pitch);
    file.setValue(QStringLiteral("timeskew"), m_tempoPercent);
    file.endGroup();

    for (int i = 0; i < kMidiChannels; ++i) {
        if (!m_player.song().channelUsed(i))
            continue;
        const auto& channel = m_channelModel.channel(i);
        file.beginGroup(songSettingsGroup(i));
        file.setValue(QStringLiteral("name"), channel.name);
        file.setValue(QStringLiteral("muted"), channel.muted);
        file.setValue(QStringLiteral("solo"), channel.solo);
        file.setValue(QStringLiteral("locked"), channel.locked);
        file.setValue(QStringLiteral("patch"), channel.patch);
        file.setValue(QStringLiteral("level"), channel.volume);
        file.endGroup();
    }
    file.sync();
    Q_EMIT message(i18n("Song settings saved."));
}

void PlayerController::loadSongSettings()
{
    const QString name = songSettingsName();
    if (name.isEmpty())
        return;
    const QString path =
        QString::fromStdString(AppSettings::songSettingsPath(name.toStdString()));
    if (!QFileInfo::exists(path))
        return;

    QSettings file(path, QSettings::IniFormat);
    file.beginGroup(QStringLiteral("Global"));
    const QString encoding = file.value(QStringLiteral("encoding")).toString();
    if (!encoding.isEmpty())
        m_player.song().setCurrentCharset(encoding.toStdString());
    const int volume = file.value(QStringLiteral("volume"), 0).toInt();
    if (volume > 0)
        setVolumePercent(volume);
    setPitch(file.value(QStringLiteral("pitch"), 0).toInt());
    const int skew = file.value(QStringLiteral("timeskew"), 0).toInt();
    if (skew > 0)
        setTempoPercent(skew);
    file.endGroup();

    for (int i = 0; i < kMidiChannels; ++i) {
        const QString group = songSettingsGroup(i);
        if (!file.childGroups().contains(group))
            continue;
        file.beginGroup(group);
        const QModelIndex index = m_channelModel.index(i);
        const QString channelName = file.value(QStringLiteral("name")).toString();
        if (!channelName.isEmpty())
            m_channelModel.setData(index, channelName, ChannelModel::NameRole);
        m_channelModel.setData(index, file.value(QStringLiteral("muted"), false).toBool(),
                               ChannelModel::MutedRole);
        m_channelModel.setData(index, file.value(QStringLiteral("solo"), false).toBool(),
                               ChannelModel::SoloRole);
        m_channelModel.setData(index, file.value(QStringLiteral("locked"), false).toBool(),
                               ChannelModel::LockedRole);
        m_channelModel.setData(index, file.value(QStringLiteral("patch"), 0).toInt(),
                               ChannelModel::PatchRole);
        const int level = file.value(QStringLiteral("level"), 0).toInt();
        if (level > 0)
            m_channelModel.setData(index, level, ChannelModel::VolumeRole);
        file.endGroup();
    }
    applyMix();
    rebuildLyrics();
}

QString PlayerController::fileInformation() const
{
    auto& song = m_player.song();
    if (song.currentFile().empty())
        return i18n("No file is loaded.");

    QStringList lines;
    lines << i18n("File: %1", QString::fromStdString(song.currentFile()));
    if (m_songIsRemote)
        lines << i18n("Source: %1", m_songLocator);
    lines << i18n("Format: %1", QString::fromStdString(song.fileFormat()))
          << i18n("Duration: %1", QString::fromStdString(song.durationString()))
          << i18n("Initial tempo: %1 bpm",
                  QString::number(song.initialTempo() > 0 ? tempoToBpm(song.initialTempo()) : 120.0,
                                  'f', 1))
          << i18n("Tracks: %1", song.getNumTracks())
          << i18n("Division: %1", song.division());

    const QString metadata = QString::fromStdString(song.metadataInfo());
    if (!metadata.isEmpty())
        lines << metadata.split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (const auto& copyright : song.getText(TextType::Copyright))
        lines << i18n("Copyright: %1", QString::fromStdString(copyright));

    return lines.join(QLatin1Char('\n'));
}

void PlayerController::persistWindowState(int width, int height)
{
    if (width > 0)
        st().windowWidth = width;
    if (height > 0)
        st().windowHeight = height;
    st().repeatMode = m_repeat;
    st().save();
}

} // namespace dmidi
