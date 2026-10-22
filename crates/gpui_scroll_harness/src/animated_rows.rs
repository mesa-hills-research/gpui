//! The frame cost of one animating row in a list of 1,000 mixed rows, with the rows as elements
//! of the view that has the list, as cached views of the heights an app could know, and as cached
//! views of measured height. It prints the times of frames in a release build:
//!
//! ```sh
//! cargo test -p mhr-gpui-scroll-harness --release animated_rows -- --ignored --nocapture
//! ```

use std::{cell::Cell, rc::Rc, time::Instant};

use gpui::{
    AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, ListAlignment, ListOffset,
    ListState, ParentElement as _, Render, StyleRefinement, Styled as _, Window, div, hsla, list,
    px, rgb, size,
};
use gpui_screenshot::{SANS_FAMILY, ScreenshotApp};

use crate::{
    run::{draw, measure},
    transcript::{Row, render_row, transcript},
    view::OVERDRAW,
};

const WIDTH: f32 = 800.;

/// A row as its own view.
struct RowView {
    row: Row,
    ix: usize,
    animating: bool,
    tick: usize,
    renders: Rc<Cell<usize>>,
}

impl Render for RowView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        let element = render_row(&self.row, self.ix);
        if self.animating {
            // The spinner of the row, which changes every frame.
            div().relative().child(element).child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .size(px(14.))
                    .rounded_full()
                    .bg(hsla((self.tick % 60) as f32 / 60., 0.8, 0.5, 1.)),
            )
        } else {
            div().child(element)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// Rows are elements of the list's owner, which the animation redraws.
    Plain,
    /// Rows are cached views of the exact heights an app could know.
    CachedFixed,
    /// Rows are cached views of measured heights.
    CachedMeasured,
}

struct Transcript {
    mode: Mode,
    list: ListState,
    rows: Rc<Vec<Row>>,
    heights: Rc<Vec<f32>>,
    views: Vec<Entity<RowView>>,
    animating_ix: usize,
    tick: usize,
    renders: Rc<Cell<usize>>,
}

impl Render for Transcript {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let heights = self.heights.clone();
        let views = self.views.clone();
        let renders = self.renders.clone();
        let tick = self.tick;
        let animating_ix = self.animating_ix;
        let mode = self.mode;
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .font_family(SANS_FAMILY)
            .child(
                list(self.list.clone(), move |ix, _, _| match mode {
                    Mode::Plain => {
                        renders.set(renders.get() + 1);
                        let element = render_row(&rows[ix], ix);
                        if ix == animating_ix {
                            div()
                                .relative()
                                .child(element)
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .right_0()
                                        .size(px(14.))
                                        .rounded_full()
                                        .bg(hsla((tick % 60) as f32 / 60., 0.8, 0.5, 1.)),
                                )
                                .into_any_element()
                        } else {
                            element
                        }
                    }
                    Mode::CachedFixed => views[ix]
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(heights[ix])))
                        .into_any_element(),
                    Mode::CachedMeasured => views[ix]
                        .clone()
                        .cached_measured(StyleRefinement::default().w_full())
                        .into_any_element(),
                })
                .size_full(),
            )
    }
}

/// The row to start at that has the most rows in `viewport`, and how many that is.
fn busiest(heights: &[f32], viewport: f32) -> (usize, usize) {
    let mut best = (0, 0);
    for start in 0..heights.len() {
        let mut filled = 0.;
        let mut count = 0;
        for height in &heights[start..] {
            if filled >= viewport {
                break;
            }
            filled += height;
            count += 1;
        }
        if count > best.1 {
            best = (start, count);
        }
    }
    best
}

fn run(
    mode: Mode,
    rows: &Rc<Vec<Row>>,
    heights: &Rc<Vec<f32>>,
    viewport: f32,
) -> (f64, f64, f64, usize, usize) {
    let (start, visible) = busiest(heights, viewport);
    let animating_ix = start + visible / 2;
    let mut app = ScreenshotApp::new().unwrap();
    let renders = Rc::new(Cell::new(0));
    let row_renders = Rc::new(Cell::new(0));
    let window = app
        .open_window(size(px(WIDTH), px(viewport)), 1., |_, cx| {
            let views = rows
                .iter()
                .enumerate()
                .map(|(ix, row)| {
                    let row = row.clone();
                    let renders = row_renders.clone();
                    cx.new(|_| RowView {
                        row,
                        ix,
                        animating: ix == animating_ix,
                        tick: 0,
                        renders,
                    })
                })
                .collect();
            cx.new(|_| Transcript {
                mode,
                list: {
                    let list = ListState::new(rows.len(), ListAlignment::Top, px(OVERDRAW));
                    list.scroll_to(ListOffset {
                        item_ix: start,
                        offset_in_item: px(0.),
                    });
                    list
                },
                rows: rows.clone(),
                heights: heights.clone(),
                views,
                animating_ix,
                tick: 0,
                renders: renders.clone(),
            })
        })
        .unwrap();
    let any: AnyWindowHandle = window.into();
    draw(&mut app, any).unwrap();
    draw(&mut app, any).unwrap();

    let views: Vec<Entity<RowView>> = app.update(|cx| window.read(cx).unwrap().views.clone());
    let animating = views[animating_ix].clone();

    // In test mode the update's effect flush draws the window, so the update is the frame.
    let step = |app: &mut ScreenshotApp, n: usize| -> f64 {
        let started = Instant::now();
        match mode {
            Mode::Plain => {
                app.update_window(any, |view, _, cx| {
                    let transcript = view.downcast::<Transcript>().unwrap();
                    transcript.update(cx, |transcript, cx| {
                        transcript.tick = n;
                        cx.notify();
                    });
                })
                .unwrap();
            }
            Mode::CachedFixed | Mode::CachedMeasured => {
                app.update(|cx| {
                    animating.update(cx, |row, cx| {
                        row.tick = n;
                        cx.notify();
                    })
                });
            }
        }
        started.elapsed().as_secs_f64() * 1000.
    };
    for n in 0..30 {
        step(&mut app, n);
    }
    let renders_before = renders.get() + row_renders.get();
    const FRAMES: usize = 300;
    let mut times: Vec<f64> = (0..FRAMES).map(|n| step(&mut app, 100 + n)).collect();
    let renders_per_frame = (renders.get() + row_renders.get() - renders_before) / FRAMES;
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    (
        mean,
        times[times.len() / 2],
        times[times.len() * 9 / 10],
        renders_per_frame,
        visible,
    )
}

#[test]
#[ignore]
fn animated_rows() {
    let rows = Rc::new(transcript(120, 7));
    println!("{} rows", rows.len());
    let mut app = ScreenshotApp::new().unwrap();
    let heights = Rc::new(measure(&mut app, &rows, WIDTH).unwrap());
    drop(app);
    for viewport in [600., 1200.] {
        for mode in [
            Mode::Plain,
            Mode::CachedFixed,
            Mode::CachedMeasured,
            Mode::Plain,
            Mode::CachedFixed,
            Mode::CachedMeasured,
        ] {
            let (mean, p50, p90, renders, visible) = run(mode, &rows, &heights, viewport);
            println!(
                "viewport {viewport} ({visible} rows) {mode:?}: frame mean {mean:.3} ms, p50 {p50:.3}, p90 {p90:.3}, row renders per frame {renders}"
            );
        }
    }
}
