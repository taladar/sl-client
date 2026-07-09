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
//! **P28.2 driver.** [`drive_texture_animations`] advances every animated object
//! each frame: it ports the reference viewer's `LLViewerTextureAnim::animateTextures`
//! to derive the current frame's texture-entry placement (an offset / scale /
//! rotation, with the un-driven components falling back to the face's static
//! [`TextureEntry`](sl_client_bevy::TextureEntry) placement) and folds it into each
//! affected face's `StandardMaterial::uv_transform` — exactly as the reference
//! viewer replaces a face's UV transform with `mTextureMatrix` while an animation
//! runs. [`restore_stopped_animations`] resets a face back to its static placement
//! when the animation is removed. The `ROTATE` / `SCALE` modes spin / grow the
//! whole texture; the default flipbook mode steps through the sprite grid; a plain
//! `size` with no grid scrolls the texture across the face.

use bevy::math::Affine2;
use bevy::prelude::*;
use sl_client_bevy::{TextureAnimation, texture_anim_mode, texture_uv_transform};

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

/// The per-object clock the P28.2 driver keeps beside an [`ObjectTextureAnimation`]
/// holder: the seconds elapsed since the current animation started, and the params
/// it is timing so a re-parameterisation (a fresh `llSetTextureAnim` on the prim)
/// restarts the clock. The reference viewer's `LLViewerTextureAnim` holds the same
/// state (`mTimer` / an elapsed accumulator); here the elapsed time is advanced by
/// [`drive_texture_animations`] each frame rather than read from a wall clock, so a
/// paused / slow frame never skips animation frames.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TextureAnimationClock {
    /// Seconds elapsed since this animation (re)started.
    elapsed: f32,
    /// The animation params this clock is timing, compared each frame to detect a
    /// re-parameterisation (which resets [`elapsed`](Self::elapsed) to zero).
    anim: TextureAnimation,
}

/// The current texture-entry placement of an animated face: the offset / scale /
/// rotation to fold into the face's `uv_transform` this frame, with each component
/// carrying whether the animation *drives* it (else it falls back to the face's
/// static [`TextureFace`](sl_client_bevy::TextureFace) value). A port of the local
/// variables `LLViewerTextureAnim::animateTextures` fills in.
#[derive(Debug, Clone, Copy)]
struct AnimatedPlacement {
    /// The rotation angle in radians; `None` when the animation does not drive it.
    rotation: Option<f32>,
    /// The `(s, t)` offset; `None` when the animation does not drive it.
    offset: Option<(f32, f32)>,
    /// The `(s, t)` repeats / scale; `None` when the animation does not drive it.
    scale: Option<(f32, f32)>,
}

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

/// Drive every running texture animation (P28.2): advance each
/// [`ObjectTextureAnimation`] holder's clock and fold the current frame's
/// texture-entry placement into each affected face's `uv_transform`.
///
/// Mirrors `LLViewerTextureAnim::updateClass` walking every animated object and
/// `LLVOVolume::animateTextures` folding a per-face texture matrix onto its faces.
/// The animation *replaces* the face's static placement (the un-driven components
/// falling back to it), exactly as the reference viewer uses `mTextureMatrix`
/// instead of the static UV transform while an animation runs.
pub(crate) fn drive_texture_animations(
    time: Res<Time>,
    mut commands: Commands,
    mut holders: Query<(
        Entity,
        &ObjectTextureAnimation,
        Option<&mut TextureAnimationClock>,
    )>,
    children: Query<&Children>,
    faces: Query<(
        &PrimFaceEntity,
        &FaceTextureDebug,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    for (holder, tex_anim, clock) in &mut holders {
        let anim = tex_anim.anim;
        // Advance (or start) the object's clock, restarting it on a re-parameterised
        // animation so a fresh `llSetTextureAnim` plays from frame zero.
        let elapsed = match clock {
            Some(mut clock) => {
                if clock.anim != anim {
                    clock.anim = anim;
                    clock.elapsed = 0.0;
                }
                clock.elapsed += dt;
                clock.elapsed
            }
            None => {
                commands
                    .entity(holder)
                    .insert(TextureAnimationClock { elapsed: 0.0, anim });
                dt
            }
        };
        let Some(placement) = animate(&anim, elapsed) else {
            continue;
        };
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
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.uv_transform = placement.uv_transform(tf);
            }
        }
    }
}

/// Restore a face to its static texture-entry placement when its object's animation
/// stops (P28.2): when [`apply_texture_animation`](crate::objects) removes the
/// [`ObjectTextureAnimation`] holder (the `ON` bit cleared in-world, or the prim
/// gone), reset each of the holder's faces' `uv_transform` back to
/// [`texture_face_uv_transform`](sl_client_bevy::texture_face_uv_transform).
///
/// Mirrors `LLVOVolume::animateTextures` writing the texture entry's own
/// offset / scale / rotation back to the faces once `mTexAnimMode` clears.
pub(crate) fn restore_stopped_animations(
    mut stopped: RemovedComponents<ObjectTextureAnimation>,
    mut commands: Commands,
    children: Query<&Children>,
    faces: Query<(&FaceTextureDebug, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for holder in stopped.read() {
        // The clock is meaningless without a running animation; drop it so a later
        // animation on the same object starts from frame zero.
        if let Ok(mut holder) = commands.get_entity(holder) {
            holder.remove::<TextureAnimationClock>();
        }
        let Ok(face_entities) = children.get(holder) else {
            continue;
        };
        for &face_entity in face_entities {
            let Ok((FaceTextureDebug(tf), material)) = faces.get(face_entity) else {
                continue;
            };
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.uv_transform = sl_client_bevy::texture_face_uv_transform(tf);
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
