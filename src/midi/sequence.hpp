/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#pragma once

#include "events.hpp"

#include <chrono>
#include <map>
#include <string>
#include <utility>
#include <vector>

namespace dmidi {

struct TextRec {
    int64_t tick{};
    int track{};
    TextType type{TextType::None};
    std::vector<uint8_t> text;
};

class Sequence {
public:
    bool loadFile(const std::string& fileName);
    void clear();

    bool empty() const { return m_events.empty(); }
    void resetPosition() { m_pos = 0; }
    bool hasMoreEvents() const { return m_pos < m_events.size(); }
    MidiEvent* nextEvent();
    void setTickPosition(int64_t tick);

    MidiEvent* jumpToBar(int bar);
    MidiEvent* previousBar(const MidiEvent* latest);
    MidiEvent* nextBar(const MidiEvent* latest);
    MidiEvent* nearestBeatByTicks(int64_t ticks);
    MidiEvent* firstBeat();
    int lastBar() const { return m_barCount; }

    int format() const { return m_format; }
    int division() const { return m_division; }
    int numTracks() const { return m_numTracks; }
    int songLengthTicks() const { return static_cast<int>(m_ticksDuration); }
    int lowestNote() const { return m_lowestNote; }
    int highestNote() const { return m_highestNote; }

    double tempoFactor() const { return m_tempoFactor; }
    void setTempoFactor(double factor);
    double currentTempo() const { return m_tempo / m_tempoFactor; }
    double initialTempo() const { return m_initialTempo; }
    void updateTempo(double newTempo);
    bool simpleTimeProcess() const { return m_numTempoChanges <= 1; }

    std::chrono::microseconds timeOfEvent(const MidiEvent& ev) const;
    std::chrono::microseconds deltaTimeOfEvent(const MidiEvent& ev) const;
    std::chrono::microseconds timeOfTicks(uint64_t ticks) const;

    bool channelUsed(int channel) const;
    std::string channelLabel(int channel) const;
    int trackMaxPoints() const;
    int typeMaxPoints() const;
    int getNumTracks() const { return m_numTracks; }
    std::string trackName(int track) const;
    int trackChannel(int track) const;

    std::string currentFile() const { return m_currentFile; }
    std::string currentFullFileName() const { return m_currentFileFull; }
    std::string fileFormat() const { return m_fileFormat; }
    std::string durationString() const;
    std::string metadataInfo() const;
    std::string loadingErrors() const;
    int errorsCount() const { return static_cast<int>(m_loadingErrors.size()); }

    std::string currentCharset() const { return m_charset; }
    void setCurrentCharset(const std::string& charset);
    std::string decodeText(const std::vector<uint8_t>& data) const;
    std::vector<std::string> getText(TextType type) const;
    std::vector<std::pair<int, std::vector<uint8_t>>> getRawText(int track, TextType type) const;
    const std::vector<TextRec>& textEvents() const { return m_textEvents; }

    static std::vector<std::string> extraCodecNames();

    size_t size() const { return m_events.size(); }
    const std::vector<MidiEvent>& events() const { return m_events; }

private:
    struct TimeSigRec {
        int bar{};
        int num{4};
        int den{4};
        int64_t time{};
    };
    struct TrackMapRec {
        int channel{-1};
        int pitch{};
        int velocity{};
    };

    bool loadSmfBytes(const uint8_t* data, size_t size, const std::string& formatLabel);
    bool loadRmi(const std::vector<uint8_t>& bytes);
    bool loadWrk(const std::vector<uint8_t>& bytes);
    void sortAndFinalize();
    void insertBeatsToEnd();
    void addMetaData(int64_t time, int track, int type, const std::vector<uint8_t>& data);
    void feedCharset(const std::vector<uint8_t>& data);
    void detectCharset();

    std::vector<MidiEvent> m_events;
    std::vector<TextRec> m_textEvents;
    std::vector<std::string> m_loadingErrors;
    std::map<std::string, std::string> m_infoMap;
    std::map<int, std::string> m_trkName;
    std::map<int, int> m_trkScore;
    std::map<int, int> m_typScore;
    std::map<int, int> m_trkChannel;
    std::map<int, TrackMapRec> m_trackMap;
    std::map<int, MidiEvent> m_savedSysex;
    std::vector<TimeSigRec> m_bars;

    std::string m_currentFile;
    std::string m_currentFileFull;
    std::string m_fileFormat;
    std::string m_charset{"UTF-8"};
    std::string m_lblName;

    size_t m_pos{};
    int m_format{};
    int m_numTracks{};
    int64_t m_ticksDuration{};
    int m_division{120};
    int m_lowestNote{127};
    int m_highestNote{0};
    int m_barCount{1};
    int m_beatMax{4};
    int64_t m_beatLength{120};
    double m_tempo{500000.0};
    double m_tempoFactor{1.0};
    double m_ticks2micros{0};
    double m_initialTempo{500000.0};
    int m_numTempoChanges{};
    bool m_channelUsed[kMidiChannels]{};
    int m_channelEvents[kMidiChannels]{};
    std::string m_channelLabel[kMidiChannels];
    std::vector<uint8_t> m_charsetSample;
};

} // namespace dmidi
