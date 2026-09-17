//! Object flag bits and attachment points.
//!
//! The `ObjectUpdate` flag word says what may be done with an object and how it
//! behaves: whether this agent owns it, may copy, modify or move it, whether it
//! takes physics or is phantom, whether it accepts a dropped inventory item.
//! Every tier tests against these -- the build tool to grey a control, a menu
//! to enable an entry, the world to decide whether to simulate -- so the bits
//! live here rather than in the module that happens to parse the word.

use sl_client_bevy::AttachmentPoint;

/// The `FLAGS_USE_PHYSICS` bit of an object's update flags (`object_flags.h`):
/// the object is simulated by the server's physics engine. This is the "physical
/// object" flag the reference viewer reads (`LLViewerObject::flagUsePhysics`).
pub const FLAGS_USE_PHYSICS: u32 = 1 << 0;

/// The `FLAGS_SERVER_AUTOPILOT` bit of an object's update flags
/// (`object_flags.h`): the update is for an agent the **simulator** is steering
/// — a walk-to-seat, an `llMoveToTarget`-style server autopilot — so the facing it
/// carries is the simulator's to decide, not the viewer's. The one case the
/// reference lets an echoed rotation turn its own agent
/// (`LLViewerObject::processUpdateMessage` → `gAgent.rotate`).
pub const FLAGS_SERVER_AUTOPILOT: u32 = 1 << 24;

/// The agent-relative `FLAGS_OBJECT_MODIFY` bit of `PrimFlags` (`object_flags.h`):
/// this agent may modify the object. The simulator sets it per-agent, folding in
/// the object's owner / group / everyone modify permission.
pub const FLAGS_OBJECT_MODIFY: u32 = 1 << 2;

/// The agent-relative `FLAGS_OBJECT_COPY` bit: this agent may copy the object.
pub const FLAGS_OBJECT_COPY: u32 = 1 << 3;

/// The agent-relative `FLAGS_OBJECT_YOU_OWNER` bit: this agent owns the object.
pub const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// The agent-relative `FLAGS_OBJECT_MOVE` bit: this agent may move (position /
/// rotate) the object — set for the owner and for an "anyone can move" object.
pub const FLAGS_OBJECT_MOVE: u32 = 1 << 8;

/// The `FLAGS_ALLOW_INVENTORY_DROP` bit of `PrimFlags` (`object_flags.h`): the
/// object is set to let **anyone** add inventory to its contents, the reference
/// viewer's `flagAllowInventoryAdd`. Unlike the modify / copy bits this is a
/// property of the object itself (not agent-relative), and it is the one
/// exception to needing modify on the object to drop an item into it.
pub const FLAGS_ALLOW_INVENTORY_DROP: u32 = 1 << 16;

/// The `FLAGS_PHANTOM` bit of `PrimFlags` (`object_flags.h`): the object is
/// non-solid — nothing collides with it. The static collider index
/// (`physics::build_static_colliders`) still gives a phantom prim a
/// collider (so it is in the shared spatial index for proximity queries) but
/// files it in the non-collidable layer.
pub const FLAGS_PHANTOM: u32 = 1 << 10;

/// Whether a raw attachment-point id names a HUD (screen-space) slot rather than
/// a body joint — the reference viewer's `LLVOVolume::isHUDAttachment`, which
/// tests the same `31..=38` id range.
#[must_use]
pub const fn is_hud_point(point_id: u8) -> bool {
    AttachmentPoint::from_code(point_id).is_hud()
}

/// Whether the worn-attachment trace is enabled (`SL_VIEWER_LOG_ATTACHMENT_BIND=1`).
///
/// A worn object crosses three layers before it is drawn — the object ingest
/// tracks it, the avatar layer seats it on its wearer's attachment-point node,
/// and (for a rigged mesh) the skin bind builds it — and an attachment that
/// never appears is silent in all three. The trace is read here, in the crate
/// both layers share, so one environment variable turns on the whole chain
/// rather than each layer inventing its own switch.
#[must_use]
pub fn log_attachment_bind_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_ATTACHMENT_BIND").as_deref() == Ok("1")
}
