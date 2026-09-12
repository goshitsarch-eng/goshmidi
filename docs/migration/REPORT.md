# Migration REPORT — Gosh MIDI Player 1.8.0 (libcosmic/Rust)

Status: **DRAFT skeleton (T15-S2 inputs landed)** — S4 finishes this file at
sign-off (PLAN §0 release gate, Phase 3). S0/S1/S2 sections carry observed
evidence; S3/S4 are stubs until those slices land.

## 0. Release + environment

- Version 1.8.0, 2026-09-12 — first libcosmic/Rust release (GTK4/C++ retired).
- Branch `cosmic-migration`; app ID `com.goshapps.GoshMIDI`; command `dmidiplayer`.
- Toolchain: rustc 1.98.1 (`rust-toolchain.toml`), libcosmic rev `7e82198`
  (D16), freedesktop runtime/SDK 25.08, FluidSynth 2.6.0 (flatpak module).
- Bundled SoundFont: `FluidR3Mono_GM.sf3` v2.312, MIT, 14,563,174 bytes
  (LibreScore/sf3 @ `3a6eda8`, sha256 `cfcd66d8…2794ec2` — S2 §3).

## 1. Parity counts (PLAN §1/U1 — all pinned)

| Count | Value | Pins |
|---|---|---|
| Actions | 23 | `MenuAction::ALL: [MenuAction; 23]` + unit `inventory_is_23_entries_in_cpp_section_order` + `keys_flows::menu_maps_all_23_entries_to_documented_messages` |
| Menu entries | 23 | same inventory (actions+menu unified in Rust) |
| Settings keys | 30 | `KNOWN_KEYS: [&str; 30]` + unit `KNOWN_KEYS.len() == 30` |
| Toast trigger sites | 6 | `Message::Toast` funnel pin + per-path tests (`open`/`playlist`/`lyrics`/`dialogs`/`keys` flows + `app.rs` error→toast units) |
| Accelerators | 9 | `ACCELERATORS: [Accelerator; 9]` + unit + `keys_flows` |

T15 retired the C++ greps; the Rust successors above are enforced by both
`cargo test` and the `verify.sh` count-pins step (S2).

## 2. Gates

- `sh scripts/verify.sh --fast`: **PASS** (S2, 2026-09-12 — 551 lib +
  10 bin + 102 integration-file tests, 0 failed, +1 ignored doctest;
  `desktop-file-validate` + `appstreamcli validate` strict-ok).
- `sh scripts/verify.sh` (full: vendoring freshness + flatpak-builder +
  in-sandbox smoke): **PASS** (S2, 2026-09-12 — single full run; smoke
  confirms the bundled SoundFont in-sandbox: 14,563,174 bytes, sfbk magic,
  MIT license).
- S4 release gate (DE matrix, fresh-profile audible playback, second
  `flatpak run` forward): TODO S4.

## 3. Slice inputs

### S0 — drift triage (landed: `2cb6426`)

- Committed test-only drift (settings/song round-trip pins, loop×repeat
  engine legs, `finish_action` doc); held `theme.rs` for S3 (cites ux §15.1,
  which does not exist yet).

### S1 — arch coverage + P10b close-out (landed: `4dadd91`)

- Portal `.lst` legs, loop×repeat note, Message-variant audit
  (per commit title; details: commit + arch owner).

### S2 — pack retirement + metainfo + migration story (this slice)

- CMake/C++ retired (D14/P9): 30 tracked paths deleted (all `CMakeLists.txt`,
  `cmake/`, C++ sources, `check_identity.cmake`, `test_sequence.cpp`,
  `data/dmidiplayer.gresource.xml`, `linux-build.yml`); legacy gresource
  prefix resolved by removal. `data/style.css` kept as the `theme.rs`
  mapping reference (no build consumer).
- Out of scope, carried to S3 (S0 hold stands): `theme.rs` per-mode
  contrast + `warning_text` cite ux §15.1, which does not exist yet (ux.md
  ends at §14) — reverted from this slice; S3 lands them with §15.1.
- Count pins replaced with Rust successors (table above); toast sites pin
  the `Message::Toast` funnel + named tests (no naive count exists in Rust).
- Metainfo `<release version="1.8.0" date="2026-09-12">` (P11); T14c
  toolkit + rhythm wording re-verified by the same test file.
- Flatpak `soundfont` module: pinned `type: file` sources (font + MIT
  license) installing to `/app/share/soundfonts/`; engine searches the
  bundled file first (`BUNDLED_SOUNDFONT_NAME`). Decode proven pre-merge
  (FluidSynth 2.6.0 renders `test.mid` → 121,408 non-silent frames);
  in-sandbox presence/size/magic/license asserted by `smoke.sh`.
- Stale-path fixtures (`tests/fixtures/stale/` + `tests/stale_flows.rs`):
  dead-absolute config/`.cfg`/`.lst` load verbatim with no drops or panics
  — the shapes S3's re-prompt UX builds against.
- Version 1.8.0 end to end (`Cargo.toml`/`Cargo.lock`, about dialog pins,
  man page header, `ChangeLog`); README build section rewritten for
  cargo/Flatpak with the bundled-SF story.
- New/updated tests: `stale_flows.rs` (4), `desktop_integration.rs` (+3:
  retirement, release, soundfont bond — bond pins both font + license
  sha256, like `verify.sh`), `output_fluid.rs` search-path pin
  updated, version pins bumped. Each observed failing before its fix
  (stale pins passed on landing — they pin new fixtures over unchanged
  verbatim-load behavior; license pin red-probed with a bogus hash).

### S3 — stale-SF wiring + theming (TODO)

- Stub: `theme.rs` per-mode mapping + ux §15.1 + contrast ruling; stale-SF
  fallback + toast + D11 chooser offer against the S2 fixture shapes.

### S4 — release sign-off (TODO)

- Stub: Phase-3 matrix + audible gate + this REPORT finished.

## 4. Known gaps / Phase-3 recipes

- Portal-provider matrix (§3.2/P12), clipboard + second-`flatpak run`
  recipes: manual per packaging.md §3.2 — S4.
- `.flatpak` bundle verification on the provider matrix: S4.
