//! The metrics collector a test writes timings and counts into.
//!
//! A test receives a `&mut Metrics` (via its [`TestContext`](crate::context))
//! and records named measurements; the runner folds these into the [`Run`] it
//! appends to the record. Timing via [`Metrics::time`] also marks the metric as
//! "lower is better" so the reporter colours its trend correctly.
//!
//! [`Run`]: crate::record::Run

use std::collections::BTreeMap;

use crate::record::{MetricMeta, MetricValue};

/// A collector of named metric values and their per-metric metadata.
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    /// The recorded values, keyed by metric name.
    values: BTreeMap<String, MetricValue>,
    /// Metadata for metrics that have a direction or partial flag.
    meta: BTreeMap<String, MetricMeta>,
}

impl Metrics {
    /// An empty collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a metric value with no inherent direction (neutral).
    pub fn set(&mut self, key: &str, value: impl Into<MetricValue>) {
        let _previous = self.values.insert(key.to_owned(), value.into());
    }

    /// Record a metric value and mark it as covering only a partial dataset, so
    /// the reporter will not compare it against a complete run's value.
    pub fn set_partial(&mut self, key: &str, value: impl Into<MetricValue>) {
        let _previous = self.values.insert(key.to_owned(), value.into());
        self.meta.entry(key.to_owned()).or_default().complete = Some(false);
    }

    /// Record a pre-measured duration in seconds under `key`, marking it "lower
    /// is better". Use this when the work cannot be wrapped in [`Metrics::time`]
    /// (e.g. it borrows the same context as the collector).
    pub fn set_timing(&mut self, key: &str, seconds: f64) {
        let _previous = self
            .values
            .insert(key.to_owned(), MetricValue::Float(seconds));
        self.meta.entry(key.to_owned()).or_default().lower_is_better = Some(true);
    }

    /// Time an async operation, store its duration in seconds under `key`
    /// (marked "lower is better"), and return the operation's output.
    pub async fn time<T, F>(&mut self, key: &str, fut: F) -> T
    where
        F: core::future::Future<Output = T>,
    {
        let start = std::time::Instant::now();
        let output = fut.await;
        let seconds = start.elapsed().as_secs_f64();
        let _previous = self
            .values
            .insert(key.to_owned(), MetricValue::Float(seconds));
        self.meta.entry(key.to_owned()).or_default().lower_is_better = Some(true);
        output
    }

    /// Consume the collector, yielding the values and (non-empty) metadata maps
    /// for storage in a [`Run`](crate::record::Run).
    #[must_use]
    pub fn into_parts(self) -> (BTreeMap<String, MetricValue>, BTreeMap<String, MetricMeta>) {
        (self.values, self.meta)
    }
}

#[cfg(test)]
mod tests {
    use super::Metrics;
    use crate::record::MetricValue;
    use pretty_assertions::assert_eq;

    /// `set` records a neutral value with no metadata entry.
    #[test]
    fn set_is_neutral() {
        let mut metrics = Metrics::new();
        metrics.set("folders", 312_i64);
        let (values, meta) = metrics.into_parts();
        assert_eq!(values.get("folders"), Some(&MetricValue::Int(312)));
        assert!(meta.is_empty());
    }

    /// `set_partial` marks the metric incomplete.
    #[test]
    fn set_partial_marks_incomplete() {
        let mut metrics = Metrics::new();
        metrics.set_partial("folders", 312_i64);
        let (_values, meta) = metrics.into_parts();
        assert_eq!(meta.get("folders").and_then(|m| m.complete), Some(false));
    }

    /// `time` stores a float and marks the metric lower-is-better.
    #[tokio::test]
    async fn time_marks_lower_is_better() {
        let mut metrics = Metrics::new();
        let result = metrics.time("op_secs", async { 7_i32 }).await;
        assert_eq!(result, 7);
        let (values, meta) = metrics.into_parts();
        assert!(matches!(
            values.get("op_secs"),
            Some(&MetricValue::Float(_))
        ));
        assert_eq!(
            meta.get("op_secs").and_then(|m| m.lower_is_better),
            Some(true)
        );
    }
}
