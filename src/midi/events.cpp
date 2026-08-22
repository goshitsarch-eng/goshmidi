/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#include "events.hpp"

#include <algorithm>
#include <cctype>
#include <filesystem>

namespace dmidi {

bool isSupportedMidiFile(const std::string& path)
{
    namespace fs = std::filesystem;
    std::string ext = fs::path(path).extension().string();
    std::transform(ext.begin(), ext.end(), ext.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return ext == ".mid" || ext == ".midi" || ext == ".kar" || ext == ".rmi" || ext == ".wrk";
}

} // namespace dmidi
