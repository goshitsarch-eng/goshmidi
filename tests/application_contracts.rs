//! Controller regression tests use the real engine and isolated compatible data.
use dmidiplayer::{
    application::{Controller, Effect},
    commands::{Action, Command, Preference},
    config::{
        paths::{Env, Paths},
        settings::Settings,
        song::SongSettings,
    },
    models::{Dialog, RepeatMode, Status, Theme, View},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn app(files: Vec<PathBuf>) -> (tempfile::TempDir, Controller) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    let paths = Paths::with_env(Env::for_test(&home, &config, &data, temp.path()), None);
    let mut settings = Settings::new();
    settings.auto_play = false;
    settings.auto_song_settings = false;
    settings.auto_advance = false;
    settings.save(&paths).unwrap();
    let (controller, _) = Controller::new(paths, files, Some(("Dummy".into(), "dummy".into())));
    (temp, controller)
}
fn fixture() -> PathBuf {
    std::fs::canonicalize("examples/test.mid").unwrap()
}
fn until(app: &mut Controller, predicate: impl Fn(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate(app) {
        assert!(
            Instant::now() < deadline,
            "Engine deadline: {:?}",
            app.errors
        );
        let events = app.take_events();
        app.poll(events);
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn loaded_transport_and_mix_are_real_engine_events() {
    let (_temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    app.dispatch(Command::Action(Action::Play));
    until(&mut app, |app| app.playback.status == Status::Playing);
    app.dispatch(Command::Action(Action::Pause));
    app.poll(app.take_events());
    assert_eq!(app.playback.status, Status::Paused);
    app.dispatch(Command::Action(Action::Pause));
    until(&mut app, |app| app.playback.status == Status::Playing);
    app.dispatch(Command::Action(Action::Stop));
    assert_eq!(app.playback.position_ticks, 0);
    app.dispatch(Command::Tempo(999));
    app.dispatch(Command::Volume(999));
    app.dispatch(Command::Pitch(-99));
    assert_eq!(app.playback.tempo_factor_pct, 200);
    assert_eq!(app.playback.volume_pct, 200);
    assert_eq!(app.playback.pitch_semitones, -12);
}
#[test]
fn malformed_load_clears_old_song_and_surfaces_failure() {
    let (temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    let corrupt = temp.path().join("broken.mid");
    std::fs::write(&corrupt, b"not MIDI").unwrap();
    app.dispatch(Command::OpenFiles(vec![corrupt], true));
    app.dispatch(Command::Select(1, false));
    assert!(app.song.is_none());
    until(&mut app, |app| app.loading.is_none());
    assert!(!app.errors.is_empty());
    assert!(app.song.is_none());
}
#[test]
fn replacement_and_quit_protect_dirty_playlist() {
    let (_temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    app.dispatch(Command::Quit);
    assert_eq!(app.dialog, Some(Dialog::Unsaved));
    app.dispatch(Command::CancelDialog);
    assert_eq!(app.playlist.len(), 1);
    app.dispatch(Command::Action(Action::Clear));
    assert_eq!(app.dialog, Some(Dialog::Unsaved));
    app.dispatch(Command::ConfirmDiscard);
    assert!(app.playlist.is_empty());
    assert!(app.song.is_none());
}
#[test]
fn preferences_apply_is_scoped_and_cancel_is_transactional() {
    let (_temp, mut app) = app(vec![]);
    let original = app.settings.clone();
    app.dispatch(Command::Dialog(Dialog::Preferences));
    app.dispatch(Command::EditPreference(Preference::Autoplay, "true".into()));
    app.dispatch(Command::CancelDialog);
    assert_eq!(app.settings, original);
    app.dispatch(Command::Dialog(Dialog::Preferences));
    app.dispatch(Command::EditPreference(
        Preference::SoloReduction,
        "77".into(),
    ));
    let effects = app.dispatch(Command::ApplyPreferences);
    assert_eq!(app.settings.solo_volume_reduction, 77);
    assert_eq!(
        app.settings.last_output_backend,
        original.last_output_backend
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::WriteSettings(..)))
    );
}
#[test]
fn reversed_loop_is_rejected_and_repeat_persists() {
    let (_temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    app.dispatch(Command::Loop(2, 1));
    assert!(!app.playback.loop_enabled);
    assert!(!app.errors.is_empty());
    app.dispatch(Command::Loop(1, 1));
    assert!(app.playback.loop_enabled);
    app.dispatch(Command::LoopOff);
    assert!(!app.playback.loop_enabled);
    let effects = app.dispatch(Command::Repeat(RepeatMode::Playlist));
    assert_eq!(app.settings.repeat_mode, 2);
    assert!(matches!(effects[0], Effect::WriteSettings(..)));
}
#[test]
fn all_views_are_reachable_and_channel_edits_are_bounded() {
    let (_temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    for view in [View::Lyrics, View::Channels, View::Piano] {
        app.dispatch(Command::View(view));
        assert_eq!(app.view, view);
    }
    app.dispatch(Command::ChannelMute(0, true));
    app.dispatch(Command::ChannelSolo(0, true));
    app.dispatch(Command::ChannelLock(0, true));
    app.dispatch(Command::ChannelPatch(0, 255));
    app.dispatch(Command::ChannelLevel(0, 999));
    app.dispatch(Command::ChannelName(0, "主旋律".into()));
    app.dispatch(Command::ChannelMute(255, true));
    let row = &app.channels[0];
    assert!(row.muted && row.soloed && row.locked);
    assert_eq!(row.patch, 127);
    assert_eq!(row.level_pct, 200);
    assert_eq!(row.label, "主旋律");
}
#[test]
fn stale_settings_completion_cannot_modify_new_song() {
    let (_temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    let mut saved = SongSettings::empty();
    saved.pitch = 9;
    app.dispatch(Command::SongSettingsLoaded(
        PathBuf::from("other.mid"),
        Box::new(saved),
    ));
    assert_eq!(app.playback.pitch_semitones, 0);
}
#[test]
fn theme_and_geometry_are_separate_from_compatible_ini() {
    let (_temp, mut app) = app(vec![]);
    app.dispatch(Command::Theme(Theme::Dark));
    app.dispatch(Command::Geometry(1000, 700, false));
    app.dispatch(Command::Geometry(2000, 1400, true));
    assert_eq!(app.settings.window_width, 1000);
    assert_eq!(app.appearance.theme, Theme::Dark);
    assert!(app.appearance.maximized);
    assert!(!app.settings.to_data().contains("theme="));
}
#[test]
fn distinct_song_folders_cannot_collide_and_legacy_location_is_preserved() {
    let (_temp, app) = app(vec![]);
    let a = app.paths.song_settings_for_file(Path::new("/a/song.mid"));
    let b = app.paths.song_settings_for_file(Path::new("/b/song.mid"));
    assert_ne!(a, b);
    assert!(a.starts_with(app.paths.data_dir()));
    assert!(
        app.paths
            .legacy_song_settings_path("/a/song.mid")
            .ends_with(".dmidiplayer/song.mid.cfg")
    );
}
#[test]
fn exports_do_not_duplicate_sung_lyrics() {
    let (_temp, mut app) = app(vec![fixture()]);
    until(&mut app, |app| app.song.is_some());
    let body = app.lyrics.body.clone();
    app.lyrics.append_live(1, app.lyrics.type_filter, "sung");
    let effects = app.dispatch(Command::Action(Action::CopyLyrics));
    assert!(matches!(&effects[0],Effect::Clipboard(text) if *text==body));
}
#[test]
fn executable_validation_decodes_and_rejects_bad_files() {
    let binary = env!("CARGO_BIN_EXE_dmidiplayer");
    let valid = std::process::Command::new(binary)
        .args(["--dry-run", "examples/test.mid"])
        .output()
        .unwrap();
    assert!(valid.status.success());
    assert!(String::from_utf8_lossy(&valid.stdout).contains("SMF"));
    let temp = tempfile::tempdir().unwrap();
    let corrupt = temp.path().join("broken.mid");
    std::fs::write(&corrupt, b"MThd").unwrap();
    let failed = std::process::Command::new(binary)
        .arg("--dry-run")
        .arg(corrupt)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(
        !std::process::Command::new(binary)
            .args(["--dry-run", "absent.mid"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn preview_expiry_cannot_release_a_newer_audition() {
    let (_temp, mut app) = app(vec![]);
    app.dispatch(Command::Preview(0, 60));
    app.dispatch(Command::Preview(1, 64));
    app.dispatch(Command::PreviewExpired(1));
    assert!(!app.piano.notes_on[0][60]);
    assert!(app.piano.notes_on[1][64]);
    app.dispatch(Command::PreviewOff);
    assert!(!app.piano.notes_on[1][64]);
}
#[test]
fn invalid_preference_color_keeps_the_draft_open() {
    let (_temp, mut app) = app(vec![]);
    app.dispatch(Command::Dialog(Dialog::Preferences));
    app.dispatch(Command::EditPreference(
        Preference::FutureColor,
        "invalid color".into(),
    ));
    assert!(app.dispatch(Command::ApplyPreferences).is_empty());
    assert_eq!(app.dialog, Some(Dialog::Preferences));
    assert!(!app.errors.is_empty());
}
#[cfg(unix)]
#[test]
fn song_settings_use_the_original_non_utf8_path() {
    use std::os::unix::ffi::OsStringExt;
    let (temp, mut app) = app(vec![]);
    let path = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"song-\xff.mid".to_vec()));
    std::fs::copy(fixture(), &path).unwrap();
    app.dispatch(Command::OpenFiles(vec![path.clone()], true));
    until(&mut app, |app| app.song.is_some());
    let effects = app.dispatch(Command::Action(Action::SaveSettings));
    assert!(
        matches!(&effects[0], Effect::WriteSong(saved, _) if *saved == app.paths.song_settings_for_file(&path))
    );
}
#[cfg(unix)]
#[test]
fn failed_load_events_preserve_non_utf8_file_identity() {
    use dmidiplayer::core::player::PlayerEvent;
    use std::os::unix::ffi::OsStringExt;
    let (_temp, mut app) = app(vec![]);
    let path = PathBuf::from(std::ffi::OsString::from_vec(b"bad-\xff.mid".to_vec()));
    app.loading = Some(path.clone());
    let other = PathBuf::from(std::ffi::OsString::from_vec(b"bad-\xfe.mid".to_vec()));
    app.poll(vec![PlayerEvent::LoadFailed {
        file: other,
        errors: vec!["bad".into()],
    }]);
    assert_eq!(app.loading, Some(path.clone()));
    app.poll(vec![PlayerEvent::LoadFailed {
        file: path,
        errors: vec!["bad".into()],
    }]);
    assert!(app.loading.is_none());
}
#[test]
fn failed_quit_requires_an_explicit_choice() {
    let (_temp, mut app) = app(vec![]);
    assert!(app.dispatch(Command::QuitWithoutSaving).is_empty());
    app.dispatch(Command::QuitSaveFailed("Permission denied".into()));
    assert_eq!(app.dialog, Some(Dialog::SaveFailure));
    let effects = app.dispatch(Command::QuitWithoutSaving);
    assert!(matches!(effects.as_slice(), [Effect::Close]));
}
#[test]
fn non_synth_output_does_not_require_a_saved_soundfont() {
    let (_temp, mut app) = app(vec![]);
    app.dispatch(Command::Dialog(Dialog::MidiSetup));
    app.dispatch(Command::MidiBackend("Dummy".into()));
    app.dispatch(Command::MidiPort("dummy".into()));
    app.dispatch(Command::MidiSoundfont(
        "/nonexistent/old-linux-font.sf2".into(),
    ));
    app.dispatch(Command::MidiApply);
    assert!(app.dialog.is_none());
    assert_eq!(app.midi.backend, "Dummy");
}
