//! SMF decoder — port of `Sequence::loadSmfBytes` + the `Reader` cursor
//! from `src/midi/sequence.cpp` (PLAN T3, arch §1.3).
//!
//! 1:1 rules: `MThd` header (format/ntrks/division; SMPTE division
//! degrades to 96 with a loading error; `div == 0` becomes 96 silently
//! per arch A7), per-track `MTrk` scan with resync on missing magic, VLQ
//! deltas, running status, channel-prefix meta (`0x20`), tempo (`0x51`),
//! time signature (`0x58`), key signature (`0x59`), end-of-track
//! (`0x2F`), text metas `0x01–0x07` via the `add_meta_data` path (raw
//! storage; UTF-8 decode is T4), SysEx `0xF0/0xF7`, channel usage/event
//! counts/note range/dominant-channel labels. All leniency and every
//! loading-error string are kept identical to the C++.
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use crate::core::event::{Kind, MIDI_CHANNELS, MidiEvent, make_note_off, make_note_on, make_tempo};
use crate::core::sequence::{Sequence, TimeSigRec};

/// Byte cursor over a borrowed buffer (C++ anonymous-namespace `Reader`).
/// Reads past the end yield 0 / empty, exactly as in C++; multi-byte
/// integers read left-to-right (the C++ evaluation order in practice).
/// Shared with the T4 RMI/WRK decoders (`rmi.rs`/`wrk.rs`), which need the
/// same leniency plus little-endian integers.
pub(crate) struct Reader<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub(crate) fn eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub(crate) fn need(&self, n: usize) -> bool {
        self.remaining() >= n
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..]
    }

    pub(crate) fn u8(&mut self) -> u8 {
        if !self.need(1) {
            return 0;
        }
        let b = self.data[self.pos];
        self.pos += 1;
        b
    }

    pub(crate) fn be16(&mut self) -> u16 {
        let hi = self.u8();
        let lo = self.u8();
        ((hi as u16) << 8) | lo as u16
    }

    pub(crate) fn be32(&mut self) -> u32 {
        let hi = self.be16();
        let lo = self.be16();
        ((hi as u32) << 16) | lo as u32
    }

    pub(crate) fn le16(&mut self) -> u16 {
        let lo = self.u8();
        let hi = self.u8();
        ((hi as u16) << 8) | lo as u16
    }

    pub(crate) fn le24(&mut self) -> u32 {
        let b0 = self.u8() as u32;
        let b1 = self.u8() as u32;
        let b2 = self.u8() as u32;
        b0 | (b1 << 8) | (b2 << 16)
    }

    pub(crate) fn le32(&mut self) -> u32 {
        let b0 = self.u8() as u32;
        let b1 = self.u8() as u32;
        let b2 = self.u8() as u32;
        let b3 = self.u8() as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    fn vlq(&mut self) -> u32 {
        let mut v = 0u32;
        for _ in 0..4 {
            if self.eof() {
                break;
            }
            let b = self.u8();
            v = (v << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                break;
            }
        }
        v
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Vec<u8> {
        let n = n.min(self.remaining());
        let out = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        out
    }

    pub(crate) fn skip(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.data.len());
    }

    /// Carve the next `n` bytes out as a sub-reader, advancing past them
    /// (C++ `Reader chunk{r.p, r.p + ckLen}; r.p += ckLen` in `loadWrk`).
    /// Lengths clamp to the remaining input, as in C++.
    pub(crate) fn split(&mut self, n: usize) -> Reader<'a> {
        let data: &'a [u8] = self.data;
        let n = n.min(self.remaining());
        let sub = Reader::new(&data[self.pos..self.pos + n]);
        self.pos += n;
        sub
    }

    pub(crate) fn match_tag(&mut self, tag: &[u8]) -> bool {
        if !self.need(tag.len()) || &self.data[self.pos..self.pos + tag.len()] != tag {
            return false;
        }
        self.pos += tag.len();
        true
    }
}

/// Time-signature denominator exponent → power of two (C++ `fourDen`).
/// Shared with `wrk.rs` (METER/METERKEY chunks).
pub(crate) fn four_den(exp: i32) -> i32 {
    let mut den = 1;
    let mut i = 0;
    while i < exp && i < 8 {
        den *= 2;
        i += 1;
    }
    if den <= 0 { 4 } else { den }
}

/// Note event builder with range/usage bookkeeping (the C++ `note`
/// lambda in `loadSmfBytes`): velocity-0 note-on becomes note-off.
fn note_event(seq: &mut Sequence, ch: usize, pitch: i32, vel: i32, on: bool) -> MidiEvent {
    if pitch < seq.lowest_note {
        seq.lowest_note = pitch;
    }
    if pitch > seq.highest_note {
        seq.highest_note = pitch;
    }
    seq.channel_used[ch] = true;
    seq.channel_events[ch] += 1;
    if on && vel > 0 {
        make_note_on(ch as i32, pitch, vel)
    } else {
        make_note_off(ch as i32, pitch, vel)
    }
}

/// Parse SMF bytes into `seq` (C++ `Sequence::loadSmfBytes`, 1:1).
/// Returns true when any playable events or text records were stored.
pub(crate) fn load_smf_bytes(seq: &mut Sequence, data: &[u8], format_label: &str) -> bool {
    let mut r = Reader::new(data);
    if !r.match_tag(b"MThd") {
        seq.loading_errors.push("Missing MThd header".to_string());
        return false;
    }
    let hdr_len = r.be32();
    if hdr_len < 6 {
        seq.loading_errors.push("Invalid header length".to_string());
        return false;
    }
    seq.format = r.be16() as i32;
    let ntrks = r.be16() as i32;
    let div = r.be16();
    if hdr_len > 6 {
        r.skip((hdr_len - 6) as usize);
    }
    if div & 0x8000 != 0 {
        seq.loading_errors
            .push("SMPTE time division is not fully supported".to_string());
        seq.division = 96;
    } else {
        seq.division = if div == 0 { 96 } else { div as i32 };
    }
    seq.file_format = format!("{format_label} type {}", seq.format);
    seq.beat_length = seq.division as i64;
    seq.beat_max = 4;
    seq.num_tracks = 0;

    for _ in 0..ntrks {
        if r.eof() {
            break;
        }
        if !r.match_tag(b"MTrk") {
            // Skip until the next MTrk.
            let mut found = false;
            while r.remaining() >= 8 {
                if r.rest().starts_with(b"MTrk") {
                    found = true;
                    break;
                }
                r.u8();
            }
            if !found {
                break;
            }
            r.match_tag(b"MTrk");
        }
        let trk_len = r.be32() as usize;
        let trk_end = r.pos + trk_len.min(r.remaining());
        let mut tr = Reader::new(&r.data[r.pos..trk_end]);
        r.pos = trk_end;
        seq.num_tracks += 1;
        let track = seq.num_tracks;
        let mut abs_tick = 0i64;
        let mut running = 0u8;
        let mut channel_prefix = -1i32;
        // Raw label bytes: C++ keeps `std::string` verbatim (sequence.cpp
        // `loadSmfBytes`); decode happens in the `channel_label` accessor
        // (T4 hook), never here.
        let mut track_label: Vec<u8> = Vec::new();
        let mut local_ch_events = [0i32; MIDI_CHANNELS];

        while !tr.eof() {
            abs_tick += tr.vlq() as i64;
            if abs_tick > seq.ticks_duration {
                seq.ticks_duration = abs_tick;
            }
            if tr.eof() {
                break;
            }
            let mut status = tr.u8();
            if status < 0x80 {
                if running == 0 {
                    seq.loading_errors
                        .push("Invalid running status".to_string());
                    break;
                }
                tr.pos -= 1; // Data byte; reuse the running status.
                status = running;
            } else if status < 0xF0 {
                running = status;
            }

            let mut ev = MidiEvent {
                tick: abs_tick,
                tag: track,
                ..MidiEvent::default()
            };

            if status == 0xFF {
                let typ = tr.u8();
                let len = tr.vlq() as usize;
                let payload = tr.bytes(len);
                if typ == 0x2F {
                    break;
                }
                if (1..=7).contains(&typ) {
                    seq.add_meta_data(abs_tick, track, typ as i32, &payload);
                }
                if typ == 3 || typ == 4 {
                    track_label = payload.clone();
                }
                if typ == 0x51 && payload.len() >= 3 {
                    let tempo = ((payload[0] as i32) << 16)
                        | ((payload[1] as i32) << 8)
                        | payload[2] as i32;
                    ev = make_tempo(tempo as f64);
                    ev.tick = abs_tick;
                    ev.tag = track;
                    seq.events.push(ev);
                    if abs_tick == 0 {
                        seq.update_tempo(tempo as f64);
                    }
                    if seq.num_tempo_changes == 0 {
                        seq.initial_tempo = tempo as f64;
                    }
                    seq.num_tempo_changes += 1;
                } else if typ == 0x58 && payload.len() >= 2 {
                    let num = payload[0] as i32;
                    let den = four_den(payload[1] as i32);
                    ev.kind = Kind::TimeSignature;
                    ev.a = num;
                    ev.b = den;
                    ev.tag = seq.bar_count;
                    seq.events.push(ev);
                    seq.beat_max = num;
                    seq.beat_length = seq.division as i64 * 4 / 1.max(den) as i64;
                    seq.bars.push(TimeSigRec {
                        bar: seq.bar_count,
                        num,
                        den,
                        time: abs_tick,
                    });
                } else if typ == 0x59 && payload.len() >= 2 {
                    ev.kind = Kind::KeySignature;
                    ev.a = payload[0] as i8 as i32;
                    ev.c = payload[1] as i32;
                    seq.events.push(ev);
                } else if typ == 0x20 && !payload.is_empty() {
                    channel_prefix = (payload[0] & 0x0f) as i32;
                }
            } else if status == 0xF0 || status == 0xF7 {
                let len = tr.vlq() as usize;
                let payload = tr.bytes(len);
                ev.kind = Kind::SysEx;
                if status == 0xF0 {
                    ev.data.push(0xF0);
                }
                ev.data.extend_from_slice(&payload);
                seq.events.push(ev);
            } else {
                let typ = status & 0xF0;
                let mut ch = (status & 0x0F) as i32;
                if channel_prefix >= 0 {
                    ch = channel_prefix;
                }
                let (d1, d2) = if typ == 0xC0 || typ == 0xD0 {
                    (tr.u8(), 0)
                } else {
                    (tr.u8(), tr.u8())
                };
                let chi = ch as usize;
                seq.channel_used[chi] = true;
                local_ch_events[chi] += 1;
                seq.channel_events[chi] += 1;
                ev.channel = ch;
                match typ {
                    0x80 => {
                        ev = note_event(seq, chi, d1 as i32, d2 as i32, false);
                        ev.tick = abs_tick;
                        ev.tag = track;
                        seq.events.push(ev);
                    }
                    0x90 => {
                        ev = note_event(seq, chi, d1 as i32, d2 as i32, true);
                        ev.tick = abs_tick;
                        ev.tag = track;
                        seq.events.push(ev);
                    }
                    0xA0 => {
                        ev.kind = Kind::KeyPressure;
                        ev.a = d1 as i32;
                        ev.b = d2 as i32;
                        seq.events.push(ev);
                    }
                    0xB0 => {
                        ev.kind = Kind::ControlChange;
                        ev.a = d1 as i32;
                        ev.b = d2 as i32;
                        seq.events.push(ev);
                    }
                    0xC0 => {
                        ev.kind = Kind::ProgramChange;
                        ev.a = d1 as i32;
                        seq.events.push(ev);
                    }
                    0xD0 => {
                        ev.kind = Kind::ChannelPressure;
                        ev.a = d1 as i32;
                        seq.events.push(ev);
                    }
                    0xE0 => {
                        ev.kind = Kind::PitchBend;
                        ev.a = ((d2 as i32) << 7) + d1 as i32 - 8192;
                        seq.events.push(ev);
                    }
                    _ => {}
                }
            }
        }
        if !track_label.is_empty() {
            let mut max = 0;
            let mut chan = -1;
            for (i, &n) in local_ch_events.iter().enumerate() {
                if n > max {
                    max = n;
                    chan = i as i32;
                }
            }
            if chan >= 0 {
                seq.channel_label[chan as usize] = track_label;
                seq.trk_channel.insert(track, chan);
            }
        }
    }
    !seq.events.is_empty() || !seq.text_events.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble a format-0 SMF with the given division and raw tracks.
    fn smf_file(div: u16, tracks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"MThd".to_vec();
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
        out.extend_from_slice(&div.to_be_bytes());
        for t in tracks {
            out.extend_from_slice(b"MTrk");
            out.extend_from_slice(&(t.len() as u32).to_be_bytes());
            out.extend_from_slice(t);
        }
        out
    }

    fn end_of_track() -> Vec<u8> {
        vec![0x00, 0xFF, 0x2F, 0x00]
    }

    fn decode(data: &[u8]) -> Sequence {
        let mut seq = Sequence::default();
        let ok = load_smf_bytes(&mut seq, data, "SMF");
        assert!(ok, "decode failed: {:?}", seq.loading_errors);
        seq
    }

    #[test]
    fn vlq_decodes_up_to_four_bytes() {
        let mut r = Reader::new(&[0x00]);
        assert_eq!(r.vlq(), 0);
        let mut r = Reader::new(&[0x83, 0x60]);
        assert_eq!(r.vlq(), 480);
        assert!(r.eof());
        let mut r = Reader::new(&[0xFF, 0xFF, 0xFF, 0x7F]);
        assert_eq!(r.vlq(), 0x0FFF_FFFF);
        // A fifth continuation byte is left unconsumed (C++ 4-byte cap).
        let mut r = Reader::new(&[0x81, 0x81, 0x81, 0x81, 0x00]);
        assert_eq!(r.vlq(), 2_113_665);
        assert_eq!(r.rest(), &[0x00]);
        // Truncated VLQ yields the partial value.
        let mut r = Reader::new(&[0x81]);
        assert_eq!(r.vlq(), 1);
    }

    #[test]
    fn reader_helpers_match_cpp() {
        let mut r = Reader::new(&[0x12, 0x34, 0x56]);
        assert_eq!(r.be16(), 0x1234);
        assert_eq!(r.u8(), 0x56);
        assert_eq!(r.u8(), 0); // Past-end reads yield 0.
        let mut r = Reader::new(&[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(r.be32(), 0x0102_0304);
        let mut r = Reader::new(&[1, 2]);
        assert_eq!(r.bytes(9), vec![1, 2]); // Lengths clamp to remaining.
        let mut r = Reader::new(&[1, 2]);
        r.skip(99);
        assert!(r.eof());
        let mut r = Reader::new(b"MTrk");
        assert!(r.match_tag(b"MTrk"));
        assert!(!r.match_tag(b"MTrk"));
        let mut r = Reader::new(b"MT");
        assert!(!r.match_tag(b"MTrk"));
    }

    #[test]
    fn rejects_bad_headers() {
        for (name, data) in [
            ("empty", b"".to_vec()),
            ("truncated-magic", b"MTh".to_vec()),
            ("bad-magic", b"XXXX".to_vec()),
        ] {
            let mut seq = Sequence::default();
            assert!(!load_smf_bytes(&mut seq, &data, "SMF"), "{name}");
            assert_eq!(seq.loading_errors(), "Missing MThd header\n", "{name}");
        }
        // Header length below 6 is rejected (C++ oracle: edge_badhdrlen).
        let mut data = b"MThd".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&[0; 4]);
        let mut seq = Sequence::default();
        assert!(!load_smf_bytes(&mut seq, &data, "SMF"));
        assert_eq!(seq.loading_errors(), "Invalid header length\n");
        // ...while extra header bytes are skipped (len > 6).
        let mut data = b"MThd".to_vec();
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(&[0, 1, 0, 1]); // format 1, ntrks 1
        data.extend_from_slice(&120u16.to_be_bytes());
        data.extend_from_slice(&[0xAA, 0xBB]); // padding
        let track = vec![0x00, 0x90, 60, 100, 0x00, 0xFF, 0x2F, 0x00];
        data.extend_from_slice(b"MTrk");
        data.extend_from_slice(&(track.len() as u32).to_be_bytes());
        data.extend_from_slice(&track);
        let seq = decode(&data);
        assert_eq!(seq.format(), 1);
        assert_eq!(seq.num_tracks(), 1);
    }

    #[test]
    fn smpte_and_zero_division_become_96() {
        // SMPTE division: 96 + a loading error (C++ oracle: edge_smpte).
        let data = smf_file(0xE7_28, &[end_of_track()]);
        let mut seq = Sequence::default();
        assert!(!load_smf_bytes(&mut seq, &data, "SMF"));
        assert_eq!(seq.division(), 96);
        assert_eq!(
            seq.loading_errors(),
            "SMPTE time division is not fully supported\n"
        );
        // div == 0: silent 96, no error (arch A7).
        let data = smf_file(0, &[end_of_track()]);
        let mut seq = Sequence::default();
        assert!(!load_smf_bytes(&mut seq, &data, "SMF"));
        assert_eq!(seq.division(), 96);
        assert_eq!(seq.errors_count(), 0);
        // ...and at `load_file` level the empty song reports "No playable
        // MIDI events" (C++ oracle: edge_div0).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("div0.mid");
        std::fs::write(&path, &data).unwrap();
        let mut seq = Sequence::default();
        assert!(!seq.load_file(&path));
        assert_eq!(seq.division(), 96);
        assert_eq!(seq.loading_errors(), "No playable MIDI events\n");
    }

    #[test]
    fn running_status_reuses_last_voice_status() {
        let track = vec![
            0x00, 0x90, 60, 100, // note on ch0
            0x0A, 62, 90, // running-status note on
            0x0A, 60, 0, // running-status note on vel 0 -> note off
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let seq = decode(&smf_file(120, &[track]));
        assert_eq!(seq.events.len(), 3);
        assert_eq!(seq.events[0].kind, Kind::NoteOn);
        assert_eq!((seq.events[0].a, seq.events[0].b), (60, 100));
        assert_eq!(seq.events[1].kind, Kind::NoteOn);
        assert_eq!((seq.events[1].tick, seq.events[1].a), (10, 62));
        assert_eq!(seq.events[2].kind, Kind::NoteOff);
        assert_eq!(seq.events[2].tick, 20);
        // C++ counts note events twice (generic block + note lambda).
        assert_eq!(seq.channel_events[0], 6);
    }

    #[test]
    fn stray_data_byte_without_running_status_errors() {
        let track = vec![0x00, 0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00];
        let data = smf_file(120, &[track]);
        let mut seq = Sequence::default();
        assert!(!load_smf_bytes(&mut seq, &data, "SMF"));
        assert_eq!(seq.loading_errors(), "Invalid running status\n");
    }

    #[test]
    fn resync_finds_mtrk_after_garbage() {
        let track1 = vec![0x00, 0x90, 60, 100, 0x00, 0xFF, 0x2F, 0x00];
        let track2 = vec![0x00, 0x91, 64, 100, 0x00, 0xFF, 0x2F, 0x00];
        let mut data = b"MThd".to_vec();
        data.extend_from_slice(&6u32.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&120u16.to_be_bytes());
        data.extend_from_slice(b"MTrk");
        data.extend_from_slice(&(track1.len() as u32).to_be_bytes());
        data.extend_from_slice(&track1);
        data.extend_from_slice(b"GARB"); // Missing magic -> resync scan.
        data.extend_from_slice(b"MTrk");
        data.extend_from_slice(&(track2.len() as u32).to_be_bytes());
        data.extend_from_slice(&track2);
        let seq = decode(&data);
        assert_eq!(seq.num_tracks(), 2);
        assert_eq!(seq.events.len(), 2);
        assert_eq!(seq.events[1].tag, 2);
        assert_eq!(seq.events[1].channel, 1);
    }

    #[test]
    fn meta_events_map_to_store() {
        let track = vec![
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, // tempo 500000 @0
            0x00, 0xFF, 0x58, 0x04, 0x03, 0x02, 0x18, 0x08, // 3/4
            0x00, 0xFF, 0x59, 0x02, 0xFE, 0x00, // Bb major: -2 flats
            0x00, 0xFF, 0x20, 0x01, 0x05, // channel prefix 5
            0x00, 0x90, 60, 100, // lands on ch 5 via the prefix
            0x00, 0xF0, 0x03, 0x41, 0x10, 0xF7, // SysEx keeps F0...
            0x00, 0xF7, 0x02, 0x10, 0xF7, // ...while F7 (split) has none
            0x00, 0xFF, 0x2F, 0x00, 0x00, 0x90, 64, 100, // past end-of-track: ignored
        ];
        let seq = decode(&smf_file(120, &[track]));
        assert_eq!(seq.num_tempo_changes, 1);
        assert!((seq.initial_tempo() - 500_000.0).abs() < f64::EPSILON);
        assert!(seq.simple_time_process());
        let tempo = &seq.events[0];
        assert_eq!(tempo.kind, Kind::Tempo);
        assert!((tempo.tempo - 500_000.0).abs() < f64::EPSILON);
        let ts = &seq.events[1];
        assert_eq!((ts.kind, ts.a, ts.b), (Kind::TimeSignature, 3, 4));
        assert_eq!(ts.tag, 1); // C++ tags time-sigs with the bar count.
        assert_eq!(seq.bars.len(), 1);
        let ks = &seq.events[2];
        assert_eq!((ks.kind, ks.a, ks.c), (Kind::KeySignature, -2, 0));
        assert_eq!(seq.events[3].channel, 5);
        assert_eq!(seq.events[4].data, vec![0xF0, 0x41, 0x10, 0xF7]);
        assert_eq!(seq.events[5].data, vec![0x10, 0xF7]);
        assert_eq!(seq.events.len(), 6);
    }

    #[test]
    fn text_metas_score_store_and_label() {
        let name = b"Lead";
        let mut track = vec![0x00, 0xFF, 0x03, name.len() as u8];
        track.extend_from_slice(name);
        track.extend_from_slice(&[0x00, 0x92, 60, 100, 0x00, 0x92, 62, 100]);
        track.extend_from_slice(&[0x00, 0x90, 64, 100]); // ch0: fewer events
        let lyric = b"la";
        track.push(0x00);
        track.extend_from_slice(&[0xFF, 0x05, lyric.len() as u8]);
        track.extend_from_slice(lyric);
        track.extend_from_slice(&[0x00, 0xFF, 0x08, 0x01, 0x41]); // type 8: skipped
        track.extend_from_slice(&[0x00, 0xFF, 0x01, 0x00]); // empty: ignored
        track.extend_from_slice(&end_of_track());
        let seq = decode(&smf_file(120, &[track]));
        // Types 3 + 5 stored; 8 and the empty payload are not.
        assert_eq!(seq.text_events().len(), 2);
        assert_eq!(seq.track_max_points(), 1);
        assert_eq!(seq.type_max_points(), 3); // tie 3/5 -> lowest first
        assert_eq!(seq.track_name(1), "Lead");
        // Dominant-channel heuristic: ch2 wins 2:1 over ch0.
        assert_eq!(seq.channel_label(2), "Lead");
        assert_eq!(seq.channel_label(0), "");
        assert_eq!(seq.track_channel(1), 2);
        // The lyric also became a playable Text event.
        assert!(
            seq.events
                .iter()
                .any(|ev| ev.kind == Kind::Text && ev.a == 5)
        );
    }

    #[test]
    fn name_only_track_assigns_no_label() {
        let name = b"Empty";
        let mut track = vec![0x00, 0xFF, 0x03, name.len() as u8];
        track.extend_from_slice(name);
        track.extend_from_slice(&end_of_track());
        // Text-only tracks still count as parsed (C++ return rule).
        let data = smf_file(120, &[track]);
        let mut seq = Sequence::default();
        assert!(load_smf_bytes(&mut seq, &data, "SMF"));
        assert_eq!(seq.channel_label(0), "");
        assert_eq!(seq.track_channel(1), -1);
    }

    #[test]
    fn non_utf8_track_name_keeps_raw_bytes() {
        // Latin-1 `Caf\xe9`: C++ stores the payload bytes verbatim in
        // `std::string` (sequence.cpp `loadSmfBytes`/`addMetaData`), so
        // decoding must not happen at parse time — a U+FFFD baked in
        // early is irreversible, and T4 charset decode could never
        // recover the original byte.
        let name = b"Caf\xe9";
        let mut track = vec![0x00, 0xFF, 0x03, name.len() as u8];
        track.extend_from_slice(name);
        track.extend_from_slice(&[0x00, 0x92, 60, 100]);
        track.extend_from_slice(&end_of_track());
        let seq = decode(&smf_file(120, &[track]));
        // Raw bytes survive storage verbatim in both maps (the C++
        // differential: label bytes `Caf\xe9`, never `Caf U+FFFD`).
        assert_eq!(seq.trk_name.get(&1).map(Vec::as_slice), Some(&name[..]));
        assert_eq!(seq.channel_label[2].as_slice(), &name[..]);
        // Accessor output shape: owned String, lossy UTF-8 until T4
        // swaps in charset-aware decode at the T4 hook.
        assert_eq!(seq.track_name(1), "Caf\u{fffd}");
        assert_eq!(seq.channel_label(2), "Caf\u{fffd}");
        assert_eq!(seq.track_channel(1), 2);
    }

    #[test]
    #[cfg(unix)]
    fn non_utf8_file_name_keeps_raw_bytes() {
        use std::os::unix::ffi::OsStringExt;
        // Same class as the track-name fix: C++ keeps the file-name
        // `std::string` bytes verbatim; decode is display-only.
        let dir = tempfile::tempdir().unwrap();
        let raw = b"caf\xe9.mid".to_vec();
        let path = dir.path().join(std::ffi::OsString::from_vec(raw));
        std::fs::write(&path, mini_file()).unwrap();
        let mut seq = Sequence::default();
        assert!(seq.load_file(&path), "{:?}", seq.loading_errors);
        assert_eq!(
            seq.current_file,
            std::ffi::OsString::from_vec(b"caf\xe9.mid".to_vec())
        );
        assert_eq!(seq.current_file(), "caf\u{fffd}.mid");
        assert!(
            seq.current_full_file_name().ends_with("caf\u{fffd}.mid"),
            "full path display: {}",
            seq.current_full_file_name()
        );
    }

    #[test]
    fn truncated_track_length_clamps() {
        // Declared length far exceeds the file: parse what exists.
        let mut data = smf_file(120, &[vec![0x00, 0x90, 60, 100]]);
        let len_pos = data.len() - 4 - 4;
        data[len_pos..len_pos + 4].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        let seq = decode(&data);
        assert_eq!(seq.events.len(), 1);
        assert_eq!(seq.events[0].a, 60);
    }

    // --- Goldens vs the C++ decoder ---
    //
    // Oracle: a throwaway `g++` harness (`smf_dump`, NOT committed)
    // built against the C++ core as:
    //   g++ -std=c++17 -I src /tmp/smf_dump.cpp build/libdmidi-core.a \
    //     $(pkg-config --libs glib-2.0 uchardet) -o /tmp/smf_dump
    // and run over `examples/*.mid examples/*.kar` plus byte-crafted
    // edge files. The harness source is gone with /tmp; the durable
    // evidence is the committed `FIXTURES` table plus the golden
    // assertions below — every expected value a verbatim C++ output
    // row — together with the byte-level edge tests above. The Rust
    // side must reproduce them exactly. Independent cross-checks:
    // `test.mid` was additionally walked by hand (format 0, div 120,
    // 1 track, tempo 600000, notes 60–72 on ch 0), and all durations
    // re-derive from ticks/division/tempo by hand.

    /// Byte-identical twin of the oracle's `/tmp/edge_mini.mid`: type 0,
    /// div 120, tempo 500000 @0, note 60 on/off 480 ticks apart.
    fn mini_file() -> Vec<u8> {
        let track = vec![
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, 0x00, 0x90, 0x3C, 0x40, 0x83, 0x60, 0x80,
            0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00,
        ];
        smf_file(120, &[track])
    }

    fn channel_mask(seq: &Sequence) -> u32 {
        let mut mask = 0u32;
        for ch in 0..16 {
            if seq.channel_used(ch) {
                mask |= 1 << ch;
            }
        }
        mask
    }

    #[test]
    fn golden_minimal_file_matches_cpp() {
        // C++ row: ok=1 events=8 ticks=480 bars=2 div=120 fmt=0 ntrk=1
        // lo=60 hi=60 chmask=0x0001 texts=0 initTempo=500000 simple=1
        // errs=0 dur=00:00:02.000 filefmt='SMF type 0'
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mini.mid");
        std::fs::write(&path, mini_file()).unwrap();
        let mut seq = Sequence::default();
        assert!(seq.load_file(&path));
        assert_eq!(seq.size(), 8); // 3 parsed + 5 beats @0..480
        assert_eq!(seq.song_length_ticks(), 480);
        assert_eq!(seq.last_bar(), 2);
        assert_eq!(seq.division(), 120);
        assert_eq!(seq.format(), 0);
        assert_eq!(seq.num_tracks(), 1);
        assert_eq!((seq.lowest_note(), seq.highest_note()), (60, 60));
        assert_eq!(channel_mask(&seq), 0x0001);
        assert_eq!(seq.text_events().len(), 0);
        assert!((seq.initial_tempo() - 500_000.0).abs() < f64::EPSILON);
        assert!(seq.simple_time_process());
        assert_eq!(seq.errors_count(), 0);
        assert_eq!(seq.duration_string(), "00:00:02.000");
        assert_eq!(seq.file_format(), "SMF type 0");
        assert_eq!(seq.current_file(), "mini.mid");
    }

    struct Fixture {
        name: &'static str,
        events: usize,
        ticks: i32,
        bars: i32,
        div: i32,
        fmt: i32,
        ntrk: i32,
        lo: i32,
        hi: i32,
        chmask: u32,
        texts: usize,
        init_tempo: i64,
        simple: bool,
        dur: &'static str,
        labels: &'static [(i32, &'static str)],
    }

    // Verbatim /tmp/smf_dump rows over examples/*.mid + *.kar.
    const FIXTURES: &[Fixture] = &[
        Fixture {
            name: "haendel_hallelujah.mid",
            events: 9224,
            ticks: 184_320,
            bars: 97,
            div: 480,
            fmt: 1,
            ntrk: 11,
            lo: 38,
            hi: 86,
            chmask: 0x05ff,
            texts: 310,
            init_tempo: 550_458,
            simple: false,
            dur: "00:03:31.376",
            labels: &[
                (0, "S"),
                (1, "A"),
                (2, "T"),
                (3, "B"),
                (4, "Strings I"),
                (5, "Strings II"),
                (6, "Sctrings III"),
                (7, "Timpani"),
                (8, "Brasses"),
                (10, "BC"),
            ],
        },
        Fixture {
            name: "lindaamiga.mid",
            events: 1455,
            ticks: 48_000,
            bars: 51,
            div: 480,
            fmt: 1,
            ntrk: 5,
            lo: 48,
            hi: 72,
            chmask: 0x000f,
            texts: 146,
            init_tempo: 1_000_000,
            simple: false,
            dur: "00:01:40.000",
            labels: &[(0, "S"), (1, "A"), (2, "T"), (3, "B")],
        },
        Fixture {
            name: "mozart_aveverum.mid",
            events: 2725,
            ticks: 90_240,
            bars: 48,
            div: 480,
            fmt: 1,
            ntrk: 11,
            lo: 40,
            hi: 79,
            chmask: 0x01ff,
            texts: 83,
            init_tempo: 1_000_000,
            simple: true,
            dur: "00:03:08.000",
            labels: &[
                (0, "Violino I"),
                (1, "Violino II"),
                (2, "Viola"),
                (3, "Violoncello"),
                (4, "Soprano"),
                (5, "Alto"),
                (6, "Tenore"),
                (7, "Basso"),
                (8, "Organo II"),
            ],
        },
        Fixture {
            name: "mozart_diesirae.mid",
            events: 7491,
            ticks: 132_479,
            bars: 69,
            div: 480,
            fmt: 1,
            ntrk: 12,
            lo: 40,
            hi: 88,
            chmask: 0x1d7f,
            texts: 170,
            init_tempo: 387_096,
            simple: true,
            dur: "00:01:46.838",
            labels: &[
                (0, "soprano"),
                (1, "alto"),
                (2, "tenore"),
                (3, "basso"),
                (4, "clarinet"),
                (5, "fagotti"),
                (6, "trombe"),
                (8, "timpani"),
                (10, "violinoA"),
                (11, "violinoB"),
                (12, "viola"),
            ],
        },
        Fixture {
            name: "mozart_konigdernacht.mid",
            events: 9201,
            ticks: 191_999,
            bars: 100,
            div: 480,
            fmt: 1,
            ntrk: 13,
            lo: 29,
            hi: 93,
            chmask: 0x1dff,
            texts: 178,
            init_tempo: 428_571,
            simple: true,
            dur: "00:02:51.428",
            labels: &[
                (0, "flauti"),
                (1, "oboi"),
                (2, "fagotti"),
                (3, "cornif"),
                (4, "trombed"),
                (5, "timpani"),
                (6, "viola"),
                (7, "violino1"),
                (8, "violino2"),
                (10, "violoncello"),
                (11, "contrabasso"),
                (12, "voice"),
            ],
        },
        Fixture {
            name: "schubert_avemaria.mid",
            events: 5099,
            ticks: 82_560,
            bars: 44,
            div: 480,
            fmt: 1,
            ntrk: 5,
            lo: 27,
            hi: 80,
            chmask: 0x0007,
            texts: 226,
            init_tempo: 2_142_857,
            simple: true,
            dur: "00:06:08.571",
            labels: &[(0, "Lyrics"), (1, "right hand"), (2, "left hand")],
        },
        Fixture {
            name: "Schubert_Standchen.mid",
            events: 4743,
            ticks: 120_960,
            bars: 85,
            div: 480,
            fmt: 1,
            ntrk: 4,
            lo: 29,
            hi: 83,
            chmask: 0x0007,
            texts: 167,
            init_tempo: 857_142,
            simple: false,
            dur: "00:03:36.000",
            labels: &[(0, "Melody"), (1, "Piano1"), (2, "Piano2")],
        },
        Fixture {
            name: "test.mid",
            events: 25,
            ticks: 480,
            bars: 2,
            div: 120,
            fmt: 0,
            ntrk: 1,
            lo: 60,
            hi: 72,
            chmask: 0x0001,
            texts: 1,
            init_tempo: 600_000,
            simple: true,
            dur: "00:00:02.400",
            labels: &[],
        },
        Fixture {
            name: "Negra_Sombra.kar",
            events: 2472,
            ticks: 103_680,
            bars: 73,
            div: 480,
            fmt: 1,
            ntrk: 8,
            lo: 40,
            hi: 72,
            chmask: 0x001f,
            texts: 192,
            init_tempo: 1_200_000,
            simple: false,
            dur: "00:04:19.200",
            labels: &[(0, "S"), (1, "A"), (2, "T"), (3, "B"), (4, "Solo")],
        },
        Fixture {
            name: "twinkle.kar",
            events: 3107,
            ticks: 39_948,
            bars: 53,
            div: 192,
            fmt: 1,
            ntrk: 6,
            lo: 31,
            hi: 88,
            chmask: 0xf200,
            texts: 97,
            init_tempo: 240_000,
            simple: true,
            dur: "00:00:49.935",
            labels: &[
                (9, "Drum"),
                (12, "Chord"),
                (13, "Arpeggio"),
                (14, "Bass"),
                (15, "Solo"),
            ],
        },
    ];

    #[test]
    fn golden_fixtures_match_cpp() {
        assert_eq!(FIXTURES.len(), 10);
        for fx in FIXTURES {
            let path = std::path::Path::new("examples").join(fx.name);
            let mut seq = Sequence::default();
            assert!(
                seq.load_file(&path),
                "{}: {:?}",
                fx.name,
                seq.loading_errors
            );
            assert_eq!(seq.size(), fx.events, "{}: event count", fx.name);
            assert_eq!(seq.song_length_ticks(), fx.ticks, "{}: ticks", fx.name);
            assert_eq!(seq.last_bar(), fx.bars, "{}: bars", fx.name);
            assert_eq!(seq.division(), fx.div, "{}: division", fx.name);
            assert_eq!(seq.format(), fx.fmt, "{}: format", fx.name);
            assert_eq!(seq.num_tracks(), fx.ntrk, "{}: tracks", fx.name);
            assert_eq!(seq.lowest_note(), fx.lo, "{}: lowest", fx.name);
            assert_eq!(seq.highest_note(), fx.hi, "{}: highest", fx.name);
            assert_eq!(channel_mask(&seq), fx.chmask, "{}: chmask", fx.name);
            assert_eq!(seq.text_events().len(), fx.texts, "{}: texts", fx.name);
            assert_eq!(
                seq.initial_tempo() as i64,
                fx.init_tempo,
                "{}: tempo",
                fx.name
            );
            assert_eq!(seq.simple_time_process(), fx.simple, "{}: simple", fx.name);
            assert_eq!(seq.errors_count(), 0, "{}: errors", fx.name);
            assert_eq!(seq.duration_string(), fx.dur, "{}: duration", fx.name);
            assert_eq!(
                seq.file_format(),
                format!("SMF type {}", fx.fmt),
                "{}: filefmt",
                fx.name
            );
            assert_eq!(seq.current_file(), fx.name, "{}: basename", fx.name);
            for (ch, label) in fx.labels {
                assert_eq!(seq.channel_label(*ch), *label, "{}: ch{ch}", fx.name);
            }
            // No labels beyond the expected set.
            for ch in 0..16 {
                if !fx.labels.iter().any(|(c, _)| *c == ch) {
                    assert_eq!(seq.channel_label(ch), "", "{}: ch{ch} unlabeled", fx.name);
                }
            }
        }
    }
}
