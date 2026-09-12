//! Remaining-dialog logic: Jump (D7), Loop (D8), File info (D6), Help (D5),
//! About (D4), and the manual song-settings triggers (arch §2.1, ux dialog
//! table + checklist).
//!
//! Pure state only — no views (T13b `ui/jump_loop.rs` + `ui/info_dialogs.rs`);
//! the `app.rs` arms + dialog shells landed in T13c. Every rule cites its
//! C++ twin in `src/ui/window.cpp`.

use crate::core::event::tempo_to_bpm;

// ---------------------------------------------------------------------------
// D7 Jump to bar (`window.cpp:775-829`)
// ---------------------------------------------------------------------------

/// Jump spin range: `1..=max(1, last_bar)` (`window.cpp:790`).
pub fn jump_range(last_bar: i32) -> (u32, u32) {
    (1, last_bar.max(1) as u32)
}

/// Clamp a jump target into [`jump_range`] (the C++ spin button clamps
/// inputs the same way — out-of-range values cannot occur there).
pub fn clamp_jump_bar(bar: i32, last_bar: i32) -> u32 {
    bar.clamp(1, last_bar.max(1)) as u32
}

/// Jump availability: C++ returns early with no song loaded
/// (`window.cpp:777-778`).
pub fn jump_allowed(song_present: bool) -> bool {
    song_present
}

/// Pause-jump-resume plan (`window.cpp:800-805`): pause first and resume
/// after if and only if the player is running; otherwise jump alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpPlan {
    pub pause_first: bool,
    pub bar: u32,
    pub resume_after: bool,
}

/// Build the jump plan: pause + resume ride on `playing`.
pub fn plan_jump(bar: u32, playing: bool) -> JumpPlan {
    JumpPlan {
        pause_first: playing,
        bar,
        resume_after: playing,
    }
}

// ---------------------------------------------------------------------------
// D8 Loop (`window.cpp:831-882`)
// ---------------------------------------------------------------------------

/// Dialog entry point: the header toggle (with its state) or the `app.loop`
/// menu item (`window.cpp:1393-1394,1602,1638-1645`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopEntry {
    Toggle(bool),
    Menu,
}

/// What a dialog entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopEntryAction {
    /// Open the From/To range dialog.
    OpenDialog,
    /// Disable the loop directly, no dialog (`setLoop(false)`).
    DisableLoop,
}

/// Loop entry decision.
///
/// * Toggle OFF → disable directly, no dialog (C++ `1642-1643`).
/// * Toggle ON → open the dialog. C++ no-ops with no song loaded
///   (`window.cpp:832-834`); the port opens unconditionally instead of a
///   silently dead toggle (pre-existing deviation, kept — see the
///   `LoopToggled` arm note in `app.rs`).
/// * Menu → ALWAYS open (U3 FIX): C++ hits the toggle-OFF early return
///   (`window.cpp:835-838`: `setLoop(false)` + return) and is a silent
///   no-op whenever the toggle is off — DECISIONS D11 rules this a trap and
///   the port forces the toggle ON, then opens the dialog. Silent no-op
///   banned.
pub fn loop_entry_action(entry: LoopEntry) -> LoopEntryAction {
    match entry {
        LoopEntry::Toggle(false) => LoopEntryAction::DisableLoop,
        LoopEntry::Toggle(true) | LoopEntry::Menu => LoopEntryAction::OpenDialog,
    }
}

/// Loop spin range: `1..=max(1, last_bar)` (`window.cpp:849-851`).
pub fn loop_range(last_bar: i32) -> (u32, u32) {
    (1, last_bar.max(1) as u32)
}

/// From/To seeds: the current `loopStart`/`loopEnd`, each clamped into
/// [`loop_range`] (`window.cpp:852-853` — `gtk_spin_button_set_value`
/// clamps to the adjustment range).
pub fn loop_seeds(loop_start: u32, loop_end: u32, last_bar: i32) -> (u32, u32) {
    let max = last_bar.max(1) as u32;
    (loop_start.clamp(1, max), loop_end.clamp(1, max))
}

/// Loop confirm: clamp each end into [`loop_range`]. There is deliberately
/// NO from≤to enforcement — C++ `setLoop(a, c)` assigns blindly
/// (`player.cpp:237-242`) — preserved.
pub fn loop_confirm(from: i32, to: i32, last_bar: i32) -> (u32, u32) {
    let max = last_bar.max(1) as u32;
    (
        from.clamp(1, max as i32) as u32,
        to.clamp(1, max as i32) as u32,
    )
}

// Cancel rule (`window.cpp:873-880`): on close, the toggle stays ON if and
// only if a loop is confirmed enabled — an unconfirmed toggle resets to OFF.
// T14a T13-minor resolution: the former `loop_cancel_toggle` identity helper
// is removed (it had no callers — the port's toggle derives from
// `loop_enabled`, which only `LoopSet` flips, so the rule falls out of the
// model). The rule stays pinned at the composition arm instead:
// `CloseDialog` (`app.rs`, "D8 cancel rule falls out of the model") +
// `loop_dialog_cancel_resets_unconfirmed_toggle` + the `dlg-loop-x`
// `Harness` flow in `tests/dialogs_flows.rs`.

// ---------------------------------------------------------------------------
// D6 File info (`window.cpp:749-773`)
// ---------------------------------------------------------------------------

/// File-info field labels in C++ body order (`window.cpp:756-763`), before
/// the metadata block and the copyright lines.
pub const FILE_INFO_FIELDS: [&str; 6] = [
    "File",
    "Format",
    "Duration",
    "Initial tempo",
    "Tracks",
    "Division",
];

/// Assembled file-info body: every C++ field plus the metadata pairs (in
/// map order) and one line per copyright record (in event order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfoData {
    pub title: String,
    pub format: String,
    /// `HH:MM:SS.mmm` (see [`format_file_duration_ms`]).
    pub duration: String,
    /// `"%.1f bpm"` (see [`format_initial_tempo_us`]).
    pub initial_tempo: String,
    pub num_tracks: i32,
    pub division: i32,
    /// `(key, value)` metadata pairs, in map order
    /// (`Sequence::metadataInfo`, `sequence.cpp:340-350`).
    pub metadata: Vec<(String, String)>,
    /// Decoded copyright records, in event order (`getText(Copyright)`).
    pub copyrights: Vec<String>,
}

/// File-info dialog content: empty state or the assembled fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileInfo {
    /// No file loaded (`window.cpp:753-754`).
    Empty,
    Loaded(FileInfoData),
}

/// Empty-state content (`"<b>No file loaded</b>"`, `window.cpp:754`).
pub fn file_info_empty() -> FileInfo {
    FileInfo::Empty
}

/// Duration label from whole milliseconds: `HH:MM:SS.mmm`
/// (`Sequence::durationString`, `sequence.cpp:326-338`).
///
/// NOTE: C++ rounds from microseconds while the port's engine snapshot
/// carries truncated whole milliseconds — sub-millisecond rendering delta,
/// documented, untestable at this layer (inputs are already ms).
pub fn format_file_duration_ms(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms / 60_000) % 60;
    let s = (ms / 1_000) % 60;
    let millis = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Initial-tempo label (`window.cpp:760-761`): `tempoToBpm` of the raw first
/// tempo when positive, else 120 — then `"%.1f bpm"`. (The guard duplicates
/// `tempo_to_bpm`'s own ≤0→120 fallback; both are mirrored exactly.)
pub fn format_initial_tempo_us(tempo_us: f64) -> String {
    let bpm = if tempo_us > 0.0 {
        tempo_to_bpm(tempo_us)
    } else {
        120.0
    };
    format!("{bpm:.1} bpm")
}

/// Assemble the loaded-song body from engine-snapshot parts. Composition
/// feeds `title`/`format`/`duration_ms`/`initial_tempo_us`/`num_tracks`/
/// `division` from the existing `LoadedSong` fields; `metadata` and
/// `copyrights` need new snapshot fields (engine fills them from
/// `info_map` + `get_text(Copyright)` — proposed in the T13a report, since
/// `message.rs` is composition-owned).
#[allow(clippy::too_many_arguments)]
pub fn assemble_file_info(
    title: &str,
    format: &str,
    duration_ms: u64,
    initial_tempo_us: f64,
    num_tracks: i32,
    division: i32,
    metadata: Vec<(String, String)>,
    copyrights: Vec<String>,
) -> FileInfo {
    FileInfo::Loaded(FileInfoData {
        title: title.to_string(),
        format: format.to_string(),
        duration: format_file_duration_ms(duration_ms),
        initial_tempo: format_initial_tempo_us(initial_tempo_us),
        num_tracks,
        division,
        metadata,
        copyrights,
    })
}

// ---------------------------------------------------------------------------
// D5 Help (`window.cpp:23-37,922-927`)
// ---------------------------------------------------------------------------

/// Help dialog title (`window.cpp:924`).
pub const HELP_TITLE: &str = "Help";

/// Exact `kHelpText` copy (`window.cpp:23-37`), paragraph breaks included.
pub const HELP_TEXT: &str = "Gosh MIDI Player is a MIDI file player with lyrics, piano, channels, and playlists.\n\
    It is based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.\n\n\
    Supported files: .mid .midi .kar .rmi .wrk\n\n\
    Playback: Play, Pause, Stop, previous/next playlist item, jump to bar, loop between bars.\n\
    Transpose with Pitch (-12..+12, percussion excluded). Tempo 50–200%. Volume 0–200% (CC7).\n\n\
    Playlists are .lst text files with one path per line. Drag files onto the window to make a \
    temporary playlist. Repeat can be off, current song, or whole playlist.\n\n\
    MIDI output: ALSA sequencer ports (Munt/MT-32, Nuked SC-55, USB MIDI) or \
    FluidSynth with a SoundFont you choose. Configure in MIDI Setup.\n\n\
    Start Munt (mt32emu-qt) or Nuked-SC55 first so they appear as ALSA ports. \
    Then MIDI Setup → ALSA → pick MT-32:Standard or Virtual SC55. Companion .syx \
    files next to a MIDI file are sent automatically before playback.\n\n\
    Song settings can be stored in ~/.dmidiplayer/<song>.cfg (encoding, pitch, tempo, volume, \
    per-channel mute/solo/lock/patch/level).\n";

// ---------------------------------------------------------------------------
// D4 About (`window.cpp:884-920`)
// ---------------------------------------------------------------------------

/// About dialog content: every string, the credit section, and the 3 links
/// (`window.cpp:884-920`). The version rides `VERSION` (1.7.5,
/// `CMakeLists.txt:9`) — composition reads `env!("CARGO_PKG_VERSION")`;
/// the license is GPL-3.0 (`GTK_LICENSE_GPL_3_0`, `window.cpp:893`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutInfo {
    pub app_name: &'static str,
    pub app_icon: &'static str,
    pub developer: &'static str,
    pub copyright: &'static str,
    pub website: &'static str,
    pub support_url: &'static str,
    pub issue_url: &'static str,
    pub comments: &'static str,
    pub developers: [&'static str; 2],
    pub credit_section: &'static str,
    pub credit_entries: [&'static str; 1],
    pub links: [(&'static str, &'static str); 3],
}

/// The About content, verbatim.
pub const ABOUT: AboutInfo = AboutInfo {
    app_name: "Gosh MIDI Player",
    app_icon: "com.goshapps.GoshMIDI",
    developer: "Pedro López-Cabanillas",
    copyright: "Copyright © 2006–2026 Pedro López-Cabanillas and contributors",
    website: "https://github.com/goshitsarch-eng/goshmidi",
    support_url: "https://dmidiplayer.sourceforge.io/",
    issue_url: "https://github.com/goshitsarch-eng/goshmidi/issues",
    comments: "GTK4/libadwaita MIDI player based on dmidiplayer (Drumstick MIDI File Player) \
        by Pedro López-Cabanillas. The original application remains available from SourceForge \
        and GitHub. The command name and settings paths are still dmidiplayer.",
    developers: [
        "Pedro López-Cabanillas (original dmidiplayer)",
        "Gosh MIDI Player contributors",
    ],
    credit_section: "Original application",
    credit_entries: ["dmidiplayer — Drumstick MIDI File Player"],
    links: [
        (
            "Original dmidiplayer (SourceForge)",
            "https://sourceforge.net/p/dmidiplayer/",
        ),
        (
            "Original dmidiplayer (GitHub)",
            "https://github.com/pedrolcl/dmidiplayer",
        ),
        (
            "Original documentation",
            "https://dmidiplayer.sourceforge.io/",
        ),
    ],
};

// ---------------------------------------------------------------------------
// Song-settings manual actions (`window.cpp:1405-1408`)
// ---------------------------------------------------------------------------

/// Manual save/load precondition: a song is loaded (non-empty title). C++
/// guards both `saveSongSettings` and `loadSongSettings` on an empty song
/// name (`window.cpp:633-635,663-665`); the menu actions call them directly
/// (`window.cpp:1405-1408`) with NO `autoSongSettings` gate — that gate
/// applies only to the auto path (finish/load/next-song call sites). Missing
/// or corrupt load files are a silent no-op inside the load itself
/// (`window.cpp:667-670`).
pub fn manual_song_action_allowed(song_title: Option<&str>) -> bool {
    song_title.is_some_and(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_range_clamps_to_min_1() {
        assert_eq!(jump_range(32), (1, 32));
        assert_eq!(jump_range(1), (1, 1));
        assert_eq!(jump_range(0), (1, 1));
        assert_eq!(jump_range(-4), (1, 1));
    }

    #[test]
    fn jump_clamp_and_availability() {
        assert_eq!(clamp_jump_bar(4, 32), 4);
        assert_eq!(clamp_jump_bar(1, 32), 1);
        assert_eq!(clamp_jump_bar(32, 32), 32);
        assert_eq!(clamp_jump_bar(0, 32), 1);
        assert_eq!(clamp_jump_bar(-9, 32), 1);
        assert_eq!(clamp_jump_bar(99, 32), 32);
        // Degenerate last bar still yields bar 1.
        assert_eq!(clamp_jump_bar(99, 0), 1);
        assert_eq!(clamp_jump_bar(-99, -5), 1);
        assert!(jump_allowed(true));
        assert!(!jump_allowed(false));
    }

    #[test]
    fn jump_plan_pauses_only_when_playing() {
        assert_eq!(
            plan_jump(4, true),
            JumpPlan {
                pause_first: true,
                bar: 4,
                resume_after: true
            }
        );
        assert_eq!(
            plan_jump(4, false),
            JumpPlan {
                pause_first: false,
                bar: 4,
                resume_after: false
            }
        );
    }

    #[test]
    fn loop_entries_route_toggle_and_menu() {
        // Toggle OFF disables directly, no dialog.
        assert_eq!(
            loop_entry_action(LoopEntry::Toggle(false)),
            LoopEntryAction::DisableLoop
        );
        // Toggle ON opens the dialog.
        assert_eq!(
            loop_entry_action(LoopEntry::Toggle(true)),
            LoopEntryAction::OpenDialog
        );
        // U3 FIX: menu ALWAYS opens (C++ silent-no-ops with toggle OFF).
        assert_eq!(
            loop_entry_action(LoopEntry::Menu),
            LoopEntryAction::OpenDialog
        );
    }

    #[test]
    fn loop_range_seeds_and_confirm_clamp() {
        assert_eq!(loop_range(16), (1, 16));
        assert_eq!(loop_range(0), (1, 1));
        assert_eq!(loop_range(-2), (1, 1));
        // Seeds pass through in range, clamp out of range.
        assert_eq!(loop_seeds(3, 8, 16), (3, 8));
        assert_eq!(loop_seeds(0, 99, 16), (1, 16));
        assert_eq!(loop_seeds(5, 5, 1), (1, 1));
        // Confirm clamps each end; from > to passes through (C++ assigns
        // blindly — preserved, no ordering enforcement).
        assert_eq!(loop_confirm(3, 8, 16), (3, 8));
        assert_eq!(loop_confirm(8, 3, 16), (8, 3));
        assert_eq!(loop_confirm(-4, 999, 16), (1, 16));
        assert_eq!(loop_confirm(4, 4, 0), (1, 1));
    }

    // NOTE: no unit test for the D8 cancel rule at this layer anymore
    // (T14a removed the `loop_cancel_toggle` identity helper — see above).
    // The rule is pinned at the composition arm instead
    // (`loop_dialog_cancel_resets_unconfirmed_toggle` in `app.rs`).

    #[test]
    fn file_info_field_order_matches_cpp() {
        assert_eq!(
            FILE_INFO_FIELDS,
            [
                "File",
                "Format",
                "Duration",
                "Initial tempo",
                "Tracks",
                "Division",
            ]
        );
        assert!(matches!(file_info_empty(), FileInfo::Empty));
    }

    #[test]
    fn file_duration_formats_hh_mm_ss_mmm() {
        assert_eq!(format_file_duration_ms(0), "00:00:00.000");
        assert_eq!(format_file_duration_ms(1), "00:00:00.001");
        assert_eq!(format_file_duration_ms(999), "00:00:00.999");
        assert_eq!(format_file_duration_ms(1_000), "00:00:01.000");
        assert_eq!(format_file_duration_ms(61_000), "00:01:01.000");
        assert_eq!(format_file_duration_ms(3_661_001), "01:01:01.001");
        assert_eq!(format_file_duration_ms(36_000_000), "10:00:00.000");
        // ms roll over into seconds/minutes/hours correctly.
        assert_eq!(format_file_duration_ms(3_599_999), "00:59:59.999");
    }

    #[test]
    fn initial_tempo_label_with_nonpositive_fallback() {
        assert_eq!(format_initial_tempo_us(500_000.0), "120.0 bpm");
        assert_eq!(format_initial_tempo_us(250_000.0), "240.0 bpm");
        assert_eq!(format_initial_tempo_us(1_000_000.0), "60.0 bpm");
        // Non-positive (incl. NaN-safe via the > 0.0 guard) → 120.
        assert_eq!(format_initial_tempo_us(0.0), "120.0 bpm");
        assert_eq!(format_initial_tempo_us(-5.0), "120.0 bpm");
        assert_eq!(format_initial_tempo_us(f64::NAN), "120.0 bpm");
    }

    #[test]
    fn file_info_assembly_keeps_order_and_all_fields() {
        let info = assemble_file_info(
            "song.mid",
            "SMF type 1",
            61_001,
            500_000.0,
            4,
            96,
            vec![
                ("Title".to_string(), "Twinkle".to_string()),
                ("Artist".to_string(), "Anon".to_string()),
            ],
            vec!["© 1999 Anon".to_string(), "© 2000 Anon".to_string()],
        );
        let FileInfo::Loaded(data) = info else {
            panic!("expected Loaded");
        };
        assert_eq!(data.title, "song.mid");
        assert_eq!(data.format, "SMF type 1");
        assert_eq!(data.duration, "00:01:01.001");
        assert_eq!(data.initial_tempo, "120.0 bpm");
        assert_eq!(data.num_tracks, 4);
        assert_eq!(data.division, 96);
        // Metadata order preserved (engine BTreeMap iterates sorted, like
        // C++ std::map); copyrights in event order.
        assert_eq!(
            data.metadata,
            vec![
                ("Title".to_string(), "Twinkle".to_string()),
                ("Artist".to_string(), "Anon".to_string()),
            ]
        );
        assert_eq!(data.copyrights, vec!["© 1999 Anon", "© 2000 Anon"]);
    }

    #[test]
    fn file_info_empty_metadata_and_copyrights() {
        let FileInfo::Loaded(data) = assemble_file_info(
            "s.kar",
            "SMF type 1",
            0,
            0.0,
            1,
            120,
            Vec::new(),
            Vec::new(),
        ) else {
            panic!("expected Loaded");
        };
        assert_eq!(data.duration, "00:00:00.000");
        assert_eq!(data.initial_tempo, "120.0 bpm");
        assert!(data.metadata.is_empty());
        assert!(data.copyrights.is_empty());
    }

    #[test]
    fn help_copy_is_exact() {
        assert_eq!(HELP_TITLE, "Help");
        // Structural pins: 7 paragraphs, exact head/tail, no stray spaces at
        // continuation joins (a join glitch would show as a double space or
        // a glued word).
        let paras: Vec<&str> = HELP_TEXT.split("\n\n").collect();
        assert_eq!(paras.len(), 7);
        assert!(HELP_TEXT.starts_with("Gosh MIDI Player is a MIDI file player"));
        assert!(HELP_TEXT.ends_with("per-channel mute/solo/lock/patch/level).\n"));
        assert!(!HELP_TEXT.contains("  "));
        for phrase in [
            "Pedro López-Cabanillas",
            "Supported files: .mid .midi .kar .rmi .wrk",
            "Tempo 50–200%. Volume 0–200% (CC7).",
            "Drag files onto the window to make a temporary playlist.",
            "MIDI output: ALSA sequencer ports (Munt/MT-32, Nuked SC-55, USB MIDI) or FluidSynth",
            "Then MIDI Setup → ALSA → pick MT-32:Standard or Virtual SC55.",
            "Companion .syx files next to a MIDI file are sent automatically before playback.",
            "Song settings can be stored in ~/.dmidiplayer/<song>.cfg",
        ] {
            assert!(HELP_TEXT.contains(phrase), "missing {phrase:?}");
        }
        // Full-paragraph equality on the two wrapped paragraphs (the
        // continuation-sensitive ones).
        assert_eq!(
            paras[3],
            "Playlists are .lst text files with one path per line. Drag files onto the window to make a temporary playlist. Repeat can be off, current song, or whole playlist."
        );
        assert_eq!(
            paras[6],
            "Song settings can be stored in ~/.dmidiplayer/<song>.cfg (encoding, pitch, tempo, volume, per-channel mute/solo/lock/patch/level).\n"
        );
    }

    #[test]
    fn about_strings_credit_and_links_are_verbatim() {
        assert_eq!(ABOUT.app_name, "Gosh MIDI Player");
        assert_eq!(ABOUT.app_icon, "com.goshapps.GoshMIDI");
        assert_eq!(ABOUT.developer, "Pedro López-Cabanillas");
        assert_eq!(
            ABOUT.copyright,
            "Copyright © 2006–2026 Pedro López-Cabanillas and contributors"
        );
        assert_eq!(ABOUT.website, "https://github.com/goshitsarch-eng/goshmidi");
        assert_eq!(ABOUT.support_url, "https://dmidiplayer.sourceforge.io/");
        assert_eq!(
            ABOUT.issue_url,
            "https://github.com/goshitsarch-eng/goshmidi/issues"
        );
        assert_eq!(
            ABOUT.comments,
            "GTK4/libadwaita MIDI player based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas. The original application remains available from SourceForge and GitHub. The command name and settings paths are still dmidiplayer."
        );
        assert_eq!(
            ABOUT.developers,
            [
                "Pedro López-Cabanillas (original dmidiplayer)",
                "Gosh MIDI Player contributors",
            ]
        );
        assert_eq!(ABOUT.credit_section, "Original application");
        assert_eq!(
            ABOUT.credit_entries,
            ["dmidiplayer — Drumstick MIDI File Player"]
        );
        assert_eq!(
            ABOUT.links,
            [
                (
                    "Original dmidiplayer (SourceForge)",
                    "https://sourceforge.net/p/dmidiplayer/",
                ),
                (
                    "Original dmidiplayer (GitHub)",
                    "https://github.com/pedrolcl/dmidiplayer",
                ),
                (
                    "Original documentation",
                    "https://dmidiplayer.sourceforge.io/",
                ),
            ]
        );
    }

    #[test]
    fn manual_song_actions_need_a_loaded_song() {
        assert!(manual_song_action_allowed(Some("song.mid")));
        assert!(!manual_song_action_allowed(None));
        assert!(!manual_song_action_allowed(Some("")));
    }
}
