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
| `gpui-pre` and 24 sibling crates (`crates/`, `tooling/perf`) | gpui-pre 0.3.8 | Unchanged: repackaging them gives the crates.io files byte for byte |
| `gpui-pre-mobile` (`crates/gpui_mobile`) | [longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile) `9075e3a`, moved from gpui-pre 0.3.7 to 0.3.8 | Experimental, see [Mobile](#mobile) |

`script/verify-upstream.sh` repeats the byte-for-byte check against crates.io.

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

Each gpui-pre release lands on the `upstream` branch as one commit holding the crates exactly as
GPUI Kit's tooling stages them for crates.io, made by running that tooling at the revisions the
release used and checked against the published crates. `main` merges `upstream` and carries the
fork's own changes on top. [docs/upstream.md](docs/upstream.md) has the steps.

## Mobile

`gpui-pre-mobile` implements GPUI's platform for iOS (Metal through wgpu, CoreText) and Android
(Vulkan or GLES through wgpu, cosmic-text). Upstream calls it experimental: IME composition,
accessibility and the app lifecycle are incomplete. Its gpui dependencies come from this
workspace, so it always builds against the gpui beside it.

| Target | Checked so far |
|---|---|
| Android (`aarch64-linux-android`, `x86_64-linux-android`) | Type-checks, and the example app's `.so` builds and links with NDK r29 |
| iOS (`aarch64-apple-ios`, `-sim`) | Type-checks on Linux. Linking and running need a Mac. |
| Linux host | 46 unit tests pass |

[docs/mobile.md](docs/mobile.md) covers building, the gaps and the open upstream work.
[docs/testing.md](docs/testing.md) covers tests, including headless screenshots on Linux.

## License

The gpui crates are Apache-2.0, like Zed's gpui. Each carries `LICENSE-APACHE`, and the files the
release tooling rewrites say so in their first line. `gpui-pre-mobile` keeps upstream's choice of
GPL-3.0-or-later, AGPL-3.0-or-later or Apache-2.0. `LICENSE-GPL` at the root comes over from Zed's
root with the release tooling and applies to none of the gpui crates.
