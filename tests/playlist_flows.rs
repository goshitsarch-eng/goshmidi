//! T10c integration: playlist flows through `App::update`.
//!
//! Same hermetic harness as `open_flows` (temp HOME, real fixtures, headless
//! engine): row activation, remove/move/shuffle/clear with A14 index pins,
//! D9 load (no autoplay), D10 save round-trip, portal-path replacement, and
//! the second-open forward shape. Playlist state is observed through the
//! song-settings `file` probe (see `common`); exact lengths are pinned by
//! draining. Toast text, the `pending_autoplay`/`dirty` flags, and the window
//! "present" half stay unit-pinned in `app.rs` (named per test).

mod common;

use std::path::PathBuf;

use common::{Harness, canon, fixture};
use dmidiplayer::message::{LoadedPlaylist, Message};
use dmidiplayer::state::playlist::PlaylistState;

/// Row-activated (double-click/Enter, U11): the activated row becomes current
/// (the load+play target). Select-only follows the click; an out-of-range
/// activate keeps current (A14). (Load dispatch + pending autoplay:
/// unit-pinned by `playlist_row_activated_*`.)
#[test]
fn row_activated_targets_current_row() {
    let mut h = Harness::new("pl-activate");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    let c = fixture("lindaamiga.mid");
    h.upd(Message::OpenFiles(
        vec![a.clone(), b.clone(), c.clone()],
        true,
    ));

    h.upd(Message::PlaylistRowActivated(2));
    assert_eq!(h.probe_current_file("activated"), canon(&c));
    h.upd(Message::PlaylistSelect(0));
    assert_eq!(h.probe_current_file("selected"), canon(&a));
    h.upd(Message::PlaylistRowActivated(99));
    assert_eq!(h.probe_current_file("oor-keeps"), canon(&a));
    // No message on this path persists the global config.
    assert!(!h.config_exists());
}

/// Remove keeps the index (clamping only past the end), move carries current
/// with the item, clear drops to none — all A14, all through `update`.
#[test]
fn remove_move_clear_keep_a14_index() {
    let mut h = Harness::new("pl-edit");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    let c = fixture("lindaamiga.mid");
    h.upd(Message::OpenFiles(
        vec![a.clone(), b.clone(), c.clone()],
        true,
    ));

    // Remove keeps the index: drop row 0 with current=1 → current still 1 → c.
    h.upd(Message::PlaylistSelect(1));
    h.upd(Message::PlaylistRemove(0));
    assert_eq!(h.probe_current_file("remove-keeps"), canon(&c));
    // Remove at the end clamps: drop row 1 (c) → current 1→0 → b.
    h.upd(Message::PlaylistRemove(1));
    assert_eq!(h.probe_current_file("remove-clamps"), canon(&b));
    // Clear → no current.
    h.upd(Message::PlaylistClear);
    let cleared = h.probe_current_file("cleared");
    assert!(cleared.is_empty(), "clear left {cleared:?}");

    // Move carries current with the item: reseed, current=1 (b), move_up(1).
    h.upd(Message::OpenFiles(
        vec![a.clone(), b.clone(), c.clone()],
        true,
    ));
    h.upd(Message::PlaylistSelect(1));
    h.upd(Message::PlaylistMoveUp(1));
    // [b,a,c], current followed b to 0.
    assert_eq!(h.probe_current_file("moved-with-item"), canon(&b));
    h.upd(Message::PlaylistSelect(1));
    assert_eq!(h.probe_current_file("swapped-neighbor"), canon(&a));
    // move_down(0) restores [a,b,c] with current back at 1 (b).
    h.upd(Message::PlaylistSelect(0));
    h.upd(Message::PlaylistMoveDown(0));
    assert_eq!(h.probe_current_file("moved-down"), canon(&b));
    // No-op edge: move_up(0) leaves current=1 (b) alone.
    h.upd(Message::PlaylistMoveUp(0));
    assert_eq!(h.probe_current_file("up-at-top-noop"), canon(&b));
}

/// Shuffle through `update` preserves the item set (order-agnostic — the
/// app-level shuffle is time-seeded); the deterministic `shuffle_with_seed`
/// pins the A14 half directly: current index untouched, same items.
#[test]
fn shuffle_preserves_item_set_and_current() {
    let mut h = Harness::new("pl-shuffle");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    let c = fixture("lindaamiga.mid");
    h.upd(Message::OpenFiles(
        vec![a.clone(), b.clone(), c.clone()],
        true,
    ));
    h.upd(Message::PlaylistShuffle);
    let mut got = Vec::new();
    for (i, probe) in ["s0", "s1", "s2"].iter().enumerate() {
        h.upd(Message::PlaylistSelect(i));
        got.push(h.probe_current_file(probe));
    }
    got.sort();
    let mut want = vec![canon(&a), canon(&b), canon(&c)];
    want.sort();
    assert_eq!(got, want);

    // Deterministic A14 half: current untouched by the shuffle.
    let mut p = PlaylistState::new();
    let items = vec![
        PathBuf::from("/m/a.mid"),
        PathBuf::from("/m/b.mid"),
        PathBuf::from("/m/c.mid"),
    ];
    p.set_items(items.clone());
    p.set_current_index(1);
    p.shuffle_with_seed(0x1234_5678);
    assert_eq!(p.current_index(), 1);
    let mut shuffled = p.items().to_vec();
    shuffled.sort();
    assert_eq!(shuffled, items);
}

/// D9 load REPLACES the playlist and records the source, but loads no song
/// (unlike D1 — no autoplay path). Error half: toast + untouched.
/// (Toast text + `dirty=false`: unit-pinned by
/// `playlist_loaded_applies_without_autoplay`.)
#[test]
fn d9_load_replaces_without_loading_song() {
    let mut h = Harness::new("pl-d9");
    h.upd(Message::OpenFiles(vec![fixture("lindaamiga.mid")], true));
    let a = canon(&fixture("test.mid"));
    let b = canon(&fixture("mozart_aveverum.mid"));
    let source = h.scratch().join("gig.lst");
    std::fs::write(&source, format!("{a}\n{b}\n")).unwrap();
    // The D9 chooser result shape, built exactly as the D9 `Task` builds it
    // (`PlaylistState::load` verbatim — no parallel parser).
    let mut parsed = PlaylistState::new();
    assert!(parsed.load(&source));
    h.upd(Message::PlaylistLoaded(Ok(LoadedPlaylist {
        source: Some(source.clone()),
        items: parsed.items().to_vec(),
    })));

    // No song loads: a save with no `Opened` writes nothing …
    h.assert_save_without_song_writes_nothing("d9-no-load");
    // … but the playlist IS replaced.
    assert_eq!(h.probe_current_file("first"), a);
    h.upd(Message::PlaylistSelect(1));
    assert_eq!(h.probe_current_file("second"), b);
    assert_eq!(
        h.persisted_last_play_list(),
        source.to_string_lossy().into_owned()
    );
    // Error half: playlist + current untouched.
    h.upd(Message::PlaylistLoaded(Err(
        "Could not open playlist: boom".to_string(),
    )));
    assert_eq!(h.probe_current_file("err-keeps"), b);
}

/// D10 round-trip: save the items (absolute paths, LF — the byte shape the
/// D10 `Task` writes via `PlaylistState::save`), reload, compare. App wiring
/// half: `PlaylistSaved(Ok)` records the source; error half leaves state.
#[test]
fn d10_save_round_trip_and_source_recorded() {
    let mut h = Harness::new("pl-d10");
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    h.upd(Message::OpenFiles(vec![a.clone(), b.clone()], true));

    // D10 `Task` body over the same items: snapshot → `save` verbatim.
    let mut saved = PlaylistState::new();
    saved.set_items(vec![PathBuf::from(canon(&a)), PathBuf::from(canon(&b))]);
    let out = h.scratch().join("saved.lst");
    assert!(saved.save(&out));
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(
        bytes,
        format!("{}\n{}\n", canon(&a), canon(&b)).into_bytes()
    );
    let mut reloaded = PlaylistState::new();
    assert!(reloaded.load(&out));
    assert_eq!(reloaded.items(), saved.items());

    // App wiring half: `PlaylistSaved(Ok)` records the source …
    h.upd(Message::PlaylistSaved(Ok(out.clone())));
    assert_eq!(
        h.persisted_last_play_list(),
        out.to_string_lossy().into_owned()
    );
    // … and the error half toasts without touching the playlist (current=0).
    h.upd(Message::PlaylistSaved(Err(
        "Could not save playlist: denied".to_string(),
    )));
    assert_eq!(h.probe_current_file("err-keeps"), canon(&a));
}

/// Portal document paths replace like normal paths (P2): a literal
/// never-created `/run/…/doc/…` path is kept (A14, no existence check) and
/// replaces, and a real file under a fake doc dir replaces exactly.
#[test]
fn portal_doc_paths_replace_like_normal() {
    let mut h = Harness::new("pl-portal");
    h.upd(Message::OpenFiles(
        vec![fixture("test.mid"), fixture("lindaamiga.mid")],
        true,
    ));

    let portal = PathBuf::from("/run/user/1000/doc/deadbeef/song.mid");
    h.upd(Message::OpenFiles(vec![portal], true));
    let got = h.probe_current_file("portal");
    assert!(
        PathBuf::from(&got).is_absolute(),
        "portal path kept absolute: {got}",
    );
    assert!(
        got.ends_with("doc/deadbeef/song.mid"),
        "portal tail preserved: {got}",
    );
    // Drain pins the exact length 1 (the seed is gone).
    h.upd(Message::PlaylistRemove(0));
    let drained = h.probe_current_file("portal-drained");
    assert!(drained.is_empty(), "drain left {drained:?}");

    // Real file under a fake doc dir: exact replace.
    let doc = h.scratch().join("run/1000/doc/deadbeef");
    std::fs::create_dir_all(&doc).unwrap();
    std::fs::copy(fixture("test.mid"), doc.join("song.mid")).unwrap();
    h.upd(Message::OpenFiles(vec![doc.join("song.mid")], true));
    assert_eq!(
        h.probe_current_file("doc-file"),
        canon(&doc.join("song.mid"))
    );
    h.upd(Message::PlaylistRemove(0));
    let drained = h.probe_current_file("doc-drained");
    assert!(drained.is_empty(), "drain left {drained:?}");
}

/// Rust successor shape of `tests/check_single_instance_open.cmake` (retired):
/// a forwarded second open lands as `OpenFiles(paths, replace=true)` —
/// REPLACE into the non-empty running state. The `dbus_activation` override
/// itself runs without panicking on every variant (its `Task` output —
/// `OpenFiles` + `gain_focus` present — is runtime-delivered, so "present"
/// stays a code-shape pin; the state half is asserted here).
#[test]
fn second_open_forwards_replace_into_running_state() {
    use cosmic::Application;
    use cosmic::dbus_activation::{Details, Message as Activation};

    fn activation(msg: Details<String, Vec<String>>) -> Activation<String, Vec<String>> {
        Activation {
            activation_token: None,
            desktop_startup_id: None,
            msg,
        }
    }

    let mut h = Harness::new("pl-second");
    h.upd(Message::OpenFiles(
        vec![fixture("test.mid"), fixture("lindaamiga.mid")],
        true,
    ));

    // The state half: replace=true into non-empty state replaces + selects first.
    let new = fixture("mozart_aveverum.mid");
    h.upd(Message::OpenFiles(vec![new.clone()], true));
    assert_eq!(h.probe_current_file("forwarded"), canon(&new));
    h.upd(Message::PlaylistRemove(0));
    let drained = h.probe_current_file("forwarded-drained");
    assert!(drained.is_empty(), "drain left {drained:?}");

    // The override runs cleanly on every variant.
    std::mem::drop(h.app.dbus_activation(activation(Details::ActivateAction {
        action: "open".to_string(),
        args: vec!["/m/new.mid".to_string()],
    })));
    std::mem::drop(h.app.dbus_activation(activation(Details::Open {
        url: vec![url::Url::parse("file:///m/a.mid").unwrap()],
    })));
    std::mem::drop(h.app.dbus_activation(activation(Details::Activate)));
}
