//! Lyrics filter + body state (arch §2.1, ux §3.6).
//!
//! The C++ twin is the lyrics page (`src/ui/window.cpp:1755-1869`):
//! track/type/encoding dropdowns, the filtered static body (`refreshLyrics`)
//! plus the live `onText` tail — including the prefill+append duplication
//! quirk, which is PRESERVED (U10). T11 builds the body from
//! `LoadedSong.texts`; T9 owns the filters, the codec, and the live tail.

/// Lyrics page state (arch §2.1 `LyricsState`).
#[allow(dead_code)] // T11 renders body/live_tail; T9 owns filters + tail.
#[derive(Debug, Clone)]
pub struct LyricsState {
    /// Track dropdown: `None` = all tracks, else 1-based track number.
    pub track_filter: Option<u32>,
    /// Type dropdown: 0 = all types, else `TextType` discriminant.
    pub type_filter: i32,
    /// Encoding dropdown: active decode label (starts at the song's detected
    /// charset, overridden by song settings or the dropdown).
    pub codec: String,
    /// Filtered static text (T11 fills from `LoadedSong.texts`).
    pub body: String,
    /// Live `onText` tail appended during playback (U10 duplication quirk:
    /// sung lines appear here AND in the prefilled body).
    pub live_tail: String,
}

impl Default for LyricsState {
    fn default() -> Self {
        Self {
            track_filter: None,
            // C++ ultimate fallback when no type scores (`window.cpp:569-571`).
            type_filter: 5, // TextType::Lyric
            codec: String::new(),
            body: String::new(),
            live_tail: String::new(),
        }
    }
}

impl LyricsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset on song load: codec starts at the detected charset, body + tail
    /// cleared, filters back to defaults (T11 refines the preselects with
    /// track/type scoring).
    pub fn reset_for_song(&mut self, charset: String) {
        self.track_filter = None;
        self.type_filter = 5;
        self.codec = charset;
        self.body.clear();
        self.live_tail.clear();
    }

    /// Whether a live text event passes the current filters (`onText`
    /// guards, `window.cpp:158-161`: track filter applies when set, type
    /// filter applies when > 0).
    pub fn passes_filter(&self, track: u32, text_type: i32) -> bool {
        if let Some(want) = self.track_filter
            && track != want
        {
            return false;
        }
        if self.type_filter > 0 && text_type != self.type_filter {
            return false;
        }
        true
    }

    /// Append decoded live text when it passes the filters; returns whether
    /// anything was appended.
    pub fn append_live(&mut self, track: u32, text_type: i32, text: &str) -> bool {
        if !self.passes_filter(track, text_type) {
            return false;
        }
        self.live_tail.push_str(text);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_cpp_fallbacks() {
        let l = LyricsState::new();
        assert_eq!(l.track_filter, None);
        assert_eq!(l.type_filter, 5);
        assert!(l.codec.is_empty());
        assert!(l.body.is_empty());
        assert!(l.live_tail.is_empty());
    }

    #[test]
    fn reset_for_song_seeds_codec_and_clears() {
        let mut l = LyricsState::new();
        l.track_filter = Some(3);
        l.type_filter = 1;
        l.codec = "SHIFT_JIS".to_string();
        l.body = "static".to_string();
        l.live_tail = "live".to_string();
        l.reset_for_song("WINDOWS-1252".to_string());
        assert_eq!(l.track_filter, None);
        assert_eq!(l.type_filter, 5);
        assert_eq!(l.codec, "WINDOWS-1252");
        assert!(l.body.is_empty());
        assert!(l.live_tail.is_empty());
    }

    /// `onText` filter matrix (`window.cpp:158-161`).
    #[test]
    fn live_filter_matrix() {
        let mut l = LyricsState::new();
        // Defaults: all tracks, lyric type only.
        assert!(l.passes_filter(7, 5));
        assert!(!l.passes_filter(7, 1));
        // Type 0 = all types.
        l.type_filter = 0;
        assert!(l.passes_filter(7, 1));
        assert!(l.passes_filter(7, 13));
        // Track filter set: only that track passes.
        l.track_filter = Some(2);
        assert!(l.passes_filter(2, 1));
        assert!(!l.passes_filter(3, 1));
    }

    #[test]
    fn append_live_respects_filters_and_concatenates() {
        let mut l = LyricsState::new();
        assert!(l.append_live(1, 5, "Hel"));
        assert!(l.append_live(1, 5, "lo"));
        assert!(!l.append_live(1, 1, "skipped"));
        assert_eq!(l.live_tail, "Hello");
    }
}
