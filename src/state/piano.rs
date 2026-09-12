//! Piano roll state: note matrix, velocities, visibility, tighten (arch
//! §2.1, ux §3.8).
//!
//! The C++ twin is `m_notesOn[16][128]` / `m_noteVel[16][128]` /
//! `m_pianoVisible[16]` / `m_tightenKeys` (`src/ui/window.cpp:1948-2119`).
//! Geometry + canvas rendering land in T13; T9 owns the matrix updates
//! (NoteOn/Off, stop/load clears, preview lighting).

use crate::core::event::MIDI_CHANNELS;

/// MIDI note range (`window.cpp:1979-1983` full range; tighten narrows it).
pub const PIANO_NOTES: usize = 128;

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
}
