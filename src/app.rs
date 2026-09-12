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
use crate::core::player::{EngineEventReceiver, EngineHandle, spawn_engine};
use crate::core::sequence::Sequence;
use crate::message::{
    Dialog, EngineCommand, LoadedPlaylist, LoadedSong, Message, PlayerEvent, RepeatMode,
    SettingsState, Status, View,
};
use crate::state::channels::{self, ChannelState};
use crate::state::lyrics::LyricsState;
use crate::state::midi::MidiState;
use crate::state::piano::PianoState;
use crate::state::playback::{self, PlaybackState};
use crate::state::playlist::PlaylistState;
use crate::ui::{PlaylistViewModel, TransportViewModel, sidebar, transport};

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
    view: View,
    toasts: Vec<String>,
    /// Autoplay flag for the in-flight `Load` (consumed by `Opened`).
    pending_autoplay: bool,
}

impl Drop for App {
    fn drop(&mut self) {
        // Prompt engine exit (the thread would also end on handle-drop).
        self.engine.send(EngineCommand::Shutdown);
    }
}

impl App {
    // -- startup ----------------------------------------------------------

    /// Initial output selection (ctor chain, `window.cpp:227-242`): optional
    /// SoundFont, saved-or-CLI backend/port, drums channel, SysEx reset.
    /// Recovery from a dead saved port happens in `OutputChanged` via the
    /// staged fallback chain (FluidSynth → Dummy + warning toast).
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
            self.engine.send(EngineCommand::SetSoundFont(
                self.settings.sound_font.clone(),
            ));
        }
        self.midi.backend = backend.clone();
        self.midi.port = port.clone();
        self.midi.soundfont = self.settings.sound_font.clone();
        self.midi.advanced = self.settings.advanced_ports;
        self.midi.instrument_map = self.settings.instrument_map.clamp(0, 2) as u8;
        self.midi.sysex_reset = self.settings.sysex_reset.clamp(0, 4) as u8;
        self.engine
            .send(EngineCommand::SelectOutput { backend, port });
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
        for (ch, slot) in self.channels.iter_mut().enumerate() {
            *slot = ChannelState::new();
            slot.used = song.channel_used[ch];
            slot.label = song.channel_labels[ch].clone();
        }
        self.piano.reset_for_song();
        self.lyrics.reset_for_song(song.charset.clone());
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
        let any = channels::any_solo(&self.channels);
        let row = &self.channels[idx];
        self.engine.send(EngineCommand::SetMuted(ch, row.muted));
        self.engine.send(EngineCommand::SetChannelVolume(
            ch,
            row.factor(any, self.settings.solo_volume_reduction),
        ));
    }

    /// Push the whole mix (the full `applyChannelSoloMute` loop): used after
    /// solo flips (which change every row's factor), song-settings loads,
    /// and prefs commits (solo% — U14).
    fn push_all_channels(&self) {
        for ch in 0..MIDI_CHANNELS as u8 {
            self.push_channel(ch);
        }
    }

    // -- prefs / MIDI setup apply --------------------------------------------

    /// Commit-on-close prefs apply (U14 FIX, `window.cpp:1310-1340`): adopt
    /// the dialog scratch state wholesale, push drums channel, SysEx reset,
    /// and the full mix (solo% applies at close too) to the engine, then
    /// save the full config (save-point row 3). Lyrics/piano rows apply at
    /// render time from `self.settings` (T11–T13); the instrument map
    /// relabels the patch model (T12).
    fn apply_prefs(&mut self, prefs: SettingsState) {
        self.settings = prefs;
        self.engine.send(EngineCommand::SetDrumsChannel(
            (self.settings.drums_channel.clamp(1, 16) - 1) as u8,
        ));
        let reset = self.settings.sysex_reset.clamp(0, 4) as u8;
        self.engine.send(EngineCommand::SetSysexReset(reset));
        self.midi.instrument_map = self.settings.instrument_map.clamp(0, 2) as u8;
        self.midi.sysex_reset = reset;
        self.push_all_channels();
        // C++ parity: `AppSettings::save()` is void — failures are silent.
        if self.settings.save(&self.paths).is_err() {
            // Silent, as in C++.
        }
    }

    /// D3 Apply (`window.cpp:1150-1187` + `connectOutput`, `window.cpp:328-349`):
    /// commit soundFont/advanced/map/reset, push reset + SoundFont to the
    /// engine, select the output (success toast on `OutputChanged`),
    /// persist (save-point row 2).
    fn apply_midi_setup(&mut self) {
        self.settings.sound_font = self.midi.soundfont.clone();
        self.settings.advanced_ports = self.midi.advanced;
        self.settings.instrument_map = i32::from(self.midi.instrument_map);
        self.settings.sysex_reset = i32::from(self.midi.sysex_reset);
        self.engine
            .send(EngineCommand::SetSysexReset(self.midi.sysex_reset));
        if !self.midi.soundfont.is_empty() {
            self.engine
                .send(EngineCommand::SetSoundFont(self.midi.soundfont.clone()));
        }
        self.settings.last_output_backend = self.midi.backend.clone();
        self.settings.last_output_connection = self.midi.port.clone();
        // User-initiated apply: announce the result (unlike the silent
        // startup select).
        self.midi.announce_applied = true;
        let backend = self.midi.backend.clone();
        let port = self.midi.port.clone();
        self.engine
            .send(EngineCommand::SelectOutput { backend, port });
        if self.settings.save(&self.paths).is_err() {
            // Silent, as in C++.
        }
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
            // T11 refreshes the body from `LoadedSong.texts` under the codec.
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

    // -- engine → UI --------------------------------------------------------------

    /// Engine event handling (arch §2.3): the 12 callback twins plus
    /// open/load/output results. `Time` is latest-wins coalescing (the
    /// forwarder already keeps only the newest per poll); notes/programs/
    /// text/beat/transport arrive losslessly in order.
    fn on_player_event(&mut self, event: PlayerEvent) {
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
                    *meter = meter.max(f32::from(vel) / 127.0);
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
                self.lyrics.append_live(track, text_type, &text);
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
                    self.toast(warning);
                    // Startup fallback chain only (ctor parity): once any
                    // output has been confirmed, later failures (user
                    // applies) toast without second-guessing the selection.
                    if !self.midi.output_ok_once
                        && let Some((fallback, port)) = self.midi.next_fallback()
                    {
                        self.engine.send(EngineCommand::SelectOutput {
                            backend: fallback,
                            port,
                        });
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
    }

    // -- misc ---------------------------------------------------------------------

    /// Push a toast; the stack is capped (`MAX_TOASTS`, oldest dropped).
    fn toast(&mut self, text: String) {
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(text);
    }

    /// Build the transport view model for `view()`.
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
            loop_enabled: self.playback.loop_enabled,
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
            view: View::Lyrics,
            toasts: Vec::new(),
            pending_autoplay: false,
        };
        app.core.set_header_title("Gosh MIDI Player".to_owned());
        app.playback.repeat = RepeatMode::from_i32(app.settings.repeat_mode);
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

    // No `footer()` override (T9 status-dup resolved at T10a2 composition):
    // the transport region already renders the status line (ux §3.9), so the
    // COSMIC footer stays at its `None` default — exactly one status line.

    fn subscription(&self) -> Subscription<Self::Message> {
        let engine = Subscription::run_with(self.engine_sub, run_engine_stream);
        let resize = cosmic::iced::event::listen_with(window_event_message);
        // Intended deviation from C++ (`m_meterTimer` is always on): the
        // 50 ms meter-decay ticker runs only while Playing. Visual-only
        // difference — paused/stopped meters freeze instead of decaying —
        // and idle CPU stays at zero.
        if self.playback.status == Status::Playing {
            Subscription::batch([
                engine,
                cosmic::iced::time::every(TICK_INTERVAL).map(Message::Tick),
                resize,
            ])
        } else {
            Subscription::batch([engine, resize])
        }
    }

    /// State transitions + engine commands (arch §2.3). Transport/mix/seek/
    /// loop messages update state optimistically and send the matching
    /// command; D1/D9/D10 launch portal chooser `Task`s (lyrics/T11 and MIDI
    /// dialog/T13 still return `Task::none()`) — every arm is covered by a
    /// test below.
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
                // the C++ sequence across the thread boundary.
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
                    // D8: toggle ON opens the range dialog (rendered T13);
                    // the engine command goes out on `LoopSet` confirm.
                    // Intended deviation from C++ (`showLoop` early-returns
                    // with no song loaded, `window.cpp:832-834`): the dialog
                    // opens unconditionally instead of a silently dead
                    // toggle. Deliberate; the D8 cancel rule still applies.
                    self.dialog = Some(Dialog::Loop);
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
            Message::LyricTrackFilter(track) => self.lyrics.track_filter = track,
            Message::LyricTypeFilter(typ) => self.lyrics.type_filter = typ,
            Message::LyricCodec(codec) => self.lyrics.codec = codec,
            // T11: clipboard write / portal save+export / print routing.
            Message::LyricsCopy | Message::LyricsSaveDialog | Message::LyricsPrint => {}
            Message::LyricsSaved(result) => {
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
                // A3: audition lights the key at vel 80; the audible NoteOn
                // goes straight to the outputs in T13, bypassing mute + pitch.
                self.piano.note_on(ch, note, 80);
            }
            Message::PianoPreviewOff => {
                // A3 idle-only note-off (`window.cpp:2104-2112`): when idle
                // no song notes can be lit (stop/load clear the matrix), so
                // clearing matches C++; while playing, releases must not
                // kill song notes.
                if self.playback.status != Status::Playing {
                    self.piano.clear();
                }
            }

            // --- MIDI setup ---
            Message::MidiBackendSelected(backend) => {
                self.midi.backend = backend;
                // Port list belongs to the old backend (refill is T13).
                self.midi.ports.clear();
            }
            Message::MidiPortsRefreshed(ports) => self.midi.ports = ports,
            Message::MidiPortSelected(port) => self.midi.port = port,
            Message::MidiAdvancedToggled(advanced) => self.midi.advanced = advanced,
            Message::MidiInstrumentMapChanged(map) => self.midi.instrument_map = map.min(2),
            Message::MidiResetChanged(reset) => self.midi.sysex_reset = reset.min(4),
            Message::MidiSoundFontChanged(path) => self.midi.soundfont = path,
            // T13: portal chooser `Task` / middle-C test note.
            Message::MidiSoundFontBrowse | Message::MidiTestNote => {}
            Message::MidiApply => self.apply_midi_setup(),

            // --- preferences / song settings ---
            Message::PrefsChanged(prefs) => self.apply_prefs(prefs),
            Message::SongSettingsSave => self.save_song_settings(),
            Message::SongSettingsLoad => self.load_song_settings(),

            // --- dialogs / navigation ---
            Message::ShowDialog(dialog) => self.dialog = Some(dialog),
            // D8 cancel rule falls out of the model: the loop toggle derives
            // from `loop_enabled`, so closing an unconfirmed dialog resets it.
            Message::CloseDialog => self.dialog = None,
            Message::ShowFileInfo => self.dialog = Some(Dialog::FileInfo),
            Message::ShowAbout => self.dialog = Some(Dialog::About),
            Message::ShowHelp => self.dialog = Some(Dialog::Help),
            Message::ViewSelected(view) => self.view = view,
            Message::Toast(text) => self.toast(text),
            Message::DismissToast => {
                if !self.toasts.is_empty() {
                    self.toasts.remove(0);
                }
            }

            // --- engine → UI / async ---
            Message::Player(event) => self.on_player_event(event),
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
    /// column (transport region with THE status line, view-switcher
    /// placeholder, toasts). The sidebar honors `playlistVisible`
    /// (`settings.playlist_visible`, default true); there is no runtime
    /// collapse toggle yet — any toggle needs a new `Message` + update arm
    /// (arch decision, recorded gap), so a hidden sidebar reappears only via
    /// the setting. The whole layout stays wrapped in the Wayland drop
    /// target (`dnd_destination`, arch §2.3, same mimes + callbacks); X11
    /// drops arrive via the `FileDropped` subscription instead.
    fn view(&self) -> Element<'_, Self::Message> {
        let mut main: Vec<Element<'_, Self::Message>> = vec![
            transport::view(self.transport_view_model()),
            cosmic::widget::text::body(format!(
                "{:?} view — switcher lands in T11+ (dialog={:?})",
                self.view, self.dialog,
            ))
            .into(),
        ];
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
        cosmic::widget::dnd_destination(content, vec![DND_URI_LIST_MIME.into()])
            .on_data_received(|mime, data| Message::DndDataReceived { mime, data })
            .on_file_transfer(Message::DndFileTransfer)
            .into()
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
            view: View::Lyrics,
            toasts: Vec::new(),
            pending_autoplay: false,
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
    fn loop_set_enables_range_and_closes() {
        let (mut app, _rx, _tmp) = test_app();
        app.dialog = Some(Dialog::Loop);
        upd(&mut app, Message::LoopSet { start: 3, end: 5 });
        assert!(app.playback.loop_enabled);
        assert_eq!((app.playback.loop_start, app.playback.loop_end), (3, 5));
        assert_eq!(app.dialog, None);
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

    // -- lyrics -----------------------------------------------------------------

    #[test]
    fn lyric_filters_and_codec_stored() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::LyricTrackFilter(Some(3)));
        assert_eq!(app.lyrics.track_filter, Some(3));
        upd(&mut app, Message::LyricTypeFilter(1));
        assert_eq!(app.lyrics.type_filter, 1);
        upd(&mut app, Message::LyricCodec("SHIFT_JIS".to_string()));
        assert_eq!(app.lyrics.codec, "SHIFT_JIS");
    }

    #[test]
    fn lyrics_actions_noop_and_saved_error_toasts() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::LyricsCopy);
        upd(&mut app, Message::LyricsSaveDialog);
        upd(&mut app, Message::LyricsPrint);
        assert!(app.toasts.is_empty());
        upd(
            &mut app,
            Message::LyricsSaved(Ok(PathBuf::from("/t/x.txt"))),
        );
        assert!(app.toasts.is_empty());
        upd(&mut app, Message::LyricsSaved(Err("ro".to_string())));
        assert_eq!(app.toasts, vec!["ro".to_string()]);
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

    #[test]
    fn midi_selections_stored_and_clamped() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::MidiBackendSelected("ALSA".to_string()));
        assert_eq!(app.midi.backend, "ALSA");
        upd(
            &mut app,
            Message::MidiPortsRefreshed(vec![crate::core::output::MidiPort {
                id: "128:0".to_string(),
                label: "Synth".to_string(),
            }]),
        );
        assert_eq!(app.midi.ports.len(), 1);
        // Switching backend drops the stale port list.
        upd(
            &mut app,
            Message::MidiBackendSelected("FluidSynth".to_string()),
        );
        assert!(app.midi.ports.is_empty());
        upd(&mut app, Message::MidiPortSelected("128:0".to_string()));
        assert_eq!(app.midi.port, "128:0");
        upd(&mut app, Message::MidiAdvancedToggled(true));
        assert!(app.midi.advanced);
        upd(&mut app, Message::MidiInstrumentMapChanged(9));
        assert_eq!(app.midi.instrument_map, 2);
        upd(&mut app, Message::MidiResetChanged(9));
        assert_eq!(app.midi.sysex_reset, 4);
        upd(
            &mut app,
            Message::MidiSoundFontChanged("/sf/gm.sf2".to_string()),
        );
        assert_eq!(app.midi.soundfont, "/sf/gm.sf2");
    }

    #[test]
    fn midi_dialog_actions_are_staged_noops() {
        let (mut app, _rx, _tmp) = test_app();
        upd(&mut app, Message::MidiSoundFontBrowse);
        upd(&mut app, Message::MidiTestNote);
        assert_eq!(app.dialog, None);
        assert!(app.toasts.is_empty());
    }

    #[test]
    fn midi_apply_persists_and_announces_e2e() {
        let (mut app, rx, _tmp) = test_app();
        app.midi.backend = "Dummy".to_string();
        app.midi.port = "dummy".to_string();
        app.midi.soundfont = "/sf/gm.sf2".to_string();
        upd(&mut app, Message::MidiApply);
        // Persisted (save-point row 2).
        let disk = std::fs::read_to_string(app.paths.config_file()).unwrap();
        assert!(disk.contains("lastOutputBackend=Dummy\n"));
        assert!(disk.contains("soundFont=/sf/gm.sf2\n"));
        // The engine answers; the user-initiated apply announces.
        pump_until(&mut app, &rx, Duration::from_secs(5), |ev| {
            matches!(ev, PlayerEvent::OutputChanged { .. })
        });
        assert!(app.midi.output_ok_once);
        assert_eq!(app.toasts, vec!["MIDI output: Dummy dummy".to_string()]);
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
        app.lyrics.live_tail = "stale".to_string();
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
        assert!(app.lyrics.live_tail.is_empty());
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
        assert_eq!(app.lyrics.live_tail, "Hello ✓");
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
        assert_eq!(app.lyrics.live_tail, "Hello ✓");
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
}
