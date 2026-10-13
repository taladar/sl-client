//! What a region passes on from one viewer to the others: its effects and
//! its sounds.
//!
//! A `ViewerEffect` (a gaze, a pointing, an editing beam, a sphere) and a
//! viewer's `SoundTrigger` (the sound of a gesture) are not requests of the
//! simulator. They are addressed to the *other viewers*, and a region's whole
//! part in them is to decide who is sent which. The two live grids decide it
//! differently in nearly every respect — measured by the `effects-sounds`
//! conformance case on aditi and the local OpenSim (2026-10-13,
//! `book/src/gridspec/avatars.md` § Effects and sounds):
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | an effect goes to | everybody, the sender too | everybody but the sender |
//! | how far | the whole region | the whole region — but a message of nothing but gazes and beams only to a viewer whose **camera** is within 10 m of the sender |
//! | the agent an effect names | made the sender | left as sent |
//! | dropped | floating text (type 0); a gaze, a pointing or a beam whose payload does not begin with the sender's id | nothing |
//! | a sound goes to | everybody, the sender too | everybody within 100 m of where the sound is *said* to be, the sender too |
//! | its `ParentID` | nil | the sender |
//! | a parcel that bars avatar sounds | nobody is sent it | (by its source) the same |
//!
//! Both pass a sound's id, gain, position and region handle on exactly as the
//! viewer stated them — a gain of 2.5 or of −1, a nil sound, a position
//! outside the region, another region's handle — and both name the sender as
//! the sound's owner and its object.
//!
//! The grid publishes each as a
//! [`RegionChange`](crate::world::RegionChange) and every session's watcher
//! asks the policy whether its own avatar is sent it, the same way local chat
//! is routed ([`crate::chat`]).
//!
//! Not modelled: the group a parcel may leave its avatar sounds to (a parcel
//! that bars everybody's bars the sender's), and — because the fake grid
//! tracks no walking — distance from anywhere but where an avatar last
//! arrived.

use sl_proto::{TriggeredSound, ViewerEffect, ViewerEffectType};
use sl_types::key::{AgentKey, ObjectKey, OwnerKey};
use sl_types::lsl::Vector;

/// How far from the sender a viewer's camera may be for OpenSim to send it a
/// message of nothing but gazes and beams, in metres
/// (`Scene.ShouldSendDiscardableEffect`).
pub const OPENSIM_GAZE_RANGE_M: f32 = 10.0;

/// How far from where a sound is said to be OpenSim sends it, in metres
/// (`SoundModule.MaxDistance`).
pub const OPENSIM_SOUND_RANGE_M: f32 = 100.0;

/// How a grid passes a viewer's effects and sounds on
/// ([`ImitatedGrid::relay_policy`](crate::ImitatedGrid::relay_policy)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelayPolicy {
    /// Whether the sender is sent its own effects back.
    pub effects_echo: bool,
    /// What the grid does to the effects it passes on.
    pub effects: EffectsVetting,
    /// How near the sender a viewer's camera has to be to be sent a message
    /// of nothing but gazes and beams; [`None`] where distance is not asked.
    pub gaze_camera_range_m: Option<f32>,
    /// How far from where a sound is said to be it is sent; [`None`] for the
    /// whole region.
    pub sound_range_m: Option<f32>,
    /// Whether a viewer's sound names the sender as its `ParentID` too,
    /// rather than nobody.
    pub sound_names_a_parent: bool,
}

/// What a grid does to the effects it passes on ([`RelayPolicy::effects`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectsVetting {
    /// The agent each effect names is made the sender, whatever the viewer
    /// wrote, and floating text, and a gaze, a pointing or a beam whose
    /// payload does not begin with the sender's id, are dropped: Second Life.
    Vetted,
    /// Every effect goes on as it came: OpenSim.
    AsSent,
}

impl Default for RelayPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.relay_policy()
    }
}

/// Effects a viewer sent, as the region passes them on.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectsSent {
    /// The avatar whose viewer sent them.
    pub from: AgentKey,
    /// Where that avatar stands, region-local metres.
    pub at: Vector,
    /// The effects, already as the grid passes them on.
    pub effects: Vec<ViewerEffect>,
}

/// The square of the distance between two points.
fn distance_squared(left: &Vector, right: &Vector) -> f32 {
    let (dx, dy, dz) = (left.x - right.x, left.y - right.y, left.z - right.z);
    dz.mul_add(dz, dx.mul_add(dx, dy * dy))
}

impl RelayPolicy {
    /// Whether the grid passes `effect` of `from`'s on at all.
    fn passes(&self, from: AgentKey, effect: &ViewerEffect) -> bool {
        if self.effects == EffectsVetting::AsSent {
            return true;
        }
        match effect.effect_type {
            ViewerEffectType::Text => false,
            ViewerEffectType::LookAt | ViewerEffectType::PointAt | ViewerEffectType::Beam => effect
                .data
                .to_wire()
                .get(..16)
                .is_some_and(|source| source == from.uuid().as_bytes()),
            _other => true,
        }
    }

    /// `effects` of `from`'s, standing at `at`, as the grid passes them on:
    /// what it drops left out and the agent each names set as the grid sets
    /// it. [`None`] when nothing is left to pass on.
    #[must_use]
    pub fn effects_passed_on(
        &self,
        from: AgentKey,
        at: Vector,
        effects: &[ViewerEffect],
    ) -> Option<EffectsSent> {
        let effects: Vec<ViewerEffect> = effects
            .iter()
            .filter(|effect| self.passes(from, effect))
            .map(|effect| {
                let mut effect = effect.clone();
                if self.effects == EffectsVetting::Vetted {
                    effect.agent_id = from;
                }
                effect
            })
            .collect();
        (!effects.is_empty()).then_some(EffectsSent { from, at, effects })
    }

    /// Whether the session of `viewer`, whose camera is at `camera`, is sent
    /// `sent` — a viewer looking in from a neighbouring region as much as
    /// one whose avatar stands in this one: both grids send a child agent
    /// the effects of the avatars it can see
    /// (`book/src/gridspec/teleport.md`).
    #[must_use]
    pub fn effects_reach(&self, sent: &EffectsSent, viewer: AgentKey, camera: &Vector) -> bool {
        if viewer == sent.from {
            return self.effects_echo;
        }
        let Some(range) = self.gaze_camera_range_m else {
            return true;
        };
        let discardable = sent.effects.iter().all(|effect| {
            matches!(
                effect.effect_type,
                ViewerEffectType::LookAt | ViewerEffectType::Beam
            )
        });
        !discardable || distance_squared(camera, &sent.at) < range * range
    }

    /// A viewer's sound as the grid passes it on: under the sender's name,
    /// with everything else as the viewer stated it.
    #[must_use]
    pub fn sound_passed_on(&self, from: AgentKey, mut sound: TriggeredSound) -> TriggeredSound {
        let sender = ObjectKey::from(from.uuid());
        sound.owner = OwnerKey::Agent(from);
        sound.object = sender;
        sound.parent = self.sound_names_a_parent.then_some(sender);
        sound
    }

    /// Whether an avatar standing at `position` is sent `sound`.
    #[must_use]
    pub fn sound_reaches(&self, sound: &TriggeredSound, position: &Vector) -> bool {
        self.sound_range_m
            .is_none_or(|range| distance_squared(position, &sound.position) <= range * range)
    }
}

#[cfg(test)]
mod tests {
    use super::{EffectsSent, RelayPolicy};
    use crate::ImitatedGrid;
    use pretty_assertions::assert_eq;
    use sl_proto::{
        AssetKey, GlobalCoordinates, LookAtType, RegionHandle, TriggeredSound, ViewerEffect,
        ViewerEffectData, ViewerEffectType,
    };
    use sl_types::key::{AgentKey, ObjectKey, OwnerKey};
    use sl_types::lsl::Vector;
    use uuid::Uuid;

    /// The avatar that sends.
    fn sender() -> AgentKey {
        AgentKey::from(Uuid::from_u128(1))
    }

    /// The avatar that watches.
    fn watcher() -> AgentKey {
        AgentKey::from(Uuid::from_u128(2))
    }

    /// A point `x` metres east of the region's corner.
    const fn east(x: f32) -> Vector {
        Vector { x, y: 0.0, z: 0.0 }
    }

    /// A gaze of `source`'s, as the sender's viewer sends it.
    fn gaze(source: AgentKey) -> ViewerEffect {
        ViewerEffect {
            id: Uuid::from_u128(7),
            agent_id: sender(),
            effect_type: ViewerEffectType::LookAt,
            duration: 0.5,
            color: [255; 4],
            data: ViewerEffectData::LookAt {
                source: Some(source),
                target: None,
                target_position: GlobalCoordinates::new(1.0, 2.0, 3.0),
                look_at_type: LookAtType::Focus,
            },
        }
    }

    /// An effect of the type `effect_type` with the payload `bytes`, naming
    /// `agent`.
    fn effect(effect_type: ViewerEffectType, agent: AgentKey, bytes: Vec<u8>) -> ViewerEffect {
        ViewerEffect {
            id: Uuid::from_u128(8),
            agent_id: agent,
            effect_type,
            duration: 0.5,
            color: [255; 4],
            data: ViewerEffectData::Raw(bytes),
        }
    }

    /// The effects `policy` passes on of `effects`.
    fn passed(policy: &RelayPolicy, effects: &[ViewerEffect]) -> Vec<ViewerEffect> {
        policy
            .effects_passed_on(sender(), east(0.0), effects)
            .map_or_else(Vec::new, |sent| sent.effects)
    }

    #[test]
    fn second_life_checks_what_it_passes_on() {
        let policy = ImitatedGrid::SecondLife.relay_policy();
        // Floating text is dropped; a sphere of any payload is not.
        assert_eq!(
            passed(
                &policy,
                &[effect(ViewerEffectType::Text, sender(), vec![1, 2, 3, 4])]
            ),
            Vec::new()
        );
        let sphere = effect(ViewerEffectType::Sphere, sender(), Vec::new());
        assert_eq!(
            passed(&policy, std::slice::from_ref(&sphere)),
            vec![sphere.clone()]
        );
        // A gaze of the sender's passes; one of somebody else's, or one too
        // short to say whose it is, does not — and neither does a beam.
        assert_eq!(passed(&policy, &[gaze(sender())]), vec![gaze(sender())]);
        assert_eq!(passed(&policy, &[gaze(watcher())]), Vec::new());
        for effect_type in [
            ViewerEffectType::LookAt,
            ViewerEffectType::PointAt,
            ViewerEffectType::Beam,
        ] {
            assert_eq!(
                passed(&policy, &[effect(effect_type, sender(), vec![1, 2, 3])]),
                Vec::new()
            );
        }
        // What is left of a message goes on without what was dropped, and
        // every effect names the sender whatever the viewer wrote.
        let foreign = effect(ViewerEffectType::Sphere, watcher(), Vec::new());
        assert_eq!(passed(&policy, &[gaze(watcher()), foreign]), vec![sphere]);
    }

    #[test]
    fn opensim_passes_everything_on_as_sent() {
        let policy = ImitatedGrid::OpenSim.relay_policy();
        let effects = vec![
            effect(ViewerEffectType::Text, watcher(), vec![1, 2, 3, 4]),
            gaze(watcher()),
            effect(ViewerEffectType::Beam, sender(), Vec::new()),
        ];
        assert_eq!(passed(&policy, &effects), effects);
    }

    #[test]
    fn who_is_sent_an_effect() {
        let gazes = EffectsSent {
            from: sender(),
            at: east(0.0),
            effects: vec![gaze(sender())],
        };
        let mixed = EffectsSent {
            from: sender(),
            at: east(0.0),
            effects: vec![
                gaze(sender()),
                effect(ViewerEffectType::Sphere, sender(), Vec::new()),
            ],
        };
        let second_life = ImitatedGrid::SecondLife.relay_policy();
        let opensim = ImitatedGrid::OpenSim.relay_policy();
        // The sender: Second Life sends it its own.
        assert_eq!(
            second_life.effects_reach(&gazes, sender(), &east(0.0)),
            true
        );
        assert_eq!(opensim.effects_reach(&gazes, sender(), &east(0.0)), false);
        // A camera 16 m off: OpenSim holds back a message of gazes alone.
        assert_eq!(
            second_life.effects_reach(&gazes, watcher(), &east(16.0)),
            true
        );
        assert_eq!(opensim.effects_reach(&gazes, watcher(), &east(16.0)), false);
        assert_eq!(opensim.effects_reach(&gazes, watcher(), &east(9.0)), true);
        assert_eq!(opensim.effects_reach(&mixed, watcher(), &east(120.0)), true);
    }

    #[test]
    fn a_sound_is_passed_on_under_the_senders_name() {
        let asked = TriggeredSound {
            sound: AssetKey::from(Uuid::from_u128(9)),
            owner: OwnerKey::Agent(AgentKey::from(Uuid::nil())),
            object: ObjectKey::from(Uuid::nil()),
            parent: None,
            region_handle: RegionHandle(42),
            position: east(300.0),
            gain: 2.5,
        };
        let me = ObjectKey::from(sender().uuid());
        for (grid, parent, far) in [
            (ImitatedGrid::SecondLife, None, true),
            (ImitatedGrid::OpenSim, Some(me), false),
        ] {
            let policy = grid.relay_policy();
            let sound = policy.sound_passed_on(sender(), asked.clone());
            assert_eq!(sound.owner, OwnerKey::Agent(sender()));
            assert_eq!(sound.object, me);
            assert_eq!(sound.parent, parent);
            // As the viewer stated it, out of range or not.
            assert_eq!(sound.region_handle, RegionHandle(42));
            assert_eq!(sound.gain.to_bits(), 2.5_f32.to_bits());
            assert_eq!(policy.sound_reaches(&sound, &east(250.0)), true);
            assert_eq!(policy.sound_reaches(&sound, &east(150.0)), far);
        }
    }
}
