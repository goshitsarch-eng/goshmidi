//! T13d integration: piano flows through `App::update` + public piano state.
//!
//! Same hermetic harness as T10c/T11c/T12c (temp HOME, per-test portable
//! config, headless engine, synthetic `Opened` with forced `used` rows).
//! `App` fields are private, so the preview arms pin behavior through
//! filesystem silence (preview persists nothing and disturbs no mix row —
//! the song-`.cfg` probe stays at the fresh slider) plus the pure
//! [`dmidiplayer::state::piano`] halves (U12 click mapping, A3 audition,
//! idle-only note-off, palette/velocity/names read path). Matrix
//! light/clear and the engine-carried raw sends stay unit-pinned in
//! `app.rs` / `core::player` (named per test); pixel output is never matched.

mod common;

use std::path::PathBuf;

use common::{Harness, hermetic_home};
use dmidiplayer::config::paths::{Env, Paths};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::config::song::SongSettings;
use dmidiplayer::message::{LoadedSong, Message, PlayerEvent, SongSnapshot, View};
use dmidiplayer::state::piano::{
    PIANO_MARGIN_PX, PianoState, PreviewNote, click_rows, click_to_channel, click_to_note,
    click_to_preview, count_whites, drawn_range, highlight_rgba, note_label, preview_off_targets,
    row_height_px, show_note_name, white_width_px,
};
use dmidiplayer::ui::piano;

/// Snapshot with `used` channels forced on (the piano arms read
/// `channel_used` + note range; the rest keeps the snapshot honest for the
/// sibling arms `on_opened` runs). Titles are unique per test so parallel
/// tests never share a song `.cfg`.
fn snapshot(title: &str, used: &[usize]) -> SongSnapshot {
    let mut channel_used = [false; 16];
    for &ch in used {
        channel_used[ch] = true;
    }
    SongSnapshot {
        title: title.to_string(),
        format: "SMF".to_string(),
        bars: 16,
        len_ticks: 4800,
        duration_ms: 60_000,
        division: 96,
        num_tracks: 2,
        initial_tempo_us: 500_000.0,
        channel_used,
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
/// `Opened` would reset the piano under test).
fn open_piano(h: &mut Harness, title: &str, used: &[usize]) {
    h.upd(Message::Player(PlayerEvent::Opened(Box::new(snapshot(
        title, used,
    )))));
}

/// Song-`.cfg` path for a synthetic title (same `<title>.cfg` form `Paths`
/// uses under the hermetic data dir).
fn song_cfg(title: &str) -> PathBuf {
    hermetic_home()
        .join(".dmidiplayer")
        .join(format!("{title}.cfg"))
}

/// Parsed song `.cfg` for `title` — the mix-disturbance probe (channel rows
/// recorded by the last `SongSettingsSave`).
fn mix_cfg(title: &str) -> SongSettings {
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

/// U12 click→preview mapping in the DRAWN range: with tighten drawn
/// (34–98), slot 0 auditions note 35 (C++ fixed-range mapping would audition
/// 21 — the FIX); the target previews through `update` persisting nothing
/// and disturbing no mix row. OOR clicks (both axes) yield `None` — no
/// message is constructed, so nothing emits. Inversion (noteless song),
/// no-song, and empty-rows cases all map to `None`. (Mapping: unit-pinned by
/// `click_maps_in_drawn_range_u12`, `click_to_note_edges_and_oor`,
/// `click_to_channel_rows_and_oor`, `click_to_preview_combines_axes`,
/// `drawn_range_inverts_for_noteless_songs`,
/// `rows_draw_vs_click_differ_on_empty_song`; canvas wiring by
/// `click_maps_white_keys_in_drawn_range` / `click_rejects_degenerate_input`;
/// matrix light by `piano_preview_idle_only_note_off`.)
#[test]
fn click_preview_maps_in_drawn_range_u12() {
    let mut h = Harness::new("pn-click");
    open_piano(&mut h, "pn-click.mid", &[0, 1]);
    let used: [bool; 16] = std::array::from_fn(|ch| ch < 2);
    let visible = [true; 16];
    let rows = click_rows(false, &used, &visible);
    assert_eq!(rows, vec![0, 1]);

    // Tighten off: full 21–108; tighten on: song min/max ± 2.
    assert_eq!(drawn_range(false, false, 36, 96), (21, 108));
    let (lo, hi) = drawn_range(true, false, 36, 96);
    assert_eq!((lo, hi), (34, 98));

    // Slot 0 of the drawn range: first white at/after 34 is 35 (34 is A#).
    let width = 808.0;
    let ww = white_width_px(width, count_whites(lo, hi));
    let row_h = row_height_px(480, rows.len());
    assert_eq!(row_h, 240);
    let x = PIANO_MARGIN_PX + ww * 0.5;
    assert_eq!(click_to_note(x, width, lo, hi), Some(35));
    assert_eq!(
        click_to_preview(x, 10.0, width, row_h, lo, hi, &rows),
        Some((0, 35))
    );
    // The U12 delta: C++ would map the same slot in fixed 21–108 (note 21).
    assert_eq!(click_to_note(x, width, 21, 108), Some(21));

    // The mapped target previews through `update`: persists nothing,
    // disturbs no mix row.
    h.upd(Message::PianoPreviewOn(0, 35));
    h.upd(Message::PianoPreviewOff);
    assert!(!h.config_exists());
    h.upd(Message::SongSettingsSave);
    let ch0 = mix_cfg("pn-click.mid").channels[0]
        .clone()
        .expect("ch 0 row recorded");
    assert!(!ch0.muted);
    assert_eq!(ch0.level, Some(100));

    // OOR clicks emit nothing: a full key-width left of the margin, past
    // the last key, and a full row-height above the first row all yield
    // `None`. (Just inside the 4px margin still hits slot 0 — the generous
    // C++ hit area, preserved.)
    assert_eq!(click_to_note(0.0, width, lo, hi), Some(35));
    assert_eq!(click_to_note(-100.0, width, lo, hi), None);
    assert_eq!(click_to_note(width + 50.0, width, lo, hi), None);
    assert_eq!(click_to_channel(-250.0, row_h, &rows), None);
    assert_eq!(
        click_to_preview(-100.0, 10.0, width, row_h, lo, hi, &rows),
        None
    );
    assert_eq!(
        click_to_preview(x, -250.0, width, row_h, lo, hi, &rows),
        None
    );
    // Empty matrix → idle release sends nothing (pure half).
    assert!(
        preview_off_targets(&[[false; 128]; 16], false).is_empty(),
        "T13d: idle release with no lit keys targets nothing"
    );

    // Inversion: noteless song (lowest 127 / highest 0) inverts the drawn
    // range — no whites, no click target (C++ parity).
    let (lo, hi) = drawn_range(true, false, 127, 0);
    assert!(lo > hi);
    assert_eq!(count_whites(lo, hi), 0);
    assert_eq!(click_to_note(100.0, 800.0, lo, hi), None);
    // No song: click rows are empty even though the draw path shows ch 1.
    assert!(click_rows(true, &used, &visible).is_empty());
    assert_eq!(click_to_preview(x, 10.0, width, row_h, 21, 108, &[]), None);
    // Nothing used+visible: no rows, no target.
    assert!(click_rows(false, &used, &[false; 16]).is_empty());
}

/// A3 audition bypasses mute + pitch: a muted channel still previews, and
/// the preview disturbs neither the mute row nor the pitch (the cfg probe
/// pins the stored mix; the audition velocity pins the fixed 80). (Matrix
/// light: unit-pinned by `piano_preview_idle_only_note_off`; fixed velocity
/// by `preview_note_fixes_velocity_80`; the engine-carried raw write
/// bypassing the mix by
/// `preview_note_writes_straight_to_output_bypassing_mix`.)
#[test]
fn audition_bypasses_mute_and_pitch_a3() {
    let mut h = Harness::new("pn-a3");
    open_piano(&mut h, "pn-a3.mid", &[0, 1]);
    h.upd(Message::ChannelMute(0, true));
    h.upd(Message::PitchChanged(5));
    // A muted channel still previews (bypass, not mix).
    h.upd(Message::PianoPreviewOn(0, 60));
    h.upd(Message::PianoPreviewOff);
    assert!(!h.config_exists());
    h.upd(Message::SongSettingsSave);
    let cfg = mix_cfg("pn-a3.mid");
    let ch0 = cfg.channels[0].as_ref().expect("ch 0 row recorded");
    assert!(ch0.muted, "T13d: preview must not unmute the row");
    assert_eq!(ch0.level, Some(100));
    assert_eq!(cfg.pitch, 5, "T13d: preview must not reset pitch");

    // Audition spec: fixed velocity 80, channel + note verbatim.
    assert_eq!(
        PreviewNote::audition(0, 60),
        PreviewNote {
            ch: 0,
            note: 60,
            vel: 80
        }
    );
}

/// Note-off is idle-only: while playing, a preview release targets nothing
/// (song notes must survive); while idle, every lit key is a target. Both
/// legs route through `update` persisting nothing. (Pure halves by
/// `preview_off_is_idle_only`; matrix keep/clear by
/// `piano_preview_idle_only_note_off`; raw NoteOff sends by
/// `preview_off_sends_every_note_vel_0`.)
#[test]
fn preview_note_off_is_idle_only() {
    // Pure half (the exact code the `PianoPreviewOff` arm calls).
    let mut lit = [[false; 128]; 16];
    lit[0][60] = true;
    lit[1][64] = true;
    assert!(
        preview_off_targets(&lit, true).is_empty(),
        "T13d: release while playing targets nothing"
    );
    assert_eq!(preview_off_targets(&lit, false), vec![(0, 60), (1, 64)]);

    // Both legs through `update`: silent either way.
    let mut h = Harness::new("pn-off");
    open_piano(&mut h, "pn-off.mid", &[0, 1]);
    h.upd(Message::PianoPreviewOn(0, 60));
    h.upd(Message::PianoPreviewOff); // idle leg: clears (unit-pinned)
    assert!(!h.config_exists());
    h.upd(Message::Play); // optimistic `Playing`
    h.upd(Message::PianoPreviewOn(1, 64));
    h.upd(Message::PianoPreviewOff); // playing leg: keeps lit (unit-pinned)
    assert!(!h.config_exists());
    h.upd(Message::Stop);
    assert!(!h.config_exists());
}

/// Matrix light/clear halves plus the tighten/visibility/view routing: the
/// pure `PianoState` halves pin light/unlight/clear, and the toolbar + page
/// messages persist nothing. (Matrix halves by `note_on_off_matrix_and_vel`;
/// toggles by `piano_view_toggles`; stop-clears by
/// `transport_pause_stop_round_trip_e2e`.)
#[test]
fn matrix_light_and_clear_with_silent_toolbar() {
    // Pure halves (the same code the arms + NoteOn/Off events call).
    let mut piano = PianoState::new();
    piano.note_on(0, 60, 80);
    assert!(piano.notes_on[0][60]);
    assert_eq!(piano.note_vel[0][60], 80);
    piano.note_off(0, 60);
    assert!(!piano.notes_on[0][60]);
    piano.note_on(1, 64, 100);
    piano.clear();
    assert!(piano.notes_on.iter().all(|ch| ch.iter().all(|&n| !n)));

    // Toolbar + page routing persists nothing.
    let mut h = Harness::new("pn-matrix");
    open_piano(&mut h, "pn-matrix.mid", &[0, 1]);
    h.upd(Message::ViewSelected(View::Piano));
    h.upd(Message::PianoTighten(true));
    h.upd(Message::PianoHideUnused);
    h.upd(Message::PianoShowAll);
    h.upd(Message::PianoTighten(false));
    assert!(!h.config_exists());
}

/// Palette/velocity/names read path: D2 Piano settings edits persist via
/// the commit probe and reflect verbatim in the piano view model (rows,
/// range, tighten, all five settings rows); the pure highlight/label halves
/// pin palette-vs-single, the velocity tint, garbage→gray, visibility modes,
/// and octave labels. (Pure halves by `highlight_palette_and_single_paths`,
/// `highlight_velocity_tint_scales_rgb`, `note_labels_match_cpp_octaves`,
/// `note_name_visibility_matrix`; canvas halves by
/// `channel_color_renders_palette_single_velocity`,
/// `note_labels_use_scientific_octaves`, `name_visibility_modes`,
/// `builder_selects_rows_and_range`.)
#[test]
fn palette_velocity_names_read_path() {
    let mut h = Harness::new("pn-vm");
    let mut next = Settings::new();
    next.highlight_palette = 0;
    next.single_color = "red".to_string();
    next.velocity_color = false;
    next.names_visibility = 3;
    next.octave_subscript = true;
    h.upd(Message::PrefsChanged(next.clone()));
    assert_eq!(persisted_settings(&h, "pn-vm"), next);

    // The VM reads every row verbatim from settings + piano + song.
    let song = LoadedSong::from(snapshot("pn-vm.mid", &[0, 1]));
    let live = persisted_settings(&h, "pn-vm");
    let vm = piano::view_model(&PianoState::new(), Some(&song), &live);
    assert_eq!(
        vm.rows.iter().map(|row| row.channel).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!((vm.lo, vm.hi), (21, 108));
    assert!(!vm.tighten);
    assert_eq!(vm.highlight_palette, 0);
    assert_eq!(vm.single_color, "red");
    assert!(!vm.velocity_color);
    assert_eq!(vm.names_visibility, 3);
    assert!(vm.octave_subscript);
    assert!(vm.has_song);
    // Tighten flips the drawn range through the same builder.
    let mut tightened = PianoState::new();
    tightened.tighten = true;
    let vm = piano::view_model(&tightened, Some(&song), &live);
    assert_eq!((vm.lo, vm.hi), (34, 98));

    // Palette 0 renders the single color; garbage falls back to gray.
    let red = highlight_rgba(0, 100, 0, "red", false);
    assert!((red.r - 1.0).abs() < 1e-6 && red.g < 1e-6 && red.b < 1e-6);
    let gray = highlight_rgba(0, 100, 0, "notacolor", false);
    assert!((gray.r - 0.5).abs() < 1e-6 && (gray.g - 0.5).abs() < 1e-6);
    // Any nonzero palette renders the per-channel entry; velocity tints it.
    let full = highlight_rgba(0, 100, 2, "red", false);
    let tinted = highlight_rgba(0, 100, 2, "red", true);
    assert!(tinted.r < full.r && tinted.r > 0.0);
    let soft = highlight_rgba(0, 10, 2, "red", true);
    assert!(soft.r < tinted.r);
    // Visibility modes + octave labels.
    assert!(show_note_name(61, false, 3));
    assert!(show_note_name(60, false, 1));
    assert!(!show_note_name(61, false, 1));
    assert_eq!(show_note_name(61, true, 2), true);
    assert_eq!(show_note_name(61, false, 2), false);
    assert_eq!(note_label(60), "C4");
    assert_eq!(note_label(69), "A4");
    assert_eq!(note_label(21), "A0");
    assert_eq!(note_label(108), "C8");
}
