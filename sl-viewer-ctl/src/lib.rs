//! `sl-viewer-ctl`: the viewer automation driver, from the shell — for a
//! developer poking at a live viewer, and for an agent that would otherwise
//! ask a person to "log in and check".
//!
//! - **Reach a viewer** three ways. `launch` starts a headless viewer (the
//!   real binary, confined to a run directory, behind an automation socket)
//!   logged into a grid; `stage` starts a fresh fake grid and the viewers a
//!   small TOML file names ([`stage`]); both print each viewer's socket once
//!   it has settled and hold it until Ctrl-C, then log it out. `attach`
//!   connects to a socket and runs commands from standard input over the one
//!   connection.
//! - **Verbs** drive the viewer at `--socket` (or `SL_VIEWER_SOCKET`, or the
//!   one socket that answers in the viewer's default directory): `tree`,
//!   `find`, `click`, `drag`, `fill`, `press`, `wait`, `open`, `menu`,
//!   `world find`, `world touch`, `chat`, `notifications`, `agent`,
//!   `environment`, `screenshot`, `events --follow`. Each is one
//!   `sl-viewer-driver` call, so every action waits in the viewer for its
//!   node to be actionable and every failure explains itself — with its
//!   screenshot, tree and event tail saved under `--artifacts`.
//! - **Selectors** are `sl-automation-proto`'s string grammar
//!   (`window[test_id=floater:build] >> button[name_key=build-apply]`), or the
//!   locator's JSON.
//! - **Output** is text for a person, or one JSON document per result with
//!   `--json`. The exit status is non-zero when a command failed.

mod attach;
pub mod cli;
mod error;
mod launch;
pub mod output;
pub mod stage;
mod target;
mod verbs;

use std::io::Write;

use sl_viewer_driver::Viewer;
use tokio::io::BufReader;

use crate::cli::{Cli, Command};
pub use crate::error::CtlError;
use crate::output::Printer;

/// Run `cli`, printing results through `printer`; answers whether every
/// command succeeded (a failed `attach` line makes it `false`).
///
/// # Errors
///
/// The [`CtlError`] of the command that failed.
pub async fn run<W: Write>(cli: &Cli, printer: &mut Printer<W>) -> Result<bool, CtlError> {
    let global = &cli.global;
    match &cli.command {
        Command::Launch(args) => launch::launch(args, global, printer).await.map(|()| true),
        Command::Stage { file } => stage::stage(file, global, printer).await.map(|()| true),
        Command::Attach { socket } => {
            let socket = target::resolve(socket.as_deref().or(global.socket.as_deref()))?;
            let viewer =
                Viewer::connect(&socket, launch::driver_options(&label(&socket), global)).await?;
            let stdin = BufReader::new(tokio::io::stdin());
            let failures = attach::session(&viewer, stdin, printer).await?;
            Ok(failures == 0)
        }
        Command::Verb(verb) => {
            let socket = target::resolve(global.socket.as_deref())?;
            let viewer =
                Viewer::connect(&socket, launch::driver_options(&label(&socket), global)).await?;
            let stop = async {
                drop(tokio::signal::ctrl_c().await);
            };
            verbs::run(&viewer, verb, printer, stop)
                .await
                .map(|()| true)
        }
    }
}

/// What a viewer reached through `socket` is called in errors and artifact
/// paths: the socket's file name without its extension (`ctl-4242-alice`,
/// `automation-4242`).
fn label(socket: &std::path::Path) -> String {
    socket.file_stem().map_or_else(
        || socket.display().to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests;
