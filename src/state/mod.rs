//! UI state: playback, playlist, channels, lyrics, piano, MIDI (arch §2.1).
//!
//! `playlist.rs` lands in PLAN T8, the rest with T9/T11–T13.

// T8 lands ahead of its T9/T10 call sites, so the bin build sees this module
// as unused; drop the allow once the app shell wires it up.
#[allow(dead_code)]
pub mod playlist;
