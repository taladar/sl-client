//! An avatar's animations: the set a viewer's `AgentAnimation` starts and
//! stops, and the `AvatarAnimation` that states it back.
//!
//! A simulator keeps one set per avatar: a **default** — the posture it puts
//! the avatar in, `stand` for one standing about — and the animations started
//! on top of it, each under the sequence number it was started with. A viewer
//! asks for a change with an `AgentAnimation`, a list of `(animation, start)`
//! pairs; the simulator folds them in and states the whole set again. A stop is
//! an animation dropping out of a later statement.
//!
//! What the two live grids do with a request was measured by the `animations`
//! conformance case (2026-10-19, `book/src/gridspec/avatars.md`
//! § Animations) and is set per flavour in
//! [`ImitatedGrid::animation_policy`](crate::ImitatedGrid::animation_policy).
//! Both grids:
//!
//! - start any id they are given, an asset nobody has included, and add it
//!   after everything already playing under a number above all of theirs;
//! - leave a start of something already playing, a stop of something not
//!   playing and a stop of the nil id (the reference viewer's reset of the
//!   animation-state overrides) changing nothing;
//! - keep the default when it is stopped by hand;
//! - start nothing of their own for typing or for the away control;
//! - never drop a one-shot by themselves: the viewer that plays it says when
//!   it has run out.
//!
//! Where they differ is in [`AnimationPolicy`].

use sl_proto::{AgentKey, AnimationKey, ObjectKey, PlayingAnimation, ServerEvent, SimSession};
use std::time::Instant;
use uuid::Uuid;

/// The built-in **stand** every avatar's set starts from: the `stand` entry of
/// `sl_anim::BUILTIN_ANIMATIONS`.
pub const STAND: Uuid = uuid::uuid!("2408fe9e-df1d-1d7d-f4ff-1384fa7b350f");

/// How a grid answers an `AgentAnimation`, where the live grids disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationPolicy {
    /// Whether a request that changes nothing is answered with a statement of
    /// the unchanged set anyway. OpenSim states the set after every request
    /// (`ScenePresenceAnimator.avnChangeAnim` ends in `SendAnimPack` whatever
    /// the blocks did).
    pub restates_unchanged: bool,
    /// Whether a statement names a source for every entry — the nil id for
    /// the default, the agent for what it started — as Second Life's
    /// statements to the agent itself do. OpenSim names none for what the
    /// agent started (`LLClientView.SendAnimations` lists the objects'
    /// entries only).
    pub names_sources: bool,
    /// Whether a statement carries one empty `PhysicalAvatarEventList` block,
    /// as Second Life's all do. OpenSim's carry none.
    pub empty_physical_block: bool,
}

impl AnimationPolicy {
    /// The `PhysicalAvatarEventList` a statement carries.
    #[must_use]
    pub fn physical_events(self) -> Vec<Vec<u8>> {
        if self.empty_physical_block {
            vec![Vec::new()]
        } else {
            Vec::new()
        }
    }
}

/// One entry of the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    /// The animation.
    anim_id: Uuid,
    /// The sequence number it was started under.
    sequence_id: i32,
}

/// One avatar's animation set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationSet {
    /// The posture the avatar is in.
    default: Entry,
    /// What was started on top of it, in the order it was.
    started: Vec<Entry>,
    /// The sequence number the next start is given.
    next_sequence: i32,
}

impl Default for AnimationSet {
    fn default() -> Self {
        Self::arriving()
    }
}

impl AnimationSet {
    /// The set of an avatar that has just arrived: standing, nothing else.
    #[must_use]
    pub const fn arriving() -> Self {
        Self {
            default: Entry {
                anim_id: STAND,
                sequence_id: 1,
            },
            started: Vec::new(),
            next_sequence: 2,
        }
    }

    /// The set as a statement of `agent`'s avatar states it under `policy`:
    /// the default first, then everything started, in order.
    #[must_use]
    pub fn playing(&self, policy: AnimationPolicy, agent: AgentKey) -> Vec<PlayingAnimation> {
        let source = |id: Uuid| policy.names_sources.then(|| ObjectKey::from(id));
        core::iter::once(PlayingAnimation {
            anim_id: self.default.anim_id,
            sequence_id: self.default.sequence_id,
            source_id: source(Uuid::nil()),
        })
        .chain(self.started.iter().map(|entry| PlayingAnimation {
            anim_id: entry.anim_id,
            sequence_id: entry.sequence_id,
            source_id: source(agent.uuid()),
        }))
        .collect()
    }

    /// Whether `anim_id` is in the set.
    fn holds(&self, anim_id: Uuid) -> bool {
        self.default.anim_id == anim_id || self.started.iter().any(|entry| entry.anim_id == anim_id)
    }

    /// Folds one request's `(animation, start)` pairs in, in order, and returns
    /// whether the set changed.
    ///
    /// A start of something playing, a stop of something not, a stop of the
    /// default and a stop of the nil id change nothing.
    pub fn apply(&mut self, requests: &[(AnimationKey, bool)]) -> bool {
        let mut changed = false;
        for &(animation, start) in requests {
            let anim_id = animation.uuid();
            if anim_id.is_nil() {
                continue;
            }
            if start {
                if self.holds(anim_id) {
                    continue;
                }
                self.started.push(Entry {
                    anim_id,
                    sequence_id: self.next_sequence,
                });
                self.next_sequence = self.next_sequence.saturating_add(1);
                changed = true;
            } else if let Some(index) = self
                .started
                .iter()
                .position(|entry| entry.anim_id == anim_id)
            {
                let _stopped = self.started.remove(index);
                changed = true;
            }
        }
        changed
    }
}

/// Answers a viewer's `AgentAnimation`: folds it into `set` and states the set
/// back to the agent when the grid would. Other sessions are not told: the
/// fake grid shows no resident to another
/// (`server-fake-grid-agent-avatars-shared`). Anything else is not this
/// module's, and `false` says so.
pub(crate) fn answer(
    set: &mut AnimationSet,
    policy: AnimationPolicy,
    agent: AgentKey,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> bool {
    let ServerEvent::AgentAnimation { animations, .. } = event else {
        return false;
    };
    let changed = set.apply(animations);
    if (changed || policy.restates_unchanged)
        && let Err(error) = sim.send_avatar_animation_with_events(
            agent,
            &set.playing(policy, agent),
            &policy.physical_events(),
            now,
        )
    {
        tracing::warn!("stating the agent's animations failed: {error}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{AnimationSet, STAND};
    use pretty_assertions::assert_eq;
    use sl_proto::{AgentKey, AnimationKey, ObjectKey};
    use uuid::Uuid;

    /// An animation id for a test.
    fn animation(n: u128) -> AnimationKey {
        AnimationKey::from(Uuid::from_u128(n))
    }

    /// The set's ids and sequence numbers, in statement order.
    fn stated(set: &AnimationSet) -> Vec<(Uuid, i32)> {
        set.playing(
            crate::ImitatedGrid::OpenSim.animation_policy(),
            AgentKey::from(Uuid::from_u128(0xA)),
        )
        .into_iter()
        .map(|entry| (entry.anim_id, entry.sequence_id))
        .collect()
    }

    #[test]
    fn the_stand_is_stand() -> Result<(), String> {
        let stand = sl_anim::builtin_animation(STAND).ok_or("STAND is no built-in")?;
        assert_eq!(stand.name, "stand");
        Ok(())
    }

    #[test]
    fn a_start_goes_after_the_rest_under_a_higher_number() {
        let mut set = AnimationSet::arriving();
        assert!(set.apply(&[(animation(1), true), (animation(2), true)]));
        assert_eq!(
            stated(&set),
            vec![(STAND, 1), (Uuid::from_u128(1), 2), (Uuid::from_u128(2), 3)]
        );
        // Stopped and started in one message: the order is the message's.
        assert!(set.apply(&[
            (animation(1), false),
            (animation(2), false),
            (animation(3), true)
        ]));
        assert_eq!(stated(&set), vec![(STAND, 1), (Uuid::from_u128(3), 4)]);
    }

    #[test]
    fn second_life_names_a_source_for_every_entry() {
        let agent = AgentKey::from(Uuid::from_u128(0xA));
        let mut set = AnimationSet::arriving();
        assert!(set.apply(&[(animation(1), true)]));
        let sources = |grid: crate::ImitatedGrid| -> Vec<Option<ObjectKey>> {
            set.playing(grid.animation_policy(), agent)
                .into_iter()
                .map(|entry| entry.source_id)
                .collect()
        };
        assert_eq!(
            sources(crate::ImitatedGrid::SecondLife),
            vec![
                Some(ObjectKey::from(Uuid::nil())),
                Some(ObjectKey::from(agent.uuid()))
            ]
        );
        assert_eq!(sources(crate::ImitatedGrid::OpenSim), vec![None, None]);
    }

    #[test]
    fn what_changes_nothing_is_said_to() {
        let mut set = AnimationSet::arriving();
        assert!(set.apply(&[(animation(1), true)]));
        let before = set.clone();
        assert!(!set.apply(&[(animation(1), true)]), "a replay");
        assert!(!set.apply(&[(animation(9), false)]), "a stop of nothing");
        assert!(
            !set.apply(&[(AnimationKey::from(STAND), false)]),
            "the default"
        );
        assert!(
            !set.apply(&[(AnimationKey::from(Uuid::nil()), false)]),
            "the reset"
        );
        assert_eq!(set, before);
    }
}
