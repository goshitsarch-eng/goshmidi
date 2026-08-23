/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#include "app/settings.hpp"
#include "ui/window.hpp"

#include <adwaita.h>
#include <filesystem>
#include <iostream>
#include <vector>

static dmidi::MainWindow* mainWindow = nullptr;

static dmidi::MainWindow* ensure_window(GtkApplication* app)
{
    if (!mainWindow) {
        mainWindow = new dmidi::MainWindow(ADW_APPLICATION(app));
        auto* backend = static_cast<std::string*>(g_object_get_data(G_OBJECT(app), "backend"));
        auto* conn = static_cast<std::string*>(g_object_get_data(G_OBJECT(app), "conn"));
        if (backend && conn && !backend->empty() && !conn->empty())
            mainWindow->connectOutput(*backend, *conn);
    }
    return mainWindow;
}

static void on_activate(GtkApplication* app, gpointer)
{
    auto* win = ensure_window(app);
    gtk_window_present(win->gtkWindow());
}

static void on_open(GtkApplication* app, GFile** files, gint nFiles, const gchar*, gpointer)
{
    std::vector<std::string> paths;
    paths.reserve(static_cast<std::size_t>(nFiles));
    for (gint i = 0; i < nFiles; ++i) {
        gchar* path = g_file_get_path(files[i]);
        if (path) {
            paths.emplace_back(path);
            g_free(path);
        }
    }

    auto* win = ensure_window(app);
    if (!paths.empty())
        win->openFiles(paths, true);
    gtk_window_present(win->gtkWindow());
}

int main(int argc, char** argv)
{
    std::vector<std::string> files;
    std::string backend, conn;
    bool portable = false;
    std::string portableFile;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "-h" || a == "--help") {
            std::cout << "Gosh MIDI Player (dmidiplayer) " << VERSION << "\n"
                      << "Based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.\n"
                      << "Usage: dmidiplayer [options] [midi_files]\n"
                      << "  -h, --help                 Show help\n"
                      << "  -v, --version              Show version\n"
                      << "  -p, --portable             Portable settings mode\n"
                      << "  -f, --file FILE            Portable settings file\n"
                      << "  -d, --driver DRIVER        MIDI output backend (ALSA, FluidSynth, Dummy)\n"
                      << "  -c, --connection PORT      MIDI output connection\n";
            return 0;
        }
        if (a == "-v" || a == "--version") {
            std::cout << "Gosh MIDI Player (dmidiplayer) " << VERSION << "\n"
                      << "Based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.\n";
            return 0;
        }
        if (a == "-p" || a == "--portable") {
            portable = true;
            continue;
        }
        if ((a == "-f" || a == "--file") && i + 1 < argc) {
            portable = true;
            portableFile = argv[++i];
            continue;
        }
        if ((a == "-d" || a == "--driver") && i + 1 < argc) {
            backend = argv[++i];
            continue;
        }
        if ((a == "-c" || a == "--connection") && i + 1 < argc) {
            conn = argv[++i];
            continue;
        }
        if (!a.empty() && a[0] != '-') {
            std::error_code ec;
            if (std::filesystem::exists(a, ec))
                files.push_back(std::filesystem::weakly_canonical(a).string());
            else
                std::cerr << "File not found: " << a << "\n";
        }
    }
    if (portable)
        dmidi::AppSettings::setPortable(portableFile);

    adw_init();
    dmidi::AppSettings::instance().load();

    AdwApplication* app = adw_application_new("com.goshapps.GoshMIDI", G_APPLICATION_HANDLES_OPEN);
    g_object_set_data_full(G_OBJECT(app), "backend", new std::string(backend),
                           [](gpointer p) { delete static_cast<std::string*>(p); });
    g_object_set_data_full(G_OBJECT(app), "conn", new std::string(conn),
                           [](gpointer p) { delete static_cast<std::string*>(p); });
    g_signal_connect(app, "activate", G_CALLBACK(on_activate), nullptr);
    g_signal_connect(app, "open", G_CALLBACK(on_open), nullptr);

    std::vector<std::string> applicationArgs{argv[0]};
    applicationArgs.insert(applicationArgs.end(), files.begin(), files.end());
    std::vector<char*> applicationArgv;
    applicationArgv.reserve(applicationArgs.size() + 1);
    for (auto& arg : applicationArgs)
        applicationArgv.push_back(arg.data());
    applicationArgv.push_back(nullptr);

    int status = g_application_run(G_APPLICATION(app), static_cast<int>(applicationArgs.size()),
                                   applicationArgv.data());
    g_object_unref(app);
    return status;
}
