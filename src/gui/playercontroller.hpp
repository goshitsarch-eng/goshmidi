/*
    Gosh MIDI Player — Qt6/Kirigami

    Everything the QML layer drives: transport, playlist, lyrics, channels and
    MIDI output selection.
*/

#pragma once

#include "../app/playlist.hpp"
#include "../midi/output.hpp"
#include "../midi/player.hpp"

#include "channelmodel.hpp"
#include "playlistmodel.hpp"
#include "remotefileresolver.hpp"

#include <KAboutData>

#include <QColor>
#include <QObject>
#include <QQmlEngine>
#include <QString>
#include <QStringList>
#include <QTimer>
#include <QUrl>

#include <vector>

namespace dmidi {

class PlayerController : public QObject
{
    Q_OBJECT
    QML_NAMED_ELEMENT(Player)
    QML_SINGLETON

    Q_PROPERTY(dmidi::PlaylistModel* playlist READ playlistModel CONSTANT)
    Q_PROPERTY(dmidi::ChannelModel* channels READ channelModel CONSTANT)

    // Transport
    Q_PROPERTY(bool playing READ isPlaying NOTIFY playbackStateChanged)
    Q_PROPERTY(bool paused READ isPaused NOTIFY playbackStateChanged)
    Q_PROPERTY(bool hasSong READ hasSong NOTIFY songChanged)
    Q_PROPERTY(QString statusText READ statusText NOTIFY statusChanged)
    Q_PROPERTY(QString songTitle READ songTitle NOTIFY songChanged)
    Q_PROPERTY(QString songLocation READ songLocation NOTIFY songChanged)
    Q_PROPERTY(bool songIsRemote READ songIsRemote NOTIFY songChanged)
    Q_PROPERTY(QString timeText READ timeText NOTIFY positionChanged)
    Q_PROPERTY(QString durationText READ durationText NOTIFY songChanged)
    Q_PROPERTY(qreal position READ position WRITE setPosition NOTIFY positionChanged)
    Q_PROPERTY(QString barBeatText READ barBeatText NOTIFY beatChanged)
    Q_PROPERTY(int currentBeat READ currentBeat NOTIFY beatChanged)
    Q_PROPERTY(int beatsPerBar READ beatsPerBar NOTIFY beatChanged)
    Q_PROPERTY(int lastBar READ lastBar NOTIFY songChanged)

    // Adjustments
    Q_PROPERTY(int tempoPercent READ tempoPercent WRITE setTempoPercent NOTIFY tempoChanged)
    Q_PROPERTY(QString bpmText READ bpmText NOTIFY tempoChanged)
    Q_PROPERTY(int volumePercent READ volumePercent WRITE setVolumePercent NOTIFY volumeChanged)
    Q_PROPERTY(int pitch READ pitch WRITE setPitch NOTIFY pitchChanged)
    Q_PROPERTY(bool loopEnabled READ loopEnabled WRITE setLoopEnabled NOTIFY loopChanged)
    Q_PROPERTY(int loopStart READ loopStart NOTIFY loopChanged)
    Q_PROPERTY(int loopEnd READ loopEnd NOTIFY loopChanged)
    Q_PROPERTY(int repeatMode READ repeatMode WRITE setRepeatMode NOTIFY repeatModeChanged)

    // Lyrics
    Q_PROPERTY(QString lyricsText READ lyricsText NOTIFY lyricsChanged)
    Q_PROPERTY(QString plainLyrics READ plainLyrics NOTIFY lyricsChanged)
    Q_PROPERTY(QStringList lyricTracks READ lyricTracks NOTIFY lyricsChanged)
    Q_PROPERTY(QStringList lyricTypes READ lyricTypes CONSTANT)
    Q_PROPERTY(QStringList encodings READ encodings NOTIFY lyricsChanged)
    Q_PROPERTY(int lyricTrackIndex READ lyricTrackIndex WRITE setLyricTrackIndex NOTIFY lyricsChanged)
    Q_PROPERTY(int lyricTypeIndex READ lyricTypeIndex WRITE setLyricTypeIndex NOTIFY lyricsChanged)
    Q_PROPERTY(int encodingIndex READ encodingIndex WRITE setEncodingIndex NOTIFY lyricsChanged)

    // Piano
    Q_PROPERTY(int lowestNote READ lowestNote NOTIFY songChanged)
    Q_PROPERTY(int highestNote READ highestNote NOTIFY songChanged)

    // MIDI output
    Q_PROPERTY(QStringList backends READ backends CONSTANT)
    Q_PROPERTY(QString currentBackend READ currentBackend NOTIFY outputChanged)
    Q_PROPERTY(QString currentPort READ currentPort NOTIFY outputChanged)
    Q_PROPERTY(QStringList portLabels READ portLabels NOTIFY portsChanged)
    Q_PROPERTY(QString outputSummary READ outputSummary NOTIFY outputChanged)
    Q_PROPERTY(QStringList patchNames READ patchNames NOTIFY patchNamesChanged)

    // Shown by Kirigami.AboutPage
    Q_PROPERTY(KAboutData aboutData READ aboutData CONSTANT)

    // Remote fetches
    Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
    Q_PROPERTY(QString busyText READ busyText NOTIFY busyChanged)
    Q_PROPERTY(int busyProgress READ busyProgress NOTIFY busyChanged)

public:
    ~PlayerController() override;

    // The controller is a singleton shared by main() and QML. The constructor
    // stays private so Qt cannot default-construct a second one for the engine
    // — it checks default-constructibility before looking for create().
    static PlayerController* instance();
    static PlayerController* create(QQmlEngine*, QJSEngine*);

    PlaylistModel* playlistModel() { return &m_playlistModel; }
    ChannelModel* channelModel() { return &m_channelModel; }

    bool isPlaying() const;
    bool isPaused() const;
    bool hasSong() const;
    QString statusText() const { return m_status; }
    QString songTitle() const { return m_songTitle; }
    QString songLocation() const { return m_songLocation; }
    bool songIsRemote() const { return m_songIsRemote; }
    QString timeText() const { return m_timeText; }
    QString durationText() const;
    qreal position() const { return m_position; }
    QString barBeatText() const { return m_barBeat; }
    int currentBeat() const { return m_beat; }
    int beatsPerBar() const { return m_beatsPerBar; }
    int lastBar() const;

    int tempoPercent() const { return m_tempoPercent; }
    QString bpmText() const { return m_bpmText; }
    int volumePercent() const { return m_volumePercent; }
    int pitch() const { return m_pitch; }
    bool loopEnabled() const;
    int loopStart() const;
    int loopEnd() const;
    int repeatMode() const { return m_repeat; }

    QString lyricsText() const { return m_lyricsRich; }
    QString plainLyrics() const { return m_lyricsPlain; }
    QStringList lyricTracks() const { return m_lyricTracks; }
    QStringList lyricTypes() const;
    QStringList encodings() const { return m_encodings; }
    int lyricTrackIndex() const { return m_lyricTrackIndex; }
    int lyricTypeIndex() const { return m_lyricTypeIndex; }
    int encodingIndex() const { return m_encodingIndex; }

    int lowestNote() const;
    int highestNote() const;

    QStringList backends() const;
    QString currentBackend() const;
    QString currentPort() const;
    QStringList portLabels() const { return m_portLabels; }
    QString outputSummary() const { return m_outputSummary; }
    QStringList patchNames() const { return m_patchNames; }

    KAboutData aboutData() const { return KAboutData::applicationData(); }

    bool busy() const { return m_busy; }
    QString busyText() const { return m_busyText; }
    int busyProgress() const { return m_busyProgress; }

    void setPosition(qreal fraction);
    void setTempoPercent(int percent);
    void setVolumePercent(int percent);
    void setPitch(int semitones);
    void setLoopEnabled(bool enabled);
    void setRepeatMode(int mode);
    void setLyricTrackIndex(int index);
    void setLyricTypeIndex(int index);
    void setEncodingIndex(int index);

    // Entry points used by main() for command-line files and D-Bus activation.
    void openLocators(const QStringList& locators, bool replacePlaylist);
    void connectOutput(const QString& backend, const QString& portId);

public Q_SLOTS:
    void play();
    void pause();
    void stop();
    void next();
    void previous();
    void forwardOneBar();
    void rewindOneBar();
    void jumpToBar(int bar);
    void setLoopRange(int fromBar, int toBar);

    void openUrls(const QList<QUrl>& urls, bool replacePlaylist);
    void addUrls(const QList<QUrl>& urls);
    void playIndex(int index);
    void removeAt(int index);
    void moveUp(int index);
    void moveDown(int index);
    void shufflePlaylist();
    void clearPlaylist();
    void loadPlaylist(const QUrl& url);
    void savePlaylist(const QUrl& url);
    void cancelPendingLoad();

    void saveSongSettings();
    void loadSongSettings();
    QString fileInformation() const;

    void refreshPorts(const QString& backend, bool advanced);
    QString portIdAt(int index) const;
    QVariantMap deviceProfileFor(int portIndex) const;
    void applyMidiSetup(const QString& backend, int portIndex, const QString& soundFont,
                        int instrumentMap, int sysexReset, bool advancedPorts);
    void sendTestNote();
    void refreshPatchNames();

    void playNote(int channel, int note, int velocity);
    void releaseNotes();

    void copyLyricsToClipboard();
    void saveLyrics(const QUrl& url);

    void persistWindowState(int width, int height);

Q_SIGNALS:
    void playbackStateChanged();
    void songChanged();
    void statusChanged();
    void positionChanged();
    void beatChanged();
    void tempoChanged();
    void volumeChanged();
    void pitchChanged();
    void loopChanged();
    void repeatModeChanged();
    void lyricsChanged();
    void lyricsAdvanced(int characterOffset);
    void outputChanged();
    void portsChanged();
    void patchNamesChanged();
    void busyChanged();
    void patchNamesChangedForChannel(int channel, int patch);

    // Piano keyboard feed.
    void noteOn(int channel, int note, int velocity);
    void noteOff(int channel, int note);
    void notesCleared();

    // Transient user feedback, shown as a Kirigami inline message.
    void message(const QString& text);
    void errorMessage(const QString& text);

private:
    explicit PlayerController();

    void wirePlayer();
    void loadCurrent(bool autoPlay);
    void loadResolvedFile(const QString& locator, const QString& localPath, bool autoPlay);
    void refreshSongInfo();
    void rebuildLyrics();
    int lyricCursorAt(int64_t ticks) const;
    void applyMix();
    void setStatus(const QString& text);
    void setBusy(bool busy, const QString& text = {}, int progress = -1);
    void restoreOutput();
    void loadInitialPlaylist();
    void handleFinished();
    QString songSettingsName() const;

    SequencePlayer m_player;
    MidiOutputManager m_outputs;
    Playlist m_playlist;
    PlaylistModel m_playlistModel{&m_playlist};
    ChannelModel m_channelModel;
    RemoteFileResolver m_resolver;
    QTimer m_meterTimer;

    QString m_status;
    QString m_songTitle;
    QString m_songLocation;
    QString m_songLocator;
    bool m_songIsRemote{};
    QString m_timeText{QStringLiteral("00:00:00")};
    QString m_barBeat{QStringLiteral("1:1")};
    QString m_bpmText;
    QString m_outputSummary;
    qreal m_position{};
    int m_beat{1};
    int m_beatsPerBar{4};
    int m_tempoPercent{100};
    int m_volumePercent{100};
    int m_pitch{};
    int m_repeat{};
    bool m_seeking{};
    bool m_pendingAutoPlay{};

    QString m_lyricsRich;
    QString m_lyricsPlain;
    QStringList m_lyricTracks;
    QStringList m_encodings;
    int m_lyricTrackIndex{};
    int m_lyricTypeIndex{5};
    int m_encodingIndex{};
    int m_lyricsCursor{};

    // How far into the lyrics each text event carries the karaoke cursor.
    // Keyed by tick so that seeking lands in the right place.
    struct LyricSpan {
        int64_t tick{};
        int end{};
    };
    std::vector<LyricSpan> m_lyricSpans;

    QStringList m_portLabels;
    std::vector<MidiPort> m_ports;
    QStringList m_patchNames;

    bool m_busy{};
    QString m_busyText;
    int m_busyProgress{-1};
};

} // namespace dmidi
