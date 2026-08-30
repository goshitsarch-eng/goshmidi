/*
    Gosh MIDI Player — Qt6/Kirigami
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#include "events.hpp"

#include <algorithm>
#include <cctype>
#include <filesystem>

namespace dmidi {
namespace {

std::string lowered(std::string s)
{
    std::transform(s.begin(), s.end(), s.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return s;
}

// Path part of a locator: everything before a URL query or fragment.
std::string pathPart(const std::string& locator)
{
    const auto cut = locator.find_first_of("?#");
    return cut == std::string::npos ? locator : locator.substr(0, cut);
}

} // namespace

bool isRemoteLocator(const std::string& locator)
{
    // "scheme://" with a scheme that is neither empty nor "file". Windows drive
    // letters ("C:\...") are not schemes, hence the two-character minimum.
    const auto sep = locator.find("://");
    if (sep == std::string::npos || sep < 2)
        return false;
    for (size_t i = 0; i < sep; ++i) {
        const unsigned char c = static_cast<unsigned char>(locator[i]);
        if (!std::isalnum(c) && c != '+' && c != '-' && c != '.')
            return false;
    }
    return lowered(locator.substr(0, sep)) != "file";
}

std::string locatorFileName(const std::string& locator)
{
    return std::filesystem::path(pathPart(locator)).filename().string();
}

bool isSupportedMidiFile(const std::string& path)
{
    namespace fs = std::filesystem;
    const std::string ext = lowered(fs::path(pathPart(path)).extension().string());
    return ext == ".mid" || ext == ".midi" || ext == ".kar" || ext == ".rmi" || ext == ".wrk";
}

bool isPlaylistFile(const std::string& path)
{
    namespace fs = std::filesystem;
    return lowered(fs::path(pathPart(path)).extension().string()) == ".lst";
}

} // namespace dmidi
