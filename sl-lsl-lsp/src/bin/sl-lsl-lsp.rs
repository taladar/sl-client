//! The standalone **`sl-lsl-lsp`** language-server binary: an LSL LSP server an
//! external editor (nvim, VS Code, Helix, …) spawns and talks to over stdio.
//!
//! It is the thinnest possible wrapper around [`sl_lsl_lsp::run`]: set up logging
//! *to stderr* (stdout is the LSP transport and must carry only protocol
//! messages), open the stdio [`Connection`], run the server to completion, and
//! join the transport threads.
//!
//! This standalone process has **no grid connection**, so it starts with an
//! empty library table ([`LslSyntax::default`]): document synchronisation and
//! the symbol outline (which read only the user's own parse tree) work fully
//! without it. The grid-backed intelligence — completion and hover against the
//! `ll*` library — arrives when the server is instead run *embedded* in the
//! viewer, which fetches the `LSLSyntax` capability and hands the table in.

use lsp_server::Connection;
use sl_lsl::LslSyntax;
use tracing_subscriber::EnvFilter;

/// Start the server on stdio and run it until the editor shuts it down.
///
/// # Errors
///
/// Returns an error if the LSP handshake or message loop fails, or if the stdio
/// transport threads cannot be joined.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    tracing::info!("sl-lsl-lsp starting on stdio");

    let (connection, io_threads) = Connection::stdio();
    // No grid is connected to a standalone process; start with an empty library.
    sl_lsl_lsp::run(connection, LslSyntax::default())?;
    io_threads.join()?;

    tracing::info!("sl-lsl-lsp shut down");
    Ok(())
}

/// Install a stderr tracing subscriber honouring `RUST_LOG` (defaulting to
/// `info`), so diagnostics never contaminate the stdout LSP channel.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
