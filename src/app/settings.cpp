/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#include "settings.hpp"

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <glib.h>
#include <sstream>

namespace dmidi {
namespace {
std::string g_portableFile;
bool g_portable{};

std::string defaultConfigPath()
{
    if (g_portable) {
        if (!g_portableFile.empty())
            return g_portableFile;
        return std::filesystem::current_path() / "dmidiplayer.conf";
    }
    auto* dir = g_get_user_config_dir();
    std::filesystem::path p = std::filesystem::path(dir) / "dmidiplayer";
    std::error_code ec;
    std::filesystem::create_directories(p, ec);
    return (p / "dmidiplayer.conf").string();
}

std::string colorOr(const char* v, const char* fallback)
{
    return (v && *v) ? v : fallback;
}
} // namespace

AppSettings& AppSettings::instance()
{
    static AppSettings s;
    return s;
}

void AppSettings::setPortable(const std::string& file)
{
    g_portable = true;
    g_portableFile = file;
}

std::string AppSettings::configDir()
{
    if (g_portable)
        return std::filesystem::path(defaultConfigPath()).parent_path().string();
    return (std::filesystem::path(g_get_user_config_dir()) / "dmidiplayer").string();
}

std::string AppSettings::dataDir()
{
    auto p = std::filesystem::path(g_get_home_dir()) / ".dmidiplayer";
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
    GKeyFile* kf = g_key_file_new();
    GError* err = nullptr;
    if (!g_key_file_load_from_file(kf, defaultConfigPath().c_str(), G_KEY_FILE_NONE, &err)) {
        if (err)
            g_error_free(err);
        g_key_file_free(kf);
        return;
    }
    auto str = [&](const char* k, const std::string& def) {
        GError* e = nullptr;
        char* v = g_key_file_get_string(kf, "General", k, &e);
        if (e) {
            g_error_free(e);
            return def;
        }
        std::string s = v ? v : def;
        g_free(v);
        return s;
    };
    auto num = [&](const char* k, int def) {
        GError* e = nullptr;
        int v = g_key_file_get_integer(kf, "General", k, &e);
        if (e) {
            g_error_free(e);
            return def;
        }
        return v;
    };
    auto flag = [&](const char* k, bool def) {
        GError* e = nullptr;
        gboolean v = g_key_file_get_boolean(kf, "General", k, &e);
        if (e) {
            g_error_free(e);
            return def;
        }
        return bool(v);
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
    highlightPalette = num("highlightPalette", highlightPalette);
    velocityColor = flag("velocityColor", velocityColor);
    octaveSubscript = flag("octaveSubscript", octaveSubscript);
    namesVisibility = num("namesVisibility", namesVisibility);
    lyricsFont = str("lyricsFont", lyricsFont);
    notesFont = str("notesFont", notesFont);
    futureColor = colorOr(str("futureColor", futureColor).c_str(), "#888888");
    pastColor = str("pastColor", pastColor);
    singleColor = str("singleColor", singleColor);
    highlightColor = str("highlightColor", highlightColor);
    textAlignment = num("textAlignment", textAlignment);
    windowWidth = num("windowWidth", windowWidth);
    windowHeight = num("windowHeight", windowHeight);
    playlistVisible = flag("playlistVisible", playlistVisible);
    repeatMode = num("repeatMode", repeatMode);
    gsize n = 0;
    char** rec = g_key_file_get_string_list(kf, "General", "recentFiles", &n, nullptr);
    recentFiles.clear();
    if (rec) {
        for (gsize i = 0; i < n; ++i)
            recentFiles.emplace_back(rec[i]);
        g_strfreev(rec);
    }
    g_key_file_free(kf);
}

void AppSettings::save() const
{
    GKeyFile* kf = g_key_file_new();
    g_key_file_set_string(kf, "General", "lastDirectory", lastDirectory.c_str());
    g_key_file_set_string(kf, "General", "lastOutputBackend", lastOutputBackend.c_str());
    g_key_file_set_string(kf, "General", "lastOutputConnection", lastOutputConnection.c_str());
    g_key_file_set_string(kf, "General", "lastPlayList", lastPlayList.c_str());
    g_key_file_set_string(kf, "General", "soundFont", soundFont.c_str());
    g_key_file_set_string(kf, "General", "language", language.c_str());
    g_key_file_set_integer(kf, "General", "drumsChannel", drumsChannel);
    g_key_file_set_integer(kf, "General", "soloVolumeReduction", soloVolumeReduction);
    g_key_file_set_boolean(kf, "General", "autoPlay", autoPlay);
    g_key_file_set_boolean(kf, "General", "autoAdvance", autoAdvance);
    g_key_file_set_boolean(kf, "General", "autoSongSettings", autoSongSettings);
    g_key_file_set_boolean(kf, "General", "advancedPorts", advancedPorts);
    g_key_file_set_integer(kf, "General", "sysexReset", sysexReset);
    g_key_file_set_integer(kf, "General", "highlightPalette", highlightPalette);
    g_key_file_set_boolean(kf, "General", "velocityColor", velocityColor);
    g_key_file_set_boolean(kf, "General", "octaveSubscript", octaveSubscript);
    g_key_file_set_integer(kf, "General", "namesVisibility", namesVisibility);
    g_key_file_set_string(kf, "General", "lyricsFont", lyricsFont.c_str());
    g_key_file_set_string(kf, "General", "notesFont", notesFont.c_str());
    g_key_file_set_string(kf, "General", "futureColor", futureColor.c_str());
    g_key_file_set_string(kf, "General", "pastColor", pastColor.c_str());
    g_key_file_set_string(kf, "General", "singleColor", singleColor.c_str());
    g_key_file_set_string(kf, "General", "highlightColor", highlightColor.c_str());
    g_key_file_set_integer(kf, "General", "textAlignment", textAlignment);
    g_key_file_set_integer(kf, "General", "windowWidth", windowWidth);
    g_key_file_set_integer(kf, "General", "windowHeight", windowHeight);
    g_key_file_set_boolean(kf, "General", "playlistVisible", playlistVisible);
    g_key_file_set_integer(kf, "General", "repeatMode", repeatMode);
    std::vector<char*> rec;
    rec.reserve(recentFiles.size());
    for (auto& s : recentFiles)
        rec.push_back(const_cast<char*>(s.c_str()));
    if (!rec.empty())
        g_key_file_set_string_list(kf, "General", "recentFiles", rec.data(), rec.size());
    gsize len = 0;
    char* data = g_key_file_to_data(kf, &len, nullptr);
    GError* err = nullptr;
    g_file_set_contents(defaultConfigPath().c_str(), data, static_cast<gssize>(len), &err);
    if (err)
        g_error_free(err);
    g_free(data);
    g_key_file_free(kf);
}

void AppSettings::addRecent(const std::string& path)
{
    recentFiles.erase(std::remove(recentFiles.begin(), recentFiles.end(), path), recentFiles.end());
    recentFiles.insert(recentFiles.begin(), path);
    if (recentFiles.size() > 10)
        recentFiles.resize(10);
}

} // namespace dmidi
