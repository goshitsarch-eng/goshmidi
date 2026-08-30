/*
    Gosh MIDI Player — Qt6/Kirigami
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#include "sequence.hpp"

#include <QByteArray>
#include <QTextCodec>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <uchardet.h>

namespace dmidi {
namespace {

struct Reader {
    const uint8_t* p{};
    const uint8_t* end{};
    size_t origin{};

    size_t pos() const { return static_cast<size_t>(p - (end - (end - p))) ; }

    bool eof() const { return p >= end; }
    size_t remaining() const { return p < end ? static_cast<size_t>(end - p) : 0; }

    bool need(size_t n) const { return remaining() >= n; }

    uint8_t u8()
    {
        if (!need(1))
            return 0;
        return *p++;
    }
    uint16_t be16() { return static_cast<uint16_t>((u8() << 8) | u8()); }
    uint32_t be24() { return (static_cast<uint32_t>(u8()) << 16) | (u8() << 8) | u8(); }
    uint32_t be32() { return (static_cast<uint32_t>(be16()) << 16) | be16(); }
    uint16_t le16() { return static_cast<uint16_t>(u8() | (u8() << 8)); }
    uint32_t le24() { return u8() | (u8() << 8) | (static_cast<uint32_t>(u8()) << 16); }
    uint32_t le32() { return u8() | (u8() << 8) | (u8() << 16) | (static_cast<uint32_t>(u8()) << 24); }

    uint32_t vlq()
    {
        uint32_t v = 0;
        for (int i = 0; i < 4 && !eof(); ++i) {
            uint8_t b = u8();
            v = (v << 7) | (b & 0x7f);
            if ((b & 0x80) == 0)
                break;
        }
        return v;
    }

    std::vector<uint8_t> bytes(size_t n)
    {
        n = std::min(n, remaining());
        std::vector<uint8_t> out(p, p + n);
        p += n;
        return out;
    }

    std::string str(size_t n)
    {
        auto b = bytes(n);
        return std::string(b.begin(), b.end());
    }

    void skip(size_t n) { p += std::min(n, remaining()); }

    bool match(const char* s, size_t n)
    {
        if (!need(n) || std::memcmp(p, s, n) != 0)
            return false;
        p += n;
        return true;
    }
};

std::string lowerExt(const std::string& path)
{
    std::string ext = std::filesystem::path(path).extension().string();
    std::transform(ext.begin(), ext.end(), ext.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return ext;
}

std::vector<uint8_t> readAll(const std::string& path)
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
    return buf;
}

int fourDen(int exp)
{
    int den = 1;
    for (int i = 0; i < exp && i < 8; ++i)
        den *= 2;
    return den <= 0 ? 4 : den;
}

} // namespace

void Sequence::clear()
{
    m_events.clear();
    m_textEvents.clear();
    m_loadingErrors.clear();
    m_infoMap.clear();
    m_trkName.clear();
    m_trkScore.clear();
    m_typScore.clear();
    m_trkChannel.clear();
    m_trackMap.clear();
    m_savedSysex.clear();
    m_bars.clear();
    m_currentFile.clear();
    m_currentFileFull.clear();
    m_fileFormat.clear();
    m_charset = "UTF-8";
    m_lblName.clear();
    m_pos = 0;
    m_format = 0;
    m_numTracks = 0;
    m_ticksDuration = 0;
    m_division = 120;
    m_lowestNote = 127;
    m_highestNote = 0;
    m_barCount = 1;
    m_beatMax = 4;
    m_beatLength = 120;
    m_tempo = 500000.0;
    m_tempoFactor = 1.0;
    m_ticks2micros = 0;
    m_initialTempo = 500000.0;
    m_numTempoChanges = 0;
    m_charsetSample.clear();
    for (int i = 0; i < kMidiChannels; ++i) {
        m_channelUsed[i] = false;
        m_channelEvents[i] = 0;
        m_channelLabel[i].clear();
    }
}

MidiEvent* Sequence::nextEvent()
{
    if (m_pos < m_events.size())
        return &m_events[m_pos++];
    return nullptr;
}

void Sequence::setTickPosition(int64_t tick)
{
    for (size_t i = 0; i < m_events.size(); ++i) {
        if (m_events[i].tick >= tick) {
            m_pos = i;
            return;
        }
    }
    m_pos = m_events.size();
}

MidiEvent* Sequence::jumpToBar(int bar)
{
    MidiEvent* nearest = nullptr;
    for (auto& ev : m_events) {
        if (ev.kind == MidiEvent::Kind::Beat) {
            nearest = &ev;
            if (ev.a >= bar && ev.b == 1)
                break;
        }
    }
    if (nearest)
        setTickPosition(nearest->tick);
    return nearest;
}

MidiEvent* Sequence::previousBar(const MidiEvent* latest)
{
    if (!latest)
        return firstBeat();
    int prevBar = latest->a - 1;
    MidiEvent* nearest = const_cast<MidiEvent*>(latest);
    for (auto it = m_events.rbegin(); it != m_events.rend(); ++it) {
        if (it->kind == MidiEvent::Kind::Beat) {
            nearest = &(*it);
            if (nearest->a <= prevBar && nearest->b == 1)
                break;
            if (&(*it) == latest)
                continue;
        }
    }
    return nearest;
}

MidiEvent* Sequence::nextBar(const MidiEvent* latest)
{
    if (!latest)
        return firstBeat();
    int next = latest->a + 1;
    MidiEvent* nearest = const_cast<MidiEvent*>(latest);
    for (auto& ev : m_events) {
        if (ev.kind == MidiEvent::Kind::Beat) {
            nearest = &ev;
            if (ev.a >= next && ev.b == 1)
                break;
        }
    }
    return nearest;
}

MidiEvent* Sequence::nearestBeatByTicks(int64_t ticks)
{
    MidiEvent* nearest = nullptr;
    for (auto& ev : m_events) {
        if (ev.kind == MidiEvent::Kind::Beat) {
            if (ev.tick > ticks)
                break;
            nearest = &ev;
        }
    }
    return nearest;
}

MidiEvent* Sequence::firstBeat()
{
    for (auto& ev : m_events) {
        if (ev.kind == MidiEvent::Kind::Beat)
            return &ev;
    }
    return nullptr;
}

void Sequence::setTempoFactor(double factor)
{
    if (factor >= 0.1 && factor <= 10.0) {
        m_tempoFactor = factor;
        m_ticks2micros = m_tempo / (m_division * m_tempoFactor);
    }
}

void Sequence::updateTempo(double newTempo)
{
    if (m_tempo != newTempo) {
        m_tempo = newTempo;
        m_ticks2micros = m_tempo / (m_division * m_tempoFactor);
    }
}

std::chrono::microseconds Sequence::timeOfEvent(const MidiEvent& ev) const
{
    return std::chrono::microseconds(static_cast<uint64_t>(ev.tick * m_ticks2micros));
}

std::chrono::microseconds Sequence::deltaTimeOfEvent(const MidiEvent& ev) const
{
    return std::chrono::microseconds(static_cast<uint64_t>(ev.delta * m_ticks2micros));
}

std::chrono::microseconds Sequence::timeOfTicks(uint64_t ticks) const
{
    return std::chrono::microseconds(static_cast<uint64_t>(ticks * m_ticks2micros));
}

bool Sequence::channelUsed(int channel) const
{
    if (channel < 0 || channel >= kMidiChannels)
        return false;
    return m_channelUsed[channel];
}

std::string Sequence::channelLabel(int channel) const
{
    if (channel < 0 || channel >= kMidiChannels)
        return {};
    return m_channelLabel[channel];
}

int Sequence::trackMaxPoints() const
{
    int best = -1, bestv = -1;
    for (auto& [k, v] : m_trkScore) {
        if (v > bestv) {
            bestv = v;
            best = k;
        }
    }
    return best;
}

int Sequence::typeMaxPoints() const
{
    int best = -1, bestv = -1;
    for (auto& [k, v] : m_typScore) {
        if (v > bestv) {
            bestv = v;
            best = k;
        }
    }
    return best;
}

std::string Sequence::trackName(int track) const
{
    auto it = m_trkName.find(track);
    return it == m_trkName.end() ? std::string() : it->second;
}

int Sequence::trackChannel(int track) const
{
    auto it = m_trkChannel.find(track);
    return it == m_trkChannel.end() ? -1 : it->second;
}

std::string Sequence::durationString() const
{
    auto us = timeOfTicks(static_cast<uint64_t>(m_ticksDuration));
    double seconds = us.count() / 1e6;
    int msTotal = static_cast<int>(std::lround(seconds * 1000.0));
    int h = msTotal / 3600000;
    int m = (msTotal / 60000) % 60;
    int s = (msTotal / 1000) % 60;
    int ms = msTotal % 1000;
    char buf[32];
    std::snprintf(buf, sizeof(buf), "%02d:%02d:%02d.%03d", h, m, s, ms);
    return buf;
}

std::string Sequence::metadataInfo() const
{
    std::string out;
    for (auto& [k, v] : m_infoMap) {
        out += k;
        out += ": <b>";
        out += v;
        out += "</b><br/>";
    }
    return out;
}

std::string Sequence::loadingErrors() const
{
    std::string out;
    for (auto& e : m_loadingErrors) {
        out += e;
        out += "<br/>";
    }
    return out;
}

void Sequence::setCurrentCharset(const std::string& charset)
{
    if (!charset.empty())
        m_charset = charset;
}

std::string Sequence::decodeText(const std::vector<uint8_t>& data) const
{
    if (data.empty())
        return {};
    const QByteArray raw(reinterpret_cast<const char*>(data.data()),
                         static_cast<qsizetype>(data.size()));
    if (auto* codec = QTextCodec::codecForName(QByteArray::fromStdString(m_charset)))
        return codec->toUnicode(raw).toStdString();
    return std::string(data.begin(), data.end());
}

std::vector<std::string> Sequence::getText(TextType type) const
{
    std::vector<std::string> out;
    for (auto& t : m_textEvents) {
        if (t.type == type)
            out.push_back(decodeText(t.text));
    }
    return out;
}

std::vector<std::pair<int, std::vector<uint8_t>>> Sequence::getRawText(int track, TextType type) const
{
    std::vector<std::pair<int, std::vector<uint8_t>>> out;
    for (auto& t : m_textEvents) {
        if ((track <= 0 || t.track == track) && (type == TextType::None || t.type == type))
            out.emplace_back(static_cast<int>(t.tick), t.text);
    }
    return out;
}

std::vector<std::string> Sequence::extraCodecNames()
{
    // Curated shortlist — the encodings karaoke files are actually written in.
    // Anything Qt cannot provide on this system is dropped so the encoding
    // chooser never offers a codec that would fail to decode.
    static const char* const kPreferred[] = {
        "UTF-8",       "ISO-8859-1",   "WINDOWS-1252", "SHIFT_JIS",    "GBK",
        "BIG5",        "KOI8-R",       "EUC-KR",       "EUC-JP",       "ISO-8859-2",
        "ISO-8859-5",  "ISO-8859-7",   "ISO-8859-9",   "ISO-8859-15",  "WINDOWS-1250",
        "WINDOWS-1251", "WINDOWS-1253", "WINDOWS-1254", "WINDOWS-1256", "MACINTOSH",
    };
    std::vector<std::string> out;
    for (const char* name : kPreferred) {
        if (QTextCodec::codecForName(name))
            out.emplace_back(name);
    }
    if (out.empty())
        out.emplace_back("UTF-8");
    return out;
}

void Sequence::feedCharset(const std::vector<uint8_t>& data)
{
    m_charsetSample.insert(m_charsetSample.end(), data.begin(), data.end());
}

void Sequence::detectCharset()
{
    if (m_charsetSample.empty()) {
        m_charset = "UTF-8";
        return;
    }
    uchardet_t h = uchardet_new();
    uchardet_handle_data(h, reinterpret_cast<const char*>(m_charsetSample.data()), m_charsetSample.size());
    uchardet_data_end(h);
    const char* cs = uchardet_get_charset(h);
    if (cs && *cs)
        m_charset = cs;
    else
        m_charset = "UTF-8";
    uchardet_delete(h);
}

void Sequence::addMetaData(int64_t time, int track, int type, const std::vector<uint8_t>& data)
{
    if (data.empty())
        return;
    feedCharset(data);
    m_trkScore[track]++;
    m_typScore[type]++;
    TextType t = static_cast<TextType>(type);
    if (data.size() > 1 && data[0] == '@') {
        switch (data[1]) {
        case 'K':
            t = TextType::KarFileType;
            break;
        case 'V':
            t = TextType::KarVersion;
            break;
        case 'I':
            t = TextType::KarInformation;
            break;
        case 'L':
            t = TextType::KarLanguage;
            break;
        case 'T':
            t = TextType::KarTitles;
            break;
        case 'W':
            t = TextType::KarWhatever;
            break;
        default:
            break;
        }
    }
    m_textEvents.push_back(TextRec{time, track, t, data});
    if (t == TextType::Lyric || t == TextType::Text) {
        MidiEvent ev;
        ev.kind = MidiEvent::Kind::Text;
        ev.tick = time;
        ev.tag = track;
        ev.a = static_cast<int>(t);
        ev.data = data;
        m_events.push_back(std::move(ev));
    } else if (t == TextType::TrackName || t == TextType::InstrumentName) {
        std::string s(data.begin(), data.end());
        if (m_trkName[track].empty())
            m_trkName[track] = s;
        else {
            m_trkName[track] += ' ';
            m_trkName[track] += s;
        }
    }
}

void Sequence::sortAndFinalize()
{
    insertBeatsToEnd();
    std::stable_sort(m_events.begin(), m_events.end(), [](const MidiEvent& a, const MidiEvent& b) {
        return a.tick < b.tick;
    });
    int64_t last = 0;
    for (auto& ev : m_events) {
        ev.delta = ev.tick - last;
        last = ev.tick;
        if (ev.tick > m_ticksDuration)
            m_ticksDuration = ev.tick;
        if (ev.kind == MidiEvent::Kind::Beat)
            m_barCount = std::max(m_barCount, ev.a);
    }
    m_ticks2micros = m_tempo / (std::max(1, m_division) * m_tempoFactor);
}

void Sequence::insertBeatsToEnd()
{
    if (m_division <= 0)
        m_division = 120;
    int64_t beatLen = m_beatLength > 0 ? m_beatLength : m_division;
    int num = m_beatMax > 0 ? m_beatMax : 4;
    int bar = 1, beat = 1;
    int64_t t = 0;
    int64_t end = m_ticksDuration;
    std::vector<TimeSigRec> changes = m_bars;
    std::sort(changes.begin(), changes.end(), [](auto& a, auto& b) { return a.time < b.time; });
    size_t ci = 0;
    while (t <= end) {
        while (ci < changes.size() && changes[ci].time <= t) {
            num = changes[ci].num > 0 ? changes[ci].num : 4;
            int den = changes[ci].den > 0 ? changes[ci].den : 4;
            beatLen = m_division * 4 / den;
            if (beatLen <= 0)
                beatLen = m_division;
            ++ci;
        }
        MidiEvent ev = makeBeat(bar, beat, num);
        ev.tick = t;
        m_events.push_back(ev);
        t += beatLen;
        ++beat;
        if (beat > num) {
            beat = 1;
            ++bar;
        }
        if (beatLen <= 0)
            break;
    }
    m_barCount = std::max(1, bar - (beat == 1 ? 1 : 0));
    m_beatMax = num;
    m_beatLength = beatLen;
}

bool Sequence::loadFile(const std::string& fileName)
{
    clear();
    namespace fs = std::filesystem;
    std::error_code ec;
    if (!fs::exists(fileName, ec)) {
        m_loadingErrors.push_back("File not found");
        return false;
    }
    auto bytes = readAll(fileName);
    if (bytes.empty()) {
        m_loadingErrors.push_back("Unable to read file");
        return false;
    }
    std::string ext = lowerExt(fileName);
    bool ok = false;
    if (ext == ".wrk")
        ok = loadWrk(bytes);
    else if (ext == ".rmi")
        ok = loadRmi(bytes);
    else
        ok = loadSmfBytes(bytes.data(), bytes.size(), "SMF");
    if (!ok || m_events.empty()) {
        if (m_loadingErrors.empty())
            m_loadingErrors.push_back("No playable MIDI events");
        return false;
    }
    detectCharset();
    sortAndFinalize();
    m_lblName = fs::path(fileName).filename().string();
    m_currentFile = m_lblName;
    m_currentFileFull = fs::absolute(fileName).string();
    return true;
}

bool Sequence::loadSmfBytes(const uint8_t* data, size_t size, const std::string& formatLabel)
{
    Reader r{data, data + size};
    if (!r.match("MThd", 4)) {
        m_loadingErrors.push_back("Missing MThd header");
        return false;
    }
    uint32_t hdrLen = r.be32();
    if (hdrLen < 6) {
        m_loadingErrors.push_back("Invalid header length");
        return false;
    }
    m_format = r.be16();
    int ntrks = r.be16();
    uint16_t div = r.be16();
    if (hdrLen > 6)
        r.skip(hdrLen - 6);
    if (div & 0x8000) {
        m_loadingErrors.push_back("SMPTE time division is not fully supported");
        m_division = 96;
    } else {
        m_division = div == 0 ? 96 : div;
    }
    m_fileFormat = formatLabel + " type " + std::to_string(m_format);
    m_beatLength = m_division;
    m_beatMax = 4;
    m_numTracks = 0;

    auto note = [&](int ch, int pitch, int vel, bool on) {
        if (pitch < m_lowestNote)
            m_lowestNote = pitch;
        if (pitch > m_highestNote)
            m_highestNote = pitch;
        m_channelUsed[ch] = true;
        m_channelEvents[ch]++;
        MidiEvent ev = on && vel > 0 ? makeNoteOn(ch, pitch, vel) : makeNoteOff(ch, pitch, vel);
        return ev;
    };

    for (int t = 0; t < ntrks && !r.eof(); ++t) {
        if (!r.match("MTrk", 4)) {
            // skip until next MTrk
            bool found = false;
            while (r.remaining() >= 8) {
                if (std::memcmp(r.p, "MTrk", 4) == 0) {
                    found = true;
                    break;
                }
                r.u8();
            }
            if (!found)
                break;
            r.match("MTrk", 4);
        }
        uint32_t trkLen = r.be32();
        const uint8_t* trkEnd = r.p + std::min<size_t>(trkLen, r.remaining());
        Reader tr{r.p, trkEnd};
        r.p = trkEnd;
        ++m_numTracks;
        int track = m_numTracks;
        int64_t absTick = 0;
        uint8_t running = 0;
        int channelPrefix = -1;
        std::string trackLabel;
        int localChEvents[kMidiChannels]{};

        while (!tr.eof()) {
            absTick += tr.vlq();
            if (absTick > m_ticksDuration)
                m_ticksDuration = absTick;
            if (tr.eof())
                break;
            uint8_t status = tr.u8();
            if (status < 0x80) {
                if (running == 0) {
                    m_loadingErrors.push_back("Invalid running status");
                    break;
                }
                tr.p--; // data byte
                status = running;
            } else if (status < 0xF0) {
                running = status;
            }

            MidiEvent ev;
            ev.tick = absTick;
            ev.tag = track;

            if (status == 0xFF) {
                uint8_t type = tr.u8();
                uint32_t len = tr.vlq();
                auto payload = tr.bytes(len);
                if (type == 0x2F)
                    break;
                if (type >= 1 && type <= 7)
                    addMetaData(absTick, track, type, payload);
                if (type == 3 || type == 4)
                    trackLabel = std::string(payload.begin(), payload.end());
                if (type == 0x51 && payload.size() >= 3) {
                    int tempo = (payload[0] << 16) | (payload[1] << 8) | payload[2];
                    ev = makeTempo(tempo);
                    ev.tick = absTick;
                    ev.tag = track;
                    m_events.push_back(ev);
                    if (absTick == 0)
                        updateTempo(tempo);
                    if (m_numTempoChanges == 0)
                        m_initialTempo = tempo;
                    m_numTempoChanges++;
                } else if (type == 0x58 && payload.size() >= 2) {
                    int num = payload[0];
                    int den = fourDen(payload[1]);
                    ev.kind = MidiEvent::Kind::TimeSignature;
                    ev.a = num;
                    ev.b = den;
                    ev.tag = m_barCount;
                    m_events.push_back(ev);
                    m_beatMax = num;
                    m_beatLength = m_division * 4 / std::max(1, den);
                    TimeSigRec ts;
                    ts.bar = m_barCount;
                    ts.num = num;
                    ts.den = den;
                    ts.time = absTick;
                    m_bars.push_back(ts);
                } else if (type == 0x59 && payload.size() >= 2) {
                    ev.kind = MidiEvent::Kind::KeySignature;
                    ev.a = static_cast<int8_t>(payload[0]);
                    ev.c = payload[1];
                    m_events.push_back(ev);
                } else if (type == 0x20 && !payload.empty()) {
                    channelPrefix = payload[0] & 0x0f;
                }
            } else if (status == 0xF0 || status == 0xF7) {
                uint32_t len = tr.vlq();
                auto payload = tr.bytes(len);
                ev.kind = MidiEvent::Kind::SysEx;
                if (status == 0xF0) {
                    ev.data.push_back(0xF0);
                }
                ev.data.insert(ev.data.end(), payload.begin(), payload.end());
                m_events.push_back(ev);
            } else {
                int type = status & 0xF0;
                int ch = status & 0x0F;
                if (channelPrefix >= 0)
                    ch = channelPrefix;
                uint8_t d1 = 0, d2 = 0;
                if (type == 0xC0 || type == 0xD0) {
                    d1 = tr.u8();
                } else {
                    d1 = tr.u8();
                    d2 = tr.u8();
                }
                m_channelUsed[ch] = true;
                localChEvents[ch]++;
                m_channelEvents[ch]++;
                ev.channel = ch;
                switch (type) {
                case 0x80:
                    ev = note(ch, d1, d2, false);
                    ev.tick = absTick;
                    ev.tag = track;
                    m_events.push_back(ev);
                    break;
                case 0x90:
                    ev = note(ch, d1, d2, true);
                    ev.tick = absTick;
                    ev.tag = track;
                    m_events.push_back(ev);
                    break;
                case 0xA0:
                    ev.kind = MidiEvent::Kind::KeyPressure;
                    ev.a = d1;
                    ev.b = d2;
                    m_events.push_back(ev);
                    break;
                case 0xB0:
                    ev.kind = MidiEvent::Kind::ControlChange;
                    ev.a = d1;
                    ev.b = d2;
                    m_events.push_back(ev);
                    break;
                case 0xC0:
                    ev.kind = MidiEvent::Kind::ProgramChange;
                    ev.a = d1;
                    m_events.push_back(ev);
                    break;
                case 0xD0:
                    ev.kind = MidiEvent::Kind::ChannelPressure;
                    ev.a = d1;
                    m_events.push_back(ev);
                    break;
                case 0xE0: {
                    ev.kind = MidiEvent::Kind::PitchBend;
                    ev.a = (static_cast<int>(d2) << 7) + d1 - 8192;
                    m_events.push_back(ev);
                    break;
                }
                default:
                    break;
                }
            }
        }
        if (!trackLabel.empty()) {
            int max = 0, chan = -1;
            for (int i = 0; i < kMidiChannels; ++i) {
                if (localChEvents[i] > max) {
                    max = localChEvents[i];
                    chan = i;
                }
            }
            if (chan >= 0) {
                m_channelLabel[chan] = trackLabel;
                m_trkChannel[track] = chan;
            }
        }
        (void)ntrks;
    }
    return !m_events.empty() || !m_textEvents.empty();
}

bool Sequence::loadRmi(const std::vector<uint8_t>& bytes)
{
    if (bytes.size() < 12 || std::memcmp(bytes.data(), "RIFF", 4) != 0) {
        m_loadingErrors.push_back("Not a RIFF file");
        return false;
    }
    Reader r{bytes.data(), bytes.data() + bytes.size()};
    r.skip(4);
    r.le32();
    if (r.remaining() < 4 || std::memcmp(r.p, "RMID", 4) != 0) {
        m_loadingErrors.push_back("Not an RMID container");
        return false;
    }
    r.skip(4);
    const uint8_t* smf = nullptr;
    size_t smfLen = 0;
    while (r.remaining() >= 8) {
        char id[5]{};
        std::memcpy(id, r.p, 4);
        r.skip(4);
        uint32_t sz = r.le32();
        if (std::strcmp(id, "data") == 0) {
            smf = r.p;
            smfLen = std::min<size_t>(sz, r.remaining());
            r.skip(sz + (sz & 1));
        } else if (std::strcmp(id, "LIST") == 0) {
            const uint8_t* listEnd = r.p + std::min<size_t>(sz, r.remaining());
            if (r.remaining() >= 4 && std::memcmp(r.p, "INFO", 4) == 0) {
                r.skip(4);
                static const std::map<std::string, std::string> keys{
                    {"IALB", "Album"},     {"IARL", "Archival Location"}, {"IART", "Artist"},
                    {"ICMS", "Commissioned"}, {"ICMT", "Comments"}, {"ICOP", "Copyright"},
                    {"ICRD", "Creation date"}, {"IENG", "Engineer"}, {"IGNR", "Genre"},
                    {"IKEY", "Keywords"}, {"IMED", "Medium"}, {"INAM", "Name"},
                    {"IPRD", "Product"}, {"ISBJ", "Subject"}, {"ISFT", "Software"},
                    {"ISRC", "Source"}, {"ITCH", "Technician"}};
                while (r.p + 8 <= listEnd) {
                    char iid[5]{};
                    std::memcpy(iid, r.p, 4);
                    r.skip(4);
                    uint32_t isz = r.le32();
                    auto val = r.bytes(std::min<size_t>(isz, r.remaining()));
                    if (isz & 1)
                        r.u8();
                    auto it = keys.find(iid);
                    std::string key = it == keys.end() ? iid : it->second;
                    while (!val.empty() && val.back() == 0)
                        val.pop_back();
                    m_infoMap[key] = std::string(val.begin(), val.end());
                }
            }
            r.p = listEnd;
            if (sz & 1 && !r.eof())
                r.u8();
        } else {
            r.skip(sz + (sz & 1));
        }
    }
    if (!smf || smfLen < 8) {
        m_loadingErrors.push_back("RMID container has no MIDI data");
        return false;
    }
    bool ok = loadSmfBytes(smf, smfLen, "SMF");
    if (ok)
        m_fileFormat += " in RIFF RMID container";
    return ok;
}

bool Sequence::loadWrk(const std::vector<uint8_t>& bytes)
{
    constexpr uint8_t TRACK_CHUNK = 0x01;
    constexpr uint8_t STREAM_CHUNK = 0x02;
    constexpr uint8_t VARS_CHUNK = 0x03;
    constexpr uint8_t TEMPO_CHUNK = 0x04;
    constexpr uint8_t METER_CHUNK = 0x05;
    constexpr uint8_t SYSEX_CHUNK = 0x06;
    constexpr uint8_t COMMENTS_CHUNK = 0x08;
    constexpr uint8_t TIMEBASE_CHUNK = 0x0A;
    constexpr uint8_t TRKPATCH_CHUNK = 0x0E;
    constexpr uint8_t NTEMPO_CHUNK = 0x0F;
    constexpr uint8_t LYRICS_CHUNK = 0x12;
    constexpr uint8_t TRKVOL_CHUNK = 0x13;
    constexpr uint8_t SYSEX2_CHUNK = 0x14;
    constexpr uint8_t MARKERS_CHUNK = 0x15;
    constexpr uint8_t METERKEY_CHUNK = 0x17;
    constexpr uint8_t TRKNAME_CHUNK = 0x18;
    constexpr uint8_t VARIABLE_CHUNK = 0x1A;
    constexpr uint8_t TRKBANK_CHUNK = 0x1E;
    constexpr uint8_t NTRACK_CHUNK = 0x24;
    constexpr uint8_t NSYSEX_CHUNK = 0x2C;
    constexpr uint8_t NSTREAM_CHUNK = 0x2D;
    constexpr uint8_t SGMNT_CHUNK = 0x31;
    constexpr uint8_t END_CHUNK = 0xFF;

    if (bytes.size() < 12 || std::memcmp(bytes.data(), "CAKEWALK", 8) != 0) {
        m_loadingErrors.push_back("Invalid WRK header");
        return false;
    }
    Reader r{bytes.data(), bytes.data() + bytes.size()};
    r.skip(8);
    r.u8(); // gap
    int vme = r.u8();
    int vma = r.u8();
    m_fileFormat = "WRK file version v" + std::to_string(vma) + "." + std::to_string(vme);
    m_division = 120;
    m_beatLength = 120;
    m_beatMax = 4;
    m_numTracks = 0;

    auto recFor = [&](int track0) -> TrackMapRec {
        auto it = m_trackMap.find(track0 + 1);
        if (it == m_trackMap.end())
            return {};
        return it->second;
    };

    auto wrkNote = [&](int track, int64_t time, int chan, int pitch, int vol, int dur) {
        auto rec = recFor(track);
        int key = clampMidi(pitch + rec.pitch);
        int vel = clampMidi(vol + rec.velocity);
        int channel = rec.channel > -1 ? rec.channel : chan;
        m_lowestNote = std::min(m_lowestNote, pitch);
        m_highestNote = std::max(m_highestNote, pitch);
        m_channelUsed[channel] = true;
        MidiEvent on = makeNoteOn(channel, key, vel);
        on.tick = time;
        on.tag = track + 1;
        m_events.push_back(on);
        MidiEvent off = makeNoteOff(channel, key, vel);
        off.tick = time + dur;
        off.tag = track + 1;
        m_events.push_back(off);
        m_channelEvents[channel] += 2;
        m_ticksDuration = std::max(m_ticksDuration, time + dur);
    };

    auto wrkChanEvent = [&](MidiEvent ev, int track, int64_t time, int chan) {
        auto rec = recFor(track);
        int channel = rec.channel > -1 ? rec.channel : chan;
        ev.channel = channel;
        ev.tick = time;
        ev.tag = track + 1;
        m_channelUsed[channel] = true;
        m_channelEvents[channel]++;
        m_events.push_back(ev);
        m_ticksDuration = std::max(m_ticksDuration, time);
    };

    auto wrkMeta = [&](int track1, int64_t time, TextType type, const std::vector<uint8_t>& data) {
        addMetaData(time, track1, static_cast<int>(type), data);
        if (type == TextType::TrackName || type == TextType::InstrumentName) {
            auto rec = m_trackMap[track1];
            if (rec.channel > -1)
                m_channelLabel[rec.channel] = m_trkName[track1];
        }
    };

    auto processNoteArray = [&](int track, int events) {
        for (int i = 0; i < events && !r.eof(); ++i) {
            int64_t time = r.le24();
            uint8_t status = r.u8();
            if (status >= 0x90) {
                int type = status & 0xF0;
                int channel = status & 0x0F;
                uint8_t d1 = r.u8();
                uint8_t d2 = 0;
                uint16_t dur = 0;
                if (type == 0x90 || type == 0xA0 || type == 0xB0 || type == 0xE0)
                    d2 = r.u8();
                if (type == 0x90)
                    dur = r.le16();
                switch (type) {
                case 0x90:
                    wrkNote(track, time, channel, d1, d2, dur);
                    break;
                case 0xA0: {
                    MidiEvent ev;
                    ev.kind = MidiEvent::Kind::KeyPressure;
                    ev.a = d1;
                    ev.b = d2;
                    wrkChanEvent(ev, track, time, channel);
                    break;
                }
                case 0xB0: {
                    MidiEvent ev;
                    ev.kind = MidiEvent::Kind::ControlChange;
                    ev.a = d1;
                    ev.b = d2;
                    wrkChanEvent(ev, track, time, channel);
                    break;
                }
                case 0xC0: {
                    MidiEvent ev;
                    ev.kind = MidiEvent::Kind::ProgramChange;
                    ev.a = d1;
                    wrkChanEvent(ev, track, time, channel);
                    break;
                }
                case 0xD0: {
                    MidiEvent ev;
                    ev.kind = MidiEvent::Kind::ChannelPressure;
                    ev.a = d1;
                    wrkChanEvent(ev, track, time, channel);
                    break;
                }
                case 0xE0: {
                    MidiEvent ev;
                    ev.kind = MidiEvent::Kind::PitchBend;
                    ev.a = (d2 << 7) + d1 - 8192;
                    wrkChanEvent(ev, track, time, channel);
                    break;
                }
                case 0xF0: {
                    auto it = m_savedSysex.find(d1);
                    if (it != m_savedSysex.end()) {
                        MidiEvent ev = it->second;
                        ev.tick = time;
                        ev.tag = track + 1;
                        m_events.push_back(ev);
                    }
                    break;
                }
                default:
                    break;
                }
            } else if (status == 5) {
                r.le16();
                uint32_t len = r.le32();
                auto data = r.bytes(len);
                wrkMeta(track + 1, time, TextType::Cue, data);
            } else if (status == 6) {
                r.le16();
                r.le16();
                r.skip(4);
            } else if (status == 7) {
                uint32_t len = r.le32();
                auto name = r.bytes(len);
                r.skip(13);
                wrkMeta(track + 1, time, TextType::Cue, name);
            } else {
                uint16_t len = r.le16();
                auto data = r.bytes(len);
                wrkMeta(track + 1, time, TextType::Lyric, data);
            }
        }
    };

    auto processStream = [&]() {
        uint16_t track = r.le16();
        int events = r.le16();
        int64_t last = 0;
        for (int i = 0; i < events && !r.eof(); ++i) {
            int64_t time = r.le24();
            uint8_t status = r.u8();
            uint8_t d1 = r.u8();
            uint8_t d2 = r.u8();
            uint16_t dur = r.le16();
            int type = status & 0xF0;
            int channel = status & 0x0F;
            last = time + dur;
            switch (type) {
            case 0x90:
                wrkNote(track, time, channel, d1, d2, dur);
                break;
            case 0xA0: {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::KeyPressure;
                ev.a = d1;
                ev.b = d2;
                wrkChanEvent(ev, track, time, channel);
                break;
            }
            case 0xB0: {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::ControlChange;
                ev.a = d1;
                ev.b = d2;
                wrkChanEvent(ev, track, time, channel);
                break;
            }
            case 0xC0: {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::ProgramChange;
                ev.a = d1;
                wrkChanEvent(ev, track, time, channel);
                break;
            }
            case 0xD0: {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::ChannelPressure;
                ev.a = d1;
                wrkChanEvent(ev, track, time, channel);
                break;
            }
            case 0xE0: {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::PitchBend;
                ev.a = (d2 << 7) + d1 - 8192;
                wrkChanEvent(ev, track, time, channel);
                break;
            }
            default:
                break;
            }
        }
        m_ticksDuration = std::max(m_ticksDuration, last);
    };

    auto storeTrack = [&](int trackno, int channel, int pitch, int velocity, const std::vector<uint8_t>& name) {
        TrackMapRec rec;
        rec.channel = channel;
        rec.pitch = pitch;
        rec.velocity = velocity;
        int t1 = trackno + 1;
        m_trackMap[t1] = rec;
        if (t1 > m_numTracks)
            m_numTracks = t1;
        if (!name.empty())
            wrkMeta(t1, 0, TextType::TrackName, name);
        if (channel > -1 && !name.empty())
            m_channelLabel[channel & 0x0f] = std::string(name.begin(), name.end());
        m_trkChannel[t1] = channel;
    };
    (void)storeTrack;

    while (!r.eof()) {
        uint8_t ck = r.u8();
        if (ck == END_CHUNK)
            break;
        uint32_t ckLen = r.le32();
        if (ckLen > r.remaining()) {
            m_loadingErrors.push_back("Corrupted WRK chunk");
            break;
        }
        Reader chunk{r.p, r.p + ckLen};
        r.p += ckLen;
        Reader saved = r;
        r = chunk;

        auto readName = [&]() {
            uint8_t n = r.u8();
            return r.bytes(n);
        };

        switch (ck) {
        case TRACK_CHUNK: {
            int trackno = r.le16();
            auto n1 = readName();
            auto n2 = readName();
            int channel = r.u8() & 0x0f;
            int pitch = static_cast<int8_t>(r.u8());
            int velocity = static_cast<int8_t>(r.u8());
            r.u8(); // port
            r.u8(); // flags
            n1.insert(n1.end(), n2.begin(), n2.end());
            TrackMapRec rec{channel, pitch, velocity};
            m_trackMap[trackno + 1] = rec;
            if (trackno + 1 > m_numTracks)
                m_numTracks = trackno + 1;
            if (!n1.empty())
                wrkMeta(trackno + 1, 0, TextType::TrackName, n1);
            break;
        }
        case NTRACK_CHUNK: {
            int track = r.le16();
            auto name = readName();
            int bank = static_cast<int16_t>(r.le16());
            int patch = static_cast<int16_t>(r.le16());
            r.le16();
            r.le16();
            int key = static_cast<int8_t>(r.u8());
            int vel = static_cast<int8_t>(r.u8());
            r.skip(7);
            r.u8();
            int channel = static_cast<int8_t>(r.u8());
            r.u8();
            TrackMapRec rec{channel, key, vel};
            m_trackMap[track + 1] = rec;
            if (track + 1 > m_numTracks)
                m_numTracks = track + 1;
            if (!name.empty())
                wrkMeta(track + 1, 0, TextType::TrackName, name);
            if (bank > -1) {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::ControlChange;
                ev.a = kCcBankMsb;
                ev.b = bank / 0x80;
                wrkChanEvent(ev, track, 0, channel < 0 ? 0 : channel);
                ev.a = kCcBankLsb;
                ev.b = bank % 0x80;
                wrkChanEvent(ev, track, 0, channel < 0 ? 0 : channel);
            }
            if (patch > -1) {
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::ProgramChange;
                ev.a = patch;
                wrkChanEvent(ev, track, 0, channel < 0 ? 0 : channel);
            }
            break;
        }
        case STREAM_CHUNK:
            processStream();
            break;
        case LYRICS_CHUNK: {
            int track = r.le16();
            int events = static_cast<int>(r.le32());
            processNoteArray(track, events);
            break;
        }
        case NSTREAM_CHUNK: {
            int track = r.le16();
            auto name = readName();
            if (!name.empty())
                wrkMeta(track + 1, 0, TextType::Marker, name);
            int events = static_cast<int>(r.le32());
            processNoteArray(track, events);
            break;
        }
        case SGMNT_CHUNK: {
            int track = r.le16();
            int offset = static_cast<int>(r.le32());
            r.skip(8);
            auto name = readName();
            r.skip(20);
            if (!name.empty())
                wrkMeta(track + 1, offset, TextType::Marker, name);
            int events = static_cast<int>(r.le32());
            processNoteArray(track, events);
            break;
        }
        case TIMEBASE_CHUNK:
            m_division = r.le16();
            m_beatLength = m_division;
            break;
        case TEMPO_CHUNK:
        case NTEMPO_CHUNK: {
            int factor = ck == TEMPO_CHUNK ? 100 : 1;
            int count = r.le16();
            for (int i = 0; i < count && !r.eof(); ++i) {
                int64_t time = r.le32();
                r.skip(4);
                long tempo = r.le16() * factor;
                r.skip(8);
                double bpm = tempo / 100.0;
                if (bpm <= 0)
                    bpm = 120;
                MidiEvent ev = makeTempo(6e7 / bpm);
                ev.tick = time;
                m_events.push_back(ev);
                if (time == 0)
                    updateTempo(ev.tempo);
                if (m_numTempoChanges == 0)
                    m_initialTempo = ev.tempo;
                m_numTempoChanges++;
            }
            break;
        }
        case METER_CHUNK: {
            int count = r.le16();
            for (int i = 0; i < count && !r.eof(); ++i) {
                r.skip(4);
                int measure = r.le16();
                int num = r.u8();
                int den = fourDen(r.u8());
                r.skip(4);
                MidiEvent ev;
                ev.kind = MidiEvent::Kind::TimeSignature;
                ev.a = num;
                ev.b = den;
                ev.tag = measure;
                TimeSigRec ts;
                ts.bar = measure;
                ts.num = num;
                ts.den = den;
                ts.time = 0;
                if (!m_bars.empty()) {
                    auto& last = m_bars.back();
                    ts.time = last.time + (last.num * 4 * m_division / last.den * (measure - last.bar));
                }
                ev.tick = ts.time;
                m_bars.push_back(ts);
                m_events.push_back(ev);
                m_beatMax = num;
                m_beatLength = m_division * 4 / std::max(1, den);
            }
            break;
        }
        case METERKEY_CHUNK: {
            int count = r.le16();
            for (int i = 0; i < count && !r.eof(); ++i) {
                int measure = r.le16();
                int num = r.u8();
                int den = fourDen(r.u8());
                int8_t alt = static_cast<int8_t>(r.u8());
                MidiEvent ts;
                ts.kind = MidiEvent::Kind::TimeSignature;
                ts.a = num;
                ts.b = den;
                ts.tag = measure;
                m_events.push_back(ts);
                MidiEvent ks;
                ks.kind = MidiEvent::Kind::KeySignature;
                ks.a = alt;
                m_events.push_back(ks);
                m_beatMax = num;
                m_beatLength = m_division * 4 / std::max(1, den);
            }
            break;
        }
        case COMMENTS_CHUNK: {
            int len = r.le16();
            wrkMeta(1, 0, TextType::Text, r.bytes(len));
            break;
        }
        case TRKNAME_CHUNK: {
            int track = r.le16();
            wrkMeta(track + 1, 0, TextType::TrackName, readName());
            break;
        }
        case TRKPATCH_CHUNK: {
            int track = r.le16();
            int patch = static_cast<int8_t>(r.u8());
            auto rec = recFor(track);
            int ch = rec.channel > -1 ? rec.channel : 0;
            MidiEvent ev;
            ev.kind = MidiEvent::Kind::ProgramChange;
            ev.a = patch;
            wrkChanEvent(ev, track, 0, ch);
            break;
        }
        case TRKVOL_CHUNK: {
            int track = r.le16();
            int vol = r.le16();
            auto rec = recFor(track);
            int ch = rec.channel > -1 ? rec.channel : 0;
            MidiEvent ev;
            ev.kind = MidiEvent::Kind::ControlChange;
            ev.a = kCcVolume;
            ev.b = vol < 128 ? vol : vol / 0x80;
            wrkChanEvent(ev, track, 0, ch);
            break;
        }
        case TRKBANK_CHUNK: {
            int track = r.le16();
            int bank = r.le16();
            auto rec = recFor(track);
            int ch = rec.channel > -1 ? rec.channel : 0;
            MidiEvent ev;
            ev.kind = MidiEvent::Kind::ControlChange;
            ev.a = kCcBankMsb;
            ev.b = bank / 0x80;
            wrkChanEvent(ev, track, 0, ch);
            ev.a = kCcBankLsb;
            ev.b = bank % 0x80;
            wrkChanEvent(ev, track, 0, ch);
            break;
        }
        case SYSEX_CHUNK: {
            int bank = r.u8();
            int length = r.le16();
            bool autosend = r.u8() != 0;
            readName();
            auto data = r.bytes(length);
            MidiEvent ev;
            ev.kind = MidiEvent::Kind::SysEx;
            ev.data = data;
            if (autosend)
                m_events.push_back(ev);
            else
                m_savedSysex[bank] = ev;
            break;
        }
        case SYSEX2_CHUNK: {
            int bank = r.le16();
            uint32_t length = r.le32();
            r.u8();
            readName();
            auto data = r.bytes(length);
            MidiEvent ev;
            ev.kind = MidiEvent::Kind::SysEx;
            ev.data = data;
            m_savedSysex[bank] = ev;
            break;
        }
        case NSYSEX_CHUNK: {
            int bank = r.le16();
            uint32_t length = r.le32();
            r.le16();
            bool autosend = r.u8() != 0;
            readName();
            auto data = r.bytes(length);
            MidiEvent ev;
            ev.kind = MidiEvent::Kind::SysEx;
            ev.data = data;
            if (autosend)
                m_events.push_back(ev);
            else
                m_savedSysex[bank] = ev;
            break;
        }
        case VARIABLE_CHUNK: {
            std::string name;
            while (!r.eof()) {
                char c = static_cast<char>(r.u8());
                if (c == 0)
                    break;
                name.push_back(c);
                if (name.size() >= 31)
                    break;
            }
            while (name.size() < 31 && !r.eof())
                r.u8();
            auto data = r.bytes(r.remaining());
            TextType type = TextType::Text;
            if (name == "Title" || name == "Subtitle")
                type = TextType::TrackName;
            else if (name == "Copyright" || name == "Author")
                type = TextType::Copyright;
            if (name == "Title" || name == "Author" || name == "Copyright" || name == "Subtitle"
                || name == "Instructions" || name == "Keywords")
                wrkMeta(1, 0, type, data);
            break;
        }
        case MARKERS_CHUNK: {
            int count = r.le16();
            for (int i = 0; i < count && !r.eof(); ++i) {
                int64_t time = r.le32();
                r.le16();
                auto name = readName();
                if (!name.empty())
                    wrkMeta(1, time, TextType::Marker, name);
            }
            break;
        }
        case VARS_CHUNK:
        default:
            break;
        }
        r = saved;
    }
    return !m_events.empty();
}

} // namespace dmidi
