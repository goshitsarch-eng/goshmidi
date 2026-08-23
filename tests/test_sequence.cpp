/*
    Headless tests for MIDI file loading — feature-parity engine.
*/

#include "midi/sequence.hpp"
#include "midi/sysex.hpp"
#include "app/playlist.hpp"
#include "app/instruments.hpp"

#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

static int g_fail;

#define CHECK(cond, msg)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            std::cerr << "FAIL: " << msg << " (" << __FILE__ << ":" << __LINE__ << ")\n";          \
            ++g_fail;                                                                              \
        }                                                                                          \
    } while (0)

int main(int argc, char** argv)
{
    std::filesystem::path examples = argc > 1 ? argv[1] : "examples";
    if (!std::filesystem::exists(examples / "twinkle.kar")) {
        std::cerr << "examples not found at " << examples << "\n";
        return 2;
    }

    dmidi::Sequence seq;
    CHECK(seq.loadFile((examples / "twinkle.kar").string()), "load twinkle.kar");
    CHECK(!seq.empty(), "twinkle has events");
    CHECK(seq.songLengthTicks() > 0, "twinkle duration ticks");
    CHECK(seq.lastBar() >= 1, "twinkle bars");
    CHECK(!seq.getText(dmidi::TextType::Lyric).empty() || !seq.textEvents().empty(),
          "twinkle lyrics or text");

    dmidi::Sequence mid;
    CHECK(mid.loadFile((examples / "test.mid").string()), "load test.mid");
    CHECK(!mid.empty(), "test.mid events");
    CHECK(mid.division() > 0, "test.mid division");
    bool anyCh = false;
    for (int i = 0; i < 16; ++i)
        anyCh = anyCh || mid.channelUsed(i);
    CHECK(anyCh, "test.mid uses a channel");
    CHECK(mid.firstBeat() != nullptr, "beats generated");
    CHECK(mid.jumpToBar(1) != nullptr, "jump to bar 1");

    dmidi::Sequence kar;
    CHECK(kar.loadFile((examples / "Negra_Sombra.kar").string()), "load Negra_Sombra.kar");
    CHECK(!kar.textEvents().empty(), "karaoke text events");

    dmidi::Playlist pl;
    pl.add((examples / "twinkle.kar").string());
    pl.add((examples / "test.mid").string());
    CHECK(pl.size() == 2, "playlist size");
    CHECK(pl.selectFirst(), "select first");
    CHECK(pl.selectNext(), "select next");
    CHECK(pl.atLast(), "at last");
    CHECK(pl.selectPrev(), "select prev");
    pl.shuffle();
    CHECK(pl.size() == 2, "shuffle keeps size");
    auto tmp = std::filesystem::temp_directory_path() / "dmidi-test.lst";
    CHECK(pl.save(tmp.string()), "save playlist");
    dmidi::Playlist pl2;
    CHECK(pl2.load(tmp.string()), "load playlist");
    CHECK(pl2.size() == 2, "loaded playlist size");

    CHECK(dmidi::isSupportedMidiFile("song.MID"), "case-insensitive mid");
    CHECK(dmidi::isSupportedMidiFile("song.wrk"), "wrk supported");
    CHECK(dmidi::isSupportedMidiFile("song.rmi"), "rmi supported");
    CHECK(!dmidi::isSupportedMidiFile("song.wav"), "wav rejected");

    auto gm = dmidi::sysexResetMessage(dmidi::kSysexResetGm);
    CHECK(gm.size() == 6 && gm.front() == 0xF0 && gm.back() == 0xF7, "GM reset framing");
    auto mt = dmidi::sysexResetMessage(dmidi::kSysexResetMt32);
    CHECK(mt.size() == 11 && mt[3] == 0x16 && mt[4] == 0x12, "MT-32 reset model/command");
    CHECK(dmidi::sysexResetSettleMs(dmidi::kSysexResetMt32) >= 40, "MT-32 needs settle time");
    std::vector<uint8_t> blob{0x00, 0xF0, 0x41, 0x10, 0xF7, 0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7};
    auto msgs = dmidi::parseSysexMessages(blob);
    CHECK(msgs.size() == 2, "parse concatenated sysex");
    CHECK(msgs[1] == gm, "second message is GM reset");

    CHECK(std::string(dmidi::patchName(dmidi::kInstrumentMapMt32, 0)) == "Acou Piano 1", "MT-32 piano");
    CHECK(std::string(dmidi::patchName(dmidi::kInstrumentMapGm, 0)) == "Acoustic Grand Piano", "GM piano");
    auto mtProf = dmidi::inferDeviceProfile("Munt:MT-32 Synth (128:0)");
    CHECK(mtProf.first == dmidi::kInstrumentMapMt32 && mtProf.second == dmidi::kSysexResetMt32,
          "infer munt/mt32");
    auto scProf = dmidi::inferDeviceProfile("Roland:SC-55 (20:0)");
    CHECK(scProf.first == dmidi::kInstrumentMapGs && scProf.second == dmidi::kSysexResetGs,
          "infer sc-55");
    auto rtProf = dmidi::inferDeviceProfile("USB MIDI:RT-55 Port 1");
    CHECK(rtProf.first == dmidi::kInstrumentMapGs, "infer rt-55 as GS/SC-55 family");

    auto tmpdir = std::filesystem::temp_directory_path();
    auto midPath = tmpdir / "dmidi-syx-song.mid";
    auto syx = tmpdir / "dmidi-syx-song.syx";
    {
        std::ofstream out(syx, std::ios::binary);
        out.write(reinterpret_cast<const char*>(mt.data()), static_cast<std::streamsize>(mt.size()));
    }
    CHECK(dmidi::companionSyxPath(midPath.string()) == syx.string(), "companion .syx next to midi");
    auto loaded = dmidi::loadSysexFile(syx.string());
    CHECK(loaded.size() == 1 && loaded[0] == mt, "load .syx file");
    std::filesystem::remove(syx);

    if (g_fail) {
        std::cerr << g_fail << " checks failed\n";
        return 1;
    }
    std::cout << "All sequence/playlist checks passed\n";
    return 0;
}
