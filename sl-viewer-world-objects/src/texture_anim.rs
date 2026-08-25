//! Per-object animated-texture state and driver (Phase 28).
//!
//! A prim animates its textures with `llSetTextureAnim`: a UV scroll / rotate /
//! scale, or a sprite-sheet flipbook stepping through a `size_x` × `size_y` grid
//! of frames. The 16-byte wire block is decoded upstream by
//! [`decode_texture_anim`](sl_client_bevy::decode_texture_anim) onto the object's
//! [`TextureAnimation`], and this module carries that decoded state onto the
//! renderable object (P28.1) and advances it each frame (P28.2).
//!
//! **P28.1 ingest.** The state rides an [`ObjectTextureAnimation`] component on
//! the object's **geometry holder** entity — the parent of its face entities —
//! mirroring the P27 [`ObjectRenderMaterials`](crate::materials::ObjectRenderMaterials)
//! holder. `apply_texture_animation` (in [`objects`](crate::objects)) refreshes it
//! on every object update and removes it when the animation stops
//! ([`ON`](sl_client_bevy::texture_anim_mode::ON) clear) or is absent, so a prim
//! whose animation is turned off in-world goes static again.
//!
//! **P28.2 driver — GPU-side (PERF).** The animation is evaluated **in the
//! shader** ([`face_material.wgsl`](crate::face_material)'s `sl_animated_uv`, a
//! port of the reference viewer's `LLViewerTextureAnim::animateTextures`) from
//! `globals.time`, so the material's UV is derived per-fragment on the GPU. The CPU
//! driver [`drive_texture_animations`] therefore only writes the animation's
//! *params* (rate / start / length / grid + the face's static fall-back placement +
//! a `start_time` seeding the shader clock) into each face's `SlFaceParams`, and
//! only **when they change** — so a steadily-running animation dirties **no**
//! materials per frame. This matters enormously: a busy region has ~1000+ animated
//! faces (the `SlFaceParams`(crate::face_material::SlFaceParams) `anim_*` fields
//! carry the params), and the old per-frame `uv_transform` write forced a full render-world
//! material re-prepare of every one of them each frame (Bevy recreates a material's
//! whole bind group on any change), which is the dominant cost the GPU path removes.
//! [`restore_stopped_animations`] clears the animation on each face when the object's
//! [`ObjectTextureAnimation`] is removed. The `ROTATE` / `SCALE` modes spin / grow
//! the whole texture; the default flipbook mode steps through the sprite grid; a
//! plain `size` with no grid scrolls the texture across the face.
//!
//! The Rust `animate` / `AnimatedPlacement` port is retained (test-gated) as the
//! **reference** the WGSL is translated from and the tests pin; production no longer
//! calls it.

#[cfg(test)]
use bevy::math::Affine2;
use bevy::prelude::*;
#[cfg(test)]
use sl_client_bevy::texture_uv_transform;
use sl_client_bevy::{TextureAnimation, texture_anim_mode};

use crate::face_material::FaceMaterial;
use crate::objects::{FaceTextureDebug, PrimFaceEntity};

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
pub struct ObjectTextureAnimation {
    /// The decoded animation block driving this object's faces.
    pub anim: TextureAnimation,
}

impl ObjectTextureAnimation {
    /// Whether this animation targets the given Linden face index — see
    /// [`anim_applies_to_face`].
    pub(crate) fn applies_to_face(&self, face_id: u16) -> bool {
        anim_applies_to_face(&self.anim, face_id)
    }
}

/// Whether `anim` targets the given Linden face index. The wire `face` is `-1`
/// for "all faces" (`llSetTextureAnim`'s `ALL_SIDES`), else the single face it
/// applies to — the target-face resolution the P28.2 driver uses to pick which
/// of an object's faces to fold the UV transform onto, and the intern-decision
/// check the [material cache](crate::material_cache) shares.
pub(crate) fn anim_applies_to_face(anim: &TextureAnimation, face_id: u16) -> bool {
    anim.face < 0 || u16::try_from(anim.face).is_ok_and(|target| target == face_id)
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

/// The current texture-entry placement of an animated face: the offset / scale /
/// rotation to fold into the face's `uv_transform` this frame, with each component
/// carrying whether the animation *drives* it (else it falls back to the face's
/// static [`TextureFace`](sl_client_bevy::TextureFace) value). A port of the local
/// variables `LLViewerTextureAnim::animateTextures` fills in.
///
/// Test-gated: this is the Rust **reference** the shader's `sl_animated_uv` is
/// translated from and [the tests](self) pin; the GPU path is what production runs.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct AnimatedPlacement {
    /// The rotation angle in radians; `None` when the animation does not drive it.
    rotation: Option<f32>,
    /// The `(s, t)` offset; `None` when the animation does not drive it.
    offset: Option<(f32, f32)>,
    /// The `(s, t)` repeats / scale; `None` when the animation does not drive it.
    scale: Option<(f32, f32)>,
}

#[cfg(test)]
impl AnimatedPlacement {
    /// Resolve this placement against a face's static texture-entry values (the
    /// fall-back for every component the animation does not drive) and build the
    /// `uv_transform` [`Affine2`] — the same affine
    /// [`texture_face_uv_transform`](sl_client_bevy::texture_face_uv_transform)
    /// builds for a static face, matching the reference viewer's `mTextureMatrix`.
    fn uv_transform(&self, face: &sl_client_bevy::TextureFace) -> Affine2 {
        let rotation = self.rotation.unwrap_or(face.rotation);
        let (offset_s, offset_t) = self.offset.unwrap_or((face.offset_s, face.offset_t));
        let (scale_s, scale_t) = self.scale.unwrap_or((face.scale_s, face.scale_t));
        texture_uv_transform(rotation, offset_s, offset_t, scale_s, scale_t)
    }
}

/// Advance one texture animation to `elapsed` seconds and return the frame's
/// texture-entry placement — a faithful port of the reference viewer's
/// `LLViewerTextureAnim::animateTextures` (`indra/newview/llviewertextureanim.cpp`).
///
/// The elapsed time is passed in (accumulated per frame by the driver) rather than
/// read from a timer; for both the stepped and `SMOOTH` paths a constant-rate
/// animation's frame counter is `elapsed × rate`, so the accumulator the reference
/// keeps for `SMOOTH` collapses to the same value. Returns [`None`] only when the
/// animation is not running ([`ON`](texture_anim_mode::ON) clear), which the driver
/// treats as "leave the face alone".
///
/// Test-gated: the production animation is [`sl_animated_uv`](crate::face_material)
/// in WGSL, a faithful translation of this; this Rust version is kept to pin the
/// math in unit tests.
#[cfg(test)]
fn animate(anim: &TextureAnimation, elapsed: f32) -> Option<AnimatedPlacement> {
    use texture_anim_mode::{LOOP, ON, PING_PONG, REVERSE, ROTATE, SCALE, SMOOTH};
    let mode = anim.mode;
    if mode & ON == 0 {
        return None;
    }

    let size_x = f32::from(anim.size_x);
    let size_y = f32::from(anim.size_y);
    let num_frames = if anim.length != 0.0 {
        anim.length
    } else {
        (size_x * size_y).max(1.0)
    };

    let full_length = if mode & PING_PONG != 0 {
        if mode & SMOOTH != 0 {
            2.0 * num_frames
        } else if mode & LOOP != 0 {
            (2.0 * num_frames - 2.0).max(1.0)
        } else {
            (2.0 * num_frames - 1.0).max(1.0)
        }
    } else {
        num_frames
    };

    // The raw frame counter: elapsed time scaled by the playback rate. (`%` on an
    // `f32` is C's `fmod`, matching the reference's `fmod` for the loop wrap.)
    let mut frame_counter = elapsed * anim.rate;
    if mode & LOOP != 0 {
        frame_counter %= full_length;
    } else {
        frame_counter = frame_counter.min(full_length - 1.0);
    }
    if mode & SMOOTH == 0 {
        // Step to a whole frame; the +0.01 nudge (and re-clamp) mirrors the
        // reference so a frame is not skipped at the boundary.
        frame_counter = (frame_counter + 0.01).floor();
        frame_counter = frame_counter.min(full_length - 1.0);
    }
    if mode & PING_PONG != 0 && frame_counter >= num_frames {
        frame_counter = if mode & SMOOTH != 0 {
            num_frames - (frame_counter - num_frames)
        } else {
            (num_frames - 1.99) - (frame_counter - num_frames)
        };
    }
    if mode & REVERSE != 0 {
        frame_counter = if mode & SMOOTH != 0 {
            num_frames - frame_counter
        } else {
            (num_frames - 0.99) - frame_counter
        };
    }
    frame_counter += anim.start;
    if mode & SMOOTH == 0 {
        frame_counter = frame_counter.round();
    }

    // Derive the placement from the frame counter. ROTATE / SCALE drive one
    // component and leave the rest to the texture entry; the default paging mode
    // drives the offset (and, with a frame grid, the scale) to select a cell.
    let mut placement = AnimatedPlacement {
        rotation: None,
        offset: None,
        scale: None,
    };
    if mode & ROTATE != 0 {
        placement.rotation = Some(frame_counter);
    } else if mode & SCALE != 0 {
        placement.scale = Some((frame_counter, frame_counter));
    } else if anim.size_x != 0 && anim.size_y != 0 {
        // Flipbook: divide the texture into a `size_x` × `size_y` grid and offset to
        // the current cell, with the scale set to one cell.
        let scale_s = 1.0 / size_x;
        let scale_t = 1.0 / size_y;
        let x_frame = frame_counter % size_x;
        let y_frame = (frame_counter / size_x).trunc();
        let x_pos = x_frame * scale_s;
        let y_pos = y_frame * scale_t;
        placement.scale = Some((scale_s, scale_t));
        placement.offset = Some((
            (-0.5 + 0.5 * scale_s) + x_pos,
            (0.5 - 0.5 * scale_t) - y_pos,
        ));
    } else {
        // No frame grid: scroll the texture across the face (scale falls back to the
        // texture entry, so only the offset is driven). With the reference's local
        // `scale_s` of 1, `off_s = (-0.5 + 0.5) + frame_counter` and `off_t = 0`.
        placement.offset = Some((frame_counter, 0.0));
    }
    Some(placement)
}

/// The face's static texture-entry placement, packed for the GPU animation params:
/// `anim_static = (rotation, offset_s, offset_t, scale_s)` and `scale_t` in
/// `anim_grid.z` (the shader's fall-back for whichever placement components the
/// animation does not drive), together with the flip-book grid `(size_x, size_y)`.
fn animation_placement(
    anim: &TextureAnimation,
    face: &sl_client_bevy::TextureFace,
) -> (Vec4, Vec4) {
    let static_placement = Vec4::new(face.rotation, face.offset_s, face.offset_t, face.scale_s);
    let grid = Vec4::new(
        f32::from(anim.size_x),
        f32::from(anim.size_y),
        face.scale_t,
        0.0,
    );
    (static_placement, grid)
}

/// Publish every running texture animation's params to its faces (P28.2), for the
/// **GPU** animation path: the shader ([`face_material.wgsl`](crate::face_material)
/// `sl_animated_uv`) evaluates the animation from `globals.time` each frame, so this
/// only writes the per-face `SlFaceParams`(crate::face_material::SlFaceParams)
/// `anim_*` fields — and **only when they change** (a fresh `llSetTextureAnim`, a
/// re-textured face, or a recomposition that wiped them). A steadily-running
/// animation therefore dirties **no** material here (the read is a non-mutating
/// [`Assets::get`]), which is the whole point: the old per-frame `uv_transform` write
/// re-prepared every animated material every frame (Bevy recreates a material's
/// whole bind group on any change) and dominated frame time on busy regions.
///
/// A rewrite seeds `anim_params.w` (`start_time`) with the current time so the shader
/// clock starts at zero — matching the reference viewer restarting a
/// re-parameterised animation from frame zero.
pub fn drive_texture_animations(
    time: Res<Time>,
    holders: Query<(Entity, &ObjectTextureAnimation)>,
    children: Query<&Children>,
    faces: Query<(
        &PrimFaceEntity,
        &FaceTextureDebug,
        &MeshMaterial3d<FaceMaterial>,
    )>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    // Seed the shader clock with the SAME wrapped time the shader reads
    // (`globals.time == Time::elapsed_secs_wrapped()`, wrapping hourly), so the
    // shader's `globals.time - start_time` is a valid elapsed once the hour-wrap is
    // unwound (see `sl_animated_uv`).
    let now = time.elapsed_secs_wrapped();
    for (holder, tex_anim) in &holders {
        let anim = tex_anim.anim;
        let mode = u32::from(anim.mode);
        let Ok(face_entities) = children.get(holder) else {
            continue;
        };
        for &face_entity in face_entities {
            let Ok((face, FaceTextureDebug(tf), material)) = faces.get(face_entity) else {
                continue;
            };
            if !tex_anim.applies_to_face(face.face_id.get()) {
                continue;
            }
            let (static_placement, grid) = animation_placement(&anim, tf);
            // Non-mutating check: is the stored setup already current? A `get` (not
            // `get_mut`) does not mark the material changed, so an already-set-up face
            // is not re-prepared — leaving `start_time` (`anim_params.w`) untouched, so
            // the running animation keeps its phase.
            let up_to_date = materials.get(&material.0).is_some_and(|material| {
                let params = &material.extension.params;
                // Compare the timing as a `Vec3` (rate, start, length) — excluding
                // `anim_params.w` (`start_time`), which is left as-is for an unchanged
                // animation so it keeps its phase — and via vector equality so clippy's
                // `float_cmp` is satisfied (exact equality is what we want: the values
                // are re-derived from the same source, so a difference means a change).
                params.anim_mode == mode
                    && params.anim_params.truncate()
                        == Vec3::new(anim.rate, anim.start, anim.length)
                    && params.anim_static == static_placement
                    && params.anim_grid == grid
            });
            if up_to_date {
                continue;
            }
            if let Some(mut material) = materials.get_mut(&material.0) {
                let params = &mut material.extension.params;
                params.anim_mode = mode;
                params.anim_params = Vec4::new(anim.rate, anim.start, anim.length, now);
                params.anim_static = static_placement;
                params.anim_grid = grid;
            }
        }
    }
}

/// Restore a face to its static texture-entry placement when its object's animation
/// stops (P28.2): when [`apply_texture_animation`](crate::objects) removes the
/// [`ObjectTextureAnimation`] holder (the `ON` bit cleared in-world, or the prim
/// gone), clear the GPU animation on each of the holder's faces so the shader stops
/// animating and the face reverts to its static texture-entry placement (also reset
/// `base.uv_transform` to [`texture_face_uv_transform`](sl_client_bevy::texture_face_uv_transform)
/// for good measure, since the static placement is what the base samples once
/// `anim_mode` is clear).
///
/// Mirrors `LLVOVolume::animateTextures` writing the texture entry's own
/// offset / scale / rotation back to the faces once `mTexAnimMode` clears.
pub fn restore_stopped_animations(
    mut stopped: RemovedComponents<ObjectTextureAnimation>,
    children: Query<&Children>,
    faces: Query<(&FaceTextureDebug, &MeshMaterial3d<FaceMaterial>)>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    for holder in stopped.read() {
        let Ok(face_entities) = children.get(holder) else {
            continue;
        };
        for &face_entity in face_entities {
            let Ok((FaceTextureDebug(tf), material)) = faces.get(face_entity) else {
                continue;
            };
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.base.uv_transform = sl_client_bevy::texture_face_uv_transform(tf);
                let params = &mut material.extension.params;
                params.anim_mode = 0;
                params.anim_params = Vec4::ZERO;
                params.anim_static = Vec4::ZERO;
                params.anim_grid = Vec4::ZERO;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

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

    /// A fully-specified flipbook / scroll `TextureAnimation` (mode targets all
    /// faces).
    fn flipbook(mode: u8, size_x: u8, size_y: u8, rate: f32) -> TextureAnimation {
        TextureAnimation {
            mode,
            face: -1,
            size_x,
            size_y,
            start: 0.0,
            length: 0.0,
            rate,
        }
    }

    /// A stepped (non-`SMOOTH`) flipbook selects the expected grid cell offset for a
    /// given elapsed time, at one cell's scale — the port's core.
    /// The driven placement of a running animation at `elapsed`.
    fn placement_at(
        anim: &TextureAnimation,
        elapsed: f32,
    ) -> Result<AnimatedPlacement, &'static str> {
        animate(anim, elapsed).ok_or("animation should be running")
    }

    #[test]
    fn flipbook_selects_the_current_cell() -> Result<(), String> {
        use texture_anim_mode::{LOOP, ON};
        let anim = flipbook(ON | LOOP, 2, 2, 1.0);
        // 2×2 grid → quarter-size cells; four cells stepped at 1 fps.
        let offset_at = |elapsed: f32| -> Result<(f32, f32), &'static str> {
            placement_at(&anim, elapsed)?.offset.ok_or("offset driven")
        };
        // Frame 0 (top-left): scale (0.5, 0.5), offset (-0.25, 0.25).
        let scale = placement_at(&anim, 0.0)?.scale.ok_or("scale driven")?;
        assert!((scale.0 - 0.5).abs() < 1e-6 && (scale.1 - 0.5).abs() < 1e-6);
        let offset = offset_at(0.0)?;
        assert!((offset.0 + 0.25).abs() < 1e-6 && (offset.1 - 0.25).abs() < 1e-6);
        // Frame 1 (top-right): offset (0.25, 0.25).
        let offset = offset_at(1.0)?;
        assert!((offset.0 - 0.25).abs() < 1e-6 && (offset.1 - 0.25).abs() < 1e-6);
        // Frame 2 (bottom-left): offset (-0.25, -0.25).
        let offset = offset_at(2.0)?;
        assert!((offset.0 + 0.25).abs() < 1e-6 && (offset.1 + 0.25).abs() < 1e-6);
        // Frame 3 (bottom-right): offset (0.25, -0.25).
        let offset = offset_at(3.0)?;
        assert!((offset.0 - 0.25).abs() < 1e-6 && (offset.1 + 0.25).abs() < 1e-6);
        // Frame 4 wraps back to frame 0 (LOOP).
        let offset = offset_at(4.0)?;
        assert!((offset.0 + 0.25).abs() < 1e-6 && (offset.1 - 0.25).abs() < 1e-6);
        Ok(())
    }

    /// A non-looping animation clamps to its last frame rather than wrapping.
    #[test]
    fn non_loop_clamps_to_the_last_frame() -> Result<(), String> {
        let anim = flipbook(texture_anim_mode::ON, 2, 2, 1.0);
        // Well past the end: held at frame 3 (bottom-right).
        let p = animate(&anim, 100.0).ok_or("running")?;
        let offset = p.offset.ok_or("offset driven")?;
        assert!((offset.0 - 0.25).abs() < 1e-6 && (offset.1 + 0.25).abs() < 1e-6);
        Ok(())
    }

    /// A gridless smooth scroll drives only the offset; the scale falls back to the
    /// face. A single-frame scroll needs `LOOP` to wrap (else it clamps to frame 0).
    #[test]
    fn scroll_drives_only_the_offset() -> Result<(), String> {
        use texture_anim_mode::{LOOP, ON, SMOOTH};
        let anim = flipbook(ON | SMOOTH | LOOP, 0, 0, 2.0);
        // full_length == 1, so off_s == fmod(elapsed × rate, 1) == 0.5, off_t == 0.
        let p = animate(&anim, 0.25).ok_or("running")?;
        let offset = p.offset.ok_or("offset driven")?;
        assert!((offset.0 - 0.5).abs() < 1e-6 && offset.1.abs() < 1e-6);
        assert!(p.scale.is_none());
        assert!(p.rotation.is_none());
        Ok(())
    }

    /// ROTATE mode drives only the rotation (the angle is the frame counter).
    #[test]
    fn rotate_drives_only_the_rotation() -> Result<(), String> {
        use texture_anim_mode::{ON, ROTATE, SMOOTH};
        // start angle 0, end angle via `length`; SMOOTH so the angle is continuous.
        let mut anim = flipbook(ON | ROTATE | SMOOTH, 0, 0, 1.0);
        anim.length = 4.0;
        let p = animate(&anim, 1.5).ok_or("running")?;
        assert!((p.rotation.ok_or("rotation driven")? - 1.5).abs() < 1e-6);
        assert!(p.offset.is_none());
        assert!(p.scale.is_none());
        Ok(())
    }

    /// A stopped animation (`ON` clear) yields no placement.
    #[test]
    fn stopped_animation_yields_no_placement() {
        assert!(animate(&flipbook(0, 2, 2, 1.0), 1.0).is_none());
    }

    /// Un-driven placement components fall back to the face's static texture-entry
    /// values (an identity face → identity transform when nothing is driven).
    #[test]
    fn placement_falls_back_to_the_face() {
        let face = sl_client_bevy::TextureFace::new(sl_client_bevy::TextureKey::from(
            sl_client_bevy::Uuid::nil(),
        ));
        let placement = AnimatedPlacement {
            rotation: None,
            offset: None,
            scale: None,
        };
        // A default (identity) face with nothing driven yields the identity xform.
        let identity = sl_client_bevy::texture_face_uv_transform(&face);
        assert_eq!(placement.uv_transform(&face), identity);
    }
}
