//! Golden-image screenshot tests for GPUI views.
//!
//! [`ScreenshotApp`] renders windows headlessly on a software Vulkan driver (Mesa's lavapipe)
//! with a bundled set of fonts, so a view renders to the same pixels on every run and every
//! machine with the same Mesa. [`Goldens`] compares the images with PNG files committed next to
//! the tests and, on a mismatch, writes the actual image and a diff beside them.
//!
//! ```ignore
//! use gpui::{px, size};
//!
//! let mut app = gpui_screenshot::ScreenshotApp::new()?;
//! let shot = app.render_view(size(px(240.), px(80.)), 2., |_, cx| cx.new(|_| MyView))?;
//! gpui_screenshot::goldens!().assert("my_view", &shot);
//! ```
//!
//! `UPDATE_GOLDENS=1` writes the actual images as the new goldens, and `PRINT_SCREENSHOTS=1`
//! also prints every image the harness writes as a base64 line, for machines whose files don't
//! come back on their own. `docs/screenshots.md` in this repository has the details.

mod app;
mod compare;
mod fonts;
mod golden;
mod output;

pub use app::*;
pub use compare::*;
pub use fonts::*;
pub use golden::*;
pub use output::{PRINT_MARKER, PRINT_VAR, UPDATE_VAR};
