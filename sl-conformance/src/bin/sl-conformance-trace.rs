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

use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser as _;
use sl_conformance::trace::logfile::{self, LogFile};
use sl_conformance::trace::timeline::{self, Endpoints};
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
    /// Treat this IP as the simulator side (repeatable). Needed only without a
    /// `--log`.
    #[clap(long = "sim-addr")]
    sim_addr: Vec<IpAddr>,
    /// Treat this IP as the viewer side (repeatable). Optional fallback.
    #[clap(long = "viewer-addr")]
    viewer_addr: Vec<IpAddr>,
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
    let datagrams = pcap::read_udp_datagrams(&options.pcap)?;
    let total_datagrams = datagrams.len();

    let log = match &options.log {
        Some(path) => logfile::read_log(path)?,
        None => LogFile::default(),
    };

    let mut endpoints = Endpoints::default();
    endpoints.sim_ips.extend(log.sim_hosts.iter().copied());
    endpoints.sim_ips.extend(options.sim_addr.iter().copied());
    endpoints
        .viewer_ips
        .extend(options.viewer_addr.iter().copied());
    if endpoints.sim_ips.is_empty() && endpoints.viewer_ips.is_empty() {
        return Err(Error::Trace(TraceError::NoEndpoints));
    }

    let entries = timeline::build_timeline(datagrams, &log, &endpoints);
    let errors = timeline::error_count(&entries);

    let text = timeline::render_text(&entries, options.include_raw);
    match &options.out {
        Some(path) => write_file(path, &text)?,
        None => print_stdout(&text),
    }
    if let Some(path) = &options.jsonl {
        let jsonl = timeline::render_jsonl(&entries)?;
        write_file(path, &jsonl)?;
    }

    report_summary(total_datagrams, entries.len(), errors);
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

/// Prints a one-line summary to stderr.
#[expect(
    clippy::print_stderr,
    reason = "a CLI binary reports its run summary on stderr"
)]
fn report_summary(total_datagrams: usize, entries: usize, errors: usize) {
    let dropped = total_datagrams.saturating_sub(entries);
    eprintln!(
        "traced {entries} UDP message(s): {errors} parse error(s), \
         {dropped} non-circuit datagram(s) dropped"
    );
}
