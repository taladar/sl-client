//! Ordering phases.

use bevy::prelude::*;

/// The points in a frame that parts of the world order themselves against.
///
/// A system that must run once the objects have been folded in, or once the
/// avatars have, would otherwise have to name the system that does it -- and
/// naming a system across a boundary is a dependency on the code that produces
/// the world, not on the world it produced. These sets are the vocabulary for
/// that ordering, so the constraint can be stated without the reference.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldPhase {
    /// This frame's `ObjectUpdate` batch has been folded into the object store
    /// and its entities spawned, moved or despawned.
    ObjectsUpdated,
    /// This frame's avatar updates -- full objects and coarse locations alike --
    /// have been folded into the avatar store.
    AvatarsUpdated,
    /// The third-person camera has consumed this frame's orbit input.
    CameraOrbited,
    /// This frame's playing animations have been sampled onto every avatar's
    /// skeleton, so the set each avatar is playing is settled.
    AvatarSkeletonsDriven,
    /// This frame's avatar appearance rebuild is done: every rigged body has
    /// been re-shaped from its visual params and its baked region materials
    /// settled.
    AvatarAppearanceApplied,
    /// This frame's per-avatar runtime morph params (eye blink, body physics,
    /// hand pose) have been folded into the mesh morph weights.
    AvatarMorphsFolded,
    /// This frame's movement intent has been folded into the own avatar's
    /// controls and advertised to the simulator.
    ///
    /// The client-side locomotion animations live in the object layer but must
    /// read the intent the *view* layer just advertised; this set is how that
    /// constraint is stated without the object layer naming a system above it.
    AvatarControlsDriven,
    /// This frame's drag-time world pick has been folded into
    /// `sl_viewer_intents::DragWorldPick`, so what the cursor is over in the
    /// world is settled
    /// for whoever started the drag.
    ///
    /// The panel that starts an inventory drag lives well below the picker that
    /// resolves it; this set is how it orders its own drop / hover work after
    /// the answer without naming the system that produces it.
    DragPickResolved,
    /// The camera's final pose for this frame has been written.
    ///
    /// Everything that faces, follows or centres on the viewpoint — the sky
    /// dome and its discs, clouds and stars, the ocean, the particle
    /// billboards, the local-light budget, the underwater-fog matrix — orders
    /// itself against this. It is what lets those live in the scene layer,
    /// *below* the camera that drives them, without naming the system that
    /// positions it.
    CameraPositioned,
}
