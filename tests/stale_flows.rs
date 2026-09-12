//! T15 integration: stale-path fixtures for the sandbox data-migration story.
//!
//! `tests/fixtures/stale/` holds the §6 QA shapes — a native-install config,
//! a song `.cfg`, and a `.lst` whose absolute entries point at a dead home
//! dir. Loading them must never panic, never drop entries, and never rewrite
//! paths: stale values load verbatim so S3's re-prompt UX can name them
//! (packaging.md §6). These tests pin the fixture shapes + the verbatim-load
//! seam; the bundled-fallback + toast + D11 offer land in S3.

use std::path::{Path, PathBuf};

use dmidiplayer::config::settings::Settings;
use dmidiplayer::config::song::SongSettings;
use dmidiplayer::state::midi::{
    MidiState, is_stale_soundfont, stale_soundfont_hint, stale_soundfont_toast,
};
use dmidiplayer::state::playlist::PlaylistState;
use dmidiplayer::ui::midi_setup;

fn stale(rel: &str) -> PathBuf {
    Path::new("tests/fixtures/stale").join(rel)
}

/// Every absolute path named in the fixtures is dead on this machine — the
/// precondition the re-prompt UX is built on. Fails loudly (not silently
/// un-stale) if the fixture prefix ever exists.
#[test]
fn stale_fixture_paths_are_actually_dead() {
    for leaf in ["dmidiplayer.conf", "gig.lst", "song.cfg"] {
        let text = std::fs::read_to_string(stale(leaf))
            .unwrap_or_else(|why| panic!("T15: cannot read stale fixture {leaf}: {why}"));
        assert!(!text.is_empty(), "T15: stale fixture {leaf} is empty");
    }
    assert!(
        !Path::new("/home/staleuser").exists(),
        "T15: /home/staleuser exists — fixtures are not stale here"
    );
}

/// Stale config values (dead `soundFont`/`lastPlayList`/`lastDirectory`/
/// `recentFiles`) load verbatim and round-trip byte-stable: nothing is
/// dropped or rewritten at load (S3 adds the re-prompt on top of this).
#[test]
fn stale_config_loads_verbatim_and_round_trips() {
    let mut s = Settings::new();
    s.load_file(&stale("dmidiplayer.conf"))
        .expect("T15: stale config must parse");
    assert_eq!(s.sound_font, "/home/staleuser/sf/old-gm.sf2");
    assert_eq!(s.last_play_list, "/home/staleuser/Music/gig.lst");
    assert_eq!(s.last_directory, "/home/staleuser/Music/midi");
    assert_eq!(
        s.recent_files,
        vec![
            "/home/staleuser/Music/a.mid".to_string(),
            "/home/staleuser/Music/b.kar".to_string(),
        ]
    );
    for p in [
        &s.sound_font,
        &s.last_play_list,
        &s.last_directory,
        &s.recent_files[0],
    ] {
        assert!(!Path::new(p).exists(), "T15: fixture path exists: {p}");
    }
    // Round-trip: stale values survive save→load unchanged (P7: never
    // silently drop user data).
    let mut again = Settings::new();
    again
        .load_from_str(&s.to_data())
        .expect("T15: stale round-trip must parse");
    assert_eq!(again.sound_font, s.sound_font);
    assert_eq!(again.last_play_list, s.last_play_list);
    assert_eq!(again.last_directory, s.last_directory);
    assert_eq!(again.recent_files, s.recent_files);
}

/// Stale `.lst`: dead absolute entries are kept verbatim (A14 — load never
/// existence-checks), and the relative entry resolves against the `.lst`
/// dir, existing or not.
#[test]
fn stale_lst_keeps_dead_entries() {
    let mut list = PlaylistState::new();
    assert!(list.load(&stale("gig.lst")), "T15: stale .lst must load");
    let items = list.items();
    assert_eq!(items.len(), 4, "T15: stale entries dropped: {items:?}");
    assert_eq!(items[0], PathBuf::from("/home/staleuser/Music/opener.mid"));
    assert_eq!(
        items[1],
        PathBuf::from("/home/staleuser/Music/with space.kar")
    );
    assert_eq!(
        items[2],
        PathBuf::from("/home/staleuser/Music/set-2/closer.mid")
    );
    assert!(
        items[3].ends_with("tests/fixtures/stale/neighbor.mid"),
        "T15: relative entry mis-resolved: {:?}",
        items[3]
    );
}

/// Stale song `.cfg` parses with the dead `file` probe carried verbatim
/// (`file` is write-only metadata the loader never resolves).
#[test]
fn stale_song_cfg_parses_with_dead_file_probe() {
    let song = SongSettings::load(&stale("song.cfg")).expect("T15: stale .cfg must parse");
    assert_eq!(song.file, "/home/staleuser/Music/opener.mid");
    assert!(!Path::new(&song.file).exists());
    assert_eq!(song.encoding.as_deref(), Some("UTF-8"));
    assert_eq!(song.volume, Some(150));
    assert_eq!(song.pitch, -2);
    assert_eq!(song.timeskew, Some(100));
    let ch = song.channels[0].as_ref().expect("T15: channel group lost");
    assert_eq!(ch.name.as_deref(), Some("Piano"));
    assert_eq!(ch.level, Some(100));
    assert!(song.channels[1..].iter().all(Option::is_none));
}

/// S3 re-prompt contract against the S2 fixture shapes (packaging §6, ux
/// §15.1): the stale config's `soundFont` probes stale, the D3 hint and the
/// startup toast both name it, and the D3 view model raises the warning flag
/// (the `warning_text` style). The engine fallback send + the toast push stay
/// unit-pinned in `app.rs` (`startup_stale_soundfont_*`).
#[test]
fn stale_soundfont_fixture_triggers_reprompt_contract() {
    let mut s = Settings::new();
    s.load_file(&stale("dmidiplayer.conf"))
        .expect("T15: stale config must parse");
    let stale_path = s.sound_font.clone();
    assert_eq!(stale_path, "/home/staleuser/sf/old-gm.sf2");
    assert!(
        is_stale_soundfont(&stale_path),
        "T15-S3: the fixture soundFont must probe stale"
    );
    // D3 hint names the stale entry + offers Browse (D11).
    let hint = stale_soundfont_hint(&stale_path);
    assert!(hint.contains(&stale_path), "T15-S3: hint: {hint:?}");
    assert!(hint.contains("Browse…"), "T15-S3: hint: {hint:?}");
    // Startup toast names the stale path + the bundled fallback + MIDI Setup.
    let toast = stale_soundfont_toast(&stale_path, "/app/share/soundfonts/FluidR3Mono_GM.sf3");
    assert!(toast.contains(&stale_path), "T15-S3: toast: {toast:?}");
    assert!(
        toast.contains("FluidR3Mono_GM.sf3"),
        "T15-S3: toast: {toast:?}"
    );
    assert!(toast.contains("MIDI Setup"), "T15-S3: toast: {toast:?}");
    // D3 view model raises the warning flag for the stale entry.
    let mut midi = MidiState::new();
    midi.soundfont = stale_path.clone();
    let vm = midi_setup::view_model(&midi, None);
    assert!(vm.status_warning, "T15-S3: D3 must flag the stale entry");
    assert!(
        vm.status.contains(&stale_path),
        "T15-S3: D3 status: {:?}",
        vm.status
    );
}
