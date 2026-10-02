//! [`WorldModel`]: the in-world things the viewer tracks, read as
//! [`WorldNode`]s on request.
//!
//! Everything here is already kept by the world layers — the object graph
//! ([`ObjectState`]), the avatar mirror ([`AvatarState`]), the edit selection,
//! the name tags — except the one thing the object stream does not carry: an
//! object's name, description and owner. Those arrive in separate property
//! replies, which [`ObjectFacts`] collects.

use std::collections::HashMap;

use bevy::ecs::system::{SystemParam, SystemParamValidationError, SystemState};
use bevy::prelude::*;
use sl_automation_proto::{WorldKind, WorldNode};
use sl_client_bevy::{
    AgentKey, ObjectKey, ScopedObjectId, SlEvent, SlIdentity, SlSessionEvent, Uuid, pcode,
};
use sl_viewer_kit::coords::{bevy_to_sl_vec, region_offset_bevy, sl_to_bevy_rotation};
use sl_viewer_world_api::{
    AvatarState, MAX_PARENT_WALK, ObjectState, SelectionSet, TagContent, TrackedObject,
    is_hud_point,
};

/// Collects what [`ObjectFacts`] needs from the session's events. Added by
/// whoever installs automation; everything else the world model reads is kept
/// by the world layers anyway.
#[derive(Debug, Default)]
pub struct WorldModelPlugin;

impl Plugin for WorldModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ObjectFacts>()
            .add_message::<SlEvent>()
            .add_systems(Update, record_object_facts);
    }
}

/// An object's name, description and owner, as its last property reply said.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectFact {
    /// The object's name.
    name: String,
    /// The object's description.
    description: String,
    /// The owner: an agent, or a group for a deeded object.
    owner: Uuid,
}

/// The name, description and owner of every object a property reply has
/// described, keyed by its full id.
///
/// The object stream places an object but does not name it: a name arrives
/// only in an `ObjectProperties` reply (to a selection) or an
/// `ObjectPropertiesFamily` reply (to a hover, a pay dialog — or a world
/// locator that asked). Every reply is kept, whoever asked for it, so a
/// locator rarely has to ask twice. An entry outlives its object; a full id is
/// never reused, so a stale one is only unread memory.
#[derive(Debug, Default, Resource)]
pub struct ObjectFacts {
    /// What is known, by full object id.
    facts: HashMap<ObjectKey, ObjectFact>,
}

impl ObjectFacts {
    /// Record what a reply said about `object`, replacing what an older one
    /// said.
    fn record(&mut self, object: ObjectKey, name: &str, description: &str, owner: Uuid) {
        let _previous = self.facts.insert(
            object,
            ObjectFact {
                name: name.to_owned(),
                description: description.to_owned(),
                owner,
            },
        );
    }
}

/// Fold every property reply of the frame into [`ObjectFacts`]: the full and
/// the family replies, and an object update that carries properties the
/// session merged into its cached object.
fn record_object_facts(mut events: MessageReader<SlEvent>, mut facts: ResMut<ObjectFacts>) {
    for SlEvent(event) in events.read() {
        match event {
            SlSessionEvent::ObjectProperties(properties) => facts.record(
                properties.object_id,
                &properties.name,
                &properties.description,
                properties.owner.uuid(),
            ),
            SlSessionEvent::ObjectPropertiesFamily { properties } => facts.record(
                properties.object_id,
                &properties.name,
                &properties.description,
                properties.owner.uuid(),
            ),
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                if let Some(properties) = &object.properties {
                    facts.record(
                        object.full_id,
                        &properties.name,
                        &properties.description,
                        properties.owner.uuid(),
                    );
                }
            }
            _ => {}
        }
    }
}

/// Take a snapshot of every in-world thing `world` tracks: avatars first (the
/// own one leading), then objects by region-local id.
///
/// For a caller holding the world exclusively; a system takes a
/// [`WorldModel`] instead.
///
/// # Errors
///
/// When the model's parameters fail validation. Every resource it reads is
/// optional, so that would be Bevy refusing to run it at all.
pub fn world_snapshot(world: &mut World) -> Result<Vec<WorldNode>, SystemParamValidationError> {
    let mut state = SystemState::<WorldModel<'_, '_>>::new(world);
    let model = state.get(world)?;
    Ok(model.snapshot())
}

/// The world model: the in-world things the viewer tracks, read from the ECS
/// when asked and never per frame.
///
/// Every resource is optional, so a partial app (a UI test with no world)
/// reads an empty world rather than failing.
#[derive(SystemParam)]
#[expect(
    missing_debug_implementations,
    reason = "a bundle of resources and queries; its Debug would print query state, not the world"
)]
pub struct WorldModel<'w, 's> {
    /// The own agent and its region.
    identity: Option<Res<'w, SlIdentity>>,
    /// The object graph.
    objects: Option<Res<'w, ObjectState>>,
    /// The avatar mirror.
    avatars: Option<Res<'w, AvatarState>>,
    /// The edit selection.
    selection: Option<Res<'w, SelectionSet>>,
    /// Object names, descriptions and owners.
    facts: Option<Res<'w, ObjectFacts>>,
    /// Where things are drawn.
    transforms: Query<'w, 's, &'static GlobalTransform>,
    /// What name tags say.
    tags: Query<'w, 's, &'static TagContent>,
}

impl WorldModel<'_, '_> {
    /// Every tracked thing: avatars first, the own one leading and the rest
    /// by agent id, then objects and attachments by region-local id.
    #[must_use]
    pub fn snapshot(&self) -> Vec<WorldNode> {
        let mut nodes = self.avatar_nodes();
        nodes.extend(self.object_nodes());
        nodes
    }

    /// The own agent's id, once logged in.
    fn own_agent(&self) -> Option<AgentKey> {
        self.identity
            .as_ref()
            .and_then(|identity| identity.agent_id)
    }

    /// The Bevy-space offset of the agent's current region from the scene
    /// origin: subtracted from a drawn position to make it region-local.
    fn region_offset(&self) -> Vec3 {
        let handle = self
            .identity
            .as_ref()
            .and_then(|identity| identity.region_handle);
        let origin = self.objects.as_ref().and_then(|objects| objects.origin);
        handle.map_or(Vec3::ZERO, |handle| region_offset_bevy(handle, origin))
    }

    /// Where `entity` is drawn, as a region-local position and rotation.
    fn placement(&self, entity: Entity) -> Option<([f32; 3], [f32; 4])> {
        let transform = self.transforms.get(entity).ok()?;
        let (_scale, rotation, translation) = transform.to_scale_rotation_translation();
        let offset = self.region_offset();
        let local = bevy_to_sl_vec(Vec3::new(
            translation.x - offset.x,
            translation.y - offset.y,
            translation.z - offset.z,
        ));
        Some(([local.x, local.y, local.z], region_rotation(rotation)))
    }

    /// The avatars: every full-object avatar, and every avatar known only
    /// from the coarse locations.
    fn avatar_nodes(&self) -> Vec<WorldNode> {
        let Some(avatars) = self.avatars.as_deref() else {
            return Vec::new();
        };
        let own = self.own_agent();
        let scoped_of: HashMap<AgentKey, ScopedObjectId> = avatars
            .by_scoped
            .iter()
            .map(|(scoped, agent)| (*agent, *scoped))
            .collect();
        let mut agents: Vec<(AgentKey, Entity, Entity)> = avatars.labelled_avatars().collect();
        agents.sort_by_key(|(agent, _anchor, _label)| (Some(*agent) != own, agent.uuid()));
        agents
            .into_iter()
            .map(|(agent, anchor, label)| {
                let scoped = scoped_of.get(&agent).copied();
                // A full-object avatar is placed by its object entity, which
                // carries the region basis every object does; the anchor (a
                // sphere or a body root) is drawn in its own. A coarse dot has
                // only its anchor, and no rotation anyone sent.
                let tracked_entity = scoped.and_then(|scoped| {
                    self.objects
                        .as_deref()
                        .and_then(|objects| objects.entity_by_scoped(&scoped))
                });
                let placement = match tracked_entity {
                    Some(entity) => self
                        .placement(entity)
                        .map(|(position, rotation)| (position, Some(rotation))),
                    None => self
                        .placement(anchor)
                        .map(|(position, _rotation)| (position, None)),
                };
                let mut children: Vec<u32> = match (scoped, self.objects.as_deref()) {
                    (Some(scoped), Some(objects)) => objects
                        .children_of(&scoped)
                        .iter()
                        .map(|child| child.id.0)
                        .collect(),
                    _ => Vec::new(),
                };
                children.sort_unstable();
                WorldNode {
                    kind: WorldKind::Avatar,
                    own: Some(agent) == own,
                    full_id: agent.uuid(),
                    local_id: scoped.map(|scoped| scoped.id.0),
                    pcode: pcode::AVATAR,
                    name: avatars.shown_name_of(agent).map(ToOwned::to_owned),
                    description: None,
                    owner: None,
                    position: placement.map(|(position, _rotation)| position),
                    rotation: placement.and_then(|(_position, rotation)| rotation),
                    scale: None,
                    parent: None,
                    children,
                    attachment_point: None,
                    worn_by: None,
                    sitting_on: avatars.seated.get(&agent).map(|seated| seated.seat.id.0),
                    selected: false,
                    hover_text: None,
                    name_tag: self.tag_text(label),
                    bakes: avatars.baked_texture_ids(agent),
                }
            })
            .collect()
    }

    /// What the name tag `label` says, its lines joined by newlines.
    fn tag_text(&self, label: Entity) -> Option<String> {
        let content = self.tags.get(label).ok()?;
        (!content.lines.is_empty()).then(|| {
            content
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// The objects and attachments: every tracked object that is not an
    /// avatar's own object.
    fn object_nodes(&self) -> Vec<WorldNode> {
        let Some(objects) = self.objects.as_deref() else {
            return Vec::new();
        };
        let own = self.own_agent();
        let mut tracked: Vec<(&ScopedObjectId, &TrackedObject)> = objects
            .objects()
            .iter()
            .filter(|(scoped, tracked)| {
                tracked.shape.pcode() != pcode::AVATAR
                    && self
                        .avatars
                        .as_deref()
                        .is_none_or(|avatars| !avatars.by_scoped.contains_key(scoped))
            })
            .collect();
        tracked.sort_by_key(|(scoped, tracked)| (scoped.id.0, tracked.full_key.uuid()));
        tracked
            .into_iter()
            .map(|(scoped, tracked)| {
                let wearer = self
                    .avatars
                    .as_deref()
                    .and_then(|avatars| avatars.wearer_of(*scoped));
                let attachment_point = attachment_point_of(objects, scoped, tracked);
                let on_hud = attachment_point.is_some_and(is_hud_point);
                let placement = (!on_hud).then(|| self.placement(tracked.entity)).flatten();
                let fact = self
                    .facts
                    .as_deref()
                    .and_then(|facts| facts.facts.get(&tracked.full_key));
                let mut children: Vec<u32> = objects
                    .children_of(scoped)
                    .iter()
                    .map(|child| child.id.0)
                    .collect();
                children.sort_unstable();
                WorldNode {
                    kind: if wearer.is_some() {
                        WorldKind::Attachment
                    } else {
                        WorldKind::Object
                    },
                    own: wearer.is_some() && wearer == own,
                    full_id: tracked.full_key.uuid(),
                    local_id: Some(scoped.id.0),
                    pcode: tracked.shape.pcode(),
                    name: fact.map(|fact| fact.name.clone()),
                    description: fact.map(|fact| fact.description.clone()),
                    owner: fact.map(|fact| fact.owner),
                    position: placement.map(|(position, _rotation)| position),
                    rotation: placement.map(|(_position, rotation)| rotation),
                    scale: Some([tracked.scale.x, tracked.scale.y, tracked.scale.z]),
                    parent: (!tracked.is_root).then_some(tracked.parent.id.0),
                    children,
                    attachment_point,
                    worn_by: wearer.map(|wearer| wearer.uuid()),
                    sitting_on: None,
                    selected: self
                        .selection
                        .as_deref()
                        .is_some_and(|selection| selection.is_selected(*scoped)),
                    hover_text: (!tracked.text.is_empty()).then(|| tracked.text.clone()),
                    name_tag: None,
                    bakes: Vec::new(),
                }
            })
            .collect()
    }
}

/// The attachment point `tracked` is worn on: its own, or — for a child prim
/// of an attachment — its linkset root's.
fn attachment_point_of(
    objects: &ObjectState,
    scoped: &ScopedObjectId,
    tracked: &TrackedObject,
) -> Option<u8> {
    let mut current = (*scoped, tracked);
    for _hop in 0..MAX_PARENT_WALK {
        let (scoped, tracked) = current;
        if let Some(point) = tracked.attachment_point {
            return Some(point);
        }
        if tracked.is_root || tracked.parent == scoped {
            return None;
        }
        let parent = objects.objects().get(&tracked.parent)?;
        current = (tracked.parent, parent);
    }
    None
}

/// A Bevy world rotation as a region rotation `[x, y, z, w]`: the basis
/// change every drawn thing carries undone, and the sign pinned so `w` is not
/// negative (a quaternion and its negation are the same rotation, and a test
/// comparing two should not see a difference).
fn region_rotation(world: Quat) -> [f32; 4] {
    let sl = sl_to_bevy_rotation().inverse().mul_quat(world);
    if sl.w < 0.0 {
        [-sl.x, -sl.y, -sl.z, -sl.w]
    } else {
        [sl.x, sl.y, sl.z, sl.w]
    }
}
