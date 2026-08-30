/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <utility>
#include <vector>

namespace dmidi {

struct MidiPort {
    std::string id;
    std::string label;
};

class MidiOutput {
public:
    virtual ~MidiOutput() = default;
    virtual std::string backendName() const = 0;
    virtual std::vector<MidiPort> ports(bool advanced) const = 0;
    virtual bool open(const std::string& portId) = 0;
    virtual void close() = 0;
    virtual std::string currentPort() const = 0;
    virtual std::string lastError() const { return {}; }

    virtual void sendNoteOn(int ch, int note, int vel) = 0;
    virtual void sendNoteOff(int ch, int note, int vel) = 0;
    virtual void sendKeyPressure(int ch, int note, int vel) = 0;
    virtual void sendController(int ch, int cc, int val) = 0;
    virtual void sendProgram(int ch, int pgm) = 0;
    virtual void sendChannelPressure(int ch, int val) = 0;
    virtual void sendPitchBend(int ch, int value) = 0; // -8192..8191
    virtual void sendSysex(const std::vector<uint8_t>& data) = 0;
};

class MidiOutputManager {
public:
    MidiOutputManager();
    ~MidiOutputManager();

    std::vector<std::string> backendNames() const;
    MidiOutput* find(const std::string& backend) const;
    MidiOutput* current() const { return m_current; }
    bool select(const std::string& backend, const std::string& portId);
    void setSoundFont(const std::string& path);

private:
    std::vector<std::unique_ptr<MidiOutput>> m_backends;
    MidiOutput* m_current{};
};

std::vector<std::string> defaultSoundFontPaths();
std::string findDefaultSoundFont();

} // namespace dmidi
