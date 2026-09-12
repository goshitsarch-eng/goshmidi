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
use dmidiplayer::state::playlist::PlaylistState;

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
