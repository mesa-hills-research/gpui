# Changelog

Each release lists the crates it publishes. Crates that aren't listed stay at their earlier
version, and a requirement such as `version = "0.1"` picks up the newest 0.1.x of each.
[docs/upstream.md](docs/upstream.md#changes-to-the-gpui-crates) describes each change in more
detail.

## Unreleased

### Views

- A cached view that is reused keeps its nodes in the accessibility tree. Every frame that
  reused one used to leave out its nodes, its focus and its action handlers, so a screen reader
  found holes where cached views were. The nodes are now copied from the previous frame along
  with the view's drawing. A cached view draws again when accessibility starts, to have nodes to
  copy afterwards.

### Lists

- `Entity::cached_measured` and `AnyView::cached_measured` make a list item a cached view whose
  height is measured from its contents. A cached view took its size from its style alone, so a row
  of a list could not be one. Such a row renders when it is first shown, when it is notified, when
  the width it is laid out at or the text style around it changes, and when the window refreshes.
  Otherwise it reuses what it drew and the height it measured, so a row that animates redraws
  alone and the list's other rows stay as they were. On a list of 1,043 mixed rows, a frame in
  which one row animates took 0.15 ms with 14 rows in view, against 0.29 ms when every row is an
  element of the list's view, and 0.27 ms against 0.76 ms with 20 rows in view. The row has to be
  the item itself, since only the root of a layout knows its width. Anywhere else it renders in
  every frame, as an uncached view does.

### Text

- A long line of text is shaped in segments of a few hundred bytes on Linux and Android, and each
  segment is kept for later frames. A long paragraph that grows or changes by a word shapes the
  segment it touches instead of the whole paragraph. On a paragraph of 20 KB, a frame that adds
  a word takes 1.2 ms instead of 9.6 ms, and one that edits a word in the middle takes 1.0 ms
  instead of 9.0 ms. At 100 KB they take 2.7 and 2.9 ms instead of 51 and 52 ms. A paragraph
  drawn for the first time takes about 10% longer. Glyphs, line breaks and wrapping are those
  of the whole line, and positions differ by no more than the rounding of the sums of the glyphs'
  widths. Lines under 1 KB, lines with right-to-left text, tabs or control characters, and lines
  on other platforms are shaped whole. A text system opts in with
  `PlatformTextSystem::shapes_words_independently`.

## 0.1.1

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.1. The other crates stay at
0.1.0.

### Lists

- A list's scrollbar thumb holds steady over rows that aren't drawn yet. Scrolling up a long
  list of rows of mixed heights used to shrink the thumb and make it jump back, a dragged thumb
  slid away from the pointer, and a thumb dragged to either end stopped short of it. A row that
  hasn't been measured now counts its size hint, or else the mean height of the rows that have.
  The thumb reaches both ends of the track together with the list, and it changes speed
  gradually when a row turns out taller or shorter than counted.
- `List::estimate_item_heights` takes a closure that estimates a row's height at a width, for
  example from its lines of text. The scrollbar counts the estimates for the rows that haven't
  been drawn, so the thumb keeps its size and moves evenly as the reader scrolls onto new rows.
  The list asks for estimates of new rows, of every row after `ListState::remeasure`, of every
  row when its width changes, and of rows that change through `ListState::remeasure_items` or a
  one-for-one `splice`. On a transcript of 1043 rows of 20 to 900px, the largest step of the
  thumb while scrolling up from the end went from 6.4 times the median step to 1.0 times.

### Views

- A cached view inside another cached view is reused when the view around it redraws. It redraws
  only when it was notified, a view inside it was notified, or its bounds, content mask or text
  style changed. Before, caching a view inside another one saved nothing. On a settings pane of
  8 cached groups of 6 rows of text, a frame in which one group changes now renders 1 group
  instead of 8 and takes 1.0 ms instead of 2.8 ms.
- A cached view under an element that was hidden for a frame no longer replays old paint when
  the element shows again. It draws again instead.

### Text

- Text measured without a width is laid out at its natural width. A box sized by its text, such
  as a chat message at the side of a column, used to stay as narrow as an earlier wrapped layout
  had made it.
- `TextSystem::has_font_family` says whether one font family is available, without listing every
  family with `all_font_names`. On a Linux machine with 231 families it takes 0.15 µs for a
  family that's there, against 38 µs for listing the families and searching them. A custom
  `PlatformTextSystem` gets a default that searches `all_font_names`.

### In the repository

`mhr-gpui-scroll-harness` measures and films how smoothly a list's scrollbar moves over a
transcript of mixed rows. It isn't published. The velocity tracker test in `mhr-gpui-mobile` gives
its samples their own times instead of sleeping between them, which made it fail on busy machines.

## 0.1.0

The first release of the fork, built on gpui-pre 0.3.8 (Zed `279fe070`). It adds a fix for
variable font weights on Linux and Android, test support for screenshot tests, recent files in
Windows jump lists, and throttled animations that share frames. It takes Zed's forks of font-kit,
xim, scap and wasm_thread from crates.io.
