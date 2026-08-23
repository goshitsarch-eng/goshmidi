/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#include "player.hpp"
#include "sysex.hpp"

#include <algorithm>
#include <cmath>
#include <glib.h>

namespace dmidi {

namespace {
struct IdleJob {
    std::function<void()> fn;
};

gboolean idleTrampoline(gpointer data)
{
    auto* job = static_cast<IdleJob*>(data);
    job->fn();
    delete job;
    return G_SOURCE_REMOVE;
}
} // namespace

SequencePlayer::SequencePlayer()
{
    initChannels();
}

SequencePlayer::~SequencePlayer()
{
    stop();
}

void SequencePlayer::initChannels()
{
    for (int ch = 0; ch < kMidiChannels; ++ch) {
        m_lastPgm[ch] = 0;
        m_volumeShift[ch] = 1.0;
        m_volume[ch] = 100;
        m_muted[ch] = false;
        m_locked[ch] = false;
        m_lockedPgm[ch] = 0;
    }
}

int SequencePlayer::boundedFloor(int initial, double factor) const
{
    int temp = static_cast<int>(std::floor(initial * factor));
    return clampMidi(temp);
}

void SequencePlayer::emitToUi(const std::function<void()>& fn)
{
    if (!fn)
        return;
    g_idle_add(idleTrampoline, new IdleJob{fn});
}

bool SequencePlayer::loadFile(const std::string& fileName)
{
    stop();
    m_paused = false;
    m_resumeWithoutReset = false;
    bool ok = m_song.loadFile(fileName);
    m_companionSyx = companionSyxPath(fileName);
    m_songPositionTicks = 0;
    m_firstBeat = m_song.firstBeat();
    m_latestBeat = m_firstBeat;
    m_loopStart = 1;
    m_loopEnd = m_song.lastBar();
    m_loopEnabled = false;
    initChannels();
    return ok;
}

void SequencePlayer::play()
{
    if (m_running.load())
        return;
    if (m_song.empty())
        return;
    m_resumeWithoutReset = m_paused.load();
    m_stopRequested = false;
    m_paused = false;
    m_running = true;
    if (m_thread.joinable())
        m_thread.join();
    m_thread = std::thread([this] { playerLoop(); });
}

void SequencePlayer::pause()
{
    if (!m_running.load())
        return;
    m_paused = true;
    m_stopRequested = true;
    if (m_thread.joinable())
        m_thread.join();
    m_running = false;
    allNotesOff();
}

void SequencePlayer::stop()
{
    m_paused = false;
    m_resumeWithoutReset = false;
    m_stopRequested = true;
    if (m_thread.joinable())
        m_thread.join();
    m_running = false;
    shutupSound();
}

void SequencePlayer::resetPosition()
{
    if (!m_song.empty()) {
        m_song.resetPosition();
        m_songPositionTicks = 0;
        m_latestBeat = m_firstBeat;
    }
}

void SequencePlayer::setPosition(int64_t ticks)
{
    allNotesOff();
    m_song.setTickPosition(ticks);
    m_songPositionTicks = ticks;
}

void SequencePlayer::jumpToBar(int bar)
{
    allNotesOff();
    auto* ev = m_song.jumpToBar(bar);
    if (ev) {
        m_latestBeat = ev;
        m_songPositionTicks = ev->tick;
        m_song.setTickPosition(ev->tick);
    }
}

void SequencePlayer::beatForward()
{
    auto* ev = m_song.nextBar(m_latestBeat);
    if (ev) {
        jumpToBar(ev->a);
    }
}

void SequencePlayer::beatBackward()
{
    auto* ev = m_song.previousBar(m_latestBeat);
    if (ev) {
        jumpToBar(ev->a);
    }
}

void SequencePlayer::setPitchShift(int pitch)
{
    allNotesOff();
    m_pitchShift = pitch;
}

void SequencePlayer::setVolumeFactor(int vol)
{
    m_volumeFactor = vol;
    sendVolumeEvents();
}

void SequencePlayer::setVolume(int channel, double factor)
{
    if (channel < 0 || channel >= kMidiChannels)
        return;
    m_volumeShift[channel] = factor;
    if (m_out) {
        int value = boundedFloor(m_volume[channel], m_volumeShift[channel] * m_volumeFactor / 100.0);
        m_out->sendController(channel, kCcVolume, value);
    }
}

void SequencePlayer::setMuted(int channel, bool mute)
{
    if (channel < 0 || channel >= kMidiChannels)
        return;
    m_muted[channel] = mute;
    if (mute && m_out) {
        m_out->sendController(channel, kCcAllNotesOff, 0);
        m_out->sendController(channel, kCcAllSoundsOff, 0);
    }
}

void SequencePlayer::setLocked(int channel, bool lock)
{
    if (channel < 0 || channel >= kMidiChannels)
        return;
    m_locked[channel] = lock;
    if (lock)
        m_lockedPgm[channel] = m_lastPgm[channel];
}

void SequencePlayer::setPatch(int channel, int patch)
{
    if (channel < 0 || channel >= kMidiChannels)
        return;
    m_lastPgm[channel] = patch;
    if (m_locked[channel])
        m_lockedPgm[channel] = patch;
    if (m_out)
        m_out->sendProgram(channel, patch);
}

double SequencePlayer::volume(int channel) const
{
    if (channel < 0 || channel >= kMidiChannels)
        return 1.0;
    return m_volumeShift[channel];
}

bool SequencePlayer::isMuted(int channel) const
{
    return channel >= 0 && channel < kMidiChannels && m_muted[channel];
}

bool SequencePlayer::isLocked(int channel) const
{
    return channel >= 0 && channel < kMidiChannels && m_locked[channel];
}

void SequencePlayer::setLoop(bool enabled)
{
    m_loopEnabled = enabled;
}

void SequencePlayer::setLoop(int startBar, int endBar)
{
    m_loopEnabled = true;
    m_loopStart = startBar;
    m_loopEnd = endBar;
}

void SequencePlayer::allNotesOff()
{
    if (!m_out)
        return;
    for (int ch = 0; ch < kMidiChannels; ++ch) {
        m_out->sendController(ch, kCcAllNotesOff, 0);
        m_out->sendController(ch, kCcAllSoundsOff, 0);
    }
}

void SequencePlayer::shutupSound()
{
    allNotesOff();
}

void SequencePlayer::resetControllers()
{
    if (!m_out)
        return;
    for (int ch = 0; ch < kMidiChannels; ++ch) {
        m_out->sendController(ch, kCcResetControllers, 0);
        m_out->sendController(ch, kCcVolume, 100);
    }
}

void SequencePlayer::resetPrograms()
{
    if (!m_out)
        return;
    for (int ch = 0; ch < kMidiChannels; ++ch) {
        int pgm = m_locked[ch] ? m_lockedPgm[ch] : 0;
        m_out->sendProgram(ch, pgm);
    }
}

void SequencePlayer::sendSysexPaced(const std::vector<uint8_t>& data)
{
    if (!m_out || data.empty())
        return;
    auto msgs = parseSysexMessages(data);
    if (msgs.empty()) {
        m_out->sendSysex(data);
        return;
    }
    for (auto& msg : msgs) {
        m_out->sendSysex(msg);
        int wait = sysexPacingMs(msg.size());
        if (wait > 0)
            std::this_thread::sleep_for(std::chrono::milliseconds(wait));
    }
}

void SequencePlayer::sendResetMessage()
{
    if (!m_out)
        return;
    auto msg = sysexResetMessage(m_sysexReset);
    if (msg.empty())
        return;
    m_out->sendSysex(msg);
    int wait = sysexResetSettleMs(m_sysexReset);
    if (wait > 0)
        std::this_thread::sleep_for(std::chrono::milliseconds(wait));
}

void SequencePlayer::sendCompanionSysex()
{
    if (!m_out || m_companionSyx.empty())
        return;
    auto msgs = loadSysexFile(m_companionSyx);
    for (auto& msg : msgs) {
        m_out->sendSysex(msg);
        int wait = std::max(sysexPacingMs(msg.size()), 15);
        std::this_thread::sleep_for(std::chrono::milliseconds(wait));
    }
}

void SequencePlayer::sendVolumeEvents()
{
    if (!m_out)
        return;
    for (int ch = 0; ch < kMidiChannels; ++ch) {
        m_volume[ch] = 100;
        int value = boundedFloor(100, m_volumeShift[ch] * m_volumeFactor / 100.0);
        m_out->sendController(ch, kCcVolume, value);
    }
}

double SequencePlayer::currentBpm() const
{
    return tempoToBpm(m_song.currentTempo());
}

double SequencePlayer::initialBpm() const
{
    return tempoToBpm(m_song.initialTempo());
}

void SequencePlayer::playEvent(const MidiEvent& ev)
{
    if (!m_out)
        return;
    if (ev.isChannel()) {
        int chan = ev.channel;
        if (m_muted[chan])
            return;
        switch (ev.kind) {
        case MidiEvent::Kind::NoteOff: {
            int key = ev.a;
            if (chan != m_drumsChannel)
                key += m_pitchShift;
            m_out->sendNoteOff(chan, key, ev.b);
            if (onNoteOff) {
                int k = key, c = chan, v = ev.b;
                emitToUi([this, c, k, v] { onNoteOff(c, k, v); });
            }
            break;
        }
        case MidiEvent::Kind::NoteOn: {
            int key = ev.a;
            if (chan != m_drumsChannel)
                key += m_pitchShift;
            m_out->sendNoteOn(chan, key, ev.b);
            if (onNoteOn) {
                int k = key, c = chan, v = ev.b;
                emitToUi([this, c, k, v] { onNoteOn(c, k, v); });
            }
            break;
        }
        case MidiEvent::Kind::KeyPressure: {
            int key = ev.a;
            if (chan != m_drumsChannel)
                key += m_pitchShift;
            m_out->sendKeyPressure(chan, key, ev.b);
            break;
        }
        case MidiEvent::Kind::ControlChange: {
            int val = ev.b;
            if (ev.a == kCcVolume) {
                m_volume[chan] = val;
                val = boundedFloor(val, m_volumeShift[chan] * m_volumeFactor / 100.0);
            }
            m_out->sendController(chan, ev.a, val);
            break;
        }
        case MidiEvent::Kind::ProgramChange: {
            int pgm = m_locked[chan] ? m_lockedPgm[chan] : ev.a;
            m_out->sendProgram(chan, pgm);
            m_lastPgm[chan] = pgm;
            if (onProgram)
                emitToUi([this, chan, pgm] { onProgram(chan, pgm); });
            break;
        }
        case MidiEvent::Kind::ChannelPressure:
            m_out->sendChannelPressure(chan, ev.a);
            break;
        case MidiEvent::Kind::PitchBend:
            m_out->sendPitchBend(chan, ev.a);
            break;
        default:
            break;
        }
    } else {
        switch (ev.kind) {
        case MidiEvent::Kind::SysEx:
            sendSysexPaced(ev.data);
            break;
        case MidiEvent::Kind::Text:
            if (onText) {
                auto data = ev.data;
                int track = ev.tag, type = ev.a;
                int64_t ticks = ev.tick;
                emitToUi([this, track, type, ticks, data] { onText(track, type, ticks, data); });
            }
            break;
        case MidiEvent::Kind::Tempo:
            m_song.updateTempo(ev.tempo);
            if (onTempo) {
                double t = m_song.currentTempo();
                emitToUi([this, t] { onTempo(t); });
            }
            break;
        case MidiEvent::Kind::Beat:
            m_latestBeat = &ev;
            if (onBeat) {
                int bar = ev.a, beat = ev.b, max = ev.c;
                emitToUi([this, bar, beat, max] { onBeat(bar, beat, max); });
            }
            break;
        case MidiEvent::Kind::TimeSignature:
            if (onTimeSignature) {
                int bar = ev.tag, n = ev.a, d = ev.b;
                emitToUi([this, bar, n, d] { onTimeSignature(bar, n, d); });
            }
            break;
        case MidiEvent::Kind::KeySignature:
            if (onKeySignature) {
                int track = ev.tag, alt = ev.a;
                bool minor = ev.c != 0;
                emitToUi([this, track, alt, minor] { onKeySignature(track, alt, minor); });
            }
            break;
        default:
            break;
        }
    }
}

void SequencePlayer::playerLoop()
{
    using namespace std::chrono;
    using Clock = steady_clock;
    const bool simple = m_song.simpleTimeProcess();
    int64_t echoTicks = 0;
    const int echoRes = 50;
    auto currentTime = Clock::now();
    auto startTime = currentTime;
    if (!m_resumeWithoutReset) {
        sendResetMessage();
        sendCompanionSysex();
        resetControllers();
        sendVolumeEvents();
    }
    m_resumeWithoutReset = false;
    if (onStarted)
        emitToUi([this] { onStarted(); });

    do {
        while (m_song.hasMoreEvents() && !m_stopRequested.load()) {
            MidiEvent* ev = m_song.nextEvent();
            if (!ev)
                break;
            if (ev->kind == MidiEvent::Kind::Beat) {
                if (m_loopEnabled && ev->a > m_loopEnd)
                    break;
            }
            if (ev->delta > 0) {
                Clock::time_point nextTime;
                if (simple)
                    nextTime = startTime + m_song.timeOfEvent(*ev);
                else
                    nextTime = currentTime + m_song.deltaTimeOfEvent(*ev);
                auto echoDelta = m_song.timeOfTicks(echoRes);
                auto nextEcho = currentTime + echoDelta;
                while (nextEcho < nextTime && !m_stopRequested.load()) {
                    std::this_thread::sleep_until(nextEcho);
                    echoTicks += echoRes;
                    auto ms = duration_cast<milliseconds>(m_song.timeOfTicks(echoTicks));
                    int64_t t = echoTicks;
                    if (onTime)
                        emitToUi([this, ms, t] { onTime(ms, t); });
                    currentTime = Clock::now();
                    nextEcho = currentTime + echoDelta;
                }
                if (m_stopRequested.load())
                    break;
                std::this_thread::sleep_until(nextTime);
                echoTicks = ev->tick;
                m_songPositionTicks = echoTicks;
                currentTime = Clock::now();
                auto ms = duration_cast<milliseconds>(m_song.timeOfTicks(echoTicks));
                int64_t t = echoTicks;
                if (onTime)
                    emitToUi([this, ms, t] { onTime(ms, t); });
            }
            playEvent(*ev);
        }
        if (m_loopEnabled && !m_stopRequested.load()) {
            jumpToBar(m_loopStart);
            currentTime = Clock::now();
            startTime = currentTime;
            echoTicks = m_songPositionTicks.load();
        }
    } while (m_song.hasMoreEvents() && !m_stopRequested.load());

    bool finished = !m_song.hasMoreEvents() && !m_stopRequested.load();
    m_running = false;
    allNotesOff();
    if (onStopped)
        emitToUi([this] { onStopped(); });
    if (finished && onFinished)
        emitToUi([this] { onFinished(); });
}

} // namespace dmidi
