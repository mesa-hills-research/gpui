//! A list laid out by queued moves, as a chat transcript lays out the end it follows: the lines
//! never move down by themselves, blank space below the end stays until what comes fills it, a
//! scroll down stops where the end meets the bottom edge, and every move is a whole number of
//! device pixels. Each case runs at pixel ratios 1, 1.25, 1.5 and 2, with deltas in device
//! pixels.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use crate::{
    self as gpui, AnyWindowHandle, App, AppContext as _, Context, Follow, IntoElement as _,
    ListFrame, ListMove, ListState, Render, ScrollDelta, ScrollGesture, ScrollWheelEvent, Styled,
    TestAppContext, TouchPhase, VisualContext as _, VisualTestContext, Window, div, list, point,
    px, size,
};

use super::ListAlignment;

const SCALES: [f32; 4] = [1., 1.25, 1.5, 2.];
const WIDTH: f32 = 400.;
/// The view's height, a whole number of device pixels at every ratio.
const HEIGHT: f32 = 400.;

/// The reader's choice, follow or read, as an app keeps it, set by what each move did.
fn follower(choice: Rc<Cell<bool>>) -> impl FnMut(&ListFrame<'_>, &mut Window, &mut App) -> Follow {
    move |frame, _, _| {
        let was = choice.get();
        for effect in frame.moves {
            match effect.what {
                ListMove::Scroll { delta, .. } => {
                    if effect.moved > 0 {
                        choice.set(false);
                    } else if delta > px(0.) && effect.end_shows {
                        choice.set(true);
                    }
                }
                ListMove::Thumb { .. } => {
                    if effect.moved > 0 {
                        choice.set(false);
                    } else if effect.end_shows {
                        choice.set(true);
                    }
                }
                ListMove::ToEnd => choice.set(true),
                ListMove::ToStart | ListMove::To(_) | ListMove::Reveal(_) => {
                    if effect.moved != 0 {
                        choice.set(false);
                    }
                }
                _ => {}
            }
        }
        let shows = if frame.moves.is_empty() {
            frame.end_showed
        } else {
            frame.end_shows
        };
        match (choice.get(), was) {
            (true, _) if frame.width_changed => Follow::ToEnd,
            (true, false) if shows => Follow::Again,
            (true, true) if shows => Follow::Yes,
            _ => Follow::No,
        }
    }
}

/// The view that draws the list.
struct TestList {
    state: ListState,
    heights: Rc<RefCell<Vec<f32>>>,
    choice: Rc<Cell<bool>>,
    padding: f32,
}

impl Render for TestList {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        let heights = self.heights.clone();
        list(self.state.clone(), move |ix, _, _| {
            let height = heights.borrow().get(ix).copied().unwrap_or(0.);
            div().h(px(height)).w_full().into_any_element()
        })
        .follow_end(follower(self.choice.clone()))
        .w_full()
        .h_full()
        .pt(px(self.padding))
        .pb(px(self.padding))
    }
}

/// A list of rows of `heights`, drawn `HEIGHT` tall at a pixel ratio.
struct Rows {
    cx: VisualTestContext,
    state: ListState,
    heights: Rc<RefCell<Vec<f32>>>,
    choice: Rc<Cell<bool>>,
    scale: f32,
    height: f32,
    padding: f32,
}

impl Rows {
    fn new(cx: &mut TestAppContext, heights: Vec<f32>, scale: f32) -> Self {
        let window: AnyWindowHandle = cx.add_empty_window().window_handle();
        cx.simulate_window_scale_factor_change(window, scale);
        let cx = VisualTestContext::from_window(window, cx);
        let state = ListState::new(heights.len(), ListAlignment::Top, px(100.)).with_queued_moves();
        state.queue_move(ListMove::ToEnd);
        let mut rows = Self {
            cx,
            state,
            heights: Rc::new(RefCell::new(heights)),
            choice: Rc::new(Cell::new(true)),
            scale,
            height: HEIGHT,
            padding: 0.,
        };
        rows.draw();
        rows
    }

    fn draw(&mut self) {
        let view = TestList {
            state: self.state.clone(),
            heights: self.heights.clone(),
            choice: self.choice.clone(),
            padding: self.padding,
        };
        let view = self.cx.update(|_, cx| cx.new(|_| view));
        self.cx.draw(
            point(px(0.), px(0.)),
            size(px(WIDTH), px(self.height)),
            |_, _| view.into_any_element(),
        );
    }

    /// Each row's top on the screen, in device pixels, from the first drawn.
    fn tops(&self) -> Vec<(usize, f64)> {
        let mut tops = Vec::new();
        let mut ix = self.state.logical_scroll_top().item_ix;
        while let Some(bounds) = self.state.bounds_for_item(ix) {
            let top = f32::from(bounds.top()) as f64 * self.scale as f64;
            assert!(
                (top - top.round()).abs() < 0.01,
                "at {}: row {ix} at {top}, between device pixels",
                self.scale
            );
            tops.push((ix, top.round()));
            if f32::from(bounds.top()) > self.height {
                break;
            }
            ix += 1;
        }
        tops
    }

    /// The top of row `ix` on the screen, in device pixels.
    fn top(&self, ix: usize) -> f64 {
        self.tops()
            .into_iter()
            .find(|(row, _)| *row == ix)
            .map(|(_, top)| top)
            .unwrap_or_else(|| panic!("row {ix} isn't drawn"))
    }

    /// The blank below the end, in device pixels.
    fn blank(&self) -> f64 {
        let last = self.heights.borrow().len() - 1;
        let bottom = self.top(last) + (self.heights.borrow()[last] * self.scale) as f64;
        ((self.height - self.padding) * self.scale) as f64 - bottom
    }

    /// Scrolls by `delta` device pixels, down the list when positive, in `gesture`.
    fn scroll(&mut self, delta: f64, gesture: ScrollGesture) {
        self.state.queue_move(ListMove::Scroll {
            delta: px((delta / self.scale as f64) as f32),
            gesture,
        });
    }

    /// Every row drawn starts on a whole device pixel, which [`Self::tops`] checks.
    fn assert_whole(&self) {
        self.tops();
    }
}

/// A gesture's event, `ms` into it.
fn moving(ms: u64) -> ScrollGesture {
    ScrollGesture {
        starts: false,
        ends: false,
        coasting: false,
        at: Some(scheduler::Instant::now() + Duration::from_millis(ms)),
    }
}

fn turns(count: usize) -> Vec<f32> {
    // Heights that are whole device pixels at 1.25 and 1.5 (multiples of 4).
    (0..count).map(|ix| [24., 48., 72., 36.][ix % 4]).collect()
}

#[gpui::test]
fn a_shorter_end_leaves_blank_and_moves_no_line(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(40), scale);
        assert!(rows.choice.get());
        let before = rows.tops();
        let thumb = rows.state.scroll_px_offset_for_scrollbar();
        // The end gets 24 px shorter.
        let last = rows.heights.borrow().len() - 1;
        rows.heights.borrow_mut()[last] -= 24.;
        rows.state.remeasure_items(last..last + 1);
        rows.draw();
        assert_eq!(rows.tops(), before, "at {scale}");
        assert_eq!(rows.blank(), (24. * scale) as f64, "at {scale}");
        assert!(rows.state.report().end_shows);
        assert_eq!(
            rows.state.scroll_px_offset_for_scrollbar(),
            thumb,
            "the thumb moved at {scale}"
        );
        // What comes fills the blank first, then moves the lines up.
        rows.heights.borrow_mut()[last] += 12.;
        rows.state.remeasure_items(last..last + 1);
        rows.draw();
        assert_eq!(rows.tops(), before, "at {scale}");
        rows.heights.borrow_mut().push(36.);
        rows.state.splice(last + 1..last + 1, 1);
        rows.draw();
        let moved: Vec<f64> = rows
            .tops()
            .iter()
            .filter_map(|(ix, top)| {
                before
                    .iter()
                    .find(|(b, _)| b == ix)
                    .map(|(_, was)| top - was)
            })
            .collect();
        assert!(
            moved.iter().all(|up| *up == -(24. * scale) as f64),
            "at {scale}: {moved:?}"
        );
        assert_eq!(rows.blank(), 0., "at {scale}");
        rows.assert_whole();
    }
}

#[gpui::test]
fn a_scroll_down_with_blank_below_the_end_moves_nothing_and_follows(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(40), scale);
        let last = rows.heights.borrow().len() - 1;
        rows.heights.borrow_mut()[last] -= 32.;
        rows.state.remeasure_items(last..last + 1);
        rows.draw();
        let before = rows.tops();
        // Up 10 reads and leaves part of the blank. Down 0.5 then follows from there.
        rows.scroll(-10., ScrollGesture::step());
        rows.draw();
        assert!(!rows.choice.get(), "at {scale}");
        assert_eq!(rows.blank(), (32. * scale) as f64 - 10., "at {scale}");
        let read = rows.tops();
        rows.scroll(0.5, ScrollGesture::step());
        rows.draw();
        assert_eq!(rows.tops(), read, "at {scale}");
        assert!(
            rows.choice.get(),
            "a scroll down that leaves the end in sight follows, at {scale}"
        );
        // Down from there moves nothing.
        for _ in 0..5 {
            rows.scroll(40., ScrollGesture::step());
            rows.draw();
        }
        assert_eq!(rows.tops(), read, "at {scale}");
        let _ = before;
    }
}

#[gpui::test]
fn scrolls_below_a_device_pixel_wait_for_the_rest_of_their_gesture(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(40), scale);
        let before = rows.tops();
        // Three scrolls up of 0.4 in one gesture: the third moves the lines down 1.
        for (step, ms) in [(1, 0), (2, 16), (3, 32)] {
            rows.scroll(-0.4, moving(ms));
            rows.draw();
            let moved = rows.tops()[0].1 - before[0].1;
            if step < 3 {
                assert_eq!(moved, 0., "at {scale}, step {step}");
                assert!(rows.choice.get(), "at {scale}, step {step}");
            } else {
                assert_eq!(moved, 1., "at {scale}");
                assert!(!rows.choice.get(), "at {scale}");
            }
        }
        rows.assert_whole();
        // Up 0.4 and down 0.4 again and again moves nothing.
        let mut rows = Rows::new(cx, turns(40), scale);
        let before = rows.tops();
        for ms in 0..10 {
            rows.scroll(-0.4, moving(ms * 32));
            rows.draw();
            rows.scroll(0.4, moving(ms * 32 + 16));
            rows.draw();
        }
        assert_eq!(rows.tops(), before, "at {scale}");
        assert!(rows.choice.get(), "at {scale}");
    }
}

#[gpui::test]
fn a_flick_past_the_end_stops_at_it_and_its_coast_moves_nothing(cx: &mut TestAppContext) {
    for scale in SCALES {
        for past in [0.3, 300.] {
            let mut rows = Rows::new(cx, turns(40), scale);
            rows.scroll(-200., ScrollGesture::step());
            rows.draw();
            assert!(!rows.choice.get());
            rows.scroll(200. + past, ScrollGesture::step());
            rows.draw();
            assert!(rows.choice.get(), "at {scale}, {past} past");
            assert_eq!(rows.blank(), 0., "at {scale}, {past} past");
            let end = rows.tops();
            for ms in 0..30 {
                rows.scroll(
                    5.,
                    ScrollGesture {
                        coasting: true,
                        ..moving(ms * 16)
                    },
                );
                rows.draw();
            }
            assert_eq!(rows.tops(), end, "at {scale}, {past} past");
        }
    }
}

#[gpui::test]
fn the_end_asked_for_drops_the_rest_of_a_coast(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(60), scale);
        // A flick up, and the end clicked as it coasts.
        rows.scroll(-300., moving(0));
        rows.draw();
        rows.scroll(
            -40.,
            ScrollGesture {
                coasting: true,
                ..moving(16)
            },
        );
        rows.draw();
        assert!(!rows.choice.get());
        rows.state.queue_move(ListMove::ToEnd);
        rows.draw();
        let end = rows.tops();
        for ms in 0..30 {
            rows.scroll(
                -20.,
                ScrollGesture {
                    coasting: true,
                    ..moving(32 + ms * 16)
                },
            );
            rows.draw();
        }
        assert_eq!(rows.tops(), end, "at {scale}");
        assert!(rows.choice.get(), "at {scale}");
        // Fingers on the touchpad move the lines again.
        rows.scroll(-20., moving(600));
        rows.draw();
        assert_ne!(rows.tops(), end, "at {scale}");
        assert!(!rows.choice.get(), "at {scale}");
    }
}

#[gpui::test]
fn a_shorter_view_and_the_same_taller_one_put_every_line_back(cx: &mut TestAppContext) {
    for scale in SCALES {
        for blank in [0., 24., 120.] {
            let mut rows = Rows::new(cx, turns(40), scale);
            let last = rows.heights.borrow().len() - 1;
            rows.heights.borrow_mut()[last] += 200.;
            rows.state.remeasure_items(last..last + 1);
            rows.draw();
            rows.heights.borrow_mut()[last] -= blank;
            rows.state.remeasure_items(last..last + 1);
            rows.draw();
            let before = rows.tops();
            for grow in [72., 24.] {
                rows.height = HEIGHT - grow;
                rows.draw();
                rows.height = HEIGHT;
                rows.draw();
                assert_eq!(
                    rows.tops(),
                    before,
                    "at {scale}, {blank} blank, {grow} taller"
                );
            }
        }
    }
}

#[gpui::test]
fn a_taller_view_with_nothing_to_give_back_shows_blank(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(40), scale);
        let before = rows.tops();
        rows.height = HEIGHT + 48.;
        rows.draw();
        assert_eq!(rows.tops(), before, "at {scale}");
        assert_eq!(rows.blank(), (48. * scale) as f64, "at {scale}");
        rows.state.queue_move(ListMove::ToEnd);
        rows.draw();
        assert_eq!(rows.blank(), 0., "at {scale}");
    }
}

#[gpui::test]
fn a_row_kept_on_its_place_keeps_its_top_and_moves_the_rows_above(cx: &mut TestAppContext) {
    for scale in SCALES {
        for reading in [false, true] {
            let mut rows = Rows::new(cx, turns(40), scale);
            if reading {
                rows.scroll(-120., ScrollGesture::step());
                rows.draw();
            }
            let row = rows.tops()[4].0;
            let (top, above) = (rows.top(row), rows.top(row - 1));
            // The row opens, and the space above it grows by 8.
            rows.heights.borrow_mut()[row] += 100.;
            rows.state.remeasure_items(row..row + 1);
            rows.state.queue_move(ListMove::KeepTop {
                ix: row,
                by: px(-8.),
            });
            rows.draw();
            assert_eq!(rows.top(row), top - (8. * scale) as f64, "at {scale}");
            assert_eq!(rows.top(row - 1), above - (8. * scale) as f64, "at {scale}");
            assert_eq!(rows.choice.get(), !reading, "at {scale}");
            rows.assert_whole();
        }
    }
}

#[gpui::test]
fn a_row_above_the_screen_that_grows_moves_no_line(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(60), scale);
        rows.scroll(-400., ScrollGesture::step());
        rows.draw();
        let before = rows.tops();
        let above = before[0].0 - 3;
        rows.heights.borrow_mut()[above] += 37.;
        rows.state.remeasure_items(above..above + 1);
        rows.draw();
        assert_eq!(rows.tops(), before, "at {scale}");
    }
}

#[gpui::test]
fn deltas_in_one_frame_or_in_many_end_on_the_same_lines(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut once = Rows::new(cx, turns(80), scale);
        let mut apart = Rows::new(cx, turns(80), scale);
        for ix in 0..6 {
            once.scroll(-130., moving(ix * 16));
            apart.scroll(-130., moving(ix * 16));
            apart.draw();
        }
        once.draw();
        assert_eq!(once.tops(), apart.tops(), "at {scale}");
        assert_eq!(once.choice.get(), apart.choice.get());
    }
}

#[gpui::test]
fn a_scroll_down_in_the_frame_the_end_grows_stops_at_the_new_end(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(40), scale);
        rows.scroll(-60., ScrollGesture::step());
        rows.draw();
        let before = rows.tops();
        // The last row grows by 24 as the reader scrolls down by the 60 they went up.
        let last = rows.heights.borrow().len() - 1;
        rows.heights.borrow_mut()[last] += 24.;
        rows.state.remeasure_items(last..last + 1);
        rows.scroll(60., ScrollGesture::step());
        rows.draw();
        let up = before[0].1 - rows.tops()[0].1;
        assert_eq!(up, 60., "the lines move by the delta, at {scale}");
        assert!(!rows.choice.get(), "the new end doesn't show, at {scale}");
        rows.scroll(60., ScrollGesture::step());
        rows.draw();
        assert!(rows.choice.get(), "at {scale}");
        assert_eq!(rows.blank(), 0., "at {scale}");
    }
}

#[gpui::test]
fn the_thumb_dragged_up_reads_and_at_the_bottom_follows(cx: &mut TestAppContext) {
    for scale in SCALES {
        let mut rows = Rows::new(cx, turns(40), scale);
        let last = rows.heights.borrow().len() - 1;
        rows.heights.borrow_mut()[last] -= 40.;
        rows.state.remeasure_items(last..last + 1);
        rows.draw();
        let max = rows.state.max_offset_for_scrollbar().y;
        assert_eq!(
            rows.state.scroll_px_offset_for_scrollbar().y,
            -max,
            "the thumb sits at the bottom with blank, at {scale}"
        );
        rows.state.scrollbar_drag_started();
        rows.state
            .set_offset_from_scrollbar(point(px(0.), -(max - px(80.))));
        rows.draw();
        assert!(!rows.choice.get(), "at {scale}");
        rows.state.set_offset_from_scrollbar(point(px(0.), -max));
        rows.draw();
        rows.state.scrollbar_drag_ended();
        assert!(rows.choice.get(), "at {scale}");
    }
}

#[gpui::test]
fn a_wheel_event_reaches_the_list_as_a_move(cx: &mut TestAppContext) {
    let mut rows = Rows::new(cx, turns(40), 2.);
    let before = rows.tops();
    rows.cx.simulate_event(ScrollWheelEvent {
        position: point(px(100.), px(100.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(30.))),
        touch_phase: TouchPhase::Moved,
        ..Default::default()
    });
    rows.draw();
    assert_eq!(
        rows.tops()[0].1 - before[0].1,
        60.,
        "30 logical pixels up at 2"
    );
    assert!(!rows.choice.get());
}
