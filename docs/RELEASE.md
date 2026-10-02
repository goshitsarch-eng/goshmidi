# Release procedure

Version 2.0.0-alpha.1 remains a prerelease until every platform's acceptance
gates pass. The desktop workflow builds Ubuntu 24.04, Windows 2025 MSVC,
macOS 15 Apple Silicon and macOS 15 Intel, plus Flatpak. It runs format/lint,
Rust tests, resource checksum checks, live WebView acceptance and packaging.
Windows additionally installs the MSI and checks its installed executable;
macOS checks the app's signatures and executable.

`python3 scripts/package-native.py --target TARGET` packages the corresponding
`target/TARGET/release/dmidiplayer` on that OS. Windows prerequisites are the
pinned `packaging/windows/vcpkg.json` tree, WiX 4.0.6 and an Authenticode-verified
Microsoft WebView2 offline installer. The workflow supplies these. Native
packages include the GM SoundFont, examples, application license/notices and
runtime dependency notices. macOS gathers each Homebrew formula's upstream
license texts from a checksum-verified source archive; missing notices fail packaging.

Artifacts have platform/architecture/version names and SHA256SUMS. A `v*` tag
runs the gates and creates a **draft prerelease** only after all jobs pass.
Review artifact contents, source/license compliance and platform acceptance
before publishing. Never treat an unrun workflow as release evidence.

## Production identity

The current macOS bundler signs ad-hoc for development. Developer ID signing,
hardened-runtime entitlements and Apple notarization are not configured.
Windows application/installer signing is also not configured. A production
release must supply the organization's signing identities, implement their
credential-bound signing steps, and test downloads through Gatekeeper/SmartScreen.
No credentials or certificates are stored in this repository.

## Installed-package acceptance

On each native platform: install/uninstall/upgrade the actual artifact; launch
with a fresh profile and with existing 1.8.0 data; open Unicode/spaced paths;
exercise all three views, transport, mixer and preferences; verify actual sound
and external MIDI including SysEx; test drop, clipboard, printing, opener/reveal,
shortcuts, modal focus, resize/maximize/restore, theme changes and secondary
file activation. On macOS test both architectures; on Linux test X11 and
Wayland. Test the actual Flatpak inside its sandbox and through portals.

Record results in VALIDATION.md and the audit acceptance ledger. Hardware and
native-OS gaps remain open gates, even when source and cross-checks pass.
