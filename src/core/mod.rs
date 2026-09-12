//! MIDI core: events, formats, engine, outputs (arch §5, PLAN T2–T7).
//!
//! `event.rs` landed in T2; `sequence.rs`/`smf.rs` in T3, `rmi.rs`/`wrk.rs`
//! in T4, `sysex.rs`/`output.rs` in T5, `output_alsa.rs`/`output_fluid.rs`
//! in T6, `player.rs` in T7. Headless: no iced/libcosmic dependency allowed
//! in this module.

// T2 lands ahead of its T3+ call sites, so the bin build sees this module as
// unused; drop the allow once the engine wires it up.
#[allow(dead_code)]
pub mod event;
