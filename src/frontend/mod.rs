//! Dioxus Desktop presentation. Domain decisions live in `application`.
mod dialogs;
#[cfg(feature = "ui-test")]
mod qa;
mod views;
use crate::{
    application::{Controller, Effect},
    commands::{Command, MENU},
    config::paths::Paths,
    models::Dialog,
};
use dioxus::html::HasFileData;
use dioxus::{
    desktop::{
        self, Config, WindowBuilder, WindowCloseBehaviour,
        tao::{
            dpi::LogicalSize,
            event::{Event, WindowEvent},
        },
    },
    prelude::*,
};
use futures_util::StreamExt;
use std::path::PathBuf;

#[derive(Clone)]
struct Boot {
    paths: Paths,
    files: Vec<PathBuf>,
    output: Option<(String, String)>,
    instance: std::rc::Rc<crate::platform::instance::LocalInstance>,
    menu_items: Vec<(crate::commands::Action, desktop::muda::MenuItem)>,
}
#[derive(Clone, Copy)]
pub(crate) struct Dispatch {
    pub app: Signal<Controller>,
    effects: Coroutine<Vec<Effect>>,
}
impl Dispatch {
    pub fn send(mut self, command: Command) {
        let effects = self.app.write().dispatch(command);
        if !effects.is_empty() {
            self.effects.send(effects);
        }
    }
}
pub(crate) fn dispatch() -> Dispatch {
    use_context::<Dispatch>()
}

pub fn launch(paths: Paths, files: Vec<PathBuf>, output: Option<(String, String)>) -> ! {
    let instance = match crate::platform::instance::activate(paths.config_file(), files.clone()) {
        Ok(crate::platform::instance::Instance::Forwarded) => std::process::exit(0),
        Ok(crate::platform::instance::Instance::Primary(instance)) => std::rc::Rc::new(instance),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let settings = crate::config::settings::Settings::load(&paths);
    #[cfg(target_os = "linux")]
    {
        gtk::glib::set_prgname(Some("com.goshapps.GoshMIDI"));
        gtk::glib::set_application_name("Gosh MIDI Player");
    }
    let window = WindowBuilder::new()
        .with_title("Gosh MIDI Player")
        .with_inner_size(LogicalSize::new(
            settings.window_width.clamp(640, 7680),
            settings.window_height.clamp(480, 4320),
        ))
        .with_min_inner_size(LogicalSize::new(640, 480));
    let icon = image::load_from_memory_with_format(
        include_bytes!("../../icons/dmidiplayer_128x128.png"),
        image::ImageFormat::Png,
    )
    .ok()
    .and_then(|image| {
        let image = image.into_rgba8();
        let (width, height) = image.dimensions();
        desktop::tao::window::Icon::from_rgba(image.into_raw(), width, height).ok()
    });
    let window = window.with_window_icon(icon);
    let (menu, menu_items) = crate::platform::desktop::menu().unwrap_or_else(|error| {
        tracing::error!(%error,"Could not build native menu");
        (desktop::muda::Menu::new(), Vec::new())
    });
    let config = Config::new()
        .with_window(window)
        .with_menu(menu)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_data_directory(paths.data_dir().join("webview"))
        .with_disable_context_menu(!cfg!(debug_assertions))
        .with_navigation_handler(|url| {
            url::Url::parse(url).is_ok_and(|url| {
                url.scheme() == "dioxus"
                    || (matches!(url.scheme(), "http" | "https")
                        && url.host_str() == Some("dioxus.localhost"))
            })
        });
    let dom = VirtualDom::new(root);
    dom.provide_root_context(Boot {
        paths,
        files,
        output,
        instance,
        menu_items,
    });
    desktop::launch::launch_virtual_dom(dom, config)
}
fn root() -> Element {
    let boot = use_context::<Boot>();
    let mut initial = Vec::new();
    let mut app = use_signal(|| {
        let (app, effects) = Controller::new(boot.paths, boot.files, boot.output);
        initial = effects;
        app
    });
    let mut editing = use_signal(|| false);
    let menu_items = boot.menu_items.clone();
    use_effect(move || {
        let app = app.read();
        for (action, item) in &menu_items {
            item.set_enabled(
                app.action_enabled(*action)
                    && !(editing() && crate::commands::accelerator(*action).is_some()),
            );
        }
    });
    let window = desktop::use_window();
    let effect_window = window.clone();
    let effects = use_coroutine(move |mut receiver: UnboundedReceiver<Vec<Effect>>| {
        let window = effect_window.clone();
        async move {
            while let Some(batch) = receiver.next().await {
                let mut pending: std::collections::VecDeque<_> = batch.into();
                while let Some(effect) = pending.pop_front() {
                    let playlist = app.peek().playlist.clone();
                    match crate::platform::desktop::perform(effect, window.clone(), playlist).await
                    {
                        Ok(Some(command)) => {
                            let next = app.write().dispatch(command);
                            pending.extend(next);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let quitting =
                                pending.iter().any(|effect| matches!(effect, Effect::Close));
                            pending.retain(|effect| !matches!(effect, Effect::Close));
                            window.set_visible(true);
                            app.write().dispatch(if quitting {
                                Command::QuitSaveFailed(error)
                            } else {
                                Command::Error(error)
                            });
                        }
                    }
                }
            }
        }
    });
    let dispatcher = Dispatch { app, effects };
    use_context_provider(|| dispatcher);
    use_hook(|| {
        if !initial.is_empty() {
            effects.send(initial);
        }
        window.set_maximized(app.peek().appearance.maximized);
        if let Some((x, y)) = app.peek().appearance.position {
            let visible = window.available_monitors().any(|monitor| {
                let p = monitor.position();
                let size = monitor.size();
                x >= p.x
                    && y >= p.y
                    && x < p.x + size.width as i32 - 100
                    && y < p.y + size.height as i32 - 100
            });
            if visible {
                window.set_outer_position(desktop::tao::dpi::PhysicalPosition::new(x, y));
            }
        }
    });
    let instance = boot.instance.clone();
    let activation_window = window.clone();
    use_future(move || {
        let instance = instance.clone();
        let activation_window = activation_window.clone();
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                while let Some(files) = instance.take() {
                    activation_window.set_visible(true);
                    activation_window.set_minimized(false);
                    activation_window.set_focus();
                    if !files.is_empty() {
                        let next = app.write().dispatch(Command::OpenFiles(files, true));
                        if !next.is_empty() {
                            effects.send(next);
                        }
                    }
                }
                let events = app.peek().take_events();
                let active = app.peek().active();
                if !events.is_empty() || active {
                    let next = app.write().poll(events);
                    if !next.is_empty() {
                        effects.send(next);
                    }
                }
            }
        }
    });
    desktop::use_muda_event_handler(move |event| {
        if let Some(item) = MENU.iter().find(|entry| entry.id == event.id.0)
            && app.peek().action_enabled(item.action)
        {
            dispatcher.send(Command::Action(item.action));
        }
    });
    let events_window = window.clone();
    desktop::use_wry_event_handler(move |event, _| match event {
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => dispatcher.send(Command::Quit),
        Event::WindowEvent {
            event: WindowEvent::Resized(size),
            ..
        } => {
            let logical = size.to_logical::<u32>(events_window.scale_factor());
            dispatcher.send(Command::Geometry(
                logical.width,
                logical.height,
                events_window.is_maximized(),
            ));
        }
        Event::WindowEvent {
            event: WindowEvent::Moved(position),
            ..
        } if !events_window.is_maximized() => {
            dispatcher.send(Command::WindowPosition(position.x, position.y))
        }
        Event::WindowEvent {
            event: WindowEvent::Focused(false),
            ..
        } => dispatcher.send(Command::PreviewOff),
        Event::Opened { urls } => {
            let paths = urls
                .iter()
                .filter_map(|url| url.to_file_path().ok())
                .collect();
            dispatcher.send(Command::OpenFiles(paths, true));
        }
        _ => {}
    });
    use_effect(move || {
        if app.read().dialog == Some(Dialog::Unsaved) {
            desktop::window().set_visible(true);
            desktop::window().set_focus();
        }
    });
    // This small DOM bridge supplies the focused element information missing
    // from KeyboardData, so playback shortcuts never steal text editing keys.
    use_future(move || async move {
        let script = r#"
        const editingNow=()=>!!document.activeElement?.closest('input,textarea,select,[contenteditable=true]');
        document.addEventListener('focusin',()=>dioxus.send({focus:editingNow()}));
        document.addEventListener('focusout',()=>setTimeout(()=>dioxus.send({focus:editingNow()}),0));
        document.addEventListener('keydown',e=>{
          const modal=document.querySelector('[role=dialog]');
          if(modal&&e.key==='Tab') {
            const focusable=[...modal.querySelectorAll('button:not(:disabled),input:not(:disabled),select:not(:disabled),a[href]')];
            const first=focusable[0],last=focusable.at(-1);
            if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}
            else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}
          }
          if(e.key==='Escape'&&modal){e.preventDefault();dioxus.send({key:'Escape',primary:false,other:false,editing:false});return;}

          if(e.target.closest('[data-piano-note]')&&[' ','Enter'].includes(e.key))return;
          const editing=!!e.target.closest('input,textarea,select,[contenteditable=true]');
          const primary=navigator.platform.includes('Mac')?e.metaKey:e.ctrlKey;
          const other=e.altKey||e.shiftKey||(primary&&(navigator.platform.includes('Mac')?e.ctrlKey:e.metaKey));
          if(!editing&&!other&&!primary&&[' ','F1','Escape'].includes(e.key)) {
            e.preventDefault();dioxus.send({key:e.key,primary,other,editing});
          }
        });
        "#;
        let mut evaluation = document::eval(script);
        while let Ok(value) = evaluation.recv::<serde_json::Value>().await {
            if let Some(focus) = value["focus"].as_bool() {
                editing.set(focus);
                continue;
            }
            let key = value["key"].as_str().unwrap_or_default();
            if key == "Escape" {
                dispatcher.send(Command::CancelDialog);
                continue;
            }
            if let Some(action) = crate::commands::shortcut(
                key,
                value["primary"].as_bool().unwrap_or(false),
                value["other"].as_bool().unwrap_or(false),
                value["editing"].as_bool().unwrap_or(true),
                app.peek().dialog.is_some(),
            ) {
                dispatcher.send(Command::Action(action));
            }
        }
    });
    #[cfg(feature = "ui-test")]
    qa::use_qa(dispatcher);
    let theme = match app.read().appearance.theme {
        crate::models::Theme::System => "system",
        crate::models::Theme::Light => "light",
        crate::models::Theme::Dark => "dark",
    };
    rsx! {
        document::Title { "Gosh MIDI Player" }
        style { {include_str!("style.css")} }
        div { class:"app", "data-theme":theme,
            ondragover:move|event:DragEvent|event.prevent_default(),
            ondrop:move|event:DragEvent|{event.prevent_default();let files=event.files().into_iter().map(|file|file.path()).collect();dispatcher.send(Command::OpenFiles(files,true));},
            views::Header {}
            if let Some(error)=app.read().errors.first() { div {class:"error",role:"alert",span {"{error}"} button {aria_label:"Dismiss error",onclick:move |_|dispatcher.send(Command::DismissError),"×"} } }
            views::Transport {}
            div {class:"workspace", if app.read().settings.playlist_visible {views::Playlist {}} main {views::Workspace {}} }
            footer {role:"status",span {"{app.read().playback.status.text()}"} span {"{app.read().notice}"} }
            if let Some(dialog)=app.read().dialog {dialogs::Modal {dialog}}
        }
    }
}
