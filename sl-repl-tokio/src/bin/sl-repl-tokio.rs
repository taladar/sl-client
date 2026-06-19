#![doc = include_str!("../../README.md")]

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, ExternalPrinter};
use sl_client_tokio::{Client, Command, Event, LoginParams, LoginRequest};
use sl_proto::Diagnostic;
use sl_repl::{
    Avatar, Credentials, MetaCommand, ReplAction, ScriptRecorder, SessionContext, format_command,
    format_diagnostic, format_event, parse_line, smoke_battery,
};
use tokio::sync::{mpsc, oneshot};
use tracing_subscriber::{
    EnvFilter, Layer as _, Registry, filter::LevelFilter, layer::SubscriberExt as _,
    util::SubscriberInitExt as _,
};

/// The local OpenSim grid login URI used when none is otherwise resolved.
const DEFAULT_LOGIN_URI: &str = "http://127.0.0.1:9000/";

/// An error from the REPL binary.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// A command-line parsing error.
    #[error("error in CLI option parsing: {0}")]
    Clap(
        #[source]
        #[from]
        clap::Error,
    ),
    /// A credentials-file or MFA-acquisition error.
    #[error("authentication error: {0}")]
    Auth(
        #[source]
        #[from]
        sl_repl::AuthError,
    ),
    /// An error from the underlying tokio client.
    #[error("client error: {0}")]
    Client(
        #[source]
        #[from]
        sl_client_tokio::Error,
    ),
    /// An I/O error (reading a script, opening the log or recorder).
    #[error("I/O error: {0}")]
    Io(
        #[source]
        #[from]
        std::io::Error,
    ),
    /// A log-filter directive could not be parsed.
    #[error("error parsing log filter: {0}")]
    LogFilterParse(
        #[source]
        #[from]
        tracing_subscriber::filter::ParseError,
    ),
    /// The client run task could not be joined.
    #[error("error joining the client task: {0}")]
    Join(
        #[source]
        #[from]
        tokio::task::JoinError,
    ),
    /// A grid nickname could not be mapped to a login URI.
    #[error("unknown grid `{0}`; pass --login-uri explicitly")]
    UnknownGrid(String),
    /// The grid issued an MFA challenge but the avatar has no `mfa_command`.
    #[error("the grid requires multi-factor authentication but no mfa_command is configured")]
    MfaRequired,
    /// The interactive line editor could not be initialized.
    #[error("could not initialize the interactive line editor")]
    LineEditor,
    /// The man page could not be generated.
    #[error("error generating man page: {0}")]
    GenerateManpage(#[source] std::io::Error),
    /// The shell completion could not be generated.
    #[error("error generating shell completion: {0}")]
    GenerateShellCompletion(#[source] std::io::Error),
}

/// The parameters of an interactive REPL session (the default, sub-command-less
/// invocation).
#[derive(clap::Args, Debug, Clone)]
pub struct RunArgs {
    /// The TOML credentials file.
    #[clap(long, default_value = "credentials.toml", env = "SL_REPL_CREDENTIALS")]
    credentials: PathBuf,
    /// Which avatar in the credentials file to log in as (defaults to the file's
    /// `default_avatar`, or its sole avatar).
    #[clap(long)]
    avatar: Option<String>,
    /// A grid nickname (`agni`/`aditi`/`localhost`) to log in to.
    #[clap(long)]
    grid: Option<String>,
    /// An explicit XML-RPC login URI, overriding `--grid` and the avatar's own.
    #[clap(long)]
    login_uri: Option<String>,
    /// The login start location (`last`, `home`, or `uri:Region&x&y&z`).
    #[clap(long, default_value = "last")]
    start: String,
    /// The viewer channel reported to the grid.
    #[clap(long, default_value = "sl-repl-tokio")]
    channel: String,
    /// The viewer version reported to the grid.
    #[clap(long, default_value = clap::crate_version!())]
    version: String,
    /// Replay a `.repl` script instead of reading interactively.
    #[clap(long)]
    script: Option<PathBuf>,
    /// Fire the read-only smoke battery once the region handshake lands.
    #[clap(long)]
    smoke: bool,
    /// The always-on trace log file.
    #[clap(long, default_value = "sl-repl-tokio.log")]
    log_file: PathBuf,
    /// Record the interactive session to a replayable `.repl` transcript.
    #[clap(long)]
    script_out: Option<PathBuf>,
}

/// The packaging sub-commands (the absence of a sub-command runs a session).
#[derive(clap::Subcommand, Debug)]
pub enum Subcommand {
    /// Generate the man page.
    GenerateManpage {
        /// The target directory for man-page generation.
        #[clap(long)]
        output_dir: PathBuf,
    },
    /// Generate shell completion.
    GenerateShellCompletion {
        /// The output file for the completion script.
        #[clap(long)]
        output_file: PathBuf,
        /// Which shell to generate completion for.
        #[clap(long)]
        shell: clap_complete::aot::Shell,
    },
}

/// The top-level command-line options.
#[derive(clap::Parser, Debug)]
#[clap(
    name = "sl-repl-tokio",
    about = clap::crate_description!(),
    author = clap::crate_authors!(),
    version = clap::crate_version!(),
    disable_version_flag = true,
)]
struct Options {
    /// The session parameters (used when no sub-command is given).
    #[clap(flatten)]
    run: RunArgs,
    /// The packaging sub-command, if any.
    #[clap(subcommand)]
    command: Option<Subcommand>,
}

/// Where the interactive REPL gets its input lines.
#[derive(Debug)]
enum InputMode {
    /// Read interactively from a terminal via the line editor.
    Interactive,
    /// Replay a `.repl` script file (honouring its `sleep` directives).
    Script(PathBuf),
    /// Replay lines piped on standard input (a non-terminal stdin).
    Stdin,
}

/// A [`tracing_subscriber`] writer that routes formatted log lines through
/// rustyline's [`ExternalPrinter`] so they never corrupt the interactive prompt.
#[derive(Clone)]
struct PrinterWriter {
    /// The shared, locked external printer the log lines are written to.
    printer: Arc<Mutex<Box<dyn ExternalPrinter + Send>>>,
}

impl PrinterWriter {
    /// Wrap an external printer as a cloneable, thread-safe log writer.
    fn new(printer: Box<dyn ExternalPrinter + Send>) -> Self {
        Self {
            printer: Arc::new(Mutex::new(printer)),
        }
    }
}

/// The per-event writer handed out by [`PrinterWriter`]: it buffers a single
/// formatted log record and emits it through the external printer on flush.
struct PrinterGuard {
    /// The shared external printer.
    printer: Arc<Mutex<Box<dyn ExternalPrinter + Send>>>,
    /// The bytes of the in-progress log record.
    buffer: Vec<u8>,
}

impl std::io::Write for PrinterGuard {
    /// Buffer the bytes of the formatted record.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Emit the buffered record (one trailing newline trimmed) through the
    /// external printer, which redraws the prompt cleanly beneath it.
    fn flush(&mut self) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buffer).trim_end().to_owned();
        self.buffer.clear();
        if !text.is_empty()
            && let Ok(mut printer) = self.printer.lock()
        {
            let _printed = printer.print(text);
        }
        Ok(())
    }
}

impl Drop for PrinterGuard {
    /// Flush any buffered record when the writer is dropped.
    fn drop(&mut self) {
        let _flushed = std::io::Write::flush(self);
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for PrinterWriter {
    type Writer = PrinterGuard;

    /// Hand out a fresh buffering writer for one log record.
    fn make_writer(&'writer self) -> Self::Writer {
        PrinterGuard {
            printer: Arc::clone(&self.printer),
            buffer: Vec::new(),
        }
    }
}

/// The terminal log destination chosen for a run.
enum TerminalSink {
    /// The interactive prompt-safe external printer.
    Printer(PrinterWriter),
    /// Plain standard error (replay modes, with no prompt to protect).
    Stderr,
}

/// Map a grid nickname to its XML-RPC login URI, or `None` if unknown.
fn grid_login_uri(grid: &str) -> Option<&'static str> {
    match grid.to_ascii_lowercase().as_str() {
        "agni" | "secondlife" | "sl" => Some("https://login.agni.lindenlab.com/cgi-bin/login.cgi"),
        "aditi" | "beta" => Some("https://login.aditi.lindenlab.com/cgi-bin/login.cgi"),
        "localhost" | "local" | "opensim" => Some(DEFAULT_LOGIN_URI),
        _other => None,
    }
}

/// Resolve the login URI from (in priority order) the explicit `--login-uri`,
/// `--grid`, the avatar's own `login_uri`/`grid`, and finally the local default.
///
/// # Errors
///
/// Returns [`Error::UnknownGrid`] if a grid nickname has no known login URI.
fn resolve_login_uri(args: &RunArgs, avatar: &Avatar) -> Result<String, Error> {
    if let Some(uri) = &args.login_uri {
        return Ok(uri.clone());
    }
    if let Some(grid) = &args.grid {
        return grid_login_uri(grid)
            .map(str::to_owned)
            .ok_or_else(|| Error::UnknownGrid(grid.clone()));
    }
    if let Some(uri) = avatar.login_uri() {
        return Ok(uri.to_owned());
    }
    if let Some(grid) = avatar.grid() {
        return grid_login_uri(grid)
            .map(str::to_owned)
            .ok_or_else(|| Error::UnknownGrid(grid.to_owned()));
    }
    Ok(DEFAULT_LOGIN_URI.to_owned())
}

/// Decide where the REPL reads its input lines from.
fn input_mode(args: &RunArgs) -> InputMode {
    if let Some(path) = &args.script {
        return InputMode::Script(path.clone());
    }
    if std::io::stdin().is_terminal() {
        InputMode::Interactive
    } else {
        InputMode::Stdin
    }
}

/// Read the replay lines for a non-interactive mode, or `None` when interactive.
///
/// # Errors
///
/// Returns [`Error::Io`] if the script file or standard input cannot be read.
fn replay_lines(mode: &InputMode) -> Result<Option<Vec<String>>, Error> {
    let text = match mode {
        InputMode::Interactive => return Ok(None),
        InputMode::Script(path) => std::fs::read_to_string(path)?,
        InputMode::Stdin => std::io::read_to_string(std::io::stdin())?,
    };
    Ok(Some(text.lines().map(str::to_owned).collect()))
}

/// Spawn the blocking line-editor thread: it creates the editor, hands its
/// external printer back over `printer_tx`, then forwards each entered line over
/// `line_tx` until end-of-file.
fn spawn_line_editor(
    line_tx: mpsc::Sender<String>,
    printer_tx: oneshot::Sender<Box<dyn ExternalPrinter + Send>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut editor = match DefaultEditor::new() {
            Ok(editor) => editor,
            Err(error) => {
                tracing::error!("could not create the line editor: {error}");
                return;
            }
        };
        match editor.create_external_printer() {
            Ok(printer) => {
                let boxed: Box<dyn ExternalPrinter + Send> = Box::new(printer);
                if printer_tx.send(boxed).is_err() {
                    return;
                }
            }
            Err(error) => {
                tracing::error!("could not create the external printer: {error}");
                return;
            }
        }
        loop {
            match editor.readline("> ") {
                Ok(line) => {
                    let _added = editor.add_history_entry(line.as_str());
                    if line_tx.blocking_send(line).is_err() {
                        break;
                    }
                }
                Err(ReadlineError::Interrupted) => {}
                Err(ReadlineError::Eof) => break,
                Err(error) => {
                    tracing::warn!("line editor error: {error}");
                    break;
                }
            }
        }
    })
}

/// Feed replay `lines` to the session, sleeping on each `sleep` directive so the
/// replay reproduces the original pacing.
async fn feed_replay(lines: Vec<String>, line_tx: mpsc::Sender<String>) {
    for raw in lines {
        if let Ok(Some(ReplAction::Meta(MetaCommand::Sleep(duration)))) = parse_line(&raw) {
            tokio::time::sleep(duration).await;
            continue;
        }
        if line_tx.send(raw).await.is_err() {
            break;
        }
    }
}

/// Build the terminal and file [`EnvFilter`]s (terminal default `info`, file
/// default `trace`; override via `RUST_LOG` / `SL_REPL_LOG`).
///
/// # Errors
///
/// Returns [`Error::LogFilterParse`] if an environment directive is malformed.
fn build_filters() -> Result<(EnvFilter, EnvFilter), Error> {
    let terminal = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse(std::env::var("RUST_LOG").unwrap_or_else(|_ignored| "info".to_owned()))?;
    let file = EnvFilter::builder()
        .with_default_directive(LevelFilter::TRACE.into())
        .parse(std::env::var("SL_REPL_LOG").unwrap_or_else(|_ignored| "trace".to_owned()))?;
    Ok((terminal, file))
}

/// Initialize the always-on file log (`trace`) plus the chosen terminal layer.
///
/// # Errors
///
/// Returns [`Error::LogFilterParse`] if a log-filter directive is malformed.
fn init_logging(log_file: &Path, terminal: TerminalSink) -> Result<(), Error> {
    let (terminal_filter, file_filter) = build_filters()?;
    let directory = log_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let file_name = log_file
        .file_name()
        .map_or_else(|| OsString::from("sl-repl-tokio.log"), OsString::from);
    let appender = tracing_appender::rolling::never(directory, file_name);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(appender)
        .with_filter(file_filter);
    let registry = Registry::default().with(file_layer);
    match terminal {
        TerminalSink::Printer(writer) => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer)
                    .with_filter(terminal_filter),
            )
            .init(),
        TerminalSink::Stderr => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(std::io::stderr)
                    .with_filter(terminal_filter),
            )
            .init(),
    }
    Ok(())
}

/// Connect to the grid, answering an MFA challenge via the avatar's
/// `mfa_command` and retrying the login with the acquired token.
///
/// # Errors
///
/// Returns [`Error::Client`] on a login failure, [`Error::Auth`] if acquiring an
/// MFA token fails, or [`Error::MfaRequired`] if the grid challenges but the
/// avatar has no `mfa_command`.
async fn connect_with_mfa(
    login_uri: &str,
    mut request: LoginRequest,
    avatar: &Avatar,
) -> Result<Client, Error> {
    loop {
        let params = LoginParams {
            login_uri: login_uri.to_owned(),
            request: request.clone(),
        };
        match Client::connect(params).await {
            Ok(client) => return Ok(client),
            Err(sl_client_tokio::Error::MfaChallenge(challenge)) => {
                tracing::info!(
                    "multi-factor authentication required: {}",
                    challenge.message
                );
                let token = avatar.acquire_mfa()?.ok_or(Error::MfaRequired)?;
                request = request.with_mfa(token.expose(), challenge.mfa_hash);
            }
            Err(other) => return Err(Error::Client(other)),
        }
    }
}

/// Apply a parsed REPL meta command to the placeholder context.
fn apply_meta(meta: MetaCommand, ctx: &mut SessionContext) {
    match meta {
        MetaCommand::Comment(_) | MetaCommand::Sleep(_) => {}
        MetaCommand::Set { name, value } => ctx.set_var(&name, &value),
        MetaCommand::Unset(name) => {
            let _removed = ctx.unset_var(&name);
        }
        MetaCommand::Vars => {
            for (name, value) in ctx.vars() {
                tracing::info!("var ${name} = {value}");
            }
        }
    }
}

/// Handle one input line: a meta command updates the context, a grid command is
/// resolved against the context and dispatched, and anything malformed is
/// logged.
async fn handle_input(raw: &str, ctx: &mut SessionContext, command_tx: &mpsc::Sender<Command>) {
    match parse_line(raw) {
        Ok(None) => {}
        Ok(Some(ReplAction::Meta(meta))) => apply_meta(meta, ctx),
        Ok(Some(ReplAction::Command(pending))) => match pending.resolve(&*ctx) {
            Ok(command) => {
                tracing::info!("{}", format_command(&command, &*ctx));
                command_tx.send(command).await.ok();
            }
            Err(error) => tracing::warn!("could not build command: {error}"),
        },
        Err(error) => tracing::warn!("could not parse line: {error}"),
    }
}

/// Run one interactive (or replayed) REPL session end-to-end.
///
/// # Errors
///
/// Returns an [`enum@Error`] if credentials cannot be loaded, login fails, the
/// log cannot be initialized, or the client task errors.
async fn run_repl(args: RunArgs) -> Result<(), Error> {
    let credentials = Credentials::load(&args.credentials)?;
    let avatar = credentials.select(args.avatar.as_deref())?;
    let login_uri = resolve_login_uri(&args, avatar)?;
    let mode = input_mode(&args);
    let interactive = matches!(mode, InputMode::Interactive);

    let (line_tx, mut line_rx) = mpsc::channel::<String>(64);
    if interactive {
        let (printer_tx, printer_rx) = oneshot::channel();
        let _editor = spawn_line_editor(line_tx.clone(), printer_tx);
        let printer = printer_rx.await.map_err(|_ignored| Error::LineEditor)?;
        init_logging(
            &args.log_file,
            TerminalSink::Printer(PrinterWriter::new(printer)),
        )?;
    } else {
        init_logging(&args.log_file, TerminalSink::Stderr)?;
    }
    if let Some(lines) = replay_lines(&mode)? {
        let line_tx = line_tx.clone();
        let _feeder = tokio::spawn(feed_replay(lines, line_tx));
    }
    drop(line_tx);

    tracing::info!(
        "logging in as {} {} to {login_uri}",
        avatar.first(),
        avatar.last()
    );
    let request = LoginRequest::new(
        avatar.first().to_owned(),
        avatar.last().to_owned(),
        avatar.password().expose().to_owned(),
        args.start.clone(),
        args.channel.clone(),
        args.version.clone(),
    );
    let mut client = connect_with_mfa(&login_uri, request, avatar).await?;
    tracing::info!("login succeeded");
    client.set_diagnostics(true);
    let (caps_tx, mut caps_rx) = mpsc::channel::<HashMap<String, String>>(8);
    client.set_caps_reporter(caps_tx);

    let mut ctx = SessionContext::new();
    if let (Some(agent), Some(session), Some(circuit)) = (
        client.agent_id(),
        client.session_id(),
        client.circuit_code(),
    ) {
        ctx.set_identity(agent, session, circuit);
    }
    if let Some(seed) = client.seed_capability() {
        ctx.set_cap("Seed", seed);
    }
    let self_agent = client.agent_id();

    let mut recorder = match (&args.script_out, interactive) {
        (Some(path), true) => {
            let mut recorder = ScriptRecorder::create(path)?;
            recorder.comment(&format!("grid {login_uri}")).ok();
            recorder
                .comment(&format!("avatar {} {}", avatar.first(), avatar.last()))
                .ok();
            Some(recorder)
        }
        _other => None,
    };

    let (event_tx, mut event_rx) = mpsc::channel::<Event>(256);
    let (diag_tx, mut diag_rx) = mpsc::channel::<Diagnostic>(64);
    let (command_tx, command_rx) = mpsc::channel::<Command>(64);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));

    let mut input_open = true;
    let mut smoke_fired = false;
    loop {
        tokio::select! {
            maybe_event = event_rx.recv() => {
                let Some(event) = maybe_event else { break; };
                ctx.apply_event(&event);
                tracing::info!("{}", format_event(&event, &ctx));
                match event {
                    Event::RegionHandshakeComplete => {
                        if args.smoke && !smoke_fired {
                            smoke_fired = true;
                            if let Some(agent) = self_agent {
                                for command in smoke_battery(agent) {
                                    command_tx.send(command).await.ok();
                                }
                            }
                        }
                    }
                    Event::LoggedOut | Event::Disconnected(_) => break,
                    _other => {}
                }
            }
            maybe_diag = diag_rx.recv() => {
                if let Some(diagnostic) = maybe_diag {
                    tracing::warn!("{}", format_diagnostic(&diagnostic));
                }
            }
            maybe_caps = caps_rx.recv() => {
                if let Some(caps) = maybe_caps {
                    ctx.set_caps(caps.into_iter().collect());
                }
            }
            maybe_line = line_rx.recv(), if input_open => {
                match maybe_line {
                    Some(raw) => {
                        if let Some(recorder) = recorder.as_mut()
                            && let Err(error) = recorder.record(&raw, Instant::now())
                        {
                            tracing::warn!("could not record line: {error}");
                        }
                        handle_input(&raw, &mut ctx, &command_tx).await;
                    }
                    None => {
                        input_open = false;
                        command_tx.send(Command::Logout).await.ok();
                    }
                }
            }
            else => break,
        }
    }

    drop(command_tx);
    run.await??;
    tracing::info!("session ended");
    Ok(())
}

/// Parse the command line and dispatch to a session run or a packaging
/// sub-command.
///
/// # Errors
///
/// Returns an [`enum@Error`] if the selected action fails.
#[tokio::main]
async fn main() -> Result<(), Error> {
    let options = <Options as clap::Parser>::parse();
    match options.command {
        None => run_repl(options.run).await?,
        Some(Subcommand::GenerateManpage { output_dir }) => {
            clap_mangen::generate_to(<Options as clap::CommandFactory>::command(), output_dir)
                .map_err(Error::GenerateManpage)?;
        }
        Some(Subcommand::GenerateShellCompletion { output_file, shell }) => {
            let mut file =
                std::fs::File::create(output_file).map_err(Error::GenerateShellCompletion)?;
            let mut command = <Options as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut command, "sl-repl-tokio", &mut file);
        }
    }
    Ok(())
}
