//! D3 MIDI Setup dialog content (ux D3).
//!
//! Real T13b implementation of the frozen [`MidiSetupViewModel`] contract,
//! consumed as-is: owned `vm` in, one [`cosmic::Element`] out — the C++
//! fields in order (backend, show-all + refresh, connection, map, reset,
//! SoundFont + browse, status, test + apply). (Arch composes this content
//! into the [`chrome`][super::chrome] wrap next — see the composition
//! contract below.)
//!
//! Widget → `Message` map (all variants exist in `message.rs`):
//!
//! | widget | message |
//! |---|
//! | Output backend dropdown | `MidiBackendSelected(name)` |
//! | `Show all ports` checkbox | `MidiAdvancedToggled(bool)` |
//! | `Refresh ports` | `MidiBackendSelected(current)` (refill retrigger — see below) |
//! | Connection dropdown (labels shown) | `MidiPortSelected(id)` |
//! | Instrument map dropdown | `MidiInstrumentMapChanged(index)` |
//! | SysEx reset dropdown | `MidiResetChanged(index)` |
//! | SoundFont entry | `MidiSoundFontChanged(String)` (verbatim) |
//! | `Browse…` | `MidiSoundFontBrowse` (D11; entry-only, deferred commit) |
//! | `Test note` | `MidiTestNote` (Middle C ch1 96, 400 ms + transient status) |
//! | suggested `Apply` | `MidiApply` (commit + reseat + save + close) |
//! | status line | display-only (no message) |
//!
//! Strings + field order are SINGLE-SOURCED in [`state::midi`][crate::state::midi]
//! (T13a): [`MIDI_SETUP_FIELDS`][crate::state::midi::MIDI_SETUP_FIELDS], the
//! button labels, the SoundFont placeholder, and the hint strings. Map/reset
//! options reuse [`prefs::MAP_OPTIONS`]/[`prefs::RESET_OPTIONS`] (never
//! duplicated — the D3 lists are the same C++ literals).
//!
//! T13 contract notes (implemented exactly):
//!
//! * Field order is verbatim C++ (`window.cpp:961-1128`).
//! * The builder implements the C++ refill preselect (`window.cpp:1044-1051`):
//!   the wanted port is the saved/current id, matched by exact id or label
//!   substring, LAST match wins, else index 0 (`None` when the list is
//!   empty — C++ leaves the dropdown model-less there). Single-sourced in
//!   [`select_refilled_port`][crate::state::midi::select_refilled_port]
//!   (the T13b divergence flag is reconciled — T13c fixed the shared fn to
//!   last-wins per the C++ loop).
//! * Refresh re-emits the CURRENT backend selection: C++ `refillFn` reads
//!   the backend dropdown on every trigger (backend notify, `Show all`
//!   toggle, Refresh click — `window.cpp:1063-1077`), so reselecting the
//!   current backend retriggers enumeration arch-side with NO new message
//!   variant. Profile inference (port change → map+reset,
//!   `window.cpp:1078-1089`) lives arch-side too.
//! * `status` is the hint line: the transient test result after Test, else
//!   the stale-SoundFont warning when the entry is stale (ux §15.1,
//!   `warning_text` style), else the no-destinations hint when `ports` is
//!   empty, else empty (C++ `window.cpp:1055-1058,1144-1154` for the latter
//!   two; the stale warning is new, dim otherwise).
//! * Defensive clamps (lyrics precedent): dropdown selections clamp to the
//!   nearest valid option (C++ clamps map/reset via `std::clamp`,
//!   `window.cpp:995-996,1008-1009`; the port clamps all four, so corrupt
//!   state can never panic a widget).
//!
//! Composition contract (arch wires next — `app`/`state`/`message` are
//! untouched here):
//!
//! * shell state: dialog scratch or live `MidiState` (arch decides — C++
//!   holds dialog-local `Widgets` and commits on Apply/Test, so a scratch
//!   is the faithful shape; the live arms exist today as the interim);
//! * per-widget messages above; backend/advanced/Refresh arms relaunch the
//!   port-enumeration `Task` (results arrive as `MidiPortsRefreshed`);
//! * port-select arms run profile inference (map+reset follow the label);
//! * Test/Apply arms implement `window.cpp:1130-1180` (Test saves the
//!   SoundFont + connects + sends Middle C; Apply commits soundFont/
//!   advanced/map/reset + reseats + saves + closes); the transient test
//!   status feeds the builder's `test_status`.
//!
//! A11y: checkbox + all five buttons carry names + descriptions;
//! dropdowns and the entry have no `.name()` API in the pinned rev, so
//! their visible labels are the accessible names per ux §11 (T11b
//! precedent).
//!
//! [`MidiSetupViewModel`]: super::MidiSetupViewModel
//! [`prefs::MAP_OPTIONS`]: super::prefs::MAP_OPTIONS
//! [`prefs::RESET_OPTIONS`]: super::prefs::RESET_OPTIONS

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget::{button, checkbox, column, container, dropdown, row, text, text_input};

use super::MidiSetupViewModel;
use super::prefs::{MAP_OPTIONS, RESET_OPTIONS};
use super::theme;
use crate::message::Message;
use crate::state::midi::{
    APPLY_LABEL, MIDI_SETUP_FIELDS, MidiState, NO_DESTINATIONS_HINT, NO_MIDI_OUTPUT,
    REFRESH_PORTS_LABEL, SOUNDFONT_BROWSE_LABEL, SOUNDFONT_PLACEHOLDER, TEST_NOTE_LABEL,
    TEST_NOTE_STATUS, is_stale_soundfont, refill_hint, stale_soundfont_hint,
};

/// Dialog title (`window.cpp:950`, verbatim — composition sets it).
pub const TITLE: &str = "MIDI Setup";
/// Field labels: readable aliases over [`MIDI_SETUP_FIELDS`] (single source;
/// the unit tests pin the verbatim strings through them).
pub const LABEL_BACKEND: &str = MIDI_SETUP_FIELDS[0];
/// Field labels: readable aliases over [`MIDI_SETUP_FIELDS`] (single source;
/// the unit tests pin the verbatim strings through them).
pub const LABEL_ADVANCED: &str = MIDI_SETUP_FIELDS[1];
/// Field labels: readable aliases over [`MIDI_SETUP_FIELDS`] (single source;
/// the unit tests pin the verbatim strings through them).
pub const LABEL_CONNECTION: &str = MIDI_SETUP_FIELDS[2];
/// Field labels: readable aliases over [`MIDI_SETUP_FIELDS`] (single source;
/// the unit tests pin the verbatim strings through them).
pub const LABEL_MAP: &str = MIDI_SETUP_FIELDS[3];
/// Field labels: readable aliases over [`MIDI_SETUP_FIELDS`] (single source;
/// the unit tests pin the verbatim strings through them).
pub const LABEL_RESET: &str = MIDI_SETUP_FIELDS[4];
/// Field labels: readable aliases over [`MIDI_SETUP_FIELDS`] (single source;
/// the unit tests pin the verbatim strings through them).
pub const LABEL_SOUNDFONT: &str = MIDI_SETUP_FIELDS[5];
/// Button labels: aliases over the shared D3 consts (single source).
pub const LABEL_REFRESH: &str = REFRESH_PORTS_LABEL;
/// Button labels: aliases over the shared D3 consts (single source).
pub const LABEL_BROWSE: &str = SOUNDFONT_BROWSE_LABEL;
/// Button labels: aliases over the shared D3 consts (single source).
pub const LABEL_TEST: &str = TEST_NOTE_LABEL;
/// Button labels: aliases over the shared D3 consts (single source).
pub const LABEL_APPLY: &str = APPLY_LABEL;
/// Focus-on-open target (T14d G4 partial): composition focuses the D3
/// Apply button on open (the `focus(id)` op unfocuses everything else —
/// pinned `iced/core/src/widget/operation/focusable.rs:30-57` — so base
/// Enter cannot fire silently under the dialog).
pub const APPLY_ID: &str = "midi-apply";
/// Hint aliases over the shared D3 consts (single source; the builder only
/// carries the Test results — arch composes them after Test).
pub const HINT_NO_DESTINATIONS: &str = NO_DESTINATIONS_HINT;
/// Hint aliases over the shared D3 consts (single source; the builder only
/// carries the Test results — arch composes them after Test).
pub const HINT_TEST_SENT: &str = TEST_NOTE_STATUS;
/// Hint aliases over the shared D3 consts (single source; the builder only
/// carries the Test results — arch composes them after Test).
pub const HINT_NO_OUTPUT: &str = NO_MIDI_OUTPUT;

/// Clamp a selection against its options: out-of-range → nearest valid,
/// empty options → no selection (lyrics precedent).
fn clamp_selected(selected: usize, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(selected.min(len - 1))
    }
}

/// C++ refill preselect (`window.cpp:1047-1051`): the LAST port whose id
/// equals `want` or whose label contains it; `None` when the list is empty,
/// else index 0 when nothing matches. Delegates to the shared
/// [`select_refilled_port`][crate::state::midi::select_refilled_port] (the
/// T13b divergence flag is reconciled — T13c fixed the shared fn to
/// last-wins, so the duplication is gone).
fn preselect_port(ports: &[crate::message::MidiPort], want: &str) -> Option<usize> {
    if ports.is_empty() {
        return None;
    }
    Some(crate::state::midi::select_refilled_port(ports, want))
}

/// Build the D3 view model: backend/port selections (C++ refill preselect),
/// widget mirrors, and the status line (the transient `test_status` wins;
/// else the stale-SoundFont warning when the entry is stale (ux §15.1); else
/// the no-destinations hint when `ports` is empty, else empty). Pure — no
/// `App` access; composition calls it fresh per frame.
pub fn view_model(midi: &MidiState, test_status: Option<&str>) -> MidiSetupViewModel {
    let backend_selected = midi
        .backends
        .iter()
        .position(|name| *name == midi.backend)
        .unwrap_or(0);
    let backend_name = midi
        .backends
        .get(backend_selected)
        .cloned()
        .unwrap_or_else(|| midi.backend.clone());
    let stale = test_status.is_none() && is_stale_soundfont(&midi.soundfont);
    let status = match test_status {
        Some(status) => status.to_string(),
        None if stale => stale_soundfont_hint(&midi.soundfont),
        None => refill_hint(midi.ports.is_empty()).unwrap_or("").to_string(),
    };
    MidiSetupViewModel {
        backends: midi.backends.clone(),
        backend_selected,
        backend_name,
        ports: midi.ports.clone(),
        port_selected: preselect_port(&midi.ports, &midi.port),
        advanced: midi.advanced,
        map: midi.instrument_map.min(2),
        reset: midi.sysex_reset.min(4),
        soundfont: midi.soundfont.clone(),
        status,
        status_warning: stale,
    }
}

/// One D3 button: label + a11y name/description. `suggested` styles Apply
/// (`window.cpp:1125`); `id` is the focus-on-open target for Apply only
/// (`None` everywhere else — T14d G4 partial).
fn dialog_button(
    label: &'static str,
    description: &'static str,
    on_press: Message,
    suggested: bool,
    id: Option<&'static str>,
) -> Element<'static, Message> {
    let clicked = if suggested {
        button::suggested(label).on_press(on_press)
    } else {
        button::standard(label).on_press(on_press)
    };
    let clicked = match id {
        Some(id) => clicked.id(cosmic::iced::core::widget::Id::new(id)),
        None => clicked,
    };
    clicked.name(label).description(description).into()
}

/// D3 MIDI Setup dialog content. `vm` is the whole contract — no other
/// `App` access. By value: owned data moves into the widgets (see
/// [`MidiSetupViewModel`]).
pub fn view<'a>(vm: MidiSetupViewModel) -> Element<'a, Message> {
    // Explicit destructure pins the frozen contract: an arch-side shape
    // change fails here, loudly, instead of silently dropping a field.
    let MidiSetupViewModel {
        backends,
        backend_selected,
        backend_name,
        ports,
        port_selected,
        advanced,
        map,
        reset,
        soundfont,
        status,
        status_warning,
    } = vm;

    // Backend dropdown: the selected NAME travels (C++ reads the model
    // string, `window.cpp:1035`); a stale index degrades to the current
    // name, never a panic, never a bogus backend.
    let backend_names = backends.clone();
    let backend_fallback = backend_name.clone();
    let backend_dropdown = dropdown(
        backends,
        clamp_selected(backend_selected, backend_names.len()),
        move |index| {
            Message::MidiBackendSelected(
                backend_names
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| backend_fallback.clone()),
            )
        },
    );

    // Refresh reselects the CURRENT backend (refill retrigger — C++
    // `refillFn` reads the dropdown on every trigger, `window.cpp:1073-1077`).
    let refresh_name = backend_name.clone();
    let advanced_row = row::with_children(vec![
        checkbox(advanced)
            .label(LABEL_ADVANCED)
            .on_toggle(Message::MidiAdvancedToggled)
            .name(LABEL_ADVANCED)
            .description("List every MIDI port, including hidden ones")
            .into(),
        dialog_button(
            LABEL_REFRESH,
            "Re-enumerate MIDI ports for the selected backend",
            Message::MidiBackendSelected(refresh_name),
            false,
            None,
        ),
    ])
    .spacing(8)
    .align_y(cosmic::iced::Alignment::Center);

    // Connection dropdown: labels shown, ids messaged (C++ carries the id
    // separately, `window.cpp:1134-1137`); a stale index degrades to `""`.
    let port_labels: Vec<String> = ports.iter().map(|port| port.label.clone()).collect();
    let port_ids: Vec<String> = ports.iter().map(|port| port.id.clone()).collect();
    let port_len = port_labels.len();
    let port_dropdown = dropdown(
        port_labels,
        port_selected.and_then(|selected| clamp_selected(selected, port_len)),
        move |index| Message::MidiPortSelected(port_ids.get(index).cloned().unwrap_or_default()),
    );

    // Map/reset dropdowns: the option INDEX travels (`gtk_drop_down_get_selected`,
    // `window.cpp:1170-1171`); defensive `min` mirrors the arm clamps.
    let map_dropdown = dropdown(
        MAP_OPTIONS.as_slice(),
        clamp_selected(map as usize, MAP_OPTIONS.len()),
        |index| Message::MidiInstrumentMapChanged(index.min(2) as u8),
    );
    let reset_dropdown = dropdown(
        RESET_OPTIONS.as_slice(),
        clamp_selected(reset as usize, RESET_OPTIONS.len()),
        |index| Message::MidiResetChanged(index.min(4) as u8),
    );

    // SoundFont entry (hexpand) + Browse (D11; entry-only, deferred commit —
    // committed by Test/Apply, `window.cpp:1104-1117,1138,1168`).
    let soundfont_row = row::with_children(vec![
        text_input(SOUNDFONT_PLACEHOLDER, soundfont)
            .on_input(Message::MidiSoundFontChanged)
            .width(Length::Fill)
            .into(),
        dialog_button(
            LABEL_BROWSE,
            "Choose a SoundFont file",
            Message::MidiSoundFontBrowse,
            false,
            None,
        ),
    ])
    .spacing(8)
    .align_y(cosmic::iced::Alignment::Center);

    // Status label: dim (C++ `dim-label`, `window.cpp:1025-1030`), or the
    // warning ink when it carries the stale-SoundFont hint (ux §15.1).
    let status_style = if status_warning {
        theme::warning_text
    } else {
        theme::dim_label
    };
    let status_label = text::body(status).class(theme::text_class(status_style));

    // Button row, right-aligned, spacing 8 (`window.cpp:1121-1128`).
    let button_row = container(
        row::with_children(vec![
            dialog_button(
                LABEL_TEST,
                "Play a Middle C test note",
                Message::MidiTestNote,
                false,
                None,
            ),
            dialog_button(
                LABEL_APPLY,
                "Apply the MIDI setup and close",
                Message::MidiApply,
                true,
                Some(APPLY_ID),
            ),
        ])
        .spacing(8),
    )
    .width(Length::Fill)
    .align_x(cosmic::iced::Alignment::End);

    // Field order + margins 18 / spacing 10 are verbatim C++
    // (`window.cpp:954-959`). Dropdowns/entries have no `.name()` API —
    // their visible labels above are the accessible names (ux §11).
    column::with_children(vec![
        text::body(LABEL_BACKEND).into(),
        backend_dropdown.into(),
        advanced_row.into(),
        text::body(LABEL_CONNECTION).into(),
        port_dropdown.into(),
        text::body(LABEL_MAP).into(),
        map_dropdown.into(),
        text::body(LABEL_RESET).into(),
        reset_dropdown.into(),
        text::body(LABEL_SOUNDFONT).into(),
        soundfont_row.into(),
        status_label.into(),
        button_row.into(),
    ])
    .spacing(10)
    .padding(18)
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MidiPort;

    fn test_midi() -> MidiState {
        let mut midi = MidiState::new();
        midi.ports = vec![
            MidiPort {
                id: "128:0".to_string(),
                label: "MT-32:Standard".to_string(),
            },
            MidiPort {
                id: "128:1".to_string(),
                label: "MT-32:GM Emulation".to_string(),
            },
            MidiPort {
                id: "130:0".to_string(),
                label: "Virtual SC55".to_string(),
            },
        ];
        midi
    }

    #[test]
    fn strings_are_verbatim_cpp() {
        assert_eq!(TITLE, "MIDI Setup");
        assert_eq!(LABEL_BACKEND, "Output backend");
        assert_eq!(
            LABEL_CONNECTION,
            "Connection (your MIDI synth / Munt / USB port)"
        );
        assert_eq!(LABEL_MAP, "Instrument map");
        assert_eq!(
            LABEL_RESET,
            "System Exclusive reset (sent before each song)"
        );
        assert_eq!(LABEL_SOUNDFONT, "FluidSynth SoundFont (.sf2 / .sf3)");
        assert_eq!(LABEL_ADVANCED, "Show all ports");
        assert_eq!(LABEL_REFRESH, "Refresh ports");
        assert_eq!(LABEL_BROWSE, "Browse…");
        assert_eq!(LABEL_TEST, "Test note");
        assert_eq!(LABEL_APPLY, "Apply");
        assert_eq!(
            SOUNDFONT_PLACEHOLDER,
            "Optional: your GM, MT-32, or SC-55 SoundFont"
        );
        assert_eq!(
            HINT_NO_DESTINATIONS,
            "No destinations. Start mt32emu-qt (Munt) or Nuked-SC55, then Refresh."
        );
        assert_eq!(HINT_TEST_SENT, "Sent Middle C on channel 1");
        assert_eq!(HINT_NO_OUTPUT, "No MIDI output");
    }

    #[test]
    fn preselect_matches_id_or_substring_last_wins() {
        let midi = test_midi();
        // Exact id.
        assert_eq!(preselect_port(&midi.ports, "130:0"), Some(2));
        // Label substring.
        assert_eq!(preselect_port(&midi.ports, "Virtual"), Some(2));
        // Last match wins ("MT-32" hits 0 and 1).
        assert_eq!(preselect_port(&midi.ports, "MT-32"), Some(1));
        // No match → 0; empty want → 0; empty list → None.
        assert_eq!(preselect_port(&midi.ports, "nope"), Some(0));
        assert_eq!(preselect_port(&midi.ports, ""), Some(0));
        assert_eq!(preselect_port(&[], "128:0"), None);
    }

    #[test]
    fn builder_selections_status_and_clamps() {
        let mut midi = test_midi();
        midi.backend = "ALSA".to_string();
        midi.port = "128:1".to_string();
        midi.advanced = true;
        midi.instrument_map = 9;
        midi.sysex_reset = 9;
        midi.soundfont = "/sf/sc55.sf2".to_string();
        let vm = view_model(&midi, None);
        assert_eq!(vm.backend_selected, 0);
        assert_eq!(vm.backend_name, "ALSA");
        assert_eq!(vm.port_selected, Some(1));
        assert!(vm.advanced);
        assert_eq!(vm.map, 2);
        assert_eq!(vm.reset, 4);
        assert_eq!(vm.soundfont, "/sf/sc55.sf2");
        // S3: the stale entry shows its warning hint (ux §15.1), not empty.
        assert_eq!(vm.status, stale_soundfont_hint("/sf/sc55.sf2"));
        assert!(vm.status_warning);
        // Unknown backend → 0 (first backend name; the live-selection
        // fallback only fires when the backend list itself is empty).
        midi.backend = "Jack".to_string();
        let vm = view_model(&midi, None);
        assert_eq!(vm.backend_selected, 0);
        assert_eq!(vm.backend_name, "ALSA");
        // Empty ports + fresh entry → None + the no-destinations hint (the
        // stale entry above would win over the refill hint — see
        // `builder_stale_status_priority`).
        midi.soundfont.clear();
        midi.ports.clear();
        let vm = view_model(&midi, None);
        assert_eq!(vm.port_selected, None);
        assert_eq!(vm.status, HINT_NO_DESTINATIONS);
        assert!(!vm.status_warning);
        // Transient test status wins over everything (even a stale entry).
        midi.soundfont = "/sf/sc55.sf2".to_string();
        let vm = view_model(&midi, Some(HINT_TEST_SENT));
        assert_eq!(vm.status, HINT_TEST_SENT);
        assert!(!vm.status_warning);
        let midi = test_midi();
        let vm = view_model(&midi, Some(HINT_NO_OUTPUT));
        assert_eq!(vm.status, HINT_NO_OUTPUT);
        assert!(!vm.status_warning);
    }

    /// S3 status priority (ux §15.1): transient test result > stale-SoundFont
    /// warning > refill hint > empty. The warning names the stale entry and
    /// flips the style flag; a fresh entry never warns.
    #[test]
    fn builder_stale_status_priority() {
        // Stale entry + populated ports → stale warning (not empty).
        let mut midi = test_midi();
        midi.soundfont = "/home/staleuser/sf/old-gm.sf2".to_string();
        let vm = view_model(&midi, None);
        assert_eq!(
            vm.status,
            stale_soundfont_hint("/home/staleuser/sf/old-gm.sf2")
        );
        assert!(vm.status_warning);
        // Stale entry + empty ports → stale still wins over the refill hint.
        midi.ports.clear();
        let vm = view_model(&midi, None);
        assert_eq!(
            vm.status,
            stale_soundfont_hint("/home/staleuser/sf/old-gm.sf2")
        );
        assert!(vm.status_warning);
        // Fresh empty entry + populated ports → empty, no warning.
        let midi = test_midi();
        let vm = view_model(&midi, None);
        assert!(vm.status.is_empty());
        assert!(!vm.status_warning);
        // Fresh empty entry + empty ports → refill hint, no warning.
        let mut midi = test_midi();
        midi.ports.clear();
        let vm = view_model(&midi, None);
        assert_eq!(vm.status, HINT_NO_DESTINATIONS);
        assert!(!vm.status_warning);
    }

    #[test]
    fn clamps_pin_widget_edges() {
        assert_eq!(clamp_selected(0, 3), Some(0));
        assert_eq!(clamp_selected(9, 3), Some(2));
        assert_eq!(clamp_selected(0, 0), None);
    }

    #[test]
    fn view_builds_across_contract_shapes() {
        // `view` is not introspectable, but it must build without panic
        // across the contract's shapes: populated, empty ports, empty
        // backends, transient status, stale warning (both styles), corrupt
        // selections.
        let _ = view(view_model(&test_midi(), None));
        let mut midi = test_midi();
        midi.ports.clear();
        let _ = view(view_model(&midi, None));
        let _ = view(view_model(&midi, Some(HINT_TEST_SENT)));
        midi.backends.clear();
        let _ = view(view_model(&midi, None));
        let mut stale = test_midi();
        stale.soundfont = "/home/staleuser/sf/old-gm.sf2".to_string();
        let stale_vm = view_model(&stale, None);
        assert!(stale_vm.status_warning);
        let _ = view(stale_vm);
        let mut corrupt = view_model(&test_midi(), None);
        corrupt.backend_selected = 99;
        corrupt.port_selected = Some(99);
        corrupt.map = 255;
        corrupt.reset = 255;
        let _ = view(corrupt);
        let mut none_port = view_model(&test_midi(), None);
        none_port.port_selected = None;
        let _ = view(none_port);
    }
}
