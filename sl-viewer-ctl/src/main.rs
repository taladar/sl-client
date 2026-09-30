//! `sl-viewer-ctl`: drive a running Second Life viewer from the shell. See
//! the library's documentation, or `sl-viewer-ctl --help`.

use std::process::ExitCode;

use clap::Parser as _;
use sl_viewer_ctl::cli::Cli;
use sl_viewer_ctl::output::Printer;

fn main() -> ExitCode {
    // Logs go to standard error, results to standard output: a script reads
    // one and a person watches the other.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_error| {
                tracing_subscriber::EnvFilter::new("warn,sl_viewer_ctl=info,sl_viewer_launch=info")
            }),
        )
        .init();
    let cli = Cli::parse();
    let mut printer = Printer::new(std::io::stdout(), cli.global.json);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!("starting the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(sl_viewer_ctl::run(&cli, &mut printer)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            let mut errors = Printer::new(std::io::stderr(), cli.global.json);
            if let Err(print) = errors.error(&error.to_string()) {
                tracing::error!("{error} (and printing it failed: {print})");
            }
            ExitCode::FAILURE
        }
    }
}
