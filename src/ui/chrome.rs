//! Shared modal-dialog chrome: the T12 FIXED pattern, generalized (D20).
//!
//! [`wrap_modal`] is the exact T12 prefs overlay (`app.rs` D2 chrome,
//! `Stack` + full-bounds capturing `mouse_area` + dim scrim) factored so
//! every T13 dialog composes identically — composition builds its `dialog()`
//! sheet (title + [`midi_setup`][super::midi_setup] /
//! [`jump_loop`][super::jump_loop] / [`info_dialogs`][super::info_dialogs]
//! content + actions) and wraps it; no per-dialog chrome code, no `app.rs`
//! touch to adopt it.
//!
//! Capture semantics (verified in pinned framework source, rev `7e82198`):
//! iced offers the event to the dialog content first, and anything the
//! dialog does not consume is swallowed by the area, which captures left
//! press/release + touch unconditionally when the cursor is over its bounds
//! (`mouse_area.rs` — no handler needed) and the wheel via `on_scroll` into
//! `ChooserCancelled` (the silent no-op arm: no state change, no toast).
//! Right- and middle-press still propagate, but no base widget binds them.
//! The wrap container also paints the dim scrim (C++ modal windows dim
//! their parent).
//!
//! Composition recipe (per dialog, in `App::view`):
//!
//! ```ignore
//! // 1. Build the sheet: `dialog()` + title + content view + Cancel.
//! let sheet: Element<'_, Message> = cosmic::widget::dialog()
//!     .title("Jump to bar")
//!     .control(jump_loop::jump_view(vm, on_edit))
//!     .secondary_action(button::standard("Cancel").on_press(Message::CloseDialog))
//!     .into();
//! // 2. Wrap base + sheet identically for every dialog.
//! chrome::wrap_modal(base, sheet)
//! ```
//!
//! Recommended actions per dialog (C++ parity — the views document each).
//! Confirm buttons live IN the content views (C++ field order — D3's
//! `Test note` + suggested `Apply` row, D7's `Jump`, D8's `Loop`, the
//! info-dialog `OK`s); composition only adds a `Cancel` secondary where
//! C++ closed via window-X without committing:
//!
//! | dialog | content buttons | chrome secondary |
//! |---|---|---|
//! | D3 MIDI Setup | `Test note` → `MidiTestNote`, suggested `Apply` → `MidiApply` | `Cancel` → `CloseDialog` (closes unapplied) |
//! | D7 Jump | `Jump` → `JumpToBar(bar)` | `Cancel` → `CloseDialog` |
//! | D8 Loop | `Loop` → `LoopSet{from,to}` | `Cancel` → `CloseDialog` (resets the toggle) |
//! | D6 File info | `OK` → `CloseDialog` | — |
//! | D5 Help | `OK` → `CloseDialog` | — |
//! | D4 About | `Close` → `CloseDialog` | — |
//!
//! A11y: the chrome adds no focusable controls — names live on the dialog
//! content + actions (each view documents its own).

use cosmic::Element;
use cosmic::iced::{Alignment, Length};

use crate::message::Message;

/// Scrim opacity (the T12 D2 value — dim, content stays legible).
const SCRIM_ALPHA: f32 = 0.45;

/// Wrap `base` + a fully-built `dialog()` `sheet` in the FIXED modal
/// chrome: a `Stack` whose top layer centers the sheet over a dim scrim
/// inside a full-bounds capturing `mouse_area` (see module docs). The base
/// keeps its size; the sheet centers both axes.
pub fn wrap_modal<'a>(
    base: Element<'a, Message>,
    sheet: Element<'a, Message>,
) -> Element<'a, Message> {
    let overlay: Element<'a, Message> = cosmic::iced::widget::mouse_area(
        cosmic::widget::container(sheet)
            .style(|_theme| cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(cosmic::iced::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: SCRIM_ALPHA,
                })),
                ..Default::default()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .on_scroll(|_| Message::ChooserCancelled)
    .into();
    cosmic::iced::widget::Stack::with_children(vec![base, overlay])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::widget::{button, dialog, text};

    #[test]
    fn wrap_builds_over_any_base_and_sheet() {
        // `wrap_modal` is not introspectable, but it must build without
        // panic over representative shapes: text base + titled dialog,
        // empty base + action dialog.
        let base: Element<'static, Message> = text::body("base").into();
        let sheet: Element<'static, Message> = dialog()
            .title("Jump to bar")
            .control(text::body("spin"))
            .into();
        let _ = wrap_modal(base, sheet);
        let base: Element<'static, Message> = text::body("").into();
        let sheet: Element<'static, Message> = dialog()
            .title("MIDI Setup")
            .primary_action(button::standard("Apply").on_press(Message::MidiApply))
            .secondary_action(button::standard("Cancel").on_press(Message::CloseDialog))
            .into();
        let _ = wrap_modal(base, sheet);
    }

    #[test]
    fn scrim_dims_without_hiding() {
        assert!((0.0..1.0).contains(&SCRIM_ALPHA));
        assert_eq!(SCRIM_ALPHA, 0.45);
    }
}
