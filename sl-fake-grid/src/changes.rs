//! Which `changed` a change made from a viewer raises in a prim's scripts.
//!
//! [`crate::scripts::raise_changed_in`] delivers a bit; this module decides
//! which bit each grid-side cause raises, and in which prims, as the
//! `events/changed-edits` and `events/changed-contents` probes measured it
//! (`book/src/gridspec/lsl.md` § `changed`: what a viewer's change raises).
//! Where the grids disagree the region's
//! [`ChangePolicy`](crate::imitates::ChangePolicy) says how.
//!
//! The causes here are a viewer's: an edit with the build floater, face media
//! set over `ObjectMedia`, and a write into a prim's contents. A script's own
//! setters raise theirs where the setter is (`server-lsl-lib-prim-state`), a
//! link and a sit theirs in [`crate::link_sets`] and [`crate::seating`].

use sl_proto::{Object, ObjectTransform, RegionLocalObjectId, TextureFace};

use crate::imitates::{ChildResizeHeard, MovesHeard};
use crate::world::SceneFixtures;

/// `CHANGED_INVENTORY`.
pub(crate) const CHANGED_INVENTORY: i32 = 0x1;
/// `CHANGED_COLOR`.
pub(crate) const CHANGED_COLOR: i32 = 0x2;
/// `CHANGED_SHAPE`.
pub(crate) const CHANGED_SHAPE: i32 = 0x4;
/// `CHANGED_SCALE`.
pub(crate) const CHANGED_SCALE: i32 = 0x8;
/// `CHANGED_TEXTURE`.
pub(crate) const CHANGED_TEXTURE: i32 = 0x10;
/// `CHANGED_ALLOWED_DROP`.
pub(crate) const CHANGED_ALLOWED_DROP: i32 = 0x40;
/// `CHANGED_MEDIA`.
pub(crate) const CHANGED_MEDIA: i32 = 0x800;

/// The bits a face edit raises: `CHANGED_COLOR` where a face's colour or
/// alpha changed, `CHANGED_TEXTURE` where anything else of a face did —
/// nothing for an edit that leaves every face as it was, on both grids. A
/// legacy material id alone raises nothing (OpenSim ignores it, a material
/// being the `RenderMaterials` capability's business).
fn face_bits(old: &[TextureFace], new: &[TextureFace]) -> i32 {
    let mut bits = 0;
    for (before, after) in old.iter().zip(new) {
        if before.color != after.color {
            bits |= CHANGED_COLOR;
        }
        let textured = before.texture_id != after.texture_id
            || before.scale_s.to_bits() != after.scale_s.to_bits()
            || before.scale_t.to_bits() != after.scale_t.to_bits()
            || before.offset_s.to_bits() != after.offset_s.to_bits()
            || before.offset_t.to_bits() != after.offset_t.to_bits()
            || before.rotation.to_bits() != after.rotation.to_bits()
            || before.bump_shiny_fullbright != after.bump_shiny_fullbright
            || before.media_flags != after.media_flags
            || before.glow.to_bits() != after.glow.to_bits();
        if textured {
            bits |= CHANGED_TEXTURE;
        }
    }
    bits
}

/// Raises what a viewer's retexture of the prim `local_id` raises, `before`
/// being the prim as it was: in that prim alone.
pub(crate) fn retextured(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    before: &Object,
) {
    let Some(after) = world.object_by_local_id(local_id) else {
        return;
    };
    let faces = sl_object_asset::rendered_face_count(&after.shape);
    let old = sl_proto::decode_texture_entry(&before.texture_entry, faces);
    let new = sl_proto::decode_texture_entry(&after.texture_entry, faces);
    let bits = face_bits(&old.faces, &new.faces);
    if bits != 0 {
        crate::scripts::raise_changed_in(world, &[local_id], bits);
    }
}

/// Raises what a viewer's change of the prim `local_id`'s shape raises:
/// `CHANGED_SHAPE`, in that prim alone.
pub(crate) fn reshaped(world: &mut SceneFixtures, local_id: RegionLocalObjectId) {
    crate::scripts::raise_changed_in(world, &[local_id], CHANGED_SHAPE);
}

/// Raises what a viewer's face media set or cleared on the prim `local_id`
/// raises: `CHANGED_MEDIA`, in that prim alone.
pub(crate) fn media_set(world: &mut SceneFixtures, local_id: RegionLocalObjectId) {
    crate::scripts::raise_changed_in(world, &[local_id], CHANGED_MEDIA);
}

/// Raises what a viewer's move, turn or resize of the prim `target` raises.
///
/// A resize is `CHANGED_SCALE` whatever the size was, in the prim on OpenSim
/// and — for a child — in the root on Second Life. A move or a turn raises
/// nothing on Second Life and OpenSim's own `changed(32768)` there: a root
/// moved once in the root and twice in each child, a root turned once in
/// each child, a child moved once in the child.
pub(crate) fn transformed(
    world: &mut SceneFixtures,
    target: RegionLocalObjectId,
    transform: &ObjectTransform,
) {
    let policy = world.changes;
    let root = world.root_of(target);
    if transform.scale.is_some() {
        let heard = match policy.child_resize {
            ChildResizeHeard::TheRoot => root,
            ChildResizeHeard::TheChild => target,
        };
        crate::scripts::raise_changed_in(world, &[heard], CHANGED_SCALE);
    }
    if policy.moves != MovesHeard::OpenSimPosition {
        return;
    }
    let position = sl_lsl_runtime::vm::OPENSIM_CHANGED_POSITION;
    if target != root {
        if transform.position.is_some() {
            crate::scripts::raise_changed_in(world, &[target], position);
        }
        return;
    }
    let children = crate::link_sets::children_in_order(world, root);
    if transform.position.is_some() {
        crate::scripts::raise_changed_in(world, &[root], position);
        crate::scripts::raise_changed_in(world, &children, position);
        crate::scripts::raise_changed_in(world, &children, position);
    } else if transform.rotation.is_some() {
        crate::scripts::raise_changed_in(world, &children, position);
    }
}

/// Raises what a write into the prim `local_id`'s contents raises:
/// `CHANGED_INVENTORY`, or `CHANGED_ALLOWED_DROP` for a drop by an avatar
/// who may not edit the prim, let in because it allows drops — in that prim
/// alone, on both grids.
pub(crate) fn contents_written(
    world: &mut SceneFixtures,
    local_id: RegionLocalObjectId,
    allowed_drop: bool,
) {
    let bit = if allowed_drop {
        CHANGED_ALLOWED_DROP
    } else {
        CHANGED_INVENTORY
    };
    crate::scripts::raise_changed_in(world, &[local_id], bit);
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::TextureFace;
    use sl_types::key::TextureKey;

    use super::{CHANGED_COLOR, CHANGED_TEXTURE, face_bits};

    /// A plain face.
    fn face() -> TextureFace {
        TextureFace::new(TextureKey::from(uuid::Uuid::from_u128(1)))
    }

    /// A colour or alpha change raises `CHANGED_COLOR`, any other face field
    /// `CHANGED_TEXTURE`, the same faces nothing, a material id alone nothing.
    #[test]
    fn a_face_edit_raises_what_changed() {
        let plain = vec![face(); 6];
        assert_eq!(face_bits(&plain, &plain), 0);
        let mut red = plain.clone();
        if let Some(first) = red.first_mut() {
            first.color = [255, 0, 0, 255];
        }
        assert_eq!(face_bits(&plain, &red), CHANGED_COLOR);
        let mut faded = plain.clone();
        if let Some(last) = faded.last_mut() {
            last.color[3] = 128;
        }
        assert_eq!(face_bits(&plain, &faded), CHANGED_COLOR);
        for change in [
            |face: &mut TextureFace| face.texture_id = TextureKey::from(uuid::Uuid::from_u128(2)),
            |face: &mut TextureFace| face.scale_s = 2.0,
            |face: &mut TextureFace| face.glow = 0.2,
            |face: &mut TextureFace| face.bump_shiny_fullbright = 0x20,
            |face: &mut TextureFace| face.media_flags = 1,
            |face: &mut TextureFace| face.rotation = 1.0,
        ] {
            let mut changed = plain.clone();
            for face in &mut changed {
                change(face);
            }
            assert_eq!(face_bits(&plain, &changed), CHANGED_TEXTURE);
            red.clone_from(&changed);
            if let Some(first) = red.first_mut() {
                first.color = [0, 255, 0, 255];
            }
            assert_eq!(face_bits(&plain, &red), CHANGED_COLOR | CHANGED_TEXTURE);
        }
        let mut material = plain.clone();
        for face in &mut material {
            face.material_id = Some(uuid::Uuid::from_u128(3));
        }
        assert_eq!(face_bits(&plain, &material), 0);
    }
}
