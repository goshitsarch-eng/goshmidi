//! CLI entry point. Parsing and file validation work without a desktop session.
use dmidiplayer::{config::paths::Paths, core::sequence::Sequence};
use std::{ffi::OsString, path::PathBuf};

#[derive(Default, Debug)]
struct Cli {
    portable: bool,
    config: Option<PathBuf>,
    backend: Option<String>,
    port: Option<String>,
    files: Vec<PathBuf>,
    validate: bool,
    info: bool,
}
fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = arguments.into_iter();
    let mut positional = false;
    while let Some(argument) = args.next() {
        if positional {
            cli.files.push(argument.into());
            continue;
        }
        match argument.to_str() {
            Some("--") => positional = true,
            Some("-p" | "--portable") => cli.portable = true,
            Some("-f" | "--file") => {
                cli.portable = true;
                cli.config = Some(args.next().ok_or("--file requires a path")?.into());
            }
            Some("-d" | "--driver") => {
                cli.backend = Some(
                    args.next()
                        .ok_or("--driver requires a backend")?
                        .into_string()
                        .map_err(|_| "Backend must be Unicode")?,
                )
            }
            Some("-c" | "--connection") => {
                cli.port = Some(
                    args.next()
                        .ok_or("--connection requires a port")?
                        .into_string()
                        .map_err(|_| "Port must be Unicode")?,
                )
            }
            Some("--dry-run") => cli.validate = true,
            Some("--dump-info") => {
                cli.validate = true;
                cli.info = true;
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}"));
            }
            _ => cli.files.push(argument.into()),
        }
    }
    if cli.backend.is_some() != cli.port.is_some() {
        return Err("Use --driver and --connection together".into());
    }
    Ok(cli)
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Gosh MIDI Player {}\nBased on dmidiplayer by Pedro López-Cabanillas.\n\nUsage: dmidiplayer [OPTIONS] [MIDI_FILES]\n  -p, --portable          Settings in the working directory\n  -f, --file PATH         Explicit portable settings file\n  -d, --driver BACKEND    ALSA (Linux), MIDI (Windows/macOS), FluidSynth, Dummy\n  -c, --connection PORT   MIDI destination (use with --driver)\n  --dry-run              Decode inputs and report failures without a GUI\n  --dump-info            Decode and print JSON metadata without a GUI\n  -h, --help             Show help\n  -v, --version          Show version",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-v") {
        println!("Gosh MIDI Player {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let cli = parse(args)?;
    let mut files = Vec::new();
    for path in cli.files {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lst"))
        {
            let mut playlist = dmidiplayer::state::playlist::PlaylistState::new();
            if !playlist.load(&path) {
                return Err(format!("Could not read playlist: {}", path.display()));
            }
            files.extend_from_slice(playlist.items());
        } else {
            files.push(
                std::fs::canonicalize(&path)
                    .map_err(|e| format!("Could not open {}: {e}", path.display()))?,
            );
        }
    }
    if cli.validate {
        if files.is_empty() {
            return Err("Validation requires at least one MIDI file or playlist".into());
        }
        let mut failures = Vec::new();
        for path in files {
            let mut song = Sequence::default();
            if !song.load_file(&path) {
                failures.push(format!("{}: {}", path.display(), song.loading_errors()));
                continue;
            }
            if cli.info {
                println!(
                    "{}",
                    serde_json::json!({"path":path,"format":song.file_format(),"tracks":song.num_tracks(),"bars":song.last_bar(),"division":song.division(),"ticks":song.song_length_ticks(),"duration_ms":song.time_of_ticks(song.song_length_ticks().max(0) as u64).as_millis(),"events":song.events().len()})
                );
            } else {
                println!(
                    "OK: {} ({}, {} tracks)",
                    path.display(),
                    song.file_format(),
                    song.num_tracks()
                );
            }
        }
        return if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        };
    }
    let paths = if cli.portable {
        Paths::portable_from_env(cli.config)
    } else {
        Paths::from_env()
    };
    let output = cli.backend.zip(cli.port);
    #[cfg(feature = "desktop")]
    dmidiplayer::frontend::launch(paths, files, output);
    #[cfg(not(feature = "desktop"))]
    {
        let _ = (paths, files, output);
        Err(
            "This build has no desktop frontend; use --dry-run or build with the desktop feature"
                .into(),
        )
    }
}
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_options_are_errors() {
        assert!(parse(["--file".into()]).is_err());
        assert!(parse(["--unknown".into()]).is_err());
        assert!(parse(["--driver".into(), "Dummy".into()]).is_err());
    }
    #[test]
    fn paths_and_portable_file_are_preserved() {
        let cli = parse([
            "--file".into(),
            "配置.conf".into(),
            "--".into(),
            "-song.mid".into(),
        ])
        .unwrap();
        assert!(cli.portable);
        assert_eq!(cli.config, Some(PathBuf::from("配置.conf")));
        assert_eq!(cli.files, [PathBuf::from("-song.mid")]);
    }
}
