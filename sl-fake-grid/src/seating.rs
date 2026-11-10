//! Sitting and wearing as the region's own state: who sits on which prim and
//! where on it, who wears which object on which point, and what the scripts
//! of the object are told of each (`server-world-sit-and-attach`).
//!
//! The facts live in the region's [`ObjectStore`](crate::scene_store::ObjectStore)
//! — a prim's [`Sitters`](crate::scene_store::Sitters) and its
//! [`SeatSettings`](crate::scene_store::SeatSettings), a worn root's
//! [`AttachedTo`](crate::scene_store::AttachedTo), and the store's indices from
//! an avatar to its seat and to what it wears — so a script, the arrival burst
//! and a sit request all read one answer. This module is what changes them:
//!
//! - **Sitting**: [`place`] says where an avatar goes on a prim — the prim's
//!   free sit target raised by
//!   [`SIT_TARGET_RAISE_M`](crate::imitates::SIT_TARGET_RAISE_M), or the
//!   grid's [`PlainSeat`](crate::imitates::PlainSeat) where there is none or
//!   it is taken, stacked by
//!   [`SitPolicy::shared_seat_raise_m`](crate::imitates::SitPolicy::shared_seat_raise_m)
//!   on Second Life — and [`sit`] records it, raising `changed(CHANGED_LINK)`
//!   in the object's scripts. Both grids seat the avatar on the request, so
//!   the region does too, without waiting for the client's `AgentSit`.
//! - **Standing**: [`stand`] takes the avatar off, puts it in front of where
//!   it sat as the grid does
//!   ([`SitPolicy::stand_forward_m`](crate::imitates::SitPolicy::stand_forward_m)),
//!   raises `CHANGED_LINK` and lapses `PERMISSION_TAKE_CONTROLS` and
//!   `PERMISSION_CONTROL_CAMERA` (`avatar/sit`: 1040 held after a stand).
//!   [`stand_all_on`] does it for every sitter of a linkset about to leave.
//! - **Wearing**: [`wear`] puts a root on its owner's point — parented to the
//!   avatar, the point in its state, the same ids kept — and [`drop_off`]
//!   puts it back on the ground where its wearer stands; each tells the
//!   scripts (`attach`, after OpenSim's own `changed(32768)`), and a drop
//!   lapses `PERMISSION_TAKE_CONTROLS`.
//!
//! The inventory half of wearing — the item a wear from the world makes, the
//! item a drop removes, a wear from the inventory, and what a region move
//! carries — is `fake-grid-own-attachments-and-region-moves`'s.

use sl_lsl_runtime::vm::permissions::{PERMISSION_CONTROL_CAMERA, PERMISSION_TAKE_CONTROLS};
use sl_lsl_runtime::vm::seating::CHANGED_LINK;
use sl_proto::{Object, RegionLocalObjectId, SitTransform};
use sl_types::key::AgentKey;
use sl_types::lsl::{Rotation, Vector};

use crate::object_edits::{multiply, rotate};
use crate::scene_store::AttachedTo;
use crate::world::{RegionChange, SceneFixtures};

/// The identity rotation.
const UNTURNED: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// A half turn about the vertical: facing back the way the prim faces.
const HALF_TURN: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 1.0,
    s: 0.0,
};

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// `a` and `b` added.
fn plus(a: &Vector, b: &Vector) -> Vector {
    Vector {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// Whether every component of `vector` is zero.
fn is_zero(vector: &Vector) -> bool {
    vector.x == 0.0 && vector.y == 0.0 && vector.z == 0.0
}

/// Where `prim` stands in the region and how it is turned: a root's own
/// position, a worn root's wearer's, and a child's offset carried by its root.
#[must_use]
pub(crate) fn frame_of(world: &SceneFixtures, prim: &Object) -> (Vector, Rotation) {
    let root_id = world.root_of(prim.local_id);
    let root_frame = |root: &Object| {
        world.objects.attached_to(root.local_id).map_or_else(
            || (root.motion.position.clone(), root.motion.rotation.clone()),
            |attached| (attached.at.clone(), root.motion.rotation.clone()),
        )
    };
    if root_id == prim.local_id {
        return root_frame(prim);
    }
    let Some(root) = world.objects.get(root_id) else {
        return (prim.motion.position.clone(), prim.motion.rotation.clone());
    };
    let (at, turn) = root_frame(root);
    (
        plus(&at, &rotate(&turn, &prim.motion.position)),
        multiply(&turn, &prim.motion.rotation),
    )
}

/// Where `agent` is in the region while it sits, or [`None`] when it sits on
/// nothing here.
#[must_use]
pub(crate) fn seated_position(world: &SceneFixtures, agent: AgentKey) -> Option<Vector> {
    let (seat, sitter) = world.objects.seat_of(agent)?;
    let (at, turn) = frame_of(world, seat);
    Some(plus(&at, &rotate(&turn, &sitter.offset)))
}

/// Where an avatar asking to sit on a prim is put.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    /// From the prim's centre, in its frame: what the avatar's own update
    /// states.
    pub(crate) offset: Vector,
    /// Its turn, in the prim's frame.
    pub(crate) rotation: Rotation,
    /// What the sit response states as the seat's position.
    pub(crate) stated: Vector,
    /// Whether it takes the prim's sit target.
    pub(crate) on_target: bool,
}

/// Where an avatar asking now to sit on `prim` is put: on its sit target
/// when it has one nobody took, raised by
/// [`SIT_TARGET_RAISE_M`](crate::imitates::SIT_TARGET_RAISE_M) along the
/// target's up (`ScenePresence`'s `SIT_TARGET_ADJUSTMENT`), the response
/// stating that on Second Life and the target on OpenSim; otherwise where the
/// grid's [`PlainSeat`](crate::imitates::PlainSeat) puts it, above the
/// avatars already there where the grid stacks them.
#[must_use]
pub(crate) fn place(world: &SceneFixtures, prim: &Object) -> Placed {
    let policy = world.sits;
    let sitters = world.objects.sitters(prim.local_id);
    let target = world
        .objects
        .seat_settings(prim.local_id)
        .and_then(|seat| seat.target.clone())
        .filter(|_target| !sitters.iter().any(|sitter| sitter.on_target));
    if let Some(target) = target {
        let raise = rotate(
            &target.rotation,
            &Vector {
                x: 0.0,
                y: 0.0,
                z: crate::imitates::SIT_TARGET_RAISE_M,
            },
        );
        let offset = plus(&target.offset, &raise);
        let stated = if policy.response_states_seated_position {
            offset.clone()
        } else {
            target.offset.clone()
        };
        return Placed {
            offset,
            rotation: target.rotation,
            stated,
            on_target: true,
        };
    }
    let plain = policy.plain_seat;
    let others = u16::try_from(sitters.len()).unwrap_or(u16::MAX);
    let offset = Vector {
        x: plain.off_centre_m,
        y: 0.0,
        z: plain.above_m + policy.shared_seat_raise_m * f32::from(others),
    };
    let stated = Vector {
        z: offset.z - plain.stated_below_m,
        ..offset.clone()
    };
    Placed {
        offset,
        rotation: if plain.faces_outwards {
            HALF_TURN
        } else {
            UNTURNED
        },
        stated,
        on_target: false,
    }
}

/// Whether a sit on `prim` asked from `at` is answered: from anywhere for a
/// prim whose target is free, and on a grid with a
/// [`plain_reach_m`](crate::imitates::SitPolicy::plain_reach_m) from within it
/// for any other, measured along the ground to the prim's centre.
#[must_use]
pub(crate) fn answers(world: &SceneFixtures, prim: &Object, at: &Vector) -> bool {
    let Some(reach) = world.sits.plain_reach_m else {
        return true;
    };
    if place(world, prim).on_target {
        return true;
    }
    let (centre, _turn) = frame_of(world, prim);
    (at.x - centre.x).hypot(at.y - centre.y) <= reach
}

/// The camera a sitter on `prim` is given, and whether it is put in
/// mouselook: the prim's own offsets, or its root's where the prim set none
/// (`ScenePresence.SendSitResponse`).
fn camera(world: &SceneFixtures, prim: &Object) -> (Vector, Vector, bool) {
    let root = world.root_of(prim.local_id);
    let own = world.objects.seat_settings(prim.local_id);
    let rooted = world.objects.seat_settings(root);
    let pick = |read: fn(&sl_lsl_runtime::vm::seating::Seat) -> &Vector| {
        own.map(read)
            .filter(|offset| !is_zero(offset))
            .or_else(|| rooted.map(read))
            .cloned()
            .unwrap_or(ZERO)
    };
    let eye = pick(|seat| &seat.camera_eye);
    let focus = pick(|seat| &seat.camera_at);
    let mouselook = own.is_some_and(|seat| seat.force_mouselook);
    (eye, focus, mouselook)
}

/// Seats `agent` on `prim` where [`place`] says, standing it up first from
/// wherever it sat, and tells the object's scripts; the response to send it.
/// [`None`] for a prim that is not one of the region's own — an NPC's body,
/// say.
pub(crate) fn sit(
    world: &mut SceneFixtures,
    agent: AgentKey,
    prim: &Object,
) -> Option<SitTransform> {
    if !world.objects.contains(prim.local_id) {
        return None;
    }
    let _earlier = stand(world, agent);
    let placed = place(world, prim);
    let (camera_eye_offset, camera_at_offset, force_mouselook) = camera(world, prim);
    let _sat = world.objects.sit(
        agent,
        prim.local_id,
        (placed.offset.clone(), placed.rotation.clone()),
        placed.on_target,
    )?;
    if let Some(at) = seated_position(world, agent) {
        world.objects.move_wearer(agent, &at);
    }
    let root = world.root_of(prim.local_id);
    crate::scripts::raise_changed(world, root, CHANGED_LINK);
    Some(SitTransform {
        // Set by both grids on every response, whatever the distance.
        autopilot: true,
        sit_position: placed.stated,
        sit_rotation: placed.rotation,
        camera_eye_offset,
        camera_at_offset,
        force_mouselook,
    })
}

/// Stands `agent` up from wherever it sits in the region: where it now
/// stands — in front of where it sat, by the grid's measure, along the way
/// it faced — or [`None`] when it sat on nothing here. The object's scripts
/// are told, and lose the controls and the camera `agent` granted them.
pub(crate) fn stand(world: &mut SceneFixtures, agent: AgentKey) -> Option<Vector> {
    let policy = world.sits;
    let sat = seated_position(world, agent);
    let seat = world
        .objects
        .seat_of(agent)
        .map(|(seat, _sitter)| seat.clone());
    let (local_id, sitter) = world.objects.stand(agent)?;
    let (sat, facing) = match (sat, seat) {
        (Some(sat), Some(seat)) => {
            let (_at, turn) = frame_of(world, &seat);
            (sat, multiply(&turn, &sitter.rotation))
        }
        _gone => (sitter.offset.clone(), sitter.rotation.clone()),
    };
    let ahead = rotate(
        &facing,
        &Vector {
            x: policy.stand_forward_m,
            y: 0.0,
            z: 0.0,
        },
    );
    let stands = Vector {
        x: sat.x + ahead.x,
        y: sat.y + ahead.y,
        z: sat.z + policy.stand_up_m,
    };
    world.objects.move_wearer(agent, &stands);
    let root = world.root_of(local_id);
    crate::scripts::raise_changed(world, root, CHANGED_LINK);
    crate::scripts::lapse(
        world,
        root,
        agent,
        PERMISSION_TAKE_CONTROLS | PERMISSION_CONTROL_CAMERA,
    );
    Some(stands)
}

/// Stands up every avatar sitting on the linkset whose root is `root` — one
/// about to leave the region, or that a script is delinking: what each one's
/// session is to be told.
pub(crate) fn stand_all_on(
    world: &mut SceneFixtures,
    root: RegionLocalObjectId,
) -> Vec<RegionChange> {
    let agents: Vec<AgentKey> = std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .flat_map(|prim| world.objects.sitters(prim).to_vec())
        .map(|sitter| sitter.agent)
        .collect();
    agents
        .into_iter()
        .filter_map(|agent| {
            stand(world, agent).map(|stands_at| RegionChange::Unseated { agent, stands_at })
        })
        .collect()
}

/// `agent` left the region: it is stood up, and what it wore leaves with
/// it, scripts and all. What the region's sessions are to be told.
///
/// The avatar's own `KillObject` goes first, as both live grids send it
/// (`avatar-presence`, 2026-10-08): its seat's sitters and its worn
/// objects' kills follow.
pub(crate) fn leave(world: &mut SceneFixtures, agent: AgentKey) -> Vec<RegionChange> {
    let _stood = stand(world, agent);
    let shown = world.depart_avatar(agent);
    let worn: Vec<RegionChange> = world
        .objects
        .worn_by(agent)
        .into_iter()
        .flat_map(|root| {
            let _off = world.objects.take_off(root);
            crate::scripts::remove_linkset(world, root)
        })
        .collect();
    shown
        .map(|local_id| RegionChange::ResidentLeft { agent, local_id })
        .into_iter()
        .chain(worn)
        .collect()
}

/// The avatars sitting on the linkset whose root is `root`, by key, in the
/// order they sat: the link numbers after its last prim's.
#[must_use]
pub(crate) fn sitters_of(world: &SceneFixtures, root: RegionLocalObjectId) -> Vec<AgentKey> {
    let mut sitters: Vec<crate::scene_store::Sitter> = std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .flat_map(|prim| world.objects.sitters(prim).to_vec())
        .collect();
    sitters.sort_by_key(|sitter| sitter.order);
    sitters.into_iter().map(|sitter| sitter.agent).collect()
}

/// The point an attachment goes on when none was named: the right hand, the
/// reference viewer's default for *Wear*.
const DEFAULT_POINT: u8 = 6;

/// Puts the root `root` on `agent`'s `point` — `0` for the point it was last
/// worn on, or the right hand — turned by `rotation` on it, standing at `at`,
/// worn as `item`: the same object under the same ids, parented to the avatar
/// with the point in its state and its `AttachItemID` among its name-values,
/// as both grids re-send it. Its scripts are told. The records sent, the root
/// first, or [`None`] for a root that is not here or is worn already.
pub(crate) fn wear(
    world: &mut SceneFixtures,
    (agent, at): (AgentKey, &Vector),
    root: RegionLocalObjectId,
    (point, rotation): (u8, Option<Rotation>),
    item: crate::scene_store::WornItem,
) -> Option<Vec<Object>> {
    if world.objects.attached_to(root).is_some() {
        return None;
    }
    let point = match point {
        0 => world
            .objects
            .get(root)
            .and_then(Object::attachment_point_id)
            .unwrap_or(DEFAULT_POINT),
        named => named,
    };
    let avatar = world.avatar_id(agent);
    {
        let mut worn = world.object_mut(root)?;
        worn.parent_id = avatar;
        worn.state = sl_proto::attachment_state_from_point(point);
        worn.motion.position = ZERO;
        if let Some(rotation) = rotation {
            worn.motion.rotation = rotation;
        }
        worn.name_value = crate::attachments::name_values(item, worn.full_id);
    }
    let _worn = world.objects.wear(
        root,
        AttachedTo {
            agent,
            point,
            at: at.clone(),
            item,
            travelled: false,
        },
    );
    crate::scripts::raise_attach(world, root, Some(agent), false);
    Some(linkset_records(world, root))
}

/// The records of the linkset whose root is `root`, the root first.
fn linkset_records(world: &SceneFixtures, root: RegionLocalObjectId) -> Vec<Object> {
    std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .filter_map(|prim| world.objects.get(prim).cloned())
        .collect()
}

/// Puts the worn root `root` of `agent`'s back in the world where its wearer
/// stands, under the same ids; its scripts are told, and lose the controls
/// the wearer granted them. The records, the root first, and what it was
/// worn as, or [`None`] for a root `agent` does not wear.
pub(crate) fn drop_off(
    world: &mut SceneFixtures,
    agent: AgentKey,
    root: RegionLocalObjectId,
) -> Option<(Vec<Object>, crate::scene_store::WornItem)> {
    if world
        .objects
        .attached_to(root)
        .is_none_or(|attached| attached.agent != agent)
    {
        return None;
    }
    let attached = world.objects.take_off(root)?;
    {
        let mut dropped = world.object_mut(root)?;
        dropped.parent_id = RegionLocalObjectId(0);
        dropped.state = 0;
        dropped.motion.position = attached.at;
        dropped.name_value = String::new();
    }
    crate::scripts::raise_attach(world, root, None, attached.travelled);
    crate::scripts::lapse(world, root, agent, PERMISSION_TAKE_CONTROLS);
    Some((linkset_records(world, root), attached.item))
}

/// Whether `object` may be sent to `agent`'s viewer: everything but an
/// attachment of another avatar's that is on its wearer's screen (a HUD,
/// which nobody else ever sees) or whose wearer the region does not show
/// ([`SceneFixtures::resident`]) — a viewer with no avatar to hang it off
/// would wait on a parent that never comes, asking for it again and again.
#[must_use]
pub(crate) fn shown_to(world: &SceneFixtures, object: &Object, agent: AgentKey) -> bool {
    let root = world.root_of(object.local_id);
    world.objects.attached_to(root).is_none_or(|attached| {
        attached.agent == agent
            || (!sl_proto::AttachmentPoint::from_code(attached.point).is_hud()
                && world.resident(attached.agent).is_some())
    })
}
