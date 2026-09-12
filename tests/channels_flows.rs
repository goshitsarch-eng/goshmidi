//! T12c integration: channels/mix flows through `App::update` + public mix state.
//!
//! Same hermetic harness as T10c/T11c (temp HOME, real `examples/*` fixtures,
//! headless engine, synthetic `Opened` carrying REAL fixture metadata —
//! labels, charset, texts — with `used` flags forced on for the probed rows).
//! `App` fields are private, so the mix arms pin behavior through the
//! song-`.cfg` channel rows (`muted`/`solo`/`locked`/`patch`/`level` recorded
//! by `SongSettingsSave` for used channels only) plus the pure
//! [`dmidiplayer::state::channels`] sync math (`sync_all`: the exact
//! `SetMuted` + `SetChannelVolume` values the arms push — mix commands emit no
//! engine events, so there is no engine-side readback; the sends themselves
//! stay unit-pinned in `app.rs`, named per test). Meter levels are likewise
//! render-only state: routing goes through `update` (no persistence, no
//! slider disturbance) while values go through the public `note_peak` /
//! `decay_meters` halves plus the named unit tests.

mod common;

use std::path::PathBuf;

use common::{Harness, fixture, hermetic_home};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::config::song::SongSettings;
use dmidiplayer::core::event::TextType;
use dmidiplayer::core::sequence::Sequence;
use dmidiplayer::message::{Message, PlayerEvent, SongSnapshot};
use dmidiplayer::state::channels::{
    ChannelState, decay_meters, default_mix, note_peak, patch_label, patch_options, sync_all,
};

/// Load a real example fixture through the public `Sequence` API.
fn load_fixture(name: &str) -> Sequence {
    let mut seq = Sequence::default();
    assert!(
        seq.load_file(&fixture(name)),
        "T12c: fixture {name} must load"
    );
    seq
}

/// Real-metadata `Opened` snapshot (labels/charset/texts from the fixture;
/// the mix arms read `channel_used` + `channel_labels`).
fn snapshot(title: &str, seq: &Sequence) -> SongSnapshot {
    SongSnapshot {
        title: title.to_string(),
        format: seq.file_format().to_string(),
        bars: seq.last_bar(),
        len_ticks: i64::from(seq.song_length_ticks()),
        duration_ms: 60_000,
        division: seq.division(),
        num_tracks: seq.num_tracks(),
        initial_tempo_us: seq.initial_tempo(),
        channel_used: std::array::from_fn(|ch| seq.channel_used(ch as i32)),
        channel_labels: std::array::from_fn(|ch| seq.channel_label(ch as i32)),
        lowest_note: seq.lowest_note(),
        highest_note: seq.highest_note(),
        charset: seq.current_charset().to_string(),
        texts: seq.text_events().to_vec(),
        // SMF fixtures carry no RIFF INFO map (engine-internal either way).
        metadata: Vec::new(),
        copyrights: seq.get_text(TextType::Copyright),
    }
}

/// Feed a real-metadata `Opened` for `title`, force-marking `force_used` rows
/// used so the song-`.cfg` probe records them (`save_song_settings` writes
/// used rows only). Titles are unique per test so parallel tests never share
/// a song `.cfg`. Never mixed with `probe_current_file` (its `Opened` would
/// reset the mix under test).
fn open_mix(h: &mut Harness, title: &str, seq: &Sequence, force_used: &[usize]) {
    let mut snap = snapshot(title, seq);
    for &ch in force_used {
        snap.channel_used[ch] = true;
    }
    h.upd(Message::Player(PlayerEvent::Opened(Box::new(snap))));
}

/// Song-`.cfg` path for a synthetic title (same `<title>.cfg` form `Paths`
/// uses under the hermetic data dir).
fn song_cfg(title: &str) -> PathBuf {
    hermetic_home()
        .join(".dmidiplayer")
        .join(format!("{title}.cfg"))
}

/// Parsed song `.cfg` for `title` — the mix filesystem probe (channel rows
/// recorded by the last `SongSettingsSave`).
fn mix_cfg(title: &str) -> SongSettings {
    SongSettings::load(&song_cfg(title))
        .unwrap_or_else(|| panic!("T12c: expected a song cfg for {title}"))
}

/// Rebuild the 16-row mix from a probed cfg (absent group = fresh default;
/// the engine sync math consumes `ChannelState`, never the cfg shape).
fn rows_from_cfg(cfg: &SongSettings) -> [ChannelState; 16] {
    std::array::from_fn(|ch| match &cfg.channels[ch] {
        Some(row) => ChannelState {
            muted: row.muted,
            soloed: row.solo,
            locked: row.locked,
            patch: row.patch.clamp(0, 127) as u8,
            level_pct: row.level.unwrap_or(100).clamp(0, 200) as u32,
            ..ChannelState::new()
        },
        None => ChannelState::new(),
    })
}

/// Solo/mute toggles land in the stored mix and drive the exact engine sync
/// values: the cfg probe pins the stored rows, `sync_all` over the rebuilt
/// rows pins the pushed `SetMuted` + `SetChannelVolume` pairs (mix commands
/// are fire-and-forget into the headless engine — no readback — so the pure
/// matrix is the values oracle). Clearing the solo restores every slider.
/// (Sends shape: unit-pinned by `channel_arms_drive_exact_engine_sync_values`;
/// formula by `solo_mute_reduction_math_is_exact` /
/// `sync_all_reduces_every_non_solo_row`.)
#[test]
fn solo_mute_toggles_drive_engine_sync_values() {
    let mut h = Harness::new("ch-mix");
    let seq = load_fixture("lindaamiga.mid");
    open_mix(&mut h, "ch-mix.mid", &seq, &[0, 1]);
    let mut base = Settings::new();
    base.solo_volume_reduction = 75;
    h.upd(Message::PrefsChanged(base));
    let disk = std::fs::read_to_string(h.scratch().join("ch-mix.conf")).unwrap();
    assert!(disk.contains("soloVolumeReduction=75\n"));

    h.upd(Message::ChannelMute(0, true));
    h.upd(Message::ChannelLevel(0, 150));
    h.upd(Message::ChannelSolo(1, true));
    h.upd(Message::SongSettingsSave);
    let cfg = mix_cfg("ch-mix.mid");
    let ch0 = cfg.channels[0].as_ref().expect("ch 0 row recorded");
    assert!(ch0.muted);
    assert!(!ch0.solo);
    assert_eq!(ch0.level, Some(150));
    let ch1 = cfg.channels[1].as_ref().expect("ch 1 row recorded");
    assert!(ch1.solo);
    assert!(!ch1.muted);

    // Non-solo ch 0: (100-75)% × 150%; soloed ch 1 keeps its slider; mute
    // silences engine-side, independent of the factor.
    let sync = sync_all(&rows_from_cfg(&cfg), 75);
    assert!((sync[0].factor - 0.375).abs() < 1e-9);
    assert!(sync[0].muted);
    assert!((sync[1].factor - 1.0).abs() < 1e-9);
    assert!(!sync[1].muted);

    // Clearing the solo restores every slider.
    h.upd(Message::ChannelSolo(1, false));
    h.upd(Message::SongSettingsSave);
    let sync = sync_all(&rows_from_cfg(&mix_cfg("ch-mix.mid")), 75);
    assert!((sync[0].factor - 1.5).abs() < 1e-9);
    assert!((sync[1].factor - 1.0).abs() < 1e-9);
}

/// Patch reselect + relabel: live `Program` events reselect the stored patch
/// (clamped — observable via the cfg probe), user patch edits store likewise,
/// and the same patch number relabels GM → MT-32 once the instrument map
/// commits. (Clamp + reselect: unit-pinned by
/// `program_reselect_clamps_and_relabels_per_map`; 128-entry model by
/// `patch_options_relabel_per_map`; row/model wiring by
/// `channels_view_model_contract`.)
#[test]
fn patch_reselect_relabels_per_map_gm_to_mt32() {
    let mut h = Harness::new("ch-patch");
    let seq = load_fixture("test.mid");
    open_mix(&mut h, "ch-patch.mid", &seq, &[0, 1]);

    h.upd(Message::Player(PlayerEvent::Program { ch: 0, patch: 40 }));
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        mix_cfg("ch-patch.mid").channels[0]
            .as_ref()
            .expect("ch 0 row recorded")
            .patch,
        40
    );
    assert_eq!(patch_label(0, 40), "Violin");
    // Live reselect clamps to 127, like the user dropdown.
    h.upd(Message::Player(PlayerEvent::Program { ch: 0, patch: 200 }));
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        mix_cfg("ch-patch.mid").channels[0]
            .as_ref()
            .expect("ch 0 row recorded")
            .patch,
        127
    );
    assert_eq!(patch_label(0, 127), "Gunshot");
    // User patch change stores verbatim.
    h.upd(Message::ChannelPatch(1, 10));
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        mix_cfg("ch-patch.mid").channels[1]
            .as_ref()
            .expect("ch 1 row recorded")
            .patch,
        10
    );

    // GM → MT-32 map commit relabels rows + the shared 128-entry model.
    let mut base = Settings::new();
    base.instrument_map = 2;
    h.upd(Message::PrefsChanged(base));
    let disk = std::fs::read_to_string(h.scratch().join("ch-patch.conf")).unwrap();
    assert!(disk.contains("instrumentMap=2\n"));
    assert_eq!(patch_label(2, 40), "Echo Bell");
    assert_eq!(patch_options(0)[0], "Acoustic Grand Piano");
    assert_eq!(patch_options(2)[0], "Acou Piano 1");
    assert_eq!(patch_options(2)[127], "Hollow");
}

/// Level slider resends through the cfg probe + sync math (clamped 0–200),
/// and out-of-range channels are silent no-ops on every mix arm (never panic,
/// never disturb a stored row). (Transitions + OOR guard: unit-pinned by
/// `channel_mix_transitions_and_oor_guard`; clamp by
/// `level_slider_clamps_0_to_200`.)
#[test]
fn level_slider_resends_and_clamps_with_oor_guard() {
    let mut h = Harness::new("ch-level");
    let seq = load_fixture("test.mid");
    open_mix(&mut h, "ch-level.mid", &seq, &[0, 1]);

    h.upd(Message::ChannelLevel(0, 999));
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        mix_cfg("ch-level.mid").channels[0]
            .as_ref()
            .expect("ch 0 row recorded")
            .level,
        Some(200)
    );
    let sync = sync_all(&rows_from_cfg(&mix_cfg("ch-level.mid")), 50);
    assert!((sync[0].factor - 2.0).abs() < 1e-9);
    // Slider 0 round-trips as `None` (the C++ `if (lvl)` load rule maps a
    // written 0 back to untouched), so the factor-0 half pins a 0-slider row
    // directly rather than through the reconstruction.
    h.upd(Message::ChannelLevel(0, 0));
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        mix_cfg("ch-level.mid").channels[0]
            .as_ref()
            .expect("ch 0 row recorded")
            .level,
        None
    );
    let mut rows = rows_from_cfg(&mix_cfg("ch-level.mid"));
    rows[0].set_level_pct(0);
    let sync = sync_all(&rows, 50);
    assert!((sync[0].factor - 0.0).abs() < 1e-9);

    // Out-of-range arms: no panic, stored rows untouched.
    h.upd(Message::ChannelMute(16, true));
    h.upd(Message::ChannelSolo(255, true));
    h.upd(Message::ChannelLock(99, true));
    h.upd(Message::ChannelPatch(99, 1));
    h.upd(Message::ChannelLevel(99, 50));
    h.upd(Message::ChannelName(99, "x".to_string()));
    h.upd(Message::SongSettingsSave);
    let ch0 = mix_cfg("ch-level.mid").channels[0]
        .clone()
        .expect("ch 0 row recorded");
    assert!(!ch0.muted && !ch0.solo && !ch0.locked);
    assert_eq!(ch0.level, None);
}

/// Meter routing through `update`: NoteOn/NoteOff/Tick persist nothing and
/// disturb no slider (meters are render-only — the cfg probe stays at the
/// fresh slider), while the peak/max/decay values go through the public
/// halves. (Wiring: unit-pinned by `note_on_peaks_meter_note_off_leaves_it`
/// and `tick_decays_meters`; pure halves by `note_peak_keeps_max` and
/// `meter_decay_matches_cpp_timer`.)
#[test]
fn meter_peak_on_note_on_decays_on_tick() {
    let mut h = Harness::new("ch-meter");
    let seq = load_fixture("test.mid");
    open_mix(&mut h, "ch-meter.mid", &seq, &[0, 1]);

    h.upd(Message::Player(PlayerEvent::NoteOn {
        ch: 0,
        note: 60,
        vel: 100,
    }));
    h.upd(Message::Player(PlayerEvent::NoteOn {
        ch: 0,
        note: 64,
        vel: 10,
    }));
    h.upd(Message::Player(PlayerEvent::NoteOff {
        ch: 0,
        note: 60,
        vel: 64,
    }));
    h.upd(Message::Tick(std::time::Instant::now()));
    h.upd(Message::Tick(std::time::Instant::now()));
    // Out-of-range NoteOn never touches a meter.
    h.upd(Message::Player(PlayerEvent::NoteOn {
        ch: 16,
        note: 60,
        vel: 127,
    }));
    assert!(!h.config_exists());
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        mix_cfg("ch-meter.mid").channels[0]
            .as_ref()
            .expect("ch 0 row recorded")
            .level,
        Some(100)
    );

    // Public value halves: peak is max (a quieter NoteOn never lowers it),
    // one tick decays ×0.82.
    assert!((note_peak(0.0, 100) - 100.0 / 127.0).abs() < 1e-6);
    assert!((note_peak(100.0 / 127.0, 10) - 100.0 / 127.0).abs() < 1e-6);
    let mut mix = default_mix();
    mix[0].level = 1.0;
    decay_meters(&mut mix);
    assert!((mix[0].level - 0.82).abs() < 1e-6);
}

/// Load-time mix sync (A8, engine-first): with `autoSongSettings` the song
/// cfg overlays the reset rows at `Opened` (globals + per-group mix);
/// with it off the same cfg is ignored (pure reset, stale mix from the
/// previous song cleared). The cfg probe pins the resulting UI rows; the
/// engine-first send order itself is code-shape pinned by the unit halves
/// (`opened_syncs_ui_to_engine_a8`,
/// `opened_with_auto_song_settings_overlays_mix`,
/// `reset_for_song_replaces_mix_with_snapshot_rows`).
#[test]
fn load_time_mix_sync_is_engine_first_a8() {
    std::fs::create_dir_all(hermetic_home().join(".dmidiplayer")).unwrap();
    let cfg_text = "[Global]\nvolume=150\n\n\
         [MIDI Channel  1]\nname=Keys\nmuted=true\nsolo=true\nlocked=true\n\
         patch=5\nlevel=80\n";

    // Auto ON: the cfg overlays the reset rows at load.
    let mut h = Harness::new("ch-loadon");
    let mut base = Settings::new();
    base.auto_song_settings = true;
    h.upd(Message::PrefsChanged(base));
    std::fs::write(song_cfg("ch-loadon.mid"), cfg_text).unwrap();
    let seq = load_fixture("schubert_avemaria.mid");
    open_mix(&mut h, "ch-loadon.mid", &seq, &[0, 1]);
    h.upd(Message::SongSettingsSave);
    let cfg = mix_cfg("ch-loadon.mid");
    assert_eq!(cfg.volume, Some(150));
    let ch0 = cfg.channels[0].as_ref().expect("ch 0 row recorded");
    assert_eq!(ch0.name.as_deref(), Some("Keys"));
    assert!(ch0.muted && ch0.solo && ch0.locked);
    assert_eq!(ch0.patch, 5);
    assert_eq!(ch0.level, Some(80));
    // Rows without a cfg group stay fresh-reset.
    let ch1 = cfg.channels[1].as_ref().expect("ch 1 row recorded");
    assert!(!ch1.muted && !ch1.solo && !ch1.locked);
    assert_eq!(ch1.level, Some(100));

    // Auto OFF: the same cfg is ignored — pure reset — and a stale mix from
    // the previous song clears on the next open.
    let mut h = Harness::new("ch-loadoff");
    std::fs::write(song_cfg("ch-loadoff.mid"), cfg_text).unwrap();
    let seq = load_fixture("schubert_avemaria.mid");
    let label0 = seq.channel_label(0);
    open_mix(&mut h, "ch-loadoff.mid", &seq, &[0, 1]);
    h.upd(Message::SongSettingsSave);
    let ch0 = mix_cfg("ch-loadoff.mid").channels[0]
        .clone()
        .expect("ch 0 row recorded");
    assert!(!ch0.muted && !ch0.solo && !ch0.locked);
    assert_eq!(ch0.patch, 0);
    assert_eq!(ch0.level, Some(100));
    assert_eq!(ch0.name, Some(label0));
    h.upd(Message::ChannelMute(0, true));
    open_mix(&mut h, "ch-loadoff-b.mid", &seq, &[0, 1]);
    h.upd(Message::SongSettingsSave);
    let ch0 = mix_cfg("ch-loadoff-b.mid").channels[0]
        .clone()
        .expect("ch 0 row recorded");
    assert!(!ch0.muted);
    assert_eq!(ch0.level, Some(100));
}

/// The drums row (index 9 = channel 10, the default `drumsChannel`) syncs
/// identically to any other row under the same inputs — no mute/solo/level
/// exception anywhere in the mix path (the only drums special-case is
/// pitch-shift exclusion at note dispatch, engine-side). (Unit-pinned by
/// state test `drums_row_has_no_mix_exception` and
/// `channel_arms_drive_exact_engine_sync_values`.)
#[test]
fn drums_row_has_no_mix_exception() {
    let mut h = Harness::new("ch-drums");
    let seq = load_fixture("test.mid");
    open_mix(&mut h, "ch-drums.mid", &seq, &[0, 1, 9]);
    let mut base = Settings::new();
    base.solo_volume_reduction = 60;
    h.upd(Message::PrefsChanged(base));

    h.upd(Message::ChannelLevel(0, 140));
    h.upd(Message::ChannelLevel(9, 140));
    h.upd(Message::ChannelMute(9, true));
    h.upd(Message::ChannelSolo(1, true));
    h.upd(Message::SongSettingsSave);
    let cfg = mix_cfg("ch-drums.mid");
    let drums = cfg.channels[9].as_ref().expect("drums row recorded");
    assert!(drums.muted);
    assert_eq!(drums.level, Some(140));
    let sync = sync_all(&rows_from_cfg(&cfg), 60);
    assert_eq!(sync[0].factor, sync[9].factor);
    assert!((sync[9].factor - 0.56).abs() < 1e-9);
    assert!(!sync[0].muted);
    assert!(sync[9].muted);

    // A soloed drums row keeps its slider like any soloed row.
    h.upd(Message::ChannelMute(9, false));
    h.upd(Message::ChannelSolo(9, true));
    h.upd(Message::SongSettingsSave);
    let sync = sync_all(&rows_from_cfg(&mix_cfg("ch-drums.mid")), 60);
    assert!((sync[9].factor - 1.4).abs() < 1e-9);
    assert!(!sync[9].muted);
}
