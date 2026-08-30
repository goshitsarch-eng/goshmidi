/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#pragma once

#include <algorithm>
#include <array>
#include <cctype>
#include <string>
#include <utility>

namespace dmidi {

constexpr int kInstrumentMapGm = 0;
constexpr int kInstrumentMapGs = 1;   // Roland SC-55 / Sound Canvas capital tones
constexpr int kInstrumentMapMt32 = 2; // Roland MT-32 / CM-32L

inline const std::array<const char*, 128> kGmPatchNames = {
    "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-Tonk",
    "Rhodes Piano", "Chorused Piano", "Harpsichord", "Clavinet",
    "Celesta", "Glockenspiel", "Music Box", "Vibraphone",
    "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
    "Hammond Organ", "Percussive Organ", "Rock Organ", "Church Organ",
    "Reed Organ", "Accordion", "Harmonica", "Tango Accordion",
    "Acoustic Guitar (Nylon)", "Acoustic Guitar (Steel)", "Electric Guitar (Jazz)",
    "Electric Guitar (Clean)", "Electric Guitar (Muted)", "Overdriven Guitar",
    "Distortion Guitar", "Guitar Harmonics",
    "Acoustic Bass", "Electric Bass (Finger)", "Electric Bass (Pick)", "Fretless Bass",
    "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
    "Violin", "Viola", "Cello", "Contrabass",
    "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
    "String Ensemble 1", "String Ensemble 2", "Synth Strings 1", "Synth Strings 2",
    "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
    "Trumpet", "Trombone", "Tuba", "Muted Trumpet",
    "French Horn", "Brass Section", "Synth Brass 1", "Synth Brass 2",
    "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax",
    "Oboe", "English Horn", "Bassoon", "Clarinet",
    "Piccolo", "Flute", "Recorder", "Pan Flute",
    "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina",
    "Lead 1 - Square Wave", "Lead 2 - Saw Tooth", "Lead 3 - Calliope", "Lead 4 - Chiflead",
    "Lead 5 - Charang", "Lead 6 - Voice", "Lead 7 - Fifths", "Lead 8 - Bass+Lead",
    "Pad 1 - New Age", "Pad 2 - Warm", "Pad 3 - Polysynth", "Pad 4 - Choir",
    "Pad 5 - Bow", "Pad 6 - Metallic", "Pad 7 - Halo", "Pad 8 - Sweep",
    "FX 1 - Rain", "FX 2 - Soundtrack", "FX 3 - Crystal", "FX 4 - Atmosphere",
    "FX 5 - Brightness", "FX 6 - Goblins", "FX 7 - Echoes", "FX 8 - Sci-fi",
    "Sitar", "Banjo", "Shamisen", "Koto",
    "Kalimba", "Bagpipe", "Fiddle", "Shannai",
    "Tinkle Bell", "Agogo", "Steel Drum", "Wood Block",
    "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
    "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet",
    "Telephone", "Helicopter", "Applause", "Gunshot"};

// Roland MT-32 / CM-32L preset patch names (program 0–127).
inline const std::array<const char*, 128> kMt32PatchNames = {
    "Acou Piano 1", "Acou Piano 2", "Acou Piano 3", "Elec Piano 1",
    "Elec Piano 2", "Elec Piano 3", "Elec Piano 4", "Honkytonk",
    "Elec Org 1", "Elec Org 2", "Elec Org 3", "Elec Org 4",
    "Pipe Org 1", "Pipe Org 2", "Pipe Org 3", "Accordion",
    "Harpsi 1", "Harpsi 2", "Harpsi 3", "Clavi 1",
    "Clavi 2", "Clavi 3", "Celesta 1", "Celesta 2",
    "Syn Brass 1", "Syn Brass 2", "Syn Brass 3", "Syn Brass 4",
    "Syn Bass 1", "Syn Bass 2", "Syn Bass 3", "Syn Bass 4",
    "Fantasy", "Harmo Pan", "Chorale", "Glasses",
    "Soundtrack", "Atmosphere", "Warm Bell", "Funny Vox",
    "Echo Bell", "Ice Rain", "Oboe 2001", "Echo Pan",
    "Doctor Solo", "School Daze", "Bellsinger", "Square Wave",
    "Str Sect 1", "Str Sect 2", "Str Sect 3", "Pizzicato",
    "Violin 1", "Violin 2", "Cello 1", "Cello 2",
    "Contrabass", "Harp 1", "Harp 2", "Guitar 1",
    "Guitar 2", "Elec Gtr 1", "Elec Gtr 2", "Sitar",
    "Acou Bass 1", "Acou Bass 2", "Elec Bass 1", "Elec Bass 2",
    "Slap Bass 1", "Slap Bass 2", "Fretless 1", "Fretless 2",
    "Flute 1", "Flute 2", "Piccolo 1", "Piccolo 2",
    "Recorder", "Pan Pipes", "Sax 1", "Sax 2",
    "Sax 3", "Sax 4", "Clarinet 1", "Clarinet 2",
    "Oboe", "Engl Horn", "Bassoon", "Harmonica",
    "Trumpet 1", "Trumpet 2", "Trombone 1", "Trombone 2",
    "Fr Horn 1", "Fr Horn 2", "Tuba", "Brs Sect 1",
    "Brs Sect 2", "Vibe 1", "Vibe 2", "Marimba",
    "Koto", "Sho", "Shakuhachi", "Whistle 1",
    "Whistle 2", "Bottle Blow", "Breath Pipe", "Timpani",
    "Melodic Tom", "Deep Snare", "Elec Perc 1", "Elec Perc 2",
    "Taiko", "Taiko Rim", "Cymbal", "Castanets",
    "Triangle", "Orche Hit", "Telephone", "Bird Tweet",
    "One Note Jam", "Water Bells", "Jungle Tune", "Nightmare",
    "Wind", "Atmosphere 2", "Ninja", "Hollow"};

inline const char* instrumentMapName(int map)
{
    switch (map) {
    case kInstrumentMapGs:
        return "GS (SC-55)";
    case kInstrumentMapMt32:
        return "MT-32";
    default:
        return "GM";
    }
}

inline const char* gmPatchName(int patch)
{
    if (patch < 0 || patch > 127)
        return "Unknown";
    return kGmPatchNames[static_cast<size_t>(patch)];
}

inline const char* mt32PatchName(int patch)
{
    if (patch < 0 || patch > 127)
        return "Unknown";
    return kMt32PatchNames[static_cast<size_t>(patch)];
}

inline const char* patchName(int map, int patch)
{
    if (map == kInstrumentMapMt32)
        return mt32PatchName(patch);
    return gmPatchName(patch);
}

inline std::string percussionName(int patch)
{
    return "Percussion " + std::to_string(patch + 1);
}

// Guess map + SysEx reset from an ALSA/FluidSynth port label.
inline std::pair<int, int> inferDeviceProfile(const std::string& label)
{
    auto lower = label;
    std::transform(lower.begin(), lower.end(), lower.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    auto has = [&](const char* s) { return lower.find(s) != std::string::npos; };
    // Munt's second port remaps GM onto the MT-32; keep GM names/reset there.
    if (has("gm emulation") || has("gm-emulation"))
        return {kInstrumentMapGm, 1};
    if (has("mt-32") || has("mt32") || has("munt") || has("mt32emu") || has("cm-32") || has("cm32")
        || has("cm-64") || has("cm64") || has("lapc"))
        return {kInstrumentMapMt32, 4}; // MT-32 reset
    if (has("sc-55") || has("sc55") || has("sc-88") || has("sc88") || has("sc-8850") || has("canvas")
        || has("rt-55") || has("rt55") || has("nuked") || has("virtual sc55") || has("virtual sc-55"))
        return {kInstrumentMapGs, 2}; // GS reset
    if (has("xg") || has("s-yxg") || has("mu50") || has("mu80") || has("mu100"))
        return {kInstrumentMapGm, 3};
    return {kInstrumentMapGm, 1};
}

} // namespace dmidi
