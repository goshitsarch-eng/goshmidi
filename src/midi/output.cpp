/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#include "output.hpp"
#include "events.hpp"

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fluidsynth.h>
#include <alsa/asoundlib.h>

namespace dmidi {
namespace {

class DummyOutput : public MidiOutput {
public:
    std::string backendName() const override { return "Dummy"; }
    std::vector<MidiPort> ports(bool) const override { return {{"dummy", "No MIDI output"}}; }
    bool open(const std::string& id) override
    {
        m_port = id.empty() ? "dummy" : id;
        return true;
    }
    void close() override { m_port.clear(); }
    std::string currentPort() const override { return m_port; }
    void sendNoteOn(int, int, int) override {}
    void sendNoteOff(int, int, int) override {}
    void sendKeyPressure(int, int, int) override {}
    void sendController(int, int, int) override {}
    void sendProgram(int, int) override {}
    void sendChannelPressure(int, int) override {}
    void sendPitchBend(int, int) override {}
    void sendSysex(const std::vector<uint8_t>&) override {}

private:
    std::string m_port;
};

#ifdef HAVE_ALSA
#endif

class AlsaOutput : public MidiOutput {
public:
    AlsaOutput()
    {
        if (snd_seq_open(&m_seq, "default", SND_SEQ_OPEN_OUTPUT, 0) < 0) {
            m_error = "Unable to open ALSA sequencer";
            m_seq = nullptr;
            return;
        }
        snd_seq_set_client_name(m_seq, "dmidiplayer");
        m_port = snd_seq_create_simple_port(m_seq,
                                            "MIDI Out",
                                            SND_SEQ_PORT_CAP_READ | SND_SEQ_PORT_CAP_SUBS_READ,
                                            SND_SEQ_PORT_TYPE_MIDI_GENERIC | SND_SEQ_PORT_TYPE_APPLICATION);
        if (m_port < 0) {
            m_error = "Unable to create ALSA port";
        }
    }
    ~AlsaOutput() override
    {
        close();
        if (m_seq)
            snd_seq_close(m_seq);
    }
    std::string backendName() const override { return "ALSA"; }
    std::string lastError() const override { return m_error; }

    std::vector<MidiPort> ports(bool advanced) const override
    {
        std::vector<MidiPort> out;
        if (!m_seq)
            return out;
        snd_seq_client_info_t* cinfo;
        snd_seq_port_info_t* pinfo;
        snd_seq_client_info_alloca(&cinfo);
        snd_seq_port_info_alloca(&pinfo);
        snd_seq_client_info_set_client(cinfo, -1);
        while (snd_seq_query_next_client(m_seq, cinfo) >= 0) {
            int client = snd_seq_client_info_get_client(cinfo);
            if (client == snd_seq_client_id(m_seq))
                continue;
            snd_seq_port_info_set_client(pinfo, client);
            snd_seq_port_info_set_port(pinfo, -1);
            while (snd_seq_query_next_port(m_seq, pinfo) >= 0) {
                unsigned caps = snd_seq_port_info_get_capability(pinfo);
                unsigned type = snd_seq_port_info_get_type(pinfo);
                if (!(caps & SND_SEQ_PORT_CAP_WRITE) || !(caps & SND_SEQ_PORT_CAP_SUBS_WRITE))
                    continue;
                if (!advanced && !(type & (SND_SEQ_PORT_TYPE_MIDI_GENERIC | SND_SEQ_PORT_TYPE_SYNTH
                                           | SND_SEQ_PORT_TYPE_MIDI_GM | SND_SEQ_PORT_TYPE_APPLICATION
                                           | SND_SEQ_PORT_TYPE_HARDWARE | SND_SEQ_PORT_TYPE_PORT
                                           | SND_SEQ_PORT_TYPE_SOFTWARE)))
                    continue;
                char id[32];
                std::snprintf(id, sizeof(id), "%d:%d", client, snd_seq_port_info_get_port(pinfo));
                std::string label = std::string(snd_seq_client_info_get_name(cinfo)) + ":"
                    + snd_seq_port_info_get_name(pinfo) + " (" + id + ")";
                out.push_back({id, label});
            }
        }
        return out;
    }

    bool open(const std::string& portId) override
    {
        m_error.clear();
        if (!m_seq)
            return false;
        close();
        int client = 0, port = 0;
        if (std::sscanf(portId.c_str(), "%d:%d", &client, &port) != 2) {
            auto list = ports(true);
            if (list.empty()) {
                m_error = "No ALSA MIDI destinations";
                return false;
            }
            std::sscanf(list.front().id.c_str(), "%d:%d", &client, &port);
            m_current = list.front().id;
        } else {
            m_current = portId;
        }
        snd_seq_addr_t sender{static_cast<unsigned char>(snd_seq_client_id(m_seq)),
                              static_cast<unsigned char>(m_port)};
        snd_seq_addr_t dest{static_cast<unsigned char>(client), static_cast<unsigned char>(port)};
        snd_seq_port_subscribe_t* sub;
        snd_seq_port_subscribe_alloca(&sub);
        snd_seq_port_subscribe_set_sender(sub, &sender);
        snd_seq_port_subscribe_set_dest(sub, &dest);
        if (snd_seq_subscribe_port(m_seq, sub) < 0) {
            m_error = "Unable to connect ALSA port";
            m_current.clear();
            return false;
        }
        m_dest = dest;
        m_subscribed = true;
        return true;
    }

    void close() override
    {
        if (m_seq && m_subscribed) {
            snd_seq_addr_t sender{static_cast<unsigned char>(snd_seq_client_id(m_seq)),
                                  static_cast<unsigned char>(m_port)};
            snd_seq_port_subscribe_t* sub;
            snd_seq_port_subscribe_alloca(&sub);
            snd_seq_port_subscribe_set_sender(sub, &sender);
            snd_seq_port_subscribe_set_dest(sub, &m_dest);
            snd_seq_unsubscribe_port(m_seq, sub);
            m_subscribed = false;
        }
        m_current.clear();
    }

    std::string currentPort() const override { return m_current; }

    void sendNoteOn(int ch, int note, int vel) override
    {
        event(SND_SEQ_EVENT_NOTEON, ch, note, vel);
    }
    void sendNoteOff(int ch, int note, int vel) override
    {
        event(SND_SEQ_EVENT_NOTEOFF, ch, note, vel);
    }
    void sendKeyPressure(int ch, int note, int vel) override
    {
        event(SND_SEQ_EVENT_KEYPRESS, ch, note, vel);
    }
    void sendController(int ch, int cc, int val) override
    {
        event(SND_SEQ_EVENT_CONTROLLER, ch, cc, val);
    }
    void sendProgram(int ch, int pgm) override { event(SND_SEQ_EVENT_PGMCHANGE, ch, pgm, 0); }
    void sendChannelPressure(int ch, int val) override
    {
        event(SND_SEQ_EVENT_CHANPRESS, ch, val, 0);
    }
    void sendPitchBend(int ch, int value) override
    {
        event(SND_SEQ_EVENT_PITCHBEND, ch, value, 0);
    }
    void sendSysex(const std::vector<uint8_t>& data) override
    {
        if (!m_seq || data.empty())
            return;
        snd_seq_event_t ev;
        snd_seq_ev_clear(&ev);
        snd_seq_ev_set_source(&ev, m_port);
        snd_seq_ev_set_subs(&ev);
        snd_seq_ev_set_direct(&ev);
        snd_seq_ev_set_sysex(&ev, data.size(), const_cast<uint8_t*>(data.data()));
        snd_seq_event_output_direct(m_seq, &ev);
    }

private:
    void event(snd_seq_event_type_t type, int ch, int a, int b)
    {
        if (!m_seq)
            return;
        snd_seq_event_t ev;
        snd_seq_ev_clear(&ev);
        ev.type = type;
        ev.data.control.channel = ch & 0x0f;
        switch (type) {
        case SND_SEQ_EVENT_NOTEON:
        case SND_SEQ_EVENT_NOTEOFF:
        case SND_SEQ_EVENT_KEYPRESS:
            ev.data.note.channel = ch & 0x0f;
            ev.data.note.note = clampMidi(a);
            ev.data.note.velocity = clampMidi(b);
            break;
        case SND_SEQ_EVENT_CONTROLLER:
            ev.data.control.param = a;
            ev.data.control.value = clampMidi(b);
            break;
        case SND_SEQ_EVENT_PGMCHANGE:
        case SND_SEQ_EVENT_CHANPRESS:
            ev.data.control.value = clampMidi(a);
            break;
        case SND_SEQ_EVENT_PITCHBEND:
            ev.data.control.value = a;
            break;
        default:
            break;
        }
        snd_seq_ev_set_source(&ev, m_port);
        snd_seq_ev_set_subs(&ev);
        snd_seq_ev_set_direct(&ev);
        snd_seq_event_output_direct(m_seq, &ev);
    }

    snd_seq_t* m_seq{};
    int m_port{-1};
    snd_seq_addr_t m_dest{};
    bool m_subscribed{};
    std::string m_current;
    std::string m_error;
};

class FluidOutput : public MidiOutput {
public:
    explicit FluidOutput(std::string sf)
        : m_requestedSf(std::move(sf))
    {
    }
    ~FluidOutput() override { close(); }
    std::string backendName() const override { return "FluidSynth"; }
    std::string lastError() const override { return m_error; }
    void setSoundFont(const std::string& p) { m_requestedSf = p; }

    std::vector<MidiPort> ports(bool) const override
    {
        std::string sf = m_requestedSf.empty() ? findDefaultSoundFont() : m_requestedSf;
        if (sf.empty())
            return {{"fluidsynth", "FluidSynth (no soundfont found)"}};
        return {{"fluidsynth", "FluidSynth (" + std::filesystem::path(sf).filename().string() + ")"}};
    }

    bool open(const std::string&) override
    {
        close();
        m_error.clear();
        m_settings = new_fluid_settings();
        fluid_settings_setstr(m_settings, "audio.driver", "pulseaudio");
        fluid_settings_setnum(m_settings, "synth.gain", 0.6);
        m_synth = new_fluid_synth(m_settings);
        m_adriver = new_fluid_audio_driver(m_settings, m_synth);
        std::string sf = m_requestedSf.empty() ? findDefaultSoundFont() : m_requestedSf;
        if (sf.empty() || fluid_synth_sfload(m_synth, sf.c_str(), 1) == FLUID_FAILED) {
            m_error = sf.empty() ? "No SoundFont found. Install fluid-soundfont-gm or choose a .sf2 file."
                                 : "Unable to load SoundFont";
            // keep synth so MIDI is accepted silently
        }
        m_current = "fluidsynth";
        return true;
    }

    void close() override
    {
        if (m_adriver)
            delete_fluid_audio_driver(m_adriver);
        if (m_synth)
            delete_fluid_synth(m_synth);
        if (m_settings)
            delete_fluid_settings(m_settings);
        m_adriver = nullptr;
        m_synth = nullptr;
        m_settings = nullptr;
        m_current.clear();
    }

    std::string currentPort() const override { return m_current; }

    void sendNoteOn(int ch, int note, int vel) override
    {
        if (m_synth)
            fluid_synth_noteon(m_synth, ch, clampMidi(note), clampMidi(vel));
    }
    void sendNoteOff(int ch, int note, int) override
    {
        if (m_synth)
            fluid_synth_noteoff(m_synth, ch, clampMidi(note));
    }
    void sendKeyPressure(int ch, int note, int vel) override
    {
        if (m_synth)
            fluid_synth_key_pressure(m_synth, ch, clampMidi(note), clampMidi(vel));
    }
    void sendController(int ch, int cc, int val) override
    {
        if (m_synth)
            fluid_synth_cc(m_synth, ch, cc, clampMidi(val));
    }
    void sendProgram(int ch, int pgm) override
    {
        if (m_synth)
            fluid_synth_program_change(m_synth, ch, clampMidi(pgm));
    }
    void sendChannelPressure(int ch, int val) override
    {
        if (m_synth)
            fluid_synth_channel_pressure(m_synth, ch, clampMidi(val));
    }
    void sendPitchBend(int ch, int value) override
    {
        if (m_synth)
            fluid_synth_pitch_bend(m_synth, ch, clampMidiValue(value + 8192, 0, 16383));
    }
    void sendSysex(const std::vector<uint8_t>& data) override
    {
        if (m_synth && !data.empty())
            fluid_synth_sysex(m_synth, reinterpret_cast<const char*>(data.data()),
                              static_cast<int>(data.size()), nullptr, nullptr, nullptr, 0);
    }

private:
    static int clampMidiValue(int v, int lo, int hi)
    {
        return std::max(lo, std::min(hi, v));
    }
    fluid_settings_t* m_settings{};
    fluid_synth_t* m_synth{};
    fluid_audio_driver_t* m_adriver{};
    std::string m_requestedSf;
    std::string m_current;
    std::string m_error;
};

} // namespace

std::vector<std::string> defaultSoundFontPaths()
{
    return {
        "/usr/share/sounds/sf2/FluidR3_GM.sf2",
        "/usr/share/soundfonts/FluidR3_GM.sf2",
        "/usr/share/sounds/sf2/default-GM.sf2",
        "/usr/share/soundfonts/default.sf2",
        "/usr/share/sounds/sf3/default.sf3",
        "/usr/share/sounds/sf2/TimGM6mb.sf2",
        "/usr/share/sounds/sf2/FluidR3_GS.sf2",
    };
}

std::string findDefaultSoundFont()
{
    for (auto& p : defaultSoundFontPaths()) {
        std::error_code ec;
        if (std::filesystem::exists(p, ec))
            return p;
    }
    return {};
}

MidiOutputManager::MidiOutputManager()
{
    m_backends.push_back(std::make_unique<AlsaOutput>());
    m_backends.push_back(std::make_unique<FluidOutput>(findDefaultSoundFont()));
    m_backends.push_back(std::make_unique<DummyOutput>());
}

MidiOutputManager::~MidiOutputManager() = default;

std::vector<std::string> MidiOutputManager::backendNames() const
{
    std::vector<std::string> n;
    for (auto& b : m_backends)
        n.push_back(b->backendName());
    return n;
}

MidiOutput* MidiOutputManager::find(const std::string& backend) const
{
    for (auto& b : m_backends) {
        if (b->backendName() == backend)
            return b.get();
    }
    return m_backends.empty() ? nullptr : m_backends.front().get();
}

bool MidiOutputManager::select(const std::string& backend, const std::string& portId)
{
    auto* out = find(backend);
    if (!out)
        return false;
    if (m_current && m_current != out)
        m_current->close();
    m_current = out;
    return out->open(portId);
}

void MidiOutputManager::setSoundFont(const std::string& path)
{
    for (auto& b : m_backends) {
        if (b->backendName() == "FluidSynth") {
            static_cast<FluidOutput*>(b.get())->setSoundFont(path);
        }
    }
}

} // namespace dmidi
