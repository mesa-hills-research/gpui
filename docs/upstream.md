# Following upstream

## Where the gpui crates come from

Zed publishes its `gpui` crate to crates.io only now and then. GPUI Kit publishes snapshots of it
instead, as the `gpui-pre-*` crates. Its "Release GPUI" workflow runs
[`script/bump-gpui.ts`](https://github.com/longbridge/gpui-kit/blob/main/script/bump-gpui.ts)
every Monday and by hand. For 0.3.8 the script:

- took Zed's `gpui`, `gpui_platform`, `gpui_macros` and `reqwest_client` and the 21 workspace
  crates they need
- renamed each to `gpui-pre-*`, kept the Zed name as the `[lib]` name, gave all 25 the version
  0.3.8 and pinned them to each other with `=0.3.8`
- swapped `zed-reqwest` for `gpui-pre-reqwest` and proptest's git source for crates.io, and
  dropped `http_client`'s `github-download` feature with its git-only `async-tar`
- relaxed exact pins on outside crates to carets (`taffy`, `unicode-properties` and nine
  workspace entries)
- rewrote three things so gpui also works behind a facade crate such as `gpui-kit`:
  `gpui_macros`' 19 proc-macro entry points, the `actions!` macro, and `gpui_apple`'s build
  script, which gets the five gpui sources it reads copied under `vendor/gpui`. Each rewritten
  file starts with a "Modified for gpui-pre" line.
- left out every `[dev-dependencies]` table (see [testing.md](testing.md))
- put `LICENSE-APACHE` in every crate and Zed's root `LICENSE-*` files at the workspace root

## Bringing in a release

The worked example is 0.3.8, which needed Zed `279fe070`, gpui-kit `42fbb97` and Bun 1.4.2.

1. **Find the revisions.** Each gpui-pre crate's description on crates.io ends with the Zed
   revision, for example "(gpui-pre snapshot of zed@279fe07)", and `[package.metadata.gpui-pre]`
   in its `Cargo.toml` has it in full. The script version is the head commit of the "Release
   GPUI" run that published the release, and that run's log shows the Bun version and the
   staging output:

   ```sh
   gh api 'repos/longbridge/gpui-kit/actions/workflows/release-gpui.yml/runs?per_page=10' \
     --jq '.workflow_runs[] | [.id, .head_sha, .created_at, .conclusion] | @tsv'
   gh api repos/longbridge/gpui-kit/actions/runs/<run>/jobs --jq '.jobs[].id'
   gh api repos/longbridge/gpui-kit/actions/jobs/<job>/logs > run.log
   ```

2. **Stage the crates** with that script and that Zed revision:

   ```sh
   git init -q zed && git -C zed fetch -q --depth 1 https://github.com/zed-industries/zed <zed-rev>
   git -C zed checkout -q --detach FETCH_HEAD
   mkdir -p bump/script
   git -C <gpui-kit clone> show <kit-commit>:script/bump-gpui.ts > bump/script/bump-gpui.ts
   (cd bump && bun script/bump-gpui.ts <version> --zed ../zed --stage-only)
   ```

   The output should match the run's log, apart from the "published files changed since" line,
   which an explicit version skips. The staged workspace is
   `bump/target/gpui-pre/workspace`.

3. **Commit it on `upstream`**, replacing the whole tree:

   ```sh
   git switch upstream
   git rm -rq .
   (cd bump/target/gpui-pre/workspace && tar --exclude=./target -cf - .) | tar -xf -
   git add -A --force -- . ':!target'
   git commit -m "gpui-pre <version>, as released (Zed <short rev>)"
   ```

   `--force` keeps files that a crate's own `.gitignore` would hide, and `':!target'` keeps a
   local build out. The commit message records the revisions and the command, as the 0.3.8
   commit does.

4. **Compare it with crates.io** from a checkout of `main`, against a worktree of `upstream`:

   ```sh
   git worktree add --detach ../gpui-upstream upstream
   script/verify-upstream.sh -C ../gpui-upstream
   ```

   The script packages each gpui-pre crate and compares it with the published one. Every crate
   should match.

5. **Merge into `main`** with `git merge upstream`. Conflicts mark lines the fork changed too,
   listed in [Changes to the gpui crates](#changes-to-the-gpui-crates) and
   [The fork's names](#the-forks-names). In each `Cargo.toml`, keep `main`'s package name,
   version and crates.io metadata, and take upstream's dependencies and features. If
   `Cargo.lock` conflicts, take upstream's and let cargo rename the crates and add
   `mhr-gpui-mobile`'s dependencies back with `cargo metadata --format-version 1 > /dev/null`.
   Then:
   - fix `mhr-gpui-mobile` where gpui's API moved. Its gpui dependencies follow the workspace
     version on their own, and breaks show up in the checks in [mobile.md](mobile.md).
   - run the GPUI Kit fork's tests against the new `main`
   - list the files the fork has changed with `git diff --stat upstream main -- crates tooling`

## The fork's names

The crates are published as `mhr-` followed by their library names, at versions of their own
starting from 0.1.0, because the `gpui-pre` names belong to GPUI Kit on crates.io. Each crate's
`Cargo.toml` has its own name, version, description, repository, keywords, categories and README,
and the workspace sets `rust-version` and pins the crates to each other with `=`.
`[package.metadata.gpui-pre]` keeps the Zed crate and revision each one comes from.

## Publishing

The published crates share one version, set in each crate's `Cargo.toml` and in the `=` pins of
`[workspace.dependencies]`, so a release bumps all of them together. Then:

```sh
packages=$(cargo metadata --no-deps --format-version 1 \
  | jq -j '.packages[] | select(.publish != []) | "-p \(.name) "')
cargo publish --dry-run --locked $packages
cargo publish --locked $packages
```

Cargo packages the crates, builds each package against the others, and uploads them in
dependency order. crates.io takes new versions of existing crates quickly, but it limits how fast
one account creates new crates, so a release that adds a crate may have to wait and retry.

## Changes to the gpui crates

Each of upstream's files that these changes touch starts with a line saying Mesa Hills Research
modified it, as the Apache License asks of modified files. A change to another of upstream's files
adds the same line.

- **Variable font weights**, in `mhr-gpui-wgpu`'s `src/cosmic_text_system.rs`, the text system
  of Linux and Android. It chose a face for the requested weight and then drew every weight of a
  variable face at the face's default, so Bold in a variable font such as Ubuntu Sans Mono came out
  Regular. Each weight of a variable face now gets its own `FontId`, shaped and rasterized at that
  point of the `wght` axis. Italic comes from the family's italic face, and other axes (`ital`,
  `slnt`, `wdth`) stay at their defaults, since cosmic-text shapes along `wght` alone.
  `crates/gpui_screenshot/tests/variable_fonts.rs` covers it. Zed's gpui has the same bug, which
  hasn't been reported upstream yet.
- **Screenshot test support** in `mhr-gpui` and `mhr-gpui-wgpu`, behind their `test-support`
  feature. [screenshots.md](screenshots.md#changes-to-the-gpui-crates) has the details.
- **Recent files in Windows jump lists**, in `mhr-gpui`'s `src/platform.rs` and `src/app.rs` and
  `mhr-gpui-windows`' `src/destination_list.rs` and `src/platform.rs`. Zed lists folders, so
  `App::update_jump_list` titles the recent entries "Recent Folders" with a folder icon beside
  each, and it still does. `App::update_jump_list_with` takes a `JumpListRecent` that names the
  title, the icon (a folder, the app's own, or one from a file) and whether to add Windows' own
  Recent category below it. `Platform::update_jump_list` takes the `JumpListRecent`.
  `App::add_recent_document` now works on Windows too, through `SHAddToRecentDocs` with the
  process's AppUserModelID when it has one. A jump list whose recent entries Windows declines,
  because the user turned off recent items, keeps its tasks. `mhr-gpui-apple`'s copy of
  `platform.rs` under `vendor/gpui` follows the original.
- **The golden ratio**, in `mhr-gpui`'s `src/geometry.rs`. `phi()` returns
  `f32::consts::GOLDEN_RATIO` in place of the literal `1.618_034`, which is the same `f32` and
  which the clippy of Rust 1.99 rejects. `mhr-gpui-apple`'s copy under `vendor/gpui` follows it.
- **Zed's forks from crates.io**, in the workspace's `Cargo.toml` and those of `mhr-gpui` and
  five sibling crates. Zed's manifests name its forks of font-kit, xim, scap and wasm_thread by
  git revision beside a crates.io version, and a published crate gets the crates.io release. This
  workspace names only the crates.io releases, so its builds and tests use what the published
  crates get. font-kit, xim and scap have the same sources as the revisions. In wasm_thread 0.3.3
  the web workers start wasm-bindgen with positional arguments, which wasm-bindgen still accepts
  with a deprecation warning in the console. A merge from `upstream` brings the git revisions back
  where upstream changed those lines.
- **Throttled animations share frames**, in `mhr-gpui`'s `src/elements/animation.rs` and
  `src/window.rs`. `Animation::with_max_fps` scheduled each animation's next render on a timer of
  its own, so animations started at different times drew in different frames, and two spinners
  capped at 30 fps drew 60 frames a second. Renders now fall on a grid of `1 / max_fps` from the
  app's animation epoch, and the throttled animations of a window that wait for the same point of
  the grid share one timer. `mhr-gpui-apple`'s copy of `window.rs` under `vendor/gpui` follows it.
- **List scrollbars that hold steady** over items not drawn yet, in `mhr-gpui`'s
  `src/elements/list.rs`. `ListState` counted an item it hadn't measured as 0px tall, so the
  scrollbar's range grew as the list drew items. While the reader scrolled, the thumb shrank and
  jumped back, a dragged thumb slid away from the pointer, and a thumb dragged to the top stopped
  short of the first item. An item not measured yet now counts its size hint, or else the mean
  height of the items whose height is known, and an item replaced one for one, or invalidated by a
  width change, keeps its old height as its estimate. The thumb moves with the list in proportion
  to the distance left to the end it moves toward, so measured items change its speed instead of
  its place, and both ends of the track are the ends of the list. A dragged thumb keeps its size
  and stays under the pointer. Wheel and track scrolling through `set_offset_from_scrollbar` moves
  the list by the distance asked. The tests at the end of `list.rs` cover it.
- **Cached views inside cached views stay cached**, in `mhr-gpui`'s `src/view.rs`,
  `src/window.rs` and `src/text_system/line_layout.rs`. A cached view that redrew made every
  cached view inside it redraw too, so caching a view inside another gave nothing. Such a view is
  now reused when the view around it redraws, unless it was notified or its bounds, content mask
  or text style changed. The cached views inside a view reused whole move with it in the frame,
  and a view is reused only from what it drew in the previous frame, so a cached view under an
  element that was hidden for a frame draws again when it shows. The tests at the end of
  `view.rs` cover it. `mhr-gpui-apple`'s copy of `window.rs` under `vendor/gpui` follows it.
- **Text measured for its natural width**, in `mhr-gpui`'s `src/elements/text.rs`. A text
  element asked for its size without a width answered with the size it last wrapped to, so a
  box as wide as its text stayed as narrow as some earlier layout made it. It now lays the text
  out without wrapping, and keeps the layout it replaced, so text measured at its natural width
  and then laid out at a narrower one, frame after frame, is laid out once at each width. The
  tests at the end of `text.rs` cover it.

## The mobile crate

`crates/gpui_mobile` follows [longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile)
`main`, last taken at `9075e3a`. Newer upstream commits apply with their history:

```sh
git -C <gpui-mobile clone> format-patch --stdout 9075e3a..origin/main -- . ':!.github' \
  | git am -3 --directory=crates/gpui_mobile
```

Skip upstream's "Bump gpui-pre" commits, since the workspace sets that version.

## Formatting

Format only the files you change, with `cargo fmt -p mhr-gpui-mobile` or `rustfmt <file>`.
`cargo fmt --all` would reformat code that the release tooling generates, such as
`crates/gpui_macros/src/gpui_pre_facade_paths.rs`, and every later import would conflict with it.
CI checks the formatting of every crate except `mhr-gpui-macros`.
