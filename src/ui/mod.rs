//! Views: transport, playlist sidebar, lyrics, channels, piano, dialogs, theme.
//!
//! T9 landed the `transport` view + the path-hack-declared `theme` styles;
//! T10b adds the real `sidebar` view and promotes `theme` to `ui::theme`;
//! T11b adds the `lyrics` page view + D2 Lyrics prefs rows.

pub mod lyrics;
pub mod sidebar;
pub mod theme;
pub mod transport;

use std::path::PathBuf;

use crate::message::Status;
use crate::state::lyrics::AppliedLyricsPrefs;

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

/// View model for the playlist sidebar (ux §3.1). Built fresh by
/// `App::playlist_view_model` on every `view()` and passed BY VALUE to
/// `ui::sidebar::view`; ux must not reach into `App` directly.
///
/// By value (not `&'a`) deliberately, same rationale as
/// [`TransportViewModel`]: a `view(&model)` signature borrowing a fresh
/// per-frame model cannot return its `Element` past the local, so the
/// reference form is uncallable here — owned data moves into the widgets
/// and the `Element` keeps the caller's lifetime.
///
/// Field contract (frozen for T10; additions need arch+ux sign-off):
/// * `items` are full paths in playlist order; the view renders basenames
///   (`file_name`, middle-ellipsized) and never mutates order;
/// * `selected` is `playlist.current_index()` as `Option<usize>` (`-1` →
///   `None`); it drives the highlight and Remove/Up/Down sensitivity.
///   Out-of-range values highlight nothing (defensive; the state
///   guarantees in-range-or-`-1`, and its guards no-op regardless).
#[derive(Debug, Clone, Default)]
pub struct PlaylistViewModel {
    /// Playlist items, full paths in order.
    pub items: Vec<PathBuf>,
    /// Selected row (`current_index`, `-1` → `None`).
    pub selected: Option<usize>,
}

/// View model for the lyrics page (ux §3.6). Built fresh by
/// `App::lyrics_view_model` on every `view()` and passed BY VALUE to
/// `ui::lyrics::view`; ux must not reach into `App` directly.
///
/// By value (not `&'a`) deliberately, same rationale as
/// [`TransportViewModel`]: a `view(&model)` signature borrowing a fresh
/// per-frame model cannot return its `Element` past the local, so the
/// reference form is uncallable here — owned data moves into the widgets
/// and the `Element` keeps the caller's lifetime.
///
/// Field contract (frozen for T11; additions need arch+ux sign-off):
/// * dropdown options are owned labels; `*_selected` are option indices —
///   arch computes them per the T11a formulas (`track_filter.map_or(0)`,
///   `type_filter`, `codec_option_index(&codec)`) and the view re-clamps
///   defensively (out-of-range → nearest valid, empty options → no
///   selection), so a stale index can never panic the dropdown;
/// * `body`/`live_past`/`live_current` are the U10 segments verbatim
///   (prefill + live tail, duplication preserved state-side); `sung`
///   flips `body` from the future color to the past color;
/// * `live_seq` is the snap trigger generation (arch's live-text arm
///   calls [`lyrics::snap_to_end_task`] when `append_live` accepts);
///   `autoscroll` gates that call (always true — there is no C++ toggle
///   surface, so the view ignores it and arch honors it);
/// * `applied` is the U14 applied D2 Lyrics rows (never settings strings —
///   the view parses nothing per frame);
/// * `has_song` selects the empty states (no-song placeholder vs body).
///
/// [`lyrics::snap_to_end_task`]: lyrics::snap_to_end_task
#[derive(Debug, Clone)]
pub struct LyricsViewModel {
    /// Track dropdown labels (`track_options`: `All tracks` + `Track N …`).
    pub track_options: Vec<String>,
    /// Track dropdown selection (`track_filter.map_or(0)`).
    pub track_selected: usize,
    /// Type dropdown labels (`TYPE_OPTION_NAMES`, 14 entries).
    pub type_options: Vec<String>,
    /// Type dropdown selection (`type_filter` discriminant as index).
    pub type_selected: usize,
    /// Encoding dropdown labels (`Sequence::extra_codec_names()`, 14).
    pub codec_options: Vec<String>,
    /// Encoding dropdown selection (`codec_option_index(&codec)`).
    pub codec_selected: usize,
    /// Filtered static text (`refreshLyrics` body half).
    pub body: String,
    /// Earlier live appends (past color).
    pub live_past: String,
    /// Newest live append (highlight color).
    pub live_current: String,
    /// Whether any live event passed the filters yet (flips `body` past).
    pub sung: bool,
    /// Live-append generation (snap trigger, monotonic within a song).
    pub live_seq: u64,
    /// Snap-to-end gate (always true; honored arch-side, ignored here).
    pub autoscroll: bool,
    /// Applied D2 Lyrics rows (colors/font/alignment).
    pub applied: AppliedLyricsPrefs,
    /// Whether a song is loaded (empty-state switch).
    pub has_song: bool,
}
