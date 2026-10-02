//! Portable application enums shared by state, commands, and the frontend.

/// Transport status line: the 5 C++ states verbatim (ux §3.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Ready,
    Playing,
    Paused,
    Stopped,
    Finished,
}

impl Status {
    /// Status-line text, exactly the C++ `setStatus` strings.
    pub fn text(self) -> &'static str {
        match self {
            Status::Ready => "Ready",
            Status::Playing => "Playing",
            Status::Paused => "Paused",
            Status::Stopped => "Stopped",
            Status::Finished => "Finished",
        }
    }
}

/// Repeat policy: off / repeat song / repeat playlist (persisted to
/// `repeatMode`: `Off = 0`, `Song = 1`, `Playlist = 2`, like `m_repeat`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    Song,
    Playlist,
}

impl RepeatMode {
    /// `m_repeat` integer twin (window.cpp:2140-2152, `repeatMode` key).
    pub fn as_i32(&self) -> i32 {
        match self {
            RepeatMode::Off => 0,
            RepeatMode::Song => 1,
            RepeatMode::Playlist => 2,
        }
    }

    /// Inverse of [`RepeatMode::as_i32`]; out-of-range reads as `Off`.
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => RepeatMode::Song,
            2 => RepeatMode::Playlist,
            _ => RepeatMode::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Lyrics,
    Channels,
    Piano,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    Preferences,
    MidiSetup,
    FileInfo,
    About,
    Help,
    Jump,
    Loop,
    Unsaved,
    SaveFailure,
}
