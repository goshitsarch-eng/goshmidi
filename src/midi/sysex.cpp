/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#include "sysex.hpp"

#include <filesystem>
#include <fstream>

namespace dmidi {
namespace {

const uint8_t kGmReset[] = {0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7};
const uint8_t kGsReset[] = {0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x00, 0x41, 0xF7};
const uint8_t kXgReset[] = {0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7};
// Roland MT-32 / CM-32L: DT1 all-parameters reset (addr 7F 00 00, data 01)
const uint8_t kMt32Reset[] = {0xF0, 0x41, 0x10, 0x16, 0x12, 0x7F, 0x00, 0x00, 0x01, 0x00, 0xF7};

std::vector<uint8_t> fromArray(const uint8_t* p, size_t n)
{
    return {p, p + n};
}

bool existsFile(const std::filesystem::path& p)
{
    std::error_code ec;
    return std::filesystem::is_regular_file(p, ec);
}

} // namespace

const char* sysexResetName(int kind)
{
    switch (kind) {
    case kSysexResetGm:
        return "GM Reset";
    case kSysexResetGs:
        return "GS Reset (SC-55)";
    case kSysexResetXg:
        return "XG Reset";
    case kSysexResetMt32:
        return "MT-32 Reset";
    default:
        return "None";
    }
}

std::vector<uint8_t> sysexResetMessage(int kind)
{
    switch (kind) {
    case kSysexResetGm:
        return fromArray(kGmReset, sizeof(kGmReset));
    case kSysexResetGs:
        return fromArray(kGsReset, sizeof(kGsReset));
    case kSysexResetXg:
        return fromArray(kXgReset, sizeof(kXgReset));
    case kSysexResetMt32:
        return fromArray(kMt32Reset, sizeof(kMt32Reset));
    default:
        return {};
    }
}

int sysexResetSettleMs(int kind)
{
    switch (kind) {
    case kSysexResetMt32:
        return 80;
    case kSysexResetGs:
    case kSysexResetXg:
        return 50;
    case kSysexResetGm:
        return 40;
    default:
        return 0;
    }
}

int sysexPacingMs(size_t messageBytes)
{
    if (messageBytes > 256)
        return 40;
    if (messageBytes > 32)
        return 15;
    return 0;
}

std::vector<std::vector<uint8_t>> parseSysexMessages(const std::vector<uint8_t>& bytes)
{
    std::vector<std::vector<uint8_t>> out;
    std::vector<uint8_t> cur;
    bool inMsg = false;
    for (uint8_t b : bytes) {
        if (b == 0xF0) {
            cur.clear();
            cur.push_back(b);
            inMsg = true;
            continue;
        }
        if (!inMsg)
            continue;
        cur.push_back(b);
        if (b == 0xF7) {
            if (cur.size() >= 2)
                out.push_back(cur);
            cur.clear();
            inMsg = false;
        }
    }
    return out;
}

std::vector<std::vector<uint8_t>> loadSysexFile(const std::string& path)
{
    std::ifstream in(path, std::ios::binary);
    if (!in)
        return {};
    in.seekg(0, std::ios::end);
    auto sz = in.tellg();
    if (sz <= 0)
        return {};
    in.seekg(0);
    std::vector<uint8_t> buf(static_cast<size_t>(sz));
    in.read(reinterpret_cast<char*>(buf.data()), static_cast<std::streamsize>(buf.size()));
    return parseSysexMessages(buf);
}

std::string companionSyxPath(const std::string& midiPath)
{
    if (midiPath.empty())
        return {};
    namespace fs = std::filesystem;
    fs::path midi(midiPath);
    fs::path dir = midi.parent_path();
    fs::path stem = midi.stem();
    const fs::path candidates[] = {
        dir / (stem.string() + ".syx"),
        dir / (stem.string() + ".SYX"),
        dir / "Folder.syx",
        dir / "folder.syx",
        dir / "FOLDER.SYX",
    };
    for (auto& p : candidates) {
        if (existsFile(p))
            return p.string();
    }
    return {};
}

} // namespace dmidi
