use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, AvailableSpace, Bounds, ContentMask, Context,
    Element, ElementId, Entity, EntityId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintIndex, Pixels, PrepaintStateIndex, Render, RenderOnce, Size, Style,
    StyleRefinement, TextStyle, WeakEntity,
};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use std::{
    any::{TypeId, type_name},
    fmt,
    ops::Range,
};

/// A dynamically-typed view handle that can be downcast to a specific `Entity<V>`.
///
/// This is the type-erased counterpart to [`ViewElement`]: it holds an entity plus
/// a function pointer to its render, and is itself a [`View`], so embedding it as an
/// element goes through the same [`ViewElement`] machinery as any other view.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
        }
    }
}

impl AnyView {
    /// Embed this view as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is recycled from the previous frame unless
    /// [Context::notify] was called on the backing entity since it was rendered
    /// (or [Window::refresh] is called, which ignores caching). That holds
    /// inside another cached view too, when the outer view redraws.
    pub fn cached(self, style: StyleRefinement) -> ViewElement<AnyView> {
        ViewElement::new(self).cached(style)
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// The [`EntityId`] of this view.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

/// `AnyView` is the type-erased [`View`]: its `render` is a function pointer rather
/// than a concrete type, but it participates in the reactive graph exactly like any
/// other view via [`ViewElement`].
impl View for AnyView {
    fn entity_id(&self) -> Option<EntityId> {
        Some(self.entity.entity_id())
    }

    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.render)(&self, window, cx)
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = ViewElement<Entity<V>>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }

    #[inline(never)]
    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

/// A weak, dynamically-typed view handle.
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl AnyWeakView {
    /// Upgrade to a strong `AnyView` handle, if the view is still alive.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        // Record the view's Render type name so the accessibility debug dump can
        // attribute nodes to the view that produced them.
        #[cfg(debug_assertions)]
        window
            .a11y
            .view_type_names
            .insert(view.entity_id(), std::any::type_name::<V>());
        view.update(cx, |view, cx| view.render(window, cx).into_any_element())
    }
}

/// A renderable that participates in GPUI's reactive graph — the unifying model
/// behind [`Render`] and [`RenderOnce`].
///
/// When `entity_id()` returns `Some`, that id becomes the view's identity: it gets
/// a unique element-id space (so internal `use_state` / `.id(..)` never collide
/// across siblings) and `cx.notify()` on that entity re-renders only this view's
/// subtree. `None` behaves like a stateless component.
///
/// You rarely implement `View` directly. `Entity<T: Render>` and any `T: RenderOnce`
/// get a blanket impl below; implement it by hand only when a component needs both
/// parent-supplied props *and* a backing entity for identity.
pub trait View: 'static + Sized {
    /// This view's identity, if it has one. A view typically holds the backing
    /// entity as a field and returns its [`EntityId`] here.
    ///
    /// The id becomes this view's [`ElementId`], so two views keyed on the same
    /// entity must not be rendered at the same position in the element tree
    /// (e.g. as siblings under the same parent): their internal element state
    /// (`use_state`, scroll offsets, etc.) would silently collide. Nesting is
    /// fine — the id is scoped by the parent path.
    fn entity_id(&self) -> Option<EntityId>;

    /// Render this view into an element tree, consuming `self`.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// A stateless component (`RenderOnce`) is a `View` with no identity.
impl<T: RenderOnce> View for T {
    fn entity_id(&self) -> Option<EntityId> {
        None
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        RenderOnce::render(self, window, cx)
    }
}

/// An entity that renders itself (`Render`) is a `View` keyed on its own id.
impl<T: Render> View for Entity<T> {
    fn entity_id(&self) -> Option<EntityId> {
        Some(Entity::entity_id(self))
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.update(cx, |this, cx| {
            Render::render(this, window, cx).into_any_element()
        })
    }
}

impl<T: Render> Entity<T> {
    /// Embed this entity as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is reused until the entity is notified (or the
    /// cached bounds / text style change), inside another cached view too,
    /// when the outer view redraws. Caching requires a definite size:
    /// a cached view is laid out from `style` and is *not* measured from its
    /// contents. Use [`ViewElement::new`] (or `.child(entity)`) for the
    /// uncached case.
    #[track_caller]
    pub fn cached(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        ViewElement::new(self).cached(style)
    }
}

/// The element type for [`View`] implementations. Wraps a `View` and hooks it
/// into layout, prepaint, and paint. Constructed via [`ViewElement::new`].
#[doc(hidden)]
pub struct ViewElement<V: View> {
    view: Option<V>,
    entity_id: Option<EntityId>,
    cached_style: Option<StyleRefinement>,
    #[cfg(debug_assertions)]
    source: &'static core::panic::Location<'static>,
}

impl<V: View> ViewElement<V> {
    /// Wrap a [`View`] as an element.
    #[track_caller]
    pub fn new(view: V) -> Self {
        let entity_id = view.entity_id();
        ViewElement {
            entity_id,
            cached_style: None,
            view: Some(view),
            #[cfg(debug_assertions)]
            source: core::panic::Location::caller(),
        }
    }

    /// Enable caching of this view's rendered subtree, laid out at `style`.
    /// The composer supplies the layout style because caching skips rendering
    /// the contents to measure them.
    ///
    /// Crate-private on purpose: caching is only sound for entity-backed views,
    /// where [`Context::notify`] is the contract that busts the cache. A stateless
    /// view has no such contract, so a frozen subtree could never be invalidated.
    /// Reach this through [`Entity::cached`] or [`AnyView::cached`], which are
    /// entity-backed by construction.
    pub(crate) fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self
    }
}

impl<V: View> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(crate) struct ViewElementState {
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    /// The [`Window::frame_number`]s of the frames that `prepaint_range` and
    /// `paint_range` index. A view is reused only if both are the rendered
    /// frame's.
    prepaint_frame: u64,
    paint_frame: u64,
    cache_key: ViewElementCacheKey,
    accessed_entities: FxHashSet<EntityId>,
}

impl ViewElementState {
    /// Follows a range of the rendered frame, starting at `from`, that this
    /// view was drawn inside, to `to` in frame `frame_number`, where
    /// [`Window::reuse_prepaint`] copied it.
    pub(crate) fn follow_reused_prepaint(
        &mut self,
        from: &PrepaintStateIndex,
        to: &PrepaintStateIndex,
        frame_number: u64,
    ) {
        if self.prepaint_frame + 1 == frame_number {
            let range = &self.prepaint_range;
            self.prepaint_range = range.start.rebased(from, to)..range.end.rebased(from, to);
            self.prepaint_frame = frame_number;
        }
    }

    /// Follows a range of the rendered frame, starting at `from`, that this
    /// view was painted inside, to `to` in frame `frame_number`, where
    /// [`Window::reuse_paint`] copied it.
    pub(crate) fn follow_reused_paint(
        &mut self,
        from: &PaintIndex,
        to: &PaintIndex,
        frame_number: u64,
    ) {
        if self.paint_frame + 1 == frame_number {
            let range = &self.paint_range;
            self.paint_range = range.start.rebased(from, to)..range.end.rebased(from, to);
            self.paint_frame = frame_number;
        }
    }
}

struct ViewElementCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
}

impl<V: View> Element for ViewElement<V> {
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        self.entity_id.map(ElementId::View)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        return None;
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path: create a reactive boundary.
            let view = &mut self.view;
            request_layout_view(
                entity_id,
                self.cached_style.as_ref(),
                window,
                cx,
                &mut |window, cx| view.take().unwrap().render(window, cx).into_any_element(),
            )
        } else {
            // Stateless path: isolate subtree via type name (no entity identity).
            request_layout_component(type_name::<V>(), window, cx, &mut |window, cx| {
                self.view
                    .take()
                    .unwrap()
                    .render(window, cx)
                    .into_any_element()
            })
        }
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            prepaint_view(
                entity_id,
                global_id,
                bounds,
                element,
                window,
                cx,
                &mut |window, cx| {
                    self.view
                        .take()
                        .unwrap()
                        .render(window, cx)
                        .into_any_element()
                },
            )
        } else {
            // Stateless path: just prepaint the element.
            prepaint_component(type_name::<V>(), element, window, cx)
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            paint_view(
                entity_id,
                self.cached_style.is_some(),
                global_id,
                element,
                window,
                cx,
            );
        } else {
            // Stateless path: just paint the element.
            paint_component(std::any::type_name::<V>(), element, window, cx);
        }
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[inline(never)]
fn request_layout_view(
    entity_id: EntityId,
    cached_style: Option<&StyleRefinement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        match cached_style {
            Some(style) if !caching_disabled => {
                let mut root_style = Style::default();
                root_style.refine(style);
                let layout_id = window.request_layout(root_style, None, cx);
                (layout_id, None)
            }
            _ => {
                let mut element = render(window, cx);
                let layout_id = element.request_layout(window, cx);
                (layout_id, Some(element))
            }
        }
    })
}

#[inline(never)]
fn request_layout_component(
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_id(ElementId::from(name), |window| {
        let mut element = render(window, cx);
        let layout_id = element.request_layout(window, cx);
        (layout_id, Some(element))
    })
}

#[inline(never)]
fn prepaint_view(
    entity_id: EntityId,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> Option<AnyElement> {
    window.set_view_id(entity_id);
    window.with_rendered_view(entity_id, |window| {
        if let Some(mut element) = element.take() {
            element.prepaint(window, cx);
            return Some(element);
        }

        window.with_element_state::<ViewElementState, _>(
            global_id.unwrap(),
            |element_state, window| {
                let content_mask = window.content_mask();
                let text_style = window.text_style();
                let frame_number = window.frame_number;

                // A view inside another cached view that redraws is reused too,
                // unless it was notified itself.
                if let Some(mut element_state) = element_state
                    && element_state.cache_key.bounds == bounds
                    && element_state.cache_key.content_mask == content_mask
                    && element_state.cache_key.text_style == text_style
                    && element_state.prepaint_frame + 1 == frame_number
                    && element_state.paint_frame + 1 == frame_number
                    && !window.dirty_views.contains(&entity_id)
                    && !window.refreshing
                {
                    let prepaint_start = window.prepaint_index();
                    window.reuse_prepaint(element_state.prepaint_range.clone());
                    cx.entities
                        .extend_accessed(&element_state.accessed_entities);
                    let prepaint_end = window.prepaint_index();
                    element_state.prepaint_range = prepaint_start..prepaint_end;
                    element_state.prepaint_frame = frame_number;

                    return (None, element_state);
                }

                let prepaint_start = window.prepaint_index();
                let (element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                    let mut element = render(window, cx);
                    element.layout_as_root(Size::<AvailableSpace>::from(bounds.size), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    element
                });

                let prepaint_end = window.prepaint_index();

                (
                    Some(element),
                    ViewElementState {
                        accessed_entities,
                        prepaint_range: prepaint_start..prepaint_end,
                        paint_range: PaintIndex::default()..PaintIndex::default(),
                        prepaint_frame: frame_number,
                        // Set when the view is painted in this frame.
                        paint_frame: 0,
                        cache_key: ViewElementCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                        },
                    },
                )
            },
        )
    })
}

#[inline(never)]
fn prepaint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    window.with_id(ElementId::from(name), |window| {
        element.as_mut().unwrap().prepaint(window, cx);
    });
    Some(element.take().unwrap())
}

#[inline(never)]
fn paint_view(
    entity_id: EntityId,
    cached: bool,
    global_id: Option<&GlobalElementId>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        if cached && !caching_disabled {
            window.with_element_state::<ViewElementState, _>(
                global_id.unwrap(),
                |element_state, window| {
                    let mut element_state = element_state.unwrap();

                    let paint_start = window.paint_index();

                    if let Some(element) = element {
                        element.paint(window, cx);
                    } else {
                        window.reuse_paint(element_state.paint_range.clone());
                    }

                    let paint_end = window.paint_index();
                    element_state.paint_range = paint_start..paint_end;
                    element_state.paint_frame = window.frame_number;

                    ((), element_state)
                },
            )
        } else {
            element.as_mut().unwrap().paint(window, cx);
        }
    });
}

#[inline(never)]
fn paint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_id(ElementId::Name(name.into()), |window| {
        element.as_mut().unwrap().paint(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use crate::{
        AnyWindowHandle, App, Background, Bounds, ContentMask, Context, Entity, Hsla, Modifiers,
        Pixels, ScaledPixels, StyleRefinement, TestAppContext, VisualTestContext, Window,
        WindowHandle, blue, deferred, div, green, hsla, point, prelude::*, px, red, size,
    };
    use rand::{Rng as _, SeedableRng as _, rngs::StdRng};
    use std::{cell::Cell, rc::Rc};

    const GROUP_COUNT: usize = 4;
    const GROUP_HEIGHT: f32 = 40.;

    /// A cached group in a pane, which counts its renders and its clicks.
    struct Group {
        color: Hsla,
        renders: Rc<Cell<usize>>,
        clicks: Rc<Cell<usize>>,
    }

    impl Render for Group {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let clicks = self.clicks.clone();
            div()
                .id("group")
                .size_full()
                .bg(self.color)
                .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
        }
    }

    /// A cached pane of cached groups, one of which it can hide.
    struct Pane {
        groups: Vec<Entity<Group>>,
        hidden_group: Option<usize>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Pane {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div()
                .flex()
                .flex_col()
                .size_full()
                .children(self.groups.iter().enumerate().map(|(ix, group)| {
                    div()
                        .w_full()
                        .h(px(GROUP_HEIGHT))
                        .when(self.hidden_group == Some(ix), |this| this.invisible())
                        .child(group.clone().cached(StyleRefinement::default().size_full()))
                }))
        }
    }

    /// A cached view holding the pane, so that the groups are two cached views deep.
    struct Outer {
        pane: Entity<Pane>,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Outer {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div().size_full().child(
                self.pane
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
        }
    }

    /// The window's root. Each time it's notified it draws one more small
    /// element ahead of the outer view, so the outer view's drawing lands at
    /// other positions in the frame.
    struct Root {
        outer: Entity<Outer>,
        leading: usize,
    }

    impl Render for Root {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children((0..self.leading).map(|ix| {
                    div()
                        .id(ix)
                        .absolute()
                        .size(px(1.))
                        .bg(blue())
                        .on_click(|_, _, _| {})
                }))
                .child(
                    self.outer
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
        }
    }

    struct Fixture {
        window: WindowHandle<Root>,
        pane: Entity<Pane>,
        groups: Vec<Entity<Group>>,
        outer_renders: Rc<Cell<usize>>,
        pane_renders: Rc<Cell<usize>>,
        group_renders: Vec<Rc<Cell<usize>>>,
        group_clicks: Vec<Rc<Cell<usize>>>,
    }

    impl Fixture {
        fn open(cx: &mut TestAppContext) -> Self {
            let outer_renders = Rc::new(Cell::new(0));
            let pane_renders = Rc::new(Cell::new(0));
            let group_renders: Vec<_> = (0..GROUP_COUNT).map(|_| Rc::new(Cell::new(0))).collect();
            let group_clicks: Vec<_> = (0..GROUP_COUNT).map(|_| Rc::new(Cell::new(0))).collect();
            let window = cx.open_window(size(px(200.), px(200.)), |_, cx| {
                let groups = (0..GROUP_COUNT)
                    .map(|ix| {
                        cx.new(|_| Group {
                            color: red(),
                            renders: group_renders[ix].clone(),
                            clicks: group_clicks[ix].clone(),
                        })
                    })
                    .collect();
                let pane = cx.new(|_| Pane {
                    groups,
                    hidden_group: None,
                    renders: pane_renders.clone(),
                });
                let outer = cx.new(|_| Outer {
                    pane,
                    renders: outer_renders.clone(),
                });
                Root { outer, leading: 0 }
            });
            cx.run_until_parked();
            let (pane, groups) = window
                .update(cx, |root, _, cx| {
                    let pane = root.outer.read(cx).pane.clone();
                    let groups = pane.read(cx).groups.clone();
                    (pane, groups)
                })
                .unwrap();
            Self {
                window,
                pane,
                groups,
                outer_renders,
                pane_renders,
                group_renders,
                group_clicks,
            }
        }

        /// How many times the outer view, the pane and each group rendered.
        fn renders(&self) -> (usize, usize, Vec<usize>) {
            (
                self.outer_renders.get(),
                self.pane_renders.get(),
                self.group_renders
                    .iter()
                    .map(|renders| renders.get())
                    .collect(),
            )
        }

        /// The groups' backgrounds in the rendered frame, from top to bottom,
        /// with their tops in pixels.
        fn painted_groups(&self, cx: &mut TestAppContext) -> Vec<(f32, Background)> {
            self.window
                .update(cx, |_, window, _| {
                    let scale_factor = window.scale_factor();
                    let mut quads: Vec<_> = window
                        .rendered_frame
                        .scene
                        .quads
                        .iter()
                        .filter(|quad| quad.bounds.size.height.0 / scale_factor == GROUP_HEIGHT)
                        .map(|quad| (quad.bounds.origin.y.0 / scale_factor, quad.background))
                        .collect();
                    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
                    quads
                })
                .unwrap()
        }

        fn expect_painted(&self, cx: &mut TestAppContext, colors: [Hsla; GROUP_COUNT]) {
            let expected: Vec<_> = colors
                .iter()
                .enumerate()
                .map(|(ix, color)| (ix as f32 * GROUP_HEIGHT, Background::from(*color)))
                .collect();
            assert_eq!(self.painted_groups(cx), expected);
        }

        /// Clicks each group once and checks that each one's own handler ran.
        fn click_each_group(&self, cx: &mut TestAppContext) {
            let mut cx = VisualTestContext::from_window(AnyWindowHandle::from(self.window), cx);
            for (ix, clicks) in self.group_clicks.iter().enumerate() {
                let before = clicks.get();
                let y = (ix as f32 + 0.5) * GROUP_HEIGHT;
                cx.simulate_click(point(px(100.), px(y)), Modifiers::none());
                assert_eq!(clicks.get(), before + 1, "group {ix} handles its click");
            }
        }

        fn notify_root(&self, cx: &mut TestAppContext) {
            self.window
                .update(cx, |root, _, cx| {
                    root.leading += 1;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        fn recolor_group(&self, cx: &mut TestAppContext, ix: usize, color: Hsla) {
            self.groups[ix].update(cx, |group, cx| {
                group.color = color;
                cx.notify();
            });
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn a_clean_nested_view_is_reused_when_the_view_around_it_redraws(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);
        assert_eq!(fixture.renders(), (1, 1, vec![1; GROUP_COUNT]));

        fixture.pane.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (2, 2, vec![1; GROUP_COUNT]));
        fixture.expect_painted(cx, [red(); GROUP_COUNT]);
        fixture.click_each_group(cx);
    }

    #[gpui::test]
    fn a_notified_nested_view_redraws_alone(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        fixture.recolor_group(cx, 1, blue());
        assert_eq!(fixture.renders(), (2, 2, vec![1, 2, 1, 1]));
        fixture.expect_painted(cx, [red(), blue(), red(), red()]);
        fixture.click_each_group(cx);
    }

    #[gpui::test]
    fn nested_views_stay_reusable_while_the_views_around_them_are_reused(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        // The outer view is reused whole twice, so the groups aren't drawn,
        // and then each group in turn redraws while the others are reused.
        fixture.notify_root(cx);
        fixture.notify_root(cx);
        assert_eq!(fixture.renders(), (1, 1, vec![1; GROUP_COUNT]));
        let mut colors = [red(); GROUP_COUNT];
        for ix in 0..GROUP_COUNT {
            colors[ix] = green();
            fixture.recolor_group(cx, ix, green());
            fixture.notify_root(cx);
            fixture.expect_painted(cx, colors);
        }
        assert_eq!(fixture.renders(), (5, 5, vec![2; GROUP_COUNT]));
        fixture.click_each_group(cx);
    }

    #[gpui::test]
    fn a_refresh_or_a_resize_redraws_every_nested_view(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        fixture
            .window
            .update(cx, |_, window, _| window.refresh())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (2, 2, vec![2; GROUP_COUNT]));

        cx.simulate_window_resize(fixture.window.into(), size(px(300.), px(250.)));
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (3, 3, vec![3; GROUP_COUNT]));
        fixture.expect_painted(cx, [red(); GROUP_COUNT]);
    }

    #[gpui::test]
    fn a_nested_view_hidden_in_the_last_frame_redraws_when_shown(cx: &mut TestAppContext) {
        let fixture = Fixture::open(cx);

        // Hidden, the group is laid out but not painted.
        fixture.pane.update(cx, |pane, cx| {
            pane.hidden_group = Some(2);
            cx.notify();
        });
        cx.run_until_parked();
        let painted = fixture.painted_groups(cx);
        assert_eq!(painted.len(), GROUP_COUNT - 1);
        assert!(painted.iter().all(|(top, _)| *top != 2. * GROUP_HEIGHT));

        // What it painted two frames ago is gone, so it draws again.
        fixture.pane.update(cx, |pane, cx| {
            pane.hidden_group = None;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(fixture.renders(), (3, 3, vec![1, 1, 2, 1]));
        fixture.expect_painted(cx, [red(); GROUP_COUNT]);
        fixture.click_each_group(cx);
    }

    /// A view in a randomly built tree of cached views.
    struct Node {
        color: Hsla,
        popover: bool,
        children: Vec<Child>,
    }

    struct Child {
        node: Entity<Node>,
        height: f32,
        hidden: bool,
    }

    impl Render for Node {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("node")
                .flex()
                .flex_col()
                .size_full()
                .overflow_hidden()
                .bg(self.color)
                .on_click(|_, _, _| {})
                .when(self.popover, |this| {
                    this.child(deferred(
                        div()
                            .id("popover")
                            .size(px(6.))
                            .bg(self.color.opacity(0.5))
                            .on_click(|_, _, _| {}),
                    ))
                })
                .children(self.children.iter().map(|child| {
                    div()
                        .w_full()
                        .flex_none()
                        .h(px(child.height))
                        .when(child.hidden, |this| this.invisible())
                        .child(
                            child
                                .node
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                        )
                }))
        }
    }

    struct Tree {
        node: Entity<Node>,
    }

    impl Render for Tree {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                self.node
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
        }
    }

    /// A node and its children, to draw the same tree from scratch.
    struct NodeSnapshot {
        color: Hsla,
        popover: bool,
        children: Vec<(NodeSnapshot, f32, bool)>,
    }

    impl NodeSnapshot {
        fn of(node: &Entity<Node>, cx: &App) -> Self {
            let node = node.read(cx);
            Self {
                color: node.color,
                popover: node.popover,
                children: node
                    .children
                    .iter()
                    .map(|child| (Self::of(&child.node, cx), child.height, child.hidden))
                    .collect(),
            }
        }

        fn build(&self, cx: &mut App) -> Entity<Node> {
            let children = self
                .children
                .iter()
                .map(|(node, height, hidden)| Child {
                    node: node.build(cx),
                    height: *height,
                    hidden: *hidden,
                })
                .collect();
            cx.new(|_| Node {
                color: self.color,
                popover: self.popover,
                children,
            })
        }
    }

    #[derive(Debug, PartialEq)]
    struct DrawnFrame {
        quads: Vec<(
            u32,
            Bounds<ScaledPixels>,
            ContentMask<ScaledPixels>,
            Background,
        )>,
        hitboxes: Vec<Bounds<Pixels>>,
    }

    fn drawn_frame(window: AnyWindowHandle, cx: &mut TestAppContext) -> DrawnFrame {
        cx.update_window(window, |_, window, _| {
            let frame = &window.rendered_frame;
            DrawnFrame {
                quads: frame
                    .scene
                    .quads
                    .iter()
                    .map(|quad| (quad.order, quad.bounds, quad.content_mask, quad.background))
                    .collect(),
                hitboxes: frame.hitboxes.iter().map(|hitbox| hitbox.bounds).collect(),
            }
        })
        .unwrap()
    }

    fn random_node(
        rng: &mut StdRng,
        depth: usize,
        nodes: &mut Vec<Entity<Node>>,
        cx: &mut App,
    ) -> Entity<Node> {
        let child_count = if depth == 0 {
            0
        } else {
            rng.random_range(0..=3)
        };
        let children = (0..child_count)
            .map(|_| Child {
                node: random_node(rng, depth - 1, nodes, cx),
                height: rng.random_range(10..=120) as f32,
                hidden: false,
            })
            .collect();
        let color = hsla(rng.random(), 0.5, 0.5, 1.);
        let popover = rng.random_bool(0.2);
        let node = cx.new(|_| Node {
            color,
            popover,
            children,
        });
        nodes.push(node.clone());
        node
    }

    /// Changes random parts of a tree of cached views, frame after frame, and
    /// compares each frame with the same tree drawn from scratch.
    #[gpui::test]
    fn nested_views_draw_what_a_fresh_window_draws(cx: &mut TestAppContext) {
        const WINDOW_SIZE: f32 = 400.;
        for seed in 0..24 {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut nodes = Vec::new();
            let root = cx.update(|cx| random_node(&mut rng, 3, &mut nodes, cx));
            let window = cx.open_window(size(px(WINDOW_SIZE), px(WINDOW_SIZE)), |_, _| Tree {
                node: root.clone(),
            });
            cx.run_until_parked();

            for step in 0..40 {
                let changes = rng.random_range(1..=2);
                for _ in 0..changes {
                    let node = nodes[rng.random_range(0..nodes.len())].clone();
                    match rng.random_range(0..7) {
                        0 => node.update(cx, |node, cx| {
                            node.color = hsla(rng.random(), 0.5, 0.5, 1.);
                            cx.notify();
                        }),
                        1 => node.update(cx, |_, cx| cx.notify()),
                        2 => node.update(cx, |node, cx| {
                            node.popover = !node.popover;
                            cx.notify();
                        }),
                        3 => node.update(cx, |node, cx| {
                            if !node.children.is_empty() {
                                let ix = rng.random_range(0..node.children.len());
                                node.children[ix].height = rng.random_range(10..=120) as f32;
                                cx.notify();
                            }
                        }),
                        4 => node.update(cx, |node, cx| {
                            if !node.children.is_empty() {
                                let ix = rng.random_range(0..node.children.len());
                                node.children[ix].hidden = !node.children[ix].hidden;
                                cx.notify();
                            }
                        }),
                        5 => window.update(cx, |_, _, cx| cx.notify()).unwrap(),
                        _ => window.update(cx, |_, window, _| window.refresh()).unwrap(),
                    }
                }
                cx.run_until_parked();
                let drawn = drawn_frame(window.into(), cx);

                let snapshot = cx.read(|cx| NodeSnapshot::of(&root, cx));
                let fresh = cx.open_window(size(px(WINDOW_SIZE), px(WINDOW_SIZE)), |_, cx| Tree {
                    node: snapshot.build(cx),
                });
                cx.run_until_parked();
                let expected = drawn_frame(fresh.into(), cx);
                fresh
                    .update(cx, |_, window, _| window.remove_window())
                    .unwrap();
                assert_eq!(drawn, expected, "seed {seed}, step {step}");
            }
            window
                .update(cx, |_, window, _| window.remove_window())
                .unwrap();
            cx.run_until_parked();
        }
    }
}
