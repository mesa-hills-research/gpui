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

1. **Find the revisions.** Each crate's description ends with the Zed revision, for example
   "(gpui-pre snapshot of zed@279fe07)", and `[package.metadata.gpui-pre]` in its `Cargo.toml`
   has it in full. The script version is the head commit of the "Release GPUI" run that
   published the release, and that run's log shows the Bun version and the staging output:

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
   git worktree add --detach ../mhr_gpui-upstream upstream
   script/verify-upstream.sh -C ../mhr_gpui-upstream
   ```

   The script packages each gpui-pre crate and compares it with the published one. Every crate
   should match.

5. **Merge into `main`** with `git merge upstream`. Conflicts mark lines the fork changed too.
   If `Cargo.lock` conflicts, take upstream's and let cargo add `gpui-pre-mobile`'s dependencies
   back with `cargo metadata --format-version 1 > /dev/null`. Then:
   - fix `gpui-pre-mobile` where gpui's API moved: its gpui dependencies follow the workspace
     version on their own, and breaks show up in the checks in [mobile.md](mobile.md)
   - run mhr_gpui_kit's tests against the new `main`
   - run `script/verify-upstream.sh` on `main` to list the crates the fork has changed. Crates
     that only use a changed crate show as differing "in Cargo.lock only".

## The mobile crate

`crates/gpui_mobile` follows [longbridge/gpui-mobile](https://github.com/longbridge/gpui-mobile)
`main`, last taken at `9075e3a`. Newer upstream commits apply with their history:

```sh
git -C <gpui-mobile clone> format-patch --stdout 9075e3a..origin/main -- . ':!.github' \
  | git am -3 --directory=crates/gpui_mobile
```

Skip upstream's "Bump gpui-pre" commits, since the workspace sets that version.

## Formatting

Format only the files you change, with `cargo fmt -p gpui-pre-mobile` or `rustfmt <file>`.
`cargo fmt --all` would reformat code that the release tooling generates, such as
`crates/gpui_macros/src/gpui_pre_facade_paths.rs`, and every later import would conflict with it.
