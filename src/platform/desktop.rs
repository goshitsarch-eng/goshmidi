//! Native integrations; requests originate in the application controller.
use crate::{
    application::Effect,
    commands::{Action, Command, MENU},
    config::song::SongSettings,
    state::playlist::PlaylistState,
};
use dioxus::desktop::{
    DesktopContext,
    muda::{Menu, MenuItem, PredefinedMenuItem, Submenu},
};

pub fn menu() -> Result<(Menu, Vec<(Action, MenuItem)>), String> {
    let menu = Menu::new();
    let mut handles = Vec::new();
    #[cfg(target_os = "macos")]
    {
        let app = Submenu::new("Gosh MIDI Player", true);
        let about = MenuItem::with_id("about", "About Gosh MIDI Player", true, None);
        let preferences = MenuItem::with_id(
            "preferences",
            "Preferences…",
            true,
            Some(
                "CommandOrControl+,"
                    .parse()
                    .map_err(|e| format!("Invalid accelerator: {e}"))?,
            ),
        );
        let quit = MenuItem::with_id(
            "quit",
            "Quit Gosh MIDI Player",
            true,
            Some(
                "CommandOrControl+Q"
                    .parse()
                    .map_err(|e| format!("Invalid accelerator: {e}"))?,
            ),
        );
        app.append_items(&[
            &about,
            &PredefinedMenuItem::separator(),
            &preferences,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        menu.append(&app).map_err(|e| e.to_string())?;
        handles.extend([
            (Action::About, about),
            (Action::Preferences, preferences),
            (Action::Quit, quit),
        ]);
    }
    for group in ["File", "Playback", "Settings", "Help"] {
        let submenu = Submenu::new(group, true);
        for item in MENU.iter().filter(|entry| entry.group == group) {
            #[cfg(target_os = "macos")]
            if matches!(
                item.action,
                Action::About | Action::Preferences | Action::Quit
            ) {
                continue;
            }
            let accelerator = crate::commands::accelerator(item.action)
                .map(|key| format!("CommandOrControl+{key}").parse())
                .transpose()
                .map_err(|error| format!("Invalid menu accelerator: {error}"))?;
            let native = MenuItem::with_id(item.id, item.label, true, accelerator);
            submenu.append(&native).map_err(|e| e.to_string())?;
            handles.push((item.action, native));
        }
        menu.append(&submenu).map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        if group == "Help" {
            submenu.set_as_help_menu_for_nsapp();
        }
    }
    let edit = Submenu::new("Edit", true);
    edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ])
    .map_err(|e| e.to_string())?;
    menu.append(&edit).map_err(|e| e.to_string())?;
    Ok((menu, handles))
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|e| format!("Background operation failed: {e}"))?
}

async fn ensure_file_chooser() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    blocking(|| {
        use gtk::gio::prelude::DBusProxyExt;
        use gtk::glib::variant::ToVariant;
        let proxy = gtk::gio::DBusProxy::for_bus_sync(
            gtk::gio::BusType::Session,
            gtk::gio::DBusProxyFlags::NONE,
            None,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            None::<&gtk::gio::Cancellable>,
        )
        .map_err(|error| format!("Desktop session unavailable: {error}"))?;
        proxy
            .call_sync(
                "StartServiceByName",
                Some(&("org.freedesktop.portal.Desktop", 0u32).to_variant()),
                gtk::gio::DBusCallFlags::NONE,
                5000,
                None::<&gtk::gio::Cancellable>,
            )
            .map_err(|error| format!("File chooser portal unavailable: {error}"))?;
        let chooser = gtk::gio::DBusProxy::for_bus_sync(
            gtk::gio::BusType::Session,
            gtk::gio::DBusProxyFlags::NONE,
            None,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
            None::<&gtk::gio::Cancellable>,
        )
        .map_err(|error| format!("File chooser unavailable: {error}"))?;
        if chooser.cached_property("version").is_none() {
            return Err(
                "No file chooser portal backend is configured. Install a desktop portal provider."
                    .into(),
            );
        }
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn perform(
    effect: Effect,
    desktop: DesktopContext,
    playlist: PlaylistState,
) -> Result<Option<Command>, String> {
    match effect {
        Effect::Choose(action, directory) => {
            ensure_file_chooser().await?;
            let mut dialog = rfd::AsyncFileDialog::new().set_parent(&*desktop.window);
            if directory.is_dir() {
                dialog = dialog.set_directory(directory);
            }
            match action {
                Action::Open | Action::Add => {
                    let selected = dialog
                        .add_filter("MIDI music", &["mid", "midi", "kar", "rmi", "wrk"])
                        .set_title("Choose MIDI songs")
                        .pick_files()
                        .await;
                    Ok(selected.map(|files| {
                        Command::OpenFiles(
                            files
                                .into_iter()
                                .map(|file| file.path().to_path_buf())
                                .collect(),
                            action == Action::Add,
                        )
                    }))
                }
                Action::OpenPlaylist => {
                    if let Some(file) = dialog
                        .add_filter("MIDI playlist", &["lst"])
                        .pick_file()
                        .await
                    {
                        let path = file.path().to_path_buf();
                        let (path, items) = blocking(move || {
                            let mut loaded = PlaylistState::new();
                            if !loaded.load(&path) {
                                return Err(format!("Could not read playlist: {}", path.display()));
                            }
                            Ok((path, loaded.items().to_vec()))
                        })
                        .await?;
                        Ok(Some(Command::PlaylistLoaded(path, items, false)))
                    } else {
                        Ok(None)
                    }
                }
                Action::SavePlaylist => {
                    if let Some(file) = dialog
                        .add_filter("MIDI playlist", &["lst"])
                        .set_file_name("playlist.lst")
                        .save_file()
                        .await
                    {
                        let path = file.path().to_path_buf();
                        let path = blocking(move || {
                            let mut playlist = playlist;
                            if !playlist.save(&path) {
                                return Err(format!("Could not save playlist: {}", path.display()));
                            }
                            Ok(path)
                        })
                        .await?;
                        Ok(Some(Command::PlaylistSaved(path)))
                    } else {
                        Ok(None)
                    }
                }
                Action::BrowseSoundfont => Ok(dialog
                    .add_filter("SoundFont", &["sf2", "sf3"])
                    .pick_file()
                    .await
                    .map(|file| {
                        Command::MidiSoundfont(file.path().to_string_lossy().into_owned())
                    })),
                _ => Err("Unsupported file dialog request".into()),
            }
        }
        Effect::WriteSettings(paths, settings, appearance) => {
            blocking(move || {
                settings
                    .save(&paths)
                    .map_err(|e| format!("Could not save settings: {e}"))?;
                let data = serde_json::to_vec_pretty(&appearance).map_err(|e| e.to_string())?;
                super::persistence::write_atomic(&paths.config_dir().join("desktop.json"), data)
                    .map_err(|e| format!("Could not save appearance: {e}"))
            })
            .await?;
            Ok(None)
        }
        Effect::WriteSong(path, song) => {
            blocking(move || {
                song.save(&path)
                    .map_err(|e| format!("Could not save song settings: {e}"))
            })
            .await?;
            Ok(Some(Command::SaveCompleted("Song settings saved".into())))
        }
        Effect::ReadSong(song_path, candidates, manual) => {
            let result = blocking(move || {
                for (index, candidate) in candidates.into_iter().enumerate() {
                    if candidate.is_file() {
                        let data = std::fs::read_to_string(&candidate)
                            .map_err(|e| format!("Could not read {}: {e}", candidate.display()))?;
                        let saved = SongSettings::parse(&data).map_err(|e| {
                            format!("Invalid song settings in {}: {e}", candidate.display())
                        })?;
                        // A legacy filename collision must never apply another song's mix.
                        if index == 0
                            || saved.file.is_empty()
                            || std::path::Path::new(&saved.file) == song_path
                        {
                            return Ok(Command::SongSettingsLoaded(song_path, Box::new(saved)));
                        }
                    }
                }
                if manual {
                    Err("No matching song settings were found".into())
                } else {
                    Ok(Command::SongSettingsAbsent(song_path))
                }
            })
            .await?;
            Ok(Some(result))
        }
        Effect::Ports(backend, advanced) => {
            let result = blocking(move || {
                let ports = crate::application::enumerate_ports(&backend, advanced);
                Ok(Command::MidiPorts {
                    backend,
                    advanced,
                    ports,
                })
            })
            .await?;
            Ok(Some(result))
        }
        Effect::Clipboard(text) => {
            #[cfg(target_os = "linux")]
            {
                let display =
                    gtk::gdk::Display::default().ok_or("No desktop display for clipboard")?;
                if display.default_seat().is_none() {
                    return Err("The desktop session has no input seat for clipboard access".into());
                }
                let clipboard =
                    gtk::Clipboard::default(&display).ok_or("No system clipboard available")?;
                clipboard.set_text(&text);
                clipboard.store();
            }
            #[cfg(not(target_os = "linux"))]
            blocking(move || {
                static CLIPBOARD: std::sync::Mutex<Option<arboard::Clipboard>> =
                    std::sync::Mutex::new(None);
                let mut clipboard = CLIPBOARD.lock().map_err(|e| e.to_string())?;
                if clipboard.is_none() {
                    *clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
                }
                clipboard
                    .as_mut()
                    .ok_or("No clipboard")?
                    .set_text(text)
                    .map_err(|e| format!("Could not copy lyrics: {e}"))
            })
            .await?;
            Ok(Some(Command::SaveCompleted("Lyrics copied".into())))
        }
        Effect::SaveLyrics(text) => {
            ensure_file_chooser().await?;
            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_parent(&*desktop.window)
                .add_filter("Plain text", &["txt"])
                .set_file_name("lyrics.txt")
                .save_file()
                .await
            {
                let path = file.path().to_path_buf();
                blocking(move || {
                    super::persistence::write_atomic(&path, text)
                        .map_err(|e| format!("Could not save lyrics: {e}"))
                })
                .await?;
                Ok(Some(Command::SaveCompleted("Lyrics saved".into())))
            } else {
                Ok(None)
            }
        }
        Effect::Print => {
            desktop
                .webview
                .print()
                .map_err(|e| format!("Could not open print dialog: {e}"))?;
            Ok(None)
        }
        Effect::Url(url) => {
            blocking(move || {
                opener::open(&url).map_err(|e| format!("Could not open browser: {e}"))
            })
            .await?;
            Ok(None)
        }
        Effect::Reveal(path) => {
            blocking(move || {
                opener::reveal(path).map_err(|e| format!("Could not reveal file: {e}"))
            })
            .await?;
            Ok(None)
        }
        Effect::ReleaseAfter(delay, generation) => {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            Ok(Some(Command::PreviewExpired(generation)))
        }
        Effect::Close => {
            desktop.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
            desktop.close();
            Ok(None)
        }
    }
}
