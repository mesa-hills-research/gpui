# Testing

## What runs here

- `cargo check --workspace` covers every library.
- `cargo test -p gpui-pre-mobile` runs the mobile crate's unit tests.
- `cargo test -p gpui-pre-screenshot` runs the screenshot harness's tests and its goldens of
  plain GPUI elements, on Linux.
- mhr_gpui_kit's suite, about 2,650 tests, exercises the gpui crates through GPUI Kit. Run it from
  an mhr_gpui_kit checkout whose `[patch.crates-io]` points at this one.

The gpui crates' own tests, examples and benchmarks don't build here. The release tooling leaves
out their `[dev-dependencies]`, and several read fonts from Zed's `assets/` folder, which the
snapshot doesn't include. Running them would mean restoring both, which is future work. Until
then, a change to gpui itself can be tested in a Zed checkout at the snapshot's revision.

## Headless screenshots on Linux

GPUI's test platform can render windows for real and read the pixels back.
`HeadlessAppContext::with_platform` takes a renderer factory, and
`gpui_platform::current_headless_renderer()` returns a `WgpuHeadlessRenderer` on Linux (Metal on
macOS). It needs no GPU: Mesa's lavapipe, the CPU Vulkan driver in `mesa-vulkan-drivers`, is
enough.

With `gpui-pre`, `gpui-pre-platform` and `gpui-pre-wgpu` built with their `test-support` feature:

```rust
let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("DejaVu Sans"));
let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(()), || {
    gpui_platform::current_headless_renderer()
});
let window = cx.open_window(size(px(240.), px(100.)), |_, cx| cx.new(|_| MyView))?;
cx.run_until_parked();
let image = cx.capture_screenshot(window.into())?; // an image::RgbaImage
image.save("screenshot.png")?;
```

On a machine without a GPU, wgpu draws through lavapipe on Vulkan. The test platform renders at
scale 2, so a 240×100 window comes back as a 480×200 image, and repeated runs give the same
pixels. EGL may log "DRI2: failed to load driver" while wgpu probes its GL backend, which leaves
the Vulkan renderer unaffected.

This reaches further than desktop: Android draws through the same wgpu renderer core and the
same cosmic-text system, so screenshot tests on Linux cover Android's rendering and text. They
leave out its platform code: touch, IME, surfaces and the lifecycle.

`gpui-pre-screenshot` builds golden-image tests on this, with bundled fonts and a software
adapter so every run gives the same pixels. [screenshots.md](screenshots.md) covers writing the
tests and updating their goldens.
