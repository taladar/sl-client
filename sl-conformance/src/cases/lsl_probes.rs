//! Run the LSL probe corpus on a live grid and keep what each probe printed.
//!
//! The corpus is `sl-lsl-runtime/probes/` ([`sl_lsl_runtime::probe`] is its
//! format, the book's `tools/lsl-probes.md` its conventions). For each probe,
//! in id order, the case:
//!
//! 1. rezzes a cube per prim the probe names — prim A against the reference
//!    primitive, every other prim the `distance` its scripts' header gives
//!    east of A;
//! 2. makes each script in its prim with [`Command::RezScript`] under the
//!    name its header gives (a single-script probe's is `Probe`), in the
//!    header's save order, and uploads the source running — on OpenSim with
//!    the first line that picks the engine (`//YEngine:` or `//XEngine:`).
//!    A script whose `skip-on` names this grid is left out, with a note;
//! 3. answers each `waiting` record the probe prints with the stimulus it
//!    names ([`Stimulus`]): a touch of prim A, a line of chat on a channel,
//!    an answer to the permission question the probe's script puts, a sit
//!    on prim A, a stand, or movement controls held down for a moment, from
//!    the owner or from a second avatar — one the probe's header declares
//!    among its `drive`s;
//! 4. keeps every owner-say line of the probe's (`PROBE <id> …`) from its
//!    prims, and every `DEBUG_CHANNEL` message they shout as a `#` note,
//!    until the probe prints `end` — and a little longer, for a second
//!    prim's late lines — or [`PROBE_DEADLINE`] passes. What the grid sends
//!    the two viewers about the avatars meanwhile is noted too: permission
//!    questions, sit responses, control changes, follow cameras, animation
//!    lists and alerts, with both avatars' names put as placeholders;
//! 5. derezzes its prims and writes the transcript as a
//!    [`ResultFile`] to
//!    `sl-lsl-runtime/probes/results/<grid>/<id>.txt`.
//!
//! The case collects; it never judges what a probe printed. It fails only
//! when it could not run the corpus (a refused rez, a script the grid did
//! not name as asked); a probe that never printed `end` is written with
//! `complete: no`, and a script that does not compile is noted in its file.
//!
//! Two cases, one body: `lsl-probes` (Second Life's Mono, OpenSim's YEngine)
//! and `lsl-probes-xengine` (OpenSim's XEngine). `2av`: the secondary is
//! the other avatar a probe can ask to touch or speak, and is only that.
//! `--probe <id>` runs only the probes it names. On Second Life the case
//! needs the fixtures' build location, as every case that rezzes does.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, ChatChannel, ChatSource, ChatType, Command, ControlFlags, DeRezDestination, Event,
    FolderType, InventoryFolder, InventoryFolderKey, InventoryKey, InventoryType, Object,
    ObjectKey, PrimShape, RestoreItem, RezScriptParams, ScopedObjectId, ScriptPermissionRequest,
    ScriptPermissions, ScriptTarget, ScriptUploadLocation, TaskInventoryItem, TransactionId, Uuid,
    Vector, XferListing,
};
use sl_lsl_runtime::probe::{Action, By, Line, PREFIX, ResultFile, Stimulus};

use crate::context::{Session, TestContext, TestFailure, now_rfc3339};
use crate::grid::Grid;
use crate::probe_corpus::{Probe, corpus_root, load_corpus, result_path, select};
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, settle_scene, wait_for_own_new_object,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred,
/// whose test prim is the rez placement reference. On Second Life the avatar
/// keeps `"last"`, unless the fixtures name a build location.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long each step's confirming event may take: a rez, a listing, a
/// compile, a derez.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a probe may run before its transcript is written as
/// incomplete. The longest probe in the corpus runs about thirty seconds.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(120);

/// How long the case keeps listening after a probe's `end`, for a second
/// prim's lines that are still on their way.
const LINGER: Duration = Duration::from_secs(3);

/// How far above the reference primitive prim A is rezzed, in metres.
const LIFT_M: f32 = 1.0;

/// The script engine (virtual machine) a run is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vm {
    /// Second Life's Mono.
    Mono,
    /// OpenSim's YEngine.
    YEngine,
    /// OpenSim's XEngine.
    XEngine,
}

impl Vm {
    /// The engine a case runs on `grid`.
    const fn on(grid: Grid, xengine: bool) -> Self {
        match (is_opensim(grid), xengine) {
            (false, _) => Self::Mono,
            (true, false) => Self::YEngine,
            (true, true) => Self::XEngine,
        }
    }

    /// The first line that picks this engine for a script on OpenSim.
    const fn source_prefix(self) -> &'static str {
        match self {
            Self::Mono => "",
            Self::YEngine => "//YEngine:\n",
            Self::XEngine => "//XEngine:\n",
        }
    }

    /// Its name in a result header.
    const fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::YEngine => "YEngine",
            Self::XEngine => "XEngine",
        }
    }

    /// The results directory for this engine.
    const fn results_grid(self) -> &'static str {
        match self {
            Self::Mono => "aditi",
            Self::YEngine => "opensim-yengine",
            Self::XEngine => "opensim-xengine",
        }
    }
}

/// Runs the corpus under Second Life's Mono or OpenSim's YEngine.
#[derive(Debug)]
pub struct LslProbes;

impl GridTest for LslProbes {
    fn name(&self) -> &'static str {
        "lsl-probes"
    }

    fn description(&self) -> &'static str {
        "Run the LSL probe corpus and write each probe's transcript (Mono / YEngine)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn selects_probes(&self) -> bool {
        true
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        start_location(grid)
    }

    fn timeout(&self) -> Duration {
        CORPUS_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(run_corpus(ctx, false))
    }
}

/// Runs the corpus under OpenSim's XEngine.
#[derive(Debug)]
pub struct XEngineProbes;

impl GridTest for XEngineProbes {
    fn name(&self) -> &'static str {
        "lsl-probes-xengine"
    }

    fn description(&self) -> &'static str {
        "Run the LSL probe corpus and write each probe's transcript (XEngine)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn selects_probes(&self) -> bool {
        true
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        start_location(grid)
    }

    fn timeout(&self) -> Duration {
        CORPUS_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(run_corpus(ctx, true))
    }
}

/// The whole corpus's budget: every probe's deadline, with room for the
/// rezzes and uploads.
const CORPUS_TIMEOUT: Duration = Duration::from_secs(60 * 30);

/// The instant `wait` from now (now itself, were that past the clock's
/// range).
fn after(wait: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(wait).unwrap_or(now)
}

/// Where both cases log in.
const fn start_location(grid: Grid) -> &'static str {
    if is_opensim(grid) {
        OPENSIM_START
    } else {
        "last"
    }
}

/// Who the two avatars are, so that a note can say which one a thing is
/// about, and never name either: results are committed.
#[derive(Debug, Clone, Default)]
struct Cast {
    /// The owner's agent id.
    owner: Option<AgentKey>,
    /// The other avatar's agent id.
    other: Option<AgentKey>,
    /// The owner's first and last name.
    owner_name: (String, String),
    /// The other avatar's first and last name.
    other_name: (String, String),
}

impl Cast {
    /// Which avatar `agent` is, as a note says it.
    fn role(&self, agent: AgentKey) -> &'static str {
        if Some(agent) == self.owner {
            "owner"
        } else if Some(agent) == self.other {
            "other"
        } else {
            "someone"
        }
    }

    /// `text` with both avatars' names put as placeholders (`<first>`,
    /// `<last>` for the owner's, `<other-first>`, `<other-last>` for the
    /// other's), matched without regard to case: what a note may hold of
    /// something a grid wrote.
    fn scrub(&self, text: &str) -> String {
        let mut scrubbed = text.to_owned();
        for (name, placeholder) in [
            (&self.owner_name.0, "<first>"),
            (&self.owner_name.1, "<last>"),
            (&self.other_name.0, "<other-first>"),
            (&self.other_name.1, "<other-last>"),
        ] {
            scrubbed = replace_ignoring_case(&scrubbed, name, placeholder);
        }
        scrubbed
    }
}

/// `text` with every occurrence of `name` as a whole word, in any ASCII
/// case, replaced by `placeholder`; `text` itself for an empty `name`.
fn replace_ignoring_case(text: &str, name: &str, placeholder: &str) -> String {
    if name.is_empty() {
        return text.to_owned();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_name = name.to_ascii_lowercase();
    let is_word = |c: char| c.is_alphanumeric();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0_usize;
    let mut from = 0_usize;
    while let Some(found) = lower_text
        .get(from..)
        .and_then(|rest| rest.find(&lower_name))
    {
        let at = from.saturating_add(found);
        let end = at.saturating_add(lower_name.len());
        let before = lower_text
            .get(..at)
            .and_then(|head| head.chars().next_back());
        let after = lower_text.get(end..).and_then(|tail| tail.chars().next());
        if before.is_some_and(is_word) || after.is_some_and(is_word) {
            from = at.saturating_add(lower_name.chars().next().map_or(1, char::len_utf8));
            continue;
        }
        out.push_str(text.get(copied..at).unwrap_or_default());
        out.push_str(placeholder);
        copied = end;
        from = end;
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// What one probe's run produced.
#[derive(Debug, Default)]
struct Transcript {
    /// The `PROBE` lines and `#` notes, in arrival order.
    body: Vec<String>,
    /// Whether the probe printed its `end`.
    ended: bool,
    /// The region its `start` record named.
    region: Option<String>,
    /// Its `waiting` records, in order.
    waits: Vec<Line>,
    /// Who the avatars are.
    cast: Cast,
    /// The permission questions the probe's scripts put to the owner, not
    /// yet answered, oldest first.
    questions: VecDeque<ScriptPermissionRequest>,
    /// The permission questions they put to the other avatar, likewise.
    other_questions: VecDeque<ScriptPermissionRequest>,
    /// The animation list last noted for each avatar.
    animations: HashMap<AgentKey, String>,
}

impl Transcript {
    /// An empty transcript for the avatars `cast`.
    fn new(cast: Cast) -> Self {
        Self {
            cast,
            ..Self::default()
        }
    }

    /// Notes a permission question from one of the probe's prims, put to
    /// the avatar `by` names (`""` for the owner, `" by=other"`), and keeps
    /// it for the answer.
    fn question(&mut self, by: &str, request: &ScriptPermissionRequest) {
        let experience = if request.experience_id.is_some() {
            "set"
        } else {
            "none"
        };
        self.body.push(format!(
            "# question{by} permissions={} object={} owner={} experience={experience}",
            request.permissions.0,
            quote(&self.cast.scrub(&request.object_name)),
            quote(&self.cast.scrub(&request.object_owner)),
        ));
        if by.is_empty() {
            self.questions.push_back(request.clone());
        } else {
            self.other_questions.push_back(request.clone());
        }
    }

    /// Notes an alert the grid sent the avatar `by` names.
    fn alert(&mut self, by: &str, event: &Event) {
        match event {
            Event::AlertMessage { message, .. } => self.body.push(format!(
                "# alert{by} text={}",
                quote(&self.cast.scrub(message))
            )),
            Event::AgentAlertMessage { modal, message, .. } => self.body.push(format!(
                "# agent-alert{by} modal={} text={}",
                u8::from(*modal),
                quote(&self.cast.scrub(message))
            )),
            _ => {}
        }
    }

    /// Takes one event of the owner's session: a probe line, a
    /// `DEBUG_CHANNEL` message or another owner-say line from one of the
    /// probe's prims, or what the grid sent the owner's viewer about the
    /// avatars meanwhile — a permission question from the prims, a control
    /// change, a follow camera, an animation list that changed, an alert —
    /// which it notes. Returns the record name of a probe line it kept.
    fn take(&mut self, id: &str, prims: &HashSet<ObjectKey>, event: &Event) -> Option<String> {
        match event {
            Event::ScriptPermissionRequest(request) if prims.contains(&request.task_id) => {
                self.question("", request);
                return None;
            }
            Event::ScriptControlChange(changes) => {
                for change in changes {
                    self.body.push(format!(
                        "# script-control-change action={} controls={} pass_to_agent={}",
                        if change.action.takes_controls() {
                            "take"
                        } else {
                            "release"
                        },
                        change.controls.bits(),
                        u8::from(change.pass_to_agent)
                    ));
                }
                return None;
            }
            Event::SetFollowCamProperties {
                object_id,
                properties,
            } => {
                let values: Vec<String> = properties
                    .iter()
                    .map(|value| format!("{}={}", value.property.to_i32(), value.value))
                    .collect();
                self.body.push(format!(
                    "# follow-cam object={} {}",
                    object_role(prims, *object_id),
                    values.join(" ")
                ));
                return None;
            }
            Event::ClearFollowCamProperties { object_id } => {
                self.body.push(format!(
                    "# follow-cam-clear object={}",
                    object_role(prims, *object_id)
                ));
                return None;
            }
            Event::AvatarAnimation {
                avatar_id,
                animations,
                ..
            } => {
                let role = self.cast.role(*avatar_id);
                if role == "someone" {
                    return None;
                }
                // Only the built-in animations: an uploaded one is what the
                // account wears (an animation overrider), which changes by
                // itself and is no grid's behaviour. A source that is the
                // avatar itself is no object's.
                let mut playing: Vec<String> = animations
                    .iter()
                    .filter_map(|animation| {
                        let name = sl_anim::builtin_animation(animation.anim_id)?.name;
                        Some(match animation.source_id {
                            Some(source) if source.0 != avatar_id.0 => {
                                format!("{name}@{}", object_role(prims, source))
                            }
                            Some(_) | None => name.to_owned(),
                        })
                    })
                    .collect();
                playing.sort();
                let playing = playing.join(",");
                if self.animations.get(avatar_id) != Some(&playing) {
                    self.body.push(format!(
                        "# animations avatar={role} playing={}",
                        quote(&playing)
                    ));
                    let _previous = self.animations.insert(*avatar_id, playing);
                }
                return None;
            }
            Event::AlertMessage { .. } | Event::AgentAlertMessage { .. } => {
                self.alert("", event);
                return None;
            }
            _ => {}
        }
        let Event::ChatReceived(chat) = event else {
            return None;
        };
        let ChatSource::Object(source) = chat.source else {
            return None;
        };
        if !prims.contains(&source) {
            return None;
        }
        if chat.chat_type == ChatType::DebugChannel {
            self.body.push(format!(
                "# debug-channel: from={} text={}",
                quote(&chat.from_name),
                quote(&self.cast.scrub(&chat.message))
            ));
            return None;
        }
        let line = chat.message.trim_end();
        if !line.starts_with(&format!("{PREFIX}{id} ")) {
            if chat.chat_type == ChatType::Owner {
                self.body.push(format!(
                    "# owner-say: from={} text={}",
                    quote(&chat.from_name),
                    quote(&self.cast.scrub(&chat.message))
                ));
            }
            return None;
        }
        self.body.push(line.to_owned());
        let parsed = Line::parse(line).ok()?;
        if parsed.rec == "start" && self.region.is_none() {
            self.region = parsed.field("region").map(str::to_owned);
        }
        if parsed.rec == "end" {
            self.ended = true;
        }
        let rec = parsed.rec.clone();
        if rec == "waiting" {
            self.waits.push(parsed);
        }
        Some(rec)
    }
}

/// What `object` is to a note: one of the probe's prims (`probe`), or
/// another object.
fn object_role(prims: &HashSet<ObjectKey>, object: ObjectKey) -> &'static str {
    if prims.contains(&object) {
        "probe"
    } else {
        "other-object"
    }
}

/// A vector as a note puts it.
fn vector_note(vector: &Vector) -> String {
    format!("<{:.3}, {:.3}, {:.3}>", vector.x, vector.y, vector.z)
}

/// A note value in the probe lines' quoting.
fn quote(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// The case body both cases share.
async fn run_corpus(ctx: &mut TestContext, xengine: bool) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let engine = Vm::on(grid, xengine);
    let build_position = ctx.build_position();
    let settings = if is_opensim(grid) {
        ctx.lsl_settings().map_or_else(
            || "not given to the runner (no `lsl_settings` fixture)".to_owned(),
            str::to_owned,
        )
    } else {
        "Linden Lab's; a resident cannot set them".to_owned()
    };
    let root = corpus_root();
    let probes = load_corpus(&root)
        .and_then(|probes| select(probes, ctx.probe_selection()))
        .map_err(|error| TestFailure::State(error.to_string()))?;

    let trash = {
        let session = ctx.primary();
        let folders = session
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::InventorySkeleton(folders) => Some(folders.clone()),
                _ => None,
            })
            .await?;
        trash_folder(&folders).ok_or_else(|| {
            TestFailure::Assertion("inventory skeleton had neither a Trash nor a root".to_owned())
        })?
    };
    let (mut seen, reference) = {
        let session = ctx.primary();
        session.wait_for_region(REGION_TIMEOUT).await?;
        settle_scene(
            session,
            grid,
            build_position.clone(),
            SETTLE_WINDOW,
            SETTLE_IDLE,
        )
        .await?
    };
    let other_active_group = {
        let other = ctx.secondary().ok_or_else(|| {
            TestFailure::State("the case runs with a secondary avatar".to_owned())
        })?;
        other.wait_for_region(REGION_TIMEOUT).await?;
        // The other avatar stands where the owner does, within chat range of
        // the prims: a login that a telehub redirected (Mauve's) leaves it at
        // the hub.
        if build_position.is_some() {
            let _settled =
                settle_scene(other, grid, build_position, SETTLE_WINDOW, SETTLE_IDLE).await?;
        }
        active_group(other).await?
    };
    let Some(reference) = reference else {
        if is_opensim(grid) {
            return Err(TestFailure::Assertion(
                "no primitive appeared in the region's object stream".to_owned(),
            ));
        }
        ctx.mark_partial("landing region streamed no primitive to place against");
        return Ok(());
    };
    let base = Vector {
        x: reference.x,
        y: reference.y,
        z: reference.z + LIFT_M,
    };
    let date = now_rfc3339()?.chars().take(10).collect::<String>();
    let cast = {
        let (session, other) = ctx.primary_and_secondary().ok_or_else(|| {
            TestFailure::State("the case runs with a secondary avatar".to_owned())
        })?;
        Cast {
            owner: session.agent_id(),
            other: other.agent_id(),
            owner_name: split_name(&session.avatar_legacy_name()),
            other_name: split_name(&other.avatar_legacy_name()),
        }
    };

    let mut incomplete = Vec::new();
    for probe in &probes {
        let (session, other) = ctx.primary_and_secondary().ok_or_else(|| {
            TestFailure::State("the case runs with a secondary avatar".to_owned())
        })?;
        let started = Instant::now();
        let transcript = run_probe(
            &mut Avatars {
                session,
                other,
                other_active_group,
                cast: cast.clone(),
            },
            probe,
            engine,
            &base,
            &mut seen,
            trash,
        )
        .await?;
        let ended = transcript.ended;
        let file = ResultFile {
            header: result_header(&ResultHeader {
                grid: engine.results_grid(),
                date: &date,
                region: transcript.region.as_deref(),
                engine,
                settings: &settings,
                ended,
            }),
            body: transcript.body,
        };
        let path = result_path(&root, engine.results_grid(), &probe.id);
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)
                .map_err(|error| TestFailure::State(error.to_string()))?;
        }
        fs_err::write(&path, file.render())
            .map_err(|error| TestFailure::State(error.to_string()))?;
        tracing::info!(
            probe = probe.id,
            ended,
            lines = file.body.len(),
            seconds = started.elapsed().as_secs_f64(),
            "probe transcript written"
        );
        let metrics = ctx.metrics();
        let key = probe.id.replace(['/', '-'], "_");
        metrics.set(&format!("{key}_ended"), ended);
        metrics.set(&format!("{key}_lines"), file.body.len().to_string());
        if !ended {
            incomplete.push(probe.id.as_str());
        }
    }
    ctx.metrics().set("probes", probes.len().to_string());
    if !incomplete.is_empty() {
        ctx.mark_partial(&format!("probes that never printed `end`: {incomplete:?}"));
    }
    Ok(())
}

/// What a result header states.
struct ResultHeader<'a> {
    /// The results directory's grid.
    grid: &'a str,
    /// The run's date.
    date: &'a str,
    /// The region the probe named, if it started.
    region: Option<&'a str>,
    /// The engine.
    engine: Vm,
    /// The engine settings.
    settings: &'a str,
    /// Whether the probe printed its `end`.
    ended: bool,
}

/// A result file's header lines.
fn result_header(header: &ResultHeader<'_>) -> Vec<(String, String)> {
    let mut lines = vec![
        ("grid".to_owned(), header.grid.to_owned()),
        ("date".to_owned(), header.date.to_owned()),
        (
            "runner".to_owned(),
            "sl-conformance lsl-probes (rezzed, uploaded and read by our client)".to_owned(),
        ),
        (
            "region".to_owned(),
            header
                .region
                .unwrap_or("unknown (no start record)")
                .to_owned(),
        ),
        ("engine".to_owned(), header.engine.label().to_owned()),
        ("settings".to_owned(), header.settings.to_owned()),
    ];
    if !header.ended {
        lines.push(("complete".to_owned(), "no".to_owned()));
    }
    lines
}

/// The two avatars a probe's run has: the owner, which rezzes, plants and
/// listens, and another, which only touches or speaks when asked to.
struct Avatars<'a> {
    /// The owner's session.
    session: &'a mut Session,
    /// The other avatar's session.
    other: &'a mut Session,
    /// Whether the other avatar has an active group, which decides what a
    /// script's `llDetectedGroup` says of its touch; `None` when the grid
    /// did not say.
    other_active_group: Option<bool>,
    /// Who the two avatars are.
    cast: Cast,
}

/// A legacy name's first and last name (the last empty for a name of one
/// word).
fn split_name(legacy: &str) -> (String, String) {
    let (first, last) = legacy.split_once(' ').unwrap_or((legacy, ""));
    (first.to_owned(), last.to_owned())
}

/// Whether `session`'s avatar has an active group, as the grid answers an
/// `AgentDataUpdateRequest`; `None`, with a warning, when it does not answer.
async fn active_group(session: &mut Session) -> Result<Option<bool>, TestFailure> {
    session.send(Command::RequestAgentDataUpdate).await?;
    let answer = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ActiveGroupChanged(active) => Some(active.active_group_id.is_some()),
            _ => None,
        })
        .await;
    match answer {
        Ok(set) => Ok(Some(set)),
        Err(TestFailure::Timeout(_)) => {
            tracing::warn!("the grid did not say whether the other avatar has an active group");
            Ok(None)
        }
        Err(other) => Err(other),
    }
}

/// Runs one probe: rezzes its prims, plants its scripts, drives it, keeps
/// what it printed and clears its prims away.
async fn run_probe(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    engine: Vm,
    base: &Vector,
    seen: &mut HashSet<ScopedObjectId>,
    trash: InventoryFolderKey,
) -> Result<Transcript, TestFailure> {
    let session = &mut *avatars.session;
    let mut prims: BTreeMap<String, Object> = BTreeMap::new();
    for (label, distance) in probe.prims() {
        let position = Vector {
            x: base.x + distance,
            y: base.y,
            z: base.z,
        };
        session
            .send(Command::RezObject {
                shape: PrimShape::cube(position.clone()),
                group_id: None,
            })
            .await?;
        let object = wait_for_own_new_object(session, seen, STEP_TIMEOUT)
            .await?
            .map_err(|reason| {
                TestFailure::Assertion(format!(
                    "{}: prim {label} was not rezzed at {:.0}/{:.0}/{:.0}: {reason}",
                    probe.id, position.x, position.y, position.z
                ))
            })?;
        seen.insert(object.scoped_id());
        prims.insert(label, object);
    }
    let keys: HashSet<ObjectKey> = prims.values().map(|object| object.full_id).collect();
    let mut transcript = Transcript::new(avatars.cast.clone());

    let mut planted = true;
    for script in &probe.scripts {
        if script.header.skipped_on(engine.results_grid()) {
            transcript.body.push(format!(
                "# skipped: script {} (skip-on: {})",
                quote(&script.name),
                engine.results_grid()
            ));
            continue;
        }
        let prim = prims.get(&script.header.prim).ok_or_else(|| {
            TestFailure::State(format!("{}: no prim {}", probe.id, script.header.prim))
        })?;
        let item = new_script(
            session,
            prim,
            &script.name,
            &mut Watch {
                id: &probe.id,
                prims: &keys,
                transcript: &mut transcript,
            },
        )
        .await?;
        let source = format!("{}{}", engine.source_prefix(), script.source);
        session
            .send(Command::UploadScript {
                location: ScriptUploadLocation::TaskInventory {
                    task_id: prim.full_id,
                    item_id: item,
                    running: true,
                    experience: None,
                },
                target: ScriptTarget::Mono,
                source: source.into_bytes(),
            })
            .await?;
        let id = &probe.id;
        let compiled = session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ScriptUploaded {
                    compiled, errors, ..
                } => Some(Ok((*compiled, errors.clone()))),
                Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                _ => {
                    let _kept = transcript.take(id, &keys, event);
                    None
                }
            })
            .await?;
        match compiled {
            Ok((true, _)) => {}
            Ok((false, errors)) => {
                for error in errors {
                    transcript.body.push(format!(
                        "# compile error in {}: {}",
                        quote(&script.name),
                        quote(&error.raw)
                    ));
                }
                planted = false;
                break;
            }
            Err(reason) => {
                return Err(TestFailure::Assertion(format!(
                    "{}: uploading {} failed: {reason}",
                    probe.id, script.name
                )));
            }
        }
    }

    if planted {
        let deadline = after(PROBE_DEADLINE);
        let mut answered = 0_usize;
        while collect(
            avatars.session,
            probe,
            &keys,
            &mut transcript,
            deadline,
            Until::Stimulus(answered),
        )
        .await?
        {
            let Some(wait) = transcript.waits.get(answered).cloned() else {
                break;
            };
            answered = answered.saturating_add(1);
            let stimulus = Stimulus::from_waiting(&wait)
                .map_err(|reason| TestFailure::Assertion(format!("{}: {reason}", probe.id)))?;
            drain_other(avatars.other, &keys, &mut transcript).await?;
            stimulate(avatars, probe, &prims, &keys, &mut transcript, &stimulus).await?;
        }
        if transcript.ended {
            let linger = after(LINGER);
            collect(
                avatars.session,
                probe,
                &keys,
                &mut transcript,
                linger,
                Until::Deadline,
            )
            .await?;
        }
        drain_other(avatars.other, &keys, &mut transcript).await?;
    }

    let session = &mut *avatars.session;
    let local_ids: Vec<ScopedObjectId> = prims.values().map(Object::scoped_id).collect();
    session
        .send(Command::DerezObjects {
            local_ids: local_ids.clone(),
            destination: DeRezDestination::Trash(trash),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    let mut gone: HashSet<ScopedObjectId> = HashSet::new();
    while gone.len() < local_ids.len() {
        let removed = session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ObjectRemoved { local_id, .. } if local_ids.contains(local_id) => {
                    Some(*local_id)
                }
                _ => None,
            })
            .await?;
        gone.insert(removed);
    }
    Ok(transcript)
}

/// What ends a [`collect`].
#[derive(Debug, Clone, Copy)]
enum Until {
    /// The probe printing its `end`, or a `waiting` record beyond the first
    /// this many, which have been answered.
    Stimulus(usize),
    /// Only the deadline.
    Deadline,
}

impl Until {
    /// Whether `transcript` has come this far.
    const fn reached(self, transcript: &Transcript) -> bool {
        match self {
            Self::Stimulus(answered) => transcript.ended || transcript.waits.len() > answered,
            Self::Deadline => false,
        }
    }
}

/// Keeps the probe's lines until `until` or `deadline`, whichever is first;
/// whether `until` came is the answer — at once, for a record the probe
/// printed already (while the case waited for something else). The
/// deadline passing is not a failure.
async fn collect(
    session: &mut Session,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    deadline: Instant,
    until: Until,
) -> Result<bool, TestFailure> {
    if until.reached(transcript) {
        return Ok(true);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let outcome = session
        .wait_for(remaining, |event| {
            let _kept = transcript.take(&probe.id, prims, event);
            until.reached(transcript).then_some(())
        })
        .await;
    match outcome {
        Ok(()) => Ok(true),
        Err(TestFailure::Timeout(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

/// Gives the stimulus a probe asked for, [`STIMULUS_SETTLE`] after it asked,
/// and notes it in the transcript: a touch of prim A or a line of chat, from
/// the owner or the other avatar.
async fn stimulate(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    prims: &BTreeMap<String, Object>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    stimulus: &Stimulus,
) -> Result<(), TestFailure> {
    let drive = stimulus.action.drive();
    if !probe.drives().contains(&drive) {
        return Err(TestFailure::Assertion(format!(
            "{}: asked for {drive:?}, which its header's `drive` does not declare",
            probe.id
        )));
    }
    let settle = after(STIMULUS_SETTLE);
    collect(
        avatars.session,
        probe,
        keys,
        transcript,
        settle,
        Until::Deadline,
    )
    .await?;
    let by = match stimulus.by {
        By::Owner => "",
        By::Other => " by=other",
    };
    match &stimulus.action {
        Action::Answer { grant } => {
            return answer(avatars, stimulus.by, probe, keys, transcript, *grant).await;
        }
        Action::Sit => return sit(avatars, stimulus.by, probe, prims, keys, transcript).await,
        Action::Control { keys: held } => {
            transcript.body.push(format!(
                "# control{by} keys={held} held={}s",
                CONTROL_HOLD.as_secs()
            ));
            let controls = ControlFlags::from_bits(*held);
            match stimulus.by {
                By::Owner => avatars.session.send(Command::SetControls(controls)).await?,
                By::Other => avatars.other.send(Command::SetControls(controls)).await?,
            }
            collect(
                avatars.session,
                probe,
                keys,
                transcript,
                after(CONTROL_HOLD),
                Until::Deadline,
            )
            .await?;
            let released = Command::SetControls(ControlFlags::empty());
            return match stimulus.by {
                By::Owner => avatars.session.send(released).await,
                By::Other => avatars.other.send(released).await,
            };
        }
        Action::Touch | Action::Chat { .. } | Action::Stand => {}
    }
    let actor = match stimulus.by {
        By::Owner => &mut *avatars.session,
        By::Other => &mut *avatars.other,
    };
    match &stimulus.action {
        Action::Stand => {
            transcript.body.push(format!("# stand{by}"));
            actor.send(Command::Stand).await
        }
        Action::Answer { .. } | Action::Sit | Action::Control { .. } => Ok(()),
        Action::Touch => {
            let prim = prims
                .get("A")
                .ok_or_else(|| TestFailure::State(format!("{}: no prim A", probe.id)))?;
            let group = match (stimulus.by, avatars.other_active_group) {
                (By::Owner, _) => "",
                (By::Other, Some(true)) => " active_group=set",
                (By::Other, Some(false)) => " active_group=none",
                (By::Other, None) => " active_group=unknown",
            };
            transcript.body.push(format!("# touch{by}{group}"));
            actor
                .send(Command::GrabObject {
                    local_id: prim.scoped_id(),
                    grab_offset: Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    surface: None,
                })
                .await?;
            actor
                .send(Command::DegrabObject {
                    local_id: prim.scoped_id(),
                    surface: None,
                })
                .await
        }
        Action::Chat { channel, text } => {
            transcript
                .body
                .push(format!("# chat{by} channel={channel} text={}", quote(text)));
            actor
                .send(Command::Chat {
                    message: text.clone(),
                    chat_type: ChatType::Normal,
                    channel: ChatChannel(*channel),
                })
                .await
        }
    }
}

/// How long the case waits after a probe asks for a stimulus before it
/// gives it.
const STIMULUS_SETTLE: Duration = Duration::from_secs(1);

/// How long a `control` stimulus holds its keys down.
const CONTROL_HOLD: Duration = Duration::from_secs(1);

/// How long an `answer` stimulus waits for the question it answers, and a
/// `sit` for the grid's sit response.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a look at the other avatar's events waits for more.
const DRAIN_WINDOW: Duration = Duration::from_millis(200);

/// Notes what the grid sent the other avatar's viewer since the last look:
/// the permission questions from the probe's prims (kept for an answer by
/// that avatar) and alerts. Its other events are dropped.
async fn drain_other(
    other: &mut Session,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let outcome = other
        .wait_for(DRAIN_WINDOW, |event| {
            match event {
                Event::ScriptPermissionRequest(request) if prims.contains(&request.task_id) => {
                    transcript.question(" by=other", request);
                }
                Event::AlertMessage { .. } | Event::AgentAlertMessage { .. } => {
                    transcript.alert(" by=other", event);
                }
                _ => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Answers the permission question the probe's script put to the avatar
/// `by` names with `grant`, as it is, and notes it — or notes that no
/// question came within [`ANSWER_TIMEOUT`].
async fn answer(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    grant: i32,
) -> Result<(), TestFailure> {
    let deadline = after(ANSWER_TIMEOUT);
    let request = match by {
        By::Owner => {
            if transcript.questions.is_empty() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let outcome = avatars
                    .session
                    .wait_for(remaining, |event| {
                        let _kept = transcript.take(&probe.id, prims, event);
                        (!transcript.questions.is_empty()).then_some(())
                    })
                    .await;
                match outcome {
                    Ok(()) | Err(TestFailure::Timeout(_)) => {}
                    Err(other) => return Err(other),
                }
            }
            transcript.questions.pop_front()
        }
        By::Other => {
            while transcript.other_questions.is_empty() && Instant::now() < deadline {
                drain_other(avatars.other, prims, transcript).await?;
            }
            transcript.other_questions.pop_front()
        }
    };
    let label = match by {
        By::Owner => "",
        By::Other => " by=other",
    };
    let Some(request) = request else {
        transcript
            .body
            .push(format!("# answer{label} grant={grant} question=none"));
        return Ok(());
    };
    transcript
        .body
        .push(format!("# answer{label} grant={grant}"));
    let command = Command::AnswerScriptPermissions {
        task_id: request.task_id,
        item_id: request.item_id,
        permissions: ScriptPermissions(grant),
        experience_id: request.experience_id,
    };
    match by {
        By::Owner => avatars.session.send(command).await,
        By::Other => avatars.other.send(command).await,
    }
}

/// Sits the avatar `by` names on prim A and notes the grid's sit response,
/// or that none came within [`ANSWER_TIMEOUT`].
async fn sit(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    prims: &BTreeMap<String, Object>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let target = prims
        .get("A")
        .ok_or_else(|| TestFailure::State(format!("{}: no prim A", probe.id)))?
        .full_id;
    let (actor, label) = match by {
        By::Owner => (&mut *avatars.session, ""),
        By::Other => (&mut *avatars.other, " by=other"),
    };
    actor
        .send(Command::Sit {
            target,
            offset: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        })
        .await?;
    let outcome = actor
        .wait_for(ANSWER_TIMEOUT, |event| match event {
            Event::SitResult {
                sit_object,
                autopilot,
                sit_position,
                sit_rotation,
                camera_eye_offset,
                camera_at_offset,
                force_mouselook,
            } if *sit_object == target => Some(format!(
                "# sit{label} sit_position={} sit_rotation=<{:.3}, {:.3}, {:.3}, {:.3}> \
                 camera_eye={} camera_at={} force_mouselook={} autopilot={}",
                vector_note(sit_position),
                sit_rotation.x,
                sit_rotation.y,
                sit_rotation.z,
                sit_rotation.s,
                vector_note(camera_eye_offset),
                vector_note(camera_at_offset),
                u8::from(*force_mouselook),
                u8::from(*autopilot)
            )),
            _ => {
                match by {
                    By::Owner => {
                        let _kept = transcript.take(&probe.id, keys, event);
                    }
                    By::Other => match event {
                        Event::ScriptPermissionRequest(request)
                            if keys.contains(&request.task_id) =>
                        {
                            transcript.question(label, request);
                        }
                        Event::AlertMessage { .. } | Event::AgentAlertMessage { .. } => {
                            transcript.alert(label, event);
                        }
                        _ => {}
                    },
                }
                None
            }
        })
        .await;
    match outcome {
        Ok(note) => transcript.body.push(note),
        Err(TestFailure::Timeout(_)) => {
            transcript.body.push(format!("# sit{label} response=none"));
        }
        Err(other) => return Err(other),
    }
    Ok(())
}

/// Waits for `prim`'s task inventory listing, keeping the probe's lines
/// that arrive meanwhile.
async fn listing(
    session: &mut Session,
    prim: &Object,
    watch: &mut Watch<'_>,
) -> Result<Vec<TaskInventoryItem>, TestFailure> {
    let task = prim.full_id;
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::TaskInventoryContents {
                task: got, items, ..
            } if *got == task => Some(Ok(items.clone())),
            Event::XferDecodeFailed {
                file: XferListing::TaskInventory { task: got, .. },
                error,
                ..
            } if *got == task => Some(Err(error.clone())),
            other => {
                let _kept = watch.transcript.take(watch.id, watch.prims, other);
                None
            }
        })
        .await?
        .map_err(|error| {
            TestFailure::Assertion(format!(
                "the prim's task inventory listing failed to parse: {error}"
            ))
        })
}

/// What a wait inside a probe's run keeps: the probe's lines from its
/// prims.
struct Watch<'a> {
    /// The probe's id.
    id: &'a str,
    /// Its prims.
    prims: &'a HashSet<ObjectKey>,
    /// Where its lines go.
    transcript: &'a mut Transcript,
}

/// Makes a script named `name` in `prim` and returns its task item id, once
/// the prim's listing shows it under that name.
async fn new_script(
    session: &mut Session,
    prim: &Object,
    name: &str,
    watch: &mut Watch<'_>,
) -> Result<InventoryKey, TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    session
        .send(Command::RezScript {
            target: prim.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: false,
                item: RestoreItem::new_script(agent, prim.full_id, name, Uuid::new_v4()),
            }),
        })
        .await?;
    let started = Instant::now();
    loop {
        session
            .send(Command::FetchTaskInventory {
                target: prim.scoped_id(),
                held_serial: None,
            })
            .await?;
        let listing = listing(session, prim, watch).await?;
        if let Some(item) = listing
            .iter()
            .find(|item| item.inv_type == InventoryType::Script && item.name == name)
        {
            return Ok(item.item_id);
        }
        if started.elapsed() >= STEP_TIMEOUT {
            let names: Vec<&str> = listing.iter().map(|item| item.name.as_str()).collect();
            return Err(TestFailure::Assertion(format!(
                "no script named {name:?} appeared in the prim; it holds {names:?}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The Trash folder of a login skeleton, or its root when it has none.
fn trash_folder(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == FolderType::Trash.to_code())
        .or_else(|| folders.iter().find(|folder| folder.parent_id.is_none()))
        .map(|folder| folder.folder_id)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Cast, replace_ignoring_case, split_name};

    #[test]
    fn a_name_is_replaced_as_a_whole_word_in_any_case() {
        assert_eq!(
            replace_ignoring_case("Knob Label, knob label: KNOB", "Knob", "<first>"),
            "<first> Label, <first> label: <first>"
        );
        assert_eq!(
            replace_ignoring_case("Knobs knobby aknob knob", "knob", "<first>"),
            "Knobs knobby aknob <first>"
        );
        assert_eq!(replace_ignoring_case("anything", "", "<first>"), "anything");
    }

    #[test]
    fn both_avatars_names_become_placeholders() {
        let cast = Cast {
            owner_name: split_name("Knob Label"),
            other_name: split_name("Lever Dial"),
            ..Cast::default()
        };
        assert_eq!(
            cast.scrub("Knob Label offered Lever Dial a seat"),
            "<first> <last> offered <other-first> <other-last> a seat"
        );
        assert_eq!(split_name("Single"), ("Single".to_owned(), String::new()));
    }
}
