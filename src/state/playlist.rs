//! Playlist state: `.lst` load/save + current-index semantics (arch §1.6, A14).
//!
//! Mirrors `src/app/playlist.{hpp,cpp}` exactly:
//!
//! - `.lst` = one path per line; CRLF tolerated, blank lines skipped;
//!   relative entries resolve against the playlist file's directory;
//!   `weakly_canonical` + supported-extension filter on load/add;
//!   **nonexistent paths are kept** (no existence check, A14).
//! - `save` writes absolute paths (LF only).
//! - Index semantics (`current`, `-1` = none): `load`/`set_items` → 0 if
//!   non-empty else -1; `clear` → -1; `add` → 0 when -1; `remove_at` keeps
//!   the index (clamping only when it ran past the end); `move_up`/`move_down`
//!   move `current` with the item; `select_next`/`select_prev` return `false`
//!   at the ends (no wrap); `shuffle` leaves `current` untouched; `at_first`
//!   = `current < 1` (true when empty); `at_last` = `current == len-1` (true
//!   when empty, since -1 == -1 — preserved quirk).
//!
//! Idiom deviations from C++ (behavior-identical): items are [`PathBuf`]s,
//! `current()` returns `Option<&Path>` instead of `""`-when-out-of-range,
//! `add` reports whether the path was accepted, `save` takes `&mut self`
//! instead of `const_cast`, and the misleadingly-named `fileName()` (which
//! returns the full source path, or `""`) is [`PlaylistState::source_path`]
//! (`None` when unset).

use std::path::{Path, PathBuf};

use crate::core::event::is_supported_midi_file;

// ---------------------------------------------------------------------------
// PlaylistState
// ---------------------------------------------------------------------------

/// Playlist items + current index + dirty flag + source path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistState {
    items: Vec<PathBuf>,
    source: Option<PathBuf>,
    current: i32,
    dirty: bool,
}

impl Default for PlaylistState {
    /// Empty list with `current = -1` (C++ `m_current{-1}` member init —
    /// a derived `Default` would wrongly give 0).
    fn default() -> Self {
        Self {
            items: Vec::new(),
            source: None,
            current: -1,
            dirty: false,
        }
    }
}

impl PlaylistState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a `.lst` file, replacing the current items. Returns `false` when
    /// the file cannot be read, leaving all state untouched (`playlist.cpp:15-19`
    /// returns before `m_items.clear()`).
    pub fn load(&mut self, path: &Path) -> bool {
        let lines = match read_lst_lines(path) {
            Ok(lines) => lines,
            Err(_) => return false,
        };
        // `parent_path(fileName)`; a bare file name yields `""`, so relative
        // entries resolve against the CWD via `weakly_canonical` — as in C++.
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        let mut items = Vec::new();
        for line in lines {
            let joined = base.join(&line);
            if is_supported_midi_file(&joined) {
                items.push(weakly_canonical(&joined));
            }
        }
        self.items = items;
        self.source = Some(path.to_path_buf());
        self.current = if self.items.is_empty() { -1 } else { 0 };
        self.dirty = false;
        true
    }

    /// Save all items (absolute paths, LF) to `path`. On success records the
    /// source path and clears the dirty flag.
    pub fn save(&mut self, path: &Path) -> bool {
        let mut buf = Vec::new();
        for item in &self.items {
            buf.extend_from_slice(path_bytes(item));
            buf.push(b'\n');
        }
        match std::fs::write(path, buf) {
            Ok(()) => {
                self.source = Some(path.to_path_buf());
                self.dirty = false;
                true
            }
            Err(_) => false,
        }
    }

    /// Clear all items (dirty only when something was cleared).
    pub fn clear(&mut self) {
        if !self.items.is_empty() {
            self.items.clear();
            self.current = -1;
            self.dirty = true;
        }
    }

    /// Fisher–Yates shuffle. Leaves `current` untouched (A14) and always
    /// marks dirty — even for an empty list, like the C++.
    pub fn shuffle(&mut self) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        let seed = nanos ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.shuffle_with_seed(seed);
    }

    /// Deterministic shuffle for tests (same algorithm as [`PlaylistState::shuffle`]).
    pub fn shuffle_with_seed(&mut self, seed: u64) {
        let mut state = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        let len = self.items.len();
        for i in (1..len).rev() {
            let j = (xorshift64(&mut state) % (i as u64 + 1)) as usize;
            self.items.swap(i, j);
        }
        self.dirty = true;
    }

    /// Replace all items (e.g. open-files / DnD replace-playlist).
    pub fn set_items(&mut self, items: Vec<PathBuf>) {
        self.items = items;
        self.current = if self.items.is_empty() { -1 } else { 0 };
        self.dirty = true;
    }

    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    /// Append `path` (canonicalized weakly) when its extension is supported;
    /// silently ignored otherwise. Returns whether it was added.
    pub fn add(&mut self, path: &Path) -> bool {
        if !is_supported_midi_file(path) {
            return false;
        }
        self.items.push(weakly_canonical(path));
        if self.current < 0 {
            self.current = 0;
        }
        self.dirty = true;
        true
    }

    /// Remove the item at `index` (out of range → no-op). The current index
    /// is kept — the next item shifts into place — clamping only when it ran
    /// past the end (A14).
    pub fn remove_at(&mut self, index: i32) {
        if index < 0 || index >= self.len() as i32 {
            return;
        }
        self.items.remove(index as usize);
        if self.current >= self.len() as i32 {
            self.current = self.len() as i32 - 1;
        }
        self.dirty = true;
    }

    /// Swap `index` with the item above it, moving `current` with the item.
    pub fn move_up(&mut self, index: i32) {
        let len = self.len() as i32;
        if index <= 0 || index >= len {
            return;
        }
        self.items.swap((index - 1) as usize, index as usize);
        if self.current == index {
            self.current -= 1;
        } else if self.current == index - 1 {
            self.current += 1;
        }
        self.dirty = true;
    }

    /// Swap `index` with the item below it, moving `current` with the item.
    pub fn move_down(&mut self, index: i32) {
        let len = self.len() as i32;
        if index < 0 || index >= len - 1 {
            return;
        }
        self.items.swap(index as usize, (index + 1) as usize);
        if self.current == index {
            self.current += 1;
        } else if self.current == index + 1 {
            self.current -= 1;
        }
        self.dirty = true;
    }

    /// Current item, or `None` when out of range / empty (C++ returns `""`).
    pub fn current(&self) -> Option<&Path> {
        if self.current < 0 || self.current >= self.len() as i32 {
            return None;
        }
        Some(&self.items[self.current as usize])
    }

    /// Current index (`-1` = none).
    pub fn current_index(&self) -> i32 {
        self.current
    }

    /// Set the current index; out-of-range values are ignored (no dirty change).
    pub fn set_current_index(&mut self, index: i32) {
        if index >= 0 && index < self.len() as i32 {
            self.current = index;
        }
    }

    pub fn select_first(&mut self) -> bool {
        if self.is_empty() {
            return false;
        }
        self.current = 0;
        true
    }

    pub fn select_next(&mut self) -> bool {
        if self.current + 1 < self.len() as i32 {
            self.current += 1;
            return true;
        }
        false
    }

    pub fn select_prev(&mut self) -> bool {
        if self.current > 0 {
            self.current -= 1;
            return true;
        }
        false
    }

    /// `current < 1` — true when empty (preserved quirk).
    pub fn at_first(&self) -> bool {
        self.current < 1
    }

    /// `current == len - 1` — true when empty (-1 == -1, preserved quirk).
    pub fn at_last(&self) -> bool {
        self.current == self.len() as i32 - 1
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// Source `.lst` path from the last load/save, if any.
    pub fn source_path(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// Record the source path after an out-of-band load (T9 `PlaylistLoaded`:
    /// the portal chooser read the file, so `load` already ran elsewhere).
    pub fn set_source(&mut self, source: Option<PathBuf>) {
        self.source = source;
    }
}

fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// `std::filesystem::weakly_canonical` equivalent: canonicalize the longest
/// existing prefix (resolving symlinks), then append the non-existent tail
/// lexically. Relative inputs resolve against the CWD. Never fails.
pub(crate) fn weakly_canonical(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut existing: &Path = &absolute;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        let parent = match existing.parent() {
            Some(parent) => parent,
            None => break, // root/prefix: nothing left to pop
        };
        // `file_name()` is `None` for a trailing `..` (or `.`) — push the
        // `..` literally so the tail resolver below can collapse it.
        match existing.file_name() {
            Some(name) => tail.push(name),
            None if existing.ends_with("..") => tail.push(std::ffi::OsStr::new("..")),
            None if existing.ends_with(".") => {}
            None => break,
        }
        existing = parent;
    }
    let mut out = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    // Append the non-existent tail, resolving `.`/`..` lexically (C++
    // `weakly_canonical` normalizes: `/a/b/../c` with missing `b` → `/a/c`).
    for component in tail.iter().rev() {
        if *component == ".." {
            out.pop();
        } else if *component != "." {
            out.push(component);
        }
    }
    out
}

/// Read `.lst` lines byte-wise (C++ `ifstream` is byte-oriented: a
/// non-UTF-8 entry must load, not fail the whole file). Splits on `\n`,
/// strips one trailing `\r`, skips empties.
fn read_lst_lines(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let bytes = std::fs::read(path)?;
    let mut out = Vec::new();
    for mut line in bytes.split(|b| *b == b'\n') {
        if let Some(stripped) = line.strip_suffix(b"\r") {
            line = stripped;
        }
        if line.is_empty() {
            continue;
        }
        out.push(bytes_to_path(line));
    }
    Ok(out)
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> &[u8] {
    path.to_string_lossy().as_bytes()
}

// ---------------------------------------------------------------------------
// Save-on-close-request (U2 FIX) + save-point truth table
// ---------------------------------------------------------------------------

/// Window-close save bundle: the U2 FIX shape consumed by T9/T16 wiring.
///
/// C++ persists window geometry + repeat mode + the full config in the
/// `MainWindow` destructor (`window.cpp:275-285`). Rust has no equivalent
/// drop hook at the right lifecycle point, so the libcosmic close-request
/// handler must call [`CloseSave::apply`] explicitly. There is deliberately
/// no dirty check: C++ rewrites the whole file unconditionally.
///
/// # Save-point truth table
///
/// Every row is grounded in the C++ call sites; T9/T16 wiring must preserve
/// the "File write?" column or settings silently stop persisting.
///
/// | # | Trigger (C++ site) | In-memory update | Config file write? | Rust wiring |
/// |---|---|---|---|---|
/// | 1 | Window close (dtor `window.cpp:275-285`) | `windowWidth`/`Height` ← actual size; `repeatMode` ← `m_repeat` | **YES, full save** | U2 FIX: close-request → `CloseSave::apply`. Skipping this loses geometry+repeat **and** every pending update from rows 4–5. |
/// | 2 | MIDI output applied (`connectOutput` :332-340, MIDI-setup Apply :1177) | `lastOutputBackend`/`lastOutputConnection` | **YES, full save** | `MidiApply` → update fields → `Settings::save`. |
/// | 3 | Prefs dialog closed (:1310-1336) | all prefs keys, commit-on-close incl. solo% (U14) | **YES, full save** | Prefs close → apply `PrefsChanged` → `Settings::save`. |
/// | 4 | Song loaded (`loadCurrent` :372+) | `addRecent(path)`, `lastDirectory` | **NO** | Persists via the next save (rows 1–3). Same in Rust: no save on song load. |
/// | 5 | Playlist opened (`openFiles` :355-360) | `lastPlayList` | **NO** | Same: the next save persists it. |
/// | 6 | Playlist edited (add/remove/move/shuffle/clear) | items + `dirty = true` | **NO** — `.lst` writes only via the explicit Save action | Same; a dirty playlist on close is dropped silently (no prompt in C++ — preserved behavior, not a bug to fix here). |
/// | 7 | Song settings saved (manual action / auto on leave+finish) | — | `.cfg` file written immediately (separate file, not the global config) | Same via `SongSettings::save`. |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseSave {
    /// Actual window width at close-request time.
    pub width: i32,
    /// Actual window height at close-request time.
    pub height: i32,
    /// Repeat mode (0 none, 1 song, 2 playlist).
    pub repeat_mode: i32,
}

impl CloseSave {
    /// Set geometry + repeat mode on `settings` and save the full config
    /// through `paths` (dir created on demand). Consumed by the T9/T16
    /// close-request handler.
    pub fn apply(
        &self,
        settings: &mut crate::config::settings::Settings,
        paths: &crate::config::paths::Paths,
    ) -> std::io::Result<()> {
        settings.window_width = self.width;
        settings.window_height = self.height;
        settings.repeat_mode = self.repeat_mode;
        settings.save(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::paths::{Env, Paths};
    use crate::config::settings::Settings;

    fn three() -> PlaylistState {
        // Bypass `add` (which canonicalizes against the CWD): these tests
        // pin index semantics, not path resolution.
        let mut p = PlaylistState::new();
        p.set_items(vec![
            PathBuf::from("/m/a.mid"),
            PathBuf::from("/m/b.mid"),
            PathBuf::from("/m/c.mid"),
        ]);
        p.set_dirty(false);
        p
    }

    #[test]
    fn lst_load_shape_crlf_relative_filter_nonexistent_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("gig");
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("song.mid"), b"fake").unwrap();
        std::fs::write(sub.join("deep.kar"), b"fake").unwrap();
        let list = dir.join("gig.lst");
        // CRLF + blank lines + relative + absolute + unsupported + nonexistent.
        let abs_deep = sub.join("deep.kar");
        let body = format!(
            "song.mid\r\n\r\n../outside.mid\r\nnotes.txt\r\n{}\r\nsub/../song.mid\r\n",
            abs_deep.display()
        );
        std::fs::write(&list, body).unwrap();

        let mut p = PlaylistState::new();
        assert!(p.load(&list));
        assert!(!p.dirty());
        assert_eq!(p.current_index(), 0);
        assert_eq!(p.source_path(), Some(list.as_path()));
        // Kept: relative-existing, relative-nonexistent (A14!), absolute,
        // dot-dot-normalized. Dropped: blank lines, notes.txt.
        assert_eq!(p.len(), 4);
        let canon = |rel: &str| std::fs::canonicalize(dir.join(rel)).unwrap();
        assert_eq!(p.items()[0], canon("song.mid"));
        assert!(p.items()[1].is_absolute()); // ../outside.mid kept, normalized
        assert!(p.items()[1].ends_with("outside.mid"));
        assert!(!p.items()[1].exists());
        assert_eq!(p.items()[2], std::fs::canonicalize(&abs_deep).unwrap());
        assert_eq!(p.items()[3], canon("song.mid"));
        assert!(p.items().iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn load_failure_keeps_all_state() {
        let mut p = three();
        p.set_current_index(2);
        p.set_dirty(true);
        assert!(!p.load(Path::new("/nonexistent/dir/no.lst")));
        assert_eq!(p.len(), 3);
        assert_eq!(p.current_index(), 2);
        assert!(p.dirty());
        assert_eq!(p.source_path(), None);
    }

    #[test]
    fn save_writes_absolute_lf_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = three();
        let out = tmp.path().join("out.lst");
        assert!(p.save(&out));
        assert!(!p.dirty());
        assert_eq!(p.source_path(), Some(out.as_path()));
        let raw = std::fs::read(&out).unwrap();
        assert_eq!(raw, b"/m/a.mid\n/m/b.mid\n/m/c.mid\n");
        let mut q = PlaylistState::new();
        assert!(q.load(&out));
        assert_eq!(q.items(), p.items());
        assert_eq!(q.current_index(), 0);
    }

    #[test]
    fn save_failure_reports_false() {
        let mut p = three();
        assert!(!p.save(Path::new("/nonexistent/dir/out.lst")));
        // Untouched on failure: items, source, index and dirty flag kept.
        assert_eq!(p.len(), 3);
        assert_eq!(p.source_path(), None);
        assert_eq!(p.current_index(), 0);
        assert!(!p.dirty());
    }

    #[test]
    fn a14_remove_at_keeps_index_with_end_clamp() {
        // Remove first while current=0: next item shifts into place, index kept.
        let mut p = three();
        p.remove_at(0);
        assert_eq!(
            p.items(),
            &[PathBuf::from("/m/b.mid"), PathBuf::from("/m/c.mid")]
        );
        assert_eq!(p.current_index(), 0);
        assert_eq!(p.current().unwrap(), Path::new("/m/b.mid"));

        // Remove middle above current: index kept (now points at a new item).
        let mut p = three();
        p.set_current_index(2);
        p.remove_at(0);
        assert_eq!(p.current_index(), 2 - 1); // clamped: ran past the end
        assert_eq!(p.current().unwrap(), Path::new("/m/c.mid"));

        // Remove current in the middle: index kept, next shifts in.
        let mut p = three();
        p.set_current_index(1);
        p.remove_at(1);
        assert_eq!(p.current_index(), 1);
        assert_eq!(p.current().unwrap(), Path::new("/m/c.mid"));

        // Remove last while current=last: clamp to new end.
        let mut p = three();
        p.set_current_index(2);
        p.remove_at(2);
        assert_eq!(p.current_index(), 1);

        // Remove down to empty: -1.
        let mut p = three();
        p.remove_at(0);
        p.remove_at(0);
        p.remove_at(0);
        assert!(p.is_empty());
        assert_eq!(p.current_index(), -1);
        assert_eq!(p.current(), None);

        // Out of range: no-op, no dirty change.
        let mut p = three();
        p.set_dirty(false);
        p.remove_at(-1);
        p.remove_at(3);
        assert_eq!(p.len(), 3);
        assert!(!p.dirty());
    }

    #[test]
    fn a14_move_follows_current() {
        let mut p = three();
        p.set_current_index(1);
        p.move_up(1);
        assert_eq!(
            p.items(),
            &[
                PathBuf::from("/m/b.mid"),
                PathBuf::from("/m/a.mid"),
                PathBuf::from("/m/c.mid")
            ]
        );
        assert_eq!(p.current_index(), 0); // moved with the item
        p.move_down(0);
        assert_eq!(p.current_index(), 1);
        assert_eq!(p.items()[1], PathBuf::from("/m/b.mid"));

        // Moving the item BELOW current pushes current down.
        let mut p = three();
        p.set_current_index(0);
        p.move_down(0);
        assert_eq!(p.current_index(), 1);
        // Moving an unrelated item leaves current alone.
        let mut p = three();
        p.set_current_index(0);
        p.move_down(1);
        assert_eq!(p.current_index(), 0);

        // Edges: no-op, no dirty change.
        let mut p = three();
        p.set_dirty(false);
        p.move_up(0);
        p.move_up(9);
        p.move_down(2);
        p.move_down(-1);
        p.move_up(3); // boundary: index == len is out of range
        assert!(!p.dirty());
        assert_eq!(p.current_index(), 0);
    }

    #[test]
    fn a14_select_ends_and_empty_quirks() {
        let mut p = three();
        assert!(p.select_first());
        assert_eq!(p.current_index(), 0);
        assert!(p.at_first());
        assert!(!p.at_last());
        assert!(!p.select_prev()); // at start: false, no wrap
        assert_eq!(p.current_index(), 0);
        assert!(p.select_next());
        assert!(p.select_next());
        assert!(p.at_last());
        assert!(!p.select_next()); // at end: false, no wrap
        assert_eq!(p.current_index(), 2);

        // Empty-list quirks (preserved): at_first AND at_last are true,
        // current() is None, select_first is false.
        let mut e = PlaylistState::new();
        assert!(e.at_first());
        assert!(e.at_last());
        assert_eq!(e.current(), None);
        assert!(!e.select_first());
        assert!(!e.select_next());
        assert!(!e.select_prev());

        // select_next from -1 on a non-empty list advances to 0 (C++ parity:
        // `m_current + 1 < size` with -1 → 0 < size).
        let mut p = three();
        p.set_current_index(0);
        p.current = -1; // same-module white-box: stale -1, non-empty
        assert!(p.select_next());
        assert_eq!(p.current_index(), 0);
    }

    #[test]
    fn a14_shuffle_keeps_index_and_is_seeded() {
        let mut a = three();
        a.set_current_index(1);
        a.shuffle_with_seed(42);
        assert_eq!(a.current_index(), 1); // untouched
        assert!(a.dirty());
        let mut b = three();
        b.shuffle_with_seed(42);
        assert_eq!(a.items(), b.items()); // deterministic per seed
        // Seed-sensitivity on 10 items (a 3-item collision would be likely).
        let ten = || {
            let mut p = PlaylistState::new();
            p.set_items(
                (0..10)
                    .map(|i| PathBuf::from(format!("/m/{i}.mid")))
                    .collect(),
            );
            p
        };
        let mut d = ten();
        d.shuffle_with_seed(42);
        let mut e = ten();
        e.shuffle_with_seed(43);
        assert_ne!(d.items(), e.items());
        // Permutation check: same multiset.
        let mut sorted = a.items().to_vec();
        sorted.sort();
        assert_eq!(
            sorted,
            vec![
                PathBuf::from("/m/a.mid"),
                PathBuf::from("/m/b.mid"),
                PathBuf::from("/m/c.mid")
            ]
        );
        // C++ marks dirty even when shuffling an empty list.
        let mut f = PlaylistState::new();
        f.shuffle_with_seed(1);
        assert!(f.dirty());
        PlaylistState::new().shuffle(); // time-seeded path must not panic
    }

    #[test]
    fn add_clear_set_items_semantics() {
        let mut p = PlaylistState::new();
        assert_eq!(p.current_index(), -1);
        assert!(p.add(Path::new("/m/song.MID"))); // case-insensitive ext
        assert_eq!(p.current_index(), 0); // -1 → 0 on first add
        assert!(!p.add(Path::new("/m/notes.txt"))); // silently ignored
        assert!(!p.add(Path::new(".mid"))); // bare ".mid" has empty ext (T2 rule)
        assert_eq!(p.len(), 1);

        p.clear();
        assert!(p.is_empty());
        assert_eq!(p.current_index(), -1);
        assert!(p.dirty());
        p.set_dirty(false);
        p.clear(); // clearing an empty list: no dirty change
        assert!(!p.dirty());

        p.set_items(vec![PathBuf::from("/x.mid")]);
        assert_eq!(p.current_index(), 0);
        assert!(p.dirty());
        p.set_items(vec![]);
        assert_eq!(p.current_index(), -1);

        // set_current_index ignores out-of-range values.
        let mut p = three();
        p.set_current_index(9);
        p.set_current_index(-2);
        assert_eq!(p.current_index(), 0);
    }

    #[test]
    fn load_current_branch_shape() {
        // `window.cpp:372-376`: `auto path = m_playlist.current(); if
        // (path.empty()) return;` — the Rust twin is `let Some(path) =
        // playlist.current() else { return; };` (T9 wires the real call).
        let empty = PlaylistState::new();
        assert_eq!(empty.current(), None); // → early return: no toast, no change

        // A14 keep-nonexistent ⇒ `current()` is `Some` even for a missing
        // file, so the loader proceeds to `loadFile`, whose failure toasts
        // "Could not load <basename>" (+ the damaged-file warning when the
        // parser reports errors). This pins the branch condition; T9 owns
        // the load + toast.
        let mut p = PlaylistState::new();
        p.set_items(vec![PathBuf::from("/gone/missing.mid")]);
        assert_eq!(p.current(), Some(Path::new("/gone/missing.mid")));
    }

    #[test]
    fn weakly_canonical_resolves_existing_keeps_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real.mid");
        std::fs::write(&real, b"x").unwrap();
        assert_eq!(
            weakly_canonical(&real),
            std::fs::canonicalize(&real).unwrap()
        );
        let missing = tmp.path().join("sub/../gone.mid");
        let weak = weakly_canonical(&missing);
        assert!(weak.is_absolute());
        assert!(weak.ends_with("gone.mid"));
        assert!(!weak.components().any(|c| c.as_os_str() == ".."));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_lst_entries_load_verbatim() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let list = tmp.path().join("weird.lst");
        std::fs::write(&list, b"/m/good.mid\n/tmp/weird-\xff.mid\nnotes.txt\n").unwrap();
        let mut p = PlaylistState::new();
        assert!(p.load(&list));
        assert_eq!(p.len(), 2); // .txt filtered, both .mid kept
        assert_eq!(p.items()[1].as_os_str().as_bytes(), b"/tmp/weird-\xff.mid");
        // Save preserves the raw bytes too.
        let out = tmp.path().join("out.lst");
        assert!(p.save(&out));
        let raw = std::fs::read(&out).unwrap();
        assert!(raw.windows(7).any(|w| w == b"weird-\xff"));
    }

    /// P10b portal-path entries (save/load half; replace/forward halves are
    /// pinned at flow level by `portal_doc_paths_replace_like_normal` and
    /// `second_open_forwards_replace_into_running_state`): never-created
    /// `/run/…/doc/…` `.lst` lines (CRLF tolerated) load verbatim (A14, no
    /// existence check), unsupported extensions still filter, and save → load
    /// round-trips the kept entries.
    #[test]
    fn portal_doc_entries_save_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let list = tmp.path().join("portal.lst");
        std::fs::write(
            &list,
            "/run/user/1000/doc/deadbeef/song.mid\r\n/run/user/1000/doc/deadbeef/notes.txt\n",
        )
        .unwrap();
        let mut p = PlaylistState::new();
        assert!(p.load(&list));
        assert_eq!(p.len(), 1); // .txt filtered, portal .mid kept
        assert!(p.items()[0].is_absolute());
        assert!(p.items()[0].ends_with("doc/deadbeef/song.mid"));
        let out = tmp.path().join("out.lst");
        assert!(p.save(&out));
        assert_eq!(
            std::fs::read(&out).unwrap(),
            format!("{}\n", p.items()[0].display()).into_bytes()
        );
        let mut re = PlaylistState::new();
        assert!(re.load(&out));
        assert_eq!(re.items(), p.items());
    }

    #[test]
    fn close_save_sets_geometry_repeat_and_saves_full_config() {
        // Truth-table row 1 (U2 FIX): close-request persists geometry +
        // repeat + the whole config, creating the config dir on demand.
        let tmp = tempfile::tempdir().unwrap();
        let env = Env::for_test(
            &tmp.path().join("home"),
            &tmp.path().join("home/.config"),
            &tmp.path().join("home/.local/share"),
            tmp.path(),
        );
        let paths = Paths::with_env(env, None);
        let mut settings = Settings::new();
        settings.last_directory = "/pending/rows-4-5".to_string();
        CloseSave {
            width: 1280,
            height: 800,
            repeat_mode: 2,
        }
        .apply(&mut settings, &paths)
        .unwrap();
        assert_eq!(settings.window_width, 1280);
        assert_eq!(settings.window_height, 800);
        assert_eq!(settings.repeat_mode, 2);
        let back = Settings::load(&paths);
        assert_eq!(back.window_width, 1280);
        assert_eq!(back.repeat_mode, 2);
        // Pending row-4/5 updates ride along (full-config save).
        assert_eq!(back.last_directory, "/pending/rows-4-5");
    }
}
