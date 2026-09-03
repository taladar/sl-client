//! Report the divergences: contact sheet, image diff, scene-dump diff.
//!
//! ```text
//! cargo run --release -p sl-crosscheck --bin sl-crosscheck-report -- \
//!     crosscheck-runs/catalogue
//! ```
//!
//! One run directory in, one `report/` directory out — a contact sheet, a
//! difference image per frame, `report.txt` and `report.json`. Point it at
//! several runs to rank the scenes by how far apart the two viewers are in each,
//! which is how attention gets to the worst one.
//!
//! **It never fails a build and never enters `cargo nextest`.** A pixel
//! comparison across two renderers, two GPUs and two driver versions measures
//! the environment at least as much as the code, and a check that fails on a
//! Mesa upgrade is one that gets disabled and then ignored. This says
//! *different*; the tiered harness says *wrong*; a person decides which viewer
//! is right — and the answer is not automatically ours. (Firestorm drew every
//! avatar with no right hand for a decade before this harness noticed:
//! `avatarSkinV.glsl` reads one past its matrix palette, and `NaN * 0` is `NaN`.
//! secondlife/viewer#6240.)
//!
//! The exit status says whether the *reports were produced*, never whether the
//! viewers agreed.

use std::path::PathBuf;

use clap::Parser;
use sl_crosscheck::report::{self, Report, Spec};
use sl_crosscheck::scene_diff::Tolerances;

/// Command-line options.
#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Options {
    /// The run directories to report on — what `sl-crosscheck` collected, one
    /// per scene. Several of them are ranked against each other at the end.
    #[arg(required = true)]
    runs: Vec<PathBuf>,

    /// Where each report goes. Defaults to `report/` inside its own run
    /// directory, so a run stays one thing that can be copied or deleted whole.
    #[arg(long)]
    out: Option<PathBuf>,

    /// How many rows the contact sheet gets. The frames are spread across the
    /// whole run rather than taken from its start, and the report says how many
    /// it left out.
    #[arg(long, default_value_t = 6)]
    rows: usize,

    /// How wide one contact-sheet cell is drawn, in pixels.
    #[arg(long, default_value_t = 640)]
    cell_width: u32,

    /// How many findings and frames the printed report lists before saying how
    /// many more there are.
    #[arg(long, default_value_t = 25)]
    findings: usize,

    /// Skip the per-frame difference images.
    #[arg(long)]
    no_heatmaps: bool,

    /// How far apart two positions may be, in metres, before the scene diff
    /// says so.
    #[arg(long, default_value_t = 0.01)]
    tolerance_metres: f64,

    /// How far apart two orientations may be, in degrees.
    #[arg(long, default_value_t = 0.5)]
    tolerance_degrees: f64,
}

/// Print to standard output: the report is this binary's primary output.
#[expect(
    clippy::print_stdout,
    reason = "the report is this binary's primary output"
)]
fn print(text: &str) {
    println!("{text}");
}

/// The line ranking one run against the others.
fn ranking(reports: &[Report]) -> String {
    let mut ranked: Vec<(&Report, Option<f64>)> = reports
        .iter()
        .map(|report| (report, report.median_difference()))
        .collect();
    ranked.sort_by(|first, second| {
        second
            .1
            .unwrap_or(f64::MIN)
            .total_cmp(&first.1.unwrap_or(f64::MIN))
    });
    let mut lines = vec![
        "scenes, furthest apart first — a ranking of where to look, not of who is wrong:"
            .to_owned(),
    ];
    for (report, median) in ranked {
        lines.push(format!(
            // The run directory as well as the scene: two runs of one scenario
            // is the commonest reason to rank at all — before a change and
            // after it — and a ranking that names them both "catalogue" cannot
            // be acted on.
            "  {} ({}): {}",
            report.scenario.as_deref().unwrap_or("(no run.json)"),
            report.run.display(),
            median.map_or_else(
                || "nothing compared".to_owned(),
                |median| format!("median frame difference {median:.4}")
            )
        ));
    }
    lines.join("\n")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_error| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let options = Options::parse();
    if options.out.is_some() && options.runs.len() > 1 {
        // Two reports written over one another is one report and a lie about
        // the other.
        return Err("--out names one directory, so it takes one run".into());
    }

    let mut reports = Vec::new();
    for run in &options.runs {
        let mut spec = Spec::new(run);
        if let Some(out) = &options.out {
            spec.out.clone_from(out);
        }
        spec.rows = options.rows;
        spec.cell_width = options.cell_width;
        spec.findings = options.findings;
        spec.heatmaps = !options.no_heatmaps;
        spec.tolerances = Tolerances {
            metres: options.tolerance_metres,
            degrees: options.tolerance_degrees,
            ..Tolerances::default()
        };
        let report = report::build(&spec)?;
        print(&format!("\n{}\n", report.render(options.findings)));
        print(&format!("written to {}", spec.out.display()));
        reports.push(report);
    }
    if reports.len() > 1 {
        print(&format!("\n{}", ranking(&reports)));
    }
    Ok(())
}
