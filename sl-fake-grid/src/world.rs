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

use sl_proto::{
    Object, ObjectExtraParams, ObjectMotion, ParcelCategory, ParcelInfo, ParcelRequestResult,
    ParcelStatus, PrimShapeParams, RegionLocalObjectId, RegionLocalParcelId, ServerEvent,
    SimSession, pcode,
};
use sl_types::key::{AgentKey, ObjectKey, OwnerKey};
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
    /// The objects rezzed in the region, sent in one full `ObjectUpdate` on
    /// arrival (after the agent's own avatar object, which the driver adds
    /// from the login).
    pub objects: Vec<Object>,
    /// The region-local id the arriving agent's own avatar object gets. A
    /// real simulator mints one per avatar; with one agent per session a
    /// fixed id is enough, but it must not collide with [`objects`].
    ///
    /// [`objects`]: Self::objects
    pub avatar_local_id: RegionLocalObjectId,
}

impl SceneFixtures {
    /// No parcels, no objects.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            parcels: Vec::new(),
            objects: Vec::new(),
            avatar_local_id: RegionLocalObjectId(1),
        }
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

/// The identity the agent's own avatar object is rezzed with.
#[derive(Debug, Clone)]
pub(crate) struct AvatarIdentity {
    /// The agent id (the avatar object's full id).
    pub(crate) agent_id: AgentKey,
    /// The account's first name.
    pub(crate) first_name: String,
    /// The account's last name.
    pub(crate) last_name: String,
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
#[must_use]
pub(crate) fn avatar_object(
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

/// Pushes the arrival burst at the agent: its own avatar object, the parcel
/// overlay, the parcel it stands on, and every fixture object. Send
/// failures are logged, never fatal.
pub(crate) fn push_arrival_world(
    world: &SceneFixtures,
    identity: &AvatarIdentity,
    sim: &mut SimSession,
    now: Instant,
) {
    let arrival = sl_proto::Camera::region_center().center;
    let avatar = avatar_object(world.avatar_local_id, identity, arrival.clone());
    if let Err(error) = sim.send_object_update(&[avatar], REAL_TIME_DILATION, now) {
        tracing::warn!("rezzing the arriving avatar failed: {error}");
    }
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
    if !world.objects.is_empty()
        && let Err(error) = sim.send_object_update(&world.objects, REAL_TIME_DILATION, now)
    {
        tracing::warn!("rezzing the fixture objects failed: {error}");
    }
}

/// Answers one drained [`ServerEvent`] from the world fixtures, under the
/// session lock: a parcel request (by rectangle or id) gets the covering
/// parcel's record with the request's sequence id echoed — or a
/// [`ParcelRequestResult::NoData`] reply on a miss, as a simulator says
/// "no such parcel" — and an object refetch gets a full `ObjectUpdate` of
/// the matching fixtures (an unknown id is silently dropped, as a simulator
/// drops a stale refetch).
pub(crate) fn answer_world_request(
    world: &SceneFixtures,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) {
    match event {
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
        ServerEvent::RequestObjects { objects } => {
            let matching: Vec<Object> = world
                .objects
                .iter()
                .filter(|object| objects.iter().any(|(id, _)| *id == object.local_id))
                .cloned()
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
    fn avatar_object_carries_the_name_values() {
        let identity = AvatarIdentity {
            agent_id: agent(9),
            first_name: "Test".to_owned(),
            last_name: "User".to_owned(),
        };
        let avatar = avatar_object(RegionLocalObjectId(1), &identity, ZERO);
        assert_eq!(avatar.pcode, pcode::AVATAR);
        assert_eq!(avatar.full_id.uuid(), agent(9).uuid());
        assert!(
            avatar
                .name_value
                .starts_with("FirstName STRING RW SV Test\n")
        );
        assert!(avatar.name_value.contains("LastName STRING RW SV User\n"));
    }
}
