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
//! - keep the default when it is stopped by hand, but drop it when a script
//!   stops it (`llStopAnimation("sit")` on a seated avatar leaves nothing
//!   playing until the posture changes: `avatar/sit-animation`, 2026-11-12,
//!   on OpenSim, after `ScenePresenceAnimator.RemoveAnimation(id, true)`);
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
    /// Whether a statement to the agent itself names a source for every
    /// entry — the nil id for the default, the agent for what it started, the
    /// prim for what a script started — as Second Life's do. OpenSim names
    /// the objects' entries only, and lists them first
    /// (`LLClientView.SendAnimations`).
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

/// One `AvatarAnimation` statement of an avatar's set: to its own viewer
/// ([`AnimationSet::own`]) or to the region's others
/// ([`AnimationSet::observed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The entries, in statement order, their own sources unread.
    pub animations: Vec<PlayingAnimation>,
    /// The `AnimationSourceList`, block for block.
    pub sources: Vec<ObjectKey>,
    /// The `PhysicalAvatarEventList`, block for block.
    pub physical_events: Vec<Vec<u8>>,
}

impl Statement {
    /// Sends this statement of `agent`'s set down `sim`.
    ///
    /// # Errors
    ///
    /// As [`SimSession::send_avatar_animation_statement`].
    pub fn send(
        &self,
        agent: AgentKey,
        sim: &mut SimSession,
        now: Instant,
    ) -> Result<(), sl_proto::Error> {
        sim.send_avatar_animation_statement(
            agent,
            &self.animations,
            &self.sources,
            &self.physical_events,
            now,
        )
    }
}

/// One entry of the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    /// The animation.
    anim_id: Uuid,
    /// The sequence number it was started under.
    sequence_id: i32,
    /// The prim whose script started it; [`None`] for the default and for
    /// what the agent's viewer started.
    object: Option<ObjectKey>,
}

/// One avatar's animation set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationSet {
    /// The posture the avatar is in; [`None`] once a script stopped it.
    default: Option<Entry>,
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
            default: Some(Entry {
                anim_id: STAND,
                sequence_id: 1,
                object: None,
            }),
            started: Vec::new(),
            next_sequence: 2,
        }
    }

    /// Every entry with its source, in the set's order: the default first,
    /// sourced from the nil id, then everything started, sourced from the
    /// prim whose script started it or else from `agent`.
    fn sourced(&self, agent: AgentKey) -> impl Iterator<Item = (Entry, ObjectKey)> + '_ {
        self.default
            .iter()
            .map(|entry| (*entry, ObjectKey::from(Uuid::nil())))
            .chain(self.started.iter().map(move |entry| {
                (
                    *entry,
                    entry
                        .object
                        .unwrap_or_else(|| ObjectKey::from(agent.uuid())),
                )
            }))
    }

    /// The set, in its order, each entry with its source: what a script reads
    /// of `agent`'s avatar.
    #[must_use]
    pub fn playing(&self, agent: AgentKey) -> Vec<PlayingAnimation> {
        self.sourced(agent)
            .map(|(entry, source)| PlayingAnimation {
                anim_id: entry.anim_id,
                sequence_id: entry.sequence_id,
                source_id: Some(source),
            })
            .collect()
    }

    /// The set as a statement to `agent`'s own viewer states it under
    /// `policy`.
    ///
    /// Second Life lists the set in order and names every entry's source: the
    /// nil id for the default, the agent for what its viewer started, the
    /// prim for what a script started (`dance1@probe` in `avatar/animation`
    /// and `sit@probe` in `avatar/sit-animation`, aditi). OpenSim lists the
    /// entries an object started first, names their sources alone, then the
    /// rest (`LLClientView.SendAnimations`; the same two probes on both its
    /// engines).
    #[must_use]
    pub fn own(&self, policy: AnimationPolicy, agent: AgentKey) -> Statement {
        let entry = |(entry, _source): &(Entry, ObjectKey)| PlayingAnimation {
            anim_id: entry.anim_id,
            sequence_id: entry.sequence_id,
            source_id: None,
        };
        let sourced: Vec<(Entry, ObjectKey)> = self.sourced(agent).collect();
        let (animations, sources) = if policy.names_sources {
            (
                sourced.iter().map(entry).collect(),
                sourced.iter().map(|(_entry, source)| *source).collect(),
            )
        } else {
            let (objects, rest): (Vec<_>, Vec<_>) = sourced
                .into_iter()
                .partition(|(entry, _source)| entry.object.is_some());
            (
                objects.iter().chain(&rest).map(entry).collect(),
                objects.iter().map(|(_entry, source)| *source).collect(),
            )
        };
        Statement {
            animations,
            sources,
            physical_events: policy.physical_events(),
        }
    }

    /// The set as the region's **other** viewers are told it under `policy`:
    /// the same entries under the same sequence numbers as its own viewer is
    /// told, with the sources and the physical block of a statement to
    /// somebody else.
    ///
    /// Second Life names only the first entry's source, and that one nil, and
    /// carries one empty `PhysicalAvatarEventList` block; OpenSim names none
    /// and carries none (`animations`, 2026-10-19,
    /// `book/src/gridspec/avatars.md` § Animations). The source list is
    /// positional, so Second Life's one block is the default's. Neither names
    /// the prim of what a script started (`avatar/animation`'s `of=other`
    /// steps, aditi and OpenSim).
    #[must_use]
    pub fn observed(&self, policy: AnimationPolicy, agent: AgentKey) -> Statement {
        Statement {
            animations: self
                .sourced(agent)
                .map(|(entry, _source)| PlayingAnimation {
                    anim_id: entry.anim_id,
                    sequence_id: entry.sequence_id,
                    source_id: None,
                })
                .collect(),
            // The grid that names sources to the agent itself names the
            // default's alone to anybody else.
            sources: if policy.names_sources && self.default.is_some() {
                vec![ObjectKey::from(Uuid::nil())]
            } else {
                Vec::new()
            },
            physical_events: policy.physical_events(),
        }
    }

    /// Puts the avatar in the posture `anim_id`: the set's default, kept
    /// under its sequence number so a statement lists one locomotion state at
    /// a time. Returns whether the set changed.
    pub fn set_default(&mut self, anim_id: Uuid) -> bool {
        match &mut self.default {
            Some(entry) if entry.anim_id == anim_id => false,
            Some(entry) => {
                entry.anim_id = anim_id;
                true
            }
            None => {
                self.default = Some(Entry {
                    anim_id,
                    sequence_id: 1,
                    object: None,
                });
                true
            }
        }
    }

    /// Whether `anim_id` is in the set.
    fn holds(&self, anim_id: Uuid) -> bool {
        self.default.is_some_and(|entry| entry.anim_id == anim_id)
            || self.started.iter().any(|entry| entry.anim_id == anim_id)
    }

    /// Folds in an `llStartAnimation` or `llStopAnimation` of `anim_id` by a
    /// script in the prim `object` and returns whether the set changed: as a
    /// viewer's request, but a stop of the default drops it, and a start is
    /// the prim's (OpenSim's `LSL_Api` adds it under `m_host.UUID`).
    pub fn apply_scripted(&mut self, anim_id: Uuid, start: bool, object: ObjectKey) -> bool {
        if !start && self.default.is_some_and(|entry| entry.anim_id == anim_id) {
            self.default = None;
            return true;
        }
        self.fold(AnimationKey::from(anim_id), start, Some(object))
    }

    /// Folds one request's `(animation, start)` pairs in, in order, and returns
    /// whether the set changed.
    ///
    /// A start of something playing, a stop of something not, a stop of the
    /// default and a stop of the nil id change nothing.
    pub fn apply(&mut self, requests: &[(AnimationKey, bool)]) -> bool {
        let mut changed = false;
        for &(animation, start) in requests {
            changed |= self.fold(animation, start, None);
        }
        changed
    }

    /// Folds one start or stop of `animation` in, a start as `object`'s
    /// ([`None`] for the agent's viewer), and returns whether the set
    /// changed. A start of something playing keeps whose it was; a stop
    /// stops it whoever started it.
    fn fold(&mut self, animation: AnimationKey, start: bool, object: Option<ObjectKey>) -> bool {
        let anim_id = animation.uuid();
        if anim_id.is_nil() {
            return false;
        }
        if start {
            if self.holds(anim_id) {
                return false;
            }
            self.started.push(Entry {
                anim_id,
                sequence_id: self.next_sequence,
                object,
            });
            self.next_sequence = self.next_sequence.saturating_add(1);
            true
        } else if let Some(index) = self
            .started
            .iter()
            .position(|entry| entry.anim_id == anim_id)
        {
            let _stopped = self.started.remove(index);
            true
        } else {
            false
        }
    }
}

/// Answers a viewer's `AgentAnimation`: folds it into `set` and states the set
/// back to the agent when the grid would. What the region's other viewers are
/// to be told, the same statement in their shape, is returned for the caller
/// to publish; [`None`] when the agent was told nothing — every statement the
/// agent is sent, the others are sent too — or the event is not this module's.
pub(crate) fn answer(
    set: &mut AnimationSet,
    policy: AnimationPolicy,
    agent: AgentKey,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Statement> {
    let ServerEvent::AgentAnimation { animations, .. } = event else {
        return None;
    };
    let changed = set.apply(animations);
    if !changed && !policy.restates_unchanged {
        return None;
    }
    if let Err(error) = set.own(policy, agent).send(agent, sim, now) {
        tracing::warn!("stating the agent's animations failed: {error}");
    }
    Some(set.observed(policy, agent))
}

#[cfg(test)]
mod tests {
    use super::{AnimationSet, STAND, Statement};
    use pretty_assertions::assert_eq;
    use sl_proto::{AgentKey, AnimationKey, ObjectKey};
    use uuid::Uuid;

    /// The prim a test's script is in.
    fn prim() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0xB))
    }

    /// An animation id for a test.
    fn animation(n: u128) -> AnimationKey {
        AnimationKey::from(Uuid::from_u128(n))
    }

    /// The set's ids and sequence numbers, in statement order.
    fn stated(set: &AnimationSet) -> Vec<(Uuid, i32)> {
        set.playing(AgentKey::from(Uuid::from_u128(0xA)))
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

    /// A statement's ids and sequence numbers, in its order, and its sources.
    fn listed(statement: &Statement) -> (Vec<(Uuid, i32)>, Vec<ObjectKey>) {
        (
            statement
                .animations
                .iter()
                .map(|entry| (entry.anim_id, entry.sequence_id))
                .collect(),
            statement.sources.clone(),
        )
    }

    #[test]
    fn the_own_viewer_is_told_whose_each_entry_is_as_its_grid_says() {
        let agent = AgentKey::from(Uuid::from_u128(0xA));
        let nil = ObjectKey::from(Uuid::nil());
        let mut set = AnimationSet::arriving();
        assert!(set.apply(&[(animation(1), true)]));
        assert!(set.apply_scripted(Uuid::from_u128(2), true, prim()));
        let own = |grid: crate::ImitatedGrid| listed(&set.own(grid.animation_policy(), agent));
        // Second Life: in order, a source for every entry.
        assert_eq!(
            own(crate::ImitatedGrid::SecondLife),
            (
                vec![(STAND, 1), (Uuid::from_u128(1), 2), (Uuid::from_u128(2), 3)],
                vec![nil, ObjectKey::from(agent.uuid()), prim()]
            )
        );
        // OpenSim: the object's entries first, and their sources alone.
        assert_eq!(
            own(crate::ImitatedGrid::OpenSim),
            (
                vec![(Uuid::from_u128(2), 3), (STAND, 1), (Uuid::from_u128(1), 2)],
                vec![prim()]
            )
        );
        // Nobody else is told the prim: Second Life names the default's nil,
        // OpenSim nothing, both in the set's order.
        let observed =
            |grid: crate::ImitatedGrid| listed(&set.observed(grid.animation_policy(), agent));
        let order = vec![(STAND, 1), (Uuid::from_u128(1), 2), (Uuid::from_u128(2), 3)];
        assert_eq!(
            observed(crate::ImitatedGrid::SecondLife),
            (order.clone(), vec![nil])
        );
        assert_eq!(observed(crate::ImitatedGrid::OpenSim), (order, vec![]));
    }

    #[test]
    fn a_start_of_what_plays_keeps_whose_it_was() {
        let agent = AgentKey::from(Uuid::from_u128(0xA));
        let mut set = AnimationSet::arriving();
        assert!(set.apply(&[(animation(1), true)]));
        assert!(!set.apply_scripted(Uuid::from_u128(1), true, prim()));
        assert!(set.apply_scripted(Uuid::from_u128(2), true, prim()));
        assert!(!set.apply(&[(animation(2), true)]));
        let sources: Vec<Option<ObjectKey>> = set
            .playing(agent)
            .into_iter()
            .map(|entry| entry.source_id)
            .collect();
        assert_eq!(
            sources,
            vec![
                Some(ObjectKey::from(Uuid::nil())),
                Some(ObjectKey::from(agent.uuid())),
                Some(prim())
            ]
        );
        // A viewer's stop stops the script's start too.
        assert!(set.apply(&[(animation(2), false)]));
        assert_eq!(stated(&set), vec![(STAND, 1), (Uuid::from_u128(1), 2)]);
    }

    #[test]
    fn a_script_stops_the_default_until_the_posture_changes() {
        let sit = Uuid::from_u128(5);
        let mut set = AnimationSet::arriving();
        assert!(set.set_default(sit));
        assert!(
            set.apply_scripted(sit, false, prim()),
            "the seat's script stops sit"
        );
        assert_eq!(stated(&set), vec![]);
        assert!(
            set.apply_scripted(sit, true, prim()),
            "and starts it as its own"
        );
        assert_eq!(stated(&set), vec![(sit, 2)]);
        assert!(set.apply_scripted(sit, false, prim()));
        assert!(
            set.apply_scripted(Uuid::from_u128(6), true, prim()),
            "the pose"
        );
        assert!(set.set_default(STAND), "the stand");
        assert_eq!(stated(&set), vec![(STAND, 1), (Uuid::from_u128(6), 3)]);
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
