//! The harness's own goldens: plain GPUI elements, so a change in GPUI's rendering shows up here
//! before it shows up in a component library.
#![cfg(target_os = "linux")]

use gpui::{
    AppContext as _, Context, FontWeight, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px, rgb, size,
};
use gpui_screenshot::{MONO_FAMILY, ScreenshotApp, goldens};

fn shapes() -> impl IntoElement {
    div()
        .size_full()
        .bg(rgb(0xffffff))
        .p_4()
        .flex()
        .items_center()
        .gap_4()
        .child(div().size(px(40.)).bg(rgb(0xe11d48)))
        .child(div().size(px(40.)).rounded(px(8.)).bg(rgb(0x2563eb)))
        .child(
            div()
                .size(px(40.))
                .rounded_full()
                .border_2()
                .border_color(rgb(0x16a34a)),
        )
        .child(
            div()
                .size(px(40.))
                .rounded(px(6.))
                .bg(rgb(0xffffff))
                .shadow_md(),
        )
        .child(div().w(px(56.)).h(px(1.)).bg(rgb(0x111827)))
}

fn text() -> impl IntoElement {
    div()
        .size_full()
        .bg(rgb(0xffffff))
        .text_color(rgb(0x111827))
        .p_3()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(12.))
                .child("Regular 12: The quick brown fox jumps over the lazy dog"),
        )
        .child(
            div()
                .text_size(px(16.))
                .font_weight(FontWeight::MEDIUM)
                .child("Medium 16: Sphinx of black quartz, judge my vow"),
        )
        .child(
            div()
                .text_size(px(20.))
                .font_weight(FontWeight::BOLD)
                .child("Bold 20: 0123456789 ½ € ©"),
        )
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_size(px(13.))
                .child("fn main() { println!(\"{}\", 6 * 7); }"),
        )
        .child(
            div()
                .text_size(px(14.))
                .child("Ελληνικά · Кириллица · Ünïcödé"),
        )
        .child(
            div()
                .text_size(px(14.))
                .text_color(rgb(0xffffff))
                .bg(rgb(0x2563eb))
                .px_2()
                .child("White on blue"),
        )
}

fn badge() -> impl IntoElement {
    div().size_full().bg(rgb(0xf3f4f6)).p_2().child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xd1d5db))
            .child(div().size(px(8.)).rounded_full().bg(rgb(0x16a34a)))
            .child(div().text_size(px(13.)).child("Online")),
    )
}

#[test]
fn shapes_render_to_their_golden() {
    let mut app = ScreenshotApp::new().unwrap();
    let shot = app
        .render_element(size(px(320.), px(72.)), 2., |_, _| shapes())
        .unwrap();
    assert_eq!(shot.image.dimensions(), (640, 144));
    goldens!().assert("shapes", &shot);
}

#[test]
fn text_renders_to_its_golden() {
    let mut app = ScreenshotApp::new().unwrap();
    let shot = app
        .render_element(size(px(400.), px(176.)), 2., |_, _| text())
        .unwrap();
    goldens!().assert("text", &shot);
}

#[test]
fn each_scale_factor_has_its_own_golden() {
    let mut app = ScreenshotApp::new().unwrap();
    for (scale, name) in [
        (1., "badge@1x"),
        (1.5, "badge@1.5x"),
        (2.625, "badge@2.625x"),
        (3., "badge@3x"),
    ] {
        let shot = app
            .render_element(size(px(120.), px(40.)), scale, |_, _| badge())
            .unwrap();
        let expected = ((120. * scale) as u32, (40. * scale) as u32);
        assert_eq!(shot.image.dimensions(), expected, "{name}");
        goldens!().assert(name, &shot);
    }
}

struct Counter {
    count: usize,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .p_2()
            .text_size(px(14.))
            .child(format!("Clicked {} times", self.count))
    }
}

/// A window stays open between captures, so a test can change state and capture again.
#[test]
fn captures_follow_state_changes() {
    let mut app = ScreenshotApp::new().unwrap();
    let window = app
        .open_window(size(px(160.), px(32.)), 2., |_, cx| {
            cx.new(|_| Counter { count: 0 })
        })
        .unwrap();
    let before = app.capture(window).unwrap();
    window
        .update(&mut *app, |counter, _, cx| {
            counter.count = 3;
            cx.notify();
        })
        .unwrap();
    let after = app.capture(window).unwrap();
    assert!(before.image != after.image);
    goldens!().assert("counter", &after);
}

/// Renders on several threads at once, each with its own app, and on one app twice: every
/// image must be the same.
#[test]
fn every_thread_renders_the_same_pixels() {
    let render = || {
        let mut app = ScreenshotApp::new().unwrap();
        let first = app
            .render_element(size(px(400.), px(176.)), 2., |_, _| text())
            .unwrap();
        let second = app
            .render_element(size(px(400.), px(176.)), 2., |_, _| text())
            .unwrap();
        assert!(first.image == second.image, "one app rendered two images");
        first.image
    };
    let images: Vec<_> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..4).map(|_| scope.spawn(render)).collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect()
    });
    for image in &images[1..] {
        assert!(*image == images[0], "two threads rendered different images");
    }
}
