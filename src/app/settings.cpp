/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "settings.hpp"

#include <QDir>
#include <QSettings>
#include <QStandardPaths>
#include <QString>
#include <QStringList>

#include <algorithm>
#include <filesystem>

namespace dmidi {
namespace {

std::string s_portableFile;
bool s_portable{};

// Top-level QSettings keys land in the INI file's [General] section, which is
// exactly where the previous key-file based configuration wrote them, so an
// existing ~/.config/dmidiplayer/dmidiplayer.conf keeps working untouched.
std::string defaultConfigPath()
{
    if (s_portable) {
        if (!s_portableFile.empty())
            return s_portableFile;
        return (std::filesystem::current_path() / "dmidiplayer.conf").string();
    }
    const QString base = QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation);
    QDir().mkpath(base + QStringLiteral("/dmidiplayer"));
    return (base + QStringLiteral("/dmidiplayer/dmidiplayer.conf")).toStdString();
}

QSettings openSettings()
{
    return QSettings(QString::fromStdString(defaultConfigPath()), QSettings::IniFormat);
}

} // namespace

AppSettings& AppSettings::instance()
{
    static AppSettings s;
    return s;
}

void AppSettings::setPortable(const std::string& file)
{
    s_portable = true;
    s_portableFile = file;
}

std::string AppSettings::configDir()
{
    return std::filesystem::path(defaultConfigPath()).parent_path().string();
}

std::string AppSettings::dataDir()
{
    auto p = std::filesystem::path(QDir::homePath().toStdString()) / ".dmidiplayer";
    std::error_code ec;
    std::filesystem::create_directories(p, ec);
    return p.string();
}

std::string AppSettings::songSettingsPath(const std::string& songName)
{
    return (std::filesystem::path(dataDir()) / (songName + ".cfg")).string();
}

void AppSettings::resetDefaults()
{
    *this = AppSettings{};
}

void AppSettings::load()
{
    QSettings s = openSettings();

    auto str = [&](const char* key, const std::string& def) {
        const QVariant v = s.value(QString::fromLatin1(key));
        if (!v.isValid())
            return def;
        const QString text = v.toString();
        return text.isEmpty() ? def : text.toStdString();
    };
    auto num = [&](const char* key, int def) {
        bool ok = false;
        const int v = s.value(QString::fromLatin1(key)).toInt(&ok);
        return ok ? v : def;
    };
    auto flag = [&](const char* key, bool def) {
        const QVariant v = s.value(QString::fromLatin1(key));
        return v.isValid() ? v.toBool() : def;
    };

    lastDirectory = str("lastDirectory", lastDirectory);
    lastOutputBackend = str("lastOutputBackend", lastOutputBackend);
    lastOutputConnection = str("lastOutputConnection", lastOutputConnection);
    lastPlayList = str("lastPlayList", lastPlayList);
    soundFont = str("soundFont", soundFont);
    language = str("language", language);
    drumsChannel = num("drumsChannel", drumsChannel);
    soloVolumeReduction = num("soloVolumeReduction", soloVolumeReduction);
    autoPlay = flag("autoPlay", autoPlay);
    autoAdvance = flag("autoAdvance", autoAdvance);
    autoSongSettings = flag("autoSongSettings", autoSongSettings);
    advancedPorts = flag("advancedPorts", advancedPorts);
    sysexReset = num("sysexReset", sysexReset);
    instrumentMap = num("instrumentMap", instrumentMap);
    highlightPalette = num("highlightPalette", highlightPalette);
    velocityColor = flag("velocityColor", velocityColor);
    octaveSubscript = flag("octaveSubscript", octaveSubscript);
    namesVisibility = num("namesVisibility", namesVisibility);
    lyricsFont = str("lyricsFont", lyricsFont);
    notesFont = str("notesFont", notesFont);
    futureColor = str("futureColor", futureColor);
    pastColor = str("pastColor", pastColor);
    singleColor = str("singleColor", singleColor);
    highlightColor = str("highlightColor", highlightColor);
    textAlignment = num("textAlignment", textAlignment);
    windowWidth = num("windowWidth", windowWidth);
    windowHeight = num("windowHeight", windowHeight);
    playlistVisible = flag("playlistVisible", playlistVisible);
    repeatMode = num("repeatMode", repeatMode);

    // Semicolon-separated, matching the list syntax the previous releases wrote.
    recentFiles.clear();
    const QStringList recent =
        s.value(QStringLiteral("recentFiles")).toString().split(QLatin1Char(';'), Qt::SkipEmptyParts);
    for (const QString& r : recent)
        recentFiles.push_back(r.toStdString());
}

void AppSettings::save() const
{
    QSettings s = openSettings();
    auto set = [&](const char* key, const QVariant& value) {
        s.setValue(QString::fromLatin1(key), value);
    };

    set("lastDirectory", QString::fromStdString(lastDirectory));
    set("lastOutputBackend", QString::fromStdString(lastOutputBackend));
    set("lastOutputConnection", QString::fromStdString(lastOutputConnection));
    set("lastPlayList", QString::fromStdString(lastPlayList));
    set("soundFont", QString::fromStdString(soundFont));
    set("language", QString::fromStdString(language));
    set("drumsChannel", drumsChannel);
    set("soloVolumeReduction", soloVolumeReduction);
    set("autoPlay", autoPlay);
    set("autoAdvance", autoAdvance);
    set("autoSongSettings", autoSongSettings);
    set("advancedPorts", advancedPorts);
    set("sysexReset", sysexReset);
    set("instrumentMap", instrumentMap);
    set("highlightPalette", highlightPalette);
    set("velocityColor", velocityColor);
    set("octaveSubscript", octaveSubscript);
    set("namesVisibility", namesVisibility);
    set("lyricsFont", QString::fromStdString(lyricsFont));
    set("notesFont", QString::fromStdString(notesFont));
    set("futureColor", QString::fromStdString(futureColor));
    set("pastColor", QString::fromStdString(pastColor));
    set("singleColor", QString::fromStdString(singleColor));
    set("highlightColor", QString::fromStdString(highlightColor));
    set("textAlignment", textAlignment);
    set("windowWidth", windowWidth);
    set("windowHeight", windowHeight);
    set("playlistVisible", playlistVisible);
    set("repeatMode", repeatMode);

    QStringList recent;
    recent.reserve(static_cast<qsizetype>(recentFiles.size()));
    for (const auto& r : recentFiles)
        recent << QString::fromStdString(r);
    set("recentFiles", recent.join(QLatin1Char(';')));

    s.sync();
}

void AppSettings::addRecent(const std::string& path)
{
    recentFiles.erase(std::remove(recentFiles.begin(), recentFiles.end(), path), recentFiles.end());
    recentFiles.insert(recentFiles.begin(), path);
    if (recentFiles.size() > 10)
        recentFiles.resize(10);
}

} // namespace dmidi
