//! T11c integration: lyrics flows through `App::update` + public lyrics state.
//!
//! Same hermetic harness as T10c (temp HOME, real `examples/*` fixtures —
//! incl. `twinkle.kar` and `Negra_Sombra.kar` — headless engine, synthetic
//! `Opened` carrying REAL fixture texts): codec/filter/live-tail message
//! wiring, copy/save/print routing incl. the D12/D13 result shapes,
//! song-settings encoding round-trip, and prefs-commit isolation. `App`
//! fields are private, so the wiring halves pin behavior through the
//! song-`.cfg` `encoding` probe and the portable-config round-trip; body /
//! live-tail / option halves go through the public `LyricsState` API (the
//! same code the arms call) plus the `app.rs` / `state::lyrics` unit tests
//! named per test.

mod common;

use std::path::PathBuf;

use common::{Harness, canon, fixture, hermetic_home};
use dmidiplayer::config::paths::{Env, Paths};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::config::song::SongSettings;
use dmidiplayer::core::sequence::Sequence;
use dmidiplayer::message::{Message, PlayerEvent, SongSnapshot};
use dmidiplayer::state::lyrics::{LyricsState, codec_option_index, preselect_filters};

/// Load a real example fixture through the public `Sequence` API.
fn load_fixture(name: &str) -> Sequence {
    let mut seq = Sequence::default();
    assert!(
        seq.load_file(&fixture(name)),
        "T11c: fixture {name} must load"
    );
    seq
}

/// Real-text `Opened` snapshot: texts, charset, track/channel metadata from
/// the fixture (the lyrics arms read texts + charset; the rest keeps the
/// snapshot honest for the sibling arms `on_opened` runs).
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
    }
}

/// Feed a real-text `Opened` for `title` (synthetic delivery, like T10c: the
/// headless engine's own `Opened` sits unpumped). Titles are unique per test
/// so parallel tests never share a song `.cfg`.
fn open_real(h: &mut Harness, title: &str, seq: &Sequence) {
    h.upd(Message::Player(PlayerEvent::Opened(Box::new(snapshot(
        title, seq,
    )))));
}

/// Song-`.cfg` path for a synthetic title (same `<title>.cfg` form `Paths`
/// uses under the hermetic data dir).
fn song_cfg(title: &str) -> PathBuf {
    hermetic_home()
        .join(".dmidiplayer")
        .join(format!("{title}.cfg"))
}

/// `encoding` row of a song `.cfg` — the lyrics-codec filesystem probe:
/// `SongSettingsSave` records `lyrics.codec` there.
fn cfg_encoding(title: &str) -> Option<String> {
    SongSettings::load(&song_cfg(title))
        .unwrap_or_else(|| panic!("T11c: expected a song cfg for {title}"))
        .encoding
}

/// Whole persisted portable config through the real reader (the
/// `PrefsChanged`-commit probe; replicates the harness `MidiApply` shape).
fn persisted_settings(h: &Harness, tag: &str) -> Settings {
    let home = hermetic_home();
    let env = Env::for_test(home, home, home, home);
    let paths = Paths::with_env(env, Some(Some(h.scratch().join(format!("{tag}.conf")))));
    Settings::load(&paths)
}

/// `goshmidi-lyrics-*` temp files (the D13 export prefix, mirrored here —
/// the app constant is private): driving `update` must never create one,
/// since print `Task`s are dropped headless, never executed.
fn print_temp_files() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("goshmidi-lyrics-"))
        })
        .collect();
    found.sort();
    found
}

/// `LyricCodec` persists through the song-`.cfg` `encoding` row (the codec
/// filesystem probe); empty codec is ignored, off-list labels persist
/// verbatim while the dropdown shows index 0. (Body recompute under the
/// codec + preselect seeding: unit-pinned by
/// `lyric_filters_and_codec_recompute_body`.)
#[test]
fn lyric_codec_arm_persists_to_song_cfg() {
    let mut h = Harness::new("lyr-codec");
    let seq = load_fixture("lindaamiga.mid");
    open_real(&mut h, "lyr-codec.mid", &seq);
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-codec.mid").as_deref(), Some("UTF-8"));
    h.upd(Message::LyricCodec("SHIFT_JIS".to_string()));
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-codec.mid").as_deref(), Some("SHIFT_JIS"));
    // Empty codec is ignored (`setCurrentCharset` parity).
    h.upd(Message::LyricCodec(String::new()));
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-codec.mid").as_deref(), Some("SHIFT_JIS"));
    // Off-list labels persist too (display ≠ decode) at dropdown index 0.
    h.upd(Message::LyricCodec("X-GOSH-OFFLIST".to_string()));
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        cfg_encoding("lyr-codec.mid").as_deref(),
        Some("X-GOSH-OFFLIST")
    );
    assert_eq!(codec_option_index("X-GOSH-OFFLIST"), 0);
    assert_eq!(codec_option_index("shift_jis"), 3);
}

/// Track/type filter arms keep the codec (only the body recomputes) and
/// persist nothing by themselves. (Recompute + no-snap-back + live-clear:
/// unit-pinned by `lyric_filters_and_codec_recompute_body` and
/// `lyric_filter_change_clears_live_tail`.)
#[test]
fn filter_arms_keep_codec_and_persist_nothing() {
    let mut h = Harness::new("lyr-filter");
    // Filter arms with no song loaded: no crash, no song fabricated.
    h.upd(Message::LyricTrackFilter(Some(1)));
    h.upd(Message::LyricTypeFilter(0));
    h.assert_save_without_song_writes_nothing("no-song-yet");
    let seq = load_fixture("schubert_avemaria.mid");
    open_real(&mut h, "lyr-filter.mid", &seq);
    h.upd(Message::LyricTrackFilter(Some(1)));
    h.upd(Message::LyricTypeFilter(0));
    h.upd(Message::LyricTrackFilter(None));
    h.upd(Message::LyricTypeFilter(99));
    h.upd(Message::LyricTypeFilter(5));
    assert!(!h.config_exists());
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-filter.mid").as_deref(), Some("UTF-8"));
}

/// Live `Text` events route through `update` (passing + track/type
/// filtered + non-ASCII payload) without persisting anything. (Decode +
/// filter matrix + past/current segments: unit-pinned by
/// `live_text_filters_and_decodes`.)
#[test]
fn live_text_events_route_without_persisting() {
    let mut h = Harness::new("lyr-live");
    let seq = load_fixture("lindaamiga.mid");
    open_real(&mut h, "lyr-live.mid", &seq);
    for (track, text_type, bytes) in [
        (2, 5, "Ay, ".as_bytes()),
        (1, 5, "wrong track".as_bytes()),
        (2, 1, "wrong type".as_bytes()),
        (2, 5, "se ✓".as_bytes()),
    ] {
        h.upd(Message::Player(PlayerEvent::Text {
            track,
            text_type,
            ticks: 0,
            bytes: bytes.to_vec(),
        }));
    }
    assert!(!h.config_exists());
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-live.mid").as_deref(), Some("UTF-8"));
}

/// Copy/save/print launches and their result routes (`LyricsSaved` /
/// `LyricsPrinted` / `ChooserCancelled`) disturb no lyrics state: the codec
/// probe is stable, the config stays untouched, and no
/// `goshmidi-lyrics-*` temp file appears (the `Task`s are dropped headless,
/// so no opener ever spawns). (Launch-no-change + Ok-silent/Err-toast:
/// unit-pinned by `lyrics_copy_save_print_launch_without_state_change` and
/// `lyrics_saved_and_printed_route_ok_silent_err_toast`; opener spawn
/// mapping by `spawn_opener_maps_spawn_errors`.)
#[test]
fn copy_save_print_routes_disturb_nothing() {
    let before = print_temp_files();
    let mut h = Harness::new("lyr-routes");
    let first = fixture("test.mid");
    h.upd(Message::OpenFiles(
        vec![first.clone(), fixture("lindaamiga.mid")],
        true,
    ));
    let seq = load_fixture("twinkle.kar");
    open_real(&mut h, "lyr-routes.mid", &seq);
    h.upd(Message::LyricCodec("WINDOWS-1252".to_string()));
    h.upd(Message::LyricsCopy);
    h.upd(Message::LyricsSaveDialog);
    h.upd(Message::LyricsPrint);
    // D12 result shape: `Ok` silent, `Err` toasts — neither touches state.
    h.upd(Message::LyricsSaved(Ok(h.scratch().join("lyrics.txt"))));
    h.upd(Message::LyricsSaved(Err(
        "Could not save lyrics: denied".to_string()
    )));
    // D13 result shape: same silent/error split, no temp-file side effect.
    h.upd(Message::LyricsPrinted(Ok(())));
    h.upd(Message::LyricsPrinted(Err(
        "Could not print lyrics: boom".to_string()
    )));
    h.upd(Message::ChooserCancelled);
    assert!(!h.config_exists());
    // D12/D13 results record no playlist source …
    assert!(h.persisted_last_play_list().is_empty());
    h.upd(Message::SongSettingsSave);
    assert_eq!(
        cfg_encoding("lyr-routes.mid").as_deref(),
        Some("WINDOWS-1252")
    );
    // … and lyrics routing leaves the playlist current alone.
    assert_eq!(h.probe_current_file("current-kept"), canon(&first));
    assert_eq!(
        print_temp_files(),
        before,
        "dropped print tasks must not write temp files"
    );
}

/// D12 export content: the full body+live text (U10 duplication included)
/// round-trips through a file byte-identically — the byte shape the D12
/// `Task` writes via `write_lyrics_file`. (The write half itself:
/// unit-pinned by `write_lyrics_file_round_trips_and_errors`; the U10 shape
/// by `full_text_duplicates_sung_lines_u10`.)
#[test]
fn d12_export_text_round_trips_through_file() {
    let h = Harness::new("lyr-d12");
    let seq = load_fixture("lindaamiga.mid");
    let mut lyrics = LyricsState::new();
    lyrics.reset_for_song(seq.current_charset().to_string(), seq.text_events());
    // One sung line (U10: it lives in the prefill AND the tail).
    let first = seq
        .text_events()
        .iter()
        .find(|rec| rec.text_type as i32 == 5)
        .expect("T11c: lindaamiga.mid must carry lyrics");
    let line = String::from_utf8_lossy(&first.text).into_owned();
    assert!(lyrics.append_live(first.track as u32, 5, &line));
    assert!(lyrics.body.contains(&line));
    assert_eq!(lyrics.live_tail(), line);
    let export = lyrics.full_text();
    assert_eq!(export, format!("{}{}", lyrics.body, line));
    let out = h.scratch().join("lyrics.txt");
    std::fs::write(&out, &export).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), export.as_bytes());
}

/// Song-settings encoding round-trip through `update`: a pre-written cfg
/// auto-loads at `Opened` (once enabled), `LyricCodec` overrides, and
/// `SongSettingsLoad` restores — all observed via the `encoding` probe.
/// (Body recompute + filters-kept: unit-pinned by
/// `song_settings_encoding_recomputes_body_keeping_filters`.)
#[test]
fn song_settings_encoding_round_trip_through_app() {
    let mut h = Harness::new("lyr-sng");
    std::fs::create_dir_all(hermetic_home().join(".dmidiplayer")).unwrap();
    std::fs::write(song_cfg("lyr-sng.mid"), "[Global]\nencoding=WINDOWS-1252\n").unwrap();
    let mut prefs = Settings::new();
    prefs.auto_song_settings = true;
    h.upd(Message::PrefsChanged(prefs));
    let seq = load_fixture("test.mid");
    open_real(&mut h, "lyr-sng.mid", &seq);
    // Auto-load restored the saved codec instead of the detected UTF-8.
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-sng.mid").as_deref(), Some("WINDOWS-1252"));
    // Override, then explicit load restores.
    h.upd(Message::LyricCodec("SHIFT_JIS".to_string()));
    h.upd(Message::SongSettingsLoad);
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-sng.mid").as_deref(), Some("WINDOWS-1252"));
}

/// Prefs commit persists the Lyrics rows and round-trips every other row
/// untouched; the song codec probe is unaffected by prefs commits.
/// (Applied-color/font/alignment derivation + filters-untouched:
/// unit-pinned by `prefs_commit_applies_lyrics_rows_without_touching_filters`.)
#[test]
fn prefs_commit_persists_lyrics_rows_untouched_others() {
    let mut h = Harness::new("lyr-prefs");
    let seq = load_fixture("lindaamiga.mid");
    open_real(&mut h, "lyr-prefs.mid", &seq);
    h.upd(Message::LyricCodec("EUC-KR".to_string()));

    let mut base = Settings::new();
    base.drums_channel = 5;
    base.solo_volume_reduction = 33;
    base.repeat_mode = 2;
    base.auto_advance = false;
    base.future_color = "red".to_string();
    base.past_color = "#1c71d8".to_string();
    base.highlight_color = "#e01b24".to_string();
    base.lyrics_font = "Monospace Bold 9".to_string();
    base.text_alignment = 1;
    base.single_color = "#123456".to_string();
    h.upd(Message::PrefsChanged(base.clone()));
    assert_eq!(persisted_settings(&h, "lyr-prefs"), base);

    // A lyrics-only second commit leaves every other row alone.
    let mut tweak = base.clone();
    tweak.future_color = "blue".to_string();
    tweak.text_alignment = 2;
    h.upd(Message::PrefsChanged(tweak.clone()));
    assert_eq!(persisted_settings(&h, "lyr-prefs"), tweak);

    // Song codec untouched by prefs commits.
    h.upd(Message::SongSettingsSave);
    assert_eq!(cfg_encoding("lyr-prefs.mid").as_deref(), Some("EUC-KR"));
}

/// Filter recompute over real `.mid` texts: preselects, track/type
/// narrowing, and the non-lyric trailing-newline rule. (App-arm wiring:
/// unit-pinned by `lyric_filters_and_codec_recompute_body`; the filter
/// matrix by state tests `reset_for_song_preselects_and_rebuilds` and
/// `refresh_body_filters_track_type_and_newline_rule`.)
#[test]
fn mid_filter_recompute_over_real_texts() {
    let seq = load_fixture("lindaamiga.mid");
    let texts = seq.text_events();
    assert_eq!(preselect_filters(texts), (Some(2), 5));
    let mut lyrics = LyricsState::new();
    lyrics.reset_for_song(seq.current_charset().to_string(), texts);
    assert_eq!(lyrics.track_filter, Some(2));
    assert_eq!(lyrics.type_filter, 5);
    // Preselect body: the 139 track-2 lyrics concatenated, no newlines.
    assert_eq!(
        texts
            .iter()
            .filter(|rec| rec.track == 2 && rec.text_type as i32 == 5)
            .count(),
        139
    );
    assert!(lyrics.body.starts_with("Ay, "));
    assert!(!lyrics.body.contains('\n'));
    // All-types on track 2 adds the "S" track name with the newline rule.
    lyrics.type_filter = 0;
    lyrics.refresh_body(texts);
    assert!(lyrics.body.contains("Ay, "));
    assert!(lyrics.body.contains("S\n"));
    // Type-only: the single copyright record + its newline, nothing else.
    lyrics.track_filter = None;
    lyrics.type_filter = 2;
    lyrics.refresh_body(texts);
    assert!(lyrics.body.contains("Pedro Lopez-Cabanil"));
    assert_eq!(lyrics.body.matches('\n').count(), 1);
    // Out-of-range type matches nothing.
    lyrics.type_filter = 99;
    lyrics.refresh_body(texts);
    assert!(lyrics.body.is_empty());
}

/// KAR mapping over a real `.kar`: `@`-headers stored as `Kar*` types score
/// as `Text` for the preselect but filter by stored type; an off-list codec
/// still decodes while the dropdown shows index 0. (State halves:
/// `preselect_counts_kar_headers_as_text`,
/// `kar_type_filter_matches_remapped_records`, and
/// `codec_option_index_is_case_insensitive_with_off_list_zero`.)
#[test]
fn kar_headers_remap_score_and_filter() {
    let seq = load_fixture("twinkle.kar");
    let texts = seq.text_events();
    // 84 Text + 6 headers scored as Text → type 1; bulk on track 1.
    assert_eq!(preselect_filters(texts), (Some(1), 1));
    let mut lyrics = LyricsState::new();
    lyrics.reset_for_song(seq.current_charset().to_string(), texts);
    // Preselect body: the 84 Text records only — headers excluded by type.
    // (`Text` is non-lyric, so each syllable carries the trailing newline.)
    assert!(!lyrics.body.contains('@'));
    assert!(lyrics.body.contains("tle \nstar; "));
    // Stored-type filters find the remapped headers.
    lyrics.type_filter = 8;
    lyrics.refresh_body(texts);
    assert!(lyrics.body.contains("@KMIDI KARAOKE FILE"));
    lyrics.type_filter = 12;
    lyrics.refresh_body(texts);
    assert!(lyrics.body.contains("@TTwinkle, Twinkle, Little Star"));
    // All-types shows headers AND lyrics together.
    lyrics.type_filter = 0;
    lyrics.refresh_body(texts);
    assert!(lyrics.body.contains("@KMIDI KARAOKE FILE"));
    assert!(lyrics.body.contains("tle \nstar; "));
    // Off-list codec: dropdown index 0, decode still works.
    lyrics.codec = "X-GOSH-OFFLIST".to_string();
    lyrics.track_filter = None;
    lyrics.refresh_body(texts);
    assert_eq!(codec_option_index(&lyrics.codec), 0);
    assert!(lyrics.body.contains("@KMIDI KARAOKE FILE"));
    // Second KAR file: same mapping, different track preselect.
    let negra = load_fixture("Negra_Sombra.kar");
    assert_eq!(preselect_filters(negra.text_events()), (Some(3), 1));
}

/// Live-tail append over real texts: the filter matrix, past/current
/// segments, the monotonic `live_seq`, and the U10 prefill+append
/// duplication in `full_text`; a filter change clears the tail. (App event
/// wiring: unit-pinned by `live_text_filters_and_decodes`; the segment
/// shape by `append_live_segments_current_past_and_seq` and
/// `full_text_duplicates_sung_lines_u10`.)
#[test]
fn live_tail_appends_with_u10_duplication() {
    let seq = load_fixture("schubert_avemaria.mid");
    let texts = seq.text_events();
    let mut lyrics = LyricsState::new();
    lyrics.reset_for_song(seq.current_charset().to_string(), texts);
    assert_eq!((lyrics.track_filter, lyrics.type_filter), (Some(3), 5));
    // Filtered-out events append nothing and don't bump the generation.
    assert!(!lyrics.append_live(2, 5, "nope"));
    assert!(!lyrics.append_live(3, 1, "nope"));
    assert_eq!(lyrics.live_seq, 0);
    assert!(!lyrics.sung);
    // Two passing appends: the previous current becomes past.
    assert!(lyrics.append_live(3, 5, "A"));
    assert!(lyrics.append_live(3, 5, "ve"));
    assert_eq!(lyrics.live_past, "A");
    assert_eq!(lyrics.live_current, "ve");
    assert_eq!(lyrics.live_tail(), "Ave");
    assert_eq!(lyrics.live_seq, 2);
    assert!(lyrics.sung);
    // U10: "Ave" is sung from the prefill AND the tail — `full_text` holds
    // both copies, exactly like the C++ buffer.
    assert!(lyrics.body.contains("Ave"));
    assert_eq!(lyrics.full_text(), format!("{}{}", lyrics.body, "Ave"));
    // A filter change (recompute) clears the whole tail, C++ parity.
    lyrics.track_filter = None;
    lyrics.refresh_body(texts);
    assert!(lyrics.live_tail().is_empty());
    assert!(!lyrics.sung);
}

/// No-snap-back: recompute keeps user filter selections (the lead-directed
/// T11 FIX — C++ `refreshLyrics` resets both to best-score). (Arm wiring:
/// unit-pinned by `lyric_filters_and_codec_recompute_body`.)
#[test]
fn recompute_keeps_user_filters_no_snap_back() {
    let seq = load_fixture("mozart_aveverum.mid");
    let texts = seq.text_events();
    let mut lyrics = LyricsState::new();
    lyrics.reset_for_song(seq.current_charset().to_string(), texts);
    assert_eq!((lyrics.track_filter, lyrics.type_filter), (Some(6), 5));
    // User picks off-preselect filters; every recompute keeps them.
    lyrics.track_filter = Some(1);
    lyrics.type_filter = 7;
    lyrics.refresh_body(texts);
    assert_eq!((lyrics.track_filter, lyrics.type_filter), (Some(1), 7));
    assert!(lyrics.body.contains("Rosegarden"));
    lyrics.codec = "ISO-8859-1".to_string();
    lyrics.refresh_body(texts);
    assert_eq!((lyrics.track_filter, lyrics.type_filter), (Some(1), 7));
    assert!(lyrics.body.contains("Rosegarden"));
}
