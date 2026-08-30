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
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The environment variable that rewrites every checked baseline instead of
/// comparing against it.
pub const BLESS_ENV: &str = "SL_VIEWER_BLESS_BASELINES";

/// The file format's version, for a future migration to name.
const SCHEMA: u32 = 1;

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
pub fn check(path: &Path, current: BTreeMap<String, Fact>) -> Result<(), BaselineError> {
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

    use super::{Baseline, BaselineError, Fact, compare};

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
        let path = PathBuf::from("/nonexistent/sl-viewer-testkit/baseline.toml");
        let Err(error @ BaselineError::Missing { .. }) =
            super::check(&path, facts(&[("count", Fact::Int(1))]))
        else {
            return Err("expected a missing-baseline error".into());
        };
        let message = error.to_string();
        assert!(message.contains(super::BLESS_ENV), "{message}");
        Ok(())
    }
}
