//! The read-only conformance report.
//!
//! Walks the committed `records/` tree and prints a `cargo test`-style summary:
//! one row per test, a status per grid, and — under each — the per-metric trend
//! of the latest run versus the previous one. Does no network I/O; it only reads
//! records, so it can run anywhere the repository is checked out. Exits non-zero
//! when any recorded run failed, so it can gate scripts.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use clap::Parser as _;
use owo_colors::OwoColorize as _;
use sl_conformance::gitinfo;
use sl_conformance::grid::Grid;
use sl_conformance::record::Record;
use sl_conformance::registry::registry;
use sl_conformance::report::{
    Cell, CellStatus, Freshness, Judgement, MetricDelta, classify, freshness_of,
};

/// When to colourise output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum ColorChoice {
    /// Colour only when stdout is a terminal and `NO_COLOR` is unset.
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

/// Run the reporter.
fn main() {
    let args = Options::parse();
    match run(&args) {
        Ok(any_failed) => {
            if any_failed {
                std::process::exit(1);
            }
        }
        Err(error) => {
            report_error(&error);
            std::process::exit(2);
        }
    }
}

/// Print a fatal error to stderr.
#[expect(
    clippy::print_stderr,
    reason = "a CLI binary reports fatal errors on stderr"
)]
fn report_error(error: &Error) {
    eprintln!("error: {error}");
}

/// Render the whole report, returning whether any recorded run failed.
///
/// # Errors
///
/// Returns an [`enum@Error`] if the records directory cannot be located.
fn run(args: &Options) -> Result<bool, Error> {
    let repo_root = gitinfo::repo_root(Path::new(".")).ok();
    let records_dir = match &args.records {
        Some(dir) => dir.clone(),
        None => repo_root
            .clone()
            .ok_or_else(|| Error::Records("not in a git repository (pass --records)".to_owned()))?
            .join("records"),
    };
    // The current behaviour-aware describe, used to flag stale records. Absent
    // when git is unavailable; freshness then reads as Unknown.
    let current_describe = repo_root
        .as_ref()
        .and_then(|root| gitinfo::behavior_describe(root).ok())
        .map(|describe| describe.describe_string());
    let color = color_enabled(args.color);
    let grids: Vec<Grid> = match args.grid {
        Some(grid) => vec![grid],
        None => Grid::ALL.to_vec(),
    };

    let mut any_failed = false;
    let mut tallies: Vec<(Grid, usize, usize, usize)> =
        grids.iter().map(|grid| (*grid, 0, 0, 0)).collect();

    for test in registry() {
        let mut cells: Vec<String> = Vec::new();
        let mut detail_lines: Vec<String> = Vec::new();
        for grid in &grids {
            let applicable = test.grids().contains(grid);
            let record = load_record(&records_dir, *grid, test.name());
            let recorded = record
                .as_ref()
                .and_then(Record::newest)
                .map(|run| run.behavior_describe.clone());
            // Count behaviour-changing commits since the recorded run, so a record
            // stays "current" through commits that only touched records/docs.
            let behind = match (repo_root.as_deref(), recorded.as_deref()) {
                (Some(root), Some(rec)) => gitinfo::behavioural_commits_behind(root, rec),
                _ => None,
            };
            let freshness = match recorded.as_deref() {
                Some(rec) => freshness_of(rec, current_describe.as_deref(), behind),
                None => Freshness::Unknown,
            };
            let cell = classify(applicable, record.as_ref(), freshness);
            cells.push(format!(
                "{}: {}",
                grid.dir_name(),
                render_status(&cell, behind, color)
            ));
            tally(&mut tallies, *grid, cell.status);
            if matches!(cell.status, CellStatus::Fail) {
                any_failed = true;
            }
            for delta in &cell.deltas {
                detail_lines.push(format!(
                    "    [{}] {}",
                    grid.dir_name(),
                    render_delta(delta, color)
                ));
            }
        }
        print_line(&format!("{:24}{}", test.name(), cells.join("   ")));
        for detail in detail_lines {
            print_line(&detail);
        }
    }

    print_line("");
    for (grid, ok, failed, never) in &tallies {
        let summary = format!(
            "{}: {} ok, {} FAILED, {} never ran",
            grid.dir_name(),
            ok,
            failed,
            never
        );
        print_line(&summary);
    }
    Ok(any_failed)
}

/// Load a record, treating a parse error as a missing record (with a warning).
fn load_record(records_dir: &Path, grid: Grid, test: &str) -> Option<Record> {
    let path = Record::path(records_dir, grid, test);
    match Record::load(&path) {
        Ok(record) => record,
        Err(error) => {
            warn_line(&format!("could not read {}: {error}", path.display()));
            None
        }
    }
}

/// Update the ok/failed/never tally for a grid from a cell status.
fn tally(tallies: &mut [(Grid, usize, usize, usize)], grid: Grid, status: CellStatus) {
    for entry in tallies.iter_mut() {
        if entry.0 == grid {
            match status {
                CellStatus::Pass => entry.1 = entry.1.saturating_add(1),
                CellStatus::Fail => entry.2 = entry.2.saturating_add(1),
                CellStatus::NeverRan => entry.3 = entry.3.saturating_add(1),
                CellStatus::NotApplicable => {}
            }
        }
    }
}

/// Whether colour should be applied for the chosen mode.
fn color_enabled(choice: ColorChoice) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
        }
    }
}

/// Render a status cell, annotating it with commit freshness, dirtiness, and
/// partial state.
fn render_status(cell: &Cell, behind: Option<u32>, color: bool) -> String {
    let base = match cell.status {
        CellStatus::Pass => paint(color, "ok", Tone::Green),
        CellStatus::Fail => paint(color, "FAILED", Tone::Red),
        CellStatus::NeverRan => paint(color, "\u{b7} never ran", Tone::Dim),
        CellStatus::NotApplicable => paint(color, "n/a", Tone::Dim),
    };
    let mut text = base;
    match cell.freshness {
        Freshness::Current => {
            if cell.dirty {
                // Ran at the current commit, but on an uncommitted tree.
                text.push_str(&paint(color, " (dirty@current)", Tone::Yellow));
            }
        }
        Freshness::Stale => {
            let recorded = cell.recorded_describe.as_deref().unwrap_or("?");
            let distance = match behind {
                Some(1) => "1 behavioural commit behind".to_owned(),
                Some(count) => format!("{count} behavioural commits behind"),
                None => "older commit".to_owned(),
            };
            text.push_str(&paint(
                color,
                &format!(" (stale: {distance} @ {recorded})"),
                Tone::Yellow,
            ));
        }
        Freshness::Unknown => {
            if let Some(recorded) = &cell.recorded_describe {
                text.push_str(&paint(color, &format!(" (@ {recorded})"), Tone::Dim));
            }
        }
    }
    if cell.partial {
        let note = cell.note.as_deref().map_or_else(
            || " (partial)".to_owned(),
            |note| format!(" (partial: {note})"),
        );
        text.push_str(&paint(color, &note, Tone::Yellow));
    }
    text
}

/// Render one metric delta line.
fn render_delta(delta: &MetricDelta, color: bool) -> String {
    let new = format_number(delta.new);
    // A first-ever value has nothing to compare against.
    if matches!(delta.judgement, Judgement::New) {
        return format!(
            "{key:28} {new} ({tag})",
            key = delta.key,
            tag = paint(color, "new", Tone::Dim),
        );
    }
    let old = delta.old.map_or_else(|| "new".to_owned(), format_number);
    let change = match delta.percent {
        Some(percent) => format!("{percent:+.1}%"),
        None => "n/a".to_owned(),
    };
    let (label, tone) = match delta.judgement {
        Judgement::Better => ("better", Tone::Green),
        Judgement::Worse => ("worse", Tone::Red),
        Judgement::Unchanged => ("unchanged", Tone::Dim),
        Judgement::New => ("new", Tone::Dim),
        Judgement::Neutral => {
            if delta.comparable {
                ("", Tone::None)
            } else {
                ("partial", Tone::Dim)
            }
        }
    };
    let verdict = if label.is_empty() {
        String::new()
    } else {
        format!(" {}", paint(color, label, tone))
    };
    format!(
        "{key:28} {old} -> {new} ({change}{verdict})",
        key = delta.key,
    )
}

/// Format a numeric value compactly: integers without a fraction, else three
/// decimals.
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.3}")
    }
}

/// The colour tones used by the reporter.
#[derive(Clone, Copy, Debug)]
enum Tone {
    /// No colour.
    None,
    /// Green (good).
    Green,
    /// Red (bad).
    Red,
    /// Yellow (warning).
    Yellow,
    /// Dim (de-emphasised).
    Dim,
}

/// Apply a colour tone to `text` when `enabled`.
fn paint(enabled: bool, text: &str, tone: Tone) -> String {
    if !enabled {
        return text.to_owned();
    }
    match tone {
        Tone::None => text.to_owned(),
        Tone::Green => text.green().to_string(),
        Tone::Red => text.red().to_string(),
        Tone::Yellow => text.yellow().to_string(),
        Tone::Dim => text.dimmed().to_string(),
    }
}

/// Print a line of report output to stdout.
#[expect(
    clippy::print_stdout,
    reason = "the report's primary output goes to stdout"
)]
fn print_line(line: &str) {
    println!("{line}");
}

/// Print a warning to stderr.
#[expect(
    clippy::print_stderr,
    reason = "warnings about unreadable records go to stderr"
)]
fn warn_line(line: &str) {
    eprintln!("warning: {line}");
}

/// The reporter command-line.
#[derive(clap::Parser, Debug)]
#[clap(
    name = "sl-conformance-report",
    about = "Summarise conformance test records",
    author = clap::crate_authors!(),
    version = clap::crate_version!(),
)]
struct Options {
    /// Restrict the report to one grid.
    #[clap(long, value_enum)]
    grid: Option<Grid>,
    /// The records directory (defaults to `<repo root>/records`).
    #[clap(long)]
    records: Option<PathBuf>,
    /// When to colourise output.
    #[clap(long, value_enum, default_value = "auto")]
    color: ColorChoice,
}

/// A reporter error.
#[derive(Debug, thiserror::Error)]
enum Error {
    /// The records directory could not be located.
    #[error("could not locate records directory: {0}")]
    Records(String),
}
