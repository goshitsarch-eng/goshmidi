//! `dmidiplayer` entry point: CLI parsing, portable setup, app boot.
//!
//! CLI parity with `src/main.cpp`: `-h/-v`, `-p/-f`, `-d/-c` (both-or-neither
//! per arch §1.6), existing-files-only positionals. Plus headless
//! `--dry-run`/`--dump-info`, which parse + validate inputs and exit 0
//! *before* any GUI init (P10c: must work with no display).

mod app;
mod config;
pub(crate) mod core;
mod message;
mod state;
mod ui;

use std::path::PathBuf;

const APP_NAME: &str = "Gosh MIDI Player (dmidiplayer)";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const ATTRIBUTION: &str =
    "Based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.";

/// Parsed command line (mirrors the `main.cpp` loop, in order).
#[derive(Debug, Default)]
struct Cli {
    portable: bool,
    portable_file: Option<String>,
    backend: Option<String>,
    conn: Option<String>,
    /// Existing files, canonicalized, in argv order.
    files: Vec<PathBuf>,
    /// Positional args naming nothing on disk (reported, then skipped).
    missing: Vec<String>,
    dry_run: bool,
}

/// Early exit requested by `-h`/`-v`: stops parsing where it appears,
/// exactly like the C++ loop's immediate `return 0`.
#[derive(Debug, PartialEq, Eq)]
enum EarlyExit {
    Help,
    Version,
}

fn parse_args(args: &[String]) -> (Cli, Option<EarlyExit>) {
    let mut cli = Cli::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "-h" || a == "--help" {
            return (cli, Some(EarlyExit::Help));
        }
        if a == "-v" || a == "--version" {
            return (cli, Some(EarlyExit::Version));
        }
        if a == "-p" || a == "--portable" {
            cli.portable = true;
        } else if (a == "-f" || a == "--file") && i + 1 < args.len() {
            i += 1;
            cli.portable = true;
            cli.portable_file = Some(args[i].clone());
        } else if (a == "-d" || a == "--driver") && i + 1 < args.len() {
            i += 1;
            cli.backend = Some(args[i].clone());
        } else if (a == "-c" || a == "--connection") && i + 1 < args.len() {
            i += 1;
            cli.conn = Some(args[i].clone());
        } else if a == "--dry-run" || a == "--dump-info" {
            cli.dry_run = true;
        } else if !a.is_empty() && !a.starts_with('-') {
            let path = PathBuf::from(a);
            if path.exists() {
                cli.files.push(std::fs::canonicalize(&path).unwrap_or(path));
            } else {
                cli.missing.push(a.clone());
            }
        }
        // Unknown `-` options are silently ignored, as in the C++ loop
        // (including a trailing `-f`/`-d`/`-c` with no value).
        i += 1;
    }
    (cli, None)
}

/// `-d`/`-c` are both-or-neither (arch §1.6): applied only when both are
/// present and non-empty; otherwise ignored.
fn midi_override(cli: &Cli) -> (Option<String>, Option<String>) {
    match (&cli.backend, &cli.conn) {
        (Some(b), Some(c)) if !b.is_empty() && !c.is_empty() => (Some(b.clone()), Some(c.clone())),
        _ => (None, None),
    }
}

fn print_help() {
    println!("{APP_NAME} {VERSION}");
    println!("{ATTRIBUTION}");
    println!("Usage: dmidiplayer [options] [midi_files]");
    println!("  -h, --help                 Show help");
    println!("  -v, --version              Show version");
    println!("  -p, --portable             Portable settings mode");
    println!("  -f, --file FILE            Portable settings file");
    println!("  -d, --driver DRIVER        MIDI output backend (ALSA, FluidSynth, Dummy)");
    println!("  -c, --connection PORT      MIDI output connection");
    println!("  --dry-run, --dump-info     Validate inputs and exit without starting the GUI");
}

fn print_version() {
    println!("{APP_NAME} {VERSION}");
    println!("{ATTRIBUTION}");
}

/// T1 dry-run marker: parse + validate inputs, print markers, exit 0.
/// T10+ extends this with playlist/config resolution; the GUI is never
/// initialized on this path (P10c headless requirement).
fn print_dry_run(cli: &Cli, backend: &Option<String>, conn: &Option<String>) {
    println!(
        "dry-run: files={} backend={} connection={} portable={}",
        cli.files.len(),
        backend.as_deref().unwrap_or("-"),
        conn.as_deref().unwrap_or("-"),
        if cli.portable { "yes" } else { "no" },
    );
    for f in &cli.files {
        println!("dry-run: file {}", f.display());
    }
    println!("dry-run: OK (GUI skipped)");
}

fn main() -> cosmic::iced::Result {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (cli, early) = parse_args(&argv);

    // Missing positionals are reported in argv order (C++ parity), including
    // on the early-exit and dry-run paths.
    for m in &cli.missing {
        eprintln!("File not found: {m}");
    }

    if let Some(exit) = early {
        match exit {
            EarlyExit::Help => print_help(),
            EarlyExit::Version => print_version(),
        }
        return Ok(());
    }

    let (backend, conn) = midi_override(&cli);

    if cli.dry_run {
        print_dry_run(&cli, &backend, &conn);
        return Ok(());
    }

    // Portable mode (`AppSettings::setPortable`, `main.cpp:102-103`):
    // threaded through `Flags` into `init`, which resolves `Paths` from it.
    let settings = cosmic::app::Settings::default().size(cosmic::iced::Size::new(1100.0, 720.0));
    let flags = app::Flags::new(
        cli.files,
        backend,
        conn,
        cli.portable,
        cli.portable_file.map(PathBuf::from),
    );
    cosmic::app::run_single_instance::<app::App>(settings, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn help_and_version_exit_early() {
        for flag in ["-h", "--help"] {
            let (_, early) = parse_args(&argv(&[flag, "unparsed.mid"]));
            assert_eq!(early, Some(EarlyExit::Help));
        }
        for flag in ["-v", "--version"] {
            let (_, early) = parse_args(&argv(&[flag]));
            assert_eq!(early, Some(EarlyExit::Version));
        }
        let (_, early) = parse_args(&argv(&[]));
        assert_eq!(early, None);
    }

    #[test]
    fn help_wins_over_dry_run() {
        let (cli, early) = parse_args(&argv(&["--dry-run", "-h"]));
        assert!(cli.dry_run);
        assert_eq!(early, Some(EarlyExit::Help));
    }

    #[test]
    fn args_after_early_exit_are_unparsed() {
        // C++ parity: `dmidiplayer -h missing.mid` reports no missing file.
        let (cli, early) = parse_args(&argv(&["-h", "missing.mid"]));
        assert_eq!(early, Some(EarlyExit::Help));
        assert!(cli.missing.is_empty());
    }

    #[test]
    fn portable_and_settings_file() {
        let (cli, _) = parse_args(&argv(&["-p"]));
        assert!(cli.portable);
        assert_eq!(cli.portable_file, None);

        let (cli, _) = parse_args(&argv(&["--file", "alt.conf"]));
        assert!(cli.portable);
        assert_eq!(cli.portable_file, Some("alt.conf".to_owned()));

        // Trailing `-f` with no value is silently ignored (C++ parity).
        let (cli, _) = parse_args(&argv(&["-f"]));
        assert!(!cli.portable);
    }

    #[test]
    fn portable_cli_threads_into_app_flags() {
        // `-f FILE` implies portable mode and carries the file into `Flags`
        // (the `setPortable` half of `main.cpp:102-103`).
        let (cli, _) = parse_args(&argv(&["-f", "alt.conf"]));
        let flags = app::Flags::new(
            vec![],
            None,
            None,
            cli.portable,
            cli.portable_file.map(PathBuf::from),
        );
        assert!(flags.portable);
        assert_eq!(flags.portable_file, Some(PathBuf::from("alt.conf")));

        // Bare `-p`: portable with no file (resolves to `./dmidiplayer.conf`).
        let (cli, _) = parse_args(&argv(&["-p"]));
        let flags = app::Flags::new(
            vec![],
            None,
            None,
            cli.portable,
            cli.portable_file.map(PathBuf::from),
        );
        assert!(flags.portable);
        assert_eq!(flags.portable_file, None);

        // Default: not portable.
        let (cli, _) = parse_args(&argv(&[]));
        let flags = app::Flags::new(vec![], None, None, cli.portable, None);
        assert!(!flags.portable);
        assert_eq!(flags.portable_file, None);
    }

    #[test]
    fn driver_connection_are_both_or_neither() {
        let (cli, _) = parse_args(&argv(&["-d", "ALSA", "-c", "14:0"]));
        assert_eq!(
            midi_override(&cli),
            (Some("ALSA".into()), Some("14:0".into()))
        );

        for args in [&["-d", "ALSA"][..], &["-c", "14:0"][..], &[][..]] {
            let (cli, _) = parse_args(&argv(args));
            assert_eq!(midi_override(&cli), (None, None), "args: {args:?}");
        }
    }

    #[test]
    fn positionals_keep_existing_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song.mid");
        std::fs::write(&song, b"MThd").unwrap();

        let (cli, _) = parse_args(&argv(&[
            song.to_str().unwrap(),
            "definitely-not-here-12345.mid",
        ]));
        assert_eq!(cli.files, vec![std::fs::canonicalize(&song).unwrap()]);
        assert_eq!(
            cli.missing,
            vec!["definitely-not-here-12345.mid".to_owned()]
        );
    }

    #[test]
    fn empty_positional_is_ignored() {
        // C++ parity: main.cpp guards `!a.empty()`, so an empty argv element
        // is neither a file nor a missing entry (no `File not found: `).
        let (cli, early) = parse_args(&argv(&[""]));
        assert_eq!(early, None);
        assert!(cli.files.is_empty() && cli.missing.is_empty());
    }

    #[test]
    fn unknown_dash_options_are_ignored() {
        let (cli, early) = parse_args(&argv(&["--bogus", "-z"]));
        assert_eq!(early, None);
        assert!(!cli.dry_run);
        assert!(cli.files.is_empty() && cli.missing.is_empty());
    }

    #[test]
    fn dry_run_flags() {
        for flag in ["--dry-run", "--dump-info"] {
            let (cli, early) = parse_args(&argv(&[flag]));
            assert!(cli.dry_run);
            assert_eq!(early, None);
        }
    }
}
