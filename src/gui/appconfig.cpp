/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "appconfig.hpp"

#include "../app/settings.hpp"

#include <QQmlEngine>
#include <QRegularExpression>

namespace dmidi {
namespace {

AppSettings& st()
{
    return AppSettings::instance();
}

QColor toColor(const std::string& text, const QColor& fallback)
{
    const QColor c(QString::fromStdString(text));
    return c.isValid() ? c : fallback;
}

// Settings keep the Pango-style "Family Size" string the earlier releases
// wrote; QML only needs the point size out of it.
int fontSizeOf(const std::string& description, int fallback)
{
    static const QRegularExpression trailingSize(QStringLiteral("(\\d+)\\s*$"));
    const auto match = trailingSize.match(QString::fromStdString(description));
    if (!match.hasMatch())
        return fallback;
    bool ok = false;
    const int size = match.captured(1).toInt(&ok);
    return ok && size > 0 ? size : fallback;
}

std::string withFontSize(const std::string& description, int size)
{
    static const QRegularExpression trailingSize(QStringLiteral("\\s*\\d+\\s*$"));
    QString family = QString::fromStdString(description).remove(trailingSize).trimmed();
    if (family.isEmpty())
        family = QStringLiteral("Sans");
    return QStringLiteral("%1 %2").arg(family).arg(size).toStdString();
}

} // namespace

AppConfig::AppConfig() = default;

AppConfig* AppConfig::instance()
{
    static AppConfig config;
    return &config;
}

AppConfig* AppConfig::create(QQmlEngine*, QJSEngine*)
{
    auto* config = instance();
    QQmlEngine::setObjectOwnership(config, QQmlEngine::CppOwnership);
    return config;
}

void AppConfig::touch()
{
    Q_EMIT changed();
}

void AppConfig::notifyChanged()
{
    touch();
}

int AppConfig::drumsChannel() const { return st().drumsChannel; }
int AppConfig::soloVolumeReduction() const { return st().soloVolumeReduction; }
bool AppConfig::autoPlay() const { return st().autoPlay; }
bool AppConfig::autoAdvance() const { return st().autoAdvance; }
bool AppConfig::autoSongSettings() const { return st().autoSongSettings; }
bool AppConfig::advancedPorts() const { return st().advancedPorts; }
int AppConfig::sysexReset() const { return st().sysexReset; }
int AppConfig::instrumentMap() const { return st().instrumentMap; }
QString AppConfig::soundFont() const { return QString::fromStdString(st().soundFont); }
QColor AppConfig::futureColor() const { return toColor(st().futureColor, QColor(0x88, 0x88, 0x88)); }
QColor AppConfig::pastColor() const { return toColor(st().pastColor, QColor(0x1c, 0x71, 0xd8)); }
QColor AppConfig::highlightColor() const { return toColor(st().highlightColor, QColor(0xe0, 0x1b, 0x24)); }
int AppConfig::lyricsFontSize() const { return fontSizeOf(st().lyricsFont, 16); }
int AppConfig::textAlignment() const { return st().textAlignment; }
int AppConfig::highlightPalette() const { return st().highlightPalette; }
QColor AppConfig::singleColor() const { return toColor(st().singleColor, QColor(0xf6, 0xd3, 0x2d)); }
bool AppConfig::velocityColor() const { return st().velocityColor; }
int AppConfig::namesVisibility() const { return st().namesVisibility; }
bool AppConfig::octaveSubscript() const { return st().octaveSubscript; }
int AppConfig::windowWidth() const { return st().windowWidth; }
int AppConfig::windowHeight() const { return st().windowHeight; }
bool AppConfig::playlistVisible() const { return st().playlistVisible; }
int AppConfig::repeatMode() const { return st().repeatMode; }

#define DMIDI_ASSIGN(field, value)                                                                 \
    do {                                                                                           \
        const auto next = (value);                                                                 \
        if (st().field == next)                                                                    \
            return;                                                                                \
        st().field = next;                                                                         \
        touch();                                                                                   \
    } while (0)

void AppConfig::setDrumsChannel(int value) { DMIDI_ASSIGN(drumsChannel, qBound(1, value, 16)); }
void AppConfig::setSoloVolumeReduction(int value) { DMIDI_ASSIGN(soloVolumeReduction, qBound(0, value, 100)); }
void AppConfig::setAutoPlay(bool value) { DMIDI_ASSIGN(autoPlay, value); }
void AppConfig::setAutoAdvance(bool value) { DMIDI_ASSIGN(autoAdvance, value); }
void AppConfig::setAutoSongSettings(bool value) { DMIDI_ASSIGN(autoSongSettings, value); }
void AppConfig::setAdvancedPorts(bool value) { DMIDI_ASSIGN(advancedPorts, value); }
void AppConfig::setSysexReset(int value) { DMIDI_ASSIGN(sysexReset, qBound(0, value, 4)); }
void AppConfig::setInstrumentMap(int value) { DMIDI_ASSIGN(instrumentMap, qBound(0, value, 2)); }
void AppConfig::setTextAlignment(int value) { DMIDI_ASSIGN(textAlignment, qBound(0, value, 2)); }
void AppConfig::setHighlightPalette(int value) { DMIDI_ASSIGN(highlightPalette, qBound(0, value, 3)); }
void AppConfig::setVelocityColor(bool value) { DMIDI_ASSIGN(velocityColor, value); }
void AppConfig::setNamesVisibility(int value) { DMIDI_ASSIGN(namesVisibility, qBound(0, value, 3)); }
void AppConfig::setOctaveSubscript(bool value) { DMIDI_ASSIGN(octaveSubscript, value); }
void AppConfig::setWindowWidth(int value) { DMIDI_ASSIGN(windowWidth, value); }
void AppConfig::setWindowHeight(int value) { DMIDI_ASSIGN(windowHeight, value); }
void AppConfig::setPlaylistVisible(bool value) { DMIDI_ASSIGN(playlistVisible, value); }
void AppConfig::setRepeatMode(int value) { DMIDI_ASSIGN(repeatMode, qBound(0, value, 2)); }

#undef DMIDI_ASSIGN

void AppConfig::setSoundFont(const QString& value)
{
    const std::string next = value.toStdString();
    if (st().soundFont == next)
        return;
    st().soundFont = next;
    touch();
}

void AppConfig::setFutureColor(const QColor& value)
{
    if (!value.isValid() || futureColor() == value)
        return;
    st().futureColor = value.name().toStdString();
    touch();
}

void AppConfig::setPastColor(const QColor& value)
{
    if (!value.isValid() || pastColor() == value)
        return;
    st().pastColor = value.name().toStdString();
    touch();
}

void AppConfig::setHighlightColor(const QColor& value)
{
    if (!value.isValid() || highlightColor() == value)
        return;
    st().highlightColor = value.name().toStdString();
    touch();
}

void AppConfig::setSingleColor(const QColor& value)
{
    if (!value.isValid() || singleColor() == value)
        return;
    st().singleColor = value.name().toStdString();
    touch();
}

void AppConfig::setLyricsFontSize(int value)
{
    const int size = qBound(6, value, 96);
    if (lyricsFontSize() == size)
        return;
    st().lyricsFont = withFontSize(st().lyricsFont, size);
    touch();
}

void AppConfig::save()
{
    st().save();
}

void AppConfig::restoreDefaults()
{
    const auto width = st().windowWidth;
    const auto height = st().windowHeight;
    const auto recent = st().recentFiles;
    st().resetDefaults();
    st().windowWidth = width;
    st().windowHeight = height;
    st().recentFiles = recent;
    st().save();
    touch();
}

} // namespace dmidi
