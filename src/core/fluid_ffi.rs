//! Optional runtime FluidSynth ABI. Library handles outlive every function pointer.
//! The engine owns all synth/settings/audio pointers and destroys them in reverse order.
#![allow(non_camel_case_types)]
use libloading::Library;
use std::ffi::{c_char, c_double, c_int};
use std::sync::OnceLock;

#[repr(C)]
pub struct fluid_settings_t {
    _private: [u8; 0],
}
#[repr(C)]
pub struct fluid_synth_t {
    _private: [u8; 0],
}
#[repr(C)]
pub struct fluid_audio_driver_t {
    _private: [u8; 0],
}
pub const FLUID_FAILED: c_int = -1;

macro_rules! fluid_api {
    ($(fn $name:ident($($argument:ident: $ty:ty),*) -> $result:ty = $fallback:expr;)*) => {
        struct Api {
            _library: Library,
            $($name: unsafe extern "C" fn($($ty),*) -> $result,)*
        }
        impl Api {
            unsafe fn load(library: Library) -> Result<Self, String> {
                // SAFETY: symbols are the stable FluidSynth 2/3 C ABI declared below.
                $(let $name = *unsafe { library.get::<unsafe extern "C" fn($($ty),*) -> $result>(concat!(stringify!($name), "\0").as_bytes()) }.map_err(|error| error.to_string())?;)*
                Ok(Self { _library: library, $($name,)* })
            }
        }
        $(
            /// # Safety
            /// All pointers must satisfy the corresponding FluidSynth C API contract.
            pub unsafe fn $name($($argument: $ty),*) -> $result {
                match api() {
                    Ok(api) => unsafe { (api.$name)($($argument),*) },
                    Err(_) => $fallback,
                }
            }
        )*
    }
}

fluid_api! {
    fn new_fluid_settings() -> *mut fluid_settings_t = std::ptr::null_mut();
    fn delete_fluid_settings(settings: *mut fluid_settings_t) -> () = ();
    fn fluid_settings_setstr(settings: *mut fluid_settings_t, name: *const c_char, value: *const c_char) -> c_int = FLUID_FAILED;
    fn fluid_settings_setnum(settings: *mut fluid_settings_t, name: *const c_char, val: c_double) -> c_int = FLUID_FAILED;
    fn new_fluid_synth(settings: *mut fluid_settings_t) -> *mut fluid_synth_t = std::ptr::null_mut();
    fn delete_fluid_synth(synth: *mut fluid_synth_t) -> () = ();
    fn fluid_synth_sfload(synth: *mut fluid_synth_t, filename: *const c_char, reset_presets: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_noteon(synth: *mut fluid_synth_t, chan: c_int, key: c_int, vel: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_noteoff(synth: *mut fluid_synth_t, chan: c_int, key: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_cc(synth: *mut fluid_synth_t, chan: c_int, ctrl: c_int, val: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_sysex(synth: *mut fluid_synth_t, data: *const c_char, len: c_int, response: *mut c_char, response_len: *mut c_int, handled: *mut c_int, dryrun: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_pitch_bend(synth: *mut fluid_synth_t, chan: c_int, val: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_program_change(synth: *mut fluid_synth_t, chan: c_int, program: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_channel_pressure(synth: *mut fluid_synth_t, chan: c_int, val: c_int) -> c_int = FLUID_FAILED;
    fn fluid_synth_key_pressure(synth: *mut fluid_synth_t, chan: c_int, key: c_int, val: c_int) -> c_int = FLUID_FAILED;
    fn new_fluid_audio_driver(settings: *mut fluid_settings_t, synth: *mut fluid_synth_t) -> *mut fluid_audio_driver_t = std::ptr::null_mut();
    fn delete_fluid_audio_driver(driver: *mut fluid_audio_driver_t) -> () = ();
}

fn api() -> &'static Result<Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| {
        for candidate in crate::platform::fluid_library_candidates() {
            // SAFETY: only the packaged library or platform loader's system paths
            // are used. No MIDI input or document can influence this list.
            #[cfg(target_os = "windows")]
            let library = unsafe {
                use libloading::os::windows::{Library as WindowsLibrary, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32};
                WindowsLibrary::load_with_flags(&candidate, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32).map(Library::from)
            };
            #[cfg(not(target_os = "windows"))]
            let library = unsafe { Library::new(candidate) };
            if let Ok(library) = library
                && let Ok(api) = unsafe { Api::load(library) } { return Ok(api); }
        }
        Err("FluidSynth runtime is unavailable. Install FluidSynth or use a packaged build, or select an external MIDI port.".to_owned())
    })
}

pub fn available() -> Result<(), String> {
    api().as_ref().map(|_| ()).map_err(Clone::clone)
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
