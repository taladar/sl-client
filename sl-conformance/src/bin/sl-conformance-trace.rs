//! Offline protocol-trace tool.
//!
//! Parses a `.pcap` capture (full LLUDP UDP datagrams) and, optionally, a
//! Firestorm `SecondLife.log` (with `LogMessages = 1`), and emits a single
//! chronological, human-readable timeline of every UDP message exchanged
//! between the viewer and the simulator — parsed with the workspace's own
//! `sl-wire` decoders — so a divergence with `sl-client` can be compared side
//! by side. A parallel JSON-Lines file can be written for programmatic diffing.
//!
//! Direction is taken from the log's `#Messaging#` lines; without a log, pass
//! `--sim-addr` / `--viewer-addr` to identify the endpoints.

use std::path::PathBuf;

use clap::Parser as _;
use sl_conformance::trace::logfile::{self, LogFile};
use sl_conformance::trace::pcap::Capture;
use sl_conformance::trace::timeline::{self, EndpointSpec, Endpoints, Timeline};
use sl_conformance::trace::{TraceError, pcap};

/// Command-line options.
#[derive(Debug, clap::Parser)]
#[clap(name = "sl-conformance-trace", about = clap::crate_description!(), version)]
struct Options {
    /// The `.pcap` / `.pcapng` capture to read.
    #[clap(long)]
    pcap: PathBuf,
    /// A Firestorm `SecondLife.log` (with `LogMessages = 1`) for direction and
    /// viewer-timestamp correlation.
    #[clap(long)]
    log: Option<PathBuf>,
    /// Where to write the human-readable timeline (default: stdout).
    #[clap(long)]
    out: Option<PathBuf>,
    /// Where to write the JSON-Lines timeline (optional).
    #[clap(long)]
    jsonl: Option<PathBuf>,
    /// Treat this `ip` or `ip:port` as the simulator side (repeatable). Needed
    /// only without a `--log`. On a loopback capture, where both sides share an
    /// IP, give the **port** — a bare IP cannot tell the two directions apart.
    #[clap(long = "sim-addr")]
    sim_addr: Vec<EndpointSpec>,
    /// Treat this `ip` or `ip:port` as the viewer side (repeatable). Optional
    /// fallback.
    #[clap(long = "viewer-addr")]
    viewer_addr: Vec<EndpointSpec>,
    /// Also dump raw hex for successfully-decoded messages in the text output.
    #[clap(long)]
    include_raw: bool,
}

/// Errors the binary can fail with.
#[derive(Debug, thiserror::Error)]
enum Error {
    /// A trace-building error.
    #[error(transparent)]
    Trace(#[from] TraceError),
    /// An I/O error writing an output file.
    #[error("writing {path}: {source}")]
    Write {
        /// The path being written.
        path: String,
        /// The underlying error.
        source: std::io::Error,
    },
}

fn main() {
    let options = Options::parse();
    if let Err(error) = run(&options) {
        report_error(&error);
        std::process::exit(1);
    }
}

/// Prints a fatal error to stderr.
#[expect(
    clippy::print_stderr,
    reason = "a CLI binary reports fatal errors on stderr"
)]
fn report_error(error: &Error) {
    eprintln!("error: {error}");
}

/// Reads the inputs, builds the timeline, and writes the outputs.
fn run(options: &Options) -> Result<(), Error> {
    let capture = pcap::read_capture(&options.pcap)?;

    let log = match &options.log {
        Some(path) => logfile::read_log(path)?,
        None => LogFile::default(),
    };
    if options.log.is_some() {
        report_log_health(&log);
    }

    let mut endpoints = Endpoints::default();
    endpoints.sim.sockets.extend(log.sim_hosts.iter().copied());
    for spec in &options.sim_addr {
        endpoints.sim.insert(*spec);
    }
    for spec in &options.viewer_addr {
        endpoints.viewer.insert(*spec);
    }
    if endpoints.sim.is_empty() && endpoints.viewer.is_empty() {
        return Err(Error::Trace(TraceError::NoEndpoints));
    }

    let Capture {
        datagrams,
        stopped_early,
        snaplen_truncated,
        skipped_frames,
    } = capture;
    let total_datagrams = datagrams.len();
    let timeline = timeline::build_timeline(datagrams, &log, &endpoints);

    let text = timeline::render_text(&timeline.entries, options.include_raw);
    match &options.out {
        Some(path) => write_file(path, &text)?,
        None => print_stdout(&text),
    }
    if let Some(path) = &options.jsonl {
        let jsonl = timeline::render_jsonl(&timeline.entries)?;
        write_file(path, &jsonl)?;
    }

    report_summary(
        &CaptureHealth {
            total_datagrams,
            stopped_early,
            snaplen_truncated,
            skipped_frames,
        },
        &timeline,
    );
    Ok(())
}

/// Writes `contents` to `path`, mapping I/O errors to [`Error::Write`].
fn write_file(path: &std::path::Path, contents: &str) -> Result<(), Error> {
    fs_err::write(path, contents).map_err(|source| Error::Write {
        path: path.display().to_string(),
        source,
    })
}

/// Writes the text timeline to stdout.
#[expect(
    clippy::print_stdout,
    reason = "the text timeline goes to stdout when no --out is given"
)]
fn print_stdout(text: &str) {
    print!("{text}");
}

/// What reading the capture cost, for the run summary.
struct CaptureHealth {
    /// How many UDP datagrams were recovered in all.
    total_datagrams: usize,
    /// The error that ended the read early, if the file was truncated.
    stopped_early: Option<String>,
    /// How many recovered datagrams were snaplen-truncated.
    snaplen_truncated: usize,
    /// How many frames were skipped whole.
    skipped_frames: usize,
}

/// Prints a run summary to stderr, including everything that was **not** in the
/// timeline: a capture that stopped early, frames skipped, datagrams cut short
/// by the snaplen, and datagrams whose direction could not be told.
#[expect(
    clippy::print_stderr,
    reason = "a CLI binary reports its run summary on stderr"
)]
fn report_summary(capture: &CaptureHealth, timeline: &Timeline) {
    let entries = timeline.entries.len();
    let errors = timeline::error_count(&timeline.entries);
    let truncated = timeline::truncated_count(&timeline.entries);
    eprintln!(
        "traced {entries} UDP message(s) of {} datagram(s): {errors} parse error(s), \
         {truncated} snaplen-truncated, {} non-circuit dropped",
        capture.total_datagrams, timeline.unlabelled
    );
    if timeline.ambiguous > 0 {
        eprintln!(
            "warning: {} datagram(s) dropped with an undecidable direction — \
             both ends match a known side equally well; pass --sim-addr with \
             the simulator's port (e.g. 127.0.0.1:9000)",
            timeline.ambiguous
        );
    }
    if capture.skipped_frames > 0 {
        eprintln!(
            "warning: {} capture frame(s) skipped (unsupported link type or \
             unrepresentable timestamp)",
            capture.skipped_frames
        );
    }
    if capture.snaplen_truncated > 0 {
        eprintln!(
            "warning: {} datagram(s) were cut short by the capture snaplen — \
             their bodies are incomplete, so a decode failure there is the \
             capture, not a protocol divergence",
            capture.snaplen_truncated
        );
    }
    if let Some(error) = &capture.stopped_early {
        eprintln!("warning: the capture ended mid-record and was read only that far: {error}");
    }
}

/// Warns if the log yielded nothing, or if lines that looked like
/// `#Messaging#` lines could not be parsed — otherwise a log whose format has
/// drifted silently produces a timeline with no viewer timestamps at all and
/// nothing saying why.
#[expect(
    clippy::print_stderr,
    reason = "a CLI binary reports input problems on stderr"
)]
fn report_log_health(log: &LogFile) {
    if log.messages.is_empty() {
        eprintln!(
            "warning: the log contained no #Messaging# lines — was it captured \
             with the LogMessages debug setting enabled? Direction and viewer \
             timestamps will be missing"
        );
    }
    if log.skipped_lines > 0 {
        eprintln!(
            "warning: {} log line(s) looked like #Messaging# lines but did not \
             parse; their viewer timestamps are missing from the timeline",
            log.skipped_lines
        );
    }
}
