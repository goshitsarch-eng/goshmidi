//! Playlist sidebar: basename rows + 8-tool toolbar (ux §3.1).
//!
//! Real T10b implementation of the frozen [`PlaylistViewModel`] contract,
//! consumed as-is: owned `vm` in, one [`cosmic::Element`] out — a column of
//! `Playlist` header, scrollable rows, and the toolbar row. (Arch composes
//! this pane into the real layout next — width fraction, collapse toggle,
//! `dnd_destination` wrapper — like C++ `AdwOverlaySplitView` 0.28.)
//!
//! Widget → `Message` map (all variants exist in `message.rs`):
//!
//! | widget | message |
//! |---|---|
//! | row press | `PlaylistSelect(i)` (select-only) |
//! | row double-click | `PlaylistRowActivated(i)` (select + load + play) |
//! | row Play button (click or Enter) | `PlaylistRowActivated(i)` |
//! | Add | `OpenFilesDialog` (D1) |
//! | Remove | `PlaylistRemove(selected)` |
//! | Move up / Move down | `PlaylistMoveUp(selected)` / `PlaylistMoveDown(selected)` |
//! | Shuffle / Clear | `PlaylistShuffle` / `PlaylistClear` |
//! | Open playlist | `PlaylistLoadDialog` (D9) |
//! | Save playlist | `PlaylistSaveDialog` (D10) |
//!
//! U11 verdict — click-select vs double-click/Enter-play:
//!
//! * Double-click is FULL parity, no Play-affordance fallback needed:
//!   `mouse_area` exposes press + double-click detection
//!   (`on_press` + `on_double_click`, iced `widget/src/mouse_area.rs`),
//!   and its docs pin the coexistence sequence
//!   (`on_press` → `on_release` → `on_press` → `on_double_click` → …),
//!   so a double-click selects (presses) then activates — exactly the C++
//!   `row-activated` handler (`setCurrentIndex` + `loadCurrent`, and the
//!   `PlaylistRowActivated` arm re-selects anyway).
//! * Enter is PARTIAL parity with a per-row keyboard path: a plain `button`
//!   cannot distinguish Enter from click (both produce the same `on_press`
//!   message — iced `widget/src/button.rs` — so a select-on-press button
//!   would only select on Enter), and `mouse_area` is not focusable. Each
//!   row therefore carries an explicit Play icon button whose Enter/Space
//!   activation emits `PlaylistRowActivated(i)`. Gap vs C++: after
//!   click-selecting a row, Enter does not play until focus is tabbed to
//!   that row's Play button (`GtkListBox` keeps row focus; iced has no
//!   focusable list row in the pinned rev — `ListButton` is single-press
//!   only, same as `button`).
//!
//! Sensitivity (C++ `window.cpp:1487-1568`): Remove/Up/Down early-return
//! with no row selected while staying live. The port renders them
//! insensitive when `selected` is `None` — no valid index exists to send
//! (sending 0 would wrongly act on the first item), and the observable
//! outcome is identical (click → nothing happens). All other tools stay
//! live like C++ (Shuffle/Clear no-op on an empty list state-side).
//!
//! Empty state: C++ shows a blank list box; the port renders a dim
//! `Playlist is empty` placeholder (deliberate discoverability addition).
//! Row hover feedback is absent (no hover-styled press/double-click widget
//! in the pinned rev) — cosmetic delta only; selection, tooltips, and the
//! Play buttons carry the UX.
//!
//! [`PlaylistViewModel`]: super::PlaylistViewModel

use std::path::Path;

use cosmic::Element;
use cosmic::iced::Alignment;
use cosmic::iced::Length;
use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit};
use cosmic::widget::{button, column, container, icon, mouse_area, row, scrollable, text};

use super::PlaylistViewModel;
use super::theme;
use crate::message::Message;

/// Basename for a row (`std::filesystem::path(item).filename()` parity,
/// `window.cpp:505`). `file_name` is `None` only for degenerate paths
/// (root, `..`, empty) — those fall back to the full path so the row is
/// never blank (C++ would render an empty label there).
fn basename_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.as_os_str().to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// U11 row mapping, factored pure so the contract is unit-pinned (iced
/// `Element`s are not introspectable): press selects, double-click (and
/// the row Play button / its Enter key) activates.
fn row_messages(index: usize) -> (Message, Message) {
    (
        Message::PlaylistSelect(index),
        Message::PlaylistRowActivated(index),
    )
}

/// One toolbar tool: freedesktop symbolic icon + tooltip/a11y name +
/// description + message (`None` renders insensitive — see module docs).
struct Tool {
    icon: &'static str,
    name: &'static str,
    description: &'static str,
    message: Option<Message>,
}

/// The 8 sidebar tools in C++ order with verbatim tooltip strings
/// (`window.cpp:1487-1568`: `Add files`, `Remove`, `Move up`, `Move down`,
/// `Shuffle`, `Clear`, `Open playlist`, `Save playlist`).
fn tools(selected: Option<usize>) -> [Tool; 8] {
    [
        Tool {
            icon: "list-add-symbolic",
            name: "Add files",
            description: "Add MIDI files to the playlist",
            message: Some(Message::OpenFilesDialog),
        },
        Tool {
            icon: "list-remove-symbolic",
            name: "Remove",
            description: "Remove the selected song",
            message: selected.map(Message::PlaylistRemove),
        },
        Tool {
            icon: "go-up-symbolic",
            name: "Move up",
            description: "Move the selected song up",
            message: selected.map(Message::PlaylistMoveUp),
        },
        Tool {
            icon: "go-down-symbolic",
            name: "Move down",
            description: "Move the selected song down",
            message: selected.map(Message::PlaylistMoveDown),
        },
        Tool {
            icon: "media-playlist-shuffle-symbolic",
            name: "Shuffle",
            description: "Shuffle the playlist",
            message: Some(Message::PlaylistShuffle),
        },
        Tool {
            icon: "edit-clear-symbolic",
            name: "Clear",
            description: "Clear the playlist",
            message: Some(Message::PlaylistClear),
        },
        Tool {
            icon: "document-open-symbolic",
            name: "Open playlist",
            description: "Open a playlist file",
            message: Some(Message::PlaylistLoadDialog),
        },
        Tool {
            icon: "document-save-symbolic",
            name: "Save playlist",
            description: "Save the playlist to a file",
            message: Some(Message::PlaylistSaveDialog),
        },
    ]
}

/// One toolbar icon button: symbolic icon + tooltip + a11y name/description;
/// `None` renders insensitive (Remove/Up/Down with no selection).
fn tool_button(tool: Tool) -> Element<'static, Message> {
    button::icon(icon::from_name(tool.icon))
        .on_press_maybe(tool.message)
        .tooltip(tool.name)
        .name(tool.name)
        .description(tool.description)
        .into()
}

/// One playlist row: basename label (left, middle-ellipsized) + Play button,
/// wrapped in `mouse_area` for press-select + double-click-activate. The
/// inner Play button captures presses over its own bounds, so clicking it
/// activates without selecting first (harmless: the activate arm selects).
fn playlist_row(index: usize, basename: String, selected: bool) -> Element<'static, Message> {
    let (select, activate) = row_messages(index);
    let play_description = format!("Play {basename}");
    let label = text::body(basename)
        .width(Length::Fill)
        .ellipsize(Ellipsize::Middle(EllipsizeHeightLimit::Lines(1)));
    let play = button::icon(icon::from_name("media-playback-start-symbolic"))
        .on_press(activate.clone())
        .tooltip("Play")
        .name("Play")
        .description(play_description);
    let content = container(
        row::with_children(vec![label.into(), play.into()])
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([6, 8])
    .class(cosmic::theme::Container::custom(theme::playlist_row(
        selected,
    )));
    mouse_area(content)
        .on_press(select)
        .on_double_click(activate)
        .into()
}

/// Playlist sidebar pane. `vm` is the whole contract — no other `App`
/// access. By value: owned data moves into the widgets (see
/// [`PlaylistViewModel`]).
pub fn view<'a>(vm: PlaylistViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let PlaylistViewModel { items, selected } = vm;

    let header = container(text::heading("Playlist")).padding([8, 12]);

    let list: Element<'_, Message> = if items.is_empty() {
        container(text::body("Playlist is empty").class(theme::text_class(theme::dim_label)))
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .padding(12)
            .into()
    } else {
        let mut rows = Vec::with_capacity(items.len());
        for (index, path) in items.iter().enumerate() {
            rows.push(playlist_row(
                index,
                basename_of(path),
                Some(index) == selected,
            ));
        }
        scrollable(column::with_children(rows).spacing(2).width(Length::Fill))
            .height(Length::Fill)
            .into()
    };

    let toolbar = container(
        row::with_children(tools(selected).map(tool_button))
            .spacing(4)
            .align_y(Alignment::Center),
    )
    .padding([8, 8])
    .width(Length::Fill);

    column::with_children(vec![header.into(), list, toolbar.into()])
        .spacing(4)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn basename_of_matches_cpp_filename() {
        assert_eq!(basename_of(Path::new("/m/dir/song.mid")), "song.mid");
        assert_eq!(basename_of(Path::new("rel/my song.KAR")), "my song.KAR");
        assert_eq!(basename_of(Path::new("bare.lst")), "bare.lst");
        // Degenerate paths fall back to the full path (never blank).
        assert_eq!(basename_of(Path::new("/")), "/");
        assert_eq!(basename_of(Path::new("")), "");
    }

    #[test]
    fn basename_of_handles_non_utf8_lossily() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![0xFF, 0xFE, b'.', b'm', b'i', b'd']);
        let name = basename_of(Path::new(&raw));
        assert!(name.ends_with(".mid"), "{name}");
        assert!(name.contains('�'), "{name}");
    }

    #[test]
    fn row_messages_map_press_to_select_and_double_click_to_activate() {
        let (select, activate) = row_messages(3);
        assert!(matches!(select, Message::PlaylistSelect(3)));
        assert!(matches!(activate, Message::PlaylistRowActivated(3)));
    }

    #[test]
    fn tools_keep_cpp_order_icons_tooltips_and_messages() {
        let got = tools(Some(2));
        let icons: Vec<&str> = got.iter().map(|t| t.icon).collect();
        assert_eq!(
            icons,
            [
                "list-add-symbolic",
                "list-remove-symbolic",
                "go-up-symbolic",
                "go-down-symbolic",
                "media-playlist-shuffle-symbolic",
                "edit-clear-symbolic",
                "document-open-symbolic",
                "document-save-symbolic",
            ]
        );
        let names: Vec<&str> = got.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "Add files",
                "Remove",
                "Move up",
                "Move down",
                "Shuffle",
                "Clear",
                "Open playlist",
                "Save playlist",
            ]
        );
        assert!(matches!(got[0].message, Some(Message::OpenFilesDialog)));
        assert!(matches!(got[1].message, Some(Message::PlaylistRemove(2))));
        assert!(matches!(got[2].message, Some(Message::PlaylistMoveUp(2))));
        assert!(matches!(got[3].message, Some(Message::PlaylistMoveDown(2))));
        assert!(matches!(got[4].message, Some(Message::PlaylistShuffle)));
        assert!(matches!(got[5].message, Some(Message::PlaylistClear)));
        assert!(matches!(got[6].message, Some(Message::PlaylistLoadDialog)));
        assert!(matches!(got[7].message, Some(Message::PlaylistSaveDialog)));
    }

    #[test]
    fn selection_gated_tools_go_insensitive_without_selection() {
        let got = tools(None);
        assert!(got[1].message.is_none());
        assert!(got[2].message.is_none());
        assert!(got[3].message.is_none());
        // The rest stay live like C++.
        for tool in [&got[0], &got[4], &got[5], &got[6], &got[7]] {
            assert!(tool.message.is_some(), "{}", tool.name);
        }
    }

    #[test]
    fn view_builds_for_empty_and_populated_models() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes (empty, unselected, selected,
        // out-of-range-selected).
        let _ = view(PlaylistViewModel::default());
        let items = vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.kar")];
        let _ = view(PlaylistViewModel {
            items: items.clone(),
            selected: None,
        });
        let _ = view(PlaylistViewModel {
            items: items.clone(),
            selected: Some(1),
        });
        let _ = view(PlaylistViewModel {
            items,
            selected: Some(99),
        });
    }
}
