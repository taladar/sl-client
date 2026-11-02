//! What a script sets of a prim as a seat, and where its object is.
//!
//! A prim is a seat by what its scripts set: a sit target (`llSitTarget`,
//! `llLinkSitTarget`) — the offset and turn an avatar sitting on it is put
//! at — the camera it is seated with (`llSetCameraEyeOffset`,
//! `llSetCameraAtOffset`) and whether its sitter is put in mouselook
//! (`llForceMouselook`). The host keeps a [`Seat`] per prim, which a reset of
//! the script leaves, and reads it when an avatar sits; who sits where is the
//! region's.

use sl_types::lsl::{Rotation, Vector};

/// A prim's sit target: where on it an avatar sitting there is put, and
/// turned how, from the prim's own centre and frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SitTarget {
    /// The offset from the prim's centre, in its frame.
    pub offset: Vector,
    /// The sitter's turn, in the prim's frame.
    pub rotation: Rotation,
}

/// `CHANGED_LINK`: raised in every script of an object when an avatar sits
/// on it or stands up from it, as when a prim is linked or unlinked.
pub const CHANGED_LINK: i32 = 0x20;

/// The zero vector: no camera offset.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// What the scripts of one prim set of it as a seat.
#[derive(Debug, Clone, PartialEq)]
pub struct Seat {
    /// Its sit target, or [`None`] for a prim without one: an avatar is then
    /// put where the region chooses, and `llAvatarOnSitTarget` answers
    /// `NULL_KEY` for it.
    pub target: Option<SitTarget>,
    /// The camera's position for its sitter, from the prim
    /// (`llSetCameraEyeOffset`); zero for none.
    pub camera_eye: Vector,
    /// Where that camera looks, from the prim (`llSetCameraAtOffset`); zero
    /// for none.
    pub camera_at: Vector,
    /// Whether its sitter is put in mouselook (`llForceMouselook`).
    pub force_mouselook: bool,
}

impl Default for Seat {
    fn default() -> Self {
        Self {
            target: None,
            camera_eye: ZERO,
            camera_at: ZERO,
            force_mouselook: false,
        }
    }
}

/// Where the prim a script is in stands, as the placement functions read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Its region position (`llGetPos`): a worn prim's is its wearer's.
    pub position: Vector,
    /// Its position from its parent (`llGetLocalPos`): a root's is its region
    /// position, a worn root's its offset on the attachment point, a child's
    /// its offset from the root.
    pub local: Vector,
    /// Its linkset root's region position (`llGetRootPosition`): a worn
    /// object's is its wearer's.
    pub root: Vector,
}
