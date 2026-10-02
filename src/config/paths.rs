//! Native config/data paths and compatibility with the original key-file data.
//!
//! Current path contract:
//!
//! | Item | Path |
//! |---|---|
//! | Global config | OS config directory + `dmidiplayer/dmidiplayer.conf` |
//! | Portable config | `./dmidiplayer.conf` (`-p`) or `-f FILE` (parent = config dir) |
//! | Data dir | OS local-data directory + `dmidiplayer` |
//! | Song settings | Full-path hash under the data directory |
//! | Legacy song settings | Read-only `~/.dmidiplayer/<song-file-name>.cfg` fallback |
//! | Last playlist | path stored verbatim in config (`lastPlayList`) |
//! | Examples fallback | DATADIR, `$XDG_DATA_HOME`, `/usr/share`, `./examples/` |
//!
//! Environment access is injectable via [`Env`] so every resolution rule is
//! unit-testable without touching the real home directory. There are no
//! process-global singletons here (the C++ `g_portable`/`g_portableFile`
//! globals become constructor arguments).
//!
//! Deviation from C++ (deliberate): `defaultConfigPath()`/`dataDir()` create
//! directories as a side effect of *every* call, even on read paths. Here
//! every getter is pure; directory creation happens only through
//! [`Paths::ensure_config_dir`] / [`Paths::ensure_data_dir`], which the save
//! paths call. Permission and other IO failures are returned to the caller.

use std::path::{Path, PathBuf};

/// Injected process environment snapshot used for path resolution.
///
/// Production code uses [`Env::from_os`]; tests build fixed instances with
/// [`Env::for_test`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    home: PathBuf,
    config_home: PathBuf,
    data_home: PathBuf,
    cwd: PathBuf,
}

impl Env {
    /// Read the real process environment (`HOME`, `XDG_CONFIG_HOME`,
    /// `XDG_DATA_HOME`, current dir) with the same fallbacks `g_get_home_dir` /
    /// `g_get_user_config_dir` / `g_get_user_data_dir` apply (empty XDG values
    /// count as unset, per the XDG base-directory spec).
    pub fn from_os() -> Self {
        let native = crate::platform::native_directories();
        let home = native
            .as_ref()
            .map(|dirs| dirs.0.clone())
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| {
                // Degenerate (no passwd lookup without libc): keep resolving under
                // `/` rather than failing; tests always inject a real home.
                PathBuf::from("/")
            });
        let config_home = native
            .as_ref()
            .map(|dirs| dirs.1.clone())
            .unwrap_or_else(|| xdg_or(&home, "XDG_CONFIG_HOME", ".config"));
        let data_home = native
            .as_ref()
            .map(|dirs| dirs.2.clone())
            .unwrap_or_else(|| xdg_or(&home, "XDG_DATA_HOME", ".local/share"));
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            home,
            config_home,
            data_home,
            cwd,
        }
    }

    /// Fixed environment for tests.
    pub fn for_test(home: &Path, config_home: &Path, data_home: &Path, cwd: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            config_home: config_home.to_path_buf(),
            data_home: data_home.to_path_buf(),
            cwd: cwd.to_path_buf(),
        }
    }
}

fn xdg_or(home: &Path, var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home.join(fallback),
    }
}

/// Resolved config/data paths for one process lifetime.
///
/// Construct with [`Paths::from_env`] (normal mode) or
/// [`Paths::portable_from_env`] (`-p` / `-f FILE`), or with [`Paths::with_env`]
/// for an injected [`Env`].
#[derive(Debug, Clone)]
pub struct Paths {
    config_file: PathBuf,
    portable: bool,
    env: Env,
}

impl Paths {
    /// Normal (non-portable) paths from the real process environment.
    pub fn from_env() -> Self {
        Self::with_env(Env::from_os(), None)
    }

    /// Portable-mode paths from the real process environment.
    ///
    /// `file` is the `-f FILE` argument when given (`None` = bare `-p`, which
    /// resolves to `./dmidiplayer.conf`, i.e. `$CWD/dmidiplayer.conf`).
    /// A relative `-f` value is kept relative (resolved against the CWD at
    /// use), exactly like the C++ `return g_portableFile`.
    pub fn portable_from_env(file: Option<PathBuf>) -> Self {
        Self::with_env(Env::from_os(), Some(file))
    }

    /// Paths for an injected environment. `portable_file` mirrors
    /// `main.cpp`: `None` = normal mode, `Some(None)` = bare `-p`,
    /// `Some(Some(f))` = `-f f`.
    pub fn with_env(env: Env, portable_file: Option<Option<PathBuf>>) -> Self {
        match portable_file {
            None => {
                let config_file = env.config_home.join("dmidiplayer").join("dmidiplayer.conf");
                Self {
                    config_file,
                    portable: false,
                    env,
                }
            }
            Some(file) => {
                let config_file = file.unwrap_or_else(|| PathBuf::from("dmidiplayer.conf"));
                Self {
                    config_file,
                    portable: true,
                    env,
                }
            }
        }
    }

    /// Whether portable mode (`-p` / `-f`) is active.
    pub fn portable(&self) -> bool {
        self.portable
    }

    /// Full path of the global config file (`dmidiplayer.conf`).
    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    /// Config directory: the config file's parent in portable mode
    /// (`""` for a bare `-f FILE` name, mirroring C++ `parent_path`), else
    /// `$XDG_CONFIG_HOME/dmidiplayer`.
    pub fn config_dir(&self) -> PathBuf {
        if self.portable {
            self.config_file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        } else {
            self.env.config_home.join("dmidiplayer")
        }
    }

    /// Create the config directory (and parents) if missing. Called by save
    /// paths; read paths never create directories (see module docs).
    pub fn ensure_config_dir(&self) -> std::io::Result<()> {
        let dir = self.config_dir();
        if dir.as_os_str().is_empty() || dir == Path::new(".") {
            return Ok(());
        }
        std::fs::create_dir_all(dir)
    }

    /// OS local-data directory plus `dmidiplayer` (identical in portable mode).
    pub fn data_dir(&self) -> PathBuf {
        self.env.data_home.join("dmidiplayer")
    }

    /// Create the data directory (and parents) if missing. Called by save
    /// paths (song settings, and anything writing under the data dir).
    pub fn ensure_data_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.data_dir())
    }

    /// Basename settings helper under the current data directory.
    ///
    /// `song_name` is the song's file name (no separators; callers pass the
    /// equivalent of C++ `currentFile()`). Separators are replaced so the result
    /// stays under the data directory. New saves use `song_settings_for_file`.
    pub fn song_settings_path(&self, song_name: &str) -> PathBuf {
        let safe = song_name.replace(['/', '\\'], "_");
        self.data_dir().join(format!("{safe}.cfg"))
    }

    pub fn legacy_song_settings_path(&self, song_name: &str) -> PathBuf {
        let safe = song_name.rsplit(['/', '\\']).next().unwrap_or("song");
        self.env
            .home
            .join(".dmidiplayer")
            .join(format!("{safe}.cfg"))
    }

    /// A stable path hash prevents equal basenames in different folders from
    /// overwriting each other's settings. Legacy files remain read-only fallbacks.
    pub fn song_settings_for_file(&self, path: &Path) -> PathBuf {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .replace(['/', '\\'], "_");
        let hash = path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        self.data_dir().join(format!("{name}-{hash:016x}.cfg"))
    }

    /// Ordered `examples.lst` fallback candidates (`window.cpp:247-257`):
    /// `$DATADIR/dmidiplayer/examples.lst` (when `datadir` is `Some`, i.e. the
    /// C++ `#ifdef DATADIR` entry), `$XDG_DATA_HOME/dmidiplayer/examples.lst`,
    /// `/usr/share/dmidiplayer/examples.lst`, `./examples/examples.lst`.
    ///
    /// Startup (T9) loads the first candidate that exists *and* parses;
    /// `lastPlayList` is tried before this chain, and [`Paths::examples_dir`]
    /// is scanned after it (`window.cpp:259-269`).
    pub fn examples_playlist_candidates(&self, datadir: Option<&str>) -> Vec<PathBuf> {
        let mut out = Vec::with_capacity(4);
        if let Some(d) = datadir {
            out.push(Path::new(d).join("dmidiplayer").join("examples.lst"));
        }
        out.push(self.env.data_home.join("dmidiplayer").join("examples.lst"));
        out.push(Path::new("/usr/share/dmidiplayer/examples.lst").to_path_buf());
        out.push(self.env.cwd.join("examples").join("examples.lst"));
        out
    }

    /// `./examples/` directory, scanned for supported songs when no playlist
    /// source yields anything (`window.cpp:260-269`).
    pub fn examples_dir(&self) -> PathBuf {
        self.env.cwd.join("examples")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env::for_test(
            Path::new("/home/u"),
            Path::new("/home/u/.config"),
            Path::new("/home/u/.local/share"),
            Path::new("/work"),
        )
    }

    #[test]
    fn normal_mode_resolution() {
        let p = Paths::with_env(env(), None);
        assert!(!p.portable());
        assert_eq!(
            p.config_file(),
            Path::new("/home/u/.config/dmidiplayer/dmidiplayer.conf")
        );
        assert_eq!(p.config_dir(), Path::new("/home/u/.config/dmidiplayer"));
        assert_eq!(p.data_dir(), Path::new("/home/u/.local/share/dmidiplayer"));
        assert_eq!(
            p.song_settings_path("song.mid"),
            Path::new("/home/u/.local/share/dmidiplayer/song.mid.cfg")
        );
    }

    #[test]
    fn bare_portable_uses_cwd_conf() {
        let p = Paths::with_env(env(), Some(None));
        assert!(p.portable());
        assert_eq!(p.config_file(), Path::new("dmidiplayer.conf"));
        // Parent of a bare file name is "" (mirrors C++ parent_path("")).
        assert_eq!(p.config_dir(), Path::new(""));
        // Data dir is unchanged by portable mode.
        assert_eq!(p.data_dir(), Path::new("/home/u/.local/share/dmidiplayer"));
    }

    #[test]
    fn file_portable_absolute_and_relative() {
        let abs = Paths::with_env(env(), Some(Some(PathBuf::from("/mnt/u/app.conf"))));
        assert!(abs.portable());
        assert_eq!(abs.config_file(), Path::new("/mnt/u/app.conf"));
        assert_eq!(abs.config_dir(), Path::new("/mnt/u"));

        // Relative -f stays relative (resolved against CWD at use).
        let rel = Paths::with_env(env(), Some(Some(PathBuf::from("sub/app.conf"))));
        assert_eq!(rel.config_file(), Path::new("sub/app.conf"));
        assert_eq!(rel.config_dir(), Path::new("sub"));
    }

    #[test]
    fn ensure_dirs_create_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let env = Env::for_test(&home, &home.join("cfg"), &home.join("share"), tmp.path());
        let p = Paths::with_env(env, None);
        assert!(!p.config_dir().exists());
        p.ensure_config_dir().unwrap();
        assert!(p.config_dir().is_dir());
        // Idempotent.
        p.ensure_config_dir().unwrap();
        assert!(!p.data_dir().exists());
        p.ensure_data_dir().unwrap();
        assert!(p.data_dir().is_dir());

        // Portable nested -f parent is created too.
        let env2 = Env::for_test(&home, &home.join("cfg"), &home.join("share"), tmp.path());
        let pp = Paths::with_env(env2, Some(Some(tmp.path().join("deep/nest/port.conf"))));
        pp.ensure_config_dir().unwrap();
        assert!(tmp.path().join("deep/nest").is_dir());
    }

    #[test]
    fn examples_fallback_chain_order() {
        let p = Paths::with_env(env(), None);
        assert_eq!(
            p.examples_playlist_candidates(Some("/app/share")),
            vec![
                PathBuf::from("/app/share/dmidiplayer/examples.lst"),
                PathBuf::from("/home/u/.local/share/dmidiplayer/examples.lst"),
                PathBuf::from("/usr/share/dmidiplayer/examples.lst"),
                PathBuf::from("/work/examples/examples.lst"),
            ]
        );
        // No DATADIR (C++ `#ifdef DATADIR` off): chain starts at XDG data home.
        assert_eq!(
            p.examples_playlist_candidates(None),
            vec![
                PathBuf::from("/home/u/.local/share/dmidiplayer/examples.lst"),
                PathBuf::from("/usr/share/dmidiplayer/examples.lst"),
                PathBuf::from("/work/examples/examples.lst"),
            ]
        );
        assert_eq!(p.examples_dir(), Path::new("/work/examples"));
    }

    #[test]
    fn from_env_smoke() {
        // Real environment: only the well-known suffixes are asserted.
        let p = Paths::from_env();
        assert!(p.config_file().ends_with("dmidiplayer/dmidiplayer.conf"));
        assert!(p.data_dir().ends_with("dmidiplayer"));
        let pp = Paths::portable_from_env(None);
        assert!(pp.portable());
        assert_eq!(pp.config_file(), Path::new("dmidiplayer.conf"));
    }
}
