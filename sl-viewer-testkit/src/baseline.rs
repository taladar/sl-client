//! **Baselines**: a committed recording of *derived* facts — counts, extents,
//! angles, positions — that are not wrong at any particular value but must not
//! change by accident, checked on every run and changed only deliberately.
//!
//! The universal and declared check tiers catch what is *wrong*. A pie option's
//! angle, a floater's default size, the vertex count a box tessellates to at
//! each LOD: nothing is incorrect if one of those moves, and a refactor can move
//! it for free — and a user who has opened the same menu ten thousand times
//! notices. So a named subject records the facts that are load-bearing for it
//! into `baselines/<crate>/<tier>/<id>.toml`; a run compares; a difference
//! fails; and the only way to change one is to re-bless the file in the same
//! commit, where a reviewer sees "this moved the Sit option 12°" as a sentence
//! they can object to.
//!
//! Two tiers share this one format — the UI's and the render harness's — so
//! there is one bless flow (`SL_VIEWER_BLESS_BASELINES=1`, the settings golden's
//! idiom) and one file shape. Record derived intent, never raw dumps: a vertex
//! dump changes whenever a float does and teaches everyone to re-bless without
//! reading.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::math::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// The environment variable that rewrites every checked baseline instead of
/// comparing against it.
pub const BLESS_ENV: &str = "SL_VIEWER_BLESS_BASELINES";

/// The file format's version, for a future migration to name.
const SCHEMA: u32 = 1;

/// The directory every baseline lives in, under the workspace root.
const BASELINE_DIR: &str = "baselines";

/// The workspace root, resolved at compile time from this crate's own manifest
/// directory.
///
/// This crate is a **direct member** of the workspace root, so its parent is the
/// root — and a baseline path has to be absolute, because a test's working
/// directory is the crate the test lives in, not the workspace. The assumption
/// is held to the tree by a test of its own rather than left as a comment; see
/// `the_resolved_workspace_root_is_the_workspace_root`.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Where the baseline for one subject lives: `baselines/<crate>/<tier>/<id>.toml`
/// under the workspace root.
///
/// One file per subject rather than one file per tier, so two subjects moving in
/// two commits never conflict, and so a review diff names the subject in the file
/// path.
#[must_use]
pub fn subject_path(krate: &str, tier: &str, id: &str) -> PathBuf {
    PathBuf::from(WORKSPACE_ROOT)
        .join(BASELINE_DIR)
        .join(krate)
        .join(tier)
        .join(format!("{id}.toml"))
}

/// The facts one run measured, keyed by name.
///
/// A builder rather than a bare map because a subject records a dozen facts and
/// each one is a `(name, Fact)` pair the caller would otherwise spell out in
/// full; the recording code should read as the list of facts it is, not as map
/// insertions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// The facts so far, sorted by name — the order they are written in.
    facts: BTreeMap<String, Fact>,
}

/// How many decimals a recorded number keeps.
///
/// The file is a *reviewed record* compared within a tolerance, not a bit-exact
/// dump: widening a measured `f32` to `f64` writes `0.42309999465942383` where
/// the measurement was `0.4231`, and a reviewer reading a diff of those learns
/// nothing the sixth decimal did not already say. Six decimals is a micrometre
/// on a metre and a micro-degree on an angle — far under every tolerance any
/// fact here is compared at.
const ROUNDING: f64 = 1.0e6;

/// A measured number, rounded to [`ROUNDING`].
fn rounded(value: f32) -> f64 {
    (f64::from(value) * ROUNDING).round() / ROUNDING
}

impl Facts {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a count, an index or an ordinal.
    pub fn int(&mut self, key: &str, value: i64) -> &mut Self {
        self.facts.insert(key.to_owned(), Fact::Int(value));
        self
    }

    /// Record a name, an order or a symbolic state.
    pub fn text(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.facts.insert(key.to_owned(), Fact::Text(value.into()));
        self
    }

    /// Record a measurement, compared within `tolerance`.
    pub fn float(&mut self, key: &str, value: f32, tolerance: f64) -> &mut Self {
        self.facts.insert(
            key.to_owned(),
            Fact::Float {
                value: rounded(value),
                tolerance,
            },
        );
        self
    }

    /// Record a two-dimensional point or extent, compared per component.
    pub fn vec2(&mut self, key: &str, value: Vec2, tolerance: f64) -> &mut Self {
        self.facts.insert(
            key.to_owned(),
            Fact::Vec2 {
                x: rounded(value.x),
                y: rounded(value.y),
                tolerance,
            },
        );
        self
    }

    /// Record a three-dimensional point or extent, compared per component.
    pub fn vec3(&mut self, key: &str, value: Vec3, tolerance: f64) -> &mut Self {
        self.facts.insert(
            key.to_owned(),
            Fact::Vec3 {
                x: rounded(value.x),
                y: rounded(value.y),
                z: rounded(value.z),
                tolerance,
            },
        );
        self
    }

    /// How many facts have been recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Whether nothing has been recorded — a subject that measured nothing is a
    /// baseline that would compare nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// The recorded facts, by name.
    #[must_use]
    pub fn into_map(self) -> BTreeMap<String, Fact> {
        self.facts
    }
}

/// One recorded fact. Floats carry the tolerance they are compared at, so the
/// file says how exact each number is meant to be.
///
/// Untagged, so the file reads as plain values: `vertices = 24`, `name = "box"`,
/// `angle = { value = 45.0, tolerance = 0.5 }`. The variants are ordered most
/// specific first — an untagged enum takes the first variant that fits, and a
/// three-component point would otherwise read as a two-component one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Fact {
    /// A count, an index, an ordinal.
    Int(i64),
    /// A name, an order, a symbolic state.
    Text(String),
    /// A point or extent in three dimensions, compared per component.
    Vec3 {
        /// X.
        x: f64,
        /// Y.
        y: f64,
        /// Z.
        z: f64,
        /// How far any component may differ.
        tolerance: f64,
    },
    /// A point or extent in two dimensions, compared per component.
    Vec2 {
        /// X.
        x: f64,
        /// Y.
        y: f64,
        /// How far either component may differ.
        tolerance: f64,
    },
    /// A measurement, compared within its tolerance.
    Float {
        /// The value.
        value: f64,
        /// How far it may differ.
        tolerance: f64,
    },
}

impl Fact {
    /// Whether `current` is this recorded fact, within its tolerance.
    fn matches(&self, current: &Self) -> bool {
        match (self, current) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Text(a), Self::Text(b)) => a == b,
            (
                Self::Float {
                    value: a,
                    tolerance,
                },
                Self::Float { value: b, .. },
            ) => (a - b).abs() <= *tolerance,
            (
                Self::Vec2 {
                    x: ax,
                    y: ay,
                    tolerance,
                },
                Self::Vec2 { x: bx, y: by, .. },
            ) => (ax - bx).abs() <= *tolerance && (ay - by).abs() <= *tolerance,
            (
                Self::Vec3 {
                    x: ax,
                    y: ay,
                    z: az,
                    tolerance,
                },
                Self::Vec3 {
                    x: bx,
                    y: by,
                    z: bz,
                    ..
                },
            ) => {
                (ax - bx).abs() <= *tolerance
                    && (ay - by).abs() <= *tolerance
                    && (az - bz).abs() <= *tolerance
            }
            _different_kinds => false,
        }
    }
}

/// A baseline file: provenance, then the facts, sorted by key so a diff is
/// stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    /// The file format's version.
    pub schema: u32,
    /// `git describe` of the tree the facts were blessed from — provenance for
    /// the reviewer, never compared.
    pub blessed_describe: String,
    /// Seconds since the Unix epoch when the facts were blessed — provenance,
    /// never compared.
    pub blessed_at_unix: u64,
    /// The recorded facts, by name.
    pub facts: BTreeMap<String, Fact>,
}

/// One fact that moved, appeared, or vanished.
#[derive(Debug, Clone, PartialEq)]
pub struct Drift {
    /// The fact's name.
    pub key: String,
    /// What the file records, if anything.
    pub recorded: Option<Fact>,
    /// What the run measured, if anything.
    pub current: Option<Fact>,
}

impl core::fmt::Display for Drift {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (&self.recorded, &self.current) {
            (Some(recorded), Some(current)) => {
                write!(f, "{}: recorded {recorded:?}, now {current:?}", self.key)
            }
            (Some(recorded), None) => write!(f, "{}: recorded {recorded:?}, now absent", self.key),
            (None, Some(current)) => {
                write!(f, "{}: not recorded, now {current:?}", self.key)
            }
            (None, None) => write!(f, "{}: absent on both sides", self.key),
        }
    }
}

/// Every difference between the recorded baseline and the `current` facts.
#[must_use]
pub fn compare(recorded: &Baseline, current: &BTreeMap<String, Fact>) -> Vec<Drift> {
    let mut drifts = Vec::new();
    for (key, fact) in &recorded.facts {
        match current.get(key) {
            Some(now) if fact.matches(now) => {}
            now => drifts.push(Drift {
                key: key.clone(),
                recorded: Some(fact.clone()),
                current: now.cloned(),
            }),
        }
    }
    for (key, now) in current {
        if !recorded.facts.contains_key(key) {
            drifts.push(Drift {
                key: key.clone(),
                recorded: None,
                current: Some(now.clone()),
            });
        }
    }
    drifts
}

/// Why a baseline check did not pass.
#[expect(
    clippy::module_name_repetitions,
    reason = "read at call sites as `baseline::BaselineError` only in this module's \
              own signatures; consumers import it, where the module name is gone"
)]
#[derive(Debug)]
pub enum BaselineError {
    /// No baseline file yet: bless one, deliberately.
    Missing {
        /// Where the file was looked for.
        path: String,
    },
    /// The file exists but is not a baseline.
    Unreadable {
        /// Where the file is.
        path: String,
        /// What went wrong.
        reason: String,
    },
    /// The facts moved.
    Drifted {
        /// Where the file is.
        path: String,
        /// Every fact that differs.
        drifts: Vec<Drift>,
    },
    /// Blessing failed.
    Bless {
        /// Where the file was to be written.
        path: String,
        /// What went wrong.
        reason: String,
    },
}

impl core::fmt::Display for BaselineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Missing { path } => write!(
                f,
                "no baseline at {path}; record one deliberately with {BLESS_ENV}=1"
            ),
            Self::Unreadable { path, reason } => {
                write!(f, "the baseline at {path} could not be read: {reason}")
            }
            Self::Drifted { path, drifts } => {
                writeln!(
                    f,
                    "{} recorded fact(s) moved against {path} — if that is intended, re-bless \
                     with {BLESS_ENV}=1 in the same commit:",
                    drifts.len()
                )?;
                for drift in drifts {
                    writeln!(f, "  {drift}")?;
                }
                Ok(())
            }
            Self::Bless { path, reason } => write!(f, "could not bless {path}: {reason}"),
        }
    }
}

impl core::error::Error for BaselineError {}

/// Compare `current` against the baseline at `path`, or — with [`BLESS_ENV`]
/// set — rewrite it.
///
/// # Errors
///
/// A missing file is an error naming the bless command (never an implicit
/// bless), an unreadable file is an error, and any drift is an error listing
/// every moved fact.
pub fn check(path: &Path, current: Facts) -> Result<(), BaselineError> {
    let current = current.into_map();
    if std::env::var_os(BLESS_ENV).is_some() {
        return bless(path, current);
    }
    let shown = path.display().to_string();
    let text = match fs_err::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(BaselineError::Missing { path: shown });
        }
        Err(error) => {
            return Err(BaselineError::Unreadable {
                path: shown,
                reason: error.to_string(),
            });
        }
    };
    let recorded: Baseline = toml::from_str(&text).map_err(|error| BaselineError::Unreadable {
        path: shown.clone(),
        reason: error.to_string(),
    })?;
    let drifts = compare(&recorded, &current);
    if drifts.is_empty() {
        Ok(())
    } else {
        Err(BaselineError::Drifted {
            path: shown,
            drifts,
        })
    }
}

/// Compare one subject's facts against [`subject_path`], or — with [`BLESS_ENV`]
/// set — rewrite it.
///
/// The call every baselined subject makes: it names the crate, the tier and the
/// subject rather than a path, so no caller spells the layout out and the layout
/// stays changeable in one place.
///
/// # Errors
///
/// As [`check`].
pub fn check_subject(
    krate: &str,
    tier: &str,
    id: &str,
    current: Facts,
) -> Result<(), BaselineError> {
    check(&subject_path(krate, tier, id), current)
}

/// The baseline files in `baselines/<crate>/<tier>/` whose subject is not in
/// `known` — a recording for something that no longer exists.
///
/// The failure this catches is quiet and one-directional: a subject that is
/// *renamed or removed* leaves its file behind, no check reads it any more, and
/// the tree accumulates recordings of things that are gone — while every check
/// stays green. A missing directory is no orphans (nothing is baselined yet), so
/// a tier can adopt baselines without its orphan test having to know first.
///
/// Ids come back sorted, so a failure message is stable.
///
/// # Errors
///
/// If the directory exists but cannot be read.
pub fn orphans(krate: &str, tier: &str, known: &[&str]) -> Result<Vec<String>, BaselineError> {
    let dir = PathBuf::from(WORKSPACE_ROOT)
        .join(BASELINE_DIR)
        .join(krate)
        .join(tier);
    let entries = match fs_err::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(BaselineError::Unreadable {
                path: dir.display().to_string(),
                reason: error.to_string(),
            });
        }
    };
    let mut orphans = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| BaselineError::Unreadable {
                path: dir.display().to_string(),
                reason: error.to_string(),
            })?
            .path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
        else {
            continue;
        };
        if !known.contains(&id.as_str()) {
            orphans.push(id);
        }
    }
    orphans.sort();
    Ok(orphans)
}

/// Write `current` to `path` as a fresh baseline, stamped with the tree's
/// `git describe` and the time.
fn bless(path: &Path, facts: BTreeMap<String, Fact>) -> Result<(), BaselineError> {
    let shown = path.display().to_string();
    let baseline = Baseline {
        schema: SCHEMA,
        blessed_describe: git_describe(),
        blessed_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
        facts,
    };
    let text = toml::to_string_pretty(&baseline).map_err(|error| BaselineError::Bless {
        path: shown.clone(),
        reason: error.to_string(),
    })?;
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent).map_err(|error| BaselineError::Bless {
            path: shown.clone(),
            reason: error.to_string(),
        })?;
    }
    fs_err::write(path, text).map_err(|error| BaselineError::Bless {
        path: shown,
        reason: error.to_string(),
    })
}

/// `git describe --tags --always --long --dirty` of the working tree, or
/// `unknown` outside a repository.
fn git_describe() -> String {
    std::process::Command::new("git")
        .args(["describe", "--tags", "--always", "--long", "--dirty"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |output| String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::{Baseline, BaselineError, Fact, Facts, compare};

    type TestError = Box<dyn core::error::Error>;

    fn facts(entries: &[(&str, Fact)]) -> BTreeMap<String, Fact> {
        entries
            .iter()
            .map(|(key, fact)| ((*key).to_owned(), fact.clone()))
            .collect()
    }

    fn recorded(entries: &[(&str, Fact)]) -> Baseline {
        Baseline {
            schema: 1,
            blessed_describe: "test".to_owned(),
            blessed_at_unix: 0,
            facts: facts(entries),
        }
    }

    #[test]
    fn every_fact_kind_round_trips_through_toml() -> Result<(), TestError> {
        let baseline = recorded(&[
            ("count", Fact::Int(24)),
            ("name", Fact::Text("box".to_owned())),
            (
                "angle",
                Fact::Float {
                    value: 45.0,
                    tolerance: 0.5,
                },
            ),
            (
                "at",
                Fact::Vec2 {
                    x: 1.0,
                    y: 2.0,
                    tolerance: 0.1,
                },
            ),
            (
                "extent",
                Fact::Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    tolerance: 0.1,
                },
            ),
        ]);
        let text = toml::to_string_pretty(&baseline)?;
        let back: Baseline = toml::from_str(&text)?;
        assert_eq!(back, baseline);
        Ok(())
    }

    #[test]
    fn a_fact_within_tolerance_is_not_drift_and_one_outside_is() {
        let angle = |value: f64| Fact::Float {
            value,
            tolerance: 0.5,
        };
        let base = recorded(&[("angle", angle(45.0))]);
        assert!(compare(&base, &facts(&[("angle", angle(45.4))])).is_empty());
        let drifts = compare(&base, &facts(&[("angle", angle(46.0))]));
        assert_eq!(drifts.len(), 1);
        assert_eq!(
            drifts.first().map(|drift| drift.key.as_str()),
            Some("angle")
        );
    }

    #[test]
    fn an_appearing_or_vanishing_fact_is_drift() {
        let base = recorded(&[("count", Fact::Int(24))]);
        let drifts = compare(&base, &facts(&[("other", Fact::Int(1))]));
        let keys: Vec<&str> = drifts.iter().map(|drift| drift.key.as_str()).collect();
        assert_eq!(keys, vec!["count", "other"]);
    }

    #[test]
    fn a_fact_that_changed_kind_is_drift() {
        let base = recorded(&[("count", Fact::Int(24))]);
        assert_eq!(
            compare(&base, &facts(&[("count", Fact::Text("24".to_owned()))])).len(),
            1
        );
    }

    #[test]
    fn a_missing_file_names_the_bless_command() -> Result<(), TestError> {
        if std::env::var_os(super::BLESS_ENV).is_some() {
            // Blessing writes rather than compares, and this path is deliberately
            // unwritable, so under a bless run there is no missing-file error to
            // assert — only a failed write.
            return Ok(());
        }
        let path = PathBuf::from("/nonexistent/sl-viewer-testkit/baseline.toml");
        let mut current = Facts::new();
        current.int("count", 1);
        let Err(error @ BaselineError::Missing { .. }) = super::check(&path, current) else {
            return Err("expected a missing-baseline error".into());
        };
        let message = error.to_string();
        assert!(message.contains(super::BLESS_ENV), "{message}");
        Ok(())
    }

    /// The builder writes exactly the facts it was handed, sorted.
    #[test]
    fn the_builder_records_every_kind() {
        let mut built = Facts::new();
        built
            .int("count", 24)
            .text("name", "box")
            .float("angle", 45.0, 0.5)
            .vec2("at", bevy::math::Vec2::new(1.0, 2.0), 0.1)
            .vec3("extent", bevy::math::Vec3::new(1.0, 2.0, 3.0), 0.1);
        let keys: Vec<String> = built.clone().into_map().into_keys().collect();
        assert_eq!(keys, vec!["angle", "at", "count", "extent", "name"]);
        assert_eq!(built.len(), 5);
        assert!(!built.is_empty());
    }

    /// The compile-time workspace root really is the workspace root.
    ///
    /// [`super::WORKSPACE_ROOT`] is this crate's manifest directory with a `..`
    /// on the end, which holds only while this crate is a **direct member** of
    /// the workspace. Move it into a subdirectory and every baseline path
    /// silently points somewhere else, which would read as "every baseline is
    /// missing" — so the assumption is a test, not a comment.
    #[test]
    fn the_resolved_workspace_root_is_the_workspace_root() -> Result<(), TestError> {
        let manifest = PathBuf::from(super::WORKSPACE_ROOT).join("Cargo.toml");
        let text = fs_err::read_to_string(&manifest)?;
        assert!(
            text.contains("[workspace]"),
            "{} is not the workspace manifest",
            manifest.display()
        );
        Ok(())
    }

    /// A subject's path is `baselines/<crate>/<tier>/<id>.toml`.
    #[test]
    fn a_subject_path_is_crate_then_tier_then_id() {
        let path = super::subject_path("sl-client-bevy-viewer", "render", "prim-box");
        let tail: PathBuf = path
            .components()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(
            tail,
            PathBuf::from("baselines/sl-client-bevy-viewer/render/prim-box.toml")
        );
    }

    /// The crate, the tier and the id the committed self-test fixture is
    /// recorded under.
    const SELF_TEST: (&str, &str, &str) = ("sl-viewer-testkit", "self-test", "known-subject");

    /// The facts the committed self-test fixture records.
    ///
    /// Constants rather than a measurement, because the subject under test here
    /// is the *mechanism* — path, read, compare — and a fixture that measured
    /// something real would fail for the measurement's reasons instead.
    fn self_test_facts() -> Facts {
        let mut facts = Facts::new();
        facts
            .int("count", 24)
            .text("name", "box")
            .float("angle", 45.0, 0.5)
            .vec3("extent", bevy::math::Vec3::new(1.0, 2.0, 3.0), 0.01);
        facts
    }

    /// **The whole chain, against a committed file.** The layout resolves, the
    /// file parses, and the facts match.
    #[test]
    fn the_committed_self_test_fixture_still_matches() -> Result<(), TestError> {
        let (krate, tier, id) = SELF_TEST;
        super::check_subject(krate, tier, id, self_test_facts())?;
        Ok(())
    }

    /// A drifted fact against the committed file is reported, and the message
    /// names the bless command.
    #[test]
    fn a_drift_against_the_committed_fixture_is_reported() -> Result<(), TestError> {
        if std::env::var_os(super::BLESS_ENV).is_some() {
            // Blessing rewrites rather than compares, so there is nothing to
            // report — and rewriting the fixture with the *drifted* facts would
            // corrupt it for every other test.
            return Ok(());
        }
        let (krate, tier, id) = SELF_TEST;
        let mut drifted = self_test_facts();
        drifted.int("count", 25);
        let Err(error @ BaselineError::Drifted { .. }) =
            super::check_subject(krate, tier, id, drifted)
        else {
            return Err("a moved count must be reported as drift".into());
        };
        let message = error.to_string();
        assert!(message.contains("count"), "{message}");
        assert!(message.contains(super::BLESS_ENV), "{message}");
        Ok(())
    }

    /// A file whose subject is gone is reported, and a known one is not.
    #[test]
    fn a_recording_for_a_vanished_subject_is_an_orphan() -> Result<(), TestError> {
        let (krate, tier, id) = SELF_TEST;
        assert_eq!(
            super::orphans(krate, tier, &[])?,
            vec![id.to_owned()],
            "a committed file for a subject nobody claims is an orphan"
        );
        assert_eq!(
            super::orphans(krate, tier, &[id])?,
            Vec::<String>::new(),
            "the subject is claimed, so its file is not an orphan"
        );
        assert_eq!(
            super::orphans(krate, "no-such-tier", &[])?,
            Vec::<String>::new(),
            "a tier that has never recorded anything has no orphans"
        );
        Ok(())
    }
}
