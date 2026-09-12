//! Player engine — port of `SequencePlayer` (`src/midi/player.{hpp,cpp}`).
//!
//! PLAN T7, arch §1.4 and §2.4. One scheduler owning the [`Sequence`] and
//! the [`MidiOutputManager`], driven by [`EngineCommand`] messages and
//! emitting [`PlayerEvent`]s — no shared mutable state with the UI thread
//! (arch §2.4 risk 6).
//!
//! ## Protocol vs `message.rs`
//!
//! The engine defines its own [`EngineCommand`] / [`PlayerEvent`] shapes,
//! mirroring `crate::message` field-for-field so T9 can bridge them
//! mechanically. Deliberate deltas (message.rs is untouched in T7):
//! * `PreviewNote` / `PreviewOff` ARE engine commands (T13c D19 refinement):
//!   audition **bypasses** the player mix and talks to the outputs directly
//!   (A3 — bypasses mute/pitch, idle-only note-off) — but the outputs live
//!   in the engine thread, so "app-only" can only mean "bypasses player
//!   mix", not "no engine command". The handlers write straight to the
//!   current output and touch no mute/pitch/sequencing state.
//! * `PlayerEvent::NoteOff` carries `vel`: the C++ `onNoteOff(ch, note, vel)`
//!   callback has it; the message.rs T1 stub dropped it. T9 reconciles.
//! * `Opened` carries the engine-side [`SongSnapshot`] (full UI fields since
//!   T9, + D6 metadata/copyrights since T13c; the app converts it to
//!   `message::LoadedSong` via `From`).
//!
//! ## Threading and wakeup (A2)
//!
//! [`spawn_engine`] builds the [`Engine`] *inside* the new thread, so the
//! outputs (ALSA `Seq`, FluidSynth raw pointers) never cross threads and no
//! `Send` bound is needed on [`MidiOutput`][crate::core::output::MidiOutput].
//! Idle wakeup is immediate (`mpsc::recv`); while playing, commands are
//! polled on **every echo tick and every event** (a superset of the C++
//! `m_stopRequested` checks), and every sleep is chunked so a command takes
//! effect within ~one echo quantum.
//!
//! ## Stop-during-sleep: deliberate improvement over C++
//!
//! C++ checks stop per echo iteration but blocks uninterruptibly in the tail
//! `sleep_until` (≤1 echo quantum) and — worse — in every SysEx pacing sleep
//! with no checks at all, so stopping during a big companion `.syx` waits
//! out the *whole* remaining start sequence, and a stop landing in the tail
//! sleep still plays the pending event. The Rust engine instead:
//! * chunks the tail wait and every pacing/settle sleep (echo-quantum and
//!   5 ms chunks respectively), polling commands between chunks;
//! * aborts between SysEx/companion messages;
//! * skips the pending event uniformly once an aborting command
//!   (`Stop` / `Pause` / `Load` / `Shutdown`) is observed.
//!
//! Stop latency is therefore bounded by ~one echo quantum even with huge
//! SysEx blobs. Pinned by `sparse_stop_latency_is_one_quantum` and
//! `stop_aborts_companion_pacing`.
//!
//! ## Backpressure (A13)
//!
//! [`event_channel`] returns a dual plain-`mpsc` outbound end:
//! * lossless, ordered, unbounded `mpsc` for notes / programs / text /
//!   beats / tempo / signatures / transport — never dropped, never
//!   reordered (single producer), engine never blocks on a slow UI;
//! * bounded (`TIME_CHANNEL_CAP`) `sync_channel` for `Time`: `try_send`,
//!   shed on full (counted in `time_dropped`, shared with the receiver
//!   end). A slow UI skips stale clock updates; fresher ones keep coming.
//!
//! T9 drains lossless in order and takes the latest `Time`
//! ([`EngineEventReceiver::drain_time_latest`]) per frame.
//!
//! ## Further 1:1 notes and deviations
//!
//! * Scheduler: simple-song absolute vs multi-tempo incremental deadlines,
//!   50-tick echo `Time` emits, loop re-jump, end policy (`Stopped` always,
//!   `Finished` only when exhausted without a stop request) — all 1:1.
//! * Start sequence (fresh starts only, skipped on resume): reset SysEx +
//!   settle → companion `.syx` (pacing, min 15 ms) → reset-controllers →
//!   volume events (incl. the `m_volume[ch] = 100` reset, A9).
//! * `echo_ticks` restarts at 0 on every `play()` call, including resume —
//!   the C++ local does the same, so the clock re-counts from 0 until the
//!   first event wait completes.
//! * With no output selected, `play_event` is a whole-function no-op —
//!   including *skipped tempo updates* — exactly like the C++ null-`m_out`
//!   early return.
//! * Zero-tempo guard (DEVIATION): a zero/truncated echo quantum
//!   (sub-microsecond `ticks2micros` truncation with a nonzero wait) skips
//!   the echo loop and waits in one chunk. Same timing, no spin.
//! * Shifted note keys go to the output unclamped (1:1) but are clamped to
//!   0..=127 in `NoteOn`/`NoteOff` events so UI note matrices stay in
//!   range; channel events with out-of-range channels are ignored (the
//!   decoders cannot produce them).
//! * Stop-during-play sends the postlude all-notes-off once. C++ sends it
//!   twice (loop postlude + `stop()`'s `shutupSound`); the CCs are
//!   idempotent, so there is no audible difference.
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use super::event::{
    CC_ALL_NOTES_OFF, CC_ALL_SOUNDS_OFF, CC_RESET_CONTROLLERS, CC_VOLUME, MIDI_CHANNELS, TextType,
    clamp_midi, tempo_to_bpm,
};
use super::event::{Kind, MidiEvent};
use super::output::MidiOutputManager;
use super::sequence::{Sequence, TextRec};
use super::sysex::{
    companion_syx_path, load_sysex_file, parse_sysex_messages, sysex_pacing_ms,
    sysex_reset_message, sysex_reset_settle_ms,
};

/// Echo quantum in ticks (player.cpp:458 `echoRes`).
pub const ECHO_RES_TICKS: i64 = 50;
/// Capacity of the bounded `Time` channel (A13): covers echo bursts; a slow
/// UI sheds stale clock updates while lossless events queue separately.
pub const TIME_CHANNEL_CAP: usize = 8;
/// Minimum per-message pacing for companion `.syx` (player.cpp:316).
pub const COMPANION_MIN_PACING_MS: u64 = 15;
/// Chunk size for SysEx pacing / reset settle sleeps: commands are polled
/// between chunks (A2 prompt preemption).
const PACING_CHUNK_MS: u64 = 5;

// ---------------------------------------------------------------------------
// Protocol: EngineCommand / PlayerEvent
// ---------------------------------------------------------------------------

/// Immutable song metadata snapshot for `PlayerEvent::Opened`: everything
/// the UI shows about a song (arch §2.1 `LoadedSong`: labels, bars,
/// duration, …), built from the engine's [`Sequence`] at load time. The app
/// shell converts this to its own `LoadedSong` (`From`, no bridge code).
#[derive(Debug, Clone, PartialEq)]
pub struct SongSnapshot {
    pub title: String,
    pub format: String,
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
    pub charset: String,
    /// Raw text/meta records for the lyrics view (T11 decodes + filters).
    pub texts: Vec<TextRec>,
    /// RIFF INFO `(key, value)` pairs in map order, for the D6 file-info
    /// dialog (`Sequence::metadataInfo` source; `BTreeMap` iterates sorted
    /// like the C++ `std::map`).
    pub metadata: Vec<(String, String)>,
    /// Decoded copyright records in event order, for D6 (`getText(Copyright)`
    /// at load time — same bytes the view decodes from `texts` + `charset`).
    pub copyrights: Vec<String>,
}

/// Engine → UI events: the 12 C++ player callbacks plus open/load/output
/// results. Owned here; `crate::message` re-exports this type (D19), so the
/// app and the engine can never drift.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// Boxed: the snapshot is ~0.5 KB but `Opened` fires once per song,
    /// while the other variants flow at event rate.
    Opened(Box<SongSnapshot>),
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
    /// Carries `vel` (C++ `onNoteOff(ch, note, vel)` has it).
    NoteOff {
        ch: u8,
        note: u8,
        vel: u8,
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

/// UI → engine commands (arch §2.4). All data is owned: no shared `Sequence`
/// (message passing only). The preview variants bypass the player MIX but
/// still cross into the engine thread — the outputs live there (A3/D19, see
/// module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineCommand {
    Load(PathBuf),
    Play,
    Pause,
    Stop {
        reset: bool,
    },
    /// `SequencePlayer::resetPosition` (A1): cursor + ticks + beat cursor
    /// reset, unlike `SeekTicks` which leaves the beat cursor alone.
    ResetPosition,
    SeekTicks(i64),
    JumpToBar(u32),
    /// Positive = forward a bar, negative = back, zero = no-op.
    BeatStep(i8),
    /// Tempo factor 0.1–10; out-of-range values are rejected (A7), never
    /// clamped.
    SetTempoFactor(f64),
    /// Master volume percent (UI 0–200); resends all CC7 (A9).
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
    /// 0-based drums channel (C++ `m_drumsChannel`).
    SetDrumsChannel(u8),
    /// SysEx reset kind 0–4 (C++ `m_sysexReset`).
    SetSysexReset(u8),
    SelectOutput {
        backend: String,
        port: String,
    },
    SetSoundFont(String),
    /// Piano audition / MIDI-setup test note (A3/D19): raw NoteOn straight
    /// to the current output — no mute, pitch-shift, or sequencing touch
    /// (`window.cpp:2098-2102`, `:1147`). No-op when no output is live.
    PreviewNote {
        ch: u8,
        note: u8,
        vel: u8,
    },
    /// Preview/test release: raw NoteOffs straight to the current output,
    /// same bypass (`window.cpp:2105-2116`, `:1148-1153`). The idle-only
    /// gate lives app-side (`preview_off_targets`); the test-note off is
    /// unconditional like C++ — the engine just sends what it is given.
    PreviewOff {
        notes: Vec<(u8, u8)>,
    },
    Shutdown,
}

// ---------------------------------------------------------------------------
// Clock (injectable for deterministic scheduler tests)
// ---------------------------------------------------------------------------

/// Wall clock + sleep seam. [`RealClock`] sleeps for real; tests inject a
/// fake that advances instantly.
pub trait PlayerClock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

/// Real-time clock for the spawned engine thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealClock;

impl PlayerClock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

// ---------------------------------------------------------------------------
// Command source / event sink seams
// ---------------------------------------------------------------------------

/// Something the engine can poll commands from: the `mpsc` receiver on the
/// engine thread, a scripted queue in tests.
pub trait CommandPoll {
    /// Next pending command, or `None` when the queue is empty. Never blocks.
    fn poll(&mut self) -> Option<EngineCommand>;
}

/// The live engine-thread command queue (`try_recv` never blocks).
impl CommandPoll for &mpsc::Receiver<EngineCommand> {
    fn poll(&mut self) -> Option<EngineCommand> {
        self.try_recv().ok()
    }
}

/// Command source that never yields (for waits outside playback).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoCommands;

impl CommandPoll for NoCommands {
    fn poll(&mut self) -> Option<EngineCommand> {
        None
    }
}

/// Something the engine can emit events to: the [`EventEmitter`] on the
/// engine thread, a recording `Vec` in tests.
pub trait EventSink {
    fn emit(&mut self, event: PlayerEvent);
}

/// Engine-side outbound end (A13): lossless events go to an unbounded
/// ordered channel; `Time` goes to a bounded channel with shed-on-full.
pub struct EventEmitter {
    lossless: mpsc::Sender<PlayerEvent>,
    time: mpsc::SyncSender<PlayerEvent>,
    time_dropped: Arc<AtomicU64>,
}

/// UI-side outbound end: plain `mpsc` receivers (T9 bridges these into an
/// iced subscription).
pub struct EngineEventReceiver {
    lossless: mpsc::Receiver<PlayerEvent>,
    time: mpsc::Receiver<PlayerEvent>,
    time_dropped: Arc<AtomicU64>,
}

/// Build the outbound event channel pair.
pub fn event_channel() -> (EventEmitter, EngineEventReceiver) {
    let (lossless_tx, lossless_rx) = mpsc::channel();
    let (time_tx, time_rx) = mpsc::sync_channel(TIME_CHANNEL_CAP);
    let time_dropped = Arc::new(AtomicU64::new(0));
    (
        EventEmitter {
            lossless: lossless_tx,
            time: time_tx,
            time_dropped: Arc::clone(&time_dropped),
        },
        EngineEventReceiver {
            lossless: lossless_rx,
            time: time_rx,
            time_dropped,
        },
    )
}

impl EventSink for EventEmitter {
    fn emit(&mut self, event: PlayerEvent) {
        if matches!(event, PlayerEvent::Time { .. }) {
            // Shed stale clock updates under backpressure; only a *full*
            // channel counts as shed (a hung-up receiver is shutdown, not
            // backpressure).
            if let Err(mpsc::TrySendError::Full(_)) = self.time.try_send(event) {
                self.time_dropped.fetch_add(1, Ordering::Relaxed);
            }
        } else if self.lossless.send(event).is_err() {
            // Receiver hung up (UI gone): nothing to do; the thread exits
            // on handle-drop / Shutdown.
        }
    }
}

impl EngineEventReceiver {
    /// Blocking receive of the next lossless event; `None` once the engine
    /// thread is gone.
    pub fn recv_lossless(&self) -> Option<PlayerEvent> {
        self.lossless.recv().ok()
    }

    /// Non-blocking drain of one lossless event.
    pub fn try_recv_lossless(&self) -> Option<PlayerEvent> {
        self.lossless.try_recv().ok()
    }

    /// Blocking receive of the next lossless event with a timeout; `None`
    /// on timeout or once the engine thread is gone.
    pub fn recv_lossless_timeout(&self, timeout: Duration) -> Option<PlayerEvent> {
        self.lossless.recv_timeout(timeout).ok()
    }

    /// Non-blocking drain of one `Time` event.
    pub fn try_recv_time(&self) -> Option<PlayerEvent> {
        self.time.try_recv().ok()
    }

    /// Drain all pending `Time` events and keep the latest (latest-wins for
    /// the UI frame clock); `None` when no `Time` is pending.
    pub fn drain_time_latest(&self) -> Option<PlayerEvent> {
        let mut latest = None;
        while let Ok(ev) = self.time.try_recv() {
            latest = Some(ev);
        }
        latest
    }

    /// How many `Time` events were shed to backpressure so far.
    pub fn time_dropped(&self) -> u64 {
        self.time_dropped.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Engine handle + thread spawn
// ---------------------------------------------------------------------------

/// UI-side handle to the engine thread: `Clone` + `Send`, infallible send
/// (a command sent after shutdown is a silent no-op, like a C++ call on a
/// stopped player).
#[derive(Debug, Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<EngineCommand>,
}

impl EngineHandle {
    pub fn send(&self, command: EngineCommand) {
        let _ = self.tx.send(command);
    }
}

/// Spawn the engine thread with the default output trio (ALSA, FluidSynth,
/// Dummy). The [`Engine`] is constructed inside the thread, so the outputs
/// never cross threads.
pub fn spawn_engine() -> (EngineHandle, EngineEventReceiver, thread::JoinHandle<()>) {
    spawn_engine_with(MidiOutputManager::new)
}

/// Spawn the engine thread with custom-built outputs (test seam: the factory
/// runs inside the thread, so only the factory itself must be `Send`).
pub fn spawn_engine_with(
    make_outputs: impl FnOnce() -> MidiOutputManager + Send + 'static,
) -> (EngineHandle, EngineEventReceiver, thread::JoinHandle<()>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (mut emitter, receiver) = event_channel();
    let join = thread::Builder::new()
        .name("goshmidi-engine".to_string())
        .spawn(move || {
            let mut engine = Engine::new(RealClock, make_outputs());
            engine.run(&cmd_rx, &mut emitter);
        })
        .expect("engine thread spawns");
    (EngineHandle { tx: cmd_tx }, receiver, join)
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// How a playback run ended (returned by [`Engine::run_playback`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackOutcome {
    /// Song exhausted: postlude emitted `Stopped` + `Finished`.
    Ended,
    /// `Stop`: postlude emitted `Stopped` (position reset first when the
    /// command asked for it).
    Stopped,
    /// `Pause`: postlude emitted `Stopped`, position kept for resume.
    Paused,
    /// `Load` arrived mid-playback: postlude ran, then the song loads.
    Load(PathBuf),
    /// `Shutdown`: the thread must exit after the postlude.
    Shutdown,
}

/// What one command asks the playback loop to do.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandEffect {
    Continue,
    Stop { reset: bool },
    Pause,
    Load(PathBuf),
    Shutdown,
}

/// Latched abort request inside a playback run. Priority is
/// Shutdown > Load > Pause > Stop; `Stop` clears a latched pause (C++
/// `stop()` clears `m_paused`), and a pause arriving after a stop/load is a
/// no-op (C++ `pause()` on a non-running player is a no-op).
#[derive(Debug, Default)]
struct AbortState {
    stopped: bool,
    reset: bool,
    paused: bool,
    load: Option<PathBuf>,
    shutdown: bool,
}

impl AbortState {
    fn aborted(&self) -> bool {
        self.stopped || self.paused || self.load.is_some() || self.shutdown
    }

    fn stop(&mut self, reset: bool) {
        self.stopped = true;
        self.paused = false;
        self.reset |= reset;
    }

    fn pause(&mut self) {
        if !self.stopped && !self.shutdown && self.load.is_none() {
            self.paused = true;
        }
    }

    fn load(&mut self, path: PathBuf) {
        self.load = Some(path);
        self.paused = false;
    }

    fn shutdown(&mut self) {
        self.shutdown = true;
    }

    fn outcome(&self) -> PlaybackOutcome {
        if self.shutdown {
            PlaybackOutcome::Shutdown
        } else if let Some(path) = self.load.clone() {
            PlaybackOutcome::Load(path)
        } else if self.paused {
            PlaybackOutcome::Paused
        } else if self.stopped {
            PlaybackOutcome::Stopped
        } else {
            PlaybackOutcome::Ended
        }
    }
}

/// The player engine: owns the [`Sequence`] and the [`MidiOutputManager`],
/// runs the scheduler. All state is owned by the engine thread; control
/// arrives as owned [`EngineCommand`]s (message passing only).
pub struct Engine<C: PlayerClock = RealClock> {
    clock: C,
    song: Sequence,
    outputs: MidiOutputManager,
    playing: bool,
    paused: bool,
    resume_without_reset: bool,
    position_ticks: i64,
    loop_enabled: bool,
    loop_start: i32,
    loop_end: i32,
    pitch_shift: i32,
    volume_factor: i32,
    drums_channel: i32,
    sysex_reset: i32,
    companion_syx: Option<PathBuf>,
    volume: [i32; MIDI_CHANNELS],
    last_pgm: [i32; MIDI_CHANNELS],
    locked_pgm: [i32; MIDI_CHANNELS],
    volume_shift: [f64; MIDI_CHANNELS],
    muted: [bool; MIDI_CHANNELS],
    locked: [bool; MIDI_CHANNELS],
    latest_beat: Option<usize>,
    first_beat: Option<usize>,
}

impl<C: PlayerClock> Engine<C> {
    pub fn new(clock: C, outputs: MidiOutputManager) -> Self {
        let mut engine = Self {
            clock,
            song: Sequence::default(),
            outputs,
            playing: false,
            paused: false,
            resume_without_reset: false,
            position_ticks: 0,
            loop_enabled: false,
            loop_start: 1,
            loop_end: 1,
            pitch_shift: 0,
            volume_factor: 100,
            drums_channel: super::event::GM_DRUM_CHANNEL,
            sysex_reset: 0,
            companion_syx: None,
            volume: [100; MIDI_CHANNELS],
            last_pgm: [0; MIDI_CHANNELS],
            locked_pgm: [0; MIDI_CHANNELS],
            volume_shift: [1.0; MIDI_CHANNELS],
            muted: [false; MIDI_CHANNELS],
            locked: [false; MIDI_CHANNELS],
            latest_beat: None,
            first_beat: None,
        };
        engine.init_channels();
        engine
    }

    /// Engine-thread main loop: block for commands while idle (immediate
    /// wakeup), run the scheduler for `Play`. Returns on `Shutdown` or once
    /// all handles are dropped.
    pub fn run(&mut self, rx: &mpsc::Receiver<EngineCommand>, events: &mut EventEmitter) {
        while let Ok(cmd) = rx.recv() {
            if matches!(cmd, EngineCommand::Shutdown) {
                break;
            }
            if matches!(cmd, EngineCommand::Play) {
                let mut cmds = rx;
                match self.play(&mut cmds, events) {
                    Some(PlaybackOutcome::Shutdown) => break,
                    Some(PlaybackOutcome::Load(path)) => self.load_song(&path, events),
                    _ => {}
                }
                continue;
            }
            match self.handle_command(cmd, events) {
                CommandEffect::Load(path) => self.load_song(&path, events),
                CommandEffect::Shutdown => break,
                CommandEffect::Stop { reset } => {
                    // Idle `stop()` still shuts up the output (player.cpp:107)
                    // and clears paused state unconditionally (player.cpp:109-110),
                    // so a later Play runs the full start sequence.
                    self.paused = false;
                    self.resume_without_reset = false;
                    self.all_notes_off();
                    if reset {
                        self.reset_position();
                    }
                }
                CommandEffect::Continue | CommandEffect::Pause => {}
            }
        }
    }

    /// C++ `play()`: no-op (`None`, no events) when already playing or the
    /// song is empty; otherwise runs the scheduler to stop/pause/finish.
    pub fn play(
        &mut self,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
    ) -> Option<PlaybackOutcome> {
        if self.playing || self.song.empty() {
            return None;
        }
        self.resume_without_reset = self.paused;
        self.paused = false;
        Some(self.run_playback(cmds, events))
    }

    /// The scheduler: 1:1 port of `playerLoop` (player.cpp:452-526) with
    /// interruptible waits (see module docs). `Started` is emitted even when
    /// the start sequence aborts (C++ emits it unconditionally); the postlude
    /// always sends all-notes-off + `Stopped`, plus `Finished` only when the
    /// song was exhausted without a stop request.
    pub fn run_playback(
        &mut self,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
    ) -> PlaybackOutcome {
        self.playing = true;
        let simple = self.song.simple_time_process();
        // NOTE: restarts at 0 on every play, including resume — the C++
        // local does the same (player.cpp:457).
        let mut echo_ticks: i64 = 0;
        let mut current_time = self.clock.now();
        let mut start_time = current_time;
        let mut abort = AbortState::default();

        if !self.resume_without_reset {
            self.start_sequence(cmds, events, &mut abort);
        }
        self.resume_without_reset = false;
        events.emit(PlayerEvent::Started);

        if !abort.aborted() {
            loop {
                while self.song.has_more_events() && !abort.aborted() {
                    // A2: commands are polled every event *and* every echo
                    // tick, so Stop stays responsive even during storms of
                    // zero-delta events (which never wait).
                    self.drain_commands(cmds, events, &mut abort);
                    if abort.aborted() {
                        break;
                    }
                    let Some(ev) = self.song.next_event().cloned() else {
                        break;
                    };
                    let ev_idx = self.song.pos - 1;
                    if ev.kind == Kind::Beat && self.loop_enabled && ev.a > self.loop_end {
                        break;
                    }
                    if ev.delta > 0 {
                        let next_time = if simple {
                            start_time + self.song.time_of_event(&ev)
                        } else {
                            current_time + self.song.delta_time_of_event(&ev)
                        };
                        let echo_delta = self.song.time_of_ticks(ECHO_RES_TICKS as u64);
                        // Zero-tempo guard (DEVIATION, see module docs): a
                        // zero echo quantum would busy-spin the echo loop.
                        if !echo_delta.is_zero() {
                            let mut next_echo = current_time + echo_delta;
                            while next_echo < next_time && !abort.aborted() {
                                self.sleep_until(next_echo);
                                echo_ticks += ECHO_RES_TICKS;
                                events.emit(time_event(&self.song, echo_ticks));
                                current_time = self.clock.now();
                                self.drain_commands(cmds, events, &mut abort);
                                next_echo = current_time + echo_delta;
                            }
                            if abort.aborted() {
                                break;
                            }
                        }
                        // A2: the C++ tail `sleep_until` is one
                        // uninterruptible wait; here it is chunked with
                        // command polls (same deadline, prompt preemption).
                        if !self.wait_until(next_time, echo_delta, cmds, events, &mut abort) {
                            break;
                        }
                        echo_ticks = ev.tick;
                        self.position_ticks = echo_ticks;
                        current_time = self.clock.now();
                        events.emit(time_event(&self.song, echo_ticks));
                    }
                    self.play_event(&ev, ev_idx, cmds, events, &mut abort);
                }
                if self.loop_enabled && !abort.aborted() {
                    self.jump_to_bar(self.loop_start);
                    current_time = self.clock.now();
                    start_time = current_time;
                    echo_ticks = self.position_ticks;
                }
                if !(self.song.has_more_events() && !abort.aborted()) {
                    break;
                }
            }
        }

        let finished = !self.song.has_more_events() && !abort.aborted();
        self.playing = false;
        self.all_notes_off();
        events.emit(PlayerEvent::Stopped);
        if finished {
            events.emit(PlayerEvent::Finished);
        }
        if abort.reset {
            self.reset_position();
        }
        if abort.paused {
            self.paused = true;
        }
        abort.outcome()
    }

    /// Apply one command immediately (live, even mid-playback — the C++
    /// setters are direct calls with the same immediacy) and report what
    /// the playback loop should do about it. `Load` is *not* performed
    /// here: the caller loads after the postlude, so `Stopped` precedes
    /// `Opened` exactly like the C++ `stop()`-then-load order.
    pub fn handle_command(
        &mut self,
        cmd: EngineCommand,
        events: &mut impl EventSink,
    ) -> CommandEffect {
        match cmd {
            EngineCommand::Load(path) => CommandEffect::Load(path),
            EngineCommand::Play => CommandEffect::Continue,
            EngineCommand::Pause => CommandEffect::Pause,
            EngineCommand::Stop { reset } => CommandEffect::Stop { reset },
            EngineCommand::Shutdown => CommandEffect::Shutdown,
            EngineCommand::ResetPosition => {
                self.reset_position();
                CommandEffect::Continue
            }
            EngineCommand::SeekTicks(ticks) => {
                self.all_notes_off();
                self.song.set_tick_position(ticks);
                self.position_ticks = ticks;
                CommandEffect::Continue
            }
            EngineCommand::JumpToBar(bar) => {
                self.jump_to_bar(i32::try_from(bar).unwrap_or(i32::MAX));
                CommandEffect::Continue
            }
            EngineCommand::BeatStep(dir) => {
                self.beat_step(dir);
                CommandEffect::Continue
            }
            EngineCommand::SetTempoFactor(factor) => {
                self.song.set_tempo_factor(factor);
                CommandEffect::Continue
            }
            EngineCommand::SetVolumeFactor(vol) => {
                self.volume_factor = vol.min(i32::MAX as u32) as i32;
                self.send_volume_events();
                CommandEffect::Continue
            }
            EngineCommand::SetPitchShift(pitch) => {
                self.all_notes_off();
                self.pitch_shift = pitch;
                CommandEffect::Continue
            }
            EngineCommand::SetChannelVolume(ch, factor) => {
                if let Some(ch) = channel_index(ch) {
                    self.volume_shift[ch] = factor;
                    let raw = self.volume[ch];
                    let shift = self.volume_shift[ch];
                    let master = self.volume_factor;
                    if let Some(out) = self.outputs.current_mut() {
                        let value = bounded_floor(raw, shift * f64::from(master) / 100.0);
                        out.send_controller(ch as i32, CC_VOLUME, value);
                    }
                }
                CommandEffect::Continue
            }
            EngineCommand::SetMuted(ch, mute) => {
                if let Some(ch) = channel_index(ch) {
                    self.muted[ch] = mute;
                    if mute && let Some(out) = self.outputs.current_mut() {
                        out.send_controller(ch as i32, CC_ALL_NOTES_OFF, 0);
                        out.send_controller(ch as i32, CC_ALL_SOUNDS_OFF, 0);
                    }
                }
                CommandEffect::Continue
            }
            EngineCommand::SetLocked(ch, lock) => {
                if let Some(ch) = channel_index(ch) {
                    self.locked[ch] = lock;
                    if lock {
                        self.locked_pgm[ch] = self.last_pgm[ch];
                    }
                }
                CommandEffect::Continue
            }
            EngineCommand::SetPatch(ch, patch) => {
                if let Some(ch) = channel_index(ch) {
                    let patch = i32::from(patch);
                    self.last_pgm[ch] = patch;
                    if self.locked[ch] {
                        self.locked_pgm[ch] = patch;
                    }
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_program(ch as i32, patch);
                    }
                }
                CommandEffect::Continue
            }
            EngineCommand::SetLoop {
                enabled,
                start,
                end,
            } => {
                self.loop_enabled = enabled;
                self.loop_start = i32::try_from(start).unwrap_or(i32::MAX);
                self.loop_end = i32::try_from(end).unwrap_or(i32::MAX);
                CommandEffect::Continue
            }
            EngineCommand::SetDrumsChannel(ch) => {
                self.drums_channel = i32::from(ch);
                CommandEffect::Continue
            }
            EngineCommand::SetSysexReset(kind) => {
                self.sysex_reset = i32::from(kind);
                CommandEffect::Continue
            }
            EngineCommand::SelectOutput { backend, port } => {
                let warning = self
                    .outputs
                    .select(&backend, &port)
                    .err()
                    .map(|e| e.message);
                let (name, current) = match self.outputs.current() {
                    Some(out) => (
                        out.backend_name().to_string(),
                        out.current_port().to_string(),
                    ),
                    None => (backend, String::new()),
                };
                events.emit(PlayerEvent::OutputChanged {
                    backend: name,
                    port: current,
                    warning,
                });
                CommandEffect::Continue
            }
            EngineCommand::SetSoundFont(path) => {
                self.outputs.set_sound_font(&path);
                CommandEffect::Continue
            }
            // A3 bypass (D19 refinement): straight to the output — no mute,
            // pitch, lock, or sequencing state is read or written, exactly
            // like the C++ direct `sendNoteOn/Off` calls. No output → no-op
            // (C++ guards on `m_outputs.current()` too).
            EngineCommand::PreviewNote { ch, note, vel } => {
                if let Some(out) = self.outputs.current_mut() {
                    out.send_note_on(i32::from(ch), i32::from(note), i32::from(vel));
                }
                CommandEffect::Continue
            }
            EngineCommand::PreviewOff { notes } => {
                if let Some(out) = self.outputs.current_mut() {
                    for (ch, note) in notes {
                        out.send_note_off(i32::from(ch), i32::from(note), 0);
                    }
                }
                CommandEffect::Continue
            }
        }
    }

    /// Apply every pending command; abort effects latch into `abort` but the
    /// queue is always fully drained (a `Seek` after a `Stop` still lands,
    /// like the equivalent C++ call sequence).
    fn drain_commands(
        &mut self,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) {
        while let Some(cmd) = cmds.poll() {
            match self.handle_command(cmd, events) {
                CommandEffect::Continue => {}
                CommandEffect::Stop { reset } => abort.stop(reset),
                CommandEffect::Pause => abort.pause(),
                CommandEffect::Load(path) => abort.load(path),
                CommandEffect::Shutdown => abort.shutdown(),
            }
        }
    }

    /// Sleep until an absolute instant in one chunk (echo-tick granularity:
    /// the caller polls commands right after).
    fn sleep_until(&mut self, deadline: Instant) {
        let now = self.clock.now();
        if deadline > now {
            self.clock.sleep(deadline - now);
        }
    }

    /// Sleep until `deadline` in `quantum`-sized chunks (a zero quantum
    /// means one chunk), polling commands between chunks. Returns `false`
    /// when an aborting command arrived (the caller must skip the pending
    /// event — deliberate A2 improvement over C++, see module docs).
    fn wait_until(
        &mut self,
        deadline: Instant,
        quantum: Duration,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) -> bool {
        loop {
            if abort.aborted() {
                return false;
            }
            let now = self.clock.now();
            if now >= deadline {
                return true;
            }
            let remaining = deadline - now;
            let chunk = if quantum.is_zero() {
                remaining
            } else {
                remaining.min(quantum)
            };
            self.clock.sleep(chunk);
            self.drain_commands(cmds, events, abort);
        }
    }

    /// Chunked millisecond sleep with command polls for SysEx pacing and
    /// reset settle. Returns `false` on abort.
    fn sleep_ms(
        &mut self,
        ms: u64,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) -> bool {
        if ms == 0 {
            return !abort.aborted();
        }
        self.wait_until(
            self.clock.now() + Duration::from_millis(ms),
            Duration::from_millis(PACING_CHUNK_MS),
            cmds,
            events,
            abort,
        )
    }

    /// 1:1 port of `playEvent` (player.cpp:342-450). With no output
    /// selected the whole function is a no-op — including skipped tempo
    /// updates — exactly like the C++ null-`m_out` early return.
    fn play_event(
        &mut self,
        ev: &MidiEvent,
        ev_idx: usize,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) {
        if self.outputs.current().is_none() {
            return;
        }
        if ev.is_channel() {
            let channel = ev.channel;
            if !(0..MIDI_CHANNELS as i32).contains(&channel) {
                return;
            }
            let ch = channel as usize;
            if self.muted[ch] {
                return;
            }
            // Pitch shift applies to every channel except drums; the shifted
            // key goes to the output unclamped (1:1) and clamped in events.
            let shifted_key = if channel == self.drums_channel {
                ev.a
            } else {
                ev.a + self.pitch_shift
            };
            match ev.kind {
                Kind::NoteOff => {
                    let key = shifted_key;
                    let vel = ev.b;
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_note_off(channel, key, vel);
                    }
                    events.emit(PlayerEvent::NoteOff {
                        ch: channel as u8,
                        note: clamp_u8(key),
                        vel: clamp_u8(vel),
                    });
                }
                Kind::NoteOn => {
                    let key = shifted_key;
                    let vel = ev.b;
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_note_on(channel, key, vel);
                    }
                    events.emit(PlayerEvent::NoteOn {
                        ch: channel as u8,
                        note: clamp_u8(key),
                        vel: clamp_u8(vel),
                    });
                }
                Kind::KeyPressure => {
                    let key = shifted_key;
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_key_pressure(channel, key, ev.b);
                    }
                }
                Kind::ControlChange => {
                    let mut val = ev.b;
                    if ev.a == CC_VOLUME {
                        self.volume[ch] = val;
                        val = bounded_floor(
                            val,
                            self.volume_shift[ch] * f64::from(self.volume_factor) / 100.0,
                        );
                    }
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_controller(channel, ev.a, val);
                    }
                }
                Kind::ProgramChange => {
                    let pgm = if self.locked[ch] {
                        self.locked_pgm[ch]
                    } else {
                        ev.a
                    };
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_program(channel, pgm);
                    }
                    self.last_pgm[ch] = pgm;
                    events.emit(PlayerEvent::Program {
                        ch: channel as u8,
                        patch: clamp_u8(pgm),
                    });
                }
                Kind::ChannelPressure => {
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_channel_pressure(channel, ev.a);
                    }
                }
                Kind::PitchBend => {
                    if let Some(out) = self.outputs.current_mut() {
                        out.send_pitch_bend(channel, ev.a);
                    }
                }
                _ => {}
            }
        } else {
            match ev.kind {
                Kind::SysEx => self.send_sysex_paced(&ev.data, cmds, events, abort),
                Kind::Text => events.emit(PlayerEvent::Text {
                    track: u32_from_i32(ev.tag),
                    text_type: ev.a,
                    ticks: ev.tick,
                    bytes: ev.data.clone(),
                }),
                Kind::Tempo => {
                    self.song.update_tempo(ev.tempo);
                    events.emit(PlayerEvent::Tempo {
                        tempo_us: self.song.current_tempo(),
                    });
                }
                Kind::Beat => {
                    self.latest_beat = Some(ev_idx);
                    events.emit(PlayerEvent::Beat {
                        bar: u32_from_i32(ev.a),
                        beat: u32_from_i32(ev.b),
                        max: u32_from_i32(ev.c),
                    });
                }
                Kind::TimeSignature => events.emit(PlayerEvent::TimeSignature {
                    bar: u32_from_i32(ev.tag),
                    num: clamp_u8(ev.a),
                    den: clamp_u8(ev.b),
                }),
                Kind::KeySignature => events.emit(PlayerEvent::KeySignature {
                    track: u32_from_i32(ev.tag),
                    alterations: ev.a.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8,
                    minor: ev.c != 0,
                }),
                _ => {}
            }
        }
    }

    /// Fresh-start sequence (player.cpp:461-466): reset SysEx + settle →
    /// companion `.syx` + pacing → reset-controllers → volume events.
    /// Aborting commands cut it short (A2 improvement over C++, which runs
    /// the whole sequence uninterruptibly).
    fn start_sequence(
        &mut self,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) {
        if !self.send_reset_message(cmds, events, abort) {
            return;
        }
        if !self.send_companion_sysex(cmds, events, abort) {
            return;
        }
        self.reset_controllers();
        self.send_volume_events();
    }

    /// C++ `loadFile` (player.cpp:63-78): load + companion lookup + cursor /
    /// loop / channel reset, then `Opened` or `LoadFailed`.
    fn load_song(&mut self, path: &Path, events: &mut impl EventSink) {
        self.playing = false;
        self.paused = false;
        self.resume_without_reset = false;
        let ok = self.song.load_file(path);
        self.companion_syx = companion_syx_path(path);
        self.position_ticks = 0;
        self.first_beat = self.song.first_beat();
        self.latest_beat = self.first_beat;
        self.loop_start = 1;
        self.loop_end = self.song.last_bar();
        self.loop_enabled = false;
        self.init_channels();
        if ok {
            events.emit(PlayerEvent::Opened(Box::new(self.snapshot())));
        } else {
            events.emit(PlayerEvent::LoadFailed {
                file: path.to_string_lossy().into_owned(),
                errors: self.song.loading_errors.clone(),
            });
        }
    }

    fn snapshot(&self) -> SongSnapshot {
        let len = self.song.song_length_ticks().max(0);
        SongSnapshot {
            title: self.song.current_file(),
            format: self.song.file_format().to_string(),
            bars: self.song.last_bar(),
            len_ticks: i64::from(len),
            duration_ms: self.song.time_of_ticks(len as u64).as_millis() as u64,
            division: self.song.division(),
            num_tracks: self.song.num_tracks(),
            initial_tempo_us: self.song.initial_tempo(),
            channel_used: std::array::from_fn(|ch| self.song.channel_used(ch as i32)),
            channel_labels: std::array::from_fn(|ch| self.song.channel_label(ch as i32)),
            lowest_note: self.song.lowest_note(),
            highest_note: self.song.highest_note(),
            charset: self.song.current_charset().to_string(),
            texts: self.song.text_events().to_vec(),
            metadata: self
                .song
                .info_map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            copyrights: self.song.get_text(TextType::Copyright),
        }
    }

    fn init_channels(&mut self) {
        self.volume = [100; MIDI_CHANNELS];
        self.last_pgm = [0; MIDI_CHANNELS];
        self.locked_pgm = [0; MIDI_CHANNELS];
        self.volume_shift = [1.0; MIDI_CHANNELS];
        self.muted = [false; MIDI_CHANNELS];
        self.locked = [false; MIDI_CHANNELS];
    }

    /// A1: cursor + ticks + beat cursor reset (no all-notes-off — callers
    /// stop first, like the C++ repeat-song / stop-with-reset paths).
    fn reset_position(&mut self) {
        if !self.song.empty() {
            self.song.reset_position();
            self.position_ticks = 0;
            self.latest_beat = self.first_beat;
        }
    }

    fn jump_to_bar(&mut self, bar: i32) {
        self.all_notes_off();
        if let Some(idx) = self.song.jump_to_bar(bar) {
            self.latest_beat = Some(idx);
            let tick = self.song.events()[idx].tick;
            self.position_ticks = tick;
            // Redundant with `jump_to_bar` (as in C++), kept 1:1.
            self.song.set_tick_position(tick);
        }
    }

    fn beat_step(&mut self, dir: i8) {
        let target = if dir > 0 {
            self.song.next_bar(self.latest_beat)
        } else if dir < 0 {
            self.song.previous_bar(self.latest_beat)
        } else {
            return;
        };
        if let Some(idx) = target {
            let bar = self.song.events()[idx].a;
            self.jump_to_bar(bar);
        }
    }

    fn all_notes_off(&mut self) {
        let Some(out) = self.outputs.current_mut() else {
            return;
        };
        for ch in 0..MIDI_CHANNELS as i32 {
            out.send_controller(ch, CC_ALL_NOTES_OFF, 0);
            out.send_controller(ch, CC_ALL_SOUNDS_OFF, 0);
        }
    }

    fn reset_controllers(&mut self) {
        let Some(out) = self.outputs.current_mut() else {
            return;
        };
        for ch in 0..MIDI_CHANNELS as i32 {
            out.send_controller(ch, CC_RESET_CONTROLLERS, 0);
            // NOTE: literal 100, unscaled (player.cpp:265) — unlike
            // `send_volume_events` below.
            out.send_controller(ch, CC_VOLUME, 100);
        }
    }

    /// C++ `resetPrograms` (not part of the start sequence): locked channels
    /// get their locked patch, the rest program 0.
    fn reset_programs(&mut self) {
        let Some(out) = self.outputs.current_mut() else {
            return;
        };
        for ch in 0..MIDI_CHANNELS {
            let pgm = if self.locked[ch] {
                self.locked_pgm[ch]
            } else {
                0
            };
            out.send_program(ch as i32, pgm);
        }
    }

    /// A9: resets the stored raw volume to 100 on every channel, then sends
    /// the scaled CC7.
    fn send_volume_events(&mut self) {
        let Some(out) = self.outputs.current_mut() else {
            return;
        };
        for ch in 0..MIDI_CHANNELS {
            self.volume[ch] = 100;
            let value = bounded_floor(
                100,
                self.volume_shift[ch] * f64::from(self.volume_factor) / 100.0,
            );
            out.send_controller(ch as i32, CC_VOLUME, value);
        }
    }

    /// C++ `sendSysexPaced` (player.cpp:279-294) with abort checks between
    /// messages and chunked pacing sleeps (A2). An unparseable blob is sent
    /// whole, with no pacing (C++ 1:1).
    fn send_sysex_paced(
        &mut self,
        data: &[u8],
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) {
        if self.outputs.current().is_none() || data.is_empty() {
            return;
        }
        let msgs = parse_sysex_messages(data);
        if msgs.is_empty() {
            if let Some(out) = self.outputs.current_mut() {
                out.send_sysex(data);
            }
            return;
        }
        for msg in &msgs {
            if abort.aborted() {
                return;
            }
            if let Some(out) = self.outputs.current_mut() {
                out.send_sysex(msg);
            }
            let wait = sysex_pacing_ms(msg.len());
            if wait > 0 && !self.sleep_ms(wait, cmds, events, abort) {
                return;
            }
        }
    }

    /// C++ `sendResetMessage` (player.cpp:296-307): empty message (kind none
    /// / unknown) sends nothing and skips the settle sleep. Returns `false`
    /// on abort.
    fn send_reset_message(
        &mut self,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) -> bool {
        if self.outputs.current().is_none() {
            return true;
        }
        let msg = sysex_reset_message(self.sysex_reset);
        if msg.is_empty() {
            return true;
        }
        if let Some(out) = self.outputs.current_mut() {
            out.send_sysex(&msg);
        }
        let wait = sysex_reset_settle_ms(self.sysex_reset);
        if wait > 0 {
            self.sleep_ms(wait, cmds, events, abort)
        } else {
            !abort.aborted()
        }
    }

    /// C++ `sendCompanionSysex` (player.cpp:309-319): each message + pacing
    /// of at least 15 ms, abortable between messages (A2). Returns `false`
    /// on abort.
    fn send_companion_sysex(
        &mut self,
        cmds: &mut impl CommandPoll,
        events: &mut impl EventSink,
        abort: &mut AbortState,
    ) -> bool {
        let Some(path) = self.companion_syx.clone() else {
            return true;
        };
        if self.outputs.current().is_none() {
            return true;
        }
        let msgs = load_sysex_file(&path);
        for msg in &msgs {
            if abort.aborted() {
                return false;
            }
            if let Some(out) = self.outputs.current_mut() {
                out.send_sysex(msg);
            }
            let wait = sysex_pacing_ms(msg.len()).max(COMPANION_MIN_PACING_MS);
            if !self.sleep_ms(wait, cmds, events, abort) {
                return false;
            }
        }
        true
    }

    // --- accessors (C++ getter parity) ---

    pub fn position(&self) -> i64 {
        self.position_ticks
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn loop_enabled(&self) -> bool {
        self.loop_enabled
    }

    pub fn pitch_shift(&self) -> i32 {
        self.pitch_shift
    }

    pub fn volume_factor(&self) -> i32 {
        self.volume_factor
    }

    pub fn volume(&self, channel: i32) -> f64 {
        if (0..MIDI_CHANNELS as i32).contains(&channel) {
            self.volume_shift[channel as usize]
        } else {
            1.0
        }
    }

    pub fn is_muted(&self, channel: i32) -> bool {
        (0..MIDI_CHANNELS as i32).contains(&channel) && self.muted[channel as usize]
    }

    pub fn is_locked(&self, channel: i32) -> bool {
        (0..MIDI_CHANNELS as i32).contains(&channel) && self.locked[channel as usize]
    }

    pub fn drums_channel(&self) -> i32 {
        self.drums_channel
    }

    pub fn sysex_reset(&self) -> i32 {
        self.sysex_reset
    }

    pub fn current_bpm(&self) -> f64 {
        tempo_to_bpm(self.song.current_tempo())
    }

    pub fn initial_bpm(&self) -> f64 {
        tempo_to_bpm(self.song.initial_tempo())
    }

    #[cfg(test)]
    pub fn install_song(&mut self, song: Sequence) {
        self.song = song;
        self.song.reset_position();
        self.position_ticks = 0;
        self.first_beat = self.song.first_beat();
        self.latest_beat = self.first_beat;
    }

    #[cfg(test)]
    pub fn set_companion(&mut self, path: Option<PathBuf>) {
        self.companion_syx = path;
    }

    #[cfg(test)]
    pub fn clock(&self) -> &C {
        &self.clock
    }

    #[cfg(test)]
    pub fn song(&self) -> &Sequence {
        &self.song
    }
}

/// `floor(initial * factor)` clamped to 0..=127 (C++ `boundedFloor`).
fn bounded_floor(initial: i32, factor: f64) -> i32 {
    clamp_midi(f64::floor(f64::from(initial) * factor) as i32)
}

/// Validated 0-based channel index for `u8` command channels.
fn channel_index(ch: u8) -> Option<usize> {
    let idx = usize::from(ch);
    (idx < MIDI_CHANNELS).then_some(idx)
}

/// Lossy `i32` → `u8` for event payloads (negative → 0, over 127 → 127).
fn clamp_u8(value: i32) -> u8 {
    value.clamp(0, 127) as u8
}

/// Lossy `i32` → `u32` for event payloads (negative → 0).
fn u32_from_i32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// `Time` event for an echo-tick position (player.cpp:491-494, 504-507).
fn time_event(song: &Sequence, echo_ticks: i64) -> PlayerEvent {
    PlayerEvent::Time {
        elapsed_ms: song.time_of_ticks(echo_ticks.max(0) as u64).as_millis() as u64,
        ticks: echo_ticks,
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use crate::core::event::{
        CC_ALL_NOTES_OFF, CC_ALL_SOUNDS_OFF, CC_RESET_CONTROLLERS, GM_DRUM_CHANNEL, MidiEvent,
        make_beat, make_control, make_note_off, make_note_on, make_program, make_tempo,
    };
    use crate::core::output::{DummyCall, MidiOutput, MidiPort, OutputError};

    // --- fake clock: sleeps advance instantly, fully deterministic ---

    #[derive(Debug)]
    struct FakeClock {
        now: Instant,
        slept: Duration,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Instant::now(),
                slept: Duration::ZERO,
            }
        }

        fn elapsed(&self) -> Duration {
            self.slept
        }
    }

    impl PlayerClock for FakeClock {
        fn now(&self) -> Instant {
            self.now
        }

        fn sleep(&mut self, duration: Duration) {
            self.now += duration;
            self.slept += duration;
        }
    }

    impl CommandPoll for VecDeque<EngineCommand> {
        fn poll(&mut self) -> Option<EngineCommand> {
            self.pop_front()
        }
    }

    impl EventSink for Vec<PlayerEvent> {
        fn emit(&mut self, event: PlayerEvent) {
            self.push(event);
        }
    }

    // --- shared-recording output: DummyCall log readable across the engine ---

    #[derive(Debug, Clone)]
    struct SharedRecorder {
        calls: Arc<Mutex<Vec<DummyCall>>>,
        current: String,
    }

    impl SharedRecorder {
        fn new() -> (Self, Arc<Mutex<Vec<DummyCall>>>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    calls: Arc::clone(&calls),
                    current: String::new(),
                },
                calls,
            )
        }

        fn from_arc(calls: Arc<Mutex<Vec<DummyCall>>>) -> Self {
            Self {
                calls,
                current: String::new(),
            }
        }

        fn push(&self, call: DummyCall) {
            self.calls.lock().unwrap().push(call);
        }
    }

    impl MidiOutput for SharedRecorder {
        fn backend_name(&self) -> &str {
            "Recorder"
        }

        fn ports(&self, _advanced: bool) -> Vec<MidiPort> {
            vec![MidiPort {
                id: "rec:0".to_string(),
                label: "Recorder".to_string(),
            }]
        }

        fn open(&mut self, port_id: &str) -> Result<(), OutputError> {
            self.push(DummyCall::Open(port_id.to_string()));
            self.current = port_id.to_string();
            Ok(())
        }

        fn close(&mut self) {
            self.push(DummyCall::Close);
            self.current.clear();
        }

        fn current_port(&self) -> &str {
            &self.current
        }

        fn send_note_on(&mut self, ch: i32, note: i32, vel: i32) {
            self.push(DummyCall::NoteOn { ch, note, vel });
        }

        fn send_note_off(&mut self, ch: i32, note: i32, vel: i32) {
            self.push(DummyCall::NoteOff { ch, note, vel });
        }

        fn send_key_pressure(&mut self, ch: i32, note: i32, vel: i32) {
            self.push(DummyCall::KeyPressure { ch, note, vel });
        }

        fn send_controller(&mut self, ch: i32, cc: i32, val: i32) {
            self.push(DummyCall::Controller { ch, cc, val });
        }

        fn send_program(&mut self, ch: i32, pgm: i32) {
            self.push(DummyCall::Program { ch, pgm });
        }

        fn send_channel_pressure(&mut self, ch: i32, val: i32) {
            self.push(DummyCall::ChannelPressure { ch, val });
        }

        fn send_pitch_bend(&mut self, ch: i32, value: i32) {
            self.push(DummyCall::PitchBend { ch, value });
        }

        fn send_sysex(&mut self, data: &[u8]) {
            self.push(DummyCall::SysEx(data.to_vec()));
        }
    }

    /// Engine with `FakeClock` and the recorder backend selected; the `Open`
    /// call is cleared so tests observe only engine traffic.
    fn test_engine() -> (Engine<FakeClock>, Arc<Mutex<Vec<DummyCall>>>) {
        let (rec, calls) = SharedRecorder::new();
        let mut manager = MidiOutputManager::new();
        manager.add_backend(Box::new(rec));
        let mut engine = Engine::new(FakeClock::new(), manager);
        let mut events = Vec::new();
        engine.handle_command(
            EngineCommand::SelectOutput {
                backend: "Recorder".to_string(),
                port: "rec:0".to_string(),
            },
            &mut events,
        );
        calls.lock().unwrap().clear();
        (engine, calls)
    }

    fn calls_of(calls: &Arc<Mutex<Vec<DummyCall>>>) -> Vec<DummyCall> {
        calls.lock().unwrap().clone()
    }

    fn run_cmd(engine: &mut Engine<FakeClock>, cmd: EngineCommand) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        engine.handle_command(cmd, &mut events);
        events
    }

    // --- T13c preview commands (A3 bypass, D19 refinement) ---

    #[test]
    fn preview_note_writes_straight_to_output_bypassing_mix() {
        let (mut engine, calls) = test_engine();
        // Mute channel 0 and shift pitch: the bypass must ignore both.
        run_cmd(&mut engine, EngineCommand::SetMuted(0, true));
        run_cmd(&mut engine, EngineCommand::SetPitchShift(5));
        calls.lock().unwrap().clear();
        let events = run_cmd(
            &mut engine,
            EngineCommand::PreviewNote {
                ch: 0,
                note: 60,
                vel: 80,
            },
        );
        assert!(events.is_empty());
        assert_eq!(
            calls_of(&calls),
            vec![DummyCall::NoteOn {
                ch: 0,
                note: 60,
                vel: 80
            }]
        );
        // Mix state untouched by the bypass (mute/pitch still hold).
        assert!(engine.muted[0]);
        assert_eq!(engine.pitch_shift, 5);
    }

    #[test]
    fn preview_off_sends_every_note_vel_0() {
        let (mut engine, calls) = test_engine();
        run_cmd(
            &mut engine,
            EngineCommand::PreviewOff {
                notes: vec![(0, 60), (1, 64)],
            },
        );
        assert_eq!(
            calls_of(&calls),
            vec![
                DummyCall::NoteOff {
                    ch: 0,
                    note: 60,
                    vel: 0
                },
                DummyCall::NoteOff {
                    ch: 1,
                    note: 64,
                    vel: 0
                },
            ]
        );
        // Empty list sends nothing (C++ empty-loop parity).
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::PreviewOff { notes: vec![] });
        assert!(calls_of(&calls).is_empty());
    }

    #[test]
    fn preview_commands_noop_without_live_output() {
        // No `SelectOutput`: `current()` is None, like the C++ null guard.
        let (rec, calls) = SharedRecorder::new();
        let mut manager = MidiOutputManager::new();
        manager.add_backend(Box::new(rec));
        let mut engine = Engine::new(FakeClock::new(), manager);
        run_cmd(
            &mut engine,
            EngineCommand::PreviewNote {
                ch: 0,
                note: 60,
                vel: 96,
            },
        );
        run_cmd(
            &mut engine,
            EngineCommand::PreviewOff {
                notes: vec![(0, 60)],
            },
        );
        assert!(
            calls_of(&calls).is_empty(),
            "no output → no traffic, got {:?}",
            calls_of(&calls)
        );
    }

    #[test]
    fn snapshot_carries_d6_metadata_and_copyrights() {
        let (engine, _calls) = test_engine();
        // Empty engine song: no metadata, no copyrights (shape pin — the
        // RMI/SMF decoders fill `info_map`/`text_events` upstream).
        let snap = engine.snapshot();
        assert!(snap.metadata.is_empty());
        assert!(snap.copyrights.is_empty());
    }

    // --- song fixtures: division 120, live ticks2micros, hand-set delta ---

    fn song_base() -> Sequence {
        let mut seq = Sequence::default();
        seq.division = 120;
        seq.set_tempo_factor(1.0);
        seq
    }

    fn at_tick(mut ev: MidiEvent, tick: i64, delta: i64) -> MidiEvent {
        ev.tick = tick;
        ev.delta = delta;
        ev
    }

    fn text_event(tick: i64, track: i32, text_type: i32, bytes: &[u8]) -> MidiEvent {
        MidiEvent {
            kind: Kind::Text,
            tick,
            tag: track,
            a: text_type,
            data: bytes.to_vec(),
            ..Default::default()
        }
    }

    // --- adaptive command harness: Stop once N target events observed ---
    // (split halves sharing trip state — `play()` takes two `&mut`s).

    #[derive(Debug)]
    struct TripState {
        count: usize,
        limit: usize,
        tripped: bool,
        fired: bool,
        fire: EngineCommand,
    }

    impl Default for TripState {
        fn default() -> Self {
            Self {
                count: 0,
                limit: 0,
                tripped: false,
                fired: false,
                fire: EngineCommand::Stop { reset: false },
            }
        }
    }

    struct TripCmd {
        state: Arc<Mutex<TripState>>,
    }

    struct TripSink {
        state: Arc<Mutex<TripState>>,
        events: Vec<PlayerEvent>,
        is_target: fn(&PlayerEvent) -> bool,
    }

    fn trip_pair(
        limit: usize,
        is_target: fn(&PlayerEvent) -> bool,
        fire: EngineCommand,
    ) -> (TripCmd, TripSink) {
        let state = Arc::new(Mutex::new(TripState {
            limit,
            fire,
            ..Default::default()
        }));
        (
            TripCmd {
                state: Arc::clone(&state),
            },
            TripSink {
                state,
                events: Vec::new(),
                is_target,
            },
        )
    }

    impl CommandPoll for TripCmd {
        fn poll(&mut self) -> Option<EngineCommand> {
            let mut st = self.state.lock().unwrap();
            if st.tripped && !st.fired {
                st.fired = true;
                return Some(st.fire.clone());
            }
            None
        }
    }

    impl EventSink for TripSink {
        fn emit(&mut self, event: PlayerEvent) {
            if (self.is_target)(&event) {
                let mut st = self.state.lock().unwrap();
                st.count += 1;
                if st.count >= st.limit {
                    st.tripped = true;
                }
            }
            self.events.push(event);
        }
    }

    fn is_beat(ev: &PlayerEvent) -> bool {
        matches!(ev, PlayerEvent::Beat { .. })
    }

    fn is_time(ev: &PlayerEvent) -> bool {
        matches!(ev, PlayerEvent::Time { .. })
    }

    fn is_note_on(ev: &PlayerEvent) -> bool {
        matches!(ev, PlayerEvent::NoteOn { .. })
    }

    // --- start sequence, scheduler, end-of-song ---

    #[test]
    fn start_sequence_order_recorded() {
        // reset SysEx → companion → CC79+CC7=100 ×16 → scaled CC7 ×16 →
        // notes → postlude offs. Plus the exact event stream and the exact
        // fake-clock cost (40 ms settle + 15 ms companion pacing + song).
        let (mut engine, calls) = test_engine();
        let dir = tempfile::tempdir().unwrap();
        let syx = dir.path().join("c.syx");
        std::fs::write(&syx, [0xF0, 0x41, 0xF7]).unwrap();
        engine.set_companion(Some(syx));
        run_cmd(&mut engine, EngineCommand::SetSysexReset(1)); // GM: 40 ms
        let mut song = song_base();
        song.events.push(at_tick(make_beat(1, 1, 4), 0, 0));
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events
            .push(at_tick(make_note_off(0, 60, 64), 120, 120));
        engine.install_song(song);

        let mut cmds = VecDeque::new();
        let mut events = Vec::new();
        let outcome = engine.play(&mut cmds, &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));

        let mut expected = vec![
            DummyCall::SysEx(vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7]),
            DummyCall::SysEx(vec![0xF0, 0x41, 0xF7]),
        ];
        for ch in 0..16 {
            expected.push(DummyCall::Controller {
                ch,
                cc: CC_RESET_CONTROLLERS,
                val: 0,
            });
            expected.push(DummyCall::Controller {
                ch,
                cc: CC_VOLUME,
                val: 100,
            });
        }
        for ch in 0..16 {
            expected.push(DummyCall::Controller {
                ch,
                cc: CC_VOLUME,
                val: 100,
            });
        }
        expected.push(DummyCall::NoteOn {
            ch: 0,
            note: 60,
            vel: 80,
        });
        expected.push(DummyCall::NoteOff {
            ch: 0,
            note: 60,
            vel: 64,
        });
        for ch in 0..16 {
            expected.push(DummyCall::Controller {
                ch,
                cc: CC_ALL_NOTES_OFF,
                val: 0,
            });
            expected.push(DummyCall::Controller {
                ch,
                cc: CC_ALL_SOUNDS_OFF,
                val: 0,
            });
        }
        assert_eq!(calls_of(&calls), expected);

        assert_eq!(
            events,
            vec![
                PlayerEvent::Started,
                PlayerEvent::Beat {
                    bar: 1,
                    beat: 1,
                    max: 4
                },
                PlayerEvent::NoteOn {
                    ch: 0,
                    note: 60,
                    vel: 80
                },
                PlayerEvent::Time {
                    elapsed_ms: 208,
                    ticks: 50
                },
                PlayerEvent::Time {
                    elapsed_ms: 416,
                    ticks: 100
                },
                PlayerEvent::Time {
                    elapsed_ms: 500,
                    ticks: 120
                },
                PlayerEvent::NoteOff {
                    ch: 0,
                    note: 60,
                    vel: 64
                },
                PlayerEvent::Stopped,
                PlayerEvent::Finished,
            ]
        );
        // The absolute schedule absorbs the start cost (C++ 1:1: startTime
        // is captured *before* the start sequence, player.cpp:459-466), so
        // the total equals the last tick's time, not 55 ms + song time.
        assert_eq!(engine.clock().elapsed(), engine.song().time_of_ticks(120));
        assert_eq!(engine.position(), 120);
        assert!(!engine.is_playing());
    }

    #[test]
    fn start_sequence_consumes_absolute_schedule_budget() {
        // A 490 ms start (40 ms settle + 30 × 15 ms companion pacing) eats
        // the echo budget of the first 500 ms wait: the two real echoes
        // collapse to a single zero-cost phantom (the stale `current_time`
        // passes the first echo check, `sleep_until(past)` no-ops, but the
        // emit still fires — exactly like the C++ loop, whose `currentTime`
        // is equally stale after the start sequence). The total still
        // equals the last tick's time (see `start_sequence_order_recorded`).
        let (mut engine, calls) = test_engine();
        let dir = tempfile::tempdir().unwrap();
        let syx = dir.path().join("big.syx");
        let mut blob = Vec::new();
        for _ in 0..30 {
            blob.extend_from_slice(&[0xF0, 0x41, 0xF7]);
        }
        std::fs::write(&syx, &blob).unwrap();
        engine.set_companion(Some(syx));
        run_cmd(&mut engine, EngineCommand::SetSysexReset(1));
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events
            .push(at_tick(make_note_off(0, 60, 64), 120, 120));
        engine.install_song(song);
        let mut events = Vec::new();
        let outcome = engine.play(&mut VecDeque::new(), &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));
        let sysex_count = calls_of(&calls)
            .iter()
            .filter(|c| matches!(c, DummyCall::SysEx(_)))
            .count();
        assert_eq!(sysex_count, 31); // reset + 30 companion messages
        let times: Vec<i64> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Time { ticks, .. } => Some(*ticks),
                _ => None,
            })
            .collect();
        assert_eq!(times, vec![50, 120]);
        assert_eq!(engine.clock().elapsed(), engine.song().time_of_ticks(120));
    }

    #[test]
    fn simple_schedule_echo_times_are_exact() {
        // Absolute deadlines: gap 0→120 echoes at 50/100 then the tail lands
        // exactly on 120; the short 120→130 gap gets no echo, just the tail.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(1, 64, 90), 120, 120));
        song.events.push(at_tick(make_note_off(0, 60, 64), 130, 10));
        engine.install_song(song);
        assert!(engine.song().simple_time_process());

        let mut cmds = NoCommands;
        let mut events = Vec::new();
        let outcome = engine.play(&mut cmds, &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));
        let times: Vec<(u64, i64)> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Time { elapsed_ms, ticks } => Some((*elapsed_ms, *ticks)),
                _ => None,
            })
            .collect();
        assert_eq!(times, vec![(208, 50), (416, 100), (500, 120), (541, 130)]);
        // Absolute schedule: total fake time equals the last tick's time.
        assert_eq!(engine.clock().elapsed(), engine.song().time_of_ticks(130));
        assert_eq!(events.last(), Some(&PlayerEvent::Finished));
    }

    #[test]
    fn multi_tempo_schedule_is_incremental() {
        // Tempos chosen for exact integer tick math (480000/120 = 4000 µs,
        // 240000/120 = 2000 µs): waits are 480000 + 480000 + 240000 µs.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.num_tempo_changes = 2;
        song.events.push(at_tick(make_tempo(480_000.0), 0, 0));
        song.events.push(at_tick(make_note_on(0, 60, 80), 120, 120));
        song.events.push(at_tick(make_tempo(240_000.0), 240, 120));
        song.events.push(at_tick(make_note_on(0, 62, 80), 360, 120));
        engine.install_song(song);
        assert!(!engine.song().simple_time_process());

        let mut cmds = VecDeque::new();
        let mut events = Vec::new();
        let outcome = engine.play(&mut cmds, &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));
        assert_eq!(engine.clock().elapsed(), Duration::from_micros(1_200_000));
        let tempos: Vec<f64> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Tempo { tempo_us } => Some(*tempo_us),
                _ => None,
            })
            .collect();
        assert_eq!(tempos.len(), 2);
        assert!((tempos[0] - 480_000.0).abs() < f64::EPSILON);
        assert!((tempos[1] - 240_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn play_empty_song_is_silent_noop() {
        let (mut engine, calls) = test_engine();
        let mut cmds = VecDeque::new();
        let mut events = Vec::new();
        assert_eq!(engine.play(&mut cmds, &mut events), None);
        assert!(events.is_empty());
        assert!(calls_of(&calls).is_empty());
    }

    #[test]
    fn end_of_song_matrix() {
        // (a) Exhaustion → Stopped + Finished.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        engine.install_song(song);
        let mut events = Vec::new();
        assert_eq!(
            engine.play(&mut VecDeque::new(), &mut events),
            Some(PlaybackOutcome::Ended)
        );
        assert_eq!(
            events,
            vec![
                PlayerEvent::Started,
                PlayerEvent::NoteOn {
                    ch: 0,
                    note: 60,
                    vel: 80
                },
                PlayerEvent::Stopped,
                PlayerEvent::Finished,
            ]
        );

        // (b) Stop mid-run → Stopped only, pending event skipped.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        engine.install_song(song);
        let mut events = Vec::new();
        let mut cmds = VecDeque::from([EngineCommand::Stop { reset: false }]);
        assert_eq!(
            engine.play(&mut cmds, &mut events),
            Some(PlaybackOutcome::Stopped)
        );
        assert_eq!(events, vec![PlayerEvent::Started, PlayerEvent::Stopped]);
        assert_eq!(engine.position(), 0);

        // (c) Stop{reset} → Stopped only, position rewound (window.cpp:432
        // stop-with-reset: stop() then resetPosition()).
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(0, 61, 80), 120, 120));
        engine.install_song(song);
        run_cmd(&mut engine, EngineCommand::SeekTicks(120));
        assert_eq!(engine.position(), 120);
        let mut events = Vec::new();
        let mut cmds = VecDeque::from([EngineCommand::Stop { reset: true }]);
        assert_eq!(
            engine.play(&mut cmds, &mut events),
            Some(PlaybackOutcome::Stopped)
        );
        assert_eq!(events, vec![PlayerEvent::Started, PlayerEvent::Stopped]);
        assert_eq!(engine.position(), 0);
    }

    #[test]
    fn idle_stop_and_pause_through_run_loop() {
        // Idle `Stop` shuts up the output but emits nothing (player.cpp:107
        // with no running loop); idle `Pause` is a full no-op (C++ pause()
        // returns when !running). Driven through run() so the idle-dispatch
        // paths are covered.
        let (mut engine, calls) = test_engine();
        let (tx, rx) = mpsc::channel();
        let (mut emitter, receiver) = event_channel();
        tx.send(EngineCommand::Stop { reset: false }).unwrap();
        tx.send(EngineCommand::Pause).unwrap();
        tx.send(EngineCommand::Shutdown).unwrap();
        engine.run(&rx, &mut emitter);
        assert_eq!(calls_of(&calls).len(), 32);
        assert!(calls_of(&calls).iter().all(|c| matches!(
            c,
            DummyCall::Controller { cc, val: 0, .. }
                if *cc == CC_ALL_NOTES_OFF || *cc == CC_ALL_SOUNDS_OFF
        )));
        assert!(receiver.try_recv_lossless().is_none());
        assert!(!engine.is_paused());
    }

    #[test]
    fn pause_resume_skips_start_sequence() {
        let (mut engine, calls) = test_engine();
        let dir = tempfile::tempdir().unwrap();
        let syx = dir.path().join("c.syx");
        std::fs::write(&syx, [0xF0, 0x41, 0xF7]).unwrap();
        engine.set_companion(Some(syx));
        run_cmd(&mut engine, EngineCommand::SetSysexReset(1));
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(0, 61, 80), 120, 120));
        engine.install_song(song);

        // Pause aborts inside the reset settle: reset sent, companion never.
        let mut events = Vec::new();
        let mut cmds = VecDeque::from([EngineCommand::Pause]);
        assert_eq!(
            engine.play(&mut cmds, &mut events),
            Some(PlaybackOutcome::Paused)
        );
        assert_eq!(events, vec![PlayerEvent::Started, PlayerEvent::Stopped]);
        assert!(engine.is_paused());
        assert_eq!(engine.clock().elapsed(), Duration::from_millis(5));
        let sysexes: Vec<Vec<u8>> = calls_of(&calls)
            .into_iter()
            .filter_map(|c| match c {
                DummyCall::SysEx(bytes) => Some(bytes),
                _ => None,
            })
            .collect();
        assert_eq!(sysexes.len(), 1);

        // Resume plays everything with no second start sequence.
        let mut events = Vec::new();
        assert_eq!(
            engine.play(&mut VecDeque::new(), &mut events),
            Some(PlaybackOutcome::Ended)
        );
        assert!(!engine.is_paused());
        assert_eq!(events.last(), Some(&PlayerEvent::Finished));
        let notes: Vec<_> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::NoteOn { note, .. } => Some(*note),
                _ => None,
            })
            .collect();
        assert_eq!(notes, vec![60, 61]);
        let sysex_count = calls_of(&calls)
            .iter()
            .filter(|c| matches!(c, DummyCall::SysEx(_)))
            .count();
        assert_eq!(sysex_count, 1);
        assert_eq!(engine.position(), 120);
    }

    #[test]
    fn pause_stop_play_runs_full_start_sequence() {
        // C++ stop() clears m_paused + m_resumeWithoutReset unconditionally
        // (player.cpp:109-110): Pause → idle Stop → Play must run the FULL
        // start sequence, not resume. The Stop is driven through run() so
        // the idle-Stop dispatch arm is covered.
        let (mut engine, calls) = test_engine();
        let dir = tempfile::tempdir().unwrap();
        let syx = dir.path().join("c.syx");
        std::fs::write(&syx, [0xF0, 0x41, 0xF7]).unwrap();
        engine.set_companion(Some(syx));
        run_cmd(&mut engine, EngineCommand::SetSysexReset(1)); // GM reset + settle
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(0, 61, 80), 120, 120));
        engine.install_song(song);

        // Pause mid-play (aborts inside the reset settle, like
        // `pause_resume_skips_start_sequence`), then idle Stop via run().
        let mut events = Vec::new();
        let mut cmds = VecDeque::from([EngineCommand::Pause]);
        assert_eq!(
            engine.play(&mut cmds, &mut events),
            Some(PlaybackOutcome::Paused)
        );
        assert!(engine.is_paused());
        let (tx, rx) = mpsc::channel();
        let (mut emitter, receiver) = event_channel();
        tx.send(EngineCommand::Stop { reset: false }).unwrap();
        tx.send(EngineCommand::Shutdown).unwrap();
        engine.run(&rx, &mut emitter);
        assert!(!engine.is_paused());
        assert!(receiver.try_recv_lossless().is_none());

        // Replay runs the FULL start sequence: reset + companion SysEx,
        // reset-controllers ×16, literal CC7 ×16, scaled CC7 ×16.
        calls.lock().unwrap().clear();
        let mut events = Vec::new();
        assert_eq!(
            engine.play(&mut VecDeque::new(), &mut events),
            Some(PlaybackOutcome::Ended)
        );
        let got = calls_of(&calls);
        let sysexes: Vec<Vec<u8>> = got
            .iter()
            .filter_map(|c| match c {
                DummyCall::SysEx(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            sysexes,
            vec![
                vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7],
                vec![0xF0, 0x41, 0xF7],
            ]
        );
        let resets = got
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    DummyCall::Controller {
                        cc: CC_RESET_CONTROLLERS,
                        val: 0,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(resets, 16);
        let volumes = got
            .iter()
            .filter(|c| matches!(c, DummyCall::Controller { cc: CC_VOLUME, .. }))
            .count();
        assert_eq!(volumes, 32); // 16 literal + 16 scaled, no song CC7s
        assert_eq!(events.last(), Some(&PlayerEvent::Finished));
    }

    #[test]
    fn echo_ticks_restart_at_zero_on_resume() {
        // C++-faithful quirk (player.cpp:457): the echo counter is a fresh
        // local on every play, so post-resume Times count from 0 until the
        // first event wait completes.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 100, 100));
        song.events.push(at_tick(make_note_on(0, 61, 80), 500, 400));
        engine.install_song(song);
        // Pause right after the first note plays (position 100 kept).
        let (mut cmds, mut sink) = trip_pair(1, is_note_on, EngineCommand::Pause);
        assert_eq!(
            engine.play(&mut cmds, &mut sink),
            Some(PlaybackOutcome::Paused)
        );
        assert_eq!(engine.position(), 100);
        // Resume: position continues from 100, but the echo Times count
        // from 0 again.
        let mut events = Vec::new();
        assert_eq!(
            engine.play(&mut VecDeque::new(), &mut events),
            Some(PlaybackOutcome::Ended)
        );
        assert_eq!(engine.position(), 500);
        let times: Vec<i64> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Time { ticks, .. } => Some(*ticks),
                _ => None,
            })
            .collect();
        // Echo ticks restart at 50 (not 150) although the position was 100.
        // The doubled 500 is µs-truncation faithful to the C++ loop: 10 ×
        // time_of_ticks(50) = 2083330 µs < time_of_event(500) = 2083333 µs,
        // so a tenth echo fits exactly on tick 500 before the tail Time.
        assert_eq!(
            times,
            vec![50, 100, 150, 200, 250, 300, 350, 400, 450, 500, 500]
        );
    }

    // --- playEvent mixing rules ---

    #[test]
    fn mute_skips_channel_events_but_not_others() {
        let (mut engine, calls) = test_engine();
        // Muting also sends AllNotesOff + AllSoundsOff on that channel.
        let events = run_cmd(&mut engine, EngineCommand::SetMuted(0, true));
        assert!(events.is_empty());
        assert!(engine.is_muted(0));
        assert_eq!(
            calls_of(&calls),
            vec![
                DummyCall::Controller {
                    ch: 0,
                    cc: CC_ALL_NOTES_OFF,
                    val: 0
                },
                DummyCall::Controller {
                    ch: 0,
                    cc: CC_ALL_SOUNDS_OFF,
                    val: 0
                },
            ]
        );
        calls.lock().unwrap().clear();

        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(1, 64, 90), 0, 0));
        song.events
            .push(at_tick(make_control(0, CC_VOLUME, 100), 0, 0));
        engine.install_song(song);
        let mut events = Vec::new();
        assert_eq!(
            engine.play(&mut VecDeque::new(), &mut events),
            Some(PlaybackOutcome::Ended)
        );
        // Muted ch0: no output, no events — ch1 passes through.
        assert_eq!(
            calls_of(&calls)
                .iter()
                .filter(|c| matches!(c, DummyCall::NoteOn { .. }))
                .count(),
            1
        );
        assert!(calls_of(&calls).contains(&DummyCall::NoteOn {
            ch: 1,
            note: 64,
            vel: 90
        }));
        // The song's ch0 CC7 was skipped: ch0 saw exactly the 2 start-
        // sequence CC7s (literal 100 + scaled 100), no song CC7.
        let ch0_cc7 = calls_of(&calls)
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    DummyCall::Controller {
                        ch: 0,
                        cc: CC_VOLUME,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(ch0_cc7, 2);
        let notes: Vec<u8> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::NoteOn { note, .. } => Some(*note),
                _ => None,
            })
            .collect();
        assert_eq!(notes, vec![64]);

        // Unmuting sends nothing; the channel plays again on replay.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::SetMuted(0, false));
        assert!(calls_of(&calls).is_empty());
        engine.install_song({
            let mut song = song_base();
            song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
            song
        });
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        assert!(events.iter().any(|ev| matches!(
            ev,
            PlayerEvent::NoteOn {
                ch: 0,
                note: 60,
                ..
            }
        )));
    }

    #[test]
    fn pitch_shift_excludes_drums() {
        assert_eq!(GM_DRUM_CHANNEL, 9);
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetPitchShift(2));
        assert_eq!(engine.pitch_shift(), 2);
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(9, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_off(0, 60, 64), 10, 10));
        song.events.push(at_tick(make_note_off(9, 60, 64), 10, 0));
        engine.install_song(song);
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);

        // Output gets the shifted key (ch0) and the raw key (drums ch9).
        let notes: Vec<DummyCall> = calls_of(&calls)
            .into_iter()
            .filter(|c| matches!(c, DummyCall::NoteOn { .. } | DummyCall::NoteOff { .. }))
            .collect();
        assert_eq!(
            notes,
            vec![
                DummyCall::NoteOn {
                    ch: 0,
                    note: 62,
                    vel: 80
                },
                DummyCall::NoteOn {
                    ch: 9,
                    note: 60,
                    vel: 80
                },
                DummyCall::NoteOff {
                    ch: 0,
                    note: 62,
                    vel: 64
                },
                DummyCall::NoteOff {
                    ch: 9,
                    note: 60,
                    vel: 64
                },
            ]
        );
        // Events carry the shifted key too (player.cpp:353-369).
        let on_notes: Vec<(u8, u8)> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::NoteOn { ch, note, .. } => Some((*ch, *note)),
                _ => None,
            })
            .collect();
        assert_eq!(on_notes, vec![(0, 62), (9, 60)]);

        // setPitchShift sends all-notes-off first.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::SetPitchShift(-1));
        let got = calls_of(&calls);
        assert_eq!(got.len(), 32);
        assert!(matches!(
            got[0],
            DummyCall::Controller {
                ch: 0,
                cc: CC_ALL_NOTES_OFF,
                val: 0
            }
        ));
    }

    #[test]
    fn cc7_floor_clamp_and_passthrough() {
        let (mut engine, _) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(2, 0.5));
        run_cmd(&mut engine, EngineCommand::SetVolumeFactor(50));
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(4, 2.0));
        let mut song = song_base();
        // floor(100 * 0.5 * 50/100) = 25.
        song.events
            .push(at_tick(make_control(2, CC_VOLUME, 100), 0, 0));
        // Other CCs pass through untouched.
        song.events.push(at_tick(make_control(2, 10, 64), 0, 0));
        // floor(127 * 1.0 * 50/100) = floor(63.5) = 63.
        song.events
            .push(at_tick(make_control(3, CC_VOLUME, 127), 0, 0));
        // floor(1 * 1.0 * 50/100) = floor(0.5) = 0.
        song.events
            .push(at_tick(make_control(5, CC_VOLUME, 1), 0, 0));
        engine.install_song(song);
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        // No events for CCs (C++ emits nothing).
        assert!(
            !events
                .iter()
                .any(|ev| matches!(ev, PlayerEvent::Program { .. } | PlayerEvent::NoteOn { .. }))
        );

        // Clamp: floor(127 * 2.0 * 200/100) = 508 → 127.
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(4, 2.0));
        run_cmd(&mut engine, EngineCommand::SetVolumeFactor(200));
        let mut song = song_base();
        song.events
            .push(at_tick(make_control(4, CC_VOLUME, 127), 0, 0));
        engine.install_song(song);
        calls.lock().unwrap().clear();
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        // The scaled start-sequence CC7 and the song CC7 both clamp to 127
        // (reset-controllers sends the literal 100, not 127).
        let clamped = calls_of(&calls)
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    DummyCall::Controller {
                        ch: 4,
                        cc: CC_VOLUME,
                        val: 127
                    }
                )
            })
            .count();
        assert_eq!(clamped, 2);
    }

    #[test]
    fn cc7_values_pinned() {
        // Exact scaled values (see `cc7_floor_clamp_and_passthrough`).
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(2, 0.5));
        run_cmd(&mut engine, EngineCommand::SetVolumeFactor(50));
        let mut song = song_base();
        song.events
            .push(at_tick(make_control(2, CC_VOLUME, 100), 0, 0));
        song.events.push(at_tick(make_control(2, 10, 64), 0, 0));
        song.events
            .push(at_tick(make_control(3, CC_VOLUME, 127), 0, 0));
        song.events
            .push(at_tick(make_control(5, CC_VOLUME, 1), 0, 0));
        engine.install_song(song);
        calls.lock().unwrap().clear();
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        let volumes: Vec<(i32, i32)> = calls_of(&calls)
            .iter()
            .filter_map(|c| match c {
                DummyCall::Controller { ch, cc, val } if *cc == CC_VOLUME => Some((*ch, *val)),
                _ => None,
            })
            .collect();
        // 16 literal (reset-controllers) + 16 scaled (volume events; ch2
        // has shift 0.5 → 25) + the 3 song values.
        assert_eq!(volumes.len(), 35);
        assert!(volumes[..16].iter().all(|&(_, v)| v == 100));
        let scaled: Vec<(i32, i32)> = (0..16)
            .map(|ch| (ch, if ch == 2 { 25 } else { 50 }))
            .collect();
        assert_eq!(volumes[16..32], scaled[..]);
        assert_eq!(&volumes[32..], &[(2, 25), (3, 63), (5, 0)]);
        // The CC10 passes through with its raw value.
        assert!(calls_of(&calls).contains(&DummyCall::Controller {
            ch: 2,
            cc: 10,
            val: 64
        }));
    }

    #[test]
    fn program_lock_last_and_patch() {
        let (mut engine, calls) = test_engine();
        // Lock snapshots last→locked (fresh last is 0).
        run_cmd(&mut engine, EngineCommand::SetLocked(1, true));
        assert!(engine.is_locked(1));
        let mut song = song_base();
        song.events.push(at_tick(make_program(0, 5), 0, 0));
        song.events.push(at_tick(make_program(1, 7), 0, 0));
        engine.install_song(song);
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        // Locked ch1 forces locked_pgm (0); ch0 passes through.
        assert!(calls_of(&calls).contains(&DummyCall::Program { ch: 0, pgm: 5 }));
        assert!(calls_of(&calls).contains(&DummyCall::Program { ch: 1, pgm: 0 }));
        let programs: Vec<(u8, u8)> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Program { ch, patch } => Some((*ch, *patch)),
                _ => None,
            })
            .collect();
        assert_eq!(programs, vec![(0, 5), (1, 0)]);

        // setPatch sends immediately and updates last+locked when locked.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::SetPatch(1, 40));
        assert_eq!(
            calls_of(&calls),
            vec![DummyCall::Program { ch: 1, pgm: 40 }]
        );
        calls.lock().unwrap().clear();
        engine.install_song({
            let mut song = song_base();
            song.events.push(at_tick(make_program(1, 7), 0, 0));
            song
        });
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        assert!(calls_of(&calls).contains(&DummyCall::Program { ch: 1, pgm: 40 }));

        // Unlock: song programs pass through again.
        run_cmd(&mut engine, EngineCommand::SetLocked(1, false));
        calls.lock().unwrap().clear();
        engine.install_song({
            let mut song = song_base();
            song.events.push(at_tick(make_program(1, 7), 0, 0));
            song
        });
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        assert!(calls_of(&calls).contains(&DummyCall::Program { ch: 1, pgm: 7 }));
    }

    #[test]
    fn set_locked_snapshots_last_patch() {
        // last=9 via unlocked setPatch, then lock snapshots 9 → songs are
        // forced to 9 even though they ask for 20.
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetPatch(4, 9));
        run_cmd(&mut engine, EngineCommand::SetLocked(4, true));
        let mut song = song_base();
        song.events.push(at_tick(make_program(4, 20), 0, 0));
        engine.install_song(song);
        calls.lock().unwrap().clear();
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        assert_eq!(
            calls_of(&calls)
                .iter()
                .filter(|c| matches!(c, DummyCall::Program { .. }))
                .count(),
            1
        );
        assert!(calls_of(&calls).contains(&DummyCall::Program { ch: 4, pgm: 9 }));
    }

    #[test]
    fn sysex_split_and_raw_fallback() {
        let (mut engine, calls) = test_engine();
        let msg1 = vec![0xF0, 0x41, 0xF7];
        let mut msg2 = vec![0xF0, 0x43];
        msg2.extend(std::iter::repeat_n(0x10, 36));
        msg2.push(0xF7);
        assert_eq!(msg2.len(), 39); // > 32 → 15 ms pacing
        let mut blob = msg1.clone();
        blob.extend_from_slice(&msg2);
        let mut song = song_base();
        song.events.push(at_tick(
            MidiEvent {
                kind: Kind::SysEx,
                data: blob,
                ..Default::default()
            },
            0,
            0,
        ));
        // Unparseable blob: sent whole, no pacing.
        song.events.push(at_tick(
            MidiEvent {
                kind: Kind::SysEx,
                data: vec![0x01, 0x02, 0x03],
                ..Default::default()
            },
            0,
            0,
        ));
        engine.install_song(song);
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        let sysexes: Vec<Vec<u8>> = calls_of(&calls)
            .iter()
            .filter_map(|c| match c {
                DummyCall::SysEx(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(sysexes, vec![msg1, msg2, vec![0x01, 0x02, 0x03]]);
        // Only the 39-byte message paced (15 ms); no other sleeps.
        assert_eq!(engine.clock().elapsed(), Duration::from_millis(15));
        // No events for SysEx.
        assert!(
            !events
                .iter()
                .any(|ev| matches!(ev, PlayerEvent::Text { .. } | PlayerEvent::Program { .. }))
        );
    }

    #[test]
    fn tempo_beat_text_sigs_emit_owned_copies() {
        let (mut engine, calls) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_tempo(400_000.0), 0, 0));
        song.events.push(at_tick(make_beat(2, 3, 4), 0, 0));
        song.events.push(at_tick(text_event(0, 3, 5, b"hi"), 0, 0));
        song.events.push(at_tick(
            MidiEvent {
                kind: Kind::TimeSignature,
                tag: 7,
                a: 3,
                b: 4,
                ..Default::default()
            },
            0,
            0,
        ));
        song.events.push(at_tick(
            MidiEvent {
                kind: Kind::KeySignature,
                tag: 1,
                a: -2,
                c: 1,
                ..Default::default()
            },
            0,
            0,
        ));
        engine.install_song(song);
        let mut events = Vec::new();
        engine.play(&mut VecDeque::new(), &mut events);
        assert!(events.contains(&PlayerEvent::Beat {
            bar: 2,
            beat: 3,
            max: 4
        }));
        assert!(events.contains(&PlayerEvent::Text {
            track: 3,
            text_type: 5,
            ticks: 0,
            bytes: b"hi".to_vec(),
        }));
        assert!(events.contains(&PlayerEvent::TimeSignature {
            bar: 7,
            num: 3,
            den: 4
        }));
        assert!(events.contains(&PlayerEvent::KeySignature {
            track: 1,
            alterations: -2,
            minor: true
        }));
        let tempos: Vec<f64> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Tempo { tempo_us } => Some(*tempo_us),
                _ => None,
            })
            .collect();
        assert_eq!(tempos.len(), 1);
        assert!((tempos[0] - 400_000.0).abs() < f64::EPSILON);
        // Tempo events update the song conversion (and only the postlude
        // touched the output — no channel traffic).
        assert!((engine.song().current_tempo() - 400_000.0).abs() < f64::EPSILON);
        assert!(
            calls_of(&calls)
                .iter()
                .all(|c| matches!(c, DummyCall::Controller { .. }))
        );
    }

    // --- loop, beat cursor, latency, edge cases ---

    #[test]
    fn loop_rejump_replays_and_skips_past_end() {
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_beat(1, 1, 4), 0, 0));
        song.events.push(at_tick(make_beat(2, 1, 4), 120, 120));
        song.events.push(at_tick(make_beat(3, 1, 4), 240, 120));
        engine.install_song(song);
        run_cmd(
            &mut engine,
            EngineCommand::SetLoop {
                enabled: true,
                start: 1,
                end: 2,
            },
        );
        assert!(engine.loop_enabled());

        let (mut cmds, mut sink) = trip_pair(4, is_beat, EngineCommand::Stop { reset: false });
        let outcome = engine.play(&mut cmds, &mut sink);
        assert_eq!(outcome, Some(PlaybackOutcome::Stopped));
        // Bars 1,2 play, bar 3 trips the re-jump (never emitted), bars 1,2
        // replay — then the stop lands.
        let bars: Vec<u32> = sink
            .events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Beat { bar, .. } => Some(*bar),
                _ => None,
            })
            .collect();
        assert_eq!(bars, vec![1, 2, 1, 2]);
        assert!(!sink.events.contains(&PlayerEvent::Finished));
        assert_eq!(sink.events.last(), Some(&PlayerEvent::Stopped));
        assert_eq!(engine.position(), 120);
        // The beat cursor rode along: stepping back from bar 2 lands bar 1.
        run_cmd(&mut engine, EngineCommand::BeatStep(-1));
        assert_eq!(engine.position(), 0);
    }

    #[test]
    fn reset_position_vs_seek_beat_cursor() {
        // A1: ResetPosition rewinds cursor + ticks + beat cursor; SeekTicks
        // rewinds cursor + ticks but leaves the beat cursor alone.
        let (mut engine, calls) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_beat(1, 1, 4), 0, 0));
        song.events.push(at_tick(make_beat(2, 1, 4), 120, 120));
        song.events.push(at_tick(make_beat(3, 1, 4), 240, 120));
        engine.install_song(song);

        run_cmd(&mut engine, EngineCommand::JumpToBar(3));
        assert_eq!(engine.position(), 240);
        run_cmd(&mut engine, EngineCommand::SeekTicks(0));
        assert_eq!(engine.position(), 0);
        // Beat cursor still on bar 3: stepping back lands bar 2 (had it
        // reset to the first beat, we would land bar 1 at tick 0).
        run_cmd(&mut engine, EngineCommand::BeatStep(-1));
        assert_eq!(engine.position(), 120);

        run_cmd(&mut engine, EngineCommand::ResetPosition);
        assert_eq!(engine.position(), 0);
        // Beat cursor is the first beat now: forward lands bar 2, and back
        // from the first beat stays on bar 1.
        run_cmd(&mut engine, EngineCommand::BeatStep(1));
        assert_eq!(engine.position(), 120);
        run_cmd(&mut engine, EngineCommand::ResetPosition);
        run_cmd(&mut engine, EngineCommand::BeatStep(-1));
        assert_eq!(engine.position(), 0);

        // ResetPosition sends no all-notes-off (callers stop first);
        // SeekTicks does.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::ResetPosition);
        assert!(calls_of(&calls).is_empty());
        run_cmd(&mut engine, EngineCommand::SeekTicks(0));
        assert_eq!(calls_of(&calls).len(), 32);
    }

    #[test]
    fn seek_jump_beatstep_all_notes_off_first() {
        let (mut engine, calls) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_beat(1, 1, 4), 0, 0));
        song.events.push(at_tick(make_beat(2, 1, 4), 120, 120));
        engine.install_song(song);

        run_cmd(&mut engine, EngineCommand::SeekTicks(120));
        assert_eq!(calls_of(&calls).len(), 32);
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::JumpToBar(1));
        assert_eq!(engine.position(), 0);
        assert_eq!(calls_of(&calls).len(), 32);
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::BeatStep(1));
        assert_eq!(engine.position(), 120);
        assert_eq!(calls_of(&calls).len(), 32);

        // BeatStep(0) is a no-op: no offs, position kept.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::BeatStep(0));
        assert!(calls_of(&calls).is_empty());
        assert_eq!(engine.position(), 120);
    }

    #[test]
    fn sparse_stop_latency_is_one_quantum() {
        // A 1M-tick gap (≈69 minutes of song time): stopping right after
        // the first echo Time costs exactly one echo quantum of fake time,
        // and the pending far-future note is skipped (A2 uniform skip —
        // C++ would play it if the stop landed in the tail sleep).
        let (mut engine, calls) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events
            .push(at_tick(make_note_on(0, 61, 80), 1_000_000, 1_000_000));
        engine.install_song(song);

        let (mut cmds, mut sink) = trip_pair(1, is_time, EngineCommand::Stop { reset: false });
        let outcome = engine.play(&mut cmds, &mut sink);
        assert_eq!(outcome, Some(PlaybackOutcome::Stopped));
        assert_eq!(
            engine.clock().elapsed(),
            engine.song().time_of_ticks(ECHO_RES_TICKS as u64)
        );
        assert_eq!(
            sink.events,
            vec![
                PlayerEvent::Started,
                PlayerEvent::NoteOn {
                    ch: 0,
                    note: 60,
                    vel: 80
                },
                PlayerEvent::Time {
                    elapsed_ms: 208,
                    ticks: 50
                },
                PlayerEvent::Stopped,
            ]
        );
        let on_count = calls_of(&calls)
            .iter()
            .filter(|c| matches!(c, DummyCall::NoteOn { .. }))
            .count();
        assert_eq!(on_count, 1);
    }

    #[test]
    fn stop_aborts_companion_pacing() {
        // 50 companion messages × 15 ms = 750 ms of C++-uninterruptible
        // pacing (+ 40 ms reset settle): here the stop lands after the
        // first 5 ms chunk and the companion is never touched.
        let (mut engine, calls) = test_engine();
        let dir = tempfile::tempdir().unwrap();
        let syx = dir.path().join("big.syx");
        let mut blob = Vec::new();
        for _ in 0..50 {
            blob.extend_from_slice(&[0xF0, 0x41, 0xF7]);
        }
        std::fs::write(&syx, &blob).unwrap();
        engine.set_companion(Some(syx));
        run_cmd(&mut engine, EngineCommand::SetSysexReset(1));
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        engine.install_song(song);

        let mut events = Vec::new();
        let mut cmds = VecDeque::from([EngineCommand::Stop { reset: false }]);
        let outcome = engine.play(&mut cmds, &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Stopped));
        assert_eq!(events, vec![PlayerEvent::Started, PlayerEvent::Stopped]);
        assert_eq!(engine.clock().elapsed(), Duration::from_millis(5));
        let sysexes: Vec<Vec<u8>> = calls_of(&calls)
            .iter()
            .filter_map(|c| match c {
                DummyCall::SysEx(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .collect();
        // Reset sent, then the abort: companion never started.
        assert_eq!(sysexes, vec![vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7]]);
    }

    #[test]
    fn zero_tempo_song_completes_without_echo_spin() {
        // DEVIATION pin: zero ticks2micros would busy-spin the C++ echo
        // loop emitting Time spam; here the song just completes with one
        // Time per event wait and zero fake time.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_tempo(0.0), 0, 0));
        song.events.push(at_tick(make_note_on(0, 60, 80), 120, 120));
        song.events.push(at_tick(make_note_on(0, 61, 80), 240, 120));
        engine.install_song(song);
        let mut events = Vec::new();
        let outcome = engine.play(&mut VecDeque::new(), &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));
        assert_eq!(engine.clock().elapsed(), Duration::ZERO);
        let times: Vec<i64> = events
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::Time { ticks, .. } => Some(*ticks),
                _ => None,
            })
            .collect();
        assert_eq!(times, vec![120, 240]);
        assert_eq!(events.last(), Some(&PlayerEvent::Finished));
    }

    #[test]
    fn no_output_selected_plays_silently_without_tempo_update() {
        // C++ 1:1 quirk: with null m_out, play_event is a whole-function
        // no-op — no callbacks AND no tempo adoption.
        let manager = MidiOutputManager::new();
        assert!(manager.current().is_none());
        let mut engine = Engine::new(FakeClock::new(), manager);
        let mut song = song_base();
        song.events.push(at_tick(make_tempo(400_000.0), 0, 0));
        song.events.push(at_tick(make_note_on(0, 60, 80), 120, 120));
        engine.install_song(song);
        let mut events = Vec::new();
        let outcome = engine.play(&mut VecDeque::new(), &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));
        assert!(
            events.iter().all(|ev| matches!(
                ev,
                PlayerEvent::Started
                    | PlayerEvent::Time { .. }
                    | PlayerEvent::Stopped
                    | PlayerEvent::Finished
            )),
            "unexpected events: {events:?}"
        );
        assert!((engine.song().current_tempo() - 500_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn set_volume_factor_resends_and_resets_raw() {
        // A9: SetVolumeFactor resends all CC7 *and* resets the stored raw
        // volumes to 100 first.
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(2, 0.5));
        let mut song = song_base();
        song.events
            .push(at_tick(make_control(2, CC_VOLUME, 64), 0, 0));
        engine.install_song(song);
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        // Raw 64 learned: floor(64 * 0.5 * 100/100) = 32 was sent.
        assert!(calls_of(&calls).contains(&DummyCall::Controller {
            ch: 2,
            cc: CC_VOLUME,
            val: 32
        }));
        // Resend with master 100: every channel gets scaled 100.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::SetVolumeFactor(100));
        assert_eq!(engine.volume_factor(), 100);
        let last16: Vec<DummyCall> = calls_of(&calls)
            .iter()
            .filter(|c| matches!(c, DummyCall::Controller { cc, .. } if *cc == CC_VOLUME))
            .cloned()
            .collect();
        assert_eq!(last16.len(), 16);
        // The raw reset is observable: a shift resend now scales 100,
        // not the learned 64.
        calls.lock().unwrap().clear();
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(2, 0.5));
        assert_eq!(
            calls_of(&calls),
            vec![DummyCall::Controller {
                ch: 2,
                cc: CC_VOLUME,
                val: 50
            }]
        );
    }

    #[test]
    fn tempo_factor_reject_live() {
        // A7 through the engine: 999.0 is rejected (timing unchanged),
        // 2.0 halves the wall time.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(0, 61, 80), 120, 120));
        engine.install_song(song);
        // Factor lives on the song: set it after installing (load resets
        // it to 1.0, like C++ clear()).
        run_cmd(&mut engine, EngineCommand::SetTempoFactor(999.0));
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        assert_eq!(engine.clock().elapsed(), Duration::from_micros(500_000));

        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        song.events.push(at_tick(make_note_on(0, 61, 80), 120, 120));
        engine.install_song(song);
        run_cmd(&mut engine, EngineCommand::SetTempoFactor(2.0));
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        assert_eq!(engine.clock().elapsed(), Duration::from_micros(250_000));
    }

    #[test]
    fn reset_programs_locked_and_free() {
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetPatch(3, 40));
        run_cmd(&mut engine, EngineCommand::SetLocked(3, true));
        calls.lock().unwrap().clear();
        engine.reset_programs();
        let mut expected = Vec::new();
        for ch in 0..16 {
            expected.push(DummyCall::Program {
                ch,
                pgm: if ch == 3 { 40 } else { 0 },
            });
        }
        assert_eq!(calls_of(&calls), expected);
    }

    #[test]
    fn invalid_channels_are_ignored() {
        let (mut engine, calls) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetMuted(16, true));
        run_cmd(&mut engine, EngineCommand::SetMuted(255, true));
        run_cmd(&mut engine, EngineCommand::SetLocked(16, true));
        run_cmd(&mut engine, EngineCommand::SetPatch(99, 1));
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(200, 0.5));
        assert!(calls_of(&calls).is_empty());
        assert!(!engine.is_muted(16));
        assert!(!engine.is_locked(-1));
        assert!((engine.volume(99) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn drums_and_reset_kinds_are_plain_setters() {
        let (mut engine, _) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetDrumsChannel(3));
        assert_eq!(engine.drums_channel(), 3);
        run_cmd(&mut engine, EngineCommand::SetSysexReset(4));
        assert_eq!(engine.sysex_reset(), 4);
    }

    #[test]
    fn bpm_accessors_follow_tempo() {
        let (mut engine, _) = test_engine();
        assert!((engine.current_bpm() - 120.0).abs() < 1e-9);
        assert!((engine.initial_bpm() - 120.0).abs() < 1e-9);
        let mut song = song_base();
        song.events.push(at_tick(make_tempo(250_000.0), 0, 0));
        engine.install_song(song);
        engine.play(&mut VecDeque::new(), &mut Vec::new());
        assert!((engine.current_bpm() - 240.0).abs() < 1e-9);
    }

    // --- backpressure, load, output selection ---

    #[test]
    fn backpressure_emitter_unit() {
        // A13: Time sheds on a full bounded channel (counted); lossless
        // events are never dropped and keep their order.
        let (mut emitter, receiver) = event_channel();
        for ticks in 0..20 {
            emitter.emit(PlayerEvent::Time {
                elapsed_ms: ticks as u64,
                ticks: ticks as i64,
            });
        }
        assert_eq!(receiver.time_dropped(), 20 - TIME_CHANNEL_CAP as u64);
        let mut got = 0;
        while receiver.try_recv_time().is_some() {
            got += 1;
        }
        assert_eq!(got, TIME_CHANNEL_CAP);
        assert!(receiver.try_recv_time().is_none());

        for note in 0..100u8 {
            emitter.emit(PlayerEvent::NoteOn {
                ch: 0,
                note,
                vel: 64,
            });
        }
        let mut notes = Vec::new();
        while let Some(PlayerEvent::NoteOn { note, .. }) = receiver.try_recv_lossless() {
            notes.push(note);
        }
        assert_eq!(notes, (0..100u8).collect::<Vec<_>>());

        // drain_time_latest keeps the freshest pending Time.
        for ticks in [1, 2, 3] {
            emitter.emit(PlayerEvent::Time {
                elapsed_ms: 0,
                ticks,
            });
        }
        assert_eq!(
            receiver.drain_time_latest(),
            Some(PlayerEvent::Time {
                elapsed_ms: 0,
                ticks: 3
            })
        );
        assert!(receiver.try_recv_time().is_none());
    }

    #[test]
    fn load_resets_state_and_reports() {
        let (mut engine, _) = test_engine();
        run_cmd(&mut engine, EngineCommand::SetMuted(0, true));
        run_cmd(&mut engine, EngineCommand::SetChannelVolume(1, 0.25));
        run_cmd(
            &mut engine,
            EngineCommand::SetLoop {
                enabled: true,
                start: 5,
                end: 9,
            },
        );
        assert!(engine.is_muted(0));

        let mut events = Vec::new();
        let effect =
            engine.handle_command(EngineCommand::Load("examples/test.mid".into()), &mut events);
        assert!(matches!(effect, CommandEffect::Load(_)));
        // The idle run-loop path performs the load after the (empty here)
        // postlude; drive it directly.
        if let CommandEffect::Load(path) = effect {
            engine.load_song(&path, &mut events);
        }
        assert_eq!(events.len(), 1);
        match &events[0] {
            PlayerEvent::Opened(snapshot) => {
                assert_eq!(snapshot.title, "test.mid");
                assert!(snapshot.bars > 0);
                assert!(snapshot.len_ticks > 0);
            }
            other => panic!("expected Opened, got {other:?}"),
        }
        // Load resets mix + loop + position (player.cpp:63-78).
        assert!(!engine.is_muted(0));
        assert!((engine.volume(1) - 1.0).abs() < f64::EPSILON);
        assert!(!engine.loop_enabled());
        assert_eq!(engine.position(), 0);
    }

    #[test]
    fn load_failed_event() {
        let (mut engine, _) = test_engine();
        let mut events = Vec::new();
        let effect = engine.handle_command(
            EngineCommand::Load("/nonexistent/t7-missing.mid".into()),
            &mut events,
        );
        if let CommandEffect::Load(path) = effect {
            engine.load_song(&path, &mut events);
        }
        assert_eq!(events.len(), 1);
        match &events[0] {
            PlayerEvent::LoadFailed { file, errors } => {
                assert!(file.contains("t7-missing.mid"));
                assert!(!errors.is_empty());
            }
            other => panic!("expected LoadFailed, got {other:?}"),
        }
    }

    #[test]
    fn load_during_playback_runs_postlude_first() {
        // C++ loadFile stops the thread first: Stopped precedes Opened.
        let (mut engine, _) = test_engine();
        let mut song = song_base();
        song.events.push(at_tick(make_note_on(0, 60, 80), 0, 0));
        engine.install_song(song);
        let mut sink = Vec::new();
        let mut cmds = VecDeque::from([EngineCommand::Load("examples/test.mid".into())]);
        let outcome = engine.play(&mut cmds, &mut sink);
        assert!(matches!(outcome, Some(PlaybackOutcome::Load(_))));
        // Caller (run()) performs the load after the postlude.
        if let Some(PlaybackOutcome::Load(path)) = outcome {
            engine.load_song(&path, &mut sink);
        }
        assert_eq!(sink.len(), 3);
        assert_eq!(sink[0], PlayerEvent::Started);
        assert_eq!(sink[1], PlayerEvent::Stopped);
        assert!(matches!(sink[2], PlayerEvent::Opened(_)));
        assert!(!sink.contains(&PlayerEvent::Finished));
    }

    #[test]
    fn real_file_plays_to_finish_on_fake_clock() {
        // End to end through the real SMF decoder + beat synthesis +
        // scheduler, deterministically (fake clock: no wall waiting).
        let (mut engine, _) = test_engine();
        let mut events = Vec::new();
        let effect =
            engine.handle_command(EngineCommand::Load("examples/test.mid".into()), &mut events);
        if let CommandEffect::Load(path) = effect {
            engine.load_song(&path, &mut events);
        }
        let mut events = Vec::new();
        let outcome = engine.play(&mut VecDeque::new(), &mut events);
        assert_eq!(outcome, Some(PlaybackOutcome::Ended));
        assert_eq!(events.first(), Some(&PlayerEvent::Started));
        assert_eq!(events.last(), Some(&PlayerEvent::Finished));
        assert!(
            events
                .iter()
                .any(|ev| matches!(ev, PlayerEvent::NoteOn { .. }))
        );
        assert!(
            events
                .iter()
                .any(|ev| matches!(ev, PlayerEvent::Beat { .. }))
        );
        assert!(engine.clock().elapsed() > Duration::ZERO);
    }

    #[test]
    fn select_output_reports_resolved_backend() {
        let (mut engine, _) = test_engine();
        // Known backend: exact report, no warning.
        let events = run_cmd(
            &mut engine,
            EngineCommand::SelectOutput {
                backend: "Recorder".to_string(),
                port: "rec:0".to_string(),
            },
        );
        assert_eq!(
            events,
            vec![PlayerEvent::OutputChanged {
                backend: "Recorder".to_string(),
                port: "rec:0".to_string(),
                warning: None,
            }]
        );
        // Unknown backend resolves to the front backend (ALSA, A6c).
        // Whether open succeeds depends on host hardware; the report stays
        // consistent either way.
        let events = run_cmd(
            &mut engine,
            EngineCommand::SelectOutput {
                backend: "Nope".to_string(),
                port: "14:0".to_string(),
            },
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            PlayerEvent::OutputChanged {
                backend,
                port,
                warning,
            } => {
                assert_eq!(backend, "ALSA");
                if warning.is_none() {
                    assert_eq!(port, "14:0");
                }
                if port.is_empty() {
                    assert!(warning.is_some());
                }
            }
            other => panic!("expected OutputChanged, got {other:?}"),
        }
        // SoundFont paths are accepted (FluidSynth acts on them at open).
        let events = run_cmd(
            &mut engine,
            EngineCommand::SetSoundFont("/sound/fonts/gm.sf2".to_string()),
        );
        assert!(events.is_empty());
    }

    // --- threaded integration (real clock, generous bounds) ---

    fn vlq(mut value: u32) -> Vec<u8> {
        let mut out = vec![(value & 0x7F) as u8];
        value >>= 7;
        while value > 0 {
            out.push(0x80 | (value & 0x7F) as u8);
            value >>= 7;
        }
        out.reverse();
        out
    }

    fn smf_bytes(division: u16, track: &[u8]) -> Vec<u8> {
        let mut bytes = vec![
            0x4D,
            0x54,
            0x68,
            0x64, // MThd
            0x00,
            0x00,
            0x00,
            0x06, // len 6
            0x00,
            0x00, // format 0
            0x00,
            0x01, // 1 track
            (division >> 8) as u8,
            division as u8,
            0x4D,
            0x54,
            0x72,
            0x6B, // MTrk
        ];
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(track);
        bytes
    }

    fn spawn_with_recorder() -> (
        EngineHandle,
        EngineEventReceiver,
        std::thread::JoinHandle<()>,
        Arc<Mutex<Vec<DummyCall>>>,
    ) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let factory_calls = Arc::clone(&calls);
        let (handle, receiver, join) = spawn_engine_with(move || {
            let mut manager = MidiOutputManager::new();
            manager.add_backend(Box::new(SharedRecorder::from_arc(factory_calls)));
            manager
        });
        (handle, receiver, join, calls)
    }

    fn recv_until_lossless(
        receiver: &EngineEventReceiver,
        mut want: impl FnMut(&PlayerEvent) -> bool,
    ) -> Vec<PlayerEvent> {
        let mut got = Vec::new();
        loop {
            let ev = receiver
                .recv_lossless_timeout(Duration::from_secs(30))
                .expect("engine answers within 30 s");
            let done = want(&ev);
            got.push(ev);
            if done {
                return got;
            }
        }
    }

    #[test]
    fn threaded_play_stop_and_shutdown() {
        let (handle, receiver, join, _) = spawn_with_recorder();
        handle.send(EngineCommand::SelectOutput {
            backend: "Recorder".to_string(),
            port: "rec:0".to_string(),
        });
        handle.send(EngineCommand::Load("examples/test.mid".into()));
        let ev = receiver.recv_lossless_timeout(Duration::from_secs(5));
        assert!(
            matches!(ev, Some(PlayerEvent::OutputChanged { .. })),
            "{ev:?}"
        );
        let ev = receiver.recv_lossless_timeout(Duration::from_secs(5));
        assert!(matches!(ev, Some(PlayerEvent::Opened(_))), "{ev:?}");

        handle.send(EngineCommand::Play);
        let ev = receiver.recv_lossless_timeout(Duration::from_secs(5));
        assert_eq!(ev, Some(PlayerEvent::Started));
        // Song events may precede the stop in the queue; drain to Stopped.
        handle.send(EngineCommand::Stop { reset: false });
        let got = recv_until_lossless(&receiver, |ev| matches!(ev, PlayerEvent::Stopped));
        assert!(!got.contains(&PlayerEvent::Finished));

        handle.send(EngineCommand::Shutdown);
        join.join().expect("engine thread exits on Shutdown");
    }

    #[test]
    fn sparse_stop_latency_threaded() {
        // 20000-tick gap ≈ 83 s of wall time; the stop must land in ~one
        // 208 ms echo quantum (5 s bound = 25× slack).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sparse.mid");
        let mut track = vec![0x00, 0x90, 0x3C, 0x50];
        track.extend_from_slice(&vlq(20_000));
        track.extend_from_slice(&[0x80, 0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00]);
        std::fs::write(&path, smf_bytes(120, &track)).unwrap();

        let (handle, receiver, join, _) = spawn_with_recorder();
        handle.send(EngineCommand::SelectOutput {
            backend: "Recorder".to_string(),
            port: "rec:0".to_string(),
        });
        handle.send(EngineCommand::Load(path));
        recv_until_lossless(&receiver, |ev| matches!(ev, PlayerEvent::Opened(_)));
        handle.send(EngineCommand::Play);
        assert_eq!(
            receiver.recv_lossless_timeout(Duration::from_secs(5)),
            Some(PlayerEvent::Started)
        );
        // Settle into the gap (first echo Time lands at ~208 ms).
        std::thread::sleep(Duration::from_millis(600));
        let start = Instant::now();
        handle.send(EngineCommand::Stop { reset: false });
        // Queued song events drain instantly; the block ends at Stopped.
        let deadline = Instant::now() + Duration::from_secs(10);
        let got = recv_until_lossless(&receiver, |ev| {
            assert!(
                Instant::now() < deadline,
                "Stopped never arrived within 10 s"
            );
            matches!(ev, PlayerEvent::Stopped)
        });
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "stop took {:?} (>5 s of an 83 s gap)",
            start.elapsed()
        );
        assert!(!got.contains(&PlayerEvent::Finished));
        handle.send(EngineCommand::Shutdown);
        join.join().expect("engine thread exits");
    }

    #[test]
    fn flood_threaded_time_sheds_notes_lossless() {
        // 2000 same-tick NoteOns + 2000 NoteOffs, then a fast-tempo gap
        // producing ~340 echo Times with nobody draining Time: every note
        // survives in order while most Times shed.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storm.mid");
        let mut track = vec![0x00, 0xFF, 0x51, 0x03, 0x00, 0x04, 0x00];
        for i in 0..2000u32 {
            track.extend_from_slice(&[0x00, 0x90, (i % 128) as u8, 0x40]);
        }
        for i in 0..2000u32 {
            track.extend_from_slice(&[0x00, 0x80, (i % 128) as u8, 0x40]);
        }
        track.extend_from_slice(&vlq(20_000));
        track.extend_from_slice(&[0x90, 0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00]);
        std::fs::write(&path, smf_bytes(120, &track)).unwrap();

        let (handle, receiver, join, _) = spawn_with_recorder();
        handle.send(EngineCommand::SelectOutput {
            backend: "Recorder".to_string(),
            port: "rec:0".to_string(),
        });
        handle.send(EngineCommand::Load(path));
        recv_until_lossless(&receiver, |ev| matches!(ev, PlayerEvent::Opened(_)));
        handle.send(EngineCommand::Play);
        let got = recv_until_lossless(&receiver, |ev| matches!(ev, PlayerEvent::Finished));
        assert_eq!(got.first(), Some(&PlayerEvent::Started));

        let ons: Vec<u8> = got
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::NoteOn { note, .. } => Some(*note),
                _ => None,
            })
            .collect();
        // The storm's 2000 plus the gap-closing note, all in order.
        let expected: Vec<u8> = (0..2000u32).map(|i| (i % 128) as u8).collect();
        assert_eq!(ons.len(), 2001);
        assert_eq!(&ons[..2000], &expected[..]);
        let offs: Vec<u8> = got
            .iter()
            .filter_map(|ev| match ev {
                PlayerEvent::NoteOff { note, .. } => Some(*note),
                _ => None,
            })
            .collect();
        assert_eq!(offs.len(), 2000);
        assert_eq!(
            offs,
            (0..2000u32).map(|i| (i % 128) as u8).collect::<Vec<_>>()
        );

        let mut times = 0;
        while receiver.try_recv_time().is_some() {
            times += 1;
        }
        assert!(
            times <= TIME_CHANNEL_CAP,
            "Time channel must stay bounded, got {times}"
        );
        assert!(
            receiver.time_dropped() > 100,
            "expected heavy Time shedding, dropped {}",
            receiver.time_dropped()
        );
        handle.send(EngineCommand::Shutdown);
        join.join().expect("engine thread exits");
    }

    #[test]
    fn handle_drop_exits_thread() {
        let (handle, _, join, _) = spawn_with_recorder();
        drop(handle);
        join.join().expect("engine thread exits on handle drop");
    }
}
