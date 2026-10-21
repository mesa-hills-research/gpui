//! A long transcript of a conversation with a coding agent, as rows of very
//! different heights, from one-line notices and collapsed tool calls to wrapped
//! prose, code blocks, open tool output and image thumbnails. The kinds,
//! sizes and spacing follow a chat transcript such as Arko Studio's, and a
//! seed makes every run lay out the same rows.

use gpui::{
    AnyElement, FontWeight, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    px, rems, rgb,
};
use gpui_screenshot::{MONO_FAMILY, SANS_FAMILY};

/// The widest the reading column gets, side padding included.
pub const COLUMN: f32 = 736.;
/// The side padding inside the reading column.
pub const SIDE_PADDING: f32 = 16.;
/// The most lines a code block or a tool's output shows.
pub const SHOWN_LINES: usize = 40;

/// What a row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Where the conversation was resumed, a rule with a label.
    Separator,
    /// The user's message in a bubble.
    User,
    /// A message that only kept the prompt cache warm, one quiet line.
    Refresh,
    /// The agent's thinking, collapsed to one line.
    Thinking,
    /// A tool call, collapsed to one line.
    Call,
    /// A tool call opened to show its output.
    CallOpen,
    /// Several calls folded into one line.
    Group,
    /// A notice from the app, one or two lines.
    Notice,
    /// A paragraph of the reply.
    Paragraph,
    /// A heading in the reply.
    Heading,
    /// A bulleted list in the reply.
    Bullets,
    /// A code block in the reply, which scrolls sideways instead of wrapping.
    Code,
    /// An image thumbnail in the reply.
    Image,
    /// The actions under a reply.
    Actions,
    /// The turn's footer, with its time and cost.
    Turn,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Separator => "separator",
            Kind::User => "user",
            Kind::Refresh => "refresh",
            Kind::Thinking => "thinking",
            Kind::Call => "call",
            Kind::CallOpen => "call-open",
            Kind::Group => "group",
            Kind::Notice => "notice",
            Kind::Paragraph => "paragraph",
            Kind::Heading => "heading",
            Kind::Bullets => "bullets",
            Kind::Code => "code",
            Kind::Image => "image",
            Kind::Actions => "actions",
            Kind::Turn => "turn",
        }
    }
}

/// One row of the transcript.
#[derive(Clone, Debug)]
pub struct Row {
    pub kind: Kind,
    /// The row's text, which is the prose, the one-line label, or the lines of code
    /// or output, separated by newlines.
    pub text: SharedString,
    /// The items of a bulleted list.
    pub items: Vec<SharedString>,
    /// How many lines of code or output the row has, before the cap.
    pub lines: usize,
    /// The space above the row, in rems.
    pub gap: f32,
    /// The characters of the row's text, or of each of its items, counted
    /// once, as an app keeps them.
    char_count: usize,
    item_chars: Vec<usize>,
}

#[allow(dead_code, reason = "facts for candidates that need them")]
impl Row {
    fn new(kind: Kind, text: impl Into<SharedString>, gap: f32) -> Self {
        Self {
            kind,
            text: text.into(),
            items: Vec::new(),
            lines: 0,
            gap,
            char_count: 0,
            item_chars: Vec::new(),
        }
    }

    /// Counts the row's characters, once its text and items are set.
    fn counted(mut self) -> Self {
        self.item_chars = self.items.iter().map(|item| item.chars().count()).collect();
        self.char_count = if self.items.is_empty() {
            self.text.chars().count()
        } else {
            self.item_chars.iter().sum()
        };
        self
    }

    /// The characters of prose the row wraps, in its text or its items.
    pub fn chars(&self) -> usize {
        self.char_count
    }

    /// The characters of each item of a bulleted list.
    pub fn item_chars(&self) -> &[usize] {
        &self.item_chars
    }

    /// The lines of code or output the row shows.
    pub fn shown_lines(&self) -> usize {
        self.lines.min(SHOWN_LINES)
    }
}

/// A small, fast generator of pseudo-random numbers (SplitMix64), so the
/// same seed makes the same transcript everywhere.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from `low` up to and including `high`.
    pub fn range(&mut self, low: usize, high: usize) -> usize {
        low + (self.next() % (high - low + 1) as u64) as usize
    }

    /// A number from 0 to 1.
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }
}

const WORDS: &[&str] = &[
    "the",
    "list",
    "measures",
    "a",
    "row",
    "when",
    "it",
    "draws",
    "and",
    "estimates",
    "rest",
    "scrollbar",
    "thumb",
    "moves",
    "with",
    "content",
    "height",
    "of",
    "every",
    "item",
    "in",
    "window",
    "width",
    "changes",
    "so",
    "text",
    "wraps",
    "into",
    "more",
    "lines",
    "than",
    "before",
    "agent",
    "reads",
    "file",
    "then",
    "runs",
    "tests",
    "which",
    "pass",
    "on",
    "first",
    "try",
    "function",
    "returns",
    "value",
    "because",
    "cache",
    "keeps",
    "offset",
    "pixels",
    "steady",
    "wheel",
    "scrolls",
    "transcript",
    "reply",
    "streams",
    "while",
    "reader",
    "waits",
    "for",
    "build",
    "finish",
    "configuration",
    "dependency",
    "implementation",
    "straightforward",
    "notably",
    "renderer",
    "layout",
    "viewport",
    "overdraw",
    "unmeasured",
    "proportional",
];

/// Sentences of about `chars` characters.
fn prose(rng: &mut Rng, chars: usize) -> String {
    let mut text = String::new();
    while text.len() < chars {
        if !text.is_empty() {
            text.push(' ');
        }
        let words = rng.range(5, 18);
        for word in 0..words {
            let mut next = WORDS[rng.range(0, WORDS.len() - 1)].to_string();
            if word == 0 {
                next[..1].make_ascii_uppercase();
            } else {
                text.push(' ');
            }
            text.push_str(&next);
        }
        text.push('.');
    }
    text
}

fn code(rng: &mut Rng, lines: usize) -> String {
    (0..lines)
        .map(|line| {
            let indent = "    ".repeat(rng.range(0, 3));
            let name = WORDS[rng.range(0, WORDS.len() - 1)];
            match rng.range(0, 3) {
                0 => format!(
                    "{indent}let {name}_{line} = compute(&items[{line}], {});",
                    rng.range(0, 99)
                ),
                1 => format!("{indent}if {name}.is_empty() {{ return None; }}"),
                2 => {
                    let chars = rng.range(10, 70);
                    format!("{indent}// {}", prose(rng, chars))
                }
                _ => format!("{indent}}}"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn output(rng: &mut Rng, lines: usize) -> String {
    (0..lines)
        .map(|line| {
            let name = WORDS[rng.range(0, WORDS.len() - 1)];
            format!("test {name}::case_{line} ... ok ({} ms)", rng.range(0, 400))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const TOOLS: &[&str] = &[
    "read_file",
    "shell",
    "index_search",
    "edit_file",
    "list_files",
];

/// The rows of `turns` turns of a conversation made from `seed`.
pub fn transcript(turns: usize, seed: u64) -> Vec<Row> {
    let mut rng = Rng::new(seed);
    let mut rows = Vec::new();
    for turn in 0..turns {
        if turn > 0 && turn % 40 == 0 {
            rows.push(Row::new(Kind::Separator, "Resumed the conversation", 1.));
        }
        let ask = rng.range(20, 480);
        rows.push(Row::new(Kind::User, prose(&mut rng, ask), 1.75));
        if rng.chance(0.05) {
            rows.push(Row::new(Kind::Refresh, "Kept the cache warm", 0.5));
        }
        if rng.chance(0.5) {
            rows.push(Row::new(Kind::Thinking, "Thought for 4 seconds", 1.));
        }
        let calls = [0, 0, 1, 1, 2, 2, 3, 4, 6][rng.range(0, 8)];
        if calls >= 4 && rng.chance(0.5) {
            rows.push(Row::new(
                Kind::Group,
                format!("Read {calls} files and searched the code"),
                0.5,
            ));
        } else {
            for _ in 0..calls {
                let tool = TOOLS[rng.range(0, TOOLS.len() - 1)];
                let label = format!("{tool} src/{}.rs", WORDS[rng.range(0, WORDS.len() - 1)]);
                if rng.chance(0.15) {
                    let lines = rng.range(3, 120);
                    let mut row = Row::new(
                        Kind::CallOpen,
                        format!("{label}\n{}", output(&mut rng, lines.min(SHOWN_LINES))),
                        0.25,
                    );
                    row.lines = lines;
                    rows.push(row);
                } else {
                    rows.push(Row::new(Kind::Call, label, 0.25));
                }
            }
        }
        if rng.chance(0.08) {
            let chars = rng.range(30, 200);
            rows.push(Row::new(Kind::Notice, prose(&mut rng, chars), 0.5));
        }
        if rng.chance(0.12) {
            rows.push(Row::new(Kind::Paragraph, "Done.", 1.));
        } else {
            let blocks = rng.range(1, 7);
            for block in 0..blocks {
                let gap = if block == 0 { 1. } else { 0.5 };
                let pick = rng.unit();
                if pick < 0.5 {
                    let chars = rng.range(40, 900);
                    rows.push(Row::new(Kind::Paragraph, prose(&mut rng, chars), gap));
                } else if pick < 0.58 {
                    let chars = rng.range(10, 50);
                    rows.push(Row::new(Kind::Heading, prose(&mut rng, chars), 1.));
                } else if pick < 0.73 {
                    let mut row = Row::new(Kind::Bullets, "", gap);
                    let count = rng.range(3, 10);
                    row.items = (0..count)
                        .map(|_| {
                            let chars = rng.range(20, 200);
                            prose(&mut rng, chars).into()
                        })
                        .collect();
                    rows.push(row);
                } else if pick < 0.94 {
                    let lines = rng.range(3, 140);
                    let mut row = Row::new(Kind::Code, code(&mut rng, lines.min(SHOWN_LINES)), gap);
                    row.lines = lines;
                    rows.push(row);
                } else {
                    rows.push(Row::new(Kind::Image, "", gap));
                }
            }
        }
        rows.push(Row::new(Kind::Actions, "", 0.25));
        rows.push(Row::new(Kind::Turn, "3 s · 1.2k tokens · $0.01", 0.25));
    }
    rows.into_iter().map(Row::counted).collect()
}

const FOREGROUND: u32 = 0x1f2328;
const MUTED: u32 = 0x6e7781;
const RULE: u32 = 0xd0d7de;
const BUBBLE: u32 = 0xe8eef7;
const CODE_BACKGROUND: u32 = 0xf3f4f6;
const NOTICE_BACKGROUND: u32 = 0xfff4d6;
const ACCENT: u32 = 0x8fb3e8;
const IMAGE_COLORS: [u32; 4] = [0xb9d7a8, 0xe3b7a0, 0xa9c4e6, 0xd8c6ea];

fn color(rgb_value: u32) -> Hsla {
    rgb(rgb_value).into()
}

fn line(size: f32, height: f32) -> gpui::Div {
    div()
        .font_family(SANS_FAMILY)
        .text_size(px(size))
        .line_height(px(height))
}

fn mono(lines: &SharedString) -> gpui::Div {
    div()
        .rounded(px(6.))
        .bg(color(CODE_BACKGROUND))
        .px(px(12.))
        .py(px(10.))
        .font_family(MONO_FAMILY)
        .text_size(px(13.))
        .line_height(px(20.))
        .whitespace_nowrap()
        .overflow_hidden()
        .child(lines.clone())
}

fn one_line(label: &SharedString, mark: u32) -> gpui::Div {
    div()
        .py(px(3.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_none()
                .size(px(16.))
                .rounded(px(4.))
                .bg(color(mark)),
        )
        .child(
            line(14., 22.)
                .min_w_0()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(label.clone()),
        )
}

/// The element of `row`, the `ix`th of the transcript.
pub fn render_row(row: &Row, ix: usize) -> AnyElement {
    let body = match row.kind {
        Kind::Separator => div()
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().h(px(1.)).bg(color(RULE)))
            .child(
                line(12., 16.)
                    .text_color(color(MUTED))
                    .child(row.text.clone()),
            )
            .child(div().flex_1().h(px(1.)).bg(color(RULE))),
        Kind::User => div().flex().justify_end().child(
            line(15., 22.)
                .max_w(px(560.))
                .px(px(14.))
                .py(px(8.))
                .rounded(px(12.))
                .bg(color(BUBBLE))
                .child(row.text.clone()),
        ),
        Kind::Refresh | Kind::Turn => line(12., 18.)
            .text_color(color(MUTED))
            .child(row.text.clone()),
        Kind::Thinking => one_line(&row.text, RULE).text_color(color(MUTED)),
        Kind::Call | Kind::Group => one_line(&row.text, ACCENT),
        Kind::CallOpen => {
            let (label, lines) = row.text.split_once('\n').unwrap_or((&row.text, ""));
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(one_line(&SharedString::from(label.to_string()), ACCENT))
                .child(
                    mono(&SharedString::from(lines.to_string()))
                        .text_size(px(12.))
                        .line_height(px(18.)),
                )
        }
        Kind::Notice => line(13., 20.)
            .px(px(10.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(color(NOTICE_BACKGROUND))
            .child(row.text.clone()),
        Kind::Paragraph => line(15., 24.).child(row.text.clone()),
        Kind::Heading => line(18., 28.)
            .pt(px(4.))
            .font_weight(FontWeight::BOLD)
            .child(row.text.clone()),
        Kind::Bullets => div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(row.items.iter().map(|item| {
                div()
                    .flex()
                    .gap(px(8.))
                    .child(line(15., 24.).flex_none().w(px(12.)).child("•"))
                    .child(line(15., 24.).flex_1().min_w_0().child(item.clone()))
            })),
        Kind::Code => {
            let block = div().flex().flex_col().child(mono(&row.text));
            if row.lines > SHOWN_LINES {
                block.child(
                    line(13., 20.)
                        .pt(px(4.))
                        .text_color(color(MUTED))
                        .child(format!("Show all {} lines", row.lines)),
                )
            } else {
                block
            }
        }
        Kind::Image => div()
            .w(px(160.))
            .h(px(120.))
            .rounded(px(8.))
            .bg(color(IMAGE_COLORS[ix % IMAGE_COLORS.len()])),
        Kind::Actions => div()
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(4.))
            .children((0..3).map(|_| {
                div()
                    .size(px(24.))
                    .rounded(px(6.))
                    .bg(color(CODE_BACKGROUND))
            })),
    };
    div()
        .w_full()
        .pt(rems(row.gap))
        .flex()
        .justify_center()
        .text_color(color(FOREGROUND))
        .child(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .px(px(SIDE_PADDING))
                .child(body),
        )
        .into_any_element()
}
