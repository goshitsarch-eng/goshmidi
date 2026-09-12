//! T9 application shell: `App` state (arch §2.1), `update()`, subscriptions.
//!
//! Owns the engine thread handle, all UI state, and the dialog/toast stack.
//! Transport/mix/seek/loop/repeat/prefs/song/dialog/nav messages update state
//! optimistically and send the matching `EngineCommand` (arch §2.3);
//! engine feedback arrives as `Message::Player` through the event-stream
//! subscription, meter decay through the 50 ms ticker.
//!
//! `view()` is the T10a2 layout (ux W1/§3.1): playlist sidebar (left) +
//! main column (transport region with the status line, view-switcher
//! placeholder, toasts), wrapped in the `dnd_destination` drop target. View
//! models are built fresh per frame against the
//! [`TransportViewModel`][crate::ui::TransportViewModel] and
//! [`PlaylistViewModel`][crate::ui::PlaylistViewModel] contracts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use cosmic::app::{ContextDrawer, Core, CosmicFlags, Task};
use cosmic::iced::Subscription;
use cosmic::{Action, Application, Element};

use url::Url;

use crate::config::paths::Paths;
use crate::config::settings::Settings;
use crate::config::song::{SongChannelSettings, SongSettings};
use crate::core::event::{MIDI_CHANNELS, clamp_midi, is_supported_midi_file};
use crate::core::output::MidiOutputManager;
use crate::core::output_fluid::find_default_sound_font;
use crate::core::player::{EngineEventReceiver, EngineHandle, spawn_engine};
use crate::core::sequence::Sequence;
use crate::message::{
    Dialog, EngineCommand, LoadedPlaylist, LoadedSong, Message, MidiPort, PlayerEvent, RepeatMode,
    SettingsState, Status, View,
};
use crate::state::channels::{self, ChannelState};
use crate::state::dialogs::{
    clamp_jump_bar, jump_allowed, loop_confirm, loop_seeds, manual_song_action_allowed,
};
use crate::state::keys::{KeyAction, resolve_key};
use crate::state::lyrics::{
    AppliedLyricsPrefs, LyricsState, TYPE_OPTION_NAMES, codec_option_index, track_options,
};
use crate::state::midi::{
    MidiState, NO_MIDI_OUTPUT, OutputCommit, TEST_NOTE_STATUS, is_stale_soundfont, plan_test_note,
    profile_for_port, select_refilled_port, stale_soundfont_toast,
};
use crate::state::piano::{PianoState, PreviewNote, preview_off_targets};
use crate::state::playback::{self, PlaybackState};
use crate::state::playlist::PlaylistState;
use crate::ui::{
    ChannelRowViewModel, ChannelsViewModel, LyricsViewModel, MenuViewModel, PlaylistViewModel,
    PrefsViewModel, TransportViewModel, chrome, info_dialogs, jump_loop, lyrics, menu, midi_setup,
    piano, prefs, sidebar, transport,
};
// Aliased: `channels` already names the mix-state module above.
use crate::ui::channels as channels_view;
use crate::ui::lyrics::{
    LyricsColorRow, LyricsPickerModels, adopt_prefs_scratch, apply_picker_update,
};

/// Meter/clock ticker interval while playing (`m_meterTimer`, 50 ms).
const TICK_INTERVAL: Duration = Duration::from_millis(50);
/// Idle poll quantum of the engine-event forwarder thread.
const ENGINE_POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Toast stack cap: a flood of engine warnings must not grow memory.
const MAX_TOASTS: usize = 8;

// ---------------------------------------------------------------------------
// File-chooser contracts (D1/D9/D10, ux.md dialog table; C++ `window.cpp`
// filter/pattern strings verbatim). The portal `Task`s below build their
// `FileFilter`s from these so tests pin the contract without a portal.
// ---------------------------------------------------------------------------

/// D1 dialog title (`window.cpp:712`).
const OPEN_DIALOG_TITLE: &str = "Open MIDI files";
/// D1 filter label (`window.cpp:714`).
const OPEN_FILTER_LABEL: &str = "MIDI / Karaoke / Cakewalk / Playlist";
/// D1 filter: one filter, 6 glob patterns (`window.cpp:715-720`).
const OPEN_FILTER_GLOBS: [&str; 6] = ["*.mid", "*.midi", "*.kar", "*.rmi", "*.wrk", "*.lst"];
/// D9 dialog title (`window.cpp:1530`); the `.lst` filter is parity-safe
/// added (C++ has no filter there).
const PLAYLIST_OPEN_TITLE: &str = "Open playlist";
const PLAYLIST_FILTER_LABEL: &str = "Playlists";
const PLAYLIST_FILTER_GLOB: &str = "*.lst";
/// D10 dialog title + initial name (`window.cpp:1553-1554`).
const PLAYLIST_SAVE_TITLE: &str = "Save playlist";
const PLAYLIST_SAVE_INITIAL_NAME: &str = "playlist.lst";
/// D12 initial name (`window.cpp:1825`). C++ sets no title and no filter on
/// the lyrics save dialog — parity keeps both unset.
const LYRICS_SAVE_INITIAL_NAME: &str = "lyrics.txt";
/// D13 temp-export name shape: `<prefix><pid>.txt` under the system temp dir.
const PRINT_TEMP_PREFIX: &str = "goshmidi-lyrics-";
/// D13 opener command (DECISIONS D6 choice (a), ux D13 alternative (a)).
const PRINT_OPENER: &str = "xdg-open";
/// D4 link opener: the same `xdg-open` mechanism as D13 (in-sandbox it
/// forwards URLs through the OpenURI portal — `/usr/bin/xdg-open:872-899`
/// `open_flatpak` — so no finish-args are needed, like D13).
const URL_OPENER: &str = "xdg-open";
/// D4 sheet title (the C++ `AdwAboutDialog` sets no window title — the
/// content headings carry it; the port's `dialog()` needs one, so the File
/// menu label `About Gosh MIDI Player` (`window.cpp:1612`) doubles here).
const ABOUT_SHEET_TITLE: &str = "About Gosh MIDI Player";
/// Focus-on-open target (T14d G4 partial): composition focuses the D2
/// `Close` button on open (the `focus(id)` op unfocuses everything else —
/// pinned `iced/core/src/widget/operation/focusable.rs:30-57` — so base
/// Enter cannot fire silently under the dialog). The other dialog targets
/// live in their view modules (`midi_setup::APPLY_ID`,
/// `jump_loop::{JUMP,LOOP}_CONFIRM_ID`, `info_dialogs::{OK,ABOUT_CLOSE}_ID`).
const PREFS_CLOSE_ID: &str = "prefs-close";
/// D11 dialog title + filter (`window.cpp:1094-1100` verbatim).
const SOUNDFONT_DIALOG_TITLE: &str = "Choose SoundFont";
const SOUNDFONT_FILTER_LABEL: &str = "SoundFont";
const SOUNDFONT_FILTER_GLOBS: [&str; 4] = ["*.sf2", "*.sf3", "*.SF2", "*.SF3"];
/// Wayland drop mime accepted by the `dnd_destination` wrapper (arch §2.3).
const DND_URI_LIST_MIME: &str = "text/uri-list";

/// Startup flags parsed from the CLI in `main.rs` (arch §2.5).
/// `backend`/`conn` are applied once as the initial output selection.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// Existing files from CLI positionals (opened replace-playlist, like
    /// `on_open` on first launch).
    pub files: Vec<PathBuf>,
    /// Initial MIDI backend from `-d` (only when `-c` was also given).
    pub backend: Option<String>,
    /// Initial MIDI connection from `-c` (only when `-d` was also given).
    pub conn: Option<String>,
    /// Portable settings mode from `-p` / `-f FILE` (`main.cpp:102-103`).
    pub portable: bool,
    /// Portable settings file from `-f FILE` (`None` = bare `-p`, which
    /// resolves to `./dmidiplayer.conf`).
    pub portable_file: Option<PathBuf>,
    /// String form of `files` for single-instance forwarding (`args()` must
    /// return `&str` borrowed from `self`).
    args_cache: Vec<String>,
}

impl Flags {
    pub fn new(
        files: Vec<PathBuf>,
        backend: Option<String>,
        conn: Option<String>,
        portable: bool,
        portable_file: Option<PathBuf>,
    ) -> Self {
        let args_cache = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        Self {
            files,
            backend,
            conn,
            portable,
            portable_file,
            args_cache,
        }
    }

    /// Portable-mode selector for [`Paths::with_env`]: `None` = normal mode,
    /// `Some(None)` = bare `-p`, `Some(Some(f))` = `-f f` (`main.cpp` parity).
    pub fn portable_config(&self) -> Option<Option<PathBuf>> {
        if self.portable {
            Some(self.portable_file.clone())
        } else {
            None
        }
    }
}

/// Subcommand sent when forwarding a second launch to the running instance.
static OPEN_COMMAND: &str = "open";

impl CosmicFlags for Flags {
    type SubCommand = &'static str;
    type Args = Vec<String>;

    fn action(&self) -> Option<&Self::SubCommand> {
        if self.files.is_empty() {
            None
        } else {
            Some(&OPEN_COMMAND)
        }
    }

    fn args(&self) -> Vec<&str> {
        self.args_cache.iter().map(String::as_str).collect()
    }
}

// ---------------------------------------------------------------------------
// Engine event stream subscription
// ---------------------------------------------------------------------------

/// Process-wide bridge from engine receivers into the iced subscription.
/// `Subscription::run_with` takes `Hash` data plus a plain `fn` pointer
/// (`Recipe` is not re-exported by `cosmic::iced`), so `init` registers its
/// receiver here under a fresh id and the stream builder takes it out.
/// Entries are removed on take; ids are unique per process (tests included).
static ENGINE_REGISTRY: OnceLock<Mutex<HashMap<u64, EngineEventReceiver>>> = OnceLock::new();
static NEXT_ENGINE_ID: AtomicU64 = AtomicU64::new(1);

fn engine_registry() -> &'static Mutex<HashMap<u64, EngineEventReceiver>> {
    ENGINE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Subscription identity for one engine receiver (hash key for `run_with`).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct EngineSubId(u64);

/// Build the engine event stream: a forwarder thread drains the dual
/// outbound channel (lossless in order + latest-wins `Time`, per the T7
/// backpressure design) into an unbounded futures channel.
fn run_engine_stream(
    id: &EngineSubId,
) -> cosmic::iced::futures::stream::BoxStream<'static, Message> {
    use cosmic::iced::futures::stream::StreamExt;

    let rx = engine_registry()
        .lock()
        .ok()
        .and_then(|mut reg| reg.remove(&id.0));
    let Some(rx) = rx else {
        // Already taken (the recipe runs once per id): yield nothing, end
        // never (subscriptions must not end on their own).
        return cosmic::iced::futures::stream::pending().boxed();
    };
    let (tx, stream) = cosmic::iced::futures::channel::mpsc::unbounded();
    let _forwarder = std::thread::Builder::new()
        .name("goshmidi-events".to_string())
        .spawn(move || {
            loop {
                let mut idle = true;
                while let Some(ev) = rx.try_recv_lossless() {
                    idle = false;
                    if tx.unbounded_send(Message::Player(ev)).is_err() {
                        return;
                    }
                }
                if let Some(time) = rx.drain_time_latest() {
                    idle = false;
                    if tx.unbounded_send(Message::Player(time)).is_err() {
                        return;
                    }
                }
                if idle {
                    std::thread::sleep(ENGINE_POLL_INTERVAL);
                }
            }
        })
        .expect("engine event forwarder spawns");
    stream.boxed()
}

/// Map runtime window events to messages: resize → `WindowResized`, X11
/// file drops → `FilesDropped` (replace-playlist, arch §2.3). Hover has no
/// C++ surface and stays unmapped; Wayland drops arrive through the
/// `dnd_destination` widget in `view()` instead (same message path).
fn window_event_message(
    event: cosmic::iced::event::Event,
    _status: cosmic::iced::event::Status,
    _window: cosmic::iced::window::Id,
) -> Option<Message> {
    match event {
        cosmic::iced::event::Event::Window(cosmic::iced::window::Event::Resized(size)) => Some(
            Message::WindowResized(size.width as u32, size.height as u32),
        ),
        cosmic::iced::event::Event::Window(cosmic::iced::window::Event::FileDropped(paths)) => {
            Some(Message::FilesDropped(paths))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Url / drop payload conversion (pure, portal-independent)
// ---------------------------------------------------------------------------

/// Convert portal-chooser `Url`s to local paths (arch §2.3): percent-decoding
/// included via `Url::to_file_path`. The first non-file `Url` aborts the
/// whole batch with a toastable error — never a panic, never a silent skip.
fn urls_to_paths(urls: &[Url]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::with_capacity(urls.len());
    for url in urls {
        match url.to_file_path() {
            Ok(path) => out.push(path),
            Err(()) => return Err(format!("Cannot open {url}: not a local file")),
        }
    }
    Ok(out)
}

/// Drop filter (`dmidi_drop_files`, `window.cpp:80-99`): the 5 supported song
/// extensions (case-insensitive) plus `.lst` (case-sensitive suffix, like
/// `g_str_has_suffix`). Portal `/run/…/doc/…` paths pass like any other —
/// they are ordinary absolute paths by the time they reach here.
fn is_droppable(path: &Path) -> bool {
    is_supported_midi_file(path) || path.to_string_lossy().ends_with(".lst")
}

/// Parse a `text/uri-list` drop payload (RFC 2483 lines: `file://` URIs,
/// `#` comments, CRLF): percent-decoding + non-file-URL rules shared with
/// the chooser path, then the C++ drop filter — unsupported entries and
/// unparsable lines are silently skipped, exactly like the GTK target.
fn parse_uri_list(data: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Ok(url) = Url::parse(line) else {
            continue;
        };
        let Ok(path) = url.to_file_path() else {
            continue;
        };
        if is_droppable(&path) {
            out.push(path);
        }
    }
    out
}

/// D12 write half (`g_file_set_contents` twin, `window.cpp:1836`): full-text
/// write. Errors toast (FIX — C++ ignores the result); success is silent
/// (parity — C++ shows nothing on save).
fn write_lyrics_file(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text)
        .map_err(|why| format!("Could not save lyrics {}: {why}", path.display()))
}

/// D13 export half: write the full lyrics text to a per-process temp `.txt`
/// (no cleanup — the OS reaps temp; a second print overwrites the same path)
/// and hand it to the opener. Sync I/O + detached spawn, run inside the
/// print `Task` like the other chooser-task blocking halves.
fn print_lyrics_via_opener(text: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("{PRINT_TEMP_PREFIX}{}.txt", std::process::id()));
    std::fs::write(&path, text)
        .map_err(|why| format!("Could not print lyrics {}: {why}", path.display()))?;
    spawn_opener(PRINT_OPENER, &path)
}

/// Detached opener spawn (stdout/stderr/stdin nulled, no wait): `Ok` once
/// the child is spawned — the viewer owns everything after that.
fn spawn_opener(cmd: &str, path: &Path) -> Result<(), String> {
    std::process::Command::new(cmd)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|why| format!("Could not print lyrics: {why}"))
}

/// D4 link half: hand a URL to the system opener (detached spawn, same
/// `xdg-open` mechanism + sandbox coverage as D13 — see `URL_OPENER`).
fn open_url_via_opener(url: &str) -> Result<(), String> {
    std::process::Command::new(URL_OPENER)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|why| format!("Could not open {url}: {why}"))
}

/// D3 refill half (C++ `refillFn`, `window.cpp:1032-1060`): enumerate one
/// backend's ports. App-side construction is precedented (`MidiState::new`
/// already builds a manager here) and safe: `AlsaOutput::new` degrades to
/// an empty list without a sequencer, `FluidOutput::new` performs no FFI.
/// Unknown backends enumerate the front backend (manager A6c rule).
fn enumerate_ports(backend: &str, advanced: bool) -> Vec<MidiPort> {
    let manager = MidiOutputManager::new();
    manager
        .find(backend)
        .map_or_else(Vec::new, |out| out.ports(advanced))
}

/// Pure half of `dbus_activation`: map a forwarded second launch to file
/// paths. `Activate` (bare second launch) → no paths, present-only;
/// `Open` → `Url`s with non-file entries skipped (C++ `g_file_get_path`
/// NULL-skip parity); `ActivateAction` → our own CLI path args, empty
/// entries skipped. No existence check (A14 keeps nonexistent paths).
fn activation_files(msg: &cosmic::dbus_activation::Message) -> Vec<PathBuf> {
    use cosmic::dbus_activation::Details;

    match &msg.msg {
        Details::Activate => Vec::new(),
        Details::Open { url } => url.iter().filter_map(|u| u.to_file_path().ok()).collect(),
        Details::ActivateAction { action: _, args } => args
            .iter()
            .filter(|a| !a.is_empty())
            .map(PathBuf::from)
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Application state (arch §2.1): engine handle + all UI state + dialog +
/// toasts. The engine thread owns the `Sequence` and outputs; everything
/// here is either an immutable snapshot or optimistic UI state reconciled
/// through `PlayerEvent`s.
pub struct App {
    core: Core,
    engine: EngineHandle,
    engine_sub: EngineSubId,
    playback: PlaybackState,
    song: Option<LoadedSong>,
    playlist: PlaylistState,
    channels: [ChannelState; MIDI_CHANNELS],
    lyrics: LyricsState,
    piano: PianoState,
    midi: MidiState,
    settings: Settings,
    paths: Paths,
    dialog: Option<Dialog>,
    /// D2 prefs dialog shell (T12b): `None` while shut; `Some` while open —
    /// whole-`Settings` scratch + D17 picker models. Seeded on
    /// `ShowDialog(Preferences)`, edited scratch-only, committed once +
    /// cleared on close.
    prefs: Option<(PrefsViewModel, LyricsPickerModels)>,
    /// D3 MIDI Setup dialog shell (T13c): `None` while shut; `Some` while
    /// open — dialog-local scratch (C++ `Widgets`, `window.cpp:929-1187`)
    /// plus the transient test status. Seeded from live `midi` on
    /// `ShowDialog(MidiSetup)`; Test/Apply commit to settings + engine;
    /// close abandons. Live `midi` mirrors only move on commit.
    midi_dlg: Option<MidiSetupDialog>,
    /// D7 jump scratch: bar number, seeded to 1 on open (the C++ spin is
    /// never `set_value`d, so it shows the range min, `window.cpp:790`).
    jump_bar: u32,
    /// D8 loop scratch: from/to bars, seeded from `loop_start`/`loop_end`
    /// on open (C++ `window.cpp:852-853`, clamped like the spins).
    loop_from: u32,
    loop_to: u32,
    view: View,
    toasts: Vec<String>,
    /// Autoplay flag for the in-flight `Load` (consumed by `Opened`).
    pending_autoplay: bool,
    /// Output-selection epoch (T14d minor c): bumped by
    /// [`send_select_output`](Self::send_select_output) on EVERY
    /// `SelectOutput` send (startup, test, apply, prefs reseat, fallback).
    /// `MidiTestNote` captures it into the 400 ms `MidiTestNoteOff`, so the
    /// off addresses the NoteOn output exactly when no reselect landed in
    /// between (C++ `:1148-1153` captures the `MidiOutput*` — an epoch is
    /// the port's equivalent, since only the engine addresses outputs).
    output_epoch: u64,
}

/// D3 dialog shell: dialog-local scratch + the transient test status line.
/// The scratch is a whole `MidiState` clone so the D3 view builder (which
/// takes `&MidiState`) applies unchanged; the engine-mirror fields
/// (`announce_applied`, `fallback_stage`, `output_ok_once`) ride along but
/// are never read from the scratch and never committed back — Apply/Test
/// copy exactly the 7 dialog fields.
pub struct MidiSetupDialog {
    scratch: MidiState,
    test_status: Option<String>,
}

impl Drop for App {
    fn drop(&mut self) {
        // Prompt engine exit (the thread would also end on handle-drop).
        self.engine.send(EngineCommand::Shutdown);
    }
}

impl App {
    // -- startup ----------------------------------------------------------

    /// The ONLY `SelectOutput` sender: bumps [`output_epoch`](Self::output_epoch)
    /// with every send so a captured epoch identifies the selection the
    /// engine will hold (T14d minor c — every future reselect path must
    /// route through here or the test-note capture goes stale-blind).
    fn send_select_output(&mut self, backend: String, port: String) {
        self.output_epoch = self.output_epoch.wrapping_add(1);
        self.engine
            .send(EngineCommand::SelectOutput { backend, port });
    }

    /// Initial output selection (ctor chain, `window.cpp:227-242`): optional
    /// SoundFont, saved-or-CLI backend/port, drums channel, SysEx reset.
    /// Recovery from a dead saved port happens in `OutputChanged` via the
    /// staged fallback chain (FluidSynth → Dummy + warning toast).
    ///
    /// T15-S3 stale-SF fallback (ux §15.1, packaging §6): an unreadable
    /// configured SoundFont keeps its settings value (P7: never silently
    /// drop) but the engine gets the bundled/default fallback instead, with
    /// a toast naming the stale path. The live mirror keeps the stale value
    /// so D3 shows the entry with its warning hint + Browse (D11) offer.
    fn startup_select_output(&mut self, flags: &Flags) {
        let mut backend = flags
            .backend
            .clone()
            .unwrap_or_else(|| self.settings.last_output_backend.clone());
        if backend.is_empty() {
            backend = "FluidSynth".to_string();
        }
        let port = flags
            .conn
            .clone()
            .unwrap_or_else(|| self.settings.last_output_connection.clone());
        if !self.settings.sound_font.is_empty() {
            if is_stale_soundfont(&self.settings.sound_font) {
                let fallback = find_default_sound_font();
                if fallback.is_empty() {
                    self.engine.send(EngineCommand::SetSoundFont(
                        self.settings.sound_font.clone(),
                    ));
                } else {
                    self.engine
                        .send(EngineCommand::SetSoundFont(fallback.clone()));
                }
                self.toast(stale_soundfont_toast(&self.settings.sound_font, &fallback));
            } else {
                self.engine.send(EngineCommand::SetSoundFont(
                    self.settings.sound_font.clone(),
                ));
            }
        }
        self.midi.backend = backend.clone();
        self.midi.port = port.clone();
        self.midi.soundfont = self.settings.sound_font.clone();
        self.midi.advanced = self.settings.advanced_ports;
        self.midi.instrument_map = self.settings.instrument_map.clamp(0, 2) as u8;
        self.midi.sysex_reset = self.settings.sysex_reset.clamp(0, 4) as u8;
        self.send_select_output(backend, port);
        self.engine.send(EngineCommand::SetDrumsChannel(
            (self.settings.drums_channel.clamp(1, 16) - 1) as u8,
        ));
        self.engine
            .send(EngineCommand::SetSysexReset(self.midi.sysex_reset));
    }

    /// F1 playlist restore (`window.cpp:244-268`): `lastPlayList`, then the
    /// `examples.lst` candidates, then a scan of `./examples/` for supported
    /// songs. Restores selection only — nothing loads until Play.
    fn restore_playlist(&mut self) {
        if !self.settings.last_play_list.is_empty() {
            self.playlist.load(Path::new(&self.settings.last_play_list));
        }
        if !self.playlist.is_empty() {
            return;
        }
        // NOTE: `datadir` is `None` until T10/pack wires the install prefix;
        // the Flatpak build always ships `./examples/` next to the binary
        // path equivalent, and the dir scan below covers it.
        for candidate in self.paths.examples_playlist_candidates(None) {
            if candidate.exists() && self.playlist.load(&candidate) {
                break;
            }
        }
        if !self.playlist.is_empty() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(self.paths.examples_dir()) {
            let mut found: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| is_supported_midi_file(p))
                .collect();
            found.sort();
            for path in found {
                self.playlist.add(&path);
            }
        }
    }

    // -- file / song loading ----------------------------------------------

    /// `openFiles` (`window.cpp:351-370`): a single `.lst` loads as a
    /// playlist (records `lastPlayList`); otherwise the files replace or
    /// extend the playlist, the first is selected, and the current song
    /// loads with autoplay. The `.lst` sniff is a case-sensitive suffix
    /// match, exactly like `g_str_has_suffix`.
    fn open_files(&mut self, files: Vec<PathBuf>, replace: bool) {
        if files.is_empty() {
            return;
        }
        if files.len() == 1 && files[0].to_string_lossy().ends_with(".lst") {
            if self.playlist.load(&files[0]) {
                self.settings.last_play_list = files[0].to_string_lossy().into_owned();
                let autoplay = self.settings.auto_play;
                self.load_current(autoplay);
            }
            return;
        }
        if replace {
            self.playlist.clear();
        }
        for file in &files {
            self.playlist.add(file);
        }
        self.playlist.select_first();
        let autoplay = self.settings.auto_play;
        self.load_current(autoplay);
    }

    /// OS file drop (`dmidi_drop_files`, `window.cpp:80-99`): keep only
    /// droppable entries, then `openFiles(paths, replace=true)`. An empty or
    /// fully-filtered drop is a silent no-op (the GTK target rejects it).
    fn dropped_files(&mut self, paths: Vec<PathBuf>) {
        let kept: Vec<PathBuf> = paths.into_iter().filter(|p| is_droppable(p)).collect();
        if !kept.is_empty() {
            self.open_files(kept, true);
        }
    }

    /// `loadCurrent` (`window.cpp:372-404`), async half: stop the engine,
    /// clear the piano/position UI, and send `Load`. Everything else (title,
    /// mix sync, song settings, autoplay) happens in `on_opened` when the
    /// engine answers — engine-first (A8).
    fn load_current(&mut self, autoplay: bool) {
        let Some(path) = self.playlist.current().map(Path::to_path_buf) else {
            return;
        };
        self.engine.send(EngineCommand::Stop { reset: false });
        self.piano.clear();
        self.playback.position_ticks = 0;
        self.playback.elapsed_ms = 0;
        self.playback.seeking = false;
        self.pending_autoplay = autoplay;
        self.engine.send(EngineCommand::Load(path));
    }

    /// `PlayerEvent::Opened` (`loadCurrent` completion): build the UI song,
    /// sync UI state to the freshly-reset engine (A8 FIX — the engine's
    /// `load_song` already reset mix + loop + position; the C++ UI kept
    /// stale mix widgets here), record recent + directory (persisted by the
    /// next save, never here), auto-load song settings, then autoplay or
    /// stop.
    fn on_opened(&mut self, song: LoadedSong) {
        let autoplay = self.pending_autoplay;
        self.pending_autoplay = false;
        let last_bar = song.last_bar();
        self.playback
            .reset_for_song(song.len_ticks, song.initial_tempo_us, last_bar);
        channels::reset_for_song(&mut self.channels, &song.channel_used, &song.channel_labels);
        self.piano.reset_for_song();
        self.lyrics
            .reset_for_song(song.charset.clone(), &song.texts);
        if let Some(current) = self.playlist.current() {
            self.settings.add_recent(&current.to_string_lossy());
            if let Some(parent) = current.parent() {
                self.settings.last_directory = parent.to_string_lossy().into_owned();
            }
        }
        self.core
            .set_header_title(format!("Gosh MIDI Player — {}", song.title));
        self.song = Some(song);
        if self.settings.auto_song_settings {
            self.load_song_settings();
        }
        if autoplay {
            self.start_playback();
        } else {
            self.playback.status = Status::Stopped;
        }
    }

    // -- transport ----------------------------------------------------------

    /// `play()` (`window.cpp:406-416`): empty song + non-empty playlist loads
    /// first; otherwise send `Play` with an optimistic `Playing` status
    /// (`Started` confirms).
    fn start_playback(&mut self) {
        if self.song.is_none() {
            if !self.playlist.is_empty() {
                self.load_current(true);
            }
            return;
        }
        self.engine.send(EngineCommand::Play);
        self.playback.status = Status::Playing;
    }

    /// `pause()` (`window.cpp:418-426`): pause/resume toggle.
    fn toggle_pause(&mut self) {
        if self.playback.status == Status::Playing {
            self.engine.send(EngineCommand::Pause);
            self.playback.status = Status::Paused;
        } else if self.song.is_some() {
            self.start_playback();
        }
    }

    /// `stop(true)` (`window.cpp:428-438`, the `app.stop` action): stop with
    /// reset, clear piano + clock UI, optimistic `Stopped`.
    fn stop_playback(&mut self) {
        self.engine.send(EngineCommand::Stop { reset: true });
        self.playback.position_ticks = 0;
        self.playback.elapsed_ms = 0;
        self.playback.seeking = false;
        self.piano.clear();
        self.playback.status = Status::Stopped;
    }

    /// `nextSong` (`window.cpp:440-448`): auto-save, advance (no wrap),
    /// load + autoplay.
    fn next_song(&mut self) {
        self.maybe_save_song_settings();
        if self.playlist.select_next() {
            self.load_current(true);
        }
    }

    /// `prevSong` (`window.cpp:450-456`): advance back, load + autoplay.
    /// NOTE: no auto-save — the C++ prev path never saves (preserved).
    fn prev_song(&mut self) {
        if self.playlist.select_prev() {
            self.load_current(true);
        }
    }

    /// `onFinished` (`window.cpp:194-214`): status first, then the finish
    /// policy. Repeat-song returns early (engine reset + play, no save);
    /// every other finish saves song settings first (`window.cpp:202-203`,
    /// including stay-`Finished`), then branches — repeat-list (advance with
    /// wrap + load), auto-advance (same as Next, whose own save restores
    /// C++'s double-save: same bytes, harmless), or stay `Finished`.
    fn on_finished(&mut self) {
        self.playback.status = Status::Finished;
        let action = playback::finish_action(
            self.playback.repeat,
            self.settings.auto_advance,
            self.playlist.at_last(),
        );
        if action == playback::FinishAction::RepeatSong {
            self.engine.send(EngineCommand::ResetPosition);
            self.engine.send(EngineCommand::Play);
            self.playback.status = Status::Playing;
            return;
        }
        // Save before branching on every finish except repeat-song (which
        // returned above) — `window.cpp:202-203` parity, including
        // `StayFinished`. `AutoAdvance` delegates to `next_song`, whose own
        // save restores C++'s double-save (identical bytes, harmless).
        self.maybe_save_song_settings();
        match action {
            playback::FinishAction::RepeatSong => unreachable!("handled above"),
            playback::FinishAction::RepeatList => {
                if !self.playlist.select_next() {
                    self.playlist.select_first();
                }
                self.load_current(true);
            }
            playback::FinishAction::AutoAdvance => self.next_song(),
            playback::FinishAction::StayFinished => {}
        }
    }

    // -- mix ------------------------------------------------------------------

    /// Push one channel's mute + volume factor to the engine (the per-row
    /// half of `applyChannelSoloMute`, `window.cpp:479-495`).
    fn push_channel(&self, ch: u8) {
        let idx = ch as usize;
        if idx >= MIDI_CHANNELS {
            return;
        }
        let sync = channels::sync_row(
            &self.channels[idx],
            channels::any_solo(&self.channels),
            self.settings.solo_volume_reduction,
        );
        self.engine.send(EngineCommand::SetMuted(ch, sync.muted));
        self.engine
            .send(EngineCommand::SetChannelVolume(ch, sync.factor));
    }

    /// Push the whole mix (the full `applyChannelSoloMute` loop): used after
    /// solo flips (which change every row's factor), song-settings loads,
    /// and prefs commits (solo% — U14).
    fn push_all_channels(&self) {
        for (ch, sync) in channels::sync_all(&self.channels, self.settings.solo_volume_reduction)
            .iter()
            .enumerate()
        {
            let ch = ch as u8;
            self.engine.send(EngineCommand::SetMuted(ch, sync.muted));
            self.engine
                .send(EngineCommand::SetChannelVolume(ch, sync.factor));
        }
    }

    // -- prefs / MIDI setup apply --------------------------------------------

    /// Commit-on-close prefs apply (U14, `window.cpp:1310-1340`): adopt the
    /// merged commit wholesale (the close arm narrows the dialog scratch to
    /// the 17 D2-owned keys over live settings first — T12 devil-BLOCKING
    /// FIX), apply the General rows (drums, SysEx reset, instrument map,
    /// full mix incl. solo% at close, output reseat on a D3 delta), the
    /// Lyrics rows, and the Piano rows, then save the full config
    /// (save-point row 3).
    fn apply_prefs(&mut self, prefs: SettingsState) {
        let old = std::mem::replace(&mut self.settings, prefs);
        self.apply_general_prefs(&old);
        self.apply_lyrics_prefs();
        self.apply_piano_prefs();
        // C++ parity: `AppSettings::save()` is void — failures are silent.
        if self.settings.save(&self.paths).is_err() {
            // Silent, as in C++.
        }
    }

    /// General page of the prefs apply (D2 General rows): drums channel →
    /// engine (clamped 1–16, sent 0-based); SysEx reset → engine + mirror;
    /// instrument map → mirror (the patch model relabels render-time via
    /// [`channels::patch_options`], T12b); solo% → full mix repush (U14 FIX —
    /// C++ never repushed here, so a changed solo% silently did nothing until
    /// the next mix touch); `autoPlay` / `autoAdvance` / `autoSongSettings`
    /// are behavior flags with no engine command (read at load/finish/next
    /// time). `lastOutputBackend` / `lastOutputConnection` / `soundFont` /
    /// `advancedPorts` are D3-owned keys that reach the commit from live
    /// (the close arm narrows the scratch to the 17 D2 keys, so on the close
    /// path the planner below always sees an empty delta and no-ops —
    /// definitionally current, in particular a startup-fallback selection
    /// survives); a `PrefsChanged` carrying a D3 change vs `old` still
    /// reseats the output (`SetSoundFont` + `SelectOutput`, close+reopen
    /// inside the manager) and announces like a user apply (contract,
    /// unit-pinned — close is the only production producer and never
    /// carries one). C++ twin: `window.cpp:1315-1321`, `:1332-1334`.
    fn apply_general_prefs(&mut self, old: &SettingsState) {
        self.engine.send(EngineCommand::SetDrumsChannel(
            (self.settings.drums_channel.clamp(1, 16) - 1) as u8,
        ));
        let reset = self.settings.sysex_reset.clamp(0, 4) as u8;
        self.engine.send(EngineCommand::SetSysexReset(reset));
        self.midi.sysex_reset = reset;
        self.midi.instrument_map = self.settings.instrument_map.clamp(0, 2) as u8;
        self.push_all_channels();
        let reseat = self.midi.plan_output_reseat(
            OutputCommit {
                backend: &old.last_output_backend,
                port: &old.last_output_connection,
                soundfont: &old.sound_font,
                advanced: old.advanced_ports,
            },
            OutputCommit {
                backend: &self.settings.last_output_backend,
                port: &self.settings.last_output_connection,
                soundfont: &self.settings.sound_font,
                advanced: self.settings.advanced_ports,
            },
        );
        if let Some(font) = reseat.sound_font {
            self.engine.send(EngineCommand::SetSoundFont(font));
        }
        if reseat.select_output {
            // User-initiated apply: announce the result (unlike the silent
            // startup select).
            self.midi.announce_applied = true;
            let backend = self.midi.backend.clone();
            let port = self.midi.port.clone();
            self.send_select_output(backend, port);
        }
    }

    /// Lyrics rows of the prefs apply (U14 pathway, D2 Lyrics page): derive
    /// the applied colors/font/alignment from the adopted settings
    /// (lyrics colors/fonts/alignment ONLY — `singleColor` is a Piano row,
    /// T12). Body and filters are UNTOUCHED: C++ `refreshLyrics` on
    /// prefs-close resets both dropdowns to best-score as a side effect of
    /// its rebuild (`window.cpp:1335` → `:566-573`, same snap-back family as
    /// the filter handlers); the port keeps user filter selections (FIX,
    /// lead-directed T11).
    fn apply_lyrics_prefs(&mut self) {
        self.lyrics.applied = AppliedLyricsPrefs::from_rows(
            &self.settings.future_color,
            &self.settings.past_color,
            &self.settings.highlight_color,
            &self.settings.lyrics_font,
            self.settings.text_alignment,
        );
    }

    /// Piano page of the prefs apply (D2 Piano rows, `window.cpp:1327-1331`):
    /// `highlightPalette` / `singleColor` / `velocityColor` /
    /// `namesVisibility` / `octaveSubscript` are adopted by the wholesale
    /// settings replace and apply at render time from `self.settings` — the
    /// T13 canvas reads them per draw, exactly like the C++ `draw_func`
    /// (`singleColor` parses under `gdk_rgba_parse` parity at the renderer,
    /// gray on garbage). Deliberately a documented step rather than a silent
    /// gap: C++ close sends no engine command and derives no eager piano
    /// state either.
    fn apply_piano_prefs(&self) {}

    /// D3 Apply (`window.cpp:1158-1180` + `connectOutput`, `window.cpp:328-349`):
    /// commit the scratch soundFont/advanced/map/reset (verbatim, C++
    /// `:1168-1171`), send `SetSysexReset` → `Stop{reset:false}` (only when
    /// the plan sends, T13a) → `SetSoundFont` (non-empty only, T12 apply
    /// rule) → `SelectOutput`, announce the reselect (success toast on
    /// `OutputChanged`), persist (save-point row 2), and close.
    ///
    /// The applied port is the DROPDOWN-SHOWN id (T14d minor d — C++ `:1164-1166`
    /// reads `plist[pi].id`, i.e. the refill preselect over the scratch want,
    /// exactly like the test path `:1135-1137`), never the raw want. The two
    /// differ only when the want is stale at Apply: a mid-refill race (ports
    /// still empty — shown `""`, C++ `pi < size` fails the same way) or a
    /// refill disjoint from the want (unplugged device accepted as-is —
    /// shown is index 0). Both are contrived against the steady state (the
    /// refill lands in ms; users pick); pinned by test.
    ///
    /// Stop-first delta (T13a, for devil judgment): C++ stops
    /// unconditionally — Apply always calls `connectOutput`, whose first
    /// statement is `m_player.stop()` (`window.cpp:330`). The port plans
    /// through the T12 reseat planner and stops only when the plan actually
    /// sends something (`stop_first`), so a no-change Apply — and a
    /// map/reset-only Apply — never interrupts playback. A deliberate
    /// improvement. The send order differs from C++ (`:1172-1175` stages the
    /// font BEFORE stopping; T14d minor a rewords the old "safer" gloss):
    /// both orders are behaviorally EQUIVALENT — each side only stages the
    /// path (`m_requestedSf` / `requested_sf`) and the actual `sfload` runs
    /// at open (`output.cpp:316` / `output_fluid.rs`), so stopping first
    /// changes nothing observable.
    ///
    /// No shell (stale message) → silent no-op, prefs precedent.
    fn apply_midi_setup(&mut self) {
        let Some(dlg) = self.midi_dlg.as_ref() else {
            return;
        };
        // Clone the scratch pieces first: the planner mutates live `midi`
        // below, and the shell borrow must end before that. The port want is
        // the dropdown-shown id (see above), resolved like the test path.
        let reset = dlg.scratch.sysex_reset;
        let soundfont = dlg.scratch.soundfont.clone();
        let backend = dlg.scratch.backend.clone();
        let shown = select_refilled_port(&dlg.scratch.ports, &dlg.scratch.port);
        let port = dlg
            .scratch
            .ports
            .get(shown)
            .map_or_else(String::new, |port| port.id.clone());
        let advanced = dlg.scratch.advanced;
        let map = dlg.scratch.instrument_map;
        self.settings.sound_font = soundfont.clone();
        self.settings.advanced_ports = advanced;
        self.settings.instrument_map = i32::from(map);
        self.settings.sysex_reset = i32::from(reset);
        // T13d F1: sync the map/reset live mirrors (the D3 seed reads them —
        // without this a reopen seeds the stale values and a no-change
        // Apply clobbers the commit). The other four mirrors moved via the
        // planner below; these two have no planner leg (render/engine-only).
        self.midi.instrument_map = map;
        self.midi.sysex_reset = reset;
        self.engine.send(EngineCommand::SetSysexReset(reset));
        let plan = self.midi.plan_output_reseat(
            OutputCommit {
                backend: &self.midi.backend.clone(),
                port: &self.midi.port.clone(),
                soundfont: &self.midi.soundfont.clone(),
                advanced: self.midi.advanced,
            },
            OutputCommit {
                backend: &backend,
                port: &port,
                soundfont: &soundfont,
                advanced,
            },
        );
        if plan.stop_first() {
            self.engine.send(EngineCommand::Stop { reset: false });
        }
        if let Some(font) = plan.sound_font {
            self.engine.send(EngineCommand::SetSoundFont(font));
        }
        if plan.select_output {
            // User-initiated apply: announce the result (unlike the silent
            // startup select). Set ONLY with a reselect — a bare flag with
            // no `OutputChanged` coming would mis-attribute a later event.
            self.midi.announce_applied = true;
            let backend = self.midi.backend.clone();
            let port = self.midi.port.clone();
            self.send_select_output(backend, port);
        }
        self.settings.last_output_backend = self.midi.backend.clone();
        self.settings.last_output_connection = self.midi.port.clone();
        // C++ `applyInstrumentMap` (`:1176`) relabels the patch dropdowns;
        // the port relabels render-time from `settings.instrument_map`
        // (T12b) — nothing to send.
        if self.settings.save(&self.paths).is_err() {
            // Silent, as in C++.
        }
        self.midi_dlg = None;
        self.dialog = None;
    }

    /// D3 Test note (`window.cpp:1130-1156`): commit the scratch SoundFont
    /// entry verbatim (even empty — C++ `:1138` assigns unconditionally),
    /// `Stop{reset:false}` → `SetSoundFont` → unconditional
    /// `SelectOutput` (the test path does NOT use the conditional T12
    /// planner — its job is to audition the live selection), then Middle C
    /// (ch 0, note 60, vel 96) as an engine-carried raw `PreviewNote` + the
    /// 400 ms `MidiTestNoteOff` sleep. Status `TEST_NOTE_STATUS`
    /// optimistically; a failing reselect overwrites it with
    /// `NO_MIDI_OUTPUT` in the `OutputChanged` arm (C++ `:1143-1145`). The
    /// dialog stays open (C++ destroys nothing on Test). No shell (stale
    /// message) → silent no-op, prefs precedent.
    fn test_midi_note(&mut self) -> Task<Message> {
        let Some(dlg) = self.midi_dlg.as_ref() else {
            return Task::none();
        };
        // The auditioned port is the DROPDOWN selection (C++ `:1135-1137`
        // reads `plist[pi].id`), i.e. the refill preselect over the scratch
        // want — not the want itself.
        let selected = select_refilled_port(&dlg.scratch.ports, &dlg.scratch.port);
        let port_id = dlg
            .scratch
            .ports
            .get(selected)
            .map_or_else(String::new, |port| port.id.clone());
        let plan = plan_test_note(&dlg.scratch.backend, &port_id, &dlg.scratch.soundfont);
        self.settings.sound_font = plan.soundfont.clone();
        self.midi.soundfont = plan.soundfont.clone();
        self.engine.send(EngineCommand::Stop { reset: false });
        self.engine
            .send(EngineCommand::SetSoundFont(plan.soundfont));
        self.midi.announce_applied = true;
        let (backend, port) = plan.select_output;
        self.send_select_output(backend, port);
        // Capture the post-send epoch: the 400 ms off addresses this
        // selection (T14d minor c — C++ `:1148-1153` captures `out`).
        let epoch = self.output_epoch;
        let (ch, note, vel) = plan.note_on;
        self.engine
            .send(EngineCommand::PreviewNote { ch, note, vel });
        if let Some(dlg) = self.midi_dlg.as_mut() {
            dlg.test_status = Some(TEST_NOTE_STATUS.to_string());
        }
        Self::test_note_off_task(ch, note, epoch, plan.off_delay_ms)
    }

    // -- song settings ---------------------------------------------------------

    fn song_settings_name(&self) -> Option<String> {
        self.song.as_ref().map(|s| s.title.clone())
    }

    fn maybe_save_song_settings(&mut self) {
        if self.settings.auto_song_settings {
            self.save_song_settings();
        }
    }

    /// `saveSongSettings` (`window.cpp:631-659`): build from live UI state —
    /// global volume/pitch/tempo + used channels only — `ensure_data_dir`
    /// first (T8 obligation: the C++ path helper creates the dir as a side
    /// effect; here the save path calls it explicitly), then write.
    fn save_song_settings(&mut self) {
        let Some(name) = self.song_settings_name() else {
            return;
        };
        let mut cfg = SongSettings::empty();
        cfg.file = self
            .playlist
            .current()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        cfg.encoding = Some(self.lyrics.codec.clone());
        cfg.volume = Some(self.playback.volume_pct as i32);
        cfg.pitch = self.playback.pitch_semitones;
        cfg.timeskew = Some(self.playback.tempo_factor_pct as i32);
        for (ch, row) in self.channels.iter().enumerate() {
            if !row.used {
                continue;
            }
            cfg.channels[ch] = Some(SongChannelSettings {
                name: Some(row.label.clone()),
                muted: row.muted,
                solo: row.soloed,
                locked: row.locked,
                patch: i32::from(row.patch),
                level: Some(row.level_pct as i32),
            });
        }
        let path = self.paths.song_settings_path(&name);
        if self.paths.ensure_data_dir().is_err() {
            return;
        }
        if cfg.save(&path).is_err() {
            // Silent, as in C++ (`g_file_set_contents` result ignored).
        }
    }

    /// `loadSongSettings` (`window.cpp:661-708`), engine-first (arch §4.3
    /// FIX): send the parsed mix to the engine first
    /// (`SetMuted`/`SetLocked`/`SetPatch`/`SetChannelVolume`/
    /// `SetVolumeFactor`/`SetPitchShift`/`SetTempoFactor`), then sync UI
    /// state from the applied values — never UI-first via widget signals.
    /// Missing/unreadable/corrupt file → silent no-op (C++ early-return).
    /// Widget-range clamps mirror the GTK control behavior (sliders/spins
    /// clamp out-of-range file values).
    fn load_song_settings(&mut self) {
        let Some(name) = self.song_settings_name() else {
            return;
        };
        let path = self.paths.song_settings_path(&name);
        let Some(cfg) = SongSettings::load(&path) else {
            return;
        };
        let plan = cfg.apply_plan();
        if let Some(volume) = plan.volume {
            let pct = volume.clamp(
                playback::VOLUME_MIN_PCT as i32,
                playback::VOLUME_MAX_PCT as i32,
            ) as u32;
            self.engine.send(EngineCommand::SetVolumeFactor(pct));
            self.playback.volume_pct = pct;
        }
        let pitch = plan.pitch.clamp(playback::PITCH_MIN, playback::PITCH_MAX);
        self.engine.send(EngineCommand::SetPitchShift(pitch));
        self.playback.pitch_semitones = pitch;
        if let Some(skew) = plan.timeskew {
            let pct = skew.clamp(
                playback::TEMPO_MIN_PCT as i32,
                playback::TEMPO_MAX_PCT as i32,
            ) as u32;
            self.engine
                .send(EngineCommand::SetTempoFactor(f64::from(pct) / 100.0));
            self.playback.tempo_factor_pct = pct;
            self.playback.bpm = self.playback.live_bpm();
        }
        for apply in &plan.channels {
            let idx = apply.channel as usize;
            if idx >= MIDI_CHANNELS {
                continue;
            }
            let patch = {
                let row = &mut self.channels[idx];
                if let Some(n) = &apply.name {
                    row.label = n.clone();
                }
                row.muted = apply.muted;
                row.soloed = apply.solo;
                row.locked = apply.locked;
                row.patch = clamp_midi(apply.patch) as u8;
                if let Some(level) = apply.level {
                    row.set_level_pct(level.clamp(0, 200) as u32);
                }
                row.patch
            };
            // C++ order per channel: `setLocked` then `setPatch`
            // (`window.cpp:700-701`); mute joins explicitly here (C++ applies
            // it through the trailing `applyChannelSoloMute`).
            self.engine
                .send(EngineCommand::SetMuted(apply.channel, apply.muted));
            self.engine
                .send(EngineCommand::SetLocked(apply.channel, apply.locked));
            self.engine
                .send(EngineCommand::SetPatch(apply.channel, patch));
        }
        self.push_all_channels();
        if let Some(encoding) = plan.encoding {
            self.lyrics.codec = encoding;
            // Recompute the body under the restored codec. Filters are KEPT
            // (C++ `refreshLyrics` here resets them to best-score —
            // `window.cpp:675` → `:566-573` — same snap-back FIX as prefs).
            self.refresh_lyrics_body();
        }
    }

    // -- playlist results -------------------------------------------------------

    /// D1 portal chooser `Task` (pinned provider: xdg-portal/ashpd per D16/A5
    /// — NEVER rfd): multi-select over the one 6-glob filter. `Ok` converts
    /// every `Url` to a `PathBuf` and feeds `OpenFilesChosen`; `Cancelled`
    /// maps to the silent `ChooserCancelled`; any other portal error toasts.
    /// NOTE: `lastDirectory` is deliberately NOT wired as the initial folder
    /// — the open-dialog `directory` builder is dead upstream (ux D1).
    fn open_files_task() -> Task<Message> {
        cosmic::task::future(async move {
            use cosmic::dialog::file_chooser::{FileFilter, open};
            let mut filter = FileFilter::new(OPEN_FILTER_LABEL);
            for glob in OPEN_FILTER_GLOBS {
                filter = filter.glob(glob);
            }
            let dialog = open::Dialog::new().title(OPEN_DIALOG_TITLE).filter(filter);
            match dialog.open_files().await {
                Ok(response) => Message::OpenFilesChosen(urls_to_paths(response.urls())),
                Err(cosmic::dialog::file_chooser::Error::Cancelled) => Message::ChooserCancelled,
                Err(why) => Message::OpenFilesChosen(Err(format!("Could not open files: {why}"))),
            }
        })
    }

    /// D9 portal chooser `Task`: single-file open with the `.lst` filter. The
    /// `.lst` itself is parsed by reusing `PlaylistState::load` verbatim (no
    /// parallel parser), then handed to `PlaylistLoaded` (no autoplay).
    fn playlist_load_task() -> Task<Message> {
        cosmic::task::future(async move {
            use cosmic::dialog::file_chooser::{FileFilter, open};
            let dialog = open::Dialog::new()
                .title(PLAYLIST_OPEN_TITLE)
                .filter(FileFilter::new(PLAYLIST_FILTER_LABEL).glob(PLAYLIST_FILTER_GLOB));
            match dialog.open_file().await {
                Ok(response) => {
                    let url = response.url().clone();
                    match url.to_file_path() {
                        Ok(path) => {
                            let mut state = PlaylistState::new();
                            if state.load(&path) {
                                Message::PlaylistLoaded(Ok(LoadedPlaylist {
                                    source: Some(path),
                                    items: state.items().to_vec(),
                                }))
                            } else {
                                Message::PlaylistLoaded(Err(format!(
                                    "Could not open playlist {}",
                                    path.display()
                                )))
                            }
                        }
                        Err(()) => Message::PlaylistLoaded(Err(format!(
                            "Cannot open {url}: not a local file"
                        ))),
                    }
                }
                Err(cosmic::dialog::file_chooser::Error::Cancelled) => Message::ChooserCancelled,
                Err(why) => Message::PlaylistLoaded(Err(format!("Could not open playlist: {why}"))),
            }
        })
    }

    /// D10 portal chooser `Task`: save dialog with initial `playlist.lst` +
    /// `.lst` filter. The write reuses `PlaylistState::save` verbatim on a
    /// snapshot of the current items (absolute paths, LF).
    ///
    /// Deviation window (T10 finding 3, negligible): C++ reads the live
    /// playlist at dialog-finish (`window.cpp:1559-1564`), while this portal
    /// `Task` moves a snapshot taken at launch — edits made while the dialog
    /// is open would not be saved. Negligible: the window spans only the
    /// seconds the save dialog is up, and the pre-dialog state is exactly
    /// what the user chose to save.
    fn playlist_save_task(items: Vec<PathBuf>) -> Task<Message> {
        cosmic::task::future(async move {
            use cosmic::dialog::file_chooser::{FileFilter, save};
            let dialog = save::Dialog::new()
                .title(PLAYLIST_SAVE_TITLE.to_string())
                .file_name(PLAYLIST_SAVE_INITIAL_NAME.to_string())
                .filter(FileFilter::new(PLAYLIST_FILTER_LABEL).glob(PLAYLIST_FILTER_GLOB));
            match dialog.save_file().await {
                Ok(response) => match response.url().cloned() {
                    Some(url) => match url.to_file_path() {
                        Ok(path) => {
                            let mut state = PlaylistState::new();
                            state.set_items(items);
                            if state.save(&path) {
                                Message::PlaylistSaved(Ok(path))
                            } else {
                                Message::PlaylistSaved(Err(format!(
                                    "Could not save playlist {}",
                                    path.display()
                                )))
                            }
                        }
                        Err(()) => Message::PlaylistSaved(Err(format!(
                            "Cannot save to {url}: not a local file"
                        ))),
                    },
                    None => Message::PlaylistSaved(Err("Could not save playlist".to_string())),
                },
                Err(cosmic::dialog::file_chooser::Error::Cancelled) => Message::ChooserCancelled,
                Err(why) => Message::PlaylistSaved(Err(format!("Could not save playlist: {why}"))),
            }
        })
    }

    /// Rebuild the lyrics body from the loaded song under the current
    /// filters + codec (the recompute half of `refreshLyrics`, WITHOUT the
    /// C++ dropdown/filter snap-back reset — FIX, see `state::lyrics`). No
    /// song → empty texts → the body clears (matches C++ on an empty song).
    fn refresh_lyrics_body(&mut self) {
        if let Some(song) = self.song.as_ref() {
            self.lyrics.refresh_body(&song.texts);
        } else {
            self.lyrics.refresh_body(&[]);
        }
    }

    /// D12 portal save `Task` (`window.cpp:1822-1842`): initial `lyrics.txt`,
    /// no title, no filter (C++ sets neither). Writes the full body+live
    /// text snapshotted at launch; `Cancelled` is the silent
    /// `ChooserCancelled`, write/portal failures toast via `LyricsSaved`.
    ///
    /// Same snapshot rule as D10 (T10 finding 3): C++ reads the live buffer
    /// at dialog-finish (`window.cpp:1832-1836`); lines sung while the dialog
    /// is open are not saved. Negligible (same seconds-long window).
    fn lyrics_save_task(text: String) -> Task<Message> {
        cosmic::task::future(async move {
            use cosmic::dialog::file_chooser::save;
            let dialog = save::Dialog::new().file_name(LYRICS_SAVE_INITIAL_NAME.to_string());
            match dialog.save_file().await {
                Ok(response) => match response.url().cloned() {
                    Some(url) => match url.to_file_path() {
                        Ok(path) => match write_lyrics_file(&path, &text) {
                            Ok(()) => Message::LyricsSaved(Ok(path)),
                            Err(why) => Message::LyricsSaved(Err(why)),
                        },
                        Err(()) => Message::LyricsSaved(Err(format!(
                            "Cannot save to {url}: not a local file"
                        ))),
                    },
                    None => Message::LyricsSaved(Err("Could not save lyrics".to_string())),
                },
                Err(cosmic::dialog::file_chooser::Error::Cancelled) => Message::ChooserCancelled,
                Err(why) => Message::LyricsSaved(Err(format!("Could not save lyrics: {why}"))),
            }
        })
    }

    /// D13 print `Task`: temp-export the full body+live text and open it
    /// with `xdg-open` (DECISIONS D6 choice (a), ux D13 alternative (a) —
    /// listed first there, ahead of the ashpd print portal).
    ///
    /// Portal-vs-`xdg-open` evidence (recorded per T11a): ashpd 0.12.3 DOES
    /// ship `desktop::print::PrintProxy`, reachable via
    /// `cosmic::dialog::ashpd` — but its flow (`prepare_print` → token →
    /// `print` over a document fd, `print.rs` doc example) is oriented at
    /// print-ready documents (PDF): handing it plain `.txt` leans on
    /// backend text-filter support that varies per DE (risk 10/P12), and we
    /// ship no PDF writer. `xdg-open` instead routes the temp `.txt` to the
    /// user's default viewer, which owns its own print path; in-sandbox it
    /// stays correct (xdg-utils `open_flatpak` forwards file opens through
    /// the OpenURI portal, `/usr/bin/xdg-open:872-885`), and the pinned
    /// 25.08 Platform runtime ships both `xdg-open` and `gdbus` (verified in
    /// the installed runtime `files/bin`), so no extra finish-args are
    /// needed. Failures (temp write, missing/spawn-failing opener) toast via
    /// `LyricsPrinted`; success is silent, like the C++ print dialog.
    fn lyrics_print_task(text: String) -> Task<Message> {
        cosmic::task::future(async move { Message::LyricsPrinted(print_lyrics_via_opener(&text)) })
    }

    /// D3 refill `Task` (C++ `refillFn`, `window.cpp:1032-1060`): enumerate
    /// one backend's ports off the UI thread; results arrive as
    /// `MidiPortsRefreshed`. Relaunched on dialog open, backend select,
    /// `Show all` toggle, and Refresh (C++ `:1062-1077` — every trigger
    /// re-reads the backend dropdown, so Refresh re-emits the current
    /// backend with no new variant). No single-flight assumption: each
    /// task carries its requesting backend+advanced as echo (T13g F4), so
    /// two rapid switches landing out of order (each task blocks on ALSA
    /// enumeration) resolve by dropping the stale arrival; C++ is
    /// synchronous and has no equivalent race.
    fn midi_enumerate_task(backend: String, advanced: bool) -> Task<Message> {
        cosmic::task::future(async move {
            let ports = enumerate_ports(&backend, advanced);
            Message::MidiPortsRefreshed {
                backend,
                advanced,
                ports,
            }
        })
    }

    /// D11 portal chooser `Task` (C++ `window.cpp:1091-1119`): single-file
    /// open with the SoundFont filter. `Ok` commits the path into the D3
    /// scratch entry (deferred — Test/Apply carry it); portal cancel maps
    /// to the silent `ChooserCancelled`; any other error toasts.
    fn soundfont_browse_task() -> Task<Message> {
        cosmic::task::future(async move {
            use cosmic::dialog::file_chooser::{FileFilter, open};
            let mut filter = FileFilter::new(SOUNDFONT_FILTER_LABEL);
            for glob in SOUNDFONT_FILTER_GLOBS {
                filter = filter.glob(glob);
            }
            let dialog = open::Dialog::new()
                .title(SOUNDFONT_DIALOG_TITLE)
                .filter(filter);
            match dialog.open_file().await {
                Ok(response) => {
                    let url = response.url().clone();
                    match url.to_file_path() {
                        Ok(path) => Message::MidiSoundFontChosen(Ok(path)),
                        Err(()) => Message::MidiSoundFontChosen(Err(format!(
                            "Cannot open {url}: not a local file"
                        ))),
                    }
                }
                Err(cosmic::dialog::file_chooser::Error::Cancelled) => Message::ChooserCancelled,
                Err(why) => {
                    Message::MidiSoundFontChosen(Err(format!("Could not choose SoundFont: {why}")))
                }
            }
        })
    }

    /// Test-note release `Task` (C++ `g_timeout_add(400, …)`,
    /// `window.cpp:1148-1153`): sleep, then resolve as `MidiTestNoteOff`.
    /// A blocking sleep inside the future, like the other task blocking
    /// halves (iced ships no timer future in the pinned rev; the 50 ms
    /// `Tick` subscription only runs while Playing, so it cannot carry an
    /// idle test-note release). `epoch` is the captured output selection
    /// (T14d minor c).
    fn test_note_off_task(ch: u8, note: u8, epoch: u64, delay_ms: u64) -> Task<Message> {
        cosmic::task::future(async move {
            std::thread::sleep(Duration::from_millis(delay_ms));
            Message::MidiTestNoteOff { ch, note, epoch }
        })
    }

    /// D4 link `Task`: hand the URL to the opener; `Ok` is silent, spawn
    /// failures toast via `UrlOpened` (D13 `LyricsPrinted` precedent).
    fn open_url_task(url: String) -> Task<Message> {
        cosmic::task::future(async move { Message::UrlOpened(open_url_via_opener(&url)) })
    }

    /// Focus-on-open target per dialog (T14d G4 partial): the widget id
    /// `open_dialog` focuses so base Enter cannot fire silently under the
    /// dialog (the `focus(id)` op unfocuses everything else — pinned
    /// `iced/core/src/widget/operation/focusable.rs:30-57`). `None` for the
    /// portal-chooser dialogs (D9/D10/D11/D12 — tasks, no retained sheet to
    /// focus). Pure + unit-pinned over all 11 variants (a 12th variant
    /// fails here, loudly, forcing a focus decision).
    fn dialog_focus_id(dialog: Dialog) -> Option<&'static str> {
        match dialog {
            Dialog::Preferences => Some(PREFS_CLOSE_ID),
            Dialog::MidiSetup => Some(midi_setup::APPLY_ID),
            Dialog::About => Some(info_dialogs::ABOUT_CLOSE_ID),
            Dialog::Help | Dialog::FileInfo => Some(info_dialogs::OK_ID),
            Dialog::JumpToBar => Some(jump_loop::JUMP_CONFIRM_ID),
            Dialog::Loop => Some(jump_loop::LOOP_CONFIRM_ID),
            Dialog::PlaylistOpen
            | Dialog::PlaylistSave
            | Dialog::SoundFontBrowse
            | Dialog::SaveLyrics => None,
        }
    }

    /// Focus-on-open task for `open_dialog` (`Task::none()` when the dialog
    /// has no sheet target). No runtime `unfocus` task wrapper exists in
    /// the pinned rev (only the raw `focusable::unfocus` op — no `Task`
    /// wrapper in `iced/runtime/src/widget/operation.rs`), so focus moves
    /// INTO the dialog rather than clearing.
    fn dialog_focus_task(dialog: Dialog) -> Task<Message> {
        Self::dialog_focus_id(dialog)
            .map_or_else(Task::none, cosmic::iced::widget::operation::focus)
    }

    /// `ShowDialog` body (returns the D3 enumeration task batched with the
    /// focus-on-open task on MidiSetup; the focus task everywhere else —
    /// `Task::none()` only for the gated D7 and the sheet-less portal
    /// dialogs). Opening any dialog abandons the other shells uncommitted
    /// (prefs + D3 scratch — commit rides close / Apply, never a switch).
    /// D7 gates on a loaded song (C++ `showJump`
    /// early-return, `window.cpp:777-778`); D8 opens unconditionally (the
    /// documented `LoopToggled` deviation — a silently dead toggle is a
    /// trap, and U3 forces the menu path through the same arm).
    fn open_dialog(&mut self, dialog: Dialog) -> Task<Message> {
        // T13g F5: the D7 no-song gate runs BEFORE any shell is touched —
        // a gated jump is a pure no-op (a prefs/D3 shell open underneath
        // survives; the gate used to run after `self.prefs = None`).
        if dialog == Dialog::JumpToBar && !jump_allowed(self.song.is_some()) {
            return Task::none();
        }
        if dialog == Dialog::Preferences {
            self.midi_dlg = None;
            self.prefs = Some((
                PrefsViewModel {
                    scratch: self.settings.clone(),
                },
                LyricsPickerModels::from_settings(&self.settings),
            ));
            self.dialog = Some(dialog);
            return Self::dialog_focus_task(dialog);
        }
        self.prefs = None;
        let task = match dialog {
            Dialog::MidiSetup => {
                let scratch = self.midi.clone();
                let task = Self::midi_enumerate_task(scratch.backend.clone(), scratch.advanced);
                self.midi_dlg = Some(MidiSetupDialog {
                    scratch,
                    test_status: None,
                });
                self.dialog = Some(dialog);
                task
            }
            Dialog::JumpToBar => {
                // Gated at the top of `open_dialog` (T13g F5) — reaching
                // here means a song is loaded.
                self.midi_dlg = None;
                self.jump_bar = 1;
                self.dialog = Some(dialog);
                Task::none()
            }
            Dialog::Loop => {
                self.midi_dlg = None;
                self.open_loop_dialog();
                Task::none()
            }
            _ => {
                self.midi_dlg = None;
                self.dialog = Some(dialog);
                Task::none()
            }
        };
        // Focus-on-open (T14d G4 partial): every sheet path moves focus
        // into the dialog (a no-op `Task::none()` leg for the sheet-less
        // portal dialogs).
        Task::batch([task, Self::dialog_focus_task(dialog)])
    }

    /// D8 open half, shared by `ShowDialog(Loop)` and `LoopToggled(true)`
    /// (the U3 menu path emits the same variant — no new variant): seed the
    /// from/to scratch from `loop_start`/`loop_end` (C++ `:852-853`, spins
    /// clamp) and open. Unconditional on song presence (documented
    /// deviation — see `open_dialog`).
    fn open_loop_dialog(&mut self) {
        let bars = self.song.as_ref().map_or(0, |song| song.bars);
        let (from, to) = loop_seeds(self.playback.loop_start, self.playback.loop_end, bars);
        self.loop_from = from;
        self.loop_to = to;
        self.dialog = Some(Dialog::Loop);
    }

    /// D9 result: replace items, record the source in both states, no
    /// autoplay (unlike D1). `Err` is a real I/O error — portal cancel maps
    /// to the silent `ChooserCancelled`, never here — so it gets a toast
    /// (the C++ callback had no error path to preserve).
    fn apply_loaded_playlist(&mut self, list: LoadedPlaylist) {
        if let Some(source) = &list.source {
            self.settings.last_play_list = source.to_string_lossy().into_owned();
        }
        self.playlist.set_items(list.items);
        self.playlist.set_source(list.source);
        self.playlist.set_dirty(false);
    }

    /// T13 dialog sheets (D3/D7/D8/D6/D5/D4): fully-built `dialog()` sheets
    /// for `chrome::wrap_modal`. `None` renders the base alone (no dialog,
    /// D2 (inline above), portal-chooser dialogs (D9/D10/D11/D12 — tasks,
    /// no retained modal state), or a stale shell).
    fn dialog_sheet(&self) -> Option<Element<'_, Message>> {
        match self.dialog {
            Some(Dialog::MidiSetup) => {
                let dlg = self.midi_dlg.as_ref()?;
                let vm = midi_setup::view_model(&dlg.scratch, dlg.test_status.as_deref());
                Some(
                    cosmic::widget::dialog()
                        .title(midi_setup::TITLE)
                        .control(midi_setup::view(vm))
                        .secondary_action(
                            cosmic::widget::button::standard("Cancel")
                                .on_press(Message::CloseDialog),
                        )
                        .into(),
                )
            }
            Some(Dialog::JumpToBar) => {
                let bars = self.song.as_ref().map_or(0, |song| song.bars);
                let vm = jump_loop::jump_view_model(self.jump_bar, bars);
                Some(
                    cosmic::widget::dialog()
                        .title(jump_loop::JUMP_TITLE)
                        .control(jump_loop::jump_view(vm, Message::JumpBarEdited))
                        .secondary_action(
                            cosmic::widget::button::standard("Cancel")
                                .on_press(Message::CloseDialog),
                        )
                        .into(),
                )
            }
            Some(Dialog::Loop) => {
                let bars = self.song.as_ref().map_or(0, |song| song.bars);
                let vm = jump_loop::loop_view_model(self.loop_from, self.loop_to, bars);
                Some(
                    cosmic::widget::dialog()
                        .title(jump_loop::LOOP_TITLE)
                        .control(jump_loop::loop_view(
                            vm,
                            Message::LoopFromEdited,
                            Message::LoopToEdited,
                        ))
                        .secondary_action(
                            cosmic::widget::button::standard("Cancel")
                                .on_press(Message::CloseDialog),
                        )
                        .into(),
                )
            }
            Some(Dialog::FileInfo) => {
                let metadata: &[(String, String)] =
                    self.song.as_ref().map_or(&[], |song| &song.metadata);
                let vm = info_dialogs::file_info_view_model(self.song.as_ref(), metadata);
                Some(
                    cosmic::widget::dialog()
                        .title(info_dialogs::FILE_INFO_TITLE)
                        .control(info_dialogs::file_info_view(vm))
                        .into(),
                )
            }
            Some(Dialog::Help) => Some(
                cosmic::widget::dialog()
                    .title(crate::state::dialogs::HELP_TITLE)
                    .control(info_dialogs::help_view())
                    .into(),
            ),
            Some(Dialog::About) => Some(
                cosmic::widget::dialog()
                    .title(ABOUT_SHEET_TITLE)
                    .control(info_dialogs::about_view(|url: &str| {
                        Message::OpenUrl(url.to_string())
                    }))
                    .into(),
            ),
            _ => None,
        }
    }

    /// D2 prefs sheet: the T12 inline modal chrome, verbatim (kept inline —
    /// see the `view()` note; `wrap_modal` generalizes this shape).
    fn prefs_sheet<'a>(
        base: Element<'a, Message>,
        vm: &'a PrefsViewModel,
        pickers: &'a LyricsPickerModels,
    ) -> Element<'a, Message> {
        let sheet: Element<'a, Message> = cosmic::iced::widget::mouse_area(
            cosmic::widget::container(
                cosmic::widget::dialog()
                    .title("Preferences")
                    .control(cosmic::widget::scrollable(prefs::view(
                        vm,
                        pickers,
                        Message::PrefsScratchEdited,
                        [
                            Message::PrefsPickerFuture,
                            Message::PrefsPickerPast,
                            Message::PrefsPickerHighlight,
                        ],
                    )))
                    .primary_action(
                        cosmic::widget::button::standard("Close")
                            .on_press(Message::CloseDialog)
                            .id(cosmic::iced::core::widget::Id::new(PREFS_CLOSE_ID)),
                    ),
            )
            .style(|_theme| cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(cosmic::iced::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.45,
                })),
                ..Default::default()
            })
            .width(cosmic::iced::Length::Fill)
            .height(cosmic::iced::Length::Fill)
            .align_x(cosmic::iced::Alignment::Center)
            .align_y(cosmic::iced::Alignment::Center),
        )
        .on_scroll(|_| Message::ChooserCancelled)
        .into();
        cosmic::iced::widget::Stack::with_children(vec![base, sheet])
            .width(cosmic::iced::Length::Fill)
            .height(cosmic::iced::Length::Fill)
            .into()
    }

    // -- engine → UI --------------------------------------------------------------

    /// Engine event handling (arch §2.3): the 12 callback twins plus
    /// open/load/output results. `Time` is latest-wins coalescing (the
    /// forwarder already keeps only the newest per poll); notes/programs/
    /// text/beat/transport arrive losslessly in order. Returns the
    /// lyrics-body snap task when a live text append is accepted (gated on
    /// `autoscroll`), `Task::none()` everywhere else.
    fn on_player_event(&mut self, event: PlayerEvent) -> Task<Message> {
        match event {
            PlayerEvent::Opened(snapshot) => self.on_opened(LoadedSong::from(*snapshot)),
            PlayerEvent::LoadFailed { file, errors } => {
                self.pending_autoplay = false;
                let name = Path::new(&file)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(file);
                self.toast(format!("Could not load {name}"));
                if !errors.is_empty() {
                    self.toast("Warning: non-standard or damaged file".to_string());
                }
            }
            PlayerEvent::Time { elapsed_ms, ticks } => {
                self.playback.elapsed_ms = elapsed_ms;
                self.playback.position_ticks = ticks;
                // The seek landed: clear the in-flight marker.
                self.playback.seeking = false;
            }
            PlayerEvent::Beat { bar, beat, max } => {
                self.playback.bar = bar;
                self.playback.beat = beat;
                self.playback.beats_per_bar = max;
            }
            PlayerEvent::NoteOn { ch, note, vel } => {
                self.piano.note_on(ch, note, vel);
                if (ch as usize) < MIDI_CHANNELS {
                    let meter = &mut self.channels[ch as usize].level;
                    *meter = channels::note_peak(*meter, vel);
                }
            }
            PlayerEvent::NoteOff { ch, note, .. } => {
                self.piano.note_off(ch, note);
            }
            PlayerEvent::Text {
                track,
                text_type,
                ticks: _,
                bytes,
            } => {
                let text = Sequence::decode_with_charset(&bytes, &self.lyrics.codec);
                if self.lyrics.append_live(track, text_type, &text) && self.lyrics.autoscroll {
                    return lyrics::snap_to_end_task();
                }
            }
            PlayerEvent::Program { ch, patch } => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].patch = patch.min(127);
                }
            }
            PlayerEvent::Tempo { tempo_us } => {
                self.playback.note_tempo_event(tempo_us);
            }
            // No UI handler in C++ either (`onTimeSignature` is a no-op
            // comment: lamps resize on the next beat — `window.cpp:189-192`).
            PlayerEvent::KeySignature { .. } | PlayerEvent::TimeSignature { .. } => {}
            PlayerEvent::Started => {
                self.playback.status = Status::Playing;
            }
            PlayerEvent::Stopped => {
                self.playback.status = Status::Stopped;
            }
            PlayerEvent::Finished => self.on_finished(),
            PlayerEvent::OutputChanged {
                backend,
                port,
                warning,
            } => {
                self.midi.backend = backend;
                self.midi.port = port;
                if let Some(warning) = warning {
                    // A failing Test-note reselect surfaces verbatim in the
                    // D3 status line (C++ `:1143-1145`), alongside the toast.
                    // Guarded on the open shell: Apply closes synchronously,
                    // so its failures never touch a dead scratch.
                    if let Some(dlg) = self.midi_dlg.as_mut() {
                        dlg.test_status = Some(NO_MIDI_OUTPUT.to_string());
                    }
                    self.toast(warning);
                    // Startup fallback chain only (ctor parity): once any
                    // output has been confirmed, later failures (user
                    // applies) toast without second-guessing the selection.
                    if !self.midi.output_ok_once
                        && let Some((fallback, port)) = self.midi.next_fallback()
                    {
                        self.send_select_output(fallback, port);
                    }
                } else {
                    self.midi.output_ok_once = true;
                    if self.midi.announce_applied {
                        self.midi.announce_applied = false;
                        self.toast(if self.midi.port.is_empty() {
                            format!("MIDI output: {}", self.midi.backend)
                        } else {
                            format!("MIDI output: {} {}", self.midi.backend, self.midi.port)
                        });
                    }
                }
            }
        }
        Task::none()
    }

    // -- misc ---------------------------------------------------------------------

    /// Push a toast; the stack is capped (`MAX_TOASTS`, oldest dropped).
    fn toast(&mut self, text: String) {
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(text);
    }

    /// Build the transport view model for `view()`. The loop toggle
    /// derives from `loop_enabled` — forced ON while the D8 dialog is open
    /// (U3/C++ `:869,873-880`: confirm keeps it ON, cancel resets it; the
    /// modal chrome blocks base clicks, so the forced-ON toggle cannot emit
    /// a spurious `LoopToggled(false)`).
    fn transport_view_model(&self) -> TransportViewModel {
        TransportViewModel {
            song_title: self
                .song
                .as_ref()
                .map_or("No file loaded".to_string(), |s| s.title.clone()),
            status: self.playback.status,
            time_text: playback::format_time_ms(self.playback.elapsed_ms),
            tempo_text: playback::format_bpm(self.playback.bpm),
            volume_text: format!("{}%", self.playback.volume_pct),
            pitch: self.playback.pitch_semitones,
            bar_beat: format!("{}:{}", self.playback.bar, self.playback.beat),
            beats: self.playback.beats_per_bar,
            beat: self.playback.beat,
            position: self.playback.position_fraction(),
            seeking: self.playback.seeking,
            tempo_pct: self.playback.tempo_factor_pct,
            volume_pct: self.playback.volume_pct,
            loop_enabled: self.playback.loop_enabled || self.dialog == Some(Dialog::Loop),
            has_song: self.song.is_some(),
        }
    }

    /// Build the playlist sidebar view model for `view()`: full paths in
    /// playlist order + `current_index` as `Option<usize>` (`-1` → `None`).
    fn playlist_view_model(&self) -> PlaylistViewModel {
        PlaylistViewModel {
            items: self.playlist.items().to_vec(),
            selected: usize::try_from(self.playlist.current_index()).ok(),
        }
    }

    /// Build the lyrics page view model for `view()` (T11b contract): owned
    /// dropdown labels + `*_selected` indices per the T11a formulas, the U10
    /// body/live segments verbatim, the snap trigger + gate, the U14 applied
    /// D2 rows, and the empty-state switch.
    fn lyrics_view_model(&self) -> LyricsViewModel {
        let texts: &[crate::core::sequence::TextRec] = self.song.as_ref().map_or(&[], |s| &s.texts);
        let num_tracks = self.song.as_ref().map_or(0, |s| s.num_tracks);
        LyricsViewModel {
            track_options: track_options(texts, num_tracks, &self.lyrics.codec),
            track_selected: self.lyrics.track_filter.map_or(0, |t| t as usize),
            type_options: TYPE_OPTION_NAMES.iter().map(ToString::to_string).collect(),
            type_selected: self.lyrics.type_filter.max(0) as usize,
            codec_options: Sequence::extra_codec_names(),
            codec_selected: codec_option_index(&self.lyrics.codec),
            body: self.lyrics.body.clone(),
            live_past: self.lyrics.live_past.clone(),
            live_current: self.lyrics.live_current.clone(),
            sung: self.lyrics.sung,
            live_seq: self.lyrics.live_seq,
            autoscroll: self.lyrics.autoscroll,
            applied: self.lyrics.applied.clone(),
            has_song: self.song.is_some(),
        }
    }

    /// Build the channels page view model for `view()` (T12b contract):
    /// used channels ONLY (`ChannelState::used`), in channel order (C++
    /// `refreshChannels` visibility rule) — the view renders every row it is
    /// given and filters nothing itself. Patch labels come from
    /// `patch_label` under the current instrument map; the shared 128-entry
    /// model relabels on map change (prefs close / MIDI apply), while live
    /// `Program` reselects only move `patch` state-side.
    fn channels_view_model(&self) -> ChannelsViewModel {
        let map = self.settings.instrument_map;
        let rows = self
            .channels
            .iter()
            .enumerate()
            .filter(|(_, row)| row.used)
            .map(|(ch, row)| ChannelRowViewModel {
                channel: ch as u8,
                name: row.label.clone(),
                muted: row.muted,
                soloed: row.soloed,
                locked: row.locked,
                meter: row.level,
                level_pct: row.level_pct,
                patch: row.patch,
                patch_label: channels::patch_label(map, row.patch).to_string(),
            })
            .collect();
        ChannelsViewModel {
            rows,
            patch_options: channels::patch_options(map),
            has_song: self.song.is_some(),
        }
    }
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "com.goshapps.GoshMIDI";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        // Portable mode (`setPortable`, `main.cpp:102-103`): `-p` / `-f FILE`
        // select the portable config path, otherwise the XDG config path.
        let paths = match flags.portable_config() {
            Some(file) => Paths::portable_from_env(file),
            None => Paths::from_env(),
        };
        let settings = Settings::load(&paths);
        let (engine, receiver, _engine_thread) = spawn_engine();
        let engine_sub = EngineSubId(NEXT_ENGINE_ID.fetch_add(1, Ordering::Relaxed));
        engine_registry()
            .lock()
            .expect("engine registry lock")
            .insert(engine_sub.0, receiver);
        let mut app = Self {
            core,
            engine,
            engine_sub,
            playback: PlaybackState::new(),
            song: None,
            playlist: PlaylistState::new(),
            channels: channels::default_mix(),
            lyrics: LyricsState::new(),
            piano: PianoState::new(),
            midi: MidiState::new(),
            settings,
            paths,
            dialog: None,
            prefs: None,
            midi_dlg: None,
            jump_bar: 1,
            loop_from: 1,
            loop_to: 1,
            view: View::Lyrics,
            toasts: Vec::new(),
            pending_autoplay: false,
            output_epoch: 0,
        };
        app.core.set_header_title("Gosh MIDI Player".to_owned());
        app.playback.repeat = RepeatMode::from_i32(app.settings.repeat_mode);
        app.apply_lyrics_prefs();
        app.startup_select_output(&flags);
        if flags.files.is_empty() {
            app.restore_playlist();
        } else {
            app.open_files(flags.files, true);
        }
        (app, Task::none())
    }

    fn context_drawer(&self) -> Option<ContextDrawer<'_, Self::Message>> {
        None
    }

    /// Hamburger menu at the header end (T14d composition of the T14b
    /// 23-item `menu::bar` — C++ packs the `open-menu-symbolic` menu button
    /// at the header end, `window.cpp:1584,1618`). Single root; the VM is
    /// inventory-only (T14d retired the `quit` callback for `Message::Quit`).
    fn header_end(&self) -> Vec<Element<'_, Self::Message>> {
        vec![menu::view(MenuViewModel {})]
    }

    // No `footer()` override (T9 status-dup resolved at T10a2 composition):
    // the transport region already renders the status line (ux §3.9), so the
    // COSMIC footer stays at its `None` default — exactly one status line.

    fn subscription(&self) -> Subscription<Self::Message> {
        let engine = Subscription::run_with(self.engine_sub, run_engine_stream);
        let resize = cosmic::iced::event::listen_with(window_event_message);
        // Keyboard (T14d over T14a `state::keys`): ignored-status events
        // ONLY (pinned `iced/futures/src/keyboard.rs:7-19` — a focused
        // button consumes Enter and a focused text input consumes text
        // keys, so those never arrive here and cannot double-fire). The
        // `KeyEvent` arm resolves each event against the live
        // dialog/selection context (dialog gate + Esc/Tab + 9 accelerators
        // + Enter-play, lead ruling D22: no invented bindings).
        let keyboard = cosmic::iced::keyboard::listen().map(Message::KeyEvent);
        // Intended deviation from C++ (`m_meterTimer` is always on): the
        // 50 ms meter-decay ticker runs only while Playing. Visual-only
        // difference — paused/stopped meters freeze instead of decaying —
        // and idle CPU stays at zero.
        if self.playback.status == Status::Playing {
            Subscription::batch([
                engine,
                cosmic::iced::time::every(TICK_INTERVAL).map(Message::Tick),
                resize,
                keyboard,
            ])
        } else {
            Subscription::batch([engine, resize, keyboard])
        }
    }

    /// State transitions + engine commands (arch §2.3). Transport/mix/seek/
    /// loop messages update state optimistically and send the matching
    /// command; D1/D9/D10/D11/D12 launch portal chooser `Task`s, copy/print
    /// launch clipboard/export `Task`s, D3 refill/test-note and D4 links
    /// launch their `Task`s, picker updates return the picker's mapped task
    /// — every arm is covered by a test below.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            // --- transport ---
            Message::Play => self.start_playback(),
            Message::Pause => self.toggle_pause(),
            Message::Stop => self.stop_playback(),
            Message::NextSong => self.next_song(),
            Message::PrevSong => self.prev_song(),
            Message::BeatForward => self.engine.send(EngineCommand::BeatStep(1)),
            Message::BeatBackward => self.engine.send(EngineCommand::BeatStep(-1)),
            Message::JumpToBar(bar) => {
                // D7 pause-jump-resume (`window.cpp:802-805`): the engine
                // drains commands in order, so Pause → jump → Play reproduces
                // the C++ sequence across the thread boundary. The bar clamps
                // to `1..=last_bar` like the C++ spin (`clamp_jump_bar` —
                // the view already clamps for display; this pins the arm).
                let last_bar = self.song.as_ref().map_or(1, |song| song.bars);
                let bar = clamp_jump_bar(i32::try_from(bar).unwrap_or(i32::MAX), last_bar);
                let resume = self.playback.status == Status::Playing;
                if resume {
                    self.engine.send(EngineCommand::Pause);
                }
                self.engine.send(EngineCommand::JumpToBar(bar));
                if resume {
                    self.engine.send(EngineCommand::Play);
                }
                self.dialog = None;
            }
            Message::LoopToggled(enabled) => {
                if enabled {
                    // D8: toggle ON opens the range dialog; the engine
                    // command goes out on `LoopSet` confirm. The U3 menu
                    // path emits this same variant (T13a: no new variant —
                    // menu Loop forces toggle ON, then opens).
                    // Intended deviation from C++ (`showLoop` early-returns
                    // with no song loaded, `window.cpp:832-834`): the dialog
                    // opens unconditionally instead of a silently dead
                    // toggle. Deliberate; the D8 cancel rule still applies.
                    // T13d F2(a): route through `open_dialog` so the toggle
                    // path abandons the D3/prefs shells exactly like
                    // `ShowDialog(Loop)` (the T12 switch rule — C++ `showLoop`
                    // / `showMidiSetup` are sibling modals over a blocked main
                    // window, `window.cpp:839-841` / `:948-952`, so C++ never
                    // opens Loop above a live MIDI shell at all).
                    return self.open_dialog(Dialog::Loop);
                } else {
                    self.playback.loop_enabled = false;
                    let start = self.playback.loop_start;
                    let end = self.playback.loop_end;
                    self.engine.send(EngineCommand::SetLoop {
                        enabled: false,
                        start,
                        end,
                    });
                }
            }
            Message::LoopSet { start, end } => {
                // D8 confirm: clamp each end to `1..=last_bar`
                // (`loop_confirm` — deliberately NO from≤to enforcement,
                // C++ `setLoop` assigns blindly), enable, force the header
                // toggle ON via `loop_enabled`, and close (`window.cpp:864-871`).
                let last_bar = self.song.as_ref().map_or(0, |song| song.bars);
                let (start, end) = loop_confirm(
                    i32::try_from(start).unwrap_or(i32::MAX),
                    i32::try_from(end).unwrap_or(i32::MAX),
                    last_bar,
                );
                self.playback.loop_enabled = true;
                self.playback.loop_start = start;
                self.playback.loop_end = end;
                self.engine.send(EngineCommand::SetLoop {
                    enabled: true,
                    start,
                    end,
                });
                self.dialog = None;
            }
            // D7/D8 spin scratch (stored verbatim — the spins only emit
            // in-range values; the views clamp for display and the confirm
            // arms clamp via `clamp_jump_bar`/`loop_confirm`).
            Message::JumpBarEdited(bar) => self.jump_bar = bar,
            Message::LoopFromEdited(from) => self.loop_from = from,
            Message::LoopToEdited(to) => self.loop_to = to,
            Message::RepeatModeChanged(mode) => {
                self.playback.repeat = mode;
                // Persisted at close (U2 save-point row 1), like `m_repeat`.
                self.settings.repeat_mode = mode.as_i32();
            }
            Message::PositionSeek(frac) => {
                let len = self.playback.song_len_ticks;
                if len > 0 {
                    let ticks = (frac.clamp(0.0, 1.0) * len as f64) as i64;
                    self.playback.seeking = true;
                    self.playback.position_ticks = ticks;
                    self.engine.send(EngineCommand::SeekTicks(ticks));
                }
            }
            Message::TempoChanged(pct) => {
                let pct = self.playback.set_tempo_pct(pct);
                self.engine
                    .send(EngineCommand::SetTempoFactor(f64::from(pct) / 100.0));
                self.playback.bpm = self.playback.live_bpm();
            }
            Message::VolumeChanged(pct) => {
                let pct = self.playback.set_volume_pct(pct);
                self.engine.send(EngineCommand::SetVolumeFactor(pct));
            }
            Message::PitchChanged(semis) => {
                let semis = self.playback.set_pitch(semis);
                self.engine.send(EngineCommand::SetPitchShift(semis));
            }

            // --- playlist ---
            Message::OpenFiles(paths, replace) => self.open_files(paths, replace),
            Message::OpenFilesDialog => return Self::open_files_task(),
            Message::OpenFilesChosen(result) => match result {
                Ok(paths) => {
                    if !paths.is_empty() {
                        self.open_files(paths, true);
                    }
                }
                Err(error) => self.toast(error),
            },
            // Select-only (click-select, U11); activation is the next arm.
            Message::PlaylistSelect(i) => {
                self.playlist.set_current_index(i as i32);
            }
            // Row-activated (double-click/Enter, `window.cpp:1572-1578`):
            // select + load + play. An out-of-range index is ignored by the
            // select (A14) and the load still targets the current item — C++.
            Message::PlaylistRowActivated(i) => {
                self.playlist.set_current_index(i as i32);
                self.load_current(true);
            }
            Message::PlaylistRemove(i) => self.playlist.remove_at(i as i32),
            Message::PlaylistMoveUp(i) => self.playlist.move_up(i as i32),
            Message::PlaylistMoveDown(i) => self.playlist.move_down(i as i32),
            Message::PlaylistShuffle => self.playlist.shuffle(),
            Message::PlaylistClear => self.playlist.clear(),
            Message::PlaylistLoadDialog => return Self::playlist_load_task(),
            Message::PlaylistSaveDialog => {
                return Self::playlist_save_task(self.playlist.items().to_vec());
            }
            Message::PlaylistLoaded(result) => match result {
                Ok(list) => self.apply_loaded_playlist(list),
                Err(error) => self.toast(error),
            },
            Message::PlaylistSaved(result) => match result {
                Ok(path) => {
                    self.settings.last_play_list = path.to_string_lossy().into_owned();
                    self.playlist.set_source(Some(path));
                    self.playlist.set_dirty(false);
                }
                Err(error) => self.toast(error),
            },
            // Portal `Cancelled` from any chooser: silent no-op.
            Message::ChooserCancelled => {}

            // --- drag-and-drop (same `openFiles(replace)` path as D1) ---
            Message::FilesDropped(paths) => self.dropped_files(paths),
            Message::DndDataReceived { mime, data } => {
                if mime == DND_URI_LIST_MIME {
                    self.dropped_files(parse_uri_list(&data));
                }
            }
            Message::DndFileTransfer(key) => {
                return cosmic::command::file_transfer_receive(key).map(|result| {
                    Action::App(match result {
                        Ok(files) => Message::DndFileTransferDone(Ok(files
                            .into_iter()
                            .map(PathBuf::from)
                            .collect())),
                        Err(why) => Message::DndFileTransferDone(Err(format!(
                            "Could not receive dropped files: {why}"
                        ))),
                    })
                });
            }
            Message::DndFileTransferDone(result) => match result {
                Ok(paths) => self.dropped_files(paths),
                Err(error) => self.toast(error),
            },

            // --- per-channel mix ---
            Message::ChannelMute(ch, muted) => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].muted = muted;
                    self.push_channel(ch);
                }
            }
            Message::ChannelSolo(ch, soloed) => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].soloed = soloed;
                    // A solo flip changes every row's factor: push all.
                    self.push_all_channels();
                }
            }
            Message::ChannelLock(ch, locked) => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].locked = locked;
                    self.engine.send(EngineCommand::SetLocked(ch, locked));
                }
            }
            Message::ChannelPatch(ch, patch) => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].patch = patch.min(127);
                    self.engine
                        .send(EngineCommand::SetPatch(ch, patch.min(127)));
                }
            }
            Message::ChannelLevel(ch, pct) => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].set_level_pct(pct);
                    self.push_channel(ch);
                }
            }
            Message::ChannelName(ch, name) => {
                if (ch as usize) < MIDI_CHANNELS {
                    self.channels[ch as usize].label = name;
                }
            }

            // --- lyrics ---
            // Filter/codec changes recompute the body under the current
            // selections (snap-back FIX, see `state::lyrics`); empty codec
            // is ignored (`setCurrentCharset`, `sequence.cpp:362-366`).
            Message::LyricTrackFilter(track) => {
                // T11 follow-up 1: `Some(0)` normalizes to `None` (C++
                // `m_lyricTrackFilter` is an int where only `> 0` filters —
                // `window.cpp:158,594` — so 0 passes all, while `Some(0)`
                // would match nothing. Unreachable via the UI: the dropdown
                // maps index 0 to `None`).
                self.lyrics.track_filter = track.filter(|&t| t != 0);
                self.refresh_lyrics_body();
            }
            Message::LyricTypeFilter(typ) => {
                self.lyrics.type_filter = typ;
                self.refresh_lyrics_body();
            }
            Message::LyricCodec(codec) => {
                if !codec.is_empty() {
                    self.lyrics.codec = codec;
                    self.refresh_lyrics_body();
                }
            }
            // Copy writes the whole buffer text (`window.cpp:1811-1820`).
            Message::LyricsCopy => return cosmic::iced::clipboard::write(self.lyrics.full_text()),
            Message::LyricsSaveDialog => return Self::lyrics_save_task(self.lyrics.full_text()),
            Message::LyricsPrint => return Self::lyrics_print_task(self.lyrics.full_text()),
            // Save success is silent (C++ shows nothing); errors toast (FIX —
            // C++ ignores the write result).
            Message::LyricsSaved(result) => {
                if let Err(error) = result {
                    self.toast(error);
                }
            }
            // Print success is silent (C++ print dialog shows no toast);
            // temp-write/opener failures toast.
            Message::LyricsPrinted(result) => {
                if let Err(error) = result {
                    self.toast(error);
                }
            }

            // --- piano ---
            Message::PianoTighten(tighten) => self.piano.tighten = tighten,
            Message::PianoShowAll => self.piano.show_all(),
            Message::PianoHideUnused => {
                let used: [bool; MIDI_CHANNELS] = std::array::from_fn(|ch| self.channels[ch].used);
                self.piano.hide_unused(&used);
            }
            Message::PianoPreviewOn(ch, note) => {
                // A3: audition lights the key at vel 80 AND sends the raw
                // NoteOn straight to the output via `PreviewNote` (D19
                // refinement — engine-carried, bypassing mute + pitch like
                // the C++ direct `sendNoteOn`, `window.cpp:2099`). Both legs
                // sit inside the C++ `current()` guard (T14d minor b —
                // `window.cpp:2098-2102` lights the matrix only with an
                // output): without a confirmed output the engine send would
                // no-op while the key lit anyway — a lit key with no sound.
                // The port's guard is `output_ok_once` ("an output confirmed
                // working"): the only divergence from the letter of C++ (any
                // select attempted, success or fail) is the failure window,
                // where C++ lights a key that cannot sound and the port
                // honestly stays dark.
                if self.midi.output_ok_once {
                    let preview = PreviewNote::audition(ch, note);
                    self.piano.note_on(preview.ch, preview.note, preview.vel);
                    self.engine.send(EngineCommand::PreviewNote {
                        ch: preview.ch,
                        note: preview.note,
                        vel: preview.vel,
                    });
                }
            }
            Message::PianoPreviewOff => {
                // A3 idle-only note-off (`window.cpp:2105-2116`): when idle
                // no song notes can be lit (stop/load clear the matrix), so
                // every lit key is a preview target — send the raw NoteOffs
                // via `PreviewOff` and clear; while playing, releases must
                // not kill song notes (no send, no clear).
                let targets = preview_off_targets(
                    &self.piano.notes_on,
                    self.playback.status == Status::Playing,
                );
                if !targets.is_empty() {
                    self.engine
                        .send(EngineCommand::PreviewOff { notes: targets });
                    self.piano.clear();
                }
            }

            // --- MIDI setup (all scratch-scoped: no shell → silent no-op,
            // prefs precedent; live `midi` mirrors move only on Test/Apply) ---
            Message::MidiBackendSelected(backend) => {
                if let Some(dlg) = self.midi_dlg.as_mut() {
                    dlg.scratch.backend = backend.clone();
                    // The port list belongs to the old backend: clear and
                    // refill (C++ `refillFn` rebuilds on backend notify,
                    // `window.cpp:1063-1067`; Refresh re-emits the CURRENT
                    // backend, so it retriggers through this same arm).
                    dlg.scratch.ports.clear();
                    let advanced = dlg.scratch.advanced;
                    return Self::midi_enumerate_task(backend, advanced);
                }
            }
            Message::MidiPortsRefreshed {
                backend,
                advanced,
                ports,
            } => {
                // Refill completion: store the ports (the dropdown preselect
                // derives render-time via `select_refilled_port`) and clear
                // the transient test status (C++ refill overwrites the
                // status label, `window.cpp:1055-1058`). Deliberately NO
                // profile inference here — the C++ `filling` guard
                // (`window.cpp:1080-1081`). Late results after close drop;
                // stale results (echo mismatch after a rapid backend/`Show
                // all` switch — T13g F4) drop too, keeping the current
                // request's list.
                if let Some(dlg) = self.midi_dlg.as_mut()
                    && backend == dlg.scratch.backend
                    && advanced == dlg.scratch.advanced
                {
                    dlg.scratch.ports = ports;
                    dlg.test_status = None;
                }
            }
            Message::MidiPortSelected(port) => {
                if let Some(dlg) = self.midi_dlg.as_mut() {
                    dlg.scratch.port = port.clone();
                    // User-initiated pick → map+reset follow the port label
                    // (C++ `window.cpp:1082-1087`, via `profile_for_port`).
                    // Unknown ids (stale/defensive) set the want only.
                    let label = dlg
                        .scratch
                        .ports
                        .iter()
                        .find(|p| p.id == port)
                        .map(|picked| picked.label.clone());
                    if let Some(label) = label {
                        let (map, reset) = profile_for_port(&label);
                        dlg.scratch.instrument_map = map;
                        dlg.scratch.sysex_reset = reset;
                    }
                }
            }
            Message::MidiAdvancedToggled(advanced) => {
                if let Some(dlg) = self.midi_dlg.as_mut() {
                    dlg.scratch.advanced = advanced;
                    // `Show all` retriggers the refill (C++ `:1068-1072`).
                    dlg.scratch.ports.clear();
                    let backend = dlg.scratch.backend.clone();
                    return Self::midi_enumerate_task(backend, advanced);
                }
            }
            Message::MidiInstrumentMapChanged(map) => {
                if let Some(dlg) = self.midi_dlg.as_mut() {
                    dlg.scratch.instrument_map = map.min(2);
                }
            }
            Message::MidiResetChanged(reset) => {
                if let Some(dlg) = self.midi_dlg.as_mut() {
                    dlg.scratch.sysex_reset = reset.min(4);
                }
            }
            Message::MidiSoundFontChanged(path) => {
                if let Some(dlg) = self.midi_dlg.as_mut() {
                    dlg.scratch.soundfont = path;
                }
            }
            Message::MidiSoundFontBrowse => {
                if self.midi_dlg.is_some() {
                    return Self::soundfont_browse_task();
                }
            }
            Message::MidiSoundFontChosen(result) => match result {
                // Entry-only, deferred commit (C++ `:1111-1113` writes the
                // entry; Test/Apply carry it to the engine).
                Ok(path) => {
                    if let Some(dlg) = self.midi_dlg.as_mut() {
                        dlg.scratch.soundfont = path.to_string_lossy().into_owned();
                    }
                }
                Err(error) => self.toast(error),
            },
            Message::MidiTestNote => return self.test_midi_note(),
            Message::MidiTestNoteOff { ch, note, epoch } => {
                // Captured-output release (T14d minor c — C++ `g_timeout`,
                // `:1148-1153`, sends the off to the CAPTURED `out`, and has
                // no idle gate, unlike piano release): send only when the
                // selection is untouched since the NoteOn, so the off always
                // addresses the output the note went to. A stale epoch means
                // a stop-first reselect (Apply/prefs/test — the realistic
                // Apply-within-400ms case stays covered by that `Stop`, as
                // before) already silenced the old output, or a post-failure
                // fallback reselected an output that never sounded — either
                // way skip, never misfire at the new current. Silent by
                // design. (An identity compare would misfire here: ALSA
                // `open("")` falls back to the first advanced port (A6b), so
                // the RESOLVED port legitimately differs from the requested
                // one — the epoch is immune to that.)
                if epoch == self.output_epoch {
                    self.engine.send(EngineCommand::PreviewOff {
                        notes: vec![(ch, note)],
                    });
                }
            }
            Message::MidiApply => self.apply_midi_setup(),

            // --- preferences / song settings ---
            Message::PrefsChanged(prefs) => self.apply_prefs(prefs),
            // Scratch-only edit: adopt + re-drive the picker models. Live
            // settings, engine, disk untouched — the single commit happens
            // at close. No shell (stale message) → silent no-op.
            Message::PrefsScratchEdited(next) => {
                if let Some((vm, pickers)) = self.prefs.as_mut() {
                    adopt_prefs_scratch(pickers, &mut vm.scratch, next);
                }
            }
            // One arm per color row (the picker builder takes `fn` pointers,
            // so the row rides the variant): feed the dialog update into the
            // model, write the canonical text on apply events. Returns the
            // picker's mapped task (its copy-to-clipboard arm needs it).
            Message::PrefsPickerFuture(update) => {
                if let Some((vm, pickers)) = self.prefs.as_mut() {
                    return apply_picker_update(
                        pickers,
                        &mut vm.scratch,
                        LyricsColorRow::Future,
                        update,
                    );
                }
            }
            Message::PrefsPickerPast(update) => {
                if let Some((vm, pickers)) = self.prefs.as_mut() {
                    return apply_picker_update(
                        pickers,
                        &mut vm.scratch,
                        LyricsColorRow::Past,
                        update,
                    );
                }
            }
            Message::PrefsPickerHighlight(update) => {
                if let Some((vm, pickers)) = self.prefs.as_mut() {
                    return apply_picker_update(
                        pickers,
                        &mut vm.scratch,
                        LyricsColorRow::Highlight,
                        update,
                    );
                }
            }
            // Manual song-settings actions (C++ `app.save/load-settings`,
            // `window.cpp:1405-1408`): gated on a loaded song
            // (`manual_song_action_allowed` — the `:633-635`/`:663-665`
            // empty-name guards), UNGATED by `auto_song_settings` (the menu
            // actions call save/load directly; the auto flag drives only the
            // finish/load/next-song call sites).
            Message::SongSettingsSave => {
                if manual_song_action_allowed(self.song.as_ref().map(|song| song.title.as_str())) {
                    self.save_song_settings();
                }
            }
            Message::SongSettingsLoad => {
                if manual_song_action_allowed(self.song.as_ref().map(|song| song.title.as_str())) {
                    self.load_song_settings();
                }
            }

            // --- dialogs / navigation ---
            Message::ShowDialog(dialog) => return self.open_dialog(dialog),
            // D8 cancel rule falls out of the model: the loop toggle derives
            // from `loop_enabled` (forced ON while the dialog is open), so
            // closing an unconfirmed dialog resets it. D3 close abandons the
            // scratch unapplied (Cancel parity — C++ destroys `Widgets`).
            // D2 commit-on-close: narrow the scratch to exactly the 17
            // D2-owned keys over live settings (T12 devil-BLOCKING FIX — a
            // wholesale live←scratch replace would silently revert live
            // writes that land while the dialog is open: `WindowResized`,
            // `on_opened` recents, `PlaylistSaved`), then ONE `apply_prefs`
            // of the merged commit (U14 mix repush + save), then clear the
            // shell. Other dialogs just close.
            Message::CloseDialog => {
                if self.dialog == Some(Dialog::Preferences)
                    && let Some((vm, _)) = self.prefs.take()
                {
                    let mut committed = self.settings.clone();
                    committed.commit_prefs_dialog(&vm.scratch);
                    self.apply_prefs(committed);
                }
                self.midi_dlg = None;
                self.dialog = None;
            }
            // D4 link: hand the URL to the opener (D13 mechanism + sandbox
            // coverage — see `URL_OPENER`); failures toast, success silent.
            Message::OpenUrl(url) => return Self::open_url_task(url),
            Message::UrlOpened(result) => {
                if let Err(error) = result {
                    self.toast(error);
                }
            }
            // T13g F3: through `open_dialog` so the prefs/D3 shells are
            // abandoned like every other dialog switch (the F2(a) hazard).
            Message::ShowFileInfo => return self.open_dialog(Dialog::FileInfo),
            Message::ShowAbout => return self.open_dialog(Dialog::About),
            Message::ShowHelp => return self.open_dialog(Dialog::Help),
            Message::ViewSelected(view) => self.view = view,
            Message::Toast(text) => self.toast(text),
            Message::DismissToast => {
                if !self.toasts.is_empty() {
                    self.toasts.remove(0);
                }
            }

            // --- keyboard (T14d composition over `state::keys`) ---
            Message::KeyEvent(event) => {
                // Resolve against the LIVE dialog + selection (the
                // subscription only forwards the raw event — it cannot see
                // current state), then route. `FocusNext`/`FocusPrevious`
                // are focus `Task`s (no message exists for them); `Quit`
                // recurses into the arm below (`KeyAction::message` predates
                // the variant and still returns `None` for it — keys.rs
                // behavior is untouched by design, see the T14d report);
                // the rest recurse through `update` (T14a pseudocode).
                // Exactly the 9 C++ accelerators route here — no invented
                // bindings (lead ruling D22; the 11 menu-only actions stay
                // tracked as G3).
                return match resolve_key(
                    &event,
                    self.dialog,
                    usize::try_from(self.playlist.current_index()).ok(),
                ) {
                    None => Task::none(),
                    Some(KeyAction::FocusNext) => cosmic::iced::widget::operation::focus_next(),
                    Some(KeyAction::FocusPrevious) => {
                        cosmic::iced::widget::operation::focus_previous()
                    }
                    Some(KeyAction::Quit) => self.update(Message::Quit),
                    Some(action) => match action.message() {
                        Some(msg) => self.update(msg),
                        // Unreachable: only Quit/Focus* map to `None`
                        // (`KeyAction::message`), and those arms sit above.
                        None => Task::none(),
                    },
                };
            }
            // `app.quit` (`window.cpp:1375-1376`): close the main window.
            // T14a proposal adopted — mirrors the `gain_focus` present path
            // in `dbus_activation` (no window yet → `Task::none()`).
            Message::Quit => {
                return self
                    .core
                    .main_window_id()
                    .map(cosmic::iced::window::close)
                    .unwrap_or_else(Task::none);
            }

            // --- engine → UI / async ---
            Message::Player(event) => return self.on_player_event(event),
            Message::Tick(_) => {
                channels::decay_meters(&mut self.channels);
            }
            Message::WindowResized(width, height) => {
                // Persisted at close (U2 save-point row 1), never here.
                self.settings.window_width = i32::try_from(width).unwrap_or(i32::MAX);
                self.settings.window_height = i32::try_from(height).unwrap_or(i32::MAX);
            }
        }
        Task::none()
    }

    /// T10a2 layout (ux W1/§3.1): playlist sidebar on the left at the C++
    /// `AdwOverlaySplitView` 0.28 width fraction (`FillPortion(28/72)`) + main
    /// column (transport region with THE status line, view-switcher page
    /// — Lyrics + Channels composed (T12b), Piano canvas (T13c) —, toasts),
    /// with the D2 prefs dialog (T12b, inline chrome) or a T13 dialog sheet
    /// (D3/D7/D8/D6/D5/D4 via `chrome::wrap_modal`) stacked over it while
    /// open. The sidebar
    /// honors `playlistVisible`
    /// (`settings.playlist_visible`, default true); there is no runtime
    /// collapse toggle yet — any toggle needs a new `Message` + update arm
    /// (arch decision, recorded gap), so a hidden sidebar reappears only via
    /// the setting. The whole layout stays wrapped in the Wayland drop
    /// target (`dnd_destination`, arch §2.3, same mimes + callbacks); X11
    /// drops arrive via the `FileDropped` subscription instead.
    fn view(&self) -> Element<'_, Self::Message> {
        let page: Element<'_, Self::Message> = match self.view {
            View::Lyrics => lyrics::view(self.lyrics_view_model()),
            View::Channels => channels_view::view(self.channels_view_model()),
            View::Piano => piano::view(piano::view_model(
                &self.piano,
                self.song.as_ref(),
                &self.settings,
            )),
        };
        let mut main: Vec<Element<'_, Self::Message>> =
            vec![transport::view(self.transport_view_model()), page];
        for toast in &self.toasts {
            main.push(cosmic::widget::text::body(toast.as_str()).into());
        }
        let main = cosmic::widget::column::with_children(main)
            .spacing(8)
            .width(cosmic::iced::Length::Fill);
        let content = if self.settings.playlist_visible {
            cosmic::widget::row::with_children(vec![
                cosmic::widget::container(sidebar::view(self.playlist_view_model()))
                    .width(cosmic::iced::Length::FillPortion(28))
                    .height(cosmic::iced::Length::Fill)
                    .into(),
                cosmic::widget::container(main)
                    .width(cosmic::iced::Length::FillPortion(72))
                    .height(cosmic::iced::Length::Fill)
                    .into(),
            ])
            .spacing(8)
        } else {
            cosmic::widget::row::with_children(vec![
                cosmic::widget::container(main)
                    .width(cosmic::iced::Length::Fill)
                    .height(cosmic::iced::Length::Fill)
                    .into(),
            ])
        }
        .height(cosmic::iced::Length::Fill);
        let base: Element<'_, Self::Message> =
            cosmic::widget::dnd_destination(content, vec![DND_URI_LIST_MIME.into()])
                .on_data_received(|mime, data| Message::DndDataReceived { mime, data })
                .on_file_transfer(Message::DndFileTransfer)
                .into();
        // D2 dialog chrome: `Stack` overlay whose top layer is an ACTUAL
        // modal capture layer (T12 devil-BLOCKING FIX — the old "modal
        // `widget::dialog`" comment lied: `Dialog` is a pure styled
        // container with zero capture code, and `Stack::update` propagates
        // every event to lower layers unless `shell.is_event_captured()`,
        // pinned rev `7e82198` `iced/widget/src/stack.rs`). The mechanism:
        // a full-bounds `mouse_area` wraps the centered sheet — iced offers
        // the event to the dialog content first, and anything the dialog
        // does not consume is swallowed by the area, which captures left
        // press/release + touch unconditionally when the cursor is over its
        // bounds (`mouse_area.rs` — no handler needed) and the wheel via
        // `on_scroll` into `ChooserCancelled` (the silent no-op arm: no
        // state change, no toast). Clicks therefore cannot reach the base
        // (buttons/toggles/sliders/dropdowns/text inputs all arm on left
        // press); right- and middle-press still propagate, but no base widget binds either.
        // The wrap container also paints the dim scrim (C++ `AdwPrefsWindow`
        // dims its parent). Keyboard (T14d wired): the `keyboard::listen`
        // subscription resolves through `state::keys` (9 accelerators +
        // Enter-play + Esc/Tab, gated to Esc/Tab-only under dialogs), and
        // every open focuses a dialog widget (G4 partial — the `focus(id)`
        // op unfocuses everything else, so base Enter cannot fire silently
        // under the dialog). Residuals, honestly kept: Tab can still cycle
        // focus out to base widgets (no subtree-scoping op exists in the
        // pinned rev), and the narrow close commit keeps any such stray
        // activation state-safe. Async completions (engine events, resize,
        // DnD drops) still apply under the dialog and now survive the close
        // instead of being reverted.
        // Close commits once (commit-on-close — no Cancel row, C++ parity).
        // T13 dialogs compose below through `chrome::wrap_modal`, which
        // generalizes this exact chrome verbatim (T13c verified: same
        // full-bounds `mouse_area` + 0.45 dim scrim + centered sheet +
        // wheel-into-`ChooserCancelled` + `Stack` — only the sheet
        // construction moves to the caller). D2 keeps its inline copy:
        // zero T12 churn, identical pixels.
        if let (Some(Dialog::Preferences), Some((vm, pickers))) = (&self.dialog, &self.prefs) {
            return Self::prefs_sheet(base, vm, pickers);
        }
        match self.dialog_sheet() {
            Some(sheet) => chrome::wrap_modal(base, sheet),
            None => base,
        }
    }

    /// Second-launch / portal activation: forward files as
    /// `OpenFiles(paths, replace=true)` + present the window (arch §2.5,
    /// same contract as C++ `on_open` in `src/main.cpp`: forward, then
    /// `gtk_window_present` unconditionally — even with no files).
    ///
    /// Present mechanism: `iced::window::gain_focus` on the main window id,
    /// batched with the `OpenFiles` task (no window yet → `Task::none()` for
    /// the present half). The `COSMIC_SINGLE_INSTANCE=false` escape hatch is
    /// honored inside libcosmic's `run_single_instance` boot path (which we
    /// call unconditionally from `main.rs`): a second process then runs
    /// standalone and this override never fires.
    ///
    /// No `cfg` gate: this crate unconditionally enables libcosmic's
    /// `single-instance` feature, so the trait method always exists.
    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        let paths = activation_files(&msg);

        let present = self
            .core
            .main_window_id()
            .map(cosmic::iced::window::gain_focus)
            .unwrap_or_else(Task::none);

        if paths.is_empty() {
            present
        } else {
            Task::batch([
                Task::done(Action::App(Message::OpenFiles(paths, true))),
                present,
            ])
        }
    }
}

// NOTE: `cosmic::ApplicationExt` (title, view_main, …) is blanket-implemented
// for all `Application`s — no manual impl. The header bar shows the title set
// via `set_header_title` in `init`/`on_opened` above.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::paths::Env;
    use crate::core::player::SongSnapshot;

    /// `update()` without the must-use `Task` dance.
    fn upd(app: &mut App, msg: Message) {
        std::mem::drop(app.update(msg));
    }

    /// App wired to a real engine thread (no output selected, so playback
    /// commands run silently) with temp-dir paths. The receiver lets tests
    /// observe engine answers.
    fn test_app() -> (App, EngineEventReceiver, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let env = Env::for_test(
            &tmp.path().join("home"),
            &tmp.path().join("home/.config"),
            &tmp.path().join("home/.local/share"),
            tmp.path(),
        );
        let (engine, receiver, _thread) = spawn_engine();
        let app = App {
            core: Core::default(),
            engine,
            engine_sub: EngineSubId(u64::MAX),
            playback: PlaybackState::new(),
            song: None,
            playlist: PlaylistState::new(),
            channels: channels::default_mix(),
            lyrics: LyricsState::new(),
            piano: PianoState::new(),
            midi: MidiState::new(),
            settings: Settings::new(),
            paths: Paths::with_env(env, None),
            dialog: None,
            prefs: None,
            midi_dlg: None,
            jump_bar: 1,
            loop_from: 1,
            loop_to: 1,
            view: View::Lyrics,
            toasts: Vec::new(),
            pending_autoplay: false,
            output_epoch: 0,
        };
        (app, receiver, tmp)
    }

    fn test_song() -> LoadedSong {
        LoadedSong::from(SongSnapshot {
            title: "song.mid".to_string(),
            format: "SMF".to_string(),
            bars: 16,
            len_ticks: 4800,
            duration_ms: 60_000,
            division: 96,
            num_tracks: 2,
            initial_tempo_us: 500_000.0,
            channel_used: std::array::from_fn(|ch| ch < 2),
            channel_labels: std::array::from_fn(|ch| format!("Track{ch}")),
            lowest_note: 36,
            highest_note: 96,
            charset: "UTF-8".to_string(),
            texts: Vec::new(),
            metadata: Vec::new(),
            copyrights: Vec::new(),
        })
    }

    /// Feed the song as already opened (no autoplay): drives `on_opened`
    /// without an engine round trip.
    fn open_fake_song(app: &mut App) {
        upd(
            app,
            Message::Player(PlayerEvent::Opened(Box::new(test_song_into_snapshot()))),
        );
    }

    /// Pump lossless engine events into `update` until `pred` matches.
    fn pump_until(
        app: &mut App,
        rx: &EngineEventReceiver,
        timeout: Duration,
        pred: impl Fn(&PlayerEvent) -> bool,
    ) -> PlayerEvent {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let now = std::time::Instant::now();
            assert!(
                now < deadline,
                "timed out waiting for engine event (toasts: {:?})",
                app.toasts
            );
            match rx.recv_lossless_timeout(deadline - now) {
                Some(ev) => {
                    let hit = pred(&ev);
                    upd(app, Message::Player(ev.clone()));
                    if hit {
                        return ev;
                    }
                }
                None => panic!(
                    "engine hung up waiting for event (toasts: {:?})",
                    app.toasts
                ),
            }
        }
    }

    fn pump_opened(app: &mut App, rx: &EngineEventReceiver) {
        pump_until(app, rx, Duration::from_secs(10), |ev| {
            matches!(ev, PlayerEvent::Opened(_))
        });
    }

    // -- transport --------------------------------------------------------

    #[test]
    fn play_with_empty_song_loads_playlist_first_e2e() {
        let (mut app, rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("examples/test.mid")]);
        upd(&mut app, Message::Play);
        pump_opened(&mut app, &rx);
        // Real `Opened` snapshot: labels, bars, duration all populated.
        let song = app.song.as_ref().unwrap();
        assert_eq!(song.title, "test.mid");
        assert!(song.bars > 0);
        assert!(song.len_ticks > 0);
        assert!(song.channel_used.iter().any(|&u| u));
        assert_eq!(song.channel_labels.len(), 16);
        assert_eq!(app.playback.status, Status::Playing);
        // The autoplay Play reaches the engine: Started confirms.
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Started)
        });
        upd(&mut app, Message::Stop);
    }

    #[test]
    fn play_with_no_song_and_empty_playlist_is_noop() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::Play);
        assert!(app.song.is_none());
        assert_eq!(app.playback.status, Status::Ready);
        assert!(!app.pending_autoplay);
    }

    #[test]
    fn transport_pause_stop_round_trip_e2e() {
        let (mut app, rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("examples/test.mid")]);
        upd(&mut app, Message::Play);
        pump_opened(&mut app, &rx);
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Started)
        });

        upd(&mut app, Message::Pause);
        assert_eq!(app.playback.status, Status::Paused);
        // C++ parity quirk: the engine postlude emits `Stopped` on pause
        // too, and the async event overwrites the optimistic `Paused`
        // (C++: `g_idle_add` onStopped after `setStatus("Paused")`).
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Stopped)
        });
        assert_eq!(app.playback.status, Status::Stopped);

        upd(&mut app, Message::Pause); // idle + song → resume
        assert_eq!(app.playback.status, Status::Playing);
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Started)
        });

        app.piano.note_on(0, 60, 100);
        upd(&mut app, Message::Stop);
        assert_eq!(app.playback.status, Status::Stopped);
        assert_eq!(app.playback.position_ticks, 0);
        assert_eq!(app.playback.elapsed_ms, 0);
        assert!(!app.piano.notes_on[0][60]);
    }

    #[test]
    fn pause_with_no_song_is_noop() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::Pause);
        assert_eq!(app.playback.status, Status::Ready);
    }

    #[test]
    fn next_advances_and_prev_does_not_save() {
        let (mut app, _rx, _tmp) = test_app();
        app.settings.auto_song_settings = true;
        app.song = Some(test_song());
        app.playlist.set_items(vec![
            PathBuf::from("/m/a.mid"),
            PathBuf::from("/m/b.mid"),
            PathBuf::from("/m/c.mid"),
        ]);
        app.playlist.set_current_index(1);
        assert!(!app.paths.data_dir().exists());

        // Next saves before leaving (auto mode).
        upd(&mut app, Message::NextSong);
        assert_eq!(app.playlist.current_index(), 2);
        assert!(app.pending_autoplay);
        assert!(app.paths.song_settings_path("song.mid").is_file());

        // Prev never saves (C++ parity): remove the cfg, go back, still gone.
        std::fs::remove_file(app.paths.song_settings_path("song.mid")).unwrap();
        upd(&mut app, Message::PrevSong);
        assert_eq!(app.playlist.current_index(), 1);
        assert!(!app.paths.song_settings_path("song.mid").exists());
    }

    #[test]
    fn next_at_end_and_prev_at_start_do_not_wrap() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        app.playlist.set_current_index(1);
        upd(&mut app, Message::NextSong);
        assert_eq!(app.playlist.current_index(), 1);
        assert!(!app.pending_autoplay);
        app.playlist.set_current_index(0);
        upd(&mut app, Message::PrevSong);
        assert_eq!(app.playlist.current_index(), 0);
    }

    #[test]
    fn beat_step_messages_leave_state_untouched() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(&mut app, Message::BeatForward);
        upd(&mut app, Message::BeatBackward);
        assert_eq!(app.playback.status, Status::Stopped);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn jump_to_bar_confirms_and_closes_dialog() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        app.dialog = Some(Dialog::JumpToBar);
        upd(&mut app, Message::JumpToBar(4));
        assert_eq!(app.dialog, None);
    }

    #[test]
    fn loop_toggle_opens_dialog_off_disables() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::LoopToggled(true));
        assert_eq!(app.dialog, Some(Dialog::Loop));
        assert!(!app.playback.loop_enabled);
        upd(&mut app, Message::LoopToggled(false));
        assert!(!app.playback.loop_enabled);
    }

    #[test]
    fn loop_toggle_on_abandons_shells_like_switch() {
        // T13d F2(a): `LoopToggled(true)` abandons the D3/prefs shells
        // exactly like `ShowDialog(Loop)` (the T12 switch rule) — no hidden
        // shell survives under the Loop dialog for a stale `MidiApply` to
        // commit (which would also wrongly close the Loop dialog).
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiInstrumentMapChanged(2));
        upd(&mut app, Message::LoopToggled(true));
        assert_eq!(app.dialog, Some(Dialog::Loop));
        assert!(app.midi_dlg.is_none());
        // A stale Apply now no-ops: settings untouched, Loop still open.
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.settings.instrument_map, 0);
        assert_eq!(app.dialog, Some(Dialog::Loop));
        // Same for the prefs shell.
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        assert!(app.prefs.is_some());
        upd(&mut app, Message::LoopToggled(true));
        assert!(app.prefs.is_none());
        assert_eq!(app.dialog, Some(Dialog::Loop));
    }

    #[test]
    fn loop_set_enables_range_and_closes() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        app.dialog = Some(Dialog::Loop);
        upd(&mut app, Message::LoopSet { start: 3, end: 5 });
        assert!(app.playback.loop_enabled);
        assert_eq!((app.playback.loop_start, app.playback.loop_end), (3, 5));
        assert_eq!(app.dialog, None);
    }

    #[test]
    fn loop_set_clamps_each_end_without_ordering() {
        // `loop_confirm`: each end clamps to `1..=last_bar`, from > to
        // passes through (C++ `setLoop` assigns blindly).
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(&mut app, Message::LoopSet { start: 0, end: 99 });
        assert_eq!((app.playback.loop_start, app.playback.loop_end), (1, 16));
        upd(&mut app, Message::LoopSet { start: 8, end: 3 });
        assert_eq!((app.playback.loop_start, app.playback.loop_end), (8, 3));
        // No song: everything pins to bar 1.
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::LoopSet { start: 4, end: 9 });
        assert_eq!((app.playback.loop_start, app.playback.loop_end), (1, 1));
    }

    #[test]
    fn jump_dialog_gates_on_song_and_seeds_bar_1() {
        // D7: C++ `showJump` early-returns with no song (`:777-778`).
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::ShowDialog(Dialog::JumpToBar));
        assert_eq!(app.dialog, None);
        // With a song: opens, scratch seeds to 1 (the C++ spin's unset
        // initial value is the range min), edits store without jumping.
        open_fake_song(&mut app);
        upd(&mut app, Message::ShowDialog(Dialog::JumpToBar));
        assert_eq!(app.dialog, Some(Dialog::JumpToBar));
        assert_eq!(app.jump_bar, 1);
        upd(&mut app, Message::JumpBarEdited(7));
        assert_eq!(app.jump_bar, 7);
        assert_eq!(app.dialog, Some(Dialog::JumpToBar));
        upd(&mut app, Message::JumpToBar(7));
        assert_eq!(app.dialog, None);
    }

    #[test]
    fn gated_jump_preserves_shells() {
        // T13g F5: a gated D7 open is a pure no-op — the prefs shell and
        // the D3 shell both survive (the gate used to run after
        // `open_dialog` cleared prefs, orphaning the shell under D3).
        let (mut app, _rx, _tmp) = test_app();
        // No song → gated; the prefs shell survives underneath.
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        assert!(app.prefs.is_some());
        upd(&mut app, Message::ShowDialog(Dialog::JumpToBar));
        assert_eq!(app.dialog, Some(Dialog::Preferences));
        assert!(app.prefs.is_some());
        // Same for the D3 shell (scratch edits intact).
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiInstrumentMapChanged(2));
        upd(&mut app, Message::ShowDialog(Dialog::JumpToBar));
        assert_eq!(app.dialog, Some(Dialog::MidiSetup));
        assert_eq!(scratch(&app).instrument_map, 2);
    }

    #[test]
    fn loop_dialog_seeds_from_playback_and_stores_scratch() {
        // D8 seeds come from `loop_start`/`loop_end` (C++ `:852-853`,
        // clamped like the spins); spin edits store without applying.
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        app.playback.loop_start = 3;
        app.playback.loop_end = 12;
        upd(&mut app, Message::LoopToggled(true));
        assert_eq!(app.dialog, Some(Dialog::Loop));
        assert_eq!((app.loop_from, app.loop_to), (3, 12));
        assert!(!app.playback.loop_enabled);
        upd(&mut app, Message::LoopFromEdited(5));
        upd(&mut app, Message::LoopToEdited(9));
        assert_eq!((app.loop_from, app.loop_to), (5, 9));
        assert!(!app.playback.loop_enabled);
        // OOR playback seeds clamp on open.
        app.playback.loop_start = 0;
        app.playback.loop_end = 999;
        upd(&mut app, Message::ShowDialog(Dialog::Loop));
        assert_eq!((app.loop_from, app.loop_to), (1, 16));
    }

    #[test]
    fn loop_toggle_forces_on_while_dialog_open_u3() {
        // U3 FIX pin: the menu Loop path emits `LoopToggled(true)` (no new
        // variant) — the dialog opens and the header toggle renders ON
        // while it is open, even with no song (the documented `showLoop`
        // deviation); cancel resets it, confirm keeps it ON.
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::LoopToggled(true));
        assert_eq!(app.dialog, Some(Dialog::Loop));
        assert_eq!((app.loop_from, app.loop_to), (1, 1));
        assert!(!app.playback.loop_enabled);
        assert!(app.transport_view_model().loop_enabled);
        upd(&mut app, Message::CloseDialog);
        assert!(!app.transport_view_model().loop_enabled);
        // Confirm path: `LoopSet` enables → toggle stays ON after close.
        upd(&mut app, Message::LoopToggled(true));
        upd(&mut app, Message::LoopSet { start: 1, end: 1 });
        assert!(app.playback.loop_enabled);
        assert!(app.transport_view_model().loop_enabled);
    }

    #[test]
    fn repeat_mode_syncs_settings_for_close_save() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::RepeatModeChanged(RepeatMode::Playlist));
        assert_eq!(app.playback.repeat, RepeatMode::Playlist);
        assert_eq!(app.settings.repeat_mode, 2);
    }

    #[test]
    fn position_seek_guards_empty_song_and_lands() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::PositionSeek(0.5));
        assert!(!app.playback.seeking);
        open_fake_song(&mut app);
        upd(&mut app, Message::PositionSeek(0.5));
        assert!(app.playback.seeking);
        assert_eq!(app.playback.position_ticks, 2400);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Time {
                elapsed_ms: 30_000,
                ticks: 2400,
            }),
        );
        assert!(!app.playback.seeking);
        assert_eq!(app.playback.elapsed_ms, 30_000);
    }

    #[test]
    fn tempo_volume_pitch_clamp_and_recompute() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(&mut app, Message::TempoChanged(999));
        assert_eq!(app.playback.tempo_factor_pct, 200);
        assert!((app.playback.bpm - 240.0).abs() < 1e-9);
        upd(&mut app, Message::TempoChanged(0));
        assert_eq!(app.playback.tempo_factor_pct, 50);
        upd(&mut app, Message::VolumeChanged(999));
        assert_eq!(app.playback.volume_pct, 200);
        upd(&mut app, Message::PitchChanged(-99));
        assert_eq!(app.playback.pitch_semitones, -12);
        upd(&mut app, Message::PitchChanged(99));
        assert_eq!(app.playback.pitch_semitones, 12);
    }

    // -- playlist ---------------------------------------------------------

    #[test]
    fn open_files_replaces_and_loads() {
        let (mut app, _rx, _tmp) = test_app();
        upd(
            &mut app,
            Message::OpenFiles(
                vec![PathBuf::from("/m/a.mid"), PathBuf::from("b.kar")],
                true,
            ),
        );
        assert_eq!(app.playlist.len(), 2);
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
    }

    #[test]
    fn open_files_appends_and_ignores_empty() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        upd(
            &mut app,
            Message::OpenFiles(vec![PathBuf::from("/m/new.mid")], false),
        );
        assert_eq!(app.playlist.len(), 2);
        upd(&mut app, Message::OpenFiles(Vec::new(), true));
        assert_eq!(app.playlist.len(), 2);
    }

    #[test]
    fn open_files_single_lst_loads_playlist() {
        let (mut app, _rx, tmp) = test_app();
        let list = tmp.path().join("gig.lst");
        std::fs::write(&list, "a.mid\nb.mid\nnotes.txt\n").unwrap();
        upd(&mut app, Message::OpenFiles(vec![list.clone()], true));
        assert_eq!(app.playlist.len(), 2);
        assert_eq!(app.settings.last_play_list, list.to_string_lossy());
        assert!(app.pending_autoplay);
        // Uppercase `.LST` is NOT a playlist (case-sensitive suffix, C++ parity).
        let upper = tmp.path().join("gig.LST");
        std::fs::write(&upper, "a.mid\n").unwrap();
        upd(&mut app, Message::OpenFiles(vec![upper], true));
        assert!(app.playlist.is_empty());
    }

    #[test]
    fn open_files_missing_lst_is_noop() {
        let (mut app, _rx, tmp) = test_app();
        upd(
            &mut app,
            Message::OpenFiles(vec![tmp.path().join("nope.lst")], true),
        );
        assert!(app.settings.last_play_list.is_empty());
        assert!(!app.pending_autoplay);
    }

    #[test]
    fn playlist_select_remove_move_shuffle_clear() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![
            PathBuf::from("/m/a.mid"),
            PathBuf::from("/m/b.mid"),
            PathBuf::from("/m/c.mid"),
        ]);
        upd(&mut app, Message::PlaylistSelect(2));
        assert_eq!(app.playlist.current_index(), 2);
        assert!(app.song.is_none()); // select-only, no load
        upd(&mut app, Message::PlaylistSelect(99));
        assert_eq!(app.playlist.current_index(), 2);
        upd(&mut app, Message::PlaylistRemove(0));
        assert_eq!(app.playlist.len(), 2);
        upd(&mut app, Message::PlaylistMoveDown(0));
        upd(&mut app, Message::PlaylistMoveUp(1));
        upd(&mut app, Message::PlaylistShuffle);
        assert_eq!(app.playlist.len(), 2);
        assert!(app.playlist.dirty());
        upd(&mut app, Message::PlaylistClear);
        assert!(app.playlist.is_empty());
        assert_eq!(app.playlist.current_index(), -1);
    }

    #[test]
    fn playlist_dialogs_launch_without_state_change() {
        // T10: D9/D10 launch portal chooser `Task`s (dropped here — no portal
        // headless). Launching itself mutates nothing; results arrive as
        // `PlaylistLoaded` / `PlaylistSaved` (tested below and in T9).
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(&mut app, Message::PlaylistLoadDialog);
        upd(&mut app, Message::PlaylistSaveDialog);
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
        assert_eq!(app.playlist.len(), 1);
    }

    #[test]
    fn playlist_loaded_applies_without_autoplay() {
        let (mut app, _rx, _tmp) = test_app();
        upd(
            &mut app,
            Message::PlaylistLoaded(Ok(crate::message::LoadedPlaylist {
                source: Some(PathBuf::from("/m/gig.lst")),
                items: vec![PathBuf::from("/m/a.mid")],
            })),
        );
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.playlist.current_index(), 0);
        assert_eq!(app.settings.last_play_list, "/m/gig.lst");
        assert_eq!(app.playlist.source_path(), Some(Path::new("/m/gig.lst")));
        assert!(!app.playlist.dirty());
        assert!(app.song.is_none());
        assert!(!app.pending_autoplay);
        upd(&mut app, Message::PlaylistLoaded(Err("boom".to_string())));
        assert_eq!(app.toasts, vec!["boom".to_string()]);
    }

    #[test]
    fn playlist_saved_records_source_and_clears_dirty() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(
            &mut app,
            Message::PlaylistSaved(Ok(PathBuf::from("/m/gig.lst"))),
        );
        assert_eq!(app.settings.last_play_list, "/m/gig.lst");
        assert!(!app.playlist.dirty());
        upd(&mut app, Message::PlaylistSaved(Err("denied".to_string())));
        assert_eq!(app.toasts, vec!["denied".to_string()]);
    }

    // -- T10 file flows ---------------------------------------------------------

    #[test]
    fn chooser_contract_strings_match_cpp() {
        assert_eq!(OPEN_DIALOG_TITLE, "Open MIDI files");
        assert_eq!(OPEN_FILTER_LABEL, "MIDI / Karaoke / Cakewalk / Playlist");
        assert_eq!(
            OPEN_FILTER_GLOBS,
            ["*.mid", "*.midi", "*.kar", "*.rmi", "*.wrk", "*.lst"]
        );
        assert_eq!(PLAYLIST_OPEN_TITLE, "Open playlist");
        assert_eq!(PLAYLIST_FILTER_LABEL, "Playlists");
        assert_eq!(PLAYLIST_FILTER_GLOB, "*.lst");
        assert_eq!(PLAYLIST_SAVE_TITLE, "Save playlist");
        assert_eq!(PLAYLIST_SAVE_INITIAL_NAME, "playlist.lst");
        assert_eq!(DND_URI_LIST_MIME, "text/uri-list");
    }

    #[test]
    fn open_files_dialog_launches_without_state_change() {
        // D1 trigger: launches the portal multi-select `Task` (dropped here —
        // no portal headless). Results arrive as `OpenFilesChosen`.
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(&mut app, Message::OpenFilesDialog);
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
        assert!(!app.pending_autoplay);
    }

    #[test]
    fn open_files_chosen_replaces_via_open_files_path() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        upd(
            &mut app,
            Message::OpenFilesChosen(Ok(vec![
                PathBuf::from("/m/a.mid"),
                PathBuf::from("/m/b.kar"),
            ])),
        );
        // REPLACE (P2), first selected, autoplay per settings (default on).
        assert_eq!(app.playlist.len(), 2);
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn open_files_chosen_single_lst_loads_playlist_with_autoplay() {
        // D1 `.lst` follows C++ `openFiles` (window.cpp:354-360): playlist
        // load + `loadCurrent(autoPlay)` — autoplay per settings. The
        // no-autoplay list load is D9, not D1 (ux F3 keeps the distinction).
        let (mut app, _rx, tmp) = test_app();
        let list = tmp.path().join("gig.lst");
        std::fs::write(&list, "a.mid\nb.mid\n").unwrap();
        upd(&mut app, Message::OpenFilesChosen(Ok(vec![list.clone()])));
        assert_eq!(app.playlist.len(), 2);
        assert_eq!(app.settings.last_play_list, list.to_string_lossy());
        assert!(app.pending_autoplay);
    }

    #[test]
    fn open_files_chosen_empty_is_silent_noop_and_err_toasts() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(&mut app, Message::OpenFilesChosen(Ok(Vec::new())));
        assert_eq!(app.playlist.len(), 1);
        assert!(!app.pending_autoplay);
        assert!(app.toasts.is_empty());

        upd(
            &mut app,
            Message::OpenFilesChosen(Err("Could not open files: boom".to_string())),
        );
        assert_eq!(app.toasts, vec!["Could not open files: boom".to_string()]);
        assert_eq!(app.playlist.len(), 1);
        assert!(!app.pending_autoplay);
    }

    #[test]
    fn chooser_cancelled_is_silent_noop() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        app.dialog = Some(Dialog::Loop);
        upd(&mut app, Message::ChooserCancelled);
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.playlist.current_index(), 0);
        assert_eq!(app.dialog, Some(Dialog::Loop));
        assert!(app.toasts.is_empty());
        assert!(!app.pending_autoplay);
    }

    #[test]
    fn playlist_row_activated_selects_loads_and_plays() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![
            PathBuf::from("/m/a.mid"),
            PathBuf::from("/m/b.mid"),
            PathBuf::from("/m/c.mid"),
        ]);
        upd(&mut app, Message::PlaylistRowActivated(2));
        assert_eq!(app.playlist.current_index(), 2);
        assert!(app.pending_autoplay);
        // Select-only still never loads (U11 click vs activate).
        upd(&mut app, Message::PlaylistSelect(0));
        assert_eq!(app.playlist.current_index(), 0);
    }

    #[test]
    fn playlist_row_activated_out_of_range_keeps_current() {
        // C++ parity: the select ignores the bad index (A14) and the load
        // still targets the current item.
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(&mut app, Message::PlaylistRowActivated(99));
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
    }

    #[test]
    fn urls_to_paths_converts_multi_and_percent_decodes() {
        let urls = [
            Url::parse("file:///m/a.mid").unwrap(),
            Url::parse("file:///m/my%20song.KAR").unwrap(),
        ];
        assert_eq!(
            urls_to_paths(&urls).unwrap(),
            vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/my song.KAR"),]
        );
    }

    #[test]
    fn urls_to_paths_rejects_non_file_url_with_error() {
        let urls = [
            Url::parse("file:///m/a.mid").unwrap(),
            Url::parse("https://example.com/b.mid").unwrap(),
        ];
        assert_eq!(
            urls_to_paths(&urls).unwrap_err(),
            "Cannot open https://example.com/b.mid: not a local file"
        );
        // The error toasts through the D1 result arm without touching state.
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        let err = urls_to_paths(&urls).unwrap_err();
        upd(&mut app, Message::OpenFilesChosen(Err(err.clone())));
        assert_eq!(app.toasts, vec![err]);
        assert_eq!(app.playlist.len(), 1);
    }

    #[test]
    fn is_droppable_matches_gtk_target_filter() {
        // 5 song extensions, case-insensitive …
        for name in [
            "a.mid", "b.MID", "c.midi", "d.Midi", "e.kar", "f.KAR", "g.rmi", "h.RMI", "i.wrk",
            "j.WRK",
        ] {
            assert!(is_droppable(Path::new(name)), "{name}");
        }
        // … plus `.lst`, case-SENSITIVE (`g_str_has_suffix` parity) …
        assert!(is_droppable(Path::new("gig.lst")));
        assert!(!is_droppable(Path::new("gig.LST")));
        // … and nothing else.
        for name in ["notes.txt", "song.mid.bak", "lst", ".mid~", "noext"] {
            assert!(!is_droppable(Path::new(name)), "{name}");
        }
        // Portal document paths pass like any other absolute path.
        assert!(is_droppable(Path::new(
            "/run/user/1000/doc/abc123/song.mid"
        )));
    }

    #[test]
    fn parse_uri_list_skips_comments_blanks_and_junk() {
        let payload = b"# comment\r\n\r\nfile:///m/a.mid\r\nfile:///m/my%20song.kar\r\n\
            https://example.com/b.mid\r\nnot-a-url\r\nfile:///m/notes.txt\r\nfile:///gig.lst\r\n";
        assert_eq!(
            parse_uri_list(payload),
            vec![
                PathBuf::from("/m/a.mid"),
                PathBuf::from("/m/my song.kar"),
                PathBuf::from("/gig.lst"),
            ]
        );
        assert!(parse_uri_list(b"").is_empty());
        assert!(parse_uri_list(b"# only a comment\n").is_empty());
    }

    #[test]
    fn files_dropped_filters_and_replaces() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        upd(
            &mut app,
            Message::FilesDropped(vec![
                PathBuf::from("/m/notes.txt"), // filtered out
                PathBuf::from("/m/a.mid"),
            ]),
        );
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
    }

    #[test]
    fn files_dropped_unsupported_only_is_silent_noop() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        upd(
            &mut app,
            Message::FilesDropped(vec![PathBuf::from("/m/notes.txt")]),
        );
        upd(&mut app, Message::FilesDropped(Vec::new()));
        assert_eq!(app.playlist.len(), 1);
        assert!(!app.pending_autoplay);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn files_dropped_single_lst_loads_playlist() {
        let (mut app, _rx, tmp) = test_app();
        let list = tmp.path().join("drop.lst");
        std::fs::write(&list, "a.mid\n").unwrap();
        upd(&mut app, Message::FilesDropped(vec![list.clone()]));
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.settings.last_play_list, list.to_string_lossy());
        assert!(app.pending_autoplay);
    }

    #[test]
    fn dnd_data_received_parses_uri_list_and_gates_mime() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        upd(
            &mut app,
            Message::DndDataReceived {
                mime: "text/uri-list".to_string(),
                data: b"file:///m/a.mid\r\nfile:///m/notes.txt\r\n".to_vec(),
            },
        );
        assert_eq!(app.playlist.len(), 1);
        assert!(app.pending_autoplay);

        // Any other mime is ignored (offered mimes outside our contract).
        app.pending_autoplay = false;
        upd(
            &mut app,
            Message::DndDataReceived {
                mime: "text/plain".to_string(),
                data: b"file:///m/b.mid".to_vec(),
            },
        );
        assert_eq!(app.playlist.len(), 1);
        assert!(!app.pending_autoplay);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn dnd_file_transfer_launches_without_state_change() {
        // The portal file-transfer resolution is a runtime `Task` (dropped
        // here); results arrive as `DndFileTransferDone`.
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(
            &mut app,
            Message::DndFileTransfer("transfer-key".to_string()),
        );
        assert_eq!(app.playlist.len(), 1);
        assert!(app.toasts.is_empty());
        assert!(!app.pending_autoplay);
    }

    #[test]
    fn dnd_file_transfer_done_opens_or_toasts() {
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![PathBuf::from("/m/old.mid")]);
        upd(
            &mut app,
            Message::DndFileTransferDone(Ok(vec![
                PathBuf::from("/run/user/1000/doc/deadbeef/song.wrk"),
                PathBuf::from("/m/notes.txt"),
            ])),
        );
        assert_eq!(app.playlist.len(), 1);
        assert!(app.pending_autoplay);

        upd(
            &mut app,
            Message::DndFileTransferDone(Err("Could not receive dropped files: gone".to_string())),
        );
        assert_eq!(
            app.toasts,
            vec!["Could not receive dropped files: gone".to_string()]
        );
    }

    #[test]
    fn window_event_message_maps_resize_drop_and_ignores_hover() {
        use cosmic::iced::event::{Event, Status};
        use cosmic::iced::window::{Event as WindowEvent, Id};

        let id = Id::unique();
        match window_event_message(
            Event::Window(WindowEvent::Resized(cosmic::iced::Size::new(3.0, 4.0))),
            Status::Ignored,
            id,
        ) {
            Some(Message::WindowResized(3, 4)) => {}
            other => panic!("expected WindowResized, got {other:?}"),
        }
        match window_event_message(
            Event::Window(WindowEvent::FileDropped(vec![PathBuf::from("/m/a.mid")])),
            Status::Ignored,
            id,
        ) {
            Some(Message::FilesDropped(paths)) => {
                assert_eq!(paths, vec![PathBuf::from("/m/a.mid")]);
            }
            other => panic!("expected FilesDropped, got {other:?}"),
        }
        // Hover has no C++ surface: unmapped.
        assert!(
            window_event_message(
                Event::Window(WindowEvent::FileHovered(vec![PathBuf::from("/m/a.mid")])),
                Status::Ignored,
                id,
            )
            .is_none()
        );
    }

    #[test]
    fn portal_doc_paths_replace_playlist_integration_shape() {
        // P2 replacing-integration shape: a song delivered through a portal
        // document path (`/run/…/doc/…`, copied fixture) REPLACES the
        // playlist — never appends — via the same `OpenFiles` path the
        // chooser, DnD, and single-instance forward all share.
        let (mut app, _rx, tmp) = test_app();
        let doc_dir = tmp.path().join("run/1000/doc/deadbeef");
        std::fs::create_dir_all(&doc_dir).unwrap();
        let song = doc_dir.join("song.mid");
        std::fs::copy("examples/test.mid", &song).unwrap();
        app.playlist.set_items(vec![
            PathBuf::from("/m/old-a.mid"),
            PathBuf::from("/m/old-b.mid"),
        ]);

        upd(&mut app, Message::OpenFiles(vec![song.clone()], true));

        assert_eq!(app.playlist.len(), 1);
        let current = app.playlist.current().unwrap().to_path_buf();
        assert!(current.ends_with("song.mid"));
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
        assert!(app.toasts.is_empty());
    }

    /// Rust successor of `tests/check_single_instance_open.cmake` (retired in
    /// T10): a forwarded second-open lands in running state as
    /// `OpenFiles(paths, replace=true)` — REPLACE, never append (P2).
    ///
    /// Present mechanism (observed in `dbus_activation`, not in this test's
    /// reach): the override batches `Task::done(Action::App(OpenFiles))` with
    /// `iced::window::gain_focus(main_window_id)` — the `gtk_window_present`
    /// twin, issued unconditionally even with no files. `Task` values expose
    /// no introspection, so this test pins the pure mapping
    /// (`activation_files`) plus the state half (driven through `update()`,
    /// exactly as the runtime delivers the batched task).
    #[test]
    fn second_open_forwards_into_running_state() {
        use cosmic::dbus_activation::{Details, Message as Activation};

        fn activation(msg: Details<String, Vec<String>>) -> Activation<String, Vec<String>> {
            Activation {
                activation_token: None,
                desktop_startup_id: None,
                msg,
            }
        }

        // `ActivateAction{"open", args}` — our own second CLI launch.
        let forwarded = activation(Details::ActivateAction {
            action: "open".to_string(),
            args: vec!["/m/new.mid".to_string(), String::new()],
        });
        assert_eq!(
            activation_files(&forwarded),
            vec![PathBuf::from("/m/new.mid")]
        );

        // `Open{urls}` — portal/MIME activation; non-file URLs skipped.
        let opened = activation(Details::Open {
            url: vec![
                Url::parse("file:///m/a.mid").unwrap(),
                Url::parse("https://example.com/b.mid").unwrap(),
            ],
        });
        assert_eq!(activation_files(&opened), vec![PathBuf::from("/m/a.mid")]);

        // Bare `Activate` — present-only, no files.
        assert!(activation_files(&activation(Details::Activate)).is_empty());

        // The state half: `OpenFiles(paths, replace=true)` REPLACES.
        let (mut app, _rx, _tmp) = test_app();
        app.playlist.set_items(vec![
            PathBuf::from("/m/old-a.mid"),
            PathBuf::from("/m/old-b.mid"),
        ]);
        let paths = activation_files(&forwarded);
        upd(&mut app, Message::OpenFiles(paths, true));
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
        assert!(app.toasts.is_empty());

        // The override itself runs without panicking on every variant (its
        // `Task` output is runtime-delivered, asserted above by shape).
        std::mem::drop(app.dbus_activation(forwarded));
        std::mem::drop(app.dbus_activation(opened));
        std::mem::drop(app.dbus_activation(activation(Details::Activate)));
    }

    // -- mix ------------------------------------------------------------------

    #[test]
    fn channel_mix_transitions_and_oor_guard() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::ChannelMute(0, true));
        assert!(app.channels[0].muted);
        upd(&mut app, Message::ChannelSolo(1, true));
        assert!(app.channels[1].soloed);
        upd(&mut app, Message::ChannelLock(2, true));
        assert!(app.channels[2].locked);
        upd(&mut app, Message::ChannelPatch(0, 200));
        assert_eq!(app.channels[0].patch, 127);
        upd(&mut app, Message::ChannelLevel(0, 999));
        assert_eq!(app.channels[0].level_pct, 200);
        upd(&mut app, Message::ChannelName(0, "Lead".to_string()));
        assert_eq!(app.channels[0].label, "Lead");
        // Out-of-range channels are ignored, never panic.
        upd(&mut app, Message::ChannelMute(16, true));
        upd(&mut app, Message::ChannelSolo(255, true));
        upd(&mut app, Message::ChannelLock(99, true));
        upd(&mut app, Message::ChannelPatch(99, 1));
        upd(&mut app, Message::ChannelLevel(99, 1));
        upd(&mut app, Message::ChannelName(99, "x".to_string()));
        assert!(!app.channels[15].muted);
    }

    /// Mix arms end to end (state → engine sync values): the arms store, and
    /// the pushed `SetMuted` + `SetChannelVolume` pairs are exactly
    /// `sync_all` over the stored rows (the sends themselves are
    /// fire-and-forget into the real engine — mix commands emit no events —
    /// so the pure sync matrix in `state::channels` pins the values).
    #[test]
    fn channel_arms_drive_exact_engine_sync_values() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        // Drums-row mute (ch 9 = channel 10, the default drums channel):
        // mute applies uniformly — no drums exception in the mix path.
        upd(&mut app, Message::ChannelMute(9, true));
        upd(&mut app, Message::ChannelLevel(0, 150));
        upd(&mut app, Message::ChannelSolo(1, true));
        app.settings.solo_volume_reduction = 75;
        let sync = channels::sync_all(&app.channels, app.settings.solo_volume_reduction);
        // Non-solo ch 0: (100-75)% × 150%.
        assert!((sync[0].factor - 0.375).abs() < 1e-9);
        assert!(!sync[0].muted);
        // Soloed ch 1 keeps its 100% slider.
        assert!((sync[1].factor - 1.0).abs() < 1e-9);
        // Muted drums row still computes its reduced factor (mute silences
        // engine-side, independent of the factor).
        assert!(sync[9].muted);
        assert!((sync[9].factor - 0.25).abs() < 1e-9);
        // Clearing the solo restores every slider.
        upd(&mut app, Message::ChannelSolo(1, false));
        let sync = channels::sync_all(&app.channels, app.settings.solo_volume_reduction);
        assert!((sync[0].factor - 1.5).abs() < 1e-9);
    }

    /// Patch reselect: live `Program` events reselect the stored patch
    /// (clamped, OOR channels ignored) and relabel under the current
    /// instrument map; user patch changes store likewise.
    #[test]
    fn program_reselect_clamps_and_relabels_per_map() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Program { ch: 0, patch: 200 }),
        );
        assert_eq!(app.channels[0].patch, 127);
        // Reselect relabels under the current instrument map (GM default).
        assert_eq!(channels::patch_label(0, app.channels[0].patch), "Gunshot");
        // Out-of-range channels never reselect.
        upd(
            &mut app,
            Message::Player(PlayerEvent::Program { ch: 16, patch: 5 }),
        );
        assert_eq!(app.channels[15].patch, 0);
        // User patch change stores (clamped) + relabels.
        upd(&mut app, Message::ChannelPatch(2, 40));
        assert_eq!(app.channels[2].patch, 40);
        assert_eq!(channels::patch_label(0, 40), "Violin");
        // MT-32 map relabels the same patch number differently.
        app.settings.instrument_map = 2;
        assert_eq!(
            channels::patch_label(app.settings.instrument_map, 40),
            "Echo Bell"
        );
    }

    /// Meter wiring: NoteOn peaks (`max`), a quieter NoteOn never lowers the
    /// peak, NoteOff leaves the meter alone (C++ parity — only the piano key
    /// clears), and the 50 ms tick decays the peak.
    #[test]
    fn note_on_peaks_meter_note_off_leaves_it() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        let on = |vel| {
            Message::Player(PlayerEvent::NoteOn {
                ch: 0,
                note: 60,
                vel,
            })
        };
        upd(&mut app, on(100));
        assert!((app.channels[0].level - 100.0 / 127.0).abs() < 1e-6);
        upd(&mut app, on(10));
        assert!((app.channels[0].level - 100.0 / 127.0).abs() < 1e-6);
        upd(
            &mut app,
            Message::Player(PlayerEvent::NoteOff {
                ch: 0,
                note: 60,
                vel: 64,
            }),
        );
        assert!(!app.piano.notes_on[0][60]);
        assert!((app.channels[0].level - 100.0 / 127.0).abs() < 1e-6);
        upd(&mut app, Message::Tick(std::time::Instant::now()));
        assert!((app.channels[0].level - 100.0 / 127.0 * 0.82).abs() < 1e-6);
        // Out-of-range channels never touch meters.
        upd(
            &mut app,
            Message::Player(PlayerEvent::NoteOn {
                ch: 16,
                note: 60,
                vel: 127,
            }),
        );
        assert!((app.channels[15].level - 0.0).abs() < f32::EPSILON);
    }

    // -- lyrics -----------------------------------------------------------------

    /// Texts for the filter-recompute tests: two lyric tracks + a text note.
    fn lyric_test_texts() -> Vec<crate::core::sequence::TextRec> {
        use crate::core::event::TextType;
        use crate::core::sequence::TextRec;
        vec![
            TextRec {
                tick: 0,
                track: 1,
                text_type: TextType::Lyric,
                text: b"la ".to_vec(),
            },
            TextRec {
                tick: 10,
                track: 2,
                text_type: TextType::Lyric,
                text: b"lu".to_vec(),
            },
            TextRec {
                tick: 20,
                track: 2,
                text_type: TextType::Text,
                text: b"note".to_vec(),
            },
        ]
    }

    /// Fake song carrying lyric texts (filters preselect track 2 + Lyric).
    fn open_lyric_song(app: &mut App) {
        let mut snapshot = test_song_into_snapshot();
        snapshot.texts = lyric_test_texts();
        upd(
            app,
            Message::Player(PlayerEvent::Opened(Box::new(snapshot))),
        );
    }

    #[test]
    fn lyric_filters_and_codec_recompute_body() {
        let (mut app, _rx, _tmp) = test_app();
        open_lyric_song(&mut app);
        // Preselects: track 2 (2 hits), Lyric (2:1 over Text).
        assert_eq!(app.lyrics.track_filter, Some(2));
        assert_eq!(app.lyrics.type_filter, 5);
        assert_eq!(app.lyrics.body, "lu");
        // Track change recomputes under the new selection (snap-back FIX:
        // the selection STAYS, `window.cpp:1788-1792` parity would revert).
        upd(&mut app, Message::LyricTrackFilter(Some(1)));
        assert_eq!(app.lyrics.track_filter, Some(1));
        assert_eq!(app.lyrics.body, "la ");
        // All-tracks + all-types shows everything with the newline rule.
        upd(&mut app, Message::LyricTrackFilter(None));
        upd(&mut app, Message::LyricTypeFilter(0));
        assert_eq!(app.lyrics.body, "la lunote\n");
        // Out-of-range type matches nothing; empty codec is ignored.
        upd(&mut app, Message::LyricTypeFilter(99));
        assert!(app.lyrics.body.is_empty());
        upd(&mut app, Message::LyricCodec(String::new()));
        assert_eq!(app.lyrics.codec, "UTF-8");
        upd(&mut app, Message::LyricCodec("SHIFT_JIS".to_string()));
        assert_eq!(app.lyrics.codec, "SHIFT_JIS");
    }

    #[test]
    fn lyric_track_filter_some_zero_normalizes_to_none() {
        let (mut app, _rx, _tmp) = test_app();
        open_lyric_song(&mut app);
        // T11 follow-up 1: `Some(0)` behaves as all-tracks (C++ `> 0` rule),
        // not as a filter matching nothing.
        upd(&mut app, Message::LyricTrackFilter(Some(0)));
        assert_eq!(app.lyrics.track_filter, None);
        assert_eq!(app.lyrics.body, "la lu");
        // Live filtering agrees: every track passes.
        assert!(app.lyrics.passes_filter(7, 5));
    }

    #[test]
    fn lyric_filter_change_clears_live_tail() {
        let (mut app, _rx, _tmp) = test_app();
        open_lyric_song(&mut app);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Text {
                track: 2,
                text_type: 5,
                ticks: 0,
                bytes: b"live".to_vec(),
            }),
        );
        assert_eq!(app.lyrics.live_tail(), "live");
        // C++ `refreshLyrics` clears the whole buffer incl. live appends.
        upd(&mut app, Message::LyricTrackFilter(None));
        assert!(app.lyrics.live_tail().is_empty());
        assert!(!app.lyrics.sung);
    }

    #[test]
    fn lyrics_copy_save_print_launch_without_state_change() {
        // The clipboard write / portal save / print export are runtime
        // `Task`s (dropped here); results arrive as `LyricsSaved` /
        // `LyricsPrinted`, cancel as the silent `ChooserCancelled`.
        let (mut app, _rx, _tmp) = test_app();
        open_lyric_song(&mut app);
        upd(&mut app, Message::LyricsCopy);
        upd(&mut app, Message::LyricsSaveDialog);
        upd(&mut app, Message::LyricsPrint);
        assert_eq!(app.lyrics.body, "lu");
        assert!(app.toasts.is_empty());
        upd(&mut app, Message::ChooserCancelled);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn lyrics_saved_and_printed_route_ok_silent_err_toast() {
        let (mut app, _rx, _tmp) = test_app();
        upd(
            &mut app,
            Message::LyricsSaved(Ok(PathBuf::from("/t/x.txt"))),
        );
        assert!(app.toasts.is_empty());
        upd(&mut app, Message::LyricsSaved(Err("ro".to_string())));
        assert_eq!(app.toasts, vec!["ro".to_string()]);
        upd(&mut app, Message::LyricsPrinted(Ok(())));
        assert_eq!(app.toasts.len(), 1);
        upd(
            &mut app,
            Message::LyricsPrinted(Err("Could not print lyrics: boom".to_string())),
        );
        assert_eq!(
            app.toasts,
            vec!["ro".to_string(), "Could not print lyrics: boom".to_string()]
        );
    }

    #[test]
    fn write_lyrics_file_round_trips_and_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lyrics.txt");
        write_lyrics_file(&path, "la la\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "la la\n");
        let missing = tmp.path().join("no-such-dir/lyrics.txt");
        let err = write_lyrics_file(&missing, "x").unwrap_err();
        assert!(err.starts_with("Could not save lyrics"), "{err}");
    }

    #[test]
    fn spawn_opener_maps_spawn_errors() {
        // `true` ignores its arg and spawns fine; a bogus command fails.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("print.txt");
        std::fs::write(&path, "x").unwrap();
        spawn_opener("true", &path).unwrap();
        let err = spawn_opener("goshmidi-no-such-opener-xyz", &path).unwrap_err();
        assert!(err.starts_with("Could not print lyrics"), "{err}");
    }

    #[test]
    fn prefs_commit_applies_lyrics_rows_without_touching_filters() {
        let (mut app, _rx, _tmp) = test_app();
        open_lyric_song(&mut app);
        upd(&mut app, Message::LyricTrackFilter(Some(1)));
        let mut prefs = Settings::new();
        prefs.future_color = "red".to_string();
        prefs.past_color = "garbage".to_string();
        prefs.highlight_color = "#1c71d8".to_string();
        prefs.lyrics_font = "Monospace Bold 9".to_string();
        prefs.text_alignment = 1;
        upd(&mut app, Message::PrefsChanged(prefs));
        let applied = &app.lyrics.applied;
        assert_eq!(
            applied.future,
            crate::state::lyrics::Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            }
        );
        assert_eq!(applied.past, crate::state::lyrics::GRAY_FALLBACK);
        assert_eq!(applied.font.family, "Monospace");
        assert_eq!(applied.font.weight, 700);
        assert_eq!(applied.alignment, crate::state::lyrics::LyricAlign::Center);
        // Body + filters untouched (C++ resets both dropdowns here — FIX).
        assert_eq!(app.lyrics.track_filter, Some(1));
        assert_eq!(app.lyrics.body, "la ");
    }

    #[test]
    fn song_settings_encoding_recomputes_body_keeping_filters() {
        let (mut app, _rx, _tmp) = test_app();
        open_lyric_song(&mut app);
        upd(&mut app, Message::LyricTrackFilter(None));
        upd(&mut app, Message::LyricTypeFilter(0));
        assert_eq!(app.lyrics.body, "la lunote\n");
        // A song-settings file carrying only an encoding: codec applies,
        // body recomputes, filter selections survive (C++ resets them — FIX).
        app.paths.ensure_data_dir().unwrap();
        let cfg = "[Global]\nencoding=SHIFT_JIS\n";
        std::fs::write(app.paths.song_settings_path("song.mid"), cfg).unwrap();
        upd(&mut app, Message::SongSettingsLoad);
        assert_eq!(app.lyrics.codec, "SHIFT_JIS");
        assert_eq!(app.lyrics.track_filter, None);
        assert_eq!(app.lyrics.type_filter, 0);
        assert_eq!(app.lyrics.body, "la lunote\n");
    }

    // -- piano ------------------------------------------------------------------

    #[test]
    fn piano_view_toggles() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(&mut app, Message::PianoTighten(true));
        assert!(app.piano.tighten);
        upd(&mut app, Message::PianoHideUnused);
        assert!(app.piano.visible[0]);
        assert!(!app.piano.visible[2]);
        upd(&mut app, Message::PianoShowAll);
        assert!(app.piano.visible.iter().all(|&v| v));
    }

    #[test]
    fn piano_preview_idle_only_note_off() {
        let (mut app, _rx, _tmp) = test_app();
        // T14d minor b: the `current()` guard needs a confirmed output —
        // the ungated half is pinned by `piano_preview_gated_without_output`.
        app.midi.output_ok_once = true;
        upd(&mut app, Message::PianoPreviewOn(0, 60));
        assert!(app.piano.notes_on[0][60]);
        assert_eq!(app.piano.note_vel[0][60], 80);
        // Idle → release clears (A3).
        upd(&mut app, Message::PianoPreviewOff);
        assert!(!app.piano.notes_on[0][60]);
        // Playing → release keeps lit notes (song notes must survive).
        app.playback.status = Status::Playing;
        upd(&mut app, Message::PianoPreviewOn(1, 64));
        upd(&mut app, Message::PianoPreviewOff);
        assert!(app.piano.notes_on[1][64]);
    }

    // -- MIDI setup ---------------------------------------------------------------

    /// Open the D3 shell in tests (drops the enumeration `Task` — the
    /// portal-free tests below feed `MidiPortsRefreshed` synthetically).
    fn open_midi_setup(app: &mut App) {
        upd(app, Message::ShowDialog(Dialog::MidiSetup));
        assert_eq!(app.dialog, Some(Dialog::MidiSetup));
        assert!(app.midi_dlg.is_some());
    }

    fn scratch(app: &App) -> &MidiState {
        &app.midi_dlg.as_ref().expect("D3 shell open").scratch
    }

    /// Feed a refill completion with the CURRENT scratch echo (T14d): models
    /// the realistic steady state (refill landed) so Apply reads a populated
    /// dropdown like C++ (`:1164-1166`). Tests that want the mid-refill race
    /// leave the ports empty instead (pinned explicitly).
    fn feed_ports(app: &mut App, ports: Vec<crate::core::output::MidiPort>) {
        let (backend, advanced) = {
            let dlg = scratch(app);
            (dlg.backend.clone(), dlg.advanced)
        };
        upd(
            app,
            Message::MidiPortsRefreshed {
                backend,
                advanced,
                ports,
            },
        );
    }

    /// One synthetic D3 port row.
    fn midi_port(id: &str, label: &str) -> crate::core::output::MidiPort {
        crate::core::output::MidiPort {
            id: id.to_string(),
            label: label.to_string(),
        }
    }

    #[test]
    fn midi_open_seeds_scratch_and_abandons_on_close() {
        let (mut app, _rx, _tmp) = test_app();
        app.midi.backend = "ALSA".to_string();
        app.midi.soundfont = "/sf/live.sf2".to_string();
        open_midi_setup(&mut app);
        // Seeded from live mirrors.
        assert_eq!(scratch(&app).backend, "ALSA");
        assert_eq!(scratch(&app).soundfont, "/sf/live.sf2");
        // Close abandons the scratch: live mirrors untouched.
        upd(&mut app, Message::MidiBackendSelected("Dummy".to_string()));
        assert_eq!(scratch(&app).backend, "Dummy");
        assert_eq!(app.midi.backend, "ALSA");
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.dialog, None);
        assert!(app.midi_dlg.is_none());
        assert_eq!(app.midi.backend, "ALSA");
    }

    #[test]
    fn midi_selections_stored_and_clamped() {
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiBackendSelected("ALSA".to_string()));
        assert_eq!(scratch(&app).backend, "ALSA");
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "ALSA".to_string(),
                advanced: false,
                ports: vec![crate::core::output::MidiPort {
                    id: "128:0".to_string(),
                    label: "Synth".to_string(),
                }],
            },
        );
        assert_eq!(scratch(&app).ports.len(), 1);
        // Switching backend drops the stale port list.
        upd(
            &mut app,
            Message::MidiBackendSelected("FluidSynth".to_string()),
        );
        assert!(scratch(&app).ports.is_empty());
        upd(&mut app, Message::MidiPortSelected("128:0".to_string()));
        assert_eq!(scratch(&app).port, "128:0");
        upd(&mut app, Message::MidiAdvancedToggled(true));
        assert!(scratch(&app).advanced);
        upd(&mut app, Message::MidiInstrumentMapChanged(9));
        assert_eq!(scratch(&app).instrument_map, 2);
        upd(&mut app, Message::MidiResetChanged(9));
        assert_eq!(scratch(&app).sysex_reset, 4);
        upd(
            &mut app,
            Message::MidiSoundFontChanged("/sf/gm.sf2".to_string()),
        );
        assert_eq!(scratch(&app).soundfont, "/sf/gm.sf2");
        // Live mirrors never move on edits.
        assert_eq!(app.midi.backend, "FluidSynth");
        assert!(app.midi.ports.is_empty());
    }

    #[test]
    fn midi_arms_noop_without_shell() {
        // Stale messages with no open dialog: silent no-ops (prefs
        // precedent) — no toast, no state change, no shell created.
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::MidiBackendSelected("ALSA".to_string()));
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "ALSA".to_string(),
                advanced: false,
                ports: vec![],
            },
        );
        upd(&mut app, Message::MidiPortSelected("128:0".to_string()));
        upd(&mut app, Message::MidiAdvancedToggled(true));
        upd(&mut app, Message::MidiInstrumentMapChanged(2));
        upd(&mut app, Message::MidiResetChanged(4));
        upd(
            &mut app,
            Message::MidiSoundFontChanged("/sf/x.sf2".to_string()),
        );
        upd(&mut app, Message::MidiSoundFontBrowse);
        upd(
            &mut app,
            Message::MidiSoundFontChosen(Ok(PathBuf::from("/sf/x.sf2"))),
        );
        upd(&mut app, Message::MidiTestNote);
        upd(&mut app, Message::MidiApply);
        assert!(app.midi_dlg.is_none());
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
        assert_eq!(app.midi.backend, "FluidSynth");
    }

    #[test]
    fn midi_port_pick_infers_profile_refill_does_not() {
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        // Refill completion stores ports but never infers (C++ `filling`
        // guard): map/reset stay at the seeded 0/1.
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "FluidSynth".to_string(),
                advanced: false,
                ports: vec![
                    crate::core::output::MidiPort {
                        id: "128:0".to_string(),
                        label: "Munt MT-32:Standard".to_string(),
                    },
                    crate::core::output::MidiPort {
                        id: "129:0".to_string(),
                        label: "Virtual SC55".to_string(),
                    },
                ],
            },
        );
        assert_eq!(scratch(&app).instrument_map, 0);
        assert_eq!(scratch(&app).sysex_reset, 1);
        // User pick → MT-32 map + MT-32 reset follow the label.
        upd(&mut app, Message::MidiPortSelected("128:0".to_string()));
        assert_eq!(scratch(&app).instrument_map, 2);
        assert_eq!(scratch(&app).sysex_reset, 4);
        // SC-55 pick → GS map + GS reset.
        upd(&mut app, Message::MidiPortSelected("129:0".to_string()));
        assert_eq!(scratch(&app).instrument_map, 1);
        assert_eq!(scratch(&app).sysex_reset, 2);
        // Unknown id: want stored, no inference (map/reset keep last).
        upd(&mut app, Message::MidiPortSelected("nope".to_string()));
        assert_eq!(scratch(&app).port, "nope");
        assert_eq!(scratch(&app).instrument_map, 1);
        assert_eq!(scratch(&app).sysex_reset, 2);
    }

    #[test]
    fn midi_soundfont_chosen_commits_entry_errors_toast() {
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(
            &mut app,
            Message::MidiSoundFontChosen(Ok(PathBuf::from("/sf/sc55.sf2"))),
        );
        assert_eq!(scratch(&app).soundfont, "/sf/sc55.sf2");
        // Deferred: live mirrors + settings untouched until Test/Apply.
        assert!(app.midi.soundfont.is_empty());
        assert!(app.settings.sound_font.is_empty());
        // Late result after close drops; errors toast.
        upd(&mut app, Message::CloseDialog);
        upd(
            &mut app,
            Message::MidiSoundFontChosen(Ok(PathBuf::from("/sf/late.sf2"))),
        );
        assert!(app.midi.soundfont.is_empty());
        upd(
            &mut app,
            Message::MidiSoundFontChosen(Err("Cannot open x: not a local file".to_string())),
        );
        assert_eq!(
            app.toasts,
            vec!["Cannot open x: not a local file".to_string()]
        );
    }

    #[test]
    fn midi_apply_persists_and_announces_e2e() {
        let (mut app, rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        {
            let dlg = app.midi_dlg.as_mut().unwrap();
            dlg.scratch.backend = "Dummy".to_string();
            dlg.scratch.port = "dummy".to_string();
            dlg.scratch.soundfont = "/sf/gm.sf2".to_string();
        }
        // T14d minor d: Apply reads the DROPDOWN — land the refill so the
        // shown id is the want (the mid-refill race is pinned separately).
        feed_ports(&mut app, vec![midi_port("dummy", "Dummy (no output)")]);
        upd(&mut app, Message::MidiApply);
        // Closed + shell cleared.
        assert_eq!(app.dialog, None);
        assert!(app.midi_dlg.is_none());
        // Live mirrors adopted the scratch.
        assert_eq!(app.midi.backend, "Dummy");
        assert_eq!(app.midi.port, "dummy");
        assert_eq!(app.midi.soundfont, "/sf/gm.sf2");
        // Persisted (save-point row 2).
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("lastOutputBackend=Dummy\n"));
        assert!(disk.contains("lastOutputConnection=dummy\n"));
        assert!(disk.contains("soundFont=/sf/gm.sf2\n"));
        // The engine answers; the user-initiated apply announces.
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert!(app.midi.output_ok_once);
        assert_eq!(app.toasts, vec!["MIDI output: Dummy dummy".to_string()]);
    }

    #[test]
    fn midi_apply_no_change_sends_only_sysex_reset() {
        // Stop-first delta (T13a): a no-change Apply must not interrupt
        // playback — C++ stops unconditionally via `connectOutput`, the
        // port plans first and sends only the state-only `SetSysexReset`
        // (correct C++ `:1172` parity — T13g F2: the old "nothing at all"
        // wording overclaimed). No `Stop`, no reselect.
        let (mut app, rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        // T14d minor d: the steady-state refill (shown == want).
        feed_ports(
            &mut app,
            vec![midi_port("fluidsynth", "FluidSynth (gm.sf2)")],
        );
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.dialog, None);
        assert!(app.midi_dlg.is_none());
        // The engine answers every `SelectOutput` — silence proves none.
        assert!(
            rx.recv_lossless_timeout(Duration::from_millis(300))
                .is_none(),
            "no-change Apply must send no Stop/reselect"
        );
        assert!(app.toasts.is_empty());
        assert!(!app.midi.announce_applied);
    }

    #[test]
    fn midi_apply_map_only_sends_only_sysex_reset_but_persists() {
        // Map/reset-only Apply: committed + persisted; the only engine
        // send is the state-only `SetSysexReset` (T13g F2 — the old "no
        // engine traffic" wording overclaimed). The patch model relabels
        // render-time; C++ would still stop + reselect here.
        let (mut app, rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiInstrumentMapChanged(2));
        upd(&mut app, Message::MidiResetChanged(4));
        // T14d minor d: the steady-state refill (shown == want).
        feed_ports(
            &mut app,
            vec![midi_port("fluidsynth", "FluidSynth (gm.sf2)")],
        );
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.settings.instrument_map, 2);
        assert_eq!(app.settings.sysex_reset, 4);
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("instrumentMap=2\n"));
        assert!(disk.contains("sysexReset=4\n"));
        assert!(
            rx.recv_lossless_timeout(Duration::from_millis(300))
                .is_none(),
            "map-only Apply must send no Stop/reselect"
        );
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn midi_apply_syncs_map_reset_mirrors_for_reopen() {
        // T13d F1: Apply must sync the map/reset live mirrors — the D3 seed
        // reads them, so without the sync a reopen seeds the stale values
        // and a no-change Apply clobbers the commit with them.
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiInstrumentMapChanged(2));
        upd(&mut app, Message::MidiResetChanged(4));
        upd(&mut app, Message::MidiApply);
        // Mirrors follow the commit.
        assert_eq!(app.midi.instrument_map, 2);
        assert_eq!(app.midi.sysex_reset, 4);
        // Reopen seeds the committed values (not the stale 0/1 defaults).
        open_midi_setup(&mut app);
        assert_eq!(scratch(&app).instrument_map, 2);
        assert_eq!(scratch(&app).sysex_reset, 4);
        // …so a no-change Apply persists them unchanged (no clobber).
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.settings.instrument_map, 2);
        assert_eq!(app.settings.sysex_reset, 4);
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("instrumentMap=2\n"));
        assert!(disk.contains("sysexReset=4\n"));
    }

    #[test]
    fn midi_apply_changed_stops_first_then_reselects_e2e() {
        // Stop-first over a real playing song: the Apply sends `Stop`
        // before `SelectOutput` (structural — sequential sends on one
        // channel), and both take effect: the reselect answers with
        // `OutputChanged`, and the stop lands mid-playback, so the
        // scheduler postlude emits `Stopped`. Pumped reselect-first: the
        // reselect answers synchronously out of `handle_command`, while
        // `Stopped` waits for the postlude — arrival order is NOT send
        // order here.
        let (mut app, rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("examples/test.mid")]);
        upd(&mut app, Message::Play);
        pump_opened(&mut app, &rx);
        pump_until(&mut app, &rx, Duration::from_secs(10), |ev| {
            matches!(ev, PlayerEvent::Started)
        });
        open_midi_setup(&mut app);
        {
            let dlg = app.midi_dlg.as_mut().unwrap();
            dlg.scratch.backend = "Dummy".to_string();
            dlg.scratch.port = "dummy".to_string();
        }
        // T14d minor d: Apply reads the DROPDOWN — land the refill so the
        // shown id is the want.
        feed_ports(&mut app, vec![midi_port("dummy", "Dummy (no output)")]);
        upd(&mut app, Message::MidiApply);
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Stopped)
        });
        assert_eq!(app.playback.status, Status::Stopped);
        assert_eq!(app.toasts, vec!["MIDI output: Dummy dummy".to_string()]);
    }

    #[test]
    fn midi_apply_no_change_while_playing_sends_no_stop_e2e() {
        // T13g F2: idle `Stop` is event-silent (`player.rs:640-649`), so
        // the idle pins above prove no-`SelectOutput` but CANNOT prove
        // no-`Stop`. Over real playback a `Stop` lands mid-scheduler and
        // the postlude emits `Stopped` (cf. the changed-Apply pin above)
        // — so: play, no-change Apply, drain a window of live events, and
        // assert no `Stopped`/`OutputChanged` arrived with status still
        // `Playing`. (Playback emits `Time`/`Note` traffic, so this drains
        // rather than asserting total silence.)
        let (mut app, rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("examples/test.mid")]);
        upd(&mut app, Message::Play);
        pump_opened(&mut app, &rx);
        pump_until(&mut app, &rx, Duration::from_secs(10), |ev| {
            matches!(ev, PlayerEvent::Started)
        });
        open_midi_setup(&mut app);
        // T14d minor d: the steady-state refill (shown == want).
        feed_ports(
            &mut app,
            vec![midi_port("fluidsynth", "FluidSynth (gm.sf2)")],
        );
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.dialog, None);
        assert!(app.midi_dlg.is_none());
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        loop {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            match rx.recv_lossless_timeout(deadline - now) {
                Some(ev) => {
                    assert!(
                        !matches!(ev, PlayerEvent::Stopped),
                        "no-change Apply must not stop playback"
                    );
                    assert!(
                        !matches!(ev, PlayerEvent::OutputChanged { .. }),
                        "no-change Apply must not reselect"
                    );
                    upd(&mut app, Message::Player(ev));
                }
                None => {}
            }
        }
        assert_eq!(app.playback.status, Status::Playing);
        assert!(app.toasts.is_empty());
        upd(&mut app, Message::Stop);
    }

    #[test]
    fn midi_test_note_commits_stays_open_and_announces_e2e() {
        let (mut app, rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        {
            let dlg = app.midi_dlg.as_mut().unwrap();
            dlg.scratch.backend = "Dummy".to_string();
            dlg.scratch.port = "dummy".to_string();
            dlg.scratch.soundfont = "/sf/gm.sf2".to_string();
            dlg.scratch.ports = vec![crate::core::output::MidiPort {
                id: "dummy".to_string(),
                label: "No MIDI output".to_string(),
            }];
        }
        upd(&mut app, Message::MidiTestNote);
        // Committed verbatim (settings + live mirror), dialog stays open,
        // optimistic status set. (The 400 ms `MidiTestNoteOff` task is
        // dropped by `upd` — its arm is pinned separately below.)
        assert_eq!(app.settings.sound_font, "/sf/gm.sf2");
        assert_eq!(app.midi.soundfont, "/sf/gm.sf2");
        assert_eq!(app.dialog, Some(Dialog::MidiSetup));
        assert!(app.midi_dlg.is_some());
        assert_eq!(
            app.midi_dlg.as_ref().unwrap().test_status.as_deref(),
            Some(TEST_NOTE_STATUS)
        );
        // The unconditional reselect answers + announces like Apply.
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert_eq!(app.toasts, vec!["MIDI output: Dummy dummy".to_string()]);
    }

    #[test]
    fn midi_test_note_empty_soundfont_commits_verbatim() {
        // C++ assigns the entry unconditionally (`:1138`) — empty included
        // (unlike Apply's skip-empty rule).
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(
            &mut app,
            Message::MidiSoundFontChanged("/sf/before.sf2".to_string()),
        );
        upd(&mut app, Message::MidiSoundFontChanged(String::new()));
        upd(&mut app, Message::MidiTestNote);
        assert!(app.settings.sound_font.is_empty());
        assert!(app.midi.soundfont.is_empty());
        assert_eq!(
            app.midi_dlg.as_ref().unwrap().test_status.as_deref(),
            Some(TEST_NOTE_STATUS)
        );
    }

    #[test]
    fn midi_test_note_failure_surfaces_no_output_status() {
        // C++ `:1143-1145`: a failing test reselect sets the status line
        // (alongside the toast) instead of the sent status.
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiTestNote);
        assert_eq!(
            app.midi_dlg.as_ref().unwrap().test_status.as_deref(),
            Some(TEST_NOTE_STATUS)
        );
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "ALSA".to_string(),
                port: "128:0".to_string(),
                warning: Some("Unable to open MIDI output".to_string()),
            }),
        );
        assert_eq!(
            app.midi_dlg.as_ref().unwrap().test_status.as_deref(),
            Some(NO_MIDI_OUTPUT)
        );
        assert!(
            app.toasts
                .contains(&"Unable to open MIDI output".to_string())
        );
    }

    #[test]
    fn midi_test_note_off_is_silent() {
        // The release arm only sends the raw NoteOff — no state, no toast.
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiTestNote);
        let epoch = app.output_epoch;
        upd(
            &mut app,
            Message::MidiTestNoteOff {
                ch: 0,
                note: 60,
                epoch,
            },
        );
        assert_eq!(app.dialog, Some(Dialog::MidiSetup));
        assert!(app.toasts.is_empty());
        // …and without a shell too (a late timer after close — here with a
        // stale capture, pinning the skip half too).
        upd(&mut app, Message::CloseDialog);
        upd(
            &mut app,
            Message::MidiTestNoteOff {
                ch: 0,
                note: 60,
                epoch: epoch.wrapping_add(1),
            },
        );
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn midi_refill_clears_transient_test_status() {
        // C++ refill overwrites the status label (`:1055-1058`).
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiTestNote);
        assert!(app.midi_dlg.as_ref().unwrap().test_status.is_some());
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "FluidSynth".to_string(),
                advanced: false,
                ports: vec![],
            },
        );
        assert!(app.midi_dlg.as_ref().unwrap().test_status.is_none());
    }

    #[test]
    fn midi_refill_drops_stale_arrivals() {
        // T13g F4: two rapid backend switches launch two refills; each
        // task blocks on ALSA enumeration, so the first task's ports can
        // arrive late — the backend+advanced echo must drop them instead
        // of storing A's ports under B.
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiBackendSelected("ALSA".to_string()));
        upd(&mut app, Message::MidiBackendSelected("Dummy".to_string()));
        // A's ports arrive late: echo mismatch → dropped.
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "ALSA".to_string(),
                advanced: false,
                ports: vec![crate::core::output::MidiPort {
                    id: "128:0".to_string(),
                    label: "ALSA port".to_string(),
                }],
            },
        );
        assert!(scratch(&app).ports.is_empty());
        // B's ports arrive: echo match → stored.
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "Dummy".to_string(),
                advanced: false,
                ports: vec![crate::core::output::MidiPort {
                    id: "dummy".to_string(),
                    label: "Dummy".to_string(),
                }],
            },
        );
        assert_eq!(scratch(&app).ports.len(), 1);
        assert_eq!(scratch(&app).ports[0].id, "dummy");
        // Same for the `Show all` leg: a pre-toggle arrival drops.
        upd(&mut app, Message::MidiAdvancedToggled(true));
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "Dummy".to_string(),
                advanced: false,
                ports: vec![crate::core::output::MidiPort {
                    id: "stale".to_string(),
                    label: "Stale".to_string(),
                }],
            },
        );
        assert!(scratch(&app).ports.is_empty());
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "Dummy".to_string(),
                advanced: true,
                ports: vec![crate::core::output::MidiPort {
                    id: "dummy".to_string(),
                    label: "Dummy".to_string(),
                }],
            },
        );
        assert_eq!(scratch(&app).ports.len(), 1);
    }

    #[test]
    fn enumerate_ports_lists_known_backends() {
        // FluidSynth + Dummy enumerate deterministically (no hardware);
        // unknown names fall back to the front backend (manager A6c rule —
        // pinned by equality with ALSA, whatever the host sequencer says).
        let fluid = enumerate_ports("FluidSynth", false);
        assert_eq!(fluid.len(), 1);
        assert_eq!(fluid[0].id, "fluidsynth");
        let dummy = enumerate_ports("Dummy", true);
        assert_eq!(
            dummy,
            vec![crate::core::output::MidiPort {
                id: "dummy".to_string(),
                label: "No MIDI output".to_string(),
            }]
        );
        assert_eq!(
            enumerate_ports("NoSuchBackend", false),
            enumerate_ports("ALSA", false)
        );
    }

    // -- prefs / song settings ------------------------------------------------------

    #[test]
    fn prefs_changed_applies_and_saves_u14() {
        let (mut app, _rx, _tmp) = test_app();
        let mut prefs = Settings::new();
        prefs.solo_volume_reduction = 75;
        prefs.drums_channel = 9;
        upd(&mut app, Message::PrefsChanged(prefs));
        assert_eq!(app.settings.solo_volume_reduction, 75);
        assert_eq!(app.settings.drums_channel, 9);
        assert_eq!(app.midi.sysex_reset, 1);
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("soloVolumeReduction=75\n"));
        assert!(disk.contains("drumsChannel=9\n"));
    }

    /// General rows end to end through the close path: wholesale adoption +
    /// persistence, clamped mirrors, solo% mix repush (U14), and no output
    /// touch when the D3-owned keys are unchanged.
    #[test]
    fn prefs_general_rows_apply_per_row() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(&mut app, Message::ChannelSolo(1, true));
        upd(&mut app, Message::ChannelLevel(0, 200));
        let mut prefs = Settings::new();
        prefs.drums_channel = 99; // adopted raw; engine send clamps to ch 15
        prefs.sysex_reset = 9; // mirror clamps to 4
        prefs.instrument_map = 9; // mirror clamps to 2
        prefs.solo_volume_reduction = 75;
        prefs.auto_play = false;
        prefs.auto_advance = false;
        prefs.auto_song_settings = true;
        upd(&mut app, Message::PrefsChanged(prefs));
        assert_eq!(app.settings.drums_channel, 99);
        assert!(!app.settings.auto_play);
        assert!(!app.settings.auto_advance);
        assert!(app.settings.auto_song_settings);
        assert_eq!(app.midi.sysex_reset, 4);
        assert_eq!(app.midi.instrument_map, 2);
        // Solo% repushes the mix at close: non-solo ch 0 reads 25% × 200%.
        let sync = channels::sync_all(&app.channels, app.settings.solo_volume_reduction);
        assert!((sync[0].factor - 0.5).abs() < 1e-9);
        assert!((sync[1].factor - 1.0).abs() < 1e-9);
        // Output keys unchanged → no reseat, no announce, mirrors untouched.
        assert!(!app.midi.announce_applied);
        assert_eq!(app.midi.backend, "FluidSynth");
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("soloVolumeReduction=75\n"));
        assert!(disk.contains("autoSongSettings=true\n"));
    }

    /// A prefs commit carrying a new backend reseats the output end to end:
    /// mirrors follow, the engine answers, the close-path apply announces.
    #[test]
    fn prefs_backend_switch_reseats_and_announces_e2e() {
        let (mut app, rx, _tmp) = test_app();
        let mut prefs = Settings::new();
        prefs.last_output_backend = "Dummy".to_string();
        prefs.last_output_connection = "dummy".to_string();
        prefs.sound_font = "/sf/gm.sf2".to_string();
        upd(&mut app, Message::PrefsChanged(prefs));
        assert_eq!(app.midi.backend, "Dummy");
        assert_eq!(app.midi.port, "dummy");
        assert_eq!(app.midi.soundfont, "/sf/gm.sf2");
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert!(app.midi.output_ok_once);
        assert_eq!(app.toasts, vec!["MIDI output: Dummy dummy".to_string()]);
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("lastOutputBackend=Dummy\n"));
    }

    /// SoundFont/advanced commits adopt into the mirrors without reselecting:
    /// no `SelectOutput`, no announce. Clearing the SoundFont clears the
    /// mirror (the engine keeps its font — no send — per the reseat plan).
    #[test]
    fn prefs_soundfont_and_advanced_adopt_without_reselect() {
        let (mut app, _rx, _tmp) = test_app();
        let mut prefs = Settings::new();
        prefs.sound_font = "/sf/new.sf2".to_string();
        prefs.advanced_ports = true;
        upd(&mut app, Message::PrefsChanged(prefs));
        assert_eq!(app.midi.soundfont, "/sf/new.sf2");
        assert!(app.midi.advanced);
        assert!(!app.midi.announce_applied);
        assert_eq!(app.midi.backend, "FluidSynth");
        let mut prefs = app.settings.clone();
        prefs.sound_font = String::new();
        upd(&mut app, Message::PrefsChanged(prefs));
        assert!(app.midi.soundfont.is_empty());
        assert!(!app.midi.announce_applied);
    }

    /// Piano rows end to end through the close path: adopted wholesale and
    /// persisted (render-time rows — the T13 canvas reads settings per draw).
    #[test]
    fn prefs_piano_rows_adopt_and_persist() {
        let (mut app, _rx, _tmp) = test_app();
        let mut prefs = Settings::new();
        prefs.highlight_palette = 0;
        prefs.single_color = "red".to_string();
        prefs.velocity_color = false;
        prefs.names_visibility = 3;
        prefs.octave_subscript = true;
        upd(&mut app, Message::PrefsChanged(prefs));
        assert_eq!(app.settings.highlight_palette, 0);
        assert_eq!(app.settings.single_color, "red");
        assert!(!app.settings.velocity_color);
        assert_eq!(app.settings.names_visibility, 3);
        assert!(app.settings.octave_subscript);
        // The renderer parses `singleColor` under gdk_rgba parity (gray on
        // garbage — `None` here).
        assert!(crate::state::lyrics::parse_gdk_rgba(&app.settings.single_color).is_some());
        assert!(crate::state::lyrics::parse_gdk_rgba("garbage").is_none());
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("highlightPalette=0\n"));
        assert!(disk.contains("singleColor=red\n"));
        assert!(disk.contains("namesVisibility=3\n"));
    }

    #[test]
    fn song_settings_save_writes_used_channels_only() {
        let (mut app, _rx, _tmp) = test_app();
        // No song → no-op.
        upd(&mut app, Message::SongSettingsSave);
        assert!(!app.paths.data_dir().exists());

        app.song = Some(test_song());
        app.playlist.set_items(vec![PathBuf::from("/m/song.mid")]);
        app.channels[0].used = true;
        app.channels[0].label = "Lead".to_string();
        app.channels[0].muted = true;
        app.channels[0].patch = 5;
        app.playback.volume_pct = 150;
        app.playback.pitch_semitones = -2;
        app.lyrics.codec = "UTF-8".to_string();
        upd(&mut app, Message::SongSettingsSave);

        // T8 obligation: the save path creates the data dir.
        let path = app.paths.song_settings_path("song.mid");
        assert!(path.is_file());
        let disk = std::fs::read_to_string(&path).unwrap();
        assert!(disk.contains("volume=150\n"));
        assert!(disk.contains("pitch=-2\n"));
        assert!(disk.contains("[MIDI Channel  1]\n"));
        assert!(disk.contains("name=Lead\n"));
        assert!(disk.contains("muted=true\n"));
        assert!(disk.contains("patch=5\n"));
        assert!(!disk.contains("MIDI Channel  2"));
    }

    #[test]
    fn song_settings_load_applies_and_clamps() {
        let (mut app, _rx, _tmp) = test_app();
        app.song = Some(test_song());
        app.playlist.set_items(vec![PathBuf::from("/m/song.mid")]);
        // Missing file → silent no-op.
        upd(&mut app, Message::SongSettingsLoad);
        assert_eq!(app.playback.volume_pct, 100);

        app.paths.ensure_data_dir().unwrap();
        std::fs::write(
            app.paths.song_settings_path("song.mid"),
            "[Global]\nencoding=SHIFT_JIS\nvolume=999\npitch=99\ntimeskew=-5\n\n\
             [MIDI Channel  1]\nname=Keys\nmuted=true\nsolo=true\nlocked=true\n\
             patch=999\nlevel=999\n",
        )
        .unwrap();
        upd(&mut app, Message::SongSettingsLoad);
        // Widget-range clamps mirror the GTK controls.
        assert_eq!(app.playback.volume_pct, 200);
        assert_eq!(app.playback.pitch_semitones, 12);
        assert_eq!(app.playback.tempo_factor_pct, 50);
        assert_eq!(app.lyrics.codec, "SHIFT_JIS");
        assert_eq!(app.channels[0].label, "Keys");
        assert!(app.channels[0].muted);
        assert!(app.channels[0].soloed);
        assert!(app.channels[0].locked);
        assert_eq!(app.channels[0].patch, 127);
        assert_eq!(app.channels[0].level_pct, 200);

        // Corrupt file → silent no-op (state untouched).
        std::fs::write(
            app.paths.song_settings_path("song.mid"),
            "garbage no group\n",
        )
        .unwrap();
        upd(&mut app, Message::SongSettingsLoad);
        assert_eq!(app.playback.volume_pct, 200);
        assert_eq!(app.lyrics.codec, "SHIFT_JIS");
    }

    #[test]
    fn manual_song_actions_ungated_by_auto_flag() {
        // C++ `app.save/load-settings` call save/load directly (`:1405-1408`)
        // — the `autoSongSettings` flag drives only the auto call sites.
        // Both flag states run the action with a song loaded.
        for auto in [false, true] {
            let (mut app, _rx, _tmp) = test_app();
            app.song = Some(test_song());
            app.settings.auto_song_settings = auto;
            upd(&mut app, Message::SongSettingsSave);
            assert!(
                app.paths.song_settings_path("song.mid").is_file(),
                "manual save must run with auto_song_settings={auto}"
            );
            // Load runs too (silent — the fresh save parses cleanly).
            upd(&mut app, Message::SongSettingsLoad);
            assert!(app.toasts.is_empty());
        }
    }

    #[test]
    fn manual_song_actions_noop_on_empty_title() {
        // `manual_song_action_allowed`: `Some("")` is as gated as `None`
        // (the C++ empty-name guards, `:633-635`/`:663-665`).
        let (mut app, _rx, _tmp) = test_app();
        let mut song = test_song();
        song.title = String::new();
        app.song = Some(song);
        upd(&mut app, Message::SongSettingsSave);
        upd(&mut app, Message::SongSettingsLoad);
        assert!(app.toasts.is_empty());
        assert!(!app.paths.data_dir().exists());
    }

    // -- dialogs / navigation ---------------------------------------------------------

    #[test]
    fn dialogs_view_and_toasts() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        assert_eq!(app.dialog, Some(Dialog::Preferences));
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.dialog, None);
        upd(&mut app, Message::ShowFileInfo);
        assert_eq!(app.dialog, Some(Dialog::FileInfo));
        upd(&mut app, Message::ShowAbout);
        assert_eq!(app.dialog, Some(Dialog::About));
        upd(&mut app, Message::ShowHelp);
        assert_eq!(app.dialog, Some(Dialog::Help));
        upd(&mut app, Message::ViewSelected(View::Piano));
        assert_eq!(app.view, View::Piano);
        upd(&mut app, Message::Toast("a".to_string()));
        upd(&mut app, Message::Toast("b".to_string()));
        assert_eq!(app.toasts, vec!["a".to_string(), "b".to_string()]);
        upd(&mut app, Message::DismissToast);
        assert_eq!(app.toasts, vec!["b".to_string()]);
        upd(&mut app, Message::DismissToast);
        upd(&mut app, Message::DismissToast); // empty → no panic
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn info_dialogs_abandon_shells_like_switch() {
        // T13g F3: `ShowFileInfo`/`ShowAbout`/`ShowHelp` route through
        // `open_dialog`, so the D3/prefs shells are abandoned exactly like
        // any dialog switch — no hidden shell survives under the info
        // dialog for a stale `MidiApply` to commit (the F2(a) hazard).
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiInstrumentMapChanged(2));
        upd(&mut app, Message::ShowFileInfo);
        assert_eq!(app.dialog, Some(Dialog::FileInfo));
        assert!(app.midi_dlg.is_none());
        // A stale Apply now no-ops: settings untouched, dialog still open.
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.settings.instrument_map, 0);
        assert_eq!(app.dialog, Some(Dialog::FileInfo));
        // Same for the prefs shell, and the other two info dialogs.
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        assert!(app.prefs.is_some());
        upd(&mut app, Message::ShowAbout);
        assert!(app.prefs.is_none());
        assert_eq!(app.dialog, Some(Dialog::About));
        open_midi_setup(&mut app);
        upd(&mut app, Message::ShowHelp);
        assert!(app.midi_dlg.is_none());
        assert_eq!(app.dialog, Some(Dialog::Help));
    }

    #[test]
    fn open_url_launches_silently_errors_toast() {
        // `OpenUrl` only launches the opener task — no state change;
        // `UrlOpened(Ok)` is silent (D13 precedent), `Err` toasts.
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::ShowDialog(Dialog::About));
        upd(
            &mut app,
            Message::OpenUrl("https://example.invalid/".to_string()),
        );
        assert_eq!(app.dialog, Some(Dialog::About));
        assert!(app.toasts.is_empty());
        upd(&mut app, Message::UrlOpened(Ok(())));
        assert!(app.toasts.is_empty());
        upd(
            &mut app,
            Message::UrlOpened(Err("Could not open x: boom".to_string())),
        );
        assert_eq!(app.toasts, vec!["Could not open x: boom".to_string()]);
    }

    #[test]
    fn toast_stack_capped_oldest_dropped() {
        let (mut app, _rx, _tmp) = test_app();
        for i in 0..10 {
            upd(&mut app, Message::Toast(format!("t{i}")));
        }
        assert_eq!(app.toasts.len(), 8);
        assert_eq!(app.toasts[0], "t2".to_string());
        assert_eq!(app.toasts[7], "t9".to_string());
    }

    #[test]
    fn loop_dialog_cancel_resets_unconfirmed_toggle() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::LoopToggled(true));
        assert_eq!(app.dialog, Some(Dialog::Loop));
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.dialog, None);
        assert!(!app.playback.loop_enabled);
        // A confirmed loop survives dialog churn.
        upd(&mut app, Message::LoopSet { start: 1, end: 2 });
        upd(&mut app, Message::LoopToggled(true));
        upd(&mut app, Message::CloseDialog);
        assert!(app.playback.loop_enabled);
    }

    // -- engine → UI --------------------------------------------------------------------

    #[test]
    fn opened_syncs_ui_to_engine_a8() {
        let (mut app, _rx, tmp) = test_app();
        // Dirty UI state from a previous song.
        app.channels[0].muted = true;
        app.channels[1].soloed = true;
        app.channels[2].locked = true;
        app.channels[0].patch = 40;
        app.channels[0].level_pct = 20;
        app.channels[0].label = "Stale".to_string();
        app.playback.loop_enabled = true;
        app.playback.position_ticks = 999;
        app.playback.seeking = true;
        app.lyrics.codec = "SHIFT_JIS".to_string();
        app.lyrics.live_past = "stale".to_string();
        app.lyrics.live_current = "tail".to_string();
        app.lyrics.sung = true;
        app.piano.note_on(0, 60, 100);
        let song_path = tmp.path().join("song.mid");
        std::fs::write(&song_path, b"fake").unwrap();
        app.playlist.set_items(vec![song_path.clone()]);

        upd(
            &mut app,
            Message::Player(PlayerEvent::Opened(Box::new(test_song_into_snapshot()))),
        );
        // A8: mix reset to the engine-fresh state, used+labels from snapshot.
        assert!(!app.channels[0].muted);
        assert!(!app.channels[1].soloed);
        assert!(!app.channels[2].locked);
        assert_eq!(app.channels[0].patch, 0);
        assert_eq!(app.channels[0].level_pct, 100);
        assert_eq!(app.channels[0].label, "Track0");
        assert!(app.channels[0].used);
        assert!(app.channels[1].used);
        assert!(!app.channels[2].used);
        // Loop synced to the engine reset; position/clock cleared.
        assert!(!app.playback.loop_enabled);
        assert_eq!((app.playback.loop_start, app.playback.loop_end), (1, 16));
        assert_eq!(app.playback.position_ticks, 0);
        assert_eq!(app.playback.elapsed_ms, 0);
        assert!(!app.playback.seeking);
        assert_eq!(app.playback.song_len_ticks, 4800);
        // Lyrics codec reseeded, tail cleared; piano cleared.
        assert_eq!(app.lyrics.codec, "UTF-8");
        assert!(app.lyrics.live_tail().is_empty());
        assert!(!app.lyrics.sung);
        assert!(!app.piano.notes_on[0][60]);
        // No autoplay → Stopped; recent + directory recorded (unsaved).
        assert_eq!(app.playback.status, Status::Stopped);
        assert_eq!(
            app.settings.recent_files,
            vec![song_path.to_string_lossy().into_owned()]
        );
        assert_eq!(app.settings.last_directory, tmp.path().to_string_lossy());
        assert!(!app.paths.config_file().exists());
    }

    fn test_song_into_snapshot() -> SongSnapshot {
        SongSnapshot {
            title: "song.mid".to_string(),
            format: "SMF".to_string(),
            bars: 16,
            len_ticks: 4800,
            duration_ms: 60_000,
            division: 96,
            num_tracks: 2,
            initial_tempo_us: 500_000.0,
            channel_used: std::array::from_fn(|ch| ch < 2),
            channel_labels: std::array::from_fn(|ch| format!("Track{ch}")),
            lowest_note: 36,
            highest_note: 96,
            charset: "UTF-8".to_string(),
            texts: Vec::new(),
            metadata: Vec::new(),
            copyrights: Vec::new(),
        }
    }

    #[test]
    fn opened_autoplay_starts_playback() {
        let (mut app, _rx, _tmp) = test_app();
        app.pending_autoplay = true;
        upd(
            &mut app,
            Message::Player(PlayerEvent::Opened(Box::new(test_song_into_snapshot()))),
        );
        assert_eq!(app.playback.status, Status::Playing);
        assert!(!app.pending_autoplay);
    }

    /// Load-time mix sync vs `autoSongSettings`: auto mode overlays the song
    /// mix on the reset rows at load; auto OFF ignores the cfg (pure reset).
    #[test]
    fn opened_with_auto_song_settings_overlays_mix() {
        let (mut app, _rx, _tmp) = test_app();
        app.paths.ensure_data_dir().unwrap();
        std::fs::write(
            app.paths.song_settings_path("song.mid"),
            "[Global]\nvolume=150\npitch=-2\n\n\
             [MIDI Channel  1]\nname=Keys\nmuted=true\nsolo=true\nlocked=true\n\
             patch=5\nlevel=80\n",
        )
        .unwrap();
        app.settings.auto_song_settings = true;
        upd(
            &mut app,
            Message::Player(PlayerEvent::Opened(Box::new(test_song_into_snapshot()))),
        );
        assert_eq!(app.channels[0].label, "Keys");
        assert!(app.channels[0].muted);
        assert!(app.channels[0].soloed);
        assert!(app.channels[0].locked);
        assert_eq!(app.channels[0].patch, 5);
        assert_eq!(app.channels[0].level_pct, 80);
        assert_eq!(app.playback.volume_pct, 150);
        assert_eq!(app.playback.pitch_semitones, -2);
        // Untouched rows stay fresh-reset.
        assert!(!app.channels[1].muted);
        assert_eq!(app.channels[1].level_pct, 100);
        assert_eq!(app.channels[1].label, "Track1");

        // Auto OFF with the same cfg present: pure reset, cfg ignored.
        let (mut app, _rx, _tmp) = test_app();
        app.paths.ensure_data_dir().unwrap();
        std::fs::write(
            app.paths.song_settings_path("song.mid"),
            "[Global]\nvolume=150\n\n[MIDI Channel  1]\nmuted=true\n",
        )
        .unwrap();
        assert!(!app.settings.auto_song_settings);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Opened(Box::new(test_song_into_snapshot()))),
        );
        assert!(!app.channels[0].muted);
        assert_eq!(app.playback.volume_pct, 100);
    }

    #[test]
    fn load_failed_toasts_and_clears_pending() {
        let (mut app, _rx, _tmp) = test_app();
        app.pending_autoplay = true;
        upd(
            &mut app,
            Message::Player(PlayerEvent::LoadFailed {
                file: "/m/broken.mid".to_string(),
                errors: Vec::new(),
            }),
        );
        assert!(!app.pending_autoplay);
        assert_eq!(app.toasts, vec!["Could not load broken.mid".to_string()]);
        upd(
            &mut app,
            Message::Player(PlayerEvent::LoadFailed {
                file: "/m/damaged.mid".to_string(),
                errors: vec!["truncated".to_string()],
            }),
        );
        assert_eq!(
            app.toasts,
            vec![
                "Could not load broken.mid".to_string(),
                "Could not load damaged.mid".to_string(),
                "Warning: non-standard or damaged file".to_string(),
            ]
        );
    }

    #[test]
    fn time_beat_tempo_program_note_flow() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Time {
                elapsed_ms: 5_000,
                ticks: 2400,
            }),
        );
        assert_eq!(app.playback.elapsed_ms, 5_000);
        assert_eq!(app.playback.position_ticks, 2400);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Beat {
                bar: 3,
                beat: 2,
                max: 4,
            }),
        );
        assert_eq!(app.playback.bar, 3);
        assert_eq!(app.playback.beat, 2);
        assert_eq!(app.playback.beats_per_bar, 4);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Tempo {
                tempo_us: 250_000.0,
            }),
        );
        assert!((app.playback.bpm - 240.0).abs() < 1e-9);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Program { ch: 0, patch: 200 }),
        );
        assert_eq!(app.channels[0].patch, 127);
        upd(
            &mut app,
            Message::Player(PlayerEvent::NoteOn {
                ch: 0,
                note: 60,
                vel: 100,
            }),
        );
        assert!(app.piano.notes_on[0][60]);
        assert!((app.channels[0].level - 100.0 / 127.0).abs() < 1e-6);
        upd(
            &mut app,
            Message::Player(PlayerEvent::NoteOff {
                ch: 0,
                note: 60,
                vel: 64,
            }),
        );
        assert!(!app.piano.notes_on[0][60]);
        // Signature events: no UI handler (C++ parity).
        upd(
            &mut app,
            Message::Player(PlayerEvent::KeySignature {
                track: 0,
                alterations: 0,
                minor: false,
            }),
        );
        upd(
            &mut app,
            Message::Player(PlayerEvent::TimeSignature {
                bar: 1,
                num: 4,
                den: 4,
            }),
        );
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn live_text_filters_and_decodes() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        assert_eq!(app.lyrics.codec, "UTF-8");
        upd(
            &mut app,
            Message::Player(PlayerEvent::Text {
                track: 1,
                text_type: 5,
                ticks: 0,
                bytes: b"Hel".to_vec(),
            }),
        );
        upd(
            &mut app,
            Message::Player(PlayerEvent::Text {
                track: 1,
                text_type: 5,
                ticks: 0,
                bytes: "lo ✓".as_bytes().to_vec(),
            }),
        );
        // Wrong type → filtered out (default lyric-only filter).
        upd(
            &mut app,
            Message::Player(PlayerEvent::Text {
                track: 1,
                text_type: 1,
                ticks: 0,
                bytes: b"skip".to_vec(),
            }),
        );
        assert_eq!(app.lyrics.live_tail(), "Hello ✓");
        // Exact `onText` segmentation: previous `current` is `past` now.
        assert_eq!(app.lyrics.live_past, "Hel");
        assert_eq!(app.lyrics.live_current, "lo ✓");
        assert!(app.lyrics.sung);
        // Track filter set → other tracks filtered out.
        app.lyrics.track_filter = Some(2);
        upd(
            &mut app,
            Message::Player(PlayerEvent::Text {
                track: 1,
                text_type: 5,
                ticks: 0,
                bytes: b"skip".to_vec(),
            }),
        );
        assert_eq!(app.lyrics.live_tail(), "Hello ✓");
    }

    #[test]
    fn started_and_stopped_drive_status() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::Player(PlayerEvent::Started));
        assert_eq!(app.playback.status, Status::Playing);
        upd(&mut app, Message::Player(PlayerEvent::Stopped));
        assert_eq!(app.playback.status, Status::Stopped);
    }

    #[test]
    fn finished_repeat_song_replays_e2e() {
        let (mut app, rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("examples/test.mid")]);
        // The Play button always autoplays the loaded song (C++ :410 —
        // `auto_play` gates open-dialog loads, not Play).
        upd(&mut app, Message::Play);
        pump_opened(&mut app, &rx);
        assert_eq!(app.playback.status, Status::Playing);
        upd(&mut app, Message::Stop);
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Stopped)
        });
        app.playback.repeat = RepeatMode::Song;
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playback.status, Status::Playing);
        // Engine reset + replayed: Started confirms (no save: song not left).
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::Started)
        });
        assert!(!app.paths.data_dir().exists());
        upd(&mut app, Message::Stop);
    }

    #[test]
    fn finished_repeat_list_wraps_and_saves() {
        let (mut app, _rx, _tmp) = test_app();
        app.settings.auto_song_settings = true;
        app.song = Some(test_song());
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        app.playlist.set_current_index(1);
        app.playback.repeat = RepeatMode::Playlist;
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 0);
        assert!(app.pending_autoplay);
        assert!(app.paths.song_settings_path("song.mid").is_file());
    }

    #[test]
    fn flags_action_and_args_forward_files_to_running_instance() {
        // Sending half of the contract CMakeLists pins to the retired
        // `.cmake` successor (receiving half: `activation_files` tests):
        // files → `open` + argv strings; no files → present-only.
        let flags = Flags::new(vec![PathBuf::from("/m/a.mid")], None, None, false, None);
        assert_eq!(flags.action(), Some(&"open"));
        assert_eq!(flags.args(), vec!["/m/a.mid"]);
        let idle = Flags::new(Vec::new(), None, None, false, None);
        assert_eq!(idle.action(), None);
        assert!(idle.args().is_empty());
    }

    #[test]
    fn portable_flags_route_into_paths_with_temp_home() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let env = Env::for_test(
            &home,
            &home.join(".config"),
            &home.join(".local/share"),
            tmp.path(),
        );

        // `-f FILE` routes into `Paths`: portable, config is the file
        // verbatim, data dir still under the (temp) home.
        let flags = Flags::new(
            Vec::new(),
            None,
            None,
            true,
            Some(PathBuf::from("/m/port.conf")),
        );
        let paths = Paths::with_env(env.clone(), flags.portable_config());
        assert!(paths.portable());
        assert_eq!(paths.config_file(), Path::new("/m/port.conf"));
        assert_eq!(paths.data_dir(), home.join(".dmidiplayer"));

        // Bare `-p`: portable, config is `./dmidiplayer.conf`.
        let flags = Flags::new(Vec::new(), None, None, true, None);
        let paths = Paths::with_env(env.clone(), flags.portable_config());
        assert!(paths.portable());
        assert_eq!(paths.config_file(), Path::new("dmidiplayer.conf"));

        // Default: normal XDG paths under the temp home.
        let flags = Flags::default();
        let paths = Paths::with_env(env, flags.portable_config());
        assert!(!paths.portable());
        assert_eq!(
            paths.config_file(),
            home.join(".config/dmidiplayer/dmidiplayer.conf")
        );
    }

    #[test]
    fn finished_save_matrix_stay_saves_and_autoadvance_double_saves() {
        // Auto song settings on: every finish except repeat-song saves
        // before branching (`window.cpp:202-203` parity).
        let (mut app, _rx, _tmp) = test_app();
        app.settings.auto_song_settings = true;
        app.song = Some(test_song());
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        let cfg = app.paths.song_settings_path("song.mid");

        // Repeat-song: no save (the song is not left).
        app.playback.repeat = RepeatMode::Song;
        app.playlist.set_current_index(0);
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert!(!cfg.exists());

        // Repeat-list: saves, then wraps + loads.
        app.playback.repeat = RepeatMode::Playlist;
        app.playlist.set_current_index(1);
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 0);
        assert!(cfg.is_file());
        std::fs::remove_file(&cfg).unwrap();

        // Auto-advance (not last): saves twice — before-branch plus
        // `next_song`'s own save — both pre-advance, so the file equals one
        // pre-advance save exactly (same bytes, harmless).
        app.playback.repeat = RepeatMode::Off;
        app.settings.auto_advance = true;
        app.playlist.set_current_index(0);
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 1);
        assert!(cfg.is_file());
        let auto_bytes = std::fs::read(&cfg).unwrap();
        app.playlist.set_current_index(0);
        app.save_song_settings();
        assert_eq!(std::fs::read(&cfg).unwrap(), auto_bytes);
        std::fs::remove_file(&cfg).unwrap();

        // Stay-finished at last: saves (no advance).
        app.playlist.set_current_index(1);
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 1);
        assert!(cfg.is_file());
        std::fs::remove_file(&cfg).unwrap();

        // Stay-finished with auto-advance off (mid-list): still saves.
        app.settings.auto_advance = false;
        app.playlist.set_current_index(0);
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 0);
        assert!(cfg.is_file());
    }

    #[test]
    fn finished_auto_advance_and_stay_branches() {
        let (mut app, _rx, _tmp) = test_app();
        app.song = Some(test_song());
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        // Auto-advance, not last → next + load.
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playback.status, Status::Finished);
        assert_eq!(app.playlist.current_index(), 1);
        assert!(app.pending_autoplay);
        // At last → stay Finished.
        app.pending_autoplay = false;
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 1);
        assert!(!app.pending_autoplay);
        // Disabled → stay even mid-list.
        app.settings.auto_advance = false;
        app.playlist.set_current_index(0);
        upd(&mut app, Message::Player(PlayerEvent::Finished));
        assert_eq!(app.playlist.current_index(), 0);
    }

    #[test]
    fn output_changed_announces_and_gates_fallback() {
        let (mut app, rx, _tmp) = test_app();
        // Clean + announced (user apply) → success toast.
        app.midi.announce_applied = true;
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "Dummy".to_string(),
                port: "dummy".to_string(),
                warning: None,
            }),
        );
        assert!(app.midi.output_ok_once);
        assert_eq!(app.toasts, vec!["MIDI output: Dummy dummy".to_string()]);
        // Clean + silent (startup) → no toast.
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "Dummy".to_string(),
                port: String::new(),
                warning: None,
            }),
        );
        assert_eq!(app.toasts.len(), 1);
        // Warning after confirmation → toast only, no fallback select.
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "ALSA".to_string(),
                port: "128:0".to_string(),
                warning: Some("gone".to_string()),
            }),
        );
        assert_eq!(app.toasts.len(), 2);
        assert!(
            rx.recv_lossless_timeout(Duration::from_millis(150))
                .is_none()
        );
        // Warning before any confirmation → fallback select goes out.
        app.midi.output_ok_once = false;
        app.midi.backend = "ALSA".to_string();
        app.midi.fallback_stage = 0;
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "ALSA".to_string(),
                port: "128:0".to_string(),
                warning: Some("gone".to_string()),
            }),
        );
        assert_eq!(app.midi.backend, "FluidSynth");
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
    }

    // -- tick / resize / startup helpers --------------------------------------------

    #[test]
    fn tick_decays_meters() {
        let (mut app, _rx, _tmp) = test_app();
        app.channels[0].level = 1.0;
        upd(&mut app, Message::Tick(std::time::Instant::now()));
        assert!((app.channels[0].level - 0.82).abs() < 1e-6);
    }

    #[test]
    fn window_resized_updates_settings_without_saving() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::WindowResized(1280, 800));
        assert_eq!(app.settings.window_width, 1280);
        assert_eq!(app.settings.window_height, 800);
        assert!(!app.paths.config_file().exists());
    }

    #[test]
    fn transport_view_model_contract() {
        let (mut app, _rx, _tmp) = test_app();
        let vm = app.transport_view_model();
        assert_eq!(vm.song_title, "No file loaded");
        assert!(!vm.has_song);
        assert_eq!(vm.status, Status::Ready);
        open_fake_song(&mut app);
        app.playback.elapsed_ms = 61_000;
        app.playback.bar = 3;
        app.playback.beat = 2;
        app.playback.position_ticks = 2400;
        let vm = app.transport_view_model();
        assert_eq!(vm.song_title, "song.mid");
        assert!(vm.has_song);
        assert_eq!(vm.time_text, "00:01:01");
        assert_eq!(vm.tempo_text, "120.0 bpm");
        assert_eq!(vm.volume_text, "100%");
        assert_eq!(vm.pitch, 0);
        assert_eq!(vm.bar_beat, "3:2");
        assert_eq!(vm.beats, 4);
        assert_eq!(vm.beat, 2);
        assert!((vm.position - 0.5).abs() < 1e-9);
        assert!(!vm.seeking);
        assert_eq!(vm.tempo_pct, 100);
        assert_eq!(vm.volume_pct, 100);
        assert!(!vm.loop_enabled);
    }

    #[test]
    fn playlist_view_model_contract() {
        let (mut app, _rx, _tmp) = test_app();
        // Empty: no items, `current == -1` → `None`.
        let vm = app.playlist_view_model();
        assert!(vm.items.is_empty());
        assert_eq!(vm.selected, None);
        // Populated: full paths in playlist order + current as `Some`.
        let items = vec![
            PathBuf::from("/m/b.mid"),
            PathBuf::from("/m/a.mid"),
            PathBuf::from("/m/c.mid"),
        ];
        app.playlist.set_items(items.clone());
        app.playlist.set_current_index(2);
        let vm = app.playlist_view_model();
        assert_eq!(vm.items, items);
        assert_eq!(vm.selected, Some(2));
    }

    #[test]
    fn view_builds_with_sidebar_visible_and_hidden() {
        // `view` is not introspectable, but it must build without panic on
        // both sides of the `playlistVisible` branch (populated playlist +
        // a toast, so every row of the layout is exercised).
        let (mut app, _rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        app.toast("hello".to_string());
        app.settings.playlist_visible = true;
        let _ = app.view();
        app.settings.playlist_visible = false;
        let _ = app.view();
    }

    #[test]
    fn restore_playlist_prefers_last_play_list() {
        let (mut app, _rx, tmp) = test_app();
        let list = tmp.path().join("gig.lst");
        std::fs::write(&list, "a.mid\n").unwrap();
        app.settings.last_play_list = list.to_string_lossy().into_owned();
        app.restore_playlist();
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.playlist.current_index(), 0);
    }

    #[test]
    fn restore_playlist_empty_without_sources() {
        let (mut app, _rx, _tmp) = test_app();
        // Temp CWD has no `examples/` dir; no lastPlayList → stays empty.
        app.restore_playlist();
        assert!(app.playlist.is_empty());
    }

    #[test]
    fn startup_select_output_prefers_cli_flags() {
        let (mut app, rx, _tmp) = test_app();
        app.settings.last_output_backend = "ALSA".to_string();
        app.settings.last_output_connection = "128:0".to_string();
        let flags = Flags::new(
            Vec::new(),
            Some("Dummy".to_string()),
            Some("dummy".to_string()),
            false,
            None,
        );
        app.startup_select_output(&flags);
        assert_eq!(app.midi.backend, "Dummy");
        assert_eq!(app.midi.port, "dummy");
        // No SoundFont configured → the select is the first engine answer.
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert!(app.midi.output_ok_once);
        // Startup select stays silent (no announce flag).
        assert!(app.toasts.is_empty());
    }

    /// S3 stale-SF fallback (ux §15.1, packaging §6): an unreadable configured
    /// SoundFont keeps its settings value (P7: never silently drop) and live
    /// mirror (so D3 shows the stale entry with its warning hint), toasts
    /// naming the stale path, and the engine gets the bundled/default
    /// fallback instead of the stale path. (The engine send is opaque
    /// headless; the toast + preserved values pin the contract here, the D3
    /// hint in `midi_setup`, the exact toast text in `state::midi`.)
    #[test]
    fn startup_stale_soundfont_toasts_and_preserves_stale() {
        let (mut app, rx, _tmp) = test_app();
        app.settings.sound_font = "/home/staleuser/sf/old-gm.sf2".to_string();
        app.settings.last_output_backend = "Dummy".to_string();
        app.settings.last_output_connection = "dummy".to_string();
        app.startup_select_output(&Flags::default());
        // Settings + mirror keep the stale path (P7 + D3 display).
        assert_eq!(app.settings.sound_font, "/home/staleuser/sf/old-gm.sf2");
        assert_eq!(app.midi.soundfont, "/home/staleuser/sf/old-gm.sf2");
        // Exactly one toast, naming the stale path + MIDI Setup (the fallback
        // half varies by machine — pinned purely in `state::midi`).
        assert_eq!(app.toasts.len(), 1);
        assert!(
            app.toasts[0].contains("/home/staleuser/sf/old-gm.sf2"),
            "stale toast must name the path: {:?}",
            app.toasts[0]
        );
        assert!(
            app.toasts[0].contains("MIDI Setup"),
            "stale toast must offer MIDI Setup: {:?}",
            app.toasts[0]
        );
        // The startup reselect still answers cleanly (Dummy ignores the font).
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert!(app.midi.output_ok_once);
    }

    /// S3: a readable configured SoundFont sends verbatim with no toast —
    /// the stale path above must not fire on fresh paths.
    #[test]
    fn startup_fresh_soundfont_sends_verbatim_silent() {
        let (mut app, rx, tmp) = test_app();
        let font = tmp.path().join("fresh-gm.sf2");
        std::fs::write(&font, b"RIFF....sfbk").unwrap();
        app.settings.sound_font = font.to_string_lossy().into_owned();
        app.settings.last_output_backend = "Dummy".to_string();
        app.settings.last_output_connection = "dummy".to_string();
        app.startup_select_output(&Flags::default());
        assert_eq!(app.midi.soundfont, font.to_string_lossy());
        assert!(app.toasts.is_empty());
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert!(app.midi.output_ok_once);
    }

    // -- T12b composition: channels page + prefs shell --------------------------

    #[test]
    fn channels_view_model_contract() {
        let (mut app, _rx, _tmp) = test_app();
        // No song: no rows, no-song switch off, model still 128 wide.
        let vm = app.channels_view_model();
        assert!(vm.rows.is_empty());
        assert!(!vm.has_song);
        assert_eq!(vm.patch_options.len(), 128);

        open_fake_song(&mut app); // ch 0..2 used, labels `Track{ch}`.
        app.channels[0].muted = true;
        app.channels[0].soloed = true;
        app.channels[0].locked = true;
        app.channels[0].level = 0.5;
        app.channels[0].set_level_pct(150);
        app.channels[0].patch = 40;
        app.channels[1].patch = 127;
        let vm = app.channels_view_model();
        assert!(vm.has_song);
        // Used-only, in channel order.
        let chans: Vec<u8> = vm.rows.iter().map(|r| r.channel).collect();
        assert_eq!(chans, vec![0, 1]);
        let row = &vm.rows[0];
        assert_eq!(row.name, "Track0");
        assert!(row.muted && row.soloed && row.locked);
        assert_eq!(row.meter, 0.5);
        assert_eq!(row.level_pct, 150);
        assert_eq!(row.patch, 40);
        assert_eq!(row.patch_label, "Violin"); // GM map default.
        assert_eq!(vm.rows[1].patch_label, "Gunshot");
        assert_eq!(vm.patch_options[0], "Acoustic Grand Piano");
        // Map change relabels rows + model (MT-32).
        app.settings.instrument_map = 2;
        let vm = app.channels_view_model();
        assert_eq!(vm.rows[0].patch_label, "Echo Bell");
        assert_eq!(vm.patch_options[0], "Acou Piano 1");
    }

    #[test]
    fn prefs_open_seeds_scratch_and_pickers() {
        let (mut app, _rx, _tmp) = test_app();
        assert!(app.prefs.is_none());
        app.settings.drums_channel = 9;
        app.settings.future_color = "red".to_string();
        app.settings.past_color = "notacolor".to_string();
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        assert_eq!(app.dialog, Some(Dialog::Preferences));
        let (vm, pickers) = app.prefs.as_ref().expect("shell seeded on open");
        // Scratch is a clone of live settings (all 30 keys).
        assert_eq!(vm.scratch, app.settings);
        assert_eq!(vm.scratch.drums_channel, 9);
        // Pickers seeded from the scratch texts: valid parses, garbage seeds
        // nothing (gray stays render-only), all dialogs shut.
        assert!(pickers.future.get_applied_color().is_some());
        assert!(pickers.past.get_applied_color().is_none());
        assert!(pickers.highlight.get_applied_color().is_some());
        assert!(!pickers.future.get_is_active());
        assert!(!pickers.past.get_is_active());
        assert!(!pickers.highlight.get_is_active());
    }

    #[test]
    fn prefs_scratch_edits_are_isolated_until_close() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        let live_before = app.settings.clone();
        let mut next = live_before.clone();
        next.drums_channel = 3;
        next.solo_volume_reduction = 75;
        next.auto_play = false;
        next.future_color = "blue".to_string();
        next.single_color = "notacolor".to_string();
        upd(&mut app, Message::PrefsScratchEdited(next));
        // Live settings untouched, nothing saved.
        assert_eq!(app.settings, live_before);
        assert!(!app.paths.config_file().exists());
        // Scratch holds the edits.
        let (vm, _) = app.prefs.as_ref().unwrap();
        assert_eq!(vm.scratch.drums_channel, 3);
        assert_eq!(vm.scratch.solo_volume_reduction, 75);
        assert!(!vm.scratch.auto_play);
        assert_eq!(vm.scratch.future_color, "blue");
        // A scratch color edit re-drives its picker model (adopt hook).
        let (_, pickers) = app.prefs.as_ref().unwrap();
        assert!(pickers.future.get_applied_color().is_some());
        // Close commits ONCE: narrow adoption (== wholesale here — live is
        // otherwise untouched) + save + shell cleared.
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.dialog, None);
        assert!(app.prefs.is_none());
        assert_eq!(app.settings.drums_channel, 3);
        assert_eq!(app.settings.solo_volume_reduction, 75);
        assert!(!app.settings.auto_play);
        assert_eq!(app.settings.single_color, "notacolor"); // verbatim, like C++.
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("drumsChannel=3\n"));
        assert!(disk.contains("soloVolumeReduction=75\n"));
    }

    /// Picker-active flags for one row triple (routing probe).
    fn picker_active(app: &App) -> (bool, bool, bool) {
        let (_, pickers) = app.prefs.as_ref().unwrap();
        (
            pickers.future.get_is_active(),
            pickers.past.get_is_active(),
            pickers.highlight.get_is_active(),
        )
    }

    #[test]
    fn prefs_picker_updates_route_per_row() {
        use crate::state::lyrics::parse_gdk_rgba;
        use cosmic::widget::color_picker::ColorPickerUpdate;

        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        // Each toggle routes to its own model only.
        upd(
            &mut app,
            Message::PrefsPickerFuture(ColorPickerUpdate::ToggleColorPicker),
        );
        assert_eq!(picker_active(&app), (true, false, false));
        upd(
            &mut app,
            Message::PrefsPickerFuture(ColorPickerUpdate::ToggleColorPicker),
        );
        upd(
            &mut app,
            Message::PrefsPickerPast(ColorPickerUpdate::ToggleColorPicker),
        );
        assert_eq!(picker_active(&app), (false, true, false));
        upd(
            &mut app,
            Message::PrefsPickerPast(ColorPickerUpdate::ToggleColorPicker),
        );
        upd(
            &mut app,
            Message::PrefsPickerHighlight(ColorPickerUpdate::ToggleColorPicker),
        );
        assert_eq!(picker_active(&app), (false, false, true));
        upd(
            &mut app,
            Message::PrefsPickerHighlight(ColorPickerUpdate::ToggleColorPicker),
        );
        // Reset restores the seeded fallback exactly, on its row only.
        upd(&mut app, Message::PrefsPickerPast(ColorPickerUpdate::Reset));
        let (vm, _) = app.prefs.as_ref().unwrap();
        assert_eq!(vm.scratch.past_color, "#1c71d8");
        assert_eq!(vm.scratch.future_color, "#888888");
        assert_eq!(vm.scratch.highlight_color, "#e01b24");
        upd(
            &mut app,
            Message::PrefsPickerHighlight(ColorPickerUpdate::Reset),
        );
        let (vm, _) = app.prefs.as_ref().unwrap();
        assert_eq!(vm.scratch.highlight_color, "#e01b24");
        assert_eq!(vm.scratch.past_color, "#1c71d8");
        // Save commits the model's applied color as canonical hex-ish text
        // (HSV round trip — one 8-bit step of slack per channel), on its row
        // only.
        upd(
            &mut app,
            Message::PrefsPickerFuture(ColorPickerUpdate::AppliedColor),
        );
        let (vm, _) = app.prefs.as_ref().unwrap();
        let got = parse_gdk_rgba(&vm.scratch.future_color).expect("canonical hex");
        let want = parse_gdk_rgba("#888888").unwrap();
        for (got, want) in [
            (got.r, want.r),
            (got.g, want.g),
            (got.b, want.b),
            (got.a, want.a),
        ] {
            assert!(
                (got - want).abs() < 2.0 / 255.0,
                "{}",
                vm.scratch.future_color
            );
        }
        assert_eq!(vm.scratch.past_color, "#1c71d8");
        // Picker edits are scratch-only too: live settings untouched, unsaved.
        assert_eq!(app.settings, Settings::new());
        assert!(!app.paths.config_file().exists());
    }

    #[test]
    fn prefs_close_commits_once_with_solo_repush() {
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        upd(&mut app, Message::ChannelSolo(1, true));
        upd(&mut app, Message::ChannelLevel(0, 200));
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        let mut next = app.settings.clone();
        next.solo_volume_reduction = 75;
        upd(&mut app, Message::PrefsScratchEdited(next));
        // Pre-close: live solo% untouched (no premature repush).
        assert_eq!(app.settings.solo_volume_reduction, 50);
        // Close: one commit — adoption + full mix repush (U14) + save + clear.
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.settings.solo_volume_reduction, 75);
        assert!(app.prefs.is_none());
        let sync = channels::sync_all(&app.channels, app.settings.solo_volume_reduction);
        assert!((sync[0].factor - 0.5).abs() < 1e-9); // 25% × 200%.
        assert!((sync[1].factor - 1.0).abs() < 1e-9); // soloed keeps slider.
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("soloVolumeReduction=75\n"));
    }

    /// Race family DEAD (T12 devil-BLOCKING FIX): live writes that land
    /// while prefs are open — a resize (OS-level, needs no click), a
    /// playlist-save completion, a song load recording recents — survive the
    /// close, while the scratch D2 edits still commit and scratch D3 edits
    /// (no D2 row writes them) are dropped in favor of live.
    #[test]
    fn prefs_close_keeps_live_writes_and_drops_scratch_d3() {
        let (mut app, _rx, _tmp) = test_app();
        app.settings.last_output_backend = "ALSA".to_string();
        app.settings.last_output_connection = "128:0".to_string();
        app.playlist.set_items(vec![PathBuf::from("/m/race.mid")]);
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        // Scratch: D2 edits plus synthetic D3 edits (unreachable via rows).
        let mut next = app.settings.clone();
        next.drums_channel = 3;
        next.solo_volume_reduction = 75;
        next.last_output_backend = "Dummy".to_string();
        next.last_output_connection = "dummy".to_string();
        next.sound_font = "/sf/scratch.sf2".to_string();
        upd(&mut app, Message::PrefsScratchEdited(next));
        // Live writes while the dialog is open …
        upd(&mut app, Message::WindowResized(800, 600));
        upd(
            &mut app,
            Message::PlaylistSaved(Ok(PathBuf::from("/m/race.lst"))),
        );
        open_fake_song(&mut app); // on_opened records recent + directory.
        upd(&mut app, Message::CloseDialog);
        // … the D2 edits committed …
        assert_eq!(app.settings.drums_channel, 3);
        assert_eq!(app.settings.solo_volume_reduction, 75);
        // … the live writes survived (wholesale would have reverted all) …
        assert_eq!(app.settings.window_width, 800);
        assert_eq!(app.settings.window_height, 600);
        assert_eq!(app.settings.last_play_list, "/m/race.lst");
        assert_eq!(app.settings.recent_files, vec!["/m/race.mid".to_string()]);
        assert_eq!(app.settings.last_directory, "/m");
        // … and the D3 keys come from live (scratch D3 dropped, no reseat).
        assert_eq!(app.settings.last_output_backend, "ALSA");
        assert_eq!(app.settings.last_output_connection, "128:0");
        assert!(app.settings.sound_font.is_empty());
        assert!(!app.midi.announce_applied);
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("windowWidth=800\n"));
        assert!(disk.contains("lastPlayList=/m/race.lst\n"));
        assert!(disk.contains("lastOutputBackend=ALSA\n"));
    }

    #[test]
    fn prefs_arms_without_shell_are_silent_noops() {
        use cosmic::widget::color_picker::ColorPickerUpdate;

        let (mut app, _rx, _tmp) = test_app();
        let live = app.settings.clone();
        // Scratch/picker with no shell: no panic, no live change, no save.
        let mut next = live.clone();
        next.drums_channel = 1;
        upd(&mut app, Message::PrefsScratchEdited(next));
        upd(
            &mut app,
            Message::PrefsPickerFuture(ColorPickerUpdate::Reset),
        );
        upd(
            &mut app,
            Message::PrefsPickerPast(ColorPickerUpdate::Cancel),
        );
        upd(
            &mut app,
            Message::PrefsPickerHighlight(ColorPickerUpdate::ToggleColorPicker),
        );
        assert_eq!(app.settings, live);
        assert!(app.prefs.is_none());
        assert!(!app.paths.config_file().exists());
        // Close with no prefs dialog: just closes, commits nothing.
        app.dialog = Some(Dialog::Loop);
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.dialog, None);
        assert_eq!(app.settings, live);
        // Dialog switch abandons an open scratch without committing.
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        let mut next = app.settings.clone();
        next.drums_channel = 1;
        upd(&mut app, Message::PrefsScratchEdited(next));
        upd(&mut app, Message::ShowDialog(Dialog::About));
        assert!(app.prefs.is_none());
        upd(&mut app, Message::CloseDialog);
        assert_eq!(app.settings, live);
        assert!(!app.paths.config_file().exists());
    }

    #[test]
    fn view_builds_channels_page_and_prefs_dialog() {
        use cosmic::widget::color_picker::ColorPickerUpdate;

        let (mut app, _rx, _tmp) = test_app();
        // Channels page, both empty states.
        app.view = View::Channels;
        let _ = app.view();
        open_fake_song(&mut app);
        app.channels[0].level = 0.75;
        let _ = app.view();
        // Prefs overlay over the channels page, picker open + odd scratch.
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        upd(
            &mut app,
            Message::PrefsPickerFuture(ColorPickerUpdate::ToggleColorPicker),
        );
        let mut next = app.settings.clone();
        next.single_color = "notacolor".to_string();
        next.drums_channel = 99;
        upd(&mut app, Message::PrefsScratchEdited(next));
        let _ = app.view();
        // Overlay over the lyrics page too, then close.
        app.view = View::Lyrics;
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        let _ = app.view();
    }

    #[test]
    fn view_builds_piano_page_and_t13_dialogs() {
        // Every T13c composition surface builds without panic: the piano
        // page (no-song ch0 keyboard + lit tightened song keyboards) and
        // each dialog sheet over it (D3 with ports + transient status, D7,
        // D8, D6 loaded + empty, D5, D4 with link constructors).
        let (mut app, _rx, _tmp) = test_app();
        app.view = View::Piano;
        let _ = app.view();
        open_fake_song(&mut app);
        app.piano.tighten = true;
        // T14d minor b: the `current()` guard needs a confirmed output for
        // the lit-keyboard half below.
        app.midi.output_ok_once = true;
        upd(&mut app, Message::PianoPreviewOn(0, 60));
        let _ = app.view();
        // D3, populated + transient status.
        open_midi_setup(&mut app);
        upd(
            &mut app,
            Message::MidiPortsRefreshed {
                backend: "FluidSynth".to_string(),
                advanced: false,
                ports: vec![crate::core::output::MidiPort {
                    id: "128:0".to_string(),
                    label: "Munt MT-32:Standard".to_string(),
                }],
            },
        );
        upd(&mut app, Message::MidiTestNote);
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        // D7 + D8.
        upd(&mut app, Message::ShowDialog(Dialog::JumpToBar));
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        upd(&mut app, Message::ShowDialog(Dialog::Loop));
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        // D6 loaded + empty, D5, D4.
        upd(&mut app, Message::ShowFileInfo);
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        app.song = None;
        upd(&mut app, Message::ShowFileInfo);
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        upd(&mut app, Message::ShowHelp);
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        upd(&mut app, Message::ShowAbout);
        let _ = app.view();
        upd(&mut app, Message::CloseDialog);
        let _ = app.view();
    }

    // -- T14d composition: keyboard, menu, focus-on-open, T13 minors ---------

    /// Synthetic ignored-status `KeyPressed` for the `KeyEvent` arm pins
    /// (mirrors the `state::keys` test builders).
    fn key_event(
        key: cosmic::iced::keyboard::Key,
        physical: cosmic::iced::keyboard::key::Physical,
        modifiers: cosmic::iced::keyboard::Modifiers,
    ) -> cosmic::iced::keyboard::Event {
        use cosmic::iced::keyboard::{Event, Location};
        Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }
    }

    #[test]
    fn key_event_routes_transport_accelerators() {
        use cosmic::iced::keyboard::{
            Key, Modifiers,
            key::{Code, Physical},
        };
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        // Space → Play (optimistic `Playing`).
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character(" ".into()),
                Physical::Code(Code::Space),
                Modifiers::empty(),
            )),
        );
        assert_eq!(app.playback.status, Status::Playing);
        // Ctrl+P → Pause toggle.
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character("p".into()),
                Physical::Code(Code::KeyP),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.playback.status, Status::Paused);
        // Ctrl+S → Stop.
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character("s".into()),
                Physical::Code(Code::KeyS),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.playback.status, Status::Stopped);
    }

    #[test]
    fn key_event_routes_next_prev_selection() {
        use cosmic::iced::keyboard::{
            Key, Modifiers,
            key::{Code, Named, Physical},
        };
        let (mut app, _rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        app.playlist.set_current_index(0);
        // Ctrl+Right → advance + autoplay the next row.
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Named(Named::ArrowRight),
                Physical::Code(Code::ArrowRight),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.playlist.current_index(), 1);
        assert!(app.pending_autoplay);
        // Ctrl+Left → back (prev never saves — preserved through the key path).
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Named(Named::ArrowLeft),
                Physical::Code(Code::ArrowLeft),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.playlist.current_index(), 0);
    }

    #[test]
    fn key_event_routes_open_prefs_help_and_quit() {
        use cosmic::iced::keyboard::{
            Key, Modifiers,
            key::{Code, Named, Physical},
        };
        let (mut app, _rx, _tmp) = test_app();
        // Ctrl+O → the D1 portal trigger (task dropped headless — same bar
        // as `open_files_dialog_launches_without_state_change`).
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character("o".into()),
                Physical::Code(Code::KeyO),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.playlist.len(), 1);
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
        // Ctrl+, → D2.
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character(",".into()),
                Physical::Code(Code::Comma),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.dialog, Some(Dialog::Preferences));
        upd(&mut app, Message::CloseDialog);
        // F1 → D5.
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Named(Named::F1),
                Physical::Code(Code::F1),
                Modifiers::empty(),
            )),
        );
        assert_eq!(app.dialog, Some(Dialog::Help));
        upd(&mut app, Message::CloseDialog);
        // Ctrl+Q → Quit arm (headless: no window, `Task::none()` — silent).
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character("q".into()),
                Physical::Code(Code::KeyQ),
                Modifiers::CTRL,
            )),
        );
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
        assert_eq!(app.playback.status, Status::Ready);
        // …and the variant directly (the File-menu Quit path).
        upd(&mut app, Message::Quit);
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn key_event_enter_plays_selection() {
        use cosmic::iced::keyboard::{
            Key, Modifiers,
            key::{Code, Named, Physical},
        };
        let enter = || {
            Message::KeyEvent(key_event(
                Key::Named(Named::Enter),
                Physical::Code(Code::Enter),
                Modifiers::empty(),
            ))
        };
        // With a selection: select + load + autoplay (U11).
        let (mut app, _rx, _tmp) = test_app();
        app.playlist
            .set_items(vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")]);
        app.playlist.set_current_index(1);
        upd(&mut app, enter());
        assert_eq!(app.playlist.current_index(), 1);
        assert!(app.pending_autoplay);
        // Without one: silent no-op.
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, enter());
        assert!(!app.pending_autoplay);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn key_event_dialog_gate_pins_subscription_level_blocking() {
        use cosmic::iced::keyboard::{
            Key, Modifiers,
            key::{Code, Named, Physical},
        };
        // Under ANY dialog, subscription keys resolve to nothing except
        // Esc (dismiss) and Tab (traversal): base Enter/Space/accelerators
        // cannot fire silently. (The `resolve_key` half is pinned in
        // `state::keys`; this pins the composition ROUTING: the gated keys
        // reach `update` and change nothing.)
        let (mut app, _rx, _tmp) = test_app();
        open_fake_song(&mut app);
        app.playlist.set_items(vec![PathBuf::from("/m/a.mid")]);
        app.playlist.set_current_index(0);
        upd(&mut app, Message::ShowDialog(Dialog::Preferences));
        assert!(app.prefs.is_some());
        // Space (Play), Enter (EnterPlay), Ctrl+S (Stop), F1 (Help): gated.
        for event in [
            key_event(
                Key::Character(" ".into()),
                Physical::Code(Code::Space),
                Modifiers::empty(),
            ),
            key_event(
                Key::Named(Named::Enter),
                Physical::Code(Code::Enter),
                Modifiers::empty(),
            ),
            key_event(
                Key::Character("s".into()),
                Physical::Code(Code::KeyS),
                Modifiers::CTRL,
            ),
            key_event(
                Key::Named(Named::F1),
                Physical::Code(Code::F1),
                Modifiers::empty(),
            ),
        ] {
            upd(&mut app, Message::KeyEvent(event));
            assert_eq!(app.dialog, Some(Dialog::Preferences));
            assert_eq!(app.playback.status, Status::Stopped);
            assert!(!app.pending_autoplay);
        }
        // Tab / Shift+Tab: traversal tasks (opaque) — no state change.
        for modifiers in [Modifiers::empty(), Modifiers::SHIFT] {
            upd(
                &mut app,
                Message::KeyEvent(key_event(
                    Key::Named(Named::Tab),
                    Physical::Code(Code::Tab),
                    modifiers,
                )),
            );
            assert_eq!(app.dialog, Some(Dialog::Preferences));
            assert!(app.prefs.is_some());
        }
        // Esc dismisses (commit-on-close runs — the prefs shell commits).
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Named(Named::Escape),
                Physical::Code(Code::Escape),
                Modifiers::empty(),
            )),
        );
        assert_eq!(app.dialog, None);
        assert!(app.prefs.is_none());
    }

    #[test]
    fn key_event_ignores_releases_and_unbound_keys() {
        use cosmic::iced::keyboard::{
            Event, Key, Location, Modifiers,
            key::{Code, Physical},
        };
        let (mut app, _rx, _tmp) = test_app();
        // KeyReleased never resolves.
        upd(
            &mut app,
            Message::KeyEvent(Event::KeyReleased {
                key: Key::Character("o".into()),
                modified_key: Key::Character("o".into()),
                physical_key: Physical::Code(Code::KeyO),
                location: Location::Standard,
                modifiers: Modifiers::CTRL,
            }),
        );
        // Unbound chord (Ctrl+X — D22: no invented bindings) never resolves.
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Character("x".into()),
                Physical::Code(Code::KeyX),
                Modifiers::CTRL,
            )),
        );
        // Plain arrows stay unmapped (no C++ port path).
        upd(
            &mut app,
            Message::KeyEvent(key_event(
                Key::Named(cosmic::iced::keyboard::key::Named::ArrowRight),
                Physical::Code(Code::ArrowRight),
                Modifiers::empty(),
            )),
        );
        assert_eq!(app.dialog, None);
        assert_eq!(app.playback.status, Status::Ready);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn dialog_focus_id_pins_every_dialog_target() {
        // G4 partial: every sheet dialog focuses a dialog widget on open;
        // the portal-chooser dialogs have no sheet to focus.
        assert_eq!(
            App::dialog_focus_id(Dialog::Preferences),
            Some(PREFS_CLOSE_ID)
        );
        assert_eq!(
            App::dialog_focus_id(Dialog::MidiSetup),
            Some(midi_setup::APPLY_ID)
        );
        assert_eq!(
            App::dialog_focus_id(Dialog::About),
            Some(info_dialogs::ABOUT_CLOSE_ID)
        );
        assert_eq!(
            App::dialog_focus_id(Dialog::Help),
            Some(info_dialogs::OK_ID)
        );
        assert_eq!(
            App::dialog_focus_id(Dialog::FileInfo),
            Some(info_dialogs::OK_ID)
        );
        assert_eq!(
            App::dialog_focus_id(Dialog::JumpToBar),
            Some(jump_loop::JUMP_CONFIRM_ID)
        );
        assert_eq!(
            App::dialog_focus_id(Dialog::Loop),
            Some(jump_loop::LOOP_CONFIRM_ID)
        );
        assert_eq!(App::dialog_focus_id(Dialog::PlaylistOpen), None);
        assert_eq!(App::dialog_focus_id(Dialog::PlaylistSave), None);
        assert_eq!(App::dialog_focus_id(Dialog::SoundFontBrowse), None);
        assert_eq!(App::dialog_focus_id(Dialog::SaveLyrics), None);
    }

    #[test]
    fn menu_bar_places_single_root_in_header_end() {
        // T14d: the hamburger root lives at the header end (C++ `:1584,1618`).
        let (app, _rx, _tmp) = test_app();
        assert_eq!(app.header_end().len(), 1);
    }

    #[test]
    fn piano_preview_gated_without_output() {
        // T14d minor b (`window.cpp:2098-2102`): no confirmed output → the
        // key stays dark (the send would no-op engine-side anyway).
        let (mut app, _rx, _tmp) = test_app();
        assert!(!app.midi.output_ok_once);
        upd(&mut app, Message::PianoPreviewOn(0, 60));
        assert!(!app.piano.notes_on[0][60]);
        // Confirmed output → lights (the send leg is engine-opaque headless).
        app.midi.output_ok_once = true;
        upd(&mut app, Message::PianoPreviewOn(0, 60));
        assert!(app.piano.notes_on[0][60]);
        assert_eq!(app.piano.note_vel[0][60], 80);
    }

    #[test]
    fn output_epoch_bumps_once_per_select_output_send() {
        // T14d minor c accounting: every reselect path bumps exactly once;
        // non-reselect paths never bump.
        let (mut app, _rx, _tmp) = test_app();
        assert_eq!(app.output_epoch, 0);
        // Test path (unconditional reselect).
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiTestNote);
        assert_eq!(app.output_epoch, 1);
        // No-change Apply: no reselect, no bump.
        feed_ports(
            &mut app,
            vec![midi_port("fluidsynth", "FluidSynth (gm.sf2)")],
        );
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.output_epoch, 1);
        // Changed Apply: reselect, one bump.
        open_midi_setup(&mut app);
        app.midi_dlg.as_mut().unwrap().scratch.backend = "Dummy".to_string();
        feed_ports(&mut app, vec![midi_port("dummy", "Dummy (no output)")]);
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.output_epoch, 2);
        // Prefs commit carrying a D3 change: reselect, one bump.
        let mut prefs = Settings::new();
        prefs.last_output_backend = "ALSA".to_string();
        prefs.last_output_connection = "128:0".to_string();
        upd(&mut app, Message::PrefsChanged(prefs));
        assert_eq!(app.output_epoch, 3);
        // Post-failure fallback: each chain step bumps once, then the
        // exhausted chain sends nothing.
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "ALSA".to_string(),
                port: "128:0".to_string(),
                warning: Some("boom".to_string()),
            }),
        );
        assert_eq!(app.output_epoch, 4);
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "FluidSynth".to_string(),
                port: "fluidsynth".to_string(),
                warning: Some("boom".to_string()),
            }),
        );
        assert_eq!(app.output_epoch, 5);
        upd(
            &mut app,
            Message::Player(PlayerEvent::OutputChanged {
                backend: "Dummy".to_string(),
                port: "dummy".to_string(),
                warning: Some("boom".to_string()),
            }),
        );
        assert_eq!(app.output_epoch, 5, "fallback chain exhausted: no send");
    }

    #[test]
    fn test_note_off_skips_after_intervening_reselect() {
        // T14d minor c, realistic case: Test, then Apply-with-reselect
        // within 400 ms. The Apply's `Stop` covers the sounding note; the
        // stale off skips instead of misfiring at the new current (C++
        // `:1148-1153` addresses the CAPTURED output — the port skips when
        // the capture is stale, since the old output was silenced).
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        upd(&mut app, Message::MidiTestNote);
        let captured = app.output_epoch;
        open_midi_setup(&mut app);
        app.midi_dlg.as_mut().unwrap().scratch.backend = "Dummy".to_string();
        feed_ports(&mut app, vec![midi_port("dummy", "Dummy (no output)")]);
        upd(&mut app, Message::MidiApply);
        assert_ne!(app.output_epoch, captured);
        upd(
            &mut app,
            Message::MidiTestNoteOff {
                ch: 0,
                note: 60,
                epoch: captured,
            },
        );
        assert!(app.toasts.is_empty());
        // A fresh capture still routes (silent by design too — the engine
        // send itself is opaque headless, the established PreviewNote
        // precedent — so both halves share the silence bar).
        let live = app.output_epoch;
        upd(
            &mut app,
            Message::MidiTestNoteOff {
                ch: 0,
                note: 60,
                epoch: live,
            },
        );
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn midi_apply_uses_dropdown_shown_port_not_raw_want() {
        // T14d minor d: stale want (unplugged "128:0" accepted as-is) +
        // disjoint refill — C++ applies `plist[pi].id` (`:1164-1166`),
        // index 0 here (no id match, no label-substring match).
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        app.midi_dlg.as_mut().unwrap().scratch.port = "128:0".to_string();
        feed_ports(&mut app, vec![midi_port("129:0", "USB MIDI")]);
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.midi.backend, "FluidSynth");
        assert_eq!(app.midi.port, "129:0");
        assert!(app.midi.announce_applied);
    }

    #[test]
    fn midi_apply_empty_list_selects_empty_port_id() {
        // T14d minor d, mid-refill race (ports still empty): C++ `pi < size`
        // fails and `portId` stays "" (`:1164-1166`) — the port matches.
        let (mut app, _rx, _tmp) = test_app();
        open_midi_setup(&mut app);
        // (ports deliberately unfed — the race under test)
        upd(&mut app, Message::MidiApply);
        assert_eq!(app.midi.backend, "FluidSynth");
        assert_eq!(app.midi.port, "");
        assert!(app.midi.announce_applied);
    }
}
