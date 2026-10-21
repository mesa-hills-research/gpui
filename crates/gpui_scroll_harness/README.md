# gpui-pre-scroll-harness

Measures how smoothly a `gpui::list` scrollbar moves while the wheel scrolls through rows of very
different heights, most of them not measured yet, and films it.

```sh
cargo run -p gpui-pre-scroll-harness -- --out target/scroll-harness --film tail
```

It draws headlessly with `gpui-pre-screenshot`, on Mesa's software Vulkan driver, so text wraps with
real fonts (see [docs/screenshots.md](../../docs/screenshots.md)).

## What it measures

The window holds a seeded transcript of a conversation with a coding agent: notices, collapsed tool
calls, wrapped prose, bullet lists, code blocks, open tool output and image thumbnails. The list is
800 by 600 pixels and draws 400 pixels ahead, with a scrollbar placed as GPUI Kit places its thumb.
Each step of the wheel scrolls 60 pixels, through GPUI Kit's `ScrollableMask` or the list's own
handler, until the list reaches its other end. Every step's thumb is compared with where it would
be if every row's height were known.

| Scenario | What scrolls |
|---|---|
| `tail` | A long transcript opened at its end, up to the start and back down |
| `head` | A long transcript opened at its start, down to the end and back up |
| `medium` | As `tail`, on a medium transcript, where each step moves the thumb further |
| `tail-list` | As `medium`, with the list's own wheel handling |
| `resize` | As `medium` on the way up, then narrower, down over heights measured at the old width |
| `short` | As `tail`, on a transcript short enough for the thumb's length to show |

`report.txt` gives, for each way through each scenario, the thumb's travel per step (its mean,
spread, largest spike and largest change from one step to the next), its distance from the exact
place, changes of its length, steps that moved the content more or less than the wheel asked, the
cost of the frame after each step and the rows it measured, and the memory a list state holds.
`trace-<scenario>.tsv` has every step and `chart-<scenario>.png` plots them. `--film` adds a
contact sheet (`film-<scenario>.png`) and a GIF of the frames around the largest step.

`--frames N` gives each step N frames, the one after the step and idle ones, for changes that
animate the thumb or measure rows ahead. An idle frame draws only if something asked for a frame,
as `Window::request_animation_frame` does, unless `--refresh-idle` redraws every one. `--settle N`
gives the window N idle frames after it opens, before the first step. Frame costs depend on the
build, so compare them between runs of the same profile.

## Trying a change to the list

A change to `ListState` that needs something from the list's user, such as height estimates, gets
it in `src/adopt.rs`, which sees each row's kind, text and line count, and the rows' real heights
for bounds and for hints with a set error. `adopt::element` gets the `List` element in each frame,
for an API on the element.
