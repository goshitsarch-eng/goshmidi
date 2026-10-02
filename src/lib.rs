//! `dmidiplayer` library root: shares the application modules between the
//! binary (`src/main.rs`) and integration tests (`tests/`).
//!
//! The binary and the test harness use this single copy of the code; no
//! module is duplicated between targets.

pub mod application;
pub mod commands;
pub mod config;
pub mod core;
#[cfg(feature = "desktop")]
pub mod frontend;
pub mod models;
pub mod platform;
pub mod state;
