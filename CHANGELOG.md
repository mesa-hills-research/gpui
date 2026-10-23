# Changelog

Each release lists the crates it publishes. Crates that aren't listed stay at their earlier
version, and a requirement such as `version = "0.1"` picks up the newest 0.1.x of each.
[docs/upstream.md](docs/upstream.md#changes-to-the-gpui-crates) describes each change in more
detail.

## Unreleased

### Keyboard

- `Window::observe_modifiers_changed` calls back after every change of the modifier keys or of Caps
  Lock in a window, whatever has focus. Element handlers such as `on_modifiers_changed` hear a
  change only along the focused element's path, which is the root alone while nothing has focus, so
  an app that reacts to a held modifier, such as underlining menu access keys while Alt is down, had
  to poll `Window::modifiers`. The callback runs after the elements have seen the change, even when
  one of them stops it, and dropping the returned `Subscription` ends it.

### Text

- DirectWrite shapes a long line in segments on Windows, as the cosmic-text system does on Linux and
  Android. A paragraph that grows or changes by a word shapes the segment it touches and takes the
  rest from the cache, instead of shaping the whole paragraph again. Lines under 1 KB, lines with
  right-to-left text, tabs or control characters, and lines on macOS and the web are shaped whole.

### Test support

- `gpui_windows::text_system` and `gpui_macos::MacTextSystem` give a test the platform's
  text system, with the crate's `test-support` feature (and `font-kit` on macOS).

## 0.1.4

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.4. The other crates stay at
0.1.0.

### Key bindings

- A binding with Shift on a symbol key, such as `ctrl-shift-\`, matches the symbol the platforms
  send. Windows, Linux and macOS send Ctrl+Shift+\ on a US keyboard as `ctrl-|`, and the web sends
  `ctrl-shift-|`. A binding written with Shift on the key matched neither, so it could not fire,
  except on Windows in a keymap loaded with the platform's keyboard mapper.
  `Keystroke::should_match` now reads Shift on one of the 21 symbol keys of a US keyboard as the
  symbol it types, so `ctrl-shift-\`, `ctrl-|` and `ctrl-shift-|` are one chord whichever is bound
  or typed. A binding shows as written, a keystroke that matched before still matches, and letters
  and named keys keep their Shift. A chord bound in both spellings is two bindings for one
  keystroke, ordered like any two. Where a layout's Shift types another symbol, such as `"` on
  Shift+2 of a UK keyboard, bind the symbol the layout sends. The documentation of
  `KeyBinding::new` says what to write.

## 0.1.3

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.3. The other crates stay at
0.1.0.

### Lists

- A list given height estimates keeps the height an item had when the item changes, until its
  next frame estimates the item again. An item invalidated by `remeasure_items` or replaced by a
  one-for-one `splice` counted the mean height of the other items in the meantime. A reply that
  streamed in below the reader made the scroll range too short for that moment, and the reader's
  place fell beyond its end. Reading the scrollbar then moved the thumb to the end of its track and
  left it there, and a turn of the wheel through `set_offset_from_scrollbar` could move the list by
  hundreds of pixels. The scrollbar still gives a position while items wait for an estimate, but
  doesn't keep it as the place the thumb moves on from.
- The scrollbar's thumb stays on its track. A thumb that moved down with more speed than the list
  had distance left to the end went past 1, and the offset the scrollbar showed went past the end of
  its range. A wheel turn through `set_offset_from_scrollbar` then moved the list by the turn and
  by the excess as well.
- A turn of the wheel scrolls the list from where it is. The list's wheel handler scrolled from
  where the list was when it last painted, so a turn that came after another scroll in the same
  frame, by `scroll_by`, by `scroll_to` or through the scrollbar's offset, undid that scroll.

## 0.1.2

Published as `mhr-gpui`, `mhr-gpui-wgpu`, `mhr-gpui-linux`, `mhr-gpui-windows`,
`mhr-gpui-macos`, `mhr-gpui-apple`, `mhr-gpui-web` and `mhr-gpui-platform`. They pin each other
exactly, so an app that uses several of them gets them all at 0.1.2. The other crates stay at
0.1.0.

### Views

- A list that draws its items twice in a frame, as it does when an item asks to scroll, reports
  them to the accessibility tree once. Its second drawing pushed the nodes of the first again,
  which panicked in debug builds and left the first drawing's places in the tree in release builds.
- A cached view that is reused keeps its nodes in the accessibility tree. Every frame that
  reused one used to leave out its nodes, its focus and its action handlers, so a screen reader
  found holes where cached views were. The nodes are now copied from the previous frame along
  with the view's drawing. A cached view draws again when accessibility starts, to have nodes to
  copy afterwards. While accessibility is on, the window keeps one more frame of nodes and copies
  the tree once more in each frame. With accessibility off, a frame does no extra work.

### Animation

- `AnimationExt::with_paint_animation` animates how an element paints, its opacity and the
  rotation of its icons, without drawing the view it is in again. An animated element notified its
  view in every frame, so a spinner in a pane drew the whole pane, and every view above it, 60
  times a second. A paint animation makes a `PaintEffect` from the animation's value, and the
  window draws its root view in each frame and replays what the cached views drew with the
  effects applied. On a pane of 30, 120 and 480 rows, a frame in which a spinner turns took 0.26,
  0.32 and 0.65 ms, against 1.3, 4.2 and 17 ms when the spinner redraws the pane. Frames stop when
  the animations end or when `App::reduce_motion` is set, and `Animation::with_max_fps` draws
  them on the grid that throttled animations share. A view that holds a paint animation no longer
  draws in each frame, so code that counts that view's draws to measure the animation's frame rate
  counts fewer.

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
  drawn for the first time takes about 10% longer. The glyphs are those of the whole line.
  Positions can differ from the whole line's by float noise, up to about 0.16px at a cut in a line
  of monospace text, because each segment continues at the width the text system reports for it.
  Wrapping follows the positions, so a word can land on the other side of a line end than it did in
  0.1.1 when that line end sits within a fraction of a pixel of the wrap width. In a test of random
  paragraphs of 1 to 10 KB at six widths, 1.3% of the wraps put one word on the other line. Lines
  under 1 KB, lines with right-to-left text, tabs or control characters, and lines on other
  platforms are shaped whole. A text system opts in with
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
