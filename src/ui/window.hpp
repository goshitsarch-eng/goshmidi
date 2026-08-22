/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#pragma once

#include "../app/playlist.hpp"
#include "../midi/output.hpp"
#include "../midi/player.hpp"

#include <adwaita.h>
#include <gtk/gtk.h>
#include <memory>
#include <string>
#include <vector>

namespace dmidi {

class MainWindow {
public:
    MainWindow(AdwApplication* app);
    ~MainWindow();

    GtkWindow* gtkWindow() const { return GTK_WINDOW(m_window); }
    void openFiles(const std::vector<std::string>& files, bool replacePlaylist);
    void connectOutput(const std::string& backend, const std::string& port);

    SequencePlayer* player() { return &m_player; }
    Playlist* playlist() { return &m_playlist; }

private:
    friend gboolean dmidi_drop_files(GtkDropTarget*, const GValue*, double, double, gpointer);

    void buildUi(AdwApplication* app);
    void bindActions(AdwApplication* app);
    void refreshPlaylistView();
    void refreshChannels();
    void refreshLyrics();
    void refreshMidiSetupLists();
    void setStatus(const std::string& text);
    void updateTime(std::chrono::milliseconds ms, int64_t ticks);
    void loadCurrent(bool autoPlay);
    void play();
    void pause();
    void stop(bool reset);
    void nextSong();
    void prevSong();
    void openDialog();
    void showPrefs();
    void showMidiSetup();
    void showAbout();
    void showHelp();
    void showFileInfo();
    void showJump();
    void showLoop();
    void applyTempo(int percent);
    void applyVolume(int percent);
    void applyPitch(int semis);
    void saveSongSettings();
    void loadSongSettings();
    void applyChannelSoloMute();
    GdkRGBA channelColor(int ch, int vel) const;
    void toast(const std::string& msg);

    AdwApplicationWindow* m_window{};
    AdwToastOverlay* m_toasts{};
    AdwOverlaySplitView* m_split{};
    GtkListBox* m_playListBox{};
    GtkLabel* m_timeLabel{};
    GtkLabel* m_tempoValue{};
    GtkLabel* m_volumeValue{};
    GtkLabel* m_status{};
    GtkLabel* m_posLabel{};
    GtkLabel* m_songTitle{};
    GtkScale* m_posScale{};
    GtkScale* m_tempoScale{};
    GtkScale* m_volumeScale{};
    GtkSpinButton* m_pitch{};
    GtkToggleButton* m_loopBtn{};
    GtkBox* m_rhythmBox{};
    AdwViewStack* m_stack{};
    GtkTextView* m_lyricsView{};
    GtkDropDown* m_lyricTrack{};
    GtkDropDown* m_lyricType{};
    GtkDropDown* m_lyricCodec{};
    GtkBox* m_channelsBox{};
    GtkDrawingArea* m_piano{};
    GtkDropDown* m_backendDrop{};
    GtkDropDown* m_portDrop{};
    GtkCheckButton* m_advancedPorts{};

    GtkWidget* m_chRow[kMidiChannels]{};
    GtkEntry* m_chName[kMidiChannels]{};
    GtkToggleButton* m_chMute[kMidiChannels]{};
    GtkToggleButton* m_chSolo[kMidiChannels]{};
    GtkToggleButton* m_chLock[kMidiChannels]{};
    GtkLevelBar* m_chMeter[kMidiChannels]{};
    GtkScale* m_chVol[kMidiChannels]{};
    GtkDropDown* m_chPatch[kMidiChannels]{};
    GtkCheckButton* m_pianoShow[kMidiChannels]{};
    double m_chLevel[kMidiChannels]{};
    bool m_chSoloed[kMidiChannels]{};
    bool m_pianoVisible[kMidiChannels]{};
    bool m_notesOn[kMidiChannels][128]{};
    int m_noteVel[kMidiChannels][128]{};
    guint m_meterTimer{};

    SequencePlayer m_player;
    MidiOutputManager m_outputs;
    Playlist m_playlist;
    int m_lyricTrackFilter{-1};
    int m_lyricTypeFilter{5};
    bool m_seeking{};
    bool m_tightenKeys{};
    int m_repeat{};
    bool m_refreshingLyrics{};
};

} // namespace dmidi
