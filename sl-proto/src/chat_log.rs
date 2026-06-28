//! The optional, default-off local **chat-log file** core — the grid-agnostic,
//! sans-IO half of the runtime feature that writes every text-chat line to a
//! per-conversation transcript and reads it back for long-term scrollback.
//!
//! This module is **pure**: it owns the configuration ([`ChatLogConfig`]), the
//! Firestorm-compatible line **format** ([`format_log_line`]) and **parse**
//! ([`parse_log_lines`]), the filename schemes ([`nearby_log_file_name`] et al.),
//! the [`clean_file_name`] sanitiser, and the `conversation.log` index line — none
//! of which touch the filesystem or a clock. The runtimes (`sl-client-tokio` /
//! `sl-client-bevy` / the REPL) wrap it in a thin file-I/O shell that supplies the
//! local wall-clock as a [`LogLineTime`] and does the actual `append` / `seek`.
//! Keeping the format pure makes it testable on any grid (write a line, assert the
//! bytes; read it back, assert the parse) and keeps all file I/O out of the sans-IO
//! [`Session`](crate::Session).
//!
//! Grounded in Firestorm `LLLogChat` (`lllogchat.cpp`) and `LLConversationLog`
//! (`llconversationlog.cpp`), and byte-compatible with them so the files interleave
//! with a Firestorm install.

use crate::session::SessionMessage;
use crate::types::ImDialog;
use sl_types::key::AgentKey;
use std::collections::BTreeSet;

/// Firestorm's `LOG_RECALL_SIZE` (`lllogchat.h`): the number of trailing bytes a
/// viewer reads from a transcript to seed its scrollback. Reused here as the
/// default file read/page window ([`ChatLogConfig::recall_window`]).
pub const LOG_RECALL_SIZE: usize = 20480;

/// Firestorm's `FSConversationLogLifetime` default (`llconversationlog.cpp`): the
/// number of days a `conversation.log` index entry is retained before it is purged
/// on load.
pub const CONVERSATION_LOG_RETENTION_DAYS: u32 = 30;

/// The system sender name Firestorm writes for a message with no avatar sender
/// (`SYSTEM_FROM`). A system line is logged as `Second Life: …`.
pub const SYSTEM_SENDER_NAME: &str = "Second Life";

/// The characters Firestorm's `cleanFileName` (`lllogchat.cpp`) strips from a log
/// filename stem, each replaced with `_`. Note that `.` is in the set, so a dotted
/// account name collapses its dots to underscores in the stem (the `.txt` suffix is
/// appended afterwards, untouched).
const FORBIDDEN_FILENAME_CHARS: &[char] = &[
    '"', '\'', '\\', '/', '?', '*', ':', '.', '<', '>', '|', '[', ']', '{', '}', '~',
];

/// A broken-down **local** wall-clock instant, supplied by the runtime (the
/// sans-IO core has no clock). Carries exactly the fields a Firestorm log line
/// renders — year, month, day, hour, minute, second — with no time-zone, mirroring
/// Firestorm's zone-less local timestamps. Produced by the runtime from
/// `SystemTime::now()` (or an inbound message's wire Unix time) and consumed by
/// [`format_log_line`]; recovered (best-effort) by [`parse_log_lines`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLineTime {
    /// The calendar year (e.g. `2026`).
    pub year: i32,
    /// The month of the year, `1..=12`.
    pub month: u8,
    /// The day of the month, `1..=31`.
    pub day: u8,
    /// The hour of the day on a 24-hour clock, `0..=23`.
    pub hour: u8,
    /// The minute of the hour, `0..=59`.
    pub minute: u8,
    /// The second of the minute, `0..=60` (a leap second yields `60`).
    pub second: u8,
}

/// Which of the three logged conversation kinds a transcript / `conversation.log`
/// entry is — the discriminator behind the filename scheme and the index line's
/// numeric type field. Nearby chat is **not** here: it is region-local, has no
/// conversation identity, and always logs to the single `chat.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    /// A 1:1 instant-message conversation (`conversation.log` type `0`).
    Direct,
    /// A group IM session (`conversation.log` type `1`).
    Group,
    /// An ad-hoc conference / multi-party session (`conversation.log` type `2`).
    Conference,
}

impl ConversationKind {
    /// The numeric type code Firestorm's `conversation.log` uses for this kind
    /// (`0` P2P / `1` group / `2` ad-hoc).
    #[must_use]
    pub const fn log_type_code(self) -> u8 {
        match self {
            Self::Direct => 0,
            Self::Group => 1,
            Self::Conference => 2,
        }
    }

    /// The [`LoggedChatType`] this conversation kind enables under, used to test it
    /// against [`ChatLogConfig::enabled`].
    #[must_use]
    pub const fn logged_type(self) -> LoggedChatType {
        match self {
            Self::Direct => LoggedChatType::InstantMessage,
            Self::Group => LoggedChatType::Group,
            Self::Conference => LoggedChatType::Conference,
        }
    }
}

/// One enable-able text-chat type — the membership element of
/// [`ChatLogConfig::enabled`]. Modelled as a set element rather than a clutch of
/// `bool`s so the four toggles cannot be confused at a call site (and so the config
/// stays under the project's struct-bool limit). [`Nearby`](Self::Nearby) is the
/// only one with no [`ConversationKind`] — region-local chat has no conversation
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoggedChatType {
    /// Region-local nearby chat, logged to `chat.txt`.
    Nearby,
    /// 1:1 instant messages, logged to `<name>.txt`.
    InstantMessage,
    /// Group session messages, logged to `<group> (group).txt`.
    Group,
    /// Ad-hoc conference messages, logged to `Ad-hoc Conference hash<md5>.txt`.
    Conference,
}

/// Whether log timestamps use a 24-hour or a 12-hour `AM`/`PM` clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStyle {
    /// A 24-hour clock (`14:05:09`).
    TwentyFourHour,
    /// A 12-hour clock with an `AM`/`PM` marker (`02:05:09 PM`).
    TwelveHour,
}

/// How the `[…]` timestamp prefix of a log line is rendered. Its presence is the
/// "log a timestamp at all" toggle: [`ChatLogConfig::timestamp`] is `None` to omit
/// the prefix entirely. The two `bool`s plus the [`ClockStyle`] enum keep the type
/// under the struct-bool limit while spelling each knob out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampFormat {
    /// Include the `YYYY/MM/DD` date (Firestorm `LogTimestampDate`); when `false`
    /// the prefix is time-only, `[HH:MM:SS]`.
    pub date: bool,
    /// Include seconds — **default on** (user-set 2026-06-27), flipping Firestorm's
    /// default-off `FSSecondsinChatTimestamps`.
    pub seconds: bool,
    /// The 12-/24-hour clock style.
    pub clock: ClockStyle,
}

impl Default for TimestampFormat {
    fn default() -> Self {
        Self {
            date: true,
            seconds: true,
            clock: ClockStyle::TwentyFourHour,
        }
    }
}

/// The set of per-account filesystem directories a runtime persists its optional,
/// default-off features under. Each field is `Option<PathBuf>`: `None` disables that
/// feature entirely (there is no built-in default location — the host application
/// supplies the path, e.g. an XDG cache dir, or opts out by leaving it `None`). It
/// is supplied **once** at each runtime's construction, alongside the
/// [`ChatLogConfig`], and threaded to the file-I/O shells that own the actual writes
/// (the sans-IO [`Session`](crate::Session) never touches the filesystem).
///
/// The directories are deliberately separate so the three features can live in
/// different roots: a per-account cache dir (machine-regenerable), a per-account
/// chat-log dir (user-facing transcripts), and a cross-account shared cache.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientDirectories {
    /// The directory the per-account inventory disk-cache files
    /// (`<agent-uuid>.inv.llsd.gz` and `<agent-uuid>.lib.inv.llsd.gz`) are written
    /// **directly** under, or `None` to disable inventory caching. Consumed by the
    /// runtime inventory-cache shell (see the `B10` cache shells).
    pub agent_cache_dir: Option<std::path::PathBuf>,
    /// The directory chat-log transcripts (and the optional `conversation.log`
    /// index) are written **directly** under, or `None` to disable chat logging.
    /// Passed verbatim to the runtime `ChatLog` shell — there is no derived
    /// per-account sub-directory; the host supplies an already-per-account path.
    pub agent_chat_log_dir: Option<std::path::PathBuf>,
    /// Reserved for a cache directory shared across accounts (e.g. a grid-wide
    /// texture or name cache). Not yet consumed; `None` until a shared-cache
    /// feature lands.
    pub shared_cache_dir: Option<std::path::PathBuf>,
}

/// The runtime inventory disk-cache configuration — opt-in, **default OFF**
/// (mirroring the chat-log toggles). Even when enabled, the feature stays
/// dormant unless [`ClientDirectories::agent_cache_dir`] also supplies a
/// directory to write the per-account `<agent-uuid>.inv.llsd.gz` /
/// `.lib.inv.llsd.gz` files under. Supplied once at each runtime's construction
/// and consumed by the runtime inventory-cache shell (the sans-IO
/// [`Session`](crate::Session) never touches the filesystem).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryCacheConfig {
    /// The master switch for the inventory disk cache. While `false` (the
    /// default) the runtime neither loads a cache at login nor writes one at
    /// logout/idle, so a consumer that does not want on-disk inventory pays
    /// nothing. While `true` (and a [`ClientDirectories::agent_cache_dir`] is
    /// set) the runtime loads the cache before the login skeleton, reconciles it
    /// against the skeleton, and persists the cacheable snapshot on logout and on
    /// the dirty/idle tick.
    pub enabled: bool,
    /// Whether to also persist the read-only shared **Library** tree to
    /// `<agent-uuid>.lib.inv.llsd.gz`. The agent's own tree is always cached when
    /// the feature is enabled; the (large, rarely-changing) Library cache can be
    /// turned off independently. Defaults to `true` — both caches are written, as
    /// Firestorm does (its single-instance guard does not apply here).
    pub cache_library: bool,
}

impl Default for InventoryCacheConfig {
    /// The feature off, but the Library toggle on, so merely flipping
    /// [`enabled`](Self::enabled) caches both trees.
    fn default() -> Self {
        Self {
            enabled: false,
            cache_library: true,
        }
    }
}

/// The runtime chat-log configuration — opt-in, **default OFF**, mirroring
/// Firestorm's per-account toggles. The whole feature is disabled until a runtime
/// enables one or more text-chat types (via [`enabled`](Self::enabled)); the
/// timestamp/format knobs then shape every line written. [`Default`] yields the
/// all-off configuration with Firestorm's format defaults (timestamp on, date on,
/// **seconds on**, 24-hour, the [`LOG_RECALL_SIZE`] window, the 30-day index
/// retention).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLogConfig {
    /// The text-chat types whose messages are written to a transcript. Empty means
    /// the feature is fully off.
    pub enabled: BTreeSet<LoggedChatType>,
    /// Use the legacy `firstname.lastname` IM filename scheme rather than the
    /// modern display/account name (Firestorm `UseLegacyIMLogNames`). The runtime
    /// selects which name string to pass; this records the user's preference so it
    /// can thread through and the REPL can toggle it.
    pub legacy_im_names: bool,
    /// Append a date suffix to filenames (Firestorm `LogFileNamewithDate`): `daily`
    /// (`-%Y-%m-%d`) for nearby chat, `monthly` (`-%Y-%m`) for IM / group; **never**
    /// for ad-hoc conferences.
    pub date_suffix: bool,
    /// How the `[…]` timestamp prefix is rendered, or `None` to omit it (Firestorm
    /// `LogTimestamp` off).
    pub timestamp: Option<TimestampFormat>,
    /// The newest-message recall window, in bytes, kept for parity with Firestorm's
    /// `LOG_RECALL_SIZE` (the size of the tail it seeds a freshly-opened
    /// conversation from); defaults to [`LOG_RECALL_SIZE`]. Note that, unlike
    /// Firestorm, the runtime's on-demand `QueryChatHistoryPage` paging reads the
    /// **whole** transcript so all history is reachable — this value does not cap
    /// how far back a page can go.
    pub recall_window: usize,
    /// Maintain the per-account `conversation.log` index (Firestorm
    /// `KeepConversationLogTranscripts`); default off.
    pub conversation_log: bool,
    /// Days a `conversation.log` entry is retained before it is purged on load
    /// (Firestorm `FSConversationLogLifetime`); defaults to
    /// [`CONVERSATION_LOG_RETENTION_DAYS`].
    pub conversation_log_retention_days: u32,
}

impl Default for ChatLogConfig {
    fn default() -> Self {
        Self {
            enabled: BTreeSet::new(),
            legacy_im_names: false,
            date_suffix: false,
            timestamp: Some(TimestampFormat::default()),
            recall_window: LOG_RECALL_SIZE,
            conversation_log: false,
            conversation_log_retention_days: CONVERSATION_LOG_RETENTION_DAYS,
        }
    }
}

impl ChatLogConfig {
    /// Whether *any* text-chat type is enabled — the cheap gate a runtime checks
    /// before doing any chat-log work.
    #[must_use]
    pub fn any_enabled(&self) -> bool {
        !self.enabled.is_empty()
    }

    /// Whether region-local nearby chat is logged.
    #[must_use]
    pub fn logs_nearby(&self) -> bool {
        self.enabled.contains(&LoggedChatType::Nearby)
    }

    /// Whether messages of `kind` (a session conversation, not nearby chat) are
    /// logged under the current configuration.
    #[must_use]
    pub fn logs_conversation(&self, kind: ConversationKind) -> bool {
        self.enabled.contains(&kind.logged_type())
    }
}

/// Sanitises a filename stem with Firestorm's `cleanFileName` rule: every
/// `FORBIDDEN_FILENAME_CHARS` character is replaced with `_`. The `.txt`
/// suffix and any date suffix are added by the callers *after* cleaning, so they
/// are not affected.
#[must_use]
pub fn clean_file_name(stem: &str) -> String {
    stem.chars()
        .map(|c| {
            if FORBIDDEN_FILENAME_CHARS.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// The daily date suffix Firestorm appends to nearby-chat filenames
/// (`-%Y-%m-%d`).
fn daily_date_suffix(time: &LogLineTime) -> String {
    format!("-{:04}-{:02}-{:02}", time.year, time.month, time.day)
}

/// The monthly date suffix Firestorm appends to IM / group filenames (`-%Y-%m`).
fn monthly_date_suffix(time: &LogLineTime) -> String {
    format!("-{:04}-{:02}", time.year, time.month)
}

/// The filename for **nearby** (region-local) chat: `chat.txt`, with an optional
/// daily date suffix.
#[must_use]
pub fn nearby_log_file_name(config: &ChatLogConfig, time: &LogLineTime) -> String {
    let suffix = if config.date_suffix {
        daily_date_suffix(time)
    } else {
        String::new()
    };
    format!("chat{suffix}.txt")
}

/// The filename for a **1:1 IM** conversation, built from the peer's name (the
/// runtime chooses the legacy or modern name per
/// [`ChatLogConfig::legacy_im_names`]): `<clean name>[-YYYY-MM].txt`.
#[must_use]
pub fn im_log_file_name(config: &ChatLogConfig, peer_name: &str, time: &LogLineTime) -> String {
    let stem = clean_file_name(peer_name);
    let suffix = if config.date_suffix {
        monthly_date_suffix(time)
    } else {
        String::new()
    };
    format!("{stem}{suffix}.txt")
}

/// The filename for a **group** IM session: `<clean group name> (group)[-YYYY-MM].txt`
/// (the ` (group)` marker is part of the cleaned stem; its parentheses survive
/// cleaning).
#[must_use]
pub fn group_log_file_name(config: &ChatLogConfig, group_name: &str, time: &LogLineTime) -> String {
    let stem = clean_file_name(&format!("{group_name} (group)"));
    let suffix = if config.date_suffix {
        monthly_date_suffix(time)
    } else {
        String::new()
    };
    format!("{stem}{suffix}.txt")
}

/// The filename for an **ad-hoc conference**:
/// `Ad-hoc Conference hash<md5>.txt`, where `<md5>` is the lowercase-hex MD5 of the
/// sorted participant ids' hyphenated UUID strings (the [`BTreeSet`] iterates in
/// sorted order, so the hash is participant-order-independent). A conference name
/// is never used and a date suffix is **never** applied — the hash *is* the stable
/// identity.
#[must_use]
pub fn conference_log_file_name(participants: &BTreeSet<AgentKey>) -> String {
    let mut context = md5::Context::new();
    for id in participants {
        context.consume(id.uuid().to_string().as_bytes());
    }
    let digest = context.compute();
    format!("Ad-hoc Conference hash{digest:x}.txt")
}

/// Renders the `[HH:MM[:SS]][ AM/PM]` time portion of a timestamp, honouring the
/// seconds and 12-/24-hour toggles.
fn format_clock(format: TimestampFormat, time: &LogLineTime) -> String {
    match format.clock {
        ClockStyle::TwelveHour => {
            // 12-hour clock: 0 and 12 both display as 12; AM below noon, PM at/above.
            let meridiem = if time.hour < 12 { "AM" } else { "PM" };
            let hour12 = match time.hour.checked_rem(12) {
                Some(0) | None => 12,
                Some(other) => other,
            };
            if format.seconds {
                format!(
                    "{hour12:02}:{:02}:{:02} {meridiem}",
                    time.minute, time.second
                )
            } else {
                format!("{hour12:02}:{:02} {meridiem}", time.minute)
            }
        }
        ClockStyle::TwentyFourHour => {
            if format.seconds {
                format!("{:02}:{:02}:{:02}", time.hour, time.minute, time.second)
            } else {
                format!("{:02}:{:02}", time.hour, time.minute)
            }
        }
    }
}

/// Renders the full bracketed `[YYYY/MM/DD HH:MM:SS]` timestamp prefix, or `None`
/// when [`ChatLogConfig::timestamp`] is off (no prefix at all).
fn format_timestamp(config: &ChatLogConfig, time: &LogLineTime) -> Option<String> {
    let format = config.timestamp?;
    let clock = format_clock(format, time);
    if format.date {
        Some(format!(
            "[{:04}/{:02}/{:02} {clock}]",
            time.year, time.month, time.day
        ))
    } else {
        Some(format!("[{clock}]"))
    }
}

/// Escapes a sender name for a log line: a literal colon (which would otherwise be
/// read as the name/message separator) is URI-encoded `%3A`, matching Firestorm.
fn escape_name(name: &str) -> String {
    name.replace(':', "%3A")
}

/// Folds a possibly multi-line message body into a single physical line: each
/// embedded newline becomes a newline followed by one leading space (`\n␠`), so
/// continuation lines are space-prefixed and [`parse_log_lines`] re-joins them.
fn fold_message(message: &str) -> String {
    message.replace('\n', "\n ")
}

/// Formats one Firestorm-compatible transcript line (no trailing newline):
///
/// ```text
/// [YYYY/MM/DD HH:MM:SS]  Name: message
/// ```
///
/// Two spaces separate the `]` from the name (Firestorm's `IM_SEPARATOR` context);
/// the name's colons are `%3A`-escaped; embedded newlines in `message` are folded
/// to `\n␠` continuations. When the timestamp is disabled the line is just
/// `Name: message`. A system message passes [`SYSTEM_SENDER_NAME`] as `name`.
#[must_use]
pub fn format_log_line(
    config: &ChatLogConfig,
    time: &LogLineTime,
    name: &str,
    message: &str,
) -> String {
    let prefix = match format_timestamp(config, time) {
        Some(stamp) => format!("{stamp}  "),
        None => String::new(),
    };
    let name = escape_name(name);
    let body = fold_message(message);
    format!("{prefix}{name}: {body}")
}

/// One parsed transcript line: the recovered local timestamp (when the line
/// carried a `[…]` prefix that parsed), the sender name (`None` for a line that
/// failed the `Name: message` shape — kept as a plain-text fallback), and the
/// message body with its folded continuations rejoined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLogLine {
    /// The line's local timestamp, if it carried a parseable `[…]` prefix.
    pub time: Option<LogLineTime>,
    /// The sender name with `%3A` unescaped, or `None` for a plain-text fallback
    /// line (no recognisable `Name:` separator).
    pub name: Option<String>,
    /// The message body, with `\n␠` continuations rejoined to embedded newlines.
    pub message: String,
}

impl ParsedLogLine {
    /// Builds a [`SessionMessage`] from this parsed line, supplying the context the
    /// transcript does not store: the resolved `sender` key (the runtime maps the
    /// recovered name to a key — our own id for our name, the peer for a 1:1, a nil
    /// key otherwise), the session's `dialog`, and the `timestamp` the runtime
    /// recovered from [`Self::time`] (the local-zone conversion lives in the
    /// runtime). The recovered `name` is intentionally dropped — `SessionMessage`
    /// keys by `sender`, not by display name.
    #[must_use]
    pub fn into_session_message(
        self,
        sender: AgentKey,
        dialog: ImDialog,
        timestamp: Option<u32>,
    ) -> SessionMessage {
        SessionMessage {
            sender,
            dialog,
            text: self.message,
            timestamp,
        }
    }
}

/// Parses a single `[YYYY/MM/DD HH:MM:SS]` (or seconds-/date-less) prefix at the
/// start of `line`, returning the recovered [`LogLineTime`] and the remainder of
/// the line after the prefix and its trailing spaces. Returns `None` if the line
/// does not start with a well-formed bracketed timestamp.
fn parse_timestamp_prefix(line: &str) -> Option<(Option<LogLineTime>, &str)> {
    let rest = line.strip_prefix('[')?;
    let close = rest.find(']')?;
    let (inside, after) = rest.split_at(close);
    // `after` starts with the `]`; drop it and the following separator spaces.
    let remainder = after.get(1..)?.trim_start_matches(' ');
    Some((parse_timestamp_inner(inside), remainder))
}

/// Parses the text *inside* a `[…]` timestamp into a [`LogLineTime`]. Accepts the
/// `YYYY/MM/DD HH:MM:SS`, date-less `HH:MM:SS`, and seconds-less variants, plus a
/// trailing `AM`/`PM`. Returns `None` (a timestamp we could not interpret) rather
/// than failing the whole line — the caller still keeps the message text.
fn parse_timestamp_inner(inside: &str) -> Option<LogLineTime> {
    let mut date_part: Option<&str> = None;
    let mut clock_part = inside;
    if let Some((date, clock)) = inside.split_once(' ') {
        date_part = Some(date);
        clock_part = clock;
    }
    let (year, month, day) = match date_part {
        Some(date) => {
            let mut fields = date.split('/');
            let year = fields.next()?.parse().ok()?;
            let month = fields.next()?.parse().ok()?;
            let day = fields.next()?.parse().ok()?;
            (year, month, day)
        }
        None => (0, 1, 1),
    };
    let (clock, meridiem) = match clock_part.split_once(' ') {
        Some((clock, mer)) => (clock, Some(mer)),
        None => (clock_part, None),
    };
    let mut units = clock.split(':');
    let raw_hour: u8 = units.next()?.parse().ok()?;
    let minute: u8 = units.next()?.parse().ok()?;
    let second: u8 = units.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let hour = apply_meridiem(raw_hour, meridiem);
    Some(LogLineTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    })
}

/// Converts a clock hour and optional `AM`/`PM` marker back to a 24-hour hour.
/// With no marker the hour is already 24-hour. `12 AM` is hour `0`; `12 PM` is
/// hour `12`.
fn apply_meridiem(hour: u8, meridiem: Option<&str>) -> u8 {
    match meridiem {
        Some("PM") if hour < 12 => hour.saturating_add(12),
        Some("AM") if hour == 12 => 0,
        _ => hour,
    }
}

/// Whether a physical line *starts a new record* rather than continuing the
/// previous message. A Second Life message may itself contain newlines, so every
/// physical line that is **not** the start of a new record — including an empty one
/// — belongs to the message above it (Firestorm's parser folds the same way). In a
/// timestamped transcript (the default, and Firestorm's) a new record is exactly a
/// line that opens with a parseable `[…]` timestamp; nothing else begins a record,
/// so internal newlines never split a message. In a timestamp-less transcript there
/// is no such anchor, so a non-empty, non-space-prefixed line carrying the `name: `
/// separator opens a record and space-prefixed / empty lines continue it.
fn starts_new_record(raw: &str, timestamped: bool) -> bool {
    if timestamped {
        parse_timestamp_prefix(raw).is_some()
    } else {
        !raw.starts_with(' ') && !raw.is_empty() && raw.contains(": ")
    }
}

/// Parses a whole transcript chunk (one or more physical lines) back into
/// [`ParsedLogLine`]s, **oldest first** (file order). Each [`ParsedLogLine`] is one
/// logical message: a physical line that starts a new record (a timestamped line,
/// or — in a timestamp-less transcript — a non-space, non-empty `name: ` line)
/// opens an entry, and every following line that does not —
/// a space-prefixed continuation, **or an embedded blank line of a multi-line
/// message** — folds back into it as an embedded newline, so a Second Life message
/// containing internal newlines round-trips intact. A space-prefixed continuation
/// drops its single leading space (the byte the writer inserts); other folded lines
/// are kept verbatim. The colon in a name is `%3A`-unescaped; a record with no
/// `Name:` separator is a plain-text fallback entry (`name: None`).
#[must_use]
pub fn parse_log_lines(text: &str) -> Vec<ParsedLogLine> {
    // Decide once whether the transcript is timestamped: if any line opens with a
    // parseable timestamp, only timestamped lines start records and every other
    // line (continuation or blank) belongs to the message above it.
    let timestamped = text
        .lines()
        .any(|line| parse_timestamp_prefix(line).is_some());
    // Drop the single trailing newline terminator so it does not fold a spurious
    // blank line onto the last message; internal blank lines are preserved.
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut entries: Vec<ParsedLogLine> = Vec::new();
    for raw in body.split('\n') {
        if starts_new_record(raw, timestamped) || entries.is_empty() {
            // Skip a leading blank line (no message to attach it to).
            if raw.is_empty() && entries.is_empty() {
                continue;
            }
            entries.push(parse_one_line(raw));
            continue;
        }
        // A continuation of the message above: fold it back as an embedded newline,
        // dropping the one leading space the writer adds to space-prefixed lines.
        let folded = raw.strip_prefix(' ').unwrap_or(raw);
        if let Some(last) = entries.last_mut() {
            last.message.push('\n');
            last.message.push_str(folded);
        }
    }
    entries
}

/// Parses a single physical (non-continuation) transcript line into a
/// [`ParsedLogLine`], recovering the optional timestamp prefix and splitting the
/// `Name: message` body; a line with no `Name:` separator is a plain-text fallback.
fn parse_one_line(raw: &str) -> ParsedLogLine {
    let (time, body) = match parse_timestamp_prefix(raw) {
        Some((time, rest)) => (time, rest),
        None => (None, raw),
    };
    match body.split_once(": ") {
        Some((name, message)) => ParsedLogLine {
            time,
            name: Some(name.replace("%3A", ":")),
            message: message.to_owned(),
        },
        None => ParsedLogLine {
            time,
            name: None,
            message: body.to_owned(),
        },
    }
}

/// Formats one `conversation.log` index line (no trailing newline), Firestorm's
/// `LLConversationLogFriends` format:
///
/// ```text
/// [<unix>] <type> 0 <offline> <name>| <pid> <sid> <file>|
/// ```
///
/// `unix` is the conversation's last-activity Unix time, `type` the
/// [`ConversationKind::log_type_code`], the `reserved` field is always `0`,
/// `offline` is `1` when the conversation has unread messages else `0`, `name` is
/// the display name, `pid` / `sid` the participant and session ids, and `file` the
/// transcript filename. The two `|` terminators bracket the free-text name and file
/// fields so a name containing spaces parses unambiguously.
#[must_use]
pub fn conversation_log_line(
    unix: u32,
    kind: ConversationKind,
    has_unread: bool,
    name: &str,
    participant_id: AgentKey,
    session_id: uuid::Uuid,
    file_name: &str,
) -> String {
    let type_code = kind.log_type_code();
    let offline = u8::from(has_unread);
    let pid = participant_id.uuid();
    format!("[{unix}] {type_code} 0 {offline} {name}| {pid} {session_id} {file_name}|")
}

/// The last-activity Unix time of a `conversation.log` line (its `[<unix>]`
/// prefix), for the runtime's retention purge on load. `None` if the line does not
/// open with a parseable bracketed integer.
#[must_use]
pub fn conversation_log_unix(line: &str) -> Option<u32> {
    line.strip_prefix('[')?.split_once(']')?.0.parse().ok()
}

/// The transcript file name field of a `conversation.log` line — the stable
/// per-conversation key the runtime upserts by. The file name may contain spaces,
/// so it is read as everything between the third whitespace-separated token after
/// the first `|` and the closing `|`, rather than by naive splitting.
#[must_use]
pub fn conversation_log_file(line: &str) -> Option<&str> {
    // After the first `|`: " <pid> <sid> <file>|". Strip the trailing `|`, then drop
    // the two fixed-width id tokens; the remainder is the (possibly spaced) file.
    let tail = line.split_once('|')?.1.strip_suffix('|')?.trim_start();
    let after_pid = tail.split_once(' ')?.1;
    Some(after_pid.split_once(' ')?.1)
}

#[cfg(test)]
mod tests {
    use super::{
        ChatLogConfig, ClockStyle, ConversationKind, LOG_RECALL_SIZE, LogLineTime, LoggedChatType,
        ParsedLogLine, TimestampFormat, clean_file_name, conference_log_file_name,
        conversation_log_file, conversation_log_line, conversation_log_unix, format_log_line,
        group_log_file_name, im_log_file_name, nearby_log_file_name, parse_log_lines,
    };
    use crate::types::ImDialog;
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use std::collections::BTreeSet;
    use uuid::Uuid;

    /// A fixed local timestamp used across the format tests.
    const SAMPLE_TIME: LogLineTime = LogLineTime {
        year: 2026,
        month: 6,
        day: 27,
        hour: 14,
        minute: 5,
        second: 9,
    };

    /// The configuration that enables a date suffix on top of the defaults.
    fn dated_config() -> ChatLogConfig {
        ChatLogConfig {
            date_suffix: true,
            ..ChatLogConfig::default()
        }
    }

    #[test]
    fn formats_the_firestorm_line_with_seconds_by_default() {
        let line = format_log_line(
            &ChatLogConfig::default(),
            &SAMPLE_TIME,
            "Alice Resident",
            "hi there",
        );
        assert_eq!(line, "[2026/06/27 14:05:09]  Alice Resident: hi there");
    }

    #[test]
    fn drops_seconds_when_disabled() {
        let format = TimestampFormat {
            seconds: false,
            ..TimestampFormat::default()
        };
        let config = ChatLogConfig {
            timestamp: Some(format),
            ..ChatLogConfig::default()
        };
        let line = format_log_line(&config, &SAMPLE_TIME, "Bob", "yo");
        assert_eq!(line, "[2026/06/27 14:05]  Bob: yo");
    }

    #[test]
    fn time_only_when_date_disabled() {
        let format = TimestampFormat {
            date: false,
            ..TimestampFormat::default()
        };
        let config = ChatLogConfig {
            timestamp: Some(format),
            ..ChatLogConfig::default()
        };
        let line = format_log_line(&config, &SAMPLE_TIME, "Bob", "yo");
        assert_eq!(line, "[14:05:09]  Bob: yo");
    }

    #[test]
    fn twelve_hour_clock_renders_meridiem() {
        let format = TimestampFormat {
            clock: ClockStyle::TwelveHour,
            ..TimestampFormat::default()
        };
        let config = ChatLogConfig {
            timestamp: Some(format),
            ..ChatLogConfig::default()
        };
        let line = format_log_line(&config, &SAMPLE_TIME, "Bob", "yo");
        assert_eq!(line, "[2026/06/27 02:05:09 PM]  Bob: yo");
    }

    #[test]
    fn escapes_colon_in_name_and_folds_newlines() {
        let line = format_log_line(&ChatLogConfig::default(), &SAMPLE_TIME, "a:b", "one\ntwo");
        assert_eq!(line, "[2026/06/27 14:05:09]  a%3Ab: one\n two");
    }

    #[test]
    fn clean_file_name_replaces_forbidden_chars() {
        assert_eq!(clean_file_name("a/b:c.d"), "a_b_c_d");
    }

    #[test]
    fn nearby_filename_with_and_without_date() {
        assert_eq!(
            nearby_log_file_name(&ChatLogConfig::default(), &SAMPLE_TIME),
            "chat.txt"
        );
        assert_eq!(
            nearby_log_file_name(&dated_config(), &SAMPLE_TIME),
            "chat-2026-06-27.txt"
        );
    }

    #[test]
    fn im_and_group_filenames_sanitise_and_date() {
        assert_eq!(
            im_log_file_name(&dated_config(), "first.last", &SAMPLE_TIME),
            "first_last-2026-06.txt"
        );
        assert_eq!(
            group_log_file_name(&dated_config(), "My Group", &SAMPLE_TIME),
            "My Group (group)-2026-06.txt"
        );
    }

    #[test]
    fn conference_filename_is_order_independent_md5() {
        let a = AgentKey::from(Uuid::from_u128(1));
        let b = AgentKey::from(Uuid::from_u128(2));
        let one: BTreeSet<AgentKey> = [a, b].into_iter().collect();
        let two: BTreeSet<AgentKey> = [b, a].into_iter().collect();
        let name = conference_log_file_name(&one);
        assert_eq!(name, conference_log_file_name(&two));
        assert_eq!(name.starts_with("Ad-hoc Conference hash"), true);
        let extension = std::path::Path::new(&name)
            .extension()
            .and_then(|ext| ext.to_str());
        assert_eq!(extension, Some("txt"));
    }

    #[test]
    fn round_trips_a_stored_line() {
        let line = format_log_line(
            &ChatLogConfig::default(),
            &SAMPLE_TIME,
            "Alice Resident",
            "hello world",
        );
        let expected = vec![ParsedLogLine {
            time: Some(SAMPLE_TIME),
            name: Some("Alice Resident".to_owned()),
            message: "hello world".to_owned(),
        }];
        assert_eq!(parse_log_lines(&line), expected);
    }

    #[test]
    fn round_trips_a_multi_line_message() {
        let line = format_log_line(
            &ChatLogConfig::default(),
            &SAMPLE_TIME,
            "Alice",
            "first\nsecond\nthird",
        );
        let expected = vec![ParsedLogLine {
            time: Some(SAMPLE_TIME),
            name: Some("Alice".to_owned()),
            message: "first\nsecond\nthird".to_owned(),
        }];
        assert_eq!(parse_log_lines(&line), expected);
    }

    #[test]
    fn folds_internal_blank_lines_in_a_multi_line_message() {
        let line = format_log_line(&ChatLogConfig::default(), &SAMPLE_TIME, "Alice", "a\n\nb");
        let expected = vec![ParsedLogLine {
            time: Some(SAMPLE_TIME),
            name: Some("Alice".to_owned()),
            message: "a\n\nb".to_owned(),
        }];
        assert_eq!(parse_log_lines(&line), expected);
    }

    #[test]
    fn timestamped_record_absorbs_untimestamped_following_lines() {
        // A multi-line message whose continuation lines carry no timestamp (and no
        // leading space) must stay part of the one record above them.
        let text = "[2026/06/27 14:05:09]  Alice: first\nstill same message\nand more";
        let expected = vec![ParsedLogLine {
            time: Some(SAMPLE_TIME),
            name: Some("Alice".to_owned()),
            message: "first\nstill same message\nand more".to_owned(),
        }];
        assert_eq!(parse_log_lines(text), expected);
    }

    #[test]
    fn separate_timestamped_records_do_not_merge() {
        let config = ChatLogConfig::default();
        let mut text = format_log_line(&config, &SAMPLE_TIME, "Alice", "hi");
        text.push('\n');
        text.push_str(&format_log_line(&config, &SAMPLE_TIME, "Bob", "yo"));
        let expected = vec![
            ParsedLogLine {
                time: Some(SAMPLE_TIME),
                name: Some("Alice".to_owned()),
                message: "hi".to_owned(),
            },
            ParsedLogLine {
                time: Some(SAMPLE_TIME),
                name: Some("Bob".to_owned()),
                message: "yo".to_owned(),
            },
        ];
        assert_eq!(parse_log_lines(&text), expected);
    }

    #[test]
    fn malformed_line_is_plain_text_fallback() {
        let expected = vec![ParsedLogLine {
            time: None,
            name: None,
            message: "this is not a log line".to_owned(),
        }];
        assert_eq!(parse_log_lines("this is not a log line"), expected);
    }

    #[test]
    fn parses_into_a_session_message() {
        let sender = AgentKey::from(Uuid::from_u128(7));
        let line = format_log_line(&ChatLogConfig::default(), &SAMPLE_TIME, "Alice", "hi");
        let message = parse_log_lines(&line)
            .into_iter()
            .next()
            .map(|parsed| parsed.into_session_message(sender, ImDialog::Message, Some(42)));
        let parsed = message.map(|m| (m.sender, m.dialog, m.text, m.timestamp));
        assert_eq!(
            parsed,
            Some((sender, ImDialog::Message, "hi".to_owned(), Some(42)))
        );
    }

    #[test]
    fn conversation_log_line_round_trips_shape() {
        let pid = AgentKey::from(Uuid::from_u128(3));
        let sid = Uuid::from_u128(4);
        let line = conversation_log_line(
            1000,
            ConversationKind::Group,
            true,
            "My Group",
            pid,
            sid,
            "f.txt",
        );
        assert_eq!(line.starts_with("[1000] 1 0 1 My Group| "), true);
        assert_eq!(line.ends_with(" f.txt|"), true);
    }

    #[test]
    fn conversation_log_line_fields_extract_with_a_spaced_file_name() {
        let pid = AgentKey::from(Uuid::from_u128(3));
        let sid = Uuid::from_u128(4);
        let line = conversation_log_line(
            1700,
            ConversationKind::Group,
            false,
            "My Group",
            pid,
            sid,
            "My Group (group).txt",
        );
        assert_eq!(conversation_log_unix(&line), Some(1700));
        assert_eq!(conversation_log_file(&line), Some("My Group (group).txt"));
    }

    #[test]
    fn default_config_is_all_off() {
        let config = ChatLogConfig::default();
        assert_eq!(config.any_enabled(), false);
        assert_eq!(config.timestamp, Some(TimestampFormat::default()));
        assert_eq!(config.recall_window, LOG_RECALL_SIZE);
    }

    #[test]
    fn enabled_set_drives_logging_predicates() {
        let config = ChatLogConfig {
            enabled: [LoggedChatType::Nearby, LoggedChatType::Group]
                .into_iter()
                .collect(),
            ..ChatLogConfig::default()
        };
        assert_eq!(config.logs_nearby(), true);
        assert_eq!(config.logs_conversation(ConversationKind::Group), true);
        assert_eq!(config.logs_conversation(ConversationKind::Direct), false);
    }
}
