//! `MidiEvent` value type and helpers — port of `src/midi/events.{hpp,cpp}`.
//!
//! PLAN T2, arch §1.2 (event row) and §5 (`core/event.rs`). Integer widths
//! mirror the C++ `int`/`int64_t` fields exactly: `channel`/`a`/`b`/`c` stay
//! `i32` so intermediate values can go negative before [`clamp_midi`], just
//! as in C++ (unsigned fields would underflow-panic in debug builds).
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use std::path::Path;

/// Number of MIDI channels (C++ `kMidiChannels`). `usize` so it can size
/// channel arrays and ranges; the only intentional type deviation.
pub const MIDI_CHANNELS: usize = 16;
/// Zero-based General MIDI drum channel, i.e. channel 10
/// (C++ `kGmDrumChannel`).
pub const GM_DRUM_CHANNEL: i32 = 9;

/// Status nibbles for channel events (C++ `kStatus*`).
pub const STATUS_NOTE_OFF: i32 = 0x80;
pub const STATUS_NOTE_ON: i32 = 0x90;
pub const STATUS_KEY_PRESSURE: i32 = 0xA0;
pub const STATUS_CONTROL_CHANGE: i32 = 0xB0;
pub const STATUS_PROGRAM_CHANGE: i32 = 0xC0;
pub const STATUS_CHANNEL_PRESSURE: i32 = 0xD0;
pub const STATUS_PITCH_BEND: i32 = 0xE0;

/// Controller numbers (C++ `kCc*`).
pub const CC_BANK_MSB: i32 = 0x00;
pub const CC_BANK_LSB: i32 = 0x20;
pub const CC_VOLUME: i32 = 0x07;
pub const CC_VOLUME_LSB: i32 = 0x27;
pub const CC_ALL_SOUNDS_OFF: i32 = 0x78;
pub const CC_RESET_CONTROLLERS: i32 = 0x79;
pub const CC_ALL_NOTES_OFF: i32 = 0x7B;

/// Text/meta-event flavor (C++ `TextType`; discriminants match 1:1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TextType {
    None = 0,
    Text = 1,
    Copyright = 2,
    TrackName = 3,
    InstrumentName = 4,
    Lyric = 5,
    Marker = 6,
    Cue = 7,
    KarFileType = 8,
    KarVersion = 9,
    KarInformation = 10,
    KarLanguage = 11,
    KarTitles = 12,
    KarWhatever = 13,
}

impl TextType {
    /// First text flavor (C++ `TextType::First`).
    pub const FIRST: TextType = TextType::Text;
    /// Last text flavor (C++ `TextType::Last`).
    pub const LAST: TextType = TextType::KarWhatever;
}

impl From<TextType> for i32 {
    fn from(value: TextType) -> i32 {
        value as i32
    }
}

/// Rejected `i32` for `TextType::try_from` (C++ `static_cast<TextType>` takes
/// any `int`; Rust makes the fallibility explicit for T4's meta-type mapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownTextType(pub i32);

impl std::fmt::Display for UnknownTextType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown TextType value {}", self.0)
    }
}

impl std::error::Error for UnknownTextType {}

impl TryFrom<i32> for TextType {
    type Error = UnknownTextType;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TextType::None),
            1 => Ok(TextType::Text),
            2 => Ok(TextType::Copyright),
            3 => Ok(TextType::TrackName),
            4 => Ok(TextType::InstrumentName),
            5 => Ok(TextType::Lyric),
            6 => Ok(TextType::Marker),
            7 => Ok(TextType::Cue),
            8 => Ok(TextType::KarFileType),
            9 => Ok(TextType::KarVersion),
            10 => Ok(TextType::KarInformation),
            11 => Ok(TextType::KarLanguage),
            12 => Ok(TextType::KarTitles),
            13 => Ok(TextType::KarWhatever),
            _ => Err(UnknownTextType(value)),
        }
    }
}

/// Event flavor (C++ `MidiEvent::Kind`; order matches 1:1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    NoteOn,
    NoteOff,
    KeyPressure,
    ControlChange,
    ProgramChange,
    ChannelPressure,
    PitchBend,
    SysEx,
    Text,
    Tempo,
    TimeSignature,
    KeySignature,
    Beat,
}

/// One sequenced event (C++ `MidiEvent`). All fields are public so T3 can
/// build events field-wise exactly as `sequence.cpp` does.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiEvent {
    pub kind: Kind,
    pub tick: i64,
    pub delta: i64,
    /// Track number (C++ `tag`).
    pub tag: i32,
    pub channel: i32,
    /// Note / controller / program / numerator / bar / text type (C++ `a`).
    pub a: i32,
    /// Velocity / value / denominator / beat / alterations (C++ `b`).
    pub b: i32,
    /// Extra payload: bar length / minor mode (C++ `c`).
    pub c: i32,
    /// Microseconds per quarter note (C++ `tempo`).
    pub tempo: f64,
    /// SysEx / text payload bytes (C++ `data`).
    pub data: Vec<u8>,
}

impl Default for MidiEvent {
    /// Mirrors C++ value-init (`MidiEvent e;`): zeroed fields, `kind` 0
    /// (i.e. `NoteOn`), `tempo` 500000.0.
    fn default() -> Self {
        MidiEvent {
            kind: Kind::NoteOn,
            tick: 0,
            delta: 0,
            tag: 0,
            channel: 0,
            a: 0,
            b: 0,
            c: 0,
            tempo: 500_000.0,
            data: Vec::new(),
        }
    }
}

impl MidiEvent {
    /// True for the seven channel-voice event kinds (C++ `isChannel()`).
    pub fn is_channel(&self) -> bool {
        matches!(
            self.kind,
            Kind::NoteOn
                | Kind::NoteOff
                | Kind::KeyPressure
                | Kind::ControlChange
                | Kind::ProgramChange
                | Kind::ChannelPressure
                | Kind::PitchBend
        )
    }

    /// True for SysEx/meta/beat events (C++ `isMeta()`).
    pub fn is_meta(&self) -> bool {
        !self.is_channel()
    }

    /// MIDI status nibble for channel events, `0` for meta events
    /// (C++ `status()`).
    pub fn status(&self) -> i32 {
        match self.kind {
            Kind::NoteOff => STATUS_NOTE_OFF,
            Kind::NoteOn => STATUS_NOTE_ON,
            Kind::KeyPressure => STATUS_KEY_PRESSURE,
            Kind::ControlChange => STATUS_CONTROL_CHANGE,
            Kind::ProgramChange => STATUS_PROGRAM_CHANGE,
            Kind::ChannelPressure => STATUS_CHANNEL_PRESSURE,
            Kind::PitchBend => STATUS_PITCH_BEND,
            _ => 0,
        }
    }
}

/// Note-on event (C++ `makeNoteOn`).
pub fn make_note_on(channel: i32, note: i32, velocity: i32) -> MidiEvent {
    MidiEvent {
        kind: Kind::NoteOn,
        channel,
        a: note,
        b: velocity,
        ..Default::default()
    }
}

/// Note-off event (C++ `makeNoteOff`).
pub fn make_note_off(channel: i32, note: i32, velocity: i32) -> MidiEvent {
    MidiEvent {
        kind: Kind::NoteOff,
        channel,
        a: note,
        b: velocity,
        ..Default::default()
    }
}

/// Control-change event (C++ `makeControl`).
pub fn make_control(channel: i32, cc: i32, value: i32) -> MidiEvent {
    MidiEvent {
        kind: Kind::ControlChange,
        channel,
        a: cc,
        b: value,
        ..Default::default()
    }
}

/// Program-change event (C++ `makeProgram`).
pub fn make_program(channel: i32, program: i32) -> MidiEvent {
    MidiEvent {
        kind: Kind::ProgramChange,
        channel,
        a: program,
        ..Default::default()
    }
}

/// Tempo event (C++ `makeTempo`).
pub fn make_tempo(microseconds_per_quarter: f64) -> MidiEvent {
    MidiEvent {
        kind: Kind::Tempo,
        tempo: microseconds_per_quarter,
        ..Default::default()
    }
}

/// Synthesized bar/beat event (C++ `makeBeat`).
pub fn make_beat(bar: i32, beat: i32, beats_per_bar: i32) -> MidiEvent {
    MidiEvent {
        kind: Kind::Beat,
        a: bar,
        b: beat,
        c: beats_per_bar,
        ..Default::default()
    }
}

/// Beats per minute for a tempo in microseconds per quarter note
/// (C++ `tempoToBpm`): 120.0 for non-positive input. NaN input yields NaN,
/// as in C++ (`NaN <= 0` is false on both sides).
pub fn tempo_to_bpm(tempo_us: f64) -> f64 {
    if tempo_us <= 0.0 {
        120.0
    } else {
        60_000_000.0 / tempo_us
    }
}

/// Clamp to the 7-bit MIDI range (C++ `clampMidi`).
pub fn clamp_midi(value: i32) -> i32 {
    value.clamp(0, 127)
}

/// True for `.mid` / `.midi` / `.kar` / `.rmi` / `.wrk`, case-insensitive
/// (C++ `isSupportedMidiFile`).
///
/// Matches the C++ comparison byte for byte: the file name is the bytes
/// after the last `/` (empty for trailing-slash paths, which are therefore
/// rejected), the extension runs from the *last* dot of the file name to
/// the end (dot included), is empty when there is no dot, the file name is
/// `.` / `..`, or the last dot is the first character (dotfiles such as
/// `.mid`, which are therefore rejected), and is compared ASCII-
/// case-insensitively with no trimming (`song.MID ` is rejected). Matching
/// runs on the raw path bytes, so non-UTF-8 names behave as in C++.
pub fn is_supported_midi_file<P: AsRef<Path>>(path: P) -> bool {
    // Raw bytes after the last `/`, mirroring
    // `std::filesystem::path::filename()` on POSIX. (`Path::file_name()`
    // cannot be used: it strips trailing slashes, while C++ reports an
    // empty file name — and empty extension — for them.)
    let bytes = path.as_ref().as_os_str().as_encoded_bytes();
    let file_name = match bytes.iter().rposition(|&b| b == b'/') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    };
    if file_name.is_empty() || file_name == b"." || file_name == b".." {
        return false;
    }
    let dot = match file_name.iter().rposition(|&b| b == b'.') {
        None | Some(0) => return false,
        Some(i) => i,
    };
    let ext = &file_name[dot..];
    ext.eq_ignore_ascii_case(b".mid")
        || ext.eq_ignore_ascii_case(b".midi")
        || ext.eq_ignore_ascii_case(b".kar")
        || ext.eq_ignore_ascii_case(b".rmi")
        || ext.eq_ignore_ascii_case(b".wrk")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn note_on_factory_maps_fields() {
        let ev = make_note_on(3, 60, 100);
        assert_eq!(ev.kind, Kind::NoteOn);
        assert_eq!(ev.channel, 3);
        assert_eq!((ev.a, ev.b, ev.c), (60, 100, 0));
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert!((ev.tempo - 500_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn note_off_factory_maps_fields() {
        let ev = make_note_off(9, 36, 64);
        assert_eq!(ev.kind, Kind::NoteOff);
        assert_eq!(ev.channel, 9);
        assert_eq!((ev.a, ev.b, ev.c), (36, 64, 0));
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert!((ev.tempo - 500_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn control_factory_maps_fields() {
        let ev = make_control(0, CC_VOLUME, 100);
        assert_eq!(ev.kind, Kind::ControlChange);
        assert_eq!(ev.channel, 0);
        assert_eq!((ev.a, ev.b, ev.c), (0x07, 100, 0));
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert!((ev.tempo - 500_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn program_factory_maps_fields() {
        let ev = make_program(5, 42);
        assert_eq!(ev.kind, Kind::ProgramChange);
        assert_eq!(ev.channel, 5);
        assert_eq!((ev.a, ev.b, ev.c), (42, 0, 0));
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert!((ev.tempo - 500_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn tempo_factory_maps_fields() {
        let ev = make_tempo(250_000.0);
        assert_eq!(ev.kind, Kind::Tempo);
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert_eq!((ev.channel, ev.a, ev.b, ev.c), (0, 0, 0, 0));
        assert!((ev.tempo - 250_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn beat_factory_maps_fields() {
        let ev = make_beat(7, 3, 4);
        assert_eq!(ev.kind, Kind::Beat);
        assert_eq!((ev.a, ev.b, ev.c), (7, 3, 4));
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert_eq!(ev.channel, 0);
        assert!((ev.tempo - 500_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn default_matches_cpp_value_init() {
        // C++ `MidiEvent e;` value-initializes: kind 0 == NoteOn, tempo 500000.
        let ev = MidiEvent::default();
        assert_eq!(ev.kind, Kind::NoteOn);
        assert_eq!((ev.tick, ev.delta, ev.tag), (0, 0, 0));
        assert_eq!((ev.channel, ev.a, ev.b, ev.c), (0, 0, 0, 0));
        assert!((ev.tempo - 500_000.0).abs() < f64::EPSILON);
        assert!(ev.data.is_empty());
    }

    #[test]
    fn channel_predicates_cover_all_kinds() {
        let table = [
            (Kind::NoteOn, true),
            (Kind::NoteOff, true),
            (Kind::KeyPressure, true),
            (Kind::ControlChange, true),
            (Kind::ProgramChange, true),
            (Kind::ChannelPressure, true),
            (Kind::PitchBend, true),
            (Kind::SysEx, false),
            (Kind::Text, false),
            (Kind::Tempo, false),
            (Kind::TimeSignature, false),
            (Kind::KeySignature, false),
            (Kind::Beat, false),
        ];
        assert_eq!(table.len(), 13);
        for (kind, channel) in table {
            let ev = MidiEvent {
                kind,
                ..Default::default()
            };
            assert_eq!(ev.is_channel(), channel, "{kind:?}");
            assert_eq!(ev.is_meta(), !channel, "{kind:?}");
        }
    }

    #[test]
    fn status_bytes_cover_all_kinds() {
        let table = [
            (Kind::NoteOff, STATUS_NOTE_OFF),
            (Kind::NoteOn, STATUS_NOTE_ON),
            (Kind::KeyPressure, STATUS_KEY_PRESSURE),
            (Kind::ControlChange, STATUS_CONTROL_CHANGE),
            (Kind::ProgramChange, STATUS_PROGRAM_CHANGE),
            (Kind::ChannelPressure, STATUS_CHANNEL_PRESSURE),
            (Kind::PitchBend, STATUS_PITCH_BEND),
            (Kind::SysEx, 0),
            (Kind::Text, 0),
            (Kind::Tempo, 0),
            (Kind::TimeSignature, 0),
            (Kind::KeySignature, 0),
            (Kind::Beat, 0),
        ];
        assert_eq!(table.len(), 13);
        for (kind, status) in table {
            let ev = MidiEvent {
                kind,
                ..Default::default()
            };
            assert_eq!(ev.status(), status, "{kind:?}");
        }
    }

    #[test]
    fn tempo_to_bpm_values() {
        for (tempo_us, bpm) in [
            (500_000.0, 120.0),
            (1_000_000.0, 60.0),
            (250_000.0, 240.0),
            (0.0, 120.0),
            (-1.0, 120.0),
            (-500_000.0, 120.0),
        ] {
            assert!(
                (tempo_to_bpm(tempo_us) - bpm).abs() < 1e-9,
                "tempo {tempo_us}"
            );
        }
        assert!(tempo_to_bpm(f64::NAN).is_nan());
    }

    #[test]
    fn clamp_midi_bounds() {
        assert_eq!(clamp_midi(-1), 0);
        assert_eq!(clamp_midi(-1000), 0);
        assert_eq!(clamp_midi(i32::MIN), 0);
        assert_eq!(clamp_midi(0), 0);
        assert_eq!(clamp_midi(1), 1);
        assert_eq!(clamp_midi(64), 64);
        assert_eq!(clamp_midi(126), 126);
        assert_eq!(clamp_midi(127), 127);
        assert_eq!(clamp_midi(128), 127);
        assert_eq!(clamp_midi(1000), 127);
        assert_eq!(clamp_midi(i32::MAX), 127);
    }

    #[test]
    fn supported_extensions_matrix() {
        let accepted = [
            "song.mid",
            "song.midi",
            "song.kar",
            "song.rmi",
            "song.wrk",
            "SONG.MID",
            "song.MIDI",
            "song.KAR",
            "song.RMI",
            "song.WRK",
            "song.Mid",
            "song.mId",
            "song.MiDi",
            "song.KaR",
            "song.rMi",
            "song.WrK",
            // Multi-dot names match on the last extension, as in C++.
            "archive.tar.mid",
            "a.b.c.WRK",
            "song..mid",
            "..mid",
            ".mid.mid",
            // Directories and leading spaces don't affect the extension.
            "/tmp/Music/Song.KAR",
            "dir.mid/song.rmi",
            " song.mid",
        ];
        for name in accepted {
            assert!(is_supported_midi_file(name), "should accept {name}");
        }
        // Generic over path-like types, not just &str.
        assert!(is_supported_midi_file(PathBuf::from("song.MID")));
        let owned = "song.kar".to_string();
        assert!(is_supported_midi_file(&owned));
        assert!(is_supported_midi_file(Path::new("song.wrk")));
    }

    #[test]
    fn unsupported_names_rejected() {
        let rejected = [
            "song.wav",
            "song.txt",
            "song.mp3",
            // `.lst` is handled separately in C++ (window.cpp `openFiles`
            // checks `g_str_has_suffix(p, ".lst")`), never by this filter.
            "song.lst",
            "song.syx",
            "songmid",
            "mid",
            "",
            ".",
            "..",
            // Dotfiles: a leading dot alone is not an extension (C++ parity).
            ".mid",
            ".MID",
            ".kar",
            ".profile",
            // Trailing-dot / dot-only extensions.
            "song.",
            "song..",
            "...",
            "a.",
            "..a",
            // No trimming, no partial matches.
            "song.MID ",
            "song.mid ",
            "song.mid.wav",
            "x.MID.mid2",
            "song.m idi",
            // A dotted directory is not an extension; trailing slashes and
            // extensionless basenames have none.
            "dir.mid/file",
            "dir.mid/",
            "dir.mid//",
            "song.mid/",
            "/",
            "foo/.",
            "foo/..",
        ];
        for name in rejected {
            assert!(!is_supported_midi_file(name), "should reject {name}");
        }
        assert!(!is_supported_midi_file(PathBuf::from("song.wav")));
    }

    #[test]
    fn dot_edge_cases_match_cpp_filesystem() {
        // Pinned against libstdc++ `std::filesystem::path::extension()`:
        // a leading dot alone never forms an extension, but a second dot does.
        assert!(!is_supported_midi_file(".mid")); // ext ""
        assert!(is_supported_midi_file("..mid")); // ext ".mid"
        assert!(is_supported_midi_file(".mid.mid")); // ext ".mid"
        assert!(!is_supported_midi_file("...")); // ext "."
        assert!(!is_supported_midi_file("song.")); // ext "."
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_names_match_on_raw_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let supported = OsString::from_vec(b"song\xff.MID".to_vec());
        assert!(is_supported_midi_file(&supported));
        let other = OsString::from_vec(b"song\xff.wav".to_vec());
        assert!(!is_supported_midi_file(&other));
    }

    #[test]
    fn text_type_discriminants_and_aliases() {
        let table = [
            (TextType::None, 0),
            (TextType::Text, 1),
            (TextType::Copyright, 2),
            (TextType::TrackName, 3),
            (TextType::InstrumentName, 4),
            (TextType::Lyric, 5),
            (TextType::Marker, 6),
            (TextType::Cue, 7),
            (TextType::KarFileType, 8),
            (TextType::KarVersion, 9),
            (TextType::KarInformation, 10),
            (TextType::KarLanguage, 11),
            (TextType::KarTitles, 12),
            (TextType::KarWhatever, 13),
        ];
        assert_eq!(table.len(), 14);
        for (text_type, value) in table {
            assert_eq!(i32::from(text_type), value, "{text_type:?}");
            assert_eq!(text_type as i32, value, "{text_type:?}");
        }
        assert_eq!(TextType::FIRST, TextType::Text);
        assert_eq!(TextType::LAST, TextType::KarWhatever);
    }

    #[test]
    fn text_type_try_from_round_trip() {
        for value in 0..=13 {
            let text_type = TextType::try_from(value).unwrap();
            assert_eq!(i32::from(text_type), value);
        }
        for value in [-1, 14, 99, 127, i32::MIN, i32::MAX] {
            assert_eq!(TextType::try_from(value), Err(UnknownTextType(value)));
        }
    }

    #[test]
    fn constants_match_cpp_headers() {
        assert_eq!(MIDI_CHANNELS, 16);
        assert_eq!(GM_DRUM_CHANNEL, 9);
        assert_eq!(
            [
                STATUS_NOTE_OFF,
                STATUS_NOTE_ON,
                STATUS_KEY_PRESSURE,
                STATUS_CONTROL_CHANGE,
                STATUS_PROGRAM_CHANGE,
                STATUS_CHANNEL_PRESSURE,
                STATUS_PITCH_BEND,
            ],
            [0x80, 0x90, 0xA0, 0xB0, 0xC0, 0xD0, 0xE0]
        );
        assert_eq!(
            [
                CC_BANK_MSB,
                CC_BANK_LSB,
                CC_VOLUME,
                CC_VOLUME_LSB,
                CC_ALL_SOUNDS_OFF,
                CC_RESET_CONTROLLERS,
                CC_ALL_NOTES_OFF,
            ],
            [0x00, 0x20, 0x07, 0x27, 0x78, 0x79, 0x7B]
        );
    }
}
