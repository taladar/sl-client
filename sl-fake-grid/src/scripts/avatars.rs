//! Avatars as the region's scripts reach them (`server-lsl-lib-avatar-control`):
//! what a script reads of an avatar in the region and of any account, the
//! animations and overrides it plays on one, the controls it takes and the
//! follow camera it sets. What a script asks of a viewer goes out after the
//! tick ([`super::Delivery`]); the keys an avatar holds reach the scripts that
//! took them before the next tick's scripts run ([`hand_in_controls`]).

use std::collections::BTreeMap;

use sl_lsl_runtime::vm::avatars::{Agent, AgentRecord, CameraRule, ControlChange};
use sl_lsl_runtime::vm::{CallerId, Engine};
use sl_types::key::AgentKey;
use sl_types::lsl::{Rotation, Vector};

use super::{Delivery, Present, RegionHost};

/// A standing avatar's size as both grids report a default shape.
const AVATAR_SIZE: Vector = Vector {
    x: 0.45,
    y: 0.6,
    z: 1.9,
};

/// `AGENT_ATTACHMENTS`.
const AGENT_ATTACHMENTS: i32 = 0x2;
/// `AGENT_SITTING`.
const AGENT_SITTING: i32 = 0x10;
/// `AGENT_ON_OBJECT`.
const AGENT_ON_OBJECT: i32 = 0x20;

/// What scripts hold of the region's avatars: the controls each script
/// took, and the animation overrides each avatar was given.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegionAvatars {
    /// The avatar whose controls each script took.
    controls: BTreeMap<CallerId, AgentKey>,
    /// Each avatar's animation overrides, by state: the name a script gave
    /// and the asset.
    overrides: BTreeMap<AgentKey, BTreeMap<String, (String, String)>>,
}

impl RegionAvatars {
    /// What `agent` plays in `posture`: a script's override of the
    /// posture's state, or the built-in.
    pub(crate) fn posture_animation(
        &self,
        agent: AgentKey,
        posture: crate::movement::Posture,
    ) -> uuid::Uuid {
        self.overrides
            .get(&agent)
            .and_then(|overrides| overrides.get(posture.state_name()))
            .and_then(|(_name, asset)| uuid::Uuid::parse_str(asset).ok())
            .unwrap_or_else(|| posture.animation())
    }
}

/// The rotation whose forward, left and up axes are `at`, `left` and `up`
/// (`llAxes2Rot`).
fn axes_to_rotation(at: &Vector, left: &Vector, up: &Vector) -> Rotation {
    let trace = at.x + left.y + up.z;
    let (x, y, z, s) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        (
            (left.z - up.y) / s,
            (up.x - at.z) / s,
            (at.y - left.x) / s,
            0.25 * s,
        )
    } else if at.x > left.y && at.x > up.z {
        let s = (1.0 + at.x - left.y - up.z).sqrt() * 2.0;
        (
            0.25 * s,
            (left.x + at.y) / s,
            (up.x + at.z) / s,
            (left.z - up.y) / s,
        )
    } else if left.y > up.z {
        let s = (1.0 + left.y - at.x - up.z).sqrt() * 2.0;
        (
            (left.x + at.y) / s,
            0.25 * s,
            (up.y + left.z) / s,
            (up.x - at.z) / s,
        )
    } else {
        let s = (1.0 + up.z - at.x - left.y).sqrt() * 2.0;
        (
            (up.x + at.z) / s,
            (up.y + left.z) / s,
            0.25 * s,
            (at.y - left.x) / s,
        )
    };
    Rotation { x, y, z, s }
}

/// What a script reads of `present`: its camera's place and turn.
pub(super) fn camera_of(present: &Present) -> Option<(Vector, Rotation)> {
    let camera = present.camera.as_ref()?;
    Some((
        camera.center.clone(),
        axes_to_rotation(&camera.at_axis, &camera.left_axis, &camera.up_axis),
    ))
}

impl RegionHost<'_> {
    /// What the fake grid does not model of a script's call, said in its
    /// log where a reader of a run looks: the call does nothing.
    pub(super) fn refuse_unmodelled(&self, caller: CallerId, what: &str) {
        let whereabouts = sl_lsl_runtime::vm::Host::whereabouts(self, caller);
        tracing::warn!(
            object = %whereabouts.name,
            script = %whereabouts.script,
            "the fake grid does not model {what} yet; the call does nothing"
        );
    }

    /// The avatar keyed `agent` among those present.
    fn present_agent(&self, agent: &str) -> Option<&Present> {
        let key = Self::agent_named(agent)?;
        self.present.iter().find(|present| present.agent == key)
    }

    /// `llGetAgentInfo` and the rest of what a script reads of an avatar in
    /// the region.
    pub(super) fn agent_of(&self, agent: &str) -> Option<Agent> {
        let present = self.present_agent(agent)?;
        let key = present.agent;
        let seated = self.world.objects.seat_of(key).is_some();
        let worn = !self.world.objects.worn_by(key).is_empty();
        let mut info = present.posture_info;
        if seated {
            info |= AGENT_SITTING | AGENT_ON_OBJECT;
        }
        if worn {
            info |= AGENT_ATTACHMENTS;
        }
        let account = self.core.account_by_agent(key)?;
        let legacy_name = format!("{} {}", account.config.first_name, account.config.last_name);
        Some(Agent {
            username: username(&account.config.first_name, &account.config.last_name),
            display_name: legacy_name.clone(),
            legacy_name,
            size: AVATAR_SIZE,
            info,
            language: "en-us".to_owned(),
            state: if seated {
                "Sitting".to_owned()
            } else {
                present.state.to_owned()
            },
            animations: present.animations.clone(),
        })
    }

    /// `llRequestAgentData` and its kin: any account of the grid; online when
    /// it is in the region (the tick sees no further).
    pub(super) fn agent_record_of(&self, agent: &str) -> Option<AgentRecord> {
        let key = Self::agent_named(agent)?;
        let account = self.core.account_by_agent(key)?;
        let legacy_name = format!("{} {}", account.config.first_name, account.config.last_name);
        Some(AgentRecord {
            online: self.present.iter().any(|present| present.agent == key),
            username: username(&account.config.first_name, &account.config.last_name),
            display_name: legacy_name.clone(),
            legacy_name,
            born: "2026-01-01".to_owned(),
            payment: 0,
        })
    }

    /// `llName2Key`, `llRequestUserKey`: the account named `name`.
    pub(super) fn agent_called(&self, name: &str) -> Option<(String, bool)> {
        let account = self.core.accounts().find(|account| {
            let legacy = format!("{} {}", account.config.first_name, account.config.last_name);
            legacy.eq_ignore_ascii_case(name)
                || username(&account.config.first_name, &account.config.last_name)
                    .eq_ignore_ascii_case(name)
        })?;
        let here = self
            .present
            .iter()
            .any(|present| present.agent == account.agent_id);
        Some((account.agent_id.to_string(), here))
    }

    /// `llStartAnimation`, `llStopAnimation` by a script in the prim
    /// `object`.
    pub(super) fn animate_agent(
        &mut self,
        agent: &str,
        animation: &str,
        start: bool,
        object: sl_types::key::ObjectKey,
    ) {
        let (Some(agent), Ok(animation)) =
            (Self::agent_named(agent), uuid::Uuid::parse_str(animation))
        else {
            return;
        };
        self.deliveries.push(Delivery::Animate {
            agent,
            animation,
            start,
            object,
        });
    }

    /// `llSetAnimationOverride`, `llResetAnimationOverride`: kept, and what
    /// the avatar stands as told its viewer.
    pub(super) fn override_animation(
        &mut self,
        agent: &str,
        state: &str,
        animation: Option<(&str, &str)>,
    ) {
        let Some(key) = Self::agent_named(agent) else {
            return;
        };
        let overrides = self
            .world
            .script_instances
            .avatars
            .overrides
            .entry(key)
            .or_default();
        match animation {
            Some((name, asset)) => {
                let _previous =
                    overrides.insert(state.to_owned(), (name.to_owned(), asset.to_owned()));
            }
            None => {
                let _gone = overrides.remove(state);
            }
        }
        self.deliveries.push(Delivery::Overridden { agent: key });
    }

    /// `llGetAnimationOverride`.
    pub(super) fn override_of(&self, agent: &str, state: &str) -> Option<String> {
        let key = Self::agent_named(agent)?;
        self.world
            .script_instances
            .avatars
            .overrides
            .get(&key)?
            .get(state)
            .map(|(name, _)| name.clone())
    }

    /// `llTakeControls`, `llReleaseControls`.
    pub(super) fn change_controls(
        &mut self,
        caller: CallerId,
        agent: &str,
        changes: &[ControlChange],
        taken: Option<i32>,
    ) {
        let Some(key) = Self::agent_named(agent) else {
            return;
        };
        let controls = &mut self.world.script_instances.avatars.controls;
        match taken {
            Some(_) => {
                let _previous = controls.insert(caller, key);
            }
            None => {
                let _gone = controls.remove(&caller);
            }
        }
        if !changes.is_empty() {
            self.deliveries.push(Delivery::Controls {
                agent: key,
                changes: changes.to_vec(),
            });
        }
    }

    /// `llSetCameraParams`, or with [`None`] `llClearCameraParams`.
    pub(super) fn follow_camera(
        &mut self,
        caller: CallerId,
        agent: &str,
        rules: Option<&[CameraRule]>,
    ) {
        let (Some(agent), Some(origin)) = (Self::agent_named(agent), self.origin(caller)) else {
            return;
        };
        self.deliveries.push(Delivery::FollowCam {
            agent,
            object: origin.prim,
            rules: rules.map(<[CameraRule]>::to_vec),
        });
    }

    /// `llGetCameraPos`, `llGetCameraRot`.
    pub(super) fn camera_of_agent(&self, agent: &str) -> Option<(Vector, Rotation)> {
        camera_of(self.present_agent(agent)?)
    }

    /// `llGetAttachedList`: the keys of the roots the avatar wears.
    pub(super) fn attached_of(&self, agent: &str) -> Option<Vec<String>> {
        let present = self.present_agent(agent)?;
        Some(
            self.world
                .objects
                .worn_by(present.agent)
                .into_iter()
                .filter_map(|root| self.world.objects.get(root))
                .map(|object| object.full_id.to_string())
                .collect(),
        )
    }
}

/// A user name as the grids spell one: `first.last`, or `first` for a
/// `Resident`.
fn username(first: &str, last: &str) -> String {
    if last.eq_ignore_ascii_case("resident") {
        first.to_lowercase()
    } else {
        format!("{}.{}", first.to_lowercase(), last.to_lowercase())
    }
}

/// Before a tick's scripts run: every script that took an avatar's controls
/// hears what the avatar holds down now, and a script that no longer holds
/// controls (a reset dropped them) stops hearing.
pub(super) fn hand_in_controls(
    engine: &mut Engine,
    avatars: &mut RegionAvatars,
    present: &[Present],
) {
    avatars
        .controls
        .retain(|caller, _| engine.taken_controls(*caller).is_some());
    for (caller, agent) in &avatars.controls {
        let held = present
            .iter()
            .find(|present| present.agent == *agent)
            .map_or(0, |present| present.controls);
        let _posted = engine.controls(*caller, &agent.to_string(), held);
    }
}
