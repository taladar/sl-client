//! Source cap and priority eviction.
//!
//! SL scenes routinely ask for more simultaneous sounds than any device wants to
//! render, so the mixer caps the number of concurrent voices and evicts the
//! least important one when a more important sound arrives. This module is the
//! pure policy: it scores sounds ([`SoundPriority`]) and, given a fixed budget,
//! decides whether a newcomer is admitted, admitted by evicting a weaker voice,
//! or rejected ([`VoicePool`]). The mixer owns the real firewheel voice handles
//! and applies the decision.

/// How important a sound is, independent of how loud or close it is. Higher
/// tiers always outrank lower ones regardless of distance.
///
/// The ordering matters: attached and UI sounds should survive a flood of
/// distant one-shots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Importance {
    /// Ambient / environment beds — the first to go under pressure.
    Ambient,
    /// A fire-and-forget one-shot (`llTriggerSound`, a collision tick).
    OneShot,
    /// A looped world sound.
    Looped,
    /// A sound attached to an object or avatar (follows it, tends to be
    /// deliberate and close).
    Attached,
    /// The viewer's own UI feedback — always wanted, never spatial.
    Ui,
}

impl Importance {
    /// The base score contributed by the tier. Spaced far enough apart that a
    /// higher tier always beats a lower one before distance / loudness tiebreak.
    const fn base(self) -> f32 {
        match self {
            Self::Ambient => 0.0,
            Self::OneShot => 100.0,
            Self::Looped => 200.0,
            Self::Attached => 300.0,
            Self::Ui => 400.0,
        }
    }
}

/// The priority of a single sound, from which a keep/evict [`score`] is derived.
///
/// [`score`]: SoundPriority::score
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundPriority {
    /// The sound's tier.
    pub importance: Importance,
    /// The effective linear gain in `[0.0, 1.0]` (louder sounds are kept).
    pub gain: f32,
    /// The listener→source distance in metres (nearer sounds are kept). Use
    /// `0.0` for non-spatial (2-D) sounds so they score at full proximity.
    pub distance: f32,
}

impl SoundPriority {
    /// A non-spatial UI sound at full gain — the highest ordinary priority.
    #[must_use]
    pub const fn ui() -> Self {
        Self {
            importance: Importance::Ui,
            gain: 1.0,
            distance: 0.0,
        }
    }

    /// The keep/evict score: higher survives. Tier dominates; within a tier a
    /// louder, nearer sound wins.
    ///
    /// Distance contributes a bounded proximity term in `[0.0, 1.0]` (`1.0` at
    /// the listener, decaying with distance) so it never overpowers the tier
    /// spacing, and gain contributes up to `1.0`.
    #[must_use]
    pub fn score(&self) -> f32 {
        // Proximity: 1 / (1 + d/10) — 1.0 at the ear, 0.5 at 10m, ->0 far away.
        let proximity = (1.0 + self.distance.max(0.0) / 10.0).recip();
        self.importance.base() + self.gain.clamp(0.0, 1.0) + proximity
    }
}

/// The decision for a sound that wants to start playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision<Id> {
    /// Play it; the pool had spare capacity.
    Admit,
    /// Play it, but first stop this weaker voice to make room.
    Evict(Id),
    /// Do not play it; the pool is full of higher-priority voices.
    Reject,
}

/// A fixed-capacity set of active voices, each carrying a keep/evict score.
///
/// The mixer calls [`consider`](VoicePool::consider) before creating a firewheel
/// voice, applies the returned [`Decision`], then records the new voice with
/// [`insert`](VoicePool::insert). Finished voices are dropped with
/// [`remove`](VoicePool::remove); moving sources update their score with
/// [`reprioritize`](VoicePool::reprioritize).
#[derive(Debug, Clone)]
pub struct VoicePool<Id> {
    /// The maximum number of concurrent voices.
    capacity: usize,
    /// The active voices as `(id, score)`, unordered.
    voices: Vec<(Id, f32)>,
}

impl<Id: Copy + PartialEq> VoicePool<Id> {
    /// Create an empty pool that holds at most `capacity` voices (at least one).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            voices: Vec::new(),
        }
    }

    /// The maximum number of concurrent voices.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of active voices.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.voices.len()
    }

    /// Whether the pool holds no voices.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Whether the pool is at capacity.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.voices.len() >= self.capacity
    }

    /// The active voice with the lowest score, if any.
    #[must_use]
    pub fn weakest(&self) -> Option<(Id, f32)> {
        self.voices
            .iter()
            .copied()
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Decide what to do with a sound of the given `score` without mutating the
    /// pool. If the pool is full, the newcomer must *strictly* outrank the
    /// weakest active voice to evict it (equal scores are rejected, which avoids
    /// churn between equally-ranked sounds).
    #[must_use]
    pub fn consider(&self, score: f32) -> Decision<Id> {
        if !self.is_full() {
            return Decision::Admit;
        }
        match self.weakest() {
            Some((id, weakest)) if score > weakest => Decision::Evict(id),
            _ => Decision::Reject,
        }
    }

    /// Record a newly-started voice. If this would exceed capacity the weakest
    /// voice is dropped from the bookkeeping first (the caller is expected to
    /// have already stopped it per [`consider`](VoicePool::consider)).
    pub fn insert(&mut self, id: Id, score: f32) {
        if self.is_full()
            && let Some((weakest, _)) = self.weakest()
        {
            self.remove(weakest);
        }
        self.voices.push((id, score));
    }

    /// Remove a voice (e.g. it finished or was evicted). Returns whether it was
    /// present.
    pub fn remove(&mut self, id: Id) -> bool {
        if let Some(pos) = self.voices.iter().position(|(vid, _)| *vid == id) {
            self.voices.swap_remove(pos);
            true
        } else {
            false
        }
    }

    /// Update the score of an active voice (its source moved, or its gain
    /// changed). No-op if the voice is not present.
    pub fn reprioritize(&mut self, id: Id, score: f32) {
        if let Some(entry) = self.voices.iter_mut().find(|(vid, _)| *vid == id) {
            entry.1 = score;
        }
    }

    /// Iterate the active voice ids.
    pub fn ids(&self) -> impl Iterator<Item = Id> + '_ {
        self.voices.iter().map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn tier_dominates_distance() {
        let near_ambient = SoundPriority {
            importance: Importance::Ambient,
            gain: 1.0,
            distance: 0.0,
        };
        let far_attached = SoundPriority {
            importance: Importance::Attached,
            gain: 0.1,
            distance: 200.0,
        };
        assert!(far_attached.score() > near_ambient.score());
    }

    #[test]
    fn within_tier_closer_wins() {
        let near = SoundPriority {
            importance: Importance::OneShot,
            gain: 0.5,
            distance: 1.0,
        };
        let far = SoundPriority {
            importance: Importance::OneShot,
            gain: 0.5,
            distance: 50.0,
        };
        assert!(near.score() > far.score());
    }

    #[test]
    fn admits_until_full() {
        let mut pool: VoicePool<u32> = VoicePool::new(2);
        assert_eq!(pool.consider(10.0), Decision::Admit);
        pool.insert(1, 10.0);
        assert_eq!(pool.consider(10.0), Decision::Admit);
        pool.insert(2, 10.0);
        assert!(pool.is_full());
    }

    #[test]
    fn evicts_weakest_when_outranked() {
        let mut pool: VoicePool<u32> = VoicePool::new(2);
        pool.insert(1, 5.0);
        pool.insert(2, 8.0);
        // A newcomer at 6.0 outranks voice 1 (5.0) but not voice 2.
        assert_eq!(pool.consider(6.0), Decision::Evict(1));
    }

    #[test]
    fn rejects_when_all_higher() {
        let mut pool: VoicePool<u32> = VoicePool::new(2);
        pool.insert(1, 5.0);
        pool.insert(2, 8.0);
        assert_eq!(pool.consider(4.0), Decision::Reject);
        // Equal to the weakest is also rejected (no churn).
        assert_eq!(pool.consider(5.0), Decision::Reject);
    }

    #[test]
    fn insert_over_capacity_drops_weakest() {
        let mut pool: VoicePool<u32> = VoicePool::new(2);
        pool.insert(1, 5.0);
        pool.insert(2, 8.0);
        pool.insert(3, 9.0);
        assert_eq!(pool.len(), 2);
        let ids: Vec<u32> = pool.ids().collect();
        assert!(!ids.contains(&1), "weakest (1) should have been dropped");
        assert!(ids.contains(&2) && ids.contains(&3));
    }

    #[test]
    fn reprioritize_changes_eviction_target() {
        let mut pool: VoicePool<u32> = VoicePool::new(2);
        pool.insert(1, 5.0);
        pool.insert(2, 8.0);
        // Voice 2 moves far away and drops to 3.0; now it is the weakest.
        pool.reprioritize(2, 3.0);
        assert_eq!(pool.consider(4.0), Decision::Evict(2));
    }

    #[test]
    fn remove_reports_presence() {
        let mut pool: VoicePool<u32> = VoicePool::new(4);
        pool.insert(7, 1.0);
        assert!(pool.remove(7));
        assert!(!pool.remove(7));
        assert!(pool.is_empty());
    }
}
