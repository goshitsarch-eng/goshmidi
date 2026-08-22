/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#include "window.hpp"

#include "../app/instruments.hpp"
#include "../app/settings.hpp"

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <pango/pangocairo.h>
#include <glib/gi18n.h>
#include <sstream>

namespace dmidi {
namespace {

const char* kHelpText =
    "dmidiplayer is a MIDI file player with lyrics, piano, channels, and playlists.\n\n"
    "Supported files: .mid .midi .kar .rmi .wrk\n\n"
    "Playback: Play, Pause, Stop, previous/next playlist item, jump to bar, loop between bars.\n"
    "Transpose with Pitch (-12..+12, percussion excluded). Tempo 50–200%. Volume 0–200% (CC7).\n\n"
    "Playlists are .lst text files with one path per line. Drag files onto the window to make a "
    "temporary playlist. Repeat can be off, current song, or whole playlist.\n\n"
    "MIDI output: ALSA sequencer ports or FluidSynth (SoundFont). Configure in MIDI Setup.\n\n"
    "Song settings can be stored in ~/.dmidiplayer/<song>.cfg (encoding, pitch, tempo, volume, "
    "per-channel mute/solo/lock/patch/level).\n";

GdkRGBA parseColor(const std::string& s)
{
    GdkRGBA c{0.5, 0.5, 0.5, 1};
    gdk_rgba_parse(&c, s.c_str());
    return c;
}

GtkWidget* iconButton(const char* icon, const char* tooltip, GCallback cb, gpointer data)
{
    GtkWidget* b = gtk_button_new_from_icon_name(icon);
    gtk_widget_set_tooltip_text(b, tooltip);
    gtk_widget_add_css_class(b, "flat");
    g_signal_connect(b, "clicked", cb, data);
    return b;
}

int alignmentFromSetting(int a)
{
    if (a == 1)
        return GTK_JUSTIFY_CENTER;
    if (a == 2)
        return GTK_JUSTIFY_RIGHT;
    return GTK_JUSTIFY_LEFT;
}

const char* kTypeNames[] = {"All types", "Text", "Copyright", "Track name", "Instrument",
                            "Lyrics",    "Marker", "Cue point", "KAR type", "KAR version",
                            "KAR info",  "KAR language", "KAR titles", "KAR other", nullptr};

bool noteIsBlack(int midi)
{
    int n = midi % 12;
    return n == 1 || n == 3 || n == 6 || n == 8 || n == 10;
}

struct PrintData {
    char* text{};
};

} // namespace

gboolean dmidi_drop_files(GtkDropTarget*, const GValue* value, double, double, gpointer data)
{
    auto* self = static_cast<MainWindow*>(data);
    if (!G_VALUE_HOLDS(value, GDK_TYPE_FILE_LIST))
        return false;
    auto* list = static_cast<GdkFileList*>(g_value_get_boxed(value));
    GSList* sl = gdk_file_list_get_files(list);
    std::vector<std::string> files;
    for (GSList* l = sl; l; l = l->next) {
        auto* f = static_cast<GFile*>(l->data);
        char* p = g_file_get_path(f);
        if (p) {
            if (isSupportedMidiFile(p) || g_str_has_suffix(p, ".lst"))
                files.emplace_back(p);
            g_free(p);
        }
    }
    if (!files.empty())
        self->openFiles(files, true);
    return !files.empty();
}

MainWindow::MainWindow(AdwApplication* app)
{
    for (int c = 0; c < kMidiChannels; ++c)
        m_pianoVisible[c] = true;
    m_repeat = AppSettings::instance().repeatMode;
    buildUi(app);
    bindActions(app);

    m_player.onTime = [this](auto ms, auto ticks) { updateTime(ms, ticks); };
    m_player.onBeat = [this](int bar, int beat, int max) {
        char pbuf[32];
        std::snprintf(pbuf, sizeof(pbuf), "%d:%d", bar, beat);
        gtk_label_set_text(m_posLabel, pbuf);
        gtk_widget_set_visible(GTK_WIDGET(m_rhythmBox), true);
        auto* child = gtk_widget_get_first_child(GTK_WIDGET(m_rhythmBox));
        int i = 1;
        while (child) {
            if (i > max)
                gtk_widget_set_visible(child, false);
            else {
                gtk_widget_set_visible(child, true);
                gtk_widget_remove_css_class(child, "on");
                gtk_widget_remove_css_class(child, "accent");
                if (i == beat)
                    gtk_widget_add_css_class(child, i == 1 ? "accent" : "on");
            }
            child = gtk_widget_get_next_sibling(child);
            ++i;
        }
        while (gtk_widget_get_first_child(GTK_WIDGET(m_rhythmBox)) == nullptr)
            break;
        int existing = 0;
        for (auto* c = gtk_widget_get_first_child(GTK_WIDGET(m_rhythmBox)); c;
             c = gtk_widget_get_next_sibling(c))
            existing++;
        while (existing < max) {
            GtkWidget* lamp = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
            gtk_widget_add_css_class(lamp, "rhythm-lamp");
            gtk_box_append(m_rhythmBox, lamp);
            ++existing;
        }
    };
    m_player.onNoteOn = [this](int ch, int note, int vel) {
        if (note >= 0 && note < 128) {
            m_notesOn[ch][note] = vel > 0;
            m_noteVel[ch][note] = vel;
        }
        m_chLevel[ch] = std::max(m_chLevel[ch], vel / 127.0);
        gtk_widget_queue_draw(GTK_WIDGET(m_piano));
    };
    m_player.onNoteOff = [this](int ch, int note, int) {
        if (note >= 0 && note < 128)
            m_notesOn[ch][note] = false;
        gtk_widget_queue_draw(GTK_WIDGET(m_piano));
    };
    m_player.onText = [this](int track, int type, int64_t, const std::vector<uint8_t>& text) {
        if (m_lyricTrackFilter > 0 && track != m_lyricTrackFilter)
            return;
        if (m_lyricTypeFilter > 0 && type != m_lyricTypeFilter
            && !(m_lyricTypeFilter == 0))
            return;
        auto buf = gtk_text_view_get_buffer(m_lyricsView);
        GtkTextIter start, end;
        gtk_text_buffer_get_bounds(buf, &start, &end);
        gtk_text_buffer_remove_tag_by_name(buf, "current", &start, &end);
        gtk_text_buffer_apply_tag_by_name(buf, "past", &start, &end);
        std::string s = m_player.song().decodeText(text);
        GtkTextIter ins;
        gtk_text_buffer_get_end_iter(buf, &ins);
        GtkTextMark* mark = gtk_text_buffer_create_mark(buf, nullptr, &ins, true);
        gtk_text_buffer_insert(buf, &ins, s.c_str(), -1);
        GtkTextIter a, b;
        gtk_text_buffer_get_iter_at_mark(buf, &a, mark);
        gtk_text_buffer_get_end_iter(buf, &b);
        gtk_text_buffer_apply_tag_by_name(buf, "current", &a, &b);
        gtk_text_view_scroll_mark_onscreen(m_lyricsView, mark);
        gtk_text_buffer_delete_mark(buf, mark);
    };
    m_player.onProgram = [this](int ch, int pgm) {
        if (m_chPatch[ch])
            gtk_drop_down_set_selected(m_chPatch[ch], static_cast<guint>(clampMidi(pgm)));
    };
    m_player.onTempo = [this](double tempo) {
        char buf[32];
        std::snprintf(buf, sizeof(buf), "%.1f bpm", tempoToBpm(tempo));
        gtk_label_set_text(m_tempoValue, buf);
    };
    m_player.onTimeSignature = [this](int, int num, int) {
        // rhythm lamps resized on next beat
        (void)num;
    };
    m_player.onStopped = [this] { setStatus("Stopped"); };
    m_player.onFinished = [this] {
        setStatus("Finished");
        if (m_repeat == 1) {
            m_player.resetPosition();
            play();
            return;
        }
        auto& st = AppSettings::instance();
        if (st.autoSongSettings)
            saveSongSettings();
        if (m_repeat == 2) {
            if (!m_playlist.selectNext())
                m_playlist.selectFirst();
            refreshPlaylistView();
            loadCurrent(true);
            return;
        }
        if (st.autoAdvance && !m_playlist.atLast()) {
            nextSong();
        }
    };
    m_player.onStarted = [this] { setStatus("Playing"); };

    m_meterTimer = g_timeout_add(50, [](gpointer data) -> gboolean {
        auto* self = static_cast<MainWindow*>(data);
        for (int i = 0; i < kMidiChannels; ++i) {
            self->m_chLevel[i] *= 0.82;
            if (self->m_chMeter[i])
                gtk_level_bar_set_value(self->m_chMeter[i], self->m_chLevel[i]);
        }
        return G_SOURCE_CONTINUE;
    }, this);

    auto& st = AppSettings::instance();
    if (!st.soundFont.empty())
        m_outputs.setSoundFont(st.soundFont);
    std::string backend = st.lastOutputBackend.empty() ? "FluidSynth" : st.lastOutputBackend;
    std::string port = st.lastOutputConnection;
    if (!m_outputs.select(backend, port))
        m_outputs.select("Dummy", "dummy");
    m_player.setOutput(m_outputs.current());
    m_player.setDrumsChannel(std::clamp(st.drumsChannel, 1, 16) - 1);
    m_player.setSysexReset(st.sysexReset);

    if (!st.lastPlayList.empty())
        m_playlist.load(st.lastPlayList);
    if (m_playlist.empty()) {
        std::vector<std::string> candidates = {
#ifdef DATADIR
            std::string(DATADIR) + "/dmidiplayer/examples.lst",
#endif
            std::string(g_get_user_data_dir()) + "/dmidiplayer/examples.lst",
            "/usr/share/dmidiplayer/examples.lst",
            (std::filesystem::current_path() / "examples" / "examples.lst").string(),
        };
        for (auto& c : candidates) {
            if (std::filesystem::exists(c) && m_playlist.load(c))
                break;
        }
        if (m_playlist.empty()) {
            std::filesystem::path ex = std::filesystem::current_path() / "examples";
            if (std::filesystem::exists(ex)) {
                for (auto& p : std::filesystem::directory_iterator(ex)) {
                    if (isSupportedMidiFile(p.path().string()))
                        m_playlist.add(p.path().string());
                }
            }
        }
    }
    refreshPlaylistView();
}

MainWindow::~MainWindow()
{
    if (m_meterTimer)
        g_source_remove(m_meterTimer);
    m_player.stop();
    auto& st = AppSettings::instance();
    int w = 0, h = 0;
    gtk_window_get_default_size(GTK_WINDOW(m_window), &w, &h);
    st.windowWidth = w;
    st.windowHeight = h;
    st.repeatMode = m_repeat;
    st.save();
}

void MainWindow::toast(const std::string& msg)
{
    adw_toast_overlay_add_toast(m_toasts, adw_toast_new(msg.c_str()));
}

void MainWindow::setStatus(const std::string& text)
{
    gtk_label_set_text(m_status, text.c_str());
}

void MainWindow::updateTime(std::chrono::milliseconds ms, int64_t ticks)
{
    int total = static_cast<int>(ms.count() / 1000);
    int h = total / 3600;
    int m = (total / 60) % 60;
    int s = total % 60;
    char buf[16];
    std::snprintf(buf, sizeof(buf), "%02d:%02d:%02d", h, m, s);
    gtk_label_set_text(m_timeLabel, buf);
    int len = m_player.song().songLengthTicks();
    if (!m_seeking && len > 0)
        gtk_range_set_value(GTK_RANGE(m_posScale), 1000.0 * ticks / len);
}

GdkRGBA MainWindow::channelColor(int ch, int vel) const
{
    static const char* pal[] = {"#e01b24", "#ff7800", "#f6d32d", "#33d17a", "#3584e4",
                                "#9141ac", "#c061cb", "#62a0ea", "#26a269", "#e5a50a",
                                "#c01c28", "#1c71d8", "#613583", "#241f31", "#5e5c64", "#f66151"};
    GdkRGBA c = parseColor(pal[ch & 15]);
    auto& st = AppSettings::instance();
    if (st.highlightPalette == 0)
        c = parseColor(st.singleColor);
    if (st.velocityColor) {
        double t = vel / 127.0;
        c.red *= 0.4 + 0.6 * t;
        c.green *= 0.4 + 0.6 * t;
        c.blue *= 0.4 + 0.6 * t;
    }
    return c;
}

void MainWindow::connectOutput(const std::string& backend, const std::string& port)
{
    m_player.stop();
    if (!m_outputs.select(backend, port)) {
        toast("Unable to open MIDI output");
        return;
    }
    m_player.setOutput(m_outputs.current());
    auto& st = AppSettings::instance();
    st.lastOutputBackend = backend;
    st.lastOutputConnection = port;
    if (m_outputs.current() && !m_outputs.current()->lastError().empty())
        toast(m_outputs.current()->lastError());
}

void MainWindow::openFiles(const std::vector<std::string>& files, bool replacePlaylist)
{
    if (files.empty())
        return;
    if (files.size() == 1 && g_str_has_suffix(files[0].c_str(), ".lst")) {
        if (m_playlist.load(files[0])) {
            AppSettings::instance().lastPlayList = files[0];
            refreshPlaylistView();
            loadCurrent(AppSettings::instance().autoPlay);
        }
        return;
    }
    if (replacePlaylist)
        m_playlist.clear();
    for (auto& f : files)
        m_playlist.add(f);
    m_playlist.selectFirst();
    refreshPlaylistView();
    loadCurrent(AppSettings::instance().autoPlay);
}

void MainWindow::loadCurrent(bool autoPlayNow)
{
    auto path = m_playlist.current();
    if (path.empty())
        return;
    m_player.stop();
    if (!m_player.loadFile(path)) {
        toast("Could not load " + std::filesystem::path(path).filename().string());
        if (m_player.song().errorsCount() > 0)
            toast("Warning: non-standard or damaged file");
        return;
    }
    AppSettings::instance().addRecent(path);
    AppSettings::instance().lastDirectory = std::filesystem::path(path).parent_path().string();
    gtk_label_set_text(m_songTitle, m_player.song().currentFile().c_str());
    gtk_window_set_title(GTK_WINDOW(m_window),
                         (std::string("dmidiplayer — ") + m_player.song().currentFile()).c_str());
    gtk_range_set_value(GTK_RANGE(m_posScale), 0);
    gtk_label_set_text(m_timeLabel, "00:00:00");
    char bpm[32];
    std::snprintf(bpm, sizeof(bpm), "%.1f bpm", m_player.initialBpm());
    gtk_label_set_text(m_tempoValue, bpm);
    std::fill(&m_notesOn[0][0], &m_notesOn[0][0] + kMidiChannels * 128, false);
    refreshChannels();
    refreshLyrics();
    gtk_widget_queue_draw(GTK_WIDGET(m_piano));
    if (AppSettings::instance().autoSongSettings)
        loadSongSettings();
    if (autoPlayNow)
        play();
    else
        setStatus("Stopped");
}

void MainWindow::play()
{
    if (m_player.song().empty()) {
        if (!m_playlist.empty())
            loadCurrent(true);
        return;
    }
    m_player.setOutput(m_outputs.current());
    m_player.play();
    setStatus("Playing");
}

void MainWindow::pause()
{
    if (m_player.isRunning()) {
        m_player.pause();
        setStatus("Paused");
    } else if (!m_player.song().empty()) {
        play();
    }
}

void MainWindow::stop(bool reset)
{
    m_player.stop();
    if (reset)
        m_player.resetPosition();
    gtk_range_set_value(GTK_RANGE(m_posScale), 0);
    gtk_label_set_text(m_timeLabel, "00:00:00");
    std::fill(&m_notesOn[0][0], &m_notesOn[0][0] + kMidiChannels * 128, false);
    gtk_widget_queue_draw(GTK_WIDGET(m_piano));
    setStatus("Stopped");
}

void MainWindow::nextSong()
{
    if (AppSettings::instance().autoSongSettings)
        saveSongSettings();
    if (m_playlist.selectNext()) {
        refreshPlaylistView();
        loadCurrent(true);
    }
}

void MainWindow::prevSong()
{
    if (m_playlist.selectPrev()) {
        refreshPlaylistView();
        loadCurrent(true);
    }
}

void MainWindow::applyTempo(int percent)
{
    m_player.song().setTempoFactor(percent / 100.0);
    char bpm[32];
    std::snprintf(bpm, sizeof(bpm), "%.1f bpm", m_player.currentBpm());
    gtk_label_set_text(m_tempoValue, bpm);
}

void MainWindow::applyVolume(int percent)
{
    m_player.setVolumeFactor(percent);
    char buf[16];
    std::snprintf(buf, sizeof(buf), "%d%%", percent);
    gtk_label_set_text(m_volumeValue, buf);
}

void MainWindow::applyPitch(int semis)
{
    m_player.setPitchShift(semis);
}

void MainWindow::applyChannelSoloMute()
{
    bool anySolo = false;
    for (int i = 0; i < kMidiChannels; ++i)
        anySolo = anySolo || m_chSoloed[i];
    double reduce = (100 - AppSettings::instance().soloVolumeReduction) / 100.0;
    for (int i = 0; i < kMidiChannels; ++i) {
        bool mute = m_chMute[i] && gtk_toggle_button_get_active(m_chMute[i]);
        m_player.setMuted(i, mute);
        double factor = 1.0;
        if (anySolo && !m_chSoloed[i])
            factor = reduce;
        if (m_chVol[i])
            factor *= gtk_range_get_value(GTK_RANGE(m_chVol[i])) / 100.0;
        m_player.setVolume(i, factor);
    }
}

void MainWindow::refreshPlaylistView()
{
    GtkWidget* row;
    while ((row = gtk_widget_get_first_child(GTK_WIDGET(m_playListBox))))
        gtk_list_box_remove(m_playListBox, row);
    int i = 0;
    for (auto& item : m_playlist.items()) {
        auto name = std::filesystem::path(item).filename().string();
        GtkWidget* r = gtk_list_box_row_new();
        GtkWidget* lab = gtk_label_new(name.c_str());
        gtk_label_set_xalign(GTK_LABEL(lab), 0);
        gtk_label_set_ellipsize(GTK_LABEL(lab), PANGO_ELLIPSIZE_MIDDLE);
        gtk_list_box_row_set_child(GTK_LIST_BOX_ROW(r), lab);
        g_object_set_data(G_OBJECT(r), "index", GINT_TO_POINTER(i));
        gtk_list_box_append(m_playListBox, r);
        if (i == m_playlist.currentIndex())
            gtk_list_box_select_row(m_playListBox, GTK_LIST_BOX_ROW(r));
        ++i;
    }
}

void MainWindow::refreshChannels()
{
    auto& song = m_player.song();
    GtkStringList* patches = gtk_string_list_new(nullptr);
    for (int p = 0; p < 128; ++p)
        gtk_string_list_append(patches, gmPatchName(p));
    for (int i = 0; i < kMidiChannels; ++i) {
        bool used = song.channelUsed(i);
        gtk_widget_set_visible(m_chRow[i], used);
        if (!used)
            continue;
        gtk_editable_set_text(GTK_EDITABLE(m_chName[i]), song.channelLabel(i).c_str());
        gtk_drop_down_set_model(m_chPatch[i], G_LIST_MODEL(patches));
        m_pianoVisible[i] = true;
        if (m_pianoShow[i])
            gtk_check_button_set_active(m_pianoShow[i], true);
    }
}

void MainWindow::refreshLyrics()
{
    if (m_refreshingLyrics)
        return;
    m_refreshingLyrics = true;
    auto& song = m_player.song();
    GtkStringList* tracks = gtk_string_list_new(nullptr);
    gtk_string_list_append(tracks, "All tracks");
    for (int t = 1; t <= song.getNumTracks(); ++t) {
        std::string n = "Track " + std::to_string(t);
        auto tn = song.trackName(t);
        if (!tn.empty())
            n += " — " + tn;
        gtk_string_list_append(tracks, n.c_str());
    }
    gtk_drop_down_set_model(m_lyricTrack, G_LIST_MODEL(tracks));
    int best = song.trackMaxPoints();
    gtk_drop_down_set_selected(m_lyricTrack, best > 0 ? static_cast<guint>(best) : 0);
    m_lyricTrackFilter = best > 0 ? best : -1;
    int bestType = song.typeMaxPoints();
    if (bestType < 0)
        bestType = 5;
    gtk_drop_down_set_selected(m_lyricType, static_cast<guint>(bestType));
    m_lyricTypeFilter = bestType;

    GtkStringList* codecs = gtk_string_list_new(nullptr);
    auto names = Sequence::extraCodecNames();
    guint sel = 0, idx = 0;
    for (auto& n : names) {
        gtk_string_list_append(codecs, n.c_str());
        if (g_ascii_strcasecmp(n.c_str(), song.currentCharset().c_str()) == 0)
            sel = idx;
        ++idx;
    }
    gtk_drop_down_set_model(m_lyricCodec, G_LIST_MODEL(codecs));
    gtk_drop_down_set_selected(m_lyricCodec, sel);

    auto buf = gtk_text_view_get_buffer(m_lyricsView);
    gtk_text_buffer_set_text(buf, "", -1);
    GtkTextIter it;
    gtk_text_buffer_get_start_iter(buf, &it);
    int track = m_lyricTrackFilter;
    TextType type = static_cast<TextType>(m_lyricTypeFilter);
    for (auto& rec : song.textEvents()) {
        if (track > 0 && rec.track != track)
            continue;
        if (m_lyricTypeFilter > 0 && rec.type != type)
            continue;
        auto s = song.decodeText(rec.text);
        gtk_text_buffer_insert(buf, &it, s.c_str(), -1);
        if (!s.empty() && s.back() != '\n' && rec.type != TextType::Lyric)
            gtk_text_buffer_insert(buf, &it, "\n", -1);
    }
    auto& st = AppSettings::instance();
    auto future = parseColor(st.futureColor);
    auto past = parseColor(st.pastColor);
    auto hi = parseColor(st.highlightColor);
    GtkTextTagTable* table = gtk_text_buffer_get_tag_table(buf);
    auto ensure = [&](const char* name, const GdkRGBA& c) {
        GtkTextTag* tag = gtk_text_tag_table_lookup(table, name);
        if (!tag) {
            tag = gtk_text_tag_new(name);
            gtk_text_tag_table_add(table, tag);
        }
        g_object_set(tag, "foreground-rgba", &c, nullptr);
    };
    ensure("future", future);
    ensure("past", past);
    ensure("current", hi);
    GtkTextIter a, b;
    gtk_text_buffer_get_bounds(buf, &a, &b);
    gtk_text_buffer_apply_tag_by_name(buf, "future", &a, &b);
    gtk_text_view_set_justification(m_lyricsView, static_cast<GtkJustification>(alignmentFromSetting(st.textAlignment)));
    auto* css = gtk_css_provider_new();
    std::string cssText = ".lyrics-view { font: " + st.lyricsFont + "; }";
    gtk_css_provider_load_from_string(css, cssText.c_str());
    gtk_style_context_add_provider_for_display(gdk_display_get_default(), GTK_STYLE_PROVIDER(css),
                                               GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    m_refreshingLyrics = false;
}

void MainWindow::saveSongSettings()
{
    auto name = m_player.song().currentFile();
    if (name.empty())
        return;
    GKeyFile* kf = g_key_file_new();
    g_key_file_set_string(kf, "Global", "file", m_player.song().currentFullFileName().c_str());
    g_key_file_set_string(kf, "Global", "encoding", m_player.song().currentCharset().c_str());
    g_key_file_set_integer(kf, "Global", "volume", static_cast<int>(gtk_range_get_value(GTK_RANGE(m_volumeScale))));
    g_key_file_set_integer(kf, "Global", "pitch", gtk_spin_button_get_value_as_int(m_pitch));
    g_key_file_set_integer(kf, "Global", "timeskew", static_cast<int>(gtk_range_get_value(GTK_RANGE(m_tempoScale))));
    for (int i = 0; i < kMidiChannels; ++i) {
        if (!m_player.song().channelUsed(i))
            continue;
        char grp[32];
        std::snprintf(grp, sizeof(grp), "MIDI Channel %2d", i + 1);
        g_key_file_set_string(kf, grp, "name", gtk_editable_get_text(GTK_EDITABLE(m_chName[i])));
        g_key_file_set_boolean(kf, grp, "muted", gtk_toggle_button_get_active(m_chMute[i]));
        g_key_file_set_boolean(kf, grp, "solo", m_chSoloed[i]);
        g_key_file_set_boolean(kf, grp, "locked", gtk_toggle_button_get_active(m_chLock[i]));
        g_key_file_set_integer(kf, grp, "patch", static_cast<int>(gtk_drop_down_get_selected(m_chPatch[i])));
        g_key_file_set_integer(kf, grp, "level", static_cast<int>(gtk_range_get_value(GTK_RANGE(m_chVol[i]))));
    }
    gsize len = 0;
    char* data = g_key_file_to_data(kf, &len, nullptr);
    g_file_set_contents(AppSettings::songSettingsPath(name).c_str(), data, static_cast<gssize>(len), nullptr);
    g_free(data);
    g_key_file_free(kf);
}

void MainWindow::loadSongSettings()
{
    auto name = m_player.song().currentFile();
    if (name.empty())
        return;
    GKeyFile* kf = g_key_file_new();
    if (!g_key_file_load_from_file(kf, AppSettings::songSettingsPath(name).c_str(), G_KEY_FILE_NONE, nullptr)) {
        g_key_file_free(kf);
        return;
    }
    char* enc = g_key_file_get_string(kf, "Global", "encoding", nullptr);
    if (enc) {
        m_player.song().setCurrentCharset(enc);
        g_free(enc);
        refreshLyrics();
    }
    int vol = g_key_file_get_integer(kf, "Global", "volume", nullptr);
    if (vol)
        gtk_range_set_value(GTK_RANGE(m_volumeScale), vol);
    gtk_spin_button_set_value(m_pitch, g_key_file_get_integer(kf, "Global", "pitch", nullptr));
    int skew = g_key_file_get_integer(kf, "Global", "timeskew", nullptr);
    if (skew)
        gtk_range_set_value(GTK_RANGE(m_tempoScale), skew);
    for (int i = 0; i < kMidiChannels; ++i) {
        char grp[32];
        std::snprintf(grp, sizeof(grp), "MIDI Channel %2d", i + 1);
        if (!g_key_file_has_group(kf, grp))
            continue;
        char* nm = g_key_file_get_string(kf, grp, "name", nullptr);
        if (nm) {
            gtk_editable_set_text(GTK_EDITABLE(m_chName[i]), nm);
            g_free(nm);
        }
        gtk_toggle_button_set_active(m_chMute[i], g_key_file_get_boolean(kf, grp, "muted", nullptr));
        m_chSoloed[i] = g_key_file_get_boolean(kf, grp, "solo", nullptr);
        gtk_toggle_button_set_active(m_chSolo[i], m_chSoloed[i]);
        gtk_toggle_button_set_active(m_chLock[i], g_key_file_get_boolean(kf, grp, "locked", nullptr));
        int pgm = g_key_file_get_integer(kf, grp, "patch", nullptr);
        gtk_drop_down_set_selected(m_chPatch[i], static_cast<guint>(clampMidi(pgm)));
        m_player.setLocked(i, gtk_toggle_button_get_active(m_chLock[i]));
        m_player.setPatch(i, pgm);
        int lvl = g_key_file_get_integer(kf, grp, "level", nullptr);
        if (lvl)
            gtk_range_set_value(GTK_RANGE(m_chVol[i]), lvl);
    }
    applyChannelSoloMute();
    g_key_file_free(kf);
}

void MainWindow::openDialog()
{
    GtkFileDialog* dlg = gtk_file_dialog_new();
    gtk_file_dialog_set_title(dlg, "Open MIDI files");
    GtkFileFilter* flt = gtk_file_filter_new();
    gtk_file_filter_set_name(flt, "MIDI / Karaoke / Cakewalk / Playlist");
    gtk_file_filter_add_pattern(flt, "*.mid");
    gtk_file_filter_add_pattern(flt, "*.midi");
    gtk_file_filter_add_pattern(flt, "*.kar");
    gtk_file_filter_add_pattern(flt, "*.rmi");
    gtk_file_filter_add_pattern(flt, "*.wrk");
    gtk_file_filter_add_pattern(flt, "*.lst");
    GListStore* filters = g_list_store_new(GTK_TYPE_FILE_FILTER);
    g_list_store_append(filters, flt);
    gtk_file_dialog_set_filters(dlg, G_LIST_MODEL(filters));
    gtk_file_dialog_open_multiple(dlg, GTK_WINDOW(m_window), nullptr,
                                  +[](GObject* src, GAsyncResult* res, gpointer data) {
                                      auto* self = static_cast<MainWindow*>(data);
                                      GError* err = nullptr;
                                      GListModel* files =
                                          gtk_file_dialog_open_multiple_finish(GTK_FILE_DIALOG(src), res, &err);
                                      if (!files)
                                          return;
                                      std::vector<std::string> paths;
                                      for (guint i = 0; i < g_list_model_get_n_items(files); ++i) {
                                          auto* f = static_cast<GFile*>(g_list_model_get_item(files, i));
                                          char* p = g_file_get_path(f);
                                          if (p) {
                                              paths.emplace_back(p);
                                              g_free(p);
                                          }
                                          g_object_unref(f);
                                      }
                                      self->openFiles(paths, true);
                                      g_object_unref(files);
                                  },
                                  this);
}

void MainWindow::showFileInfo()
{
    std::ostringstream ss;
    auto& song = m_player.song();
    if (song.currentFile().empty())
        ss << "<b>No file loaded</b>";
    else {
        ss << "File: <b>" << song.currentFile() << "</b>\n";
        ss << "Format: <b>" << song.fileFormat() << "</b>\n";
        ss << "Duration: <b>" << song.durationString() << "</b>\n";
        char t[32];
        std::snprintf(t, sizeof(t), "%.1f", song.initialTempo() > 0 ? tempoToBpm(song.initialTempo()) : 120);
        ss << "Initial tempo: <b>" << t << " bpm</b>\n";
        ss << "Tracks: <b>" << song.getNumTracks() << "</b>\n";
        ss << "Division: <b>" << song.division() << "</b>\n";
        ss << song.metadataInfo();
        auto copy = song.getText(TextType::Copyright);
        for (auto& c : copy)
            ss << "Copyright: <b>" << c << "</b>\n";
    }
    AdwDialog* d = ADW_DIALOG(adw_alert_dialog_new("File information", nullptr));
    adw_alert_dialog_set_body(ADW_ALERT_DIALOG(d), ss.str().c_str());
    adw_alert_dialog_add_response(ADW_ALERT_DIALOG(d), "ok", "OK");
    adw_dialog_present(d, GTK_WIDGET(m_window));
}

void MainWindow::showJump()
{
    if (m_player.song().empty())
        return;
    GtkWidget* dlg = gtk_window_new();
    gtk_window_set_transient_for(GTK_WINDOW(dlg), GTK_WINDOW(m_window));
    gtk_window_set_modal(GTK_WINDOW(dlg), true);
    gtk_window_set_title(GTK_WINDOW(dlg), "Jump to bar");
    GtkWidget* box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
    gtk_widget_set_margin_top(box, 18);
    gtk_widget_set_margin_bottom(box, 18);
    gtk_widget_set_margin_start(box, 18);
    gtk_widget_set_margin_end(box, 18);
    gtk_window_set_child(GTK_WINDOW(dlg), box);
    gtk_box_append(GTK_BOX(box), gtk_label_new("Bar number:"));
    GtkWidget* spin = gtk_spin_button_new_with_range(1, std::max(1, m_player.song().lastBar()), 1);
    gtk_box_append(GTK_BOX(box), spin);
    GtkWidget* btn = gtk_button_new_with_label("Jump");
    gtk_box_append(GTK_BOX(box), btn);
    g_signal_connect(btn, "clicked", (GCallback)(+[](GtkButton*, gpointer data) {
                         auto** pack = static_cast<gpointer*>(data);
                         auto* self = static_cast<MainWindow*>(pack[0]);
                         auto* sp = GTK_SPIN_BUTTON(pack[1]);
                         auto* w = GTK_WINDOW(pack[2]);
                         int bar = gtk_spin_button_get_value_as_int(sp);
                         bool was = self->m_player.isRunning();
                         if (was)
                             self->m_player.pause();
                         self->m_player.jumpToBar(bar);
                         if (was)
                             self->play();
                         gtk_window_destroy(w);
                     }),
                     g_new0(gpointer, 3)); // leak-ish; use object data instead
    // store pointers on button
    g_object_set_data(G_OBJECT(btn), "self", this);
    g_object_set_data(G_OBJECT(btn), "spin", spin);
    g_object_set_data(G_OBJECT(btn), "win", dlg);
    g_signal_handlers_disconnect_by_data(btn, nullptr);
    g_signal_connect(btn, "clicked", (GCallback)(+[](GtkButton* b, gpointer) {
                         auto* self = static_cast<MainWindow*>(g_object_get_data(G_OBJECT(b), "self"));
                         auto* sp = GTK_SPIN_BUTTON(g_object_get_data(G_OBJECT(b), "spin"));
                         auto* w = GTK_WINDOW(g_object_get_data(G_OBJECT(b), "win"));
                         int bar = gtk_spin_button_get_value_as_int(sp);
                         bool was = self->m_player.isRunning();
                         if (was)
                             self->m_player.pause();
                         self->m_player.jumpToBar(bar);
                         if (was)
                             self->play();
                         gtk_window_destroy(w);
                     }),
                     nullptr);
    gtk_window_present(GTK_WINDOW(dlg));
}

void MainWindow::showLoop()
{
    if (m_player.song().empty())
        return;
    if (gtk_toggle_button_get_active(m_loopBtn) == false) {
        m_player.setLoop(false);
        return;
    }
    GtkWidget* dlg = gtk_window_new();
    gtk_window_set_transient_for(GTK_WINDOW(dlg), GTK_WINDOW(m_window));
    gtk_window_set_modal(GTK_WINDOW(dlg), true);
    gtk_window_set_title(GTK_WINDOW(dlg), "Loop");
    GtkWidget* box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
    gtk_widget_set_margin_top(box, 18);
    gtk_widget_set_margin_bottom(box, 18);
    gtk_widget_set_margin_start(box, 18);
    gtk_widget_set_margin_end(box, 18);
    gtk_window_set_child(GTK_WINDOW(dlg), box);
    int last = std::max(1, m_player.song().lastBar());
    GtkWidget* from = gtk_spin_button_new_with_range(1, last, 1);
    GtkWidget* to = gtk_spin_button_new_with_range(1, last, 1);
    gtk_spin_button_set_value(GTK_SPIN_BUTTON(from), m_player.loopStart());
    gtk_spin_button_set_value(GTK_SPIN_BUTTON(to), m_player.loopEnd());
    gtk_box_append(GTK_BOX(box), gtk_label_new("From bar"));
    gtk_box_append(GTK_BOX(box), from);
    gtk_box_append(GTK_BOX(box), gtk_label_new("To bar"));
    gtk_box_append(GTK_BOX(box), to);
    GtkWidget* ok = gtk_button_new_with_label("Loop");
    gtk_box_append(GTK_BOX(box), ok);
    g_object_set_data(G_OBJECT(ok), "self", this);
    g_object_set_data(G_OBJECT(ok), "from", from);
    g_object_set_data(G_OBJECT(ok), "to", to);
    g_object_set_data(G_OBJECT(ok), "win", dlg);
    g_signal_connect(ok, "clicked", (GCallback)(+[](GtkButton* b, gpointer) {
                         auto* self = static_cast<MainWindow*>(g_object_get_data(G_OBJECT(b), "self"));
                         int a = gtk_spin_button_get_value_as_int(GTK_SPIN_BUTTON(g_object_get_data(G_OBJECT(b), "from")));
                         int c = gtk_spin_button_get_value_as_int(GTK_SPIN_BUTTON(g_object_get_data(G_OBJECT(b), "to")));
                         self->m_player.setLoop(a, c);
                         gtk_toggle_button_set_active(self->m_loopBtn, true);
                         gtk_window_destroy(GTK_WINDOW(g_object_get_data(G_OBJECT(b), "win")));
                     }),
                     nullptr);
    g_signal_connect(dlg, "close-request", (GCallback)(+[](GtkWindow* w, gpointer data) -> gboolean {
                         auto* self = static_cast<MainWindow*>(data);
                         if (!self->m_player.loopEnabled())
                             gtk_toggle_button_set_active(self->m_loopBtn, false);
                         gtk_window_destroy(w);
                         return true;
                     }),
                     this);
    gtk_window_present(GTK_WINDOW(dlg));
}

void MainWindow::showAbout()
{
    AdwDialog* about = adw_about_dialog_new();
    adw_about_dialog_set_application_name(ADW_ABOUT_DIALOG(about), "dmidiplayer");
    adw_about_dialog_set_application_icon(ADW_ABOUT_DIALOG(about), "dmidiplayer");
    adw_about_dialog_set_version(ADW_ABOUT_DIALOG(about), VERSION);
    adw_about_dialog_set_developer_name(ADW_ABOUT_DIALOG(about), "Pedro López-Cabanillas");
    adw_about_dialog_set_license_type(ADW_ABOUT_DIALOG(about), GTK_LICENSE_GPL_3_0);
    adw_about_dialog_set_website(ADW_ABOUT_DIALOG(about), "https://sourceforge.net/p/dmidiplayer/");
    adw_about_dialog_set_comments(ADW_ABOUT_DIALOG(about),
                                  "Drumstick Multiplatform MIDI File Player — GTK4/libadwaita rewrite with playlists.");
    adw_dialog_present(about, GTK_WIDGET(m_window));
}

void MainWindow::showHelp()
{
    AdwDialog* d = ADW_DIALOG(adw_alert_dialog_new("Help", kHelpText));
    adw_alert_dialog_add_response(ADW_ALERT_DIALOG(d), "ok", "OK");
    adw_dialog_present(d, GTK_WIDGET(m_window));
}

void MainWindow::showMidiSetup()
{
    GtkWidget* dlg = gtk_window_new();
    gtk_window_set_title(GTK_WINDOW(dlg), "MIDI Setup");
    gtk_window_set_transient_for(GTK_WINDOW(dlg), GTK_WINDOW(m_window));
    gtk_window_set_modal(GTK_WINDOW(dlg), true);
    gtk_window_set_default_size(GTK_WINDOW(dlg), 420, 280);
    GtkWidget* box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
    gtk_widget_set_margin_top(box, 18);
    gtk_widget_set_margin_bottom(box, 18);
    gtk_widget_set_margin_start(box, 18);
    gtk_widget_set_margin_end(box, 18);
    gtk_window_set_child(GTK_WINDOW(dlg), box);
    gtk_box_append(GTK_BOX(box), gtk_label_new("Output backend"));
    GtkStringList* backs = gtk_string_list_new(nullptr);
    auto names = m_outputs.backendNames();
    guint bsel = 0;
    for (guint i = 0; i < names.size(); ++i) {
        gtk_string_list_append(backs, names[i].c_str());
        if (m_outputs.current() && names[i] == m_outputs.current()->backendName())
            bsel = i;
    }
    GtkWidget* backend = gtk_drop_down_new(G_LIST_MODEL(backs), nullptr);
    gtk_drop_down_set_selected(GTK_DROP_DOWN(backend), bsel);
    gtk_box_append(GTK_BOX(box), backend);
    GtkWidget* adv = gtk_check_button_new_with_label("Show all ports");
    gtk_check_button_set_active(GTK_CHECK_BUTTON(adv), AppSettings::instance().advancedPorts);
    gtk_box_append(GTK_BOX(box), adv);
    gtk_box_append(GTK_BOX(box), gtk_label_new("Connection"));
    GtkWidget* ports = gtk_drop_down_new(G_LIST_MODEL(gtk_string_list_new(nullptr)), nullptr);
    gtk_box_append(GTK_BOX(box), ports);
    gtk_box_append(GTK_BOX(box), gtk_label_new("FluidSynth SoundFont"));
    GtkWidget* sf = gtk_entry_new();
    gtk_editable_set_text(GTK_EDITABLE(sf), AppSettings::instance().soundFont.c_str());
    gtk_box_append(GTK_BOX(box), sf);

    auto refill = [this, backend, ports, adv]() {
        guint bi = gtk_drop_down_get_selected(GTK_DROP_DOWN(backend));
        auto names = m_outputs.backendNames();
        if (bi >= names.size())
            return;
        auto* out = m_outputs.find(names[bi]);
        GtkStringList* list = gtk_string_list_new(nullptr);
        auto plist = out ? out->ports(gtk_check_button_get_active(GTK_CHECK_BUTTON(adv))) : std::vector<MidiPort>{};
        for (auto& p : plist)
            gtk_string_list_append(list, p.label.c_str());
        gtk_drop_down_set_model(GTK_DROP_DOWN(ports), G_LIST_MODEL(list));
        g_object_set_data(G_OBJECT(ports), "raw", new std::vector<MidiPort>(plist));
        if (!plist.empty())
            gtk_drop_down_set_selected(GTK_DROP_DOWN(ports), 0);
    };
    refill();
    g_signal_connect(backend, "notify::selected", (GCallback)(+[](GObject*, GParamSpec*, gpointer data) {
                         (*static_cast<std::function<void()>*>(data))();
                     }),
                     new std::function<void()>(refill));
    g_signal_connect(adv, "toggled", (GCallback)(+[](GtkCheckButton*, gpointer data) {
                         (*static_cast<std::function<void()>*>(data))();
                     }),
                     new std::function<void()>(refill));

    GtkWidget* apply = gtk_button_new_with_label("Apply");
    gtk_box_append(GTK_BOX(box), apply);
    g_object_set_data(G_OBJECT(apply), "self", this);
    g_object_set_data(G_OBJECT(apply), "backend", backend);
    g_object_set_data(G_OBJECT(apply), "ports", ports);
    g_object_set_data(G_OBJECT(apply), "sf", sf);
    g_object_set_data(G_OBJECT(apply), "adv", adv);
    g_object_set_data(G_OBJECT(apply), "win", dlg);
    g_signal_connect(apply, "clicked", (GCallback)(+[](GtkButton* b, gpointer) {
                         auto* self = static_cast<MainWindow*>(g_object_get_data(G_OBJECT(b), "self"));
                         auto* backend = GTK_DROP_DOWN(g_object_get_data(G_OBJECT(b), "backend"));
                         auto* ports = GTK_DROP_DOWN(g_object_get_data(G_OBJECT(b), "ports"));
                         auto* sf = GTK_EDITABLE(g_object_get_data(G_OBJECT(b), "sf"));
                         auto names = self->m_outputs.backendNames();
                         guint bi = gtk_drop_down_get_selected(backend);
                         auto* plist = static_cast<std::vector<MidiPort>*>(g_object_get_data(G_OBJECT(ports), "raw"));
                         std::string portId;
                         guint pi = gtk_drop_down_get_selected(ports);
                         if (plist && pi < plist->size())
                             portId = (*plist)[pi].id;
                         AppSettings::instance().soundFont = gtk_editable_get_text(sf);
                         AppSettings::instance().advancedPorts =
                             gtk_check_button_get_active(GTK_CHECK_BUTTON(g_object_get_data(G_OBJECT(b), "adv")));
                         self->m_outputs.setSoundFont(AppSettings::instance().soundFont);
                         if (bi < names.size())
                             self->connectOutput(names[bi], portId);
                         gtk_window_destroy(GTK_WINDOW(g_object_get_data(G_OBJECT(b), "win")));
                     }),
                     nullptr);
    gtk_window_present(GTK_WINDOW(dlg));
}

void MainWindow::showPrefs()
{
    AdwPreferencesWindow* win = ADW_PREFERENCES_WINDOW(adw_preferences_window_new());
    gtk_window_set_transient_for(GTK_WINDOW(win), GTK_WINDOW(m_window));
    auto& st = AppSettings::instance();

    auto* page = ADW_PREFERENCES_PAGE(adw_preferences_page_new());
    adw_preferences_page_set_title(page, "General");
    auto* grp = ADW_PREFERENCES_GROUP(adw_preferences_group_new());
    auto* drums = adw_spin_row_new_with_range(1, 16, 1);
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(drums), "Percussion MIDI channel");
    adw_spin_row_set_value(ADW_SPIN_ROW(drums), st.drumsChannel);
    auto* solo = adw_spin_row_new_with_range(0, 100, 1);
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(solo), "Solo button % volume reduction");
    adw_spin_row_set_value(ADW_SPIN_ROW(solo), st.soloVolumeReduction);
    auto* autoplay = adw_switch_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(autoplay), "Start playback after loading");
    adw_switch_row_set_active(ADW_SWITCH_ROW(autoplay), st.autoPlay);
    auto* adv = adw_switch_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(adv), "Advance to next playlist item");
    adw_switch_row_set_active(ADW_SWITCH_ROW(adv), st.autoAdvance);
    auto* songs = adw_switch_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(songs), "Automatically load and save song settings");
    adw_switch_row_set_active(ADW_SWITCH_ROW(songs), st.autoSongSettings);
    GtkStringList* resets = gtk_string_list_new(nullptr);
    gtk_string_list_append(resets, "None");
    gtk_string_list_append(resets, "GM Reset");
    gtk_string_list_append(resets, "GS Reset");
    gtk_string_list_append(resets, "XG Reset");
    auto* reset = adw_combo_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(reset), "MIDI System Exclusive Reset");
    adw_combo_row_set_model(ADW_COMBO_ROW(reset), G_LIST_MODEL(resets));
    adw_combo_row_set_selected(ADW_COMBO_ROW(reset), st.sysexReset);
    adw_preferences_group_add(grp, drums);
    adw_preferences_group_add(grp, solo);
    adw_preferences_group_add(grp, autoplay);
    adw_preferences_group_add(grp, adv);
    adw_preferences_group_add(grp, songs);
    adw_preferences_group_add(grp, reset);
    adw_preferences_page_add(page, grp);
    adw_preferences_window_add(win, page);

    auto* lpage = ADW_PREFERENCES_PAGE(adw_preferences_page_new());
    adw_preferences_page_set_title(lpage, "Lyrics");
    auto* lgrp = ADW_PREFERENCES_GROUP(adw_preferences_group_new());
    auto* future = adw_entry_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(future), "Future text color");
    gtk_editable_set_text(GTK_EDITABLE(future), st.futureColor.c_str());
    auto* past = adw_entry_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(past), "Past text color");
    gtk_editable_set_text(GTK_EDITABLE(past), st.pastColor.c_str());
    auto* hi = adw_entry_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(hi), "Highlight color");
    gtk_editable_set_text(GTK_EDITABLE(hi), st.highlightColor.c_str());
    auto* font = adw_entry_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(font), "Lyrics font");
    gtk_editable_set_text(GTK_EDITABLE(font), st.lyricsFont.c_str());
    GtkStringList* aligns = gtk_string_list_new(nullptr);
    gtk_string_list_append(aligns, "Left");
    gtk_string_list_append(aligns, "Center");
    gtk_string_list_append(aligns, "Right");
    auto* align = adw_combo_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(align), "Text alignment");
    adw_combo_row_set_model(ADW_COMBO_ROW(align), G_LIST_MODEL(aligns));
    adw_combo_row_set_selected(ADW_COMBO_ROW(align), st.textAlignment);
    adw_preferences_group_add(lgrp, future);
    adw_preferences_group_add(lgrp, past);
    adw_preferences_group_add(lgrp, hi);
    adw_preferences_group_add(lgrp, font);
    adw_preferences_group_add(lgrp, align);
    adw_preferences_page_add(lpage, lgrp);
    adw_preferences_window_add(win, lpage);

    auto* ppage = ADW_PREFERENCES_PAGE(adw_preferences_page_new());
    adw_preferences_page_set_title(ppage, "Piano");
    auto* pgrp = ADW_PREFERENCES_GROUP(adw_preferences_group_new());
    GtkStringList* pals = gtk_string_list_new(nullptr);
    gtk_string_list_append(pals, "Single color");
    gtk_string_list_append(pals, "Double");
    gtk_string_list_append(pals, "Channels");
    gtk_string_list_append(pals, "Scale");
    auto* pal = adw_combo_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(pal), "Note highlighting");
    adw_combo_row_set_model(ADW_COMBO_ROW(pal), G_LIST_MODEL(pals));
    adw_combo_row_set_selected(ADW_COMBO_ROW(pal), st.highlightPalette);
    auto* single = adw_entry_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(single), "Single highlight color");
    gtk_editable_set_text(GTK_EDITABLE(single), st.singleColor.c_str());
    auto* vel = adw_switch_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(vel), "Note velocity to color tint");
    adw_switch_row_set_active(ADW_SWITCH_ROW(vel), st.velocityColor);
    GtkStringList* vis = gtk_string_list_new(nullptr);
    gtk_string_list_append(vis, "Never");
    gtk_string_list_append(vis, "Minimal");
    gtk_string_list_append(vis, "When activated");
    gtk_string_list_append(vis, "Always");
    auto* names = adw_combo_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(names), "Show note names");
    adw_combo_row_set_model(ADW_COMBO_ROW(names), G_LIST_MODEL(vis));
    adw_combo_row_set_selected(ADW_COMBO_ROW(names), st.namesVisibility);
    auto* oct = adw_switch_row_new();
    adw_preferences_row_set_title(ADW_PREFERENCES_ROW(oct), "Octave subscript designation");
    adw_switch_row_set_active(ADW_SWITCH_ROW(oct), st.octaveSubscript);
    adw_preferences_group_add(pgrp, pal);
    adw_preferences_group_add(pgrp, single);
    adw_preferences_group_add(pgrp, vel);
    adw_preferences_group_add(pgrp, names);
    adw_preferences_group_add(pgrp, oct);
    adw_preferences_page_add(ppage, pgrp);
    adw_preferences_window_add(win, ppage);

    g_signal_connect(win, "close-request", (GCallback)(+[](GtkWindow* w, gpointer data) -> gboolean {
                         auto* self = static_cast<MainWindow*>(data);
                         auto& st = AppSettings::instance();
                         // widgets looked up by walking is hard; store on window
                         auto get = [&](const char* k) { return g_object_get_data(G_OBJECT(w), k); };
                         st.drumsChannel = static_cast<int>(adw_spin_row_get_value(ADW_SPIN_ROW(get("drums"))));
                         st.soloVolumeReduction = static_cast<int>(adw_spin_row_get_value(ADW_SPIN_ROW(get("solo"))));
                         st.autoPlay = adw_switch_row_get_active(ADW_SWITCH_ROW(get("autoplay")));
                         st.autoAdvance = adw_switch_row_get_active(ADW_SWITCH_ROW(get("adv")));
                         st.autoSongSettings = adw_switch_row_get_active(ADW_SWITCH_ROW(get("songs")));
                         st.sysexReset = static_cast<int>(adw_combo_row_get_selected(ADW_COMBO_ROW(get("reset"))));
                         st.futureColor = gtk_editable_get_text(GTK_EDITABLE(get("future")));
                         st.pastColor = gtk_editable_get_text(GTK_EDITABLE(get("past")));
                         st.highlightColor = gtk_editable_get_text(GTK_EDITABLE(get("hi")));
                         st.lyricsFont = gtk_editable_get_text(GTK_EDITABLE(get("font")));
                         st.textAlignment = static_cast<int>(adw_combo_row_get_selected(ADW_COMBO_ROW(get("align"))));
                         st.highlightPalette = static_cast<int>(adw_combo_row_get_selected(ADW_COMBO_ROW(get("pal"))));
                         st.singleColor = gtk_editable_get_text(GTK_EDITABLE(get("single")));
                         st.velocityColor = adw_switch_row_get_active(ADW_SWITCH_ROW(get("vel")));
                         st.namesVisibility = static_cast<int>(adw_combo_row_get_selected(ADW_COMBO_ROW(get("names"))));
                         st.octaveSubscript = adw_switch_row_get_active(ADW_SWITCH_ROW(get("oct")));
                         self->m_player.setDrumsChannel(std::clamp(st.drumsChannel, 1, 16) - 1);
                         self->m_player.setSysexReset(st.sysexReset);
                         self->refreshLyrics();
                         st.save();
                         gtk_window_destroy(w);
                         return true;
                     }),
                     this);
    g_object_set_data(G_OBJECT(win), "drums", drums);
    g_object_set_data(G_OBJECT(win), "solo", solo);
    g_object_set_data(G_OBJECT(win), "autoplay", autoplay);
    g_object_set_data(G_OBJECT(win), "adv", adv);
    g_object_set_data(G_OBJECT(win), "songs", songs);
    g_object_set_data(G_OBJECT(win), "reset", reset);
    g_object_set_data(G_OBJECT(win), "future", future);
    g_object_set_data(G_OBJECT(win), "past", past);
    g_object_set_data(G_OBJECT(win), "hi", hi);
    g_object_set_data(G_OBJECT(win), "font", font);
    g_object_set_data(G_OBJECT(win), "align", align);
    g_object_set_data(G_OBJECT(win), "pal", pal);
    g_object_set_data(G_OBJECT(win), "single", single);
    g_object_set_data(G_OBJECT(win), "vel", vel);
    g_object_set_data(G_OBJECT(win), "names", names);
    g_object_set_data(G_OBJECT(win), "oct", oct);
    gtk_window_present(GTK_WINDOW(win));
}

void MainWindow::bindActions(AdwApplication* app)
{
    struct Act {
        const char* name;
        void (MainWindow::*fn)();
    };
    // simple named callbacks via GSimpleAction
    auto add = [&](const char* name, void (MainWindow::*method)()) {
        GSimpleAction* a = g_simple_action_new(name, nullptr);
        g_signal_connect(a, "activate", (GCallback)(+[](GSimpleAction* act, GVariant*, gpointer data) {
                             auto* self = static_cast<MainWindow*>(data);
                             const char* n = g_action_get_name(G_ACTION(act));
                             if (g_strcmp0(n, "open") == 0)
                                 self->openDialog();
                             else if (g_strcmp0(n, "quit") == 0)
                                 g_application_quit(G_APPLICATION(gtk_window_get_application(GTK_WINDOW(self->m_window))));
                             else if (g_strcmp0(n, "play") == 0)
                                 self->play();
                             else if (g_strcmp0(n, "pause") == 0)
                                 self->pause();
                             else if (g_strcmp0(n, "stop") == 0)
                                 self->stop(true);
                             else if (g_strcmp0(n, "next") == 0)
                                 self->nextSong();
                             else if (g_strcmp0(n, "prev") == 0)
                                 self->prevSong();
                             else if (g_strcmp0(n, "fwd") == 0)
                                 self->m_player.beatForward();
                             else if (g_strcmp0(n, "rew") == 0)
                                 self->m_player.beatBackward();
                             else if (g_strcmp0(n, "jump") == 0)
                                 self->showJump();
                             else if (g_strcmp0(n, "loop") == 0)
                                 self->showLoop();
                             else if (g_strcmp0(n, "prefs") == 0)
                                 self->showPrefs();
                             else if (g_strcmp0(n, "midi") == 0)
                                 self->showMidiSetup();
                             else if (g_strcmp0(n, "about") == 0)
                                 self->showAbout();
                             else if (g_strcmp0(n, "help") == 0)
                                 self->showHelp();
                             else if (g_strcmp0(n, "info") == 0)
                                 self->showFileInfo();
                             else if (g_strcmp0(n, "save-settings") == 0)
                                 self->saveSongSettings();
                             else if (g_strcmp0(n, "load-settings") == 0)
                                 self->loadSongSettings();
                             else if (g_strcmp0(n, "search") == 0)
                                 gtk_show_uri(GTK_WINDOW(self->m_window), "https://midisite.co.uk", GDK_CURRENT_TIME);
                             else if (g_strcmp0(n, "website") == 0)
                                 gtk_show_uri(GTK_WINDOW(self->m_window), "https://sourceforge.net/p/dmidiplayer/",
                                              GDK_CURRENT_TIME);
                         }),
                         this);
        g_action_map_add_action(G_ACTION_MAP(app), G_ACTION(a));
        (void)method;
    };
    add("open", &MainWindow::openDialog);
    add("quit", &MainWindow::openDialog);
    add("play", &MainWindow::play);
    add("pause", &MainWindow::pause);
    add("stop", nullptr);
    add("next", &MainWindow::nextSong);
    add("prev", &MainWindow::prevSong);
    add("fwd", nullptr);
    add("rew", nullptr);
    add("jump", &MainWindow::showJump);
    add("loop", &MainWindow::showLoop);
    add("prefs", &MainWindow::showPrefs);
    add("midi", &MainWindow::showMidiSetup);
    add("about", &MainWindow::showAbout);
    add("help", &MainWindow::showHelp);
    add("info", &MainWindow::showFileInfo);
    add("save-settings", nullptr);
    add("load-settings", nullptr);
    add("search", nullptr);
    add("website", nullptr);

    const struct {
        const char* accels[3];
        const char* action;
    } acc[] = {
        {{"<Ctrl>o", nullptr}, "app.open"},
        {{"<Ctrl>q", nullptr}, "app.quit"},
        {{"space", nullptr}, "app.play"},
        {{"<Ctrl>p", nullptr}, "app.pause"},
        {{"<Ctrl>s", nullptr}, "app.stop"},
        {{"<Ctrl>Right", nullptr}, "app.next"},
        {{"<Ctrl>Left", nullptr}, "app.prev"},
        {{"F1", nullptr}, "app.help"},
        {{"<Ctrl>comma", nullptr}, "app.prefs"},
    };
    for (auto& a : acc)
        gtk_application_set_accels_for_action(GTK_APPLICATION(app), a.action, a.accels);
}

void MainWindow::buildUi(AdwApplication* app)
{
    auto& st = AppSettings::instance();
    m_window = ADW_APPLICATION_WINDOW(adw_application_window_new(GTK_APPLICATION(app)));
    gtk_window_set_title(GTK_WINDOW(m_window), "dmidiplayer");
    gtk_window_set_default_size(GTK_WINDOW(m_window), st.windowWidth, st.windowHeight);

    m_toasts = ADW_TOAST_OVERLAY(adw_toast_overlay_new());
    m_split = ADW_OVERLAY_SPLIT_VIEW(adw_overlay_split_view_new());
    adw_overlay_split_view_set_sidebar_width_fraction(m_split, 0.28);
    adw_toast_overlay_set_child(m_toasts, GTK_WIDGET(m_split));
    adw_application_window_set_content(m_window, GTK_WIDGET(m_toasts));

    // Sidebar playlist
    auto* sideView = ADW_TOOLBAR_VIEW(adw_toolbar_view_new());
    auto* sideHead = ADW_HEADER_BAR(adw_header_bar_new());
    adw_header_bar_set_title_widget(sideHead, gtk_label_new("Playlist"));
    adw_toolbar_view_add_top_bar(sideView, GTK_WIDGET(sideHead));
    m_playListBox = GTK_LIST_BOX(gtk_list_box_new());
    gtk_list_box_set_selection_mode(m_playListBox, GTK_SELECTION_SINGLE);
    gtk_widget_add_css_class(GTK_WIDGET(m_playListBox), "navigation-sidebar");
    auto* scroll = gtk_scrolled_window_new();
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(scroll), GTK_WIDGET(m_playListBox));
    adw_toolbar_view_set_content(sideView, scroll);
    GtkWidget* tools = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
    gtk_widget_add_css_class(tools, "toolbar");
    auto addTool = [&](const char* icon, const char* tip, GCallback cb) {
        gtk_box_append(GTK_BOX(tools), iconButton(icon, tip, cb, this));
    };
    addTool("list-add-symbolic", "Add files", (GCallback)(+[](GtkButton*, gpointer d) {
                static_cast<MainWindow*>(d)->openDialog();
            }));
    addTool("list-remove-symbolic", "Remove", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                auto* row = gtk_list_box_get_selected_row(self->m_playListBox);
                if (!row)
                    return;
                int i = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(row), "index"));
                self->m_playlist.removeAt(i);
                self->refreshPlaylistView();
            }));
    addTool("go-up-symbolic", "Move up", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                auto* row = gtk_list_box_get_selected_row(self->m_playListBox);
                if (!row)
                    return;
                int i = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(row), "index"));
                self->m_playlist.moveUp(i);
                self->refreshPlaylistView();
            }));
    addTool("go-down-symbolic", "Move down", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                auto* row = gtk_list_box_get_selected_row(self->m_playListBox);
                if (!row)
                    return;
                int i = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(row), "index"));
                self->m_playlist.moveDown(i);
                self->refreshPlaylistView();
            }));
    addTool("media-playlist-shuffle-symbolic", "Shuffle", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                self->m_playlist.shuffle();
                self->refreshPlaylistView();
            }));
    addTool("edit-clear-symbolic", "Clear", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                self->m_playlist.clear();
                self->refreshPlaylistView();
            }));
    addTool("document-open-symbolic", "Open playlist", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                GtkFileDialog* dlg = gtk_file_dialog_new();
                gtk_file_dialog_set_title(dlg, "Open playlist");
                gtk_file_dialog_open(dlg, GTK_WINDOW(self->m_window), nullptr,
                                     +[](GObject* src, GAsyncResult* res, gpointer data) {
                                         auto* self = static_cast<MainWindow*>(data);
                                         GError* err = nullptr;
                                         GFile* f = gtk_file_dialog_open_finish(GTK_FILE_DIALOG(src), res, &err);
                                         if (!f)
                                             return;
                                         char* p = g_file_get_path(f);
                                         if (p) {
                                             self->m_playlist.load(p);
                                             AppSettings::instance().lastPlayList = p;
                                             self->refreshPlaylistView();
                                             g_free(p);
                                         }
                                         g_object_unref(f);
                                     },
                                     self);
            }));
    addTool("document-save-symbolic", "Save playlist", (GCallback)(+[](GtkButton*, gpointer d) {
                auto* self = static_cast<MainWindow*>(d);
                GtkFileDialog* dlg = gtk_file_dialog_new();
                gtk_file_dialog_set_title(dlg, "Save playlist");
                gtk_file_dialog_set_initial_name(dlg, "playlist.lst");
                gtk_file_dialog_save(dlg, GTK_WINDOW(self->m_window), nullptr,
                                     +[](GObject* src, GAsyncResult* res, gpointer data) {
                                         auto* self = static_cast<MainWindow*>(data);
                                         GFile* f = gtk_file_dialog_save_finish(GTK_FILE_DIALOG(src), res, nullptr);
                                         if (!f)
                                             return;
                                         char* p = g_file_get_path(f);
                                         if (p) {
                                             self->m_playlist.save(p);
                                             AppSettings::instance().lastPlayList = p;
                                             g_free(p);
                                         }
                                         g_object_unref(f);
                                     },
                                     self);
            }));
    adw_toolbar_view_add_bottom_bar(sideView, tools);
    adw_overlay_split_view_set_sidebar(m_split, GTK_WIDGET(sideView));
    g_signal_connect(m_playListBox, "row-activated", (GCallback)(+[](GtkListBox*, GtkListBoxRow* row, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         int i = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(row), "index"));
                         self->m_playlist.setCurrentIndex(i);
                         self->loadCurrent(true);
                     }),
                     this);

    // Content
    auto* mainView = ADW_TOOLBAR_VIEW(adw_toolbar_view_new());
    auto* header = ADW_HEADER_BAR(adw_header_bar_new());
    GtkWidget* menuBtn = gtk_menu_button_new();
    gtk_menu_button_set_icon_name(GTK_MENU_BUTTON(menuBtn), "open-menu-symbolic");
    GMenu* menu = g_menu_new();
    GMenu* file = g_menu_new();
    g_menu_append(file, "Open…", "app.open");
    g_menu_append(file, "File information", "app.info");
    g_menu_append(file, "Load song settings", "app.load-settings");
    g_menu_append(file, "Save song settings", "app.save-settings");
    g_menu_append(file, "Search MIDI files", "app.search");
    g_menu_append(file, "Quit", "app.quit");
    GMenu* play = g_menu_new();
    g_menu_append(play, "Play", "app.play");
    g_menu_append(play, "Pause", "app.pause");
    g_menu_append(play, "Stop", "app.stop");
    g_menu_append(play, "Previous", "app.prev");
    g_menu_append(play, "Next", "app.next");
    g_menu_append(play, "Rewind 1 bar", "app.rew");
    g_menu_append(play, "Forward 1 bar", "app.fwd");
    g_menu_append(play, "Jump to bar…", "app.jump");
    g_menu_append(play, "Loop…", "app.loop");
    GMenu* rpt = g_menu_new();
    g_menu_append(rpt, "Repeat off", "app.repeat-off");
    g_menu_append(rpt, "Repeat song", "app.repeat-song");
    g_menu_append(rpt, "Repeat playlist", "app.repeat-list");
    GMenu* help = g_menu_new();
    g_menu_append(help, "MIDI Setup", "app.midi");
    g_menu_append(help, "Preferences", "app.prefs");
    g_menu_append(help, "Help", "app.help");
    g_menu_append(help, "Website", "app.website");
    g_menu_append(help, "About dmidiplayer", "app.about");
    g_menu_append_section(menu, "File", G_MENU_MODEL(file));
    g_menu_append_section(menu, "Playback", G_MENU_MODEL(play));
    g_menu_append_section(menu, "Repeat", G_MENU_MODEL(rpt));
    g_menu_append_section(menu, nullptr, G_MENU_MODEL(help));
    gtk_menu_button_set_menu_model(GTK_MENU_BUTTON(menuBtn), G_MENU_MODEL(menu));
    adw_header_bar_pack_end(header, menuBtn);

    auto packBtn = [&](const char* icon, const char* action) {
        GtkWidget* b = gtk_button_new_from_icon_name(icon);
        gtk_actionable_set_action_name(GTK_ACTIONABLE(b), action);
        gtk_widget_add_css_class(b, "flat");
        adw_header_bar_pack_start(header, b);
    };
    packBtn("document-open-symbolic", "app.open");
    packBtn("media-skip-backward-symbolic", "app.prev");
    packBtn("media-playback-start-symbolic", "app.play");
    packBtn("media-playback-pause-symbolic", "app.pause");
    packBtn("media-playback-stop-symbolic", "app.stop");
    packBtn("media-skip-forward-symbolic", "app.next");
    packBtn("media-seek-backward-symbolic", "app.rew");
    packBtn("media-seek-forward-symbolic", "app.fwd");
    m_loopBtn = GTK_TOGGLE_BUTTON(gtk_toggle_button_new());
    gtk_button_set_icon_name(GTK_BUTTON(m_loopBtn), "media-playlist-repeat-symbolic");
    gtk_widget_set_tooltip_text(GTK_WIDGET(m_loopBtn), "Loop");
    gtk_widget_add_css_class(GTK_WIDGET(m_loopBtn), "flat");
    g_signal_connect(m_loopBtn, "toggled", (GCallback)(+[](GtkToggleButton* b, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         if (gtk_toggle_button_get_active(b))
                             self->showLoop();
                         else
                             self->m_player.setLoop(false);
                     }),
                     this);
    adw_header_bar_pack_start(header, GTK_WIDGET(m_loopBtn));
    adw_toolbar_view_add_top_bar(mainView, GTK_WIDGET(header));

    GtkWidget* vbox = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    GtkWidget* dash = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 18);
    gtk_widget_add_css_class(dash, "player-dashboard");
    m_timeLabel = GTK_LABEL(gtk_label_new("00:00:00"));
    gtk_widget_add_css_class(GTK_WIDGET(m_timeLabel), "time-display");
    gtk_box_append(GTK_BOX(dash), GTK_WIDGET(m_timeLabel));
    GtkWidget* stats = gtk_grid_new();
    gtk_grid_set_row_spacing(GTK_GRID(stats), 4);
    gtk_grid_set_column_spacing(GTK_GRID(stats), 10);
    auto addStat = [&](int row, const char* name, GtkWidget* w) {
        GtkWidget* l = gtk_label_new(name);
        gtk_widget_add_css_class(l, "dash-label");
        gtk_widget_set_halign(l, GTK_ALIGN_END);
        gtk_grid_attach(GTK_GRID(stats), l, 0, row, 1, 1);
        gtk_grid_attach(GTK_GRID(stats), w, 1, row, 1, 1);
    };
    m_tempoValue = GTK_LABEL(gtk_label_new("120.0 bpm"));
    gtk_widget_add_css_class(GTK_WIDGET(m_tempoValue), "dash-value");
    m_volumeValue = GTK_LABEL(gtk_label_new("100%"));
    gtk_widget_add_css_class(GTK_WIDGET(m_volumeValue), "dash-value");
    m_pitch = GTK_SPIN_BUTTON(gtk_spin_button_new_with_range(-12, 12, 1));
    addStat(0, "Tempo", GTK_WIDGET(m_tempoValue));
    addStat(1, "Volume", GTK_WIDGET(m_volumeValue));
    addStat(2, "Pitch", GTK_WIDGET(m_pitch));
    gtk_box_append(GTK_BOX(dash), stats);
    GtkWidget* dashRight = gtk_box_new(GTK_ORIENTATION_VERTICAL, 6);
    m_songTitle = GTK_LABEL(gtk_label_new("No file loaded"));
    gtk_label_set_xalign(m_songTitle, 0);
    gtk_label_set_ellipsize(m_songTitle, PANGO_ELLIPSIZE_MIDDLE);
    gtk_box_append(GTK_BOX(dashRight), GTK_WIDGET(m_songTitle));
    m_rhythmBox = GTK_BOX(gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6));
    for (int i = 0; i < 4; ++i) {
        GtkWidget* lamp = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
        gtk_widget_add_css_class(lamp, "rhythm-lamp");
        gtk_box_append(m_rhythmBox, lamp);
    }
    gtk_box_append(GTK_BOX(dashRight), GTK_WIDGET(m_rhythmBox));
    m_posLabel = GTK_LABEL(gtk_label_new("1:1"));
    gtk_box_append(GTK_BOX(dashRight), GTK_WIDGET(m_posLabel));
    gtk_box_append(GTK_BOX(dash), dashRight);
    gtk_box_append(GTK_BOX(vbox), dash);

    GtkWidget* controls = gtk_box_new(GTK_ORIENTATION_VERTICAL, 4);
    gtk_widget_set_margin_start(controls, 16);
    gtk_widget_set_margin_end(controls, 16);
    m_posScale = GTK_SCALE(gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 1000, 1));
    gtk_scale_set_draw_value(m_posScale, false);
    gtk_widget_set_hexpand(GTK_WIDGET(m_posScale), true);
    gtk_box_append(GTK_BOX(controls), GTK_WIDGET(m_posScale));
    GtkWidget* sliders = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 12);
    m_tempoScale = GTK_SCALE(gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 50, 200, 1));
    gtk_range_set_value(GTK_RANGE(m_tempoScale), 100);
    gtk_widget_set_hexpand(GTK_WIDGET(m_tempoScale), true);
    gtk_widget_set_tooltip_text(GTK_WIDGET(m_tempoScale), "Tempo 50–200%");
    m_volumeScale = GTK_SCALE(gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 200, 1));
    gtk_range_set_value(GTK_RANGE(m_volumeScale), 100);
    gtk_widget_set_hexpand(GTK_WIDGET(m_volumeScale), true);
    gtk_widget_set_tooltip_text(GTK_WIDGET(m_volumeScale), "Volume 0–200%");
    GtkWidget* tempoReset = gtk_button_new_with_label("Tempo");
    GtkWidget* volReset = gtk_button_new_with_label("Volume");
    gtk_box_append(GTK_BOX(sliders), tempoReset);
    gtk_box_append(GTK_BOX(sliders), GTK_WIDGET(m_tempoScale));
    gtk_box_append(GTK_BOX(sliders), volReset);
    gtk_box_append(GTK_BOX(sliders), GTK_WIDGET(m_volumeScale));
    gtk_box_append(GTK_BOX(controls), sliders);
    gtk_box_append(GTK_BOX(vbox), controls);

    g_signal_connect(m_tempoScale, "value-changed", (GCallback)(+[](GtkRange* r, gpointer d) {
                         static_cast<MainWindow*>(d)->applyTempo(static_cast<int>(gtk_range_get_value(r)));
                     }),
                     this);
    g_signal_connect(m_volumeScale, "value-changed", (GCallback)(+[](GtkRange* r, gpointer d) {
                         static_cast<MainWindow*>(d)->applyVolume(static_cast<int>(gtk_range_get_value(r)));
                     }),
                     this);
    g_signal_connect(m_pitch, "value-changed", (GCallback)(+[](GtkSpinButton* s, gpointer d) {
                         static_cast<MainWindow*>(d)->applyPitch(gtk_spin_button_get_value_as_int(s));
                     }),
                     this);
    g_signal_connect(tempoReset, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         gtk_range_set_value(GTK_RANGE(static_cast<MainWindow*>(d)->m_tempoScale), 100);
                     }),
                     this);
    g_signal_connect(volReset, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         gtk_range_set_value(GTK_RANGE(static_cast<MainWindow*>(d)->m_volumeScale), 100);
                     }),
                     this);
    g_signal_connect(m_posScale, "change-value", (GCallback)(+[](GtkRange*, GtkScrollType, gdouble value, gpointer d) -> gboolean {
                         auto* self = static_cast<MainWindow*>(d);
                         self->m_seeking = true;
                         int len = self->m_player.song().songLengthTicks();
                         if (len > 0)
                             self->m_player.setPosition(static_cast<int64_t>(value / 1000.0 * len));
                         self->m_seeking = false;
                         return false;
                     }),
                     this);

    // View stack
    m_stack = ADW_VIEW_STACK(adw_view_stack_new());
    GtkWidget* switcher = adw_view_switcher_new();
    adw_view_switcher_set_stack(ADW_VIEW_SWITCHER(switcher), m_stack);
    adw_view_switcher_set_policy(ADW_VIEW_SWITCHER(switcher), ADW_VIEW_SWITCHER_POLICY_WIDE);
    gtk_box_append(GTK_BOX(vbox), switcher);

    // Lyrics
    GtkWidget* lyricsPage = gtk_box_new(GTK_ORIENTATION_VERTICAL, 6);
    GtkWidget* lbar = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 8);
    gtk_widget_set_margin_start(lbar, 8);
    gtk_widget_set_margin_end(lbar, 8);
    m_lyricTrack = GTK_DROP_DOWN(gtk_drop_down_new(G_LIST_MODEL(gtk_string_list_new(nullptr)), nullptr));
    m_lyricType = GTK_DROP_DOWN(gtk_drop_down_new(G_LIST_MODEL(gtk_string_list_new(kTypeNames)), nullptr));
    m_lyricCodec = GTK_DROP_DOWN(gtk_drop_down_new(G_LIST_MODEL(gtk_string_list_new(nullptr)), nullptr));
    gtk_box_append(GTK_BOX(lbar), gtk_label_new("Track"));
    gtk_box_append(GTK_BOX(lbar), GTK_WIDGET(m_lyricTrack));
    gtk_box_append(GTK_BOX(lbar), gtk_label_new("Type"));
    gtk_box_append(GTK_BOX(lbar), GTK_WIDGET(m_lyricType));
    gtk_box_append(GTK_BOX(lbar), gtk_label_new("Encoding"));
    gtk_box_append(GTK_BOX(lbar), GTK_WIDGET(m_lyricCodec));
    GtkWidget* copyBtn = gtk_button_new_from_icon_name("edit-copy-symbolic");
    GtkWidget* saveBtn = gtk_button_new_from_icon_name("document-save-symbolic");
    GtkWidget* printBtn = gtk_button_new_from_icon_name("document-print-symbolic");
    gtk_widget_set_tooltip_text(copyBtn, "Copy lyrics");
    gtk_widget_set_tooltip_text(saveBtn, "Save lyrics");
    gtk_widget_set_tooltip_text(printBtn, "Print lyrics");
    gtk_box_append(GTK_BOX(lbar), copyBtn);
    gtk_box_append(GTK_BOX(lbar), saveBtn);
    gtk_box_append(GTK_BOX(lbar), printBtn);
    gtk_box_append(GTK_BOX(lyricsPage), lbar);
    m_lyricsView = GTK_TEXT_VIEW(gtk_text_view_new());
    gtk_text_view_set_wrap_mode(m_lyricsView, GTK_WRAP_WORD_CHAR);
    gtk_text_view_set_editable(m_lyricsView, false);
    gtk_widget_add_css_class(GTK_WIDGET(m_lyricsView), "lyrics-view");
    GtkWidget* lscroll = gtk_scrolled_window_new();
    gtk_widget_set_vexpand(lscroll, true);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(lscroll), GTK_WIDGET(m_lyricsView));
    gtk_box_append(GTK_BOX(lyricsPage), lscroll);
    adw_view_stack_add_titled_with_icon(m_stack, lyricsPage, "lyrics", "Lyrics", "audio-headphones-symbolic");
    g_signal_connect(m_lyricTrack, "notify::selected", (GCallback)(+[](GObject* o, GParamSpec*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         guint s = gtk_drop_down_get_selected(GTK_DROP_DOWN(o));
                         self->m_lyricTrackFilter = s == 0 ? -1 : static_cast<int>(s);
                         self->refreshLyrics();
                     }),
                     this);
    g_signal_connect(m_lyricType, "notify::selected", (GCallback)(+[](GObject* o, GParamSpec*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         self->m_lyricTypeFilter = static_cast<int>(gtk_drop_down_get_selected(GTK_DROP_DOWN(o)));
                         self->refreshLyrics();
                     }),
                     this);
    g_signal_connect(m_lyricCodec, "notify::selected", (GCallback)(+[](GObject* o, GParamSpec*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         auto* model = gtk_drop_down_get_model(GTK_DROP_DOWN(o));
                         guint s = gtk_drop_down_get_selected(GTK_DROP_DOWN(o));
                         const char* name = gtk_string_list_get_string(GTK_STRING_LIST(model), s);
                         if (name)
                             self->m_player.song().setCurrentCharset(name);
                         self->refreshLyrics();
                     }),
                     this);
    g_signal_connect(copyBtn, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         GtkTextIter a, b;
                         auto* buf = gtk_text_view_get_buffer(self->m_lyricsView);
                         gtk_text_buffer_get_bounds(buf, &a, &b);
                         char* t = gtk_text_buffer_get_text(buf, &a, &b, false);
                         gdk_display_get_clipboard(gdk_display_get_default());
                         gdk_clipboard_set_text(gdk_display_get_clipboard(gdk_display_get_default()), t);
                         g_free(t);
                     }),
                     this);
    g_signal_connect(saveBtn, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         GtkFileDialog* dlg = gtk_file_dialog_new();
                         gtk_file_dialog_set_initial_name(dlg, "lyrics.txt");
                         gtk_file_dialog_save(dlg, GTK_WINDOW(self->m_window), nullptr,
                                              +[](GObject* src, GAsyncResult* res, gpointer data) {
                                                  auto* self = static_cast<MainWindow*>(data);
                                                  GFile* f = gtk_file_dialog_save_finish(GTK_FILE_DIALOG(src), res, nullptr);
                                                  if (!f)
                                                      return;
                                                  GtkTextIter a, b;
                                                  auto* buf = gtk_text_view_get_buffer(self->m_lyricsView);
                                                  gtk_text_buffer_get_bounds(buf, &a, &b);
                                                  char* t = gtk_text_buffer_get_text(buf, &a, &b, false);
                                                  g_file_set_contents(g_file_get_path(f), t, -1, nullptr);
                                                  g_free(t);
                                                  g_object_unref(f);
                                              },
                                              self);
                     }),
                     this);
    g_signal_connect(printBtn, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         GtkTextIter a, b;
                         auto* buf = gtk_text_view_get_buffer(self->m_lyricsView);
                         gtk_text_buffer_get_bounds(buf, &a, &b);
                         char* t = gtk_text_buffer_get_text(buf, &a, &b, false);
                         GtkPrintOperation* op = gtk_print_operation_new();
                         gtk_print_operation_set_n_pages(op, 1);
                         auto* pd = new PrintData{t};
                         g_signal_connect(op, "draw-page", (GCallback)(+[](GtkPrintOperation*, GtkPrintContext* ctx, gint, gpointer data) {
                                              auto* pd = static_cast<PrintData*>(data);
                                              cairo_t* cr = gtk_print_context_get_cairo_context(ctx);
                                              PangoLayout* layout = gtk_print_context_create_pango_layout(ctx);
                                              pango_layout_set_text(layout, pd->text ? pd->text : "", -1);
                                              pango_layout_set_width(layout,
                                                  pango_units_from_double(gtk_print_context_get_width(ctx)));
                                              pango_cairo_show_layout(cr, layout);
                                              g_object_unref(layout);
                                          }), pd);
                         gtk_print_operation_run(op, GTK_PRINT_OPERATION_ACTION_PRINT_DIALOG,
                                                 GTK_WINDOW(self->m_window), nullptr);
                         g_object_unref(op);
                         g_free(pd->text);
                         delete pd;
                     }),
                     this);

    // Channels
    GtkWidget* chPage = gtk_scrolled_window_new();
    m_channelsBox = GTK_BOX(gtk_box_new(GTK_ORIENTATION_VERTICAL, 4));
    gtk_widget_set_margin_start(GTK_WIDGET(m_channelsBox), 8);
    gtk_widget_set_margin_end(GTK_WIDGET(m_channelsBox), 8);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(chPage), GTK_WIDGET(m_channelsBox));
    GtkStringList* patchModel = gtk_string_list_new(nullptr);
    for (int p = 0; p < 128; ++p)
        gtk_string_list_append(patchModel, gmPatchName(p));
    for (int i = 0; i < kMidiChannels; ++i) {
        GtkWidget* row = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 8);
        m_chRow[i] = row;
        char lab[8];
        std::snprintf(lab, sizeof(lab), "%d", i + 1);
        gtk_box_append(GTK_BOX(row), gtk_label_new(lab));
        m_chName[i] = GTK_ENTRY(gtk_entry_new());
        gtk_widget_set_hexpand(GTK_WIDGET(m_chName[i]), true);
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chName[i]));
        m_chMute[i] = GTK_TOGGLE_BUTTON(gtk_toggle_button_new_with_label("M"));
        m_chSolo[i] = GTK_TOGGLE_BUTTON(gtk_toggle_button_new_with_label("S"));
        m_chLock[i] = GTK_TOGGLE_BUTTON(gtk_toggle_button_new());
        gtk_button_set_icon_name(GTK_BUTTON(m_chLock[i]), "channel-secure-symbolic");
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chMute[i]));
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chSolo[i]));
        m_chMeter[i] = GTK_LEVEL_BAR(gtk_level_bar_new());
        gtk_level_bar_set_value(m_chMeter[i], 0);
        gtk_widget_set_size_request(GTK_WIDGET(m_chMeter[i]), 80, 10);
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chMeter[i]));
        m_chVol[i] = GTK_SCALE(gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 200, 1));
        gtk_range_set_value(GTK_RANGE(m_chVol[i]), 100);
        gtk_widget_set_size_request(GTK_WIDGET(m_chVol[i]), 100, -1);
        gtk_scale_set_draw_value(m_chVol[i], false);
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chVol[i]));
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chLock[i]));
        m_chPatch[i] = GTK_DROP_DOWN(gtk_drop_down_new(G_LIST_MODEL(patchModel), nullptr));
        gtk_widget_set_size_request(GTK_WIDGET(m_chPatch[i]), 180, -1);
        gtk_box_append(GTK_BOX(row), GTK_WIDGET(m_chPatch[i]));
        gtk_box_append(m_channelsBox, row);
        gtk_widget_set_visible(row, false);
        g_object_set_data(G_OBJECT(m_chMute[i]), "ch", GINT_TO_POINTER(i));
        g_object_set_data(G_OBJECT(m_chSolo[i]), "ch", GINT_TO_POINTER(i));
        g_object_set_data(G_OBJECT(m_chLock[i]), "ch", GINT_TO_POINTER(i));
        g_object_set_data(G_OBJECT(m_chPatch[i]), "ch", GINT_TO_POINTER(i));
        g_object_set_data(G_OBJECT(m_chVol[i]), "ch", GINT_TO_POINTER(i));
        g_signal_connect(m_chMute[i], "toggled", (GCallback)(+[](GtkToggleButton*, gpointer d) {
                             static_cast<MainWindow*>(d)->applyChannelSoloMute();
                         }),
                         this);
        g_signal_connect(m_chSolo[i], "toggled", (GCallback)(+[](GtkToggleButton* b, gpointer d) {
                             auto* self = static_cast<MainWindow*>(d);
                             int ch = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(b), "ch"));
                             self->m_chSoloed[ch] = gtk_toggle_button_get_active(b);
                             self->applyChannelSoloMute();
                         }),
                         this);
        g_signal_connect(m_chLock[i], "toggled", (GCallback)(+[](GtkToggleButton* b, gpointer d) {
                             auto* self = static_cast<MainWindow*>(d);
                             int ch = GPOINTER_TO_INT(g_object_get_data(G_OBJECT(b), "ch"));
                             self->m_player.setLocked(ch, gtk_toggle_button_get_active(b));
                         }),
                         this);
        g_signal_connect(m_chPatch[i], "notify::selected", (GCallback)(+[](GObject* o, GParamSpec*, gpointer d) {
                             auto* self = static_cast<MainWindow*>(d);
                             int ch = GPOINTER_TO_INT(g_object_get_data(o, "ch"));
                             int pgm = static_cast<int>(gtk_drop_down_get_selected(GTK_DROP_DOWN(o)));
                             self->m_player.setPatch(ch, pgm);
                         }),
                         this);
        g_signal_connect(m_chVol[i], "value-changed", (GCallback)(+[](GtkRange*, gpointer d) {
                             static_cast<MainWindow*>(d)->applyChannelSoloMute();
                         }),
                         this);
    }
    adw_view_stack_add_titled_with_icon(m_stack, chPage, "channels", "Channels", "audio-speakers-symbolic");

    // Piano
    GtkWidget* pianoPage = gtk_box_new(GTK_ORIENTATION_VERTICAL, 6);
    GtkWidget* pbar = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 8);
    GtkWidget* tighten = gtk_check_button_new_with_label("Tighten keys");
    gtk_box_append(GTK_BOX(pbar), tighten);
    GtkWidget* showAll = gtk_button_new_with_label("Show all");
    GtkWidget* hideAll = gtk_button_new_with_label("Hide unused");
    gtk_box_append(GTK_BOX(pbar), showAll);
    gtk_box_append(GTK_BOX(pbar), hideAll);
    gtk_box_append(GTK_BOX(pianoPage), pbar);
    g_signal_connect(tighten, "toggled", (GCallback)(+[](GtkCheckButton* b, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         self->m_tightenKeys = gtk_check_button_get_active(b);
                         gtk_widget_queue_draw(GTK_WIDGET(self->m_piano));
                     }),
                     this);
    m_piano = GTK_DRAWING_AREA(gtk_drawing_area_new());
    gtk_widget_add_css_class(GTK_WIDGET(m_piano), "piano-area");
    gtk_widget_set_vexpand(GTK_WIDGET(m_piano), true);
    gtk_drawing_area_set_draw_func(
        m_piano,
        [](GtkDrawingArea*, cairo_t* cr, int width, int height, gpointer data) {
            auto* self = static_cast<MainWindow*>(data);
            cairo_set_source_rgb(cr, 0.14, 0.12, 0.19);
            cairo_paint(cr);
            int used = 0;
            for (int c = 0; c < kMidiChannels; ++c)
                if (self->m_player.song().channelUsed(c) && self->m_pianoVisible[c])
                    used++;
            if (used == 0)
                used = 1;
            int rowH = std::max(48, height / used);
            int lo = 21, hi = 108;
            if (self->m_tightenKeys && !self->m_player.song().empty()) {
                lo = std::max(0, self->m_player.song().lowestNote() - 2);
                hi = std::min(127, self->m_player.song().highestNote() + 2);
            }
            int whites = 0;
            for (int n = lo; n <= hi; ++n)
                if (!noteIsBlack(n))
                    whites++;
            if (whites < 1)
                whites = 1;
            double ww = double(width - 8) / whites;
            int row = 0;
            auto& st = AppSettings::instance();
            for (int c = 0; c < kMidiChannels; ++c) {
                if (!(self->m_player.song().empty() ? c == 0 : (self->m_player.song().channelUsed(c) && self->m_pianoVisible[c])))
                    continue;
                double y = row * rowH + 4;
                double xw = 4;
                for (int n = lo; n <= hi; ++n) {
                    if (noteIsBlack(n))
                        continue;
                    bool on = self->m_notesOn[c][n];
                    if (on) {
                        auto col = self->channelColor(c, self->m_noteVel[c][n]);
                        cairo_set_source_rgb(cr, col.red, col.green, col.blue);
                    } else
                        cairo_set_source_rgb(cr, 0.95, 0.95, 0.93);
                    cairo_rectangle(cr, xw, y, ww - 1, rowH - 18);
                    cairo_fill_preserve(cr);
                    cairo_set_source_rgb(cr, 0.2, 0.2, 0.2);
                    cairo_stroke(cr);
                    if (st.namesVisibility == 3 || (st.namesVisibility == 1 && n % 12 == 0)
                        || (st.namesVisibility == 2 && on)) {
                        cairo_set_source_rgb(cr, 0.1, 0.1, 0.1);
                        cairo_select_font_face(cr, "Sans", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_NORMAL);
                        cairo_set_font_size(cr, 9);
                        const char* namesn[] = {"C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"};
                        int oct = n / 12 - 1;
                        char lab[16];
                        if (st.octaveSubscript)
                            std::snprintf(lab, sizeof(lab), "%s%d", namesn[n % 12], oct);
                        else
                            std::snprintf(lab, sizeof(lab), "%s%d", namesn[n % 12], oct);
                        cairo_move_to(cr, xw + 2, y + rowH - 22);
                        cairo_show_text(cr, lab);
                    }
                    xw += ww;
                }
                xw = 4;
                for (int n = lo; n <= hi; ++n) {
                    if (noteIsBlack(n)) {
                        double bx = xw - ww * 0.35;
                        bool on = self->m_notesOn[c][n];
                        if (on) {
                            auto col = self->channelColor(c, self->m_noteVel[c][n]);
                            cairo_set_source_rgb(cr, col.red, col.green, col.blue);
                        } else
                            cairo_set_source_rgb(cr, 0.1, 0.1, 0.12);
                        cairo_rectangle(cr, bx, y, ww * 0.6, (rowH - 18) * 0.62);
                        cairo_fill(cr);
                    } else
                        xw += ww;
                }
                cairo_set_source_rgb(cr, 0.9, 0.9, 0.9);
                cairo_move_to(cr, 8, y + rowH - 6);
                char clab[64];
                std::snprintf(clab, sizeof(clab), "Ch %d %s", c + 1, self->m_player.song().channelLabel(c).c_str());
                cairo_show_text(cr, clab);
                ++row;
            }
        },
        this,
        nullptr);
    gtk_box_append(GTK_BOX(pianoPage), GTK_WIDGET(m_piano));
    GtkGesture* click = gtk_gesture_click_new();
    gtk_widget_add_controller(GTK_WIDGET(m_piano), GTK_EVENT_CONTROLLER(click));
    g_signal_connect(click, "pressed", (GCallback)(+[](GtkGestureClick*, gint, gdouble x, gdouble y, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         int used = 0;
                         for (int c = 0; c < kMidiChannels; ++c)
                             if (self->m_player.song().channelUsed(c) && self->m_pianoVisible[c])
                                 used++;
                         if (used == 0)
                             return;
                         int w = gtk_widget_get_width(GTK_WIDGET(self->m_piano));
                         int h = gtk_widget_get_height(GTK_WIDGET(self->m_piano));
                         int rowH = std::max(48, h / used);
                         int row = std::min(used - 1, static_cast<int>(y / rowH));
                         int ch = -1, seen = 0;
                         for (int c = 0; c < kMidiChannels; ++c) {
                             if (self->m_player.song().channelUsed(c) && self->m_pianoVisible[c]) {
                                 if (seen == row) {
                                     ch = c;
                                     break;
                                 }
                                 ++seen;
                             }
                         }
                         if (ch < 0)
                             return;
                         int lo = 21, hi = 108;
                         int whites = 0;
                         for (int n = lo; n <= hi; ++n)
                             if (!noteIsBlack(n))
                                 whites++;
                         double ww = double(w - 8) / std::max(1, whites);
                         int idx = static_cast<int>((x - 4) / ww);
                         int wi = 0, note = lo;
                         for (int n = lo; n <= hi; ++n) {
                             if (!noteIsBlack(n)) {
                                 if (wi == idx) {
                                     note = n;
                                     break;
                                 }
                                 ++wi;
                             }
                         }
                         if (self->m_outputs.current()) {
                             self->m_outputs.current()->sendNoteOn(ch, note, 80);
                             self->m_notesOn[ch][note] = true;
                             gtk_widget_queue_draw(GTK_WIDGET(self->m_piano));
                         }
                     }),
                     this);
    g_signal_connect(click, "released", (GCallback)(+[](GtkGestureClick*, gint, gdouble, gdouble, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         if (!self->m_outputs.current())
                             return;
                         for (int c = 0; c < kMidiChannels; ++c)
                             for (int n = 0; n < 128; ++n)
                                 if (self->m_notesOn[c][n] && !self->m_player.isRunning()) {
                                     self->m_outputs.current()->sendNoteOff(c, n, 0);
                                     self->m_notesOn[c][n] = false;
                                 }
                         gtk_widget_queue_draw(GTK_WIDGET(self->m_piano));
                     }),
                     this);
    adw_view_stack_add_titled_with_icon(m_stack, pianoPage, "piano", "Piano", "audio-x-generic-symbolic");
    gtk_box_append(GTK_BOX(vbox), GTK_WIDGET(m_stack));

    m_status = GTK_LABEL(gtk_label_new("Ready"));
    gtk_widget_add_css_class(GTK_WIDGET(m_status), "dim-label");
    gtk_label_set_xalign(m_status, 0);
    gtk_widget_set_margin_start(GTK_WIDGET(m_status), 12);
    gtk_widget_set_margin_bottom(GTK_WIDGET(m_status), 6);
    gtk_box_append(GTK_BOX(vbox), GTK_WIDGET(m_status));
    adw_toolbar_view_set_content(mainView, vbox);
    adw_overlay_split_view_set_content(m_split, GTK_WIDGET(mainView));

    GtkDropTarget* drop = gtk_drop_target_new(GDK_TYPE_FILE_LIST, GDK_ACTION_COPY);
    g_signal_connect(drop, "drop", G_CALLBACK(dmidi_drop_files), this);
    gtk_widget_add_controller(GTK_WIDGET(m_window), GTK_EVENT_CONTROLLER(drop));

    auto* css = gtk_css_provider_new();
    gtk_css_provider_load_from_resource(css, "/net/sourceforge/dmidiplayer/style.css");
    gtk_style_context_add_provider_for_display(gdk_display_get_default(), GTK_STYLE_PROVIDER(css),
                                               GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);

    // repeat actions
    auto addRepeat = [&](const char* name, int mode) {
        GSimpleAction* a = g_simple_action_new(name, nullptr);
        g_object_set_data(G_OBJECT(a), "mode", GINT_TO_POINTER(mode));
        g_signal_connect(a, "activate", (GCallback)(+[](GSimpleAction* act, GVariant*, gpointer d) {
                             static_cast<MainWindow*>(d)->m_repeat =
                                 GPOINTER_TO_INT(g_object_get_data(G_OBJECT(act), "mode"));
                         }),
                         this);
        g_action_map_add_action(G_ACTION_MAP(app), G_ACTION(a));
    };
    addRepeat("repeat-off", 0);
    addRepeat("repeat-song", 1);
    addRepeat("repeat-list", 2);

    g_signal_connect(showAll, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         for (int i = 0; i < kMidiChannels; ++i)
                             self->m_pianoVisible[i] = true;
                         gtk_widget_queue_draw(GTK_WIDGET(self->m_piano));
                     }),
                     this);
    g_signal_connect(hideAll, "clicked", (GCallback)(+[](GtkButton*, gpointer d) {
                         auto* self = static_cast<MainWindow*>(d);
                         for (int i = 0; i < kMidiChannels; ++i)
                             self->m_pianoVisible[i] = self->m_player.song().channelUsed(i);
                         gtk_widget_queue_draw(GTK_WIDGET(self->m_piano));
                     }),
                     this);
}

} // namespace dmidi
