//! Pure status classification and performance-delta computation for the
//! `sl-conformance-report` binary.
//!
//! This module does no I/O and no colouring: it turns a loaded [`Record`] (and
//! whether the test applies to the grid) into a [`Cell`] the binary renders. The
//! delta logic refuses to judge a metric as better/worse unless it is complete
//! in both the newest and previous runs.
//!
//! It also lines a live grid's newest run up against the newest run of the fake
//! flavour imitating it ([`divergences`]), which is how the reporter shows a
//! case's live and fake answers side by side.

use crate::record::{MetricValue, Outcome, Record, Run};

/// The percentage change below which a metric is treated as unchanged, to avoid
/// noise from tiny run-to-run variation.
pub const UNCHANGED_THRESHOLD_PERCENT: f64 = 2.0;

/// The status of one `(test, grid)` cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellStatus {
    /// The test does not apply to this grid.
    NotApplicable,
    /// The test has never been run on this grid.
    NeverRan,
    /// The newest run passed.
    Pass,
    /// The newest run failed.
    Fail,
}

/// How the newest run's commit relates to the current checkout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// The newest run was recorded at the current commit (ignoring dirtiness).
    Current,
    /// The newest run was recorded at an older commit.
    Stale,
    /// The current commit is unknown (git unavailable), so freshness cannot be
    /// determined.
    Unknown,
}

/// How a metric's latest value compares to the previous run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judgement {
    /// No previous value to compare against.
    New,
    /// Within the unchanged threshold.
    Unchanged,
    /// Comparable and an improvement.
    Better,
    /// Comparable and a regression.
    Worse,
    /// Numerically changed but not judged (no direction, or not comparable).
    Neutral,
}

/// One metric's latest-vs-previous comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricDelta {
    /// The metric name.
    pub key: String,
    /// The previous run's numeric value, if any.
    pub old: Option<f64>,
    /// The newest run's numeric value.
    pub new: f64,
    /// The percentage change from `old` to `new`, when both are present and
    /// `old` is non-zero.
    pub percent: Option<f64>,
    /// Whether the metric was complete in both runs (and so may be judged).
    pub comparable: bool,
    /// The improved/worse/unchanged judgement.
    pub judgement: Judgement,
}

/// The fully classified state of one `(test, grid)` cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    /// The pass/fail/never/n-a status.
    pub status: CellStatus,
    /// Whether the newest run was recorded against a dirty tree.
    pub dirty: bool,
    /// Whether the newest run was partial.
    pub partial: bool,
    /// The newest run's completeness note, if any.
    pub note: Option<String>,
    /// The newest run's recorded describe (with any `-dirty` suffix), if any.
    pub recorded_describe: Option<String>,
    /// Whether the newest run is at the current commit, an older one, or
    /// unknown.
    pub freshness: Freshness,
    /// The per-metric deltas (newest vs previous), sorted by key.
    pub deltas: Vec<MetricDelta>,
}

/// The base describe with any `-dirty` suffix removed, for commit comparison.
fn describe_base(describe: &str) -> &str {
    describe.strip_suffix("-dirty").unwrap_or(describe)
}

/// Decide a run's freshness against the current checkout, given how many
/// behaviour-changing commits lie between the recorded commit and `HEAD`.
///
/// A run is [`Current`](Freshness::Current) when it was recorded at the current
/// commit, or when no behaviour-changing commit has landed since
/// (`behavioural_commits_behind == Some(0)`) — mirroring the dirty rule, which
/// also ignores record/doc changes. It is [`Stale`](Freshness::Stale) when one
/// or more behavioural commits have landed (or the recorded commit is no longer
/// in history), and [`Unknown`](Freshness::Unknown) when the current commit
/// cannot be determined.
#[must_use]
pub fn freshness_of(
    recorded_describe: &str,
    current_describe: Option<&str>,
    behavioural_commits_behind: Option<u32>,
) -> Freshness {
    let Some(current) = current_describe else {
        return Freshness::Unknown;
    };
    if describe_base(recorded_describe) == describe_base(current) {
        return Freshness::Current;
    }
    match behavioural_commits_behind {
        Some(0) => Freshness::Current,
        _ => Freshness::Stale,
    }
}

/// Compute the percentage change from `old` to `new`, or `None` when `old` is
/// zero (an undefined ratio).
fn percent_change(old: f64, new: f64) -> Option<f64> {
    if old == 0.0 {
        return None;
    }
    Some((new - old) / old * 100.0)
}

/// Judge a comparable metric given its percentage change and direction hint.
fn judge(percent: Option<f64>, lower_is_better: Option<bool>) -> Judgement {
    let Some(percent) = percent else {
        return Judgement::Neutral;
    };
    if percent.abs() < UNCHANGED_THRESHOLD_PERCENT {
        return Judgement::Unchanged;
    }
    match lower_is_better {
        Some(true) => {
            if percent < 0.0 {
                Judgement::Better
            } else {
                Judgement::Worse
            }
        }
        Some(false) => {
            if percent > 0.0 {
                Judgement::Better
            } else {
                Judgement::Worse
            }
        }
        None => Judgement::Neutral,
    }
}

/// Build the metric deltas for `newest` against the optional `previous` run.
fn deltas(newest: &Run, previous: Option<&Run>) -> Vec<MetricDelta> {
    let mut result = Vec::new();
    for (key, value) in &newest.metrics {
        let Some(new) = value.as_f64() else {
            continue;
        };
        let old = previous
            .and_then(|run| run.metrics.get(key))
            .and_then(crate::record::MetricValue::as_f64);
        let comparable = match previous {
            Some(previous_run) => {
                old.is_some()
                    && newest.metric_is_complete(key)
                    && previous_run.metric_is_complete(key)
            }
            None => false,
        };
        let (percent, judgement) = match old {
            None => (None, Judgement::New),
            Some(old_value) => {
                let percent = percent_change(old_value, new);
                if comparable {
                    let lower_is_better = newest
                        .metric_meta
                        .get(key)
                        .and_then(|meta| meta.lower_is_better);
                    (percent, judge(percent, lower_is_better))
                } else {
                    (percent, Judgement::Neutral)
                }
            }
        };
        result.push(MetricDelta {
            key: key.clone(),
            old,
            new,
            percent,
            comparable,
            judgement,
        });
    }
    result
}

/// Classify a `(test, grid)` cell from whether the test applies and its loaded
/// record.
#[must_use]
pub fn classify(applicable: bool, record: Option<&Record>, freshness: Freshness) -> Cell {
    if !applicable {
        return Cell {
            status: CellStatus::NotApplicable,
            dirty: false,
            partial: false,
            note: None,
            recorded_describe: None,
            freshness: Freshness::Unknown,
            deltas: Vec::new(),
        };
    }
    let Some(newest) = record.and_then(Record::newest) else {
        return Cell {
            status: CellStatus::NeverRan,
            dirty: false,
            partial: false,
            note: None,
            recorded_describe: None,
            freshness: Freshness::Unknown,
            deltas: Vec::new(),
        };
    };
    let status = match newest.outcome {
        Outcome::Pass => CellStatus::Pass,
        Outcome::Fail => CellStatus::Fail,
    };
    let previous = record.and_then(Record::previous);
    Cell {
        status,
        dirty: newest.dirty,
        partial: matches!(newest.completeness, crate::record::Completeness::Partial),
        note: newest.completeness_note.clone(),
        recorded_describe: Some(newest.behavior_describe.clone()),
        freshness,
        deltas: deltas(newest, previous),
    }
}

/// One metric on which a fake flavour's newest run and the newest run on the
/// live grid it imitates disagree.
#[derive(Clone, Debug, PartialEq)]
pub struct Divergence {
    /// The metric name.
    pub key: String,
    /// The live grid's value, or `None` when it recorded none.
    pub live: Option<MetricValue>,
    /// The fake grid's value, or `None` when it recorded none.
    pub fake: Option<MetricValue>,
}

/// Whether `key` is a timing in either run: a duration measures the machine and
/// the network as much as the grid, so a fake grid answering faster than a live
/// one is not a disagreement about behaviour.
fn is_timing(key: &str, live: &Run, fake: &Run) -> bool {
    [live, fake].iter().any(|run| {
        run.metric_meta
            .get(key)
            .is_some_and(|meta| meta.lower_is_better.is_some())
    })
}

/// Whether `value` is an id — a UUID, or a comma-separated list of them. Ids
/// are minted by whichever grid created the thing, so a live grid's and a fake
/// grid's never match and their differing says nothing.
fn is_identifier(value: Option<&MetricValue>) -> bool {
    match value {
        Some(MetricValue::Text(text)) => text
            .split(',')
            .all(|piece| sl_client_tokio::Uuid::parse_str(piece.trim()).is_ok()),
        Some(MetricValue::Bool(_) | MetricValue::Int(_) | MetricValue::Float(_)) | None => false,
    }
}

/// The metrics a fake run (`fake`) and the live run it imitates (`live`)
/// record differently — present in one only, or with different values —
/// sorted by key. Two kinds are left out: timings, since a duration measures
/// the machine and the network as much as the grid, and ids on both sides
/// (`is_identifier`), since each grid mints its own.
///
/// This is a listing, not a verdict. Some metrics legitimately differ — a count
/// of the objects in a region describes the fixture, not the grid — and the
/// ones that must not are held to the live answer inside the case itself, by a
/// [`Measured`](crate::measured::Measured) check that fails the run. What this
/// adds is the *unasserted* rest: every recorded field a fake grid answers
/// differently, which is where the next gridspec measurement starts.
#[must_use]
pub fn divergences(live: &Run, fake: &Run) -> Vec<Divergence> {
    let mut keys: Vec<&String> = live.metrics.keys().chain(fake.metrics.keys()).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter(|key| !is_timing(key, live, fake))
        .filter_map(|key| {
            let live_value = live.metrics.get(key);
            let fake_value = fake.metrics.get(key);
            let both_ids = is_identifier(live_value) && is_identifier(fake_value);
            (live_value != fake_value && !both_ids).then(|| Divergence {
                key: key.clone(),
                live: live_value.cloned(),
                fake: fake_value.cloned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        CellStatus, Divergence, Freshness, Judgement, classify, divergences, freshness_of,
    };
    use crate::record::{Completeness, MetricMeta, MetricValue, Outcome, Record, Run};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    /// A run with one timing metric at `secs`, optionally partial.
    fn run(describe: &str, secs: f64, completeness: Completeness) -> Run {
        let mut metrics = BTreeMap::new();
        metrics.insert("op_secs".to_owned(), MetricValue::Float(secs));
        let mut metric_meta = BTreeMap::new();
        metric_meta.insert(
            "op_secs".to_owned(),
            MetricMeta {
                lower_is_better: Some(true),
                complete: None,
            },
        );
        Run {
            behavior_describe: describe.to_owned(),
            dirty: false,
            outcome: Outcome::Pass,
            completeness,
            completeness_note: None,
            recorded_at: "2026-06-28T00:00:00Z".to_owned(),
            sl_conformance_version: "0.1.0".to_owned(),
            metrics,
            metric_meta,
        }
    }

    /// An inapplicable test is n/a; a missing record is never-ran.
    #[test]
    fn applicability_and_missing() {
        assert_eq!(
            classify(false, None, Freshness::Unknown).status,
            CellStatus::NotApplicable
        );
        assert_eq!(
            classify(true, None, Freshness::Unknown).status,
            CellStatus::NeverRan
        );
    }

    /// Freshness compares the recorded commit to the current one (ignoring
    /// `-dirty`), counting only behaviour-changing commits since.
    #[test]
    fn freshness_rules() {
        // Same commit (with or without -dirty) is current.
        assert_eq!(
            freshness_of("v0.1.0-2-gbbbbbb", Some("v0.1.0-2-gbbbbbb"), None),
            Freshness::Current
        );
        assert_eq!(
            freshness_of("v0.1.0-2-gbbbbbb", Some("v0.1.0-2-gbbbbbb-dirty"), None),
            Freshness::Current
        );
        // Different commit, but zero behaviour-changing commits since: current.
        assert_eq!(
            freshness_of("v0.1.0-2-gbbbbbb", Some("v0.1.0-3-gccccccc"), Some(0)),
            Freshness::Current
        );
        // Different commit with behaviour-changing commits since: stale.
        assert_eq!(
            freshness_of("v0.1.0-2-gbbbbbb", Some("v0.1.0-7-gddddddd"), Some(2)),
            Freshness::Stale
        );
        // Different commit, commit not in history: stale (conservative).
        assert_eq!(
            freshness_of("v0.1.0-2-gbbbbbb", Some("v0.1.0-7-gddddddd"), None),
            Freshness::Stale
        );
        // Current commit unknown: unknown.
        assert_eq!(
            freshness_of("v0.1.0-2-gbbbbbb", None, None),
            Freshness::Unknown
        );
    }

    /// The single delta of a one-metric record, or an error string.
    fn only_delta(cell: &super::Cell) -> Result<&super::MetricDelta, String> {
        cell.deltas
            .first()
            .ok_or_else(|| "expected exactly one delta".to_owned())
    }

    /// A faster second run (lower-is-better) is judged Better.
    #[test]
    fn faster_run_is_better() -> Result<(), String> {
        let record = Record {
            test: "t".to_owned(),
            grid: "opensim".to_owned(),
            runs: vec![
                run("a", 4.80, Completeness::Complete),
                run("b", 4.20, Completeness::Complete),
            ],
        };
        let cell = classify(true, Some(&record), Freshness::Current);
        assert_eq!(cell.status, CellStatus::Pass);
        let delta = only_delta(&cell)?;
        assert_eq!(delta.judgement, Judgement::Better);
        assert!(delta.comparable);
        Ok(())
    }

    /// A partial newest run is not comparable, so no better/worse judgement.
    #[test]
    fn partial_run_not_comparable() -> Result<(), String> {
        let record = Record {
            test: "t".to_owned(),
            grid: "opensim".to_owned(),
            runs: vec![
                run("a", 4.80, Completeness::Complete),
                run("b", 4.20, Completeness::Partial),
            ],
        };
        let cell = classify(true, Some(&record), Freshness::Current);
        assert!(cell.partial);
        let delta = only_delta(&cell)?;
        assert!(!delta.comparable);
        assert_eq!(delta.judgement, Judgement::Neutral);
        Ok(())
    }

    /// A tiny change is Unchanged.
    #[test]
    fn small_change_unchanged() -> Result<(), String> {
        let record = Record {
            test: "t".to_owned(),
            grid: "opensim".to_owned(),
            runs: vec![
                run("a", 4.00, Completeness::Complete),
                run("b", 4.01, Completeness::Complete),
            ],
        };
        let cell = classify(true, Some(&record), Freshness::Current);
        let delta = only_delta(&cell)?;
        assert_eq!(delta.judgement, Judgement::Unchanged);
        Ok(())
    }

    /// A live run and its fake twin are compared on everything but timings and
    /// ids: a matching field is silent, a differing or one-sided one is listed.
    #[test]
    fn divergences_skip_timings_and_list_what_differs() {
        let mut live = run("a", 4.0, Completeness::Complete);
        let mut fake = run("a", 0.1, Completeness::Complete);
        for (key, live_value, fake_value) in [
            ("same", MetricValue::Int(3), MetricValue::Int(3)),
            (
                "announcement",
                MetricValue::Text("bulk".to_owned()),
                MetricValue::Text("legacy".to_owned()),
            ),
            (
                "rezzed_objects",
                MetricValue::Text(
                    "91850a38-81b0-4d3a-89a9-3a5a9760b0dd,cf065737-1298-469c-952c-51a3a30176c3"
                        .to_owned(),
                ),
                MetricValue::Text("2b694c16-1f7a-f999-c323-3d74269d4a2a".to_owned()),
            ),
        ] {
            live.metrics.insert(key.to_owned(), live_value);
            fake.metrics.insert(key.to_owned(), fake_value);
        }
        live.metrics
            .insert("live_only".to_owned(), MetricValue::Bool(true));
        assert_eq!(
            divergences(&live, &fake),
            vec![
                Divergence {
                    key: "announcement".to_owned(),
                    live: Some(MetricValue::Text("bulk".to_owned())),
                    fake: Some(MetricValue::Text("legacy".to_owned())),
                },
                Divergence {
                    key: "live_only".to_owned(),
                    live: Some(MetricValue::Bool(true)),
                    fake: None,
                },
            ]
        );
    }
}
