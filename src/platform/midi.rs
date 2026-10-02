//! Cross-platform native MIDI: CoreMIDI on macOS, WinMM on Windows.
use crate::core::output::{MidiOutput, MidiPort, OutputError};

pub struct NativeMidiOutput {
    connection: Option<midir::MidiOutputConnection>,
    current: String,
}

impl Default for NativeMidiOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeMidiOutput {
    pub fn new() -> Self {
        Self {
            connection: None,
            current: String::new(),
        }
    }
    fn send(&mut self, data: &[u8]) {
        if let Some(connection) = &mut self.connection
            && let Err(error) = connection.send(data)
        {
            tracing::warn!(%error, "MIDI message failed");
        }
    }
    fn channel_message(&mut self, kind: u8, ch: i32, a: i32, b: Option<i32>) {
        if !(0..16).contains(&ch) {
            return;
        }
        let status = kind | ch as u8;
        let a = a.clamp(0, 127) as u8;
        if let Some(b) = b {
            self.send(&[status, a, b.clamp(0, 127) as u8]);
        } else {
            self.send(&[status, a]);
        }
    }
}

impl MidiOutput for NativeMidiOutput {
    fn backend_name(&self) -> &str {
        "MIDI"
    }
    fn ports(&self, _: bool) -> Vec<MidiPort> {
        let Ok(output) = midir::MidiOutput::new("Gosh MIDI Player") else {
            return Vec::new();
        };
        output
            .ports()
            .iter()
            .filter_map(|port| output.port_name(port).ok().map(|name| (port.id(), name)))
            .map(|(id, name)| MidiPort { id, label: name })
            .collect()
    }
    fn open(&mut self, id: &str) -> Result<(), OutputError> {
        self.close();
        let output = midir::MidiOutput::new("Gosh MIDI Player")
            .map_err(|error| OutputError::new(error.to_string()))?;
        let ports = output.ports();
        let port = ports.iter().find(|port| port.id() == id).or_else(|| ports.iter().find(|port| output.port_name(port).is_ok_and(|name| name == id))).cloned().ok_or_else(|| OutputError::new("The selected MIDI port is no longer available. Refresh ports and select a destination."))?;
        self.connection = Some(
            output
                .connect(&port, "Gosh MIDI Player")
                .map_err(|error| OutputError::new(error.to_string()))?,
        );
        self.current = port.id();
        Ok(())
    }
    fn close(&mut self) {
        self.connection = None;
        self.current.clear();
    }
    fn current_port(&self) -> &str {
        &self.current
    }
    fn send_note_on(&mut self, ch: i32, note: i32, vel: i32) {
        self.channel_message(0x90, ch, note, Some(vel));
    }
    fn send_note_off(&mut self, ch: i32, note: i32, vel: i32) {
        self.channel_message(0x80, ch, note, Some(vel));
    }
    fn send_key_pressure(&mut self, ch: i32, note: i32, vel: i32) {
        self.channel_message(0xA0, ch, note, Some(vel));
    }
    fn send_controller(&mut self, ch: i32, cc: i32, value: i32) {
        self.channel_message(0xB0, ch, cc, Some(value));
    }
    fn send_program(&mut self, ch: i32, pgm: i32) {
        self.channel_message(0xC0, ch, pgm, None);
    }
    fn send_channel_pressure(&mut self, ch: i32, value: i32) {
        self.channel_message(0xD0, ch, value, None);
    }
    fn send_pitch_bend(&mut self, ch: i32, value: i32) {
        let bend = value.saturating_add(8192).clamp(0, 16383);
        self.channel_message(0xE0, ch, bend & 127, Some(bend >> 7));
    }
    fn send_sysex(&mut self, bytes: &[u8]) {
        self.send(bytes);
    }
}
