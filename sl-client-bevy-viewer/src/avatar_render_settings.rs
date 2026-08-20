//! **Per-avatar render exceptions** (`viewer-avatar-render-settings-manager`):
//! the standing, *persisted* per-avatar overrides of the automatic complexity
//! rules — always draw this person in full whatever they cost, or never draw
//! them in full at all.
//!
//! The automatic rules ([`crate::avatar_complexity`]) answer "what does this
//! avatar cost me"; this answers "never mind the cost, I have decided about
//! *this* person". A friend who wears a heavy mesh outfit you actually want to
//! see is **Render Fully**; the one avatar at every event whose particle-emitting
//! attachments halve your frame rate is **Never Render**. Both are per person
//! rather than per session, so the decision is made once and holds — which is
//! what makes this different from the session-only intent the pie menu used to
//! record.
//!
//! # One store, one way in
//!
//! [`AvatarRenderSettings`] is the whole model: the exception list, its
//! per-account file, and the index the render decision reads. Every affordance
//! that sets an exception writes a [`RequestRenderException`] rather than
//! touching the list ([`apply_render_exception_requests`] runs the guards), the
//! same shape the derender list ([`crate::derender`]) and the mute list
//! ([`crate::mutes`]) use.
//!
//! The render decision does **not** read this resource directly: the complexity
//! model mirrors it by revision
//! ([`sync_complexity_exceptions`](crate::avatar_complexity::sync_complexity_exceptions)),
//! exactly as it mirrors the friends roster, so the per-avatar decision stays
//! one hash lookup at a crowded event.
//!
//! # Names
//!
//! An exception is a decision about a *person*, and the floater that manages it
//! has to name them — including someone who is nowhere near you, which is most
//! of the list most of the time. So each entry carries the name the surface that
//! recorded it knew, and the **live name cache** is read over it when it has an
//! answer: [`refresh_exception_names`] asks the grid for every listed avatar the
//! cache does not know and mirrors what comes back, so the floater's rebuild is
//! driven by one revision rather than a per-row poll.
//!
//! What a resolution never does is *rewrite* the stored name. A grid answers an
//! id it cannot resolve with a placeholder — the local OpenSim says
//! `Unknown UserU…` for a real account that happens to be offline — and adopting
//! that would permanently destroy the record of who the decision was about. So
//! the stored name is the deciding surface's, the mirror is the grid's, and the
//! list shows the grid's when there is one.
//!
//! # The stored numbering is the reference's
//!
//! [`RenderOverride`] persists as the reference's `VisualMuteSettings` integer
//! (`0` normally, `1` never, `2` always), so a list exported from Firestorm's
//! `avatar_render_settings.xml` ports across by transcription alone.
//!
//! Reference (Firestorm, read-only): `fsavatarrenderpersistence` (the per-account
//! `avatar_render_settings.xml` and its `LLUUID → VisualMuteSettings` map),
//! `LLVOAvatar::setVisualMuteSettings`.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{AgentKey, SlIdentity, Uuid};
use tracing::{debug, info, warn};

use crate::avatar_complexity::RenderOverride;
use crate::avatars::AvatarState;
use crate::settings::ViewerSettings;

/// The per-account file the exceptions are stored in (a sibling of the account
/// `settings.toml`, like [`crate::derender`]'s blacklist). Our account directory
/// is already per-grid and per-avatar, so the bare name suffices.
const STORE_FILE: &str = "avatar_render_settings.json";

/// How often an entry whose name has not resolved is chased up — a name request
/// is one batched round trip, and a name nobody can answer must not be asked for
/// every frame.
const NAME_REFRESH_SECONDS: f64 = 2.0;

// ---------------------------------------------------------------------------
// The model.
// ---------------------------------------------------------------------------

/// One persisted per-avatar render exception.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RenderException {
    /// The avatar this decision is about.
    pub(crate) agent: Uuid,
    /// The name the surface that recorded this decision knew them by — the
    /// fallback the list reads by until the live name cache answers, so a relog
    /// shows people rather than raw ids. Never rewritten by a resolution.
    #[serde(default)]
    pub(crate) name: String,
    /// What was decided — persisted as the reference's `VisualMuteSettings`
    /// integer so a Firestorm list ports across.
    #[serde(with = "stored_override")]
    pub(crate) setting: RenderOverride,
    /// When it was decided, as Unix epoch seconds (a plain integer, so the file
    /// needs no date parser).
    #[serde(default)]
    pub(crate) added_epoch_secs: i64,
}

/// Serde for [`RenderOverride`] as the reference's stored integer, so the file
/// is the same shape Firestorm writes (and an unknown value degrades to "no
/// exception" rather than failing the whole load).
mod stored_override {
    use serde::{Deserialize as _, Deserializer, Serializer};

    use crate::avatar_complexity::RenderOverride;

    /// Write the reference's `VisualMuteSettings` integer.
    pub(super) fn serialize<S: Serializer>(
        over: &RenderOverride,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(over.stored())
    }

    /// Read it back, mapping anything unexpected to "no exception".
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<RenderOverride, D::Error> {
        let stored = u32::deserialize(deserializer)?;
        Ok(RenderOverride::from_stored(stored))
    }
}

/// The per-avatar render exceptions: the list, its index, and its persistence.
#[derive(Resource, Debug, Default)]
pub(crate) struct AvatarRenderSettings {
    /// The exceptions, newest last.
    entries: Vec<RenderException>,
    /// The exceptions by agent — the index the complexity model mirrors.
    by_agent: HashMap<AgentKey, RenderOverride>,
    /// The live name cache's answer for each listed avatar, mirrored here by
    /// [`refresh_exception_names`] so the floater's rebuild is driven by one
    /// revision instead of polling the name cache per row. Session state: never
    /// persisted, and dropped for anyone who leaves the list.
    live_names: HashMap<AgentKey, String>,
    /// Bumped on every change to the list, so the mirror and the floater rebuild
    /// exactly when it moved.
    revision: u64,
    /// The per-account store path, resolved at login; `None` until then (and
    /// when the platform has no per-avatar directory, disabling persistence).
    path: Option<PathBuf>,
    /// Whether the on-disk list has been read — a once-per-session load.
    loaded: bool,
    /// Whether the list has changed since the last flush.
    dirty: bool,
}

impl AvatarRenderSettings {
    /// The whole list, in insertion order.
    pub(crate) fn entries(&self) -> &[RenderException] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and rebuilds
    /// when it advances.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// This avatar's standing exception (`Normal` when they have none).
    pub(crate) fn setting_of(&self, agent: AgentKey) -> RenderOverride {
        self.by_agent.get(&agent).copied().unwrap_or_default()
    }

    /// The exceptions as the flat map the complexity model mirrors.
    pub(crate) fn overrides(&self) -> HashMap<AgentKey, RenderOverride> {
        self.by_agent.clone()
    }

    /// What the live name cache last answered for this avatar, if anything —
    /// what the floater shows in preference to the stored name.
    pub(crate) fn live_name(&self, agent: AgentKey) -> Option<&str> {
        self.live_names.get(&agent).map(String::as_str)
    }

    /// Mirror the live name cache's answer for `agent`, bumping the revision (so
    /// the floater rebuilds) when it says something new.
    fn note_live_name(&mut self, agent: AgentKey, name: &str) {
        if name.is_empty() || self.live_names.get(&agent).map(String::as_str) == Some(name) {
            return;
        }
        let _previous = self.live_names.insert(agent, name.to_owned());
        self.revision = self.revision.wrapping_add(1);
    }

    /// Record (or clear) an exception. `Normal` **removes** the entry — the
    /// reference's own rule, and the only reading that makes sense: "let the
    /// automatic rules decide" is the absence of a decision, not a third one.
    ///
    /// Setting an exception an avatar already has is a no-op, so a repeated pie
    /// pick neither re-stamps the date nor dirties the file.
    pub(crate) fn set(&mut self, agent: AgentKey, setting: RenderOverride, name: &str, now: i64) {
        let held = self
            .entries
            .iter_mut()
            .find(|entry| entry.agent == agent.uuid());
        match (setting, held) {
            (RenderOverride::Normal, None) => return,
            (RenderOverride::Normal, Some(_entry)) => {
                self.entries.retain(|entry| entry.agent != agent.uuid());
            }
            (_other, Some(entry)) => {
                let unchanged = entry.setting == setting;
                entry.setting = setting;
                if !name.is_empty() && entry.name != name {
                    name.clone_into(&mut entry.name);
                } else if unchanged {
                    return;
                }
            }
            (_other, None) => self.entries.push(RenderException {
                agent: agent.uuid(),
                name: name.to_owned(),
                setting,
                added_epoch_secs: now,
            }),
        }
        self.dirty = true;
        self.reindex();
    }

    /// Rebuild the by-agent index and bump the revision.
    fn reindex(&mut self) {
        self.by_agent = self
            .entries
            .iter()
            .map(|entry| (AgentKey::from(entry.agent), entry.setting))
            .collect();
        // The live-name mirror exists only to label the list, so it follows it.
        self.live_names
            .retain(|agent, _name| self.by_agent.contains_key(agent));
        self.revision = self.revision.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Requests.
// ---------------------------------------------------------------------------

/// Ask for an avatar's standing render exception to be set (or, with
/// [`RenderOverride::Normal`], cleared). The one way in: the avatar pie, the
/// management floater and its avatar picker all write this.
#[derive(Message, Debug, Clone)]
pub(crate) struct RequestRenderException {
    /// The avatar the decision is about.
    pub(crate) agent: AgentKey,
    /// The best name the requesting surface knows for them (empty when it knows
    /// none — [`refresh_exception_names`] fills it in later).
    pub(crate) name: String,
    /// What to decide.
    pub(crate) setting: RenderOverride,
}

/// Why a [`RequestRenderException`] was refused — the guards, named so the test
/// can assert on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExceptionRefusal {
    /// The request named a null agent.
    NullAgent,
    /// The request named the agent itself: you are never drawn as a jellydoll,
    /// so an exception on yourself could only ever be a confusing no-op entry.
    OwnAgent,
}

/// Run the guards over one request, returning why it must be refused.
pub(crate) fn check_render_exception(
    agent: AgentKey,
    own: Option<AgentKey>,
) -> Option<ExceptionRefusal> {
    if agent.uuid().is_nil() {
        return Some(ExceptionRefusal::NullAgent);
    }
    if own == Some(agent) {
        return Some(ExceptionRefusal::OwnAgent);
    }
    None
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// Registers the exception store, its requests and its persistence. The floater
/// over it is [`crate::avatar_render_floater`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AvatarRenderSettingsPlugin;

impl Plugin for AvatarRenderSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvatarRenderSettings>()
            .add_message::<RequestRenderException>()
            .add_systems(
                Update,
                (
                    load_avatar_render_settings,
                    apply_render_exception_requests,
                    refresh_exception_names,
                    flush_avatar_render_settings,
                )
                    .chain()
                    // The complexity model mirrors this store, and its mirror
                    // runs after the scene mirror — so the exception a pie set
                    // this frame reaches the decision in the same frame.
                    .before(crate::avatar_complexity::sync_complexity_exceptions),
            );
    }
}

/// Turn each [`RequestRenderException`] into a stored decision, after the
/// guards.
pub(crate) fn apply_render_exception_requests(
    mut requests: MessageReader<RequestRenderException>,
    mut settings: ResMut<AvatarRenderSettings>,
    identity: Res<SlIdentity>,
) {
    let own = identity.agent_id;
    for request in requests.read() {
        if let Some(refusal) = check_render_exception(request.agent, own) {
            debug!(agent = %request.agent, ?refusal, "refusing a render exception");
            continue;
        }
        settings.set(
            request.agent,
            request.setting,
            &request.name,
            now_epoch_secs(),
        );
        info!(agent = %request.agent, setting = ?request.setting, "per-avatar render exception set");
    }
}

/// Keep the name cache warm for every listed avatar and mirror what it answers,
/// so the floater reads live names off one revision. Throttled: the list is
/// small, but a name nobody answers must not be re-requested every frame.
pub(crate) fn refresh_exception_names(
    mut settings: ResMut<AvatarRenderSettings>,
    avatars: Option<ResMut<AvatarState>>,
    time: Res<Time>,
    mut last_run: Local<Option<f64>>,
) {
    let Some(mut avatars) = avatars else {
        return;
    };
    if settings.entries.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    if last_run.is_some_and(|last| now - last < NAME_REFRESH_SECONDS) {
        return;
    }
    *last_run = Some(now);
    let agents: Vec<AgentKey> = settings
        .entries
        .iter()
        .map(|entry| AgentKey::from(entry.agent))
        .collect();
    for agent in agents {
        let resolved = avatars
            .name_record(agent)
            .and_then(crate::avatars::NameRecord::preferred_name)
            .map(ToOwned::to_owned);
        match resolved {
            Some(name) => settings.note_live_name(agent, &name),
            // Nothing known yet: the request de-duplicates itself, so a name
            // that is simply slow is asked for once and a name nobody can
            // answer costs one request for the session.
            None => avatars.request_name(agent),
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence.
// ---------------------------------------------------------------------------

/// The wall clock in Unix epoch seconds, for stamping a new entry.
fn now_epoch_secs() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// Once the per-account directory resolves (post login), read the stored
/// exceptions. Runs once.
pub(crate) fn load_avatar_render_settings(
    mut list: ResMut<AvatarRenderSettings>,
    settings: Option<Res<ViewerSettings>>,
) {
    if list.loaded {
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
    list.loaded = true;
    let saved = read_store(&path);
    list.path = Some(path);
    if saved.is_empty() {
        return;
    }
    // A load is not an edit — everything read is already on disk — but a
    // decision made before the account directory resolved is, and must survive
    // the load and still be written.
    let pending = list.dirty;
    for entry in saved {
        // A stored `Normal` is the reference's "no exception" and would index as
        // an entry that says nothing, so it is dropped on the way in; a person
        // already decided about this session keeps the fresher decision.
        if entry.setting == RenderOverride::Normal
            || list.entries.iter().any(|held| held.agent == entry.agent)
        {
            continue;
        }
        list.entries.push(entry);
    }
    list.reindex();
    list.dirty = pending;
}

/// Read the persisted exceptions from `path`, tolerating a missing file (the
/// first-run case) and a malformed one (logged, treated as empty — a corrupt
/// store must not abort login).
fn read_store(path: &std::path::Path) -> Vec<RenderException> {
    if !path.exists() {
        return Vec::new();
    }
    match fs_err::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<Vec<RenderException>>(&contents) {
            Ok(list) => {
                info!(count = list.len(), path = %path.display(), "loaded the avatar render exceptions");
                list
            }
            Err(error) => {
                warn!(path = %path.display(), %error, "malformed avatar render exceptions; ignoring");
                Vec::new()
            }
        },
        Err(error) => {
            warn!(path = %path.display(), %error, "could not read the avatar render exceptions");
            Vec::new()
        }
    }
}

/// Write the exceptions when they have changed, once the path is known
/// (best-effort — a write failure is logged, never fatal).
pub(crate) fn flush_avatar_render_settings(mut list: ResMut<AvatarRenderSettings>) {
    if !list.dirty {
        return;
    }
    let Some(path) = list.path.clone() else {
        return;
    };
    match serde_json::to_string_pretty(&list.entries) {
        Ok(contents) => {
            if let Err(error) = fs_err::write(&path, contents) {
                warn!(path = %path.display(), %error, "could not write the avatar render exceptions");
            } else {
                debug!(
                    count = list.entries.len(),
                    "flushed the avatar render exceptions"
                );
                list.dirty = false;
            }
        }
        Err(error) => warn!(%error, "could not serialize the avatar render exceptions"),
    }
}

#[cfg(test)]
mod tests {
    use super::{AvatarRenderSettings, ExceptionRefusal, RenderException, check_render_exception};
    use crate::avatar_complexity::RenderOverride;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{AgentKey, Uuid};

    /// An agent key from a small integer.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(id))
    }

    /// Setting an exception lists and indexes it; setting `Normal` removes it
    /// again, as the reference's own store does.
    #[test]
    fn setting_and_clearing_an_exception() {
        let mut settings = AvatarRenderSettings::default();
        settings.set(agent(1), RenderOverride::Never, "Alpha Resident", 100);
        assert_eq!(settings.entries().len(), 1);
        assert_eq!(settings.setting_of(agent(1)), RenderOverride::Never);
        assert_eq!(settings.setting_of(agent(2)), RenderOverride::Normal);

        settings.set(agent(1), RenderOverride::Normal, "", 200);
        assert!(settings.entries().is_empty());
        assert_eq!(settings.setting_of(agent(1)), RenderOverride::Normal);
    }

    /// Re-deciding replaces the setting in place — one entry per person, its
    /// original date kept — while an identical re-decision changes nothing at
    /// all (so a repeated pie pick does not dirty the file).
    #[test]
    fn re_deciding_replaces_in_place() {
        let mut settings = AvatarRenderSettings::default();
        settings.set(agent(1), RenderOverride::Never, "Alpha Resident", 100);
        settings.set(agent(1), RenderOverride::AlwaysFull, "", 500);
        assert_eq!(settings.entries().len(), 1);
        assert_eq!(settings.setting_of(agent(1)), RenderOverride::AlwaysFull);
        assert_eq!(
            settings
                .entries()
                .first()
                .map(|entry| (entry.added_epoch_secs, entry.name.clone())),
            Some((100, "Alpha Resident".to_owned()))
        );

        // An identical re-decision is not a change: neither the revision (which
        // gates the render mirror) nor the dirty flag (which gates the write)
        // moves, so a repeated pie pick costs nothing.
        let revision = settings.revision();
        settings.dirty = false;
        settings.set(agent(1), RenderOverride::AlwaysFull, "", 900);
        assert_eq!(settings.revision(), revision);
        assert!(!settings.dirty);

        // Clearing one nobody has is likewise nothing.
        let mut empty = AvatarRenderSettings::default();
        empty.set(agent(9), RenderOverride::Normal, "", 1);
        assert_eq!(empty.revision(), 0);
        assert!(!empty.dirty);
    }

    /// A resolved name is mirrored for the list to read by, never written into
    /// the stored decision — a grid answers an id it cannot resolve with a
    /// placeholder (the local one says `Unknown UserU…`), and persisting that
    /// would destroy the record of who the decision was about. The mirror moves
    /// the revision only when it says something new, and follows the list.
    #[test]
    fn resolved_names_are_mirrored_not_stored() {
        let mut settings = AvatarRenderSettings::default();
        settings.set(agent(1), RenderOverride::Never, "Alpha Resident", 100);
        let revision = settings.revision();
        settings.dirty = false;

        settings.note_live_name(agent(1), "Alpha Renamed");
        assert_eq!(settings.live_name(agent(1)), Some("Alpha Renamed"));
        assert_ne!(settings.revision(), revision);
        assert!(!settings.dirty, "a resolution is not a change to the file");
        assert_eq!(
            settings.entries().first().map(|entry| entry.name.as_str()),
            Some("Alpha Resident"),
            "the stored name is what the deciding surface knew, and stays"
        );

        let settled = settings.revision();
        settings.note_live_name(agent(1), "Alpha Renamed");
        settings.note_live_name(agent(1), "");
        assert_eq!(settings.revision(), settled, "no news is not a change");

        settings.set(agent(1), RenderOverride::Normal, "", 200);
        assert_eq!(
            settings.live_name(agent(1)),
            None,
            "the mirror follows the list"
        );
    }

    /// The guards: never the null id, never the agent itself.
    #[test]
    fn request_guards() {
        let own = agent(7);
        assert_eq!(
            check_render_exception(AgentKey::from(Uuid::nil()), Some(own)),
            Some(ExceptionRefusal::NullAgent)
        );
        assert_eq!(
            check_render_exception(own, Some(own)),
            Some(ExceptionRefusal::OwnAgent)
        );
        assert_eq!(check_render_exception(agent(3), Some(own)), None);
        assert_eq!(check_render_exception(agent(3), None), None);
    }

    /// An entry round-trips through the file, and its setting is written as the
    /// reference's `VisualMuteSettings` integer so a Firestorm list ports across
    /// by transcription.
    #[test]
    fn entries_round_trip_as_the_reference_numbering() -> Result<(), String> {
        let entries = vec![
            RenderException {
                agent: Uuid::from_u128(1),
                name: "Alpha Resident".to_owned(),
                setting: RenderOverride::Never,
                added_epoch_secs: 100,
            },
            RenderException {
                agent: Uuid::from_u128(2),
                name: "Beta Resident".to_owned(),
                setting: RenderOverride::AlwaysFull,
                added_epoch_secs: 200,
            },
        ];
        let json = serde_json::to_string(&entries).map_err(|error| error.to_string())?;
        assert!(
            json.contains("\"setting\":1"),
            "never render is the reference's 1: {json}"
        );
        assert!(
            json.contains("\"setting\":2"),
            "always render is the reference's 2: {json}"
        );
        let read: Vec<RenderException> =
            serde_json::from_str(&json).map_err(|error| error.to_string())?;
        assert_eq!(read, entries);
        Ok(())
    }

    /// A stored value the file should never hold reads back as "no exception"
    /// rather than failing the load, and a `Normal` entry is dropped on the way
    /// in so the index never holds a decision that says nothing.
    #[test]
    fn unknown_stored_values_degrade() -> Result<(), String> {
        let read: Vec<RenderException> = serde_json::from_str(
            r#"[{"agent":"00000000-0000-0000-0000-000000000001","setting":47}]"#,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            read.first().map(|entry| entry.setting),
            Some(RenderOverride::Normal)
        );
        Ok(())
    }
}
