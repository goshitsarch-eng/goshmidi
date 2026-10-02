# Building and packaging

Use stable Rust, Python 3.11 or newer and the checked-in Cargo.lock.
`rust-toolchain.toml` pins the tested compiler. Install rustfmt and clippy with
`rustup component add rustfmt clippy`. Cargo builds Dioxus Desktop directly;
the Dioxus CLI and Node.js are not required.

Linux instructions were executed in this cloud workspace. Windows and macOS
commands describe the native CI procedure and require validation on those OSes.
See [validation](docs/VALIDATION.md) for the evidence behind each platform.

## Linux

On Debian/Ubuntu:

```sh
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev \
  libayatana-appindicator3-dev libasound2-dev libfluidsynth3 \
  xdg-desktop-portal xdg-desktop-portal-gtk
cargo build --locked
cargo run --locked -- examples/twinkle.kar
```

Use your desktop's GTK or KDE portal provider. A running audio server and a
GM SoundFont are needed for synthesizer playback. To exercise the UI without
an audio device, add `--driver Dummy --connection dummy` after `--`.

```sh
cargo build --release --locked
cargo fetch --locked
python3 scripts/fetch-resources.py
python3 scripts/package-native.py
(cd dist && sha256sum -c SHA256SUMS)
```

The x64 DEB and tar.gz are written to `dist/`. Install the DEB using
`sudo apt install ./dist/gosh-midi-2.0.0-alpha.1-linux-x86_64.deb`, or extract
the archive and run `bin/dmidiplayer`. The archive needs the same system runtime
libraries as the DEB. Bundled resources must stay beside the executable.

## Windows x64 MSVC

Install Visual Studio C++ Build Tools with the Windows SDK, stable Rust for
`x86_64-pc-windows-msvc`, Python 3.11+, .NET and Microsoft WebView2 Runtime.
Run the following in Developer PowerShell from the repository. The Windows SDK
`rc.exe` embeds the application icon and version information in the executable:

```powershell
git clone https://github.com/microsoft/vcpkg.git "$env:TEMP/gosh-vcpkg"
git -C "$env:TEMP/gosh-vcpkg" checkout fdd4e6059c803f56bb5df86e285bf374d9d44407
& "$env:TEMP/gosh-vcpkg/bootstrap-vcpkg.bat" -disableMetrics
& "$env:TEMP/gosh-vcpkg/vcpkg.exe" install --triplet x64-windows --x-manifest-root=packaging/windows --x-install-root=packaging/windows/vcpkg_installed
cargo build --release --locked --target x86_64-pc-windows-msvc
cargo fetch --locked
python scripts/fetch-resources.py
dotnet tool install --global wix --version 4.0.6
Invoke-WebRequest https://go.microsoft.com/fwlink/?linkid=2124701 -OutFile packaging/windows/WebView2Setup.exe
$signature = Get-AuthenticodeSignature packaging/windows/WebView2Setup.exe
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') { throw 'Invalid WebView2 installer signature' }
python scripts/package-native.py --target x86_64-pc-windows-msvc
```

Packaging requires the installed Visual Studio app-local CRT redistributables,
found through `VCToolsRedistDir` or `vswhere`. It includes FluidSynth DLLs and
their dependency DLLs, CRT DLLs, the verified WebView2 installer, SoundFont and
licenses. Install the per-user MSI or extract the portable ZIP, then run
`dmidiplayer.exe`. Keep the ZIP's resources and DLLs with the executable.

## macOS Apple Silicon and Intel

Install Xcode Command Line Tools, stable Rust, Python 3.11+ and Homebrew:

```sh
xcode-select --install
brew install fluidsynth
```

Build on the corresponding native architecture. Use
`aarch64-apple-darwin` on Apple Silicon or `x86_64-apple-darwin` on Intel:

```sh
rustup target add aarch64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo fetch --locked
python3 scripts/fetch-resources.py
python3 scripts/package-native.py --target aarch64-apple-darwin
```

The packager creates an app bundle ZIP, gathers Homebrew shared libraries and
verified upstream license texts, rewrites library references and verifies an
ad-hoc signature. Extract the ZIP and move `Gosh MIDI Player.app` to Applications.
Developer ID signing and notarization remain production release gates.

## Flatpak

Install Flatpak and flatpak-builder, then:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08
python3 flatpak/flatpak-cargo-generator.py Cargo.lock -o /tmp/gosh-sources.json
cmp flatpak/cargo-sources.json /tmp/gosh-sources.json
flatpak-builder --force-clean --user --install --repo=flatpak-repo \
  build-dir flatpak/com.goshapps.GoshMIDI.yml
flatpak run com.goshapps.GoshMIDI --dry-run /app/share/dmidiplayer/test.mid
flatpak run com.goshapps.GoshMIDI
flatpak build-bundle flatpak-repo gosh-midi.flatpak com.goshapps.GoshMIDI
```

Test chooser/document portals, printing, clipboard, real sound and activation
inside the installed sandbox. The actual GNOME SDK build, 338 core tests, installed
parser and nine-stage WebView suite passed here. Document portals still need a
desktop with working FUSE; the cloud host has no `/dev/fuse`.

The managed cloud machine has no FUSE device. Use
`flatpak-builder --disable-rofiles-fuse` there, with
`FLATPAK_USER_DIR=/workspace/goshmidi-environment/flatpak-user` and the environment's
activation script. That supported copy mode keeps the normal build/runtime sandbox.

## Checks

Run `sh scripts/verify.sh`. For actual WebView interaction, build with
`cargo build --locked --features ui-test` and run
`python3 scripts/desktop-qa.py target/debug/dmidiplayer` in a desktop session
(`dmidiplayer.exe` on Windows). Linux can use `xvfb-run -a dbus-run-session --`
before that command. Normal release builds exclude this opt-in harness.

See [development](docs/DEVELOPMENT.md) for audio tests and troubleshooting,
and [release procedure](docs/RELEASE.md) for installed-package acceptance.
