//! The object half of a client's edits: everything the build floater changes
//! about an in-world object other than its shape, its textures and its
//! contents.
//!
//! Until `test-fake-grid-edit-surfaces` the fake grid had exactly three writes
//! — a rez, a derez, and a drop into a prim's task inventory — and every other
//! edit was decoded by [`SimSession`] and dropped. That made "did my edit reach
//! the grid" a question no tier below a live grid could answer, and made the
//! region a test looks at always the region its fixture stated.
//!
//! Two stores are edited here and they are pushed to the client by **different
//! messages**, which is the thing to keep straight:
//!
//! - the [`Object`] itself — its motion, scale, material, click action and
//!   flags — which travels in an `ObjectUpdate`, so a change to it is streamed
//!   to the whole region;
//! - its [`ObjectProperties`] — name, description, category, sale state,
//!   permissions, owner and group — which travel in a message of their own that
//!   a simulator sends to the clients holding the object *selected*. An
//!   `ObjectUpdate` carries none of those fields, so a client that renames an
//!   object learns the rename took only from that message.
//!
//! Only the first of the two reaches the region's *other* viewers. A change to
//! the object goes to everyone, because an `ObjectUpdate` needs no
//! subscription. An edit of the properties is answered to the session that
//! made it and to nobody else, on both live grids (`object-properties`,
//! 2026-10-09): a resident holding a prim selected while somebody else renames
//! it is not told. The one thing a selection does subscribe to, on Second Life
//! alone, is the record's contents serial — see [`Selection`].

use std::time::Instant;

use sl_proto::{
    Object, ObjectPhysicsData, ObjectProperties, PhysicsShapeType, RegionLocalObjectId,
    ServerEvent, SimSession, prim_flags,
};
use sl_types::key::{ObjectKey, OwnerKey};
use sl_types::lsl::{Rotation, Vector};

use crate::imitates::{
    CopiedSerial, EditPolicy, LinkPolicy, NoBuildAlert, Placement, RayAtNothing, RootRecord,
};
use crate::world::{RegionChange, SceneFixtures};

/// The physics record both live grids send for a prim nobody has changed the
/// physics of: the prim's own shape, the density of water, and the friction
/// and restitution of the default material.
const NEW_PRIM_PHYSICS: ObjectPhysicsData = ObjectPhysicsData {
    physics_shape_type: PhysicsShapeType::Prim,
    density: 1000.0,
    friction: 0.6,
    restitution: 0.5,
    gravity_multiplier: 1.0,
};

/// The objects one session holds selected.
///
/// Selection is a **subscription**, not a lock: Second Life has no edit lock,
/// and two residents may hold the same object selected indefinitely. What the
/// set decides is only who is *told* of a write into a prim's contents —
/// Second Life sends the record, with its advanced contents serial, to every
/// client holding the object selected. It is a narrow subscription: an edit
/// of the record itself is told to nobody but its editor, on either grid, and
/// OpenSim publishes nothing to a selection at all
/// ([`PropertiesPolicy`](crate::imitates::PropertiesPolicy)).
pub(crate) type Selection = std::collections::BTreeSet<RegionLocalObjectId>;

/// Answers one drained [`ServerEvent`] that edits an object, returning the
/// [`RegionChange`]s the region's other sessions have to be told about — or
/// [`None`] when the event is not an object edit at all, which is how
/// [`answer_world_request`](crate::world::answer_world_request) knows to carry
/// on looking.
///
/// `mint` supplies the ids the simulator chooses (a duplicate's object keys),
/// so a seeded grid duplicates the same object twice. `selection` is this
/// session's own — the objects it holds selected, which a select opens and a
/// deselect closes, and which the region watcher reads to decide whether a
/// properties change published by another session is any of this client's
/// business.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per edit message, each a handful of lines; splitting the \
              match would only move the arms somewhere they are harder to \
              compare against the message family they answer"
)]
pub(crate) fn answer_object_edit(
    world: &mut SceneFixtures,
    mint: &dyn Fn() -> uuid::Uuid,
    selection: &mut Selection,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    match event {
        // ----- edits to the properties record ------------------------------
        // The two edits OpenSim stores without a word; Second Life answers
        // them as it answers every other.
        ServerEvent::ObjectNameSet { local_id, name } => {
            let answer = world.properties.rename_answered.then_some((sim, now));
            let kept = world.edit.kept_text(name, EditPolicy::NAME_BYTES);
            edit_properties(world, *local_id, answer, |properties| {
                properties.name = kept;
            });
        }
        ServerEvent::ObjectDescriptionSet {
            local_id,
            description,
        } => {
            let answer = world.properties.rename_answered.then_some((sim, now));
            let kept = world
                .edit
                .kept_text(description, EditPolicy::DESCRIPTION_BYTES);
            edit_properties(world, *local_id, answer, |properties| {
                properties.description = kept;
            });
        }
        ServerEvent::ObjectSaleInfoSet {
            local_id,
            sale_type,
            sale_price,
        } => {
            edit_properties(world, *local_id, Some((sim, now)), |properties| {
                properties.sale_type = sale_type.to_code();
                properties.sale_price.clone_from(sale_price);
            });
        }
        // A grant sets the named bits and a revoke clears them: the message
        // carries the bits being *changed*, not the mask's new value — and
        // which of them an owner may change is the grid's
        // ([`EditPolicy::edit_masks`]). What each
        // viewer may do with the object follows from the masks and travels in
        // its update flags, so the object goes out again too — to each viewer
        // with its own.
        ServerEvent::ObjectPermissionsSet {
            local_id,
            field,
            set,
            mask,
            ..
        } => {
            let policy = world.edit;
            edit_properties(world, *local_id, Some((&mut *sim, now)), |properties| {
                policy.edit_masks(&mut properties.permissions, *field, *set, *mask);
            });
            let mut changes = Vec::new();
            if let Some(object) = world.object_by_local_id(*local_id) {
                push_object(sim, &object, now);
                changes.push(RegionChange::Updated(Box::new(object)));
            }
            return Some(changes);
        }
        // Two edits that change nothing. A category is a field no viewer has
        // set in years, and neither grid takes it: a record reads zero
        // afterwards on both. And a group an object is set to, like a deed to
        // it, is for an agent that is in the group — the fake grid has no
        // groups, so its agents are in none: the object keeps the group it
        // had and stays its owner's, as on both live grids, and Second Life
        // says of the deed why.
        ServerEvent::ObjectCategorySet { .. } | ServerEvent::ObjectGroupSet { .. } => {}
        ServerEvent::ObjectOwnerSet { .. } => {
            if let Some(alert) = world.edit.deed_refusal {
                send_alert(sim, alert, &[], now);
            }
        }
        // ----- edits to the object update itself ---------------------------
        ServerEvent::ObjectClickActionSet {
            local_id,
            click_action,
        } => {
            return Some(quiet_edit(world, *local_id, sim, now, |object| {
                object.click_action = click_action.to_code();
            }));
        }
        ServerEvent::ObjectMaterialSet { local_id, material } => {
            return Some(update_object(world, *local_id, sim, now, |object| {
                object.material = material.to_code();
            }));
        }
        ServerEvent::ObjectFlagsSet { local_id, flags } => {
            // Second Life refuses to make a set physical that costs its
            // physics more than 32; a cube costs one.
            let prims = crate::link_sets::prims_of(world, world.root_of(*local_id));
            if flags.use_physics
                && let Some(most_physical) = world.linking.most_physical_prims
                && prims > most_physical
            {
                send_alert(
                    sim,
                    LinkPolicy::TOO_MUCH_PHYSICS,
                    &[("MAX_OBJECTS", most_physical)],
                    now,
                );
                return Some(Vec::new());
            }
            return Some(update_object(world, *local_id, sim, now, |object| {
                set_flag(object, prim_flags::USE_PHYSICS, flags.use_physics);
                set_flag(object, prim_flags::PHANTOM, flags.is_phantom);
                set_flag(object, prim_flags::TEMPORARY_ON_REZ, flags.is_temporary);
                // `casts_shadows` has nowhere to go: the bit it used to set
                // (1 << 23) has been unused for years, and the reference sends
                // the field only because the message has always had it.
            }));
        }
        ServerEvent::ObjectIncludeInSearchSet {
            local_id,
            include_in_search,
        } => {
            return Some(quiet_edit(world, *local_id, sim, now, |object| {
                set_flag(object, prim_flags::INCLUDE_IN_SEARCH, *include_in_search);
            }));
        }
        // The three edits that were typed before this module existed and still
        // had nowhere to land. Each carries a decoded value *and* travels in the
        // update as a packed blob, so both halves are written: the typed field
        // is what a fixture and a test read, the blob is what
        // `full_update_block` copies onto the wire.
        ServerEvent::ObjectShapeSet { local_id, shape } => {
            return Some(update_object(world, *local_id, sim, now, |object| {
                object.shape.clone_from(shape);
            }));
        }
        ServerEvent::ObjectImageSet {
            local_id,
            media_url,
            texture_entry,
        } => {
            // The legacy media URL rides along on the retexture and reaches the
            // grid as a string; a viewer that sends one that will not parse has
            // set no media, and saying so is better than storing a URL nothing
            // can fetch.
            let media = media_url.as_ref().and_then(|url| match url.parse() {
                Ok(parsed) => Some(parsed),
                Err(error) => {
                    tracing::debug!(
                        "an ObjectImage carried the unparsable media URL {url}: {error}"
                    );
                    None
                }
            });
            return Some(update_object(world, *local_id, sim, now, |object| {
                object.media_url = media;
                object.texture_entry = sl_proto::encode_texture_entry(texture_entry);
            }));
        }
        ServerEvent::ObjectExtraParamsSet { local_id, params } => {
            return Some(update_object(world, *local_id, sim, now, |object| {
                object.extra_params = sl_proto::encode_extra_params(params);
                object.extra.clone_from(params);
            }));
        }
        // A move, a rotate or a resize. `group` means "the whole linkset", and
        // a linkset moves by its root: a child's position on the wire is
        // relative to that root, so moving the root moves them all and moving
        // the child alone is exactly what `group` is clear for.
        ServerEvent::ObjectTransformSet {
            local_id,
            transform,
        } => {
            let target = if transform.group {
                world.root_of(*local_id)
            } else {
                *local_id
            };
            let policy = world.edit;
            let whole = transform.position.is_some()
                && transform.rotation.is_some()
                && transform.scale.is_some();
            if whole && !policy.takes_whole_transform {
                tracing::debug!("a move, a turn and a resize in one message is dropped");
                return Some(Vec::new());
            }
            // A place the grid does not take. An object that leaves the world
            // for it is the region's to file, which has the inventory and the
            // asset store this module does not.
            let placement = transform
                .position
                .as_ref()
                .map_or(Placement::Taken, |position| policy.placement(position));
            if placement == Placement::Returned {
                return None;
            }
            if let Placement::Refused { in_full } = placement {
                if let Some(object) = world.object_by_local_id(target) {
                    if in_full {
                        push_object(sim, &object, now);
                    } else {
                        push_terse(sim, &object, now);
                    }
                }
                return Some(Vec::new());
            }
            world.record_undo(target);
            let Some(changed) = edit_object(world, target, |object| {
                if let Some(position) = &transform.position {
                    object.motion.position.clone_from(position);
                }
                if let Some(rotation) = &transform.rotation {
                    object.motion.rotation.clone_from(rotation);
                }
                if let Some(scale) = &transform.scale {
                    object.scale = policy.kept_scale(scale);
                }
            }) else {
                tracing::debug!("a transform named {target:?}, which this region does not have");
                return Some(Vec::new());
            };
            // A move and a turn come back in a terse update on both grids, a
            // resize in a full one.
            if transform.scale.is_some() {
                push_object(sim, &changed, now);
            } else {
                push_terse(sim, &changed, now);
            }
            return Some(vec![RegionChange::Updated(Box::new(changed))]);
        }
        // ----- linking, duplicating, deleting ------------------------------
        // The first id is the root the rest are parented to (the reference
        // packs the selection root-first, and both grids read it that way).
        // What a link makes of the order, and which links it refuses, is
        // [`crate::link_sets`]'s.
        ServerEvent::ObjectsLinked { local_ids } => {
            let root = match crate::link_sets::link(world, local_ids) {
                Ok(Some(root)) => root,
                Ok(None) => return Some(Vec::new()),
                Err(refusal) => {
                    send_alert(sim, refusal.alert(), &refusal.parameters(), now);
                    return Some(Vec::new());
                }
            };
            let changes = resend_set(world, sim, root, now);
            // A link is followed by the root's record: always on OpenSim, and
            // on Second Life where the linker holds the root selected.
            let told = match world.linking.root_record {
                RootRecord::Always => true,
                RootRecord::WhenSelected => selection.contains(&root),
            };
            if told && let Some(record) = world.record_of(root) {
                push_properties(sim, &record, now);
            }
            return Some(changes);
        }
        // Everything named leaves its set. Every prim of a set that changed
        // is sent again, the ones now on their own among them.
        ServerEvent::ObjectsDelinked { local_ids } => {
            let allowed: Vec<RegionLocalObjectId> = local_ids
                .iter()
                .copied()
                .filter(|prim| crate::link_sets::may_delink(world, *prim))
                .collect();
            if allowed.len() != local_ids.len() {
                send_alert(sim, LinkPolicy::UNLINK_REFUSED, &[], now);
            }
            let mut changes = Vec::new();
            for root in crate::link_sets::delink(world, &allowed) {
                changes.extend(resend_set(world, sim, root, now));
            }
            return Some(changes);
        }
        // A copy is a rez the client did not describe: the simulator mints both
        // ids, so the duplicating client learns them the same way a rezzing one
        // does — from the `ObjectUpdate` that comes back.
        ServerEvent::ObjectsDuplicated {
            local_ids,
            offset,
            duplicate_flags,
            ..
        } => {
            let mut changes = Vec::new();
            for local_id in local_ids {
                let Some(original) = world.object_by_local_id(*local_id) else {
                    continue;
                };
                let at = offset_by(&original.motion.position, offset);
                changes.extend(copy_set(
                    world,
                    mint,
                    sim,
                    &original,
                    at,
                    *duplicate_flags,
                    now,
                ));
            }
            return Some(changes);
        }
        // A copy stood where a ray ends rather than at an offset. The fake
        // grid casts no ray: a copy aimed at a prim stands on top of it, which
        // is where both grids put one whose ray comes down on that prim, and
        // one aimed at nothing stands on the ray's end on Second Life and is
        // not made on OpenSim, which copies only onto a prim.
        ServerEvent::DuplicateObjectsOnRay {
            local_ids,
            ray_end,
            ray_target_id,
            duplicate_flags,
            ..
        } => {
            let mut changes = Vec::new();
            for local_id in local_ids {
                let Some(original) = world.object_by_local_id(*local_id) else {
                    continue;
                };
                let target = ray_target_id
                    .and_then(|target| world.local_id_of(target))
                    .and_then(|target| world.object_by_local_id(target));
                let at = match target {
                    Some(target) => Vector {
                        x: target.motion.position.x,
                        y: target.motion.position.y,
                        z: target.motion.position.z
                            + f32::midpoint(target.scale.z, original.scale.z),
                    },
                    None if world.edit.ray_at_nothing == RayAtNothing::CopiesAtItsEnd => Vector {
                        x: ray_end.x,
                        y: ray_end.y,
                        z: ray_end.z + original.scale.z / 2.0,
                    },
                    None => continue,
                };
                changes.extend(copy_set(
                    world,
                    mint,
                    sim,
                    &original,
                    at,
                    *duplicate_flags,
                    now,
                ));
            }
            return Some(changes);
        }
        // The force-delete, which leaves nothing behind: no inventory item, no
        // trash. The ordinary delete-to-trash is a derez, and it is answered
        // where the takes are.
        ServerEvent::ObjectsDeleted { local_ids, .. } => {
            // OpenSim has no handler for the message: the objects stand.
            if world.rez.object_delete == crate::imitates::ObjectDelete::Ignored {
                return Some(Vec::new());
            }
            let mut killed = Vec::new();
            let mut changes = Vec::new();
            for local_id in local_ids {
                if world.remove_object(*local_id).is_some() {
                    changes.push(RegionChange::Killed(*local_id));
                }
                // A client that believes in an object this region does not have
                // is told to forget it either way, exactly as a derez of one is.
                killed.push(*local_id);
            }
            if !killed.is_empty()
                && let Err(error) = sim.send_kill_object(&killed, now)
            {
                tracing::warn!("killing a deleted object failed: {error}");
            }
            return Some(changes);
        }
        // ----- the undo stack ----------------------------------------------
        ServerEvent::ObjectsUndone { object_ids } => {
            return Some(step_history(world, sim, object_ids, now, true));
        }
        ServerEvent::ObjectsRedone { object_ids } => {
            return Some(step_history(world, sim, object_ids, now, false));
        }
        // ----- reading the record back -------------------------------------
        // A select is answered with the record and with the object's physics
        // record over the event queue — every time, whoever owns the object
        // and whether or not it was selected already. It is also remembered:
        // on Second Life a selection is the subscription a change to the
        // object's *contents* is told to ([`RegionChange::PropertiesChanged`]).
        // A local id the region does not have, and an avatar's, get nothing.
        ServerEvent::ObjectsSelected { local_ids } => {
            let agent = sim.agent_id();
            for local_id in local_ids {
                selection.insert(*local_id);
                let Some(object) = world.object_by_local_id(*local_id) else {
                    continue;
                };
                let Some(record) = world.record_of(*local_id) else {
                    continue;
                };
                let owned = agent.is_some_and(|agent| OwnerKey::Agent(agent) == record.owner);
                if world.properties.strangers_select_resends_object && !owned {
                    push_object(sim, &object, now);
                }
                push_properties(sim, &record, now);
                sim.enqueue_caps_event(
                    "ObjectPhysicsProperties",
                    sl_proto::build_object_physics_properties(&[(*local_id, NEW_PRIM_PHYSICS)]),
                );
            }
        }
        // The end of the subscription. Second Life answers it with a terse
        // update of the object; OpenSim with nothing.
        ServerEvent::ObjectsDeselected { local_ids } => {
            for local_id in local_ids {
                selection.remove(local_id);
                if !world.properties.deselect_sends_terse {
                    continue;
                }
                let Some(object) = world.object_by_local_id(*local_id) else {
                    continue;
                };
                let update = sl_proto::TerseUpdate {
                    local_id: *local_id,
                    state: object.state,
                    motion: object.motion,
                };
                if let Err(error) =
                    sim.send_terse_update(&[update], crate::world::REAL_TIME_DILATION, now)
                {
                    tracing::warn!("answering a deselect failed: {error}");
                }
            }
        }
        // The condensed form, which needs no selection: what a viewer shows on
        // hover and in the pay / report dialogs. Asked about a child prim,
        // both live grids answer with its linkset's root.
        ServerEvent::RequestObjectPropertiesFamily {
            request_flags,
            object_id,
        } => {
            let Some(local_id) = world.local_id_of(*object_id) else {
                tracing::debug!("properties were asked of {object_id}, which is not here");
                return Some(Vec::new());
            };
            let Some(properties) = world.properties_of(world.root_of(local_id)) else {
                return Some(Vec::new());
            };
            let family = sl_proto::ObjectPropertiesFamily {
                request_flags: *request_flags,
                object_id: properties.object_id,
                owner: properties.owner,
                group: properties.group,
                permissions: properties.permissions,
                ownership_cost: properties.ownership_cost.clone(),
                sale_type: properties.sale_type,
                sale_price: properties.sale_price.clone(),
                category: properties.category,
                last_owner_id: properties.last_owner_id,
                name: properties.name.clone(),
                description: properties.description.clone(),
            };
            if let Err(error) = sim.send_object_properties_family(&family, now) {
                tracing::warn!("answering an object properties family request failed: {error}");
            }
        }
        _other => return None,
    }
    // Every arm that falls through here read the region or edited an object's
    // record, and neither is anything its other sessions are told.
    Some(Vec::new())
}

/// Sets or clears `flag` in the object's `PrimFlags` field.
const fn set_flag(object: &mut Object, flag: u32, on: bool) {
    object.update_flags = if on {
        object.update_flags | flag
    } else {
        object.update_flags & !flag
    };
}

/// A placement stated in the region's frame, restated in `root`'s: what a
/// prim's position and rotation become when it is linked under `root`.
pub(crate) fn into_frame(
    root_position: &Vector,
    root_rotation: &Rotation,
    position: &Vector,
    rotation: &Rotation,
) -> (Vector, Rotation) {
    let inverse = conjugate(root_rotation);
    (
        rotate(&inverse, &difference(position, root_position)),
        multiply(&inverse, rotation),
    )
}

/// The inverse of [`into_frame`]: a child's placement in `root`'s frame,
/// restated in the region's, which is where a delinked prim goes back to.
pub(crate) fn out_of_frame(
    root_position: &Vector,
    root_rotation: &Rotation,
    position: &Vector,
    rotation: &Rotation,
) -> (Vector, Rotation) {
    (
        offset_by(root_position, &rotate(root_rotation, position)),
        multiply(root_rotation, rotation),
    )
}

/// The rotation that undoes `rotation` (its conjugate, which for the unit
/// quaternions a placement carries is also its inverse).
const fn conjugate(rotation: &Rotation) -> Rotation {
    Rotation {
        x: -rotation.x,
        y: -rotation.y,
        z: -rotation.z,
        s: rotation.s,
    }
}

/// `first` followed by `second`, in the order Second Life composes rotations
/// (`second * first` in Hamilton's product).
fn multiply(second: &Rotation, first: &Rotation) -> Rotation {
    Rotation {
        x: second.s.mul_add(
            first.x,
            second
                .x
                .mul_add(first.s, second.y.mul_add(first.z, -(second.z * first.y))),
        ),
        y: second.s.mul_add(
            first.y,
            second
                .y
                .mul_add(first.s, second.z.mul_add(first.x, -(second.x * first.z))),
        ),
        z: second.s.mul_add(
            first.z,
            second
                .z
                .mul_add(first.s, second.x.mul_add(first.y, -(second.y * first.x))),
        ),
        s: second.s.mul_add(
            first.s,
            -second
                .x
                .mul_add(first.x, second.y.mul_add(first.y, second.z * first.z)),
        ),
    }
}

/// `vector` turned by `rotation` (`q * v * q⁻¹`, written out).
pub(crate) fn rotate(rotation: &Rotation, vector: &Vector) -> Vector {
    let as_vector = Rotation {
        x: vector.x,
        y: vector.y,
        z: vector.z,
        s: 0.0,
    };
    let turned = multiply(&multiply(rotation, &as_vector), &conjugate(rotation));
    Vector {
        x: turned.x,
        y: turned.y,
        z: turned.z,
    }
}

/// `position` less `origin`.
fn difference(position: &Vector, origin: &Vector) -> Vector {
    Vector {
        x: position.x - origin.x,
        y: position.y - origin.y,
        z: position.z - origin.z,
    }
}

/// `position` moved by `offset` metres.
fn offset_by(position: &Vector, offset: &Vector) -> Vector {
    Vector {
        x: position.x + offset.x,
        y: position.y + offset.y,
        z: position.z + offset.z,
    }
}

/// Applies `edit` to the object's stored [`ObjectProperties`] and answers the
/// editing client with the new record where `answer` says there is an answer.
///
/// Nobody else is told. Neither live grid sends an edited record to another
/// session, whether or not it holds the object selected: a resident looking
/// at a prim somebody else renames keeps the old name until they select it
/// again.
fn edit_properties(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    answer: Option<(&mut SimSession, Instant)>,
    edit: impl FnOnce(&mut ObjectProperties),
) {
    let Some(current) = world.properties_of(local_id) else {
        tracing::debug!("an object edit named {local_id:?}, which this region does not have");
        return;
    };
    let Some(object) = world.object_mut(local_id) else {
        return;
    };
    let mut properties = current;
    edit(&mut properties);
    object.properties = Some(properties);
    let Some((sim, now)) = answer else {
        return;
    };
    // Re-read rather than pushing what was just written: the contents serial a
    // client reads off the record lives with the task inventory, not with the
    // object, and a child prim's record states its root's masks.
    if let Some(record) = world.record_of(local_id) {
        push_properties(sim, &record, now);
    }
}

/// Applies `edit` to the object itself and returns the changed object.
fn edit_object(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    edit: impl FnOnce(&mut Object),
) -> Option<Object> {
    let object = world.object_mut(local_id)?;
    edit(object);
    Some(object.clone())
}

/// [`edit_object`], then the two things every change to an object update owes:
/// the editing client its own copy, and the region the change to broadcast.
fn update_object(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    sim: &mut SimSession,
    now: Instant,
    edit: impl FnOnce(&mut Object),
) -> Vec<RegionChange> {
    let Some(changed) = edit_object(world, local_id, edit) else {
        tracing::debug!("an object edit named {local_id:?}, which this region does not have");
        return Vec::new();
    };
    push_object(sim, &changed, now);
    vec![RegionChange::Updated(Box::new(changed))]
}

/// An edit of the object that OpenSim stores and tells nobody of — a click
/// action, the search flag — and that Second Life answers like any other.
fn quiet_edit(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    sim: &mut SimSession,
    now: Instant,
    edit: impl FnOnce(&mut Object),
) -> Vec<RegionChange> {
    if world.edit.quiet_edits_resend {
        return update_object(world, local_id, sim, now, edit);
    }
    if edit_object(world, local_id, edit).is_none() {
        tracing::debug!("an object edit named {local_id:?}, which this region does not have");
    }
    Vec::new()
}

/// Sends the client the alert a refused edit is answered with — its
/// `AlertInfo` stating `parameters`, as the map of integers Second Life
/// states them in — logging a send failure rather than failing the edit.
fn send_alert(
    sim: &mut SimSession,
    alert: NoBuildAlert,
    parameters: &[(&str, usize)],
    now: Instant,
) {
    let extra_params = if parameters.is_empty() {
        String::new()
    } else {
        let entries: Vec<String> = parameters
            .iter()
            .map(|(key, value)| {
                [
                    "<key>",
                    key,
                    "</key><integer>",
                    &value.to_string(),
                    "</integer>",
                ]
                .concat()
            })
            .collect();
        let entries = entries.concat();
        format!("<? LLSD/XML ?>\n<llsd><map>{entries}</map></llsd>\n")
    };
    let info: Vec<sl_proto::AlertInfo> = alert
        .key
        .map(|key| sl_proto::AlertInfo {
            message: key.to_owned(),
            extra_params: extra_params.clone(),
        })
        .into_iter()
        .collect();
    if let Err(error) = sim.send_alert_message(alert.text, &info, &[], now) {
        tracing::warn!("refusing an edit failed: {error}");
    }
}

/// Streams every prim of the set `root` is the root of to the client, in the
/// order the grid being imitated sends a set in, and returns the changes the
/// region's other sessions are owed.
fn resend_set(
    world: &SceneFixtures,
    sim: &mut SimSession,
    root: RegionLocalObjectId,
    now: Instant,
) -> Vec<RegionChange> {
    crate::link_sets::sent_in_order(world, root)
        .into_iter()
        .filter_map(|prim| world.object_by_local_id(prim))
        .map(|object| {
            push_object(sim, &object, now);
            RegionChange::Updated(Box::new(object))
        })
        .collect()
}

/// [`copy_object`] of `original` and, where it is the root of a linkset, of
/// every prim under it: a copy of a root is a copy of its set, in the set's
/// order.
fn copy_set(
    world: &mut SceneFixtures,
    mint: &dyn Fn() -> uuid::Uuid,
    sim: &mut SimSession,
    original: &Object,
    at: Vector,
    duplicate_flags: u32,
    now: Instant,
) -> Vec<RegionChange> {
    let children: Vec<Object> = crate::link_sets::children_in_order(world, original.local_id)
        .into_iter()
        .filter_map(|child| world.object_by_local_id(child))
        .collect();
    let root = copy_object(world, mint, sim, original, at, None, duplicate_flags, now);
    let RegionChange::Rezzed(copied) = &root else {
        return vec![root];
    };
    let copied_root = copied.local_id;
    let mut changes = vec![root];
    let mut copied_children = Vec::with_capacity(children.len());
    for child in &children {
        // A child stands where its update says, in its root's frame.
        let change = copy_object(
            world,
            mint,
            sim,
            child,
            child.motion.position.clone(),
            Some(copied_root),
            duplicate_flags,
            now,
        );
        if let RegionChange::Rezzed(copy) = &change {
            copied_children.push(copy.local_id);
        }
        changes.push(change);
    }
    crate::link_sets::note_copy(world, copied_root, copied_children);
    changes
}

/// Makes a copy of `original` standing at `at` — under `parent`, for a copy of
/// a child prim — with its record, its contents and its creation date,
/// streams it to the client and returns the change the region's other
/// sessions are owed.
///
/// The copy's last owner is the agent that made it, on both grids — also
/// where the original, a prim nobody has taken, names none.
#[expect(
    clippy::too_many_arguments,
    reason = "the world and what mints for it, the session, and the four things a copy is made from"
)]
fn copy_object(
    world: &mut SceneFixtures,
    mint: &dyn Fn() -> uuid::Uuid,
    sim: &mut SimSession,
    original: &Object,
    at: Vector,
    parent: Option<RegionLocalObjectId>,
    duplicate_flags: u32,
    now: Instant,
) -> RegionChange {
    let mut copy = original.clone();
    copy.local_id = world.mint_local_id();
    copy.full_id = ObjectKey::from(mint());
    copy.motion.position = at;
    if let Some(parent) = parent {
        copy.parent_id = parent;
    }
    let mut record = world.properties_of(original.local_id);
    if let Some(record) = record.as_mut() {
        record.object_id = copy.full_id;
        if let Some(agent) = sim.agent_id() {
            record.last_owner_id = agent.uuid();
        }
    }
    copy.properties = record;
    if let Some(contents) = world.task_inventories.get(&original.local_id).cloned() {
        let serial = match world.edit.copy_contents_serial {
            CopiedSerial::Originals => contents.serial,
            CopiedSerial::Advanced => contents.serial.saturating_add(2),
        };
        let items = contents
            .items
            .into_iter()
            .map(|item| sl_proto::TaskInventoryItem {
                item_id: sl_types::key::InventoryKey::from(mint()),
                parent_task: copy.full_id,
                ..item
            })
            .collect();
        let _none = world.task_inventories.insert(
            copy.local_id,
            crate::world::TaskInventory::stated(serial, items),
        );
    }
    world.objects.push(copy.clone());
    // The flag is for the session that asked for the copy selected, and is
    // not the object's: nobody else is sent it.
    let mut sent = copy.clone();
    if world.edit.copy_flagged_selected && duplicate_flags & prim_flags::CREATE_SELECTED != 0 {
        sent.update_flags |= prim_flags::CREATE_SELECTED;
    }
    push_object(sim, &sent, now);
    RegionChange::Rezzed(Box::new(copy))
}

/// Steps each named object one place along its history of moves, turns and
/// resizes — back on an undo, forward on a redo — and streams whatever that
/// restored: in a full update where the object's size changed, in a terse one
/// otherwise.
///
/// The named objects are full ids, not region-local ones: the `Undo` / `Redo`
/// messages are the one place in the object family where that is true.
fn step_history(
    world: &mut SceneFixtures,
    sim: &mut SimSession,
    object_ids: &[ObjectKey],
    now: Instant,
    backwards: bool,
) -> Vec<RegionChange> {
    let mut changes = Vec::new();
    for object_id in object_ids {
        let Some(local_id) = world.local_id_of(*object_id) else {
            tracing::debug!("an undo named {object_id}, which this region does not have");
            continue;
        };
        let Some((before, restored)) = world.step_history(local_id, backwards) else {
            // Nothing left to undo is not an error: the viewer's own undo stack
            // is deeper than the simulator's, and it keeps asking.
            continue;
        };
        if before.scale == restored.scale {
            push_terse(sim, &restored, now);
        } else {
            push_object(sim, &restored, now);
        }
        changes.push(RegionChange::Updated(Box::new(restored)));
    }
    changes
}

/// Sends the client a terse update of where one object stands, logging a
/// send failure rather than failing the edit.
fn push_terse(sim: &mut SimSession, object: &Object, now: Instant) {
    let update = sl_proto::TerseUpdate {
        local_id: object.local_id,
        state: object.state,
        motion: object.motion.clone(),
    };
    if let Err(error) = sim.send_terse_update(&[update], crate::world::REAL_TIME_DILATION, now) {
        tracing::warn!("sending a terse update of an edited object failed: {error}");
    }
}

/// Streams one object to the client, logging a send failure rather than
/// failing the edit.
fn push_object(sim: &mut SimSession, object: &Object, now: Instant) {
    if let Err(error) = crate::world::send_objects(sim, std::slice::from_ref(object), now) {
        tracing::warn!("streaming an edited object failed: {error}");
    }
}

/// Sends one object's full properties to the client, logging a send failure
/// rather than failing the edit.
fn push_properties(sim: &mut SimSession, properties: &ObjectProperties, now: Instant) {
    if let Err(error) = sim.send_object_properties(properties, now) {
        tracing::warn!("sending an object's properties failed: {error}");
    }
}

#[cfg(test)]
mod test {
    use super::{Rotation, Vector, into_frame, out_of_frame, rotate};

    /// How close two metre values have to be to count as the same place after a
    /// round trip through two quaternion products.
    const EPSILON: f32 = 1.0e-5;

    /// A quarter turn about the Z axis.
    fn quarter_turn() -> Rotation {
        Rotation {
            x: 0.0,
            y: 0.0,
            z: std::f32::consts::FRAC_1_SQRT_2,
            s: std::f32::consts::FRAC_1_SQRT_2,
        }
    }

    /// Asserts two vectors name the same point.
    fn assert_near(left: &Vector, right: &Vector) {
        for (a, b) in [(left.x, right.x), (left.y, right.y), (left.z, right.z)] {
            assert!((a - b).abs() < EPSILON, "{left:?} is not {right:?}");
        }
    }

    /// A quarter turn about Z takes the X axis to the Y axis, which is the one
    /// hand-checkable case of the rotation the linkset maths rests on.
    #[test]
    fn a_quarter_turn_about_z_takes_x_to_y() {
        let turned = rotate(
            &quarter_turn(),
            &Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        );
        assert_near(
            &turned,
            &Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        );
    }

    /// Linking a prim restates its placement in the root's frame and delinking
    /// puts it back, so a link followed by a delink leaves it exactly where it
    /// stood — including when the root is turned, which is the case a naive
    /// subtraction gets wrong.
    #[test]
    fn a_link_and_a_delink_leave_a_prim_where_it_stood() {
        let root_position = Vector {
            x: 128.0,
            y: 64.0,
            z: 25.0,
        };
        let root_rotation = quarter_turn();
        let position = Vector {
            x: 130.0,
            y: 65.0,
            z: 26.5,
        };
        let rotation = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        };

        let (local, local_rotation) =
            into_frame(&root_position, &root_rotation, &position, &rotation);
        // The root is turned, so the child's offset is not the plain difference:
        // a two-metre step east of a root facing north reads as two metres to
        // the root's right.
        assert_near(
            &local,
            &Vector {
                x: 1.0,
                y: -2.0,
                z: 1.5,
            },
        );

        let (back, back_rotation) =
            out_of_frame(&root_position, &root_rotation, &local, &local_rotation);
        assert_near(&back, &position);
        for (sent, seen) in [
            (rotation.x, back_rotation.x),
            (rotation.y, back_rotation.y),
            (rotation.z, back_rotation.z),
            (rotation.s, back_rotation.s),
        ] {
            assert!(
                (sent - seen).abs() < EPSILON,
                "the rotation came back as {back_rotation:?}, not {rotation:?}"
            );
        }
    }
}
