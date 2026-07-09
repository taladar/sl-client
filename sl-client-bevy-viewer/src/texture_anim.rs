//! Per-object animated-texture state (P28.1).
//!
//! A prim animates its textures with `llSetTextureAnim`: a UV scroll / rotate /
//! scale, or a sprite-sheet flipbook stepping through a `size_x` × `size_y` grid
//! of frames. The 16-byte wire block is decoded upstream by
//! [`decode_texture_anim`](sl_client_bevy::decode_texture_anim) onto the object's
//! [`TextureAnimation`], and this module carries that decoded state onto the
//! renderable object so the P28.2 driver can advance it each frame.
//!
//! The state rides an [`ObjectTextureAnimation`] component on the object's
//! **geometry holder** entity — the parent of its face entities — mirroring the
//! P27 [`ObjectRenderMaterials`](crate::materials::ObjectRenderMaterials) holder.
//! `apply_texture_animation` (in [`objects`](crate::objects)) refreshes
//! it on every object update and removes it when the animation stops
//! ([`ON`](sl_client_bevy::texture_anim_mode::ON) clear) or is absent, so a prim
//! whose animation is turned off in-world goes static again.
//!
//! The actual per-frame UV / flipbook folding is P28.2; this phase is only the
//! ingest, mirroring `LLViewerTextureAnim` holding the decoded block on the
//! object before `LLVOVolume::animateTextures` drives it.

use bevy::prelude::*;
use sl_client_bevy::{TextureAnimation, texture_anim_mode};

/// The decoded [`TextureAnimation`] (`llSetTextureAnim`) parameters an object is
/// currently animating with, attached to the object's **geometry holder** entity
/// (the parent of its face entities) so the P28.2 driver can fold a per-frame UV
/// transform onto each affected face.
///
/// Present only while the object carries a **running** animation (its
/// [`mode`](TextureAnimation::mode) has the [`ON`](texture_anim_mode::ON) bit
/// set); `apply_texture_animation` (in [`objects`](crate::objects)) removes it
/// when the animation stops so the faces revert to their static texture-entry
/// placement.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ObjectTextureAnimation {
    /// The decoded animation block driving this object's faces.
    pub(crate) anim: TextureAnimation,
}

impl ObjectTextureAnimation {
    /// Whether this animation targets the given Linden face index. The wire
    /// `face` is `-1` for "all faces" (`llSetTextureAnim`'s `ALL_SIDES`), else the
    /// single face it applies to — the target-face resolution the P28.2 driver
    /// uses to pick which of an object's faces to fold the UV transform onto.
    pub(crate) fn applies_to_face(&self, face_id: u16) -> bool {
        self.anim.face < 0 || u16::try_from(self.anim.face).is_ok_and(|target| target == face_id)
    }
}

/// The object's running texture animation, if any: the decoded
/// [`TextureAnimation`] when the object carries one with the
/// [`ON`](texture_anim_mode::ON) bit set, else `None` (no animation block, or one
/// whose `ON` bit is clear — a stopped animation the simulator still reports).
pub(crate) fn running_texture_animation(
    anim: Option<TextureAnimation>,
) -> Option<TextureAnimation> {
    anim.filter(|anim| anim.mode & texture_anim_mode::ON != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `TextureAnimation` with the given `mode` / `face`, other fields zeroed.
    fn anim(mode: u8, face: i8) -> TextureAnimation {
        TextureAnimation {
            mode,
            face,
            size_x: 1,
            size_y: 1,
            start: 0.0,
            length: 0.0,
            rate: 0.0,
        }
    }

    #[test]
    fn running_only_when_the_on_bit_is_set() {
        assert!(running_texture_animation(None).is_none());
        // ON clear (a stopped animation still reported by the simulator).
        assert!(running_texture_animation(Some(anim(texture_anim_mode::LOOP, 0))).is_none());
        // ON set: a running animation.
        assert!(running_texture_animation(Some(anim(texture_anim_mode::ON, 0))).is_some());
    }

    #[test]
    fn all_faces_sentinel_targets_every_face() {
        let all = ObjectTextureAnimation {
            anim: anim(texture_anim_mode::ON, -1),
        };
        assert!(all.applies_to_face(0));
        assert!(all.applies_to_face(7));
    }

    #[test]
    fn a_single_face_targets_only_that_face() {
        let one = ObjectTextureAnimation {
            anim: anim(texture_anim_mode::ON, 3),
        };
        assert!(one.applies_to_face(3));
        assert!(!one.applies_to_face(0));
        assert!(!one.applies_to_face(4));
    }
}
