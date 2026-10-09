# Using the fork

Every crate here keeps the crates.io package name, version and library name of the crate it
replaces, so a `[patch.crates-io]` in the app's root `Cargo.toml` switches an app to the fork.
Cargo applies only the root workspace's patches, so an app that uses the
[GPUI Kit fork](https://github.com/mesa-hills-research/gpui_kit) patches the gpui crates itself too.

## The patch

Patch every `gpui-pre-*` crate the app's `Cargo.lock` lists, so none of them is left on
crates.io beside its replacement. `gpui-pre-reqwest` is the exception: GPUI Kit publishes it on
its own, and it stays on crates.io.

```toml
[patch.crates-io]
gpui-pre                   = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-apple             = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-bench-metrics     = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-collections       = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-derive-refineable = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-http-client       = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-http-client-tls   = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-linux             = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-macos             = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-macros            = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-mobile            = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-perf              = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-platform          = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-refineable        = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-reqwest-client    = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-scheduler         = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-shared-string     = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-sum-tree          = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-util              = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-util-macros       = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-web               = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-wgpu              = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-windows           = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-zlog              = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-ztracing          = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
gpui-pre-ztracing-macro    = { git = "https://github.com/mesa-hills-research/gpui", rev = "<commit>" }
```

Cargo warns about entries the app's graph doesn't use, such as `gpui-pre-mobile` in a desktop app.
Delete those.

Then `cargo update -p gpui-pre` (or any cargo command without `--locked`) moves the lock file onto
the patch. `cargo tree -i gpui-pre` should show one `gpui-pre` and it should come from
`github.com/mesa-hills-research/gpui`.

## Working against a local checkout

For changes in progress, point the entries at a checkout instead, for example
`gpui-pre = { path = "../gpui/crates/gpui" }`. The crates' directories:

| Package | Directory |
|---|---|
| `gpui-pre` | `crates/gpui` |
| `gpui-pre-apple` | `crates/gpui_apple` |
| `gpui-pre-bench-metrics` | `crates/bench_metrics` |
| `gpui-pre-collections` | `crates/collections` |
| `gpui-pre-derive-refineable` | `crates/refineable/derive_refineable` |
| `gpui-pre-http-client` | `crates/http_client` |
| `gpui-pre-http-client-tls` | `crates/http_client_tls` |
| `gpui-pre-linux` | `crates/gpui_linux` |
| `gpui-pre-macos` | `crates/gpui_macos` |
| `gpui-pre-macros` | `crates/gpui_macros` |
| `gpui-pre-mobile` | `crates/gpui_mobile` |
| `gpui-pre-perf` | `tooling/perf` |
| `gpui-pre-platform` | `crates/gpui_platform` |
| `gpui-pre-refineable` | `crates/refineable` |
| `gpui-pre-reqwest-client` | `crates/reqwest_client` |
| `gpui-pre-scheduler` | `crates/scheduler` |
| `gpui-pre-shared-string` | `crates/gpui_shared_string` |
| `gpui-pre-sum-tree` | `crates/sum_tree` |
| `gpui-pre-util` | `crates/gpui_util` |
| `gpui-pre-util-macros` | `crates/util_macros` |
| `gpui-pre-web` | `crates/gpui_web` |
| `gpui-pre-wgpu` | `crates/gpui_wgpu` |
| `gpui-pre-windows` | `crates/gpui_windows` |
| `gpui-pre-zlog` | `crates/zlog` |
| `gpui-pre-ztracing` | `crates/ztracing` |
| `gpui-pre-ztracing-macro` | `crates/ztracing_macro` |

Cargo caps lints for git and registry dependencies, and treats path dependencies as the app's own
code. With `RUSTFLAGS="-D warnings"`, which `actions-rust-lang/setup-rust-toolchain` sets by
default and GPUI Kit's CI uses, gpui's own warnings then fail the build. With rustc 1.99 there are three: gpui's profiler calls
`Atomic*::fetch_update`, which 1.99 deprecates in favour of `try_update`. Use git entries in CI.

## What changes in the lock file

Besides the gpui crates themselves, four dependencies change source. Zed's manifests give them a
git revision next to a crates.io version, and cargo builds a git or path dependency's
dependencies from the revision, where the published gpui-pre crates use the crates.io release.

| Dependency | From the revision | crates.io release | Difference |
|---|---|---|---|
| `zed-font-kit` 0.14.1-zed | `94b0f28` | from `1105231` | `dirs` 6 instead of 5 |
| `zed-xim` 0.4.0-zed | `16f35a2` | from `471c10c` | Same tree. `xim-parser` 0.2.1 from the repository instead of 0.2.2. |
| `zed-scap` 0.0.8-zed | `4afea48` | from `4afea48` | None |
| `wasm_thread` 0.3.3 | `0cf96c7` | 0.3.3 | Used only on wasm |

Inside this workspace, `gpui-pre-mobile` names the same `zed-font-kit` revision as
`gpui-pre-wgpu`, so iOS builds get one font-kit.
