//! Operating-system integrations. Domain and UI code depend on this boundary.
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod instance;
pub mod midi;
pub mod persistence;

use std::path::PathBuf;

pub fn native_directories() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let base = directories::BaseDirs::new()?;
    Some((
        base.home_dir().to_owned(),
        base.config_dir().to_owned(),
        base.data_local_dir().to_owned(),
    ))
}

pub fn soundfont_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        paths.push(dir.join("resources/soundfonts/FluidR3Mono_GM.sf3"));
        paths.push(dir.join("soundfonts/FluidR3Mono_GM.sf3"));
        paths.push(dir.join("../Resources/soundfonts/FluidR3Mono_GM.sf3"));
        paths.push(dir.join("../share/dmidiplayer/soundfonts/FluidR3Mono_GM.sf3"));
    }
    #[cfg(target_os = "linux")]
    paths.extend(
        [
            "/app/share/soundfonts/FluidR3Mono_GM.sf3",
            "/usr/share/sounds/sf2/FluidR3_GM.sf2",
            "/usr/share/soundfonts/FluidR3_GM.sf2",
            "/usr/share/sounds/sf2/default-GM.sf2",
            "/usr/share/soundfonts/default.sf2",
            "/usr/share/sounds/sf3/default.sf3",
            "/usr/share/sounds/sf2/TimGM6mb.sf2",
            "/usr/share/sounds/sf2/FluidR3_GS.sf2",
            "/usr/local/share/soundfonts/default.sf2",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths
}

pub fn fluid_library_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    let names = ["libfluidsynth-3.dll", "libfluidsynth.dll", "fluidsynth.dll"];
    #[cfg(target_os = "macos")]
    let names = ["libfluidsynth.3.dylib", "libfluidsynth.dylib"];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let names = ["libfluidsynth.so.3", "libfluidsynth.so"];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for name in names {
            out.extend([
                dir.join(name),
                dir.join("lib").join(name),
                dir.join("../Resources/lib").join(name),
            ]);
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    out.extend(names.into_iter().map(PathBuf::from));
    #[cfg(target_os = "macos")]
    out.extend(
        [
            "/opt/homebrew/lib/libfluidsynth.3.dylib",
            "/usr/local/lib/libfluidsynth.3.dylib",
        ]
        .map(PathBuf::from),
    );
    out
}

pub fn audio_drivers() -> &'static [&'static std::ffi::CStr] {
    #[cfg(target_os = "windows")]
    {
        &[c"wasapi", c"dsound", c"portaudio"]
    }
    #[cfg(target_os = "macos")]
    {
        &[c"coreaudio", c"portaudio"]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        &[c"pulseaudio", c"pipewire", c"alsa", c"jack", c"sdl3"]
    }
}

pub fn packaged_examples() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        paths.extend([
            dir.join("resources/examples"),
            dir.join("../Resources/examples"),
            dir.join("../share/dmidiplayer"),
        ]);
    }
    #[cfg(target_os = "linux")]
    paths.extend([
        PathBuf::from("/app/share/dmidiplayer"),
        PathBuf::from("/usr/share/dmidiplayer"),
    ]);
    paths.push(PathBuf::from("examples"));
    paths
}
