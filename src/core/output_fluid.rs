//! FluidSynth backend — port of `FluidOutput` (`src/midi/output.cpp:276-398`).
//!
//! PLAN T6, arch §1.5 and §3.3. Settings (gain 0.6) + synth + audio driver
//! with the C++ driver fallback order, `sfload` of the requested or default
//! SoundFont, and the pitch-bend `-8192..=8191 → 0..=16383` remap. All FFI
//! goes through the hand-written [`super::fluid_ffi`] (D16, no bindgen).
//!
//! One deliberate A15 change: C++ `open` returns `true` and stashes problems
//! in `lastError` for a UI toast; Rust has no `lastError` channel, so `open`
//! returns `Err` carrying the same accumulated text and the caller (T9)
//! toasts it and falls back to Dummy. Same observable UX, never silent.
//!
//! Dependency rule (arch §5): `std`/`core` only — no iced/libcosmic in `core`.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use core::ffi::c_int;

use super::event::clamp_midi;
use super::fluid_ffi as ffi;
use super::output::{MidiOutput, MidiPort, OutputError};

/// Audio-driver fallback order (output.cpp:306 positions, SDL name corrected
/// for the 2.6 pin), tried in order until one constructs. Pinned by
/// `driver_fallback_order` — do not reorder lightly: sandboxed/CI machines
/// without PulseAudio depend on the later entries.
///
/// DELTA vs C++ (T7 nit): the trailing entry is `sdl3`, not `sdl2`.
/// Upstream removed the SDL2 audio driver in 2.5.0 ("Support for SDL2 has
/// been removed - pls. use SDL3 instead", #1594; deprecated since 2.4.4),
/// so `sdl2` can never construct against our pinned 2.6.0 (host probe:
/// this build supports `alsa, file, jack, oss, pipewire, pulseaudio` —
/// neither SDL name; the Flatpak module probes `sdl3` present). On builds
/// without SDL the entry fails harmlessly like any other miss.
#[cfg(test)]
pub const AUDIO_DRIVERS: [&std::ffi::CStr; 5] =
    [c"pulseaudio", c"pipewire", c"alsa", c"jack", c"sdl3"];

/// Synth gain (output.cpp:300).
pub const SYNTH_GAIN: f64 = 0.6;

/// Bundled SoundFont file name (T15 choice, P7 — the T6-recorded `.sf3`
/// substitution for the 141.5 MiB `FluidR3_GM.sf2`: same Fluid R3 GM set,
/// Ogg-compressed mono build at one-tenth the size).
///
/// Choice: **FluidR3Mono_GM.sf3** v2.312 (Fluid Release 3, General MIDI,
/// mono), installed by the `soundfont` flatpak module to
/// `/app/share/soundfonts/FluidR3Mono_GM.sf3` (full provenance in the
/// manifest comments + packaging.md §6).
///
/// - License: **MIT** — evidence: `FluidR3Mono_License.md` in
///   LibreScore/sf3 (Frank Wen 2000-2002/2008 + Michael Cowgill 2014-16
///   mono version, full COPYING text; installed next to the file in-sandbox).
/// - Size: **14,563,174 bytes (~13.9 MiB)** — evidence: two identical
///   downloads (sha256 pinned in the manifest), `RIFF....sfbk` magic with a
///   consistent size field.
/// - Decode: **proven** — host FluidSynth 2.6.0 (the flatpak pin) renders
///   `examples/test.mid` through it to 121,408 non-silent stereo frames.
/// - Why: full GM coverage incl. the drum bank (the player supports
///   GM/MT-32/SC-55); the `.sf3` the D11 chooser already accepts; the
///   driver fallback keeps its trailing `sdl3` last-resort entry (C++ had
///   `sdl2`), which fails harmlessly on builds without SDL.
pub const BUNDLED_SOUNDFONT_NAME: &str = "FluidR3Mono_GM.sf3";

/// Default SoundFont search paths: the sandbox-bundled file first (P7 —
/// host `/usr` is invisible inside Flatpak), then the 8 host paths in exact
/// C++ order (output.cpp:403-414).
pub fn default_sound_font_paths() -> Vec<String> {
    crate::platform::soundfont_candidates()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// First existing search-path entry, or `""` when no SoundFont is anywhere
/// (output.cpp:416-424 `findDefaultSoundFont`).
pub fn find_default_sound_font() -> String {
    default_sound_font_paths()
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or_default()
}

/// Pure `open` error accumulation (output.cpp:306-324): `None` on full
/// success, else the `"; "`-joined message. Note the C++ driver-failure text
/// omits the trailing SDL driver although it is tried (`sdl2` there, `sdl3`
/// here) — preserved verbatim (display text).
pub fn combine_open_errors(driver_ok: bool, sf: &str, sf_ok: bool) -> Option<String> {
    let mut error = String::new();
    if !driver_ok {
        error.push_str("No audio driver (tried PulseAudio, PipeWire, ALSA, JACK)");
    }
    if !sf_ok {
        let sf_err = if sf.is_empty() {
            "No SoundFont found. Choose a .sf2/.sf3 file (GM, MT-32, or SC-55) in MIDI Setup."
        } else {
            "Unable to load SoundFont"
        };
        if !error.is_empty() {
            error.push_str("; ");
        }
        error.push_str(sf_err);
    }
    if error.is_empty() { None } else { Some(error) }
}

/// Pure single-port entry (output.cpp:287-293): the resolved SoundFont file
/// name in the label, or the no-SoundFont placeholder when `resolved` is empty.
pub fn fluid_port_entry(resolved_sf: &str) -> MidiPort {
    if resolved_sf.is_empty() {
        MidiPort {
            id: "fluidsynth".to_string(),
            label: "FluidSynth (no soundfont found)".to_string(),
        }
    } else {
        let file = Path::new(resolved_sf)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| resolved_sf.to_string());
        MidiPort {
            id: "fluidsynth".to_string(),
            label: format!("FluidSynth ({file})"),
        }
    }
}

/// Pure pitch-bend remap (output.cpp:378): `-8192..=8191 → 0..=16383`,
/// clamped. Saturating add (C++ `value + 8192` overflows — UB — on absurd
/// out-of-range inputs; real bends never approach the `i32` extremes).
pub fn fluid_bend(value: i32) -> i32 {
    value.saturating_add(8192).clamp(0, 16383)
}

/// FluidSynth output backend. The three raw pointers are null while closed;
/// all sends tolerate that (C++ `if (m_synth)` guards).
pub struct FluidOutput {
    settings: *mut ffi::fluid_settings_t,
    synth: *mut ffi::fluid_synth_t,
    driver: *mut ffi::fluid_audio_driver_t,
    requested_sf: String,
    current: String,
}

// The pointers are owned exclusively by this struct (created in `open`,
// destroyed in `close`/`Drop`); the engine thread owns the backend.
unsafe impl Send for FluidOutput {}

impl FluidOutput {
    /// Seeds the requested SoundFont with the default search, mirroring the
    /// C++ manager (`FluidOutput(findDefaultSoundFont())`, output.cpp:429).
    /// Performs no FFI — safe to construct anywhere, including bare CI.
    pub fn new() -> Self {
        Self {
            settings: ptr::null_mut(),
            synth: ptr::null_mut(),
            driver: ptr::null_mut(),
            requested_sf: find_default_sound_font(),
            current: String::new(),
        }
    }

    fn synth_is_live(&self) -> bool {
        !self.synth.is_null()
    }
}

impl Default for FluidOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FluidOutput {
    fn drop(&mut self) {
        self.close();
    }
}

impl MidiOutput for FluidOutput {
    fn backend_name(&self) -> &str {
        "FluidSynth"
    }

    fn ports(&self, _advanced: bool) -> Vec<MidiPort> {
        let sf = if self.requested_sf.is_empty() {
            find_default_sound_font()
        } else {
            self.requested_sf.clone()
        };
        vec![fluid_port_entry(&sf)]
    }

    fn open(&mut self, _port_id: &str) -> Result<(), OutputError> {
        self.close();
        ffi::available().map_err(OutputError::new)?;
        // SAFETY: each pointer is null or freshly returned by its constructor;
        // all C strings are valid (`c"..."` literals or `CString`); teardown
        // runs in reverse order in `close`/`Drop`, exactly once per object.
        unsafe {
            let settings = ffi::new_fluid_settings();
            if settings.is_null() {
                return Err(OutputError::new("Unable to create FluidSynth"));
            }
            self.settings = settings;
            ffi::fluid_settings_setnum(settings, c"synth.gain".as_ptr(), SYNTH_GAIN);
            let synth = ffi::new_fluid_synth(settings);
            if synth.is_null() {
                self.close();
                return Err(OutputError::new("Unable to create FluidSynth"));
            }
            self.synth = synth;
            for driver_name in crate::platform::audio_drivers() {
                ffi::fluid_settings_setstr(
                    settings,
                    c"audio.driver".as_ptr(),
                    driver_name.as_ptr(),
                );
                let driver = ffi::new_fluid_audio_driver(settings, synth);
                if !driver.is_null() {
                    self.driver = driver;
                    break;
                }
            }
            let sf = if self.requested_sf.is_empty() {
                find_default_sound_font()
            } else {
                self.requested_sf.clone()
            };
            let sf_ok = !sf.is_empty()
                && CString::new(sf.as_str()).is_ok_and(|path| {
                    ffi::fluid_synth_sfload(synth, path.as_ptr(), 1) != ffi::FLUID_FAILED
                });
            // C++ sets the current port even when errors accumulated (:325).
            self.current = "fluidsynth".to_string();
            match combine_open_errors(!self.driver.is_null(), &sf, sf_ok) {
                Some(message) => Err(OutputError::new(message)),
                None => Ok(()),
            }
        }
    }

    fn close(&mut self) {
        // SAFETY: mirrors the constructor protocol; each live pointer is
        // deleted exactly once and immediately nulled.
        unsafe {
            if !self.driver.is_null() {
                ffi::delete_fluid_audio_driver(self.driver);
                self.driver = ptr::null_mut();
            }
            if !self.synth.is_null() {
                ffi::delete_fluid_synth(self.synth);
                self.synth = ptr::null_mut();
            }
            if !self.settings.is_null() {
                ffi::delete_fluid_settings(self.settings);
                self.settings = ptr::null_mut();
            }
        }
        self.current.clear();
    }

    fn current_port(&self) -> &str {
        &self.current
    }

    fn set_sound_font(&mut self, path: &str) {
        self.requested_sf = path.to_string();
    }

    fn send_note_on(&mut self, ch: i32, note: i32, vel: i32) {
        if self.synth_is_live() {
            // SAFETY: live synth, plain ints (C++ passes `ch` unmasked too).
            unsafe {
                ffi::fluid_synth_noteon(self.synth, ch, clamp_midi(note), clamp_midi(vel));
            }
        }
    }

    fn send_note_off(&mut self, ch: i32, note: i32, _vel: i32) {
        if self.synth_is_live() {
            unsafe {
                ffi::fluid_synth_noteoff(self.synth, ch, clamp_midi(note));
            }
        }
    }

    fn send_key_pressure(&mut self, ch: i32, note: i32, vel: i32) {
        if self.synth_is_live() {
            unsafe {
                ffi::fluid_synth_key_pressure(self.synth, ch, clamp_midi(note), clamp_midi(vel));
            }
        }
    }

    fn send_controller(&mut self, ch: i32, cc: i32, val: i32) {
        if self.synth_is_live() {
            unsafe {
                ffi::fluid_synth_cc(self.synth, ch, cc, clamp_midi(val));
            }
        }
    }

    fn send_program(&mut self, ch: i32, pgm: i32) {
        if self.synth_is_live() {
            unsafe {
                ffi::fluid_synth_program_change(self.synth, ch, clamp_midi(pgm));
            }
        }
    }

    fn send_channel_pressure(&mut self, ch: i32, val: i32) {
        if self.synth_is_live() {
            unsafe {
                ffi::fluid_synth_channel_pressure(self.synth, ch, clamp_midi(val));
            }
        }
    }

    fn send_pitch_bend(&mut self, ch: i32, value: i32) {
        if self.synth_is_live() {
            unsafe {
                ffi::fluid_synth_pitch_bend(self.synth, ch, fluid_bend(value));
            }
        }
    }

    fn send_sysex(&mut self, data: &[u8]) {
        if !data.is_empty() && self.synth_is_live() {
            // SAFETY: live synth; `data` outlives the call; null response
            // triple means "no reply wanted" (output.cpp:383-384).
            unsafe {
                ffi::fluid_synth_sysex(
                    self.synth,
                    data.as_ptr().cast(),
                    data.len() as c_int,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_fallback_order_pinned() {
        let names: Vec<&str> = AUDIO_DRIVERS
            .iter()
            .map(|d| d.to_str().expect("driver names are ASCII"))
            .collect();
        assert_eq!(names, ["pulseaudio", "pipewire", "alsa", "jack", "sdl3"]);
        assert_eq!(SYNTH_GAIN, 0.6);
    }

    #[test]
    fn bend_remap_boundaries() {
        assert_eq!(fluid_bend(-8192), 0);
        assert_eq!(fluid_bend(-8191), 1);
        assert_eq!(fluid_bend(-1), 8191);
        assert_eq!(fluid_bend(0), 8192);
        assert_eq!(fluid_bend(1), 8193);
        assert_eq!(fluid_bend(8190), 16382);
        assert_eq!(fluid_bend(8191), 16383);
        // Out-of-range inputs clamp instead of wrapping.
        assert_eq!(fluid_bend(-8193), 0);
        assert_eq!(fluid_bend(-100_000), 0);
        assert_eq!(fluid_bend(i32::MIN), 0);
        assert_eq!(fluid_bend(8192), 16383);
        assert_eq!(fluid_bend(100_000), 16383);
        assert_eq!(fluid_bend(i32::MAX), 16383);
    }

    #[test]
    fn error_accumulation_matrix() {
        assert_eq!(combine_open_errors(true, "/sf/gm.sf2", true), None);
        assert_eq!(
            combine_open_errors(false, "/sf/gm.sf2", true).as_deref(),
            Some("No audio driver (tried PulseAudio, PipeWire, ALSA, JACK)")
        );
        assert_eq!(
            combine_open_errors(true, "", false).as_deref(),
            Some(
                "No SoundFont found. Choose a .sf2/.sf3 file (GM, MT-32, or SC-55) in MIDI Setup."
            )
        );
        assert_eq!(
            combine_open_errors(true, "/sf/missing.sf2", false).as_deref(),
            Some("Unable to load SoundFont")
        );
        assert_eq!(
            combine_open_errors(false, "", false).as_deref(),
            Some(
                "No audio driver (tried PulseAudio, PipeWire, ALSA, JACK); \
                 No SoundFont found. Choose a .sf2/.sf3 file (GM, MT-32, or SC-55) in MIDI Setup."
            )
        );
        assert_eq!(
            combine_open_errors(false, "/sf/missing.sf2", false).as_deref(),
            Some(
                "No audio driver (tried PulseAudio, PipeWire, ALSA, JACK); Unable to load SoundFont"
            )
        );
    }

    #[test]
    fn search_path_order_packaged_then_platform() {
        let paths = default_sound_font_paths();
        assert_eq!(
            paths,
            crate::platform::soundfont_candidates()
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        );
        assert!(paths[0].ends_with(BUNDLED_SOUNDFONT_NAME));
        assert!(std::path::Path::new(&paths[0]).is_absolute());
        #[cfg(target_os = "linux")]
        {
            let bundled = paths
                .iter()
                .position(|path| path == "/app/share/soundfonts/FluidR3Mono_GM.sf3")
                .unwrap();
            let system = paths
                .iter()
                .position(|path| path == "/usr/share/sounds/sf2/FluidR3_GM.sf2")
                .unwrap();
            assert!(bundled < system);
        }
    }

    #[test]
    fn port_entry_label_matrix() {
        let none = fluid_port_entry("");
        assert_eq!(none.id, "fluidsynth");
        assert_eq!(none.label, "FluidSynth (no soundfont found)");
        let sf = fluid_port_entry("/usr/share/soundfonts/FluidR3_GM.sf2");
        assert_eq!(sf.id, "fluidsynth");
        assert_eq!(sf.label, "FluidSynth (FluidR3_GM.sf2)");
        let custom = fluid_port_entry("/home/u/my GM.sf3");
        assert_eq!(custom.label, "FluidSynth (my GM.sf3)");
    }

    #[test]
    fn backend_name_and_set_sound_font_label() {
        let mut out = FluidOutput::new();
        assert_eq!(out.backend_name(), "FluidSynth");
        assert_eq!(out.current_port(), "");
        out.set_sound_font("/tmp/custom-font.sf2");
        let ports = out.ports(false);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].id, "fluidsynth");
        assert_eq!(ports[0].label, "FluidSynth (custom-font.sf2)");
        assert_eq!(out.ports(true), ports);
    }

    #[test]
    fn closed_sends_and_double_close_are_noops() {
        let mut out = FluidOutput::new();
        // Null-synth guards: none of these may crash (C++ `if (m_synth)`).
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
        out.close();
        assert_eq!(out.current_port(), "");
    }

    #[test]
    fn open_with_unloadable_soundfont_errors() {
        // Real synth + real driver probing (headless-safe: no audio needed to
        // fail here), deterministic Err — only the driver half of the message
        // varies with the machine, so assert on the SoundFont half.
        let mut out = FluidOutput::new();
        out.set_sound_font("/nonexistent/t6-probe-missing.sf2");
        let err = out.open("fluidsynth").unwrap_err();
        assert!(
            err.to_string().contains("Unable to load SoundFont"),
            "unexpected message: {err}"
        );
        // C++ sets the current port even on the error path (output.cpp:325).
        assert_eq!(out.current_port(), "fluidsynth");
        out.close();
        assert_eq!(out.current_port(), "");
    }

    #[test]
    fn open_ignores_port_id_like_cpp() {
        // Both ids take the same path: a bad SoundFont fails identically.
        let mut out = FluidOutput::new();
        out.set_sound_font("/nonexistent/t6-probe-missing.sf2");
        let a = out.open("").unwrap_err().to_string();
        let b = out.open("whatever:808").unwrap_err().to_string();
        assert_eq!(a, b);
        out.close();
    }
}
