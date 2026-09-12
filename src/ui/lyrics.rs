//! Lyrics page + D2 Lyrics prefs rows (ux §3.6, D2/D12/D13).
//!
//! Real T11b implementation of the frozen [`LyricsViewModel`] contract,
//! consumed as-is: owned `vm` in, one [`cosmic::Element`] out — a column of
//! the filter toolbar (3 dropdowns + Copy/Save/Print) and the scrollable
//! lyrics body. (Arch composes this page into the view switcher next.)
//!
//! Widget → `Message` map (all variants exist in `message.rs`):
//!
//! | widget | message |
//! |---|
//! | Track dropdown (`All tracks` + `Track N …`) | `LyricTrackFilter(None if 0 else Some(i))` |
//! | Type dropdown (14 `kTypeNames`) | `LyricTypeFilter(i)` |
//! | Encoding dropdown (14 codec names) | `LyricCodec(options[i])` |
//! | Copy / Save / Print | `LyricsCopy` / `LyricsSaveDialog` / `LyricsPrint` |
//!
//! T11a contract notes (implemented exactly):
//!
//! * Preselects are honored from state: the view renders the `*_selected`
//!   indices verbatim (re-clamped defensively only) — filters change solely
//!   via filter messages + song-load preselect (C++ snap-back FIX).
//! * Body = three `text::Span` runs, empties skipped: `body` in
//!   `sung ? past : future`, `live_past` in past, `live_current` in
//!   highlight. Colors map [`Rgba`] → [`iced::Color`] field-for-field;
//!   font = first family + px size + weight + style + stretch (`small_caps`
//!   ignored — iced `Font` has no variant); alignment → [`Horizontal`].
//! * U10 duplication (prefill + live append) is preserved state-side; the
//!   view concatenates the three segments in order, like the C++ buffer.
//! * Autoscroll is split across the view/update boundary: the scrollable
//!   carries the stable [`LYRICS_BODY_SCROLL_ID`], and arch's live-text arm
//!   returns [`snap_to_end_task`] when `append_live` accepts (gated on
//!   `autoscroll`, always true — there is no C++ toggle surface).
//!
//! Prefs rows ([`lyrics_prefs_rows`], D2 Lyrics page): the C++ 5 rows —
//! Future/Past/Highlight color entries, Lyrics font, Text alignment — as a
//! reusable view fn the T12/13 dialog shell embeds. Each color row is a D17
//! `color_picker` + text binding (text stays source of truth):
//! [`seed_color_picker`] (`ColorPickerModel::new`, fallback+initial from
//! `parse_gdk_rgba`), apply events → [`rgba_to_hex_string`], text edits
//! re-parse and drive the model only on `Some` (`None` keeps last-good,
//! gray is render-only). The row builders are key-agnostic, so T12 reuses
//! them for the 4th D2 color (`singleColor`) and the 2nd U6 font
//! (`notesFont`) — but the Lyrics page itself renders exactly the C++ 5
//! rows (a `singleColor`/`notesFont` row here would break the 17-row
//! checklist and D13's headless-`notesFont` ruling).
//!
//! Empty states: C++ shows a blank text view; the port renders dim
//! placeholders (`No file loaded` with no song, `No lyrics match the
//! current filters` when the filters exclude everything) — a deliberate
//! discoverability addition in the sidebar-placeholder spirit.
//!
//! A11y: icon buttons carry tooltip + name + description (tooltips are the
//! exact C++ strings); dropdowns and text rows have no `.name()` API in the
//! pinned rev, so the visible labels (toolbar labels, settings titles)
//! are the accessible names per ux §11.
//!
//! [`LyricsViewModel`]: super::LyricsViewModel
//! [`Rgba`]: crate::state::lyrics::Rgba
//! [`iced::Color`]: cosmic::iced::Color
//! [`Horizontal`]: cosmic::iced::alignment::Horizontal
//! [`rgba_to_hex_string`]: crate::state::lyrics::rgba_to_hex_string

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced::alignment::Horizontal;
use cosmic::iced::core::text::Wrapping;
use cosmic::iced::font::{Family, Stretch as IcedStretch, Style as IcedStyle, Weight};
use cosmic::iced::widget::text::Span;
use cosmic::iced::widget::{rich_text, span};
use cosmic::widget::color_picker::{ColorPickerModel, ColorPickerUpdate};
use cosmic::widget::settings::{self, view_column};
use cosmic::widget::{
    button, column, container, dropdown, icon, row, scrollable, text, text_input,
};

use super::LyricsViewModel;
use super::theme;
use crate::message::{Message, SettingsState};
use crate::state::lyrics::{
    DEFAULT_LYRICS_SIZE_PT, FontStretch, FontStyle, LyricAlign, ParsedFont, Rgba, font_size_px,
    parse_alignment, parse_gdk_rgba, rgba_to_hex_string,
};

/// Stable id of the body scrollable: arch's [`snap_to_end_task`] targets it.
pub const LYRICS_BODY_SCROLL_ID: &str = "lyrics-body";

/// Text-alignment dropdown labels (`window.cpp:1257-1259`, verbatim).
pub const ALIGNMENT_OPTIONS: [&str; 3] = ["Left", "Center", "Right"];

/// D17 picker segment labels (libcosmic demo parity).
const PICKER_HEX_LABEL: &str = "Hex";
/// D17 picker segment labels (libcosmic demo parity).
const PICKER_RGB_LABEL: &str = "RGB";

/// Prefs text-field width: titles (~130px) + field + spacing fit a ~480px
/// dialog like D3's without squeezing the titles (text inputs default to
/// `Fill`, which would starve the title in a settings row).
const TEXT_FIELD_WIDTH: f32 = 240.0;
/// Picker swatch button edge: a 28px square next to the text field (the
/// stock `color_button` is `space_s`-sized — too small for a settings row).
const PICKER_SWATCH: f32 = 28.0;

// ---------------------------------------------------------------------------
// Lyrics page
// ---------------------------------------------------------------------------

/// Clamp a `*_selected` index against its options: out-of-range → nearest
/// valid, empty options → no selection (the dropdown then shows its empty
/// state instead of indexing out of bounds).
fn clamp_selected(selected: usize, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(selected.min(len - 1))
    }
}

/// One lyrics toolbar icon button: freedesktop symbolic icon + tooltip +
/// a11y name/description. Tooltips are the exact C++ strings
/// (`window.cpp:1772-1774`); names match them (transport/sidebar precedent).
fn lyric_tool_button(
    icon_name: &'static str,
    name: &'static str,
    description: &'static str,
    on_press: Message,
) -> Element<'static, Message> {
    button::icon(icon::from_name(icon_name))
        .on_press(on_press)
        .tooltip(name)
        .name(name)
        .description(description)
        .into()
}

/// Dim body placeholder (empty states — deliberate addition, see module docs).
fn body_placeholder(label: &'static str) -> Element<'static, Message> {
    container(text::body(label).class(theme::text_class(theme::dim_label)))
        .width(Length::Fill)
        .align_x(cosmic::iced::Alignment::Center)
        .padding(12)
        .into()
}

/// The three body runs, empties skipped: `body` in `sung ? past : future`,
/// `live_past` in past, `live_current` in highlight. Owned `String`s move
/// into the spans, so the `Vec` (and the `Rich` built from it) is `'static`
/// and the `Element` keeps the caller's lifetime.
fn body_spans(
    body: &str,
    live_past: &str,
    live_current: &str,
    sung: bool,
    applied: &crate::state::lyrics::AppliedLyricsPrefs,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    if !body.is_empty() {
        let color = if sung { applied.past } else { applied.future };
        spans.push(span(body.to_owned()).color(rgba_to_iced(color)));
    }
    if !live_past.is_empty() {
        spans.push(span(live_past.to_owned()).color(rgba_to_iced(applied.past)));
    }
    if !live_current.is_empty() {
        spans.push(span(live_current.to_owned()).color(rgba_to_iced(applied.highlight)));
    }
    spans
}

/// Lyrics page. `vm` is the whole contract — no other `App` access. By
/// value: owned data moves into the widgets (see [`LyricsViewModel`]).
pub fn view<'a>(vm: LyricsViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let LyricsViewModel {
        track_options,
        track_selected,
        type_options,
        type_selected,
        codec_options,
        codec_selected,
        body,
        live_past,
        live_current,
        sung,
        // `live_seq` is the arch-side snap trigger (see `snap_to_end_task`);
        // the view cannot issue tasks, so it carries the field unrendered.
        live_seq: _live_seq,
        // `autoscroll` gates the arch-side snap call; there is no C++ toggle
        // surface, so the view renders nothing for it.
        autoscroll: _autoscroll,
        applied,
        has_song,
    } = vm;

    // Option lengths before the labels move into the dropdowns.
    let track_len = track_options.len();
    let type_len = type_options.len();
    let codec_len = codec_options.len();

    // Track: index IS the track number (`track_options` contract); 0 = all.
    let track_dropdown = dropdown(
        track_options,
        clamp_selected(track_selected, track_len),
        |index| Message::LyricTrackFilter(if index == 0 { None } else { Some(index as u32) }),
    );
    // Type: index IS the `TextType` discriminant (`TYPE_OPTION_NAMES`
    // contract); 0 = all.
    let type_dropdown = dropdown(
        type_options,
        clamp_selected(type_selected, type_len),
        |index| Message::LyricTypeFilter(index as i32),
    );
    // Encoding: the selected NAME travels (C++ reads the model string,
    // `window.cpp:1801-1805`); a stale index degrades to `""`, which the
    // arch arm ignores — never a panic.
    let codec_labels = codec_options.clone();
    let codec_dropdown = dropdown(
        codec_options,
        clamp_selected(codec_selected, codec_len),
        move |index| Message::LyricCodec(codec_labels.get(index).cloned().unwrap_or_default()),
    );

    // Toolbar order + labels are verbatim C++ (`window.cpp:1763-1776`);
    // margins 8/8, spacing 8.
    let toolbar = container(
        row::with_children(vec![
            text::body("Track").into(),
            track_dropdown.into(),
            text::body("Type").into(),
            type_dropdown.into(),
            text::body("Encoding").into(),
            codec_dropdown.into(),
            lyric_tool_button(
                "edit-copy-symbolic",
                "Copy lyrics",
                "Copy all lyrics text to the clipboard",
                Message::LyricsCopy,
            ),
            lyric_tool_button(
                "document-save-symbolic",
                "Save lyrics",
                "Save all lyrics text to a file",
                Message::LyricsSaveDialog,
            ),
            lyric_tool_button(
                "document-print-symbolic",
                "Print lyrics",
                "Print all lyrics text",
                Message::LyricsPrint,
            ),
        ])
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center),
    )
    .padding([0, 8])
    .width(Length::Fill);

    let body_el: Element<'_, Message> = if !has_song {
        body_placeholder("No file loaded")
    } else {
        let spans = body_spans(&body, &live_past, &live_current, sung, &applied);
        if spans.is_empty() {
            body_placeholder("No lyrics match the current filters")
        } else {
            let font = lyrics_font(&applied.font);
            let size_px = font_size_px(applied.font.size, DEFAULT_LYRICS_SIZE_PT);
            // `WordOrGlyph` ≈ `GTK_WRAP_WORD_CHAR` (prefer words, break long
            // ones); full width so Center/Right justification can show.
            scrollable(
                rich_text(spans)
                    .font(font)
                    .size(size_px)
                    .align_x(align_to_horizontal(applied.alignment))
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Length::Fill),
            )
            .id(LYRICS_BODY_SCROLL_ID)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        }
    };

    // Page spacing 6, scrolled body vexpands (`window.cpp:1756-1784`).
    column::with_children(vec![toolbar.into(), body_el])
        .spacing(6)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// T11a → iced mappings (pure, unit-pinned)
// ---------------------------------------------------------------------------

/// [`Rgba`] → [`iced::Color`], field-for-field.
///
/// [`iced::Color`]: cosmic::iced::Color
fn rgba_to_iced(color: Rgba) -> cosmic::iced::Color {
    cosmic::iced::Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

/// [`iced::Color`] → [`Rgba`], field-for-field (picker-applied path).
///
/// [`iced::Color`]: cosmic::iced::Color
fn iced_to_rgba(color: cosmic::iced::Color) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

/// Parsed Pango weight (100–1000) → iced [`Weight`] by CSS bucket. The T11a
/// parser only emits 100/200/300/350/380/400/500/600/700/800/900/1000, so
/// 350 (Semi-Light) lands on Light and 380 (Book) on Normal.
fn weight_from_u16(weight: u16) -> Weight {
    match weight {
        0..=150 => Weight::Thin,
        151..=250 => Weight::ExtraLight,
        251..=350 => Weight::Light,
        351..=450 => Weight::Normal,
        451..=550 => Weight::Medium,
        551..=650 => Weight::Semibold,
        651..=750 => Weight::Bold,
        751..=850 => Weight::ExtraBold,
        _ => Weight::Black,
    }
}

/// Parsed Pango stretch → iced stretch (`None` = normal/unspecified).
fn stretch_from_parsed(stretch: Option<FontStretch>) -> IcedStretch {
    match stretch {
        None => IcedStretch::Normal,
        Some(FontStretch::UltraCondensed) => IcedStretch::UltraCondensed,
        Some(FontStretch::ExtraCondensed) => IcedStretch::ExtraCondensed,
        Some(FontStretch::Condensed) => IcedStretch::Condensed,
        Some(FontStretch::SemiCondensed) => IcedStretch::SemiCondensed,
        Some(FontStretch::SemiExpanded) => IcedStretch::SemiExpanded,
        Some(FontStretch::Expanded) => IcedStretch::Expanded,
        Some(FontStretch::ExtraExpanded) => IcedStretch::ExtraExpanded,
        Some(FontStretch::UltraExpanded) => IcedStretch::UltraExpanded,
    }
}

/// Parsed Pango style → iced style.
fn style_from_parsed(style: FontStyle) -> IcedStyle {
    match style {
        FontStyle::Normal => IcedStyle::Normal,
        FontStyle::Italic => IcedStyle::Italic,
        FontStyle::Oblique => IcedStyle::Oblique,
    }
}

/// Parsed `textAlignment` → [`Horizontal`] (then into the text alignment).
fn align_to_horizontal(alignment: LyricAlign) -> Horizontal {
    match alignment {
        LyricAlign::Left => Horizontal::Left,
        LyricAlign::Center => Horizontal::Center,
        LyricAlign::Right => Horizontal::Right,
    }
}

/// Process-lifetime intern table for Pango family names: iced's
/// [`Family::Name`] holds a `&'static str`, so each distinct runtime name is
/// leaked exactly once here. Bounded by user input (prefs font edits) —
/// a handful of tiny strings, never per-frame.
fn family_cache() -> &'static Mutex<HashMap<String, &'static str>> {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Intern a family name for [`Family::Name`] (leaked once per distinct name).
fn intern_family(name: &str) -> &'static str {
    let mut cache = family_cache().lock().expect("family cache lock");
    if let Some(interned) = cache.get(name) {
        return interned;
    }
    let interned: &'static str = Box::leak(name.to_owned().into_boxed_str());
    cache.insert(name.to_owned(), interned);
    interned
}

/// Parsed `lyricsFont` → iced [`cosmic::font::Font`]: first comma-separated
/// family wins (arch §2.6), weight/style/stretch mapped; `small_caps` is
/// parsed but has no iced mapping, so it renders as normal text (T11a).
fn lyrics_font(font: &ParsedFont) -> cosmic::font::Font {
    cosmic::font::Font {
        family: Family::Name(intern_family(font.first_family())),
        weight: weight_from_u16(font.weight),
        stretch: stretch_from_parsed(font.stretch),
        style: style_from_parsed(font.style),
    }
}

/// Snap-to-end task for the lyrics body: arch's live-text arm returns this
/// when `append_live` accepts (gated on `autoscroll`), reproducing C++
/// `scroll_mark_onscreen` (`window.cpp:177`). Vertical end, horizontal
/// untouched; never emits a message.
pub fn snap_to_end_task() -> cosmic::app::Task<Message> {
    use cosmic::iced::widget::scrollable::{RelativeOffset, snap_to};
    use cosmic::{Action, iced};

    let id = iced::core::widget::Id::from(LYRICS_BODY_SCROLL_ID);
    snap_to(
        id,
        RelativeOffset {
            x: None,
            y: Some(1.0),
        },
    )
    .map(Action::App)
}

// ---------------------------------------------------------------------------
// D2 Lyrics prefs rows: state hooks (shell-owned, T12/13 embeds the view)
// ---------------------------------------------------------------------------

/// The three D2 Lyrics-page color rows, in C++ order (`window.cpp:1245-1252`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsColorRow {
    Future,
    Past,
    Highlight,
}

/// Shell-owned D17 picker models for the three Lyrics color rows: seeded
/// from the scratch settings when the prefs dialog opens
/// ([`LyricsPickerModels::from_settings`]), fed picker-dialog updates via
/// [`apply_picker_update`], re-driven from text edits via
/// [`apply_color_text`]/[`adopt_prefs_scratch`].
pub struct LyricsPickerModels {
    /// `futureColor` picker.
    pub future: ColorPickerModel,
    /// `pastColor` picker.
    pub past: ColorPickerModel,
    /// `highlightColor` picker.
    pub highlight: ColorPickerModel,
}

impl LyricsPickerModels {
    /// Seed all three models from the scratch settings (dialog open).
    pub fn from_settings(settings: &SettingsState) -> Self {
        Self {
            future: seed_color_picker(&settings.future_color),
            past: seed_color_picker(&settings.past_color),
            highlight: seed_color_picker(&settings.highlight_color),
        }
    }

    /// Mutable model for `row`.
    fn model_mut(&mut self, row: LyricsColorRow) -> &mut ColorPickerModel {
        match row {
            LyricsColorRow::Future => &mut self.future,
            LyricsColorRow::Past => &mut self.past,
            LyricsColorRow::Highlight => &mut self.highlight,
        }
    }
}

/// Mutable scratch text for `row`.
fn color_text_mut(scratch: &mut SettingsState, row: LyricsColorRow) -> &mut String {
    match row {
        LyricsColorRow::Future => &mut scratch.future_color,
        LyricsColorRow::Past => &mut scratch.past_color,
        LyricsColorRow::Highlight => &mut scratch.highlight_color,
    }
}

/// Seed one D17 picker from a settings text row: `fallback` + `initial`
/// are both `parse_gdk_rgba(text)` (both `None` when the text is invalid —
/// the button then shows the placeholder swatch until the first valid edit
/// or picker apply; gray stays render-only, never seeded into the model).
pub fn seed_color_picker(text: &str) -> ColorPickerModel {
    let parsed = parse_gdk_rgba(text).map(rgba_to_iced);
    ColorPickerModel::new(PICKER_HEX_LABEL, PICKER_RGB_LABEL, parsed, parsed)
}

/// Drive a picker from an edited text row: re-parse; `Some` re-seeds the
/// model (preserving the open state — the picker dialog, if open, retargets
/// live), `None` keeps last-good (the model is untouched; the lyrics body
/// renders gray via the applied prefs, render-only).
pub fn drive_picker_from_text(model: &mut ColorPickerModel, text: &str) {
    if let Some(parsed) = parse_gdk_rgba(text).map(rgba_to_iced) {
        let active = model.get_is_active();
        *model = ColorPickerModel::new(
            PICKER_HEX_LABEL,
            PICKER_RGB_LABEL,
            Some(parsed),
            Some(parsed),
        )
        .active(active);
    }
}

/// Text-edit hook for one color row: the scratch text ALWAYS wins (text is
/// the D17 source of truth — garbage commits verbatim like C++, gray is
/// render-only); the model follows only when the text parses.
pub fn apply_color_text(
    models: &mut LyricsPickerModels,
    scratch: &mut SettingsState,
    row: LyricsColorRow,
    text: String,
) {
    drive_picker_from_text(models.model_mut(row), &text);
    *color_text_mut(scratch, row) = text;
}

/// Scratch-arm hook for the shell's whole-settings edit message: adopt the
/// new scratch, re-driving each color-row model whose text changed (font +
/// alignment rows are plain scratch fields — no models to drive).
pub fn adopt_prefs_scratch(
    models: &mut LyricsPickerModels,
    scratch: &mut SettingsState,
    next: SettingsState,
) {
    for row in [
        LyricsColorRow::Future,
        LyricsColorRow::Past,
        LyricsColorRow::Highlight,
    ] {
        let changed = match row {
            LyricsColorRow::Future => next.future_color != scratch.future_color,
            LyricsColorRow::Past => next.past_color != scratch.past_color,
            LyricsColorRow::Highlight => next.highlight_color != scratch.highlight_color,
        };
        if changed {
            let text = match row {
                LyricsColorRow::Future => next.future_color.clone(),
                LyricsColorRow::Past => next.past_color.clone(),
                LyricsColorRow::Highlight => next.highlight_color.clone(),
            };
            drive_picker_from_text(models.model_mut(row), &text);
        }
    }
    *scratch = next;
}

/// Whether a picker-dialog update is an apply event: `AppliedColor` (Save
/// button) and `ActionFinished` (slider/canvas release, picker-input
/// submit) both commit `active` → `applied` in the model, and `Reset`
/// restores `fallback` → `applied` — all three must write the text row or
/// the button and the text diverge. Live drags (`ActiveColor`), the
/// picker's internal input, toggles, and cancel leave the text alone.
fn is_apply_update(update: &ColorPickerUpdate) -> bool {
    matches!(
        update,
        ColorPickerUpdate::AppliedColor
            | ColorPickerUpdate::ActionFinished
            | ColorPickerUpdate::Reset
    )
}

/// Picker-arm hook for one color row: feed the dialog update into the
/// model; on apply events write the canonical [`rgba_to_hex_string`] text
/// (when the model holds an applied color). Returns the model's task
/// mapped into the app task (the picker's copy-to-clipboard arm needs it).
pub fn apply_picker_update(
    models: &mut LyricsPickerModels,
    scratch: &mut SettingsState,
    row: LyricsColorRow,
    update: ColorPickerUpdate,
) -> cosmic::app::Task<Message> {
    let apply = is_apply_update(&update);
    let task = models.model_mut(row).update(update);
    if apply && let Some(applied) = models.model_mut(row).get_applied_color() {
        *color_text_mut(scratch, row) = rgba_to_hex_string(iced_to_rgba(applied));
    }
    task.map(cosmic::Action::App)
}

/// Dropdown selection for a `textAlignment` scratch value: through
/// `parse_alignment`, so the dropdown always shows the EFFECTIVE alignment
/// (out-of-range/negative values render Left, and select Left too).
pub fn alignment_selected(value: i32) -> usize {
    match parse_alignment(value) {
        LyricAlign::Left => 0,
        LyricAlign::Center => 1,
        LyricAlign::Right => 2,
    }
}

// ---------------------------------------------------------------------------
// D2 Lyrics prefs rows: view (T12/13 shell embeds this in the dialog)
// ---------------------------------------------------------------------------

/// Scratch setters for the text rows, as `fn` pointers so the row builders
/// (and the `'static` dropdown closure) stay key-agnostic: T12 reuses
/// [`font_row`] with a `notes_font` setter and the color-row shape with a
/// `single_color` setter without touching this file.
fn set_future_color(scratch: &mut SettingsState, value: String) {
    scratch.future_color = value;
}
/// Scratch setter for the Past text color row (see [`set_future_color`]).
fn set_past_color(scratch: &mut SettingsState, value: String) {
    scratch.past_color = value;
}
/// Scratch setter for the Highlight color row (see [`set_future_color`]).
fn set_highlight_color(scratch: &mut SettingsState, value: String) {
    scratch.highlight_color = value;
}
/// Scratch setter for the Lyrics font row (see [`set_future_color`]).
fn set_lyrics_font(scratch: &mut SettingsState, value: String) {
    scratch.lyrics_font = value;
}

/// Whole-scratch updater for one text row: clones the dialog scratch, sets
/// the row's field (unconditionally — text is the source of truth), and
/// emits `on_scratch`. Captures only owned data, so the closure is
/// `'static` (dropdown-compatible) as well as `'a` (text-input-compatible).
/// Shared with the T12 prefs shell (the Piano `singleColor` row).
pub(crate) fn text_updater(
    scratch: &SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, String),
) -> impl Fn(String) -> Message + 'static {
    let base = scratch.clone();
    move |value| {
        let mut next = base.clone();
        set(&mut next, value);
        on_scratch(next)
    }
}

/// One D17 color row: settings item with a swatch button + text field, and
/// the inline picker dialog while the model is active. The picker button
/// reuses the row title for tooltip + a11y name (the text field has no
/// `.name()` API — the settings title is its visible label per ux §11).
fn color_row<'a>(
    title: &'static str,
    current: &'a str,
    model: &'a ColorPickerModel,
    on_text: impl Fn(String) -> Message + 'a,
    on_picker: fn(ColorPickerUpdate) -> Message,
) -> Element<'a, Message> {
    // Stock `Button` has no `.tooltip()` (unlike icon buttons), so the
    // swatch is wrapped like transport's sliders; name/description are native.
    let swatch = model
        .picker_button(on_picker, None)
        .width(Length::Fixed(PICKER_SWATCH))
        .height(Length::Fixed(PICKER_SWATCH))
        .name(title)
        .description("Open the color picker");
    let swatch = cosmic::widget::tooltip(
        swatch,
        text::body(title),
        cosmic::widget::tooltip::Position::Top,
    );
    let control = row::with_children(vec![
        swatch.into(),
        text_input("", current)
            .on_input(on_text)
            .width(Length::Fixed(TEXT_FIELD_WIDTH))
            .into(),
    ])
    .spacing(8)
    .align_y(cosmic::iced::Alignment::Center);

    let mut rows = vec![settings::item(title, control).into()];
    if model.get_is_active() {
        rows.push(
            model
                .builder(on_picker)
                .reset_label("Reset")
                .save_label("Save")
                .cancel_label("Cancel")
                .build("Recent Colors", "Copy to clipboard", "Copied to clipboard")
                .into(),
        );
    }
    column::with_children(rows).spacing(4).into()
}

/// One font row: settings item with a Pango-string text field (`text_input`
/// per the ux D2 ruling — no Pango font picker, documented gap). Generic
/// over the scratch setter so both U6 fonts share the shape.
fn font_row<'a>(
    title: &'static str,
    current: &'a str,
    scratch: &'a SettingsState,
    on_scratch: fn(SettingsState) -> Message,
    set: fn(&mut SettingsState, String),
) -> Element<'a, Message> {
    settings::item(
        title,
        text_input("", current)
            .on_input(text_updater(scratch, on_scratch, set))
            .width(Length::Fixed(TEXT_FIELD_WIDTH)),
    )
    .into()
}

/// D2 Lyrics page rows: Future/Past/Highlight color rows (D17 picker + text
/// binding) + Lyrics font + Text alignment, in C++ order
/// (`window.cpp:1244-1264`). The T12/13 dialog shell embeds this in the
/// preferences dialog with its scratch `SettingsState`, its
/// [`LyricsPickerModels`], its whole-scratch edit-message constructor, and
/// its three per-row picker-update constructors (`fn` items — the picker
/// builder takes a `fn` pointer, so one per row is the routing shape).
pub fn lyrics_prefs_rows<'a>(
    scratch: &'a SettingsState,
    pickers: &'a LyricsPickerModels,
    on_scratch: fn(SettingsState) -> Message,
    on_picker: [fn(ColorPickerUpdate) -> Message; 3],
) -> Element<'a, Message> {
    let align_base = scratch.clone();
    let align_dropdown = dropdown(
        ALIGNMENT_OPTIONS.as_slice(),
        Some(alignment_selected(scratch.text_alignment)),
        move |index| {
            let mut next = align_base.clone();
            next.text_alignment = index as i32;
            on_scratch(next)
        },
    );
    view_column(vec![
        color_row(
            "Future text color",
            &scratch.future_color,
            &pickers.future,
            text_updater(scratch, on_scratch, set_future_color),
            on_picker[0],
        ),
        color_row(
            "Past text color",
            &scratch.past_color,
            &pickers.past,
            text_updater(scratch, on_scratch, set_past_color),
            on_picker[1],
        ),
        color_row(
            "Highlight color",
            &scratch.highlight_color,
            &pickers.highlight,
            text_updater(scratch, on_scratch, set_highlight_color),
            on_picker[2],
        ),
        font_row(
            "Lyrics font",
            &scratch.lyrics_font,
            scratch,
            on_scratch,
            set_lyrics_font,
        ),
        settings::item("Text alignment", align_dropdown).into(),
    ])
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::lyrics::{AppliedLyricsPrefs, parse_pango_font};

    fn test_vm() -> LyricsViewModel {
        LyricsViewModel {
            track_options: vec![
                "All tracks".to_string(),
                "Track 1 — Intro".to_string(),
                "Track 2".to_string(),
            ],
            track_selected: 1,
            type_options: vec!["All types".to_string(), "Text".to_string()],
            type_selected: 1,
            codec_options: vec!["UTF-8".to_string(), "SHIFT_JIS".to_string()],
            codec_selected: 0,
            body: "La la\n".to_string(),
            live_past: " sung".to_string(),
            live_current: " now".to_string(),
            sung: true,
            live_seq: 7,
            autoscroll: true,
            applied: AppliedLyricsPrefs::default(),
            has_song: true,
        }
    }

    fn test_picker_fn(update: ColorPickerUpdate) -> Message {
        Message::Toast(format!("{update:?}"))
    }

    #[test]
    fn clamp_selected_pins_bounds_and_empty() {
        assert_eq!(clamp_selected(1, 3), Some(1));
        assert_eq!(clamp_selected(0, 1), Some(0));
        assert_eq!(clamp_selected(99, 3), Some(2));
        assert_eq!(clamp_selected(5, 0), None);
    }

    #[test]
    fn rgba_maps_field_for_field_both_ways() {
        let rgba = Rgba {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 0.4,
        };
        let iced = rgba_to_iced(rgba);
        assert_eq!((iced.r, iced.g, iced.b, iced.a), (0.1, 0.2, 0.3, 0.4));
        assert_eq!(iced_to_rgba(iced), rgba);
    }

    #[test]
    fn weight_buckets_cover_all_parser_outputs() {
        assert_eq!(weight_from_u16(100), Weight::Thin);
        assert_eq!(weight_from_u16(200), Weight::ExtraLight);
        assert_eq!(weight_from_u16(300), Weight::Light);
        assert_eq!(weight_from_u16(350), Weight::Light);
        assert_eq!(weight_from_u16(380), Weight::Normal);
        assert_eq!(weight_from_u16(400), Weight::Normal);
        assert_eq!(weight_from_u16(500), Weight::Medium);
        assert_eq!(weight_from_u16(600), Weight::Semibold);
        assert_eq!(weight_from_u16(700), Weight::Bold);
        assert_eq!(weight_from_u16(800), Weight::ExtraBold);
        assert_eq!(weight_from_u16(900), Weight::Black);
        assert_eq!(weight_from_u16(1000), Weight::Black);
    }

    #[test]
    fn stretch_style_align_cover_every_variant() {
        assert_eq!(stretch_from_parsed(None), IcedStretch::Normal);
        for (parsed, iced) in [
            (FontStretch::UltraCondensed, IcedStretch::UltraCondensed),
            (FontStretch::ExtraCondensed, IcedStretch::ExtraCondensed),
            (FontStretch::Condensed, IcedStretch::Condensed),
            (FontStretch::SemiCondensed, IcedStretch::SemiCondensed),
            (FontStretch::SemiExpanded, IcedStretch::SemiExpanded),
            (FontStretch::Expanded, IcedStretch::Expanded),
            (FontStretch::ExtraExpanded, IcedStretch::ExtraExpanded),
            (FontStretch::UltraExpanded, IcedStretch::UltraExpanded),
        ] {
            assert_eq!(stretch_from_parsed(Some(parsed)), iced);
        }
        assert_eq!(style_from_parsed(FontStyle::Normal), IcedStyle::Normal);
        assert_eq!(style_from_parsed(FontStyle::Italic), IcedStyle::Italic);
        assert_eq!(style_from_parsed(FontStyle::Oblique), IcedStyle::Oblique);
        assert_eq!(align_to_horizontal(LyricAlign::Left), Horizontal::Left);
        assert_eq!(align_to_horizontal(LyricAlign::Center), Horizontal::Center);
        assert_eq!(align_to_horizontal(LyricAlign::Right), Horizontal::Right);
    }

    #[test]
    fn intern_family_leaks_once_per_distinct_name() {
        let first = intern_family("DejaVu Sans Mono");
        let second = intern_family("DejaVu Sans Mono");
        assert!(std::ptr::eq(first, second));
        let other = intern_family("Sans");
        assert!(!std::ptr::eq(first, other));
        assert_eq!((first, other), ("DejaVu Sans Mono", "Sans"));
    }

    #[test]
    fn lyrics_font_applies_first_family_weight_style_stretch() {
        // Both U6 fonts share this path: lyrics default + notes default.
        let lyrics = lyrics_font(&parse_pango_font("Sans 16"));
        assert!(matches!(lyrics.family, Family::Name("Sans")));
        assert_eq!(lyrics.weight, Weight::Normal);
        let notes = lyrics_font(&parse_pango_font("Sans 9"));
        assert!(matches!(notes.family, Family::Name("Sans")));
        let fancy = lyrics_font(&parse_pango_font(
            "DejaVu Sans, Sans Condensed Bold Oblique 12",
        ));
        assert!(matches!(fancy.family, Family::Name("DejaVu Sans")));
        assert_eq!(fancy.weight, Weight::Bold);
        assert_eq!(fancy.style, IcedStyle::Oblique);
        assert_eq!(fancy.stretch, IcedStretch::Condensed);
        // `small_caps` parses but renders as normal text (no iced mapping).
        let caps = lyrics_font(&parse_pango_font("Sans Small-Caps 12"));
        assert!(matches!(caps.family, Family::Name("Sans")));
        assert_eq!(
            font_size_px(parse_pango_font("Sans 16").size, 16.0),
            16.0 * 96.0 / 72.0
        );
    }

    #[test]
    fn body_spans_skip_empties_and_color_by_sung() {
        let applied = AppliedLyricsPrefs::from_rows("red", "green", "blue", "Sans 16", 0);
        // Unsung body renders future; live runs render past/highlight.
        let spans = body_spans("body", "past", "current", false, &applied);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].color, Some(rgba_to_iced(applied.future)));
        assert_eq!(spans[1].color, Some(rgba_to_iced(applied.past)));
        assert_eq!(spans[2].color, Some(rgba_to_iced(applied.highlight)));
        // Sung body flips to past.
        let spans = body_spans("body", "past", "current", true, &applied);
        assert_eq!(spans[0].color, Some(rgba_to_iced(applied.past)));
        // Empties are skipped, in order.
        let spans = body_spans("", "past", "", true, &applied);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].color, Some(rgba_to_iced(applied.past)));
        assert!(body_spans("", "", "", false, &applied).is_empty());
    }

    #[test]
    fn alignment_selected_shows_effective_alignment() {
        assert_eq!(alignment_selected(0), 0);
        assert_eq!(alignment_selected(1), 1);
        assert_eq!(alignment_selected(2), 2);
        // Out-of-range renders Left — and selects Left too.
        assert_eq!(alignment_selected(99), 0);
        assert_eq!(alignment_selected(-1), 0);
    }

    #[test]
    fn seed_uses_parsed_color_or_nothing() {
        let valid = seed_color_picker("#1c71d8");
        assert_eq!(
            valid.get_applied_color(),
            Some(rgba_to_iced(parse_gdk_rgba("#1c71d8").unwrap()))
        );
        assert!(!valid.get_is_active());
        // Invalid text seeds nothing (gray stays render-only, never seeded).
        for garbage in ["notacolor", "", "   ", "transparent"] {
            assert_eq!(
                seed_color_picker(garbage).get_applied_color(),
                None,
                "{garbage:?}"
            );
        }
        // All four D2 color keys share this hook (T12 reuses it for single).
        let single = seed_color_picker("#f6d32d");
        assert_eq!(
            single.get_applied_color(),
            Some(rgba_to_iced(parse_gdk_rgba("#f6d32d").unwrap()))
        );
    }

    #[test]
    fn drive_follows_valid_text_and_keeps_last_good() {
        let mut model = seed_color_picker("red");
        drive_picker_from_text(&mut model, "rgb(0, 0, 255)");
        let last_good = model.get_applied_color();
        assert_eq!(
            last_good,
            Some(rgba_to_iced(parse_gdk_rgba("rgb(0, 0, 255)").unwrap()))
        );
        // Garbage keeps last-good (model untouched).
        drive_picker_from_text(&mut model, "notacolor");
        assert_eq!(model.get_applied_color(), last_good);
        drive_picker_from_text(&mut model, "");
        assert_eq!(model.get_applied_color(), last_good);
        // The open state survives a re-drive.
        let _ = model.update::<Message>(ColorPickerUpdate::ToggleColorPicker);
        assert!(model.get_is_active());
        drive_picker_from_text(&mut model, "green");
        assert!(model.get_is_active());
        assert_eq!(
            model.get_applied_color(),
            Some(rgba_to_iced(parse_gdk_rgba("green").unwrap()))
        );
    }

    fn test_scratch() -> (LyricsPickerModels, SettingsState) {
        let scratch = SettingsState::new();
        let models = LyricsPickerModels::from_settings(&scratch);
        (models, scratch)
    }

    #[test]
    fn apply_color_text_always_wins_drives_only_on_some() {
        let (mut models, mut scratch) = test_scratch();
        // Valid edit: text + model follow.
        apply_color_text(
            &mut models,
            &mut scratch,
            LyricsColorRow::Past,
            "rgb(0, 0, 255)".to_string(),
        );
        assert_eq!(scratch.past_color, "rgb(0, 0, 255)");
        assert_eq!(
            models.past.get_applied_color(),
            Some(rgba_to_iced(parse_gdk_rgba("rgb(0, 0, 255)").unwrap()))
        );
        // Garbage edit: text still wins (source of truth, verbatim like
        // C++); the model keeps last-good.
        let last_good = models.past.get_applied_color();
        apply_color_text(
            &mut models,
            &mut scratch,
            LyricsColorRow::Past,
            "notacolor".to_string(),
        );
        assert_eq!(scratch.past_color, "notacolor");
        assert_eq!(models.past.get_applied_color(), last_good);
        // Untouched rows are unaffected.
        assert_eq!(scratch.future_color, "#888888");
    }

    #[test]
    fn adopt_redrives_only_changed_rows() {
        let (mut models, mut scratch) = test_scratch();
        let past_before = models.past.get_applied_color();
        let mut next = scratch.clone();
        next.future_color = "#ff0000".to_string();
        next.lyrics_font = "Monospace 9".to_string();
        next.text_alignment = 2;
        adopt_prefs_scratch(&mut models, &mut scratch, next);
        assert_eq!(scratch.future_color, "#ff0000");
        assert_eq!(scratch.lyrics_font, "Monospace 9");
        assert_eq!(scratch.text_alignment, 2);
        assert_eq!(
            models.future.get_applied_color(),
            Some(rgba_to_iced(parse_gdk_rgba("#ff0000").unwrap()))
        );
        assert_eq!(models.past.get_applied_color(), past_before);
        // Adopting identical scratch drives nothing.
        let same = scratch.clone();
        adopt_prefs_scratch(&mut models, &mut scratch, same);
        assert_eq!(scratch.future_color, "#ff0000");
    }

    /// Hex text ≈ expected color: the picker's apply path round-trips
    /// through HSV floats, so exact-hex equality is too strict — one
    /// 8-bit step of slack per channel.
    fn assert_hex_near(hex: &str, expected: Rgba) {
        let parsed = parse_gdk_rgba(hex).unwrap_or_else(|| panic!("not hex: {hex:?}"));
        for (got, want) in [
            (parsed.r, expected.r),
            (parsed.g, expected.g),
            (parsed.b, expected.b),
            (parsed.a, expected.a),
        ] {
            assert!((got - want).abs() < 2.0 / 255.0, "{hex:?} vs {expected:?}");
        }
    }

    #[test]
    fn picker_apply_events_write_canonical_hex() {
        let (mut models, mut scratch) = test_scratch();
        scratch.highlight_color = "garbage-in".to_string();
        // Save commits active (seeded default) → canonical hex-ish text.
        let _ = apply_picker_update(
            &mut models,
            &mut scratch,
            LyricsColorRow::Highlight,
            ColorPickerUpdate::AppliedColor,
        );
        assert_hex_near(&scratch.highlight_color, parse_gdk_rgba("#e01b24").unwrap());
        // ActionFinished (slider/canvas release, picker-input submit) also
        // commits active → applied in the model, so it writes canonical hex
        // too (T11 follow-up 2: all three apply events proven here).
        let _ = apply_picker_update(
            &mut models,
            &mut scratch,
            LyricsColorRow::Highlight,
            ColorPickerUpdate::ActionFinished,
        );
        assert_hex_near(&scratch.highlight_color, parse_gdk_rgba("#e01b24").unwrap());
        let committed = scratch.highlight_color.clone();
        // Non-apply events leave the text alone.
        for update in [
            ColorPickerUpdate::ToggleColorPicker,
            ColorPickerUpdate::Cancel,
            ColorPickerUpdate::Input("blue".to_string()),
        ] {
            let _ =
                apply_picker_update(&mut models, &mut scratch, LyricsColorRow::Highlight, update);
        }
        assert_eq!(scratch.highlight_color, committed);
        // Reset restores the seeded fallback (exact — no HSV round trip).
        let _ = apply_picker_update(
            &mut models,
            &mut scratch,
            LyricsColorRow::Highlight,
            ColorPickerUpdate::Reset,
        );
        assert_eq!(scratch.highlight_color, "#e01b24");
    }

    #[test]
    fn picker_save_after_text_drive_writes_driven_color() {
        let (mut models, mut scratch) = test_scratch();
        apply_color_text(
            &mut models,
            &mut scratch,
            LyricsColorRow::Future,
            "blue".to_string(),
        );
        let _ = apply_picker_update(
            &mut models,
            &mut scratch,
            LyricsColorRow::Future,
            ColorPickerUpdate::AppliedColor,
        );
        assert_hex_near(&scratch.future_color, parse_gdk_rgba("blue").unwrap());
    }

    #[test]
    fn text_updater_emits_whole_scratch_with_row_set() {
        let scratch = SettingsState::new();
        let update = text_updater(&scratch, Message::PrefsChanged, set_past_color);
        match update("red".to_string()) {
            Message::PrefsChanged(next) => {
                assert_eq!(next.past_color, "red");
                assert_eq!(next.future_color, scratch.future_color);
                assert_eq!(next.lyrics_font, scratch.lyrics_font);
            }
            other => panic!("expected PrefsChanged, got {other:?}"),
        }
        // Both U6 fonts share the row shape (notes setter shown for T12;
        // headless per D13 — parsed the same, applied nowhere).
        fn set_notes_font(scratch: &mut SettingsState, value: String) {
            scratch.notes_font = value;
        }
        let update = text_updater(&scratch, Message::PrefsChanged, set_notes_font);
        match update("Monospace 9".to_string()) {
            Message::PrefsChanged(next) => assert_eq!(next.notes_font, "Monospace 9"),
            other => panic!("expected PrefsChanged, got {other:?}"),
        }
    }

    #[test]
    fn view_builds_across_contract_shapes() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes: full, unsung, no-song, filtered
        // empty, stale selections, and empty option lists.
        let _ = view(test_vm());
        let mut unsung = test_vm();
        unsung.sung = false;
        let _ = view(unsung);
        let mut no_song = test_vm();
        no_song.has_song = false;
        no_song.body.clear();
        no_song.live_past.clear();
        no_song.live_current.clear();
        let _ = view(no_song);
        let mut filtered = test_vm();
        filtered.body.clear();
        filtered.live_past.clear();
        filtered.live_current.clear();
        let _ = view(filtered);
        let mut stale = test_vm();
        stale.track_selected = 99;
        stale.type_selected = 99;
        stale.codec_selected = 99;
        let _ = view(stale);
        let mut empty_opts = test_vm();
        empty_opts.track_options.clear();
        empty_opts.type_options.clear();
        empty_opts.codec_options.clear();
        let _ = view(empty_opts);
        // The snap helper builds (a widget-operation task; never emits).
        let _ = snap_to_end_task();
    }

    #[test]
    fn prefs_rows_build_open_and_closed() {
        let (mut models, scratch) = test_scratch();
        let on_picker: [fn(ColorPickerUpdate) -> Message; 3] = [test_picker_fn; 3];
        let _ = lyrics_prefs_rows(&scratch, &models, Message::PrefsChanged, on_picker);
        // One picker open renders the inline dialog branch too.
        let _ = models
            .future
            .update::<Message>(ColorPickerUpdate::ToggleColorPicker);
        assert!(models.future.get_is_active());
        let _ = lyrics_prefs_rows(&scratch, &models, Message::PrefsChanged, on_picker);
        // Garbage scratch texts + out-of-range alignment still build.
        let mut weird = scratch.clone();
        weird.future_color = "notacolor".to_string();
        weird.text_alignment = 99;
        let weird_models = LyricsPickerModels::from_settings(&weird);
        let _ = lyrics_prefs_rows(&weird, &weird_models, Message::PrefsChanged, on_picker);
        // The notes-font row shape builds (headless per D13, T12 reuse).
        fn set_notes_font(scratch: &mut SettingsState, value: String) {
            scratch.notes_font = value;
        }
        let _ = font_row(
            "Notes font",
            &scratch.notes_font,
            &scratch,
            Message::PrefsChanged,
            set_notes_font,
        );
    }
}
