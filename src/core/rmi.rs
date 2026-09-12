//! RMI decoder — port of `Sequence::loadRmi` from `src/midi/sequence.cpp`
//! (PLAN T4, arch §1.3).
//!
//! 1:1 rules: RIFF `RMID` container; the `data` chunk feeds the SMF decoder;
//! `LIST`/`INFO` metadata (`INAM`/`IART`/`ICOP`/…) lands in the info map
//! under display names (unknown fourccs keep their raw id). All leniency
//! and every loading-error string are kept identical to the C++.
//!
//! Dependency rule (arch §5): `std` + detector/codec crates only — no
//! iced/libcosmic in `core`.

use crate::core::sequence::Sequence;
use crate::core::smf::Reader;

/// `LIST`/`INFO` fourcc → display name (the C++ `keys` table, verbatim).
fn info_display_name(id: &[u8; 4]) -> &str {
    match id {
        b"IALB" => "Album",
        b"IARL" => "Archival Location",
        b"IART" => "Artist",
        b"ICMS" => "Commissioned",
        b"ICMT" => "Comments",
        b"ICOP" => "Copyright",
        b"ICRD" => "Creation date",
        b"IENG" => "Engineer",
        b"IGNR" => "Genre",
        b"IKEY" => "Keywords",
        b"IMED" => "Medium",
        b"INAM" => "Name",
        b"IPRD" => "Product",
        b"ISBJ" => "Subject",
        b"ISFT" => "Software",
        b"ISRC" => "Source",
        b"ITCH" => "Technician",
        _ => "",
    }
}

/// Parse RMI bytes into `seq` (C++ `Sequence::loadRmi`, 1:1). Returns true
/// when the embedded SMF decoded to playable events or text records.
pub(crate) fn load_rmi(seq: &mut Sequence, data: &[u8]) -> bool {
    if data.len() < 12 || &data[0..4] != b"RIFF" {
        seq.loading_errors.push("Not a RIFF file".to_string());
        return false;
    }
    let mut r = Reader::new(data);
    r.skip(4);
    r.le32();
    if r.remaining() < 4 || &r.rest()[..4] != b"RMID" {
        seq.loading_errors.push("Not an RMID container".to_string());
        return false;
    }
    r.skip(4);
    let mut smf: Option<(usize, usize)> = None;
    while r.remaining() >= 8 {
        let mut id = [0u8; 4];
        id.copy_from_slice(&r.rest()[..4]);
        r.skip(4);
        let sz = r.le32();
        // Padding-aware advance in C++ `uint32_t` arithmetic: `sz + (sz & 1)`
        // wraps to 0 for `sz == u32::MAX`, skipping nothing, so the scan
        // continues right after this 8-byte header instead of jumping to
        // EOF. This always terminates — every iteration consumes the 8
        // header bytes — and a `data` chunk embedded in such a payload is
        // found, exactly as the C++ loop does. Only `sz == u32::MAX`
        // wraps; every other size advances identically to a saturating add.
        let advance = sz.wrapping_add(sz & 1) as usize;
        if &id == b"data" {
            smf = Some((r.pos, sz.min(r.remaining() as u32) as usize));
            r.skip(advance);
        } else if &id == b"LIST" {
            let list_end = r.pos + (sz as usize).min(r.remaining());
            if r.remaining() >= 4 && r.rest().starts_with(b"INFO") {
                r.skip(4);
                while r.pos + 8 <= list_end {
                    let mut iid = [0u8; 4];
                    iid.copy_from_slice(&r.rest()[..4]);
                    r.skip(4);
                    let isz = r.le32();
                    let mut val = r.bytes(isz.min(r.remaining() as u32) as usize);
                    if isz & 1 != 0 {
                        r.u8();
                    }
                    let name = info_display_name(&iid);
                    let key = if name.is_empty() {
                        String::from_utf8_lossy(&iid).into_owned()
                    } else {
                        name.to_string()
                    };
                    while val.last() == Some(&0) {
                        val.pop();
                    }
                    seq.info_map.insert(
                        key,
                        String::from_utf8_lossy(&val).into_owned(),
                    );
                }
            }
            r.pos = list_end;
            if sz & 1 != 0 && !r.eof() {
                r.u8();
            }
        } else {
            r.skip(advance);
        }
    }
    let (smf_pos, smf_len) = match smf {
        Some((pos, len)) if len >= 8 => (pos, len),
        _ => {
            seq.loading_errors
                .push("RMID container has no MIDI data".to_string());
            return false;
        }
    };
    let ok = crate::core::smf::load_smf_bytes(seq, &data[smf_pos..smf_pos + smf_len], "SMF");
    if ok {
        seq.file_format += " in RIFF RMID container";
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::Kind;

    /// Byte-identical twin of the oracle's `/tmp/t4_mini.rmi`: RIFF RMID
    /// wrapping the T3 mini SMF (type 0, div 120, tempo 500000@0, note 60
    /// on/off 480 ticks apart), a `JUNK` chunk (skipped), and a
    /// `LIST`/`INFO` chunk covering odd-length padding (`INAM`, `IART`),
    /// an even value (`ICOP`), and an unknown fourcc with trailing NULs
    /// (`IXYZ` → `mystery`).
    /// T3 mini SMF shared by the RMI builders below (type 0, div 120,
    /// tempo 500000@0, note 60 on/off 480 ticks apart).
    fn mini_smf() -> Vec<u8> {
        let track = vec![
            0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, 0x00, 0x90, 0x3C, 0x40, 0x83, 0x60, 0x80,
            0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00,
        ];
        let mut smf = b"MThd".to_vec();
        smf.extend_from_slice(&6u32.to_be_bytes());
        smf.extend_from_slice(&0u16.to_be_bytes());
        smf.extend_from_slice(&1u16.to_be_bytes());
        smf.extend_from_slice(&120u16.to_be_bytes());
        smf.extend_from_slice(b"MTrk");
        smf.extend_from_slice(&(track.len() as u32).to_be_bytes());
        smf.extend_from_slice(&track);
        smf
    }

    fn mini_rmi() -> Vec<u8> {
        let smf = mini_smf();

        fn info_entry(fourcc: &[u8; 4], val: &[u8]) -> Vec<u8> {
            let mut out = fourcc.to_vec();
            out.extend_from_slice(&(val.len() as u32).to_le_bytes());
            out.extend_from_slice(val);
            if val.len() & 1 != 0 {
                out.push(0);
            }
            out
        }
        let mut info = b"INFO".to_vec();
        info.extend_from_slice(&info_entry(b"INAM", b"Mini Song"));
        info.extend_from_slice(&info_entry(b"IART", b"Mini Artist"));
        info.extend_from_slice(&info_entry(b"ICOP", b"2026"));
        info.extend_from_slice(&info_entry(b"IXYZ", b"mystery\x00\x00"));

        let mut body = b"JUNK".to_vec();
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"junk");
        body.extend_from_slice(b"LIST");
        body.extend_from_slice(&(info.len() as u32).to_le_bytes());
        body.extend_from_slice(&info);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(smf.len() as u32).to_le_bytes());
        body.extend_from_slice(&smf);

        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"RMID");
        out.extend_from_slice(&body);
        out
    }

    fn fnv1a64(bytes: &[u8]) -> u64 {
        let mut h = 14695981039346656037u64;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }

    #[test]
    fn mini_builder_matches_oracle_fixture_bytes() {
        // Pins the Rust builder byte-identical to the python-built file
        // the C++ oracle ran on (`/tmp/t4_build_fixtures.py` output).
        let built = mini_rmi();
        assert_eq!(built.len(), 154);
        assert_eq!(fnv1a64(&built), 0xffd87546031e4244);
    }

    /// Oracle: throwaway `/tmp/t4_dump --events` (NOT committed) built as
    /// `g++ -std=c++17 -I src /tmp/t4_dump.cpp build/libdmidi-core.a
    /// $(pkg-config --libs glib-2.0 uchardet)`. Every expected value below
    /// is a verbatim oracle row for `/tmp/t4_mini.rmi`; the Rust side must
    /// reproduce it exactly (differential run: 27/27 dump lines identical).
    #[test]
    fn mini_matches_oracle_event_for_event() {
        let bytes = mini_rmi();
        let mut seq = Sequence::default();
        assert!(load_rmi(&mut seq, &bytes), "{:?}", seq.loading_errors);
        seq.sort_and_finalize();
        // Same aggregates the SMF mini golden pins, plus the container tag.
        assert_eq!(seq.size(), 8);
        assert_eq!(seq.song_length_ticks(), 480);
        assert_eq!(seq.division(), 120);
        assert_eq!(seq.format(), 0);
        assert_eq!(seq.num_tracks(), 1);
        assert_eq!((seq.lowest_note(), seq.highest_note()), (60, 60));
        assert!(seq.channel_used(0));
        assert_eq!(seq.text_events().len(), 0);
        assert!((seq.initial_tempo() - 500_000.0).abs() < f64::EPSILON);
        assert!(seq.simple_time_process());
        assert_eq!(seq.errors_count(), 0);
        assert_eq!(seq.file_format(), "SMF type 0 in RIFF RMID container");
        // INFO map: display names for known fourccs, raw id for `IXYZ`,
        // trailing NULs stripped, odd lengths padded (oracle `info=` row).
        assert_eq!(seq.info_map.len(), 4);
        assert_eq!(seq.info_map["Artist"], "Mini Artist");
        assert_eq!(seq.info_map["Copyright"], "2026");
        assert_eq!(seq.info_map["IXYZ"], "mystery");
        assert_eq!(seq.info_map["Name"], "Mini Song");
        assert_eq!(
            seq.metadata_info(),
            "Artist: <b>Mini Artist</b><br/>Copyright: <b>2026</b><br/>\
             IXYZ: <b>mystery</b><br/>Name: <b>Mini Song</b><br/>"
        );
        // Full event stream, verbatim oracle `EV` rows (sorted order: the
        // SMF parse order is already sorted; beats join in finalize).
        let rows: Vec<(Kind, i64, i32, i32, i32, i32, i32, u64, String)> = seq
            .events()
            .iter()
            .map(|ev| {
                (
                    ev.kind,
                    ev.tick,
                    ev.tag,
                    ev.channel,
                    ev.a,
                    ev.b,
                    ev.c,
                    ev.tempo.to_bits(),
                    ev.data.iter().map(|b| format!("{b:02x}")).collect(),
                )
            })
            .collect();
        let t500k = 500_000f64.to_bits();
        assert_eq!(
            rows,
            vec![
                (Kind::Tempo, 0, 1, 0, 0, 0, 0, t500k, String::new()),
                (Kind::NoteOn, 0, 1, 0, 60, 64, 0, t500k, String::new()),
                (Kind::Beat, 0, 0, 0, 1, 1, 4, t500k, String::new()),
                (Kind::Beat, 120, 0, 0, 1, 2, 4, t500k, String::new()),
                (Kind::Beat, 240, 0, 0, 1, 3, 4, t500k, String::new()),
                (Kind::Beat, 360, 0, 0, 1, 4, 4, t500k, String::new()),
                (Kind::NoteOff, 480, 1, 0, 60, 64, 0, t500k, String::new()),
                (Kind::Beat, 480, 0, 0, 2, 1, 4, t500k, String::new()),
            ]
        );
    }

    #[test]
    fn mini_loads_through_load_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mini.rmi");
        std::fs::write(&path, mini_rmi()).unwrap();
        let mut seq = Sequence::default();
        assert!(seq.load_file(&path), "{:?}", seq.loading_errors);
        assert_eq!(seq.size(), 8);
        assert_eq!(seq.last_bar(), 2);
        assert_eq!(seq.duration_string(), "00:00:02.000");
        assert_eq!(seq.file_format(), "SMF type 0 in RIFF RMID container");
        assert_eq!(seq.current_file(), "mini.rmi");
    }

    #[test]
    fn malformed_arms_match_oracle_errors() {
        // (name, bytes, load_rmi ok, loading_errors) — every error string
        // verified against the C++ oracle on twin files.
        let mut bad_smf_data = b"RIFF".to_vec();
        bad_smf_data.extend_from_slice(&20u32.to_le_bytes());
        bad_smf_data.extend_from_slice(b"RMID");
        bad_smf_data.extend_from_slice(b"data");
        bad_smf_data.extend_from_slice(&8u32.to_le_bytes());
        bad_smf_data.extend_from_slice(b"12345678");
        let cases: &[(&str, Vec<u8>, bool, &str)] = &[
            ("empty", vec![], false, "Not a RIFF file<br/>"),
            ("short", b"RIFF\x04\0\0\0".to_vec(), false, "Not a RIFF file<br/>"),
            ("non-riff", b"XXXX\x04\0\0\0XXXX".to_vec(), false, "Not a RIFF file<br/>"),
            (
                "non-rmid",
                [b"RIFF".as_slice(), &4u32.to_le_bytes(), b"XXXX"].concat(),
                false,
                "Not an RMID container<br/>",
            ),
            (
                "no-data",
                [
                    b"RIFF".as_slice(),
                    &12u32.to_le_bytes(),
                    b"RMID",
                    b"JUNK",
                    &4u32.to_le_bytes(),
                    b"junk",
                ]
                .concat(),
                false,
                "RMID container has no MIDI data<br/>",
            ),
            (
                "short-data",
                [
                    b"RIFF".as_slice(),
                    &16u32.to_le_bytes(),
                    b"RMID",
                    b"data",
                    &4u32.to_le_bytes(),
                    b"1234",
                ]
                .concat(),
                false,
                "RMID container has no MIDI data<br/>",
            ),
            ("bad-smf", bad_smf_data, false, "Missing MThd header<br/>"),
        ];
        for (name, bytes, ok, errors) in cases {
            let mut seq = Sequence::default();
            assert_eq!(load_rmi(&mut seq, bytes), *ok, "{name}");
            assert!(seq.empty(), "{name}");
            assert_eq!(seq.loading_errors(), *errors, "{name}");
        }
        // Through `load_file` the failure gains no extra error (the
        // decoder already recorded one).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nodata.rmi");
        std::fs::write(&path, &cases[4].1).unwrap();
        let mut seq = Sequence::default();
        assert!(!seq.load_file(&path));
        assert_eq!(seq.loading_errors(), "RMID container has no MIDI data<br/>");
    }

    /// Lead-confirmed parity (devil fixture): a `JUNK` chunk declaring
    /// `sz == u32::MAX` whose declared payload contains a benign filler
    /// chunk plus a valid `data` chunk loads OK. The C++ oracle computes
    /// `sz + (sz & 1)` in `uint32_t` (wraps to 0 → `skip(0)`) and keeps
    /// scanning — every iteration consumes the 8 header bytes, so the
    /// loop always terminates and the embedded `data` is found (oracle
    /// `/tmp/t4_dump --events /tmp/t4_devil_max.rmi`: `ok=1`, 8 events,
    /// `SMF type 0 in RIFF RMID container`, empty errors). A saturating
    /// advance instead jumps to EOF and wrongly reports no MIDI data.
    #[test]
    fn max_size_junk_rescans_to_embedded_data() {
        let smf = mini_smf();
        let mut body = b"JUNK".to_vec();
        body.extend_from_slice(&u32::MAX.to_le_bytes());
        body.extend_from_slice(b"PAD ");
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(smf.len() as u32).to_le_bytes());
        body.extend_from_slice(&smf);
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(b"RMID");
        bytes.extend_from_slice(&body);
        // Pins the builder byte-identical to the python-built file the
        // C++ oracle ran on (`/tmp/build_devil.py` output).
        assert_eq!(bytes.len(), 78);
        assert_eq!(fnv1a64(&bytes), 0xe00f5637a35667b9);

        let mut seq = Sequence::default();
        assert!(load_rmi(&mut seq, &bytes), "{:?}", seq.loading_errors);
        seq.sort_and_finalize();
        // Verbatim oracle aggregates for the embedded mini SMF.
        assert_eq!(seq.size(), 8);
        assert_eq!(seq.song_length_ticks(), 480);
        assert_eq!(seq.division(), 120);
        assert_eq!(seq.format(), 0);
        assert_eq!(seq.num_tracks(), 1);
        assert_eq!((seq.lowest_note(), seq.highest_note()), (60, 60));
        assert!(seq.channel_used(0));
        assert_eq!(seq.text_events().len(), 0);
        assert!((seq.initial_tempo() - 500_000.0).abs() < f64::EPSILON);
        assert!(seq.simple_time_process());
        assert_eq!(seq.errors_count(), 0);
        assert_eq!(seq.file_format(), "SMF type 0 in RIFF RMID container");
        assert!(seq.info_map.is_empty());
        assert_eq!(seq.loading_errors(), "");
    }
}
