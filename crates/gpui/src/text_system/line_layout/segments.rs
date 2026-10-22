//! Shaping a long line in segments.
//!
//! Shaping a long plain paragraph as one line costs time in proportion to its length, every time
//! its text changes. A paragraph that grows by a word, as a streamed reply does, would shape all of
//! its text again for each word. When the platform shapes each word on its own, a line cut after a
//! space can be shaped in pieces, and the layout of the pieces put side by side is the layout of
//! the line. Each piece is cached by its text, its font size and its font runs, so a change shapes
//! the pieces it touches and takes the others from the cache.
//!
//! Where to cut depends on the text around the cut alone, so that a change moves the cuts near it
//! and leaves the others where they were.

use super::{AsCacheKeyRef, CacheKey, CacheKeyRef, FontRun, LineLayout, ShapedGlyph, ShapedRun};
use crate::{Pixels, SharedString, point};
use collections::FxHashMap;
use smallvec::SmallVec;
use std::{ops::Range, sync::Arc};

/// Lines shorter than this are shaped whole.
pub(super) const MIN_LINE_LEN: usize = 1024;

/// A segment is at least this long, unless it is the last.
const MIN_SEGMENT_LEN: usize = 128;

/// A segment ends at the first place to cut after it reaches this length.
const MAX_SEGMENT_LEN: usize = 2048;

/// A place to cut is used when the hash of the two words before it begins with this many zero
/// bits, which is one place in 32 on average.
const CUT_BITS: u32 = 5;

/// The glyphs of the cached segments are kept up to this many, about 8 MiB.
const MAX_CACHED_GLYPHS: usize = 1 << 18;

/// Where to cut `text` into segments, as byte offsets inside the text in increasing order.
///
/// Returns `None` when the text is short, has no place to cut, or has characters whose layout
/// depends on the text around them in more than the word before them. Those are right-to-left
/// text and bidirectional controls, which reorder across words, and tabs and other control
/// characters, whose width depends on where in the line they fall.
///
/// A cut falls after a space, before a character that starts a word and that a line may break
/// before. Cutting there changes nothing in the text system's choice of fonts for the characters
/// on either side, since a space is in the primary font. The text system shapes the word before
/// the space and the word after it on their own either way.
pub(super) fn cut_points(text: &str) -> Option<Vec<usize>> {
    if text.len() < MIN_LINE_LEN {
        return None;
    }

    let mut cuts = Vec::new();
    let mut last_cut = 0;
    let mut previous_was_space = false;
    let mut word_hash = WORD_HASH_SEED;
    let mut previous_word_hash = WORD_HASH_SEED;
    let mut prior_word_hash = WORD_HASH_SEED;
    for (ix, character) in text.char_indices() {
        if character == ' ' {
            if !previous_was_space {
                prior_word_hash = previous_word_hash;
                previous_word_hash = word_hash;
                word_hash = WORD_HASH_SEED;
            }
            previous_was_space = true;
            continue;
        }

        if character.is_ascii() {
            if character.is_ascii_control() {
                return None;
            }
        } else if character.is_control() || is_bidirectional(character) {
            return None;
        }

        if previous_was_space {
            let len = ix - last_cut;
            if starts_word(character)
                && len >= MIN_SEGMENT_LEN
                && (len >= MAX_SEGMENT_LEN || is_cut(previous_word_hash, prior_word_hash))
            {
                cuts.push(ix);
                last_cut = ix;
            }
            previous_was_space = false;
        }
        word_hash = hash_character(word_hash, character);
    }

    (!cuts.is_empty()).then_some(cuts)
}

const WORD_HASH_SEED: u32 = 0x811c_9dc5;

/// Whether the words before a place to cut, as hashes, make it one to cut at.
fn is_cut(previous_word_hash: u32, prior_word_hash: u32) -> bool {
    let hash = (previous_word_hash ^ prior_word_hash.rotate_left(13)).wrapping_mul(0x9e37_79b1);
    hash >> (u32::BITS - CUT_BITS) == 0
}

fn hash_character(hash: u32, character: char) -> u32 {
    (hash ^ character as u32).wrapping_mul(0x0100_0193)
}

/// Whether a line may break before `character` after a space, and the character doesn't join the
/// space in shaping. These are the letters and digits of scripts shaped one character to a glyph,
/// and the punctuation that opens a word.
fn starts_word(character: char) -> bool {
    match character {
        'a'..='z' | 'A'..='Z' | '0'..='9' => true,
        '(' | '[' | '{' | '"' | '\'' | '`' | '*' | '_' | '#' | '@' | '$' => true,
        // Latin, Greek and Cyrillic letters, without the combining marks among them.
        '\u{c0}'..='\u{24f}'
        | '\u{370}'..='\u{3ff}'
        | '\u{400}'..='\u{481}'
        | '\u{48a}'..='\u{52f}' => character.is_alphabetic(),
        _ => false,
    }
}

/// Whether `character` is written right to left, or controls how text is ordered.
fn is_bidirectional(character: char) -> bool {
    matches!(
        character,
        '\u{590}'..='\u{8ff}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{fb1d}'..='\u{fdff}'
            | '\u{fe70}'..='\u{feff}'
            | '\u{10800}'..='\u{10fff}'
            | '\u{1e800}'..='\u{1efff}'
    )
}

/// Clips the font runs of a line to its segments, one after the other.
pub(super) struct RunClipper<'a> {
    runs: &'a [FontRun],
    /// The first run that ends after the end of the last segment clipped, and where it starts.
    next_run: usize,
    next_run_start: usize,
}

impl<'a> RunClipper<'a> {
    pub(super) fn new(runs: &'a [FontRun]) -> Self {
        Self {
            runs,
            next_run: 0,
            next_run_start: 0,
        }
    }

    /// The font runs that fall inside `range`, with the start of the range taken off. The ranges
    /// must come one after the other from the start of the line.
    pub(super) fn clip(&mut self, range: Range<usize>) -> SmallVec<[FontRun; 1]> {
        let mut clipped = SmallVec::new();
        let mut run_start = self.next_run_start;
        for (ix, run) in self.runs.iter().enumerate().skip(self.next_run) {
            let run_end = run_start + run.len;
            if run_start >= range.end {
                break;
            }
            let len = run_end
                .min(range.end)
                .saturating_sub(run_start.max(range.start));
            if len > 0 {
                clipped.push(FontRun {
                    len,
                    font_id: run.font_id,
                });
            }
            if run_end > range.end {
                // The next segment starts inside this run.
                self.next_run = ix;
                self.next_run_start = run_start;
                return clipped;
            }
            run_start = run_end;
            self.next_run = ix + 1;
            self.next_run_start = run_end;
        }
        clipped
    }
}

/// Puts the layouts of segments side by side, as the layout of the whole line. Each part is a
/// segment's offset in the text and its layout.
pub(super) fn join(
    parts: &[(usize, Arc<LineLayout>)],
    len: usize,
    font_size: Pixels,
) -> LineLayout {
    let mut layout = LineLayout {
        font_size,
        len,
        ascent: parts
            .first()
            .map_or_else(Default::default, |(_, part)| part.ascent),
        descent: parts
            .first()
            .map_or_else(Default::default, |(_, part)| part.descent),
        ..Default::default()
    };
    let glyph_count = parts
        .iter()
        .flat_map(|(_, part)| &part.runs)
        .map(|run| run.glyphs.len())
        .sum::<usize>();
    for (start, part) in parts {
        let x_offset = layout.width;
        for run in &part.runs {
            let glyphs = run.glyphs.iter().map(|glyph| ShapedGlyph {
                id: glyph.id,
                position: point(glyph.position.x + x_offset, glyph.position.y),
                index: glyph.index + start,
                is_emoji: glyph.is_emoji,
            });
            match layout
                .runs
                .last_mut()
                .filter(|last| last.font_id == run.font_id)
            {
                Some(last) => last.glyphs.extend(glyphs),
                None => {
                    let mut run_glyphs = Vec::with_capacity(if layout.runs.is_empty() {
                        glyph_count
                    } else {
                        run.glyphs.len()
                    });
                    run_glyphs.extend(glyphs);
                    layout.runs.push(ShapedRun {
                        font_id: run.font_id,
                        glyphs: run_glyphs,
                    });
                }
            }
        }
        layout.width += part.width;
        layout.ascent = layout.ascent.max(part.ascent);
        layout.descent = layout.descent.max(part.descent);
    }
    layout
}

/// The shaped segments of recent lines, by their text, font size and font runs. It is kept apart
/// from the frames' caches, which drop a line that no frame lays out, so that a paragraph that is
/// left alone for a while and then changes still finds its segments.
#[derive(Default)]
pub(super) struct SegmentCache {
    segments: FxHashMap<Arc<CacheKey>, CachedSegment>,
    clock: u64,
    glyph_count: usize,
}

struct CachedSegment {
    layout: Arc<LineLayout>,
    glyph_count: usize,
    last_used: u64,
}

impl SegmentCache {
    pub(super) fn get(
        &mut self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
    ) -> Option<Arc<LineLayout>> {
        let key = &CacheKeyRef {
            text,
            font_size,
            runs,
            wrap_width: None,
            force_width: None,
        } as &dyn AsCacheKeyRef;
        let segment = self.segments.get_mut(key)?;
        self.clock += 1;
        segment.last_used = self.clock;
        Some(segment.layout.clone())
    }

    pub(super) fn insert(
        &mut self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        layout: Arc<LineLayout>,
    ) {
        let glyph_count = layout.runs.iter().map(|run| run.glyphs.len()).sum();
        let key = Arc::new(CacheKey {
            text: SharedString::from(text.to_owned()),
            font_size,
            runs: SmallVec::from(runs),
            wrap_width: None,
            force_width: None,
        });
        self.clock += 1;
        let previous = self.segments.insert(
            key,
            CachedSegment {
                layout,
                glyph_count,
                last_used: self.clock,
            },
        );
        self.glyph_count += glyph_count;
        if let Some(previous) = previous {
            self.glyph_count -= previous.glyph_count;
        }
        if self.glyph_count > MAX_CACHED_GLYPHS {
            self.evict();
        }
    }

    /// Drops the segments used longest ago, down to three quarters of the limit.
    fn evict(&mut self) {
        let mut by_age = self
            .segments
            .iter()
            .map(|(key, segment)| (segment.last_used, key.clone()))
            .collect::<Vec<_>>();
        by_age.sort_unstable_by_key(|(last_used, _)| *last_used);
        for (_, key) in by_age {
            if self.glyph_count <= MAX_CACHED_GLYPHS / 4 * 3 {
                break;
            }
            if let Some(segment) = self.segments.remove(&key) {
                self.glyph_count -= segment.glyph_count;
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.segments.clear();
        self.glyph_count = 0;
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.segments.len()
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{FontId, GlyphId, px};

    /// Text of random words, a sentence at a time, with accented, Greek and Cyrillic letters
    /// and punctuation among them.
    pub(crate) fn paragraph(len: usize, seed: u32) -> String {
        const ALPHABET: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'l', 'm', 'n', 'o', 'p', 'r', 's', 't',
            'u', 'w', 'y', 'é', 'ï', 'ß', 'Ω', 'μ', 'д', 'ж',
        ];
        let mut text = String::new();
        let mut state = seed.wrapping_mul(2654435761).wrapping_add(1);
        let mut next = move |bound: u32| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 16) % bound
        };
        while text.len() < len {
            let word_len = 1 + next(9);
            match next(40) {
                0 => text.push('('),
                1 => text.push('"'),
                _ => {}
            }
            for _ in 0..word_len {
                text.push(ALPHABET[next(ALPHABET.len() as u32) as usize]);
            }
            match next(20) {
                0 => text.push(','),
                1 => text.push_str(".)"),
                2 => text.push_str(";"),
                _ => {}
            }
            text.push(' ');
        }
        text
    }

    fn segments<'a>(text: &'a str, cuts: &[usize]) -> Vec<&'a str> {
        let mut start = 0;
        let mut segments = Vec::new();
        for end in cuts.iter().copied().chain([text.len()]) {
            segments.push(&text[start..end]);
            start = end;
        }
        segments
    }

    #[test]
    fn short_text_is_not_cut() {
        assert_eq!(cut_points(&paragraph(MIN_LINE_LEN - 100, 1)), None);
        assert_eq!(cut_points(""), None);
    }

    #[test]
    fn text_without_a_place_to_cut_is_not_cut() {
        assert_eq!(cut_points(&"x".repeat(5000)), None);
        // Spaces followed by characters that join them in shaping or in line breaking.
        assert_eq!(cut_points(&"a ,".repeat(2000)), None);
        assert_eq!(cut_points(&"a \u{301}".repeat(2000)), None);
    }

    #[test]
    fn cuts_follow_spaces_and_come_before_word_starts() {
        let text = paragraph(30_000, 2);
        let cuts = cut_points(&text).unwrap();
        assert!(cuts.windows(2).all(|pair| pair[0] < pair[1]));
        for &cut in &cuts {
            assert!(text.is_char_boundary(cut));
            assert_eq!(text.as_bytes()[cut - 1], b' ');
            assert!(starts_word(text[cut..].chars().next().unwrap()));
        }
        let segments = segments(&text, &cuts);
        assert_eq!(segments.concat(), text);
        let (last, rest) = segments.split_last().unwrap();
        assert!(rest.iter().all(|segment| segment.len() >= MIN_SEGMENT_LEN));
        assert!(
            segments
                .iter()
                .all(|segment| segment.len() < MAX_SEGMENT_LEN + 64)
        );
        // Several hundred bytes on average, and the last segment is whatever is left.
        let mean = (text.len() - last.len()) / rest.len();
        assert!((MIN_SEGMENT_LEN..1000).contains(&mean), "{mean}");
    }

    #[test]
    fn a_cut_after_a_run_of_spaces_comes_after_all_of_them() {
        let text = format!(
            "{}  {}   {}",
            "a".repeat(2500),
            "b".repeat(2500),
            "c".repeat(2500)
        );
        let cuts = cut_points(&text).unwrap();
        assert_eq!(cuts, vec![2502, 5005]);
        for &cut in &cuts {
            assert_eq!(text.as_bytes()[cut - 1], b' ');
            assert_ne!(text.as_bytes()[cut], b' ');
        }
    }

    #[test]
    fn long_words_are_cut_at_the_next_space_past_the_longest_segment() {
        let text = format!("{} {}", "a".repeat(5000), "b".repeat(5000));
        assert_eq!(cut_points(&text), Some(vec![5001]));
    }

    #[test]
    fn text_that_reorders_or_depends_on_its_position_is_not_cut() {
        let plain = paragraph(5000, 3);
        for bad in [
            "\u{5d0}\u{5d1}",
            "\u{627}\u{644}\u{639}",
            "\u{200f}",
            "\u{202e}",
            "\u{2067}",
            "\t",
            "\u{7}",
            "\u{85}",
            "\u{fb50}",
        ] {
            for at in [0, plain.len() / 2, plain.len() - 1] {
                let mut text = plain.clone();
                let at = (0..=at)
                    .rev()
                    .find(|&at| text.is_char_boundary(at))
                    .unwrap();
                text.insert_str(at, bad);
                assert_eq!(cut_points(&text), None, "{bad:?} at {at}");
            }
        }
        assert!(cut_points(&plain).is_some());
    }

    #[test]
    fn a_change_moves_only_the_cuts_near_it() {
        let text = paragraph(20_000, 4);
        let cuts = cut_points(&text).unwrap();
        let segments_before = segments(&text, &cuts);
        assert!(segments_before.len() > 20);

        let reshaped = |edited: &str| {
            let cuts = cut_points(edited).unwrap();
            let after = segments(edited, &cuts);
            after
                .iter()
                .filter(|segment| !segments_before.contains(segment))
                .count()
        };

        let mut appended = text.clone();
        appended.push_str("one more word ");
        assert!(reshaped(&appended) <= 1);

        for fraction in [10, 25, 50, 75, 90] {
            // The space before the point a fraction of the way through.
            let at = (0..=text.len() * fraction / 100)
                .rev()
                .find(|&at| text.as_bytes()[at] == b' ')
                .unwrap();
            let mut edited = text.clone();
            edited.insert_str(at, " inserted");
            assert!(reshaped(&edited) <= 4, "{fraction}%");
            // The word before it, deleted.
            let word_start = text[..at].rfind(' ').unwrap() + 1;
            let mut edited = text.clone();
            edited.replace_range(word_start..at, "");
            assert!(reshaped(&edited) <= 4, "{fraction}%");
        }
    }

    #[test]
    fn runs_are_clipped_to_the_segments() {
        let run = |len, font_id| FontRun {
            len,
            font_id: FontId(font_id),
        };
        let runs = [run(10, 1), run(5, 2), run(20, 1)];
        let clip = |ranges: &[Range<usize>]| {
            let mut clipper = RunClipper::new(&runs);
            ranges
                .iter()
                .map(|range| clipper.clip(range.clone()).to_vec())
                .collect::<Vec<_>>()
        };
        assert_eq!(clip(&[0..35]), [runs.to_vec()]);
        assert_eq!(
            clip(&[0..8, 8..16, 16..30, 30..35]),
            [
                vec![run(8, 1)],
                vec![run(2, 1), run(5, 2), run(1, 1)],
                vec![run(14, 1)],
                vec![run(5, 1)],
            ]
        );
        assert_eq!(
            clip(&[0..10, 10..15, 15..35]),
            [vec![run(10, 1)], vec![run(5, 2)], vec![run(20, 1)]]
        );
        // Text past the runs has none.
        assert_eq!(
            clip(&[0..30, 30..50, 50..60]),
            [
                vec![run(10, 1), run(5, 2), run(15, 1)],
                vec![run(5, 1)],
                vec![]
            ]
        );
    }

    fn glyph(id: u32, x: f32, index: usize) -> ShapedGlyph {
        ShapedGlyph {
            id: GlyphId(id),
            position: point(px(x), px(0.)),
            index,
            is_emoji: false,
        }
    }

    fn part(font_ids: &[u32], width: f32, ascent: f32) -> Arc<LineLayout> {
        let mut runs: Vec<ShapedRun> = Vec::new();
        for (ix, &font_id) in font_ids.iter().enumerate() {
            let glyph = glyph(1, ix as f32 * 10., ix);
            match runs
                .last_mut()
                .filter(|run| run.font_id == FontId(font_id as usize))
            {
                Some(run) => run.glyphs.push(glyph),
                None => runs.push(ShapedRun {
                    font_id: FontId(font_id as usize),
                    glyphs: vec![glyph],
                }),
            }
        }
        Arc::new(LineLayout {
            font_size: px(10.),
            width: px(width),
            ascent: px(ascent),
            descent: px(2.),
            runs,
            len: font_ids.len(),
        })
    }

    #[test]
    fn segments_are_joined_side_by_side() {
        let parts = [
            (0, part(&[1, 1, 2], 30., 8.)),
            (3, part(&[2, 2], 20., 9.)),
            (5, part(&[1], 10., 7.)),
        ];
        let layout = join(&parts, 6, px(10.));
        assert_eq!(layout.len, 6);
        assert_eq!(layout.width, px(60.));
        assert_eq!(layout.ascent, px(9.));
        assert_eq!(layout.descent, px(2.));
        let runs = layout
            .runs
            .iter()
            .map(|run| {
                (
                    run.font_id.0,
                    run.glyphs
                        .iter()
                        .map(|glyph| (glyph.index, f32::from(glyph.position.x)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        // The run of the second font goes on across the cut.
        assert_eq!(
            runs,
            vec![
                (1, vec![(0, 0.), (1, 10.)]),
                (2, vec![(2, 20.), (3, 30.), (4, 40.)]),
                (1, vec![(5, 50.)]),
            ]
        );
    }

    #[test]
    fn the_cache_drops_the_segments_used_longest_ago() {
        let mut cache = SegmentCache::default();
        let runs = [FontRun {
            len: 10,
            font_id: FontId(0),
        }];
        let layout = |glyphs: usize| {
            Arc::new(LineLayout {
                runs: vec![ShapedRun {
                    font_id: FontId(0),
                    glyphs: (0..glyphs).map(|ix| glyph(1, ix as f32, ix)).collect(),
                }],
                ..Default::default()
            })
        };
        let chunk = MAX_CACHED_GLYPHS / 8;
        for ix in 0..8 {
            cache.insert(&format!("segment {ix}"), px(10.), &runs, layout(chunk));
        }
        assert_eq!(cache.len(), 8);
        // Using the first segment makes the second the one used longest ago.
        assert!(cache.get("segment 0", px(10.), &runs).is_some());
        cache.insert("one more", px(10.), &runs, layout(chunk));
        assert_eq!(cache.len(), 6);
        assert!(cache.get("segment 0", px(10.), &runs).is_some());
        assert!(cache.get("segment 1", px(10.), &runs).is_none());
        assert!(cache.get("one more", px(10.), &runs).is_some());
        // The size and the runs are part of the key.
        assert!(cache.get("segment 0", px(11.), &runs).is_none());
        assert!(cache.get("segment 0", px(10.), &[]).is_none());
    }
}
