//! WRK decoder — port of `Sequence::loadWrk` from `src/midi/sequence.cpp`
//! (PLAN T4, arch §1.3).
//!
//! 1:1 rules: `CAKEWALK` header; chunk loop over TRACK/NTRACK/STREAM/
//! NSTREAM/LYRICS/SGMNT note arrays (explicit durations → NoteOn+NoteOff
//! pairs), TIMEBASE, TEMPO/NTEMPO, METER/METERKEY, COMMENTS,
//! TRKNAME/TRKPATCH/TRKVOL/TRKBANK, SYSEX/SYSEX2/NSYSEX (banked,
//! `autosend` → immediate vs `saved_sysex` reference from `0xF0`
//! note-array events), VARIABLE (Title/Author/Copyright/…), MARKERS.
//! Per-track channel/pitch/velocity offsets apply to all events. All
//! leniency and every loading-error string are kept identical to the C++.
//!
//! Two deliberate hardenings over undefined C++ behavior (both documented
//! at the site): out-of-range channel indices are masked to 0–15 for the
//! channel-array access (the event keeps its raw channel), and the
//! METER tick math runs in `i64`.
//!
//! Dependency rule (arch §5): `std` + detector/codec crates only — no
//! iced/libcosmic in `core`.

use crate::core::event::{Kind, TextType, clamp_midi, make_note_off, make_note_on, make_tempo};
use crate::core::sequence::{Sequence, TrackMapRec};
use crate::core::smf::{Reader, four_den};

const TRACK_CHUNK: u8 = 0x01;
const STREAM_CHUNK: u8 = 0x02;
// Named for the chunk-table record; handled by the skip-everything arm.
#[allow(dead_code)]
const VARS_CHUNK: u8 = 0x03;
const TEMPO_CHUNK: u8 = 0x04;
const METER_CHUNK: u8 = 0x05;
const SYSEX_CHUNK: u8 = 0x06;
const COMMENTS_CHUNK: u8 = 0x08;
const TIMEBASE_CHUNK: u8 = 0x0A;
const TRKPATCH_CHUNK: u8 = 0x0E;
const NTEMPO_CHUNK: u8 = 0x0F;
const LYRICS_CHUNK: u8 = 0x12;
const TRKVOL_CHUNK: u8 = 0x13;
const SYSEX2_CHUNK: u8 = 0x14;
const MARKERS_CHUNK: u8 = 0x15;
const METERKEY_CHUNK: u8 = 0x17;
const TRKNAME_CHUNK: u8 = 0x18;
const VARIABLE_CHUNK: u8 = 0x1A;
const TRKBANK_CHUNK: u8 = 0x1E;
const NTRACK_CHUNK: u8 = 0x24;
const NSYSEX_CHUNK: u8 = 0x2C;
const NSTREAM_CHUNK: u8 = 0x2D;
const SGMNT_CHUNK: u8 = 0x31;
const END_CHUNK: u8 = 0xFF;

/// Channel-array index for a WRK channel value. Valid files only carry
/// 0–15 (TRACK masks `& 0x0f`, event nibbles are 4-bit); a corrupt NTRACK
/// record can smuggle larger `int8` values past the `> -1` override check,
/// where C++ indexes the 16-entry arrays out of bounds (undefined). Mask
/// like TRACK does; the event itself keeps the raw channel, as in C++.
fn chan_idx(channel: i32) -> usize {
    (channel & 0x0f) as usize
}

/// Track offsets for a 0-based WRK track (the C++ `recFor` closure):
/// the 1-based map entry, or a default (no override) record.
fn rec_for(seq: &Sequence, track0: i32) -> TrackMapRec {
    seq.track_map
        .get(&(track0 + 1))
        .copied()
        .unwrap_or_default()
}

/// Channel-voice event with the track's channel override applied (the C++
/// `wrkChanEvent` closure). The caller's `kind`/`a`/`b` survive; channel,
/// tick and tag are assigned here.
fn wrk_chan_event(
    seq: &mut Sequence,
    track0: i32,
    time: i64,
    chan: i32,
    kind: Kind,
    a: i32,
    b: i32,
) {
    let rec = rec_for(seq, track0);
    let channel = if rec.channel > -1 { rec.channel } else { chan };
    let idx = chan_idx(channel);
    seq.channel_used[idx] = true;
    seq.channel_events[idx] += 1;
    seq.events.push(crate::core::event::MidiEvent {
        kind,
        tick: time,
        tag: track0 + 1,
        channel,
        a,
        b,
        ..Default::default()
    });
    if time > seq.ticks_duration {
        seq.ticks_duration = time;
    }
}

/// Note with explicit duration → NoteOn + NoteOff pair (the C++ `wrkNote`
/// closure). Note-range tracking uses the raw `pitch`, while the sounded
/// key/velocity add the track offsets and clamp — exactly as in C++.
fn wrk_note(seq: &mut Sequence, track0: i32, time: i64, chan: i32, pitch: i32, vol: i32, dur: i64) {
    let rec = rec_for(seq, track0);
    let key = clamp_midi(pitch + rec.pitch);
    let vel = clamp_midi(vol + rec.velocity);
    let channel = if rec.channel > -1 { rec.channel } else { chan };
    if pitch < seq.lowest_note {
        seq.lowest_note = pitch;
    }
    if pitch > seq.highest_note {
        seq.highest_note = pitch;
    }
    let idx = chan_idx(channel);
    seq.channel_used[idx] = true;
    let mut on = make_note_on(channel, key, vel);
    on.tick = time;
    on.tag = track0 + 1;
    seq.events.push(on);
    let mut off = make_note_off(channel, key, vel);
    off.tick = time + dur;
    off.tag = track0 + 1;
    seq.events.push(off);
    seq.channel_events[idx] += 2;
    if time + dur > seq.ticks_duration {
        seq.ticks_duration = time + dur;
    }
}

/// Text record plus the TrackName/InstrumentName channel-label rule (the
/// C++ `wrkMeta` closure). Like C++, the map entry is created even when
/// `add_meta_data` ignores empty payloads.
fn wrk_meta(seq: &mut Sequence, track1: i32, time: i64, text_type: TextType, data: &[u8]) {
    seq.add_meta_data(time, track1, text_type as i32, data);
    if matches!(text_type, TextType::TrackName | TextType::InstrumentName) {
        let channel = seq.track_map.entry(track1).or_default().channel;
        if channel > -1 {
            let name = seq.trk_name.get(&track1).cloned().unwrap_or_default();
            seq.channel_label[chan_idx(channel)] = name;
        }
    }
}

/// Length-prefixed (`u8` length) name blob (the C++ `readName` closure).
fn read_name(r: &mut Reader<'_>) -> Vec<u8> {
    let n = r.u8() as usize;
    r.bytes(n)
}

/// Event array behind LYRICS/NSTREAM/SGMNT (the C++ `processNoteArray`
/// closure). Status bytes ≥ 0x90 are channel events (0x90 notes carry an
/// explicit duration; 0xF0 dereferences the SysEx bank); status 5/7 are
/// Cue texts, 6 is skipped, anything else is a Lyric.
fn process_note_array(seq: &mut Sequence, r: &mut Reader<'_>, track0: i32, events: i32) {
    for _ in 0..events {
        if r.eof() {
            break;
        }
        let time = i64::from(r.le24());
        let status = r.u8();
        if status >= 0x90 {
            let typ = status & 0xF0;
            let channel = i32::from(status & 0x0F);
            let d1 = r.u8();
            let mut d2 = 0u8;
            let mut dur = 0u16;
            if typ == 0x90 || typ == 0xA0 || typ == 0xB0 || typ == 0xE0 {
                d2 = r.u8();
            }
            if typ == 0x90 {
                dur = r.le16();
            }
            match typ {
                0x90 => wrk_note(
                    seq,
                    track0,
                    time,
                    channel,
                    i32::from(d1),
                    i32::from(d2),
                    i64::from(dur),
                ),
                0xA0 => wrk_chan_event(
                    seq,
                    track0,
                    time,
                    channel,
                    Kind::KeyPressure,
                    i32::from(d1),
                    i32::from(d2),
                ),
                0xB0 => wrk_chan_event(
                    seq,
                    track0,
                    time,
                    channel,
                    Kind::ControlChange,
                    i32::from(d1),
                    i32::from(d2),
                ),
                0xC0 => wrk_chan_event(
                    seq,
                    track0,
                    time,
                    channel,
                    Kind::ProgramChange,
                    i32::from(d1),
                    0,
                ),
                0xD0 => wrk_chan_event(
                    seq,
                    track0,
                    time,
                    channel,
                    Kind::ChannelPressure,
                    i32::from(d1),
                    0,
                ),
                0xE0 => wrk_chan_event(
                    seq,
                    track0,
                    time,
                    channel,
                    Kind::PitchBend,
                    (i32::from(d2) << 7) + i32::from(d1) - 8192,
                    0,
                ),
                0xF0 => {
                    if let Some(saved) = seq.saved_sysex.get(&i32::from(d1)).cloned() {
                        let mut ev = saved;
                        ev.tick = time;
                        ev.tag = track0 + 1;
                        seq.events.push(ev);
                    }
                }
                _ => {}
            }
        } else if status == 5 {
            r.le16();
            let len = r.le32() as usize;
            let data = r.bytes(len);
            wrk_meta(seq, track0 + 1, time, TextType::Cue, &data);
        } else if status == 6 {
            r.le16();
            r.le16();
            r.skip(4);
        } else if status == 7 {
            let len = r.le32() as usize;
            let name = r.bytes(len);
            r.skip(13);
            wrk_meta(seq, track0 + 1, time, TextType::Cue, &name);
        } else {
            let len = r.le16() as usize;
            let data = r.bytes(len);
            wrk_meta(seq, track0 + 1, time, TextType::Lyric, &data);
        }
    }
}

/// Fixed-stride note stream behind STREAM (the C++ `processStream`
/// closure): every record is time + status + d1 + d2 + duration.
fn process_stream(seq: &mut Sequence, r: &mut Reader<'_>) {
    let track = i32::from(r.le16());
    let events = i32::from(r.le16());
    let mut last = 0i64;
    for _ in 0..events {
        if r.eof() {
            break;
        }
        let time = i64::from(r.le24());
        let status = r.u8();
        let d1 = r.u8();
        let d2 = r.u8();
        let dur = r.le16();
        let typ = status & 0xF0;
        let channel = i32::from(status & 0x0F);
        last = time + i64::from(dur);
        match typ {
            0x90 => wrk_note(
                seq,
                track,
                time,
                channel,
                i32::from(d1),
                i32::from(d2),
                i64::from(dur),
            ),
            0xA0 => wrk_chan_event(
                seq,
                track,
                time,
                channel,
                Kind::KeyPressure,
                i32::from(d1),
                i32::from(d2),
            ),
            0xB0 => wrk_chan_event(
                seq,
                track,
                time,
                channel,
                Kind::ControlChange,
                i32::from(d1),
                i32::from(d2),
            ),
            0xC0 => wrk_chan_event(
                seq,
                track,
                time,
                channel,
                Kind::ProgramChange,
                i32::from(d1),
                0,
            ),
            0xD0 => wrk_chan_event(
                seq,
                track,
                time,
                channel,
                Kind::ChannelPressure,
                i32::from(d1),
                0,
            ),
            0xE0 => wrk_chan_event(
                seq,
                track,
                time,
                channel,
                Kind::PitchBend,
                (i32::from(d2) << 7) + i32::from(d1) - 8192,
                0,
            ),
            _ => {}
        }
    }
    if last > seq.ticks_duration {
        seq.ticks_duration = last;
    }
}

/// Parse WRK bytes into `seq` (C++ `Sequence::loadWrk`, 1:1). Returns true
/// when any playable events were stored — text-only songs fail, unlike the
/// SMF decoder, exactly as in C++.
pub(crate) fn load_wrk(seq: &mut Sequence, data: &[u8]) -> bool {
    if data.len() < 12 || &data[0..8] != b"CAKEWALK" {
        seq.loading_errors.push("Invalid WRK header".to_string());
        return false;
    }
    let mut r = Reader::new(data);
    r.skip(8);
    r.u8(); // gap
    let vme = r.u8();
    let vma = r.u8();
    seq.file_format = format!("WRK file version v{vma}.{vme}");
    seq.division = 120;
    seq.beat_length = 120;
    seq.beat_max = 4;
    seq.num_tracks = 0;

    while !r.eof() {
        let ck = r.u8();
        if ck == END_CHUNK {
            break;
        }
        let ck_len = r.le32() as usize;
        if ck_len > r.remaining() {
            seq.loading_errors.push("Corrupted WRK chunk".to_string());
            break;
        }
        let mut chunk = r.split(ck_len);
        match ck {
            TRACK_CHUNK => {
                let trackno = i32::from(chunk.le16());
                let mut n1 = read_name(&mut chunk);
                let n2 = read_name(&mut chunk);
                let channel = i32::from(chunk.u8() & 0x0f);
                let pitch = i32::from(chunk.u8() as i8);
                let velocity = i32::from(chunk.u8() as i8);
                chunk.u8(); // port
                chunk.u8(); // flags
                n1.extend_from_slice(&n2);
                seq.track_map.insert(
                    trackno + 1,
                    TrackMapRec {
                        channel,
                        pitch,
                        velocity,
                    },
                );
                if trackno + 1 > seq.num_tracks {
                    seq.num_tracks = trackno + 1;
                }
                if !n1.is_empty() {
                    wrk_meta(seq, trackno + 1, 0, TextType::TrackName, &n1);
                }
            }
            NTRACK_CHUNK => {
                let track = i32::from(chunk.le16());
                let name = read_name(&mut chunk);
                let bank = i32::from(chunk.le16() as i16);
                let patch = i32::from(chunk.le16() as i16);
                chunk.le16();
                chunk.le16();
                let key = i32::from(chunk.u8() as i8);
                let vel = i32::from(chunk.u8() as i8);
                chunk.skip(7);
                chunk.u8();
                let channel = i32::from(chunk.u8() as i8);
                chunk.u8();
                seq.track_map.insert(
                    track + 1,
                    TrackMapRec {
                        channel,
                        pitch: key,
                        velocity: vel,
                    },
                );
                if track + 1 > seq.num_tracks {
                    seq.num_tracks = track + 1;
                }
                if !name.is_empty() {
                    wrk_meta(seq, track + 1, 0, TextType::TrackName, &name);
                }
                let ch = if channel < 0 { 0 } else { channel };
                if bank > -1 {
                    wrk_chan_event(
                        seq,
                        track,
                        0,
                        ch,
                        Kind::ControlChange,
                        crate::core::event::CC_BANK_MSB,
                        bank / 0x80,
                    );
                    wrk_chan_event(
                        seq,
                        track,
                        0,
                        ch,
                        Kind::ControlChange,
                        crate::core::event::CC_BANK_LSB,
                        bank % 0x80,
                    );
                }
                if patch > -1 {
                    wrk_chan_event(seq, track, 0, ch, Kind::ProgramChange, patch, 0);
                }
            }
            STREAM_CHUNK => process_stream(seq, &mut chunk),
            LYRICS_CHUNK => {
                let track = i32::from(chunk.le16());
                let events = chunk.le32() as i32;
                process_note_array(seq, &mut chunk, track, events);
            }
            NSTREAM_CHUNK => {
                let track = i32::from(chunk.le16());
                let name = read_name(&mut chunk);
                if !name.is_empty() {
                    wrk_meta(seq, track + 1, 0, TextType::Marker, &name);
                }
                let events = chunk.le32() as i32;
                process_note_array(seq, &mut chunk, track, events);
            }
            SGMNT_CHUNK => {
                let track = i32::from(chunk.le16());
                let offset = chunk.le32() as i32;
                chunk.skip(8);
                let name = read_name(&mut chunk);
                chunk.skip(20);
                if !name.is_empty() {
                    wrk_meta(seq, track + 1, i64::from(offset), TextType::Marker, &name);
                }
                let events = chunk.le32() as i32;
                process_note_array(seq, &mut chunk, track, events);
            }
            TIMEBASE_CHUNK => {
                seq.division = i32::from(chunk.le16());
                seq.beat_length = i64::from(seq.division);
            }
            TEMPO_CHUNK | NTEMPO_CHUNK => {
                let factor = if ck == TEMPO_CHUNK { 100i64 } else { 1i64 };
                let count = chunk.le16();
                for _ in 0..count {
                    if chunk.eof() {
                        break;
                    }
                    let time = i64::from(chunk.le32());
                    chunk.skip(4);
                    let tempo = i64::from(chunk.le16()) * factor;
                    chunk.skip(8);
                    let mut bpm = tempo as f64 / 100.0;
                    if bpm <= 0.0 {
                        bpm = 120.0;
                    }
                    let mut ev = make_tempo(6e7 / bpm);
                    ev.tick = time;
                    let tempo_val = ev.tempo;
                    seq.events.push(ev);
                    if time == 0 {
                        seq.update_tempo(tempo_val);
                    }
                    if seq.num_tempo_changes == 0 {
                        seq.initial_tempo = tempo_val;
                    }
                    seq.num_tempo_changes += 1;
                }
            }
            METER_CHUNK => {
                let count = chunk.le16();
                for _ in 0..count {
                    if chunk.eof() {
                        break;
                    }
                    chunk.skip(4);
                    let measure = i32::from(chunk.le16());
                    let num = i32::from(chunk.u8());
                    let den = four_den(i32::from(chunk.u8()));
                    chunk.skip(4);
                    let mut ts_time = 0i64;
                    if let Some(last) = seq.bars.last() {
                        ts_time = last.time
                            + last.num as i64 * 4 * seq.division as i64 / last.den as i64
                                * (i64::from(measure) - i64::from(last.bar));
                    }
                    seq.bars.push(crate::core::sequence::TimeSigRec {
                        bar: measure,
                        num,
                        den,
                        time: ts_time,
                    });
                    seq.events.push(crate::core::event::MidiEvent {
                        kind: Kind::TimeSignature,
                        tick: ts_time,
                        tag: measure,
                        a: num,
                        b: den,
                        ..Default::default()
                    });
                    seq.beat_max = num;
                    seq.beat_length = seq.division as i64 * 4 / 1.max(den) as i64;
                }
            }
            METERKEY_CHUNK => {
                let count = chunk.le16();
                for _ in 0..count {
                    if chunk.eof() {
                        break;
                    }
                    let measure = i32::from(chunk.le16());
                    let num = i32::from(chunk.u8());
                    let den = four_den(i32::from(chunk.u8()));
                    let alt = i32::from(chunk.u8() as i8);
                    seq.events.push(crate::core::event::MidiEvent {
                        kind: Kind::TimeSignature,
                        tag: measure,
                        a: num,
                        b: den,
                        ..Default::default()
                    });
                    seq.events.push(crate::core::event::MidiEvent {
                        kind: Kind::KeySignature,
                        a: alt,
                        ..Default::default()
                    });
                    seq.beat_max = num;
                    seq.beat_length = seq.division as i64 * 4 / 1.max(den) as i64;
                }
            }
            COMMENTS_CHUNK => {
                let len = usize::from(chunk.le16());
                let data = chunk.bytes(len);
                wrk_meta(seq, 1, 0, TextType::Text, &data);
            }
            TRKNAME_CHUNK => {
                let track = i32::from(chunk.le16());
                let name = read_name(&mut chunk);
                wrk_meta(seq, track + 1, 0, TextType::TrackName, &name);
            }
            TRKPATCH_CHUNK => {
                let track = i32::from(chunk.le16());
                let patch = i32::from(chunk.u8() as i8);
                let rec = rec_for(seq, track);
                let ch = if rec.channel > -1 { rec.channel } else { 0 };
                wrk_chan_event(seq, track, 0, ch, Kind::ProgramChange, patch, 0);
            }
            TRKVOL_CHUNK => {
                let track = i32::from(chunk.le16());
                let vol = i32::from(chunk.le16());
                let rec = rec_for(seq, track);
                let ch = if rec.channel > -1 { rec.channel } else { 0 };
                let b = if vol < 128 { vol } else { vol / 0x80 };
                wrk_chan_event(
                    seq,
                    track,
                    0,
                    ch,
                    Kind::ControlChange,
                    crate::core::event::CC_VOLUME,
                    b,
                );
            }
            TRKBANK_CHUNK => {
                let track = i32::from(chunk.le16());
                let bank = i32::from(chunk.le16());
                let rec = rec_for(seq, track);
                let ch = if rec.channel > -1 { rec.channel } else { 0 };
                wrk_chan_event(
                    seq,
                    track,
                    0,
                    ch,
                    Kind::ControlChange,
                    crate::core::event::CC_BANK_MSB,
                    bank / 0x80,
                );
                wrk_chan_event(
                    seq,
                    track,
                    0,
                    ch,
                    Kind::ControlChange,
                    crate::core::event::CC_BANK_LSB,
                    bank % 0x80,
                );
            }
            SYSEX_CHUNK => {
                let bank = i32::from(chunk.u8());
                let length = usize::from(chunk.le16());
                let autosend = chunk.u8() != 0;
                read_name(&mut chunk);
                let data = chunk.bytes(length);
                let ev = crate::core::event::MidiEvent {
                    kind: Kind::SysEx,
                    data,
                    ..Default::default()
                };
                if autosend {
                    seq.events.push(ev);
                } else {
                    seq.saved_sysex.insert(bank, ev);
                }
            }
            SYSEX2_CHUNK => {
                let bank = i32::from(chunk.le16());
                let length = chunk.le32() as usize;
                chunk.u8();
                read_name(&mut chunk);
                let data = chunk.bytes(length);
                seq.saved_sysex.insert(
                    bank,
                    crate::core::event::MidiEvent {
                        kind: Kind::SysEx,
                        data,
                        ..Default::default()
                    },
                );
            }
            NSYSEX_CHUNK => {
                let bank = i32::from(chunk.le16());
                let length = chunk.le32() as usize;
                chunk.le16();
                let autosend = chunk.u8() != 0;
                read_name(&mut chunk);
                let data = chunk.bytes(length);
                let ev = crate::core::event::MidiEvent {
                    kind: Kind::SysEx,
                    data,
                    ..Default::default()
                };
                if autosend {
                    seq.events.push(ev);
                } else {
                    seq.saved_sysex.insert(bank, ev);
                }
            }
            VARIABLE_CHUNK => {
                // NOTE — C++ quirk, preserved 1:1: the pad loop below tests
                // `name.len()`, which is loop-invariant, so for any name
                // shorter than 31 bytes it consumes the whole rest of the
                // chunk *including the value data*. `data` is therefore
                // always empty here and `add_meta_data` ignores it: C++
                // VARIABLE chunks effectively never record (oracle-verified:
                // a Title/Author-only file yields `texts=0`). The author
                // meant to pad the bytes consumed to 31; "fixing" that
                // would diverge from the oracle, so the quirk stays.
                let mut name = Vec::new();
                loop {
                    if chunk.eof() {
                        break;
                    }
                    let c = chunk.u8();
                    if c == 0 {
                        break;
                    }
                    name.push(c);
                    if name.len() >= 31 {
                        break;
                    }
                }
                while name.len() < 31 && !chunk.eof() {
                    chunk.u8();
                }
                let data = chunk.bytes(chunk.remaining());
                let mut text_type = TextType::Text;
                if name.as_slice() == b"Title" || name.as_slice() == b"Subtitle" {
                    text_type = TextType::TrackName;
                } else if name.as_slice() == b"Copyright" || name.as_slice() == b"Author" {
                    text_type = TextType::Copyright;
                }
                if name.as_slice() == b"Title"
                    || name.as_slice() == b"Author"
                    || name.as_slice() == b"Copyright"
                    || name.as_slice() == b"Subtitle"
                    || name.as_slice() == b"Instructions"
                    || name.as_slice() == b"Keywords"
                {
                    wrk_meta(seq, 1, 0, text_type, &data);
                }
            }
            MARKERS_CHUNK => {
                let count = chunk.le16();
                for _ in 0..count {
                    if chunk.eof() {
                        break;
                    }
                    let time = i64::from(chunk.le32());
                    chunk.le16();
                    let name = read_name(&mut chunk);
                    if !name.is_empty() {
                        wrk_meta(seq, 1, time, TextType::Marker, &name);
                    }
                }
            }
            // VARS_CHUNK and unknown chunks are skipped, as in C++.
            _ => {}
        }
    }
    !seq.events.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::TextType;

    fn chunk(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn name_blob(s: &str) -> Vec<u8> {
        let mut out = vec![s.len() as u8];
        out.extend_from_slice(s.as_bytes());
        out
    }

    fn t24(t: u32) -> [u8; 3] {
        [t as u8, (t >> 8) as u8, (t >> 16) as u8]
    }

    /// Byte-identical twin of the oracle's `/tmp/t4_mini.wrk`: every chunk
    /// arm in one file — TIMEBASE, TRACK, NTRACK (bank+patch), STREAM (3
    /// records), TEMPO ×2, NTEMPO, METER ×2, METERKEY, COMMENTS (UTF-8 ü),
    /// TRKNAME/TRKPATCH/TRKVOL/TRKBANK, SYSEX autosend + SYSEX2/NSYSEX
    /// banked, VARIABLE Title/Author/Bogus (record nothing — the C++
    /// pad-loop quirk, oracle-verified), MARKERS (one empty, skipped),
    /// LYRICS (11 note-array events incl. channel override, all voice
    /// kinds, and a `0xF0` SysEx-bank reference), NSTREAM, SGMNT, VARS and
    /// an unknown chunk (skipped), END.
    fn mini_wrk() -> Vec<u8> {
        let mut out = b"CAKEWALK".to_vec();
        out.extend_from_slice(&[0x00, 0x02, 0x01]); // gap, vme, vma
        out.extend_from_slice(&chunk(TIMEBASE_CHUNK, &480u16.to_le_bytes()));
        let mut track = 0u16.to_le_bytes().to_vec();
        track.extend_from_slice(&name_blob("Pia"));
        track.extend_from_slice(&name_blob("no"));
        track.extend_from_slice(&[0x00, 0x02, 0xFD, 0x00, 0x00]); // ch,pitch,vel,port,flags
        out.extend_from_slice(&chunk(TRACK_CHUNK, &track));
        let mut ntrack = 1u16.to_le_bytes().to_vec();
        ntrack.extend_from_slice(&name_blob("Bass"));
        ntrack.extend_from_slice(&129i16.to_le_bytes());
        ntrack.extend_from_slice(&33i16.to_le_bytes());
        ntrack.extend_from_slice(&0u16.to_le_bytes());
        ntrack.extend_from_slice(&0u16.to_le_bytes());
        ntrack.extend_from_slice(&[0x01, 0x00]); // key, vel
        ntrack.extend_from_slice(&[0; 7]);
        ntrack.extend_from_slice(&[0x00, 0x01, 0x00]); // pad, ch, pad
        out.extend_from_slice(&chunk(NTRACK_CHUNK, &ntrack));
        let mut stream = vec![];
        stream.extend_from_slice(&0u16.to_le_bytes());
        stream.extend_from_slice(&3u16.to_le_bytes());
        stream.extend_from_slice(&t24(0));
        stream.extend_from_slice(&[0x90, 60, 100]);
        stream.extend_from_slice(&240u16.to_le_bytes());
        stream.extend_from_slice(&t24(240));
        stream.extend_from_slice(&[0xB0, 7, 90]);
        stream.extend_from_slice(&0u16.to_le_bytes());
        stream.extend_from_slice(&t24(480));
        stream.extend_from_slice(&[0xC0, 5, 0]);
        stream.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&chunk(STREAM_CHUNK, &stream));
        let mut tempo = 2u16.to_le_bytes().to_vec();
        tempo.extend_from_slice(&0u32.to_le_bytes());
        tempo.extend_from_slice(&[0; 4]);
        tempo.extend_from_slice(&120u16.to_le_bytes());
        tempo.extend_from_slice(&[0; 8]);
        tempo.extend_from_slice(&960u32.to_le_bytes());
        tempo.extend_from_slice(&[0; 4]);
        tempo.extend_from_slice(&60u16.to_le_bytes());
        tempo.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&chunk(TEMPO_CHUNK, &tempo));
        let mut ntempo = 1u16.to_le_bytes().to_vec();
        ntempo.extend_from_slice(&1920u32.to_le_bytes());
        ntempo.extend_from_slice(&[0; 4]);
        ntempo.extend_from_slice(&15000u16.to_le_bytes());
        ntempo.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&chunk(NTEMPO_CHUNK, &ntempo));
        let mut meter = 2u16.to_le_bytes().to_vec();
        meter.extend_from_slice(&[0; 4]);
        meter.extend_from_slice(&1u16.to_le_bytes());
        meter.extend_from_slice(&[4, 2]);
        meter.extend_from_slice(&[0; 4]);
        meter.extend_from_slice(&[0; 4]);
        meter.extend_from_slice(&5u16.to_le_bytes());
        meter.extend_from_slice(&[3, 2]);
        meter.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&chunk(METER_CHUNK, &meter));
        let mut meterkey = 1u16.to_le_bytes().to_vec();
        meterkey.extend_from_slice(&9u16.to_le_bytes());
        meterkey.extend_from_slice(&[6, 3, 0xFE]); // 6/8, alt -2
        out.extend_from_slice(&chunk(METERKEY_CHUNK, &meterkey));
        let cmt = "WRK comments ü".as_bytes();
        let mut comments = (cmt.len() as u16).to_le_bytes().to_vec();
        comments.extend_from_slice(cmt);
        out.extend_from_slice(&chunk(COMMENTS_CHUNK, &comments));
        let mut trkname = 0u16.to_le_bytes().to_vec();
        trkname.extend_from_slice(&name_blob("Lead"));
        out.extend_from_slice(&chunk(TRKNAME_CHUNK, &trkname));
        let mut trkpatch = 1u16.to_le_bytes().to_vec();
        trkpatch.push(40);
        out.extend_from_slice(&chunk(TRKPATCH_CHUNK, &trkpatch));
        let mut trkvol = 0u16.to_le_bytes().to_vec();
        trkvol.extend_from_slice(&200u16.to_le_bytes());
        out.extend_from_slice(&chunk(TRKVOL_CHUNK, &trkvol));
        let mut trkbank = 0u16.to_le_bytes().to_vec();
        trkbank.extend_from_slice(&0x0203u16.to_le_bytes());
        out.extend_from_slice(&chunk(TRKBANK_CHUNK, &trkbank));
        let mut sysex = vec![0x00]; // bank
        sysex.extend_from_slice(&6u16.to_le_bytes());
        sysex.push(0x01); // autosend
        sysex.extend_from_slice(&name_blob("gm"));
        sysex.extend_from_slice(&[0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7]);
        out.extend_from_slice(&chunk(SYSEX_CHUNK, &sysex));
        let mut sysex2 = 1u16.to_le_bytes().to_vec(); // bank
        sysex2.extend_from_slice(&5u32.to_le_bytes());
        sysex2.push(0x00);
        sysex2.extend_from_slice(&name_blob("x"));
        sysex2.extend_from_slice(&[0xF0, 0x41, 0x10, 0x42, 0xF7]);
        out.extend_from_slice(&chunk(SYSEX2_CHUNK, &sysex2));
        let mut nsysex = 2u16.to_le_bytes().to_vec(); // bank
        nsysex.extend_from_slice(&4u32.to_le_bytes());
        nsysex.extend_from_slice(&0u16.to_le_bytes());
        nsysex.push(0x00); // stored, not sent
        nsysex.extend_from_slice(&name_blob("y"));
        nsysex.extend_from_slice(&[0xF0, 0x43, 0x12, 0xF7]);
        out.extend_from_slice(&chunk(NSYSEX_CHUNK, &nsysex));
        fn variable(name: &str, data: &[u8]) -> Vec<u8> {
            let mut payload = name.as_bytes().to_vec();
            payload.push(0);
            payload.resize(31, 0);
            payload.extend_from_slice(data);
            payload
        }
        out.extend_from_slice(&chunk(
            VARIABLE_CHUNK,
            &variable("Title", b"Mini WRK Title"),
        ));
        out.extend_from_slice(&chunk(VARIABLE_CHUNK, &variable("Author", b"Gosh")));
        out.extend_from_slice(&chunk(VARIABLE_CHUNK, &variable("Bogus", b"ignored")));
        let mut markers = 2u16.to_le_bytes().to_vec();
        markers.extend_from_slice(&480u32.to_le_bytes());
        markers.extend_from_slice(&0u16.to_le_bytes());
        markers.extend_from_slice(&name_blob("Verse"));
        markers.extend_from_slice(&960u32.to_le_bytes());
        markers.extend_from_slice(&0u16.to_le_bytes());
        markers.extend_from_slice(&name_blob(""));
        out.extend_from_slice(&chunk(MARKERS_CHUNK, &markers));
        let mut lyr = 0u16.to_le_bytes().to_vec();
        lyr.extend_from_slice(&11u32.to_le_bytes());
        lyr.extend_from_slice(&t24(0));
        lyr.extend_from_slice(&[0x01]);
        lyr.extend_from_slice(&2u16.to_le_bytes());
        lyr.extend_from_slice(b"la");
        lyr.extend_from_slice(&t24(120));
        lyr.extend_from_slice(&[0x05]);
        lyr.extend_from_slice(&0u16.to_le_bytes());
        lyr.extend_from_slice(&3u32.to_le_bytes());
        lyr.extend_from_slice(b"cue");
        lyr.extend_from_slice(&t24(240));
        lyr.extend_from_slice(&[0x06]);
        lyr.extend_from_slice(&0u16.to_le_bytes());
        lyr.extend_from_slice(&0u16.to_le_bytes());
        lyr.extend_from_slice(&[0; 4]);
        lyr.extend_from_slice(&t24(360));
        lyr.extend_from_slice(&[0x07]);
        lyr.extend_from_slice(&4u32.to_le_bytes());
        lyr.extend_from_slice(b"cue2");
        lyr.extend_from_slice(&[0; 13]);
        lyr.extend_from_slice(&t24(480));
        lyr.extend_from_slice(&[0x91, 64, 80]);
        lyr.extend_from_slice(&120u16.to_le_bytes());
        lyr.extend_from_slice(&t24(600));
        lyr.extend_from_slice(&[0xA2, 65, 70]);
        lyr.extend_from_slice(&t24(720));
        lyr.extend_from_slice(&[0xB0, 10, 20]);
        lyr.extend_from_slice(&t24(840));
        lyr.extend_from_slice(&[0xF0, 0x01]);
        lyr.extend_from_slice(&t24(960));
        lyr.extend_from_slice(&[0xC3, 0x07]);
        lyr.extend_from_slice(&t24(1080));
        lyr.extend_from_slice(&[0xD0, 0x09]);
        lyr.extend_from_slice(&t24(1200));
        lyr.extend_from_slice(&[0xE0, 0x00, 0x40]);
        out.extend_from_slice(&chunk(LYRICS_CHUNK, &lyr));
        let mut nstream = 1u16.to_le_bytes().to_vec();
        nstream.extend_from_slice(&name_blob("Part"));
        nstream.extend_from_slice(&1u32.to_le_bytes());
        nstream.extend_from_slice(&t24(0));
        nstream.extend_from_slice(&[0x92, 70, 90]);
        nstream.extend_from_slice(&60u16.to_le_bytes());
        out.extend_from_slice(&chunk(NSTREAM_CHUNK, &nstream));
        let mut sgmnt = 1u16.to_le_bytes().to_vec();
        sgmnt.extend_from_slice(&240u32.to_le_bytes());
        sgmnt.extend_from_slice(&[0; 8]);
        sgmnt.extend_from_slice(&name_blob("Seg"));
        sgmnt.extend_from_slice(&[0; 20]);
        sgmnt.extend_from_slice(&1u32.to_le_bytes());
        sgmnt.extend_from_slice(&t24(240));
        sgmnt.push(0x02);
        sgmnt.extend_from_slice(&2u16.to_le_bytes());
        sgmnt.extend_from_slice(b"si");
        out.extend_from_slice(&chunk(SGMNT_CHUNK, &sgmnt));
        out.extend_from_slice(&chunk(VARS_CHUNK, &[0; 4]));
        out.extend_from_slice(&chunk(0x77, &[1, 2, 3]));
        out.push(END_CHUNK);
        out
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut h = 14695981039346656037u64;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }

    fn load(bytes: &[u8]) -> Sequence {
        let mut seq = Sequence::default();
        assert!(load_wrk(&mut seq, bytes), "{:?}", seq.loading_errors);
        seq.sort_and_finalize();
        seq
    }

    #[test]
    fn mini_builder_matches_oracle_fixture_bytes() {
        // Pins the Rust builder byte-identical to the python-built file
        // the C++ oracle ran on (`/tmp/t4_build_fixtures.py` output).
        let built = mini_wrk();
        assert_eq!(built.len(), 696);
        assert_eq!(fnv1a64(&built), 0x26dd61a822ff5472);
    }

    /// Oracle: throwaway `/tmp/t4_dump --events` (NOT committed); every
    /// expected value below is a verbatim oracle row for `/tmp/t4_mini.wrk`
    /// (differential run: 67/68 dump lines identical — the single delta is
    /// the COMMENTS `ü` default decode, where uchardet's `IBM865` verdict
    /// mojibakes and chardetng's `UTF-8` decodes correctly; same detector-
    /// swap class as `test.mid`, documented in the charset goldens).
    #[test]
    fn mini_matches_oracle_event_for_event() {
        let seq = load(&mini_wrk());
        assert_eq!(seq.size(), 35);
        assert_eq!(seq.song_length_ticks(), 7680);
        assert_eq!(seq.last_bar(), 1);
        assert_eq!(seq.division(), 480);
        assert_eq!(seq.format(), 0);
        assert_eq!(seq.num_tracks(), 2);
        assert_eq!((seq.lowest_note(), seq.highest_note()), (60, 70));
        for ch in 0..16 {
            assert_eq!(seq.channel_used(ch), ch == 0 || ch == 1, "ch{ch}");
        }
        assert_eq!(seq.text_events().len(), 11);
        assert!((seq.initial_tempo() - 500_000.0).abs() < f64::EPSILON);
        assert!(!seq.simple_time_process());
        assert_eq!(seq.errors_count(), 0);
        assert_eq!(seq.duration_string(), "00:00:08.000");
        assert_eq!(seq.file_format(), "WRK file version v1.2");
        // Track names accumulate space-joined across TRACK/TRKNAME (but
        // never VARIABLE — the pad-loop quirk); labels follow the dominant
        // record channel. `track_channel` stays -1 on the WRK path: only
        // the SMF label path fills `trk_channel` (the C++ `storeTrack`
        // lambda that would do it is dead code).
        assert_eq!(seq.track_name(1), "Piano Lead");
        assert_eq!(seq.track_name(2), "Bass");
        assert_eq!(seq.track_channel(1), -1);
        assert_eq!(seq.track_channel(2), -1);
        assert_eq!(seq.channel_label(0), "Piano Lead");
        assert_eq!(seq.channel_label(1), "Bass");
        assert_eq!(seq.track_max_points(), 1);
        assert_eq!(seq.type_max_points(), 3);
        // Text records, verbatim oracle `T` rows (track, type, tick, raw).
        let texts: Vec<(i32, i32, i64, String)> = seq
            .text_events()
            .iter()
            .map(|rec| {
                (
                    rec.track,
                    rec.text_type as i32,
                    rec.tick,
                    rec.text.iter().map(|b| format!("{b:02x}")).collect(),
                )
            })
            .collect();
        assert_eq!(
            texts,
            vec![
                (1, 3, 0, "5069616e6f".to_string()),
                (2, 3, 0, "42617373".to_string()),
                (1, 1, 0, "57524b20636f6d6d656e747320c3bc".to_string()),
                (1, 3, 0, "4c656164".to_string()),
                (1, 6, 480, "5665727365".to_string()),
                (1, 5, 0, "6c61".to_string()),
                (1, 7, 120, "637565".to_string()),
                (1, 7, 360, "63756532".to_string()),
                (2, 6, 0, "50617274".to_string()),
                (2, 6, 240, "536567".to_string()),
                (2, 5, 240, "7369".to_string()),
            ]
        );
        // Full event stream, verbatim oracle `EV` rows. Spot-checks worth
        // reading: the STREAM note sounds key 62 vel 97 (track +2/-3
        // offsets); the `0x91` LYRICS note lands on channel 0 (track
        // override, not the event nibble); TRKVOL 200 becomes 1 (`/0x80`
        // arm); the `0xF0` reference replays SYSEX2 bank 1 at tick 840.
        let rows: Vec<(Kind, i64, i32, i32, i32, i32, i32, u64, String)> = seq
            .events()
            .iter()
            .map(|ev| {
                (
                    ev.kind,
                    ev.tick,
                    ev.tag,
                    ev.channel,
                    ev.a,
                    ev.b,
                    ev.c,
                    ev.tempo.to_bits(),
                    ev.data.iter().map(|b| format!("{b:02x}")).collect(),
                )
            })
            .collect();
        let t500k = 500_000f64.to_bits();
        assert_eq!(
            rows,
            vec![
                (Kind::ControlChange, 0, 2, 1, 0, 1, 0, t500k, String::new()),
                (Kind::ControlChange, 0, 2, 1, 32, 1, 0, t500k, String::new()),
                (Kind::ProgramChange, 0, 2, 1, 33, 0, 0, t500k, String::new()),
                (Kind::NoteOn, 0, 1, 0, 62, 97, 0, t500k, String::new()),
                (Kind::Tempo, 0, 0, 0, 0, 0, 0, t500k, String::new()),
                (Kind::TimeSignature, 0, 1, 0, 4, 4, 0, t500k, String::new()),
                (Kind::TimeSignature, 0, 9, 0, 6, 8, 0, t500k, String::new()),
                (Kind::KeySignature, 0, 0, 0, -2, 0, 0, t500k, String::new()),
                (
                    Kind::Text,
                    0,
                    1,
                    0,
                    1,
                    0,
                    0,
                    t500k,
                    "57524b20636f6d6d656e747320c3bc".into()
                ),
                (Kind::ProgramChange, 0, 2, 1, 40, 0, 0, t500k, String::new()),
                (Kind::ControlChange, 0, 1, 0, 7, 1, 0, t500k, String::new()),
                (Kind::ControlChange, 0, 1, 0, 0, 4, 0, t500k, String::new()),
                (Kind::ControlChange, 0, 1, 0, 32, 3, 0, t500k, String::new()),
                (Kind::SysEx, 0, 0, 0, 0, 0, 0, t500k, "f07e7f0901f7".into()),
                (Kind::Text, 0, 1, 0, 5, 0, 0, t500k, "6c61".into()),
                (Kind::NoteOn, 0, 2, 1, 71, 90, 0, t500k, String::new()),
                (Kind::Beat, 0, 0, 0, 1, 1, 4, t500k, String::new()),
                (Kind::NoteOff, 60, 2, 1, 71, 90, 0, t500k, String::new()),
                (Kind::NoteOff, 240, 1, 0, 62, 97, 0, t500k, String::new()),
                (
                    Kind::ControlChange,
                    240,
                    1,
                    0,
                    7,
                    90,
                    0,
                    t500k,
                    String::new()
                ),
                (Kind::Text, 240, 2, 0, 5, 0, 0, t500k, "7369".into()),
                (
                    Kind::ProgramChange,
                    480,
                    1,
                    0,
                    5,
                    0,
                    0,
                    t500k,
                    String::new()
                ),
                (Kind::NoteOn, 480, 1, 0, 66, 77, 0, t500k, String::new()),
                (Kind::Beat, 480, 0, 0, 1, 2, 4, t500k, String::new()),
                (Kind::NoteOff, 600, 1, 0, 66, 77, 0, t500k, String::new()),
                (
                    Kind::KeyPressure,
                    600,
                    1,
                    0,
                    65,
                    70,
                    0,
                    t500k,
                    String::new()
                ),
                (
                    Kind::ControlChange,
                    720,
                    1,
                    0,
                    10,
                    20,
                    0,
                    t500k,
                    String::new()
                ),
                (Kind::SysEx, 840, 1, 0, 0, 0, 0, t500k, "f0411042f7".into()),
                (
                    Kind::Tempo,
                    960,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1_000_000f64.to_bits(),
                    String::new()
                ),
                (
                    Kind::ProgramChange,
                    960,
                    1,
                    0,
                    7,
                    0,
                    0,
                    t500k,
                    String::new()
                ),
                (Kind::Beat, 960, 0, 0, 1, 3, 4, t500k, String::new()),
                (
                    Kind::ChannelPressure,
                    1080,
                    1,
                    0,
                    9,
                    0,
                    0,
                    t500k,
                    String::new()
                ),
                (Kind::PitchBend, 1200, 1, 0, 0, 0, 0, t500k, String::new()),
                (
                    Kind::Tempo,
                    1920,
                    0,
                    0,
                    0,
                    0,
                    0,
                    400_000f64.to_bits(),
                    String::new()
                ),
                (
                    Kind::TimeSignature,
                    7680,
                    5,
                    0,
                    3,
                    4,
                    0,
                    t500k,
                    String::new()
                ),
            ]
        );
    }

    #[test]
    fn mini_loads_through_load_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mini.wrk");
        std::fs::write(&path, mini_wrk()).unwrap();
        let mut seq = Sequence::default();
        assert!(seq.load_file(&path), "{:?}", seq.loading_errors);
        assert_eq!(seq.size(), 35);
        assert_eq!(seq.file_format(), "WRK file version v1.2");
        assert_eq!(seq.current_file(), "mini.wrk");
        // chardetng reads the COMMENTS `ü` sample as UTF-8 (uchardet says
        // `IBM865` and mojibakes — detector-swap delta, pinned here).
        assert_eq!(seq.current_charset(), "UTF-8");
        assert_eq!(seq.decode_text(b"WRK comments \xc3\xbc"), "WRK comments ü");
    }

    #[test]
    fn variable_chunks_record_nothing_like_cpp() {
        // The C++ pad-loop quirk (see the VARIABLE_CHUNK comment):
        // Title/Author values are consumed as padding, so even a
        // Title-only file yields `texts=0` — oracle-verified on the twin
        // `/tmp/t4_varonly.wrk`.
        let mut payload = b"Title\0".to_vec();
        payload.resize(31, 0);
        payload.extend_from_slice(b"Mini WRK Title");
        let file = [
            b"CAKEWALK".as_slice(),
            &[0x00, 0x02, 0x01],
            &chunk(VARIABLE_CHUNK, &payload),
            &[END_CHUNK],
        ]
        .concat();
        let mut seq = Sequence::default();
        assert!(!load_wrk(&mut seq, &file));
        assert_eq!(seq.text_events().len(), 0);
        assert_eq!(seq.track_name(1), "");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("varonly.wrk");
        std::fs::write(&path, &file).unwrap();
        assert!(!seq.load_file(&path));
        assert_eq!(seq.loading_errors(), "No playable MIDI events<br/>");
    }

    #[test]
    fn text_only_songs_follow_the_wrk_return_rule() {
        // WRK returns `!m_events.empty()` — unlike SMF, text records alone
        // do not count. Marker-only: no playable event → fail (but the
        // record is stored). Text-type-only (COMMENTS): the `Kind::Text`
        // event IS playable → loads. Both oracle-verified.
        let mut markers = 1u16.to_le_bytes().to_vec();
        markers.extend_from_slice(&480u32.to_le_bytes());
        markers.extend_from_slice(&0u16.to_le_bytes());
        markers.extend_from_slice(&name_blob("Verse"));
        let mark_only = [
            b"CAKEWALK".as_slice(),
            &[0x00, 0x02, 0x01],
            &chunk(MARKERS_CHUNK, &markers),
            &[END_CHUNK],
        ]
        .concat();
        let mut comments = 5u16.to_le_bytes().to_vec();
        comments.extend_from_slice(b"hello");
        let text_only = [
            b"CAKEWALK".as_slice(),
            &[0x00, 0x02, 0x01],
            &chunk(COMMENTS_CHUNK, &comments),
            &[END_CHUNK],
        ]
        .concat();
        let dir = tempfile::tempdir().unwrap();
        let mark_path = dir.path().join("markonly.wrk");
        std::fs::write(&mark_path, &mark_only).unwrap();
        let mut seq = Sequence::default();
        assert!(!seq.load_file(&mark_path));
        assert_eq!(seq.text_events().len(), 1);
        assert_eq!(seq.loading_errors(), "No playable MIDI events<br/>");
        let text_path = dir.path().join("textonly.wrk");
        std::fs::write(&text_path, &text_only).unwrap();
        assert!(seq.load_file(&text_path), "{:?}", seq.loading_errors);
        assert_eq!(seq.size(), 2); // Text event + the tick-0 beat
        assert_eq!(seq.text_events().len(), 1);
        assert_eq!(seq.get_text(TextType::Text), vec!["hello".to_string()]);
    }

    #[test]
    fn malformed_arms_match_oracle_errors() {
        // (name, bytes, ok, errors, filefmt) — error strings verified
        // against the C++ oracle on twin files. Note the format label is
        // set before the chunk loop, so even `Corrupted WRK chunk` keeps it.
        let header: &[u8] = b"CAKEWALK\x00\x02\x01";
        let mut corrupt = header.to_vec();
        corrupt.push(TIMEBASE_CHUNK);
        corrupt.extend_from_slice(&0xFFFFu32.to_le_bytes());
        let cases: &[(&str, Vec<u8>, bool, &str, &str)] = &[
            ("empty", vec![], false, "Invalid WRK header<br/>", ""),
            (
                "short",
                b"CAKEWALK\x00\x02".to_vec(),
                false,
                "Invalid WRK header<br/>",
                "",
            ),
            (
                "bad-head",
                [b"CAKEWALX".as_slice(), &[0; 8]].concat(),
                false,
                "Invalid WRK header<br/>",
                "",
            ),
            (
                "corrupt",
                corrupt,
                false,
                "Corrupted WRK chunk<br/>",
                "WRK file version v1.2",
            ),
            (
                "end-only",
                [header, &[END_CHUNK]].concat(),
                false,
                "",
                "WRK file version v1.2",
            ),
        ];
        for (name, bytes, ok, errors, filefmt) in cases {
            let mut seq = Sequence::default();
            assert_eq!(load_wrk(&mut seq, bytes), *ok, "{name}");
            assert!(seq.empty(), "{name}");
            assert_eq!(seq.loading_errors(), *errors, "{name}");
            assert_eq!(seq.file_format(), *filefmt, "{name}");
        }
        // Through `load_file`, the silent end-only failure gains the
        // shared "No playable MIDI events" error (oracle-verified).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wrk");
        std::fs::write(&path, &cases[4].1).unwrap();
        let mut seq = Sequence::default();
        assert!(!seq.load_file(&path));
        assert_eq!(seq.loading_errors(), "No playable MIDI events<br/>");
    }

    #[test]
    fn out_of_range_track_channel_cannot_panic() {
        // A corrupt NTRACK record can smuggle channel values past the
        // `> -1` override check where C++ indexes its 16-entry channel
        // arrays out of bounds (undefined behavior — no oracle possible).
        // Rust masks the array index and keeps the raw channel on the
        // event; the file must load without panicking.
        let mut ntrack = 0u16.to_le_bytes().to_vec();
        ntrack.extend_from_slice(&name_blob("Wild"));
        ntrack.extend_from_slice(&(-1i16).to_le_bytes());
        ntrack.extend_from_slice(&(-1i16).to_le_bytes());
        ntrack.extend_from_slice(&0u16.to_le_bytes());
        ntrack.extend_from_slice(&0u16.to_le_bytes());
        ntrack.extend_from_slice(&[0x00, 0x00]);
        ntrack.extend_from_slice(&[0; 7]);
        ntrack.extend_from_slice(&[0x00, 20, 0x00]); // channel 20
        let mut stream = vec![];
        stream.extend_from_slice(&0u16.to_le_bytes());
        stream.extend_from_slice(&1u16.to_le_bytes());
        stream.extend_from_slice(&t24(0));
        stream.extend_from_slice(&[0x90, 60, 100]);
        stream.extend_from_slice(&120u16.to_le_bytes());
        let file = [
            b"CAKEWALK".as_slice(),
            &[0x00, 0x02, 0x01],
            &chunk(NTRACK_CHUNK, &ntrack),
            &chunk(STREAM_CHUNK, &stream),
            &[END_CHUNK],
        ]
        .concat();
        let mut seq = Sequence::default();
        assert!(load_wrk(&mut seq, &file), "{:?}", seq.loading_errors);
        assert!(seq.channel_used(20 & 0x0f));
        assert!(
            seq.events()
                .iter()
                .any(|ev| ev.kind == Kind::NoteOn && ev.channel == 20)
        );
    }
}
