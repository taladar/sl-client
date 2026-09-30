//! `sl-viewer-ctl attach`: one connection, and the commands a person or a
//! script types on standard input, one per line, run over it in turn.

use std::io::Write;

use clap::Parser as _;
use clap::error::ErrorKind;
use sl_viewer_driver::{DriverError, Viewer};
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _};

use crate::cli::Line;
use crate::error::CtlError;
use crate::output::{Outcome, Printer};
use crate::verbs;

/// Run the commands `input` holds against `viewer`, one per line, printing
/// each one's results — or its error — through `printer`, until `quit`,
/// `exit` or the end of the input. Blank lines and `#` comments are skipped;
/// a line is split into words the way a shell splits them.
///
/// Answers how many lines failed. The connection closing ends the session
/// (and counts as a failure).
///
/// # Errors
///
/// [`CtlError::Output`] when printing fails or the input cannot be read.
pub(crate) async fn session<W: Write>(
    viewer: &Viewer,
    input: impl AsyncBufRead + Unpin,
    printer: &mut Printer<W>,
) -> Result<usize, CtlError> {
    printer.print(&Outcome::Identity(viewer.identity().clone()))?;
    let mut failures = 0_usize;
    let mut lines = input.lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if matches!(line, "quit" | "exit") {
            break;
        }
        let Some(words) = shlex::split(line) else {
            printer.error(&format!("{line}: unbalanced quotes"))?;
            failures = failures.saturating_add(1);
            continue;
        };
        let verb = match Line::try_parse_from(words) {
            Ok(Line { verb }) => verb,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
            {
                printer.note(&error.render().to_string())?;
                continue;
            }
            Err(error) => {
                printer.error(error.render().to_string().trim_end())?;
                failures = failures.saturating_add(1);
                continue;
            }
        };
        let stop = async {
            drop(tokio::signal::ctrl_c().await);
        };
        match verbs::run(viewer, &verb, printer, stop).await {
            Ok(()) => {}
            Err(error) => {
                printer.error(&error.to_string())?;
                failures = failures.saturating_add(1);
                if matches!(error, CtlError::Driver(DriverError::Closed { .. })) {
                    break;
                }
            }
        }
    }
    Ok(failures)
}
