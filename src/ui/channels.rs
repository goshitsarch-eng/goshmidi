//! Channels page: used-only mix rows (ux §3.7).
//!
//! Real T12b implementation of the frozen [`ChannelsViewModel`] contract,
//! consumed as-is: owned `vm` in, one [`cosmic::Element`] out — a scrolled
//! column of channel rows. (Arch composes this page into the view switcher
//! next.)
//!
//! Widget → `Message` map (all variants exist in `message.rs`):
//!
//! | widget | message |
//! |---|
//! | name entry | `ChannelName(ch, String)` (verbatim) |
//! | M / S / lock toggles | `ChannelMute/Solo/Lock(ch, !current)` (negated, loop-toggle precedent) |
//! | volume slider 0–200 | `ChannelLevel(ch, u32)` (verbatim) |
//! | patch dropdown (128 shared labels) | `ChannelPatch(ch, index as u8)` |
//! | meter | display-only (no message) |
//!
//! T12a contract notes (implemented exactly):
//!
//! * Used-only, channel order: `rows` arrives pre-filtered from state (C++
//!   `refreshChannels` visibility rule, `window.cpp:537-546`); the view
//!   renders every row it is given and filters nothing itself.
//! * Number labels are `channel + 1` (`window.cpp:1883-1885`); `channel`
//!   itself stays 0-based in every message.
//! * `patch_options` is the shared 128-entry model (one copy per frame from
//!   arch, relabeled on instrument-map change); each row's dropdown moves
//!   its own copy (the array is `Copy`). Live `Program` reselects only move
//!   `patch` state-side — the model is never rebuilt for a reselect.
//! * The patch dropdown's tooltip shows `patch_label` (the selected patch
//!   under the current map); the meter renders `meter` verbatim (peak +
//!   `*= 0.82` decay live state-side); the solo/mute/volume combine formula
//!   lives state-side too (the view sends raw toggles + slider values).
//! * Defensive clamps (lyrics precedent — a stale value can never panic a
//!   widget): patch → `min(127)`, level → `min(200)`, meter → `0.0–1.0`.
//!
//! Empty states: C++ shows a blank box; the port renders dim placeholders
//! (`No file loaded` with no song, `No channels in this song` when a song
//! carries no used rows) — a deliberate discoverability addition in the
//! sidebar-placeholder spirit.
//!
//! A11y: M/S/lock buttons carry per-channel names + descriptions (lock
//! additionally a tooltip — it is icon-only, and the M/S text buttons have
//! no `.tooltip()`); the volume slider carries a per-channel name +
//! description. The name entry, patch dropdown, and meter have no `.name()`
//! API in the pinned rev, so the row's number label is their accessible
//! context per ux §11 (toolbar/dropdown precedent from T11b); the meter's
//! state is duplicated in the slider value, so no information is
//! pointer-only.
//!
//! [`ChannelsViewModel`]: super::ChannelsViewModel

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget::{
    button, column, container, dropdown, icon, row, scrollable, slider, text, text_input, tooltip,
};

use super::theme;
use super::{ChannelRowViewModel, ChannelsViewModel};
use crate::message::Message;
use crate::state::channels::{LEVEL_MAX_PCT, LEVEL_MIN_PCT};

/// Meter geometry (`GtkLevelBar` 80x10, `window.cpp:1895`).
const METER_WIDTH: f32 = 80.0;
/// Meter geometry (`GtkLevelBar` 80x10, `window.cpp:1895`).
const METER_HEIGHT: f32 = 10.0;
/// Volume slider geometry (`GtkScale` 100px, `window.cpp:1899`).
const SLIDER_WIDTH: f32 = 100.0;
/// Patch dropdown geometry (`GtkDropDown` 180px, `window.cpp:1904`).
const PATCH_WIDTH: f32 = 180.0;
/// Highest valid patch index (128-entry model, `patch.min(127)` parity with
/// the `ChannelPatch` arm).
const MAX_PATCH: u8 = 127;

/// 0-based channel → 1-based display number (`window.cpp:1884`). Widened
/// before adding so a corrupt `255` shows `256`, never wraps to `0`.
fn display_number(channel: u8) -> u16 {
    u16::from(channel) + 1
}

/// Number label text for a row.
fn number_text(channel: u8) -> String {
    display_number(channel).to_string()
}

/// Per-channel accessible name: `Mute channel 3`, `Solo channel 3`, … —
/// sixteen bare `Mute` names would be indistinguishable to AT.
fn control_name(action: &str, channel: u8) -> String {
    format!("{action} channel {}", display_number(channel))
}

/// Volume slider accessible name: `Channel 3 volume`.
fn volume_name(channel: u8) -> String {
    format!("Channel {} volume", display_number(channel))
}

/// Meter clamp: out-of-range state renders at the nearest edge.
fn meter_clamped(meter: f32) -> f32 {
    meter.clamp(0.0, 1.0)
}

/// Volume slider selection: `u32` cannot go negative; the top clamps.
fn level_selected(level_pct: u32) -> u32 {
    level_pct.min(LEVEL_MAX_PCT)
}

/// Patch dropdown selection: nearest valid (the model always holds 128).
fn patch_selected(patch: u8) -> usize {
    usize::from(patch.min(MAX_PATCH))
}

/// Dropdown index → patch message value (defensive; the widget only offers
/// 0–127, mirroring the arm's own `min(127)`).
fn patch_from_index(index: usize) -> u8 {
    index.min(usize::from(MAX_PATCH)) as u8
}

/// M toggle message: the negated current state (loop-toggle precedent).
fn mute_message(channel: u8, muted: bool) -> Message {
    Message::ChannelMute(channel, !muted)
}

/// S toggle message: the negated current state.
fn solo_message(channel: u8, soloed: bool) -> Message {
    Message::ChannelSolo(channel, !soloed)
}

/// Lock toggle message: the negated current state.
fn lock_message(channel: u8, locked: bool) -> Message {
    Message::ChannelLock(channel, !locked)
}

/// One M/S text toggle: visible letter label + selected style + a11y
/// name/description. Built on the classic custom button: the new text
/// `Builder` (`button::standard`) has no `.selected()` passthrough in the
/// pinned rev (only the icon/image builders do), while `button::custom`
/// exposes the underlying selected state directly. No `.tooltip()` on
/// either classic or stock text buttons; the letter is the visible label,
/// unlike the icon-only lock below.
fn mix_toggle(
    label: &'static str,
    action: &'static str,
    description: &'static str,
    channel: u8,
    active: bool,
    message: Message,
) -> Element<'static, Message> {
    button::custom(text::body(label))
        .on_press(message)
        .selected(active)
        .padding(8)
        .name(control_name(action, channel))
        .description(description)
        .into()
}

/// One channel row: number + name + M/S + meter + volume + lock + patch, in
/// C++ order (`window.cpp:1882-1906`). All owned data — the row is `'static`
/// and the page keeps the caller's lifetime.
fn channel_row(
    row: ChannelRowViewModel,
    patch_options: [&'static str; 128],
) -> Element<'static, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let ChannelRowViewModel {
        channel,
        name,
        muted,
        soloed,
        locked,
        meter,
        level_pct,
        patch,
        patch_label,
    } = row;

    let number = text::body(number_text(channel));

    // No `.name()` API in the pinned rev (T11b precedent): the number label
    // beside the entry is its accessible context.
    let name_entry = text_input("", name)
        .on_input(move |value| Message::ChannelName(channel, value))
        .width(Length::Fill);

    let mute = mix_toggle(
        "M",
        "Mute",
        "Silence this channel",
        channel,
        muted,
        mute_message(channel, muted),
    );
    let solo = mix_toggle(
        "S",
        "Solo",
        "Solo this channel, attenuating the rest",
        channel,
        soloed,
        solo_message(channel, soloed),
    );

    // Display-only (ux gap 2): `progress_bar` on the state-decayed meter.
    // No name API; the slider value duplicates the state (ux §11).
    let meter_bar = cosmic::widget::determinate_linear(meter_clamped(meter))
        .width(Length::Fixed(METER_WIDTH))
        .girth(Length::Fixed(METER_HEIGHT));

    let volume = container(
        slider(
            LEVEL_MIN_PCT..=LEVEL_MAX_PCT,
            level_selected(level_pct),
            move |value| Message::ChannelLevel(channel, value),
        )
        .name(volume_name(channel))
        .description("Channel volume, 0 to 200 percent"),
    )
    .width(Length::Fixed(SLIDER_WIDTH));

    let lock = button::icon(icon::from_name("channel-secure-symbolic"))
        .on_press(lock_message(channel, locked))
        .selected(locked)
        .tooltip(control_name("Lock", channel))
        .name(control_name("Lock", channel))
        .description(format!(
            "Lock channel {} to its patch",
            display_number(channel)
        ));

    // The shared model moves in per row (one 128-label copy each); the
    // tooltip shows the selected patch under the current map.
    let patch_dropdown = container(tooltip(
        dropdown(
            patch_options.to_vec(),
            Some(patch_selected(patch)),
            move |index| Message::ChannelPatch(channel, patch_from_index(index)),
        ),
        text::body(patch_label),
        tooltip::Position::Top,
    ))
    .width(Length::Fixed(PATCH_WIDTH));

    // Row spacing 8 (`window.cpp:1881`).
    row::with_children(vec![
        number.into(),
        name_entry.into(),
        mute,
        solo,
        meter_bar.into(),
        volume.into(),
        lock.into(),
        patch_dropdown.into(),
    ])
    .spacing(8)
    .align_y(cosmic::iced::Alignment::Center)
    .into()
}

/// Dim page placeholder (empty states — deliberate addition, see module docs).
fn page_placeholder(label: &'static str) -> Element<'static, Message> {
    container(text::body(label).class(theme::text_class(theme::dim_label)))
        .width(Length::Fill)
        .align_x(cosmic::iced::Alignment::Center)
        .padding(12)
        .into()
}

/// Channels page. `vm` is the whole contract — no other `App` access. By
/// value: owned data moves into the widgets (see [`ChannelsViewModel`]).
pub fn view<'a>(vm: ChannelsViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let ChannelsViewModel {
        rows,
        patch_options,
        has_song,
    } = vm;

    if !has_song {
        return page_placeholder("No file loaded");
    }
    if rows.is_empty() {
        return page_placeholder("No channels in this song");
    }

    // Rows box spacing 4, margins 8/8 (`window.cpp:1872-1875`).
    let mut children = Vec::with_capacity(rows.len());
    for row in rows {
        children.push(channel_row(row, patch_options));
    }
    container(
        scrollable(
            column::with_children(children)
                .spacing(4)
                .width(Length::Fill),
        )
        .height(Length::Fill),
    )
    .padding([0, 8])
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::channels::patch_options;

    fn test_row(channel: u8) -> ChannelRowViewModel {
        ChannelRowViewModel {
            channel,
            name: format!("Track{channel}"),
            muted: false,
            soloed: false,
            locked: false,
            meter: 0.5,
            level_pct: 100,
            patch: 0,
            patch_label: "Acoustic Grand Piano".to_string(),
        }
    }

    fn test_vm() -> ChannelsViewModel {
        ChannelsViewModel {
            rows: vec![test_row(0), test_row(9)],
            patch_options: patch_options(0),
            has_song: true,
        }
    }

    #[test]
    fn number_text_is_one_based_without_wrap() {
        assert_eq!(number_text(0), "1");
        assert_eq!(number_text(9), "10");
        assert_eq!(number_text(15), "16");
        // A corrupt 255 shows 256 (widened before adding — never wraps).
        assert_eq!(number_text(255), "256");
    }

    #[test]
    fn control_names_qualify_every_row() {
        assert_eq!(control_name("Mute", 0), "Mute channel 1");
        assert_eq!(control_name("Solo", 9), "Solo channel 10");
        assert_eq!(control_name("Lock", 15), "Lock channel 16");
        assert_eq!(volume_name(0), "Channel 1 volume");
        assert_eq!(volume_name(9), "Channel 10 volume");
    }

    #[test]
    fn clamps_pin_widget_edges() {
        assert_eq!(meter_clamped(0.5), 0.5);
        assert_eq!(meter_clamped(-1.0), 0.0);
        assert_eq!(meter_clamped(2.0), 1.0);
        assert_eq!(level_selected(0), 0);
        assert_eq!(level_selected(200), 200);
        assert_eq!(level_selected(999), 200);
        assert_eq!(patch_selected(0), 0);
        assert_eq!(patch_selected(127), 127);
        assert_eq!(patch_selected(200), 127);
        assert_eq!(patch_from_index(0), 0);
        assert_eq!(patch_from_index(127), 127);
        assert_eq!(patch_from_index(999), 127);
    }

    #[test]
    fn toggles_send_the_negated_state() {
        assert!(matches!(
            mute_message(3, false),
            Message::ChannelMute(3, true)
        ));
        assert!(matches!(
            mute_message(3, true),
            Message::ChannelMute(3, false)
        ));
        assert!(matches!(
            solo_message(3, false),
            Message::ChannelSolo(3, true)
        ));
        assert!(matches!(
            solo_message(3, true),
            Message::ChannelSolo(3, false)
        ));
        assert!(matches!(
            lock_message(3, false),
            Message::ChannelLock(3, true)
        ));
        assert!(matches!(
            lock_message(3, true),
            Message::ChannelLock(3, false)
        ));
    }

    #[test]
    fn view_builds_across_contract_shapes() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes: rows, no-song, song-without-rows,
        // all 16 rows, and corrupt values (OOR patch/level/meter/channel).
        let _ = view(test_vm());
        let _ = view(ChannelsViewModel {
            rows: Vec::new(),
            patch_options: patch_options(0),
            has_song: false,
        });
        let _ = view(ChannelsViewModel {
            rows: Vec::new(),
            patch_options: patch_options(2),
            has_song: true,
        });
        let all: Vec<ChannelRowViewModel> = (0..16).map(test_row).collect();
        let _ = view(ChannelsViewModel {
            rows: all,
            patch_options: patch_options(1),
            has_song: true,
        });
        let mut corrupt = test_row(255);
        corrupt.muted = true;
        corrupt.soloed = true;
        corrupt.locked = true;
        corrupt.meter = 99.0;
        corrupt.level_pct = 999;
        corrupt.patch = 255;
        corrupt.patch_label = String::new();
        corrupt.name = String::new();
        let _ = view(ChannelsViewModel {
            rows: vec![corrupt],
            patch_options: patch_options(99),
            has_song: true,
        });
    }
}
