//! `Sequence`: event store + timing + navigation — port of
//! `src/midi/sequence.{hpp,cpp}` (PLAN T3+T4, arch §1.3).
//!
//! Covers: `clear`, `loadFile` (SMF/RMI/WRK dispatch), `sortAndFinalize`,
//! `insertBeatsToEnd`, `addMetaData` (raw storage; KAR `@`-headers), tempo
//! math (`setTempoFactor` **rejects** out-of-range factors per arch A7 —
//! no clamping), bar/beat navigation, `durationString`/`metadataInfo`/
//! `loadingErrors`, and the T4 text/charset layer (`feedCharset`,
//! `detectCharset` via chardetng, `decodeText` via encoding_rs + the A10
//! normalization spec, `getText`/`getRawText`/`extraCodecNames`).
//!
//! Dependency rule (arch §5): `std` + detector/codec crates only — no
//! iced/libcosmic in `core`.

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

/// Per-track WRK channel/pitch/velocity offsets (C++ `TrackMapRec`),
/// keyed by 1-based track number. `channel == -1` means "no override".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackMapRec {
    pub channel: i32,
    pub pitch: i32,
    pub velocity: i32,
}

impl Default for TrackMapRec {
    fn default() -> Self {
        TrackMapRec {
            channel: -1,
            pitch: 0,
            velocity: 0,
        }
    }
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
    pub(crate) track_map: BTreeMap<i32, TrackMapRec>,
    pub(crate) saved_sysex: BTreeMap<i32, MidiEvent>,
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
            track_map: BTreeMap::new(),
            saved_sysex: BTreeMap::new(),
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

/// True for charset labels that `g_convert`/iconv decode as true
/// ISO-8859-1 (bytes 0x80–0x9F → U+0080–U+009F controls): the dropdown
/// spelling plus its common aliases, ASCII-case-insensitive. WHATWG folds
/// all of these onto WINDOWS-1252 (printable glyphs there), so
/// `decode_text` special-cases them before `Encoding::for_label` per the
/// A10 spec. `WINDOWS-1252` itself is deliberately absent.
fn is_latin1_label(charset: &str) -> bool {
    const ALIASES: &[&str] = &[
        "iso-8859-1",
        "iso8859-1",
        "iso88591",
        "iso_8859-1",
        "iso_8859-1:1987",
        "iso-ir-100",
        "l1",
        "latin1",
        "csisolatin1",
        "cp819",
        "ibm819",
    ];
    ALIASES.iter().any(|alias| charset.eq_ignore_ascii_case(alias))
}

/// True for charset labels that `g_convert`/iconv decode as true
/// ISO-8859-9 (latin5): the dropdown spelling plus its common aliases,
/// ASCII-case-insensitive. WHATWG folds all of these onto WINDOWS-1254
/// (same class of fold as latin1 → WINDOWS-1252), so `decode_text`
/// special-cases them before `Encoding::for_label`.
fn is_latin5_label(charset: &str) -> bool {
    const ALIASES: &[&str] = &[
        "iso-8859-9",
        "iso8859-9",
        "iso88599",
        "iso_8859-9",
        "iso-ir-148",
        "l5",
        "latin5",
        "csisolatin5",
    ];
    ALIASES.iter().any(|alias| charset.eq_ignore_ascii_case(alias))
}

/// ISO-8859-9 (latin5) single-byte table: identity except the six Turkish
/// letters (0xD0/0xDD/0xDE/0xF0/0xFD/0xFE). Bytes 0x80–0x9F are C1
/// controls, as in every true latin table — never windows-1254 glyphs.
fn latin5_char(byte: u8) -> char {
    match byte {
        0xD0 => 'Ğ',
        0xDD => 'İ',
        0xDE => 'Ş',
        0xF0 => 'ğ',
        0xFD => 'ı',
        0xFE => 'ş',
        _ => byte as char,
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

    /// Load a song file (C++ `Sequence::loadFile`): `.wrk` →
    /// `wrk.rs`, `.rmi` → `rmi.rs`, anything else (`.mid/.midi/.kar`)
    /// → the SMF decoder.
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
        let ok = if ext == b".wrk" {
            crate::core::wrk::load_wrk(self, &bytes)
        } else if ext == b".rmi" {
            crate::core::rmi::load_rmi(self, &bytes)
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

    /// Accumulate raw text bytes for charset detection (C++
    /// `Sequence::feedCharset`).
    pub(crate) fn feed_charset(&mut self, data: &[u8]) {
        self.charset_sample.extend_from_slice(data);
    }

    /// Detect the song charset from the accumulated text sample (C++
    /// `Sequence::detectCharset`, uchardet → chardetng per arch §3.4).
    /// An empty sample keeps the default UTF-8, as in C++. Otherwise the
    /// stored name is the chardetng guess's canonical `encoding_rs` name
    /// (e.g. `windows-1252`, `Shift_JIS`, `Big5`), which `decode_text`
    /// resolves back via `Encoding::for_label`.
    pub(crate) fn detect_charset(&mut self) {
        if self.charset_sample.is_empty() {
            self.charset = "UTF-8".to_string();
            return;
        }
        // MIDI text is not web content, so ISO-2022-JP stays a possible
        // guess (the email/honest-detection setting), and UTF-8 is always
        // allowed; the TLD is unknown so detection runs unconditioned.
        let mut detector =
            chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
        detector.feed(&self.charset_sample, true);
        let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
        self.charset = encoding.name().to_string();
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
    /// (C++ `std::string` keeps non-UTF8 bytes verbatim); decoded here
    /// with the current charset.
    pub fn channel_label(&self, channel: i32) -> String {
        if !(0..MIDI_CHANNELS as i32).contains(&channel) {
            return String::new();
        }
        self.decode_text(&self.channel_label[channel as usize])
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
    /// raw (C++ `std::string` keeps non-UTF8 bytes verbatim); decoded here
    /// with the current charset.
    pub fn track_name(&self, track: i32) -> String {
        self.trk_name
            .get(&track)
            .map(|raw| self.decode_text(raw))
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

    /// Decode raw text bytes with the current charset (C++ `decodeText`,
    /// `g_convert` → encoding_rs per arch §3.4/A10):
    ///
    /// * `ISO-8859-1` (and its latin1 aliases) decodes byte-for-byte to
    ///   U+0000–U+00FF — bytes 0x80–0x9F become C1 controls — exactly as
    ///   `g_convert` does. This branch must come first: WHATWG folds every
    ///   latin1 label onto WINDOWS-1252, where those bytes are printable
    ///   glyphs (€, ‚, …) instead. The two labels are never normalized
    ///   into each other.
    /// * `ISO-8859-9` (latin5) gets the same treatment against its WHATWG
    ///   fold target WINDOWS-1254: the true latin5 table (C1 controls +
    ///   the six Turkish letters), never the windows-1254 glyphs.
    /// * Any other name resolves via `Encoding::for_label`, which is
    ///   ASCII-case-insensitive and alias-tolerant like `g_convert`, so
    ///   detector spellings (`Shift_JIS`, `Big5`, `EUC-KR`, …) and legacy
    ///   song-setting values (`GB2312`, `x-mac-cyrillic`, …) map onto the
    ///   right codec.
    /// * Malformed input under a known codec, and any input under an
    ///   unknown name, falls back to lossy UTF-8. (C++ fails the whole
    ///   record and returns the raw bytes verbatim; Rust `String` must be
    ///   valid UTF-8, so lossy decode is the adaptation — byte-identical
    ///   to C++ whenever the raw bytes are valid UTF-8, the common case
    ///   of UTF-8 lyrics viewed under another codec.)
    ///
    /// Known library delta (pinned by goldens, not emulated): WHATWG
    /// Shift_JIS maps bytes 0x5C/0x7E to U+005C/U+007E while `g_convert`
    /// maps them to U+00A5/U+203E (¥/‾), so backslash/tilde bytes diverge
    /// under an explicit SHIFT_JIS selection.
    pub fn decode_text(&self, data: &[u8]) -> String {
        if data.is_empty() {
            return String::new();
        }
        if is_latin1_label(&self.charset) {
            return data.iter().map(|&b| b as char).collect();
        }
        if is_latin5_label(&self.charset) {
            return data.iter().map(|&b| latin5_char(b)).collect();
        }
        match encoding_rs::Encoding::for_label(self.charset.as_bytes()) {
            Some(encoding) => {
                let (text, _, malformed) = encoding.decode(data);
                if malformed {
                    String::from_utf8_lossy(data).into_owned()
                } else {
                    text.into_owned()
                }
            }
            None => String::from_utf8_lossy(data).into_owned(),
        }
    }

    /// Decoded payloads of every stored record of `text_type` (C++
    /// `getText`).
    pub fn get_text(&self, text_type: TextType) -> Vec<String> {
        self.text_events
            .iter()
            .filter(|rec| rec.text_type == text_type)
            .map(|rec| self.decode_text(&rec.text))
            .collect()
    }

    /// Raw `(tick, bytes)` payloads filtered by track and type (C++
    /// `getRawText`): `track <= 0` matches all tracks,
    /// `TextType::None` matches all types.
    pub fn get_raw_text(&self, track: i32, text_type: TextType) -> Vec<(i32, Vec<u8>)> {
        self.text_events
            .iter()
            .filter(|rec| {
                (track <= 0 || rec.track == track)
                    && (text_type == TextType::None || rec.text_type == text_type)
            })
            .map(|rec| (rec.tick as i32, rec.text.clone()))
            .collect()
    }

    /// Codec names for the lyrics Encoding dropdown (C++
    /// `extraCodecNames`): the same 14 entries in the same order.
    ///
    /// Off-list rule (window.cpp:575-585, preserved for the T11 UI): the
    /// dropdown compares case-insensitively and selects index 0 when the
    /// detected name matches no entry, while the engine keeps decoding
    /// with the detected `m_charset` — display ≠ decode charset.
    pub fn extra_codec_names() -> Vec<String> {
        [
            "UTF-8",
            "ISO-8859-1",
            "WINDOWS-1252",
            "SHIFT_JIS",
            "GBK",
            "BIG5",
            "KOI8-R",
            "EUC-KR",
            "ISO-8859-2",
            "ISO-8859-5",
            "ISO-8859-9",
            "WINDOWS-1251",
            "WINDOWS-1250",
            "MACINTOSH",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
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
        self.feed_charset(data);
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
    fn rmi_and_wrk_dispatch_to_t4_decoders() {
        // `.rmi`/`.wrk` (any case) reach the T4 decoders: garbage bytes
        // surface each decoder's own header error (C++ oracle strings).
        let dir = tempfile::tempdir().unwrap();
        for (name, error) in [
            ("song.rmi", "Not a RIFF file<br/>"),
            ("song.RMI", "Not a RIFF file<br/>"),
            ("song.wrk", "Invalid WRK header<br/>"),
            ("song.WRK", "Invalid WRK header<br/>"),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"nonempty but not a container").unwrap();
            let mut seq = Sequence::default();
            assert!(!seq.load_file(&path), "{name} must fail");
            assert!(seq.empty(), "{name} must store no events");
            assert_eq!(seq.loading_errors(), error, "{name}");
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

    // --- T4 text/charset goldens ---
    //
    // Oracle: throwaway `/tmp/t4_dump` (NOT committed) built as
    // `g++ -std=c++17 -I src /tmp/t4_dump.cpp build/libdmidi-core.a
    // $(pkg-config --libs glib-2.0 uchardet)` and run over
    // `examples/*.mid examples/*.kar`. Its `CODEC` rows (concatenated
    // `g_convert` decode of every text record per codec: length + FNV-1a64)
    // are pasted below verbatim; the Rust side must reproduce them exactly
    // except for the pinned `DIVERGENT` cells, where the libraries
    // genuinely differ (see each reason).

    fn fnv1a64_str(s: &str) -> u64 {
        let mut h = 14695981039346656037u64;
        for &b in s.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }

    /// Concatenated decode of every text record under `codec`, as the
    /// oracle's `CODEC` rows do.
    fn decode_all(seq: &mut Sequence, codec: &str) -> String {
        seq.set_current_charset(codec);
        let mut cat = String::new();
        for rec in seq.text_events() {
            cat.push_str(&seq.decode_text(&rec.text));
        }
        cat
    }

    struct CodecFixture {
        name: &'static str,
        /// Verbatim oracle `(decoded length, FNV-1a64)` per codec, in
        /// `extra_codec_names` order.
        oracle: [(usize, u64); 14],
    }

    const CODEC_FIXTURES: &[CodecFixture] = &[
        CodecFixture {
            name: "haendel_hallelujah.mid",
            oracle: [
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1658, 0xdc13299fc2e6d49f),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
                (1619, 0xf8b9d9e3e9414be4),
            ],
        },
        CodecFixture {
            name: "lindaamiga.mid",
            oracle: [(784, 0x79613d8e28abeffc); 14],
        },
        CodecFixture {
            name: "mozart_aveverum.mid",
            oracle: [(592, 0xd5db686a98fbdc7d); 14],
        },
        CodecFixture {
            name: "mozart_diesirae.mid",
            oracle: [(1044, 0xd0560a7b666ca263); 14],
        },
        CodecFixture {
            name: "mozart_konigdernacht.mid",
            oracle: [
                (1168, 0xdd458106513e539a),
                (1186, 0x6958bd8885fa8c53),
                (1186, 0x6958bd8885fa8c53),
                (1225, 0x7fa80732d6d1f144),
                (1177, 0x579b2e2cd47acec3),
                (1177, 0x55fec90125f673bd),
                (1195, 0x27e8ac5689154629),
                (1177, 0x591b5d573416a987),
                (1186, 0xabf61e17137b2ef8),
                (1186, 0x63e1dda3e96642e4),
                (1186, 0x6958bd8885fa8c53),
                (1186, 0x9d54a25ebf20419f),
                (1186, 0x018f6b6f8fd210b8),
                (1201, 0xd06ad270c2e03a63),
            ],
        },
        CodecFixture {
            name: "schubert_avemaria.mid",
            oracle: [
                (1426, 0x98ec118504301c92),
                (1447, 0x4219ed8dec7d7f2a),
                (1449, 0xa935d09b0aba881b),
                (1462, 0x0992ff5efd9153a1),
                (1435, 0x4a601a094d6615df),
                (1435, 0x08214d00162703e3),
                (1458, 0x060236a4b111befd),
                (1435, 0x10b38c4b2b2261d3),
                (1447, 0x8eae2eceef14cfdf),
                (1447, 0x5549e8d4c27bee17),
                (1447, 0x4219ed8dec7d7f2a),
                (1448, 0x27ae34a0c8788e5a),
                (1449, 0xabecaf13b47f4d76),
                (1461, 0x1efad3f924c6ae0c),
            ],
        },
        CodecFixture {
            name: "Schubert_Standchen.mid",
            oracle: [
                (783, 0x5d9f9d58ae3ac7fd),
                (817, 0x987f5060b8592e52),
                (817, 0x7dbe5c152d05d3ca),
                (846, 0xb53c39df50cadd4f),
                (800, 0xf921b6e9029deac9),
                (798, 0x089fe29bd0089186),
                (831, 0x2a8de8cfa3ce2dba),
                (798, 0x7d65bc101af5ae54),
                (817, 0x00365bd0345ee551),
                (817, 0xbd1af37d4a4f44c3),
                (817, 0x987f5060b8592e52),
                (817, 0x794ef094065a8d88),
                (817, 0x86ffbba956720915),
                (839, 0x320043995db5bab6),
            ],
        },
        CodecFixture {
            name: "test.mid",
            oracle: [
                (47, 0x7af40e2cd7665f13),
                (49, 0xb0919b1bbec8b9f0),
                (49, 0xb0919b1bbec8b9f0),
                (51, 0x766f5fa2cecfb552),
                (48, 0x3bae3e459620b8c7),
                (48, 0xa78fe18a458bf987),
                (49, 0xf5857202dee3b749),
                (48, 0x34494ae4963d3f2d),
                (49, 0xefb84f16eb836360),
                (49, 0xe222b0c26dafac51),
                (49, 0xb0919b1bbec8b9f0),
                (49, 0x58d2ec192487e44b),
                (49, 0xefb84f16eb836360),
                (51, 0x425a9cd8564b041d),
            ],
        },
        CodecFixture {
            name: "Negra_Sombra.kar",
            oracle: [
                (883, 0x21bf8542ec18918a),
                (899, 0x6a719c1e03221e0a),
                (899, 0x6a719c1e03221e0a),
                (920, 0x9a53fa48443c847b),
                (891, 0xd3f1e619bd547bbd),
                (891, 0x88d48f92fee099cb),
                (905, 0xce3e83d1e6bacd9d),
                (891, 0x5047c4e3636e2cf5),
                (899, 0x363b41db2f8fbc21),
                (899, 0x2f704eb96c9906ac),
                (899, 0x6a719c1e03221e0a),
                (899, 0xa3d94180af7f805b),
                (899, 0x6c1408b6c30c834b),
                (910, 0x4b18f19bbd7dec2b),
            ],
        },
        CodecFixture {
            name: "twinkle.kar",
            oracle: [
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (650, 0x617ef059881a2d9c),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
                (648, 0x34e08aa2a95b567e),
            ],
        },
    ];

    /// The 5 of 140 codec cells where the libraries genuinely differ (the
    /// oracle C++ pair is recorded alongside the pinned Rust pair — both
    /// sides are honest, neither is emulated):
    /// * SHIFT_JIS ×4: bytes 0x5C/0x7E decode to U+00A5/U+203E (¥/‾) in
    ///   `g_convert` but stay U+005C/U+007E in WHATWG Shift_JIS.
    /// * EUC-KR ×1 (`/Laß `, bytes `C3 9F`): glib's strict trail range
    ///   (`0xA1+`) errors and returns the record raw, while WHATWG EUC-KR
    ///   accepts the low trail and decodes Hangul U+D6A9.
    const DIVERGENT: &[(&str, &str, usize, u64)] = &[
        ("haendel_hallelujah.mid", "SHIFT_JIS", 1619, 0xf8b9d9e3e9414be4),
        ("mozart_konigdernacht.mid", "SHIFT_JIS", 1204, 0x0cdd3c0f50d66859),
        ("Schubert_Standchen.mid", "EUC-KR", 800, 0x60331bb9c4ad82a2),
        ("Negra_Sombra.kar", "SHIFT_JIS", 915, 0x0af6041f8942ff02),
        ("twinkle.kar", "SHIFT_JIS", 648, 0x34e08aa2a95b567e),
    ];

    #[test]
    fn lyric_decode_sweep_matches_oracle_across_fixtures_and_charsets() {
        assert_eq!(CODEC_FIXTURES.len(), 10);
        let codecs = Sequence::extra_codec_names();
        assert_eq!(codecs.len(), 14);
        let mut checked = 0;
        for fx in CODEC_FIXTURES {
            let path = Path::new("examples").join(fx.name);
            let mut seq = Sequence::default();
            assert!(seq.load_file(&path), "{}: {:?}", fx.name, seq.loading_errors);
            for (i, codec) in codecs.iter().enumerate() {
                let cat = decode_all(&mut seq, codec);
                let (len, fnv) = (cat.len(), fnv1a64_str(&cat));
                match DIVERGENT.iter().find(|(f, c, _, _)| *f == fx.name && *c == codec) {
                    Some((_, _, rust_len, rust_fnv)) => {
                        // Pinned library delta: assert the Rust pair, and
                        // that it still differs from the oracle (a silent
                        // convergence must update this table, not slip by).
                        assert_eq!((len, fnv), (*rust_len, *rust_fnv), "{} {codec}", fx.name);
                        assert_ne!((len, fnv), fx.oracle[i], "{} {codec} converged?", fx.name);
                    }
                    None => assert_eq!((len, fnv), fx.oracle[i], "{} {codec}", fx.name),
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 140);
    }

    #[test]
    fn default_decoded_lyrics_match_oracle_except_detector_deltas() {
        // (file, chardetng verdict, uchardet verdict, decoded length,
        // FNV-1a64 of the concatenated default decode). chardetng reads
        // every fixture sample as UTF-8; the decode then equals the C++
        // default everywhere except `test.mid`, where uchardet's
        // `MAC-CENTRALEUROPE` misdetection mojibakes (oracle pair
        // 51/`425a9cd8564b041d`) while chardetng decodes correctly.
        const DEFAULTS: &[(&str, &str, &str, usize, u64)] = &[
            ("haendel_hallelujah.mid", "UTF-8", "ASCII", 1619, 0xf8b9d9e3e9414be4),
            ("lindaamiga.mid", "UTF-8", "ASCII", 784, 0x79613d8e28abeffc),
            ("mozart_aveverum.mid", "UTF-8", "ASCII", 592, 0xd5db686a98fbdc7d),
            ("mozart_diesirae.mid", "UTF-8", "ASCII", 1044, 0xd0560a7b666ca263),
            ("mozart_konigdernacht.mid", "UTF-8", "UTF-8", 1168, 0xdd458106513e539a),
            ("schubert_avemaria.mid", "UTF-8", "UTF-8", 1426, 0x98ec118504301c92),
            ("Schubert_Standchen.mid", "UTF-8", "UTF-8", 783, 0x5d9f9d58ae3ac7fd),
            ("test.mid", "UTF-8", "MAC-CENTRALEUROPE", 47, 0x7af40e2cd7665f13),
            ("Negra_Sombra.kar", "UTF-8", "UTF-8", 883, 0x21bf8542ec18918a),
            ("twinkle.kar", "UTF-8", "ASCII", 648, 0x34e08aa2a95b567e),
        ];
        for (name, rust_charset, _cpp_charset, len, fnv) in DEFAULTS {
            let path = Path::new("examples").join(name);
            let mut seq = Sequence::default();
            assert!(seq.load_file(&path), "{name}: {:?}", seq.loading_errors);
            assert_eq!(seq.current_charset(), *rust_charset, "{name}");
            let mut cat = String::new();
            for rec in seq.text_events() {
                cat.push_str(&seq.decode_text(&rec.text));
            }
            assert_eq!((cat.len(), fnv1a64_str(&cat)), (*len, *fnv), "{name}");
        }
        // Verbatim anchors (not just hashes): the `test.mid` copyright the
        // C++ default mojibakes, and real KAR lyric lines.
        let mut seq = Sequence::default();
        assert!(seq.load_file(Path::new("examples/test.mid")));
        assert_eq!(
            seq.get_text(TextType::Copyright),
            vec!["Copyright (C) 2006-2024 Pedro López-Cabanillas".to_string()]
        );
        let mut seq = Sequence::default();
        assert!(seq.load_file(Path::new("examples/Negra_Sombra.kar")));
        // Lyrics ride as Text (type 1) in these files, with KAR `@`
        // headers remapped to Kar* types (oracle type census).
        assert!(seq.get_text(TextType::Text).join("").contains("\\Can"));
        assert_eq!(
            seq.get_text(TextType::KarTitles).first().map(String::as_str),
            Some("@TNegra Sombra")
        );
        assert_eq!(seq.type_max_points(), TextType::Text as i32);
        let mut seq = Sequence::default();
        assert!(seq.load_file(Path::new("examples/twinkle.kar")));
        assert!(seq.get_text(TextType::Text).join("").contains("\\Twin"));
        assert_eq!(
            seq.get_text(TextType::KarTitles),
            vec!["@TTwinkle, Twinkle, Little Star".to_string()]
        );
        assert!(!seq.get_text(TextType::KarFileType).is_empty());
        assert!(!seq.get_text(TextType::KarVersion).is_empty());
        assert!(!seq.get_text(TextType::KarInformation).is_empty());
        assert!(!seq.get_text(TextType::KarLanguage).is_empty());
    }

    #[test]
    fn a10_edge_bytes_decode_like_g_convert() {
        // The A10 edge range 0x80–0x9F plus real UTF-8 (`é`, `€`) — the
        // same lyric bytes as the oracle's `/tmp/t4_edge.mid` (builder
        // hash pinned by `edge_builder_matches_oracle_fixture`).
        let raw = b"edge:\x80\x81\x8d\x8f\x90\x9d\x9f \xc3\xa9\xe2\x82\xac";
        let mut seq = Sequence::default();
        // ISO-8859-1: true latin1 — C1 controls, never cp1252 glyphs.
        // Oracle CODEC row agrees: len 30, fnv 9492badd77d024c1.
        seq.set_current_charset("ISO-8859-1");
        let latin1 = seq.decode_text(raw);
        assert_eq!(
            latin1,
            "edge:\u{80}\u{81}\u{8d}\u{8f}\u{90}\u{9d}\u{9f} Ã©â\u{82}¬"
        );
        assert_eq!((latin1.len(), fnv1a64_str(&latin1)), (30, 0x9492badd77d024c1));
        // WINDOWS-1252: WHATWG maps the undefined C1 bytes to C1 controls
        // (no error), 0x80 → €, 0x9F → Ÿ. `g_convert` instead errors and
        // returns the 18 raw bytes — pinned divergence (a Rust `String`
        // cannot hold lone 0x80–0x9F bytes, so the raw fallback is
        // unrepresentable here; the lossy path covers the cases it can).
        seq.set_current_charset("WINDOWS-1252");
        let cp1252 = seq.decode_text(raw);
        assert_eq!(cp1252, "edge:€\u{81}\u{8d}\u{8f}\u{90}\u{9d}Ÿ Ã©â\u{201a}¬");
        assert_eq!((cp1252.len(), fnv1a64_str(&cp1252)), (32, 0xda59c9526fd98b13));
        // The A10 core: the two labels must never collapse into each other.
        assert_ne!(latin1, cp1252);
        seq.set_current_charset("ISO-8859-1");
        assert_eq!(seq.decode_text(b"\x80"), "\u{80}");
        seq.set_current_charset("WINDOWS-1252");
        assert_eq!(seq.decode_text(b"\x80"), "€");
        // ISO-8859-9: true latin5 (C1 controls + Turkish letters — the
        // iconv table, verified byte-for-byte), never windows-1254 glyphs.
        // Oracle CODEC row agrees: len 30, fnv 9492badd77d024c1.
        seq.set_current_charset("ISO-8859-9");
        let latin5 = seq.decode_text(raw);
        assert_eq!(latin5, latin1);
        assert_eq!((latin5.len(), fnv1a64_str(&latin5)), (30, 0x9492badd77d024c1));
        assert_eq!(seq.decode_text(b"\xd0\xdd\xde\xf0\xfd\xfe"), "ĞİŞğış");
        // Case-insensitive aliases reach the same tables.
        for alias in ["iso-8859-1", "LATIN1", "l1", "iso88591"] {
            seq.set_current_charset(alias);
            assert_eq!(seq.decode_text(b"\x80"), "\u{80}", "{alias}");
        }
        for alias in ["iso-8859-9", "LATIN5", "l5", "iso-ir-148"] {
            seq.set_current_charset(alias);
            assert_eq!(seq.decode_text(b"\x80"), "\u{80}", "{alias}");
            assert_eq!(seq.decode_text(b"\xfe"), "ş", "{alias}");
        }
        // Unknown names fall back to lossy UTF-8 (never panic, never empty
        // for non-empty input).
        seq.set_current_charset("NoSuchCodec-XYZ");
        assert_eq!(seq.decode_text("plain".as_bytes()), "plain");
        assert_eq!(seq.decode_text(b"\xff\xfe"), "\u{fffd}\u{fffd}");
        assert_eq!(seq.decode_text(b""), "");
    }

    #[test]
    fn edge_builder_matches_oracle_fixture() {
        // Byte-identical twin of the oracle's `/tmp/t4_edge.mid`: SMF with
        // the A10 edge lyric. Pins the builder bytes (len + FNV) and the
        // agreed oracle CODEC cells for this lyric.
        let lyric = b"edge:\x80\x81\x8d\x8f\x90\x9d\x9f \xc3\xa9\xe2\x82\xac";
        let mut track = vec![0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20];
        track.extend_from_slice(&[0x00, 0xFF, 0x05, lyric.len() as u8]);
        track.extend_from_slice(lyric);
        track.extend_from_slice(&[0x00, 0x90, 0x3C, 0x40, 0x83, 0x60, 0x80, 0x3C, 0x40]);
        track.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        let mut file = b"MThd".to_vec();
        file.extend_from_slice(&6u32.to_be_bytes());
        file.extend_from_slice(&0u16.to_be_bytes());
        file.extend_from_slice(&1u16.to_be_bytes());
        file.extend_from_slice(&120u16.to_be_bytes());
        file.extend_from_slice(b"MTrk");
        file.extend_from_slice(&(track.len() as u32).to_be_bytes());
        file.extend_from_slice(&track);
        let mut h = 14695981039346656037u64;
        for &b in &file {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        assert_eq!(file.len(), 64);
        assert_eq!(h, 0xc38412a36d65a7c7);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.mid");
        std::fs::write(&path, &file).unwrap();
        let mut seq = Sequence::default();
        assert!(seq.load_file(&path), "{:?}", seq.loading_errors);
        // Oracle-agreed cells for this lyric (the remaining 7 codecs are
        // the pinned lone-C1 divergences: `g_convert` returns the 18 raw
        // bytes where Rust maps controls or falls back to lossy UTF-8).
        for (codec, len, fnv) in [
            ("ISO-8859-1", 30, 0x9492badd77d024c1u64),
            ("ISO-8859-9", 30, 0x9492badd77d024c1u64),
            ("ISO-8859-2", 30, 0xb8054286ce3de3fdu64),
            ("ISO-8859-5", 30, 0xaec7c8d097b948a0u64),
            ("KOI8-R", 38, 0xfc67f4f5808780b4u64),
            ("WINDOWS-1251", 31, 0xfb0b9f16deec6e8eu64),
            ("MACINTOSH", 32, 0xdb78513c670f94d5u64),
        ] {
            let cat = decode_all(&mut seq, codec);
            assert_eq!((cat.len(), fnv1a64_str(&cat)), (len, fnv), "{codec}");
        }
    }

    #[test]
    fn charset_label_resolution_table() {
        // `Encoding::for_label` is the detector-name mapping (arch §3.4):
        // ASCII-case-insensitive, alias-tolerant like `g_convert`. One row
        // per vocabulary entry: the 14 dropdown names, chardetng/uchardet
        // spellings, legacy song-setting values, and the unknown-name
        // fallback. (The latin1/latin5 WHATWG folds are asserted via
        // decode behavior in `a10_edge_bytes_decode_like_g_convert`, since
        // `decode_text` intercepts those labels before `for_label`.)
        for (label, expected) in [
            ("UTF-8", Some("UTF-8")),
            ("ISO-8859-1", Some("windows-1252")), // fold: intercepted in decode_text
            ("WINDOWS-1252", Some("windows-1252")),
            ("SHIFT_JIS", Some("Shift_JIS")),
            ("GBK", Some("GBK")),
            ("BIG5", Some("Big5")),
            ("KOI8-R", Some("KOI8-R")),
            ("EUC-KR", Some("EUC-KR")),
            ("ISO-8859-2", Some("ISO-8859-2")),
            ("ISO-8859-5", Some("ISO-8859-5")),
            ("ISO-8859-9", Some("windows-1254")), // fold: intercepted in decode_text
            ("WINDOWS-1251", Some("windows-1251")),
            ("WINDOWS-1250", Some("windows-1250")),
            ("MACINTOSH", Some("macintosh")),
            ("Shift_JIS", Some("Shift_JIS")),
            ("Big5", Some("Big5")),
            ("windows-1252", Some("windows-1252")),
            ("utf-8", Some("UTF-8")),
            ("GB2312", Some("GBK")),
            ("x-mac-cyrillic", Some("x-mac-cyrillic")),
            ("EUC-JP", Some("EUC-JP")),
            ("MAC-CENTRALEUROPE", None), // legacy uchardet value → UTF-8 fallback
            ("NoSuchCodec-XYZ", None),
            ("", None),
        ] {
            let got = encoding_rs::Encoding::for_label(label.as_bytes()).map(|e| e.name());
            assert_eq!(got, expected, "{label}");
        }
    }

    #[test]
    fn extra_codec_names_are_the_verbatim_14() {
        assert_eq!(
            Sequence::extra_codec_names(),
            [
                "UTF-8",
                "ISO-8859-1",
                "WINDOWS-1252",
                "SHIFT_JIS",
                "GBK",
                "BIG5",
                "KOI8-R",
                "EUC-KR",
                "ISO-8859-2",
                "ISO-8859-5",
                "ISO-8859-9",
                "WINDOWS-1251",
                "WINDOWS-1250",
                "MACINTOSH",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn off_list_charset_still_decodes() {
        // Off-list rule: the engine keeps decoding with the detected name
        // even when it matches no dropdown entry (the index-0 display rule
        // lives in the T11 UI — see the `extra_codec_names` docs).
        assert!(!Sequence::extra_codec_names().contains(&"EUC-JP".to_string()));
        let mut seq = Sequence::default();
        seq.set_current_charset("EUC-JP");
        assert_eq!(seq.current_charset(), "EUC-JP");
        // あ U+3042 in EUC-JP is A4 A2.
        assert_eq!(seq.decode_text(b"\xa4\xa2"), "あ");
    }

    #[test]
    fn get_text_and_raw_text_filter_like_cpp() {
        let mut seq = Sequence::default();
        seq.add_meta_data(0, 1, 1, "t1".as_bytes());
        seq.add_meta_data(10, 1, 5, "la".as_bytes());
        seq.add_meta_data(20, 2, 5, "si".as_bytes());
        seq.add_meta_data(30, 2, 3, "Piano".as_bytes());
        assert_eq!(seq.get_text(TextType::Lyric), vec!["la".to_string(), "si".to_string()]);
        assert_eq!(seq.get_text(TextType::Text), vec!["t1".to_string()]);
        assert!(seq.get_text(TextType::Marker).is_empty());
        // `track <= 0` matches all tracks; `None` matches all types.
        assert_eq!(seq.get_raw_text(0, TextType::None).len(), 4);
        assert_eq!(seq.get_raw_text(-3, TextType::None).len(), 4);
        assert_eq!(
            seq.get_raw_text(1, TextType::None),
            vec![(0, b"t1".to_vec()), (10, b"la".to_vec())]
        );
        assert_eq!(seq.get_raw_text(0, TextType::Lyric).len(), 2);
        assert_eq!(seq.get_raw_text(2, TextType::Lyric), vec![(20, b"si".to_vec())]);
        assert!(seq.get_raw_text(9, TextType::None).is_empty());
    }

    #[test]
    fn kar_headers_all_six_map_case_sensitively() {
        let mut seq = Sequence::default();
        for (header, mapped) in [
            ("@KMIDI KARAOKE FILE", TextType::KarFileType),
            ("@V0100", TextType::KarVersion),
            ("@ITitle", TextType::KarInformation),
            ("@LEnglish", TextType::KarLanguage),
            ("@TSong", TextType::KarTitles),
            ("@Wwhatever", TextType::KarWhatever),
        ] {
            seq.add_meta_data(0, 1, 1, header.as_bytes());
            assert_eq!(seq.text_events().last().unwrap().text_type, mapped, "{header}");
        }
        // Lowercase `@k` and bare `@` keep the original meta type.
        seq.add_meta_data(0, 1, 5, b"@knope");
        assert_eq!(seq.text_events().last().unwrap().text_type, TextType::Lyric);
        seq.add_meta_data(0, 1, 5, b"@");
        assert_eq!(seq.text_events().last().unwrap().text_type, TextType::Lyric);
        // KAR-mapped records are stored but create no playable events.
        assert_eq!(seq.text_events().len(), 8);
        assert_eq!(seq.events.iter().filter(|ev| ev.kind == Kind::Text).count(), 2);
    }

    #[test]
    fn track_name_and_channel_label_follow_the_charset() {
        // Latin-1 `Caf\xe9` stored raw; the accessors decode with the
        // current charset (T3 raw-byte storage + T4 decode hook).
        let mut seq = Sequence::default();
        seq.add_meta_data(0, 1, 3, b"Caf\xe9");
        seq.channel_label[2] = b"Caf\xe9".to_vec();
        assert_eq!(seq.track_name(1), "Caf\u{fffd}"); // UTF-8 default
        assert_eq!(seq.channel_label(2), "Caf\u{fffd}");
        seq.set_current_charset("ISO-8859-1");
        assert_eq!(seq.track_name(1), "Café");
        assert_eq!(seq.channel_label(2), "Café");
        seq.set_current_charset("WINDOWS-1252");
        assert_eq!(seq.track_name(1), "Café");
    }

    #[test]
    fn detect_charset_defaults_and_detects() {
        // Empty sample keeps UTF-8 (C++ parity).
        let mut seq = Sequence::default();
        seq.detect_charset();
        assert_eq!(seq.current_charset(), "UTF-8");
        // Non-empty samples resolve through chardetng (pinned behavior).
        seq.feed_charset("plain ascii lyrics here".as_bytes());
        seq.detect_charset();
        assert_eq!(seq.current_charset(), "UTF-8");
        let mut seq = Sequence::default();
        seq.feed_charset("Grüße üäö — €".as_bytes());
        seq.detect_charset();
        assert_eq!(seq.current_charset(), "UTF-8");
    }
}
