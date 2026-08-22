/*
    Headless tests for MIDI file loading — feature-parity engine.
*/

#include "midi/sequence.hpp"
#include "app/playlist.hpp"

#include <filesystem>
#include <iostream>

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

    if (g_fail) {
        std::cerr << g_fail << " checks failed\n";
        return 1;
    }
    std::cout << "All sequence/playlist checks passed\n";
    return 0;
}
