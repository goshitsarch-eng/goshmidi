//! Per-song settings: `~/.dmidiplayer/<song>.cfg` files (arch §4.3).
//!
//! Schema (`window.cpp:631-710`, `saveSongSettings`/`loadSongSettings`):
//! `[Global]` = `file`/`encoding`/`volume`/`pitch`/`timeskew`, plus one
//! `[MIDI Channel N]` group per used channel with `name`/`muted`/`solo`/
//! `locked`/`patch`/`level`. Channel group names are space-padded to width 2
//! (`format!("MIDI Channel {:2}", ch + 1)`, the Rust twin of C++
//! `snprintf(grp, "%2d")`).
//!
//! Load quirks, mirrored exactly (every one is pinned by a test below):
//!
//! - Missing/unreadable/corrupt file → no settings at all: [`SongSettings::load`]
//!   returns `None` and the caller changes nothing (C++ early-returns).
//! - `encoding`: applied only when the key holds a valid string (even `""`);
//!   missing/invalid → untouched.
//! - `volume`/`timeskew`: applied only when the key holds a valid **non-zero**
//!   integer (C++ `if (vol)` / `if (skew)`); missing/invalid/zero → untouched.
//! - `pitch`: applied **unconditionally** — missing/invalid reads as 0 and
//!   still resets the control (C++ `set_value` with no guard).
//! - A channel group that is absent is skipped wholesale; when present,
//!   missing/invalid `muted`/`solo`/`locked` read as `false`, a missing
//!   `patch` as 0, a missing `name` as untouched, and a missing/invalid/zero
//!   `level` as untouched.
//! - `file` is write-only metadata (C++ never reads it); it round-trips here
//!   as informational data.
//!
//! [`SongSettings::apply_plan`] packages the parsed result as engine-first
//! apply data for T9+ (arch §4.3 FIX ruling): the engine mix is sent first,
//! then UI state syncs from the applied engine values — never UI-first via
//! widget-signal side effects. Actual `EngineCommand` wiring lives in T9.

use std::path::Path;

use super::settings::{KeyFile, ParseError};

const GLOBAL: &str = "Global";

const GLOBAL_KNOWN: [&str; 5] = ["file", "encoding", "volume", "pitch", "timeskew"];
const CHANNEL_KNOWN: [&str; 6] = ["name", "muted", "solo", "locked", "patch", "level"];

/// Number of MIDI channels (twin of C++ `kMidiChannels`).
pub const MIDI_CHANNELS: usize = 16;

/// Group name for a 0-based channel index: `"MIDI Channel  1"` … `"MIDI
/// Channel 16"` (space-padded to width 2, like C++ `"%2d"`).
pub fn channel_group(channel: usize) -> String {
    format!("MIDI Channel {:2}", channel + 1)
}

/// Per-channel song settings. `None` at the `SongSettings::channels` level
/// means "no group in the file → skip this channel entirely".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongChannelSettings {
    /// Channel display name. `None` = key missing/invalid → untouched;
    /// `Some("")` = present-but-empty → applied (clears the name).
    pub name: Option<String>,
    /// Missing/invalid key reads as `false` (group present).
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    /// Missing/invalid key reads as 0. Clamping (`clampMidi`) happens at
    /// apply time in T9, not here.
    pub patch: i32,
    /// Channel level slider. `None` = missing/invalid/**zero** → untouched
    /// (C++ `if (lvl)`).
    pub level: Option<i32>,
}

/// Parsed song-settings file plus its preserved unknown tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongSettings {
    /// Write-only metadata: the full song path at save time. Never applied.
    pub file: String,
    /// Lyrics charset override. `None` → untouched (no refresh).
    pub encoding: Option<String>,
    /// Master volume slider (0–200). `None` → untouched.
    pub volume: Option<i32>,
    /// Pitch control. Always applied; missing/invalid reads as 0.
    pub pitch: i32,
    /// Tempo slider (50–200). `None` → untouched.
    pub timeskew: Option<i32>,
    /// Per-channel groups; `None` = group absent → skip the channel.
    pub channels: [Option<SongChannelSettings>; MIDI_CHANNELS],
    /// Unknown keys/groups preserved verbatim across load→save (A11).
    unknown: KeyFile,
}

impl SongSettings {
    /// Empty settings: what T9 starts from when [`SongSettings::load`]
    /// returns `None` (no file → change nothing).
    pub fn empty() -> Self {
        Self {
            file: String::new(),
            encoding: None,
            volume: None,
            pitch: 0,
            timeskew: None,
            channels: std::array::from_fn(|_| None),
            unknown: KeyFile::new(),
        }
    }

    /// Load and parse `path`. Returns `None` when the file is missing,
    /// unreadable, non-UTF-8 or structurally corrupt (caller changes nothing,
    /// mirroring the C++ early-return).
    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let text = std::str::from_utf8(&bytes).ok()?;
        Self::parse(text).ok()
    }

    /// Parse full file text. `Err` = structural failure.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let kf = KeyFile::parse(text)?;
        let mut settings = Self::empty();
        if let Some(Ok(v)) = kf.get_string(GLOBAL, "file") {
            settings.file = v;
        }
        if let Some(Ok(v)) = kf.get_string(GLOBAL, "encoding") {
            settings.encoding = Some(v);
        }
        if let Some(Ok(v)) = kf.get_int(GLOBAL, "volume")
            && v != 0
        {
            settings.volume = Some(v);
        }
        // Unconditional: missing/invalid is a 0 that still applies.
        settings.pitch = kf
            .get_int(GLOBAL, "pitch")
            .and_then(Result::ok)
            .unwrap_or(0);
        if let Some(Ok(v)) = kf.get_int(GLOBAL, "timeskew")
            && v != 0
        {
            settings.timeskew = Some(v);
        }
        for ch in 0..MIDI_CHANNELS {
            let group = channel_group(ch);
            // Absent group → skip the channel (`has_group` check). A group
            // holding only unknown keys still counts as present: C++ proceeds
            // and every getter fails, i.e. all-false/0/skip.
            if !kf.has_group(&group) {
                continue;
            }
            settings.channels[ch] = Some(SongChannelSettings {
                name: kf.get_string(&group, "name").and_then(Result::ok),
                muted: kf
                    .get_bool(&group, "muted")
                    .and_then(Result::ok)
                    .unwrap_or(false),
                solo: kf
                    .get_bool(&group, "solo")
                    .and_then(Result::ok)
                    .unwrap_or(false),
                locked: kf
                    .get_bool(&group, "locked")
                    .and_then(Result::ok)
                    .unwrap_or(false),
                patch: kf
                    .get_int(&group, "patch")
                    .and_then(Result::ok)
                    .unwrap_or(0),
                level: match kf.get_int(&group, "level") {
                    Some(Ok(v)) if v != 0 => Some(v),
                    _ => None,
                },
            });
        }
        // Unknown tail: everything except known [Global] keys and known
        // channel-group keys.
        let mut unknown = kf.without_keys(GLOBAL, &GLOBAL_KNOWN);
        for ch in 0..MIDI_CHANNELS {
            unknown = unknown.without_keys(&channel_group(ch), &CHANNEL_KNOWN);
        }
        settings.unknown = unknown;
        Ok(settings)
    }

    /// Save to `path` (parent dir must exist — T9 passes
    /// [`crate::config::paths::Paths::song_settings_path`] after
    /// `ensure_data_dir`). Writes `[Global]` (`file` always; the rest only
    /// when `Some`) then one group per `Some` channel in C++ key order,
    /// then the preserved unknown tail.
    ///
    /// Only channels that are `Some` get a group: T9 sets `Some` exactly for
    /// used channels (C++ `if (!channelUsed(i)) continue`). Within a group,
    /// `None` name/level keys are skipped so partial loads round-trip
    /// stably; T9-constructed saves always carry full data.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_data())
    }

    /// Serialize exactly as [`SongSettings::save`] would write.
    pub fn to_data(&self) -> String {
        let mut kf = KeyFile::new();
        kf.set_string(GLOBAL, "file", &self.file);
        if let Some(v) = &self.encoding {
            kf.set_string(GLOBAL, "encoding", v);
        }
        if let Some(v) = self.volume {
            kf.set_int(GLOBAL, "volume", v);
        }
        kf.set_int(GLOBAL, "pitch", self.pitch);
        if let Some(v) = self.timeskew {
            kf.set_int(GLOBAL, "timeskew", v);
        }
        for (ch, slot) in self.channels.iter().enumerate() {
            let Some(c) = slot else { continue };
            let group = channel_group(ch);
            if let Some(n) = &c.name {
                kf.set_string(&group, "name", n);
            }
            kf.set_bool(&group, "muted", c.muted);
            kf.set_bool(&group, "solo", c.solo);
            kf.set_bool(&group, "locked", c.locked);
            kf.set_int(&group, "patch", c.patch);
            if let Some(v) = c.level {
                kf.set_int(&group, "level", v);
            }
        }
        for (name, keys) in &self.unknown.snapshot() {
            for (k, v) in keys {
                kf.set_raw(name, k, v);
            }
        }
        kf.to_data()
    }

    /// Engine-first apply data for T9+ (arch §4.3): send the mix to the
    /// engine first (`SetMuted`/`SetLocked`/`SetPatch`/`SetChannelVolume`/
    /// `SetVolumeFactor`/`SetPitchShift`/`SetTempoFactor`), then sync UI
    /// state from the applied engine values. Only `Some` entries apply,
    /// except `pitch`, which always applies (missing reads as 0).
    pub fn apply_plan(&self) -> SongApplyPlan {
        SongApplyPlan {
            encoding: self.encoding.clone(),
            volume: self.volume,
            pitch: self.pitch,
            timeskew: self.timeskew,
            channels: self
                .channels
                .iter()
                .enumerate()
                .filter_map(|(ch, slot)| {
                    slot.as_ref().map(|c| SongChannelApply {
                        channel: ch as u8,
                        name: c.name.clone(),
                        muted: c.muted,
                        solo: c.solo,
                        locked: c.locked,
                        patch: c.patch,
                        level: c.level,
                    })
                })
                .collect(),
        }
    }
}

/// Owned apply package produced by [`SongSettings::apply_plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongApplyPlan {
    pub encoding: Option<String>,
    pub volume: Option<i32>,
    pub pitch: i32,
    pub timeskew: Option<i32>,
    /// Only channels present in the file, in ascending channel order.
    pub channels: Vec<SongChannelApply>,
}

/// One channel's apply data (`channel` is 0-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongChannelApply {
    pub channel: u8,
    pub name: Option<String>,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub patch: i32,
    pub level: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Representative song `.cfg`, shaped like `g_key_file_to_data` output:
    /// full `[Global]`, two channel groups (1-based 1 and 16, padded), an
    /// unpadded lookalike group (unknown to the loader), and an unknown key.
    const FIXTURE: &str = "[Global]\n\
        file=/home/u/MIDI/song.mid\n\
        encoding=UTF-8\n\
        volume=150\n\
        pitch=-2\n\
        timeskew=110\n\
        custom=kept\n\
        \n\
        [MIDI Channel  1]\n\
        name=Piano\\nGrand\n\
        muted=false\n\
        solo=true\n\
        locked=false\n\
        patch=0\n\
        level=120\n\
        \n\
        [MIDI Channel 16]\n\
        name=Drums\n\
        muted=false\n\
        solo=false\n\
        locked=true\n\
        patch=8\n\
        level=100\n\
        \n\
        [MIDI Channel 1]\n\
        name=Unpadded impostor\n";

    #[test]
    fn channel_group_padding_matches_snprintf() {
        assert_eq!(channel_group(0), "MIDI Channel  1");
        assert_eq!(channel_group(8), "MIDI Channel  9");
        assert_eq!(channel_group(9), "MIDI Channel 10");
        assert_eq!(channel_group(15), "MIDI Channel 16");
    }

    #[test]
    fn fixture_parses_global_and_channels() {
        let s = SongSettings::parse(FIXTURE).unwrap();
        assert_eq!(s.file, "/home/u/MIDI/song.mid");
        assert_eq!(s.encoding, Some("UTF-8".to_string()));
        assert_eq!(s.volume, Some(150));
        assert_eq!(s.pitch, -2);
        assert_eq!(s.timeskew, Some(110));
        let ch0 = s.channels[0].as_ref().unwrap();
        assert_eq!(ch0.name, Some("Piano\nGrand".to_string()));
        assert!(!ch0.muted);
        assert!(ch0.solo);
        assert!(!ch0.locked);
        assert_eq!(ch0.patch, 0);
        assert_eq!(ch0.level, Some(120));
        let ch15 = s.channels[15].as_ref().unwrap();
        assert_eq!(ch15.name, Some("Drums".to_string()));
        assert!(ch15.locked);
        assert_eq!(ch15.patch, 8);
        // Middle channels absent.
        assert!(s.channels[1..15].iter().all(Option::is_none));
        // Unpadded "[MIDI Channel 1]" is NOT channel 0 (C++ looks up the
        // padded name only).
        assert_eq!(ch0.name, Some("Piano\nGrand".to_string()));
    }

    #[test]
    fn fixture_round_trip_stable_with_unknowns_preserved() {
        let s = SongSettings::parse(FIXTURE).unwrap();
        let once = s.to_data();
        let s2 = SongSettings::parse(&once).unwrap();
        assert_eq!(s, s2);
        assert_eq!(s2.to_data(), once, "parse→write→parse must be stable");
        // Unknowns preserved verbatim (A11).
        assert!(once.contains("custom=kept\n"));
        assert!(once.contains("[MIDI Channel 1]\nname=Unpadded impostor\n"));
        // Known [Global] key order: file/encoding/volume/pitch/timeskew.
        let global = &once[..once.find("\n\n").unwrap()];
        let order = ["file=", "encoding=", "volume=", "pitch=", "timeskew="];
        let mut last = 0;
        for key in order {
            let pos = global.find(key).unwrap();
            assert!(pos > last, "{key} out of order");
            last = pos;
        }
    }

    #[test]
    fn save_shape_matches_cpp_key_order() {
        let mut s = SongSettings::empty();
        s.file = "/m/s.mid".to_string();
        s.encoding = Some("ISO-8859-1".to_string());
        s.volume = Some(150);
        s.pitch = -2;
        s.timeskew = Some(110);
        s.channels[0] = Some(SongChannelSettings {
            name: Some("  Lead".to_string()),
            muted: false,
            solo: true,
            locked: false,
            patch: 5,
            level: Some(120),
        });
        let expected = "[Global]\n\
            file=/m/s.mid\n\
            encoding=ISO-8859-1\n\
            volume=150\n\
            pitch=-2\n\
            timeskew=110\n\
            \n\
            [MIDI Channel  1]\n\
            name=\\s\\sLead\n\
            muted=false\n\
            solo=true\n\
            locked=false\n\
            patch=5\n\
            level=120\n";
        assert_eq!(s.to_data(), expected);
    }

    #[test]
    fn load_quirks_mirror_cpp_guards() {
        // volume/timeskew/level of 0 are skipped; pitch missing still applies 0.
        let s = SongSettings::parse(
            "[Global]\nvolume=0\ntimeskew=0\n\n[MIDI Channel  2]\nlevel=0\npatch=3\n",
        )
        .unwrap();
        assert_eq!(s.volume, None);
        assert_eq!(s.timeskew, None);
        assert_eq!(s.pitch, 0);
        let ch = s.channels[1].as_ref().unwrap();
        assert_eq!(ch.level, None);
        assert_eq!(ch.patch, 3);
        // Group present but keys missing: bools false, patch 0, name untouched.
        assert!(!ch.muted && !ch.solo && !ch.locked);
        assert_eq!(ch.name, None);

        // Invalid values behave like missing (except pitch → 0).
        let s = SongSettings::parse("[Global]\nvolume=xx\npitch=xx\ntimeskew=xx\nencoding=a\\qb\n")
            .unwrap();
        assert_eq!(s.volume, None);
        assert_eq!(s.pitch, 0);
        assert_eq!(s.timeskew, None);
        assert_eq!(s.encoding, None);

        // Present-but-empty encoding IS applied (C++ `if (enc)` is non-null).
        let s = SongSettings::parse("[Global]\nencoding=\n").unwrap();
        assert_eq!(s.encoding, Some(String::new()));
        // Present-but-empty name IS applied.
        let s = SongSettings::parse("[MIDI Channel  3]\nname=\n").unwrap();
        assert_eq!(s.channels[2].as_ref().unwrap().name, Some(String::new()));
    }

    #[test]
    fn missing_or_corrupt_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(SongSettings::load(&tmp.path().join("nope.mid.cfg")).is_none());
        let bad = tmp.path().join("bad.mid.cfg");
        std::fs::write(&bad, "garbage no group\n").unwrap();
        assert!(SongSettings::load(&bad).is_none());
        std::fs::write(&bad, [0xff, 0xfe]).unwrap();
        assert!(SongSettings::load(&bad).is_none());
        assert!(SongSettings::parse("garbage\n").is_err());
    }

    #[test]
    fn save_and_load_through_real_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("song.mid.cfg");
        let s = SongSettings::parse(FIXTURE).unwrap();
        s.save(&path).unwrap();
        let back = SongSettings::load(&path).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn apply_plan_shape_for_t9() {
        let s = SongSettings::parse(FIXTURE).unwrap();
        let plan = s.apply_plan();
        assert_eq!(plan.encoding, Some("UTF-8".to_string()));
        assert_eq!(plan.volume, Some(150));
        assert_eq!(plan.pitch, -2);
        assert_eq!(plan.timeskew, Some(110));
        assert_eq!(plan.channels.len(), 2);
        assert_eq!(plan.channels[0].channel, 0);
        assert_eq!(plan.channels[0].name, Some("Piano\nGrand".to_string()));
        assert!(plan.channels[0].solo);
        assert_eq!(plan.channels[1].channel, 15);
        assert!(plan.channels[1].locked);
        // Empty settings (no file): only the always-applied pitch=0 remains,
        // and T9 skips apply entirely on `load() -> None` anyway.
        let plan = SongSettings::empty().apply_plan();
        assert_eq!(plan.pitch, 0);
        assert!(plan.channels.is_empty());
        assert_eq!(plan.encoding, None);
    }
}
