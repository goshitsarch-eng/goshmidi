//! Per-channel mix state + the solo/mute reduction math (arch §2.1, ux
//! §3.7).
//!
//! The C++ twin is the channels page (`src/ui/window.cpp:1870-1947`) with
//! `applyChannelSoloMute` (`window.cpp:479-495`): any-solo attenuates every
//! non-soloed channel by `(100 - soloVolumeReduction)%`, combined
//! multiplicatively with the per-channel slider; mute goes straight to the
//! engine.

use crate::core::event::MIDI_CHANNELS;

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
    #[allow(dead_code)]
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

/// Decay every meter one tick (`*= 0.82`, `window.cpp:220`).
pub fn decay_meters(channels: &mut [ChannelState; MIDI_CHANNELS]) {
    for c in channels.iter_mut() {
        c.level *= METER_DECAY;
    }
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
}
