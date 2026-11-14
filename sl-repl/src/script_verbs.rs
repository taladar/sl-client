//! The `script` verbs: put a source file into a prim, list, query, start, stop
//! and reset its scripts, tail what scripts say, and check a source file with
//! no grid at all.
//!
//! A grid command line is one [`Command`]; a `script` line is a short
//! conversation. `script put` lists the prim, makes the script item when it is
//! missing, waits for the listing to show it, uploads the source and waits for
//! the compile result — the same sequence the `lsl-probes` conformance case
//! drives. So the verbs are a small sans-I/O state machine, [`ScriptDriver`],
//! which both runtimes drive the same way: [`ScriptDriver::start`] a verb,
//! feed it every [`Event`] ([`ScriptDriver::on_event`]) and a periodic
//! [`ScriptDriver::tick`], and send the [`Command`]s and print the
//! [`ScriptLine`]s each returns. While [`ScriptDriver::is_busy`] the runtime
//! holds further input lines back, so a replayed `.repl` script's next line
//! sees the previous verb finished.
//!
//! The verbs (see [`SCRIPT_USAGE`]):
//!
//! - `script put <object> <name> <file.lsl> [target=] [running=]` — upload
//!   over the task's script item, made first when the prim has no script of
//!   that name, and print whether it compiled, each compiler error rendered
//!   with a caret under its source line;
//! - `script list <object>` — the prim's scripts;
//! - `script state|run|stop|reset <object> <item>` — the run state, or set it;
//! - `script watch [seconds|off]` — print everything a script can say to the
//!   session: object chat on every type (`DEBUG_CHANNEL` included), dialogs,
//!   permission questions, `LoadURL`s, map teleports and object IMs, each with
//!   its object;
//! - `script check <file.lsl> [syntax=<LSLSyntax.xml>]` — lex, parse, analyse
//!   and render the diagnostics locally ([`check_script`]).
//!
//! An `<object>` is its region-local id (scoped to the current circuit) or its
//! UUID; an `<item>` is the script item's UUID or its name in the prim.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sl_proto::{
    AgentKey, ChatSource, ChatType, Command, Event, ImDialog, InventoryKey, InventoryType,
    LslSyntax, ObjectKey, RestoreItem, RezScriptParams, ScopedObjectId, ScriptTarget,
    ScriptUploadLocation, TaskInventoryItem, Uuid, XferListing,
};

use crate::args::Args;
use crate::context::ReplContext;
use crate::error::ReplError;
use crate::registry::{parse_script_target, scoped_object};

/// The usage line of every `script` verb, as `help script` prints them.
pub const SCRIPT_USAGE: &[&str] = &[
    "script put <object> <name> <file.lsl> [target=mono|lsl2] [running=true]",
    "script list <object>",
    "script state <object> <item>",
    "script run <object> <item>",
    "script stop <object> <item>",
    "script reset <object> <item>",
    "script watch [seconds|off]",
    "script check <file.lsl> [syntax=<LSLSyntax.xml>]",
];

/// How long a verb waits for the prim it names to be in the scene.
const LOCATE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a listing, or a made script's appearance in one, may take.
const LISTING_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a compile may take; the grid compiles before it answers.
const COMPILE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a run-state answer may take.
const STATE_TIMEOUT: Duration = Duration::from_secs(30);

/// How often a made script's prim is listed again until it shows the item.
const LISTING_POLL: Duration = Duration::from_millis(500);

/// A `script` line with its arguments parsed but not yet resolved: the verb
/// and the rest of the line. [`ScriptRequest::resolve`] turns it into a
/// [`ScriptVerb`] at dispatch, when the placeholders have their values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRequest {
    /// The verb (`put`, `list`, …), empty when the line was a bare `script`.
    pub verb: String,
    /// The verb's arguments.
    pub args: Args,
}

/// The prim a verb is about, as the line named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRef {
    /// By its region-local id, scoped to the circuit current at dispatch.
    Local(ScopedObjectId),
    /// By its UUID.
    Key(ObjectKey),
}

/// The script item a verb is about, as the line named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemRef {
    /// By its item UUID.
    Key(InventoryKey),
    /// By its name in the prim, found in the prim's listing.
    Name(String),
}

/// What a `script watch` line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchSpan {
    /// Watch until told otherwise; the REPL is not held.
    On,
    /// Watch for this long, holding the REPL's further lines until it ends.
    For(Duration),
    /// Stop watching.
    Off,
}

/// The change a run-state verb makes before it asks the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateChange {
    /// None: only ask (`script state`).
    Ask,
    /// Set the script running (`script run`).
    Run,
    /// Set it not running (`script stop`).
    Stop,
    /// Reset it (`script reset`); the grid does not answer one.
    Reset,
}

/// A resolved `script` verb.
#[derive(Debug, Clone)]
pub enum ScriptVerb {
    /// Upload a source file into a prim's script of a name.
    Put {
        /// The prim.
        object: ObjectRef,
        /// The script's name in the prim.
        name: String,
        /// Where the source came from, for the report.
        path: PathBuf,
        /// The source.
        source: String,
        /// The compile target.
        target: ScriptTarget,
        /// Whether the script runs once saved.
        running: bool,
        /// The agent, who creates the item when it is missing.
        creator: AgentKey,
    },
    /// List a prim's scripts.
    List {
        /// The prim.
        object: ObjectRef,
    },
    /// Ask or change a script's run state.
    State {
        /// The prim.
        object: ObjectRef,
        /// The script.
        item: ItemRef,
        /// What to do before asking.
        change: StateChange,
    },
    /// Start, end or time-limit the watch.
    Watch(WatchSpan),
    /// Check a source file with no grid.
    Check {
        /// Where the source came from, for the report.
        path: PathBuf,
        /// The source.
        source: String,
        /// The library table to check calls against, when one was given.
        syntax: Option<LslSyntax>,
    },
}

impl ScriptRequest {
    /// Resolve this request into a [`ScriptVerb`] against `ctx`, reading any
    /// file it names.
    ///
    /// # Errors
    ///
    /// Returns [`ReplError::UnknownCommand`] for a verb that is not one of
    /// [`SCRIPT_USAGE`], whatever an argument accessor returns for a missing,
    /// malformed or unresolvable argument, [`ReplError::ReadFile`] when a
    /// source or syntax file cannot be read or decoded, and
    /// [`ReplError::UnreadArgs`] for an argument the verb does not take.
    pub fn resolve(&self, ctx: &dyn ReplContext) -> Result<ScriptVerb, ReplError> {
        let command = format!("script {}", self.verb);
        let args = self.args.clone().with_command(&command);
        let verb = match self.verb.as_str() {
            "put" => ScriptVerb::Put {
                object: object_arg(&args, ctx)?,
                name: args.req_str(ctx, "name", 1)?,
                path: PathBuf::from(args.req_str(ctx, "file", 2)?),
                source: String::new(),
                target: match args.opt_str(ctx, "target", 3)? {
                    Some(value) => parse_script_target("target", &value)?,
                    None => ScriptTarget::Mono,
                },
                running: args.bool_or(ctx, "running", 4, true)?,
                creator: ctx
                    .resolve_placeholder("self")
                    .and_then(|agent| Uuid::parse_str(&agent).ok())
                    .map(AgentKey::from)
                    .ok_or_else(|| ReplError::Unresolved("self".to_owned()))?,
            },
            "list" => ScriptVerb::List {
                object: object_arg(&args, ctx)?,
            },
            "state" | "run" | "stop" | "reset" => ScriptVerb::State {
                object: object_arg(&args, ctx)?,
                item: item_arg(&args, ctx)?,
                change: match self.verb.as_str() {
                    "run" => StateChange::Run,
                    "stop" => StateChange::Stop,
                    "reset" => StateChange::Reset,
                    _ => StateChange::Ask,
                },
            },
            "watch" => ScriptVerb::Watch(match args.opt_str(ctx, "seconds", 0)?.as_deref() {
                None | Some("on") => WatchSpan::On,
                Some("off") => WatchSpan::Off,
                Some(value) => WatchSpan::For(
                    value
                        .parse::<f64>()
                        .ok()
                        .filter(|seconds| seconds.is_finite())
                        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
                        .ok_or_else(|| ReplError::InvalidArg {
                            field: "seconds".to_owned(),
                            value: value.to_owned(),
                            expected: "number of seconds, `on` or `off`".to_owned(),
                        })?,
                ),
            }),
            "check" => ScriptVerb::Check {
                path: PathBuf::from(args.req_str(ctx, "file", 0)?),
                source: String::new(),
                syntax: match args.opt_str(ctx, "syntax", 1)? {
                    Some(path) => Some(read_syntax(Path::new(&path))?),
                    None => None,
                },
            },
            other => return Err(ReplError::UnknownCommand(format!("script {other}"))),
        };
        let unread = args.unread();
        if !unread.is_empty() {
            return Err(ReplError::UnreadArgs { command, unread });
        }
        Ok(match verb {
            ScriptVerb::Put {
                object,
                name,
                path,
                source: _,
                target,
                running,
                creator,
            } => ScriptVerb::Put {
                object,
                name,
                source: read_source(&path)?,
                path,
                target,
                running,
                creator,
            },
            ScriptVerb::Check {
                path,
                source: _,
                syntax,
            } => ScriptVerb::Check {
                source: read_source(&path)?,
                path,
                syntax,
            },
            other => other,
        })
    }
}

/// The `<object>` argument (position 0): a region-local id or a UUID.
fn object_arg(args: &Args, ctx: &dyn ReplContext) -> Result<ObjectRef, ReplError> {
    let value = args.req_str(ctx, "object", 0)?;
    if let Ok(local) = value.parse::<u32>() {
        return Ok(ObjectRef::Local(scoped_object(ctx, local)?));
    }
    Uuid::parse_str(&value)
        .map(|key| ObjectRef::Key(ObjectKey::from(key)))
        .map_err(|_error| ReplError::InvalidArg {
            field: "object".to_owned(),
            value,
            expected: "region-local object id or object UUID".to_owned(),
        })
}

/// The `<item>` argument (position 1): a UUID, else a name.
fn item_arg(args: &Args, ctx: &dyn ReplContext) -> Result<ItemRef, ReplError> {
    let value = args.req_str(ctx, "item", 1)?;
    Ok(Uuid::parse_str(&value).map_or(ItemRef::Name(value), |key| {
        ItemRef::Key(InventoryKey::from(key))
    }))
}

/// Read a script source file.
fn read_source(path: &Path) -> Result<String, ReplError> {
    fs_err::read_to_string(path).map_err(|error| ReplError::ReadFile(error.to_string()))
}

/// Read an `LSLSyntax` document (LLSD XML, as the capability serves it, or as
/// OpenSim ships it in `bin/ScriptSyntax.xml`: the syntax id on the first line,
/// the document after it).
fn read_syntax(path: &Path) -> Result<LslSyntax, ReplError> {
    let text = read_source(path)?;
    let document = match text.split_once('\n') {
        Some((first, rest)) if Uuid::parse_str(first.trim()).is_ok() => rest,
        _ => text.as_str(),
    };
    let llsd = sl_llsd::parse_llsd_xml(document)
        .map_err(|error| ReplError::ReadFile(format!("{}: {error}", path.display())))?;
    sl_wire::parse_lsl_syntax(&llsd)
        .map_err(|error| ReplError::ReadFile(format!("{}: {error}", path.display())))
}

/// What a [`check_script`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCheck {
    /// The report: a summary line, then each finding rendered with its caret.
    pub lines: Vec<ScriptLine>,
    /// How many of the findings are errors (the grid would refuse the script).
    pub errors: usize,
}

/// Check `source` (read from `path`) with no grid: parse it and, when it
/// parses, run the semantic pass against `syntax` (an empty table skips the
/// checks that need the library, as the semantic pass does).
#[must_use]
pub fn check_script(path: &Path, source: &str, syntax: Option<&LslSyntax>) -> ScriptCheck {
    let empty = LslSyntax::default();
    let syntax = syntax.unwrap_or(&empty);
    let parse = sl_lsl::parse(source);
    let mut findings = Vec::new();
    let mut errors = 0_usize;
    if parse.has_errors() {
        for error in &parse.errors {
            errors = errors.saturating_add(1);
            findings.push(ScriptLine::Warn(sl_lsl::render_parse_error(source, error)));
        }
    } else {
        for diagnostic in sl_lsl::analyze(&parse.script, syntax) {
            let rendered = sl_lsl::render_diagnostic(source, &diagnostic, syntax);
            if diagnostic.severity == sl_lsl::Severity::Error {
                errors = errors.saturating_add(1);
            }
            findings.push(ScriptLine::Warn(rendered));
        }
    }
    let library = if syntax.is_empty() {
        " (no library table: calls and constants unchecked)"
    } else {
        ""
    };
    let summary = format!(
        "script check {}: {errors} error(s), {} warning(s){library}",
        path.display(),
        findings.len().saturating_sub(errors)
    );
    let mut lines = vec![if errors == 0 {
        ScriptLine::Info(summary)
    } else {
        ScriptLine::Warn(summary)
    }];
    lines.append(&mut findings);
    ScriptCheck { lines, errors }
}

/// Read `path` (and the `LSLSyntax` document `syntax`, when given) and
/// [`check_script`] it — `script check` as a command-line tool.
///
/// # Errors
///
/// Returns [`ReplError::ReadFile`] when either file cannot be read or the
/// syntax document cannot be decoded.
pub fn check_script_file(path: &Path, syntax: Option<&Path>) -> Result<ScriptCheck, ReplError> {
    let source = read_source(path)?;
    let syntax = syntax.map(read_syntax).transpose()?;
    Ok(check_script(path, &source, syntax.as_ref()))
}

/// One line of a verb's report, at the level a runtime logs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptLine {
    /// An ordinary report line.
    Info(String),
    /// A failure, a compile error or a finding.
    Warn(String),
}

/// What one call into the [`ScriptDriver`] asks the runtime to do.
#[derive(Debug, Clone, Default)]
pub struct ScriptOutput {
    /// The commands to send to the session, in order.
    pub commands: Vec<Command>,
    /// The lines to print, in order.
    pub lines: Vec<ScriptLine>,
}

impl ScriptOutput {
    /// Add a command.
    fn send(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// Add an info line.
    fn info(&mut self, line: String) {
        self.lines.push(ScriptLine::Info(line));
    }

    /// Add a warning line.
    fn warn(&mut self, line: String) {
        self.lines.push(ScriptLine::Warn(line));
    }
}

/// The verb a job carries out.
#[derive(Debug, Clone)]
enum Goal {
    /// `script put`.
    Put {
        /// The script's name in the prim.
        name: String,
        /// The source.
        source: String,
        /// The compile target.
        target: ScriptTarget,
        /// Whether it runs once saved.
        running: bool,
        /// Who creates a missing item.
        creator: AgentKey,
        /// Whether this job has already asked for the item to be made.
        made: bool,
    },
    /// `script list`.
    List,
    /// `script state|run|stop|reset`.
    State {
        /// The script.
        item: ItemRef,
        /// What to do before asking.
        change: StateChange,
    },
}

/// Where a job is in its conversation with the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Waiting for the prim to be in the scene.
    Locating,
    /// Waiting for a listing; `poll_at` is when to ask again, when the last
    /// one did not show a made script yet.
    Listing {
        /// When to ask for the next listing, if one is not already asked.
        poll_at: Option<Instant>,
    },
    /// Waiting for the compile result.
    Uploading,
    /// Waiting for the run state of this item.
    AskingState(InventoryKey),
}

/// A verb in progress.
#[derive(Debug, Clone)]
struct Job {
    /// The prim as the line named it.
    object: ObjectRef,
    /// The prim's scoped id and UUID, once it is in the scene.
    prim: Option<(ScopedObjectId, ObjectKey)>,
    /// What the job is for.
    goal: Goal,
    /// Where it is.
    phase: Phase,
    /// When the current phase gives up.
    deadline: Instant,
}

/// Whether, and until when, the driver prints what scripts say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watch {
    /// Not watching.
    Off,
    /// Watching until told otherwise.
    On,
    /// Watching until this instant, holding the REPL.
    Until(Instant),
}

/// The sans-I/O state machine behind the `script` verbs; see the
/// [module documentation](self).
#[derive(Debug, Clone)]
pub struct ScriptDriver {
    /// Every prim seen, by UUID: its scoped id.
    scoped: HashMap<ObjectKey, ScopedObjectId>,
    /// Every prim seen, by scoped id: its UUID.
    keys: HashMap<ScopedObjectId, ObjectKey>,
    /// The verb in progress, if any.
    job: Option<Job>,
    /// The watch.
    watch: Watch,
}

impl Default for ScriptDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptDriver {
    /// A driver with no verb in progress, not watching.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scoped: HashMap::new(),
            keys: HashMap::new(),
            job: None,
            watch: Watch::Off,
        }
    }

    /// Whether a verb is in progress, or a timed watch is running: the runtime
    /// holds further input lines until it is not.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.job.is_some() || matches!(self.watch, Watch::Until(_))
    }

    /// Start `verb` at `now`. A verb started while another is in progress
    /// replaces it, saying so; a runtime that holds its lines while
    /// [`is_busy`](Self::is_busy) never does that.
    pub fn start(&mut self, verb: ScriptVerb, now: Instant) -> ScriptOutput {
        let mut out = ScriptOutput::default();
        let (object, goal) = match verb {
            ScriptVerb::Check {
                path,
                source,
                syntax,
            } => {
                out.lines = check_script(&path, &source, syntax.as_ref()).lines;
                return out;
            }
            ScriptVerb::Watch(span) => {
                self.watch = match span {
                    WatchSpan::On => Watch::On,
                    WatchSpan::Off => Watch::Off,
                    WatchSpan::For(duration) => {
                        Watch::Until(now.checked_add(duration).unwrap_or(now))
                    }
                };
                out.info(match span {
                    WatchSpan::On => "script watch: on".to_owned(),
                    WatchSpan::Off => "script watch: off".to_owned(),
                    WatchSpan::For(duration) => {
                        format!("script watch: on for {}s", duration.as_secs_f64())
                    }
                });
                return out;
            }
            ScriptVerb::Put {
                object,
                name,
                path: _,
                source,
                target,
                running,
                creator,
            } => (
                object,
                Goal::Put {
                    name,
                    source,
                    target,
                    running,
                    creator,
                    made: false,
                },
            ),
            ScriptVerb::List { object } => (object, Goal::List),
            ScriptVerb::State {
                object,
                item,
                change,
            } => (object, Goal::State { item, change }),
        };
        if self.job.take().is_some() {
            out.warn("script: the previous verb was abandoned".to_owned());
        }
        self.job = Some(Job {
            object,
            prim: None,
            goal,
            phase: Phase::Locating,
            deadline: now.checked_add(LOCATE_TIMEOUT).unwrap_or(now),
        });
        self.locate(now, &mut out);
        out
    }

    /// Feed one session event at `now`.
    pub fn on_event(&mut self, event: &Event, now: Instant) -> ScriptOutput {
        let mut out = ScriptOutput::default();
        if let Event::ObjectAdded(object) | Event::ObjectUpdated(object) = event {
            let scoped = object.scoped_id();
            let _previous = self.scoped.insert(object.full_id, scoped);
            let _previous = self.keys.insert(scoped, object.full_id);
            if self
                .job
                .as_ref()
                .is_some_and(|job| job.phase == Phase::Locating)
            {
                self.locate(now, &mut out);
            }
        }
        if self.watch != Watch::Off
            && let Some(line) = watch_line(event)
        {
            out.info(line);
        }
        self.answer(event, now, &mut out);
        out
    }

    /// Advance time to `now`: give up a phase past its deadline, ask for a
    /// made script's listing again, end a timed watch.
    pub fn tick(&mut self, now: Instant) -> ScriptOutput {
        let mut out = ScriptOutput::default();
        if let Watch::Until(until) = self.watch
            && now >= until
        {
            self.watch = Watch::Off;
            out.info("script watch: ended".to_owned());
        }
        let Some(job) = self.job.as_mut() else {
            return out;
        };
        if now >= job.deadline {
            let made = matches!(job.goal, Goal::Put { made: true, .. });
            let what = match job.phase {
                Phase::Locating => "waiting for the prim to be in the scene",
                // A grid says nothing when it refuses to make a script.
                Phase::Listing { .. } if made => {
                    "waiting for the new script to show in the prim's listing (a grid makes \
                     one only in a prim the agent may edit)"
                }
                Phase::Listing { .. } => "waiting for the prim's listing",
                Phase::Uploading => "waiting for the compile result",
                Phase::AskingState(_) => "waiting for the script's run state",
            };
            self.job = None;
            out.warn(format!("script: timed out {what}"));
            return out;
        }
        if let (Phase::Listing { poll_at: Some(at) }, Some((scoped, _key))) = (job.phase, job.prim)
            && now >= at
        {
            job.phase = Phase::Listing { poll_at: None };
            out.send(list_command(scoped));
        }
        out
    }

    /// Find the job's prim in the scene and, when it is there, begin.
    fn locate(&mut self, now: Instant, out: &mut ScriptOutput) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        let prim = match job.object {
            ObjectRef::Local(scoped) => self.keys.get(&scoped).map(|key| (scoped, *key)),
            ObjectRef::Key(key) => self.scoped.get(&key).map(|scoped| (*scoped, key)),
        };
        let Some((scoped, key)) = prim else {
            return;
        };
        job.prim = Some((scoped, key));
        match &job.goal {
            Goal::State {
                item: ItemRef::Key(item),
                change,
            } => {
                let (item, change) = (*item, *change);
                self.change_state(key, item, change, now, out);
            }
            Goal::Put { .. } | Goal::List | Goal::State { .. } => {
                job.phase = Phase::Listing { poll_at: None };
                job.deadline = now.checked_add(LISTING_TIMEOUT).unwrap_or(now);
                out.send(list_command(scoped));
            }
        }
    }

    /// Make `change` to `item` in the prim `key`, and ask its state after.
    fn change_state(
        &mut self,
        key: ObjectKey,
        item: InventoryKey,
        change: StateChange,
        now: Instant,
        out: &mut ScriptOutput,
    ) {
        match change {
            StateChange::Ask => {}
            StateChange::Run | StateChange::Stop => out.send(Command::SetScriptRunning {
                object_id: key,
                item_id: item,
                running: change == StateChange::Run,
            }),
            StateChange::Reset => {
                out.send(Command::ResetScript {
                    object_id: key,
                    item_id: item,
                });
                out.info(format!("script: reset {item} in {key}"));
                self.job = None;
                return;
            }
        }
        out.send(Command::RequestScriptRunning {
            object_id: key,
            item_id: item,
        });
        if let Some(job) = self.job.as_mut() {
            job.phase = Phase::AskingState(item);
            job.deadline = now.checked_add(STATE_TIMEOUT).unwrap_or(now);
        }
    }

    /// Act on an event that answers the job's current phase.
    fn answer(&mut self, event: &Event, now: Instant, out: &mut ScriptOutput) {
        let Some(job) = self.job.as_ref() else {
            return;
        };
        let Some((_scoped, key)) = job.prim else {
            return;
        };
        match (job.phase, event) {
            (
                Phase::Listing { poll_at: None },
                Event::TaskInventoryContents { task, items, .. },
            ) if *task == key => {
                self.listed(items, now, out);
            }
            (Phase::Listing { poll_at: None }, Event::TaskInventoryUnanswered { task })
                if task.is_none_or(|task| task == key) =>
            {
                self.job = None;
                out.warn(format!(
                    "script: the grid never answered the listing of {key}"
                ));
            }
            (
                Phase::Listing { poll_at: None },
                Event::XferDecodeFailed {
                    file: XferListing::TaskInventory { task, .. },
                    error,
                    ..
                },
            ) if *task == key => {
                self.job = None;
                out.warn(format!(
                    "script: the listing of {key} did not parse: {error}"
                ));
            }
            (
                Phase::Uploading,
                Event::ScriptUploaded {
                    compiled,
                    errors,
                    running,
                    ..
                },
            ) => {
                if let Goal::Put { name, source, .. } = &job.goal {
                    let running =
                        running.map_or_else(String::new, |running| format!(", running={running}"));
                    if *compiled {
                        out.info(format!(
                            "script: put {name:?} into {key}: compiled{running}"
                        ));
                    } else {
                        out.warn(format!(
                            "script: put {name:?} into {key}: did not compile, {} error(s){running}",
                            errors.len()
                        ));
                    }
                    for error in errors {
                        out.warn(error.render(source));
                    }
                }
                self.job = None;
            }
            (Phase::Uploading, Event::AssetUploadFailed { reason }) => {
                self.job = None;
                out.warn(format!("script: the upload into {key} failed: {reason}"));
            }
            (
                Phase::AskingState(asked),
                Event::ScriptRunning {
                    object_id,
                    item_id,
                    running,
                },
            ) if *object_id == key && *item_id == asked => {
                self.job = None;
                out.info(format!(
                    "script: {item_id} in {key} is {}",
                    if *running { "running" } else { "not running" }
                ));
            }
            _ => {}
        }
    }

    /// Act on the prim's listing.
    fn listed(&mut self, items: &[TaskInventoryItem], now: Instant, out: &mut ScriptOutput) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        let Some((scoped, key)) = job.prim else {
            return;
        };
        let scripts = items
            .iter()
            .filter(|item| item.inv_type == InventoryType::Script);
        let named = |name: &str| {
            items
                .iter()
                .find(|item| item.inv_type == InventoryType::Script && item.name == name)
                .map(|item| item.item_id)
        };
        match &mut job.goal {
            Goal::List => {
                let mut count = 0_usize;
                let mut lines = Vec::new();
                for item in scripts {
                    count = count.saturating_add(1);
                    let mut line = format!("script:   {:?} item={}", item.name, item.item_id);
                    if let Some(asset) = item.asset_id {
                        let _written = write!(line, " asset={asset}");
                    }
                    lines.push(line);
                }
                out.info(format!("script: {key} holds {count} script(s)"));
                for line in lines {
                    out.info(line);
                }
                self.job = None;
            }
            Goal::State { item, change } => {
                let change = *change;
                let found = match item {
                    ItemRef::Key(item) => Some(*item),
                    ItemRef::Name(name) => named(name),
                };
                if let Some(item) = found {
                    self.change_state(key, item, change, now, out);
                } else {
                    let name = match item {
                        ItemRef::Key(item) => item.to_string(),
                        ItemRef::Name(name) => format!("{name:?}"),
                    };
                    self.job = None;
                    out.warn(format!("script: {key} holds no script {name}"));
                }
            }
            Goal::Put {
                name,
                source,
                target,
                running,
                creator,
                made,
            } => {
                if let Some(item) = named(name) {
                    out.send(Command::UploadScript {
                        location: ScriptUploadLocation::TaskInventory {
                            task_id: key,
                            item_id: item,
                            running: *running,
                            experience: None,
                        },
                        target: *target,
                        source: source.as_bytes().to_vec(),
                    });
                    job.phase = Phase::Uploading;
                    job.deadline = now.checked_add(COMPILE_TIMEOUT).unwrap_or(now);
                } else {
                    if !*made {
                        *made = true;
                        out.send(Command::RezScript {
                            target: scoped,
                            params: Box::new(RezScriptParams {
                                group_id: None,
                                enabled: false,
                                item: RestoreItem::new_script(*creator, key, name, Uuid::new_v4()),
                            }),
                        });
                    }
                    job.phase = Phase::Listing {
                        poll_at: Some(now.checked_add(LISTING_POLL).unwrap_or(now)),
                    };
                }
            }
        }
    }
}

/// The `FetchTaskInventory` that lists the prim `scoped`.
const fn list_command(scoped: ScopedObjectId) -> Command {
    Command::FetchTaskInventory {
        target: scoped,
        held_serial: None,
    }
}

/// The watch line for `event`, when it is something a script said to the
/// session.
fn watch_line(event: &Event) -> Option<String> {
    match event {
        Event::ChatReceived(chat) => {
            let ChatSource::Object(object) = &chat.source else {
                return None;
            };
            let kind = match chat.chat_type {
                ChatType::StartTyping | ChatType::StopTyping => return None,
                ChatType::Whisper => "whisper".to_owned(),
                ChatType::Normal => "say".to_owned(),
                ChatType::Shout => "shout".to_owned(),
                ChatType::DebugChannel => "debug".to_owned(),
                ChatType::Region => "region-say".to_owned(),
                ChatType::Owner => "owner-say".to_owned(),
                ChatType::Direct => "say-to".to_owned(),
                ChatType::Unknown(code) => format!("chat-type-{code}"),
                other => format!("{other:?}"),
            };
            Some(format!(
                "script: [{kind}] {:?} ({object}): {}",
                chat.from_name, chat.message
            ))
        }
        Event::ScriptDialog(dialog) => Some(format!(
            "script: [dialog channel {}] {:?} ({}): {} {:?}",
            dialog.chat_channel,
            dialog.object_name,
            dialog.object_id,
            dialog.message,
            dialog.buttons
        )),
        Event::ScriptPermissionRequest(request) => Some(format!(
            "script: [permissions] {:?} ({}) item {} asks for {:?}",
            request.object_name, request.task_id, request.item_id, request.permissions
        )),
        Event::LoadUrl(request) => Some(format!(
            "script: [load-url] {:?} ({}): {} {}",
            request.object_name, request.object_id, request.message, request.url
        )),
        Event::ScriptTeleport(request) => Some(format!(
            "script: [map-destination] {:?}: {} {:?}",
            request.object_name,
            request
                .region_name
                .as_ref()
                .map_or_else(|| "(this region)".to_owned(), ToString::to_string),
            request.position
        )),
        Event::InstantMessageReceived(im) if matches!(im.dialog, ImDialog::FromTask) => {
            Some(format!(
                "script: [im] {:?} ({}): {}",
                im.from_agent_name, im.from_agent_id, im.message
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::{
        AgentKey, AssetType, ChatAudible, ChatMessage, ChatSource, ChatType, CircuitId, Command,
        Event, InventoryKey, InventoryType, LindenAmount, Object, ObjectExtraParams, ObjectKey,
        ObjectMotion, OwnerKey, Permissions, Permissions5, PrimShapeParams, RegionCoordinates,
        RegionHandle, RegionLocalObjectId, Rotation, SaleType, ScopedObjectId, ScriptCompileError,
        ScriptTarget, ScriptUploadLocation, TaskInventoryItem, Uuid, Vector,
    };

    use super::{
        ItemRef, ObjectRef, ScriptDriver, ScriptLine, ScriptRequest, ScriptVerb, StateChange,
        WatchSpan, check_script,
    };
    use crate::context::NoContext;
    use crate::error::ReplError;
    use crate::parse::{ReplAction, parse_line};

    /// The prim the tests put scripts into.
    const PRIM: u128 = 0xC0BE;

    /// The script item the tests' listings hold.
    const ITEM: u128 = 0x17E3;

    /// The prim's UUID.
    fn prim_key() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(PRIM))
    }

    /// The prim's scoped id.
    const fn prim_scoped() -> ScopedObjectId {
        ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(42))
    }

    /// The prim as an object update reports it.
    fn prim() -> Object {
        /// The zero vector.
        const ZERO: Vector = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(42),
            circuit: CircuitId::new(1),
            full_id: prim_key(),
            parent_id: RegionLocalObjectId(0),
            pcode: 9,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
            motion: ObjectMotion {
                position: ZERO,
                velocity: ZERO,
                acceleration: ZERO,
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: ZERO,
                collision_plane: None,
            },
            owner_id: Uuid::nil(),
            sound: Uuid::nil(),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: ZERO,
            joint_axis_or_anchor: ZERO,
        }
    }

    /// A script item called `name` in the prim.
    fn script_item(name: &str) -> TaskInventoryItem {
        TaskInventoryItem {
            item_id: InventoryKey::from(Uuid::from_u128(ITEM)),
            parent_task: prim_key(),
            permissions: Permissions5 {
                base: Permissions::ALL,
                owner: Permissions::ALL,
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::ALL,
            },
            creator_id: AgentKey::from(Uuid::from_u128(1)),
            last_owner_id: AgentKey::from(Uuid::from_u128(1)),
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(1))),
            group: None,
            group_owned: false,
            asset_id: None,
            asset_type: AssetType::ScriptText,
            inv_type: InventoryType::Script,
            flags: 0,
            sale_type: SaleType::NotForSale,
            sale_price: LindenAmount(0),
            name: name.to_owned(),
            description: String::new(),
            creation_date: 0,
        }
    }

    /// The prim's listing holding `items`.
    fn listing(items: Vec<TaskInventoryItem>) -> Event {
        Event::TaskInventoryContents {
            task: prim_key(),
            serial: 1,
            items,
        }
    }

    /// The text of every line, warnings marked.
    fn texts(lines: &[ScriptLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                ScriptLine::Info(text) => text.clone(),
                ScriptLine::Warn(text) => format!("WARN {text}"),
            })
            .collect()
    }

    /// Parse a `script` line into its request.
    fn request(line: &str) -> Result<ScriptRequest, String> {
        match parse_line(line) {
            Ok(Some(ReplAction::Script(request))) => Ok(request),
            other => Err(format!("{line:?} parsed as {other:?}")),
        }
    }

    #[test]
    fn a_script_line_is_a_script_request() -> Result<(), String> {
        let parsed = request("script state 9f1a2c4e-0000-4000-8000-000000000001 hello")?;
        assert_eq!(parsed.verb, "state");
        let verb = parsed
            .resolve(&NoContext)
            .map_err(|error| error.to_string())?;
        let ScriptVerb::State {
            object,
            item,
            change,
        } = verb
        else {
            return Err(format!("resolved as {verb:?}"));
        };
        assert_eq!(
            object,
            ObjectRef::Key(ObjectKey::from(Uuid::from_u128(
                0x9f1a_2c4e_0000_4000_8000_0000_0000_0001
            )))
        );
        assert_eq!(item, ItemRef::Name("hello".to_owned()));
        assert_eq!(change, StateChange::Ask);
        Ok(())
    }

    #[test]
    fn a_watch_takes_seconds_or_off() -> Result<(), String> {
        let resolve = |line: &str| -> Result<ScriptVerb, String> {
            request(line)?
                .resolve(&NoContext)
                .map_err(|error| error.to_string())
        };
        assert!(matches!(
            resolve("script watch 2.5")?,
            ScriptVerb::Watch(WatchSpan::For(duration)) if duration == Duration::from_millis(2500)
        ));
        assert!(matches!(
            resolve("script watch off")?,
            ScriptVerb::Watch(WatchSpan::Off)
        ));
        assert!(matches!(
            resolve("script watch")?,
            ScriptVerb::Watch(WatchSpan::On)
        ));
        Ok(())
    }

    #[test]
    fn an_unknown_verb_or_a_surplus_argument_is_refused() -> Result<(), String> {
        let refused = |line: &str| -> Result<ReplError, String> {
            match request(line)?.resolve(&NoContext) {
                Ok(verb) => Err(format!("{line:?} resolved as {verb:?}")),
                Err(error) => Ok(error),
            }
        };
        assert_eq!(
            refused("script frobnicate 1")?,
            ReplError::UnknownCommand("script frobnicate".to_owned())
        );
        assert!(matches!(
            refused("script watch 1 extra=2")?,
            ReplError::UnreadArgs { .. }
        ));
        // A local id needs a circuit to scope it to.
        assert_eq!(
            refused("script list 42")?,
            ReplError::Unresolved("circuitid".to_owned())
        );
        Ok(())
    }

    #[test]
    fn a_put_makes_the_missing_item_waits_for_it_and_reports_the_compile() -> Result<(), String> {
        let mut driver = ScriptDriver::new();
        let start = Instant::now();
        let source = "default\n{\n    state_entry() { llSay(0, x); }\n}\n";
        let out = driver.start(
            ScriptVerb::Put {
                object: ObjectRef::Key(prim_key()),
                name: "hello".to_owned(),
                path: "hello.lsl".into(),
                source: source.to_owned(),
                target: ScriptTarget::Mono,
                running: true,
                creator: AgentKey::from(Uuid::from_u128(1)),
            },
            start,
        );
        assert!(out.commands.is_empty(), "the prim is not in the scene yet");
        assert!(driver.is_busy());

        let out = driver.on_event(&Event::ObjectAdded(Box::new(prim())), start);
        assert!(matches!(
            out.commands.as_slice(),
            [Command::FetchTaskInventory { target, held_serial: None }] if *target == prim_scoped()
        ));

        let out = driver.on_event(&listing(Vec::new()), start);
        let [Command::RezScript { target, params }] = out.commands.as_slice() else {
            return Err(format!("an empty prim brought {:?}", out.commands));
        };
        assert_eq!(*target, prim_scoped());
        assert_eq!(params.item.name, "hello");

        let soon = start
            .checked_add(Duration::from_millis(100))
            .ok_or("time")?;
        assert!(
            driver.tick(soon).commands.is_empty(),
            "not time to ask again"
        );
        let later = start
            .checked_add(Duration::from_millis(600))
            .ok_or("time")?;
        assert!(matches!(
            driver.tick(later).commands.as_slice(),
            [Command::FetchTaskInventory { .. }]
        ));

        let out = driver.on_event(&listing(vec![script_item("hello")]), later);
        let [
            Command::UploadScript {
                location:
                    ScriptUploadLocation::TaskInventory {
                        task_id,
                        item_id,
                        running: true,
                        experience: None,
                    },
                target: ScriptTarget::Mono,
                source: uploaded,
            },
        ] = out.commands.as_slice()
        else {
            return Err(format!("the listed item brought {:?}", out.commands));
        };
        assert_eq!(*task_id, prim_key());
        assert_eq!(*item_id, InventoryKey::from(Uuid::from_u128(ITEM)));
        assert_eq!(uploaded.as_slice(), source.as_bytes());

        let out = driver.on_event(
            &Event::ScriptUploaded {
                new_asset: None,
                new_inventory_item: None,
                compiled: false,
                errors: vec![ScriptCompileError::parse(
                    "(2,30) Error: undefined variable x",
                )],
                running: Some(false),
            },
            later,
        );
        let lines = texts(&out.lines);
        assert_eq!(
            lines.first().map(String::as_str),
            Some(
                format!(
                    "WARN script: put \"hello\" into {}: did not compile, 1 error(s), \
                     running=false",
                    prim_key()
                )
                .as_str()
            )
        );
        let rendered = lines.get(1).ok_or("no rendered error")?;
        assert!(
            rendered.contains("state_entry() { llSay(0, x); }") && rendered.contains('^'),
            "the error is rendered under its line: {rendered}"
        );
        assert!(!driver.is_busy());
        Ok(())
    }

    #[test]
    fn a_run_by_name_finds_the_item_sets_it_running_and_reports_the_state() -> Result<(), String> {
        let mut driver = ScriptDriver::new();
        let start = Instant::now();
        let _seen = driver.on_event(&Event::ObjectAdded(Box::new(prim())), start);
        let out = driver.start(
            ScriptVerb::State {
                object: ObjectRef::Local(prim_scoped()),
                item: ItemRef::Name("hello".to_owned()),
                change: StateChange::Run,
            },
            start,
        );
        assert!(matches!(
            out.commands.as_slice(),
            [Command::FetchTaskInventory { .. }]
        ));
        let item = InventoryKey::from(Uuid::from_u128(ITEM));
        let out = driver.on_event(&listing(vec![script_item("hello")]), start);
        assert!(matches!(
            out.commands.as_slice(),
            [
                Command::SetScriptRunning { object_id, item_id, running: true },
                Command::RequestScriptRunning { .. },
            ] if *object_id == prim_key() && *item_id == item
        ));
        let out = driver.on_event(
            &Event::ScriptRunning {
                object_id: prim_key(),
                item_id: item,
                running: true,
            },
            start,
        );
        assert_eq!(
            texts(&out.lines),
            vec![format!("script: {item} in {} is running", prim_key())]
        );
        assert!(!driver.is_busy());
        Ok(())
    }

    #[test]
    fn a_missing_script_name_is_reported_not_waited_for() {
        let mut driver = ScriptDriver::new();
        let start = Instant::now();
        let _seen = driver.on_event(&Event::ObjectAdded(Box::new(prim())), start);
        let _listing = driver.start(
            ScriptVerb::State {
                object: ObjectRef::Key(prim_key()),
                item: ItemRef::Name("absent".to_owned()),
                change: StateChange::Reset,
            },
            start,
        );
        let out = driver.on_event(&listing(vec![script_item("hello")]), start);
        assert_eq!(
            texts(&out.lines),
            vec![format!(
                "WARN script: {} holds no script \"absent\"",
                prim_key()
            )]
        );
        assert!(!driver.is_busy());
    }

    #[test]
    fn a_prim_that_never_appears_times_out() -> Result<(), String> {
        let mut driver = ScriptDriver::new();
        let start = Instant::now();
        let _nothing = driver.start(
            ScriptVerb::List {
                object: ObjectRef::Key(prim_key()),
            },
            start,
        );
        let late = start.checked_add(Duration::from_secs(31)).ok_or("time")?;
        assert_eq!(
            texts(&driver.tick(late).lines),
            vec!["WARN script: timed out waiting for the prim to be in the scene".to_owned()]
        );
        assert!(!driver.is_busy());
        Ok(())
    }

    #[test]
    fn a_timed_watch_holds_the_repl_and_prints_debug_chat() -> Result<(), String> {
        let mut driver = ScriptDriver::new();
        let start = Instant::now();
        let _on = driver.start(
            ScriptVerb::Watch(WatchSpan::For(Duration::from_secs(2))),
            start,
        );
        assert!(driver.is_busy());
        let chat = Event::ChatReceived(Box::new(ChatMessage {
            from_name: "Probe".to_owned(),
            source: ChatSource::Object(prim_key()),
            owner_id: None,
            chat_type: ChatType::DebugChannel,
            audible: ChatAudible::Fully,
            position: RegionCoordinates::new(128.0, 128.0, 25.0),
            message: "Script run-time error at line 3".to_owned(),
        }));
        assert_eq!(
            texts(&driver.on_event(&chat, start).lines),
            vec![format!(
                "script: [debug] \"Probe\" ({}): Script run-time error at line 3",
                prim_key()
            )]
        );
        let late = start.checked_add(Duration::from_secs(3)).ok_or("time")?;
        assert_eq!(
            texts(&driver.tick(late).lines),
            vec!["script watch: ended".to_owned()]
        );
        assert!(!driver.is_busy());
        assert!(driver.on_event(&chat, late).lines.is_empty());
        Ok(())
    }

    #[test]
    fn a_check_renders_a_syntax_error_and_passes_a_clean_script() {
        let broken = check_script(
            Path::new("broken.lsl"),
            "default {\n state_entry() {\n",
            None,
        );
        assert!(broken.errors > 0);
        assert!(
            texts(&broken.lines)
                .iter()
                .skip(1)
                .any(|line| line.contains('^')),
            "a syntax error has a caret: {:?}",
            broken.lines
        );
        let clean = check_script(
            Path::new("clean.lsl"),
            "default\n{\n    state_entry()\n    {\n    }\n}\n",
            None,
        );
        assert_eq!(
            texts(&clean.lines),
            vec![
                "script check clean.lsl: 0 error(s), 0 warning(s) (no library table: calls and \
                 constants unchecked)"
                    .to_owned()
            ]
        );
        assert_eq!(clean.errors, 0);
    }
}
