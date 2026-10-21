//! Measures how smoothly a `gpui::list`'s scrollbar moves while the wheel
//! scrolls a long transcript of rows of very different heights, most of
//! them not measured yet, and films it.
//!
//! ```text
//! cargo run -p mhr-gpui-scroll-harness -- --out DIR [--label NAME]
//!     [--scenarios tail,head,medium,tail-list,resize,short] [--film tail[:LEG],...]
//!     [--no-metric] [--turns N] [--medium-turns N] [--short-turns N] [--seed N]
//!     [--frames N] [--refresh-idle]
//! ```
//!
//! It writes `report.txt` (also printed), and for each scenario a trace of
//! every step (`trace-<scenario>.tsv`) and a chart (`chart-<scenario>.png`).
//! `--film` adds a contact sheet (`film-<scenario>.png`) and a GIF
//! (`film-<scenario>.gif`) of frames drawn during the scroll.

mod adopt;
mod film;
mod metrics;
mod run;
mod transcript;
mod view;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::BTreeSet,
    fmt::Write as _,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::{Context as _, Result, bail};
use gpui::{ListAlignment, ListState, px};
use gpui_screenshot::ScreenshotApp;

use crate::adopt::{Facts, adopt};
use crate::metrics::{KEY, LegMetrics, leg_metrics, summary, tables};
use crate::run::{Options, Run, Scenario, WHEEL_STEP, WIDE, rows_for, scenarios};
use crate::view::{MIN_THUMB, OVERDRAW, PADDING, VIEWPORT};

/// Counts the bytes the process has allocated and not freed, and the most
/// it ever had.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            if new_size > layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size
                    - layout.size();
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The bytes the process has allocated and not freed.
pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

const HUD_HEIGHT: f32 = 36.;
/// Frames on a contact sheet, and steps a GIF shows.
const STILLS: usize = 24;
const GIF_STEPS: usize = 120;
/// Steps of the thumb's trail in a filmed frame.
const TRAIL: usize = 12;
const GIF_DELAY_MS: u16 = 50;
/// Rows of the list whose state's memory is measured.
const MEMORY_ROWS: usize = 100_000;

struct Args {
    out: PathBuf,
    label: String,
    scenarios: Vec<String>,
    film: Vec<(String, usize)>,
    metric: bool,
    options: Options,
}

const USAGE: &str = "Usage\n  scroll-harness --out DIR [--label NAME] [--scenarios a,b] [--film a[:leg],b] [--no-metric] [--turns N] [--medium-turns N] [--short-turns N] [--seed N] [--frames N] [--refresh-idle]";

fn parse() -> Result<Args> {
    let mut args = Args {
        out: PathBuf::from("target/scroll-harness"),
        label: String::new(),
        scenarios: Vec::new(),
        film: Vec::new(),
        metric: true,
        options: Options {
            long_turns: 120,
            medium_turns: 30,
            short_turns: 5,
            seed: 7,
            frames_per_step: 1,
            refresh_idle: false,
        },
    };
    let mut given = std::env::args().skip(1);
    while let Some(arg) = given.next() {
        let mut value = || {
            given
                .next()
                .with_context(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--out" => args.out = PathBuf::from(value()?),
            "--label" => args.label = value()?,
            "--scenarios" => args.scenarios = value()?.split(',').map(str::to_string).collect(),
            "--film" => {
                for item in value()?.split(',') {
                    let (name, leg) = item.split_once(':').unwrap_or((item, "0"));
                    args.film.push((name.to_string(), leg.parse()?));
                }
            }
            "--no-metric" => args.metric = false,
            "--turns" => args.options.long_turns = value()?.parse()?,
            "--medium-turns" => args.options.medium_turns = value()?.parse()?,
            "--short-turns" => args.options.short_turns = value()?.parse()?,
            "--seed" => args.options.seed = value()?.parse()?,
            "--frames" => args.options.frames_per_step = value()?.parse()?,
            "--refresh-idle" => args.options.refresh_idle = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ => bail!("unknown argument {arg}\n{USAGE}"),
        }
    }
    if args.label.is_empty() {
        args.label = git(&["rev-parse", "--abbrev-ref", "HEAD"]);
    }
    Ok(args)
}

fn git(args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn chosen(names: &[String]) -> Result<Vec<Scenario>> {
    let all = scenarios();
    if names.is_empty() {
        return Ok(all);
    }
    names
        .iter()
        .map(|name| {
            all.iter()
                .find(|scenario| scenario.name == name)
                .cloned()
                .with_context(|| format!("no scenario {name}"))
        })
        .collect()
}

/// The bytes a list state for `MEMORY_ROWS` rows holds, with what the
/// candidate's `adopt` gives it.
fn list_memory(app: &mut ScreenshotApp, options: &Options) -> Result<usize> {
    let long = scenarios()
        .into_iter()
        .find(|scenario| scenario.length == run::Length::Long)
        .unwrap();
    let rows = rows_for(&long, options);
    let heights = run::measure(app, &rows, WIDE)?;
    let rows: Vec<_> = rows.iter().cycle().take(MEMORY_ROWS).cloned().collect();
    let exact: Vec<f32> = heights.iter().cycle().take(MEMORY_ROWS).copied().collect();
    let before = live_bytes();
    let state = ListState::new(MEMORY_ROWS, ListAlignment::Top, px(OVERDRAW));
    adopt(
        &state,
        &Facts {
            rows: &rows,
            width: px(WIDE),
            exact: &exact,
        },
    );
    let held = live_bytes().saturating_sub(before);
    drop(state);
    Ok(held)
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024. * 1024.)
}

fn main() -> Result<()> {
    let args = parse()?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;
    let mut app = ScreenshotApp::new()?;
    let options = &args.options;
    let commit = git(&["rev-parse", "--short", "HEAD"]);
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    let mut header = String::new();
    let _ = writeln!(
        header,
        "scroll-harness  label {}  commit {}  profile {}  frames a step {}{}",
        args.label,
        commit,
        profile,
        options.frames_per_step,
        if options.refresh_idle {
            ", idle frames redraw"
        } else {
            ""
        }
    );
    let _ = writeln!(
        header,
        "viewport {WIDE}x{VIEWPORT}, padding {PADDING}, overdraw {OVERDRAW}, wheel {WHEEL_STEP}px a step, shortest thumb {MIN_THUMB}px, seed {}, turns {} long, {} medium, {} short",
        options.seed, options.long_turns, options.medium_turns, options.short_turns
    );

    let mut runs: Vec<(Run, Vec<LegMetrics>)> = Vec::new();
    if args.metric {
        let mut all = Vec::new();
        let mut heaps = String::new();
        for scenario in chosen(&args.scenarios)? {
            eprintln!("scenario {}: {}", scenario.name, scenario.about);
            let heap_before = live_bytes();
            let run = run::run(&mut app, &scenario, options, 0., None)?;
            let _ = writeln!(
                heaps,
                "  {:<10} heap while open {:>7.1} MB",
                scenario.name,
                mb(run.heap.saturating_sub(heap_before))
            );
            let metrics: Vec<LegMetrics> = (0..run.legs.len())
                .map(|ix| leg_metrics(&run, ix))
                .collect();
            std::fs::write(
                args.out.join(format!("trace-{}.tsv", scenario.name)),
                metrics::trace(&run),
            )?;
            if options.frames_per_step > 1 {
                std::fs::write(
                    args.out.join(format!("frames-{}.tsv", scenario.name)),
                    metrics::frames(&run),
                )?;
            }
            let exact: Vec<f32> = metrics.iter().map(|leg| leg.exact_travel).collect();
            film::write_chart(
                &mut app,
                format!("{} · {}, {}", args.label, scenario.name, scenario.about),
                film::chart_legs(&run, &exact),
                &args.out.join(format!("chart-{}.png", scenario.name)),
            )?;
            all.extend(metrics.iter().cloned());
            runs.push((run, metrics));
        }
        let state_bytes = list_memory(&mut app, options)?;
        let mut report = header.clone();
        report.push('\n');
        report.push_str(&tables(&all));
        report.push('\n');
        report.push_str(&summary(&all));
        report.push('\n');
        let _ = writeln!(
            report,
            "MEMORY\n  list state for {MEMORY_ROWS} rows {:>7.2} MB ({:.1} bytes a row)\n{heaps}  process heap now {:>7.1} MB, at most {:.1} MB",
            mb(state_bytes),
            state_bytes as f64 / MEMORY_ROWS as f64,
            mb(live_bytes()),
            mb(PEAK.load(Ordering::Relaxed)),
        );
        report.push('\n');
        report.push_str(KEY);
        std::fs::write(args.out.join("report.txt"), &report)?;
        println!("{report}");
    }

    for (name, leg_ix) in &args.film {
        let scenario = chosen(std::slice::from_ref(name))?.remove(0);
        anyhow::ensure!(*leg_ix < scenario.legs.len(), "{name} has no leg {leg_ix}");
        let found = runs
            .iter()
            .position(|(run, _)| run.scenario.name == scenario.name);
        let ix = match found {
            Some(ix) => ix,
            None => {
                let run = run::run(&mut app, &scenario, options, 0., None)?;
                let metrics = (0..run.legs.len())
                    .map(|ix| leg_metrics(&run, ix))
                    .collect();
                runs.push((run, metrics));
                runs.len() - 1
            }
        };
        let (run, metrics) = &runs[ix];
        film_scenario(&mut app, &args, &scenario, *leg_ix, run, metrics, &header)?;
    }
    Ok(())
}

/// Replays `scenario` up to the leg `leg_ix`, and films that leg.
fn film_scenario(
    app: &mut ScreenshotApp,
    args: &Args,
    scenario: &Scenario,
    leg_ix: usize,
    run: &Run,
    metrics: &[LegMetrics],
    header: &str,
) -> Result<()> {
    eprintln!("filming {} leg {leg_ix}", scenario.name);
    let leg = &run.legs[leg_ix];
    let exact_travel = metrics[leg_ix].exact_travel;
    let steps = leg.steps.len() - 1;
    let direction = if leg.leg.up { -1. } else { 1. };
    // Half the stills spread over the leg, the other half one step after
    // the other around its largest step, and a GIF around that step too.
    let spike = (1..steps)
        .max_by(|a, b| {
            let travel =
                |ix: usize| (leg.steps[ix].thumb.top - leg.steps[ix - 1].thumb.top) * direction;
            travel(*a).total_cmp(&travel(*b))
        })
        .unwrap_or(1);
    let spread = STILLS / 2;
    let around_start = spike
        .saturating_sub(spread / 2)
        .max(1)
        .min(steps.saturating_sub(spread).max(1));
    let stills: BTreeSet<usize> = (0..spread)
        .map(|ix| 1 + ix * (steps.saturating_sub(2)) / (spread - 1).max(1))
        .chain(around_start..(around_start + spread).min(steps + 1))
        .collect();
    let gif_start = spike.saturating_sub(GIF_STEPS / 2).max(1);
    let gif_steps = gif_start..(gif_start + GIF_STEPS).min(steps + 1);

    let mut replay = scenario.clone();
    replay.legs.truncate(leg_ix + 1);
    let mut sheet = Vec::new();
    let mut gif = Vec::new();
    let leg_name = leg.leg.name();
    let up = leg.leg.up;
    let mut film = |app: &mut ScreenshotApp,
                    window: gpui::WindowHandle<view::Harness>,
                    shot: &run::Shot|
     -> Result<()> {
        if shot.leg != leg_ix {
            return Ok(());
        }
        let (still, framed) = (stills.contains(&shot.step), gif_steps.contains(&shot.step));
        if !still && !framed {
            return Ok(());
        }
        let caption = film::caption(shot, up, exact_travel);
        let hud = caption.clone();
        let trail: Vec<f32> = shot.steps[shot.step.saturating_sub(TRAIL)..shot.step]
            .iter()
            .map(|step| step.thumb.top)
            .collect();
        app.update(|cx| {
            window.update(cx, |view, _, cx| {
                view.hud = hud.into();
                view.trail = trail;
                cx.notify();
            })
        })?;
        let shot_image = app.capture(window)?.image;
        if still {
            sheet.push(film::Still {
                image: shot_image.clone(),
                caption,
            });
        }
        if framed {
            gif.push(shot_image);
        }
        Ok(())
    };
    run::run(app, &replay, &args.options, HUD_HEIGHT, Some(&mut film))?;

    let exact: Vec<f32> = metrics.iter().map(|leg| leg.exact_travel).collect();
    let mut report = header.to_string();
    report.push('\n');
    report.push_str(&tables(metrics));
    report.push('\n');
    report.push_str(KEY);
    film::write_sheet(
        app,
        format!("{} · {} {}", args.label, scenario.name, leg_name),
        format!(
            "{}, {} rows, {:.0}px of content. The first {spread} stills are spread over the leg, the next {spread} follow one another around its largest step ({spike}), as does film-{}.gif (steps {} to {}). Red ticks beside the track mark the thumb's last {TRAIL} places.",
            scenario.about,
            run.rows.len(),
            leg.content,
            scenario.name,
            gif_steps.start,
            gif_steps.end - 1,
        ),
        &sheet,
        film::chart_legs(run, &exact),
        report,
        &args.out.join(format!("film-{}.png", scenario.name)),
    )?;
    film::write_gif(
        &gif,
        GIF_DELAY_MS,
        &args.out.join(format!("film-{}.gif", scenario.name)),
    )?;
    Ok(())
}
