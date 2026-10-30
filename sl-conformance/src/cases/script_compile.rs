//! A census of script compilation: what each grid answers a script upload
//! whose source does not compile, per compile target and engine, and what a
//! failed compile does to the item and to the script already running in it.
//!
//! A script is compiled by its upload — the viewer POSTs the source to
//! `UpdateScriptTask` (or `UpdateScriptAgent`) and the simulator answers
//! `compiled` and an `errors` array. There is no dry run, so everything the
//! viewer's error list and the fake grid's compiler must reproduce is in
//! those answers. One avatar; per backend (Second Life: the `mono`, `lsl2`
//! and `luau` targets; OpenSim: YEngine and XEngine, chosen by the source's
//! first line as the probe corpus chooses it, and the `luau` target under
//! YEngine) the census:
//!
//! 1. rezzes a cube, makes a script in it (`RezScript`) and uploads a valid
//!    one, running — it says `CMP <backend> v1 start` and answers a line on
//!    channel 7 from its owner with `CMP <backend> v1 alive`;
//! 2. uploads each broken source of `BROKEN`, running, and records the
//!    answer verbatim: `compiled`, every error (ids scrubbed) and where our
//!    own parse puts the first, what the answer names as `new_asset`, and
//!    which asset the item is listed with afterwards;
//! 3. after the first failure, the one over the running v1: whether the
//!    script runs (`GetScriptRunning`), whether v1 still answers or was
//!    restarted, and which source the item holds now, read back over the
//!    UDP transfer;
//! 4. uploads one call each to the five functions no served `LSLSyntax`
//!    document lists (`UNSERVED`), for whether the grid still compiles them;
//! 5. uploads source of growing size (`SIZES`) for the limit, and one
//!    whose constants are large;
//! 6. uploads a valid script with `is_script_running` false, then true, and
//!    then the same source again over the running script;
//! 7. makes a script in the agent's inventory and uploads a broken then a
//!    valid source over `UpdateScriptAgent`.
//!
//! XEngine answers no upload to a script that holds a running instance, and
//! each unanswered upload keeps one of OpenSim's HTTP handlers; so under
//! XEngine every leg but the two over a running script gets a script of its
//! own, and after any unanswered upload the next leg does (`Run::step`).
//!
//! The `luau` backend has its own short list (`LUAU`): a valid Luau body, a
//! broken one, and LSL source under the Luau target.
//!
//! What each live grid was measured to answer is written up in
//! `book/src/gridspec/lsl.md` § Compiling and held below. `1av`, live grids
//! only: the fake grid compiles nothing yet
//! (`server-fake-grid-script-compile-on-upload`).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AssetKey, AssetType, ChatChannel, ChatSource, ChatType, Command, DeRezDestination, Event,
    FolderType, InventoryFolder, InventoryFolderKey, InventoryKey, InventoryType, NewInventoryItem,
    Object, PrimShape, RestoreItem, RezScriptParams, ScopedObjectId, ScriptCompileError,
    ScriptLanguage, ScriptTarget, ScriptUploadLocation, TaskInventoryItem, TransactionId, Uuid,
    Vector, WearableType, XferListing,
};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::Answer;
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, secs_metric, settle_scene, wait_for_own_new_object,
};

/// Where the measurements are written down.
const SOURCE: &str = "book/src/gridspec/lsl.md § Compiling (script-compile, 2026-10-25)";

/// The OpenSim start location: the "Default Region" centre, whose test prim
/// is the rez reference.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long a rez, a listing or a derez may take.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long one upload's answer may take; a large source compiles slowly.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the census listens for what a script says after an upload or a
/// ping.
const LISTEN: Duration = Duration::from_secs(5);

/// The channel the valid script listens to its owner on — positive, since
/// Second Life passes no avatar's chat on a negative one.
const PING_CHANNEL: i32 = 7;

/// How far above the reference primitive the cube is rezzed, in metres.
const LIFT_M: f32 = 1.0;

/// How many of an answer's errors are recorded one by one.
const ERRORS_KEPT: usize = 5;

/// What the census compiles on: a compile target, and on OpenSim an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// Second Life's Mono target.
    Mono,
    /// Second Life's LSL2 (LSO) target.
    Lsl2,
    /// The Luau target (YEngine on OpenSim, which knows no Luau).
    Luau,
    /// OpenSim's YEngine.
    YEngine,
    /// OpenSim's XEngine.
    XEngine,
}

impl Backend {
    /// The backends a grid is measured on, in order.
    const fn on(grid: Grid) -> &'static [Self] {
        if is_opensim(grid) {
            &[Self::YEngine, Self::XEngine, Self::Luau]
        } else {
            &[Self::Mono, Self::Lsl2, Self::Luau]
        }
    }

    /// The metric prefix.
    const fn key(self) -> &'static str {
        match self {
            Self::Mono => "mono",
            Self::Lsl2 => "lsl2",
            Self::Luau => "luau",
            Self::YEngine => "yengine",
            Self::XEngine => "xengine",
        }
    }

    /// The compile target the upload names.
    const fn target(self) -> ScriptTarget {
        match self {
            Self::Mono | Self::YEngine | Self::XEngine => ScriptTarget::Mono,
            Self::Lsl2 => ScriptTarget::Lsl2,
            Self::Luau => ScriptTarget::Luau,
        }
    }

    /// Whether every leg is uploaded to a script of its own (see
    /// [`Run::step`]).
    const fn fresh_each(self) -> bool {
        matches!(self, Self::XEngine)
    }

    /// The line an OpenSim source opens with to pick its engine.
    const fn prefix(self, grid: Grid) -> &'static str {
        match self {
            Self::YEngine => "//YEngine:\n",
            Self::XEngine => "//XEngine:\n",
            Self::Luau if is_opensim(grid) => "//YEngine:\n",
            Self::Mono | Self::Lsl2 | Self::Luau => "",
        }
    }
}

/// One source a leg uploads, by the leg's name.
struct Leg {
    /// The leg's name in the metrics.
    name: &'static str,
    /// The source.
    source: &'static str,
}

/// The LSL sources that do not compile, each with one fault (or three, for
/// `three_errors`). The fault's place is in the comment beside it, one-based.
const BROKEN: &[Leg] = &[
    // A missing expression: the `;` at 5:21.
    Leg {
        name: "syntax",
        source: "default\n{\n    state_entry()\n    {\n        integer x = ;\n    }\n}\n",
    },
    // A string assigned to an integer: the `"a"` at 5:21.
    Leg {
        name: "type_mismatch",
        source: "default\n{\n    state_entry()\n    {\n        integer x = \"a\";\n    }\n}\n",
    },
    // An undeclared name: `y` at 5:9.
    Leg {
        name: "undeclared",
        source: "default\n{\n    state_entry()\n    {\n        y = 1;\n    }\n}\n",
    },
    // A function nobody defined: `llNope` at 5:9.
    Leg {
        name: "unknown_function",
        source: "default\n{\n    state_entry()\n    {\n        llNope();\n    }\n}\n",
    },
    // A path without a return: the function's closing brace at 4:1.
    Leg {
        name: "missing_return",
        source: "integer f()\n{\n    if (TRUE) return 1;\n}\ndefault\n{\n    state_entry()\n    {\n    }\n}\n",
    },
    // A global declared twice: the second `x` at 2:9.
    Leg {
        name: "duplicate_global",
        source: "integer x;\ninteger x;\ndefault\n{\n    state_entry()\n    {\n    }\n}\n",
    },
    // Three semantic faults: 5:21, 6:9 and 7:9.
    Leg {
        name: "three_errors",
        source: "default\n{\n    state_entry()\n    {\n        integer x = \"a\";\n        y = 1;\n        llNope();\n    }\n}\n",
    },
    // Two syntax faults: 5:21 and 6:21.
    Leg {
        name: "two_syntax_errors",
        source: "default\n{\n    state_entry()\n    {\n        integer x = ;\n        integer z = ;\n    }\n}\n",
    },
    // Nothing at all.
    Leg {
        name: "empty",
        source: "",
    },
    // A state but no `default`.
    Leg {
        name: "no_default",
        source: "state other\n{\n    state_entry()\n    {\n    }\n}\n",
    },
    // A string that never ends, from 5:20.
    Leg {
        name: "unterminated_string",
        source: "default\n{\n    state_entry()\n    {\n        string s = \"abc;\n    }\n}\n",
    },
    // Three two-byte characters before the fault: the `;` at character 39,
    // byte 42 of line 5.
    Leg {
        name: "multibyte_column",
        source: "default\n{\n    state_entry()\n    {\n        string s = \"äöü\"; integer x = ;\n    }\n}\n",
    },
    // Two tabs before the fault: the `;` at character 15 of line 5.
    Leg {
        name: "tab_column",
        source: "default\n{\n    state_entry()\n    {\n\t\tinteger x = ;\n    }\n}\n",
    },
    // `syntax` with CR LF line ends: the `;` at 5:21.
    Leg {
        name: "crlf",
        source: "default\r\n{\r\n    state_entry()\r\n    {\r\n        integer x = ;\r\n    }\r\n}\r\n",
    },
    // A jump to a label that is not there: `nowhere` at 5:14.
    Leg {
        name: "unknown_label",
        source: "default\n{\n    state_entry()\n    {\n        jump nowhere;\n    }\n}\n",
    },
    // A state change to a state that is not there: `other` at 5:15.
    Leg {
        name: "unknown_state",
        source: "default\n{\n    state_entry()\n    {\n        state other;\n    }\n}\n",
    },
];

/// The Luau target's sources: a valid body, a broken one (`local x =` with
/// nothing after it, line 1), one broken mid-line (the second `=` at 2:11),
/// and the valid LSL script.
const LUAU: &[Leg] = &[
    Leg {
        name: "luau_valid",
        source: "-- sl-conformance script-compile\nlocal n = 1\n",
    },
    Leg {
        name: "luau_syntax",
        source: "local x =\n",
    },
    Leg {
        name: "luau_syntax_mid",
        source: "local a = 1\nlocal b = = 2\nlocal c = 3\n",
    },
    Leg {
        name: "lsl_source",
        source: "default\n{\n    state_entry()\n    {\n    }\n}\n",
    },
];

/// One call each to the five functions tailslide's `builtins.txt` lists and
/// no `LSLSyntax` document Second Life serves does — whether the grid still
/// compiles them decides whether our library table carries them. The call
/// sits in a `touch_start` nobody raises, so a script that compiles runs
/// nothing.
const UNSERVED: &[Leg] = &[
    Leg {
        name: "unserved_point_at",
        source: "default\n{\n    touch_start(integer n)\n    {\n        llPointAt(<1.0, 2.0, 3.0>);\n    }\n}\n",
    },
    Leg {
        name: "unserved_stop_point_at",
        source: "default\n{\n    touch_start(integer n)\n    {\n        llStopPointAt();\n    }\n}\n",
    },
    Leg {
        name: "unserved_remote_load_script",
        source: "default\n{\n    touch_start(integer n)\n    {\n        llRemoteLoadScript(NULL_KEY, \"x\", FALSE, 0);\n    }\n}\n",
    },
    Leg {
        name: "unserved_clear_experience_permissions",
        source: "default\n{\n    touch_start(integer n)\n    {\n        llClearExperiencePermissions(NULL_KEY);\n    }\n}\n",
    },
    Leg {
        name: "unserved_get_experience_list",
        source: "default\n{\n    touch_start(integer n)\n    {\n        llOwnerSay(llList2CSV(llGetExperienceList(NULL_KEY)));\n    }\n}\n",
    },
];

/// The sizes of valid source the limit legs upload, in bytes: under and
/// over 64 KiB and the reference editor's 256 KiB (`max_length` 262144).
const SIZES: &[(&str, usize)] = &[
    ("size_64k_under", 65_000),
    ("size_64k_over", 66_000),
    ("size_256k_over", 262_200),
    ("size_1m", 1_048_600),
];

/// How many functions the `large_code` leg defines, each returning a string
/// of [`LARGE_CODE_STRING`] characters — about 60 KB of constants, over an
/// LSO script's 16 KiB and near a Mono script's 64 KiB.
const LARGE_CODE_FUNCTIONS: usize = 600;

/// The length of each `large_code` function's string.
const LARGE_CODE_STRING: usize = 100;

/// The valid script `version` of `backend`: says it started, and answers its
/// owner on [`PING_CHANNEL`].
fn valid_source(backend: Backend, version: &str) -> String {
    format!(
        "default\n{{\n    state_entry()\n    {{\n        llOwnerSay(\"CMP {key} {version} start\");\n        llListen({PING_CHANNEL}, \"\", llGetOwner(), \"\");\n    }}\n    listen(integer channel, string name, key id, string message)\n    {{\n        llOwnerSay(\"CMP {key} {version} alive \" + message);\n    }}\n}}\n",
        key = backend.key()
    )
}

/// A valid script of `size` bytes: `syntax`'s fixed body padded with a
/// comment.
fn sized_source(size: usize) -> String {
    let body = "default\n{\n    state_entry()\n    {\n    }\n}\n";
    let mut source = String::with_capacity(size);
    let line = format!("// {}\n", "x".repeat(76));
    while source
        .len()
        .saturating_add(line.len())
        .saturating_add(body.len())
        <= size
    {
        source.push_str(&line);
    }
    while source.len().saturating_add(body.len()) < size {
        source.push(' ');
    }
    source.push_str(body);
    source
}

/// A valid script whose constants are large: [`LARGE_CODE_FUNCTIONS`]
/// functions each returning a distinct string.
fn large_code_source() -> String {
    let mut source = String::new();
    for index in 0..LARGE_CODE_FUNCTIONS {
        let number = format!("{index:04}");
        let text: String = number.chars().cycle().take(LARGE_CODE_STRING).collect();
        source.push_str("string f");
        source.push_str(&index.to_string());
        source.push_str("()\n{\n    return \"");
        source.push_str(&text);
        source.push_str("\";\n}\n");
    }
    source.push_str("default\n{\n    state_entry()\n    {\n        llOwnerSay(f0());\n    }\n}\n");
    source
}

/// Census of script compilation on each live grid.
#[derive(Debug)]
pub struct ScriptCompile;

impl GridTest for ScriptCompile {
    fn name(&self) -> &'static str {
        "script-compile"
    }

    fn description(&self) -> &'static str {
        "What each grid answers a script that does not compile, per target and engine, and what \
         a failed compile does to the item and the running script"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(3600)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold(grid, &metrics))
        })
    }
}

/// The census body.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let (trash, root) = {
        let session = ctx.primary();
        let folders = session
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::InventorySkeleton(folders) => Some(folders.clone()),
                _ => None,
            })
            .await?;
        let root = folders
            .iter()
            .find(|folder| folder.parent_id.is_none())
            .map(|folder| folder.folder_id)
            .ok_or_else(|| TestFailure::Assertion("the skeleton has no root".to_owned()))?;
        (
            folder_of_type(&folders, FolderType::Trash).unwrap_or(root),
            root,
        )
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
    let position = Vector {
        x: reference.x,
        y: reference.y,
        z: reference.z + LIFT_M,
    };
    let session = ctx.primary();
    for backend in Backend::on(grid) {
        let mut run = Run {
            session: &mut *session,
            metrics: &mut *metrics,
            grid,
            backend: *backend,
            heard: Vec::new(),
            stalled: None,
        };
        run.backend_legs(&position, &mut seen, trash, root).await?;
    }
    Ok(())
}

/// One backend's legs.
struct Run<'a> {
    /// The avatar's session.
    session: &'a mut Session,
    /// Where the answers go.
    metrics: &'a mut Metrics,
    /// The grid.
    grid: Grid,
    /// The backend.
    backend: Backend,
    /// The `CMP` lines the script said since the last time they were read.
    heard: Vec<String>,
    /// The last upload the grid did not answer, until a fresh script is
    /// made for the next.
    stalled: Option<String>,
}

/// The cube's script the legs upload to.
struct Held {
    /// Its item id.
    item: InventoryKey,
    /// The asset it was last listed with.
    asset: Option<Uuid>,
    /// How many scripts the cube has been given.
    made: u32,
}

/// One upload's answer.
#[derive(Debug, Default)]
struct Answered {
    /// `compiled`.
    compiled: bool,
    /// The errors.
    errors: Vec<ScriptCompileError>,
    /// The asset the answer names.
    new_asset: Option<Uuid>,
    /// The running flag the answer echoes.
    running: Option<bool>,
}

impl Run<'_> {
    /// A metric key of this backend's.
    fn key(&self, leg: &str, field: &str) -> String {
        format!("{}_{leg}_{field}", self.backend.key())
    }

    /// Rezzes the cube, runs every leg in it and in the agent's inventory,
    /// and clears both away.
    async fn backend_legs(
        &mut self,
        position: &Vector,
        seen: &mut HashSet<ScopedObjectId>,
        trash: InventoryFolderKey,
        root: InventoryFolderKey,
    ) -> Result<(), TestFailure> {
        self.session
            .send(Command::RezObject {
                shape: PrimShape::cube(position.clone()),
                group_id: None,
            })
            .await?;
        let cube = match wait_for_own_new_object(self.session, seen, STEP_TIMEOUT).await? {
            Ok(cube) => cube,
            Err(reason) => {
                return Err(TestFailure::Assertion(format!(
                    "the cube was not rezzed at {:.0}/{:.0}/{:.0}: {reason}",
                    position.x, position.y, position.z
                )));
            }
        };
        seen.insert(cube.scoped_id());
        let outcome = self.task_legs(&cube).await;
        self.session
            .send(Command::DerezObjects {
                local_ids: vec![cube.scoped_id()],
                destination: DeRezDestination::Trash(trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
        let gone = cube.scoped_id();
        self.session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ObjectRemoved { local_id, .. } if *local_id == gone => Some(()),
                _ => None,
            })
            .await?;
        outcome?;
        self.agent_legs(root).await
    }

    /// The legs in the cube's script.
    async fn task_legs(&mut self, cube: &Object) -> Result<(), TestFailure> {
        let item = self.new_script(cube, "Compile").await?;
        let asset = self.listed_asset(cube, item).await?;
        let mut held = Held {
            item,
            asset,
            made: 1,
        };

        if self.backend == Backend::Luau {
            for leg in LUAU {
                let _answered = self
                    .step(cube, &mut held, leg.name, leg.source, true)
                    .await?;
                if leg.name == "luau_valid" {
                    let flags = self
                        .listed(cube, held.item)
                        .await?
                        .map(|script| script.flags);
                    let language = flags.map_or_else(
                        || "no item".to_owned(),
                        |flags| match ScriptLanguage::from_item_flags(flags) {
                            Some(ScriptLanguage::Lsl) => "lsl".to_owned(),
                            Some(ScriptLanguage::Luau) => "luau".to_owned(),
                            Some(_) | None => format!("subtype {}", flags & 0xff),
                        },
                    );
                    let key = self.key(leg.name, "item_language");
                    self.metrics.set(&key, language);
                }
            }
            return Ok(());
        }

        // 1. The valid script, running.
        let v1 = valid_source(self.backend, "v1");
        let _answered = self.step(cube, &mut held, "v1", &v1, true).await?;
        self.listen().await?;
        self.record_heard("v1");
        self.read_source(cube, &held, "v1", &[("v1", &v1)]).await?;

        // 2. Every broken source; after the first, what became of v1.
        for (index, leg) in BROKEN.iter().enumerate() {
            let _answered = self
                .step(cube, &mut held, leg.name, leg.source, true)
                .await?;
            if index == 0 {
                self.listen().await?;
                self.record_heard(&format!("{}_after", leg.name));
                let running = self.running(cube, held.item).await?;
                let key = self.key(leg.name, "after_running");
                self.metrics.set(&key, running);
                self.ping(&format!("{}_after", leg.name)).await?;
                self.read_source(
                    cube,
                    &held,
                    leg.name,
                    &[("v1", &v1), (leg.name, leg.source)],
                )
                .await?;
            }
        }
        self.ping("broken_after").await?;

        // 3. The functions no served document lists.
        for leg in UNSERVED {
            let _answered = self
                .step(cube, &mut held, leg.name, leg.source, true)
                .await?;
        }

        // 4. Sizes.
        for (name, size) in SIZES {
            let source = sized_source(*size);
            let _answered = self.step(cube, &mut held, name, &source, true).await?;
        }
        let large = large_code_source();
        let _answered = self
            .step(cube, &mut held, "large_code", &large, true)
            .await?;
        self.listen().await?;
        self.record_heard("large_code");

        // 5. The running flag of a valid upload.
        let v2 = valid_source(self.backend, "v2");
        let _answered = self.step(cube, &mut held, "v2_stopped", &v2, false).await?;
        self.read_source(cube, &held, "v2_stopped", &[("v2", &v2)])
            .await?;
        self.listen().await?;
        self.record_heard("v2_stopped");
        let running = self.running(cube, held.item).await?;
        let key = self.key("v2_stopped", "after_running");
        self.metrics.set(&key, running);
        let v3 = valid_source(self.backend, "v3");
        let _answered = self.step(cube, &mut held, "v3_running", &v3, true).await?;
        self.listen().await?;
        self.record_heard("v3_running");
        let running = self.running(cube, held.item).await?;
        let key = self.key("v3_running", "after_running");
        self.metrics.set(&key, running);

        // 6. The same valid source again, over the running script.
        let _answered = self.step(cube, &mut held, "v3_again", &v3, true).await?;
        self.listen().await?;
        self.record_heard("v3_again");
        Ok(())
    }

    /// One task leg: uploads `source` to the cube's script and records the
    /// answer and what the item is listed with afterwards. A fresh script is
    /// made for it first when the grid left the previous upload unanswered,
    /// and under XEngine for every leg but `v1` and the two over the running
    /// script it leaves (`syntax`, `v3_again`): XEngine answers no upload
    /// to a script that holds a running instance (it stores and restarts it
    /// all the same), and every unanswered upload keeps one of OpenSim's HTTP
    /// handlers until later uploads are refused outright.
    async fn step(
        &mut self,
        cube: &Object,
        held: &mut Held,
        leg: &str,
        source: &str,
        running: bool,
    ) -> Result<Answered, TestFailure> {
        let reuse = matches!(leg, "v1" | "syntax" | "v3_again");
        let stalled = self.stalled.take().is_some();
        let fresh = if stalled || (self.backend.fresh_each() && !reuse) {
            held.made = held.made.saturating_add(1);
            held.item = self
                .new_script(cube, &format!("Compile {}", held.made))
                .await?;
            held.asset = self.listed_asset(cube, held.item).await?;
            true
        } else {
            false
        };
        let key = self.key(leg, "fresh_item");
        self.metrics.set(&key, fresh);
        let answered = self.upload(cube, held.item, leg, source, running).await?;
        held.asset = self
            .record_asset(cube, held.item, leg, &answered, held.asset)
            .await?;
        Ok(answered)
    }

    /// The legs in the agent's inventory: a script item made in the root,
    /// a broken and a valid upload over `UpdateScriptAgent`, then removed.
    async fn agent_legs(&mut self, root: InventoryFolderKey) -> Result<(), TestFailure> {
        if self.backend == Backend::Luau {
            return Ok(());
        }
        self.stalled = None;
        let name = format!("sl-conformance script-compile {}", self.backend.key());
        let wanted = name.clone();
        self.session
            .send(Command::CreateInventoryItem(NewInventoryItem {
                folder_id: root,
                transaction_id: Uuid::nil(),
                next_owner_mask: 0x0008_e000,
                asset_type: AssetType::ScriptText,
                inv_type: InventoryType::Script,
                wearable_type: WearableType::Shape,
                name,
                description: String::new(),
            }))
            .await?;
        let (item, created_asset) = self
            .session
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::InventoryItemCreated { item, .. } if item.name == wanted => {
                    Some((item.item_id, item.asset_id))
                }
                _ => None,
            })
            .await?;
        let mut asset = created_asset;
        let outcome = async {
            for (leg, source) in [
                (
                    "agent_syntax",
                    BROKEN.first().map_or("", |broken| broken.source),
                ),
                (
                    "agent_valid",
                    "default\n{\n    state_entry()\n    {\n    }\n}\n",
                ),
            ] {
                let prefixed = format!("{}{source}", self.backend.prefix(self.grid));
                let answered = self
                    .answer(
                        leg,
                        ScriptUploadLocation::AgentInventory { item_id: item },
                        prefixed,
                    )
                    .await?;
                let listed = self.agent_asset(root, item).await?;
                let state = asset_state(listed, asset);
                let key = self.key(leg, "item_asset");
                self.metrics.set(&key, state);
                let key = self.key(leg, "new_asset");
                self.metrics
                    .set(&key, named_state(answered.new_asset, item.uuid(), listed));
                if let Some(listed) = listed {
                    asset = listed;
                }
            }
            Ok::<(), TestFailure>(())
        }
        .await;
        self.session
            .send(Command::RemoveInventoryItems(vec![item]))
            .await?;
        outcome
    }

    /// Uploads `source` (after the engine line) to the cube's script and
    /// records the answer under `leg`.
    async fn upload(
        &mut self,
        cube: &Object,
        item: InventoryKey,
        leg: &str,
        source: &str,
        running: bool,
    ) -> Result<Answered, TestFailure> {
        let prefixed = format!("{}{source}", self.backend.prefix(self.grid));
        self.answer(
            leg,
            ScriptUploadLocation::TaskInventory {
                task_id: cube.full_id,
                item_id: item,
                running,
                experience: None,
            },
            prefixed,
        )
        .await
    }

    /// Sends one upload and records its answer under `leg`: `compiled`, the
    /// errors, the new asset and the running echo — or the transport's
    /// refusal.
    async fn answer(
        &mut self,
        leg: &str,
        location: ScriptUploadLocation,
        source: String,
    ) -> Result<Answered, TestFailure> {
        let started = Instant::now();
        self.session
            .send(Command::UploadScript {
                location,
                target: self.backend.target(),
                source: source.into_bytes(),
            })
            .await?;
        let heard = &mut self.heard;
        let outcome = self
            .session
            .wait_for(UPLOAD_TIMEOUT, |event| match event {
                Event::ScriptUploaded {
                    compiled,
                    errors,
                    new_asset,
                    running,
                    ..
                } => Some(Ok(Answered {
                    compiled: *compiled,
                    errors: errors.clone(),
                    new_asset: *new_asset,
                    running: *running,
                })),
                Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                other => {
                    keep_heard(heard, other);
                    None
                }
            })
            .await;
        let answered = match outcome {
            Ok(Ok(answered)) => answered,
            Ok(Err(reason)) => {
                tracing::warn!(
                    backend = self.backend.key(),
                    leg,
                    reason,
                    seconds = started.elapsed().as_secs_f64(),
                    "script upload refused"
                );
                let key = self.key(leg, "upload");
                self.metrics
                    .set(&key, format!("refused: {}", without_ids(&reason)));
                return Ok(Answered::default());
            }
            Err(TestFailure::Timeout(_)) => {
                tracing::warn!(
                    backend = self.backend.key(),
                    leg,
                    "script upload not answered"
                );
                let key = self.key(leg, "upload");
                self.metrics.set(&key, "no answer");
                self.stalled = Some(leg.to_owned());
                return Ok(Answered::default());
            }
            Err(other) => return Err(other),
        };
        tracing::info!(
            backend = self.backend.key(),
            leg,
            compiled = answered.compiled,
            errors = answered.errors.len(),
            seconds = started.elapsed().as_secs_f64(),
            "script upload answered"
        );
        let key = self.key(leg, "upload");
        self.metrics.set(&key, "answered");
        let key = secs_metric(&self.key(leg, "answer"));
        self.metrics
            .set_timing(&key, started.elapsed().as_secs_f64());
        let key = self.key(leg, "compiled");
        self.metrics.set(&key, answered.compiled);
        let key = self.key(leg, "errors_count");
        self.metrics.set(&key, answered.errors.len().to_string());
        if let Some(first) = answered.errors.first() {
            let at = match (first.line, first.column) {
                (Some(line), Some(column)) => format!("line {line} column {column}"),
                (Some(line), None) => format!("line {line}"),
                (None, _) => "nowhere".to_owned(),
            };
            let key = self.key(leg, "error_0_at");
            self.metrics.set(&key, at);
        }
        for (index, error) in answered.errors.iter().take(ERRORS_KEPT).enumerate() {
            let key = self.key(leg, &format!("error_{index}"));
            self.metrics.set(&key, without_uuids(&error.raw));
        }
        let key = self.key(leg, "running_echo");
        self.metrics.set(
            &key,
            answered
                .running
                .map_or_else(|| "none".to_owned(), |running| running.to_string()),
        );
        Ok(answered)
    }

    /// Records which asset the cube's script is listed with after `leg`'s
    /// upload, and what the answer named, and returns the asset.
    async fn record_asset(
        &mut self,
        cube: &Object,
        item: InventoryKey,
        leg: &str,
        answered: &Answered,
        before: Option<Uuid>,
    ) -> Result<Option<Uuid>, TestFailure> {
        let listed = self.listed(cube, item).await?;
        let asset = listed
            .as_ref()
            .and_then(|script| script.asset_id)
            .map(|asset| asset.uuid());
        let state = match listed {
            None => "not listed",
            Some(_) => asset_state(asset, before.unwrap_or_default()),
        };
        let key = self.key(leg, "item_asset");
        self.metrics.set(&key, state);
        let key = self.key(leg, "new_asset");
        self.metrics
            .set(&key, named_state(answered.new_asset, item.uuid(), asset));
        Ok(asset.or(before))
    }

    /// Reads the cube's script's source back over the UDP transfer (under the
    /// listed asset id, nil where the grid hides it) and records under `leg`
    /// which of `uploaded` (the sources as written, before the engine line)
    /// it is.
    async fn read_source(
        &mut self,
        cube: &Object,
        held: &Held,
        leg: &str,
        uploaded: &[(&str, &str)],
    ) -> Result<(), TestFailure> {
        let item = held.item;
        self.session
            .send(Command::FetchTaskItemAsset {
                task: cube.full_id,
                item_id: item,
                asset_id: AssetKey::from(held.asset.unwrap_or_default()),
                asset_type: AssetType::ScriptText,
            })
            .await?;
        let heard = &mut self.heard;
        let read = self
            .session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::TaskItemAssetReceived {
                    item: got, data, ..
                } if *got == item => Some(Ok(data.clone())),
                Event::TransferFailed {
                    item: Some(got),
                    status,
                    ..
                } if *got == item => Some(Err(format!("{status:?}"))),
                other => {
                    keep_heard(heard, other);
                    None
                }
            })
            .await;
        let prefix = self.backend.prefix(self.grid);
        let state = match read {
            Ok(Ok(data)) => {
                let text = String::from_utf8_lossy(&data);
                let text = text.trim_end_matches('\0');
                uploaded
                    .iter()
                    .find(|(_name, source)| text == format!("{prefix}{source}") || text == *source)
                    .map_or_else(
                        || format!("other ({} bytes)", data.len()),
                        |(name, _source)| format!("{name}'s source"),
                    )
            }
            Ok(Err(status)) => format!("refused: {status}"),
            Err(TestFailure::Timeout(_)) => "no answer".to_owned(),
            Err(other) => return Err(other),
        };
        let key = self.key(leg, "source");
        self.metrics.set(&key, state);
        Ok(())
    }

    /// The asset the cube's script names in a fresh listing.
    async fn listed_asset(
        &mut self,
        cube: &Object,
        item: InventoryKey,
    ) -> Result<Option<Uuid>, TestFailure> {
        Ok(self
            .listed(cube, item)
            .await?
            .and_then(|script| script.asset_id)
            .map(|asset| asset.uuid()))
    }

    /// The cube's script `item` in a fresh listing.
    async fn listed(
        &mut self,
        cube: &Object,
        item: InventoryKey,
    ) -> Result<Option<TaskInventoryItem>, TestFailure> {
        self.session
            .send(Command::FetchTaskInventory {
                target: cube.scoped_id(),
                held_serial: None,
            })
            .await?;
        let items = self.listing(cube).await?;
        Ok(items.into_iter().find(|listed| listed.item_id == item))
    }

    /// Waits for the cube's listing, keeping what the script says meanwhile.
    async fn listing(&mut self, cube: &Object) -> Result<Vec<TaskInventoryItem>, TestFailure> {
        let task = cube.full_id;
        let heard = &mut self.heard;
        self.session
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
                    keep_heard(heard, other);
                    None
                }
            })
            .await?
            .map_err(|error| {
                TestFailure::Assertion(format!("the cube's listing failed to parse: {error}"))
            })
    }

    /// The asset the agent's script item names in a fresh fetch of `root`.
    async fn agent_asset(
        &mut self,
        root: InventoryFolderKey,
        item: InventoryKey,
    ) -> Result<Option<Uuid>, TestFailure> {
        self.session
            .send(Command::FetchInventoryFolders(vec![root]))
            .await?;
        self.session
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::InventoryDescendents {
                    folder_id, items, ..
                } if *folder_id == root => Some(
                    items
                        .iter()
                        .find(|listed| listed.item_id == item)
                        .map(|listed| listed.asset_id),
                ),
                _ => None,
            })
            .await
    }

    /// Makes a script named `name` in the cube and returns its item id.
    async fn new_script(&mut self, cube: &Object, name: &str) -> Result<InventoryKey, TestFailure> {
        let agent = self
            .session
            .agent_id()
            .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
        self.session
            .send(Command::RezScript {
                target: cube.scoped_id(),
                params: Box::new(RezScriptParams {
                    group_id: None,
                    enabled: false,
                    item: RestoreItem::new_script(agent, cube.full_id, name, Uuid::new_v4()),
                }),
            })
            .await?;
        let started = Instant::now();
        loop {
            self.session
                .send(Command::FetchTaskInventory {
                    target: cube.scoped_id(),
                    held_serial: None,
                })
                .await?;
            let items = self.listing(cube).await?;
            if let Some(found) = items
                .iter()
                .find(|listed| listed.inv_type == InventoryType::Script && listed.name == name)
            {
                return Ok(found.item_id);
            }
            if started.elapsed() >= STEP_TIMEOUT {
                return Err(TestFailure::Assertion(format!(
                    "the new script {name:?} did not appear in the cube"
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Whether the cube's script runs, as `GetScriptRunning` answers.
    async fn running(&mut self, cube: &Object, item: InventoryKey) -> Result<String, TestFailure> {
        let object_id = cube.full_id;
        self.session
            .send(Command::RequestScriptRunning {
                object_id,
                item_id: item,
            })
            .await?;
        let heard = &mut self.heard;
        let answer = self
            .session
            .wait_for(REPLY_TIMEOUT, |event| match event {
                Event::ScriptRunning {
                    object_id: got,
                    item_id: got_item,
                    running,
                } if *got == object_id && *got_item == item => Some(*running),
                other => {
                    keep_heard(heard, other);
                    None
                }
            })
            .await;
        match answer {
            Ok(running) => Ok(running.to_string()),
            Err(TestFailure::Timeout(_)) => Ok("no answer".to_owned()),
            Err(other) => Err(other),
        }
    }

    /// Says a line to the script on [`PING_CHANNEL`] and records what it
    /// answered under `leg`.
    async fn ping(&mut self, leg: &str) -> Result<(), TestFailure> {
        self.session
            .send(Command::Chat {
                message: format!("ping {leg}"),
                chat_type: ChatType::Normal,
                channel: ChatChannel(PING_CHANNEL),
            })
            .await?;
        self.listen().await?;
        self.record_heard(&format!("{leg}_ping"));
        Ok(())
    }

    /// Keeps what the script says for [`LISTEN`].
    async fn listen(&mut self) -> Result<(), TestFailure> {
        let heard = &mut self.heard;
        let outcome = self
            .session
            .wait_for(LISTEN, |event| {
                keep_heard(heard, event);
                None::<()>
            })
            .await;
        match outcome {
            Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
            Err(other) => Err(other),
        }
    }

    /// Records the lines heard since the last record under `leg`, and
    /// forgets them.
    fn record_heard(&mut self, leg: &str) {
        let prefix = format!("CMP {} ", self.backend.key());
        let lines: Vec<String> = self
            .heard
            .drain(..)
            .map(|line| {
                line.strip_prefix(&prefix)
                    .map_or_else(|| line.clone(), str::to_owned)
            })
            .collect();
        let key = self.key(leg, "heard");
        self.metrics.set(
            &key,
            if lines.is_empty() {
                "nothing".to_owned()
            } else {
                lines.join(" | ")
            },
        );
    }
}

/// Keeps a `CMP` owner-say line, or a debug-channel line, of an object's.
fn keep_heard(heard: &mut Vec<String>, event: &Event) {
    let Event::ChatReceived(chat) = event else {
        return;
    };
    if !matches!(chat.source, ChatSource::Object(_)) {
        return;
    }
    let line = chat.message.trim_end();
    if chat.chat_type == ChatType::Owner && line.starts_with("CMP ") {
        heard.push(line.to_owned());
    } else if chat.chat_type == ChatType::DebugChannel {
        heard.push(format!("debug: {}", without_uuids(line)));
    }
}

/// `text` with every UUID replaced by `<id>` (an engine names the asset it
/// compiled) and nothing else: an error's own numbers, such as a C# error
/// code, are the grid's answer.
fn without_uuids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        if let Some(candidate) = rest.get(..36)
            && candidate.contains('-')
            && Uuid::parse_str(candidate).is_ok()
        {
            out.push_str("<id>");
            rest = rest.get(36..).unwrap_or_default();
        } else {
            out.push(first);
            rest = rest.get(first.len_utf8()..).unwrap_or_default();
        }
    }
    out
}

/// What the item points at after an upload: the asset it had, or a new one.
fn asset_state(listed: Option<Uuid>, before: Uuid) -> &'static str {
    match listed {
        None => "hidden",
        Some(asset) if asset.is_nil() => "hidden",
        Some(asset) if asset == before => "kept",
        Some(_) => "replaced",
    }
}

/// What the id an upload's answer names as `new_asset` is: nothing, the
/// item's own id, the asset the item is listed with afterwards, or another.
fn named_state(named: Option<Uuid>, item: Uuid, listed: Option<Uuid>) -> &'static str {
    match named {
        None => "none",
        Some(named) if named == item => "the item's id",
        Some(named) if Some(named) == listed => "the listed asset",
        Some(_) => "another",
    }
}

/// The id of the first folder of `folder_type` in a skeleton.
fn folder_of_type(
    folders: &[InventoryFolder],
    folder_type: FolderType,
) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == folder_type.to_code())
        .map(|folder| folder.folder_id)
}

// The two tables are generated from the last two runs on aditi and the last
// run on the local OpenSim (2026-10-25; the one before it agreed but for the
// XEngine error codes an older scrub replaced): every key but the timings and
// `mono_large_code_heard` — the two `DEBUG_CHANNEL` lines of the Stack-Heap
// Collision arrived once with their text and once empty.
/// What Second Life (aditi) was measured to answer, by metric.
const SECOND_LIFE: &[(&str, Answer)] = &[
    ("lsl2_agent_syntax_compiled", Answer::Flag(false)),
    (
        "lsl2_agent_syntax_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    (
        "lsl2_agent_syntax_error_0_at",
        Answer::Text("line 5 column 21"),
    ),
    ("lsl2_agent_syntax_errors_count", Answer::Text("1")),
    ("lsl2_agent_syntax_item_asset", Answer::Text("hidden")),
    ("lsl2_agent_syntax_new_asset", Answer::Text("another")),
    ("lsl2_agent_syntax_running_echo", Answer::Text("none")),
    ("lsl2_agent_syntax_upload", Answer::Text("answered")),
    ("lsl2_agent_valid_compiled", Answer::Flag(true)),
    ("lsl2_agent_valid_errors_count", Answer::Text("0")),
    ("lsl2_agent_valid_item_asset", Answer::Text("hidden")),
    ("lsl2_agent_valid_new_asset", Answer::Text("another")),
    ("lsl2_agent_valid_running_echo", Answer::Text("none")),
    ("lsl2_agent_valid_upload", Answer::Text("answered")),
    ("lsl2_broken_after_ping_heard", Answer::Text("nothing")),
    ("lsl2_crlf_compiled", Answer::Flag(false)),
    (
        "lsl2_crlf_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    ("lsl2_crlf_error_0_at", Answer::Text("line 5 column 21")),
    ("lsl2_crlf_errors_count", Answer::Text("1")),
    ("lsl2_crlf_fresh_item", Answer::Flag(false)),
    ("lsl2_crlf_item_asset", Answer::Text("hidden")),
    ("lsl2_crlf_new_asset", Answer::Text("another")),
    ("lsl2_crlf_running_echo", Answer::Text("true")),
    ("lsl2_crlf_upload", Answer::Text("answered")),
    ("lsl2_duplicate_global_compiled", Answer::Flag(false)),
    (
        "lsl2_duplicate_global_error_0",
        Answer::Text("(1, 9) : ERROR : Name previously declared within scope\n"),
    ),
    (
        "lsl2_duplicate_global_error_0_at",
        Answer::Text("line 2 column 10"),
    ),
    ("lsl2_duplicate_global_errors_count", Answer::Text("1")),
    ("lsl2_duplicate_global_fresh_item", Answer::Flag(false)),
    ("lsl2_duplicate_global_item_asset", Answer::Text("hidden")),
    ("lsl2_duplicate_global_new_asset", Answer::Text("another")),
    ("lsl2_duplicate_global_running_echo", Answer::Text("true")),
    ("lsl2_duplicate_global_upload", Answer::Text("answered")),
    ("lsl2_empty_compiled", Answer::Flag(false)),
    (
        "lsl2_empty_error_0",
        Answer::Text("(0, 0) : ERROR : Syntax error\n"),
    ),
    ("lsl2_empty_error_0_at", Answer::Text("line 1 column 1")),
    ("lsl2_empty_errors_count", Answer::Text("1")),
    ("lsl2_empty_fresh_item", Answer::Flag(false)),
    ("lsl2_empty_item_asset", Answer::Text("hidden")),
    ("lsl2_empty_new_asset", Answer::Text("another")),
    ("lsl2_empty_running_echo", Answer::Text("true")),
    ("lsl2_empty_upload", Answer::Text("answered")),
    ("lsl2_large_code_compiled", Answer::Flag(false)),
    (
        "lsl2_large_code_error_0",
        Answer::Text("Function Args: (null)\n"),
    ),
    ("lsl2_large_code_error_0_at", Answer::Text("nowhere")),
    (
        "lsl2_large_code_error_1",
        Answer::Text("Local List: (null)\n"),
    ),
    (
        "lsl2_large_code_error_2",
        Answer::Text("Function Args: (null)\n"),
    ),
    (
        "lsl2_large_code_error_3",
        Answer::Text("Local List: (null)\n"),
    ),
    (
        "lsl2_large_code_error_4",
        Answer::Text("Function Args: (null)\n"),
    ),
    ("lsl2_large_code_errors_count", Answer::Text("1203")),
    ("lsl2_large_code_fresh_item", Answer::Flag(false)),
    ("lsl2_large_code_heard", Answer::Text("nothing")),
    ("lsl2_large_code_item_asset", Answer::Text("hidden")),
    ("lsl2_large_code_new_asset", Answer::Text("another")),
    ("lsl2_large_code_running_echo", Answer::Text("true")),
    ("lsl2_large_code_upload", Answer::Text("answered")),
    ("lsl2_missing_return_compiled", Answer::Flag(false)),
    (
        "lsl2_missing_return_error_0",
        Answer::Text("(3, 0) : ERROR : Not all code paths return a value\n"),
    ),
    (
        "lsl2_missing_return_error_0_at",
        Answer::Text("line 4 column 1"),
    ),
    ("lsl2_missing_return_errors_count", Answer::Text("1")),
    ("lsl2_missing_return_fresh_item", Answer::Flag(false)),
    ("lsl2_missing_return_item_asset", Answer::Text("hidden")),
    ("lsl2_missing_return_new_asset", Answer::Text("another")),
    ("lsl2_missing_return_running_echo", Answer::Text("true")),
    ("lsl2_missing_return_upload", Answer::Text("answered")),
    ("lsl2_multibyte_column_compiled", Answer::Flag(false)),
    (
        "lsl2_multibyte_column_error_0",
        Answer::Text("(4, 41) : ERROR : Syntax error\n"),
    ),
    (
        "lsl2_multibyte_column_error_0_at",
        Answer::Text("line 5 column 42"),
    ),
    ("lsl2_multibyte_column_errors_count", Answer::Text("1")),
    ("lsl2_multibyte_column_fresh_item", Answer::Flag(false)),
    ("lsl2_multibyte_column_item_asset", Answer::Text("hidden")),
    ("lsl2_multibyte_column_new_asset", Answer::Text("another")),
    ("lsl2_multibyte_column_running_echo", Answer::Text("true")),
    ("lsl2_multibyte_column_upload", Answer::Text("answered")),
    ("lsl2_no_default_compiled", Answer::Flag(false)),
    (
        "lsl2_no_default_error_0",
        Answer::Text("(0, 0) : ERROR : Syntax error\n"),
    ),
    (
        "lsl2_no_default_error_0_at",
        Answer::Text("line 1 column 1"),
    ),
    ("lsl2_no_default_errors_count", Answer::Text("1")),
    ("lsl2_no_default_fresh_item", Answer::Flag(false)),
    ("lsl2_no_default_item_asset", Answer::Text("hidden")),
    ("lsl2_no_default_new_asset", Answer::Text("another")),
    ("lsl2_no_default_running_echo", Answer::Text("true")),
    ("lsl2_no_default_upload", Answer::Text("answered")),
    ("lsl2_size_1m_compiled", Answer::Flag(true)),
    ("lsl2_size_1m_errors_count", Answer::Text("0")),
    ("lsl2_size_1m_fresh_item", Answer::Flag(false)),
    ("lsl2_size_1m_item_asset", Answer::Text("hidden")),
    ("lsl2_size_1m_new_asset", Answer::Text("another")),
    ("lsl2_size_1m_running_echo", Answer::Text("true")),
    ("lsl2_size_1m_upload", Answer::Text("answered")),
    ("lsl2_size_256k_over_compiled", Answer::Flag(true)),
    ("lsl2_size_256k_over_errors_count", Answer::Text("0")),
    ("lsl2_size_256k_over_fresh_item", Answer::Flag(false)),
    ("lsl2_size_256k_over_item_asset", Answer::Text("hidden")),
    ("lsl2_size_256k_over_new_asset", Answer::Text("another")),
    ("lsl2_size_256k_over_running_echo", Answer::Text("true")),
    ("lsl2_size_256k_over_upload", Answer::Text("answered")),
    ("lsl2_size_64k_over_compiled", Answer::Flag(true)),
    ("lsl2_size_64k_over_errors_count", Answer::Text("0")),
    ("lsl2_size_64k_over_fresh_item", Answer::Flag(false)),
    ("lsl2_size_64k_over_item_asset", Answer::Text("hidden")),
    ("lsl2_size_64k_over_new_asset", Answer::Text("another")),
    ("lsl2_size_64k_over_running_echo", Answer::Text("true")),
    ("lsl2_size_64k_over_upload", Answer::Text("answered")),
    ("lsl2_size_64k_under_compiled", Answer::Flag(true)),
    ("lsl2_size_64k_under_errors_count", Answer::Text("0")),
    ("lsl2_size_64k_under_fresh_item", Answer::Flag(false)),
    ("lsl2_size_64k_under_item_asset", Answer::Text("hidden")),
    ("lsl2_size_64k_under_new_asset", Answer::Text("another")),
    ("lsl2_size_64k_under_running_echo", Answer::Text("true")),
    ("lsl2_size_64k_under_upload", Answer::Text("answered")),
    ("lsl2_syntax_after_heard", Answer::Text("nothing")),
    ("lsl2_syntax_after_ping_heard", Answer::Text("nothing")),
    ("lsl2_syntax_after_running", Answer::Text("false")),
    ("lsl2_syntax_compiled", Answer::Flag(false)),
    (
        "lsl2_syntax_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    ("lsl2_syntax_error_0_at", Answer::Text("line 5 column 21")),
    ("lsl2_syntax_errors_count", Answer::Text("1")),
    ("lsl2_syntax_fresh_item", Answer::Flag(false)),
    ("lsl2_syntax_item_asset", Answer::Text("hidden")),
    ("lsl2_syntax_new_asset", Answer::Text("another")),
    ("lsl2_syntax_running_echo", Answer::Text("true")),
    ("lsl2_syntax_source", Answer::Text("syntax's source")),
    ("lsl2_syntax_upload", Answer::Text("answered")),
    ("lsl2_tab_column_compiled", Answer::Flag(false)),
    (
        "lsl2_tab_column_error_0",
        Answer::Text("(4, 16) : ERROR : Syntax error\n"),
    ),
    (
        "lsl2_tab_column_error_0_at",
        Answer::Text("line 5 column 17"),
    ),
    ("lsl2_tab_column_errors_count", Answer::Text("1")),
    ("lsl2_tab_column_fresh_item", Answer::Flag(false)),
    ("lsl2_tab_column_item_asset", Answer::Text("hidden")),
    ("lsl2_tab_column_new_asset", Answer::Text("another")),
    ("lsl2_tab_column_running_echo", Answer::Text("true")),
    ("lsl2_tab_column_upload", Answer::Text("answered")),
    ("lsl2_three_errors_compiled", Answer::Flag(false)),
    (
        "lsl2_three_errors_error_0",
        Answer::Text("(5, 10) : ERROR : Name not defined within scope\n"),
    ),
    (
        "lsl2_three_errors_error_0_at",
        Answer::Text("line 6 column 11"),
    ),
    ("lsl2_three_errors_errors_count", Answer::Text("1")),
    ("lsl2_three_errors_fresh_item", Answer::Flag(false)),
    ("lsl2_three_errors_item_asset", Answer::Text("hidden")),
    ("lsl2_three_errors_new_asset", Answer::Text("another")),
    ("lsl2_three_errors_running_echo", Answer::Text("true")),
    ("lsl2_three_errors_upload", Answer::Text("answered")),
    ("lsl2_two_syntax_errors_compiled", Answer::Flag(false)),
    (
        "lsl2_two_syntax_errors_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    (
        "lsl2_two_syntax_errors_error_0_at",
        Answer::Text("line 5 column 21"),
    ),
    ("lsl2_two_syntax_errors_errors_count", Answer::Text("1")),
    ("lsl2_two_syntax_errors_fresh_item", Answer::Flag(false)),
    ("lsl2_two_syntax_errors_item_asset", Answer::Text("hidden")),
    ("lsl2_two_syntax_errors_new_asset", Answer::Text("another")),
    ("lsl2_two_syntax_errors_running_echo", Answer::Text("true")),
    ("lsl2_two_syntax_errors_upload", Answer::Text("answered")),
    ("lsl2_type_mismatch_compiled", Answer::Flag(false)),
    (
        "lsl2_type_mismatch_error_0",
        Answer::Text("(4, 23) : ERROR : Type mismatch\n"),
    ),
    (
        "lsl2_type_mismatch_error_0_at",
        Answer::Text("line 5 column 24"),
    ),
    ("lsl2_type_mismatch_errors_count", Answer::Text("1")),
    ("lsl2_type_mismatch_fresh_item", Answer::Flag(false)),
    ("lsl2_type_mismatch_item_asset", Answer::Text("hidden")),
    ("lsl2_type_mismatch_new_asset", Answer::Text("another")),
    ("lsl2_type_mismatch_running_echo", Answer::Text("true")),
    ("lsl2_type_mismatch_upload", Answer::Text("answered")),
    ("lsl2_undeclared_compiled", Answer::Flag(false)),
    (
        "lsl2_undeclared_error_0",
        Answer::Text("(4, 10) : ERROR : Name not defined within scope\n"),
    ),
    (
        "lsl2_undeclared_error_0_at",
        Answer::Text("line 5 column 11"),
    ),
    ("lsl2_undeclared_errors_count", Answer::Text("1")),
    ("lsl2_undeclared_fresh_item", Answer::Flag(false)),
    ("lsl2_undeclared_item_asset", Answer::Text("hidden")),
    ("lsl2_undeclared_new_asset", Answer::Text("another")),
    ("lsl2_undeclared_running_echo", Answer::Text("true")),
    ("lsl2_undeclared_upload", Answer::Text("answered")),
    ("lsl2_unknown_function_compiled", Answer::Flag(false)),
    (
        "lsl2_unknown_function_error_0",
        Answer::Text("(4, 15) : ERROR : Name not defined within scope\n"),
    ),
    (
        "lsl2_unknown_function_error_0_at",
        Answer::Text("line 5 column 16"),
    ),
    ("lsl2_unknown_function_errors_count", Answer::Text("1")),
    ("lsl2_unknown_function_fresh_item", Answer::Flag(false)),
    ("lsl2_unknown_function_item_asset", Answer::Text("hidden")),
    ("lsl2_unknown_function_new_asset", Answer::Text("another")),
    ("lsl2_unknown_function_running_echo", Answer::Text("true")),
    ("lsl2_unknown_function_upload", Answer::Text("answered")),
    ("lsl2_unknown_label_compiled", Answer::Flag(false)),
    (
        "lsl2_unknown_label_error_0",
        Answer::Text("(4, 20) : ERROR : Name not defined within scope\n"),
    ),
    (
        "lsl2_unknown_label_error_0_at",
        Answer::Text("line 5 column 21"),
    ),
    ("lsl2_unknown_label_errors_count", Answer::Text("1")),
    ("lsl2_unknown_label_fresh_item", Answer::Flag(false)),
    ("lsl2_unknown_label_item_asset", Answer::Text("hidden")),
    ("lsl2_unknown_label_new_asset", Answer::Text("another")),
    ("lsl2_unknown_label_running_echo", Answer::Text("true")),
    ("lsl2_unknown_label_upload", Answer::Text("answered")),
    ("lsl2_unknown_state_compiled", Answer::Flag(false)),
    (
        "lsl2_unknown_state_error_0",
        Answer::Text("(4, 19) : ERROR : Name not defined within scope\n"),
    ),
    (
        "lsl2_unknown_state_error_0_at",
        Answer::Text("line 5 column 20"),
    ),
    ("lsl2_unknown_state_errors_count", Answer::Text("1")),
    ("lsl2_unknown_state_fresh_item", Answer::Flag(false)),
    ("lsl2_unknown_state_item_asset", Answer::Text("hidden")),
    ("lsl2_unknown_state_new_asset", Answer::Text("another")),
    ("lsl2_unknown_state_running_echo", Answer::Text("true")),
    ("lsl2_unknown_state_upload", Answer::Text("answered")),
    (
        "lsl2_unserved_clear_experience_permissions_compiled",
        Answer::Flag(true),
    ),
    (
        "lsl2_unserved_clear_experience_permissions_errors_count",
        Answer::Text("0"),
    ),
    (
        "lsl2_unserved_get_experience_list_compiled",
        Answer::Flag(true),
    ),
    (
        "lsl2_unserved_get_experience_list_errors_count",
        Answer::Text("0"),
    ),
    ("lsl2_unserved_point_at_compiled", Answer::Flag(true)),
    ("lsl2_unserved_point_at_errors_count", Answer::Text("0")),
    (
        "lsl2_unserved_remote_load_script_compiled",
        Answer::Flag(true),
    ),
    (
        "lsl2_unserved_remote_load_script_errors_count",
        Answer::Text("0"),
    ),
    ("lsl2_unserved_stop_point_at_compiled", Answer::Flag(true)),
    (
        "lsl2_unserved_stop_point_at_errors_count",
        Answer::Text("0"),
    ),
    ("lsl2_unterminated_string_compiled", Answer::Flag(false)),
    (
        "lsl2_unterminated_string_error_0",
        Answer::Text("(4, 22) : ERROR : Name not defined within scope\n"),
    ),
    (
        "lsl2_unterminated_string_error_0_at",
        Answer::Text("line 5 column 23"),
    ),
    ("lsl2_unterminated_string_errors_count", Answer::Text("1")),
    ("lsl2_unterminated_string_fresh_item", Answer::Flag(false)),
    (
        "lsl2_unterminated_string_item_asset",
        Answer::Text("hidden"),
    ),
    (
        "lsl2_unterminated_string_new_asset",
        Answer::Text("another"),
    ),
    (
        "lsl2_unterminated_string_running_echo",
        Answer::Text("true"),
    ),
    ("lsl2_unterminated_string_upload", Answer::Text("answered")),
    ("lsl2_v1_compiled", Answer::Flag(true)),
    ("lsl2_v1_errors_count", Answer::Text("0")),
    ("lsl2_v1_fresh_item", Answer::Flag(false)),
    ("lsl2_v1_heard", Answer::Text("v1 start")),
    ("lsl2_v1_item_asset", Answer::Text("hidden")),
    ("lsl2_v1_new_asset", Answer::Text("another")),
    ("lsl2_v1_running_echo", Answer::Text("true")),
    ("lsl2_v1_source", Answer::Text("v1's source")),
    ("lsl2_v1_upload", Answer::Text("answered")),
    ("lsl2_v2_stopped_after_running", Answer::Text("false")),
    ("lsl2_v2_stopped_compiled", Answer::Flag(true)),
    ("lsl2_v2_stopped_errors_count", Answer::Text("0")),
    ("lsl2_v2_stopped_fresh_item", Answer::Flag(false)),
    ("lsl2_v2_stopped_heard", Answer::Text("nothing")),
    ("lsl2_v2_stopped_item_asset", Answer::Text("hidden")),
    ("lsl2_v2_stopped_new_asset", Answer::Text("another")),
    ("lsl2_v2_stopped_running_echo", Answer::Text("false")),
    ("lsl2_v2_stopped_source", Answer::Text("v2's source")),
    ("lsl2_v2_stopped_upload", Answer::Text("answered")),
    ("lsl2_v3_again_compiled", Answer::Flag(true)),
    ("lsl2_v3_again_errors_count", Answer::Text("0")),
    ("lsl2_v3_again_fresh_item", Answer::Flag(false)),
    ("lsl2_v3_again_heard", Answer::Text("v3 start")),
    ("lsl2_v3_again_item_asset", Answer::Text("hidden")),
    ("lsl2_v3_again_new_asset", Answer::Text("another")),
    ("lsl2_v3_again_running_echo", Answer::Text("true")),
    ("lsl2_v3_again_upload", Answer::Text("answered")),
    ("lsl2_v3_running_after_running", Answer::Text("true")),
    ("lsl2_v3_running_compiled", Answer::Flag(true)),
    ("lsl2_v3_running_errors_count", Answer::Text("0")),
    ("lsl2_v3_running_fresh_item", Answer::Flag(false)),
    ("lsl2_v3_running_heard", Answer::Text("v3 start")),
    ("lsl2_v3_running_item_asset", Answer::Text("hidden")),
    ("lsl2_v3_running_new_asset", Answer::Text("another")),
    ("lsl2_v3_running_running_echo", Answer::Text("true")),
    ("lsl2_v3_running_upload", Answer::Text("answered")),
    ("luau_lsl_source_compiled", Answer::Flag(true)),
    ("luau_lsl_source_errors_count", Answer::Text("0")),
    ("luau_lsl_source_fresh_item", Answer::Flag(false)),
    ("luau_lsl_source_item_asset", Answer::Text("hidden")),
    ("luau_lsl_source_new_asset", Answer::Text("another")),
    ("luau_lsl_source_running_echo", Answer::Text("true")),
    ("luau_lsl_source_upload", Answer::Text("answered")),
    ("luau_luau_syntax_compiled", Answer::Flag(false)),
    (
        "luau_luau_syntax_error_0",
        Answer::Text(":2: Expected identifier when parsing expression, got <eof>"),
    ),
    ("luau_luau_syntax_error_0_at", Answer::Text("line 2")),
    ("luau_luau_syntax_errors_count", Answer::Text("1")),
    ("luau_luau_syntax_fresh_item", Answer::Flag(false)),
    ("luau_luau_syntax_item_asset", Answer::Text("hidden")),
    ("luau_luau_syntax_mid_compiled", Answer::Flag(false)),
    (
        "luau_luau_syntax_mid_error_0",
        Answer::Text(":2: Expected identifier when parsing expression, got '='"),
    ),
    ("luau_luau_syntax_mid_error_0_at", Answer::Text("line 2")),
    ("luau_luau_syntax_mid_errors_count", Answer::Text("1")),
    ("luau_luau_syntax_mid_fresh_item", Answer::Flag(false)),
    ("luau_luau_syntax_mid_item_asset", Answer::Text("hidden")),
    ("luau_luau_syntax_mid_new_asset", Answer::Text("another")),
    ("luau_luau_syntax_mid_running_echo", Answer::Text("true")),
    ("luau_luau_syntax_mid_upload", Answer::Text("answered")),
    ("luau_luau_syntax_new_asset", Answer::Text("another")),
    ("luau_luau_syntax_running_echo", Answer::Text("true")),
    ("luau_luau_syntax_upload", Answer::Text("answered")),
    ("luau_luau_valid_compiled", Answer::Flag(true)),
    ("luau_luau_valid_errors_count", Answer::Text("0")),
    ("luau_luau_valid_fresh_item", Answer::Flag(false)),
    ("luau_luau_valid_item_asset", Answer::Text("hidden")),
    ("luau_luau_valid_item_language", Answer::Text("luau")),
    ("luau_luau_valid_new_asset", Answer::Text("another")),
    ("luau_luau_valid_running_echo", Answer::Text("true")),
    ("luau_luau_valid_upload", Answer::Text("answered")),
    ("mono_agent_syntax_compiled", Answer::Flag(false)),
    (
        "mono_agent_syntax_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    (
        "mono_agent_syntax_error_0_at",
        Answer::Text("line 5 column 21"),
    ),
    ("mono_agent_syntax_errors_count", Answer::Text("1")),
    ("mono_agent_syntax_item_asset", Answer::Text("hidden")),
    ("mono_agent_syntax_new_asset", Answer::Text("another")),
    ("mono_agent_syntax_running_echo", Answer::Text("none")),
    ("mono_agent_syntax_upload", Answer::Text("answered")),
    ("mono_agent_valid_compiled", Answer::Flag(true)),
    ("mono_agent_valid_errors_count", Answer::Text("0")),
    ("mono_agent_valid_item_asset", Answer::Text("hidden")),
    ("mono_agent_valid_new_asset", Answer::Text("another")),
    ("mono_agent_valid_running_echo", Answer::Text("none")),
    ("mono_agent_valid_upload", Answer::Text("answered")),
    ("mono_broken_after_ping_heard", Answer::Text("nothing")),
    ("mono_crlf_compiled", Answer::Flag(false)),
    (
        "mono_crlf_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    ("mono_crlf_error_0_at", Answer::Text("line 5 column 21")),
    ("mono_crlf_errors_count", Answer::Text("1")),
    ("mono_crlf_fresh_item", Answer::Flag(false)),
    ("mono_crlf_item_asset", Answer::Text("hidden")),
    ("mono_crlf_new_asset", Answer::Text("another")),
    ("mono_crlf_running_echo", Answer::Text("true")),
    ("mono_crlf_upload", Answer::Text("answered")),
    ("mono_duplicate_global_compiled", Answer::Flag(false)),
    (
        "mono_duplicate_global_error_0",
        Answer::Text("(1, 9) : ERROR : Name previously declared within scope\n"),
    ),
    (
        "mono_duplicate_global_error_0_at",
        Answer::Text("line 2 column 10"),
    ),
    ("mono_duplicate_global_errors_count", Answer::Text("1")),
    ("mono_duplicate_global_fresh_item", Answer::Flag(false)),
    ("mono_duplicate_global_item_asset", Answer::Text("hidden")),
    ("mono_duplicate_global_new_asset", Answer::Text("another")),
    ("mono_duplicate_global_running_echo", Answer::Text("true")),
    ("mono_duplicate_global_upload", Answer::Text("answered")),
    ("mono_empty_compiled", Answer::Flag(false)),
    (
        "mono_empty_error_0",
        Answer::Text("(0, 0) : ERROR : Syntax error\n"),
    ),
    ("mono_empty_error_0_at", Answer::Text("line 1 column 1")),
    ("mono_empty_errors_count", Answer::Text("1")),
    ("mono_empty_fresh_item", Answer::Flag(false)),
    ("mono_empty_item_asset", Answer::Text("hidden")),
    ("mono_empty_new_asset", Answer::Text("another")),
    ("mono_empty_running_echo", Answer::Text("true")),
    ("mono_empty_upload", Answer::Text("answered")),
    ("mono_large_code_compiled", Answer::Flag(true)),
    ("mono_large_code_errors_count", Answer::Text("0")),
    ("mono_large_code_fresh_item", Answer::Flag(false)),
    ("mono_large_code_item_asset", Answer::Text("hidden")),
    ("mono_large_code_new_asset", Answer::Text("another")),
    ("mono_large_code_running_echo", Answer::Text("true")),
    ("mono_large_code_upload", Answer::Text("answered")),
    ("mono_missing_return_compiled", Answer::Flag(false)),
    (
        "mono_missing_return_error_0",
        Answer::Text("(3, 0) : ERROR : Not all code paths return a value\n"),
    ),
    (
        "mono_missing_return_error_0_at",
        Answer::Text("line 4 column 1"),
    ),
    ("mono_missing_return_errors_count", Answer::Text("1")),
    ("mono_missing_return_fresh_item", Answer::Flag(false)),
    ("mono_missing_return_item_asset", Answer::Text("hidden")),
    ("mono_missing_return_new_asset", Answer::Text("another")),
    ("mono_missing_return_running_echo", Answer::Text("true")),
    ("mono_missing_return_upload", Answer::Text("answered")),
    ("mono_multibyte_column_compiled", Answer::Flag(false)),
    (
        "mono_multibyte_column_error_0",
        Answer::Text("(4, 41) : ERROR : Syntax error\n"),
    ),
    (
        "mono_multibyte_column_error_0_at",
        Answer::Text("line 5 column 42"),
    ),
    ("mono_multibyte_column_errors_count", Answer::Text("1")),
    ("mono_multibyte_column_fresh_item", Answer::Flag(false)),
    ("mono_multibyte_column_item_asset", Answer::Text("hidden")),
    ("mono_multibyte_column_new_asset", Answer::Text("another")),
    ("mono_multibyte_column_running_echo", Answer::Text("true")),
    ("mono_multibyte_column_upload", Answer::Text("answered")),
    ("mono_no_default_compiled", Answer::Flag(false)),
    (
        "mono_no_default_error_0",
        Answer::Text("(0, 0) : ERROR : Syntax error\n"),
    ),
    (
        "mono_no_default_error_0_at",
        Answer::Text("line 1 column 1"),
    ),
    ("mono_no_default_errors_count", Answer::Text("1")),
    ("mono_no_default_fresh_item", Answer::Flag(false)),
    ("mono_no_default_item_asset", Answer::Text("hidden")),
    ("mono_no_default_new_asset", Answer::Text("another")),
    ("mono_no_default_running_echo", Answer::Text("true")),
    ("mono_no_default_upload", Answer::Text("answered")),
    ("mono_size_1m_compiled", Answer::Flag(true)),
    ("mono_size_1m_errors_count", Answer::Text("0")),
    ("mono_size_1m_fresh_item", Answer::Flag(false)),
    ("mono_size_1m_item_asset", Answer::Text("hidden")),
    ("mono_size_1m_new_asset", Answer::Text("another")),
    ("mono_size_1m_running_echo", Answer::Text("true")),
    ("mono_size_1m_upload", Answer::Text("answered")),
    ("mono_size_256k_over_compiled", Answer::Flag(true)),
    ("mono_size_256k_over_errors_count", Answer::Text("0")),
    ("mono_size_256k_over_fresh_item", Answer::Flag(false)),
    ("mono_size_256k_over_item_asset", Answer::Text("hidden")),
    ("mono_size_256k_over_new_asset", Answer::Text("another")),
    ("mono_size_256k_over_running_echo", Answer::Text("true")),
    ("mono_size_256k_over_upload", Answer::Text("answered")),
    ("mono_size_64k_over_compiled", Answer::Flag(true)),
    ("mono_size_64k_over_errors_count", Answer::Text("0")),
    ("mono_size_64k_over_fresh_item", Answer::Flag(false)),
    ("mono_size_64k_over_item_asset", Answer::Text("hidden")),
    ("mono_size_64k_over_new_asset", Answer::Text("another")),
    ("mono_size_64k_over_running_echo", Answer::Text("true")),
    ("mono_size_64k_over_upload", Answer::Text("answered")),
    ("mono_size_64k_under_compiled", Answer::Flag(true)),
    ("mono_size_64k_under_errors_count", Answer::Text("0")),
    ("mono_size_64k_under_fresh_item", Answer::Flag(false)),
    ("mono_size_64k_under_item_asset", Answer::Text("hidden")),
    ("mono_size_64k_under_new_asset", Answer::Text("another")),
    ("mono_size_64k_under_running_echo", Answer::Text("true")),
    ("mono_size_64k_under_upload", Answer::Text("answered")),
    ("mono_syntax_after_heard", Answer::Text("nothing")),
    ("mono_syntax_after_ping_heard", Answer::Text("nothing")),
    ("mono_syntax_after_running", Answer::Text("false")),
    ("mono_syntax_compiled", Answer::Flag(false)),
    (
        "mono_syntax_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    ("mono_syntax_error_0_at", Answer::Text("line 5 column 21")),
    ("mono_syntax_errors_count", Answer::Text("1")),
    ("mono_syntax_fresh_item", Answer::Flag(false)),
    ("mono_syntax_item_asset", Answer::Text("hidden")),
    ("mono_syntax_new_asset", Answer::Text("another")),
    ("mono_syntax_running_echo", Answer::Text("true")),
    ("mono_syntax_source", Answer::Text("syntax's source")),
    ("mono_syntax_upload", Answer::Text("answered")),
    ("mono_tab_column_compiled", Answer::Flag(false)),
    (
        "mono_tab_column_error_0",
        Answer::Text("(4, 16) : ERROR : Syntax error\n"),
    ),
    (
        "mono_tab_column_error_0_at",
        Answer::Text("line 5 column 17"),
    ),
    ("mono_tab_column_errors_count", Answer::Text("1")),
    ("mono_tab_column_fresh_item", Answer::Flag(false)),
    ("mono_tab_column_item_asset", Answer::Text("hidden")),
    ("mono_tab_column_new_asset", Answer::Text("another")),
    ("mono_tab_column_running_echo", Answer::Text("true")),
    ("mono_tab_column_upload", Answer::Text("answered")),
    ("mono_three_errors_compiled", Answer::Flag(false)),
    (
        "mono_three_errors_error_0",
        Answer::Text("(5, 10) : ERROR : Name not defined within scope\n"),
    ),
    (
        "mono_three_errors_error_0_at",
        Answer::Text("line 6 column 11"),
    ),
    ("mono_three_errors_errors_count", Answer::Text("1")),
    ("mono_three_errors_fresh_item", Answer::Flag(false)),
    ("mono_three_errors_item_asset", Answer::Text("hidden")),
    ("mono_three_errors_new_asset", Answer::Text("another")),
    ("mono_three_errors_running_echo", Answer::Text("true")),
    ("mono_three_errors_upload", Answer::Text("answered")),
    ("mono_two_syntax_errors_compiled", Answer::Flag(false)),
    (
        "mono_two_syntax_errors_error_0",
        Answer::Text("(4, 20) : ERROR : Syntax error\n"),
    ),
    (
        "mono_two_syntax_errors_error_0_at",
        Answer::Text("line 5 column 21"),
    ),
    ("mono_two_syntax_errors_errors_count", Answer::Text("1")),
    ("mono_two_syntax_errors_fresh_item", Answer::Flag(false)),
    ("mono_two_syntax_errors_item_asset", Answer::Text("hidden")),
    ("mono_two_syntax_errors_new_asset", Answer::Text("another")),
    ("mono_two_syntax_errors_running_echo", Answer::Text("true")),
    ("mono_two_syntax_errors_upload", Answer::Text("answered")),
    ("mono_type_mismatch_compiled", Answer::Flag(false)),
    (
        "mono_type_mismatch_error_0",
        Answer::Text("(4, 23) : ERROR : Type mismatch\n"),
    ),
    (
        "mono_type_mismatch_error_0_at",
        Answer::Text("line 5 column 24"),
    ),
    ("mono_type_mismatch_errors_count", Answer::Text("1")),
    ("mono_type_mismatch_fresh_item", Answer::Flag(false)),
    ("mono_type_mismatch_item_asset", Answer::Text("hidden")),
    ("mono_type_mismatch_new_asset", Answer::Text("another")),
    ("mono_type_mismatch_running_echo", Answer::Text("true")),
    ("mono_type_mismatch_upload", Answer::Text("answered")),
    ("mono_undeclared_compiled", Answer::Flag(false)),
    (
        "mono_undeclared_error_0",
        Answer::Text("(4, 10) : ERROR : Name not defined within scope\n"),
    ),
    (
        "mono_undeclared_error_0_at",
        Answer::Text("line 5 column 11"),
    ),
    ("mono_undeclared_errors_count", Answer::Text("1")),
    ("mono_undeclared_fresh_item", Answer::Flag(false)),
    ("mono_undeclared_item_asset", Answer::Text("hidden")),
    ("mono_undeclared_new_asset", Answer::Text("another")),
    ("mono_undeclared_running_echo", Answer::Text("true")),
    ("mono_undeclared_upload", Answer::Text("answered")),
    ("mono_unknown_function_compiled", Answer::Flag(false)),
    (
        "mono_unknown_function_error_0",
        Answer::Text("(4, 15) : ERROR : Name not defined within scope\n"),
    ),
    (
        "mono_unknown_function_error_0_at",
        Answer::Text("line 5 column 16"),
    ),
    ("mono_unknown_function_errors_count", Answer::Text("1")),
    ("mono_unknown_function_fresh_item", Answer::Flag(false)),
    ("mono_unknown_function_item_asset", Answer::Text("hidden")),
    ("mono_unknown_function_new_asset", Answer::Text("another")),
    ("mono_unknown_function_running_echo", Answer::Text("true")),
    ("mono_unknown_function_upload", Answer::Text("answered")),
    ("mono_unknown_label_compiled", Answer::Flag(false)),
    (
        "mono_unknown_label_error_0",
        Answer::Text("(4, 20) : ERROR : Name not defined within scope\n"),
    ),
    (
        "mono_unknown_label_error_0_at",
        Answer::Text("line 5 column 21"),
    ),
    ("mono_unknown_label_errors_count", Answer::Text("1")),
    ("mono_unknown_label_fresh_item", Answer::Flag(false)),
    ("mono_unknown_label_item_asset", Answer::Text("hidden")),
    ("mono_unknown_label_new_asset", Answer::Text("another")),
    ("mono_unknown_label_running_echo", Answer::Text("true")),
    ("mono_unknown_label_upload", Answer::Text("answered")),
    ("mono_unknown_state_compiled", Answer::Flag(false)),
    (
        "mono_unknown_state_error_0",
        Answer::Text("(4, 19) : ERROR : Name not defined within scope\n"),
    ),
    (
        "mono_unknown_state_error_0_at",
        Answer::Text("line 5 column 20"),
    ),
    ("mono_unknown_state_errors_count", Answer::Text("1")),
    ("mono_unknown_state_fresh_item", Answer::Flag(false)),
    ("mono_unknown_state_item_asset", Answer::Text("hidden")),
    ("mono_unknown_state_new_asset", Answer::Text("another")),
    ("mono_unknown_state_running_echo", Answer::Text("true")),
    ("mono_unknown_state_upload", Answer::Text("answered")),
    (
        "mono_unserved_clear_experience_permissions_compiled",
        Answer::Flag(true),
    ),
    (
        "mono_unserved_clear_experience_permissions_errors_count",
        Answer::Text("0"),
    ),
    (
        "mono_unserved_get_experience_list_compiled",
        Answer::Flag(true),
    ),
    (
        "mono_unserved_get_experience_list_errors_count",
        Answer::Text("0"),
    ),
    ("mono_unserved_point_at_compiled", Answer::Flag(true)),
    ("mono_unserved_point_at_errors_count", Answer::Text("0")),
    (
        "mono_unserved_remote_load_script_compiled",
        Answer::Flag(true),
    ),
    (
        "mono_unserved_remote_load_script_errors_count",
        Answer::Text("0"),
    ),
    ("mono_unserved_stop_point_at_compiled", Answer::Flag(true)),
    (
        "mono_unserved_stop_point_at_errors_count",
        Answer::Text("0"),
    ),
    ("mono_unterminated_string_compiled", Answer::Flag(false)),
    (
        "mono_unterminated_string_error_0",
        Answer::Text("(4, 22) : ERROR : Name not defined within scope\n"),
    ),
    (
        "mono_unterminated_string_error_0_at",
        Answer::Text("line 5 column 23"),
    ),
    ("mono_unterminated_string_errors_count", Answer::Text("1")),
    ("mono_unterminated_string_fresh_item", Answer::Flag(false)),
    (
        "mono_unterminated_string_item_asset",
        Answer::Text("hidden"),
    ),
    (
        "mono_unterminated_string_new_asset",
        Answer::Text("another"),
    ),
    (
        "mono_unterminated_string_running_echo",
        Answer::Text("true"),
    ),
    ("mono_unterminated_string_upload", Answer::Text("answered")),
    ("mono_v1_compiled", Answer::Flag(true)),
    ("mono_v1_errors_count", Answer::Text("0")),
    ("mono_v1_fresh_item", Answer::Flag(false)),
    ("mono_v1_heard", Answer::Text("v1 start")),
    ("mono_v1_item_asset", Answer::Text("hidden")),
    ("mono_v1_new_asset", Answer::Text("another")),
    ("mono_v1_running_echo", Answer::Text("true")),
    ("mono_v1_source", Answer::Text("v1's source")),
    ("mono_v1_upload", Answer::Text("answered")),
    ("mono_v2_stopped_after_running", Answer::Text("false")),
    ("mono_v2_stopped_compiled", Answer::Flag(true)),
    ("mono_v2_stopped_errors_count", Answer::Text("0")),
    ("mono_v2_stopped_fresh_item", Answer::Flag(false)),
    ("mono_v2_stopped_heard", Answer::Text("nothing")),
    ("mono_v2_stopped_item_asset", Answer::Text("hidden")),
    ("mono_v2_stopped_new_asset", Answer::Text("another")),
    ("mono_v2_stopped_running_echo", Answer::Text("false")),
    ("mono_v2_stopped_source", Answer::Text("v2's source")),
    ("mono_v2_stopped_upload", Answer::Text("answered")),
    ("mono_v3_again_compiled", Answer::Flag(true)),
    ("mono_v3_again_errors_count", Answer::Text("0")),
    ("mono_v3_again_fresh_item", Answer::Flag(false)),
    ("mono_v3_again_heard", Answer::Text("v3 start")),
    ("mono_v3_again_item_asset", Answer::Text("hidden")),
    ("mono_v3_again_new_asset", Answer::Text("another")),
    ("mono_v3_again_running_echo", Answer::Text("true")),
    ("mono_v3_again_upload", Answer::Text("answered")),
    ("mono_v3_running_after_running", Answer::Text("true")),
    ("mono_v3_running_compiled", Answer::Flag(true)),
    ("mono_v3_running_errors_count", Answer::Text("0")),
    ("mono_v3_running_fresh_item", Answer::Flag(false)),
    ("mono_v3_running_heard", Answer::Text("v3 start")),
    ("mono_v3_running_item_asset", Answer::Text("hidden")),
    ("mono_v3_running_new_asset", Answer::Text("another")),
    ("mono_v3_running_running_echo", Answer::Text("true")),
    ("mono_v3_running_upload", Answer::Text("answered")),
];

/// What the local OpenSim was measured to answer, by metric.
const OPENSIM: &[(&str, Answer)] = &[
    ("luau_lsl_source_compiled", Answer::Flag(true)),
    ("luau_lsl_source_errors_count", Answer::Text("0")),
    ("luau_lsl_source_fresh_item", Answer::Flag(false)),
    ("luau_lsl_source_item_asset", Answer::Text("replaced")),
    ("luau_lsl_source_new_asset", Answer::Text("the item's id")),
    ("luau_lsl_source_running_echo", Answer::Text("true")),
    ("luau_lsl_source_upload", Answer::Text("answered")),
    ("luau_luau_syntax_compiled", Answer::Flag(false)),
    (
        "luau_luau_syntax_error_0",
        Answer::Text("(1,6) Error: looking for open paren after assuming local is a function name"),
    ),
    (
        "luau_luau_syntax_error_0_at",
        Answer::Text("line 2 column 7"),
    ),
    ("luau_luau_syntax_errors_count", Answer::Text("1")),
    ("luau_luau_syntax_fresh_item", Answer::Flag(false)),
    ("luau_luau_syntax_item_asset", Answer::Text("replaced")),
    ("luau_luau_syntax_mid_compiled", Answer::Flag(false)),
    (
        "luau_luau_syntax_mid_error_0",
        Answer::Text("(1,6) Error: looking for open paren after assuming local is a function name"),
    ),
    (
        "luau_luau_syntax_mid_error_0_at",
        Answer::Text("line 2 column 7"),
    ),
    ("luau_luau_syntax_mid_errors_count", Answer::Text("1")),
    ("luau_luau_syntax_mid_fresh_item", Answer::Flag(false)),
    ("luau_luau_syntax_mid_item_asset", Answer::Text("replaced")),
    (
        "luau_luau_syntax_mid_new_asset",
        Answer::Text("the item's id"),
    ),
    ("luau_luau_syntax_mid_running_echo", Answer::Text("true")),
    ("luau_luau_syntax_mid_upload", Answer::Text("answered")),
    ("luau_luau_syntax_new_asset", Answer::Text("the item's id")),
    ("luau_luau_syntax_running_echo", Answer::Text("true")),
    ("luau_luau_syntax_upload", Answer::Text("answered")),
    ("luau_luau_valid_compiled", Answer::Flag(false)),
    (
        "luau_luau_valid_error_0",
        Answer::Text(
            "(1,0) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    (
        "luau_luau_valid_error_0_at",
        Answer::Text("line 2 column 1"),
    ),
    ("luau_luau_valid_errors_count", Answer::Text("1")),
    ("luau_luau_valid_fresh_item", Answer::Flag(false)),
    ("luau_luau_valid_item_asset", Answer::Text("replaced")),
    ("luau_luau_valid_item_language", Answer::Text("lsl")),
    ("luau_luau_valid_new_asset", Answer::Text("the item's id")),
    ("luau_luau_valid_running_echo", Answer::Text("true")),
    ("luau_luau_valid_upload", Answer::Text("answered")),
    ("xengine_agent_syntax_compiled", Answer::Flag(true)),
    ("xengine_agent_syntax_errors_count", Answer::Text("0")),
    ("xengine_agent_syntax_item_asset", Answer::Text("replaced")),
    (
        "xengine_agent_syntax_new_asset",
        Answer::Text("the listed asset"),
    ),
    ("xengine_agent_syntax_running_echo", Answer::Text("none")),
    ("xengine_agent_syntax_upload", Answer::Text("answered")),
    ("xengine_agent_valid_compiled", Answer::Flag(true)),
    ("xengine_agent_valid_errors_count", Answer::Text("0")),
    ("xengine_agent_valid_item_asset", Answer::Text("replaced")),
    (
        "xengine_agent_valid_new_asset",
        Answer::Text("the listed asset"),
    ),
    ("xengine_agent_valid_running_echo", Answer::Text("none")),
    ("xengine_agent_valid_upload", Answer::Text("answered")),
    ("xengine_broken_after_ping_heard", Answer::Text("nothing")),
    ("xengine_crlf_compiled", Answer::Flag(false)),
    ("xengine_crlf_error_0", Answer::Text("(5,20) syntax error")),
    ("xengine_crlf_error_0_at", Answer::Text("line 6 column 21")),
    ("xengine_crlf_errors_count", Answer::Text("1")),
    ("xengine_crlf_fresh_item", Answer::Flag(true)),
    ("xengine_crlf_item_asset", Answer::Text("replaced")),
    ("xengine_crlf_new_asset", Answer::Text("the item's id")),
    ("xengine_crlf_running_echo", Answer::Text("true")),
    ("xengine_crlf_upload", Answer::Text("answered")),
    ("xengine_duplicate_global_compiled", Answer::Flag(false)),
    (
        "xengine_duplicate_global_error_0",
        Answer::Text(
            "(2,0): Error CS0102: The type `SecondLife.XEngineScript' already contains a definition for `x'\n",
        ),
    ),
    (
        "xengine_duplicate_global_error_0_at",
        Answer::Text("line 3 column 1"),
    ),
    ("xengine_duplicate_global_errors_count", Answer::Text("1")),
    ("xengine_duplicate_global_fresh_item", Answer::Flag(true)),
    (
        "xengine_duplicate_global_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_duplicate_global_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "xengine_duplicate_global_running_echo",
        Answer::Text("true"),
    ),
    ("xengine_duplicate_global_upload", Answer::Text("answered")),
    ("xengine_empty_compiled", Answer::Flag(false)),
    (
        "xengine_empty_error_0",
        Answer::Text("(2,-1) Unexpected EOF"),
    ),
    ("xengine_empty_error_0_at", Answer::Text("line 3 column 1")),
    ("xengine_empty_errors_count", Answer::Text("1")),
    ("xengine_empty_fresh_item", Answer::Flag(true)),
    ("xengine_empty_item_asset", Answer::Text("replaced")),
    ("xengine_empty_new_asset", Answer::Text("the item's id")),
    ("xengine_empty_running_echo", Answer::Text("true")),
    ("xengine_empty_upload", Answer::Text("answered")),
    ("xengine_large_code_compiled", Answer::Flag(true)),
    ("xengine_large_code_errors_count", Answer::Text("0")),
    ("xengine_large_code_fresh_item", Answer::Flag(true)),
    ("xengine_large_code_heard", Answer::Text("nothing")),
    ("xengine_large_code_item_asset", Answer::Text("replaced")),
    (
        "xengine_large_code_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_large_code_running_echo", Answer::Text("true")),
    ("xengine_large_code_upload", Answer::Text("answered")),
    ("xengine_missing_return_compiled", Answer::Flag(false)),
    (
        "xengine_missing_return_error_0",
        Answer::Text(
            "(1,0): Error CS0161: `SecondLife.XEngineScript.f()': not all code paths return a value\n",
        ),
    ),
    (
        "xengine_missing_return_error_0_at",
        Answer::Text("line 2 column 1"),
    ),
    ("xengine_missing_return_errors_count", Answer::Text("1")),
    ("xengine_missing_return_fresh_item", Answer::Flag(true)),
    (
        "xengine_missing_return_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_missing_return_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_missing_return_running_echo", Answer::Text("true")),
    ("xengine_missing_return_upload", Answer::Text("answered")),
    ("xengine_multibyte_column_compiled", Answer::Flag(false)),
    (
        "xengine_multibyte_column_error_0",
        Answer::Text("(5,38) syntax error"),
    ),
    (
        "xengine_multibyte_column_error_0_at",
        Answer::Text("line 6 column 39"),
    ),
    ("xengine_multibyte_column_errors_count", Answer::Text("1")),
    ("xengine_multibyte_column_fresh_item", Answer::Flag(true)),
    (
        "xengine_multibyte_column_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_multibyte_column_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "xengine_multibyte_column_running_echo",
        Answer::Text("true"),
    ),
    ("xengine_multibyte_column_upload", Answer::Text("answered")),
    ("xengine_no_default_compiled", Answer::Flag(true)),
    ("xengine_no_default_errors_count", Answer::Text("0")),
    ("xengine_no_default_fresh_item", Answer::Flag(true)),
    ("xengine_no_default_item_asset", Answer::Text("replaced")),
    (
        "xengine_no_default_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_no_default_running_echo", Answer::Text("true")),
    ("xengine_no_default_upload", Answer::Text("answered")),
    ("xengine_size_1m_compiled", Answer::Flag(true)),
    ("xengine_size_1m_errors_count", Answer::Text("0")),
    ("xengine_size_1m_fresh_item", Answer::Flag(true)),
    ("xengine_size_1m_item_asset", Answer::Text("replaced")),
    ("xengine_size_1m_new_asset", Answer::Text("the item's id")),
    ("xengine_size_1m_running_echo", Answer::Text("true")),
    ("xengine_size_1m_upload", Answer::Text("answered")),
    ("xengine_size_256k_over_compiled", Answer::Flag(true)),
    ("xengine_size_256k_over_errors_count", Answer::Text("0")),
    ("xengine_size_256k_over_fresh_item", Answer::Flag(true)),
    (
        "xengine_size_256k_over_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_size_256k_over_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_size_256k_over_running_echo", Answer::Text("true")),
    ("xengine_size_256k_over_upload", Answer::Text("answered")),
    ("xengine_size_64k_over_compiled", Answer::Flag(true)),
    ("xengine_size_64k_over_errors_count", Answer::Text("0")),
    ("xengine_size_64k_over_fresh_item", Answer::Flag(true)),
    ("xengine_size_64k_over_item_asset", Answer::Text("replaced")),
    (
        "xengine_size_64k_over_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_size_64k_over_running_echo", Answer::Text("true")),
    ("xengine_size_64k_over_upload", Answer::Text("answered")),
    ("xengine_size_64k_under_compiled", Answer::Flag(true)),
    ("xengine_size_64k_under_errors_count", Answer::Text("0")),
    ("xengine_size_64k_under_fresh_item", Answer::Flag(true)),
    (
        "xengine_size_64k_under_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_size_64k_under_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_size_64k_under_running_echo", Answer::Text("true")),
    ("xengine_size_64k_under_upload", Answer::Text("answered")),
    ("xengine_syntax_after_heard", Answer::Text("nothing")),
    ("xengine_syntax_after_ping_heard", Answer::Text("nothing")),
    ("xengine_syntax_after_running", Answer::Text("no answer")),
    ("xengine_syntax_fresh_item", Answer::Flag(false)),
    ("xengine_syntax_item_asset", Answer::Text("replaced")),
    ("xengine_syntax_new_asset", Answer::Text("none")),
    ("xengine_syntax_source", Answer::Text("syntax's source")),
    ("xengine_syntax_upload", Answer::Text("no answer")),
    ("xengine_tab_column_compiled", Answer::Flag(false)),
    (
        "xengine_tab_column_error_0",
        Answer::Text("(5,14) syntax error"),
    ),
    (
        "xengine_tab_column_error_0_at",
        Answer::Text("line 6 column 15"),
    ),
    ("xengine_tab_column_errors_count", Answer::Text("1")),
    ("xengine_tab_column_fresh_item", Answer::Flag(true)),
    ("xengine_tab_column_item_asset", Answer::Text("replaced")),
    (
        "xengine_tab_column_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_tab_column_running_echo", Answer::Text("true")),
    ("xengine_tab_column_upload", Answer::Text("answered")),
    ("xengine_three_errors_compiled", Answer::Flag(false)),
    (
        "xengine_three_errors_error_0",
        Answer::Text(
            "(5,22): Error CS0266: Cannot implicitly convert type `string' to `integer'. An explicit conversion exists (are you missing a cast?)\n(6,8): Error CS0103: The name `y' does not exist in the current context\n(7,8): Error CS0103: The name `llNope' does not exist in the current context\n",
        ),
    ),
    (
        "xengine_three_errors_error_0_at",
        Answer::Text("line 6 column 23"),
    ),
    ("xengine_three_errors_errors_count", Answer::Text("1")),
    ("xengine_three_errors_fresh_item", Answer::Flag(true)),
    ("xengine_three_errors_item_asset", Answer::Text("replaced")),
    (
        "xengine_three_errors_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_three_errors_running_echo", Answer::Text("true")),
    ("xengine_three_errors_upload", Answer::Text("answered")),
    ("xengine_two_syntax_errors_compiled", Answer::Flag(false)),
    (
        "xengine_two_syntax_errors_error_0",
        Answer::Text("(5,20) syntax error"),
    ),
    (
        "xengine_two_syntax_errors_error_0_at",
        Answer::Text("line 6 column 21"),
    ),
    ("xengine_two_syntax_errors_errors_count", Answer::Text("1")),
    ("xengine_two_syntax_errors_fresh_item", Answer::Flag(true)),
    (
        "xengine_two_syntax_errors_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_two_syntax_errors_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "xengine_two_syntax_errors_running_echo",
        Answer::Text("true"),
    ),
    ("xengine_two_syntax_errors_upload", Answer::Text("answered")),
    ("xengine_type_mismatch_compiled", Answer::Flag(false)),
    (
        "xengine_type_mismatch_error_0",
        Answer::Text(
            "(5,22): Error CS0266: Cannot implicitly convert type `string' to `integer'. An explicit conversion exists (are you missing a cast?)\n",
        ),
    ),
    (
        "xengine_type_mismatch_error_0_at",
        Answer::Text("line 6 column 23"),
    ),
    ("xengine_type_mismatch_errors_count", Answer::Text("1")),
    ("xengine_type_mismatch_fresh_item", Answer::Flag(true)),
    ("xengine_type_mismatch_item_asset", Answer::Text("replaced")),
    (
        "xengine_type_mismatch_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_type_mismatch_running_echo", Answer::Text("true")),
    ("xengine_type_mismatch_upload", Answer::Text("answered")),
    ("xengine_undeclared_compiled", Answer::Flag(false)),
    (
        "xengine_undeclared_error_0",
        Answer::Text("(5,8): Error CS0103: The name `y' does not exist in the current context\n"),
    ),
    (
        "xengine_undeclared_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("xengine_undeclared_errors_count", Answer::Text("1")),
    ("xengine_undeclared_fresh_item", Answer::Flag(true)),
    ("xengine_undeclared_item_asset", Answer::Text("replaced")),
    (
        "xengine_undeclared_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_undeclared_running_echo", Answer::Text("true")),
    ("xengine_undeclared_upload", Answer::Text("answered")),
    ("xengine_unknown_function_compiled", Answer::Flag(false)),
    (
        "xengine_unknown_function_error_0",
        Answer::Text(
            "(5,8): Error CS0103: The name `llNope' does not exist in the current context\n",
        ),
    ),
    (
        "xengine_unknown_function_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("xengine_unknown_function_errors_count", Answer::Text("1")),
    ("xengine_unknown_function_fresh_item", Answer::Flag(true)),
    (
        "xengine_unknown_function_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_unknown_function_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "xengine_unknown_function_running_echo",
        Answer::Text("true"),
    ),
    ("xengine_unknown_function_upload", Answer::Text("answered")),
    ("xengine_unknown_label_compiled", Answer::Flag(false)),
    (
        "xengine_unknown_label_error_0",
        Answer::Text(
            "(5,8): Error CS0159: The label `nowhere:' could not be found within the scope of the goto statement\n",
        ),
    ),
    (
        "xengine_unknown_label_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("xengine_unknown_label_errors_count", Answer::Text("1")),
    ("xengine_unknown_label_fresh_item", Answer::Flag(true)),
    ("xengine_unknown_label_item_asset", Answer::Text("replaced")),
    (
        "xengine_unknown_label_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_unknown_label_running_echo", Answer::Text("true")),
    ("xengine_unknown_label_upload", Answer::Text("answered")),
    ("xengine_unknown_state_compiled", Answer::Flag(true)),
    ("xengine_unknown_state_errors_count", Answer::Text("0")),
    ("xengine_unknown_state_fresh_item", Answer::Flag(true)),
    ("xengine_unknown_state_item_asset", Answer::Text("replaced")),
    (
        "xengine_unknown_state_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_unknown_state_running_echo", Answer::Text("true")),
    ("xengine_unknown_state_upload", Answer::Text("answered")),
    (
        "xengine_unserved_clear_experience_permissions_compiled",
        Answer::Flag(false),
    ),
    (
        "xengine_unserved_clear_experience_permissions_error_0",
        Answer::Text(
            "(5,8): Error CS0103: The name `llClearExperiencePermissions' does not exist in the current context\n",
        ),
    ),
    (
        "xengine_unserved_clear_experience_permissions_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    (
        "xengine_unserved_clear_experience_permissions_errors_count",
        Answer::Text("1"),
    ),
    (
        "xengine_unserved_get_experience_list_compiled",
        Answer::Flag(false),
    ),
    (
        "xengine_unserved_get_experience_list_error_0",
        Answer::Text(
            "(5,30): Error CS0103: The name `llGetExperienceList' does not exist in the current context\n",
        ),
    ),
    (
        "xengine_unserved_get_experience_list_error_0_at",
        Answer::Text("line 6 column 31"),
    ),
    (
        "xengine_unserved_get_experience_list_errors_count",
        Answer::Text("1"),
    ),
    ("xengine_unserved_point_at_compiled", Answer::Flag(true)),
    ("xengine_unserved_point_at_errors_count", Answer::Text("0")),
    (
        "xengine_unserved_remote_load_script_compiled",
        Answer::Flag(true),
    ),
    (
        "xengine_unserved_remote_load_script_errors_count",
        Answer::Text("0"),
    ),
    (
        "xengine_unserved_stop_point_at_compiled",
        Answer::Flag(true),
    ),
    (
        "xengine_unserved_stop_point_at_errors_count",
        Answer::Text("0"),
    ),
    ("xengine_unterminated_string_compiled", Answer::Flag(false)),
    (
        "xengine_unterminated_string_error_0",
        Answer::Text("(9,-1) Unexpected EOF"),
    ),
    (
        "xengine_unterminated_string_error_0_at",
        Answer::Text("line 10 column 1"),
    ),
    (
        "xengine_unterminated_string_errors_count",
        Answer::Text("1"),
    ),
    ("xengine_unterminated_string_fresh_item", Answer::Flag(true)),
    (
        "xengine_unterminated_string_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "xengine_unterminated_string_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "xengine_unterminated_string_running_echo",
        Answer::Text("true"),
    ),
    (
        "xengine_unterminated_string_upload",
        Answer::Text("answered"),
    ),
    ("xengine_v1_compiled", Answer::Flag(true)),
    ("xengine_v1_errors_count", Answer::Text("0")),
    ("xengine_v1_fresh_item", Answer::Flag(false)),
    ("xengine_v1_heard", Answer::Text("v1 start")),
    ("xengine_v1_item_asset", Answer::Text("replaced")),
    ("xengine_v1_new_asset", Answer::Text("the item's id")),
    ("xengine_v1_running_echo", Answer::Text("true")),
    ("xengine_v1_source", Answer::Text("v1's source")),
    ("xengine_v1_upload", Answer::Text("answered")),
    ("xengine_v2_stopped_after_running", Answer::Text("false")),
    ("xengine_v2_stopped_compiled", Answer::Flag(true)),
    ("xengine_v2_stopped_errors_count", Answer::Text("0")),
    ("xengine_v2_stopped_fresh_item", Answer::Flag(true)),
    ("xengine_v2_stopped_heard", Answer::Text("nothing")),
    ("xengine_v2_stopped_item_asset", Answer::Text("replaced")),
    (
        "xengine_v2_stopped_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_v2_stopped_running_echo", Answer::Text("false")),
    ("xengine_v2_stopped_source", Answer::Text("v2's source")),
    ("xengine_v2_stopped_upload", Answer::Text("answered")),
    ("xengine_v3_again_fresh_item", Answer::Flag(false)),
    ("xengine_v3_again_heard", Answer::Text("v3 start")),
    ("xengine_v3_again_item_asset", Answer::Text("replaced")),
    ("xengine_v3_again_new_asset", Answer::Text("none")),
    ("xengine_v3_again_upload", Answer::Text("no answer")),
    ("xengine_v3_running_after_running", Answer::Text("true")),
    ("xengine_v3_running_compiled", Answer::Flag(true)),
    ("xengine_v3_running_errors_count", Answer::Text("0")),
    ("xengine_v3_running_fresh_item", Answer::Flag(true)),
    ("xengine_v3_running_heard", Answer::Text("v3 start")),
    ("xengine_v3_running_item_asset", Answer::Text("replaced")),
    (
        "xengine_v3_running_new_asset",
        Answer::Text("the item's id"),
    ),
    ("xengine_v3_running_running_echo", Answer::Text("true")),
    ("xengine_v3_running_upload", Answer::Text("answered")),
    ("yengine_agent_syntax_compiled", Answer::Flag(true)),
    ("yengine_agent_syntax_errors_count", Answer::Text("0")),
    ("yengine_agent_syntax_item_asset", Answer::Text("replaced")),
    (
        "yengine_agent_syntax_new_asset",
        Answer::Text("the listed asset"),
    ),
    ("yengine_agent_syntax_running_echo", Answer::Text("none")),
    ("yengine_agent_syntax_upload", Answer::Text("answered")),
    ("yengine_agent_valid_compiled", Answer::Flag(true)),
    ("yengine_agent_valid_errors_count", Answer::Text("0")),
    ("yengine_agent_valid_item_asset", Answer::Text("replaced")),
    (
        "yengine_agent_valid_new_asset",
        Answer::Text("the listed asset"),
    ),
    ("yengine_agent_valid_running_echo", Answer::Text("none")),
    ("yengine_agent_valid_upload", Answer::Text("answered")),
    ("yengine_broken_after_ping_heard", Answer::Text("nothing")),
    ("yengine_crlf_compiled", Answer::Flag(false)),
    (
        "yengine_crlf_error_0",
        Answer::Text("(5,20) Error: invalid operand token"),
    ),
    ("yengine_crlf_error_0_at", Answer::Text("line 6 column 21")),
    (
        "yengine_crlf_error_1",
        Answer::Text(
            "(6,4) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    (
        "yengine_crlf_error_2",
        Answer::Text(
            "(7,0) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    ("yengine_crlf_errors_count", Answer::Text("3")),
    ("yengine_crlf_fresh_item", Answer::Flag(false)),
    ("yengine_crlf_item_asset", Answer::Text("replaced")),
    ("yengine_crlf_new_asset", Answer::Text("the item's id")),
    ("yengine_crlf_running_echo", Answer::Text("true")),
    ("yengine_crlf_upload", Answer::Text("answered")),
    ("yengine_duplicate_global_compiled", Answer::Flag(false)),
    (
        "yengine_duplicate_global_error_0",
        Answer::Text("(2,8) Error: duplicate variable x"),
    ),
    (
        "yengine_duplicate_global_error_0_at",
        Answer::Text("line 3 column 9"),
    ),
    ("yengine_duplicate_global_errors_count", Answer::Text("1")),
    ("yengine_duplicate_global_fresh_item", Answer::Flag(false)),
    (
        "yengine_duplicate_global_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_duplicate_global_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "yengine_duplicate_global_running_echo",
        Answer::Text("true"),
    ),
    ("yengine_duplicate_global_upload", Answer::Text("answered")),
    ("yengine_empty_compiled", Answer::Flag(false)),
    (
        "yengine_empty_error_0",
        Answer::Text("fetched source empty asset://<id>"),
    ),
    ("yengine_empty_error_0_at", Answer::Text("nowhere")),
    ("yengine_empty_errors_count", Answer::Text("1")),
    ("yengine_empty_fresh_item", Answer::Flag(false)),
    ("yengine_empty_item_asset", Answer::Text("replaced")),
    ("yengine_empty_new_asset", Answer::Text("the item's id")),
    ("yengine_empty_running_echo", Answer::Text("true")),
    ("yengine_empty_upload", Answer::Text("answered")),
    ("yengine_large_code_compiled", Answer::Flag(true)),
    ("yengine_large_code_errors_count", Answer::Text("0")),
    ("yengine_large_code_fresh_item", Answer::Flag(false)),
    ("yengine_large_code_heard", Answer::Text("nothing")),
    ("yengine_large_code_item_asset", Answer::Text("replaced")),
    (
        "yengine_large_code_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_large_code_running_echo", Answer::Text("true")),
    ("yengine_large_code_upload", Answer::Text("answered")),
    ("yengine_missing_return_compiled", Answer::Flag(true)),
    ("yengine_missing_return_errors_count", Answer::Text("0")),
    ("yengine_missing_return_fresh_item", Answer::Flag(false)),
    (
        "yengine_missing_return_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_missing_return_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_missing_return_running_echo", Answer::Text("true")),
    ("yengine_missing_return_upload", Answer::Text("answered")),
    ("yengine_multibyte_column_compiled", Answer::Flag(false)),
    (
        "yengine_multibyte_column_error_0",
        Answer::Text("(5,38) Error: invalid operand token"),
    ),
    (
        "yengine_multibyte_column_error_0_at",
        Answer::Text("line 6 column 39"),
    ),
    (
        "yengine_multibyte_column_error_1",
        Answer::Text(
            "(6,4) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    (
        "yengine_multibyte_column_error_2",
        Answer::Text(
            "(7,0) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    ("yengine_multibyte_column_errors_count", Answer::Text("3")),
    ("yengine_multibyte_column_fresh_item", Answer::Flag(false)),
    (
        "yengine_multibyte_column_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_multibyte_column_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "yengine_multibyte_column_running_echo",
        Answer::Text("true"),
    ),
    ("yengine_multibyte_column_upload", Answer::Text("answered")),
    ("yengine_no_default_compiled", Answer::Flag(false)),
    (
        "yengine_no_default_error_0",
        Answer::Text("(1,0) Error: no default state defined"),
    ),
    (
        "yengine_no_default_error_0_at",
        Answer::Text("line 2 column 1"),
    ),
    ("yengine_no_default_errors_count", Answer::Text("1")),
    ("yengine_no_default_fresh_item", Answer::Flag(false)),
    ("yengine_no_default_item_asset", Answer::Text("replaced")),
    (
        "yengine_no_default_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_no_default_running_echo", Answer::Text("true")),
    ("yengine_no_default_upload", Answer::Text("answered")),
    ("yengine_size_1m_compiled", Answer::Flag(true)),
    ("yengine_size_1m_errors_count", Answer::Text("0")),
    ("yengine_size_1m_fresh_item", Answer::Flag(false)),
    ("yengine_size_1m_item_asset", Answer::Text("replaced")),
    ("yengine_size_1m_new_asset", Answer::Text("the item's id")),
    ("yengine_size_1m_running_echo", Answer::Text("true")),
    ("yengine_size_1m_upload", Answer::Text("answered")),
    ("yengine_size_256k_over_compiled", Answer::Flag(true)),
    ("yengine_size_256k_over_errors_count", Answer::Text("0")),
    ("yengine_size_256k_over_fresh_item", Answer::Flag(false)),
    (
        "yengine_size_256k_over_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_size_256k_over_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_size_256k_over_running_echo", Answer::Text("true")),
    ("yengine_size_256k_over_upload", Answer::Text("answered")),
    ("yengine_size_64k_over_compiled", Answer::Flag(true)),
    ("yengine_size_64k_over_errors_count", Answer::Text("0")),
    ("yengine_size_64k_over_fresh_item", Answer::Flag(false)),
    ("yengine_size_64k_over_item_asset", Answer::Text("replaced")),
    (
        "yengine_size_64k_over_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_size_64k_over_running_echo", Answer::Text("true")),
    ("yengine_size_64k_over_upload", Answer::Text("answered")),
    ("yengine_size_64k_under_compiled", Answer::Flag(true)),
    ("yengine_size_64k_under_errors_count", Answer::Text("0")),
    ("yengine_size_64k_under_fresh_item", Answer::Flag(false)),
    (
        "yengine_size_64k_under_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_size_64k_under_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_size_64k_under_running_echo", Answer::Text("true")),
    ("yengine_size_64k_under_upload", Answer::Text("answered")),
    ("yengine_syntax_after_heard", Answer::Text("nothing")),
    ("yengine_syntax_after_ping_heard", Answer::Text("nothing")),
    ("yengine_syntax_after_running", Answer::Text("no answer")),
    ("yengine_syntax_compiled", Answer::Flag(false)),
    (
        "yengine_syntax_error_0",
        Answer::Text("(5,20) Error: invalid operand token"),
    ),
    (
        "yengine_syntax_error_0_at",
        Answer::Text("line 6 column 21"),
    ),
    (
        "yengine_syntax_error_1",
        Answer::Text(
            "(6,4) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    (
        "yengine_syntax_error_2",
        Answer::Text(
            "(7,0) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    ("yengine_syntax_errors_count", Answer::Text("3")),
    ("yengine_syntax_fresh_item", Answer::Flag(false)),
    ("yengine_syntax_item_asset", Answer::Text("replaced")),
    ("yengine_syntax_new_asset", Answer::Text("the item's id")),
    ("yengine_syntax_running_echo", Answer::Text("true")),
    ("yengine_syntax_source", Answer::Text("syntax's source")),
    ("yengine_syntax_upload", Answer::Text("answered")),
    ("yengine_tab_column_compiled", Answer::Flag(false)),
    (
        "yengine_tab_column_error_0",
        Answer::Text("(5,14) Error: invalid operand token"),
    ),
    (
        "yengine_tab_column_error_0_at",
        Answer::Text("line 6 column 15"),
    ),
    (
        "yengine_tab_column_error_1",
        Answer::Text(
            "(6,4) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    (
        "yengine_tab_column_error_2",
        Answer::Text(
            "(7,0) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    ("yengine_tab_column_errors_count", Answer::Text("3")),
    ("yengine_tab_column_fresh_item", Answer::Flag(false)),
    ("yengine_tab_column_item_asset", Answer::Text("replaced")),
    (
        "yengine_tab_column_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_tab_column_running_echo", Answer::Text("true")),
    ("yengine_tab_column_upload", Answer::Text("answered")),
    ("yengine_three_errors_compiled", Answer::Flag(false)),
    (
        "yengine_three_errors_error_0",
        Answer::Text("(5,20) Error: must explicitly cast from string to integer"),
    ),
    (
        "yengine_three_errors_error_0_at",
        Answer::Text("line 6 column 21"),
    ),
    (
        "yengine_three_errors_error_1",
        Answer::Text("(6,8) Error: undefined constant/function/variable y"),
    ),
    (
        "yengine_three_errors_error_2",
        Answer::Text("(7,8) Error: undefined constant/function/variable llNope()"),
    ),
    ("yengine_three_errors_errors_count", Answer::Text("3")),
    ("yengine_three_errors_fresh_item", Answer::Flag(false)),
    ("yengine_three_errors_item_asset", Answer::Text("replaced")),
    (
        "yengine_three_errors_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_three_errors_running_echo", Answer::Text("true")),
    ("yengine_three_errors_upload", Answer::Text("answered")),
    ("yengine_two_syntax_errors_compiled", Answer::Flag(false)),
    (
        "yengine_two_syntax_errors_error_0",
        Answer::Text("(5,20) Error: invalid operand token"),
    ),
    (
        "yengine_two_syntax_errors_error_0_at",
        Answer::Text("line 6 column 21"),
    ),
    (
        "yengine_two_syntax_errors_error_1",
        Answer::Text("(6,20) Error: invalid operand token"),
    ),
    (
        "yengine_two_syntax_errors_error_2",
        Answer::Text(
            "(7,4) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    (
        "yengine_two_syntax_errors_error_3",
        Answer::Text(
            "(8,0) Error: looking for var name, type, state or default, script-defined type declaration",
        ),
    ),
    ("yengine_two_syntax_errors_errors_count", Answer::Text("4")),
    ("yengine_two_syntax_errors_fresh_item", Answer::Flag(false)),
    (
        "yengine_two_syntax_errors_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_two_syntax_errors_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "yengine_two_syntax_errors_running_echo",
        Answer::Text("true"),
    ),
    ("yengine_two_syntax_errors_upload", Answer::Text("answered")),
    ("yengine_type_mismatch_compiled", Answer::Flag(false)),
    (
        "yengine_type_mismatch_error_0",
        Answer::Text("(5,20) Error: must explicitly cast from string to integer"),
    ),
    (
        "yengine_type_mismatch_error_0_at",
        Answer::Text("line 6 column 21"),
    ),
    ("yengine_type_mismatch_errors_count", Answer::Text("1")),
    ("yengine_type_mismatch_fresh_item", Answer::Flag(false)),
    ("yengine_type_mismatch_item_asset", Answer::Text("replaced")),
    (
        "yengine_type_mismatch_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_type_mismatch_running_echo", Answer::Text("true")),
    ("yengine_type_mismatch_upload", Answer::Text("answered")),
    ("yengine_undeclared_compiled", Answer::Flag(false)),
    (
        "yengine_undeclared_error_0",
        Answer::Text("(5,8) Error: undefined constant/function/variable y"),
    ),
    (
        "yengine_undeclared_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("yengine_undeclared_errors_count", Answer::Text("1")),
    ("yengine_undeclared_fresh_item", Answer::Flag(false)),
    ("yengine_undeclared_item_asset", Answer::Text("replaced")),
    (
        "yengine_undeclared_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_undeclared_running_echo", Answer::Text("true")),
    ("yengine_undeclared_upload", Answer::Text("answered")),
    ("yengine_unknown_function_compiled", Answer::Flag(false)),
    (
        "yengine_unknown_function_error_0",
        Answer::Text("(5,8) Error: undefined constant/function/variable llNope()"),
    ),
    (
        "yengine_unknown_function_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("yengine_unknown_function_errors_count", Answer::Text("1")),
    ("yengine_unknown_function_fresh_item", Answer::Flag(false)),
    (
        "yengine_unknown_function_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_unknown_function_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "yengine_unknown_function_running_echo",
        Answer::Text("true"),
    ),
    ("yengine_unknown_function_upload", Answer::Text("answered")),
    ("yengine_unknown_label_compiled", Answer::Flag(false)),
    (
        "yengine_unknown_label_error_0",
        Answer::Text("(5,8) Error: undefined label nowhere"),
    ),
    (
        "yengine_unknown_label_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("yengine_unknown_label_errors_count", Answer::Text("1")),
    ("yengine_unknown_label_fresh_item", Answer::Flag(false)),
    ("yengine_unknown_label_item_asset", Answer::Text("replaced")),
    (
        "yengine_unknown_label_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_unknown_label_running_echo", Answer::Text("true")),
    ("yengine_unknown_label_upload", Answer::Text("answered")),
    ("yengine_unknown_state_compiled", Answer::Flag(false)),
    (
        "yengine_unknown_state_error_0",
        Answer::Text("(5,8) Error: undefined state other"),
    ),
    (
        "yengine_unknown_state_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    ("yengine_unknown_state_errors_count", Answer::Text("1")),
    ("yengine_unknown_state_fresh_item", Answer::Flag(false)),
    ("yengine_unknown_state_item_asset", Answer::Text("replaced")),
    (
        "yengine_unknown_state_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_unknown_state_running_echo", Answer::Text("true")),
    ("yengine_unknown_state_upload", Answer::Text("answered")),
    (
        "yengine_unserved_clear_experience_permissions_compiled",
        Answer::Flag(false),
    ),
    (
        "yengine_unserved_clear_experience_permissions_error_0",
        Answer::Text(
            "(5,8) Error: undefined constant/function/variable llClearExperiencePermissions(string)",
        ),
    ),
    (
        "yengine_unserved_clear_experience_permissions_error_0_at",
        Answer::Text("line 6 column 9"),
    ),
    (
        "yengine_unserved_clear_experience_permissions_errors_count",
        Answer::Text("1"),
    ),
    (
        "yengine_unserved_get_experience_list_compiled",
        Answer::Flag(false),
    ),
    (
        "yengine_unserved_get_experience_list_error_0",
        Answer::Text(
            "(5,30) Error: undefined constant/function/variable llGetExperienceList(string)",
        ),
    ),
    (
        "yengine_unserved_get_experience_list_error_0_at",
        Answer::Text("line 6 column 31"),
    ),
    (
        "yengine_unserved_get_experience_list_errors_count",
        Answer::Text("1"),
    ),
    ("yengine_unserved_point_at_compiled", Answer::Flag(true)),
    ("yengine_unserved_point_at_errors_count", Answer::Text("0")),
    (
        "yengine_unserved_remote_load_script_compiled",
        Answer::Flag(true),
    ),
    (
        "yengine_unserved_remote_load_script_errors_count",
        Answer::Text("0"),
    ),
    (
        "yengine_unserved_stop_point_at_compiled",
        Answer::Flag(true),
    ),
    (
        "yengine_unserved_stop_point_at_errors_count",
        Answer::Text("0"),
    ),
    ("yengine_unterminated_string_compiled", Answer::Flag(false)),
    (
        "yengine_unterminated_string_error_0",
        Answer::Text("(9,0) Error: invalid operator"),
    ),
    (
        "yengine_unterminated_string_error_0_at",
        Answer::Text("line 10 column 1"),
    ),
    (
        "yengine_unterminated_string_errors_count",
        Answer::Text("1"),
    ),
    (
        "yengine_unterminated_string_fresh_item",
        Answer::Flag(false),
    ),
    (
        "yengine_unterminated_string_item_asset",
        Answer::Text("replaced"),
    ),
    (
        "yengine_unterminated_string_new_asset",
        Answer::Text("the item's id"),
    ),
    (
        "yengine_unterminated_string_running_echo",
        Answer::Text("true"),
    ),
    (
        "yengine_unterminated_string_upload",
        Answer::Text("answered"),
    ),
    ("yengine_v1_compiled", Answer::Flag(true)),
    ("yengine_v1_errors_count", Answer::Text("0")),
    ("yengine_v1_fresh_item", Answer::Flag(false)),
    ("yengine_v1_heard", Answer::Text("v1 start")),
    ("yengine_v1_item_asset", Answer::Text("replaced")),
    ("yengine_v1_new_asset", Answer::Text("the item's id")),
    ("yengine_v1_running_echo", Answer::Text("true")),
    ("yengine_v1_source", Answer::Text("v1's source")),
    ("yengine_v1_upload", Answer::Text("answered")),
    ("yengine_v2_stopped_after_running", Answer::Text("false")),
    ("yengine_v2_stopped_compiled", Answer::Flag(true)),
    ("yengine_v2_stopped_errors_count", Answer::Text("0")),
    ("yengine_v2_stopped_fresh_item", Answer::Flag(false)),
    ("yengine_v2_stopped_heard", Answer::Text("nothing")),
    ("yengine_v2_stopped_item_asset", Answer::Text("replaced")),
    (
        "yengine_v2_stopped_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_v2_stopped_running_echo", Answer::Text("false")),
    ("yengine_v2_stopped_source", Answer::Text("v2's source")),
    ("yengine_v2_stopped_upload", Answer::Text("answered")),
    ("yengine_v3_again_compiled", Answer::Flag(true)),
    ("yengine_v3_again_errors_count", Answer::Text("0")),
    ("yengine_v3_again_fresh_item", Answer::Flag(false)),
    ("yengine_v3_again_heard", Answer::Text("v3 start")),
    ("yengine_v3_again_item_asset", Answer::Text("replaced")),
    ("yengine_v3_again_new_asset", Answer::Text("the item's id")),
    ("yengine_v3_again_running_echo", Answer::Text("true")),
    ("yengine_v3_again_upload", Answer::Text("answered")),
    ("yengine_v3_running_after_running", Answer::Text("true")),
    ("yengine_v3_running_compiled", Answer::Flag(true)),
    ("yengine_v3_running_errors_count", Answer::Text("0")),
    ("yengine_v3_running_fresh_item", Answer::Flag(false)),
    ("yengine_v3_running_heard", Answer::Text("v3 start")),
    ("yengine_v3_running_item_asset", Answer::Text("replaced")),
    (
        "yengine_v3_running_new_asset",
        Answer::Text("the item's id"),
    ),
    ("yengine_v3_running_running_echo", Answer::Text("true")),
    ("yengine_v3_running_upload", Answer::Text("answered")),
];

/// Holds `metrics` to `grid`'s table.
fn hold(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let table = if is_opensim(grid) {
        OPENSIM
    } else {
        SECOND_LIFE
    };
    let failures: Vec<String> = table
        .iter()
        .filter_map(|(key, expected)| match metrics.get(key) {
            Some(value) if expected.is(value) => None,
            Some(value) => Some(format!("{key} was {value:?}, measured {expected:?}")),
            None => Some(format!("{key} was not recorded, measured {expected:?}")),
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(TestFailure::Assertion(format!(
        "{grid}: the grid no longer matches {SOURCE}: {}",
        failures.join("; ")
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        BROKEN, Backend, LUAU, UNSERVED, large_code_source, sized_source, valid_source,
        without_uuids,
    };
    use pretty_assertions::assert_eq;

    /// `source` compiles under our own compiler, or the error says why.
    fn compiles(what: &str, source: &str) -> Result<(), String> {
        sl_lsl_runtime::compile(source)
            .map(|_program| ())
            .map_err(|errors| format!("{what} does not compile: {errors:?}"))
    }

    /// Every broken source fails our own compiler, and the valid ones pass
    /// it — a leg that measures nothing costs a live run. The unserved
    /// functions' calls compile, as they do on Second Life.
    #[test]
    fn the_sources_are_what_they_claim() -> Result<(), String> {
        for broken in BROKEN {
            if sl_lsl_runtime::compile(broken.source).is_ok() {
                return Err(format!("{} compiles", broken.name));
            }
        }
        for backend in [Backend::Mono, Backend::YEngine] {
            for version in ["v1", "v2", "v3"] {
                compiles(version, &valid_source(backend, version))?;
            }
        }
        compiles("large_code", &large_code_source())?;
        for leg in UNSERVED {
            compiles(leg.name, leg.source)?;
        }
        for leg in LUAU.iter().filter(|leg| leg.name == "lsl_source") {
            compiles(leg.name, leg.source)?;
        }
        Ok(())
    }

    /// An error loses its UUIDs and keeps its own numbers.
    #[test]
    fn only_uuids_are_scrubbed() {
        assert_eq!(
            without_uuids("fetched source empty asset://6bbe62d3-e800-40f7-a673-8e5220c8024a"),
            "fetched source empty asset://<id>"
        );
        assert_eq!(
            without_uuids("Object [script:Compile] Script run-time error"),
            "Object [script:Compile] Script run-time error"
        );
        assert_eq!(
            without_uuids("Stack-Heap Collision"),
            "Stack-Heap Collision"
        );
        assert_eq!(
            without_uuids("(5,8): Error CS0103: The name `y' does not exist äö"),
            "(5,8): Error CS0103: The name `y' does not exist äö"
        );
    }

    /// A sized source is exactly its size and compiles.
    #[test]
    fn a_sized_source_is_its_size() -> Result<(), String> {
        for size in [65_000, 66_000, 262_200] {
            let source = sized_source(size);
            assert_eq!(source.len(), size);
            compiles("a sized source", &source)?;
        }
        Ok(())
    }
}
