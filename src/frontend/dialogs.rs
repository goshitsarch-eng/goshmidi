use super::dispatch;
use crate::{
    commands::{Action, Command, Preference},
    models::Dialog,
};
use dioxus::prelude::*;

#[component]
pub fn Modal(dialog: Dialog) -> Element {
    let d = dispatch();
    let title = match dialog {
        Dialog::Preferences => "Preferences",
        Dialog::MidiSetup => "MIDI Setup",
        Dialog::FileInfo => "File information",
        Dialog::About => "About Gosh MIDI Player",
        Dialog::Help => "Help",
        Dialog::Jump => "Jump to bar",
        Dialog::Loop => "Loop range",
        Dialog::Unsaved => "Save playlist changes?",
        Dialog::SaveFailure => "Settings could not be saved",
    };
    use_effect(|| {
        let _ = document::eval(
            "requestAnimationFrame(()=>document.querySelector('[role=dialog] input,[role=dialog] select,[role=dialog] button')?.focus())",
        );
    });
    rsx! {div {class:"backdrop",
        section {class:"modal",role:"dialog",aria_modal:"true",aria_labelledby:"dialog-title",tabindex:"-1",
            div {class:"modal-head",h2 {id:"dialog-title","{title}"}button {id:"close-dialog",aria_label:"Close dialog",onclick:move |_|d.send(Command::CancelDialog),"×"}}
            div {class:"modal-body",
                match dialog {
                    Dialog::Preferences=>rsx!{Preferences {}},Dialog::MidiSetup=>rsx!{Midi {}},Dialog::FileInfo=>rsx!{FileInfo {}},
                    Dialog::About=>rsx!{About {}},Dialog::Help=>rsx!{p {class:"help",{crate::state::dialogs::HELP_TEXT}}},
                    Dialog::Jump=>rsx!{Jump {}},Dialog::Loop=>rsx!{Loop {}},
                    Dialog::Unsaved=>rsx!{p {"Your playlist has unsaved changes. Save it before continuing, or discard the changes."}},
                    Dialog::SaveFailure=>rsx!{p {"The save failed. Cancel to keep the application open and retry, or quit without saving the latest settings."}},
                }
            }
            if matches!(dialog,Dialog::Preferences|Dialog::MidiSetup|Dialog::Unsaved|Dialog::SaveFailure) {
                div {class:"modal-actions",
                    button {id:"cancel-dialog",onclick:move |_|d.send(Command::CancelDialog),"Cancel"}
                    if dialog==Dialog::Preferences {button {id:"apply-preferences",class:"primary",onclick:move |_|d.send(Command::ApplyPreferences),"Apply"}}
                    if dialog==Dialog::MidiSetup {button {id:"test-note",onclick:move |_|d.send(Command::Action(Action::TestNote)),"Test note"}button {id:"apply-midi",class:"primary",onclick:move |_|d.send(Command::MidiApply),"Apply"}}
                    if dialog==Dialog::Unsaved {button {id:"discard",onclick:move |_|d.send(Command::ConfirmDiscard),"Discard"}button {id:"save-before-close",class:"primary",onclick:move |_|d.send(Command::Action(Action::SavePlaylist)),"Save playlist…"}}
                    if dialog==Dialog::SaveFailure {button {id:"quit-without-saving",onclick:move |_|d.send(Command::QuitWithoutSaving),"Quit without saving"}}
                }
            } else if !matches!(dialog,Dialog::Jump|Dialog::Loop) {div {class:"modal-actions",button {id:"done-dialog",onclick:move |_|d.send(Command::CancelDialog),"Done"}}}
        }
    }}
}
#[component]
fn PrefText(
    field: Preference,
    label: &'static str,
    value: String,
    #[props(default = "text")] kind: &'static str,
    #[props(default)] min: i32,
    #[props(default)] max: i32,
) -> Element {
    let d = dispatch();
    rsx! {label {class:"form-row",span {"{label}"}input {r#type:kind,value,min:min.to_string(),max:max.to_string(),oninput:move|e|d.send(Command::EditPreference(field,e.value()))}}}
}
#[component]
fn PrefBool(field: Preference, label: &'static str, value: bool) -> Element {
    let d = dispatch();
    rsx! {label {class:"form-row",span {"{label}"}input {r#type:"checkbox",checked:value,onchange:move|e|d.send(Command::EditPreference(field,e.checked().to_string()))}}}
}
#[component]
fn PrefSelect(
    field: Preference,
    label: &'static str,
    value: i32,
    options: Vec<&'static str>,
) -> Element {
    let d = dispatch();
    rsx! {label {class:"form-row",span {"{label}"}select {value:value.to_string(),onchange:move|e|d.send(Command::EditPreference(field,e.value())),for (index,label) in options.iter().enumerate(){option {value:index.to_string(),"{label}"}}}}}
}
#[component]
fn Preferences() -> Element {
    let d = dispatch();
    let settings = d.app.read().prefs.clone().unwrap_or_default();
    rsx! {
        h3 {"Playback"}
        PrefText {field:Preference::Drums,label:"Drums channel (1–16)",value:settings.drums_channel.to_string(),kind:"number",min:1,max:16}
        PrefText {field:Preference::SoloReduction,label:"Non-solo reduction (%)",value:settings.solo_volume_reduction.to_string(),kind:"number",min:0,max:100}
        PrefBool {field:Preference::Autoplay,label:"Play when opening a song",value:settings.auto_play}
        PrefBool {field:Preference::Advance,label:"Advance after the song ends",value:settings.auto_advance}
        PrefBool {field:Preference::SongSettings,label:"Automatically load and save song settings",value:settings.auto_song_settings}
        PrefSelect {field:Preference::Reset,label:"System Exclusive reset",value:settings.sysex_reset,options:vec!["None","General MIDI","Roland GS","Yamaha XG","Roland MT-32"]}
        PrefSelect {field:Preference::Map,label:"Instrument map",value:settings.instrument_map,options:vec!["General MIDI","GS / SC-55","MT-32"]}
        h3 {"Lyrics"}
        PrefText {field:Preference::LyricsFont,label:"Lyrics font (family and size)",value:settings.lyrics_font}
        PrefSelect {field:Preference::Alignment,label:"Text alignment",value:settings.text_alignment,options:vec!["Left","Center","Right"]}
        PrefText {field:Preference::FutureColor,label:"Future text color",value:settings.future_color}
        PrefText {field:Preference::PastColor,label:"Past text color",value:settings.past_color}
        PrefText {field:Preference::HighlightColor,label:"Current text color",value:settings.highlight_color}
        h3 {"Piano"}
        PrefText {field:Preference::NotesFont,label:"Note names font (family and size)",value:settings.notes_font}
        PrefSelect {field:Preference::Palette,label:"Highlight palette",value:settings.highlight_palette,options:vec!["Single color","Double","Channels","Scale"]}
        PrefText {field:Preference::SingleColor,label:"Single highlight color",value:settings.single_color}
        PrefBool {field:Preference::Velocity,label:"Vary color with note velocity",value:settings.velocity_color}
        PrefSelect {field:Preference::Names,label:"Show note names",value:settings.names_visibility,options:vec!["Never","C notes","Active notes","Always"]}
        PrefBool {field:Preference::Octave,label:"Use subscript octave numbers",value:settings.octave_subscript}
    }
}
#[component]
fn Midi() -> Element {
    let d = dispatch();
    let app = d.app.read();
    let Some(midi) = &app.midi_draft else {
        return rsx! {p {"Open MIDI Setup again."}};
    };
    let backend = midi.backend.clone();
    let port = midi.port.clone();
    let backends = midi.backends.clone();
    let ports = midi.ports.clone();
    let advanced = midi.advanced;
    let map = midi.instrument_map;
    let reset = midi.sysex_reset;
    let soundfont = midi.soundfont.clone();
    drop(app);
    rsx! {
        label {class:"form-row",span {"Output backend"}select {id:"midi-backend",value:backend,onchange:move|e|d.send(Command::MidiBackend(e.value())),for backend in backends {option {value:backend.clone(),"{backend}"}}}}
        label {class:"form-row",span {"Show all ports"}input {id:"midi-advanced",r#type:"checkbox",checked:advanced,onchange:move|e|d.send(Command::MidiAdvanced(e.checked()))}}
        label {class:"form-row",span {"MIDI destination"}select {id:"midi-port",value:port,onchange:move|e|d.send(Command::MidiPort(e.value())),for port in &ports {option {value:port.id.clone(),"{port.label}"}}}}
        button {id:"refresh-ports",onclick:move |_|d.send(Command::Action(Action::RefreshPorts)),"Refresh ports"}
        if ports.is_empty(){p {role:"status",class:"muted","No destinations. Connect a MIDI device or start your synthesizer, then refresh."}}
        label {class:"form-row",span {"Instrument map"}select {id:"midi-map",value:map.to_string(),onchange:move|e|d.send(Command::MidiMap(e.value().parse().unwrap_or(0))),for (index,label) in ["General MIDI","GS / SC-55","MT-32"].iter().enumerate(){option {value:index.to_string(),"{label}"}}}}
        label {class:"form-row",span {"System Exclusive reset"}select {id:"midi-reset",value:reset.to_string(),onchange:move|e|d.send(Command::MidiReset(e.value().parse().unwrap_or(0))),for (index,label) in ["None","General MIDI","GS","XG","MT-32"].iter().enumerate(){option {value:index.to_string(),"{label}"}}}}
        label {class:"form-row",span {"FluidSynth SoundFont"}input {id:"midi-soundfont",r#type:"text",value:soundfont.clone(),placeholder:"Optional .sf2 / .sf3 file",oninput:move|e|d.send(Command::MidiSoundfont(e.value()))}}
        button {id:"browse-soundfont",onclick:move |_|d.send(Command::Action(Action::BrowseSoundfont)),"Browse…"}
        if crate::state::midi::is_stale_soundfont(&soundfont){p {role:"alert","SoundFont not found. Choose an accessible .sf2 or .sf3 file."}}
        p {class:"muted","Apply stops playback before changing the output. Dummy is a silent output for testing."}
    }
}
#[component]
fn FileInfo() -> Element {
    let d = dispatch();
    let song = d.app.read().song.clone();
    if let Some(song) = song {
        rsx! {dl {class:"info",dt {"File"}dd {"{song.path.display()}"}dt {"Format"}dd {"{song.format}"}dt {"Tracks"}dd {"{song.num_tracks}"}dt {"Bars"}dd {"{song.bars}"}dt {"Duration"}dd {"{crate::state::playback::format_time_ms(song.duration_ms)}"}dt {"Division"}dd {"{song.division}"}dt {"Encoding"}dd {"{song.charset}"}for (key,value) in song.metadata {dt {"{key}"}dd {"{value}"}}}for copyright in song.copyrights{p {"{copyright}"}}}
    } else {
        rsx! {p {"Open a song to see its information."}}
    }
}
#[component]
fn About() -> Element {
    rsx! {
        h3 {"Gosh MIDI Player ",{env!("CARGO_PKG_VERSION")}}
        p {"A MIDI and karaoke player built with Rust and Dioxus Desktop."}
        p {"Based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas. Copyright © 2006–2026 Pedro López-Cabanillas and contributors."}
        p {"Licensed under GNU GPL version 3 or later."}
        p {"Original application: dmidiplayer — Drumstick MIDI File Player."}
        for (label,url) in crate::state::dialogs::ABOUT.links {NativeLink {label,url}}
    }
}
#[component]
fn NativeLink(label: &'static str, url: &'static str) -> Element {
    let d = dispatch();
    rsx! {p {a {href:url,onclick:move|e|{e.prevent_default();d.send(Command::OpenUrl(url.into()));},"{label}"}}}
}
#[component]
fn Jump() -> Element {
    let d = dispatch();
    let max = d
        .app
        .read()
        .song
        .as_ref()
        .map(|s| s.bars.max(1))
        .unwrap_or(1);
    let mut bar = use_signal(|| d.app.peek().playback.bar);
    rsx! {p {"Choose a bar from 1 to {max}."}label {class:"form-row",span {"Bar"}input {id:"jump-bar",r#type:"number",min:"1",max:max.to_string(),value:bar().to_string(),oninput:move|e|bar.set(e.value().parse().unwrap_or(1))}}div {class:"row",button {id:"cancel-jump",onclick:move |_|d.send(Command::CancelDialog),"Cancel"}button {id:"apply-jump",class:"primary",onclick:move |_|d.send(Command::Jump(bar())),"Jump"}}}
}
#[component]
fn Loop() -> Element {
    let d = dispatch();
    let max = d
        .app
        .read()
        .song
        .as_ref()
        .map(|s| s.bars.max(1))
        .unwrap_or(1);
    let mut start = use_signal(|| d.app.peek().playback.loop_start);
    let mut end = use_signal(|| d.app.peek().playback.loop_end);
    rsx! {p {"Repeat an inclusive bar range. Start must be at or before the end."}
        label {class:"form-row",span {"Start bar"}input {id:"loop-start",r#type:"number",min:"1",max:max.to_string(),value:start().to_string(),oninput:move|e|start.set(e.value().parse().unwrap_or(1))}}
        label {class:"form-row",span {"End bar"}input {id:"loop-end",r#type:"number",min:"1",max:max.to_string(),value:end().to_string(),oninput:move|e|end.set(e.value().parse().unwrap_or(1))}}
        div {class:"row",button {id:"cancel-loop",onclick:move |_|d.send(Command::CancelDialog),"Cancel"}button {id:"apply-loop",class:"primary",disabled:start()>end(),onclick:move |_|d.send(Command::Loop(start(),end())),"Enable loop"}}
    }
}
