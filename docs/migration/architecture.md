# Gosh MIDI Player — Architecture Migration (GTK4/C++ → libcosmic/Rust)

- App: Gosh MIDI Player, id `com.goshapps.GoshMIDI`, command/settings name still `dmidiplayer`.
- Current stack: C++17, GTK4 + libadwaita UI, ALSA + FluidSynth output, GLib keyfiles, uchardet.
- Target stack: Rust + libcosmic (iced `Application` model).
- Branch: `cosmic-migration`. Phase 1: docs only; no source changes.

Source of truth: `src/midi/*`, `src/app/*`, `src/ui/window.*`, `src/main.cpp`, `CMakeLists.txt`.

---

## 1. Current structure (C++ / GTK4)

### 1.1 Build layout

- `dmidi-core` static library (`CMakeLists.txt`): `src/midi/{events,sequence,output,player,sysex}.cpp`,
  `src/app/{settings,playlist}.cpp`. Links GLib, ALSA, FluidSynth, uchardet, Threads.
- `dmidiplayer` binary: `src/main.cpp` + `src/ui/window.cpp` + gresource CSS.
  Defines `VERSION`, `DATADIR`.
- All code in namespace `dmidi`. Tests in `tests/` cover sequence/playlist plus identity
  and single-instance-open contract checks.

### 1.2 Class inventory

| Class / unit | File | Responsibility |
|---|---|---|
| `MidiEvent` + helpers | `src/midi/events.{hpp,cpp}` | Event value type. `Kind`: NoteOn/Off, KeyPressure, ControlChange, ProgramChange, ChannelPressure, PitchBend, SysEx, Text, Tempo, TimeSignature, KeySignature, Beat. Fields: `tick`, `delta`, `tag` (track no.), `channel`, `a/b/c` (overloaded payloads), `tempo`, `data` (SysEx/text bytes). `make*` factories, `tempoToBpm`, `clampMidi`. `isSupportedMidiFile`: `.mid .midi .kar .rmi .wrk` (case-insensitive). |
| `Sequence` | `src/midi/sequence.{hpp,cpp}` (1446 lines) | File parsing + event store + timing + navigation + text/charset. Owns `m_events` (sorted), `m_textEvents`, per-track/channel maps, tempo state. Cursor `m_pos` for playback. |
| `SequencePlayer` | `src/midi/player.{hpp,cpp}` (528 lines) | Owns `Sequence m_song` + `MidiOutput* m_out`. Playback thread, channel mix state, loop, pitch/volume, reset/companion logic. Callback fan-out to UI. `resetPosition()` resets song cursor + `m_songPositionTicks` + `m_latestBeat=m_firstBeat` (player.cpp:118-125); used by repeat-song (window.cpp:196-198) and stop-with-reset (window.cpp:432). |
| `MidiOutput` / `MidiOutputManager` | `src/midi/output.{hpp,cpp}` (472 lines) | Abstract backend (`sendNoteOn/Off/KeyPressure/Controller/Program/ChannelPressure/PitchBend/SysEx`, `ports/open/close`). `DummyOutput`, `AlsaOutput`, `FluidOutput` (file-local). Manager owns backends, `find/select/setSoundFont`. |
| SysEx helpers | `src/midi/sysex.{hpp,cpp}` | Reset messages (0 none, 1 GM, 2 GS/SC-55, 3 XG, 4 MT-32), settle/pacing delays, `.syx` parsing/loading, companion-file lookup. |
| `AppSettings` | `src/app/settings.{hpp,cpp}` | Process-wide singleton. Global prefs, recent files, config/data paths, portable mode. GKeyFile persistence. |
| `Playlist` | `src/app/playlist.{hpp,cpp}` | `.lst` playlist: items, current index, dirty flag, load/save/add/remove/move/shuffle. |
| Instrument tables | `src/app/instruments.hpp` | GM + MT-32 patch names, `patchName(map, patch)`, `inferDeviceProfile(label)` (port label → instrument map + reset kind). |
| `MainWindow` | `src/ui/window.{hpp,cpp}` (2170 lines) | Owns `SequencePlayer`, `MidiOutputManager`, `Playlist` **by value**. All widgets, actions, dialogs, player-callback wiring, song-settings load/save, piano rendering. |
| `main()` | `src/main.cpp` | CLI parsing, portable setup, settings load, `AdwApplication` single-instance open (`activate`/`open`). |

### 1.3 `Sequence` internals (formats, beats, timing, text)

Load dispatch (`Sequence::loadFile`): `.wrk` → `loadWrk`, `.rmi` → `loadRmi`, else
`loadSmfBytes` (covers `.mid/.midi/.kar`).

- **SMF** (`loadSmfBytes`): `MThd` header (format, ntrks, division; SMPTE division
  degrades to 96 with a loading error), per-track `MTrk` scan with resync on missing
  magic, VLQ delta times, running status, channel-prefix meta (`0x20`), tempo (`0x51`),
  time signature (`0x58`), key signature (`0x59`), end-of-track (`0x2F`), text metas
  `0x01–0x07` via `addMetaData`, SysEx `0xF0/0xF7`. Tracks channel usage, per-channel
  event counts, note range, channel labels (dominant-channel track name heuristic).
- **RMI** (`loadRmi`): RIFF `RMID` container; extracts `data` chunk → SMF; parses
  `LIST/INFO` metadata (`INAM/IART/ICOP/…`) into `m_infoMap` (shown in File info).
- **WRK** (`loadWrk`): `CAKEWALK` header; chunk loop over TRACK/NTRACK/STREAM/NSTREAM/
  LYRICS/SGMNT (note arrays with explicit durations → NoteOn+NoteOff pairs), TIMEBASE,
  TEMPO/NTEMPO, METER/METERKEY, COMMENTS, TRKNAME/TRKPATCH/TRKVOL/TRKBANK,
  SYSEX/SYSEX2/NSYSEX (banked, `autosend` flag → immediate vs `m_savedSysex` reference
  from `0xF0` note-array events), VARIABLE (Title/Author/Copyright/…), MARKERS.
  Track map applies per-track channel/pitch/velocity offsets to all events.
- **Beats** (`insertBeatsToEnd`): synthesizes `Kind::Beat` events (`a`=bar, `b`=beat,
  `c`=beats-per-bar) from tick 0 to song end using division + time-signature change
  list (`m_bars`). `m_barCount` = last bar.
- **Finalize** (`sortAndFinalize`): stable sort by tick, recompute `delta`,
  `m_ticksDuration`, `m_ticks2micros`.
- **Timing**: `m_ticks2micros = tempo / (division * tempoFactor)`.
  `timeOfEvent / deltaTimeOfEvent / timeOfTicks` convert ticks → microseconds.
  `setTempoFactor` **rejects** out-of-range factors (keeps the old value; no
  clamping — sequence.cpp:245-251). `updateTempo` refreshes the conversion on
  tempo events. `simpleTimeProcess()` = at most one tempo change → absolute
  scheduling. SMF `div == 0` → 96, same as the SMPTE fallback (sequence.cpp:599-602).
- **Navigation**: `setTickPosition` (linear scan to tick), `jumpToBar`,
  `previousBar/nextBar` (operate on `Kind::Beat` events, beat 1), `nearestBeatByTicks`,
  `firstBeat`. Playback cursor `m_pos` + `nextEvent()`.
- **Text/charset**: `addMetaData` feeds an uchardet sample, scores track/type for
  lyric defaults (`trackMaxPoints/typeMaxPoints`), detects KAR `@K/@V/@I/@L/@T/@W`
  headers. `Lyric`/`Text` also become playable `Kind::Text` events; TrackName/
  InstrumentName accumulate into `m_trkName`. `detectCharset` (uchardet) +
  `decodeText` (glib `g_convert` from `m_charset` to UTF-8). `extraCodecNames` =
  14-entry codec list for the lyrics Encoding dropdown.

### 1.4 `SequencePlayer` internals (threading, scheduling, mixing)

- **Thread model**: one `std::thread m_thread` running `playerLoop()`.
  - `play()`: no-op if already running or song empty; records `m_resumeWithoutReset`
    from paused state; joins stale thread; spawns loop. Sets `m_running`.
  - `pause()`: sets `m_paused` + `m_stopRequested`, joins thread, clears `m_running`,
    `allNotesOff()`. Position is kept (resume path).
  - `stop()`: clears paused, requests stop, joins, clears running, `shutupSound()`.
  - Destructor calls `stop()`.
- **Start-of-playback sequence** (fresh start only, skipped on resume):
  `sendResetMessage()` (SysEx reset + settle sleep) → `sendCompanionSysex()` (each
  `.syx` message + `max(pacing, 15ms)`) → `resetControllers()` (CC79 + CC7=100 per
  channel) → `sendVolumeEvents()` (resets stored `m_volume[ch]=100`, then sends
  CC7=scaled 100 per channel — player.cpp:321-330).
- **Scheduling** (`playerLoop`): `steady_clock` based.
  - Simple songs (≤1 tempo): absolute deadline `startTime + timeOfEvent(ev)`.
  - Multi-tempo songs: incremental `currentTime + deltaTimeOfEvent(ev)`.
  - 50-tick "echo" timer emits `onTime(ms, ticks)` while waiting (`sleep_until` loop).
  - After each event's wait: update `m_songPositionTicks`, emit `onTime`, `playEvent`.
  - Loop: when a Beat event exceeds `m_loopEnd`, break inner loop and
    `jumpToBar(m_loopStart)`, resetting `startTime`/`currentTime`/`echoTicks`.
  - End: `allNotesOff()`, emit `onStopped`; if song exhausted without stop request,
    also emit `onFinished`.
- **Event dispatch** (`playEvent`):
  - Muted channel → skip (channel events only).
  - NoteOn/Off/KeyPressure: add `m_pitchShift` unless channel == drums; emit
    `onNoteOn/onNoteOff` with the *shifted* key.
  - CC7 (Volume): store raw in `m_volume[ch]`, rescale
    `floor(raw * chFactor * masterPct / 100)` clamped 0–127. Other CCs pass through.
  - ProgramChange: locked channels force `m_lockedPgm[ch]`; update `m_lastPgm`;
    emit `onProgram`.
  - SysEx: `sendSysexPaced` (F0..F7 split, per-message pacing sleep).
  - Tempo: `song.updateTempo` then emit `onTempo(currentTempo)`.
  - Beat: update `m_latestBeat`, emit `onBeat(bar, beat, max)`.
  - Text/TimeSig/KeySig → corresponding callbacks (copies for thread safety).
- **Mixing setters** (called from UI thread, some while playing):
  `setPitchShift` (all-notes-off first), `setVolumeFactor` (resend all CC7),
  `setVolume(ch, factor)` (resend one CC7), `setMuted` (mute also sends
  AllNotesOff+AllSoundsOff), `setLocked` (snapshot `m_lastPgm` → `m_lockedPgm`),
  `setPatch` (updates last/locked + immediate `sendProgram`).
- **Seeking**: `setPosition(ticks)` and `jumpToBar` send all-notes-off first, then
  move the `Sequence` cursor + `m_songPositionTicks`. `beatForward/beatBackward`
  navigate relative to `m_latestBeat`. `resetPosition()` (no all-notes-off:
  callers stop first) resets cursor + ticks + `m_latestBeat=m_firstBeat`.
- **Stop polling**: `m_stopRequested` is checked on every 50-tick echo iteration
  (player.cpp:488), but the tail `sleep_until(nextTime)` (player.cpp:500) and the
  SysEx pacing sleeps are uninterruptible — a stop there waits out the sleep.
- **Callbacks** (all marshalled to the GTK thread via `emitToUi` → `g_idle_add`):
  `onTime(ms, ticks)`, `onBeat`, `onNoteOn/Off`, `onText`, `onProgram`, `onTempo`,
  `onKeySignature`, `onTimeSignature`, `onStarted/Stopped/Finished`.

### 1.5 Output backends

- **ALSA** (`AlsaOutput`): opens sequencer `default`, client name `dmidiplayer`,
  one `MIDI Out` port (READ|SUBS_READ). `ports(advanced)`: enumerates all clients
  except self/0, requires WRITE|SUBS_WRITE caps, skips NO_EXPORT; non-advanced mode
  filters by a MIDI-ish type mask (generic/synth/GM/GS/XG/MT32/GM2/application/
  hardware/PORT/software/synthesizer — note `SND_SEQ_PORT_TYPE_PORT`),
  with `type == 0` ports passing through unfiltered (output.cpp:113-119).
  `open("client:port")` subscribes sender→dest; unparsable id falls back to the
  first port of the **advanced** listing (`ports(true)` — output.cpp:139). Sends via
  `snd_seq_event_output_direct` (NOTEON/NOTEOFF/KEYPRESS/CONTROLLER/PGMCHANGE/
  CHANPRESS/PITCHBEND + SysEx with a scratch buffer).
- **FluidSynth** (`FluidOutput`): creates settings (gain 0.6) + synth + audio driver,
  trying `pulseaudio → pipewire → alsa → jack → sdl2`; loads requested or default
  SoundFont (`sfload`). Single port `fluidsynth` labelled with the SF filename.
  Rust note (T6, carried here in T9): the Rust port tries `sdl3`, not `sdl2`,
  as the trailing entry — upstream removed the SDL2 audio driver in
  FluidSynth 2.5.0, so `sdl2` can never construct against the pinned 2.6.0
  (`src/core/output_fluid.rs: AUDIO_DRIVERS`).
  Pitch bend remapped -8192..8191 → 0..16383. Accumulates `lastError` for UI toast.
- **Dummy**: silent sink, always available.
- **Manager**: owns backends in order ALSA, FluidSynth, Dummy; `select` closes the
  previous backend before opening the new one. `find` with an unknown backend name
  returns the front backend (ALSA), never null (output.cpp:449). `select` therefore
  never fails on the backend name, only on `open(portId)`.
  `findDefaultSoundFont` scans 8 well-known `/usr/...` paths.
- **Device profiles** (`inferDeviceProfile`): port-label sniffing (Munt/MT-32 →
  MT-32 map + MT-32 reset; SC-55/Canvas/Nuked → GS map + GS reset; XG → GM + XG
  reset; "GM emulation" → GM + GM reset). Applied when the user picks a port in
  MIDI Setup.

### 1.6 Settings, playlist, single-instance open

- **`AppSettings`** (singleton, GKeyFile `[General]` group):
  lastDirectory/lastOutputBackend/lastOutputConnection/lastPlayList, soundFont,
  language, drumsChannel (1-based, default 10), soloVolumeReduction (default 50),
  autoPlay, autoAdvance, autoSongSettings, advancedPorts, sysexReset (default GM),
  instrumentMap, highlightPalette, velocityColor, octaveSubscript, namesVisibility,
  lyricsFont/notesFont, future/past/single/highlight colors, textAlignment,
  windowWidth/Height, playlistVisible, repeatMode, recentFiles (max 10, MRU).
- **Paths**: config `$XDG_CONFIG_HOME/dmidiplayer/dmidiplayer.conf` (dir created on
  demand); data `~/.dmidiplayer/`; song settings `~/.dmidiplayer/<song>.cfg`.
  **Portable mode** (`-p` / `-f FILE`): config path = given file or
  `./dmidiplayer.conf` (`configDir` = its parent; `dataDir` unchanged).
- **`Playlist`**: `.lst` = one path per line (CRLF tolerated, blanks skipped);
  relative entries resolve against the playlist dir; `weakly_canonical` + supported-
  extension filter on load/add. `save` writes absolute paths. Dirty flag, current
  index, `selectFirst/Next/Prev`, `atFirst/atLast`.
- **Playlist index semantics** (must be preserved exactly; `m_current`, -1 = none):
  `load`/`setItems` → 0 if non-empty else -1; `clear` → -1; `add` → 0 if -1;
  `removeAt` keeps the index (next item shifts into place) but clamps to
  `size()-1` when it ran past the end; `moveUp`/`moveDown` move `m_current` with
  the item; `selectNext`/`selectPrev` return false at the ends (no wrap);
  `current()` is empty when out of range; `atFirst` = `m_current < 1` (true when
  empty), `atLast` = `m_current == size()-1`; `shuffle` keeps `m_current`
  unchanged (touches no `m_current`, playlist.cpp:61-66); `load`/`add` keep
  nonexistent paths (`weakly_canonical` + extension-only filter, no existence
  check, playlist.cpp:32,80). Tests: remove-current (first/middle/
  last), move-current up/down, next-at-end/prev-at-start return false, empty-list
  `atFirst`/`current()`.
- **`main.cpp`**: CLI (`-h/-v`, `-p/--portable`, `-f/--file FILE`,
  `-d/--driver BACKEND`, `-c/--connection PORT`, existing-files only, canonicalized).
  `AdwApplication("com.goshapps.GoshMIDI", HANDLES_OPEN)`: first launch creates the
  window (`ensure_window`, applies CLI backend/conn once); later invocations route
  through `on_open` → `openFiles(paths, replace=true)` + present (single instance).
  `-d`/`-c` are both-or-neither: applied only when both are non-empty (main.cpp:22).

### 1.7 `MainWindow` state and async

- **Owned state**: player/outputs/playlist (by value); lyric filters
  (`m_lyricTrackFilter`, `m_lyricTypeFilter`, default type 5=Lyric); `m_seeking`
  guard for the position slider; per-channel UI state `m_chLevel[16]` (meter decay),
  `m_chSoloed[16]`, `m_pianoVisible[16]`, `m_notesOn[16][128]`, `m_noteVel[16][128]`;
  `m_tightenKeys`, `m_repeat` (0/1/2), `m_refreshingLyrics/Channels` re-entrancy
  guards, `m_meterTimer` (50 ms).
- **Player-callback wiring** (constructor): `onTime`→clock label + slider (unless
  seeking); `onBeat`→`bar:beat` label + rhythm lamps (grow lamp row to `max`,
  accent on beat 1); `onNoteOn/Off`→note matrix + channel level + piano redraw;
  `onText`→filtered append with past/current tags + autoscroll; `onProgram`→patch
  dropdown; `onTempo`→bpm label; `onStarted/Stopped`→status; `onFinished`→repeat-
  song (reset+play) / save-song-settings if enabled / repeat-list (wrap) /
  autoAdvance-next.
- **Startup**: apply saved SoundFont; `select(savedBackend, savedPort)` with
  FluidSynth fallback + toast if the saved port vanished; apply drums channel +
  reset kind; load `lastPlayList` else bundled `examples.lst` else scan `./examples`.
- **Actions** (`bindActions` + header/menu): open/quit/play/pause/stop/next/prev/
  fwd/rew (bar step)/jump/loop/prefs/midi/about/help/info/save-settings/
  load-settings/search/website + repeat-off/song/list. Accelerators: Ctrl+O/Q/P/S,
  Space=play, Ctrl+arrows=song nav, F1, Ctrl+comma.
- **Dialogs**: async GTK file dialogs (open-multi, playlist open/save, SoundFont
  browse, lyrics save); `showMidiSetup` (backend/ports/advanced/map/reset/SF/test-
  note/apply; port pick auto-applies device profile); `showPrefs` (General/Lyrics/
  Piano pages, applied+saved on close); `showJump`/`showLoop` modal spin dialogs
  (jump pauses around the seek if playing); file-info/about/help alerts.
- **Song settings** (`save/loadSongSettings`): GKeyFile at
  `~/.dmidiplayer/<song-filename>.cfg`; `[Global]` = file/encoding/volume/pitch/
  timeskew; `[MIDI Channel N]` (N space-padded, e.g. `"MIDI Channel  1"`) =
  name/muted/solo/locked/patch/level. Auto load/save when `autoSongSettings`.
- **Piano**: Cairo `draw_func`, one row per used+visible channel (fallback: channel 1
  when no song); range 21–108, or tightened to song min/max ± 2; white/black key
  geometry; note-name labels per `namesVisibility` (0 never / 1 minimal C-only /
  2 when active / 3 always); click-to-preview sends NoteOn(vel 80) straight to the
  current output, **bypassing mute and pitch shift**, and on release sends note-off
  for stuck preview notes **only when the player is idle** (`!isRunning()` —
  window.cpp:2098-2116). Per-channel visibility is `m_pianoVisible[]`, toggled only
  by Show-all / Hide-unused; `m_pianoShow[]` checkboxes are never created
  (window.hpp:103, only null-checked at window.cpp:544), so per-channel piano
  toggles are an out-of-scope new feature — do not add them.
- **Lyrics view**: track/type/encoding dropdowns (defaults: best-score track,
  best-score type else Lyrics, detected charset); full-filtered static text + live
  appended `onText` with future/past/current colors; copy/save/print actions.
- **Channels view**: per-channel row (name/mute/solo/meter 0–200% volume slider/
  lock/patch dropdown), visible only for used channels; solo = attenuate
  non-soloed by `(100 - soloVolumeReduction)%`, combined multiplicatively with the
  per-channel slider.
- **Drag-drop**: `.mid/.midi/.kar/.rmi/.wrk/.lst` onto window → `openFiles`
  (replace playlist). Playlist sidebar: add/remove/move-up/move-down/shuffle/
  clear/open/save; row double-click loads+plays.

### 1.8 Threading and shared-state summary

- Threads: GTK main thread + one player thread (spawned/joined per play session).
- UI→player calls are direct method calls, some while the thread runs (`setPosition`,
  `setTempoFactor`, mix setters). No mutex on `Sequence`; benign races exist today
  (e.g. `m_ticks2micros`); the Rust port must close these (message passing).
- Player→UI is main-loop marshalled (`g_idle_add`); callback lambdas capture copies.
- Singletons/globals: `AppSettings::instance()`, portable-mode globals, GTK action
  map. Everything else is owned by `MainWindow`.

---

## 2. Target model (libcosmic / iced `Application`)

### 2.1 State model (`App` struct sketch)

```rust
struct App {
    core: cosmic::Core,          // libcosmic window/app handle
    engine: EngineHandle,        // channel to the player thread (see 2.4)
    playback: PlaybackState,     // transport + position + tempo/volume/pitch + loop + repeat
    song: Option<LoadedSong>,    // metadata snapshot for the UI (labels, bars, duration)
    playlist: PlaylistState,     // items, current index, dirty, source path
    channels: [ChannelState; 16],// mix + meter levels + labels + patch
    lyrics: LyricsState,         // filters, codec, static text, live tail
    piano: PianoState,           // note matrix, velocities, visibility, tighten
    midi: MidiState,             // backends, ports, selections, soundfont, map, reset
    settings: SettingsState,     // mirrors AppSettings (section 4)
    dialog: DialogState,         // which modal is open + its scratch state
    toasts: Vec<String>,
}

struct PlaybackState {
    status: Status,              // Ready | Playing | Paused | Stopped | Finished
    position_ticks: i64,
    song_len_ticks: i64,
    elapsed_ms: u64,
    tempo_factor_pct: u32,       // 50..=200
    volume_pct: u32,             // 0..=200
    pitch_semitones: i32,        // -12..=12
    bpm: f64,
    bar: u32, beat: u32, beats_per_bar: u32,
    loop_enabled: bool, loop_start: u32, loop_end: u32,
    repeat: RepeatMode,          // Off | Song | Playlist
    seeking: bool,
}

struct ChannelState {
    used: bool, label: String,
    muted: bool, soloed: bool, locked: bool,
    patch: u8, level_pct: u32,   // slider 0..=200
    level: f32,                  // decaying meter 0.0..=1.0
}

struct LyricsState {
    track_filter: Option<u32>,   // None = all
    type_filter: i32,            // 0 = all, else TextType
    codec: String,
    body: String,                // filtered static text
    live_tail: String,           // appended during playback
}
```

Rules: the UI never touches the engine's `Sequence` directly (unlike C++).
All song data shown in the UI arrives as immutable snapshots (`LoadedSong`:
title, format, duration, bars, division, tempo, tracks, channel usage/labels,
text events, note range, charset). All control flows as `Message` → engine
command; all engine feedback flows as `PlayerEvent` messages.

### 2.2 `Message` enum draft (must cover every current event)

```rust
enum Message {
    // --- transport ---
    Play, Pause, Stop,
    NextSong, PrevSong,
    BeatForward, BeatBackward,
    JumpToBar(u32),
    LoopToggled(bool), LoopSet { start: u32, end: u32 },
    RepeatModeChanged(RepeatMode),
    PositionSeek(f64),           // 0.0..=1.0 of song
    TempoChanged(u32),            // 50..=200 %
    VolumeChanged(u32),           // 0..=200 %
    PitchChanged(i32),            // -12..=12

    // --- playlist ---
    OpenFiles(Vec<PathBuf>, bool /*replace*/),
    PlaylistSelect(usize),
    PlaylistRemove(usize),
    PlaylistMoveUp(usize), PlaylistMoveDown(usize),
    PlaylistShuffle, PlaylistClear,
    PlaylistLoadDialog, PlaylistLoaded(Result<LoadedPlaylist, String>),
    PlaylistSaveDialog, PlaylistSaved(Result<PathBuf, String>),

    // --- per-channel mix ---
    ChannelMute(u8, bool), ChannelSolo(u8, bool), ChannelLock(u8, bool),
    ChannelPatch(u8, u8), ChannelLevel(u8, u32), ChannelName(u8, String),

    // --- lyrics ---
    LyricTrackFilter(Option<u32>), LyricTypeFilter(i32), LyricCodec(String),
    LyricsCopy, LyricsSaveDialog, LyricsSaved(Result<PathBuf, String>),
    LyricsPrint,

    // --- piano ---
    // NOTE: no per-channel PianoVisible — m_pianoShow[] is never created
    // (window.hpp:103); per-channel toggles would be a new feature (§1.7).
    PianoTighten(bool), PianoShowAll, PianoHideUnused,
    PianoPreviewOn(u8 /*ch*/, u8 /*note*/), PianoPreviewOff,

    // --- MIDI setup ---
    MidiBackendSelected(String),
    MidiPortsRefreshed(Vec<MidiPort>),
    MidiPortSelected(String),
    MidiAdvancedToggled(bool),
    MidiInstrumentMapChanged(u8), MidiResetChanged(u8),
    MidiSoundFontChanged(String), MidiSoundFontBrowse,
    MidiTestNote, MidiApply,

    // --- preferences (one per persisted key or grouped) ---
    PrefsChanged(SettingsState),

    // --- song settings ---
    SongSettingsSave, SongSettingsLoad,

    // --- dialogs / navigation ---
    ShowDialog(Dialog), CloseDialog,
    ShowFileInfo, ShowAbout, ShowHelp,
    ViewSelected(View),          // Lyrics | Channels | Piano
    Toast(String), DismissToast,

    // --- engine → UI (replaces the 12 C++ callbacks) ---
    Player(PlayerEvent),
    // --- async ---
    Tick(Instant),               // meter decay + clock refresh
    WindowResized(u32, u32),
}

enum PlayerEvent {
    Opened(LoadedSong),          // loadFile result + metadata snapshot
    LoadFailed { file: String, errors: Vec<String> },
    Time { elapsed_ms: u64, ticks: i64 },
    Beat { bar: u32, beat: u32, max: u32 },
    NoteOn { ch: u8, note: u8, vel: u8 },
    NoteOff { ch: u8, note: u8 },
    Text { track: u32, text_type: i32, ticks: i64, bytes: Vec<u8> },
    Program { ch: u8, patch: u8 },
    Tempo { tempo_us: f64 },
    KeySignature { track: u32, alterations: i8, minor: bool },
    TimeSignature { bar: u32, num: u8, den: u8 },
    Started, Stopped, Finished,
    OutputChanged { backend: String, port: String, warning: Option<String> },
}
```

Coverage check against C++: 12 player callbacks → 12 `PlayerEvent` variants
(+ open/load/output results); ~20 GActions → transport/playlist/dialog messages;
all sliders/spins/dropdowns/toggles have a message; file dialogs return
`Result` messages; meter timer → `Tick`.

### 2.3 `update` / `view` / `Task` / `Subscription`

- **`update(msg) -> Task<Message>`**: pure state transitions + engine commands.
  - Transport/mix/seek/loop messages: update `playback`/`channels` immediately
    (optimistic UI, mirrors current direct calls) and send the matching
    `EngineCommand` through `EngineHandle` (non-blocking mpsc send).
  - `Player(PlayerEvent::Finished)` encodes the current `onFinished` policy:
    repeat-song → engine reset+play; repeat-list → advance w/ wrap; else
    autoAdvance-next; autoSongSettings save before leaving the song.
  - Dialog-launching messages return file-chooser `Task`s via
    `cosmic::dialog::file_chooser` (provider: `xdg-portal`/ashpd — T0-confirmed,
    pin the `xdg-portal` feature, not `rfd`; NEVER enable `rfd` + `xdg-portal`
    together — duplicate definitions, source-verified per DECISIONS D16; same
    open-multi/save/browse roles as the current `GtkFileDialog` calls, incl. the
    MIDI/playlist glob filters in `openDialog`; `FileFilter::mimetype` is
    source-verified in ashpd 0.12.3 (`file_chooser.rs:127` — the ashpd rev
    libcosmic pins, `Cargo.toml:177`; the 0.11.1 copy in the lockfile belongs
    to rejected `rfd`) though the T0 probe compile-touched `.glob()` only
    (probe `main.rs:191-195`), so mimetype is a source-verified backup, not
    the primary filter). Results come back as
    `PlaylistLoaded / PlaylistSaved / LyricsSaved` etc. after converting each
    result `Url` to `PathBuf` (`Url::to_file_path`, percent-decoding included);
    a non-file `Url` is a load error, not a panic. `Error::Cancelled` is a
    silent no-op (no state change, no toast) — it must be distinguishable from
    real I/O errors. T0 spike caveat: open-dialog initial dir / `file_name`
    are dead upstream (ignored by the portal binding), so `lastDirectory`
    wiring into the open dialog is blocked — save-dialog names still work.
    Tests: multi-open returns several `Url`s → several `PathBuf`s; cancel →
    no message side effects; non-file `Url` → error message.
  - `Tick` decays `channels[].level *= 0.82` and refreshes the clock label.
  - DnD contract (T0-confirmed per DECISIONS D16, portal chooser stays
    primary): on X11 subscribe `cosmic::iced::window::Event::FileDropped` /
    `FileHovered` (probe compile-touched both); on Wayland wrap the drop
    target in `widget::dnd_destination(child, vec!["text/uri-list"])` +
    `.on_drop(..)` (probe compile-touched; `dnd_destination<'a, Message:
    'static>(child, mimes)`, libcosmic `widget/dnd_destination.rs:16`), parse
    the `text/uri-list` payload manually (`Url` → `PathBuf` each, same
    percent-decoding/non-file-`Url` rules as the chooser path), and accept
    sandboxed drops via the portal file transfer
    (`FILE_TRANSFER_MIME = "application/vnd.portal.filetransfer"`,
    `#[cfg(xdg_portal)]`). Every dropped `.mid/.midi/.kar/.rmi/.wrk/.lst`
    maps to `Message::OpenFiles(paths, replace=true)` — the §1.7
    replace-playlist contract. Runtime-confirm COSMIC Wayland drags in T10.
- **`view()`**: builds the widget tree from `&self`: sidebar playlist, transport
  header, dashboard (time/tempo/volume/pitch/rhythm/bar), position + tempo +
  volume sliders, Lyrics/Channels/Piano views, status line, active dialog, toasts.
  No retained widget pointers (replaces the ~171 widget fields in `MainWindow`:
  27 scalar + 9×16 per-channel arrays, window.hpp:67-103).
- **`Task` (one-shot async)**: file load (parse off the UI thread, then
  `PlayerEvent::Opened`), playlist load/save, lyrics save, port enumeration
  refresh, SoundFont browse, test-note-off delay.
- **`Subscription` (streams)**: engine event stream (`PlayerEvent` batch channel →
  `Message::Player`); 50 ms ticker while playing (or always, cheap); single-
  instance file-open listener (files from a second CLI invocation → `OpenFiles`);
  window-resize events for persisting geometry.

### 2.4 Engine thread and command channel

Keep the proven C++ topology — one scheduler thread owning the sequence and the
output backend — but replace shared-memory calls with message passing:

```rust
enum EngineCommand {
    Load(PathBuf), Play, Pause, Stop { reset: bool },
    ResetPosition, // = SequencePlayer::resetPosition: cursor + ticks + m_latestBeat
    SeekTicks(i64), JumpToBar(u32), BeatStep(i8),
    SetTempoFactor(f64), SetVolumeFactor(u32), SetPitchShift(i32),
    SetChannelVolume(u8, f64), SetMuted(u8, bool),
    SetLocked(u8, bool), SetPatch(u8, u8),
    SetLoop { enabled: bool, start: u32, end: u32 },
    SetDrumsChannel(u8), SetSysexReset(u8),
    SelectOutput { backend: String, port: String },
    SetSoundFont(String),
    PreviewNote { ch: u8, note: u8, vel: u8 }, PreviewOff { notes: Vec<(u8, u8)> },
    Shutdown,
}
```

D19 refinement (T13c): `PreviewNote`/`PreviewOff` ARE engine commands, not
app-local calls — the outputs live in the engine thread, so "app-only"
(D19's original wording) can only mean "bypasses the player MIX", not "no
engine command". The handlers write straight to the current output and touch
no mute/pitch/sequencing state (A3 bypass, `window.cpp:2098-2116`); the
piano/test-note arms in `app.rs` send them, and the idle-only release gate
lives app-side (`preview_off_targets`).

- `EngineHandle { tx: mpsc::Sender<EngineCommand> }` (`Clone`, `Send`).
- Engine loop: command queue polled on **every echo tick** (mirroring the C++
  `m_stopRequested` check per 50-tick echo, player.cpp:488), replacing the atomic
  flags; emits `PlayerEvent`s on an outbound channel bridged into an
  iced `Subscription`. Timing/scheduling/pacing algorithms ported 1:1 (section 5).
- **Wakeup**: the sleep between events must be interruptible (condvar/park +
  `tx`-side wakeup, or equivalent) so `Stop`/`Shutdown`/`SeekTicks` take effect
  within one echo quantum. C++ blocks uninterruptibly in the tail
  `sleep_until(nextTime)` (player.cpp:500) and in SysEx pacing sleeps — specify
  stop-during-long-sleep as prompt preemption, not wait-until-next-event.
  `ResetPosition` is the repeat-song/reset primitive (window.cpp:196-198, :432).
- Backpressure, per variant: `Time` is coalesced (latest-wins, bounded channel)
  so a slow UI can't grow memory; `NoteOn`/`NoteOff`/`Program`/`Text`/`Beat` and
  transport events (`Started/Stopped/Finished`) are **lossless and ordered**
  (separate unbounded-or-deep queue, never coalesced or dropped).
- Engine tests: repeat-song regression (`Finished` + repeat-song → cursor 0,
  ticks 0, beat cursor = first beat); sparse latency (long inter-event gap +
  mid-gap `Stop` → stops within ~one echo quantum); flood (note storm → every
  `NoteOn`/`NoteOff` delivered in order while `Time` may skip).

### 2.5 Single instance

Do **not** hand-roll zbus name acquisition. Boot with
`cosmic::app::run_single_instance::<App>(settings, flags)` (libcosmic
`single-instance` feature), which owns the `com.goshapps.GoshMIDI` D-Bus name:
a second launch forwards its argv to the running instance and exits. T0 spike
correction (DECISIONS D16): `Flags: cosmic::app::CosmicFlags` and `Message:
Clone + Debug + Send + 'static` are required bounds (`run_single_instance`,
app/mod.rs:198; same bound on `Application::Message`, app/mod.rs:334);
`init`/`update` return `cosmic::app::Task`, which is
`iced::Task<cosmic::Action<M>>` — wrap every app message via
`cosmic::Action::App`. The running instance overrides
`fn dbus_activation(&mut self, msg: crate::dbus_activation::Message) ->
Task<Self::Message>` (app/mod.rs:502, `#[cfg(feature = "single-instance")]`,
default `Task::none()`) to receive the forwarded launch:
`Message { activation_token: Option<String>, desktop_startup_id:
Option<String>, msg: Details }` with `Details::{Activate | Open { url:
Vec<Url> } | ActivateAction { action, args } }` (dbus_activation.rs:93-108,
generic over `Action = String, Args = Vec<String>`); the override maps
`Open`/`ActivateAction` file args to `Message::OpenFiles(paths,
replace=true)` + window present — the same contract as `on_open` today
(§1.6). CLI parsing (`-p/-f/-d/-c`,
files) stays in `main.rs` before app boot; `-d/-c` (both-or-neither, §1.6)
become the initial `SelectOutput` command instead of `connectOutput`.
Escape hatch: `COSMIC_SINGLE_INSTANCE=false` disables the single-instance
path (second process runs standalone) — document it for debugging and
multi-window testing.

**T0 pins pointer**: libcosmic rev `7e82198a`, iced 0.14.0 / iced_widget
0.14.2, MSRV 1.93, features `about` + `single-instance` + `xdg-portal`.
Full pin table lives in DECISIONS D16 — the single source of truth; this doc
only references it.

### 2.6 Color + font parser specs (T0 empirical; feeds `ui/theme.rs`)

`parseColor` (window.cpp:40-45) is `gdk_rgba_parse` over a gray init; the
lyric font string follows `pango_font_description_from_string` semantics
(DECISIONS D15: FIX — parse per this spec and apply, so default `Sans 16`
renders as 16pt Sans). Both parsers are **hand-rolled** in Rust: color is a
~80-line `gdk_rgba_parse` parity port, explicitly NOT the `css-color` crate
(DECISIONS D16). Evidence: `/tmp/t0-arch/parity`, `parity2`, `parity3`
re-run 2026-09-12 (src P1/P2/P3 below), plus follow-up probes `parity4`
(stretch/variant, src P4) and `parity5` (hsla, src P5). Every `ok=0` row
preserved the `(9,9,9,9)` sentinel, i.e. failure leaves the value untouched.

**Color spec** — applies to `futureColor`/`pastColor`/`highlightColor`/
`singleColor` (defaults `#888888`/`#1c71d8`/`#e01b24`/`#f6d32d`,
settings.hpp:33-36):

- Hex `#3/#4/#6/#8` only (`#88`, `#88g` fail): each nibble ×17, then /255
  (`#888` → 0.5333×3); alpha defaults 1.0; case-insensitive (`#aBcDeF` ok).
- `rgb()`/`rgba()` names are lowercase-only (`RGB()` fails); comma-separated
  (`rgb(255 0 0)` fails); inner spaces ok; every component clamped
  (`rgb(300,0,0)` → 1.0, `rgb(-10,0,0)` → 0.0, alpha `2` → 1.0, `-1` → 0.0);
  `rgb()` `%` components ok, but `rgba()` `%` alpha FAILS.
- `hsl()`/`hsla()` accepted (`hsl(0,100%,50%)` → red), lowercase-only too
  (`HSL()` fails).
- Named CSS table, case-insensitive (`red`/`Red`/`RED`), incl `grey` + `gray`
  (both 0.5020) and `rebeccapurple` (0.4,0.2,0.6); NO `transparent`; NO
  outer-trim (`'  #fff  '`, `' transparent '` fail).
- Any failure (unknown name, bad arity, bad length, empty/blank) leaves the
  value untouched, so the gray `(0.5,0.5,0.5,1.0)` init survives
  (window.cpp:41).

| src | input | ok | observed rgba |
|---|---|---|---|
| P1 | `#888` | 1 | 0.5333,0.5333,0.5333,1.0000 |
| P1 | `#8888` | 1 | 0.5333,0.5333,0.5333,0.5333 |
| P1 | `#888888` | 1 | 0.5333,0.5333,0.5333,1.0000 |
| P1 | `#88888888` | 1 | 0.5333,0.5333,0.5333,0.5333 |
| P1 | `#88` | 0 | untouched |
| P1 | `#ffff` | 1 | 1.0000,1.0000,1.0000,1.0000 |
| P1 | `#FFF` | 1 | 1.0000,1.0000,1.0000,1.0000 |
| P1 | `'  #fff  '` | 0 | untouched (no outer-trim) |
| P1 | `rgb(255,0,0)` | 1 | 1.0000,0.0000,0.0000,1.0000 |
| P1 | `rgba(255,0,0,0.5)` | 1 | 1.0000,0.0000,0.0000,0.5000 |
| P1 | `rgb(100%,0%,0%)` | 1 | 1.0000,0.0000,0.0000,1.0000 |
| P1 | `rgba(100%,0%,0%,50%)` | 0 | untouched (%-alpha fails) |
| P1 | `rgb(300,0,0)` | 1 | clamped 1.0000,0.0000,0.0000,1.0000 |
| P1 | `rgb(255,0)` | 0 | untouched (bad arity) |
| P1 | `rgb(255 0 0)` | 0 | untouched (commas required) |
| P1 | `red` / `Red` / `RED` | 1 | 1.0000,0.0000,0.0000,1.0000 |
| P1 | `transparent` | 0 | untouched (not in table) |
| P1 | `notacolor` / `""` / `"   "` | 0 | untouched |
| P1 | `grey` / `gray` | 1 | 0.5020,0.5020,0.5020,1.0000 |
| P1 | `rebeccapurple` | 1 | 0.4000,0.2000,0.6000,1.0000 |
| P2 | `rgba(255,0,0,2)` | 1 | alpha clamped 1.0000 |
| P2 | `rgba(255,0,0,-1)` | 1 | alpha clamped 0.0000 |
| P2 | `rgb(255, 0, 0)` | 1 | inner spaces ok |
| P2 | `RGB(255,0,0)` | 0 | untouched (lowercase-only) |
| P2 | `rgb(-10,0,0)` | 1 | clamped 0.0000,0.0000,0.0000,1.0000 |
| P2 | `hsl(0,100%,50%)` | 1 | 1.0000,0.0000,0.0000,1.0000 |
| P2 | `rgba(255, 0, 0, 0.5)` | 1 | ok |
| P2 | `#aBcDeF` | 1 | 0.6706,0.8039,0.9373,1.0000 |
| P2 | `#abcd` | 1 | 0.6667,0.7333,0.8000,0.8667 |
| P5 | `hsla(0,100%,50%,0.5)` | 1 | 1.0000,0.0000,0.0000,0.5000 |
| P5 | `HSL(0,100%,50%)` | 0 | untouched (lowercase-only) |
| P5 | `' transparent '` | 0 | untouched (no outer-trim) |
| P5 | `#88g` | 0 | untouched (bad hex) |

**Font spec** — defaults lyrics `Sans 16`, notes `Sans 9`
(settings.hpp:31-32; `notesFont` stays a headless carried key per D13 —
parse it the same, apply nothing):

- Trailing float > 0 = size in pt (Pango units = pt×1024: `16` → 16384,
  `10.5` → 10752); `Sans 0` → size 0 = no size → default size; `Sans -5` →
  absorbed into the family.
- `12px` suffix = absolute px size (abs=1); `12pt` = literal family text
  (family `Sans 12pt`, no size). Divergent by design — pin both.
- Keywords are case-insensitive (`sans bold 12` → weight 700): style
  Italic/Oblique, weight incl Semi-Bold 600 / Bold 700 / Ultra-Bold 800,
  stretch Condensed…Expanded, Small-Caps variant; numeric weights ignored
  (`Sans 600 12` → family `Sans 600`, weight 400).
- Family = remainder verbatim: spaces kept (`DejaVu Sans Mono`), commas kept
  (`Sans,Serif`); when resolving a concrete typeface, the first
  comma-separated entry wins.
- Empty string / bare number (`16` → family null) → `Sans` fallback; `!!!`
  → family text, default size.
- Convert pt sizes to iced logical px with `px = pt × 96/72`.

| src | input | family | size (Pango units) | attrs |
|---|---|---|---|---|
| P1 | `Sans 16` | Sans | 16384 = 16pt | weight 400 |
| P1 | `Sans Bold 12` | Sans | 12288 = 12pt | weight 700 |
| P1 | `Sans Italic Bold 10.5` | Sans | 10752 = 10.5pt | weight 700, Italic |
| P1 | `Sans` | Sans | 0 (none → default) | — |
| P1 | `16` | (null → Sans) | 16384 = 16pt | bare number |
| P1 | `""` | (null → Sans) | 0 (none → default) | — |
| P1 | `!!!` | `!!!` | 0 (none → default) | family text |
| P1 | `Monospace 9` | Monospace | 9216 = 9pt | — |
| P1 | `Sans,Serif 12` | `Sans,Serif` | 12288 = 12pt | comma kept |
| P1 | `'  Sans   16  '` | Sans | 16384 = 16pt | outer space tolerated |
| P1 | `Cantarell Semi-Bold 11` | Cantarell | 11264 = 11pt | weight 600 |
| P1 | `Sans 0` | Sans | 0 (none → default) | zero is not a size |
| P1 | `Sans -5` | `Sans -5` | 0 (none → default) | negative absorbed |
| P3 | `sans bold 12` | sans | 12288 = 12pt | weight 700, ci keywords |
| P3 | `Sans 12pt` | `Sans 12pt` | 0 (none → default) | pt = family text |
| P3 | `Sans 12px` | Sans | 12288 absolute | px = absolute |
| P3 | `Sans 600 12` | `Sans 600` | 12288 = 12pt | weight 400, numeric ignored |
| P3 | `DejaVu Sans Mono Bold Oblique 9` | DejaVu Sans Mono | 9216 = 9pt | weight 700, Oblique |
| P4 | `Sans Condensed 12` | Sans | 12288 = 12pt | stretch Condensed |
| P4 | `Sans Small-Caps 12` | Sans | 12288 = 12pt | variant Small-Caps |
| P4 | `Sans Expanded Bold Italic 12` | Sans | 12288 = 12pt | 700, Italic, Expanded |
| P4 | `Sans Ultra-Bold 12` | Sans | 12288 = 12pt | weight 800 |
| P4 | `Sans Oblique 12` | Sans | 12288 = 12pt | Oblique |
| P4 | `DejaVu Sans, Sans 12` | `DejaVu Sans,Sans` | 12288 = 12pt | first entry wins |

CSS-shaping evidence (P2, backs the D15 FIX ruling — `font: Sans 16` as the
C++ emits it is invalid CSS, so the setting is a no-op there):

| rule | parsing errors |
|---|---|
| `.lyrics-view { font: Sans 16; }` | 1 (`Expected a number`) |
| `.lyrics-view { font: 16pt Sans; }` | 0 |
| `.lyrics-view { font: Bold 12px Sans; }` | 0 |
| `.lyrics-view { font: Sans; }` | 1 (`Expected a number`) |

**Rejection / divergence tests** (unit-test every row): `RGB(255,0,0)` →
Err; `' transparent '` → Err (bare `transparent` too); `rgba(100%,0%,0%,
50%)` → Err; bad arity `rgb(255,0)` / `rgb(255 0 0)` / `#88` / `#88g` /
`""` / `"   "` → Err with the value untouched; `'Sans 12px'` → absolute
12px vs `'Sans 12pt'` → family `Sans 12pt` + default size; `'Sans 600 12'`
→ family `Sans 600`, weight 400, 12pt; `'!!!'` → family `!!!`, default
size, no crash.

---

## 3. Business logic: reuse vs rewrite

**Recommendation: rewrite the core in Rust, porting algorithms 1:1.** A `cxx`/FFI
bridge over `dmidi-core` is rejected: `player.cpp` is coupled to GLib (`g_idle_add`)
for every callback, `sequence.cpp` to GLib (`g_convert`) + uchardet, and the
header-only instrument tables plus playback cursor aliasing (`MidiEvent*` into a
`std::vector`) do not map safely to Rust. A clean-room port keeps one language,
one async model, and eliminates the current data races.

### 3.1 MIDI parsing (rewrite)

- Port `Reader` (BE/LE ints, VLQ, chunk framing) as a small `nom`-free cursor
  module; keep the same leniency (MTrk resync, corrupted-chunk bail with error
  strings, SMPTE→96 fallback).
- Port SMF/RMI/WRK decoders field-for-field, including: WRK banked SysEx +
  `autosend`, per-track channel/pitch/velocity offsets, tempo/meter chunk variants,
  VARIABLE/MARKERS mapping; KAR `@`-headers; INFO-chunk metadata map.
- Port beat synthesis (`insertBeatsToEnd`), stable tick sort + delta recompute,
  tempo-factor math, bar navigation, track/type scoring, note-range tracking.
- **Parity proof**: golden tests — parse `examples/*` in both implementations and
  diff event counts, tick duration, bar count, note range, channel usage, decoded
  lyrics (see also the existing `tests/test_sequence.cpp`).

### 3.2 Playback engine (rewrite)

- Port `playerLoop` scheduling (simple-absolute vs multi-tempo-incremental,
  50-tick echo, loop re-jump, start sequence reset→companion→controllers→volumes,
  per-message SysEx pacing/sleeps) and `playEvent` mixing rules exactly.
- Replace `std::thread` + atomics + `g_idle_add` with the section 2.4 command
  channel + event subscription. No shared mutable state between UI and engine.

### 3.3 ALSA / FluidSynth bindings (bind, don't rewrite)

- **ALSA**: `alsa` crate (sequencer API). Map 1:1 — client/port creation, port
  enumeration with the same caps/type filter (**including** `PORT` in the
  MIDI-ish mask **and** the `type == 0` passthrough, output.cpp:113-119),
  subscribe/unsubscribe, `output_direct`-equivalent event sends,
  `client:port` id parsing + first-port-of-**advanced**-listing fallback
  (output.cpp:139), unknown-backend→front (ALSA) rule (output.cpp:449), and
  `-d`/`-c` both-or-neither (main.cpp:22). Client name stays `dmidiplayer`
  (or app id — decide once; tests may pin it). Tests (mock the port list):
  `type==0` port visible in non-advanced mode; unparsable id → first advanced
  port; unknown backend selects ALSA; `-d` without `-c` is ignored.
- **FluidSynth**: hand-written FFI over `libfluidsynth` — FINAL per the T0
  pack spike (DECISIONS D16). The `fluidsynth` 0.0.1 crate is 2018 abandonware
  and is rejected; no `bindgen` (avoids a libclang build dependency). Scope is
  small (~17 functions, 3 opaque types: settings, synth, audio driver):
  settings/synth/audio-driver creation with the same driver fallback order
  (trailing SDL entry `sdl3` in Rust, `sdl2` in C++ — upstream removed the
  SDL2 audio driver in 2.5.0 and the pin is 2.6.0, see the §1.5 note) and
  gain 0.6, `sfload`, note/cc/program/pressure/bend/sysex calls, pitch-bend range
  remap. Keep default-SoundFont search paths and `lastError` accumulation.
- **Dummy**: trivial Rust sink.
- Keep the `MidiOutput` trait shape (backend name, ports, open/close, 8 send fns)
  so `MidiOutputManager::select` semantics (close-previous-first) survive, with
  one deliberate change: Rust `open` returns `Result<(), OutputError>` (typed
  error carrying the former `lastError` text) instead of `bool` + `lastError()`.
  Test: no SoundFont anywhere on the search path **and** no ALSA sequencer →
  FluidSynth/ALSA `open` returns `Err` with a surfaced message and the manager
  can still fall back to Dummy (never a panic, never silent success).

### 3.4 Text / charset (replace libs, keep behavior)

- Replace uchardet → `chardetng`; replace `g_convert` → `encoding_rs` (with the
  same 14-codec list exposed in the UI; map codec names accordingly).
- Keep: detection over the concatenated text sample, per-song override, UTF-8
  fallback that returns raw bytes on conversion failure, best-track/type defaults.
- **Charset normalization spec** (parity-critical):
  - `ISO-8859-1` vs `WINDOWS-1252` must stay distinct: bytes 0x80–0x9F decode to
    U+0080–U+009F controls under `g_convert`/ISO-8859-1 but to printable glyphs
    (€, ‚, …) under WINDOWS-1252. Never normalize one to the other; map both to
    the matching `encoding_rs` labels (`ISO-8859-1` ≈ `latin1`,
    `WINDOWS-1252`).
  - Detector-name mapping: `chardetng` names must be mapped onto the same
    14-entry dropdown vocabulary uchardet names are matched against today
    (case-insensitive compare at window.cpp:580; e.g. detector `Shift_JIS` →
    dropdown `SHIFT_JIS`, `Big5` → `BIG5`, `x-mac-*` → `MACINTOSH`). Pin the
    full mapping table in code with a unit test per entry.
  - Off-list fallback: when the detected name matches no dropdown entry, the
    dropdown selects index 0 while `m_charset` keeps the detected value
    (window.cpp:575-585) — preserve this split (display ≠ decode charset).
- Golden tests: byte vectors containing 0x80–0x9F decoded under both
  ISO-8859-1 and WINDOWS-1252 labels (assert the divergent outputs); an
  off-list detected name → dropdown index 0 + decode still uses the detected
  name; `examples/*.kar` lyrics byte-identical to the C++ decoder.

### 3.5 What stays conceptually identical

Playlist `.lst` format + canonicalization rules + index semantics (§1.6), GKeyFile-compatible settings
keys (section 4), song-settings schema, device-profile sniffing table, GM/MT-32
patch tables, repeat/autoplay/auto-advance policy, lyric tag color roles,
piano geometry + visibility modes, CLI flags and single-instance open contract.

---

## 4. Settings / persistence

### 4.1 Paths (must not change without a migration)

| Item | Path |
|---|---|
| Global config | `$XDG_CONFIG_HOME/dmidiplayer/dmidiplayer.conf` (create dirs on save) |
| Portable config | `./dmidiplayer.conf` or `-f FILE` (parent = config dir) |
| Data dir | `~/.dmidiplayer/` (created on demand) |
| Song settings | `~/.dmidiplayer/<song-file-name>.cfg` |
| Last playlist | path stored in config (`lastPlayList`) |
| Examples fallback | `<datadir>/dmidiplayer/examples.lst`, `~/.local/share/dmidiplayer/examples.lst`, `/usr/share/dmidiplayer/examples.lst`, `./examples/` |

### 4.2 Global config keys (`[General]`, GKeyFile INI booleans/integers/strings)

All keys from `AppSettings` (§1.6) must round-trip: lastDirectory,
lastOutputBackend/Connection, lastPlayList, soundFont, language, drumsChannel,
soloVolumeReduction, autoPlay, autoAdvance, autoSongSettings, advancedPorts,
sysexReset, instrumentMap, highlightPalette, velocityColor, octaveSubscript,
namesVisibility, lyricsFont, notesFont, future/past/single/highlight colors,
textAlignment, windowWidth/Height, playlistVisible, repeatMode, recentFiles
(list, max 10 MRU).

Rust: implement a small GKeyFile-compatible reader/writer (or `configparser`
style crate) — do **not** silently migrate to TOML/JSON, or existing user
configs and song files stop working. Unknown keys must be preserved. With teeth:
- **Escaping**: GKeyFile string escaping (`\n`, `\t`, `\\`, `\r`, unicode) must
  round-trip byte-identically — channel names and paths with newlines, backslashes,
  `;`/`#` (comment chars), and non-ASCII text must survive save→load.
- **Corrupt → defaults**: an unloadable config keeps in-memory defaults and
  returns (settings.cpp:79-84); per-key parse errors keep that field's default;
  a missing `recentFiles` key yields an empty list. Never crash, never partial-write.
- **Dir creation**: the config dir is created on demand by the path helper
  itself (`create_directories` in `defaultConfigPath`, settings.cpp) — not by the
  caller — so first-run save always succeeds.
- Fixture tests: corrupt conf → defaults; escaped values round-trip; missing dir
  created on save; unknown keys preserved across load→save.

### 4.3 Song settings (`[Global]` + `[MIDI Channel N]`, N space-padded to width 2)

Keys: `file`, `encoding`, `volume` (0–200), `pitch`, `timeskew` (50–200);
per channel: `name`, `muted`, `solo`, `locked`, `patch`, `level`. Load applies
encoding→lyrics refresh, volume/pitch/tempo controls, per-channel mix + patch.
Auto load on song open / auto save on song leave + finish, gated by
`autoSongSettings`. Manual actions always available.
**Apply-direction ruling (FIX)**: on load, the engine is the source of truth —
send the mix to the engine first (`SetMuted`/`SetLocked`/`SetPatch`/
`SetChannelVolume`/`SetVolumeFactor`/`SetPitchShift`/`SetTempoFactor`), then
sync UI state (`ChannelState`, sliders) from the applied engine values. Never
UI-first with the engine catching up via widget signals (the C++ load path leans
on GTK signal side effects, window.cpp:661-710). Test: load song settings while
a song is loaded → engine mix equals the file, UI equals the engine, and an
audible check (CC7 on the Dummy/ALSA spy) confirms the engine actually applied it.

### 4.4 `config` module sketch

```rust
config::Paths { config_file, config_dir, data_dir, portable: bool }
config::Settings  // all §4.2 keys + load/save/add_recent/reset_defaults
config::SongSettings // all §4.3 keys + load/save/apply
```

---

## 5. Module layout (Rust workspace)

```text
src/
  main.rs            CLI parsing, portable setup, run_single_instance boot (§2.5)
  app.rs             App state (§2.1), update(), view(), subscriptions
                     + T13c D3/D7/D8 dialog shells, piano/test-note arms,
                     T13 sheet wiring via `chrome::wrap_modal`
  message.rs         Message + PlayerEvent + EngineCommand + Dialog + View enums
                     + T13c scratch/URI/test-note variants, D6 `LoadedSong` fields
  core/
    mod.rs
    event.rs         MidiEvent, Kind, TextType, constants, factories  (events.*)
    sequence.rs      Sequence store, timing, navigation, text scoring (sequence.*)
    smf.rs           loadSmfBytes                                      (sequence.cpp split)
    rmi.rs           loadRmi
    wrk.rs           loadWrk (+ chunk consts, banked sysex)
    player.rs        Engine thread, scheduler, mixing, pacing          (player.*)
                     + T13c `PreviewNote`/`PreviewOff` bypass commands +
                     D6 `metadata`/`copyrights` snapshot fields
    output.rs        MidiOutput trait + Manager + port types           (output.*)
    output_alsa.rs   AlsaOutput                                        (output.cpp split)
    output_fluid.rs  FluidOutput + SoundFont search
    sysex.rs         resets, pacing, .syx parse/load, companion lookup(sysex.*)
  state/
    mod.rs
    playback.rs      PlaybackState + repeat/finish policy
    playlist.rs      PlaylistState + .lst load/save                   (playlist.*)
    channels.rs      [ChannelState; 16] + solo/mute reduction math
    lyrics.rs        LyricsState + filtering + tag-span model (+ Rgba /
                     gdk_rgba_parse parity shared by the piano read path)
    piano.rs         PianoState + T13a geometry (drawn range, rows, click→note
                     with the U12 drawn-range FIX), A3 audition rules
                     (PreviewNote spec → engine-carried bypass, idle-only
                     offs), highlight / velocity / note-name settings read path
    midi.rs          MidiState (backends/ports/selections) + T13a D3 logic
                     (field order, refill preselect, port→profile, test-note
                     plan, hint strings, stop-first on reseat)
    dialogs.rs       T13a D7/D8/D6/D5/D4 pure logic: jump range + pause-jump-
                     resume plan, loop entry/seeds/confirm/cancel (+ U3 menu
                     FIX pin), file-info assembly, kHelpText copy, About table,
                     manual song-settings precondition
  config/
    mod.rs
    paths.rs         config/data/song paths + portable mode
    settings.rs      global keys load/save                            (settings.*)
    song.rs          song-settings load/save/apply
  ui/
    mod.rs
    transport.rs     header + dashboard + sliders views
    playlist_view.rs sidebar
    lyrics_view.rs   lyrics page + dialogs hooks
    channels_view.rs channels page
    piano_view.rs    piano canvas (iced canvas program)
    dialogs.rs       MIDI setup, prefs, jump, loop, file info, about, help
    theme.rs         palette/lyrics colors/fonts from settings
```

Dependency rule: `ui` → `state`/`config` → `core`; `core` never depends on UI
types. `core` has no iced/libcosmic dependency so the engine stays testable
headless (keep/extend `tests/` golden tests against `core`).

---

## 6. Parity notes (behavior that must match 1:1)

1. **File support**: `.mid/.midi/.kar/.rmi/.wrk` (+`.lst` playlists); same
   canonicalization + extension filter; same loading-error strings surfaced.
2. **Scheduling**: simple-song absolute vs multi-tempo incremental timing;
   50-tick echo `Time` events; identical tick→microsecond math incl. tempo-factor
   **reject** outside 0.1–10 (not clamp) and UI tempo 50–200%.
3. **Start sequence**: SysEx reset (+settle) → companion `.syx` (+pacing, min
   15 ms) → reset-controllers → volume events; skipped on pause-resume.
4. **Mixing**: CC7 `floor(raw·ch·master/100)` clamp; pitch shift excludes drums;
   mute sends AllNotesOff+AllSoundsOff; lock snapshots/forces patch; immediate
   `sendProgram` on patch change; seek/jump/bar-step all-notes-off first.
5. **Loop**: bar-range re-jump inside the scheduler; enable/disable + start/end
   bars preserved across UI round-trips; loop toggle-off clears immediately.
6. **End-of-song**: Stopped always; Finished only when exhausted; repeat-song
   (via `ResetPosition`) / repeat-list-wrap / autoAdvance-next /
   autoSongSettings-save policy identical.
7. **Lyrics**: best-track/type defaults; static filtered body + live tail;
   future/past/current color roles; encoding override + detection; copy/save/print.
8. **Channels**: rows only for used channels; solo reduction math; patch dropdown
   follows incoming programs; instrument map relabels all 128 entries.
9. **Piano**: row-per-used-channel, 21–108 / tighten ±2, name-visibility modes,
   velocity tint, per-channel palettes, click-preview with idle-only note-off.
10. **MIDI setup**: ALSA enumeration filter (incl. advanced toggle, PORT type,
    `type==0` passthrough), first-**advanced**-port fallback, unknown-backend→ALSA,
    device-profile auto-apply on port pick, driver fallback order,
    default-SF search paths, test-note (ch1 middle C, 400 ms), error toasts.
11. **Persistence**: config/song-settings paths, key names, `[MIDI Channel N]`
    padding, recent-files MRU-10, window geometry + repeat mode saved on exit,
    portable-mode semantics.
12. **App contract**: id `com.goshapps.GoshMIDI`, CLI flags, single-instance
    forward-to-running + present, drag-drop replace-playlist, accelerators,
    repeat/song/playlist menu structure, help/about content parity.



