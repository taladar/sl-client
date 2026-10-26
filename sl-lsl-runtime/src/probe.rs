//! The **probe corpus** format: LSL scripts that print what a grid did as
//! machine-parsable lines, and the per-grid result files that keep what they
//! printed. The conventions are the book chapter `tools/lsl-probes.md`; the
//! corpus is `sl-lsl-runtime/probes/`.
//!
//! - A probe is one script, or several that run together in one or more
//!   prims; its id is its path below `probes/` without the extension
//!   (`events/queue-limit`, `errors/runtime-errors`).
//! - Every script opens with a [`Header`] of `// key: value` comment lines.
//! - What a probe prints is [`Line`]s:
//!   `PROBE <id> rec=<name> key=value…`: one `start`, from the script that
//!   leads it, and one `end`, last. In a probe of several scripts the ones
//!   saved before the leader may print first.
//! - A probe asks the runner for a stimulus — a touch, a line of chat, an
//!   answer to its permission question or its dialog, a sit, a stand, held
//!   controls, the owner leaving the region for a while — with a
//!   `waiting` record, read as a [`Stimulus`].
//! - A run's transcript is a [`ResultFile`]: a header of `key: value` lines,
//!   a blank line, then the `PROBE` lines with `#` notes between.
//! - Two runs are set side by side by [`compare`](compare::compare), under
//!   the tolerance its module describes.
//!
//! Like the rest of the crate this is I/O-free: callers read the files.

use std::collections::BTreeMap;

pub mod compare;

/// A stimulus a probe may ask the runner for, with a `waiting` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Drive {
    /// A touch of the probe's prim A.
    Touch,
    /// A line of chat on a channel.
    Chat,
    /// An answer to the permission question the probe's script puts.
    Answer,
    /// Sitting on the probe's prim A.
    Sit,
    /// Standing up.
    Stand,
    /// Holding movement controls down for a moment.
    Control,
    /// A reply to the dialog (`llDialog`, `llTextBox`) the probe's script
    /// puts.
    Dialog,
    /// The owner leaving the probe's region for a while.
    Away,
    /// The owner attaching the probe's prim A to itself from the ground.
    Attach,
    /// The owner dropping the probe's prim A, worn, to the ground.
    Drop,
    /// The owner taking the probe's prim A into its inventory and rezzing it
    /// back where it stood.
    Rerez,
    /// An HTTP request from outside the grid to a URL the probe's script
    /// was given (`llRequestURL`).
    Fetch,
    /// Paying one of the probe's prims an amount of L$ (`MoneyTransferRequest`).
    Pay,
    /// Asking one of the probe's prims for its pay buttons (`RequestPayPrice`).
    PayPrice,
    /// Revoking script permissions, as a viewer does (`RevokePermissions`).
    Revoke,
}

impl Drive {
    /// The drive a header or `waiting` record names.
    fn parse(name: &str) -> Option<Self> {
        match name {
            "touch" => Some(Self::Touch),
            "chat" => Some(Self::Chat),
            "answer" => Some(Self::Answer),
            "sit" => Some(Self::Sit),
            "stand" => Some(Self::Stand),
            "control" => Some(Self::Control),
            "dialog" => Some(Self::Dialog),
            "away" => Some(Self::Away),
            "attach" => Some(Self::Attach),
            "drop" => Some(Self::Drop),
            "rerez" => Some(Self::Rerez),
            "fetch" => Some(Self::Fetch),
            "pay" => Some(Self::Pay),
            "pay-price" => Some(Self::PayPrice),
            "revoke" => Some(Self::Revoke),
            _ => None,
        }
    }
}

/// The compile target a probe script asks for (its header's `target`).
///
/// Second Life compiles a script to the target its upload names; OpenSim
/// ignores the upload's target and compiles every script for the engine its
/// first line picks, so there an LSO script runs as every other does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Target {
    /// Mono (`mono`), Second Life's default.
    #[default]
    Mono,
    /// The legacy LSO bytecode (`lsl2`), with its 16 KB budget.
    Lsl2,
}

impl Target {
    /// The target a header names.
    fn parse(name: &str) -> Option<Self> {
        match name {
            "mono" => Some(Self::Mono),
            "lsl2" => Some(Self::Lsl2),
            _ => None,
        }
    }
}

/// The library a probe script is written against (its header's `library`).
///
/// The corpus compiles every script against Second Life's library, the only
/// one the workspace's compiler knows. A script for OpenSim calls OSSL and
/// its sibling families as well, which that compiler cannot resolve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Library {
    /// Second Life's (`sl`), the default.
    #[default]
    SecondLife,
    /// OpenSim's (`opensim`): Second Life's plus the `os*` (OSSL), `ls*`
    /// (LightShare) and `mod*` (MOD) functions and their constants.
    OpenSim,
}

impl Library {
    /// The library a header names.
    fn parse(name: &str) -> Option<Self> {
        match name {
            "sl" => Some(Self::SecondLife),
            "opensim" => Some(Self::OpenSim),
            _ => None,
        }
    }

    /// Whether `name`, which Second Life's library does not define, is one
    /// this library adds: a function of the OSSL, LightShare or MOD family,
    /// or one of their constants.
    #[must_use]
    pub fn adds(self, name: &str) -> bool {
        match self {
            Self::SecondLife => false,
            Self::OpenSim => {
                let family = |prefix: &str| {
                    name.strip_prefix(prefix)
                        .and_then(|rest| rest.chars().next())
                        .is_some_and(|first| first.is_ascii_uppercase())
                };
                family("os")
                    || family("ls")
                    || family("mod")
                    || ["OS_", "OSTPOBJ_", "NPCLOOKAT_", "WL_"]
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
            }
        }
    }
}

/// The `// key: value` lines that open a probe script.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// `probe`: the probe's id.
    pub probe: String,
    /// `drive`: `none`, or the [`Drive`]s the probe asks for, comma-separated.
    pub drives: Vec<Drive>,
    /// `script`: the inventory name the script must have — required in a
    /// probe of several scripts, which find each other by name.
    pub script: Option<String>,
    /// `prim`: which object holds the script (`A`, `B`, …); `A` when absent.
    pub prim: String,
    /// `order`: where the script comes in the order the scripts are saved —
    /// required in a probe of several scripts.
    pub order: Option<u32>,
    /// `distance`: how far east of prim A, in metres, this script's prim
    /// stands.
    pub distance: Option<f32>,
    /// `height`: how far above prim A, in metres, this script's prim
    /// stands — for a range no parcel is wide enough for, since a range is
    /// a distance in three dimensions.
    pub height: Option<f32>,
    /// `skip-on`: the [`GRIDS`] this script is left out on, comma-separated
    /// — where running it would take the region down rather than measure
    /// anything (the script's comment says why).
    pub skip_on: Vec<String>,
    /// `vary`: the fields that are measurements — a duration, a rate, a
    /// clock — which two runs need not agree on, comma-separated.
    pub vary: Vec<String>,
    /// `unordered`: the records whose order among a run of their own kind is
    /// not defined, comma-separated.
    pub unordered: Vec<String>,
    /// `bare`: prims that hold no script, comma-separated as
    /// `<label>@<east>` or `<label>@<east>/<up>` (metres from prim A) — the
    /// targets a sensor or a ray is to find, which need no script of their
    /// own. They are rezzed in the order listed, after the script's own
    /// prim.
    pub bare: Vec<BarePrim>,
    /// `target`: the compile target the script is uploaded for, `mono` (the
    /// default) or `lsl2` ([`Target`]).
    pub target: Target,
    /// `stats: active-scripts`: the runner notes the region's active-script
    /// count (the `SimStats` the grid sends the owner's viewer) each time it
    /// changes while the probe runs.
    pub active_scripts: bool,
    /// `library`: the library the script is written against, `sl` (the
    /// default) or `opensim` ([`Library`]).
    pub library: Library,
}

/// A prim a [`Header`]'s `bare` key names: one that holds no script.
#[derive(Debug, Clone, PartialEq)]
pub struct BarePrim {
    /// Its label (`C`, `D`, …).
    pub label: String,
    /// How far east of prim A it stands, in metres.
    pub distance: f32,
    /// How far above prim A it stands, in metres.
    pub height: f32,
}

impl BarePrim {
    /// Reads one `<label>@<east>[/<up>]` item.
    fn parse(item: &str) -> Option<Self> {
        let (label, place) = item.split_once('@')?;
        let (east, up) = place.split_once('/').unwrap_or((place, "0"));
        let metres = |text: &str| {
            text.parse::<f32>()
                .ok()
                .filter(|metres| metres.is_finite() && *metres >= 0.0)
        };
        (!label.is_empty() && !label.contains([' ', '@', '/'])).then_some(Self {
            label: label.to_owned(),
            distance: metres(east)?,
            height: metres(up)?,
        })
    }
}

/// Why a script's header was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    /// A required key is absent.
    #[error("header `{0}` is missing")]
    Missing(&'static str),
    /// A key's value does not parse.
    #[error("header `{key}` is `{value}`, which does not parse")]
    Invalid {
        /// The key.
        key: String,
        /// Its value.
        value: String,
    },
    /// A key the format does not have.
    #[error("header `{0}` is not a probe header key")]
    Unknown(String),
}

impl Header {
    /// Reads the header of a script's source: the run of `// key: value`
    /// lines it opens with, up to the first line that is not one.
    ///
    /// # Errors
    ///
    /// A [`HeaderError`] when `probe` or `drive` is missing, a value does not
    /// parse, or a key is unknown.
    pub fn parse(source: &str) -> Result<Self, HeaderError> {
        let pairs: BTreeMap<&str, &str> = source
            .lines()
            .map_while(|line| line.strip_prefix("// "))
            .map_while(|line| line.split_once(": "))
            .take_while(|(key, _)| !key.contains(' '))
            .map(|(key, value)| (key, value.trim()))
            .collect();
        for key in pairs.keys() {
            if ![
                "probe",
                "drive",
                "script",
                "prim",
                "order",
                "distance",
                "height",
                "skip-on",
                "vary",
                "unordered",
                "bare",
                "target",
                "stats",
                "library",
            ]
            .contains(key)
            {
                return Err(HeaderError::Unknown((*key).to_owned()));
            }
        }
        let invalid = |key: &str, value: &str| HeaderError::Invalid {
            key: key.to_owned(),
            value: value.to_owned(),
        };
        let probe = (*pairs.get("probe").ok_or(HeaderError::Missing("probe"))?).to_owned();
        let drive = *pairs.get("drive").ok_or(HeaderError::Missing("drive"))?;
        let drives = if drive == "none" {
            Vec::new()
        } else {
            list(drive)
                .iter()
                .map(|name| Drive::parse(name))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid("drive", drive))?
        };
        let order = pairs
            .get("order")
            .map(|value| value.parse().map_err(|_error| invalid("order", value)))
            .transpose()?;
        let metres = |key: &str| {
            pairs
                .get(key)
                .map(|value| {
                    value
                        .parse::<f32>()
                        .ok()
                        .filter(|metres| metres.is_finite() && *metres >= 0.0)
                        .ok_or_else(|| invalid(key, value))
                })
                .transpose()
        };
        let distance = metres("distance")?;
        let height = metres("height")?;
        let skip_on = match pairs.get("skip-on") {
            None => Vec::new(),
            Some(value) => {
                let grids = list(value);
                if !grids.iter().all(|grid| GRIDS.contains(&grid.as_str())) {
                    return Err(invalid("skip-on", value));
                }
                grids
            }
        };
        let bare = match pairs.get("bare") {
            None => Vec::new(),
            Some(value) => list(value)
                .iter()
                .map(|item| BarePrim::parse(item))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid("bare", value))?,
        };
        let target = pairs
            .get("target")
            .map(|value| Target::parse(value).ok_or_else(|| invalid("target", value)))
            .transpose()?
            .unwrap_or_default();
        let active_scripts = match pairs.get("stats") {
            None => false,
            Some(&"active-scripts") => true,
            Some(value) => return Err(invalid("stats", value)),
        };
        let library = pairs
            .get("library")
            .map(|value| Library::parse(value).ok_or_else(|| invalid("library", value)))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            probe,
            drives,
            script: pairs.get("script").map(|name| (*name).to_owned()),
            prim: pairs.get("prim").map_or("A", |prim| *prim).to_owned(),
            order,
            distance,
            height,
            skip_on,
            vary: pairs
                .get("vary")
                .map(|value| list(value))
                .unwrap_or_default(),
            unordered: pairs
                .get("unordered")
                .map(|value| list(value))
                .unwrap_or_default(),
            bare,
            target,
            active_scripts,
            library,
        })
    }

    /// Whether the script is left out on `grid`.
    #[must_use]
    pub fn skipped_on(&self, grid: &str) -> bool {
        self.skip_on.iter().any(|skipped| skipped == grid)
    }
}

/// A header value's comma-separated items, trimmed.
fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_owned())
        .collect()
}

/// One line a probe printed: `PROBE <id> rec=<name> key=value…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The probe's id.
    pub id: String,
    /// The record's name, the `rec` field, which comes first.
    pub rec: String,
    /// The other fields in order, quoted values unescaped.
    pub fields: Vec<(String, String)>,
}

/// Why a line is not a [`Line`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}: {line}")]
pub struct LineError {
    /// What is wrong.
    pub reason: &'static str,
    /// The line.
    pub line: String,
}

/// The prefix every probe line starts with.
pub const PREFIX: &str = "PROBE ";

impl Line {
    /// Parses a line. A value is a bare token (no space, no `"`) or a `"…"`
    /// string with `\\`, `\"` and `\n` escapes — what the corpus's `q()`
    /// helper prints.
    ///
    /// # Errors
    ///
    /// A [`LineError`] when the line is not a probe line, `rec` is not its
    /// first field, or a field does not parse.
    pub fn parse(line: &str) -> Result<Self, LineError> {
        let refuse = |reason| LineError {
            reason,
            line: line.to_owned(),
        };
        let rest = line
            .strip_prefix(PREFIX)
            .ok_or_else(|| refuse("not a PROBE line"))?;
        let (id, mut rest) = rest.split_once(' ').ok_or_else(|| refuse("no fields"))?;
        let mut fields = Vec::new();
        while !rest.is_empty() {
            let (key, after) = rest
                .split_once('=')
                .ok_or_else(|| refuse("a field without `=`"))?;
            if key.is_empty() || key.contains([' ', '"']) {
                return Err(refuse("a bad key"));
            }
            let (value, after) = if let Some(quoted) = after.strip_prefix('"') {
                unquote(quoted).ok_or_else(|| refuse("a bad quoted value"))?
            } else {
                let (value, after) = after.split_at(after.find(' ').unwrap_or(after.len()));
                if value.contains('"') {
                    return Err(refuse("a quote inside a bare value"));
                }
                (value.to_owned(), after)
            };
            rest = match after.strip_prefix(' ') {
                Some(next) => next,
                None if after.is_empty() => after,
                None => return Err(refuse("no space after a value")),
            };
            fields.push((key.to_owned(), value));
        }
        let mut fields = fields.into_iter();
        let rec = match fields.next() {
            Some((key, rec)) if key == "rec" => rec,
            _ => return Err(refuse("the first field is not `rec`")),
        };
        Ok(Self {
            id: id.to_owned(),
            rec,
            fields: fields.collect(),
        })
    }

    /// The value of the field `key`, if the line has it.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// Which avatar gives a [`Stimulus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// The probe's owner, the runner's own avatar.
    Owner,
    /// A second avatar, which owns nothing of the probe's.
    Other,
}

/// Where on prim A a touch says it landed: the surface block a viewer sends
/// with a grab (`ObjectGrab`'s and `ObjectDeGrab`'s `SurfaceInfo`), which a
/// script reads with `llDetectedTouch*`.
#[derive(Debug, Clone, PartialEq)]
pub struct TouchSurface {
    /// The face (`llDetectedTouchFace`).
    pub face: i32,
    /// The texture coordinate (`llDetectedTouchUV`).
    pub uv: [f32; 2],
    /// The surface coordinate (`llDetectedTouchST`).
    pub st: [f32; 2],
    /// The point touched, in metres from prim A's centre along the region's
    /// axes (the runner adds prim A's position: `llDetectedTouchPos` is
    /// region-local).
    pub at: [f32; 3],
    /// The surface normal (`llDetectedTouchNormal`).
    pub normal: [f32; 3],
    /// The surface binormal (`llDetectedTouchBinormal`).
    pub binormal: [f32; 3],
}

impl TouchSurface {
    /// Reads a touch's surface from a `waiting` record's `face`, `uv`, `st`,
    /// `at`, `normal` and `binormal` fields: `None` when it has none of them.
    ///
    /// # Errors
    ///
    /// The field that is missing, given some of them, or does not parse:
    /// `face` an integer, the rest comma-separated numbers, two or three.
    fn from_waiting(line: &Line) -> Result<Option<Self>, String> {
        const KEYS: [&str; 6] = ["face", "uv", "st", "at", "normal", "binormal"];
        if KEYS.iter().all(|key| line.field(key).is_none()) {
            return Ok(None);
        }
        let field = |key: &str| {
            line.field(key)
                .ok_or_else(|| format!("a touch with a surface without `{key}`"))
        };
        let numbers = |key: &str| -> Result<Vec<f32>, String> {
            field(key)?
                .split(',')
                .map(|number| number.trim().parse::<f32>().ok().filter(|n| n.is_finite()))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| format!("a touch's `{key}` is not comma-separated numbers"))
        };
        let pair = |key: &str| -> Result<[f32; 2], String> {
            <[f32; 2]>::try_from(numbers(key)?)
                .map_err(|_numbers| format!("a touch's `{key}` is not two numbers"))
        };
        let triple = |key: &str| -> Result<[f32; 3], String> {
            <[f32; 3]>::try_from(numbers(key)?)
                .map_err(|_numbers| format!("a touch's `{key}` is not three numbers"))
        };
        Ok(Some(Self {
            face: field("face")?
                .parse()
                .map_err(|_error| "a touch's `face` is not an integer".to_owned())?,
            uv: pair("uv")?,
            st: pair("st")?,
            at: triple("at")?,
            normal: triple("normal")?,
            binormal: triple("binormal")?,
        }))
    }
}

/// What a stimulus is.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Touch prim A — with no surface, as a viewer that picked none (or an
    /// old one) sends it, or with the one given.
    Touch(Option<TouchSurface>),
    /// Say `text` on `channel`, at normal range.
    Chat {
        /// The channel.
        channel: i32,
        /// What is said.
        text: String,
    },
    /// Answer the permission question a probe's script puts to the avatar
    /// with the mask `grant`, as it is — `0` denies, and a mask beyond what
    /// was asked is sent too.
    Answer {
        /// The permissions granted.
        grant: i32,
    },
    /// Sit on prim A.
    Sit,
    /// Stand up.
    Stand,
    /// Hold the movement controls `keys` (the `CONTROL_*` bits, which are
    /// the agent's own control flags) for a moment, then let go.
    Control {
        /// The controls held.
        keys: u32,
    },
    /// Reply `reply` to the dialog a probe's script puts to the avatar: the
    /// label of the button pressed, which need not be one of the dialog's,
    /// or the text typed into an `llTextBox`.
    Dialog {
        /// The label or text sent back.
        reply: String,
    },
    /// Leave the probe's region for `seconds`: the owner teleports to
    /// another, stays, and comes back to where it stood.
    Away {
        /// How long the owner stays away.
        seconds: u32,
    },
    /// Attach prim A, from the ground, to the owner at `point` (an
    /// `ATTACH_*` constant).
    Attach {
        /// The attachment point.
        point: u8,
    },
    /// Drop prim A, worn, to the ground.
    Drop,
    /// Take prim A into the owner's inventory and rez it back where it
    /// stood.
    Rerez,
    /// Send an HTTP request from outside the grid to `url` followed by
    /// `path`.
    Fetch(Fetch),
    /// Pay the prim labelled `prim` `amount` L$, as a viewer's pay dialog
    /// does (a `MoneyTransferRequest` of the object-payment type), the amount
    /// sent as it is.
    Pay {
        /// The amount, which may be one no pay dialog would offer (none, or
        /// more than the payer holds).
        amount: u64,
        /// The prim paid (`A` unless the record names another).
        prim: String,
    },
    /// Ask the prim labelled `prim` for its pay buttons, as a viewer does
    /// before it shows its pay dialog (`RequestPayPrice`).
    PayPrice {
        /// The prim asked (`A` unless the record names another).
        prim: String,
    },
    /// Revoke the permissions `permissions` (the `PERMISSION_*` bits, sent as
    /// they are) of prim A's scripts, or of every object in the region — the
    /// two `RevokePermissions` a viewer sends.
    Revoke {
        /// The permissions revoked.
        permissions: u32,
        /// Whether the revocation names the region (every object in it)
        /// rather than prim A.
        whole_region: bool,
    },
}

/// The HTTP request a `fetch` stimulus sends: `url=<url> [method=<name>]
/// [path="…"] [body="…"] [repeat=<n>] [type="…"] [header="<name>: <value>"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetch {
    /// The URL the probe's script was given, as it printed it.
    pub url: String,
    /// The method, as it is sent (`GET` when the record names none).
    pub method: String,
    /// What follows the URL: a path and a query, as they are sent.
    pub path: String,
    /// The body: the record's `body` repeated `repeat` times (once when it
    /// names no `repeat`), which lets a record of a few characters send a
    /// body longer than a line of chat.
    pub body: String,
    /// The body's `Content-Type`, if the record names one.
    pub content_type: Option<String>,
    /// One more header, as a name and a value, if the record names one.
    pub header: Option<(String, String)>,
}

impl Fetch {
    /// The most a `repeat` may ask for: a body of at most this many copies.
    pub const MAX_REPEAT: usize = 50_000;

    /// Reads a `fetch` record's fields.
    ///
    /// # Errors
    ///
    /// What is wrong: no `url`, a `repeat` that is not a count up to
    /// [`Self::MAX_REPEAT`], a `header` that is not `<name>: <value>`, or a
    /// method or header name that is not a token.
    fn from_waiting(line: &Line) -> Result<Self, String> {
        let url = line.field("url").ok_or("a fetch drive without `url`")?;
        let method = line.field("method").unwrap_or("GET");
        let is_token = |text: &str| {
            !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
        };
        if !is_token(method) {
            return Err(format!("a fetch drive's `method={method}` is not a token"));
        }
        let repeat = line
            .field("repeat")
            .map(|repeat| {
                repeat
                    .parse::<usize>()
                    .ok()
                    .filter(|repeat| *repeat <= Self::MAX_REPEAT)
                    .ok_or_else(|| format!("a fetch drive's `repeat={repeat}` is not a count"))
            })
            .transpose()?
            .unwrap_or(1);
        let header = line
            .field("header")
            .map(|header| {
                header
                    .split_once(": ")
                    .filter(|(name, _)| is_token(name))
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
                    .ok_or_else(|| {
                        format!("a fetch drive's `header` \"{header}\" is not `<name>: <value>`")
                    })
            })
            .transpose()?;
        Ok(Self {
            url: url.to_owned(),
            method: method.to_owned(),
            path: line.field("path").unwrap_or_default().to_owned(),
            body: line.field("body").unwrap_or_default().repeat(repeat),
            content_type: line.field("type").map(str::to_owned),
            header,
        })
    }
}

impl Action {
    /// Its [`Drive`].
    #[must_use]
    pub const fn drive(&self) -> Drive {
        match self {
            Self::Touch(_) => Drive::Touch,
            Self::Chat { .. } => Drive::Chat,
            Self::Answer { .. } => Drive::Answer,
            Self::Sit => Drive::Sit,
            Self::Stand => Drive::Stand,
            Self::Control { .. } => Drive::Control,
            Self::Dialog { .. } => Drive::Dialog,
            Self::Away { .. } => Drive::Away,
            Self::Attach { .. } => Drive::Attach,
            Self::Drop => Drive::Drop,
            Self::Rerez => Drive::Rerez,
            Self::Fetch(_) => Drive::Fetch,
            Self::Pay { .. } => Drive::Pay,
            Self::PayPrice { .. } => Drive::PayPrice,
            Self::Revoke { .. } => Drive::Revoke,
        }
    }
}

/// What a `waiting` record asks the runner for:
/// `rec=waiting
/// drive=touch|chat|answer|sit|stand|control|dialog|away|attach|drop|rerez|fetch|pay|pay-price|revoke
/// [by=owner|other]`, and for a touch optionally `face=<n> uv=<u>,<v>
/// st=<s>,<t> at=<x>,<y>,<z> normal=<x>,<y>,<z> binormal=<x>,<y>,<z>` (all of
/// them, or none), for chat `channel=<n> text="…"`, for an answer
/// `grant=<mask>`, for controls `keys=<mask>`, for a dialog `reply="…"`, for
/// away `seconds=<n>`, for attach `point=<n>`, for a fetch what [`Fetch`]
/// reads, for a payment `amount=<n> [prim=<label>]`, for the pay buttons
/// `[prim=<label>]`, for a revocation `perms=<mask> [target=object|region]`. Without `by` it is the owner; only the owner goes `away`, attaches,
/// drops or re-rezzes, and a fetch comes from no avatar at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Stimulus {
    /// Who gives it.
    pub by: By,
    /// What it is.
    pub action: Action,
}

impl Stimulus {
    /// Reads a `waiting` record.
    ///
    /// # Errors
    ///
    /// What is wrong with it: not a `waiting` record, an unknown drive or
    /// avatar, a touch with part of a surface, a chat without its channel or
    /// text, a dialog without its
    /// reply, an away without its seconds, an attach without its point, a
    /// fetch [`Fetch`] refuses, the other avatar sent away, attaching,
    /// dropping or re-rezzing, or a fetch given `by` at all.
    pub fn from_waiting(line: &Line) -> Result<Self, String> {
        if line.rec != "waiting" {
            return Err(format!("`{}` is not a `waiting` record", line.rec));
        }
        let by = match line.field("by") {
            None | Some("owner") => By::Owner,
            Some("other") => By::Other,
            Some(other) => return Err(format!("`by={other}` is neither owner nor other")),
        };
        let drive = line
            .field("drive")
            .ok_or("a `waiting` record without `drive`")?;
        let action = match Drive::parse(drive) {
            Some(Drive::Touch) => Action::Touch(TouchSurface::from_waiting(line)?),
            Some(Drive::Chat) => Action::Chat {
                channel: line
                    .field("channel")
                    .and_then(|channel| channel.parse().ok())
                    .ok_or("a chat drive without an integer `channel`")?,
                text: line
                    .field("text")
                    .ok_or("a chat drive without `text`")?
                    .to_owned(),
            },
            Some(Drive::Answer) => Action::Answer {
                grant: line
                    .field("grant")
                    .and_then(|grant| grant.parse().ok())
                    .ok_or("an answer drive without an integer `grant`")?,
            },
            Some(Drive::Sit) => Action::Sit,
            Some(Drive::Stand) => Action::Stand,
            Some(Drive::Control) => Action::Control {
                keys: line
                    .field("keys")
                    .and_then(|keys| keys.parse().ok())
                    .ok_or("a control drive without an unsigned integer `keys`")?,
            },
            Some(Drive::Dialog) => Action::Dialog {
                reply: line
                    .field("reply")
                    .ok_or("a dialog drive without `reply`")?
                    .to_owned(),
            },
            Some(Drive::Away) => Action::Away {
                seconds: line
                    .field("seconds")
                    .and_then(|seconds| seconds.parse().ok())
                    .ok_or("an away drive without an unsigned integer `seconds`")?,
            },
            Some(Drive::Attach) => Action::Attach {
                point: line
                    .field("point")
                    .and_then(|point| point.parse().ok())
                    .ok_or("an attach drive without an attachment point `point`")?,
            },
            Some(Drive::Drop) => Action::Drop,
            Some(Drive::Rerez) => Action::Rerez,
            Some(Drive::Fetch) => {
                if line.field("by").is_some() {
                    return Err("a fetch comes from no avatar, so it takes no `by`".to_owned());
                }
                Action::Fetch(Fetch::from_waiting(line)?)
            }
            Some(Drive::Pay) => Action::Pay {
                amount: line
                    .field("amount")
                    .and_then(|amount| amount.parse().ok())
                    .ok_or("a pay drive without an unsigned integer `amount`")?,
                prim: line.field("prim").unwrap_or("A").to_owned(),
            },
            Some(Drive::PayPrice) => Action::PayPrice {
                prim: line.field("prim").unwrap_or("A").to_owned(),
            },
            Some(Drive::Revoke) => Action::Revoke {
                permissions: line
                    .field("perms")
                    .and_then(|perms| perms.parse().ok())
                    .ok_or("a revoke drive without an unsigned integer `perms`")?,
                whole_region: match line.field("target") {
                    None | Some("object") => false,
                    Some("region") => true,
                    Some(target) => {
                        return Err(format!("`target={target}` is neither object nor region"));
                    }
                },
            },
            None => return Err(format!("`drive={drive}` is not a drive")),
        };
        if by == By::Other
            && matches!(
                action,
                Action::Away { .. } | Action::Attach { .. } | Action::Drop | Action::Rerez
            )
        {
            return Err(format!("`drive={drive}` is the owner's alone"));
        }
        Ok(Self { by, action })
    }
}

/// Reads a quoted value from just after its opening quote: the value and
/// what follows its closing quote, or `None` for a bad escape or no closing
/// quote.
fn unquote(quoted: &str) -> Option<(String, &str)> {
    let mut value = String::new();
    let mut chars = quoted.char_indices();
    loop {
        match chars.next()? {
            (at, '"') => return Some((value, quoted.get(at.saturating_add(1)..)?)),
            (_, '\\') => match chars.next()? {
                (_, '\\') => value.push('\\'),
                (_, '"') => value.push('"'),
                (_, 'n') => value.push('\n'),
                _ => return None,
            },
            (_, other) => value.push(other),
        }
    }
}

/// The grids a result may be for: the live grids, then the fake grid's
/// flavours.
pub const GRIDS: &[&str] = &[
    "aditi",
    "opensim-yengine",
    "opensim-xengine",
    "fake-sl",
    "fake-opensim",
];

/// The header keys every result file carries, in the order they are
/// written.
pub const RESULT_KEYS: &[&str] = &["grid", "date", "runner", "region", "engine", "settings"];

/// How a result file's note for a compile error the grid reported starts:
/// the runner writes one per error of a script that did not compile.
pub const COMPILE_ERROR_NOTE: &str = "# compile error in ";

/// One run's transcript: `probes/results/<grid>/<probe id>.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultFile {
    /// The header, in file order: the [`RESULT_KEYS`], and `complete: no`
    /// for a run that never printed its `end`.
    pub header: Vec<(String, String)>,
    /// The body: `PROBE` lines and `#` notes, in the order they came.
    pub body: Vec<String>,
}

impl ResultFile {
    /// Splits a result file's text into its header and body.
    ///
    /// # Errors
    ///
    /// The header line that is not `key: value`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let mut header = Vec::new();
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            match line.split_once(": ") {
                Some((key, value)) if !key.contains(' ') && !value.trim().is_empty() => {
                    header.push((key.to_owned(), value.trim().to_owned()));
                }
                _ => return Err(format!("a bad header line `{line}`")),
            }
        }
        let body = lines
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(Self { header, body })
    }

    /// The header value of `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.header
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Whether the run printed its `end`, as the header says.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.get("complete") != Some("no")
    }

    /// The file's text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        for (key, value) in &self.header {
            text.push_str(key);
            text.push_str(": ");
            text.push_str(value);
            text.push('\n');
        }
        text.push('\n');
        for line in &self.body {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    /// What is wrong with this file as the run of probe `id` on `grid`: the
    /// header keys, the date, and the transcript — every line a probe line
    /// of `id` or a `#` note, exactly one `start` and, unless the header
    /// says `complete: no`, exactly one `end`, last. A run the grid would
    /// not compile holds no record at all: `complete: no`, and a compile
    /// error note in place of the `start`.
    #[must_use]
    pub fn problems(&self, grid: &str, id: &str) -> Vec<String> {
        let mut problems = Vec::new();
        for key in RESULT_KEYS {
            if self.get(key).is_none() {
                problems.push(format!("header `{key}` is missing"));
            }
        }
        if self.get("grid").is_some_and(|value| value != grid) {
            problems.push(format!("header `grid` is not `{grid}`"));
        }
        if self.get("date").is_some_and(|date| !is_date(date)) {
            problems.push("header `date` is not YYYY-MM-DD".to_owned());
        }
        if !matches!(self.get("complete"), None | Some("yes" | "no")) {
            problems.push("header `complete` is neither yes nor no".to_owned());
        }
        let mut records = Vec::new();
        for line in &self.body {
            if line.starts_with('#') {
                continue;
            }
            match Line::parse(line) {
                Ok(parsed) if parsed.id != id => {
                    problems.push(format!("a line for `{}`", parsed.id));
                }
                Ok(parsed) => records.push(parsed.rec),
                Err(error) => problems.push(error.to_string()),
            }
        }
        let not_compiled = !self.complete()
            && records.is_empty()
            && self
                .body
                .iter()
                .any(|line| line.starts_with(COMPILE_ERROR_NOTE));
        if !not_compiled && records.iter().filter(|rec| *rec == "start").count() != 1 {
            problems.push("the run does not hold one `start`".to_owned());
        }
        let ends = records.iter().filter(|rec| *rec == "end").count();
        if self.complete() {
            if ends != 1 || records.last().map(String::as_str) != Some("end") {
                problems.push("the run does not end with one `end`".to_owned());
            }
        } else if ends != 0 {
            problems.push("`complete: no`, yet the run printed `end`".to_owned());
        }
        problems
    }
}

/// Whether `text` reads as a `YYYY-MM-DD` date.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.iter().map(|part| part.len()).eq([4, 2, 2])
        && parts
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        Action, BarePrim, By, Drive, Fetch, Header, HeaderError, Library, Line, ResultFile,
        Stimulus, Target, TouchSurface,
    };

    #[test]
    fn a_header_reads_up_to_the_first_other_line() -> Result<(), HeaderError> {
        let header = Header::parse(
            "// probe: errors/runtime-errors\n// script: Div int\n// prim: B\n\
             // order: 3\n// distance: 50\n// height: 99\n// drive: none\n\
             // skip-on: opensim-xengine, fake-sl\n// target: lsl2\n// stats: active-scripts\n\
             // library: opensim\n//\n// probe: not/this\n",
        )?;
        assert_eq!(
            header,
            Header {
                probe: "errors/runtime-errors".to_owned(),
                drives: Vec::new(),
                script: Some("Div int".to_owned()),
                prim: "B".to_owned(),
                order: Some(3),
                distance: Some(50.0),
                height: Some(99.0),
                skip_on: vec!["opensim-xengine".to_owned(), "fake-sl".to_owned()],
                vary: Vec::new(),
                unordered: Vec::new(),
                bare: Vec::new(),
                target: Target::Lsl2,
                active_scripts: true,
                library: Library::OpenSim,
            }
        );
        assert!(header.skipped_on("fake-sl"));
        assert!(!header.skipped_on("aditi"));
        Ok(())
    }

    #[test]
    fn a_header_defaults_to_the_second_life_library_and_refuses_another() {
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n").map(|header| header.library),
            Ok(Library::SecondLife)
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// library: xengine\n"),
            Err(HeaderError::Invalid {
                key: "library".to_owned(),
                value: "xengine".to_owned(),
            })
        );
    }

    #[test]
    fn opensims_library_adds_only_its_own_families() {
        for name in [
            "osGetScriptEngineName",
            "lsGetWindlightScene",
            "modSendCommand",
            "OS_LISTEN_REGEX_MESSAGE",
            "OSTPOBJ_STOPATTARGET",
            "NPCLOOKAT_FOCUS",
            "WL_WATER_COLOR",
        ] {
            assert!(Library::OpenSim.adds(name), "{name}");
            assert!(!Library::SecondLife.adds(name), "{name}");
        }
        for name in ["os", "osmium", "llOwnerSay", "lsx", "model", "nope"] {
            assert!(!Library::OpenSim.adds(name), "{name}");
        }
    }

    #[test]
    fn a_header_lists_its_drives_and_its_tolerances() -> Result<(), HeaderError> {
        let header = Header::parse(
            "// probe: a/b\n// drive: chat, touch, answer, sit, stand, control, dialog, away, attach, \
             drop, rerez, fetch, pay, pay-price, revoke\n\
             // vary: seconds, rate\n// unordered: heard\n",
        )?;
        assert_eq!(
            header.drives,
            vec![
                Drive::Chat,
                Drive::Touch,
                Drive::Answer,
                Drive::Sit,
                Drive::Stand,
                Drive::Control,
                Drive::Dialog,
                Drive::Away,
                Drive::Attach,
                Drive::Drop,
                Drive::Rerez,
                Drive::Fetch,
                Drive::Pay,
                Drive::PayPrice,
                Drive::Revoke
            ]
        );
        assert_eq!(header.vary, vec!["seconds".to_owned(), "rate".to_owned()]);
        assert_eq!(header.unordered, vec!["heard".to_owned()]);
        Ok(())
    }

    #[test]
    fn a_waiting_record_names_its_stimulus() -> Result<(), String> {
        let read = |line: &str| {
            Line::parse(line)
                .map_err(|error| error.to_string())
                .and_then(|line| Stimulus::from_waiting(&line))
        };
        assert_eq!(
            read("PROBE a/b rec=waiting drive=touch")?,
            Stimulus {
                by: By::Owner,
                action: Action::Touch(None)
            }
        );
        assert_eq!(
            read(
                "PROBE a/b rec=waiting drive=touch face=2 uv=0.25,0.75 st=0.3,-1 \
                 at=0.25,0,0.1 normal=1,0,0 binormal=0,0,1"
            )?
            .action,
            Action::Touch(Some(TouchSurface {
                face: 2,
                uv: [0.25, 0.75],
                st: [0.3, -1.0],
                at: [0.25, 0.0, 0.1],
                normal: [1.0, 0.0, 0.0],
                binormal: [0.0, 0.0, 1.0],
            }))
        );
        assert_eq!(
            read(r#"PROBE a/b rec=waiting drive=chat by=other channel=-7 text="hi there""#)?,
            Stimulus {
                by: By::Other,
                action: Action::Chat {
                    channel: -7,
                    text: "hi there".to_owned()
                }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=answer by=other grant=3092")?,
            Stimulus {
                by: By::Other,
                action: Action::Answer { grant: 3092 }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=control keys=257")?,
            Stimulus {
                by: By::Owner,
                action: Action::Control { keys: 257 }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=sit by=other")?.action,
            Action::Sit
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=stand")?.action,
            Action::Stand
        );
        assert_eq!(
            read(r#"PROBE a/b rec=waiting drive=dialog by=other reply="Not a button""#)?,
            Stimulus {
                by: By::Other,
                action: Action::Dialog {
                    reply: "Not a button".to_owned()
                }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=away by=owner seconds=20")?.action,
            Action::Away { seconds: 20 }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=attach point=5")?.action,
            Action::Attach { point: 5 }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=drop")?.action,
            Action::Drop
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=rerez")?.action,
            Action::Rerez
        );
        assert_eq!(
            read(
                r#"PROBE a/b rec=waiting drive=fetch url=http://h:1/cap/x method=POST path="/p?q=1" body=ab repeat=3 type="text/plain" header="X-Probe: one two""#
            )?,
            Stimulus {
                by: By::Owner,
                action: Action::Fetch(Fetch {
                    url: "http://h:1/cap/x".to_owned(),
                    method: "POST".to_owned(),
                    path: "/p?q=1".to_owned(),
                    body: "ababab".to_owned(),
                    content_type: Some("text/plain".to_owned()),
                    header: Some(("X-Probe".to_owned(), "one two".to_owned())),
                })
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=pay by=other amount=3 prim=B")?,
            Stimulus {
                by: By::Other,
                action: Action::Pay {
                    amount: 3,
                    prim: "B".to_owned()
                }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=pay-price")?.action,
            Action::PayPrice {
                prim: "A".to_owned()
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=revoke perms=4294967295")?.action,
            Action::Revoke {
                permissions: u32::MAX,
                whole_region: false
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=revoke perms=32784 target=region")?.action,
            Action::Revoke {
                permissions: 32784,
                whole_region: true
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=fetch url=http://h:1/cap/x")?.action,
            Action::Fetch(Fetch {
                url: "http://h:1/cap/x".to_owned(),
                method: "GET".to_owned(),
                path: String::new(),
                body: String::new(),
                content_type: None,
                header: None,
            })
        );
        for refused in [
            "PROBE a/b rec=waiting drive=fetch",
            "PROBE a/b rec=waiting drive=fetch url=u by=owner",
            "PROBE a/b rec=waiting drive=fetch url=u method=\"GE T\"",
            "PROBE a/b rec=waiting drive=fetch url=u repeat=-1",
            "PROBE a/b rec=waiting drive=fetch url=u repeat=50001",
            "PROBE a/b rec=waiting drive=fetch url=u header=\"no colon\"",
            "PROBE a/b rec=waiting drive=answer",
            "PROBE a/b rec=waiting drive=answer grant=all",
            "PROBE a/b rec=waiting drive=control keys=-1",
            "PROBE a/b rec=start",
            "PROBE a/b rec=waiting",
            "PROBE a/b rec=waiting drive=wave",
            "PROBE a/b rec=waiting drive=touch by=everyone",
            "PROBE a/b rec=waiting drive=chat text=hi",
            "PROBE a/b rec=waiting drive=chat channel=7",
            "PROBE a/b rec=waiting drive=dialog",
            "PROBE a/b rec=waiting drive=away",
            "PROBE a/b rec=waiting drive=away by=other seconds=20",
            "PROBE a/b rec=waiting drive=attach",
            "PROBE a/b rec=waiting drive=attach point=300",
            "PROBE a/b rec=waiting drive=attach by=other point=5",
            "PROBE a/b rec=waiting drive=drop by=other",
            "PROBE a/b rec=waiting drive=rerez by=other",
            "PROBE a/b rec=waiting drive=pay",
            "PROBE a/b rec=waiting drive=pay amount=-1",
            "PROBE a/b rec=waiting drive=revoke",
            "PROBE a/b rec=waiting drive=revoke perms=16 target=parcel",
            "PROBE a/b rec=waiting drive=touch face=1",
            "PROBE a/b rec=waiting drive=touch face=x uv=0,0 st=0,0 at=0,0,0 normal=0,0,1 \
             binormal=0,1,0",
            "PROBE a/b rec=waiting drive=touch face=1 uv=0 st=0,0 at=0,0,0 normal=0,0,1 \
             binormal=0,1,0",
        ] {
            assert!(read(refused).is_err(), "{refused}");
        }
        Ok(())
    }

    #[test]
    fn a_header_names_its_bare_prims() -> Result<(), HeaderError> {
        let header =
            Header::parse("// probe: a/b\n// drive: none\n// bare: C@22, D@3.5/2, E@0/100\n")?;
        assert_eq!(
            header.bare,
            vec![
                BarePrim {
                    label: "C".to_owned(),
                    distance: 22.0,
                    height: 0.0
                },
                BarePrim {
                    label: "D".to_owned(),
                    distance: 3.5,
                    height: 2.0
                },
                BarePrim {
                    label: "E".to_owned(),
                    distance: 0.0,
                    height: 100.0
                },
            ]
        );
        for refused in ["C", "C@", "@3", "C@-1", "C@3/x"] {
            assert_eq!(
                Header::parse(&format!(
                    "// probe: a/b\n// drive: none\n// bare: {refused}\n"
                )),
                Err(HeaderError::Invalid {
                    key: "bare".to_owned(),
                    value: refused.to_owned()
                }),
                "{refused}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_header_without_its_drive_or_with_a_stray_key_is_refused() {
        assert_eq!(
            Header::parse("// probe: a/b\n"),
            Err(HeaderError::Missing("drive"))
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: sometimes\n"),
            Err(HeaderError::Invalid {
                key: "drive".to_owned(),
                value: "sometimes".to_owned()
            })
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// skip-on: mainland\n"),
            Err(HeaderError::Invalid {
                key: "skip-on".to_owned(),
                value: "mainland".to_owned()
            })
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// target: luau\n"),
            Err(HeaderError::Invalid {
                key: "target".to_owned(),
                value: "luau".to_owned()
            })
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// stats: lag\n"),
            Err(HeaderError::Invalid {
                key: "stats".to_owned(),
                value: "lag".to_owned()
            })
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// colour: red\n"),
            Err(HeaderError::Unknown("colour".to_owned()))
        );
    }

    #[test]
    fn a_probe_line_parses_bare_and_quoted_values() -> Result<(), super::LineError> {
        let parsed = Line::parse(
            r#"PROBE errors/runtime-errors rec=heard name="Object" dist=49.5 msg="a \"b\"\nc\\" len=7"#,
        )?;
        assert_eq!(
            parsed,
            Line {
                id: "errors/runtime-errors".to_owned(),
                rec: "heard".to_owned(),
                fields: vec![
                    ("name".to_owned(), "Object".to_owned()),
                    ("dist".to_owned(), "49.5".to_owned()),
                    ("msg".to_owned(), "a \"b\"\nc\\".to_owned()),
                    ("len".to_owned(), "7".to_owned()),
                ],
            }
        );
        assert_eq!(parsed.field("dist"), Some("49.5"));
        Ok(())
    }

    #[test]
    fn a_probe_line_without_rec_first_or_with_a_stray_quote_is_refused() {
        for line in [
            "PROBE a/b at=1 rec=start",
            "PROBE a/b rec=start x=\"open",
            "PROBE a/b rec=start x=a\"b",
            "PROBE a/b rec=start x=\"a\"b",
            "PROBE a/b rec=start x=\"bad \\t escape\"",
            "PROBE a/b",
            "chat PROBE a/b rec=start",
        ] {
            assert!(Line::parse(line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_result_file_round_trips_and_holds_its_run_to_the_format() -> Result<(), String> {
        let text = "grid: aditi\ndate: 2026-10-23\nrunner: sl-conformance\n\
                    region: Somewhere\nengine: Mono\nsettings: Linden Lab's\n\n\
                    PROBE a/b rec=start\n# a note\nPROBE a/b rec=end\n";
        let file = ResultFile::parse(text)?;
        assert_eq!(file.render(), text);
        assert_eq!(file.problems("aditi", "a/b"), Vec::<String>::new());
        assert_eq!(
            file.problems("opensim-yengine", "a/c"),
            vec![
                "header `grid` is not `opensim-yengine`".to_owned(),
                "a line for `a/b`".to_owned(),
                "a line for `a/b`".to_owned(),
                "the run does not hold one `start`".to_owned(),
                "the run does not end with one `end`".to_owned(),
            ]
        );
        Ok(())
    }

    #[test]
    fn an_incomplete_run_says_so_and_prints_no_end() -> Result<(), String> {
        let file = ResultFile::parse(
            "grid: aditi\ndate: 2026-10-23\nrunner: r\nregion: r\nengine: Mono\n\
             settings: s\ncomplete: no\n\nPROBE a/b rec=start\n",
        )?;
        assert!(!file.complete());
        assert_eq!(file.problems("aditi", "a/b"), Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_run_that_never_compiled_needs_no_start_but_only_when_incomplete() -> Result<(), String> {
        let head = "grid: opensim-xengine\ndate: 2026-10-25\nrunner: r\nregion: r\n\
                    engine: XEngine\nsettings: s\n";
        let note = "\n# compile error in \"Probe\": \"(272,4) syntax error\"\n";
        let incomplete = ResultFile::parse(&format!("{head}complete: no\n{note}"))?;
        assert_eq!(
            incomplete.problems("opensim-xengine", "a/b"),
            Vec::<String>::new()
        );
        let complete = ResultFile::parse(&format!("{head}{note}"))?;
        assert_eq!(
            complete.problems("opensim-xengine", "a/b"),
            vec![
                "the run does not hold one `start`".to_owned(),
                "the run does not end with one `end`".to_owned(),
            ]
        );
        let silent = ResultFile::parse(&format!("{head}complete: no\n\n# a note\n"))?;
        assert_eq!(
            silent.problems("opensim-xengine", "a/b"),
            vec!["the run does not hold one `start`".to_owned()]
        );
        Ok(())
    }
}
