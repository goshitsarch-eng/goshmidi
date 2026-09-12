//! Config: GKeyFile-compatible paths/settings/song-settings (arch §4).

// T8 lands ahead of its T9/T10 call sites, so the bin build sees these
// modules as unused; drop the allows once the app shell wires them up.
#[allow(dead_code)]
pub mod paths;
#[allow(dead_code)]
pub mod settings;
#[allow(dead_code)]
pub mod song;
