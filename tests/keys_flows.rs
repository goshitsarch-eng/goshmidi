//! T14e integration: keyboard shortcuts + menu flows through `App::update`.
//!
//! Same hermetic harness as T10c–T13d (temp HOME, per-test portable config,
//! headless engine, synthetic `Opened`). Every test drives the real [`App`]
//! through the public `cosmic::Application` trait (`init` + `update`).
//!
//! Observability contract (`App` fields are private, `Task`s are opaque):
//! * dialog-open state pins through open/close probes — a D2 open followed
//!   by `CloseDialog` writes the portable config (commit-on-close), while
//!   `CloseDialog` with no dialog (or a non-D2 dialog) writes nothing;
//!   dialog SWITCHES pin through abandonment (a D2 scratch edit lost when a
//!   second dialog opens, committed when the key was gated);
//! * playlist advance pins through the song-`.cfg` `file` probe (see
//!   `common`); transport status, toasts, engine sends, and focus moves are
//!   engine-/runtime-opaque headless, so those legs pin routing-through-
//!   `update` + observable silence here while the state halves stay
//!   unit-pinned in `app.rs` / `state::keys` (named per test);
//! * `Task` emission (D1/D9–D12 choosers, `OpenUrl`, focus moves, Quit with
//!   no window) is a code-shape pin by precedent (the `dbus_activation`
//!   "present" half in `playlist_flows`): the task drops headless and the
//!   test pins the message path + state silence.
//! Pure halves ride the exact code the arms call (`resolve_key`,
//! `KeyAction::message`, `MenuAction::*`, `select_refilled_port`,
//! `PreviewNote::audition`, `manual_song_action_allowed`); pixel output is
//! never matched.

mod common;

use std::path::PathBuf;

use common::{Harness, canon, fixture, hermetic_home};
use cosmic::iced::keyboard::{
    Event as KeyboardEvent, Key, Location, Modifiers,
    key::{Code, Named, Physical},
};
use dmidiplayer::config::paths::{Env, Paths};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::message::{Dialog, Message, MidiPort, PlayerEvent, SongSnapshot};
use dmidiplayer::state::dialogs::manual_song_action_allowed;
use dmidiplayer::state::keys::{ACCELERATORS, KeyAction, resolve_key};
use dmidiplayer::state::midi::select_refilled_port;
use dmidiplayer::state::piano::{PREVIEW_VELOCITY, PreviewNote};
use dmidiplayer::ui::{MenuViewModel, info_dialogs, jump_loop, menu, menu::MenuAction, midi_setup};

/// Synthetic ignored-status `KeyPressed` (mirrors the `state::keys` and
/// `app.rs` test builders).
fn key_event(key: Key, physical: Physical, modifiers: Modifiers) -> KeyboardEvent {
    KeyboardEvent::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: physical,
        location: Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    }
}

fn char_key(c: &str, code: Code, modifiers: Modifiers) -> KeyboardEvent {
    key_event(Key::Character(c.into()), Physical::Code(code), modifiers)
}

fn named_key(named: Named, code: Code, modifiers: Modifiers) -> KeyboardEvent {
    key_event(Key::Named(named), Physical::Code(code), modifiers)
}

/// The 9 C++ accelerators as synthetic events (US layout, C++ table order —
/// mirrors `state::keys::tests::accelerator_events`).
fn accel_events() -> [(KeyboardEvent, KeyAction); 9] {
    const CTRL: Modifiers = Modifiers::CTRL;
    [
        (char_key("o", Code::KeyO, CTRL), KeyAction::Open),
        (char_key("q", Code::KeyQ, CTRL), KeyAction::Quit),
        (
            char_key(" ", Code::Space, Modifiers::empty()),
            KeyAction::Play,
        ),
        (char_key("p", Code::KeyP, CTRL), KeyAction::Pause),
        (char_key("s", Code::KeyS, CTRL), KeyAction::Stop),
        (
            named_key(Named::ArrowRight, Code::ArrowRight, CTRL),
            KeyAction::Next,
        ),
        (
            named_key(Named::ArrowLeft, Code::ArrowLeft, CTRL),
            KeyAction::Prev,
        ),
        (
            named_key(Named::F1, Code::F1, Modifiers::empty()),
            KeyAction::Help,
        ),
        (char_key(",", Code::Comma, CTRL), KeyAction::Prefs),
    ]
}

fn enter_event() -> KeyboardEvent {
    named_key(Named::Enter, Code::Enter, Modifiers::empty())
}

fn esc_event() -> KeyboardEvent {
    named_key(Named::Escape, Code::Escape, Modifiers::empty())
}

/// Feed all 9 accelerator events through `App::update` (the gating tests'
/// bombardment).
fn fire_all_accels(h: &mut Harness) {
    for (event, _) in accel_events() {
        h.upd(Message::KeyEvent(event));
    }
}

/// Feed a synthetic `Opened` for `title` (never mixed with
/// `probe_current_file` — its `Opened` would reset the state under test).
fn open_song(h: &mut Harness, title: &str) {
    h.upd(Message::Player(PlayerEvent::Opened(Box::new(
        SongSnapshot {
            title: title.to_string(),
            format: "SMF".to_string(),
            bars: 16,
            len_ticks: 4800,
            duration_ms: 60_000,
            division: 96,
            num_tracks: 2,
            initial_tempo_us: 500_000.0,
            channel_used: std::array::from_fn(|ch| ch < 2),
            channel_labels: std::array::from_fn(|ch| format!("Track{ch}")),
            lowest_note: 36,
            highest_note: 96,
            charset: "UTF-8".to_string(),
            texts: Vec::new(),
            metadata: Vec::new(),
            copyrights: Vec::new(),
        },
    ))));
}

/// Song-`.cfg` path for a synthetic title (same `<title>.cfg` form `Paths`
/// uses under the hermetic data dir).
fn song_cfg(title: &str) -> PathBuf {
    hermetic_home()
        .join(".dmidiplayer")
        .join(format!("{title}.cfg"))
}

/// Whole persisted portable config through the real reader (the commit
/// probe; same shape as the T12c/T13d prefs probes).
fn persisted_settings(h: &Harness, tag: &str) -> Settings {
    let home = hermetic_home();
    let env = Env::for_test(home, home, home, home);
    let paths = Paths::with_env(env, Some(Some(h.scratch().join(format!("{tag}.conf")))));
    Settings::load(&paths)
}

/// D3 refill echo for flows that never switch backend/`Show all` (the live
/// FluidSynth default).
fn refreshed(ports: Vec<MidiPort>) -> Message {
    Message::MidiPortsRefreshed {
        backend: "FluidSynth".to_string(),
        advanced: false,
        ports,
    }
}

/// Ctrl+, event (the gating probe: seeds D2 when routing is live, gated to
/// nothing under any dialog).
fn prefs_event() -> KeyboardEvent {
    char_key(",", Code::Comma, Modifiers::CTRL)
}

/// All 9 accelerator events resolve to their `ACCELERATORS` table ids (the
/// flow-level cross-pin; the table order/count half is unit-pinned by
/// `accelerator_table_pins_all_9_gtk_specs_in_cpp_order`,
/// `all_9_accelerators_resolve_without_dialog`, and
/// `accelerator_table_targets_match_resolver` in `state::keys`).
#[test]
fn accel_table_resolves_all_9_ids() {
    assert_eq!(ACCELERATORS.len(), 9);
    for (event, want) in accel_events() {
        assert_eq!(resolve_key(&event, None, Some(0)), Some(want));
        assert!(
            ACCELERATORS.iter().any(|acc| acc.id == want),
            "{want:?} missing from ACCELERATORS"
        );
    }
}

/// Ctrl+Right/Ctrl+Left advance through `KeyEvent`s: row 0 → 1 → 2, no wrap
/// at the end, back with Ctrl+Left — all through `update`, all observed via
/// the song-`.cfg` probe. (Autoplay dispatch + prev-never-saves:
/// unit-pinned by `key_event_routes_next_prev_selection`.)
#[test]
fn ctrl_arrows_advance_playlist_through_key_events() {
    let mut h = Harness::new("key-arrows");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    let c = fixture("lindaamiga.mid");
    h.upd(Message::OpenFiles(
        vec![a.clone(), b.clone(), c.clone()],
        true,
    ));
    let (right, left) = (
        named_key(Named::ArrowRight, Code::ArrowRight, Modifiers::CTRL),
        named_key(Named::ArrowLeft, Code::ArrowLeft, Modifiers::CTRL),
    );
    assert_eq!(resolve_key(&right, None, Some(0)), Some(KeyAction::Next));
    assert_eq!(resolve_key(&left, None, Some(1)), Some(KeyAction::Prev));

    h.upd(Message::KeyEvent(right.clone()));
    assert_eq!(h.probe_current_file("next-1"), canon(&b));
    h.upd(Message::KeyEvent(right.clone()));
    assert_eq!(h.probe_current_file("next-2"), canon(&c));
    h.upd(Message::KeyEvent(right));
    assert_eq!(h.probe_current_file("next-nowr"), canon(&c));
    h.upd(Message::KeyEvent(left));
    assert_eq!(h.probe_current_file("prev-1"), canon(&b));
    assert!(!h.config_exists(), "T14e: transport persists nothing");
}

/// Space / Ctrl+P / Ctrl+S route through `update` without stray effects:
/// the playlist stays put, no config is written, and no dialog opens (a
/// trailing `CloseDialog` is a silent no-op). The status halves (Playing →
/// Paused → Stopped) are unit-pinned by
/// `key_event_routes_transport_accelerators` (status is private); the
/// resolution halves by the `state::keys` accel pins.
#[test]
fn transport_keys_route_without_stray_effects() {
    let mut h = Harness::new("key-transport");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    open_song(&mut h, "key-transport.mid");
    let (space, pause, stop) = (
        char_key(" ", Code::Space, Modifiers::empty()),
        char_key("p", Code::KeyP, Modifiers::CTRL),
        char_key("s", Code::KeyS, Modifiers::CTRL),
    );
    assert_eq!(resolve_key(&space, None, Some(0)), Some(KeyAction::Play));
    assert_eq!(resolve_key(&pause, None, Some(0)), Some(KeyAction::Pause));
    assert_eq!(resolve_key(&stop, None, Some(0)), Some(KeyAction::Stop));

    h.upd(Message::KeyEvent(space));
    h.upd(Message::KeyEvent(pause));
    h.upd(Message::KeyEvent(stop));
    assert_eq!(h.probe_current_file("transport-keeps"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// Ctrl+O routes to the D1 portal trigger: headless the chooser task drops,
/// so the playlist stays intact, no config is written, and no dialog state
/// is retained (D1 is deliberately absent from `Dialog` — a trailing
/// `CloseDialog` is silent). (Unit-pinned by
/// `key_event_routes_open_prefs_help_and_quit` and
/// `open_files_dialog_launches_without_state_change`.)
#[test]
fn ctrl_o_launches_chooser_silently_headless() {
    let mut h = Harness::new("key-open");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    let event = char_key("o", Code::KeyO, Modifiers::CTRL);
    assert_eq!(resolve_key(&event, None, Some(0)), Some(KeyAction::Open));
    assert!(matches!(
        KeyAction::Open.message(),
        Some(Message::OpenFilesDialog)
    ));
    assert!(matches!(
        MenuAction::Open.message(),
        Message::OpenFilesDialog
    ));

    h.upd(Message::KeyEvent(event));
    assert_eq!(h.probe_current_file("open-keeps"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// Ctrl+Q routes to the `Quit` arm and the bare `Message::Quit` (the
/// File-menu path) runs it directly: headless the app owns no main window,
/// so the arm returns `Task::none()` and state is untouched (playlist
/// intact, no config, no dialog). `window::close` targeting is a code-shape
/// pin (the `dbus_activation` "present" precedent — nothing real is
/// closed); `KeyAction::message` deliberately returns `None` for `Quit`
/// (the arm is built in `update`). (Unit-pinned by
/// `key_event_routes_open_prefs_help_and_quit`; the 13-variant mapping by
/// `key_action_message_mapping_pins_all_13`.)
#[test]
fn ctrl_q_and_quit_arm_are_silent_with_no_window() {
    let mut h = Harness::new("key-quit");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    let event = char_key("q", Code::KeyQ, Modifiers::CTRL);
    assert_eq!(resolve_key(&event, None, Some(0)), Some(KeyAction::Quit));
    assert!(KeyAction::Quit.message().is_none());
    assert!(matches!(MenuAction::Quit.message(), Message::Quit));

    h.upd(Message::KeyEvent(event));
    h.upd(Message::Quit);
    assert_eq!(h.probe_current_file("quit-keeps"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// F1 opens D5 and Ctrl+, opens D2, both through `KeyEvent`s from the base:
/// the prefs key seeds a shell (a trailing close commits the config),
/// while F1 opens the Help sheet (the prefs key gates under it, so the
/// trailing close writes nothing — if F1 were a no-op the prefs key would
/// seed D2 and the close would commit). (Unit-pinned by
/// `key_event_routes_open_prefs_help_and_quit`.)
#[test]
fn f1_and_prefs_keys_open_their_dialogs() {
    let f1 = named_key(Named::F1, Code::F1, Modifiers::empty());
    assert_eq!(resolve_key(&f1, None, None), Some(KeyAction::Help));
    assert!(matches!(KeyAction::Help.message(), Some(Message::ShowHelp)));
    assert_eq!(
        resolve_key(&prefs_event(), None, None),
        Some(KeyAction::Prefs)
    );

    // Ctrl+, seeds D2: the close commits.
    let mut h = Harness::new("key-prefs");
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "key-prefs"), Settings::new());

    // F1 opens D5 from the base: the prefs key gates under it (if F1
    // were a no-op, the prefs key would seed D2 and the close would
    // commit instead).
    let mut h = Harness::new("key-f1");
    h.upd(Message::KeyEvent(f1));
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        !h.config_exists(),
        "T14e: F1 must open D5 (prefs key gated under it)"
    );
}

/// D2 gates all 9 accelerators + Enter: with a scratch edit open, the
/// bombardment changes nothing (playlist intact, shell intact — the close
/// commits the edit). Esc closes: afterwards a scratch edit drops (no
/// shell) and the close persists live. (Routing half unit-pinned by
/// `key_event_dialog_gate_pins_subscription_level_blocking`; resolution by
/// `dialog_gates_all_9_accelerators_and_enter`.)
#[test]
fn d2_gates_accelerators_esc_closes() {
    let mut h = Harness::new("key-gate-d2");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 3;
    h.upd(Message::PrefsScratchEdited(next));
    fire_all_accels(&mut h);
    h.upd(Message::KeyEvent(enter_event()));
    assert_eq!(h.probe_current_file("d2-gated"), canon(&a));
    h.upd(Message::CloseDialog);
    assert_eq!(
        persisted_settings(&h, "key-gate-d2").drums_channel,
        3,
        "T14e: gated keys must leave the D2 shell intact"
    );

    // Esc closes (commit-on-close writes): the shell is gone, so the
    // follow-up edit drops and the persisted commit is live, not the edit
    // (if Esc had failed, the edit would commit instead).
    let mut h = Harness::new("key-esc-d2");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    h.upd(Message::KeyEvent(esc_event()));
    let mut next = Settings::new();
    next.drums_channel = 3;
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::CloseDialog);
    assert_eq!(
        persisted_settings(&h, "key-esc-d2"),
        Settings::new(),
        "T14e: Esc must close D2 (edit dropped)"
    );
}

/// D3 gates all 9 accelerators + Enter: the bombardment neither seeds D2
/// (the close writes nothing) nor disturbs the shell (a later edit +
/// Apply still commits). Esc closes: routing is restored, so a following
/// Ctrl+, seeds D2 (the close commits — if D3 were still open the prefs
/// key would be gated and the close silent). (Unit halves as in
/// `d2_gates_accelerators_esc_closes`.)
#[test]
fn d3_gates_accelerators_esc_closes() {
    // Bombardment seeds no D2 shell.
    let mut h = Harness::new("key-gate-d3");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    fire_all_accels(&mut h);
    h.upd(Message::KeyEvent(enter_event()));
    assert_eq!(h.probe_current_file("d3-gated"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists(), "T14e: gated keys must seed nothing");

    // The D3 shell survives the bombardment.
    let mut h = Harness::new("key-gate-d3b");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    fire_all_accels(&mut h);
    h.upd(Message::MidiSoundFontChanged("/sf/kept.sf2".to_string()));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "key-gate-d3b").sound_font,
        "/sf/kept.sf2"
    );

    // Esc closes: Ctrl+, routes again.
    let mut h = Harness::new("key-esc-d3");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::KeyEvent(esc_event()));
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        h.config_exists(),
        "T14e: Esc must close D3 (prefs key routes again)"
    );
}

/// D7 + D8 gate all 9 accelerators + Enter (playlist intact, nothing
/// seeded), and Esc closes each (routing restored). The gate proof is the
/// Ctrl+, probe: under an open D7/D8 it resolves to nothing, so the close
/// writes nothing; after Esc it seeds D2, so the close commits. (Unit
/// halves as in `d2_gates_accelerators_esc_closes`.)
#[test]
fn d7_and_d8_gate_accelerators_esc_closes() {
    // D7 bombardment: playlist intact, nothing seeded.
    let mut h = Harness::new("key-gate-d7");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    open_song(&mut h, "key-gate-d7.mid");
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    fire_all_accels(&mut h);
    h.upd(Message::KeyEvent(enter_event()));
    assert_eq!(h.probe_current_file("d7-gated"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());

    // D7 gate proof + Esc proof.
    let mut h = Harness::new("key-gate-d7b");
    open_song(&mut h, "key-gate-d7b.mid");
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        !h.config_exists(),
        "T14e: Ctrl+, must gate under D7 (no D2 seeded)"
    );
    let mut h = Harness::new("key-esc-d7");
    open_song(&mut h, "key-esc-d7.mid");
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::KeyEvent(esc_event()));
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(h.config_exists(), "T14e: Esc must close D7");

    // D8 (unconditional open, no song needed): same three probes.
    let mut h = Harness::new("key-gate-d8");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(Message::LoopToggled(true));
    fire_all_accels(&mut h);
    h.upd(Message::KeyEvent(enter_event()));
    assert_eq!(h.probe_current_file("d8-gated"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
    let mut h = Harness::new("key-gate-d8b");
    h.upd(Message::LoopToggled(true));
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        !h.config_exists(),
        "T14e: Ctrl+, must gate under D8 (no D2 seeded)"
    );
    let mut h = Harness::new("key-esc-d8");
    h.upd(Message::LoopToggled(true));
    h.upd(Message::KeyEvent(esc_event()));
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(h.config_exists(), "T14e: Esc must close D8");
}

/// Tab / Shift+Tab traverse with and without dialogs: no playlist move, no
/// config write, and open shells survive (the D2 edit still commits). The
/// focus `Task`s are runtime-opaque headless (code-shape pin — the arms
/// return `focus_next` / `focus_previous`); the no-state-change routing is
/// unit-pinned by `key_event_dialog_gate_pins_subscription_level_blocking`
/// and resolution by the `state::keys` Tab pins.
#[test]
fn tab_traverses_under_dialogs_without_state_change() {
    let tab = named_key(Named::Tab, Code::Tab, Modifiers::empty());
    let stab = named_key(Named::Tab, Code::Tab, Modifiers::SHIFT);
    assert_eq!(resolve_key(&tab, None, None), Some(KeyAction::FocusNext));
    assert_eq!(
        resolve_key(&stab, Some(Dialog::Preferences), None),
        Some(KeyAction::FocusPrevious)
    );
    assert!(KeyAction::FocusNext.message().is_none());

    // No dialog: pure traversal, zero state change.
    let mut h = Harness::new("key-tab");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(Message::KeyEvent(tab.clone()));
    h.upd(Message::KeyEvent(stab.clone()));
    assert_eq!(h.probe_current_file("tab-keeps"), canon(&a));
    assert!(!h.config_exists());

    // Under D2: the shell survives traversal.
    let mut h = Harness::new("key-tab-d2");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 3;
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::KeyEvent(tab.clone()));
    h.upd(Message::KeyEvent(stab.clone()));
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "key-tab-d2").drums_channel, 3);

    // Under D7: traversal, then the close stays silent.
    let mut h = Harness::new("key-tab-d7");
    open_song(&mut h, "key-tab-d7.mid");
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::KeyEvent(tab));
    h.upd(Message::KeyEvent(stab));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// Enter with a selection plays the row (U11 — observed via the probe);
/// with no selection it is a silent no-op; under a dialog it gates (the
/// D2 edit still commits). (Dispatch + autoplay: unit-pinned by
/// `key_event_enter_plays_selection` and the `playlist_row_activated_*`
/// halves; resolution by the `state::keys` Enter pins.)
#[test]
fn enter_plays_selection_only_without_dialog() {
    assert_eq!(
        resolve_key(&enter_event(), None, Some(1)),
        Some(KeyAction::EnterPlay(1))
    );
    assert_eq!(resolve_key(&enter_event(), None, None), None);
    assert_eq!(
        resolve_key(&enter_event(), Some(Dialog::Preferences), Some(1)),
        None
    );
    assert!(matches!(
        KeyAction::EnterPlay(1).message(),
        Some(Message::PlaylistRowActivated(1))
    ));

    // With a selection: the row activates.
    let mut h = Harness::new("key-enter");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    h.upd(Message::OpenFiles(vec![a.clone(), b.clone()], true));
    h.upd(Message::PlaylistSelect(1));
    h.upd(Message::KeyEvent(enter_event()));
    assert_eq!(h.probe_current_file("enter-plays"), canon(&b));
    assert!(!h.config_exists());

    // Without one: silent no-op.
    let mut h = Harness::new("key-enter-none");
    h.upd(Message::KeyEvent(enter_event()));
    h.assert_save_without_song_writes_nothing("enter-none");
    assert!(!h.config_exists());

    // Under D2: gated (the edit still commits).
    let mut h = Harness::new("key-enter-d2");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 3;
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::KeyEvent(enter_event()));
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "key-enter-d2").drums_channel, 3);
}

/// All 23 menu entries produce their documented `Message` (the
/// `MenuViewModel`-callback → message mapping — the VM is empty since T14d,
/// so `MenuAction::message` IS the callback), in C++ section order
/// (6/9/3/5), with verbatim URLs and the U3 Loop path. (Inventory halves
/// unit-pinned by `inventory_is_23_entries_in_cpp_section_order`,
/// `labels_are_verbatim_cpp_menu_entries`, and
/// `messages_map_to_the_existing_variants`.)
#[test]
fn menu_maps_all_23_entries_to_documented_messages() {
    use dmidiplayer::message::RepeatMode;
    assert_eq!(MenuAction::ALL.len(), 23);
    assert_eq!(menu::SECTIONS.len(), 4);
    assert_eq!(menu::SECTIONS[0].actions.len(), 6);
    assert_eq!(menu::SECTIONS[1].actions.len(), 9);
    assert_eq!(menu::SECTIONS[2].actions.len(), 3);
    assert_eq!(menu::SECTIONS[3].actions.len(), 5);
    // File entries 3–4 are the song-settings pair, in C++ order.
    assert_eq!(MenuAction::ALL[2], MenuAction::LoadSettings);
    assert_eq!(MenuAction::ALL[3], MenuAction::SaveSettings);
    // Exactly the 9 C++ accelerators carry hints.
    assert_eq!(
        MenuAction::ALL
            .iter()
            .filter(|action| action.keybind().is_some())
            .count(),
        9
    );

    for action in MenuAction::ALL {
        let message = action.message();
        let ok = matches!(
            (action, &message),
            (MenuAction::Open, Message::OpenFilesDialog)
                | (MenuAction::FileInfo, Message::ShowFileInfo)
                | (MenuAction::LoadSettings, Message::SongSettingsLoad)
                | (MenuAction::SaveSettings, Message::SongSettingsSave)
                | (MenuAction::Quit, Message::Quit)
                | (MenuAction::Play, Message::Play)
                | (MenuAction::Pause, Message::Pause)
                | (MenuAction::Stop, Message::Stop)
                | (MenuAction::Prev, Message::PrevSong)
                | (MenuAction::Next, Message::NextSong)
                | (MenuAction::Rew, Message::BeatBackward)
                | (MenuAction::Fwd, Message::BeatForward)
                | (MenuAction::Jump, Message::ShowDialog(Dialog::JumpToBar))
                | (MenuAction::Loop, Message::LoopToggled(true))
                | (
                    MenuAction::RepeatOff,
                    Message::RepeatModeChanged(RepeatMode::Off)
                )
                | (
                    MenuAction::RepeatSong,
                    Message::RepeatModeChanged(RepeatMode::Song)
                )
                | (
                    MenuAction::RepeatList,
                    Message::RepeatModeChanged(RepeatMode::Playlist)
                )
                | (
                    MenuAction::MidiSetup,
                    Message::ShowDialog(Dialog::MidiSetup)
                )
                | (MenuAction::Prefs, Message::ShowDialog(Dialog::Preferences))
                | (MenuAction::Help, Message::ShowHelp)
                | (MenuAction::About, Message::ShowAbout)
        ) || matches!((action, &message),
            (MenuAction::Search, Message::OpenUrl(url))
                if url.as_str() == menu::SEARCH_URL
        ) || matches!((action, &message),
            (MenuAction::Website, Message::OpenUrl(url))
                if url.as_str() == menu::WEBSITE_URL
        );
        assert!(ok, "T14e: {action:?} produced an undocumented message");
    }
}

/// Repeat entries are plain (non-radio, C++ parity): the `MenuViewModel`
/// carries no repeat mode (empty by value — the bar builds from it), and
/// each entry's message persists its own mode through `update`.
/// (Settings leg unit-pinned by `repeat_mode_syncs_settings_for_close_save`.)
#[test]
fn menu_repeat_entries_are_plain_and_persist_each_mode() {
    // Non-radio: the contract is empty and the whole bar builds from it —
    // no live value can condition any entry.
    let vm = MenuViewModel {};
    std::mem::drop(menu::view(vm));
    std::mem::drop(menu::view(MenuViewModel::default()));

    let mut h = Harness::new("key-menu-repeat");
    h.upd(MenuAction::RepeatSong.message());
    h.persist_live_settings();
    assert_eq!(persisted_settings(&h, "key-menu-repeat").repeat_mode, 1);
    h.upd(MenuAction::RepeatList.message());
    h.persist_live_settings();
    assert_eq!(persisted_settings(&h, "key-menu-repeat").repeat_mode, 2);
    h.upd(MenuAction::RepeatOff.message());
    h.persist_live_settings();
    assert_eq!(persisted_settings(&h, "key-menu-repeat").repeat_mode, 0);
}

/// File entries 3–4 route to `SongSettingsSave`/`SongSettingsLoad` with the
/// loaded-song guards: with a song the save writes the song `.cfg` (and the
/// load routes silently); with no song both are silent no-ops. (Guard
/// halves unit-pinned by `manual_song_actions_noop_on_empty_title` and
/// `manual_song_actions_need_a_loaded_song`; the auto-flag non-gate by
/// `manual_song_actions_ungated_by_auto_flag`.)
#[test]
fn menu_file_song_settings_route_with_guards() {
    assert!(!manual_song_action_allowed(None));
    assert!(!manual_song_action_allowed(Some("")));
    assert!(manual_song_action_allowed(Some("song.mid")));

    // With a song: save writes, load routes.
    let mut h = Harness::new("key-menu-song");
    open_song(&mut h, "key-menu-song.mid");
    h.upd(MenuAction::SaveSettings.message());
    assert!(
        song_cfg("key-menu-song.mid").is_file(),
        "T14e: menu Save must write the song cfg"
    );
    h.upd(MenuAction::LoadSettings.message());
    assert!(song_cfg("key-menu-song.mid").is_file());
    assert!(!h.config_exists());

    // Without one: both guard out silently.
    let mut h = Harness::new("key-menu-nosong");
    h.upd(MenuAction::SaveSettings.message());
    h.assert_save_without_song_writes_nothing("menu-save-guarded");
    h.upd(MenuAction::LoadSettings.message());
    assert!(!h.config_exists());
}

/// The menu's dialog entries drive through `update`: D1 is task-only
/// silence; D2/D3 open shells (close commits / Apply persists); D4/D5/D6
/// open sheets (proven by the Ctrl+, gate probe); D7 gates on a loaded
/// song (no song → nothing opens, proven by restored routing; song →
/// opens); D8 opens unconditionally with no song. (Placement unit-pinned
/// by `menu_bar_places_single_root_in_header_end`; the D7 gate by the
/// `jump_allowed` halves.)
#[test]
fn menu_dialog_entries_drive_through_update() {
    // D1: task-only silence, no retained dialog.
    let mut h = Harness::new("key-menu-open");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(MenuAction::Open.message());
    assert_eq!(h.probe_current_file("menu-open"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());

    // D2: the close commits.
    let mut h = Harness::new("key-menu-prefs");
    h.upd(MenuAction::Prefs.message());
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "key-menu-prefs"), Settings::new());

    // D3: an edit + Apply persists.
    let mut h = Harness::new("key-menu-midi");
    h.upd(MenuAction::MidiSetup.message());
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    h.upd(Message::MidiSoundFontChanged("/sf/menu.sf2".to_string()));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "key-menu-midi").sound_font,
        "/sf/menu.sf2"
    );

    // D4/D5/D6: sheets open (the prefs key gates under each).
    for (tag, action) in [
        ("key-menu-about", MenuAction::About),
        ("key-menu-help", MenuAction::Help),
        ("key-menu-info", MenuAction::FileInfo),
    ] {
        let mut h = Harness::new(tag);
        h.upd(action.message());
        h.upd(Message::KeyEvent(prefs_event()));
        h.upd(Message::CloseDialog);
        assert!(
            !h.config_exists(),
            "T14e: {action:?} must open its sheet (prefs key gated)"
        );
    }

    // D7 without a song: the gate holds (routing stays live).
    let mut h = Harness::new("key-menu-jump0");
    h.upd(MenuAction::Jump.message());
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        h.config_exists(),
        "T14e: menu Jump with no song must open nothing"
    );

    // D7 with a song: opens (the prefs key gates).
    let mut h = Harness::new("key-menu-jump");
    open_song(&mut h, "key-menu-jump.mid");
    h.upd(MenuAction::Jump.message());
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        !h.config_exists(),
        "T14e: menu Jump with a song must open D7"
    );

    // D8 with no song: opens unconditionally (documented deviation).
    let mut h = Harness::new("key-menu-loop");
    h.upd(MenuAction::Loop.message());
    h.upd(Message::KeyEvent(prefs_event()));
    h.upd(Message::CloseDialog);
    assert!(
        !h.config_exists(),
        "T14e: menu Loop must open D8 even with no song"
    );
}

/// The menu's transport entries drive through `update`: Prev/Next move the
/// playlist (observed via the probe); Play/Pause/Stop/Rew/Fwd route with
/// no observable disturbance (status + engine sends are unit-pinned —
/// `key_event_routes_transport_accelerators` — and the beat steps no-op on
/// an empty song engine-side).
#[test]
fn menu_transport_entries_drive_through_update() {
    let mut h = Harness::new("key-menu-play");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    h.upd(Message::OpenFiles(vec![a.clone(), b.clone()], true));
    open_song(&mut h, "key-menu-play.mid");
    h.upd(MenuAction::Next.message());
    assert_eq!(h.probe_current_file("menu-next"), canon(&b));
    h.upd(MenuAction::Prev.message());
    assert_eq!(h.probe_current_file("menu-prev"), canon(&a));

    h.upd(MenuAction::Play.message());
    h.upd(MenuAction::Pause.message());
    h.upd(MenuAction::Stop.message());
    h.upd(MenuAction::Rew.message());
    h.upd(MenuAction::Fwd.message());
    assert_eq!(h.probe_current_file("menu-keeps"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// The menu's external links emit `OpenUrl` with verbatim URLs (search: no
/// trailing slash; website: trailing slash) and drive silently headless
/// (the opener task drops). (Launch/toast halves unit-pinned by
/// `open_url_launches_silently_errors_toast`.)
#[test]
fn menu_external_links_emit_verbatim_urls() {
    assert_eq!(menu::SEARCH_URL, "https://midisite.co.uk");
    assert_eq!(menu::WEBSITE_URL, "https://sourceforge.net/p/dmidiplayer/");
    assert!(matches!(
        MenuAction::Search.message(),
        Message::OpenUrl(url) if url == "https://midisite.co.uk"
    ));
    assert!(matches!(
        MenuAction::Website.message(),
        Message::OpenUrl(url) if url == "https://sourceforge.net/p/dmidiplayer/"
    ));

    let mut h = Harness::new("key-menu-url");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(MenuAction::Search.message());
    h.upd(MenuAction::Website.message());
    assert_eq!(h.probe_current_file("menu-url"), canon(&a));
    assert!(!h.config_exists());
}

/// The menu Quit entry drives the `Quit` arm: silent with no window (see
/// `ctrl_q_and_quit_arm_are_silent_with_no_window` for the hermetic pin).
#[test]
fn menu_quit_drives_the_quit_arm() {
    let mut h = Harness::new("key-menu-quit");
    let a = fixture("test.mid");
    h.upd(Message::OpenFiles(vec![a.clone()], true));
    h.upd(MenuAction::Quit.message());
    assert_eq!(h.probe_current_file("menu-quit"), canon(&a));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// Focus-on-open targets: the documented widget ids are pinned at their
/// public constants (D2's `prefs-close` lives in a private const, so its
/// value half stays with the unit test), every sheet dialog opens through
/// `update` (D2 commits, D3 applies, D4–D8 open + close silently), and the
/// D9–D12 portal triggers retain no dialog (routing stays live — the
/// `None` half). The focus `Task` itself is runtime-opaque headless; the
/// dialog→id mapping is unit-pinned by
/// `dialog_focus_id_pins_every_dialog_target`.
#[test]
fn dialog_open_focuses_the_documented_widget() {
    assert_eq!(midi_setup::APPLY_ID, "midi-apply");
    assert_eq!(jump_loop::JUMP_CONFIRM_ID, "jump-confirm");
    assert_eq!(jump_loop::LOOP_CONFIRM_ID, "loop-confirm");
    assert_eq!(info_dialogs::OK_ID, "dialog-ok");
    assert_eq!(info_dialogs::ABOUT_CLOSE_ID, "about-close");

    // D2 commits on close; D3 applies.
    let mut h = Harness::new("key-focus-d2");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "key-focus-d2"), Settings::new());
    let mut h = Harness::new("key-focus-d3");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    h.upd(Message::MidiSoundFontChanged("/sf/focus.sf2".to_string()));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "key-focus-d3").sound_font,
        "/sf/focus.sf2"
    );

    // D4/D5/D6/D7/D8 open + close silently (D7 with a song).
    for (tag, open) in [
        ("key-focus-d4", Message::ShowAbout),
        ("key-focus-d5", Message::ShowHelp),
        ("key-focus-d6", Message::ShowFileInfo),
    ] {
        let mut h = Harness::new(tag);
        h.upd(open);
        h.upd(Message::CloseDialog);
        assert!(!h.config_exists(), "T14e: {tag} close must be silent");
    }
    let mut h = Harness::new("key-focus-d7");
    open_song(&mut h, "key-focus-d7.mid");
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
    let mut h = Harness::new("key-focus-d8");
    h.upd(Message::LoopToggled(true));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());

    // D9/D10/D12: portal tasks retain no dialog (routing stays live).
    for (tag, trigger) in [
        ("key-focus-d9", Message::PlaylistLoadDialog),
        ("key-focus-d10", Message::PlaylistSaveDialog),
        ("key-focus-d12", Message::LyricsSaveDialog),
    ] {
        let mut h = Harness::new(tag);
        h.upd(trigger);
        h.upd(Message::KeyEvent(prefs_event()));
        h.upd(Message::CloseDialog);
        assert!(
            h.config_exists(),
            "T14e: {tag} must retain no dialog (routing live)"
        );
    }

    // D11 browse is task-only too (needs the D3 shell): the shell survives
    // it undisturbed and a later Apply commits.
    let mut h = Harness::new("key-focus-d11");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    h.upd(Message::MidiSoundFontBrowse);
    h.upd(Message::MidiSoundFontChanged("/sf/d11.sf2".to_string()));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "key-focus-d11").sound_font,
        "/sf/d11.sf2"
    );
}

/// T13 minor (b) behavioral half: piano preview routes silently with no
/// confirmed output (the key stays dark — no config, no disturbance) and
/// stays externally silent once an output confirms (a clean
/// `OutputChanged`). The matrix halves are unit-pinned by
/// `piano_preview_gated_without_output` (the matrix is private); the
/// release leg by `piano_preview_idle_only_note_off`; the audition spec is
/// the pure `PreviewNote::audition` (vel 80, C++ `:2099`).
#[test]
fn piano_preview_gated_without_confirmed_output_flow() {
    assert_eq!(
        PreviewNote::audition(0, 60),
        PreviewNote {
            ch: 0,
            note: 60,
            vel: 80
        }
    );
    assert_eq!(PREVIEW_VELOCITY, 80);

    let mut h = Harness::new("key-piano");
    h.upd(Message::PianoPreviewOn(0, 60));
    h.upd(Message::PianoPreviewOff);
    assert!(
        !h.config_exists(),
        "T14e: unconfirmed preview must persist nothing"
    );

    // Confirm an output: the preview still disturbs nothing observable.
    h.upd(Message::Player(PlayerEvent::OutputChanged {
        backend: "FluidSynth".to_string(),
        port: "fluidsynth".to_string(),
        warning: None,
    }));
    h.upd(Message::PianoPreviewOn(0, 60));
    h.upd(Message::PianoPreviewOff);
    assert!(!h.config_exists());
}

/// T13 minor (c) behavioral half: Test, then an Apply-with-reselect, then
/// stale test-note releases (epochs 0–2 — startup 1 + Test 1 + Apply 1
/// leaves live at 3) — all route silently and the Apply commit stays
/// intact. Skip-vs-send is engine-opaque headless (both legs silent), so
/// the epoch compare stays unit-pinned by
/// `test_note_off_skips_after_intervening_reselect` and the accounting by
/// `output_epoch_bumps_once_per_select_output_send`.
#[test]
fn stale_test_note_off_skips_at_flow_level() {
    let mut h = Harness::new("key-epoch");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    h.upd(Message::MidiTestNote);
    h.upd(Message::MidiBackendSelected("Dummy".to_string()));
    h.upd(Message::MidiPortsRefreshed {
        backend: "Dummy".to_string(),
        advanced: false,
        ports: vec![MidiPort {
            id: "dummy".to_string(),
            label: "Dummy (no output)".to_string(),
        }],
    });
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "key-epoch").last_output_backend,
        "Dummy"
    );

    for epoch in [0, 1, 2] {
        h.upd(Message::MidiTestNoteOff {
            ch: 0,
            note: 60,
            epoch,
        });
    }
    assert_eq!(
        persisted_settings(&h, "key-epoch").last_output_backend,
        "Dummy",
        "T14e: stale offs must leave the commit intact"
    );
}

/// T13 minor (d) behavioral halves: a mid-refill Apply (empty dropdown)
/// persists the empty port id, and a disjoint refill (stale want, no id
/// or label match) persists index 0 — C++ `plist[pi].id` (`:1164-1166`).
/// (Mirror halves unit-pinned by
/// `midi_apply_empty_list_selects_empty_port_id` and
/// `midi_apply_uses_dropdown_shown_port_not_raw_want`; the pure resolver
/// is pinned inline.)
#[test]
fn apply_resolves_dropdown_shown_id_mid_refill_and_disjoint() {
    // Mid-refill race: no refill landed, the shown id is empty.
    let mut h = Harness::new("key-shown-race");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiApply);
    assert!(
        persisted_settings(&h, "key-shown-race")
            .last_output_connection
            .is_empty(),
        "T14e: mid-refill Apply must persist the empty shown id"
    );

    // Disjoint refill: stale want matches nothing → index 0.
    let mut h = Harness::new("key-shown-disjoint");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiPortSelected("128:0".to_string()));
    h.upd(refreshed(vec![MidiPort {
        id: "129:0".to_string(),
        label: "USB MIDI".to_string(),
    }]));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "key-shown-disjoint").last_output_connection,
        "129:0"
    );

    // Pure resolver halves (the exact code both arms call).
    assert_eq!(select_refilled_port(&[], "128:0"), 0);
    assert_eq!(
        select_refilled_port(
            &[MidiPort {
                id: "129:0".to_string(),
                label: "USB MIDI".to_string(),
            }],
            "128:0"
        ),
        0
    );
}
