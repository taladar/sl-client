//! The one invariant that turns a content bug into a dead viewer: a mesh's skin
//! **vertex attributes** and its entity's [`SkinnedMesh`] must agree.
//!
//! # Why this cannot be left to the spawn sites
//!
//! Bevy decides the two halves of a skinned draw from two different places, and
//! nothing in Bevy reconciles them:
//!
//! - the **pipeline** is specialized from the *mesh asset's vertex layout* —
//!   `bevy_pbr`'s `is_skinned(layout)`, true when the mesh carries
//!   `JOINT_INDEX` **and** `JOINT_WEIGHT`;
//! - the **bind group** is chosen from the *entity*, at draw time, by whether it
//!   has an entry in `SkinUniforms` (`skin_byte_offset(entity).is_some()`).
//!
//! Disagree in either direction and wgpu rejects the draw:
//!
//! ```text
//! The BindGroupLayout with 'mesh_layout' label of current set BindGroup with
//! 'model_only_mesh_bind_group' label at index 2 is not compatible with the
//! corresponding BindGroupLayout with 'skinned_mesh_layout' label
//! ```
//!
//! Bevy's render error handler quits the application on that, so the window
//! simply vanishes — with no clue as to *which* entity was malformed. That is
//! the whole cost of this bug: not the artifact, but that the report names
//! nothing you can act on.
//!
//! # Why it matters far beyond the one bad entity
//!
//! Worn rigged submeshes deliberately **share** their converted mesh asset
//! across wearers (`sl_viewer_kit::GeometryCache`'s rigged slots), because Bevy
//! batches on the mesh asset and that is what collapses N wearers of the same
//! body into one instanced draw. On any device where skins live in storage
//! buffers — every desktop GPU — `bevy_pbr`'s `no_automatic_skin_batching`
//! returns early, so skinned meshes really are batched, and the batch's bind
//! group is chosen from **one representative entity**.
//!
//! So a single wearer whose skin fails to register does not render itself
//! wrongly: it poisons the whole `MultiDrawIndirect` batch, taking down every
//! other avatar sharing that body. That is why the crash looks like it depends
//! on *whose* attachments are in view rather than on anything this repository
//! changed.
//!
//! # What this does
//!
//! Checks the invariant in the main world, on the entities that just changed,
//! **before** the extract that would hand the mismatch to wgpu — and fails
//! naming the entity, its ancestry and its mesh.
//!
//! It is deliberately still fatal. Making the validation error non-fatal would
//! hide a real bug in exactly the release builds this project tests with; the
//! point here is to fail *earlier*, and to say something actionable when it
//! does. The static twin of this check is `crate::render_test`'s
//! `unskinned_violations`, which decides the same property for the scenes a test
//! can build — this one covers the content only a grid can produce.

use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;

/// The entities whose mesh or skin changed this frame — every window in which
/// the agreement can be newly broken, and the only ones worth re-deciding.
type ChangedSkinnable<'world, 'state> = Query<
    'world,
    'state,
    (Entity, &'static Mesh3d, Option<&'static SkinnedMesh>),
    Or<(Changed<Mesh3d>, Changed<SkinnedMesh>)>,
>;

/// Names an entity as helpfully as the world allows: its own [`Name`] if it has
/// one, then its nearest named ancestor, then its id.
///
/// A rigged submesh is spawned nameless under a body root, so its id alone says
/// nothing; the ancestor is what identifies the avatar or attachment it belongs
/// to.
fn describe(
    entity: Entity,
    names: &Query<&Name>,
    parents: &Query<&ChildOf>,
    objects: &Query<&crate::world_api::SceneObject>,
) -> String {
    let own = names
        .get(entity)
        .map_or_else(|_error| String::new(), |name| format!(" `{name}`"));
    let mut ancestry = Vec::new();
    let mut current = entity;
    // A bounded walk: a malformed hierarchy must not turn a diagnostic into a
    // hang. Every ancestor is reported by id even when it has no `Name`, because
    // *whether the entity is parented at all* already tells the reader which
    // spawn path built it — a worn rigged submesh hangs under its wearer's body
    // root, a base part under the skeleton instance, and a stray has no parent.
    for _step in 0..16_u8 {
        let Ok(parent) = parents.get(current) else {
            break;
        };
        current = parent.parent();
        // The `SceneObject` is the identifying fact when there is no `Name`: it
        // carries the scoped object id to look the content up by, and the render
        // category that says which spawn path claimed it.
        let object = objects.get(current).map_or_else(
            |_error| String::new(),
            |object| format!(" [{:?} {:?}]", object.scoped_id, object.category),
        );
        match names.get(current) {
            Ok(name) => ancestry.push(format!("{current} `{name}`{object}")),
            Err(_error) => ancestry.push(format!("{current}{object}")),
        }
        if ancestry.len() >= 4 {
            break;
        }
    }
    if ancestry.is_empty() {
        format!("{entity}{own} (no parent)")
    } else {
        format!("{entity}{own} (under {})", ancestry.join(" < "))
    }
}

/// Fail on any entity whose mesh skin attributes and [`SkinnedMesh`] disagree.
///
/// Runs over the entities whose mesh or skin *changed*, which is every window in
/// which the invariant can be newly broken: a spawn, a mesh swap (a level of
/// detail change, an appearance morph), or a skin bound after the fact.
///
/// A mesh handle whose asset is not loaded yet is skipped rather than judged —
/// it carries no attributes to compare, and Bevy will not draw it either. It is
/// re-checked when the swap that loads it marks `Mesh3d` changed.
pub(crate) fn assert_skin_agreement(
    changed: ChangedSkinnable<'_, '_>,
    meshes: Res<Assets<Mesh>>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    objects: Query<&crate::world_api::SceneObject>,
    mut exit: MessageWriter<AppExit>,
) {
    // Every disagreement this frame is reported before the run ends, not just
    // the first: they arrive in bursts as a scene rezzes, and one name is a
    // worse lead than the set — whether it is one wearer or every wearer of one
    // body is most of the diagnosis.
    let mut found = false;
    for (entity, mesh, skin) in &changed {
        let Some(asset) = meshes.get(&mesh.0) else {
            continue;
        };
        // Both attributes, as `is_skinned` requires: a mesh carrying only one of
        // them does not specialize skinned, so it is not this bug.
        let attributes = asset.contains_attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
            && asset.contains_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT);
        let skinned = skin.is_some();
        if attributes == skinned {
            continue;
        }
        let who = describe(entity, &names, &parents, &objects);
        let mesh_id = mesh.0.id();
        if attributes {
            error!(
                "{who}: its mesh ({mesh_id:?}) carries skin vertex attributes but the entity has \
                 no `SkinnedMesh`. Bevy specializes the skinned pipeline from those attributes \
                 and then hands it a model-only bind group — and because worn rigged submeshes \
                 share one mesh asset across wearers to be batched, this takes down every other \
                 wearer drawn in the same batch. Either bind the skin or convert this mesh \
                 statically."
            );
        } else {
            error!(
                "{who}: the entity has a `SkinnedMesh` but its mesh ({mesh_id:?}) carries no \
                 skin vertex attributes. Bevy specializes the model-only pipeline from the \
                 attributes and then hands it a skinned bind group. Either drop the \
                 `SkinnedMesh` or convert this mesh with its weights."
            );
        }
        found = true;
    }
    if found {
        // Deliberately fatal, and deliberately *here* rather than at the wgpu
        // validation error this would otherwise become. Making the render error
        // non-fatal would hide a real content bug in exactly the release builds
        // this project tests with; ending the run on the frame that built the
        // malformed entity, having named it, is the opposite of hiding it. The
        // status is non-zero (`crate::run_session` no longer discards a failing
        // `AppExit`), so an unattended harness sees the failure too.
        exit.write(AppExit::error());
    }
}
