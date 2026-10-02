//! MIDI core: events, formats, engine, outputs (arch §5, PLAN T2–T7).
//!
//! `event.rs` landed in T2; `sequence.rs`/`smf.rs` in T3, `rmi.rs`/`wrk.rs`
//! in T4, `sysex.rs`/`instruments.rs`/`output.rs` in T5,
//! `fluid_ffi.rs`/`output_alsa.rs`/`output_fluid.rs` in T6, `player.rs` in
//! T7. Headless: no iced/libcosmic dependency allowed in this module.

// T2–T7 land ahead of their T9 app-shell call sites, so the bin build sees
// these modules as unused; drop the allows once T9 wires the engine up.
#[allow(dead_code)]
pub mod event;
#[allow(dead_code)]
pub mod fluid_ffi;
#[allow(dead_code)]
pub mod instruments;
#[allow(dead_code)]
pub mod output;
#[allow(dead_code)]
#[cfg(target_os = "linux")]
pub mod output_alsa;
#[allow(dead_code)]
pub mod output_fluid;
#[allow(dead_code)]
pub mod player;
#[allow(dead_code)]
pub(crate) mod rmi;
#[allow(dead_code)]
pub mod sequence;
#[allow(dead_code)]
pub(crate) mod smf;
#[allow(dead_code)]
pub mod sysex;
#[allow(dead_code)]
pub(crate) mod wrk;
