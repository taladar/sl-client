//! The conformance test runner.
//!
//! Runs exactly one test, against one grid, per invocation — there is no
//! "run all" command, by design (logging in many times in quick succession on
//! aditi risks rate-limiting or a ban). The result, git-stamped with the
//! behaviour-aware describe and any metrics the test wrote, is appended to the
//! committed record for that `(test, grid)` pair.

use std::path::{Path, PathBuf};

use clap::Parser as _;
use sl_conformance::context::{self, TestContext, TestFailure};
use sl_conformance::grid::Grid;
use sl_conformance::record::{Outcome, Record, Run};
use sl_conformance::registry::{GridTest, find, registry};
use sl_conformance::{gitinfo, record};
use sl_repl::{Avatar, Credentials};
use tracing_subscriber::Layer as _;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// The viewer channel reported to the grid at login.
const CHANNEL: &str = "sl-conformance";

/// Run the runner, mapping any error to a process exit code.
#[tokio::main]
async fn main() {
    let options = Options::parse();
    if let Err(error) = dispatch(options).await {
        // The logging layer may not be initialised yet; report on stderr.
        report_error(&error);
        std::process::exit(1);
    }
}

/// Print a fatal error to stderr.
#[expect(
    clippy::print_stderr,
    reason = "a CLI binary reports fatal errors to the user on stderr"
)]
fn report_error(error: &Error) {
    eprintln!("error: {error}");
}

/// Print a line of normal output to stdout.
#[expect(
    clippy::print_stdout,
    reason = "a CLI binary writes its primary output to stdout"
)]
fn print_line(line: &str) {
    println!("{line}");
}

/// Dispatch the parsed command.
///
/// # Errors
///
/// Returns an [`enum@Error`] if the chosen subcommand fails.
async fn dispatch(options: Options) -> Result<(), Error> {
    match options.command {
        Subcommand::Run(args) => run(args).await,
        Subcommand::List(args) => {
            list(args.grid);
            Ok(())
        }
        Subcommand::GenerateManpage { output_dir } => generate_manpage(&output_dir),
        Subcommand::GenerateShellCompletion { output_file, shell } => {
            generate_completion(&output_file, shell)
        }
    }
}

/// List the registered tests, optionally filtered to one grid.
#[expect(clippy::print_stdout, reason = "a CLI listing writes to stdout")]
fn list(grid: Option<Grid>) {
    for test in registry() {
        if let Some(grid) = grid
            && !test.grids().contains(&grid)
        {
            continue;
        }
        let grids: Vec<&str> = test.grids().iter().map(|grid| grid.dir_name()).collect();
        println!(
            "{name:20} [{grids}] accounts={accounts}  {description}",
            name = test.name(),
            grids = grids.join(","),
            accounts = test.accounts(),
            description = test.description(),
        );
    }
}

/// The stable per-account label used for the record's `avatar` field and the
/// cooldown filename: the avatar's `First Last` identity.
fn avatar_label(avatar: &Avatar) -> String {
    format!("{} {}", avatar.first(), avatar.last())
}

/// Whether two avatars are distinct accounts (by login identity).
fn distinct(left: &Avatar, right: &Avatar) -> bool {
    left.first() != right.first() || left.last() != right.last()
}

/// Resolve the secondary avatar for a two-account test: an explicit
/// `--secondary`, else the conventional `secondary` entry, else the first other
/// distinct avatar in the file.
fn resolve_secondary<'creds>(
    credentials: &'creds Credentials,
    primary: &Avatar,
    explicit: Option<&str>,
) -> Option<&'creds Avatar> {
    if let Some(name) = explicit {
        return credentials.select(Some(name)).ok();
    }
    if let Ok(candidate) = credentials.select(Some("secondary"))
        && distinct(candidate, primary)
    {
        return Some(candidate);
    }
    for name in credentials.avatar_names() {
        if let Ok(candidate) = credentials.select(Some(name))
            && distinct(candidate, primary)
        {
            return Some(candidate);
        }
    }
    None
}

/// Resolve the tertiary avatar for a three-account test: an explicit
/// `--tertiary`, else the conventional `tertiary` entry, else the first avatar
/// in the file distinct from *both* the primary and the secondary.
fn resolve_tertiary<'creds>(
    credentials: &'creds Credentials,
    primary: &Avatar,
    secondary: &Avatar,
    explicit: Option<&str>,
) -> Option<&'creds Avatar> {
    let other = |candidate: &Avatar| distinct(candidate, primary) && distinct(candidate, secondary);
    if let Some(name) = explicit {
        return credentials.select(Some(name)).ok();
    }
    if let Ok(candidate) = credentials.select(Some("tertiary"))
        && other(candidate)
    {
        return Some(candidate);
    }
    for name in credentials.avatar_names() {
        if let Ok(candidate) = credentials.select(Some(name))
            && other(candidate)
        {
            return Some(candidate);
        }
    }
    None
}

/// The default credentials path for a grid when `--credentials` is omitted.
fn default_credentials(grid: Grid) -> PathBuf {
    match grid {
        Grid::Opensim => PathBuf::from("credentials.toml"),
        Grid::Aditi => PathBuf::from("credentials.aditi.toml"),
    }
}

/// Run one test against one grid and record the result.
///
/// # Errors
///
/// Returns an [`enum@Error`] if credentials cannot be loaded, the test is
/// unknown or inapplicable, not enough avatars are configured, a cooldown is
/// active, or the record cannot be written.
async fn run(args: RunArgs) -> Result<(), Error> {
    let repo_root = gitinfo::repo_root(Path::new(".")).map_err(Error::Git)?;
    init_logging(&repo_root.join("sl-conformance.log"))?;

    let test = find(&args.test).ok_or_else(|| Error::UnknownTest(args.test.clone()))?;
    if !test.grids().contains(&args.grid) {
        return Err(Error::NotApplicable {
            test: test.name().to_owned(),
            grid: args.grid,
        });
    }

    let credentials_path = args
        .credentials
        .clone()
        .unwrap_or_else(|| default_credentials(args.grid));
    let credentials =
        Credentials::load(&credentials_path).map_err(|error| Error::Auth(error.to_string()))?;
    let primary = credentials
        .select(args.avatar.as_deref())
        .map_err(|error| Error::Auth(error.to_string()))?;

    // Avatar-availability precondition: refuse only when more avatars are needed
    // than the file provides, before any login happens.
    let secondary = if test.accounts() >= 2 {
        let resolved = resolve_secondary(&credentials, primary, args.secondary.as_deref());
        Some(resolved.ok_or_else(|| Error::NotEnoughAvatars {
            test: test.name().to_owned(),
            needed: test.accounts(),
            found: 1,
        })?)
    } else {
        None
    };
    let tertiary = if test.accounts() >= 3 {
        let secondary = secondary.ok_or_else(|| Error::NotEnoughAvatars {
            test: test.name().to_owned(),
            needed: test.accounts(),
            found: 1,
        })?;
        let resolved = resolve_tertiary(&credentials, primary, secondary, args.tertiary.as_deref());
        Some(resolved.ok_or_else(|| Error::NotEnoughAvatars {
            test: test.name().to_owned(),
            needed: test.accounts(),
            found: 2,
        })?)
    } else {
        None
    };

    let state_dir = repo_root.join(".sl-conformance");
    let records_dir = repo_root.join("records");
    let version = clap::crate_version!();

    // Per-avatar aditi cooldown (no guard on the local OpenSim grid).
    if args.grid.needs_cooldown() {
        context::enforce_cooldown(&state_dir, &avatar_label(primary), args.force)
            .map_err(|error| Error::Test(error.to_string()))?;
        if let Some(secondary) = secondary {
            context::enforce_cooldown(&state_dir, &avatar_label(secondary), args.force)
                .map_err(|error| Error::Test(error.to_string()))?;
        }
        if let Some(tertiary) = tertiary {
            context::enforce_cooldown(&state_dir, &avatar_label(tertiary), args.force)
                .map_err(|error| Error::Test(error.to_string()))?;
        }
    }

    tracing::info!(
        "running test `{}` on {} as {}",
        test.name(),
        args.grid,
        avatar_label(primary)
    );
    let primary_session = context::login(args.grid, primary, CHANNEL, version)
        .await
        .map_err(|error| Error::Test(error.to_string()))?;
    let secondary_session = match secondary {
        Some(secondary) => Some(
            context::login(args.grid, secondary, CHANNEL, version)
                .await
                .map_err(|error| Error::Test(error.to_string()))?,
        ),
        None => None,
    };
    let tertiary_session = match tertiary {
        Some(tertiary) => Some(
            context::login(args.grid, tertiary, CHANNEL, version)
                .await
                .map_err(|error| Error::Test(error.to_string()))?,
        ),
        None => None,
    };

    let mut ctx = TestContext::new(
        args.grid,
        primary_session,
        secondary_session,
        tertiary_session,
    );
    let outcome = test.run(&mut ctx).await;
    let (metrics, completeness, completeness_note, primary_sess, secondary_sess, tertiary_sess) =
        ctx.into_parts();

    // Log out cleanly regardless of the test outcome.
    if let Err(error) = primary_sess.logout().await {
        tracing::warn!("primary logout error: {error}");
    }
    if let Some(session) = secondary_sess
        && let Err(error) = session.logout().await
    {
        tracing::warn!("secondary logout error: {error}");
    }
    if let Some(session) = tertiary_sess
        && let Err(error) = session.logout().await
    {
        tracing::warn!("tertiary logout error: {error}");
    }

    write_record(
        &repo_root,
        &records_dir,
        args.grid,
        test.as_ref(),
        &avatar_label(primary),
        outcome,
        metrics,
        completeness,
        completeness_note,
    )
}

/// Build and append the record for a finished run.
#[expect(
    clippy::too_many_arguments,
    reason = "assembling one record from the run's parts; grouping them adds no clarity"
)]
fn write_record(
    repo_root: &Path,
    records_dir: &Path,
    grid: Grid,
    test: &dyn GridTest,
    avatar: &str,
    outcome: Result<(), TestFailure>,
    metrics: sl_conformance::metrics::Metrics,
    completeness: record::Completeness,
    completeness_note: Option<String>,
) -> Result<(), Error> {
    let describe = gitinfo::behavior_describe(repo_root).map_err(Error::Git)?;
    let passed = outcome.is_ok();
    if let Err(error) = &outcome {
        tracing::error!("test failed: {error}");
    }
    let (values, meta) = metrics.into_parts();
    let run = Run {
        behavior_describe: describe.describe_string(),
        dirty: describe.dirty,
        outcome: if passed { Outcome::Pass } else { Outcome::Fail },
        completeness,
        completeness_note,
        recorded_at: context::now_rfc3339().map_err(|error| Error::Test(error.to_string()))?,
        avatar: avatar.to_owned(),
        sl_conformance_version: clap::crate_version!().to_owned(),
        metrics: values,
        metric_meta: meta,
    };
    Record::append(records_dir, grid, test.name(), run).map_err(Error::Record)?;
    print_line(&format!(
        "{}: {} on {} at {}",
        if passed { "PASS" } else { "FAIL" },
        test.name(),
        grid,
        describe.describe_string(),
    ));
    if passed {
        Ok(())
    } else {
        Err(Error::Test(format!("test `{}` failed", test.name())))
    }
}

/// Initialise the always-on file log (trace) plus an env-filtered stderr layer.
///
/// # Errors
///
/// Returns [`Error::Log`] if the subscriber cannot be installed.
fn init_logging(log_file: &Path) -> Result<(), Error> {
    let directory = log_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let file_name = log_file.file_name().map_or_else(
        || std::ffi::OsString::from("sl-conformance.log"),
        std::ffi::OsString::from,
    );
    let appender = tracing_appender::rolling::never(directory, file_name);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(appender)
        .with_filter(tracing_subscriber::EnvFilter::new("trace"));
    let stderr_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_unset| tracing_subscriber::EnvFilter::new("info"));
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .with_filter(stderr_filter);
    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|error| Error::Log(error.to_string()))
}

/// Write the runner's man page into `output_dir`.
///
/// # Errors
///
/// Returns [`Error::Log`] if the page cannot be rendered or written.
fn generate_manpage(output_dir: &Path) -> Result<(), Error> {
    let command = <Options as clap::CommandFactory>::command();
    let man = clap_mangen::Man::new(command);
    let mut buffer: Vec<u8> = Vec::new();
    man.render(&mut buffer)
        .map_err(|error| Error::Log(error.to_string()))?;
    fs_err::create_dir_all(output_dir).map_err(|error| Error::Log(error.to_string()))?;
    fs_err::write(output_dir.join("sl-conformance.1"), buffer)
        .map_err(|error| Error::Log(error.to_string()))
}

/// Write shell completions for `shell` to `output_file`.
///
/// # Errors
///
/// Returns [`Error::Log`] if the file cannot be written.
fn generate_completion(output_file: &Path, shell: clap_complete::aot::Shell) -> Result<(), Error> {
    let mut command = <Options as clap::CommandFactory>::command();
    let mut file =
        fs_err::File::create(output_file).map_err(|error| Error::Log(error.to_string()))?;
    clap_complete::aot::generate(shell, &mut command, "sl-conformance", &mut file);
    Ok(())
}

/// The runner command-line.
#[derive(clap::Parser, Debug)]
#[clap(
    name = "sl-conformance",
    about = clap::crate_description!(),
    author = clap::crate_authors!(),
    version = clap::crate_version!(),
)]
struct Options {
    /// The subcommand to run.
    #[clap(subcommand)]
    command: Subcommand,
}

/// The runner subcommands. There is intentionally no "run all" variant.
#[derive(clap::Subcommand, Debug)]
enum Subcommand {
    /// Run exactly one test against one grid and record the result.
    Run(RunArgs),
    /// List the registered tests.
    List(ListArgs),
    /// Generate the man page.
    GenerateManpage {
        /// The directory to write the man page into.
        #[clap(long)]
        output_dir: PathBuf,
    },
    /// Generate shell completions.
    GenerateShellCompletion {
        /// The file to write completions into.
        #[clap(long)]
        output_file: PathBuf,
        /// The shell to generate completions for.
        #[clap(long)]
        shell: clap_complete::aot::Shell,
    },
}

/// Arguments for the `run` subcommand.
#[derive(clap::Args, Debug)]
struct RunArgs {
    /// The grid to test against.
    #[clap(long, value_enum)]
    grid: Grid,
    /// The primary avatar name in the credentials file (defaults to the file's
    /// default avatar).
    #[clap(long)]
    avatar: Option<String>,
    /// The secondary avatar name for two-account tests.
    #[clap(long)]
    secondary: Option<String>,
    /// The tertiary avatar name for three-account tests.
    #[clap(long)]
    tertiary: Option<String>,
    /// The credentials TOML file (defaults per grid: credentials.toml /
    /// credentials.aditi.toml).
    #[clap(long)]
    credentials: Option<PathBuf>,
    /// Bypass the per-avatar aditi login cooldown.
    #[clap(long)]
    force: bool,
    /// The single test to run.
    test: String,
}

/// Arguments for the `list` subcommand.
#[derive(clap::Args, Debug)]
struct ListArgs {
    /// Restrict the listing to tests applicable to this grid.
    #[clap(long, value_enum)]
    grid: Option<Grid>,
}

/// A runner error.
#[derive(Debug, thiserror::Error)]
enum Error {
    /// A git error computing the repository root or describe.
    #[error("git error: {0}")]
    Git(gitinfo::GitError),
    /// Credentials could not be loaded or an avatar could not be selected.
    #[error("credentials error: {0}")]
    Auth(String),
    /// The named test is not registered.
    #[error("unknown test `{0}`")]
    UnknownTest(String),
    /// The test does not apply to the chosen grid.
    #[error("test `{test}` does not apply to grid {grid}")]
    NotApplicable {
        /// The test name.
        test: String,
        /// The grid that was requested.
        grid: Grid,
    },
    /// The test needs more avatars than the credentials provide.
    #[error("test `{test}` needs {needed} avatars but only {found} configured")]
    NotEnoughAvatars {
        /// The test name.
        test: String,
        /// The number of avatars required.
        needed: u8,
        /// The number of avatars found.
        found: u8,
    },
    /// A test run failed, or a cooldown blocked it.
    #[error("{0}")]
    Test(String),
    /// A record could not be written.
    #[error("record error: {0}")]
    Record(record::RecordError),
    /// Logging or output-generation failed.
    #[error("{0}")]
    Log(String),
}
