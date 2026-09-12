//! GM + MT-32 patch-name tables and device-profile sniffing — port of
//! `src/app/instruments.hpp`.
//!
//! PLAN T5, arch §1.2 (instrument row) and §1.5 (`inferDeviceProfile`).
//! Table entries, `patchName` fallback (GS shares GM capital tones), and the
//! label-sniffing order (GM-emulation exception first) match the C++ exactly.
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use crate::core::sysex::{SYSEX_RESET_GM, SYSEX_RESET_GS, SYSEX_RESET_MT32, SYSEX_RESET_XG};

/// General MIDI map (C++ `kInstrumentMapGm`).
pub const INSTRUMENT_MAP_GM: i32 = 0;
/// Roland SC-55 / Sound Canvas map, capital tones (C++ `kInstrumentMapGs`).
pub const INSTRUMENT_MAP_GS: i32 = 1;
/// Roland MT-32 / CM-32L map (C++ `kInstrumentMapMt32`).
pub const INSTRUMENT_MAP_MT32: i32 = 2;

/// General MIDI patch names, programs 0–127 (C++ `kGmPatchNames`).
pub const GM_PATCH_NAMES: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-Tonk",
    "Rhodes Piano",
    "Chorused Piano",
    "Harpsichord",
    "Clavinet",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Hammond Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Acoustic Guitar (Nylon)",
    "Acoustic Guitar (Steel)",
    "Electric Guitar (Jazz)",
    "Electric Guitar (Clean)",
    "Electric Guitar (Muted)",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    "Acoustic Bass",
    "Electric Bass (Finger)",
    "Electric Bass (Pick)",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Lead 1 - Square Wave",
    "Lead 2 - Saw Tooth",
    "Lead 3 - Calliope",
    "Lead 4 - Chiflead",
    "Lead 5 - Charang",
    "Lead 6 - Voice",
    "Lead 7 - Fifths",
    "Lead 8 - Bass+Lead",
    "Pad 1 - New Age",
    "Pad 2 - Warm",
    "Pad 3 - Polysynth",
    "Pad 4 - Choir",
    "Pad 5 - Bow",
    "Pad 6 - Metallic",
    "Pad 7 - Halo",
    "Pad 8 - Sweep",
    "FX 1 - Rain",
    "FX 2 - Soundtrack",
    "FX 3 - Crystal",
    "FX 4 - Atmosphere",
    "FX 5 - Brightness",
    "FX 6 - Goblins",
    "FX 7 - Echoes",
    "FX 8 - Sci-fi",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bagpipe",
    "Fiddle",
    "Shannai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drum",
    "Wood Block",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone",
    "Helicopter",
    "Applause",
    "Gunshot",
];

/// Roland MT-32 / CM-32L preset patch names, programs 0–127
/// (C++ `kMt32PatchNames`).
pub const MT32_PATCH_NAMES: [&str; 128] = [
    "Acou Piano 1",
    "Acou Piano 2",
    "Acou Piano 3",
    "Elec Piano 1",
    "Elec Piano 2",
    "Elec Piano 3",
    "Elec Piano 4",
    "Honkytonk",
    "Elec Org 1",
    "Elec Org 2",
    "Elec Org 3",
    "Elec Org 4",
    "Pipe Org 1",
    "Pipe Org 2",
    "Pipe Org 3",
    "Accordion",
    "Harpsi 1",
    "Harpsi 2",
    "Harpsi 3",
    "Clavi 1",
    "Clavi 2",
    "Clavi 3",
    "Celesta 1",
    "Celesta 2",
    "Syn Brass 1",
    "Syn Brass 2",
    "Syn Brass 3",
    "Syn Brass 4",
    "Syn Bass 1",
    "Syn Bass 2",
    "Syn Bass 3",
    "Syn Bass 4",
    "Fantasy",
    "Harmo Pan",
    "Chorale",
    "Glasses",
    "Soundtrack",
    "Atmosphere",
    "Warm Bell",
    "Funny Vox",
    "Echo Bell",
    "Ice Rain",
    "Oboe 2001",
    "Echo Pan",
    "Doctor Solo",
    "School Daze",
    "Bellsinger",
    "Square Wave",
    "Str Sect 1",
    "Str Sect 2",
    "Str Sect 3",
    "Pizzicato",
    "Violin 1",
    "Violin 2",
    "Cello 1",
    "Cello 2",
    "Contrabass",
    "Harp 1",
    "Harp 2",
    "Guitar 1",
    "Guitar 2",
    "Elec Gtr 1",
    "Elec Gtr 2",
    "Sitar",
    "Acou Bass 1",
    "Acou Bass 2",
    "Elec Bass 1",
    "Elec Bass 2",
    "Slap Bass 1",
    "Slap Bass 2",
    "Fretless 1",
    "Fretless 2",
    "Flute 1",
    "Flute 2",
    "Piccolo 1",
    "Piccolo 2",
    "Recorder",
    "Pan Pipes",
    "Sax 1",
    "Sax 2",
    "Sax 3",
    "Sax 4",
    "Clarinet 1",
    "Clarinet 2",
    "Oboe",
    "Engl Horn",
    "Bassoon",
    "Harmonica",
    "Trumpet 1",
    "Trumpet 2",
    "Trombone 1",
    "Trombone 2",
    "Fr Horn 1",
    "Fr Horn 2",
    "Tuba",
    "Brs Sect 1",
    "Brs Sect 2",
    "Vibe 1",
    "Vibe 2",
    "Marimba",
    "Koto",
    "Sho",
    "Shakuhachi",
    "Whistle 1",
    "Whistle 2",
    "Bottle Blow",
    "Breath Pipe",
    "Timpani",
    "Melodic Tom",
    "Deep Snare",
    "Elec Perc 1",
    "Elec Perc 2",
    "Taiko",
    "Taiko Rim",
    "Cymbal",
    "Castanets",
    "Triangle",
    "Orche Hit",
    "Telephone",
    "Bird Tweet",
    "One Note Jam",
    "Water Bells",
    "Jungle Tune",
    "Nightmare",
    "Wind",
    "Atmosphere 2",
    "Ninja",
    "Hollow",
];

/// Display name for an instrument map; unknown maps read as GM
/// (C++ `instrumentMapName`).
pub fn instrument_map_name(map: i32) -> &'static str {
    match map {
        INSTRUMENT_MAP_GS => "GS (SC-55)",
        INSTRUMENT_MAP_MT32 => "MT-32",
        _ => "GM",
    }
}

/// GM patch name; out-of-range programs read as "Unknown"
/// (C++ `gmPatchName`).
pub fn gm_patch_name(patch: i32) -> &'static str {
    if (0..128).contains(&patch) {
        GM_PATCH_NAMES[patch as usize]
    } else {
        "Unknown"
    }
}

/// MT-32 patch name; out-of-range programs read as "Unknown"
/// (C++ `mt32PatchName`).
pub fn mt32_patch_name(patch: i32) -> &'static str {
    if (0..128).contains(&patch) {
        MT32_PATCH_NAMES[patch as usize]
    } else {
        "Unknown"
    }
}

/// Patch name for a map: MT-32 uses its own table, every other map (GM, GS,
/// unknown) uses GM capital tones (C++ `patchName`).
pub fn patch_name(map: i32, patch: i32) -> &'static str {
    if map == INSTRUMENT_MAP_MT32 {
        mt32_patch_name(patch)
    } else {
        gm_patch_name(patch)
    }
}

/// Drum-channel patch label (C++ `percussionName`).
pub fn percussion_name(patch: i32) -> String {
    format!("Percussion {}", patch + 1)
}

/// Guess the instrument map + SysEx reset kind from an ALSA/FluidSynth port
/// label (C++ `inferDeviceProfile`). Munt's GM-emulation port is checked
/// first so it keeps GM names/reset despite matching "munt" too.
pub fn infer_device_profile(label: &str) -> (i32, i32) {
    let lower = label.to_ascii_lowercase();
    let has = |s: &str| lower.contains(s);
    if has("gm emulation") || has("gm-emulation") {
        return (INSTRUMENT_MAP_GM, SYSEX_RESET_GM);
    }
    if has("mt-32")
        || has("mt32")
        || has("munt")
        || has("mt32emu")
        || has("cm-32")
        || has("cm32")
        || has("cm-64")
        || has("cm64")
        || has("lapc")
    {
        return (INSTRUMENT_MAP_MT32, SYSEX_RESET_MT32);
    }
    if has("sc-55")
        || has("sc55")
        || has("sc-88")
        || has("sc88")
        || has("sc-8850")
        || has("canvas")
        || has("rt-55")
        || has("rt55")
        || has("nuked")
        || has("virtual sc55")
        || has("virtual sc-55")
    {
        return (INSTRUMENT_MAP_GS, SYSEX_RESET_GS);
    }
    if has("xg") || has("s-yxg") || has("mu50") || has("mu80") || has("mu100") {
        return (INSTRUMENT_MAP_GM, SYSEX_RESET_XG);
    }
    (INSTRUMENT_MAP_GM, SYSEX_RESET_GM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_hold_all_128_programs() {
        assert_eq!(GM_PATCH_NAMES.len(), 128);
        assert_eq!(MT32_PATCH_NAMES.len(), 128);
        assert!(!GM_PATCH_NAMES.iter().any(|n| n.is_empty()));
        assert!(!MT32_PATCH_NAMES.iter().any(|n| n.is_empty()));
    }

    #[test]
    fn patch_table_spot_checks() {
        assert_eq!(GM_PATCH_NAMES[0], "Acoustic Grand Piano");
        assert_eq!(GM_PATCH_NAMES[40], "Violin");
        assert_eq!(GM_PATCH_NAMES[80], "Lead 1 - Square Wave");
        assert_eq!(GM_PATCH_NAMES[127], "Gunshot");
        assert_eq!(MT32_PATCH_NAMES[0], "Acou Piano 1");
        assert_eq!(MT32_PATCH_NAMES[32], "Fantasy");
        assert_eq!(MT32_PATCH_NAMES[100], "Koto");
        assert_eq!(MT32_PATCH_NAMES[127], "Hollow");
    }

    #[test]
    fn patch_name_uses_gm_for_gm_gs_and_unknown_maps() {
        for map in [INSTRUMENT_MAP_GM, INSTRUMENT_MAP_GS, -1, 99] {
            assert_eq!(patch_name(map, 0), "Acoustic Grand Piano", "map {map}");
            assert_eq!(patch_name(map, 127), "Gunshot", "map {map}");
        }
        assert_eq!(patch_name(INSTRUMENT_MAP_MT32, 0), "Acou Piano 1");
        assert_eq!(patch_name(INSTRUMENT_MAP_MT32, 127), "Hollow");
    }

    #[test]
    fn out_of_range_patch_is_unknown() {
        for patch in [-1, 128, -100, 1000, i32::MIN, i32::MAX] {
            assert_eq!(gm_patch_name(patch), "Unknown");
            assert_eq!(mt32_patch_name(patch), "Unknown");
            assert_eq!(patch_name(INSTRUMENT_MAP_GM, patch), "Unknown");
            assert_eq!(patch_name(INSTRUMENT_MAP_MT32, patch), "Unknown");
        }
    }

    #[test]
    fn map_names_and_percussion() {
        assert_eq!(instrument_map_name(INSTRUMENT_MAP_GM), "GM");
        assert_eq!(instrument_map_name(INSTRUMENT_MAP_GS), "GS (SC-55)");
        assert_eq!(instrument_map_name(INSTRUMENT_MAP_MT32), "MT-32");
        assert_eq!(instrument_map_name(99), "GM");
        assert_eq!(percussion_name(0), "Percussion 1");
        assert_eq!(percussion_name(127), "Percussion 128");
    }

    #[test]
    fn infer_mt32_family() {
        let mt = (INSTRUMENT_MAP_MT32, SYSEX_RESET_MT32);
        assert_eq!(infer_device_profile("Munt MT-32 (128:0)"), mt);
        assert_eq!(infer_device_profile("Roland MT-32"), mt);
        assert_eq!(infer_device_profile("mt32emu"), mt);
        assert_eq!(infer_device_profile("MUNT"), mt);
        assert_eq!(infer_device_profile("Roland CM-32L"), mt);
        assert_eq!(infer_device_profile("CM64"), mt);
        assert_eq!(infer_device_profile("LAPC-I"), mt);
    }

    #[test]
    fn infer_gm_emulation_exception_wins_over_munt() {
        let gm = (INSTRUMENT_MAP_GM, SYSEX_RESET_GM);
        assert_eq!(infer_device_profile("Munt GM Emulation"), gm);
        assert_eq!(infer_device_profile("Munt GM-Emulation (129:0)"), gm);
        assert_eq!(infer_device_profile("gm emulation"), gm);
    }

    #[test]
    fn infer_gs_family() {
        let gs = (INSTRUMENT_MAP_GS, SYSEX_RESET_GS);
        assert_eq!(infer_device_profile("Roland SC-55 (24:0)"), gs);
        assert_eq!(infer_device_profile("Virtual SC55"), gs);
        assert_eq!(infer_device_profile("Virtual SC-55"), gs);
        assert_eq!(infer_device_profile("Sound Canvas"), gs);
        assert_eq!(infer_device_profile("Nuked SC-55"), gs);
        assert_eq!(infer_device_profile("Roland RT-55"), gs);
        assert_eq!(infer_device_profile("SC-88"), gs);
        assert_eq!(infer_device_profile("SC-8850"), gs);
    }

    #[test]
    fn infer_xg_family_and_unknown_default() {
        let xg = (INSTRUMENT_MAP_GM, SYSEX_RESET_XG);
        assert_eq!(infer_device_profile("Yamaha XG"), xg);
        assert_eq!(infer_device_profile("S-YXG50"), xg);
        assert_eq!(infer_device_profile("MU80"), xg);
        assert_eq!(infer_device_profile("MU100"), xg);
        let gm = (INSTRUMENT_MAP_GM, SYSEX_RESET_GM);
        assert_eq!(infer_device_profile("TiMidity"), gm);
        assert_eq!(infer_device_profile("FluidSynth (default.sf2)"), gm);
        assert_eq!(infer_device_profile(""), gm);
    }
}
