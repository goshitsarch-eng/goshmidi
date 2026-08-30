/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#pragma once

#include <string>
#include <vector>

namespace dmidi {

struct AppSettings {
    std::string lastDirectory;
    std::string lastOutputBackend{"FluidSynth"};
    std::string lastOutputConnection{"fluidsynth"};
    std::string lastPlayList;
    std::string soundFont;
    std::string language;
    int drumsChannel{10}; // 1-based like original
    int soloVolumeReduction{50};
    bool autoPlay{true};
    bool autoAdvance{true};
    bool autoSongSettings{false};
    bool advancedPorts{false};
    int sysexReset{1}; // 0 none, 1 GM, 2 GS, 3 XG, 4 MT-32
    int instrumentMap{0}; // 0 GM, 1 GS/SC-55, 2 MT-32
    int highlightPalette{2}; // channels
    bool velocityColor{true};
    bool octaveSubscript{false};
    int namesVisibility{1}; // 0 never, 1 minimal, 2 when active, 3 always
    std::string lyricsFont{"Sans 16"};
    std::string notesFont{"Sans 9"};
    std::string futureColor{"#888888"};
    std::string pastColor{"#1c71d8"};
    std::string singleColor{"#f6d32d"};
    std::string highlightColor{"#e01b24"};
    int textAlignment{0}; // 0 left, 1 center, 2 right
    std::vector<std::string> recentFiles;
    int windowWidth{1100};
    int windowHeight{720};
    bool playlistVisible{true};
    int repeatMode{0}; // 0 none, 1 song, 2 playlist

    static AppSettings& instance();
    void load();
    void save() const;
    void resetDefaults();
    void addRecent(const std::string& path);
    static std::string configDir();
    static std::string dataDir();
    static std::string songSettingsPath(const std::string& songName);
    static void setPortable(const std::string& file);
};

} // namespace dmidi
