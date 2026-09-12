//! Piano roll state: note matrix, velocities, visibility, tighten (arch
//! §2.1, ux §3.8).
//!
//! The C++ twin is `m_notesOn[16][128]` / `m_noteVel[16][128]` /
//! `m_pianoVisible[16]` / `m_tightenKeys` (`src/ui/window.cpp:1948-2119`).
//! T9 owns the matrix updates (NoteOn/Off, stop/load clears, preview
//! lighting); T13a adds the pure geometry (drawn range, rows, click→note
//! mapping with the U12 drawn-range FIX), the A3 audition rules, and the
//! palette/velocity/names settings read path. Canvas rendering is the view
//! slice's (canvas API verdict: sufficient — see the T13a report).

use crate::core::event::MIDI_CHANNELS;

use super::lyrics::{GRAY_FALLBACK, Rgba, parse_gdk_rgba};

/// MIDI note range (`window.cpp:1979-1983` full range; tighten narrows it).
pub const PIANO_NOTES: usize = 128;

/// Full drawn range when tighten is off (`window.cpp:1980`).
pub const PIANO_LO: u8 = 21;
/// Full drawn range when tighten is off (`window.cpp:1980`).
pub const PIANO_HI: u8 = 108;

/// Audition velocity: click-preview sends NoteOn at 80 (`window.cpp:2099`).
pub const PREVIEW_VELOCITY: u8 = 80;

/// Minimum channel-row height in px (`window.cpp:1979,2067`).
pub const PIANO_MIN_ROW_H: i32 = 48;

/// Horizontal canvas margin in px: keys span `4..width-4` (`window.cpp:1991,2086`).
pub const PIANO_MARGIN_PX: f32 = 4.0;

/// Per-channel highlight palette (`channelColor`, `window.cpp:312-315`).
pub const CHANNEL_PALETTE: [&str; MIDI_CHANNELS] = [
    "#e01b24", "#ff7800", "#f6d32d", "#33d17a", "#3584e4", "#9141ac", "#c061cb", "#62a0ea",
    "#26a269", "#e5a50a", "#c01c28", "#1c71d8", "#613583", "#241f31", "#5e5c64", "#f66151",
];

/// Note-name syllables (`window.cpp:2017`).
pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Black-key test (`noteIsBlack`, `window.cpp:68-72`).
pub fn note_is_black(midi: u8) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

/// Drawn note range, inclusive (`window.cpp:1980-1984`): full 21–108, or —
/// when tighten is on and a song is loaded — the song min/max padded by 2
/// and clamped to 0–127.
///
/// A song with events but no notes reports lowest 127 / highest 0 (the C++
/// inits, `sequence.hpp:133-134`), which inverts the range (`lo > hi`); C++
/// then draws no keys (empty loops, `whites` forced to 1 for the width div).
/// The inversion is preserved here — [`count_whites`] yields 0 and
/// [`click_to_note`] yields `None` — and pinned by test.
pub fn drawn_range(tighten: bool, song_empty: bool, lowest: i32, highest: i32) -> (u8, u8) {
    if tighten && !song_empty {
        let lo = (lowest - 2).clamp(0, 127) as u8;
        let hi = (highest + 2).clamp(0, 127) as u8;
        (lo, hi)
    } else {
        (PIANO_LO, PIANO_HI)
    }
}

/// White-key count in an inclusive range (0 when `lo > hi`).
pub fn count_whites(lo: u8, hi: u8) -> usize {
    if lo > hi {
        return 0;
    }
    (lo..=hi).filter(|n| !note_is_black(*n)).count()
}

/// White-key width in px: `(width - 8) / max(1, whites)` (`window.cpp:1991`).
pub fn white_width_px(width_px: f32, whites: usize) -> f32 {
    (width_px - 2.0 * PIANO_MARGIN_PX) / whites.max(1) as f32
}

/// Channels drawn as rows, ascending (`window.cpp:1995`): channel 0 alone
/// when no song is loaded, else every used-and-visible channel.
pub fn draw_rows(
    song_empty: bool,
    used: &[bool; MIDI_CHANNELS],
    visible: &[bool; MIDI_CHANNELS],
) -> Vec<u8> {
    if song_empty {
        return vec![0];
    }
    used.iter()
        .zip(visible.iter())
        .enumerate()
        .filter(|(_, (u, v))| **u && **v)
        .map(|(ch, _)| ch as u8)
        .collect()
}

/// Channels that accept click-preview, ascending. Unlike [`draw_rows`], an
/// empty song (or no used+visible channel) yields NO rows: the C++ click
/// handler returns early when `used == 0` (`window.cpp:2063-2064`), so there
/// is no audition without a song even though the draw path shows ch 1.
pub fn click_rows(
    song_empty: bool,
    used: &[bool; MIDI_CHANNELS],
    visible: &[bool; MIDI_CHANNELS],
) -> Vec<u8> {
    if song_empty {
        return Vec::new();
    }
    draw_rows(false, used, visible)
}

/// Row height in px: `max(48, height / max(1, rows))`, truncating integer
/// division like C++ (`window.cpp:1979,2067`).
pub fn row_height_px(height_px: i32, rows: usize) -> i32 {
    PIANO_MIN_ROW_H.max(height_px / rows.max(1) as i32)
}

/// Click x → white-key note in the DRAWN range, or `None` when the click
/// lands outside the keys (a full key-width left of the margin, past the
/// last key, empty range, or a canvas too narrow to hold keys). Like C++,
/// the slot truncates toward zero, so clicks just inside the 4px margin
/// still hit slot 0 (generous hit area — preserved, out of U12 scope).
///
/// U12 FIX: C++ maps clicks over the FIXED 21–108 range (`window.cpp:2081-
/// 2086`) while drawing the tightened range, so tightened clicks audition
/// the wrong note. The port maps in the drawn `(lo, hi)` instead. Black keys
/// are not hit-tested (C++ `2087-2097` walks white keys only — a click over
/// a black key yields the white key at that slot); preserved.
///
/// OOR improvement inside the FIX: C++ defaults an out-of-range slot to
/// `note = lo` and auditions it anyway; the port yields `None` (no wrong
/// note).
pub fn click_to_note(x_px: f32, width_px: f32, lo: u8, hi: u8) -> Option<u8> {
    let whites = count_whites(lo, hi);
    if whites == 0 {
        return None;
    }
    let ww = white_width_px(width_px, whites);
    if !ww.is_finite() || ww <= 0.0 {
        return None;
    }
    // Truncation toward zero, like C++ `static_cast<int>((x - 4) / ww)`.
    let idx = ((x_px - PIANO_MARGIN_PX) / ww).trunc() as i32;
    if idx < 0 || idx >= whites as i32 {
        return None;
    }
    (lo..=hi).filter(|n| !note_is_black(*n)).nth(idx as usize)
}

/// Click y → channel row, or `None` with no rows / a full row-height (or
/// more) above the first row (`window.cpp:2068-2078`: `row =
/// min(used-1, y / rowH)` truncating toward zero, then the nth used+visible
/// channel; a negative row matches nothing).
pub fn click_to_channel(y_px: f32, row_height_px: i32, rows: &[u8]) -> Option<u8> {
    if rows.is_empty() || row_height_px <= 0 {
        return None;
    }
    // Truncation toward zero, like C++ `static_cast<int>(y / rowH)`.
    let row = (y_px / row_height_px as f32).trunc() as i32;
    if row < 0 {
        return None;
    }
    let row = (row as usize).min(rows.len() - 1);
    Some(rows[row])
}

/// Combined click → `(channel, note)` audition target, or `None` when either
/// axis misses.
pub fn click_to_preview(
    x_px: f32,
    y_px: f32,
    width_px: f32,
    row_height_px: i32,
    lo: u8,
    hi: u8,
    rows: &[u8],
) -> Option<(u8, u8)> {
    let ch = click_to_channel(y_px, row_height_px, rows)?;
    let note = click_to_note(x_px, width_px, lo, hi)?;
    Some((ch, note))
}

/// Audition note (D19 refinement): piano preview bypasses the player MIX
/// and goes straight to the outputs (`window.cpp:2098-2102`:
/// `sendNoteOn(ch, note, 80)`, bypassing mute and pitch shift — A3
/// PRESERVE). The outputs live engine-side, so composition sends this spec
/// as the engine-carried `EngineCommand::PreviewNote`, whose handler writes
/// straight to the current output without touching mix/sequencing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewNote {
    pub ch: u8,
    pub note: u8,
    pub vel: u8,
}

impl PreviewNote {
    /// Audition spec for a clicked key: fixed velocity 80.
    pub fn audition(ch: u8, note: u8) -> Self {
        Self {
            ch,
            note,
            vel: PREVIEW_VELOCITY,
        }
    }
}

/// Note-off targets for a preview release, in matrix order: every lit key —
/// but ONLY when the player is idle (`window.cpp:2105-2116`: the release
/// handler skips all note-offs while `isRunning()`, so releasing a preview
/// mid-song never kills song notes — A3 PRESERVE). Composition sends
/// `sendNoteOff(ch, note, 0)` for each and unlights the matrix.
pub fn preview_off_targets(
    notes_on: &[[bool; PIANO_NOTES]; MIDI_CHANNELS],
    player_running: bool,
) -> Vec<(u8, u8)> {
    if player_running {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (ch, keys) in notes_on.iter().enumerate() {
        for (note, lit) in keys.iter().enumerate() {
            if *lit {
                out.push((ch as u8, note as u8));
            }
        }
    }
    out
}

/// Lit-key highlight color (`channelColor`, `window.cpp:310-326`): the
/// per-channel palette entry, or `single_color` when `highlight_palette ==
/// 0` (every other palette value — Double/Channels/Scale AND out-of-range —
/// renders the same per-channel palette; only 0 differs — preserved), then
/// the velocity tint (`rgb *= 0.4 + 0.6 * vel/127`) when `velocity_color`.
///
/// Parse failures (including a garbage `single_color`) fall back to gray,
/// like C++ `parseColor`'s surviving `(0.5,0.5,0.5,1)` init. Alpha is forced
/// to 1.0: C++ paints with `cairo_set_source_rgb`, ignoring alpha.
pub fn highlight_rgba(
    ch: u8,
    vel: u8,
    highlight_palette: i32,
    single_color: &str,
    velocity_color: bool,
) -> Rgba {
    let base = if highlight_palette == 0 {
        parse_gdk_rgba(single_color).unwrap_or(GRAY_FALLBACK)
    } else {
        // Palette constants always parse; the fallback is unreachable.
        parse_gdk_rgba(CHANNEL_PALETTE[(ch & 15) as usize]).unwrap_or(GRAY_FALLBACK)
    };
    let mut out = Rgba {
        r: base.r,
        g: base.g,
        b: base.b,
        a: 1.0,
    };
    if velocity_color {
        let t = f32::from(vel) / 127.0;
        let k = 0.4 + 0.6 * t;
        out.r *= k;
        out.g *= k;
        out.b *= k;
    }
    out
}

/// Note-name label visibility (`window.cpp:2012-2013`): 3 always, 1 minimal
/// (C notes only), 2 when the key is lit; anything else (incl. out-of-range)
/// shows nothing — preserved.
pub fn show_note_name(note: u8, lit: bool, names_visibility: i32) -> bool {
    names_visibility == 3
        || (names_visibility == 1 && note.is_multiple_of(12))
        || (names_visibility == 2 && lit)
}

/// Note-name label: syllable + octave (`C4` for middle C, `window.cpp:2017-
/// 2023`: `oct = n / 12 - 1`).
///
/// NOTE: C++ formats the label identically whether `octaveSubscript` is on
/// or off (both branches emit `"%s%d"`) — the setting is draw-dead there.
/// The port keeps the single shape; no flag is taken.
pub fn note_label(note: u8) -> String {
    let octave = i32::from(note) / 12 - 1;
    format!("{}{octave}", NOTE_NAMES[(note % 12) as usize])
}

/// Piano page state (arch §2.1 `PianoState`).
#[allow(dead_code)] // T13 canvas reads the matrix; T9 owns the updates.
#[derive(Debug, Clone)]
pub struct PianoState {
    /// Lit keys per channel (`m_notesOn`).
    pub notes_on: [[bool; PIANO_NOTES]; MIDI_CHANNELS],
    /// Last velocity per key (`m_noteVel`, for velocity tinting).
    pub note_vel: [[u8; PIANO_NOTES]; MIDI_CHANNELS],
    /// Per-channel row visibility (`m_pianoVisible`): all true at startup,
    /// reset on song load; Show-all / Hide-unused only (no per-channel
    /// toggle exists in C++ — arch §1.7).
    pub visible: [bool; MIDI_CHANNELS],
    /// Tighten keys to the song range (`m_tightenKeys`).
    pub tighten: bool,
}

impl Default for PianoState {
    fn default() -> Self {
        Self {
            notes_on: [[false; PIANO_NOTES]; MIDI_CHANNELS],
            note_vel: [[0; PIANO_NOTES]; MIDI_CHANNELS],
            visible: [true; MIDI_CHANNELS],
            tighten: false,
        }
    }
}

impl PianoState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Light a key (`onNoteOn`: `m_notesOn[ch][note] = vel > 0`,
    /// `window.cpp:144-151`). Out-of-range notes are ignored, as in C++.
    pub fn note_on(&mut self, ch: u8, note: u8, vel: u8) {
        let (ch, note) = (ch as usize, note as usize);
        if ch < MIDI_CHANNELS && note < PIANO_NOTES {
            self.notes_on[ch][note] = vel > 0;
            self.note_vel[ch][note] = vel;
        }
    }

    /// Unlight a key (`onNoteOff`, `window.cpp:152-156`).
    pub fn note_off(&mut self, ch: u8, note: u8) {
        let (ch, note) = (ch as usize, note as usize);
        if ch < MIDI_CHANNELS && note < PIANO_NOTES {
            self.notes_on[ch][note] = false;
        }
    }

    /// Clear the whole matrix (stop / song load, `window.cpp:394,435`).
    pub fn clear(&mut self) {
        self.notes_on = [[false; PIANO_NOTES]; MIDI_CHANNELS];
    }

    /// Reset on song load: matrix cleared, all channels visible
    /// (`refreshChannels`, `window.cpp:543`).
    pub fn reset_for_song(&mut self) {
        self.clear();
        self.visible = [true; MIDI_CHANNELS];
    }

    /// `Show all`: every channel visible.
    pub fn show_all(&mut self) {
        self.visible = [true; MIDI_CHANNELS];
    }

    /// `Hide unused`: visibility follows channel usage.
    pub fn hide_unused(&mut self, used: &[bool; MIDI_CHANNELS]) {
        self.visible = *used;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_cpp_init() {
        let p = PianoState::new();
        assert!(p.notes_on.iter().all(|ch| ch.iter().all(|&n| !n)));
        assert!(p.visible.iter().all(|&v| v));
        assert!(!p.tighten);
    }

    #[test]
    fn note_on_off_matrix_and_vel() {
        let mut p = PianoState::new();
        p.note_on(0, 60, 100);
        assert!(p.notes_on[0][60]);
        assert_eq!(p.note_vel[0][60], 100);
        // Velocity-0 NoteOn unlights (C++ `vel > 0` rule).
        p.note_on(0, 60, 0);
        assert!(!p.notes_on[0][60]);
        assert_eq!(p.note_vel[0][60], 0);
        p.note_on(3, 64, 80);
        p.note_off(3, 64);
        assert!(!p.notes_on[3][64]);
    }

    #[test]
    fn out_of_range_notes_are_ignored() {
        let mut p = PianoState::new();
        p.note_on(16, 60, 100);
        p.note_on(0, 128, 100);
        p.note_off(99, 99);
        assert!(p.notes_on.iter().all(|ch| ch.iter().all(|&n| !n)));
    }

    #[test]
    fn show_all_and_hide_unused() {
        let mut p = PianoState::new();
        let used: [bool; MIDI_CHANNELS] = std::array::from_fn(|ch| ch % 2 == 0);
        p.hide_unused(&used);
        assert_eq!(p.visible, used);
        p.show_all();
        assert!(p.visible.iter().all(|&v| v));
    }

    #[test]
    fn reset_for_song_clears_and_shows_all() {
        let mut p = PianoState::new();
        p.note_on(0, 60, 100);
        p.visible[0] = false;
        p.reset_for_song();
        assert!(!p.notes_on[0][60]);
        assert!(p.visible.iter().all(|&v| v));
    }

    #[test]
    fn black_key_pattern_matches_cpp() {
        // One octave: C D E F G A B white; C# D# F# G# A# black.
        let black = [1, 3, 6, 8, 10];
        for n in 0..12u8 {
            assert_eq!(note_is_black(n), black.contains(&n), "note {n}");
            assert_eq!(note_is_black(n + 60), black.contains(&n), "note {}", n + 60);
        }
        assert!(!note_is_black(21)); // A0, range start
        assert!(note_is_black(22)); // A#0
        assert!(!note_is_black(108)); // C8, range end
    }

    #[test]
    fn drawn_range_full_and_tightened() {
        // Tighten off (or no song): full 21–108 whatever the song range.
        assert_eq!(drawn_range(false, false, 60, 72), (21, 108));
        assert_eq!(drawn_range(true, true, 60, 72), (21, 108));
        assert_eq!(drawn_range(false, true, 0, 0), (21, 108));
        // Tighten on: song min/max ± 2.
        assert_eq!(drawn_range(true, false, 60, 72), (58, 74));
        // Clamped to 0–127.
        assert_eq!(drawn_range(true, false, 0, 127), (0, 127));
        assert_eq!(drawn_range(true, false, 1, 126), (0, 127));
        // Corrupt song range stays raw (defensive; decoders never emit it).
        assert_eq!(drawn_range(true, false, -50, 999), (0, 127));
    }

    #[test]
    fn drawn_range_inverts_for_noteless_songs() {
        // Events but no notes: lowest 127 / highest 0 (C++ inits) → lo > hi.
        let (lo, hi) = drawn_range(true, false, 127, 0);
        assert_eq!((lo, hi), (125, 2));
        assert_eq!(count_whites(lo, hi), 0);
        assert_eq!(click_to_note(100.0, 800.0, lo, hi), None);
    }

    #[test]
    fn white_count_and_width_cover_full_range() {
        // 21–108: A0–C8 = 7 octaves + C8 → 52 white keys (piano standard).
        assert_eq!(count_whites(21, 108), 52);
        assert_eq!(count_whites(60, 60), 1); // middle C
        assert_eq!(count_whites(61, 61), 0); // C#4
        assert_eq!(count_whites(60, 59), 0); // inverted
        assert!((white_width_px(812.0, 52) - 804.0 / 52.0).abs() < 1e-6);
        assert!((white_width_px(812.0, 0) - 804.0).abs() < 1e-6); // max(1) guard
    }

    #[test]
    fn rows_draw_vs_click_differ_on_empty_song() {
        let used: [bool; MIDI_CHANNELS] = std::array::from_fn(|ch| ch < 3);
        let mut visible = [true; MIDI_CHANNELS];
        visible[1] = false;
        // Draw: ch 1 alone when empty; used+visible otherwise.
        assert_eq!(draw_rows(true, &used, &visible), vec![0]);
        assert_eq!(draw_rows(false, &used, &visible), vec![0, 2]);
        // Click: never without a song; same filter otherwise.
        assert!(click_rows(true, &used, &visible).is_empty());
        assert_eq!(click_rows(false, &used, &visible), vec![0, 2]);
        // Nothing used+visible → no rows anywhere but the empty-song draw.
        let none = [false; MIDI_CHANNELS];
        assert!(draw_rows(false, &none, &visible).is_empty());
        assert!(click_rows(false, &used, &none).is_empty());
    }

    #[test]
    fn row_height_clamps_to_48_with_truncation() {
        assert_eq!(row_height_px(480, 4), 120);
        assert_eq!(row_height_px(100, 4), 48); // max(48, 25)
        assert_eq!(row_height_px(47, 1), 48);
        assert_eq!(row_height_px(500, 3), 166); // 500/3 truncates
        assert_eq!(row_height_px(480, 0), 480); // rows forced to 1
        assert_eq!(row_height_px(0, 4), 48);
    }

    /// U12 core: with tighten drawn, a click maps in the DRAWN range — C++
    /// would map the same x in fixed 21–108 and audition the wrong note.
    #[test]
    fn click_maps_in_drawn_range_u12() {
        // Tightened to one octave 60–72 (8 whites), 804px canvas → 100px keys.
        let (lo, hi) = drawn_range(true, false, 60, 72);
        assert_eq!((lo, hi), (58, 74));
        let whites = count_whites(lo, hi);
        let width = 8.0 + whites as f32 * 100.0;
        // 58 is A#3 (black), so slot 0 is the first white ≥ 58: B3=59.
        assert_eq!(click_to_note(4.0, width, lo, hi), Some(59));
        assert_eq!(click_to_note(103.0, width, lo, hi), Some(59)); // same slot
        assert_eq!(click_to_note(104.0, width, lo, hi), Some(60)); // next slot
        // Same x in C++'s fixed 21–108 mapping would be slot 0 → A0=21.
        assert_eq!(click_to_note(4.0, width, 21, 108), Some(21));
        // Last slot → last white ≤ 74 (D5=74).
        let last_x = 4.0 + whites as f32 * 100.0 - 1.0;
        assert_eq!(click_to_note(last_x, width, lo, hi), Some(74));
    }

    #[test]
    fn click_to_note_edges_and_oor() {
        let width = 8.0 + 52.0 * 10.0; // full range, 10px white keys
        assert_eq!(click_to_note(4.0, width, 21, 108), Some(21));
        assert_eq!(click_to_note(13.9, width, 21, 108), Some(21));
        assert_eq!(click_to_note(14.0, width, 21, 108), Some(23)); // B0 (22 is A#)
        // Black keys are not hit-tested: x over A#0 still yields the slot's white.
        assert_eq!(click_to_note(5.0, width, 21, 108), Some(21));
        // Truncation toward zero (C++ parity): just inside the 4px margin
        // still hits slot 0; a full key-width left of it misses.
        assert_eq!(click_to_note(3.9, width, 21, 108), Some(21));
        assert_eq!(click_to_note(-5.9, width, 21, 108), Some(21));
        assert_eq!(click_to_note(-6.1, width, 21, 108), None);
        // Out-of-range clicks audition nothing (C++ would audition lo).
        assert_eq!(click_to_note(-50.0, width, 21, 108), None);
        assert_eq!(click_to_note(width - 4.0, width, 21, 108), None);
        assert_eq!(click_to_note(width + 99.0, width, 21, 108), None);
        // Right-edge slot boundary: last px of the last key still hits.
        assert_eq!(click_to_note(width - 5.0, width, 21, 108), Some(108));
        // Degenerate canvases/ranges.
        assert_eq!(click_to_note(4.0, 8.0, 21, 108), None); // ww = 0
        assert_eq!(click_to_note(4.0, 0.0, 21, 108), None); // ww < 0
        assert_eq!(click_to_note(4.0, width, 60, 59), None); // inverted
        assert_eq!(click_to_note(4.0, width, 61, 61), None); // no whites
    }

    #[test]
    fn click_to_channel_rows_and_oor() {
        let rows = vec![0u8, 2, 5];
        assert_eq!(click_to_channel(0.0, 100, &rows), Some(0));
        assert_eq!(click_to_channel(99.9, 100, &rows), Some(0));
        assert_eq!(click_to_channel(100.0, 100, &rows), Some(2));
        assert_eq!(click_to_channel(250.0, 100, &rows), Some(5));
        // Past the last row clamps (C++ min(used-1, …)).
        assert_eq!(click_to_channel(9999.0, 100, &rows), Some(5));
        // Truncation toward zero (C++ parity): marginally above still row 0;
        // a full row-height above misses.
        assert_eq!(click_to_channel(-0.1, 100, &rows), Some(0));
        assert_eq!(click_to_channel(-100.0, 100, &rows), None);
        assert_eq!(click_to_channel(-250.0, 100, &rows), None);
        // No rows / dead height → None.
        assert_eq!(click_to_channel(10.0, 100, &[]), None);
        assert_eq!(click_to_channel(10.0, 0, &rows), None);
        assert_eq!(click_to_channel(10.0, -5, &rows), None);
    }

    #[test]
    fn click_to_preview_combines_axes() {
        let width = 8.0 + 52.0 * 10.0;
        let rows = vec![0u8, 2];
        assert_eq!(
            click_to_preview(4.0, 10.0, width, 100, 21, 108, &rows),
            Some((0, 21))
        );
        assert_eq!(
            click_to_preview(4.0, 150.0, width, 100, 21, 108, &rows),
            Some((2, 21))
        );
        assert_eq!(
            click_to_preview(-7.0, 10.0, width, 100, 21, 108, &rows),
            None
        );
        assert_eq!(
            click_to_preview(4.0, -100.0, width, 100, 21, 108, &rows),
            None
        );
        assert_eq!(click_to_preview(4.0, 10.0, width, 100, 21, 108, &[]), None);
    }

    #[test]
    fn preview_note_fixes_velocity_80() {
        assert_eq!(
            PreviewNote::audition(3, 64),
            PreviewNote {
                ch: 3,
                note: 64,
                vel: 80
            }
        );
        assert_eq!(PREVIEW_VELOCITY, 80);
    }

    #[test]
    fn preview_off_is_idle_only() {
        let mut on = [[false; PIANO_NOTES]; MIDI_CHANNELS];
        on[0][60] = true;
        on[3][64] = true;
        // Idle → every lit key, matrix order.
        assert_eq!(preview_off_targets(&on, false), vec![(0, 60), (3, 64)]);
        // Running → nothing (song notes survive a preview release).
        assert!(preview_off_targets(&on, true).is_empty());
        // Idle + nothing lit → nothing.
        let off = [[false; PIANO_NOTES]; MIDI_CHANNELS];
        assert!(preview_off_targets(&off, false).is_empty());
    }

    fn assert_rgba_near(got: Rgba, want: (f32, f32, f32, f32)) {
        for (g, w) in [
            (got.r, want.0),
            (got.g, want.1),
            (got.b, want.2),
            (got.a, want.3),
        ] {
            assert!((g - w).abs() < 1e-4, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn highlight_palette_and_single_paths() {
        // Palette entry 0 = #e01b24, no velocity tint.
        assert_rgba_near(
            highlight_rgba(0, 100, 2, "#f6d32d", false),
            (
                0xe0 as f32 / 255.0,
                0x1b as f32 / 255.0,
                0x24 as f32 / 255.0,
                1.0,
            ),
        );
        // Single-color mode reads the setting.
        assert_rgba_near(
            highlight_rgba(7, 100, 0, "#ff7800", false),
            (1.0, 0x78 as f32 / 255.0, 0.0, 1.0),
        );
        // Only palette 0 differs: 1/2/3 AND out-of-range all use per-channel.
        let ch5 = highlight_rgba(5, 100, 2, "#ff7800", false);
        for pal in [1, 2, 3, -1, 99] {
            assert_eq!(highlight_rgba(5, 100, pal, "#ff7800", false), ch5);
        }
        // Channel masks to 4 bits like C++ `pal[ch & 15]`.
        assert_eq!(
            highlight_rgba(16, 100, 2, "", false),
            highlight_rgba(0, 100, 2, "", false)
        );
        // Garbage single color → gray (C++ untouched-init parity).
        assert_rgba_near(
            highlight_rgba(0, 100, 0, "notacolor", false),
            (0.5, 0.5, 0.5, 1.0),
        );
        assert_rgba_near(highlight_rgba(0, 100, 0, "", false), (0.5, 0.5, 0.5, 1.0));
        // Alpha forced opaque (C++ set_source_rgb ignores alpha).
        assert_rgba_near(
            highlight_rgba(0, 100, 0, "rgba(255,0,0,0.25)", false),
            (1.0, 0.0, 0.0, 1.0),
        );
    }

    #[test]
    fn highlight_velocity_tint_scales_rgb() {
        // vel 127 → ×1.0; vel 0 → ×0.4; vel 64 → ×(0.4+0.6·64/127).
        let full = highlight_rgba(0, 127, 2, "", true);
        let plain = highlight_rgba(0, 127, 2, "", false);
        assert_eq!(full, plain);
        let dark = highlight_rgba(0, 0, 2, "", true);
        assert_rgba_near(dark, (plain.r * 0.4, plain.g * 0.4, plain.b * 0.4, 1.0));
        let mid = highlight_rgba(0, 64, 2, "", true);
        let k = 0.4 + 0.6 * (64.0 / 127.0);
        assert_rgba_near(mid, (plain.r * k, plain.g * k, plain.b * k, 1.0));
        // Tint off → velocity ignored.
        assert_eq!(
            highlight_rgba(0, 0, 2, "", false),
            highlight_rgba(0, 127, 2, "", false)
        );
    }

    #[test]
    fn note_name_visibility_matrix() {
        // 3 always; 1 C-only; 2 when lit; else never.
        for lit in [false, true] {
            assert!(show_note_name(61, lit, 3));
            assert!(show_note_name(60, lit, 1));
            assert!(!show_note_name(61, lit, 1));
            assert_eq!(show_note_name(61, lit, 2), lit);
            for vis in [0, -1, 4, 99] {
                assert!(!show_note_name(60, lit, vis), "vis {vis} lit {lit}");
            }
        }
        // C boundary: 0 and 120 are C; 11 and 119 are B.
        assert!(show_note_name(0, false, 1));
        assert!(show_note_name(120, false, 1));
        assert!(!show_note_name(11, false, 1));
    }

    #[test]
    fn note_labels_match_cpp_octaves() {
        assert_eq!(note_label(60), "C4");
        assert_eq!(note_label(61), "C#4");
        assert_eq!(note_label(69), "A4");
        assert_eq!(note_label(21), "A0");
        assert_eq!(note_label(108), "C8");
        assert_eq!(note_label(0), "C-1");
        assert_eq!(note_label(127), "G9");
    }
}
