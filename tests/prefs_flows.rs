//! T12c integration: preferences flows through `App::update` + config probes.
//!
//! Same hermetic harness as T10c/T11c (temp HOME, per-test portable config,
//! headless engine — no fixtures needed: the prefs shell is song-free).
//! `App` fields are private, so scratch/live halves pin behavior through the
//! portable config: the harness `persist_live_settings` probe (D2 open +
//! unchanged close) persists the LIVE settings without touching
//! `lastPlayList`, while dialog close persists the narrowed commit (the
//! 17 D2-owned keys over live — T12 devil-BLOCKING FIX). Mid-dialog probes
//! reopen prefs + resend scratch afterwards (the probe reseeds, abandoning
//! their edits — T14d; was a dialog switch under the old D3 probe).
//! Toast text, picker active flags, MIDI mirrors, and the engine reselect /
//! announce / solo-repush sends stay unit-pinned in `app.rs` (named per
//! test); this file pins the open/edit/close wiring plus the persisted bytes.

mod common;

use common::{Harness, canon, hermetic_home};
use cosmic::widget::color_picker::ColorPickerUpdate;
use dmidiplayer::config::paths::{Env, Paths};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::message::{Dialog, Message, PlayerEvent, SongSnapshot};
use dmidiplayer::state::lyrics::parse_gdk_rgba;

/// Whole persisted portable config through the real reader (the commit
/// probe; same shape as the T11c `PrefsChanged` probe).
fn persisted_settings(h: &Harness, tag: &str) -> Settings {
    let home = hermetic_home();
    let env = Env::for_test(home, home, home, home);
    let paths = Paths::with_env(env, Some(Some(h.scratch().join(format!("{tag}.conf")))));
    Settings::load(&paths)
}

/// Picker-canonical colors round-trip through float conversions (one 8-bit
/// step of slack per channel — same tolerance as the unit half).
fn assert_color_near(actual: &str, want: &str, row: &str) {
    let got =
        parse_gdk_rgba(actual).unwrap_or_else(|| panic!("T12c: {row} color unparsable: {actual}"));
    let want = parse_gdk_rgba(want).expect("T12c: want color must parse");
    for (g, w) in [(got.r, want.r), (got.g, want.g), (got.b, want.b)] {
        assert!(
            (g - w).abs() < 2.0 / 255.0,
            "T12c: {row} color {actual} drifted from seed"
        );
    }
}

/// Open seeds scratch == live: a close with no edits persists the live
/// settings byte-identically (a zeroed/default scratch would persist
/// defaults instead). (Seed + picker seeding incl. garbage-seeds-nothing:
/// unit-pinned by `prefs_open_seeds_scratch_and_pickers`.)
#[test]
fn open_seeds_scratch_equal_to_live() {
    let mut h = Harness::new("prf-seed");
    let mut base = Settings::new();
    base.drums_channel = 9;
    base.solo_volume_reduction = 75;
    base.auto_play = false;
    base.instrument_map = 2;
    base.future_color = "red".to_string();
    base.past_color = "notacolor".to_string();
    base.single_color = "notacolor".to_string();
    base.text_alignment = 1;
    h.upd(Message::PrefsChanged(base.clone()));
    assert_eq!(persisted_settings(&h, "prf-seed"), base);

    h.upd(Message::ShowDialog(Dialog::Preferences));
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "prf-seed"), base);
}

/// Scratch edits stay isolated until close: open writes nothing, edits write
/// nothing, the live probe still reads the pre-dialog live values — then
/// one close commits the whole scratch verbatim (garbage colors included,
/// C++ parity). (Unit-pinned by
/// `prefs_scratch_edits_are_isolated_until_close`.)
#[test]
fn scratch_edits_isolated_until_close() {
    let mut h = Harness::new("prf-iso");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    assert!(!h.config_exists());

    let live = Settings::new();
    let mut next = live.clone();
    next.drums_channel = 3;
    next.solo_volume_reduction = 75;
    next.auto_play = false;
    next.future_color = "blue".to_string();
    next.single_color = "notacolor".to_string();
    h.upd(Message::PrefsScratchEdited(next.clone()));
    assert!(!h.config_exists());

    // Live probe: the scratch edits leaked nowhere. The probe abandons the
    // prefs shell (reseed), so reopen + resend before the close.
    h.persist_live_settings();
    assert_eq!(persisted_settings(&h, "prf-iso"), live);
    h.upd(Message::ShowDialog(Dialog::Preferences));
    h.upd(Message::PrefsScratchEdited(next.clone()));

    // Close commits once: narrow adoption (== wholesale here — live is
    // otherwise untouched) + save.
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "prf-iso"), next);
}

/// Close commits the D2 rows (one commit across General/Lyrics/Piano pages)
/// and repushes solo% (U14): pre-close the live probe still reads the old
/// solo% (no premature repush), post-close the narrowed commit persists —
/// which equals the wholesale scratch here because live is otherwise
/// untouched (narrow==wholesale pin). (Adoption + U14 repush + save:
/// unit-pinned by `prefs_close_commits_once_with_solo_repush` and
/// `prefs_general_rows_apply_per_row`; Lyrics/Piano rows by
/// `prefs_commit_applies_lyrics_rows_without_touching_filters` and
/// `prefs_piano_rows_adopt_and_persist`.)
#[test]
fn close_commits_once_with_solo_repush() {
    let mut h = Harness::new("prf-commit");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 3;
    next.solo_volume_reduction = 75;
    next.auto_advance = false;
    next.sysex_reset = 2;
    next.instrument_map = 1;
    next.future_color = "blue".to_string();
    next.lyrics_font = "Monospace Bold 9".to_string();
    next.text_alignment = 2;
    next.highlight_palette = 0;
    next.single_color = "red".to_string();
    next.velocity_color = false;
    next.names_visibility = 3;
    next.octave_subscript = true;
    h.upd(Message::PrefsScratchEdited(next.clone()));

    // Pre-close: live solo% untouched (no premature repush). The probe
    // abandons the prefs shell (reseed), so reopen + resend.
    h.persist_live_settings();
    assert_eq!(
        persisted_settings(&h, "prf-commit").solo_volume_reduction,
        50
    );
    h.upd(Message::ShowDialog(Dialog::Preferences));
    h.upd(Message::PrefsScratchEdited(next.clone()));

    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "prf-commit"), next);
}

/// Picker updates route per row: with all three scratch texts garbage (models
/// keep last-good seeds), `Reset` on one row restores that row's seed while
/// the other rows commit their garbage verbatim; `Toggle`/`Cancel` on the
/// other rows write no text. (Per-row active flags + apply matrix:
/// unit-pinned by `prefs_picker_updates_route_per_row`.)
#[test]
fn picker_updates_route_per_row() {
    // Past row: toggle future, cancel highlight, reset past.
    let mut h = Harness::new("prf-pick-p");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.future_color = "notacolor-f".to_string();
    next.past_color = "notacolor-p".to_string();
    next.highlight_color = "notacolor-h".to_string();
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::PrefsPickerFuture(
        ColorPickerUpdate::ToggleColorPicker,
    ));
    h.upd(Message::PrefsPickerHighlight(ColorPickerUpdate::Cancel));
    h.upd(Message::PrefsPickerPast(ColorPickerUpdate::Reset));
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-pick-p");
    assert_color_near(&got.past_color, "#1c71d8", "past");
    assert_eq!(got.future_color, "notacolor-f");
    assert_eq!(got.highlight_color, "notacolor-h");

    // Future row: toggle past, cancel highlight, reset future.
    let mut h = Harness::new("prf-pick-f");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.future_color = "notacolor-f".to_string();
    next.past_color = "notacolor-p".to_string();
    next.highlight_color = "notacolor-h".to_string();
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::PrefsPickerPast(
        ColorPickerUpdate::ToggleColorPicker,
    ));
    h.upd(Message::PrefsPickerHighlight(ColorPickerUpdate::Cancel));
    h.upd(Message::PrefsPickerFuture(ColorPickerUpdate::Reset));
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-pick-f");
    assert_color_near(&got.future_color, "#888888", "future");
    assert_eq!(got.past_color, "notacolor-p");
    assert_eq!(got.highlight_color, "notacolor-h");

    // Highlight row: toggle future, cancel past, reset highlight.
    let mut h = Harness::new("prf-pick-h");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.future_color = "notacolor-f".to_string();
    next.past_color = "notacolor-p".to_string();
    next.highlight_color = "notacolor-h".to_string();
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::PrefsPickerFuture(
        ColorPickerUpdate::ToggleColorPicker,
    ));
    h.upd(Message::PrefsPickerPast(ColorPickerUpdate::Cancel));
    h.upd(Message::PrefsPickerHighlight(ColorPickerUpdate::Reset));
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-pick-h");
    assert_color_near(&got.highlight_color, "#e01b24", "highlight");
    assert_eq!(got.future_color, "notacolor-f");
    assert_eq!(got.past_color, "notacolor-p");
}

/// Scratch D3 edits are dropped at close (T12 devil-BLOCKING FIX — no D2
/// row writes them): the live output selection survives byte-identically
/// while the scratch D2 edits still commit. (The `PrefsChanged`-with-D3-delta
/// reselect + announce contract stays e2e-pinned by the unit half
/// `prefs_backend_switch_reseats_and_announces_e2e` — the headless engine's
/// `OutputChanged` answer sits unpumped here, so no announce toast can be
/// observed through `update`.)
#[test]
fn scratch_d3_edits_dropped_live_output_survives() {
    let mut h = Harness::new("prf-be");
    let mut base = Settings::new();
    base.last_output_backend = "ALSA".to_string();
    base.last_output_connection = "128:0".to_string();
    base.sound_font = "/sf/live.sf2".to_string();
    base.advanced_ports = true;
    h.upd(Message::PrefsChanged(base));
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.last_output_backend = "Dummy".to_string();
    next.last_output_connection = "dummy".to_string();
    next.sound_font = "/sf/scratch.sf2".to_string();
    next.advanced_ports = false;
    next.drums_channel = 3;
    next.solo_volume_reduction = 75;
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-be");
    // D2 edits committed; D3 keys came from live, not scratch.
    assert_eq!(got.drums_channel, 3);
    assert_eq!(got.solo_volume_reduction, 75);
    assert_eq!(got.last_output_backend, "ALSA");
    assert_eq!(got.last_output_connection, "128:0");
    assert_eq!(got.sound_font, "/sf/live.sf2");
    assert!(got.advanced_ports);
}

/// Switching dialogs abandons the scratch: edits never commit, the close
/// persists nothing, and live settings survive untouched. (Unit-pinned by
/// `prefs_arms_without_shell_are_silent_noops`.)
#[test]
fn dialog_switch_abandons_scratch() {
    let mut h = Harness::new("prf-abandon");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 1;
    next.future_color = "blue".to_string();
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::ShowDialog(Dialog::About));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());

    // Live probe: nothing leaked (no shell to restore here).
    h.persist_live_settings();
    assert_eq!(persisted_settings(&h, "prf-abandon"), Settings::new());
}

/// Cancel-less close (C++ parity — the dialog has a Close row, no Cancel
/// path): even an unedited close writes the full config. (Chrome shape:
/// covered by `view_builds_channels_page_and_prefs_dialog`; commit-on-close
/// by `prefs_scratch_edits_are_isolated_until_close`.)
#[test]
fn close_is_cancel_less_always_commits() {
    let mut h = Harness::new("prf-nocancel");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    h.upd(Message::CloseDialog);
    assert!(h.config_exists());
    assert_eq!(persisted_settings(&h, "prf-nocancel"), Settings::new());
}

/// D3-carried keys (`lastOutputBackend` / `lastOutputConnection` /
/// `soundFont` / `advancedPorts`) ride the commit unchanged through a
/// lyrics-only edit — the D2 shell writes no D3 row, and the close arm takes
/// D3 from live. (No-reseat on unchanged keys: unit-pinned by
/// `prefs_general_rows_apply_per_row` and
/// `prefs_soundfont_and_advanced_adopt_without_reselect`.)
#[test]
fn d3_carried_keys_survive_lyrics_only_edit() {
    let mut h = Harness::new("prf-d3");
    let mut base = Settings::new();
    base.last_output_backend = "ALSA".to_string();
    base.last_output_connection = "128:0".to_string();
    base.sound_font = "/sf/keep.sf2".to_string();
    base.advanced_ports = true;
    base.drums_channel = 5;
    base.solo_volume_reduction = 33;
    h.upd(Message::PrefsChanged(base.clone()));
    assert_eq!(persisted_settings(&h, "prf-d3"), base);

    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = base.clone();
    next.future_color = "blue".to_string();
    h.upd(Message::PrefsScratchEdited(next.clone()));
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-d3");
    assert_eq!(got, next);
    assert_eq!(got.last_output_backend, "ALSA");
    assert_eq!(got.last_output_connection, "128:0");
    assert_eq!(got.sound_font, "/sf/keep.sf2");
    assert!(got.advanced_ports);
}

/// Race DEAD, resize leg (T12 devil-BLOCKING FIX): a `WindowResized` live
/// write while prefs are open (OS-level — needs no click, so no capture
/// layer can stop it) survives the close, AND the scratch D2 edits commit.
/// (Private-state triple incl. the `PlaylistSaved` + `on_opened` legs:
/// unit-pinned by `prefs_close_keeps_live_writes_and_drops_scratch_d3`.)
#[test]
fn live_window_resize_survives_prefs_close() {
    let mut h = Harness::new("prf-race-w");
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 3;
    next.solo_volume_reduction = 75;
    next.future_color = "blue".to_string();
    h.upd(Message::PrefsScratchEdited(next));
    h.upd(Message::WindowResized(800, 600));
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-race-w");
    assert_eq!(got.window_width, 800);
    assert_eq!(got.window_height, 600);
    assert_eq!(got.drums_channel, 3);
    assert_eq!(got.solo_volume_reduction, 75);
    assert_eq!(got.future_color, "blue");
}

/// Race DEAD, song-load leg (T12 devil-BLOCKING FIX): an `on_opened` live
/// write while prefs are open (`recentFiles` + `lastDirectory` from the
/// current playlist item) survives the close, AND the scratch D2 edits
/// commit. (Private-state triple incl. the resize + `PlaylistSaved` legs:
/// unit-pinned by `prefs_close_keeps_live_writes_and_drops_scratch_d3`.)
#[test]
fn live_recent_files_survive_prefs_close() {
    let mut h = Harness::new("prf-race-r");
    let song = h.scratch().join("race.mid");
    std::fs::write(&song, b"fixture bytes are never parsed").unwrap();
    h.upd(Message::OpenFiles(vec![song.clone()], true));
    h.upd(Message::ShowDialog(Dialog::Preferences));
    let mut next = Settings::new();
    next.drums_channel = 7;
    next.single_color = "red".to_string();
    h.upd(Message::PrefsScratchEdited(next));
    // Synthetic engine answer for the in-flight load (same pattern as the
    // T10c `probe_current_file` helper): `on_opened` records the recents.
    h.upd(Message::Player(PlayerEvent::Opened(Box::new(
        SongSnapshot {
            title: "race.mid".to_string(),
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
    h.upd(Message::CloseDialog);
    let got = persisted_settings(&h, "prf-race-r");
    assert_eq!(got.recent_files, vec![canon(&song)]);
    assert_eq!(
        got.last_directory,
        canon(&song)
            .rsplit_once('/')
            .map_or(String::new(), |(parent, _)| parent.to_string())
    );
    assert_eq!(got.drums_channel, 7);
    assert_eq!(got.single_color, "red");
}
