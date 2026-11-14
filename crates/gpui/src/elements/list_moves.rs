// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
//! A list laid out by the moves queued for its next frame ([`ListState::with_queued_moves`]).
//!
//! The list applies the frame's moves in its layout, over the rows it measures there, in whole
//! device pixels, and reports what each did (how far the lines moved, and whether the end
//! shows).
//! The app decides from that report whether the frame follows the end, so the app owns the
//! reader's choice to follow and the list owns where the lines are. A chat transcript is the
//! list this is for.
//!
//! Heights the list has only estimated place the scrollbar's thumb and nothing on the screen.
//! The lines never move down by themselves. When the end gets shorter, blank space opens below
//! it and stays until what comes fills it, and a scroll down stops where the end meets the
//! bottom edge.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use scheduler::Instant;
use sum_tree::{Bias, SumTree};

use super::{
    Count, ItemLayout, LayoutItemsResponse, ListItem, ListOffset, ListState, PendingScroll,
    RenderItemFn, StateInner,
};
use crate::{
    AnyElement, App, AvailableSpace, Edges, Pixels, ScrollDelta, ScrollWheelEvent, Size,
    TouchPhase, Window, px, size,
};

/// How long a gesture lasts after its last event, where the platform says nothing of its end.
const GESTURE_GAP: std::time::Duration = std::time::Duration::from_millis(100);

/// A move of a list laid out by queued moves, applied in its next layout.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ListMove {
    /// The reader's scroll by `delta`, down the list when positive (the lines move up), from a
    /// wheel, a touchpad, a finger or a key. Its part below a device pixel waits for the next
    /// delta of its gesture.
    Scroll {
        /// How far, down the list when positive.
        delta: Pixels,
        /// The gesture it is part of.
        gesture: ScrollGesture,
    },
    /// The scrollbar's thumb dragged to the place `to`, or to the end of its track (`None`).
    Thumb {
        /// Where the thumb puts the list, by the estimated heights.
        to: Option<ListOffset>,
    },
    /// The end on the view's bottom edge, the blank below it closed, or the start on the top
    /// edge where the items don't fill the view.
    ToEnd,
    /// The start on the top edge.
    ToStart,
    /// The row `item_ix` on the top edge, `offset_in_item` into it.
    To(ListOffset),
    /// The row into view, by as little as it takes. A row taller than the view shows its top.
    Reveal(usize),
    /// The end on the view's bottom edge where blank shows below it and rows are hidden above,
    /// as after the rows were made again for another view.
    CloseBlank,
    /// The row's top `by` below where the last frame drew it, and every row above with it. A
    /// row the reader opened or closed keeps its line still this way.
    KeepTop {
        /// The row.
        ix: usize,
        /// How far the row's top goes down from where it was drawn, up when negative.
        by: Pixels,
    },
}

impl ListMove {
    /// The move a wheel or touchpad event asks for, at `at` by the app's clock. A line of a
    /// wheel is `line_height` long.
    pub fn scroll_wheel(event: &ScrollWheelEvent, line_height: Pixels, at: Instant) -> Self {
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => delta.y,
            ScrollDelta::Lines(delta) => line_height * delta.y,
        };
        ListMove::Scroll {
            delta: -delta,
            gesture: ScrollGesture::of(event, at),
        }
    }

    /// Whether the move places the view, which ends a coast going.
    fn places(&self) -> bool {
        matches!(
            self,
            ListMove::ToEnd
                | ListMove::ToStart
                | ListMove::To(_)
                | ListMove::Reveal(_)
                | ListMove::CloseBlank
        )
    }
}

/// What a scroll is part of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollGesture {
    /// The gesture starts with it (fingers came down).
    pub starts: bool,
    /// The gesture ends with it (fingers lifted, or a coast stopped).
    pub ends: bool,
    /// It is a coast, the motion after the fingers lifted, which the reader no longer drives.
    pub coasting: bool,
    /// When it came, by the app's clock. A gesture whose platform says nothing of its end ends
    /// when no event has come for a tenth of a second.
    pub at: Option<Instant>,
}

impl ScrollGesture {
    /// The gesture of a wheel or touchpad event that came at `at`.
    pub fn of(event: &ScrollWheelEvent, at: Instant) -> Self {
        Self {
            starts: event.touch_phase == TouchPhase::Started,
            ends: matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled),
            coasting: event.coasting,
            at: Some(at),
        }
    }

    /// A gesture of one step, as a key gives.
    pub fn step() -> Self {
        Self {
            starts: true,
            ends: true,
            coasting: false,
            at: None,
        }
    }
}

/// What one move did.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct MoveEffect {
    /// The move.
    pub what: ListMove,
    /// How far it moved the lines, in device pixels, down when positive.
    pub moved: i32,
    /// Whether the end shows after it.
    pub end_shows: bool,
}

/// What the app decides from, in a frame of a list laid out by queued moves.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ListFrame<'a> {
    /// The frame's moves, in the order they came, with what each did.
    pub moves: &'a [MoveEffect],
    /// Whether the end showed in the last frame.
    pub end_showed: bool,
    /// Whether the end shows after the moves.
    pub end_shows: bool,
    /// Whether the list has another width than in the last frame, and its rows wrap again.
    pub width_changed: bool,
}

/// What a frame does with the end, as the app decides it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Follow {
    /// The lines stay where the moves put them, and what comes goes below them.
    #[default]
    No,
    /// The lines stay, and what comes at the end takes the blank below it first, then moves
    /// the lines up so the end stays on the bottom edge. A view that gets shorter takes the
    /// blank first and then moves the lines up, and one that gets taller moves them back down
    /// by as much.
    Yes,
    /// As [`Follow::Yes`], from a reader who wasn't following, so what earlier shorter views
    /// took isn't given back.
    Again,
    /// The end on the bottom edge, the blank below it closed, as after another width.
    ToEnd,
}

/// What the last layout of a list laid out by queued moves did.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ListReport {
    /// Its moves, in order, with what each did.
    pub moves: Vec<MoveEffect>,
    /// What the app decided.
    pub follow: Follow,
    /// Whether the end shows, with no device pixel row of it or of the padding below it below
    /// the view's bottom edge.
    pub end_shows: bool,
    /// How far the end, with the padding below it, is below the view's bottom edge, in logical
    /// pixels, by the rows measured where they were and estimated elsewhere. It is below zero
    /// where the end shows with blank below it, by the blank's height.
    pub below: Pixels,
}

/// The state of a list laid out by queued moves.
#[derive(Default)]
pub(super) struct Moves {
    pub(super) queue: Vec<ListMove>,
    pub(super) report: ListReport,
    /// The part of the gesture's deltas below a device pixel, its direction and when its last
    /// delta came.
    carry: f64,
    carry_down: bool,
    last_scroll: Option<Instant>,
    /// A move placed the view since the coast going started.
    coast_ended: bool,
    /// How far shorter views moved the lines up while following, in device pixels, which a
    /// taller view gives back.
    give_back: f64,
    /// The rows the last layout drew, each with its top below the view's top.
    drawn: Vec<(usize, Pixels)>,
    /// Rows to measure in the next layout, wherever they are.
    measure: Vec<Range<usize>>,
    /// The view's height in the last layout.
    height: Option<Pixels>,
}

impl Moves {
    /// The rows the last layout drew, after `old_range` was replaced by `count` rows. The
    /// first row that replaced them starts where the first of them did.
    pub(super) fn splice(&mut self, old_range: &Range<usize>, count: usize) {
        self.drawn.retain_mut(|(ix, _)| {
            if old_range.contains(ix) {
                *ix == old_range.start && count > 0
            } else {
                if *ix >= old_range.end {
                    *ix = *ix - old_range.len() + count;
                }
                true
            }
        });
    }
}

impl ListState {
    /// Lays the list out by the moves queued for its next frame ([`Self::queue_move`]), in
    /// whole device pixels, and reports what they did ([`Self::report`]). The app decides from
    /// that whether a frame follows the end ([`super::List::follow_end`]), and the list's own
    /// follow mode has no effect.
    ///
    /// The lines never move down by themselves. When the end gets shorter, blank space opens
    /// below it and stays until what comes fills it, and a scroll down stops where the end meets
    /// the bottom edge. Heights the list has only estimated place the scrollbar's thumb and
    /// nothing on the screen. A remeasured row and another width keep the place inside the top
    /// row by its share of the row's height.
    pub fn with_queued_moves(self) -> Self {
        self.0.borrow_mut().moves = Some(Moves::default());
        self
    }

    /// Whether the list is laid out by queued moves.
    pub fn has_queued_moves(&self) -> bool {
        self.0.borrow().moves.is_some()
    }

    /// Whether moves wait for the next layout.
    pub fn moves_waiting(&self) -> bool {
        self.0
            .borrow()
            .moves
            .as_ref()
            .is_some_and(|moves| !moves.queue.is_empty())
    }

    /// Queues `what` for the next layout, after the moves queued before it. A list not laid out
    /// by queued moves does what the move asks at once, as its other methods do.
    pub fn queue_move(&self, what: ListMove) {
        let mut state = self.0.borrow_mut();
        if let Some(moves) = state.moves.as_mut() {
            moves.queue.push(what);
            return;
        }
        drop(state);
        match what {
            ListMove::Scroll { delta, .. } => self.scroll_by(delta),
            ListMove::Thumb { to: Some(to) } | ListMove::To(to) => self.scroll_to(to),
            ListMove::Thumb { to: None } | ListMove::ToEnd => self.scroll_to_end(),
            ListMove::CloseBlank => {}
            ListMove::ToStart => self.scroll_to(ListOffset::default()),
            ListMove::Reveal(ix) => self.scroll_to_reveal_item(ix),
            ListMove::KeepTop { .. } => {}
        }
    }

    /// What the last layout's moves did, for a list laid out by queued moves.
    pub fn report(&self) -> ListReport {
        let state = self.0.borrow();
        state
            .moves
            .as_ref()
            .map(|moves| moves.report.clone())
            .unwrap_or_default()
    }

    /// Measures the rows in `range` in the next layout, wherever they are, as an app does that
    /// counts the height of rows the reader hasn't seen ([`Self::item_height`]).
    pub fn measure_items(&self, range: Range<usize>) {
        if let Some(moves) = self.0.borrow_mut().moves.as_mut()
            && !range.is_empty()
        {
            moves.measure.push(range);
        }
    }

    /// The height row `ix` had where it was last measured, if it was measured since it last
    /// changed.
    pub fn item_height(&self, ix: usize) -> Option<Pixels> {
        let state = self.0.borrow();
        let mut cursor = state.items.cursor::<Count>(());
        cursor.seek(&Count(ix), Bias::Right);
        cursor
            .item()
            .and_then(|item| item.size())
            .map(|size| size.height)
    }
}

/// The rows of one layout, measured as it goes.
struct Rows<'a> {
    items: &'a SumTree<ListItem>,
    count: usize,
    /// The sizes measured in this layout.
    sizes: BTreeMap<usize, Size<Pixels>>,
    /// The elements made to measure them, which the rows drawn take.
    elements: HashMap<usize, AnyElement>,
    space: Size<AvailableSpace>,
    scale: f64,
}

impl Rows<'_> {
    /// Row `ix`'s height in device pixels, measured if it hasn't been.
    fn height(
        &mut self,
        ix: usize,
        render: &mut RenderItemFn,
        window: &mut Window,
        cx: &mut App,
    ) -> f64 {
        self.size(ix, render, window, cx)
            .map_or(0., |size| self.device(size.height))
    }

    fn size(
        &mut self,
        ix: usize,
        render: &mut RenderItemFn,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Size<Pixels>> {
        if ix >= self.count {
            return None;
        }
        if let Some(size) = self.sizes.get(&ix) {
            return Some(*size);
        }
        let mut cursor = self.items.cursor::<Count>(());
        cursor.seek(&Count(ix), Bias::Right);
        if let Some(size) = cursor.item().and_then(|item| item.size()) {
            return Some(size);
        }
        Some(self.render(ix, render, window, cx))
    }

    /// Makes row `ix`'s element and measures it.
    fn render(
        &mut self,
        ix: usize,
        render: &mut RenderItemFn,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels> {
        let mut element = render(ix, window, cx);
        let size = element.layout_as_root(self.space, window, cx);
        self.sizes.insert(ix, size);
        self.elements.insert(ix, element);
        size
    }

    /// The height a row is counted at where it hasn't been measured, as the scrollbar counts it.
    fn estimate(&self, range: Range<usize>) -> f64 {
        let mut cursor = self.items.cursor::<super::ListItemSummary>(());
        cursor.seek(&Count(range.start), Bias::Right);
        let summary: super::ListItemSummary = cursor.summary(&Count(range.end), Bias::Right);
        let mut height = self.device(summary.height);
        for (ix, size) in self.sizes.range(range) {
            let mut cursor = self.items.cursor::<Count>(());
            cursor.seek(&Count(*ix), Bias::Right);
            let counted = cursor
                .item()
                .and_then(|item| item.size_hint())
                .map_or(px(0.), |size| size.height);
            height += self.device(size.height) - self.device(counted);
        }
        height
    }

    fn device(&self, pixels: Pixels) -> f64 {
        (pixels.0 as f64 * self.scale).round()
    }
}

/// A place in the list in device pixels, as a row and how far into it the view's top edge is,
/// past the padding above the rows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    ix: usize,
    offset: f64,
}

impl StateInner {
    /// Lays the list out by its queued moves, over the rows measured here, and reports what
    /// they did. `decide` is the app's choice of what the frame does with the end.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_by_moves(
        &mut self,
        width: Pixels,
        height: Pixels,
        padding: &Edges<Pixels>,
        width_changed: bool,
        render: &mut RenderItemFn,
        decide: &mut dyn FnMut(&ListFrame<'_>, &mut Window, &mut App) -> Follow,
        window: &mut Window,
        cx: &mut App,
    ) -> LayoutItemsResponse {
        let scale = window.scale_factor() as f64;
        let device = |pixels: Pixels| (pixels.0 as f64 * scale).round();
        let logical = |device: f64| px((device / scale) as f32);
        let mut moves = self.moves.take().unwrap_or_default();
        let items = self.items.clone();
        let count = items.summary().count;
        let mut rows = Rows {
            items: &items,
            count,
            sizes: BTreeMap::new(),
            elements: HashMap::new(),
            space: size(AvailableSpace::Definite(width), AvailableSpace::MinContent),
            scale,
        };
        // The rows' room, the view less the padding above and below them.
        let (top_pad, bottom_pad) = (device(padding.top), device(padding.bottom));
        let view = device(height);
        let room = (view - top_pad - bottom_pad).max(0.);

        // The rows on the screen count at the heights the frame draws them at, though no
        // remeasure was asked for.
        for &(ix, _) in &moves.drawn {
            if ix < count {
                rows.render(ix, render, window, cx);
            }
        }

        // Where the view is, with a remeasured top row kept by its place in it.
        let anchor = self.logical_scroll_top();
        let mut place = Place {
            ix: anchor.item_ix.min(count),
            offset: device(anchor.offset_in_item),
        };
        if let Some(pending) = self.pending_scroll.take() {
            let row = rows.height(place.ix, render, window, cx);
            match pending {
                PendingScroll::Absolute { item_ix, offset } if item_ix == place.ix => {
                    place.offset = device(offset).min(row);
                }
                PendingScroll::Proportional(pending) if pending.item_ix == place.ix => {
                    place.offset = (pending.fraction as f64 * row).round();
                }
                _ => {}
            }
        }
        for range in std::mem::take(&mut moves.measure) {
            for ix in range.start..range.end.min(count) {
                rows.size(ix, render, window, cx);
            }
        }
        let end_showed = moves.report.end_shows;

        // The frame's moves, in order.
        let queue = std::mem::take(&mut moves.queue);
        let mut effects = Vec::with_capacity(queue.len());
        for what in queue {
            if what.places() {
                moves.coast_ended = true;
            }
            let before = place;
            match what {
                ListMove::Scroll { delta, gesture } => {
                    if gesture.coasting && moves.coast_ended {
                        // (The reader asked for a place since the coast began.)
                    } else {
                        if !gesture.coasting {
                            moves.coast_ended = false;
                        }
                        let whole = moves.carry_scroll(delta, gesture, scale);
                        place = if whole > 0. {
                            let below =
                                end_below(&mut rows, place, room, whole, render, window, cx);
                            let down = match below {
                                Some(below) => whole.min(below.max(0.)),
                                None => whole,
                            };
                            forward(&mut rows, place, down, render, window, cx)
                        } else {
                            back(&mut rows, place, -whole, render, window, cx)
                        };
                    }
                }
                ListMove::Thumb { to: Some(to) } | ListMove::To(to) => {
                    place = Place {
                        ix: to.item_ix.min(count),
                        offset: device(to.offset_in_item).max(0.),
                    };
                }
                ListMove::Thumb { to: None } => {
                    if end_below(&mut rows, place, room, 0., render, window, cx)
                        .is_none_or(|below| below > 0.)
                    {
                        place = end_place(&mut rows, room, render, window, cx);
                    }
                }
                ListMove::ToEnd => place = end_place(&mut rows, room, render, window, cx),
                ListMove::CloseBlank => {
                    let blank = end_below(&mut rows, place, room, 0., render, window, cx)
                        .is_some_and(|below| below < 0.);
                    if blank && place != (Place { ix: 0, offset: 0. }) {
                        place = end_place(&mut rows, room, render, window, cx);
                    }
                }
                ListMove::ToStart => place = Place { ix: 0, offset: 0. },
                ListMove::Reveal(ix) if ix < count => {
                    let top = top_of(&mut rows, place, ix, room, render, window, cx);
                    let row = rows.height(ix, render, window, cx);
                    match top {
                        Some(top) if top < 0. => place = Place { ix, offset: 0. },
                        Some(top) if top + row > room => {
                            let down = (top + row - room).min(top);
                            place = forward(&mut rows, place, down, render, window, cx);
                        }
                        Some(_) => {}
                        None => place = Place { ix, offset: 0. },
                    }
                }
                ListMove::Reveal(_) => {}
                ListMove::KeepTop { ix, by } => {
                    if let Some(&(_, drawn)) = moves.drawn.iter().find(|(drawn, _)| *drawn == ix) {
                        // The row's top below the rows' top edge, where it goes.
                        let target = device(drawn) + device(by) - top_pad;
                        place = if target >= 0. {
                            back(
                                &mut rows,
                                Place { ix, offset: 0. },
                                target,
                                render,
                                window,
                                cx,
                            )
                        } else {
                            forward(
                                &mut rows,
                                Place { ix, offset: 0. },
                                -target,
                                render,
                                window,
                                cx,
                            )
                        };
                    }
                }
            }
            let moved = position(&mut rows, before, place, render, window, cx);
            let end_shows = end_below(&mut rows, place, room, 0., render, window, cx)
                .is_some_and(|below| below <= 0.);
            effects.push(MoveEffect {
                what,
                moved: moved as i32,
                end_shows,
            });
        }
        let end_shows = end_below(&mut rows, place, room, 0., render, window, cx)
            .is_some_and(|below| below <= 0.);
        let follow = decide(
            &ListFrame {
                moves: &effects,
                end_showed,
                end_shows,
                width_changed,
            },
            window,
            cx,
        );

        // What the frame does with the end, and with a view of another height.
        let grew = moves.height.map_or(0., |old| view - device(old));
        if follow == Follow::Again {
            moves.give_back = 0.;
        }
        match follow {
            Follow::Yes | Follow::Again => {
                if grew > 0. {
                    let give = grew.min(moves.give_back);
                    let given = back(&mut rows, place, give, render, window, cx);
                    moves.give_back -= position(&mut rows, place, given, render, window, cx);
                    place = given;
                }
                let below = end_below(&mut rows, place, room, room, render, window, cx);
                match below {
                    // What came takes the blank and then moves the lines up.
                    Some(below) if below > 0. => {
                        let end = end_place(&mut rows, room, render, window, cx);
                        if grew < 0. {
                            moves.give_back += below.min(-grew);
                        }
                        place = end;
                    }
                    // A blank that would fill the whole view brings the end down.
                    Some(below) if -below >= room && room > 0. => {
                        place = end_place(&mut rows, room, render, window, cx);
                    }
                    Some(_) => {}
                    None => place = end_place(&mut rows, room, render, window, cx),
                }
            }
            Follow::ToEnd => place = end_place(&mut rows, room, render, window, cx),
            Follow::No => {
                if grew > 0. {
                    moves.give_back -= grew.min(moves.give_back);
                }
            }
        }

        // The rows on the screen, from the place.
        let (layouts, drawn, max_width) = draw(
            &mut rows,
            place,
            top_pad,
            room + bottom_pad,
            self.overdraw,
            render,
            window,
            cx,
        );
        let below_end = end_below(&mut rows, place, room, f64::INFINITY, render, window, cx);
        let end_shows = below_end.is_some_and(|below| below <= 0.);
        let below = match below_end {
            Some(below) => below,
            None => {
                let shown = draw_extent(&mut rows, place, room, render, window, cx);
                (rows.estimate(shown..count)
                    - remainder(&mut rows, place, room, render, window, cx))
                .max(0.)
            }
        };

        // The sizes measured go into the list.
        let measured = std::mem::take(&mut rows.sizes);
        drop(rows);
        self.store_sizes(&measured);
        self.logical_scroll_top = Some(ListOffset {
            item_ix: place.ix,
            offset_in_item: logical(place.offset),
        });
        moves.drawn = drawn
            .into_iter()
            .map(|(ix, top)| (ix, logical(top)))
            .collect();
        moves.height = Some(height);
        moves.report = ListReport {
            moves: effects,
            follow,
            end_shows,
            below: logical(below),
        };
        self.moves = Some(moves);
        LayoutItemsResponse {
            max_item_width: max_width,
            scroll_top: ListOffset {
                item_ix: place.ix,
                offset_in_item: logical(place.offset),
            },
            item_layouts: layouts,
        }
    }

    /// Lays the list out by its queued moves and places its rows in `bounds`. Requests of the
    /// rows to scroll themselves into view are dropped, since every move comes as a queued move.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepaint_by_moves(
        &mut self,
        bounds: crate::Bounds<Pixels>,
        padding: Edges<Pixels>,
        width_changed: bool,
        render: &mut RenderItemFn,
        decide: Option<&mut super::FollowEndFn>,
        window: &mut Window,
        cx: &mut App,
    ) -> LayoutItemsResponse {
        let mut stay = |_: &ListFrame<'_>, _: &mut Window, _: &mut App| Follow::No;
        let decide: &mut dyn FnMut(&ListFrame<'_>, &mut Window, &mut App) -> Follow = match decide {
            Some(decide) => decide,
            None => &mut stay,
        };
        let mut layout = self.layout_by_moves(
            bounds.size.width,
            bounds.size.height,
            &padding,
            width_changed,
            render,
            decide,
            window,
            cx,
        );
        window.take_autoscroll();
        if bounds.size.height > padding.top + padding.bottom {
            let mut origin = bounds.origin + crate::Point::new(px(0.), padding.top);
            origin.y -= layout.scroll_top.offset_in_item;
            for item in &mut layout.item_layouts {
                window.with_content_mask(Some(crate::ContentMask { bounds }), |window| {
                    item.element.prepaint_at(origin, window, cx);
                });
                window.take_autoscroll();
                origin.y += item.size.height;
            }
        } else {
            layout.item_layouts.clear();
        }
        layout
    }

    /// Writes the sizes measured in a layout into the list.
    fn store_sizes(&mut self, sizes: &BTreeMap<usize, Size<Pixels>>) {
        if sizes.is_empty() {
            return;
        }
        let mut cursor = self.items.cursor::<Count>(());
        let mut items = SumTree::default();
        for (&ix, &size) in sizes {
            items.append(cursor.slice(&Count(ix), Bias::Right), ());
            if let Some(item) = cursor.item() {
                items.push(
                    ListItem::Measured {
                        size,
                        focus_handle: item.focus_handle(),
                    },
                    (),
                );
                cursor.next();
            }
        }
        items.append(cursor.suffix(), ());
        drop(cursor);
        self.items = items;
    }
}

impl Moves {
    /// The whole device pixels a scroll by `delta` moves, with the carry of its gesture added and
    /// the part below a device pixel kept for the gesture's next delta.
    fn carry_scroll(&mut self, delta: Pixels, gesture: ScrollGesture, scale: f64) -> f64 {
        // (A logical delta of a whole number of device pixels can land a hair below it.)
        let asked = (delta.0 as f64 * scale * 1024.).round() / 1024.;
        let gap = match (gesture.at, self.last_scroll) {
            (Some(at), Some(last)) => at.saturating_duration_since(last) > GESTURE_GAP,
            _ => false,
        };
        let turned = asked != 0. && (asked > 0.) != self.carry_down;
        if gesture.starts || gap || turned {
            self.carry = 0.;
        }
        if asked != 0. {
            self.carry_down = asked > 0.;
        }
        self.last_scroll = gesture.at.or(self.last_scroll);
        let total = self.carry + asked;
        let whole = total.trunc();
        self.carry = total - whole;
        if gesture.ends {
            self.carry = 0.;
        }
        whole
    }
}

/// The place `by` device pixels further down the list from `place`.
fn forward(
    rows: &mut Rows,
    mut place: Place,
    by: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> Place {
    let mut left = place.offset + by;
    loop {
        if place.ix >= rows.count {
            return Place {
                ix: rows.count,
                offset: 0.,
            };
        }
        let row = rows.height(place.ix, render, window, cx);
        if left < row {
            return Place {
                ix: place.ix,
                offset: left,
            };
        }
        left -= row;
        place = Place {
            ix: place.ix + 1,
            offset: 0.,
        };
    }
}

/// The place `by` device pixels further up the list from `place`, the start at most.
fn back(
    rows: &mut Rows,
    place: Place,
    by: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> Place {
    let mut ix = place.ix;
    let mut offset = place.offset - by;
    while offset < 0. {
        if ix == 0 {
            return Place { ix: 0, offset: 0. };
        }
        ix -= 1;
        offset += rows.height(ix, render, window, cx);
    }
    Place { ix, offset }
}

/// How far the lines moved from `from` to `to`, in device pixels, down when positive.
fn position(
    rows: &mut Rows,
    from: Place,
    to: Place,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> f64 {
    if from == to {
        return 0.;
    }
    let (first, last, sign) = if (from.ix, from.offset) <= (to.ix, to.offset) {
        (from, to, -1.)
    } else {
        (to, from, 1.)
    };
    let mut distance = last.offset - first.offset;
    for ix in first.ix..last.ix {
        distance += rows.height(ix, render, window, cx);
    }
    sign * distance
}

/// How far the end, with the padding below it, is below the view's bottom edge with the view
/// at `place`, in device pixels, below zero where blank shows under it. `None` where it is
/// more than `beyond` below the edge, which isn't measured.
fn end_below(
    rows: &mut Rows,
    place: Place,
    room: f64,
    beyond: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> Option<f64> {
    let mut bottom = -place.offset;
    let mut ix = place.ix;
    while ix < rows.count {
        bottom += rows.height(ix, render, window, cx);
        if bottom - room > beyond {
            return None;
        }
        ix += 1;
    }
    Some(bottom - room)
}

/// The row `ix`'s top below the rows' top edge with the view at `place`, if it is within a
/// view's height of it.
fn top_of(
    rows: &mut Rows,
    place: Place,
    ix: usize,
    room: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> Option<f64> {
    if ix < place.ix {
        let mut top = -place.offset;
        for row in (ix..place.ix).rev() {
            top -= rows.height(row, render, window, cx);
            if top < -room * 2. {
                return None;
            }
        }
        return Some(top);
    }
    let mut top = -place.offset;
    for row in place.ix..ix {
        top += rows.height(row, render, window, cx);
        if top > room * 2. {
            return None;
        }
    }
    Some(top)
}

/// The place with the end on the bottom edge, or the start on the top edge where the rows don't
/// fill the view.
fn end_place(
    rows: &mut Rows,
    room: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> Place {
    let end = Place {
        ix: rows.count,
        offset: 0.,
    };
    back(rows, end, room, render, window, cx)
}

/// The rows the view shows from `place`, each with its top below the view's top, their
/// elements laid out, and the widest.
#[allow(clippy::too_many_arguments)]
fn draw(
    rows: &mut Rows,
    place: Place,
    top_pad: f64,
    view_bottom: f64,
    overdraw: Pixels,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> (
    std::collections::VecDeque<ItemLayout>,
    Vec<(usize, f64)>,
    Pixels,
) {
    let mut layouts = std::collections::VecDeque::new();
    let mut drawn = Vec::new();
    let mut max_width = px(0.);
    let mut top = top_pad - place.offset;
    let mut ix = place.ix;
    while ix < rows.count && top < view_bottom + top_pad {
        let size = match rows.elements.contains_key(&ix) {
            true => rows.sizes[&ix],
            false => rows.render(ix, render, window, cx),
        };
        let element = rows.elements.remove(&ix).expect("the row was just made");
        layouts.push_back(ItemLayout {
            index: ix,
            element,
            size,
        });
        drawn.push((ix, top));
        max_width = max_width.max(size.width);
        top += rows.device(size.height);
        ix += 1;
    }
    // The rows past the edges are measured, so a scroll onto them moves by their heights.
    let overdraw = rows.device(overdraw);
    let mut below = top;
    while ix < rows.count && below < view_bottom + overdraw {
        below += rows.height(ix, render, window, cx);
        ix += 1;
    }
    let mut above = place.offset;
    let mut ix = place.ix;
    while ix > 0 && above < overdraw {
        ix -= 1;
        above += rows.height(ix, render, window, cx);
    }
    (layouts, drawn, max_width)
}

/// The first row past the view's bottom edge with the view at `place`.
fn draw_extent(
    rows: &mut Rows,
    place: Place,
    room: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> usize {
    let mut bottom = -place.offset;
    let mut ix = place.ix;
    while ix < rows.count && bottom < room {
        bottom += rows.height(ix, render, window, cx);
        ix += 1;
    }
    ix
}

/// How far the rows the view shows from `place` reach past its bottom edge.
fn remainder(
    rows: &mut Rows,
    place: Place,
    room: f64,
    render: &mut RenderItemFn,
    window: &mut Window,
    cx: &mut App,
) -> f64 {
    let mut bottom = -place.offset;
    let mut ix = place.ix;
    while ix < rows.count && bottom < room {
        bottom += rows.height(ix, render, window, cx);
        ix += 1;
    }
    (room - bottom).min(0.)
}
