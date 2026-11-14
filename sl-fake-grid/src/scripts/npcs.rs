//! OpenSim's NPCs as the region's scripts make them (`osNpc*`,
//! `server-lsl-lib-ossl-avatars`), and what the region keeps of every
//! avatar's health (`osGetHealth` and its kin).
//!
//! An NPC is shown to the region's viewers as a resident with no session
//! behind it ([`crate::world::Resident`]): its arrival, its moves, its
//! animations, its appearance and its departure reach every viewer down the
//! same changes a logged-in avatar's do, and a viewer arriving later is shown
//! it with the others. What only an NPC has — its owner, where it is walking
//! to, how it looks as a script reads it — is kept here, beside the health
//! the region heals every avatar by.
//!
//! An NPC walks the way the fake grid's own avatar controller moves an agent,
//! at the grid's speeds ([`crate::movement::MovementPolicy`]), but as a point
//! in a straight line: no physics, no prims in the way. It walks along the
//! ground, or flies straight to a target more than a stride above the ground
//! unless told not to, and stops within half a metre of it.

use std::collections::BTreeMap;

use sl_lsl_runtime::vm::avatars::Agent;
use sl_lsl_runtime::vm::npcs::{
    Appearance, AppearanceAttachment, DEFAULT_AVATAR_TEXTURE, DEFAULT_VISUAL_PARAMS, Npc,
    NpcAction, NpcRefusal, NpcSpec, NpcSpeech, Vitals,
};
use sl_lsl_runtime::vm::{CallerId, Heard, Overheard};
use sl_proto::{ChatChannel, ChatSource, ChatType, Object, ObjectKey, TextureKey};
use sl_types::key::AgentKey;
use sl_types::lsl::{Rotation, Vector};

use super::{Delivery, Effect, RegionHost, chat_type_of, heard_of};
use crate::animations::AnimationSet;
use crate::chat::Line;
use crate::movement::{MovementPolicy, Posture};
use crate::world::{
    AVATAR_CENTRE_ABOVE_GROUND_M, AvatarIdentity, RegionChange, Resident, SceneFixtures,
};

/// The most NPCs a region holds (OpenSim's `MaxNumberNPCsPerScene`).
const MAX_NPCS: usize = 40;

/// How near its target an NPC stops.
const ARRIVED_M: f32 = 0.5;

/// How far above the ground under it a target may be for an NPC to walk to
/// it rather than fly.
const WALKS_UP_TO_M: f32 = 2.0;

/// The height a new appearance reads, as OpenSim's default avatar.
const DEFAULT_HEIGHT_M: f32 = 1.9;

/// The built-in `sit` animation, which a seated NPC plays.
const SIT: uuid::Uuid = uuid::uuid!("1a5fe8ac-a804-8a5d-7cbd-56bd83184568");

/// Where an NPC is walking, or flying, to (`osNpcMoveToTarget`).
#[derive(Debug, Clone, PartialEq)]
struct Walk {
    /// The target.
    target: Vector,
    /// Whether it flies there.
    fly: bool,
    /// Whether it lands at the end of a flight.
    land_at_target: bool,
    /// Whether it runs.
    running: bool,
}

/// One NPC a script created.
#[derive(Debug, Clone)]
struct ScriptedNpc {
    /// Its agent key.
    agent: AgentKey,
    /// Its name, `First Last`.
    name: String,
    /// The avatar that owns it, or [`None`] for an unowned one.
    owner: Option<AgentKey>,
    /// Whether a sensor sees it as an agent (`OS_NPC_SENSE_AS_AGENT`).
    sense_as_agent: bool,
    /// How it looks, as a script reads it.
    appearance: Appearance,
    /// What it plays.
    animations: AnimationSet,
    /// Where it is going, if anywhere.
    walk: Option<Walk>,
    /// How many times as fast as an avatar it moves (`osSetSpeed`).
    speed: f32,
}

/// The region's NPCs, and every avatar's health.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegionNpcs {
    /// The NPCs, in the order they were created.
    npcs: Vec<ScriptedNpc>,
    /// The health of each avatar and NPC a script has read or changed; one
    /// not listed has a fresh presence's ([`Vitals::default`]).
    vitals: BTreeMap<AgentKey, Vitals>,
    /// The health each avatar's viewer was last told.
    told: BTreeMap<AgentKey, f32>,
}

impl RegionNpcs {
    /// The NPC keyed `agent`.
    fn get(&self, agent: AgentKey) -> Option<&ScriptedNpc> {
        self.npcs.iter().find(|npc| npc.agent == agent)
    }

    /// The NPC keyed `agent`, to change.
    fn get_mut(&mut self, agent: AgentKey) -> Option<&mut ScriptedNpc> {
        self.npcs.iter_mut().find(|npc| npc.agent == agent)
    }

    /// Whether `agent` is an NPC of the region.
    pub(crate) fn is_npc(&self, agent: AgentKey) -> bool {
        self.get(agent).is_some()
    }
}

/// The default avatar's appearance, as OpenSim starts one.
fn default_appearance() -> Appearance {
    Appearance {
        serial: 0,
        height: DEFAULT_HEIGHT_M,
        hover: 0.0,
        textures: vec![DEFAULT_AVATAR_TEXTURE.to_owned(); sl_proto::avatar_texture::COUNT],
        extra_bakes: None,
        visual_params: DEFAULT_VISUAL_PARAMS.to_vec(),
        attachments: Vec::new(),
    }
}

/// The `AvatarAppearance` record a viewer is sent for an avatar `agent` that
/// looks as `appearance` says: its visual parameters and the textures of its
/// slots.
fn appearance_record(agent: AgentKey, appearance: &Appearance) -> sl_proto::AvatarAppearance {
    let mut record = crate::fixtures::NpcAppearance {
        visual_params: appearance.visual_params.clone(),
        bakes: Vec::new(),
    }
    .record(agent, Vec::new());
    for (face, texture) in record
        .texture_entry
        .faces
        .iter_mut()
        .zip(&appearance.textures)
    {
        if let Ok(id) = uuid::Uuid::parse_str(texture) {
            face.texture_id = TextureKey::from(id);
        }
    }
    record
}

/// How the avatar `agent`, shown as `resident`, looks as a script reads it:
/// its record's parameters and textures, its height, and what it wears.
fn resident_appearance(world: &SceneFixtures, agent: AgentKey, resident: &Resident) -> Appearance {
    let attachments = world
        .objects
        .worn_by(agent)
        .into_iter()
        .filter_map(|root| {
            let attached = world.objects.attached_to(root)?;
            let item = match attached.item {
                crate::scene_store::WornItem::Item(item) => item.uuid(),
                crate::scene_store::WornItem::Unfiled | crate::scene_store::WornItem::Temporary => {
                    uuid::Uuid::nil()
                }
            };
            Some(AppearanceAttachment {
                point: i32::from(attached.point),
                item: item.to_string(),
                asset: uuid::Uuid::nil().to_string(),
            })
        })
        .collect();
    Appearance {
        serial: 0,
        height: resident.body.scale.z,
        hover: resident
            .appearance
            .hover_height
            .as_ref()
            .map_or(0.0, |hover| hover.z),
        textures: resident
            .appearance
            .texture_entry
            .faces
            .iter()
            .map(|face| face.texture_id.uuid().to_string())
            .collect(),
        extra_bakes: None,
        visual_params: resident.appearance.visual_params.clone(),
        attachments,
    }
}

/// The turn about the vertical that faces along `(x, y)`.
fn facing(x: f32, y: f32) -> Rotation {
    let half = y.atan2(x) / 2.0;
    Rotation {
        x: 0.0,
        y: 0.0,
        z: half.sin(),
        s: half.cos(),
    }
}

/// The region's NPCs walk one tick of `step` on, at `policy`'s speeds over the
/// ground `ground` reads: each one's move told to every viewer as a terse
/// update, and each change of what it plays as its animations — what the
/// region's sessions are to be told.
pub(crate) fn step(
    world: &mut SceneFixtures,
    step: core::time::Duration,
    (policy, animations): (&MovementPolicy, crate::animations::AnimationPolicy),
    ground: &dyn Fn(f32, f32) -> f32,
) -> Vec<RegionChange> {
    let mut changes = Vec::new();
    let walking: Vec<(AgentKey, Walk, f32)> = world
        .script_instances
        .npcs
        .npcs
        .iter()
        .filter_map(|npc| npc.walk.clone().map(|walk| (npc.agent, walk, npc.speed)))
        .collect();
    for (agent, walk, speed) in walking {
        let Some(resident) = world.resident(agent) else {
            continue;
        };
        if resident.body.parent_id.0 != 0 {
            continue;
        }
        let from = resident.at.clone();
        let pace = if walk.fly {
            policy.fly_speed
        } else if walk.running {
            policy.run_speed
        } else {
            policy.walk_speed
        } * speed;
        let stride = pace * step.as_secs_f32();
        let (dx, dy) = (walk.target.x - from.x, walk.target.y - from.y);
        let dz = if walk.fly {
            walk.target.z - from.z
        } else {
            0.0
        };
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();
        let arrived = distance <= ARRIVED_M.max(stride);
        let mut to = if arrived {
            walk.target.clone()
        } else {
            Vector {
                x: from.x + dx / distance * stride,
                y: from.y + dy / distance * stride,
                z: from.z + dz / distance * stride,
            }
        };
        let standing_z = ground(to.x, to.y) + AVATAR_CENTRE_ABOVE_GROUND_M;
        if !walk.fly || (arrived && walk.land_at_target) || to.z < standing_z {
            to.z = standing_z;
        }
        let turn = if dx.abs() > f32::EPSILON || dy.abs() > f32::EPSILON {
            facing(dx, dy)
        } else {
            resident.body.motion.rotation.clone()
        };
        let velocity = if arrived {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        } else {
            let seconds = step.as_secs_f32().max(f32::EPSILON);
            Vector {
                x: (to.x - from.x) / seconds,
                y: (to.y - from.y) / seconds,
                z: (to.z - from.z) / seconds,
            }
        };
        let posture = if arrived && !(walk.fly && !walk.land_at_target) {
            Posture::Standing
        } else if arrived {
            Posture::Hovering
        } else if walk.fly {
            Posture::Flying
        } else if walk.running {
            Posture::Running
        } else {
            Posture::Walking
        };
        world.objects.move_wearer(agent, &to);
        let Some(resident) = world.resident_mut(agent) else {
            continue;
        };
        resident.at = to.clone();
        resident.body.motion.position = to;
        resident.body.motion.velocity = velocity;
        resident.body.motion.rotation = turn;
        let update = sl_proto::TerseUpdate {
            local_id: resident.body.local_id,
            state: 0,
            motion: resident.body.motion.clone(),
        };
        changes.push(RegionChange::ResidentMoved {
            agent,
            update: Box::new(update),
        });
        let Some(npc) = world.script_instances.npcs.get_mut(agent) else {
            continue;
        };
        if arrived {
            npc.walk = None;
        }
        if npc.animations.set_default(posture.animation()) {
            let statement = npc.animations.observed(animations, agent);
            if let Some(resident) = world.resident_mut(agent) {
                resident.animations = statement.clone();
            }
            changes.push(RegionChange::ResidentAnimated {
                agent,
                statement: Box::new(statement),
            });
        }
    }
    changes
}

/// One tick of `step`: every avatar under full health heals at its rate, and
/// its viewer is told when it reaches full or has moved a point since it was
/// last told, as OpenSim's presence does — what to send.
pub(crate) fn heal(world: &mut SceneFixtures, step: core::time::Duration) -> Vec<(AgentKey, f32)> {
    let npcs = &mut world.script_instances.npcs;
    let mut tell = Vec::new();
    for (agent, vitals) in &mut npcs.vitals {
        if vitals.heal_rate == 0.0 || (vitals.health - 100.0).abs() < f32::EPSILON {
            continue;
        }
        vitals.health += vitals.heal_rate * step.as_secs_f32();
        let told = npcs.told.get(agent).copied().unwrap_or(100.0);
        if vitals.health >= 100.0 {
            vitals.health = 100.0;
            tell.push((*agent, vitals.health));
        } else if (vitals.health - told).abs() > 1.0 {
            tell.push((*agent, vitals.health));
        }
    }
    for (agent, health) in &tell {
        let _told = npcs.told.insert(*agent, *health);
    }
    tell.into_iter()
        .filter(|(agent, _health)| !npcs.is_npc(*agent))
        .collect()
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// The NPC keyed `npc`, as a script names it.
    fn npc_keyed(&self, npc: &str) -> Option<AgentKey> {
        let agent = Self::agent_named(npc)?;
        self.world
            .script_instances
            .npcs
            .is_npc(agent)
            .then_some(agent)
    }

    /// `osNpcCreate`: an NPC shown to the region's viewers as an arriving
    /// avatar, standing.
    pub(super) fn create_npc_as(
        &mut self,
        caller: CallerId,
        spec: &NpcSpec,
    ) -> Result<String, NpcRefusal> {
        if self.world.script_instances.npcs.npcs.len() >= MAX_NPCS {
            return Err(NpcRefusal::Full);
        }
        let owner = AgentKey::from(
            self.origin(caller)
                .map_or_else(uuid::Uuid::nil, |origin| origin.object.owner_id),
        );
        if !self.may_rez_at(owner, &spec.position) {
            return Err(NpcRefusal::NoRez);
        }
        let agent = AgentKey::from(self.core.minter.uuid());
        let local_id = self.world.arrive_avatar(agent);
        let identity = AvatarIdentity::new(agent, &spec.first, &spec.last);
        let mut body = crate::world::avatar_prim(local_id, &identity, spec.position.clone());
        body.name_value = format!(
            "FirstName STRING RW SV {}\nLastName STRING RW SV {}\nTitle STRING RW SV {}",
            spec.first, spec.last, spec.group_title
        );
        let appearance = spec.appearance.clone().unwrap_or_else(default_appearance);
        let animations = AnimationSet::arriving();
        let statement = animations.observed(self.core.animation_policy, agent);
        self.world.show_resident(
            agent,
            Resident {
                at: spec.position.clone(),
                body,
                appearance: self
                    .core
                    .bakes
                    .applied(appearance_record(agent, &appearance)),
                animations: statement,
            },
        );
        self.world.script_instances.npcs.npcs.push(ScriptedNpc {
            agent,
            name: format!("{} {}", spec.first, spec.last),
            owner: spec.owner.as_deref().and_then(Self::agent_named),
            sense_as_agent: spec.sense_as_agent,
            appearance,
            animations,
            walk: None,
            speed: 1.0,
        });
        self.effects
            .push(Effect::Tell(vec![RegionChange::ResidentArrived(agent)]));
        Ok(agent.uuid().to_string())
    }

    /// Whether `owner` may rez at `position` (OpenSim's `CanRezObject`), as
    /// the region judges an object's rez.
    fn may_rez_at(&self, owner: AgentKey, position: &Vector) -> bool {
        self.world.refuses_rez(owner, position).is_none()
    }

    /// The region's NPCs as a script reads them.
    pub(super) fn npc_list(&self) -> Vec<Npc> {
        self.world
            .script_instances
            .npcs
            .npcs
            .iter()
            .filter_map(|npc| {
                let resident = self.world.resident(npc.agent)?;
                Some(Npc {
                    key: npc.agent.uuid().to_string(),
                    name: npc.name.clone(),
                    owner: npc.owner.map(|owner| owner.uuid().to_string()),
                    position: resident.at.clone(),
                    rotation: resident.body.motion.rotation.clone(),
                })
            })
            .collect()
    }

    /// What a script reads of the NPC keyed `npc` as an avatar of the region
    /// (`llGetAgentSize`, `llKey2Name`, …), as OpenSim answers of an NPC's
    /// presence.
    pub(super) fn npc_agent(&self, npc: &str) -> Option<Agent> {
        let agent = self.npc_keyed(npc)?;
        let scripted = self.world.script_instances.npcs.get(agent)?;
        let resident = self.world.resident(agent)?;
        let seated = resident.body.parent_id.0 != 0;
        let username = scripted.name.replace(' ', ".").to_lowercase();
        Some(Agent {
            legacy_name: scripted.name.clone(),
            display_name: scripted.name.clone(),
            username,
            size: resident.body.scale.clone(),
            info: if seated { 0x10 | 0x20 } else { 0 },
            language: String::new(),
            state: if seated {
                "Sitting".to_owned()
            } else {
                "Standing".to_owned()
            },
            animations: scripted
                .animations
                .playing(agent)
                .into_iter()
                .map(|animation| animation.anim_id.to_string())
                .collect(),
        })
    }

    /// Where the NPC keyed `npc` stands, and its body.
    pub(super) fn npc_body(&self, npc: &str) -> Option<(Vector, Object)> {
        let agent = self.npc_keyed(npc)?;
        let resident = self.world.resident(agent)?;
        Some((resident.at.clone(), resident.body.clone()))
    }

    /// `osNpc*`: the NPC keyed `npc` does `action`.
    pub(super) fn act_npc(&mut self, caller: CallerId, npc: &str, action: &NpcAction) {
        let Some(agent) = self.npc_keyed(npc) else {
            return;
        };
        match action {
            NpcAction::MoveTo {
                target,
                no_fly,
                land_at_target,
                running,
            } => {
                let seated = self
                    .world
                    .resident(agent)
                    .is_some_and(|resident| resident.body.parent_id.0 != 0);
                if seated {
                    return;
                }
                let fly = !no_fly && target.z - self.ground_at(target.x, target.y) > WALKS_UP_TO_M;
                if let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) {
                    scripted.walk = Some(Walk {
                        target: target.clone(),
                        fly,
                        land_at_target: *land_at_target,
                        running: *running,
                    });
                }
            }
            NpcAction::StopMoving => {
                if let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) {
                    scripted.walk = None;
                }
                let tell = self.world.resident_mut(agent).map(|resident| {
                    resident.body.motion.velocity = Vector {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    };
                    (resident.body.local_id, resident.body.motion.clone())
                });
                self.posture(agent, Posture::Standing);
                if let Some((local_id, motion)) = tell {
                    self.tell_moved(agent, local_id, motion);
                }
            }
            NpcAction::Turn(rotation) => {
                let tell = self.world.resident_mut(agent).map(|resident| {
                    resident.body.motion.rotation = rotation.clone();
                    (resident.body.local_id, resident.body.motion.clone())
                });
                if let Some((local_id, motion)) = tell {
                    self.tell_moved(agent, local_id, motion);
                }
            }
            NpcAction::Stand => self.stand_npc(agent),
            NpcAction::Remove => self.remove_npc(agent),
            NpcAction::Touch(prim) => self.npc_touch(agent, prim),
            NpcAction::About(_) | NpcAction::Image(_) => {
                self.refuse_unmodelled(caller, "profiles (an NPC's \"about\" text or picture)");
            }
            NpcAction::Appearance(appearance) => {
                let record = self
                    .core
                    .bakes
                    .applied(appearance_record(agent, appearance));
                if let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) {
                    scripted.appearance = (**appearance).clone();
                }
                if let Some(resident) = self.world.resident_mut(agent) {
                    resident.appearance = record.clone();
                }
                self.effects
                    .push(Effect::Tell(vec![RegionChange::ResidentAppearance(
                        Box::new(record),
                    )]));
            }
            NpcAction::LookAt {
                kind,
                target,
                offset,
            } => self.npc_look_at(agent, *kind, target, offset),
        }
    }

    /// Tells the region's viewers that the NPC `agent`, shown as `local_id`,
    /// moved to `motion`.
    fn tell_moved(
        &mut self,
        agent: AgentKey,
        local_id: sl_proto::RegionLocalObjectId,
        motion: sl_proto::ObjectMotion,
    ) {
        self.effects
            .push(Effect::Tell(vec![RegionChange::ResidentMoved {
                agent,
                update: Box::new(sl_proto::TerseUpdate {
                    local_id,
                    state: 0,
                    motion,
                }),
            }]));
    }

    /// The NPC `agent` plays `posture`'s animation as its default, the
    /// viewers told when that changes.
    fn posture(&mut self, agent: AgentKey, posture: Posture) {
        self.default_animation(agent, posture.animation());
    }

    /// The NPC `agent` plays `animation` as its default, the viewers told
    /// when that changes.
    fn default_animation(&mut self, agent: AgentKey, animation: uuid::Uuid) {
        let policy = self.core.animation_policy;
        let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) else {
            return;
        };
        if !scripted.animations.set_default(animation) {
            return;
        }
        let statement = scripted.animations.observed(policy, agent);
        self.tell_animations(agent, statement);
    }

    /// Keeps `statement` as what the NPC `agent` is shown playing, and tells
    /// the region's viewers.
    fn tell_animations(&mut self, agent: AgentKey, statement: crate::animations::Statement) {
        if let Some(resident) = self.world.resident_mut(agent) {
            resident.animations = statement.clone();
        }
        self.effects
            .push(Effect::Tell(vec![RegionChange::ResidentAnimated {
                agent,
                statement: Box::new(statement),
            }]));
    }

    /// `osNpcPlayAnimation`, `osNpcStopAnimation` and the `osAvatar*` twins on
    /// an NPC: `animation` started or stopped by the prim `object`'s script.
    pub(super) fn animate_npc(
        &mut self,
        npc: &str,
        animation: &str,
        start: bool,
        object: ObjectKey,
    ) {
        let (Some(agent), Ok(animation)) = (self.npc_keyed(npc), uuid::Uuid::parse_str(animation))
        else {
            return;
        };
        let policy = self.core.animation_policy;
        let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) else {
            return;
        };
        if !scripted.animations.apply_scripted(animation, start, object) {
            return;
        }
        let statement = scripted.animations.observed(policy, agent);
        self.tell_animations(agent, statement);
    }

    /// Re-shows the NPC `agent` with `body`, as it now sits or stands.
    fn reshow_npc(&mut self, agent: AgentKey, body: &Object) {
        if let Some(shown) = self.world.reshow_resident(agent, body) {
            self.effects.push(Effect::Tell(vec![shown]));
        }
    }

    /// `osNpcSit`, `osForceOtherSit` of an NPC: it sits on the prim
    /// `local_id` where a sit request would put it, playing `sit`.
    pub(super) fn sit_npc(&mut self, agent: AgentKey, local_id: sl_proto::RegionLocalObjectId) {
        let Some(seat) = self.world.objects.get(local_id).cloned() else {
            return;
        };
        if let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) {
            scripted.walk = None;
        }
        if crate::seating::sit(self.world, agent, &seat).is_none() {
            return;
        }
        let Some((_seat, sitter)) = self
            .world
            .objects
            .seat_of(agent)
            .map(|(seat, sitter)| (seat.clone(), sitter.clone()))
        else {
            return;
        };
        let Some(mut body) = self
            .world
            .resident(agent)
            .map(|resident| resident.body.clone())
        else {
            return;
        };
        body.parent_id = seat.local_id;
        body.motion.position = sitter.offset;
        body.motion.rotation = sitter.rotation;
        body.motion.velocity = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        self.reshow_npc(agent, &body);
        self.default_animation(agent, SIT);
    }

    /// `osNpcStand`: the NPC stands up where the region stands a sitter.
    fn stand_npc(&mut self, agent: AgentKey) {
        let Some(stands_at) = crate::seating::stand(self.world, agent) else {
            return;
        };
        let Some(mut body) = self
            .world
            .resident(agent)
            .map(|resident| resident.body.clone())
        else {
            return;
        };
        body.parent_id = sl_proto::RegionLocalObjectId(0);
        body.motion.position = stands_at;
        self.reshow_npc(agent, &body);
        self.posture(agent, Posture::Standing);
    }

    /// `osNpcRemove`, and an NPC killed or kicked: it leaves the region,
    /// stood up from its seat, what it wears with it.
    pub(super) fn remove_npc(&mut self, agent: AgentKey) {
        let changes = crate::seating::leave(self.world, agent);
        let npcs = &mut self.world.script_instances.npcs;
        npcs.npcs.retain(|npc| npc.agent != agent);
        let _health = npcs.vitals.remove(&agent);
        self.effects.push(Effect::Tell(changes));
    }

    /// `osNpcTouch`: the NPC clicks the prim keyed `prim`, if it takes
    /// touches.
    fn npc_touch(&mut self, agent: AgentKey, prim: &str) {
        let Some(local_id) = self.prim_keyed(prim) else {
            return;
        };
        let (Some(scripted), Some(at)) = (
            self.world.script_instances.npcs.get(agent).cloned(),
            self.world
                .resident(agent)
                .map(|resident| resident.at.clone()),
        ) else {
            return;
        };
        // The touch events go to the engine, which is lent to the tick.
        self.effects.push(Effect::NpcTouch {
            agent,
            name: scripted.name,
            at,
            prim: local_id,
        });
    }

    /// `osNpcLookAt`: the NPC's `LookAt` viewer effect, sent every viewer.
    fn npc_look_at(&mut self, agent: AgentKey, kind: i32, target: &str, offset: &Vector) {
        let Some(at) = self
            .world
            .resident(agent)
            .map(|resident| resident.at.clone())
        else {
            return;
        };
        let target = uuid::Uuid::parse_str(target).unwrap_or_default();
        let mut data = Vec::with_capacity(57);
        data.extend_from_slice(agent.uuid().as_bytes());
        data.extend_from_slice(target.as_bytes());
        for component in [offset.x, offset.y, offset.z] {
            #[expect(
                clippy::little_endian_bytes,
                reason = "a LookAt effect's offset is a wire-defined little-endian double"
            )]
            let bytes = f64::from(component).to_le_bytes();
            data.extend_from_slice(&bytes);
        }
        data.push(u8::try_from(kind).unwrap_or(0));
        let effect = sl_proto::ViewerEffect {
            id: self.core.minter.uuid(),
            agent_id: agent,
            effect_type: sl_proto::ViewerEffectType::LookAt,
            duration: 0.0,
            color: [0; 4],
            data: sl_proto::ViewerEffectData::Raw(data),
        };
        self.effects
            .push(Effect::Tell(vec![RegionChange::Effects(Box::new(
                crate::relays::EffectsSent {
                    from: agent,
                    at,
                    effects: vec![effect],
                },
            ))]));
    }

    /// `osNpcSay` and its kin: the NPC's line, to the avatars and the
    /// scripts in earshot, or to one avatar or prim.
    pub(super) fn npc_says(&mut self, npc: &str, speech: &NpcSpeech) -> Option<Overheard> {
        let agent = self.npc_keyed(npc)?;
        let name = self.world.script_instances.npcs.get(agent)?.name.clone();
        let at = self.world.resident(agent)?.at.clone();
        let line = Line {
            source: ChatSource::Agent(agent),
            from_name: name.clone(),
            owner_id: agent.uuid(),
            chat_type: chat_type_of(speech.volume),
            channel: ChatChannel(speech.channel),
            position: at,
            message: speech.text.clone(),
        };
        let Some(target) = &speech.to else {
            let by = self
                .world
                .script_instances
                .in_earshot(&line, &self.world.objects);
            self.effects.push(Effect::Say(line.clone()));
            return (!by.is_empty()).then(|| Overheard {
                heard: heard_of(&line),
                by,
            });
        };
        // `DeliverMessageTo`: a prim's scripts on any channel; an avatar on
        // channel 0 as chat, on any other its attachments' scripts.
        let heard = Heard {
            channel: speech.channel,
            name,
            speaker: agent.uuid().to_string(),
            message: speech.text.clone(),
        };
        if let Some(local_id) = self.prim_keyed(target) {
            let prim = self.world.objects.get(local_id)?.full_id;
            let by = self.scripts_in(prim);
            return (!by.is_empty()).then_some(Overheard { heard, by });
        }
        let to = Self::agent_named(target)?;
        if speech.channel == 0 {
            self.deliveries.push(Delivery::Chat {
                agent: to,
                line: Line {
                    chat_type: ChatType::Normal,
                    ..line
                },
            });
            return None;
        }
        let by: Vec<CallerId> = self
            .world
            .objects
            .worn_by(to)
            .into_iter()
            .flat_map(|root| super::linkset_scripts(self.world, root))
            .collect();
        (!by.is_empty()).then_some(Overheard { heard, by })
    }

    /// How the avatar or NPC keyed `agent` looks.
    pub(super) fn appearance_of(&self, agent: &str) -> Option<Appearance> {
        let key = Self::agent_named(agent)?;
        if let Some(scripted) = self.world.script_instances.npcs.get(key) {
            return Some(scripted.appearance.clone());
        }
        self.present(agent)?;
        let resident = self.world.resident(key)?;
        Some(resident_appearance(self.world, key, resident))
    }

    /// The health of the avatar or NPC keyed `agent`.
    pub(super) fn vitals_of(&self, agent: &str) -> Option<Vitals> {
        let key = Self::agent_named(agent)?;
        let here = self.present(agent).is_some() || self.world.script_instances.npcs.is_npc(key);
        here.then(|| {
            self.world
                .script_instances
                .npcs
                .vitals
                .get(&key)
                .copied()
                .unwrap_or_default()
        })
    }

    /// Keeps `vitals` for the avatar or NPC keyed `agent`, an avatar's viewer
    /// told its health.
    pub(super) fn keep_vitals(&mut self, agent: &str, vitals: Vitals) {
        let Some(key) = Self::agent_named(agent) else {
            return;
        };
        let npcs = &mut self.world.script_instances.npcs;
        let _was = npcs.vitals.insert(key, vitals);
        let _told = npcs.told.insert(key, vitals.health);
        if !npcs.is_npc(key) {
            self.deliveries.push(Delivery::Health {
                agent: key,
                health: vitals.health,
            });
        }
    }

    /// `osSetSpeed` of an NPC.
    pub(super) fn speed_npc(&mut self, agent: AgentKey, factor: f32) {
        if let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) {
            scripted.speed = factor;
        }
    }

    /// A teleport within the region of the NPC `agent`: put at `position`,
    /// stood up first, its walk given up.
    pub(super) fn place_npc(
        &mut self,
        agent: AgentKey,
        position: &Vector,
        velocity: Option<&Vector>,
    ) {
        if self
            .world
            .resident(agent)
            .is_some_and(|resident| resident.body.parent_id.0 != 0)
        {
            self.stand_npc(agent);
        }
        if let Some(scripted) = self.world.script_instances.npcs.get_mut(agent) {
            scripted.walk = None;
        }
        self.world.objects.move_wearer(agent, position);
        let tell = self.world.resident_mut(agent).map(|resident| {
            resident.at = position.clone();
            resident.body.motion.position = position.clone();
            if let Some(velocity) = velocity {
                resident.body.motion.velocity = velocity.clone();
            }
            (resident.body.local_id, resident.body.motion.clone())
        });
        if let Some((local_id, motion)) = tell {
            self.tell_moved(agent, local_id, motion);
        }
    }

    /// The region's NPCs as a sensor's sweep weighs them.
    pub(super) fn npcs_sensed(&self) -> Vec<sl_lsl_runtime::vm::sensing::Sensed> {
        self.world
            .script_instances
            .npcs
            .npcs
            .iter()
            .filter_map(|npc| {
                let resident = self.world.resident(npc.agent)?;
                let key = npc.agent.uuid().to_string();
                Some(sl_lsl_runtime::vm::sensing::Sensed {
                    key: key.clone(),
                    name: npc.name.clone(),
                    username: npc.name.replace(' ', ".").to_lowercase(),
                    owner: npc
                        .owner
                        .map_or_else(|| key.clone(), |owner| owner.uuid().to_string()),
                    group: uuid::Uuid::nil().to_string(),
                    position: resident.at.clone(),
                    rotation: resident.body.motion.rotation.clone(),
                    velocity: resident.body.motion.velocity.clone(),
                    agent: true,
                    physical: true,
                    scripted: false,
                    npc: Some(npc.sense_as_agent),
                })
            })
            .collect()
    }

    /// What `llGetObjectDetails` reads of the NPC keyed `npc`.
    pub(super) fn npc_details(&self, npc: &str) -> Option<sl_lsl_runtime::vm::prim::ObjectDetails> {
        let agent = self.npc_keyed(npc)?;
        let scripted = self.world.script_instances.npcs.get(agent)?;
        let resident = self.world.resident(agent)?;
        let key = agent.uuid().to_string();
        let mut details = self.avatar_like_details(&key, &scripted.name, &resident.at);
        details.rotation = resident.body.motion.rotation.clone();
        details.velocity = resident.body.motion.velocity.clone();
        details.owner = scripted
            .owner
            .map_or_else(|| key.clone(), |owner| owner.uuid().to_string());
        Some(details)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn facing_turns_about_the_vertical() {
        let east = facing(1.0, 0.0);
        assert!((east.s - 1.0).abs() < 1e-6 && east.z.abs() < 1e-6);
        let north = facing(0.0, 1.0);
        assert!((north.z - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((north.s - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn a_record_carries_the_appearances_textures_and_parameters() {
        let agent = AgentKey::from(uuid::Uuid::from_u128(7));
        let mut appearance = default_appearance();
        let bake = "11111111-1111-1111-1111-111111111111";
        if let Some(slot) = appearance.textures.get_mut(8) {
            *slot = bake.to_owned();
        }
        let record = appearance_record(agent, &appearance);
        assert_eq!(record.visual_params, DEFAULT_VISUAL_PARAMS.to_vec());
        assert_eq!(
            record
                .texture_entry
                .faces
                .get(8)
                .map(|face| face.texture_id.uuid().to_string()),
            Some(bake.to_owned())
        );
    }
}
