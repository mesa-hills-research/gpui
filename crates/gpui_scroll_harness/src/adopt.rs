//! Where a candidate gives the list what it needs from the rows.
//!
//! This branch's list takes height estimates on its element
//! (`List::estimate_item_heights`). [`element`] gives it the estimates an
//! app such as a chat transcript can make without laying a row out: rows of
//! fixed layout exactly, and wrapped text as its lines times their height,
//! the lines guessed from the text's characters and the font's average
//! character width, measured once from a sample sentence.
//!
//! `ESTIMATE` in the environment picks other estimates, for bounds:
//! `exact` gives every row's real height (as a cache of heights kept from
//! an earlier session would), `exact-25` the real height off by up to 25%
//! either way, and `none` no estimates at all.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{App, Font, FontWeight, List, ListState, Pixels, TextRun, Window, font, hsla, px};
use gpui_screenshot::SANS_FAMILY;

use crate::transcript::{COLUMN, Kind, Rng, Row, SHOWN_LINES, SIDE_PADDING};

/// What a list's user knows about its rows.
#[allow(dead_code, reason = "for candidates that need them")]
pub struct Facts<'a> {
    pub rows: &'a [Row],
    /// The list's width, which rows wrap to.
    pub width: Pixels,
    /// Each row's height at this width, as drawn. An app doesn't know these
    /// before it draws its rows, so use them only for bounds and for hints
    /// with an error, through [`Facts::hint`].
    pub exact: &'a [f32],
}

#[allow(dead_code, reason = "for candidates that need it")]
impl Facts<'_> {
    /// Row `ix`'s exact height, off by up to `error` (0.25 for 25%) either
    /// way, the same for every run.
    pub fn hint(&self, ix: usize, error: f32) -> f32 {
        self.exact[ix] * (1. + off(ix, error))
    }
}

/// How far row `ix`'s hint is off, as a share, the same at every width.
fn off(ix: usize, error: f32) -> f32 {
    let mut rng = Rng::new(ix as u64 ^ 0x5EED);
    (rng.unit() * 2. - 1.) * error
}

thread_local! {
    /// The rows' real heights at each width the list has had, for the
    /// bounds.
    static EXACT: RefCell<HashMap<u32, Rc<Vec<f32>>>> = RefCell::default();
}

/// Called after the list's state is made, and again after the list's width
/// changes, with what the app knows about its rows then.
pub fn adopt(_state: &ListState, facts: &Facts) {
    EXACT.with(|exact| {
        exact.borrow_mut().insert(
            f32::from(facts.width).to_bits(),
            Rc::new(facts.exact.to_vec()),
        )
    });
}

/// Called with the list element in every frame, for a candidate whose API
/// is on the element rather than on its state.
pub fn element(list: List, rows: &Rc<Vec<Row>>) -> List {
    let rows = rows.clone();
    let mode = std::env::var("ESTIMATE").unwrap_or_default();
    match mode.as_str() {
        "none" => list,
        "exact" | "exact-25" => {
            let error = if mode == "exact" { 0. } else { 0.25 };
            list.estimate_item_heights(move |ix, width, _, _| {
                let exact =
                    EXACT.with(|exact| exact.borrow().get(&f32::from(width).to_bits()).cloned());
                match exact {
                    Some(exact) => px(exact[ix] * (1. + off(ix, error))),
                    None => px(estimate(&rows[ix], f32::from(width), &widths())),
                }
            })
        }
        _ => list.estimate_item_heights(move |ix, width, window, cx| {
            px(estimate(
                &rows[ix],
                f32::from(width),
                &char_widths(window, cx),
            ))
        }),
    }
}

/// The average character width of the fonts the rows wrap text in.
#[derive(Clone, Copy, Debug)]
struct CharWidths {
    prose: f32,
    notice: f32,
    heading: f32,
}

thread_local! {
    static WIDTHS: RefCell<Option<CharWidths>> = const { RefCell::new(None) };
}

fn widths() -> CharWidths {
    WIDTHS.with(|widths| {
        widths.borrow().unwrap_or(CharWidths {
            prose: 7.,
            notice: 6.,
            heading: 9.5,
        })
    })
}

/// Measures the fonts' average character width once, from a sample
/// sentence.
fn char_widths(window: &mut Window, _cx: &mut App) -> CharWidths {
    if let Some(widths) = WIDTHS.with(|widths| *widths.borrow()) {
        return widths;
    }
    const SAMPLE: &str = "The list measures a row when it draws it, and estimates the height of every other row from its text.";
    let measure = |font: Font, size: f32| {
        let run = TextRun {
            len: SAMPLE.len(),
            font,
            color: hsla(0., 0., 0., 1.),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        f32::from(
            window
                .text_system()
                .layout_line(SAMPLE, px(size), &[run], None)
                .width,
        ) / SAMPLE.chars().count() as f32
    };
    let widths = CharWidths {
        prose: measure(font(SANS_FAMILY), 15.),
        notice: measure(font(SANS_FAMILY), 13.),
        heading: measure(
            Font {
                weight: FontWeight::BOLD,
                ..font(SANS_FAMILY)
            },
            18.,
        ),
    };
    WIDTHS.with(|cell| *cell.borrow_mut() = Some(widths));
    widths
}

/// Row `row`'s height at `width`, from its kind, its text's length and its
/// lines, and the layout its kind has.
fn estimate(row: &Row, width: f32, widths: &CharWidths) -> f32 {
    let column = width.min(COLUMN) - 2. * SIDE_PADDING;
    let lines = |chars: usize, char_width: f32, text_width: f32| {
        (chars as f32 * char_width / text_width.max(1.))
            .ceil()
            .max(1.)
    };
    let body = match row.kind {
        Kind::Separator | Kind::Thinking | Kind::Call | Kind::Group | Kind::Actions => 28.,
        Kind::Refresh | Kind::Turn => 18.,
        Kind::User => lines(row.chars(), widths.prose, column.min(560.) - 28.) * 22. + 16.,
        Kind::CallOpen => 28. + 4. + row.shown_lines() as f32 * 18. + 20.,
        Kind::Notice => lines(row.chars(), widths.notice, column - 20.) * 20. + 8.,
        Kind::Paragraph => lines(row.chars(), widths.prose, column) * 24.,
        Kind::Heading => lines(row.chars(), widths.heading, column) * 28. + 4.,
        Kind::Bullets => {
            row.items
                .iter()
                .map(|item| lines(item.chars().count(), widths.prose, column - 20.) * 24.)
                .sum::<f32>()
                + 4. * row.items.len().saturating_sub(1) as f32
        }
        Kind::Code => {
            row.shown_lines() as f32 * 20. + 20. + if row.lines > SHOWN_LINES { 24. } else { 0. }
        }
        Kind::Image => 120.,
    };
    body + row.gap * 16.
}
