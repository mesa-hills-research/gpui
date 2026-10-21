//! Pictures of a run, a chart of the thumb over every step and a
//! filmstrip of frames drawn while the wheel scrolls, as a contact sheet
//! and a GIF.

use std::{fs::File, io::BufWriter, path::Path, sync::Arc};

use anyhow::{Context as _, Result};
use gpui::{
    AppContext as _, Bounds, Context, Hsla, IntoElement, ParentElement as _, Pixels, Render,
    RenderImage, SharedString, Styled as _, Window, canvas, div, fill, img, point, px, rgb, size,
};
use gpui_screenshot::{MONO_FAMILY, SANS_FAMILY, ScreenshotApp};
use image::RgbaImage;

use crate::run::{Run, Shot, Step};
use crate::view::VIEWPORT;

/// The kymograph's height, the track at half its size.
const KYMO: f32 = VIEWPORT / 2.;
/// The ratio plot's height, and the largest ratio it shows.
const RATIO_HEIGHT: f32 = 180.;
const RATIO_TOP: f32 = 3.;

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

/// The steps of each leg of a run, and their exact travel, for the chart.
#[derive(Clone)]
pub struct ChartLeg {
    pub name: String,
    pub up: bool,
    pub steps: Vec<Step>,
    pub exact_travel: f32,
}

pub fn chart_legs(run: &Run, exact_travel: &[f32]) -> Vec<ChartLeg> {
    run.legs
        .iter()
        .zip(exact_travel)
        .map(|(leg, exact)| ChartLeg {
            name: leg.leg.name(),
            up: leg.leg.up,
            steps: leg.steps.clone(),
            exact_travel: *exact,
        })
        .collect()
}

/// Two plots over the steps of every leg, side by side. Above, the
/// thumb's place on its track step by step (gray), with the place exact
/// heights give it (red). Below, each step's travel as a share of the
/// exact travel, with 1 marked in green.
pub fn chart(legs: Vec<ChartLeg>, width: f32) -> impl IntoElement {
    let labels = div()
        .flex()
        .w(px(width))
        .font_family(MONO_FAMILY)
        .text_size(px(12.))
        .text_color(color(0x57606a))
        .children(legs.iter().map(|leg| {
            div().flex_1().child(format!(
                "{}: {} steps",
                leg.name,
                leg.steps.len().saturating_sub(1)
            ))
        }));
    let plot = canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let total: usize = legs.iter().map(|leg| leg.steps.len()).sum();
            let step_width = f32::from(bounds.size.width) / total.max(1) as f32;
            let dot = step_width.max(1.);
            let left = bounds.left();
            let top = bounds.top();
            let ratio_top = top + px(KYMO + 16.);
            let ratio_y = |ratio: f32| {
                ratio_top + px(RATIO_HEIGHT * (1. - (ratio / RATIO_TOP).clamp(0., 1.)))
            };
            window.paint_quad(fill(
                Bounds::new(point(left, top), size(bounds.size.width, px(KYMO))),
                color(0xf6f8fa),
            ));
            window.paint_quad(fill(
                Bounds::new(
                    point(left, ratio_top),
                    size(bounds.size.width, px(RATIO_HEIGHT)),
                ),
                color(0xf6f8fa),
            ));
            for line in [1., 2.] {
                window.paint_quad(fill(
                    Bounds::new(point(left, ratio_y(line)), size(bounds.size.width, px(1.))),
                    color(if line == 1. { 0x2da44e } else { 0xd0d7de }),
                ));
            }
            let mut x = 0.;
            for leg in &legs {
                let direction = if leg.up { -1. } else { 1. };
                for (ix, step) in leg.steps.iter().enumerate() {
                    let at = left + px(x);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(at, top + px(step.thumb.top / 2.)),
                            size(px(dot), px(step.thumb.length / 2.)),
                        ),
                        color(0x8c959f),
                    ));
                    window.paint_quad(fill(
                        Bounds::new(
                            point(at, top + px(step.exact.top / 2.)),
                            size(px(dot), px(1.)),
                        ),
                        color(0xcf222e),
                    ));
                    if ix > 0 && ix + 1 < leg.steps.len() && leg.exact_travel > 0. {
                        let travel = (step.thumb.top - leg.steps[ix - 1].thumb.top) * direction;
                        let ratio = travel / leg.exact_travel;
                        window.paint_quad(fill(
                            Bounds::new(point(at, ratio_y(ratio) - px(1.)), size(px(dot), px(2.))),
                            color(if ratio < 0. { 0xcf222e } else { 0x0969da }),
                        ));
                    }
                    x += step_width;
                }
                window.paint_quad(fill(
                    Bounds::new(
                        point(left + px(x) - px(1.), top),
                        size(px(1.), px(KYMO + 16. + RATIO_HEIGHT)),
                    ),
                    color(0x24292f),
                ));
            }
        },
    )
    .w(px(width))
    .h(px(KYMO + 16. + RATIO_HEIGHT));
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(labels)
        .child(plot)
        .child(
            div()
                .font_family(SANS_FAMILY)
                .text_size(px(12.))
                .text_color(color(0x57606a))
                .child(
                    "Above, the thumb on its track at each step (gray) and where exact heights put it (red). \
                     Below, each step's travel / exact travel (blue, red below 0), with lines at 1 (green) and 2 (gray).",
                ),
        )
}

struct ChartView {
    title: SharedString,
    legs: Vec<ChartLeg>,
    width: f32,
}

impl Render for ChartView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x1f2328))
            .child(
                div()
                    .font_family(SANS_FAMILY)
                    .text_size(px(16.))
                    .child(self.title.clone()),
            )
            .child(chart(self.legs.clone(), self.width))
    }
}

pub const CHART_WIDTH: f32 = 1600.;

/// Writes the chart of a run to `path`.
pub fn write_chart(
    app: &mut ScreenshotApp,
    title: String,
    legs: Vec<ChartLeg>,
    path: &Path,
) -> Result<()> {
    let height = 16. * 2. + 24. + 20. + KYMO + 16. + RATIO_HEIGHT + 40.;
    let shot = app.render_view(size(px(CHART_WIDTH + 32.), px(height)), 1., |_, cx| {
        cx.new(|_| ChartView {
            title: title.into(),
            legs,
            width: CHART_WIDTH,
        })
    })?;
    shot.image
        .save(path)
        .with_context(|| format!("writing {}", path.display()))
}

/// A frame of the filmstrip and its caption.
pub struct Still {
    pub image: RgbaImage,
    pub caption: String,
}

fn render_image(image: &RgbaImage) -> Arc<RenderImage> {
    let mut bgra = image.clone();
    for pixel in bgra.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(bgra)]))
}

const COLUMNS: usize = 6;
const STILL_SCALE: f32 = 0.5;

struct Sheet {
    title: SharedString,
    about: SharedString,
    stills: Vec<(Arc<RenderImage>, SharedString)>,
    still_size: (f32, f32),
    legs: Vec<ChartLeg>,
    report: SharedString,
    width: f32,
}

impl Render for Sheet {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (still_width, still_height) = self.still_size;
        div()
            .size_full()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .bg(rgb(0xffffff))
            .text_color(rgb(0x1f2328))
            .font_family(SANS_FAMILY)
            .child(div().text_size(px(20.)).child(self.title.clone()))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(0x57606a))
                    .child(self.about.clone()),
            )
            .child(
                div()
                    .w(px(self.width))
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(self.stills.iter().map(|(image, caption)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                img(image.clone())
                                    .w(px(still_width))
                                    .h(px(still_height))
                                    .border_1()
                                    .border_color(rgb(0xd0d7de)),
                            )
                            .child(
                                div()
                                    .w(px(still_width))
                                    .font_family(MONO_FAMILY)
                                    .text_size(px(11.))
                                    .line_height(px(14.))
                                    .child(caption.clone()),
                            )
                    })),
            )
            .child(chart(self.legs.clone(), self.width))
            .child(
                div()
                    .font_family(MONO_FAMILY)
                    .text_size(px(12.))
                    .line_height(px(15.))
                    .whitespace_nowrap()
                    .child(self.report.clone()),
            )
    }
}

/// Writes a contact sheet of `stills`, with the chart of the run and its
/// report under them, to `path`.
pub fn write_sheet(
    app: &mut ScreenshotApp,
    title: String,
    about: String,
    stills: &[Still],
    legs: Vec<ChartLeg>,
    report: String,
    path: &Path,
) -> Result<()> {
    let first = stills.first().context("no stills to show")?;
    let still_size = (
        first.image.width() as f32 * STILL_SCALE,
        first.image.height() as f32 * STILL_SCALE,
    );
    let rows = stills.len().div_ceil(COLUMNS);
    let width = COLUMNS as f32 * (still_size.0 + 8.) - 8.;
    let report_lines = report.lines().count() as f32;
    let height = 16. * 2.
        + 28.
        + 20.
        + rows as f32 * (still_size.1 + 2. + 30. + 8.)
        + 24.
        + 20.
        + KYMO
        + 16.
        + RATIO_HEIGHT
        + 40.
        + report_lines * 15.
        + 48.;
    let stills = stills
        .iter()
        .map(|still| {
            (
                render_image(&still.image),
                SharedString::from(still.caption.clone()),
            )
        })
        .collect();
    let shot = app.render_view(size(px(width + 32.), px(height)), 1., |_, cx| {
        cx.new(|_| Sheet {
            title: title.into(),
            about: about.into(),
            stills,
            still_size,
            legs,
            report: report.into(),
            width,
        })
    })?;
    shot.image
        .save(path)
        .with_context(|| format!("writing {}", path.display()))
}

/// Halves an image's size, averaging each square of four pixels.
fn half(image: &RgbaImage) -> RgbaImage {
    let (width, height) = (image.width() / 2, image.height() / 2);
    RgbaImage::from_fn(width, height, |x, y| {
        let mut sum = [0u32; 4];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let pixel = image.get_pixel(x * 2 + dx, y * 2 + dy);
            for channel in 0..4 {
                sum[channel] += pixel.0[channel] as u32;
            }
        }
        image::Rgba(sum.map(|value| (value / 4) as u8))
    })
}

/// A palette of a 6×6×6 color cube and 40 grays, which suits flat UI
/// colors and anti-aliased text, and the index of a pixel's nearest color.
fn palette() -> Vec<u8> {
    let mut colors = Vec::with_capacity(256 * 3);
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                colors.extend([r * 51, g * 51, b * 51]);
            }
        }
    }
    for gray in 0..40u32 {
        let value = (gray * 255 / 39) as u8;
        colors.extend([value, value, value]);
    }
    colors
}

fn index_of(pixel: [u8; 4]) -> u8 {
    let [r, g, b, _] = pixel;
    let (low, high) = (r.min(g).min(b), r.max(g).max(b));
    if high - low < 10 {
        let gray = (r as u32 + g as u32 + b as u32) / 3;
        216 + ((gray * 39 + 127) / 255) as u8
    } else {
        let level = |value: u8| (value as u32 + 25) / 51;
        (level(r) * 36 + level(g) * 6 + level(b)) as u8
    }
}

/// Writes `frames` at half size as a looping GIF, `delay_ms` apart.
pub fn write_gif(frames: &[RgbaImage], delay_ms: u16, path: &Path) -> Result<()> {
    let first = frames.first().context("no frames for the GIF")?;
    let (width, height) = (first.width() / 2, first.height() / 2);
    let file =
        BufWriter::new(File::create(path).with_context(|| format!("writing {}", path.display()))?);
    let mut encoder = gif::Encoder::new(file, width as u16, height as u16, &palette())?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    for frame in frames {
        let small = half(frame);
        let indices: Vec<u8> = small.pixels().map(|pixel| index_of(pixel.0)).collect();
        let mut gif_frame =
            gif::Frame::from_indexed_pixels(width as u16, height as u16, indices, None);
        gif_frame.delay = delay_ms / 10;
        encoder.write_frame(&gif_frame)?;
    }
    Ok(())
}

/// The caption of the step `shot` is at, for its frame.
pub fn caption(shot: &Shot, up: bool, exact_travel: f32) -> String {
    let step = &shot.steps[shot.step];
    let before = &shot.steps[shot.step - 1];
    let direction = if up { -1. } else { 1. };
    let travel = (step.thumb.top - before.thumb.top) * direction;
    format!(
        "step {:>4}  thumb {:>6.1} (exact {:>6.1})  travel {:>5.2}px ×{:>4.2}  length {:>4.1}",
        shot.step,
        step.thumb.top,
        step.exact.top,
        travel,
        if exact_travel > 0. {
            travel / exact_travel
        } else {
            0.
        },
        step.thumb.length,
    )
}
