use std::{
    borrow::Cow,
    cell::RefCell,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use anyhow::{Context as _, Result};
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, AssetSource, Bounds, Context, Entity,
    HeadlessAppContext, IntoElement, Pixels, PlatformHeadlessRenderer, PlatformTextSystem as _,
    Render, Size, Window, WindowBounds, WindowHandle, WindowOptions, point, px,
};
use gpui_wgpu::{CosmicTextSystem, WgpuHeadlessRenderer, wgpu};
use image::RgbaImage;

use crate::{SANS_FAMILY, bundled_fonts};

/// Frames drawn before each capture, so that state a frame's layout feeds back, such as a list
/// scrolling to its selection, has settled.
const SETTLE_FRAMES: usize = 2;

/// An image a [`ScreenshotApp`] rendered, and the adapter that drew it.
#[derive(Clone, Debug)]
pub struct Screenshot {
    pub image: RgbaImage,
    /// The adapter, backend and driver that drew the image, for example
    /// "llvmpipe (LLVM 20.1.2, 256 bits) on Vulkan, Mesa 25.2.8 (LLVM 20.1.2)".
    pub source: String,
}

/// A headless GPUI app whose windows render to the same pixels on every run.
///
/// It draws with a software (CPU) adapter such as Mesa's lavapipe even on a machine with a GPU,
/// lays out text in the [bundled fonts](bundled_fonts) alone, shows animations in their final
/// state (`App::set_reduce_motion`) and runs GPUI's test scheduler, whose clock only moves when a
/// test advances it. Windows open active, at the size and scale factor a test asks for.
///
/// It dereferences to GPUI's [`HeadlessAppContext`], for updating entities and windows and
/// dispatching events between captures.
pub struct ScreenshotApp {
    cx: HeadlessAppContext,
    source: String,
}

impl ScreenshotApp {
    /// An app with the bundled fonts and no assets.
    pub fn new() -> Result<Self> {
        Self::with_assets(())
    }

    /// An app with the bundled fonts that loads images, icons and other assets from `assets`.
    pub fn with_assets(assets: impl AssetSource) -> Result<Self> {
        let renderer = WgpuHeadlessRenderer::new_software()
            .context("screenshot tests render with a software (CPU) adapter")?;
        let source = describe(&renderer.adapter_info());
        let text_system = Arc::new(CosmicTextSystem::new_without_system_fonts(SANS_FAMILY));
        text_system.add_fonts(bundled_fonts())?;
        // The first window takes the renderer made above and each later one makes its own, all
        // on the process's one software device.
        let first = RefCell::new(Some(renderer));
        let cx = HeadlessAppContext::with_platform(text_system, Arc::new(assets), move || {
            let renderer = match first.borrow_mut().take() {
                Some(renderer) => renderer,
                None => WgpuHeadlessRenderer::new_software()?,
            };
            Ok(Some(Box::new(renderer) as Box<dyn PlatformHeadlessRenderer>))
        });
        let mut app = Self { cx, source };
        app.cx.update(|cx| cx.set_reduce_motion(true));
        Ok(app)
    }

    /// The adapter, backend and driver this app draws with.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Adds fonts beyond the bundled ones, for example a CJK or emoji font a test needs.
    pub fn add_fonts(&mut self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.cx.update(|cx| cx.text_system().add_fonts(fonts))
    }

    /// Opens a window of `size` logical pixels at `scale_factor`, with `build`'s view as its root.
    ///
    /// Like a device's surface, the window covers a whole number of device pixels: its size is
    /// nudged to the nearest one that does. 108 logical pixels at scale 2.625 come to 283.5
    /// device pixels, so the window gets 284 of them, 108.19 logical pixels.
    pub fn open_window<V: Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        scale_factor: f32,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> Result<WindowHandle<V>> {
        let size = whole_device_pixels(size, scale_factor)?;
        let window = self.cx.open_window(size, build)?;
        self.prepare(window.into(), scale_factor)?;
        Ok(window)
    }

    /// Opens a window through `open`, which gets the window options to pass on, for views a
    /// library wraps in its own root, such as `gpui_kit::open_window`. Sizes work as in
    /// [`Self::open_window`].
    pub fn open_window_with<W: Into<AnyWindowHandle>>(
        &mut self,
        size: Size<Pixels>,
        scale_factor: f32,
        open: impl FnOnce(WindowOptions, &mut App) -> Result<W>,
    ) -> Result<AnyWindowHandle> {
        let size = whole_device_pixels(size, scale_factor)?;
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(0.), px(0.)),
                size,
            })),
            focus: false,
            show: false,
            ..Default::default()
        };
        let window = self.cx.update(|cx| open(options, cx))?.into();
        self.prepare(window, scale_factor)?;
        Ok(window)
    }

    fn prepare(&mut self, window: AnyWindowHandle, scale_factor: f32) -> Result<()> {
        let current = self
            .cx
            .update_window(window, |_, window, _| window.scale_factor())?;
        if current != scale_factor {
            self.cx
                .simulate_window_scale_factor_change(window, scale_factor)?;
        }
        self.cx
            .update_window(window, |_, window, _| window.activate_window())?;
        self.cx.run_until_parked();
        Ok(())
    }

    /// Draws a fresh frame of `window` and reads it back. The image is the window's size times
    /// its scale factor, in device pixels.
    pub fn capture(&mut self, window: impl Into<AnyWindowHandle>) -> Result<Screenshot> {
        let window = window.into();
        for _ in 0..SETTLE_FRAMES {
            self.cx.run_until_parked();
            self.cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })?;
        }
        self.cx.run_until_parked();
        let image = self.cx.capture_screenshot(window)?;
        Ok(Screenshot {
            image,
            source: self.source.clone(),
        })
    }

    /// Opens a window with `build`'s view, captures it and closes it.
    pub fn render_view<V: Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        scale_factor: f32,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> Result<Screenshot> {
        let window = self.open_window(size, scale_factor, build)?;
        let screenshot = self.capture(window);
        self.cx
            .update_window(window.into(), |_, window, _| window.remove_window())?;
        self.cx.run_until_parked();
        screenshot
    }

    /// Renders the element `render` returns as the whole content of a window.
    pub fn render_element<E: IntoElement>(
        &mut self,
        size: Size<Pixels>,
        scale_factor: f32,
        render: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Result<Screenshot> {
        self.render_view(size, scale_factor, |_, cx| {
            cx.new(|_| {
                ElementView(Box::new(move |window, cx| {
                    render(window, cx).into_any_element()
                }))
            })
        })
    }
}

impl Deref for ScreenshotApp {
    type Target = HeadlessAppContext;

    fn deref(&self) -> &HeadlessAppContext {
        &self.cx
    }
}

impl DerefMut for ScreenshotApp {
    fn deref_mut(&mut self) -> &mut HeadlessAppContext {
        &mut self.cx
    }
}

struct ElementView(Box<dyn Fn(&mut Window, &mut App) -> AnyElement>);

impl Render for ElementView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        (self.0)(window, cx)
    }
}

/// `size`, nudged so that at `scale_factor` it covers a whole number of device pixels, as GPUI
/// rounds it when it sizes the render target. Otherwise the last row and column would be only
/// partly covered and blend with the target's black clear color.
fn whole_device_pixels(size: Size<Pixels>, scale_factor: f32) -> Result<Size<Pixels>> {
    anyhow::ensure!(
        scale_factor.is_finite() && scale_factor > 0.,
        "invalid scale factor {scale_factor}"
    );
    let snap = |length: Pixels| px((f32::from(length) * scale_factor).round() / scale_factor);
    Ok(Size {
        width: snap(size.width),
        height: snap(size.height),
    })
}

fn describe(info: &wgpu::AdapterInfo) -> String {
    let mut text = format!("{} on {:?}", info.name, info.backend);
    if !info.driver_info.is_empty() {
        text.push_str(", ");
        text.push_str(&info.driver_info);
    }
    text
}
