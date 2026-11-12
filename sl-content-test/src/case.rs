//! A case: the objects to rez and what each holds, the avatars to bring and
//! what they do, and what should then have happened — read from a TOML file a
//! content creator writes, with every script, notecard and asset it names read
//! from files beside it.
//!
//! [`Document`] is the file as written; [`Case::load`] checks it and reads what
//! it names into a [`Case`], which is what [`crate::run()`] plays. The format is
//! documented field by field in the book chapter `tools/content-test.md`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sl_fake_grid::{
    AnswerPolicy, AvatarAction, DialogAnswer, DialogChoice, ImitatedGrid, PaymentAnswer,
    PermissionAnswer, Target,
};
use sl_proto::ClickAction;
use sl_types::lsl::{ScriptPermissions, Vector};

/// Why a case could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// A file could not be read.
    #[error("{path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The case file is not valid TOML of the case format.
    #[error("{path}: {source}")]
    Parse {
        /// The case file.
        path: PathBuf,
        /// What the parser said.
        source: Box<toml::de::Error>,
    },
    /// The case file is TOML of the right shape but says something that
    /// cannot be played.
    #[error("{path}: {what}")]
    Invalid {
        /// The case file.
        path: PathBuf,
        /// What is wrong, naming the object, avatar step or expectation.
        what: String,
    },
}

/// The default size of a prim, in metres, on each axis.
const DEFAULT_EDGE_M: f32 = 0.5;

/// A case file as written. Every table and field is described in the book
/// chapter `tools/content-test.md`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// What the report calls the case; the case file's directory or stem
    /// when absent.
    pub name: Option<String>,
    /// The grids the case is played on, each in its own run.
    #[serde(default = "default_grids")]
    pub grids: Vec<GridName>,
    /// The seed every id, `llFrand` and timer of the run is drawn from.
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// How long one tick of the region is, in milliseconds of the region's
    /// own clock.
    #[serde(default = "default_step_ms")]
    pub step_ms: u64,
    /// The most ticks the case may run before it is failed for not
    /// finishing.
    #[serde(default = "default_max_ticks")]
    pub max_ticks: u32,
    /// How many ticks the region runs on after every avatar has finished its
    /// steps, for what those steps set going.
    #[serde(default = "default_settle_ticks")]
    pub settle_ticks: u32,
    /// The objects rezzed when the region starts.
    #[serde(default, rename = "object")]
    pub objects: Vec<ObjectSpec>,
    /// Assets the region serves that no object holds, by key: what a script
    /// names by uuid.
    #[serde(default, rename = "asset")]
    pub assets: Vec<AssetSpec>,
    /// The avatars the grid logs in and drives.
    #[serde(default, rename = "avatar")]
    pub avatars: Vec<AvatarSpec>,
    /// What should have happened.
    #[serde(default, rename = "expect")]
    pub expectations: Vec<ExpectSpec>,
}

/// The grids a case plays on when it names none.
fn default_grids() -> Vec<GridName> {
    vec![GridName::SecondLife]
}

/// The seed a case runs from when it names none.
const fn default_seed() -> u64 {
    1
}

/// A tick's length when a case names none: the fake grid's own default step.
const fn default_step_ms() -> u64 {
    100
}

/// The tick budget when a case names none: a minute of region time.
const fn default_max_ticks() -> u32 {
    600
}

/// The settling ticks when a case names none: two seconds of region time.
const fn default_settle_ticks() -> u32 {
    20
}

/// A grid a case plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GridName {
    /// The fake grid imitating Second Life.
    SecondLife,
    /// The fake grid imitating OpenSim.
    Opensim,
}

impl GridName {
    /// The fake grid's flavour.
    #[must_use]
    pub const fn imitated(self) -> ImitatedGrid {
        match self {
            Self::SecondLife => ImitatedGrid::SecondLife,
            Self::Opensim => ImitatedGrid::OpenSim,
        }
    }
}

/// A position, size or offset as written: `[x, y, z]` in metres.
type Triple = [f32; 3];

/// The default prim size.
const fn default_size() -> Triple {
    [DEFAULT_EDGE_M, DEFAULT_EDGE_M, DEFAULT_EDGE_M]
}

/// An object as written: its root prim, and the prims linked to it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectSpec {
    /// The root prim's name, which avatars' steps and expectations name it
    /// by.
    pub name: String,
    /// Where it stands, in region metres.
    pub position: Triple,
    /// Its size, in metres.
    #[serde(default = "default_size")]
    pub size: Triple,
    /// Who owns it: one of the case's avatars by name, or anybody else, who
    /// is given an account that never logs in. The case's merchant,
    /// `Content Owner`, when absent.
    pub owner: Option<String>,
    /// What clicking it does before any script sets it.
    pub click: Option<ClickSpec>,
    /// What the root prim holds.
    #[serde(default)]
    pub contents: Vec<ContentSpec>,
    /// The prims linked to the root, in link order.
    #[serde(default, rename = "link")]
    pub links: Vec<LinkSpec>,
}

/// A child prim of a linkset as written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSpec {
    /// Its name.
    pub name: String,
    /// Where it stands relative to the root, in metres.
    pub offset: Triple,
    /// Its size, in metres.
    #[serde(default = "default_size")]
    pub size: Triple,
    /// What it holds.
    #[serde(default)]
    pub contents: Vec<ContentSpec>,
}

/// A click action as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClickSpec {
    /// Touch.
    Touch,
    /// Sit.
    Sit,
    /// Buy.
    Buy,
    /// Pay.
    Pay,
    /// Open.
    Open,
    /// Play media.
    Play,
    /// Open media.
    OpenMedia,
    /// Zoom.
    Zoom,
}

impl ClickSpec {
    /// The click action.
    const fn action(self) -> ClickAction {
        match self {
            Self::Touch => ClickAction::Touch,
            Self::Sit => ClickAction::Sit,
            Self::Buy => ClickAction::Buy,
            Self::Pay => ClickAction::Pay,
            Self::Open => ClickAction::Open,
            Self::Play => ClickAction::Play,
            Self::OpenMedia => ClickAction::OpenMedia,
            Self::Zoom => ClickAction::Zoom,
        }
    }
}

/// One item of a prim's contents as written: exactly one of its kinds, each
/// naming a file beside the case file (but `object`, which names the item).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentSpec {
    /// The item's name; the file's stem when absent.
    pub name: Option<String>,
    /// An LSL script's source file.
    pub script: Option<PathBuf>,
    /// Whether the script runs from the start (it does when absent).
    pub running: Option<bool>,
    /// A notecard's text file.
    pub notecard: Option<PathBuf>,
    /// An object item, by name: one a script can give (`llGiveInventory`).
    pub object: Option<String>,
    /// An animation asset file (`.anim`).
    pub animation: Option<PathBuf>,
    /// A sound asset file.
    pub sound: Option<PathBuf>,
    /// A texture asset file (`.j2c`).
    pub texture: Option<PathBuf>,
    /// The asset's key, for content that names it by uuid; one is minted
    /// when absent.
    pub id: Option<uuid::Uuid>,
}

/// An asset the region serves by key, as written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetSpec {
    /// Its key.
    pub id: uuid::Uuid,
    /// Its file, beside the case file.
    pub file: PathBuf,
}

/// An avatar as written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarSpec {
    /// Its name, `First Last`.
    pub name: String,
    /// Where in the region it logs in; the region's arrival point when
    /// absent.
    pub start: Option<Triple>,
    /// Whether it wears clothes over its body parts.
    #[serde(default)]
    pub dressed: bool,
    /// How it answers what no step answers.
    #[serde(default)]
    pub answers: AnswersSpec,
    /// What it does, in order.
    #[serde(default)]
    pub steps: Vec<StepSpec>,
}

/// How an avatar answers what no step answers, as written; each is
/// `"ignore"` when absent.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswersSpec {
    /// A permission question: `"ignore"`, `"grant"` or `"refuse"`.
    #[serde(default)]
    pub permissions: PermissionsSpec,
    /// A dialog: `"ignore"` or `"first-button"`.
    #[serde(default)]
    pub dialogs: DialogsSpec,
    /// A touch on something clicked to pay: `"never"` (a touch) or
    /// `"default-price"`.
    #[serde(default)]
    pub payments: PaymentsSpec,
}

/// A permission answer as written.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionsSpec {
    /// Leave it unanswered.
    #[default]
    Ignore,
    /// Grant what it asks.
    Grant,
    /// Answer it, granting nothing.
    Refuse,
}

/// A dialog answer as written.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DialogsSpec {
    /// Leave it open.
    #[default]
    Ignore,
    /// Take its first button.
    FirstButton,
}

/// A payment answer as written.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaymentsSpec {
    /// Touch it.
    #[default]
    Never,
    /// Pay what its pay window offers first.
    DefaultPrice,
}

impl AnswersSpec {
    /// The scripted avatar's policy.
    const fn policy(self) -> AnswerPolicy {
        AnswerPolicy {
            permissions: match self.permissions {
                PermissionsSpec::Ignore => PermissionAnswer::Ignore,
                PermissionsSpec::Grant => PermissionAnswer::GrantAll,
                PermissionsSpec::Refuse => PermissionAnswer::Refuse,
            },
            dialogs: match self.dialogs {
                DialogsSpec::Ignore => DialogAnswer::Ignore,
                DialogsSpec::FirstButton => DialogAnswer::FirstButton,
            },
            payments: match self.payments {
                PaymentsSpec::Never => PaymentAnswer::Never,
                PaymentsSpec::DefaultPrice => PaymentAnswer::DefaultPrice,
            },
        }
    }
}

/// One step of an avatar as written: exactly one action, and optionally how
/// many ticks after the previous step it is taken.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    /// Ticks after the previous step: 1 for an action, 0 for an answer or
    /// an awaited line, when absent.
    pub after_ticks: Option<u32>,
    /// Walk to this region point.
    pub walk: Option<Triple>,
    /// Touch the object of this name.
    pub touch: Option<String>,
    /// Sit on the object of this name.
    pub sit: Option<String>,
    /// Stand up.
    pub stand: Option<bool>,
    /// Say this.
    pub say: Option<String>,
    /// The channel `say` is said on; 0 when absent.
    pub channel: Option<i32>,
    /// Pay the object of this name.
    pub pay: Option<String>,
    /// How much `pay` pays; what its pay window offers first when absent.
    pub amount: Option<i32>,
    /// Answer the next dialog with the button of this label.
    pub answer_dialog: Option<String>,
    /// Answer the next dialog with the button at this position, from 0.
    pub answer_dialog_index: Option<usize>,
    /// Answer the next text box with this text.
    pub answer_text: Option<String>,
    /// Answer the next permission question granting these permissions
    /// (`PERMISSION_*` bits), of those it asked for.
    pub grant: Option<i32>,
    /// Answer the next permission question granting nothing.
    pub refuse: Option<bool>,
    /// Wait until a line containing this is heard.
    pub await_chat: Option<String>,
}

/// One expectation as written: what should, or should not, have happened.
/// Which kind it is follows from its fields: `transcript`, `fault`, `object`,
/// or else `says` / `says_containing`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectSpec {
    /// The speaker's name, for a line.
    pub by: Option<String>,
    /// The line's exact text.
    pub says: Option<String>,
    /// Text the line contains.
    pub says_containing: Option<String>,
    /// The channel the line was said on (region chat only).
    pub channel: Option<i32>,
    /// Look for the line among what this avatar was told — owner-says and
    /// lines said to it alone included — rather than in the region's chat.
    pub heard_by: Option<String>,
    /// The line must *not* have been said.
    #[serde(default)]
    pub never: bool,
    /// An avatar whose transcript is checked.
    pub transcript: Option<String>,
    /// Lines the transcript holds, in this order, others between them
    /// allowed.
    pub lines: Option<Vec<String>>,
    /// A run-time error expected, by text the error contains.
    pub fault: Option<String>,
    /// The script a `fault` is expected in.
    pub script: Option<String>,
    /// A prim whose place at the end of the run is checked, by name.
    pub object: Option<String>,
    /// Where `object` should stand at the end of the run, in region metres.
    pub at: Option<Triple>,
    /// How far from `at` it may stand, in metres; a centimetre when absent.
    pub within: Option<f32>,
}

/// A case, read and checked: what [`crate::run()`] plays.
#[derive(Debug, Clone)]
pub struct Case {
    /// What the report calls it.
    pub name: String,
    /// The case file.
    pub path: PathBuf,
    /// The grids it plays on.
    pub grids: Vec<ImitatedGrid>,
    /// The run's seed.
    pub seed: u64,
    /// A tick's length.
    pub step: Duration,
    /// The tick budget.
    pub max_ticks: u32,
    /// The ticks run after every avatar has finished.
    pub settle_ticks: u32,
    /// The objects.
    pub objects: Vec<Object>,
    /// The assets served by key.
    pub assets: Vec<(uuid::Uuid, Vec<u8>)>,
    /// The avatars.
    pub avatars: Vec<Avatar>,
    /// What should have happened.
    pub expectations: Vec<Expectation>,
}

/// An object of a case: its root prim and its linked prims.
#[derive(Debug, Clone)]
pub struct Object {
    /// The root prim.
    pub root: Prim,
    /// Where it stands.
    pub position: Vector,
    /// Who owns it, by name.
    pub owner: String,
    /// What clicking it does before a script sets it.
    pub click: Option<ClickAction>,
    /// The linked prims, in link order.
    pub links: Vec<Prim>,
}

/// One prim of an object.
#[derive(Debug, Clone)]
pub struct Prim {
    /// Its name.
    pub name: String,
    /// Its offset from the root; zero for the root.
    pub offset: Vector,
    /// Its size.
    pub size: Vector,
    /// What it holds.
    pub contents: Vec<Content>,
}

/// One item a prim holds.
#[derive(Debug, Clone)]
pub struct Content {
    /// Its name.
    pub name: String,
    /// What it is.
    pub item: Item,
}

/// What an item is.
#[derive(Debug, Clone)]
pub enum Item {
    /// An LSL script.
    Script {
        /// Its source.
        source: String,
        /// Whether it runs from the start.
        running: bool,
    },
    /// A notecard.
    Notecard {
        /// Its text.
        text: String,
    },
    /// An object, which a script can give but not rez: no object asset
    /// stands behind it.
    Object,
    /// An asset-backed item.
    Asset {
        /// What kind.
        kind: AssetKind,
        /// Its bytes.
        bytes: Vec<u8>,
        /// Its key, when the case fixed one.
        id: Option<uuid::Uuid>,
    },
}

/// The kind of an asset-backed item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// An animation.
    Animation,
    /// A sound.
    Sound,
    /// A texture.
    Texture,
}

/// An avatar of a case.
#[derive(Debug, Clone)]
pub struct Avatar {
    /// Its first name.
    pub first: String,
    /// Its last name.
    pub last: String,
    /// Where it logs in.
    pub start: Option<Vector>,
    /// Whether it wears clothes.
    pub dressed: bool,
    /// How it answers what no step answers.
    pub policy: AnswerPolicy,
    /// Its steps: ticks after the previous one, and the action.
    pub steps: Vec<(u32, AvatarAction)>,
}

impl Avatar {
    /// Its full name.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{} {}", self.first, self.last)
    }
}

/// What should have happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Expectation {
    /// A line was said (or, `never`, was not).
    Said(SaidExpectation),
    /// An avatar's transcript holds these lines in this order.
    Transcript {
        /// The avatar.
        avatar: String,
        /// The lines.
        lines: Vec<String>,
    },
    /// A script hit a run-time error.
    Fault {
        /// Text the error contains.
        error: String,
        /// The script it happened in, when named.
        script: Option<String>,
    },
    /// A prim stands somewhere at the end of the run.
    Position {
        /// The prim, by name.
        object: String,
        /// Where, in region metres.
        at: Vector,
        /// How far from there it may stand, in metres.
        within: f32,
    },
}

/// A line expected (or expected never) to be said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaidExpectation {
    /// The speaker, when named.
    pub by: Option<String>,
    /// The text.
    pub text: TextMatch,
    /// The channel, when named.
    pub channel: Option<i32>,
    /// The avatar whose lines are searched, or the region's chat.
    pub heard_by: Option<String>,
    /// Whether the line must not have been said.
    pub never: bool,
}

/// How a line's text is matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextMatch {
    /// Exactly.
    Exactly(String),
    /// As a part.
    Containing(String),
}

impl TextMatch {
    /// Whether `text` matches.
    #[must_use]
    pub fn matches(&self, text: &str) -> bool {
        match self {
            Self::Exactly(wanted) => text == wanted,
            Self::Containing(part) => text.contains(part.as_str()),
        }
    }
}

impl std::fmt::Display for TextMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exactly(text) => write!(f, "{text:?}"),
            Self::Containing(part) => write!(f, "something containing {part:?}"),
        }
    }
}

/// How far from its expected place a prim may stand when the case says
/// nothing, in metres.
const DEFAULT_WITHIN_M: f32 = 0.01;

/// The merchant who owns an object whose case names no owner.
pub const DEFAULT_OWNER: &str = "Content Owner";

/// A triple as a vector.
const fn vector([x, y, z]: Triple) -> Vector {
    Vector { x, y, z }
}

impl Case {
    /// Reads the case file at `path` and every file it names.
    ///
    /// # Errors
    ///
    /// [`LoadError`] when a file cannot be read, the case file does not
    /// parse, or it says something that cannot be played.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let text = fs_err::read_to_string(path).map_err(|source| LoadError::Read {
            path: path.to_owned(),
            source,
        })?;
        let file: Document = toml::from_str(&text).map_err(|source| LoadError::Parse {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
        Self::from_file(path, file)
    }

    /// Checks `file`, read from `path`, and reads what it names from beside
    /// it.
    ///
    /// # Errors
    ///
    /// As [`load`](Self::load), but for parsing.
    pub fn from_file(path: &Path, file: Document) -> Result<Self, LoadError> {
        let reader = Reader {
            path,
            dir: path.parent().unwrap_or_else(|| Path::new(".")),
        };
        let name = file.name.clone().unwrap_or_else(|| default_name(path));
        if file.grids.is_empty() {
            return Err(reader.invalid("`grids` names no grid".to_owned()));
        }
        if file.step_ms == 0 {
            return Err(reader.invalid("`step_ms` must be above 0".to_owned()));
        }
        let mut objects = Vec::new();
        for object in &file.objects {
            objects.push(reader.object(object)?);
        }
        let mut assets = Vec::new();
        for asset in &file.assets {
            assets.push((asset.id, reader.bytes(&asset.file)?));
        }
        let mut avatars = Vec::new();
        for avatar in &file.avatars {
            avatars.push(reader.avatar(avatar)?);
        }
        let mut expectations = Vec::new();
        for (index, expect) in file.expectations.iter().enumerate() {
            expectations.push(reader.expectation(index, expect)?);
        }
        let case = Self {
            name,
            path: path.to_owned(),
            grids: file.grids.iter().map(|grid| grid.imitated()).collect(),
            seed: file.seed,
            step: Duration::from_millis(file.step_ms),
            max_ticks: file.max_ticks,
            settle_ticks: file.settle_ticks,
            objects,
            assets,
            avatars,
            expectations,
        };
        case.check_names(&reader)?;
        Ok(case)
    }

    /// Checks that every avatar a step or an expectation names is one of the
    /// case's, and that no two avatars share a name.
    fn check_names(&self, reader: &Reader<'_>) -> Result<(), LoadError> {
        let names: Vec<String> = self.avatars.iter().map(Avatar::name).collect();
        for (index, name) in names.iter().enumerate() {
            if names
                .get(..index)
                .is_some_and(|before| before.contains(name))
            {
                return Err(reader.invalid(format!("two avatars are called {name:?}")));
            }
        }
        for expectation in &self.expectations {
            let avatar = match expectation {
                Expectation::Said(said) => said.heard_by.as_ref(),
                Expectation::Transcript { avatar, .. } => Some(avatar),
                Expectation::Fault { .. } | Expectation::Position { .. } => None,
            };
            if let Some(avatar) = avatar
                && !names.contains(avatar)
            {
                return Err(reader.invalid(format!(
                    "an expectation names the avatar {avatar:?}, which the case does not bring"
                )));
            }
        }
        Ok(())
    }

    /// Every script of the case: the prim it is in, and its item.
    pub fn scripts(&self) -> impl Iterator<Item = (&Prim, &Content)> {
        self.objects
            .iter()
            .flat_map(|object| std::iter::once(&object.root).chain(&object.links))
            .flat_map(|prim| prim.contents.iter().map(move |content| (prim, content)))
            .filter(|(_prim, content)| matches!(content.item, Item::Script { .. }))
    }

    /// Replaces the source of every script called `name` with `source`;
    /// whether there was one. How a test breaks a script to see the case
    /// fail.
    pub fn replace_script(&mut self, name: &str, source: &str) -> bool {
        let mut replaced = false;
        for object in &mut self.objects {
            for prim in std::iter::once(&mut object.root).chain(&mut object.links) {
                for content in &mut prim.contents {
                    if content.name == name
                        && let Item::Script { source: held, .. } = &mut content.item
                    {
                        source.clone_into(held);
                        replaced = true;
                    }
                }
            }
        }
        replaced
    }
}

/// A case's name when its file gives none: its directory's name for a file
/// called `case.toml`, else the file's stem.
fn default_name(path: &Path) -> String {
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    let dir = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|dir| dir.to_str());
    match (stem, dir) {
        (Some("case"), Some(dir)) => dir.to_owned(),
        (Some(stem), _) => stem.trim_end_matches(".case").to_owned(),
        (None, _) => path.display().to_string(),
    }
}

/// Reads what a case file names, relative to its directory.
struct Reader<'path> {
    /// The case file, for errors.
    path: &'path Path,
    /// Its directory.
    dir: &'path Path,
}

impl Reader<'_> {
    /// An invalid-case error.
    fn invalid(&self, what: String) -> LoadError {
        LoadError::Invalid {
            path: self.path.to_owned(),
            what,
        }
    }

    /// The bytes of `file`, beside the case file.
    fn bytes(&self, file: &Path) -> Result<Vec<u8>, LoadError> {
        let path = self.dir.join(file);
        fs_err::read(&path).map_err(|source| LoadError::Read { path, source })
    }

    /// The text of `file`, beside the case file.
    fn text(&self, file: &Path) -> Result<String, LoadError> {
        let path = self.dir.join(file);
        fs_err::read_to_string(&path).map_err(|source| LoadError::Read { path, source })
    }

    /// An object.
    fn object(&self, spec: &ObjectSpec) -> Result<Object, LoadError> {
        let root = Prim {
            name: spec.name.clone(),
            offset: vector([0.0, 0.0, 0.0]),
            size: vector(spec.size),
            contents: self.contents(&spec.name, &spec.contents)?,
        };
        let mut links = Vec::new();
        for link in &spec.links {
            links.push(Prim {
                name: link.name.clone(),
                offset: vector(link.offset),
                size: vector(link.size),
                contents: self.contents(&link.name, &link.contents)?,
            });
        }
        Ok(Object {
            root,
            position: vector(spec.position),
            owner: spec
                .owner
                .clone()
                .unwrap_or_else(|| DEFAULT_OWNER.to_owned()),
            click: spec.click.map(ClickSpec::action),
            links,
        })
    }

    /// The contents of the prim called `prim`.
    fn contents(&self, prim: &str, specs: &[ContentSpec]) -> Result<Vec<Content>, LoadError> {
        let mut contents = Vec::new();
        for (index, spec) in specs.iter().enumerate() {
            let content = self.content(spec).map_err(|error| match error {
                LoadError::Invalid { path, what } => LoadError::Invalid {
                    path,
                    what: format!("{prim:?}'s item {}: {what}", index.saturating_add(1)),
                },
                other @ (LoadError::Read { .. } | LoadError::Parse { .. }) => other,
            })?;
            if contents
                .iter()
                .any(|held: &Content| held.name == content.name)
            {
                return Err(self.invalid(format!(
                    "{prim:?} holds two items called {:?}",
                    content.name
                )));
            }
            contents.push(content);
        }
        Ok(contents)
    }

    /// One item.
    fn content(&self, spec: &ContentSpec) -> Result<Content, LoadError> {
        let kinds = [
            spec.script.is_some(),
            spec.notecard.is_some(),
            spec.object.is_some(),
            spec.animation.is_some(),
            spec.sound.is_some(),
            spec.texture.is_some(),
        ];
        if kinds.iter().filter(|kind| **kind).count() != 1 {
            return Err(self.invalid(
                "name exactly one of `script`, `notecard`, `object`, `animation`, `sound` and \
                 `texture`"
                    .to_owned(),
            ));
        }
        if spec.running.is_some() && spec.script.is_none() {
            return Err(self.invalid("only a script has `running`".to_owned()));
        }
        let named = |file: &Path| {
            spec.name.clone().unwrap_or_else(|| {
                file.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned()
            })
        };
        let asset = |kind: AssetKind, file: &Path| -> Result<Content, LoadError> {
            Ok(Content {
                name: named(file),
                item: Item::Asset {
                    kind,
                    bytes: self.bytes(file)?,
                    id: spec.id,
                },
            })
        };
        if spec.id.is_some()
            && spec.animation.is_none()
            && spec.sound.is_none()
            && spec.texture.is_none()
        {
            return Err(self.invalid("only an animation, sound or texture has `id`".to_owned()));
        }
        if let Some(file) = &spec.script {
            return Ok(Content {
                name: named(file),
                item: Item::Script {
                    source: self.text(file)?,
                    running: spec.running.unwrap_or(true),
                },
            });
        }
        if let Some(file) = &spec.notecard {
            return Ok(Content {
                name: named(file),
                item: Item::Notecard {
                    text: self.text(file)?,
                },
            });
        }
        if let Some(object) = &spec.object {
            return Ok(Content {
                name: spec.name.clone().unwrap_or_else(|| object.clone()),
                item: Item::Object,
            });
        }
        if let Some(file) = &spec.animation {
            return asset(AssetKind::Animation, file);
        }
        if let Some(file) = &spec.sound {
            return asset(AssetKind::Sound, file);
        }
        match &spec.texture {
            Some(file) => asset(AssetKind::Texture, file),
            None => Err(self.invalid("names no item".to_owned())),
        }
    }

    /// An avatar.
    fn avatar(&self, spec: &AvatarSpec) -> Result<Avatar, LoadError> {
        let Some((first, last)) = spec.name.split_once(' ') else {
            return Err(self.invalid(format!(
                "the avatar {:?} needs a first and a last name",
                spec.name
            )));
        };
        let mut steps = Vec::new();
        for (index, step) in spec.steps.iter().enumerate() {
            steps.push(step_of(step).map_err(|what| {
                self.invalid(format!(
                    "{}'s step {}: {what}",
                    spec.name,
                    index.saturating_add(1)
                ))
            })?);
        }
        Ok(Avatar {
            first: first.to_owned(),
            last: last.to_owned(),
            start: spec.start.map(vector),
            dressed: spec.dressed,
            policy: spec.answers.policy(),
            steps,
        })
    }

    /// The expectation at `index`.
    fn expectation(&self, index: usize, spec: &ExpectSpec) -> Result<Expectation, LoadError> {
        expectation_of(spec).map_err(|what| {
            self.invalid(format!("expectation {}: {what}", index.saturating_add(1)))
        })
    }
}

/// A step's ticks and action, or what is wrong with it.
fn step_of(spec: &StepSpec) -> Result<(u32, AvatarAction), String> {
    let named = |name: &String| Target::Named(name.clone());
    let mut actions: Vec<(u32, AvatarAction)> = Vec::new();
    if let Some(to) = spec.walk {
        actions.push((1, AvatarAction::Walk { to: vector(to) }));
    }
    if let Some(target) = &spec.touch {
        actions.push((1, AvatarAction::Touch(named(target))));
    }
    if let Some(target) = &spec.sit {
        actions.push((1, AvatarAction::Sit(named(target))));
    }
    if spec.stand == Some(true) {
        actions.push((1, AvatarAction::Stand));
    }
    if let Some(message) = &spec.say {
        actions.push((
            1,
            AvatarAction::Say {
                channel: spec.channel.unwrap_or(0),
                message: message.clone(),
            },
        ));
    }
    if let Some(target) = &spec.pay {
        actions.push((
            1,
            AvatarAction::Pay {
                target: named(target),
                amount: spec.amount,
            },
        ));
    }
    if let Some(label) = &spec.answer_dialog {
        actions.push((
            0,
            AvatarAction::AnswerDialog(DialogChoice::Label(label.clone())),
        ));
    }
    if let Some(index) = spec.answer_dialog_index {
        actions.push((0, AvatarAction::AnswerDialog(DialogChoice::Index(index))));
    }
    if let Some(text) = &spec.answer_text {
        actions.push((
            0,
            AvatarAction::AnswerDialog(DialogChoice::Text(text.clone())),
        ));
    }
    if let Some(bits) = spec.grant {
        actions.push((0, AvatarAction::AnswerPermissions(ScriptPermissions(bits))));
    }
    if spec.refuse == Some(true) {
        actions.push((
            0,
            AvatarAction::AnswerPermissions(ScriptPermissions::default()),
        ));
    }
    if let Some(wanted) = &spec.await_chat {
        actions.push((0, AvatarAction::AwaitChat(wanted.clone())));
    }
    if spec.channel.is_some() && spec.say.is_none() {
        return Err("only `say` has a `channel`".to_owned());
    }
    if spec.amount.is_some() && spec.pay.is_none() {
        return Err("only `pay` has an `amount`".to_owned());
    }
    let mut actions = actions.into_iter();
    match (actions.next(), actions.next()) {
        (Some((ticks, action)), None) => Ok((spec.after_ticks.unwrap_or(ticks), action)),
        (None, _) => Err("names no action".to_owned()),
        (Some(_), Some(_)) => Err("names more than one action".to_owned()),
    }
}

/// An expectation, or what is wrong with it.
fn expectation_of(spec: &ExpectSpec) -> Result<Expectation, String> {
    let said = spec.says.is_some() || spec.says_containing.is_some();
    let line_fields = spec.by.is_some() || spec.channel.is_some() || spec.heard_by.is_some();
    let place_fields = spec.at.is_some() || spec.within.is_some();
    if let Some(object) = &spec.object {
        if said
            || line_fields
            || spec.never
            || spec.transcript.is_some()
            || spec.lines.is_some()
            || spec.fault.is_some()
            || spec.script.is_some()
        {
            return Err("an `object` expectation takes only `at` and `within`".to_owned());
        }
        let Some(at) = spec.at else {
            return Err("an `object` expectation needs `at`".to_owned());
        };
        let within = spec.within.unwrap_or(DEFAULT_WITHIN_M);
        if within.is_nan() || within < 0.0 {
            return Err("`within` is a distance: zero or more".to_owned());
        }
        return Ok(Expectation::Position {
            object: object.clone(),
            at: vector(at),
            within,
        });
    }
    if place_fields {
        return Err("only an `object` expectation has `at` and `within`".to_owned());
    }
    if let Some(avatar) = &spec.transcript {
        if said || line_fields || spec.fault.is_some() || spec.never {
            return Err("a `transcript` expectation takes only `lines`".to_owned());
        }
        let Some(lines) = spec.lines.clone() else {
            return Err("a `transcript` expectation needs `lines`".to_owned());
        };
        return Ok(Expectation::Transcript {
            avatar: avatar.clone(),
            lines,
        });
    }
    if spec.lines.is_some() {
        return Err("only a `transcript` expectation has `lines`".to_owned());
    }
    if let Some(error) = &spec.fault {
        if said || line_fields || spec.never {
            return Err("a `fault` expectation takes only `script`".to_owned());
        }
        return Ok(Expectation::Fault {
            error: error.clone(),
            script: spec.script.clone(),
        });
    }
    if spec.script.is_some() {
        return Err("only a `fault` expectation has `script`".to_owned());
    }
    let text = match (&spec.says, &spec.says_containing) {
        (Some(text), None) => TextMatch::Exactly(text.clone()),
        (None, Some(part)) => TextMatch::Containing(part.clone()),
        (Some(_), Some(_)) => return Err("name `says` or `says_containing`, not both".to_owned()),
        (None, None) => {
            return Err(
                "name what is expected: `says`, `says_containing`, `transcript` or `fault`"
                    .to_owned(),
            );
        }
    };
    if spec.heard_by.is_some() && spec.channel.is_some() {
        return Err(
            "an avatar is not told a line's channel: `heard_by` takes no `channel`".to_owned(),
        );
    }
    Ok(Expectation::Said(SaidExpectation {
        by: spec.by.clone(),
        text,
        channel: spec.channel,
        heard_by: spec.heard_by.clone(),
        never: spec.never,
    }))
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// Parses a case file's text.
    fn parsed(text: &str) -> Result<Document, toml::de::Error> {
        toml::from_str(text)
    }

    /// A step names exactly one action, takes its default ticks, and keeps
    /// what it was told.
    #[test]
    fn a_step_names_one_action() -> Result<(), String> {
        let file = parsed(
            r#"
            [[avatar]]
            name = "Buyer Resident"
            steps = [
              { touch = "Vendor" },
              { answer_dialog = "Blue" },
              { say = "hello", channel = 5, after_ticks = 3 },
              { pay = "Vendor", amount = 50 },
              { await_chat = "Thank" },
            ]
            "#,
        )
        .map_err(|error| error.to_string())?;
        let steps: Vec<(u32, AvatarAction)> = file
            .avatars
            .first()
            .ok_or("no avatar")?
            .steps
            .iter()
            .map(step_of)
            .collect::<Result<_, _>>()?;
        assert_eq!(
            steps,
            vec![
                (1, AvatarAction::Touch(Target::Named("Vendor".to_owned()))),
                (
                    0,
                    AvatarAction::AnswerDialog(DialogChoice::Label("Blue".to_owned()))
                ),
                (
                    3,
                    AvatarAction::Say {
                        channel: 5,
                        message: "hello".to_owned()
                    }
                ),
                (
                    1,
                    AvatarAction::Pay {
                        target: Target::Named("Vendor".to_owned()),
                        amount: Some(50)
                    }
                ),
                (0, AvatarAction::AwaitChat("Thank".to_owned())),
            ]
        );
        Ok(())
    }

    /// A step with no action, two actions, or a stray modifier is refused,
    /// saying which.
    #[test]
    fn a_step_that_is_not_one_action_is_refused() {
        assert_eq!(
            step_of(&StepSpec::default()),
            Err("names no action".to_owned())
        );
        assert_eq!(
            step_of(&StepSpec {
                touch: Some("A".to_owned()),
                sit: Some("A".to_owned()),
                ..StepSpec::default()
            }),
            Err("names more than one action".to_owned())
        );
        assert_eq!(
            step_of(&StepSpec {
                touch: Some("A".to_owned()),
                channel: Some(3),
                ..StepSpec::default()
            }),
            Err("only `say` has a `channel`".to_owned())
        );
    }

    /// Each kind of expectation is told by its fields, and a mix is refused.
    #[test]
    fn an_expectation_is_told_by_its_fields() {
        assert_eq!(
            expectation_of(&ExpectSpec {
                by: Some("Vendor".to_owned()),
                says_containing: Some("Thank".to_owned()),
                channel: Some(0),
                ..ExpectSpec::default()
            }),
            Ok(Expectation::Said(SaidExpectation {
                by: Some("Vendor".to_owned()),
                text: TextMatch::Containing("Thank".to_owned()),
                channel: Some(0),
                heard_by: None,
                never: false,
            }))
        );
        assert_eq!(
            expectation_of(&ExpectSpec {
                fault: Some("Math Error".to_owned()),
                script: Some("vendor".to_owned()),
                ..ExpectSpec::default()
            }),
            Ok(Expectation::Fault {
                error: "Math Error".to_owned(),
                script: Some("vendor".to_owned()),
            })
        );
        assert_eq!(
            expectation_of(&ExpectSpec {
                transcript: Some("Buyer Resident".to_owned()),
                says: Some("x".to_owned()),
                lines: Some(Vec::new()),
                ..ExpectSpec::default()
            }),
            Err("a `transcript` expectation takes only `lines`".to_owned())
        );
        assert_eq!(
            expectation_of(&ExpectSpec::default()),
            Err(
                "name what is expected: `says`, `says_containing`, `transcript` or `fault`"
                    .to_owned()
            )
        );
    }

    /// An unknown field is a parse error naming it, so a misspelt field is
    /// never silently ignored.
    #[test]
    fn an_unknown_field_is_refused() {
        let error = parsed(
            r#"
            [[object]]
            name = "Vendor"
            position = [128.0, 128.0, 21.0]
            colour = "red"
            "#,
        )
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(error.contains("colour"), "{error}");
    }

    /// A case file called `case.toml` is named after its directory, and any
    /// other after its stem.
    #[test]
    fn a_case_is_named_after_its_file() {
        assert_eq!(default_name(Path::new("cases/vendor/case.toml")), "vendor");
        assert_eq!(default_name(Path::new("cases/menu.case.toml")), "menu");
        assert_eq!(default_name(Path::new("cases/menu.toml")), "menu");
    }
}
