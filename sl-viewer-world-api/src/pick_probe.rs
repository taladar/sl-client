//! **Pick probes**: "what would a click at this pixel hit?", asked of the
//! viewer's own pick resolver without clicking.
//!
//! The automation layer aims a world click by asking exactly that for a few
//! candidate points on the object, and clicks only a point the resolver says
//! lands on it — so an object behind another is reported covered instead of
//! the click going to whatever is in front. The resolver that answers is the
//! one every click goes through: the GPU ID-buffer pick in a rendering viewer,
//! the CPU ray-cast double in a headless fixture world
//! (`sl_viewer_world_view::gpu_pick`). The automation layer must not depend on
//! that crate, so the two meet here: the asker queues points in
//! [`PickProbes`], the resolver's side takes them one a frame and files the
//! answers back.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, PrimFaceId, ScopedObjectId};

/// Names one probe, so its answer can be told from the others'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProbeId(u64);

/// What a probe's pixel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeTarget {
    /// An avatar's body, or a rigged attachment it wears (`worn`).
    Avatar {
        /// The avatar.
        agent: AgentKey,
        /// The worn object of a rigged attachment submesh; `None` for the body.
        worn: Option<ScopedObjectId>,
    },
    /// A face of an in-world object or a rigid attachment.
    Object {
        /// The prim the face belongs to.
        scoped: ScopedObjectId,
        /// The face's index.
        face: PrimFaceId,
    },
    /// Bare land.
    Ground,
    /// A water surface.
    Water,
}

/// A probe that hit something.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeHit {
    /// What was hit.
    pub target: ProbeTarget,
    /// Where, Bevy world space.
    pub world_point: Vec3,
}

/// The probe queue and its answers.
///
/// The asker [`request`](Self::request)s a point and polls
/// [`take_answer`](Self::take_answer) each frame; the resolver's side submits
/// one queued point a frame ([`next_to_submit`](Self::next_to_submit)) and
/// files what its pick found ([`answer`](Self::answer)). Answers come back two
/// or three frames after the submission on the GPU, one on the CPU; probes
/// submitted on consecutive frames are in flight together.
#[derive(Resource, Debug, Default)]
pub struct PickProbes {
    /// The last id handed out.
    last: u64,
    /// Asked for, not yet submitted, oldest first.
    queued: VecDeque<(ProbeId, Vec2)>,
    /// Submitted, not yet answered, oldest first.
    in_flight: VecDeque<(ProbeId, Vec2)>,
    /// Answered, not yet taken.
    answers: HashMap<ProbeId, Option<ProbeHit>>,
    /// Given up by the asker while in flight: their answers are dropped.
    abandoned: HashSet<ProbeId>,
}

impl PickProbes {
    /// Ask what a click at `at` (logical pixels, the primary window) would hit.
    pub fn request(&mut self, at: Vec2) -> ProbeId {
        self.last = self.last.wrapping_add(1);
        let id = ProbeId(self.last);
        self.queued.push_back((id, at));
        id
    }

    /// The answer to `id`, once it has come: `Some(None)` for a pick that hit
    /// nothing. Taking it forgets it.
    pub fn take_answer(&mut self, id: ProbeId) -> Option<Option<ProbeHit>> {
        self.answers.remove(&id)
    }

    /// Give up on `id`: it is not submitted if it has not been, and its answer
    /// is dropped if it is still in flight.
    pub fn abandon(&mut self, id: ProbeId) {
        let before = self.queued.len();
        self.queued.retain(|(queued, _at)| *queued != id);
        if self.queued.len() == before
            && self.answers.remove(&id).is_none()
            && self.in_flight.iter().any(|(flying, _at)| *flying == id)
        {
            let _new = self.abandoned.insert(id);
        }
    }

    /// Whether nothing is queued or in flight.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.queued.is_empty() && self.in_flight.is_empty()
    }

    /// The resolver's side: the oldest queued point, now in flight. One a
    /// frame, since a frame's picks share one pixel.
    pub fn next_to_submit(&mut self) -> Option<Vec2> {
        let (id, at) = self.queued.pop_front()?;
        self.in_flight.push_back((id, at));
        Some(at)
    }

    /// The resolver's side: a pick at `at` found `hit`. Answers the oldest
    /// in-flight probe at that point; an answer nobody is waiting for is
    /// ignored.
    pub fn answer(&mut self, at: Vec2, hit: Option<ProbeHit>) {
        let Some(position) = self.in_flight.iter().position(|(_id, probe)| *probe == at) else {
            return;
        };
        let Some((id, _at)) = self.in_flight.remove(position) else {
            return;
        };
        if !self.abandoned.remove(&id) {
            let _previous = self.answers.insert(id, hit);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{PickProbes, ProbeHit, ProbeTarget};

    /// A ground hit at the origin.
    const GROUND: ProbeHit = ProbeHit {
        target: ProbeTarget::Ground,
        world_point: Vec3::ZERO,
    };

    #[test]
    fn probes_are_submitted_one_at_a_time_and_answered_by_point() {
        let mut probes = PickProbes::default();
        let first = probes.request(Vec2::new(1.0, 1.0));
        let second = probes.request(Vec2::new(2.0, 2.0));
        assert_eq!(probes.next_to_submit(), Some(Vec2::new(1.0, 1.0)));
        assert_eq!(probes.next_to_submit(), Some(Vec2::new(2.0, 2.0)));
        assert_eq!(probes.next_to_submit(), None);
        // The second answer arrives first: matched by point, not by order.
        probes.answer(Vec2::new(2.0, 2.0), None);
        probes.answer(Vec2::new(1.0, 1.0), Some(GROUND));
        assert_eq!(probes.take_answer(first), Some(Some(GROUND)));
        assert_eq!(probes.take_answer(second), Some(None));
        assert_eq!(probes.take_answer(second), None, "an answer is taken once");
        assert!(probes.is_idle());
    }

    #[test]
    fn an_abandoned_probe_is_never_answered() {
        let mut probes = PickProbes::default();
        let queued = probes.request(Vec2::new(1.0, 1.0));
        let flying = probes.request(Vec2::new(2.0, 2.0));
        probes.abandon(queued);
        assert_eq!(probes.next_to_submit(), Some(Vec2::new(2.0, 2.0)));
        probes.abandon(flying);
        probes.answer(Vec2::new(2.0, 2.0), Some(GROUND));
        assert_eq!(probes.take_answer(flying), None);
        assert!(probes.is_idle());
    }
}
