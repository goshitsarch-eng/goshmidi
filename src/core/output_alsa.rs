//! ALSA sequencer backend — port of `AlsaOutput` (`src/midi/output.cpp:62-274`).
//!
//! PLAN T6, arch §1.5 and §3.3, A6. Sequencer `default`, client name
//! `dmidiplayer`, one `MIDI Out` port (`READ|SUBS_READ`). The port filter
//! (A6a), the `client:port` parse and the advanced-list fallback (A6b) are
//! pure functions so they are unit-tested without hardware; the unknown →
//! front (ALSA) rule (A6c) lives in [`MidiOutputManager`](super::output::MidiOutputManager).
//!
//! A missing sequencer (`Seq::open` fails, e.g. bare CI) degrades to empty
//! port lists and `Err` from `open` — never a panic. Constructor failure text
//! matches the C++ (`Unable to open ALSA sequencer`, output.cpp:67).
//!
//! Dependency rule (arch §5): `std` + `alsa` only — no iced/libcosmic in `core`.

use std::ffi::CStr;

use alsa::Direction;
use alsa::seq::{
    Addr, ClientIter, EvCtrl, EvNote, Event, EventType, PortCap, PortIter, PortSubscribe, PortType,
    Seq,
};

use super::event::clamp_midi;
use super::output::{MidiOutput, MidiPort, OutputError};

/// ALSA client name (output.cpp:71).
pub const ALSA_CLIENT_NAME: &CStr = c"dmidiplayer";
/// Our own sequencer port (output.cpp:72-75).
pub const ALSA_PORT_NAME: &CStr = c"MIDI Out";

/// MIDI-ish port-type mask, output.cpp:113-118 — note `PORT` is included.
/// `SYNTHESIZER` is `(1<<18)` in current ALSA headers (and the `alsa` crate);
/// the C++ `#ifndef` fallback to `(1<<10)` never triggers there, so both
/// masks agree bit-for-bit.
pub fn midiish_types() -> PortType {
    PortType::MIDI_GENERIC
        | PortType::SYNTH
        | PortType::MIDI_GM
        | PortType::MIDI_GS
        | PortType::MIDI_XG
        | PortType::MIDI_MT32
        | PortType::MIDI_GM2
        | PortType::APPLICATION
        | PortType::HARDWARE
        | PortType::PORT
        | PortType::SOFTWARE
        | PortType::SYNTHESIZER
}

/// Pure port-visibility predicate (output.cpp:108-120): requires
/// `WRITE|SUBS_WRITE`, skips `NO_EXPORT`; in non-advanced mode a port must
/// additionally carry a MIDI-ish type bit — except `type == 0` ports, which
/// pass through unfiltered (output.cpp:119).
pub fn port_visible(caps: PortCap, ty: PortType, advanced: bool) -> bool {
    if !caps.contains(PortCap::WRITE) || !caps.contains(PortCap::SUBS_WRITE) {
        return false;
    }
    if caps.contains(PortCap::NO_EXPORT) {
        return false;
    }
    if !advanced && !ty.is_empty() && (ty & midiish_types()).is_empty() {
        return false;
    }
    true
}

/// Pure `client:port` parse (output.cpp:138 `sscanf "%d:%d"`). Accepts exactly
/// two `i32` fields; surrounding whitespace per field is tolerated (sscanf
/// `%d` skips leading whitespace, so this is marginally more lenient on
/// trailing space — a benign config-file robustness widening).
pub fn parse_port_id(id: &str) -> Option<(i32, i32)> {
    let (client, port) = id.split_once(':')?;
    if port.contains(':') {
        return None;
    }
    Some((client.trim().parse().ok()?, port.trim().parse().ok()?))
}

/// ALSA output backend. `seq` is `None` when the sequencer would not open;
/// all methods degrade gracefully in that state (C++ `m_seq == nullptr`).
pub struct AlsaOutput {
    seq: Option<Seq>,
    port: i32,
    dest: Option<Addr>,
    current: String,
}

impl AlsaOutput {
    pub fn new() -> Self {
        let mut out = Self {
            seq: None,
            port: -1,
            dest: None,
            current: String::new(),
        };
        let seq = match Seq::open(None, Some(Direction::Playback), false) {
            Ok(seq) => seq,
            Err(_) => return out,
        };
        let _ = seq.set_client_name(ALSA_CLIENT_NAME);
        out.port = seq
            .create_simple_port(
                ALSA_PORT_NAME,
                PortCap::READ | PortCap::SUBS_READ,
                PortType::MIDI_GENERIC | PortType::APPLICATION,
            )
            .unwrap_or(-1);
        out.seq = Some(seq);
        out
    }

    /// `true` when the sequencer opened (unit-test seam for the degraded path).
    #[cfg(test)]
    fn has_sequencer(&self) -> bool {
        self.seq.is_some()
    }

    /// Fixed-length channel-voice event send (`output_direct` equivalent).
    fn event(&self, ty: EventType, ch: i32, a: i32, b: i32) {
        let Some(seq) = &self.seq else { return };
        let ch = (ch & 0x0f) as u8;
        let mut ev = match ty {
            EventType::Noteon | EventType::Noteoff | EventType::Keypress => Event::new(
                ty,
                &EvNote {
                    channel: ch,
                    note: clamp_midi(a) as u8,
                    velocity: clamp_midi(b) as u8,
                    ..EvNote::default()
                },
            ),
            EventType::Controller => Event::new(
                ty,
                &EvCtrl {
                    channel: ch,
                    param: a as u32,
                    value: clamp_midi(b),
                },
            ),
            EventType::Pgmchange | EventType::Chanpress => Event::new(
                ty,
                &EvCtrl {
                    channel: ch,
                    value: clamp_midi(a),
                    ..EvCtrl::default()
                },
            ),
            EventType::Pitchbend => Event::new(
                ty,
                &EvCtrl {
                    channel: ch,
                    value: a,
                    ..EvCtrl::default()
                },
            ),
            _ => return,
        };
        ev.set_source(self.port);
        match self.dest {
            Some(dest) => ev.set_dest(dest),
            None => ev.set_subs(),
        }
        ev.set_direct();
        let _ = seq.event_output_direct(&mut ev);
    }
}

impl Default for AlsaOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AlsaOutput {
    fn drop(&mut self) {
        self.close();
    }
}

impl MidiOutput for AlsaOutput {
    fn backend_name(&self) -> &str {
        "ALSA"
    }

    fn ports(&self, advanced: bool) -> Vec<MidiPort> {
        let mut list = Vec::new();
        let Some(seq) = &self.seq else {
            return list;
        };
        let own = seq.client_id().unwrap_or(-1);
        for client in ClientIter::new(seq) {
            let id = client.get_client();
            if id == own || id == 0 {
                continue;
            }
            let client_name = client.get_name().unwrap_or("?");
            for port in PortIter::new(seq, id) {
                if !port_visible(port.get_capability(), port.get_type(), advanced) {
                    continue;
                }
                let port_id = format!("{}:{}", port.get_client(), port.get_port());
                let port_name = port.get_name().unwrap_or("?");
                list.push(MidiPort {
                    id: port_id.clone(),
                    label: format!("{client_name}:{port_name} ({port_id})"),
                });
            }
        }
        list
    }

    fn open(&mut self, port_id: &str) -> Result<(), OutputError> {
        self.close();
        if self.seq.is_none() {
            return Err(OutputError::new("Unable to open ALSA sequencer"));
        }
        // Unparsable id falls back to the first port of the *advanced*
        // listing (output.cpp:138-145, A6b).
        let (client, port) = match parse_port_id(port_id) {
            Some(pair) => {
                self.current = port_id.to_string();
                pair
            }
            None => {
                let list = self.ports(true);
                match list.first() {
                    Some(first) => {
                        let pair = parse_port_id(&first.id)
                            .ok_or_else(|| OutputError::new("No ALSA MIDI destinations"))?;
                        self.current = first.id.clone();
                        pair
                    }
                    None => return Err(OutputError::new("No ALSA MIDI destinations")),
                }
            }
        };
        let seq = self.seq.as_ref().expect("sequencer checked above");
        let own = seq.client_id().unwrap_or(0);
        let sub =
            PortSubscribe::empty().map_err(|_| OutputError::new("Unable to connect ALSA port"))?;
        sub.set_sender(Addr {
            client: own,
            port: self.port,
        });
        sub.set_dest(Addr { client, port });
        if seq.subscribe_port(&sub).is_err() {
            self.current.clear();
            return Err(OutputError::new("Unable to connect ALSA port"));
        }
        self.dest = Some(Addr { client, port });
        Ok(())
    }

    fn close(&mut self) {
        if let (Some(seq), Some(dest)) = (self.seq.as_ref(), self.dest) {
            let own = seq.client_id().unwrap_or(0);
            let _ = seq.unsubscribe_port(
                Addr {
                    client: own,
                    port: self.port,
                },
                dest,
            );
        }
        self.dest = None;
        self.current.clear();
    }

    fn current_port(&self) -> &str {
        &self.current
    }

    fn send_note_on(&mut self, ch: i32, note: i32, vel: i32) {
        self.event(EventType::Noteon, ch, note, vel);
    }

    fn send_note_off(&mut self, ch: i32, note: i32, vel: i32) {
        self.event(EventType::Noteoff, ch, note, vel);
    }

    fn send_key_pressure(&mut self, ch: i32, note: i32, vel: i32) {
        self.event(EventType::Keypress, ch, note, vel);
    }

    fn send_controller(&mut self, ch: i32, cc: i32, val: i32) {
        self.event(EventType::Controller, ch, cc, val);
    }

    fn send_program(&mut self, ch: i32, pgm: i32) {
        self.event(EventType::Pgmchange, ch, pgm, 0);
    }

    fn send_channel_pressure(&mut self, ch: i32, val: i32) {
        self.event(EventType::Chanpress, ch, val, 0);
    }

    fn send_pitch_bend(&mut self, ch: i32, value: i32) {
        self.event(EventType::Pitchbend, ch, value, 0);
    }

    fn send_sysex(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let Some(seq) = &self.seq else { return };
        // `Cow::Owned` keeps the bytes alive for the call — the `alsa`-crate
        // equivalent of the C++ `m_sysexScratch` member (output.cpp:213-220).
        let mut ev = Event::new_ext(EventType::Sysex, data.to_vec());
        ev.set_source(self.port);
        match self.dest {
            Some(dest) => ev.set_dest(dest),
            None => ev.set_subs(),
        }
        ev.set_direct();
        let _ = seq.event_output_direct(&mut ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(caps: PortCap, ty: PortType, advanced: bool) -> bool {
        port_visible(caps, ty, advanced)
    }

    #[test]
    fn filter_requires_write_and_subs_write() {
        let midi = PortType::MIDI_GENERIC;
        assert!(visible(PortCap::WRITE | PortCap::SUBS_WRITE, midi, false));
        assert!(!visible(PortCap::WRITE, midi, false));
        assert!(!visible(PortCap::SUBS_WRITE, midi, false));
        assert!(!visible(PortCap::READ | PortCap::SUBS_READ, midi, false));
        // Advanced mode drops the type filter but never the caps gate.
        assert!(visible(PortCap::WRITE | PortCap::SUBS_WRITE, midi, true));
        assert!(!visible(PortCap::WRITE, midi, true));
        assert!(visible(
            PortCap::WRITE | PortCap::SUBS_WRITE,
            PortType::empty(),
            true
        ));
        assert!(!visible(PortCap::empty(), PortType::empty(), true));
    }

    #[test]
    fn filter_skips_no_export() {
        assert!(!visible(
            PortCap::WRITE | PortCap::SUBS_WRITE | PortCap::NO_EXPORT,
            PortType::MIDI_GENERIC,
            false
        ));
        assert!(!visible(
            PortCap::WRITE | PortCap::SUBS_WRITE | PortCap::NO_EXPORT,
            PortType::MIDI_GENERIC,
            true
        ));
    }

    #[test]
    fn filter_non_advanced_type_mask_exact() {
        let caps = PortCap::WRITE | PortCap::SUBS_WRITE;
        // Every mask member passes in non-advanced mode (output.cpp:113-118).
        for ty in [
            PortType::MIDI_GENERIC,
            PortType::SYNTH,
            PortType::MIDI_GM,
            PortType::MIDI_GS,
            PortType::MIDI_XG,
            PortType::MIDI_MT32,
            PortType::MIDI_GM2,
            PortType::APPLICATION,
            PortType::HARDWARE,
            PortType::PORT,
            PortType::SOFTWARE,
            PortType::SYNTHESIZER,
        ] {
            assert!(visible(caps, ty, false), "mask member {ty:?} filtered out");
        }
        // Non-mask types are hidden — incl. SPECIFIC, DIRECT_SAMPLE, SAMPLE.
        for ty in [
            PortType::SPECIFIC,
            PortType::DIRECT_SAMPLE,
            PortType::SAMPLE,
            PortType::SPECIFIC | PortType::SAMPLE,
        ] {
            assert!(!visible(caps, ty, false), "non-mask {ty:?} leaked through");
        }
        // But all of them show in advanced mode.
        for ty in [
            PortType::SPECIFIC,
            PortType::DIRECT_SAMPLE,
            PortType::SAMPLE,
            PortType::MIDI_GENERIC,
        ] {
            assert!(visible(caps, ty, true), "advanced hid {ty:?}");
        }
    }

    #[test]
    fn filter_type_zero_passthrough() {
        let caps = PortCap::WRITE | PortCap::SUBS_WRITE;
        // `type == 0` passes unfiltered in BOTH modes (output.cpp:119).
        assert!(visible(caps, PortType::empty(), false));
        assert!(visible(caps, PortType::empty(), true));
    }

    #[test]
    fn mask_matches_cpp_bit_for_bit() {
        // C++: (1<<1)|(1<<2)|(1<<3)|(1<<4)|(1<<5)|(1<<6)|(1<<10)|(1<<16)|
        //      (1<<17)|(1<<18)|(1<<19)|(1<<20) with SYNTHESIZER=(1<<18).
        let expected: u32 = (1 << 1)
            | (1 << 2)
            | (1 << 3)
            | (1 << 4)
            | (1 << 5)
            | (1 << 6)
            | (1 << 10)
            | (1 << 16)
            | (1 << 17)
            | (1 << 18)
            | (1 << 19)
            | (1 << 20);
        assert_eq!(midiish_types().bits(), expected);
    }

    #[test]
    fn parse_port_id_matrix() {
        assert_eq!(parse_port_id("14:0"), Some((14, 0)));
        assert_eq!(parse_port_id("128:4"), Some((128, 4)));
        assert_eq!(parse_port_id("0:0"), Some((0, 0)));
        // Garbage → None → triggers the A6b advanced-list fallback.
        assert_eq!(parse_port_id(""), None);
        assert_eq!(parse_port_id("fluidsynth"), None);
        assert_eq!(parse_port_id("dummy"), None);
        assert_eq!(parse_port_id("14"), None);
        assert_eq!(parse_port_id("14:"), None);
        assert_eq!(parse_port_id(":0"), None);
        assert_eq!(parse_port_id("14:0:1"), None);
        assert_eq!(parse_port_id("a:b"), None);
        assert_eq!(parse_port_id("14:x"), None);
        // sscanf-%d-compatible: negatives and surrounding space parse.
        assert_eq!(parse_port_id("-1:0"), Some((-1, 0)));
        assert_eq!(parse_port_id(" 14 : 0 "), Some((14, 0)));
    }

    /// Degraded backend (no sequencer) without touching hardware: direct
    /// struct construction forces the `None` state deterministically, so this
    /// is green on bare CI and on machines with ALSA alike.
    fn degraded() -> AlsaOutput {
        AlsaOutput {
            seq: None,
            port: -1,
            dest: None,
            current: String::new(),
        }
    }

    #[test]
    fn degraded_ports_empty_and_sends_noop() {
        let mut out = degraded();
        assert!(!out.has_sequencer());
        assert!(out.ports(false).is_empty());
        assert!(out.ports(true).is_empty());
        assert_eq!(out.current_port(), "");
        // None of these may panic without a sequencer (C++ early-returns).
        out.send_note_on(0, 60, 80);
        out.send_note_off(0, 60, 64);
        out.send_key_pressure(0, 60, 70);
        out.send_controller(0, 7, 100);
        out.send_program(0, 40);
        out.send_channel_pressure(0, 90);
        out.send_pitch_bend(0, -100);
        out.send_sysex(&[0xF0, 0x7E, 0xF7]);
        out.send_sysex(&[]);
        out.close();
        assert_eq!(out.current_port(), "");
    }

    #[test]
    fn degraded_open_errors() {
        let mut out = degraded();
        // Parsable id, no sequencer → constructor-failure text (output.cpp:67).
        let err = out.open("14:0").unwrap_err();
        assert_eq!(err.to_string(), "Unable to open ALSA sequencer");
        assert_eq!(out.current_port(), "");
    }

    #[test]
    fn new_never_panics_and_open_result_is_honest() {
        // Touches the real sequencer when present, degrades without one;
        // asserts only invariants that hold in both states (bare-CI safe).
        let mut out = AlsaOutput::new();
        assert_eq!(out.backend_name(), "ALSA");
        assert_eq!(out.current_port(), "");
        let _ = out.ports(false);
        let _ = out.ports(true);
        // Either Ok (subscribed somewhere real) or a surfaced Err — never a
        // panic, never silent success with a stale port.
        let result = out.open("definitely-not-a-port-id");
        match &result {
            Ok(()) => assert!(!out.current_port().is_empty()),
            Err(err) => {
                assert!(!err.to_string().is_empty());
                assert_eq!(out.current_port(), "");
            }
        }
        out.close();
        assert_eq!(out.current_port(), "");
    }
}
