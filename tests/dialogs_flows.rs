//! T13d integration: remaining-dialog flows through `App::update` + state.
//!
//! Same hermetic harness as T10c/T11c/T12c (temp HOME, per-test portable
//! config, headless engine, synthetic `Opened`). `App` fields are private,
//! so the dialog arms pin behavior through the portable config (D3 commits),
//! the song-`.cfg` probe (manual song-settings), and filesystem silence
//! (scratch edits, routing-only dialogs), plus the pure
//! [`dmidiplayer::state::dialogs`] / `midi` halves and the view-model
//! builders. Toast text, transient D3 status, dialog markers, and the engine
//! send order stay unit-pinned in `app.rs` (named per test); pixel output is
//! never matched.

mod common;

use std::path::PathBuf;

use common::{Harness, hermetic_home};
use dmidiplayer::config::paths::{Env, Paths};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::config::song::SongSettings;
use dmidiplayer::core::event::TextType;
use dmidiplayer::core::sequence::TextRec;
use dmidiplayer::message::{Dialog, LoadedSong, Message, MidiPort, PlayerEvent, SongSnapshot};
use dmidiplayer::state::dialogs::{
    ABOUT, FILE_INFO_FIELDS, FileInfo, HELP_TEXT, HELP_TITLE, LoopEntry, LoopEntryAction,
    assemble_file_info, clamp_jump_bar, file_info_empty, format_file_duration_ms,
    format_initial_tempo_us, jump_allowed, jump_range, loop_confirm, loop_entry_action, loop_seeds,
    manual_song_action_allowed, plan_jump,
};
use dmidiplayer::state::midi::{
    MIDI_SETUP_FIELDS, MidiState, NO_DESTINATIONS_HINT, NO_MIDI_OUTPUT, TEST_NOTE_CH,
    TEST_NOTE_OFF_DELAY_MS, TEST_NOTE_PITCH, TEST_NOTE_STATUS, TEST_NOTE_VEL, plan_test_note,
    profile_for_port, refill_hint, select_refilled_port,
};
use dmidiplayer::ui::{chrome, info_dialogs, jump_loop, menu, midi_setup};

/// 16-bar snapshot (the D7/D8 tests clamp against bar 16). Titles are unique
/// per test so parallel tests never share a song `.cfg`.
fn snapshot(title: &str) -> SongSnapshot {
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
    }
}

/// Feed a synthetic `Opened` for `title` (like T10c: the headless engine's
/// own `Opened` sits unpumped). Never mixed with `probe_current_file` (its
/// `Opened` would reset the state under test).
fn open_song(h: &mut Harness, title: &str) {
    h.upd(Message::Player(PlayerEvent::Opened(Box::new(snapshot(
        title,
    )))));
}

/// Song-`.cfg` path for a synthetic title (same `<title>.cfg` form `Paths`
/// uses under the hermetic data dir).
fn song_cfg(title: &str) -> PathBuf {
    hermetic_home()
        .join(".dmidiplayer")
        .join(format!("{title}.cfg"))
}

/// Parsed song `.cfg` for `title` (the manual song-settings probe).
fn song_settings_cfg(title: &str) -> SongSettings {
    SongSettings::load(&song_cfg(title))
        .unwrap_or_else(|| panic!("T13d: expected a song cfg for {title}"))
}

/// Whole persisted portable config through the real reader (the commit
/// probe; same shape as the T12c prefs probe).
fn persisted_settings(h: &Harness, tag: &str) -> Settings {
    let home = hermetic_home();
    let env = Env::for_test(home, home, home, home);
    let paths = Paths::with_env(env, Some(Some(h.scratch().join(format!("{tag}.conf")))));
    Settings::load(&paths)
}

/// Two refill ports with profile-bearing labels (MT-32 + SC-55).
fn profile_ports() -> Vec<MidiPort> {
    vec![
        MidiPort {
            id: "128:0".to_string(),
            label: "Munt MT-32:Standard".to_string(),
        },
        MidiPort {
            id: "129:0".to_string(),
            label: "Virtual SC55".to_string(),
        },
    ]
}

/// T13g F4: refresh results carry the requesting backend+advanced echo.
/// These flows never switch backend/`Show all` before the refresh, so the
/// echo is the live default (FluidSynth, non-advanced).
fn refreshed(ports: Vec<MidiPort>) -> Message {
    Message::MidiPortsRefreshed {
        backend: "FluidSynth".to_string(),
        advanced: false,
        ports,
    }
}

/// D3 open seeds scratch from live: a no-change Apply persists the live
/// settings byte-identically (a zeroed/default scratch would persist
/// defaults instead) while sending only the state-only sysex-reset to the
/// engine (no `Stop`, no reselect — T13g F2). (Seed: unit-pinned by
/// `midi_open_seeds_scratch_and_abandons_on_close`; no-stop/no-reselect by
/// `midi_apply_no_change_sends_only_sysex_reset`.)
#[test]
fn midi_open_seeds_scratch_from_live() {
    let mut h = Harness::new("dlg-seed");
    let mut base = Settings::new();
    base.last_output_backend = "ALSA".to_string();
    base.last_output_connection = "128:0".to_string();
    base.sound_font = "/sf/live.sf2".to_string();
    base.advanced_ports = true;
    base.instrument_map = 2;
    base.sysex_reset = 3;
    h.upd(Message::PrefsChanged(base.clone()));
    assert_eq!(persisted_settings(&h, "dlg-seed"), base);

    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    // T14d minor d: Apply reads the DROPDOWN — land the refill (ALSA echo,
    // matching the seeded scratch) so the shown id is the live want.
    h.upd(Message::MidiPortsRefreshed {
        backend: "ALSA".to_string(),
        advanced: true,
        ports: vec![MidiPort {
            id: "128:0".to_string(),
            label: "Munt MT-32:Standard".to_string(),
        }],
    });
    h.upd(Message::MidiApply);
    assert_eq!(persisted_settings(&h, "dlg-seed"), base);
}

/// D3 refill stores ports without inference (the C++ `filling` guard): a
/// refill carrying MT-32/SC-55 labels leaves the scratch map/reset at their
/// edited values, and the stored ports answer a later pick (which proves
/// they were stored — unknown ids never infer). (Unit-pinned by
/// `midi_port_pick_infers_profile_refill_does_not`.)
#[test]
fn midi_refill_stores_ports_without_inference() {
    let mut h = Harness::new("dlg-refill");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    // Scratch map/reset deliberately off the refill labels' profiles.
    h.upd(Message::MidiInstrumentMapChanged(1));
    h.upd(Message::MidiResetChanged(0));
    h.upd(refreshed(profile_ports()));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-refill");
    assert_eq!(got.instrument_map, 1, "T13d: refill must not infer map");
    assert_eq!(got.sysex_reset, 0, "T13d: refill must not infer reset");

    // The refill stored the ports: picking one now infers its profile.
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(profile_ports()));
    h.upd(Message::MidiPortSelected("128:0".to_string()));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-refill");
    assert_eq!(got.instrument_map, 2);
    assert_eq!(got.sysex_reset, 4);
    assert_eq!(got.last_output_connection, "128:0");
}

/// D3 port pick infers map+reset from the port label (MT-32 → 2/4, SC-55 →
/// 1/2); an unknown id stores the want only, keeping the last profile.
/// (Unit-pinned by `midi_port_pick_infers_profile_refill_does_not`.)
#[test]
fn midi_port_pick_infers_profile() {
    // MT-32 pick.
    let mut h = Harness::new("dlg-prof-m");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(profile_ports()));
    h.upd(Message::MidiPortSelected("128:0".to_string()));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-prof-m");
    assert_eq!((got.instrument_map, got.sysex_reset), (2, 4));

    // SC-55 pick.
    let mut h = Harness::new("dlg-prof-s");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(profile_ports()));
    h.upd(Message::MidiPortSelected("129:0".to_string()));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-prof-s");
    assert_eq!((got.instrument_map, got.sysex_reset), (1, 2));

    // Unknown id: no inference — the scratch profile survives.
    let mut h = Harness::new("dlg-prof-u");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(profile_ports()));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::MidiResetChanged(4));
    h.upd(Message::MidiPortSelected("nope".to_string()));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-prof-u");
    assert_eq!((got.instrument_map, got.sysex_reset), (2, 4));

    // Pure profile half (the exact code the pick arm calls).
    assert_eq!(profile_for_port("Munt MT-32:Standard"), (2, 4));
    assert_eq!(profile_for_port("Virtual SC55"), (1, 2));
}

/// D3 SoundFont browse + chosen: the browse launches a chooser task (dropped
/// headless — no file appears), `Chosen(Ok)` commits the entry (Apply
/// persists it), `Chosen(Err)` writes nothing. (Unit-pinned by
/// `midi_soundfont_chosen_commits_entry_errors_toast`.)
#[test]
fn midi_soundfont_browse_and_chosen() {
    let mut h = Harness::new("dlg-sf");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    let before: Vec<PathBuf> = std::fs::read_dir(h.scratch())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    h.upd(Message::MidiSoundFontBrowse);
    let after: Vec<PathBuf> = std::fs::read_dir(h.scratch())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    assert_eq!(before, after, "T13d: no chooser runs headless");
    assert!(!h.config_exists());

    h.upd(Message::MidiSoundFontChosen(Err(
        "Cannot open x: not a local file".to_string(),
    )));
    assert!(!h.config_exists());

    h.upd(Message::MidiSoundFontChosen(Ok(PathBuf::from(
        "/sf/picked.sf2",
    ))));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "dlg-sf").sound_font,
        "/sf/picked.sf2"
    );
}

/// D3 test-note flow: Test writes no config (commit is memory-only), the
/// dialog stays open (a following Apply still commits), the release routes
/// silently, and an emptied entry commits verbatim. The pure plan pins stop
/// + verbatim SoundFont + unconditional reselect + ch0/C4/96 + 400 ms, and
/// the status string. (Memory commit + stay-open + announce: unit-pinned by
/// `midi_test_note_commits_stays_open_and_announces_e2e`; empty-verbatim by
/// `midi_test_note_empty_soundfont_commits_verbatim`; silent release by
/// `midi_test_note_off_is_silent`; status overwrite on failure by
/// `midi_test_note_failure_surfaces_no_output_status`.)
#[test]
fn midi_test_note_flow() {
    let mut h = Harness::new("dlg-test");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    h.upd(Message::MidiSoundFontChanged(
        "/sf/audition.sf2".to_string(),
    ));
    h.upd(Message::MidiTestNote);
    assert!(!h.config_exists(), "T13d: Test never saves");
    // T14d minor c: the release carries the captured epoch (startup 1 +
    // Test 1 = 2 — the silence asserts below hold for either path).
    h.upd(Message::MidiTestNoteOff {
        ch: 0,
        note: 60,
        epoch: 2,
    });
    assert!(!h.config_exists());
    // Stay-open: Apply after Test still commits the scratch entry.
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "dlg-test").sound_font,
        "/sf/audition.sf2"
    );

    // Emptied entry commits verbatim (even empty — C++ assigns
    // unconditionally): live `/sf/live.sf2` clears through Test + Apply.
    let mut h = Harness::new("dlg-test-e");
    let mut base = Settings::new();
    base.sound_font = "/sf/live.sf2".to_string();
    h.upd(Message::PrefsChanged(base));
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiSoundFontChanged(String::new()));
    h.upd(Message::MidiTestNote);
    h.upd(Message::MidiApply);
    assert!(
        persisted_settings(&h, "dlg-test-e").sound_font.is_empty(),
        "T13d: empty entry commits verbatim"
    );

    // Pure plan half (the exact code the Test arm calls).
    let plan = plan_test_note("FluidSynth", "fluidsynth", "/sf/x.sf2");
    assert!(plan.stop_first);
    assert_eq!(plan.soundfont, "/sf/x.sf2");
    assert_eq!(
        plan.select_output,
        ("FluidSynth".to_string(), "fluidsynth".to_string())
    );
    assert_eq!(plan.note_on, (TEST_NOTE_CH, TEST_NOTE_PITCH, TEST_NOTE_VEL));
    assert_eq!(plan.note_on, (0, 60, 96));
    assert_eq!(plan.off_delay_ms, TEST_NOTE_OFF_DELAY_MS);
    assert_eq!(plan.off_delay_ms, 400);
    assert_eq!(TEST_NOTE_STATUS, "Sent Middle C on channel 1");
}

/// D3 Apply commits the scratch (backend/port/font/advanced/map/reset),
/// persists, and closes: a post-Apply scratch edit is dropped (no shell),
/// so a reopen+Apply persists the committed values unchanged. Selections
/// clamp (map 0–2, reset 0–4). (Send order
/// `SetSysexReset → Stop → SetSoundFont → SelectOutput` (T13g F1:
/// stop-before-font, behaviorally EQUIVALENT to C++ `:1172-1175` — both
/// stage the font, `sfload` runs at open; T14d minor a rewords the old
/// "safer" gloss) + announce:
/// unit-pinned by `midi_apply_persists_and_announces_e2e` and
/// `midi_apply_changed_stops_first_then_reselects_e2e`; clamps by
/// `midi_selections_stored_and_clamped`; shell-less no-op by
/// `midi_arms_noop_without_shell`.)
#[test]
fn midi_apply_commits_saves_and_closes() {
    let mut h = Harness::new("dlg-apply");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiBackendSelected("ALSA".to_string()));
    // T13g F4: the echo matches the just-selected backend (pre-toggle).
    h.upd(Message::MidiPortsRefreshed {
        backend: "ALSA".to_string(),
        advanced: false,
        ports: profile_ports(),
    });
    h.upd(Message::MidiPortSelected("128:0".to_string()));
    h.upd(Message::MidiSoundFontChanged("/sf/x.sf2".to_string()));
    h.upd(Message::MidiAdvancedToggled(true));
    // T14d minor d: the toggle cleared the ports (refill retrigger) — land
    // the post-toggle refill (ALSA echo, advanced now true) so Apply reads
    // the populated dropdown like C++.
    h.upd(Message::MidiPortsRefreshed {
        backend: "ALSA".to_string(),
        advanced: true,
        ports: profile_ports(),
    });
    h.upd(Message::MidiInstrumentMapChanged(9));
    h.upd(Message::MidiResetChanged(9));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-apply");
    assert_eq!(got.last_output_backend, "ALSA");
    assert_eq!(got.last_output_connection, "128:0");
    assert_eq!(got.sound_font, "/sf/x.sf2");
    assert!(got.advanced_ports);
    assert_eq!(got.instrument_map, 2, "T13d: map clamps 9 → 2");
    assert_eq!(got.sysex_reset, 4, "T13d: reset clamps 9 → 4");

    // Closed: the shell is gone, so these edits drop on the floor — the
    // reopen seeds the committed values from the synced live mirrors.
    // (Map/reset re-pin: T13d finding F1 is fixed — Apply now syncs those
    // mirrors — so the reopen seeds 2/4 and the no-change Apply persists
    // them instead of clobbering with the stale 0/1.)
    h.upd(Message::MidiBackendSelected("Dummy".to_string()));
    h.upd(Message::MidiInstrumentMapChanged(0));
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    // T14d minor d: the reopen seeds empty ports (live never carries the
    // list) — land the refill so the steady-state Apply reads 128:0.
    h.upd(Message::MidiPortsRefreshed {
        backend: "ALSA".to_string(),
        advanced: true,
        ports: profile_ports(),
    });
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-apply");
    assert_eq!(
        got.last_output_backend, "ALSA",
        "T13d: post-close edit dropped"
    );
    assert_eq!(got.last_output_connection, "128:0");
    assert_eq!(got.instrument_map, 2, "T13d/F1: reopen seeds synced map");
    assert_eq!(got.sysex_reset, 4, "T13d/F1: reopen seeds synced reset");
}

/// D3 map/reset-only Apply persists without reseating: the commit lands in
/// the config while backend/connection stay put (no `SelectOutput`, no
/// `Stop` — the stop-first delta; only the state-only sysex-reset send,
/// T13g F2). (No-stop/no-reselect: unit-pinned by
/// `midi_apply_map_only_sends_only_sysex_reset_but_persists`.)
#[test]
fn midi_apply_map_reset_only_persists_without_reselect() {
    let mut h = Harness::new("dlg-maponly");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::MidiResetChanged(4));
    // T14d minor d: land the steady-state refill (shown == want).
    h.upd(refreshed(vec![MidiPort {
        id: "fluidsynth".to_string(),
        label: "FluidSynth".to_string(),
    }]));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-maponly");
    assert_eq!(got.instrument_map, 2);
    assert_eq!(got.sysex_reset, 4);
    assert_eq!(got.last_output_backend, "FluidSynth");
    assert_eq!(got.last_output_connection, "fluidsynth");
}

/// `OutputChanged` warning with an open D3 shell: the live mirrors follow
/// the event while the scratch edit is abandoned at close (reopen+Apply
/// persists the warned mirrors, not the scratch). The status-line read path
/// pins `NO_MIDI_OUTPUT` over the transient slot and the refill hint over an
/// empty port list. (Scratch status overwrite + toast: unit-pinned by
/// `midi_test_note_failure_surfaces_no_output_status`.)
#[test]
fn midi_output_changed_warning_with_open_shell() {
    let mut h = Harness::new("dlg-warn");
    // A clean answer first: confirms an output (no fallback chain below).
    h.upd(Message::Player(PlayerEvent::OutputChanged {
        backend: "ALSA".to_string(),
        port: "128:0".to_string(),
        warning: None,
    }));
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::Player(PlayerEvent::OutputChanged {
        backend: "Dummy".to_string(),
        port: "dummy".to_string(),
        warning: Some("boom".to_string()),
    }));
    // Close abandons the scratch edit; the warned mirrors survive in live.
    h.upd(Message::CloseDialog);
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    // T14d minor d: land the refill (Dummy echo, matching the warned
    // mirrors) so the steady-state Apply reads `dummy`.
    h.upd(Message::MidiPortsRefreshed {
        backend: "Dummy".to_string(),
        advanced: false,
        ports: vec![MidiPort {
            id: "dummy".to_string(),
            label: "Dummy (no output)".to_string(),
        }],
    });
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-warn");
    assert_eq!(got.last_output_backend, "Dummy");
    assert_eq!(got.last_output_connection, "dummy");
    assert_eq!(
        got.instrument_map, 0,
        "T13d: scratch edit abandoned at close"
    );

    // Status-line read path (the builder the sheet renders).
    assert_eq!(NO_MIDI_OUTPUT, "No MIDI output");
    let shown = midi_setup::view_model(&MidiState::new(), Some(NO_MIDI_OUTPUT));
    assert_eq!(shown.status, "No MIDI output");
    let hinted = midi_setup::view_model(&MidiState::new(), None);
    assert_eq!(hinted.status, NO_DESTINATIONS_HINT);
    assert_eq!(refill_hint(true), Some(NO_DESTINATIONS_HINT));
    assert_eq!(refill_hint(false), None);
    assert_eq!(select_refilled_port(&profile_ports(), "128:0"), 0);
}

/// D3 cancel abandons the scratch: edits never commit, a bare close writes
/// nothing on a fresh harness, and `MidiApply` with no shell is a silent
/// no-op. (Unit-pinned by `midi_open_seeds_scratch_and_abandons_on_close`
/// and `midi_arms_noop_without_shell`.)
#[test]
fn midi_cancel_abandons_scratch() {
    // Fresh harness: open + edit + cancel writes nothing, and a
    // shell-less Apply afterwards is a silent no-op.
    let mut h = Harness::new("dlg-cancel");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiBackendSelected("ALSA".to_string()));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::MidiSoundFontChanged("/sf/scratch.sf2".to_string()));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
    h.upd(Message::MidiApply);
    assert!(!h.config_exists());

    // Live values survive a cancelled edit session byte-identically.
    let mut h = Harness::new("dlg-cancel-l");
    let mut base = Settings::new();
    base.last_output_backend = "ALSA".to_string();
    base.sound_font = "/sf/live.sf2".to_string();
    h.upd(Message::PrefsChanged(base.clone()));
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiBackendSelected("Dummy".to_string()));
    h.upd(Message::MidiSoundFontChanged("/sf/scratch.sf2".to_string()));
    h.upd(Message::CloseDialog);
    assert_eq!(persisted_settings(&h, "dlg-cancel-l"), base);
}

/// D7 gate + clamp + sequencing. Gate: with no song the jump trigger leaves
/// a D3 shell untouched (the gated switch abandons nothing — a following
/// Apply still commits the scratch edit); with a song the switch opens D7
/// and abandons D3 (Apply no-ops). Clamp: OOR bars clamp to `1..=last_bar`
/// (pure halves + builder). Sequencing: pause-jump-resume while playing,
/// jump-alone while idle (pure plan + silent routing both legs).
/// (Unit-pinned by `jump_dialog_gates_on_song_and_seeds_bar_1`,
/// `jump_to_bar_confirms_and_closes_dialog`, `jump_clamp_and_availability`,
/// `jump_plan_pauses_only_when_playing`.)
#[test]
fn jump_gate_clamp_and_sequencing() {
    // No song: the gated trigger abandons nothing — the D3 shell survives.
    let mut h = Harness::new("dlg-jump-g");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::MidiApply);
    assert_eq!(
        persisted_settings(&h, "dlg-jump-g").instrument_map,
        2,
        "T13d: gated D7 switch must not abandon the D3 shell"
    );

    // Song loaded: the switch opens D7 and abandons the D3 shell.
    let mut h = Harness::new("dlg-jump-o");
    open_song(&mut h, "dlg-jump-o.mid");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::MidiApply);
    assert!(!h.config_exists(), "T13d: D7 open abandons the D3 shell");

    // Clamp halves (the arm clamps like the C++ spin).
    assert_eq!(jump_range(16), (1, 16));
    assert_eq!(jump_range(0), (1, 1));
    assert!(!jump_allowed(false));
    assert!(jump_allowed(true));
    assert_eq!(clamp_jump_bar(999, 16), 16);
    assert_eq!(clamp_jump_bar(0, 16), 1);
    assert_eq!(clamp_jump_bar(7, 16), 7);
    assert_eq!(jump_loop::jump_view_model(999, 16).bar, 16);
    assert_eq!(jump_loop::jump_view_model(0, 16).bar, 1);

    // Sequencing plan: pause + resume ride on `playing`.
    let playing = plan_jump(4, true);
    assert!(playing.pause_first && playing.resume_after);
    assert_eq!(playing.bar, 4);
    let idle = plan_jump(4, false);
    assert!(!idle.pause_first && !idle.resume_after);

    // Both legs route through `update` persisting nothing.
    let mut h = Harness::new("dlg-jump-x");
    open_song(&mut h, "dlg-jump-x.mid");
    h.upd(Message::JumpBarEdited(7));
    h.upd(Message::JumpToBar(999)); // idle leg: clamps to 16 (unit-pinned)
    assert!(!h.config_exists());
    h.upd(Message::Play); // optimistic `Playing`
    h.upd(Message::JumpToBar(4)); // playing leg: pause-jump-resume sends
    assert!(!h.config_exists());
    h.upd(Message::Stop);
}

/// D8 seeds + no-ordering + U3 menu path + cancel. The `ShowDialog(Loop)`
/// switch opens and abandons the D3 shell (dialog-switch rule — Apply
/// no-ops); the `LoopToggled(true)` toggle/menu path opens too (no-song
/// included — the open marker itself stays unit-pinned). Seeds clamp OOR
/// playback values; confirm clamps each end with NO from≤to enforcement
/// (C++ `setLoop` assigns blindly). Cancel resets an unconfirmed toggle;
/// `LoopSet` keeps it ON (pure rule + silent routing). (Unit-pinned by
/// `loop_toggle_forces_on_while_dialog_open_u3`,
/// `loop_dialog_seeds_from_playback_and_stores_scratch`,
/// `loop_set_clamps_each_end_without_ordering`,
/// `loop_dialog_cancel_resets_unconfirmed_toggle`,
/// `loop_set_enables_range_and_closes`, `loop_toggle_opens_dialog_off_disables`.)
#[test]
fn loop_seeds_no_ordering_menu_cancel() {
    // D8 opens on the switch path and abandons the D3 shell: Apply no-ops.
    let mut h = Harness::new("dlg-loop-sw");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::ShowDialog(Dialog::Loop));
    h.upd(Message::MidiApply);
    assert!(!h.config_exists(), "T13d: D8 switch abandons the D3 shell");
    h.upd(Message::CloseDialog);

    // U3 toggle/menu path with NO song: routes silently (the open marker
    // incl. the no-song deviation is unit-pinned).
    let mut h = Harness::new("dlg-loop-u3");
    h.upd(Message::LoopToggled(true));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
    // T13f (F2 mirror): toggle-entry abandons the D3 shell like the switch path.
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::LoopToggled(true));
    h.upd(Message::MidiApply);
    assert!(
        !h.config_exists(),
        "T13f: toggle-entry abandons the D3 shell"
    );

    // Entry routing: toggle-OFF disables directly, menu always opens.
    assert_eq!(
        loop_entry_action(LoopEntry::Toggle(false)),
        LoopEntryAction::DisableLoop
    );
    assert_eq!(
        loop_entry_action(LoopEntry::Toggle(true)),
        LoopEntryAction::OpenDialog
    );
    assert_eq!(
        loop_entry_action(LoopEntry::Menu),
        LoopEntryAction::OpenDialog
    );

    // Seeds + confirm: clamp each end, never order them.
    assert_eq!(loop_seeds(3, 12, 16), (3, 12));
    assert_eq!(loop_seeds(0, 999, 16), (1, 16));
    assert_eq!(loop_confirm(8, 3, 16), (8, 3));
    assert_eq!(loop_confirm(-4, 999, 16), (1, 16));
    let vm = jump_loop::loop_view_model(0, 999, 16);
    assert_eq!((vm.from, vm.to, vm.last_bar), (1, 16, 16));

    // Cancel rule (unconfirmed resets, confirmed keeps ON) is model-derived
    // — no state helper (T14a removed the dead `loop_cancel_toggle`) — and
    // pinned through `update` below + `loop_dialog_cancel_resets_unconfirmed_toggle`
    // in `app.rs`.

    // Full confirm + cancel sequences route silently through `update`.
    let mut h = Harness::new("dlg-loop-x");
    open_song(&mut h, "dlg-loop-x.mid");
    h.upd(Message::LoopToggled(true));
    h.upd(Message::LoopFromEdited(8));
    h.upd(Message::LoopToEdited(3));
    h.upd(Message::LoopSet { start: 8, end: 3 }); // from > to: kept (unit-pinned)
    assert!(!h.config_exists());
    h.upd(Message::LoopToggled(true));
    h.upd(Message::CloseDialog); // cancel a confirmed loop: stays ON (unit-pinned)
    assert!(!h.config_exists());
    h.upd(Message::LoopToggled(false)); // direct disable, no dialog
    assert!(!h.config_exists());
}

/// D6 file-info body assembly: every C++ field in order + metadata pairs +
/// one line per copyright record, the duration/tempo formatters, and the
/// empty state. The builder decodes copyrights from the song texts (lyrics
/// records excluded, event order kept) and both shapes build views. Routing
/// through `update` persists nothing. (Unit-pinned by
/// `file_info_assembly_keeps_order_and_all_fields`,
/// `file_info_empty_metadata_and_copyrights`,
/// `builder_rows_copyrights_and_empty_state`.)
#[test]
fn file_info_body_assembly() {
    assert_eq!(
        FILE_INFO_FIELDS,
        [
            "File",
            "Format",
            "Duration",
            "Initial tempo",
            "Tracks",
            "Division"
        ]
    );
    assert!(matches!(file_info_empty(), FileInfo::Empty));
    assert_eq!(format_file_duration_ms(3_723_005), "01:02:03.005");
    assert_eq!(format_initial_tempo_us(500_000.0), "120.0 bpm");
    assert_eq!(format_initial_tempo_us(0.0), "120.0 bpm");

    let FileInfo::Loaded(data) = assemble_file_info(
        "song.mid",
        "SMF type 1",
        61_001,
        500_000.0,
        4,
        96,
        vec![("INAM".to_string(), "Name".to_string())],
        vec!["© 1999 Anon".to_string()],
    ) else {
        panic!("T13d: expected Loaded");
    };
    assert_eq!(data.title, "song.mid");
    assert_eq!(data.format, "SMF type 1");
    assert_eq!(data.duration, "00:01:01.001");
    assert_eq!(data.initial_tempo, "120.0 bpm");
    assert_eq!(data.num_tracks, 4);
    assert_eq!(data.division, 96);
    assert_eq!(
        data.metadata,
        vec![("INAM".to_string(), "Name".to_string())]
    );
    assert_eq!(data.copyrights, vec!["© 1999 Anon"]);

    // Builder over a real song: copyright records decoded in order,
    // non-copyright records excluded, metadata passed through.
    let mut snap = snapshot("dlg-d6.mid");
    snap.texts = vec![
        TextRec {
            tick: 0,
            track: 0,
            text_type: TextType::Copyright,
            text: b"(c) Example".to_vec(),
        },
        TextRec {
            tick: 0,
            track: 0,
            text_type: TextType::Lyric,
            text: b"la".to_vec(),
        },
        TextRec {
            tick: 0,
            track: 1,
            text_type: TextType::Copyright,
            text: b"(c) Second".to_vec(),
        },
    ];
    snap.duration_ms = 3_723_005;
    let song = LoadedSong::from(snap);
    let metadata = vec![("INAM".to_string(), "Name".to_string())];
    let vm = info_dialogs::file_info_view_model(Some(&song), &metadata);
    let body = vm.body.expect("T13d: song loaded");
    assert_eq!(body.file, "dlg-d6.mid");
    assert_eq!(body.duration, "01:02:03.005");
    assert_eq!(body.copyrights, vec!["(c) Example", "(c) Second"]);
    assert!(
        info_dialogs::file_info_view_model(None, &metadata)
            .body
            .is_none()
    );
    // Both shapes build without panic.
    let _ =
        info_dialogs::file_info_view(info_dialogs::file_info_view_model(Some(&song), &metadata));
    let _ = info_dialogs::file_info_view(info_dialogs::file_info_view_model(None, &[]));

    // Routing through `update` persists nothing.
    let mut h = Harness::new("dlg-d6");
    open_song(&mut h, "dlg-d6x.mid");
    h.upd(Message::ShowFileInfo);
    h.upd(Message::CloseDialog);
    h.upd(Message::ShowDialog(Dialog::FileInfo));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// D5/D4 static content: verbatim spot-checks over the help text and the
/// about table (title, version, links, credits), plus view-builds for both.
/// Routing through `update` persists nothing. (Unit-pinned by
/// `help_text_is_verbatim_khelptext`, `about_strings_are_verbatim_cpp`,
/// `dialogs_view_and_toasts`.)
#[test]
fn help_about_static_content() {
    assert_eq!(HELP_TITLE, "Help");
    assert!(
        HELP_TEXT.starts_with("Gosh MIDI Player is a MIDI file player"),
        "T13d: help must open on the app line"
    );
    assert!(HELP_TEXT.contains("Supported files: .mid .midi .kar .rmi .wrk"));
    assert!(HELP_TEXT.contains("MIDI Setup"));
    assert!(HELP_TEXT.contains("~/.dmidiplayer/<song>.cfg"));

    assert_eq!(ABOUT.app_name, "Gosh MIDI Player");
    assert_eq!(env!("CARGO_PKG_VERSION"), "1.7.5");
    assert_eq!(ABOUT.website, "https://github.com/goshitsarch-eng/goshmidi");
    assert_eq!(ABOUT.support_url, "https://dmidiplayer.sourceforge.io/");
    assert_eq!(
        ABOUT.issue_url,
        "https://github.com/goshitsarch-eng/goshmidi/issues"
    );
    assert_eq!(ABOUT.developers.len(), 2);
    assert_eq!(ABOUT.credit_section, "Original application");
    assert_eq!(ABOUT.links.len(), 3);
    for (_, url) in ABOUT.links {
        assert!(url.starts_with("https://"), "T13d: about link {url}");
    }
    let _ = info_dialogs::help_view();
    let _ = info_dialogs::about_view(|url: &str| Message::OpenUrl(url.to_string()));

    let mut h = Harness::new("dlg-d5d4");
    h.upd(Message::ShowHelp);
    h.upd(Message::CloseDialog);
    h.upd(Message::ShowAbout);
    h.upd(Message::CloseDialog);
    h.upd(Message::ShowDialog(Dialog::Help));
    h.upd(Message::CloseDialog);
    h.upd(Message::ShowDialog(Dialog::About));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// D4 link emission stays hermetic: `OpenUrl` only launches the opener task
/// (dropped headless — `xdg-open` never spawns), `UrlOpened(Ok)` is silent,
/// and `UrlOpened(Err)` routes without persistence (the toast itself is
/// private). (Unit-pinned by `open_url_launches_silently_errors_toast`.)
#[test]
fn open_url_task_emission_hermetic() {
    let mut h = Harness::new("dlg-url");
    h.upd(Message::ShowDialog(Dialog::About));
    // The `update` return (the opener `Task`) is dropped without polling —
    // the hermetic pin: no child process can spawn from a dropped task.
    h.upd(Message::OpenUrl(ABOUT.website.to_string()));
    assert!(!h.config_exists());
    h.upd(Message::UrlOpened(Ok(())));
    assert!(!h.config_exists());
    h.upd(Message::UrlOpened(
        Err("Could not open x: boom".to_string()),
    ));
    assert!(!h.config_exists());
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
}

/// Manual song-settings save/load execute + persist with a song loaded, with
/// the auto flag OFF (ungated): a save records the live mix, a load applies
/// a cfg back over changed live state (re-save proves it). The stub buttons
/// build with and without a song. (Unit-pinned by
/// `manual_song_actions_ungated_by_auto_flag`,
/// `song_settings_save_writes_used_channels_only`,
/// `song_settings_load_applies_and_clamps`.)
#[test]
fn manual_save_load_execute_and_persist() {
    assert!(
        !Settings::new().auto_song_settings,
        "T13d: this test runs with the auto flag OFF"
    );
    let mut h = Harness::new("dlg-manual");
    open_song(&mut h, "dlg-manual.mid");
    h.upd(Message::ChannelMute(0, true));
    h.upd(Message::VolumeChanged(150));
    h.upd(Message::SongSettingsSave);
    let cfg = song_settings_cfg("dlg-manual.mid");
    assert_eq!(cfg.volume, Some(150));
    assert!(cfg.channels[0].as_ref().expect("ch 0 row").muted);

    // Change live state, load the cfg back, re-save: the loaded values win.
    h.upd(Message::ChannelMute(0, false));
    h.upd(Message::VolumeChanged(100));
    h.upd(Message::SongSettingsLoad);
    h.upd(Message::SongSettingsSave);
    let cfg = song_settings_cfg("dlg-manual.mid");
    assert_eq!(cfg.volume, Some(150), "T13d: load must apply the cfg");
    assert!(
        cfg.channels[0].as_ref().expect("ch 0 row").muted,
        "T13d: load must apply the mix"
    );

    // Same run with the auto flag ON (unit half covers both; this pins the
    // save half above the flag too).
    let mut h = Harness::new("dlg-manual-a");
    let mut auto = Settings::new();
    auto.auto_song_settings = true;
    h.upd(Message::PrefsChanged(auto));
    open_song(&mut h, "dlg-manual-a.mid");
    h.upd(Message::SongSettingsSave);
    assert!(song_cfg("dlg-manual-a.mid").is_file());

    // The T13 stub surface is retired (T14d): File menu entries 3–4 are
    // the placement — same labels/messages, pinned by the menu inventory
    // tests (`labels_are_verbatim_cpp`, `messages_map_to_the_existing_variants`).
    assert_eq!(menu::MenuAction::LoadSettings.label(), "Load song settings");
    assert_eq!(menu::MenuAction::SaveSettings.label(), "Save song settings");
    assert!(matches!(
        menu::MenuAction::LoadSettings.message(),
        Message::SongSettingsLoad
    ));
    assert!(matches!(
        menu::MenuAction::SaveSettings.message(),
        Message::SongSettingsSave
    ));
}

/// Manual song-settings empty-name guard: with no song loaded, save writes
/// nothing and load is a silent no-op. (Unit-pinned by
/// `manual_song_actions_noop_on_empty_title`.)
#[test]
fn manual_empty_name_guard_blocks() {
    assert!(!manual_song_action_allowed(None));
    assert!(!manual_song_action_allowed(Some("")));
    assert!(manual_song_action_allowed(Some("song.mid")));

    let mut h = Harness::new("dlg-manual-g");
    h.assert_save_without_song_writes_nothing("guard");
    h.upd(Message::SongSettingsLoad);
    h.assert_save_without_song_writes_nothing("guard-after-load");
    assert!(!h.config_exists());
}

/// Modal chrome (D20): every new dialog sheet builds inside
/// `chrome::wrap_modal` (the capturing Stack + scrim + wheel→`Cancel`
/// shape composition uses — the views are not introspectable, so the pin is
/// build-without-panic over the production sheet shapes). State-level: a
/// live `WindowResized` while D3 is open survives Apply alongside the
/// scratch commit (narrow commit — the D3 arm only writes its 7 keys), and
/// the D7/D8 spin scratch never touches settings (edits + close write
/// nothing; the live probe still reads defaults). (Unit-pinned by
/// `wrap_builds_over_any_base_and_sheet`; narrow-commit precedent by
/// `prefs_close_keeps_live_writes_and_drops_scratch_d3`; scratch halves by
/// `midi_open_seeds_scratch_and_abandons_on_close`,
/// `jump_dialog_gates_on_song_and_seeds_bar_1`,
/// `loop_dialog_seeds_from_playback_and_stores_scratch`.)
#[test]
fn modal_chrome_wrap_and_narrow_commit() {
    assert_eq!(
        MIDI_SETUP_FIELDS,
        [
            "Output backend",
            "Show all ports",
            "Connection (your MIDI synth / Munt / USB port)",
            "Instrument map",
            "System Exclusive reset (sent before each song)",
            "FluidSynth SoundFont (.sf2 / .sf3)",
            "Status",
            "Test note / Apply",
        ]
    );
    // Production sheet shapes (titles + Cancel secondaries per the chrome
    // recipe), each wrapped exactly as `App::view` wraps them.
    let wrap = |sheet: cosmic::Element<'static, Message>| {
        let base: cosmic::Element<'static, Message> = cosmic::widget::text::body("base").into();
        let _ = chrome::wrap_modal(base, sheet);
    };
    wrap(
        cosmic::widget::dialog()
            .title(midi_setup::TITLE)
            .control(midi_setup::view(midi_setup::view_model(
                &MidiState::new(),
                None,
            )))
            .secondary_action(
                cosmic::widget::button::standard("Cancel").on_press(Message::CloseDialog),
            )
            .into(),
    );
    wrap(
        cosmic::widget::dialog()
            .title(jump_loop::JUMP_TITLE)
            .control(jump_loop::jump_view(
                jump_loop::jump_view_model(7, 16),
                Message::JumpBarEdited,
            ))
            .secondary_action(
                cosmic::widget::button::standard("Cancel").on_press(Message::CloseDialog),
            )
            .into(),
    );
    wrap(
        cosmic::widget::dialog()
            .title(jump_loop::LOOP_TITLE)
            .control(jump_loop::loop_view(
                jump_loop::loop_view_model(3, 12, 16),
                Message::LoopFromEdited,
                Message::LoopToEdited,
            ))
            .secondary_action(
                cosmic::widget::button::standard("Cancel").on_press(Message::CloseDialog),
            )
            .into(),
    );
    wrap(
        cosmic::widget::dialog()
            .title(info_dialogs::FILE_INFO_TITLE)
            .control(info_dialogs::file_info_view(
                info_dialogs::file_info_view_model(None, &[]),
            ))
            .into(),
    );
    wrap(
        cosmic::widget::dialog()
            .title(HELP_TITLE)
            .control(info_dialogs::help_view())
            .into(),
    );
    wrap(
        cosmic::widget::dialog()
            .title("About")
            .control(info_dialogs::about_view(|url: &str| {
                Message::OpenUrl(url.to_string())
            }))
            .into(),
    );

    // D3 narrow commit: live resize + scratch edit both persist.
    let mut h = Harness::new("dlg-chrome");
    h.upd(Message::ShowDialog(Dialog::MidiSetup));
    h.upd(Message::MidiInstrumentMapChanged(2));
    h.upd(Message::WindowResized(800, 600));
    h.upd(Message::MidiApply);
    let got = persisted_settings(&h, "dlg-chrome");
    assert_eq!(got.instrument_map, 2);
    assert_eq!((got.window_width, got.window_height), (800, 600));

    // D7/D8 scratch isolation: spin edits never touch settings.
    let mut h = Harness::new("dlg-chrome-s");
    open_song(&mut h, "dlg-chrome-s.mid");
    h.upd(Message::ShowDialog(Dialog::JumpToBar));
    h.upd(Message::JumpBarEdited(9));
    h.upd(Message::CloseDialog);
    h.upd(Message::LoopToggled(true));
    h.upd(Message::LoopFromEdited(5));
    h.upd(Message::LoopToEdited(9));
    h.upd(Message::CloseDialog);
    assert!(!h.config_exists());
    // Live probe: no scratch value leaked into settings (the `on_opened`
    // recents leg is expected — prefs race-R precedent — so the pin is
    // per-key, not whole-struct).
    h.persist_live_settings();
    let got = persisted_settings(&h, "dlg-chrome-s");
    assert_eq!(got.instrument_map, 0);
    assert_eq!(got.sysex_reset, 1);
    assert!(got.sound_font.is_empty());
    assert_eq!(got.last_output_backend, "FluidSynth");
    assert_eq!(got.last_output_connection, "fluidsynth");
}
