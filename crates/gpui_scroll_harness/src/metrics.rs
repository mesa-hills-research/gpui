//! What the runs measure, and the report that compares them.

use std::fmt::Write as _;

use crate::run::{LegRun, Run, WHEEL_STEP};
use crate::view::VIEWPORT;

/// How one leg of a scenario went, step by step of the wheel.
#[derive(Clone, Debug, Default)]
pub struct LegMetrics {
    pub scenario: String,
    pub leg: String,
    /// Whether the leg starts over rows the list hasn't measured at its
    /// width, where the thumb has to guess.
    pub unmeasured: bool,
    pub rows: usize,
    pub content: f32,
    pub steps: usize,
    /// The thumb's travel per step if every height were known, in px.
    pub exact_travel: f32,
    /// The thumb's travel per step, in px.
    pub travel_mean: f32,
    pub travel_sd: f32,
    pub travel_cv: f32,
    /// The travel per step as a share of the exact travel.
    pub ratio_mean: f32,
    pub ratio_sd: f32,
    pub ratio_cv: f32,
    /// The largest and smallest travel per step, as shares of the median.
    pub spike: f32,
    pub dip: f32,
    /// The largest change of travel from one step to the next, and their
    /// root mean square, as shares of the median travel.
    pub jerk: f32,
    pub jerk_rms: f32,
    /// Steps that moved the thumb against the wheel, and the largest.
    pub back: usize,
    pub back_max: f32,
    /// How far the thumb was from where exact heights put it, in px.
    pub error_max: f32,
    pub error_mean: f32,
    /// Steps that changed the thumb's length, the largest change and the
    /// whole range, in px.
    pub length_changes: usize,
    pub length_max: f32,
    pub length_range: f32,
    /// The content height the scrollbar's size came from, its largest
    /// change in a step and its whole range, as shares of the exact height.
    pub estimate_max: f32,
    pub estimate_range: f32,
    /// Steps that moved the content by more or less than the wheel asked.
    pub jumps: usize,
    pub jump_max: f32,
    /// The frame after each step, in ms.
    pub frame_p50: f32,
    pub frame_p95: f32,
    pub frame_max: f32,
    /// Idle frames after each step, in ms a step.
    pub idle_mean: f32,
    /// Rows drawn or measured each step.
    pub renders_mean: f32,
    pub renders_max: u32,
    /// The scrollbar's questions to the list each frame, in µs.
    pub query_mean: f32,
    /// Handling the wheel event, in µs.
    pub event_mean: f32,
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        0.
    } else {
        values.iter().sum::<f32>() / values.len() as f32
    }
}

fn sd(values: &[f32]) -> f32 {
    let mean = mean(values);
    if values.len() < 2 {
        return 0.;
    }
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt()
}

fn percentile(values: &[f32], share: f32) -> f32 {
    if values.is_empty() {
        return 0.;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[((sorted.len() - 1) as f32 * share).round() as usize]
}

fn ratio(value: f32, of: f32) -> f32 {
    if of.abs() > f32::EPSILON {
        value / of
    } else {
        0.
    }
}

pub fn leg_metrics(run: &Run, leg_ix: usize) -> LegMetrics {
    let leg: &LegRun = &run.legs[leg_ix];
    let steps = &leg.steps;
    let direction = if leg.leg.up { -1. } else { 1. };
    // The last step stops at the end of the list, short of a full step, so
    // only the steps before it count for travel.
    let pairs: Vec<_> = steps.windows(2).collect();
    let travel: Vec<f32> = pairs[..pairs.len().saturating_sub(1)]
        .iter()
        .map(|pair| (pair[1].thumb.top - pair[0].thumb.top) * direction)
        .collect();
    let exact_travel = mean(
        &pairs[..pairs.len().saturating_sub(1)]
            .iter()
            .map(|pair| (pair[1].exact.top - pair[0].exact.top) * direction)
            .collect::<Vec<_>>(),
    );
    let median = percentile(&travel, 0.5);
    let ratios: Vec<f32> = travel.iter().map(|t| ratio(*t, exact_travel)).collect();
    let changes: Vec<f32> = travel.windows(2).map(|t| (t[1] - t[0]).abs()).collect();
    let back: Vec<f32> = travel.iter().filter(|t| **t < -0.001).map(|t| -t).collect();

    let errors: Vec<f32> = steps
        .iter()
        .map(|step| (step.thumb.top - step.exact.top).abs())
        .collect();
    let lengths: Vec<f32> = steps.iter().map(|step| step.thumb.length).collect();
    let length_steps: Vec<f32> = lengths.windows(2).map(|l| (l[1] - l[0]).abs()).collect();
    let estimates: Vec<f32> = steps.iter().map(|step| step.thumb.max + VIEWPORT).collect();
    let estimate_steps: Vec<f32> = estimates.windows(2).map(|e| (e[1] - e[0]).abs()).collect();
    let moved: Vec<f32> = pairs[..pairs.len().saturating_sub(1)]
        .iter()
        .map(|pair| (pair[1].exact_offset - pair[0].exact_offset).abs())
        .collect();
    let jumps: Vec<f32> = moved
        .iter()
        .map(|m| (m - WHEEL_STEP).abs())
        .filter(|off| *off > 0.5)
        .collect();
    let frames: Vec<f32> = steps[1..]
        .iter()
        .map(|step| step.frame_ns as f32 / 1e6)
        .collect();
    let max_of = |values: &[f32]| values.iter().copied().fold(0f32, f32::max);
    let min_of = |values: &[f32]| values.iter().copied().fold(f32::INFINITY, f32::min);
    let travel_mean = mean(&travel);
    let ratio_mean = mean(&ratios);
    LegMetrics {
        scenario: run.scenario.name.to_string(),
        leg: leg.leg.name(),
        unmeasured: leg_ix == 0 || run.legs[leg_ix - 1].leg.width != leg.leg.width,
        rows: run.rows.len(),
        content: leg.content,
        steps: steps.len() - 1,
        exact_travel,
        travel_mean,
        travel_sd: sd(&travel),
        travel_cv: ratio(sd(&travel), travel_mean),
        ratio_mean,
        ratio_sd: sd(&ratios),
        ratio_cv: ratio(sd(&ratios), ratio_mean),
        spike: ratio(max_of(&travel), median),
        dip: if travel.is_empty() {
            0.
        } else {
            ratio(min_of(&travel), median)
        },
        jerk: ratio(max_of(&changes), median),
        jerk_rms: ratio(
            (mean(&changes.iter().map(|c| c * c).collect::<Vec<_>>())).sqrt(),
            median,
        ),
        back: back.len(),
        back_max: max_of(&back),
        error_max: max_of(&errors),
        error_mean: mean(&errors),
        length_changes: length_steps.iter().filter(|l| **l > 0.01).count(),
        length_max: max_of(&length_steps),
        length_range: max_of(&lengths) - min_of(&lengths),
        estimate_max: ratio(max_of(&estimate_steps), leg.content) * 100.,
        estimate_range: ratio(max_of(&estimates) - min_of(&estimates), leg.content) * 100.,
        jumps: jumps.len(),
        jump_max: max_of(&jumps),
        frame_p50: percentile(&frames, 0.5),
        frame_p95: percentile(&frames, 0.95),
        frame_max: max_of(&frames),
        idle_mean: mean(
            &steps[1..]
                .iter()
                .map(|step| step.idle_ns as f32 / 1e6)
                .collect::<Vec<_>>(),
        ),
        renders_mean: mean(
            &steps[1..]
                .iter()
                .map(|step| step.renders as f32)
                .collect::<Vec<_>>(),
        ),
        renders_max: steps[1..]
            .iter()
            .map(|step| step.renders)
            .max()
            .unwrap_or(0),
        query_mean: mean(
            &steps[1..]
                .iter()
                .map(|step| step.thumb.query_ns as f32 / 1e3)
                .collect::<Vec<_>>(),
        ),
        event_mean: mean(
            &steps[1..]
                .iter()
                .map(|step| step.event_ns as f32 / 1e3)
                .collect::<Vec<_>>(),
        ),
    }
}

/// The two tables of the report, one line per leg.
pub fn tables(legs: &[LegMetrics]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "THUMB TRAVEL PER WHEEL STEP (px on a {VIEWPORT}px track, ratio = travel / exact travel)"
    );
    let _ = writeln!(
        out,
        "{:<10} {:<10} {:>5} {:>6} {:>7} | {:>7} {:>7} {:>6} {:>6} | {:>6} {:>6} {:>6} | {:>6} {:>6} {:>6} {:>6} | {:>4} {:>6}",
        "scenario",
        "leg",
        "rows",
        "steps",
        "content",
        "exact",
        "mean",
        "sd",
        "cv",
        "ratio",
        "sd",
        "cv",
        "spike",
        "dip",
        "jerk",
        "jrms",
        "back",
        "max"
    );
    for leg in legs {
        let _ = writeln!(
            out,
            "{:<10} {:<10} {:>5} {:>6} {:>7.0} | {:>7.3} {:>7.3} {:>6.3} {:>6.3} | {:>6.3} {:>6.3} {:>6.3} | {:>6.2} {:>6.2} {:>6.2} {:>6.3} | {:>4} {:>6.2}",
            leg.scenario,
            format!("{}{}", leg.leg, if leg.unmeasured { "*" } else { "" }),
            leg.rows,
            leg.steps,
            leg.content,
            leg.exact_travel,
            leg.travel_mean,
            leg.travel_sd,
            leg.travel_cv,
            leg.ratio_mean,
            leg.ratio_sd,
            leg.ratio_cv,
            leg.spike,
            leg.dip,
            leg.jerk,
            leg.jerk_rms,
            leg.back,
            leg.back_max,
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "THUMB PLACE AND SIZE, CONTENT, COST (ms for the frame after a step, µs for the scrollbar's questions and the wheel event)"
    );
    let _ = writeln!(
        out,
        "{:<10} {:<10} | {:>6} {:>6} | {:>5} {:>6} {:>6} | {:>6} {:>6} | {:>5} {:>7} | {:>6} {:>6} {:>7} {:>6} | {:>6} {:>4} | {:>6} {:>6}",
        "scenario",
        "leg",
        "errmax",
        "errav",
        "sizes",
        "szmax",
        "range",
        "est%mx",
        "est%rg",
        "jumps",
        "jumpmax",
        "ms p50",
        "p95",
        "max",
        "idle",
        "rows",
        "max",
        "sb µs",
        "ev µs"
    );
    for leg in legs {
        let _ = writeln!(
            out,
            "{:<10} {:<10} | {:>6.1} {:>6.1} | {:>5} {:>6.2} {:>6.2} | {:>6.3} {:>6.2} | {:>5} {:>7.1} | {:>6.2} {:>6.2} {:>7.2} {:>6.2} | {:>6.2} {:>4} | {:>6.2} {:>6.1}",
            leg.scenario,
            format!("{}{}", leg.leg, if leg.unmeasured { "*" } else { "" }),
            leg.error_max,
            leg.error_mean,
            leg.length_changes,
            leg.length_max,
            leg.length_range,
            leg.estimate_max,
            leg.estimate_range,
            leg.jumps,
            leg.jump_max,
            leg.frame_p50,
            leg.frame_p95,
            leg.frame_max,
            leg.idle_mean,
            leg.renders_mean,
            leg.renders_max,
            leg.query_mean,
            leg.event_mean,
        );
    }
    out
}

/// One line per score over the legs over unmeasured rows, the worst of
/// them, for a quick comparison.
pub fn summary(legs: &[LegMetrics]) -> String {
    let unmeasured: Vec<&LegMetrics> = legs.iter().filter(|leg| leg.unmeasured).collect();
    let worst =
        |f: &dyn Fn(&LegMetrics) -> f32| unmeasured.iter().map(|leg| f(leg)).fold(0f32, f32::max);
    let mean_of = |f: &dyn Fn(&LegMetrics) -> f32| {
        mean(&unmeasured.iter().map(|leg| f(leg)).collect::<Vec<_>>())
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "SUMMARY of the legs marked * (over unmeasured rows), the worst and the mean"
    );
    let lines: [(&str, &dyn Fn(&LegMetrics) -> f32); 9] = [
        ("travel cv", &|leg| leg.travel_cv),
        ("ratio cv", &|leg| leg.ratio_cv),
        ("spike (max/median)", &|leg| leg.spike),
        ("jerk (max step change/median)", &|leg| leg.jerk),
        ("thumb error px (max)", &|leg| leg.error_max),
        ("size change px (max a step)", &|leg| leg.length_max),
        ("content jumps", &|leg| leg.jumps as f32),
        ("frame ms p95", &|leg| leg.frame_p95),
        ("rows a step (mean)", &|leg| leg.renders_mean),
    ];
    for (name, f) in lines {
        let _ = writeln!(out, "  {:<32} {:>9.3} {:>9.3}", name, worst(f), mean_of(f));
    }
    let backs: usize = unmeasured.iter().map(|leg| leg.back).sum();
    let _ = writeln!(out, "  {:<32} {:>9}", "backward steps (all)", backs);
    out
}

/// What the columns mean.
pub const KEY: &str = "\
KEY
  leg       up or down, at the list's width, with * on legs over rows not measured at that width
  content   the content's real height in px (rows and padding)
  exact     the thumb's travel per step if every height were known
  mean, cv  the thumb's travel per step, its sd and its coefficient of variation (sd / mean)
  ratio     travel / exact travel per step, its mean, sd and cv (1.000 and 0 are ideal)
  spike     the largest travel per step / the median (1 is ideal)
  dip       the smallest travel per step / the median (1 is ideal, below 0 is a step backward)
  jerk      the largest change of travel between two steps / the median, and jrms their rms
  back      steps that moved the thumb against the wheel, and the largest in px
  errmax    the largest distance in px between the thumb and where exact heights put it, errav the mean
  sizes     steps that changed the thumb's length, szmax the largest change and range the whole range, in px
  est%      the largest change in one step of the content height the scrollbar used, and its range, in % of the real height
  jumps     steps that moved the content more or less than the wheel's 60px (by over 0.5px), and the largest difference
  ms        the frame after a step (median, 95th percentile, largest), and idle frames in ms a step
  rows      rows the list drew or measured a step, mean and largest
  sb µs     the scrollbar's two questions to the list in a frame
  ev µs     handling a wheel event (with GPUI Kit's mask, its questions to the list and set_offset_from_scrollbar)
";

/// One line per step of every leg, for plotting and diffing.
pub fn trace(run: &Run) -> String {
    let mut out = String::from(
        "leg\tstep\titem\tkind\toffset_in_item\texact_offset\tthumb_top\tthumb_length\texact_top\texact_length\tmax\toffset\tframe_us\tidle_us\trows\n",
    );
    for leg in &run.legs {
        for (ix, step) in leg.steps.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.3}\t{:.3}\t{:.1}\t{:.1}\t{}",
                leg.leg.name(),
                ix,
                step.top.item_ix,
                run.rows
                    .get(step.top.item_ix)
                    .map_or("end", |row| row.kind.name()),
                f32::from(step.top.offset_in_item),
                step.exact_offset,
                step.thumb.top,
                step.thumb.length,
                step.exact.top,
                step.exact.length,
                step.thumb.max,
                step.thumb.offset,
                step.frame_ns as f32 / 1e3,
                step.idle_ns as f32 / 1e3,
                step.renders,
            );
        }
    }
    out
}

/// One line per frame, idle frames included.
pub fn frames(run: &Run) -> String {
    let mut out = String::from("leg\tframe\tstep\tthumb_top\tthumb_length\n");
    for leg in &run.legs {
        for (ix, frame) in leg.frames.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{:.4}\t{:.4}",
                leg.leg.name(),
                ix,
                frame.step,
                frame.thumb.top,
                frame.thumb.length
            );
        }
    }
    out
}
