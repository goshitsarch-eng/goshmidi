//! SysEx reset messages, pacing, `.syx` parsing, companion lookup — port of
//! `src/midi/sysex.{hpp,cpp}`.
//!
//! PLAN T5, arch §1.2 (SysEx row) and §5 (`core/sysex.rs`). Byte sequences,
//! settle/pacing values, parse rules, and companion candidate order match the
//! C++ exactly. The only deliberate shape change is `companion_syx_path`
//! returning `Option<PathBuf>` instead of an empty string for "absent".
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use std::path::{Path, PathBuf};

/// No reset (C++ `kSysexResetNone`).
pub const SYSEX_RESET_NONE: i32 = 0;
/// GM reset (C++ `kSysexResetGm`).
pub const SYSEX_RESET_GM: i32 = 1;
/// GS / SC-55 reset (C++ `kSysexResetGs`).
pub const SYSEX_RESET_GS: i32 = 2;
/// XG reset (C++ `kSysexResetXg`).
pub const SYSEX_RESET_XG: i32 = 3;
/// MT-32 reset (C++ `kSysexResetMt32`).
pub const SYSEX_RESET_MT32: i32 = 4;

// Universal SysEx GM System On (sysex.cpp `kGmReset`).
const GM_RESET: &[u8] = &[0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7];
// Roland GS Reset (DT1 40 00 7F 00, sysex.cpp `kGsReset`).
const GS_RESET: &[u8] = &[
    0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x00, 0x41, 0xF7,
];
// Yamaha XG System On (sysex.cpp `kXgReset`).
const XG_RESET: &[u8] = &[0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7];
// Roland MT-32 / CM-32L DT1 all-parameters reset, addr 7F 00 00 data 01
// (sysex.cpp `kMt32Reset`).
const MT32_RESET: &[u8] = &[
    0xF0, 0x41, 0x10, 0x16, 0x12, 0x7F, 0x00, 0x00, 0x01, 0x00, 0xF7,
];

/// Display name for a reset kind; unknown kinds read as "None"
/// (C++ `sysexResetName`).
pub fn sysex_reset_name(kind: i32) -> &'static str {
    match kind {
        SYSEX_RESET_GM => "GM Reset",
        SYSEX_RESET_GS => "GS Reset (SC-55)",
        SYSEX_RESET_XG => "XG Reset",
        SYSEX_RESET_MT32 => "MT-32 Reset",
        _ => "None",
    }
}

/// Full reset message bytes for a kind; none/unknown kinds yield no message
/// (C++ `sysexResetMessage`).
pub fn sysex_reset_message(kind: i32) -> Vec<u8> {
    match kind {
        SYSEX_RESET_GM => GM_RESET.to_vec(),
        SYSEX_RESET_GS => GS_RESET.to_vec(),
        SYSEX_RESET_XG => XG_RESET.to_vec(),
        SYSEX_RESET_MT32 => MT32_RESET.to_vec(),
        _ => Vec::new(),
    }
}

/// Milliseconds to let the device settle after a reset
/// (C++ `sysexResetSettleMs`).
pub fn sysex_reset_settle_ms(kind: i32) -> u64 {
    match kind {
        SYSEX_RESET_MT32 => 80,
        SYSEX_RESET_GS | SYSEX_RESET_XG => 50,
        SYSEX_RESET_GM => 40,
        _ => 0,
    }
}

/// Inter-message pacing for a SysEx blob of the given size
/// (C++ `sysexPacingMs`).
pub fn sysex_pacing_ms(message_bytes: usize) -> u64 {
    if message_bytes > 256 {
        40
    } else if message_bytes > 32 {
        15
    } else {
        0
    }
}

/// Split a `.syx` blob (or concatenated MIDI SysEx) into `F0..F7` messages
/// (C++ `parseSysexMessages`): bytes before the first `F0` are skipped, a new
/// `F0` restarts (dropping the partial message), and an unterminated tail is
/// dropped. A bare `F0 F7` pair is kept (`size >= 2`).
pub fn parse_sysex_messages(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut in_msg = false;
    for &b in bytes {
        if b == 0xF0 {
            cur.clear();
            cur.push(b);
            in_msg = true;
            continue;
        }
        if !in_msg {
            continue;
        }
        cur.push(b);
        if b == 0xF7 {
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
            in_msg = false;
        }
    }
    out
}

/// Read a `.syx` file and split it into messages; unreadable or empty files
/// yield no messages (C++ `loadSysexFile`).
pub fn load_sysex_file(path: &Path) -> Vec<Vec<u8>> {
    match std::fs::read(path) {
        Ok(buf) if !buf.is_empty() => parse_sysex_messages(&buf),
        _ => Vec::new(),
    }
}

/// Companion SysEx for a MIDI file: same-basename `.syx` next to it, then
/// `Folder.syx` in that directory (C++ `companionSyxPath`). Candidate order
/// is `stem.syx`, `stem.SYX`, `Folder.syx`, `folder.syx`, `FOLDER.SYX`;
/// only regular files match.
pub fn companion_syx_path(midi_path: &Path) -> Option<PathBuf> {
    if midi_path.as_os_str().is_empty() {
        return None;
    }
    let dir = midi_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = midi_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let candidates = [
        dir.join(format!("{stem}.syx")),
        dir.join(format!("{stem}.SYX")),
        dir.join("Folder.syx"),
        dir.join("folder.syx"),
        dir.join("FOLDER.SYX"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reset_names_cover_all_kinds() {
        assert_eq!(sysex_reset_name(0), "None");
        assert_eq!(sysex_reset_name(1), "GM Reset");
        assert_eq!(sysex_reset_name(2), "GS Reset (SC-55)");
        assert_eq!(sysex_reset_name(3), "XG Reset");
        assert_eq!(sysex_reset_name(4), "MT-32 Reset");
    }

    #[test]
    fn reset_names_fall_back_to_none() {
        for kind in [-1, 5, 99, i32::MIN, i32::MAX] {
            assert_eq!(sysex_reset_name(kind), "None", "kind {kind}");
        }
    }

    #[test]
    fn reset_messages_are_byte_exact() {
        assert_eq!(
            sysex_reset_message(SYSEX_RESET_GM),
            vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7]
        );
        assert_eq!(
            sysex_reset_message(SYSEX_RESET_GS),
            vec![
                0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x00, 0x41, 0xF7
            ]
        );
        assert_eq!(
            sysex_reset_message(SYSEX_RESET_XG),
            vec![0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7]
        );
        assert_eq!(
            sysex_reset_message(SYSEX_RESET_MT32),
            vec![
                0xF0, 0x41, 0x10, 0x16, 0x12, 0x7F, 0x00, 0x00, 0x01, 0x00, 0xF7
            ]
        );
    }

    #[test]
    fn reset_message_empty_for_none_and_unknown() {
        assert!(sysex_reset_message(SYSEX_RESET_NONE).is_empty());
        for kind in [-1, 5, 99] {
            assert!(sysex_reset_message(kind).is_empty(), "kind {kind}");
        }
    }

    #[test]
    fn settle_times_match_cpp() {
        assert_eq!(sysex_reset_settle_ms(SYSEX_RESET_NONE), 0);
        assert_eq!(sysex_reset_settle_ms(SYSEX_RESET_GM), 40);
        assert_eq!(sysex_reset_settle_ms(SYSEX_RESET_GS), 50);
        assert_eq!(sysex_reset_settle_ms(SYSEX_RESET_XG), 50);
        assert_eq!(sysex_reset_settle_ms(SYSEX_RESET_MT32), 80);
        assert_eq!(sysex_reset_settle_ms(99), 0);
        assert_eq!(sysex_reset_settle_ms(-1), 0);
    }

    #[test]
    fn pacing_thresholds_match_cpp() {
        assert_eq!(sysex_pacing_ms(0), 0);
        assert_eq!(sysex_pacing_ms(6), 0);
        assert_eq!(sysex_pacing_ms(32), 0);
        assert_eq!(sysex_pacing_ms(33), 15);
        assert_eq!(sysex_pacing_ms(256), 15);
        assert_eq!(sysex_pacing_ms(257), 40);
        assert_eq!(sysex_pacing_ms(usize::MAX), 40);
    }

    #[test]
    fn parse_single_message() {
        let msg = vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7];
        assert_eq!(parse_sysex_messages(&msg), vec![msg]);
    }

    #[test]
    fn parse_concatenated_messages() {
        let bytes = vec![
            0xF0, 0x41, 0x10, 0xF7, //
            0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7,
        ];
        let out = parse_sysex_messages(&bytes);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], vec![0xF0, 0x41, 0x10, 0xF7]);
        assert_eq!(
            out[1],
            vec![0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7]
        );
    }

    #[test]
    fn parse_skips_leading_garbage_and_keeps_bare_f0_f7() {
        assert_eq!(
            parse_sysex_messages(&[0x90, 0x40, 0x7F, 0xF0, 0xF7]),
            vec![vec![0xF0, 0xF7]]
        );
    }

    #[test]
    fn parse_malformed_inputs() {
        // No F0 at all.
        assert!(parse_sysex_messages(&[0x90, 0x40, 0x7F]).is_empty());
        // Empty input.
        assert!(parse_sysex_messages(&[]).is_empty());
        // Unterminated tail is dropped, completed prefix kept.
        assert_eq!(
            parse_sysex_messages(&[0xF0, 0x01, 0xF7, 0xF0, 0x02, 0x03]),
            vec![vec![0xF0, 0x01, 0xF7]]
        );
        // A second F0 restarts, dropping the partial message.
        assert_eq!(
            parse_sysex_messages(&[0xF0, 0x01, 0x02, 0xF0, 0x03, 0xF7]),
            vec![vec![0xF0, 0x03, 0xF7]]
        );
        // Lone F0 with nothing after.
        assert!(parse_sysex_messages(&[0xF0]).is_empty());
    }

    #[test]
    fn load_missing_or_empty_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_sysex_file(&dir.path().join("nope.syx")).is_empty());
        let empty = dir.path().join("empty.syx");
        std::fs::write(&empty, []).unwrap();
        assert!(load_sysex_file(&empty).is_empty());
    }

    #[test]
    fn load_file_splits_concatenated_with_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bank.syx");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0x00, 0xFF, 0xF0, 0x41, 0xF7, 0xF0, 0x43, 0x10, 0xF7])
            .unwrap();
        drop(f);
        let out = load_sysex_file(&path);
        assert_eq!(
            out,
            vec![vec![0xF0, 0x41, 0xF7], vec![0xF0, 0x43, 0x10, 0xF7]]
        );
    }

    #[test]
    fn companion_empty_path_is_none() {
        assert_eq!(companion_syx_path(Path::new("")), None);
    }

    #[test]
    fn companion_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let midi = dir.path().join("song.mid");
        std::fs::write(&midi, b"MThd").unwrap();
        assert_eq!(companion_syx_path(&midi), None);
    }

    #[test]
    fn companion_prefers_same_name_over_folder() {
        let dir = tempfile::tempdir().unwrap();
        let midi = dir.path().join("song.mid");
        std::fs::write(&midi, b"MThd").unwrap();
        std::fs::write(dir.path().join("Folder.syx"), [0xF0, 0xF7]).unwrap();
        assert_eq!(
            companion_syx_path(&midi),
            Some(dir.path().join("Folder.syx"))
        );
        std::fs::write(dir.path().join("song.syx"), [0xF0, 0xF7]).unwrap();
        assert_eq!(companion_syx_path(&midi), Some(dir.path().join("song.syx")));
    }

    #[test]
    fn companion_case_variants_and_scoping() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let midi = sub.join("song.mid");
        std::fs::write(&midi, b"MThd").unwrap();
        // Parent-dir Folder.syx is NOT in scope.
        std::fs::write(dir.path().join("Folder.syx"), [0xF0, 0xF7]).unwrap();
        assert_eq!(companion_syx_path(&midi), None);
        // Uppercase same-name extension matches.
        std::fs::write(sub.join("song.SYX"), [0xF0, 0xF7]).unwrap();
        assert_eq!(companion_syx_path(&midi), Some(sub.join("song.SYX")));
        // Lowercase wins over uppercase when both exist.
        std::fs::write(sub.join("song.syx"), [0xF0, 0xF7]).unwrap();
        assert_eq!(companion_syx_path(&midi), Some(sub.join("song.syx")));
    }

    #[test]
    fn companion_ignores_directories() {
        let dir = tempfile::tempdir().unwrap();
        let midi = dir.path().join("song.mid");
        std::fs::write(&midi, b"MThd").unwrap();
        std::fs::create_dir(dir.path().join("song.syx")).unwrap();
        assert_eq!(companion_syx_path(&midi), None);
    }
}
