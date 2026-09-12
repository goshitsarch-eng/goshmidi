//! Global settings: GKeyFile-compatible reader/writer (arch §4.2, A11).
//!
//! A hand-rolled reader/writer for the GLib `GKeyFile` format — deliberately
//! **not** a TOML/JSON migration, so existing `dmidiplayer.conf` files keep
//! working. Every rule below was verified empirically against the system
//! GLib (probe program, 2026-09-12) rather than guessed from the docs:
//!
//! - Strings: `\s \t \n \r \\` unescape on read; any other escape (including
//!   `\;`, `\x`, `\uXXXX`, a trailing lone `\`) is a *per-key* error, not a
//!   file error. On write, `\` → `\\`, newline → `\n`, CR → `\r`, and a
//!   leading run of spaces/tabs → `\s`/`\t`; tabs elsewhere, `;`, `#`, `=`
//!   and non-ASCII bytes are written raw.
//! - Lists (`recentFiles`): elements are `escape(scalar)` plus `;` → `\;`,
//!   joined with `;` and a trailing `;`. On read, split on unescaped `;` and
//!   drop one trailing empty piece; an empty value is an empty list.
//! - Booleans: exactly `true`/`false`/`1`/`0` (case-sensitive, surrounding
//!   whitespace tolerated). `True`/`yes` are per-key errors.
//! - Integers: trimmed ASCII decimal (`[+-]?[0-9]+`), `i32` range. `0x10`,
//!   `42x`, `3.5`, overflow are per-key errors.
//! - Structure: `#` starts a comment only at a (whitespace-stripped) line
//!   start; `;` does **not** start a comment. Group headers are exact
//!   (`[General]`; spaces become part of the name, trailing text fails).
//!   Keys split at the first `=`; keys may contain spaces; last duplicate
//!   wins; duplicate groups merge. CRLF and a missing final newline are
//!   tolerated; a BOM, `[]`, or any line that is neither blank, comment,
//!   group nor key=value fails the whole load.
//! - Corrupt → defaults: a structural failure (or a missing/unreadable
//!   file) leaves the in-memory settings untouched (`settings.cpp:79-84`); a
//!   per-key failure leaves that field's current value (`recentFiles` resets
//!   to empty, mirroring the C++ clear+fill).
//! - Invalid-UTF-8 deviation (probed 2026-09-12): GLib *loads* such files —
//!   `g_key_file_load_from_file` succeeds, the good keys stay readable, and
//!   only the undecodable key's getter fails — so C++ applies the good keys
//!   and defaults just the bad one. The Rust reader instead rejects the whole
//!   file (non-UTF-8 is a [`ParseError`]) and keeps in-memory state (defaults
//!   at startup). Salvaging would need lossy parsing; the gap is documented
//!   here, not emulated.
//!
//! Deviation from C++ (mandated by arch A11): `AppSettings::save()` rewrites
//! only the known keys, silently dropping unknown ones. The Rust writer
//! preserves unknown keys (in `[General]`, verbatim, after the known keys)
//! and unknown groups (verbatim, after `[General]`).

use std::fmt;
use std::path::Path;

use super::paths::Paths;

// ---------------------------------------------------------------------------
// KeyFile
// ---------------------------------------------------------------------------

/// Structural parse failure: the whole file is corrupt and the caller's
/// in-memory state must be kept (`settings.cpp:79-84`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number of the offending line (0 when not line-specific).
    pub line: usize,
    /// Human-readable cause.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    name: String,
    /// Raw (still escaped) values in first-seen key order; duplicates collapse
    /// to the last value.
    keys: Vec<(String, String)>,
}

/// Minimal insertion-ordered `GKeyFile` model shared by [`Settings`] and
/// `config::song` (which reuses it via `super::settings`).
///
/// Values are stored raw; typed getters unescape/parse on demand and
/// distinguish *missing* (`None`) from *present but invalid* (`Some(Err(()))`),
/// which is exactly the split the C++ per-key fallbacks need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeyFile {
    groups: Vec<Group>,
}

impl KeyFile {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Parse full file text. `Err` = structural failure (corrupt file).
    pub(crate) fn parse(text: &str) -> Result<Self, ParseError> {
        let mut kf = Self::new();
        let mut current: Option<usize> = None;
        for (idx, raw_line) in text.lines().enumerate() {
            let line_no = idx + 1;
            // `str::lines` already stripped a trailing `\r` (CRLF tolerated).
            let line = raw_line.trim_start_matches([' ', '\t']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if !line.ends_with(']') || line.len() < 3 {
                    return Err(ParseError {
                        line: line_no,
                        message: format!("not a key-value pair, group, or comment: {raw_line}"),
                    });
                }
                // Group names are verbatim (probe: "[General ]" is a *different*
                // group, not an error).
                let name = line[1..line.len() - 1].to_string();
                if name.is_empty() {
                    return Err(ParseError {
                        line: line_no,
                        message: "empty group name".to_string(),
                    });
                }
                current = Some(kf.group_or_new(&name));
                continue;
            }
            let Some(eq) = line.find('=') else {
                return Err(ParseError {
                    line: line_no,
                    message: format!("not a key-value pair, group, or comment: {raw_line}"),
                });
            };
            let Some(group) = current else {
                return Err(ParseError {
                    line: line_no,
                    message: format!("key-value pair outside any group: {raw_line}"),
                });
            };
            let key = line[..eq].trim_end_matches([' ', '\t']);
            if key.is_empty() {
                return Err(ParseError {
                    line: line_no,
                    message: format!("empty key name: {raw_line}"),
                });
            }
            // Leading whitespace after `=` is stripped; trailing is kept
            // (probe: `spaced = padded ` reads back as `padded `).
            let value = line[eq + 1..].trim_start_matches([' ', '\t']);
            kf.insert_raw(group, key, value);
        }
        Ok(kf)
    }

    pub(crate) fn group_or_new(&mut self, name: &str) -> usize {
        if let Some(i) = self.groups.iter().position(|g| g.name == name) {
            return i;
        }
        self.groups.push(Group {
            name: name.to_string(),
            keys: Vec::new(),
        });
        self.groups.len() - 1
    }

    fn insert_raw(&mut self, group: usize, key: &str, raw_value: &str) {
        let keys = &mut self.groups[group].keys;
        if let Some(slot) = keys.iter_mut().find(|(k, _)| k == key) {
            slot.1 = raw_value.to_string(); // last duplicate wins
        } else {
            keys.push((key.to_string(), raw_value.to_string()));
        }
    }

    fn find_group(&self, group: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == group)
    }

    /// Raw value, if the key is present.
    pub(crate) fn get_raw(&self, group: &str, key: &str) -> Option<&str> {
        self.find_group(group)?
            .keys
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `None` = missing; `Some(Err)` = present but invalid escape.
    pub(crate) fn get_string(&self, group: &str, key: &str) -> Option<Result<String, ()>> {
        self.get_raw(group, key).map(unescape_scalar)
    }

    /// `None` = missing; `Some(Err)` = present but not
    /// `true`/`false`/`1`/`0`.
    pub(crate) fn get_bool(&self, group: &str, key: &str) -> Option<Result<bool, ()>> {
        self.get_raw(group, key)
            .map(|raw| match raw.trim_matches([' ', '\t']) {
                "true" | "1" => Ok(true),
                "false" | "0" => Ok(false),
                _ => Err(()),
            })
    }

    /// `None` = missing; `Some(Err)` = present but not trimmed `i32` decimal.
    pub(crate) fn get_int(&self, group: &str, key: &str) -> Option<Result<i32, ()>> {
        self.get_raw(group, key)
            .map(|raw| raw.trim_matches([' ', '\t']).parse::<i32>().map_err(|_| ()))
    }

    /// `None` = missing; `Some(Err)` = present but invalid escapes.
    /// A present-but-empty value is an empty list (probe: `l=` → n=0).
    pub(crate) fn get_string_list(
        &self,
        group: &str,
        key: &str,
    ) -> Option<Result<Vec<String>, ()>> {
        self.get_raw(group, key).map(unescape_list)
    }

    /// Clone with `known` keys of `group` removed: the unknown-key/group tail
    /// preserved verbatim across load→save (A11). Empty unknown groups are
    /// preserved; a fully-known `group` is dropped (the writer re-emits it).
    pub(crate) fn without_keys(&self, group: &str, known: &[&str]) -> KeyFile {
        let mut out = KeyFile::new();
        for g in &self.groups {
            let keep_all = g.name != group;
            let mut kept_any = false;
            for (k, v) in &g.keys {
                if !keep_all && known.contains(&k.as_str()) {
                    continue;
                }
                let gi = out.group_or_new(&g.name);
                out.insert_raw(gi, k, v);
                kept_any = true;
            }
            if keep_all && !kept_any {
                out.group_or_new(&g.name);
            }
        }
        out
    }

    /// Whether the group header is present (even with no keys).
    pub(crate) fn has_group(&self, group: &str) -> bool {
        self.find_group(group).is_some()
    }

    /// Ordered snapshot of groups and their raw keys (unknown-tail emission).
    pub(crate) fn snapshot(&self) -> Vec<(String, Vec<(String, String)>)> {
        self.groups
            .iter()
            .map(|g| (g.name.clone(), g.keys.clone()))
            .collect()
    }

    /// Set a verbatim raw value (unknown-key preservation path).
    pub(crate) fn set_raw(&mut self, group: &str, key: &str, raw_value: &str) {
        let gi = self.group_or_new(group);
        self.insert_raw(gi, key, raw_value);
    }

    pub(crate) fn set_string(&mut self, group: &str, key: &str, value: &str) {
        self.set_raw(group, key, &escape_scalar(value));
    }

    pub(crate) fn set_bool(&mut self, group: &str, key: &str, value: bool) {
        self.set_raw(group, key, if value { "true" } else { "false" });
    }

    pub(crate) fn set_int(&mut self, group: &str, key: &str, value: i32) {
        self.set_raw(group, key, &value.to_string());
    }

    pub(crate) fn set_string_list(&mut self, group: &str, key: &str, values: &[String]) {
        if values.is_empty() {
            self.set_raw(group, key, "");
            return;
        }
        let mut raw = String::new();
        for v in values {
            raw.push_str(&escape_list_element(v));
            raw.push(';');
        }
        self.set_raw(group, key, &raw);
    }

    /// Serialize: groups in insertion order, keys in insertion order, one
    /// blank line between groups, single trailing `\n` (probe-verified
    /// `g_key_file_to_data` shape).
    pub(crate) fn to_data(&self) -> String {
        let mut out = String::new();
        for (gi, group) in self.groups.iter().enumerate() {
            if gi > 0 {
                out.push('\n');
            }
            out.push('[');
            out.push_str(&group.name);
            out.push_str("]\n");
            for (k, v) in &group.keys {
                out.push_str(k);
                out.push('=');
                out.push_str(v);
                out.push('\n');
            }
        }
        out
    }
}

/// Escape one scalar string value for writing (probe-verified rules).
pub(crate) fn escape_scalar(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    // Leading run of spaces/tabs must be escaped (read strips raw leading
    // whitespace after `=`).
    while let Some(&c) = chars.peek() {
        match c {
            ' ' => {
                out.push_str("\\s");
                chars.next();
            }
            '\t' => {
                out.push_str("\\t");
                chars.next();
            }
            _ => break,
        }
    }
    for c in chars {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape one list element: scalar rules plus `;` → `\;`.
pub(crate) fn escape_list_element(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, c) in value.char_indices() {
        let leading = value[..i].chars().all(|p| p == ' ' || p == '\t');
        match c {
            ' ' if leading => out.push_str("\\s"),
            '\t' if leading => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ';' => out.push_str("\\;"),
            _ => out.push(c),
        }
    }
    out
}

/// Unescape one scalar value. Only `\s \t \n \r \\` are valid (probe: `\;`,
/// `\x`, `\u…`, trailing `\` are per-key errors).
pub(crate) fn unescape_scalar(raw: &str) -> Result<String, ()> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            _ => return Err(()),
        }
    }
    Ok(out)
}

/// Unescape one list value: split on unescaped `;` (extra valid escape
/// `\;`), drop one trailing empty piece, empty value → empty list.
pub(crate) fn unescape_list(raw: &str) -> Result<Vec<String>, ()> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == ';' {
            items.push(std::mem::take(&mut cur));
            continue;
        }
        if c != '\\' {
            cur.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => cur.push(' '),
            Some('t') => cur.push('\t'),
            Some('n') => cur.push('\n'),
            Some('r') => cur.push('\r'),
            Some('\\') => cur.push('\\'),
            Some(';') => cur.push(';'),
            _ => return Err(()),
        }
    }
    items.push(cur);
    // A well-formed list ends with `;`, leaving one trailing empty piece.
    if items.last().is_some_and(String::is_empty) {
        items.pop();
    }
    Ok(items)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

const GROUP: &str = "General";

/// All 30 `[General]` keys in C++ `save()` emission order.
const KNOWN_KEYS: [&str; 30] = [
    "lastDirectory",
    "lastOutputBackend",
    "lastOutputConnection",
    "lastPlayList",
    "soundFont",
    "language",
    "drumsChannel",
    "soloVolumeReduction",
    "autoPlay",
    "autoAdvance",
    "autoSongSettings",
    "advancedPorts",
    "sysexReset",
    "instrumentMap",
    "highlightPalette",
    "velocityColor",
    "octaveSubscript",
    "namesVisibility",
    "lyricsFont",
    "notesFont",
    "futureColor",
    "pastColor",
    "singleColor",
    "highlightColor",
    "textAlignment",
    "windowWidth",
    "windowHeight",
    "playlistVisible",
    "repeatMode",
    "recentFiles",
];

/// Global application settings: all 30 `AppSettings` keys with the C++
/// defaults (`settings.hpp`).
///
/// `lastPlayList` is stored verbatim (passthrough: whatever string the app
/// set — absolute, relative, `~`-led — round-trips byte-identically and is
/// handed to the playlist loader as-is at startup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub last_directory: String,
    pub last_output_backend: String,
    pub last_output_connection: String,
    pub last_play_list: String,
    pub sound_font: String,
    pub language: String,
    /// 1-based drums channel (default 10, like the original).
    pub drums_channel: i32,
    pub solo_volume_reduction: i32,
    pub auto_play: bool,
    pub auto_advance: bool,
    pub auto_song_settings: bool,
    pub advanced_ports: bool,
    /// 0 none, 1 GM, 2 GS, 3 XG, 4 MT-32.
    pub sysex_reset: i32,
    /// 0 GM, 1 GS/SC-55, 2 MT-32.
    pub instrument_map: i32,
    pub highlight_palette: i32,
    pub velocity_color: bool,
    pub octave_subscript: bool,
    /// 0 never, 1 minimal, 2 when active, 3 always.
    pub names_visibility: i32,
    pub lyrics_font: String,
    /// Headless carried key: parsed like the rest, applied to nothing (D13).
    pub notes_font: String,
    pub future_color: String,
    pub past_color: String,
    pub single_color: String,
    pub highlight_color: String,
    /// 0 left, 1 center, 2 right.
    pub text_alignment: i32,
    pub window_width: i32,
    pub window_height: i32,
    pub playlist_visible: bool,
    /// 0 none, 1 song, 2 playlist.
    pub repeat_mode: i32,
    /// MRU-10 recent files (see [`Settings::add_recent`]).
    pub recent_files: Vec<String>,
    /// Unknown keys/groups preserved verbatim across load→save (A11).
    unknown: KeyFile,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from `paths.config_file()`, starting from defaults. A missing,
    /// unreadable, non-UTF-8 or structurally corrupt file yields defaults
    /// (`settings.cpp:79-84`).
    pub fn load(paths: &Paths) -> Self {
        let mut settings = Self::new();
        if settings.load_file(paths.config_file()).is_err() {
            return Self::new();
        }
        settings
    }

    /// Load from an explicit file. `Ok` = parsed (per-key errors already
    /// folded to keep-current); `Err` = file unreadable / non-UTF-8 /
    /// structurally corrupt, `self` untouched. (On non-UTF-8 input C++
    /// salvages the good keys instead — see the module docs.)
    pub fn load_file(&mut self, path: &Path) -> Result<(), ParseError> {
        let bytes = std::fs::read(path).map_err(|e| ParseError {
            line: 0,
            message: format!("cannot read {}: {e}", path.display()),
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|_| ParseError {
            line: 0,
            message: "config file is not valid UTF-8".to_string(),
        })?;
        self.load_from_str(text)
    }

    /// Apply parsed text onto `self`. `Err` = structural failure, `self`
    /// untouched; per-key failures keep each field's current value.
    pub fn load_from_str(&mut self, text: &str) -> Result<(), ParseError> {
        let kf = KeyFile::parse(text)?;
        macro_rules! load_str {
            ($field:ident, $key:literal) => {
                if let Some(Ok(v)) = kf.get_string(GROUP, $key) {
                    self.$field = v;
                }
            };
        }
        macro_rules! load_int {
            ($field:ident, $key:literal) => {
                if let Some(Ok(v)) = kf.get_int(GROUP, $key) {
                    self.$field = v;
                }
            };
        }
        macro_rules! load_bool {
            ($field:ident, $key:literal) => {
                if let Some(Ok(v)) = kf.get_bool(GROUP, $key) {
                    self.$field = v;
                }
            };
        }
        load_str!(last_directory, "lastDirectory");
        load_str!(last_output_backend, "lastOutputBackend");
        load_str!(last_output_connection, "lastOutputConnection");
        load_str!(last_play_list, "lastPlayList");
        load_str!(sound_font, "soundFont");
        load_str!(language, "language");
        load_int!(drums_channel, "drumsChannel");
        load_int!(solo_volume_reduction, "soloVolumeReduction");
        load_bool!(auto_play, "autoPlay");
        load_bool!(auto_advance, "autoAdvance");
        load_bool!(auto_song_settings, "autoSongSettings");
        load_bool!(advanced_ports, "advancedPorts");
        load_int!(sysex_reset, "sysexReset");
        load_int!(instrument_map, "instrumentMap");
        load_int!(highlight_palette, "highlightPalette");
        load_bool!(velocity_color, "velocityColor");
        load_bool!(octave_subscript, "octaveSubscript");
        load_int!(names_visibility, "namesVisibility");
        load_str!(lyrics_font, "lyricsFont");
        load_str!(notes_font, "notesFont");
        load_str!(future_color, "futureColor");
        // `colorOr(str(…), "#888888")` (settings.cpp:134): an empty
        // futureColor — even a kept in-memory one — falls back.
        if self.future_color.is_empty() {
            self.future_color = "#888888".to_string();
        }
        load_str!(past_color, "pastColor");
        load_str!(single_color, "singleColor");
        load_str!(highlight_color, "highlightColor");
        load_int!(text_alignment, "textAlignment");
        load_int!(window_width, "windowWidth");
        load_int!(window_height, "windowHeight");
        load_bool!(playlist_visible, "playlistVisible");
        load_int!(repeat_mode, "repeatMode");
        // C++ clears then fills: missing/invalid → empty (not keep-current).
        self.recent_files = kf
            .get_string_list(GROUP, "recentFiles")
            .and_then(Result::ok)
            .unwrap_or_default();
        // Fresh unknown tail from this file (not merged with previous loads).
        self.unknown = kf.without_keys(GROUP, &KNOWN_KEYS);
        Ok(())
    }

    /// Save the full config (known keys in C++ order, then the preserved
    /// unknown tail), creating the config dir on demand. Unconditional —
    /// there is no config dirty flag in C++ either.
    pub fn save(&self, paths: &Paths) -> std::io::Result<()> {
        paths.ensure_config_dir()?;
        std::fs::write(paths.config_file(), self.to_data())
    }

    /// Serialize exactly as [`Settings::save`] would write.
    pub fn to_data(&self) -> String {
        let mut kf = KeyFile::new();
        kf.set_string(GROUP, "lastDirectory", &self.last_directory);
        kf.set_string(GROUP, "lastOutputBackend", &self.last_output_backend);
        kf.set_string(GROUP, "lastOutputConnection", &self.last_output_connection);
        kf.set_string(GROUP, "lastPlayList", &self.last_play_list);
        kf.set_string(GROUP, "soundFont", &self.sound_font);
        kf.set_string(GROUP, "language", &self.language);
        kf.set_int(GROUP, "drumsChannel", self.drums_channel);
        kf.set_int(GROUP, "soloVolumeReduction", self.solo_volume_reduction);
        kf.set_bool(GROUP, "autoPlay", self.auto_play);
        kf.set_bool(GROUP, "autoAdvance", self.auto_advance);
        kf.set_bool(GROUP, "autoSongSettings", self.auto_song_settings);
        kf.set_bool(GROUP, "advancedPorts", self.advanced_ports);
        kf.set_int(GROUP, "sysexReset", self.sysex_reset);
        kf.set_int(GROUP, "instrumentMap", self.instrument_map);
        kf.set_int(GROUP, "highlightPalette", self.highlight_palette);
        kf.set_bool(GROUP, "velocityColor", self.velocity_color);
        kf.set_bool(GROUP, "octaveSubscript", self.octave_subscript);
        kf.set_int(GROUP, "namesVisibility", self.names_visibility);
        kf.set_string(GROUP, "lyricsFont", &self.lyrics_font);
        kf.set_string(GROUP, "notesFont", &self.notes_font);
        kf.set_string(GROUP, "futureColor", &self.future_color);
        kf.set_string(GROUP, "pastColor", &self.past_color);
        kf.set_string(GROUP, "singleColor", &self.single_color);
        kf.set_string(GROUP, "highlightColor", &self.highlight_color);
        kf.set_int(GROUP, "textAlignment", self.text_alignment);
        kf.set_int(GROUP, "windowWidth", self.window_width);
        kf.set_int(GROUP, "windowHeight", self.window_height);
        kf.set_bool(GROUP, "playlistVisible", self.playlist_visible);
        kf.set_int(GROUP, "repeatMode", self.repeat_mode);
        // C++ omits the key entirely when the list is empty (settings.cpp:190).
        if !self.recent_files.is_empty() {
            kf.set_string_list(GROUP, "recentFiles", &self.recent_files);
        }
        // Preserved unknown tail, verbatim and in original relative order.
        // The group header is touched first so keyless groups survive
        // (GLib keeps `[Empty]` through `g_key_file_to_data` too — probed).
        for group in &self.unknown.groups {
            kf.group_or_new(&group.name);
            for (k, v) in &group.keys {
                kf.set_raw(&group.name, k, v);
            }
        }
        kf.to_data()
    }

    /// Reset every key (and the unknown tail) to defaults (`*this =
    /// AppSettings{}`).
    pub fn reset_defaults(&mut self) {
        *self = Self::new();
    }

    /// D2 prefs-dialog close commit (T12 devil-BLOCKING FIX): adopt exactly
    /// the 17 D2-owned keys from `scratch` over `self` (live); every other
    /// key stays live — the D3-carried keys (`lastOutputBackend`,
    /// `lastOutputConnection`, `soundFont`, `advancedPorts`), the
    /// headless/session keys (`language`, `notesFont`, `lastDirectory`,
    /// `lastPlayList`, `recentFiles`, `windowWidth`/`windowHeight`,
    /// `playlistVisible`, `repeatMode`), and the unknown tail. The 17, in C++
    /// page order (General `window.cpp:1198-1237`, Piano `:1274-1306`;
    /// Lyrics rows per the T11b `lyrics_prefs_rows` contract):
    /// General 7 — `drumsChannel`, `soloVolumeReduction`, `autoPlay`,
    /// `autoAdvance`, `autoSongSettings`, `sysexReset`, `instrumentMap`;
    /// Lyrics 5 — `futureColor`, `pastColor`, `highlightColor`, `lyricsFont`,
    /// `textAlignment`; Piano 5 — `highlightPalette`, `singleColor`,
    /// `velocityColor`, `namesVisibility`, `octaveSubscript`.
    ///
    /// Rationale: the scratch is seeded at dialog open, so a wholesale
    /// live←scratch replace silently reverts any live write that lands while
    /// the dialog is open (`WindowResized`, `on_opened` recents,
    /// `PlaylistSaved`). The overlay captures pointer input, but OS-level and
    /// async writers stay reachable, so the commit is narrowed instead —
    /// D3-carry is then satisfied from live (definitionally current).
    pub fn commit_prefs_dialog(&mut self, scratch: &Settings) {
        // General 7.
        self.drums_channel = scratch.drums_channel;
        self.solo_volume_reduction = scratch.solo_volume_reduction;
        self.auto_play = scratch.auto_play;
        self.auto_advance = scratch.auto_advance;
        self.auto_song_settings = scratch.auto_song_settings;
        self.sysex_reset = scratch.sysex_reset;
        self.instrument_map = scratch.instrument_map;
        // Lyrics 5.
        self.future_color.clone_from(&scratch.future_color);
        self.past_color.clone_from(&scratch.past_color);
        self.highlight_color.clone_from(&scratch.highlight_color);
        self.lyrics_font.clone_from(&scratch.lyrics_font);
        self.text_alignment = scratch.text_alignment;
        // Piano 5.
        self.highlight_palette = scratch.highlight_palette;
        self.single_color.clone_from(&scratch.single_color);
        self.velocity_color = scratch.velocity_color;
        self.names_visibility = scratch.names_visibility;
        self.octave_subscript = scratch.octave_subscript;
    }

    /// Push `path` to the front of the MRU list, dropping any older
    /// duplicate and capping at 10 entries (`settings.cpp:202-208`).
    pub fn add_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.recent_files.truncate(10);
    }
}

impl Default for Settings {
    /// C++ `settings.hpp` member initializers, verbatim.
    fn default() -> Self {
        Self {
            last_directory: String::new(),
            last_output_backend: "FluidSynth".to_string(),
            last_output_connection: "fluidsynth".to_string(),
            last_play_list: String::new(),
            sound_font: String::new(),
            language: String::new(),
            drums_channel: 10,
            solo_volume_reduction: 50,
            auto_play: true,
            auto_advance: true,
            auto_song_settings: false,
            advanced_ports: false,
            sysex_reset: 1,
            instrument_map: 0,
            highlight_palette: 2,
            velocity_color: true,
            octave_subscript: false,
            names_visibility: 1,
            lyrics_font: "Sans 16".to_string(),
            notes_font: "Sans 9".to_string(),
            future_color: "#888888".to_string(),
            past_color: "#1c71d8".to_string(),
            single_color: "#f6d32d".to_string(),
            highlight_color: "#e01b24".to_string(),
            text_alignment: 0,
            window_width: 1100,
            window_height: 720,
            playlist_visible: true,
            repeat_mode: 0,
            recent_files: Vec::new(),
            unknown: KeyFile::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::paths::Env;

    /// Representative real-world `dmidiplayer.conf`: all 30 keys with
    /// non-default values, escaped names, unicode, plus unknown keys/groups.
    /// Shaped exactly like `g_key_file_to_data` output (probe-verified).
    const FIXTURE: &str = "[General]\n\
        lastDirectory=/home/u/MIDI\n\
        lastOutputBackend=ALSA\n\
        lastOutputConnection=128:0\n\
        lastPlayList=relative/gig.lst\n\
        soundFont=/home/u/sf/FluidR3 GM.sf2\n\
        language=de\n\
        drumsChannel=9\n\
        soloVolumeReduction=75\n\
        autoPlay=false\n\
        autoAdvance=1\n\
        autoSongSettings=true\n\
        advancedPorts=0\n\
        sysexReset=2\n\
        instrumentMap=1\n\
        highlightPalette=0\n\
        velocityColor=0\n\
        octaveSubscript=1\n\
        namesVisibility=3\n\
        lyricsFont=DejaVu Sans Mono Bold 11\n\
        notesFont=Sans 9\n\
        futureColor=rgb(255,0,0)\n\
        pastColor=#1c71d8\n\
        singleColor=#f6d32d\n\
        highlightColor=#e01b24\n\
        textAlignment=1\n\
        windowWidth=1280\n\
        windowHeight=800\n\
        playlistVisible=false\n\
        repeatMode=2\n\
        recentFiles=/a/one.mid;/b/two\\;live.kar;/c/\\slead.mid;/d/trail\\s.mid;/e/café.mid;\n\
        customKey=custom-value\n\
        odd\\skey=odd\\;raw\n\
        \n\
        [Extra]\n\
        keepme=yes\n";

    #[test]
    fn defaults_match_cpp_headers() {
        let s = Settings::new();
        assert_eq!(s.last_directory, "");
        assert_eq!(s.last_output_backend, "FluidSynth");
        assert_eq!(s.last_output_connection, "fluidsynth");
        assert_eq!(s.last_play_list, "");
        assert_eq!(s.sound_font, "");
        assert_eq!(s.language, "");
        assert_eq!(s.drums_channel, 10);
        assert_eq!(s.solo_volume_reduction, 50);
        assert!(s.auto_play);
        assert!(s.auto_advance);
        assert!(!s.auto_song_settings);
        assert!(!s.advanced_ports);
        assert_eq!(s.sysex_reset, 1);
        assert_eq!(s.instrument_map, 0);
        assert_eq!(s.highlight_palette, 2);
        assert!(s.velocity_color);
        assert!(!s.octave_subscript);
        assert_eq!(s.names_visibility, 1);
        assert_eq!(s.lyrics_font, "Sans 16");
        assert_eq!(s.notes_font, "Sans 9");
        assert_eq!(s.future_color, "#888888");
        assert_eq!(s.past_color, "#1c71d8");
        assert_eq!(s.single_color, "#f6d32d");
        assert_eq!(s.highlight_color, "#e01b24");
        assert_eq!(s.text_alignment, 0);
        assert_eq!(s.window_width, 1100);
        assert_eq!(s.window_height, 720);
        assert!(s.playlist_visible);
        assert_eq!(s.repeat_mode, 0);
        assert!(s.recent_files.is_empty());
        assert_eq!(KNOWN_KEYS.len(), 30);
    }

    #[test]
    fn default_serialization_matches_cpp_byte_shape() {
        // Exactly what C++ `save()` writes for default settings: 29 keys in
        // `settings.cpp:157-185` order (`recentFiles` omitted when empty),
        // single trailing `\n`.
        let expected = "[General]\n\
            lastDirectory=\n\
            lastOutputBackend=FluidSynth\n\
            lastOutputConnection=fluidsynth\n\
            lastPlayList=\n\
            soundFont=\n\
            language=\n\
            drumsChannel=10\n\
            soloVolumeReduction=50\n\
            autoPlay=true\n\
            autoAdvance=true\n\
            autoSongSettings=false\n\
            advancedPorts=false\n\
            sysexReset=1\n\
            instrumentMap=0\n\
            highlightPalette=2\n\
            velocityColor=true\n\
            octaveSubscript=false\n\
            namesVisibility=1\n\
            lyricsFont=Sans 16\n\
            notesFont=Sans 9\n\
            futureColor=#888888\n\
            pastColor=#1c71d8\n\
            singleColor=#f6d32d\n\
            highlightColor=#e01b24\n\
            textAlignment=0\n\
            windowWidth=1100\n\
            windowHeight=720\n\
            playlistVisible=true\n\
            repeatMode=0\n";
        assert_eq!(Settings::new().to_data(), expected);
    }

    #[test]
    fn full_fixture_parses_all_30_keys() {
        let mut s = Settings::new();
        s.load_from_str(FIXTURE).unwrap();
        assert_eq!(s.last_directory, "/home/u/MIDI");
        assert_eq!(s.last_output_backend, "ALSA");
        assert_eq!(s.last_output_connection, "128:0");
        // lastPlayList passthrough: stored verbatim, never resolved.
        assert_eq!(s.last_play_list, "relative/gig.lst");
        assert_eq!(s.sound_font, "/home/u/sf/FluidR3 GM.sf2");
        assert_eq!(s.language, "de");
        assert_eq!(s.drums_channel, 9);
        assert_eq!(s.solo_volume_reduction, 75);
        assert!(!s.auto_play);
        assert!(s.auto_advance); // "1" is true
        assert!(s.auto_song_settings);
        assert!(!s.advanced_ports); // "0" is false
        assert_eq!(s.sysex_reset, 2);
        assert_eq!(s.instrument_map, 1);
        assert_eq!(s.highlight_palette, 0);
        assert!(!s.velocity_color);
        assert!(s.octave_subscript);
        assert_eq!(s.names_visibility, 3);
        assert_eq!(s.lyrics_font, "DejaVu Sans Mono Bold 11");
        assert_eq!(s.future_color, "rgb(255,0,0)");
        assert_eq!(s.text_alignment, 1);
        assert_eq!(s.window_width, 1280);
        assert_eq!(s.window_height, 800);
        assert!(!s.playlist_visible);
        assert_eq!(s.repeat_mode, 2);
        assert_eq!(
            s.recent_files,
            vec![
                "/a/one.mid",
                "/b/two;live.kar",
                "/c/ lead.mid",
                "/d/trail .mid",
                "/e/café.mid", // raw UTF-8, as GLib writes it
            ]
        );
    }

    #[test]
    fn fixture_round_trip_is_stable_and_preserves_unknowns() {
        let mut s = Settings::new();
        s.load_from_str(FIXTURE).unwrap();
        assert_eq!(s.recent_files.len(), 5);
        let once = s.to_data();
        let mut s2 = Settings::new();
        s2.load_from_str(&once).unwrap();
        assert_eq!(s, s2);
        assert_eq!(s2.to_data(), once, "parse→write→parse must be stable");
        // Unknown keys/groups preserved verbatim (A11 improvement over C++,
        // which drops them): note the raw `\;` scalar and padded group.
        assert!(once.contains("customKey=custom-value\n"));
        assert!(once.contains("odd\\skey=odd\\;raw\n"));
        assert!(once.contains("[Extra]\nkeepme=yes\n"));
        // Unknown [General] keys come after the last known key.
        let recents_pos = once.find("recentFiles=").unwrap();
        let custom_pos = once.find("customKey=").unwrap();
        assert!(recents_pos < custom_pos);
    }

    #[test]
    fn invalid_list_escape_clears_recents_like_cpp() {
        // GLib `get_string_list` fails on `\u` (probe) and C++ then keeps the
        // cleared list.
        let mut s = Settings::new();
        s.recent_files = vec!["/old.mid".to_string()];
        s.load_from_str("[General]\nrecentFiles=/e/caf\\u00e9.mid;\n")
            .unwrap();
        assert!(s.recent_files.is_empty());
    }

    #[test]
    fn corrupt_variants_keep_everything() {
        let bad = [
            "garbage no group\n",
            "[General\nkey=value\n",
            "[General]\nnoequals\n",
            ";semicolon is not a comment\n[General]\nk=v\n",
            "\u{feff}[General]\nk=v\n", // BOM fails the whole load (probe)
            "[]\nk=v\n",
            "[General] trailing\nk=v\n",
            "k=v\n",           // key outside any group
            "[General]\n=v\n", // empty key
        ];
        for text in bad {
            let mut s = Settings::new();
            s.last_directory = "keep".to_string();
            s.drums_channel = 3;
            s.recent_files = vec!["/keep.mid".to_string()];
            let err = s.load_from_str(text).expect_err("must be corrupt");
            assert!(err.line > 0 || !err.message.is_empty());
            assert_eq!(s.last_directory, "keep");
            assert_eq!(s.drums_channel, 3);
            assert_eq!(s.recent_files, vec!["/keep.mid".to_string()]);
        }
    }

    #[test]
    fn per_key_errors_keep_current_value() {
        let mut s = Settings::new();
        s.drums_channel = 3;
        s.auto_play = false;
        s.lyrics_font = "Keep 12".to_string();
        s.load_from_str(
            "[General]\n\
             drumsChannel=42x\n\
             autoPlay=True\n\
             lyricsFont=a\\xb\n",
        )
        .unwrap();
        assert_eq!(s.drums_channel, 3);
        assert!(!s.auto_play);
        assert_eq!(s.lyrics_font, "Keep 12");

        // Integer matrix (probe): 0x/decimal-point/overflow/empty all fail.
        for bad in ["0x10", "42x", "3.5", "99999999999999999999", "", "4 2"] {
            let mut s = Settings::new();
            s.drums_channel = 3;
            s.load_from_str(&format!("[General]\ndrumsChannel={bad}\n"))
                .unwrap();
            assert_eq!(s.drums_channel, 3, "bad int {bad:?} must keep current");
        }
        for (good, want) in [("042", 42), ("+5", 5), ("-3", -3), (" 7 ", 7)] {
            let mut s = Settings::new();
            s.load_from_str(&format!("[General]\ndrumsChannel={good}\n"))
                .unwrap();
            assert_eq!(s.drums_channel, want);
        }
        // Boolean matrix (probe): only lowercase true/false/1/0.
        for bad in ["True", "TRUE", "False", "yes", "t", "", "2"] {
            let mut s = Settings::new();
            s.auto_play = true;
            s.load_from_str(&format!("[General]\nautoPlay={bad}\n"))
                .unwrap();
            assert!(s.auto_play, "bad bool {bad:?} must keep current");
        }
        for (good, want) in [("true", true), ("false", false), ("1", true), ("0", false)] {
            let mut s = Settings::new();
            s.load_from_str(&format!("[General]\nautoPlay={good}\n"))
                .unwrap();
            assert_eq!(s.auto_play, want);
        }
    }

    #[test]
    fn empty_future_color_falls_back_like_color_or() {
        let mut s = Settings::new();
        s.load_from_str("[General]\nfutureColor=\n").unwrap();
        assert_eq!(s.future_color, "#888888");
        // Even a kept in-memory empty string falls back (C++ colorOr wraps the
        // fallback value too).
        let mut s = Settings::new();
        s.future_color.clear();
        s.load_from_str("[General]\nlanguage=en\n").unwrap();
        assert_eq!(s.future_color, "#888888");
    }

    #[test]
    fn duplicates_last_wins_and_groups_merge() {
        let mut s = Settings::new();
        s.load_from_str("[General]\nlanguage=first\nlanguage=second\n")
            .unwrap();
        assert_eq!(s.language, "second");
        // Group names are case-sensitive: [general] is unknown, preserved.
        let mut s = Settings::new();
        s.load_from_str("[general]\nlanguage=xx\n").unwrap();
        assert_eq!(s.language, "");
        assert!(s.to_data().contains("[general]\nlanguage=xx\n"));
    }

    #[test]
    fn escape_matrices_round_trip() {
        // (in-memory, on-disk) pairs, probe-verified against GLib.
        let cases = [
            ("hello", "hello"),
            ("a\nb", "a\\nb"),
            ("a\tb", "a\tb"), // raw tab mid-string
            ("a\rb", "a\\rb"),
            ("a\\b", "a\\\\b"),
            ("  lead", "\\s\\slead"),
            ("trail  ", "trail  "),
            ("a;b", "a;b"),
            ("a#b", "a#b"),
            ("a=b", "a=b"),
            ("café 中", "café 中"),
            ("", ""),
            (" ", "\\s"),
            ("\ta", "\\ta"), // leading tab IS escaped
            ("a\r\nb", "a\\r\\nb"),
            ("#x", "#x"),
            (";x", ";x"),
        ];
        for (mem, disk) in cases {
            assert_eq!(escape_scalar(mem), disk, "escape {mem:?}");
            assert_eq!(
                unescape_scalar(disk),
                Ok(mem.to_string()),
                "unescape {disk:?}"
            );
        }
        for bad in ["a\\xb", "a\\;b", "abc\\", "caf\\u00e9"] {
            assert_eq!(
                unescape_scalar(bad),
                Err(()),
                "{bad:?} must be a per-key error"
            );
        }
        // List elements add `;` → `\;`.
        assert_eq!(escape_list_element("a;b"), "a\\;b");
        assert_eq!(escape_list_element(" x"), "\\sx");
        assert_eq!(
            unescape_list("/a/b.mid;x\\;y;trail ;"),
            Ok(vec![
                "/a/b.mid".to_string(),
                "x;y".to_string(),
                "trail ".to_string()
            ])
        );
        assert_eq!(unescape_list(""), Ok(Vec::new()));
        assert_eq!(unescape_list(";"), Ok(vec![String::new()]));
    }

    #[test]
    fn mru_cap_order_and_dedupe() {
        let mut s = Settings::new();
        for i in 0..12 {
            s.add_recent(&format!("/s{i}.mid"));
        }
        assert_eq!(s.recent_files.len(), 10);
        assert_eq!(s.recent_files[0], "/s11.mid");
        assert_eq!(s.recent_files[9], "/s2.mid");
        // Re-adding moves to front without growing.
        s.add_recent("/s5.mid");
        assert_eq!(s.recent_files.len(), 10);
        assert_eq!(s.recent_files[0], "/s5.mid");
        assert_eq!(
            s.recent_files
                .iter()
                .filter(|p| p.as_str() == "/s5.mid")
                .count(),
            1
        );
    }

    #[test]
    fn reset_defaults_clears_everything() {
        let mut s = Settings::new();
        s.load_from_str(FIXTURE).unwrap();
        assert_eq!(s.recent_files.len(), 5);
        s.reset_defaults();
        assert_eq!(s, Settings::new());
    }

    fn test_paths(tmp: &Path) -> Paths {
        let env = Env::for_test(
            &tmp.join("home"),
            &tmp.join("home/.config"),
            &tmp.join("home/.local/share"),
            tmp,
        );
        Paths::with_env(env, None)
    }

    #[test]
    fn save_creates_dir_and_round_trips_through_real_file() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        assert!(!paths.config_dir().exists());
        let mut s = Settings::new();
        s.last_directory = "/home/u/MIDI\nnewline".to_string();
        s.drums_channel = 9;
        s.add_recent("/a/one.mid");
        s.add_recent("/b/two;live.kar");
        s.save(&paths).unwrap();
        assert!(paths.config_file().is_file());
        // On-disk shape: escaped newline, `\;` in the list.
        let disk = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(disk.contains("lastDirectory=/home/u/MIDI\\nnewline\n"));
        assert!(disk.contains("recentFiles=/b/two\\;live.kar;/a/one.mid;\n"));
        let back = Settings::load(&paths);
        assert_eq!(back, s);
    }

    #[test]
    fn load_missing_or_unreadable_file_yields_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        assert_eq!(Settings::load(&paths), Settings::new());
        // Corrupt real file → defaults as well.
        std::fs::create_dir_all(paths.config_dir()).unwrap();
        std::fs::write(paths.config_file(), "garbage no group\n").unwrap();
        assert_eq!(Settings::load(&paths), Settings::new());
        // Invalid UTF-8 → defaults.
        std::fs::write(paths.config_file(), [0xff, 0xfe, 0x0a]).unwrap();
        assert_eq!(Settings::load(&paths), Settings::new());
    }

    #[test]
    fn empty_unknown_groups_survive_round_trip_like_glib() {
        let mut s = Settings::new();
        s.load_from_str("[General]\nlanguage=en\n\n[Empty]\n\n[Other]\nkeepme=yes\n")
            .unwrap();
        let data = s.to_data();
        assert!(data.contains("[Empty]\n"), "keyless group kept: {data}");
        assert!(data.contains("[Other]\nkeepme=yes\n"));
        // Stable across a second round trip.
        let mut s2 = Settings::new();
        s2.load_from_str(&data).unwrap();
        assert_eq!(s2.to_data(), data);
    }

    #[test]
    fn save_omits_empty_recents_but_keeps_present() {
        let s = Settings::new();
        assert!(!s.to_data().contains("recentFiles"));
        let mut s = Settings::new();
        s.add_recent("/only.mid");
        assert!(s.to_data().contains("recentFiles=/only.mid;\n"));
    }

    /// Distinctive live vs scratch on ALL 30 keys + the unknown tail: the
    /// close commit adopts exactly the 17 D2-owned keys and keeps everything
    /// else live (T12 devil-BLOCKING FIX — a wholesale replace would revert
    /// live writes that land while the dialog is open).
    #[test]
    fn commit_prefs_dialog_adopts_exactly_17_keys() {
        let mut live = Settings::new();
        live.load_from_str(FIXTURE).unwrap();
        let live_before = live.clone();
        let mut scratch = Settings::new();
        // Every D2 key differs from live; every non-D2 key differs too (so a
        // wholesale replace or a missed key shows up on both sides).
        scratch.drums_channel = 1;
        scratch.solo_volume_reduction = 11;
        scratch.auto_play = !live_before.auto_play;
        scratch.auto_advance = !live_before.auto_advance;
        scratch.auto_song_settings = !live_before.auto_song_settings;
        scratch.sysex_reset = 4;
        scratch.instrument_map = 2;
        scratch.future_color = "scratch-future".to_string();
        scratch.past_color = "scratch-past".to_string();
        scratch.highlight_color = "scratch-highlight".to_string();
        scratch.lyrics_font = "Scratch 1".to_string();
        scratch.text_alignment = 2;
        scratch.highlight_palette = 3;
        scratch.single_color = "scratch-single".to_string();
        scratch.velocity_color = !live_before.velocity_color;
        scratch.names_visibility = 0;
        scratch.octave_subscript = !live_before.octave_subscript;
        scratch.last_output_backend = "SCRATCH".to_string();
        scratch.last_output_connection = "scratch".to_string();
        scratch.sound_font = "/scratch.sf2".to_string();
        scratch.advanced_ports = !live_before.advanced_ports;
        scratch.language = "scratch".to_string();
        scratch.notes_font = "Scratch 2".to_string();
        scratch.last_directory = "/scratch".to_string();
        scratch.last_play_list = "scratch.lst".to_string();
        scratch.recent_files = vec!["/scratch.mid".to_string()];
        scratch.window_width = 111;
        scratch.window_height = 222;
        scratch.playlist_visible = !live_before.playlist_visible;
        scratch.repeat_mode = 1;

        live.commit_prefs_dialog(&scratch);

        // The 17 D2-owned keys come from scratch …
        assert_eq!(live.drums_channel, 1);
        assert_eq!(live.solo_volume_reduction, 11);
        assert_eq!(live.auto_play, scratch.auto_play);
        assert_eq!(live.auto_advance, scratch.auto_advance);
        assert_eq!(live.auto_song_settings, scratch.auto_song_settings);
        assert_eq!(live.sysex_reset, 4);
        assert_eq!(live.instrument_map, 2);
        assert_eq!(live.future_color, "scratch-future");
        assert_eq!(live.past_color, "scratch-past");
        assert_eq!(live.highlight_color, "scratch-highlight");
        assert_eq!(live.lyrics_font, "Scratch 1");
        assert_eq!(live.text_alignment, 2);
        assert_eq!(live.highlight_palette, 3);
        assert_eq!(live.single_color, "scratch-single");
        assert_eq!(live.velocity_color, scratch.velocity_color);
        assert_eq!(live.names_visibility, 0);
        assert_eq!(live.octave_subscript, scratch.octave_subscript);
        // … and nothing else moves: D3-carried, headless/session, recents,
        // window, repeat, and the unknown tail (FIXTURE carries `customKey`
        // + `[Extra]`, asserted through the serialized form).
        assert_eq!(live.last_output_backend, live_before.last_output_backend);
        assert_eq!(
            live.last_output_connection,
            live_before.last_output_connection
        );
        assert_eq!(live.sound_font, live_before.sound_font);
        assert_eq!(live.advanced_ports, live_before.advanced_ports);
        assert_eq!(live.language, live_before.language);
        assert_eq!(live.notes_font, live_before.notes_font);
        assert_eq!(live.last_directory, live_before.last_directory);
        assert_eq!(live.last_play_list, live_before.last_play_list);
        assert_eq!(live.recent_files, live_before.recent_files);
        assert_eq!(live.window_width, live_before.window_width);
        assert_eq!(live.window_height, live_before.window_height);
        assert_eq!(live.playlist_visible, live_before.playlist_visible);
        assert_eq!(live.repeat_mode, live_before.repeat_mode);
        assert!(live.to_data().contains("customKey=custom-value\n"));
        assert!(live.to_data().contains("[Extra]\nkeepme=yes\n"));
    }

    /// Narrow commit equals wholesale replace when live is otherwise
    /// untouched (the common close path — no behavior change there).
    #[test]
    fn commit_prefs_dialog_equals_wholesale_when_untouched() {
        let live = Settings::new();
        let mut scratch = live.clone();
        scratch.drums_channel = 3;
        scratch.solo_volume_reduction = 75;
        scratch.future_color = "blue".to_string();
        scratch.single_color = "notacolor".to_string();
        let mut narrowed = live.clone();
        narrowed.commit_prefs_dialog(&scratch);
        assert_eq!(narrowed, scratch);
    }
}
