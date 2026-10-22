//! The cost of a frame in which a spinner turns in a cached pane of rows, as the pane redraws to
//! turn the spinner, as the window's root view redraws with the pane reused, and as the spinner is
//! a paint animation. It prints the times of frames in a release build:
//!
//! ```sh
//! cargo test -p mhr-gpui-screenshot --release --test spinner_frames -- --ignored --nocapture
//! ```
#![cfg(target_os = "linux")]

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt as _, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, PaintEffect, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Window, div, px, rgb, size,
};
use gpui_screenshot::ScreenshotApp;

struct Pane {
    rows: Vec<(SharedString, SharedString, SharedString)>,
    spinner_tick: usize,
    paint_animation: bool,
    renders: Rc<Cell<usize>>,
}

impl Render for Pane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        let spinner = div().size(px(16.)).rounded_full().bg(rgb(0x4080ff));
        div()
            .flex()
            .flex_col()
            .size_full()
            .p_3()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_size(px(20.)).child("Settings"))
                    // The spinner.
                    .child(if self.paint_animation {
                        spinner
                            .with_paint_animation(
                                "spinner",
                                Animation::new(Duration::from_millis(1000)).repeat(),
                                |t| PaintEffect::opacity(0.3 + 0.7 * t),
                            )
                            .into_any_element()
                    } else {
                        spinner
                            .opacity(0.3 + (self.spinner_tick % 10) as f32 / 15.)
                            .into_any_element()
                    }),
            )
            .children(
                self.rows
                    .iter()
                    .enumerate()
                    .map(|(ix, (label, description, value))| {
                        div()
                            .id(ix)
                            .flex()
                            .flex_row()
                            .justify_between()
                            .h(px(36.))
                            .border_b_1()
                            .border_color(rgb(0x303030))
                            .hover(|style| style.bg(rgb(0x303030)))
                            .on_click(|_, _, _| {})
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(px(13.)).child(label.clone()))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(0xa0a0a0))
                                            .child(description.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .id("value")
                                    .w(px(220.))
                                    .px_1()
                                    .border_1()
                                    .border_color(rgb(0x606060))
                                    .rounded_sm()
                                    .bg(rgb(0x101010))
                                    .text_size(px(13.))
                                    .child(value.clone()),
                            )
                    }),
            )
    }
}

struct Root {
    pane: Entity<Pane>,
    ticks: usize,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(
                div()
                    .w(px(200.))
                    .h_full()
                    .child(format!("Sidebar {}", self.ticks)),
            )
            .child(
                div().flex_1().h_full().child(
                    self.pane
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                ),
            )
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[test]
#[ignore]
fn spinner_frames() {
    for rows in [30usize, 120, 480] {
        let mut app = ScreenshotApp::new().unwrap();
        let renders = Rc::new(Cell::new(0));
        let window = app
            .open_window(size(px(1200.), px(1000.)), 1., |_, cx| {
                let pane = cx.new(|_| Pane {
                    paint_animation: false,
                    renders: renders.clone(),
                    rows: (0..rows)
                        .map(|r| {
                            (
                                format!("Setting {r}").into(),
                                format!("What setting {r} changes, in a sentence or so").into(),
                                format!("value {r}").into(),
                            )
                        })
                        .collect(),
                    spinner_tick: 0,
                });
                cx.new(|_| Root { pane, ticks: 0 })
            })
            .unwrap();
        app.run_until_parked();
        let pane = app.update(|cx| window.read(cx).unwrap().pane.clone());

        let spinner_frame = |app: &mut ScreenshotApp, n: usize| {
            let pane = pane.clone();
            let started = Instant::now();
            app.update(|cx| {
                pane.update(cx, |pane, cx| {
                    pane.spinner_tick = n;
                    cx.notify();
                })
            });
            started.elapsed().as_secs_f64() * 1000.
        };
        let root_frame = |app: &mut ScreenshotApp, n: usize| {
            let started = Instant::now();
            app.update_window(window.into(), |root, _, cx| {
                let root = root.downcast::<Root>().unwrap();
                root.update(cx, |root, cx| {
                    root.ticks = n;
                    cx.notify();
                });
            })
            .unwrap();
            started.elapsed().as_secs_f64() * 1000.
        };

        for n in 0..50 {
            spinner_frame(&mut app, n);
            root_frame(&mut app, n);
        }
        let mut spinner = Vec::new();
        let mut root_only = Vec::new();
        for n in 0..300 {
            spinner.push(spinner_frame(&mut app, 100 + n));
            root_only.push(root_frame(&mut app, 100 + n));
        }
        spinner.sort_by(f64::total_cmp);
        root_only.sort_by(f64::total_cmp);
        println!(
            "rows {rows}: spinner in the pane p50 {:.3} ms (p90 {:.3}), root only p50 {:.3} ms (p90 {:.3})",
            percentile(&spinner, 0.5),
            percentile(&spinner, 0.9),
            percentile(&root_only, 0.5),
            percentile(&root_only, 0.9),
        );

        // The same pane with the spinner as a paint animation.
        app.update(|cx| cx.set_reduce_motion(false));
        app.update(|cx| {
            pane.update(cx, |pane, cx| {
                pane.paint_animation = true;
                cx.notify();
            })
        });
        app.run_until_parked();
        let renders_before = renders.get();
        let frame = |app: &mut ScreenshotApp| {
            app.advance_clock(Duration::from_millis(16));
            let started = Instant::now();
            app.update_window(window.into(), |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
            started.elapsed().as_secs_f64() * 1000.
        };
        for _ in 0..50 {
            frame(&mut app);
        }
        let mut painted = (0..300).map(|_| frame(&mut app)).collect::<Vec<_>>();
        painted.sort_by(f64::total_cmp);
        println!(
            "rows {rows}: spinner as a paint animation p50 {:.3} ms (p90 {:.3}), pane renders {}",
            percentile(&painted, 0.5),
            percentile(&painted, 0.9),
            renders.get() - renders_before
        );
    }
}
