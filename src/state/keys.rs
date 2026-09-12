//! Keyboard shortcuts: the 9 C++ accelerators + dialog focus gating,
//! sidebar Enter-play, and Tab focus traversal (T14a, arch §2.3).
//!
//! Pure mapping only — no subscription, no update arms (composition owns
//! `app.rs`/`message.rs`): [`resolve_key`] maps one ignored-status
//! [`keyboard::Event`] plus the dialog/selection context to one
//! [`KeyAction`], and [`KeyAction::message`] carries the 1:1 composition
//! mapping for the message-backed actions. (`Quit` returns `None` —
//! composition routes it to the `Message::Quit` arm (`message.rs:234`,
//! `app.rs:2551`, T14d wired); `FocusNext`/`FocusPrevious` need
//! `widget::operation` `Task`s — all three landed as the T14a
//! pseudocode proposed, since `message.rs`/`app.rs` are
//! composition-owned.)
//!
//! The 9 accelerators (`window.cpp:1440-1455`, pinned by [`ACCELERATORS`]):
//!
//! | C++ accel | action | `KeyAction` |
//! |---|---|---|
//! | `<Ctrl>o` | `app.open` | `Open` |
//! | `<Ctrl>q` | `app.quit` | `Quit` |
//! | `space` | `app.play` | `Play` |
//! | `<Ctrl>p` | `app.pause` | `Pause` |
//! | `<Ctrl>s` | `app.stop` | `Stop` |
//! | `<Ctrl>Right` | `app.next` | `Next` |
//! | `<Ctrl>Left` | `app.prev` | `Prev` |
//! | `F1` | `app.help` | `Help` |
//! | `<Ctrl>comma` | `app.prefs` | `Prefs` |
//!
//! Modifier semantics match GTK accel matching: exact non-lock modifiers
//! (extra Shift/Alt/Logo do NOT fire — except Shift on Space, which changes
//! neither the iced `key` (`key_without_modifiers`, pinned
//! `iced/winit/src/conversion.rs:341-349`) nor the GTK space keyval), lock
//! modifiers (`CapsLock`) ignored, letters case-folded. Letter matching runs
//! through [`Key::to_latin`] (the same call `text_input` uses for its
//! C/X/V shortcuts), so non-latin layouts match by physical position — a
//! deliberate superset over GTK, which matches the layout keyval only.
//!
//! Focus gating (T12 residual closure): with ANY dialog open, every key
//! except Esc (dismiss) and Tab/Shift+Tab (dialog traversal) resolves to
//! `None`. C++ verification: D7/D8/D3 are explicit modal windows
//! (`window.cpp:781,841,952`), D6/D5/D4 are `AdwDialog`s (modal by
//! default), D1/D9/D10/D11/D12 are native modal choosers — key events route
//! to the dialog and main-window widget keys (rows, buttons, spins) can
//! never fire. Two honest deltas: (1) C++ app accelerators are app-global
//! ([GTK docs](https://docs.gtk.org/gtk4/actions.html): shortcuts "function
//! across the entire application") and would fire above a modal; the port
//! gates them uniformly because the single-dialog state machine cannot
//! stack a second dialog (e.g. F1 above Jump) — quit stays available via
//! the window chrome and every dialog dismisses with Esc (two-step from
//! a focused text field: cosmic `text_input` captures the first Esc to
//! unfocus, pinned `input.rs:2235-2243` + blanket capture at `:2270` —
//! D2 color/font and D3 soundfont fields). (2) C++ D2 prefs
//! is a NON-modal transient (`window.cpp:1192` sets transient only); the
//! port's D2 is modal (DECISIONS D20), so it gates like every other dialog.
//!
//! Double-fire safety comes from the subscription, not this module:
//! composition must subscribe `keyboard::listen()`, which yields ONLY
//! ignored-status events (pinned `iced/futures/src/keyboard.rs:7-19`) — a
//! focused button consumes Enter (`widget/src/button.rs:437-448`) and a
//! focused text input consumes text keys, so those never reach [`resolve_key`].
//!
//! Framework-verified gaps (pinned rev `7e82198`, all double-checked in
//! source — composition/ux own the call sites, noted here so the mapping
//! stays honest):
//!
//! * Space on a focused button does NOT activate it (iced `button` handles
//!   Enter only) — the press falls through to the subscription and fires
//!   `Play`. C++ GTK buttons consume Space. (`ui/sidebar.rs` claims
//!   Enter/Space row-button activation — the Space half is wrong for the
//!   pinned rev; flagged for the ux owner.)
//! * `toggler`/`checkbox` bind no keyboard keys at all — Enter/Space on a
//!   focused switch falls through (Enter plays the selected row).
//! * No automatic Tab handling exists for this app's widget set: no
//!   `on_tab` is set anywhere (`text_input` captures Tab only when
//!   `on_tab` is set — without it the arm returns early and Tab falls
//!   through to `listen()`, pinned `input.rs:2244-2259`), and neither
//!   `combo_box` nor `segmented_button` (which handle Tab themselves)
//!   is used — so Tab reaches `listen()`, and without the composition
//!   mapping, Tab does nothing.
//! * Focus ops cannot scope to the dialog subtree
//!   (`core/src/widget/operation/focusable.rs` offers only
//!   `focus(Id)`/`focus_next`/`focus_previous`/count), so Tab under a
//!   dialog also reaches base widgets. There is no runtime `unfocus` task
//!   wrapper either — the T12 widget-activation half (Enter on a
//!   pre-focused base widget) needs composition to `focus()` a dialog
//!   widget on open (dialog `Id`s are ux-owned).
//! * Plain arrows map to nothing (GTK list-box arrow navigation has no port
//!   path — no focusable list row in the pinned rev). C++ parity gap,
//!   ux/composition decision, not mapped here.
//!
//! [`keyboard::Event`]: cosmic::iced::keyboard::Event

use cosmic::iced::keyboard::{
    Event as KeyEvent, Key, Modifiers, key::Code, key::Named, key::Physical,
};

use crate::message::{Dialog, Message};

/// A resolved key press: the 9 accelerators + Enter-play + Esc-dismiss +
/// Tab traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// `<Ctrl>o` → `app.open`.
    Open,
    /// `<Ctrl>q` → `app.quit` (composition routes to `Message::Quit` —
    /// `message.rs:234`, arm at `app.rs:2551`, T14d wired).
    Quit,
    /// `space` → `app.play`.
    Play,
    /// `<Ctrl>p` → `app.pause`.
    Pause,
    /// `<Ctrl>s` → `app.stop`.
    Stop,
    /// `<Ctrl>Right` → `app.next`.
    Next,
    /// `<Ctrl>Left` → `app.prev`.
    Prev,
    /// `F1` → `app.help`.
    Help,
    /// `<Ctrl>comma` → `app.prefs`.
    Prefs,
    /// Plain Enter with no dialog open: play the selected playlist row
    /// (U11 — `PlaylistRowActivated`).
    EnterPlay(usize),
    /// Esc with a dialog open: dismiss it (`CloseDialog`).
    CloseDialog,
    /// Tab: move focus forward (`widget::operation::focus_next` task).
    FocusNext,
    /// Shift+Tab: move focus backward (`widget::operation::focus_previous`).
    FocusPrevious,
}

impl KeyAction {
    /// 1:1 composition mapping for the message-backed actions. `None` marks
    /// the three arms composition builds itself: `Quit` (explicit
    /// `self.update(Message::Quit)` → `window::close(main_window_id)`,
    /// mirroring the `gain_focus` present path — variant at
    /// `message.rs:234`, arm at `app.rs:2551`, T14d wired) and
    /// `FocusNext`/`FocusPrevious`
    /// (`widget::operation::{focus_next,focus_previous}` tasks — focus
    /// moves without any message).
    pub fn message(self) -> Option<Message> {
        match self {
            KeyAction::Open => Some(Message::OpenFilesDialog),
            KeyAction::Quit => None,
            KeyAction::Play => Some(Message::Play),
            KeyAction::Pause => Some(Message::Pause),
            KeyAction::Stop => Some(Message::Stop),
            KeyAction::Next => Some(Message::NextSong),
            KeyAction::Prev => Some(Message::PrevSong),
            KeyAction::Help => Some(Message::ShowHelp),
            KeyAction::Prefs => Some(Message::ShowDialog(Dialog::Preferences)),
            KeyAction::EnterPlay(index) => Some(Message::PlaylistRowActivated(index)),
            KeyAction::CloseDialog => Some(Message::CloseDialog),
            KeyAction::FocusNext | KeyAction::FocusPrevious => None,
        }
    }
}

/// One C++ accelerator: GTK spec + action verbatim, port target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accelerator {
    /// GTK accel spec verbatim (`window.cpp:1444-1452`).
    pub gtk: &'static str,
    /// C++ action name (`window.cpp:1419-1438`).
    pub action: &'static str,
    /// Resolved port action.
    pub id: KeyAction,
}

/// All 9 C++ accelerators in C++ table order (`window.cpp:1440-1455`).
/// The count pin (`ACCELERATORS.len() == 9`) guards the PLAN checklist
/// row "all 9 accelerators kept".
pub const ACCELERATORS: [Accelerator; 9] = [
    Accelerator {
        gtk: "<Ctrl>o",
        action: "app.open",
        id: KeyAction::Open,
    },
    Accelerator {
        gtk: "<Ctrl>q",
        action: "app.quit",
        id: KeyAction::Quit,
    },
    Accelerator {
        gtk: "space",
        action: "app.play",
        id: KeyAction::Play,
    },
    Accelerator {
        gtk: "<Ctrl>p",
        action: "app.pause",
        id: KeyAction::Pause,
    },
    Accelerator {
        gtk: "<Ctrl>s",
        action: "app.stop",
        id: KeyAction::Stop,
    },
    Accelerator {
        gtk: "<Ctrl>Right",
        action: "app.next",
        id: KeyAction::Next,
    },
    Accelerator {
        gtk: "<Ctrl>Left",
        action: "app.prev",
        id: KeyAction::Prev,
    },
    Accelerator {
        gtk: "F1",
        action: "app.help",
        id: KeyAction::Help,
    },
    Accelerator {
        gtk: "<Ctrl>comma",
        action: "app.prefs",
        id: KeyAction::Prefs,
    },
];

/// Exact Ctrl-only modifiers (GTK exact-match; `CapsLock`/other lock
/// modifiers deliberately untested — locks never block an accel).
fn ctrl_exact(modifiers: Modifiers) -> bool {
    modifiers.contains(Modifiers::CTRL)
        && !modifiers.intersects(Modifiers::SHIFT | Modifiers::ALT | Modifiers::LOGO)
}

/// No main modifiers at all (locks still ignored).
fn exact_none(modifiers: Modifiers) -> bool {
    !modifiers.intersects(Modifiers::SHIFT | Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO)
}

/// Layout-independent lowercase latin char for `key` (`text_input`'s own
/// matching call — pinned `iced/widget/src/text_input.rs:937`).
fn latin_lower(key: &Key, physical_key: Physical) -> Option<char> {
    key.to_latin(physical_key)
        .map(|c| c.to_lowercase().next().unwrap_or(c))
}

/// Resolve one keyboard event to a [`KeyAction`].
///
/// * Only `KeyPressed` maps (`KeyReleased`/`ModifiersChanged` → `None`);
///   repeats resolve like first presses (GTK accels repeat — parity).
/// * Esc resolves only with a dialog open (port addition — C++ closes
///   dialogs via window chrome; GTK4 binds no Esc on plain windows).
/// * Tab/Shift+Tab always resolve (no automatic Tab handling exists in
///   the pinned rev — the app must map them or Tab does nothing).
/// * With a dialog open, every other key is gated to `None` (focus gate —
///   see the module docs for the C++ verification and the two deltas).
/// * With no dialog open, the 9 accelerators resolve plus plain Enter on
///   a selection (`EnterPlay`; `None` with no selection).
///
/// Composition MUST feed this from `keyboard::listen()` (ignored-status
/// events only) — never from a raw event tap — or widget-consumed keys
/// (Enter on a focused button, Space in a text input) would double-fire.
pub fn resolve_key(
    event: &KeyEvent,
    dialog: Option<Dialog>,
    selected: Option<usize>,
) -> Option<KeyAction> {
    let KeyEvent::KeyPressed {
        key,
        physical_key,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    let modifiers = *modifiers;
    let physical_key = *physical_key;

    if *key == Key::Named(Named::Escape) {
        return (dialog.is_some() && exact_none(modifiers)).then_some(KeyAction::CloseDialog);
    }
    if *key == Key::Named(Named::Tab) {
        if modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO) {
            return None;
        }
        return Some(if modifiers.contains(Modifiers::SHIFT) {
            KeyAction::FocusPrevious
        } else {
            KeyAction::FocusNext
        });
    }
    // Focus gate: dialogs keep Esc + Tab only (see module docs).
    if dialog.is_some() {
        return None;
    }

    // Ctrl accelerators: exact modifiers (GTK parity).
    if ctrl_exact(modifiers) {
        if *key == Key::Named(Named::ArrowRight) {
            return Some(KeyAction::Next);
        }
        if *key == Key::Named(Named::ArrowLeft) {
            return Some(KeyAction::Prev);
        }
        if let Some(c) = latin_lower(key, physical_key) {
            return match c {
                'o' => Some(KeyAction::Open),
                'q' => Some(KeyAction::Quit),
                'p' => Some(KeyAction::Pause),
                's' => Some(KeyAction::Stop),
                ',' => Some(KeyAction::Prefs),
                _ => None,
            };
        }
        // Non-latin-layout Ctrl+, fallback: `to_latin` only maps A-Z/0-9
        // by physical position, so match the comma position directly
        // (documented superset over GTK's comma-keyval match).
        if physical_key == Physical::Code(Code::Comma) {
            return Some(KeyAction::Prefs);
        }
        return None;
    }

    // Space → Play. Shift is tolerated: `key` is `key_without_modifiers`
    // (Shift never changes it) and Shift changes no GTK space keyval
    // either. Ctrl/Alt/Logo block (GTK exact-match).
    if !modifiers.intersects(Modifiers::CTRL | Modifiers::ALT | Modifiers::LOGO)
        && latin_lower(key, physical_key) == Some(' ')
    {
        return Some(KeyAction::Play);
    }

    // F1 + Enter need exact-no-modifiers (GTK exact-match).
    if !exact_none(modifiers) {
        return None;
    }
    if *key == Key::Named(Named::F1) {
        return Some(KeyAction::Help);
    }
    if *key == Key::Named(Named::Enter) {
        return selected.map(KeyAction::EnterPlay);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::iced::keyboard::Location;

    /// Synthetic `KeyPressed` (location standard, no text, no repeat —
    /// struct-update the result for the repeat/location cases).
    fn pressed(key: Key, physical: Physical, modifiers: Modifiers) -> KeyEvent {
        KeyEvent::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }
    }

    fn char_pressed(c: &str, code: Code, modifiers: Modifiers) -> KeyEvent {
        pressed(Key::Character(c.into()), Physical::Code(code), modifiers)
    }

    fn named_pressed(named: Named, code: Code, modifiers: Modifiers) -> KeyEvent {
        pressed(Key::Named(named), Physical::Code(code), modifiers)
    }

    const CTRL: Modifiers = Modifiers::CTRL;
    const ALL_DIALOGS: [Dialog; 11] = [
        Dialog::Preferences,
        Dialog::MidiSetup,
        Dialog::About,
        Dialog::Help,
        Dialog::FileInfo,
        Dialog::JumpToBar,
        Dialog::Loop,
        Dialog::PlaylistOpen,
        Dialog::PlaylistSave,
        Dialog::SoundFontBrowse,
        Dialog::SaveLyrics,
    ];

    /// The 9 accelerators as synthetic events (US layout), in C++ order.
    fn accelerator_events() -> [(KeyEvent, KeyAction); 9] {
        [
            (char_pressed("o", Code::KeyO, CTRL), KeyAction::Open),
            (char_pressed("q", Code::KeyQ, CTRL), KeyAction::Quit),
            (
                char_pressed(" ", Code::Space, Modifiers::empty()),
                KeyAction::Play,
            ),
            (char_pressed("p", Code::KeyP, CTRL), KeyAction::Pause),
            (char_pressed("s", Code::KeyS, CTRL), KeyAction::Stop),
            (
                named_pressed(Named::ArrowRight, Code::ArrowRight, CTRL),
                KeyAction::Next,
            ),
            (
                named_pressed(Named::ArrowLeft, Code::ArrowLeft, CTRL),
                KeyAction::Prev,
            ),
            (
                named_pressed(Named::F1, Code::F1, Modifiers::empty()),
                KeyAction::Help,
            ),
            (char_pressed(",", Code::Comma, CTRL), KeyAction::Prefs),
        ]
    }

    #[test]
    fn accelerator_table_pins_all_9_gtk_specs_in_cpp_order() {
        // Count pin (PLAN checklist "all 9 accelerators kept").
        assert_eq!(ACCELERATORS.len(), 9);
        let pairs: Vec<(&str, &str)> = ACCELERATORS
            .iter()
            .map(|acc| (acc.gtk, acc.action))
            .collect();
        assert_eq!(
            pairs,
            [
                ("<Ctrl>o", "app.open"),
                ("<Ctrl>q", "app.quit"),
                ("space", "app.play"),
                ("<Ctrl>p", "app.pause"),
                ("<Ctrl>s", "app.stop"),
                ("<Ctrl>Right", "app.next"),
                ("<Ctrl>Left", "app.prev"),
                ("F1", "app.help"),
                ("<Ctrl>comma", "app.prefs"),
            ]
        );
    }

    #[test]
    fn all_9_accelerators_resolve_without_dialog() {
        for (event, expected) in accelerator_events() {
            assert_eq!(resolve_key(&event, None, None), Some(expected), "{event:?}");
        }
    }

    #[test]
    fn accelerator_table_targets_match_resolver() {
        // The const table and `resolve_key` cannot drift: every table row
        // resolves to its own `id`.
        for (event, _) in accelerator_events() {
            let resolved = resolve_key(&event, None, None).unwrap();
            assert!(
                ACCELERATORS.iter().any(|acc| acc.id == resolved),
                "{resolved:?} missing from ACCELERATORS"
            );
        }
        // And every table id is reachable.
        for acc in ACCELERATORS {
            assert!(
                accelerator_events().iter().any(|(_, id)| *id == acc.id),
                "{acc:?} unreachable"
            );
        }
    }

    #[test]
    fn releases_and_modifier_changes_never_resolve() {
        let released = KeyEvent::KeyReleased {
            key: Key::Character("o".into()),
            modified_key: Key::Character("o".into()),
            physical_key: Physical::Code(Code::KeyO),
            location: Location::Standard,
            modifiers: CTRL,
        };
        assert_eq!(resolve_key(&released, None, None), None);
        assert_eq!(
            resolve_key(&KeyEvent::ModifiersChanged(CTRL), None, None),
            None
        );
    }

    #[test]
    fn dialog_gates_all_9_accelerators_and_enter() {
        let enter = named_pressed(Named::Enter, Code::Enter, Modifiers::empty());
        for dialog in ALL_DIALOGS {
            for (event, _) in accelerator_events() {
                assert_eq!(
                    resolve_key(&event, Some(dialog), Some(0)),
                    None,
                    "{event:?} under {dialog:?}"
                );
            }
            assert_eq!(
                resolve_key(&enter, Some(dialog), Some(0)),
                None,
                "Enter under {dialog:?}"
            );
        }
    }

    #[test]
    fn escape_closes_only_under_dialog_with_exact_mods() {
        let esc = named_pressed(Named::Escape, Code::Escape, Modifiers::empty());
        for dialog in ALL_DIALOGS {
            assert_eq!(
                resolve_key(&esc, Some(dialog), None),
                Some(KeyAction::CloseDialog),
                "{dialog:?}"
            );
        }
        // No dialog: C++ binds no Esc on the main window.
        assert_eq!(resolve_key(&esc, None, None), None);
        // Modified Esc is not a dismiss key.
        for extra in [Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT] {
            let modified = named_pressed(Named::Escape, Code::Escape, extra);
            assert_eq!(resolve_key(&modified, Some(Dialog::Help), None), None);
        }
    }

    #[test]
    fn tab_traversal_works_with_and_without_dialog() {
        let tab = named_pressed(Named::Tab, Code::Tab, Modifiers::empty());
        let backtab = named_pressed(Named::Tab, Code::Tab, Modifiers::SHIFT);
        for dialog in [None, Some(Dialog::Loop), Some(Dialog::Preferences)] {
            assert_eq!(
                resolve_key(&tab, dialog, None),
                Some(KeyAction::FocusNext),
                "{dialog:?}"
            );
            assert_eq!(
                resolve_key(&backtab, dialog, None),
                Some(KeyAction::FocusPrevious),
                "{dialog:?}"
            );
        }
        // Ctrl/Alt/Logo+Tab are unmapped (no C++ behavior to keep).
        for extra in [Modifiers::CTRL, Modifiers::ALT, Modifiers::LOGO] {
            let modified = named_pressed(Named::Tab, Code::Tab, extra);
            assert_eq!(resolve_key(&modified, None, None), None);
        }
    }

    #[test]
    fn enter_plays_selected_row_only_without_dialog() {
        let enter = named_pressed(Named::Enter, Code::Enter, Modifiers::empty());
        assert_eq!(
            resolve_key(&enter, None, Some(3)),
            Some(KeyAction::EnterPlay(3))
        );
        // No selection: nothing to play (C++ needs a focused row too).
        assert_eq!(resolve_key(&enter, None, None), None);
        // Gated under dialogs (focus gate).
        assert_eq!(resolve_key(&enter, Some(Dialog::JumpToBar), Some(3)), None);
        // Modified Enter is unmapped.
        for extra in [Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT] {
            let modified = named_pressed(Named::Enter, Code::Enter, extra);
            assert_eq!(resolve_key(&modified, None, Some(3)), None);
        }
        // Numpad Enter carries the same `Named::Enter` (location differs) —
        // C++ activates rows on both keypad and main Enter.
        let KeyEvent::KeyPressed {
            key,
            modified_key,
            physical_key,
            modifiers,
            text,
            ..
        } = named_pressed(Named::Enter, Code::NumpadEnter, Modifiers::empty())
        else {
            panic!("expected KeyPressed");
        };
        let numpad_enter = KeyEvent::KeyPressed {
            key,
            modified_key,
            physical_key,
            location: Location::Numpad,
            modifiers,
            text,
            repeat: false,
        };
        assert_eq!(
            resolve_key(&numpad_enter, None, Some(1)),
            Some(KeyAction::EnterPlay(1))
        );
    }

    #[test]
    fn space_matches_gtk_shift_tolerance_and_exactness() {
        let space = || char_pressed(" ", Code::Space, Modifiers::empty());
        assert_eq!(resolve_key(&space(), None, None), Some(KeyAction::Play));
        // Shift changes neither the iced `key` nor the GTK space keyval.
        let shift_space = char_pressed(" ", Code::Space, Modifiers::SHIFT);
        assert_eq!(resolve_key(&shift_space, None, None), Some(KeyAction::Play));
        // Ctrl/Alt/Logo block (GTK exact-match).
        for extra in [Modifiers::CTRL, Modifiers::ALT, Modifiers::LOGO] {
            let modified = char_pressed(" ", Code::Space, extra);
            assert_eq!(resolve_key(&modified, None, None), None);
        }
    }

    #[test]
    fn ctrl_letters_need_exact_modifiers_but_ignore_locks() {
        // Extra Shift/Alt/Logo block (GTK exact-match — `key` is
        // `key_without_modifiers`, so Shift would otherwise slip through).
        for extra in [Modifiers::SHIFT, Modifiers::ALT, Modifiers::LOGO] {
            let modified = char_pressed("o", Code::KeyO, CTRL | extra);
            assert_eq!(resolve_key(&modified, None, None), None);
        }
        // Lock modifiers never block (GTK ignores locks in accel matching).
        let caps = char_pressed("o", Code::KeyO, CTRL | Modifiers::CAPS_LOCK);
        assert_eq!(resolve_key(&caps, None, None), Some(KeyAction::Open));
        // Unrelated Ctrl+letters stay dead.
        for (c, code) in [("c", Code::KeyC), ("a", Code::KeyA), ("z", Code::KeyZ)] {
            let event = char_pressed(c, code, CTRL);
            assert_eq!(resolve_key(&event, None, None), None);
        }
    }

    #[test]
    fn ctrl_letter_matching_case_folds_like_gtk() {
        // CapsLock path: the char arrives uppercase, locks are ignored.
        let upper = char_pressed("O", Code::KeyO, CTRL);
        assert_eq!(resolve_key(&upper, None, None), Some(KeyAction::Open));
        let upper_s = char_pressed("S", Code::KeyS, CTRL);
        assert_eq!(resolve_key(&upper_s, None, None), Some(KeyAction::Stop));
    }

    #[test]
    fn non_latin_layouts_match_by_physical_position() {
        // Russian layout: the O-position key yields Cyrillic sha — `to_latin`
        // falls back to the physical code (documented superset over GTK).
        let sha = char_pressed("щ", Code::KeyO, CTRL);
        assert_eq!(resolve_key(&sha, None, None), Some(KeyAction::Open));
        // Ctrl+, by physical comma position (Russian ef at that spot) —
        // `to_latin` maps no punctuation physically, hence the fallback.
        let ef = char_pressed("б", Code::Comma, CTRL);
        assert_eq!(resolve_key(&ef, None, None), Some(KeyAction::Prefs));
        // Same char on the WRONG physical key stays dead.
        let ef_wrong_key = char_pressed("б", Code::KeyB, CTRL);
        assert_eq!(resolve_key(&ef_wrong_key, None, None), None);
    }

    #[test]
    fn ctrl_arrows_are_exact_and_plain_arrows_dead() {
        let left = named_pressed(Named::ArrowLeft, Code::ArrowLeft, CTRL);
        let right = named_pressed(Named::ArrowRight, Code::ArrowRight, CTRL);
        assert_eq!(resolve_key(&left, None, None), Some(KeyAction::Prev));
        assert_eq!(resolve_key(&right, None, None), Some(KeyAction::Next));
        // Extra Shift blocks (GTK exact-match — no text-selection gesture
        // reaches the subscription anyway, text inputs capture it).
        let shift_left = named_pressed(Named::ArrowLeft, Code::ArrowLeft, CTRL | Modifiers::SHIFT);
        assert_eq!(resolve_key(&shift_left, None, None), None);
        // Plain arrows are unmapped (C++ binds none; list-box arrow
        // navigation has no port path — documented gap).
        for (named, code) in [
            (Named::ArrowLeft, Code::ArrowLeft),
            (Named::ArrowRight, Code::ArrowRight),
            (Named::ArrowUp, Code::ArrowUp),
            (Named::ArrowDown, Code::ArrowDown),
        ] {
            let plain = named_pressed(named, code, Modifiers::empty());
            assert_eq!(resolve_key(&plain, None, Some(0)), None);
        }
    }

    #[test]
    fn f1_needs_exact_no_modifiers() {
        let f1 = named_pressed(Named::F1, Code::F1, Modifiers::empty());
        assert_eq!(resolve_key(&f1, None, None), Some(KeyAction::Help));
        for extra in [Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT] {
            let modified = named_pressed(Named::F1, Code::F1, extra);
            assert_eq!(resolve_key(&modified, None, None), None);
        }
        // Other function keys stay dead.
        let f2 = named_pressed(Named::F2, Code::F2, Modifiers::empty());
        assert_eq!(resolve_key(&f2, None, None), None);
    }

    #[test]
    fn plain_letters_digits_and_unmapped_keys_stay_dead() {
        for (c, code) in [
            ("o", Code::KeyO),
            ("q", Code::KeyQ),
            ("a", Code::KeyA),
            ("1", Code::Digit1),
            (",", Code::Comma),
        ] {
            let plain = char_pressed(c, code, Modifiers::empty());
            assert_eq!(resolve_key(&plain, None, None), None, "{c}");
        }
        // Home/End/Delete/Insert/Backspace: no C++ bindings, unmapped.
        for (named, code) in [
            (Named::Home, Code::Home),
            (Named::End, Code::End),
            (Named::Delete, Code::Delete),
            (Named::Insert, Code::Insert),
            (Named::Backspace, Code::Backspace),
        ] {
            let key = named_pressed(named, code, Modifiers::empty());
            assert_eq!(resolve_key(&key, None, None), None);
        }
    }

    #[test]
    fn repeats_resolve_like_first_presses() {
        // GTK accels fire on key repeat — hold-to-repeat is parity, not a bug.
        let KeyEvent::KeyPressed {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
            text,
            ..
        } = char_pressed(" ", Code::Space, Modifiers::empty())
        else {
            panic!("expected KeyPressed");
        };
        let repeat = KeyEvent::KeyPressed {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
            text,
            repeat: true,
        };
        assert_eq!(resolve_key(&repeat, None, None), Some(KeyAction::Play));
    }

    #[test]
    fn key_action_message_mapping_pins_all_13() {
        use KeyAction as A;
        let cases = [
            (A::Open, Some(Message::OpenFilesDialog)),
            (A::Quit, None), // `Message::Quit` exists but routes via the explicit arm (composition-owned)
            (A::Play, Some(Message::Play)),
            (A::Pause, Some(Message::Pause)),
            (A::Stop, Some(Message::Stop)),
            (A::Next, Some(Message::NextSong)),
            (A::Prev, Some(Message::PrevSong)),
            (A::Help, Some(Message::ShowHelp)),
            (A::Prefs, Some(Message::ShowDialog(Dialog::Preferences))),
            (A::EnterPlay(7), Some(Message::PlaylistRowActivated(7))),
            (A::CloseDialog, Some(Message::CloseDialog)),
            (A::FocusNext, None),     // `focus_next` task, no message
            (A::FocusPrevious, None), // `focus_previous` task, no message
        ];
        for (action, expected) in cases {
            match (action.message(), expected) {
                (None, None) => {}
                (Some(got), Some(want)) => assert!(
                    std::mem::discriminant(&got) == std::mem::discriminant(&want)
                        && format!("{got:?}") == format!("{want:?}"),
                    "{action:?} -> {got:?}, want {want:?}"
                ),
                (got, want) => panic!("{action:?} -> {got:?}, want {want:?}"),
            }
        }
    }
}
