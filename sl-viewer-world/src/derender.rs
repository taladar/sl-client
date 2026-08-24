//! **Client-side render suppression**: the derender / asset blacklist
//! (`viewer-derender-blacklist`) and the friends-only filter
//! (`viewer-render-friends-only`) — two ways to say "do not draw this", sharing
//! one machine.
//!
//! **Derender** removes one object or avatar from *your* view, temporarily
//! (until the next teleport) or permanently through a per-avatar persisted
//! blacklist: the everyday tool against visual griefing and against the one
//! laggy object a parcel owner will not remove. **Friends-only** hides every
//! avatar that is not a friend for as long as it is on: not a moderation tool
//! but a performance one — the way to survive a crowded event on a machine that
//! cannot draw two hundred attachment-laden avatars.
//!
//! Both are purely **client-side** and strictly distinct from the server-side
//! mute list (`crate::mutes`): nothing here goes on the wire, and the
//! simulator keeps streaming everything — the viewer simply refuses to mirror it
//! into the scene.
//!
//! # Suppressed, not forgotten
//!
//! Suppression stops the **render**, not the **tracking**. A suppressed avatar's
//! body is never built — no skeleton, no bakes, no attachment meshes, which is
//! the whole performance win — but the cheap coarse placeholder the position
//! path spawns is kept and merely hidden (`hide_suppressed_avatars`), so the
//! radar and the minimap still show who is around. At a crowded event that is
//! exactly what you want: draw fewer people, not know fewer people.
//!
//! # One guarded way in
//!
//! Every Derender affordance writes a [`RequestDerender`] (never touching the
//! list itself), exactly as every Block affordance writes a
//! `RequestBlock`: `apply_derender_requests`
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
//! `index_derendered_objects` keeps a set of suppressed *region-scoped* ids,
//! seeding it from the blacklisted full ids and extending it to any object whose
//! parent is already in it. A linkset's child prims (parented to its root) and an
//! avatar's attachments (parented to the avatar object) are therefore suppressed
//! with their root — without which a derendered avatar would leave its
//! attachments floating, with no parent, at the scene origin.
//!
//! Anything already in the scene when the entry is added — or that arrived
//! before its parent did — is despawned by `enforce_derender`, which purges by
//! full id (the request path) and by scoped id (the index path).
//!
//! # Temporary vs permanent
//!
//! A **permanent** entry is written to the per-avatar blacklist file (a sibling
//! of the account `settings.toml`, like `crate::notification_persist`'s store)
//! and applies again on the next login. A **temporary** entry lives in the
//! session only and is dropped on the next teleport
//! (`clear_temporary_derenders`, the reference's
//! `LLViewerObjectList::resetDerenderList` on `LLAgent::teleportCore`, gated by
//! its `FSTempDerenderUntilTeleport` setting — ours is `SETTING_UNTIL_TELEPORT`).
//!
//! # Re-rendering
//!
//! Removing an entry does more than stop the suppression. The simulator streams
//! an object once, and every update for it was dropped while it was suppressed,
//! so forgetting the entry alone would leave the object absent until the region
//! streamed it again — which is all the reference does. Because the suppression
//! index kept each hidden object's *region-local* id the whole time, the release
//! instead queues those ids for a re-fetch (`RequestMultipleObjects`, a full
//! cache miss — `refetch_released_objects`) and the object comes back within
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
    AgentKey, Command, ScopedObjectId, SlAgentParcel, SlCommand, SlCurrentRegion, SlEvent,
    SlIdentity, SlRegionIdentity, SlSessionEvent, Uuid, pcode,
};
use sl_settings::SettingValue;
use tracing::{debug, info, warn};

use crate::avatars::AvatarState;
use crate::objects::ObjectState;
use crate::settings::ViewerSettings;
use crate::world_api::{DerenderKind, FriendsModel};

/// The per-account file the permanent blacklist is stored in (a sibling of the
/// account `settings.toml`). Our account directory is already per-grid and
/// per-avatar, so the bare name suffices.
const STORE_FILE: &str = "derender_blacklist.json";

/// The persisted-settings section the derender knobs live under.
const DERENDER_SECTION: &[&str] = &["derender"];

/// Whether a temporary derender lasts until the next teleport (the reference's
/// `FSTempDerenderUntilTeleport`, default on) or for the whole session.
pub(crate) const SETTING_UNTIL_TELEPORT: &str = "TempDerenderUntilTeleport";

/// The **friends-only** filter (`viewer-render-friends-only`, the reference's
/// `FSRenderFriendsOnly`): draw only friends' avatars. Per avatar, because it is
/// a per-avatar habit — and because the reference keeps it per account too.
pub const SETTING_FRIENDS_ONLY: &str = "RenderFriendsOnly";

/// Whether the friends-only filter survives a teleport (the reference's
/// `FSRenderFriendsOnlyPersistsTP`). Default **off**: the filter is a
/// "this event is too heavy" tool, so leaving a place is the natural moment to
/// stop hiding people — and the reference added this setting precisely because
/// users forgot it was on.
pub(crate) const SETTING_FRIENDS_ONLY_PERSISTS_TP: &str = "RenderFriendsOnlyPersistsTP";

// ---------------------------------------------------------------------------
// The model.
// ---------------------------------------------------------------------------

/// One blacklist entry: what was derendered, where and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerenderEntry {
    /// The derendered thing's persistent id (an object's full id, an avatar's
    /// agent id).
    pub id: Uuid,
    /// Its name, as the surface that derendered it knew it (may be empty when
    /// the object-properties reply had not landed yet).
    pub name: String,
    /// The region it was derendered in (empty when unknown).
    pub region: String,
    /// What kind of thing it is.
    pub kind: DerenderKind,
    /// Whether it survives a teleport and a relog (the "Blacklist" slice) or is
    /// a session-only "Temporary" derender.
    pub permanent: bool,
    /// When it was added, as Unix epoch seconds (stored as a plain integer so
    /// the file needs no date parser).
    pub added_epoch_secs: i64,
}

/// Why a region-scoped id is suppressed — which release frees it again.
///
/// Two sources share one suppression index (and therefore one ingest gate, one
/// transitive parent walk, one purge and one re-fetch): the **blacklist**, keyed
/// by the entry's id, and the **friends-only filter**, keyed by the non-friend
/// agent it hides. Keeping the source on each entry is what lets a release be
/// exact — un-blacklisting one object, or befriending one avatar, frees that
/// subtree and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HiddenBy {
    /// The blacklist entry with this id ([`DerenderList::remove`] frees it).
    Blacklist(Uuid),
    /// The friends-only filter, hiding this non-friend agent (turning the filter
    /// off — or befriending them — frees it).
    FriendsOnly(Uuid),
}

impl HiddenBy {
    /// The persistent id at the root of this suppression — a blacklist entry's
    /// id, or the hidden agent's.
    const fn id(self) -> Uuid {
        match self {
            Self::Blacklist(id) | Self::FriendsOnly(id) => id,
        }
    }
}

/// The viewer's derender / blacklist state, and the friends-only filter that
/// shares its suppression machinery.
#[derive(Resource, Debug, Default)]
pub struct DerenderList {
    /// The entries, newest last.
    entries: Vec<DerenderEntry>,
    /// The blacklisted ids and what each is blacklisted **as**, derived from
    /// [`Self::entries`] — the hot-path index every check goes through. Keyed by
    /// id alone: an id is one thing, so a second entry for it would be a
    /// contradiction, and the kind rides along so a sound check never matches an
    /// object entry.
    ids: HashMap<Uuid, DerenderKind>,
    /// The region-scoped ids currently suppressed, each mapped to **what hides
    /// it**: an entry's own object maps to its own source, a linkset child or
    /// attachment to its root's. Keeping the source is what lets a single
    /// release free exactly its own subtree (see [`Self::release`]).
    /// Session-derived, never persisted.
    hidden_scoped: HashMap<ScopedObjectId, HiddenBy>,
    /// Suppressions whose scene entities still need despawning, by source: a
    /// fresh blacklist entry, or an avatar the friends-only filter just started
    /// hiding.
    pending_ids: Vec<HiddenBy>,
    /// Scoped ids whose scene entities still need despawning (an object that
    /// was already tracked when its parent became hidden).
    pending_scoped: Vec<ScopedObjectId>,
    /// Scoped ids just **released** from suppression, to be re-fetched from the
    /// simulator so an un-derendered object comes back at once
    /// (`refetch_released_objects`).
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
    /// Whether the **friends-only** filter is on (`viewer-render-friends-only`,
    /// the reference's `FSRenderFriendsOnly`): while it is, every avatar that is
    /// not a friend and not the agent itself is suppressed exactly as a
    /// derendered one is.
    friends_only: bool,
    /// The agent's own id, which the filter never hides.
    own_agent: Option<Uuid>,
    /// The friends the filter spares, mirrored from
    /// [`FriendsModel`](crate::world_api::FriendsModel) so the per-object gate stays
    /// one hash lookup.
    friends: HashSet<Uuid>,
}

impl DerenderList {
    /// Whether `id` is blacklisted **as** `kind` — the query each point of use
    /// runs (a sound before playing it, an animation before running it, a
    /// texture before fetching it).
    #[must_use]
    pub fn blacklists(&self, id: Uuid, kind: DerenderKind) -> bool {
        self.ids.get(&id) == Some(&kind)
    }

    /// Whether `id` names an in-world thing this viewer must not draw — a
    /// blacklisted object / avatar, or an avatar the friends-only filter hides.
    /// The hot-path query the scene mirror runs per streamed object.
    pub(crate) fn hides_in_world(&self, id: Uuid) -> bool {
        self.blacklists_in_world(id) || self.friends_only_hides(id)
    }

    /// Whether `id` is on the **blacklist** as an in-world kind (as opposed to
    /// being hidden by the friends-only filter).
    fn blacklists_in_world(&self, id: Uuid) -> bool {
        self.ids.get(&id).is_some_and(|kind| kind.is_in_world())
    }

    /// Whether the friends-only filter hides the avatar `agent`: the filter is
    /// on, and they are neither the agent itself nor a friend. Animesh
    /// ("control") avatars are exempt for free — they are ordinary mesh objects
    /// on the wire, never `pcode` 47, so this gate never sees them, which is the
    /// reference's `!avatar->isControlAvatar()` by construction.
    pub(crate) fn friends_only_hides(&self, agent: Uuid) -> bool {
        self.friends_only && self.own_agent != Some(agent) && !self.friends.contains(&agent)
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
    /// `index_derendered_objects`.
    pub(crate) fn is_suppressed(&self, scoped: ScopedObjectId) -> bool {
        self.hidden_scoped.contains_key(&scoped)
    }

    /// What suppresses `scoped`, if anything — the source an inherited
    /// suppression is inherited from.
    fn suppressing_root(&self, scoped: ScopedObjectId) -> Option<HiddenBy> {
        self.hidden_scoped.get(&scoped).copied()
    }

    /// Record every id in `removed` as suppressed by the blacklisted `root`.
    ///
    /// The scene purge calls this with what it despawned, because those ids are
    /// often the *only* record of them: the simulator streams a static object
    /// once, so an object derendered long after it was streamed never produces
    /// another update for `index_derendered_objects` to learn from — and
    /// without the record, un-derendering it would have nothing to re-fetch.
    fn note_hidden(&mut self, removed: impl IntoIterator<Item = ScopedObjectId>, root: HiddenBy) {
        for scoped in removed {
            let _prior = self.hidden_scoped.insert(scoped, root);
        }
    }

    /// The whole list, in insertion order.
    #[must_use]
    pub fn entries(&self) -> &[DerenderEntry] {
        &self.entries
    }

    /// The list revision — a view stores the value it last built at and rebuilds
    /// when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
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
        self.pending_ids.push(HiddenBy::Blacklist(entry.id));
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
        // another blacklisted root's children (and anything the friends-only
        // filter hides) must stay hidden.
        self.release(|root| root == HiddenBy::Blacklist(id));
    }

    /// Drop every temporary entry (a teleport, or the floater's Clear temporary).
    fn clear_temporary(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.permanent);
        if self.entries.len() == before {
            return;
        }
        self.reindex();
        // Every suppression whose blacklist entry just left the list is
        // released; the permanent entries — and the friends-only filter — keep
        // theirs.
        let live: HashSet<Uuid> = self.ids.keys().copied().collect();
        self.release(|root| match root {
            HiddenBy::Blacklist(id) => !live.contains(&id),
            HiddenBy::FriendsOnly(_agent) => false,
        });
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
    fn release(&mut self, released: impl Fn(HiddenBy) -> bool) {
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

    /// Re-apply the friends-only filter after its inputs moved (the toggle
    /// flipped, the friends list changed, or the own agent became known): free
    /// everyone it no longer hides — queuing their re-fetch, so they come back
    /// without a relog — and queue a purge for every avatar it now does.
    ///
    /// `known` is the agents this viewer currently tracks; only they can have
    /// anything in the scene to purge, and anyone streaming in later is caught
    /// by the ingest gate instead.
    fn resync_friends_only(&mut self, known: &[Uuid]) {
        let spared: HashSet<Uuid> = self
            .hidden_scoped
            .values()
            .filter_map(|hidden| match hidden {
                HiddenBy::FriendsOnly(agent) => Some(*agent),
                HiddenBy::Blacklist(_id) => None,
            })
            .filter(|agent| !self.friends_only_hides(*agent))
            .collect();
        if !spared.is_empty() {
            self.release(
                |root| matches!(root, HiddenBy::FriendsOnly(agent) if spared.contains(&agent)),
            );
        }
        for agent in known {
            if self.friends_only_hides(*agent) {
                self.pending_ids.push(HiddenBy::FriendsOnly(*agent));
            }
        }
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
pub struct RequestDerender {
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
    pub fn new(id: Uuid, name: impl Into<String>, kind: DerenderKind, permanent: bool) -> Self {
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
pub struct UnDerender {
    /// The entry to drop, or [`Uuid::nil`] for "every temporary entry".
    pub id: Uuid,
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
pub struct DerenderPlugin;

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
                    sync_friends_only_filter,
                    clear_friends_only_on_teleport,
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
                // The purge, then the presence/visibility split it leaves behind
                // (a suppressed avatar keeps a hidden placeholder so the radar
                // and minimap still see it), then the flush.
                (
                    enforce_derender,
                    hide_suppressed_avatars,
                    flush_derender_list,
                )
                    .chain()
                    .after(crate::objects::update_objects)
                    .after(crate::avatars::update_avatar_objects)
                    .after(crate::avatars::update_coarse_avatars),
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
    settings.register_in(
        DERENDER_SECTION,
        SETTING_FRIENDS_ONLY,
        SettingValue::Bool(false),
        "Draw only friends' avatars (a crowded-event performance filter)",
    );
    settings.register_in(
        DERENDER_SECTION,
        SETTING_FRIENDS_ONLY_PERSISTS_TP,
        SettingValue::Bool(false),
        "Keep the friends-only filter on across a teleport",
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
    mut selection: ResMut<crate::world_api::SelectionSet>,
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

/// Rebuild the scene mirror for everything a release freed, so what was hidden
/// comes back at once instead of waiting for the region to stream it again (see
/// [`DerenderList::release`]).
///
/// It asks **our own session**, not the simulator. The session caches every
/// streamed object and keeps it current from the motion updates that arrive
/// whatever the viewer chose to draw, so re-emitting from there costs no round
/// trip and — decisively — works for **avatars**: `RequestMultipleObjects` is
/// resolved against prims (OpenSim's `Scene.RequestPrim` looks up a
/// `SceneObjectPart`), so a request naming an avatar's local id matches nothing
/// and the simulator answers with silence. That is exactly what turning the
/// friends-only filter off used to run into.
///
/// Runs **after** the release, so the suppression is already gone by the time
/// the re-emitted `ObjectUpdate`s reach the scene mirror, which applies them
/// like any other update.
pub(crate) fn refetch_released_objects(
    mut list: ResMut<DerenderList>,
    mut commands: MessageWriter<SlCommand>,
) {
    if list.pending_refetch.is_empty() {
        return;
    }
    let local_ids: Vec<ScopedObjectId> = core::mem::take(&mut list.pending_refetch);
    debug!(count = local_ids.len(), "re-rendering released objects");
    commands.write(SlCommand(Command::ResendCachedObjects { local_ids }));
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
/// hidden is queued for `enforce_derender` to despawn.
pub(crate) fn index_derendered_objects(
    mut events: MessageReader<SlEvent>,
    mut list: ResMut<DerenderList>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                let scoped = object.scoped_id();
                let own = object.full_id.uuid();
                // What hides this object in its own right wins; otherwise it
                // inherits whatever hides its parent — which is how a hidden
                // avatar takes its attachments (the whole performance win at a
                // crowded event) and a hidden root takes its linkset.
                let root = if list.blacklists_in_world(own) {
                    Some(HiddenBy::Blacklist(own))
                } else if list.friends_only_hides(own) && object.pcode == pcode::AVATAR {
                    Some(HiddenBy::FriendsOnly(own))
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
    poses: Query<&Transform>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::face_material::FaceMaterial>>,
) {
    if list.pending_ids.is_empty() && list.pending_scoped.is_empty() {
        return;
    }
    for source in core::mem::take(&mut list.pending_ids) {
        for scoped in objects.scoped_by_full_id(source.id()) {
            let removed = objects.derender_remove(scoped, &mut commands);
            list.note_hidden(removed, source);
        }
        // Where the body stands right now, so its placeholder can take over in
        // place and the radar never sees the avatar blink out.
        let agent = AgentKey::from(source.id());
        let at = avatars
            .anchor_of(agent)
            .and_then(|anchor| poses.get(anchor).ok())
            .map(|pose| pose.translation);
        avatars.derender_agent(agent, at, &mut commands, &mut meshes, &mut materials);
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

// ---------------------------------------------------------------------------
// The friends-only filter.
// ---------------------------------------------------------------------------

/// Keep the friends-only filter's mirrored inputs current — the setting, the
/// agent's own id, and the friends set — and re-apply it whenever any of them
/// moves.
///
/// The mirror exists so the per-object gate ([`DerenderList::friends_only_hides`])
/// is one hash lookup on a resource the scene mirror already holds: it runs for
/// every streamed object at a crowded event, which is exactly where this feature
/// is used.
pub(crate) fn sync_friends_only_filter(
    mut list: ResMut<DerenderList>,
    settings: Option<Res<ViewerSettings>>,
    identity: Res<SlIdentity>,
    friends: Option<Res<FriendsModel>>,
    avatars: Res<AvatarState>,
    mut mirrored_friends: Local<Option<u64>>,
) {
    let wanted = settings
        .as_deref()
        .and_then(|settings| settings.store().get_bool(SETTING_FRIENDS_ONLY).ok())
        .unwrap_or(false);
    let own = identity.agent_id.map(|agent| agent.uuid());
    let friends_revision = friends.as_deref().map(FriendsModel::revision);
    if list.friends_only == wanted && list.own_agent == own && *mirrored_friends == friends_revision
    {
        return;
    }
    let toggled = list.friends_only != wanted;
    list.friends_only = wanted;
    list.own_agent = own;
    *mirrored_friends = friends_revision;
    if let Some(friends) = friends.as_deref() {
        list.friends = friends.friend_ids();
    }
    let known: Vec<Uuid> = avatars
        .known_agents()
        .into_iter()
        .map(|(agent, _anchor)| agent.uuid())
        .collect();
    list.resync_friends_only(&known);
    if toggled {
        info!(on = wanted, "friends-only render filter toggled");
    }
}

/// Turn the friends-only filter off on a teleport, unless the user asked it to
/// stick ([`SETTING_FRIENDS_ONLY_PERSISTS_TP`]).
///
/// The filter is a "this event is too heavy for my machine" tool, so leaving is
/// the natural moment to stop hiding people — and the reference added the same
/// escape hatch (`FSRenderFriendsOnlyPersistsTP`) because users forgot it was
/// on and wondered where everyone went.
pub(crate) fn clear_friends_only_on_teleport(
    mut events: MessageReader<SlEvent>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    let mut teleporting = false;
    for event in events.read() {
        if matches!(&event.0, SlSessionEvent::TeleportStarted) {
            teleporting = true;
        }
    }
    if !teleporting {
        return;
    }
    let Some(settings) = settings.as_mut() else {
        return;
    };
    let store = settings.store();
    if !store.get_bool(SETTING_FRIENDS_ONLY).unwrap_or(false)
        || store
            .get_bool(SETTING_FRIENDS_ONLY_PERSISTS_TP)
            .unwrap_or(false)
    {
        return;
    }
    settings.set_account(SETTING_FRIENDS_ONLY, SettingValue::Bool(false));
    settings.save_async();
    info!("friends-only render filter cleared on teleport");
}

/// Hide (rather than despawn) the placeholder of every avatar the viewer is not
/// drawing, and un-hide the rest.
///
/// Presence and rendering are deliberately separate here. A suppressed avatar's
/// **body** is never built — that is the whole performance win, and it happens at
/// the ingest gate — but the cheap coarse placeholder the position path spawns is
/// kept and merely made invisible, so the radar and the minimap still know who is
/// around. At a crowded event that is the point: you turn the filter on to
/// survive the draw, not to stop tracking the crowd. The reference does the same
/// by different means (it kills the object and its radar reads the coarse
/// positions directly).
pub(crate) fn hide_suppressed_avatars(
    list: Res<DerenderList>,
    avatars: Res<AvatarState>,
    mut visibilities: Query<&mut Visibility>,
) {
    for (agent, anchor) in avatars.known_agents() {
        let Ok(mut visibility) = visibilities.get_mut(anchor) else {
            continue;
        };
        let wanted = if list.hides_in_world(agent.uuid()) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        visibility.set_if_neq(wanted);
    }
}

/// Drop the temporary derenders when the agent teleports (the reference's
/// `resetDerenderList` from `LLAgent::teleportCore`), unless the user turned
/// `SETTING_UNTIL_TELEPORT` off — then they last the whole session.
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
        DerenderEntry, DerenderKind, DerenderList, DerenderRefusal, HiddenBy, RequestDerender,
        check_derender,
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{CircuitId, RegionLocalObjectId, ScopedObjectId, Uuid};
    use std::collections::HashSet;

    /// An entry for `id`, permanent or not.
    pub(crate) fn entry(id: u128, permanent: bool) -> DerenderEntry {
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
        assert_eq!(
            list.pending_ids,
            vec![HiddenBy::Blacklist(Uuid::from_u128(1))]
        );
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
        list.note_hidden([scoped(10), scoped(11)], HiddenBy::Blacklist(first));
        list.note_hidden([scoped(20)], HiddenBy::Blacklist(second));
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
        list.note_hidden([scoped], HiddenBy::Blacklist(id));
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
        list.note_hidden([scoped(10)], HiddenBy::Blacklist(kept));
        list.note_hidden([scoped(20)], HiddenBy::Blacklist(dropped));
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

    /// Turn the friends-only filter on for `own`, sparing `friends`.
    fn filter_on(list: &mut DerenderList, own: u128, friends: &[u128]) {
        list.friends_only = true;
        list.own_agent = Some(Uuid::from_u128(own));
        list.friends = friends.iter().map(|id| Uuid::from_u128(*id)).collect();
    }

    /// The filter hides every avatar that is neither the agent itself nor a
    /// friend — and hides nobody at all while it is off.
    #[test]
    fn friends_only_spares_friends_and_self() {
        let mut list = DerenderList::default();
        let stranger = Uuid::from_u128(0xBEEF);
        assert!(!list.friends_only_hides(stranger), "off by default");

        filter_on(&mut list, 0xAAA, &[0xF1]);
        assert!(list.friends_only_hides(stranger));
        assert!(!list.friends_only_hides(Uuid::from_u128(0xF1)), "a friend");
        assert!(!list.friends_only_hides(Uuid::from_u128(0xAAA)), "yourself");
        // The scene mirror's gate is the union of both sources.
        assert!(list.hides_in_world(stranger));
        assert!(!list.blacklists_in_world(stranger), "not a blacklist entry");
    }

    /// Re-applying the filter purges whom it now hides and frees — with a
    /// re-fetch — whom it no longer does, leaving blacklist suppressions alone.
    #[test]
    fn friends_only_resync_purges_and_releases() {
        let mut list = DerenderList::default();
        let (stranger, new_friend) = (Uuid::from_u128(0xBEEF), Uuid::from_u128(0xF2));
        // A blacklisted object, to prove the filter's resync never frees it.
        list.add(entry(1, true));
        let scoped = |id: u32| ScopedObjectId::new(CircuitId::default(), RegionLocalObjectId(id));
        list.note_hidden([scoped(1)], HiddenBy::Blacklist(Uuid::from_u128(1)));
        // Both avatars are hidden by the filter.
        filter_on(&mut list, 0xAAA, &[]);
        list.note_hidden([scoped(10)], HiddenBy::FriendsOnly(stranger));
        list.note_hidden([scoped(11)], HiddenBy::FriendsOnly(new_friend));
        list.pending_ids.clear();
        list.pending_refetch.clear();

        // One of them becomes a friend: only their subtree is released.
        list.friends = HashSet::from([new_friend]);
        list.resync_friends_only(&[stranger, new_friend]);
        assert_eq!(list.pending_refetch, vec![scoped(11)]);
        assert!(list.is_suppressed(scoped(10)), "the stranger stays hidden");
        assert!(list.is_suppressed(scoped(1)), "the blacklist is untouched");
        assert_eq!(
            list.pending_ids,
            vec![HiddenBy::FriendsOnly(stranger)],
            "whoever the filter still hides is queued for the purge"
        );

        // Turning the filter off frees everyone it hid — and nobody else.
        list.pending_ids.clear();
        list.pending_refetch.clear();
        list.friends_only = false;
        list.resync_friends_only(&[stranger, new_friend]);
        assert_eq!(list.pending_refetch, vec![scoped(10)]);
        assert!(list.pending_ids.is_empty());
        assert!(
            list.is_suppressed(scoped(1)),
            "a blacklisted object must not come back with the filter"
        );
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
