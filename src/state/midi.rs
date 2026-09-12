//! MIDI backend/port/selection state (arch §2.1, ux D3).
//!
//! The C++ twin is the MIDI Setup dialog state (`src/ui/window.cpp:929-1187`)
//! plus the startup output selection (`window.cpp:227-242`): backend + port
//! lists, the current selection, the advanced toggle, instrument map, SysEx
//! reset kind, and the SoundFont path. T9 owns the selections, the startup
//! fallback chain, and the apply path; T12 owns the reseat planner; T13a
//! adds the D3 dialog logic — field order, the refill preselect rule, the
//! port→profile rule, the test-note plan, the hint strings, and
//! stop-player-first on reseat. The port-enumeration `Task`, the D3 dialog
//! shell, and the refill/profile/test/apply arms landed in T13c (`app.rs`);
//! the dialog view is `ui/midi_setup.rs` (T13b).

use crate::core::instruments::infer_device_profile;
use crate::core::output::{MidiOutputManager, MidiPort};

/// D3 field order, top to bottom (`window.cpp:961-1128`): backend label +
/// dropdown, advanced row, connection label + dropdown, map, reset,
/// SoundFont label + entry row, status label, Test/Apply button row.
pub const MIDI_SETUP_FIELDS: [&str; 8] = [
    "Output backend",
    "Show all ports",
    "Connection (your MIDI synth / Munt / USB port)",
    "Instrument map",
    "System Exclusive reset (sent before each song)",
    "FluidSynth SoundFont (.sf2 / .sf3)",
    "Status",
    "Test note / Apply",
];

/// D3 button labels (`window.cpp:979,1019,1123-1124`).
pub const REFRESH_PORTS_LABEL: &str = "Refresh ports";
/// D3 button labels (`window.cpp:979,1019,1123-1124`).
pub const SOUNDFONT_BROWSE_LABEL: &str = "Browse…";
/// D3 button labels (`window.cpp:979,1019,1123-1124`).
pub const TEST_NOTE_LABEL: &str = "Test note";
/// D3 button labels (`window.cpp:979,1019,1123-1124`).
pub const APPLY_LABEL: &str = "Apply";

/// SoundFont entry placeholder (`window.cpp:1018`).
pub const SOUNDFONT_PLACEHOLDER: &str = "Optional: your GM, MT-32, or SC-55 SoundFont";

/// Refill status when no ports enumerate (`window.cpp:1056`).
pub const NO_DESTINATIONS_HINT: &str =
    "No destinations. Start mt32emu-qt (Munt) or Nuked-SC55, then Refresh.";
/// Test-note status after the Middle C is sent (`window.cpp:1154`).
pub const TEST_NOTE_STATUS: &str = "Sent Middle C on channel 1";
/// Test-note status when no output is live (`window.cpp:1144`).
pub const NO_MIDI_OUTPUT: &str = "No MIDI output";
/// Reseat failure toast (`connectOutput`, `window.cpp:332`).
pub const UNABLE_TO_OPEN_OUTPUT: &str = "Unable to open MIDI output";

/// Test note: Middle C on channel 1 (0-based 0) at velocity 96
/// (`window.cpp:1147`).
pub const TEST_NOTE_CH: u8 = 0;
/// Test note: Middle C on channel 1 (0-based 0) at velocity 96
/// (`window.cpp:1147`).
pub const TEST_NOTE_PITCH: u8 = 60;
/// Test note: Middle C on channel 1 (0-based 0) at velocity 96
/// (`window.cpp:1147`).
pub const TEST_NOTE_VEL: u8 = 96;
/// Test note-off delay in ms (`window.cpp:1148`).
pub const TEST_NOTE_OFF_DELAY_MS: u64 = 400;

/// Which port the refill prefers (`refillFn`, `window.cpp:1044-1046`): the
/// live current port when refilling the current backend, else the saved
/// `lastOutputConnection`.
pub fn refill_want<'a>(
    last_connection: &'a str,
    refilling_current_backend: bool,
    current_port: &'a str,
) -> &'a str {
    if refilling_current_backend {
        current_port
    } else {
        last_connection
    }
}

/// Refill preselect (`window.cpp:1047-1051`): LAST port whose id equals
/// `want`, or whose label contains `want`; 0 when nothing matches (C++
/// `sel = 0` default — the dropdown keeps index 0).
///
/// LAST wins: the C++ loop assigns `sel = i` on EVERY match, so a later
/// match overwrites an earlier one (T13c reconciliation of the T13b
/// divergence flag — the `.position()` first-match read lost; verified
/// against the C++ lines, pinned by `refill_preselect_last_match_wins`).
pub fn select_refilled_port(ports: &[MidiPort], want: &str) -> usize {
    if want.is_empty() {
        return 0;
    }
    ports
        .iter()
        .rposition(|p| p.id == want || p.label.contains(want))
        .unwrap_or(0)
}

/// Refill status hint: `Some` exactly when the port list is empty
/// (`window.cpp:1055-1058`).
pub fn refill_hint(ports_empty: bool) -> Option<&'static str> {
    ports_empty.then_some(NO_DESTINATIONS_HINT)
}

/// Port pick → (instrument map, SysEx reset) auto-apply
/// (`window.cpp:1085-1087`, via `inferDeviceProfile`). Composition applies
/// this ONLY on user-initiated port changes — never while the refill is
/// filling (the C++ `w->filling` guard, `window.cpp:1080-1081`).
pub fn profile_for_port(label: &str) -> (u8, u8) {
    let (map, reset) = infer_device_profile(label);
    (map.clamp(0, 2) as u8, reset.clamp(0, 4) as u8)
}

/// Test-note plan (`window.cpp:1130-1156`): stop the player, commit the
/// SoundFont entry verbatim (even empty — C++ assigns unconditionally),
/// reselect the output (unconditionally, like C++ `connectOutput` — the test
/// path does NOT use the conditional T12 planner; its job is to audition the
/// live selection), then Middle C + the 400 ms note-off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestNotePlan {
    /// Always true: the test path stops first via `connectOutput`.
    pub stop_first: bool,
    /// SoundFont entry text, committed verbatim (empty included).
    pub soundfont: String,
    /// Backend + port id to select.
    pub select_output: (String, String),
    /// `(ch, note, vel)` for the test NoteOn.
    pub note_on: (u8, u8, u8),
    /// Delay before the matching NoteOff.
    pub off_delay_ms: u64,
}

/// Build the test-note plan from the dialog's current selections.
pub fn plan_test_note(backend: &str, port_id: &str, soundfont_entry: &str) -> TestNotePlan {
    TestNotePlan {
        stop_first: true,
        soundfont: soundfont_entry.to_string(),
        select_output: (backend.to_string(), port_id.to_string()),
        note_on: (TEST_NOTE_CH, TEST_NOTE_PITCH, TEST_NOTE_VEL),
        off_delay_ms: TEST_NOTE_OFF_DELAY_MS,
    }
}

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

impl OutputReseat {
    /// Stop-player-first (T13a, T12 devil nit): any output-affecting apply
    /// stops the player BEFORE the reselect, like C++ `connectOutput`
    /// (`window.cpp:328-331`: `m_player.stop()` is the first statement).
    ///
    /// Verified C++ stop semantics (`player.cpp:107-116`): full STOP, not
    /// pause — `m_paused` is cleared, the thread joins, `shutupSound()` runs;
    /// position is KEPT (no `resetPosition`); NOTHING resumes afterwards
    /// (no `play()` anywhere on the path). The Rust twin is
    /// `EngineCommand::Stop { reset: false }` with no following `Play`.
    ///
    /// C++ Apply stops unconditionally (it always calls `connectOutput`);
    /// the port stops only when the plan actually sends something — a
    /// no-change Apply must not interrupt playback (deliberate
    /// improvement, pinned by test).
    pub fn stop_first(&self) -> bool {
        self.select_output || self.sound_font.is_some()
    }
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

    #[test]
    fn stop_first_only_when_plan_sends() {
        // Reselect → stop (C++ connectOutput parity).
        let plan = OutputReseat {
            select_output: true,
            sound_font: None,
        };
        assert!(plan.stop_first());
        // SoundFont-only send → stop (C++ Apply always stopped, even for a
        // font-only change, because it always called connectOutput).
        let plan = OutputReseat {
            select_output: false,
            sound_font: Some("/sf/gm.sf2".to_string()),
        };
        assert!(plan.stop_first());
        // No-op plan → NO stop: a no-change Apply must not interrupt
        // playback (deliberate improvement over C++'s unconditional stop).
        let plan = OutputReseat {
            select_output: false,
            sound_font: None,
        };
        assert!(!plan.stop_first());
    }

    #[test]
    fn d3_field_order_and_strings_match_cpp() {
        assert_eq!(
            MIDI_SETUP_FIELDS,
            [
                "Output backend",
                "Show all ports",
                "Connection (your MIDI synth / Munt / USB port)",
                "Instrument map",
                "System Exclusive reset (sent before each song)",
                "FluidSynth SoundFont (.sf2 / .sf3)",
                "Status",
                "Test note / Apply",
            ]
        );
        assert_eq!(REFRESH_PORTS_LABEL, "Refresh ports");
        assert_eq!(SOUNDFONT_BROWSE_LABEL, "Browse…");
        assert_eq!(TEST_NOTE_LABEL, "Test note");
        assert_eq!(APPLY_LABEL, "Apply");
        assert_eq!(
            SOUNDFONT_PLACEHOLDER,
            "Optional: your GM, MT-32, or SC-55 SoundFont"
        );
        assert_eq!(
            NO_DESTINATIONS_HINT,
            "No destinations. Start mt32emu-qt (Munt) or Nuked-SC55, then Refresh."
        );
        assert_eq!(TEST_NOTE_STATUS, "Sent Middle C on channel 1");
        assert_eq!(NO_MIDI_OUTPUT, "No MIDI output");
        assert_eq!(UNABLE_TO_OPEN_OUTPUT, "Unable to open MIDI output");
    }

    fn test_ports() -> Vec<MidiPort> {
        vec![
            MidiPort {
                id: "128:0".to_string(),
                label: "Munt MT-32:Standard".to_string(),
            },
            MidiPort {
                id: "129:0".to_string(),
                label: "Virtual SC55".to_string(),
            },
            MidiPort {
                id: "fluidsynth".to_string(),
                label: "FluidSynth: gm.sf2".to_string(),
            },
        ]
    }

    #[test]
    fn refill_want_prefers_live_current_port() {
        // Refilling the current backend → live current port wins over saved.
        assert_eq!(refill_want("128:0", true, "129:0"), "129:0");
        // Another backend → saved connection.
        assert_eq!(refill_want("128:0", false, "129:0"), "128:0");
        // Empty either way stays empty (preselect then yields 0).
        assert_eq!(refill_want("", false, ""), "");
    }

    #[test]
    fn refill_preselect_id_substring_and_miss() {
        let ports = test_ports();
        // Exact id match…
        assert_eq!(select_refilled_port(&ports, "129:0"), 1);
        // …label-substring match…
        assert_eq!(select_refilled_port(&ports, "SC55"), 1);
        assert_eq!(select_refilled_port(&ports, "Munt"), 0);
        // …miss → 0 (C++ sel default)…
        assert_eq!(select_refilled_port(&ports, "nope"), 0);
        // …empty want → 0 (C++ !want.empty() guard)…
        assert_eq!(select_refilled_port(&ports, ""), 0);
        // …and empty list → 0.
        assert_eq!(select_refilled_port(&[], "128:0"), 0);
        // The id arm alone: "128:0" is port 0's id and a substring of no
        // label; "fluidsynth" is port 2's id (its label starts `Fluid…`,
        // case-sensitive, so the substring arm alone would miss).
        assert_eq!(select_refilled_port(&ports, "128:0"), 0);
        assert_eq!(select_refilled_port(&ports, "fluidsynth"), 2);
    }

    /// T13c reconciliation pin: the C++ loop overwrites `sel` per match
    /// (`window.cpp:1047-1051`), so the LAST id/substring match wins — not
    /// the first (the T13a `.position()` read lost to T13b's C++ truth).
    #[test]
    fn refill_preselect_last_match_wins() {
        let ports = vec![
            MidiPort {
                id: "128:0".to_string(),
                label: "MT-32:Standard".to_string(),
            },
            MidiPort {
                id: "128:1".to_string(),
                label: "MT-32:GM Emulation".to_string(),
            },
            MidiPort {
                id: "130:0".to_string(),
                label: "Virtual SC55".to_string(),
            },
        ];
        // "MT-32" matches ports 0 and 1 → LAST wins.
        assert_eq!(select_refilled_port(&ports, "MT-32"), 1);
        // Single-match and miss shapes are unaffected.
        assert_eq!(select_refilled_port(&ports, "Virtual"), 2);
        assert_eq!(select_refilled_port(&ports, "nope"), 0);
    }

    #[test]
    fn refill_hint_only_when_empty() {
        assert_eq!(refill_hint(true), Some(NO_DESTINATIONS_HINT));
        assert_eq!(refill_hint(false), None);
    }

    #[test]
    fn profile_for_port_sniffs_labels() {
        // Munt/MT-32 → MT-32 map + MT-32 reset; SC-55/Nuked → GS + GS reset.
        assert_eq!(profile_for_port("Munt MT-32:Standard"), (2, 4));
        assert_eq!(profile_for_port("Virtual SC55"), (1, 2));
        // Munt's GM-emulation port keeps GM names/reset despite "munt".
        assert_eq!(profile_for_port("Munt GM emulation"), (0, 1));
        // Unknown labels → GM + GM reset (the infer default).
        assert_eq!(profile_for_port("USB MIDI Cable"), (0, 1));
        assert_eq!(profile_for_port(""), (0, 1));
    }

    #[test]
    fn test_note_plan_stops_reselects_and_plays_middle_c() {
        let plan = plan_test_note("ALSA", "128:0", "/sf/gm.sf2");
        assert!(plan.stop_first);
        assert_eq!(plan.soundfont, "/sf/gm.sf2");
        assert_eq!(
            plan.select_output,
            ("ALSA".to_string(), "128:0".to_string())
        );
        assert_eq!(plan.note_on, (0, 60, 96));
        assert_eq!(plan.off_delay_ms, 400);
        // Empty SoundFont commits verbatim (C++ assigns unconditionally).
        let plan = plan_test_note("FluidSynth", "fluidsynth", "");
        assert!(plan.stop_first);
        assert!(plan.soundfont.is_empty());
        assert_eq!(plan.note_on, (TEST_NOTE_CH, TEST_NOTE_PITCH, TEST_NOTE_VEL));
        assert_eq!(plan.off_delay_ms, TEST_NOTE_OFF_DELAY_MS);
    }
}
