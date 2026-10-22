//! Laying out long lines in segments gives the layout of the whole line, with the Linux text system.
#![cfg(target_os = "linux")]

use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use gpui::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, LineLayout, Pixels,
    PlatformTextSystem, RenderGlyphParams, SharedString, Size, TextRenderingMode, TextRun,
    TextSystem, WindowTextSystem, font, hsla, px,
};
use gpui_screenshot::{MONO_FAMILY, SANS_FAMILY, bundled_fonts};
use gpui_wgpu::CosmicTextSystem;

/// The Linux text system, with a say in whether it declares that it shapes words on their own,
/// and a count of what it shapes.
struct Delegate {
    inner: Arc<CosmicTextSystem>,
    segmented: bool,
    shaped: Mutex<Vec<usize>>,
}

impl PlatformTextSystem for Delegate {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.inner.add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.inner.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        self.inner.font_id(descriptor)
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.inner.font_metrics(font_id)
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        self.inner.typographic_bounds(font_id, glyph_id)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.inner.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.inner.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.inner.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.inner.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        self.shaped.lock().unwrap().push(text.len());
        self.inner.layout_line(text, font_size, runs)
    }

    fn shapes_words_independently(&self) -> bool {
        self.segmented && self.inner.shapes_words_independently()
    }

    fn recommended_rendering_mode(&self, font_id: FontId, font_size: Pixels) -> TextRenderingMode {
        self.inner.recommended_rendering_mode(font_id, font_size)
    }
}

/// Two views of one platform text system, one that lays out long lines in segments and one that
/// lays out every line whole.
struct Pair {
    segmented: Arc<Delegate>,
    whole: Arc<Delegate>,
}

impl Pair {
    fn new(platform: Arc<CosmicTextSystem>) -> Self {
        let make = |segmented| {
            Arc::new(Delegate {
                inner: platform.clone(),
                segmented,
                shaped: Mutex::default(),
            })
        };
        Self {
            segmented: make(true),
            whole: make(false),
        }
    }

    /// With the bundled fonts alone, where most scripts have no font.
    fn bundled() -> Self {
        let platform = Arc::new(CosmicTextSystem::new_without_system_fonts(SANS_FAMILY));
        platform.add_fonts(bundled_fonts()).unwrap();
        Self::new(platform)
    }

    /// With the fonts of the machine as well, which have glyphs for more of the samples.
    fn with_system_fonts() -> Self {
        let platform = Arc::new(CosmicTextSystem::new(SANS_FAMILY));
        platform.add_fonts(bundled_fonts()).unwrap();
        Self::new(platform)
    }
}

/// A window's text system with caches that are empty.
fn fresh(delegate: &Arc<Delegate>) -> WindowTextSystem {
    WindowTextSystem::new(Arc::new(TextSystem::new(delegate.clone())))
}

/// How far two layouts of a line are apart. Both add up the advances of the glyphs one after the
/// other in 32-bit floats, and the sums of a line of 100,000 pixels or more differ by rounding.
#[derive(Default, Debug)]
struct Difference {
    /// The largest difference between the distances from one glyph to the next, in pixels.
    advance: f32,
    /// The largest difference between a position and the same position divided by its size.
    relative: f32,
}

/// The distance from one glyph to the next can differ by this many steps of a 32-bit float at the
/// width of the line, and a little more. A change of kerning is much larger.
fn advance_tolerance(width: Pixels) -> f32 {
    8. * f32::EPSILON * f32::from(width) + 0.005
}

/// A position can differ by this part of its distance from the start of the line.
const RELATIVE_TOLERANCE: f32 = 3e-5;

fn compare(whole: &LineLayout, segmented: &LineLayout, context: &str) -> Difference {
    let mut difference = Difference::default();
    assert_eq!(whole.font_size, segmented.font_size, "{context}");
    assert_eq!(whole.len, segmented.len, "{context}");
    assert_eq!(whole.ascent, segmented.ascent, "{context}");
    assert_eq!(whole.descent, segmented.descent, "{context}");
    assert_eq!(whole.runs.len(), segmented.runs.len(), "{context}");
    let mut relative = |a: Pixels, b: Pixels| {
        let (a, b) = (f32::from(a), f32::from(b));
        difference.relative = difference.relative.max((a - b).abs() / a.abs().max(1.));
    };
    relative(whole.width, segmented.width);
    let mut previous = None::<(f32, f32)>;
    let mut advance = 0f32;
    for (whole_run, segmented_run) in whole.runs.iter().zip(&segmented.runs) {
        assert_eq!(whole_run.font_id, segmented_run.font_id, "{context}");
        assert_eq!(
            whole_run.glyphs.len(),
            segmented_run.glyphs.len(),
            "{context}"
        );
        for (a, b) in whole_run.glyphs.iter().zip(&segmented_run.glyphs) {
            assert_eq!(a.id, b.id, "{context}");
            assert_eq!(a.index, b.index, "{context}");
            assert_eq!(a.is_emoji, b.is_emoji, "{context}");
            assert_eq!(a.position.y, b.position.y, "{context}");
            relative(a.position.x, b.position.x);
            let (a, b) = (f32::from(a.position.x), f32::from(b.position.x));
            if let Some((previous_a, previous_b)) = previous {
                advance = advance.max(((a - previous_a) - (b - previous_b)).abs());
            }
            previous = Some((a, b));
        }
    }
    difference.advance = advance;
    difference
}

fn text_runs(text: &str, fonts: &[Font], step: usize) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + step).min(text.len());
        while !text.is_char_boundary(end) {
            end += 1;
        }
        runs.push(TextRun {
            len: end - start,
            font: fonts[runs.len() % fonts.len()].clone(),
            color: hsla(0., 0., 1., 1.),
            background_color: None,
            underline: None,
            strikethrough: None,
        });
        start = end;
    }
    runs
}

/// Lays out `text` with both systems and checks that the layouts match. Returns the lengths of
/// the segments shaped, which is empty if the line was shaped whole.
fn check(pair: &Pair, text: &str, runs: &[TextRun], context: &str) -> Vec<usize> {
    let whole = fresh(&pair.whole).layout_line(text, px(15.), runs, None);
    let segmented = fresh(&pair.segmented).layout_line(text, px(15.), runs, None);
    let difference = compare(&whole, &segmented, context);
    assert!(
        difference.advance <= advance_tolerance(whole.width)
            && difference.relative <= RELATIVE_TOLERANCE,
        "{context}: {difference:?}"
    );
    assert_eq!(
        std::mem::take(&mut *pair.whole.shaped.lock().unwrap()),
        [text.len()],
        "{context}"
    );
    let shaped = std::mem::take(&mut *pair.segmented.shaped.lock().unwrap());
    if shaped.len() == 1 {
        assert_eq!(shaped[0], text.len(), "{context}");
        Vec::new()
    } else {
        assert_eq!(shaped.iter().sum::<usize>(), text.len(), "{context}");
        shaped
    }
}

fn repeat_to(sentences: &[&str], len: usize) -> String {
    let mut text = String::new();
    let mut ix = 0;
    while text.len() < len {
        text.push_str(sentences[ix % sentences.len()]);
        // Vary the sentences so that the words around the cuts differ.
        text.push_str(&format!(" {} ", ix * 7919 % 1013));
        ix += 1;
    }
    text
}

struct Sample {
    name: &'static str,
    text: String,
    /// Whether the text has places where it can be cut.
    segmentable: bool,
}

fn samples() -> Vec<Sample> {
    let sample = |name, text: String, segmentable| Sample {
        name,
        text,
        segmentable,
    };
    vec![
        sample(
            "ascii",
            repeat_to(
                &[
                    "The quick brown fox jumps over the lazy dog while streaming tokens arrive.",
                    "Layout, caching (really) matters: Affirm AV Ty WA To Ye -> != <= >= === fi fl ffi.",
                    "Visit https://example.com/some/long/path?query=1&other=2 for \"details\", e.g. #1 @me $5.",
                    "AVATAR Tokyo Yankee Waltz Type Valve   with  double   spaces and trailing words",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "accented Latin",
            repeat_to(
                &[
                    "Voilà un café très crème, déjà vu à l'œuvre naïve façon über-süß.",
                    "Żółć gęślą jaźń, příliš žluťoučký kůň úpěl ďábelské ódy, İstanbul ığ.",
                    "El niño pidió jamón ibérico, ¿qué tal? ¡Más rápido! Año nuevo, vida nueva.",
                    "Ångström Øresund Œuvre Æther ǅ ǆ ŉ ẞ Ꞅ Ꞇ ꞓ ɐ ɑ ʃ ʒ ə",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "Greek and Cyrillic",
            repeat_to(
                &[
                    "Η γρήγορη καφέ αλεπού πηδάει πάνω από το τεμπέλικο σκυλί, ΑΒΓΔΕΖΗΘ ΊΎΏ.",
                    "Съешь же ещё этих мягких французских булок, да выпей чаю. Ёж Ъъ Ыы Эю Яя.",
                    "Привет, мир! Это длинный абзац обычного текста для проверки раскладки.",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "emoji",
            repeat_to(
                &[
                    "Great job 🎉 team 👍🏽 family 👨‍👩‍👧‍👦 flags 🇯🇵🇫🇷 hearts ❤️ ✨ stars ⭐ done ✅.",
                    "Keycaps 1️⃣ 2️⃣ #️⃣ and text, sun ☀️ rain 🌧️ 😀😃😄 faces 🥲 🫠 and more words follow.",
                    "Plain words between the pictures keep the line breakable at spaces 🚀 right here.",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "CJK with spaces",
            repeat_to(
                &[
                    "日本語の文章です。 これは長い段落の一部で、 English words are mixed in. 漢字とかな。",
                    "中文段落,包含一些 English 单词 and 标点符号,用来测试 排版 的结果。",
                    "한국어 문장입니다. 이것은 긴 단락의 일부이며 영어 words 도 섞여 있습니다.",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "CJK without spaces",
            "日本語の文章です。これは長い段落の一部です。漢字とひらがなとカタカナが混ざっています。"
                .repeat(120),
            false,
        ),
        sample(
            "right to left",
            repeat_to(
                &[
                    "مرحبا بالعالم، هذه فقرة طويلة من النص العربي لاختبار التخطيط والتشكيل.",
                    "שלום עולם, זוהי פסקה ארוכה של טקסט בעברית לבדיקת הפריסה והעיצוב.",
                    "نص عربي مع كلمات English مختلطة و 12345 أرقام وعلامات ترقيم (قوسين).",
                ],
                6_000,
            ),
            false,
        ),
        sample(
            "mixed directions",
            repeat_to(
                &[
                    "The word שלום means hello and مرحبا too, so a line runs both ways here.",
                    "Numbers 123 and العربية 456 and English text again (mixed) with more words.",
                ],
                6_000,
            ),
            false,
        ),
        sample(
            "Thai and Indic",
            repeat_to(
                &[
                    "ภาษาไทยไม่มีช่องว่างระหว่างคำ แต่ มี ช่องว่าง ระหว่าง ประโยค ได้ กี ก้ ที่",
                    "हिन्दी में लिखा गया एक लंबा अनुच्छेद, जिसमें संयुक्ताक्षर और मात्राएँ हैं क्षत्रिय ज्ञान।",
                    "తెలుగు వచనం మరియు தமிழ் உரை with English words mixed",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "combining marks",
            repeat_to(
                &[
                    "e\u{301} a\u{300} o\u{302} n\u{303} u\u{308} \u{301}combining after space",
                    "x\u{20d7} y\u{0332} z\u{0345} plain words around the marks and more of them",
                ],
                6_000,
            ),
            true,
        ),
        sample(
            "tabs",
            repeat_to(&["tab\tseparated\tcolumns\tof text that run on and on"], 6_000),
            false,
        ),
        sample(
            "one long word",
            format!("{} end", "supercalifragilistic".repeat(600)),
            true,
        ),
        sample(
            "spaces only",
            " ".repeat(6_000),
            false,
        ),
    ]
}

fn run_all(pair: &Pair, system: &str) {
    let fonts = [
        font(SANS_FAMILY),
        font(SANS_FAMILY).bold(),
        font(MONO_FAMILY),
    ];
    for sample in samples() {
        let Sample {
            name,
            text,
            segmentable,
        } = &sample;
        // One font, and fonts that change every so often in the middle of the text, in words
        // and in the middle of a word.
        for step in [usize::MAX, 997, 61] {
            let step = step.min(text.len());
            let runs = text_runs(text, &fonts[..if step == text.len() { 1 } else { 3 }], step);
            let context = format!("{system}, {name}, font runs every {step} bytes");
            let segments = check(pair, text, &runs, &context);
            assert_eq!(!segments.is_empty(), *segmentable, "{context}");
        }
        // The text cut off at different places, as when it is typed.
        for len in [1500, 3000, text.len() - 1] {
            let mut end = len.min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let text = &text[..end];
            let runs = text_runs(text, &fonts[..2], 500);
            check(pair, text, &runs, &format!("{system}, {name}, {end} bytes"));
        }
    }
}

#[test]
fn a_line_in_segments_has_the_layout_of_the_whole_line() {
    run_all(&Pair::bundled(), "bundled fonts");
}

#[test]
fn a_line_in_segments_has_the_layout_of_the_whole_line_with_system_fonts() {
    run_all(&Pair::with_system_fonts(), "system fonts");
}

#[test]
fn a_wrapped_paragraph_wraps_where_the_whole_line_does() {
    let pair = Pair::with_system_fonts();
    let fonts = [font(SANS_FAMILY), font(SANS_FAMILY).bold()];
    for sample in samples() {
        let text = SharedString::from(sample.text.clone());
        let runs = text_runs(&text, &fonts, 700);
        for wrap_width in [333., 1010.] {
            let wrap = |delegate: &Arc<Delegate>| {
                fresh(delegate)
                    .shape_text(text.clone(), px(15.), &runs, Some(px(wrap_width)), None)
                    .unwrap()
            };
            let whole = wrap(&pair.whole);
            let segmented = wrap(&pair.segmented);
            assert_eq!(whole.len(), segmented.len());
            for (whole, segmented) in whole.iter().zip(&segmented) {
                assert_eq!(
                    whole.wrap_boundaries, segmented.wrap_boundaries,
                    "{}, wrapped to {wrap_width}",
                    sample.name
                );
            }
        }
    }
}

#[test]
fn changing_a_paragraph_shapes_a_few_segments() {
    let pair = Pair::bundled();
    let text_system = fresh(&pair.segmented);
    let fonts = [font(SANS_FAMILY)];
    let text = repeat_to(
        &["The quick brown fox jumps over the lazy dog (really)."],
        20_000,
    );
    let layout = |text: &str| {
        let runs = text_runs(text, &fonts, usize::MAX);
        text_system.layout_line(text, px(15.), &runs, None);
        std::mem::take(&mut *pair.segmented.shaped.lock().unwrap())
    };
    let first = layout(&text);
    assert!(first.len() > 20, "{first:?}");

    let mut appended = text;
    appended.push_str("another word ");
    assert!(layout(&appended).len() <= 2);

    let mut edited = appended.clone();
    edited.insert_str(edited[..edited.len() / 2].rfind(' ').unwrap(), " inserted");
    let shaped = layout(&edited);
    assert!((1..=4).contains(&shaped.len()), "{shaped:?}");
    assert!(shaped.iter().sum::<usize>() < 4000);
}
