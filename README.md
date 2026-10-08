# mhr_gpui

mhr_gpui is Mesa Hills Research's fork of [GPUI](https://www.gpui.rs), the GPU-accelerated UI
framework of the [Zed](https://github.com/zed-industries/zed) editor, together with the
`gpui-pre-mobile` platform for iOS and Android. GPUI fixes and the mobile work live here, next to
each other, and [mhr_gpui_kit](https://github.com/mesa-hills-research/mhr_gpui_kit), the fork of
GPUI Kit, builds on it.

It tracks the `gpui-pre` crates that GPUI Kit publishes: snapshots of Zed's gpui, cut by GPUI
Kit's release tooling. Today that is **gpui-pre 0.3.8**, from Zed
[`279fe070`](https://github.com/zed-industries/zed/commit/279fe070bb389b79652e52065b2f001edcc0b11b)
(2026-10-04), the snapshot GPUI Kit 0.7.1 is built against.

| Crates | Source | State |
|---|---|---|
| `gpui-pre` and 24 sibling crates (`crates/`, `tooling/perf`) | gpui-pre 0.3.8 | Unchanged, apart from two test-support additions to `gpui-pre` and `gpui-pre-wgpu` for screenshot tests |
| `gpui-pre-mobile` (`crates/gpui_mobile`) | [longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile) `9075e3a`, moved from gpui-pre 0.3.7 to 0.3.8 | Experimental, see [Mobile](#mobile) |
| `gpui-pre-screenshot` (`crates/gpui_screenshot`) | This fork | Golden-image tests for GPUI views, see [Screenshot tests](#screenshot-tests) |

## Using it

The crates keep their crates.io names, versions and library names, so an app (or GPUI Kit) moves
to the fork with a `[patch.crates-io]` and no code changes:

```toml
[patch.crates-io]
gpui-pre          = { git = "https://github.com/mesa-hills-research/mhr_gpui", rev = "<commit>" }
gpui-pre-platform = { git = "https://github.com/mesa-hills-research/mhr_gpui", rev = "<commit>" }
gpui-pre-mobile   = { git = "https://github.com/mesa-hills-research/mhr_gpui", rev = "<commit>" }
# ...one entry for every gpui-pre crate in the app's Cargo.lock
```

[docs/using.md](docs/using.md) has the full list and what changes in the lock file.

## Following upstream

Each gpui-pre release lands on the `upstream` branch as one commit holding the crates as GPUI
Kit's tooling stages them for crates.io. `main` merges `upstream` and carries the fork's own
changes on top. [docs/upstream.md](docs/upstream.md) has the steps.

## Mobile

`gpui-pre-mobile` implements GPUI's platform for iOS (Metal through wgpu, CoreText) and Android
(Vulkan or GLES through wgpu, cosmic-text). Upstream calls it experimental: IME composition,
accessibility and the app lifecycle are incomplete. Its gpui dependencies come from this
workspace, so it always builds against the gpui beside it.

| Target | State |
|---|---|
| Android (`aarch64-linux-android`, `x86_64-linux-android`) | Type-checks, and the example app's `.so` builds and links with NDK r29 |
| iOS (`aarch64-apple-ios`, `-sim`) | Type-checks on Linux. Linking and running need a Mac. |

[docs/mobile.md](docs/mobile.md) covers building, the gaps and the open upstream work.
[docs/testing.md](docs/testing.md) covers tests, including headless screenshots on Linux.

## Screenshot tests

`gpui-pre-screenshot` renders GPUI views headlessly on Linux, on Mesa's software Vulkan driver
with bundled fonts, and compares them with golden PNGs. Android draws through the same renderer
and text system, so the goldens cover its rendering and text too. GPUI Kit's buttons, inputs,
list and menu have goldens in mhr_gpui_kit.

```rust
let mut app = gpui_screenshot::ScreenshotApp::new()?;
let shot = app.render_view(size(px(120.), px(40.)), 2.625, |_, cx| cx.new(|_| Badge))?;
gpui_screenshot::goldens!().assert("badge", &shot); // tests/screenshots/badge.png
```

`UPDATE_GOLDENS=1 cargo test` rewrites goldens after an intended change.
[docs/screenshots.md](docs/screenshots.md) covers writing the tests, reviewing failures, running
them on a remote machine and why the pixels repeat exactly.

## License

The gpui crates are Apache-2.0, like Zed's gpui. Each carries `LICENSE-APACHE`, and the files the
release tooling rewrites say so in their first line. `gpui-pre-mobile` keeps upstream's choice of
GPL-3.0-or-later, AGPL-3.0-or-later or Apache-2.0. `LICENSE-GPL` at the root comes over from Zed's
root with the release tooling and applies to none of the gpui crates.
