//! Shared action inventory: menus, toolbars, and shortcuts dispatch these commands.
use crate::models::{Dialog, RepeatMode, Theme, View};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Command {
    Action(Action),
    OpenFiles(Vec<PathBuf>, bool),
    Select(usize, bool),
    Remove(usize),
    Move(usize, i32),
    PlaylistLoaded(PathBuf, Vec<PathBuf>, bool),
    PlaylistSaved(PathBuf),
    Tempo(u32),
    Volume(u32),
    Pitch(i32),
    Seek(f64),
    Jump(u32),
    Loop(u32, u32),
    LoopOff,
    Repeat(RepeatMode),
    View(View),
    ChannelMute(u8, bool),
    ChannelSolo(u8, bool),
    ChannelLock(u8, bool),
    ChannelPatch(u8, u8),
    ChannelLevel(u8, u32),
    ChannelName(u8, String),
    LyricTrack(Option<u32>),
    LyricType(i32),
    LyricCodec(String),
    Autoscroll(bool),
    Tighten(bool),
    PianoShowAll,
    PianoHideUnused,
    Preview(u8, u8),
    PreviewOff,
    PreviewExpired(u64),
    Dialog(Dialog),
    CancelDialog,
    EditPreference(Preference, String),
    ApplyPreferences,
    MidiBackend(String),
    MidiPort(String),
    MidiAdvanced(bool),
    MidiMap(u8),
    MidiReset(u8),
    MidiSoundfont(String),
    MidiPorts {
        backend: String,
        advanced: bool,
        ports: Vec<crate::core::output::MidiPort>,
    },
    MidiApply,
    SongSettingsLoaded(PathBuf, Box<crate::config::song::SongSettings>),
    SongSettingsAbsent(PathBuf),
    Theme(Theme),
    Geometry(u32, u32, bool),
    WindowPosition(i32, i32),
    ToggleSidebar,
    DismissError,
    Error(String),
    Quit,
    QuitSaveFailed(String),
    QuitWithoutSaving,
    SaveCompleted(String),
    ConfirmDiscard,
    OpenUrl(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Add,
    OpenPlaylist,
    SavePlaylist,
    FileInfo,
    LoadSettings,
    SaveSettings,
    Search,
    Quit,
    Play,
    Pause,
    Stop,
    Previous,
    Next,
    Rewind,
    Forward,
    Jump,
    Loop,
    RepeatOff,
    RepeatSong,
    RepeatList,
    MidiSetup,
    Preferences,
    Help,
    Website,
    About,
    CopyLyrics,
    SaveLyrics,
    PrintLyrics,
    BrowseSoundfont,
    RefreshPorts,
    TestNote,
    Shuffle,
    Clear,
    Reveal,
}

#[derive(Clone, Copy)]
pub struct ActionInfo {
    pub action: Action,
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
}

pub const MENU: &[ActionInfo] = &[
    ActionInfo {
        action: Action::Open,
        id: "open",
        label: "Open…",
        group: "File",
    },
    ActionInfo {
        action: Action::Add,
        id: "add",
        label: "Add songs…",
        group: "File",
    },
    ActionInfo {
        action: Action::OpenPlaylist,
        id: "open-playlist",
        label: "Open playlist…",
        group: "File",
    },
    ActionInfo {
        action: Action::SavePlaylist,
        id: "save-playlist",
        label: "Save playlist…",
        group: "File",
    },
    ActionInfo {
        action: Action::FileInfo,
        id: "file-info",
        label: "File information",
        group: "File",
    },
    ActionInfo {
        action: Action::LoadSettings,
        id: "load-settings",
        label: "Load song settings",
        group: "File",
    },
    ActionInfo {
        action: Action::SaveSettings,
        id: "save-settings",
        label: "Save song settings",
        group: "File",
    },
    ActionInfo {
        action: Action::Reveal,
        id: "reveal",
        label: "Show song in folder",
        group: "File",
    },
    ActionInfo {
        action: Action::Quit,
        id: "quit",
        label: "Quit",
        group: "File",
    },
    ActionInfo {
        action: Action::Play,
        id: "play",
        label: "Play",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Pause,
        id: "pause",
        label: "Pause / resume",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Stop,
        id: "stop",
        label: "Stop",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Previous,
        id: "previous",
        label: "Previous song",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Next,
        id: "next",
        label: "Next song",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Rewind,
        id: "rewind",
        label: "Rewind one bar",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Forward,
        id: "forward",
        label: "Forward one bar",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Jump,
        id: "jump",
        label: "Jump to bar…",
        group: "Playback",
    },
    ActionInfo {
        action: Action::Loop,
        id: "loop",
        label: "Loop…",
        group: "Playback",
    },
    ActionInfo {
        action: Action::RepeatOff,
        id: "repeat-off",
        label: "Repeat off",
        group: "Playback",
    },
    ActionInfo {
        action: Action::RepeatSong,
        id: "repeat-song",
        label: "Repeat song",
        group: "Playback",
    },
    ActionInfo {
        action: Action::RepeatList,
        id: "repeat-list",
        label: "Repeat playlist",
        group: "Playback",
    },
    ActionInfo {
        action: Action::MidiSetup,
        id: "midi-setup",
        label: "MIDI Setup…",
        group: "Settings",
    },
    ActionInfo {
        action: Action::Preferences,
        id: "preferences",
        label: "Preferences…",
        group: "Settings",
    },
    ActionInfo {
        action: Action::Help,
        id: "help",
        label: "Help",
        group: "Help",
    },
    ActionInfo {
        action: Action::Search,
        id: "search",
        label: "Search MIDI files",
        group: "Help",
    },
    ActionInfo {
        action: Action::Website,
        id: "website",
        label: "Original dmidiplayer",
        group: "Help",
    },
    ActionInfo {
        action: Action::About,
        id: "about",
        label: "About Gosh MIDI Player",
        group: "Help",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    Drums,
    SoloReduction,
    Autoplay,
    Advance,
    SongSettings,
    Reset,
    Map,
    FutureColor,
    PastColor,
    HighlightColor,
    LyricsFont,
    Alignment,
    Palette,
    SingleColor,
    Velocity,
    Names,
    Octave,
    NotesFont,
}

pub fn shortcut(
    key: &str,
    primary: bool,
    other_modifiers: bool,
    editing: bool,
    modal: bool,
) -> Option<Action> {
    if editing || modal || other_modifiers {
        return None;
    }
    if primary {
        match key.to_ascii_lowercase().as_str() {
            "o" => Some(Action::Open),
            "q" => Some(Action::Quit),
            "p" => Some(Action::Pause),
            "s" => Some(Action::Stop),
            "arrowright" => Some(Action::Next),
            "arrowleft" => Some(Action::Previous),
            "," => Some(Action::Preferences),
            _ => None,
        }
    } else {
        match key {
            " " | "Space" => Some(Action::Play),
            "F1" => Some(Action::Help),
            _ => None,
        }
    }
}

// Native accelerators are generated from the same inventory as DOM shortcuts.
pub fn accelerator(action: Action) -> Option<&'static str> {
    match action {
        Action::Open => Some("O"),
        Action::Quit => Some("Q"),
        Action::Pause => Some("P"),
        Action::Stop => Some("S"),
        Action::Next => Some("Right"),
        Action::Previous => Some("Left"),
        Action::Preferences => Some(","),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_do_not_steal_text_or_modal_input() {
        assert_eq!(shortcut("o", true, false, false, false), Some(Action::Open));
        assert_eq!(shortcut("o", true, false, true, false), None);
        assert_eq!(shortcut(" ", false, false, false, true), None);
        assert_eq!(shortcut("o", true, true, false, false), None);
        let ids: std::collections::HashSet<_> = MENU.iter().map(|entry| entry.id).collect();
        assert_eq!(ids.len(), MENU.len());
        assert!(
            MENU.iter()
                .any(|entry| entry.action == Action::SaveSettings)
        );
    }
}
