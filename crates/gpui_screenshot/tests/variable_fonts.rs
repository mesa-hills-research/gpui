//! Text in a variable font: each requested weight is drawn and laid out at that point of the
//! font's `wght` axis. The fixture is Figtree, whose axis runs from 300 to 900 with its default
//! instance at 300, in an upright and an italic file.
#![cfg(target_os = "linux")]

use std::borrow::Cow;

use gpui::{
    Font, FontStyle, FontWeight, IntoElement, ParentElement as _, Styled as _, div, font, px, rgb,
    size,
};
use gpui_screenshot::{ScreenshotApp, goldens};
use image::RgbaImage;

const FAMILY: &str = "Figtree";
const UPRIGHT: &[u8] = include_bytes!("fonts/Figtree[wght].ttf");
const ITALIC: &[u8] = include_bytes!("fonts/Figtree-Italic[wght].ttf");
const SAMPLE: &str = "Hamburgefonstiv 0123";

const SCALE: f32 = 2.;
const ROW_HEIGHT: f32 = 30.;

/// The rows of the golden, top to bottom.
const ROWS: &[(f32, FontStyle)] = &[
    (300., FontStyle::Normal),
    (400., FontStyle::Normal),
    (500., FontStyle::Normal),
    (600., FontStyle::Normal),
    (700., FontStyle::Normal),
    (800., FontStyle::Normal),
    (900., FontStyle::Normal),
    (400., FontStyle::Italic),
    (700., FontStyle::Italic),
];

fn app() -> ScreenshotApp {
    let mut app = ScreenshotApp::new().unwrap();
    app.add_fonts(vec![Cow::Borrowed(UPRIGHT), Cow::Borrowed(ITALIC)])
        .unwrap();
    app
}

fn figtree(weight: f32, style: FontStyle) -> Font {
    Font {
        weight: FontWeight(weight),
        style,
        ..font(FAMILY)
    }
}

fn rows() -> impl IntoElement {
    div()
        .size_full()
        .bg(rgb(0xffffff))
        .text_color(rgb(0x000000))
        .font_family(FAMILY)
        .text_size(px(20.))
        .children(ROWS.iter().map(|&(weight, style)| {
            let row = div()
                .h(px(ROW_HEIGHT))
                .px_2()
                .font_weight(FontWeight(weight))
                .child(SAMPLE);
            match style {
                FontStyle::Normal => row,
                _ => row.italic(),
            }
        }))
}

/// The darkness summed over the pixels of one row, 255 for each fully black pixel.
fn ink(image: &RgbaImage, row: usize) -> u64 {
    let height = (ROW_HEIGHT * SCALE) as u32;
    let top = row as u32 * height;
    (top..top + height)
        .flat_map(|y| (0..image.width()).map(move |x| (x, y)))
        .map(|(x, y)| {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            255 - (u64::from(r) + u64::from(g) + u64::from(b)) / 3
        })
        .sum()
}

/// Every weight draws with more ink than the one before it, in one window, so Regular and Bold
/// glyphs also stay apart in the glyph cache and the atlas.
#[test]
fn heavier_weights_draw_more_ink() {
    let mut app = app();
    let shot = app
        .render_element(
            size(px(300.), px(ROW_HEIGHT * ROWS.len() as f32)),
            SCALE,
            |_, _| rows(),
        )
        .unwrap();

    let inks: Vec<u64> = (0..ROWS.len()).map(|row| ink(&shot.image, row)).collect();
    for ((weight, style), ink) in ROWS.iter().zip(&inks) {
        eprintln!("{weight} {style:?}: {ink}");
    }
    for pair in [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (7, 8)] {
        assert!(
            inks[pair.1] > inks[pair.0] * 21 / 20,
            "{:?} draws with {} ink, {:?} with {}",
            ROWS[pair.1],
            inks[pair.1],
            ROWS[pair.0],
            inks[pair.0],
        );
    }
    goldens!().assert("variable-font-weights", &shot);
}

/// The advance of `m` at `weight` in font units, read from the font's own tables, and the
/// font's units per em.
fn expected_advance(data: &[u8], weight: f32) -> (f32, f32) {
    let font = swash::FontRef::from_index(data, 0).unwrap();
    let coords: Vec<_> = font
        .variations()
        .normalized_coords([(swash::tag_from_bytes(b"wght"), weight)])
        .collect();
    let glyph = font.charmap().map('m');
    let advance = font.glyph_metrics(&coords).advance_width(glyph);
    (advance, f32::from(font.metrics(&coords).units_per_em))
}

/// Shaping and the per-glyph advance both use the requested weight.
#[test]
fn advances_follow_the_weight() {
    let mut app = app();
    let font_size = px(100.);
    let text_system = app.update(|cx| cx.text_system().clone());

    for (data, style) in [(UPRIGHT, FontStyle::Normal), (ITALIC, FontStyle::Italic)] {
        let mut advances = Vec::new();
        for weight in [300., 400., 700., 900.] {
            let font_id = text_system.resolve_font(&figtree(weight, style));
            let (expected, units_per_em) = expected_advance(data, weight);
            let units =
                |width: gpui::Pixels| f32::from(width) * units_per_em / f32::from(font_size);
            let advance = units(text_system.advance(font_id, font_size, 'm').unwrap().width);
            let shaped = units(text_system.layout_width(font_id, font_size, 'm'));
            eprintln!(
                "{weight} {style:?}: expected {expected}, advance {advance}, shaped {shaped} units"
            );
            assert!(
                (advance - expected).abs() < 0.01,
                "{weight} {style:?}: advance {advance}, expected {expected}",
            );
            // The shaper rounds varied advances to whole units, as HarfBuzz does.
            assert!(
                (shaped - expected).abs() <= 0.5,
                "{weight} {style:?}: shaped width {shaped}, expected {expected}",
            );
            advances.push(advance);
        }
        // The fixture's advances grow with the weight, so the checks above tell weights apart.
        assert!(
            advances.windows(2).all(|pair| pair[1] > pair[0] + 1.),
            "{advances:?}"
        );
    }
}

/// Weights outside the axis are clamped to it, and the same weight always gets the same font.
#[test]
fn weights_outside_the_axis_are_clamped() {
    let mut app = app();
    let text_system = app.update(|cx| cx.text_system().clone());
    let font_id = |weight| text_system.resolve_font(&figtree(weight, FontStyle::Normal));

    assert_eq!(font_id(100.), font_id(300.));
    assert_eq!(font_id(700.), font_id(700.));
    assert_ne!(font_id(400.), font_id(700.));
    assert_ne!(
        font_id(700.),
        text_system.resolve_font(&figtree(700., FontStyle::Italic))
    );
}
