//! `dmidiplayer` library root: shares the application modules between the
//! binary (`src/main.rs`) and integration tests (`tests/`).
//!
//! The binary and the test harness use this single copy of the code; no
//! module is duplicated between targets.

pub mod app;
pub mod config;
pub mod core;
pub mod message;
pub mod state;
pub mod ui;
