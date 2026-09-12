//! The **experience event log** (`viewer-experience-event-stream`): the
//! reference `LLExperienceLog`, which keeps what the experiences the agent has
//! joined have actually *done* to them and can raise a toast for each.
//!
//! # Why there is anything to keep
//!
//! An experience the agent has joined runs its scripts without prompting for
//! each permission — that is what joining one buys. So the in-the-moment
//! `ScriptQuestion` the ordinary permission surfaces
//! ([`crate::script_permission`], [`crate::experience_permission`]) are built on
//! never appears, and nothing else in the session mentions what an experience
//! did. The region instead reports it **afterwards**, as an `ExperienceEvent`
//! generic message, which `sl-proto` decodes into
//! [`SlSessionEvent::ExperienceEvent`]. This module is what remembers those
//! reports, so "what has this experience been doing with the trust I gave it?"
//! has an answer.
//!
//! It is also the only place in the protocol an experience **attachment** is
//! reported at all ([`ExperienceEventPermission::Attach`]).
//!
//! # What the log does with a report
//!
//! - **Coalescing.** A repeat of the same (experience, object, owner, parcel,
//!   permission) tuple on the same local day bumps the previous entry's
//!   [`count`](LoggedExperienceEvent::count) and its timestamp rather than
//!   appending — a ride that re-seats you a hundred times is one row that says
//!   100, not a hundred rows. This is the reference's rule, including that it
//!   compares only against the **last** entry, so an interleaved second object
//!   starts a new row for each.
//! - **Retention.** Entries older than the `ExperienceLogDays` setting (the
//!   reference's 7-day default) are dropped, on load and as each new one
//!   arrives. Zero days keeps nothing, which is how the reference spells
//!   "off".
//! - **Notification.** With `NotifyAllExperienceEvents` set, every recorded
//!   report — a coalesce included, as in the reference — raises the catalogue's
//!   `ExperienceEvent` or `ExperienceEventAttachment` tip, chosen by
//!   [`is_attachment`](LoggedExperienceEvent::is_attachment).
//!
//! # Where it is kept
//!
//! A per-account `experience_events.json` beside the account `settings.toml`,
//! the reference's per-account `experience_events.xml`. It follows the
//! [`crate::notification_persist`] rules — an unreadable file is preserved
//! rather than treated as empty, and writes are atomic and serialized — because
//! the log is the only copy of a record the user may want much later.
//!
//! Two divergences from the reference, both deliberate: the retention window
//! and the notify toggle are ordinary **settings** rather than fields inside the
//! log file (this viewer already has a per-avatar settings scope, which is what
//! the reference's `PER_SL_ACCOUNT` file *is*), and an entry carries an absolute
//! timestamp rather than a day-keyed map with a time-of-day string beside it, so
//! expiry is arithmetic instead of re-parsing the key it was filed under.
//!
//! Reference (Firestorm, read-only): `indra/newview/llexperiencelog.cpp`
//! (`handleExperienceMessage`, `notify`, `getPermissionString`, `eraseExpired`),
//! `llpanelexperiencelog.cpp` (the list this feeds).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, UtcOffset};
use tracing::{info, warn};

use sl_client_bevy::{ExperienceEvent, ExperienceEventPermission, ExperienceKey, SlEvent};
use sl_client_bevy::{SlSessionEvent, Uuid};
use sl_l10n::CivilDateTime;
use sl_settings::SettingValue;

use crate::i18n::{TransArgs, Translator};
use crate::notifications::ShowNotification;
use crate::settings::ViewerSettings;

/// The per-account file the log is stored in (a sibling of the account
/// `settings.toml`) — the reference's `experience_events.xml`.
const STORE_FILE: &str = "experience_events.json";

/// The persisted-settings section both knobs live under.
const EXPERIENCES_SECTION: &[&str] = &["experiences"];

/// How many days of events to keep (the reference's `LogDays` spinner and its
/// `mMaxDays`). Zero keeps nothing.
pub const SETTING_LOG_DAYS: &str = "ExperienceLogDays";

/// Whether every recorded event raises a toast (the reference's "Notify All
/// Events" checkbox and its `mNotifyNewEvent`).
pub const SETTING_NOTIFY_ALL: &str = "NotifyAllExperienceEvents";

/// The reference's default retention window, in days (`LLExperienceLog::mMaxDays`).
const DEFAULT_LOG_DAYS: i32 = 7;

/// The reference's maximum retention window, in days (the spinner's `max_val`).
const MAX_LOG_DAYS: i32 = 14;

/// Seconds in a day, for the retention arithmetic.
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// Register the experience-log settings.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        EXPERIENCES_SECTION,
        SETTING_LOG_DAYS,
        SettingValue::I32(DEFAULT_LOG_DAYS),
        "How many days of experience events to keep in the per-account log \
         (0 keeps none, 14 is the reference's maximum)",
    );
    settings.register_in(
        EXPERIENCES_SECTION,
        SETTING_NOTIFY_ALL,
        SettingValue::Bool(false),
        "Raise a toast for every experience event, not just record it in the log",
    );
}

/// One recorded experience event.
///
/// The wire report's fields, plus the two the log owns: when it happened and how
/// many consecutive identical reports it stands for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedExperienceEvent {
    /// The experience that acted.
    pub experience_id: ExperienceKey,
    /// The owner of the object whose script acted.
    pub owner_id: Uuid,
    /// The permission exercised, or `None` when the report named none.
    pub permission: Option<ExperienceEventPermission>,
    /// Whether the acting object was an attachment on the agent.
    pub is_attachment: bool,
    /// The name of the object whose script acted.
    pub object_name: String,
    /// The name of the parcel it happened on.
    pub parcel_name: String,
    /// When the most recent of the reports this entry stands for arrived, as a
    /// Unix timestamp in seconds.
    pub unix: i64,
    /// How many consecutive identical reports this entry stands for — 1 for a
    /// row that has not repeated.
    pub count: u32,
}

impl LoggedExperienceEvent {
    /// Whether `report` is the same event as this entry, by the five fields the
    /// reference compares when it decides to coalesce.
    fn same_event_as(&self, report: &ExperienceEvent) -> bool {
        self.experience_id == report.experience_id
            && self.owner_id == report.owner_id
            && self.permission == report.permission
            && self.object_name == report.object_name
            && self.parcel_name == report.parcel_name
    }
}

/// The log: the kept entries (oldest first), the resolved file path, and the
/// load / flush bookkeeping.
///
/// [`Default`] is written out rather than derived because [`UtcOffset`] has
/// none: the log starts at UTC and adopts the machine's offset when it loads,
/// which is also the fallback on a platform that will not report one.
#[derive(Resource, Debug)]
pub struct ExperienceLog {
    /// The kept entries, oldest first.
    entries: Vec<LoggedExperienceEvent>,
    /// The local UTC offset, captured once so the day boundaries a coalesce
    /// tests against do not shift mid-session.
    offset: UtcOffset,
    /// The per-account file path, resolved at login; `None` until then (and when
    /// the file could not be made safe to replace, which disables writing).
    path: Option<PathBuf>,
    /// Whether the on-disk file has been read — a once-per-session load.
    loaded: bool,
    /// Whether [`entries`](Self::entries) changed since the last flush.
    dirty: bool,
    /// Bumped on any change a list rebuild must react to.
    revision: u64,
    /// The write in flight, if any. Holding it is what serializes the writes.
    writing: Option<Task<std::io::Result<()>>>,
}

impl Default for ExperienceLog {
    /// An empty log at UTC, before login has said where its file is.
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            offset: UtcOffset::UTC,
            path: None,
            loaded: false,
            dirty: false,
            revision: 0,
            writing: None,
        }
    }
}

impl ExperienceLog {
    /// The kept entries, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[LoggedExperienceEvent] {
        &self.entries
    }

    /// A revision that moves whenever the entries do, for a list rebuild to
    /// watch.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// The local civil date and time `unix` fell on, for a list row to render
    /// through [`Translator::datetime`]. `None` for a timestamp outside the
    /// representable range, which a row then shows without a time rather than
    /// inventing one.
    #[must_use]
    pub fn civil_local(&self, unix: i64) -> Option<CivilDateTime> {
        let when = OffsetDateTime::from_unix_timestamp(unix)
            .ok()?
            .to_offset(self.offset);
        Some(CivilDateTime {
            year: when.year(),
            month: u8::from(when.month()),
            day: when.day(),
            hour: when.hour(),
            minute: when.minute(),
            second: when.second(),
        })
    }

    /// Drop every entry (the reference's Clear button).
    pub fn clear(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.entries.clear();
        self.touch();
    }

    /// Bump the revision and mark the log for writing.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
    }

    /// Record `report`, coalescing it onto the last entry when it is the same
    /// event on the same local day, and return the entry as it now stands — the
    /// value a notification is raised from.
    ///
    /// `now` is the arrival time as a Unix timestamp. The returned entry is a
    /// clone rather than a borrow so the caller can raise its toast without
    /// holding the log borrowed.
    fn record(&mut self, report: &ExperienceEvent, now: i64) -> LoggedExperienceEvent {
        let offset = self.offset;
        if let Some(last) = self.entries.last_mut()
            && last.same_event_as(report)
            && same_local_day(last.unix, now, offset)
        {
            last.count = last.count.saturating_add(1);
            last.unix = now;
            let coalesced = last.clone();
            self.touch();
            return coalesced;
        }
        let entry = LoggedExperienceEvent {
            experience_id: report.experience_id,
            owner_id: report.owner_id,
            permission: report.permission,
            is_attachment: report.is_attachment,
            object_name: report.object_name.clone(),
            parcel_name: report.parcel_name.clone(),
            unix: now,
            count: 1,
        };
        self.entries.push(entry.clone());
        self.touch();
        entry
    }

    /// Drop entries older than `max_days` days before `now`. A window of zero
    /// keeps nothing, which is how the reference spells the log being off.
    fn prune(&mut self, now: i64, max_days: i32) {
        let days = i64::from(max_days.max(0));
        let cutoff = now.saturating_sub(days.saturating_mul(SECONDS_PER_DAY));
        let before = self.entries.len();
        self.entries.retain(|entry| entry.unix >= cutoff);
        if self.entries.len() != before {
            self.touch();
        }
    }
}

/// Whether two Unix timestamps fall on the same local calendar day — the
/// boundary the reference's day-keyed map puts between two otherwise identical
/// events.
fn same_local_day(first: i64, second: i64, offset: UtcOffset) -> bool {
    let local = |unix: i64| {
        OffsetDateTime::from_unix_timestamp(unix)
            .map(|when| when.to_offset(offset).date())
            .ok()
    };
    match (local(first), local(second)) {
        // A timestamp outside the representable range cannot be shown to be the
        // same day as anything, so it starts a new row rather than merging into
        // one it might not belong to.
        (Some(first_day), Some(second_day)) => first_day == second_day,
        _unrepresentable => false,
    }
}

/// The plugin owning the experience log.
#[derive(Debug)]
pub struct ExperienceLogPlugin;

impl Plugin for ExperienceLogPlugin {
    /// Register the log resource and its load / ingest / flush systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<ExperienceLog>().add_systems(
            Update,
            (
                load_experience_log,
                ingest_experience_events,
                flush_experience_log,
            )
                .chain(),
        );
    }
}

/// Once the per-account directory resolves (post login), read the log file and
/// prune what has expired since it was written. Runs once.
fn load_experience_log(mut log: ResMut<ExperienceLog>, settings: Option<Res<ViewerSettings>>) {
    if log.loaded {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    let Some(account_dir) = Some(&*settings)
        .filter(|settings| settings.account_loaded())
        .and_then(|settings| settings.account_dir())
    else {
        return;
    };
    let path = account_dir.join(STORE_FILE);
    log.loaded = true;
    log.offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    // `log.path` is what licenses every later flush, so it is set only once the
    // file is known to be ours to replace.
    let saved = match read_store(&path) {
        StoreFile::Absent => {
            log.path = Some(path);
            Vec::new()
        }
        StoreFile::Loaded(entries) => {
            log.path = Some(path);
            entries
        }
        StoreFile::Unreadable => {
            log.path = rescue_unreadable_store(&path);
            Vec::new()
        }
    };
    log.entries = saved;
    log.prune(now_unix(), log_days(&settings));
    // A load is not a change to write back; only the prune above may be.
    log.revision = log.revision.wrapping_add(1);
}

/// Record each arriving [`SlSessionEvent::ExperienceEvent`], prune what the
/// window no longer covers, and raise its toast when the notify-all setting is
/// on.
fn ingest_experience_events(
    mut events: MessageReader<SlEvent>,
    mut log: ResMut<ExperienceLog>,
    settings: Option<Res<ViewerSettings>>,
    translator: Translator,
    mut toasts: MessageWriter<ShowNotification>,
) {
    let Some(settings) = settings else {
        return;
    };
    let notify = notify_all(&settings);
    let max_days = log_days(&settings);
    for event in events.read() {
        let SlSessionEvent::ExperienceEvent(report) = &event.0 else {
            continue;
        };
        let now = now_unix();
        let entry = log.record(report, now);
        log.prune(now, max_days);
        if notify {
            toasts.write(notification_for(&entry, &translator));
        }
    }
}

/// The toast one recorded event raises: the attachment template when the acting
/// object was worn, the object template otherwise, with the reference's five
/// substitutions.
fn notification_for(entry: &LoggedExperienceEvent, translator: &Translator) -> ShowNotification {
    let template = if entry.is_attachment {
        "ExperienceEventAttachment"
    } else {
        "ExperienceEvent"
    };
    ShowNotification::new(template)
        .arg(
            "EventType",
            permission_description(entry.permission, translator),
        )
        .arg("public_id", entry.experience_id.uuid().to_string())
        .arg("OwnerID", entry.owner_id.to_string())
        .arg("ObjectName", entry.object_name.clone())
        .arg("ParcelName", entry.parcel_name.clone())
}

/// The long description of a permission, as a notification body reads it
/// ("attach to your avatar") — the reference's
/// `getPermissionString(.., "ExperiencePermission")`.
#[must_use]
pub fn permission_description(
    permission: Option<ExperienceEventPermission>,
    translator: &Translator,
) -> String {
    permission_string(permission, translator, "experience-permission")
}

/// The short label of a permission, as an events-list column shows it
/// ("Attach") — the reference's
/// `getPermissionString(.., "ExperiencePermissionShort")`.
#[must_use]
pub fn permission_short(
    permission: Option<ExperienceEventPermission>,
    translator: &Translator,
) -> String {
    permission_string(permission, translator, "experience-permission-short")
}

/// Resolve a permission's description under `prefix`, which picks the long or
/// the short family.
///
/// The nine named cases each have their own key. Everything else takes the
/// family's `-unknown` key with the raw code interpolated — including a report
/// that named **no** permission, which is spelled `?`. The reference has the
/// same two outcomes but reaches the second by accident: its lookup falls
/// through to the literal key name for an unnamed code, and to a missing-string
/// marker when the field is absent altogether.
fn permission_string(
    permission: Option<ExperienceEventPermission>,
    translator: &Translator,
    prefix: &str,
) -> String {
    let suffix = match permission {
        Some(ExperienceEventPermission::TakeControls) => "take-controls",
        Some(ExperienceEventPermission::TriggerAnimation) => "trigger-animation",
        Some(ExperienceEventPermission::Attach) => "attach",
        Some(ExperienceEventPermission::TrackCamera) => "track-camera",
        Some(ExperienceEventPermission::ControlCamera) => "control-camera",
        Some(ExperienceEventPermission::Teleport) => "teleport",
        Some(ExperienceEventPermission::JoinExperience) => "join-experience",
        Some(ExperienceEventPermission::ForceSit) => "force-sit",
        Some(ExperienceEventPermission::ChangeEnvironment) => "change-environment",
        Some(ExperienceEventPermission::Other(_)) | None => {
            let code = permission.map_or_else(
                || "?".to_owned(),
                |permission| permission.code().to_string(),
            );
            return translator.format(
                &format!("{prefix}-unknown"),
                &TransArgs::new().text("permission", &code),
            );
        }
    };
    translator.get(&format!("{prefix}-{suffix}"))
}

/// The retention window in days, clamped to the range the reference's spinner
/// allows so a hand-edited setting cannot keep an unbounded log.
fn log_days(settings: &ViewerSettings) -> i32 {
    settings
        .store()
        .get_i32(SETTING_LOG_DAYS)
        .unwrap_or(DEFAULT_LOG_DAYS)
        .clamp(0, MAX_LOG_DAYS)
}

/// Whether every event should raise a toast.
fn notify_all(settings: &ViewerSettings) -> bool {
    settings
        .store()
        .get_bool(SETTING_NOTIFY_ALL)
        .unwrap_or(false)
}

/// The current Unix time in seconds (`0` on the pre-1970 impossibility).
fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// What reading the store file found — and specifically, whether the empty list
/// it hands back means *there was nothing* or *we could not tell*. The log is
/// the only copy of what an experience did, so the two must not be confused
/// (the rule [`crate::notification_persist`] documents).
#[derive(Debug)]
enum StoreFile {
    /// No file yet — the first-run case, and the only legitimate empty.
    Absent,
    /// The file parsed. Its entries, oldest first (possibly none).
    Loaded(Vec<LoggedExperienceEvent>),
    /// The file is there but could not be read or parsed, so its contents are
    /// unknown and not ours to overwrite.
    Unreadable,
}

/// Read the log from `path`, distinguishing a missing file from an unreadable
/// one.
fn read_store(path: &Path) -> StoreFile {
    if !path.exists() {
        return StoreFile::Absent;
    }
    match fs_err::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<Vec<LoggedExperienceEvent>>(&contents) {
            Ok(entries) => {
                info!(count = entries.len(), path = %path.display(), "loaded the experience log");
                StoreFile::Loaded(entries)
            }
            Err(error) => {
                warn!(path = %path.display(), %error, "malformed experience log");
                StoreFile::Unreadable
            }
        },
        Err(error) => {
            warn!(path = %path.display(), %error, "could not read the experience log");
            StoreFile::Unreadable
        }
    }
}

/// Preserve an unreadable log, and say whether writing to `path` is safe
/// afterwards. If the bytes cannot be moved out of the way, the only remaining
/// way not to destroy them is to write nothing for the rest of the session.
fn rescue_unreadable_store(path: &Path) -> Option<PathBuf> {
    match sl_settings::atomic_file::move_aside(path) {
        Ok(aside) => {
            warn!(
                path = %path.display(),
                aside = %aside.display(),
                "unreadable experience log moved aside; starting from empty"
            );
            Some(path.to_path_buf())
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                %error,
                "unreadable experience log could not be moved aside; logging is disabled for \
                 this session rather than overwriting it"
            );
            None
        }
    }
}

/// Write the log to disk when it has changed, once its path is known — atomic,
/// on the [`IoTaskPool`], and one write in flight at a time so an older
/// serialization can never land after a newer one.
fn flush_experience_log(mut log: ResMut<ExperienceLog>) {
    if let Some(writing) = log.writing.take() {
        if writing.is_finished() {
            if let Err(error) = block_on(writing) {
                warn!(%error, "could not write the experience log");
            }
        } else {
            log.writing = Some(writing);
            return;
        }
    }
    if !log.dirty {
        return;
    }
    let Some(path) = log.path.clone() else {
        return;
    };
    let contents = match serde_json::to_string_pretty(&log.entries) {
        Ok(contents) => contents,
        // Nothing to retry: the same entries would fail again, and leaving
        // `dirty` set would busy-serialize them every frame.
        Err(error) => {
            warn!(%error, "could not serialize the experience log");
            log.dirty = false;
            return;
        }
    };
    // Cleared now, not on completion: entries changing while the write is in
    // flight must re-dirty the log so the next flush writes the newer state.
    log.dirty = false;
    log.writing = Some(
        IoTaskPool::get()
            .spawn(async move { sl_settings::atomic_file::write_atomically(&path, &contents) }),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        ExperienceLog, LoggedExperienceEvent, SECONDS_PER_DAY, StoreFile, read_store,
        rescue_unreadable_store, same_local_day,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ExperienceEvent, ExperienceEventPermission, ExperienceKey, Uuid};
    use time::UtcOffset;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A unique throwaway directory under the system temp dir (the crate has no
    /// `tempfile` dependency; this mirrors sl-settings' test helper).
    fn tempdir(label: &str) -> Result<std::path::PathBuf, TestError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-{label}-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// A report from `object`, under the fixed experience, exercising `permission`.
    fn report(object: &str, permission: ExperienceEventPermission) -> ExperienceEvent {
        ExperienceEvent {
            experience_id: ExperienceKey::from(Uuid::from_u128(0xe0)),
            owner_id: Uuid::from_u128(0x0a),
            permission: Some(permission),
            is_attachment: false,
            object_name: object.to_owned(),
            parcel_name: "The Back Forty".to_owned(),
        }
    }

    /// A log whose day boundaries are UTC, so the fixtures' timestamps mean what
    /// they say wherever the test runs.
    fn log() -> ExperienceLog {
        ExperienceLog {
            offset: UtcOffset::UTC,
            ..ExperienceLog::default()
        }
    }

    /// **The coalescing rule.** A repeat of the same event on the same day is
    /// one row with a count, not two rows — and the returned entry is the merged
    /// one, so the toast it raises says what the row now says.
    #[test]
    fn a_repeat_on_the_same_day_bumps_the_count() {
        let mut log = log();
        let event = report("Ride Controller", ExperienceEventPermission::ForceSit);
        let first = log.record(&event, 1_000);
        assert_eq!(first.count, 1);
        let second = log.record(&event, 1_060);
        assert_eq!(second.count, 2);
        assert_eq!(second.unix, 1_060, "the row moves to the latest report");
        assert_eq!(log.entries().len(), 1);
    }

    /// **The same event on a *different* day is a different row.** The
    /// reference files events under a day key, so a boundary crossed between two
    /// otherwise identical reports separates them.
    #[test]
    fn a_repeat_on_the_next_day_starts_a_new_row() {
        let mut log = log();
        let event = report("Ride Controller", ExperienceEventPermission::ForceSit);
        drop(log.record(&event, 0));
        drop(log.record(&event, SECONDS_PER_DAY));
        assert_eq!(log.entries().len(), 2);
        assert!(log.entries().iter().all(|entry| entry.count == 1));
    }

    /// **Only the last entry is a coalescing candidate**, as in the reference:
    /// an interleaved second object means the first one's next report appends.
    #[test]
    fn an_interleaved_event_breaks_the_run() {
        let mut log = log();
        let ride = report("Ride Controller", ExperienceEventPermission::ForceSit);
        let door = report("Front Door", ExperienceEventPermission::Teleport);
        drop(log.record(&ride, 1_000));
        drop(log.record(&door, 1_010));
        drop(log.record(&ride, 1_020));
        assert_eq!(log.entries().len(), 3);
    }

    /// A differing field in the compared tuple is a different event, even
    /// seconds apart — here the permission, which is what distinguishes "it sat
    /// you" from "it teleported you" for the same object.
    #[test]
    fn a_different_permission_is_a_different_event() {
        let mut log = log();
        drop(log.record(
            &report("Ride Controller", ExperienceEventPermission::ForceSit),
            1_000,
        ));
        drop(log.record(
            &report("Ride Controller", ExperienceEventPermission::Teleport),
            1_001,
        ));
        assert_eq!(log.entries().len(), 2);
    }

    /// **The retention window.** Entries older than the window are dropped and
    /// newer ones kept; a window of zero keeps nothing at all, which is how the
    /// reference spells the log being off.
    #[test]
    fn pruning_drops_what_the_window_no_longer_covers() {
        let mut log = log();
        let now = 100 * SECONDS_PER_DAY;
        let event = report("Ride Controller", ExperienceEventPermission::Attach);
        drop(log.record(&event, now - 8 * SECONDS_PER_DAY));
        // A day apart, so this is a second row rather than a coalesce.
        drop(log.record(&event, now - 2 * SECONDS_PER_DAY));
        log.prune(now, 7);
        assert_eq!(log.entries().len(), 1);
        assert_eq!(
            log.entries().first().map(|entry| entry.unix),
            Some(now - 2 * SECONDS_PER_DAY)
        );

        log.prune(now, 0);
        assert!(log.entries().is_empty(), "a zero-day window keeps nothing");
    }

    /// A prune that drops nothing must not dirty the log — otherwise every
    /// arriving event would rewrite the file even when the window changed
    /// nothing.
    #[test]
    fn a_prune_that_drops_nothing_is_not_a_change() {
        let mut log = log();
        drop(log.record(
            &report("Ride Controller", ExperienceEventPermission::Attach),
            1_000,
        ));
        log.dirty = false;
        let revision = log.revision();
        log.prune(1_000, 7);
        assert!(!log.dirty);
        assert_eq!(log.revision(), revision);
    }

    /// Clearing empties the log and moves the revision; clearing an already
    /// empty log changes nothing.
    #[test]
    fn clearing_empties_the_log_once() {
        let mut log = log();
        drop(log.record(
            &report("Ride Controller", ExperienceEventPermission::Attach),
            1_000,
        ));
        log.clear();
        assert!(log.entries().is_empty());
        log.dirty = false;
        let revision = log.revision();
        log.clear();
        assert!(!log.dirty, "clearing an empty log is not a change");
        assert_eq!(log.revision(), revision);
    }

    /// The local-day test is what separates two otherwise identical reports, so
    /// it has to actually use the offset it is given rather than comparing UTC
    /// days.
    #[test]
    fn the_day_boundary_follows_the_local_offset() -> Result<(), TestError> {
        // 23:30 and 00:30 UTC: one hour apart, with UTC midnight between them.
        let late = 23 * 3_600 + 1_800;
        let early = late + 3_600;
        // In UTC the midnight between them makes them two days, so two
        // otherwise identical reports would be two rows.
        assert!(!same_local_day(late, early, UtcOffset::UTC));
        // An hour east pushes *both* past local midnight into the next day, so
        // the same pair is one row.
        let east = UtcOffset::from_hms(1, 0, 0)?;
        assert!(same_local_day(late, early, east));
        // An hour west pulls *both* back before it, into the previous day —
        // one row again, reached from the other side.
        let west = UtcOffset::from_hms(-1, 0, 0)?;
        assert!(same_local_day(late, early, west));
        // A timestamp that cannot be represented is never the same day as
        // anything, so it starts a row rather than merging into one.
        assert!(!same_local_day(i64::MIN, i64::MIN, UtcOffset::UTC));
        Ok(())
    }

    /// The entries round-trip through JSON, so a reloaded log is the log that
    /// was written.
    #[test]
    fn entries_round_trip_through_json() -> Result<(), TestError> {
        let original = vec![LoggedExperienceEvent {
            experience_id: ExperienceKey::from(Uuid::from_u128(0xe0)),
            owner_id: Uuid::from_u128(0x0a),
            permission: Some(ExperienceEventPermission::Other(42)),
            is_attachment: true,
            object_name: "Ride Controller".to_owned(),
            parcel_name: "The Back Forty".to_owned(),
            unix: 1_700_000_000,
            count: 3,
        }];
        let json = serde_json::to_string(&original)?;
        let parsed: Vec<LoggedExperienceEvent> = serde_json::from_str(&json)?;
        assert_eq!(parsed, original);
        Ok(())
    }

    /// **An unreadable log is not an empty one.** The log is the only record of
    /// what an experience did, so a parse failure must not be written back over.
    #[test]
    fn an_unreadable_store_is_distinguished_from_an_empty_one() -> Result<(), TestError> {
        let dir = tempdir("read")?;
        assert!(matches!(
            read_store(&dir.join("absent.json")),
            StoreFile::Absent
        ));

        let empty = dir.join("empty.json");
        fs_err::write(&empty, "[]")?;
        assert!(matches!(read_store(&empty), StoreFile::Loaded(list) if list.is_empty()));

        let wrong_shape = dir.join("wrong-shape.json");
        fs_err::write(&wrong_shape, r#"{"entries": 3}"#)?;
        assert!(matches!(read_store(&wrong_shape), StoreFile::Unreadable));

        // A store that cannot be moved aside — its parent is gone — leaves the
        // session with no path, so nothing can overwrite it later.
        assert_eq!(
            rescue_unreadable_store(&dir.join("vanished").join(super::STORE_FILE)),
            None
        );

        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }
}
