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

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
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
    let saved = read_store(&path);
    store.path = Some(path);
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

/// Read the persisted list from `path`, tolerating a missing file (the first-run
/// case) and a malformed one (logged, treated as empty — a corrupt store must not
/// abort login).
fn read_store(path: &std::path::Path) -> Vec<PersistedKind> {
    if !path.exists() {
        return Vec::new();
    }
    match fs_err::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<Vec<PersistedKind>>(&contents) {
            Ok(list) => {
                info!(count = list.len(), path = %path.display(), "loaded persisted notifications");
                list
            }
            Err(error) => {
                warn!(path = %path.display(), %error, "malformed persisted-notification store; ignoring");
                Vec::new()
            }
        },
        Err(error) => {
            warn!(path = %path.display(), %error, "could not read persisted-notification store");
            Vec::new()
        }
    }
}

/// Write the store to disk when it has changed, once its path is known
/// (best-effort — a write failure is logged, never fatal).
fn flush_persistent_notifications(mut store: ResMut<PersistentNotificationStore>) {
    if !store.dirty {
        return;
    }
    let Some(path) = store.path.clone() else {
        return;
    };
    let list: Vec<&PersistedKind> = store.entries.values().collect();
    match serde_json::to_string_pretty(&list) {
        Ok(contents) => {
            if let Err(error) = fs_err::write(&path, contents) {
                warn!(path = %path.display(), %error, "could not write persisted-notification store");
            } else {
                debug!(count = list.len(), "flushed persisted notifications");
                store.dirty = false;
            }
        }
        Err(error) => warn!(%error, "could not serialize persisted notifications"),
    }
}

#[cfg(test)]
mod tests {
    use super::{PersistedKind, PersistentNotificationStore};
    use crate::notifications::NotificationManager;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

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
}
