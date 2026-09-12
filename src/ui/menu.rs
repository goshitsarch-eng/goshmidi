//! 23-item app menu: the C++ hamburger `GMenu` (ux §3.2, U1b).
//!
//! Real T14b implementation of the frozen [`MenuViewModel`] contract:
//! owned `vm` in, one [`cosmic::Element`] out — a single-root
//! [`menu::bar`][cosmic::widget::menu::bar] (hamburger icon root, faithful to
//! the C++ `GtkMenuButton(open-menu-symbolic)`, `window.cpp:1583-1584`) whose
//! dropdown holds all 4 C++ sections / 23 entries in C++ order, separated by
//! dividers where C++ appends sections (`window.cpp:1613-1616`). (Arch
//! composes this bar into `header_end` next — see the composition contract
//! below.)
//!
//! Inventory (labels verbatim C++, `window.cpp:1587-1612`; effects are the
//! existing `Message` variants — every arm exists arch-side, Quit included
//! (`Message::Quit`, T14d composition — the T14b VM callback is retired):
//!
//! | section | label | C++ action → handler | message |
//! |---|---|---|
//! | File | `Open…` | `app.open` → D1 (`window.cpp:1373-1374`) | `OpenFilesDialog` |
//! | File | `File information` | `app.info` → D6 (`window.cpp:1403-1404`) | `ShowFileInfo` |
//! | File | `Load song settings` | `app.load-settings` (`window.cpp:1407-1408`) | `SongSettingsLoad` |
//! | File | `Save song settings` | `app.save-settings` (`window.cpp:1405-1406`) | `SongSettingsSave` |
//! | File | `Search MIDI files` | `app.search` → midisite (`window.cpp:1409-1410`) | `OpenUrl(SEARCH_URL)` |
//! | File | `Quit` | `app.quit` → `g_application_quit` (`window.cpp:1375-1376`) | `Quit` |
//! | Playback | `Play` | `app.play` (`window.cpp:1377-1378`) | `Play` |
//! | Playback | `Pause` | `app.pause` (`window.cpp:1379-1380`) | `Pause` |
//! | Playback | `Stop` | `app.stop` (`window.cpp:1381-1382`) | `Stop` |
//! | Playback | `Previous` | `app.prev` (`window.cpp:1385-1386`) | `PrevSong` |
//! | Playback | `Next` | `app.next` (`window.cpp:1383-1384`) | `NextSong` |
//! | Playback | `Rewind 1 bar` | `app.rew` (`window.cpp:1389-1390`) | `BeatBackward` |
//! | Playback | `Forward 1 bar` | `app.fwd` (`window.cpp:1387-1388`) | `BeatForward` |
//! | Playback | `Jump to bar…` | `app.jump` → D7 (`window.cpp:1391-1392`) | `ShowDialog(JumpToBar)` |
//! | Playback | `Loop…` | `app.loop` → D8 (`window.cpp:1393-1394`) | `LoopToggled(true)` (U3) |
//! | Repeat | `Repeat off` | `app.repeat-off` (`window.cpp:2140-2152`) | `RepeatModeChanged(Off)` |
//! | Repeat | `Repeat song` | `app.repeat-song` | `RepeatModeChanged(Song)` |
//! | Repeat | `Repeat playlist` | `app.repeat-list` | `RepeatModeChanged(Playlist)` |
//! | Help | `MIDI Setup` | `app.midi` → D3 (`window.cpp:1397-1398`) | `ShowDialog(MidiSetup)` |
//! | Help | `Preferences` | `app.prefs` → D2 (`window.cpp:1395-1396`) | `ShowDialog(Preferences)` |
//! | Help | `Help` | `app.help` → D5 (`window.cpp:1401-1402`) | `ShowHelp` |
//! | Help | `Original dmidiplayer` | `app.website` → sourceforge (`window.cpp:1411-1413`) | `OpenUrl(WEBSITE_URL)` |
//! | Help | `About Gosh MIDI Player` | `app.about` → D4 (`window.cpp:1399-1400`) | `ShowAbout` |
//!
//! Contract notes (implemented exactly):
//!
//! * The T13 `song_settings` stubs landed HERE and are retired (T14d):
//!   the File entries `Load song settings` / `Save song settings` are the
//!   stubs' menu placement (same labels, same messages, same always-live
//!   rule — the arms no-op on an empty song, `window.cpp:633-635,663-665`).
//! * Loop takes the U3 path: menu Loop emits `LoopToggled(true)` (no new
//!   variant — forces the toggle ON, then opens D8), never a bare dialog
//!   trigger. Jump emits `ShowDialog(JumpToBar)` — the arm gates no-song
//!   via `jump_allowed` (C++ `window.cpp:777-778` early-return parity).
//! * Repeat items are plain like C++ (`g_menu_append`, no radio state —
//!   `window.cpp:1604-1606`); the VM carries no repeat mode.
//! * Keybind hints mirror the libcosmic [`KeyBind`][cosmic::widget::menu::KeyBind]
//!   `Display` (`"Ctrl + O"`, `"Space"`, `"Ctrl + ArrowRight"`, …) so the
//!   hints match what `menu::items` would render for arch's key-bind map.
//!   Exactly the 9 C++ accelerators show (`window.cpp:1444-1452`); the other
//!   14 entries carry no hint (C++ parity — no new shortcuts are invented
//!   here; ux §14 tracks the keyboard-reachability proposal).
//! * The entries are hand-rolled over the public menu primitives
//!   (`menu_button`, `MenuTree`, `divider::horizontal::light`) instead of
//!   `menu::items`: that helper needs an `A: MenuAction` whose
//!   `message(&self)` cannot see the VM (a T14b constraint — Quit rode
//!   `vm.quit` then; T14d retired the callback for `Message::Quit` but
//!   keeps the hand-rolled layout). The entry layout (label + spacer +
//!   dim keybind) follows `entry_tree` (`menu_tree.rs:284-350`); the
//!   keybind ink reuses our theme-derived
//!   [`dim_label][super::theme::dim_label] (55% dim — same idea, own token).
//!
//! Composition contract (T14d wired — was "arch wires next" in T14b):
//!
//! * placement: `header_end` (hamburger parity — C++ packs the menu button
//!   at the header end, `window.cpp:1618`);
//! * Quit is `Message::Quit` (the `window::close` arm, arch-owned — like
//!   the menu example's `WindowClose`);
//! * keyboard: the 9-accelerator subscription over the same `Message`s
//!   (arch T14a logic, T14d wiring) + Tab focus traversal, so the hints
//!   on the entries are live (the remaining 14 actions stay menu-only —
//!   no invented bindings, lead ruling D22; ux §14 tracks G3).
//!
//! A11y: the icon-only root carries name + description (`Menu` — the visible
//! label of a text root would double as its name; the icon has none, so the
//! explicit name is load-bearing); every entry carries its label as the a11y
//! name plus a `Shortcut …` description where a keybind shows. The pinned
//! menu has NO keyboard navigation (`menu_inner.rs`/`menu_bar.rs` handle no
//! keyboard events — every entry also needs its button/accelerator path;
//! tracked in ux §14 like D13).
//!
//! [`MenuViewModel`]: super::MenuViewModel

use cosmic::Element;
use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit};
use cosmic::widget::menu::{ItemHeight, ItemWidth};
use cosmic::widget::{button, icon, menu, space, text};

use super::MenuViewModel;
use super::theme;
use crate::message::{Dialog, Message, RepeatMode};

/// `app.search` target (`window.cpp:1410`, verbatim — no trailing slash).
pub const SEARCH_URL: &str = "https://midisite.co.uk";
/// `app.website` target (`window.cpp:1412-1413`, verbatim — trailing slash).
pub const WEBSITE_URL: &str = "https://sourceforge.net/p/dmidiplayer/";
/// Hamburger root icon (C++ `open-menu-symbolic`, `window.cpp:1584`).
pub const ROOT_ICON: &str = "open-menu-symbolic";
/// Hamburger root a11y name (load-bearing — the icon root has no visible
/// label to double as its name, unlike a text root).
pub const ROOT_NAME: &str = "Menu";
/// Hamburger root a11y description.
pub const ROOT_DESCRIPTION: &str = "Application menu";

/// The 23 C++ `app.*` actions in menu order (File 6, Playback 9, Repeat 3,
/// Help 5 — `window.cpp:1587-1612`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    FileInfo,
    LoadSettings,
    SaveSettings,
    Search,
    Quit,
    Play,
    Pause,
    Stop,
    Prev,
    Next,
    Rew,
    Fwd,
    Jump,
    Loop,
    RepeatOff,
    RepeatSong,
    RepeatList,
    MidiSetup,
    Prefs,
    Help,
    Website,
    About,
}

impl MenuAction {
    /// All 23 actions in menu order.
    pub const ALL: [MenuAction; 23] = [
        MenuAction::Open,
        MenuAction::FileInfo,
        MenuAction::LoadSettings,
        MenuAction::SaveSettings,
        MenuAction::Search,
        MenuAction::Quit,
        MenuAction::Play,
        MenuAction::Pause,
        MenuAction::Stop,
        MenuAction::Prev,
        MenuAction::Next,
        MenuAction::Rew,
        MenuAction::Fwd,
        MenuAction::Jump,
        MenuAction::Loop,
        MenuAction::RepeatOff,
        MenuAction::RepeatSong,
        MenuAction::RepeatList,
        MenuAction::MidiSetup,
        MenuAction::Prefs,
        MenuAction::Help,
        MenuAction::Website,
        MenuAction::About,
    ];

    /// Verbatim C++ menu label (`window.cpp:1587-1612`).
    pub fn label(self) -> &'static str {
        match self {
            MenuAction::Open => "Open…",
            MenuAction::FileInfo => "File information",
            MenuAction::LoadSettings => "Load song settings",
            MenuAction::SaveSettings => "Save song settings",
            MenuAction::Search => "Search MIDI files",
            MenuAction::Quit => "Quit",
            MenuAction::Play => "Play",
            MenuAction::Pause => "Pause",
            MenuAction::Stop => "Stop",
            MenuAction::Prev => "Previous",
            MenuAction::Next => "Next",
            MenuAction::Rew => "Rewind 1 bar",
            MenuAction::Fwd => "Forward 1 bar",
            MenuAction::Jump => "Jump to bar…",
            MenuAction::Loop => "Loop…",
            MenuAction::RepeatOff => "Repeat off",
            MenuAction::RepeatSong => "Repeat song",
            MenuAction::RepeatList => "Repeat playlist",
            MenuAction::MidiSetup => "MIDI Setup",
            MenuAction::Prefs => "Preferences",
            MenuAction::Help => "Help",
            MenuAction::Website => "Original dmidiplayer",
            MenuAction::About => "About Gosh MIDI Player",
        }
    }

    /// C++ `g_menu_append` line for this entry (`window.cpp:1587-1612`).
    pub fn cite(self) -> &'static str {
        match self {
            MenuAction::Open => "window.cpp:1587",
            MenuAction::FileInfo => "window.cpp:1588",
            MenuAction::LoadSettings => "window.cpp:1589",
            MenuAction::SaveSettings => "window.cpp:1590",
            MenuAction::Search => "window.cpp:1591",
            MenuAction::Quit => "window.cpp:1592",
            MenuAction::Play => "window.cpp:1594",
            MenuAction::Pause => "window.cpp:1595",
            MenuAction::Stop => "window.cpp:1596",
            MenuAction::Prev => "window.cpp:1597",
            MenuAction::Next => "window.cpp:1598",
            MenuAction::Rew => "window.cpp:1599",
            MenuAction::Fwd => "window.cpp:1600",
            MenuAction::Jump => "window.cpp:1601",
            MenuAction::Loop => "window.cpp:1602",
            MenuAction::RepeatOff => "window.cpp:1604",
            MenuAction::RepeatSong => "window.cpp:1605",
            MenuAction::RepeatList => "window.cpp:1606",
            MenuAction::MidiSetup => "window.cpp:1608",
            MenuAction::Prefs => "window.cpp:1609",
            MenuAction::Help => "window.cpp:1610",
            MenuAction::Website => "window.cpp:1611",
            MenuAction::About => "window.cpp:1612",
        }
    }

    /// Accelerator hint for the 9 C++ accelerators (`window.cpp:1444-1452`),
    /// in libcosmic `KeyBind` `Display` form (`"Ctrl + O"`, `"Space"`, … —
    /// `key_bind.rs:143-155`); `None` for the other 14 (C++ parity).
    pub fn keybind(self) -> Option<&'static str> {
        match self {
            MenuAction::Open => Some("Ctrl + O"),
            MenuAction::Quit => Some("Ctrl + Q"),
            MenuAction::Play => Some("Space"),
            MenuAction::Pause => Some("Ctrl + P"),
            MenuAction::Stop => Some("Ctrl + S"),
            MenuAction::Next => Some("Ctrl + ArrowRight"),
            MenuAction::Prev => Some("Ctrl + ArrowLeft"),
            MenuAction::Help => Some("F1"),
            MenuAction::Prefs => Some("Ctrl + ,"),
            _ => None,
        }
    }

    /// Effect for this entry: the existing `Message` variant (all 23 arms
    /// exist arch-side — Quit is `Message::Quit` since T14d, which retired
    /// the T14b `vm.quit` callback).
    pub fn message(self) -> Message {
        match self {
            MenuAction::Open => Message::OpenFilesDialog,
            MenuAction::FileInfo => Message::ShowFileInfo,
            MenuAction::LoadSettings => Message::SongSettingsLoad,
            MenuAction::SaveSettings => Message::SongSettingsSave,
            MenuAction::Search => Message::OpenUrl(SEARCH_URL.to_string()),
            MenuAction::Quit => Message::Quit,
            MenuAction::Play => Message::Play,
            MenuAction::Pause => Message::Pause,
            MenuAction::Stop => Message::Stop,
            MenuAction::Prev => Message::PrevSong,
            MenuAction::Next => Message::NextSong,
            MenuAction::Rew => Message::BeatBackward,
            MenuAction::Fwd => Message::BeatForward,
            MenuAction::Jump => Message::ShowDialog(Dialog::JumpToBar),
            // U3: menu Loop forces the toggle ON, then opens D8 — the same
            // `LoopToggled(true)` the header toggle emits (no new variant).
            MenuAction::Loop => Message::LoopToggled(true),
            MenuAction::RepeatOff => Message::RepeatModeChanged(RepeatMode::Off),
            MenuAction::RepeatSong => Message::RepeatModeChanged(RepeatMode::Song),
            MenuAction::RepeatList => Message::RepeatModeChanged(RepeatMode::Playlist),
            MenuAction::MidiSetup => Message::ShowDialog(Dialog::MidiSetup),
            MenuAction::Prefs => Message::ShowDialog(Dialog::Preferences),
            MenuAction::Help => Message::ShowHelp,
            MenuAction::Website => Message::OpenUrl(WEBSITE_URL.to_string()),
            MenuAction::About => Message::ShowAbout,
        }
    }
}

/// File section: 6 entries (`window.cpp:1586-1592`).
const FILE_ACTIONS: [MenuAction; 6] = [
    MenuAction::Open,
    MenuAction::FileInfo,
    MenuAction::LoadSettings,
    MenuAction::SaveSettings,
    MenuAction::Search,
    MenuAction::Quit,
];
/// Playback section: 9 entries (`window.cpp:1593-1602`).
const PLAYBACK_ACTIONS: [MenuAction; 9] = [
    MenuAction::Play,
    MenuAction::Pause,
    MenuAction::Stop,
    MenuAction::Prev,
    MenuAction::Next,
    MenuAction::Rew,
    MenuAction::Fwd,
    MenuAction::Jump,
    MenuAction::Loop,
];
/// Repeat section: 3 entries (`window.cpp:1603-1606`).
const REPEAT_ACTIONS: [MenuAction; 3] = [
    MenuAction::RepeatOff,
    MenuAction::RepeatSong,
    MenuAction::RepeatList,
];
/// Help section (untitled in C++ — `nullptr`, `window.cpp:1616`): 5 entries
/// (`window.cpp:1607-1612`).
const HELP_ACTIONS: [MenuAction; 5] = [
    MenuAction::MidiSetup,
    MenuAction::Prefs,
    MenuAction::Help,
    MenuAction::Website,
    MenuAction::About,
];

/// One C++ `GMenu` section: the section title C++ appends (`window.cpp:1613-1616`
/// — `None` for the untitled help section) + its entries in order.
pub struct MenuSection {
    /// Section title (`File`/`Playback`/`Repeat`; `None` = untitled help).
    pub title: Option<&'static str>,
    /// Entries in C++ order.
    pub actions: &'static [MenuAction],
}

/// The 4 C++ sections in order (`window.cpp:1613-1616`).
pub const SECTIONS: [MenuSection; 4] = [
    MenuSection {
        title: Some("File"),
        actions: &FILE_ACTIONS,
    },
    MenuSection {
        title: Some("Playback"),
        actions: &PLAYBACK_ACTIONS,
    },
    MenuSection {
        title: Some("Repeat"),
        actions: &REPEAT_ACTIONS,
    },
    MenuSection {
        title: None,
        actions: &HELP_ACTIONS,
    },
];

/// One menu entry: label + spacer + dim keybind hint (the `entry_tree`
/// layout, `menu_tree.rs:284-350`), `MenuItem` class via [`menu_button`][menu::menu_button].
/// The label doubles as the a11y name; entries with a keybind add a
/// `Shortcut …` description.
fn menu_entry(action: MenuAction) -> menu::Tree<Message> {
    let label = action.label();
    let ellipsize = Ellipsize::Middle(EllipsizeHeightLimit::Lines(1));
    let items: Vec<Element<'static, Message>> = vec![
        text(label).ellipsize(ellipsize).into(),
        space::horizontal().into(),
        text(action.keybind().unwrap_or_default())
            .class(theme::text_class(theme::dim_label))
            .ellipsize(ellipsize)
            .into(),
    ];
    let mut entry = menu::menu_button(items).on_press(action.message());
    entry = entry.name(label);
    if let Some(keybind) = action.keybind() {
        entry = entry.description(format!("Shortcut {keybind}"));
    }
    menu::Tree::from(Element::from(entry))
}

/// Hamburger root: the C++ `open-menu-symbolic` icon in a `MenuRoot`-class
/// button (the `menu_root` shape, `menu_tree.rs:270-282`, with an icon for
/// the text). No `on_press` — the bar toggles the dropdown itself (menu
/// example parity); the explicit name is load-bearing (icon-only root).
fn menu_root() -> Element<'static, Message> {
    button::custom(icon::from_name(ROOT_ICON).size(16).icon())
        .padding([4, 12])
        .class(cosmic::theme::Button::MenuRoot)
        .name(ROOT_NAME)
        .description(ROOT_DESCRIPTION)
        .into()
}

/// 23-item app menu bar. `vm` is the whole contract — no other `App`
/// access. By value (see [`MenuViewModel`]); everything rendered is
/// inventory data, so the `Element` is `'static`.
pub fn view(vm: MenuViewModel) -> Element<'static, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let MenuViewModel {} = vm;

    // Sections in C++ order, divided where C++ appends sections
    // (`window.cpp:1613-1616`) — the light divider is what `menu::items`
    // renders for `MenuItem::Divider` (`menu_tree.rs:444-448`).
    let mut children: Vec<menu::Tree<Message>> = Vec::with_capacity(26);
    for (index, section) in SECTIONS.iter().enumerate() {
        if index > 0 {
            children.push(menu::Tree::from(Element::from(
                cosmic::widget::divider::horizontal::light(),
            )));
        }
        let _ = section.title;
        for action in section.actions {
            children.push(menu_entry(*action));
        }
    }

    menu::bar(vec![menu::Tree::with_children(
        cosmic::widget::RcElementWrapper::new(menu_root()),
        children,
    )])
    .item_height(ItemHeight::Dynamic(40))
    .item_width(ItemWidth::Uniform(240))
    .spacing(4.0)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_is_23_entries_in_cpp_section_order() {
        let flattened: Vec<MenuAction> = SECTIONS
            .iter()
            .flat_map(|section| section.actions.iter().copied())
            .collect();
        assert_eq!(flattened, MenuAction::ALL);
        assert_eq!(MenuAction::ALL.len(), 23);
        assert_eq!(SECTIONS[0].actions.len(), 6);
        assert_eq!(SECTIONS[1].actions.len(), 9);
        assert_eq!(SECTIONS[2].actions.len(), 3);
        assert_eq!(SECTIONS[3].actions.len(), 5);
        assert_eq!(SECTIONS[0].title, Some("File"));
        assert_eq!(SECTIONS[1].title, Some("Playback"));
        assert_eq!(SECTIONS[2].title, Some("Repeat"));
        assert_eq!(SECTIONS[3].title, None);
    }

    #[test]
    fn labels_are_verbatim_cpp_menu_entries() {
        let labels: Vec<&str> = MenuAction::ALL
            .iter()
            .map(|action| action.label())
            .collect();
        assert_eq!(
            labels,
            [
                "Open…",
                "File information",
                "Load song settings",
                "Save song settings",
                "Search MIDI files",
                "Quit",
                "Play",
                "Pause",
                "Stop",
                "Previous",
                "Next",
                "Rewind 1 bar",
                "Forward 1 bar",
                "Jump to bar…",
                "Loop…",
                "Repeat off",
                "Repeat song",
                "Repeat playlist",
                "MIDI Setup",
                "Preferences",
                "Help",
                "Original dmidiplayer",
                "About Gosh MIDI Player",
            ]
        );
    }

    #[test]
    fn cites_point_at_the_cpp_menu_lines() {
        // Spot-check the section boundaries (every cite is `window.cpp:NNNN`
        // inside `1587..=1612`; the full table is pinned by review).
        assert_eq!(MenuAction::Open.cite(), "window.cpp:1587");
        assert_eq!(MenuAction::Quit.cite(), "window.cpp:1592");
        assert_eq!(MenuAction::Play.cite(), "window.cpp:1594");
        assert_eq!(MenuAction::Loop.cite(), "window.cpp:1602");
        assert_eq!(MenuAction::RepeatOff.cite(), "window.cpp:1604");
        assert_eq!(MenuAction::MidiSetup.cite(), "window.cpp:1608");
        assert_eq!(MenuAction::About.cite(), "window.cpp:1612");
        for action in MenuAction::ALL {
            let line: u32 = action
                .cite()
                .strip_prefix("window.cpp:")
                .expect("cite shape")
                .parse()
                .expect("cite line");
            assert!((1587..=1612).contains(&line), "{action:?}");
        }
    }

    #[test]
    fn keybinds_cover_exactly_the_nine_accelerators() {
        let bound: Vec<(MenuAction, &str)> = MenuAction::ALL
            .iter()
            .filter_map(|action| action.keybind().map(|hint| (*action, hint)))
            .collect();
        assert_eq!(
            bound,
            [
                (MenuAction::Open, "Ctrl + O"),
                (MenuAction::Quit, "Ctrl + Q"),
                (MenuAction::Play, "Space"),
                (MenuAction::Pause, "Ctrl + P"),
                (MenuAction::Stop, "Ctrl + S"),
                (MenuAction::Prev, "Ctrl + ArrowLeft"),
                (MenuAction::Next, "Ctrl + ArrowRight"),
                (MenuAction::Prefs, "Ctrl + ,"),
                (MenuAction::Help, "F1"),
            ]
        );
    }

    #[test]
    fn external_urls_are_verbatim_cpp() {
        assert_eq!(SEARCH_URL, "https://midisite.co.uk");
        assert_eq!(WEBSITE_URL, "https://sourceforge.net/p/dmidiplayer/");
    }

    #[test]
    fn messages_map_to_the_existing_variants() {
        assert!(matches!(
            MenuAction::Open.message(),
            Message::OpenFilesDialog
        ));
        assert!(matches!(
            MenuAction::FileInfo.message(),
            Message::ShowFileInfo
        ));
        assert!(matches!(
            MenuAction::LoadSettings.message(),
            Message::SongSettingsLoad
        ));
        assert!(matches!(
            MenuAction::SaveSettings.message(),
            Message::SongSettingsSave
        ));
        match MenuAction::Search.message() {
            Message::OpenUrl(url) => assert_eq!(url, SEARCH_URL),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
        // Quit is the T14d variant (the T14b VM callback is retired).
        assert!(matches!(MenuAction::Quit.message(), Message::Quit));
        assert!(matches!(MenuAction::Play.message(), Message::Play));
        assert!(matches!(MenuAction::Pause.message(), Message::Pause));
        assert!(matches!(MenuAction::Stop.message(), Message::Stop));
        assert!(matches!(MenuAction::Prev.message(), Message::PrevSong));
        assert!(matches!(MenuAction::Next.message(), Message::NextSong));
        assert!(matches!(MenuAction::Rew.message(), Message::BeatBackward));
        assert!(matches!(MenuAction::Fwd.message(), Message::BeatForward));
        assert!(matches!(
            MenuAction::Jump.message(),
            Message::ShowDialog(Dialog::JumpToBar)
        ));
        // U3 pin: menu Loop emits the toggle-ON variant (no new variant).
        assert!(matches!(
            MenuAction::Loop.message(),
            Message::LoopToggled(true)
        ));
        assert!(matches!(
            MenuAction::RepeatOff.message(),
            Message::RepeatModeChanged(RepeatMode::Off)
        ));
        assert!(matches!(
            MenuAction::RepeatSong.message(),
            Message::RepeatModeChanged(RepeatMode::Song)
        ));
        assert!(matches!(
            MenuAction::RepeatList.message(),
            Message::RepeatModeChanged(RepeatMode::Playlist)
        ));
        assert!(matches!(
            MenuAction::MidiSetup.message(),
            Message::ShowDialog(Dialog::MidiSetup)
        ));
        assert!(matches!(
            MenuAction::Prefs.message(),
            Message::ShowDialog(Dialog::Preferences)
        ));
        assert!(matches!(MenuAction::Help.message(), Message::ShowHelp));
        match MenuAction::Website.message() {
            Message::OpenUrl(url) => assert_eq!(url, WEBSITE_URL),
            other => panic!("expected OpenUrl, got {other:?}"),
        }
        assert!(matches!(MenuAction::About.message(), Message::ShowAbout));
    }

    #[test]
    fn view_builds_over_the_contract() {
        // `view` is not introspectable, but it must build without panic —
        // the effects are pinned per-entry above.
        let _ = view(MenuViewModel {});
    }
}
