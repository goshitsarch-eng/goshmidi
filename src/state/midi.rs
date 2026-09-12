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

/// Backend/port/soundfont slice of a prefs commit (the D3-owned keys as seen
/// by the commit-on-close path — D2 edits none of them, so a change here
/// means the shell carried something new and the output must reseat).
#[derive(Clone, Copy, Debug)]
pub struct OutputCommit<'a> {
    pub backend: &'a str,
    pub port: &'a str,
    pub soundfont: &'a str,
    pub advanced: bool,
}

/// What a prefs-commit output change must send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputReseat {
    /// Backend or port changed → `SelectOutput` (the manager closes the
    /// previous backend first — close+reopen semantics, T5).
    pub select_output: bool,
    /// SoundFont changed to a non-empty path → `SetSoundFont(path)`. A change
    /// to empty sends nothing (D3 Apply skips empty too — the engine keeps
    /// its loaded font); the cleared path still persists via the settings save.
    pub sound_font: Option<String>,
}

impl MidiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adopt a committed output selection: update the mirrors for exactly the
    /// keys the commit changed, report what to send. Compared against the
    /// PRE-commit selection (`old`), never the live mirrors — after a startup
    /// fallback the mirrors hold the working fallback while settings still
    /// name the dead saved port, and an unrelated prefs close must neither
    /// reselect nor clobber those mirrors (no-op when `old == new`).
    /// `advanced` is mirror-only (no engine command exists for it — port
    /// enumeration is a UI `Task` owned by the MIDI dialog, T13).
    pub fn plan_output_reseat(
        &mut self,
        old: OutputCommit<'_>,
        new: OutputCommit<'_>,
    ) -> OutputReseat {
        let select_output = new.backend != old.backend || new.port != old.port;
        let sound_font = (new.soundfont != old.soundfont && !new.soundfont.is_empty())
            .then(|| new.soundfont.to_string());
        if select_output {
            self.backend = new.backend.to_string();
            self.port = new.port.to_string();
        }
        if new.soundfont != old.soundfont {
            self.soundfont = new.soundfont.to_string();
        }
        if new.advanced != old.advanced {
            self.advanced = new.advanced;
        }
        OutputReseat {
            select_output,
            sound_font,
        }
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

    fn commit<'a>(
        backend: &'a str,
        port: &'a str,
        soundfont: &'a str,
        advanced: bool,
    ) -> OutputCommit<'a> {
        OutputCommit {
            backend,
            port,
            soundfont,
            advanced,
        }
    }

    /// Unchanged commit (`old == new`): no sends, mirrors untouched — in
    /// particular a startup-fallback mirror (Dummy) survives a prefs close
    /// whose settings still name the dead saved port (ALSA).
    #[test]
    fn reseat_plan_noop_when_commit_unchanged() {
        let mut m = MidiState::new();
        m.backend = "Dummy".to_string();
        m.port = "dummy".to_string();
        let old = commit("ALSA", "128:0", "", false);
        let plan = m.plan_output_reseat(old, old);
        assert_eq!(
            plan,
            OutputReseat {
                select_output: false,
                sound_font: None,
            }
        );
        assert_eq!(m.backend, "Dummy");
        assert_eq!(m.port, "dummy");
        assert!(!m.advanced);
    }

    /// Backend or port change → reselect + mirrors follow; SoundFont change
    /// to non-empty → send + mirror; cleared SoundFont → mirror clears but no
    /// send (engine keeps its font); advanced flip → mirror only.
    #[test]
    fn reseat_plan_backend_port_soundfont_advanced() {
        let mut m = MidiState::new();
        let old = commit("FluidSynth", "fluidsynth", "", false);
        // Backend switch.
        let plan = m.plan_output_reseat(old, commit("Dummy", "dummy", "", false));
        assert!(plan.select_output);
        assert_eq!(plan.sound_font, None);
        assert_eq!(m.backend, "Dummy");
        assert_eq!(m.port, "dummy");
        // Port-only change on the same backend.
        let old = commit("Dummy", "dummy", "", false);
        let plan = m.plan_output_reseat(old, commit("Dummy", "other", "", false));
        assert!(plan.select_output);
        assert_eq!(m.port, "other");
        // SoundFont set: send + mirror, no reselect.
        let old = commit("Dummy", "other", "", false);
        let plan = m.plan_output_reseat(old, commit("Dummy", "other", "/sf/gm.sf2", false));
        assert!(!plan.select_output);
        assert_eq!(plan.sound_font, Some("/sf/gm.sf2".to_string()));
        assert_eq!(m.soundfont, "/sf/gm.sf2");
        // SoundFont cleared: mirror clears, nothing sent.
        let old = commit("Dummy", "other", "/sf/gm.sf2", false);
        let plan = m.plan_output_reseat(old, commit("Dummy", "other", "", false));
        assert!(!plan.select_output);
        assert_eq!(plan.sound_font, None);
        assert!(m.soundfont.is_empty());
        // Advanced flip: mirror only.
        let old = commit("Dummy", "other", "", false);
        let plan = m.plan_output_reseat(old, commit("Dummy", "other", "", true));
        assert!(!plan.select_output);
        assert_eq!(plan.sound_font, None);
        assert!(m.advanced);
    }
}
