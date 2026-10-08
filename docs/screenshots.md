# Screenshot tests

`gpui-pre-screenshot` (`crates/gpui_screenshot`, library `gpui_screenshot`) renders GPUI views
headlessly on Linux and compares them with golden PNGs committed beside the tests. It catches
what state assertions miss: an icon that stopped drawing, text the same color as its background,
a border one pixel off.

## What it covers

The tests run GPUI's real renderer: the wgpu renderer core and its shaders, the sprite atlas,
and text laid out by cosmic-text and rasterized by swash. Android draws through the same renderer
core and text system, so the goldens stand for Android's rendering and text as well as Linux's.

They stop at the renderer. iOS draws with Metal and lays out text with CoreText, and Android's
platform code (touch, IME, surfaces, the app lifecycle) needs a device or an emulator. The images
come from Mesa's software Vulkan driver, so they show what GPUI asks for, which a phone's GPU
can round differently at anti-aliased edges.

## Writing a test

`ScreenshotApp` is a headless app with the bundled fonts and a software renderer. Open a window
at a logical size and a scale factor, capture it, and compare the capture with its golden:

```rust
use gpui::{AppContext as _, px, size};
use gpui_screenshot::{ScreenshotApp, goldens};

#[test]
fn badge() {
    let mut app = ScreenshotApp::new().unwrap();
    let shot = app
        .render_view(size(px(120.), px(40.)), 2., |_, cx| cx.new(|_| Badge))
        .unwrap();
    goldens!().assert("badge", &shot);
}
```

`goldens!()` is the `tests/screenshots` folder of the crate it's used in, and `badge` is
`tests/screenshots/badge.png`. Names can have subfolders (`menu/dark`) and suffixes such as
`badge@3x`.

- `render_element` takes a closure that returns any element.
- `open_window` keeps the window open, so a test can change state, dispatch events and capture
  again. `ScreenshotApp` dereferences to GPUI's `HeadlessAppContext` for that.
- `open_window_with` opens the window through a library's own function, such as
  `gpui_kit::open_window`, which wraps the view in GPUI Kit's `Root`.
- `with_assets` gives the app an asset source, for icons and images.
- `add_fonts` adds fonts beyond the bundled ones, for CJK or emoji text.

GPUI Kit's tests in mhr_gpui_kit, `crates/kit/tests/screenshots.rs`, show the whole pattern:
GPUI Kit initialized, light and dark themes, typing into an input with
`gpui_kit::test::TestWindowExt`, and keyboard navigation in a list and a menu.

Windows open active, so focused inputs draw their caret. Each window covers a whole number of
device pixels, as a device's surface does. A size that doesn't come out even at the scale factor
is nudged: 108 logical pixels at 2.625 become 284 device pixels.

## Reviewing and updating goldens

`cargo test` compares every screenshot with its golden. When one differs, or has no golden yet,
the test fails and writes two images to `failures/`, a folder beside the goldens that git
ignores:

- `<name>.actual.png`, the screenshot
- `<name>.diff.png`, the golden faded to gray, with changed pixels in red (amber for changes
  within the tolerance)

```text
screenshot `buttons` differs from its golden …/crates/kit/tests/screenshots/buttons.png
  11017 of 190080 pixels changed, the largest channel difference is 245
  golden drawn by: llvmpipe (LLVM 20.1.2, 256 bits) on Vulkan, Mesa 25.2.8-0ubuntu0.24.04.4 (LLVM 20.1.2)
  this run:        llvmpipe (LLVM 20.1.2, 256 bits) on Vulkan, Mesa 25.2.8-0ubuntu0.24.04.4 (LLVM 20.1.2)
  actual: …/crates/kit/tests/screenshots/failures/buttons.actual.png
  diff:   …/crates/kit/tests/screenshots/failures/buttons.diff.png (red: over the tolerance, amber: within it)
```

When the change is intended, rerun with `UPDATE_GOLDENS=1`. Goldens that are missing or differ
are rewritten, and goldens that still match are left alone, so the commit shows only what
changed. Look at the new images before committing them.

Each golden records the adapter and driver that drew it in a PNG text chunk, which the failure
message prints next to the current run's.

## On a remote build machine

When the tests run on a machine whose files don't come back, such as a CI runner or a remote
build machine, `PRINT_SCREENSHOTS=1` also prints every image the harness writes, goldens and
failures alike, as one line on standard output:

```text
GPUI-SCREENSHOT <sha256> crates/kit/tests/screenshots/buttons.png <base64 PNG>
```

The path is relative to the workspace root, the nearest folder with a `Cargo.lock`.
`script/screenshots-from-log` writes the files back into a local checkout and checks each
against its hash. Cargo's `--config 'env.NAME="value"'` sets the variables for the tests, which
works wherever cargo's arguments get through:

```sh
# On the remote machine, in an mhr_gpui_kit checkout, with standard output saved as run.log
cargo test -p gpui-kit --features test-support --test screenshots \
  --config 'env.UPDATE_GOLDENS="1"' --config 'env.PRINT_SCREENSHOTS="1"'
# Locally, in this checkout
script/screenshots-from-log -C ../mhr_gpui_kit run.log
```

## Why every run gives the same pixels

| Source of variation | What the harness does |
|---|---|
| GPU and driver | `WgpuHeadlessRenderer::new_software` takes a CPU adapter, Mesa's lavapipe, even on a machine with a GPU |
| Fonts | The text system starts empty and loads only the bundled fonts: Roboto Regular, Medium and Bold, and DejaVu Sans Mono. `.SystemUIFont` is Roboto, as on Android |
| Scale | Every window has an explicit scale factor and covers whole device pixels |
| Time | GPUI's test scheduler runs the app, and its clock moves only when a test advances it, so timers such as the caret's blink never fire on their own |
| Animations | `App::set_reduce_motion(true)` shows each animation in its final state |
| Task order | The scheduler's seed is fixed. Setting `SEED` changes it |
| Layout feedback | Two frames are drawn before each capture, so a list scrolled to its selection has settled |
| Text rendering | Text is anti-aliased in grayscale. `ZED_FONTS_GAMMA` and the two `ZED_FONTS_*_ENHANCED_CONTRAST` variables change it, and a failure message names them when they're set |

The images then depend on the Mesa and LLVM versions and on the CPU features llvmpipe compiles
for (the "256 bits" in the adapter name is AVX2).

## Tolerance

Matches are exact by default, which holds while the goldens and the test run share a Mesa
version and CPU class. A test that has to pass on other machines, such as a CI image with an
older Mesa or a CPU without AVX2, may see anti-aliased edges and glyphs move by a few levels.
`Tolerance` allows that:

```rust
use gpui_screenshot::Tolerance;

goldens!()
    .tolerance(Tolerance::channel(3).pixels(10))
    .assert("buttons", &shot);
```

`channel(3)` lets every channel of every pixel differ by 3 out of 255, and `pixels(10)` lets 10
pixels differ by more. Keep tolerances small: a one-pixel shift changes channels by far more than
3, and a misplaced element changes far more than 10 pixels. Regenerating the goldens on the
machine that checks them is usually the better fix.

## Requirements

Linux with Mesa's lavapipe and the Vulkan loader (`mesa-vulkan-drivers` and `libvulkan1` on
Debian and Ubuntu). Neither a display nor a GPU is needed. Without a software adapter,
`ScreenshotApp::new` returns an error that says what to install.

## The fonts

`crates/gpui_screenshot/fonts` holds Roboto 3.016 from the Android build of
[googlefonts/roboto-3-classic](https://github.com/googlefonts/roboto-3-classic) (SIL Open Font
License 1.1) and DejaVu Sans Mono 2.37 (Bitstream Vera license, DejaVu changes in the public
domain), each with its license. A weight between the bundled ones resolves to the nearest
bundled weight, so semibold (600) text is drawn in Bold. Text in a family that isn't loaded
panics, as GPUI does with a missing font.

## Changes to the gpui crates

The harness needs two additions, both behind the `test-support` feature:

- `HeadlessAppContext::simulate_window_scale_factor_change` in `gpui-pre`, as
  `TestAppContext` already has, since headless windows open at scale 2
- `WgpuHeadlessRenderer::new_software` and `WgpuHeadlessRenderer::adapter_info` in
  `gpui-pre-wgpu`. The software renderer shares one device per process, like `new`, and keeps it
  apart from `new`'s.
