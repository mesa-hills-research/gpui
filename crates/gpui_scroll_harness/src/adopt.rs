//! Where a candidate gives the list what it needs from the rows.
//!
//! The list on this branch needs nothing, so [`adopt`] does nothing. A
//! candidate that adds an API to `ListState`, such as per-row height
//! estimates, calls it here, from what [`Facts`] says about the rows. Keep
//! to what an app knows cheaply before it draws a row (its kind, its
//! characters, its lines of code) unless the candidate is meant as a bound,
//! such as exact heights given up front.

use std::rc::Rc;

use gpui::{List, ListState, Pixels};

use crate::transcript::{Rng, Row};

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
        let mut rng = Rng::new(ix as u64 ^ 0x5EED);
        let off = (rng.unit() * 2. - 1.) * error;
        self.exact[ix] * (1. + off)
    }
}

/// Called after the list's state is made, and again after the list's width
/// changes, with what the app knows about its rows then.
pub fn adopt(_state: &ListState, _facts: &Facts) {}

/// Called with the list element in every frame, for a candidate whose API
/// is on the element rather than on its state.
pub fn element(list: List, _rows: &Rc<Vec<Row>>) -> List {
    list
}
