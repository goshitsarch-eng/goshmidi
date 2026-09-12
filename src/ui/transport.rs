//! Header transport + dashboard + sliders + status view (ux §3.2–3.4, §3.9).
//!
//! Real T9b implementation of the frozen [`TransportViewModel`] contract,
//! consumed as-is: owned `vm` in, one [`cosmic::Element`] out — a column of
//! transport row, dashboard card, sliders, and status line. (T14 moves the
//! transport row into the real `header_bar`; the T9 shell stacks regions.)
//!
//! Widget → `Message` map (all variants exist in `message.rs`):
//!
//! | widget | message |
//! |---|---|
//! | Open | `OpenFilesDialog` (D1; T9 gap 1 closed in T10b) |
//! | Prev / Play / Pause / Stop / Next | `PrevSong` / `Play` / `Pause` / `Stop` / `NextSong` |
//! | Rew / Fwd (±1 bar) | `BeatBackward` / `BeatForward` |
//! | Loop toggle | `LoopToggled(!loop_enabled)` |
//! | Pitch spin −12..+12 | `PitchChanged(i32)` |
//! | Position slider 0..1000 | `PositionSeek(f64)` |
//! | Tempo slider 50..200 / Tempo reset | `TempoChanged(u32)` / `TempoChanged(100)` |
//! | Volume slider 0..200 / Volume reset | `VolumeChanged(u32)` / `VolumeChanged(100)` |
//!
//! T9b → lead contract notes (arch-owned files untouched; T10b resolutions
//! inline):
//!
//! 1. CLOSED in T10b: header Open now emits `Message::OpenFilesDialog`
//!    (the T10a D1 portal-chooser trigger); results arrive as
//!    `OpenFilesChosen` into the `OpenFiles(paths, true)` path.
//! 2. CLOSED (T10 finding 4): `App::footer` is gone — app.rs keeps no
//!    `footer()` override — so this status line is the only one.
//! 3. C++ parity (lead ruling): the 7 transport action buttons + loop
//!    stay live with no song loaded — C++ never desensitizes (e.g. Play
//!    with no song triggers `loadCurrent`). `has_song` remains in the
//!    model but is not consulted for sensitivity. Sliders, the pitch
//!    spin, and the reset buttons stay live like their C++ twins
//!    (`PositionSeek` already no-ops arch-side when no song is loaded).
//! 4. CLOSED in T10b: `theme` moved up to `ui::theme` (`ui/mod.rs`); the
//!    T9 path-attribute hack is gone.
//!
//! [`TransportViewModel`]: super::TransportViewModel

use cosmic::Element;
use cosmic::iced::Alignment;
use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit};
use cosmic::iced::font::Weight;
use cosmic::iced::widget::text::LineHeight;
use cosmic::iced::{Length, Padding};
use cosmic::widget::tooltip::Position as TooltipPosition;
use cosmic::widget::{button, column, container, icon, row, slider, space, spin_button, text};

use super::TransportViewModel;
use super::theme::{self, RhythmLamp};
use crate::message::Message;

/// Position slider resolution (`m_posScale` 0..1000, `window.cpp:1696`).
const POSITION_STEPS: u32 = 1000;
/// Rhythm lamp diameter (`style.css:27-28`).
const LAMP_DIAMETER: f32 = 14.0;
/// Stats label column width (right-aligned `dash-label`s, grid column
/// spacing 10 mirrored in the row spacing below).
const STAT_LABEL_WIDTH: f32 = 52.0;
/// Lamp-row safety cap: the MIDI time-signature numerator is a `u8`, so no
/// real song needs more lamps; guards a corrupt `beats` against a giant row
/// (C++ grows the box unbounded).
const MAX_LAMPS: u32 = 256;

/// View-model position (0.0–1.0) → slider steps (0–1000).
fn position_to_slider(position: f64) -> u32 {
    (position.clamp(0.0, 1.0) * f64::from(POSITION_STEPS)).round() as u32
}

/// Slider steps (0–1000) → `PositionSeek` fraction (C++ `value/1000*len`).
fn slider_to_position(steps: u32) -> f64 {
    f64::from(steps.min(POSITION_STEPS)) / f64::from(POSITION_STEPS)
}

/// 1-based lamp style: lit iff `index == beat`, downbeat accent iff
/// `index == 1` (`window.cpp:126-128`).
fn lamp_state(index: u32, beat: u32) -> RhythmLamp {
    if index == beat {
        if index == 1 {
            RhythmLamp::Downbeat
        } else {
            RhythmLamp::Beat
        }
    } else {
        RhythmLamp::Rest
    }
}

/// One header transport icon button: freedesktop symbolic icon + tooltip +
/// a11y name/description; `None` renders insensitive.
fn transport_button<'a>(
    icon_name: &'static str,
    name: &'static str,
    description: &'static str,
    on_press: Option<Message>,
    selected: bool,
) -> Element<'a, Message> {
    button::icon(icon::from_name(icon_name))
        .on_press_maybe(on_press)
        .selected(selected)
        .tooltip(name)
        .name(name)
        .description(description)
        .into()
}

/// Right-aligned 12px `dash-label` in the fixed stats column.
fn stat_label(label: &'static str) -> Element<'static, Message> {
    container(text::caption(label).class(theme::text_class(theme::dash_label)))
        .width(Length::Fixed(STAT_LABEL_WIDTH))
        .align_x(Alignment::End)
        .into()
}

/// Transport/dashboard/sliders/status region. `vm` is the whole contract —
/// no other `App` access. By value: owned data moves into the widgets (see
/// [`TransportViewModel`]).
pub fn view<'a>(vm: TransportViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let TransportViewModel {
        song_title,
        status,
        time_text,
        tempo_text,
        volume_text,
        pitch,
        bar_beat,
        beats,
        beat,
        position,
        // `seeking` is an informational in-flight marker only — nothing
        // gates on it; engine `Time` overwrites `position`
        // unconditionally, and the view renders `position` verbatim.
        seeking: _seeking,
        tempo_pct,
        volume_pct,
        loop_enabled,
        // Note 3: kept in the contract but not consulted for sensitivity
        // (C++ parity — never desensitizes).
        has_song: _has_song,
    } = vm;

    // Note 3: transport actions stay live with no song (C++ parity).
    let transport_row = row::with_children(vec![
        transport_button(
            "document-open-symbolic",
            "Open",
            "Open MIDI files",
            Some(Message::OpenFilesDialog),
            false,
        ),
        transport_button(
            "media-skip-backward-symbolic",
            "Prev",
            "Previous song",
            Some(Message::PrevSong),
            false,
        ),
        transport_button(
            "media-playback-start-symbolic",
            "Play",
            "Play",
            Some(Message::Play),
            false,
        ),
        transport_button(
            "media-playback-pause-symbolic",
            "Pause",
            "Pause",
            Some(Message::Pause),
            false,
        ),
        transport_button(
            "media-playback-stop-symbolic",
            "Stop",
            "Stop",
            Some(Message::Stop),
            false,
        ),
        transport_button(
            "media-skip-forward-symbolic",
            "Next",
            "Next song",
            Some(Message::NextSong),
            false,
        ),
        transport_button(
            "media-seek-backward-symbolic",
            "Rew",
            "Back one bar",
            Some(Message::BeatBackward),
            false,
        ),
        transport_button(
            "media-seek-forward-symbolic",
            "Fwd",
            "Forward one bar",
            Some(Message::BeatForward),
            false,
        ),
        transport_button(
            "media-playlist-repeat-symbolic",
            "Loop",
            "Loop a bar range",
            Some(Message::LoopToggled(!loop_enabled)),
            loop_enabled,
        ),
    ])
    .spacing(4)
    .align_y(Alignment::Center);

    let time = text::monotext(time_text)
        .size(40)
        .line_height(LineHeight::Absolute(48.0.into()))
        .font(cosmic::font::Font {
            weight: Weight::Bold,
            ..cosmic::font::mono()
        })
        .class(theme::text_class(theme::time_display));

    let tempo_value = text::body(tempo_text)
        .font(cosmic::font::semibold())
        .class(theme::text_class(theme::dash_value));
    let volume_value = text::body(volume_text)
        .font(cosmic::font::semibold())
        .class(theme::text_class(theme::dash_value));
    // `spin_button` clamps into range and reports the stepped value, matching
    // the −12..+12 step-1 `GtkSpinButton` (`window.cpp:1668-1669`).
    let pitch_spin = spin_button(
        pitch.to_string(),
        "Pitch",
        pitch,
        1,
        -12,
        12,
        Message::PitchChanged,
    );
    let stats = column::with_children(vec![
        row::with_children(vec![stat_label("Tempo"), tempo_value.into()])
            .spacing(10)
            .into(),
        row::with_children(vec![stat_label("Volume"), volume_value.into()])
            .spacing(10)
            .into(),
        row::with_children(vec![stat_label("Pitch"), pitch_spin.into()])
            .spacing(10)
            .into(),
    ])
    .spacing(4);

    // Rhythm lamps resize to the meter (`beats`); the lit lamp accents on
    // the downbeat. `container` offers no naming API (T14 follow-up); the
    // `bar:beat` label below is the text equivalent (ux §11), so no
    // information is pointer-only.
    let mut lamps = Vec::new();
    for index in 1..=beats.min(MAX_LAMPS) {
        lamps.push(
            container(space::Space::new())
                .width(Length::Fixed(LAMP_DIAMETER))
                .height(Length::Fixed(LAMP_DIAMETER))
                .class(cosmic::theme::Container::custom(theme::rhythm_lamp(
                    lamp_state(index, beat),
                )))
                .into(),
        );
    }
    let dash_right = column::with_children(vec![
        text::body(song_title)
            .width(Length::Fill)
            .ellipsize(Ellipsize::Middle(EllipsizeHeightLimit::Lines(1)))
            .into(),
        row::with_children(lamps).spacing(6).into(),
        text::body(bar_beat).into(),
    ])
    .spacing(6)
    .width(Length::Fill);

    let dashboard = container(
        container(
            row::with_children(vec![time.into(), stats.into(), dash_right.into()])
                .spacing(18)
                .align_y(Alignment::Center),
        )
        .padding([16, 20])
        .class(cosmic::theme::Container::custom(theme::dashboard_card))
        .width(Length::Fill),
    )
    .padding([8, 12])
    .width(Length::Fill);

    // No GTK tooltip on the position slider — a11y name/description only.
    let position_slider = slider(0..=POSITION_STEPS, position_to_slider(position), |steps| {
        Message::PositionSeek(slider_to_position(steps))
    })
    .name("Position")
    .description("Song position");
    // Tooltip strings are the exact C++ texts (en-dash U+2013,
    // `window.cpp:1703-1707`).
    let tempo_slider = cosmic::widget::tooltip(
        slider(50..=200u32, tempo_pct, Message::TempoChanged)
            .name("Tempo")
            .description("Playback tempo, 50 to 200 percent"),
        text::body("Tempo 50–200%"),
        TooltipPosition::Top,
    );
    let volume_slider = cosmic::widget::tooltip(
        slider(0..=200u32, volume_pct, Message::VolumeChanged)
            .name("Volume")
            .description("Playback volume, 0 to 200 percent"),
        text::body("Volume 0–200%"),
        TooltipPosition::Top,
    );
    // Resets emit the 100 message directly (C++ sets the scale to 100, which
    // fires the same `value-changed` path, `window.cpp:1727-1735`).
    let tempo_reset = button::standard("Tempo")
        .on_press(Message::TempoChanged(100))
        .name("Tempo")
        .description("Reset tempo to 100 percent");
    let volume_reset = button::standard("Volume")
        .on_press(Message::VolumeChanged(100))
        .name("Volume")
        .description("Reset volume to 100 percent");
    let controls = container(
        column::with_children(vec![
            position_slider.into(),
            row::with_children(vec![
                tempo_reset.into(),
                tempo_slider.into(),
                volume_reset.into(),
                volume_slider.into(),
            ])
            .spacing(12)
            .into(),
        ])
        .spacing(4),
    )
    .padding([0, 16])
    .width(Length::Fill);

    let status_line = container(
        text::body(status.text()).class(theme::text_class(theme::dim_label)),
    )
    .padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 6.0,
        left: 12.0,
    });

    column::with_children(vec![
        transport_row.into(),
        dashboard.into(),
        controls.into(),
        status_line.into(),
    ])
    .spacing(8)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_slider_mapping_round_trips() {
        assert_eq!(position_to_slider(0.0), 0);
        assert_eq!(position_to_slider(1.0), POSITION_STEPS);
        assert_eq!(position_to_slider(0.25), 250);
        assert_eq!(position_to_slider(-1.0), 0);
        assert_eq!(position_to_slider(2.0), POSITION_STEPS);
        assert!((slider_to_position(0) - 0.0).abs() < f64::EPSILON);
        assert!((slider_to_position(1000) - 1.0).abs() < f64::EPSILON);
        assert!((slider_to_position(250) - 0.25).abs() < f64::EPSILON);
        assert!((slider_to_position(9999) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn lamp_state_accents_only_the_downbeat() {
        assert_eq!(lamp_state(1, 1), RhythmLamp::Downbeat);
        assert_eq!(lamp_state(2, 2), RhythmLamp::Beat);
        assert_eq!(lamp_state(1, 3), RhythmLamp::Rest);
        assert_eq!(lamp_state(2, 0), RhythmLamp::Rest);
        assert_eq!(lamp_state(5, 9), RhythmLamp::Rest);
    }
}
