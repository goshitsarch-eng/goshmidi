//! Views: transport, playlist, lyrics, channels, piano, dialogs, theme.
//!
//! T9 owns this contract + a compiling stub; ux implements the real
//! `transport` view next (header transport, dashboard, sliders, status).

pub mod transport;

use crate::message::Status;

/// View model for the transport/dashboard/sliders region (ux §3.2–3.4, §3.9).
/// Built fresh by `App::transport_view_model` on every `view()` and passed
/// BY VALUE to `ui::transport::view`; ux must not reach into `App` directly.
///
/// By value (not `&'a`) deliberately: a `view(&model)` signature borrowing a
/// fresh per-frame model cannot return its `Element` past the local, so the
/// reference form is uncallable here — owned data moves into the widgets and
/// the `Element` keeps the caller's lifetime.
///
/// Field contract (frozen for T9; additions need arch+ux sign-off):
/// * strings are pre-formatted display text (`HH:MM:SS`, `%.1f bpm`, `bar:beat`);
/// * percents are raw slider values in their C++ ranges (tempo 50–200,
///   volume 0–200, pitch −12–+12, position 0.0–1.0);
/// * `seeking` is informational only (view renders `position` verbatim; drag-follow is arch-side optimistic `position`);
/// * `beat` is the 1-based lit lamp, `beats` the lamp count (resize-to-meter);
/// * controls stay live like C++ (`has_song` retained in model, unused for gating); `loop_enabled` drives the toggle.
#[derive(Debug, Clone)]
pub struct TransportViewModel {
    /// Song title or `No file loaded`.
    pub song_title: String,
    /// Transport status (status line + button sensitivity).
    pub status: Status,
    /// Clock label `HH:MM:SS`.
    pub time_text: String,
    /// Tempo label `%.1f bpm` (live).
    pub tempo_text: String,
    /// Volume label `%d%%` (live).
    pub volume_text: String,
    /// Pitch spin value, −12–+12.
    pub pitch: i32,
    /// Position label `bar:beat`.
    pub bar_beat: String,
    /// Rhythm lamp count (time-signature beats).
    pub beats: u32,
    /// Lit lamp, 1-based (downbeat accents when `beat == 1`).
    pub beat: u32,
    /// Position slider 0.0–1.0.
    pub position: f64,
    /// Informational in-flight seek marker; view renders `position` verbatim.
    pub seeking: bool,
    /// Tempo slider 50–200.
    pub tempo_pct: u32,
    /// Volume slider 0–200.
    pub volume_pct: u32,
    /// Loop toggle state.
    pub loop_enabled: bool,
    /// Whether a song is loaded.
    pub has_song: bool,
}
