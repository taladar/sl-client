//! The committed per-`(test, grid)` record and its bounded run history.
//!
//! Each test/grid pair has one TOML file under `records/<grid>/<test>.toml`. The
//! file keeps a bounded history of recent [`Run`]s (newest last) so the reporter
//! can compare the latest run against the previous one; git history preserves
//! anything trimmed past the limit.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::grid::Grid;

/// How many runs to retain in a record before trimming the oldest.
pub const HISTORY_LIMIT: usize = 10;

/// A single metric value written by a test.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    /// A boolean flag.
    Bool(bool),
    /// An integer count.
    Int(i64),
    /// A floating-point measurement (e.g. a duration in seconds).
    Float(f64),
    /// A free-form text value.
    Text(String),
}

impl MetricValue {
    /// The value as `f64` when it is numeric, for delta computation.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Int(value) => Some(f64::from(i32::try_from(value).ok()?)),
            Self::Float(value) => Some(value),
            Self::Bool(_) | Self::Text(_) => None,
        }
    }
}

impl From<f64> for MetricValue {
    /// Wrap a float measurement.
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<i64> for MetricValue {
    /// Wrap an integer count.
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<u32> for MetricValue {
    /// Wrap an unsigned count, widened losslessly to `i64`.
    fn from(value: u32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i32> for MetricValue {
    /// Wrap a signed count, widened to `i64`.
    fn from(value: i32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<bool> for MetricValue {
    /// Wrap a boolean flag.
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for MetricValue {
    /// Wrap an owned text value.
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for MetricValue {
    /// Wrap a borrowed text value.
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// Per-metric metadata that guides the reporter without it having to guess.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MetricMeta {
    /// Whether a lower value is an improvement (set for timing metrics); `None`
    /// means the metric has no inherent good/bad direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lower_is_better: Option<bool>,
    /// Whether this metric covered the full dataset; `Some(false)` marks it
    /// partial even when the run overall is complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete: Option<bool>,
}

/// Whether a test run reflected a complete dataset or a truncated/aborted one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Completeness {
    /// The run exercised the full dataset.
    #[default]
    Complete,
    /// The run aborted early or truncated; its counts are not comparable to a
    /// complete run's.
    Partial,
}

/// The pass/fail outcome of a test run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// The test passed.
    Pass,
    /// The test failed.
    Fail,
}

/// One recorded run of a test against a grid.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Run {
    /// The behaviour-aware describe at which the run happened.
    pub behavior_describe: String,
    /// Whether the behaviour-relevant tree was dirty at run time.
    pub dirty: bool,
    /// The pass/fail outcome.
    pub outcome: Outcome,
    /// Whether the run's dataset was complete or partial.
    #[serde(default)]
    pub completeness: Completeness,
    /// An optional note explaining a partial run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completeness_note: Option<String>,
    /// When the run was recorded (RFC 3339, UTC).
    pub recorded_at: String,
    /// The version of this harness that produced the run.
    pub sl_conformance_version: String,
    /// The metrics the test wrote, keyed by name.
    #[serde(default)]
    pub metrics: BTreeMap<String, MetricValue>,
    /// Per-metric metadata, keyed by the same metric names.
    #[serde(default)]
    pub metric_meta: BTreeMap<String, MetricMeta>,
}

impl Run {
    /// Whether a given metric is complete in this run: the run must be complete
    /// and the metric must not be individually marked partial.
    #[must_use]
    pub fn metric_is_complete(&self, key: &str) -> bool {
        if matches!(self.completeness, Completeness::Partial) {
            return false;
        }
        !matches!(
            self.metric_meta.get(key).and_then(|meta| meta.complete),
            Some(false)
        )
    }
}

/// A whole record file: the run history for one test on one grid.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Record {
    /// The test name (record file stem).
    pub test: String,
    /// The grid directory name this record belongs to.
    pub grid: String,
    /// The bounded run history, oldest first, newest last.
    #[serde(default, rename = "run")]
    pub runs: Vec<Run>,
}

impl Record {
    /// The on-disk path of the record for `test` on `grid` under `records_dir`.
    #[must_use]
    pub fn path(records_dir: &Path, grid: Grid, test: &str) -> PathBuf {
        records_dir
            .join(grid.dir_name())
            .join(format!("{test}.toml"))
    }

    /// Load a record from disk, returning `Ok(None)` when the file does not
    /// exist yet.
    ///
    /// # Errors
    ///
    /// Returns [`RecordError::Io`] on a read error other than not-found, or
    /// [`RecordError::Parse`] if the file is not a valid record.
    pub fn load(path: &Path) -> Result<Option<Self>, RecordError> {
        match fs_err::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map(Some)
                .map_err(|error| RecordError::Parse {
                    path: path.display().to_string(),
                    message: error.to_string(),
                }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(RecordError::Io(error.to_string())),
        }
    }

    /// Append `run` to the record at `records_dir/<grid>/<test>.toml`, creating
    /// or extending it, trimming the history to [`HISTORY_LIMIT`], and writing
    /// it back.
    ///
    /// # Errors
    ///
    /// Returns a [`RecordError`] if the existing record cannot be read,
    /// serialized, or written.
    pub fn append(records_dir: &Path, grid: Grid, test: &str, run: Run) -> Result<(), RecordError> {
        let path = Self::path(records_dir, grid, test);
        let mut record = Self::load(&path)?.unwrap_or_else(|| Self {
            test: test.to_owned(),
            grid: grid.dir_name().to_owned(),
            runs: Vec::new(),
        });
        record.runs.push(run);
        let excess = record.runs.len().saturating_sub(HISTORY_LIMIT);
        if excess > 0 {
            let _trimmed = record.runs.drain(0..excess);
        }
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent).map_err(|error| RecordError::Io(error.to_string()))?;
        }
        let text = toml::to_string_pretty(&record)
            .map_err(|error| RecordError::Serialize(error.to_string()))?;
        fs_err::write(&path, text).map_err(|error| RecordError::Io(error.to_string()))?;
        Ok(())
    }

    /// The most recent run, if any.
    #[must_use]
    pub fn newest(&self) -> Option<&Run> {
        self.runs.last()
    }

    /// The run immediately before the most recent one, if any.
    #[must_use]
    pub fn previous(&self) -> Option<&Run> {
        let len = self.runs.len();
        len.checked_sub(2).and_then(|index| self.runs.get(index))
    }
}

/// An error loading, serializing, or writing a record.
#[expect(
    clippy::module_name_repetitions,
    reason = "`RecordError` reads best as this module's public error name"
)]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// The record file could not be read or written.
    #[error("record I/O error: {0}")]
    Io(String),
    /// The record file was not valid TOML or did not match the schema.
    #[error("could not parse record {path}: {message}")]
    Parse {
        /// The record path that failed to parse.
        path: String,
        /// The parse error message.
        message: String,
    },
    /// The record could not be serialized to TOML.
    #[error("could not serialize record: {0}")]
    Serialize(String),
}

#[cfg(test)]
mod tests {
    use super::{Completeness, MetricMeta, MetricValue, Outcome, Record, Run};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    /// Build a minimal run with one timing metric.
    fn sample_run(describe: &str, secs: f64) -> Run {
        let mut metrics = BTreeMap::new();
        metrics.insert("inventory_fetch_secs".to_owned(), MetricValue::Float(secs));
        let mut metric_meta = BTreeMap::new();
        metric_meta.insert(
            "inventory_fetch_secs".to_owned(),
            MetricMeta {
                lower_is_better: Some(true),
                complete: None,
            },
        );
        Run {
            behavior_describe: describe.to_owned(),
            dirty: false,
            outcome: Outcome::Pass,
            completeness: Completeness::Complete,
            completeness_note: None,
            recorded_at: "2026-06-28T19:42:11Z".to_owned(),
            sl_conformance_version: "0.1.0".to_owned(),
            metrics,
            metric_meta,
        }
    }

    /// A record round-trips through TOML preserving its run history.
    #[test]
    fn record_round_trips() -> Result<(), String> {
        let record = Record {
            test: "inventory-fetch".to_owned(),
            grid: "opensim".to_owned(),
            runs: vec![
                sample_run("v0.1.0-1-gaaa", 4.8),
                sample_run("v0.1.0-2-gbbb", 4.2),
            ],
        };
        let text = toml::to_string_pretty(&record).map_err(|error| error.to_string())?;
        let parsed: Record = toml::from_str(&text).map_err(|error| error.to_string())?;
        assert_eq!(parsed.runs.len(), 2);
        assert_eq!(
            parsed.newest().map(|run| run.behavior_describe.as_str()),
            Some("v0.1.0-2-gbbb")
        );
        assert_eq!(
            parsed.previous().map(|run| run.behavior_describe.as_str()),
            Some("v0.1.0-1-gaaa")
        );
        Ok(())
    }

    /// A partial run reports its metric as incomplete.
    #[test]
    fn partial_run_metric_incomplete() {
        let mut run = sample_run("v0.1.0-1-gaaa", 4.8);
        run.completeness = Completeness::Partial;
        assert!(!run.metric_is_complete("inventory_fetch_secs"));
    }

    /// A numeric metric exposes an `f64`; text does not.
    #[test]
    fn metric_as_f64() {
        assert_eq!(MetricValue::Int(7).as_f64(), Some(7.0));
        assert_eq!(MetricValue::Float(1.5).as_f64(), Some(1.5));
        assert_eq!(MetricValue::Text("x".to_owned()).as_f64(), None);
    }
}
