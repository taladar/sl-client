//! World-state fixtures: the parcels and objects a region's sessions see.
//!
//! A real simulator pushes a burst of world state at an arriving viewer that
//! nothing ever requested — the agent's own avatar object, the parcel
//! overlay, the record of the parcel the agent stands on, and every object
//! in view — and keeps answering the viewer's refetches afterwards. The
//! [`SceneFixtures`] on a [`Scenario`](crate::Scenario) script that burst:
//! the driver emits it when the agent's movement completes (`AgentArrived`,
//! after the `RegionHandshake` that went out on `UseCircuitCode`), and
//! the driver replays the same fixtures for the client's
//! `ParcelPropertiesRequest` / `ParcelPropertiesRequestByID` /
//! `RequestMultipleObjects`.
//!
//! The fixture types are `sl-proto`'s own [`ParcelInfo`] and [`Object`] —
//! the records the client decodes — so a test asserts what it seeded.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::estate::EstateFixture;
use crate::fixtures::{NpcAppearance, NpcFixture};
use crate::scene_store::{ObjectMut, ObjectStore};
use crate::terrain::TerrainFixture;
use sl_proto::{
    AnimationKey, AssetKey, AssetSource as _, AssetType, DeRezDestination, FolderType,
    GlobalCoordinates, InventoryItem, InventoryType, LandStatReportType, LandStatScore, Object,
    ObjectExtraParams, ObjectMotion, ObjectPlayingAnimation, ObjectProperties, ParcelAccessEntry,
    ParcelAccessScope, ParcelCategory, ParcelDetails, ParcelInfo, ParcelRequestResult,
    ParcelStatus, PrimShape, PrimShapeParams, RegionIdentity, RegionLocalObjectId,
    RegionLocalParcelId, RegionTerrainComposition, SaleType, ServerEvent, SimSession,
    TaskInventoryItem, TerrainLayerType, TransactionId, pcode, prim_flags,
};
use sl_types::key::{
    AgentKey, ExperienceKey, InventoryFolderKey, InventoryKey, ObjectKey, OwnerKey, ParcelKey,
};
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::{Direction, LandArea, RegionCoordinates};
use sl_types::money::LindenAmount;

/// Metres along each edge of a parcel overlay / bitmap cell.
const CELL_METRES: f32 = 4.0;
/// Cells along each edge of a 256 m region's overlay.
const CELLS_PER_EDGE: usize = 64;
/// The parcel-overlay ownership class of public land.
const OVERLAY_PUBLIC: u8 = 0;
/// The parcel-overlay ownership class of land the requesting agent owns.
const OVERLAY_OWNED_BY_REQUESTER: u8 = 1;
/// The parcel-overlay ownership class of group-owned land.
const OVERLAY_OWNED_BY_GROUP: u8 = 2;
/// The parcel-overlay ownership class of land somebody else owns.
const OVERLAY_OWNED_BY_OTHER: u8 = 3;
/// The parcel-overlay ownership class of land for sale.
const OVERLAY_FOR_SALE: u8 = 4;
/// The parcel-overlay bit marking a cell on a parcel's west edge.
const OVERLAY_WEST_LINE: u8 = 0x40;
/// The parcel-overlay bit marking a cell on a parcel's south edge.
const OVERLAY_SOUTH_LINE: u8 = 0x80;
/// The physics time dilation reported on fixture object updates: real time.
pub(crate) const REAL_TIME_DILATION: u16 = 0xFFFF;
/// The sequence id of an unsolicited agent-parcel push (what OpenSim's
/// `SendLandUpdateToClient` sends).
pub(crate) const UNSOLICITED_SEQUENCE_ID: i32 = 0;

/// The most objects one `KillObject` names: its block count is a byte.
const MOST_KILLS_A_MESSAGE: usize = 255;

/// The parcels and objects of one region, pushed at every arriving agent
/// and replayed on request.
#[derive(Debug, Clone)]
pub struct SceneFixtures {
    /// The region's parcels; the first whose bitmap covers a point answers
    /// a request for it. The stock scenario carries one region-wide parcel
    /// ([`region_wide_parcel`]).
    pub parcels: Vec<ParcelInfo>,
    /// The grid-wide half of each parcel in [`parcels`], keyed by its
    /// region-local id: what a `ParcelProperties` record does not carry and a
    /// `ParcelDwellRequest` / `ParcelInfoRequest` / `RemoteParcelRequest` asks
    /// for. Push both halves together with [`add_parcel`](Self::add_parcel).
    ///
    /// A parcel with no listing is answered as any other — it simply has no
    /// grid-wide identity, so a location inside it resolves to nothing and its
    /// dwell and search listing go unanswered, the way a region whose land
    /// service is down behaves.
    ///
    /// [`parcels`]: Self::parcels
    pub listings: Vec<ParcelListing>,
    /// The objects rezzed in the region, sent in one full `ObjectUpdate` on
    /// arrival (after the agent's own avatar object, which the driver adds
    /// from the login), in the order they were added.
    ///
    /// An indexed store ([`ObjectStore`]) rather than a list: a prim is found
    /// by either of its ids, and what the region keeps of it besides its
    /// record — its contents, its cost, its edit history — is held with it.
    pub objects: ObjectStore,
    /// The scripted avatars the region shows beside its logged-in residents
    /// (`Resident`): people a test wants without logging a viewer in. Each
    /// contributes its avatar body, an `AvatarAppearance`, an
    /// `AvatarAnimation` and its attachments to the arrival burst, and answers
    /// object refetches like any other object.
    pub npcs: Vec<NpcFixture>,
    /// The animations signalled on the region's **animated objects**
    /// (animesh), pushed as one `ObjectAnimation` each on arrival.
    ///
    /// An animesh is an ordinary rigged-mesh prim carrying the extended-mesh
    /// `ANIMATED_MESH_ENABLED` flag ([`PrimFixture::animated_mesh`]); what
    /// makes it *move* is this, a separate message keyed by the object's full
    /// id rather than anything in the object update.
    ///
    /// [`PrimFixture::animated_mesh`]: crate::fixtures::PrimFixture::animated_mesh
    pub object_animations: Vec<ObjectAnimationFixture>,
    /// The region-local id the region's **first** arriving avatar takes —
    /// one a fixture can name, as a scenario that dresses the agent does.
    /// Every later avatar is minted one of its own ([`avatar_ids`]), as a real
    /// simulator mints one per avatar; none may collide with [`objects`].
    ///
    /// [`objects`]: Self::objects
    /// [`avatar_ids`]: Self::avatar_id
    pub avatar_local_id: RegionLocalObjectId,
    /// The region-local id each avatar in the region has, from its arrival
    /// until it leaves. An id is never handed out twice, so an avatar that
    /// comes back is given a new one, as both grids give it.
    avatar_ids: BTreeMap<AgentKey, RegionLocalObjectId>,
    /// Whether [`avatar_local_id`](Self::avatar_local_id) has been given to an
    /// avatar: once it has, every later one is minted.
    first_avatar_arrived: bool,
    /// The agents' avatars standing in the region, as the region's other
    /// viewers are shown them ([`Resident`]): from the arrival to the
    /// departure — a logout, a teleport away, a walk over a border.
    ///
    /// The region's, not a session's, because a session answers its packets
    /// with nothing but its own state and the region's: this is how one
    /// viewer's arrival, and everything after it, is known to the rest.
    residents: BTreeMap<AgentKey, Resident>,
    /// The highest region-local id [`mint_local_id`] has handed out, so a
    /// second rez cannot reuse the id of an object a first one has since had
    /// derezzed. Never lowered.
    ///
    /// [`mint_local_id`]: Self::mint_local_id
    last_minted_local_id: RegionLocalObjectId,
    /// Each parcel's allow and ban lists, keyed by the parcel and which list it
    /// is. They live here rather than on the [`ParcelInfo`] because that is the
    /// wire record a `ParcelProperties` carries and it has no field for them: a
    /// viewer asks for a list with a request of its own and is answered with a
    /// reply of its own.
    ///
    /// A parcel with no entry has two empty lists, which is the state of every
    /// parcel a fixture did not speak about.
    parcel_access: BTreeMap<RegionLocalParcelId, ParcelAccessLists>,
    /// The region's estate record: what the Region/Estate floater reads and
    /// writes ([`crate::estate`]).
    pub estate: EstateFixture,
    /// The region's configuration as an estate manager has changed it, or
    /// [`None`] while it is still what [`region_limits`] derives from the
    /// region's identity.
    ///
    /// Lazily written rather than stated, so a region nobody has reconfigured
    /// reports exactly what its own identity says and cannot drift from it.
    limits: Option<sl_proto::RegionLimits>,
    /// The region's restart schedule, as an estate manager wrote it over
    /// the `RegionSchedule` capability; `None` for none.
    pub restart_schedule: Option<sl_proto::RegionSchedule>,
    /// The region's PBR terrain overrides, as an estate manager wrote them
    /// over the `ModifyRegion` capability: none on any slot until then,
    /// stated `undef` as Second Life states them (`terrain-editing`,
    /// 2026-10-18).
    pub terrain_material_overrides: sl_proto::TerrainMaterialOverrides,
    /// What the region holds on the grid being imitated
    /// ([`ImitatedGrid::region_capacity`](crate::ImitatedGrid::region_capacity)),
    /// set when the grid starts; `None` in a world no grid has adopted, which
    /// reports [`region_limits`]' own numbers.
    pub(crate) capacity: Option<crate::imitates::RegionCapacity>,
    /// How the region answers a select and tells of a change to an object's
    /// record ([`ImitatedGrid::properties_policy`](crate::ImitatedGrid::properties_policy)),
    /// set when the grid starts; Second Life's in a world no grid has adopted.
    pub(crate) properties: crate::imitates::PropertiesPolicy,
    /// How the region takes an object out and puts one back
    /// ([`ImitatedGrid::rez_policy`](crate::ImitatedGrid::rez_policy)), set
    /// when the grid starts; Second Life's in a world no grid has adopted.
    pub(crate) rez: crate::imitates::RezPolicy,
    /// How the region seats and stands an avatar
    /// ([`ImitatedGrid::sit_policy`](crate::ImitatedGrid::sit_policy)), set
    /// when the grid starts; Second Life's in a world no grid has adopted.
    pub(crate) sits: crate::imitates::SitPolicy,
    /// What the region makes of an edit of an object, where the live grids
    /// disagree. Second Life's unless the grid's flavour says otherwise.
    pub(crate) edit: crate::imitates::EditPolicy,
    /// What the region makes of a link and a delink, where the live grids
    /// disagree. Second Life's unless the grid's flavour says otherwise.
    pub(crate) linking: crate::imitates::LinkPolicy,
    /// What the region says of a write into a prim's contents, where the live
    /// grids disagree. Second Life's unless the grid's flavour says otherwise.
    pub(crate) contents: crate::imitates::ContentsPolicy,
    /// Which `changed` a viewer's change raises in a prim's scripts, where
    /// the live grids disagree. Second Life's unless the grid's flavour says
    /// otherwise.
    pub(crate) changes: crate::imitates::ChangePolicy,
    /// How the region passes a viewer's effects and sounds on to the other
    /// viewers, where the live grids disagree. Second Life's unless the
    /// grid's flavour says otherwise.
    pub(crate) relays: crate::relays::RelayPolicy,
    /// How the region shows its residents to each other, where the live
    /// grids disagree. Second Life's unless the grid's flavour says
    /// otherwise.
    pub(crate) presence: crate::imitates::PresencePolicy,
    /// How the region wears and takes off attachments, where the live grids
    /// disagree ([`crate::attachments`]). Second Life's unless the grid's
    /// flavour says otherwise.
    pub(crate) attachments: crate::attachments::AttachmentPolicy,
    /// What each agent on its way here wears, packed by the region it left
    /// and put back on at its arrival ([`crate::attachments::pack`]).
    pub(crate) luggage: BTreeMap<AgentKey, Vec<crate::attachments::Carried>>,
    /// The region-local id each worn prim had when it left with its wearer,
    /// which OpenSim gives it again when it comes back
    /// ([`AttachmentPolicy::returning_keeps_local_id`](crate::attachments::AttachmentPolicy::returning_keeps_local_id)).
    pub(crate) departed_ids: BTreeMap<ObjectKey, RegionLocalObjectId>,
    /// What came off an avatar and waits out a tick before it leaves
    /// ([`crate::attachments::Leaving`]).
    pub(crate) leaving: Vec<crate::attachments::Leaving>,
    /// How the region answers a script upload, where the live grids
    /// disagree ([`crate::script_compile`]). Second Life's unless the grid's
    /// flavour says otherwise.
    pub(crate) scripts: crate::script_compile::CompilePolicy,
    /// How the region passes local chat on, where the live grids disagree.
    /// Second Life's unless the grid's flavour says otherwise.
    pub(crate) chat: crate::chat::ChatPolicy,
    /// How the region states its record, where the live grids disagree.
    /// Second Life's unless the grid's flavour says otherwise.
    pub(crate) region_info: crate::imitates::RegionInfoPolicy,
    /// The region's terrain textures and blend heights as an estate manager has
    /// changed them, or [`None`] while they are still the identity's.
    ///
    /// Lazily written for the same reason as [`limits`](Self::limits), and kept
    /// here rather than on the identity because the identity is a per-session
    /// copy: a terrain texture is the region's, not one viewer's.
    terrain_composition: Option<RegionTerrainComposition>,
    /// The listens registered in the region: what hears a line on a channel
    /// no avatar does ([`crate::chat`]).
    pub listens: crate::chat::Listens,
    /// Whether the region leaves its LAND layer out of what it streams to an
    /// arriving agent ([`FakeGrid::set_withholds_ground`](crate::FakeGrid::set_withholds_ground)):
    /// a test's stand-in for the login window before a region's ground has
    /// streamed, held open for as long as the test needs. Wind and cloud
    /// still go out.
    pub(crate) withholds_ground: bool,
    /// What the region's viewers report of each other's complexity over
    /// `AvatarRenderInfo`, and what it states back ([`crate::render_info`]).
    /// Not a fixture: empty until a viewer reports.
    pub(crate) render_info: crate::render_info::RegionRenderInfo,
    /// The scripts running in the region ([`crate::scripts`]). Not a fixture:
    /// a scene states its scripts as items in its prims' contents, and the
    /// region starts an instance of each.
    pub script_instances: crate::scripts::RegionScripts,
    /// What the region's land says of experiences: the estate's lists and
    /// which experiences each parcel admits ([`crate::experiences`]). An
    /// experience script's standing is judged by it.
    pub experience_land: crate::experiences::ExperienceLand,
    /// The touches held open in the region and its prims' `llPassTouches`
    /// settings ([`crate::touches`]). Not a fixture.
    pub touches: crate::touches::Touches,
    /// What its scripts set on its objects that physics reads, how each
    /// physical object moves and what each prim touches
    /// ([`crate::physics`]). Not a fixture.
    pub physics: crate::physics::Physics,
    /// The grid's wall clock, which the region stamps calendar times from (an
    /// access pass's expiry). The grid hands every region its own; a world
    /// built by hand reads the machine's.
    pub wall: crate::time::WallClock,
}

/// One agent's avatar standing in a region, as the region's **other** viewers
/// are shown it.
///
/// A real simulator sends every avatar in the region to every agent: its
/// object, its appearance and its animations on arrival, terse updates as it
/// moves, and a `KillObject` when it leaves (`avatar-presence`, 2026-10-08,
/// `book/src/gridspec/avatars.md`). What a newcomer is sent of the avatars
/// already here is read from this record; what changes after is published
/// as a [`RegionChange`] and the record kept in step.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resident {
    /// Its body as a full `ObjectUpdate` states it: standing in the region,
    /// or a seat's child at its offset from the seat.
    pub(crate) body: Object,
    /// Where it stands in the region's own frame, which the coarse feed, the
    /// world map and a parcel's privacy go by: the body's position, or a
    /// seated one's seat's plus its offset.
    pub(crate) at: Vector,
    /// Its appearance as the others are sent it.
    pub(crate) appearance: sl_proto::AvatarAppearance,
    /// Its animations as the others are told them.
    pub(crate) animations: crate::animations::Statement,
}

/// One parcel's two access lists: who may come in, and who may not.
///
/// Two fields rather than a map keyed by the scope, because there are exactly
/// two and a wire scope byte that named neither would have nowhere to go.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParcelAccessLists {
    /// The allow list (`AL_ACCESS`).
    pub access: Vec<ParcelAccessEntry>,
    /// The ban list (`AL_BAN`).
    pub ban: Vec<ParcelAccessEntry>,
}

impl ParcelAccessLists {
    /// The list `scope` names, to read.
    #[must_use]
    pub fn list(&self, scope: ParcelAccessScope) -> &[ParcelAccessEntry] {
        match scope {
            ParcelAccessScope::Ban => &self.ban,
            _ => &self.access,
        }
    }

    /// The list `scope` names, to change.
    pub const fn list_mut(&mut self, scope: ParcelAccessScope) -> &mut Vec<ParcelAccessEntry> {
        match scope {
            ParcelAccessScope::Ban => &mut self.ban,
            _ => &mut self.access,
        }
    }
}

/// Where an object stands, how it is turned and how large it is: what an
/// `Undo` steps back out of.
///
/// Both live grids keep a history of an object's moves, turns and resizes and
/// of nothing else (`object-edit`, 2026-10-10): a rename, a sale, a material,
/// a flag and a shape stay as they are through any number of undos.
#[derive(Debug, Clone, PartialEq)]
struct Placed {
    /// Its position: in the region, or in its root's frame.
    position: Vector,
    /// Its rotation.
    rotation: Rotation,
    /// Its size.
    scale: Vector,
}

impl Placed {
    /// How `object` is placed.
    fn of(object: &Object) -> Self {
        Self {
            position: object.motion.position.clone(),
            rotation: object.motion.rotation.clone(),
            scale: object.scale.clone(),
        }
    }

    /// Places `object` so.
    fn place(&self, object: &mut Object) {
        object.motion.position.clone_from(&self.position);
        object.motion.rotation.clone_from(&self.rotation);
        object.scale.clone_from(&self.scale);
    }
}

/// One object's history of moves, turns and resizes: how it was placed before
/// each of the last few, and — once something has been undone — how it was
/// placed before each undo.
///
/// How many it keeps is the grid's
/// ([`EditPolicy::undo_depth`](crate::imitates::EditPolicy::undo_depth)). The
/// reference viewer keeps asking after the simulator has run out: an `Undo`
/// with nothing left to undo is answered with silence, not an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct EditHistory {
    /// The placements to go back to, oldest first.
    past: Vec<Placed>,
    /// The placements an undo stepped out of, oldest first — what a redo
    /// returns to. Cleared by any fresh edit, because a new edit makes the
    /// branch it held unreachable.
    future: Vec<Placed>,
}

impl EditHistory {
    /// Records `before` as a placement an undo can return to, dropping the
    /// oldest when `depth` are held and abandoning any redo branch.
    fn record(&mut self, before: Placed, depth: usize) {
        while self.past.len() >= depth.max(1) {
            let _oldest = self.past.remove(0);
        }
        self.past.push(before);
        self.future.clear();
    }

    /// Steps one place back (`backwards`) or forward, handing over the
    /// placement to restore and keeping `current` on the other stack. [`None`]
    /// when that direction has run out.
    fn step(&mut self, current: Placed, backwards: bool) -> Option<Placed> {
        let (from, to) = if backwards {
            (&mut self.past, &mut self.future)
        } else {
            (&mut self.future, &mut self.past)
        };
        let restored = from.pop()?;
        to.push(current);
        Some(restored)
    }
}

/// One region's world behind its lock: a single store every session in that
/// region reads and writes, rather than a copy each.
///
/// [`parking_lot::Mutex`] rather than the standard one because the driver
/// takes it inside a synchronous flush and holds it for a handful of map
/// lookups: there is nothing to await under it, and a lock that cannot be
/// poisoned needs no error path a fake grid has no answer for.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- a `RegionEntry`'s and a \
              `SimState`'s field type -- where a bare `World` would not say \
              whose world it is"
)]
pub type RegionWorld = Arc<parking_lot::Mutex<SceneFixtures>>;

/// A change to a region's shared world, and which session made it.
///
/// The fake grid runs no simulation loop, so nothing sweeps the region for
/// changes and streams them: the session that made the change publishes it and
/// a per-session watcher task streams it to every *other* viewer in the
/// region. "Other" is what [`source`](Self::source) is for — the session that
/// made the change was told in the same breath as the mutation, and telling it
/// again would send every rez down its circuit twice.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionUpdate {
    /// The sequence number of the session that made the change — the one
    /// session that must *not* be sent it again.
    pub source: u64,
    /// What changed.
    pub change: RegionChange,
}

/// A change one session made to its region's shared world.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RegionChange {
    /// An object was added to the region; every other viewer needs its
    /// `ObjectUpdate`.
    Rezzed(Box<Object>),
    /// An object already in the region changed — it moved, was resized, was
    /// relinked, changed material — and every other viewer needs its new
    /// `ObjectUpdate`.
    ///
    /// Distinct from [`Rezzed`](Self::Rezzed) only in what it means, since a
    /// full `ObjectUpdate` is how a simulator says both: a viewer that has the
    /// object updates it, and one that does not rezzes it. Keeping them apart
    /// is for the reader of a change stream, not for the wire.
    Updated(Box<Object>),
    /// An object already in the region went somewhere or turned, and nothing
    /// else about it changed: every other viewer needs its terse update, as
    /// both live grids tell of a prim a held mouse button drags.
    Moved(Box<Object>),
    /// An object left the region; every other viewer needs its `KillObject`.
    Killed(RegionLocalObjectId),
    /// What one heartbeat tick changed in the region's prims, coalesced to
    /// one entry per prim ([`crate::updates`]): every viewer is sent the
    /// burst's full updates, its terse updates and its kills, one message of
    /// each. Only the region publishes it.
    Burst(Box<crate::updates::UpdateBurst>),
    /// Something was written into an object's task inventory, which its
    /// **properties** record announces in its
    /// [`inventory_serial`](sl_proto::ObjectProperties::inventory_serial).
    ///
    /// The one change in this enum that does not go to the whole region, and
    /// one only a Second-Life-flavoured grid publishes. Second Life sends the
    /// record to the clients holding the object *selected*, so the watcher
    /// forwards this only to a session whose selection names it — which is
    /// what makes the push a subscription rather than a broadcast, and why the
    /// local id travels alongside the record a selection is keyed by. OpenSim
    /// tells the writer alone. An *edit* of the record — a rename, a price, a
    /// permission — is not published on either grid: it is answered to the
    /// session that made it and to nobody else.
    PropertiesChanged {
        /// The region-local id the selection subscription is keyed by.
        local_id: RegionLocalObjectId,
        /// The record as it now stands, contents serial included.
        properties: Box<ObjectProperties>,
    },
    /// A script in an object started, which on Second Life moves its contents
    /// serial again: every session holding the object selected is sent a
    /// `ReplyTaskInventory` it did not ask for, naming the listing as it now
    /// stands. A subscription like [`PropertiesChanged`](Self::PropertiesChanged),
    /// and one only a Second-Life-flavoured grid publishes.
    ContentsListed {
        /// The region-local id the selection subscription is keyed by.
        local_id: RegionLocalObjectId,
    },
    /// A parcel's record changed — an About Land save, a purchase, a deed, an
    /// abandonment. A parcel has one record and a simulator re-sends the whole
    /// of it as an unsolicited `ParcelProperties`, to the avatars standing on
    /// that parcel.
    ParcelChanged(Box<ParcelInfo>),
    /// Land changed hands, so the region's ownership overlay reads
    /// differently: every session re-sends its viewer the overlay as that
    /// viewer sees it (whose land is whose is relative to the viewer), as
    /// both live grids do after a purchase, a deed, an abandonment or a
    /// reclaim.
    OverlayChanged,
    /// The region's own configuration changed (the estate floater's Region and
    /// Terrain tabs); every session in the region needs the new `RegionInfo`,
    /// and its `SimStats` the new flags.
    RegionConfigured(Box<sl_proto::RegionLimits>),
    /// The region's flags changed with nothing else (the estate floater's
    /// Debug tab): both live grids tell of it in `SimStats` alone.
    RegionFlagsChanged(sl_proto::RegionFlags),
    /// An estate manager's notice to the region — a region or estate
    /// message, a restart notice — which every avatar in it is sent.
    Notice(Box<crate::estate::Notice>),
    /// The region's ground textures changed (the estate floater's Terrain tab,
    /// on "Apply"). The only message a terrain composition travels in is a
    /// `RegionHandshake`, and a handshake is stamped with the *receiving*
    /// session's identity, so what travels here is the composition rather than
    /// a finished message.
    TerrainRetextured(Box<RegionTerrainComposition>),
    /// An agent changed its display name over `SetDisplayName`. Every other
    /// session stores the new record (so its `GetDisplayNames` answers it)
    /// and pushes its viewer the `DisplayNameUpdate`, as the agent's own
    /// session already did.
    DisplayNameChanged(Box<sl_proto::DisplayNameUpdate>),
    /// A line was said in the region ([`crate::chat`]). The one change that
    /// reaches the session that made it too: a speaker hears its own line,
    /// which is how its viewer shows it.
    Chat(Box<crate::chat::Line>),
    /// A viewer sent effects — a gaze, a pointing, a beam — for the other
    /// viewers ([`crate::relays`]). Each session's watcher asks the grid's
    /// policy whether its own is sent them; on Second Life that includes the
    /// session that sent them.
    Effects(Box<crate::relays::EffectsSent>),
    /// A viewer triggered a sound — a gesture's — for the avatars around it
    /// ([`crate::relays`]). Like a line of chat it reaches the session that
    /// made it too, on both grids.
    Sound(Box<sl_proto::TriggeredSound>),
    /// The region stood `agent` up itself — a script's `llUnSit`, its seat
    /// gone from the region — and it stands at `stands_at`. That agent's own
    /// session, the one that made the change among them, forgets the sit and
    /// re-sends the avatar standing there, and tells the others with a
    /// [`ResidentShown`](Self::ResidentShown).
    Unseated {
        /// The avatar.
        agent: AgentKey,
        /// Where it now stands, in the region.
        stands_at: Vector,
    },
    /// The region seated `agent` itself (`llSitOnLink`): its session sends
    /// the sit response a request would have had, records the sit and
    /// re-sends the avatar as the seat's child, and tells the others with a
    /// [`ResidentShown`](Self::ResidentShown).
    Seated {
        /// The avatar.
        agent: AgentKey,
        /// The prim it sits on.
        seat: Box<Object>,
        /// The response.
        transform: Box<sl_proto::SitTransform>,
        /// Where on the prim it sits, as its own update states it.
        offset: Vector,
        /// Its turn there.
        rotation: Rotation,
    },
    /// An object was put on `agent`, the records the root first: every
    /// session the wearer is shown to is sent them worn, the wearer's own
    /// among them, and any other kills them (`seating::shown_to`).
    Worn {
        /// The wearer.
        agent: AgentKey,
        /// The records, the root first.
        objects: Vec<Object>,
    },
    /// A script took its worn object off (`llDetachFromAvatar`): the
    /// wearer's session files it in its inventory, as a detach from the
    /// viewer would.
    TakeOff {
        /// The wearer.
        agent: AgentKey,
        /// The worn root.
        root: RegionLocalObjectId,
    },
    /// A script put its object on `agent` (`llAttachToAvatar`): that
    /// agent's session files it as a new item in its inventory, sends it
    /// worn and tells the others with a [`Worn`](Self::Worn).
    FileWorn {
        /// The wearer.
        agent: AgentKey,
        /// The worn root.
        root: RegionLocalObjectId,
    },
    /// `agent`'s avatar arrived in the region and is shown (`Resident`):
    /// every other viewer is sent its object, its appearance and its
    /// animations, in that order, as both live grids introduce an avatar.
    /// What it is sent is read from the region's record, so a watcher that
    /// wakes after the avatar left sends nothing.
    ResidentArrived(AgentKey),
    /// `agent`'s avatar sat, stood or was put somewhere: every other viewer
    /// is sent its whole object again.
    ResidentShown {
        /// The avatar.
        agent: AgentKey,
        /// Its object as it now stands.
        body: Box<Object>,
    },
    /// `agent`'s avatar moved: every other viewer is sent the terse update
    /// its own viewer was, at the same cadence, as both live grids send it.
    ResidentMoved {
        /// The avatar.
        agent: AgentKey,
        /// The update.
        update: Box<sl_proto::TerseUpdate>,
    },
    /// `agent`'s animations were stated to its own viewer: every other viewer
    /// is told the same set under the same sequence numbers, in the shape a
    /// statement to somebody else takes
    /// ([`Statement`](crate::animations::Statement)).
    ResidentAnimated {
        /// The avatar.
        agent: AgentKey,
        /// The statement.
        statement: Box<crate::animations::Statement>,
    },
    /// An avatar's appearance changed — a bake the grid accepted on Second
    /// Life, a publish on OpenSim: every other viewer is sent the record.
    ResidentAppearance(Box<sl_proto::AvatarAppearance>),
    /// `agent`'s avatar left the region — logged out, teleported away, walked
    /// over a border: every other viewer is sent the `KillObject` of the id
    /// it was shown under.
    ResidentLeft {
        /// The avatar.
        agent: AgentKey,
        /// The id it was shown under.
        local_id: RegionLocalObjectId,
    },
}

impl RegionChange {
    /// The agent whose avatar a resident change is about, for a watcher that
    /// sends nobody news of its own avatar; [`None`] for any other change.
    #[must_use]
    pub(crate) const fn resident(&self) -> Option<AgentKey> {
        match self {
            Self::ResidentArrived(agent)
            | Self::ResidentShown { agent, .. }
            | Self::ResidentMoved { agent, .. }
            | Self::ResidentAnimated { agent, .. }
            | Self::ResidentLeft { agent, .. } => Some(*agent),
            Self::ResidentAppearance(appearance) => Some(appearance.avatar_id),
            _ => None,
        }
    }
}

/// Where the object `key` among `objects` hears from, region-local metres:
/// its own position for a linkset root, its root's for a child prim (whose own
/// position is an offset from it).
pub(crate) fn listening_position(objects: &ObjectStore, key: ObjectKey) -> Option<Vector> {
    let object = objects.get_by_key(key)?;
    let root = if object.parent_id.0 == 0 {
        object
    } else {
        objects.get(object.parent_id).unwrap_or(object)
    };
    // A worn object hears and speaks where its wearer stands.
    if let Some(attached) = objects.attached_to(root.local_id) {
        return Some(attached.at.clone());
    }
    Some(root.motion.position.clone())
}

/// The name a viewer gives a prim nobody has named — the reference viewer's
/// `DEFAULT_OBJECT_NAME`, and what a take files an unnamed prim away as.
const DEFAULT_OBJECT_NAME: &str = "Object";

/// The creation date stamped on an inventory item the grid mints
/// (2023-11-14T22:13:20Z).
///
/// A fixed instant rather than the wall clock, for the reason nothing else in
/// this crate reaches for `Instant::now` either: a fake grid built from a seed
/// mints the same run twice, and a timestamp read off the machine would be the
/// one field that never matched.
const FIXTURE_CREATION_DATE: i32 = 1_700_000_000;

/// How many microseconds a second holds: an object's creation date is stated
/// in them.
const MICROSECONDS_PER_SECOND: u64 = 1_000_000;

/// The permissions of a prim nobody has changed the permissions of: OpenSim's
/// `SceneObjectPart` defaults. The owner may do anything, the group and
/// everyone else nothing, and a next owner may move and transfer it.
const NEW_OBJECT_PERMISSIONS: sl_wire::Permissions5 = sl_wire::Permissions5 {
    base: sl_wire::Permissions::ALL,
    owner: sl_wire::Permissions::ALL,
    group: sl_wire::Permissions::NONE,
    everyone: sl_wire::Permissions::NONE,
    next_owner: sl_wire::Permissions::MOVE.union(sl_wire::Permissions::TRANSFER),
};

/// One in-world object's task (prim) inventory: what it contains and the
/// serial that says so.
///
/// The serial is the contract with the viewer: a simulator bumps it on every
/// change, and a viewer that sees the same serial twice keeps the listing it
/// already downloaded. [`write`](Self::write) is therefore the only way to put
/// an item in, because it is the only way that also advances it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskInventory {
    /// The contents serial reported in the `ReplyTaskInventory`.
    pub serial: i16,
    /// The items the object contains.
    pub items: Vec<TaskInventoryItem>,
    /// The experience each script among them was last compiled under, for the
    /// scripts that are under one: what a `GetMetadata` is answered from.
    pub experiences: BTreeMap<InventoryKey, ExperienceKey>,
    /// The scripts among them asked to run — by the viewer's Running
    /// checkbox, a `RezScript`, an upload, a fixture — which is how a new
    /// instance of one starts. A script with an instance is then answered
    /// from the instance ([`crate::scripts`]); one with none (it does not
    /// compile) from this.
    pub running: BTreeSet<InventoryKey>,
    /// The scripts among them whose last upload did not compile on a grid
    /// that then keeps no instance of them (OpenSim): a `GetScriptRunning`
    /// about one of them is not answered
    /// ([`FailedScript::NoInstance`](crate::script_compile::FailedScript::NoInstance)).
    pub uncompiled: BTreeSet<InventoryKey>,
}

impl TaskInventory {
    /// An inventory holding `items`, already at `serial` — a fixture stating
    /// what an object contains before anybody logged in.
    #[must_use]
    pub const fn stated(serial: i16, items: Vec<TaskInventoryItem>) -> Self {
        Self {
            serial,
            items,
            experiences: BTreeMap::new(),
            running: BTreeSet::new(),
            uncompiled: BTreeSet::new(),
        }
    }

    /// Advances the serial without changing the listing: what Second Life
    /// does when a script in the prim starts or stops.
    pub const fn touch(&mut self) {
        self.serial = self.serial.saturating_add(1);
    }

    /// `name`, or `name` with the first number after it that no item here
    /// has: both grids call the second copy of `Note` in a prim `Note 1`.
    #[must_use]
    pub fn free_name(&self, name: &str) -> String {
        let taken = |candidate: &str| self.items.iter().any(|held| held.name == candidate);
        if !taken(name) {
            return name.to_owned();
        }
        (1_u32..)
            .map(|number| format!("{name} {number}"))
            .find(|candidate| !taken(candidate))
            .unwrap_or_else(|| name.to_owned())
    }

    /// Takes the item `item_id` out and advances the serial. `None`, and the
    /// serial left alone, when the object holds no such item.
    pub fn remove(&mut self, item_id: InventoryKey) -> Option<TaskInventoryItem> {
        let index = self.items.iter().position(|held| held.item_id == item_id)?;
        let _experience = self.experiences.remove(&item_id);
        let _was_running = self.running.remove(&item_id);
        let _was_uncompiled = self.uncompiled.remove(&item_id);
        self.serial = self.serial.saturating_add(1);
        Some(self.items.remove(index))
    }

    /// Writes `item` in, replacing an entry with the same id, and advances the
    /// serial.
    ///
    /// The serial saturates rather than wrapping: a wrap would let a listing
    /// look unchanged after 32768 writes, and no fake-grid session lives that
    /// long — the saturation is a boundary that cannot be reached, not a
    /// behaviour anything depends on.
    pub fn write(&mut self, item: TaskInventoryItem) {
        self.items.retain(|held| held.item_id != item.item_id);
        self.items.push(item);
        self.serial = self.serial.saturating_add(1);
    }
}

/// The grid-wide identity of one parcel: the half a region-local
/// [`ParcelInfo`] record has no field for.
///
/// Three surfaces read it and a live grid answers all three from the same land
/// record, which is why one fixture states it once:
///
/// - the `RemoteParcelRequest` capability, which turns a location into
///   [`parcel_id`](Self::parcel_id) (registered as a
///   [`SimParcel`](sl_proto::SimParcel) cover when the session starts),
/// - a `ParcelDwellRequest`, answered with [`dwell`](Self::dwell), and
/// - a `ParcelInfoRequest` for that id, answered with the search listing
///   [`ParcelListing::details`] derives from the parcel's own record.
#[derive(Debug, Clone, PartialEq)]
pub struct ParcelListing {
    /// The region-local id of the parcel in [`SceneFixtures::parcels`] this
    /// describes.
    pub local_id: RegionLocalParcelId,
    /// The parcel's grid-wide id — the one a `RemoteParcelRequest` resolves a
    /// location to and a `ParcelInfoRequest` names.
    pub parcel_id: ParcelKey,
    /// The parcel's dwell: the traffic score a `ParcelDwellRequest` asks for,
    /// and the same number the search listing carries. Zero on a region
    /// nobody has visited, which is what a fresh OpenSim region reports.
    pub dwell: f32,
}

impl ParcelListing {
    /// The search listing a `ParcelInfoRequest` is answered with: this
    /// parcel's grid-wide identity and dwell, over `parcel`'s own name,
    /// description, owner, area and flags, anchored at the parcel's
    /// south-west corner in `region`.
    ///
    /// The listing's flags byte is its own packing, not the parcel's flags cut
    /// down ([`ParcelListingFlags`](sl_proto::ParcelListingFlags)): the
    /// region's rating, whether a group owns the parcel, and whether it is for
    /// sale. `adult` is how the imitated grid packs an adult rating
    /// ([`ParcelPolicy::adult_listing_bits`](crate::imitates::ParcelPolicy::adult_listing_bits)).
    ///
    /// Derived rather than stated so the two records cannot disagree about
    /// the parcel they both describe — the drift a live grid cannot have,
    /// because both come out of its one land record.
    #[must_use]
    pub fn details(
        &self,
        parcel: &ParcelInfo,
        region: &RegionIdentity,
        adult: sl_proto::AdultListingBits,
    ) -> ParcelDetails {
        let (global_x, global_y) = region.region_handle.global_coordinates();
        ParcelDetails {
            parcel_id: self.parcel_id,
            owner_id: parcel.owner.uuid(),
            name: parcel.name.clone(),
            description: parcel.description.clone(),
            actual_area: parcel.area,
            billable_area: parcel.area,
            flags: sl_proto::ParcelListingFlags::new(
                region.maturity,
                parcel.owner.is_group(),
                parcel.sale_price.is_some(),
                adult,
            ),
            global_position: GlobalCoordinates::new(
                f64::from(global_x) + f64::from(parcel.aabb_min.x()),
                f64::from(global_y) + f64::from(parcel.aabb_min.y()),
                f64::from(parcel.aabb_min.z()),
            ),
            sim_name: region.sim_name.clone(),
            snapshot_id: parcel.snapshot_id,
            dwell: self.dwell,
            sale_price: parcel.sale_price.clone(),
            // The same auction, in the signed field the listing carries it in;
            // an id past the signed range is not one a simulator mints, so
            // "no auction" is the honest answer for one.
            auction_id: i32::try_from(parcel.auction_id).unwrap_or(0),
        }
    }
}

/// The animations one animated object (animesh) is playing.
///
/// The list is the object's **complete** state, not a delta, exactly as
/// `ObjectAnimation` carries it: an animation that stops simply drops out of a
/// later update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectAnimationFixture {
    /// The animated object's full id — the animesh **root**, which is what an
    /// `ObjectAnimation` names.
    pub object: ObjectKey,
    /// The animations it plays, in the order they are listed on the wire.
    pub animations: Vec<AnimationKey>,
}

impl ObjectAnimationFixture {
    /// One object playing one animation.
    #[must_use]
    pub fn playing(object: ObjectKey, animation: AnimationKey) -> Self {
        Self {
            object,
            animations: vec![animation],
        }
    }

    /// The wire record: each animation numbered from one in list order, the
    /// way a simulator numbers a fresh set.
    #[must_use]
    pub fn wire(&self) -> Vec<ObjectPlayingAnimation> {
        self.animations
            .iter()
            .enumerate()
            .map(|(index, animation)| ObjectPlayingAnimation {
                anim_id: *animation,
                sequence_id: i32::try_from(index.saturating_add(1)).unwrap_or(i32::MAX),
            })
            .collect()
    }
}

impl Default for SceneFixtures {
    /// [`SceneFixtures::new`]: an empty world whose arriving avatar takes
    /// region-local id 1, never 0 — the id that means "no parent", which
    /// would leave what it wears and what sits on it hanging off nothing.
    fn default() -> Self {
        Self::new()
    }
}

impl SceneFixtures {
    /// No parcels, no objects, no NPCs.
    ///
    /// Not `const`: the estate record it starts with names the estate, and a
    /// name is a `String`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            parcels: Vec::new(),
            listings: Vec::new(),
            objects: ObjectStore::new(),
            npcs: Vec::new(),
            object_animations: Vec::new(),
            avatar_local_id: RegionLocalObjectId(1),
            avatar_ids: BTreeMap::new(),
            first_avatar_arrived: false,
            residents: BTreeMap::new(),
            last_minted_local_id: RegionLocalObjectId(0),
            parcel_access: BTreeMap::new(),
            estate: EstateFixture::default(),
            limits: None,
            restart_schedule: None,
            terrain_material_overrides: sl_proto::TerrainMaterialOverrides(core::array::from_fn(
                |_slot| sl_proto::Llsd::Undef,
            )),
            capacity: None,
            properties: crate::imitates::PropertiesPolicy::default(),
            rez: crate::imitates::RezPolicy::default(),
            sits: crate::imitates::SitPolicy::default(),
            edit: crate::imitates::EditPolicy::default(),
            linking: crate::imitates::LinkPolicy::default(),
            changes: crate::imitates::ChangePolicy::default(),
            contents: crate::imitates::ContentsPolicy::default(),
            relays: crate::relays::RelayPolicy::default(),
            presence: crate::imitates::PresencePolicy::default(),
            attachments: crate::attachments::AttachmentPolicy::default(),
            luggage: BTreeMap::new(),
            departed_ids: BTreeMap::new(),
            leaving: Vec::new(),
            scripts: crate::script_compile::CompilePolicy::default(),
            chat: crate::chat::ChatPolicy::default(),
            region_info: crate::imitates::RegionInfoPolicy::default(),
            terrain_composition: None,
            listens: crate::chat::Listens::default(),
            withholds_ground: false,
            render_info: crate::render_info::RegionRenderInfo::default(),
            script_instances: crate::scripts::RegionScripts::default(),
            experience_land: crate::experiences::ExperienceLand::default(),
            touches: crate::touches::Touches::default(),
            physics: crate::physics::Physics::default(),
            wall: crate::time::WallClock::default(),
        }
    }

    /// Say `line` in the region: to the scripts in earshot of it, to its
    /// listens, and — the change returned — to the region's avatars, each
    /// session's watcher deciding whether its own avatar hears it. The path
    /// every line takes, an avatar's, a dialog reply and a test's.
    pub fn say(&mut self, line: crate::chat::Line) -> RegionChange {
        self.script_instances.hear(&line, &self.objects);
        self.say_to_avatars(line)
    }

    /// Say `line` to the region's listens and avatars but not its scripts: a
    /// script's own line, which reaches scripts as far as the script says and
    /// avatars as far as the line's type carries, two reaches the region
    /// offers apart ([`crate::scripts`]).
    pub(crate) fn say_to_avatars(&mut self, line: crate::chat::Line) -> RegionChange {
        let objects = &self.objects;
        self.listens
            .hear(&line, |key| listening_position(objects, key));
        RegionChange::Chat(Box::new(line))
    }

    /// States the task inventory of the prim with the given region-local id —
    /// a fixture saying what a prim already contains. The prim goes in first:
    /// its contents are held with it.
    ///
    /// # Errors
    ///
    /// `inventory` itself, when the region has no prim with that id.
    pub fn add_task_inventory(
        &mut self,
        local_id: RegionLocalObjectId,
        inventory: TaskInventory,
    ) -> Result<(), TaskInventory> {
        self.objects
            .set_contents(local_id, inventory)
            .map(|_replaced| ())
    }

    /// Mints a region-local id no object in the region holds — the id a
    /// simulator hands a freshly rezzed prim.
    ///
    /// One above the highest id in use, counting every prim the region ever
    /// held, the NPCs, the agent's own avatar and every id previously minted,
    /// so a rez that followed a derez cannot hand back the derezzed object's
    /// id (a viewer keyed on it would silently update the wrong thing).
    pub fn mint_local_id(&mut self) -> RegionLocalObjectId {
        let highest = self
            .npcs
            .iter()
            .flat_map(|npc| {
                std::iter::once(npc.local_id.0).chain(
                    npc.attachments
                        .iter()
                        .map(|attachment| attachment.local_id.0),
                )
            })
            .chain(std::iter::once(self.objects.highest_local_id().0))
            .chain(std::iter::once(self.avatar_local_id.0))
            .chain(self.avatar_ids.values().map(|id| id.0))
            .chain(std::iter::once(self.last_minted_local_id.0))
            .max()
            .unwrap_or(0);
        self.last_minted_local_id = RegionLocalObjectId(highest.saturating_add(1));
        self.last_minted_local_id
    }

    /// Mints every later region-local id above `base`: where a grid starts
    /// one region's numbering so that it does not run into another's.
    pub(crate) const fn mint_above(&mut self, base: RegionLocalObjectId) {
        if base.0 > self.last_minted_local_id.0 {
            self.last_minted_local_id = base;
        }
    }

    /// Gives `agent`, arriving, its avatar's region-local id: the one it
    /// already has here, the region's first-avatar id
    /// ([`avatar_local_id`](Self::avatar_local_id)) for the first to come, and
    /// a freshly minted one for every other.
    pub fn arrive_avatar(&mut self, agent: AgentKey) -> RegionLocalObjectId {
        if let Some(id) = self.avatar_ids.get(&agent) {
            return *id;
        }
        let id = if self.first_avatar_arrived {
            self.mint_local_id()
        } else {
            self.first_avatar_arrived = true;
            self.avatar_local_id
        };
        let _first = self.avatar_ids.insert(agent, id);
        id
    }

    /// `agent` left the region: its avatar's id goes with it, never to be
    /// handed out again, and it is no longer shown. The id its avatar had,
    /// for the `KillObject` the region's other viewers are owed, or [`None`]
    /// when it was not being shown ([`hide_resident`](Self::hide_resident)).
    pub(crate) fn depart_avatar(&mut self, agent: AgentKey) -> Option<RegionLocalObjectId> {
        let _gone = self.avatar_ids.remove(&agent);
        self.hide_resident(agent)
    }

    /// Shows `agent`'s avatar to the region's other viewers as `resident`
    /// ([`Resident`]), replacing what was shown of it before.
    pub(crate) fn show_resident(&mut self, agent: AgentKey, resident: Resident) {
        let _before = self.residents.insert(agent, resident);
    }

    /// Stops showing `agent`'s avatar: it left the region on foot, which keeps
    /// it a child agent here with its id, or left it altogether. The id it
    /// was shown under, or [`None`] for an avatar not shown.
    pub(crate) fn hide_resident(&mut self, agent: AgentKey) -> Option<RegionLocalObjectId> {
        self.residents
            .remove(&agent)
            .map(|resident| resident.body.local_id)
    }

    /// Whether an avatar standing at `at` is hidden from a viewer whose own
    /// avatar stands at `viewer_at` — [`None`] for one standing in another
    /// region, down a child circuit: on a grid that hides avatars on a
    /// private parcel ([`PresencePolicy`](crate::imitates::PresencePolicy)),
    /// one on a parcel whose `SeeAVs` is off, from a viewer not on it.
    #[must_use]
    pub(crate) fn hides(&self, at: &Vector, viewer_at: Option<&Vector>) -> bool {
        if !self.presence.hides_on_private_parcels {
            return false;
        }
        let Some(parcel) = self.parcel_at(at.x, at.y) else {
            return false;
        };
        if parcel.see_avs != Some(false) {
            return false;
        }
        viewer_at
            .and_then(|viewer| self.parcel_at(viewer.x, viewer.y))
            .is_none_or(|theirs| theirs.local_id != parcel.local_id)
    }

    /// What the region's other viewers are shown of `agent`'s avatar.
    #[must_use]
    pub(crate) fn resident(&self, agent: AgentKey) -> Option<&Resident> {
        self.residents.get(&agent)
    }

    /// What is shown of `agent`'s avatar, to keep in step with a change.
    pub(crate) fn resident_mut(&mut self, agent: AgentKey) -> Option<&mut Resident> {
        self.residents.get_mut(&agent)
    }

    /// Every avatar shown in the region, by agent, in key order.
    pub(crate) fn residents(&self) -> impl Iterator<Item = (AgentKey, &Resident)> {
        self.residents
            .iter()
            .map(|(agent, resident)| (*agent, resident))
    }

    /// Keeps the shown body of `agent`'s avatar as `body` — sat down, stood
    /// up, put somewhere — and says what the region's other viewers are to be
    /// told: the change carrying it, or [`None`] for an avatar not shown.
    pub(crate) fn reshow_resident(
        &mut self,
        agent: AgentKey,
        body: &Object,
    ) -> Option<RegionChange> {
        let at = self.region_position_of(body);
        let resident = self.residents.get_mut(&agent)?;
        resident.body = body.clone();
        resident.at = at;
        Some(RegionChange::ResidentShown {
            agent,
            body: Box::new(body.clone()),
        })
    }

    /// Where `body`, an avatar's, stands in the region's own frame: its own
    /// position, or its seat's plus its offset from it, the seat's rotation
    /// left out as the coarse feed leaves it out.
    pub(crate) fn region_position_of(&self, body: &Object) -> Vector {
        let seat = (body.parent_id.0 != 0)
            .then(|| self.objects.get(body.parent_id))
            .flatten()
            .map(|seat| seat.motion.position.clone());
        let own = &body.motion.position;
        seat.map_or_else(
            || own.clone(),
            |seat| Vector {
                x: seat.x + own.x,
                y: seat.y + own.y,
                z: seat.z + own.z,
            },
        )
    }

    /// The region-local id of `agent`'s avatar here — what its own object
    /// update and everything it wears and sits on name — or the region's
    /// first-avatar id for an agent that has not arrived.
    #[must_use]
    pub fn avatar_id(&self, agent: AgentKey) -> RegionLocalObjectId {
        self.avatar_ids
            .get(&agent)
            .copied()
            .unwrap_or(self.avatar_local_id)
    }

    /// The object with the given region-local id: a prim, an NPC's body or
    /// an agent's avatar shown here (`Resident`).
    ///
    /// A prim is an index lookup; an NPC's body is made from its fixture, as
    /// an arrival burst makes it.
    #[must_use]
    pub fn object_by_local_id(&self, local_id: RegionLocalObjectId) -> Option<Object> {
        if let Some(prim) = self.objects.get(local_id) {
            return Some(prim.clone());
        }
        if let Some(resident) = self
            .residents
            .values()
            .find(|resident| resident.body.local_id == local_id)
        {
            return Some(resident.body.clone());
        }
        self.npcs.iter().find_map(|npc| {
            if npc.local_id == local_id {
                return Some(npc.avatar_prim());
            }
            npc.attachments
                .iter()
                .find(|attachment| attachment.local_id == local_id)
                .cloned()
        })
    }

    /// The object with the given full id, prims and NPC bodies alike.
    #[must_use]
    pub fn object_by_key(&self, full_id: ObjectKey) -> Option<Object> {
        self.object_by_local_id(self.local_id_of(full_id)?)
    }

    /// The prim with the given region-local id, to edit in place.
    ///
    /// Only [`objects`](Self::objects), for the reason
    /// [`remove_object`](Self::remove_object) is: an NPC's body and its
    /// attachments are scripted scenery, not prims a client may edit.
    pub fn object_mut(&mut self, local_id: RegionLocalObjectId) -> Option<ObjectMut<'_>> {
        self.objects.get_mut(local_id)
    }

    /// The region-local id of the object with the given full id — the lookup
    /// the `Undo` / `Redo` messages need, being the only client messages in the
    /// object family that name an object by its full id.
    #[must_use]
    pub fn local_id_of(&self, full_id: ObjectKey) -> Option<RegionLocalObjectId> {
        if let Some(prim) = self.objects.get_by_key(full_id) {
            return Some(prim.local_id);
        }
        if let Some(resident) = self
            .residents
            .values()
            .find(|resident| resident.body.full_id == full_id)
        {
            return Some(resident.body.local_id);
        }
        self.npcs.iter().find_map(|npc| {
            if ObjectKey::from(npc.identity.agent_id.uuid()) == full_id {
                return Some(npc.local_id);
            }
            npc.attachments
                .iter()
                .find(|attachment| attachment.full_id == full_id)
                .map(|attachment| attachment.local_id)
        })
    }

    /// The full properties record of the object with the given region-local id:
    /// what an `ObjectProperties` reply carries.
    ///
    /// Two stores meet here. The editable half — name, description, category,
    /// sale state, permissions, owner — is kept on the object itself
    /// ([`Object::properties`]), synthesised from the object the first time
    /// something asks, so a fixture that states no properties describes a prim
    /// nobody has named rather than one with no record at all. The task
    /// inventory's *serial* comes from the inventory, because that is where a
    /// write advances it, and a record that carried a stale one would tell a
    /// viewer its cached contents listing is still good.
    ///
    /// A third thing is the grid's rather than the object's: what a record
    /// says a prim nobody has edited may be done with, who owned it last and
    /// what it costs to own differ by the grid being imitated
    /// ([`PropertiesPolicy`](crate::imitates::PropertiesPolicy)).
    #[must_use]
    pub fn properties_of(&self, local_id: RegionLocalObjectId) -> Option<ObjectProperties> {
        // An agent's avatar is no object to select: both grids answer a
        // select of one with nothing (`object-select-scene`, 2026-10-09).
        if self
            .residents
            .values()
            .any(|resident| resident.body.local_id == local_id)
        {
            return None;
        }
        let object = self.object_by_local_id(local_id)?;
        let mut properties = object.properties.clone().unwrap_or_else(|| {
            let mut fresh = default_object_properties(&object);
            // Made when it came into the region: a fixture's when the region
            // started.
            if let Some(tick) = self.objects.rezzed_at(local_id) {
                let made = self
                    .script_instances
                    .calendar_micros(sl_lsl_runtime::vm::Tick(tick), &self.wall);
                fresh.creation_date = u64::try_from(made).unwrap_or(0);
            }
            fresh.permissions.base = self.properties.new_prim_owner_mask;
            fresh.permissions.owner = self.properties.new_prim_owner_mask;
            if self.properties.new_prim_last_owner == crate::imitates::LastOwner::Rezzer {
                fresh.last_owner_id = object.owner_id;
            }
            fresh
        });
        properties.ownership_cost = LindenAmount(self.properties.ownership_cost);
        properties.inventory_serial = self
            .objects
            .contents(local_id)
            .map_or(0, |contents| contents.serial);
        Some(properties)
    }

    /// The record of the object as a simulator **sends** it: what
    /// [`properties_of`](Self::properties_of) holds, with the two things a
    /// child prim's record takes from its linkset's root. Both live grids
    /// state the root's permission masks for every prim of a linkset, and
    /// OpenSim the root's sale type and price as well
    /// ([`PropertiesPolicy::child_sale`](crate::imitates::PropertiesPolicy::child_sale)).
    ///
    /// Kept apart from the stored record because an edit reads the one and
    /// writes it back: a child that read the root's masks would keep them
    /// after a delink.
    #[must_use]
    pub fn record_of(&self, local_id: RegionLocalObjectId) -> Option<ObjectProperties> {
        let mut record = self.properties_of(local_id)?;
        let root = self.root_of(local_id);
        if root != local_id
            && let Some(of_root) = self.properties_of(root)
        {
            record.permissions = of_root.permissions;
            if self.properties.child_sale == crate::imitates::ChildSale::Roots {
                record.sale_type = of_root.sale_type;
                record.sale_price = of_root.sale_price;
            }
        }
        Some(record)
    }

    /// The root of `local_id`'s linkset — itself when it is not a child, since
    /// a prim whose parent this region does not have is one nothing can be
    /// said about.
    #[must_use]
    pub fn root_of(&self, local_id: RegionLocalObjectId) -> RegionLocalObjectId {
        // A prim is read in place; only an NPC's body is made to be read.
        let parent = self.objects.get(local_id).map_or_else(
            || {
                self.object_by_local_id(local_id)
                    .map(|object| object.parent_id)
            },
            |prim| Some(prim.parent_id),
        );
        // An avatar is nobody's root: what it wears is a linkset of its own
        // whose parent id names the wearer.
        parent
            .filter(|parent| {
                parent.0 != 0
                    && (self.objects.contains(*parent)
                        || self
                            .object_by_local_id(*parent)
                            .is_some_and(|object| object.pcode != pcode::AVATAR))
            })
            .unwrap_or(local_id)
    }

    /// Records how the object is placed now as something an undo can return
    /// to.
    ///
    /// Called before a move, a turn or a resize rather than after it: what an
    /// undo restores is the placement the edit replaced.
    pub fn record_undo(&mut self, local_id: RegionLocalObjectId) {
        let Some(before) = self.objects.get(local_id).map(Placed::of) else {
            return;
        };
        let depth = self.edit.undo_depth;
        if let Some(history) = self.objects.history_mut(local_id) {
            history.record(before, depth);
        }
    }

    /// Steps the object one place back along its history of moves, turns and
    /// resizes (`backwards`) or forward again, returning the object as it was
    /// and as it is now — or [`None`] when that direction has run out, or the
    /// object is not here.
    pub fn step_history(
        &mut self,
        local_id: RegionLocalObjectId,
        backwards: bool,
    ) -> Option<(Object, Object)> {
        let before = self.objects.get(local_id)?.clone();
        let restored = self
            .objects
            .history_mut(local_id)?
            .step(Placed::of(&before), backwards)?;
        let mut held = self.object_mut(local_id)?;
        restored.place(&mut held);
        Some((before, held.clone()))
    }

    /// The experience the item `item` in the prim `task` was last compiled
    /// under — `Some(None)` for one under none — or `None` for an item the
    /// prim does not hold.
    #[must_use]
    pub fn script_experience(
        &self,
        task: ObjectKey,
        item: InventoryKey,
    ) -> Option<Option<ExperienceKey>> {
        let local_id = self.local_id_of(task)?;
        let contents = self.objects.contents(local_id)?;
        let _held = contents.items.iter().find(|held| held.item_id == item)?;
        Some(contents.experiences.get(&item).copied())
    }

    /// Removes the prim with the given region-local id, returning it.
    ///
    /// Only [`objects`](Self::objects): an NPC's body is not a prim anybody
    /// may derez. Its task inventory, cost and edit history go with it.
    pub fn remove_object(&mut self, local_id: RegionLocalObjectId) -> Option<Object> {
        self.objects.remove(local_id)
    }

    /// Adds one parcel with both its halves: the region-local record a
    /// `ParcelProperties` reply carries, and the [`ParcelListing`] naming its
    /// grid-wide id and dwell.
    ///
    /// The one call every fixture makes, so no parcel is ever pushed without
    /// the identity three of its request surfaces need.
    ///
    /// The record takes the estate's word on parcel environments
    /// ([`parcel_environments_allowed`](Self::parcel_environments_allowed)).
    pub fn add_parcel(&mut self, mut parcel: ParcelInfo, parcel_id: ParcelKey, dwell: f32) {
        self.listings.push(ParcelListing {
            local_id: parcel.local_id,
            parcel_id,
            dwell,
        });
        parcel.region_allow_environment_override = self.parcel_environments_allowed();
        self.parcels.push(parcel);
    }

    /// Whether the estate lets a parcel have an environment of its own: its
    /// `ALLOW_ENVIRONMENT_OVERRIDE` flag, which the parcel records carry as
    /// `region_allow_environment_override`. OpenSim serves a parcel's own
    /// environment only while it is set, and answers `is_default` otherwise
    /// (`book/src/gridspec/environment.md`).
    #[must_use]
    pub const fn parcel_environments_allowed(&self) -> bool {
        self.estate
            .flags
            .contains(sl_proto::RegionFlags::ALLOW_ENVIRONMENT_OVERRIDE)
    }

    /// Lets the region's parcels have environments of their own, as an
    /// estate that allows parcel environment overrides does.
    pub fn allow_parcel_environments(&mut self) {
        self.set_estate_flags(
            self.estate
                .flags
                .with(sl_proto::RegionFlags::ALLOW_ENVIRONMENT_OVERRIDE, true),
        );
    }

    /// Sets the estate's flags, and has every parcel record say what they
    /// now say of parcel environments.
    pub fn set_estate_flags(&mut self, flags: sl_proto::RegionFlags) {
        self.estate.flags = flags;
        let allowed = self.parcel_environments_allowed();
        for parcel in &mut self.parcels {
            parcel.region_allow_environment_override = allowed;
        }
    }

    /// The listing of the parcel with the given region-local id.
    #[must_use]
    pub fn listing_by_local_id(&self, local_id: RegionLocalParcelId) -> Option<&ParcelListing> {
        self.listings
            .iter()
            .find(|listing| listing.local_id == local_id)
    }

    /// The listing with the given grid-wide parcel id, and the parcel record
    /// it describes.
    #[must_use]
    pub fn listing_by_parcel_id(
        &self,
        parcel_id: ParcelKey,
    ) -> Option<(&ParcelListing, &ParcelInfo)> {
        let listing = self
            .listings
            .iter()
            .find(|listing| listing.parcel_id == parcel_id)?;
        let parcel = self.parcel_by_local_id(listing.local_id)?;
        Some((listing, parcel))
    }

    /// Every object the region shows: its prims, then each NPC's avatar body
    /// and the attachments it wears. This is what an object refetch answers
    /// from — an NPC is as refetchable as any prim.
    #[must_use]
    pub fn all_objects(&self) -> Vec<Object> {
        let mut objects: Vec<Object> = self.objects.iter().cloned().collect();
        for npc in &self.npcs {
            objects.extend(npc.objects());
        }
        objects
    }

    /// The first parcel whose bitmap covers the region-local point.
    #[must_use]
    pub fn parcel_at(&self, x: f32, y: f32) -> Option<&ParcelInfo> {
        self.parcels
            .iter()
            .find(|parcel| parcel.contains_point(x, y))
    }

    /// The first parcel whose bitmap covers the region-local point, to edit in
    /// place — a request's answer notes the sequence id it went out under.
    pub(crate) fn parcel_at_mut(&mut self, x: f32, y: f32) -> Option<&mut ParcelInfo> {
        self.parcels
            .iter_mut()
            .find(|parcel| parcel.contains_point(x, y))
    }

    /// The parcel with the given region-local id.
    #[must_use]
    pub fn parcel_by_local_id(&self, local_id: RegionLocalParcelId) -> Option<&ParcelInfo> {
        self.parcels
            .iter()
            .find(|parcel| parcel.local_id == local_id)
    }

    /// The parcel with the given region-local id, to edit in place — what an
    /// About Land save, a purchase and an abandonment all write through.
    pub fn parcel_mut(&mut self, local_id: RegionLocalParcelId) -> Option<&mut ParcelInfo> {
        self.parcels
            .iter_mut()
            .find(|parcel| parcel.local_id == local_id)
    }

    /// The region-local id of the parcel a region position stands on — how an
    /// object is decided to be *on* a parcel, for a return or a highlight.
    #[must_use]
    pub fn parcel_at_position(&self, position: &Vector) -> Option<RegionLocalParcelId> {
        self.parcel_at(position.x, position.y)
            .map(|parcel| parcel.local_id)
    }

    /// The parcel that does not let `agent` rez at `position`, if one covers
    /// it: one without "everyone may build" that the agent does not own.
    ///
    /// Group building is not modelled — the fake grid has no group roles to
    /// ask — and neither is an estate manager's or a god's right to build
    /// anywhere.
    #[must_use]
    pub fn refuses_rez(&self, agent: AgentKey, position: &Vector) -> Option<&ParcelInfo> {
        self.parcel_at(position.x, position.y).filter(|parcel| {
            !parcel
                .flags()
                .contains(sl_wire::ParcelFlags::CREATE_OBJECTS)
                && parcel.owner != OwnerKey::Agent(agent)
        })
    }

    /// One of a parcel's two access lists, as it stands.
    ///
    /// A parcel with no lists reads as two empty ones rather than as nothing:
    /// "nobody is banned" is the state of every parcel a fixture did not speak
    /// about, and it is the answer a viewer's About Land access tab expects.
    #[must_use]
    pub fn access_list(
        &self,
        local_id: RegionLocalParcelId,
        scope: ParcelAccessScope,
    ) -> &[ParcelAccessEntry] {
        self.parcel_access
            .get(&local_id)
            .map_or(&[], |lists| lists.list(scope))
    }

    /// One of a parcel's two access lists, to change — created empty on first
    /// use, since an unmentioned list is an empty one.
    pub fn access_list_mut(
        &mut self,
        local_id: RegionLocalParcelId,
        scope: ParcelAccessScope,
    ) -> &mut Vec<ParcelAccessEntry> {
        self.parcel_access
            .entry(local_id)
            .or_default()
            .list_mut(scope)
    }

    /// The region's configuration and limits as they stand: what an estate
    /// manager has made them, or what `identity` says when nobody has.
    #[must_use]
    pub fn limits(&self, identity: &RegionIdentity) -> sl_proto::RegionLimits {
        self.limits
            .clone()
            .unwrap_or_else(|| self.stock_limits(identity))
    }

    /// The limits nobody has edited: `identity`'s, holding what the imitated
    /// grid's region of this product holds.
    fn stock_limits(&self, identity: &RegionIdentity) -> sl_proto::RegionLimits {
        let mut limits = region_limits(identity);
        let stated = self.region_info.stated_flags(identity.flags());
        limits.region_flags = sl_proto::RegionFlags::from_legacy(stated.legacy_bits());
        limits.region_flags_extended = self.region_info.extended_flags_block.then_some(stated);
        limits.combat_settings = self.region_info.combat;
        limits.redirect_grid_x = self.region_info.no_redirect;
        limits.redirect_grid_y = self.region_info.no_redirect;
        if self.chat.states_ranges {
            limits.chat_settings = Some(sl_proto::RegionChatSettings {
                whisper_range: crate::chat::WHISPER_RANGE_M,
                normal_range: crate::chat::SAY_RANGE_M,
                shout_range: crate::chat::SHOUT_RANGE_M,
                whisper_offset: 0.0,
                normal_offset: 0.0,
                shout_offset: 0.0,
                flags: 0,
            });
        }
        if let Some(capacity) = self.capacity {
            limits.max_agents = capacity.max_agents;
            limits.legacy_max_agents = u8::try_from(capacity.max_agents).unwrap_or(u8::MAX);
            limits.hard_max_agents = capacity.hard_max_agents;
            limits.hard_max_objects = capacity.land_impact;
        }
        limits
    }

    /// The region's configuration and limits, to change — filled in from
    /// `identity` the first time somebody does.
    pub fn limits_mut(&mut self, identity: &RegionIdentity) -> &mut sl_proto::RegionLimits {
        let stock = self.stock_limits(identity);
        self.limits.get_or_insert(stock)
    }

    /// The region's terrain textures and blend heights as they stand.
    #[must_use]
    pub fn terrain_composition(&self, identity: &RegionIdentity) -> RegionTerrainComposition {
        self.terrain_composition.unwrap_or(identity.terrain)
    }

    /// The region's terrain textures and blend heights, to change — filled in
    /// from `identity` the first time somebody does.
    pub fn terrain_composition_mut(
        &mut self,
        identity: &RegionIdentity,
    ) -> &mut RegionTerrainComposition {
        self.terrain_composition.get_or_insert(identity.terrain)
    }

    /// The 4096-byte parcel overlay of a 256 m region (one ownership byte
    /// per 4 m cell, row-major from the south-west corner) as seen by
    /// `viewer`: the cell's parcel classifies as public / own / group /
    /// other / for sale, with the west and south parcel-edge bits set, the
    /// way OpenSim's `SendParcelOverlay` builds it. Cells no parcel covers
    /// are public.
    #[must_use]
    pub fn overlay_for(&self, viewer: AgentKey) -> Vec<u8> {
        let mut overlay = vec![OVERLAY_PUBLIC; CELLS_PER_EDGE.saturating_mul(CELLS_PER_EDGE)];
        for (index, byte) in overlay.iter_mut().enumerate() {
            let cell_x = index % CELLS_PER_EDGE;
            let cell_y = index / CELLS_PER_EDGE;
            let x = cell_metres(cell_x);
            let y = cell_metres(cell_y);
            let Some(parcel) = self.parcel_at(x, y) else {
                continue;
            };
            let mut value = overlay_class(parcel, viewer);
            if cell_x == 0 || !parcel.contains_point(x - CELL_METRES, y) {
                value |= OVERLAY_WEST_LINE;
            }
            if cell_y == 0 || !parcel.contains_point(x, y - CELL_METRES) {
                value |= OVERLAY_SOUTH_LINE;
            }
            *byte = value;
        }
        overlay
    }
}

/// The centre of a cell, in metres (cell indices are bounded by the overlay
/// edge, so the conversion is exact).
fn cell_metres(cell: usize) -> f32 {
    let index = u16::try_from(cell).unwrap_or(u16::MAX);
    f32::from(index)
        .mul_add(CELL_METRES, CELL_METRES / 2.0)
        .min(f32::from(u16::MAX))
}

/// A parcel's overlay ownership class relative to `viewer`.
fn overlay_class(parcel: &ParcelInfo, viewer: AgentKey) -> u8 {
    if parcel.sale_price.is_some() {
        return OVERLAY_FOR_SALE;
    }
    match parcel.owner {
        OwnerKey::Agent(agent) if agent == viewer => OVERLAY_OWNED_BY_REQUESTER,
        OwnerKey::Agent(_) => OVERLAY_OWNED_BY_OTHER,
        OwnerKey::Group(_) => OVERLAY_OWNED_BY_GROUP,
    }
}

/// What one object costs the region it stands in — a scene's answer to the two
/// questions a `LandStatRequest` asks, plus the extras the reply's
/// `DataExtended` half carries.
///
/// Every field is **stated by the fixture**, not measured: a scene that wants
/// a report row to say something says it here, and the fake grid steps no
/// physics, so a collision count can only be stated. An object stated nothing
/// for is reported from what its scripts cost the region's engine
/// ([`RegionScripts::usage_of_linkset`](crate::scripts::RegionScripts::usage_of_linkset)). That is the same bargain the
/// rest of the fixtures make (a prim's shape is stated, not tessellated), and it
/// is what lets the top-objects windows be exercised with no grid at all.
///
/// A cost of zero in a report's own unit keeps the object **out** of that
/// report, the way a simulator's own threshold does: OpenSim skips a script
/// under a millisecond that holds no URL and no memory.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ObjectCost {
    /// Script time used over the region's measurement window — the score of a
    /// top-scripts report.
    pub script_time: Duration,
    /// Potential collisions taken part in — the score of a top-colliders
    /// report.
    pub collisions: f32,
    /// The script memory the object holds, in bytes (`DataExtended`'s `Size`).
    /// OpenSim never reports this; Second Life does.
    pub script_memory_bytes: f32,
    /// How many public URLs the object's scripts hold (`DataExtended`'s
    /// `PublicURLs`).
    pub public_urls: i32,
}

impl ObjectCost {
    /// An object that spends `script_time` running scripts and nothing else.
    #[must_use]
    pub const fn scripted(script_time: Duration) -> Self {
        Self {
            script_time,
            collisions: 0.0,
            script_memory_bytes: 0.0,
            public_urls: 0,
        }
    }

    /// An object that takes part in `collisions` collisions and nothing else.
    #[must_use]
    pub const fn colliding(collisions: f32) -> Self {
        Self {
            script_time: Duration::ZERO,
            collisions,
            script_memory_bytes: 0.0,
            public_urls: 0,
        }
    }

    /// The same cost, with the script memory and URL count a `DataExtended`
    /// block carries.
    #[must_use]
    pub const fn with_resources(mut self, script_memory_bytes: f32, public_urls: i32) -> Self {
        self.script_memory_bytes = script_memory_bytes;
        self.public_urls = public_urls;
        self
    }

    /// This cost in the unit `report_type` asks for, or `None` when the object
    /// does not belong in that report at all.
    #[must_use]
    pub fn score_for(&self, report_type: LandStatReportType) -> Option<LandStatScore> {
        match report_type {
            LandStatReportType::TopScripts => {
                (!self.script_time.is_zero()).then_some(LandStatScore::ScriptTime(self.script_time))
            }
            LandStatReportType::TopColliders => {
                (self.collisions > 0.0).then_some(LandStatScore::Collisions(self.collisions))
            }
            // A report this build does not know has no unit to answer in.
            _unknown => None,
        }
    }
}

/// The identity an avatar object is rezzed with — the arriving agent's own,
/// or an [`NpcFixture`]'s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarIdentity {
    /// The agent id (the avatar object's full id).
    pub agent_id: AgentKey,
    /// The account's first name.
    pub first_name: String,
    /// The account's last name.
    pub last_name: String,
}

impl AvatarIdentity {
    /// The identity of the avatar `agent_id` called `first_name last_name`.
    #[must_use]
    pub fn new(agent_id: AgentKey, first_name: &str, last_name: &str) -> Self {
        Self {
            agent_id,
            first_name: first_name.to_owned(),
            last_name: last_name.to_owned(),
        }
    }

    /// This identity as a people-service record, for
    /// [`SimSession::set_display_name`](sl_proto::SimSession::set_display_name).
    ///
    /// Any avatar the viewer can see needs one, or `GetDisplayNames` puts its
    /// id in `bad_ids` and the name tag renders as `(???) (???)` — cached for
    /// an hour, so it does not fix itself during a session.
    ///
    /// The record is the shape a grid sends for an agent with no custom
    /// display name: `display_name` equal to the legacy name and
    /// `is_display_name_default` set. `username` is the SLID form — dotted and
    /// lowercase, with a `Resident` last name elided, as Second Life does.
    #[must_use]
    pub fn display_name_record(&self) -> sl_wire::DisplayName {
        let username = if self.last_name.eq_ignore_ascii_case("Resident") {
            self.first_name.to_lowercase()
        } else {
            format!(
                "{}.{}",
                self.first_name.to_lowercase(),
                self.last_name.to_lowercase()
            )
        };
        sl_wire::DisplayName {
            id: self.agent_id,
            username,
            display_name: format!("{} {}", self.first_name, self.last_name),
            legacy_first_name: self.first_name.clone(),
            legacy_last_name: self.last_name.clone(),
            is_display_name_default: true,
            // Far enough out that a capture run never re-fetches mid-session;
            // the viewer treats an elapsed expiry as a reason to ask again.
            display_name_expires: "2099-01-01T00:00:00Z".to_owned(),
            display_name_next_update: "2099-01-01T00:00:00Z".to_owned(),
            missing: false,
        }
    }
}

/// A region-wide public parcel: every cell of a 256 m region, owned by
/// `owner`, flying and rezzing allowed, not for sale. `local_id` is the
/// region-local id the viewer's parcel requests and voice lookups key on.
#[must_use]
pub fn region_wide_parcel(
    local_id: RegionLocalParcelId,
    owner: OwnerKey,
    name: &str,
) -> ParcelInfo {
    let flags = sl_wire::ParcelFlags::ALLOW_FLY
        .union(sl_wire::ParcelFlags::CREATE_OBJECTS)
        .union(sl_wire::ParcelFlags::ALLOW_OTHER_SCRIPTS)
        .union(sl_wire::ParcelFlags::ALLOW_VOICE)
        .union(sl_wire::ParcelFlags::ALLOW_LANDMARK);
    ParcelInfo {
        sequence_id: UNSOLICITED_SEQUENCE_ID,
        request_result: ParcelRequestResult::Single,
        snap_selection: false,
        self_count: 0,
        other_count: 0,
        public_count: 0,
        local_id,
        owner,
        group: None,
        auction_id: 0,
        claim_date: 0,
        claim_price: LindenAmount(0),
        rent_price: LindenAmount(0),
        aabb_min: RegionCoordinates::new(0.0, 0.0, 0.0),
        aabb_max: RegionCoordinates::new(256.0, 256.0, 0.0),
        area: LandArea(0x0001_0000),
        bitmap: vec![0xFF; CELLS_PER_EDGE.saturating_mul(CELLS_PER_EDGE) / 8],
        status: ParcelStatus::Leased,
        category: ParcelCategory::None,
        max_prims: 15_000,
        sim_wide_max_prims: 15_000,
        sim_wide_total_prims: 0,
        total_prims: 0,
        owner_prims: 0,
        group_prims: 0,
        other_prims: 0,
        selected_prims: 0,
        parcel_prim_bonus: 1.0,
        other_clean_time: 0,
        raw_parcel_flags: flags.bits(),
        sale_price: None,
        name: name.to_owned(),
        description: String::new(),
        music_url: None,
        media_url: None,
        media_id: None,
        media_auto_scale: false,
        auth_buyer_id: None,
        snapshot_id: None,
        pass_price: LindenAmount(0),
        pass_hours: 0.0,
        user_location: RegionCoordinates::new(128.0, 128.0, 25.0),
        user_look_at: Direction::new(1.0, 0.0, 0.0),
        landing_type: sl_proto::LandingType::Anywhere,
        region_push_override: false,
        region_deny_anonymous: false,
        region_deny_identified: false,
        region_deny_transacted: false,
        region_deny_age_unverified: false,
        region_allow_access_override: false,
        // No environment of its own: both grids report -1 for such a parcel
        // (`book/src/gridspec/environment.md`), which is what a viewer reads
        // as nothing to ask for.
        parcel_environment_version: -1,
        region_allow_environment_override: false,
        see_avs: None,
        any_av_sounds: None,
        group_av_sounds: None,
        media_data: None,
        media_sharing: None,
        obscure_moap: None,
    }
}

/// A **rectangular** parcel covering the region-local metre box
/// `west..east` × `south..north`, otherwise exactly a [`region_wide_parcel`].
///
/// What a region split into parcels is made of, and the thing a region-wide
/// parcel cannot express: an **interior** boundary. The viewer's property
/// lines are drawn from the overlay's west/south edge bits, and a single
/// region-wide parcel has none of those anywhere but the region rim — so a
/// test that wants to see a parcel line appear (and, after a join, disappear)
/// needs two of these.
///
/// The box is snapped outward to whole four-metre blocks, because the
/// overlay and the membership bitmap are both one bit per 4 m block and a
/// parcel edge that fell inside a block would be a boundary no viewer could
/// draw.
#[must_use]
pub fn rect_parcel(
    local_id: RegionLocalParcelId,
    owner: OwnerKey,
    name: &str,
    west: f32,
    south: f32,
    east: f32,
    north: f32,
) -> ParcelInfo {
    let mut parcel = region_wide_parcel(local_id, owner, name);
    let block = |metres: f32| {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a block index of a 64-block region, clamped to it"
        )]
        let index = (metres / CELL_METRES).floor().max(0.0) as usize;
        index.min(CELLS_PER_EDGE)
    };
    let (west_block, south_block) = (block(west), block(south));
    let (east_block, north_block) = (block(east), block(north));
    parcel.bitmap = vec![0; CELLS_PER_EDGE.saturating_mul(CELLS_PER_EDGE) / 8];
    let mut blocks = 0_u32;
    for block_y in south_block..north_block {
        for block_x in west_block..east_block {
            let bit = block_y
                .saturating_mul(CELLS_PER_EDGE)
                .saturating_add(block_x);
            if let Some(byte) = parcel.bitmap.get_mut(bit / 8) {
                *byte |= 1_u8 << (bit % 8);
                blocks = blocks.saturating_add(1);
            }
        }
    }
    parcel.aabb_min = RegionCoordinates::new(cell_edge(west_block), cell_edge(south_block), 0.0);
    parcel.aabb_max = RegionCoordinates::new(cell_edge(east_block), cell_edge(north_block), 0.0);
    // The claimed area is what a viewer's About Land reads, and OpenSim states
    // it in square metres of the blocks the bitmap actually holds.
    parcel.area = LandArea(blocks.saturating_mul(16));
    parcel
}

/// The west or south edge of block index `block`, in region metres.
fn cell_edge(block: usize) -> f32 {
    f32::from(u16::try_from(block).unwrap_or(u16::MAX)) * CELL_METRES
}

/// An [`ObjectMotion`] at rest at `position`, unrotated.
const fn resting_motion(position: Vector) -> ObjectMotion {
    ObjectMotion {
        position,
        velocity: ZERO,
        acceleration: ZERO,
        rotation: Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        },
        angular_velocity: ZERO,
        collision_plane: None,
    }
}

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// The identity rotation, for a fixture that turns nothing.
const UNROTATED: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// Where a fixture seat puts its sitter: this far above the seat's own
/// centre, facing the way the seat faces — what its sit target
/// ([`FIXTURE_SIT_TARGET`]) comes to, raised as both grids raise one.
///
/// A real vehicle sets a sit target, so the avatar snaps to the seat rather
/// than to wherever the pointer landed, and a fixture that answered the click
/// offset would seat two riders in the same place only by luck. It is the
/// offset the scripted riders already use
/// ([`SEATED_NPC_SIT_OFFSET_Z`](crate::fixtures::catalogue::SEATED_NPC_SIT_OFFSET_Z)).
pub const SIT_TARGET_OFFSET: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.55,
};

/// The sit target a fixture seat carries ([`fixture_seat`]): the one
/// [`SIT_TARGET_RAISE_M`](crate::imitates::SIT_TARGET_RAISE_M) below
/// [`SIT_TARGET_OFFSET`], which is what a sit on it states on OpenSim.
pub const FIXTURE_SIT_TARGET: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: SIT_TARGET_OFFSET.z - crate::imitates::SIT_TARGET_RAISE_M,
};

/// The seat settings of a fixture seat: an unturned sit target that puts its
/// sitter at [`SIT_TARGET_OFFSET`], no camera.
#[must_use]
pub fn fixture_seat() -> sl_lsl_runtime::vm::seating::Seat {
    sl_lsl_runtime::vm::seating::Seat {
        target: Some(sl_lsl_runtime::vm::seating::SitTarget {
            offset: FIXTURE_SIT_TARGET,
            rotation: UNROTATED,
        }),
        ..sl_lsl_runtime::vm::seating::Seat::default()
    }
}

/// The skeleton every fixture object shares: a root object with no sound,
/// text, media, particles, or extra params; the caller sets the identity,
/// geometry and motion.
///
/// The caller sets the **surface** too, because what an object wears depends
/// on what it is: a prim gets the default texture ([`default_texture_entry`]),
/// an avatar gets none at all (see [`avatar_prim`]).
fn bare_object(
    local_id: RegionLocalObjectId,
    full_id: ObjectKey,
    owner: AgentKey,
    position: Vector,
) -> Object {
    Object {
        region_handle: sl_wire::RegionHandle(0),
        local_id,
        circuit: sl_proto::CircuitId::default(),
        full_id,
        parent_id: RegionLocalObjectId(0),
        pcode: pcode::PRIMITIVE,
        state: 0,
        crc: 0,
        material: 3,
        click_action: 0,
        update_flags: 0,
        scale: Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        motion: resting_motion(position),
        owner_id: owner.uuid(),
        sound: uuid::Uuid::nil(),
        gain: 0.0,
        sound_flags: 0,
        sound_radius: 0.0,
        text: String::new(),
        text_color: [0; 4],
        name_value: String::new(),
        media_url: None,
        texture_entry: Vec::new(),
        texture_anim: Vec::new(),
        texture_animation: None,
        shape: PrimShapeParams::default(),
        particle_system: Vec::new(),
        particles: None,
        data: Vec::new(),
        extra_params: Vec::new(),
        extra: ObjectExtraParams::default(),
        properties: None,
        joint_type: 0,
        joint_pivot: ZERO,
        joint_axis_or_anchor: ZERO,
    }
}

/// The packed `TextureEntry` a simulator gives a prim nobody has textured:
/// the default plywood ([`sl_proto::DEFAULT_PRIM_TEXTURE`]) on every face,
/// which the stock asset store answers
/// ([`default_assets`](crate::scenario::default_assets)).
///
/// It takes no face count on purpose. The entry is run-length packed — one
/// default value, then `(face bitmask, value)` overrides — so an entry whose
/// faces all agree is *one* default and no overrides, and the bytes are the
/// same however many faces the prim's shape renders. A client that decodes
/// this blob for six faces and one that decodes it for nine both read plywood
/// everywhere, which is what "the whole prim wears the default texture" means
/// on the wire.
///
/// The alternative — leaving the blob empty — is not "no texture set" but *no
/// per-face data at all*: [`decode_texture_entry`](sl_proto::decode_texture_entry)
/// of an empty blob is an entry with no faces, so everything downstream (a
/// viewer's renderer, a take writing an object asset) has to invent what the
/// prim's faces are.
fn default_texture_entry() -> Vec<u8> {
    sl_proto::encode_texture_entry(&sl_proto::TextureEntry {
        faces: vec![sl_proto::TextureFace::new(sl_proto::TextureKey::from(
            sl_proto::DEFAULT_PRIM_TEXTURE,
        ))],
    })
}

/// A plain box prim of `scale` metres resting at `position`, owned by
/// `owner` — the simplest object a viewer can render, wearing the default
/// texture a fresh prim has ([`sl_proto::DEFAULT_PRIM_TEXTURE`]) on every
/// face.
#[must_use]
pub fn box_prim(
    local_id: RegionLocalObjectId,
    full_id: ObjectKey,
    owner: AgentKey,
    position: Vector,
    scale: Vector,
) -> Object {
    let mut object = bare_object(local_id, full_id, owner, position);
    object.texture_entry = default_texture_entry();
    object.scale = scale;
    object.shape = PrimShapeParams {
        // A straight-extruded square profile: the legacy "box".
        path_curve: 16,
        profile_curve: 1,
        path_scale_x: 100,
        path_scale_y: 100,
        ..PrimShapeParams::default()
    };
    object
}

/// The object a client's [`PrimShape`] rezzes as: the shape it built, at the
/// ids and owner the simulator chose for it.
///
/// The client sends every path/profile field already quantized (it is building
/// the same wire block a simulator would send back), so nothing here is
/// converted — the numbers move across as they are, and the object a rez
/// produces is the object the client described.
///
/// The one thing the client does *not* describe is the surface: a
/// [`PrimShape`] carries no `TextureEntry`, because a real simulator is the
/// one that decides what a new prim looks like, and it decides the default
/// plywood ([`sl_proto::DEFAULT_PRIM_TEXTURE`]) on every face the shape
/// renders. A viewer retextures it afterwards, with an `ObjectImage` naming
/// faces that have to already exist.
#[must_use]
pub fn prim_from_shape(
    local_id: RegionLocalObjectId,
    full_id: ObjectKey,
    owner: AgentKey,
    shape: &PrimShape,
) -> Object {
    let mut object = bare_object(local_id, full_id, owner, shape.position.clone());
    object.texture_entry = default_texture_entry();
    object.pcode = shape.pcode;
    object.state = shape.state;
    object.material = shape.material.to_code();
    object.update_flags = shape.add_flags;
    object.scale = shape.scale.clone();
    object.motion.rotation = shape.rotation.clone();
    object.shape = PrimShapeParams {
        path_curve: shape.path_curve,
        profile_curve: shape.profile_curve,
        path_begin: shape.path_begin,
        path_end: shape.path_end,
        path_scale_x: shape.path_scale_x,
        path_scale_y: shape.path_scale_y,
        path_shear_x: shape.path_shear_x,
        path_shear_y: shape.path_shear_y,
        path_twist: shape.path_twist,
        path_twist_begin: shape.path_twist_begin,
        path_radius_offset: shape.path_radius_offset,
        path_taper_x: shape.path_taper_x,
        path_taper_y: shape.path_taper_y,
        path_revolutions: shape.path_revolutions,
        path_skew: shape.path_skew,
        profile_begin: shape.profile_begin,
        profile_end: shape.profile_end,
        profile_hollow: shape.profile_hollow,
    };
    object
}

/// An avatar object for `identity` standing at `position`: the `LEGACY_AVATAR`
/// pcode, the agent id as the full id, and the `FirstName` / `LastName` /
/// `Title` name-values a simulator attaches so the viewer can label it.
///
/// This is the avatar body every fake-grid avatar is rezzed as — the arriving
/// agent's own (from the login identity) and every
/// [`NpcFixture`]'s.
///
/// Unlike a prim it carries **no** `TextureEntry`: an avatar's faces are its
/// bake slots, and what fills them travels in the `AvatarAppearance` a
/// simulator pushes separately
/// ([`NpcAppearance::texture_entry`]) — not in the object update. Giving one
/// the prim default here would put plywood in a bake slot.
#[must_use]
pub fn avatar_prim(
    local_id: RegionLocalObjectId,
    identity: &AvatarIdentity,
    position: Vector,
) -> Object {
    let mut object = bare_object(
        local_id,
        ObjectKey::from(identity.agent_id.uuid()),
        identity.agent_id,
        position,
    );
    object.pcode = pcode::AVATAR;
    object.scale = Vector {
        x: 0.45,
        y: 0.6,
        z: 1.9,
    };
    object.name_value = format!(
        "FirstName STRING RW SV {}\nLastName STRING RW SV {}\nTitle STRING RW SV ",
        identity.first_name, identity.last_name
    );
    object
}

/// Pushes the arrival burst at the agent, in the order a simulator sends it:
/// its own avatar object, the parcel overlay, the parcel it stands on, the
/// ground (LAND, then WIND and CLOUD), and every fixture object. Send
/// failures are logged, never fatal.
///
/// The avatar goes first because it is what teaches the client this
/// circuit's region handle — a `LayerData` message carries none, so a patch
/// that arrives before the first object update is stamped with handle zero.
///
/// The region's other avatars follow the objects: the NPCs, then the agents
/// already standing here ([`push_residents`]). Returns the arriving avatar's
/// object, which is what the region's other viewers are to be shown of it,
/// and the avatars a private parcel hides from it.
pub(crate) fn push_arrival_world(
    world: &SceneFixtures,
    ground: Ground<'_>,
    identity: &AvatarIdentity,
    assets: &crate::assets::GridAssets,
    bakes: crate::bakes::BakePolicy,
    sim: &mut SimSession,
    now: Instant,
) -> (Object, BTreeSet<AgentKey>) {
    // Rez the avatar where the session says it arrives, not at a second
    // hard-coded guess: the placement is set once per session (a teleport's
    // target, or the ground at the region centre for a fresh login) and this
    // has to agree with it, or the object update and the AgentMovementComplete
    // that follows disagree about where the avatar is.
    let placement = sim.arrival_position().position;
    let arrival = Vector {
        x: placement.x(),
        y: placement.y(),
        z: placement.z(),
    };
    // An agent that arrives riding something — a vehicle carried over a
    // border with it — is sent seated on the region's copy from the first, as
    // both grids send it: there is no moment it stands. The seat goes ahead
    // of it (it names the region handle as well as the avatar would), so the
    // client never holds a rider whose parent it has not heard of.
    let seat = sim.seated_on().and_then(|on| world.objects.get_by_key(on));
    let avatar = match seat {
        Some(seat) => {
            if let Err(error) = send_objects(sim, core::slice::from_ref(seat), now) {
                tracing::warn!("sending the arriving rider's seat failed: {error}");
            }
            // Where the region has it sitting, or where a fixture's seat puts
            // a rider it has not been told of yet.
            let (offset, rotation) = world
                .objects
                .seat_of(identity.agent_id)
                .map_or((SIT_TARGET_OFFSET, UNROTATED), |(_seat, sitter)| {
                    (sitter.offset.clone(), sitter.rotation.clone())
                });
            let mut avatar = avatar_prim(world.avatar_id(identity.agent_id), identity, offset);
            avatar.parent_id = seat.local_id;
            avatar.motion.rotation = rotation;
            avatar
        }
        None => avatar_prim(
            world.avatar_id(identity.agent_id),
            identity,
            arrival.clone(),
        ),
    };
    if let Err(error) = send_objects(sim, core::slice::from_ref(&avatar), now) {
        tracing::warn!("rezzing the arriving avatar failed: {error}");
    }
    push_own_appearance(
        identity,
        assets,
        bakes,
        sim,
        &listed_worn(world, identity.agent_id),
        now,
    );
    if let Err(error) = sim.send_parcel_overlay(&world.overlay_for(identity.agent_id), now) {
        tracing::warn!("sending the parcel overlay failed: {error}");
    }
    if let Some(parcel) = world.parcel_at(arrival.x, arrival.y) {
        let mut record = parcel.clone();
        record.sequence_id = UNSOLICITED_SEQUENCE_ID;
        sim.enqueue_parcel_properties(&record);
    }
    push_terrain(ground, !world.withholds_ground, true, sim, now);
    // Every object but what another avatar wears, which follows its wearer
    // ([`push_residents`]).
    let shown: Vec<&Object> = world
        .objects
        .iter()
        .filter(|object| !worn_by_another(world, object, identity.agent_id))
        .collect();
    if !shown.is_empty()
        && let Err(error) = send_objects(sim, shown, now)
    {
        tracing::warn!("rezzing the fixture objects failed: {error}");
    }
    push_npcs(&world.npcs, bakes, sim, now);
    let hidden = push_residents(world, (identity.agent_id, Some(&arrival)), sim, now);
    refresh_coarse_others(world, identity.agent_id, sim);
    push_object_animations(&world.object_animations, sim, now);
    (avatar, hidden)
}

/// Pushes the agents' avatars standing in the region to a viewer, in the
/// order a simulator introduces one: the bodies as one update, then each
/// one's appearance and its animations, then what they wear. `viewer` is the
/// agent the circuit is for, left out, and `viewer_at` where its avatar
/// stands in this region ([`None`] down a child circuit).
///
/// Both live grids send an arriving agent every avatar in the region, however
/// far off (`avatar-presence`, 2026-10-08), and a child circuit the
/// neighbour's — but for one a private parcel hides from it
/// ([`SceneFixtures::hides`]), which is returned. Send failures are logged,
/// never fatal.
pub(crate) fn push_residents(
    world: &SceneFixtures,
    (viewer, viewer_at): (AgentKey, Option<&Vector>),
    sim: &mut SimSession,
    now: Instant,
) -> BTreeSet<AgentKey> {
    let mut hidden = BTreeSet::new();
    let mut shown: Vec<(AgentKey, &Resident)> = Vec::new();
    for (agent, resident) in world.residents() {
        if agent == viewer {
            continue;
        }
        if world.hides(&resident.at, viewer_at) {
            let _new = hidden.insert(agent);
        } else {
            shown.push((agent, resident));
        }
    }
    if shown.is_empty() {
        return hidden;
    }
    let bodies: Vec<&Object> = shown
        .iter()
        .map(|(_agent, resident)| &resident.body)
        .collect();
    if let Err(error) = send_objects(sim, bodies, now) {
        tracing::warn!("rezzing the region's other avatars failed: {error}");
    }
    for (agent, resident) in &shown {
        push_resident_looks(*agent, resident, sim, now);
    }
    let worn: Vec<&Object> = shown
        .iter()
        .flat_map(|(agent, _resident)| worn_records(world, *agent, viewer))
        .collect();
    if !worn.is_empty()
        && let Err(error) = send_objects(sim, worn, now)
    {
        tracing::warn!("rezzing what the region's other avatars wear failed: {error}");
    }
    hidden
}

/// Introduces `agent`'s avatar to `viewer`'s viewer: its object, its
/// appearance and its animations, then what it wears — how an arrival, and
/// an avatar a private parcel stops hiding, is sent. Nothing for an avatar the
/// region does not show. Send failures are logged, never fatal.
pub(crate) fn push_resident(
    world: &SceneFixtures,
    agent: AgentKey,
    viewer: AgentKey,
    sim: &mut SimSession,
    now: Instant,
) {
    let Some(resident) = world.resident(agent) else {
        return;
    };
    if let Err(error) = send_objects(sim, core::slice::from_ref(&resident.body), now) {
        tracing::warn!("rezzing another avatar failed: {error}");
    }
    push_resident_looks(agent, resident, sim, now);
    let worn = worn_records(world, agent, viewer);
    if !worn.is_empty()
        && let Err(error) = send_objects(sim, worn, now)
    {
        tracing::warn!("rezzing what another avatar wears failed: {error}");
    }
}

/// The records of everything `agent` wears that `viewer` is shown
/// ([`crate::seating::shown_to`]), each linkset's root first.
fn worn_records(world: &SceneFixtures, agent: AgentKey, viewer: AgentKey) -> Vec<&Object> {
    world
        .objects
        .worn_by(agent)
        .into_iter()
        .flat_map(|root| {
            std::iter::once(root).chain(crate::link_sets::children_in_order(world, root))
        })
        .filter_map(|prim| world.objects.get(prim))
        .filter(|object| crate::seating::shown_to(world, object, viewer))
        .collect()
}

/// Whether `object` is part of something an avatar other than `viewer`
/// wears: what an arrival burst sends after its wearer's body rather than
/// with the region's objects.
fn worn_by_another(world: &SceneFixtures, object: &Object, viewer: AgentKey) -> bool {
    crate::attachments::is_leaving(world, object)
        || world
            .objects
            .attached_to(world.root_of(object.local_id))
            .is_some_and(|attached| attached.agent != viewer)
}

/// Sends what follows an avatar's object when it is introduced: its
/// appearance, then its animations. Send failures are logged, never fatal.
pub(crate) fn push_resident_looks(
    agent: AgentKey,
    resident: &Resident,
    sim: &mut SimSession,
    now: Instant,
) {
    if let Err(error) = sim.send_avatar_appearance(&resident.appearance, now) {
        tracing::warn!("sending another avatar's appearance failed: {error}");
    }
    if let Err(error) = resident.animations.send(agent, sim, now) {
        tracing::warn!("sending another avatar's animations failed: {error}");
    }
}

/// Lists the region's other avatars in `sim`'s coarse feed as they stand now:
/// the NPCs, then every agent shown here but `viewer`, whose own entry the
/// session adds itself where it is the root agent.
pub(crate) fn refresh_coarse_others(world: &SceneFixtures, viewer: AgentKey, sim: &mut SimSession) {
    sim.set_coarse_others(coarse_others(world, viewer, sim.coarse_rounding()));
}

/// The entries the region's periodic `CoarseLocationUpdate` carries for its
/// other avatars: one per NPC and then one per agent shown here but `viewer`,
/// where each stands. A seated one is placed at its seat's position plus its
/// own offset from it, the seat's rotation left out: an entry is a whole metre
/// wide.
fn coarse_others(
    world: &SceneFixtures,
    viewer: AgentKey,
    rounding: sl_proto::CoarseRounding,
) -> Vec<sl_proto::CoarseLocation> {
    let agents = world
        .residents()
        .filter(|(agent, _resident)| *agent != viewer)
        .map(|(agent, resident)| {
            sl_proto::coarse_location(
                agent,
                &sl_proto::RegionCoordinates::new(resident.at.x, resident.at.y, resident.at.z),
                rounding,
            )
        })
        .collect::<Vec<_>>();
    world
        .npcs
        .iter()
        .map(|npc| {
            let seat = npc.seat.and_then(|seat| {
                world
                    .objects
                    .get(seat)
                    .map(|object| object.motion.position.clone())
            });
            let (x, y, z) = seat.map_or((0.0, 0.0, 0.0), |seat| (seat.x, seat.y, seat.z));
            sl_proto::coarse_location(
                npc.agent_id(),
                &sl_proto::RegionCoordinates::new(
                    npc.position.x + x,
                    npc.position.y + y,
                    npc.position.z + z,
                ),
                rounding,
            )
        })
        .chain(agents)
        .collect()
}

/// Pushes what a **child** circuit is shown: the region's objects, its other
/// avatars, its ground and its parcel overlay — everything the arrival burst
/// carries except the agent, because the agent is standing in another region.
///
/// A simulator streams its scene to a child agent exactly as it does to a root
/// one; the only difference is that there is no avatar of its own to rez, no
/// appearance or animation to send for it, and no "the parcel you are standing
/// on" record, because it is not standing on one. Objects go first (an
/// `ObjectUpdate` carries the region handle, a `LayerData` does not), then the
/// ground, so a client that somehow missed the `EnableSimulator` still labels
/// the patches with the right region.
///
/// It ends with a [marker](crate::marker) named after the region, which is how
/// a test waits for a neighbour's scene to have arrived without sleeping.
/// Send failures are logged, never fatal.
///
/// `viewer` is the agent the circuit is for, which stands elsewhere; the
/// avatars a private parcel hides from it are returned.
pub(crate) fn push_child_world(
    world: &SceneFixtures,
    ground: Ground<'_>,
    (identity, viewer): (&sl_proto::RegionIdentity, AgentKey),
    bakes: crate::bakes::BakePolicy,
    sim: &mut SimSession,
    now: Instant,
) -> BTreeSet<AgentKey> {
    // What the neighbour's residents wear follows them ([`push_residents`]).
    let unworn: Vec<&Object> = world
        .objects
        .iter()
        .filter(|object| !worn_by_another(world, object, viewer))
        .collect();
    if !unworn.is_empty()
        && let Err(error) = send_objects(sim, unworn, now)
    {
        tracing::warn!("rezzing a neighbour's objects failed: {error}");
    }
    push_npcs(&world.npcs, bakes, sim, now);
    // The neighbour's own residents, and its coarse feed listing them: a
    // child circuit's agent stands elsewhere, so it is shown everybody a
    // private parcel does not hide.
    let hidden = push_residents(world, (viewer, None), sim, now);
    refresh_coarse_others(world, viewer, sim);
    sim.start_child_coarse_feed(now);
    push_object_animations(&world.object_animations, sim, now);
    push_terrain(ground, !world.withholds_ground, false, sim, now);
    // The overlay is the whole region's parcel layout, which a neighbouring
    // region draws on the minimap; it needs no viewer to be standing on it.
    // `overlay_for` colours the cells by owner, and a child agent owns nothing
    // here, so the nil key asks for the "somebody else's land" colouring.
    let nobody = AgentKey::from(uuid::Uuid::nil());
    if let Err(error) = sim.send_parcel_overlay(&world.overlay_for(nobody), now) {
        tracing::warn!("sending a neighbour's parcel overlay failed: {error}");
    }
    let name = identity
        .sim_name
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    if let Err(error) = sim.send_generic_message(&crate::marker::neighbour_marker(&name), now) {
        tracing::warn!("marking a neighbour's burst failed: {error}");
    }
    hidden
}

/// Pushes each animated object's `ObjectAnimation` — the animesh counterpart
/// of an NPC's `AvatarAnimation`, and the message that turns a rigged prim
/// carrying the animated-object flag into one that actually moves. It goes
/// after the objects because it names an object by full id the client has to
/// already know. Send failures are logged, never fatal.
fn push_object_animations(
    animations: &[ObjectAnimationFixture],
    sim: &mut SimSession,
    now: Instant,
) {
    for animated in animations {
        let playing = animated.wire();
        if playing.is_empty() {
            continue;
        }
        if let Err(error) = sim.send_object_animation(animated.object, &playing, now) {
            tracing::warn!("sending an animated object's animations failed: {error}");
        }
    }
}

/// The colour the arriving agent's own avatar is baked. Green, so a fixture
/// session tells its own body from the catalogue NPC's blue one at a glance —
/// and from the region's red-and-green checker, which no avatar wears.
pub const OWN_AVATAR_BAKE_COLOR: [u8; 4] = sl_test_assets::markers::GREEN;

/// How far an avatar's **centre** sits above the ground it stands on: half the
/// 1.9 m default avatar height.
///
/// An avatar object's position is its centre, not its feet, so a fixture that
/// wants one standing on the terrain adds this to the terrain height. The
/// catalogue NPC's `NPC_Z` is this same offset applied to the stock ground.
pub const AVATAR_CENTRE_ABOVE_GROUND_M: f32 = 0.95;

/// Pushes the arriving agent its **own** `AvatarAppearance`, registering the
/// bakes it names.
///
/// OpenSim sends an agent its own appearance like anyone else's; Second Life
/// does so for a viewer whose `RegionHandshakeReply` said it understands one
/// ([`BakePolicy::sends_own_appearance`](crate::bakes::BakePolicy::sends_own_appearance)).
/// A viewer that never receives one has no visual params and no texture entry
/// for itself: it spawns the avatar, poses its skeleton, and draws no body at
/// all — a name tag hanging in mid-air. The fake grid runs no bake service, so
/// it bakes the arriving agent exactly the way it bakes an NPC: one solid per
/// body region under ids derived from the agent's own id, whose bytes go into
/// *this session's* asset store, because the agent id is only known now.
pub(crate) fn push_own_appearance(
    identity: &AvatarIdentity,
    assets: &crate::assets::GridAssets,
    bakes: crate::bakes::BakePolicy,
    sim: &mut SimSession,
    worn: &[sl_proto::AvatarAttachment],
    now: Instant,
) {
    if !bakes.sends_own_appearance(sim.handshake_reply_flags()) {
        return;
    }
    let _sent = push_own_appearance_baked(identity, assets, bakes, sim, (None, worn), now);
}

/// What an agent's own `AvatarAppearance` lists as worn: everything it
/// wears in `world` on a grid whose appearance lists it, nothing on one whose
/// does not ([`AttachmentPolicy::appearance_lists_worn`](crate::attachments::AttachmentPolicy::appearance_lists_worn)).
pub(crate) fn listed_worn(
    world: &SceneFixtures,
    agent: AgentKey,
) -> Vec<sl_proto::AvatarAttachment> {
    if world.attachments.appearance_lists_worn {
        crate::attachments::worn_list(world, agent)
    } else {
        Vec::new()
    }
}

/// Pushes the agent its own `AvatarAppearance`
/// ([`own_appearance`]) and returns it: what the region's other viewers are
/// sent of it too.
pub(crate) fn push_own_appearance_baked(
    identity: &AvatarIdentity,
    assets: &crate::assets::GridAssets,
    bakes: crate::bakes::BakePolicy,
    sim: &mut SimSession,
    (cof_version, worn): (Option<i32>, &[sl_proto::AvatarAttachment]),
    now: Instant,
) -> sl_proto::AvatarAppearance {
    let record = own_appearance(identity, assets, bakes, sim, (cof_version, worn));
    if let Err(error) = sim.send_avatar_appearance(&record, now) {
        tracing::warn!("sending the agent's own appearance failed: {error}");
    }
    record
}

/// The agent's `AvatarAppearance`, registering the bakes it names, stamped
/// with `cof_version` — the version just baked — or, for `None`, the Current
/// Outfit Folder's version (both grids send the agent the version its outfit
/// is at). The record carries an `AppearanceHover` block, as both grids' do.
///
/// Both live grids send the region's other viewers the record they send the
/// agent: the same bakes and the same block (`appearance-baking`,
/// 2026-10-19, `book/src/gridspec/appearance.md` § Baking).
pub(crate) fn own_appearance(
    identity: &AvatarIdentity,
    assets: &crate::assets::GridAssets,
    bakes: crate::bakes::BakePolicy,
    sim: &SimSession,
    (cof_version, worn): (Option<i32>, &[sl_proto::AvatarAttachment]),
) -> sl_proto::AvatarAppearance {
    let appearance = NpcAppearance::solid(identity.agent_id, OWN_AVATAR_BAKE_COLOR);
    {
        let mut store = assets.write();
        for (key, bytes) in appearance.bake_assets() {
            let _previous = store.insert(key, bytes);
        }
    }
    // Both flavours publish the same ids for the same body; only the
    // `AppearanceData` block around them tells the viewer whether to fetch them
    // from the appearance service or as ordinary assets (`crate::bakes`).
    let mut record = appearance.record(identity.agent_id, Vec::new());
    // A grid that baked the outfit shows the bake: its ids on the baked faces
    // and nothing on the others, and the parameters it was baked under.
    if let Some(outfit) = sim.baked_outfit() {
        record.texture_entry = sl_proto::TextureEntry {
            faces: outfit
                .textures
                .iter()
                .map(|id| sl_proto::TextureFace::new(sl_proto::TextureKey::from(*id)))
                .collect(),
        };
        record.visual_params.clone_from(&outfit.visual_params);
    }
    record.cof_version = cof_version
        .or_else(|| sim.agent_inventory().current_outfit_version())
        .or(record.cof_version);
    record.hover_height = Some(Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });
    record.attachments = worn.to_vec();
    bakes.applied(record)
}

/// Pushes the arriving agent the animations it is playing: the arrival's
/// set, which is the built-in **stand** alone. The driver sends it right after
/// [`push_arrival_world`], as both grids state an arrival's animations just
/// after its avatar and appearance.
///
/// A simulator tells every avatar, its own included, what it is playing, and a
/// real one always has an answer — OpenSim's `ScenePresence` puts an arriving
/// agent into `STAND` before it has moved a metre. An avatar the grid signals
/// *nothing* for is one no motion drives, and the reference viewer then draws
/// it in the raw rest pose its skeleton was authored in: folded forwards, arms
/// against the chest, staring at the ground. That is what a fake-grid arrival
/// looked like once the bakes stopped being thrown away and the body became
/// visible at all.
///
/// The asset needs no serving. `stand` is a Linden built-in every viewer ships
/// (this workspace under `viewer-assets/static_assets`, the reference under
/// `app_settings/static_assets`), so naming it costs the grid nothing.
pub(crate) fn push_own_animation(
    identity: &AvatarIdentity,
    animations: &crate::animations::AnimationSet,
    policy: crate::animations::AnimationPolicy,
    sim: &mut SimSession,
    now: Instant,
) {
    if let Err(error) = animations
        .own(policy, identity.agent_id)
        .send(identity.agent_id, sim, now)
    {
        tracing::warn!("sending the arriving agent's own animation failed: {error}");
    }
}

/// Pushes the region's other avatars, in the order a simulator introduces
/// one: every NPC's avatar object, then each one's appearance and playing
/// animations, then their attachments.
///
/// The bodies go first as one update because the appearance and the animation
/// name an avatar the client has to already know, and the attachments last
/// because each names its wearer's region-local id as its parent. Send
/// failures are logged, never fatal.
fn push_npcs(
    npcs: &[NpcFixture],
    bakes: crate::bakes::BakePolicy,
    sim: &mut SimSession,
    now: Instant,
) {
    if npcs.is_empty() {
        return;
    }
    let bodies: Vec<Object> = npcs.iter().map(NpcFixture::avatar_prim).collect();
    if let Err(error) = send_objects(sim, &bodies, now) {
        tracing::warn!("rezzing the NPC avatars failed: {error}");
    }
    for npc in npcs {
        let record = bakes.applied(npc.appearance_record());
        if let Err(error) = sim.send_avatar_appearance(&record, now) {
            tracing::warn!("sending an NPC's appearance failed: {error}");
        }
    }
    for npc in npcs {
        let animations = npc.playing_animations();
        if !animations.is_empty()
            && let Err(error) = sim.send_avatar_animation(npc.agent_id(), &animations, now)
        {
            tracing::warn!("sending an NPC's animations failed: {error}");
        }
    }
    let attachments: Vec<Object> = npcs
        .iter()
        .flat_map(|npc| npc.attachments.iter().cloned())
        .collect();
    if !attachments.is_empty()
        && let Err(error) = send_objects(sim, &attachments, now)
    {
        tracing::warn!("rezzing the NPC attachments failed: {error}");
    }
}

/// Receives what crossed a border into this region: `objects` and the avatars
/// in `npcs` join the region's world and are streamed to the client, objects
/// first because a seated avatar names its seat as its parent.
///
/// An object is in one region at a time, so a destination learns of a vehicle
/// (and anyone riding it) only when it arrives, and under its own region-local
/// ids; the region left behind kills its copy.
pub(crate) fn receive_crossing(
    world: &mut SceneFixtures,
    objects: Vec<Object>,
    npcs: Vec<NpcFixture>,
    bakes: crate::bakes::BakePolicy,
    sim: &mut SimSession,
    now: Instant,
) {
    if !objects.is_empty()
        && let Err(error) = send_objects(sim, &objects, now)
    {
        tracing::warn!("streaming what crossed into the region failed: {error}");
    }
    push_npcs(&npcs, bakes, sim, now);
    world.objects.extend(objects);
    world.npcs.extend(npcs);
    let viewer = sim
        .agent_id()
        .unwrap_or_else(|| AgentKey::from(uuid::Uuid::nil()));
    refresh_coarse_others(world, viewer, sim);
}

/// A region's ground and how the grid being imitated sends it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ground<'a> {
    /// The ground itself.
    pub(crate) fixture: &'a TerrainFixture,
    /// How it is sent ([`crate::ImitatedGrid::terrain_policy`]).
    pub(crate) policy: &'a crate::imitates::TerrainPolicy,
}

/// Streams the region's ground as the grid being imitated does: the LAND
/// layer outwards from where the session's agent arrives (unless `land` is
/// false, for a region told to withhold it), the WIND layer
/// behind it where the grid sends a root agent its wind with the ground
/// (`root`), and the CLOUD layer if the fixture carries one. The wind that
/// follows on a timer is the session's own
/// ([`SimSession::set_wind_feed`]). Send failures are logged.
fn push_terrain(ground: Ground<'_>, land: bool, root: bool, sim: &mut SimSession, now: Instant) {
    let Ground {
        fixture: terrain,
        policy,
    } = ground;
    let handle = sim.region_handle();
    let placement = sim.arrival_position().position;
    let stream = policy.land_stream(placement.x(), placement.y());
    if land && let Err(error) = sim.send_terrain(&terrain.to_patches(handle), &stream, now) {
        tracing::warn!("streaming the region's ground failed: {error}");
    }
    let wind = terrain.wind_patches(handle);
    if root
        && policy.wind_start == crate::imitates::WindStart::WithTheGround
        && !wind.is_empty()
        && let Err(error) = sim.send_layer_data(
            TerrainLayerType::Wind,
            &wind,
            &policy.wind_encoding,
            policy.wind_reliability,
            now,
        )
    {
        tracing::warn!("sending the wind layer failed: {error}");
    }
    let clouds = terrain.cloud_patches(handle);
    if !clouds.is_empty()
        && let Err(error) = sim.send_layer_data(
            TerrainLayerType::Cloud,
            &clouds,
            &sl_proto::LayerEncoding::REFERENCE,
            sl_proto::Reliability::Reliable,
            now,
        )
    {
        tracing::warn!("sending the cloud layer failed: {error}");
    }
}
/// The stores an answer reads and writes: the region's world, the grid's
/// assets, and this session's selection and machine.
pub(crate) struct WorldStores<'a> {
    /// The region's world — its objects, parcels and terrain.
    pub(crate) world: &'a mut SceneFixtures,
    /// The grid's assets, which an object asset is served from.
    pub(crate) assets: &'a crate::assets::GridAssets,
    /// This session's current selection.
    pub(crate) selection: &'a mut crate::object_edits::Selection,
    /// The session's own machine, which every answer is sent on.
    pub(crate) sim: &'a mut SimSession,
}

/// The identities and policies that decide what an answer says: who is asking,
/// where, where the simulator's own ids come from, and which live grid this one
/// imitates for an object asset and its announcement.
pub(crate) struct WorldPolicies<'a> {
    /// The agent whose session this is.
    pub(crate) identity: &'a AvatarIdentity,
    /// The region it is in.
    pub(crate) region: &'a RegionIdentity,
    /// Which live grid's object-asset behaviour to imitate.
    pub(crate) object_assets: crate::assets::ObjectAssetPolicy,
    /// How a new inventory item is announced to the client.
    pub(crate) announcement: crate::inventory::InventoryAnnouncement,
    /// How the About Land traffic is answered.
    pub(crate) parcels: crate::imitates::ParcelPolicy,
    /// How a sit request is answered.
    pub(crate) sits: crate::imitates::SitPolicy,
    /// Where the simulator's own ids come from.
    pub(crate) mint: &'a dyn Fn() -> uuid::Uuid,
    /// What the agent may do with its groups' objects.
    pub(crate) groups: crate::object_edits::GroupRights<'a>,
}

/// Answers one drained [`ServerEvent`] against the region's world, under the
/// session lock.
///
/// The **reads**: a parcel request (by rectangle or id) gets the covering
/// parcel's record with the request's sequence id echoed — or a
/// [`ParcelRequestResult::NoData`] reply on a miss, as a simulator says
/// "no such parcel" — a dwell or search-listing request gets the parcel's
/// [`ParcelListing`] half, an object refetch gets a full `ObjectUpdate` of
/// the matching fixtures (an unknown id is silently dropped, as a simulator
/// drops a stale refetch), and a task-inventory request gets the object's
/// contents listing.
///
/// The **writes**: a rez mints an object into the region, a derez files it
/// away and takes it out again, and a task-inventory update copies an agent
/// inventory item into a prim. Each returns what it changed as a
/// [`RegionChange`] so the region's other sessions can be told; the answering
/// session is told directly, in the same breath as the mutation, because a
/// client that rezzed something has to learn the ids it did not choose before
/// it can do anything else with it.
///
/// `mint` supplies the ids the simulator chooses (object keys, inventory item
/// ids): the grid's own [`IdMinter`](crate::runtime::IdMinter) reaches through
/// here, so a seeded grid rezzes the same object twice.
///
/// `object_assets` is which live grid this one imitates where they disagree
/// about a taken object's asset
/// ([`ObjectAssetPolicy`](crate::assets::ObjectAssetPolicy)). Only the derez arm
/// reads it; the rez arm deliberately does not, because an item says for itself
/// where its body went. `announcement` is the same question about how a filed
/// item is handed to the client
/// ([`InventoryAnnouncement`](crate::inventory::InventoryAnnouncement)), and the
/// derez arm is likewise the only reader.
pub(crate) fn answer_world_request(
    stores: WorldStores<'_>,
    policies: WorldPolicies<'_>,
    event: &ServerEvent,
    now: Instant,
) -> Vec<RegionChange> {
    let WorldStores {
        world,
        assets,
        selection,
        sim,
    } = stores;
    let WorldPolicies {
        identity,
        region,
        object_assets,
        announcement,
        parcels,
        sits,
        mint,
        groups,
    } = policies;
    // The object and parcel edit families are bodies of work of their own; each
    // answers first and says so, so nothing here has to enumerate what they
    // cover.
    if let Some(changes) =
        crate::object_edits::answer_object_edit(world, mint, selection, sim, &groups, event, now)
    {
        return changes;
    }
    // A held button is a touch to the prim's scripts first; an update of it
    // is the drag's as well.
    let toucher = crate::touches::Toucher {
        identity,
        group: groups.groups.active.as_ref().map(|(group, ..)| *group),
    };
    if crate::touches::observe(world, &toucher, sim, event) {
        return Vec::new();
    }
    if let Some(changes) = crate::grabs::answer_grab(world, identity.agent_id, sim, event, now) {
        return changes;
    }
    if let Some(changes) =
        crate::scripts::answer_script_command(world, (identity, region.region_id), sim, event, now)
    {
        return changes;
    }
    // The estate's experience lists are the region's: a POST from one session
    // is what the region's scripts are judged by from then on.
    if let ServerEvent::RegionExperiencesSet {
        allowed,
        blocked,
        trusted,
    } = event
    {
        world
            .experience_land
            .set_lists(allowed.clone(), blocked.clone(), trusted.clone());
        return Vec::new();
    }
    if let Some(changes) = crate::task_contents::answer_contents_write(
        crate::task_contents::ContentsStores {
            world,
            assets,
            selection,
            sim,
        },
        announcement,
        mint,
        event,
        now,
    ) {
        return changes;
    }
    if let Some(changes) =
        crate::parcel_edits::answer_parcel_edit(world, (region, identity), parcels, sim, event, now)
    {
        return changes;
    }
    match event {
        // A new prim. The simulator owns both of the object's ids -- the
        // region-local handle and the full key -- so the client cannot know
        // what it rezzed until the update comes back.
        ServerEvent::RezObject { params } => {
            if world
                .refuses_rez(identity.agent_id, &params.shape.position)
                .is_some()
            {
                let alert = world.rez.no_build_add;
                let info: Vec<sl_proto::AlertInfo> = alert
                    .key
                    .map(|key| sl_proto::AlertInfo {
                        message: key.to_owned(),
                        extra_params: String::new(),
                    })
                    .into_iter()
                    .collect();
                if let Err(error) = sim.send_alert_message(alert.text, &info, &[], now) {
                    tracing::warn!("refusing a rez on no-build land failed: {error}");
                }
                return Vec::new();
            }
            let local_id = world.mint_local_id();
            let object = prim_from_shape(
                local_id,
                ObjectKey::from(mint()),
                identity.agent_id,
                &params.shape,
            );
            world.objects.push(object.clone());
            if let Err(error) = send_objects(sim, std::slice::from_ref(&object), now) {
                tracing::warn!("streaming a rezzed object back failed: {error}");
            }
            return vec![RegionChange::Rezzed(Box::new(object))];
        }
        // An inventory item dragged back into the world. The other half of a
        // take: the item names an asset, the asset holds the prims, and the
        // region mints every id they come back under.
        ServerEvent::RezObjectFromInventory { params } => {
            return rez_from_inventory(
                world,
                assets,
                (identity, region),
                mint,
                sim,
                RezRequest {
                    item_id: params.item.item_id,
                    from_task_id: params.from_task_id,
                    at: Some(&params.ray_end),
                },
                now,
            );
        }
        // Restore to Last Position: the same rez, at the spot the object was
        // taken from rather than one the resident aims at. OpenSim has no
        // listener for it.
        ServerEvent::RezRestoreToWorld { item } => {
            if world.rez.restore_to_world == crate::imitates::RestoreToWorld::Ignored {
                return Vec::new();
            }
            return rez_from_inventory(
                world,
                assets,
                (identity, region),
                mint,
                sim,
                RezRequest {
                    item_id: item.item_id,
                    from_task_id: None,
                    at: None,
                },
                now,
            );
        }
        // A viewer's effects, for the other viewers: passed on as the grid's
        // flavour passes them on, or not at all.
        ServerEvent::ViewerEffect(effects) => {
            let at = crate::chat::agent_position(sim);
            return world
                .relays
                .effects_passed_on(identity.agent_id, at, effects)
                .map(|sent| RegionChange::Effects(Box::new(sent)))
                .into_iter()
                .collect();
        }
        // A viewer's sound — a gesture's. A parcel that bars avatar sounds
        // lets none out: nobody is sent it, the sender included.
        ServerEvent::TriggerSound {
            sound,
            gain,
            region_handle,
            position,
        } => {
            let standing = crate::chat::agent_position(sim);
            let barred = world
                .parcel_at(standing.x, standing.y)
                .is_some_and(|parcel| parcel.any_av_sounds == Some(false));
            if barred {
                return Vec::new();
            }
            let sound = world.relays.sound_passed_on(
                identity.agent_id,
                sl_proto::TriggeredSound {
                    sound: *sound,
                    owner: sl_types::key::OwnerKey::Agent(identity.agent_id),
                    object: ObjectKey::from(identity.agent_id.uuid()),
                    parent: None,
                    region_handle: *region_handle,
                    position: Vector {
                        x: position.x(),
                        y: position.y(),
                        z: position.z(),
                    },
                    gain: *gain,
                },
            );
            return vec![RegionChange::Sound(Box::new(sound))];
        }
        // Local chat: said where the avatar stands, under its name.
        ServerEvent::Chat {
            message,
            channel,
            chat_type,
        } => {
            let line = crate::chat::Line {
                source: sl_proto::ChatSource::Agent(identity.agent_id),
                from_name: format!("{} {}", identity.first_name, identity.last_name),
                owner_id: identity.agent_id.uuid(),
                chat_type: *chat_type,
                channel: *channel,
                position: crate::chat::agent_position(sim),
                message: message.clone(),
            };
            // As the grid's flavour says it, where it says it at all.
            return world
                .chat
                .viewer_line(line)
                .map(|line| world.say(line))
                .into_iter()
                .collect();
        }
        // A take, a save, a return, a delete to trash. What each does is the
        // destination's business, and the destination alone decides both
        // halves: whether an inventory item is minted, and whether the world
        // copy goes.
        //
        // A **linkset** is filed and removed whole. A viewer derezzes its
        // selection, and a selected linkset is named by its root alone, so the
        // children have to be gathered here or a take would leave them standing
        // in the region with a parent that no longer exists.
        //
        // Whether the filed item *names* the body it was written from is the
        // one place this grid has to pick a live grid to be
        // (`ObjectAssetPolicy`): Second Life tells a viewer nothing, OpenSim
        // tells it everything, and the rez works either way. **How** the item
        // is handed over is a second such choice (`InventoryAnnouncement`).
        ServerEvent::DerezObjects {
            local_ids,
            destination,
            transaction_id,
            ..
        } => {
            return derez(
                world,
                assets,
                (identity, region),
                (object_assets, announcement),
                mint,
                sim,
                &Derez {
                    local_ids,
                    destination,
                    transaction_id: *transaction_id,
                    off_world_at: None,
                },
                now,
            );
        }
        // A move out of the world, which only gets here on a grid that sends
        // such an object back to its owner
        // ([`OutOfWorld::Returned`](crate::imitates::OutOfWorld::Returned)):
        // the same return, told of in other words.
        ServerEvent::ObjectTransformSet {
            local_id,
            transform,
        } => {
            let Some(to) = &transform.position else {
                return Vec::new();
            };
            let root = world.root_of(*local_id);
            // The object goes where it was sent first, and the session that
            // sent it is told so in a terse update, as of any other move.
            if let Some(mut object) = world.object_mut(root) {
                object.motion.position.clone_from(to);
                let update = sl_proto::TerseUpdate {
                    local_id: root,
                    state: object.state,
                    motion: object.motion.clone(),
                };
                if let Err(error) = sim.send_terse_update(&[update], REAL_TIME_DILATION, now) {
                    tracing::warn!("sending a terse update of a moved object failed: {error}");
                }
            }
            return derez(
                world,
                assets,
                (identity, region),
                (object_assets, announcement),
                mint,
                sim,
                &Derez {
                    local_ids: &[root],
                    destination: &DeRezDestination::ReturnToOwner,
                    transaction_id: TransactionId::from(uuid::Uuid::nil()),
                    off_world_at: Some(to),
                },
                now,
            );
        }
        // The contents listing of a prim. An object with nothing in it is
        // answered with serial zero and an *empty* filename -- a real
        // simulator writes no Xfer file for an empty inventory, and a viewer
        // reads the empty name as "nothing to download".
        ServerEvent::RequestTaskInventory { local_id } => {
            let Some(object) = world.object_by_local_id(*local_id) else {
                tracing::debug!(
                    "a task inventory was requested for {local_id:?}, which is not here"
                );
                return Vec::new();
            };
            let contents = world.objects.contents(*local_id);
            let result = match contents {
                Some(contents) if !contents.items.is_empty() => {
                    sim.serve_task_inventory(object.full_id, contents.serial, &contents.items, now)
                }
                empty => sim.send_reply_task_inventory(
                    &sl_proto::TaskInventoryReply {
                        task: object.full_id,
                        serial: empty.map_or(0, |contents| contents.serial),
                        filename: String::new(),
                    },
                    now,
                ),
            };
            if let Err(error) = result {
                tracing::warn!("serving the task inventory of {local_id:?} failed: {error}");
            }
        }
        // The agent asked to sit on something. A simulator answers a sit on an
        // object it has with an `AvatarSitResponse`. One it does not have,
        // Second Life refuses with a named alert and OpenSim does not answer
        // at all, which leaves it to the client's own sit timeout; nor does
        // Second Life answer a seat without a free target from too far off.
        //
        // Both grids seat the avatar on the request, without waiting for the
        // client's `AgentSit`: the response, then the avatar re-sent as the
        // seat's child — its `ParentID` the seat's region-local id, its
        // position the offset from it, the one case where an avatar's position
        // is not region-local ([`crate::seating`]).
        ServerEvent::SitRequested { target, offset } => {
            let seat = world
                .object_by_key(*target)
                .filter(|seat| world.objects.contains(seat.local_id));
            let Some(seat) = seat else {
                tracing::debug!("a sit was requested on {target}, which this region does not have");
                if let Some(refusal) = sits.unknown_target {
                    let named = sl_proto::AlertInfo {
                        message: refusal.name.to_owned(),
                        extra_params: String::new(),
                    };
                    if let Err(error) = sim.send_alert_message(refusal.text, &[named], &[], now) {
                        tracing::warn!("refusing a sit on an unknown object failed: {error}");
                    }
                }
                return Vec::new();
            };
            // Where the client touched the object, which the fake grid's
            // placement does not search a surface by ([`crate::imitates::PlainSeat`]).
            let _clicked = offset;
            let standing = crate::chat::agent_position(sim);
            if world.objects.seat_of(identity.agent_id).is_none()
                && !crate::seating::answers(world, &seat, &standing)
            {
                tracing::debug!("a sit on {target} from too far off goes unanswered");
                return Vec::new();
            }
            let Some(transform) = crate::seating::sit(world, identity.agent_id, &seat) else {
                return Vec::new();
            };
            if let Err(error) = sim.send_avatar_sit_response(seat.full_id, &transform, now) {
                tracing::warn!("answering a sit request failed: {error}");
            }
            sim.seat_agent(seat.full_id);
            let placed = world
                .objects
                .seat_of(identity.agent_id)
                .map(|(_seat, sitter)| (sitter.offset.clone(), sitter.rotation.clone()));
            if let Some(placed) = placed {
                return push_seated_avatar(world, identity, seat.local_id, placed, sim, now)
                    .into_iter()
                    .collect();
            }
        }
        // Standing up puts the avatar back in the region's own frame, in front
        // of where it sat by what that grid was measured to move it
        // ([`SitPolicy::stand_forward_m`](crate::imitates::SitPolicy)). A seat
        // the region no longer has leaves the avatar where the driver last
        // knew it.
        ServerEvent::StoodUp { from } => {
            let known = sim.arrival_position().clone();
            let stands_at = crate::seating::stand(world, identity.agent_id).unwrap_or_else(|| {
                tracing::debug!("the agent stood up from {from}, which the region has no sit on");
                Vector {
                    x: known.position.x(),
                    y: known.position.y(),
                    z: known.position.z(),
                }
            });
            return stand_avatar(world, identity, sim, &stands_at, now)
                .into_iter()
                .collect();
        }
        // An object of the agent's own put on its avatar from the world: the
        // same object under the same ids, re-sent as worn, as both grids do
        // (`gridspec-attachments`).
        ServerEvent::AttachObject {
            local_id,
            attachment_point,
            rotation,
            ..
        } => {
            let root = world.root_of(*local_id);
            let own = world
                .objects
                .get(root)
                .is_some_and(|object| object.owner_id == identity.agent_id.uuid());
            if !own {
                tracing::debug!(?local_id, "an attach of an object not the agent's own");
                return Vec::new();
            }
            let at = crate::seating::seated_position(world, identity.agent_id)
                .unwrap_or_else(|| crate::chat::agent_position(sim));
            let point = attachment_point.to_code();
            if crate::seating::wear(
                world,
                (identity.agent_id, &at),
                root,
                (point, Some(rotation.clone())),
                crate::scene_store::WornItem::Unfiled,
            )
            .is_none()
            {
                return Vec::new();
            }
            // Filed as a new item, announced over UDP, which its
            // `AttachItemID` names (`attachments`, 2026-10-20).
            let Some(objects) = crate::attachments::file_worn(
                world,
                assets,
                (identity, mint),
                object_assets,
                sim,
                root,
                now,
            ) else {
                return Vec::new();
            };
            if let Err(error) = send_objects(sim, &objects, now) {
                tracing::warn!("wearing an object failed: {error}");
            }
            return vec![RegionChange::Worn {
                agent: identity.agent_id,
                objects,
            }];
        }
        // Inventory items worn: their objects rezzed onto the avatar.
        ServerEvent::RezAttachment(rez) => {
            return wear_items(
                world,
                assets,
                (identity, mint),
                sim,
                core::slice::from_ref(&**rez),
                now,
            );
        }
        ServerEvent::RezAttachments { attachments, .. } => {
            return wear_items(world, assets, (identity, mint), sim, attachments, now);
        }
        // Taken off by the item: killed, nothing restated.
        ServerEvent::DetachAttachmentIntoInventory { item_id, .. } => {
            return crate::attachments::detach_item(world, assets, identity, sim, *item_id, now);
        }
        // Worn objects put back on the ground where the avatar stands, under
        // the same ids, and their items gone from the inventory.
        ServerEvent::DropAttachments(local_ids) => {
            let mut changes = Vec::new();
            let mut removed = Vec::new();
            for local_id in local_ids {
                let root = world.root_of(*local_id);
                let Some((objects, item)) =
                    crate::seating::drop_off(world, identity.agent_id, root)
                else {
                    continue;
                };
                if let crate::scene_store::WornItem::Item(item) = item
                    && sim.agent_inventory_mut().take_item(item).is_some()
                {
                    removed.push(item);
                }
                if let Err(error) = send_objects(sim, &objects, now) {
                    tracing::warn!("dropping a worn object failed: {error}");
                }
                changes.extend(
                    objects
                        .into_iter()
                        .map(|object| RegionChange::Rezzed(Box::new(object))),
                );
            }
            if !removed.is_empty()
                && let Err(error) = sim.send_remove_inventory_item(&removed, now)
            {
                tracing::warn!("removing a dropped object's item failed: {error}");
            }
            return changes;
        }
        // Worn objects taken off into the inventory: each killed, one item
        // restated, as both grids answer an `ObjectDetach`.
        ServerEvent::DetachObjects(local_ids) => {
            let roots: Vec<RegionLocalObjectId> = local_ids
                .iter()
                .map(|local_id| world.root_of(*local_id))
                .collect();
            return crate::attachments::detach(
                world,
                assets,
                identity,
                sim,
                (&roots, crate::attachments::Restate::Last),
                now,
            );
        }
        ServerEvent::RequestParcelProperties {
            west,
            south,
            east,
            north,
            sequence_id,
            snap_selection,
        } => {
            // A one-point request (the viewer's hover / agent-parcel probe)
            // lands on the cell of its south-west corner; a rectangle is
            // answered from its centre.
            let x = west.midpoint(*east);
            let y = south.midpoint(*north);
            let record = world.parcel_at_mut(x, y).map(|parcel| {
                // The stored record's own sequence id is the id it was last
                // sent under — what OpenSim's post-edit push reuses
                // ([`EditEcho::LastSequence`](crate::EditEcho::LastSequence)).
                parcel.sequence_id = *sequence_id;
                let mut record = parcel.clone();
                record.snap_selection = *snap_selection;
                record
            });
            send_parcel_or_no_data(sim, record, *sequence_id);
        }
        ServerEvent::RequestParcelPropertiesById {
            local_id,
            sequence_id,
        } => {
            if !parcels.answers_request_by_id {
                // OpenSim has no handler for the message: "Unhandled packet
                // ParcelPropertiesRequestByID … Ignoring."
                tracing::debug!(
                    "ignoring a by-id request for parcel {local_id:?}, as OpenSim does"
                );
                return Vec::new();
            }
            let record = world.parcel_mut(*local_id).map(|parcel| {
                parcel.sequence_id = *sequence_id;
                parcel.clone()
            });
            send_parcel_or_no_data(sim, record, *sequence_id);
        }
        // The parcel's traffic score. A parcel with no grid-wide listing is
        // left unanswered rather than answered with a zero dwell: "no such
        // parcel" and "nobody has been here" are different facts, and the
        // client's own timeout tells them apart.
        ServerEvent::RequestParcelDwell { local_id } => {
            let Some(listing) = world.listing_by_local_id(*local_id) else {
                tracing::debug!("a dwell was requested for parcel {local_id:?}, which is not here");
                return Vec::new();
            };
            if let Err(error) =
                sim.send_parcel_dwell_reply(*local_id, listing.parcel_id, listing.dwell, now)
            {
                tracing::warn!("answering a parcel dwell request failed: {error}");
            }
        }
        // The search listing behind a parcel *id* — the id a viewer got from
        // the `RemoteParcelRequest` capability, a landmark or a place profile,
        // never from the region-local record.
        //
        // Answered from the live record on both flavours. That is a known,
        // deliberate difference from OpenSim, which answers from a cache that
        // lives 30 s past its last read (`m_parcelInfoCache`): a listing read
        // straight after an edit is stale there and fresh here. A test that
        // passes against `FakeOpensim` and fails against the live grid on a
        // listing's name, price or flags after an edit is seeing that cache,
        // not a client bug (`book/src/gridspec/land.md`, § Parcel info).
        ServerEvent::RequestParcelInfo { parcel_id } => {
            let Some((listing, parcel)) = world.listing_by_parcel_id(*parcel_id) else {
                tracing::debug!(
                    "a listing was requested for parcel {parcel_id}, which is not here"
                );
                return Vec::new();
            };
            let details = listing.details(parcel, region, parcels.adult_listing_bits);
            if let Err(error) = sim.send_parcel_info_reply(&details, now) {
                tracing::warn!("answering a parcel info request failed: {error}");
            }
        }
        ServerEvent::RequestObjects { objects } => {
            let wanted = |object: &Object| objects.iter().any(|(id, _)| *id == object.local_id);
            let matching: Vec<Object> = world
                .objects
                .iter()
                .filter(|object| wanted(object))
                .cloned()
                .chain(
                    world
                        .npcs
                        .iter()
                        .flat_map(crate::fixtures::NpcFixture::objects)
                        .filter(|object| wanted(object)),
                )
                .chain(
                    world
                        .residents()
                        .map(|(_agent, resident)| &resident.body)
                        .filter(|object| wanted(object))
                        .cloned(),
                )
                .collect();
            if !matching.is_empty()
                && let Err(error) = send_objects(sim, &matching, now)
            {
                tracing::warn!("answering an object refetch failed: {error}");
            }
        }
        _other => {}
    }
    Vec::new()
}

/// The agent cap a fake region reports — the number a viewer's Region/Estate
/// floater shows, and what a full Second Life region allows.
const MAX_AGENTS: u32 = 40;

/// The hard agent cap: what the estate could raise the limit to.
const HARD_MAX_AGENTS: u32 = 100;

/// The region's object budget, matching the prim allowance
/// [`region_wide_parcel`] states for its parcel — one region, one number, or a
/// viewer's land panel and its region panel disagree. A region a grid has
/// adopted reports the imitated grid's instead, and its parcels their share of
/// it ([`ImitatedGrid::region_capacity`](crate::ImitatedGrid::region_capacity)).
const HARD_MAX_OBJECTS: u32 = 15_000;

/// How far a terraform may raise or lower the ground from its baked height, in
/// metres — the estate default a region ships with.
const TERRAIN_LIMIT_M: f32 = 4.0;

/// The estate every fake region belongs to. One estate, so the region's own and
/// its parent's id are the same, the way a standalone region reports them.
const ESTATE_ID: u32 = 1;

/// The region's configuration and limits, as a `RegionInfo` reports them.
///
/// Derived from the region's own [`RegionIdentity`] wherever the two overlap —
/// name, flags, maturity, water height, billable factor — so the region a
/// handshake describes and the region the Region/Estate floater shows cannot
/// disagree. The rest are the constants above: a fake region has no estate
/// service to ask, and a number stated once here is one a test can assert.
#[must_use]
pub fn region_limits(identity: &RegionIdentity) -> sl_proto::RegionLimits {
    sl_proto::RegionLimits {
        sim_name: identity.sim_name.clone(),
        max_agents: MAX_AGENTS,
        legacy_max_agents: u8::try_from(MAX_AGENTS).unwrap_or(u8::MAX),
        product_sku: identity.product_sku.clone(),
        product_name: identity.product_name.clone(),
        hard_max_agents: HARD_MAX_AGENTS,
        hard_max_objects: HARD_MAX_OBJECTS,
        region_flags: identity.region_flags,
        region_flags_extended: identity.region_flags_extended,
        maturity: identity.maturity,
        estate_id: ESTATE_ID,
        parent_estate_id: ESTATE_ID,
        water_height: identity.water_height,
        billable_factor: identity.billable_factor,
        object_bonus_factor: 1.0,
        terrain_raise_limit: TERRAIN_LIMIT_M,
        terrain_lower_limit: -TERRAIN_LIMIT_M,
        price_per_meter: LindenAmount(1),
        redirect_grid_x: 0,
        redirect_grid_y: 0,
        // What both live grids state (`region-info`, 2026-10-17): no estate
        // sun and hour zero. The sky is the environment's either way.
        use_estate_sun: false,
        sun_hour: 0.0,
        // Absent rather than all-zero: a region that sends the block claims
        // custom chat ranges and a custom combat ruleset, and this one has
        // neither.
        chat_settings: None,
        combat_settings: None,
    }
}

/// The properties record of an object nobody has edited: what a simulator
/// holds about a freshly rezzed prim.
///
/// Named `Object` — the reference viewer's own `DEFAULT_OBJECT_NAME` — with no
/// description, owned and created by whoever owns the object, with a new
/// prim's permissions (everything to the owner, nothing to anyone else), not
/// for sale, uncategorised: the state the build floater shows for a prim that
/// was rezzed and left alone. The inventory serial is filled in
/// by [`SceneFixtures::properties_of`], which is the only place that knows it.
#[must_use]
pub fn default_object_properties(object: &Object) -> ObjectProperties {
    let owner = AgentKey::from(object.owner_id);
    ObjectProperties {
        object_id: object.full_id,
        creator_id: owner,
        owner: OwnerKey::Agent(owner),
        group: None,
        last_owner_id: uuid::Uuid::nil(),
        // In microseconds, as both live grids state an object's (and unlike an
        // inventory item's, which is in seconds).
        creation_date: u64::from(FIXTURE_CREATION_DATE.unsigned_abs())
            .saturating_mul(MICROSECONDS_PER_SECOND),
        permissions: NEW_OBJECT_PERMISSIONS,
        ownership_cost: LindenAmount(0),
        sale_type: SaleType::NotForSale.to_code(),
        sale_price: None,
        category: 0,
        inventory_serial: 0,
        item_id: InventoryKey::from(uuid::Uuid::nil()),
        folder_id: None,
        from_task_id: None,
        aggregate_perms: 0,
        aggregate_perm_textures: 0,
        aggregate_perm_textures_owner: 0,
        name: DEFAULT_OBJECT_NAME.to_owned(),
        description: String::new(),
        touch_name: String::new(),
        sit_name: String::new(),
        texture_ids: Vec::new(),
    }
}

/// The `update_flags` bits a simulator works out for each viewer from the
/// object's permissions, rather than storing on the object: what this agent
/// may do with it and whether it owns it (OpenSim's `NOT_DEFAULT_FLAGS`).
const AGENT_RELATIVE_FLAGS: u32 = prim_flags::OBJECT_MODIFY
    | prim_flags::OBJECT_COPY
    | prim_flags::OBJECT_ANY_OWNER
    | prim_flags::OBJECT_YOU_OWNER
    | prim_flags::OBJECT_MOVE
    | prim_flags::OBJECT_TRANSFER
    | prim_flags::OBJECT_OWNER_MODIFY;

/// `object` as the viewer of `agent` is sent it: its `update_flags` carrying
/// that agent's own permission bits — OpenSim's `GenerateClientFlags`, owner
/// and everyone half. The owner gets what the owner mask grants and is told it
/// owns the object; anyone else gets what the everyone mask grants. Move and
/// modify need the owner mask to allow moving at all (OpenSim's "unlocked").
///
/// Group sharing is not modelled: the fake grid has no group roles to ask, so a
/// member of the object's group is judged as everyone. An avatar is not a prim
/// and carries no such bits.
///
/// Anyone may be told it may transfer the object where its owner may, as both
/// grids tell a stranger; `owner_modifies_to_others` also tells one the owner
/// may modify it, as Second Life does (`object-edit-stranger`,
/// `book/src/gridspec/building.md` § With no right over it).
#[must_use]
pub fn as_seen_by(
    object: &Object,
    agent: Option<AgentKey>,
    owner_modifies_to_others: bool,
) -> Object {
    let mut seen = object.clone();
    if object.pcode == pcode::AVATAR {
        return seen;
    }
    let (owner, permissions) = object.properties.as_ref().map_or_else(
        || (object.owner_id, NEW_OBJECT_PERMISSIONS),
        |properties| (properties.owner.uuid(), properties.permissions),
    );
    let owns = agent.is_some_and(|agent| agent.uuid() == owner && !owner.is_nil());
    let granted = if owns {
        permissions.owner
    } else {
        permissions.everyone
    };
    let unlocked = permissions.owner.contains(sl_wire::Permissions::MOVE);
    let mut flags = object.update_flags & !AGENT_RELATIVE_FLAGS;
    if permissions.owner.contains(sl_wire::Permissions::TRANSFER) {
        flags |= prim_flags::OBJECT_TRANSFER;
    }
    for (permission, flag, needs_unlocked) in [
        (sl_wire::Permissions::COPY, prim_flags::OBJECT_COPY, false),
        (sl_wire::Permissions::MOVE, prim_flags::OBJECT_MOVE, true),
        (
            sl_wire::Permissions::MODIFY,
            prim_flags::OBJECT_MODIFY,
            true,
        ),
        (
            sl_wire::Permissions::TRANSFER,
            prim_flags::OBJECT_TRANSFER,
            false,
        ),
    ] {
        if granted.contains(permission) && (unlocked || !needs_unlocked) {
            flags |= flag;
        }
    }
    if !owner.is_nil() {
        flags |= prim_flags::OBJECT_ANY_OWNER;
    }
    if owns {
        flags |= prim_flags::OBJECT_YOU_OWNER;
        if flags & prim_flags::OBJECT_MODIFY != 0 {
            flags |= prim_flags::OBJECT_OWNER_MODIFY;
        }
    } else if owner_modifies_to_others
        && unlocked
        && permissions.owner.contains(sl_wire::Permissions::MODIFY)
    {
        flags |= prim_flags::OBJECT_OWNER_MODIFY;
    }
    seen.update_flags = flags;
    seen
}

/// Sends a full `ObjectUpdate` of `objects` to the session's viewer, each one
/// [as that viewer's agent sees it](as_seen_by) — the only way an object
/// update leaves the grid, so no viewer is ever told another's permissions.
///
/// # Errors
///
/// What [`SimSession::send_object_update`] fails with.
pub fn send_objects<'object>(
    sim: &mut SimSession,
    objects: impl IntoIterator<Item = &'object Object>,
    now: Instant,
) -> Result<(), sl_proto::Error> {
    let agent = sim.agent_id();
    let seen: Vec<Object> = objects
        .into_iter()
        .map(|object| as_seen_by(object, agent, sim.owner_modify_to_others()))
        .collect();
    sim.send_object_update(&seen, REAL_TIME_DILATION, now)
}

/// What a rez out of the inventory asks for.
#[derive(Debug, Clone, Copy)]
struct RezRequest<'a> {
    /// The item to rez.
    item_id: InventoryKey,
    /// The prim whose contents the item is said to be in, when it is not the
    /// agent's own inventory.
    from_task_id: Option<ObjectKey>,
    /// Where the root is to stand, or [`None`] for where the object was taken
    /// from — a Restore to Last Position.
    at: Option<&'a Vector>,
}

/// Rezzes an inventory item back into the world: the other half of a take.
///
/// The item is resolved **by id alone**, out of the agent's own inventory. The
/// permission masks and the CRC the client sends in `RezObjectParams` are what
/// the *viewer* believes about the item and are not checked against it, which
/// is what OpenSim does (`object-rez-derez` records that a rez with a zero CRC
/// and invented masks is honoured there); a fake grid that validated them would
/// fail a viewer for something no real grid fails it for.
///
/// A **linkset** comes back whole, and so does the prim: what a rez puts back
/// is the linkset the take removed, kept beside the published body because the
/// body's format cannot carry a light, a flexi path, a mesh, a glowing face,
/// floating text, media, a texture animation or a particle system (see
/// [`store_taken_asset`] and the [`crate::assets`] module docs). An item this
/// grid did not take — a fixture's seeded object item — has no linkset behind
/// it and is read out of its body instead, which states which prim is the root
/// and carries each child's offset from it. Either way the region mints an id
/// per prim and parents the children to the root's region-local id — the same
/// `ParentID` a link would have set.
///
/// Where it lands is the ray's end point: the fake grid casts no rays, and
/// every viewer sets `bypass_raycast` on a drag-out anyway, so `ray_end` is
/// where the resident aimed. Only the root is moved there; a child's position
/// is its offset from the root and moving it would take the linkset apart.
///
/// The item survives unless it is **no-copy**, which is the rule OpenSim
/// applies (`DoPostRezWhenFromItem`): the client's `remove_item` flag is not
/// consulted, because the flag says what the viewer expected rather than what
/// the item permits. Everything the fake grid mints is full-permission, so this
/// bites only for an item a fixture deliberately seeded without copy.
///
/// A rez out of a **prim's** contents (`from_task_id`) is not served: the item
/// is looked for in the agent's own inventory and a rez naming a task is left
/// unanswered, which is what a simulator does with a rez of an item it cannot
/// resolve.
fn rez_from_inventory(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (identity, region): (&AvatarIdentity, &RegionIdentity),
    mint: &dyn Fn() -> uuid::Uuid,
    sim: &mut SimSession,
    request: RezRequest<'_>,
    now: Instant,
) -> Vec<RegionChange> {
    let item_id = request.item_id;
    if let Some(task) = request.from_task_id {
        tracing::debug!(
            "a rez named item {item_id} in the contents of {task}; \
             the fake grid rezzes from the agent's own inventory only"
        );
        return Vec::new();
    }
    let Some(item) = sim.agent_inventory().item(item_id).cloned() else {
        tracing::debug!("a rez named item {item_id}, which this agent does not hold");
        return Vec::new();
    };
    // Land that does not let the agent build keeps the object out and the
    // item where it was. Second Life says so; OpenSim says nothing.
    if let Some(at) = request.at
        && let Some(parcel) = world.refuses_rez(identity.agent_id, at)
    {
        let region_name = region
            .sim_name
            .as_ref()
            .map_or_else(String::new, ToString::to_string);
        if let Some(text) = world
            .rez
            .no_build_rez_alert(&item.name, at, &parcel.name, &region_name)
            && let Err(error) = sim.send_alert_message(&text, &[], &[], now)
        {
            tracing::warn!("refusing a rez on no-build land failed: {error}");
        }
        return Vec::new();
    }
    let Some(rezzed) = rez_item(
        world,
        assets,
        (sim.region_handle(), mint),
        &item,
        request.at,
        now,
    ) else {
        return Vec::new();
    };
    if let Err(error) = send_objects(sim, &rezzed, now) {
        tracing::warn!("streaming an object rezzed from inventory failed: {error}");
    }
    if !item.permissions.owner.contains(sl_wire::Permissions::COPY) {
        let _consumed = sim.agent_inventory_mut().take_item(item_id);
        if request.at.is_none() {
            // A restore says so over the event queue, as Second Life does: the
            // only grid that restores at all (`book/src/gridspec/inventory.md`
            // § Giving).
            sim.enqueue_remove_inventory_objects(&[], &[item_id]);
        } else if let Err(error) = sim.send_remove_inventory_item(&[item_id], now) {
            tracing::warn!("retiring a no-copy item a rez consumed failed: {error}");
        }
    }
    rezzed
        .into_iter()
        .map(|object| RegionChange::Rezzed(Box::new(object)))
        .collect()
}

/// Wears `rezzes` from the agent's inventory where it stands
/// ([`crate::attachments::wear_items`]), and on a grid whose own appearance
/// lists what is worn, states it again after a wear of one item.
fn wear_items(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (identity, mint): (&AvatarIdentity, &dyn Fn() -> uuid::Uuid),
    sim: &mut SimSession,
    rezzes: &[sl_proto::RezAttachment],
    now: Instant,
) -> Vec<RegionChange> {
    let wearer = crate::attachments::Wearer {
        identity,
        at: crate::seating::seated_position(world, identity.agent_id)
            .unwrap_or_else(|| crate::chat::agent_position(sim)),
        region_handle: sim.region_handle(),
    };
    let (changes, _wore) =
        crate::attachments::wear_items(world, assets, (&wearer, mint), sim, rezzes, now);
    changes
}

/// Puts the linkset the object item `item` stands for into the region under
/// ids it mints, its root at `at` ([`None`]: where it was taken from), with
/// what each prim held and its scripts back: the records, the root first, or
/// [`None`] for an item whose object nothing holds.
///
/// A linkset this grid took is put back as it was taken. Anything else — a
/// fixture's seeded object item, which no take ever made — is read out of the
/// published body, which is all there is for it. [`store_taken_asset`] and the
/// [`crate::assets`] module docs say why the two sources exist.
pub(crate) fn rez_item(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (region_handle, mint): (sl_wire::RegionHandle, &dyn Fn() -> uuid::Uuid),
    item: &InventoryItem,
    at: Option<&Vector>,
    now: Instant,
) -> Option<Vec<Object>> {
    let taken = assets
        .taken_linkset(item.item_id)
        .filter(|linkset| !linkset.prims.is_empty());
    let rezzed = match &taken {
        Some(linkset) => {
            rez_taken_linkset(world, region_handle, mint, &linkset.objects(), item, at)
        }
        None => {
            let Some(bytes) = taken_object_body(assets, item) else {
                tracing::debug!("item {} names an object body nothing holds", item.item_id);
                return None;
            };
            rez_asset_body(world, region_handle, mint, &bytes, item, at)?
        }
    };
    world.objects.extend(rezzed.iter().cloned());
    if let Some(taken) = taken {
        restore_contents(world, &rezzed, taken, (0, ""), now);
    }
    Some(rezzed)
}

/// Puts back into each prim of a linkset just rezzed (`rezzed`, in the order
/// of `taken`'s prims) what it held when it was taken: its contents, under its
/// new key, and its scripts, which the region's engine brings back from what
/// they saved — the time away counted as the engine counts it — and tells of
/// the rez with `on_rez(start_parameter)`, giving them `start_string`: 0 and
/// nothing for an object rezzed from an avatar's inventory, what the script
/// asked for one a script rezzed.
fn restore_contents(
    world: &mut SceneFixtures,
    rezzed: &[Object],
    taken: crate::assets::TakenLinkset,
    (start_parameter, start_string): (i32, &str),
    now: Instant,
) {
    let away = now.saturating_duration_since(taken.taken_at);
    for (object, prim) in rezzed.iter().zip(taken.prims) {
        let Some(mut contents) = prim.contents else {
            continue;
        };
        for item in &mut contents.items {
            item.parent_task = object.full_id;
        }
        let scripts: Vec<InventoryKey> = contents
            .items
            .iter()
            .filter(|item| item.inv_type == InventoryType::Script)
            .map(|item| item.item_id)
            .collect();
        let mut saved = prim.scripts;
        for item in scripts {
            world.script_instances.arrive(
                object.full_id,
                item,
                saved.remove(&item),
                away,
                (start_parameter, start_string),
            );
        }
        if world
            .objects
            .set_contents(object.local_id, contents)
            .is_err()
        {
            tracing::error!(
                local_id = object.local_id.0,
                "a rezzed prim is not in the region"
            );
        }
    }
}

/// Where and how a script puts an object out of its prim's contents.
pub(crate) struct ScriptRez<'a> {
    /// Where the root goes.
    pub(crate) position: Vector,
    /// How the root is turned.
    pub(crate) rotation: Rotation,
    /// What the object's scripts are given for `on_rez` and
    /// `llGetStartParameter`.
    pub(crate) start_parameter: i32,
    /// What they are given for `llGetStartString`.
    pub(crate) start_string: &'a str,
    /// Whether the object is temporary.
    pub(crate) temporary: bool,
}

/// A script's rez of the object item `item` of a prim's contents
/// (`llRezObject` and its family): the linkset the item was taken as, put
/// back with its contents and its scripts as they were saved, or for an item
/// no take made (a fixture's) its published body; the root placed and turned
/// as `how` says. The prims rezzed, root first, already in the region; their
/// scripts arrive at the next reconcile. [`None`] when the item's object is
/// nowhere to be had.
pub(crate) fn script_rez(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    region_handle: sl_wire::RegionHandle,
    mint: &dyn Fn() -> uuid::Uuid,
    item: &TaskInventoryItem,
    how: &ScriptRez<'_>,
    now: Instant,
) -> Option<Vec<Object>> {
    let filed = crate::task_contents::agent_item_from(
        item,
        InventoryFolderKey::from(uuid::Uuid::nil()),
        mint,
    );
    let taken = assets
        .taken_linkset(item.item_id)
        .filter(|linkset| !linkset.prims.is_empty());
    let mut rezzed = match &taken {
        Some(linkset) => rez_taken_linkset(
            world,
            region_handle,
            mint,
            &linkset.objects(),
            &filed,
            Some(&how.position),
        ),
        None => {
            let bytes = taken_object_body(assets, &filed)?;
            rez_asset_body(
                world,
                region_handle,
                mint,
                &bytes,
                &filed,
                Some(&how.position),
            )?
        }
    };
    let root = rezzed.first_mut()?;
    root.motion.rotation = how.rotation.clone();
    if how.temporary {
        root.update_flags |= prim_flags::TEMPORARY_ON_REZ;
    }
    world.objects.extend(rezzed.iter().cloned());
    if let Some(taken) = taken {
        restore_contents(
            world,
            &rezzed,
            taken,
            (how.start_parameter, how.start_string),
            now,
        );
    }
    Some(rezzed)
}

/// The objects a rez puts back when this grid is the one that **took** them:
/// the linkset it removed, under the ids this region mints for it.
///
/// Everything the object carried comes back, which is the point — a light, a
/// flexi path, a mesh, a glowing face, floating text, media, a texture
/// animation, a particle system. None of that is in the published body, and
/// both live grids rez from something that holds it
/// ([`crate::assets`]).
///
/// Three things are *not* carried over, because they name the object where it
/// was taken from rather than the one being made: its region-local id, its
/// object key and its parent — all minted here, exactly as
/// [`sl_object_asset::RezTarget`] does for the body path. The permission flags
/// **are** kept: they say what this resident may do with the object, which a
/// take does not change.
fn rez_taken_linkset(
    world: &mut SceneFixtures,
    region_handle: sl_wire::RegionHandle,
    mint: &dyn Fn() -> uuid::Uuid,
    linkset: &[Object],
    item: &InventoryItem,
    at: Option<&Vector>,
) -> Vec<Object> {
    let Some((root_object, children)) = linkset.split_first() else {
        return Vec::new();
    };
    let root_local_id = world.mint_local_id();
    let mut rezzed = Vec::with_capacity(linkset.len());
    let mut root = root_object.clone();
    root.region_handle = region_handle;
    root.local_id = root_local_id;
    root.parent_id = RegionLocalObjectId(0);
    root.full_id = ObjectKey::from(mint());
    // A restore names no spot: the root stands where it was taken from.
    if let Some(at) = at {
        root.motion.position = at.clone();
    }
    root.properties = Some(rezzed_properties(
        &filed_prim(root_object, item, true),
        root.full_id,
        item,
        world.rez.rezzed_record_names_folder,
    ));
    rezzed.push(root);
    for child_object in children {
        let mut child = child_object.clone();
        child.region_handle = region_handle;
        child.local_id = world.mint_local_id();
        child.parent_id = root_local_id;
        child.full_id = ObjectKey::from(mint());
        child.properties = Some(rezzed_properties(
            &filed_prim(child_object, item, false),
            child.full_id,
            item,
            world.rez.rezzed_record_names_folder,
        ));
        rezzed.push(child);
    }
    rezzed
}

/// The objects a rez puts back out of a published **body** — the path for an
/// item this grid did not take, which is a fixture's seeded object item.
///
/// The class is two formats on the two grids and this grid writes both, so the
/// body says which it is rather than the policy: a `<SceneObjectGroup>` opens
/// with `<` and the Linden text with `{'task_id':`, and nothing else can look
/// like either. Reading it off the bytes is what lets a grid rez a body written
/// under the *other* flavour — an OAR-shaped fixture on a Second Life-flavoured
/// grid, or the reverse — instead of failing on it.
///
/// [`None`] when the bytes do not decode or hold no prim; the caller has
/// already logged which item they belong to.
fn rez_asset_body(
    world: &mut SceneFixtures,
    region_handle: sl_wire::RegionHandle,
    mint: &dyn Fn() -> uuid::Uuid,
    bytes: &[u8],
    item: &InventoryItem,
    at: Option<&Vector>,
) -> Option<Vec<Object>> {
    let mut rezzed = if is_scene_object_xml(bytes) {
        rez_scene_object_xml(world, region_handle, mint, bytes, item)?
    } else {
        rez_object_text(world, region_handle, mint, bytes, item)?
    };
    // Only the root moves to where the resident aimed; a child's position is
    // its offset from the root, and moving it would take the linkset apart.
    if let Some(at) = at {
        rezzed.first_mut()?.motion.position = at.clone();
    }
    Some(rezzed)
}

/// Whether a stored object body is OpenSim's XML rather than the Linden text.
///
/// The first non-whitespace byte decides, which is enough: a
/// `<SceneObjectGroup>` opens with `<` and a text asset with the `{` of its
/// first prim header.
fn is_scene_object_xml(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b'<')
}

/// The objects a rez puts back out of a **Linden text** body.
fn rez_object_text(
    world: &mut SceneFixtures,
    region_handle: sl_wire::RegionHandle,
    mint: &dyn Fn() -> uuid::Uuid,
    bytes: &[u8],
    item: &InventoryItem,
) -> Option<Vec<Object>> {
    let asset = match sl_object_asset::ObjectAsset::decode(bytes) {
        Ok(asset) => asset,
        Err(error) => {
            tracing::warn!(
                "the object body item {} names does not decode: {error}",
                item.item_id
            );
            return None;
        }
    };
    let Some(root_prim) = asset.root() else {
        tracing::warn!(
            "the object body item {} names has no root prim",
            item.item_id
        );
        return None;
    };
    let root_local_id = world.mint_local_id();
    let mut root = root_prim.to_object(sl_object_asset::RezTarget {
        region_handle,
        local_id: root_local_id,
        full_id: mint(),
        parent_id: RegionLocalObjectId(0),
    });
    root.properties = Some(rezzed_properties(
        root_prim,
        root.full_id,
        item,
        world.rez.rezzed_record_names_folder,
    ));
    let mut rezzed = vec![root];
    for child_prim in asset.children() {
        let mut child = child_prim.to_object(sl_object_asset::RezTarget {
            region_handle,
            local_id: world.mint_local_id(),
            full_id: mint(),
            parent_id: root_local_id,
        });
        child.properties = Some(rezzed_properties(
            child_prim,
            child.full_id,
            item,
            world.rez.rezzed_record_names_folder,
        ));
        rezzed.push(child);
    }
    Some(rezzed)
}

/// The objects a rez puts back out of an OpenSim `<SceneObjectGroup>` body.
///
/// The group says which part is the root by *nesting* rather than by a marker,
/// so unlike the text there is nothing to look for: `RootPart` is the root.
fn rez_scene_object_xml(
    world: &mut SceneFixtures,
    region_handle: sl_wire::RegionHandle,
    mint: &dyn Fn() -> uuid::Uuid,
    bytes: &[u8],
    item: &InventoryItem,
) -> Option<Vec<Object>> {
    let group = match sl_object_asset::opensim::SceneObjectGroup::decode(bytes) {
        Ok(group) => group,
        Err(error) => {
            tracing::warn!(
                "the scene object body item {} names does not decode: {error}",
                item.item_id
            );
            return None;
        }
    };
    let root_local_id = world.mint_local_id();
    let mut root = group.root.to_object(sl_object_asset::RezTarget {
        region_handle,
        local_id: root_local_id,
        full_id: mint(),
        parent_id: RegionLocalObjectId(0),
    });
    root.properties = Some(stamp_rezzed(
        group.root.to_properties(root.full_id),
        item,
        world.rez.rezzed_record_names_folder,
    ));
    let mut rezzed = vec![root];
    for child_part in &group.other_parts {
        let mut child = child_part.to_object(sl_object_asset::RezTarget {
            region_handle,
            local_id: world.mint_local_id(),
            full_id: mint(),
            parent_id: root_local_id,
        });
        child.properties = Some(stamp_rezzed(
            child_part.to_properties(child.full_id),
            item,
            world.rez.rezzed_record_names_folder,
        ));
        rezzed.push(child);
    }
    Some(rezzed)
}

/// The object body `item` stands for, from whichever of the grid's two stores
/// holds it.
///
/// An item that **names** an asset is resolved through it, as every other class
/// is; an item with a nil asset id is resolved by its own id out of the withheld
/// store ([`crate::assets::ObjectAssetPolicy::Withheld`]). The item is asked
/// first and the store second, rather than the other way round, because the
/// nil id is the *observable* — a viewer that was told nothing about where this
/// object lives is exactly the case the withheld store exists for, and the
/// simulator resolving it anyway is what makes a Second Life rez work.
fn taken_object_body(assets: &crate::assets::GridAssets, item: &InventoryItem) -> Option<Vec<u8>> {
    if item.asset_id.is_nil() {
        return assets.withheld_object(item.item_id);
    }
    assets
        .read()
        .get(AssetKey::from(item.asset_id))
        .map(<[u8]>::to_vec)
}

/// The properties record of one just-rezzed prim: what the asset says about it,
/// with the three things the *region* knows and the asset cannot.
///
/// The owner is the item's holder rather than whoever owned the prim where it
/// was taken from, and the item and folder are what this object was rezzed
/// from — the pair a viewer's "find in inventory" follows back.
fn rezzed_properties(
    prim: &sl_object_asset::PrimBlock,
    object_id: ObjectKey,
    item: &InventoryItem,
    names_folder: bool,
) -> ObjectProperties {
    stamp_rezzed(prim.to_properties(object_id), item, names_folder)
}

/// The things the *region* knows about a just-rezzed prim and the body
/// cannot: who holds it now and held it last, and which item it came out of.
///
/// Both live grids name the item and name the rezzing agent as the last
/// owner; only Second Life names the item's folder too
/// ([`RezPolicy::rezzed_record_names_folder`](crate::imitates::RezPolicy::rezzed_record_names_folder)).
///
/// Both body formats go through this, so a prim rezzed from OpenSim's XML and
/// one rezzed from the Linden text cannot end up disagreeing about whose object
/// it is or where a viewer's "find in inventory" should lead.
fn stamp_rezzed(
    mut properties: ObjectProperties,
    item: &InventoryItem,
    names_folder: bool,
) -> ObjectProperties {
    properties.owner = item.owner;
    properties.last_owner_id = item.owner.uuid();
    properties.item_id = item.item_id;
    properties.folder_id = names_folder.then_some(item.folder_id);
    properties
}

/// The agent inventory folder of the well-known type `folder_type`, if the
/// session's inventory has one.
pub(crate) fn folder_of_type(
    sim: &SimSession,
    folder_type: FolderType,
) -> Option<InventoryFolderKey> {
    let code = folder_type.to_code();
    sim.agent_inventory()
        .folders()
        .find(|folder| folder.folder_type == code)
        .map(|folder| folder.folder_id)
}

/// One derez: what is to leave the region and where it is to go.
struct Derez<'a> {
    /// The objects named.
    local_ids: &'a [RegionLocalObjectId],
    /// Where they are to go.
    destination: &'a DeRezDestination,
    /// The client's transaction.
    transaction_id: TransactionId,
    /// Where the object was moved to, when what sends it back to its owner is
    /// a move out of the world rather than a `DeRezObject`.
    off_world_at: Option<&'a Vector>,
}

/// A take, a save, a return, a delete to trash — and a return the region
/// makes by itself of an object that left the world. Returns what the
/// region's other sessions are to be told.
#[expect(
    clippy::too_many_arguments,
    reason = "the region, the grid's asset store, who is asking and where, the two policies a \
              filed item is shaped by, the id mint, the session, the request and the time"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one linkset after another through the same steps: gather, file, remove, announce"
)]
fn derez(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (identity, region): (&AvatarIdentity, &RegionIdentity),
    (object_assets, announcement): (
        crate::assets::ObjectAssetPolicy,
        crate::inventory::InventoryAnnouncement,
    ),
    mint: &dyn Fn() -> uuid::Uuid,
    sim: &mut SimSession,
    request: &Derez<'_>,
    now: Instant,
) -> Vec<RegionChange> {
    let policy = world.rez;
    let destination = request.destination;
    let returned = matches!(
        destination,
        DeRezDestination::ReturnToOwner | DeRezDestination::ReturnToLastOwner
    );
    let mut created = Vec::new();
    let mut changes = Vec::new();
    // What the kill names: every prim that went, or its linkset's root
    // alone, as the grid being imitated does.
    let mut killed = Vec::new();
    let mut notices = Vec::new();
    let mut handled: BTreeSet<RegionLocalObjectId> = BTreeSet::new();
    for local_id in request.local_ids {
        if !handled.insert(*local_id) {
            // Already taken as another named object's child: a viewer
            // that sent a whole linkset's ids must not file it twice.
            continue;
        }
        let Some(object) = world.object_by_local_id(*local_id) else {
            // The client believes in an object this region does not
            // have. A real simulator kills it on the client rather
            // than ignoring the request, so the two agree again — but
            // it is *this* client that is out of step, and telling the
            // region's other viewers to forget an object they may
            // legitimately hold would spread the disagreement rather
            // than settle it.
            killed.push(*local_id);
            continue;
        };
        let mut linkset = vec![object];
        linkset.extend(
            crate::link_sets::children_in_order(world, *local_id)
                .into_iter()
                .filter_map(|child| world.object_by_local_id(child)),
        );
        handled.extend(linkset.iter().map(|member| member.local_id));
        // Each prim with its record as the region holds it, so that
        // the item and the body state the name, the masks and the
        // creation date of the object they were made from — also of
        // a prim nobody has edited, which stores none.
        for member in &mut linkset {
            if let Some(record) = world.properties_of(member.local_id) {
                member.properties = Some(record);
            }
        }
        // Where the item goes. A delete names its folder and OpenSim
        // files it in the agent's own Trash whatever that was; a return
        // names none, and the owner's Lost And Found is where both
        // grids put it. Only an agent's own object is returned to an
        // inventory this session holds.
        let own = linkset
            .first()
            .is_some_and(|root| root.owner_id == identity.agent_id.uuid());
        let group_owned = world
            .properties_of(*local_id)
            .is_some_and(|properties| matches!(properties.owner, OwnerKey::Group(_)));
        let folder = match destination {
            DeRezDestination::Trash(_named)
                if group_owned
                    && policy.group_object_trash
                        == crate::imitates::GroupObjectTrash::LostAndFound =>
            {
                folder_of_type(sim, FolderType::LostAndFound)
            }
            DeRezDestination::Trash(named) => Some(match policy.trash_folder {
                crate::imitates::TrashFolder::Named => *named,
                crate::imitates::TrashFolder::AgentsOwn => {
                    folder_of_type(sim, FolderType::Trash).unwrap_or(*named)
                }
            }),
            _returned if returned => own
                .then(|| folder_of_type(sim, FolderType::LostAndFound))
                .flatten(),
            other => other.agent_folder(),
        };
        if let Some(folder) = folder {
            // What each prim contains, gathered while the region still
            // has it: an object update carries no contents, so a body
            // that stated none would lose a scripted prim's scripts.
            let contents: Vec<TaskInventory> = linkset
                .iter()
                .map(|member| {
                    world
                        .objects
                        .contents(member.local_id)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect();
            // And each prim's scripts as they stand, which a rez brings
            // back: taken out of the region with the object, or saved and
            // left running when the take is of a copy.
            let leaving = destination.removes_from_world();
            let scripts: Vec<BTreeMap<InventoryKey, sl_lsl_runtime::vm::SavedScript>> = linkset
                .iter()
                .map(|member| world.script_instances.save_prim(member.full_id, leaving))
                .collect();
            let item = taken_item(
                &linkset,
                folder,
                identity.agent_id,
                mint,
                (
                    object_assets,
                    policy.taken_item_masks,
                    policy.undescribed_take,
                ),
            );
            store_taken_asset(assets, &item, &linkset, (&contents, scripts), now);
            if returned && let Some(root) = linkset.first() {
                notices.push(match request.off_world_at {
                    Some(at) => off_world_notice(world, region, at, &item.name),
                    None => return_notice(world, region, root, &item.name),
                });
            }
            created.push(item);
        }
        if destination.removes_from_world() {
            // Whoever sits on it is stood up first, where it stood.
            changes.extend(crate::seating::stand_all_on(world, *local_id));
            for (index, member) in linkset.iter().enumerate() {
                let _removed = world.remove_object(member.local_id);
                if index == 0 || policy.derez_kill == crate::imitates::DerezKill::EveryPrimOnce {
                    killed.push(member.local_id);
                }
                changes.push(RegionChange::Killed(member.local_id));
            }
        }
    }
    // A return is never acknowledged, whether or not it filed anything
    // this session can see.
    if created.is_empty() && returned {
    } else if created.is_empty() {
        // Nothing was filed, so there is no created-item announcement
        // to correlate the client's transaction with -- which is
        // exactly what a `DeRezAck` is for.
        if let Err(error) = sim.send_derez_ack(request.transaction_id, true, now) {
            tracing::warn!("acknowledging a derez failed: {error}");
        }
    } else {
        // Never under the derez's own transaction: Second Life states
        // one of its own, OpenSim none.
        let stated = if policy.announcement_states_a_transaction {
            mint()
        } else {
            uuid::Uuid::nil()
        };
        announce_created_items(
            announcement,
            sim,
            &created,
            TransactionId::from(stated),
            now,
        );
    }
    for item in created {
        sim.agent_inventory_mut().insert_item(item);
    }
    let kill_messages = match policy.derez_kill {
        crate::imitates::DerezKill::EveryPrimOnce => 1,
        crate::imitates::DerezKill::RootTwice => 2,
    };
    if !killed.is_empty() {
        for _message in 0..kill_messages {
            // A message's block count is one byte: a linkset of 256 prims
            // and a root beside it are more than one `KillObject` holds.
            for named in killed.chunks(MOST_KILLS_A_MESSAGE) {
                if let Err(error) = sim.send_kill_object(named, now) {
                    tracing::warn!("killing a derezzed object failed: {error}");
                }
            }
        }
    }
    for text in notices {
        let from = policy.return_notice;
        let notice = sl_proto::InstantMessage {
            // Second Life signs a return by the agent with the id
            // of the agent it tells, and one of an object that went
            // off-world with none.
            from_agent_id: if from.states_an_id() && request.off_world_at.is_none() {
                identity.agent_id
            } else {
                AgentKey::from(uuid::Uuid::nil())
            },
            from_agent_name: from.sender().to_owned(),
            to_agent_id: identity.agent_id,
            dialog: sl_proto::ImDialog::FromTask,
            from_group: false,
            region_id: None,
            position: sl_types::map::RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id: uuid::Uuid::nil(),
            parent_estate_id: ESTATE_ID,
            message: text,
            binary_bucket: Vec::new(),
        };
        if let Err(error) = sim.send_instant_message(&notice, now) {
            tracing::warn!("telling of a returned object failed: {error}");
        }
    }
    changes
}

/// Files a copy of the linkset whose root is `root` as an item of `owner`'s in
/// `folder`, backed as a take of a copy backs it — every prim with its record,
/// its contents and its scripts' state, which stay running in the region — and
/// returns the item, the object's own masks on it. A purchase of a copy is
/// what files one ([`crate::money`]); `None` when the region has no such
/// object.
pub(crate) fn file_linkset_copy(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    root: RegionLocalObjectId,
    (folder, owner): (InventoryFolderKey, AgentKey),
    object_assets: crate::assets::ObjectAssetPolicy,
    mint: &dyn Fn() -> uuid::Uuid,
    now: Instant,
) -> Option<InventoryItem> {
    let mut linkset = vec![world.object_by_local_id(root)?];
    linkset.extend(
        crate::link_sets::children_in_order(world, root)
            .into_iter()
            .filter_map(|child| world.object_by_local_id(child)),
    );
    for member in &mut linkset {
        if let Some(record) = world.properties_of(member.local_id) {
            member.properties = Some(record);
        }
    }
    let contents: Vec<TaskInventory> = linkset
        .iter()
        .map(|member| {
            world
                .objects
                .contents(member.local_id)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let scripts: Vec<BTreeMap<InventoryKey, sl_lsl_runtime::vm::SavedScript>> = linkset
        .iter()
        .map(|member| world.script_instances.save_prim(member.full_id, false))
        .collect();
    let item = taken_item(
        &linkset,
        folder,
        owner,
        mint,
        (
            object_assets,
            world.rez.taken_item_masks,
            world.rez.undescribed_take,
        ),
    );
    store_taken_asset(assets, &item, &linkset, (&contents, scripts), now);
    Some(item)
}

/// What an agent is told when its object went out of the world at `at`, filed
/// under the name `name`.
fn off_world_notice(
    world: &SceneFixtures,
    region: &RegionIdentity,
    at: &Vector,
    name: &str,
) -> String {
    let parcel = world
        .parcel_at(at.x, at.y)
        .map_or("", |parcel| parcel.name.as_str());
    let region_name = region
        .sim_name
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    crate::imitates::EditPolicy::off_world_notice(name, parcel, &region_name, at)
}

/// What an agent is told when it returns its own object `root`, filed under
/// the name `name`
/// ([`RezPolicy::return_notice`](crate::imitates::RezPolicy::return_notice)).
fn return_notice(
    world: &SceneFixtures,
    region: &RegionIdentity,
    root: &Object,
    name: &str,
) -> String {
    let position = &root.motion.position;
    let parcel = world
        .parcel_at(position.x, position.y)
        .map_or("", |parcel| parcel.name.as_str());
    let region_name = region
        .sim_name
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    world
        .rez
        .return_notice_text(name, parcel, &region_name, position)
}

/// The object asset a **take** authors, filed where the item says it lives.
///
/// A take is not a round trip: nothing uploaded these bytes, the grid *writes*
/// them from the live object, which is why the asset carries the item's name
/// and description rather than the object's own (an object rezzed by
/// `ObjectAdd` has no `ObjectProperties` yet, and the item is what a viewer
/// shows). Until this existed the take minted an id and left it unbacked, so
/// every path that opens what a take filed away — rezzing it again, reading it
/// back to check the take — had nothing to fetch.
///
/// **Which store is the item's own business.** An item that names an asset gets
/// its body in the served store under that id, as OpenSim does; an item with a
/// nil asset id gets it in the withheld store under the item's id, as Second
/// Life leaves a viewer with — see
/// [`ObjectAssetPolicy`](crate::assets::ObjectAssetPolicy), which is what
/// [`taken_item`] read to decide. Reading the rule off the item rather than off
/// the policy a second time is what keeps the two halves from ever disagreeing
/// about where a body went.
///
/// `linkset` is the object being taken and, after it, its children — the whole
/// set in one asset, root **first** here and written children-first by
/// [`ObjectAsset::linkset`](sl_object_asset::ObjectAsset::linkset), which is the
/// order the reference writes them in. An empty slice writes nothing.
///
/// The asset lock is taken while the caller still holds the region's, which is
/// the order [`push_arrival_world`] already establishes (it writes an arriving
/// agent's bakes under the same two locks). Nothing anywhere takes the region
/// lock while holding the asset one, so the pair cannot invert.
pub(crate) fn store_taken_asset(
    assets: &crate::assets::GridAssets,
    item: &InventoryItem,
    linkset: &[Object],
    (contents, scripts): (
        &[TaskInventory],
        Vec<BTreeMap<InventoryKey, sl_lsl_runtime::vm::SavedScript>>,
    ),
    now: Instant,
) {
    let Some((root_object, children)) = linkset.split_first() else {
        return;
    };
    // The objects themselves, beside the body: the *text* cannot carry a light,
    // a flexi path, a mesh, a glowing face, floating text, media, a texture
    // animation or a particle system, and a rez out of it alone would hand back
    // a plain box. Both live grids rez from something that *can* say all of it
    // -- OpenSim's XML body, Second Life's simulator-side object -- so this one
    // keeps the linkset too. See `crate::assets`.
    // With each prim's contents and its scripts' saved state, which neither
    // body carries in a form a rez could run.
    let mut scripts = scripts.into_iter();
    let prims = linkset
        .iter()
        .enumerate()
        .map(|(index, object)| crate::assets::TakenPrim {
            object: object.clone(),
            contents: contents
                .get(index)
                .filter(|held| **held != TaskInventory::default())
                .cloned(),
            scripts: scripts.next().unwrap_or_default(),
        })
        .collect();
    assets.insert_taken_linkset(
        item.item_id,
        crate::assets::TakenLinkset {
            prims,
            taken_at: now,
        },
    );
    if item.asset_id.is_nil() {
        // Second Life's side: the Linden text, filed where no capability
        // reaches it.
        let root = filed_prim(root_object, item, true);
        let prims = children
            .iter()
            .map(|child| filed_prim(child, item, false))
            .collect();
        let body = sl_object_asset::ObjectAsset::linkset(root, prims).encode();
        assets.insert_withheld_object(item.item_id, body);
        return;
    }
    // OpenSim's side, and the one configuration where these bytes cross the
    // wire -- so they are the bytes OpenSim would have written.
    let body = filed_group(linkset, contents, item).encode();
    let _previous = assets.write().insert(AssetKey::from(item.asset_id), body);
}

/// The `<SceneObjectGroup>` a take **publishes** under
/// [`ObjectAssetPolicy::Served`](crate::assets::ObjectAssetPolicy::Served) —
/// what OpenSim writes as the `AssetType.Object` body
/// (`SceneObjectSerializer.ToOriginalXmlFormat`, chosen in
/// `InventoryAccessModule`).
///
/// Two things the group cannot read off the objects alone are stamped here, and
/// both mirror what the text body's [`filed_prim`] does:
///
/// - the **root wears the item's name and description**, because a take is not
///   a round trip and the item is what a viewer shows;
/// - each prim's **contents** are written in, which is the thing an object
///   update never carries. `contents` is one entry per member of `linkset`, in
///   the same order; a prim with nothing in it writes no `TaskInventory`
///   element at all, which is what OpenSim does.
///
/// Every part then goes through [`filed_part_defaults`], for the two things
/// OpenSim's own `SceneObjectPart` is *constructed* with and a prim the wire
/// described may not have.
fn filed_group(
    linkset: &[Object],
    contents: &[TaskInventory],
    item: &InventoryItem,
) -> sl_object_asset::opensim::SceneObjectGroup {
    let mut group = sl_object_asset::opensim::SceneObjectGroup::from_linkset(linkset);
    item.name.clone_into(&mut group.root.name);
    item.description.clone_into(&mut group.root.description);
    let mut inventories = contents.iter();
    if let Some(root_contents) = inventories.next() {
        group
            .root
            .set_task_inventory(root_contents.serial, &root_contents.items);
    }
    for (part, part_contents) in group.other_parts.iter_mut().zip(inventories) {
        part.set_task_inventory(part_contents.serial, &part_contents.items);
    }
    filed_part_defaults(&mut group.root);
    for part in &mut group.other_parts {
        filed_part_defaults(part);
    }
    group
}

/// The two things OpenSim's `SceneObjectPart` and `PrimitiveBaseShape`
/// constructors give a prim that a taken one may be missing.
///
/// The same gap [`taken_prim`] fills for the text body, and for the same
/// reason: an object rezzed by `ObjectAdd` has no `ObjectProperties` until
/// something edits it.
///
/// - A prim nobody has named is called `Object`, which is what OpenSim's
///   constructor sets and what a viewer shows. Only a child needs this — the
///   root wears the item's name — but a linkset root with no children is both.
/// - A prim with no `TextureEntry` at all gets the default one. OpenSim's shape
///   is constructed with `DEFAULT_TEXTURE` and its reader hands whatever it
///   decodes straight to `Primitive.TextureEntry`, so an empty block is not a
///   prim without textures there, it is a body it cannot read.
fn filed_part_defaults(part: &mut sl_object_asset::opensim::SceneObjectPart) {
    if part.name.is_empty() {
        DEFAULT_OBJECT_NAME.clone_into(&mut part.name);
    }
    if part.shape.texture_entry.is_empty() {
        part.shape.texture_entry = default_texture_entry();
    }
}

/// One object as the asset prim block a take **files** it as: [`taken_prim`],
/// with the linkset root wearing the item's own name and description.
///
/// A take is not a round trip — the item is what a viewer shows, and an object
/// rezzed by `ObjectAdd` has no `ObjectProperties` of its own yet — so the name
/// the asset states is the item's. Only the root's: a child prim keeps whatever
/// it was called.
///
/// Both halves of a take read this. The body is encoded from it, and a rez that
/// puts the *stored linkset* back derives the same properties record through it
/// ([`rezzed_properties`]), so the object a resident gets back cannot be named
/// one thing by the asset and another by the region.
fn filed_prim(object: &Object, item: &InventoryItem, root: bool) -> sl_object_asset::PrimBlock {
    let mut prim = taken_prim(object);
    if root {
        item.name.clone_into(&mut prim.name);
        prim.description = Some(item.description.clone());
    }
    prim
}

/// One object as the asset prim a take serialises it into.
fn taken_prim(object: &Object) -> sl_object_asset::PrimBlock {
    let faces = sl_object_asset::rendered_face_count(&object.shape);
    let mut prim = sl_object_asset::PrimBlock::from_object(object, faces);
    if prim.name.is_empty() {
        // What a viewer shows for a prim nobody has named, and what the item
        // itself is called. A prim the fake grid rezzed has no
        // `ObjectProperties` until something edits it, so this is the common
        // case rather than the odd one.
        DEFAULT_OBJECT_NAME.clone_into(&mut prim.name);
    }
    if prim.faces.is_empty() {
        // An object with no `TextureEntry` at all -- which a prim this grid
        // rezzed no longer is (`default_texture_entry`), but one rezzed from an
        // asset that stated no faces still can be. The asset has to state
        // the faces the shape renders either way -- an object asset with no
        // faces rezzes an object nothing can texture -- so they are stated as
        // untextured rather than omitted.
        prim.faces = vec![sl_object_asset::LegacyFace::default(); faces];
    }
    prim
}

/// The agent inventory item a **take** files an object away as.
///
/// Named after the linkset's **root** — the first of `linkset` — because that
/// is the object a resident selected and the name a viewer shows for the whole
/// set.
///
/// Whether the item **names** its asset is the grid's flavour talking
/// ([`ObjectAssetPolicy`](crate::assets::ObjectAssetPolicy)): OpenSim mints an
/// id and a viewer may fetch the body under it, Second Life hands a viewer a nil
/// id and no way to reach the body at all.
///
/// Either way the body itself is written — [`store_taken_asset`] follows this
/// choice and files it in the store the item points at. What must not happen is
/// a *minted* id nothing serves: that is the failure the round-trip cases hunt,
/// and it is worse than a nil one, because it fails later and looks fine in an
/// inventory window until it does.
pub(crate) fn taken_item(
    linkset: &[Object],
    folder: InventoryFolderKey,
    owner: AgentKey,
    mint: &dyn Fn() -> uuid::Uuid,
    (object_assets, masks, undescribed): (
        crate::assets::ObjectAssetPolicy,
        crate::imitates::TakenItemMasks,
        Option<&'static str>,
    ),
) -> InventoryItem {
    let properties = linkset.first().and_then(|root| root.properties.as_ref());
    // The object's own masks, which OpenSim folds: no export bit on the base
    // and owner masks, and the four slam bits set.
    let mut permissions =
        properties.map_or(NEW_OBJECT_PERMISSIONS, |properties| properties.permissions);
    if masks == crate::imitates::TakenItemMasks::Folded {
        let fold = |mask: sl_wire::Permissions| {
            sl_wire::Permissions::from_bits(
                mask.difference(sl_wire::Permissions::EXPORT).bits()
                    | crate::imitates::OPENSIM_FOLDED_BITS,
            )
        };
        permissions.base = fold(permissions.base);
        permissions.owner = fold(permissions.owner);
    }
    let named = properties.map_or(DEFAULT_OBJECT_NAME, |properties| properties.name.as_str());
    InventoryItem {
        item_id: InventoryKey::from(mint()),
        folder_id: folder,
        name: named.to_owned(),
        description: properties
            .map(|properties| properties.description.clone())
            .filter(|description| !description.is_empty())
            .or_else(|| undescribed.map(str::to_owned))
            .unwrap_or_default(),
        asset_id: match object_assets {
            crate::assets::ObjectAssetPolicy::Withheld => uuid::Uuid::nil(),
            crate::assets::ObjectAssetPolicy::Served => mint(),
        },
        item_type: narrow_code(AssetType::Object.to_code()),
        inv_type: narrow_code(InventoryType::Object.to_code()),
        flags: 0,
        sale_type: SaleType::NotForSale.to_code(),
        sale_price: None,
        creation_date: FIXTURE_CREATION_DATE,
        owner: OwnerKey::Agent(owner),
        last_owner_id: uuid::Uuid::nil(),
        creator_id: owner,
        group: None,
        permissions,
    }
}

/// Hands a client the inventory items the simulator just created, the way the
/// live grid this one is imitating does.
///
/// The two shapes are not interchangeable to a viewer, which is the whole point
/// of the switch: a client that only listens for the legacy UDP message hears
/// nothing at all from a grid that moved inventory to AIS3, and one that only
/// listens for the event-queue push hears nothing from OpenSim. A take against
/// the wrong flavour is where a viewer finds that out.
///
/// The `BulkUpdateInventory` side carries no folder blocks: the folder the item
/// went into already exists in the client's model — the client named it in the
/// derez — and a real grid's bulk update announces the *changed* objects.
pub(crate) fn announce_created_items(
    announcement: crate::inventory::InventoryAnnouncement,
    sim: &mut SimSession,
    created: &[InventoryItem],
    transaction_id: TransactionId,
    now: Instant,
) {
    let result = match announcement {
        crate::inventory::InventoryAnnouncement::Legacy => sim
            .send_inventory_item_created(created, transaction_id, true, now)
            .map_err(|error| error.to_string()),
        crate::inventory::InventoryAnnouncement::BulkUpdate => sim
            .enqueue_bulk_update_inventory(transaction_id, &[], created)
            .map_err(|error| error.to_string()),
    };
    if let Err(error) = result {
        tracing::warn!("handing over a taken object's inventory item failed: {error}");
    }
}

/// An `LLAssetType` / `LLInventoryType` code narrowed to the `i8` an inventory
/// item's wire block carries.
///
/// Every code either enum defines fits in an `i8`; one that did not would be a
/// code no viewer could read off the wire either, so it falls back to LL's own
/// "none" sentinel rather than to a plausible-looking wrong class.
fn narrow_code(code: i32) -> i8 {
    i8::try_from(code).unwrap_or(UNKNOWN_ASSET_CLASS)
}

/// LL's "no asset class" sentinel (`LLAssetType::AT_NONE`), which is also
/// `LLInventoryType::IT_NONE`.
const UNKNOWN_ASSET_CLASS: i8 = -1;

/// Puts the agent's own avatar back on its feet at `stands_at`: the session's
/// placement moved there, and the avatar re-sent with no parent. The region's
/// other viewers are to be shown it so too: the change that says so, for an
/// avatar the region shows.
pub(crate) fn stand_avatar(
    world: &mut SceneFixtures,
    identity: &AvatarIdentity,
    sim: &mut SimSession,
    stands_at: &Vector,
    now: Instant,
) -> Option<RegionChange> {
    let look_at = sim.arrival_position().look_at.clone();
    sim.set_arrival_position(
        RegionCoordinates::new(stands_at.x, stands_at.y, stands_at.z),
        look_at,
    );
    let avatar = avatar_prim(
        world.avatar_id(identity.agent_id),
        identity,
        stands_at.clone(),
    );
    if let Err(error) = send_objects(sim, core::slice::from_ref(&avatar), now) {
        tracing::warn!("standing the agent up failed: {error}");
    }
    world.reshow_resident(identity.agent_id, &avatar)
}

/// Rezzes the agent's own avatar **seated**: parented to `seat` (a
/// region-local id) at `offset` from it, which is how a seated avatar travels
/// on the wire. The region's other viewers are to be shown it so too: the
/// change that says so, for an avatar the region shows.
pub(crate) fn push_seated_avatar(
    world: &mut SceneFixtures,
    identity: &AvatarIdentity,
    seat: RegionLocalObjectId,
    (offset, rotation): (Vector, Rotation),
    sim: &mut SimSession,
    now: Instant,
) -> Option<RegionChange> {
    let mut avatar = avatar_prim(world.avatar_id(identity.agent_id), identity, offset);
    avatar.parent_id = seat;
    avatar.motion.rotation = rotation;
    if let Err(error) = send_objects(sim, core::slice::from_ref(&avatar), now) {
        tracing::warn!("seating the agent's avatar failed: {error}");
    }
    world.reshow_resident(identity.agent_id, &avatar)
}

/// Sends `record`, or the "no such parcel" reply (an empty record whose
/// `RequestResult` is [`ParcelRequestResult::NoData`]) when there is none.
fn send_parcel_or_no_data(sim: &mut SimSession, record: Option<ParcelInfo>, sequence_id: i32) {
    let record = record.unwrap_or_else(|| {
        let mut none = region_wide_parcel(
            RegionLocalParcelId(-1),
            OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
            "",
        );
        none.sequence_id = sequence_id;
        none.request_result = ParcelRequestResult::NoData;
        none.bitmap = Vec::new();
        none.area = LandArea(0);
        none.raw_parcel_flags = 0;
        none
    });
    // Over the event queue, as both live grids answer: the UDP message has no
    // room for the media and extended-flag blocks.
    sim.enqueue_parcel_properties(&record);
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    /// A fixed agent for the overlay tests.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(id))
    }

    /// A one-metre box owned by agent `0x1`, carrying a physics bit of its
    /// own and a stale permission bit a viewer must never be sent.
    fn owned_box() -> Object {
        let mut object = box_prim(
            RegionLocalObjectId(7),
            ObjectKey::from(uuid::Uuid::from_u128(0x77)),
            agent(0x1),
            Vector {
                x: 128.0,
                y: 128.0,
                z: 25.0,
            },
            Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        object.update_flags = prim_flags::USE_PHYSICS | prim_flags::OBJECT_YOU_OWNER;
        object
    }

    /// The owner of a new prim is told it owns it and may do anything with
    /// it; anyone else is told somebody owns it and it may be transferred —
    /// on Second Life that the owner may modify it too — and the object's own
    /// bits reach both.
    #[test]
    fn a_new_prim_is_the_owners_to_edit_and_nobody_elses() {
        let object = owned_box();
        let owner = as_seen_by(&object, Some(agent(0x1)), false).update_flags;
        let other = as_seen_by(&object, Some(agent(0x2)), false).update_flags;
        assert_eq!(
            owner,
            prim_flags::USE_PHYSICS
                | prim_flags::OBJECT_MODIFY
                | prim_flags::OBJECT_COPY
                | prim_flags::OBJECT_ANY_OWNER
                | prim_flags::OBJECT_YOU_OWNER
                | prim_flags::OBJECT_MOVE
                | prim_flags::OBJECT_TRANSFER
                | prim_flags::OBJECT_OWNER_MODIFY
        );
        assert_eq!(
            other,
            prim_flags::USE_PHYSICS | prim_flags::OBJECT_ANY_OWNER | prim_flags::OBJECT_TRANSFER
        );
        assert_eq!(
            as_seen_by(&object, Some(agent(0x2)), true).update_flags,
            other | prim_flags::OBJECT_OWNER_MODIFY
        );
    }

    /// What the everyone mask grants, anyone else is told; a locked object
    /// (its owner may not move it) is moved and modified by nobody.
    #[test]
    fn the_everyone_mask_decides_what_anyone_else_may_do() {
        let mut object = owned_box();
        let mut properties = default_object_properties(&object);
        properties.permissions.everyone =
            sl_wire::Permissions::MODIFY.union(sl_wire::Permissions::MOVE);
        object.properties = Some(properties.clone());
        assert_eq!(
            as_seen_by(&object, Some(agent(0x2)), false).update_flags,
            prim_flags::USE_PHYSICS
                | prim_flags::OBJECT_ANY_OWNER
                | prim_flags::OBJECT_MOVE
                | prim_flags::OBJECT_MODIFY
                | prim_flags::OBJECT_TRANSFER
        );
        properties.permissions.owner =
            sl_wire::Permissions::ALL.difference(sl_wire::Permissions::MOVE);
        object.properties = Some(properties);
        assert_eq!(
            as_seen_by(&object, Some(agent(0x2)), false).update_flags,
            prim_flags::USE_PHYSICS | prim_flags::OBJECT_ANY_OWNER | prim_flags::OBJECT_TRANSFER
        );
        assert_eq!(
            as_seen_by(&object, Some(agent(0x1)), false).update_flags
                & (prim_flags::OBJECT_MOVE | prim_flags::OBJECT_MODIFY),
            0,
            "not even the owner moves a locked object"
        );
    }

    /// A region identity at grid `(grid_x, grid_y)`, as the runtime mints one
    /// for a configured region — enough of it for a parcel listing, which
    /// reads only the name and the handle.
    fn test_region(name: &str, grid_x: u32, grid_y: u32) -> RegionIdentity {
        RegionIdentity {
            sim_name: sl_wire::region_name_from_wire("fake-grid", name)
                .ok()
                .flatten(),
            region_id: uuid::Uuid::nil(),
            region_handle: sl_wire::RegionHandle::from_grid(grid_x, grid_y),
            grid_coordinates: sl_types::map::GridCoordinates::new(grid_x, grid_y),
            region_flags: sl_proto::RegionFlags::default(),
            region_flags_extended: None,
            region_protocols: 0,
            maturity: sl_proto::Maturity::Pg,
            product: sl_proto::ProductType::FullRegion,
            product_sku: String::new(),
            product_name: name.to_owned(),
            colo_name: String::new(),
            cpu_class_id: 0,
            cpu_ratio: 1,
            sim_owner: uuid::Uuid::nil(),
            is_estate_manager: false,
            water_height: 20.0,
            billable_factor: 1.0,
            terrain: crate::TerrainFixture::default().composition,
        }
    }

    #[test]
    fn empty_world_overlay_is_all_public() {
        let overlay = SceneFixtures::new().overlay_for(agent(1));
        assert_eq!(overlay.len(), 4096);
        assert!(overlay.iter().all(|&byte| byte == OVERLAY_PUBLIC));
    }

    #[test]
    fn region_wide_parcel_overlay_marks_owner_and_edges() {
        let mut world = SceneFixtures::new();
        world.parcels.push(region_wide_parcel(
            RegionLocalParcelId(1),
            OwnerKey::Agent(agent(7)),
            "Mine",
        ));
        let own = world.overlay_for(agent(7));
        // South-west corner: own land, both edge lines.
        assert_eq!(
            own.first().copied(),
            Some(OVERLAY_OWNED_BY_REQUESTER | OVERLAY_WEST_LINE | OVERLAY_SOUTH_LINE)
        );
        // An interior cell: own land, no lines.
        assert_eq!(
            own.get(CELLS_PER_EDGE + 1).copied(),
            Some(OVERLAY_OWNED_BY_REQUESTER)
        );
        let other = world.overlay_for(agent(8));
        assert_eq!(
            other.get(CELLS_PER_EDGE + 1).copied(),
            Some(OVERLAY_OWNED_BY_OTHER)
        );
        assert!(world.parcel_at(255.0, 255.0).is_some());
        assert!(world.parcel_at(256.0, 0.0).is_none());
    }

    #[test]
    fn avatar_prim_carries_the_name_values() {
        let identity = AvatarIdentity {
            agent_id: agent(9),
            first_name: "Test".to_owned(),
            last_name: "User".to_owned(),
        };
        let avatar = avatar_prim(RegionLocalObjectId(1), &identity, ZERO);
        assert_eq!(avatar.pcode, pcode::AVATAR);
        assert_eq!(avatar.full_id.uuid(), agent(9).uuid());
        assert!(
            avatar
                .name_value
                .starts_with("FirstName STRING RW SV Test\n")
        );
        assert!(avatar.name_value.contains("LastName STRING RW SV User\n"));
    }

    /// Two rectangular parcels split the region down the middle, and the
    /// overlay marks the **interior** boundary — the west-edge bit on the
    /// eastern parcel's first column, which is the only thing a viewer's
    /// property line at `x = 128` can be drawn from.
    #[test]
    fn two_rect_parcels_put_a_line_down_the_middle() {
        let mut world = SceneFixtures::new();
        world.parcels.push(rect_parcel(
            RegionLocalParcelId(1),
            OwnerKey::Agent(agent(7)),
            "West",
            0.0,
            0.0,
            128.0,
            256.0,
        ));
        world.parcels.push(rect_parcel(
            RegionLocalParcelId(2),
            OwnerKey::Agent(agent(7)),
            "East",
            128.0,
            0.0,
            256.0,
            256.0,
        ));
        assert_eq!(
            world.parcel_at(64.0, 64.0).map(|found| found.local_id),
            Some(RegionLocalParcelId(1))
        );
        assert_eq!(
            world.parcel_at(192.0, 64.0).map(|found| found.local_id),
            Some(RegionLocalParcelId(2))
        );
        // Half a region each, in square metres.
        assert_eq!(
            world.parcels.first().map(|parcel| parcel.area),
            Some(LandArea(0x0000_8000))
        );
        let overlay = world.overlay_for(agent(7));
        // The interior boundary: the first cell of the eastern parcel's west
        // column (`x = 128` is block 32) carries the west-edge bit, and the
        // cell west of it does not.
        let cell = |x: usize, y: usize| {
            overlay
                .get(y.saturating_mul(CELLS_PER_EDGE).saturating_add(x))
                .copied()
        };
        assert_eq!(
            cell(32, 8),
            Some(OVERLAY_OWNED_BY_REQUESTER | OVERLAY_WEST_LINE)
        );
        assert_eq!(cell(31, 8), Some(OVERLAY_OWNED_BY_REQUESTER));
        // And a region-wide parcel has no such line, which is what makes the
        // two states tell apart in a picture.
        let mut whole = SceneFixtures::new();
        whole.parcels.push(region_wide_parcel(
            RegionLocalParcelId(1),
            OwnerKey::Agent(agent(7)),
            "All",
        ));
        let joined = whole.overlay_for(agent(7));
        assert_eq!(
            joined
                .get(8_usize.saturating_mul(CELLS_PER_EDGE).saturating_add(32))
                .copied(),
            Some(OVERLAY_OWNED_BY_REQUESTER)
        );
    }

    /// Both halves of a parcel go in together, and its search listing is
    /// derived from the record rather than restated: the name, owner and area
    /// a `ParcelInfoRequest` answers with are the ones the
    /// `ParcelProperties` reply carries, and the id and dwell are the ones
    /// only the listing knows.
    // The listing below is derived from the parcel as it stands at the moment
    // it is asked for. The live OpenSim caches a listing for 30 s past its last
    // read instead, and the fake grid does not imitate that — see the
    // `RequestParcelInfo` arm above before trusting this test as a model of
    // what a live read-after-edit returns.
    #[test]
    fn a_parcel_carries_its_grid_wide_half() -> Result<(), String> {
        let parcel_id = ParcelKey::from(uuid::Uuid::from_u128(0xBEEF));
        let mut world = SceneFixtures::new();
        world.add_parcel(
            region_wide_parcel(
                RegionLocalParcelId(3),
                OwnerKey::Agent(agent(7)),
                "Sunny Plaza",
            ),
            parcel_id,
            42.5,
        );

        let listing = world
            .listing_by_local_id(RegionLocalParcelId(3))
            .ok_or("the parcel has no listing")?;
        assert_eq!(listing.parcel_id, parcel_id);
        let (by_id, parcel) = world
            .listing_by_parcel_id(parcel_id)
            .ok_or("the grid-wide id resolves to nothing")?;
        assert_eq!(by_id, listing);
        assert_eq!(parcel.local_id, RegionLocalParcelId(3));

        // The region's south-west corner is at (1000, 1000) regions =
        // (256_000, 256_000) metres, and the parcel starts at the corner.
        let region = test_region("Fake Region", 1000, 1000);
        let details = listing.details(parcel, &region, sl_proto::AdultListingBits::AdultAndMature);
        // An agent's general-rated parcel that is not for sale: no flag at all.
        assert_eq!(details.flags, sl_proto::ParcelListingFlags::NONE);
        assert_eq!(details.sale_price, None);
        assert_eq!(details.parcel_id, parcel_id);
        assert_eq!(details.name, "Sunny Plaza");
        assert_eq!(details.owner_id, agent(7).uuid());
        assert_eq!(details.actual_area, parcel.area);
        assert_eq!(details.dwell.to_bits(), 42.5_f32.to_bits());
        assert_eq!(
            details.sim_name.as_ref().map(ToString::to_string),
            Some("Fake Region".to_owned())
        );
        assert_eq!(
            details.global_position.x().to_bits(),
            256_000.0_f64.to_bits()
        );
        assert_eq!(
            details.global_position.y().to_bits(),
            256_000.0_f64.to_bits()
        );
        Ok(())
    }

    /// The stand animation the grid signals is the registry's `stand`, not a
    /// second copy of the uuid that could drift from it. It is a `const` rather
    /// than a lookup because it is one, but the registry stays the authority.
    #[test]
    fn the_stand_animation_is_the_builtin_one() {
        assert_eq!(
            sl_anim::builtin_animation_by_name("stand").map(|found| found.id),
            Some(crate::animations::STAND)
        );
    }

    /// Each avatar has a region-local id of its own: the first to arrive the
    /// region's first-avatar id, every other one minted; one that leaves and
    /// comes back is given a new one; and no prim is minted an avatar's.
    #[test]
    fn every_avatar_has_a_region_local_id_of_its_own() {
        let (first, second) = (agent(0xA1), agent(0xA2));
        let mut world = SceneFixtures::new();
        assert_eq!(world.arrive_avatar(first), RegionLocalObjectId(1));
        let second_id = world.arrive_avatar(second);
        assert_ne!(second_id, RegionLocalObjectId(1));
        assert_eq!(
            world.arrive_avatar(second),
            second_id,
            "an id is kept while it stays"
        );
        assert_eq!(world.avatar_id(second), second_id);
        assert!(world.mint_local_id().0 > second_id.0);
        world.depart_avatar(first);
        let back = world.arrive_avatar(first);
        assert!(
            back != RegionLocalObjectId(1) && back != second_id,
            "a return reused {back:?}"
        );
    }

    /// A minted region-local id is above everything the region already uses —
    /// its prims, its NPC bodies and the arriving agent's own avatar — and
    /// stays above an id that has since been derezzed, so a viewer keyed on a
    /// local id can never be handed a second object under the first one's
    /// handle.
    #[test]
    fn minted_local_ids_never_collide_with_a_used_or_freed_one() {
        let owner = agent(0x1);
        let mut world = SceneFixtures::new();
        world.avatar_local_id = RegionLocalObjectId(9);
        world.objects.push(box_prim(
            RegionLocalObjectId(4),
            ObjectKey::from(uuid::Uuid::from_u128(4)),
            owner,
            ZERO,
            ZERO,
        ));

        // Above the avatar, which is the highest id in use -- not above the
        // highest *prim*.
        let first = world.mint_local_id();
        assert_eq!(first, RegionLocalObjectId(10));

        world.objects.push(box_prim(
            first,
            ObjectKey::from(uuid::Uuid::from_u128(10)),
            owner,
            ZERO,
            ZERO,
        ));
        let second = world.mint_local_id();
        assert_eq!(second, RegionLocalObjectId(11));

        // The freed id is not handed out again.
        let removed = world.remove_object(first);
        assert!(removed.is_some(), "the minted prim was not in the region");
        assert_eq!(world.mint_local_id(), RegionLocalObjectId(12));
    }

    /// Removing a prim takes its contents with it: an id that comes back
    /// round to a new object must not inherit the old one's inventory.
    #[test]
    fn removing_a_prim_drops_its_task_inventory() -> Result<(), &'static str> {
        let mut world = SceneFixtures::new();
        let local_id = RegionLocalObjectId(4);
        world.objects.push(box_prim(
            local_id,
            ObjectKey::from(uuid::Uuid::from_u128(4)),
            agent(0x1),
            ZERO,
            ZERO,
        ));
        world
            .add_task_inventory(local_id, TaskInventory::stated(3, Vec::new()))
            .map_err(|_contents| "the prim is not in the region")?;
        let _removed = world.remove_object(local_id);
        assert_eq!(world.objects.contents(local_id), None);
        Ok(())
    }

    /// A task-inventory write advances the serial — which is the whole
    /// observable, since a viewer that sees the same serial twice keeps the
    /// listing it already downloaded — and re-writing one item replaces it
    /// A second item of one name is given the first free number, as both
    /// live grids give it; a remove advances the serial and forgets what
    /// was kept about the item; and a remove of an item that is not there
    /// leaves the serial alone.
    #[test]
    fn task_items_are_named_apart_and_removed_with_what_is_kept_of_them() {
        let task = ObjectKey::from(uuid::Uuid::from_u128(0x7A5C));
        let mut contents = TaskInventory::default();
        assert_eq!(contents.free_name("Note"), "Note");
        for (id, name) in [(1_u128, "Note"), (2, "Note 1")] {
            let name = contents.free_name(name);
            contents.write(task_item(task, id, &name));
        }
        assert_eq!(contents.free_name("Note"), "Note 2");
        assert_eq!(contents.free_name("Other"), "Other");

        let first = InventoryKey::from(uuid::Uuid::from_u128(1));
        let _was = contents.running.insert(first);
        let serial = contents.serial;
        assert!(contents.remove(first).is_some());
        assert_eq!(contents.serial, serial + 1);
        assert!(contents.running.is_empty());
        assert!(contents.remove(first).is_none());
        assert_eq!(contents.serial, serial + 1, "nothing was removed");
        contents.touch();
        assert_eq!(contents.serial, serial + 2);
    }

    /// rather than listing it twice.
    #[test]
    fn a_task_inventory_write_advances_the_serial() {
        let task = ObjectKey::from(uuid::Uuid::from_u128(0x7));
        let mut contents = TaskInventory::default();
        assert_eq!(contents.serial, 0);

        let item = task_item(task, 0x11, "Notecard");
        contents.write(item.clone());
        assert_eq!(contents.serial, 1);
        assert_eq!(contents.items.len(), 1);

        contents.write(TaskInventoryItem {
            name: "Renamed".to_owned(),
            ..item
        });
        assert_eq!(contents.serial, 2, "a re-write is still a change");
        assert_eq!(contents.items.len(), 1, "the same id was listed twice");
        assert_eq!(
            contents.items.first().map(|held| held.name.as_str()),
            Some("Renamed")
        );

        contents.write(task_item(task, 0x12, "Script"));
        assert_eq!(contents.serial, 3);
        assert_eq!(contents.items.len(), 2);
    }

    /// The prim a client's shape rezzes as carries the shape it described:
    /// the client sends the path/profile fields already quantized, so the
    /// object a rez produces round-trips them unchanged.
    #[test]
    fn a_rezzed_prim_carries_the_shape_the_client_described() {
        let position = Vector {
            x: 12.0,
            y: 34.0,
            z: 56.0,
        };
        let mut shape = sl_proto::PrimShape::cube(position.clone());
        shape.profile_hollow = 25_000;
        shape.path_twist = 40;
        shape.state = 3;
        let object = prim_from_shape(
            RegionLocalObjectId(7),
            ObjectKey::from(uuid::Uuid::from_u128(0x7)),
            agent(0x1),
            &shape,
        );
        assert_eq!(object.local_id, RegionLocalObjectId(7));
        assert_eq!(object.motion.position, position);
        assert_eq!(object.scale, shape.scale);
        assert_eq!(object.state, 3);
        assert_eq!(object.shape.profile_hollow, 25_000);
        assert_eq!(object.shape.path_twist, 40);
        assert_eq!(object.shape.path_curve, shape.path_curve);
        assert_eq!(object.shape.profile_curve, shape.profile_curve);
        assert_eq!(object.material, shape.material.to_code());
    }

    /// A prim a client rezzes wears the default plywood on every face the
    /// shape renders — a real simulator textures a new prim, and a client that
    /// received no per-face data at all would have nothing to retexture.
    ///
    /// The shape here is a hollow cut box, whose rendered face count is *not*
    /// the six a plain box has; both are asserted from one blob, which is what
    /// the run-length encoding buys ([`default_texture_entry`]).
    #[test]
    fn a_rezzed_prim_wears_the_default_texture() {
        let mut shape = sl_proto::PrimShape::cube(ZERO);
        shape.profile_hollow = 25_000;
        shape.profile_begin = 2_000;
        let object = prim_from_shape(
            RegionLocalObjectId(9),
            ObjectKey::from(uuid::Uuid::from_u128(0x9)),
            agent(0x1),
            &shape,
        );

        let faces = sl_object_asset::rendered_face_count(&object.shape);
        assert!(
            faces > 6,
            "a hollow cut box renders more than a plain box's six faces; got {faces}"
        );
        let entry = sl_proto::decode_texture_entry(&object.texture_entry, faces);
        assert_eq!(entry.faces.len(), faces);
        for (index, face) in entry.faces.iter().enumerate() {
            assert_eq!(
                face.texture_id.uuid(),
                sl_proto::DEFAULT_PRIM_TEXTURE,
                "face {index} is not the default prim texture"
            );
        }

        // The same blob read as a plain box's six faces is plywood too: the
        // entry states one default and no overrides, so the face count is the
        // reader's business.
        let six = sl_proto::decode_texture_entry(&object.texture_entry, 6);
        assert!(
            six.faces
                .iter()
                .all(|face| face.texture_id.uuid() == sl_proto::DEFAULT_PRIM_TEXTURE),
            "the entry is not face-count independent"
        );
    }

    /// A take of a prim the grid rezzed writes the faces it actually wears
    /// into the object asset, rather than the untextured stand-ins a prim with
    /// no `TextureEntry` forced.
    #[test]
    fn a_taken_rezzed_prim_states_its_default_faces() {
        let object = prim_from_shape(
            RegionLocalObjectId(9),
            ObjectKey::from(uuid::Uuid::from_u128(0x9)),
            agent(0x1),
            &sl_proto::PrimShape::cube(ZERO),
        );
        let prim = taken_prim(&object);
        assert_eq!(
            prim.faces.len(),
            sl_object_asset::rendered_face_count(&object.shape)
        );
        assert!(
            prim.faces
                .iter()
                .all(|face| face.image_id == sl_proto::DEFAULT_PRIM_TEXTURE),
            "a taken prim's faces are not the default texture"
        );
    }

    /// The two halves of a take, side by side, on **both** flavours: what the
    /// published body carries is the one thing the two grids disagree about,
    /// and what the rez hands back is the one thing they do not.
    ///
    /// All of it is asserted in one test on purpose. Each half alone is
    /// passable by a grid that is wrong in another direction — a Linden text
    /// that invented keywords no grid could read, an XML body that dropped the
    /// fields it exists to carry, or a rez that quietly handed back a box — and
    /// the decision this encodes is that **each format stays what its own grid
    /// writes** while the resident gets their lamp back either way.
    #[test]
    fn a_take_publishes_each_grids_own_body_and_rezzes_the_prim_it_took()
    -> Result<(), Box<dyn core::error::Error>> {
        let owner = agent(0x1);
        let mut world = SceneFixtures::new();
        let mut taken = box_prim(
            RegionLocalObjectId(5),
            ObjectKey::from(uuid::Uuid::from_u128(5)),
            owner,
            ZERO,
            ZERO,
        );
        taken.extra = ObjectExtraParams {
            light: Some(sl_proto::LightData {
                color: [255, 220, 160, 255],
                radius: 10.0,
                cutoff: 1.2,
                falloff: 0.5,
            }),
            ..ObjectExtraParams::default()
        };
        taken.extra_params = sl_proto::encode_extra_params(&taken.extra);
        taken.text = "a lamp".to_owned();

        let assets = crate::assets::GridAssets::default();
        let folder = InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0));
        let filed = |policy, id: u128| InventoryItem {
            name: "Lamp".to_owned(),
            ..taken_item(
                std::slice::from_ref(&taken),
                folder,
                owner,
                &|| uuid::Uuid::from_u128(id),
                (policy, crate::imitates::TakenItemMasks::Objects, None),
            )
        };

        // Second Life's side: a nil asset id, the Linden text out of reach of
        // every capability, and the light gone out of it. That loss is the
        // format's, and inventing a keyword for it would write an asset no grid
        // could read -- see `sl_object_asset`'s crate docs.
        let withheld = filed(crate::assets::ObjectAssetPolicy::Withheld, 0xA55E7);
        store_taken_asset(
            &assets,
            &withheld,
            std::slice::from_ref(&taken),
            (&[], Vec::new()),
            Instant::now(),
        );
        let text = assets
            .withheld_object(withheld.item_id)
            .ok_or("the withheld take published no body")?;
        let published = sl_object_asset::ObjectAsset::decode(&text)?
            .root()
            .cloned()
            .ok_or("the published body holds no prim")?;
        let from_text = published.to_object(sl_object_asset::RezTarget {
            region_handle: sl_wire::RegionHandle(0),
            local_id: RegionLocalObjectId(9),
            full_id: uuid::Uuid::from_u128(9),
            parent_id: RegionLocalObjectId(0),
        });
        assert_eq!(
            from_text.extra,
            ObjectExtraParams::default(),
            "the text carries no ExtraParams; if it now does, this is the wrong test"
        );
        assert_eq!(from_text.text, "");
        assert_eq!(published.name, "Lamp", "the body is named after the item");

        // OpenSim's side: a minted asset id, a `<SceneObjectGroup>` served
        // under it, and the light and the floating text still in it. This is
        // the format OpenSim writes, and it can say all of it.
        let served = filed(crate::assets::ObjectAssetPolicy::Served, 0x00B0_D1E5);
        store_taken_asset(
            &assets,
            &served,
            std::slice::from_ref(&taken),
            (&[], Vec::new()),
            Instant::now(),
        );
        let body = assets
            .read()
            .get(AssetKey::from(served.asset_id))
            .map(<[u8]>::to_vec)
            .ok_or("the served take published no body")?;
        let group = sl_object_asset::opensim::SceneObjectGroup::decode(&body)?;
        let from_xml = group.root.to_object(sl_object_asset::RezTarget {
            region_handle: sl_wire::RegionHandle(0),
            local_id: RegionLocalObjectId(9),
            full_id: uuid::Uuid::from_u128(9),
            parent_id: RegionLocalObjectId(0),
        });
        assert_eq!(from_xml.extra, taken.extra, "the XML lost the light");
        assert_eq!(from_xml.text, taken.text, "the XML lost the floating text");
        assert_eq!(
            from_xml.extra_params, taken.extra_params,
            "the XML lost the packed ExtraParams blob"
        );
        assert_eq!(group.root.name, "Lamp", "the body is named after the item");

        // The rez: the prim that was taken, under new ids, on either flavour.
        let rezzed = rez_taken_linkset(
            &mut world,
            sl_wire::RegionHandle(0),
            &|| uuid::Uuid::from_u128(0x9E7),
            &[taken.clone()],
            &served,
            Some(&ZERO),
        );
        let root = rezzed.first().ok_or("the rez put nothing back")?;
        assert_eq!(root.extra, taken.extra, "the rez lost the light");
        assert_eq!(root.text, taken.text, "the rez lost the floating text");
        assert_ne!(root.full_id, taken.full_id, "a rez mints a fresh key");
        assert_ne!(
            root.local_id, taken.local_id,
            "a rez mints a fresh local id"
        );
        assert_eq!(
            root.properties.as_ref().map(|record| record.name.as_str()),
            Some("Lamp"),
            "the rezzed object is not named after the item it came out of"
        );
        Ok(())
    }

    /// A linkset taken into inventory comes back in its link order: the take
    /// files it root first and the children by their link numbers, and the
    /// rez puts them in the region in that order — a set whose newcomer was
    /// numbered 2 on Second Life (`A C B`) is still `A C B` once rezzed.
    #[test]
    fn a_taken_linkset_comes_back_in_its_link_order() -> Result<(), String> {
        let owner = agent(0x1);
        let mut world = SceneFixtures::new();
        world.linking = crate::imitates::ImitatedGrid::SecondLife.link_policy();
        let named = |index: u32| {
            let mut prim = box_prim(
                RegionLocalObjectId(index),
                ObjectKey::from(uuid::Uuid::from_u128(u128::from(index))),
                owner,
                ZERO,
                ZERO,
            );
            let mut record = default_object_properties(&prim);
            record.name = format!("prim {index}");
            prim.properties = Some(record);
            prim
        };
        for index in [10, 11, 12] {
            world.objects.push(named(index));
        }
        let (a, b, c) = (
            RegionLocalObjectId(10),
            RegionLocalObjectId(11),
            RegionLocalObjectId(12),
        );
        let _linked = crate::link_sets::link(&mut world, &[a, b])
            .map_err(|refusal| format!("{refusal:?}"))?;
        let _linked = crate::link_sets::link(&mut world, &[a, c])
            .map_err(|refusal| format!("{refusal:?}"))?;
        let taken: Vec<Object> = crate::link_sets::prims_in_order(&world, a)
            .into_iter()
            .filter_map(|prim| world.object_by_local_id(prim))
            .collect();
        for prim in [a, b, c] {
            let _gone = world.remove_object(prim);
        }
        let item = InventoryItem {
            name: "Set".to_owned(),
            ..taken_item(
                &taken,
                InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0)),
                owner,
                &|| uuid::Uuid::from_u128(0x5E7),
                (
                    crate::assets::ObjectAssetPolicy::Served,
                    crate::imitates::TakenItemMasks::Objects,
                    None,
                ),
            )
        };
        let next = std::cell::Cell::new(0x100_u128);
        let rezzed = rez_taken_linkset(
            &mut world,
            sl_wire::RegionHandle(0),
            &|| {
                next.set(next.get() + 1);
                uuid::Uuid::from_u128(next.get())
            },
            &taken,
            &item,
            Some(&ZERO),
        );
        world.objects.extend(rezzed.iter().cloned());
        let root = rezzed.first().ok_or("the rez put nothing back")?.local_id;
        let names: Vec<String> = crate::link_sets::prims_in_order(&world, root)
            .into_iter()
            .filter_map(|prim| world.objects.get(prim))
            .filter_map(|prim| prim.properties.as_ref().map(|record| record.name.clone()))
            .collect();
        assert_eq!(names, vec!["Set", "prim 12", "prim 11"]);
        Ok(())
    }

    /// An object nobody has named reports the record a freshly rezzed prim has,
    /// and its contents serial comes from the inventory a write advances —
    /// never from a second copy on the object, which could then disagree with
    /// the listing it describes.
    #[test]
    fn a_properties_record_is_synthesised_and_carries_the_live_serial() -> Result<(), String> {
        let owner = agent(0x1);
        let local_id = RegionLocalObjectId(4);
        let mut world = SceneFixtures::new();
        world.objects.push(box_prim(
            local_id,
            ObjectKey::from(uuid::Uuid::from_u128(4)),
            owner,
            ZERO,
            ZERO,
        ));

        let fresh = world
            .properties_of(local_id)
            .ok_or("the object has no properties")?;
        assert_eq!(fresh.name, DEFAULT_OBJECT_NAME);
        assert_eq!(fresh.description, "");
        assert_eq!(fresh.owner, OwnerKey::Agent(owner));
        assert_eq!(fresh.sale_price, None);
        assert_eq!(fresh.inventory_serial, 0);

        world
            .add_task_inventory(local_id, TaskInventory::stated(3, Vec::new()))
            .map_err(|_contents| "the object is not in the region")?;
        let stocked = world
            .properties_of(local_id)
            .ok_or("the object has no properties")?;
        assert_eq!(stocked.inventory_serial, 3);

        // An edit is stored on the object, and the serial still comes from the
        // inventory rather than from the record that was written.
        let mut held = world.object_mut(local_id).ok_or("the object went away")?;
        held.properties = Some(ObjectProperties {
            name: "Renamed".to_owned(),
            inventory_serial: 99,
            ..stocked
        });
        drop(held);
        let edited = world
            .properties_of(local_id)
            .ok_or("the object has no properties")?;
        assert_eq!(edited.name, "Renamed");
        assert_eq!(edited.inventory_serial, 3);
        Ok(())
    }

    /// An undo restores the object as it was before the last edit, a redo puts
    /// the edit back, and a fresh edit abandons the redo branch — the three
    /// rules that make a history compose across edits of different kinds.
    #[test]
    fn an_edit_history_steps_back_and_forward() -> Result<(), String> {
        let owner = agent(0x1);
        let local_id = RegionLocalObjectId(4);
        let mut world = SceneFixtures::new();
        world.objects.push(box_prim(
            local_id,
            ObjectKey::from(uuid::Uuid::from_u128(4)),
            owner,
            ZERO,
            ZERO,
        ));
        let moved = Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };

        // Nothing recorded: an undo is silence, not an error.
        assert_eq!(world.step_history(local_id, true), None);

        world.record_undo(local_id);
        world
            .object_mut(local_id)
            .ok_or("the object went away")?
            .motion
            .position = moved.clone();

        let (_before, undone) = world
            .step_history(local_id, true)
            .ok_or("the undo restored nothing")?;
        assert_eq!(undone.motion.position, ZERO);
        let (_before, redone) = world
            .step_history(local_id, false)
            .ok_or("the redo restored nothing")?;
        assert_eq!(redone.motion.position, moved);

        // Back one step, then a fresh edit: the branch the redo would have
        // returned to is now unreachable.
        let _undone = world.step_history(local_id, true);
        world.record_undo(local_id);
        world
            .object_mut(local_id)
            .ok_or("the object went away")?
            .motion
            .position = Vector {
            x: 9.0,
            y: 9.0,
            z: 9.0,
        };
        assert_eq!(world.step_history(local_id, false), None);
        Ok(())
    }

    /// An undo puts an object back where it stood and leaves everything else
    /// about it as it is, and a history holds no more placements than the
    /// grid keeps.
    #[test]
    fn an_undo_restores_a_placement_and_nothing_else() -> Result<(), String> {
        let owner = agent(0x1);
        let local_id = RegionLocalObjectId(4);
        let mut world = SceneFixtures::new();
        world.edit = crate::imitates::ImitatedGrid::OpenSim.edit_policy();
        world.objects.push(box_prim(
            local_id,
            ObjectKey::from(uuid::Uuid::from_u128(4)),
            owner,
            ZERO,
            ZERO,
        ));
        for step in 1..=8_u8 {
            world.record_undo(local_id);
            let mut held = world.object_mut(local_id).ok_or("the object went away")?;
            held.motion.position.z = f32::from(step);
            held.material = step;
        }
        let mut undone = 0_usize;
        while world.step_history(local_id, true).is_some() {
            undone = undone.saturating_add(1);
        }
        assert_eq!(undone, world.edit.undo_depth);
        let held = world
            .object_by_local_id(local_id)
            .ok_or("the object went away")?;
        // Eight moves, five undone: it stands where the third left it, and
        // its material is the last one set.
        assert!((held.motion.position.z - 3.0).abs() < f32::EPSILON);
        assert_eq!(held.material, 8);
        Ok(())
    }

    /// An object's full id resolves to its region-local one — the lookup the
    /// `Undo` / `Redo` messages need, being the only client messages in the
    /// object family that address an object that way.
    #[test]
    fn a_full_id_resolves_to_a_region_local_one() {
        let full_id = ObjectKey::from(uuid::Uuid::from_u128(0x4444));
        let mut world = SceneFixtures::new();
        world.objects.push(box_prim(
            RegionLocalObjectId(4),
            full_id,
            agent(0x1),
            ZERO,
            ZERO,
        ));
        assert_eq!(world.local_id_of(full_id), Some(RegionLocalObjectId(4)));
        assert_eq!(
            world.local_id_of(ObjectKey::from(uuid::Uuid::from_u128(0x5555))),
            None
        );
    }

    /// A task-inventory item fixture with a given id and name.
    /// The **served** body carries what the prim contains, which no object
    /// update ever does.
    ///
    /// OpenSim writes a prim's `TaskInventory` into the asset, so a scripted
    /// object taken there comes back scripted. The contents reach the body only
    /// because the take gathers them out of the region while it still has the
    /// object; a body written from the object update alone would file every
    /// scripted prim as an empty one, and nothing downstream could tell.
    #[test]
    fn a_served_body_carries_the_prims_contents() -> Result<(), Box<dyn core::error::Error>> {
        let owner = agent(0x1);
        let taken = box_prim(
            RegionLocalObjectId(6),
            ObjectKey::from(uuid::Uuid::from_u128(6)),
            owner,
            ZERO,
            ZERO,
        );
        let contents =
            TaskInventory::stated(4, vec![task_item(taken.full_id, 0x00C0_FFEE, "a notecard")]);

        let assets = crate::assets::GridAssets::default();
        let item = taken_item(
            std::slice::from_ref(&taken),
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0)),
            owner,
            &|| uuid::Uuid::from_u128(0xC047),
            (
                crate::assets::ObjectAssetPolicy::Served,
                crate::imitates::TakenItemMasks::Objects,
                None,
            ),
        );
        store_taken_asset(
            &assets,
            &item,
            std::slice::from_ref(&taken),
            (std::slice::from_ref(&contents), Vec::new()),
            Instant::now(),
        );

        let body = assets
            .read()
            .get(AssetKey::from(item.asset_id))
            .map(<[u8]>::to_vec)
            .ok_or("the take published no body")?;
        let group = sl_object_asset::opensim::SceneObjectGroup::decode(&body)?;
        assert_eq!(
            group.root.inventory_serial, 4,
            "the contents serial is lost"
        );
        let held = group
            .root
            .task_inventory
            .first()
            .ok_or("the body carries no contents")?;
        assert_eq!(held.name, "a notecard");
        assert_eq!(held.item_id, uuid::Uuid::from_u128(0x00C0_FFEE));
        assert_eq!(
            held.parent_part_id,
            taken.full_id.uuid(),
            "the item does not name the prim holding it"
        );
        Ok(())
    }

    /// A prim the wire never named or textured still reaches the served body
    /// as OpenSim would have constructed it: called `Object`, wearing the
    /// default texture entry.
    ///
    /// The texture half is not cosmetic — OpenSim hands whatever it base64-
    /// decodes straight to `Primitive.TextureEntry`, so an empty block is a
    /// body it cannot read rather than a prim without textures.
    #[test]
    fn a_nameless_untextured_prim_is_filed_with_opensims_own_defaults()
    -> Result<(), Box<dyn core::error::Error>> {
        let owner = agent(0x1);
        let root = box_prim(
            RegionLocalObjectId(7),
            ObjectKey::from(uuid::Uuid::from_u128(7)),
            owner,
            ZERO,
            ZERO,
        );
        let mut child = box_prim(
            RegionLocalObjectId(8),
            ObjectKey::from(uuid::Uuid::from_u128(8)),
            owner,
            ZERO,
            ZERO,
        );
        child.parent_id = root.local_id;
        child.properties = None;
        child.texture_entry = Vec::new();

        let item = taken_item(
            &[root.clone(), child.clone()],
            InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0)),
            owner,
            &|| uuid::Uuid::from_u128(0xDEFA),
            (
                crate::assets::ObjectAssetPolicy::Served,
                crate::imitates::TakenItemMasks::Objects,
                None,
            ),
        );
        let group = filed_group(&[root, child], &[], &item);
        let filed = group.other_parts.first().ok_or("the child was not filed")?;
        assert_eq!(filed.name, DEFAULT_OBJECT_NAME);
        assert_eq!(filed.shape.texture_entry, default_texture_entry());
        Ok(())
    }

    /// A take of an object with no description files it as each grid does:
    /// `(No Description)` on Second Life (`inventory/newer`), empty on
    /// OpenSim; a description of its own is kept on both.
    #[test]
    fn an_undescribed_take_is_described_as_its_grid_describes_it() {
        let owner = agent(0x1);
        let mut prim = box_prim(
            RegionLocalObjectId(7),
            ObjectKey::from(uuid::Uuid::from_u128(7)),
            owner,
            ZERO,
            ZERO,
        );
        let described = |prim: &Object, imitated: crate::imitates::ImitatedGrid| {
            let policy = imitated.rez_policy();
            taken_item(
                std::slice::from_ref(prim),
                InventoryFolderKey::from(uuid::Uuid::from_u128(0xF0)),
                owner,
                &|| uuid::Uuid::from_u128(0xDE5C),
                (
                    crate::assets::ObjectAssetPolicy::Served,
                    policy.taken_item_masks,
                    policy.undescribed_take,
                ),
            )
            .description
        };
        let second_life = crate::imitates::ImitatedGrid::SecondLife;
        let opensim = crate::imitates::ImitatedGrid::OpenSim;
        assert_eq!(described(&prim, second_life), "(No Description)");
        assert_eq!(described(&prim, opensim), "");
        let mut properties = default_object_properties(&prim);
        "a lamp".clone_into(&mut properties.description);
        prim.properties = Some(properties);
        assert_eq!(described(&prim, second_life), "a lamp");
        assert_eq!(described(&prim, opensim), "a lamp");
    }

    fn task_item(task: ObjectKey, item: u128, name: &str) -> TaskInventoryItem {
        TaskInventoryItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(item)),
            parent_task: task,
            permissions: sl_wire::Permissions5 {
                base: sl_wire::Permissions::ALL,
                owner: sl_wire::Permissions::ALL,
                group: sl_wire::Permissions::ALL,
                everyone: sl_wire::Permissions::ALL,
                next_owner: sl_wire::Permissions::ALL,
            },
            creator_id: agent(0x1),
            last_owner_id: agent(0x1),
            owner: OwnerKey::Agent(agent(0x1)),
            group: None,
            group_owned: false,
            asset_id: None,
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            flags: 0,
            sale_type: SaleType::NotForSale,
            sale_price: LindenAmount(0),
            name: name.to_owned(),
            description: String::new(),
            creation_date: FIXTURE_CREATION_DATE,
        }
    }
}
