//! UI state: playback, playlist, channels, lyrics, piano, MIDI (arch §2.1).
//!
//! `playlist.rs` landed in PLAN T8; playback/channels/lyrics/piano/midi in T9.

pub mod channels;
pub mod lyrics;
pub mod midi;
pub mod piano;
pub mod playback;
// T10 view getters (`items`, `at_first`, `source_path`, …) + `CloseSave` and
// `save` land with the sidebar/close wiring; drop this allow then.
#[allow(dead_code)]
pub mod playlist;
