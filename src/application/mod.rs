//! Toolkit-independent application controller. Only this layer decides behavior.
use crate::{
    commands::{Action, Command, Preference},
    config::{
        paths::Paths,
        settings::Settings,
        song::{SongChannelSettings, SongSettings},
    },
    core::{
        output::MidiOutputManager,
        player::{
            EngineCommand as E, EngineEventReceiver, EngineHandle, PlayerEvent, SongSnapshot,
            spawn_engine,
        },
    },
    models::{Dialog, RepeatMode, Status, Theme, View},
    state::{
        channels::{self, ChannelState},
        lyrics::LyricsState,
        midi::MidiState,
        piano::PianoState,
        playback::{self, PlaybackState},
        playlist::PlaylistState,
    },
};
use std::{path::PathBuf, thread::JoinHandle};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: Theme,
    pub maximized: bool,
    pub position: Option<(i32, i32)>,
}

#[derive(Clone, Debug)]
pub enum Effect {
    Choose(Action, PathBuf),
    WriteSettings(Paths, Box<Settings>, Appearance),
    WriteSong(PathBuf, Box<SongSettings>),
    ReadSong(PathBuf, Vec<PathBuf>, bool),
    Ports(String, bool),
    Clipboard(String),
    SaveLyrics(String),
    Print,
    Url(String),
    Reveal(PathBuf),
    ReleaseAfter(u64, u64),
    Close,
}

pub struct Controller {
    pub paths: Paths,
    pub settings: Settings,
    pub appearance: Appearance,
    pub playlist: PlaylistState,
    pub playback: PlaybackState,
    pub channels: [ChannelState; 16],
    pub lyrics: LyricsState,
    pub piano: PianoState,
    pub midi: MidiState,
    pub song: Option<SongSnapshot>,
    pub loading: Option<PathBuf>,
    pub view: View,
    pub dialog: Option<Dialog>,
    pub prefs: Option<Settings>,
    pub midi_draft: Option<MidiState>,
    pub errors: Vec<String>,
    pub notice: String,
    handle: EngineHandle,
    receiver: EngineEventReceiver,
    worker: Option<JoinHandle<()>>,
    pending: Option<Box<Command>>,
    play_after_load: bool,
    previews: Vec<(u8, u8)>,
    preview_generation: u64,
}

impl Controller {
    pub fn action_enabled(&self, action: Action) -> bool {
        if self.dialog.is_some() {
            return false;
        }
        match action {
            Action::Play
            | Action::Pause
            | Action::Stop
            | Action::Rewind
            | Action::Forward
            | Action::Jump
            | Action::Loop
            | Action::FileInfo
            | Action::LoadSettings
            | Action::SaveSettings
            | Action::Search
            | Action::Reveal => self.song.is_some(),
            Action::Next => !self.playlist.is_empty() && !self.playlist.at_last(),
            Action::Previous => !self.playlist.is_empty() && !self.playlist.at_first(),
            Action::Clear => !self.playlist.is_empty(),
            Action::Shuffle => self.playlist.len() > 1,
            _ => true,
        }
    }
    pub fn new(
        paths: Paths,
        files: Vec<PathBuf>,
        output: Option<(String, String)>,
    ) -> (Self, Vec<Effect>) {
        let mut errors = Vec::new();
        let mut settings = Settings::new();
        if paths.config_file().exists()
            && let Err(error) = settings.load_file(paths.config_file())
        {
            errors.push(format!("Could not read settings: {error}"));
        }
        let appearance_path = paths.config_dir().join("desktop.json");
        let appearance = if appearance_path.exists() {
            match std::fs::read(&appearance_path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
            {
                Ok(value) => value,
                Err(error) => {
                    errors.push(format!("Could not read appearance: {error}"));
                    Appearance::default()
                }
            }
        } else {
            Appearance::default()
        };
        let mut midi = MidiState::new();
        midi.backend = settings.last_output_backend.clone();
        midi.port = settings.last_output_connection.clone();
        if let Some((backend, port)) = output {
            midi.backend = backend;
            midi.port = port;
        }
        midi.advanced = settings.advanced_ports;
        midi.instrument_map = settings.instrument_map.clamp(0, 2) as u8;
        midi.sysex_reset = settings.sysex_reset.clamp(0, 4) as u8;
        midi.soundfont = settings.sound_font.clone();
        let (handle, receiver, worker) = spawn_engine();
        let mut playlist = PlaylistState::new();
        if !settings.last_play_list.is_empty()
            && !playlist.load(&PathBuf::from(&settings.last_play_list))
        {
            errors.push(format!(
                "Could not restore playlist: {}",
                settings.last_play_list
            ));
        }
        let mut app = Self {
            paths,
            settings,
            appearance,
            playlist,
            playback: PlaybackState::new(),
            channels: channels::default_mix(),
            lyrics: LyricsState::new(),
            piano: PianoState::new(),
            midi,
            song: None,
            loading: None,
            view: View::Lyrics,
            dialog: None,
            prefs: None,
            midi_draft: None,
            errors,
            notice: String::new(),
            handle,
            receiver,
            worker: Some(worker),
            pending: None,
            play_after_load: false,
            previews: Vec::new(),
            preview_generation: 0,
        };
        app.playback.repeat = RepeatMode::from_i32(app.settings.repeat_mode);
        app.send(E::SetDrumsChannel(
            (app.settings.drums_channel.clamp(1, 16) - 1) as u8,
        ));
        app.send(E::SetSysexReset(app.midi.sysex_reset));
        app.send(E::SetSoundFont(app.midi.soundfont.clone()));
        app.send(E::SelectOutput {
            backend: app.midi.backend.clone(),
            port: app.midi.port.clone(),
        });
        let effects = if files.is_empty() {
            app.load_current(app.settings.auto_play)
        } else {
            app.dispatch(Command::OpenFiles(files, false))
        };
        (app, effects)
    }
    fn send(&self, command: E) {
        self.handle.send(command);
    }
    pub fn take_events(&self) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        for _ in 0..4096 {
            match self.receiver.try_recv_lossless() {
                Some(event) => events.push(event),
                None => break,
            }
        }
        if let Some(event) = self.receiver.drain_time_latest() {
            events.push(event);
        }
        events
    }
    pub fn active(&self) -> bool {
        self.playback.status == Status::Playing
            || self.channels.iter().any(|c| c.level > 0.01)
            || self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
    }
    pub fn poll(&mut self, events: Vec<PlayerEvent>) -> Vec<Effect> {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            self.playback.status = Status::Stopped;
            self.loading = None;
            self.errors
                .push("The playback worker stopped unexpectedly. Restart the application.".into());
        }
        let mut effects = Vec::new();
        for event in events {
            effects.extend(self.event(event));
        }
        channels::decay_meters(&mut self.channels);
        effects
    }
    fn persist(&self) -> Effect {
        Effect::WriteSettings(
            self.paths.clone(),
            Box::new(self.settings.clone()),
            self.appearance.clone(),
        )
    }
    fn refresh_lyrics(&mut self) {
        if let Some(song) = &self.song {
            self.lyrics.refresh_body(&song.texts);
            let kar = song
                .path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("kar"));
            self.lyrics.body = song
                .texts
                .iter()
                .filter(|record| {
                    self.lyrics
                        .passes_filter(record.track.max(0) as u32, record.text_type as i32)
                })
                .map(|record| crate::state::lyrics::display_record(record, &self.lyrics.codec, kar))
                .collect();
        }
    }
    fn sync_mix(&self) {
        for (ch, values) in channels::sync_all(
            &self.channels,
            self.settings.solo_volume_reduction.clamp(0, 100),
        )
        .into_iter()
        .enumerate()
        {
            self.send(E::SetMuted(ch as u8, values.muted));
            self.send(E::SetChannelVolume(ch as u8, values.factor));
        }
    }
    fn release(&mut self) {
        self.preview_generation = self.preview_generation.wrapping_add(1);
        let notes = std::mem::take(&mut self.previews);
        for &(ch, note) in &notes {
            self.piano.note_off(ch, note);
        }
        self.send(E::PreviewOff { notes });
    }
    fn capture_song(&self) -> Option<SongSettings> {
        let song = self.song.as_ref()?;
        let mut saved = SongSettings::load(&self.paths.song_settings_for_file(&song.path))
            .or_else(|| {
                SongSettings::load(
                    &self
                        .paths
                        .legacy_song_settings_path(&song.path.to_string_lossy()),
                )
                .filter(|saved| {
                    saved.file.is_empty() || std::path::Path::new(&saved.file) == song.path
                })
            })
            .unwrap_or_else(SongSettings::empty);
        saved.file = song.path.to_string_lossy().into_owned();
        saved.encoding = Some(self.lyrics.codec.clone());
        saved.volume = Some(self.playback.volume_pct as i32);
        saved.pitch = self.playback.pitch_semitones;
        saved.timeskew = Some(self.playback.tempo_factor_pct as i32);
        for (index, ch) in self.channels.iter().enumerate() {
            if ch.used {
                saved.channels[index] = Some(SongChannelSettings {
                    name: Some(ch.label.clone()),
                    muted: ch.muted,
                    solo: ch.soloed,
                    locked: ch.locked,
                    patch: i32::from(ch.patch),
                    level: Some(ch.level_pct as i32),
                });
            }
        }
        Some(saved)
    }
    fn save_song(&self) -> Vec<Effect> {
        self.capture_song()
            .map(|song| {
                vec![Effect::WriteSong(
                    self.paths
                        .song_settings_for_file(&self.song.as_ref().unwrap().path),
                    Box::new(song),
                )]
            })
            .unwrap_or_default()
    }
    fn load_current(&mut self, play: bool) -> Vec<Effect> {
        self.release();
        self.send(E::Stop { reset: true });
        self.song = None;
        self.playback.status = Status::Ready;
        self.playback.position_ticks = 0;
        self.playback.song_len_ticks = 0;
        self.playback.elapsed_ms = 0;
        self.playback.loop_enabled = false;
        self.piano.reset_for_song();
        self.lyrics = LyricsState::new();
        self.channels = channels::default_mix();
        self.loading = self.playlist.current().map(|p| p.to_path_buf());
        self.play_after_load = play;
        if let Some(path) = &self.loading {
            self.send(E::Load(path.clone()));
        }
        Vec::new()
    }
    fn guard(&mut self, command: &Command) -> bool {
        let replacing = matches!(
            command,
            Command::OpenFiles(_, false)
                | Command::PlaylistLoaded(_, _, false)
                | Command::Quit
                | Command::Action(Action::Clear)
        );
        if replacing && self.playlist.dirty() && !self.playlist.items().is_empty() {
            self.pending = Some(Box::new(command.clone()));
            self.dialog = Some(Dialog::Unsaved);
            true
        } else {
            false
        }
    }
    pub fn dispatch(&mut self, command: Command) -> Vec<Effect> {
        if self.guard(&command) {
            return Vec::new();
        }
        let mut effects = Vec::new();
        match command {
            Command::Action(action) => return self.action(action),
            Command::OpenFiles(files, append) => {
                let supported: Vec<_> = files
                    .into_iter()
                    .filter(|path| {
                        if crate::core::event::is_supported_midi_file(path) {
                            true
                        } else {
                            self.errors
                                .push(format!("Unsupported file: {}", path.display()));
                            false
                        }
                    })
                    .collect();
                if supported.is_empty() {
                    return effects;
                }
                if !append {
                    if self.settings.auto_song_settings {
                        effects.extend(self.save_song());
                    }
                    self.playlist.clear();
                }
                for path in supported {
                    self.settings.add_recent(&path.to_string_lossy());
                    if let Some(parent) = path.parent() {
                        self.settings.last_directory = parent.to_string_lossy().into_owned();
                    }
                    self.playlist.add(&path);
                }
                if !append || self.song.is_none() {
                    effects.extend(self.load_current(self.settings.auto_play));
                }
                effects.push(self.persist());
            }
            Command::Select(index, play) => {
                if index < self.playlist.len() {
                    if self.settings.auto_song_settings {
                        effects.extend(self.save_song());
                    }
                    self.playlist.set_current_index(index as i32);
                    effects.extend(self.load_current(play));
                }
            }
            Command::Remove(index) => {
                let active = self.playlist.current_index() == index as i32;
                if active && self.settings.auto_song_settings {
                    effects.extend(self.save_song());
                }
                self.playlist.remove_at(index as i32);
                if active {
                    effects.extend(self.load_current(false));
                }
            }
            Command::Move(index, direction) => {
                if direction < 0 {
                    self.playlist.move_up(index as i32);
                } else {
                    self.playlist.move_down(index as i32);
                }
            }
            Command::PlaylistLoaded(path, items, append) => {
                if append {
                    for item in items {
                        self.playlist.add(&item);
                    }
                } else {
                    if self.settings.auto_song_settings {
                        effects.extend(self.save_song());
                    }
                    self.playlist.set_items(items);
                    self.playlist.set_source(Some(path.clone()));
                    self.playlist.set_dirty(false);
                    self.settings.last_play_list = path.to_string_lossy().into_owned();
                    effects.extend(self.load_current(self.settings.auto_play));
                    effects.push(self.persist());
                }
            }
            Command::PlaylistSaved(path) => {
                self.playlist.set_source(Some(path.clone()));
                self.playlist.set_dirty(false);
                self.settings.last_play_list = path.to_string_lossy().into_owned();
                self.notice = "Playlist saved".into();
                effects.push(self.persist());
                if let Some(pending) = self.pending.take() {
                    self.dialog = None;
                    effects.extend(self.dispatch(*pending));
                }
            }
            Command::Tempo(pct) => {
                self.playback.set_tempo_pct(pct);
                self.playback.bpm = self.playback.live_bpm();
                self.send(E::SetTempoFactor(self.playback.tempo_factor()));
            }
            Command::Volume(pct) => {
                let value = self.playback.set_volume_pct(pct);
                self.send(E::SetVolumeFactor(value));
            }
            Command::Pitch(pitch) => {
                self.release();
                let value = self.playback.set_pitch(pitch);
                self.piano.clear();
                self.send(E::SetPitchShift(value));
            }
            Command::Seek(fraction) => {
                if fraction.is_finite() {
                    self.release();
                    self.piano.clear();
                    self.playback.position_ticks =
                        (fraction.clamp(0., 1.) * self.playback.song_len_ticks as f64) as i64;
                    self.send(E::SeekTicks(self.playback.position_ticks));
                    self.lyrics.clear_live();
                }
            }
            Command::Jump(bar) => {
                let bar = bar.clamp(
                    1,
                    self.song
                        .as_ref()
                        .map(|s| s.bars.max(1) as u32)
                        .unwrap_or(1),
                );
                self.release();
                self.piano.clear();
                self.lyrics.clear_live();
                self.send(E::JumpToBar(bar));
                self.dialog = None;
            }
            Command::Loop(start, end) => {
                let max = self
                    .song
                    .as_ref()
                    .map(|s| s.bars.max(1) as u32)
                    .unwrap_or(1);
                if start > end || start == 0 || end > max {
                    self.errors
                        .push(format!("Loop must satisfy 1 ≤ start ≤ end ≤ {max}"));
                } else {
                    self.playback.loop_enabled = true;
                    self.playback.loop_start = start;
                    self.playback.loop_end = end;
                    self.send(E::SetLoop {
                        enabled: true,
                        start,
                        end,
                    });
                    self.dialog = None;
                }
            }
            Command::LoopOff => {
                self.playback.loop_enabled = false;
                self.send(E::SetLoop {
                    enabled: false,
                    start: self.playback.loop_start,
                    end: self.playback.loop_end,
                });
            }
            Command::Repeat(mode) => {
                self.playback.repeat = mode;
                self.settings.repeat_mode = mode.as_i32();
                effects.push(self.persist());
            }
            Command::View(view) => self.view = view,
            Command::ChannelMute(ch, value) => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.muted = value;
                    self.sync_mix();
                }
            }
            Command::ChannelSolo(ch, value) => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.soloed = value;
                    self.sync_mix();
                }
            }
            Command::ChannelLock(ch, value) => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.locked = value;
                    self.send(E::SetLocked(ch, value));
                }
            }
            Command::ChannelPatch(ch, value) => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.patch = value.min(127);
                    self.send(E::SetPatch(ch, value.min(127)));
                }
            }
            Command::ChannelLevel(ch, value) => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.set_level_pct(value);
                    self.sync_mix();
                }
            }
            Command::ChannelName(ch, value) => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.label = value;
                }
            }
            Command::LyricTrack(value) => {
                self.lyrics.track_filter = value;
                self.refresh_lyrics();
            }
            Command::LyricType(value) => {
                self.lyrics.type_filter = value.clamp(0, 13);
                self.refresh_lyrics();
            }
            Command::LyricCodec(value) => {
                self.lyrics.codec = value;
                self.refresh_lyrics();
            }
            Command::Autoscroll(value) => self.lyrics.autoscroll = value,
            Command::Tighten(value) => self.piano.tighten = value,
            Command::PianoShowAll => self.piano.show_all(),
            Command::PianoHideUnused => self
                .piano
                .hide_unused(&self.channels.clone().map(|ch| ch.used)),
            Command::Preview(ch, note) => {
                if ch < 16 && note < 128 {
                    self.release();
                    self.previews.push((ch, note));
                    self.piano.note_on(ch, note, 96);
                    self.send(E::PreviewNote { ch, note, vel: 96 });
                }
            }
            Command::PreviewOff => self.release(),
            Command::PreviewExpired(generation) => {
                if generation == self.preview_generation {
                    self.release();
                }
            }
            Command::Dialog(dialog) => {
                self.release();
                self.dialog = Some(dialog);
                self.pending = None;
                if dialog == Dialog::Preferences {
                    self.prefs = Some(self.settings.clone());
                }
                if dialog == Dialog::MidiSetup {
                    self.midi_draft = Some(clone_midi(&self.midi));
                    effects.push(Effect::Ports(self.midi.backend.clone(), self.midi.advanced));
                }
            }
            Command::CancelDialog => {
                self.release();
                self.dialog = None;
                self.prefs = None;
                self.midi_draft = None;
                self.pending = None;
            }
            Command::EditPreference(field, value) => {
                if let Some(prefs) = &mut self.prefs {
                    edit_pref(prefs, field, &value);
                }
            }
            Command::ApplyPreferences => {
                if let Some(prefs) = &self.prefs {
                    for (label, color) in [
                        ("Future text", &prefs.future_color),
                        ("Past text", &prefs.past_color),
                        ("Current text", &prefs.highlight_color),
                        ("Note highlight", &prefs.single_color),
                    ] {
                        if crate::state::lyrics::parse_gdk_rgba(color).is_none() {
                            self.errors.push(format!(
                                "{label} color is invalid. Use a CSS color such as #5277aa."
                            ));
                            return effects;
                        }
                    }
                }
                if let Some(prefs) = self.prefs.take() {
                    self.settings.commit_prefs_dialog(&prefs);
                    self.send(E::SetDrumsChannel(
                        (self.settings.drums_channel.clamp(1, 16) - 1) as u8,
                    ));
                    self.send(E::SetSysexReset(self.settings.sysex_reset.clamp(0, 4) as u8));
                    self.midi.instrument_map = self.settings.instrument_map.clamp(0, 2) as u8;
                    self.sync_mix();
                    effects.push(self.persist());
                }
                self.dialog = None;
            }
            Command::MidiBackend(value) => {
                if let Some(draft) = &mut self.midi_draft {
                    draft.backend = value.clone();
                    draft.ports.clear();
                    draft.port.clear();
                    effects.push(Effect::Ports(value, draft.advanced));
                }
            }
            Command::MidiPort(value) => {
                if let Some(draft) = &mut self.midi_draft {
                    draft.port = value;
                    if let Some(port) = draft.ports.iter().find(|p| p.id == draft.port) {
                        (draft.instrument_map, draft.sysex_reset) =
                            crate::state::midi::profile_for_port(&port.label);
                    }
                }
            }
            Command::MidiAdvanced(value) => {
                if let Some(draft) = &mut self.midi_draft {
                    draft.advanced = value;
                    effects.push(Effect::Ports(draft.backend.clone(), value));
                }
            }
            Command::MidiMap(value) => {
                if let Some(draft) = &mut self.midi_draft {
                    draft.instrument_map = value.min(2);
                }
            }
            Command::MidiReset(value) => {
                if let Some(draft) = &mut self.midi_draft {
                    draft.sysex_reset = value.min(4);
                }
            }
            Command::MidiSoundfont(value) => {
                if let Some(draft) = &mut self.midi_draft {
                    draft.soundfont = value;
                }
            }
            Command::MidiPorts {
                backend,
                advanced,
                ports,
            } => {
                if let Some(draft) = &mut self.midi_draft
                    && draft.backend == backend
                    && draft.advanced == advanced
                {
                    if !ports.iter().any(|p| p.id == draft.port) {
                        draft.port = ports.first().map(|p| p.id.clone()).unwrap_or_default();
                    }
                    draft.ports = ports;
                }
            }
            Command::MidiApply => {
                if self.apply_midi() {
                    self.dialog = None;
                    effects.push(self.persist());
                }
            }
            Command::SongSettingsLoaded(path, saved) => {
                if self.song.as_ref().is_some_and(|song| song.path == path) {
                    self.apply_song(*saved);
                    if self.play_after_load {
                        self.send(E::Play);
                        self.play_after_load = false;
                    }
                }
            }
            Command::SongSettingsAbsent(path) => {
                if self.song.as_ref().is_some_and(|song| song.path == path) && self.play_after_load
                {
                    self.send(E::Play);
                    self.play_after_load = false;
                }
            }
            Command::WindowPosition(x, y) => self.appearance.position = Some((x, y)),
            Command::Theme(value) => {
                self.appearance.theme = value;
                effects.push(self.persist());
            }
            Command::Geometry(width, height, maximized) => {
                if !maximized {
                    self.settings.window_width = width.clamp(640, 7680) as i32;
                    self.settings.window_height = height.clamp(480, 4320) as i32;
                }
                self.appearance.maximized = maximized;
            }
            Command::ToggleSidebar => {
                self.settings.playlist_visible = !self.settings.playlist_visible;
                effects.push(self.persist());
            }
            Command::DismissError => {
                if !self.errors.is_empty() {
                    self.errors.remove(0);
                }
            }
            Command::Error(message) => self.errors.push(message),
            Command::QuitSaveFailed(message) => {
                self.errors.push(message);
                self.dialog = Some(Dialog::SaveFailure);
            }
            Command::QuitWithoutSaving => {
                if self.dialog == Some(Dialog::SaveFailure) {
                    self.dialog = None;
                    effects.push(Effect::Close);
                }
            }
            Command::SaveCompleted(message) => self.notice = message,
            Command::OpenUrl(url) => {
                if url::Url::parse(&url)
                    .is_ok_and(|parsed| matches!(parsed.scheme(), "https" | "http"))
                {
                    effects.push(Effect::Url(url));
                } else {
                    self.errors
                        .push("Only HTTP and HTTPS links can be opened".into());
                }
            }
            Command::ConfirmDiscard => {
                self.playlist.set_dirty(false);
                self.dialog = None;
                if let Some(pending) = self.pending.take() {
                    effects.extend(self.dispatch(*pending));
                }
            }
            Command::Quit => {
                self.release();
                if self.settings.auto_song_settings {
                    effects.extend(self.save_song());
                }
                effects.push(self.persist());
                effects.push(Effect::Close);
            }
        }
        effects
    }
    fn apply_midi(&mut self) -> bool {
        let Some(draft) = self.midi_draft.as_ref().map(clone_midi) else {
            return false;
        };
        if draft.port.is_empty() {
            self.errors
                .push("Choose an available MIDI destination".into());
            return false;
        }
        if draft.backend == "FluidSynth"
            && !draft.soundfont.is_empty()
            && !PathBuf::from(&draft.soundfont).is_file()
        {
            self.errors.push("SoundFont file does not exist".into());
            return false;
        }
        self.release();
        self.send(E::Stop { reset: false });
        self.playback.status = Status::Stopped;
        self.midi = draft;
        self.midi.output_ok_once = true;
        self.send(E::SetSoundFont(self.midi.soundfont.clone()));
        self.send(E::SetSysexReset(self.midi.sysex_reset));
        self.send(E::SelectOutput {
            backend: self.midi.backend.clone(),
            port: self.midi.port.clone(),
        });
        self.settings.last_output_backend = self.midi.backend.clone();
        self.settings.last_output_connection = self.midi.port.clone();
        self.settings.sound_font = self.midi.soundfont.clone();
        self.settings.advanced_ports = self.midi.advanced;
        self.settings.instrument_map = i32::from(self.midi.instrument_map);
        self.settings.sysex_reset = i32::from(self.midi.sysex_reset);
        true
    }
    fn apply_song(&mut self, saved: SongSettings) {
        if let Some(encoding) = saved.encoding {
            self.lyrics.codec = encoding;
            self.refresh_lyrics();
        }
        if let Some(volume) = saved.volume {
            self.dispatch(Command::Volume(volume.max(0) as u32));
        }
        self.dispatch(Command::Pitch(saved.pitch));
        if let Some(tempo) = saved.timeskew {
            self.dispatch(Command::Tempo(tempo.max(0) as u32));
        }
        for (ch, saved) in saved.channels.into_iter().enumerate() {
            if let Some(saved) = saved {
                let row = &mut self.channels[ch];
                if let Some(name) = saved.name {
                    row.label = name;
                }
                row.muted = saved.muted;
                row.soloed = saved.solo;
                row.locked = saved.locked;
                row.patch = saved.patch.clamp(0, 127) as u8;
                if let Some(level) = saved.level {
                    row.set_level_pct(level.max(0) as u32);
                }
                self.send(E::SetPatch(ch as u8, self.channels[ch].patch));
                self.send(E::SetLocked(ch as u8, self.channels[ch].locked));
            }
        }
        self.sync_mix();
    }
    fn action(&mut self, action: Action) -> Vec<Effect> {
        use Action::*;
        match action {
            Open | Add | OpenPlaylist | SavePlaylist | BrowseSoundfont => vec![Effect::Choose(
                action,
                PathBuf::from(&self.settings.last_directory),
            )],
            FileInfo | MidiSetup | Preferences | Help | About | Jump | Loop => {
                self.dispatch(Command::Dialog(match action {
                    FileInfo => Dialog::FileInfo,
                    MidiSetup => Dialog::MidiSetup,
                    Preferences => Dialog::Preferences,
                    Help => Dialog::Help,
                    About => Dialog::About,
                    Jump => Dialog::Jump,
                    _ => Dialog::Loop,
                }))
            }
            Quit => self.dispatch(Command::Quit),
            Play => {
                if self.song.is_some() {
                    if self.playback.status == Status::Playing {
                        self.send(E::Pause);
                        self.playback.status = Status::Paused;
                    } else {
                        if self.playback.status == Status::Finished {
                            self.send(E::ResetPosition);
                            self.lyrics.clear_live();
                        }
                        self.send(E::Play);
                    }
                }
                Vec::new()
            }
            Pause => {
                if self.playback.status == Status::Playing {
                    self.send(E::Pause);
                    self.playback.status = Status::Paused;
                } else if self.playback.status == Status::Paused {
                    self.send(E::Play);
                }
                Vec::new()
            }
            Stop => {
                self.release();
                self.piano.clear();
                self.lyrics.clear_live();
                self.send(E::Stop { reset: true });
                self.playback.status = Status::Stopped;
                self.playback.position_ticks = 0;
                self.playback.elapsed_ms = 0;
                Vec::new()
            }
            Previous | Next => {
                let index = self.playlist.current_index() + if action == Previous { -1 } else { 1 };
                if index >= 0 && index < self.playlist.len() as i32 {
                    self.dispatch(Command::Select(index as usize, true))
                } else {
                    Vec::new()
                }
            }
            Rewind | Forward => {
                self.release();
                self.piano.clear();
                self.lyrics.clear_live();
                self.send(E::BeatStep(if action == Rewind { -1 } else { 1 }));
                Vec::new()
            }
            RepeatOff => self.dispatch(Command::Repeat(RepeatMode::Off)),
            RepeatSong => self.dispatch(Command::Repeat(RepeatMode::Song)),
            RepeatList => self.dispatch(Command::Repeat(RepeatMode::Playlist)),
            LoadSettings => self
                .song
                .as_ref()
                .map(|song| {
                    vec![Effect::ReadSong(
                        song.path.clone(),
                        vec![
                            self.paths.song_settings_for_file(&song.path),
                            self.paths
                                .legacy_song_settings_path(&song.path.to_string_lossy()),
                        ],
                        true,
                    )]
                })
                .unwrap_or_default(),
            SaveSettings => self.save_song(),
            CopyLyrics => vec![Effect::Clipboard(self.lyrics.body.clone())],
            SaveLyrics => vec![Effect::SaveLyrics(self.lyrics.body.clone())],
            PrintLyrics => vec![Effect::Print],
            Search => vec![Effect::Url(
                "https://www.google.com/search?q=MIDI+files".into(),
            )],
            Website => vec![Effect::Url("https://dmidiplayer.sourceforge.io/".into())],
            RefreshPorts => self
                .midi_draft
                .as_ref()
                .map(|draft| vec![Effect::Ports(draft.backend.clone(), draft.advanced)])
                .unwrap_or_default(),
            TestNote => {
                if self.apply_midi() {
                    self.dispatch(Command::Preview(0, 60));
                    vec![Effect::ReleaseAfter(400, self.preview_generation)]
                } else {
                    Vec::new()
                }
            }
            Shuffle => {
                let current = self.playlist.current().map(|p| p.to_path_buf());
                self.playlist.shuffle();
                if let Some(path) = current
                    && let Some(index) = self.playlist.items().iter().position(|p| *p == path)
                {
                    self.playlist.set_current_index(index as i32);
                }
                Vec::new()
            }
            Clear => {
                if self.guard(&Command::Action(Clear)) {
                    return Vec::new();
                }
                let effects = if self.settings.auto_song_settings {
                    self.save_song()
                } else {
                    Vec::new()
                };
                self.playlist.clear();
                self.load_current(false);
                effects
            }
            Reveal => self
                .song
                .as_ref()
                .map(|song| vec![Effect::Reveal(song.path.clone())])
                .unwrap_or_default(),
        }
    }
    fn event(&mut self, event: PlayerEvent) -> Vec<Effect> {
        match event {
            PlayerEvent::Opened(song) => {
                if self.loading.as_ref() != Some(&song.path) {
                    return Vec::new();
                }
                self.loading = None;
                self.send(E::SetTempoFactor(self.playback.tempo_factor()));
                self.send(E::SetVolumeFactor(self.playback.volume_pct));
                self.send(E::SetPitchShift(self.playback.pitch_semitones));
                self.playback.reset_for_song(
                    song.len_ticks,
                    song.initial_tempo_us,
                    song.bars.max(1) as u32,
                );
                self.playback.status = Status::Ready;
                channels::reset_for_song(
                    &mut self.channels,
                    &song.channel_used,
                    &song.channel_labels,
                );
                self.piano.reset_for_song();
                self.lyrics
                    .reset_for_song(song.charset.clone(), &song.texts);
                let path = song.path.clone();
                self.song = Some(*song);
                self.refresh_lyrics();
                if self.settings.auto_song_settings {
                    vec![Effect::ReadSong(
                        path.clone(),
                        vec![
                            self.paths.song_settings_for_file(&path),
                            self.paths
                                .legacy_song_settings_path(&path.to_string_lossy()),
                        ],
                        false,
                    )]
                } else {
                    if self.play_after_load {
                        self.send(E::Play);
                        self.play_after_load = false;
                    }
                    Vec::new()
                }
            }
            PlayerEvent::LoadFailed { file, errors } => {
                if self.loading.as_ref().is_some_and(|path| *path == file) {
                    self.loading = None;
                    self.song = None;
                    self.play_after_load = false;
                    self.playback.status = Status::Ready;
                    self.errors.push(format!(
                        "Could not open {}: {}",
                        file.display(),
                        errors.join("; ")
                    ));
                }
                Vec::new()
            }
            PlayerEvent::Time { elapsed_ms, ticks } => {
                if self.playback.status == Status::Playing {
                    self.playback.elapsed_ms = elapsed_ms;
                    self.playback.position_ticks = ticks;
                }
                Vec::new()
            }
            PlayerEvent::Beat { bar, beat, max } => {
                self.playback.bar = bar;
                self.playback.beat = beat;
                self.playback.beats_per_bar = max;
                Vec::new()
            }
            PlayerEvent::NoteOn { ch, note, vel } => {
                self.piano.note_on(ch, note, vel);
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.level = channels::note_peak(row.level, vel);
                }
                Vec::new()
            }
            PlayerEvent::NoteOff { ch, note, .. } => {
                self.piano.note_off(ch, note);
                Vec::new()
            }
            PlayerEvent::Text {
                track,
                text_type,
                bytes,
                ..
            } => {
                let text = crate::core::sequence::Sequence::decode_with_charset(
                    &bytes,
                    &self.lyrics.codec,
                );
                if self.lyrics.passes_filter(track, text_type) {
                    self.lyrics.live_current = text;
                    self.lyrics.live_past.clear();
                    self.lyrics.sung = true;
                    self.lyrics.live_seq = self.lyrics.live_seq.wrapping_add(1);
                }
                Vec::new()
            }
            PlayerEvent::Program { ch, patch } => {
                if let Some(row) = self.channels.get_mut(ch as usize) {
                    row.patch = patch;
                }
                Vec::new()
            }
            PlayerEvent::Tempo { tempo_us } => {
                self.playback.note_tempo_event(tempo_us);
                Vec::new()
            }
            PlayerEvent::Started => {
                self.playback.status = Status::Playing;
                Vec::new()
            }
            PlayerEvent::Stopped => {
                if self.playback.status != Status::Paused {
                    self.playback.status = Status::Stopped;
                }
                self.piano.clear();
                Vec::new()
            }
            PlayerEvent::Finished => {
                self.playback.status = Status::Finished;
                self.playback.position_ticks = self.playback.song_len_ticks;
                self.playback.elapsed_ms =
                    self.song.as_ref().map(|song| song.duration_ms).unwrap_or(0);
                self.piano.clear();
                match playback::finish_action(
                    self.playback.repeat,
                    self.settings.auto_advance,
                    self.playlist.at_last(),
                ) {
                    playback::FinishAction::RepeatSong => {
                        self.send(E::ResetPosition);
                        self.lyrics.clear_live();
                        self.send(E::Play);
                        Vec::new()
                    }
                    playback::FinishAction::RepeatList => {
                        let index = if self.playlist.at_last() {
                            0
                        } else {
                            (self.playlist.current_index() + 1) as usize
                        };
                        self.dispatch(Command::Select(index, true))
                    }
                    playback::FinishAction::AutoAdvance => self.action(Action::Next),
                    playback::FinishAction::StayFinished => {
                        if self.settings.auto_song_settings {
                            self.save_song()
                        } else {
                            Vec::new()
                        }
                    }
                }
            }
            PlayerEvent::OutputChanged {
                backend,
                port,
                warning,
            } => {
                if let Some(warning) = warning {
                    self.errors.push(format!("MIDI output: {warning}"));
                    if !self.midi.output_ok_once
                        && let Some((backend, port)) = self.midi.next_fallback()
                    {
                        self.send(E::SelectOutput { backend, port });
                    }
                } else {
                    self.midi.backend = backend;
                    self.midi.port = port;
                    self.midi.output_ok_once = true;
                    self.notice = if self.midi.backend == "Dummy" {
                        "Silent MIDI output selected. Choose a synth in MIDI Setup.".into()
                    } else {
                        format!("MIDI output: {}", self.midi.backend)
                    };
                }
                Vec::new()
            }
            PlayerEvent::KeySignature { .. } | PlayerEvent::TimeSignature { .. } => Vec::new(),
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.release();
        self.send(E::Shutdown);
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            tracing::error!(?error, "MIDI worker failed");
        }
    }
}

fn clone_midi(midi: &MidiState) -> MidiState {
    MidiState {
        backends: midi.backends.clone(),
        ports: midi.ports.clone(),
        backend: midi.backend.clone(),
        port: midi.port.clone(),
        advanced: midi.advanced,
        instrument_map: midi.instrument_map,
        sysex_reset: midi.sysex_reset,
        soundfont: midi.soundfont.clone(),
        announce_applied: midi.announce_applied,
        fallback_stage: midi.fallback_stage,
        output_ok_once: midi.output_ok_once,
    }
}
pub fn enumerate_ports(backend: &str, advanced: bool) -> Vec<crate::core::output::MidiPort> {
    MidiOutputManager::new()
        .find(backend)
        .map(|output| output.ports(advanced))
        .unwrap_or_default()
}
fn edit_pref(settings: &mut Settings, field: Preference, value: &str) {
    let integer = value.parse::<i32>().unwrap_or(0);
    let boolean = value == "true";
    match field {
        Preference::Drums => settings.drums_channel = integer.clamp(1, 16),
        Preference::SoloReduction => settings.solo_volume_reduction = integer.clamp(0, 100),
        Preference::Autoplay => settings.auto_play = boolean,
        Preference::Advance => settings.auto_advance = boolean,
        Preference::SongSettings => settings.auto_song_settings = boolean,
        Preference::Reset => settings.sysex_reset = integer.clamp(0, 4),
        Preference::Map => settings.instrument_map = integer.clamp(0, 2),
        Preference::FutureColor => settings.future_color = value.into(),
        Preference::PastColor => settings.past_color = value.into(),
        Preference::HighlightColor => settings.highlight_color = value.into(),
        Preference::LyricsFont => settings.lyrics_font = value.into(),
        Preference::NotesFont => settings.notes_font = value.into(),
        Preference::Alignment => settings.text_alignment = integer.clamp(0, 2),
        Preference::Palette => settings.highlight_palette = integer.clamp(0, 3),
        Preference::SingleColor => settings.single_color = value.into(),
        Preference::Velocity => settings.velocity_color = boolean,
        Preference::Names => settings.names_visibility = integer.clamp(0, 3),
        Preference::Octave => settings.octave_subscript = boolean,
    }
}
