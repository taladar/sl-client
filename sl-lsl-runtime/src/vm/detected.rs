//! The detected block: who or what set off a touch, collision or sensor
//! event, as the `llDetected*` functions read it.

use sl_types::lsl::{Rotation, Vector};

use crate::value::{NULL_KEY, ZERO_ROTATION, ZERO_VECTOR};

/// The most detections one event carries — the reference's sixteen.
pub const MAX_DETECTED: usize = 16;

/// The events that carry a detected block. Each has exactly one parameter,
/// `integer` how many were detected, which the runtime fills from the block
/// ([`Engine::post_detected`](crate::vm::Engine::post_detected)).
pub const DETECTION_EVENTS: [&str; 7] = [
    "collision",
    "collision_end",
    "collision_start",
    "sensor",
    "touch",
    "touch_end",
    "touch_start",
];

/// Whether `event` carries a detected block.
#[must_use]
pub fn is_detection_event(event: &str) -> bool {
    DETECTION_EVENTS.contains(&event)
}

/// One detected avatar or object, as the host saw it when it raised the
/// event. The runtime stores it and answers `llDetected*` from it; it never
/// looks anything up.
#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    /// `llDetectedKey`: the avatar's or object's key.
    pub key: String,
    /// `llDetectedName`.
    pub name: String,
    /// `llDetectedOwner`: an avatar owns itself.
    pub owner: String,
    /// `llDetectedType`: `AGENT`, `ACTIVE`, `PASSIVE` and `SCRIPTED` bits.
    pub kind: i32,
    /// `llDetectedGroup`: whether it has the same active group as the
    /// scripted object.
    pub same_group: bool,
    /// `llDetectedLinkNumber`: which link of the scripted object was touched
    /// or collided with.
    pub link_number: i32,
    /// `llDetectedPos`, region coordinates.
    pub position: Vector,
    /// `llDetectedRot`.
    pub rotation: Rotation,
    /// `llDetectedVel`.
    pub velocity: Vector,
    /// `llDetectedGrab`: the grab offset, in `touch` events.
    pub grab: Vector,
    /// Where on the object a touch landed; [`Touch::INVALID`] for anything
    /// that is not a touch.
    pub touch: Touch,
}

/// Where a touch landed, `llDetectedTouch*`.
#[derive(Debug, Clone, PartialEq)]
pub struct Touch {
    /// `llDetectedTouchFace`; `TOUCH_INVALID_FACE` (-1) when unknown.
    pub face: i32,
    /// `llDetectedTouchST`: the surface coordinates.
    pub st: Vector,
    /// `llDetectedTouchUV`: the texture coordinates.
    pub uv: Vector,
    /// `llDetectedTouchPos`: the region position touched.
    pub position: Vector,
    /// `llDetectedTouchNormal`.
    pub normal: Vector,
    /// `llDetectedTouchBinormal`.
    pub binormal: Vector,
}

/// `TOUCH_INVALID_TEXCOORD`, `<-1, -1, 0>`.
const INVALID_TEXCOORD: Vector = Vector {
    x: -1.0,
    y: -1.0,
    z: 0.0,
};

impl Touch {
    /// No touch information: `TOUCH_INVALID_FACE`, `TOUCH_INVALID_TEXCOORD`
    /// and `TOUCH_INVALID_VECTOR`, what the reference reports for a
    /// collision, a sensor, or a viewer that sent no surface information.
    pub const INVALID: Self = Self {
        face: -1,
        st: INVALID_TEXCOORD,
        uv: INVALID_TEXCOORD,
        position: ZERO_VECTOR,
        normal: ZERO_VECTOR,
        binormal: ZERO_VECTOR,
    };
}

impl Detected {
    /// A detection with every field at its "nothing" value — keys
    /// `NULL_KEY`, numbers zero, no touch — for a host to fill in.
    #[must_use]
    pub fn blank() -> Self {
        Self {
            key: NULL_KEY.to_owned(),
            name: String::new(),
            owner: NULL_KEY.to_owned(),
            kind: 0,
            same_group: false,
            link_number: 0,
            position: ZERO_VECTOR,
            rotation: ZERO_ROTATION,
            velocity: ZERO_VECTOR,
            grab: ZERO_VECTOR,
            touch: Touch::INVALID,
        }
    }
}
