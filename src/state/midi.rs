//! MIDI backend/port/selection state (arch §2.1, ux D3).
//!
//! The C++ twin is the MIDI Setup dialog state (`src/ui/window.cpp:929-1187`)
//! plus the startup output selection (`window.cpp:227-242`): backend + port
//! lists, the current selection, the advanced toggle, instrument map, SysEx
//! reset kind, and the SoundFont path. The actual port enumeration `Task` and
//! the dialog land in T13; T9 owns the selections, the startup fallback
//! chain, and the apply path.

use crate::core::output::{MidiOutputManager, MidiPort};

/// MIDI setup state (arch §2.1 `MidiState`).
#[allow(dead_code)] // T13 dialog reads lists/selections; T9 owns apply.
#[derive(Debug, Clone)]
pub struct MidiState {
    /// Backend names in manager order (ALSA, FluidSynth, Dummy).
    pub backends: Vec<String>,
    /// Last enumerated ports for the selected backend.
    pub ports: Vec<MidiPort>,
    /// Selected backend name.
    pub backend: String,
    /// Selected port id (`client:port`, or `fluidsynth`/`dummy`).
    pub port: String,
    /// `Show all ports` toggle (`advancedPorts`).
    pub advanced: bool,
    /// Instrument map: 0 GM, 1 GS/SC-55, 2 MT-32.
    pub instrument_map: u8,
    /// SysEx reset kind: 0 none, 1 GM, 2 GS, 3 XG, 4 MT-32.
    pub sysex_reset: u8,
    /// SoundFont path (deferred commit: stored here, applied on Test/Apply).
    pub soundfont: String,
    /// Toast `MIDI output: <backend> <port>` on the next `OutputChanged`
    /// without a warning. Set only by user-initiated applies (`MidiApply`,
    /// `connectOutput` parity) — never by the silent startup select.
    pub announce_applied: bool,
    /// Startup fallback stage (C++ ctor chain, `window.cpp:232-239`):
    /// 0 = saved port tried, 1 = FluidSynth fallback sent, 2 = Dummy sent.
    pub fallback_stage: u8,
    /// Any output confirmed working (warning-free `OutputChanged`). Gates
    /// the fallback chain: once true, later failures toast without
    /// second-guessing the selection (`connectOutput` parity).
    pub output_ok_once: bool,
}

impl Default for MidiState {
    fn default() -> Self {
        Self {
            backends: MidiOutputManager::new().backend_names(),
            ports: Vec::new(),
            // `AppSettings` defaults (`settings.hpp`).
            backend: "FluidSynth".to_string(),
            port: "fluidsynth".to_string(),
            advanced: false,
            instrument_map: 0,
            sysex_reset: 1,
            soundfont: String::new(),
            announce_applied: false,
            fallback_stage: 0,
            output_ok_once: false,
        }
    }
}

impl MidiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Next startup fallback after an `OutputChanged` warning, or `None`
    /// when the chain is exhausted (saved → FluidSynth → Dummy, then stop —
    /// the engine keeps its last state and the warning toast stands). Steps
    /// matching the current backend are skipped, except a saved Dummy still
    /// tries FluidSynth first (C++ `backend != "FluidSynth"` parity).
    pub fn next_fallback(&mut self) -> Option<(String, String)> {
        const CHAIN: [(&str, &str); 2] = [("FluidSynth", "fluidsynth"), ("Dummy", "dummy")];
        while (self.fallback_stage as usize) < CHAIN.len() {
            let (backend, port) = CHAIN[self.fallback_stage as usize];
            self.fallback_stage += 1;
            if backend != self.backend {
                self.backend = backend.to_string();
                self.port = port.to_string();
                return Some((self.backend.clone(), self.port.clone()));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_cpp_settings() {
        let m = MidiState::new();
        assert_eq!(m.backends, vec!["ALSA", "FluidSynth", "Dummy"]);
        assert_eq!(m.backend, "FluidSynth");
        assert_eq!(m.port, "fluidsynth");
        assert!(!m.advanced);
        assert_eq!(m.instrument_map, 0);
        assert_eq!(m.sysex_reset, 1);
        assert!(m.soundfont.is_empty());
        assert!(!m.announce_applied);
        assert!(!m.output_ok_once);
    }

    /// Startup fallback chain (`window.cpp:232-239`): saved → FluidSynth →
    /// Dummy, each step once, then exhausted.
    #[test]
    fn fallback_chain_saved_fluid_dummy() {
        let mut m = MidiState::new();
        m.backend = "ALSA".to_string();
        m.port = "128:0".to_string();
        assert_eq!(
            m.next_fallback(),
            Some(("FluidSynth".to_string(), "fluidsynth".to_string()))
        );
        assert_eq!(m.backend, "FluidSynth");
        assert_eq!(
            m.next_fallback(),
            Some(("Dummy".to_string(), "dummy".to_string()))
        );
        assert_eq!(m.next_fallback(), None);
        assert_eq!(m.next_fallback(), None);
    }

    #[test]
    fn fallback_skips_current_backend() {
        // Saved backend already FluidSynth: straight to Dummy.
        let mut m = MidiState::new();
        assert_eq!(
            m.next_fallback(),
            Some(("Dummy".to_string(), "dummy".to_string()))
        );
        assert_eq!(m.next_fallback(), None);
        // Saved backend already Dummy: still tries FluidSynth first (C++
        // `backend != "FluidSynth"` parity), then back to Dummy, then done.
        let mut m = MidiState::new();
        m.backend = "Dummy".to_string();
        m.port = "dummy".to_string();
        assert_eq!(
            m.next_fallback(),
            Some(("FluidSynth".to_string(), "fluidsynth".to_string()))
        );
        assert_eq!(
            m.next_fallback(),
            Some(("Dummy".to_string(), "dummy".to_string()))
        );
        assert_eq!(m.next_fallback(), None);
    }
}
