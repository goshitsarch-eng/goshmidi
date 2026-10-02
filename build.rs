use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=icons/dmidiplayer_128x128.png");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !cfg!(target_os = "windows") {
        println!("cargo:warning=Windows icon/version resources require a native Windows SDK build");
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let png = fs::read("icons/dmidiplayer_128x128.png").expect("application icon");
    // ICO container with the original PNG pixels; no resampling.
    let mut ico = vec![0, 0, 1, 0, 1, 0, 128, 128, 0, 0, 1, 0, 32, 0];
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22_u32.to_le_bytes());
    ico.extend_from_slice(&png);
    let icon = out.join("GoshMIDI.ico");
    fs::write(&icon, ico).expect("write application icon");
    let version = env::var("CARGO_PKG_VERSION").expect("package version");
    let numbers = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| env::var(format!("CARGO_PKG_VERSION_{part}")).expect("version component"));
    let numeric = format!("{},{},{},0", numbers[0], numbers[1], numbers[2]);
    let icon = icon
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = out.join("application.rc");
    fs::write(
        &source,
        format!(
            r#"#pragma code_page(65001)
#include <winver.h>
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x2L
FILEOS VOS_NT_WINDOWS32
FILETYPE VFT_APP
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904b0"
  BEGIN
   VALUE "CompanyName", "Gosh MIDI Player contributors\0"
   VALUE "FileDescription", "Gosh MIDI Player\0"
   VALUE "FileVersion", "{version}\0"
   VALUE "ProductName", "Gosh MIDI Player\0"
   VALUE "ProductVersion", "{version}\0"
   VALUE "OriginalFilename", "dmidiplayer.exe\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x0409, 1200
 END
END
"#
        ),
    )
    .expect("write Windows version resource");
    let resource = out.join("application.res");
    let status = Command::new("rc.exe")
        .arg("/nologo")
        .arg(format!("/fo{}", resource.display()))
        .arg(source)
        .status()
        .expect("Windows SDK rc.exe is required; use Developer PowerShell");
    assert!(status.success(), "Windows resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=dmidiplayer={}",
        resource.display()
    );
}
