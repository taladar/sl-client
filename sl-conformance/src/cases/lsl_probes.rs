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
//! 3. touches prim A once the probe says it is waiting, when its `drive` is
//!    `touch`;
//! 4. keeps every owner-say line of the probe's (`PROBE <id> …`) from its
//!    prims, and every `DEBUG_CHANNEL` message they shout as a `#` note,
//!    until the probe prints `end` — and a little longer, for a second
//!    prim's late lines — or [`PROBE_DEADLINE`] passes;
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
//! and `lsl-probes-xengine` (OpenSim's XEngine). `1av`. On Second Life the
//! case needs the fixtures' build location, as every case that rezzes does.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    ChatSource, ChatType, Command, DeRezDestination, Event, FolderType, InventoryFolder,
    InventoryFolderKey, InventoryKey, InventoryType, Object, ObjectKey, PrimShape, RestoreItem,
    RezScriptParams, ScopedObjectId, ScriptTarget, ScriptUploadLocation, TaskInventoryItem,
    TransactionId, Uuid, Vector, XferListing,
};
use sl_lsl_runtime::probe::{Drive, Header, Line, PREFIX, ResultFile};

use crate::context::{Session, TestContext, TestFailure, now_rfc3339};
use crate::grid::Grid;
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

/// The inventory name of a single-script probe's script.
const SINGLE_SCRIPT_NAME: &str = "Probe";

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

/// One script of a probe, read from the corpus.
#[derive(Debug)]
struct ProbeScript {
    /// Its header.
    header: Header,
    /// The inventory name it gets.
    name: String,
    /// Its source.
    source: String,
}

/// One probe, read from the corpus, its scripts in save order.
#[derive(Debug)]
struct Probe {
    /// Its id.
    id: String,
    /// Its scripts, in the order they are saved.
    scripts: Vec<ProbeScript>,
}

impl Probe {
    /// How it is set going: by touch when any script says so.
    fn drive(&self) -> Drive {
        if self
            .scripts
            .iter()
            .any(|script| script.header.drive == Drive::Touch)
        {
            Drive::Touch
        } else {
            Drive::None
        }
    }

    /// Its prims, in the order a script first names each, with the distance
    /// east of prim A each stands.
    fn prims(&self) -> Vec<(String, f32)> {
        let mut prims: Vec<(String, f32)> = vec![("A".to_owned(), 0.0)];
        for script in &self.scripts {
            if !prims.iter().any(|(label, _)| *label == script.header.prim) {
                prims.push((
                    script.header.prim.clone(),
                    script.header.distance.unwrap_or(0.0),
                ));
            }
        }
        prims
    }
}

/// The corpus directory.
fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../sl-lsl-runtime/probes")
}

/// The entries of a directory, sorted.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, TestFailure> {
    let mut paths = fs_err::read_dir(dir)
        .map_err(|error| TestFailure::State(error.to_string()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| TestFailure::State(error.to_string()))?;
    paths.sort();
    Ok(paths)
}

/// Whether a path is an `.lsl` file.
fn is_lsl(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("lsl")
}

/// A path's final component, or its stem, as text.
fn name_of(path: &Path, stem: bool) -> String {
    let part = if stem {
        path.file_stem()
    } else {
        path.file_name()
    };
    part.map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads one script and its header.
fn read_script(path: &Path, multi: bool) -> Result<ProbeScript, TestFailure> {
    let source =
        fs_err::read_to_string(path).map_err(|error| TestFailure::State(error.to_string()))?;
    let header = Header::parse(&source)
        .map_err(|error| TestFailure::State(format!("{}: {error}", path.display())))?;
    let name = if multi {
        header.script.clone().ok_or_else(|| {
            TestFailure::State(format!("{}: header `script` is missing", path.display()))
        })?
    } else {
        SINGLE_SCRIPT_NAME.to_owned()
    };
    Ok(ProbeScript {
        header,
        name,
        source,
    })
}

/// Every probe in the corpus, in id order.
fn load_corpus(root: &Path) -> Result<Vec<Probe>, TestFailure> {
    let mut probes = Vec::new();
    for area in entries(root)? {
        if !area.is_dir() || name_of(&area, false) == "results" {
            continue;
        }
        let area_name = name_of(&area, false);
        for entry in entries(&area)? {
            let id = format!("{area_name}/{}", name_of(&entry, true));
            let mut scripts = if entry.is_dir() {
                entries(&entry)?
                    .iter()
                    .filter(|path| is_lsl(path))
                    .map(|path| read_script(path, true))
                    .collect::<Result<Vec<_>, _>>()?
            } else if is_lsl(&entry) {
                vec![read_script(&entry, false)?]
            } else {
                continue;
            };
            scripts.sort_by_key(|script| script.header.order);
            probes.push(Probe { id, scripts });
        }
    }
    probes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(probes)
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
    /// The records it printed, in order.
    records: Vec<String>,
}

impl Transcript {
    /// Takes one event: a probe line or `DEBUG_CHANNEL` message from one of
    /// the probe's prims. Returns the record name of a probe line it kept.
    fn take(&mut self, id: &str, prims: &HashSet<ObjectKey>, event: &Event) -> Option<String> {
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
                quote(&chat.message)
            ));
            return None;
        }
        let line = chat.message.trim_end();
        if !line.starts_with(&format!("{PREFIX}{id} ")) {
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
        self.records.push(parsed.rec.clone());
        Some(parsed.rec)
    }
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
    let probes = load_corpus(&corpus_root())?;

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
        settle_scene(session, grid, build_position, SETTLE_WINDOW, SETTLE_IDLE).await?
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
    let results = corpus_root().join("results").join(engine.results_grid());

    let mut incomplete = Vec::new();
    for probe in &probes {
        let session = ctx.primary();
        let started = Instant::now();
        let transcript = run_probe(session, probe, engine, &base, &mut seen, trash).await?;
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
        let path = results.join(format!("{}.txt", probe.id));
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

/// Runs one probe: rezzes its prims, plants its scripts, drives it, keeps
/// what it printed and clears its prims away.
async fn run_probe(
    session: &mut Session,
    probe: &Probe,
    engine: Vm,
    base: &Vector,
    seen: &mut HashSet<ScopedObjectId>,
    trash: InventoryFolderKey,
) -> Result<Transcript, TestFailure> {
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
    let mut transcript = Transcript::default();

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
        if probe.drive() == Drive::Touch
            && collect(
                session,
                probe,
                &keys,
                &mut transcript,
                deadline,
                Until::Record("waiting"),
            )
            .await?
            && let Some(prim) = prims.get("A")
        {
            touch(session, probe, prim, &keys, &mut transcript).await?;
        }
        if !transcript.ended {
            collect(
                session,
                probe,
                &keys,
                &mut transcript,
                deadline,
                Until::Record("end"),
            )
            .await?;
        }
        if transcript.ended {
            let linger = after(LINGER);
            collect(
                session,
                probe,
                &keys,
                &mut transcript,
                linger,
                Until::Deadline,
            )
            .await?;
        }
    }

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
enum Until<'a> {
    /// The probe printing this record.
    Record(&'a str),
    /// Only the deadline.
    Deadline,
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
    until: Until<'_>,
) -> Result<bool, TestFailure> {
    if let Until::Record(wanted) = until
        && transcript.records.iter().any(|rec| rec == wanted)
    {
        return Ok(true);
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let outcome = session
        .wait_for(remaining, |event| {
            transcript
                .take(&probe.id, prims, event)
                .filter(|rec| match until {
                    Until::Record(wanted) => rec == wanted,
                    Until::Deadline => false,
                })
        })
        .await;
    match outcome {
        Ok(_) => Ok(true),
        Err(TestFailure::Timeout(_)) => Ok(false),
        Err(other) => Err(other),
    }
}

/// Touches prim A for a probe that waits for it, [`TOUCH_SETTLE`] after it
/// says so, and notes the touch in the transcript.
async fn touch(
    session: &mut Session,
    probe: &Probe,
    prim: &Object,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let settle = after(TOUCH_SETTLE);
    collect(session, probe, keys, transcript, settle, Until::Deadline).await?;
    transcript.body.push("# touch".to_owned());
    session
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
    session
        .send(Command::DegrabObject {
            local_id: prim.scoped_id(),
            surface: None,
        })
        .await
}

/// How long the case waits after a probe says it is waiting before it
/// touches.
const TOUCH_SETTLE: Duration = Duration::from_secs(1);

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

    use super::{corpus_root, load_corpus};

    #[test]
    fn the_corpus_loads_with_every_probe_in_save_order() -> Result<(), String> {
        let probes = load_corpus(&corpus_root()).map_err(|error| error.to_string())?;
        let errors = probes
            .iter()
            .find(|probe| probe.id == "errors/runtime-errors")
            .ok_or("no errors/runtime-errors probe")?;
        assert_eq!(
            errors
                .scripts
                .iter()
                .map(|script| script.name.as_str())
                .collect::<Vec<_>>(),
            [
                "Div int",
                "Mod int",
                "Div float",
                "Recurse",
                "Listener far",
                "Listener near",
                "Controller"
            ]
        );
        assert_eq!(
            errors.prims(),
            vec![
                ("A".to_owned(), 0.0),
                ("B".to_owned(), 50.0),
                ("C".to_owned(), 10.0)
            ]
        );
        Ok(())
    }
}
