//! The cost of a long plain paragraph, laid out and painted, when it stays the same and when it
//! grows or changes by a word. Run it in a release build, which prints the times:
//!
//! ```sh
//! cargo test -p mhr-gpui-screenshot --release --test long_line_measure -- --ignored --nocapture
//! ```
#![cfg(target_os = "linux")]

use std::{sync::Arc, time::Instant};

use gpui::{
    AnyWindowHandle, AppContext as _, Context, HeadlessAppContext, IntoElement, ParentElement as _,
    PlatformTextSystem as _, Render, SharedString, Styled as _, TextRun, Window, div, font, hsla,
    px, size,
};
use gpui_screenshot::{SANS_FAMILY, bundled_fonts};
use gpui_wgpu::CosmicTextSystem;

const WORDS: &[&str] = &[
    "the",
    "quick",
    "brown",
    "fox",
    "jumps",
    "over",
    "a",
    "lazy",
    "dog",
    "while",
    "streaming",
    "tokens",
    "arrive",
    "one",
    "after",
    "another",
    "and",
    "the",
    "renderer",
    "keeps",
    "up",
    "with",
    "layout,",
    "which",
    "is",
    "why",
    "caching",
    "matters.",
    "Hello,",
    "world!",
    "It's",
    "fine",
    "(really)",
    "—",
    "naïve",
    "café",
    "résumé",
    "123,456",
    "x=1;",
    "https://example.com/a/b",
];

fn paragraph(bytes: usize, seed: usize) -> String {
    let mut text = String::with_capacity(bytes + 32);
    let mut n = seed;
    while text.len() < bytes {
        n = n
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        text.push_str(WORDS[(n >> 33) % WORDS.len()]);
        text.push(' ');
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

struct Para {
    text: SharedString,
}

impl Render for Para {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(700.))
            .text_size(px(16.))
            .text_color(hsla(0., 0., 1., 1.))
            .child(self.text.clone())
    }
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

fn stats(label: &str, mut times: Vec<f64>) {
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    println!(
        "{label}: mean {:.3} ms, p50 {:.3}, p90 {:.3}, min {:.3} (n={})",
        mean,
        times[times.len() / 2],
        times[(times.len() - 1) * 9 / 10],
        times[0],
        times.len()
    );
}

fn runs_for(text: &str) -> Vec<TextRun> {
    vec![TextRun {
        len: text.len(),
        font: font(SANS_FAMILY),
        color: hsla(0., 0., 1., 1.),
        background_color: None,
        underline: None,
        strikethrough: None,
    }]
}

#[test]
#[ignore]
fn long_line_measure() {
    let text_system = Arc::new(CosmicTextSystem::new_without_system_fonts(SANS_FAMILY));
    text_system.add_fonts(bundled_fonts()).unwrap();
    let mut cx = HeadlessAppContext::new(text_system);

    for bytes in [2_000usize, 20_000, 100_000] {
        let base = paragraph(bytes, 1);
        let window = cx
            .open_window(size(px(800.), px(1000.)), |_, cx| {
                cx.new(|_| Para {
                    text: base.clone().into(),
                })
            })
            .unwrap();
        cx.run_until_parked();
        let any_window = AnyWindowHandle::from(window);
        let para = cx
            .update_window(any_window, |root, _, _| root.downcast::<Para>().unwrap())
            .unwrap();

        // Shaping directly, cold (a new text every time) and warm (the same text).
        let iters = if bytes > 50_000 { 10 } else { 40 };
        let mut cold = Vec::new();
        for n in 0..iters {
            let text: SharedString = paragraph(bytes, 100 + n).into();
            let runs = runs_for(&text);
            let start = Instant::now();
            cx.update_window(any_window, |_, window, _| {
                let lines = window
                    .text_system()
                    .shape_text(text.clone(), px(16.), &runs, Some(px(700.)), None)
                    .unwrap();
                std::hint::black_box(lines);
            })
            .unwrap();
            cold.push(ms(start.elapsed()));
        }
        stats(&format!("{bytes}B shape_text cold"), cold);

        let text: SharedString = paragraph(bytes, 7).into();
        let runs = runs_for(&text);
        let mut warm = Vec::new();
        for _ in 0..200 {
            let start = Instant::now();
            cx.update_window(any_window, |_, window, _| {
                let lines = window
                    .text_system()
                    .shape_text(text.clone(), px(16.), &runs, Some(px(700.)), None)
                    .unwrap();
                std::hint::black_box(lines);
            })
            .unwrap();
            warm.push(ms(start.elapsed()));
        }
        stats(&format!("{bytes}B shape_text warm"), warm);

        // Whole frames, with layout and paint.
        let set_text = |cx: &mut HeadlessAppContext, text: String| {
            let para = para.clone();
            cx.update(|cx| {
                para.update(cx, |para, cx| {
                    para.text = text.into();
                    cx.notify();
                })
            });
        };
        let mut unchanged = Vec::new();
        for _ in 0..30 {
            set_text(&mut cx, base.clone());
        }
        for _ in 0..60 {
            let start = Instant::now();
            let para = para.clone();
            cx.update(|cx| para.update(cx, |_, cx| cx.notify()));
            unchanged.push(ms(start.elapsed()));
        }
        stats(&format!("{bytes}B frame, text unchanged"), unchanged);

        let mut appended = Vec::new();
        let mut current = base.clone();
        for n in 0..(if bytes > 50_000 { 15 } else { 60 }) {
            current.push_str(WORDS[n % WORDS.len()]);
            current.push(' ');
            let start = Instant::now();
            set_text(&mut cx, current.clone());
            appended.push(ms(start.elapsed()));
        }
        stats(&format!("{bytes}B frame, word appended"), appended);

        let mut edited = Vec::new();
        let mut current = base.clone();
        for n in 0..(if bytes > 50_000 { 15 } else { 60 }) {
            // Replace one word near the middle.
            let mid = current.len() / 2;
            let at = current[mid..].find(' ').unwrap() + mid + 1;
            let end = at + current[at..].find(' ').unwrap();
            current.replace_range(at..end, &format!("w{}", n % 10));
            let start = Instant::now();
            set_text(&mut cx, current.clone());
            edited.push(ms(start.elapsed()));
        }
        stats(&format!("{bytes}B frame, mid word edited"), edited);

        cx.update_window(any_window, |_, window, _| window.remove_window())
            .unwrap();
    }
}
