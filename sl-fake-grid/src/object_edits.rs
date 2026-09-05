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
//! Only the editing client is told about a properties change here. Telling the
//! region's *other* viewers needs a selection subscription — who has what
//! selected — which is [`test-fake-grid-concurrent-edits`]'s work and
//! deliberately not this module's. A change to the object itself does reach
//! them, because that push needs no subscription.
//!
//! [`test-fake-grid-concurrent-edits`]: https://example.invalid/roadmap

use std::time::Instant;

use sl_proto::{
    Object, ObjectProperties, RegionLocalObjectId, ServerEvent, SimSession, prim_flags,
};
use sl_types::key::ObjectKey;
use sl_types::lsl::{Rotation, Vector};

use crate::world::{REAL_TIME_DILATION, RegionChange, SceneFixtures};

/// Answers one drained [`ServerEvent`] that edits an object, returning the
/// [`RegionChange`]s the region's other sessions have to be told about — or
/// [`None`] when the event is not an object edit at all, which is how
/// [`answer_world_request`](crate::world::answer_world_request) knows to carry
/// on looking.
///
/// `mint` supplies the ids the simulator chooses (a duplicate's object keys),
/// so a seeded grid duplicates the same object twice.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per edit message, each a handful of lines; splitting the \
              match would only move the arms somewhere they are harder to \
              compare against the message family they answer"
)]
pub(crate) fn answer_object_edit(
    world: &mut SceneFixtures,
    mint: &dyn Fn() -> uuid::Uuid,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    match event {
        // ----- edits to the properties record ------------------------------
        ServerEvent::ObjectNameSet { local_id, name } => {
            edit_properties(world, *local_id, sim, now, |properties| {
                properties.name.clone_from(name);
            });
        }
        ServerEvent::ObjectDescriptionSet {
            local_id,
            description,
        } => {
            edit_properties(world, *local_id, sim, now, |properties| {
                properties.description.clone_from(description);
            });
        }
        ServerEvent::ObjectCategorySet { local_id, category } => {
            edit_properties(world, *local_id, sim, now, |properties| {
                properties.category = *category;
            });
        }
        ServerEvent::ObjectSaleInfoSet {
            local_id,
            sale_type,
            sale_price,
        } => {
            edit_properties(world, *local_id, sim, now, |properties| {
                properties.sale_type = sale_type.to_code();
                properties.sale_price.clone_from(sale_price);
            });
        }
        // A grant sets the named bits and a revoke clears them: the message
        // carries the bits being *changed*, not the mask's new value.
        ServerEvent::ObjectPermissionsSet {
            local_id,
            field,
            set,
            mask,
            ..
        } => {
            edit_properties(world, *local_id, sim, now, |properties| {
                let target = field.select_mut(&mut properties.permissions);
                *target = if *set {
                    target.union(*mask)
                } else {
                    target.difference(*mask)
                };
            });
        }
        ServerEvent::ObjectGroupSet {
            local_ids,
            group_id,
        } => {
            for local_id in local_ids {
                edit_properties(world, *local_id, sim, now, |properties| {
                    properties.group = *group_id;
                });
            }
        }
        // A deed names the group as the owner and no agent, which is exactly
        // what an `OwnerKey::Group` is; the object's own `owner_id` moves with
        // it, since that is what an `ObjectUpdate` carries.
        ServerEvent::ObjectOwnerSet {
            local_ids, owner, ..
        } => {
            let mut changes = Vec::new();
            for local_id in local_ids {
                edit_properties(world, *local_id, sim, now, |properties| {
                    properties.last_owner_id = properties.owner.uuid();
                    properties.owner = *owner;
                    if let sl_types::key::OwnerKey::Group(group) = *owner {
                        properties.group = Some(group);
                    }
                });
                if let Some(changed) = edit_object(world, *local_id, |object| {
                    object.owner_id = owner.uuid();
                }) {
                    push_object(sim, &changed, now);
                    changes.push(RegionChange::Updated(Box::new(changed)));
                }
            }
            return Some(changes);
        }
        // ----- edits to the object update itself ---------------------------
        ServerEvent::ObjectClickActionSet {
            local_id,
            click_action,
        } => {
            return Some(update_object(world, *local_id, sim, now, |object| {
                object.click_action = click_action.to_code();
            }));
        }
        ServerEvent::ObjectMaterialSet { local_id, material } => {
            return Some(update_object(world, *local_id, sim, now, |object| {
                object.material = material.to_code();
            }));
        }
        ServerEvent::ObjectFlagsSet { local_id, flags } => {
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
            return Some(update_object(world, *local_id, sim, now, |object| {
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
                root_of(world, *local_id)
            } else {
                *local_id
            };
            return Some(update_object(world, target, sim, now, |object| {
                if let Some(position) = &transform.position {
                    object.motion.position.clone_from(position);
                }
                if let Some(rotation) = &transform.rotation {
                    object.motion.rotation.clone_from(rotation);
                }
                if let Some(scale) = &transform.scale {
                    object.scale.clone_from(scale);
                }
            }));
        }
        // ----- linking, duplicating, deleting ------------------------------
        // The first id is the root the rest are parented to (the reference
        // packs the selection root-first, and OpenSim reads it that way).
        //
        // A child's placement on the wire is stated **in its root's frame**, so
        // linking is not only a parent id: the child's region position and
        // rotation have to be rewritten as an offset from the root, or every
        // viewer draws the linkset with its children piled at the root.
        ServerEvent::ObjectsLinked { local_ids } => {
            let Some((root, children)) = local_ids.split_first() else {
                return Some(Vec::new());
            };
            let Some(parent) = world.object_by_local_id(*root) else {
                tracing::debug!("a link named root {root:?}, which this region does not have");
                return Some(Vec::new());
            };
            let mut changes = Vec::new();
            for child in children {
                changes.extend(update_object(world, *child, sim, now, |object| {
                    object.parent_id = *root;
                    let (position, rotation) = into_frame(
                        &parent.motion.position,
                        &parent.motion.rotation,
                        &object.motion.position,
                        &object.motion.rotation,
                    );
                    object.motion.position = position;
                    object.motion.rotation = rotation;
                }));
            }
            return Some(changes);
        }
        // Everything named leaves its linkset, and a named *root* takes its
        // children with it — "delink" on a root is what breaks a linkset up.
        // Each freed child's placement goes back into the region's own frame,
        // the inverse of what the link did to it.
        ServerEvent::ObjectsDelinked { local_ids } => {
            let mut orphaned: Vec<RegionLocalObjectId> = local_ids.clone();
            for named in local_ids {
                orphaned.extend(
                    world
                        .objects
                        .iter()
                        .filter(|object| object.parent_id == *named)
                        .map(|object| object.local_id),
                );
            }
            orphaned.dedup();
            let mut changes = Vec::new();
            for local_id in orphaned {
                let parent = world
                    .object_by_local_id(local_id)
                    .filter(|object| object.parent_id.0 != 0)
                    .and_then(|object| world.object_by_local_id(object.parent_id));
                let Some(parent) = parent else {
                    // A named object that was not a child in the first place:
                    // the viewer sends its whole selection, roots included.
                    continue;
                };
                changes.extend(update_object(world, local_id, sim, now, |object| {
                    object.parent_id = RegionLocalObjectId(0);
                    let (position, rotation) = out_of_frame(
                        &parent.motion.position,
                        &parent.motion.rotation,
                        &object.motion.position,
                        &object.motion.rotation,
                    );
                    object.motion.position = position;
                    object.motion.rotation = rotation;
                }));
            }
            return Some(changes);
        }
        // A copy is a rez the client did not describe: the simulator mints both
        // ids, so the duplicating client learns them the same way a rezzing one
        // does — from the `ObjectUpdate` that comes back.
        ServerEvent::ObjectsDuplicated {
            local_ids, offset, ..
        } => {
            let mut changes = Vec::new();
            for local_id in local_ids {
                let Some(original) = world.object_by_local_id(*local_id) else {
                    continue;
                };
                let mut copy = original;
                copy.local_id = world.mint_local_id();
                copy.full_id = ObjectKey::from(mint());
                copy.motion.position = offset_by(&copy.motion.position, offset);
                if let Some(properties) = copy.properties.as_mut() {
                    properties.object_id = copy.full_id;
                }
                world.objects.push(copy.clone());
                push_object(sim, &copy, now);
                changes.push(RegionChange::Rezzed(Box::new(copy)));
            }
            return Some(changes);
        }
        // The force-delete, which leaves nothing behind: no inventory item, no
        // trash. The ordinary delete-to-trash is a derez, and it is answered
        // where the takes are.
        ServerEvent::ObjectsDeleted { local_ids, .. } => {
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
        // A selection is a simulator's cue to send the full properties, and
        // keep sending them while it stands. The fake grid keeps no selection
        // state, so it answers the ask and no more.
        ServerEvent::ObjectsSelected { local_ids } => {
            for local_id in local_ids {
                if let Some(properties) = world.properties_of(*local_id) {
                    push_properties(sim, &properties, now);
                }
            }
        }
        ServerEvent::ObjectsDeselected { .. } => {}
        // The condensed form, which needs no selection: what a viewer shows on
        // hover and in the pay / report dialogs.
        ServerEvent::RequestObjectPropertiesFamily {
            request_flags,
            object_id,
        } => {
            let Some(local_id) = world.local_id_of(*object_id) else {
                tracing::debug!("properties were asked of {object_id}, which is not here");
                return Some(Vec::new());
            };
            let Some(properties) = world.properties_of(local_id) else {
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
    // Every arm that falls through here changed the properties record only,
    // which the editing client was told about directly.
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
fn into_frame(
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
fn out_of_frame(
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
fn rotate(rotation: &Rotation, vector: &Vector) -> Vector {
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

/// The root of `local_id`'s linkset — itself when it is not a child, since a
/// prim whose parent this region does not have is one nothing can be said
/// about.
fn root_of(world: &SceneFixtures, local_id: RegionLocalObjectId) -> RegionLocalObjectId {
    world
        .object_by_local_id(local_id)
        .map(|object| object.parent_id)
        .filter(|parent| parent.0 != 0 && world.object_by_local_id(*parent).is_some())
        .unwrap_or(local_id)
}

/// Applies `edit` to the object's stored [`ObjectProperties`], recording the
/// object as it was for the undo stack, and pushes the new record at the
/// editing client.
fn edit_properties(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    sim: &mut SimSession,
    now: Instant,
    edit: impl FnOnce(&mut ObjectProperties),
) {
    let Some(current) = world.properties_of(local_id) else {
        tracing::debug!("an object edit named {local_id:?}, which this region does not have");
        return;
    };
    world.record_undo(local_id);
    let Some(object) = world.object_mut(local_id) else {
        return;
    };
    let mut properties = current;
    edit(&mut properties);
    object.properties = Some(properties);
    // Re-read rather than pushing what was just written: the contents serial a
    // client reads off the record lives with the task inventory, not with the
    // object, and only `properties_of` knows to put the two together.
    if let Some(pushed) = world.properties_of(local_id) {
        push_properties(sim, &pushed, now);
    }
}

/// Applies `edit` to the object itself, recording the object as it was for the
/// undo stack, and returns the changed object.
fn edit_object(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    edit: impl FnOnce(&mut Object),
) -> Option<Object> {
    world.record_undo(local_id);
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

/// Steps each named object one place along its edit history — back on an undo,
/// forward on a redo — and streams whatever that restored.
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
        let Some(restored) = world.step_history(local_id, backwards) else {
            // Nothing left to undo is not an error: the viewer's own undo stack
            // is deeper than the simulator's, and it keeps asking.
            continue;
        };
        push_object(sim, &restored, now);
        if let Some(properties) = world.properties_of(local_id) {
            push_properties(sim, &properties, now);
        }
        changes.push(RegionChange::Updated(Box::new(restored)));
    }
    changes
}

/// Streams one object to the client, logging a send failure rather than
/// failing the edit.
fn push_object(sim: &mut SimSession, object: &Object, now: Instant) {
    if let Err(error) =
        sim.send_object_update(std::slice::from_ref(object), REAL_TIME_DILATION, now)
    {
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
