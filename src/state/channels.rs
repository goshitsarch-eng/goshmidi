//! Per-channel mix state + the solo/mute reduction math (arch §2.1, ux
//! §3.7).
//!
//! The C++ twin is the channels page (`src/ui/window.cpp:1870-1947`) with
//! `applyChannelSoloMute` (`window.cpp:479-495`): any-solo attenuates every
//! non-soloed channel by `(100 - soloVolumeReduction)%`, combined
//! multiplicatively with the per-channel slider; mute goes straight to the
//! engine.

use crate::core::event::MIDI_CHANNELS;
use crate::core::instruments;

/// Channel volume slider range (`GtkScale` 0..200, `window.cpp:1890`).
pub const LEVEL_MIN_PCT: u32 = 0;
pub const LEVEL_MAX_PCT: u32 = 200;

/// Meter decay per 50 ms tick (`window.cpp:217-225`).
pub const METER_DECAY: f32 = 0.82;

/// One channel row (arch §2.1 `ChannelState`).
#[derive(Debug, Clone)]
pub struct ChannelState {
    /// `channelUsed`: rows render only for used channels.
    pub used: bool,
    /// Channel display name (`channelLabel` / song-settings `name`).
    pub label: String,
    pub muted: bool,
    pub soloed: bool,
    pub locked: bool,
    /// Patch dropdown selection, 0..=127.
    pub patch: u8,
    /// Volume slider, `LEVEL_MIN_PCT..=LEVEL_MAX_PCT`, default 100.
    pub level_pct: u32,
    /// Decaying meter `0.0..=1.0` (peak set on NoteOn, `*= METER_DECAY`
    /// every 50 ms). Rendered by the channels page (T12).
    pub level: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            used: false,
            label: String::new(),
            muted: false,
            soloed: false,
            locked: false,
            patch: 0,
            level_pct: 100,
            level: 0.0,
        }
    }
}

impl ChannelState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clamp + store the volume slider; returns the stored value.
    pub fn set_level_pct(&mut self, pct: u32) -> u32 {
        self.level_pct = pct.clamp(LEVEL_MIN_PCT, LEVEL_MAX_PCT);
        self.level_pct
    }

    /// Engine channel-volume factor for this row (`applyChannelSoloMute`
    /// parity — see [`engine_factor`]).
    pub fn factor(&self, any_solo: bool, solo_reduction: i32) -> f64 {
        engine_factor(self.level_pct, self.soloed, any_solo, solo_reduction)
    }
}

/// Engine channel-volume factor, EXACTLY per `applyChannelSoloMute`
/// (`window.cpp:484-493`):
/// ```text
/// reduce = (100 - soloVolumeReduction) / 100
/// factor = 1.0; if anySolo && !soloed: factor = reduce
/// factor *= levelPct / 100
/// ```
pub fn engine_factor(level_pct: u32, soloed: bool, any_solo: bool, solo_reduction: i32) -> f64 {
    let mut factor = 1.0;
    if any_solo && !soloed {
        factor = f64::from(100 - solo_reduction) / 100.0;
    }
    factor * f64::from(level_pct) / 100.0
}

/// Whether any channel is soloed (the `anySolo` scan, `window.cpp:481-483`).
pub fn any_solo(channels: &[ChannelState; MIDI_CHANNELS]) -> bool {
    channels.iter().any(|c| c.soloed)
}

/// Engine-bound sync values for one row: the per-row half of
/// `applyChannelSoloMute` (`window.cpp:486-493`). The app sends each as a
/// `SetMuted` + `SetChannelVolume` pair, in channel order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelSync {
    /// Straight to `SetMuted`: mute silences the whole channel engine-side
    /// (the muted check runs before pitch/drums dispatch), drums included —
    /// there is no drums exception anywhere in the mix path. The ONLY drums
    /// special-case is pitch-shift exclusion at note dispatch.
    pub muted: bool,
    /// Straight to `SetChannelVolume`: solo reduction × slider, EXACT
    /// `applyChannelSoloMute` math — see [`engine_factor`].
    pub factor: f64,
}

/// Per-row sync values for `row` (`applyChannelSoloMute` row half).
pub fn sync_row(row: &ChannelState, any_solo: bool, solo_reduction: i32) -> ChannelSync {
    ChannelSync {
        muted: row.muted,
        factor: row.factor(any_solo, solo_reduction),
    }
}

/// Whole-mix sync values (the full `applyChannelSoloMute` loop): `any_solo`
/// scanned once, then one [`ChannelSync`] per row. Used after solo flips
/// (which change every row's factor), song-settings loads, and prefs commits
/// (solo% — U14).
pub fn sync_all(
    channels: &[ChannelState; MIDI_CHANNELS],
    solo_reduction: i32,
) -> [ChannelSync; MIDI_CHANNELS] {
    let any = any_solo(channels);
    std::array::from_fn(|ch| sync_row(&channels[ch], any, solo_reduction))
}

/// Meter peak on NoteOn (`m_chLevel[ch] = max(m_chLevel[ch], vel / 127.0)`,
/// `window.cpp:149`). NoteOff leaves the meter alone (C++ parity: `onNoteOff`
/// never touches `m_chLevel`, `window.cpp:152-156`); decay is [`decay_meters`].
pub fn note_peak(level: f32, vel: u8) -> f32 {
    level.max(f32::from(vel) / 127.0)
}

/// Decay every meter one tick (`*= 0.82`, `window.cpp:220`).
pub fn decay_meters(channels: &mut [ChannelState; MIDI_CHANNELS]) {
    for c in channels.iter_mut() {
        c.level *= METER_DECAY;
    }
}

/// A8 load sync: fresh default rows, then `used` + `label` from the opened
/// song snapshot (`refreshChannels` label/visibility half, `window.cpp:537-546`,
/// PLUS the mix reset the C++ UI skipped — the engine's `load_song` already
/// reset its own mix, so stale mute/solo/lock/patch/level here would lie).
pub fn reset_for_song(
    channels: &mut [ChannelState; MIDI_CHANNELS],
    used: &[bool; MIDI_CHANNELS],
    labels: &[String; MIDI_CHANNELS],
) {
    for (ch, slot) in channels.iter_mut().enumerate() {
        *slot = ChannelState::new();
        slot.used = used[ch];
        slot.label = labels[ch].clone();
    }
}

/// Patch dropdown label for one patch under `map` (`patchName(map, p)`:
/// MT-32 has its own table, every other map reads GM capital tones).
pub fn patch_label(map: i32, patch: u8) -> &'static str {
    instruments::patch_name(map, i32::from(patch))
}

/// Shared 128-entry patch model (`applyInstrumentMap`, `window.cpp:518-531`):
/// all `patchName(map, p)` labels in program order. Zero-alloc (`&'static`
/// table entries): the channels view-model builder calls this per frame and
/// the labels only change when the instrument map does (prefs close / MIDI
/// apply / profile inference). Live `Program` events only reselect
/// (`ChannelState::patch`), never rebuild.
pub fn patch_options(map: i32) -> [&'static str; 128] {
    std::array::from_fn(|p| patch_label(map, p as u8))
}

/// Default 16-channel mix (fresh engine state, A8 sync target).
pub fn default_mix() -> [ChannelState; MIDI_CHANNELS] {
    std::array::from_fn(|_| ChannelState::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_cpp_row_init() {
        let c = ChannelState::new();
        assert!(!c.used);
        assert!(!c.muted && !c.soloed && !c.locked);
        assert_eq!(c.patch, 0);
        assert_eq!(c.level_pct, 100);
        assert_eq!(c.level, 0.0);
    }

    #[test]
    fn level_slider_clamps_0_to_200() {
        let mut c = ChannelState::new();
        assert_eq!(c.set_level_pct(200), 200);
        assert_eq!(c.set_level_pct(0), 0);
        assert_eq!(c.set_level_pct(999), 200);
    }

    /// `applyChannelSoloMute` formula matrix (`window.cpp:479-495`).
    #[test]
    fn solo_mute_reduction_math_is_exact() {
        let cases = [
            // (level, soloed, any_solo, reduction, want)
            // No solo anywhere: factor is just the slider.
            (100, false, false, 50, 1.0),
            (200, false, false, 50, 2.0),
            (0, false, false, 50, 0.0),
            // Soloed row keeps its slider even when a solo exists.
            (100, true, true, 50, 1.0),
            (150, true, true, 75, 1.5),
            // Non-soloed row under a solo: (100 - reduction)% × slider.
            (100, false, true, 50, 0.5),
            (200, false, true, 50, 1.0),
            (100, false, true, 0, 1.0),
            (100, false, true, 100, 0.0),
            (120, false, true, 75, 0.3),
            // Slider 0 silences even a soloed row (multiplicative combine).
            (0, true, true, 50, 0.0),
        ];
        for (level, soloed, any, reduction, want) in cases {
            let got = engine_factor(level, soloed, any, reduction);
            assert!(
                (got - want).abs() < 1e-9,
                "engine_factor({level}, {soloed}, {any}, {reduction}) = {got}, want {want}"
            );
        }
    }

    #[test]
    fn any_solo_scans_all_16() {
        let mut mix = default_mix();
        assert!(!any_solo(&mix));
        mix[15].soloed = true;
        assert!(any_solo(&mix));
        mix[15].soloed = false;
        assert!(!any_solo(&mix));
    }

    #[test]
    fn meter_decay_matches_cpp_timer() {
        let mut mix = default_mix();
        mix[0].level = 1.0;
        mix[1].level = 0.5;
        decay_meters(&mut mix);
        assert!((mix[0].level - 0.82).abs() < 1e-6);
        assert!((mix[1].level - 0.41).abs() < 1e-6);
        for _ in 0..1000 {
            decay_meters(&mut mix);
        }
        assert!(mix[0].level < 1e-9);
    }

    #[test]
    fn channel_factor_method_matches_free_function() {
        let mut c = ChannelState::new();
        c.set_level_pct(120);
        c.soloed = true;
        assert!((c.factor(true, 75) - engine_factor(120, true, true, 75)).abs() < f64::EPSILON);
    }

    /// `sync_row`: muted passes straight through, factor is the exact
    /// `applyChannelSoloMute` row (`window.cpp:486-493`), incl. the reduction
    /// edges 0/100 and raw passthrough of out-of-range reductions (C++ reads
    /// `soloVolumeReduction` raw into the formula — a corrupt config value
    /// yields a negative factor the engine's `bounded_floor` clamps to 0).
    #[test]
    fn sync_row_matrix_is_exact() {
        let mut row = ChannelState::new();
        row.set_level_pct(150);
        // No solo anywhere: slider only, mute flag straight through.
        assert_eq!(
            sync_row(&row, false, 50),
            ChannelSync {
                muted: false,
                factor: 1.5
            }
        );
        row.muted = true;
        assert!(sync_row(&row, false, 50).muted);
        row.muted = false;
        // Reduction edges on a non-soloed row under a solo.
        assert!((sync_row(&row, true, 0).factor - 1.5).abs() < 1e-9);
        assert!((sync_row(&row, true, 100).factor - 0.0).abs() < 1e-9);
        // Out-of-range reductions pass through raw (C++ parity, unclamped).
        assert!((sync_row(&row, true, 150).factor - -0.75).abs() < 1e-9);
        assert!((sync_row(&row, true, -20).factor - 1.8).abs() < 1e-9);
        // A soloed row keeps its slider whatever the reduction.
        row.soloed = true;
        assert!((sync_row(&row, true, 100).factor - 1.5).abs() < 1e-9);
    }

    /// `sync_all`: `any_solo` scanned once across all 16; a solo on the last
    /// row attenuates every other row while each row keeps its own mute flag.
    #[test]
    fn sync_all_reduces_every_non_solo_row() {
        let mut mix = default_mix();
        mix[0].set_level_pct(200);
        mix[3].muted = true;
        mix[15].soloed = true;
        let sync = sync_all(&mix, 50);
        // Soloed row: slider untouched.
        assert!((sync[15].factor - 1.0).abs() < 1e-9);
        assert!(!sync[15].muted);
        // Every other row: (100 - 50)% × slider; mute flags per-row.
        assert!((sync[0].factor - 1.0).abs() < 1e-9);
        assert!(!sync[0].muted);
        assert!((sync[3].factor - 0.5).abs() < 1e-9);
        assert!(sync[3].muted);
        // No solo anywhere: every factor is just its slider.
        mix[15].soloed = false;
        let sync = sync_all(&mix, 50);
        assert!((sync[0].factor - 2.0).abs() < 1e-9);
        assert!((sync[3].factor - 1.0).abs() < 1e-9);
    }

    /// Drums interplay: the drums row (index 9 = channel 10, the default
    /// `drumsChannel`) syncs IDENTICALLY to any other row under the same
    /// inputs — `applyChannelSoloMute` has no drums branch, and engine-side
    /// mute silences drums like every channel (the muted check precedes
    /// pitch dispatch). The only drums special-case anywhere is pitch-shift
    /// exclusion at note dispatch (engine, T7-tested).
    #[test]
    fn drums_row_has_no_mix_exception() {
        let mut mix = default_mix();
        mix[0].set_level_pct(140);
        mix[9].set_level_pct(140);
        mix[9].muted = true;
        mix[5].soloed = true;
        let sync = sync_all(&mix, 60);
        assert_eq!(sync[0].factor, sync[9].factor);
        assert!((sync[9].factor - 0.56).abs() < 1e-9);
        assert!(!sync[0].muted);
        assert!(sync[9].muted);
        // And a soloed drums row keeps its slider like any soloed row.
        mix[9].muted = false;
        mix[9].soloed = true;
        let sync = sync_all(&mix, 60);
        assert!((sync[9].factor - 1.4).abs() < 1e-9);
    }

    /// Meter peak (`window.cpp:149`): max wins, vel 0 never lowers the peak.
    #[test]
    fn note_peak_keeps_max() {
        assert!((note_peak(0.0, 100) - 100.0 / 127.0).abs() < 1e-6);
        assert!((note_peak(0.9, 10) - 0.9).abs() < f32::EPSILON);
        assert!((note_peak(0.5, 0) - 0.5).abs() < f32::EPSILON);
        assert!((note_peak(0.0, 127) - 1.0).abs() < f32::EPSILON);
    }

    /// A8 load sync: stale mix state is replaced by fresh rows carrying the
    /// snapshot's used flags + labels.
    #[test]
    fn reset_for_song_replaces_mix_with_snapshot_rows() {
        let mut mix = default_mix();
        mix[0].muted = true;
        mix[0].soloed = true;
        mix[0].locked = true;
        mix[0].patch = 40;
        mix[0].set_level_pct(20);
        mix[0].label = "Stale".to_string();
        mix[0].level = 0.7;
        let used: [bool; MIDI_CHANNELS] = std::array::from_fn(|ch| ch < 2);
        let labels: [String; MIDI_CHANNELS] = std::array::from_fn(|ch| format!("Track{ch}"));
        reset_for_song(&mut mix, &used, &labels);
        assert!(!mix[0].muted && !mix[0].soloed && !mix[0].locked);
        assert_eq!(mix[0].patch, 0);
        assert_eq!(mix[0].level_pct, 100);
        assert_eq!(mix[0].level, 0.0);
        assert!(mix[0].used);
        assert_eq!(mix[0].label, "Track0");
        assert!(mix[1].used);
        assert!(!mix[2].used);
        // Unused rows still carry their snapshot label (hidden, not rendered).
        assert_eq!(mix[2].label, "Track2");
    }

    /// Patch model relabel per map (`applyInstrumentMap` twin): MT-32 reads
    /// its own table, GM/GS/unknown read GM capital tones, 128 entries.
    #[test]
    fn patch_options_relabel_per_map() {
        let gm = patch_options(0);
        let gs = patch_options(1);
        let mt32 = patch_options(2);
        let unknown = patch_options(99);
        assert_eq!(gm.len(), 128);
        assert_eq!(gm[0], "Acoustic Grand Piano");
        assert_eq!(gm[127], "Gunshot");
        assert_eq!(gs, gm);
        assert_eq!(unknown, gm);
        assert_eq!(mt32[0], "Acou Piano 1");
        assert_eq!(mt32[127], "Hollow");
        assert_eq!(patch_label(0, 40), "Violin");
        assert_eq!(patch_label(2, 40), "Echo Bell");
    }
}
