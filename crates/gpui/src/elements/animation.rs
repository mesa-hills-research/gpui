// Modified by Mesa Hills Research, as docs/upstream.md in mesa-hills-research/gpui describes.
use scheduler::Instant;
use std::{cell::Cell, rc::Rc, time::Duration};

use crate::{
    AnyElement, App, Bounds, Element, ElementId, FrameTime, GlobalElementId, InspectorElementId,
    IntoElement, PaintEffect, PaintEffectRecord, ParentElement, Pixels, SpringAnimation,
    SpringConfig, SpringPlayback, SpringState, SpringTarget, Window,
};

pub use easing::*;
use smallvec::SmallVec;

/// An animation that can be applied to an element.
#[derive(Clone)]
pub struct Animation {
    /// The amount of time for which this animation should run
    pub duration: Duration,
    /// Whether to repeat this animation when it finishes
    pub oneshot: bool,
    /// Whether to derive the phase from a shared clock. See [`Animation::repeat_synced`].
    pub synced: bool,
    /// A function that maps normalized time to an animated value.
    /// The result may exceed 0..1 for easing functions that overshoot.
    pub easing: Rc<dyn Fn(f32) -> f32>,
    /// The maximum number of times per second this animation re-renders.
    /// When `None`, the animation re-renders on every frame.
    pub max_fps: Option<f32>,
}

impl Animation {
    /// Create a new animation with the given duration.
    /// By default the animation will only run once and will use a linear easing function.
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            oneshot: true,
            synced: false,
            easing: Rc::new(linear),
            max_fps: None,
        }
    }

    /// Set the animation to loop when it finishes.
    pub fn repeat(mut self) -> Self {
        self.oneshot = false;
        self
    }

    /// Set the animation to loop when it finishes, phase-locked to a clock shared by the whole [`App`].
    pub fn repeat_synced(mut self) -> Self {
        self.oneshot = false;
        self.synced = true;
        self
    }

    /// Sets the easing function used to map normalized time to an animated value.
    ///
    /// The output is not clamped, allowing physical easing functions such as
    /// springs to overshoot.
    pub fn with_easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Rc::new(easing);
        self
    }

    /// Limit how often this animation re-renders. Instead of re-rendering on
    /// every frame, the animation schedules its next render at most
    /// `1 / max_fps` seconds after the current one. Values that are not
    /// finite and positive are ignored.
    ///
    /// The renders fall on a grid of `1 / max_fps` seconds shared by the
    /// whole [`App`], so the throttled animations of a window render in the
    /// same frames, however many there are and whenever each started: two
    /// spinners capped at 30 fps draw 30 frames a second, not 60.
    pub fn with_max_fps(mut self, max_fps: f32) -> Self {
        self.max_fps = Some(max_fps);
        self
    }
}

/// An extension trait for adding the animation wrapper to both Elements and Components
///
/// Animations rendered through this trait automatically respect
/// [`App::reduce_motion`](crate::App::reduce_motion): when it is set,
/// the element is rendered in a static state (the end state for oneshot
/// animations, the start state for repeating ones) and no animation frames are
/// scheduled.
pub trait AnimationExt {
    /// Render this component or element with an animation
    fn with_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(move |this, _, value| animator(this, value)),
            animations: smallvec::smallvec![animation],
        }
    }

    /// Render this component or element with a chain of animations
    fn with_animations(
        self,
        id: impl Into<ElementId>,
        animations: Vec<Animation>,
        animator: impl Fn(Self, usize, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(animator),
            animations: animations.into(),
        }
    }

    /// Render this component or element with an animation that changes how it paints and nothing
    /// else, with the [`PaintEffect`] that `effect` returns for the animation's value.
    ///
    /// The element is built and laid out once, and then painted in each frame with the effect
    /// for that frame. A view the element is in that is cached stays cached while the effect
    /// runs, and a frame draws the window's root view again and replays what the cached views
    /// drew, with the effect applied. Use [`Self::with_animation`] for an animation that changes
    /// the element itself.
    ///
    /// An effect can fade the element, turn its sprites and move everything it paints, as
    /// [`PaintEffect::translation`] describes. It changes what is painted and nothing else, so
    /// the element's hitboxes, bounds and focus stay where it was laid out. A click where an
    /// element that has moved is drawn doesn't reach it, and a click where it was laid out does.
    /// Effects of nested elements add up.
    ///
    /// Like [`Self::with_animation`], the effect holds still at its start, or at its end for a
    /// oneshot animation, while [`App::reduce_motion`](crate::App::reduce_motion) is set.
    fn with_paint_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        effect: impl Fn(f32) -> PaintEffect + 'static,
    ) -> PaintAnimationElement<Self>
    where
        Self: Sized,
    {
        PaintAnimationElement {
            id: id.into(),
            element: Some(self),
            animation,
            effect: Rc::new(effect),
        }
    }

    /// Renders this component or element at the value produced by a spring.
    ///
    /// The element ID preserves position and velocity across target changes.
    /// A newly mounted spring starts at its target unless configured with
    /// [`SpringAnimation::from`].
    fn with_spring<T>(
        self,
        id: impl Into<ElementId>,
        animation: SpringAnimation<T>,
        animator: impl FnOnce(Self, T::Output) -> Self + 'static,
    ) -> SpringAnimationElement<Self>
    where
        Self: Sized,
        T: SpringTarget,
        T::Output: 'static,
    {
        let SpringAnimation {
            config,
            target,
            epsilon,
            initial,
            playback,
        } = animation;
        let scalar_target = target.target();
        SpringAnimationElement {
            id: id.into(),
            element: Some(self),
            config,
            target: scalar_target,
            epsilon,
            initial,
            playback,
            animator: Some(Box::new(move |this, value| {
                animator(this, target.resolve(value))
            })),
        }
    }
}

impl<E: IntoElement + 'static> AnimationExt for E {}

/// A GPUI element that applies an animation to another element
pub struct AnimationElement<E> {
    id: ElementId,
    element: Option<E>,
    animations: SmallVec<[Animation; 1]>,
    animator: Box<dyn Fn(E, usize, f32) -> E + 'static>,
}

/// A GPUI element driven by a stateful spring.
pub struct SpringAnimationElement<E> {
    id: ElementId,
    element: Option<E>,
    config: SpringConfig,
    target: f32,
    epsilon: f32,
    initial: Option<f32>,
    playback: SpringPlayback,
    animator: Option<Box<dyn FnOnce(E, f32) -> E + 'static>>,
}

impl<E: ParentElement> ParentElement for SpringAnimationElement<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        let Some(element) = &mut self.element else {
            return;
        };

        element.extend(elements);
    }
}

impl<E> SpringAnimationElement<E> {
    /// Returns a new [`SpringAnimationElement<E>`] after applying the given function
    /// to the element being animated.
    pub fn map_element(mut self, f: impl FnOnce(E) -> E) -> SpringAnimationElement<E> {
        self.element = self.element.map(f);
        self
    }
}

impl<E: IntoElement + 'static> IntoElement for SpringAnimationElement<E> {
    type Element = SpringAnimationElement<E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: ParentElement> ParentElement for AnimationElement<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        let Some(element) = &mut self.element else {
            return;
        };

        element.extend(elements);
    }
}

impl<E> AnimationElement<E> {
    /// Returns a new [`AnimationElement<E>`] after applying the given function
    /// to the element being animated.
    pub fn map_element(mut self, f: impl FnOnce(E) -> E) -> AnimationElement<E> {
        self.element = self.element.map(f);
        self
    }
}

impl<E: IntoElement + 'static> IntoElement for AnimationElement<E> {
    type Element = AnimationElement<E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct AnimationState {
    start: Instant,
    animation_ix: usize,
    /// Whether a throttled re-render (see [`Animation::with_max_fps`]) is
    /// already scheduled, so overlapping renders don't stack extra timers.
    delayed_frame_pending: Rc<Cell<bool>>,
}

struct SpringElementState {
    spring: SpringState,
    target: f32,
    config: SpringConfig,
    initial: f32,
    playback: SpringPlayback,
    updated_at: Instant,
}

impl<E: IntoElement + 'static> Element for SpringAnimationElement<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        window.with_element_state(global_id.unwrap(), |state, window| {
            let now = window.frame_time().now;
            let initial = self.initial.unwrap_or(self.target);
            let mut state = state.unwrap_or_else(|| SpringElementState {
                spring: SpringState {
                    position: initial,
                    velocity: 0.0,
                },
                target: self.target,
                config: self.config,
                initial,
                playback: self.playback,
                updated_at: now,
            });

            let elapsed = now.duration_since(state.updated_at).as_secs_f32();
            match state.playback {
                SpringPlayback::Running => {
                    state.spring = state.config.step(state.spring, state.target, elapsed);
                }
                SpringPlayback::Paused
                | SpringPlayback::Stopped
                | SpringPlayback::Completed
                | SpringPlayback::Cancelled => {}
            }

            state.config = self.config;
            state.target = self.target;

            let done = match self.playback {
                SpringPlayback::Running => {
                    if cx.reduce_motion() {
                        state.spring = SpringState {
                            position: state.target,
                            velocity: 0.0,
                        };
                        true
                    } else {
                        let done =
                            state
                                .config
                                .is_settled(state.spring, state.target, self.epsilon);
                        if done {
                            state.spring = SpringState {
                                position: state.target,
                                velocity: 0.0,
                            };
                        }
                        done
                    }
                }
                SpringPlayback::Paused => true,
                SpringPlayback::Stopped => {
                    state.spring.velocity = 0.0;
                    true
                }
                SpringPlayback::Completed => {
                    state.spring = SpringState {
                        position: state.target,
                        velocity: 0.0,
                    };
                    true
                }
                SpringPlayback::Cancelled => {
                    state.spring = SpringState {
                        position: state.initial,
                        velocity: 0.0,
                    };
                    true
                }
            };
            state.playback = self.playback;
            state.updated_at = now;

            let element = self.element.take().expect("should only be called once");
            let animator = self.animator.take().expect("should only be called once");
            let mut element = animator(element, state.spring.position).into_any_element();

            if !done {
                window.request_animation_frame();
            }

            ((element.request_layout(window, cx), element), state)
        })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

impl<E: IntoElement + 'static> Element for AnimationElement<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        window.with_element_state(global_id.unwrap(), |state, window| {
            let now = window.frame_time().now;
            let mut state = state.unwrap_or_else(|| AnimationState {
                start: now,
                animation_ix: 0,
                delayed_frame_pending: Rc::new(Cell::new(false)),
            });
            let (animation_ix, delta, done) = if cx.reduce_motion() {
                let animation_ix = self.animations.len() - 1;
                let delta = if self.animations[animation_ix].oneshot {
                    1.0
                } else {
                    0.0
                };
                (animation_ix, delta, true)
            } else {
                let animation_ix = state.animation_ix;
                let animation = &self.animations[animation_ix];
                let duration = animation.duration;

                // An animation of no duration ends as it starts, as a paint
                // animation of no duration does.
                let mut delta = if duration.is_zero() {
                    1.0
                } else {
                    let elapsed = if animation.synced {
                        let elapsed = now.saturating_duration_since(cx.synced_animation_epoch);
                        // Reduce modulo the duration before f32 conversion, which loses sub-second precision at scale.
                        Duration::from_nanos((elapsed.as_nanos() % duration.as_nanos()) as u64)
                    } else {
                        now.saturating_duration_since(state.start)
                    };
                    elapsed.as_secs_f32() / duration.as_secs_f32()
                };

                let mut done = false;
                if animation.oneshot {
                    if delta >= 1.0 {
                        if animation_ix >= self.animations.len() - 1 {
                            done = true;
                        } else {
                            state.start = now;
                            state.animation_ix += 1;
                        }
                        delta = 1.0;
                    }
                } else if duration.is_zero() {
                    delta = 0.0;
                    done = true;
                } else if delta > 1.0 {
                    delta %= 1.0;
                }
                (animation_ix, delta, done)
            };
            let delta = (self.animations[animation_ix].easing)(delta);

            debug_assert!(delta.is_finite(), "animated value should be finite");

            let element = self.element.take().expect("should only be called once");
            let mut element = (self.animator)(element, animation_ix, delta).into_any_element();

            if !done {
                match self.animations[animation_ix].max_fps {
                    Some(max_fps) if max_fps.is_finite() && max_fps > 0.0 => {
                        if !state.delayed_frame_pending.get() {
                            state.delayed_frame_pending.set(true);
                            // The next point of a grid of `interval` from the
                            // app's synced epoch, shared by the throttled
                            // animations of the window: one timer wakes them all.
                            let interval = Duration::from_secs_f32(1.0 / max_fps);
                            let interval_nanos = (interval.as_nanos() as u64).max(1);
                            let since_epoch = cx
                                .background_executor()
                                .now()
                                .saturating_duration_since(cx.synced_animation_epoch)
                                .as_nanos() as u64;
                            let point = since_epoch / interval_nanos + 1;
                            let delay = Duration::from_nanos(point * interval_nanos - since_epoch);
                            let waiting =
                                (window.current_view(), state.delayed_frame_pending.clone());
                            let frames = window.throttled_animation_frames.clone();
                            let mut pending = frames.borrow_mut();
                            match pending.entry((interval_nanos, point)) {
                                std::collections::hash_map::Entry::Occupied(mut views) => {
                                    views.get_mut().push(waiting);
                                }
                                std::collections::hash_map::Entry::Vacant(entry) => {
                                    entry.insert(vec![waiting]);
                                    drop(pending);
                                    window
                                        .spawn(cx, async move |cx| {
                                            cx.background_executor().timer(delay).await;
                                            let views = frames
                                                .borrow_mut()
                                                .remove(&(interval_nanos, point))
                                                .unwrap_or_default();
                                            cx.update(move |_, cx| {
                                                for (view, pending) in views {
                                                    pending.set(false);
                                                    cx.notify(view);
                                                }
                                            })
                                            .ok();
                                        })
                                        .detach();
                                }
                            }
                        }
                    }
                    _ => window.request_animation_frame(),
                }
            }

            ((element.request_layout(window, cx), element), state)
        })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

/// An element painted with an animated [`PaintEffect`]. Created by
/// [`AnimationExt::with_paint_animation`].
pub struct PaintAnimationElement<E> {
    id: ElementId,
    element: Option<E>,
    animation: Animation,
    effect: Rc<dyn Fn(f32) -> PaintEffect>,
}

impl<E: IntoElement + 'static> IntoElement for PaintAnimationElement<E> {
    type Element = PaintAnimationElement<E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct PaintAnimationState {
    start: Instant,
}

/// When a paint effect started and how it runs, kept with the effect's record in a frame.
#[derive(Clone)]
pub(crate) struct PaintAnimationTiming {
    start: Instant,
    animation: Animation,
}

impl PaintAnimationTiming {
    /// The value of the animation at `frame_time`, and whether it has ended.
    pub(crate) fn value(&self, frame_time: &FrameTime) -> (f32, bool) {
        let animation = &self.animation;
        if frame_time.reduce_motion || animation.duration.is_zero() {
            return (
                (animation.easing)(if animation.oneshot { 1.0 } else { 0.0 }),
                true,
            );
        }
        let elapsed = if animation.synced && !animation.duration.is_zero() {
            let elapsed = frame_time.now.saturating_duration_since(frame_time.epoch);
            // Reduce modulo the duration before f32 conversion, which loses sub-second precision at scale.
            Duration::from_nanos((elapsed.as_nanos() % animation.duration.as_nanos()) as u64)
        } else {
            frame_time.now.saturating_duration_since(self.start)
        };
        let mut delta = elapsed.as_secs_f32() / animation.duration.as_secs_f32();
        let mut done = false;
        if delta > 1.0 {
            if animation.oneshot {
                done = true;
                delta = 1.0;
            } else {
                delta %= 1.0;
            }
        }
        let delta = (animation.easing)(delta);
        debug_assert!(delta.is_finite(), "animated value should be finite");
        (delta, done)
    }

    /// When the next frame of the animation is due, if it isn't the next frame drawn. That
    /// is the next point of the grid its frame rate caps it to, if it has one.
    pub(crate) fn next_frame_at(&self, frame_time: &FrameTime) -> Option<Instant> {
        let max_fps = self
            .animation
            .max_fps
            .filter(|max_fps| max_fps.is_finite() && *max_fps > 0.0)?;
        let interval = Duration::from_secs_f32(1.0 / max_fps);
        let interval_nanos = (interval.as_nanos() as u64).max(1);
        let since_epoch = frame_time
            .now
            .saturating_duration_since(frame_time.epoch)
            .as_nanos() as u64;
        let point = since_epoch / interval_nanos + 1;
        Some(frame_time.epoch + Duration::from_nanos(point * interval_nanos))
    }
}

impl<E: IntoElement + 'static> Element for PaintAnimationElement<E> {
    type RequestLayoutState = (AnyElement, Instant);
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        let start = window.with_element_state(global_id.unwrap(), |state, window| {
            let state = state.unwrap_or_else(|| PaintAnimationState {
                start: window.frame_time().now,
            });
            (state.start, state)
        });
        let mut element = self
            .element
            .take()
            .expect("should only be called once")
            .into_any_element();
        let layout_id = element.request_layout(window, cx);
        (layout_id, (element, start))
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        (element, _): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (element, start): &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let timing = PaintAnimationTiming {
            start: *start,
            animation: self.animation.clone(),
        };
        window.with_paint_effect(
            PaintEffectRecord::new(bounds, timing, self.effect.clone()),
            |window| element.paint(window, cx),
        );
    }
}

mod easing {
    use std::f32::consts::PI;

    /// The linear easing function, or delta itself
    pub fn linear(delta: f32) -> f32 {
        delta
    }

    /// The quadratic easing function, delta * delta
    pub fn quadratic(delta: f32) -> f32 {
        delta * delta
    }

    /// The quadratic ease-in-out function, which starts and ends slowly but speeds up in the middle
    pub fn ease_in_out(delta: f32) -> f32 {
        if delta < 0.5 {
            2.0 * delta * delta
        } else {
            let x = -2.0 * delta + 2.0;
            1.0 - x * x / 2.0
        }
    }

    /// The Quint ease-out function, which starts quickly and decelerates to a stop
    pub fn ease_out_quint() -> impl Fn(f32) -> f32 {
        move |delta| 1.0 - (1.0 - delta).powi(5)
    }

    /// Apply the given easing function, first in the forward direction and then in the reverse direction
    pub fn bounce(easing: impl Fn(f32) -> f32) -> impl Fn(f32) -> f32 {
        move |delta| {
            if delta < 0.5 {
                easing(delta * 2.0)
            } else {
                easing((1.0 - delta) * 2.0)
            }
        }
    }

    /// A custom easing function for pulsating alpha that slows down as it approaches 0.1
    pub fn pulsating_between(min: f32, max: f32) -> impl Fn(f32) -> f32 {
        let range = max - min;

        move |delta| {
            // Use a combination of sine and cubic functions for a more natural breathing rhythm
            let t = (delta * 2.0 * PI).sin();
            let breath = (t * t * t + t) / 2.0;

            // Map the breath to our desired alpha range
            let normalized_alpha = (breath + 1.0) / 2.0;

            min + (normalized_alpha * range)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use crate::{
        Animation, App, Context, Entity, InteractiveElement, Modifiers, MouseButton, Pixels,
        Render, SpringAnimation, SpringConfig, StyleRefinement, TestAppContext, VisualTestContext,
        WindowHandle, div, point, prelude::*, px, size,
    };

    use super::*;

    struct AnimationTestView {
        rendered_deltas: Rc<RefCell<Vec<f32>>>,
        max_fps: Option<f32>,
    }

    struct SyncedAnimationTestView {
        show_second: bool,
        first_deltas: Rc<RefCell<Vec<f32>>>,
        second_deltas: Rc<RefCell<Vec<f32>>>,
    }

    struct SpringAnimationTestView {
        target: Pixels,
        initial: Option<Pixels>,
        playback: SpringPlayback,
        rendered_values: Rc<RefCell<Vec<Pixels>>>,
    }

    impl Render for SpringAnimationTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let rendered_values = self.rendered_values.clone();
            let mut animation = SpringAnimation::new(SpringConfig::new(100.0, 2.0, 1.0))
                .to(self.target)
                .with_epsilon(0.01)
                .playback(self.playback);
            if let Some(initial) = self.initial {
                animation = animation.from(initial);
            }
            div().with_spring("spring-animation", animation, move |this, value| {
                rendered_values.borrow_mut().push(value);
                this.left(value)
            })
        }
    }

    impl Render for SyncedAnimationTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let record_deltas = |deltas: Rc<RefCell<Vec<f32>>>| {
                move |this, delta| {
                    deltas.borrow_mut().push(delta);
                    this
                }
            };
            div()
                .size_full()
                .child(div().with_animation(
                    "first-synced-animation",
                    Animation::new(Duration::from_secs(1)).repeat_synced(),
                    record_deltas(self.first_deltas.clone()),
                ))
                .when(self.show_second, |this| {
                    this.child(div().with_animation(
                        "second-synced-animation",
                        Animation::new(Duration::from_secs(1)).repeat_synced(),
                        record_deltas(self.second_deltas.clone()),
                    ))
                })
        }
    }

    impl Render for AnimationTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let rendered_deltas = self.rendered_deltas.clone();
            // The throttled variant syncs to the shared clock so the deltas
            // follow the test scheduler's clock rather than wall time.
            let mut animation = Animation::new(Duration::from_secs(1));
            if let Some(max_fps) = self.max_fps {
                animation = animation.repeat_synced().with_max_fps(max_fps);
            } else {
                animation = animation.repeat();
            }
            div().size_full().child(div().with_animation(
                "repeating-animation",
                animation,
                move |this, delta| {
                    rendered_deltas.borrow_mut().push(delta);
                    this
                },
            ))
        }
    }

    fn open_test_window(
        cx: &mut TestAppContext,
    ) -> (Rc<RefCell<Vec<f32>>>, WindowHandle<AnimationTestView>) {
        open_test_window_with_max_fps(cx, None)
    }

    fn open_test_window_with_max_fps(
        cx: &mut TestAppContext,
        max_fps: Option<f32>,
    ) -> (Rc<RefCell<Vec<f32>>>, WindowHandle<AnimationTestView>) {
        let rendered_deltas = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let rendered_deltas = rendered_deltas.clone();
            move |_, _| AnimationTestView {
                rendered_deltas,
                max_fps,
            }
        });
        cx.run_until_parked();
        (rendered_deltas, window)
    }

    fn simulate_next_frame<V: Render>(window: &WindowHandle<V>, cx: &mut TestAppContext) -> usize {
        let callback_count = window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        cx.run_until_parked();
        callback_count
    }
    // Before parent-animation-element, using .with_animation
    // would not allow chaining .parent after. This is just a
    // build check that we can call div().id().with_animation().child()
    #[test]
    fn test_animation_parent() {
        div()
            .id("id")
            //
            .with_animation(
                "animation",
                Animation::new(Duration::from_secs(1)),
                |el, _t| {
                    //
                    el
                },
            )
            .child(
                //
                div(),
            );
    }

    #[test]
    fn test_spring_animation_parent() {
        div()
            .id("id")
            .with_spring(
                "spring-animation",
                SpringAnimation::new(SpringConfig::new(100.0, 10.0, 1.0))
                    .to(px(10.0))
                    .from(px(0.0)),
                |element, value| element.left(value),
            )
            .child(div());
    }

    #[gpui::test]
    fn test_spring_animation_preserves_velocity_when_retargeted(cx: &mut TestAppContext) {
        let rendered_values = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.0), px(100.0)), {
            let rendered_values = rendered_values.clone();
            move |_, _| SpringAnimationTestView {
                target: px(0.0),
                initial: None,
                playback: SpringPlayback::Running,
                rendered_values,
            }
        });
        cx.run_until_parked();
        assert_eq!(*rendered_values.borrow(), vec![px(0.0)]);

        window
            .update(cx, |view, _, cx| {
                view.target = px(100.0);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();

        cx.executor().advance_clock(Duration::from_millis(50));
        assert!(simulate_next_frame(&window, cx) > 0);
        let value_before_retargeting = *rendered_values.borrow().last().unwrap();
        // 50 ms of simulated time moves the spring by about 12 px. A spring that reads the real
        // clock moves by well under a pixel, or not at all if the frame is quick.
        assert!(value_before_retargeting > px(1.0));
        assert!(value_before_retargeting < px(100.0));

        window
            .update(cx, |view, _, cx| {
                view.target = px(0.0);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();

        cx.executor().advance_clock(Duration::from_millis(5));
        assert!(simulate_next_frame(&window, cx) > 0);
        let value_after_retargeting = *rendered_values.borrow().last().unwrap();
        assert!(value_after_retargeting > value_before_retargeting);
    }

    #[gpui::test]
    fn test_paused_spring_resumes_with_its_velocity(cx: &mut TestAppContext) {
        let rendered_values = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.0), px(100.0)), {
            let rendered_values = rendered_values.clone();
            move |_, _| SpringAnimationTestView {
                target: px(0.0),
                initial: None,
                playback: SpringPlayback::Running,
                rendered_values,
            }
        });
        cx.run_until_parked();

        window
            .update(cx, |view, _, cx| {
                view.target = px(100.0);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(50));
        assert!(simulate_next_frame(&window, cx) > 0);

        window
            .update(cx, |view, _, cx| {
                view.target = px(0.0);
                view.playback = SpringPlayback::Paused;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let paused_value = *rendered_values.borrow().last().unwrap();

        cx.executor().advance_clock(Duration::from_millis(500));
        assert!(simulate_next_frame(&window, cx) > 0);
        assert_eq!(*rendered_values.borrow().last().unwrap(), paused_value);
        assert_eq!(simulate_next_frame(&window, cx), 0);

        window
            .update(cx, |view, _, cx| {
                view.playback = SpringPlayback::Running;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(5));
        assert!(simulate_next_frame(&window, cx) > 0);
        assert!(*rendered_values.borrow().last().unwrap() > paused_value);
    }

    #[gpui::test]
    fn test_stopped_spring_resumes_without_velocity(cx: &mut TestAppContext) {
        let rendered_values = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.0), px(100.0)), {
            let rendered_values = rendered_values.clone();
            move |_, _| SpringAnimationTestView {
                target: px(0.0),
                initial: None,
                playback: SpringPlayback::Running,
                rendered_values,
            }
        });
        cx.run_until_parked();

        window
            .update(cx, |view, _, cx| {
                view.target = px(1_000_000.0);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(50));
        assert!(simulate_next_frame(&window, cx) > 0);

        window
            .update(cx, |view, _, cx| {
                view.target = px(0.0);
                view.playback = SpringPlayback::Stopped;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let stopped_value = *rendered_values.borrow().last().unwrap();

        cx.executor().advance_clock(Duration::from_millis(500));
        assert!(simulate_next_frame(&window, cx) > 0);
        assert_eq!(*rendered_values.borrow().last().unwrap(), stopped_value);
        assert_eq!(simulate_next_frame(&window, cx), 0);

        window
            .update(cx, |view, _, cx| {
                view.target = stopped_value;
                view.playback = SpringPlayback::Running;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(*rendered_values.borrow().last().unwrap(), stopped_value);
        assert_eq!(simulate_next_frame(&window, cx), 0);
    }

    #[gpui::test]
    fn test_cancelled_and_completed_springs_resolve_their_endpoints(cx: &mut TestAppContext) {
        let rendered_values = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.0), px(100.0)), {
            let rendered_values = rendered_values.clone();
            move |_, _| SpringAnimationTestView {
                target: px(100.0),
                initial: Some(px(20.0)),
                playback: SpringPlayback::Running,
                rendered_values,
            }
        });
        cx.run_until_parked();
        assert_eq!(*rendered_values.borrow(), vec![px(20.0)]);

        cx.executor().advance_clock(Duration::from_millis(50));
        assert!(simulate_next_frame(&window, cx) > 0);
        assert!(*rendered_values.borrow().last().unwrap() > px(20.0));

        window
            .update(cx, |view, _, cx| {
                view.playback = SpringPlayback::Cancelled;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(*rendered_values.borrow().last().unwrap(), px(20.0));
        assert!(simulate_next_frame(&window, cx) > 0);
        assert_eq!(simulate_next_frame(&window, cx), 0);

        window
            .update(cx, |view, _, cx| {
                view.playback = SpringPlayback::Completed;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(*rendered_values.borrow().last().unwrap(), px(100.0));
        assert_eq!(simulate_next_frame(&window, cx), 0);
    }

    #[gpui::test]
    fn test_spring_animation_respects_reduced_motion(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let rendered_values = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.0), px(100.0)), {
            let rendered_values = rendered_values.clone();
            move |_, _| SpringAnimationTestView {
                target: px(100.0),
                initial: None,
                playback: SpringPlayback::Running,
                rendered_values,
            }
        });
        cx.run_until_parked();

        assert_eq!(*rendered_values.borrow(), vec![px(100.0)]);
        assert_eq!(simulate_next_frame(&window, cx), 0);
    }

    #[gpui::test]
    fn test_repeating_animation_schedules_animation_frames(cx: &mut TestAppContext) {
        let (rendered_deltas, window) = open_test_window(cx);

        assert_eq!(rendered_deltas.borrow().len(), 1);

        for expected_frames in 2..=3 {
            assert_eq!(simulate_next_frame(&window, cx), 1);
            assert_eq!(rendered_deltas.borrow().len(), expected_frames);
        }
    }

    #[gpui::test]
    fn test_max_fps_schedules_timer_driven_frames(cx: &mut TestAppContext) {
        let (rendered_deltas, window) = open_test_window_with_max_fps(cx, Some(10.0));

        // The test scheduler's clock jitters forward slightly on each poll,
        // so compare against expectations loosely.
        let assert_deltas_approx_eq = |expected: &[f32]| {
            let actual = rendered_deltas.borrow();
            assert_eq!(actual.len(), expected.len(), "deltas: {actual:?}");
            for (actual, expected) in actual.iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-2,
                    "expected {expected}, got {actual}"
                );
            }
        };

        assert_deltas_approx_eq(&[0.0]);

        // No per-frame callback is scheduled; re-renders are timer-driven.
        assert_eq!(simulate_next_frame(&window, cx), 0);
        assert_deltas_approx_eq(&[0.0]);

        cx.executor().advance_clock(Duration::from_millis(105));
        cx.run_until_parked();
        assert_deltas_approx_eq(&[0.0, 0.105]);

        cx.executor().advance_clock(Duration::from_millis(105));
        cx.run_until_parked();
        assert_deltas_approx_eq(&[0.0, 0.105, 0.21]);
    }

    /// Throttled animations started at different times, as indicators
    /// mounted one after the other.
    struct StaggeredThrottledView {
        shown: usize,
        renders: Rc<RefCell<usize>>,
    }

    impl Render for StaggeredThrottledView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            *self.renders.borrow_mut() += 1;
            div().size_full().children((0..self.shown).map(|ix| {
                div().with_animation(
                    ("throttled", ix),
                    Animation::new(Duration::from_secs(1))
                        .repeat()
                        .with_max_fps(30.0),
                    |this, _| this,
                )
            }))
        }
    }

    #[gpui::test]
    fn test_throttled_animations_render_in_the_same_frames(cx: &mut TestAppContext) {
        let renders = Rc::new(RefCell::new(0));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let renders = renders.clone();
            move |_, _| StaggeredThrottledView { shown: 1, renders }
        });
        cx.run_until_parked();
        // Three animations, mounted 15 ms and 10 ms apart.
        for _ in 0..2 {
            cx.executor().advance_clock(Duration::from_millis(15));
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    view.shown += 1;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        *renders.borrow_mut() = 0;
        for _ in 0..1000 {
            cx.executor().advance_clock(Duration::from_millis(1));
            cx.run_until_parked();
        }
        let renders = *renders.borrow();
        assert!(
            (29..=31).contains(&renders),
            "{renders} renders in a second for three animations capped at 30 fps"
        );
    }

    #[gpui::test]
    fn test_synced_animations_share_phase_across_elements(cx: &mut TestAppContext) {
        let first_deltas = Rc::new(RefCell::new(Vec::new()));
        let second_deltas = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let first_deltas = first_deltas.clone();
            let second_deltas = second_deltas.clone();
            move |_, _| SyncedAnimationTestView {
                show_second: false,
                first_deltas,
                second_deltas,
            }
        });
        cx.run_until_parked();

        assert_eq!(*first_deltas.borrow(), vec![0.0]);

        cx.executor().advance_clock(Duration::from_millis(250));
        simulate_next_frame(&window, cx);
        assert_eq!(*first_deltas.borrow(), vec![0.0, 0.25]);

        // The second element mounts a quarter through the cycle, yet renders
        // the shared phase rather than starting at zero.
        window
            .update(cx, |view, _, cx| {
                view.show_second = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(250));
        simulate_next_frame(&window, cx);

        assert_eq!(*second_deltas.borrow().last().unwrap(), 0.5);
        assert_eq!(
            *first_deltas.borrow().last().unwrap(),
            *second_deltas.borrow().last().unwrap()
        );
        assert!(second_deltas.borrow().iter().all(|delta| *delta > 0.0));

        // The phase wraps around each full cycle.
        cx.executor().advance_clock(Duration::from_millis(2250));
        simulate_next_frame(&window, cx);
        assert_eq!(*first_deltas.borrow().last().unwrap(), 0.75);

        // Sub-second precision survives months of uptime: converting the raw
        // elapsed time to f32 would round 0.25 away entirely.
        cx.executor()
            .advance_clock(Duration::from_secs(300 * 24 * 60 * 60) + Duration::from_millis(500));
        simulate_next_frame(&window, cx);
        assert_eq!(*first_deltas.borrow().last().unwrap(), 0.25);
    }

    #[gpui::test]
    fn test_reduce_motion_renders_single_static_frame(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (rendered_deltas, window) = open_test_window(cx);

        assert_eq!(*rendered_deltas.borrow(), vec![0.0]);

        assert_eq!(simulate_next_frame(&window, cx), 0);
        assert_eq!(*rendered_deltas.borrow(), vec![0.0]);
    }

    /// Animations that run one after the other, recording the stage and value
    /// each frame draws.
    struct ChainedAnimationView {
        animations: Vec<Animation>,
        rendered: Rc<RefCell<Vec<(usize, f32)>>>,
    }

    impl Render for ChainedAnimationView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let rendered = self.rendered.clone();
            div().size_full().child(div().with_animations(
                "chained-animation",
                self.animations.clone(),
                move |this, ix, delta| {
                    rendered.borrow_mut().push((ix, delta));
                    this
                },
            ))
        }
    }

    /// A window that runs `animations` one after the other, and a function
    /// that moves the clock by its argument and draws the next frame. That
    /// function returns how many frames the previous draw asked for, and the
    /// stage and value (to two places) of the frame drawn last.
    fn open_chain(
        cx: &mut TestAppContext,
        animations: Vec<Animation>,
    ) -> impl FnMut(Duration) -> (usize, usize, f32) + '_ {
        let rendered = Rc::new(RefCell::new(Vec::new()));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let rendered = rendered.clone();
            move |_, _| ChainedAnimationView {
                animations,
                rendered,
            }
        });
        cx.run_until_parked();
        let mut frames_asked_for = 0;
        move |elapsed: Duration| {
            if !elapsed.is_zero() {
                cx.executor().advance_clock(elapsed);
                frames_asked_for = simulate_next_frame(&window, cx);
            }
            let (ix, delta) = *rendered.borrow().last().unwrap();
            (frames_asked_for, ix, (delta * 100.0).round() / 100.0)
        }
    }

    fn seconds(seconds: f32) -> Animation {
        Animation::new(Duration::from_secs_f32(seconds))
    }

    #[gpui::test]
    fn test_animations_follow_the_executor_clock(cx: &mut TestAppContext) {
        let mut frame_after = open_chain(cx, vec![seconds(1.), seconds(1.)]);

        assert_eq!(frame_after(Duration::ZERO), (0, 0, 0.0));
        assert_eq!(frame_after(Duration::from_millis(400)), (1, 0, 0.4));
        // The first animation ends, and the second starts from that frame.
        assert_eq!(frame_after(Duration::from_millis(700)), (1, 0, 1.0));
        assert_eq!(frame_after(Duration::from_millis(250)), (1, 1, 0.25));
        assert_eq!(frame_after(Duration::from_millis(800)), (1, 1, 1.0));
        // The last animation has ended, so no frame asks for another.
        assert_eq!(frame_after(Duration::from_millis(100)), (0, 1, 1.0));
    }

    #[gpui::test]
    fn test_a_stage_ends_in_the_frame_drawn_at_its_duration(cx: &mut TestAppContext) {
        let mut frame_after = open_chain(cx, vec![seconds(1.), seconds(1.)]);

        assert_eq!(frame_after(Duration::from_secs(1)), (1, 0, 1.0));
        assert_eq!(frame_after(Duration::from_millis(250)), (1, 1, 0.25));
        assert_eq!(frame_after(Duration::from_millis(750)), (1, 1, 1.0));
        assert_eq!(frame_after(Duration::from_millis(100)), (0, 1, 1.0));
    }

    #[gpui::test]
    fn test_animations_of_no_duration_end_as_they_start(cx: &mut TestAppContext) {
        {
            let mut frame_after = open_chain(cx, vec![seconds(0.)]);
            assert_eq!(frame_after(Duration::ZERO), (0, 0, 1.0));
            assert_eq!(frame_after(Duration::from_millis(100)), (0, 0, 1.0));
        }
        {
            // A stage of no duration hands over to the next in one frame.
            let mut frame_after = open_chain(cx, vec![seconds(0.), seconds(1.)]);
            assert_eq!(frame_after(Duration::ZERO), (0, 0, 1.0));
            assert_eq!(frame_after(Duration::from_millis(250)), (1, 1, 0.25));
        }
        // A repeating one draws its start, as a paint animation of no
        // duration does.
        let mut frame_after = open_chain(cx, vec![seconds(0.).repeat()]);
        assert_eq!(frame_after(Duration::ZERO), (0, 0, 0.0));
        assert_eq!(frame_after(Duration::from_millis(100)), (0, 0, 0.0));
    }

    /// A view with a spinner that turns with a paint animation.
    struct PaintSpinnerView {
        animation: Animation,
        show: bool,
        renders: Rc<RefCell<usize>>,
        values: Rc<RefCell<Vec<f32>>>,
    }

    impl Render for PaintSpinnerView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            *self.renders.borrow_mut() += 1;
            let values = self.values.clone();
            div().size_full().when(self.show, |this| {
                this.child(div().size(px(10.)).bg(crate::red()).with_paint_animation(
                    "spinner",
                    self.animation.clone(),
                    move |value| {
                        values.borrow_mut().push(value);
                        PaintEffect::opacity(value)
                    },
                ))
            })
        }
    }

    /// A window whose root draws a spinner of its own above a cached view with one.
    struct PaintRootView {
        pane: Entity<PaintSpinnerView>,
        animation: Animation,
        show: bool,
        values: Rc<RefCell<Vec<f32>>>,
    }

    impl Render for PaintRootView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let values = self.values.clone();
            div()
                .size_full()
                .when(self.show, |this| {
                    this.child(div().size(px(10.)).bg(crate::red()).with_paint_animation(
                        "root spinner",
                        self.animation.clone(),
                        move |value| {
                            values.borrow_mut().push(value);
                            PaintEffect::opacity(value)
                        },
                    ))
                })
                .child(
                    self.pane
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(10.))),
                )
        }
    }

    struct PaintFixture {
        window: WindowHandle<PaintRootView>,
        pane: Entity<PaintSpinnerView>,
        pane_renders: Rc<RefCell<usize>>,
        pane_values: Rc<RefCell<Vec<f32>>>,
        root_values: Rc<RefCell<Vec<f32>>>,
    }

    impl PaintFixture {
        fn open(cx: &mut TestAppContext, animation: Animation) -> Self {
            let pane_renders = Rc::new(RefCell::new(0));
            let pane_values = Rc::new(RefCell::new(Vec::new()));
            let root_values = Rc::new(RefCell::new(Vec::new()));
            let window = cx.open_window(size(px(100.), px(100.)), {
                let pane_renders = pane_renders.clone();
                let pane_values = pane_values.clone();
                let root_values = root_values.clone();
                move |_, cx| {
                    let pane = cx.new(|_| PaintSpinnerView {
                        animation: animation.clone(),
                        show: true,
                        renders: pane_renders,
                        values: pane_values,
                    });
                    PaintRootView {
                        pane,
                        animation,
                        show: true,
                        values: root_values,
                    }
                }
            });
            cx.run_until_parked();
            let pane = window.update(cx, |root, _, _| root.pane.clone()).unwrap();
            Self {
                window,
                pane,
                pane_renders,
                pane_values,
                root_values,
            }
        }

        /// The opacity of each spinner in the frame drawn last, from the top.
        fn spinner_alphas(&self, cx: &mut TestAppContext) -> Vec<f32> {
            self.window
                .update(cx, |_, window, _| {
                    let scale_factor = window.scale_factor();
                    let mut spinners: Vec<_> = window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .filter(|quad| quad.bounds.size.width.0 / scale_factor == 10.)
                        .map(|quad| (quad.bounds.origin.y.0, quad.background.solid.a))
                        .collect();
                    spinners.sort_by(|a, b| a.0.total_cmp(&b.0));
                    spinners.into_iter().map(|(_, alpha)| alpha).collect()
                })
                .unwrap()
        }

        fn advance(&self, cx: &mut TestAppContext, duration: Duration) -> usize {
            cx.executor().advance_clock(duration);
            simulate_next_frame(&self.window, cx)
        }
    }

    fn synced() -> Animation {
        Animation::new(Duration::from_secs(1)).repeat_synced()
    }

    #[gpui::test]
    fn test_paint_animation_runs_in_a_view_that_is_reused(cx: &mut TestAppContext) {
        let fixture = PaintFixture::open(cx, synced());
        assert_eq!(*fixture.pane_renders.borrow(), 1);

        let mut seen = Vec::new();
        for _ in 0..8 {
            assert_eq!(fixture.advance(cx, Duration::from_millis(100)), 1);
            let alphas = fixture.spinner_alphas(cx);
            assert_eq!(alphas.len(), 2);
            // The spinner in the cached view turns as the one above it, which is drawn again.
            assert_eq!(alphas[0], alphas[1]);
            seen.push(alphas[0]);
        }
        assert_eq!(
            *fixture.pane_renders.borrow(),
            1,
            "the view isn't drawn again"
        );
        assert!(seen.windows(2).all(|pair| pair[0] < pair[1]), "{seen:?}");
        assert!(
            (seen[0] - 0.1).abs() < 0.05 && (seen[7] - 0.8).abs() < 0.05,
            "{seen:?}"
        );
        // The effect is worked out once for each spinner in each frame.
        assert_eq!(
            fixture.pane_values.borrow().len(),
            fixture.root_values.borrow().len()
        );
    }

    #[gpui::test]
    fn test_paint_animation_keeps_running_when_its_view_draws_again(cx: &mut TestAppContext) {
        let fixture = PaintFixture::open(cx, Animation::new(Duration::from_secs(1)).repeat());
        fixture.advance(cx, Duration::from_millis(300));
        let before = fixture.spinner_alphas(cx)[1];

        fixture.pane.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(*fixture.pane_renders.borrow(), 2);
        let after = fixture.spinner_alphas(cx)[1];
        assert!(
            before > 0.25 && (after - before).abs() < 0.05,
            "{before} {after}"
        );

        fixture.advance(cx, Duration::from_millis(300));
        assert!(fixture.spinner_alphas(cx)[1] > after + 0.25);
    }

    #[gpui::test]
    fn test_paint_animation_stops_asking_for_frames_when_it_ends(cx: &mut TestAppContext) {
        let fixture = PaintFixture::open(cx, Animation::new(Duration::from_secs(1)));
        assert_eq!(fixture.advance(cx, Duration::from_millis(400)), 1);
        assert!(fixture.spinner_alphas(cx)[0] < 0.6);
        assert_eq!(fixture.advance(cx, Duration::from_millis(700)), 1);
        // It ended in that frame, painted as at its end, and nothing asks for another.
        assert_eq!(fixture.spinner_alphas(cx), [1., 1.]);
        assert_eq!(fixture.advance(cx, Duration::from_millis(100)), 0);
        assert_eq!(fixture.spinner_alphas(cx), [1., 1.]);
        assert_eq!(*fixture.pane_renders.borrow(), 1);
    }

    #[gpui::test]
    fn test_paint_animation_respects_reduced_motion(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let fixture = PaintFixture::open(cx, synced());
        assert_eq!(fixture.spinner_alphas(cx), [0., 0.]);
        assert_eq!(fixture.advance(cx, Duration::from_millis(300)), 0);
        assert_eq!(fixture.spinner_alphas(cx), [0., 0.]);

        let oneshot = PaintFixture::open(cx, Animation::new(Duration::from_secs(1)));
        assert_eq!(oneshot.spinner_alphas(cx), [1., 1.]);
        assert_eq!(oneshot.advance(cx, Duration::from_millis(300)), 0);
    }

    #[gpui::test]
    fn test_throttled_paint_animation_draws_on_a_timer(cx: &mut TestAppContext) {
        let fixture = PaintFixture::open(cx, synced().with_max_fps(10.));
        let draws = |fixture: &PaintFixture, cx: &mut TestAppContext| {
            fixture
                .window
                .update(cx, |_, window, _| window.frame_number)
                .unwrap()
        };

        // No frame is asked for. A draw is due when the timer fires.
        assert_eq!(simulate_next_frame(&fixture.window, cx), 0);
        let first = draws(&fixture, cx);
        let first_alphas = fixture.spinner_alphas(cx);
        cx.executor().advance_clock(Duration::from_millis(105));
        cx.run_until_parked();
        assert_eq!(draws(&fixture, cx), first + 1);
        let second_alphas = fixture.spinner_alphas(cx);
        assert_eq!(second_alphas[0], second_alphas[1]);
        assert!(second_alphas[0] > first_alphas[0] + 0.05);

        // A second of time is ten draws, and the cached view isn't drawn again.
        for _ in 0..1000 {
            cx.executor().advance_clock(Duration::from_millis(1));
            cx.run_until_parked();
        }
        let drawn = draws(&fixture, cx) - first - 1;
        assert!(
            (9..=11).contains(&drawn),
            "{drawn} draws in a second at 10 fps"
        );
        assert_eq!(*fixture.pane_renders.borrow(), 1);
    }

    #[gpui::test]
    fn test_frames_stop_when_no_paint_animation_is_running(cx: &mut TestAppContext) {
        let fixture = PaintFixture::open(cx, synced());
        // The views are drawn without their spinners, but one frame was asked for before.
        fixture.pane.update(cx, |pane, cx| {
            pane.show = false;
            cx.notify();
        });
        fixture
            .window
            .update(cx, |root, _, cx| {
                root.show = false;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(fixture.spinner_alphas(cx).len(), 0);
        // Frames are asked for once, however many draws there were since.
        assert_eq!(fixture.advance(cx, Duration::from_millis(100)), 1);
        assert_eq!(fixture.advance(cx, Duration::from_millis(100)), 0);
    }

    #[gpui::test]
    fn test_one_frame_is_asked_for_however_often_the_window_draws(cx: &mut TestAppContext) {
        let fixture = PaintFixture::open(cx, synced());
        for _ in 0..3 {
            fixture.pane.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
        }
        assert_eq!(fixture.advance(cx, Duration::from_millis(100)), 1);
    }

    /// A view with a box that rises and fades as it is pressed, inside a box that slides
    /// across, both with paint animations. It counts how often it renders and how often the
    /// inner box is pressed.
    struct SlideView {
        renders: Rc<RefCell<usize>>,
        presses: Rc<RefCell<usize>>,
    }

    impl Render for SlideView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            *self.renders.borrow_mut() += 1;
            let presses = self.presses.clone();
            div().size_full().child(
                div()
                    .size(px(10.))
                    .child(
                        div()
                            .size(px(10.))
                            .bg(crate::red())
                            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                                *presses.borrow_mut() += 1;
                            })
                            .with_paint_animation(
                                "rise",
                                Animation::new(Duration::from_secs(1)),
                                |value| {
                                    PaintEffect::translation(point(px(0.), px(40. * value)))
                                        .with_opacity(1. - value)
                                },
                            ),
                    )
                    .with_paint_animation(
                        "slide",
                        Animation::new(Duration::from_secs(1)),
                        |value| PaintEffect::translation(point(px(100. * value), px(0.))),
                    ),
            )
        }
    }

    struct SlideRoot {
        pane: Entity<SlideView>,
    }

    impl Render for SlideRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.pane
                    .clone()
                    .cached(StyleRefinement::default().w_full().h(px(10.))),
            )
        }
    }

    /// Where the box is drawn, in logical pixels from the top left, and how opaque it is.
    fn drawn_box(cx: &mut VisualTestContext) -> (f32, f32, f32) {
        cx.update(|window, _| {
            let scale_factor = window.scale_factor();
            let boxes: Vec<_> = window
                .rendered_frame
                .scene
                .quads
                .iter()
                .filter(|quad| quad.bounds.size.width.0 / scale_factor == 10.)
                .map(|quad| {
                    (
                        quad.bounds.origin.x.0 / scale_factor,
                        quad.bounds.origin.y.0 / scale_factor,
                        quad.background.solid.a,
                    )
                })
                .collect();
            assert_eq!(boxes.len(), 1);
            boxes[0]
        })
    }

    fn next_frame(cx: &mut VisualTestContext, by: Duration) -> usize {
        cx.executor().advance_clock(by);
        let asked = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        asked
    }

    #[gpui::test]
    fn test_paint_animation_moves_what_it_paints_in_a_view_that_is_reused(cx: &mut TestAppContext) {
        let renders = Rc::new(RefCell::new(0));
        let presses = Rc::new(RefCell::new(0));
        let (_, cx) = cx.add_window_view({
            let renders = renders.clone();
            let presses = presses.clone();
            move |_, cx| SlideRoot {
                pane: cx.new(|_| SlideView { renders, presses }),
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(*renders.borrow(), 1);
        assert_eq!(drawn_box(cx), (0., 0., 1.));

        // The two translations add up, and the opacities multiply, in each frame, and the
        // cached view is not drawn again.
        for step in 1..=4 {
            assert_eq!(next_frame(cx, Duration::from_millis(250)), 1);
            let value = step as f32 * 0.25;
            let (x, y, alpha) = drawn_box(cx);
            assert!(
                (x - 100. * value).abs() < 0.01
                    && (y - 40. * value).abs() < 0.01
                    && (alpha - (1. - value)).abs() < 0.01,
                "step {step}: {:?}",
                (x, y, alpha)
            );
        }
        assert_eq!(*renders.borrow(), 1, "the view isn't drawn again");

        // The animations ended in that frame, at the end of the move, and nothing asks for more.
        assert_eq!(next_frame(cx, Duration::from_millis(100)), 1);
        assert_eq!(drawn_box(cx), (100., 40., 0.));
        assert_eq!(next_frame(cx, Duration::from_millis(100)), 0);
        assert_eq!(drawn_box(cx), (100., 40., 0.));

        // Pressing hits the box where it was laid out, and not where it is drawn.
        cx.simulate_mouse_down(
            point(px(105.), px(45.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert_eq!(*presses.borrow(), 0);
        cx.simulate_mouse_down(point(px(5.), px(5.)), MouseButton::Left, Modifiers::none());
        assert_eq!(*presses.borrow(), 1);
    }

    #[gpui::test]
    fn test_a_translation_moves_with_a_view_that_draws_again(cx: &mut TestAppContext) {
        let renders = Rc::new(RefCell::new(0));
        let (root, cx) = cx.add_window_view({
            let renders = renders.clone();
            move |_, cx| SlideRoot {
                pane: cx.new(|_| SlideView {
                    renders,
                    presses: Rc::default(),
                }),
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        next_frame(cx, Duration::from_millis(500));
        let before = drawn_box(cx);
        assert!((before.0 - 50.).abs() < 0.01 && (before.1 - 20.).abs() < 0.01);

        // The view draws, and the animations go on from where they were.
        let pane = root.read_with(cx, |root, _| root.pane.clone());
        pane.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(*renders.borrow(), 2);
        let after = drawn_box(cx);
        assert!(
            (after.0 - before.0).abs() < 0.01 && (after.1 - before.1).abs() < 0.01,
            "{before:?} {after:?}"
        );
        next_frame(cx, Duration::from_millis(250));
        assert!((drawn_box(cx).0 - 75.).abs() < 0.01);
    }

    /// A view with a spinner of 10 pixels above its children, which are cached views.
    struct SpinnerNode {
        /// Each child and its height, which is that of its spinner and of the views below it.
        children: Vec<(Entity<SpinnerNode>, f32)>,
    }

    impl SpinnerNode {
        /// A tree of views `depth` deep with its nodes, and its height.
        fn tree(
            depth: usize,
            nodes: &mut Vec<Entity<SpinnerNode>>,
            cx: &mut App,
        ) -> (Entity<Self>, f32) {
            let children: Vec<_> = if depth == 0 {
                Vec::new()
            } else {
                (0..2)
                    .map(|_| SpinnerNode::tree(depth - 1, nodes, cx))
                    .collect()
            };
            let height = 10. + children.iter().map(|(_, height)| height).sum::<f32>();
            let node = cx.new(|_| SpinnerNode { children });
            nodes.push(node.clone());
            (node, height)
        }
    }

    impl Render for SpinnerNode {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div()
                        .size(px(10.))
                        .flex_none()
                        .bg(crate::red())
                        .with_paint_animation("spinner", synced(), PaintEffect::opacity),
                )
                .children(self.children.iter().map(|(child, height)| {
                    child.clone().cached(
                        StyleRefinement::default()
                            .w_full()
                            .h(px(*height))
                            .flex_none(),
                    )
                }))
        }
    }

    struct SpinnerTree {
        root: Entity<SpinnerNode>,
        height: f32,
        /// How many more spinners are drawn ahead of the tree, which move the records of the
        /// views below them.
        leading: usize,
    }

    impl Render for SpinnerTree {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children((0..self.leading).map(|ix| {
                    div()
                        .absolute()
                        .size(px(10.))
                        .bg(crate::red())
                        .with_paint_animation(("leading", ix), synced(), PaintEffect::opacity)
                }))
                // A spinner that is drawn again in every frame, to compare the others with.
                .child(
                    div()
                        .size(px(10.))
                        .flex_none()
                        .bg(crate::red())
                        .with_paint_animation("spinner", synced(), PaintEffect::opacity),
                )
                .child(
                    self.root.clone().cached(
                        StyleRefinement::default()
                            .w_full()
                            .h(px(self.height))
                            .flex_none(),
                    ),
                )
        }
    }

    /// Spinners in cached views below cached views turn as they do in a view that is drawn
    /// again, frame after frame, while views are notified and the window is notified.
    #[gpui::test]
    fn test_paint_animations_in_nested_cached_views_turn_together(cx: &mut TestAppContext) {
        use rand::{Rng as _, SeedableRng as _, rngs::StdRng};
        for seed in 0..12 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut nodes = Vec::new();
            let (root, height) = cx.update(|cx| SpinnerNode::tree(3, &mut nodes, cx));
            let window = cx.open_window(size(px(100.), px(1000.)), {
                let root = root.clone();
                move |_, _| SpinnerTree {
                    root,
                    height,
                    leading: 0,
                }
            });
            cx.run_until_parked();

            for step in 0..60 {
                match rng.random_range(0..4) {
                    0 => {
                        let node = nodes[rng.random_range(0..nodes.len())].clone();
                        node.update(cx, |_, cx| cx.notify());
                    }
                    1 => window
                        .update(cx, |tree, _, cx| {
                            tree.leading += 1;
                            cx.notify();
                        })
                        .unwrap(),
                    _ => {}
                }
                cx.executor()
                    .advance_clock(Duration::from_millis(rng.random_range(1..=300)));
                simulate_next_frame(&window, cx);
                let alphas = window
                    .update(cx, |_, window, _| {
                        let scale_factor = window.scale_factor();
                        window
                            .rendered_frame
                            .scene
                            .quads
                            .iter()
                            .filter(|quad| quad.bounds.size.width.0 / scale_factor == 10.)
                            .map(|quad| quad.background.solid.a)
                            .collect::<Vec<_>>()
                    })
                    .unwrap();
                // The spinner of the window, and 1 + 2 + 4 + 8 in the views, all at the same point of
                // the same animation.
                let leading = window.update(cx, |tree, _, _| tree.leading).unwrap();
                assert_eq!(
                    alphas.len(),
                    nodes.len() + 1 + leading,
                    "seed {seed}, step {step}"
                );
                assert!(
                    alphas.iter().all(|alpha| *alpha == alphas[0]),
                    "seed {seed}, step {step}: {alphas:?}"
                );
            }
            window
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            cx.run_until_parked();
        }
    }

    /// A view whose spinner is an icon turned by a paint animation.
    struct IconView;

    impl Render for IconView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                crate::canvas(
                    |_, _, _| (),
                    |bounds, _, window, cx| {
                        let icon = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="4" height="10"/></svg>"#;
                        window
                            .paint_svg(
                                bounds,
                                "icon.svg".into(),
                                Some(icon),
                                crate::TransformationMatrix::unit(),
                                crate::red(),
                                cx,
                            )
                            .unwrap();
                    },
                )
                .size(px(10.))
                .with_paint_animation("icon", synced(), |value| {
                    PaintEffect::rotation(crate::radians(value * std::f32::consts::TAU))
                }),
            )
        }
    }

    #[gpui::test]
    fn test_paint_animation_turns_an_icon(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| IconView);
        let turned = |cx: &mut TestAppContext| {
            window
                .update(cx, |_, window, _| {
                    let sprites = &window.rendered_frame.scene.monochrome_sprites;
                    assert_eq!(sprites.len(), 1);
                    sprites[0].transformation.rotation_scale
                })
                .unwrap()
        };
        cx.run_until_parked();
        let at_start = turned(cx);
        assert_eq!(at_start, [[1., 0.], [0., 1.]]);

        // A quarter of a second into a second, a quarter turn.
        cx.executor().advance_clock(Duration::from_millis(250));
        simulate_next_frame(&window, cx);
        let quarter = turned(cx);
        assert!(
            quarter[0][0].abs() < 0.05 && (quarter[1][0] - 1.).abs() < 0.05,
            "{quarter:?}"
        );
    }
}
