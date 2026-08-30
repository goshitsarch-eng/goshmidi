/*
    Gosh MIDI Player — Qt6/Kirigami
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors

    This program is free software; you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation; either version 3 of the License, or
    (at your option) any later version.
*/

#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace dmidi {

constexpr int kMidiChannels = 16;
constexpr int kGmDrumChannel = 9; // 0-based channel 10

constexpr int kStatusNoteOff = 0x80;
constexpr int kStatusNoteOn = 0x90;
constexpr int kStatusKeyPressure = 0xA0;
constexpr int kStatusControlChange = 0xB0;
constexpr int kStatusProgramChange = 0xC0;
constexpr int kStatusChannelPressure = 0xD0;
constexpr int kStatusPitchBend = 0xE0;

constexpr int kCcBankMsb = 0x00;
constexpr int kCcBankLsb = 0x20;
constexpr int kCcVolume = 0x07;
constexpr int kCcVolumeLsb = 0x27;
constexpr int kCcAllSoundsOff = 0x78;
constexpr int kCcResetControllers = 0x79;
constexpr int kCcAllNotesOff = 0x7B;

enum class TextType {
    None = 0,
    Text = 1,
    Copyright = 2,
    TrackName = 3,
    InstrumentName = 4,
    Lyric = 5,
    Marker = 6,
    Cue = 7,
    KarFileType = 8,
    KarVersion = 9,
    KarInformation = 10,
    KarLanguage = 11,
    KarTitles = 12,
    KarWhatever = 13,
    First = Text,
    Last = KarWhatever
};

struct MidiEvent {
    enum class Kind : uint8_t {
        NoteOn,
        NoteOff,
        KeyPressure,
        ControlChange,
        ProgramChange,
        ChannelPressure,
        PitchBend,
        SysEx,
        Text,
        Tempo,
        TimeSignature,
        KeySignature,
        Beat
    };

    Kind kind{};
    int64_t tick{};
    int64_t delta{};
    int tag{};     // track number
    int channel{};
    int a{};       // note / controller / program / numerator / bar / text type
    int b{};       // velocity / value / denominator / beat / alterations
    int c{};       // extra (bar length, minor mode)
    double tempo{500000.0};
    std::vector<uint8_t> data;

    bool isChannel() const
    {
        return kind == Kind::NoteOn || kind == Kind::NoteOff || kind == Kind::KeyPressure
            || kind == Kind::ControlChange || kind == Kind::ProgramChange
            || kind == Kind::ChannelPressure || kind == Kind::PitchBend;
    }

    bool isMeta() const { return !isChannel(); }

    int status() const
    {
        switch (kind) {
        case Kind::NoteOff:
            return kStatusNoteOff;
        case Kind::NoteOn:
            return kStatusNoteOn;
        case Kind::KeyPressure:
            return kStatusKeyPressure;
        case Kind::ControlChange:
            return kStatusControlChange;
        case Kind::ProgramChange:
            return kStatusProgramChange;
        case Kind::ChannelPressure:
            return kStatusChannelPressure;
        case Kind::PitchBend:
            return kStatusPitchBend;
        default:
            return 0;
        }
    }
};

inline MidiEvent makeNoteOn(int ch, int note, int vel)
{
    MidiEvent e;
    e.kind = MidiEvent::Kind::NoteOn;
    e.channel = ch;
    e.a = note;
    e.b = vel;
    return e;
}

inline MidiEvent makeNoteOff(int ch, int note, int vel)
{
    MidiEvent e;
    e.kind = MidiEvent::Kind::NoteOff;
    e.channel = ch;
    e.a = note;
    e.b = vel;
    return e;
}

inline MidiEvent makeControl(int ch, int cc, int val)
{
    MidiEvent e;
    e.kind = MidiEvent::Kind::ControlChange;
    e.channel = ch;
    e.a = cc;
    e.b = val;
    return e;
}

inline MidiEvent makeProgram(int ch, int pgm)
{
    MidiEvent e;
    e.kind = MidiEvent::Kind::ProgramChange;
    e.channel = ch;
    e.a = pgm;
    return e;
}

inline MidiEvent makeTempo(double microsecondsPerQuarter)
{
    MidiEvent e;
    e.kind = MidiEvent::Kind::Tempo;
    e.tempo = microsecondsPerQuarter;
    return e;
}

inline MidiEvent makeBeat(int bar, int beat, int max)
{
    MidiEvent e;
    e.kind = MidiEvent::Kind::Beat;
    e.a = bar;
    e.b = beat;
    e.c = max;
    return e;
}

inline double tempoToBpm(double tempoUs)
{
    if (tempoUs <= 0)
        return 120.0;
    return 60000000.0 / tempoUs;
}

inline int clampMidi(int v)
{
    if (v < 0)
        return 0;
    if (v > 127)
        return 127;
    return v;
}

// A "locator" is either a local filesystem path or a URL a KIO worker can
// reach (smb://, sftp://, nfs://, dav://, …).
bool isRemoteLocator(const std::string& locator);
std::string locatorFileName(const std::string& locator);
bool isSupportedMidiFile(const std::string& path);
bool isPlaylistFile(const std::string& path);

} // namespace dmidi
