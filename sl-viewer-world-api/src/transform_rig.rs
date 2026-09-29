//! **Manipulator plans**: "where do I press on this build handle, and along
//! which path do I drag, to move / turn / stretch the selection by this
//! much?", answered by the build tool's own manipulators.
//!
//! The automation layer drags a transform handle the way a user does, through
//! the real pointer, and must not reason about the handles' geometry itself:
//! which pixel is on a handle is the manipulator's own hit test, and what a
//! pointer path does to the selection is its own drag math — including the
//! **snap regime**, which engages only once the pointer strays past the snap
//! guide (off the axis, off the stretch line, outside the ring's tick
//! circle). So the automation layer asks here, and `sl-viewer-edit`'s gizmos
//! answer from the rig as it stands, by starting a drag on paper at each
//! candidate press and inverting its math.

use bevy::prelude::*;

use crate::probe_queue::ProbeQueue;

/// One of a manipulator's three axes, in the frame the rig is drawn in (the
/// grid frame for move and rotate, the object's own for stretch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManipulatorAxis {
    /// The frame's X axis (red).
    X,
    /// The frame's Y axis (green).
    Y,
    /// The frame's Z axis (blue).
    Z,
}

impl ManipulatorAxis {
    /// The axis' lower-case letter, as in the handles' test addresses.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}

/// One handle of the build tool's transform rig. Which exist depends on the
/// effective tool: the move rig has the arrows and pads, the rotate rig (also
/// under a held `Ctrl`) the rings, the stretch rig (also under `Ctrl+Shift`)
/// the face and corner cubes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManipulatorHandle {
    /// A move arrow along an axis.
    Translate(ManipulatorAxis),
    /// A move pad in the plane whose normal is the axis.
    TranslatePlane(ManipulatorAxis),
    /// A rotate ring about an axis.
    Rotate(ManipulatorAxis),
    /// A stretch face cube on an axis; `true` is the positive side.
    StretchFace(ManipulatorAxis, bool),
    /// A stretch corner cube; each `bool` is that axis' sign (x, y, z).
    StretchCorner([bool; 3]),
}

impl ManipulatorHandle {
    /// The handle's test address suffix — its entity is named
    /// `edit-gizmo:<slug>`.
    #[must_use]
    pub fn slug(self) -> String {
        match self {
            Self::Translate(axis) => format!("translate-{}", axis.slug()),
            Self::TranslatePlane(axis) => format!("translate-plane-{}", axis.slug()),
            Self::Rotate(axis) => format!("rotate-{}", axis.slug()),
            Self::StretchFace(axis, positive) => {
                let side = if positive { "pos" } else { "neg" };
                format!("scale-face-{}-{side}", axis.slug())
            }
            Self::StretchCorner([x, y, z]) => {
                let sign = |positive: bool| if positive { 'p' } else { 'n' };
                format!("scale-corner-{}{}{}", sign(x), sign(y), sign(z))
            }
        }
    }
}

/// How much a drag is to change the selection. Each handle takes one kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ManipulatorAmount {
    /// Metres: along a move arrow, or the change of the primary object's
    /// extent on a stretch face.
    Distance(f32),
    /// Metres along the two other frame axes of a move pad, in x → y → z
    /// order (the Z pad moves along x and y).
    Offset([f32; 2]),
    /// Radians about a rotate ring's axis, right-handed.
    Angle(f32),
    /// The uniform scale factor of a stretch corner.
    Factor(f32),
}

/// Which side of the snap guide the drag ends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapRegime {
    /// On the handle's line (or inside the ring's tick circle): the amount is
    /// applied as asked, even with snapping on.
    Free,
    /// Past the snap guide, with snapping on: the result lands on the grid
    /// (move and stretch) or a detent (rotate) nearest the amount.
    Grid,
}

/// A request for a [`ManipulatorPlan`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManipulatorQuery {
    /// The handle to drag.
    pub handle: ManipulatorHandle,
    /// How much.
    pub amount: ManipulatorAmount,
    /// Which side of the snap guide.
    pub regime: SnapRegime,
}

/// A drag the manipulator says does what was asked.
#[derive(Debug, Clone, PartialEq)]
pub struct ManipulatorPlan {
    /// Where to press, logical pixels: a point its own hit test puts on the
    /// handle.
    pub press: Vec2,
    /// The pointer's path after the press, logical pixels; the last point is
    /// where to release. A rotation goes round the ring in short steps, since
    /// a ring accumulates the angle it is dragged through.
    pub path: Vec<Vec2>,
    /// What the drag will do: the amount asked, or — in the grid regime — the
    /// grid mark or detent it lands on instead, in the same unit.
    pub predicted: ManipulatorAmount,
}

/// Why no plan can be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManipulatorRefusal {
    /// The rig has no such handle now (another tool, no selection, not in
    /// build mode, or a rig still being rebuilt).
    NoHandle,
    /// No pixel of the handle is on screen and hit by its own hit test.
    Unreachable,
    /// The amount is not the kind this handle takes.
    WrongAmount,
    /// The grid regime was asked with snapping off.
    SnappingOff,
    /// A move pad always snaps while snapping is on; the free regime cannot
    /// be had there.
    PadSnaps,
    /// The drag geometry is degenerate from this view (a plane seen edge-on).
    Degenerate,
    /// The drag would take the pointer out of the window, where its last
    /// movement is never seen: frame the selection closer, or ask for less.
    OutOfView,
}

impl ManipulatorRefusal {
    /// A sentence for an error message.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::NoHandle => "the rig has no such handle",
            Self::Unreachable => "no pixel of the handle is on screen and hit by its hit test",
            Self::WrongAmount => "the amount is not the kind this handle takes",
            Self::SnappingOff => "the grid regime needs snapping on",
            Self::PadSnaps => "a move pad always snaps while snapping is on",
            Self::Degenerate => "the drag geometry is degenerate from this view",
            Self::OutOfView => "the drag would take the pointer out of the window",
        }
    }
}

/// The candidate plans for one request, best first: one per press point the
/// handle's hit test accepts. Several, so the asker can pass over a press that
/// something else (a floater) would take.
pub type ManipulatorAnswer = Result<Vec<ManipulatorPlan>, ManipulatorRefusal>;

/// The manipulator plan queue: the automation layer asks, and the build tool
/// answers every waiting request in the frame it next runs — which is only
/// while build mode is on.
pub type ManipulatorProbes = ProbeQueue<ManipulatorQuery, ManipulatorAnswer>;

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        ManipulatorAmount, ManipulatorAxis, ManipulatorHandle, ManipulatorProbes, ManipulatorQuery,
        ManipulatorRefusal, SnapRegime,
    };

    #[test]
    fn slugs_are_the_handles_test_addresses() {
        assert_eq!(
            ManipulatorHandle::Translate(ManipulatorAxis::X).slug(),
            "translate-x"
        );
        assert_eq!(
            ManipulatorHandle::TranslatePlane(ManipulatorAxis::Z).slug(),
            "translate-plane-z"
        );
        assert_eq!(
            ManipulatorHandle::Rotate(ManipulatorAxis::Y).slug(),
            "rotate-y"
        );
        assert_eq!(
            ManipulatorHandle::StretchFace(ManipulatorAxis::X, false).slug(),
            "scale-face-x-neg"
        );
        assert_eq!(
            ManipulatorHandle::StretchCorner([true, false, true]).slug(),
            "scale-corner-pnp"
        );
    }

    #[test]
    fn a_request_is_answered_once() {
        let mut probes = ManipulatorProbes::default();
        let id = probes.request(ManipulatorQuery {
            handle: ManipulatorHandle::Rotate(ManipulatorAxis::Z),
            amount: ManipulatorAmount::Angle(1.0),
            regime: SnapRegime::Free,
        });
        assert!(probes.has_requests());
        let taken = probes.take_requests();
        assert_eq!(taken.len(), 1);
        assert!(!probes.has_requests());
        probes.answer(id, Err(ManipulatorRefusal::NoHandle));
        assert_eq!(
            probes.take_answer(id),
            Some(Err(ManipulatorRefusal::NoHandle))
        );
        assert_eq!(probes.take_answer(id), None);
    }
}
