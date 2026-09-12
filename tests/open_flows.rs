//! T10c integration: D1 open flows through `App::update`.
//!
//! Drives the real app (hermetic temp HOME, real `examples/*.mid` fixtures,
//! headless engine) through the D1 chooser-result messages plus the CLI
//! missing-file path. No pixel matching: playlist state is observed through
//! the song-settings `file` probe (see `common`), `lastPlayList` through a
//! config round-trip. Toast text and the `pending_autoplay` flag stay
//! unit-pinned in `app.rs` (named per test); this file pins the wiring.

mod common;

use common::{Harness, canon, fixture};
use dmidiplayer::message::Message;

/// D1 multi-select (P2): `OpenFilesChosen(Ok)` REPLACES the playlist, the
/// first item becomes current, and nothing is "loaded" until the engine's
/// `Opened` arrives (async boundary: the real `Opened` sits unpumped).
#[test]
fn d1_multi_select_replaces_playlist_with_first_current() {
    let mut h = Harness::new("open-replace");
    h.upd(Message::OpenFilesChosen(Ok(vec![fixture(
        "lindaamiga.mid",
    )])));
    let a = fixture("test.mid");
    let b = fixture("mozart_aveverum.mid");
    h.upd(Message::OpenFilesChosen(Ok(vec![a.clone(), b.clone()])));

    // The seed is gone and nothing loaded synchronously: a save with no
    // `Opened` yet writes nothing.
    h.assert_save_without_song_writes_nothing("pre-opened");
    // Current = first item, with no reselect.
    assert_eq!(h.probe_current_file("first"), canon(&a));
    h.upd(Message::PlaylistSelect(1));
    assert_eq!(h.probe_current_file("second"), canon(&b));
    // Out-of-range select is ignored (A14): current stays second.
    h.upd(Message::PlaylistSelect(7));
    assert_eq!(h.probe_current_file("oor-keeps"), canon(&b));
    // Drain pins the exact length: [a, b] and nothing else.
    h.upd(Message::PlaylistRemove(0));
    assert_eq!(h.probe_current_file("after-remove0"), canon(&b));
    h.upd(Message::PlaylistRemove(0));
    let drained = h.probe_current_file("drained");
    assert!(drained.is_empty(), "drain left {drained:?}");
}

/// D1 single `.lst` (T10a ruling): loads as a playlist WITH autoplay wiring —
/// same `open_files` path as songs. Relative entries resolve against the
/// `.lst` dir, blanks/unsupported lines filter out, `lastPlayList` records.
#[test]
fn d1_single_lst_loads_list_and_records_source() {
    let mut h = Harness::new("open-lst");
    let gig = h.scratch().join("gig");
    std::fs::create_dir_all(&gig).unwrap();
    std::fs::copy(fixture("test.mid"), gig.join("test.mid")).unwrap();
    std::fs::copy(
        fixture("mozart_aveverum.mid"),
        gig.join("mozart_aveverum.mid"),
    )
    .unwrap();
    let list = gig.join("gig.lst");
    std::fs::write(&list, "test.mid\nnotes.txt\n\nmozart_aveverum.mid\n").unwrap();

    h.upd(Message::OpenFilesChosen(Ok(vec![list.clone()])));

    assert_eq!(h.probe_current_file("first"), canon(&gig.join("test.mid")));
    h.upd(Message::PlaylistSelect(1));
    assert_eq!(
        h.probe_current_file("second"),
        canon(&gig.join("mozart_aveverum.mid"))
    );
    // Exact length 2 (junk + blank filtered): drain to empty.
    h.upd(Message::PlaylistRemove(0));
    h.upd(Message::PlaylistRemove(0));
    let drained = h.probe_current_file("drained");
    assert!(drained.is_empty(), "drain left {drained:?}");
    // The source records in `lastPlayList` and persists.
    assert_eq!(
        h.persisted_last_play_list(),
        list.to_string_lossy().into_owned()
    );
    // (Autoplay pending flag: unit-pinned by
    // `open_files_chosen_single_lst_loads_playlist_with_autoplay`.)
}

/// D1 error half of error→toast: the playlist and current item are untouched
/// and nothing persists. (Toast TEXT: unit-pinned by
/// `open_files_chosen_empty_is_silent_noop_and_err_toasts`.)
#[test]
fn d1_error_leaves_playlist_untouched() {
    let mut h = Harness::new("open-error");
    let a = fixture("test.mid");
    h.upd(Message::OpenFilesChosen(Ok(vec![a.clone()])));
    assert_eq!(h.probe_current_file("seed"), canon(&a));
    assert!(!h.config_exists());

    h.upd(Message::OpenFilesChosen(Err(
        "Could not open files: boom".to_string()
    )));

    assert_eq!(h.probe_current_file("after-err"), canon(&a));
    assert!(!h.config_exists());
}

/// `ChooserCancelled` (portal cancel from any chooser) is a silent no-op:
/// same playlist + current, nothing written.
/// (Dialog-stack preservation: unit-pinned by `chooser_cancelled_is_silent_noop`.)
#[test]
fn chooser_cancelled_is_silent_noop() {
    let mut h = Harness::new("open-cancel");
    let a = fixture("test.mid");
    h.upd(Message::OpenFilesChosen(Ok(vec![a.clone()])));
    assert_eq!(h.probe_current_file("seed"), canon(&a));

    h.upd(Message::ChooserCancelled);

    assert_eq!(h.probe_current_file("after-cancel"), canon(&a));
    assert!(!h.config_exists());
}

/// `Ok(empty)` is unreachable from the portal but stays a defensive silent
/// no-op on the same path.
#[test]
fn chosen_empty_is_silent_noop() {
    let mut h = Harness::new("open-empty");
    let a = fixture("test.mid");
    h.upd(Message::OpenFilesChosen(Ok(vec![a.clone()])));
    assert_eq!(h.probe_current_file("seed"), canon(&a));

    h.upd(Message::OpenFilesChosen(Ok(vec![])));

    assert_eq!(h.probe_current_file("after-empty"), canon(&a));
    assert!(!h.config_exists());
}

/// Missing CLI positionals report on stderr (`File not found: …`, C++ parity)
/// while `--dry-run` validates the rest headlessly and exits 0.
#[test]
fn missing_file_reports_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_dmidiplayer");
    let out = std::process::Command::new(bin)
        .args([
            "--dry-run",
            "definitely-not-here-12345.mid",
            "examples/test.mid",
        ])
        .output()
        .expect("run dmidiplayer --dry-run");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stderr.contains("File not found: definitely-not-here-12345.mid"),
        "stderr was: {stderr}",
    );
    assert!(stdout.contains("dry-run: files=1"), "stdout was: {stdout}",);
    assert!(stdout.contains("test.mid"), "stdout was: {stdout}",);
    assert!(
        stdout.contains("dry-run: OK (GUI skipped)"),
        "stdout was: {stdout}",
    );
}
