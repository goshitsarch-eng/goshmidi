use super::dispatch;
use crate::{
    commands::{Action, Command, MENU},
    models::{RepeatMode, Status, Theme, View},
    state::{
        lyrics::{self, FontStyle},
        piano,
        playback::format_time_ms,
    },
};
use dioxus::prelude::*;

#[component]
pub fn Header() -> Element {
    let d = dispatch();
    let theme = d.app.read().appearance.theme;
    let recent = d.app.read().settings.recent_files.clone();
    rsx! {header {
        div {class:"brand",span {class:"brand-mark","♫"} "Gosh MIDI Player"}
        span {class:"spacer"}
        button {id:"open",onclick:move |_|d.send(Command::Action(Action::Open)),"Open…"}
        button {id:"midi-setup",onclick:move |_|d.send(Command::Action(Action::MidiSetup)),"MIDI Setup"}
        select {id:"theme",aria_label:"Appearance",value:match theme {Theme::System=>"system",Theme::Light=>"light",Theme::Dark=>"dark"},
            onchange:move|e|d.send(Command::Theme(match e.value().as_str(){"light"=>Theme::Light,"dark"=>Theme::Dark,_=>Theme::System})),
            option {value:"system","System"} option {value:"light","Light"} option {value:"dark","Dark"}
        }
        details {id:"application-menu",summary {aria_label:"Application menu","Menu"}
            div {class:"menu-popup",for group in ["File","Playback","Settings","Help"] {
                div {class:"menu-group","{group}"}
                for item in MENU.iter().filter(|item|item.group==group) {
                    button {"data-action":item.id,disabled:!d.app.read().action_enabled(item.action),onclick:move |_|{d.send(Command::Action(item.action));let _=document::eval("document.getElementById('application-menu').open=false");},"{item.label}"}
                }
            }
            if !recent.is_empty() {
                div {class:"menu-group","Recent songs"}
                for path in recent {
                    button {title:path.clone(),onclick:move |_|{d.send(Command::OpenFiles(vec![std::path::PathBuf::from(&path)],false));let _=document::eval("document.getElementById('application-menu').open=false");},"{path}"}
                }
            }}
        }
    }}
}
#[component]
pub fn Transport() -> Element {
    let d = dispatch();
    let app = d.app.read();
    let p = app.playback.clone();
    let loaded = app.song.is_some();
    let title = app
        .song
        .as_ref()
        .map(|song| song.title.clone())
        .unwrap_or_else(|| {
            if app.loading.is_some() {
                "Opening MIDI file…".into()
            } else {
                "Ready to play".into()
            }
        });
    let duration = app.song.as_ref().map(|s| s.duration_ms).unwrap_or_default();
    let last = app.playlist.at_last();
    let first = app.playlist.at_first();
    drop(app);
    rsx! {section {class:"transport",aria_label:"Playback controls",
        div {class:"song-line",div {class:"song-title","{title}"} span {class:"spacer"} span {class:"clock","{p.bpm:.1} bpm · Bar {p.bar}, beat {p.beat}/{p.beats_per_bar}"}
            span {class:"beats",aria_hidden:"true",for beat in 1..=p.beats_per_bar.min(16) {span {class:if p.status==Status::Playing && beat==p.beat {"beat active"}else{"beat"},"●"}}}
        }
        div {class:"toolbar",
            button {id:"previous",disabled:first,aria_label:"Previous song",onclick:move |_|d.send(Command::Action(Action::Previous)),"Previous"}
            button {id:"play",class:"primary",disabled:!loaded,onclick:move |_|d.send(Command::Action(Action::Play)),if p.status==Status::Playing {"Pause"}else{"Play"}}
            button {id:"stop",disabled:!loaded,onclick:move |_|d.send(Command::Action(Action::Stop)),"Stop"}
            button {id:"next",disabled:last,aria_label:"Next song",onclick:move |_|d.send(Command::Action(Action::Next)),"Next"}
            button {id:"rewind",disabled:!loaded,aria_label:"Rewind one bar",onclick:move |_|d.send(Command::Action(Action::Rewind)),"−1 bar"}
            button {id:"forward",disabled:!loaded,aria_label:"Forward one bar",onclick:move |_|d.send(Command::Action(Action::Forward)),"+1 bar"}
            button {id:"jump",disabled:!loaded,onclick:move |_|d.send(Command::Action(Action::Jump)),"Jump…"}
            button {id:"loop",class:if p.loop_enabled{"active"}else{""},disabled:!loaded,onclick:move |_|d.send(if p.loop_enabled {Command::LoopOff}else{Command::Action(Action::Loop)}),if p.loop_enabled {"Loop on"}else{"Loop…"}}
            label {r#for:"repeat","Repeat"} select {id:"repeat",value:p.repeat.as_i32().to_string(),onchange:move|e|d.send(Command::Repeat(RepeatMode::from_i32(e.value().parse().unwrap_or(0)))),option {value:"0","Off"} option {value:"1","Song"} option {value:"2","Playlist"}}
        }
        div {class:"seek",span {class:"clock","{format_time_ms(p.elapsed_ms)}"}
            input {id:"seek",r#type:"range",aria_label:"Song position",min:"0",max:"1000",step:"1",value:(p.position_fraction()*1000.).round().to_string(),disabled:!loaded,oninput:move|e|d.send(Command::Seek(e.value().parse::<f64>().unwrap_or(0.)/1000.))}
            span {class:"clock","{format_time_ms(duration)}"}
        }
        div {class:"adjustments",
            label {"Tempo" input {id:"tempo",r#type:"range",aria_label:"Tempo percent",min:"50",max:"200",value:p.tempo_factor_pct.to_string(),oninput:move|e|d.send(Command::Tempo(e.value().parse().unwrap_or(100)))} output {"{p.tempo_factor_pct}%"}}
            label {"Volume" input {id:"volume",r#type:"range",aria_label:"Master volume percent",min:"0",max:"200",value:p.volume_pct.to_string(),oninput:move|e|d.send(Command::Volume(e.value().parse().unwrap_or(100)))} output {"{p.volume_pct}%"}}
            label {r#for:"pitch","Pitch" input {id:"pitch",r#type:"number",min:"-12",max:"12",value:p.pitch_semitones.to_string(),oninput:move|e|d.send(Command::Pitch(e.value().parse().unwrap_or(0)))} "semitones"}
        }
    }}
}
#[component]
pub fn Playlist() -> Element {
    let d = dispatch();
    let app = d.app.read();
    let items = app.playlist.items().to_vec();
    let current = app.playlist.current_index();
    let dirty = app.playlist.dirty();
    drop(app);
    rsx! {aside {class:"playlist",aria_label:"Playlist",
        div {class:"section-head",h2 {"Playlist · {items.len()}"} if dirty {span {class:"muted",title:"Unsaved changes","●"}} button {id:"add",aria_label:"Add songs",onclick:move |_|d.send(Command::Action(Action::Add)),"+"}}
        div {class:"playlist-list",if items.is_empty() {div {class:"empty","Open songs or drop MIDI files here."}}
            for (index,path) in items.iter().enumerate() {
                div {class:if current==index as i32{"song-row current"}else{"song-row"},key:"{index}:{path.display()}",
                    button {class:"song-select","data-song":index,title:path.to_string_lossy().into_owned(),aria_current:if current==index as i32{"true"}else{"false"},onclick:move |_|d.send(Command::Select(index,false)),ondoubleclick:move |_|d.send(Command::Select(index,true)),"{path.file_name().unwrap_or_default().to_string_lossy()}"}
                    button {class:"remove",aria_label:format!("Remove song {}",index+1),onclick:move |_|d.send(Command::Remove(index)),"×"}
                }
            }
        }
        div {class:"playlist-actions",
            button {id:"open-playlist",onclick:move |_|d.send(Command::Action(Action::OpenPlaylist)),"Open list"}
            button {id:"save-playlist",onclick:move |_|d.send(Command::Action(Action::SavePlaylist)),"Save list"}
            button {id:"move-up",disabled:current<=0,aria_label:"Move selected song up",onclick:move |_|d.send(Command::Move(current.max(0) as usize,-1)),"↑"}
            button {id:"move-down",disabled:current<0||current>=items.len() as i32-1,aria_label:"Move selected song down",onclick:move |_|d.send(Command::Move(current.max(0) as usize,1)),"↓"}
            button {id:"shuffle",disabled:items.len()<2,onclick:move |_|d.send(Command::Action(Action::Shuffle)),"Shuffle"}
            button {id:"clear",disabled:items.is_empty(),onclick:move |_|d.send(Command::Action(Action::Clear)),"Clear"}
        }
    }}
}
#[component]
pub fn Workspace() -> Element {
    let d = dispatch();
    let view = d.app.read().view;
    rsx! {nav {class:"tabs",aria_label:"Song views",
        for (tab,label,id) in [(View::Lyrics,"Lyrics","view-lyrics"),(View::Channels,"Channels","view-channels"),(View::Piano,"Piano","view-piano")] {
            button {id,class:if view==tab{"active"}else{""},aria_pressed:if view==tab{"true"}else{"false"},onclick:move |_|d.send(Command::View(tab)),"{label}"}
        }
        span {class:"spacer"} button {id:"sidebar",aria_label:"Toggle playlist",onclick:move |_|d.send(Command::ToggleSidebar),"Playlist"}
    }
    match view {View::Lyrics=>rsx!{Lyrics {}},View::Channels=>rsx!{Channels {}},View::Piano=>rsx!{Piano {}}}
    }
}
fn css_color(value: &str, fallback: &str) -> String {
    lyrics::parse_gdk_rgba(value)
        .map(|c| {
            format!(
                "rgba({},{},{},{})",
                (c.r * 255.).round(),
                (c.g * 255.).round(),
                (c.b * 255.).round(),
                c.a
            )
        })
        .unwrap_or_else(|| fallback.into())
}
pub(super) fn font_style(spec: &str) -> String {
    let font = lyrics::parse_pango_font(spec);
    let family = font.first_family().replace(['\'', '"', ';', '{', '}'], "");
    let style = match font.style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
        FontStyle::Oblique => "oblique",
    };
    format!(
        "font-family:'{family}',sans-serif;font-size:{}px;font-weight:{};font-style:{style};",
        lyrics::font_size_px(font.size, 14.),
        font.weight
    )
}
#[component]
pub fn Lyrics() -> Element {
    let d = dispatch();
    let app = d.app.read();
    let state = app.lyrics.clone();
    let song = app.song.clone();
    let ticks = app.playback.position_ticks;
    let settings = app.settings.clone();
    drop(app);
    let tracks = song
        .as_ref()
        .map(|s| lyrics::track_options(&s.texts, s.num_tracks, &state.codec))
        .unwrap_or_else(|| vec!["All tracks".into()]);
    let alignment = match settings.text_alignment {
        1 => "center",
        2 => "right",
        _ => "left",
    };
    let style = format!(
        "{}text-align:{alignment};",
        font_style(&settings.lyrics_font)
    );
    let mut segments = Vec::new();
    if let Some(song) = &song {
        let records: Vec<_> = song
            .texts
            .iter()
            .filter(|rec| state.passes_filter(rec.track.max(0) as u32, rec.text_type as i32))
            .collect();
        let current = records.iter().rposition(|rec| rec.tick <= ticks);
        for (index, rec) in records.iter().enumerate() {
            let text = lyrics::display_record(
                rec,
                &state.codec,
                song.path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("kar")),
            );
            let color = if Some(index) == current {
                css_color(&settings.highlight_color, "var(--text)")
            } else if current.is_some_and(|current| index < current) {
                css_color(&settings.past_color, "var(--text)")
            } else {
                css_color(&settings.future_color, "var(--muted)")
            };
            segments.push((text, color, Some(index) == current));
        }
    }
    use_effect(move || {
        let app = d.app.read();
        let _ = app.playback.position_ticks;
        if app.lyrics.autoscroll {
            let _ = document::eval(
                "document.querySelector('[data-current-lyric=true]')?.scrollIntoView({block:'nearest'})",
            );
        }
    });
    rsx! {section {class:"page",aria_label:"Lyrics view",
        div {class:"page-tools",
            label {"Track" select {id:"lyric-track",value:state.track_filter.unwrap_or(0).to_string(),onchange:move|e|{let value=e.value().parse::<u32>().unwrap_or(0);d.send(Command::LyricTrack(if value==0{None}else{Some(value)}));},for (index,label) in tracks.iter().enumerate(){option {value:index.to_string(),"{label}"}}}}
            label {"Type" select {id:"lyric-type",value:state.type_filter.to_string(),onchange:move|e|d.send(Command::LyricType(e.value().parse().unwrap_or(0))),for (index,label) in lyrics::TYPE_OPTION_NAMES.iter().enumerate(){option {value:index.to_string(),"{label}"}}}}
            label {"Encoding" select {id:"lyric-codec",value:state.codec.clone(),onchange:move|e|d.send(Command::LyricCodec(e.value())),for codec in crate::core::sequence::Sequence::extra_codec_names(){option {value:codec,"{codec}"}}}}
            label {input {id:"autoscroll",r#type:"checkbox",checked:state.autoscroll,onchange:move|e|d.send(Command::Autoscroll(e.checked()))} "Follow lyrics"}
        }
        div {class:"page-tools",button {id:"copy-lyrics",disabled:state.body.is_empty(),onclick:move |_|d.send(Command::Action(Action::CopyLyrics)),"Copy"}button {id:"save-lyrics",disabled:state.body.is_empty(),onclick:move |_|d.send(Command::Action(Action::SaveLyrics)),"Save text…"}button {id:"print-lyrics",disabled:state.body.is_empty(),onclick:move |_|d.send(Command::Action(Action::PrintLyrics)),"Print…"}}
        if segments.is_empty(){div {class:"empty",h2 {if song.is_none(){"Your music, in MIDI."}else{"No text for these filters"}} p {if song.is_none(){"Open a MIDI or karaoke file to see lyrics, mix channels, and follow every note."}else{"Try All tracks or All types to see the file’s other text events."}}}}
        else {div {class:"lyrics",id:"lyrics-body",style,for (index,(text,color,current)) in segments.into_iter().enumerate(){span {key:"{index}",style:"color:{color}","data-current-lyric":current.to_string(),"{text}"}}}}
    }}
}
#[component]
pub fn Channels() -> Element {
    let d = dispatch();
    let rows = d.app.read().channels.clone();
    let map = i32::from(d.app.read().midi.instrument_map);
    let patches = crate::state::channels::patch_options(map);
    rsx! {section {class:"page",aria_label:"Channels view",
        p {class:"muted","Mix each channel independently. Lock keeps your selected instrument through program changes."}
        div {class:"channel-grid muted",span {"Ch"}span {"Name"}span {"Mute"}span {"Solo"}span {"Lock"}span {"Instrument"}span {"Level"}span {"Meter"}}
        for (index,row) in rows.iter().enumerate().filter(|(_,row)|row.used) {
            div {class:"channel-grid",key:"{index}","data-channel":index,
                span {"{index+1}"}input {r#type:"text",aria_label:format!("Channel {} name",index+1),value:row.label.clone(),oninput:move|e|d.send(Command::ChannelName(index as u8,e.value()))}
                label {input {r#type:"checkbox",aria_label:format!("Mute channel {}",index+1),checked:row.muted,onchange:move|e|d.send(Command::ChannelMute(index as u8,e.checked()))}}
                label {input {r#type:"checkbox",aria_label:format!("Solo channel {}",index+1),checked:row.soloed,onchange:move|e|d.send(Command::ChannelSolo(index as u8,e.checked()))}}
                label {input {r#type:"checkbox",aria_label:format!("Lock channel {}",index+1),checked:row.locked,onchange:move|e|d.send(Command::ChannelLock(index as u8,e.checked()))}}
                select {aria_label:format!("Channel {} instrument",index+1),value:row.patch.to_string(),onchange:move|e|d.send(Command::ChannelPatch(index as u8,e.value().parse().unwrap_or(0))),for (patch,label) in patches.iter().enumerate(){option {value:patch.to_string(),"{patch+1}. {label}"}}}
                label {input {r#type:"range",min:"0",max:"200",aria_label:format!("Channel {} level",index+1),value:row.level_pct.to_string(),oninput:move|e|d.send(Command::ChannelLevel(index as u8,e.value().parse().unwrap_or(100)))}output {"{row.level_pct}%"}}
                meter {min:"0",max:"1",value:row.level.to_string(),aria_label:format!("Channel {} activity",index+1)}
            }
        }
        if !rows.iter().any(|row|row.used){div {class:"empty","Open a song to see its MIDI channels."}}
    }}
}
#[component]
pub fn Piano() -> Element {
    let d = dispatch();
    let app = d.app.read();
    let piano = app.piano.clone();
    let song = app.song.clone();
    let settings = app.settings.clone();
    drop(app);
    let (lo, hi) = piano::drawn_range(
        piano.tighten,
        song.is_none(),
        song.as_ref().map(|s| s.lowest_note).unwrap_or(0),
        song.as_ref().map(|s| s.highest_note).unwrap_or(127),
    );
    let style = font_style(&settings.notes_font);
    rsx! {section {class:"page",aria_label:"Piano view",
        div {class:"page-tools",label {input {id:"tighten",r#type:"checkbox",checked:piano.tighten,onchange:move|e|d.send(Command::Tighten(e.checked()))}"Fit song range"}
            button {id:"show-all",onclick:move |_|d.send(Command::PianoShowAll),"Show all channels"}
            button {id:"hide-unused",disabled:song.is_none(),onclick:move |_|d.send(Command::PianoHideUnused),"Hide unused"}
            span {class:"muted","Hold a key to audition. Release stops the note."}
        }
        div {class:"piano-scroll",style,
            onpointerup:move |_|d.send(Command::PreviewOff),onpointercancel:move |_|d.send(Command::PreviewOff),onpointerleave:move |_|d.send(Command::PreviewOff),
            for ch in (0..16).filter(|ch|piano.visible[*ch]) {
                div {class:"piano-row",key:"{ch}",span {class:"piano-ch","{ch+1}"}
                    for note in lo..=hi {
                        PianoKey {ch:ch as u8,note,lit:piano.notes_on[ch][note as usize],velocity:piano.note_vel[ch][note as usize],palette:settings.highlight_palette,velocity_color:settings.velocity_color,single_color:settings.single_color.clone(),names:settings.names_visibility,subscript:settings.octave_subscript}
                    }
                }
            }
        }
    }}
}
#[component]
fn PianoKey(
    ch: u8,
    note: u8,
    lit: bool,
    velocity: u8,
    palette: i32,
    velocity_color: bool,
    single_color: String,
    names: i32,
    subscript: bool,
) -> Element {
    let d = dispatch();
    let color =
        piano::highlight_note_rgba(ch, note, velocity, palette, &single_color, velocity_color);
    let style = format!(
        "--note:rgba({},{},{},{})",
        color.r * 255.,
        color.g * 255.,
        color.b * 255.,
        color.a
    );
    let mut label = piano::note_label(note);
    if subscript {
        label = label
            .chars()
            .map(|c| match c {
                '0' => '₀',
                '1' => '₁',
                '2' => '₂',
                '3' => '₃',
                '4' => '₄',
                '5' => '₅',
                '6' => '₆',
                '7' => '₇',
                '8' => '₈',
                '9' => '₉',
                _ => c,
            })
            .collect();
    }
    rsx! {button {class:format!("key {} {}",if piano::note_is_black(note){"black"}else{""},if lit{"lit"}else{""}),style,
        aria_label:format!("Channel {}, {}",ch+1,piano::note_label(note)),"data-piano-note":note,"data-piano-channel":ch,
        onpointerdown:move|e|{e.prevent_default();d.send(Command::Preview(ch,note));},
        onpointerup:move |_|d.send(Command::PreviewOff),onblur:move |_|d.send(Command::PreviewOff),
        onkeydown:move|e:KeyboardEvent|if e.key()==Key::Enter||e.key()==Key::Character(" ".into()){e.prevent_default();d.send(Command::Preview(ch,note));},
        onkeyup:move |_|d.send(Command::PreviewOff),
        if piano::show_note_name(note,lit,names){"{label}"}
    }}
}
