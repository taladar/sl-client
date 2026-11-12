//! Plays LSL content cases on an in-process fake grid and reports what held.
//!
//! ```text
//! cargo run --release -p sl-content-test -- sl-content-test/cases
//! ```
//!
//! Each case file is played on each grid it names, one run at a time, and gets
//! one `PASS` or `FAIL` line, with what went wrong indented beneath it. The
//! exit status is 0 when every case passed, 1 when one failed, and 2 when a
//! case could not be read or played at all.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use sl_content_test::verdict::{grid_name, transcript};
use sl_content_test::{Case, check, discover};

/// Plays LSL content cases on an in-process fake grid and reports what held.
///
/// A case passing here can still fail on Second Life: the fake grid imitates
/// it, and physics, vehicles and timing under load are approximate (book
/// chapter `tools/content-test.md`).
#[derive(Debug, Parser)]
#[command(version, about)]
struct Options {
    /// Case files, or directories searched for `case.toml` and
    /// `*.case.toml`.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Print everything each run observed: the region's chat, the run-time
    /// errors, and each avatar's transcript and what it was told.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_error| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let options = Options::parse();
    let files = match discover(&options.paths) {
        Ok(files) if files.is_empty() => {
            report_error(&format!("no case files under {:?}", options.paths));
            return ExitCode::from(2);
        }
        Ok(files) => files,
        Err(error) => {
            report_error(&format!("reading the case directories: {error}"));
            return ExitCode::from(2);
        }
    };
    let (mut passed, mut failed, mut broken) = (0_u32, 0_u32, 0_u32);
    for file in files {
        let case = match Case::load(&file) {
            Ok(case) => case,
            Err(error) => {
                print(&format!("ERROR {}\n    {error}\n", file.display()));
                broken = broken.saturating_add(1);
                continue;
            }
        };
        match check(&case).await {
            Ok(verdicts) => {
                for (verdict, observed) in verdicts {
                    print(&verdict.to_string());
                    if options.verbose {
                        print(&format!("{}\n", transcript(&observed)));
                    }
                    if verdict.passed() {
                        passed = passed.saturating_add(1);
                    } else {
                        failed = failed.saturating_add(1);
                    }
                }
            }
            Err(error) => {
                let grids: Vec<&str> = case.grids.iter().map(|grid| grid_name(*grid)).collect();
                print(&format!(
                    "ERROR {} [{}]\n    {error}\n",
                    case.name,
                    grids.join(", ")
                ));
                broken = broken.saturating_add(1);
            }
        }
    }
    print(&format!(
        "{passed} passed, {failed} failed, {broken} could not be played\n"
    ));
    if broken > 0 {
        ExitCode::from(2)
    } else if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Writes report output to stdout.
#[expect(
    clippy::print_stdout,
    reason = "the report is the binary's output, and goes to stdout"
)]
fn print(text: &str) {
    print!("{text}");
}

/// Writes why no case could be looked for to stderr.
#[expect(
    clippy::print_stderr,
    reason = "a failure to find the cases is not report output"
)]
fn report_error(text: &str) {
    eprintln!("error: {text}");
}
