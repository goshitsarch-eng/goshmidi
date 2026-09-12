//! MIDI core: events, formats, engine, outputs (arch §5, PLAN T2–T7).
//!
//! T1 placeholder — `event.rs` lands in T2, `sequence.rs`/`smf.rs` in T3,
//! `rmi.rs`/`wrk.rs` in T4, `sysex.rs`/`output.rs` in T5, `output_alsa.rs`/
//! `output_fluid.rs` in T6, `player.rs` in T7. Headless: no iced/libcosmic
//! dependency allowed in this module.

/// T1 placeholder; replaced by `core::event` in PLAN T2.
#[allow(dead_code)]
pub(crate) struct Placeholder;
