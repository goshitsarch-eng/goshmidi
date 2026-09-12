//! Transport/dashboard/status styles: the `data/style.css` (§7) mapping.
//!
//! Every item is a plain `fn(&Theme) -> Style` for `Container::custom` /
//! [`text_class`], so widgets stay theme-aware: card-local text uses the
//! explicit style.css hex (readable on the explicit dark card in both modes,
//! per the ux §7 ruling), while [`dash_value`] and [`dim_label`] derive from
//! the active COSMIC theme.
//!
//! `suggested-action` (D3 Apply, T13) maps to stock
//! `cosmic::widget::button::suggested` / `cosmic::theme::Button::Suggested` —
//! no custom style needed, so there is intentionally no item for it here.
//! (The transport Tempo/Volume reset buttons stay `standard`: plain
//! `GtkButton`s in C++, `window.cpp:1708-1709`.)
//!
//! Placement: `ui::theme` (T10b moved the declaration up from the T9
//! `transport::theme` path-attribute hack).

use cosmic::Theme;
use cosmic::iced::widget::{container, text};
use cosmic::iced::{Background, Border, Color};

/// `.player-dashboard` background (`style.css:2`).
const CARD_BG: Color = Color::from_rgb8(0x11, 0x11, 0x11);
/// `.player-dashboard` / `.time-display` text (`style.css:3,13`).
const CARD_TEXT: Color = Color::from_rgb8(0xF6, 0xF5, 0xF4);
/// `.dash-label` (`style.css:17`).
const DASH_LABEL: Color = Color::from_rgb8(0xC0, 0xBF, 0xBC);
/// `.rhythm-lamp` rest fill (`style.css:30`).
const LAMP_REST: Color = Color::from_rgb8(0x3D, 0x38, 0x46);
/// `.rhythm-lamp.on` fill (`style.css:34`).
const LAMP_ON: Color = Color::from_rgb8(0x57, 0xE3, 0x89);
/// `.rhythm-lamp.accent` fill (`style.css:38`).
const LAMP_ACCENT: Color = Color::from_rgb8(0xF6, 0x61, 0x51);

/// Plain colored text on the default selection colors.
fn ink(color: Color) -> text::Style {
    text::Style {
        color: Some(color),
        ..text::Style::default()
    }
}

/// Wrap a text style fn as a COSMIC text class: the `Text` catalog has no
/// `From<StyleFn>` (unlike `Container`), so `Text::style(f)` does not
/// typecheck — widgets use `.class(text_class(f))` instead.
pub fn text_class(style: fn(&Theme) -> text::Style) -> cosmic::theme::Text {
    cosmic::theme::Text::Custom(style)
}

/// `.player-dashboard` card: explicit dark fill + light inherited text, COSMIC
/// medium radius (≈ the 16px CSS radius). Only the radius reads the theme so
/// both modes keep contrast (ux §7 ruling).
pub fn dashboard_card(theme: &Theme) -> container::Style {
    container::Style {
        icon_color: Some(CARD_TEXT),
        text_color: Some(CARD_TEXT),
        background: Some(Background::Color(CARD_BG)),
        border: Border {
            radius: theme.cosmic().corner_radii.radius_m.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// `.time-display` ink — 40px bold mono is applied at the widget.
pub fn time_display(_theme: &Theme) -> text::Style {
    ink(CARD_TEXT)
}

/// `.dash-label` ink — 12px caption applied at the widget.
pub fn dash_label(_theme: &Theme) -> text::Style {
    ink(DASH_LABEL)
}

/// `.dash-value` ink: the active theme accent (ux §7: "theme accent, closest
/// to `#62a0ea`"). Readability note: in light mode the accent base is tuned
/// for light surfaces, so on this explicit dark card it renders dimmer than
/// in dark mode — still the spec'd mapping; revisit at theme review.
pub fn dash_value(theme: &Theme) -> text::Style {
    ink(theme.cosmic().accent.base.into())
}

/// `dim-label` ink for the status line: theme foreground at Adwaita's ~55%
/// dim opacity (same idea as the in-tree disabled-text convention, which
/// halves alpha). Theme-derived, so it stays readable in both modes.
pub fn dim_label(theme: &Theme) -> text::Style {
    let mut dimmed = Color::from(theme.cosmic().on_bg_color());
    dimmed.a *= 0.55;
    ink(dimmed)
}

/// `.rhythm-lamp` states (`style.css:26-39`): rest, lit beat (`.on`), lit
/// downbeat (`.accent`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmLamp {
    Rest,
    Beat,
    Downbeat,
}

/// Lamp fill for `state`, factored pure so the mapping is unit-testable
/// without a `Theme`.
pub fn lamp_color(state: RhythmLamp) -> Color {
    match state {
        RhythmLamp::Rest => LAMP_REST,
        RhythmLamp::Beat => LAMP_ON,
        RhythmLamp::Downbeat => LAMP_ACCENT,
    }
}

/// `.rhythm-lamp` dot: solid fill + fully rounded border; the widget fixes
/// the 14×14 size (`style.css:27-28`, radius 7px for the 14px circle).
pub fn rhythm_lamp(state: RhythmLamp) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(lamp_color(state))),
        border: Border {
            radius: 7.0f32.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Selected-row wash alpha: the accent stays recognizable on the wash in
/// both modes without swallowing the row text.
const SELECTED_ROW_ALPHA: f32 = 0.28;

/// Playlist sidebar row (`navigation-sidebar` rows, `window.cpp:1476-1480`):
/// the selected row gets an accent wash + small theme radius, unselected
/// rows stay transparent with the same radius (so selection never shifts
/// layout). Theme-derived, so it stays readable in both modes.
pub fn playlist_row(selected: bool) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let background = if selected {
            let mut wash: Color = theme.cosmic().accent.base.into();
            wash.a = SELECTED_ROW_ALPHA;
            Some(Background::Color(wash))
        } else {
            None
        };
        container::Style {
            background,
            border: Border {
                radius: theme.cosmic().corner_radii.radius_s.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamp_colors_match_style_css() {
        assert_eq!(
            lamp_color(RhythmLamp::Rest),
            Color::from_rgb8(0x3D, 0x38, 0x46)
        );
        assert_eq!(
            lamp_color(RhythmLamp::Beat),
            Color::from_rgb8(0x57, 0xE3, 0x89)
        );
        assert_eq!(
            lamp_color(RhythmLamp::Downbeat),
            Color::from_rgb8(0xF6, 0x61, 0x51)
        );
    }

    #[test]
    fn lamp_style_is_a_solid_dot() {
        // `rhythm_lamp` ignores the theme (explicit hex per ux §7); the
        // closure shape is what `Container::custom` consumes.
        let style = rhythm_lamp(RhythmLamp::Beat)(&Theme::dark());
        assert_eq!(
            style.background,
            Some(Background::Color(Color::from_rgb8(0x57, 0xE3, 0x89)))
        );
    }

    #[test]
    fn playlist_row_wash_only_when_selected() {
        let theme = Theme::dark();
        let selected = playlist_row(true)(&theme);
        let wash = match selected.background {
            Some(Background::Color(color)) => color,
            other => panic!("expected a wash fill, got {other:?}"),
        };
        let accent: Color = theme.cosmic().accent.base.into();
        assert_eq!((wash.r, wash.g, wash.b), (accent.r, accent.g, accent.b));
        assert!((wash.a - SELECTED_ROW_ALPHA).abs() < f32::EPSILON);
        assert!(playlist_row(false)(&theme).background.is_none());
    }
}
