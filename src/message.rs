//! Application message types (`docs/migration/architecture.md` §2.2, §2.4).
//!
//! T9: full `Message` enum plus the UI snapshot types (`LoadedSong`,
//! `LoadedPlaylist`, `SettingsState`, …). The engine protocol
//! (`EngineCommand` / `PlayerEvent`) is **owned by `core::player` and only
//! re-exported here** (DECISIONS D19): one definition, no bridge `From`
//! impls, no drift. Consequences of that ownership:
//! * `PlayerEvent::NoteOff` carries `vel` (C++ `onNoteOff` parity).
//! * `PreviewNote`/`PreviewOff` ARE engine commands (T13c D19 refinement):
//!   piano audition bypasses the player MIX (A3 — no mute/pitch/sequencing
//!   touch), but the outputs live in the engine thread, so "app-only" can
//!   only mean "bypasses player mix", not "no engine command". The
//!   `PianoPreviewOn/Off` / test-note arms in `app.rs` send them; the
//!   engine handlers write straight to the current output.

use std::path::PathBuf;
use std::time::Instant;

use cosmic::iced::keyboard::Event as KeyboardEvent;
use cosmic::widget::color_picker::ColorPickerUpdate;

use crate::core::event::MIDI_CHANNELS;
pub use crate::core::output::MidiPort;
pub use crate::core::player::{EngineCommand, PlayerEvent, SongSnapshot};
use crate::core::sequence::TextRec;

/// App-level messages: every transport / playlist / mix / lyrics / piano /
/// MIDI-setup / prefs / dialog / navigation event, plus engine feedback
/// (`Player`) and async results. Handled by `App::update` (`app.rs`).
#[allow(dead_code)]
// `PrefsChanged` carries the whole 30-key `Settings` by value per arch §2.2
// (one message per dialog close — boxing would churn the contract).
#[allow(clippy::large_enum_variant)]
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
    LoopSet {
        start: u32,
        end: u32,
    },
    RepeatModeChanged(RepeatMode),
    PositionSeek(f64),
    TempoChanged(u32),
    VolumeChanged(u32),
    PitchChanged(i32),

    // --- playlist ---
    OpenFiles(Vec<PathBuf>, bool /* replace */),
    /// D1 trigger (the Open button/menu/Add action): launches the portal
    /// multi-select chooser; results arrive as `OpenFilesChosen`.
    OpenFilesDialog,
    /// D1 chooser result: `Ok(paths)` feeds the `OpenFiles(paths, true)`
    /// path; `Ok(empty)` is unreachable from the portal but stays a silent
    /// no-op defensively; `Err` (I/O failure or non-file URL) toasts.
    OpenFilesChosen(Result<Vec<PathBuf>, String>),
    PlaylistSelect(usize),
    /// Sidebar row-activated (double-click/Enter, U11): select + load + play.
    PlaylistRowActivated(usize),
    PlaylistRemove(usize),
    PlaylistMoveUp(usize),
    PlaylistMoveDown(usize),
    PlaylistShuffle,
    PlaylistClear,
    PlaylistLoadDialog,
    PlaylistLoaded(Result<LoadedPlaylist, String>),
    PlaylistSaveDialog,
    PlaylistSaved(Result<PathBuf, String>),
    /// Shared portal-choosers cancel signal: `Error::Cancelled` maps here
    /// from every file-chooser `Task` (D1/D9/D10) and is a silent no-op —
    /// no state change, no toast.
    ChooserCancelled,

    // --- drag-and-drop (T0 verdict, arch §2.3; chooser stays primary) ---
    /// X11: `window::Event::FileDropped` paths from the event subscription.
    FilesDropped(Vec<PathBuf>),
    /// Wayland: raw payload from `dnd_destination.on_data_received`; only
    /// `text/uri-list` is parsed (same Url→PathBuf rules as the chooser).
    DndDataReceived {
        mime: String,
        data: Vec<u8>,
    },
    /// Wayland sandboxed drop: portal file-transfer key from
    /// `dnd_destination.on_file_transfer`; resolves via
    /// `cosmic::command::file_transfer_receive`.
    DndFileTransfer(String),
    /// Resolved sandboxed drop: `Ok(paths)` feeds the drop path, `Err` toasts.
    DndFileTransferDone(Result<Vec<PathBuf>, String>),

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
    /// D13 export result: `Ok` is silent (C++ shows the native print dialog
    /// and toasts nothing on success); `Err` (temp-write or `xdg-open`
    /// spawn failure) toasts. A dedicated variant — every `Task` must
    /// resolve to a `Message`, and neither existing silent carrier fits:
    /// `ChooserCancelled` is scoped to file-chooser cancel, and `Toast`
    /// would add a success toast C++ never shows.
    LyricsPrinted(Result<(), String>),

    // --- piano ---
    // NOTE: no per-channel visibility toggle — `m_pianoShow[]` is never
    // created in C++ (window.hpp:103); that would be a new feature (arch §1.7).
    // Preview bypasses the player mix (A3): the arms light/clear the matrix
    // AND send the engine-carried `PreviewNote`/`PreviewOff` raw writes
    // (D19 refinement — the outputs live engine-side, so the bypass still
    // crosses the thread, touching no mute/pitch/sequencing state).
    PianoTighten(bool),
    PianoShowAll,
    PianoHideUnused,
    PianoPreviewOn(u8 /* ch */, u8 /* note */),
    PianoPreviewOff,

    // --- MIDI setup ---
    MidiBackendSelected(String),
    /// D3 refill completion. `backend`/`advanced` echo the requesting
    /// scratch values (T13g F4): the arm drops arrivals whose echo no
    /// longer matches, so a late task from a superseded backend switch
    /// cannot clobber the current port list.
    MidiPortsRefreshed {
        backend: String,
        advanced: bool,
        ports: Vec<MidiPort>,
    },
    MidiPortSelected(String),
    MidiAdvancedToggled(bool),
    MidiInstrumentMapChanged(u8),
    MidiResetChanged(u8),
    MidiSoundFontChanged(String),
    MidiSoundFontBrowse,
    /// D11 chooser result: `Ok(path)` commits the path into the D3 scratch
    /// entry (deferred — Test/Apply carry it to the engine); `Err` (non-file
    /// URL or I/O failure) toasts. Portal cancel maps to the silent
    /// `ChooserCancelled`, never here.
    MidiSoundFontChosen(Result<PathBuf, String>),
    MidiTestNote,
    /// Test-note release (the `MidiTestNote` arm's 400 ms sleep resolves
    /// here): raw NoteOff via `PreviewOff`, like the C++ `g_timeout`
    /// (`window.cpp:1148-1153` — no idle gate, unlike piano). `epoch`
    /// captures the output selection at NoteOn time (the C++ callback
    /// captures the `MidiOutput*` itself): the arm sends the off only
    /// when no reselect landed in between, so it always addresses the
    /// output the NoteOn went to. A stale capture means a stop-first
    /// reselect (Apply/prefs/test) already silenced the old output, or a
    /// post-failure fallback reselected an output that never sounded —
    /// either way the off is skipped, never misfired at the new current.
    MidiTestNoteOff {
        ch: u8,
        note: u8,
        epoch: u64,
    },
    MidiApply,

    // --- D7/D8 dialog scratch (spin edits ride these, prefs `on_scratch`
    // precedent — per-edit `JumpToBar`/`LoopSet` would jump/apply-on-step
    // and is banned; the views clamp for display, the confirm arms clamp
    // via `clamp_jump_bar`/`loop_confirm`) ---
    JumpBarEdited(u32),
    LoopFromEdited(u32),
    LoopToEdited(u32),

    // --- D4 about links ---
    /// Open a URL with the system opener (`xdg-open`, D6 choice (a) like
    /// D13 — in-sandbox it forwards through the OpenURI portal, so no
    /// finish-args are needed). Resolves as `UrlOpened`.
    OpenUrl(String),
    /// Open-URI result: `Ok` is silent (the viewer owns everything after
    /// the detached spawn); `Err` (spawn failure) toasts.
    UrlOpened(Result<(), String>),

    // --- preferences (commit-on-close scratch state, applied + saved by
    // `App::apply_prefs`, incl. solo% per U14) ---
    PrefsChanged(SettingsState),
    /// Whole-scratch prefs edit (T12b shell): adopted scratch-only via
    /// `adopt_prefs_scratch` — never applied, never saved. Per-edit
    /// `PrefsChanged` would be commit-on-change and is banned; the single
    /// `PrefsChanged(merged)` of the narrowed 17-key commit fires once at dialog close.
    PrefsScratchEdited(SettingsState),
    /// D17 picker updates for the three D2 Lyrics color rows (T12b shell):
    /// one constructor per row — the picker builder takes `fn` pointers, so
    /// the row rides the variant, not a payload. Routed to
    /// `apply_picker_update` (scratch-only like any edit).
    PrefsPickerFuture(ColorPickerUpdate),
    PrefsPickerPast(ColorPickerUpdate),
    PrefsPickerHighlight(ColorPickerUpdate),

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

    // --- keyboard (T14d composition over `state::keys::resolve_key`) ---
    /// Ignored-status keyboard event from `keyboard::listen()` (a focused
    /// button consumes Enter and a focused text input consumes text keys,
    /// so those never arrive here and cannot double-fire). The arm resolves
    /// it against the live dialog/selection context and routes via
    /// `KeyAction::message` (`FocusNext`/`FocusPrevious` return focus
    /// `Task`s, `Quit` routes to the `Quit` arm below).
    KeyEvent(KeyboardEvent),
    // --- app lifecycle ---
    /// `app.quit` (`window.cpp:1375-1376`): close the main window (the
    /// `<Ctrl>q` accelerator's target and the File-menu Quit entry's
    /// message — the T14a "only gap", adopted here).
    Quit,

    // --- engine → UI (replaces the 12 C++ callbacks) ---
    Player(PlayerEvent),
    // --- async ---
    Tick(Instant),
    WindowResized(u32, u32),
}

/// Modal dialogs: PLAN D2–D12 in D-number order (Preferences=D2, MidiSetup=D3,
/// About=D4, Help=D5, FileInfo=D6, JumpToBar=D7, Loop=D8, PlaylistOpen=D9,
/// PlaylistSave=D10, SoundFontBrowse=D11, SaveLyrics=D12). Full dialog views
/// land in T11–T13. D1 (open files) and D13 (print lyrics) are deliberately
/// absent: D1 is a portal file-chooser `Task` (`Message::OpenFilesDialog`
/// trigger, results via `Message::OpenFilesChosen` into the `OpenFiles`
/// path), and D13 routes through `Message::LyricsPrint` to the portal/export
/// print path — neither needs retained modal state here.
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

/// Transport status line: the 5 C++ states verbatim (ux §3.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Ready,
    Playing,
    Paused,
    Stopped,
    Finished,
}

impl Status {
    /// Status-line text, exactly the C++ `setStatus` strings.
    pub fn text(self) -> &'static str {
        match self {
            Status::Ready => "Ready",
            Status::Playing => "Playing",
            Status::Paused => "Paused",
            Status::Stopped => "Stopped",
            Status::Finished => "Finished",
        }
    }
}

/// Repeat policy: off / repeat song / repeat playlist (persisted to
/// `repeatMode`: `Off = 0`, `Song = 1`, `Playlist = 2`, like `m_repeat`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    Song,
    Playlist,
}

impl RepeatMode {
    /// `m_repeat` integer twin (window.cpp:2140-2152, `repeatMode` key).
    pub fn as_i32(&self) -> i32 {
        match self {
            RepeatMode::Off => 0,
            RepeatMode::Song => 1,
            RepeatMode::Playlist => 2,
        }
    }

    /// Inverse of [`RepeatMode::as_i32`]; out-of-range reads as `Off`.
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => RepeatMode::Song,
            2 => RepeatMode::Playlist,
            _ => RepeatMode::Off,
        }
    }
}

/// Immutable song metadata snapshot for the UI (arch §2.1): the engine's
/// [`SongSnapshot`] converted field-for-field at `Opened` time. The UI never
/// touches the engine's `Sequence` directly.
#[allow(dead_code)] // T11 lyrics body + T13 file-info/piano consume the rest.
#[derive(Clone, Debug)]
pub struct LoadedSong {
    pub title: String,
    pub format: String,
    /// Last bar number (`Sequence::last_bar`); jump/loop ranges clamp to it.
    pub bars: i32,
    pub len_ticks: i64,
    pub duration_ms: u64,
    pub division: i32,
    pub num_tracks: i32,
    /// Raw first tempo in µs per quarter (unscaled by any tempo factor).
    pub initial_tempo_us: f64,
    pub channel_used: [bool; MIDI_CHANNELS],
    pub channel_labels: [String; MIDI_CHANNELS],
    pub lowest_note: i32,
    pub highest_note: i32,
    /// Detected charset at load time; the lyrics codec override starts here.
    pub charset: String,
    /// Raw text/meta records for the lyrics view (T11 decodes + filters).
    pub texts: Vec<TextRec>,
    /// RIFF INFO `(key, value)` pairs in map order (D6 file-info dialog).
    pub metadata: Vec<(String, String)>,
    /// Decoded copyright records in event order (D6; same bytes the view
    /// decodes from `texts` + `charset`).
    pub copyrights: Vec<String>,
}

impl From<SongSnapshot> for LoadedSong {
    fn from(s: SongSnapshot) -> Self {
        Self {
            title: s.title,
            format: s.format,
            bars: s.bars,
            len_ticks: s.len_ticks,
            duration_ms: s.duration_ms,
            division: s.division,
            num_tracks: s.num_tracks,
            initial_tempo_us: s.initial_tempo_us,
            channel_used: s.channel_used,
            channel_labels: s.channel_labels,
            lowest_note: s.lowest_note,
            highest_note: s.highest_note,
            charset: s.charset,
            texts: s.texts,
            metadata: s.metadata,
            copyrights: s.copyrights,
        }
    }
}

impl LoadedSong {
    /// Last-bar clamp for the jump dialog range `1..=last_bar` (D7: min 1).
    pub fn last_bar(&self) -> u32 {
        self.bars.max(1) as u32
    }
}

/// Result of a playlist load: source path + items (T10 fills this in via the
/// portal chooser; the `PlaylistLoaded` arm applies it).
#[derive(Clone, Debug)]
pub struct LoadedPlaylist {
    pub source: Option<PathBuf>,
    pub items: Vec<PathBuf>,
}

/// Preferences scratch state: the full [`Settings`][crate::config::settings::Settings]
/// (all 30 keys), edited by the prefs dialog; at close the 17 D2-owned keys
/// commit over live settings ([`Settings::commit_prefs_dialog`][crate::config::settings::Settings::commit_prefs_dialog],
/// T12 devil-BLOCKING FIX) and the merged commit goes through ONE
/// `App::apply_prefs` (commit-on-close, U14). A type alias — not a
/// parallel struct — so the dialog can never drift from the persisted schema.
pub type SettingsState = crate::config::settings::Settings;

#[cfg(test)]
mod tests {
    use super::*;

    /// D19: the re-exported protocol types ARE the core types — a core value
    /// needs no conversion to travel as a `Message`.
    #[test]
    fn protocol_types_are_core_owned() {
        let cmd: EngineCommand = crate::core::player::EngineCommand::Play;
        assert!(matches!(cmd, EngineCommand::Play));
        let ev: PlayerEvent = crate::core::player::PlayerEvent::Started;
        let msg = Message::Player(ev);
        assert!(matches!(msg, Message::Player(PlayerEvent::Started)));
        let port: MidiPort = crate::core::output::MidiPort {
            id: "128:0".to_string(),
            label: "Synth".to_string(),
        };
        let msg = Message::MidiPortsRefreshed {
            backend: "ALSA".to_string(),
            advanced: false,
            ports: vec![port],
        };
        assert!(matches!(msg, Message::MidiPortsRefreshed { .. }));
    }

    /// `NoteOff` carries `vel` (C++ `onNoteOff(ch, note, vel)` parity —
    /// the T1 stub dropped it, D19 restores it).
    #[test]
    fn note_off_carries_velocity() {
        match (PlayerEvent::NoteOff {
            ch: 0,
            note: 60,
            vel: 80,
        }) {
            PlayerEvent::NoteOff { ch, note, vel } => {
                assert_eq!((ch, note, vel), (0, 60, 80));
            }
            other => panic!("expected NoteOff, got {other:?}"),
        }
    }

    /// `run_single_instance` bound (D16): `Message: Clone + Debug + Send`.
    #[test]
    fn message_is_clone_debug_send_static() {
        fn assert_bounds<T: Clone + std::fmt::Debug + Send + 'static>() {}
        assert_bounds::<Message>();
    }

    #[test]
    fn status_texts_match_cpp_set_status() {
        assert_eq!(Status::Ready.text(), "Ready");
        assert_eq!(Status::Playing.text(), "Playing");
        assert_eq!(Status::Paused.text(), "Paused");
        assert_eq!(Status::Stopped.text(), "Stopped");
        assert_eq!(Status::Finished.text(), "Finished");
        assert_eq!(Status::default(), Status::Ready);
    }

    #[test]
    fn repeat_mode_round_trips_m_repeat_integers() {
        assert_eq!(RepeatMode::Off.as_i32(), 0);
        assert_eq!(RepeatMode::Song.as_i32(), 1);
        assert_eq!(RepeatMode::Playlist.as_i32(), 2);
        assert_eq!(RepeatMode::from_i32(0), RepeatMode::Off);
        assert_eq!(RepeatMode::from_i32(1), RepeatMode::Song);
        assert_eq!(RepeatMode::from_i32(2), RepeatMode::Playlist);
        assert_eq!(RepeatMode::from_i32(99), RepeatMode::Off);
        assert_eq!(RepeatMode::from_i32(-1), RepeatMode::Off);
        assert_eq!(RepeatMode::default(), RepeatMode::Off);
    }

    fn test_snapshot() -> SongSnapshot {
        SongSnapshot {
            title: "song.mid".to_string(),
            format: "SMF".to_string(),
            bars: 32,
            len_ticks: 9600,
            duration_ms: 120_000,
            division: 96,
            num_tracks: 4,
            initial_tempo_us: 500_000.0,
            channel_used: std::array::from_fn(|ch| ch < 3),
            channel_labels: std::array::from_fn(|ch| format!("Ch{ch}")),
            lowest_note: 21,
            highest_note: 108,
            charset: "UTF-8".to_string(),
            texts: Vec::new(),
            metadata: vec![("INAM".to_string(), "Name".to_string())],
            copyrights: vec!["© 1999 Anon".to_string()],
        }
    }

    #[test]
    fn loaded_song_converts_from_snapshot_field_for_field() {
        let song = LoadedSong::from(test_snapshot());
        assert_eq!(song.title, "song.mid");
        assert_eq!(song.format, "SMF");
        assert_eq!(song.bars, 32);
        assert_eq!(song.len_ticks, 9600);
        assert_eq!(song.duration_ms, 120_000);
        assert_eq!(song.division, 96);
        assert_eq!(song.num_tracks, 4);
        assert!((song.initial_tempo_us - 500_000.0).abs() < f64::EPSILON);
        assert!(song.channel_used[0]);
        assert!(!song.channel_used[3]);
        assert_eq!(song.channel_labels[1], "Ch1");
        assert_eq!(song.lowest_note, 21);
        assert_eq!(song.highest_note, 108);
        assert_eq!(song.charset, "UTF-8");
        assert!(song.texts.is_empty());
        assert_eq!(
            song.metadata,
            vec![("INAM".to_string(), "Name".to_string())]
        );
        assert_eq!(song.copyrights, vec!["© 1999 Anon".to_string()]);
    }

    #[test]
    fn last_bar_clamps_to_min_1() {
        let mut song = LoadedSong::from(test_snapshot());
        assert_eq!(song.last_bar(), 32);
        song.bars = 0;
        assert_eq!(song.last_bar(), 1);
        song.bars = -4;
        assert_eq!(song.last_bar(), 1);
    }

    #[test]
    fn settings_state_is_the_persisted_settings() {
        let s = SettingsState::new();
        assert_eq!(s.drums_channel, 10);
        assert_eq!(s.solo_volume_reduction, 50);
    }
}
