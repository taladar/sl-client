//! The shared per-frame budget for handing freshly-decoded **meshes** to Bevy.
//!
//! Inserting a new [`Mesh`] into `Assets<Mesh>` schedules a GPU
//! upload that Bevy performs in the render world's **extract** phase
//! (`extract_render_asset<RenderMesh>`). Extract is the one point where the main
//! and render threads are **serialized** (neither overlaps the other), so an upload
//! burst there stalls the *whole* pipeline, not one thread — a full-region
//! cache-warm login that builds a region's worth of prim / mesh / terrain geometry
//! in a single frame spikes it.
//!
//! Object spawn, mesh geometry, LOD re-tessellation and terrain each used to
//! throttle with their *own* independent per-frame budget, so a single frame could
//! still **stack** all of them and spike anyway. This one resource replaces those
//! counters with a single shared lane every mesh-inserting apply system spends
//! from, so the combined new-mesh count per frame is bounded regardless of how many
//! streams have work — which is what actually caps the serial extract cost (Bevy's
//! extract scales with the *total* meshes handed over, not with which system added
//! them).
//!
//! The image counterpart is the image lane of
//! [`TextureApplyBudget`](crate::textures::TextureApplyBudget), shared the same way
//! across every texture / material-map / bake apply system. Images and meshes are
//! kept in **separate** resources on purpose: an image apply system already
//! serializes on `ResMut<Assets<Image>>` and a mesh one on `ResMut<Assets<Mesh>>`,
//! so a per-kind budget resource adds no scheduling constraint — whereas a single
//! shared resource would force image and mesh systems to serialize against each
//! other for no reason.
//!
//! Backpressure stops here, at the ECS boundary. Network fetch and decode run
//! full-speed and asynchronously off the main thread on a much longer timescale;
//! their decoded results wait in each system's existing parked queue for a frame
//! with budget to spare. Nothing upstream is slowed — only the fast per-frame insert
//! into `Assets<Mesh>` is paced.
//!
//! The lane is drained in **schedule order**: the mesh apply systems run in a fixed
//! order each frame, so an earlier (higher-priority) system claims budget before a
//! later one. This is a deliberate priority choice — a smooth frame rate matters
//! more than showing a distant object a few frames earlier, so a busy high-priority
//! stream is *allowed* to starve a lower-priority one within a frame; the starved
//! work is not lost, it applies on a later frame. (If a stream ever needs a
//! guaranteed floor, reserved per-stream minimums are the follow-up — not built
//! here.)

use bevy::prelude::*;

use crate::textures::env_budget;

/// Default per-frame cap on new [`Mesh`] inserts, shared across
/// object spawn / geometry / LOD / terrain apply. Chosen below the ~40 worst-case
/// sum of the old independent mesh budgets (so stacking can no longer spike) yet at
/// or above the largest single stream (object spawn, 16) so a frame that is busy in
/// only one stream is unaffected. Tune with `SL_VIEWER_MESH_UPLOAD_BUDGET`.
const DEFAULT_MESH_UPLOAD_BUDGET: usize = 24;

/// The shared per-frame mesh-insert lane, refilled each frame by
/// [`reset_mesh_upload_budget`] and spent by every apply system that inserts a
/// freshly-built mesh into `Assets<Mesh>`.
#[derive(Debug, Resource)]
pub struct MeshUploadBudget {
    /// The full per-frame mesh-insert cap, refilled each frame.
    per_frame: usize,
    /// New mesh inserts still allowed this frame; once zero, the rest stay parked.
    ///
    /// Exposed to the crate so the folded mesh-apply systems that already spend a
    /// `remaining` counter (object geometry, sculpts, LOD) migrate onto this shared
    /// lane by only retyping their budget param — their existing
    /// `remaining > 0` / `remaining -= n` arithmetic is unchanged, it now just draws
    /// down the one shared pool in schedule order. Gate a build on
    /// [`has_budget`](Self::has_budget) first, then decrement by the meshes actually
    /// inserted.
    pub(crate) remaining: usize,
}

impl Default for MeshUploadBudget {
    fn default() -> Self {
        let per_frame = env_budget("SL_VIEWER_MESH_UPLOAD_BUDGET", DEFAULT_MESH_UPLOAD_BUDGET);
        Self {
            per_frame,
            remaining: per_frame,
        }
    }
}

impl MeshUploadBudget {
    /// Whether any mesh budget remains this frame. Consumers gate a build on this and
    /// then spend by decrementing [`remaining`](Self::remaining) by the number of
    /// meshes they actually inserted; when it reads zero the build is left parked for
    /// a later frame.
    pub(crate) const fn has_budget(&self) -> bool {
        self.remaining > 0
    }

    /// Refill the lane to its per-frame cap.
    const fn reset(&mut self) {
        self.remaining = self.per_frame;
    }
}

/// Refill the shared [`MeshUploadBudget`] at the start of each frame, before any
/// mesh apply system spends from it. Ordered ahead of every mesh-inserting apply
/// system in the `Update` schedule.
pub fn reset_mesh_upload_budget(mut budget: ResMut<MeshUploadBudget>) {
    budget.reset();
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::MeshUploadBudget;

    /// A budget with an explicit per-frame cap, bypassing the env-var default so the
    /// test is deterministic.
    fn budget(cap: usize) -> MeshUploadBudget {
        MeshUploadBudget {
            per_frame: cap,
            remaining: cap,
        }
    }

    /// Spend one mesh-insert the way a consumer does: gate on the budget, then
    /// decrement. Returns whether it was allowed.
    fn spend(b: &mut MeshUploadBudget) -> bool {
        if b.has_budget() {
            b.remaining = b.remaining.saturating_sub(1);
            true
        } else {
            false
        }
    }

    #[test]
    fn spends_down_to_zero_then_refuses() {
        let mut b = budget(2);
        assert!(b.has_budget());
        assert!(spend(&mut b));
        assert!(spend(&mut b));
        assert!(!b.has_budget());
        assert!(!spend(&mut b), "exhausted lane must refuse");
    }

    #[test]
    fn reset_refills_the_lane() {
        let mut b = budget(3);
        assert!(spend(&mut b));
        assert!(spend(&mut b));
        b.reset();
        assert_eq!(
            (0..).take_while(|_| spend(&mut b)).count(),
            3,
            "reset must restore the full per-frame cap"
        );
    }
}
