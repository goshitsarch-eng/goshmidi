//! D2 preferences dialog shell: General/Lyrics/Piano sections (ux D2).
//!
//! Real T12b implementation of the frozen [`PrefsViewModel`] contract: the
//! whole-`Settings` scratch borrowed in, one [`cosmic::Element`] out —
//! three `settings::section`s in C++ page order (General 7 rows, Lyrics 5,
//! Piano 5 = the 17-row checklist). (Arch composes this content into the
//! dialog chrome next — see the composition contract below.)
//!
//! Widget → message map: every row emits whole-scratch edits through the
//! caller-supplied `on_scratch` constructor (scratch-only — the shell never
//! applies); dialog close commits the narrowed 17-key merge through ONE
//! `apply_prefs`.
//!
//! | widget | scratch field |
//! |---|
//! | Percussion spin 1–16 | `drums_channel` |
//! | Solo % spin 0–100 | `solo_volume_reduction` (U14 — same commit) |
//! | 3 switches | `auto_play` / `auto_advance` / `auto_song_settings` |
//! | SysEx reset dropdown 0–4 | `sysex_reset` |
//! | Instrument map dropdown 0–2 | `instrument_map` |
//! | Lyrics section | exactly [`lyrics_prefs_rows`] (T11b — reused, never duplicated) |
//! | Palette dropdown 0–3 | `highlight_palette` |
//! | Single highlight color entry | `single_color` (text source of truth) |
//! | Velocity switch | `velocity_color` |
//! | Names dropdown 0–3 | `names_visibility` |
//! | Octave switch | `octave_subscript` |
//!
//! T12 contract notes (implemented exactly):
//!
//! * Commit-on-close is KEPT (lead ruling): rows only rewrite the scratch;
//!   the single `PrefsChanged(scratch)` fires at dialog close (arch's close
//!   arm). `solo_volume_reduction` rides that same commit (U14).
//! * The shell carries the D3-owned keys (`lastOutputBackend`,
//!   `lastOutputConnection`, `soundFont`, `advancedPorts`) through the
//!   commit UNCHANGED — no D2 row writes them (every updater clones the
//!   scratch wholesale), and the close arm takes D3 from live settings
//!   anyway (narrow 17-key commit — T12 devil-BLOCKING FIX).
//! * `singleColor` is text-only: no picker (unlike the three Lyrics color
//!   rows) — the text commits verbatim like C++, and the T13 canvas parses
//!   it under `gdk_rgba_parse` parity (gray on garbage, render-only).
//! * Defensive clamps (lyrics precedent): dropdown selections clamp to the
//!   nearest valid option (C++ clamps reset/map via `std::clamp` and leaves
//!   palette/names/alignment raw — the port clamps all five, so corrupt
//!   scratch can never panic a widget); spin labels show the clamped value
//!   while the scratch keeps its raw text until the user steps.
//!
//! Composition contract (arch wires next — `app`/`state`/`message` are
//! untouched here):
//!
//! * shell state: `prefs: Option<(PrefsViewModel, LyricsPickerModels)>` —
//!   `None` while the dialog is shut;
//! * open (`ShowDialog(Preferences)`): clone live settings into the scratch
//!   and seed [`LyricsPickerModels::from_settings`];
//! * per-edit message: a NEW whole-scratch variant (e.g.
//!   `PrefsScratchEdited(SettingsState)`) supplied as `on_scratch`, whose
//!   arm adopts the scratch (re-driving picker models via
//!   [`adopt_prefs_scratch`]) WITHOUT applying — per-edit `PrefsChanged`
//!   would be commit-on-change and is banned;
//! * picker updates: three NEW `(row, ColorPickerUpdate)` constructors
//!   supplied as `on_picker`, whose arms call [`apply_picker_update`];
//! * close: the dialog's close path narrows the scratch to the 17 D2-owned
//!   keys over live settings, then sends ONE `apply_prefs` of the merged
//!   commit (U14 mix repush + output-reseat-noop + save) and clears the
//!   shell state.
//!
//! A11y: spin buttons carry the row title as the a11y name; toggles,
//! dropdowns, and the text field have no `.name()` API in the pinned rev,
//! so their settings titles are the accessible names per ux §11 (T11b
//! precedent); the Lyrics section inherits its T11b a11y story verbatim.
//!
//! [`PrefsViewModel`]: super::PrefsViewModel
//! [`lyrics_prefs_rows`]: super::lyrics::lyrics_prefs_rows
//! [`LyricsPickerModels::from_settings`]: super::lyrics::LyricsPickerModels::from_settings
//! [`adopt_prefs_scratch`]: super::lyrics::adopt_prefs_scratch
//! [`apply_picker_update`]: super::lyrics::apply_picker_update
//! [`Message::PrefsChanged`]: crate::message::Message::PrefsChanged

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget::color_picker::ColorPickerUpdate;
use cosmic::widget::settings::{self};
use cosmic::widget::{column, dropdown, spin_button, text_input, toggler};

use super::PrefsViewModel;
use super::lyrics::{LyricsPickerModels, lyrics_prefs_rows, text_updater};
use crate::message::{Message, SettingsState};

/// SysEx reset dropdown labels (`window.cpp:1214-1218`, verbatim).
pub const RESET_OPTIONS: [&str; 5] = [
    "None",
    "GM Reset",
    "GS Reset (SC-55)",
    "XG Reset",
    "MT-32 Reset",
];
/// Instrument map dropdown labels (`window.cpp:1223-1225`, verbatim).
pub const MAP_OPTIONS: [&str; 3] = ["GM", "GS (SC-55)", "MT-32"];
/// Note-highlighting dropdown labels (`window.cpp:1276-1279`, verbatim).
pub const PALETTE_OPTIONS: [&str; 4] = ["Single color", "Double", "Channels", "Scale"];
/// Note-names dropdown labels (`window.cpp:1291-1294`, verbatim).
pub const NAMES_OPTIONS: [&str; 4] = ["Never", "Minimal", "When activated", "Always"];

/// Prefs text-field width (matches the T11b Lyrics rows — the constant
/// there is private, so the width is restated, not shared).
const TEXT_FIELD_WIDTH: f32 = 240.0;

/// SysEx reset selection: `std::clamp(st.sysexReset, 0, 4)` parity
/// (`window.cpp:1230`).
fn reset_selected(value: i32) -> usize {
    value.clamp(0, 4) as usize
}

/// Instrument map selection: `std::clamp(st.instrumentMap, 0, 2)` parity
/// (`window.cpp:1231`).
fn map_selected(value: i32) -> usize {
    value.clamp(0, 2) as usize
}

/// Palette selection: nearest valid (C++ passes the raw value,
/// `window.cpp:1284` — the port clamps so corrupt scratch cannot panic).
fn palette_selected(value: i32) -> usize {
    value.clamp(0, 3) as usize
}

/// Names selection: nearest valid (C++ passes the raw value,
/// `window.cpp:1298` — clamped like the palette).
fn names_selected(value: i32) -> usize {
    value.clamp(0, 3) as usize
}

/// Spin-button display text: the clamped value (the widget steps from its
/// own clamped base too, so label and steppers always agree).
fn spin_label(value: i32, min: i32, max: i32) -> String {
    value.clamp(min, max).to_string()
}

/// Whole-scratch updater for one spin row: clones the dialog scratch, sets
/// the row's `i32` field, and emits `on_scratch`. Captures only owned data,
/// so the closure is `'static` (spin-button-compatible).
fn int_updater(
    scratch: &SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, i32),
) -> impl Fn(i32) -> Message + 'static {
    let base = scratch.clone();
    move |value| {
        let mut next = base.clone();
        set(&mut next, value);
        on_scratch(next)
    }
}

/// Whole-scratch updater for one dropdown row: clones the dialog scratch,
/// sets the row's `i32` field from the option index, and emits `on_scratch`.
/// `Send + Sync` for the dropdown's `'static` callback bound.
fn index_updater(
    scratch: &SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, i32),
) -> impl Fn(usize) -> Message + Send + Sync + 'static {
    let base = scratch.clone();
    move |index| {
        let mut next = base.clone();
        set(&mut next, index as i32);
        on_scratch(next)
    }
}

/// Whole-scratch updater for one switch row: clones the dialog scratch, sets
/// the row's `bool` field, and emits `on_scratch`.
fn bool_updater(
    scratch: &SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, bool),
) -> impl Fn(bool) -> Message + 'static {
    let base = scratch.clone();
    move |value| {
        let mut next = base.clone();
        set(&mut next, value);
        on_scratch(next)
    }
}

/// Scratch setters, as `fn` pointers so the row builders stay key-agnostic
/// (T11b `text_updater` precedent).
fn set_drums_channel(scratch: &mut SettingsState, value: i32) {
    scratch.drums_channel = value;
}
/// Scratch setter for the solo% row (see [`set_drums_channel`]).
fn set_solo_reduction(scratch: &mut SettingsState, value: i32) {
    scratch.solo_volume_reduction = value;
}
/// Scratch setter for the auto-play row (see [`set_drums_channel`]).
fn set_auto_play(scratch: &mut SettingsState, value: bool) {
    scratch.auto_play = value;
}
/// Scratch setter for the auto-advance row (see [`set_drums_channel`]).
fn set_auto_advance(scratch: &mut SettingsState, value: bool) {
    scratch.auto_advance = value;
}
/// Scratch setter for the auto-song-settings row (see [`set_drums_channel`]).
fn set_auto_song_settings(scratch: &mut SettingsState, value: bool) {
    scratch.auto_song_settings = value;
}
/// Scratch setter for the SysEx reset row (see [`set_drums_channel`]).
fn set_sysex_reset(scratch: &mut SettingsState, value: i32) {
    scratch.sysex_reset = value;
}
/// Scratch setter for the instrument-map row (see [`set_drums_channel`]).
fn set_instrument_map(scratch: &mut SettingsState, value: i32) {
    scratch.instrument_map = value;
}
/// Scratch setter for the palette row (see [`set_drums_channel`]).
fn set_highlight_palette(scratch: &mut SettingsState, value: i32) {
    scratch.highlight_palette = value;
}
/// Scratch setter for the single-color row (see [`set_drums_channel`]).
fn set_single_color(scratch: &mut SettingsState, value: String) {
    scratch.single_color = value;
}
/// Scratch setter for the velocity row (see [`set_drums_channel`]).
fn set_velocity_color(scratch: &mut SettingsState, value: bool) {
    scratch.velocity_color = value;
}
/// Scratch setter for the note-names row (see [`set_drums_channel`]).
fn set_names_visibility(scratch: &mut SettingsState, value: i32) {
    scratch.names_visibility = value;
}
/// Scratch setter for the octave row (see [`set_drums_channel`]).
fn set_octave_subscript(scratch: &mut SettingsState, value: bool) {
    scratch.octave_subscript = value;
}

/// One spin row: settings item with a `spin_button` (the row title doubles
/// as the widget's a11y name).
fn spin_row<'a>(
    title: &'static str,
    scratch: &'a SettingsState,
    value: i32,
    min: i32,
    max: i32,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, i32),
) -> Element<'a, Message> {
    settings::item(
        title,
        spin_button(
            spin_label(value, min, max),
            title,
            value,
            1,
            min,
            max,
            int_updater(scratch, on_scratch, set),
        ),
    )
    .into()
}

/// One switch row: settings item with a `toggler` (no toggler label — the
/// settings title already names the row; a second label would duplicate).
fn toggle_row<'a>(
    title: &'static str,
    active: bool,
    scratch: &'a SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, bool),
) -> Element<'a, Message> {
    settings::item(
        title,
        toggler(active).on_toggle(bool_updater(scratch, on_scratch, set)),
    )
    .into()
}

/// One dropdown row: settings item with a `dropdown` (no `.name()` API —
/// the settings title is the accessible name, T11b precedent).
fn dropdown_row<'a>(
    title: &'static str,
    options: &'static [&'static str],
    selected: usize,
    scratch: &'a SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, i32),
) -> Element<'a, Message> {
    settings::item(
        title,
        dropdown(
            options,
            Some(selected),
            index_updater(scratch, on_scratch, set),
        ),
    )
    .into()
}

/// General section: the C++ 7 rows in order (`window.cpp:1198-1237`).
fn general_section<'a>(
    scratch: &'a SettingsState,
    on_scratch: fn(SettingsState) -> Message,
) -> Element<'a, Message> {
    settings::section()
        .title("General")
        .add(spin_row(
            "Percussion MIDI channel",
            scratch,
            scratch.drums_channel,
            1,
            16,
            on_scratch,
            set_drums_channel,
        ))
        .add(spin_row(
            "Solo button % volume reduction",
            scratch,
            scratch.solo_volume_reduction,
            0,
            100,
            on_scratch,
            set_solo_reduction,
        ))
        .add(toggle_row(
            "Start playback after loading",
            scratch.auto_play,
            scratch,
            on_scratch,
            set_auto_play,
        ))
        .add(toggle_row(
            "Advance to next playlist item",
            scratch.auto_advance,
            scratch,
            on_scratch,
            set_auto_advance,
        ))
        .add(toggle_row(
            "Automatically load and save song settings",
            scratch.auto_song_settings,
            scratch,
            on_scratch,
            set_auto_song_settings,
        ))
        .add(dropdown_row(
            "MIDI System Exclusive Reset",
            RESET_OPTIONS.as_slice(),
            reset_selected(scratch.sysex_reset),
            scratch,
            on_scratch,
            set_sysex_reset,
        ))
        .add(dropdown_row(
            "Instrument map (GM / GS SC-55 / MT-32)",
            MAP_OPTIONS.as_slice(),
            map_selected(scratch.instrument_map),
            scratch,
            on_scratch,
            set_instrument_map,
        ))
        .into()
}

/// Piano section: the C++ 5 rows in order (`window.cpp:1274-1306`). The
/// `singleColor` row is a plain text field (text stays source of truth —
/// garbage commits verbatim; the T13 canvas parses under `gdk_rgba_parse`
/// parity with gray fallback, render-only).
fn piano_section<'a>(
    scratch: &'a SettingsState,
    on_scratch: fn(SettingsState) -> Message,
) -> Element<'a, Message> {
    settings::section()
        .title("Piano")
        .add(dropdown_row(
            "Note highlighting",
            PALETTE_OPTIONS.as_slice(),
            palette_selected(scratch.highlight_palette),
            scratch,
            on_scratch,
            set_highlight_palette,
        ))
        .add(settings::item(
            "Single highlight color",
            text_input("", &scratch.single_color)
                .on_input(text_updater(scratch, on_scratch, set_single_color))
                .width(Length::Fixed(TEXT_FIELD_WIDTH)),
        ))
        .add(toggle_row(
            "Note velocity to color tint",
            scratch.velocity_color,
            scratch,
            on_scratch,
            set_velocity_color,
        ))
        .add(dropdown_row(
            "Show note names",
            NAMES_OPTIONS.as_slice(),
            names_selected(scratch.names_visibility),
            scratch,
            on_scratch,
            set_names_visibility,
        ))
        .add(toggle_row(
            "Octave subscript designation",
            scratch.octave_subscript,
            scratch,
            on_scratch,
            set_octave_subscript,
        ))
        .into()
}

/// D2 preferences dialog content. `vm` is the whole scratch contract (plus
/// the shell-owned `pickers`) — no other `App` access. Borrowed (`&'a`):
/// the text inputs borrow the scratch strings, so by-value is uncallable
/// here (unlike the page views — see [`PrefsViewModel`]).
pub fn view<'a>(
    vm: &'a PrefsViewModel,
    pickers: &'a LyricsPickerModels,
    on_scratch: fn(SettingsState) -> Message,
    on_picker: [fn(ColorPickerUpdate) -> Message; 3],
) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let PrefsViewModel { scratch } = vm;

    column::with_children(vec![
        general_section(scratch, on_scratch),
        settings::section()
            .title("Lyrics")
            .add(lyrics_prefs_rows(scratch, pickers, on_scratch, on_picker))
            .into(),
        piano_section(scratch, on_scratch),
    ])
    .spacing(16)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::lyrics::parse_gdk_rgba;
    use crate::ui::lyrics::LyricsPickerModels;

    /// Unwrap a whole-scratch test message (tests pass
    /// `Message::PrefsChanged` as the scratch constructor, T11b precedent —
    /// the shell's real constructor is an arch-supplied scratch-only
    /// variant; close is the only `PrefsChanged` in production).
    fn committed(message: Message) -> SettingsState {
        match message {
            Message::PrefsChanged(next) => next,
            other => panic!("expected PrefsChanged, got {other:?}"),
        }
    }

    fn test_picker_fn(update: ColorPickerUpdate) -> Message {
        Message::Toast(format!("{update:?}"))
    }

    /// Scratch with distinctive D3-owned + headless keys: every updater must
    /// carry them through byte-identically.
    fn distinctive_scratch() -> SettingsState {
        let mut scratch = SettingsState::new();
        scratch.last_output_backend = "ALSA".to_string();
        scratch.last_output_connection = "128:0".to_string();
        scratch.sound_font = "/sf/sc55.sf2".to_string();
        scratch.advanced_ports = true;
        scratch.notes_font = "Monospace 9".to_string();
        scratch.language = "de".to_string();
        scratch.recent_files = vec!["/m/a.mid".to_string()];
        scratch
    }

    fn assert_carried_through(next: &SettingsState, base: &SettingsState) {
        assert_eq!(next.last_output_backend, base.last_output_backend);
        assert_eq!(next.last_output_connection, base.last_output_connection);
        assert_eq!(next.sound_font, base.sound_font);
        assert_eq!(next.advanced_ports, base.advanced_ports);
        assert_eq!(next.notes_font, base.notes_font);
        assert_eq!(next.language, base.language);
        assert_eq!(next.recent_files, base.recent_files);
    }

    #[test]
    fn option_labels_are_verbatim_cpp() {
        assert_eq!(
            RESET_OPTIONS,
            [
                "None",
                "GM Reset",
                "GS Reset (SC-55)",
                "XG Reset",
                "MT-32 Reset"
            ]
        );
        assert_eq!(MAP_OPTIONS, ["GM", "GS (SC-55)", "MT-32"]);
        assert_eq!(
            PALETTE_OPTIONS,
            ["Single color", "Double", "Channels", "Scale"]
        );
        assert_eq!(
            NAMES_OPTIONS,
            ["Never", "Minimal", "When activated", "Always"]
        );
    }

    #[test]
    fn selections_clamp_to_nearest_valid() {
        assert_eq!(reset_selected(0), 0);
        assert_eq!(reset_selected(4), 4);
        assert_eq!(reset_selected(9), 4);
        assert_eq!(reset_selected(-1), 0);
        assert_eq!(map_selected(0), 0);
        assert_eq!(map_selected(2), 2);
        assert_eq!(map_selected(9), 2);
        assert_eq!(map_selected(-7), 0);
        assert_eq!(palette_selected(2), 2);
        assert_eq!(palette_selected(99), 3);
        assert_eq!(palette_selected(-1), 0);
        assert_eq!(names_selected(1), 1);
        assert_eq!(names_selected(99), 3);
        assert_eq!(names_selected(-1), 0);
    }

    #[test]
    fn spin_labels_show_the_clamped_value() {
        assert_eq!(spin_label(10, 1, 16), "10");
        assert_eq!(spin_label(99, 1, 16), "16");
        assert_eq!(spin_label(-5, 1, 16), "1");
        assert_eq!(spin_label(50, 0, 100), "50");
        assert_eq!(spin_label(101, 0, 100), "100");
        assert_eq!(spin_label(-1, 0, 100), "0");
    }

    #[test]
    fn every_general_row_sets_its_field_and_carries_the_rest() {
        // Seven blocks, one per General row (the section must hold 7 rows —
        // count them here, since the `Element` is not introspectable).
        let base = distinctive_scratch();
        let drums = committed(int_updater(&base, Message::PrefsChanged, set_drums_channel)(9));
        assert_eq!(drums.drums_channel, 9);
        assert_carried_through(&drums, &base);
        let solo = committed(int_updater(
            &base,
            Message::PrefsChanged,
            set_solo_reduction,
        )(75));
        assert_eq!(solo.solo_volume_reduction, 75);
        assert_carried_through(&solo, &base);
        let autoplay = committed(bool_updater(&base, Message::PrefsChanged, set_auto_play)(
            false,
        ));
        assert!(!autoplay.auto_play);
        assert_carried_through(&autoplay, &base);
        let advance =
            committed(bool_updater(&base, Message::PrefsChanged, set_auto_advance)(false));
        assert!(!advance.auto_advance);
        assert_carried_through(&advance, &base);
        let songs = committed(bool_updater(
            &base,
            Message::PrefsChanged,
            set_auto_song_settings,
        )(true));
        assert!(songs.auto_song_settings);
        assert_carried_through(&songs, &base);
        let reset = committed(index_updater(&base, Message::PrefsChanged, set_sysex_reset)(4));
        assert_eq!(reset.sysex_reset, 4);
        assert_carried_through(&reset, &base);
        let map = committed(index_updater(
            &base,
            Message::PrefsChanged,
            set_instrument_map,
        )(2));
        assert_eq!(map.instrument_map, 2);
        assert_carried_through(&map, &base);
    }

    #[test]
    fn every_piano_row_sets_its_field_and_carries_the_rest() {
        let base = distinctive_scratch();
        let palette = committed(index_updater(
            &base,
            Message::PrefsChanged,
            set_highlight_palette,
        )(0));
        assert_eq!(palette.highlight_palette, 0);
        assert_carried_through(&palette, &base);
        let single = committed(
            text_updater(&base, Message::PrefsChanged, set_single_color)("red".to_string()),
        );
        assert_eq!(single.single_color, "red");
        assert_carried_through(&single, &base);
        let velocity = committed(bool_updater(
            &base,
            Message::PrefsChanged,
            set_velocity_color,
        )(false));
        assert!(!velocity.velocity_color);
        assert_carried_through(&velocity, &base);
        let names = committed(index_updater(
            &base,
            Message::PrefsChanged,
            set_names_visibility,
        )(3));
        assert_eq!(names.names_visibility, 3);
        assert_carried_through(&names, &base);
        let octave = committed(bool_updater(
            &base,
            Message::PrefsChanged,
            set_octave_subscript,
        )(true));
        assert!(octave.octave_subscript);
        assert_carried_through(&octave, &base);
    }

    #[test]
    fn single_color_text_is_source_of_truth_with_gdk_rgba_parity() {
        let base = distinctive_scratch();
        // Garbage commits verbatim like C++ (no picker to normalize it).
        let next = committed(
            text_updater(&base, Message::PrefsChanged, set_single_color)("notacolor".to_string()),
        );
        assert_eq!(next.single_color, "notacolor");
        // …and the renderer parses it under gdk_rgba parity: `None` (gray
        // fallback, render-only) for garbage, `Some` for real colors.
        assert!(parse_gdk_rgba(&next.single_color).is_none());
        assert!(parse_gdk_rgba("#f6d32d").is_some());
        assert!(parse_gdk_rgba("red").is_some());
    }

    #[test]
    fn view_builds_across_contract_shapes() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes: default, corrupt (OOR ints, garbage
        // colors), and with a Lyrics picker open (inline dialog branch).
        let on_picker: [fn(ColorPickerUpdate) -> Message; 3] = [test_picker_fn; 3];
        let vm = PrefsViewModel {
            scratch: SettingsState::new(),
        };
        let models = LyricsPickerModels::from_settings(&vm.scratch);
        let _ = view(&vm, &models, Message::PrefsChanged, on_picker);
        let mut weird = SettingsState::new();
        weird.drums_channel = 99;
        weird.solo_volume_reduction = -5;
        weird.sysex_reset = 99;
        weird.instrument_map = -1;
        weird.highlight_palette = 99;
        weird.names_visibility = -9;
        weird.single_color = "notacolor".to_string();
        weird.future_color = String::new();
        weird.text_alignment = 99;
        let weird_vm = PrefsViewModel { scratch: weird };
        let weird_models = LyricsPickerModels::from_settings(&weird_vm.scratch);
        let _ = view(&weird_vm, &weird_models, Message::PrefsChanged, on_picker);
        let mut open_models = LyricsPickerModels::from_settings(&vm.scratch);
        let _ = open_models
            .future
            .update::<Message>(ColorPickerUpdate::ToggleColorPicker);
        assert!(open_models.future.get_is_active());
        let _ = view(&vm, &open_models, Message::PrefsChanged, on_picker);
    }
}
