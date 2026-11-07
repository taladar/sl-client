//! What a script senses of the region (`server-lsl-lib-detection-sensors`):
//! the sweeps of `llSensor` and `llSensorRepeat`, and the hits of
//! `llCastRay`.
//!
//! The host says what is there — every object and avatar, every surface a
//! segment crosses — as plain [`Sensed`] and [`RayHit`] records; which of
//! them a script is told of, in what order and how many, is each grid's rule
//! and lives here (`book/src/gridspec/lsl.md` § Sensors, raycasts and
//! touches has the measurements).

use core::time::Duration;

use sl_types::lsl::{Rotation, Vector};

use crate::value::{NULL_KEY, ZERO_VECTOR};
use crate::vm::detected::{Detected, Touch};
use crate::vm::flavour::EngineFlavour;

/// `AGENT` (`AGENT_BY_LEGACY_NAME`).
pub const AGENT: i32 = 0x1;
/// `ACTIVE`.
pub const ACTIVE: i32 = 0x2;
/// `PASSIVE`.
pub const PASSIVE: i32 = 0x4;
/// `SCRIPTED`.
pub const SCRIPTED: i32 = 0x8;
/// `AGENT_BY_USERNAME`.
pub const AGENT_BY_USERNAME: i32 = 0x10;

/// The farthest a sweep looks, in metres, on both grids (`SensorMaxRange`).
pub const SENSOR_MAX_RANGE: f32 = 96.0;

/// One object or avatar of the region, as a sweep weighs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sensed {
    /// Its key: an avatar's, or an object's root prim's.
    pub key: String,
    /// Its name: an object's, or an avatar's legacy name.
    pub name: String,
    /// An avatar's username (`first.last`, lower case); empty for an object.
    pub username: String,
    /// Its owner: an avatar owns itself.
    pub owner: String,
    /// Its group; `NULL_KEY` for none.
    pub group: String,
    /// Where it is: an object's root's centre, an avatar's position.
    pub position: Vector,
    /// How it is turned.
    pub rotation: Rotation,
    /// How fast it moves.
    pub velocity: Vector,
    /// Whether it is an avatar.
    pub agent: bool,
    /// Whether it is physical.
    pub physical: bool,
    /// Whether a script runs in it — for an avatar, in what it wears.
    pub scripted: bool,
}

/// What a sweep looks for: `llSensor`'s arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// The name to match, or empty for any.
    pub name: String,
    /// The key to match, or `NULL_KEY` for any.
    pub key: String,
    /// The type mask (`AGENT`, `ACTIVE`, `PASSIVE`, `SCRIPTED`,
    /// `AGENT_BY_USERNAME`).
    pub mask: i32,
    /// How far, in metres.
    pub range: f32,
    /// The half angle of the cone about the prim's forward axis, in
    /// radians.
    pub arc: f32,
}

/// A repeating sweep (`llSensorRepeat`), in region time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Repeat {
    /// What it looks for.
    pub(crate) query: Query,
    /// The time between two sweeps.
    pub(crate) interval: Duration,
    /// The region time the next sweep is due at.
    pub(crate) next: Duration,
}

/// Where the sweeping script's prim is: what a sweep is measured from.
#[derive(Debug, Clone, PartialEq)]
pub struct Sweeper {
    /// Its centre.
    pub position: Vector,
    /// How it is turned: its forward axis is its local east.
    pub rotation: Rotation,
    /// The key of its object's root, which a sweep never returns.
    pub root: String,
    /// Its group, for `llDetectedGroup`.
    pub group: String,
}

/// The most results a sweep returns: 32 on Second Life, 16 on OpenSim
/// (`SensorMaxResults`).
const fn cap(flavour: EngineFlavour) -> usize {
    if flavour.is_opensim() { 16 } else { 32 }
}

/// What `llDetectedType` says of `sensed` in `flavour`: an avatar is
/// `AGENT | ACTIVE` on Second Life (and `SCRIPTED` beside, 32, wearing a
/// scripted attachment), `AGENT` on OpenSim; an object `ACTIVE` when physical
/// and `PASSIVE` otherwise, with `SCRIPTED` when a script runs in it — an
/// idle scripted prim reads 12 on both grids.
#[must_use]
pub const fn detected_type(sensed: &Sensed, flavour: EngineFlavour) -> i32 {
    if sensed.agent {
        if flavour.is_opensim() {
            return AGENT;
        }
        return AGENT | ACTIVE | if sensed.scripted { 0x20 } else { 0 };
    }
    let motion = if sensed.physical { ACTIVE } else { PASSIVE };
    motion | if sensed.scripted { SCRIPTED } else { 0 }
}

/// Whether the type mask `mask` admits `sensed` in `flavour`.
///
/// Second Life's `SCRIPTED` narrows: with it, only scripted objects, and of
/// those the ones `ACTIVE`/`PASSIVE` admit — a scripted prim counting as
/// active (idle ones drift between the two on aditi, `llDetectedType` saying
/// 12 throughout; the runtime holds them active). OpenSim's mask is a union,
/// a scripted prim active and never passive.
const fn admits(mask: i32, sensed: &Sensed, flavour: EngineFlavour) -> bool {
    if sensed.agent {
        let by_name = if flavour.is_opensim() {
            AGENT
        } else {
            AGENT | AGENT_BY_USERNAME
        };
        return mask & by_name != 0;
    }
    let active = sensed.physical || sensed.scripted;
    let motion = if active { ACTIVE } else { PASSIVE };
    if flavour.is_opensim() {
        let bits = motion | if sensed.scripted { SCRIPTED } else { 0 };
        return mask & bits != 0;
    }
    if mask & SCRIPTED != 0 && !sensed.scripted {
        return false;
    }
    let wanted = mask & (ACTIVE | PASSIVE);
    if wanted == 0 {
        return mask & SCRIPTED != 0;
    }
    wanted & motion != 0
}

/// Whether `sensed`'s name or key passes `query`'s filters.
fn named(query: &Query, sensed: &Sensed, flavour: EngineFlavour) -> bool {
    if !query.key.is_empty()
        && query.key != NULL_KEY
        && !query.key.eq_ignore_ascii_case(&sensed.key)
    {
        return false;
    }
    if query.name.is_empty() {
        return true;
    }
    if sensed.agent && query.mask & AGENT_BY_USERNAME != 0 && query.mask & AGENT == 0 {
        // OpenSim matches a username to nobody (`detection/sensor`).
        return !flavour.is_opensim() && query.name == sensed.username;
    }
    query.name == sensed.name
}

/// The angle between `a` and `b`, in radians; 0 when either is zero.
fn angle(a: &Vector, b: &Vector) -> f64 {
    let dot = f64::from(a.x).mul_add(
        f64::from(b.x),
        f64::from(a.y).mul_add(f64::from(b.y), f64::from(a.z) * f64::from(b.z)),
    );
    let length = |v: &Vector| {
        f64::from(v.x)
            .mul_add(
                f64::from(v.x),
                f64::from(v.y).mul_add(f64::from(v.y), f64::from(v.z) * f64::from(v.z)),
            )
            .sqrt()
    };
    let product = length(a) * length(b);
    if product <= 0.0 {
        return 0.0;
    }
    (dot / product).clamp(-1.0, 1.0).acos()
}

/// What a sweep answers: the detected block of a `sensor` event, nearest
/// first; an empty one for `no_sensor`; [`None`] for no event at all
/// (Second Life's answer to a range of 0 or less).
#[must_use]
pub fn sweep(
    query: &Query,
    sweeper: &Sweeper,
    candidates: &[Sensed],
    flavour: EngineFlavour,
) -> Option<Vec<Detected>> {
    let opensim = flavour.is_opensim();
    if query.range <= 0.0 || query.range.is_nan() {
        return if opensim { Some(Vec::new()) } else { None };
    }
    let range = f64::from(query.range.min(SENSOR_MAX_RANGE));
    let arc = f64::from(query.arc);
    let whole = if opensim {
        arc >= core::f64::consts::PI
    } else {
        arc <= 0.0 || arc >= core::f64::consts::PI
    };
    if opensim && arc < 0.0 {
        return Some(Vec::new());
    }
    let forward = crate::ops::rotate(
        &Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        },
        &sweeper.rotation,
    );
    let mut found: Vec<(f64, &Sensed)> = candidates
        .iter()
        .filter(|sensed| !sensed.key.eq_ignore_ascii_case(&sweeper.root))
        .filter_map(|sensed| {
            let offset = Vector {
                x: sensed.position.x - sweeper.position.x,
                y: sensed.position.y - sweeper.position.y,
                z: sensed.position.z - sweeper.position.z,
            };
            let distance = f64::from(offset.x)
                .mul_add(
                    f64::from(offset.x),
                    f64::from(offset.y).mul_add(f64::from(offset.y), f64::from(offset.z).powi(2)),
                )
                .sqrt();
            let within = if opensim {
                distance <= range
            } else {
                distance < range
            };
            if !within {
                return None;
            }
            if !whole && distance > 0.0 && angle(&forward, &offset) > arc {
                return None;
            }
            (admits(query.mask, sensed, flavour) && named(query, sensed, flavour))
                .then_some((distance, sensed))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.truncate(cap(flavour));
    Some(
        found
            .into_iter()
            .map(|(_, sensed)| Detected {
                key: sensed.key.clone(),
                name: sensed.name.clone(),
                owner: if sensed.agent {
                    sensed.key.clone()
                } else {
                    sensed.owner.clone()
                },
                kind: detected_type(sensed, flavour),
                same_group: sensed.group == sweeper.group,
                link_number: 0,
                position: sensed.position.clone(),
                rotation: sensed.rotation.clone(),
                velocity: sensed.velocity.clone(),
                grab: ZERO_VECTOR,
                touch: Touch::INVALID,
            })
            .collect(),
    )
}

/// What a segment crosses, as the host reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    /// The ground.
    Land,
    /// An avatar.
    Agent,
    /// A prim.
    Prim {
        /// Whether its object is physical.
        physical: bool,
        /// Whether it is phantom.
        phantom: bool,
    },
}

/// One surface a segment crosses (`llCastRay`), as the host reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct RayHit {
    /// What it is.
    pub kind: HitKind,
    /// The prim's or avatar's key; `NULL_KEY` for land.
    pub key: String,
    /// The key of its object's root; the avatar's, or `NULL_KEY` for land.
    pub root: String,
    /// Its link number in its object: 0 for a prim linked to nothing.
    pub link: i32,
    /// Where the segment crosses it, in region coordinates.
    pub position: Vector,
    /// The surface's normal there, pointing out.
    pub normal: Vector,
    /// Whether the segment starts inside it: the hit is where it leaves.
    pub from_inside: bool,
    /// Whether it is the casting prim itself.
    pub host: bool,
}
