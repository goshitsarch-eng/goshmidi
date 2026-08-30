/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#pragma once

#include "events.hpp"
#include "output.hpp"
#include "sequence.hpp"

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <functional>
#include <mutex>
#include <string>
#include <thread>

namespace dmidi {

class SequencePlayer {
public:
    using TimeCb = std::function<void(std::chrono::milliseconds, int64_t)>;
    using BeatCb = std::function<void(int bar, int beat, int max)>;
    using NoteCb = std::function<void(int ch, int note, int vel)>;
    using TextCb = std::function<void(int track, int type, int64_t ticks, const std::vector<uint8_t>&)>;
    using ProgramCb = std::function<void(int ch, int pgm)>;
    using TempoCb = std::function<void(double tempoUs)>;
    using KeySigCb = std::function<void(int track, int alt, bool minor)>;
    using VoidCb = std::function<void()>;
    using TimeSigCb = std::function<void(int bar, int num, int den)>;

    SequencePlayer();
    ~SequencePlayer();

    void setOutput(MidiOutput* out) { m_out = out; }
    MidiOutput* output() const { return m_out; }

    bool loadFile(const std::string& fileName);
    Sequence& song() { return m_song; }
    const Sequence& song() const { return m_song; }

    void play();
    void pause();
    void stop();
    bool isRunning() const { return m_running.load(); }
    bool isPaused() const { return m_paused.load(); }

    void resetPosition();
    void setPosition(int64_t ticks);
    int64_t position() const { return m_songPositionTicks.load(); }
    void jumpToBar(int bar);
    void beatForward();
    void beatBackward();

    void setPitchShift(int pitch);
    int pitchShift() const { return m_pitchShift; }
    void setVolumeFactor(int vol);
    int volumeFactor() const { return m_volumeFactor; }

    void setVolume(int channel, double factor);
    void setMuted(int channel, bool mute);
    void setLocked(int channel, bool lock);
    void setPatch(int channel, int patch);
    double volume(int channel) const;
    bool isMuted(int channel) const;
    bool isLocked(int channel) const;

    void setLoop(bool enabled);
    void setLoop(int startBar, int endBar);
    bool loopEnabled() const { return m_loopEnabled; }
    int loopStart() const { return m_loopStart; }
    int loopEnd() const { return m_loopEnd; }

    void setDrumsChannel(int ch0based) { m_drumsChannel = ch0based; }
    int drumsChannel() const { return m_drumsChannel; }
    void setSysexReset(int kind) { m_sysexReset = kind; } // 0 none, 1 GM, 2 GS, 3 XG, 4 MT-32
    int sysexReset() const { return m_sysexReset; }
    void setCompanionSyx(const std::string& path) { m_companionSyx = path; }

    void allNotesOff();
    void resetControllers();
    void resetPrograms();
    void sendResetMessage();
    void sendCompanionSysex();
    void sendVolumeEvents();
    void shutupSound();

    double currentBpm() const;
    double initialBpm() const;

    TimeCb onTime;
    BeatCb onBeat;
    NoteCb onNoteOn;
    NoteCb onNoteOff;
    TextCb onText;
    ProgramCb onProgram;
    TempoCb onTempo;
    KeySigCb onKeySignature;
    TimeSigCb onTimeSignature;
    VoidCb onStarted;
    VoidCb onStopped;
    VoidCb onFinished;

private:
    void playerLoop();
    void playEvent(const MidiEvent& ev);
    void initChannels();
    int boundedFloor(int initial, double factor) const;
    void emitToUi(const std::function<void()>& fn);
    void sendSysexPaced(const std::vector<uint8_t>& data);

    Sequence m_song;
    MidiOutput* m_out{};
    std::thread m_thread;
    std::atomic<bool> m_running{false};
    std::atomic<bool> m_stopRequested{false};
    std::atomic<bool> m_paused{false};
    std::atomic<int64_t> m_songPositionTicks{0};

    bool m_loopEnabled{};
    int m_loopStart{1};
    int m_loopEnd{1};
    int m_pitchShift{};
    int m_volumeFactor{100};
    int m_drumsChannel{kGmDrumChannel};
    int m_sysexReset{};
    bool m_resumeWithoutReset{};
    std::string m_companionSyx;
    int m_volume[kMidiChannels]{};
    int m_lastPgm[kMidiChannels]{};
    int m_lockedPgm[kMidiChannels]{};
    double m_volumeShift[kMidiChannels]{};
    bool m_muted[kMidiChannels]{};
    bool m_locked[kMidiChannels]{};
    const MidiEvent* m_latestBeat{};
    const MidiEvent* m_firstBeat{};
};

} // namespace dmidi
