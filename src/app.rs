//! T1 application shell: minimal `cosmic::Application` (arch §2.1–2.3).
//!
//! Header bar with the app name, an empty-state view, and a `Ready` status
//! line. Full state / `update()` / views land in T9.

use std::path::PathBuf;

use cosmic::app::{ContextDrawer, Core, CosmicFlags, Task};
use cosmic::iced::Subscription;
use cosmic::{Action, Application, Element};

use crate::message::Message;

/// Startup flags parsed from the CLI in `main.rs` (arch §2.5).
/// `backend`/`conn` are read by the MIDI setup wiring in T6.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// Existing files from CLI positionals (T9 opens them into the playlist).
    pub files: Vec<PathBuf>,
    /// Initial MIDI backend from `-d` (only when `-c` was also given; T6).
    pub backend: Option<String>,
    /// Initial MIDI connection from `-c` (only when `-d` was also given; T6).
    pub conn: Option<String>,
    /// String form of `files` for single-instance forwarding (`args()` must
    /// return `&str` borrowed from `self`).
    args_cache: Vec<String>,
}

impl Flags {
    pub fn new(files: Vec<PathBuf>, backend: Option<String>, conn: Option<String>) -> Self {
        let args_cache = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        Self {
            files,
            backend,
            conn,
            args_cache,
        }
    }
}

/// Subcommand sent when forwarding a second launch to the running instance.
static OPEN_COMMAND: &str = "open";

impl CosmicFlags for Flags {
    type SubCommand = &'static str;
    type Args = Vec<String>;

    fn action(&self) -> Option<&Self::SubCommand> {
        if self.files.is_empty() {
            None
        } else {
            Some(&OPEN_COMMAND)
        }
    }

    fn args(&self) -> Vec<&str> {
        self.args_cache.iter().map(String::as_str).collect()
    }
}

/// T1 application state: shell only. Full state (arch §2.1) lands in T9.
pub struct App {
    core: Core,
    /// Status line text (`Ready` until T9 wires transport state).
    status: String,
    /// Files delivered via CLI / single-instance forward (T9 opens them).
    pending_files: Vec<PathBuf>,
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;

    const APP_ID: &'static str = "com.goshapps.GoshMIDI";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let mut app = Self {
            core,
            status: "Ready".to_owned(),
            pending_files: flags.files,
        };
        app.core.set_header_title("Gosh MIDI Player".to_owned());
        (app, Task::none())
    }

    fn context_drawer(&self) -> Option<ContextDrawer<'_, Self::Message>> {
        None
    }

    /// Status line (T1: static `Ready`; T9 wires the 5 transport states).
    fn footer(&self) -> Option<Element<'_, Self::Message>> {
        Some(cosmic::widget::text::body(&self.status).into())
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        if let Message::OpenFiles(paths, _replace) = message {
            // T9/T10: open into the playlist (replace semantics per P2).
            self.pending_files = paths;
        }
        Task::none()
    }

    /// Empty-state view (T9 builds the playlist/transport/dashboard views).
    fn view(&self) -> Element<'_, Self::Message> {
        cosmic::widget::container(
            cosmic::widget::column::with_children([
                cosmic::widget::text::title3("Gosh MIDI Player").into(),
                cosmic::widget::text::body("No file loaded.").into(),
            ])
            .spacing(8)
            .align_x(cosmic::iced::Alignment::Center),
        )
        .center(cosmic::iced::Length::Fill)
        .into()
    }

    /// Second-launch / portal activation: forward files as
    /// `OpenFiles(paths, replace=true)` + present the window (arch §2.5,
    /// same contract as C++ `on_open` in `src/main.cpp`).
    ///
    /// No `cfg` gate: this crate unconditionally enables libcosmic's
    /// `single-instance` feature, so the trait method always exists.
    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;

        let paths: Vec<PathBuf> = match msg.msg {
            Details::Activate => Vec::new(),
            Details::Open { url } => url
                .into_iter()
                .filter_map(|u| u.to_file_path().ok())
                .collect(),
            Details::ActivateAction { action: _, args } => {
                args.into_iter().map(PathBuf::from).collect()
            }
        };

        let present = self
            .core
            .main_window_id()
            .map(cosmic::iced::window::gain_focus)
            .unwrap_or_else(Task::none);

        if paths.is_empty() {
            present
        } else {
            Task::batch([
                Task::done(Action::App(Message::OpenFiles(paths, true))),
                present,
            ])
        }
    }
}

// NOTE: `cosmic::ApplicationExt` (title, view_main, …) is blanket-implemented
// for all `Application`s — no manual impl. The header bar shows the title set
// via `set_header_title` in `init` above.
