//! D7 jump-to-bar + D8 loop-range dialog contents (ux D7/D8).
//!
//! Real T13b implementation of the frozen [`JumpViewModel`]/[`LoopViewModel`]
//! contracts, consumed as-is: owned `vm` in, one [`cosmic::Element`] out —
//! label + range-bound spin(s) + confirm button, in C++ order. (Arch
//! composes these contents into the [`chrome`][super::chrome] wrap next —
//! see the composition contracts below.)
//!
//! Widget → `Message` map (confirm variants exist in `message.rs`; the spin
//! edits ride caller-supplied constructors, prefs `on_scratch` precedent):
//!
//! | widget | message |
//! |---|
//! | D7 bar spin `1..=last_bar` | `on_edit(u32)` (scratch — arch adds the variant) |
//! | D7 `Jump` | `JumpToBar(bar)` (pause-jump-resume, dialog closes) |
//! | D8 from/to spins `1..=last_bar` | `on_from`/`on_to(u32)` (scratch) |
//! | D8 `Loop` | `LoopSet{from,to}` (enables + forces the toggle ON + closes) |
//!
//! D7 contract notes (implemented exactly):
//!
//! * Label + spin + button are verbatim C++ (`window.cpp:789-793`); the spin
//!   ranges `1..=max(1, bars)` (`window.cpp:790`).
//! * OK semantics: `JumpToBar(bar)` — the arch arm pauses if running, jumps,
//!   resumes if it was running, and closes (`window.cpp:819-825`; the arm
//!   exists in `app.rs` today). Cancel semantics: C++ has NO Cancel button
//!   (window-X destroys unapplied) — composition adds a `Cancel` secondary
//!   → `CloseDialog` (see [`chrome`][super::chrome]).
//!
//! D8 contract notes (implemented exactly):
//!
//! * Labels + spins + button are verbatim C++ (`window.cpp:849-859`); both
//!   spins range `1..=max(1, bars)`, seeded from `loopStart/loopEnd`
//!   (`window.cpp:852-853`; the builder clamps OOR seeds like the spins).
//! * OK semantics: `LoopSet{from,to}` — the arch arm calls `setLoop(a, c)`
//!   (enables), forces the header toggle ON, and closes
//!   (`window.cpp:864-871`; the arm exists in `app.rs` today).
//! * Cancel semantics: closing unconfirmed resets the toggle — the port's
//!   toggle derives from `loop_enabled`, which only `LoopSet` flips, so
//!   `CloseDialog` falls out correct (C++ `window.cpp:873-880`; already
//!   documented at the `app.rs` close arm).
//! * U3 FIX: C++ `showLoop` early-returns when the header toggle is OFF
//!   (`window.cpp:835-838`), so the `app.loop` menu item with the toggle
//!   OFF is a silent no-op — the dialog never opens. The port bans the
//!   no-op: menu Loop takes the toggle-ON path (opens the dialog;
//!   `LoopToggled(true)` in the current arms), then this dialog confirms or
//!   cancels exactly like the header-toggle path. The force-ON lives in the
//!   menu arm (arch); this view implements the dialog half per C++.
//!
//! Composition contracts (arch wires next — `app`/`state`/`message` are
//! untouched here):
//!
//! * D7 shell state: `jump_bar: u32` scratch, seeded to the current bar
//!   (or 1) on `ShowDialog(JumpToBar)`; a NEW `JumpBarEdited(u32)`-shaped
//!   message (name is arch's) feeds `on_edit` WITHOUT jumping; `JumpToBar`
//!   confirms; `CloseDialog` abandons. No-song: C++ `showJump`
//!   early-returns (`window.cpp:777-778`) — the trigger must not open the
//!   dialog without a song (like the D8 no-song arm below).
//! * D8 shell state: `(loop_from, loop_to)` scratch, seeded from
//!   `loop_start/loop_end` on open; NEW `LoopFromEdited/LoopToEdited(u32)`-
//!   shaped messages feed `on_from`/`on_to` WITHOUT applying; `LoopSet`
//!   confirms; `CloseDialog` abandons (toggle resets). No-song: C++
//!   `showLoop` early-returns (`window.cpp:832-834`); the current
//!   `LoopToggled(true)` arm opens unconditionally (documented intended
//!   deviation) — arch reconciles on composition.
//!
//! A11y: spins carry the row label as the a11y name (prefs precedent);
//! confirm buttons carry names + descriptions.
//!
//! [`JumpViewModel`]: super::JumpViewModel
//! [`LoopViewModel`]: super::LoopViewModel

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget::{button, column, spin_button, text};

use super::{JumpViewModel, LoopViewModel};
use crate::message::Message;
use crate::state::dialogs::{jump_range, loop_range, loop_seeds};

/// D7 dialog title (`window.cpp:782`, verbatim — composition sets it).
pub const JUMP_TITLE: &str = "Jump to bar";
/// D7 bar label (`window.cpp:789`, verbatim).
pub const JUMP_LABEL: &str = "Bar number:";
/// D7 confirm label (`window.cpp:792`, verbatim).
pub const JUMP_CONFIRM: &str = "Jump";
/// D8 dialog title (`window.cpp:842`, verbatim — composition sets it).
pub const LOOP_TITLE: &str = "Loop";
/// D8 from label (`window.cpp:854`, verbatim).
pub const LOOP_FROM_LABEL: &str = "From bar";
/// D8 to label (`window.cpp:856`, verbatim).
pub const LOOP_TO_LABEL: &str = "To bar";
/// D8 confirm label (`window.cpp:858`, verbatim).
pub const LOOP_CONFIRM: &str = "Loop";
/// Focus-on-open target (T14d G4 partial): composition focuses the D7
/// confirm button on open (the `focus(id)` op unfocuses everything else —
/// pinned `iced/core/src/widget/operation/focusable.rs:30-57` — so base
/// Enter cannot fire silently under the dialog).
pub const JUMP_CONFIRM_ID: &str = "jump-confirm";
/// Focus-on-open target (T14d G4 partial): composition focuses the D8
/// confirm button on open (same mechanism as [`JUMP_CONFIRM_ID`]).
pub const LOOP_CONFIRM_ID: &str = "loop-confirm";

/// Build the D7 view model: the scratch bar clamped to `1..=last_bar`
/// (range from the shared [`jump_range`]). Pure — no `App` access;
/// composition calls it fresh per frame.
pub fn jump_view_model(bar: u32, bars: i32) -> JumpViewModel {
    let (_, last_bar) = jump_range(bars);
    JumpViewModel {
        bar: bar.clamp(1, last_bar),
        last_bar,
    }
}

/// Build the D8 view model: the from/to seeds clamped to `1..=last_bar`
/// (shared [`loop_seeds`]/[`loop_range`] — no from≤to enforcement, like
/// C++ `setLoop`). Pure — no `App` access; composition calls it fresh per
/// frame.
pub fn loop_view_model(from: u32, to: u32, bars: i32) -> LoopViewModel {
    let (from, to) = loop_seeds(from, to, bars);
    LoopViewModel {
        from,
        to,
        last_bar: loop_range(bars).1,
    }
}

/// One bar spin: `1..=last_bar`, step 1, the row label doubling as the
/// widget's a11y name (prefs precedent). The value/clamp shown is the
/// scratch verbatim (the builder/OOR clamp guarantees range).
fn bar_spin(
    label: &'static str,
    value: u32,
    last_bar: u32,
    on_edit: impl Fn(u32) -> Message + 'static,
) -> Element<'static, Message> {
    spin_button(value.to_string(), label, value, 1, 1, last_bar, on_edit).into()
}

/// D7 jump-to-bar dialog content: label + spin + `Jump` (C++ order,
/// `window.cpp:789-793`; margins 18 / spacing 12, `window.cpp:783-787`).
/// `on_edit` carries spin steps to the arch scratch (prefs `on_scratch`
/// precedent — per-edit `JumpToBar` would jump-on-step and is banned).
pub fn jump_view<'a>(
    vm: JumpViewModel,
    on_edit: impl Fn(u32) -> Message + 'static,
) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let JumpViewModel { bar, last_bar } = vm;

    column::with_children(vec![
        text::body(JUMP_LABEL).into(),
        bar_spin(JUMP_LABEL, bar, last_bar, on_edit),
        button::standard(JUMP_CONFIRM)
            .on_press(Message::JumpToBar(bar))
            .id(cosmic::iced::core::widget::Id::new(JUMP_CONFIRM_ID))
            .name(JUMP_CONFIRM)
            .description("Jump to the selected bar")
            .into(),
    ])
    .spacing(12)
    .padding(18)
    .width(Length::Fill)
    .into()
}

/// D8 loop-range dialog content: from/to labels + spins + `Loop` (C++ order,
/// `window.cpp:854-859`; margins 18 / spacing 12, `window.cpp:843-847`).
/// `on_from`/`on_to` carry spin steps to the arch scratch (per-edit
/// `LoopSet` would apply-on-step and is banned).
pub fn loop_view<'a>(
    vm: LoopViewModel,
    on_from: impl Fn(u32) -> Message + 'static,
    on_to: impl Fn(u32) -> Message + 'static,
) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let LoopViewModel { from, to, last_bar } = vm;

    column::with_children(vec![
        text::body(LOOP_FROM_LABEL).into(),
        bar_spin(LOOP_FROM_LABEL, from, last_bar, on_from),
        text::body(LOOP_TO_LABEL).into(),
        bar_spin(LOOP_TO_LABEL, to, last_bar, on_to),
        button::standard(LOOP_CONFIRM)
            .on_press(Message::LoopSet {
                start: from,
                end: to,
            })
            .id(cosmic::iced::core::widget::Id::new(LOOP_CONFIRM_ID))
            .name(LOOP_CONFIRM)
            .description("Loop the selected bar range")
            .into(),
    ])
    .spacing(12)
    .padding(18)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_verbatim_cpp() {
        assert_eq!(JUMP_TITLE, "Jump to bar");
        assert_eq!(JUMP_LABEL, "Bar number:");
        assert_eq!(JUMP_CONFIRM, "Jump");
        assert_eq!(LOOP_TITLE, "Loop");
        assert_eq!(LOOP_FROM_LABEL, "From bar");
        assert_eq!(LOOP_TO_LABEL, "To bar");
        assert_eq!(LOOP_CONFIRM, "Loop");
    }

    // NOTE: the shared ranges/clamps are pinned in `state::dialogs`
    // tests — these pin the view builders over them.

    #[test]
    fn builders_clamp_oor_seeds() {
        // In-range seeds pass through.
        let jump = jump_view_model(7, 32);
        assert_eq!((jump.bar, jump.last_bar), (7, 32));
        let range = loop_view_model(3, 12, 32);
        assert_eq!((range.from, range.to, range.last_bar), (3, 12, 32));
        // OOR seeds clamp (C++ spin `set_value` behavior).
        let jump = jump_view_model(0, 32);
        assert_eq!((jump.bar, jump.last_bar), (1, 32));
        let jump = jump_view_model(99, 32);
        assert_eq!((jump.bar, jump.last_bar), (32, 32));
        let range = loop_view_model(0, 999, 32);
        assert_eq!((range.from, range.to, range.last_bar), (1, 32, 32));
        // Degenerate song (0 bars): everything pins to bar 1.
        let jump = jump_view_model(5, 0);
        assert_eq!((jump.bar, jump.last_bar), (1, 1));
        let range = loop_view_model(5, 9, -2);
        assert_eq!((range.from, range.to, range.last_bar), (1, 1, 1));
    }

    #[test]
    fn view_builds_across_contract_shapes() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes. Tests pass `Message::JumpToBar`
        // as the scratch constructor (shape-compatible stand-in — the
        // shell's real constructors are arch-supplied scratch-only
        // variants; `JumpToBar`/`LoopSet` fire only from the confirm
        // buttons in production).
        let _ = jump_view(jump_view_model(7, 32), Message::JumpToBar);
        let _ = jump_view(jump_view_model(1, 1), Message::JumpToBar);
        let _ = jump_view(
            JumpViewModel {
                bar: 99,
                last_bar: 1,
            },
            Message::JumpToBar,
        );
        let _ = loop_view(
            loop_view_model(3, 12, 32),
            |value| Message::LoopSet {
                start: value,
                end: 12,
            },
            |value| Message::LoopSet {
                start: 3,
                end: value,
            },
        );
        let _ = loop_view(
            loop_view_model(1, 1, 1),
            |value| Message::LoopSet {
                start: value,
                end: 1,
            },
            |value| Message::LoopSet {
                start: 1,
                end: value,
            },
        );
        let _ = loop_view(
            LoopViewModel {
                from: 0,
                to: 999,
                last_bar: 32,
            },
            |value| Message::LoopSet {
                start: value,
                end: 32,
            },
            |value| Message::LoopSet {
                start: 1,
                end: value,
            },
        );
    }
}
