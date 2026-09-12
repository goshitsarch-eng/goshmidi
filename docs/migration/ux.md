# UX inventory & libcosmic mapping — Gosh MIDI Player

- App: Gosh MIDI Player (`com.goshapps.GoshMIDI`, command `dmidiplayer`), C++17, GTK4 + libadwaita.
- Sources inventoried: `src/ui/window.cpp` (2170 lines), `src/ui/window.hpp`,
  `src/main.cpp`, `src/app/settings.hpp`, `src/app/settings.cpp`,
  `src/app/playlist.hpp`, `src/app/playlist.cpp`, `src/midi/player.hpp`,
  `src/midi/output.hpp`, `data/style.css`, `com.goshapps.GoshMIDI.desktop`, `README.md`.
- Branch: `cosmic-migration`, Phase 1. This doc is the UX teammate's owned file;
  no source changes were made for it.
- Target: libcosmic (Rust) on iced, following COSMIC conventions:
  header bar + nav/sidebar, segmented control for view switching, settings pages
  with sections, modal dialogs, toaster for transient messages, portal file
  choosers, GKeyFile-compatible INI files for persistence (per
  `architecture.md` §4 — the same `dmidiplayer.conf` schema, not
  `cosmic-config`).

How to read: each section lists the GTK4 ground truth with `file:line`
pointers, then the libcosmic equivalent. "Closest faithful alternative" marks
places where iced/libcosmic has no 1:1 widget.

## 1. Windows

| ID | GTK implementation | Behavior | libcosmic equivalent |
|---|---|---|---|
| W1 Main window | `AdwApplicationWindow`, `src/ui/window.cpp:1461-1469`, default size from settings (`windowWidth` 1100 x `windowHeight` 720). NOT saved today: the destructor save (`window.cpp:272-284`) is dead code — `main.cpp` allocates `MainWindow` with `new` and never deletes it, so size + `repeatMode` are silently lost on quit. Lead ruling: FIX — save size + `repeatMode` on `close-request` (PLAN T8) | Single window. Title `Gosh MIDI Player — <file>` on song load (`window.cpp:387-388`), else `Gosh MIDI Player`. Single-instance via `G_APPLICATION_HANDLES_OPEN` (`src/main.cpp:108-114`): `activate` presents, `open` loads paths then presents. Whole-window drag-drop (`GtkDropTarget` `GDK_TYPE_FILE_LIST`, `window.cpp:2130-2132`, handler `window.cpp:80-100`): accepted suffixes = supported MIDI + `.lst`; drop replaces playlist (`openFiles(paths, true)`). Content = `AdwToastOverlay` > `AdwOverlaySplitView` (sidebar fraction 0.28) | `cosmic::Application` single window + `run_single_instance`-style guard. Header bar via `cosmic::widget::header_bar`. Layout: `row` with collapsible playlist pane (COSMIC convention: `nav_bar::Model` — a `segmented_button::SingleSelectModel` alias (there is no `NavBarModel`) — or a `container` sidebar with toggle) + main `column`. Persist size in the same GKeyFile INI config (`architecture.md` §4). Title = app name + current file. DnD verdict (T0): X11 window `FileDropped` subscription + Wayland `dnd_destination` with manual `text/uri-list` parsing + sandboxed `file_transfer`; portal chooser stays primary; COSMIC Wayland runtime-confirm in T10. Drop replaces playlist (`openFiles(paths, true)`) where the backend supports it, else prominent Open/Add buttons. |

Screenshot reconstruction from code: left playlist sidebar with bottom icon
toolbar; right side has a transport header bar, a dark rounded dashboard
(time + tempo/volume/pitch + song title + rhythm lamps + bar:beat), position
and tempo/volume sliders, a Lyrics/Channels/Piano switcher, and a dim status
line at the bottom. Toasts overlay everything.

## 2. Dialogs

| ID | GTK implementation | Contents & behavior | libcosmic equivalent |
|---|---|---|---|
| D1 Open MIDI files | `GtkFileDialog:open_multiple`, `window.cpp:710-747`, title `Open MIDI files`, filter `MIDI / Karaoke / Cakewalk / Playlist`: `*.mid *.midi *.kar *.rmi *.wrk *.lst` | Multi-select. Result → `openFiles(paths, true)` (replace playlist; single `.lst` → playlist load, `window.cpp:351-370`). No explicit initial-folder wiring (relies on portal/GTK default; `lastDirectory` is updated on load but not passed to the dialog). | Portal file chooser (`ashpd` multi-select over xdg-portal; NEVER `rfd`) with same 6 filters. COSMIC convention: dialog invoked from header-bar Open + playlist Add. T0 BLOCKED: `lastDirectory`-as-initial-folder is blocked upstream — open-dialog `directory`/`file_name` builders are dead (`#[allow(dead_code)]`, ignored); save-dialog initial names work. Keep carrying `lastDirectory` until upstream fixes open. |
| D2 Preferences | `AdwPreferencesWindow`, `window.cpp:1189-1359`, transient for main, 3 pages, save-on-`close-request` (`window.cpp:1310-1340`): applies drums channel, SysEx reset, instrument map, lyrics refresh, `AppSettings::save()` | General page (7 rows): Percussion MIDI channel spin 1–16; Solo % reduction spin 0–100; `Start playback after loading` switch; `Advance to next playlist item` switch; `Automatically load and save song settings` switch; SysEx Reset combo (None/GM/GS SC-55/XG/MT-32); Instrument map combo (GM/GS SC-55/MT-32). Lyrics page (5 rows): Future/Past/Highlight color entries (free text parsed by `gdk_rgba_parse`, `window.cpp:39-44`: `#rgb`/`#rrggbb` + alpha variants, `rgb()/rgba()`, CSS named colors; garbage falls back to gray); Lyrics font entry (`Sans 16`); alignment combo (Left/Center/Right). Piano page (5 rows): Note highlighting combo (Single/Double/Channels/Scale); Single highlight color entry; velocity→tint switch; note-names combo (Never/Minimal/When activated/Always); octave-subscript switch. No Reset/Cancel: closing commits. | COSMIC settings window: `tab_bar` or `segmented_button` with General/Lyrics/Piano sections built from `cosmic::widget::settings::section` + `spin_button`, `toggler`, `dropdown`, `text_input` rows (T0: `toggler` is a private mod — use the `cosmic::widget` root re-export, not `widget::toggler::…`; `widget::stack` does NOT exist — use `cosmic::iced::widget::Stack` as the green probe did, main.rs:163). Commit-on-close is KEPT (lead ruling — do NOT switch to commit-on-change); `soloVolumeReduction` applies at close along with the other General rows. Color rows: ruling D17 — use `widget::color_picker` bound to the `text_input`; the text field stays source of truth with `gdk_rgba_parse` parity; fallback to text-only if T11 integration fails. Parser parity required: accept the same set as `gdk_rgba_parse` (hex, `rgb()/rgba()`, named colors) with gray fallback on failure, plus tests. Font rows: `text_input` (no Pango font picker; document as gap). Font-string spec: parse Pango `<family> [style] <size>` (e.g. `Sans 16`); on parse failure fall back to system sans at the default size; tests. Lead ruling D15 FIX: the C++ `lyricsFont` default is dead CSS (invalid `font:` provider string, dropped by GTK) — the port parses + applies per this spec. |
| D3 MIDI Setup | Modal `GtkWindow` 480x420, `window.cpp:929-1187`, transient+modal, destroyed on Apply; state held in local `Widgets` struct | Widgets in order: Output backend `GtkDropDown` (`backendNames()`); `Show all ports` check + `Refresh ports` button; Connection `GtkDropDown` (label list, id carried separately); Instrument map dropdown (GM/GS/MT-32); SysEx reset dropdown (`sysexResetName(0..4)`); SoundFont entry (hexpand, placeholder `Optional: your GM, MT-32, or SC-55 SoundFont`) + `Browse…` (opens D11); dim status label (wrap); button row right-aligned: `Test note` + `Apply` (suggested). Logic: `refillFn` rebuilds port list on backend/`Show all`/Refresh; preselects current/saved port by id-or-substring; empty → status `No destinations. Start mt32emu-qt (Munt) or Nuked-SC55, then Refresh.`; port change auto-sets map+reset via `inferDeviceProfile(label)`; Test: saves SoundFont, `connectOutput(backend, portId)`, sends Middle C ch1 96 for 400 ms, status `Sent Middle C on channel 1` / `No MIDI output`; Apply: saves soundFont/advancedPorts/map/reset, `setSysexReset`, `setSoundFont`, `connectOutput`, `applyInstrumentMap`, `save()`, destroy. | Modal `cosmic::widget::dialog` with same field order and button row. Backend/connection/map/reset → `dropdown`. `Show all ports` → `checkbox(bool).label(…)` (T0: iced checkbox is 1-arg); Refresh → `button`. SoundFont → `text_input` + Browse button. Status → dim `text`. Keep refill + profile-inference + test-note + suggested-Apply semantics exactly. Backend names come from the Rust MIDI layer (ALSA/FluidSynth/Dummy equivalents). |
| D4 About | `AdwAboutDialog`, `window.cpp:884-920` | App name `Gosh MIDI Player`, icon `com.goshapps.GoshMIDI`, version `VERSION`, developer `Pedro López-Cabanillas`, copyright `© 2006–2026 …`, license GPL-3.0, website `github.com/goshitsarch-eng/goshmidi`, support `dmidiplayer.sourceforge.io`, issues `…/goshmidi/issues`, comments noting GTK4 rewrite + original + `dmidiplayer` command/settings compat, developers list (original + contributors), credit section `Original application`, 3 links (SourceForge, GitHub, docs). | `cosmic::widget::about`-style dialog (or `dialog` with `about` content; T0: `widget::about` needs the non-default `about` feature): same strings/links. COSMIC convention: About lives in app menu footer. |
| D5 Help | `AdwAlertDialog(Help, kHelpText)`, `window.cpp:922-927`, text `window.cpp:23-37` + OK | Static multi-paragraph help: views, supported extensions, playback/transpose/tempo/volume, `.lst` format + drag-drop + repeat, backends + MIDI Setup, Munt/Nuked start order + port names + companion `.syx`, song-settings path `~/.dmidiplayer/<song>.cfg`. | `dialog` with `scrollable` + `text`. Same copy (update paths if Rust build changes config locations; keep `~/.dmidiplayer/<song>.cfg` until decided). |
| D6 File info | `AdwAlertDialog(File information, body)`, `window.cpp:749-773` + OK | Body is dynamic Pango markup: `No file loaded`, else File/Format/Duration/Initial tempo/Tracks/Division + `metadataInfo()` + per-copyright lines. Source: `Sequence` getters (`currentFile`, `fileFormat`, `durationString`, `initialTempo`, `getNumTracks`, `division`). | `dialog` with header + `scrollable` metadata list. Same fields in same order. |
| D7 Jump to bar | Modal `GtkWindow`, `window.cpp:775-829`, transient+modal, no-op when song empty | `Bar number:` label + `GtkSpinButton` range 1..`lastBar()` (min 1) + `Jump` button. Click: pause if running, `jumpToBar(bar)`, resume if was running, destroy. | Modal `dialog` with `spin_button` 1..lastBar + Jump/Cancel. Same pause-jump-resume semantics. |
| D8 Loop | Modal `GtkWindow`, `window.cpp:831-882`, transient+modal, no-op when song empty; opened by header loop toggle ON (`window.cpp:1638-1645`) and by `app.loop` menu action | `From bar` + `To bar` spins (1..last, seeded from `loopStart/loopEnd`) + `Loop` button → `setLoop(a, c)`, force toggle ON, destroy. Toggle OFF anywhere calls `setLoop(false)` directly without dialog. BUG: the `app.loop` menu item with the toggle OFF hits the early return (`window.cpp:835-838`) and is a silent no-op — the dialog never opens. Lead ruling: FIX — menu Loop forces the toggle ON, then opens the dialog. `close-request` destroys and unchecks the toggle if `loopEnabled()` is false. | Same two-entry dialog + toggle-button pairing. Closest COSMIC shape: header-bar `button` with `selected` state + modal dialog for range. Keep the cancel-resets-toggle rule. Menu Loop = force toggle ON + open dialog (FIX ruling, not the silent no-op). |
| D9 Open playlist | `GtkFileDialog:open`, `window.cpp:1527-1548`, title `Open playlist` (no filter) | On file: `playlist.load(p)`, `lastPlayList = p`, `refreshPlaylistView()`. Does not auto-load a song (unlike D1). | Portal single-file chooser, `.lst` filter added (parity-safe). Same post-load behavior. |
| D10 Save playlist | `GtkFileDialog:save`, `window.cpp:1549-1569` (title + initial name `window.cpp:1552-1553`), initial name `playlist.lst` | On file: `playlist.save(p)`, `lastPlayList = p`. | Portal save chooser, same initial name + `.lst` filter. |
| D11 Choose SoundFont | `GtkFileDialog:open` from D3 Browse, `window.cpp:1091-1119`, title `Choose SoundFont`, filter `*.sf2 *.sf3` (both cases) | On file: sets D3 entry text only; committed by D3 Test/Apply. | Portal single-file chooser, same filters; same deferred-commit rule. |
| D12 Save lyrics | `GtkFileDialog:save` from lyrics Save, `window.cpp:1822-1842`, initial `lyrics.txt` | Writes whole buffer text via `g_file_set_contents`. | Portal save chooser + same full-text write. |
| D13 Print lyrics | `GtkPrintOperation` print dialog, `window.cpp:1843-1868`, 1 page, Pango layout of buffer text | Native print dialog → draws page via cairo/Pango. | No iced print backend. Faithful alternatives in order: (a) export path (D12) + `xdg-open`; (b) `ashpd` print portal from Rust. Keep a Print entry that routes to (a)/(b); document as known gap, not silent removal. |

Toasts (`AdwToastOverlay`, `window.cpp:1465-1469`, helper `window.cpp:286-289`),
6 call sites: `Saved MIDI port is unavailable; choose it again in MIDI Setup`
(startup fallback, `window.cpp:236`), `Unable to open MIDI output`
(`window.cpp:332`) / backend `lastError()` (`window.cpp:342`) / `MIDI output:
<backend> <port>` (`window.cpp:347`) on `connectOutput`, `Could not load <file>`
(`window.cpp:379`) and `Warning: non-standard or damaged file`
(`window.cpp:381`) on `loadCurrent` failure. Map to
`cosmic::widget::toaster` (there is no `cosmic::app::toaster`) with the same
strings and triggers.

## 3. Widgets — main window by region

### 3.1 Playlist sidebar (`window.cpp:1471-1579`)

- Container: sidebar of `AdwOverlaySplitView` + `AdwToolbarView`, header title
  `Playlist`, `GtkListBox` single-select with `navigation-sidebar` class in a
  `ScrolledWindow`. Rows show basename only (`filename`), middle-ellipsized,
  left-aligned; `index` in object data; selection tracks `currentIndex`
  (`refreshPlaylistView`, `window.cpp:497-516`). `row-activated`
  (`window.cpp:1572`) → `setCurrentIndex(i)` + `loadCurrent(true)`: a single
  click only selects; play requires double-click or Enter.
- Bottom toolbar (`toolbar` class, 8 flat icon buttons in order):
  Add (`list-add-symbolic` → D1), Remove (`list-remove-symbolic` → remove
  selected), Move up/down (`go-up/down-symbolic`), Shuffle
  (`media-playlist-shuffle-symbolic`), Clear (`edit-clear-symbolic`), Open
  playlist (`document-open-symbolic` → D9), Save playlist
  (`document-save-symbolic` → D10).
- libcosmic: sidebar `container` + `scrollable` + `column` of `button`/`list`
  rows (or `nav_bar::Model` (= `segmented_button::SingleSelectModel`)
  items if the playlist fits nav-bar idiom; a custom
  list keeps basename+selection+reorder fidelity better). Toolbar →
  `row` of `button::icon`. Keep order, tooltips, and the
  click-select vs double-click/Enter-play mapping, with a test. COSMIC
  convention: collapsible via
  header-bar sidebar toggle; persist visibility (`playlistVisible`).

### 3.2 App menu + header transport (`window.cpp:1581-1647`)

- Hamburger `GtkMenuButton(open-menu-symbolic)` → `GMenu` with 4 sections:
  File: Open… (`app.open`), File information (`app.info`), Load song settings
  (`app.load-settings`), Save song settings (`app.save-settings`), Search MIDI
  files (`app.search` → `https://midisite.co.uk`), Quit (`app.quit`).
  Playback: Play/Pause/Stop/Previous/Next/Rewind 1 bar/Forward 1 bar/Jump to
  bar…/Loop… (`app.play/pause/stop/prev/next/rew/fwd/jump/loop`).
  Repeat: Repeat off/song/playlist (`app.repeat-off/song/list`).
  Help section (no title): MIDI Setup (`app.midi`), Preferences (`app.prefs`),
  Help (`app.help`), Original dmidiplayer (`app.website` →
  `https://sourceforge.net/p/dmidiplayer/`), About (`app.about`).
- Header-bar start (flat, in order): Open, Prev, Play, Pause, Stop, Next, Rew,
  Fwd as `GtkButton(action-name)` + loop `GtkToggleButton`
  (`media-playlist-repeat-symbolic`, tooltip `Loop`).
- libcosmic: header bar with the same 8 transport buttons + loop toggle
  (`button::icon`, loop uses `selected` style). Menu → `cosmic::widget::menu`
  (`menu::bar` + `menu::root`/`menu::items` over `menu::Tree` — there is no
  `menu::MenuTree`) with the same 4 groups, or COSMIC app-menu + context
  drawer split (File/Playback in menu, MIDI Setup/Preferences/Help/About in
  app-menu footer). External links via `open::that`/portal `open_uri`.

### 3.3 Dashboard (`window.cpp:1649-1691`, style `data/style.css:1-24`)

- `.player-dashboard` horizontal box: giant `.time-display` label `HH:MM:SS`
  (40px, bold, mono-ish, updated by `updateTime`, `window.cpp:296-308`);
  stats grid: `Tempo` → `.dash-value` `%.1f bpm` (live from `onTempo` +
  `applyTempo`), `Volume` → `100%` (from `applyVolume`), `Pitch` →
  `GtkSpinButton` -12..+12 step 1 (→ `setPitchShift`, percussion excluded
  downstream); right column: song-title label (left, middle-ellipsize,
  `No file loaded` default), rhythm-lamp box (initial 4 lamps; `onBeat`
  resizes to time-signature beats, beat lamp gets `.on`, downbeat `.accent`,
  `window.cpp:111-143`), position label `bar:beat` (`1:1` default).
- libcosmic: `container` card with rounded style + `text` widgets; pitch →
  `spin_button` -12..+12; rhythm lamps → `row` of small circular `container`
  dots restyled per beat (custom `container::Style`); keep resize-to-meter,
  accent-downbeat, and `bar:beat` text. Live updates via iced subscription /
  `Message::Tick`.

### 3.4 Position / tempo / volume controls (`window.cpp:1692-1747`)

- `m_posScale` 0..1000 step 1, no value bubble, hexpand; live set from ticks
  unless seeking; `change-value` → `setPosition(value/1000*len)` with
  `m_seeking` guard.
- `m_tempoScale` 50..200 default 100 (tooltip `Tempo 50–200%`) +
  `Tempo` reset button → 100; `m_volumeScale` 0..200 default 100 (tooltip
  `Volume 0–200%`) + `Volume` reset button → 100. Margins 16/16.
- libcosmic: three `slider`s with the same ranges/defaults + two text
  `button`s (Tempo/Volume reset). Keep the seek guard and reset-to-100 rules.

### 3.5 View switcher (`window.cpp:1748-1754`)

- `AdwViewStack` + `AdwViewSwitcher`, policy WIDE. Pages: `lyrics` (Lyrics,
  `audio-headphones-symbolic`), `channels` (Channels,
  `audio-speakers-symbolic`), `piano` (Piano, `audio-x-generic-symbolic`).
  NOTE: README (`README.md:38`) and `com.goshapps.GoshMIDI.metainfo.xml:18`
  advertise a "rhythm view" but code implements rhythm as dashboard lamps,
  not a stack page — parity target follows code (3 pages); both wordings
  are doc bugs in fix scope.
- libcosmic: `segmented_button` (COSMIC convention for view switching) or
  `tab_bar` + content `cosmic::iced::widget::Stack` (there is no
  `widget::stack`; green probe main.rs:163); same 3 pages, icons, and order.

### 3.6 Lyrics page (`window.cpp:1755-1869`)

- Toolbar: `Track` label + dropdown (`All tracks` + `Track N — <name>`,
  preselect `trackMaxPoints()`), `Type` label + dropdown (14 `kTypeNames`:
  All types/Text/Copyright/Track name/Instrument/Lyrics/Marker/Cue point/KAR
  type/KAR version/KAR info/KAR language/KAR titles/KAR other, preselect
  `typeMaxPoints()` default Lyrics), `Encoding` label + dropdown
  (`Sequence::extraCodecNames()`, preselect `currentCharset()`), Copy
  (`edit-copy-symbolic`), Save (`document-save-symbolic` → D12), Print
  (`document-print-symbolic` → D13) icon buttons.
- Body: read-only `GtkTextView` (word-char wrap, `.lyrics-view` 16pt),
  `ScrolledWindow` vexpand. `refreshLyrics` (`window.cpp:550-629`) prefills
  filtered events (non-lyric records get trailing newline), applies `future`
  tag to all, justification from `textAlignment`, font CSS from `lyricsFont`
  (`Sans 16` default; T0: the C++ `font:` provider string is invalid CSS and
  dropped by GTK, so the effective default is the 16pt `.lyrics-view` rule).
  Live `onText` (`window.cpp:157-179`): un-highlights
  old `current`, marks all `past`, appends decoded text as `current`,
  scrolls to mark. Copy writes whole buffer to clipboard. QUIRK (lead
  ruling: PRESERVE + test): `refreshLyrics` prefills every filtered event
  into the buffer AND live `onText` appends each sung event again, so sung
  lines duplicate (prefill + live append). Preserve the duplication.
- libcosmic: toolbar `row` with 3 `dropdown`s + 3 icon buttons; body
  `scrollable` + styled text. No `TextView` tag equivalent: faithful
  alternative is a `text_editor` (read-only) or a `column` of `text::Rich`
  (`text::Span`) runs rebuilt on filter/live events (there is no `rich_text`
  module, but the `iced::widget::rich_text()`/`span()` helpers exist —
  helpers.rs:1208), with future/past/current colors,
  alignment, and auto-scroll-to-latest. Keep preselect rules, filter
  semantics, copy/save/print triggers, and the prefill+append duplication
  (with a test).

### 3.7 Channels page (`window.cpp:1870-1947`)

- `ScrolledWindow` + vertical box (margins 8/8). Shared `GtkStringList`
  patch model (128 `patchName(map, p)`, rebuilt by `applyInstrumentMap`).
  16 rows, each: number label (`1`..`16`), name `GtkEntry` (hexpand, from
  `channelLabel`), `M` toggle, `S` toggle, `GtkLevelBar` meter (0..1,
  80x10; 50 ms decay `*=0.82`, `window.cpp:217-225`; peak set on NoteOn),
  volume `GtkScale` 0..200 default 100 (no bubble, 100px), lock toggle
  (`channel-secure-symbolic` → `setLocked`), patch `GtkDropDown` (180px,
  → `setPatch` unless `m_refreshingChannels`; live program events reselect).
  Row visible only if `channelUsed(i)` (`refreshChannels`,
  `window.cpp:533-548`). M/S/volume combine in `applyChannelSoloMute`
  (`window.cpp:479-495`): any-solo → non-solo scaled by
  `(100 - soloVolumeReduction)/100`, times per-channel vol/100; mute →
  `setMuted`.
- libcosmic: `scrollable` + `column` of row `container`s with identical
  fields: `text` + `text_input` + `M`/`S` toggle `button`s (selected style)
  + meter + `slider` + lock icon `button` + patch `dropdown`. Meter →
  `progress_bar` on a 50 ms tick subscription (or small custom `canvas`);
  keep decay, used-only visibility, shared patch-model rebuild on map
  change, and the exact solo/mute/volume combine formula.

### 3.8 Piano page (`window.cpp:1948-2119`, style `.piano-area`)

- Toolbar: `Tighten keys` check (narrows `lo/hi` to
  `lowestNote-2..highestNote+2`, default full 21..108) + `Show all` (all 16
  channels visible) + `Hide unused` (visible = `channelUsed`) buttons.
- `GtkDrawingArea` vexpand, cairo `draw_func`: dark `#241f31` background;
  one row per visible used channel (`rowH = max(48, h/used)`); white keys
  `#f2f2ed` (highlighted → `channelColor(ch, vel)`: 16-color palette, or
  `singleColor` when `highlightPalette==0`, velocity-scaled when
  `velocityColor`); black keys `#1a1a1f` overlay 60% width, 62% height;
  note names per `namesVisibility` (Never/Minimal=C-only/When-active/Always,
  9px Sans `C#octave`, `octaveSubscript` currently renders the same text —
  carry the flag, rendering is a no-op today); channel footer
  `Ch N <label>`. Click press → `sendNoteOn(ch, whiteKeyNote, 80)` +
  light key; release → NoteOff all UI-lit notes when not playing.
  Black-key clicks map to neighboring white key (existing behavior —
  preserve). BUG: the click handler uses a fixed 21..108 range
  (`window.cpp:2081`) while the draw path tightens to
  `lowestNote-2..highestNote+2` (`window.cpp:1981-1983`), so clicks miss
  under Tighten. Lead ruling: FIX — clicks map within the drawn range.
- libcosmic: `canvas` widget with a `Program` implementing the same layout
  + mouse `press/release` handling. Toolbar → `checkbox(bool).label(…)` + 2 `button`s.
  Keep tighten/show/hide, palette/velocity/names rules, and audition
  semantics; clicks quantize to white keys within the drawn (possibly
  tightened) range. Colors come from theme + settings, not hardcoded CSS.

### 3.9 Status line

- `dim-label` left label, margins start 12 / bottom 6 (`window.cpp:2121-2126`).
  States: `Ready` (init), `Playing` (`onStarted`/`play`), `Paused`,
  `Stopped` (`onStopped`/stop/load-no-autoplay), `Finished` (`onFinished`).
- libcosmic: dim `text` footer with the same 5 strings and transitions.

## 4. Actions & keyboard shortcuts (`window.cpp:1361-1456`)

23 `GSimpleAction`s on the app (name → handler): 20 in `bindActions`
(`window.cpp:1419-1438`) + 3 repeat actions (`window.cpp:2150-2152`).
Accelerators via `gtk_application_set_accels_for_action`. Header buttons and
menu items invoke the same actions; the hamburger menu holds all 23 items in
4 sections (§3.2: 6 file + 9 playback + 3 repeat + 5 help).

| Action | Label / handler | Accel | libcosmic equivalent |
|---|---|---|---|
| `app.open` | Open… → D1 | `Ctrl+O` | Menu item + header button + `Ctrl+O` key subscription |
| `app.quit` | Quit → `g_application_quit` | `Ctrl+Q` | Menu item + `Ctrl+Q`; COSMIC closes via window-close / app exit message |
| `app.play` | Play (`play()`; empty song → `loadCurrent(true)`) | `Space` | Transport button + menu + `Space` (ensure Space doesn't trigger focused button twice — scope to window) |
| `app.pause` | Pause/resume toggle | `Ctrl+P` | Same triggers + `Ctrl+P` |
| `app.stop` | Stop + reset position | `Ctrl+S` | Same + `Ctrl+S` |
| `app.next` / `app.prev` | Next/prev song (next saves song settings when auto) | `Ctrl+Right` / `Ctrl+Left` | Same + arrow subscriptions |
| `app.rew` / `app.fwd` | `beatBackward()` / `beatForward()` (1 bar) | none | Header buttons + menu; ~~consider `Shift+Left/Right`~~ — REJECTED by lead ruling D22 (T14d): no invented bindings; Tab-Enter via header buttons (G3: counted reachable) |
| `app.jump` / `app.loop` | D7 / D8 | none | Menu + (loop also header toggle); ~~consider `Ctrl+J`/`Ctrl+L`~~ — REJECTED by D22 (T14d): no invented bindings; Jump menu-only (G3), Loop Tab-Enter via header toggle (G3: counted reachable) |
| `app.prefs` | D2 | `Ctrl+,` | App-menu footer + `Ctrl+,` |
| `app.midi` | D3 | none | App-menu entry; ~~consider `Ctrl+M`~~ — REJECTED by D22 (T14d): menu-only (G3) |
| `app.help` | D5 | `F1` | App-menu entry + `F1` |
| `app.info` | D6 | none | Menu entry; ~~consider `Ctrl+I`~~ — REJECTED by D22 (T14d): menu-only (G3) |
| `app.save-settings` / `app.load-settings` | Per-song `.cfg` write/read (`window.cpp:631-708`) | none | Menu entries; keep manual path even when auto mode is on |
| `app.search` | Open `https://midisite.co.uk` | none | Menu entry → portal open-uri |
| `app.website` | Open `https://sourceforge.net/p/dmidiplayer/` | none | Menu entry → portal open-uri |
| `app.about` | D4 | none | App-menu footer About |
| `app.repeat-off/song/list` | `m_repeat = 0/1/2` (`window.cpp:2140-2152`), persisted to `repeatMode` | none | Menu radio group or segmented control; same 3 modes + persistence |
| (no action) loop toggle | Header `GtkToggleButton` directly drives D8/`setLoop(false)` | none | Header toggle button with `selected` state, same pairing |
| (no action) playlist tools | 8 sidebar buttons (§3.1) | none | Sidebar buttons; no shortcuts today — keep none unless COSMIC review wants them |

COSMIC convention: shortcuts shown on menu items; global handling through an
iced keyboard subscription mapped to the same `Message` variants as the
buttons. Do not drop the 9 existing accelerators.

## 5. Settings

Persisted in `dmidiplayer.conf` (`General` group, `src/app/settings.cpp:75-200`;
default `~/.config/dmidiplayer/dmidiplayer.conf`, portable override via
`-p/-f`, `src/main.cpp:77-84,102-103`). 30 keys total (`settings.hpp:13-42`).
`repeatMode`/`windowWidth/Height` are MEANT to be written on destroy but the
destructor save is dead code (see W1) — FIX in the port via `close-request`
(PLAN T8); most others written on D2-close / D3-Apply / MIDI connect.
Persistence stays GKeyFile-compatible INI per `architecture.md` §4 (same
schema, unknown keys preserved); `cosmic-config` is NOT used.

| Key | Type / default (`settings.hpp`) | UI surface today | libcosmic (GKeyFile INI, same keys) |
|---|---|---|---|
| `lastDirectory` | string / "" | Updated on song load; not wired to D1 initial folder | Same key; carry only — initial-folder wiring BLOCKED per D1 (open-dialog directory dead upstream) |
| `lastOutputBackend` | string / `FluidSynth` | D3 backend; startup select + `-d` override | Same |
| `lastOutputConnection` | string / `fluidsynth` | D3 connection; startup select + `-c` override | Same |
| `lastPlayList` | string / "" | Loaded at startup; set by D1(`.lst`)/D9/D10 | Same |
| `soundFont` | string / "" | D3 entry; `setSoundFont` on startup/Test/Apply | Same |
| `language` | string / "" | NONE (stored, never read by UI) | Carry the key headless; i18n is explicitly deferred — no locale work in this migration |
| `drumsChannel` | int 1–16 / 10 | D2 General spin → `setDrumsChannel(ch-1)` | `spin_button` 1–16 |
| `soloVolumeReduction` | int 0–100 / 50 | D2 General spin, used in solo formula, applied on D2 close | `spin_button`/`slider`, applied at close (commit-on-close kept) |
| `autoPlay` | bool / true | D2 switch; gates `loadCurrent(auto)` | `toggler` |
| `autoAdvance` | bool / true | D2 switch; gates `onFinished → nextSong` | `toggler` |
| `autoSongSettings` | bool / false | D2 switch; gates auto save (next/finish) + auto load | `toggler` |
| `advancedPorts` | bool / false | D3 check (`Show all ports`) | `checkbox(bool).label(…)` |
| `sysexReset` | int 0–4 / 1 (GM) | D2 combo + D3 combo + auto-profile | `dropdown` (None/GM/GS/XG/MT-32) |
| `instrumentMap` | int 0–2 / 0 (GM) | D2 combo + D3 combo + auto-profile → patch model rebuild | `dropdown` (GM/GS/MT-32) |
| `highlightPalette` | int / 2 (Channels) | D2 Piano combo (Single/Double/Channels/Scale; only `0` changes rendering today) | `dropdown`, same 4 items; preserve current rendering semantics |
| `velocityColor` | bool / true | D2 Piano switch → channel-color scaling | `toggler` |
| `octaveSubscript` | bool / false | D2 Piano switch (no-op in cairo text today) | `toggler`; keep flag even though render is unchanged |
| `namesVisibility` | int 0–3 / 1 (Minimal) | D2 Piano combo | `dropdown` |
| `lyricsFont` | string / `Sans 16` | D2 entry → lyrics CSS (default is dead CSS, dropped) | `text_input` → parsed + applied text style (D15 FIX) |
| `notesFont` | string / `Sans 9` | NONE (stored, never applied; piano uses 9px Sans literal) | Carry the key headless; do NOT wire to piano note labels (lead ruling — wiring dropped) |
| `futureColor`/`pastColor`/`singleColor`/`highlightColor` | strings / `#888888` `#1c71d8` `#f6d32d` `#e01b24` | D2 entries (single = piano single mode) | `text_input` + swatch; `gdk_rgba_parse` parity (hex, `rgb()/rgba()`, named colors, gray fallback) + tests |
| `textAlignment` | int 0–2 / 0 (Left) | D2 combo → `GtkJustification` | `dropdown` → iced alignment |
| `recentFiles` | string list, max 10 | NONE visible (updated on load, `addRecent`) | Carry; COSMIC options: recent submenu/section or keep headless. Do not silently drop the data |
| `windowWidth`/`windowHeight` | ints / 1100x720 | Applied at startup, saved at destroy | Window size + config keys |
| `playlistVisible` | bool / true | NONE (stored, never applied to split view) | Carry; wire to sidebar toggle (fixes a latent bug, keeps data) |
| `repeatMode` | int 0–2 / 0 | Repeat menu; `m_repeat` runtime copy | Same 3 modes + persistence |

Per-song settings (`~/.dmidiplayer/<song>.cfg`, `window.cpp:631-708`):
`Global`: file, encoding, volume, pitch, timeskew; per used channel
`MIDI Channel NN`: name, muted, solo, locked, patch, level. Manual via
`app.save/load-settings`; automatic when `autoSongSettings`. Map to the same
file schema from Rust (GKeyFile-compatible INI) until a config migration is
designed; keep manual + auto paths.

## 6. User flows

| ID | Flow | GTK steps | COSMIC notes |
|---|---|---|---|
| F1 First launch | Start with no state | Load config → select saved backend/port (fallback FluidSynth → Dummy + toast) → load `lastPlayList` or examples/`examples.lst` or `examples/` dir scan → `refreshPlaylistView` (`window.cpp:227-270`) | Same fallback chain + toasts; bundle or install example playlist equivalently |
| F2 Open & play | Open files | `app.open`/Add/D1 or CLI args/`open` signal or DnD → `openFiles` → `loadCurrent(autoPlay)` → title/time/bpm/channels/lyrics/piano refresh, optional song-settings load, optional `play()` (`window.cpp:351-404`) | Same; CLI `%F` files → same path |
| F3 Playlist manage | Add/remove/reorder/shuffle/clear/save/load | Sidebar 8 tools + row-activated play; `.lst` = one path/line, relative resolved to list dir (`playlist.cpp:15-50`) | Same ops + `.lst` format; keep "open playlist doesn't auto-play" vs "open files auto-plays" distinction |
| F4 Transport | Play/pause/stop/prev/next/±1 bar | Header buttons + menu + shortcuts; `beatForward/Backward`; stop resets to 0 and clears piano | Same; keep prev/next auto-play + next auto-save |
| F5 Seek/shape | Position/tempo/volume/pitch | Pos slider seek; tempo 50–200 (live bpm label); volume 0–200 (live %); pitch -12..+12; Tempo/Volume reset buttons | Same ranges/defaults/resets |
| F6 Jump | Go to bar | D7 pause-jump-resume | Same |
| F7 Loop | Repeat bars | Toggle ON → D8 range → `setLoop(a,c)`; toggle OFF → `setLoop(false)`; cancel resets toggle; menu Loop with toggle OFF is a silent no-op today | Same pairing + FIX: menu Loop forces toggle ON, then opens D8 |
| F8 Repeat | End-of-song behavior | `onFinished`: repeat-song → reset+play; repeat-list → next-or-first + `loadCurrent(true)`; else auto-advance unless last (`window.cpp:194-214`); auto-save when enabled | Same decision tree |
| F9 MIDI setup | Choose output | D3 refill → profile inference → Test note → Apply; named ports (MT-32/SC-55) pick map+reset; missing ports → status hint; Munt/Nuked must start first + Refresh | Same, including hint strings and start-order docs |
| F10 Lyrics | Read/export karaoke text | Filter by track/type/encoding → live highlight + autoscroll → copy/save/print | Same filters + live colors + autoscroll; print via §D13 alternative |
| F11 Mix | Mute/solo/lock/patch/level per channel | 16 used-only rows; solo formula; program events reselect patch | Same formula + visibility + reselect |
| F12 Piano | Watch + audition notes | Tighten/show/hide; per-channel rows; click audition (fixed 21..108 range, misses under Tighten) | Same layout + audition; FIX: clicks map within the drawn range; preserve white-key quantization |
| F13 Rhythm | Follow meter | Lamps resize to beats; downbeat accent; `bar:beat` label | Same |
| F14 Song settings | Per-song recall | Manual save/load menu items; auto mode on next-song/finish + load | Same file schema + triggers |
| F15 Inspect/learn | File info, help, about, links | D6/D5/D4 + midisite + sourceforge links | Same content + links |
| F16 Quit | Persist state | Destructor save (`window.cpp:272-284`) is DEAD code — `main.cpp` never deletes `MainWindow`, so size + `repeatMode` are lost on quit today | FIX: `close-request` saves size + `repeatMode` + full config (PLAN T8) |

## 7. Styling / theming (`data/style.css`, loaded `window.cpp:2134-2137`)

| GTK class | Rules | libcosmic equivalent |
|---|---|---|
| `.player-dashboard` | `#111111` bg, `#f6f5f4` text, 16px radius, padding 16/20, margin 8/12 | Card `container` style: theme surface-dark, rounded (COSMIC radius token), same padding/spacing |
| `.time-display` | Cantarell/Noto Sans Mono, 700, 40px, `#f6f5f4` | `text::Style` with mono font + 40px + theme foreground |
| `.dash-label` / `.dash-value` | label `#c0bfbc` 12px; value `#62a0ea` semibold | Dim `text` 12px + accent `text` semibold (theme accent, closest to `#62a0ea`) |
| `.rhythm-lamp` / `.on` / `.accent` | 14px circle; rest `#3d3846`, beat `#57e389`, downbeat `#f66151` | Circular dot `container`s with 3 background styles; same hex until theme review |
| `.piano-area` | `#241f31` bg | Canvas background fill `#241f31` (or theme dark surface) |
| `.lyrics-view` | 16pt (plus runtime `font: <lyricsFont>` provider — invalid CSS, dropped by GTK) | Base 16pt text style; `lyricsFont` parsed + applied (D15 FIX) |
| Ad-hoc classes | `flat` (icon buttons), `toolbar`, `navigation-sidebar`, `dim-label`, `suggested-action` (D3 Apply) | COSMIC `button::icon`, toolbar row spacing, sidebar style, dim text style, `button::suggested` |

COSMIC follows the system theme (light/dark) automatically; hardcoded dark
dashboard/piano fills stay as explicit card/canvas styles so both modes keep
contrast. Spacing convention: 4/8/12/16/18px as in code (controls 4, toolbar
8–12, dashboard 18, dialog margins 18).

## 8. Desktop & OS integration

- Desktop file (`com.goshapps.GoshMIDI.desktop`): Name `Gosh MIDI Player`,
  GenericName `MIDI File Player`, `Exec=dmidiplayer %F`, icon
  `com.goshapps.GoshMIDI`, `Categories=AudioVideo;Audio;Midi;`,
  `MimeType=audio/midi;audio/x-midi;audio/cakewalk;application/x-riff;`,
  comments EN/ES/RU, `StartupWMClass=com.goshapps.GoshMIDI`. Keep all fields;
  Rust binary keeps the `dmidiplayer` command name for compat (README contract).
- CLI (`src/main.cpp:52-101`): `-h/--help`, `-v/--version`, `-p/--portable`,
  `-f/--file FILE`, `-d/--driver DRIVER`, `-c/--connection PORT`, positional
  MIDI files (missing → stderr `File not found`, no dialog). Help/version
  print `Gosh MIDI Player (dmidiplayer) <VERSION>` + attribution. Keep every
  flag and message; `-d/-c` pre-seed the same connect path as D3 Apply.
- Single instance: second launch routes files through `open` into the existing
  window and presents it (`src/main.cpp:34-50`). Keep via libcosmic/iced
  single-instance + portal `open_uri`/DBus activation.
- Icons: header/sidebar/lyrics use freedesktop symbolic names
  (`document-open`, `media-playback-*`, `media-skip-*`, `media-seek-*`,
  `media-playlist-*`, `list-add/remove`, `go-up/down`, `edit-clear/copy`,
  `document-save/print`, `open-menu`, `channel-secure`, `audio-*`). Reuse the
  same names from the COSMIC/freedesktop icon set; only `channel-secure` and
  the app icon may need fallback review.
- Clipboard: lyrics Copy → display clipboard (`window.cpp:1811-1821`). Map to
  iced clipboard write of the same full text.

## 9. Gaps — no direct libcosmic/iced equivalent

1. `GtkSpinButton` (pitch, bar numbers, D2 spins): use `spin_button`
   (text + steppers) with identical ranges/defaults; validate + clamp.
2. `GtkLevelBar` channel meters: use `progress_bar` on a ~50 ms tick or a
   tiny custom `canvas`; keep peak-set + `*=0.82` decay.
3. `GtkTextView` tags (future/past/current) + Pango justification + font CSS:
   use read-only `text_editor` or rebuilt `text::Rich` (`text::Span`) runs; keep 3 colors,
   alignment, preselect/filter rules, append + autoscroll.
4. `GtkDrawingArea` + cairo + click gestures (piano): use `canvas` `Program`
   + mouse events; reimplement the §3.8 layout math as-is.
5. Native print dialog (`GtkPrintOperation`): no iced backend; route Print
   through save + `xdg-open` and/or the `ashpd` print portal.
6. Font picker: none used today (D2 uses text entries), so a Pango-string
   `text_input` + the parse/fallback spec (see D2) is faithful. Color
   picker: ruling D17 — `widget::color_picker` bound to the text field
   (text field stays source of truth); fallback to text-only if T11
   integration fails; keep `gdk_rgba_parse` parity (hex, `rgb()/rgba()`,
   named colors, gray fallback) + tests.
7. System file-drop onto the window: T0 verdict — X11 window
   `FileDropped` subscription + Wayland `dnd_destination` with manual
   `text/uri-list` parsing + sandboxed `file_transfer`; portal choosers stay
   primary; COSMIC Wayland runtime-confirm in T10.
8. Headless settings (`language`, `notesFont`, `recentFiles`,
   `playlistVisible`): carried in config; surface only the COSMIC-approved
   ones (sidebar toggle, optional recent list), never drop stored data.

## 10. Parity checklist

Windows:
- [ ] W1 main window: size + `repeatMode` persist on close-request (destructor-save FIX, PLAN T8), title with file, single-instance open/present, DnD replace-playlist (T0 verdict: X11 `FileDropped` + Wayland `dnd_destination` w/ manual `text/uri-list` + sandboxed `file_transfer`; T10 COSMIC Wayland confirm), toast overlay

Dialogs:
- [ ] D1 open files: 6 filters, multi-select, `.lst`-vs-songs branching
- [ ] D2 preferences: all 17 rows (7 general + 5 lyrics + 5 piano), commit-on-close kept, solo% applies at close, color-picker T0 verdict + parser parity tests, font parse/fallback tests
- [ ] D3 MIDI setup: field order, refill/profile/test/apply, hint strings, deferred SoundFont commit
- [ ] D4 about: all strings, credit section, 3 links
- [ ] D5 help: full `kHelpText` copy
- [ ] D6 file info: all fields + copyright lines + empty state
- [ ] D7 jump: range 1..lastBar, pause-jump-resume
- [ ] D8 loop: from/to seeds, toggle pairing, cancel-resets-toggle, menu-forces-toggle-ON FIX
- [ ] D9/D10 playlist open/save: names, filters, no-autoplay-on-open-list
- [ ] D11 SoundFont chooser: sf2/sf3 filters, deferred commit
- [ ] D12 save lyrics: initial name + full-text write
- [ ] D13 print lyrics: print path via portal/export (gap tracked, not dropped)
- [ ] Toasts: all 6 call-site strings

Widgets:
- [ ] Sidebar: basename rows, selection sync, click-select vs double-click/Enter-play + test, 8 tools in order
- [ ] Menu: all 4 sections / 23 entries (incl. 2 external links)
- [ ] Header: 8 action buttons + loop toggle, order + icons + tooltips
- [ ] Dashboard: time format, tempo/volume live labels, pitch range, title ellipsize, lamps resize/accent, bar:beat
- [ ] Sliders: pos seek guard, tempo/volume ranges + reset buttons
- [ ] Switcher: 3 pages, order, icons (lyrics/channels/piano)
- [ ] Lyrics: 3 dropdowns + preselects, copy/save/print, tags + alignment + font + autoscroll, prefill+append duplication preserved + test
- [ ] Channels: 16 used-only rows, 8 fields each, solo/mute/volume formula, patch rebuild + reselect, meter decay
- [ ] Piano: tighten/show/hide, row layout, palette/velocity/names/footer rules, click audition within drawn range (tighten FIX)
- [ ] Status: 5 states + transitions

Actions/shortcuts:
- [ ] All 23 actions behave as §4 (20 + 3 repeat modes; incl. headless playlist/loop-toggle paths)
- [ ] All 9 accelerators kept (`Ctrl+O/Q`, `Space`, `Ctrl+P/S`, `Ctrl+Left/Right`, `F1`, `Ctrl+,`)

Settings/data:
- [ ] All 30 `AppSettings` keys round-trip (incl. 4 headless keys), GKeyFile INI schema unchanged
- [ ] Per-song `.cfg` schema + manual/auto triggers unchanged
- [ ] `.lst` playlist format (relative resolution, one path/line) unchanged

Flows/styling/integration:
- [ ] F1–F16 flows verified end to end
- [ ] `style.css` classes mapped (§7) in light + dark themes
- [ ] Desktop file, MIME, CLI flags/messages, `dmidiplayer` command compat, icon names, clipboard copy

## 11. Accessibility

Baseline (GTK4 today): accessibility comes free from stock widgets —
keyboard focus + activation for all buttons/toggles/sliders/dropdowns/
entries, 9 accelerators (§4), tooltips on every icon button (transport,
playlist tools, lyrics tools), ATK/AT-SPI names/roles from widget labels.
No custom `accessible-name` wiring exists; canvas-style regions (piano
drawing area, rhythm lamps, channel meters) expose nothing beyond their
containers.

Port requirements (libcosmic/iced):

- Keyboard-reachable: every control operable without a pointer — transport,
  playlist tools + list navigation, view switcher, sliders + resets, lyrics
  toolbar, channel rows, piano toolbar, all dialog fields + buttons. Keep
  all 9 accelerators (§4) and the double-click/Enter-play mapping (§3.1).
- Names: every icon-only button keeps an accessible name matching its GTK
  tooltip string (Open, Prev, Play, Pause, Stop, Next, Rew, Fwd, Loop; Add,
  Remove, Move up/down, Shuffle, Clear, Open/Save playlist; Copy, Save,
  Print; per-channel M/S/lock). Labeled controls use their visible labels.
- Canvas regions: piano `canvas`, rhythm lamps, and channel meters need
  text equivalents — piano keeps click-audition + toolbar keyboard paths;
  lamps/meters duplicate state already shown in `bar:beat` / time / volume
  labels, so no information is pointer-only.
- T0 verdict: tooltip at-risk CLEARED — verified via the
  `cosmic::widget::tooltip` wrapper; tooltips on all icon buttons remain a
  parity requirement, not optional polish.

## 12. T13b views — piano page + remaining dialogs (view slice)

View modules only — no state logic, no `app.rs` wiring; composition wires
later. Follows the T11/T12 patterns: VM structs in `ui/mod.rs`, owned VMs
into views (explicit destructures pin the contracts), whole-state updaters,
a11y names on every control that has a `.name()` API (checkboxes and all
button kinds do in the pinned rev; dropdowns/text inputs/canvas do not —
their visible labels are the accessible names per §11). VM builders are
pure functions of state with unit tests (empty states, OOR seeds,
verbatim-string asserts).

Shared-math rule: layout math, string tables, and formatters live
SINGLE-SOURCED in `state::{piano,midi,dialogs}` (T13a); the views own only
VMs, builders, and widgets. (The T13b-era D3 preselect exception is
reconciled — T13c fixed the shared fn to last-wins and the view delegates
again; see the D3 note.)

### 12.1 Dialog inventory D3–D8

| ID | VM (`ui/mod.rs`) | View module | Builder (pure) | Chrome |
|---|---|---|---|---|
| D3 MIDI Setup | `MidiSetupViewModel` | `ui/midi_setup.rs` | `view_model(&MidiState, test_status)` | `dialog()` + `Cancel` secondary, `chrome::wrap_modal` |
| D7 Jump | `JumpViewModel` | `ui/jump_loop.rs` | `jump_view_model(bar, bars)` | `dialog()` + `Cancel` secondary, `chrome::wrap_modal` |
| D8 Loop | `LoopViewModel` | `ui/jump_loop.rs` | `loop_view_model(from, to, bars)` | `dialog()` + `Cancel` secondary, `chrome::wrap_modal` |
| D6 File info | `FileInfoViewModel` + `FileInfoBody` | `ui/info_dialogs.rs` | `file_info_view_model(song, metadata)` | `dialog()`, no actions (`OK` in content) |
| D5 Help | — (static) | `ui/info_dialogs.rs` (`help_view`) | — (`HELP_TEXT` shared) | `dialog()`, no actions (`OK` in content) |
| D4 About | — (static) | `ui/info_dialogs.rs` (`about_view(on_url)`) | — (`ABOUT` table shared) | `dialog()`, no actions (`Close` in content) |

- D3: C++ field order verbatim (`window.cpp:961-1128`); labels/buttons/
  placeholder/hints aliased from `state::midi` (`MIDI_SETUP_FIELDS`, button
  labels, `SOUNDFONT_PLACEHOLDER`, `NO_DESTINATIONS_HINT`,
  `TEST_NOTE_STATUS`, `NO_MIDI_OUTPUT`); map/reset options reuse
  `prefs::{MAP_OPTIONS,RESET_OPTIONS}`. Refresh re-emits the CURRENT
  backend selection (C++ `refillFn` reads the dropdown on every trigger —
  retriggers enumeration with no new variant). Connection dropdown shows
  labels, messages ids. Status via shared `refill_hint` + transient test
  text. Divergence reconciled (T13c): the C++ loop overwrites `sel` per
  match (`window.cpp:1047-1051`), so LAST wins — the T13a first-match read
  lost, `select_refilled_port` is `rposition` now, and the view delegates
  to it (pinned by `refill_preselect_last_match_wins`).
- D7: label + spin `1..=max(1, bars)` + `Jump` → `JumpToBar(bar)`; spin
  steps ride `JumpBarEdited` (T13c scratch variant — per-edit `JumpToBar`
  would jump-on-step and is banned). No-song: the trigger does not open
  the dialog (C++ early-return, `window.cpp:777-778` — T13c gates via
  `jump_allowed`). Cancel = chrome `Cancel` → `CloseDialog` (window-X
  parity — C++ has no Cancel button).
- D8: from/to labels + spins + `Loop` → `LoopSet{from,to}`; seeds clamp
  via shared `loop_seeds` (no from≤to enforcement — C++ assigns blindly).
  Cancel resets the toggle (falls out of `loop_enabled`-derived toggle +
  `CloseDialog`). U3 (T13c re-verified + pinned): menu Loop emits the same
  `LoopToggled(true)` (no new variant) — the dialog opens and the toggle
  renders ON while open (`loop_toggle_forces_on_while_dialog_open_u3`).
- D6: File/Format/Duration/Initial tempo/Tracks/Division + metadata +
  per-copyright lines, values bold (Pango `<b>` parity); duration/tempo via
  shared `format_file_duration_ms` / `format_initial_tempo_us`; copyrights
  decoded in the builder with the song charset; metadata is
  `song.metadata` (T13c extended `SongSnapshot`/`LoadedSong` with
  `metadata` + `copyrights` from `info_map` + `get_text(Copyright)`).
  Empty state `No file loaded`.
- D5: full shared `HELP_TEXT` (verbatim `kHelpText`) in a `scrollable` +
  `OK`. Dialog title lives in `state::dialogs::HELP_TITLE`.
- D4: hand-built from the shared `ABOUT` table (the stock `widget::about`
  has no website/support/issues fields and would drop them): name, version
  (`env!("CARGO_PKG_VERSION")` = C++ `VERSION`), developer, copyright,
  license display name, comments, website/support/issues links, 2
  developers, `Original application` credit, 3 links, `Close`. Links ride
  `OpenUrl` (T13c: `OpenUrl`/`UrlOpened` + `xdg-open` task — OpenURI portal
  in-sandbox, no finish-args, same coverage as D13).
- Chrome: `ui/chrome.rs::wrap_modal(base, sheet)` generalizes the T12 FIXED
  pattern verbatim (D20): `Stack` + full-bounds capturing `mouse_area`
  (left press/release + touch captured, wheel → `ChooserCancelled`) + dim
  scrim. Recipe + per-dialog action table in the module docs; composition
  adopts it with no `app.rs` change. Confirm buttons live in the content
  views (C++ field order); chrome adds `Cancel` only where C++ closed via
  window-X unapplied (D3/D7/D8).
- T12 doc nits (done here): `message.rs` close-commit doc now says the
  narrowed 17-key `PrefsChanged(merged)` (not raw scratch); the `app.rs`
  chrome comment now names middle-press alongside right-press as
  still-propagating (verified in pinned `mouse_area.rs`: middle-press
  without `on_middle_press` is not captured).

### 12.2 Piano mapping (`ui/piano.rs`, ux §3.8)

- Toolbar: `Tighten keys` checkbox + `Show all` / `Hide unused` (verbatim
  labels/order) → `PianoTighten` / `PianoShowAll` / `PianoHideUnused`.
- Canvas `Program` (pinned API verified in framework source by own reads:
  `Program<Message, Theme, Renderer>`, `State: Default`,
  `update → Option<Action>` with `Action::publish/and_capture`,
  `draw → Vec<Geometry>` via `Frame::new` + `fill_rectangle` /
  `stroke_rectangle` / `fill_text`; core `Event`, `Cursor::position_in`;
  impl targets `cosmic::Theme/cosmic::Renderer` for the `Element`
  conversion). Draw loop ports `window.cpp:1969-2051`: `#241f31` bg,
  `max(48, h/rows)` rows (truncating int division, shared
  `row_height_px`), white/black passes, palette/velocity/names via shared
  `highlight_rgba` / `note_label` / `show_note_name`, `Ch N <label>`
  footers. Canvas text shifts up by its size (top-left vs cairo baseline).
- U12 FIX: clicks map within the DRAWN `(lo, hi)` via shared
  `click_to_preview` (white-key quantization preserved); no-song and
  out-of-keys clicks emit nothing (C++ sends `lo` OOR — deliberate,
  pinned). Press latches (release emits `PianoPreviewOff` only after a
  canvas press — the runtime offers every event to `update`). Empty state:
  the unlit ch0 keyboard draws (C++ parity — no placeholder).
- Inversion: a corrupt tightened range (`lo > hi`) survives like C++
  (draws no keys, clicks `None`).

### 12.3 Song-settings manual surfaces (`ui/song_settings.rs` — RETIRED T14d)

`Load song settings` / `Save song settings` buttons (verbatim C++ menu
order/labels, `window.cpp:1589-1590`) → `SongSettingsLoad` /
`SongSettingsSave` (arms exist). Always live (C++ parity — the arms no-op
on an empty song); `has_song` informational. Stubs for T14 menu placement —
placed as File entries 3–4 in T14d; the module is deleted (zero refs).

Parity-checklist note: the §10 boxes for D3–D8/piano stay unchecked —
T13c composed everything (shell state, arms, chrome wiring), but §10 waits
for the final verification pass (T11/T12 convention — their composed rows
are unchecked too).

## 13. T13c composition — T13a logic + T13b views wired into the app

Shells (all `None`/abandoned-on-switch like the T12 prefs shell; every new
arm is unit-pinned in `app.rs`):

- Piano page (`View::Piano` → `piano::view` + `piano::view_model`): clicks
  light the matrix AND send engine-carried `PreviewNote` (D19 refinement —
  bypasses the player mix, not the engine thread); release sends
  `PreviewOff` idle-only via `preview_off_targets` (A3, `window.cpp:2098-2116`).
- D3 (`midi_dlg: Option<MidiSetupDialog>` — whole-`MidiState` scratch +
  transient test status; engine-mirror fields ride but never commit back):
  refill/profile/test/apply arms per the T13a plan fns, port-enumeration
  task (app-side `MidiOutputManager`, precedented + sequencer-safe), D11
  chooser task, 400 ms `MidiTestNoteOff` sleep (blocking task half — iced
  ships no timer future and `Tick` only runs while Playing). Apply order
  `SetSysexReset → Stop{reset:false} → SetSoundFont(non-empty) →
  SelectOutput` (T13g F1: stop-before-font — C++ `:1172-1175` stages the
  font before stopping; behaviorally EQUIVALENT, T14d minor a — both
  stage the path, `sfload` runs at open); stop-first only when the plan sends
  (`midi_apply_no_change_sends_only_sysex_reset` — the only no-change send
  is the state-only sysex-reset, T13g F2; deliberate improvement over C++'s
  unconditional `:330` stop, for devil judgment). Test commits
  the SoundFont verbatim incl. empty (C++ `:1138`), reselects
  unconditionally (no T12 planner), sends ch0/C4/96 + verbatim statuses.
  T14d minors: Apply reads the DROPDOWN-shown port id (C++ `:1164-1166`,
  like Test — never the raw want); the test-note off carries the
  output-epoch capture (C++ `:1148-1153` captures `out`); piano preview
  sits inside the `current()` guard (C++ `:2098-2102`); stop-before-font
  is EQUIVALENT to C++ `:1172-1175` (both stage the font, `sfload` at
  open — the old "safer" gloss is reworded).
- D7/D8 (`jump_bar`/`loop_from`/`loop_to` scratch, seeded on open — D7 to
  1, the C++ spin's unset min; D8 from `loop_start`/`loop_end` via
  `loop_seeds`): scratch variants `JumpBarEdited`/`LoopFromEdited`/
  `LoopToEdited`, confirms clamp via `clamp_jump_bar`/`loop_confirm` (no
  from≤to enforcement), D7 gated by `jump_allowed`, D8 unconditional (kept
  deviation). U3: menu Loop emits `LoopToggled(true)`, toggle forced ON
  while open via the transport VM.
- D6/D5/D4: sheets over `dialog_sheet()` + `chrome::wrap_modal` (T13c
  verified it generalizes the T12 chrome verbatim; D2 keeps its inline
  copy); `Cancel` only for D3/D7/D8. D4 links ride `OpenUrl`/`UrlOpened` +
  `xdg-open` (OpenURI portal in-sandbox — verified in
  `/usr/bin/xdg-open:872-899` `open_flatpak`, no finish-args gap).
- Song-settings arms gated by `manual_song_action_allowed`, ungated by the
  auto flag (`window.cpp:1405-1408`); the T13b stubs are placed as File
  entries 3–4 and deleted in T14d.
- Test-harness fallout: the live probe (`Harness::persist_live_settings`)
  is D2 open + unchanged close in T14d (was: open D3 + unchanged Apply —
  retired, since Apply reads the dropdown and the helper cannot feed the
  refill); mid-dialog prefs probes reopen prefs + resend scratch (the
  probe reseeds, abandoning their edits).

## 14. T14b views — menu + theming + a11y (view slice)

View modules only — no state logic, no `app.rs` wiring; composition wires
later. Follows the T11–T13 patterns: VMs in `ui/mod.rs`, owned VMs into
views (explicit destructures pin the contracts), pure VM builders, a11y
names on every control that has a `.name()` API. VM builders pure +
unit-tested (inventory, verbatim strings, keybinds, message map, clamps,
both-theme style resolution).

Parity-checklist note: the §10 boxes stay unchecked — T14b delivers the
view slice (menu inventory + theme mapping + a11y record); §10 waits for
the final verification pass with composition (T11–T13 convention).

### 14.1 Menu inventory — 23 entries, 4 sections (U1b)

`ui/menu.rs` + `MenuViewModel` (`ui/mod.rs`): a single-root `menu::bar`
(hamburger icon root — `open-menu-symbolic`, `window.cpp:1584`) whose
dropdown holds all 4 C++ sections in C++ order, divided where C++
appends sections (`window.cpp:1613-1616`). All 23 entries map to existing
`Message` variants (arms exist arch-side — T14d added `Message::Quit` +
the `window::close` arm, retired the T14b `vm.quit` callback, and placed
the bar at `header_end`, C++ `window.cpp:1618`). Exactly the 9 C++
accelerators show as hints, in libcosmic `KeyBind` `Display` form
(`key_bind.rs:143-155`).

| # | section | label (verbatim) | menu cite | C++ action → handler | message | hint |
|---|---|---|---|---|---|---|
| 1 | File | `Open…` | `window.cpp:1587` | `app.open` → D1 (`window.cpp:1373-1374`) | `OpenFilesDialog` | `Ctrl + O` |
| 2 | File | `File information` | `window.cpp:1588` | `app.info` → D6 (`window.cpp:1403-1404`) | `ShowFileInfo` | — |
| 3 | File | `Load song settings` | `window.cpp:1589` | `app.load-settings` (`window.cpp:1407-1408`) | `SongSettingsLoad` | — |
| 4 | File | `Save song settings` | `window.cpp:1590` | `app.save-settings` (`window.cpp:1405-1406`) | `SongSettingsSave` | — |
| 5 | File | `Search MIDI files` | `window.cpp:1591` | `app.search` → `https://midisite.co.uk` (`window.cpp:1409-1410`) | `OpenUrl(SEARCH_URL)` | — |
| 6 | File | `Quit` | `window.cpp:1592` | `app.quit` → `g_application_quit` (`window.cpp:1375-1376`) | `Quit` | `Ctrl + Q` |
| 7 | Playback | `Play` | `window.cpp:1594` | `app.play` (`window.cpp:1377-1378`) | `Play` | `Space` |
| 8 | Playback | `Pause` | `window.cpp:1595` | `app.pause` (`window.cpp:1379-1380`) | `Pause` | `Ctrl + P` |
| 9 | Playback | `Stop` | `window.cpp:1596` | `app.stop` (`window.cpp:1381-1382`) | `Stop` | `Ctrl + S` |
| 10 | Playback | `Previous` | `window.cpp:1597` | `app.prev` (`window.cpp:1385-1386`) | `PrevSong` | `Ctrl + ArrowLeft` |
| 11 | Playback | `Next` | `window.cpp:1598` | `app.next` (`window.cpp:1383-1384`) | `NextSong` | `Ctrl + ArrowRight` |
| 12 | Playback | `Rewind 1 bar` | `window.cpp:1599` | `app.rew` (`window.cpp:1389-1390`) | `BeatBackward` | — |
| 13 | Playback | `Forward 1 bar` | `window.cpp:1600` | `app.fwd` (`window.cpp:1387-1388`) | `BeatForward` | — |
| 14 | Playback | `Jump to bar…` | `window.cpp:1601` | `app.jump` → D7 (`window.cpp:1391-1392`) | `ShowDialog(JumpToBar)` | — |
| 15 | Playback | `Loop…` | `window.cpp:1602` | `app.loop` → D8 (`window.cpp:1393-1394`) | `LoopToggled(true)` (U3) | — |
| 16 | Repeat | `Repeat off` | `window.cpp:1604` | `app.repeat-off` (`window.cpp:2140-2152`) | `RepeatModeChanged(Off)` | — |
| 17 | Repeat | `Repeat song` | `window.cpp:1605` | `app.repeat-song` | `RepeatModeChanged(Song)` | — |
| 18 | Repeat | `Repeat playlist` | `window.cpp:1606` | `app.repeat-list` | `RepeatModeChanged(Playlist)` | — |
| 19 | Help | `MIDI Setup` | `window.cpp:1608` | `app.midi` → D3 (`window.cpp:1397-1398`) | `ShowDialog(MidiSetup)` | — |
| 20 | Help | `Preferences` | `window.cpp:1609` | `app.prefs` → D2 (`window.cpp:1395-1396`) | `ShowDialog(Preferences)` | `Ctrl + ,` |
| 21 | Help | `Help` | `window.cpp:1610` | `app.help` → D5 (`window.cpp:1401-1402`) | `ShowHelp` | `F1` |
| 22 | Help | `Original dmidiplayer` | `window.cpp:1611` | `app.website` → `https://sourceforge.net/p/dmidiplayer/` (`window.cpp:1411-1413`) | `OpenUrl(WEBSITE_URL)` | — |
| 23 | Help | `About Gosh MIDI Player` | `window.cpp:1612` | `app.about` → D4 (`window.cpp:1399-1400`) | `ShowAbout` | — |

Notes:

- The T13 `song_settings` stubs landed HERE (entries 3–4: same labels,
  same messages, same always-live rule — the arms no-op on an empty song,
  `window.cpp:633-635,663-665`); the stub module is deleted in T14d
  (zero refs — verified by grep).
- Loop takes the U3 path (`LoopToggled(true)` — forces the toggle ON,
  then opens D8; no new variant). Jump emits `ShowDialog(JumpToBar)` —
  the arm gates no-song via `jump_allowed` (`window.cpp:777-778`
  parity). Repeat items are plain like C++ (no radio state) — the VM
  carries no repeat mode.
- Entries are hand-rolled over the public menu primitives (`menu_button`,
  `MenuTree`, `divider::horizontal::light`) instead of `menu::items`:
  that helper needs an `A: MenuAction` whose `message(&self)` cannot see
  the VM (the T14b constraint — Quit rode `vm.quit` then; T14d retired
  the callback for `Message::Quit` but keeps the layout). Layout follows
  `entry_tree` (`menu_tree.rs:284-350`); keybind ink reuses
  `theme::dim_label`.
- External links ride `OpenUrl` (the T13c `xdg-open` task — OpenURI
  portal in-sandbox, no finish-args gap).

### 14.2 Theme mapping — `style.css` → COSMIC, light + dark

Full `data/style.css` mapping (minus dead `.channel-meter`, U16 —
`style.css:49-51` is referenced by NO C++ widget: `grep channel-meter`
matches only the CSS file itself, so no theme item exists for it):

| GTK class | rules | COSMIC mapping | light | dark |
|---|---|---|---|---|
| `.player-dashboard` | `#111111` bg, `#f6f5f4` text, 16px radius | `theme::dashboard_card` (explicit fill + radius token) | explicit dark card | same |
| `.time-display` | 40px bold mono `#f6f5f4` | `theme::time_display` + widget size/weight | explicit ink | same |
| `.dash-label` | `#c0bfbc` 12px | `theme::dash_label` + widget size | explicit ink | same |
| `.dash-value` | `#62a0ea` semibold | `theme::dash_value` (theme accent) | ⚠ dim — see below | accent ink |
| `.rhythm-lamp` / `.on` / `.accent` | 14px circle; `#3d3846` / `#57e389` / `#f66151` | `theme::rhythm_lamp` (explicit fills) | same | same |
| `.piano-area` | `#241f31` bg | `theme::piano_area` (T14b — was a piano-local const) | explicit dark canvas | same |
| `.lyrics-view` | 16pt | `DEFAULT_LYRICS_SIZE_PT = 16.0` + parsed `lyricsFont` (D15 FIX) | same | same |
| `flat` | icon buttons | stock `button::icon` (no item) | theme | theme |
| `toolbar` | button rows | `row` spacing (no item) | — | — |
| `navigation-sidebar` | selected row | `theme::playlist_row` (accent wash) | tracks accent | tracks accent |
| `dim-label` | status line | `theme::dim_label` (fg @ 55%) | tracks fg | tracks fg |
| `suggested-action` | D3 Apply | stock `button::suggested` (no item) | theme | theme |

Headless verification (T14b — `ui::theme` tests, pinned rev `7e82198`
default theme): every style resolves under BOTH `Theme::dark()` and
`Theme::light()` — explicit-hex pins, accent/dim/wash mapping pins, and
WCAG contrast pins (card text ≈ 15:1, dash label ≈ 10:1 — AAA in both
modes, theme-independent).

Measured shortfall (recorded, not fixed — the accent mapping is spec'd,
ux §7): `.dash-value` accent-on-card is 10.45:1 in dark mode but
**2.12:1 in light mode** (light accent is a dark teal tuned for light
surfaces). Stays a theme-review item: lighten the card accent ink in
light mode (e.g. accent hover/focus or a white mix). The manual recipe
below covers the visual confirm.

Phase 3 manual recipe (theme): run the app under a light COSMIC theme
and a dark one; check the dashboard card (dark card, bright time,
readable tempo/volume — NOTE the dim light-mode values), rhythm lamps
(rest/lit/accent), piano canvas (dark `#241f31`), selected playlist row
(accent wash, text legible), status line (dim but legible), D3 Apply
(accent suggested button); screenshot both modes for the record.

### 14.3 A11y acceptance record (U7)

Audit method: every view module read end to end (T9–T13 + T14b menu);
every `.name()`/focus/keyboard claim re-verified in the pinned
libcosmic/iced sources (rev `7e82198`, paths below relative to that
checkout). The `a11y` cargo feature is default-on (`Cargo.toml:11-22`),
so all naming APIs below are live.

Names — all controls that HAVE a naming API carry one (gaps found →
fixed in T14b; the rest were already complete):

| view | named (T14b-verified) | fix |
|---|---|---|
| transport | 9 icon buttons (tooltip+name+desc); pitch spin (name arg); 3 sliders (name+desc; tempo/volume tooltips verbatim C++) | none — complete |
| sidebar | 8 tools (tooltip+name+desc, verbatim C++ tips); row Play buttons | FIXED: row Play names were N bare `Play`s — now `Play <basename>` (channels precedent) |
| lyrics page | 3 icon buttons (verbatim C++ tips + names) | none — complete |
| lyrics prefs | 3 picker swatches (tooltip+name+desc) | none — complete |
| channels | M/S/lock per-channel names; lock tooltip; volume sliders per-channel names | none — complete |
| prefs | 2 spins (name args) | none — complete |
| piano | checkbox + 2 buttons (names+descs) | none — complete |
| midi_setup | checkbox + 5 buttons (names+descs) | none — complete |
| jump_loop | 3 spins (name args) + 2 confirms (names+descs) | none — complete |
| info_dialogs | OK/Close/links (names+descs) | none — complete |
| song_settings | 2 buttons (names+descs) | RETIRED in T14d (File entries 3–4 are the placement) |
| menu (T14b) | icon root (`Menu` + desc — load-bearing, no visible label); 23 entries (label names + `Shortcut …` descs) | new — complete |

Tracked gaps (pinned-API limits — no `.name()` exists, or the widget is
not focusable; D13-style with evidence, not silent):

- G1 dropdown/text-field/toggler names: cosmic `Dropdown`
  (`src/widget/dropdown/widget.rs` — no `name`/`description`), cosmic
  `TextInput` (`.label()` renders a VISIBLE floating label,
  `src/widget/text_input/input.rs:1349-1365` — would duplicate the row
  title), cosmic `Toggler` (`.label()` renders visible text,
  `src/widget/toggler.rs:189` — same). Visible adjacent labels remain
  the accessible names (T11b ruling, unchanged).
- G2 keyboard reachability: `Focusable` exists only for iced
  button/text_input/text_editor (`iced/widget/src/{button,text_input,text_editor}.rs`)
  + cosmic Button (`src/widget/button/widget.rs:359`, incl. all builders
  and spin steppers, which ARE icon buttons — `src/widget/spin_button.rs:177-194`)
  + cosmic TextInput (`src/widget/text_input/input.rs:853`). NOT
  focusable: menu bar (no keyboard handling in
  `src/widget/menu/menu_inner.rs` / `menu_bar.rs`), dropdown, toggler,
  checkbox (named but unfocusable — `iced/widget/src/checkbox.rs`
  handles no keyboard), slider (ArrowUp/Down only while hovered —
  `iced/widget/src/slider.rs:493-508`, pointer-adjacent, not
  keyboard-only), canvas (ux §11 text-equivalent ruling stands).
- G3 per-action reachability after arch T14a + T14d wiring (9
  accelerators + Tab traversal live): 12/23 menu actions
  keyboard-reachable — the 9 accelerators (Open, Quit, Play, Pause,
  Stop, Prev, Next, Help, Prefs) + Tab-Enter on the focusable-button
  twins for Rew, Fwd, and the Loop header toggle (Loop is a
  `transport_button`, focusable — `ui/transport.rs:202`; the other
  twins — playlist tools, row Play, lyrics tools, M/S/lock, piano
  toolbar, dialog confirms/links, spin steppers — reach non-menu-action
  controls, outside this count).
  Menu-only remainder (11: File info, Load/Save settings, Search, Jump,
  Repeat ×3, MIDI Setup, Website, About) stays menu-only — lead ruling
  D22 rejects the §4-proposed `Ctrl+J` / `Shift+Left/Right` / `Ctrl+M` /
  `Ctrl+I` / `Ctrl+L` candidates (parity outranks a11y per the priority
  order; exactly the 9 C++ accelerators, no invented bindings — tracked
  here like D13, not dropped). Value widgets (sliders, dropdowns,
  toggles, checkboxes) are pointer-only in the pinned rev (G2) — no
  arch subscription can fix that; upstream or custom widgets only.
- G4 modal focus trap (PARTIAL in T14d): `chrome::wrap_modal` captures
  pointer events only, and Tab still cycles focus into base widgets
  under an open dialog (no subtree-scoping op exists in the pinned
  rev — honestly kept). What T14d closed: the subscription half is
  gated (Esc/Tab-only under dialogs, unit-pinned at both layers), and
  every `open_dialog` path focuses a dialog widget (D2 Close, D3 Apply,
  D7 Jump, D8 Loop, D6/D5 OK, D4 Close — the `focus(id)` op unfocuses
  everything else, so base Enter cannot fire silently under the
  dialog). Residual: a user who Tabs out and Enters a base widget
  activates it — state-safe via the narrow close commit, but not
  trapped. Full trap needs upstream focus scoping.
  Esc two-step (standard UX, honestly noted): cosmic `text_input`
  captures a focused-field Esc to unfocus (no early return — falls to
  the blanket `capture_event()`, pinned
  `src/widget/text_input/input.rs:2235-2243` + `:2270`), so with a
  text field focused — D2 color/font fields (e.g. `ui/prefs.rs:382`)
  or the D3 soundfont field (`ui/midi_setup.rs:299`) — the first Esc
  only unfocuses and the second reaches `listen()` to dismiss.

Resolved (verified, not tracked):

- Tooltip story RESOLVED: `cosmic::widget::tooltip` is the iced
  `Tooltip` wrapper (`src/widget/mod.rs:322-342`, `Position` re-exported
  from `iced/widget/src/tooltip.rs:407`); icon buttons additionally
  offer `.tooltip()` (used by transport/sidebar/lyrics/channels). All
  C++ tooltip strings are carried verbatim (transport tempo/volume
  en-dash strings, 8 sidebar tips, 3 lyrics tips, loop `Loop`).
- Focus visible RESOLVED: focused buttons render a 1px accent outline +
  2px transparent border (`src/theme/style/button.rs:189-194`), in both
  modes (accent-derived). Covers all button kinds incl. menu entries.
- Canvas regions: piano toolbar is the keyboard path (audition itself
  is click-only in C++ too — parity, not a gap); lamps/meters duplicate
  `bar:beat`/time/slider state (ux §11, unchanged).

Phase 3 manual recipe (a11y): Tab through every page + dialog (order
sane, focus ring visible on each stop, Enter activates); run with a
screen reader (names announced per the table above — esp. the qualified
row Play names and the `Menu` root); hover every icon button (tooltip
shows); press each of the 9 accelerators from every page (no double
Space activation on focused buttons).

### 14.4 T13 minor — D4 `app_icon` rendered

Resolution: RENDER (view-owned — removing the field would touch
arch-owned `state/dialogs.rs`). `about_view` now shows a 64px
`icon::from_name(ABOUT.app_icon)` beside the title/version header.
Evidence: the name resolves — `icons/CMakeLists.txt` installs
`${GOSH_APP_ID}.png` (24–128px) + `.svgz` into hicolor, and the desktop
file sets `Icon=com.goshapps.GoshMIDI`; headless `about_view` build
covered by the existing `views_build_across_contract_shapes` test.
Manual: open About (menu → `About Gosh MIDI Player`), confirm the app
icon renders (Phase 3 recipe — headless builds cannot see pixels).

### 14.5 F1–F16 flow view-coverage (states incl. empty/error)

State exists arch-side (T9–T13c); T14b verifies the VIEWS cover every
flow state (loaded/empty/error):

| flow | states | view coverage |
|---|---|---|
| F1 first launch | no playlist, no song | sidebar `Playlist is empty`; transport `No file loaded`, live controls; lyrics/channels placeholders; piano ch0 keyboard; toasts arch-rendered |
| F2 open & play | chooser → loaded song | D1 portal arch-side; all pages render the loaded song (title/time/lamps/channels/lyrics/piano) |
| F3 playlist | rows/empty/select/OOR | basename rows, selection wash, 8 tools (Remove/Up/Down insensitive w/o selection), empty placeholder, OOR-selected renders unhighlighted |
| F4 transport | 5 statuses, no-song | 9 live buttons (C++ never desensitizes); status line renders all 5 `Status::text()`s |
| F5 seek/shape | slider/spin ranges | position 0–1000 + tempo 50–200 + volume 0–200 + pitch −12–+12 + resets; clamps at both ends |
| F6 jump | D7 range/seeds | spin `1..=max(1,bars)`; OOR seeds clamp; no-song never opens (arm gates) |
| F7 loop | D8 seeds/toggle/menu | from/to spins + `Loop`; cancel falls out via `loop_enabled`; menu Loop = `LoopToggled(true)` (U3) |
| F8 repeat | 3 modes | 3 plain menu entries → `RepeatModeChanged`; end-of-song tree arch-side |
| F9 MIDI setup | ports/empty/test/apply | full D3 field order; empty ports → verbatim hint; transient test status; map/reset clamps |
| F10 lyrics | filters/live/empty/export | 3 dropdowns + preselects; `sung` colors; live tail; no-song + no-match placeholders; Copy/Save/Print buttons |
| F11 mix | rows/empty/corrupt | used-only rows, 8 fields each; no-song + no-rows placeholders; patch/level/meter clamps |
| F12 piano | tighten/rows/clicks | toolbar; ch0 empty keyboard; tightened range (U12 clicks); corrupt inversion draws no keys, clicks `None` |
| F13 rhythm | meter/accent | lamps resize to `beats` (256 cap); downbeat accent; `bar:beat` text equivalent |
| F14 song settings | manual/auto | menu entries 3–4 (stub placement); auto triggers arch-side |
| F15 inspect/learn | D6/D5/D4 + links | file-info rows + `No file loaded`; help scroll + OK; about icon + links + Close; menu entries 2/21/22/23/5 |
| F16 quit | persist + close | menu Quit → `vm.quit` (arch wires `window::close` + `close-request` save) |
| errors | load failures, chooser cancel, DnD rejects, print/export failures | all surface as toasts (arch-rendered); cancels silent; corrupt values clamp at every view (sliders/spins/dropdowns/meters) — no view can panic on state |

No view-coverage gaps found: every flow state has a rendering path, and
every corrupt/empty shape has an explicit empty state or clamp (all
covered by `view_builds_*` / clamp unit tests).
