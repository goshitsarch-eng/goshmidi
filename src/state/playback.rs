//! Transport + position + tempo/volume/pitch + loop + repeat state, and the
//! end-of-song policy (arch §2.1, `docs/migration/ux.md` F8).
//!
//! The C++ twin is the `MainWindow` transport half: `m_repeat`, `m_seeking`,
//! the position/tempo/volume/pitch controls, the loop range, and the
//! `onFinished` decision tree (`src/ui/window.cpp:194-214`).

use crate::core::event::tempo_to_bpm;
use crate::message::{RepeatMode, Status};

/// Tempo slider range, UI percent (`window.cpp:1700-1703`).
pub const TEMPO_MIN_PCT: u32 = 50;
pub const TEMPO_MAX_PCT: u32 = 200;
/// Volume slider range, UI percent (`window.cpp:1704-1707`).
pub const VOLUME_MIN_PCT: u32 = 0;
pub const VOLUME_MAX_PCT: u32 = 200;
/// Pitch spin range, semitones (ux §3.3).
pub const PITCH_MIN: i32 = -12;
pub const PITCH_MAX: i32 = 12;

/// Transport + dashboard state (arch §2.1 `PlaybackState`).
#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub status: Status,
    pub position_ticks: i64,
    pub song_len_ticks: i64,
    pub elapsed_ms: u64,
    /// Tempo slider, `TEMPO_MIN_PCT..=TEMPO_MAX_PCT`, default 100.
    pub tempo_factor_pct: u32,
    /// Volume slider, `VOLUME_MIN_PCT..=VOLUME_MAX_PCT`, default 100.
    pub volume_pct: u32,
    /// Pitch spin, `PITCH_MIN..=PITCH_MAX`, default 0.
    pub pitch_semitones: i32,
    /// Live dashboard tempo label value, in bpm.
    pub bpm: f64,
    /// Raw song tempo in µs per quarter (factor divided back out), used to
    /// recompute [`PlaybackState::bpm`] when the tempo slider moves — the
    /// C++ `applyTempo` → `currentBpm()` path (`player.hpp:52`:
    /// `currentTempo = m_tempo / m_tempoFactor`).
    pub base_tempo_us: f64,
    pub bar: u32,
    pub beat: u32,
    pub beats_per_bar: u32,
    pub loop_enabled: bool,
    pub loop_start: u32,
    pub loop_end: u32,
    pub repeat: RepeatMode,
    /// In-flight seek marker (named after C++ `m_seeking`, which is likewise
    /// behaviorally inert — set and cleared synchronously around the seek
    /// call, `window.cpp:1739-1743`): set by `PositionSeek` alongside the
    /// optimistic `position_ticks` update, cleared when the next
    /// `Player(Time)` lands. Informational only — neither the view (which
    /// renders `position` verbatim) nor the `Time` handler gates on it;
    /// drag-follow works through the optimistic position alone.
    pub seeking: bool,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: Status::Ready,
            position_ticks: 0,
            song_len_ticks: 0,
            elapsed_ms: 0,
            tempo_factor_pct: 100,
            volume_pct: 100,
            pitch_semitones: 0,
            bpm: tempo_to_bpm(500_000.0),
            base_tempo_us: 500_000.0,
            // Dashboard defaults: `1:1` label, 4 lamps (`window.cpp:1649+`).
            bar: 1,
            beat: 1,
            beats_per_bar: 4,
            loop_enabled: false,
            loop_start: 1,
            loop_end: 1,
            repeat: RepeatMode::Off,
            seeking: false,
        }
    }
}

impl PlaybackState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clamp + store the tempo slider; returns the stored value.
    pub fn set_tempo_pct(&mut self, pct: u32) -> u32 {
        self.tempo_factor_pct = pct.clamp(TEMPO_MIN_PCT, TEMPO_MAX_PCT);
        self.tempo_factor_pct
    }

    /// Clamp + store the volume slider; returns the stored value.
    pub fn set_volume_pct(&mut self, pct: u32) -> u32 {
        self.volume_pct = pct.clamp(VOLUME_MIN_PCT, VOLUME_MAX_PCT);
        self.volume_pct
    }

    /// Clamp + store the pitch spin; returns the stored value.
    pub fn set_pitch(&mut self, semis: i32) -> i32 {
        self.pitch_semitones = semis.clamp(PITCH_MIN, PITCH_MAX);
        self.pitch_semitones
    }

    /// Engine tempo factor for the current slider (`SetTempoFactor`).
    pub fn tempo_factor(&self) -> f64 {
        f64::from(self.tempo_factor_pct) / 100.0
    }

    /// Live dashboard bpm under the current slider (`applyTempo` parity:
    /// raw tempo divided by the factor, then to bpm).
    pub fn live_bpm(&self) -> f64 {
        tempo_to_bpm(self.base_tempo_us / self.tempo_factor())
    }

    /// Adopt a `Tempo` event value (which already includes the engine's
    /// factor): divide the factor back out to keep the raw base, and show
    /// the event value as-is (`onTempo` parity).
    pub fn note_tempo_event(&mut self, tempo_us: f64) {
        self.base_tempo_us = tempo_us * self.tempo_factor();
        self.bpm = tempo_to_bpm(tempo_us);
    }

    /// Slider fraction `0.0..=1.0` (`m_posScale` is 0..1000 over the song).
    pub fn position_fraction(&self) -> f64 {
        if self.song_len_ticks <= 0 {
            0.0
        } else {
            (self.position_ticks as f64 / self.song_len_ticks as f64).clamp(0.0, 1.0)
        }
    }

    /// Reset per-song transport state on `Opened` (`loadCurrent` parity):
    /// position/clock/beat cursors cleared, loop synced to the engine reset
    /// (disabled, `1..=last_bar` — A8 engine-first), tempo/volume/pitch
    /// sliders kept, `bpm` from the raw initial tempo (`initialBpm`, factor
    /// NOT applied — `player.cpp:337-340`).
    pub fn reset_for_song(&mut self, len_ticks: i64, initial_tempo_us: f64, last_bar: u32) {
        self.position_ticks = 0;
        self.song_len_ticks = len_ticks.max(0);
        self.elapsed_ms = 0;
        self.base_tempo_us = initial_tempo_us;
        self.bpm = tempo_to_bpm(initial_tempo_us);
        self.bar = 1;
        self.beat = 1;
        self.loop_enabled = false;
        self.loop_start = 1;
        self.loop_end = last_bar.max(1);
        self.seeking = false;
    }
}

/// `HH:MM:SS` clock label (`updateTime`, `window.cpp:296-308`).
pub fn format_time_ms(ms: u64) -> String {
    let total = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

/// Dashboard tempo label (`%.1f bpm`, `window.cpp:184-188`).
pub fn format_bpm(bpm: f64) -> String {
    format!("{bpm:.1} bpm")
}

/// End-of-song decision (`onFinished`, `window.cpp:194-214`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishAction {
    /// `m_repeat == 1`: engine `ResetPosition` + play (no song-settings save
    /// — the song is not left).
    RepeatSong,
    /// `m_repeat == 2`: save song settings (when auto), advance with wrap,
    /// load + autoplay.
    RepeatList,
    /// Otherwise, when auto-advance applies and the song is not last: same
    /// as pressing Next (save when auto, advance, load + autoplay) — on top
    /// of the save-before-branch, i.e. C++'s double-save (same bytes).
    AutoAdvance,
    /// Stay on the `Finished` status (the caller still saves first when
    /// auto song settings are on — `window.cpp:202-203`).
    StayFinished,
}

/// Pure end-of-song policy: the `onFinished` tree with the engine calls left
/// to the caller (`app.rs`).
///
/// Loop interplay (P10b, verified against `player.cpp:452-526`): the loop
/// leg lives ENTIRELY engine-side — a capturing loop re-jumps inside
/// `playerLoop`/`play` and `Finished` never fires, so the repeat policy
/// below is unreachable while the loop captures. Consequences, all pinned
/// engine-side (`loop_rejump_replays_and_skips_past_end`,
/// `loop_degenerate_range_still_recaptures`):
/// * Loop ON + any range + beats present → infinite loop, no `Finished`,
///   repeat/auto-advance never consulted. Even degenerate ranges (start/end
///   past the last bar) recapture: `jump_to_bar` lands on the nearest beat
///   (`sequence.cpp:174-187`), so `hasMoreEvents` holds and the song never
///   exhausts. Range-independence is the point — composition must NOT gate
///   this policy on `loop_enabled`.
/// * Loop ON + beat-less song → the loop cannot anchor (`jumpToBar`
///   returns null, the cursor stays exhausted) and `Finished` DOES fire;
///   the policy below runs normally. Real song loads always synthesize
///   beats, so this leg is theoretical — but it proves the policy must run
///   ungated: `Finished` arrival itself proves the loop did not capture.
/// * Loop OFF → `Finished` fires at exhaustion; the policy below decides.
///
/// `app.rs::on_finished` therefore calls this unconditionally — correct in
/// every cell of the loop × repeat matrix by construction.
pub fn finish_action(repeat: RepeatMode, auto_advance: bool, at_last: bool) -> FinishAction {
    if repeat == RepeatMode::Song {
        return FinishAction::RepeatSong;
    }
    if repeat == RepeatMode::Playlist {
        return FinishAction::RepeatList;
    }
    if auto_advance && !at_last {
        return FinishAction::AutoAdvance;
    }
    FinishAction::StayFinished
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_cpp_controls() {
        let p = PlaybackState::new();
        assert_eq!(p.status, Status::Ready);
        assert_eq!(p.tempo_factor_pct, 100);
        assert_eq!(p.volume_pct, 100);
        assert_eq!(p.pitch_semitones, 0);
        assert_eq!((p.bar, p.beat, p.beats_per_bar), (1, 1, 4));
        assert!(!p.loop_enabled);
        assert_eq!(p.repeat, RepeatMode::Off);
        assert!(!p.seeking);
    }

    #[test]
    fn slider_setters_clamp_to_cpp_ranges() {
        let mut p = PlaybackState::new();
        assert_eq!(p.set_tempo_pct(50), 50);
        assert_eq!(p.set_tempo_pct(200), 200);
        assert_eq!(p.set_tempo_pct(49), 50);
        assert_eq!(p.set_tempo_pct(999), 200);
        assert_eq!(p.set_volume_pct(0), 0);
        assert_eq!(p.set_volume_pct(200), 200);
        assert_eq!(p.set_volume_pct(999), 200);
        assert_eq!(p.set_pitch(-12), -12);
        assert_eq!(p.set_pitch(12), 12);
        assert_eq!(p.set_pitch(-99), -12);
        assert_eq!(p.set_pitch(99), 12);
    }

    #[test]
    fn live_bpm_applies_slider_like_apply_tempo() {
        let mut p = PlaybackState::new();
        p.reset_for_song(9600, 500_000.0, 32);
        // Initial label ignores the factor (initialBpm parity).
        assert!((p.bpm - 120.0).abs() < 1e-9);
        p.set_tempo_pct(200);
        assert!((p.live_bpm() - 240.0).abs() < 1e-9);
        p.set_tempo_pct(50);
        assert!((p.live_bpm() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn tempo_event_round_trips_through_base() {
        let mut p = PlaybackState::new();
        p.reset_for_song(9600, 500_000.0, 32);
        p.set_tempo_pct(200);
        // Engine event value already includes the factor (250000 = 500000/2).
        p.note_tempo_event(250_000.0);
        assert!((p.bpm - 240.0).abs() < 1e-9);
        // Raw base recovered: moving the slider back to 100 shows 120.
        p.set_tempo_pct(100);
        assert!((p.live_bpm() - 120.0).abs() < 1e-9);
    }

    #[test]
    fn position_fraction_guards_empty_song() {
        let mut p = PlaybackState::new();
        assert!((p.position_fraction() - 0.0).abs() < f64::EPSILON);
        p.song_len_ticks = 1000;
        p.position_ticks = 250;
        assert!((p.position_fraction() - 0.25).abs() < f64::EPSILON);
        p.position_ticks = 9999;
        assert!((p.position_fraction() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_for_song_keeps_sliders_syncs_loop() {
        let mut p = PlaybackState::new();
        p.set_tempo_pct(150);
        p.set_volume_pct(80);
        p.set_pitch(-2);
        p.loop_enabled = true;
        p.loop_start = 4;
        p.loop_end = 8;
        p.position_ticks = 1234;
        p.elapsed_ms = 9999;
        p.seeking = true;
        p.reset_for_song(9600, 500_000.0, 32);
        assert_eq!(p.tempo_factor_pct, 150);
        assert_eq!(p.volume_pct, 80);
        assert_eq!(p.pitch_semitones, -2);
        assert!(!p.loop_enabled);
        assert_eq!((p.loop_start, p.loop_end), (1, 32));
        assert_eq!(p.position_ticks, 0);
        assert_eq!(p.elapsed_ms, 0);
        assert!(!p.seeking);
        assert_eq!((p.bar, p.beat), (1, 1));
    }

    #[test]
    fn clock_format_matches_update_time() {
        assert_eq!(format_time_ms(0), "00:00:00");
        assert_eq!(format_time_ms(999), "00:00:00");
        assert_eq!(format_time_ms(61_000), "00:01:01");
        assert_eq!(format_time_ms(3_661_000), "01:01:01");
        assert_eq!(format_time_ms(36_000_000), "10:00:00");
        assert_eq!(format_bpm(120.0), "120.0 bpm");
        assert_eq!(format_bpm(120.06), "120.1 bpm");
    }

    /// Full `onFinished` matrix (`window.cpp:194-214`).
    #[test]
    fn finish_policy_matrix() {
        // Repeat-song wins over everything, no save-before-leave.
        for (auto, last) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(
                finish_action(RepeatMode::Song, auto, last),
                FinishAction::RepeatSong
            );
        }
        // Repeat-list wins over auto-advance, wraps (caller handles wrap).
        for (auto, last) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(
                finish_action(RepeatMode::Playlist, auto, last),
                FinishAction::RepeatList
            );
        }
        // Plain mode: auto-advance unless last or disabled.
        assert_eq!(
            finish_action(RepeatMode::Off, true, false),
            FinishAction::AutoAdvance
        );
        assert_eq!(
            finish_action(RepeatMode::Off, true, true),
            FinishAction::StayFinished
        );
        assert_eq!(
            finish_action(RepeatMode::Off, false, false),
            FinishAction::StayFinished
        );
        assert_eq!(
            finish_action(RepeatMode::Off, false, true),
            FinishAction::StayFinished
        );
    }
}
