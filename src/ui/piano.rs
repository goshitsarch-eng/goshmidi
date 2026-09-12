//! Piano page: per-channel keyboards on a `canvas` (ux §3.8).
//!
//! Real T13b implementation of the frozen [`PianoViewModel`] contract,
//! consumed as-is: owned `vm` in, one [`cosmic::Element`] out — a toolbar
//! (`Tighten keys` checkbox + `Show all` / `Hide unused` buttons) over a
//! `canvas` running [`PianoProgram`]. (Arch composes this page into the view
//! switcher next.)
//!
//! Widget → `Message` map (all variants exist in `message.rs`):
//!
//! | widget | message |
//! |---|
//! | `Tighten keys` checkbox | `PianoTighten(bool)` |
//! | `Show all` / `Hide unused` | `PianoShowAll` / `PianoHideUnused` |
//! | canvas left-press on a key | `PianoPreviewOn(ch, note)` (white-key quantized, drawn range) |
//! | canvas left-release | `PianoPreviewOff` (only after a canvas press) |
//!
//! Layout math is SINGLE-SOURCED in [`state::piano`][crate::state::piano]
//! (T13a): this module owns only the canvas `Program`, the draw loop, the
//! toolbar, and the VM builder — every geometric rule delegates:
//!
//! * [`drawn_range`][crate::state::piano::drawn_range] (full 21..=108 or
//!   tightened `lowest-2..=highest+2`; inversion preserved like C++ —
//!   draws no keys, clicks yield `None`),
//! * [`draw_rows`][crate::state::piano::draw_rows] (`[ch0]` with no song,
//!   else used-and-visible in channel order),
//! * [`row_height_px`][crate::state::piano::row_height_px] (`max(48, h/rows)`
//!   with truncating integer division, like C++),
//! * [`click_to_preview`][crate::state::piano::click_to_preview] (U12 FIX —
//!   white-key quantized WITHIN the drawn range),
//! * [`highlight_rgba`][crate::state::piano::highlight_rgba] (16-color
//!   palette / single mode / velocity tint, gray on garbage),
//! * [`note_label`][crate::state::piano::note_label] /
//!   [`show_note_name`][crate::state::piano::show_note_name] (names per
//!   `namesVisibility`; `octaveSubscript` renders the same text — C++
//!   no-op, carried, not dropped).
//!
//! T13 contract notes (view-owned behavior):
//!
//! * No-song draws the unlit ch0 keyboard (C++ parity — the keyboard IS the
//!   empty state, no placeholder); clicks stay dead without a song (C++
//!   `window.cpp:2060-2064`).
//! * Degenerate-input deviation (pinned in T13a, documented here): C++
//!   sends `NoteOn(lo)` for out-of-range x (the `note = lo` init survives,
//!   `window.cpp:2088-2097`); the port emits nothing.
//! * Canvas text positions are top-left (iced `Text`), while cairo draws
//!   from the baseline: note/foot labels shift up by their font size to
//!   land where C++ puts them.
//!
//! Pinned canvas API (rev `7e82198`, verified in framework source — my own
//! reads, per the T13 split; T13a checked it too):
//! `Program<Message, Theme, Renderer>` with `State: Default`, `update`
//! returning `Option<Action<Message>>` (`Action::publish` / `and_capture`),
//! `draw` returning `Vec<Geometry>` via `Frame::new(renderer, size)` +
//! `fill_rectangle` / `stroke_rectangle` / `fill_text`. Events are the core
//! `Event` (`Mouse(ButtonPressed/Released(Left))`); cursor via
//! `Cursor::position_in(bounds)`. The impl targets
//! `Program<Message, cosmic::Theme, cosmic::Renderer>` so the `Canvas`
//! converts into [`cosmic::Element`].
//!
//! A11y: the checkbox and both buttons carry names + descriptions (the
//! checkbox label doubles as its visible label); the canvas has no naming
//! API — like C++'s drawing area it exposes nothing, while the toolbar
//! stays keyboard-operable (ux §11).
//!
//! [`PianoViewModel`]: super::PianoViewModel

use cosmic::Element;
use cosmic::iced::event::Event;
use cosmic::iced::mouse::{self, Button, Cursor};
use cosmic::iced::widget::canvas::{Action, Frame, Geometry, Program, Stroke, Text};
use cosmic::iced::{Color, Length, Pixels, Point, Rectangle, Size};
use cosmic::widget::{button, canvas, checkbox, column, container, row, text};

use super::theme;
use super::{PianoRowViewModel, PianoViewModel};
use crate::core::event::MIDI_CHANNELS;
use crate::message::{LoadedSong, Message, SettingsState};
use crate::state::piano::{
    PIANO_HI, PIANO_LO, PianoState, click_to_preview, count_whites, draw_rows, drawn_range,
    highlight_rgba, note_is_black, note_label, row_height_px, show_note_name, white_width_px,
};

/// Unlit white key (`#f2f2ed`, `window.cpp:2007`).
const WHITE_KEY: Color = Color {
    r: 0.95,
    g: 0.95,
    b: 0.93,
    a: 1.0,
};
/// White-key stroke (`window.cpp:2010`).
const KEY_STROKE: Color = Color {
    r: 0.2,
    g: 0.2,
    b: 0.2,
    a: 1.0,
};
/// Unlit black key (`#1a1a1f`, `window.cpp:2038`).
const BLACK_KEY: Color = Color {
    r: 0.1,
    g: 0.1,
    b: 0.12,
    a: 1.0,
};
/// Note-name text (`window.cpp:2014`).
const NAME_TEXT: Color = Color {
    r: 0.1,
    g: 0.1,
    b: 0.1,
    a: 1.0,
};
/// Channel footer text (`window.cpp:2044`).
const FOOTER_TEXT: Color = Color {
    r: 0.9,
    g: 0.9,
    b: 0.9,
    a: 1.0,
};
/// Note-name font size (`window.cpp:2016`).
const NOTE_FONT_SIZE: f32 = 9.0;
/// Footer font size (C++ leaves the cairo size at whatever the last name
/// draw set — 9px after any name, else the 10px default; 11px splits the
/// visible difference at canvas scale).
const FOOTER_FONT_SIZE: f32 = 11.0;

// ---------------------------------------------------------------------------
// View-side thin wrappers (render conversions over the shared math)
// ---------------------------------------------------------------------------

/// Lit-key color for the canvas: [`highlight_rgba`] converted field-for-field
/// to iced (alpha forced — C++ paints with `cairo_set_source_rgb`).
fn channel_color(
    channel: u8,
    vel: u8,
    highlight_palette: i32,
    single_color: &str,
    velocity_color: bool,
) -> Color {
    let base = highlight_rgba(
        channel,
        vel,
        highlight_palette,
        single_color,
        velocity_color,
    );
    Color {
        r: base.r,
        g: base.g,
        b: base.b,
        a: 1.0,
    }
}

/// Click → `(channel, note)` audition target: dead with no song (C++
/// `window.cpp:2060-2064`) or no rows, else the shared [`click_to_preview`]
/// (U12 drawn-range FIX + white-key quantization) over the VM's rows.
fn click_note(vm: &PianoViewModel, width: f32, height: f32, x: f32, y: f32) -> Option<(u8, u8)> {
    if !vm.has_song || vm.rows.is_empty() {
        return None;
    }
    let channels: Vec<u8> = vm.rows.iter().map(|row| row.channel).collect();
    let row_h = row_height_px(height as i32, vm.rows.len());
    click_to_preview(x, y, width, row_h, vm.lo, vm.hi, &channels)
}

// ---------------------------------------------------------------------------
// View-model builder (pure over state)
// ---------------------------------------------------------------------------

/// Build the piano view model: rows ([`draw_rows`]), the drawn range
/// ([`drawn_range`]), toolbar state, and the D2 Piano settings rows
/// verbatim. Pure — no `App` access; composition calls it fresh per frame.
pub fn view_model(
    piano: &PianoState,
    song: Option<&LoadedSong>,
    settings: &SettingsState,
) -> PianoViewModel {
    let song_empty = song.is_none();
    let used = song.map_or([false; MIDI_CHANNELS], |song| song.channel_used);
    let rows: Vec<PianoRowViewModel> = draw_rows(song_empty, &used, &piano.visible)
        .iter()
        .map(|ch| PianoRowViewModel {
            channel: *ch,
            label: song.map_or_else(String::new, |song| {
                song.channel_labels[*ch as usize].clone()
            }),
            notes_on: piano.notes_on[*ch as usize],
            note_vel: piano.note_vel[*ch as usize],
        })
        .collect();
    let (lowest, highest) = song.map_or((i32::from(PIANO_LO), i32::from(PIANO_HI)), |song| {
        (song.lowest_note, song.highest_note)
    });
    let (lo, hi) = drawn_range(piano.tighten, song_empty, lowest, highest);
    PianoViewModel {
        rows,
        lo,
        hi,
        tighten: piano.tighten,
        highlight_palette: settings.highlight_palette,
        single_color: settings.single_color.clone(),
        velocity_color: settings.velocity_color,
        names_visibility: settings.names_visibility,
        octave_subscript: settings.octave_subscript,
        has_song: !song_empty,
    }
}

// ---------------------------------------------------------------------------
// Canvas program
// ---------------------------------------------------------------------------

/// Canvas press latch: release emits `PianoPreviewOff` only after a canvas
/// press lit a key (the runtime offers EVERY event to `update`, so an
/// unlatched release would clear preview notes on unrelated clicks).
#[derive(Default)]
pub struct PianoCanvasState {
    pressed: bool,
}

/// Canvas program holding the owned view model (the `Canvas` widget owns
/// the program by value, so borrowed state is uncallable here — same
/// rationale as the by-value VM).
pub struct PianoProgram {
    vm: PianoViewModel,
}

impl Program<Message, cosmic::Theme, cosmic::Renderer> for PianoProgram {
    type State = PianoCanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                let (channel, note) =
                    click_note(&self.vm, bounds.width, bounds.height, pos.x, pos.y)?;
                state.pressed = true;
                Some(Action::publish(Message::PianoPreviewOn(channel, note)).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) => {
                if state.pressed {
                    state.pressed = false;
                    Some(Action::publish(Message::PianoPreviewOff))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        draw_keyboards(&mut frame, &self.vm, bounds.width, bounds.height);
        vec![frame.into_geometry()]
    }
}

/// Cairo draw-path port (`window.cpp:1969-2051`): background, one keyboard
/// per row (white pass with names, black pass, footer). All geometry flows
/// through the shared helpers.
fn draw_keyboards(
    frame: &mut Frame<cosmic::Renderer>,
    vm: &PianoViewModel,
    width: f32,
    height: f32,
) {
    // Canvas background: the `.piano-area` fill, single-sourced in
    // `ui::theme` (T14b style mapping — explicit dark in both modes).
    frame.fill_rectangle(Point::ORIGIN, Size::new(width, height), theme::piano_area());
    let row_h = row_height_px(height as i32, vm.rows.len()) as f32;
    let ww = white_width_px(width, count_whites(vm.lo, vm.hi));
    if !ww.is_finite() || ww <= 0.0 {
        return;
    }
    let key_h = row_h - 18.0;
    for (row, keys) in vm.rows.iter().enumerate() {
        let y = row as f32 * row_h + 4.0;
        // White pass (`window.cpp:1998-2028`).
        let mut xw = 4.0;
        for note in vm.lo..=vm.hi {
            if note_is_black(note) {
                continue;
            }
            let lit = keys.notes_on[note as usize];
            let fill = if lit {
                channel_color(
                    keys.channel,
                    keys.note_vel[note as usize],
                    vm.highlight_palette,
                    &vm.single_color,
                    vm.velocity_color,
                )
            } else {
                WHITE_KEY
            };
            let top_left = Point::new(xw, y);
            let size = Size::new(ww - 1.0, key_h);
            frame.fill_rectangle(top_left, size, fill);
            frame.stroke_rectangle(
                top_left,
                size,
                Stroke {
                    style: KEY_STROKE.into(),
                    width: 1.0,
                    ..Stroke::default()
                },
            );
            if show_note_name(note, lit, vm.names_visibility) {
                frame.fill_text(Text {
                    content: note_label(note),
                    // Cairo draws from the baseline; canvas from the top —
                    // shift up by the font size (`window.cpp:2024`).
                    position: Point::new(xw + 2.0, y + row_h - 22.0 - NOTE_FONT_SIZE),
                    size: Pixels(NOTE_FONT_SIZE),
                    color: NAME_TEXT,
                    ..Text::default()
                });
            }
            xw += ww;
        }
        // Black pass (`window.cpp:2029-2043`).
        let mut xw = 4.0;
        for note in vm.lo..=vm.hi {
            if note_is_black(note) {
                let lit = keys.notes_on[note as usize];
                let fill = if lit {
                    channel_color(
                        keys.channel,
                        keys.note_vel[note as usize],
                        vm.highlight_palette,
                        &vm.single_color,
                        vm.velocity_color,
                    )
                } else {
                    BLACK_KEY
                };
                frame.fill_rectangle(
                    Point::new(xw - ww * 0.35, y),
                    Size::new(ww * 0.6, key_h * 0.62),
                    fill,
                );
            } else {
                xw += ww;
            }
        }
        // Footer (`window.cpp:2044-2048`).
        frame.fill_text(Text {
            content: format!("Ch {} {}", u16::from(keys.channel) + 1, keys.label),
            position: Point::new(8.0, y + row_h - 6.0 - FOOTER_FONT_SIZE),
            size: Pixels(FOOTER_FONT_SIZE),
            color: FOOTER_TEXT,
            ..Text::default()
        });
    }
}

// ---------------------------------------------------------------------------
// Page view
// ---------------------------------------------------------------------------

/// Dim page placeholder (reserved for the unreachable no-rows shape — with
/// no song the ch0 keyboard draws, so this only fires defensively).
fn page_placeholder(label: &'static str) -> Element<'static, Message> {
    container(text::body(label).class(theme::text_class(theme::dim_label)))
        .width(Length::Fill)
        .align_x(cosmic::iced::Alignment::Center)
        .padding(12)
        .into()
}

/// Piano page. `vm` is the whole contract — no other `App` access. By
/// value: owned data moves into the toolbar widgets and the canvas
/// `Program` (see [`PianoViewModel`]).
pub fn view<'a>(vm: PianoViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let PianoViewModel {
        rows,
        lo,
        hi,
        tighten,
        highlight_palette,
        single_color,
        velocity_color,
        names_visibility,
        octave_subscript,
        has_song,
    } = vm;

    // Toolbar order + labels are verbatim C++ (`window.cpp:1950-1956`);
    // spacing 8, vexpand canvas below.
    let toolbar = row::with_children(vec![
        checkbox(tighten)
            .label("Tighten keys")
            .on_toggle(Message::PianoTighten)
            .name("Tighten keys")
            .description("Narrow the keyboards to the song's note range")
            .into(),
        button::standard("Show all")
            .on_press(Message::PianoShowAll)
            .name("Show all")
            .description("Show every channel keyboard")
            .into(),
        button::standard("Hide unused")
            .on_press(Message::PianoHideUnused)
            .name("Hide unused")
            .description("Hide keyboards for unused channels")
            .into(),
    ])
    .spacing(8)
    .align_y(cosmic::iced::Alignment::Center);

    // The canvas has no `.name()` API — like C++'s drawing area it exposes
    // nothing; the toolbar above is the keyboard path (ux §11).
    let keyboards: Element<'_, Message> = if !has_song && rows.is_empty() {
        // Unreachable via the builder (no-song yields `[ch0]`); a corrupt
        // hand-built VM renders the placeholder instead of a blank frame.
        page_placeholder("No file loaded")
    } else {
        canvas(PianoProgram {
            vm: PianoViewModel {
                rows,
                lo,
                hi,
                tighten,
                highlight_palette,
                single_color,
                velocity_color,
                names_visibility,
                octave_subscript,
                has_song,
            },
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    // Page spacing 6 (`window.cpp:1949`).
    column::with_children(vec![toolbar.into(), keyboards])
        .spacing(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::player::SongSnapshot;

    fn test_row(channel: u8) -> PianoRowViewModel {
        PianoRowViewModel {
            channel,
            label: format!("Track{channel}"),
            notes_on: [false; 128],
            note_vel: [0; 128],
        }
    }

    fn test_vm() -> PianoViewModel {
        PianoViewModel {
            rows: vec![test_row(0), test_row(9)],
            lo: PIANO_LO,
            hi: PIANO_HI,
            tighten: false,
            highlight_palette: 2,
            single_color: "#f6d32d".to_string(),
            velocity_color: true,
            names_visibility: 1,
            octave_subscript: false,
            has_song: true,
        }
    }

    fn test_song() -> LoadedSong {
        LoadedSong::from(SongSnapshot {
            title: "song.mid".to_string(),
            format: "SMF".to_string(),
            bars: 32,
            len_ticks: 9600,
            duration_ms: 120_000,
            division: 96,
            num_tracks: 4,
            initial_tempo_us: 500_000.0,
            channel_used: std::array::from_fn(|ch| ch < 3),
            channel_labels: std::array::from_fn(|ch| format!("Ch{ch}")),
            lowest_note: 36,
            highest_note: 96,
            charset: "UTF-8".to_string(),
            texts: Vec::new(),
            metadata: Vec::new(),
            copyrights: Vec::new(),
        })
    }

    // NOTE: the shared geometry (black-key test, drawn range, white counts,
    // row height, click axes) is pinned in `state::piano` tests — these pin
    // the view boundary: the render conversion, the VM builder, the U12
    // click path end to end, and the view builds.

    #[test]
    fn channel_color_renders_palette_single_velocity() {
        // Palette entry 0 is `#e01b24`, unscaled without velocity.
        let plain = channel_color(0, 100, 2, "#00ff00", false);
        assert!((plain.r - 0xE0 as f32 / 255.0).abs() < 0.01);
        assert!((plain.g - 0x1B as f32 / 255.0).abs() < 0.01);
        assert_eq!(plain.a, 1.0);
        // `highlightPalette == 0` takes the single color instead.
        let single = channel_color(0, 100, 0, "#00ff00", false);
        assert_eq!((single.r, single.g, single.b), (0.0, 1.0, 0.0));
        // Velocity scales 0.4 + 0.6 * vel/127.
        let full = channel_color(0, 127, 0, "#ffffff", true);
        assert!((full.r - 1.0).abs() < 1e-6);
        let silent = channel_color(0, 0, 0, "#ffffff", true);
        assert!((silent.r - 0.4).abs() < 1e-6);
        // Garbage single text renders gray (never panics, never rewrites).
        let garbage = channel_color(0, 100, 0, "not a color", false);
        assert_eq!((garbage.r, garbage.g, garbage.b), (0.5, 0.5, 0.5));
        // Channel wraps at 16 (`ch & 15`).
        let wrapped = channel_color(16, 100, 2, "#00ff00", false);
        assert_eq!(
            (wrapped.r, wrapped.g, wrapped.b),
            (plain.r, plain.g, plain.b)
        );
    }

    #[test]
    fn note_labels_use_scientific_octaves() {
        assert_eq!(note_label(60), "C4");
        assert_eq!(note_label(61), "C#4");
        assert_eq!(note_label(21), "A0");
        assert_eq!(note_label(108), "C8");
        assert_eq!(note_label(0), "C-1");
    }

    #[test]
    fn name_visibility_modes() {
        // Never.
        assert!(!show_note_name(60, true, 0));
        // Minimal: C only.
        assert!(show_note_name(60, false, 1));
        assert!(!show_note_name(61, true, 1));
        // When-active: lit only.
        assert!(show_note_name(61, true, 2));
        assert!(!show_note_name(61, false, 2));
        // Always.
        assert!(show_note_name(61, false, 3));
    }

    #[test]
    fn click_maps_white_keys_in_drawn_range() {
        let vm = test_vm();
        // Full range, 2 rows: y in row 1 → ch 9; first white key → 21 (A0).
        let width = 8.0 + 52.0 * 10.0;
        assert_eq!(click_note(&vm, width, 200.0, 9.0, 150.0), Some((9, 21)));
        // Same x in row 0 → ch 0.
        assert_eq!(click_note(&vm, width, 200.0, 9.0, 10.0), Some((0, 21)));
        // Second white slot → 23 (22 is black, skipped — quantization).
        assert_eq!(click_note(&vm, width, 200.0, 19.0, 10.0), Some((0, 23)));
        // U12: tightened range maps within the drawn range, not 21..108
        // (lo 34 is black, so slot 0 quantizes to 35 — edge quantization).
        let tight = PianoViewModel {
            lo: 34,
            hi: 98,
            ..test_vm()
        };
        assert_eq!(
            click_note(&tight, 8.0 + 38.0 * 10.0, 200.0, 9.0, 10.0),
            Some((0, 35))
        );
    }

    #[test]
    fn click_rejects_degenerate_input() {
        let vm = PianoViewModel {
            rows: vec![test_row(0)],
            ..test_vm()
        };
        let width = 8.0 + 52.0 * 10.0;
        // No song: dead (C++ `used == 0` early return).
        let no_song = PianoViewModel {
            has_song: false,
            ..test_vm()
        };
        assert_eq!(click_note(&no_song, width, 200.0, 9.0, 10.0), None);
        // No rows: dead.
        let no_rows = PianoViewModel {
            rows: Vec::new(),
            ..test_vm()
        };
        assert_eq!(click_note(&no_rows, width, 200.0, 9.0, 10.0), None);
        // Left of the keys / right of the keys: nothing (C++ sends `lo`;
        // the port deliberately emits no message — see module docs).
        assert_eq!(click_note(&vm, width, 200.0, -50.0, 10.0), None);
        assert_eq!(click_note(&vm, width, 200.0, width + 50.0, 10.0), None);
        // Just above the canvas still hits row 0 (C++ truncates toward
        // zero); a full row-height above matches nothing.
        assert_eq!(click_note(&vm, width, 200.0, 9.0, -10.0), Some((0, 21)));
        assert_eq!(click_note(&vm, width, 200.0, 9.0, -1_000.0), None);
        // Below the rows clamps to the last row (C++ `min(used - 1, …)`).
        assert_eq!(click_note(&vm, width, 200.0, 9.0, 10_000.0), Some((0, 21)));
        // Zero-width canvas: nothing (no division trap).
        assert_eq!(click_note(&vm, 0.0, 200.0, 9.0, 10.0), None);
        // Inverted range (corrupt song): nothing.
        let inverted = PianoViewModel {
            lo: 94,
            hi: 38,
            ..test_vm()
        };
        assert_eq!(click_note(&inverted, width, 200.0, 9.0, 10.0), None);
    }

    #[test]
    fn builder_selects_rows_and_range() {
        let mut piano = PianoState::new();
        piano.tighten = true;
        piano.note_on(1, 60, 100);
        let song = test_song();
        let settings = SettingsState::new();
        let vm = view_model(&piano, Some(&song), &settings);
        assert!(vm.has_song);
        assert!(vm.tighten);
        assert_eq!(vm.lo, 34);
        assert_eq!(vm.hi, 98);
        let channels: Vec<u8> = vm.rows.iter().map(|row| row.channel).collect();
        assert_eq!(channels, vec![0, 1, 2]);
        assert!(vm.rows[1].notes_on[60]);
        assert_eq!(vm.rows[1].note_vel[60], 100);
        assert_eq!(vm.rows[0].label, "Ch0");
        assert_eq!(vm.highlight_palette, settings.highlight_palette);
        assert_eq!(vm.single_color, settings.single_color);
        // Hide-unused hides ch 2 from the rows.
        piano.visible[2] = false;
        let vm = view_model(&piano, Some(&song), &settings);
        let channels: Vec<u8> = vm.rows.iter().map(|row| row.channel).collect();
        assert_eq!(channels, vec![0, 1]);
    }

    #[test]
    fn builder_preserves_inverted_range_like_cpp() {
        // Events but no notes: lowest 127 / highest 0 (the C++ inits) —
        // the inversion survives (draws no keys, clicks None), like C++.
        let mut piano = PianoState::new();
        piano.tighten = true;
        let mut song = test_song();
        song.lowest_note = 127;
        song.highest_note = 0;
        let vm = view_model(&piano, Some(&song), &SettingsState::new());
        assert_eq!((vm.lo, vm.hi), (125, 2));
        assert_eq!(click_note(&vm, 528.0, 200.0, 9.0, 10.0), None);
    }

    #[test]
    fn builder_empty_song_draws_ch0_dead_clicks() {
        let piano = PianoState::new();
        let settings = SettingsState::new();
        let vm = view_model(&piano, None, &settings);
        assert!(!vm.has_song);
        assert_eq!(vm.rows.len(), 1);
        assert_eq!(vm.rows[0].channel, 0);
        assert_eq!((vm.lo, vm.hi), (PIANO_LO, PIANO_HI));
        // …and clicks stay dead on that drawn row.
        assert_eq!(click_note(&vm, 528.0, 200.0, 9.0, 10.0), None);
    }

    #[test]
    fn view_builds_across_contract_shapes() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes: rows, no-song ch0, song-without-rows
        // (blank canvas), tighten, inverted range, corrupt VM values.
        let _ = view(test_vm());
        let settings = SettingsState::new();
        let _ = view(view_model(&PianoState::new(), None, &settings));
        let mut piano = PianoState::new();
        piano.hide_unused(&[false; 16]);
        let _ = view(view_model(&piano, Some(&test_song()), &settings));
        let mut tight = test_vm();
        tight.tighten = true;
        tight.lo = 34;
        tight.hi = 98;
        tight.names_visibility = 3;
        tight.octave_subscript = true;
        let _ = view(tight);
        let mut inverted = test_vm();
        inverted.lo = 94;
        inverted.hi = 38;
        let _ = view(inverted);
        let mut corrupt = test_vm();
        corrupt.highlight_palette = 99;
        corrupt.single_color = "garbage".to_string();
        corrupt.names_visibility = -1;
        corrupt.rows[0].channel = 255;
        let _ = view(corrupt);
        // Hand-built no-song without rows: the defensive placeholder.
        let _ = view(PianoViewModel {
            rows: Vec::new(),
            has_song: false,
            ..test_vm()
        });
    }
}
