//! T14c integration: desktop file, metainfo, icons, CLI compat, single-instance.
//!
//! Headless contract pins for the desktop-integration slice (packaging.md
//! §1.5/P8, §1.6/P11, §2.4): the desktop file is the C++-shipped file plus
//! exactly `DBusActivatable=true`; the metainfo carries the toolkit + rhythm
//! rewording; every symbolic icon the UI looks up has a vendored hicolor
//! fallback; CLI help/version/messages stay byte-compatible with `main.cpp`;
//! and the single-instance bus name matches the desktop basename (the
//! `DBusActivatable` requirement) with `Flags` forwarding files as the
//! `open` action (the `flatpak run` sending half — the live double-run is a
//! Phase-3 manual recipe in packaging.md §3.2).
//!
//! All reads run against the repo root (`cargo test` CWD, same assumption as
//! `common::fixture`); no display, no bus, no sandbox needed.

use std::path::{Path, PathBuf};

/// Read a repo-root-relative file, failing fast with context.
fn repo_file(rel: &str) -> String {
    std::fs::read_to_string(rel).unwrap_or_else(|why| {
        panic!("T14c: cannot read {rel} ({why}); run cargo test from the repo root")
    })
}

/// Every line must appear verbatim in `text`.
fn assert_lines(text: &str, rel: &str, lines: &[&str]) {
    for line in lines {
        assert!(
            text.lines().any(|l| l == *line),
            "T14c: {rel} lost line: {line:?}"
        );
    }
}

/// The desktop file is the C++-shipped content (installed by `CMakeLists.txt`
/// since #4/#6) plus exactly one migration line: `DBusActivatable=true`
/// (P8). Every other field stays pinned for MIME/launcher/WM compat.
#[test]
fn desktop_file_is_cpp_shipped_plus_dbus_activatable() {
    let rel = "com.goshapps.GoshMIDI.desktop";
    let text = repo_file(rel);
    assert_lines(
        &text,
        rel,
        &[
            "[Desktop Entry]",
            "Name=Gosh MIDI Player",
            "GenericName=MIDI File Player",
            "Exec=dmidiplayer %F",
            "Icon=com.goshapps.GoshMIDI",
            "Terminal=false",
            "Type=Application",
            "Categories=AudioVideo;Audio;Midi;",
            "MimeType=audio/midi;audio/x-midi;audio/cakewalk;application/x-riff;",
            "Comment=MIDI and Karaoke player based on dmidiplayer (Drumstick MIDI File Player)",
            "StartupNotify=true",
            "StartupWMClass=com.goshapps.GoshMIDI",
            "DBusActivatable=true",
            "Comment[es]=Reproductor de archivos MIDI y Karaoke basado en dmidiplayer",
            "Comment[ru]=MIDI и караоке-проигрыватель на основе dmidiplayer",
        ],
    );
    // Exactly one migration delta: 15 lines, nothing removed, nothing extra.
    assert_eq!(text.lines().count(), 15, "T14c: {rel} line count drifted");
}

/// Metainfo P11 scope minus `<release>` (T15's): toolkit wording +
/// rhythm-claim fix. The old "Rhythm views" claim and the GTK4 wording must
/// be gone (ux.md §3.5: rhythm is dashboard lamps, DECISIONS D5).
#[test]
fn metainfo_toolkit_and_rhythm_wording() {
    let rel = "com.goshapps.GoshMIDI.metainfo.xml";
    let text = repo_file(rel);
    assert!(
        text.contains("libcosmic/Rust MIDI file player"),
        "T14c: {rel} toolkit wording missing"
    );
    assert!(
        text.contains(
            "Piano, MIDI Channels, and Lyrics (Karaoke) views, plus dashboard rhythm lamps"
        ),
        "T14c: {rel} rhythm-claim fix missing"
    );
    for stale in [
        "Rhythm, and Lyrics",
        "GTK4/libadwaita",
        "rhythm view",
        "Rhythm views",
    ] {
        assert!(
            !text.contains(stale),
            "T14c: {rel} still contains pre-T14c wording: {stale:?}"
        );
    }
    // Identity anchors untouched.
    for anchor in [
        "<id>com.goshapps.GoshMIDI</id>",
        "<launchable type=\"desktop-id\">com.goshapps.GoshMIDI.desktop</launchable>",
        "<project_license>GPL-3.0-or-later</project_license>",
    ] {
        assert!(text.contains(anchor), "T14c: {rel} lost anchor: {anchor:?}");
    }
}

/// README carried the same rhythm-views doc bug (ux.md §3.5) — fixed with
/// the same lamps/meter-follow verbiage as the metainfo.
#[test]
fn readme_rhythm_claim_matches_metainfo_fix() {
    let text = repo_file("README.md");
    assert!(
        !text.contains("rhythm views"),
        "T14c: README.md still claims rhythm views"
    );
    assert!(
        text.contains("rhythm lamps that follow the meter"),
        "T14c: README.md lacks the lamps/meter-follow verbiage"
    );
}

/// Collect every `*-symbolic` string literal under `src/` (`.rs` only, same
/// extraction as `scripts/verify.sh`).
fn symbolic_literals() -> Vec<String> {
    fn visit(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("T14c: read src dir") {
            let path = entry.expect("T14c: read src entry").path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).expect("T14c: read src file");
                let bytes = text.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] == b'"' {
                        let start = i + 1;
                        let mut j = start;
                        while j < bytes.len() && bytes[j] != b'"' {
                            j += 1;
                        }
                        let lit = &text[start..j.min(text.len())];
                        if lit.ends_with("-symbolic")
                            && lit
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b == b'-' || b.is_ascii_digit())
                        {
                            out.push(lit.to_string());
                        }
                        i = j + 1;
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    visit(Path::new("src"), &mut out);
    out.sort();
    out.dedup();
    out
}

/// Every symbolic name the UI looks up must have a vendored hicolor fallback
/// (`icons/fallback/SOURCES.md`); the manifest installs the set. Without
/// this, toolbar buttons render empty on hosts whose theme is absent from
/// the lookup chain (GNOME/KDE — packaging.md §1.6).
#[test]
fn symbolic_inventory_has_hicolor_fallback() {
    let names = symbolic_literals();
    assert!(
        !names.is_empty(),
        "T14c: extractor found no symbolic names (broken test?)"
    );
    let mut missing = Vec::new();
    for name in &names {
        let actions = PathBuf::from("icons/fallback/actions").join(format!("{name}.svg"));
        let status = PathBuf::from("icons/fallback/status").join(format!("{name}.svg"));
        if !actions.is_file() && !status.is_file() {
            missing.push(name.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "T14c: no hicolor fallback for: {missing:?}"
    );
    // The manifest installs both fallback dirs (string pin, no YAML dep).
    let yml = repo_file("flatpak/com.goshapps.GoshMIDI.yml");
    for anchor in ["icons/fallback/actions/", "icons/fallback/status/"] {
        assert!(
            yml.contains(anchor),
            "T14c: manifest does not install {anchor}"
        );
    }
}

/// Tripwire over the vendored SVGs: each is an `<svg …>…</svg>` document
/// fragment with an XML namespace (byte identity is pinned by the SHA256
/// list in `icons/fallback/SOURCES.md`).
#[test]
fn fallback_svgs_are_well_formed_fragments() {
    let mut count = 0;
    for ctx in ["actions", "status"] {
        let dir = PathBuf::from("icons/fallback").join(ctx);
        for entry in std::fs::read_dir(&dir).expect("T14c: read fallback dir") {
            let path = entry.expect("T14c: read fallback entry").path();
            let text = std::fs::read_to_string(&path).expect("T14c: read fallback svg");
            let trimmed = text.trim();
            assert!(
                trimmed.starts_with("<svg") && trimmed.ends_with("</svg>"),
                "T14c: {} is not an <svg> fragment",
                path.display()
            );
            assert!(
                text.contains("xmlns="),
                "T14c: {} lacks an XML namespace",
                path.display()
            );
            count += 1;
        }
    }
    assert!(count >= 29, "T14c: only {count} fallback SVGs (want >= 29)");
}

/// `--help` stays byte-compatible with `src/main.cpp` (plus the additive
/// `--dry-run` line, which C++ never had). Exact-line pins, not substrings.
#[test]
fn cli_help_matches_cpp_strings() {
    let bin = env!("CARGO_BIN_EXE_dmidiplayer");
    let out = std::process::Command::new(bin)
        .arg("--help")
        .output()
        .expect("T14c: run dmidiplayer --help");
    assert!(out.status.success(), "T14c: --help exited {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let version = env!("CARGO_PKG_VERSION");
    let mut expected = vec![
        format!("Gosh MIDI Player (dmidiplayer) {version}"),
        "Based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.".to_string(),
        "Usage: dmidiplayer [options] [midi_files]".to_string(),
        "  -h, --help                 Show help".to_string(),
        "  -v, --version              Show version".to_string(),
        "  -p, --portable             Portable settings mode".to_string(),
        "  -f, --file FILE            Portable settings file".to_string(),
        "  -d, --driver DRIVER        MIDI output backend (ALSA, FluidSynth, Dummy)".to_string(),
        "  -c, --connection PORT      MIDI output connection".to_string(),
    ];
    for line in &expected {
        assert!(
            stdout.contains(line.as_str()),
            "T14c: --help lost line: {line:?}"
        );
    }
    expected.push(
        "  --dry-run, --dump-info     Validate inputs and exit without starting the GUI"
            .to_string(),
    );
    assert!(
        stdout.contains(expected.last().unwrap().as_str()),
        "T14c: --help lost the additive --dry-run line"
    );
}

/// `-v` prints the same two lines as C++ (`main.cpp`, no usage trailer).
#[test]
fn cli_version_matches_cpp_strings() {
    let bin = env!("CARGO_BIN_EXE_dmidiplayer");
    let out = std::process::Command::new(bin)
        .arg("-v")
        .output()
        .expect("T14c: run dmidiplayer -v");
    assert!(out.status.success(), "T14c: -v exited {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        stdout.contains(&format!("Gosh MIDI Player (dmidiplayer) {version}")),
        "T14c: -v lost the name/version line: {stdout:?}"
    );
    assert!(
        stdout.contains("Based on dmidiplayer (Drumstick MIDI File Player) by Pedro"),
        "T14c: -v lost the attribution line: {stdout:?}"
    );
    assert!(
        !stdout.contains("Usage:"),
        "T14c: -v must not print usage (C++ parity): {stdout:?}"
    );
}

/// Binary name compat (P3/DECISIONS D3): manifest `command`, desktop `Exec`,
/// and the built binary are all `dmidiplayer`.
#[test]
fn binary_name_is_dmidiplayer_end_to_end() {
    let bin = env!("CARGO_BIN_EXE_dmidiplayer");
    assert_eq!(
        Path::new(bin).file_name().and_then(|n| n.to_str()),
        Some("dmidiplayer"),
        "T14c: built binary is not named dmidiplayer: {bin}"
    );
    let yml = repo_file("flatpak/com.goshapps.GoshMIDI.yml");
    assert!(
        yml.lines().any(|l| l.trim() == "command: dmidiplayer"),
        "T14c: manifest command is not dmidiplayer"
    );
    let desktop = repo_file("com.goshapps.GoshMIDI.desktop");
    assert!(
        desktop.contains("Exec=dmidiplayer %F"),
        "T14c: desktop Exec is not dmidiplayer"
    );
}

/// Manifest display/clipboard prerequisites: Wayland + X11 sockets for the
/// compositor-mediated clipboard (no portal, no extra permission needed),
/// and no host-filesystem escape hatch (portals instead, §1.5).
#[test]
fn manifest_keeps_display_sockets_without_host_fs() {
    let yml = repo_file("flatpak/com.goshapps.GoshMIDI.yml");
    for socket in ["--socket=wayland", "--socket=fallback-x11"] {
        assert!(
            yml.contains(socket),
            "T14c: manifest lost finish-arg {socket} (clipboard/display)"
        );
    }
    assert!(
        !yml.lines()
            .map(str::trim)
            .any(|l| l.starts_with("- --filesystem=")),
        "T14c: manifest gained a --filesystem escape hatch"
    );
}

/// The single-instance bus name is the app ID, which equals the desktop
/// basename — the `DBusActivatable=true` requirement (P8). `run_single_instance`
/// claims `APP_ID` (libcosmic `dbus_activation.rs`) and proxies second
/// launches there; a mismatch would break activation + the `flatpak run`
/// forward. (Live double-run incl. `--file-forwarding` is a Phase-3 manual
/// step, packaging.md §3.2.)
#[test]
fn single_instance_bus_name_matches_desktop_basename() {
    let app_id = <dmidiplayer::app::App as cosmic::Application>::APP_ID;
    assert_eq!(app_id, "com.goshapps.GoshMIDI");
    // Desktop basename (minus `.desktop`) must equal the claimed bus name.
    let desktop_path = Path::new("com.goshapps.GoshMIDI.desktop");
    assert_eq!(
        desktop_path.file_stem().and_then(|s| s.to_str()),
        Some(app_id)
    );
    let desktop = repo_file("com.goshapps.GoshMIDI.desktop");
    assert!(desktop.contains("DBusActivatable=true"));
}

/// Sending half of the `flatpak run` forward (A4): CLI files become the
/// `open` action + argv strings, delivered to the running instance over the
/// session bus; a bare second launch is present-only (`action() == None`).
/// (Receiving half: `dbus_activation` + replace semantics, pinned by
/// `playlist_flows::second_open_forwards_replace_into_running_state` and the
/// `app.rs` unit tests.)
#[test]
fn single_instance_flags_forward_files_as_open_action() {
    use cosmic::app::CosmicFlags as _;
    let flags = dmidiplayer::app::Flags::new(
        vec![PathBuf::from("/m/a.mid"), PathBuf::from("/m/b.mid")],
        None,
        None,
        false,
        None,
    );
    assert_eq!(flags.action(), Some(&"open"));
    assert_eq!(flags.args(), vec!["/m/a.mid", "/m/b.mid"]);
    let idle = dmidiplayer::app::Flags::new(Vec::new(), None, None, false, None);
    assert_eq!(idle.action(), None);
    assert!(idle.args().is_empty());
}
