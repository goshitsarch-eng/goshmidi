/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace dmidi {

// 0 none, 1 GM, 2 GS (SC-55), 3 XG, 4 MT-32
constexpr int kSysexResetNone = 0;
constexpr int kSysexResetGm = 1;
constexpr int kSysexResetGs = 2;
constexpr int kSysexResetXg = 3;
constexpr int kSysexResetMt32 = 4;

const char* sysexResetName(int kind);
std::vector<uint8_t> sysexResetMessage(int kind);
int sysexResetSettleMs(int kind);
int sysexPacingMs(size_t messageBytes);

// Split a .syx blob (or concatenated MIDI SysEx) into F0 ... F7 messages.
std::vector<std::vector<uint8_t>> parseSysexMessages(const std::vector<uint8_t>& bytes);
std::vector<std::vector<uint8_t>> loadSysexFile(const std::string& path);

// Same-basename .syx next to a MIDI file, then Folder.syx in that directory.
std::string companionSyxPath(const std::string& midiPath);

} // namespace dmidi
