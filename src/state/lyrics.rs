//! Lyrics filter + body state (arch §2.1, ux §3.6).
//!
//! The C++ twin is the lyrics page (`src/ui/window.cpp:1755-1869`):
//! track/type/encoding dropdowns, the filtered static body (`refreshLyrics`,
//! `window.cpp:550-629`) plus the live `onText` tail (`window.cpp:157-179`)
//! — exports use the static filtered body once, without duplicating sung text.
//! T11a owns the state + recompute + prefs-apply half; T11b renders
//! it (dropdowns, `text::Rich` runs, autoscroll snap, D2 Lyrics rows).
//!
//! C++ snap-back FIX (lead-directed, T11): `refreshLyrics` rebuilds the
//! dropdowns AND resets both filters to best-score on every call, and the
//! filter-change handlers call it after storing the user's choice
//! (`window.cpp:1788-1810` → `window.cpp:566-573`) — so any track/type
//! selection snaps back to best-score and only the codec dropdown
//! effectively works. The port keeps user filter selections: filters change
//! only via filter messages + song-load preselect, and every filter/codec
//! change recomputes the body under the current selections. Prefs close and
//! song-settings encoding loads likewise recompute without resetting.

use std::collections::BTreeMap;

use crate::core::event::TextType;
use crate::core::sequence::{Sequence, TextRec};

// ---------------------------------------------------------------------------
// Dropdown option contracts (ux §3.6; T11b renders these)
// ---------------------------------------------------------------------------

/// Track dropdown index 0 (`window.cpp:557`).
pub const TRACK_ALL_LABEL: &str = "All tracks";
/// Type dropdown labels, index = `TextType` discriminant (`kTypeNames`,
/// `window.cpp:64-66`, verbatim incl. "Cue point" / "KAR other").
pub const TYPE_OPTION_NAMES: [&str; 14] = [
    "All types",
    "Text",
    "Copyright",
    "Track name",
    "Instrument",
    "Lyrics",
    "Marker",
    "Cue point",
    "KAR type",
    "KAR version",
    "KAR info",
    "KAR language",
    "KAR titles",
    "KAR other",
];

/// Per-track display names recomputed from the text records (C++
/// `m_trkName`, `sequence.cpp:479-487`): `TrackName`/`InstrumentName`
/// payloads — matched on the STORED (post-`@` remap) type exactly like the
/// C++ `else if` — decoded with `codec` and joined with spaces. No track
/// with no name records appears in the map.
pub fn track_names(texts: &[TextRec], codec: &str) -> BTreeMap<i32, String> {
    let mut names: BTreeMap<i32, String> = BTreeMap::new();
    for rec in texts {
        if rec.text_type != TextType::TrackName && rec.text_type != TextType::InstrumentName {
            continue;
        }
        let decoded = Sequence::decode_with_charset(&rec.text, codec);
        names
            .entry(rec.track)
            .and_modify(|n| {
                n.push(' ');
                n.push_str(&decoded);
            })
            .or_insert(decoded);
    }
    names
}

/// Track dropdown labels: `All tracks` + `Track N[ — <name>]` for
/// `1..=num_tracks` (`window.cpp:556-564`, em-dash verbatim). The selected
/// index is the track number itself (`track_filter`, `None` = 0).
pub fn track_options(texts: &[TextRec], num_tracks: i32, codec: &str) -> Vec<String> {
    let mut out = vec![TRACK_ALL_LABEL.to_string()];
    if num_tracks <= 0 {
        return out;
    }
    let names = track_names(texts, codec);
    for track in 1..=num_tracks {
        let mut label = format!("Track {track}");
        if let Some(name) = names.get(&track)
            && !name.is_empty()
        {
            label.push_str(" — ");
            label.push_str(name);
        }
        out.push(label);
    }
    out
}

/// Encoding dropdown selection for `codec`: case-insensitive match in
/// [`Sequence::extra_codec_names`], else index 0 while decoding still uses
/// `codec` (off-list rule, `window.cpp:575-585`).
pub fn codec_option_index(codec: &str) -> usize {
    Sequence::extra_codec_names()
        .iter()
        .position(|n| n.eq_ignore_ascii_case(codec))
        .unwrap_or(0)
}

/// Whether a payload is a KAR `@`-header (`sequence.cpp:446-468`): length >
/// 1, `@` first, `K/V/I/L/T/W` second. The stored type of such a record was
/// remapped to `Kar*`; the scored (raw) type is unknowable from the record.
fn is_kar_header(data: &[u8]) -> bool {
    data.len() > 1 && data[0] == b'@' && matches!(data[1], b'K' | b'V' | b'I' | b'L' | b'T' | b'W')
}

/// Raw-type score key for one record (C++ `m_typScore` key): the stored
/// discriminant, except KAR `@`-headers, which C++ scores under their raw
/// meta type (always `Text` = 1 in real `.kar` files, where headers are
/// `0x01` text events) before remapping. Exact for every real file; a
/// hypothetical `@`-header under a non-`Text` raw type would miscount (the
/// raw type is not stored) — negligible, no such files observed.
fn raw_type_for_score(rec: &TextRec) -> i32 {
    if is_kar_header(&rec.text) {
        TextType::Text as i32
    } else {
        rec.text_type as i32
    }
}

/// Best-score preselects (`trackMaxPoints`/`typeMaxPoints`,
/// `sequence.cpp:290-312`): most text hits wins, ties resolve to the lowest
/// key (ordered map + strict `>`, as in C++). Track `best <= 0` (incl. the
/// empty `-1`) means all-tracks (`window.cpp:568`); type `-1` (no texts)
/// falls back to `Lyric` = 5 (`window.cpp:569-571`).
pub fn preselect_filters(texts: &[TextRec]) -> (Option<u32>, i32) {
    fn best(counts: &BTreeMap<i32, i32>) -> i32 {
        let (mut key, mut val) = (-1, -1);
        for (&k, &v) in counts {
            if v > val {
                val = v;
                key = k;
            }
        }
        key
    }
    let mut tracks: BTreeMap<i32, i32> = BTreeMap::new();
    let mut types: BTreeMap<i32, i32> = BTreeMap::new();
    for rec in texts {
        // Track counts recomputed from the records are EXACT: C++ scores
        // every non-empty `addMetaData` call and pushes exactly one record
        // per scored call (`sequence.cpp:440-470`).
        *tracks.entry(rec.track).or_insert(0) += 1;
        *types.entry(raw_type_for_score(rec)).or_insert(0) += 1;
    }
    let track = best(&tracks);
    let mut typ = best(&types);
    if typ < 0 {
        typ = TextType::Lyric as i32;
    }
    (if track > 0 { Some(track as u32) } else { None }, typ)
}

// ---------------------------------------------------------------------------
// Lyrics state
// ---------------------------------------------------------------------------

/// Lyrics page state (arch §2.1 `LyricsState`).
#[allow(dead_code)] // T11b renders body/live segments/applied/autoscroll.
#[derive(Debug, Clone)]
pub struct LyricsState {
    /// Track dropdown: `None` = all tracks, else 1-based track number.
    pub track_filter: Option<u32>,
    /// Type dropdown: 0 = all types, else `TextType` discriminant.
    pub type_filter: i32,
    /// Encoding dropdown: active decode label (starts at the song's detected
    /// charset, overridden by song settings or the dropdown).
    pub codec: String,
    /// Filtered static text (`refreshLyrics` body half).
    pub body: String,
    /// Earlier live appends (past color). Together with [`Self::live_current`]
    /// this is the `onText` tail; U10 quirk: sung lines appear here AND in
    /// the prefilled [`Self::body`].
    pub live_past: String,
    /// Newest live append (current/highlight color).
    pub live_current: String,
    /// Whether any live event passed the filters yet: flips [`Self::body`]
    /// from the future color to the past color (C++ applies `past` to the
    /// whole buffer on the first `onText`, `window.cpp:164-167`).
    pub sung: bool,
    /// Autoscroll-to-latest on live append (C++ always scrolls,
    /// `window.cpp:177`; there is no C++ toggle surface, so this stays
    /// `true` — retained for the T11b view, not user-settable).
    pub autoscroll: bool,
    /// Live-append generation, bumped per accepted event so the T11b view can
    /// snap-scroll on change. Monotonic within a song; reset on song load.
    pub live_seq: u64,
    /// Applied D2 Lyrics rows (U14 pathway): colors/font/alignment derived
    /// from settings by [`AppliedLyricsPrefs::from_rows`].
    pub applied: AppliedLyricsPrefs,
}

impl Default for LyricsState {
    fn default() -> Self {
        Self {
            track_filter: None,
            // C++ ultimate fallback when no type scores (`window.cpp:569-571`).
            type_filter: TextType::Lyric as i32,
            codec: String::new(),
            body: String::new(),
            live_past: String::new(),
            live_current: String::new(),
            sung: false,
            autoscroll: true,
            live_seq: 0,
            applied: AppliedLyricsPrefs::default(),
        }
    }
}

impl LyricsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset on song load: codec starts at the detected charset, filters to
    /// the best-score preselects, body rebuilt, live tail cleared
    /// (`refreshLyrics` on `loadCurrent`, `window.cpp:396`).
    pub fn reset_for_song(&mut self, charset: String, texts: &[TextRec]) {
        let (track, typ) = preselect_filters(texts);
        self.track_filter = track;
        self.type_filter = typ;
        self.codec = charset;
        self.live_seq = 0;
        self.refresh_body(texts);
    }

    /// Rebuild the static body under the current filters + codec and clear
    /// the live segments (C++ `refreshLyrics` clears the whole buffer incl.
    /// live appends, `window.cpp:588`). Filter selections are KEPT (snap-back
    /// FIX, see the module docs).
    pub fn refresh_body(&mut self, texts: &[TextRec]) {
        let mut body = String::new();
        for rec in texts {
            // `window.cpp:594`: `track > 0 && rec.track != track` skips.
            if let Some(want) = self.track_filter
                && i64::from(rec.track) != i64::from(want)
            {
                continue;
            }
            // `window.cpp:596`: `type > 0 && rec.type != type` skips.
            if self.type_filter > 0 && rec.text_type as i32 != self.type_filter {
                continue;
            }
            let s = Sequence::decode_with_charset(&rec.text, &self.codec);
            body.push_str(&s);
            // `window.cpp:600-601`: non-lyric records get a trailing newline
            // unless they already end in one.
            if !s.is_empty() && !s.ends_with('\n') && rec.text_type != TextType::Lyric {
                body.push('\n');
            }
        }
        self.body = body;
        self.clear_live();
    }

    /// Clear the live segments (`live_seq` untouched: it is a monotonic
    /// generation, and a clear is not an append the view must snap to).
    pub fn clear_live(&mut self) {
        self.live_past.clear();
        self.live_current.clear();
        self.sung = false;
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
    /// anything was appended. Exact `onText` port (`window.cpp:163-178`):
    /// the previous `current` becomes `past`, the new text becomes
    /// `current` — no trailing-newline rule on the live path (unlike the
    /// static prefill). Static exports retain their original body once.
    pub fn append_live(&mut self, track: u32, text_type: i32, text: &str) -> bool {
        if !self.passes_filter(track, text_type) {
            return false;
        }
        self.live_past.push_str(&self.live_current);
        self.live_current = text.to_owned();
        self.sung = true;
        self.live_seq += 1;
        true
    }

    /// Full live tail: earlier appends + the newest one.
    pub fn live_tail(&self) -> String {
        format!("{}{}", self.live_past, self.live_current)
    }

    /// Whole lyrics text without duplicating the static prefill during playback.
    pub fn full_text(&self) -> String {
        if self.body.is_empty() {
            self.live_tail()
        } else {
            self.body.clone()
        }
    }
}

// ---------------------------------------------------------------------------
// `gdk_rgba_parse` parity (arch §2.6 color spec, T0 empirical probes P1/P2/P5)
// ---------------------------------------------------------------------------

/// Parsed color, 0.0..=1.0 per channel (T11b maps this onto `iced::Color`
/// field-for-field: `Color { r, g, b, a }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

/// Gray fallback: `gdk_rgba_parse` failure leaves the value untouched, so the
/// `(0.5, 0.5, 0.5, 1.0)` init in `parseColor` survives (`window.cpp:39-44`).
/// NOTE: this is NOT `#888888` (0.5333) — that is the `futureColor` setting
/// default, a different gray.
pub const GRAY_FALLBACK: Rgba = Rgba {
    r: 0.5,
    g: 0.5,
    b: 0.5,
    a: 1.0,
};

/// Parse a `futureColor`/`pastColor`/`highlightColor` row like
/// `gdk_rgba_parse`: `#3/#4/#6/#8` hex, lowercase-only comma-separated
/// `rgb()/rgba()/hsl()/hsla()`, or a case-insensitive CSS name. `None` means
/// "untouched" — the caller keeps [`GRAY_FALLBACK`]. There is NO outer trim
/// and NO `transparent` (arch §2.6).
pub fn parse_gdk_rgba(input: &str) -> Option<Rgba> {
    if let Some(hex) = input.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    for (name, channels) in [("rgba(", 4), ("rgb(", 3), ("hsla(", 4), ("hsl(", 3)] {
        if let Some(inner) = input
            .strip_prefix(name)
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return parse_function_color(name, inner, channels);
        }
    }
    named_color(input)
}

/// `#3/#4/#6/#8` only: short nibbles ×17, long bytes as-is, all /255; alpha
/// defaults to 1.0; case-insensitive.
fn parse_hex_color(hex: &str) -> Option<Rgba> {
    let bytes: Vec<u8> = match hex.len() {
        3 | 4 => {
            let mut out = Vec::with_capacity(hex.len());
            for b in hex.bytes() {
                let nibble = (b as char).to_digit(16)? as f32;
                out.push((nibble * 17.0) as u8);
            }
            out
        }
        6 | 8 => {
            if !hex.bytes().all(|b| (b as char).is_ascii_hexdigit()) {
                return None;
            }
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                .collect::<Option<Vec<u8>>>()?
        }
        _ => return None,
    };
    let f = |i: usize| f32::from(bytes[i]) / 255.0;
    Some(match bytes.len() {
        3 => Rgba {
            r: f(0),
            g: f(1),
            b: f(2),
            a: 1.0,
        },
        4 => Rgba {
            r: f(0),
            g: f(1),
            b: f(2),
            a: f(3),
        },
        6 => Rgba {
            r: f(0),
            g: f(1),
            b: f(2),
            a: 1.0,
        },
        _ => Rgba {
            r: f(0),
            g: f(1),
            b: f(2),
            a: f(3),
        },
    })
}

/// `rgb()/rgba()/hsl()/hsla()` arm: comma-separated (exactly `channels`
/// parts), inner spaces tolerated, every component clamped. Channels accept
/// numbers or `%`; the alpha accepts numbers only (`%`-alpha fails, like
/// `rgba()` — the `hsla()` rule mirrors it by analogy, unprobed).
fn parse_function_color(name: &str, inner: &str, channels: usize) -> Option<Rgba> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != channels {
        return None;
    }
    let is_hsl = name.starts_with("hsl");
    if is_hsl {
        let h = parts[0].parse::<f32>().ok()?;
        let s = parse_percent(parts[1])?;
        let l = parse_percent(parts[2])?;
        let a = if channels == 4 {
            parse_number(parts[3])?
        } else {
            1.0
        };
        let (r, g, b) = hsl_to_rgb(h, s, l);
        return Some(Rgba { r, g, b, a });
    }
    let r = parse_channel(parts[0])?;
    let g = parse_channel(parts[1])?;
    let b = parse_channel(parts[2])?;
    let a = if channels == 4 {
        parse_number(parts[3])?
    } else {
        1.0
    };
    Some(Rgba { r, g, b, a })
}

/// Clamp to 0.0..=1.0; non-finite inputs fail (hardening: a NaN channel
/// would poison rendering, and no spec row covers `inf`/`nan`).
fn clamp01(v: f32) -> Option<f32> {
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

/// One `rgb()` channel: bare 0..=255 number or `0%..=100%`, clamped.
fn parse_channel(part: &str) -> Option<f32> {
    if let Some(pct) = part.strip_suffix('%') {
        return clamp01(pct.parse::<f32>().ok()? / 100.0);
    }
    clamp01(part.parse::<f32>().ok()? / 255.0)
}

/// Bare number clamped to 0.0..=1.0 (alpha arm; a `%` suffix fails here).
fn parse_number(part: &str) -> Option<f32> {
    if part.ends_with('%') {
        return None;
    }
    clamp01(part.parse::<f32>().ok()?)
}

/// `hsl()` saturation/lightness: `%` required (CSS-strict; bare numbers are
/// unprobed, so they fail), clamped.
fn parse_percent(part: &str) -> Option<f32> {
    let pct = part.strip_suffix('%')?;
    clamp01(pct.parse::<f32>().ok()? / 100.0)
}

/// `hsl()` → rgb (hue wraps mod 360, CSS behavior).
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

/// Case-insensitive CSS named colors (full table; NO `transparent`).
fn named_color(input: &str) -> Option<Rgba> {
    let (r, g, b) = CSS_NAMED_COLORS
        .iter()
        .find(|(name, _, _, _)| name.eq_ignore_ascii_case(input))
        .map(|(_, r, g, b)| (*r, *g, *b))?;
    Some(Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    })
}

/// Full CSS named-color table (`gdk_rgba_parse` accepts the whole table;
/// `transparent` is deliberately absent — it fails, arch §2.6). Matched
/// case-insensitively by [`named_color`].
const CSS_NAMED_COLORS: [(&str, u8, u8, u8); 148] = [
    ("aliceblue", 0xF0, 0xF8, 0xFF),
    ("antiquewhite", 0xFA, 0xEB, 0xD7),
    ("aqua", 0x00, 0xFF, 0xFF),
    ("aquamarine", 0x7F, 0xFF, 0xD4),
    ("azure", 0xF0, 0xFF, 0xFF),
    ("beige", 0xF5, 0xF5, 0xDC),
    ("bisque", 0xFF, 0xE4, 0xC4),
    ("black", 0x00, 0x00, 0x00),
    ("blanchedalmond", 0xFF, 0xEB, 0xCD),
    ("blue", 0x00, 0x00, 0xFF),
    ("blueviolet", 0x8A, 0x2B, 0xE2),
    ("brown", 0xA5, 0x2A, 0x2A),
    ("burlywood", 0xDE, 0xB8, 0x87),
    ("cadetblue", 0x5F, 0x9E, 0xA0),
    ("chartreuse", 0x7F, 0xFF, 0x00),
    ("chocolate", 0xD2, 0x69, 0x1E),
    ("coral", 0xFF, 0x7F, 0x50),
    ("cornflowerblue", 0x64, 0x95, 0xED),
    ("cornsilk", 0xFF, 0xF8, 0xDC),
    ("crimson", 0xDC, 0x14, 0x3C),
    ("cyan", 0x00, 0xFF, 0xFF),
    ("darkblue", 0x00, 0x00, 0x8B),
    ("darkcyan", 0x00, 0x8B, 0x8B),
    ("darkgoldenrod", 0xB8, 0x86, 0x0B),
    ("darkgray", 0xA9, 0xA9, 0xA9),
    ("darkgreen", 0x00, 0x64, 0x00),
    ("darkgrey", 0xA9, 0xA9, 0xA9),
    ("darkkhaki", 0xBD, 0xB7, 0x6B),
    ("darkmagenta", 0x8B, 0x00, 0x8B),
    ("darkolivegreen", 0x55, 0x6B, 0x2F),
    ("darkorange", 0xFF, 0x8C, 0x00),
    ("darkorchid", 0x99, 0x32, 0xCC),
    ("darkred", 0x8B, 0x00, 0x00),
    ("darksalmon", 0xE9, 0x96, 0x7A),
    ("darkseagreen", 0x8F, 0xBC, 0x8F),
    ("darkslateblue", 0x48, 0x3D, 0x8B),
    ("darkslategray", 0x2F, 0x4F, 0x4F),
    ("darkslategrey", 0x2F, 0x4F, 0x4F),
    ("darkturquoise", 0x00, 0xCE, 0xD1),
    ("darkviolet", 0x94, 0x00, 0xD3),
    ("deeppink", 0xFF, 0x14, 0x93),
    ("deepskyblue", 0x00, 0xBF, 0xFF),
    ("dimgray", 0x69, 0x69, 0x69),
    ("dimgrey", 0x69, 0x69, 0x69),
    ("dodgerblue", 0x1E, 0x90, 0xFF),
    ("firebrick", 0xB2, 0x22, 0x22),
    ("floralwhite", 0xFF, 0xFA, 0xF0),
    ("forestgreen", 0x22, 0x8B, 0x22),
    ("fuchsia", 0xFF, 0x00, 0xFF),
    ("gainsboro", 0xDC, 0xDC, 0xDC),
    ("ghostwhite", 0xF8, 0xF8, 0xFF),
    ("gold", 0xFF, 0xD7, 0x00),
    ("goldenrod", 0xDA, 0xA5, 0x20),
    ("gray", 0x80, 0x80, 0x80),
    ("green", 0x00, 0x80, 0x00),
    ("greenyellow", 0xAD, 0xFF, 0x2F),
    ("grey", 0x80, 0x80, 0x80),
    ("honeydew", 0xF0, 0xFF, 0xF0),
    ("hotpink", 0xFF, 0x69, 0xB4),
    ("indianred", 0xCD, 0x5C, 0x5C),
    ("indigo", 0x4B, 0x00, 0x82),
    ("ivory", 0xFF, 0xFF, 0xF0),
    ("khaki", 0xF0, 0xE6, 0x8C),
    ("lavender", 0xE6, 0xE6, 0xFA),
    ("lavenderblush", 0xFF, 0xF0, 0xF5),
    ("lawngreen", 0x7C, 0xFC, 0x00),
    ("lemonchiffon", 0xFF, 0xFA, 0xCD),
    ("lightblue", 0xAD, 0xD8, 0xE6),
    ("lightcoral", 0xF0, 0x80, 0x80),
    ("lightcyan", 0xE0, 0xFF, 0xFF),
    ("lightgoldenrodyellow", 0xFA, 0xFA, 0xD2),
    ("lightgray", 0xD3, 0xD3, 0xD3),
    ("lightgreen", 0x90, 0xEE, 0x90),
    ("lightgrey", 0xD3, 0xD3, 0xD3),
    ("lightpink", 0xFF, 0xB6, 0xC1),
    ("lightsalmon", 0xFF, 0xA0, 0x7A),
    ("lightseagreen", 0x20, 0xB2, 0xAA),
    ("lightskyblue", 0x87, 0xCE, 0xFA),
    ("lightslategray", 0x77, 0x88, 0x99),
    ("lightslategrey", 0x77, 0x88, 0x99),
    ("lightsteelblue", 0xB0, 0xC4, 0xDE),
    ("lightyellow", 0xFF, 0xFF, 0xE0),
    ("lime", 0x00, 0xFF, 0x00),
    ("limegreen", 0x32, 0xCD, 0x32),
    ("linen", 0xFA, 0xF0, 0xE6),
    ("magenta", 0xFF, 0x00, 0xFF),
    ("maroon", 0x80, 0x00, 0x00),
    ("mediumaquamarine", 0x66, 0xCD, 0xAA),
    ("mediumblue", 0x00, 0x00, 0xCD),
    ("mediumorchid", 0xBA, 0x55, 0xD3),
    ("mediumpurple", 0x93, 0x70, 0xDB),
    ("mediumseagreen", 0x3C, 0xB3, 0x71),
    ("mediumslateblue", 0x7B, 0x68, 0xEE),
    ("mediumspringgreen", 0x00, 0xFA, 0x9A),
    ("mediumturquoise", 0x48, 0xD1, 0xCC),
    ("mediumvioletred", 0xC7, 0x15, 0x85),
    ("midnightblue", 0x19, 0x19, 0x70),
    ("mintcream", 0xF5, 0xFF, 0xFA),
    ("mistyrose", 0xFF, 0xE4, 0xE1),
    ("moccasin", 0xFF, 0xE4, 0xB5),
    ("navajowhite", 0xFF, 0xDE, 0xAD),
    ("navy", 0x00, 0x00, 0x80),
    ("oldlace", 0xFD, 0xF5, 0xE6),
    ("olive", 0x80, 0x80, 0x00),
    ("olivedrab", 0x6B, 0x8E, 0x23),
    ("orange", 0xFF, 0xA5, 0x00),
    ("orangered", 0xFF, 0x45, 0x00),
    ("orchid", 0xDA, 0x70, 0xD6),
    ("palegoldenrod", 0xEE, 0xE8, 0xAA),
    ("palegreen", 0x98, 0xFB, 0x98),
    ("paleturquoise", 0xAF, 0xEE, 0xEE),
    ("palevioletred", 0xDB, 0x70, 0x93),
    ("papayawhip", 0xFF, 0xEF, 0xD5),
    ("peachpuff", 0xFF, 0xDA, 0xB9),
    ("peru", 0xCD, 0x85, 0x3F),
    ("pink", 0xFF, 0xC0, 0xCB),
    ("plum", 0xDD, 0xA0, 0xDD),
    ("powderblue", 0xB0, 0xE0, 0xE6),
    ("purple", 0x80, 0x00, 0x80),
    ("rebeccapurple", 0x66, 0x33, 0x99),
    ("red", 0xFF, 0x00, 0x00),
    ("rosybrown", 0xBC, 0x8F, 0x8F),
    ("royalblue", 0x41, 0x69, 0xE1),
    ("saddlebrown", 0x8B, 0x45, 0x13),
    ("salmon", 0xFA, 0x80, 0x72),
    ("sandybrown", 0xF4, 0xA4, 0x60),
    ("seagreen", 0x2E, 0x8B, 0x57),
    ("seashell", 0xFF, 0xF5, 0xEE),
    ("sienna", 0xA0, 0x52, 0x2D),
    ("silver", 0xC0, 0xC0, 0xC0),
    ("skyblue", 0x87, 0xCE, 0xEB),
    ("slateblue", 0x6A, 0x5A, 0xCD),
    ("slategray", 0x70, 0x80, 0x90),
    ("slategrey", 0x70, 0x80, 0x90),
    ("snow", 0xFF, 0xFA, 0xFA),
    ("springgreen", 0x00, 0xFF, 0x7F),
    ("steelblue", 0x46, 0x82, 0xB4),
    ("tan", 0xD2, 0xB4, 0x8C),
    ("teal", 0x00, 0x80, 0x80),
    ("thistle", 0xD8, 0xBF, 0xD8),
    ("tomato", 0xFF, 0x63, 0x47),
    ("turquoise", 0x40, 0xE0, 0xD0),
    ("violet", 0xEE, 0x82, 0xEE),
    ("wheat", 0xF5, 0xDE, 0xB3),
    ("white", 0xFF, 0xFF, 0xFF),
    ("whitesmoke", 0xF5, 0xF5, 0xF5),
    ("yellow", 0xFF, 0xFF, 0x00),
    ("yellowgreen", 0x9A, 0xCD, 0x32),
];

/// D17 picker→text canonical form: `#rrggbb`, or `#rrggbbaa` when the color
/// carries transparency. Binding shape (T11b): the text row stays source of
/// truth — `parse_gdk_rgba` seeds/validates the `ColorPickerModel`, and an
/// applied picker color is written back through this function.
pub fn rgba_to_hex_string(c: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if c.a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            byte(c.r),
            byte(c.g),
            byte(c.b),
            byte(c.a)
        )
    }
}

// ---------------------------------------------------------------------------
// Pango font-string spec (arch §2.6 font spec, T0 probes P1/P3/P4; D15 FIX)
// ---------------------------------------------------------------------------

/// Default lyrics size in pt when the string carries no size: the effective
/// C++ default, i.e. the `16pt` `.lyrics-view` rule that survives because the
/// `font: <lyricsFont>` provider string is invalid CSS (arch §2.6, D15).
pub const DEFAULT_LYRICS_SIZE_PT: f32 = 16.0;
/// Fallback family when the string yields none (bare number / empty).
pub const FALLBACK_FONT_FAMILY: &str = "Sans";

/// Parsed font size: points (trailing float) or absolute pixels (`Npx`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    Pt(f32),
    Px(f32),
}

/// Parsed font style (T11b maps onto `iced::font::Style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

/// Parsed font stretch (T11b maps onto `iced::font::Stretch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

/// Parsed `lyricsFont` row (`pango_font_description_from_string` semantics).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFont {
    /// Family remainder verbatim (comma-normalized: entries trimmed and
    /// rejoined with `,`), or [`FALLBACK_FONT_FAMILY`] when empty.
    pub family: String,
    /// Trailing size, if any (`None` = default size).
    pub size: Option<FontSize>,
    /// Weight, 400 default (`Semi-Bold` 600 / `Bold` 700 / `Ultra-Bold` 800…).
    pub weight: u16,
    /// Style, `Normal` default.
    pub style: FontStyle,
    /// Stretch, if any (`None` = normal/unspecified).
    pub stretch: Option<FontStretch>,
    /// `Small-Caps` variant flag. Parsed for spec completeness; iced
    /// `Font` has no variant mapping, so T11b renders it as normal text.
    pub small_caps: bool,
}

impl ParsedFont {
    /// First comma-separated family entry: the concrete typeface to resolve
    /// (arch §2.6: "the first comma-separated entry wins").
    pub fn first_family(&self) -> &str {
        self.family
            .split(',')
            .next()
            .unwrap_or(FALLBACK_FONT_FAMILY)
    }
}

/// Resolve a parsed size to iced logical px: `pt × 96/72`, absolute px
/// as-is, or `default_pt × 96/72` when the string carries no size.
pub fn font_size_px(size: Option<FontSize>, default_pt: f32) -> f32 {
    match size {
        Some(FontSize::Pt(pt)) => pt * 96.0 / 72.0,
        Some(FontSize::Px(px)) => px,
        None => default_pt * 96.0 / 72.0,
    }
}

/// Parse a Pango `<family> [attributes] [size]` string (arch §2.6): outer
/// whitespace tolerated; trailing finite float ≥ 0 is the size in pt — `0`
/// is consumed but means "no size", negatives stay family text; an `Npx`
/// suffix (case-insensitive) is an absolute px size while `Npt` is literal
/// family text; then keywords (case-insensitive, closest-to-size wins per
/// category) for style/weight/stretch/variant; the remainder is the family.
/// Numeric weights (`Sans 600 12`) are NOT keywords — family text.
pub fn parse_pango_font(input: &str) -> ParsedFont {
    let mut tokens: Vec<&str> = input.split_whitespace().collect();
    // Trailing size probe (consumes the token unless it is family text).
    let mut size: Option<FontSize> = None;
    if let Some(last) = tokens.last() {
        match split_font_size(last) {
            FontSizeProbe::Sized(s) => {
                size = Some(s);
                tokens.pop();
            }
            // `ConsumedNone` (bare `0`) eats the token but leaves no size;
            // `FamilyText` (negatives, `12pt`, words) keeps it.
            FontSizeProbe::ConsumedNone => {
                tokens.pop();
            }
            FontSizeProbe::FamilyText => {}
        }
    }
    // Keywords from the end (closest-to-size wins per category).
    let mut weight = 400u16;
    let mut style = FontStyle::Normal;
    let mut stretch: Option<FontStretch> = None;
    let mut small_caps = false;
    let mut seen_weight = false;
    let mut seen_style = false;
    let mut seen_stretch = false;
    while let Some(last) = tokens.last() {
        let word = last.to_ascii_lowercase();
        if !seen_style
            && let Some(s) = match word.as_str() {
                "italic" => Some(FontStyle::Italic),
                "oblique" => Some(FontStyle::Oblique),
                "normal" | "regular" | "upright" | "roman" => Some(FontStyle::Normal),
                _ => None,
            }
        {
            style = s;
            seen_style = true;
            tokens.pop();
            continue;
        }
        if !seen_weight
            && let Some(w) = match word.as_str() {
                "thin" => Some(100),
                "ultra-light" | "ultralight" | "extra-light" | "extralight" => Some(200),
                "light" => Some(300),
                "semi-light" | "semilight" | "demi-light" | "demilight" => Some(350),
                "book" => Some(380),
                "medium" => Some(500),
                "semi-bold" | "semibold" | "demi-bold" | "demibold" => Some(600),
                "bold" => Some(700),
                "ultra-bold" | "ultrabold" | "extra-bold" | "extrabold" => Some(800),
                "black" | "heavy" => Some(900),
                "ultra-black" | "ultrablack" | "extra-black" | "extrablack" => Some(1000),
                _ => None,
            }
        {
            weight = w;
            seen_weight = true;
            tokens.pop();
            continue;
        }
        if !seen_stretch
            && let Some(s) = match word.as_str() {
                "ultra-condensed" => Some(FontStretch::UltraCondensed),
                "extra-condensed" => Some(FontStretch::ExtraCondensed),
                "condensed" => Some(FontStretch::Condensed),
                "semi-condensed" => Some(FontStretch::SemiCondensed),
                "semi-expanded" => Some(FontStretch::SemiExpanded),
                "expanded" => Some(FontStretch::Expanded),
                "extra-expanded" => Some(FontStretch::ExtraExpanded),
                "ultra-expanded" => Some(FontStretch::UltraExpanded),
                _ => None,
            }
        {
            stretch = Some(s);
            seen_stretch = true;
            tokens.pop();
            continue;
        }
        if !small_caps && word == "small-caps" {
            small_caps = true;
            tokens.pop();
            continue;
        }
        break;
    }
    // Family = remainder verbatim, comma-normalized (entries trimmed,
    // empties dropped, rejoined with `,`); empty → `Sans` fallback.
    let family: String = tokens
        .join(" ")
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    ParsedFont {
        family: if family.is_empty() {
            FALLBACK_FONT_FAMILY.to_string()
        } else {
            family
        },
        size,
        weight,
        style,
        stretch,
        small_caps,
    }
}

/// Trailing-token size probe outcome.
enum FontSizeProbe {
    /// Real size (`16` → pt, `12px` → px).
    Sized(FontSize),
    /// Bare `0` (or `0px`): token consumed, still no size.
    ConsumedNone,
    /// Negatives, `12pt`, words, non-finite: family text.
    FamilyText,
}

/// Strip a case-insensitive `px` suffix (boundary-safe, unlike byte
/// slicing, for non-ASCII tokens).
fn strip_px_suffix(token: &str) -> Option<&str> {
    ["px", "pX", "Px", "PX"]
        .iter()
        .find_map(|suffix| token.strip_suffix(suffix))
}

/// Classify the trailing token: `Npx` (case-insensitive suffix) → absolute
/// px; finite float ≥ 0 → pt (`0` → consumed, no size); anything else —
/// negatives, `12pt`, `inf`/`nan`, words — is family text.
fn split_font_size(token: &str) -> FontSizeProbe {
    if let Some(num) = strip_px_suffix(token) {
        return match num.parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => FontSizeProbe::Sized(FontSize::Px(v as f32)),
            Ok(0.0) => FontSizeProbe::ConsumedNone,
            _ => FontSizeProbe::FamilyText,
        };
    }
    match token.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => FontSizeProbe::Sized(FontSize::Pt(v as f32)),
        Ok(0.0) => FontSizeProbe::ConsumedNone,
        _ => FontSizeProbe::FamilyText,
    }
}

// ---------------------------------------------------------------------------
// Alignment + applied prefs (U14 pathway)
// ---------------------------------------------------------------------------

/// Parsed `textAlignment` row (T11b maps onto `iced::alignment::Horizontal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LyricAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// `alignmentFromSetting` verbatim (`window.cpp:55-62`): 1 → center,
/// 2 → right, anything else (incl. out-of-range/negative) → left.
pub fn parse_alignment(a: i32) -> LyricAlign {
    if a == 1 {
        LyricAlign::Center
    } else if a == 2 {
        LyricAlign::Right
    } else {
        LyricAlign::Left
    }
}

/// Applied D2 Lyrics page rows: colors (parse failures → [`GRAY_FALLBACK`]),
/// font, alignment. Built by `App::apply_lyrics_prefs` at startup and on
/// every prefs commit (U14 pathway); T11b renders from this, never parsing
/// settings strings per frame. (`singleColor` is a Piano row — T12, not here.)
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedLyricsPrefs {
    pub future: Rgba,
    pub past: Rgba,
    pub highlight: Rgba,
    pub font: ParsedFont,
    pub alignment: LyricAlign,
}

impl AppliedLyricsPrefs {
    pub fn from_rows(
        future: &str,
        past: &str,
        highlight: &str,
        font: &str,
        alignment: i32,
    ) -> Self {
        Self {
            future: parse_gdk_rgba(future).unwrap_or(GRAY_FALLBACK),
            past: parse_gdk_rgba(past).unwrap_or(GRAY_FALLBACK),
            highlight: parse_gdk_rgba(highlight).unwrap_or(GRAY_FALLBACK),
            font: parse_pango_font(font),
            alignment: parse_alignment(alignment),
        }
    }
}

impl Default for AppliedLyricsPrefs {
    /// Applied form of the stock settings defaults (`settings.hpp`).
    fn default() -> Self {
        Self::from_rows("#888888", "#1c71d8", "#e01b24", "Sans 16", 0)
    }
}

/// KAR lyric syllables are joined; slash starts a line and backslash a verse.
/// Other metadata keeps the usual one-record-per-line presentation.
pub fn display_record(record: &TextRec, codec: &str, kar: bool) -> String {
    let mut text = Sequence::decode_with_charset(&record.text, codec);
    if kar && matches!(record.text_type, TextType::Text | TextType::Lyric) {
        return text.replace('\\', "\n\n").replace('/', "\n");
    }
    if !text.is_empty() && !text.ends_with('\n') && record.text_type != TextType::Lyric {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(track: i32, text_type: TextType, text: &[u8]) -> TextRec {
        TextRec {
            tick: 0,
            track,
            text_type,
            text: text.to_vec(),
        }
    }

    #[test]
    fn defaults_match_cpp_fallbacks() {
        let l = LyricsState::new();
        assert_eq!(l.track_filter, None);
        assert_eq!(l.type_filter, TextType::Lyric as i32);
        assert!(l.codec.is_empty());
        assert!(l.body.is_empty());
        assert!(l.live_tail().is_empty());
        assert!(!l.sung);
        assert!(l.autoscroll);
        assert_eq!(l.live_seq, 0);
        assert_eq!(l.applied, AppliedLyricsPrefs::default());
    }

    #[test]
    fn reset_for_song_preselects_and_rebuilds() {
        let texts = vec![
            rec(2, TextType::Lyric, b"la "),
            rec(2, TextType::Lyric, b"la"),
            rec(1, TextType::Text, b"note"),
        ];
        let mut l = LyricsState::new();
        l.track_filter = Some(9);
        l.type_filter = 1;
        l.live_past = "stale".to_string();
        l.live_seq = 41;
        l.reset_for_song("WINDOWS-1252".to_string(), &texts);
        // Track 2 has the most hits; Lyric wins the type race 2:1.
        assert_eq!(l.track_filter, Some(2));
        assert_eq!(l.type_filter, TextType::Lyric as i32);
        assert_eq!(l.codec, "WINDOWS-1252");
        assert_eq!(l.body, "la la");
        assert!(l.live_tail().is_empty());
        assert!(!l.sung);
        assert_eq!(l.live_seq, 0);
    }

    #[test]
    fn refresh_body_filters_track_type_and_newline_rule() {
        let texts = vec![
            rec(1, TextType::Lyric, b"la "),
            rec(1, TextType::Text, b"spoken"),
            rec(1, TextType::Marker, b"verse\n"),
            rec(2, TextType::Lyric, b"other"),
        ];
        let mut l = LyricsState::new();
        l.codec = "UTF-8".to_string();
        // All tracks, all types: lyrics concatenate raw, non-lyrics gain a
        // trailing newline unless they already end in one (`window.cpp:600`).
        l.type_filter = 0;
        l.refresh_body(&texts);
        assert_eq!(l.body, "la spoken\nverse\nother");
        // Track filter narrows; type filter narrows to lyrics only.
        l.track_filter = Some(2);
        l.refresh_body(&texts);
        assert_eq!(l.body, "other");
        l.track_filter = None;
        l.type_filter = TextType::Lyric as i32;
        l.refresh_body(&texts);
        assert_eq!(l.body, "la other");
        // Out-of-range type matches nothing (C++ casts without clamping).
        l.type_filter = 99;
        l.refresh_body(&texts);
        assert!(l.body.is_empty());
    }

    #[test]
    fn refresh_body_decodes_with_codec() {
        // あ U+3042: 0x82 0xA0 in SHIFT_JIS, invalid in UTF-8.
        let texts = vec![rec(1, TextType::Lyric, &[0x82, 0xA0])];
        let mut l = LyricsState::new();
        l.codec = "SHIFT_JIS".to_string();
        l.refresh_body(&texts);
        assert_eq!(l.body, "あ");
        l.codec = "UTF-8".to_string();
        l.refresh_body(&texts);
        assert_ne!(l.body, "あ");
        assert!(l.body.contains('\u{FFFD}'));
    }

    /// KAR `@`-headers are stored remapped (`Kar*`), so the KAR dropdown
    /// entries match them while the raw `Text` entry does not.
    #[test]
    fn kar_type_filter_matches_remapped_records() {
        let texts = vec![
            rec(1, TextType::KarTitles, b"@TSong"),
            rec(1, TextType::Text, b"plain"),
        ];
        let mut l = LyricsState::new();
        l.codec = "UTF-8".to_string();
        l.type_filter = TextType::KarTitles as i32;
        l.refresh_body(&texts);
        assert_eq!(l.body, "@TSong\n");
        l.type_filter = TextType::Text as i32;
        l.refresh_body(&texts);
        assert_eq!(l.body, "plain\n");
    }

    #[test]
    fn preselect_ties_go_to_lowest_and_empty_falls_back() {
        // Tie 1:1 → lowest track wins; tie Text:Lyric → lowest type (1).
        let texts = vec![rec(3, TextType::Lyric, b"a"), rec(2, TextType::Text, b"b")];
        assert_eq!(preselect_filters(&texts), (Some(2), TextType::Text as i32));
        // No texts → all tracks + Lyric fallback (`window.cpp:568-571`).
        assert_eq!(preselect_filters(&[]), (None, TextType::Lyric as i32));
    }

    /// C++ scores `@`-headers under their raw meta type (`Text`) before the
    /// `Kar*` remap; the recompute recovers that, so KAR preselects match.
    #[test]
    fn preselect_counts_kar_headers_as_text() {
        let texts = vec![
            rec(1, TextType::KarFileType, b"@Kfile"),
            rec(1, TextType::KarVersion, b"@V1"),
            rec(1, TextType::KarTitles, b"@Tsong"),
            rec(1, TextType::Lyric, b"la"),
        ];
        // Raw counts Text=3, Lyric=1 → best type is Text, not KarTitles.
        assert_eq!(preselect_filters(&texts), (Some(1), TextType::Text as i32));
    }

    #[test]
    fn track_options_label_tracks_with_names() {
        let texts = vec![
            rec(1, TextType::TrackName, b"Piano"),
            rec(1, TextType::InstrumentName, b"Grand"),
            rec(3, TextType::TrackName, b""),
        ];
        assert_eq!(
            track_options(&texts, 3, "UTF-8"),
            vec![
                "All tracks".to_string(),
                "Track 1 — Piano Grand".to_string(),
                "Track 2".to_string(),
                "Track 3".to_string(),
            ]
        );
        assert_eq!(
            track_options(&texts, 0, "UTF-8"),
            vec!["All tracks".to_string()]
        );
        assert_eq!(TYPE_OPTION_NAMES.len(), 14);
        assert_eq!(TYPE_OPTION_NAMES[0], "All types");
        assert_eq!(TYPE_OPTION_NAMES[5], "Lyrics");
        assert_eq!(TYPE_OPTION_NAMES[13], "KAR other");
    }

    #[test]
    fn codec_option_index_is_case_insensitive_with_off_list_zero() {
        assert_eq!(codec_option_index("UTF-8"), 0);
        assert_eq!(codec_option_index("shift_jis"), 3);
        assert_eq!(codec_option_index("WINDOWS-1252"), 2);
        // Off-list detected name → display index 0, decode unaffected.
        assert_eq!(codec_option_index("EUC-JP"), 0);
        assert_eq!(codec_option_index(""), 0);
    }

    /// `onText` filter matrix (`window.cpp:158-161`).
    #[test]
    fn live_filter_matrix() {
        let l = LyricsState::new();
        // Defaults: all tracks, lyric type only.
        assert!(l.passes_filter(7, 5));
        assert!(!l.passes_filter(7, 1));
        let mut l = l;
        // Type 0 = all types.
        l.type_filter = 0;
        assert!(l.passes_filter(7, 1));
        assert!(l.passes_filter(7, 13));
        // Track filter set: only that track passes.
        l.track_filter = Some(2);
        assert!(l.passes_filter(2, 1));
        assert!(!l.passes_filter(3, 1));
    }

    /// Exact `onText` segmentation: previous `current` becomes `past`, the
    /// new text becomes `current`; rejections bump nothing.
    #[test]
    fn append_live_segments_current_past_and_seq() {
        let mut l = LyricsState::new();
        assert!(l.append_live(1, 5, "Hel"));
        assert_eq!((l.live_past.as_str(), l.live_current.as_str()), ("", "Hel"));
        assert!(l.sung);
        assert_eq!(l.live_seq, 1);
        assert!(l.append_live(1, 5, "lo"));
        assert_eq!(
            (l.live_past.as_str(), l.live_current.as_str()),
            ("Hel", "lo")
        );
        assert_eq!(l.live_tail(), "Hello");
        assert_eq!(l.live_seq, 2);
        assert!(!l.append_live(1, 1, "skipped"));
        assert_eq!(l.live_tail(), "Hello");
        assert_eq!(l.live_seq, 2);
        l.clear_live();
        assert!(l.live_tail().is_empty());
        assert!(!l.sung);
        assert_eq!(l.live_seq, 2);
    }

    /// Static prefill already contains sung records; export them only once.
    #[test]
    fn full_text_does_not_duplicate_sung_lines() {
        let texts = vec![rec(1, TextType::Lyric, b"la ")];
        let mut l = LyricsState::new();
        l.codec = "UTF-8".to_string();
        l.refresh_body(&texts);
        assert_eq!(l.body, "la ");
        assert!(l.append_live(1, 5, "la "));
        assert_eq!(l.full_text(), "la ");
    }

    fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
        Rgba { r, g, b, a }
    }

    fn assert_close(got: Option<Rgba>, want: Rgba, input: &str) {
        let got = got.unwrap_or_else(|| panic!("{input:?} should parse"));
        for (g, w) in [
            (got.r, want.r),
            (got.g, want.g),
            (got.b, want.b),
            (got.a, want.a),
        ] {
            assert!(
                (g - w).abs() < 0.002,
                "{input:?}: got {got:?}, want {want:?}"
            );
        }
    }

    /// Every `ok=1` probe row from arch §2.6 (P1/P2/P5).
    #[test]
    fn color_spec_ok_rows() {
        let n8 = 0x88 as f32 / 255.0;
        let rows: &[(&str, Rgba)] = &[
            ("#888", rgba(n8, n8, n8, 1.0)),
            ("#8888", rgba(n8, n8, n8, n8)),
            ("#888888", rgba(n8, n8, n8, 1.0)),
            ("#88888888", rgba(n8, n8, n8, n8)),
            ("#ffff", rgba(1.0, 1.0, 1.0, 1.0)),
            ("#FFF", rgba(1.0, 1.0, 1.0, 1.0)),
            ("rgb(255,0,0)", rgba(1.0, 0.0, 0.0, 1.0)),
            ("rgba(255,0,0,0.5)", rgba(1.0, 0.0, 0.0, 0.5)),
            ("rgb(100%,0%,0%)", rgba(1.0, 0.0, 0.0, 1.0)),
            ("rgb(300,0,0)", rgba(1.0, 0.0, 0.0, 1.0)),
            ("red", rgba(1.0, 0.0, 0.0, 1.0)),
            ("Red", rgba(1.0, 0.0, 0.0, 1.0)),
            ("RED", rgba(1.0, 0.0, 0.0, 1.0)),
            ("grey", rgba(0.5020, 0.5020, 0.5020, 1.0)),
            ("gray", rgba(0.5020, 0.5020, 0.5020, 1.0)),
            ("rebeccapurple", rgba(0.4, 0.2, 0.6, 1.0)),
            ("rgba(255,0,0,2)", rgba(1.0, 0.0, 0.0, 1.0)),
            ("rgba(255,0,0,-1)", rgba(1.0, 0.0, 0.0, 0.0)),
            ("rgb(255, 0, 0)", rgba(1.0, 0.0, 0.0, 1.0)),
            ("rgb(-10,0,0)", rgba(0.0, 0.0, 0.0, 1.0)),
            ("hsl(0,100%,50%)", rgba(1.0, 0.0, 0.0, 1.0)),
            ("rgba(255, 0, 0, 0.5)", rgba(1.0, 0.0, 0.0, 0.5)),
            ("#aBcDeF", rgba(0.6706, 0.8039, 0.9373, 1.0)),
            ("#abcd", rgba(0.6667, 0.7333, 0.8000, 0.8667)),
            ("hsla(0,100%,50%,0.5)", rgba(1.0, 0.0, 0.0, 0.5)),
        ];
        for (input, want) in rows {
            assert_close(parse_gdk_rgba(input), *want, input);
        }
    }

    /// Every `ok=0` probe row + the arch §2.6 rejection list: all fail, so
    /// the gray init survives ("untouched").
    #[test]
    fn color_spec_err_rows() {
        for input in [
            "#88",
            "  #fff  ",
            "rgba(100%,0%,0%,50%)",
            "rgb(255,0)",
            "rgb(255 0 0)",
            "transparent",
            " transparent ",
            "notacolor",
            "",
            "   ",
            "RGB(255,0,0)",
            "HSL(0,100%,50%)",
            "#88g",
            "rgb(",
            "rgb(255,0,0",
            "#",
            "hsl(0,50,50)",
        ] {
            assert_eq!(parse_gdk_rgba(input), None, "{input:?} must fail");
        }
        assert_eq!(GRAY_FALLBACK, rgba(0.5, 0.5, 0.5, 1.0));
    }

    #[test]
    fn named_table_is_complete_and_cased() {
        assert_eq!(CSS_NAMED_COLORS.len(), 148);
        // Spot-check first/last/middle entries against the CSS table.
        assert_eq!(CSS_NAMED_COLORS[0].0, "aliceblue");
        assert_eq!(CSS_NAMED_COLORS[147].0, "yellowgreen");
        assert!(parse_gdk_rgba("DarkSlateBlue").is_some());
        assert!(parse_gdk_rgba("LIGHTGOLDENRODYELLOW").is_some());
    }

    #[test]
    fn hex_string_round_trips_through_parse() {
        for c in [
            rgba(1.0, 0.0, 0.0, 1.0),
            rgba(0.0, 0.5, 1.0, 0.5),
            GRAY_FALLBACK,
            rgba(0.5333, 0.5333, 0.5333, 1.0),
        ] {
            let text = rgba_to_hex_string(c);
            let back = parse_gdk_rgba(&text).unwrap_or_else(|| panic!("{text:?} parses"));
            assert_close(Some(back), c, &text);
        }
        assert_eq!(rgba_to_hex_string(rgba(1.0, 0.0, 0.0, 1.0)), "#ff0000");
        assert_eq!(rgba_to_hex_string(rgba(1.0, 0.0, 0.0, 0.5)), "#ff000080");
    }

    /// Every arch §2.6 font row (P1/P3/P4): family, size, weight, style,
    /// stretch, variant. Pango units = pt × 1024 (asserted via the pt value).
    #[test]
    fn font_spec_rows() {
        type Row<'a> = (
            &'a str,
            &'a str,
            Option<FontSize>,
            u16,
            FontStyle,
            Option<FontStretch>,
            bool,
        );
        let pt = |v: f32| Some(FontSize::Pt(v));
        let rows: &[Row<'_>] = &[
            (
                "Sans 16",
                "Sans",
                pt(16.0),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans Bold 12",
                "Sans",
                pt(12.0),
                700,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans Italic Bold 10.5",
                "Sans",
                pt(10.5),
                700,
                FontStyle::Italic,
                None,
                false,
            ),
            ("Sans", "Sans", None, 400, FontStyle::Normal, None, false),
            ("16", "Sans", pt(16.0), 400, FontStyle::Normal, None, false),
            ("", "Sans", None, 400, FontStyle::Normal, None, false),
            ("!!!", "!!!", None, 400, FontStyle::Normal, None, false),
            (
                "Monospace 9",
                "Monospace",
                pt(9.0),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans,Serif 12",
                "Sans,Serif",
                pt(12.0),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "  Sans   16  ",
                "Sans",
                pt(16.0),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Cantarell Semi-Bold 11",
                "Cantarell",
                pt(11.0),
                600,
                FontStyle::Normal,
                None,
                false,
            ),
            ("Sans 0", "Sans", None, 400, FontStyle::Normal, None, false),
            (
                "Sans -5",
                "Sans -5",
                None,
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "sans bold 12",
                "sans",
                pt(12.0),
                700,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans 12pt",
                "Sans 12pt",
                None,
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans 12px",
                "Sans",
                Some(FontSize::Px(12.0)),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans 600 12",
                "Sans 600",
                pt(12.0),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "DejaVu Sans Mono Bold Oblique 9",
                "DejaVu Sans Mono",
                pt(9.0),
                700,
                FontStyle::Oblique,
                None,
                false,
            ),
            (
                "Sans Condensed 12",
                "Sans",
                pt(12.0),
                400,
                FontStyle::Normal,
                Some(FontStretch::Condensed),
                false,
            ),
            (
                "Sans Small-Caps 12",
                "Sans",
                pt(12.0),
                400,
                FontStyle::Normal,
                None,
                true,
            ),
            (
                "Sans Expanded Bold Italic 12",
                "Sans",
                pt(12.0),
                700,
                FontStyle::Italic,
                Some(FontStretch::Expanded),
                false,
            ),
            (
                "Sans Ultra-Bold 12",
                "Sans",
                pt(12.0),
                800,
                FontStyle::Normal,
                None,
                false,
            ),
            (
                "Sans Oblique 12",
                "Sans",
                pt(12.0),
                400,
                FontStyle::Oblique,
                None,
                false,
            ),
            (
                "DejaVu Sans, Sans 12",
                "DejaVu Sans,Sans",
                pt(12.0),
                400,
                FontStyle::Normal,
                None,
                false,
            ),
        ];
        for (input, family, size, weight, style, stretch, small_caps) in rows {
            let got = parse_pango_font(input);
            assert_eq!(got.family, *family, "{input:?} family");
            assert_eq!(got.size, *size, "{input:?} size");
            assert_eq!(got.weight, *weight, "{input:?} weight");
            assert_eq!(got.style, *style, "{input:?} style");
            assert_eq!(got.stretch, *stretch, "{input:?} stretch");
            assert_eq!(got.small_caps, *small_caps, "{input:?} small-caps");
        }
        // Pango-unit cross-checks (pt × 1024).
        assert_eq!(16.0 * 1024.0, 16384.0);
        assert_eq!(10.5 * 1024.0, 10752.0);
        // First comma entry wins for typeface resolution.
        assert_eq!(
            parse_pango_font("DejaVu Sans, Sans 12").first_family(),
            "DejaVu Sans"
        );
        assert_eq!(parse_pango_font("Sans 16").first_family(), "Sans");
    }

    /// The `12px`-vs-`12pt` divergence is by design (arch §2.6): pin both,
    /// plus size resolution to iced logical px.
    #[test]
    fn font_px_vs_pt_divergence_and_px_resolution() {
        let px = parse_pango_font("Sans 12px");
        assert_eq!(px.size, Some(FontSize::Px(12.0)));
        assert_eq!(font_size_px(px.size, DEFAULT_LYRICS_SIZE_PT), 12.0);
        let pt = parse_pango_font("Sans 12pt");
        assert_eq!(pt.family, "Sans 12pt");
        assert_eq!(pt.size, None);
        assert_eq!(
            font_size_px(pt.size, DEFAULT_LYRICS_SIZE_PT),
            16.0 * 96.0 / 72.0
        );
        assert_eq!(
            font_size_px(Some(FontSize::Pt(16.0)), DEFAULT_LYRICS_SIZE_PT),
            16.0 * 96.0 / 72.0
        );
        // Non-ASCII tokens never panic the size probe (boundary-safe).
        assert_eq!(parse_pango_font("éx").family, "éx");
        assert_eq!(parse_pango_font("日本語 12").family, "日本語");
    }

    #[test]
    fn alignment_matrix_matches_setting_map() {
        assert_eq!(parse_alignment(0), LyricAlign::Left);
        assert_eq!(parse_alignment(1), LyricAlign::Center);
        assert_eq!(parse_alignment(2), LyricAlign::Right);
        // Out-of-range/negative → left (`alignmentFromSetting` verbatim).
        assert_eq!(parse_alignment(3), LyricAlign::Left);
        assert_eq!(parse_alignment(-1), LyricAlign::Left);
        assert_eq!(parse_alignment(99), LyricAlign::Left);
    }

    #[test]
    fn applied_prefs_fall_back_per_row() {
        let a = AppliedLyricsPrefs::from_rows("garbage", "#1c71d8", "", "!!!", 7);
        assert_eq!(a.future, GRAY_FALLBACK);
        assert_close(
            Some(a.past),
            rgba(
                0x1c as f32 / 255.0,
                0x71 as f32 / 255.0,
                0xd8 as f32 / 255.0,
                1.0,
            ),
            "past",
        );
        assert_eq!(a.highlight, GRAY_FALLBACK);
        assert_eq!(a.font.family, "!!!");
        assert_eq!(a.font.size, None);
        assert_eq!(a.alignment, LyricAlign::Left);
        // Stock defaults apply cleanly (16pt Sans, left).
        let d = AppliedLyricsPrefs::default();
        assert_eq!(d.font.family, "Sans");
        assert_eq!(d.font.size, Some(FontSize::Pt(16.0)));
        assert_eq!(d.alignment, LyricAlign::Left);
        assert_close(
            Some(d.future),
            rgba(
                0x88 as f32 / 255.0,
                0x88 as f32 / 255.0,
                0x88 as f32 / 255.0,
                1.0,
            ),
            "default future",
        );
    }
}
