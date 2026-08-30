/*
    Gosh MIDI Player — Qt6/Kirigami

    QML-facing view of AppSettings. Every property notifies, so Kirigami form
    controls bind straight to it and the rest of the app follows along.
*/

#pragma once

#include <QColor>
#include <QObject>
#include <QQmlEngine>
#include <QString>

namespace dmidi {

class AppConfig : public QObject
{
    Q_OBJECT
    QML_NAMED_ELEMENT(Config)
    QML_SINGLETON

    Q_PROPERTY(int drumsChannel READ drumsChannel WRITE setDrumsChannel NOTIFY changed)
    Q_PROPERTY(int soloVolumeReduction READ soloVolumeReduction WRITE setSoloVolumeReduction NOTIFY changed)
    Q_PROPERTY(bool autoPlay READ autoPlay WRITE setAutoPlay NOTIFY changed)
    Q_PROPERTY(bool autoAdvance READ autoAdvance WRITE setAutoAdvance NOTIFY changed)
    Q_PROPERTY(bool autoSongSettings READ autoSongSettings WRITE setAutoSongSettings NOTIFY changed)
    Q_PROPERTY(bool advancedPorts READ advancedPorts WRITE setAdvancedPorts NOTIFY changed)
    Q_PROPERTY(int sysexReset READ sysexReset WRITE setSysexReset NOTIFY changed)
    Q_PROPERTY(int instrumentMap READ instrumentMap WRITE setInstrumentMap NOTIFY changed)
    Q_PROPERTY(QString soundFont READ soundFont WRITE setSoundFont NOTIFY changed)

    Q_PROPERTY(QColor futureColor READ futureColor WRITE setFutureColor NOTIFY changed)
    Q_PROPERTY(QColor pastColor READ pastColor WRITE setPastColor NOTIFY changed)
    Q_PROPERTY(QColor highlightColor READ highlightColor WRITE setHighlightColor NOTIFY changed)
    Q_PROPERTY(int lyricsFontSize READ lyricsFontSize WRITE setLyricsFontSize NOTIFY changed)
    Q_PROPERTY(int textAlignment READ textAlignment WRITE setTextAlignment NOTIFY changed)

    Q_PROPERTY(int highlightPalette READ highlightPalette WRITE setHighlightPalette NOTIFY changed)
    Q_PROPERTY(QColor singleColor READ singleColor WRITE setSingleColor NOTIFY changed)
    Q_PROPERTY(bool velocityColor READ velocityColor WRITE setVelocityColor NOTIFY changed)
    Q_PROPERTY(int namesVisibility READ namesVisibility WRITE setNamesVisibility NOTIFY changed)
    Q_PROPERTY(bool octaveSubscript READ octaveSubscript WRITE setOctaveSubscript NOTIFY changed)

    Q_PROPERTY(int windowWidth READ windowWidth WRITE setWindowWidth NOTIFY changed)
    Q_PROPERTY(int windowHeight READ windowHeight WRITE setWindowHeight NOTIFY changed)
    Q_PROPERTY(bool playlistVisible READ playlistVisible WRITE setPlaylistVisible NOTIFY changed)
    Q_PROPERTY(int repeatMode READ repeatMode WRITE setRepeatMode NOTIFY changed)

public:
    // Private constructor: see PlayerController for why a QML singleton must
    // not be default-constructible.
    static AppConfig* instance();
    static AppConfig* create(QQmlEngine*, QJSEngine*);

    int drumsChannel() const;
    int soloVolumeReduction() const;
    bool autoPlay() const;
    bool autoAdvance() const;
    bool autoSongSettings() const;
    bool advancedPorts() const;
    int sysexReset() const;
    int instrumentMap() const;
    QString soundFont() const;
    QColor futureColor() const;
    QColor pastColor() const;
    QColor highlightColor() const;
    int lyricsFontSize() const;
    int textAlignment() const;
    int highlightPalette() const;
    QColor singleColor() const;
    bool velocityColor() const;
    int namesVisibility() const;
    bool octaveSubscript() const;
    int windowWidth() const;
    int windowHeight() const;
    bool playlistVisible() const;
    int repeatMode() const;

    void setDrumsChannel(int value);
    void setSoloVolumeReduction(int value);
    void setAutoPlay(bool value);
    void setAutoAdvance(bool value);
    void setAutoSongSettings(bool value);
    void setAdvancedPorts(bool value);
    void setSysexReset(int value);
    void setInstrumentMap(int value);
    void setSoundFont(const QString& value);
    void setFutureColor(const QColor& value);
    void setPastColor(const QColor& value);
    void setHighlightColor(const QColor& value);
    void setLyricsFontSize(int value);
    void setTextAlignment(int value);
    void setHighlightPalette(int value);
    void setSingleColor(const QColor& value);
    void setVelocityColor(bool value);
    void setNamesVisibility(int value);
    void setOctaveSubscript(bool value);
    void setWindowWidth(int value);
    void setWindowHeight(int value);
    void setPlaylistVisible(bool value);
    void setRepeatMode(int value);

    Q_INVOKABLE void save();
    Q_INVOKABLE void restoreDefaults();

    // For code that writes AppSettings directly and needs QML to catch up.
    void notifyChanged();

Q_SIGNALS:
    // One signal for the lot: the settings pages rebind wholesale and the
    // players of individual values are cheap to re-read.
    void changed();

private:
    AppConfig();

    void touch();
};

} // namespace dmidi
