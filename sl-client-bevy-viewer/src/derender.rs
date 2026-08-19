//! **Derender + asset blacklist** (`viewer-derender-blacklist`): remove an
//! object or an avatar from *your* view — temporarily (until the next teleport)
//! or permanently, through a per-avatar persisted blacklist.
//!
//! This is the everyday tool against visual griefing and against the one laggy
//! object a parcel owner will not remove. It is purely **client-side** and
//! strictly distinct from the server-side mute list ([`crate::mutes`]): nothing
//! here goes on the wire, and the simulator keeps streaming the object — the
//! viewer simply refuses to mirror it into the scene.
//!
//! # One guarded way in
//!
//! Every Derender affordance writes a [`RequestDerender`] (never touching the
//! list itself), exactly as every Block affordance writes a
//! [`RequestBlock`](crate::mutes::RequestBlock): [`apply_derender_requests`]
//! runs the reference's `derenderObject` guards — never the agent itself, never
//! a null id — stands the agent up when the target is its own seat (a derendered
//! seat would otherwise strand it), drops the target from the edit selection, and
//! only then records the entry.
//!
//! # How the suppression works
//!
//! The reference drops a derendered id in `LLViewerObjectList::createObject`, so
//! nothing downstream ever sees the object. Ours does the same at the scene
//! mirror's ingest ([`crate::objects::update_objects`],
//! [`crate::avatars::update_avatar_objects`]): a suppressed object is never
//! applied, so no mesh is tessellated, no texture requested and no material
//! built — the cheapest possible place to say no.
//!
//! Suppression is **transitive over the parent link**, which is what makes
//! derendering a linkset root or an avatar do the obvious thing:
//! [`index_derendered_objects`] keeps a set of suppressed *region-scoped* ids,
//! seeding it from the blacklisted full ids and extending it to any object whose
//! parent is already in it. A linkset's child prims (parented to its root) and an
//! avatar's attachments (parented to the avatar object) are therefore suppressed
//! with their root — without which a derendered avatar would leave its
//! attachments floating, with no parent, at the scene origin.
//!
//! Anything already in the scene when the entry is added — or that arrived
//! before its parent did — is despawned by [`enforce_derender`], which purges by
//! full id (the request path) and by scoped id (the index path).
//!
//! # Temporary vs permanent
//!
//! A **permanent** entry is written to the per-avatar blacklist file (a sibling
//! of the account `settings.toml`, like [`crate::notification_persist`]'s store)
//! and applies again on the next login. A **temporary** entry lives in the
//! session only and is dropped on the next teleport
//! ([`clear_temporary_derenders`], the reference's
//! `LLViewerObjectList::resetDerenderList` on `LLAgent::teleportCore`, gated by
//! its `FSTempDerenderUntilTeleport` setting — ours is [`SETTING_UNTIL_TELEPORT`]).
//!
//! # Re-rendering
//!
//! Removing an entry does more than stop the suppression. The simulator streams
//! an object once, and every update for it was dropped while it was suppressed,
//! so forgetting the entry alone would leave the object absent until the region
//! streamed it again — which is all the reference does. Because the suppression
//! index kept each hidden object's *region-local* id the whole time, the release
//! instead queues those ids for a re-fetch (`RequestMultipleObjects`, a full
//! cache miss — [`refetch_released_objects`]) and the object comes back within
//! the round trip.
//!
//! Reference (Firestorm, read-only): `fsassetblacklist`,
//! `llviewermenu.cpp`'s `derenderObject`, `LLAvatarActions::derender`,
//! `llviewerobjectlist.cpp`'s `mDerendered` / `resetDerenderList`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use sl_client_bevy::{
    AgentKey, CircuitId, Command, ScopedObjectId, SlAgentParcel, SlCommand, SlCurrentRegion,
    SlEvent, SlIdentity, SlRegionIdentity, SlSessionEvent, Uuid,
};
use sl_settings::SettingValue;
use tracing::{debug, info, warn};

use crate::avatars::AvatarState;
use crate::objects::ObjectState;
use crate::settings::ViewerSettings;

/// The per-account file the permanent blacklist is stored in (a sibling of the
/// account `settings.toml`). Our account directory is already per-grid and
/// per-avatar, so the bare name suffices.
const STORE_FILE: &str = "derender_blacklist.json";

/// The persisted-settings section the derender knobs live under.
const DERENDER_SECTION: &[&str] = &["derender"];

/// Whether a temporary derender lasts until the next teleport (the reference's
/// `FSTempDerenderUntilTeleport`, default on) or for the whole session.
pub(crate) const SETTING_UNTIL_TELEPORT: &str = "TempDerenderUntilTeleport";

// ---------------------------------------------------------------------------
// The model.
// ---------------------------------------------------------------------------

/// What kind of thing a blacklist entry names — the reference's `LLAssetType`,
/// narrowed to the kinds a viewer can actually refuse.
///
/// The two **in-world** kinds are what the derender menus produce and what the
/// scene mirror gates on. The three **asset** kinds are refused at their own
/// point of use instead — a blacklisted sound is never played, a blacklisted
/// animation never runs, a blacklisted texture is never fetched — which is
/// exactly where the reference refuses them. Their producers are the explorer
/// floaters (the sound explorer feeds `Sound`, the animation explorer
/// `Animation`); until those land, an asset entry comes from the per-account
/// file itself, which is also how the reference's distributed blacklist data
/// (`fsdata`) feeds textures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DerenderKind {
    /// An in-world object (the reference's `AT_OBJECT`).
    Object,
    /// An avatar (the reference's `AT_PERSON`).
    Resident,
    /// A sound asset, never played (`AT_SOUND`).
    Sound,
    /// An animation asset, never run (`AT_ANIMATION`).
    Animation,
    /// A texture asset, never fetched (`AT_TEXTURE`).
    Texture,
}

impl DerenderKind {
    /// The Fluent key naming this kind in the blacklist's Type column.
    pub(crate) const fn label_key(self) -> &'static str {
        match self {
            Self::Object => "derender-type-object",
            Self::Resident => "derender-type-resident",
            Self::Sound => "derender-type-sound",
            Self::Animation => "derender-type-animation",
            Self::Texture => "derender-type-texture",
        }
    }

    /// A stable sort rank, so the Type column orders deterministically.
    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::Object => 0,
            Self::Resident => 1,
            Self::Sound => 2,
            Self::Animation => 3,
            Self::Texture => 4,
        }
    }

    /// Whether this kind names something the **scene mirror** suppresses (as
    /// opposed to an asset refused at its point of use).
    const fn is_in_world(self) -> bool {
        matches!(self, Self::Object | Self::Resident)
    }
}

/// One blacklist entry: what was derendered, where and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DerenderEntry {
    /// The derendered thing's persistent id (an object's full id, an avatar's
    /// agent id).
    pub(crate) id: Uuid,
    /// Its name, as the surface that derendered it knew it (may be empty when
    /// the object-properties reply had not landed yet).
    pub(crate) name: String,
    /// The region it was derendered in (empty when unknown).
    pub(crate) region: String,
    /// What kind of thing it is.
    pub(crate) kind: DerenderKind,
    /// Whether it survives a teleport and a relog (the "Blacklist" slice) or is
    /// a session-only "Temporary" derender.
    pub(crate) permanent: bool,
    /// When it was added, as Unix epoch seconds (stored as a plain integer so
    /// the file needs no date parser).
    pub(crate) added_epoch_secs: i64,
}

/// The viewer's derender / blacklist state.
#[derive(Resource, Debug, Default)]
pub(crate) struct DerenderList {
    /// The entries, newest last.
    entries: Vec<DerenderEntry>,
    /// The blacklisted ids and what each is blacklisted **as**, derived from
    /// [`Self::entries`] — the hot-path index every check goes through. Keyed by
    /// id alone: an id is one thing, so a second entry for it would be a
    /// contradiction, and the kind rides along so a sound check never matches an
    /// object entry.
    ids: HashMap<Uuid, DerenderKind>,
    /// The region-scoped ids currently suppressed, each mapped to the
    /// **blacklisted id it hangs off**: an entry's own object maps to its own
    /// id, a linkset child or attachment to its root's. Keeping the root is what
    /// lets a single un-derender release exactly its own subtree (see
    /// [`Self::remove`]). Session-derived, never persisted.
    hidden_scoped: HashMap<ScopedObjectId, Uuid>,
    /// Full ids whose scene entities still need despawning (a fresh entry).
    pending_ids: Vec<Uuid>,
    /// Scoped ids whose scene entities still need despawning (an object that
    /// was already tracked when its parent became hidden).
    pending_scoped: Vec<ScopedObjectId>,
    /// Scoped ids just **released** from suppression, to be re-fetched from the
    /// simulator so an un-derendered object comes back at once
    /// ([`refetch_released_objects`]).
    pending_refetch: Vec<ScopedObjectId>,
    /// Bumped on every change to [`Self::entries`], so the floater rebuilds
    /// exactly when the list moved.
    revision: u64,
    /// The per-account store path, resolved at login; `None` until then (and
    /// when the platform has no per-avatar directory, disabling persistence).
    path: Option<PathBuf>,
    /// Whether the on-disk list has been read — a once-per-session load.
    loaded: bool,
    /// Whether the **permanent** entries changed since the last flush.
    dirty: bool,
}

impl DerenderList {
    /// Whether `id` is blacklisted **as** `kind` — the query each point of use
    /// runs (a sound before playing it, an animation before running it, a
    /// texture before fetching it).
    pub(crate) fn blacklists(&self, id: Uuid, kind: DerenderKind) -> bool {
        self.ids.get(&id) == Some(&kind)
    }

    /// Whether `id` names a derendered **in-world** thing (an object or an
    /// avatar) — the hot-path query the scene mirror runs per streamed object.
    pub(crate) fn hides_in_world(&self, id: Uuid) -> bool {
        self.ids.get(&id).is_some_and(|kind| kind.is_in_world())
    }

    /// Every blacklisted id of `kind` — how a consumer that cannot consult the
    /// list per item (the texture store, whose fetch gate is not a Bevy system)
    /// mirrors the set it needs.
    pub(crate) fn ids_of_kind(&self, kind: DerenderKind) -> HashSet<Uuid> {
        self.ids
            .iter()
            .filter(|(_id, held)| **held == kind)
            .map(|(id, _held)| *id)
            .collect()
    }

    /// Whether the object with region-scoped id `scoped` must not be mirrored
    /// into the scene: it is blacklisted itself, or it hangs off something that
    /// is (a linkset child, an attachment). Maintained by
    /// [`index_derendered_objects`].
    pub(crate) fn is_suppressed(&self, scoped: ScopedObjectId) -> bool {
        self.hidden_scoped.contains_key(&scoped)
    }

    /// The blacklisted id suppressing `scoped`, if it is suppressed — the root
    /// an inherited suppression is inherited from.
    fn suppressing_root(&self, scoped: ScopedObjectId) -> Option<Uuid> {
        self.hidden_scoped.get(&scoped).copied()
    }

    /// Record every id in `removed` as suppressed by the blacklisted `root`.
    ///
    /// The scene purge calls this with what it despawned, because those ids are
    /// often the *only* record of them: the simulator streams a static object
    /// once, so an object derendered long after it was streamed never produces
    /// another update for [`index_derendered_objects`] to learn from — and
    /// without the record, un-derendering it would have nothing to re-fetch.
    fn note_hidden(&mut self, removed: impl IntoIterator<Item = ScopedObjectId>, root: Uuid) {
        for scoped in removed {
            let _prior = self.hidden_scoped.insert(scoped, root);
        }
    }

    /// The whole list, in insertion order.
    pub(crate) fn entries(&self) -> &[DerenderEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and rebuilds
    /// when it advances.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// Add (or replace) an entry, marking the scene for a purge of its id.
    /// Re-derendering an id already listed **upgrades** it: a temporary entry
    /// that is blacklisted becomes permanent, never the other way round, which
    /// is what the reference's `addNewItemToBlacklist` overwrite amounts to for
    /// the only two paths that reach it.
    fn add(&mut self, entry: DerenderEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|held| held.id == entry.id) {
            let upgraded = entry.permanent && !existing.permanent;
            existing.permanent |= entry.permanent;
            if existing.name.is_empty() {
                existing.name.clone_from(&entry.name);
            }
            if upgraded {
                self.dirty = true;
                self.revision = self.revision.wrapping_add(1);
            }
            return;
        }
        self.dirty |= entry.permanent;
        self.pending_ids.push(entry.id);
        self.entries.push(entry);
        self.reindex();
    }

    /// Drop the entry for `id`, if held, releasing everything it suppressed and
    /// queueing those objects for a re-fetch (see [`Self::release`]).
    fn remove(&mut self, id: Uuid) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == before {
            return;
        }
        self.dirty = true;
        self.reindex();
        // Release exactly what this entry was suppressing — its own object and
        // everything that inherited the suppression from it — and nothing else:
        // another blacklisted root's children must stay hidden.
        self.release(|root| root == id);
    }

    /// Drop every temporary entry (a teleport, or the floater's Clear temporary).
    fn clear_temporary(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.permanent);
        if self.entries.len() == before {
            return;
        }
        self.reindex();
        // Every suppression whose root just left the list is released; the
        // permanent entries keep theirs.
        let live: HashSet<Uuid> = self.ids.keys().copied().collect();
        self.release(|root| !live.contains(&root));
    }

    /// Drop every suppression whose root `released` accepts, and queue the
    /// freed region-scoped ids for a re-fetch.
    ///
    /// The re-fetch is what makes "Re-render" mean it: the simulator streams an
    /// object once, and the viewer dropped every update for it while it was
    /// suppressed, so simply forgetting the entry would leave the object absent
    /// until the region streamed it again (a teleport away and back — which is
    /// all the reference does). Because the index kept the object's *region-local*
    /// id the whole time, we can instead ask for it back right now
    /// (`RequestMultipleObjects`, a full cache miss).
    fn release(&mut self, released: impl Fn(Uuid) -> bool) {
        let freed: Vec<ScopedObjectId> = self
            .hidden_scoped
            .iter()
            .filter(|(_scoped, root)| released(**root))
            .map(|(scoped, _root)| *scoped)
            .collect();
        for scoped in &freed {
            let _dropped = self.hidden_scoped.remove(scoped);
        }
        self.pending_refetch.extend(freed);
    }

    /// Rebuild the derived id index and bump the revision.
    fn reindex(&mut self) {
        self.ids = self
            .entries
            .iter()
            .map(|entry| (entry.id, entry.kind))
            .collect();
        self.revision = self.revision.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

/// A request to derender something — the single **guarded** entry point every
/// Derender affordance writes (the object / avatar / attachment pies, the
/// radar's row menu), rather than editing the list itself.
#[derive(Message, Debug, Clone)]
pub(crate) struct RequestDerender {
    /// The target's persistent id (an object's full id, an avatar's agent id).
    pub(crate) id: Uuid,
    /// The target's name, as the asking surface knows it.
    pub(crate) name: String,
    /// What kind of target it is.
    pub(crate) kind: DerenderKind,
    /// Whether the entry is written to the persisted blacklist ("Blacklist") or
    /// lives in this session only ("Temporary").
    pub(crate) permanent: bool,
}

impl RequestDerender {
    /// Derender `id` under `name`, permanently or for the session.
    pub(crate) fn new(
        id: Uuid,
        name: impl Into<String>,
        kind: DerenderKind,
        permanent: bool,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            permanent,
        }
    }
}

/// A request to un-derender: drop `id` from the blacklist (the floater's
/// Remove), or — with a nil id — drop every temporary entry (Clear temporary).
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct UnDerender {
    /// The entry to drop, or [`Uuid::nil`] for "every temporary entry".
    pub(crate) id: Uuid,
}

/// Why a derender request was refused, mirroring the reference's
/// `derenderObject` early-outs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerenderRefusal {
    /// The target is the agent itself — the reference's `gAgentID != objp->getID()`
    /// guard. Derendering your own avatar would hide you from yourself with no
    /// affordance to get back.
    Own,
    /// The request carried a null id (a pick that never resolved).
    Malformed,
}

/// Run the reference's `derenderObject` guards over a request — `Ok(())` when
/// the entry may be recorded.
pub(crate) fn check_derender(
    own_agent: Option<Uuid>,
    request: &RequestDerender,
) -> Result<(), DerenderRefusal> {
    if request.id.is_nil() {
        return Err(DerenderRefusal::Malformed);
    }
    if own_agent == Some(request.id) {
        return Err(DerenderRefusal::Own);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// Registers the derender list, its persistence, and the request / suppression
/// systems. The ingest-side suppression itself lives in the scene mirror
/// ([`crate::objects`] / [`crate::avatars`]), which reads [`DerenderList`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DerenderPlugin;

impl Plugin for DerenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DerenderList>()
            .add_message::<RequestDerender>()
            .add_message::<UnDerender>()
            .add_systems(Startup, register_derender_settings)
            // Everything that can *change* what is suppressed runs before the
            // scene mirror folds the frame's object events, so an ingest check
            // never uses a stale index (and a derender picked this frame drops
            // this frame's updates for its target).
            .add_systems(
                Update,
                (
                    load_derender_list,
                    apply_derender_requests,
                    apply_underender_requests,
                    clear_temporary_derenders,
                    refetch_released_objects,
                    index_derendered_objects,
                )
                    .chain()
                    .before(crate::objects::update_objects)
                    .before(crate::avatars::update_avatar_objects),
            )
            // …and the scene purge after it, so it only ever has to despawn what
            // was already standing there.
            .add_systems(
                Update,
                (enforce_derender, flush_derender_list)
                    .chain()
                    .after(crate::objects::update_objects)
                    .after(crate::avatars::update_avatar_objects),
            );
    }
}

/// Register the derender settings (the temporary-derender lifetime).
fn register_derender_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        DERENDER_SECTION,
        SETTING_UNTIL_TELEPORT,
        SettingValue::Bool(true),
        "Temporary derenders last until the next teleport (otherwise: the whole session)",
    );
}

// ---------------------------------------------------------------------------
// Requests.
// ---------------------------------------------------------------------------

/// Turn each [`RequestDerender`] into a blacklist entry: run the guards, stand
/// the agent up if the target is its seat, drop the target from the edit
/// selection, and record it stamped with the current region and clock.
pub(crate) fn apply_derender_requests(
    mut requests: MessageReader<RequestDerender>,
    mut list: ResMut<DerenderList>,
    identity: Res<SlIdentity>,
    parcel: Res<SlAgentParcel>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut selection: ResMut<crate::edit_selection::SelectionSet>,
    mut commands: MessageWriter<SlCommand>,
) {
    let own = identity.agent_id.map(|agent| agent.uuid());
    let region = regions
        .single()
        .ok()
        .and_then(|region| region.0.sim_name.as_ref())
        .map(ToString::to_string)
        .unwrap_or_default();
    for request in requests.read() {
        if let Err(refusal) = check_derender(own, request) {
            debug!("derender of {:?} refused: {refusal:?}", request.name);
            continue;
        }
        // The reference stands up rather than leaving the agent seated on an
        // object it can no longer see (or click to stand from).
        if parcel
            .seated_on
            .is_some_and(|seat| seat.uuid() == request.id)
        {
            commands.write(SlCommand(Command::Stand));
        }
        selection.remove_by_full_id(request.id);
        list.add(DerenderEntry {
            id: request.id,
            name: request.name.clone(),
            region: region.clone(),
            kind: request.kind,
            permanent: request.permanent,
            added_epoch_secs: now_epoch_secs(),
        });
        info!(
            id = %request.id,
            permanent = request.permanent,
            "derendered {:?}",
            request.name
        );
    }
}

/// Apply each [`UnDerender`]: drop one entry, or every temporary one.
pub(crate) fn apply_underender_requests(
    mut requests: MessageReader<UnDerender>,
    mut list: ResMut<DerenderList>,
) {
    for request in requests.read() {
        if request.id.is_nil() {
            list.clear_temporary();
        } else {
            list.remove(request.id);
        }
    }
}

/// The most objects one re-fetch message names. `RequestMultipleObjects` is a
/// variable block, so a released linkset of hundreds of prims would otherwise
/// be one oversized datagram; the remainder rides the next frame's message.
const REFETCH_BATCH: usize = 64;

/// Ask the simulator to re-send every object a just-removed blacklist entry
/// released, so an un-derendered object reappears at once instead of waiting
/// for the region to stream it again (see [`DerenderList::release`]).
///
/// Runs **after** the release, so the suppression is already gone by the time
/// the re-sent `ObjectUpdate`s arrive and the scene mirror applies them
/// normally.
pub(crate) fn refetch_released_objects(
    mut list: ResMut<DerenderList>,
    mut commands: MessageWriter<SlCommand>,
) {
    if list.pending_refetch.is_empty() {
        return;
    }
    let take = list.pending_refetch.len().min(REFETCH_BATCH);
    let batch: Vec<ScopedObjectId> = list.pending_refetch.drain(..take).collect();
    // One message per circuit: `RequestMultipleObjects` goes out on a single
    // circuit, and a released set can span two connected regions (a linkset at a
    // region edge, an avatar streamed by a neighbour). A mixed batch would be
    // rejected whole.
    let mut by_circuit: HashMap<CircuitId, Vec<ScopedObjectId>> = HashMap::new();
    for scoped in batch {
        by_circuit.entry(scoped.circuit()).or_default().push(scoped);
    }
    for (circuit, local_ids) in by_circuit {
        debug!(
            count = local_ids.len(),
            ?circuit,
            "re-fetching un-derendered objects"
        );
        commands.write(SlCommand(Command::RequestObjects { local_ids }));
    }
}

/// The wall clock in Unix epoch seconds, for stamping a new entry.
fn now_epoch_secs() -> i64 {
    jiff::Timestamp::now().as_second()
}

// ---------------------------------------------------------------------------
// The suppression index and its enforcement.
// ---------------------------------------------------------------------------

/// Keep the region-scoped suppression index current from the object stream:
/// an update for a blacklisted id marks its scoped id hidden, and so does one
/// whose **parent** is already hidden (a linkset child, an attachment), which is
/// what makes derendering a root or an avatar take its whole subtree.
///
/// Runs before the scene mirror folds the same events, so the ingest checks see
/// a current index; an object that was already tracked when its parent became
/// hidden is queued for [`enforce_derender`] to despawn.
pub(crate) fn index_derendered_objects(
    mut events: MessageReader<SlEvent>,
    mut list: ResMut<DerenderList>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                let scoped = object.scoped_id();
                let own = object.full_id.uuid();
                // An object's own blacklist entry wins; otherwise it inherits
                // whatever root suppresses its parent.
                let root = if list.hides_in_world(own) {
                    Some(own)
                } else if object.parent_id.get() != 0 {
                    list.suppressing_root(object.scoped_parent_id())
                } else {
                    None
                };
                match root {
                    Some(root) => {
                        if list.hidden_scoped.insert(scoped, root).is_none() {
                            list.pending_scoped.push(scoped);
                        }
                    }
                    None => {
                        let _was_hidden = list.hidden_scoped.remove(&scoped);
                    }
                }
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                let _gone = list.hidden_scoped.remove(local_id);
            }
            // A distant teleport minted a fresh circuit: the old region's
            // local-id space is gone, so every scoped suppression in it is
            // meaningless (the scene mirror purges itself for the same reason).
            // The blacklist itself is untouched — the destination re-seeds the
            // index as it streams.
            SlSessionEvent::RegionChanged {
                world_reset: true, ..
            } => list.hidden_scoped.clear(),
            _other => {}
        }
    }
}

/// Despawn whatever a fresh derender left in the scene: everything tracked
/// under a newly blacklisted full id (the request path — the object was
/// standing there when the user derendered it) and everything under a newly
/// hidden scoped id (the index path — an attachment that arrived before the
/// avatar it hangs off).
///
/// Removing an object also removes its tracked descendants
/// ([`ObjectState::derender_remove`]), so a linkset or an avatar's attachments
/// go with their root.
///
/// Every removed region-scoped id is **recorded** in the suppression index, not
/// just the ones the wire happened to teach it. The simulator streams a static
/// object once: derender a prim that has been standing there since login and no
/// further update for it ever arrives, so the index would never learn its
/// region-local id — and a later un-derender would have nothing to re-fetch,
/// leaving the object gone until the region streamed it again. The purge is the
/// other place that knows those ids, so it seeds the index here.
pub(crate) fn enforce_derender(
    mut list: ResMut<DerenderList>,
    mut objects: ResMut<ObjectState>,
    mut avatars: ResMut<AvatarState>,
    mut commands: Commands,
) {
    if list.pending_ids.is_empty() && list.pending_scoped.is_empty() {
        return;
    }
    for id in core::mem::take(&mut list.pending_ids) {
        for scoped in objects.scoped_by_full_id(id) {
            let removed = objects.derender_remove(scoped, &mut commands);
            list.note_hidden(removed, id);
        }
        avatars.derender_agent(AgentKey::from(id), &mut commands);
    }
    for scoped in core::mem::take(&mut list.pending_scoped) {
        // Descendants of an already-indexed object inherit its root, so the
        // whole subtree is released (and re-fetched) together.
        let root = list.suppressing_root(scoped);
        let removed = objects.derender_remove(scoped, &mut commands);
        if let Some(root) = root {
            list.note_hidden(removed, root);
        }
        avatars.derender_scoped(scoped, &mut commands);
    }
}

/// Drop the temporary derenders when the agent teleports (the reference's
/// `resetDerenderList` from `LLAgent::teleportCore`), unless the user turned
/// [`SETTING_UNTIL_TELEPORT`] off — then they last the whole session.
pub(crate) fn clear_temporary_derenders(
    mut events: MessageReader<SlEvent>,
    mut list: ResMut<DerenderList>,
    settings: Option<Res<ViewerSettings>>,
) {
    let until_teleport = settings
        .as_deref()
        .and_then(|settings| settings.store().get_bool(SETTING_UNTIL_TELEPORT).ok())
        .unwrap_or(true);
    let mut teleporting = false;
    for event in events.read() {
        if matches!(&event.0, SlSessionEvent::TeleportStarted) {
            teleporting = true;
        }
    }
    if teleporting && until_teleport {
        list.clear_temporary();
    }
}

// ---------------------------------------------------------------------------
// Persistence.
// ---------------------------------------------------------------------------

/// Once the per-account directory resolves (post login), read the permanent
/// blacklist and apply it. Runs once.
pub(crate) fn load_derender_list(
    mut list: ResMut<DerenderList>,
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
    for entry in saved {
        list.add(entry);
    }
    // A load is not an edit: everything read is already on disk.
    list.dirty = false;
}

/// Read the persisted list from `path`, tolerating a missing file (the first-run
/// case) and a malformed one (logged, treated as empty — a corrupt store must
/// not abort login).
fn read_store(path: &std::path::Path) -> Vec<DerenderEntry> {
    if !path.exists() {
        return Vec::new();
    }
    match fs_err::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<Vec<DerenderEntry>>(&contents) {
            Ok(list) => {
                info!(count = list.len(), path = %path.display(), "loaded the derender blacklist");
                list
            }
            Err(error) => {
                warn!(path = %path.display(), %error, "malformed derender blacklist; ignoring");
                Vec::new()
            }
        },
        Err(error) => {
            warn!(path = %path.display(), %error, "could not read the derender blacklist");
            Vec::new()
        }
    }
}

/// Write the **permanent** entries to disk when the list has changed, once its
/// path is known (best-effort — a write failure is logged, never fatal).
pub(crate) fn flush_derender_list(mut list: ResMut<DerenderList>) {
    if !list.dirty {
        return;
    }
    let Some(path) = list.path.clone() else {
        return;
    };
    let permanent: Vec<&DerenderEntry> = list
        .entries
        .iter()
        .filter(|entry| entry.permanent)
        .collect();
    match serde_json::to_string_pretty(&permanent) {
        Ok(contents) => {
            if let Err(error) = fs_err::write(&path, contents) {
                warn!(path = %path.display(), %error, "could not write the derender blacklist");
            } else {
                debug!(count = permanent.len(), "flushed the derender blacklist");
                list.dirty = false;
            }
        }
        Err(error) => warn!(%error, "could not serialize the derender blacklist"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DerenderEntry, DerenderKind, DerenderList, DerenderRefusal, RequestDerender, check_derender,
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{CircuitId, RegionLocalObjectId, ScopedObjectId, Uuid};
    use std::collections::HashSet;

    /// An entry for `id`, permanent or not.
    fn entry(id: u128, permanent: bool) -> DerenderEntry {
        DerenderEntry {
            id: Uuid::from_u128(id),
            name: "Griefer Cube".to_owned(),
            region: "Default Region".to_owned(),
            kind: DerenderKind::Object,
            permanent,
            added_epoch_secs: 1_700_000_000,
        }
    }

    /// Adding indexes the id, bumps the revision, and queues the scene purge.
    #[test]
    fn adding_indexes_the_id() {
        let mut list = DerenderList::default();
        assert!(list.entries().is_empty());
        let before = list.revision();
        list.add(entry(1, true));
        assert!(list.hides_in_world(Uuid::from_u128(1)));
        assert!(!list.hides_in_world(Uuid::from_u128(2)));
        assert_ne!(list.revision(), before);
        assert_eq!(list.pending_ids, vec![Uuid::from_u128(1)]);
        assert!(list.dirty, "a permanent entry must be persisted");
    }

    /// A temporary entry does not dirty the store; blacklisting the same id
    /// afterwards upgrades it in place (and then does).
    #[test]
    fn temporary_upgrades_to_permanent() {
        let mut list = DerenderList::default();
        list.add(entry(7, false));
        assert!(!list.dirty);
        assert_eq!(list.entries().len(), 1);
        list.add(entry(7, true));
        assert_eq!(
            list.entries().len(),
            1,
            "the same id must not be listed twice"
        );
        assert!(list.entries().first().is_some_and(|entry| entry.permanent));
        assert!(list.dirty);
        // The reverse never downgrades: a permanent entry stays permanent.
        list.dirty = false;
        list.add(entry(7, false));
        assert!(list.entries().first().is_some_and(|entry| entry.permanent));
        assert!(!list.dirty, "a no-op re-derender must not rewrite the file");
    }

    /// Removing drops the entry and its index; clearing temporary keeps the
    /// permanent ones.
    #[test]
    fn removal_and_temporary_clearing() {
        let mut list = DerenderList::default();
        list.add(entry(1, true));
        list.add(entry(2, false));
        list.clear_temporary();
        assert_eq!(list.entries().len(), 1);
        assert!(list.hides_in_world(Uuid::from_u128(1)));
        assert!(!list.hides_in_world(Uuid::from_u128(2)));
        list.remove(Uuid::from_u128(1));
        assert!(list.entries().is_empty());
        // Removing an id that is not listed changes nothing.
        list.dirty = false;
        list.remove(Uuid::from_u128(9));
        assert!(!list.dirty);
    }

    /// Un-derendering releases exactly its own suppressed subtree — leaving
    /// another blacklisted root's children hidden — and queues the released
    /// objects for the re-fetch that makes them reappear.
    #[test]
    fn releasing_refetches_only_its_own_subtree() {
        let mut list = DerenderList::default();
        let (first, second) = (Uuid::from_u128(1), Uuid::from_u128(2));
        list.add(entry(1, true));
        list.add(entry(2, true));
        // Two roots, each with one inherited child.
        let scoped = |id: u32| ScopedObjectId::new(CircuitId::default(), RegionLocalObjectId(id));
        list.note_hidden([scoped(10), scoped(11)], first);
        list.note_hidden([scoped(20)], second);
        list.pending_refetch.clear();

        list.remove(first);
        let mut released = core::mem::take(&mut list.pending_refetch);
        released.sort_by_key(|scoped| scoped.id.0);
        assert_eq!(released, vec![scoped(10), scoped(11)]);
        assert!(!list.is_suppressed(scoped(10)));
        assert!(
            list.is_suppressed(scoped(20)),
            "the other blacklisted root must keep suppressing its own subtree"
        );
    }

    /// The scene purge's record is what makes an un-derender re-fetchable: an
    /// object derendered long after it was streamed never produces another
    /// update, so [`DerenderList::note_hidden`] is the only thing that ever
    /// learns its region-local id.
    #[test]
    fn the_purge_record_is_what_gets_refetched() {
        let mut list = DerenderList::default();
        let id = Uuid::from_u128(1);
        list.add(entry(1, true));
        let scoped = ScopedObjectId::new(CircuitId::default(), RegionLocalObjectId(42));
        // Nothing on the wire ever mentioned this object; only the purge did.
        assert!(!list.is_suppressed(scoped));
        list.note_hidden([scoped], id);
        assert!(list.is_suppressed(scoped));
        list.pending_refetch.clear();
        list.remove(id);
        assert_eq!(
            list.pending_refetch,
            vec![scoped],
            "un-derendering must re-fetch what the purge despawned"
        );
    }

    /// Clearing the temporary entries releases (and re-fetches) only what they
    /// were suppressing.
    #[test]
    fn clearing_temporary_releases_only_temporary_subtrees() {
        let mut list = DerenderList::default();
        let (kept, dropped) = (Uuid::from_u128(1), Uuid::from_u128(2));
        list.add(entry(1, true));
        list.add(entry(2, false));
        let scoped = |id: u32| ScopedObjectId::new(CircuitId::default(), RegionLocalObjectId(id));
        list.note_hidden([scoped(10)], kept);
        list.note_hidden([scoped(20)], dropped);
        list.pending_refetch.clear();

        list.clear_temporary();
        assert_eq!(list.pending_refetch, vec![scoped(20)]);
        assert!(list.is_suppressed(scoped(10)));
        assert!(!list.is_suppressed(scoped(20)));
    }

    /// A kind check is exact: an entry blacklisted as one kind never matches a
    /// query for another, and only the in-world kinds gate the scene mirror.
    #[test]
    fn kind_checks_are_exact() {
        let mut list = DerenderList::default();
        let id = Uuid::from_u128(5);
        let mut sound = entry(5, true);
        sound.kind = DerenderKind::Sound;
        list.add(sound);
        assert!(list.blacklists(id, DerenderKind::Sound));
        assert!(!list.blacklists(id, DerenderKind::Object));
        assert!(
            !list.hides_in_world(id),
            "a silenced sound must not hide the object playing it"
        );

        let mut texture = entry(6, true);
        texture.kind = DerenderKind::Texture;
        list.add(texture);
        let mut resident = entry(7, true);
        resident.kind = DerenderKind::Resident;
        list.add(resident);
        assert!(list.hides_in_world(Uuid::from_u128(7)));
        assert_eq!(
            list.ids_of_kind(DerenderKind::Texture),
            HashSet::from([Uuid::from_u128(6)]),
            "the texture store mirrors exactly the texture entries"
        );
        assert!(list.ids_of_kind(DerenderKind::Animation).is_empty());
    }

    /// The reference's `derenderObject` guards: never yourself, never a null id.
    #[test]
    fn request_guards() {
        let own = Uuid::from_u128(0xAAA);
        let request = |id: u128| {
            RequestDerender::new(Uuid::from_u128(id), "Someone", DerenderKind::Resident, true)
        };
        assert_eq!(check_derender(Some(own), &request(1)), Ok(()));
        assert_eq!(
            check_derender(Some(own), &request(0xAAA)),
            Err(DerenderRefusal::Own)
        );
        assert_eq!(
            check_derender(Some(own), &request(0)),
            Err(DerenderRefusal::Malformed)
        );
    }

    /// Entries round-trip through JSON, so a reloaded blacklist is identical.
    #[test]
    fn entries_round_trip_through_json() -> Result<(), String> {
        let original = vec![entry(1, true), {
            let mut resident = entry(2, true);
            resident.kind = DerenderKind::Resident;
            resident.name = "Some Resident".to_owned();
            resident
        }];
        let json = serde_json::to_string(&original).map_err(|error| error.to_string())?;
        let parsed: Vec<DerenderEntry> =
            serde_json::from_str(&json).map_err(|error| error.to_string())?;
        assert_eq!(parsed, original);
        Ok(())
    }
}
