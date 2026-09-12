//! Application message types (`docs/migration/architecture.md` §2.2, §2.4).
//!
//! T1 walking skeleton: full enum shapes per the architecture doc, as stubs.
//! `update()` wiring lands in T9, playlist/file flows in T10.

use std::path::PathBuf;
use std::time::Instant;

/// App-level messages: every transport / playlist / mix / lyrics / piano /
/// MIDI-setup / prefs / dialog / navigation event, plus engine feedback
/// (`Player`) and async results. T9 wires `update()`; T10 the file flows.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum Message {
    // --- transport ---
    Play,
    Pause,
    Stop,
    NextSong,
    PrevSong,
    BeatForward,
    BeatBackward,
    JumpToBar(u32),
    LoopToggled(bool),
    LoopSet { start: u32, end: u32 },
    RepeatModeChanged(RepeatMode),
    PositionSeek(f64),
    TempoChanged(u32),
    VolumeChanged(u32),
    PitchChanged(i32),

    // --- playlist ---
    OpenFiles(Vec<PathBuf>, bool /* replace */),
    PlaylistSelect(usize),
    PlaylistRemove(usize),
    PlaylistMoveUp(usize),
    PlaylistMoveDown(usize),
    PlaylistShuffle,
    PlaylistClear,
    PlaylistLoadDialog,
    PlaylistLoaded(Result<LoadedPlaylist, String>),
    PlaylistSaveDialog,
    PlaylistSaved(Result<PathBuf, String>),

    // --- per-channel mix ---
    ChannelMute(u8, bool),
    ChannelSolo(u8, bool),
    ChannelLock(u8, bool),
    ChannelPatch(u8, u8),
    ChannelLevel(u8, u32),
    ChannelName(u8, String),

    // --- lyrics ---
    LyricTrackFilter(Option<u32>),
    LyricTypeFilter(i32),
    LyricCodec(String),
    LyricsCopy,
    LyricsSaveDialog,
    LyricsSaved(Result<PathBuf, String>),
    LyricsPrint,

    // --- piano ---
    // NOTE: no per-channel visibility toggle — `m_pianoShow[]` is never
    // created in C++ (window.hpp:103); that would be a new feature (arch §1.7).
    PianoTighten(bool),
    PianoShowAll,
    PianoHideUnused,
    PianoPreviewOn(u8 /* ch */, u8 /* note */),
    PianoPreviewOff,

    // --- MIDI setup ---
    MidiBackendSelected(String),
    MidiPortsRefreshed(Vec<MidiPort>),
    MidiPortSelected(String),
    MidiAdvancedToggled(bool),
    MidiInstrumentMapChanged(u8),
    MidiResetChanged(u8),
    MidiSoundFontChanged(String),
    MidiSoundFontBrowse,
    MidiTestNote,
    MidiApply,

    // --- preferences (one per persisted key or grouped) ---
    PrefsChanged(SettingsState),

    // --- song settings ---
    SongSettingsSave,
    SongSettingsLoad,

    // --- dialogs / navigation ---
    ShowDialog(Dialog),
    CloseDialog,
    ShowFileInfo,
    ShowAbout,
    ShowHelp,
    ViewSelected(View),
    Toast(String),
    DismissToast,

    // --- engine → UI (replaces the 12 C++ callbacks) ---
    Player(PlayerEvent),
    // --- async ---
    Tick(Instant),
    WindowResized(u32, u32),
}

/// Engine → UI events: the 12 C++ player callbacks plus open/load/output
/// results. Produced by the engine thread (T7), consumed by `update()` (T9).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Opened(LoadedSong),
    LoadFailed {
        file: String,
        errors: Vec<String>,
    },
    Time {
        elapsed_ms: u64,
        ticks: i64,
    },
    Beat {
        bar: u32,
        beat: u32,
        max: u32,
    },
    NoteOn {
        ch: u8,
        note: u8,
        vel: u8,
    },
    NoteOff {
        ch: u8,
        note: u8,
    },
    Text {
        track: u32,
        text_type: i32,
        ticks: i64,
        bytes: Vec<u8>,
    },
    Program {
        ch: u8,
        patch: u8,
    },
    Tempo {
        tempo_us: f64,
    },
    KeySignature {
        track: u32,
        alterations: i8,
        minor: bool,
    },
    TimeSignature {
        bar: u32,
        num: u8,
        den: u8,
    },
    Started,
    Stopped,
    Finished,
    OutputChanged {
        backend: String,
        port: String,
        warning: Option<String>,
    },
}

/// UI → engine commands sent over the engine channel (arch §2.4).
/// The engine thread + channel land in T7.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum EngineCommand {
    Load(PathBuf),
    Play,
    Pause,
    Stop {
        reset: bool,
    },
    /// `SequencePlayer::resetPosition`: cursor + ticks + latest-beat reset.
    ResetPosition,
    SeekTicks(i64),
    JumpToBar(u32),
    BeatStep(i8),
    SetTempoFactor(f64),
    SetVolumeFactor(u32),
    SetPitchShift(i32),
    SetChannelVolume(u8, f64),
    SetMuted(u8, bool),
    SetLocked(u8, bool),
    SetPatch(u8, u8),
    SetLoop {
        enabled: bool,
        start: u32,
        end: u32,
    },
    SetDrumsChannel(u8),
    SetSysexReset(u8),
    SelectOutput {
        backend: String,
        port: String,
    },
    SetSoundFont(String),
    PreviewNote {
        ch: u8,
        note: u8,
        vel: u8,
    },
    PreviewOff,
    Shutdown,
}

/// Modal dialogs: PLAN D2–D12 in D-number order (Preferences=D2, MidiSetup=D3,
/// About=D4, Help=D5, FileInfo=D6, JumpToBar=D7, Loop=D8, PlaylistOpen=D9,
/// PlaylistSave=D10, SoundFontBrowse=D11, SaveLyrics=D12). Full dialog views
/// land in T11–T13. D1 (open files) and D13 (print lyrics) are deliberately
/// absent: D1 is a portal file-chooser `Task` whose results arrive as
/// `Message::OpenFiles`, and D13 routes through `Message::LyricsPrint` to
/// the portal/export print path — neither needs retained modal state here.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    Preferences,
    MidiSetup,
    About,
    Help,
    FileInfo,
    JumpToBar,
    Loop,
    PlaylistOpen,
    PlaylistSave,
    SoundFontBrowse,
    SaveLyrics,
}

/// Main-view switcher pages (arch §2.1; rhythm stays dashboard lamps).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Lyrics,
    Channels,
    Piano,
}

/// Repeat policy: off / repeat song / repeat playlist (T9).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    Song,
    Playlist,
}

/// Immutable song metadata snapshot for the UI (arch §2.1).
/// Full shape (labels, bars, duration, …) lands with T3/T7.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct LoadedSong {
    pub title: String,
}

/// Result of a playlist load: source path + items (T8/T10 fill this in).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct LoadedPlaylist {
    pub source: Option<PathBuf>,
    pub items: Vec<PathBuf>,
}

/// One MIDI output port (T6 fills in the full ALSA/FluidSynth shape).
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct MidiPort {
    pub id: String,
    pub label: String,
}

/// Mirrors C++ `AppSettings` (all 30 keys). Full struct lands in T8
/// (`config::settings`); this stub keeps `Message` compiling for T1.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct SettingsState {}
