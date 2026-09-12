//! Song-settings manual action stubs (ux §4 `app.save/load-settings`).
//!
//! Real T13b implementation of the frozen [`SongSettingsViewModel`]
//! contract, consumed as-is: owned `vm` in, one [`cosmic::Element`] out —
//! the two C++ File-menu entries (`window.cpp:1589-1590`) as buttons. The
//! full menu lands in T14; until then these stubs give composition (and
//! tests) the exact manual surfaces to place. (Arch composes them next.)
//!
//! Widget → `Message` map (both variants exist in `message.rs`, with arms
//! in `app.rs` today):
//!
//! | widget | message |
//! |---|
//! | `Load song settings` | `SongSettingsLoad` |
//! | `Save song settings` | `SongSettingsSave` |
//!
//! T13 contract notes (implemented exactly):
//!
//! * Order + labels are verbatim C++ (Load then Save, `window.cpp:1589-1590`).
//! * The buttons stay live with no song (C++ parity — the menu items are
//!   never desensitized; the arms no-op on an empty song,
//!   `window.cpp:633-635,663-665`); `has_song` is informational only
//!   (transport `has_song` precedent).
//!
//! A11y: both buttons carry names + descriptions.
//!
//! [`SongSettingsViewModel`]: super::SongSettingsViewModel

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget::{button, row};

use super::SongSettingsViewModel;
use crate::message::Message;

/// `Load song settings` label (`window.cpp:1589`, verbatim).
pub const LABEL_LOAD: &str = "Load song settings";
/// `Save song settings` label (`window.cpp:1590`, verbatim).
pub const LABEL_SAVE: &str = "Save song settings";

/// Song-settings manual actions. `vm` is the whole contract — no other
/// `App` access. By value: owned data moves into the widgets (see
/// [`SongSettingsViewModel`]).
pub fn view<'a>(vm: SongSettingsViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    // `has_song` gates nothing (C++ parity — see module docs).
    let SongSettingsViewModel {
        has_song: _has_song,
    } = vm;

    row::with_children(vec![
        button::standard(LABEL_LOAD)
            .on_press(Message::SongSettingsLoad)
            .name(LABEL_LOAD)
            .description("Load this song's saved settings file")
            .into(),
        button::standard(LABEL_SAVE)
            .on_press(Message::SongSettingsSave)
            .name(LABEL_SAVE)
            .description("Save this song's settings to its settings file")
            .into(),
    ])
    .spacing(8)
    .width(Length::Shrink)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_verbatim_cpp_menu_entries() {
        assert_eq!(LABEL_LOAD, "Load song settings");
        assert_eq!(LABEL_SAVE, "Save song settings");
    }

    #[test]
    fn view_builds_with_and_without_song() {
        // `view` is not introspectable, but it must build without panic in
        // both shapes — and the buttons stay live either way (C++ parity).
        let _ = view(SongSettingsViewModel { has_song: true });
        let _ = view(SongSettingsViewModel { has_song: false });
    }
}
