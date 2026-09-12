//! Hand-written FluidSynth FFI (arch §3.3, DECISIONS D16).
//!
//! The `fluidsynth` 0.0.1 crate is 2018 abandonware and is rejected; no
//! `bindgen` either (avoids a libclang build dependency). Scope is exactly
//! what [`crate::core::output_fluid`] calls: 3 opaque types, 17 functions,
//! plus `FLUID_FAILED`. Plain `#[link(name = "fluidsynth")]` suffices — no
//! build script, no pkg-config probe (verified by `cargo build` linking
//! against fluidsynth-devel locally and the freedesktop SDK in Flatpak).
//!
//! Signatures transcribed from fluidsynth 2.6 headers
//! (`settings.h`, `synth.h`, `audio.h`, `misc.h`); C names are kept verbatim.
//!
//! Dependency rule (arch §5): `std`/`core` only — no iced/libcosmic in `core`.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_double, c_int};

// --- Opaque types (never dereferenced; only passed as pointers) ---

/// Opaque `fluid_settings_t`.
#[repr(C)]
pub struct fluid_settings_t {
    _private: [u8; 0],
}

/// Opaque `fluid_synth_t`.
#[repr(C)]
pub struct fluid_synth_t {
    _private: [u8; 0],
}

/// Opaque `fluid_audio_driver_t`.
#[repr(C)]
pub struct fluid_audio_driver_t {
    _private: [u8; 0],
}

/// Failure return (`misc.h`: `#define FLUID_FAILED (-1)`).
pub const FLUID_FAILED: c_int = -1;

#[link(name = "fluidsynth")]
unsafe extern "C" {
    // --- settings lifecycle (`settings.h`) ---

    /// `fluid_settings_t *new_fluid_settings(void)` — may return null on OOM.
    pub fn new_fluid_settings() -> *mut fluid_settings_t;
    /// `delete_fluid_settings`; `settings` must be a live object from above.
    pub fn delete_fluid_settings(settings: *mut fluid_settings_t);
    /// `fluid_settings_setstr`; `name`/`str` must be valid NUL-terminated C strings.
    pub fn fluid_settings_setstr(
        settings: *mut fluid_settings_t,
        name: *const c_char,
        value: *const c_char,
    ) -> c_int;
    /// `fluid_settings_setnum`; `name` must be a valid NUL-terminated C string.
    pub fn fluid_settings_setnum(
        settings: *mut fluid_settings_t,
        name: *const c_char,
        val: c_double,
    ) -> c_int;

    // --- synth lifecycle + MIDI sends (`synth.h`) ---

    /// `fluid_synth_t *new_fluid_synth(settings)` — may return null.
    pub fn new_fluid_synth(settings: *mut fluid_settings_t) -> *mut fluid_synth_t;
    /// `delete_fluid_synth`; `synth` must be a live object from above.
    pub fn delete_fluid_synth(synth: *mut fluid_synth_t);
    /// `fluid_synth_sfload`; `filename` must be a valid NUL-terminated C string.
    pub fn fluid_synth_sfload(
        synth: *mut fluid_synth_t,
        filename: *const c_char,
        reset_presets: c_int,
    ) -> c_int;
    /// `fluid_synth_noteon(synth, chan, key, vel)`.
    pub fn fluid_synth_noteon(
        synth: *mut fluid_synth_t,
        chan: c_int,
        key: c_int,
        vel: c_int,
    ) -> c_int;
    /// `fluid_synth_noteoff(synth, chan, key)`.
    pub fn fluid_synth_noteoff(synth: *mut fluid_synth_t, chan: c_int, key: c_int) -> c_int;
    /// `fluid_synth_cc(synth, chan, ctrl, val)`.
    pub fn fluid_synth_cc(synth: *mut fluid_synth_t, chan: c_int, ctrl: c_int, val: c_int)
    -> c_int;
    /// `fluid_synth_sysex` with `response`/`response_len`/`handled` nullable.
    pub fn fluid_synth_sysex(
        synth: *mut fluid_synth_t,
        data: *const c_char,
        len: c_int,
        response: *mut c_char,
        response_len: *mut c_int,
        handled: *mut c_int,
        dryrun: c_int,
    ) -> c_int;
    /// `fluid_synth_pitch_bend(synth, chan, val)`; `val` is 0..=16383.
    pub fn fluid_synth_pitch_bend(synth: *mut fluid_synth_t, chan: c_int, val: c_int) -> c_int;
    /// `fluid_synth_program_change(synth, chan, program)`.
    pub fn fluid_synth_program_change(
        synth: *mut fluid_synth_t,
        chan: c_int,
        program: c_int,
    ) -> c_int;
    /// `fluid_synth_channel_pressure(synth, chan, val)`.
    pub fn fluid_synth_channel_pressure(
        synth: *mut fluid_synth_t,
        chan: c_int,
        val: c_int,
    ) -> c_int;
    /// `fluid_synth_key_pressure(synth, chan, key, val)`.
    pub fn fluid_synth_key_pressure(
        synth: *mut fluid_synth_t,
        chan: c_int,
        key: c_int,
        val: c_int,
    ) -> c_int;

    // --- audio driver (`audio.h`) ---

    /// `new_fluid_audio_driver(settings, synth)` — null when no driver works.
    pub fn new_fluid_audio_driver(
        settings: *mut fluid_settings_t,
        synth: *mut fluid_synth_t,
    ) -> *mut fluid_audio_driver_t;
    /// `delete_fluid_audio_driver`; `driver` must be a live object from above.
    pub fn delete_fluid_audio_driver(driver: *mut fluid_audio_driver_t);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Sigil smoke: settings → synth → a note cycle → teardown. Never creates
    /// an audio driver, so it is safe headless; a null settings/synth (broken
    /// install) skips cleanly instead of failing — CI has no audio.
    #[test]
    fn ffi_settings_synth_note_cycle_smoke() {
        // SAFETY: fresh objects, valid args, teardown in reverse order.
        unsafe {
            let settings = new_fluid_settings();
            if settings.is_null() {
                return;
            }
            let synth = new_fluid_synth(settings);
            if synth.is_null() {
                delete_fluid_settings(settings);
                return;
            }
            // No SoundFont loaded: calls are accepted and silently produce
            // nothing — this only proves the signatures link and run.
            fluid_synth_noteon(synth, 0, 60, 80);
            fluid_synth_noteoff(synth, 0, 60);
            fluid_synth_cc(synth, 0, 7, 100);
            fluid_synth_program_change(synth, 0, 40);
            fluid_synth_channel_pressure(synth, 0, 90);
            fluid_synth_key_pressure(synth, 0, 60, 70);
            fluid_synth_pitch_bend(synth, 0, 8192);
            let sysex: [c_char; 3] = [0xF0u8 as c_char, 0x7E as c_char, 0xF7u8 as c_char];
            fluid_synth_sysex(
                synth,
                sysex.as_ptr(),
                sysex.len() as c_int,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
            delete_fluid_synth(synth);
            delete_fluid_settings(settings);
        }
    }

    #[test]
    fn fluid_failed_is_minus_one() {
        assert_eq!(FLUID_FAILED, -1);
    }
}
