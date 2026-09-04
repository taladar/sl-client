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

use std::time::Instant;

use crate::fixtures::{NpcAppearance, NpcFixture};
use crate::terrain::TerrainFixture;
use sl_proto::{
    AnimationKey, GlobalCoordinates, Object, ObjectExtraParams, ObjectMotion,
    ObjectPlayingAnimation, ParcelCategory, ParcelDetails, ParcelInfo, ParcelRequestResult,
    ParcelStatus, PrimShapeParams, RegionIdentity, RegionLocalObjectId, RegionLocalParcelId,
    ServerEvent, SimSession, TerrainLayerType, pcode,
};
use sl_types::key::{AgentKey, ObjectKey, OwnerKey, ParcelKey};
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
const REAL_TIME_DILATION: u16 = 0xFFFF;
/// The sequence id of an unsolicited agent-parcel push (what OpenSim's
/// `SendLandUpdateToClient` sends).
const UNSOLICITED_SEQUENCE_ID: i32 = 0;

/// The parcels and objects of one region, pushed at every arriving agent
/// and replayed on request.
#[derive(Debug, Clone, Default)]
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
    /// from the login).
    pub objects: Vec<Object>,
    /// The other avatars the region shows — scripted, because the fake grid
    /// has no inter-session broadcast. Each contributes its avatar body, an
    /// `AvatarAppearance`, an `AvatarAnimation` and its attachments to the
    /// arrival burst, and answers object refetches like any other object.
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
    /// The region-local id the arriving agent's own avatar object gets. A
    /// real simulator mints one per avatar; with one agent per session a
    /// fixed id is enough, but it must not collide with [`objects`].
    ///
    /// [`objects`]: Self::objects
    pub avatar_local_id: RegionLocalObjectId,
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
    /// Derived rather than stated so the two records cannot disagree about
    /// the parcel they both describe — the drift a live grid cannot have,
    /// because both come out of its one land record.
    #[must_use]
    pub fn details(&self, parcel: &ParcelInfo, region: &RegionIdentity) -> ParcelDetails {
        let (global_x, global_y) = region.region_handle.global_coordinates();
        ParcelDetails {
            parcel_id: self.parcel_id,
            owner_id: parcel.owner.uuid(),
            name: parcel.name.clone(),
            description: parcel.description.clone(),
            actual_area: parcel.area,
            billable_area: parcel.area,
            // The condensed byte a listing carries is not the full parcel
            // bitfield: the reference viewer reads only the mature/adult bit
            // out of it, and a listing of a general-rated parcel is zero.
            flags: 0,
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

impl SceneFixtures {
    /// No parcels, no objects, no NPCs.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            parcels: Vec::new(),
            listings: Vec::new(),
            objects: Vec::new(),
            npcs: Vec::new(),
            object_animations: Vec::new(),
            avatar_local_id: RegionLocalObjectId(1),
        }
    }

    /// Adds one parcel with both its halves: the region-local record a
    /// `ParcelProperties` reply carries, and the [`ParcelListing`] naming its
    /// grid-wide id and dwell.
    ///
    /// The one call every fixture makes, so no parcel is ever pushed without
    /// the identity three of its request surfaces need.
    pub fn add_parcel(&mut self, parcel: ParcelInfo, parcel_id: ParcelKey, dwell: f32) {
        self.listings.push(ParcelListing {
            local_id: parcel.local_id,
            parcel_id,
            dwell,
        });
        self.parcels.push(parcel);
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
        let mut objects = self.objects.clone();
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

    /// The parcel with the given region-local id.
    #[must_use]
    pub fn parcel_by_local_id(&self, local_id: RegionLocalParcelId) -> Option<&ParcelInfo> {
        self.parcels
            .iter()
            .find(|parcel| parcel.local_id == local_id)
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
        parcel_environment_version: 0,
        region_allow_environment_override: false,
        see_avs: None,
        any_av_sounds: None,
        group_av_sounds: None,
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

/// Where a seat puts an avatar: this far above the seat's own centre, and
/// facing the way the seat faces.
///
/// The fake grid's stand-in for `llSitTarget`, and the reason a sit here
/// ignores the point the client clicked. A real vehicle sets a sit target, so
/// the avatar snaps to the seat rather than to wherever the pointer landed,
/// and a fixture that answered the click offset would seat two riders in the
/// same place only by luck. It is the offset the scripted riders already use
/// ([`SEATED_NPC_SIT_OFFSET_Z`](crate::fixtures::catalogue::SEATED_NPC_SIT_OFFSET_Z)).
pub const SIT_TARGET_OFFSET: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.55,
};

/// The skeleton every fixture object shares: a root object with no sound,
/// text, media, particles, or extra params; the caller sets the identity,
/// geometry and motion.
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

/// A plain box prim of `scale` metres resting at `position`, owned by
/// `owner` — the simplest object a viewer can render.
#[must_use]
pub fn box_prim(
    local_id: RegionLocalObjectId,
    full_id: ObjectKey,
    owner: AgentKey,
    position: Vector,
    scale: Vector,
) -> Object {
    let mut object = bare_object(local_id, full_id, owner, position);
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

/// An avatar object for `identity` standing at `position`: the `LEGACY_AVATAR`
/// pcode, the agent id as the full id, and the `FirstName` / `LastName` /
/// `Title` name-values a simulator attaches so the viewer can label it.
///
/// This is the avatar body every fake-grid avatar is rezzed as — the arriving
/// agent's own (from the login identity) and every
/// [`NpcFixture`]'s.
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
pub(crate) fn push_arrival_world(
    world: &SceneFixtures,
    terrain: &TerrainFixture,
    identity: &AvatarIdentity,
    assets: &crate::assets::GridAssets,
    sim: &mut SimSession,
    now: Instant,
) {
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
    let avatar = avatar_prim(world.avatar_local_id, identity, arrival.clone());
    if let Err(error) = sim.send_object_update(&[avatar], REAL_TIME_DILATION, now) {
        tracing::warn!("rezzing the arriving avatar failed: {error}");
    }
    push_own_appearance(identity, assets, sim, now);
    push_own_animation(identity, sim, now);
    if let Err(error) = sim.send_parcel_overlay(&world.overlay_for(identity.agent_id), now) {
        tracing::warn!("sending the parcel overlay failed: {error}");
    }
    if let Some(parcel) = world.parcel_at(arrival.x, arrival.y) {
        let mut record = parcel.clone();
        record.sequence_id = UNSOLICITED_SEQUENCE_ID;
        if let Err(error) = sim.send_parcel_properties(&record, now) {
            tracing::warn!("pushing the agent's parcel failed: {error}");
        }
    }
    push_terrain(terrain, sim, now);
    if !world.objects.is_empty()
        && let Err(error) = sim.send_object_update(&world.objects, REAL_TIME_DILATION, now)
    {
        tracing::warn!("rezzing the fixture objects failed: {error}");
    }
    push_npcs(&world.npcs, sim, now);
    push_object_animations(&world.object_animations, sim, now);
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
pub(crate) fn push_child_world(
    world: &SceneFixtures,
    terrain: &TerrainFixture,
    identity: &sl_proto::RegionIdentity,
    sim: &mut SimSession,
    now: Instant,
) {
    if !world.objects.is_empty()
        && let Err(error) = sim.send_object_update(&world.objects, REAL_TIME_DILATION, now)
    {
        tracing::warn!("rezzing a neighbour's objects failed: {error}");
    }
    push_npcs(&world.npcs, sim, now);
    push_object_animations(&world.object_animations, sim, now);
    push_terrain(terrain, sim, now);
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

/// The built-in **stand** animation every avatar in a fake region plays: the
/// `stand` entry of [`sl_anim::BUILTIN_ANIMATIONS`], which is the animation a
/// real simulator puts an idle avatar into.
const STAND_ANIMATION: uuid::Uuid = uuid::uuid!("2408fe9e-df1d-1d7d-f4ff-1384fa7b350f");

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
/// A simulator sends an agent its own appearance like anyone else's, and a
/// viewer that never receives one has no visual params and no texture entry
/// for itself: it spawns the avatar, poses its skeleton, and draws no body at
/// all — a name tag hanging in mid-air. The fake grid runs no bake service, so
/// it bakes the arriving agent exactly the way it bakes an NPC: one solid per
/// body region under ids derived from the agent's own id, whose bytes go into
/// *this session's* asset store, because the agent id is only known now.
fn push_own_appearance(
    identity: &AvatarIdentity,
    assets: &crate::assets::GridAssets,
    sim: &mut SimSession,
    now: Instant,
) {
    let appearance = NpcAppearance::solid(identity.agent_id, OWN_AVATAR_BAKE_COLOR);
    {
        let mut store = assets.write();
        for (key, bytes) in appearance.bake_assets() {
            let _previous = store.insert(key, bytes);
        }
    }
    if let Err(error) =
        sim.send_avatar_appearance(&appearance.record(identity.agent_id, Vec::new()), now)
    {
        tracing::warn!("sending the arriving agent's own appearance failed: {error}");
    }
}

/// Pushes the arriving agent the animation it is playing: the built-in
/// **stand**.
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
fn push_own_animation(identity: &AvatarIdentity, sim: &mut SimSession, now: Instant) {
    let playing = vec![sl_proto::PlayingAnimation {
        anim_id: STAND_ANIMATION,
        sequence_id: 1,
        source_id: None,
    }];
    if let Err(error) = sim.send_avatar_animation(identity.agent_id, &playing, now) {
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
fn push_npcs(npcs: &[NpcFixture], sim: &mut SimSession, now: Instant) {
    if npcs.is_empty() {
        return;
    }
    let bodies: Vec<Object> = npcs.iter().map(NpcFixture::avatar_prim).collect();
    if let Err(error) = sim.send_object_update(&bodies, REAL_TIME_DILATION, now) {
        tracing::warn!("rezzing the NPC avatars failed: {error}");
    }
    for npc in npcs {
        if let Err(error) = sim.send_avatar_appearance(&npc.appearance_record(), now) {
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
        && let Err(error) = sim.send_object_update(&attachments, REAL_TIME_DILATION, now)
    {
        tracing::warn!("rezzing the NPC attachments failed: {error}");
    }
}

/// Streams the region's ground: the LAND layer as the spiral of patches a
/// simulator sends on region entry, then the WIND and CLOUD layers the
/// fixture carries (each one message). Send failures are logged.
fn push_terrain(terrain: &TerrainFixture, sim: &mut SimSession, now: Instant) {
    let handle = sim.region_handle();
    if let Err(error) = sim.send_terrain(&terrain.to_patches(handle), now) {
        tracing::warn!("streaming the region's ground failed: {error}");
    }
    let wind = terrain.wind_patches(handle);
    if !wind.is_empty()
        && let Err(error) = sim.send_layer_data(TerrainLayerType::Wind, &wind, now)
    {
        tracing::warn!("sending the wind layer failed: {error}");
    }
    let clouds = terrain.cloud_patches(handle);
    if !clouds.is_empty()
        && let Err(error) = sim.send_layer_data(TerrainLayerType::Cloud, &clouds, now)
    {
        tracing::warn!("sending the cloud layer failed: {error}");
    }
}

/// Answers one drained [`ServerEvent`] from the world fixtures, under the
/// session lock: a parcel request (by rectangle or id) gets the covering
/// parcel's record with the request's sequence id echoed — or a
/// [`ParcelRequestResult::NoData`] reply on a miss, as a simulator says
/// "no such parcel" — a dwell or search-listing request gets the parcel's
/// [`ParcelListing`] half, and an object refetch gets a full `ObjectUpdate` of
/// the matching fixtures (an unknown id is silently dropped, as a simulator
/// drops a stale refetch).
pub(crate) fn answer_world_request(
    world: &SceneFixtures,
    identity: &AvatarIdentity,
    region: &RegionIdentity,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) {
    match event {
        // The agent asked to sit on something. A simulator answers a sit on an
        // object it has with an `AvatarSitResponse` and simply does not answer
        // one it does not (the client's own sit timeout recovers that), so an
        // unknown target is dropped rather than refused.
        ServerEvent::SitRequested { target, offset } => {
            let Some(seat) = world
                .all_objects()
                .into_iter()
                .find(|object| object.full_id == *target)
            else {
                tracing::debug!("a sit was requested on {target}, which this region does not have");
                return;
            };
            // The seat has a sit target, so the click point is ignored — see
            // [`SIT_TARGET_OFFSET`]. The `offset` the client sent is where it
            // touched the object, which a scripted seat never honours either.
            let _clicked = offset;
            let transform = sl_proto::SitTransform {
                autopilot: false,
                sit_position: SIT_TARGET_OFFSET,
                sit_rotation: UNROTATED,
                camera_eye_offset: ZERO,
                camera_at_offset: ZERO,
                force_mouselook: false,
            };
            if let Err(error) = sim.send_avatar_sit_response(seat.full_id, &transform, now) {
                tracing::warn!("answering a sit request failed: {error}");
            }
        }
        // The handshake completed: the avatar is on the seat, which on the wire
        // means its object update carries the seat's **region-local** id as its
        // `ParentID` and a position that is the offset from the seat rather
        // than a region position. That is the one case where an avatar's
        // position is not region-local, and re-sending the body is how every
        // client learns it (`NpcFixture::seated_on` says the same thing
        // statically for a scripted rider).
        ServerEvent::SitConfirmed { on: Some(seat) } => {
            let Some(parent) = world
                .all_objects()
                .into_iter()
                .find(|object| object.full_id == *seat)
            else {
                return;
            };
            push_seated_avatar(
                world,
                identity,
                parent.local_id,
                SIT_TARGET_OFFSET,
                sim,
                now,
            );
        }
        // Standing up puts the avatar back in the region's own frame, at the
        // place the driver last knew it.
        ServerEvent::StoodUp => {
            let placement = sim.arrival_position().position;
            let avatar = avatar_prim(
                world.avatar_local_id,
                identity,
                Vector {
                    x: placement.x(),
                    y: placement.y(),
                    z: placement.z(),
                },
            );
            if let Err(error) = sim.send_object_update(&[avatar], REAL_TIME_DILATION, now) {
                tracing::warn!("standing the agent up failed: {error}");
            }
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
            let record = world.parcel_at(x, y).map(|parcel| {
                let mut record = parcel.clone();
                record.sequence_id = *sequence_id;
                record.snap_selection = *snap_selection;
                record
            });
            send_parcel_or_no_data(sim, record, *sequence_id, now);
        }
        ServerEvent::RequestParcelPropertiesById {
            local_id,
            sequence_id,
        } => {
            let record = world.parcel_by_local_id(*local_id).map(|parcel| {
                let mut record = parcel.clone();
                record.sequence_id = *sequence_id;
                record
            });
            send_parcel_or_no_data(sim, record, *sequence_id, now);
        }
        // The parcel's traffic score. A parcel with no grid-wide listing is
        // left unanswered rather than answered with a zero dwell: "no such
        // parcel" and "nobody has been here" are different facts, and the
        // client's own timeout tells them apart.
        ServerEvent::RequestParcelDwell { local_id } => {
            let Some(listing) = world.listing_by_local_id(*local_id) else {
                tracing::debug!("a dwell was requested for parcel {local_id:?}, which is not here");
                return;
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
        ServerEvent::RequestParcelInfo { parcel_id } => {
            let Some((listing, parcel)) = world.listing_by_parcel_id(*parcel_id) else {
                tracing::debug!(
                    "a listing was requested for parcel {parcel_id}, which is not here"
                );
                return;
            };
            if let Err(error) = sim.send_parcel_info_reply(&listing.details(parcel, region), now) {
                tracing::warn!("answering a parcel info request failed: {error}");
            }
        }
        ServerEvent::RequestObjects { objects } => {
            let matching: Vec<Object> = world
                .all_objects()
                .into_iter()
                .filter(|object| objects.iter().any(|(id, _)| *id == object.local_id))
                .collect();
            if !matching.is_empty()
                && let Err(error) = sim.send_object_update(&matching, REAL_TIME_DILATION, now)
            {
                tracing::warn!("answering an object refetch failed: {error}");
            }
        }
        _other => {}
    }
}

/// Rezzes the agent's own avatar **seated**: parented to `seat` (a
/// region-local id) at `offset` from it, which is how a seated avatar travels
/// on the wire.
pub(crate) fn push_seated_avatar(
    world: &SceneFixtures,
    identity: &AvatarIdentity,
    seat: RegionLocalObjectId,
    offset: Vector,
    sim: &mut SimSession,
    now: Instant,
) {
    let mut avatar = avatar_prim(world.avatar_local_id, identity, offset);
    avatar.parent_id = seat;
    if let Err(error) = sim.send_object_update(&[avatar], REAL_TIME_DILATION, now) {
        tracing::warn!("seating the agent's avatar failed: {error}");
    }
}

/// Sends `record`, or the "no such parcel" reply (an empty record whose
/// `RequestResult` is [`ParcelRequestResult::NoData`]) when there is none.
fn send_parcel_or_no_data(
    sim: &mut SimSession,
    record: Option<ParcelInfo>,
    sequence_id: i32,
    now: Instant,
) {
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
    if let Err(error) = sim.send_parcel_properties(&record, now) {
        tracing::warn!("answering a parcel request failed: {error}");
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A fixed agent for the overlay tests.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(uuid::Uuid::from_u128(id))
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
            region_flags: 0,
            region_flags_extended: 0,
            region_protocols: 0,
            maturity: sl_proto::Maturity::Pg,
            product: sl_proto::ProductType::FullRegion,
            product_sku: String::new(),
            product_name: name.to_owned(),
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
        let details = listing.details(parcel, &region);
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
            Some(STAND_ANIMATION)
        );
    }
}
