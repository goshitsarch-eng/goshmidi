//! D6 file info + D5 help + D4 about dialog contents (ux D6/D5/D4).
//!
//! Real T13b implementation of the frozen [`FileInfoViewModel`] contract
//! (D6) plus the static D5/D4 contents, consumed as-is: owned `vm`/nothing
//! in, one [`cosmic::Element`] out. (Arch composes these contents into the
//! [`chrome`][super::chrome] wrap next.)
//!
//! Widget → `Message` map:
//!
//! | widget | message |
//! |---|
//! | D6 `OK` | `CloseDialog` |
//! | D5 `OK` | `CloseDialog` |
//! | D4 `Close` | `CloseDialog` |
//! | D4 link buttons (6) | `on_url(url)` (caller-supplied, see below) |
//!
//! Strings + formatters are SINGLE-SOURCED in
//! [`state::dialogs`][crate::state::dialogs] (T13a):
//! [`FILE_INFO_FIELDS`][crate::state::dialogs::FILE_INFO_FIELDS],
//! [`format_file_duration_ms`][crate::state::dialogs::format_file_duration_ms],
//! [`format_initial_tempo_us`][crate::state::dialogs::format_initial_tempo_us],
//! [`HELP_TEXT`][crate::state::dialogs::HELP_TEXT], and
//! [`ABOUT`][crate::state::dialogs::ABOUT]. This module owns only the VM,
//! the builder, and the views.
//!
//! D6 contract notes (implemented exactly):
//!
//! * Body rows are verbatim C++ in order (`window.cpp:749-773`):
//!   File/Format/Duration/Initial tempo/Tracks/Division, then the RIFF
//!   INFO metadata lines, then one line per copyright record.
//! * Empty state: `No file loaded` (`window.cpp:753-754`).
//! * The builder decodes the copyright records with the song charset
//!   (`Sequence::decode_with_charset` — the same decode the lyrics path
//!   uses); metadata arrives pre-decoded (composition input — `SongSnapshot`
//!   carries no info map today, so arch extends it or passes `&[]`; the VM
//!   renders whatever it is given).
//!
//! D5 contract notes (implemented exactly):
//!
//! * Body is the full shared [`HELP_TEXT`] (`kHelpText`, `window.cpp:23-37`)
//!   inside a `scrollable`; `OK` closes (`window.cpp:922-927`).
//!
//! D4 contract notes (implemented exactly):
//!
//! * All strings come from the shared [`ABOUT`] table (verbatim C++,
//!   `window.cpp:884-920`): app name, icon, developer, copyright, comments,
//!   the 2 developers, the `Original application` credit section, and all 6
//!   URLs (website + support + issues + the 3 links). The license display
//!   name + the close label live here (the table carries neither).
//! * Version is `env!("CARGO_PKG_VERSION")` (1.8.0 — the crate version,
//!   successor of the C++ `PROJECT_VERSION` the `VERSION` macro expanded).
//! * Hand-built, not `widget::about`: the stock widget has no
//!   website/support/issues fields and would drop them. Link buttons use
//!   the `link` style with names + descriptions.
//! * Links ride the caller-supplied `on_url` constructor (prefs
//!   `on_scratch` precedent): arch adds an open-URI message + `xdg-open` /
//!   portal task and supplies it at composition. Tests pass a `Toast`
//!   stand-in (shape-compatible only — production never toasts a URL).
//!
//! [`HELP_TEXT`]: crate::state::dialogs::HELP_TEXT
//! [`ABOUT`]: crate::state::dialogs::ABOUT
//!
//! A11y: `OK`/`Close` + every link button carry names + descriptions; the
//! bodies are plain text (no interaction to name).
//!
//! [`FileInfoViewModel`]: super::FileInfoViewModel

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{button, column, container, icon, row, scrollable, text};

use super::{FileInfoBody, FileInfoViewModel};
use crate::core::event::TextType;
use crate::core::sequence::Sequence;
use crate::message::{LoadedSong, Message};
use crate::state::dialogs::{
    ABOUT, FILE_INFO_FIELDS, HELP_TEXT, format_file_duration_ms, format_initial_tempo_us,
};

/// D6 dialog title (`window.cpp:769`, verbatim — composition sets it).
pub const FILE_INFO_TITLE: &str = "File information";
/// D6 empty state (`window.cpp:754`, verbatim).
pub const FILE_INFO_EMPTY: &str = "No file loaded";
/// D6 copyright-row label (`window.cpp:767`, verbatim — the other six labels
/// come from the shared [`FILE_INFO_FIELDS`]).
pub const FILE_INFO_COPYRIGHT_LABEL: &str = "Copyright";
/// D6/D5 confirm label (`window.cpp:771,925`, verbatim).
pub const LABEL_OK: &str = "OK";
/// Focus-on-open target (T14d G4 partial): composition focuses the D6/D5
/// `OK` button on open (the `focus(id)` op unfocuses everything else —
/// pinned `iced/core/src/widget/operation/focusable.rs:30-57` — so base
/// Enter cannot fire silently under the dialog). Shared by D6 + D5: the
/// single-dialog machine never shows both at once, so the id is unique
/// in every live tree.
pub const OK_ID: &str = "dialog-ok";

/// D4 license (`GTK_LICENSE_GPL_3_0`, `window.cpp:893`, display name — the
/// shared [`ABOUT`] table carries no license field).
pub const ABOUT_LICENSE: &str = "GNU General Public License v3.0";
/// D4 close label (C++ `AdwAboutDialog` has a stock close; the port names it
/// `Close` like the T12 prefs sheet).
pub const ABOUT_CLOSE: &str = "Close";
/// Focus-on-open target (T14d G4 partial): composition focuses the D4
/// `Close` button on open (same mechanism as [`OK_ID`]).
pub const ABOUT_CLOSE_ID: &str = "about-close";

// ---------------------------------------------------------------------------
// D6 builder (pure over state)
// ---------------------------------------------------------------------------

/// Build the D6 view model: every C++ body row in order, or `None` with no
/// song. `metadata` is the pre-decoded RIFF INFO map in order (composition
/// input — see module docs); copyrights decode from the song texts with the
/// song charset. Pure — no `App` access; composition calls it fresh per frame.
pub fn file_info_view_model(
    song: Option<&LoadedSong>,
    metadata: &[(String, String)],
) -> FileInfoViewModel {
    let Some(song) = song else {
        return FileInfoViewModel { body: None };
    };
    let copyrights: Vec<String> = song
        .texts
        .iter()
        .filter(|rec| rec.text_type == TextType::Copyright)
        .map(|rec| Sequence::decode_with_charset(&rec.text, &song.charset))
        .collect();
    FileInfoViewModel {
        body: Some(FileInfoBody {
            file: song.title.clone(),
            format: song.format.clone(),
            duration: format_file_duration_ms(song.duration_ms),
            tempo: format_initial_tempo_us(song.initial_tempo_us),
            tracks: song.num_tracks,
            division: song.division,
            metadata: metadata.to_vec(),
            copyrights,
        }),
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// One D6 row: `Label: <b>value</b>` in C++ order (the value bolds like the
/// Pango markup, `window.cpp:756-767`).
fn file_info_row(label: &'static str, value: String) -> Element<'static, Message> {
    row::with_children(vec![
        text::body(label).into(),
        text::body(value).font(cosmic::font::bold()).into(),
    ])
    .spacing(6)
    .into()
}

/// D6 file-information dialog content: body rows or the `No file loaded`
/// empty state, then `OK`.
pub fn file_info_view<'a>(vm: FileInfoViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let FileInfoViewModel { body } = vm;

    let mut children: Vec<Element<'_, Message>> = Vec::new();
    match body {
        None => children.push(
            container(text::body(FILE_INFO_EMPTY).font(cosmic::font::bold()))
                .width(Length::Fill)
                .align_x(cosmic::iced::Alignment::Center)
                .into(),
        ),
        Some(FileInfoBody {
            file,
            format,
            duration,
            tempo,
            tracks,
            division,
            metadata,
            copyrights,
        }) => {
            children.push(file_info_row(FILE_INFO_FIELDS[0], file));
            children.push(file_info_row(FILE_INFO_FIELDS[1], format));
            children.push(file_info_row(FILE_INFO_FIELDS[2], duration));
            children.push(file_info_row(FILE_INFO_FIELDS[3], tempo));
            children.push(file_info_row(FILE_INFO_FIELDS[4], tracks.to_string()));
            children.push(file_info_row(FILE_INFO_FIELDS[5], division.to_string()));
            for (key, value) in metadata {
                children.push(file_info_row_owned(key, value));
            }
            for copyright in copyrights {
                children.push(file_info_row(FILE_INFO_COPYRIGHT_LABEL, copyright));
            }
        }
    }
    children.push(
        container(
            button::standard(LABEL_OK)
                .on_press(Message::CloseDialog)
                .id(cosmic::iced::core::widget::Id::new(OK_ID))
                .name(LABEL_OK)
                .description("Close the file information dialog"),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced::Alignment::End)
        .into(),
    );

    column::with_children(children)
        .spacing(6)
        .padding(18)
        .width(Length::Fill)
        .into()
}

/// D6 metadata row: like [`file_info_row`] but with a runtime key (RIFF
/// INFO keys are data, not labels).
fn file_info_row_owned(key: String, value: String) -> Element<'static, Message> {
    row::with_children(vec![
        text::body(key).into(),
        text::body(value).font(cosmic::font::bold()).into(),
    ])
    .spacing(6)
    .into()
}

/// D5 help dialog content: the full [`HELP_TEXT`] in a `scrollable`, then
/// `OK` (`window.cpp:922-927`).
pub fn help_view() -> Element<'static, Message> {
    column::with_children(vec![
        scrollable(text::body(HELP_TEXT))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        container(
            button::standard(LABEL_OK)
                .on_press(Message::CloseDialog)
                .id(cosmic::iced::core::widget::Id::new(OK_ID))
                .name(LABEL_OK)
                .description("Close the help dialog"),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced::Alignment::End)
        .into(),
    ])
    .spacing(12)
    .padding(18)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One D4 link button: `link` style + a11y name/description; the URL rides
/// the caller-supplied `on_url` constructor.
fn about_link<'a>(
    label: &'static str,
    url: &'static str,
    on_url: &impl Fn(&'a str) -> Message,
) -> Element<'a, Message> {
    button::link(label)
        .on_press(on_url(url))
        .name(label)
        .description(format!("Open {url}"))
        .into()
}

/// App-icon edge in the D4 header (the C++ `AdwAboutDialog` shows the app
/// icon prominently; 64px matches the installed hicolor sizes).
pub const ABOUT_ICON_SIZE: u16 = 64;

/// D4 about dialog content: icon + name + version + developer + copyright +
/// license + comments + website/support/issues links + developers +
/// `Original application` credit + the 3 links, then `Close`
/// (`window.cpp:884-920`).
pub fn about_view<'a>(on_url: impl Fn(&'a str) -> Message + 'a) -> Element<'a, Message> {
    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    // T14b (T13 minor): the shared `ABOUT.app_icon` (`com.goshapps.GoshMIDI`,
    // installed to hicolor per `icons/CMakeLists.txt`) renders here — the
    // T13 view carried the field but never showed it.
    let header = row::with_children(vec![
        icon::from_name(ABOUT.app_icon)
            .size(ABOUT_ICON_SIZE)
            .icon()
            .into(),
        column::with_children(vec![
            text::title3(ABOUT.app_name).into(),
            text::body(version).into(),
        ])
        .spacing(2)
        .into(),
    ])
    .spacing(12)
    .align_y(Alignment::Center);
    let mut children: Vec<Element<'a, Message>> = vec![
        header.into(),
        text::body(ABOUT.developer).into(),
        text::body(ABOUT.copyright).into(),
        text::body(ABOUT_LICENSE).into(),
        text::body(ABOUT.comments).into(),
        about_link("Website", ABOUT.website, &on_url),
        about_link("Support", ABOUT.support_url, &on_url),
        about_link("Report an issue", ABOUT.issue_url, &on_url),
        text::heading("Developers").into(),
    ];
    for developer in ABOUT.developers {
        children.push(text::body(developer).into());
    }
    children.push(text::heading(ABOUT.credit_section).into());
    for entry in ABOUT.credit_entries {
        children.push(text::body(entry).into());
    }
    children.push(text::heading("Links").into());
    for (label, url) in ABOUT.links {
        children.push(about_link(label, url, &on_url));
    }
    children.push(
        container(
            button::standard(ABOUT_CLOSE)
                .on_press(Message::CloseDialog)
                .id(cosmic::iced::core::widget::Id::new(ABOUT_CLOSE_ID))
                .name(ABOUT_CLOSE)
                .description("Close the about dialog"),
        )
        .width(Length::Fill)
        .align_x(cosmic::iced::Alignment::End)
        .into(),
    );

    scrollable(
        column::with_children(children)
            .spacing(6)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::player::SongSnapshot;
    use crate::core::sequence::TextRec;

    fn test_song() -> LoadedSong {
        LoadedSong::from(SongSnapshot {
            title: "song.mid".to_string(),
            format: "Format 1".to_string(),
            bars: 32,
            len_ticks: 9600,
            duration_ms: 3_723_005,
            division: 480,
            num_tracks: 9,
            initial_tempo_us: 500_000.0,
            channel_used: std::array::from_fn(|ch| ch < 3),
            channel_labels: std::array::from_fn(|ch| format!("Ch{ch}")),
            lowest_note: 36,
            highest_note: 96,
            charset: "UTF-8".to_string(),
            texts: vec![
                TextRec {
                    tick: 0,
                    track: 0,
                    text_type: TextType::Copyright,
                    text: b"(c) Example".to_vec(),
                },
                TextRec {
                    tick: 0,
                    track: 0,
                    text_type: TextType::Lyric,
                    text: b"la".to_vec(),
                },
                TextRec {
                    tick: 0,
                    track: 1,
                    text_type: TextType::Copyright,
                    text: b"(c) Second".to_vec(),
                },
            ],
            metadata: Vec::new(),
            copyrights: Vec::new(),
        })
    }

    #[test]
    fn d6_strings_are_verbatim_cpp() {
        assert_eq!(FILE_INFO_TITLE, "File information");
        assert_eq!(FILE_INFO_EMPTY, "No file loaded");
        // Six labels single-sourced (cross-pinned here), Copyright local.
        assert_eq!(
            FILE_INFO_FIELDS,
            [
                "File",
                "Format",
                "Duration",
                "Initial tempo",
                "Tracks",
                "Division"
            ]
        );
        assert_eq!(FILE_INFO_COPYRIGHT_LABEL, "Copyright");
        assert_eq!(LABEL_OK, "OK");
    }

    #[test]
    fn help_text_is_verbatim_khelptext() {
        // The expected value mirrors the C++ literal line-for-line
        // (`window.cpp:23-37`, 15 pieces joined) — any drift fails here.
        let expected = concat!(
            "Gosh MIDI Player is a MIDI file player with lyrics, piano, channels, and playlists.\n",
            "It is based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.\n\n",
            "Supported files: .mid .midi .kar .rmi .wrk\n\n",
            "Playback: Play, Pause, Stop, previous/next playlist item, jump to bar, loop between bars.\n",
            "Transpose with Pitch (-12..+12, percussion excluded). Tempo 50–200%. Volume 0–200% (CC7).\n\n",
            "Playlists are .lst text files with one path per line. Drag files onto the window to make a ",
            "temporary playlist. Repeat can be off, current song, or whole playlist.\n\n",
            "MIDI output: ALSA sequencer ports (Munt/MT-32, Nuked SC-55, USB MIDI) or ",
            "FluidSynth with a SoundFont you choose. Configure in MIDI Setup.\n\n",
            "Start Munt (mt32emu-qt) or Nuked-SC55 first so they appear as ALSA ports. ",
            "Then MIDI Setup → ALSA → pick MT-32:Standard or Virtual SC55. Companion .syx ",
            "files next to a MIDI file are sent automatically before playback.\n\n",
            "Song settings can be stored in ~/.dmidiplayer/<song>.cfg (encoding, pitch, tempo, volume, ",
            "per-channel mute/solo/lock/patch/level).\n",
        );
        assert_eq!(HELP_TEXT, expected);
    }

    #[test]
    fn about_strings_are_verbatim_cpp() {
        // The table is single-sourced (cross-pinned here); license + close
        // are view-local.
        assert_eq!(ABOUT.app_name, "Gosh MIDI Player");
        assert_eq!(ABOUT.app_icon, "com.goshapps.GoshMIDI");
        assert_eq!(ABOUT.developer, "Pedro López-Cabanillas");
        assert_eq!(
            ABOUT.copyright,
            "Copyright © 2006–2026 Pedro López-Cabanillas and contributors"
        );
        assert_eq!(ABOUT_LICENSE, "GNU General Public License v3.0");
        assert_eq!(ABOUT.website, "https://github.com/goshitsarch-eng/goshmidi");
        assert_eq!(ABOUT.support_url, "https://dmidiplayer.sourceforge.io/");
        assert_eq!(
            ABOUT.issue_url,
            "https://github.com/goshitsarch-eng/goshmidi/issues"
        );
        assert_eq!(
            ABOUT.comments,
            "GTK4/libadwaita MIDI player based on dmidiplayer (Drumstick MIDI File Player) \
            by Pedro López-Cabanillas. The original application remains available from SourceForge \
            and GitHub. The command name and settings paths are still dmidiplayer."
        );
        assert_eq!(
            ABOUT.developers,
            [
                "Pedro López-Cabanillas (original dmidiplayer)",
                "Gosh MIDI Player contributors"
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
                    "https://sourceforge.net/p/dmidiplayer/"
                ),
                (
                    "Original dmidiplayer (GitHub)",
                    "https://github.com/pedrolcl/dmidiplayer"
                ),
                (
                    "Original documentation",
                    "https://dmidiplayer.sourceforge.io/"
                ),
            ]
        );
        assert_eq!(ABOUT_CLOSE, "Close");
        // Crate version == shipped release (T15: 1.8.0, first libcosmic).
        assert_eq!(env!("CARGO_PKG_VERSION"), "1.8.0");
    }

    #[test]
    fn builder_rows_copyrights_and_empty_state() {
        let song = test_song();
        let metadata = vec![("INAM".to_string(), "Name".to_string())];
        let vm = file_info_view_model(Some(&song), &metadata);
        let body = vm.body.expect("song loaded");
        assert_eq!(body.file, "song.mid");
        assert_eq!(body.format, "Format 1");
        assert_eq!(body.duration, "01:02:03.005");
        assert_eq!(body.tempo, "120.0 bpm");
        assert_eq!(body.tracks, 9);
        assert_eq!(body.division, 480);
        assert_eq!(body.metadata, metadata);
        // Copyright records only, decoded, in record order.
        assert_eq!(body.copyrights, vec!["(c) Example", "(c) Second"]);
        // No song → the empty state.
        let vm = file_info_view_model(None, &metadata);
        assert!(vm.body.is_none());
        // Song without texts/metadata → empty lists, never missing rows.
        let mut bare = test_song();
        bare.texts.clear();
        let vm = file_info_view_model(Some(&bare), &[]);
        let body = vm.body.expect("song loaded");
        assert!(body.copyrights.is_empty());
        assert!(body.metadata.is_empty());
    }

    /// Tests pass a `Toast` stand-in for the link constructor
    /// (shape-compatible only — production opens the URL, never toasts it).
    fn test_on_url(url: &str) -> Message {
        Message::Toast(url.to_string())
    }

    #[test]
    fn views_build_across_contract_shapes() {
        // The views are not introspectable, but they must build without
        // panic across the contracts' shapes.
        let song = test_song();
        let metadata = vec![("INAM".to_string(), "Name".to_string())];
        let _ = file_info_view(file_info_view_model(Some(&song), &metadata));
        let _ = file_info_view(file_info_view_model(Some(&song), &[]));
        let _ = file_info_view(file_info_view_model(None, &[]));
        let _ = help_view();
        let _ = about_view(test_on_url);
    }
}
