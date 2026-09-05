//! The **persistent-notification store** (`viewer-notification-persistence`): the
//! reference `LLPersistentNotificationStorage`, which saves the *open*
//! (unacknowledged) notifications to a per-account file and re-displays them on
//! the next login.
//!
//! # Why
//!
//! A sticky notification the user never answered — a group notice
//! (`crate::group_notice`), an alert — must survive a relog: the reference
//! writes every `persist="true"` notification still on its `"Persistent"` channel
//! to `open_notifications_<grid>.xml`, reloads them at startup, and removes each
//! one when the user finally responds. "Seen" is thus a **client-side** fact
//! (the user closed it), not a server acknowledgement — a plain group notice
//! sends nothing on close.
//!
//! # How it plugs in
//!
//! - A producer persists a notification by writing a [`PersistNotification`] for
//!   its [`NotificationId`]: [`crate::notification_host`] does so for every sticky
//!   `persist` catalogue toast it raises, and `crate::group_notice` does so for
//!   each group-notice card (which is a bespoke [`PersistedKind::Custom`] payload,
//!   not a catalogue entry).
//! - The store forgets an entry when its
//!   [`NotificationResponse`] arrives —
//!   the user answered, so it must not reappear.
//! - At login, once the per-account directory resolves, `load_persisted` reads
//!   the file and **re-raises** each entry: a catalogue entry via
//!   [`ShowNotification`], a custom entry via a [`ReloadPersistedNotification`]
//!   the owning module rebuilds its card from.
//!
//! The store keys entries by the live [`NotificationId`] (session-local, not
//! serialized); the file is just the ordered list of payloads.
//!
//! # What the file is allowed to lose
//!
//! Nothing, is the short answer, and it takes three rules to mean it
//! (`viewer-audit-notification-store-overwrite`). The contents are the only copy
//! of something the user has not answered yet, so every path that could replace
//! them is narrowed:
//!
//! - **An unreadable file is never treated as an empty one.** `read_store`
//!   returns a `StoreFile`, not a `Vec`, so "there is no file" and "I could
//!   not parse it" cannot be confused. The second is preserved by
//!   `rescue_unreadable_store`; if it cannot even be preserved, the session
//!   gets no store path at all and writes nothing, which is the only remaining
//!   way not to destroy it.
//! - **A write is atomic**, via `sl_settings::atomic_file::write_atomically` —
//!   a temporary sibling renamed over the target, so a crash mid-write leaves
//!   the previous file whole rather than a truncated one the next run would
//!   then rescue.
//! - **Writes are serialized**, one in flight at a time, so an older
//!   serialization cannot land after a newer one.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::notifications::{
    NotificationArgs, NotificationId, NotificationResponse, ShowNotification, template,
};
use crate::settings::ViewerSettings;

/// The per-account file the open notifications are stored in (a sibling of the
/// account `settings.toml`). Unlike the reference's per-grid name, our account
/// directory is already per-grid + per-avatar, so the bare name suffices.
const STORE_FILE: &str = "open_notifications.json";

/// One persisted notification's payload — enough to re-raise it after a relog.
/// The [`NotificationId`] is **not** stored: it is session-local, reassigned when
/// the notification is re-raised.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PersistedKind {
    /// A catalogue notification ([`crate::notifications::NOTIFICATIONS`]),
    /// re-raised via [`ShowNotification`] with its original substitutions.
    Catalogue {
        /// The catalogue template name.
        template: String,
        /// The `[KEY]` substitution pairs, in order.
        args: Vec<(String, String)>,
        /// An already-localized body override, if the raise carried one.
        body: Option<String>,
        /// The `unique`-dedup context, if any.
        context: Option<String>,
    },
    /// A bespoke-content notification (the group-notice card): a `renderer` id and
    /// a flat string map the owning module (`crate::group_notice`) rebuilds its
    /// card from on reload.
    Custom {
        /// The renderer id the owning module matches on
        /// (e.g. `"group-notice"`).
        renderer: String,
        /// The renderer's serialized fields.
        data: BTreeMap<String, String>,
    },
}

/// A request to persist a notification under its live [`NotificationId`] — written
/// by whoever raised it (the host for a catalogue toast, `crate::group_notice`
/// for a card). Re-persisting the same id just overwrites its payload.
#[derive(Message, Debug, Clone)]
pub struct PersistNotification {
    /// The live notification this persists.
    pub id: NotificationId,
    /// The payload to re-raise it from after a relog.
    pub kind: PersistedKind,
}

/// A reloaded [`PersistedKind::Custom`] entry, for the owning module to rebuild
/// its bespoke card from — the reload counterpart of the [`ShowNotification`] a
/// catalogue entry re-raises through.
#[derive(Message, Debug, Clone)]
pub struct ReloadPersistedNotification {
    /// The renderer id (e.g. `"group-notice"`); a module ignores others.
    pub renderer: String,
    /// The renderer's serialized fields, as saved.
    pub data: BTreeMap<String, String>,
}

/// The store resource: the live open notifications, the resolved file path, and
/// the load / flush bookkeeping.
#[derive(Resource, Debug, Default)]
pub(crate) struct PersistentNotificationStore {
    /// The open (unacknowledged) notifications, keyed by live id. `NotificationId`
    /// is monotonic, so a [`BTreeMap`] iterates in raise order — a stable file.
    entries: BTreeMap<NotificationId, PersistedKind>,
    /// The per-account file path, resolved at login; `None` until then (and when
    /// the platform has no per-avatar directory, disabling persistence).
    path: Option<PathBuf>,
    /// Whether the on-disk file has been read and its entries re-raised — a
    /// once-per-session load.
    loaded: bool,
    /// Whether [`entries`](Self::entries) changed since the last flush.
    dirty: bool,
    /// The write in flight, if any.
    ///
    /// Holding it is what serializes the writes: a flush that finds this
    /// occupied does not start a second one, so two writes can never land out
    /// of order. `None` between writes, and in a test app with no
    /// [`IoTaskPool`], where the store is driven directly.
    writing: Option<Task<std::io::Result<()>>>,
}

impl PersistentNotificationStore {
    /// Insert / replace the payload for `id`, marking the store dirty.
    fn record(&mut self, id: NotificationId, kind: PersistedKind) {
        self.entries.insert(id, kind);
        self.dirty = true;
    }

    /// Forget `id` (the user answered it), marking the store dirty if it was held.
    fn forget(&mut self, id: NotificationId) {
        if self.entries.remove(&id).is_some() {
            self.dirty = true;
        }
    }
}

/// The plugin: registers the messages, the store, and the record / forget / load /
/// flush systems.
#[derive(Debug)]
pub struct NotificationPersistPlugin;

impl Plugin for NotificationPersistPlugin {
    /// Register the persistence messages, the store resource, and its systems.
    fn build(&self, app: &mut App) {
        app.add_message::<PersistNotification>()
            .add_message::<ReloadPersistedNotification>()
            .init_resource::<PersistentNotificationStore>()
            .add_systems(
                Update,
                (
                    load_persisted,
                    record_persisted_notifications,
                    forget_answered_notifications,
                    flush_persistent_notifications,
                )
                    .chain(),
            );
    }
}

/// Record each [`PersistNotification`] into the store.
fn record_persisted_notifications(
    mut requests: MessageReader<PersistNotification>,
    mut store: ResMut<PersistentNotificationStore>,
) {
    for request in requests.read() {
        store.record(request.id, request.kind.clone());
    }
}

/// Forget a persisted notification once the user answers it (any
/// [`NotificationResponse`] — a button, a modal choice, or a close ×): "seen" is
/// the active close, and a seen notice must not reappear next login.
fn forget_answered_notifications(
    mut responses: MessageReader<NotificationResponse>,
    mut store: ResMut<PersistentNotificationStore>,
) {
    for response in responses.read() {
        store.forget(response.id);
    }
}

/// Once the per-account directory resolves (post login), read the store file and
/// re-raise each open notification — a catalogue entry through [`ShowNotification`],
/// a custom entry through a [`ReloadPersistedNotification`]. Runs once.
fn load_persisted(
    mut store: ResMut<PersistentNotificationStore>,
    settings: Option<Res<ViewerSettings>>,
    mut show: MessageWriter<ShowNotification>,
    mut reload: MessageWriter<ReloadPersistedNotification>,
) {
    if store.loaded {
        return;
    }
    let Some(account_dir) = settings
        .as_deref()
        .filter(|settings| settings.account_loaded())
        .and_then(ViewerSettings::account_dir)
    else {
        return;
    };
    let path = account_dir.join(STORE_FILE);
    store.loaded = true;
    // `store.path` is what licenses every later flush, so it is set only once
    // the file is known to be ours to replace.
    let saved = match read_store(&path) {
        StoreFile::Absent => {
            store.path = Some(path);
            Vec::new()
        }
        StoreFile::Loaded(list) => {
            store.path = Some(path);
            list
        }
        StoreFile::Unreadable => {
            store.path = rescue_unreadable_store(&path);
            Vec::new()
        }
    };
    for kind in saved {
        raise_persisted(kind, &mut show, &mut reload);
    }
}

/// Re-raise one persisted payload: resolve a catalogue entry's template back to
/// its `&'static` name and emit a [`ShowNotification`]; hand a custom entry to its
/// renderer via [`ReloadPersistedNotification`]. An unknown catalogue template
/// (renamed / removed since the file was written) is dropped with a warning.
fn raise_persisted(
    kind: PersistedKind,
    show: &mut MessageWriter<ShowNotification>,
    reload: &mut MessageWriter<ReloadPersistedNotification>,
) {
    match kind {
        PersistedKind::Catalogue {
            template: name,
            args,
            body,
            context,
        } => {
            let Some(found) = template(&name) else {
                warn!(
                    template = name,
                    "persisted notification: unknown template, dropping"
                );
                return;
            };
            show.write(ShowNotification {
                template: found.name,
                args: NotificationArgs::from_pairs(args),
                body,
                context,
            });
        }
        PersistedKind::Custom { renderer, data } => {
            reload.write(ReloadPersistedNotification { renderer, data });
        }
    }
}

/// What reading the store file found — and specifically, whether the empty list
/// it hands back means *there was nothing* or *we could not tell*.
///
/// The distinction is the whole bug. Collapsing both into `Vec::new()` is what
/// let a parse error become data loss: the caller could not know the difference,
/// took the file as empty, and the next flush wrote that emptiness back over
/// every notification the user had not answered yet.
#[derive(Debug)]
enum StoreFile {
    /// No file yet — the first-run case, and the only *legitimate* empty.
    Absent,
    /// The file parsed. Its entries, in saved order (possibly none).
    Loaded(Vec<PersistedKind>),
    /// The file is there but could not be read or parsed. Its contents are
    /// unknown, so they are not ours to overwrite.
    Unreadable,
}

/// Read the persisted list from `path`, distinguishing a missing file from an
/// unreadable one — see [`StoreFile`]. A corrupt store must not abort login, but
/// it must not be silently *replaced* either.
fn read_store(path: &std::path::Path) -> StoreFile {
    if !path.exists() {
        return StoreFile::Absent;
    }
    match fs_err::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<Vec<PersistedKind>>(&contents) {
            Ok(list) => {
                info!(count = list.len(), path = %path.display(), "loaded persisted notifications");
                StoreFile::Loaded(list)
            }
            Err(error) => {
                warn!(path = %path.display(), %error, "malformed persisted-notification store");
                StoreFile::Unreadable
            }
        },
        Err(error) => {
            warn!(path = %path.display(), %error, "could not read persisted-notification store");
            StoreFile::Unreadable
        }
    }
}

/// Decide where an unreadable store leaves persistence: preserve the file, and
/// say whether writing to `path` is safe afterwards.
///
/// Moving it aside is what makes the session writable again — the bytes we could
/// not parse are then under a different name, so a later flush cannot reach
/// them. If even that fails we have not protected anything, and the only
/// remaining way to avoid destroying the user's unanswered notifications is to
/// **not write at all** for the rest of the session.
fn rescue_unreadable_store(path: &std::path::Path) -> Option<PathBuf> {
    match sl_settings::atomic_file::move_aside(path) {
        Ok(aside) => {
            warn!(
                path = %path.display(),
                aside = %aside.display(),
                "unreadable persisted-notification store moved aside; starting from empty"
            );
            Some(path.to_path_buf())
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                %error,
                "unreadable persisted-notification store could not be moved aside; persistence is \
                 disabled for this session rather than overwriting it"
            );
            None
        }
    }
}

/// Write the store to disk when it has changed, once its path is known.
///
/// Serializing is cheap and stays on the frame thread; the write itself goes to
/// the [`IoTaskPool`], because a synchronous whole-file write inside `Update` is
/// a frame hitch on any disk that stalls.
///
/// **At most one write is in flight at a time.** A detached task per flush would
/// have no ordering guarantee between two writes started in adjacent frames, so
/// an older serialization could land last and undo a newer one — the defect
/// `viewer-audit-settings-write-race` records in the settings path. Here a flush
/// that finds a write still running simply leaves `dirty` set and tries again
/// next frame, which both serializes the writes and coalesces a burst of
/// changes into one.
fn flush_persistent_notifications(mut store: ResMut<PersistentNotificationStore>) {
    // Collect the previous write first: until it is done, its result is unknown
    // and starting another would race it.
    if let Some(writing) = store.writing.take() {
        if writing.is_finished() {
            // Already finished, so this resolves without blocking the frame.
            if let Err(error) = block_on(writing) {
                warn!(%error, "could not write persisted-notification store");
            }
        } else {
            store.writing = Some(writing);
            return;
        }
    }

    if !store.dirty {
        return;
    }
    let Some(path) = store.path.clone() else {
        return;
    };
    // Scoped, so the borrow of `entries` ends before `dirty` is written below.
    let (count, serialized) = {
        let list: Vec<&PersistedKind> = store.entries.values().collect();
        (list.len(), serde_json::to_string_pretty(&list))
    };
    let contents = match serialized {
        Ok(contents) => contents,
        // Nothing to retry: the same entries would fail to serialize again, and
        // leaving `dirty` set would busy-serialize them every frame.
        Err(error) => {
            warn!(%error, "could not serialize persisted notifications");
            store.dirty = false;
            return;
        }
    };
    debug!(count, path = %path.display(), "flushing persisted notifications");
    // Cleared now, not on completion: `entries` changing while the write is in
    // flight must re-dirty the store, and the next flush then writes the newer
    // state after this one lands.
    store.dirty = false;
    store.writing = Some(
        IoTaskPool::get()
            .spawn(async move { sl_settings::atomic_file::write_atomically(&path, &contents) }),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        PersistedKind, PersistentNotificationStore, ReloadPersistedNotification, StoreFile,
        raise_persisted, read_store, rescue_unreadable_store,
    };
    use crate::notifications::{NotificationManager, ShowNotification};
    use bevy::ecs::system::RunSystemOnce as _;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

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

    /// A recorded notification is held until forgotten, and forgetting an unheld id
    /// is a no-op — the record / forget lifecycle the response teardown drives.
    #[test]
    fn record_then_forget_clears_the_entry() {
        let mut manager = NotificationManager::default();
        let first = manager.allocate_id();
        let second = manager.allocate_id();
        let mut store = PersistentNotificationStore::default();
        store.record(
            first,
            PersistedKind::Custom {
                renderer: "group-notice".to_owned(),
                data: BTreeMap::new(),
            },
        );
        store.record(
            second,
            PersistedKind::Custom {
                renderer: "group-notice".to_owned(),
                data: BTreeMap::new(),
            },
        );
        assert_eq!(store.entries.len(), 2);
        store.forget(first);
        assert_eq!(store.entries.len(), 1);
        assert!(store.entries.contains_key(&second));
        // Forgetting an id that is not held changes nothing.
        store.dirty = false;
        store.forget(first);
        assert!(!store.dirty);
    }

    /// The payload round-trips through JSON, so a reloaded entry rebuilds
    /// identically — the persistence guarantee.
    #[test]
    fn payload_round_trips_through_json() -> Result<(), String> {
        let mut data = BTreeMap::new();
        data.insert("group_id".to_owned(), "abc".to_owned());
        data.insert("subject".to_owned(), "Board meeting".to_owned());
        let original = vec![
            PersistedKind::Catalogue {
                template: "GenericAlert".to_owned(),
                args: vec![("MESSAGE".to_owned(), "hi".to_owned())],
                body: Some("hi".to_owned()),
                context: None,
            },
            PersistedKind::Custom {
                renderer: "group-notice".to_owned(),
                data,
            },
        ];
        let json = serde_json::to_string(&original).map_err(|error| error.to_string())?;
        let parsed: Vec<PersistedKind> =
            serde_json::from_str(&json).map_err(|error| error.to_string())?;
        assert_eq!(parsed, original);
        Ok(())
    }

    /// **An empty list and an unreadable file are not the same answer.**
    ///
    /// The bug in one sentence: `read_store` used to return `Vec::new()` for
    /// both "there is no file" and "I could not parse it", so `load_persisted`
    /// could not tell them apart, set the path anyway, and the next flush wrote
    /// the empty map over every unanswered notification. Every case is pinned
    /// here, including the two *legitimate* empties, so a future simplification
    /// back to a bare `Vec` fails rather than silently restoring the data loss.
    #[test]
    fn an_unreadable_store_is_distinguished_from_an_empty_one() -> Result<(), TestError> {
        let dir = tempdir("read")?;

        // No file at all — the first-run case.
        assert!(matches!(
            read_store(&dir.join("absent.json")),
            StoreFile::Absent
        ));

        // A file holding an empty list really is empty, and must not be
        // mistaken for corruption: nothing to rescue, nothing to warn about.
        let empty = dir.join("empty.json");
        fs_err::write(&empty, "[]")?;
        assert!(matches!(read_store(&empty), StoreFile::Loaded(list) if list.is_empty()));

        // Well-formed JSON that is not this schema.
        let wrong_shape = dir.join("wrong-shape.json");
        fs_err::write(&wrong_shape, r#"{"entries": 3}"#)?;
        assert!(matches!(read_store(&wrong_shape), StoreFile::Unreadable));

        // Truncated — what an interrupted non-atomic write leaves behind, and
        // the case `write_atomically` now prevents this store from creating.
        let truncated = dir.join("truncated.json");
        fs_err::write(&truncated, r#"[{"Custom":{"renderer":"group-"#)?;
        assert!(matches!(read_store(&truncated), StoreFile::Unreadable));

        // Readable and valid: the entries come back.
        let good = dir.join("good.json");
        let saved = vec![PersistedKind::Custom {
            renderer: "group-notice".to_owned(),
            data: BTreeMap::new(),
        }];
        fs_err::write(&good, serde_json::to_string(&saved)?)?;
        let StoreFile::Loaded(loaded) = read_store(&good) else {
            return Err("a valid store did not load".into());
        };
        assert_eq!(loaded, saved);

        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **An unreadable store is preserved, and only then written to.**
    ///
    /// The two outcomes of the rescue, which is where the fix actually bites:
    /// once the bytes are safe under another name the session may write again,
    /// and when they cannot be made safe the session must not write **at all**.
    /// The second case is the one that used to be data loss, so it is asserted
    /// as the absence of a path — the single value every later flush consults.
    #[test]
    fn rescuing_an_unreadable_store_decides_whether_writing_is_safe() -> Result<(), TestError> {
        let dir = tempdir("rescue")?;
        let path = dir.join("open_notifications.json");
        fs_err::write(&path, "not json at all")?;

        let licensed = rescue_unreadable_store(&path)
            .ok_or("a store that could be moved aside must stay writable")?;
        assert_eq!(licensed, path, "the session writes to the original name");
        assert!(!path.exists(), "the unreadable file is out of the way");
        let preserved: Vec<String> = fs_err::read_dir(&dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".corrupt-"))
            .collect();
        assert_eq!(
            preserved.len(),
            1,
            "the bytes were not preserved: {preserved:?}"
        );

        // A file that cannot be renamed — its parent no longer exists — leaves
        // the session with no path, so nothing can overwrite it later.
        let gone = dir.join("vanished").join("open_notifications.json");
        assert_eq!(
            rescue_unreadable_store(&gone),
            None,
            "a store that could not be protected must disable writing"
        );

        drop(fs_err::remove_dir_all(&dir));
        Ok(())
    }

    /// **The reload path: an entry whose template is gone is dropped, and every
    /// other entry still comes back.**
    ///
    /// A stale `Catalogue` name is the ordinary consequence of the catalogue
    /// being edited between two runs, so it must not take the rest of the file
    /// with it. The `Custom` entry alongside it is the one that would be lost if
    /// the drop were implemented as "stop reloading here".
    #[test]
    fn an_unknown_template_is_dropped_without_losing_its_neighbours() -> Result<(), TestError> {
        let mut app = App::new();
        app.add_message::<ShowNotification>()
            .add_message::<ReloadPersistedNotification>();

        let mut data = BTreeMap::new();
        data.insert("subject".to_owned(), "Board meeting".to_owned());
        let saved = vec![
            PersistedKind::Catalogue {
                template: "NotATemplateAnyMore".to_owned(),
                args: vec![("MESSAGE".to_owned(), "gone".to_owned())],
                body: None,
                context: None,
            },
            PersistedKind::Catalogue {
                template: "GenericAlert".to_owned(),
                args: vec![("MESSAGE".to_owned(), "kept".to_owned())],
                body: Some("kept".to_owned()),
                context: None,
            },
            PersistedKind::Custom {
                renderer: "group-notice".to_owned(),
                data: data.clone(),
            },
        ];

        // The catalogue must actually disagree about these two names, or the
        // test would pass without exercising the drop at all.
        assert!(crate::notifications::template("GenericAlert").is_some());
        assert!(crate::notifications::template("NotATemplateAnyMore").is_none());

        app.world_mut()
            .run_system_once(
                move |mut show: MessageWriter<ShowNotification>,
                      mut reload: MessageWriter<ReloadPersistedNotification>| {
                    for kind in saved.clone() {
                        raise_persisted(kind, &mut show, &mut reload);
                    }
                },
            )
            .map_err(|error| format!("{error:?}"))?;

        let raised: Vec<String> = app
            .world_mut()
            .resource_mut::<Messages<ShowNotification>>()
            .drain()
            .map(|show| show.template.to_owned())
            .collect();
        assert_eq!(
            raised,
            vec!["GenericAlert".to_owned()],
            "the stale template was raised, or took the live one with it"
        );

        let reloaded: Vec<BTreeMap<String, String>> = app
            .world_mut()
            .resource_mut::<Messages<ReloadPersistedNotification>>()
            .drain()
            .map(|entry| entry.data)
            .collect();
        assert_eq!(
            reloaded,
            vec![data],
            "the custom entry after the stale one was lost"
        );
        Ok(())
    }
}
