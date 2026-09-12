//! Shared hermetic harness for the T10c integration tests.
//!
//! Every test drives the real [`App`][dmidiplayer::app::App] through the public
//! `cosmic::Application` trait (`init` + `update`) with real `examples/*.mid`
//! fixtures. No display, no hardware: the engine thread runs headless (its
//! answers queue in an unpumped receiver) and `Opened` is fed synthetically.
//!
//! Hermeticity: one process-wide temp HOME (set exactly once, so parallel
//! tests in the binary stay sound) plus a per-test scratch dir and a per-test
//! portable config. All writes land under temp dirs; the CWD is only ever
//! read (fixture paths, `./examples/` scan). Song `.cfg` probe titles are
//! unique per test so parallel tests never share a file.
//!
//! Observability contract (`App` fields are private and `src/` is read-only
//! for T10c, so integration tests pin behavior through filesystem effects).
//! Playlist contents / current item surface via `SongSettingsSave`, which
//! records `playlist.current()` in `<title>.cfg` (see `probe_current_file`);
//! `lastPlayList` / source recording surfaces via `MidiApply`, which persists
//! the config without touching `lastPlayList` (see `persisted_last_play_list`);
//! silence is no state change (probe) plus no config write (`config_exists`).
//! Toast TEXT, the `pending_autoplay` flag, the `dirty` flag, and the window
//! "present" half of single-instance forward stay covered by the `app.rs`
//! unit tests, which see the private state; each integration test names the
//! unit test that pins its unobservable half.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cosmic::Application;
use cosmic::app::Core;
use dmidiplayer::app::{App, Flags};
use dmidiplayer::config::paths::{Env, Paths};
use dmidiplayer::config::settings::Settings;
use dmidiplayer::config::song::SongSettings;
use dmidiplayer::message::{Message, PlayerEvent, SongSnapshot};

/// Process-wide temp HOME, created and installed exactly once per test
/// binary (each `tests/*.rs` file is its own process, so binaries never
/// share it). The `TempDir` lives in the `OnceLock` for the whole run.
static TEMP_HOME: OnceLock<tempfile::TempDir> = OnceLock::new();

/// Temp HOME every harness resolves `$HOME` / `$XDG_*` against.
///
/// `XDG_CONFIG_HOME` / `XDG_DATA_HOME` are overridden too: `Env::from_os`
/// prefers them when set, and a real developer/CI environment would otherwise
/// leak through the temp HOME.
pub fn hermetic_home() -> &'static Path {
    TEMP_HOME
        .get_or_init(|| {
            let tmp = tempfile::tempdir().expect("T10c: temp HOME");
            // SAFETY: installed exactly once via `OnceLock` before any `App`
            // reads the environment, and never mutated again — every test in
            // this binary observes the same values, so there is no race.
            unsafe {
                std::env::set_var("HOME", tmp.path());
                std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));
                std::env::set_var("XDG_DATA_HOME", tmp.path().join(".local/share"));
            }
            tmp
        })
        .path()
}

/// A real `App` plus its per-test scratch space.
pub struct Harness {
    /// The app under test (public so tests can reach trait methods).
    pub app: App,
    /// Per-test scratch dir (playlists, fake doc dirs, portable config).
    /// Held for its `Drop` guard; see [`Harness::scratch`].
    tmp: tempfile::TempDir,
    home: &'static Path,
    /// Per-test portable config: parallel `MidiApply` probes never share a file.
    config: PathBuf,
    /// Unique prefix for this test's song-`.cfg` probe titles.
    tag: String,
    probe_seq: usize,
}

impl Harness {
    /// Boot a real `App` (`init` path: settings load, playlist restore,
    /// engine spawn) with `tag` identifying this test's probe titles.
    pub fn new(tag: &str) -> Self {
        let home = hermetic_home();
        // Tripwire: never silently run against the user's real HOME.
        assert_eq!(
            std::env::var_os("HOME").as_deref(),
            Some(home.as_os_str()),
            "T10c: temp HOME must win over the process environment",
        );
        let tmp = tempfile::tempdir().expect("T10c: per-test scratch");
        let config = tmp.path().join(format!("{tag}.conf"));
        let flags = Flags::new(vec![], None, None, true, Some(config.clone()));
        let (app, _task) = App::init(Core::default(), flags);
        Self {
            app,
            tmp,
            home,
            config,
            tag: tag.to_string(),
            probe_seq: 0,
        }
    }

    /// Per-test scratch dir (auto-removed).
    pub fn scratch(&self) -> &Path {
        self.tmp.path()
    }

    fn data_dir(&self) -> PathBuf {
        self.home.join(".dmidiplayer")
    }

    fn cfg_path(&self, title: &str) -> PathBuf {
        self.data_dir().join(format!("{title}.cfg"))
    }

    /// Drive one message through `App::update` (the `Task` is dropped —
    /// portal choosers never run headless; their results arrive as messages).
    pub fn upd(&mut self, msg: Message) {
        std::mem::drop(self.app.update(msg));
    }

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
        }
    }

    fn next_title(&mut self, probe: &str) -> String {
        self.probe_seq += 1;
        format!("{}-p{:02}-{probe}.mid", self.tag, self.probe_seq)
    }

    /// Feed a synthetic `Opened` for a fresh title, save song settings, and
    /// return the recorded `file` — i.e. the app's current playlist item
    /// (`""` when there is none). Each call uses a fresh title so probes
    /// never read each other's files.
    pub fn probe_current_file(&mut self, probe: &str) -> String {
        let title = self.next_title(probe);
        self.upd(Message::Player(PlayerEvent::Opened(Box::new(
            Self::snapshot(&title),
        ))));
        self.upd(Message::SongSettingsSave);
        SongSettings::load(&self.cfg_path(&title))
            .unwrap_or_else(|| panic!("T10c: expected a song cfg for {title}"))
            .file
    }

    /// `SongSettingsSave` with no song loaded must write nothing (the
    /// `save_song_settings` early return). Used to pin "no load happened".
    pub fn assert_save_without_song_writes_nothing(&mut self, probe: &str) {
        let title = self.next_title(probe);
        self.upd(Message::SongSettingsSave);
        assert!(
            !self.cfg_path(&title).exists(),
            "T10c: no song is loaded, so {title}.cfg must not exist",
        );
    }

    /// Persist via `MidiApply` (which saves the config but never touches
    /// `lastPlayList`) and read `lastPlayList` back through the real reader.
    pub fn persisted_last_play_list(&mut self) -> String {
        self.upd(Message::MidiApply);
        let env = Env::for_test(self.home, self.home, self.home, self.home);
        let paths = Paths::with_env(env, Some(Some(self.config.clone())));
        Settings::load(&paths).last_play_list
    }

    /// Whether this test's portable config has been written yet.
    pub fn config_exists(&self) -> bool {
        self.config.exists()
    }
}

/// Repo fixture path (`cargo test` runs with the CWD at the package root).
/// Panics fast with a clear message if the CWD assumption breaks.
pub fn fixture(name: &str) -> PathBuf {
    let path = PathBuf::from("examples").join(name);
    assert!(
        path.is_file(),
        "T10c: missing fixture {} (run cargo test from the repo root)",
        path.display(),
    );
    path
}

/// Canonical absolute string form, matching `PlaylistState`'s weakly
/// canonical storage for existing files.
pub fn canon(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| panic!("T10c: cannot canonicalize {}", path.display()))
        .to_string_lossy()
        .into_owned()
}
