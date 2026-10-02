//! MIDI output backends: trait, Dummy sink, manager — port of the
//! backend-agnostic parts of `src/midi/output.{hpp,cpp}`.
//!
//! PLAN T5 (trait + Dummy + manager) and T6 (ALSA/FluidSynth backends in
//! `output_alsa.rs`/`output_fluid.rs`, same [`MidiOutput`] trait); arch §1.5
//! and §3.3. One deliberate change from C++ (arch §3.3, A15): `open` returns
//! `Result<(), OutputError>` carrying the former `lastError` text instead of
//! `bool` + `lastError()`.
//!
//! Dependency rule (arch §5): `std` only — no iced/libcosmic in `core`.

use std::error::Error;
use std::fmt::{self, Display};

#[cfg(target_os = "linux")]
use super::output_alsa::AlsaOutput;
use super::output_fluid::FluidOutput;
#[cfg(not(target_os = "linux"))]
use crate::platform::midi::NativeMidiOutput;

/// An output port: `client:port` id plus the human-readable label shown in
/// MIDI Setup (C++ `MidiPort`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiPort {
    pub id: String,
    pub label: String,
}

/// Typed `open` failure carrying the former `lastError` text (A15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputError {
    pub message: String,
}

impl OutputError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for OutputError {}

/// Output backend (C++ `MidiOutput`): backend name, port enumeration,
/// failable open/close, and the 8 channel/SysEx send functions.
pub trait MidiOutput {
    fn backend_name(&self) -> &str;
    fn ports(&self, advanced: bool) -> Vec<MidiPort>;
    fn open(&mut self, port_id: &str) -> Result<(), OutputError>;
    fn close(&mut self);
    fn current_port(&self) -> &str;
    /// SoundFont path passthrough; only the T6 FluidSynth backend acts on
    /// it (C++ `FluidOutput::setSoundFont`).
    fn set_sound_font(&mut self, _path: &str) {}

    fn send_note_on(&mut self, ch: i32, note: i32, vel: i32);
    fn send_note_off(&mut self, ch: i32, note: i32, vel: i32);
    fn send_key_pressure(&mut self, ch: i32, note: i32, vel: i32);
    fn send_controller(&mut self, ch: i32, cc: i32, val: i32);
    fn send_program(&mut self, ch: i32, pgm: i32);
    fn send_channel_pressure(&mut self, ch: i32, val: i32);
    /// Pitch bend in -8192..=8191 (C++ `sendPitchBend`).
    fn send_pitch_bend(&mut self, ch: i32, value: i32);
    fn send_sysex(&mut self, data: &[u8]);
}

/// One recorded [`DummyOutput`] call, for engine/UI tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DummyCall {
    Open(String),
    Close,
    NoteOn { ch: i32, note: i32, vel: i32 },
    NoteOff { ch: i32, note: i32, vel: i32 },
    KeyPressure { ch: i32, note: i32, vel: i32 },
    Controller { ch: i32, cc: i32, val: i32 },
    Program { ch: i32, pgm: i32 },
    ChannelPressure { ch: i32, val: i32 },
    PitchBend { ch: i32, value: i32 },
    SysEx(Vec<u8>),
}

/// Silent sink, always available (C++ `DummyOutput`). Records every call so
/// tests can observe engine output and manager select semantics.
#[derive(Debug, Default)]
pub struct DummyOutput {
    current: String,
    calls: Vec<DummyCall>,
}

impl DummyOutput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> &[DummyCall] {
        &self.calls
    }

    pub fn take_calls(&mut self) -> Vec<DummyCall> {
        std::mem::take(&mut self.calls)
    }
}

impl MidiOutput for DummyOutput {
    fn backend_name(&self) -> &str {
        "Dummy"
    }

    fn ports(&self, _advanced: bool) -> Vec<MidiPort> {
        vec![MidiPort {
            id: "dummy".to_string(),
            label: "No MIDI output".to_string(),
        }]
    }

    fn open(&mut self, port_id: &str) -> Result<(), OutputError> {
        self.calls.push(DummyCall::Open(port_id.to_string()));
        self.current = if port_id.is_empty() {
            "dummy".to_string()
        } else {
            port_id.to_string()
        };
        Ok(())
    }

    fn close(&mut self) {
        self.calls.push(DummyCall::Close);
        self.current.clear();
    }

    fn current_port(&self) -> &str {
        &self.current
    }

    fn send_note_on(&mut self, ch: i32, note: i32, vel: i32) {
        self.calls.push(DummyCall::NoteOn { ch, note, vel });
    }

    fn send_note_off(&mut self, ch: i32, note: i32, vel: i32) {
        self.calls.push(DummyCall::NoteOff { ch, note, vel });
    }

    fn send_key_pressure(&mut self, ch: i32, note: i32, vel: i32) {
        self.calls.push(DummyCall::KeyPressure { ch, note, vel });
    }

    fn send_controller(&mut self, ch: i32, cc: i32, val: i32) {
        self.calls.push(DummyCall::Controller { ch, cc, val });
    }

    fn send_program(&mut self, ch: i32, pgm: i32) {
        self.calls.push(DummyCall::Program { ch, pgm });
    }

    fn send_channel_pressure(&mut self, ch: i32, val: i32) {
        self.calls.push(DummyCall::ChannelPressure { ch, val });
    }

    fn send_pitch_bend(&mut self, ch: i32, value: i32) {
        self.calls.push(DummyCall::PitchBend { ch, value });
    }

    fn send_sysex(&mut self, data: &[u8]) {
        self.calls.push(DummyCall::SysEx(data.to_vec()));
    }
}

/// Owns the backends and the current selection (C++ `MidiOutputManager`).
///
/// REQUIRED registration order in [`MidiOutputManager::new`]:
/// `[ALSA, FluidSynth, Dummy]` (output.cpp:428-430). The unknown→front rule
/// (`find`, A6c) resolves to index 0, so ALSA must be front for unknown
/// backend names to select ALSA (output.cpp:449). Neither constructor
/// touches hardware or audio, so `new` is infallible on bare CI.
#[derive(Default)]
pub struct MidiOutputManager {
    backends: Vec<Box<dyn MidiOutput>>,
    current: Option<usize>,
}

impl MidiOutputManager {
    pub fn new() -> Self {
        let mut manager = Self {
            backends: Vec::new(),
            current: None,
        };
        #[cfg(target_os = "linux")]
        manager.add_backend(Box::new(AlsaOutput::new()));
        #[cfg(not(target_os = "linux"))]
        manager.add_backend(Box::new(NativeMidiOutput::new()));
        manager.add_backend(Box::new(FluidOutput::new()));
        manager.add_backend(Box::new(DummyOutput::new()));
        manager
    }

    /// Register another backend (kept for tests; the shipped trio is fixed
    /// in `new` above).
    pub fn add_backend(&mut self, backend: Box<dyn MidiOutput>) {
        self.backends.push(backend);
    }

    pub fn backend_names(&self) -> Vec<String> {
        self.backends
            .iter()
            .map(|b| b.backend_name().to_string())
            .collect()
    }

    /// Index of the named backend, or the front backend for unknown names
    /// (A6c); `None` only when no backend is registered at all.
    pub fn find_index(&self, backend: &str) -> Option<usize> {
        if let Some(idx) = self
            .backends
            .iter()
            .position(|b| b.backend_name() == backend)
        {
            return Some(idx);
        }
        if self.backends.is_empty() {
            None
        } else {
            Some(0)
        }
    }

    /// The named backend, or the front backend for unknown names (A6c).
    pub fn find(&self, backend: &str) -> Option<&dyn MidiOutput> {
        self.find_index(backend).map(|idx| &*self.backends[idx])
    }

    pub fn current(&self) -> Option<&dyn MidiOutput> {
        self.current.map(|idx| &*self.backends[idx])
    }

    pub fn current_mut(&mut self) -> Option<&mut dyn MidiOutput> {
        match self.current {
            Some(idx) => Some(&mut *self.backends[idx]),
            None => None,
        }
    }

    /// Select a backend + port, closing the previous backend first when it
    /// differs (output.cpp:452-461). Unknown backend names resolve to the
    /// front backend, so only `open(port_id)` can fail.
    pub fn select(&mut self, backend: &str, port_id: &str) -> Result<(), OutputError> {
        let idx = self
            .find_index(backend)
            .ok_or_else(|| OutputError::new("No MIDI backends registered"))?;
        if let Some(cur) = self.current
            && cur != idx
        {
            self.backends[cur].close();
        }
        self.current = Some(idx);
        self.backends[idx].open(port_id)
    }

    /// Push a SoundFont path to every backend; only FluidSynth acts on it.
    pub fn set_sound_font(&mut self, path: &str) {
        for backend in &mut self.backends {
            backend.set_sound_font(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Second selectable backend so select-switch tests can observe
    /// close-previous-first across backends. The log is shared with the test
    /// so open/close calls stay visible behind the `dyn MidiOutput` handle.
    struct ProbeBackend {
        name: String,
        current: String,
        log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    }

    impl ProbeBackend {
        fn named(name: &str, log: std::rc::Rc<std::cell::RefCell<Vec<String>>>) -> Self {
            Self {
                name: name.to_string(),
                current: String::new(),
                log,
            }
        }
    }

    impl MidiOutput for ProbeBackend {
        fn backend_name(&self) -> &str {
            &self.name
        }

        fn ports(&self, _advanced: bool) -> Vec<MidiPort> {
            vec![MidiPort {
                id: "probe:0".to_string(),
                label: "Probe".to_string(),
            }]
        }

        fn open(&mut self, port_id: &str) -> Result<(), OutputError> {
            self.log.borrow_mut().push(format!("open:{port_id}"));
            self.current = port_id.to_string();
            Ok(())
        }

        fn close(&mut self) {
            self.log.borrow_mut().push("close".to_string());
            self.current.clear();
        }

        fn current_port(&self) -> &str {
            &self.current
        }

        fn send_note_on(&mut self, _ch: i32, _note: i32, _vel: i32) {}
        fn send_note_off(&mut self, _ch: i32, _note: i32, _vel: i32) {}
        fn send_key_pressure(&mut self, _ch: i32, _note: i32, _vel: i32) {}
        fn send_controller(&mut self, _ch: i32, _cc: i32, _val: i32) {}
        fn send_program(&mut self, _ch: i32, _pgm: i32) {}
        fn send_channel_pressure(&mut self, _ch: i32, _val: i32) {}
        fn send_pitch_bend(&mut self, _ch: i32, _value: i32) {}
        fn send_sysex(&mut self, _data: &[u8]) {}
    }

    #[test]
    fn dummy_is_always_available() {
        let dummy = DummyOutput::new();
        assert_eq!(dummy.backend_name(), "Dummy");
        let ports = dummy.ports(false);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].id, "dummy");
        assert_eq!(ports[0].label, "No MIDI output");
        assert_eq!(dummy.ports(true), ports);
        assert_eq!(dummy.current_port(), "");
    }

    #[test]
    fn dummy_open_close_ok() {
        let mut dummy = DummyOutput::new();
        assert!(dummy.open("dummy").is_ok());
        assert_eq!(dummy.current_port(), "dummy");
        dummy.close();
        assert_eq!(dummy.current_port(), "");
        // Empty id falls back to "dummy" like the C++.
        assert!(dummy.open("").is_ok());
        assert_eq!(dummy.current_port(), "dummy");
        assert_eq!(
            dummy.calls(),
            &[
                DummyCall::Open("dummy".to_string()),
                DummyCall::Close,
                DummyCall::Open(String::new()),
            ]
        );
        assert_eq!(dummy.take_calls().len(), 3);
        assert!(dummy.calls().is_empty());
    }

    #[test]
    fn dummy_records_all_eight_sends() {
        let mut dummy = DummyOutput::new();
        dummy.open("dummy").unwrap();
        dummy.send_note_on(0, 60, 80);
        dummy.send_note_off(1, 61, 64);
        dummy.send_key_pressure(2, 62, 70);
        dummy.send_controller(3, 7, 100);
        dummy.send_program(4, 40);
        dummy.send_channel_pressure(5, 90);
        dummy.send_pitch_bend(6, -100);
        dummy.send_sysex(&[0xF0, 0x7E, 0xF7]);
        assert_eq!(
            dummy.calls(),
            &[
                DummyCall::Open("dummy".to_string()),
                DummyCall::NoteOn {
                    ch: 0,
                    note: 60,
                    vel: 80
                },
                DummyCall::NoteOff {
                    ch: 1,
                    note: 61,
                    vel: 64
                },
                DummyCall::KeyPressure {
                    ch: 2,
                    note: 62,
                    vel: 70
                },
                DummyCall::Controller {
                    ch: 3,
                    cc: 7,
                    val: 100
                },
                DummyCall::Program { ch: 4, pgm: 40 },
                DummyCall::ChannelPressure { ch: 5, val: 90 },
                DummyCall::PitchBend { ch: 6, value: -100 },
                DummyCall::SysEx(vec![0xF0, 0x7E, 0xF7]),
            ]
        );
    }

    #[test]
    fn error_carries_message() {
        let err = OutputError::new("No ALSA MIDI destinations");
        assert_eq!(err.to_string(), "No ALSA MIDI destinations");
        let err: OutputError = OutputError::new(String::from("owned"));
        assert_eq!(err.message, "owned");
    }

    #[test]
    fn manager_registers_required_trio_in_order() {
        // REQUIRED order [ALSA, FluidSynth, Dummy] (output.cpp:428-430);
        // construction touches no hardware, so this is bare-CI safe.
        let manager = MidiOutputManager::new();
        assert_eq!(
            manager.backend_names(),
            vec![
                if cfg!(target_os = "linux") {
                    "ALSA"
                } else {
                    "MIDI"
                }
                .to_string(),
                "FluidSynth".to_string(),
                "Dummy".to_string()
            ]
        );
        assert!(manager.current().is_none());
    }

    #[test]
    fn manager_find_unknown_resolves_to_front() {
        let manager = MidiOutputManager::new();
        let native = if cfg!(target_os = "linux") {
            "ALSA"
        } else {
            "MIDI"
        };
        assert_eq!(manager.find(native).unwrap().backend_name(), native);
        assert_eq!(
            manager.find("FluidSynth").unwrap().backend_name(),
            "FluidSynth"
        );
        assert_eq!(manager.find("Dummy").unwrap().backend_name(), "Dummy");
        // Unknown → front backend = ALSA (output.cpp:449, A6c).
        assert_eq!(manager.find("Nope").unwrap().backend_name(), native);
        assert_eq!(manager.find_index("Nope"), Some(0));
    }

    #[test]
    fn manager_select_opens_and_tracks_current() {
        let mut manager = MidiOutputManager::new();
        assert!(manager.select("Dummy", "dummy").is_ok());
        let current = manager.current().unwrap();
        assert_eq!(current.backend_name(), "Dummy");
        assert_eq!(current.current_port(), "dummy");
        // Unknown backend still selects the front backend (ALSA since T6).
        // `open` may fail without hardware, but the selection sticks either
        // way (C++ assigns m_current before open, output.cpp:457-460).
        let _ = manager.select("Nope", "14:0");
        assert_eq!(
            manager.current().unwrap().backend_name(),
            if cfg!(target_os = "linux") {
                "ALSA"
            } else {
                "MIDI"
            }
        );
    }

    #[test]
    fn manager_falls_back_to_dummy_without_hardware() {
        // Arch §3.3: no SoundFont + no ALSA sequencer → Dummy still selects
        // cleanly (bare-CI safe: Dummy never touches hardware or audio).
        let mut manager = MidiOutputManager::new();
        manager.set_sound_font("/nonexistent/t6-probe-missing.sf2");
        assert!(manager.select("Dummy", "dummy").is_ok());
        assert_eq!(manager.current().unwrap().backend_name(), "Dummy");
    }

    #[test]
    fn manager_select_closes_previous_first() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut manager = MidiOutputManager::new();
        manager.add_backend(Box::new(ProbeBackend::named("Probe", Rc::clone(&log))));
        manager.select("Dummy", "dummy").unwrap();
        manager.select("Probe", "probe:0").unwrap();
        // Switching away closed the Dummy: its port cleared.
        assert_eq!(manager.find("Dummy").unwrap().current_port(), "");
        assert_eq!(*log.borrow(), vec!["open:probe:0".to_string()]);
        // Re-selecting the same backend re-opens but never closes it (C++
        // m_current identity check).
        manager.select("Probe", "probe:0").unwrap();
        assert_eq!(
            *log.borrow(),
            vec!["open:probe:0".to_string(), "open:probe:0".to_string()]
        );
        assert_eq!(manager.current().unwrap().current_port(), "probe:0");
        // Switching back closes the Probe first, then re-opens the Dummy.
        manager.select("Dummy", "dummy").unwrap();
        assert_eq!(
            *log.borrow(),
            vec![
                "open:probe:0".to_string(),
                "open:probe:0".to_string(),
                "close".to_string(),
            ]
        );
        assert_eq!(manager.current().unwrap().current_port(), "dummy");
        assert_eq!(manager.find("Probe").unwrap().current_port(), "");
    }

    #[test]
    fn manager_set_sound_font_is_passthrough_noop_for_dummy() {
        let mut manager = MidiOutputManager::new();
        manager.set_sound_font("/sound/fonts/gm.sf2");
        assert!(manager.select("Dummy", "dummy").is_ok());
    }

    #[test]
    fn manager_select_with_no_backends_errors() {
        let mut manager = MidiOutputManager::default();
        assert!(manager.find("Dummy").is_none());
        let err = manager.select("Dummy", "dummy").unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
