//! The differences between two grids' runs of a probe that are known and
//! accepted: `sl-lsl-runtime/probes/accepted.toml`, committed beside the
//! results, so a divergence the fake grid shows against the grid it imitates
//! is a finding once and a record after that.
//!
//! Each entry names the probe, the two result sets it is about (`between`),
//! which of the comparison's differences it covers — a record by name, the
//! fields that pick it out, and the field that differs — and why. With a
//! `task` it is a known defect a roadmap task will fix; without one it is a
//! **deliberate** difference: the fake grid answers otherwise on purpose
//! (its physics is kinematic, it prints a float where the grid printed a
//! double).
//! An entry with no `record` covers the whole pair: a probe the fake grid
//! cannot run yet.
//!
//! `sl-conformance compare-probes` reports what an entry covers as
//! `accepted`, apart from the findings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sl_lsl_runtime::probe::compare::Difference;

/// The file's name, in the corpus root.
pub const FILE_NAME: &str = "accepted.toml";

/// The file as it is written.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedFile {
    /// The entries, in the file's order.
    #[serde(default, rename = "difference")]
    differences: Vec<Accepted>,
}

/// One accepted difference, or a set of them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accepted {
    /// The probe's id.
    pub probe: String,
    /// The two result sets (`opensim-yengine`, `fake-opensim`, …), in either
    /// order.
    pub between: [String; 2],
    /// The record the difference is in; every record when absent.
    pub record: Option<String>,
    /// The fields, with their values, that pick the record out.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// The field whose values differ; any difference of the record when
    /// absent, a record only one run printed included.
    pub field: Option<String>,
    /// Why the difference is accepted.
    pub reason: String,
    /// The roadmap task that will remove it, for a known defect; none for a
    /// deliberate difference.
    pub task: Option<String>,
}

impl Accepted {
    /// Whether the entry is about a comparison of `probe` between `left` and
    /// `right`.
    #[must_use]
    pub fn applies(&self, probe: &str, left: &str, right: &str) -> bool {
        let [one, other] = &self.between;
        self.probe == probe && ((one == left && other == right) || (one == right && other == left))
    }

    /// Whether the entry covers the whole comparison: a probe it names
    /// cannot be compared yet, its `end` included.
    #[must_use]
    pub const fn covers_all(&self) -> bool {
        self.record.is_none()
    }

    /// Whether the entry covers `difference`.
    #[must_use]
    pub fn covers(&self, difference: &Difference) -> bool {
        let Some(record) = &self.record else {
            return true;
        };
        match difference {
            Difference::Field {
                record: text,
                field,
                ..
            } => {
                record_name(text) == Some(record.as_str())
                    && self.picks(text)
                    && self.field.as_ref().is_none_or(|wanted| wanted == field)
            }
            Difference::Only { line, .. } => {
                self.field.is_none()
                    && line
                        .split_whitespace()
                        .find_map(|token| token.strip_prefix("rec="))
                        == Some(record.as_str())
                    && self.picks(line)
            }
            Difference::Unreadable { .. } => false,
        }
    }

    /// Whether every field the entry names has its value in `text`.
    fn picks(&self, text: &str) -> bool {
        self.fields.iter().all(|(name, value)| {
            let token = format!("{name}={value}");
            text.split_whitespace().any(|field| field == token)
        })
    }

    /// The entry as a report line says it.
    #[must_use]
    pub fn why(&self) -> String {
        match &self.task {
            Some(task) => format!("{} ({task})", self.reason),
            None => format!("{} (deliberate)", self.reason),
        }
    }
}

/// The record name a `Difference::Field`'s record text starts with.
fn record_name(text: &str) -> Option<&str> {
    text.split_whitespace().next()
}

/// Why the file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The file could not be read.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The file is not what it should be.
    #[error("{path}: {reason}")]
    Format {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
}

/// The accepted differences of the corpus at `root`; none when it has no
/// file.
///
/// # Errors
///
/// [`LoadError`] when the file cannot be read or parsed.
pub fn load(root: &Path) -> Result<Vec<Accepted>, LoadError> {
    let path = root.join(FILE_NAME);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs_err::read_to_string(&path)?;
    let file: AcceptedFile = toml::from_str(&text).map_err(|error| LoadError::Format {
        path: path.clone(),
        reason: error.to_string(),
    })?;
    Ok(file.differences)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Accepted, AcceptedFile};
    use sl_lsl_runtime::probe::compare::{Difference, Side};

    /// An entry for one field of one case.
    fn rotate_value() -> Result<Accepted, String> {
        let file: AcceptedFile = toml::from_str(
            r#"
            [[difference]]
            probe = "math/rotations"
            between = ["opensim-yengine", "fake-opensim"]
            record = "rotate"
            fields = { case = "x-by-1234" }
            field = "value"
            reason = "OpenSim normalises first"
            task = "server-lsl-opensim-vector-rotation"
            "#,
        )
        .map_err(|error| error.to_string())?;
        file.differences
            .into_iter()
            .next()
            .ok_or_else(|| "no entry".to_owned())
    }

    #[test]
    fn an_entry_covers_the_field_of_the_record_it_picks_and_nothing_else() -> Result<(), String> {
        let entry = rotate_value()?;
        assert!(entry.applies("math/rotations", "fake-opensim", "opensim-yengine"));
        assert!(!entry.applies("math/rotations", "aditi", "fake-sl"));
        let field = |record: &str, field: &str| Difference::Field {
            record: record.to_owned(),
            field: field.to_owned(),
            left: None,
            right: None,
        };
        assert!(entry.covers(&field("rotate case=x-by-1234", "value")));
        assert!(!entry.covers(&field("rotate case=x-by-zero", "value")));
        assert!(!entry.covers(&field("rotate case=x-by-1234", "other")));
        assert!(!entry.covers(&field("unrotate case=x-by-1234", "value")));
        assert!(!entry.covers(&Difference::Only {
            side: Side::Left,
            line: "PROBE math/rotations rec=rotate case=x-by-1234 value=<1,0,0>".to_owned(),
        }));
        assert_eq!(
            entry.why(),
            "OpenSim normalises first (server-lsl-opensim-vector-rotation)"
        );
        Ok(())
    }

    #[test]
    fn an_entry_without_a_field_covers_a_record_only_one_run_printed() -> Result<(), String> {
        let mut entry = rotate_value()?;
        entry.field = None;
        assert!(entry.covers(&Difference::Only {
            side: Side::Right,
            line: "PROBE math/rotations rec=rotate case=x-by-1234 value=<1,0,0>".to_owned(),
        }));
        entry.record = None;
        assert!(entry.covers_all());
        Ok(())
    }
}
