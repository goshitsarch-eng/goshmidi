//! `Sequence`: event store + timing + navigation — port of the SMF/store
//! subset of `src/midi/sequence.{hpp,cpp}` (PLAN T3, arch §1.3).
//!
//! Covers: `clear`, `loadFile` (SMF dispatch only), `sortAndFinalize`,
//! `insertBeatsToEnd`, `addMetaData` (raw storage; KAR `@`-headers), tempo
//! math (`setTempoFactor` **rejects** out-of-range factors per arch A7 —
//! no clamping), bar/beat navigation, `durationString`/`metadataInfo`/
//! `loadingErrors`. `loadRmi`/`loadWrk` and charset decode land in T4
//! (`rmi.rs`/`wrk.rs`); the T4-owned APIs are omitted here rather than
//! stubbed, except for the `load_file` dispatch arms which record a
//! pending error until T4 replaces them.
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::core::event::{Kind, MIDI_CHANNELS, MidiEvent, TextType, make_beat};

/// One stored text/meta record (C++ `TextRec`; field `type` renamed to
/// `text_type` since `type` is a Rust keyword).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRec {
    pub tick: i64,
    pub track: i32,
    pub text_type: TextType,
    /// Raw payload bytes; decoding to UTF-8 is T4 (`decodeText`).
    pub text: Vec<u8>,
}

/// Time-signature change point driving beat synthesis (C++ `TimeSigRec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimeSigRec {
    pub bar: i32,
    pub num: i32,
    pub den: i32,
    pub time: i64,
}

/// Parsed song: sorted events, tempo state, navigation cursor (C++
/// `Sequence`). Fields are `pub(crate)` so the format decoders (`smf.rs`
/// now, `rmi.rs`/`wrk.rs` in T4) can build the store field-wise exactly
/// as `sequence.cpp` does.
#[derive(Debug, Clone)]
pub struct Sequence {
    pub(crate) events: Vec<MidiEvent>,
    pub(crate) text_events: Vec<TextRec>,
    pub(crate) loading_errors: Vec<String>,
    pub(crate) info_map: BTreeMap<String, String>,
    /// Raw track-name bytes per track (C++ `m_trkName`: `std::string`
    /// keeps non-UTF8 bytes verbatim). Decoded at the `track_name`
    /// accessor so T4's charset-aware decode sees the original bytes.
    pub(crate) trk_name: BTreeMap<i32, Vec<u8>>,
    pub(crate) trk_score: BTreeMap<i32, i32>,
    pub(crate) typ_score: BTreeMap<i32, i32>,
    pub(crate) trk_channel: BTreeMap<i32, i32>,
    pub(crate) bars: Vec<TimeSigRec>,
    /// Raw file base name (C++ `m_currentFile`: `std::string` keeps
    /// non-UTF8 bytes verbatim). Decoded for display only.
    pub(crate) current_file: OsString,
    /// Raw absolute file path (C++ `m_currentFileFull`). Display only.
    pub(crate) current_file_full: OsString,
    pub(crate) file_format: String,
    pub(crate) charset: String,
    pub(crate) pos: usize,
    pub(crate) format: i32,
    pub(crate) num_tracks: i32,
    pub(crate) ticks_duration: i64,
    pub(crate) division: i32,
    pub(crate) lowest_note: i32,
    pub(crate) highest_note: i32,
    pub(crate) bar_count: i32,
    pub(crate) beat_max: i32,
    pub(crate) beat_length: i64,
    pub(crate) tempo: f64,
    pub(crate) tempo_factor: f64,
    pub(crate) ticks2micros: f64,
    pub(crate) initial_tempo: f64,
    pub(crate) num_tempo_changes: i32,
    pub(crate) channel_used: [bool; MIDI_CHANNELS],
    pub(crate) channel_events: [i32; MIDI_CHANNELS],
    /// Raw dominant-track label bytes per channel (C++ `m_channelLabel`:
    /// `std::string` verbatim). Decoded at the `channel_label` accessor.
    pub(crate) channel_label: [Vec<u8>; MIDI_CHANNELS],
    pub(crate) charset_sample: Vec<u8>,
}

impl Default for Sequence {
    /// Mirrors `Sequence::clear()`: division 120, note range 127/0, tempo
    /// 500000, beat 4/120, charset UTF-8.
    fn default() -> Self {
        Sequence {
            events: Vec::new(),
            text_events: Vec::new(),
            loading_errors: Vec::new(),
            info_map: BTreeMap::new(),
            trk_name: BTreeMap::new(),
            trk_score: BTreeMap::new(),
            typ_score: BTreeMap::new(),
            trk_channel: BTreeMap::new(),
            bars: Vec::new(),
            current_file: OsString::new(),
            current_file_full: OsString::new(),
            file_format: String::new(),
            charset: "UTF-8".to_string(),
            pos: 0,
            format: 0,
            num_tracks: 0,
            ticks_duration: 0,
            division: 120,
            lowest_note: 127,
            highest_note: 0,
            bar_count: 1,
            beat_max: 4,
            beat_length: 120,
            tempo: 500_000.0,
            tempo_factor: 1.0,
            ticks2micros: 0.0,
            initial_tempo: 500_000.0,
            num_tempo_changes: 0,
            channel_used: [false; MIDI_CHANNELS],
            channel_events: [0; MIDI_CHANNELS],
            channel_label: std::array::from_fn(|_| Vec::new()),
            charset_sample: Vec::new(),
        }
    }
}

/// Lowercase extension bytes of `path`, mirroring C++ `lowerExt`:
/// file name after the last `/`, extension from its *last* dot (empty
/// when there is no dot or the dot is the first character).
fn lower_ext(path: &Path) -> Vec<u8> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let file_name = match bytes.iter().rposition(|&b| b == b'/') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    };
    match file_name.iter().rposition(|&b| b == b'.') {
        Some(i) if i > 0 => file_name[i..].to_ascii_lowercase(),
        _ => Vec::new(),
    }
}

impl Sequence {
    /// Reset to the default empty state (C++ `Sequence::clear`).
    pub fn clear(&mut self) {
        *self = Sequence::default();
    }

    /// Load a song file (C++ `Sequence::loadFile`). T3 handles the SMF
    /// path (`.mid/.midi/.kar` and any other non-RMI/WRK extension);
    /// `.rmi`/`.wrk` record a pending error until T4 wires `rmi.rs`/`wrk.rs`.
    pub fn load_file(&mut self, file_name: &Path) -> bool {
        self.clear();
        if !file_name.exists() {
            self.loading_errors.push("File not found".to_string());
            return false;
        }
        let bytes = std::fs::read(file_name).unwrap_or_default();
        if bytes.is_empty() {
            self.loading_errors.push("Unable to read file".to_string());
            return false;
        }
        let ext = lower_ext(file_name);
        let ok = if ext == b".wrk" || ext == b".rmi" {
            // TODO(T4): route to `rmi.rs` / `wrk.rs`; the C++ decode and
            // its error strings move there verbatim.
            self.loading_errors
                .push("RMI/WRK decoding lands in T4".to_string());
            false
        } else {
            crate::core::smf::load_smf_bytes(self, &bytes, "SMF")
        };
        if !ok || self.events.is_empty() {
            if self.loading_errors.is_empty() {
                self.loading_errors
                    .push("No playable MIDI events".to_string());
            }
            return false;
        }
        self.detect_charset();
        self.sort_and_finalize();
        // Raw storage (C++ keeps `std::string` bytes verbatim); the
        // `current_file`/`current_full_file_name` accessors lossy-decode
        // for display only.
        let base: OsString = file_name
            .file_name()
            .map(OsString::from)
            .unwrap_or_default();
        self.current_file = base;
        let full = if file_name.is_absolute() {
            file_name.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(file_name))
                .unwrap_or_else(|_| file_name.to_path_buf())
        };
        self.current_file_full = full.into_os_string();
        true
    }

    /// T3 charset detection: every song keeps the default UTF-8.
    /// TODO(T4): chardetng over `charset_sample` + `decode_text` port.
    pub(crate) fn detect_charset(&mut self) {
        self.charset = "UTF-8".to_string();
    }

    /// True when no events are stored (C++ `Sequence::empty`).
    pub fn empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Rewind the playback cursor (C++ `Sequence::resetPosition`).
    pub fn reset_position(&mut self) {
        self.pos = 0;
    }

    /// True while the cursor has events left (C++ `hasMoreEvents`).
    pub fn has_more_events(&self) -> bool {
        self.pos < self.events.len()
    }

    /// Next event under the cursor, advancing it (C++ `nextEvent`;
    /// `nullptr` becomes `None`).
    pub fn next_event(&mut self) -> Option<&MidiEvent> {
        if self.pos < self.events.len() {
            self.pos += 1;
            self.events.get(self.pos - 1)
        } else {
            None
        }
    }

    /// Move the cursor to the first event at or past `tick` (C++
    /// `setTickPosition`: linear scan, end-of-song when nothing matches).
    pub fn set_tick_position(&mut self, tick: i64) {
        for (i, ev) in self.events.iter().enumerate() {
            if ev.tick >= tick {
                self.pos = i;
                return;
            }
        }
        self.pos = self.events.len();
    }

    /// Seek to bar `bar` (C++ `jumpToBar`): last beat at or past the bar
    /// when no downbeat matches; returns the event index (`nullptr` →
    /// `None`) and always repositions the cursor through
    /// `set_tick_position` when any beat exists.
    pub fn jump_to_bar(&mut self, bar: i32) -> Option<usize> {
        let mut nearest: Option<usize> = None;
        for (i, ev) in self.events.iter().enumerate() {
            if ev.kind == Kind::Beat {
                nearest = Some(i);
                if ev.a >= bar && ev.b == 1 {
                    break;
                }
            }
        }
        if let Some(n) = nearest {
            let tick = self.events[n].tick;
            self.set_tick_position(tick);
        }
        nearest
    }

    /// Beat event one bar before `latest` (C++ `previousBar`): `None`
    /// (or an out-of-range index) resolves to the first beat, like the
    /// C++ null-pointer path. Falls back to the earliest beat — and
    /// ultimately to `latest` itself — when no earlier downbeat exists.
    pub fn previous_bar(&self, latest: Option<usize>) -> Option<usize> {
        let latest = match latest {
            Some(i) if i < self.events.len() => i,
            _ => return self.first_beat(),
        };
        let prev_bar = self.events[latest].a - 1;
        let mut nearest = latest;
        for (i, ev) in self.events.iter().enumerate().rev() {
            if ev.kind == Kind::Beat {
                nearest = i;
                if ev.a <= prev_bar && ev.b == 1 {
                    break;
                }
            }
        }
        Some(nearest)
    }

    /// Beat event one bar after `latest` (C++ `nextBar`): `None` (or an
    /// out-of-range index) resolves to the first beat. Falls back to the
    /// latest beat — and ultimately to `latest` itself — when no later
    /// downbeat exists.
    pub fn next_bar(&self, latest: Option<usize>) -> Option<usize> {
        let latest = match latest {
            Some(i) if i < self.events.len() => i,
            _ => return self.first_beat(),
        };
        let next = self.events[latest].a + 1;
        let mut nearest = latest;
        for (i, ev) in self.events.iter().enumerate() {
            if ev.kind == Kind::Beat {
                nearest = i;
                if ev.a >= next && ev.b == 1 {
                    break;
                }
            }
        }
        Some(nearest)
    }

    /// Latest beat at or before `ticks` (C++ `nearestBeatByTicks`).
    pub fn nearest_beat_by_ticks(&self, ticks: i64) -> Option<usize> {
        let mut nearest = None;
        for (i, ev) in self.events.iter().enumerate() {
            if ev.kind == Kind::Beat {
                if ev.tick > ticks {
                    break;
                }
                nearest = Some(i);
            }
        }
        nearest
    }

    /// First beat event (C++ `firstBeat`).
    pub fn first_beat(&self) -> Option<usize> {
        self.events.iter().position(|ev| ev.kind == Kind::Beat)
    }

    /// Last bar number (C++ `lastBar`).
    pub fn last_bar(&self) -> i32 {
        self.bar_count
    }

    /// SMF format (C++ `format`).
    pub fn format(&self) -> i32 {
        self.format
    }

    /// Ticks per quarter note (C++ `division`).
    pub fn division(&self) -> i32 {
        self.division
    }

    /// Parsed track count (C++ `numTracks`, also `getNumTracks`).
    pub fn num_tracks(&self) -> i32 {
        self.num_tracks
    }

    /// Song length in ticks (C++ `songLengthTicks`).
    pub fn song_length_ticks(&self) -> i32 {
        self.ticks_duration as i32
    }

    /// Lowest note pitch seen (C++ `lowestNote`).
    pub fn lowest_note(&self) -> i32 {
        self.lowest_note
    }

    /// Highest note pitch seen (C++ `highestNote`).
    pub fn highest_note(&self) -> i32 {
        self.highest_note
    }

    /// Current tempo factor (C++ `tempoFactor`).
    pub fn tempo_factor(&self) -> f64 {
        self.tempo_factor
    }

    /// Set the tempo factor, **rejecting** values outside 0.1–10 (the old
    /// value is kept; no clamping — arch A7, sequence.cpp:245-251). NaN is
    /// rejected too, as in C++ where both comparisons fail.
    pub fn set_tempo_factor(&mut self, factor: f64) {
        if (0.1..=10.0).contains(&factor) {
            self.tempo_factor = factor;
            self.ticks2micros = self.tempo / (self.division as f64 * self.tempo_factor);
        }
    }

    /// Current tempo in µs per quarter, scaled by the factor (C++
    /// `currentTempo`).
    pub fn current_tempo(&self) -> f64 {
        self.tempo / self.tempo_factor
    }

    /// First tempo event value (C++ `initialTempo`).
    pub fn initial_tempo(&self) -> f64 {
        self.initial_tempo
    }

    /// Adopt a new tempo event value (C++ `updateTempo`).
    pub fn update_tempo(&mut self, new_tempo: f64) {
        if self.tempo != new_tempo {
            self.tempo = new_tempo;
            self.ticks2micros = self.tempo / (self.division as f64 * self.tempo_factor);
        }
    }

    /// True when at most one tempo change exists, allowing absolute
    /// scheduling (C++ `simpleTimeProcess`).
    pub fn simple_time_process(&self) -> bool {
        self.num_tempo_changes <= 1
    }

    /// Absolute time of an event (C++ `timeOfEvent`).
    pub fn time_of_event(&self, ev: &MidiEvent) -> Duration {
        Duration::from_micros((ev.tick as f64 * self.ticks2micros) as u64)
    }

    /// Inter-event time of an event's delta (C++ `deltaTimeOfEvent`).
    pub fn delta_time_of_event(&self, ev: &MidiEvent) -> Duration {
        Duration::from_micros((ev.delta as f64 * self.ticks2micros) as u64)
    }

    /// Absolute time of a tick position (C++ `timeOfTicks`).
    pub fn time_of_ticks(&self, ticks: u64) -> Duration {
        Duration::from_micros((ticks as f64 * self.ticks2micros) as u64)
    }

    /// True when `channel` carries events (C++ `channelUsed`).
    pub fn channel_used(&self, channel: i32) -> bool {
        if (0..MIDI_CHANNELS as i32).contains(&channel) {
            self.channel_used[channel as usize]
        } else {
            false
        }
    }

    /// Dominant-track label for `channel` (C++ `channelLabel`, owned
    /// `std::string` by value — hence owned `String` here). Stored raw
    /// (C++ `std::string` keeps non-UTF8 bytes verbatim); decoded here.
    // TODO(T4): decode via charset-aware `decode_text` (T4 hook) instead
    // of lossy UTF-8, once `rmi.rs`/`wrk.rs` land the `decodeText` port.
    pub fn channel_label(&self, channel: i32) -> String {
        if !(0..MIDI_CHANNELS as i32).contains(&channel) {
            return String::new();
        }
        String::from_utf8_lossy(&self.channel_label[channel as usize]).into_owned()
    }

    /// Track with the most text hits (C++ `trackMaxPoints`; ties resolve
    /// to the lowest track number via the ordered map, as in C++).
    pub fn track_max_points(&self) -> i32 {
        let (mut best, mut bestv) = (-1, -1);
        for (&k, &v) in &self.trk_score {
            if v > bestv {
                bestv = v;
                best = k;
            }
        }
        best
    }

    /// Text type with the most hits (C++ `typeMaxPoints`).
    pub fn type_max_points(&self) -> i32 {
        let (mut best, mut bestv) = (-1, -1);
        for (&k, &v) in &self.typ_score {
            if v > bestv {
                bestv = v;
                best = k;
            }
        }
        best
    }

    /// Accumulated track/instrument name for `track` (C++ `trackName`,
    /// owned `std::string` by value — hence owned `String` here). Stored
    /// raw (C++ `std::string` keeps non-UTF8 bytes verbatim); decoded here.
    // TODO(T4): decode via charset-aware `decode_text` (T4 hook) instead
    // of lossy UTF-8, once `rmi.rs`/`wrk.rs` land the `decodeText` port.
    pub fn track_name(&self, track: i32) -> String {
        self.trk_name
            .get(&track)
            .map(|raw| String::from_utf8_lossy(raw).into_owned())
            .unwrap_or_default()
    }

    /// Dominant channel of `track`, -1 when unknown (C++ `trackChannel`).
    pub fn track_channel(&self, track: i32) -> i32 {
        self.trk_channel.get(&track).copied().unwrap_or(-1)
    }

    /// Loaded file base name (C++ `currentFile`, owned `std::string` by
    /// value — hence owned `String` here). Stored raw (`OsString`);
    /// lossy-decoded for display only.
    pub fn current_file(&self) -> String {
        self.current_file.to_string_lossy().into_owned()
    }

    /// Loaded file absolute path (C++ `currentFullFileName`, owned
    /// `std::string` by value — hence owned `String` here). Stored raw
    /// (`OsString`); lossy-decoded for display only.
    pub fn current_full_file_name(&self) -> String {
        self.current_file_full.to_string_lossy().into_owned()
    }

    /// Format label, e.g. `SMF type 1` (C++ `fileFormat`).
    pub fn file_format(&self) -> &str {
        &self.file_format
    }

    /// Song duration as `HH:MM:SS.mmm` (C++ `durationString`).
    pub fn duration_string(&self) -> String {
        let micros = self.time_of_ticks(self.ticks_duration as u64).as_micros() as f64;
        let ms_total = (micros / 1e6 * 1000.0).round() as i64;
        let h = ms_total / 3_600_000;
        let m = (ms_total / 60_000) % 60;
        let s = (ms_total / 1_000) % 60;
        let ms = ms_total % 1_000;
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    }

    /// RIFF INFO metadata as HTML lines (C++ `metadataInfo`; empty in T3
    /// — T4's RMI decoder fills the map).
    pub fn metadata_info(&self) -> String {
        let mut out = String::new();
        for (k, v) in &self.info_map {
            out.push_str(k);
            out.push_str(": <b>");
            out.push_str(v);
            out.push_str("</b><br/>");
        }
        out
    }

    /// Loading errors as HTML lines (C++ `loadingErrors`).
    pub fn loading_errors(&self) -> String {
        let mut out = String::new();
        for e in &self.loading_errors {
            out.push_str(e);
            out.push_str("<br/>");
        }
        out
    }

    /// Loading error count (C++ `errorsCount`).
    pub fn errors_count(&self) -> i32 {
        self.loading_errors.len() as i32
    }

    /// Active decode charset (C++ `currentCharset`; always UTF-8 in T3).
    pub fn current_charset(&self) -> &str {
        &self.charset
    }

    /// Override the decode charset; empty values are ignored (C++
    /// `setCurrentCharset`).
    pub fn set_current_charset(&mut self, charset: &str) {
        if !charset.is_empty() {
            self.charset = charset.to_string();
        }
    }

    /// Stored text records (C++ `textEvents`).
    pub fn text_events(&self) -> &[TextRec] {
        &self.text_events
    }

    /// Event count (C++ `size`).
    pub fn size(&self) -> usize {
        self.events.len()
    }

    /// Sorted event store (C++ `events`).
    pub fn events(&self) -> &[MidiEvent] {
        &self.events
    }

    /// Stable tick sort + delta recompute + duration/bar/tempo-converter
    /// refresh (C++ `sortAndFinalize`).
    pub(crate) fn sort_and_finalize(&mut self) {
        self.insert_beats_to_end();
        self.events.sort_by_key(|ev| ev.tick);
        let mut last = 0i64;
        let mut ticks_duration = self.ticks_duration;
        let mut bar_count = self.bar_count;
        for ev in &mut self.events {
            ev.delta = ev.tick - last;
            last = ev.tick;
            if ev.tick > ticks_duration {
                ticks_duration = ev.tick;
            }
            if ev.kind == Kind::Beat {
                bar_count = bar_count.max(ev.a);
            }
        }
        self.ticks_duration = ticks_duration;
        self.bar_count = bar_count;
        self.ticks2micros = self.tempo / (1.max(self.division) as f64 * self.tempo_factor);
    }

    /// Synthesize `Beat` events (`a`=bar, `b`=beat, `c`=beats-per-bar)
    /// from tick 0 to the song end, following the time-signature change
    /// list (C++ `insertBeatsToEnd`).
    pub(crate) fn insert_beats_to_end(&mut self) {
        if self.division <= 0 {
            self.division = 120;
        }
        let mut beat_len: i64 = if self.beat_length > 0 {
            self.beat_length
        } else {
            self.division as i64
        };
        let mut num: i32 = if self.beat_max > 0 { self.beat_max } else { 4 };
        let (mut bar, mut beat) = (1i32, 1i32);
        let mut t = 0i64;
        let end = self.ticks_duration;
        // C++ uses `std::sort` (unstable); the stable order is one valid
        // outcome and keeps equal-time changes deterministic.
        let mut changes = self.bars.clone();
        changes.sort_by_key(|c| c.time);
        let mut ci = 0;
        while t <= end {
            while ci < changes.len() && changes[ci].time <= t {
                num = if changes[ci].num > 0 {
                    changes[ci].num
                } else {
                    4
                };
                let den = if changes[ci].den > 0 {
                    changes[ci].den
                } else {
                    4
                };
                beat_len = self.division as i64 * 4 / den as i64;
                if beat_len <= 0 {
                    beat_len = self.division as i64;
                }
                ci += 1;
            }
            let mut ev = make_beat(bar, beat, num);
            ev.tick = t;
            self.events.push(ev);
            t += beat_len;
            beat += 1;
            if beat > num {
                beat = 1;
                bar += 1;
            }
            if beat_len <= 0 {
                break;
            }
        }
        self.bar_count = 1.max(bar - if beat == 1 { 1 } else { 0 });
        self.beat_max = num;
        self.beat_length = beat_len;
    }

    /// Store one text meta event (C++ `addMetaData`): feeds the charset
    /// sample, scores track/type for lyric defaults, maps KAR `@K/@V/@I/
    /// @L/@T/@W` headers, stores the raw record, and — for Lyric/Text —
    /// appends a playable `Text` event; TrackName/InstrumentName
    /// accumulate into the track-name map. Empty payloads are ignored.
    pub(crate) fn add_meta_data(&mut self, time: i64, track: i32, typ: i32, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.charset_sample.extend_from_slice(data);
        *self.trk_score.entry(track).or_insert(0) += 1;
        *self.typ_score.entry(typ).or_insert(0) += 1;
        let mut t = TextType::try_from(typ).unwrap_or(TextType::None);
        if data.len() > 1 && data[0] == b'@' {
            t = match data[1] {
                b'K' => TextType::KarFileType,
                b'V' => TextType::KarVersion,
                b'I' => TextType::KarInformation,
                b'L' => TextType::KarLanguage,
                b'T' => TextType::KarTitles,
                b'W' => TextType::KarWhatever,
                _ => t,
            };
        }
        self.text_events.push(TextRec {
            tick: time,
            track,
            text_type: t,
            text: data.to_vec(),
        });
        if t == TextType::Lyric || t == TextType::Text {
            self.events.push(MidiEvent {
                kind: Kind::Text,
                tick: time,
                tag: track,
                a: t as i32,
                data: data.to_vec(),
                ..MidiEvent::default()
            });
        } else if t == TextType::TrackName || t == TextType::InstrumentName {
            // Raw storage: C++ accumulates `std::string` bytes verbatim
            // (sequence.cpp `addMetaData`); decode happens in the
            // `track_name` accessor (T4 hook), never here.
            let entry = self.trk_name.entry(track).or_default();
            if entry.is_empty() {
                *entry = data.to_vec();
            } else {
                entry.push(b' ');
                entry.extend_from_slice(data);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::core::event::{make_control, make_note_on, make_tempo};

    /// Two bars of 4/4 beats at 120 ticks/beat, as `insert_beats_to_end`
    /// would synthesize them (bar, beat, max, tick).
    fn beat_fixture() -> Sequence {
        let mut seq = Sequence::default();
        for (i, (bar, beat)) in [(1, 1), (1, 2), (1, 3), (1, 4), (2, 1), (2, 2)]
            .iter()
            .enumerate()
        {
            let mut ev = make_beat(*bar, *beat, 4);
            ev.tick = i as i64 * 120;
            seq.events.push(ev);
        }
        seq.bar_count = 2;
        seq
    }

    #[test]
    fn tempo_factor_rejects_out_of_range() {
        // Arch A7: reject (keep old value), never clamp.
        let mut seq = Sequence::default();
        seq.set_tempo_factor(2.0);
        assert!((seq.tempo_factor() - 2.0).abs() < f64::EPSILON);
        let conv = seq.ticks2micros;
        assert!((conv - 500_000.0 / (120.0 * 2.0)).abs() < 1e-9);
        for bad in [0.099_999, 0.0, -1.0, 10.000_001, 100.0, f64::NAN] {
            seq.set_tempo_factor(bad);
            assert!(
                (seq.tempo_factor() - 2.0).abs() < f64::EPSILON,
                "factor {bad} must be rejected"
            );
            assert!(
                (seq.ticks2micros - conv).abs() < 1e-12,
                "converter must not move on reject"
            );
        }
        // Boundaries are inclusive.
        seq.set_tempo_factor(0.1);
        assert!((seq.tempo_factor() - 0.1).abs() < f64::EPSILON);
        seq.set_tempo_factor(10.0);
        assert!((seq.tempo_factor() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tempo_math_matches_cpp() {
        let mut seq = Sequence::default();
        seq.division = 480;
        seq.set_tempo_factor(1.0);
        // 500000 µs/qn at div 480: one quarter = exactly 500000 µs.
        let mut ev = make_note_on(0, 60, 100);
        ev.tick = 480;
        ev.delta = 480;
        assert_eq!(seq.time_of_event(&ev).as_micros(), 500_000);
        assert_eq!(seq.delta_time_of_event(&ev).as_micros(), 500_000);
        assert_eq!(seq.time_of_ticks(960).as_micros(), 1_000_000);
        // Tempo events refresh the converter; initial/current split.
        seq.update_tempo(250_000.0);
        assert!((seq.current_tempo() - 250_000.0).abs() < f64::EPSILON);
        assert_eq!(seq.time_of_ticks(480).as_micros(), 250_000);
        seq.set_tempo_factor(2.0);
        assert!((seq.current_tempo() - 125_000.0).abs() < f64::EPSILON);
        assert_eq!(seq.time_of_ticks(480).as_micros(), 125_000);
        // Same-value update is a no-op (converter untouched).
        let conv = seq.ticks2micros;
        seq.update_tempo(250_000.0);
        assert!((seq.ticks2micros - conv).abs() < f64::EPSILON);
    }

    #[test]
    fn duration_string_format() {
        let mut seq = Sequence::default();
        seq.division = 480;
        seq.set_tempo_factor(1.0);
        seq.ticks_duration = 96_000; // 100 s at 500000 µs/qn
        assert_eq!(seq.duration_string(), "00:01:40.000");
        seq.ticks_duration = 0;
        assert_eq!(seq.duration_string(), "00:00:00.000");
        // lround parity: 1.5 ms rounds up.
        seq.tempo = 500_000.0;
        seq.ticks2micros = 500_000.0 / 480.0;
        seq.ticks_duration = 1; // 1041.667 µs -> 1 ms
        assert_eq!(seq.duration_string(), "00:00:00.001");
    }

    #[test]
    fn cursor_next_event_and_reset() {
        let mut seq = beat_fixture();
        assert!(seq.has_more_events());
        let ticks: Vec<i64> = std::iter::from_fn(|| seq.next_event().map(|ev| ev.tick)).collect();
        assert_eq!(ticks, vec![0, 120, 240, 360, 480, 600]);
        assert!(!seq.has_more_events());
        assert!(seq.next_event().is_none());
        seq.reset_position();
        assert!(seq.has_more_events());
        assert_eq!(seq.next_event().map(|ev| ev.tick), Some(0));
    }

    #[test]
    fn set_tick_position_scans_linearly() {
        let mut seq = beat_fixture();
        seq.set_tick_position(240);
        assert_eq!(seq.next_event().map(|ev| ev.tick), Some(240));
        // Between events: first at or past the tick.
        seq.set_tick_position(241);
        assert_eq!(seq.next_event().map(|ev| ev.tick), Some(360));
        // Past the end: cursor parks at end-of-song.
        seq.set_tick_position(999_999);
        assert!(!seq.has_more_events());
    }

    #[test]
    fn bar_navigation() {
        let mut seq = beat_fixture();
        // jump_to_bar lands the cursor on the downbeat tick.
        assert_eq!(seq.jump_to_bar(2), Some(4));
        assert_eq!(seq.next_event().map(|ev| ev.tick), Some(480));
        // Past the last bar: falls back to the last beat, cursor follows.
        assert_eq!(seq.jump_to_bar(99), Some(5));
        assert_eq!(seq.next_event().map(|ev| ev.tick), Some(600));
        // No beats at all: None, cursor untouched.
        let mut empty = Sequence::default();
        assert_eq!(empty.jump_to_bar(1), None);

        assert_eq!(seq.first_beat(), Some(0));
        assert_eq!(seq.previous_bar(Some(4)), Some(0));
        assert_eq!(seq.next_bar(Some(0)), Some(4));
        // None resolves to the first beat (C++ null path).
        assert_eq!(seq.previous_bar(None), Some(0));
        assert_eq!(seq.next_bar(None), Some(0));
        // Edges fall back: no earlier downbeat -> earliest beat;
        // no later downbeat -> latest beat.
        assert_eq!(seq.previous_bar(Some(0)), Some(0));
        assert_eq!(seq.next_bar(Some(5)), Some(5));
        // Nearest beat by ticks: latest at or before, None before all.
        assert_eq!(seq.nearest_beat_by_ticks(479), Some(3));
        assert_eq!(seq.nearest_beat_by_ticks(480), Some(4));
        assert_eq!(seq.nearest_beat_by_ticks(-1), None);
        assert_eq!(empty.nearest_beat_by_ticks(100), None);
        assert_eq!(empty.first_beat(), None);
    }

    #[test]
    fn sort_and_finalize_is_stable_and_recomputes() {
        let mut seq = Sequence::default();
        seq.division = 480;
        seq.beat_length = 480; // load_smf_bytes copies division here
        let mut first = make_control(0, 7, 100);
        first.tick = 480;
        let mut second = make_note_on(1, 64, 80);
        second.tick = 480;
        let mut third = make_tempo(400_000.0);
        third.tick = 0;
        seq.events.extend([first, second, third]);
        seq.ticks_duration = 480;
        seq.sort_and_finalize();
        // Stable: same-tick insertion order survives (beats join at 0/480).
        let kinds: Vec<(i64, Kind)> = seq.events.iter().map(|ev| (ev.tick, ev.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (0, Kind::Tempo),
                (0, Kind::Beat),
                (480, Kind::ControlChange),
                (480, Kind::NoteOn),
                (480, Kind::Beat),
            ]
        );
        // Deltas recomputed from the sorted order.
        let deltas: Vec<i64> = seq.events.iter().map(|ev| ev.delta).collect();
        assert_eq!(deltas, vec![0, 0, 480, 0, 0]);
        assert_eq!(seq.song_length_ticks(), 480);
        assert_eq!(seq.last_bar(), 1);
    }

    #[test]
    fn beats_follow_time_signature_changes() {
        let mut seq = Sequence::default();
        seq.division = 120;
        seq.ticks_duration = 480;
        seq.bars.push(TimeSigRec {
            bar: 1,
            num: 3,
            den: 4,
            time: 240,
        });
        seq.insert_beats_to_end();
        let beats: Vec<(i32, i32, i32, i64)> = seq
            .events
            .iter()
            .map(|ev| (ev.a, ev.b, ev.c, ev.tick))
            .collect();
        // 4/4 for ticks 0..240, then 3/4: beat 4 exceeds the new
        // meter, so the bar rolls over immediately (C++ parity).
        assert_eq!(
            beats,
            vec![
                (1, 1, 4, 0),
                (1, 2, 4, 120),
                (1, 3, 3, 240),
                (2, 1, 3, 360),
                (2, 2, 3, 480),
            ]
        );
        assert_eq!(seq.last_bar(), 2);
    }

    #[test]
    fn metadata_scoring_and_charset_accessors() {
        let mut seq = Sequence::default();
        assert_eq!(seq.track_max_points(), -1);
        assert_eq!(seq.type_max_points(), -1);
        seq.add_meta_data(0, 2, 5, b"la ");
        seq.add_meta_data(10, 2, 5, b"la");
        seq.add_meta_data(20, 3, 1, b"hi");
        assert_eq!(seq.track_max_points(), 2);
        assert_eq!(seq.type_max_points(), 5);
        // Lyric/Text metas become playable Text events; raw records kept.
        assert_eq!(seq.text_events().len(), 3);
        assert_eq!(
            seq.events.iter().filter(|ev| ev.kind == Kind::Text).count(),
            3
        );
        assert_eq!(seq.events[0].a, TextType::Lyric as i32);
        // TrackName accumulates with a space join.
        seq.add_meta_data(0, 1, 3, b"Piano");
        seq.add_meta_data(0, 1, 3, b"RH");
        assert_eq!(seq.track_name(1), "Piano RH");
        assert_eq!(seq.track_name(9), "");
        // Empty payloads are ignored entirely.
        let before = seq.text_events().len();
        seq.add_meta_data(0, 1, 5, b"");
        assert_eq!(seq.text_events().len(), before);
        // KAR @-headers remap the stored type.
        seq.add_meta_data(0, 1, 1, b"@KMIDI KARAOKE FILE");
        assert_eq!(
            seq.text_events().last().unwrap().text_type,
            TextType::KarFileType
        );
        assert_eq!(seq.current_charset(), "UTF-8");
        seq.set_current_charset("SHIFT_JIS");
        assert_eq!(seq.current_charset(), "SHIFT_JIS");
        seq.set_current_charset("");
        assert_eq!(seq.current_charset(), "SHIFT_JIS");
    }

    #[test]
    fn load_file_rejects_missing_and_empty() {
        let mut seq = Sequence::default();
        assert!(!seq.load_file(Path::new("/tmp/goshmidi-t3-no-such-file-12345.mid")));
        assert_eq!(seq.errors_count(), 1);
        assert_eq!(seq.loading_errors(), "File not found<br/>");

        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.mid");
        std::fs::write(&empty, b"").unwrap();
        assert!(!seq.load_file(&empty));
        assert_eq!(seq.loading_errors(), "Unable to read file<br/>");
        assert_eq!(seq.metadata_info(), "");
    }

    #[test]
    fn rmi_and_wrk_record_pending_t4_error() {
        // T3 dispatch arms: `.rmi`/`.wrk` (any case) record a pending
        // error until T4 wires `rmi.rs`/`wrk.rs` — no panic, no events.
        let dir = tempfile::tempdir().unwrap();
        for name in ["song.rmi", "song.wrk", "song.RMI", "song.WRK"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"nonempty but undecodable until T4").unwrap();
            let mut seq = Sequence::default();
            assert!(!seq.load_file(&path), "{name} must fail until T4");
            assert!(seq.empty(), "{name} must store no events");
            assert_eq!(seq.errors_count(), 1, "{name}");
            assert_eq!(
                seq.loading_errors(),
                "RMI/WRK decoding lands in T4<br/>",
                "{name}"
            );
        }
    }

    #[test]
    fn channel_accessors_reject_out_of_range() {
        let seq = Sequence::default();
        for ch in [-1, 16, 99] {
            assert!(!seq.channel_used(ch));
            assert_eq!(seq.channel_label(ch), "");
        }
    }
}
