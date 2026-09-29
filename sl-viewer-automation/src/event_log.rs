//! [`EventLog`]: what the session reported, what the viewer asked of it and
//! what the user did, as one sequence-numbered log read by cursor.
//!
//! A test asks "did the viewer send exactly one `ObjectUpdate` after I
//! clicked?" or "did the grid say the teleport finished?". A `MessageReader`
//! cannot answer between frames — it sees a message for two frames and then
//! never again — so the log keeps every `SlEvent`, `SlCommand` and `UiAction`
//! it saw, numbered by one counter over all three, and a reader keeps its own
//! cursor. It is bounded; a reader that falls further behind than the log
//! holds is told how many entries it missed ([`LogPage::dropped`]) rather than
//! silently skipping them.
//!
//! Entries are kept as the messages themselves and printed only when read, so
//! recording costs a clone, not a format.

use std::collections::VecDeque;

use bevy::prelude::*;
use sl_automation_proto::{LogEntry, LogPage, LogStream};
use sl_client_bevy::{Command, SlCommand, SlEvent, SlSessionEvent};
use sl_viewer_ui_core::ui_element::UiAction;

/// The longest detail an entry is printed with, in characters; a longer one
/// (an object update with its whole texture entry) is cut short.
pub const DETAIL_LIMIT: usize = 2048;

/// Records every session event, outbound command and UI action into
/// [`EventLog`]. Added by whoever installs automation.
#[derive(Debug, Default)]
pub struct EventLogPlugin;

impl Plugin for EventLogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EventLog>()
            .add_message::<SlEvent>()
            .add_message::<SlCommand>()
            .add_message::<UiAction>()
            .add_systems(
                Last,
                record_log.in_set(crate::executor::AutomationSystems::Record),
            );
    }
}

/// One logged message, kept whole until read.
#[derive(Debug, Clone)]
enum Logged {
    /// A session event.
    Event(SlSessionEvent),
    /// An outbound session command.
    Command(Command),
    /// A user interface action.
    UiAction(UiAction),
}

impl Logged {
    /// Which stream the entry belongs to.
    const fn stream(&self) -> LogStream {
        match self {
            Self::Event(_) => LogStream::Event,
            Self::Command(_) => LogStream::Command,
            Self::UiAction(_) => LogStream::UiAction,
        }
    }

    /// The entry as a reader sees it.
    fn entry(&self, seq: u64) -> LogEntry {
        let (kind, detail) = match self {
            Self::Event(event) => {
                let detail = format!("{event:?}");
                (variant_name(&detail).to_owned(), detail)
            }
            Self::Command(command) => {
                let detail = format!("{command:?}");
                (variant_name(&detail).to_owned(), detail)
            }
            Self::UiAction(action) => (
                format!("{}.{}", action.element, action.action),
                format!("{action:?}"),
            ),
        };
        LogEntry {
            seq,
            stream: self.stream(),
            kind,
            detail: truncated(detail),
        }
    }
}

/// The variant name a derived `Debug` output starts with: everything before
/// the first `(`, `{` or space.
fn variant_name(debug: &str) -> &str {
    debug.split(['(', '{', ' ']).next().unwrap_or(debug)
}

/// `detail` cut to [`DETAIL_LIMIT`] characters, marked when it was.
fn truncated(detail: String) -> String {
    match detail.char_indices().nth(DETAIL_LIMIT) {
        Some((cut, _char)) => {
            let mut short = detail;
            short.truncate(cut);
            short.push('…');
            short
        }
        None => detail,
    }
}

/// The log: the most recent entries of every stream, oldest first, each with
/// its sequence number.
#[derive(Debug, Resource)]
pub struct EventLog {
    /// The entries kept, oldest first; the first has sequence number
    /// `next - entries.len()`.
    entries: VecDeque<Logged>,
    /// The sequence number the next entry gets.
    next: u64,
    /// The most entries kept.
    capacity: usize,
}

impl Default for EventLog {
    fn default() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }
}

impl EventLog {
    /// The entries a log keeps unless told otherwise: minutes of an ordinary
    /// session, seconds of an object flood.
    pub const DEFAULT_CAPACITY: usize = 4096;

    /// A log keeping at most `capacity` entries (at least one).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::with_capacity(capacity.min(Self::DEFAULT_CAPACITY)),
            next: 0,
            capacity,
        }
    }

    /// The cursor that reads only what is recorded from now on.
    #[must_use]
    pub const fn cursor(&self) -> u64 {
        self.next
    }

    /// The sequence number of the oldest entry kept.
    fn first(&self) -> u64 {
        self.next
            .saturating_sub(u64::try_from(self.entries.len()).unwrap_or(u64::MAX))
    }

    /// Append an entry, dropping the oldest past the capacity.
    fn push(&mut self, logged: Logged) {
        self.entries.push_back(logged);
        self.next = self.next.saturating_add(1);
        while self.entries.len() > self.capacity {
            let _dropped = self.entries.pop_front();
        }
    }

    /// Read from `cursor` on: at most `limit` entries of `streams` (every
    /// stream when empty), oldest first.
    ///
    /// The page's [`next`](LogPage::next) continues the read: past the last
    /// entry returned when the limit cut it short, else past everything
    /// recorded, so a filtered reader does not re-scan what it skipped. Its
    /// [`dropped`](LogPage::dropped) counts the entries after `cursor` that
    /// the log no longer holds, of every stream.
    #[must_use]
    pub fn read(&self, cursor: u64, streams: &[LogStream], limit: usize) -> LogPage<LogEntry> {
        let first = self.first();
        let dropped = first.saturating_sub(cursor);
        let start = cursor.max(first);
        let mut entries = Vec::new();
        let mut next = self.next.max(cursor);
        let kept = self
            .entries
            .iter()
            .zip(first..)
            .skip_while(|(_logged, seq)| *seq < start)
            .filter(|(logged, _seq)| streams.is_empty() || streams.contains(&logged.stream()));
        for (logged, seq) in kept {
            if entries.len() == limit {
                // Cut short: resume just past the last entry returned (or at the
                // cursor, for a zero limit).
                next = entries
                    .last()
                    .map_or(start, |last: &LogEntry| last.seq.saturating_add(1));
                break;
            }
            entries.push(logged.entry(seq));
        }
        LogPage {
            entries,
            next,
            dropped,
        }
    }
}

/// Append this frame's events, commands and UI actions to [`EventLog`], in
/// that order within the frame.
///
/// In `Last`, after every system that writes one: a message lives for two
/// frames, so this reader's own cursor sees each exactly once whenever in the
/// frame it was written.
fn record_log(
    mut events: MessageReader<SlEvent>,
    mut commands: MessageReader<SlCommand>,
    mut actions: MessageReader<UiAction>,
    mut log: ResMut<EventLog>,
) {
    for SlEvent(event) in events.read() {
        log.push(Logged::Event(event.clone()));
    }
    for SlCommand(command) in commands.read() {
        log.push(Logged::Command(command.clone()));
    }
    for action in actions.read() {
        log.push(Logged::UiAction(*action));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_automation_proto::LogStream;
    use sl_client_bevy::{Command, SlSessionEvent};
    use sl_viewer_ui_core::ui_element::UiAction;

    use super::{DETAIL_LIMIT, EventLog, Logged, truncated, variant_name};

    /// A UI action named `action`.
    const fn action(action: &'static str) -> Logged {
        Logged::UiAction(UiAction {
            element: "toolbar",
            action,
        })
    }

    #[test]
    fn a_cursor_read_returns_a_burst_in_order() {
        let mut log = EventLog::default();
        let cursor = log.cursor();
        log.push(Logged::Event(SlSessionEvent::TeleportStarted));
        log.push(Logged::Command(Command::Stand));
        log.push(action("inventory"));
        let page = log.read(cursor, &[], usize::MAX);
        let kinds: Vec<(u64, LogStream, &str)> = page
            .entries
            .iter()
            .map(|entry| (entry.seq, entry.stream, entry.kind.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (0, LogStream::Event, "TeleportStarted"),
                (1, LogStream::Command, "Stand"),
                (2, LogStream::UiAction, "toolbar.inventory"),
            ]
        );
        assert_eq!(page.next, 3);
        assert_eq!(page.dropped, 0);
        assert!(log.read(page.next, &[], usize::MAX).entries.is_empty());
    }

    #[test]
    fn a_limit_and_a_filter_continue_where_they_stopped() {
        let mut log = EventLog::default();
        for _round in 0..3 {
            log.push(action("a"));
            log.push(Logged::Command(Command::Stand));
        }
        let first = log.read(0, &[LogStream::Command], 2);
        assert_eq!(
            first
                .entries
                .iter()
                .map(|entry| entry.seq)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(first.next, 4, "resumes after the last one returned");
        let rest = log.read(first.next, &[LogStream::Command], 2);
        assert_eq!(
            rest.entries
                .iter()
                .map(|entry| entry.seq)
                .collect::<Vec<_>>(),
            [5]
        );
        assert_eq!(rest.next, 6, "past everything recorded");
    }

    #[test]
    fn a_reader_left_behind_is_told_what_it_missed() {
        let mut log = EventLog::with_capacity(2);
        for name in ["a", "b", "c", "d", "e"] {
            log.push(action(name));
        }
        let page = log.read(1, &[], usize::MAX);
        assert_eq!(page.dropped, 2, "entries 1 and 2 are gone");
        assert_eq!(
            page.entries
                .iter()
                .map(|entry| entry.kind.as_str())
                .collect::<Vec<_>>(),
            ["toolbar.d", "toolbar.e"]
        );
        assert_eq!(log.read(4, &[], usize::MAX).dropped, 0);
    }

    #[test]
    fn a_long_detail_is_cut_and_marked() {
        let long = "é".repeat(DETAIL_LIMIT + 5);
        let short = truncated(long);
        assert_eq!(short.chars().count(), DETAIL_LIMIT + 1);
        assert!(short.ends_with('…'));
        assert_eq!(truncated("fits".to_owned()), "fits");
        assert_eq!(
            variant_name("ChatReceived(ChatMessage { .. })"),
            "ChatReceived"
        );
        assert_eq!(variant_name("Chat { message: \"\" }"), "Chat");
        assert_eq!(variant_name("Stand"), "Stand");
    }
}
