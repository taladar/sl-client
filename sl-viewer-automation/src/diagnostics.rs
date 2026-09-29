//! [`LogTally`]: every warning and error the viewer logged, counted, with the
//! recent lines kept — so a test can assert "nothing logged an ERROR" and, when
//! something did, say what.
//!
//! It is a `tracing` layer, installed in the subscriber by whoever installs it
//! (the viewer binary's `init_tracing`, or a test's own scoped subscriber), and
//! read from the world through [`DiagnosticsSource`] — or, when an app names
//! none, the process-wide [`LogTally::global`] the binary installs.
//!
//! The tracing subscriber is process-wide, so the global tally is too: two
//! viewers in one process share it, as they share the log itself.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use bevy::prelude::*;
use sl_automation_proto::{DiagnosticLine, DiagnosticsReadout, LogLevel, LogPage};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// The most recent lines a tally keeps.
pub const RECENT_LINES: usize = 256;

/// What a tally has counted and kept.
#[derive(Debug, Default)]
struct Tally {
    /// Warnings ever logged.
    warnings: u64,
    /// Errors ever logged.
    errors: u64,
    /// The most recent lines, oldest first.
    lines: VecDeque<DiagnosticLine>,
    /// The sequence number the next line gets.
    next: u64,
}

/// A count of the warnings and errors logged, and the most recent of their
/// lines. Clones share one tally.
#[derive(Debug, Clone, Default)]
pub struct LogTally(Arc<Mutex<Tally>>);

impl LogTally {
    /// The process-wide tally the viewer binary installs in its subscriber, and
    /// every app reads unless it names its own ([`DiagnosticsSource`]).
    #[must_use]
    pub fn global() -> &'static Self {
        /// The one process-wide tally.
        static GLOBAL: LazyLock<LogTally> = LazyLock::new(LogTally::default);
        &GLOBAL
    }

    /// The `tracing` layer that feeds this tally.
    #[must_use]
    pub fn layer(&self) -> LogTallyLayer {
        LogTallyLayer(self.clone())
    }

    /// Count a line and keep it.
    fn record(&self, level: LogLevel, target: &str, message: String) {
        let mut tally = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match level {
            LogLevel::Warn => tally.warnings = tally.warnings.saturating_add(1),
            LogLevel::Error => tally.errors = tally.errors.saturating_add(1),
        }
        let seq = tally.next;
        tally.next = seq.saturating_add(1);
        tally.lines.push_back(DiagnosticLine {
            seq,
            level,
            target: target.to_owned(),
            message,
        });
        while tally.lines.len() > RECENT_LINES {
            let _dropped = tally.lines.pop_front();
        }
        drop(tally);
    }

    /// The cursor that reads only what is logged from now on.
    #[must_use]
    pub fn cursor(&self) -> u64 {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).next
    }

    /// The counts, and the lines kept from `cursor` on, oldest first.
    #[must_use]
    pub fn read(&self, cursor: u64) -> DiagnosticsReadout {
        let tally = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let first = tally
            .next
            .saturating_sub(u64::try_from(tally.lines.len()).unwrap_or(u64::MAX));
        DiagnosticsReadout {
            warnings: tally.warnings,
            errors: tally.errors,
            lines: LogPage {
                entries: tally
                    .lines
                    .iter()
                    .filter(|line| line.seq >= cursor)
                    .cloned()
                    .collect(),
                next: tally.next.max(cursor),
                dropped: first.saturating_sub(cursor),
            },
        }
    }
}

/// The `tracing` layer of a [`LogTally`]: it counts every `WARN` and `ERROR`
/// event the subscriber lets through and keeps its line.
#[derive(Debug, Clone)]
pub struct LogTallyLayer(LogTally);

impl<S: Subscriber> Layer<S> for LogTallyLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        let level = match *metadata.level() {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            _quieter => return,
        };
        let mut line = LineWriter::default();
        event.record(&mut line);
        self.0.record(level, metadata.target(), line.text);
    }
}

/// Prints an event's fields as the log does: the message, then `name=value`
/// for each other field.
#[derive(Debug, Default)]
struct LineWriter {
    /// The line so far.
    text: String,
}

impl Visit for LineWriter {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        if field.name() == "message" {
            let _infallible = write!(self.text, "{value:?}");
        } else {
            let _infallible = write!(self.text, "{}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            if !self.text.is_empty() {
                self.text.push(' ');
            }
            self.text.push_str(value);
        } else {
            self.record_debug(field, &value);
        }
    }
}

/// The tally an app's diagnostics probe reads, when it is not the process-wide
/// one — a test's, fed by its own scoped subscriber.
#[derive(Debug, Clone, Default, Resource)]
pub struct DiagnosticsSource(pub LogTally);

/// The warnings and errors logged, and the lines kept from `cursor` on: from
/// the app's [`DiagnosticsSource`], else from [`LogTally::global`].
#[must_use]
pub fn read_diagnostics(world: &World, cursor: u64) -> DiagnosticsReadout {
    match world.get_resource::<DiagnosticsSource>() {
        Some(source) => source.0.read(cursor),
        None => LogTally::global().read(cursor),
    }
}

/// The diagnostics cursor that reads only what is logged from now on, in the
/// tally [`read_diagnostics`] reads.
#[must_use]
pub fn diagnostics_cursor(world: &World) -> u64 {
    match world.get_resource::<DiagnosticsSource>() {
        Some(source) => source.0.cursor(),
        None => LogTally::global().cursor(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_automation_proto::LogLevel;
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::{LogTally, RECENT_LINES};

    #[test]
    fn warnings_and_errors_are_counted_and_kept() {
        let tally = LogTally::default();
        let subscriber = tracing_subscriber::registry().with(tally.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("not counted");
            tracing::warn!(code = 3, "slow");
            tracing::error!("broken");
        });
        let readout = tally.read(0);
        assert_eq!((readout.warnings, readout.errors), (1, 1));
        let lines: Vec<(u64, LogLevel, &str)> = readout
            .lines
            .entries
            .iter()
            .map(|line| (line.seq, line.level, line.message.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (0, LogLevel::Warn, "slow code=3"),
                (1, LogLevel::Error, "broken")
            ]
        );
        assert!(
            readout
                .lines
                .entries
                .iter()
                .all(|line| line.target.contains("diagnostics"))
        );
        let later = tally.read(readout.lines.next);
        assert!(later.lines.entries.is_empty());
        assert_eq!(
            later.errors, 1,
            "the counts are totals, not since the cursor"
        );
    }

    #[test]
    fn old_lines_drop_and_the_reader_is_told() {
        let tally = LogTally::default();
        let subscriber = tracing_subscriber::registry().with(tally.layer());
        tracing::subscriber::with_default(subscriber, || {
            for index in 0..RECENT_LINES + 3 {
                tracing::warn!(index, "again");
            }
        });
        let readout = tally.read(0);
        assert_eq!(readout.lines.dropped, 3);
        assert_eq!(readout.lines.entries.len(), RECENT_LINES);
        assert_eq!(
            readout.warnings,
            u64::try_from(RECENT_LINES + 3).unwrap_or(0)
        );
    }
}
