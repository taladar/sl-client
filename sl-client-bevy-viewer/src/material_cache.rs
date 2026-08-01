//! Cross-instance **material cache**: shared [`FaceMaterial`] handles for every
//! face whose material *inputs* are byte-identical (roadmap
//! `viewer-perf-material-intern`).
//!
//! The [`GeometryCache`](crate::geometry_cache) already shares one `Mesh`
//! handle across identical object instances, but Bevy only collapses draws into
//! instanced batches when the **material** handle matches too — and every face
//! used to build its own material, so even a row of identical fence posts cost
//! one draw each. This cache keys a face's diffuse material by its content —
//! the decoded [`TextureFace`] (texture id, tint, repeats / offset / rotation,
//! bump-shiny-fullbright byte, media / texgen flags, glow) plus the
//! [`TextureAlpha`] mode — and shares one material asset across every face with
//! equal inputs, so matched-shape matched-texture copies batch into ~one
//! instanced draw. Floats are keyed by their exact bit patterns: the values
//! come from the wire's quantized encoding, so identical wire faces dequantize
//! to identical bits (a rounding scheme would only merge faces the wire already
//! distinguishes).
//!
//! **Exclusions.** A face whose material is *mutated per instance* after spawn
//! must not share a handle, or the mutation would leak into every sharer. The
//! per-face exclusions (legacy `material_id`, bump map, media) ride the texture
//! entry, so a change re-tessellates the faces and re-evaluates the decision;
//! the object-level ones (a running texture animation, PBR render materials, a
//! HUD attachment) are carried by [`MaterialInternContext`]. The texture-decode
//! drape and level-of-detail re-upload mutate a shared material too, but
//! content-identically (same key ⇒ same texture id ⇒ same pixels), so they are
//! safe.
//!
//! **Detach net.** Some mutation sources arrive *without* a texture-entry
//! change: a late `llSetTextureAnim`, a PBR material assigned to existing
//! faces, a HUD routing, or the edit floaters' live previews on the selection.
//! [`detach_shared_face_materials`] gives any such face a private material
//! (copy-on-write) before the mutating systems run — every interned face
//! carries the [`SharedFaceMaterial`] marker so the sweep is a cheap no-op in
//! steady state.
//!
//! **Lifetime** mirrors the geometry cache: weak [`AssetId`]s only, revived via
//! [`Assets::get_strong_handle`] while some live face still holds the asset,
//! with dead entries dropped by the periodic [`prune_material_cache`] sweep.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use sl_client_bevy::{Object, PrimFaceId, Priority, TextureAnimation, TextureFace, TextureKey};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::edit_selection::SelectionSet;
use crate::face_material::FaceMaterial;
use crate::hud::on_hud_layer;
use crate::materials::ObjectRenderMaterials;
use crate::objects::{FaceTextureDebug, PrimFaceEntity};
use crate::texture_anim::{
    ObjectTextureAnimation, anim_applies_to_face, running_texture_animation,
};
use crate::textures::{PrimTextures, TextureAlpha, TextureManager, face_material};

/// The content key of one internable face material: every input
/// [`face_material`] composes from, with the float fields stored as their exact
/// [`f32::to_bits`] patterns so the key is `Eq + Hash` without any float
/// comparison. Equal keys ⇒ byte-identical composed materials. The legacy
/// `material_id` is deliberately absent — a face carrying one is excluded from
/// interning ([`MaterialInternContext::internable`]), so it can never reach a
/// key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MaterialKey {
    /// The face's diffuse texture asset id.
    texture_id: TextureKey,
    /// The RGBA tint bytes (the wire's un-inverted `color`).
    color: [u8; 4],
    /// Horizontal texture repeats, as bits.
    scale_s: u32,
    /// Vertical texture repeats, as bits.
    scale_t: u32,
    /// Horizontal texture offset, as bits.
    offset_s: u32,
    /// Vertical texture offset, as bits.
    offset_t: u32,
    /// Texture rotation in radians, as bits.
    rotation: u32,
    /// The packed bump / shiny / fullbright byte (bump is always `0` here — a
    /// bumped face is excluded — but shiny / fullbright shape the material).
    bump_shiny_fullbright: u8,
    /// The packed media / texture-generation flags byte (media is always clear
    /// here — a media face is excluded).
    media_flags: u8,
    /// Glow amount, as bits.
    glow: u32,
    /// How the diffuse texture's alpha channel is treated once it decodes.
    texture_alpha: TextureAlpha,
}

impl MaterialKey {
    /// The key of `face`'s material under alpha mode `texture_alpha`.
    pub(crate) const fn new(face: &TextureFace, texture_alpha: TextureAlpha) -> Self {
        Self {
            texture_id: face.texture_id,
            color: face.color,
            scale_s: face.scale_s.to_bits(),
            scale_t: face.scale_t.to_bits(),
            offset_s: face.offset_s.to_bits(),
            offset_t: face.offset_t.to_bits(),
            rotation: face.rotation.to_bits(),
            bump_shiny_fullbright: face.bump_shiny_fullbright,
            media_flags: face.media_flags,
            glow: face.glow.to_bits(),
            texture_alpha,
        }
    }
}

/// A cumulative snapshot of the cache counters for the pipeline panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MaterialCacheStats {
    /// Distinct material keys currently cached.
    pub(crate) entries: usize,
    /// Faces that revived a live shared material (no compose ran).
    pub(crate) hits: u64,
    /// Internable faces that composed a fresh material (now shared forward).
    pub(crate) misses: u64,
    /// Faces excluded from interning (per-instance mutation expected).
    pub(crate) excluded: u64,
}

/// The viewer-wide cross-instance face-material cache resource. See the module
/// docs for the design; [`intern_face_material`](crate::textures) is the only
/// writer.
#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialCache {
    /// The cached materials by content key (weak ids only).
    entries: HashMap<MaterialKey, AssetId<FaceMaterial>>,
    /// Faces that revived a live shared material.
    hits: u64,
    /// Internable faces that composed (and recorded) a fresh material.
    misses: u64,
    /// Faces excluded from interning.
    excluded: u64,
}

impl MaterialCache {
    /// Try to revive the shared material cached under `key`: `None` when the
    /// key was never recorded or its asset died (every sharer despawned) — the
    /// caller composes a fresh material and [`record`](Self::record)s it over
    /// the dead id.
    pub(crate) fn revive(
        &self,
        key: &MaterialKey,
        materials: &mut Assets<FaceMaterial>,
    ) -> Option<Handle<FaceMaterial>> {
        let id = self.entries.get(key)?;
        materials.get_strong_handle(*id)
    }

    /// Record the material composed for `key`, overwriting a dead prior id.
    pub(crate) fn record(&mut self, key: MaterialKey, id: AssetId<FaceMaterial>) {
        let _previous = self.entries.insert(key, id);
    }

    /// Count a face that revived a live shared material.
    pub(crate) const fn note_hit(&mut self) {
        self.hits = self.hits.saturating_add(1);
    }

    /// Count an internable face that composed a fresh material.
    pub(crate) const fn note_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
    }

    /// Count a face excluded from interning.
    pub(crate) const fn note_excluded(&mut self) {
        self.excluded = self.excluded.saturating_add(1);
    }

    /// A snapshot of the counters for the pipeline panel.
    pub(crate) fn stats(&self) -> MaterialCacheStats {
        MaterialCacheStats {
            entries: self.entries.len(),
            hits: self.hits,
            misses: self.misses,
            excluded: self.excluded,
        }
    }

    /// Drop every entry whose material asset died (every sharing face
    /// despawned — object removal, region teardown). The assets themselves are
    /// freed by Bevy the moment their last face despawns; this is bookkeeping.
    pub(crate) fn prune(&mut self, materials: &Assets<FaceMaterial>) {
        self.entries.retain(|_key, id| materials.contains(*id));
    }
}

/// How often the periodic [`prune_material_cache`] sweep runs (via an
/// `on_timer` run condition in `main`), matching the geometry cache's cadence —
/// prune latency only delays freeing the (small) bookkeeping entries.
pub(crate) const PRUNE_INTERVAL: Duration = Duration::from_secs(30);

/// System: periodically drop cache entries whose shared materials died (see
/// [`MaterialCache::prune`]).
pub(crate) fn prune_material_cache(
    mut cache: ResMut<MaterialCache>,
    materials: Res<Assets<FaceMaterial>>,
) {
    cache.prune(&materials);
}

/// Marker on a face entity whose material handle is (or seeded) a
/// [`MaterialCache`] entry: mutating that material in place would leak into
/// every sharer, so any system about to do so must first detach the face
/// ([`detach_shared_face_materials`]). Removed on detach.
#[derive(Component, Debug)]
pub(crate) struct SharedFaceMaterial;

/// The object-level inputs of the per-face intern decision, computed once per
/// geometry build from the live [`Object`] and retained (cloned) in the
/// deferred-rebuild structs (`PendingPrim` / `PendingMesh` / `PendingSculpt`),
/// which rebuild faces without the `Object` at hand.
#[derive(Debug, Clone, Default)]
pub(crate) struct MaterialInternContext {
    /// The object's **running** texture animation, if any — its target faces'
    /// materials get per-instance GPU animation params.
    texture_animation: Option<TextureAnimation>,
    /// The face indices covered by the object's PBR (GLTF) render materials —
    /// those faces' materials are rewritten by the P27.1 pipeline.
    pbr_faces: Vec<u8>,
    /// Whether the object belongs to a HUD attachment — every HUD face's
    /// material is forced fullbright (`unlit`) in place by
    /// [`apply_hud_fullbright`](crate::hud::apply_hud_fullbright).
    hud: bool,
}

impl MaterialInternContext {
    /// The intern context of `object`. `hud` is passed in (rather than derived
    /// here) because recognising a HUD linkset **child** needs the tracked
    /// parent chain, which only the object-lifecycle caller holds.
    pub(crate) fn for_object(object: &Object, hud: bool) -> Self {
        Self {
            texture_animation: running_texture_animation(object.texture_animation),
            pbr_faces: object
                .extra
                .render_material
                .iter()
                .map(|reference| reference.face)
                .collect(),
            hud,
        }
    }

    /// Whether the face `face_id` / `face` may share a cached material: false
    /// for every face some system mutates per instance after spawn — a legacy
    /// (`material_id`) face, a bump-mapped face, a media face, a texture-
    /// animated face, a PBR-covered face, and any HUD face.
    pub(crate) fn internable(&self, face_id: PrimFaceId, face: &TextureFace) -> bool {
        !self.hud
            && !face.material_id.is_some_and(|id| !id.is_nil())
            && face.bumpmap() == 0
            && !face.media_enabled()
            && !self
                .texture_animation
                .as_ref()
                .is_some_and(|anim| anim_applies_to_face(anim, face_id.get()))
            && !self
                .pbr_faces
                .iter()
                .any(|&index| u16::from(index) == face_id.get())
    }
}

/// Give `face` a private (unshared) material recomposed from its
/// [`FaceTextureDebug`] texture entry, and drop its [`SharedFaceMaterial`]
/// marker. A full recompose (not a clone) so the [`PrimTextures`] pending /
/// LOD bookkeeping is re-registered for the new handle; the texture was
/// already requested at spawn priority, so the re-request rides idle.
fn detach_face(
    face: Entity,
    texture_face: &TextureFace,
    commands: &mut Commands,
    materials: &mut Assets<FaceMaterial>,
    manager: &mut TextureManager,
    prim_textures: &mut PrimTextures,
) {
    let private = face_material(
        texture_face,
        materials,
        manager,
        prim_textures,
        Priority::IDLE,
        TextureAlpha::Mask,
    );
    commands
        .entity(face)
        .insert(MeshMaterial3d(private))
        .remove::<SharedFaceMaterial>();
}

/// System (`PreUpdate`): copy-on-write **detach net** for the mutation sources
/// that arrive without a texture-entry change (which would re-build the faces
/// and re-evaluate the intern decision): a late-started texture animation, PBR
/// render materials assigned to existing faces, a face routed onto the HUD
/// layer, and the faces of any *selected* object (whose materials the edit
/// floaters' live previews write). Each such face still marked
/// [`SharedFaceMaterial`] gets a private material before this frame's `Update`
/// mutators run — the trigger components / layers are themselves applied at an
/// earlier frame boundary, so the swap always lands first. The marker filter
/// makes the steady-state sweep free (all queries converge to empty).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system joining the mutation-source queries with the ECS resources the recompose needs"
)]
pub(crate) fn detach_shared_face_materials(
    mut commands: Commands,
    anim_holders: Query<(Entity, &ObjectTextureAnimation)>,
    pbr_holders: Query<(Entity, &ObjectRenderMaterials)>,
    children: Query<&Children>,
    shared_faces: Query<(&PrimFaceEntity, &FaceTextureDebug), With<SharedFaceMaterial>>,
    hud_faces: Query<(Entity, &RenderLayers), With<SharedFaceMaterial>>,
    selection: Res<SelectionSet>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut manager: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
) {
    // A face can match several sources at once (an animated face on a selected
    // HUD, say); detach it once.
    let mut detached: HashSet<Entity> = HashSet::new();
    // A running animation's target faces get per-instance GPU anim params.
    for (holder, tex_anim) in &anim_holders {
        let Ok(face_entities) = children.get(holder) else {
            continue;
        };
        for &face in face_entities {
            let Ok((prim_face, texture)) = shared_faces.get(face) else {
                continue;
            };
            if !tex_anim.applies_to_face(prim_face.face_id.get()) {
                continue;
            }
            if detached.insert(face) {
                detach_face(
                    face,
                    &texture.0,
                    &mut commands,
                    &mut materials,
                    &mut manager,
                    &mut prim_textures,
                );
            }
        }
    }
    // A PBR-covered face's material is rewritten by the render-material
    // pipeline.
    for (holder, render_materials) in &pbr_holders {
        let Ok(face_entities) = children.get(holder) else {
            continue;
        };
        for &face in face_entities {
            let Ok((prim_face, texture)) = shared_faces.get(face) else {
                continue;
            };
            let covered = render_materials
                .faces
                .iter()
                .any(|&(index, _material)| u16::from(index) == prim_face.face_id.get());
            if !covered {
                continue;
            }
            if detached.insert(face) {
                detach_face(
                    face,
                    &texture.0,
                    &mut commands,
                    &mut materials,
                    &mut manager,
                    &mut prim_textures,
                );
            }
        }
    }
    // A face routed onto the HUD layer is forced fullbright in place.
    for (face, layers) in &hud_faces {
        if !on_hud_layer(Some(layers)) {
            continue;
        }
        let Ok((_prim_face, texture)) = shared_faces.get(face) else {
            continue;
        };
        if detached.insert(face) {
            detach_face(
                face,
                &texture.0,
                &mut commands,
                &mut materials,
                &mut manager,
                &mut prim_textures,
            );
        }
    }
    // Every selected object's faces go private pre-emptively, so the edit
    // floaters' live previews (colour / glow / fullbright writes on the
    // selection) never touch a shared material.
    for node in selection.iter() {
        for descendant in children.iter_descendants(node.entity) {
            let Ok((_prim_face, texture)) = shared_faces.get(descendant) else {
                continue;
            };
            if detached.insert(descendant) {
                detach_face(
                    descendant,
                    &texture.0,
                    &mut commands,
                    &mut materials,
                    &mut manager,
                    &mut prim_textures,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MaterialCache, MaterialCacheStats, MaterialInternContext, MaterialKey};
    use crate::face_material::FaceMaterial;
    use crate::texture_anim::running_texture_animation;
    use crate::textures::TextureAlpha;
    use bevy::prelude::*;
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        PrimFaceId, TextureAnimation, TextureFace, TextureKey, Uuid, texture_anim_mode,
    };

    /// A plain textured face with neutral placement.
    fn test_face() -> TextureFace {
        TextureFace::new(TextureKey::from(Uuid::from_u128(7)))
    }

    /// A running (`ON`) texture animation targeting wire face `face`.
    fn running_anim(face: i8) -> Option<TextureAnimation> {
        running_texture_animation(Some(TextureAnimation {
            mode: texture_anim_mode::ON,
            face,
            size_x: 1,
            size_y: 1,
            start: 0.0,
            length: 0.0,
            rate: 1.0,
        }))
    }

    /// Identical face inputs key equal; every varying input keys distinct.
    #[test]
    fn key_is_exact_on_content() {
        let face = test_face();
        let key = MaterialKey::new(&face, TextureAlpha::Mask);
        assert_eq!(key, MaterialKey::new(&test_face(), TextureAlpha::Mask));
        let mut retextured = face;
        retextured.texture_id = TextureKey::from(Uuid::from_u128(8));
        let mut tinted = face;
        tinted.color = [255, 255, 255, 254];
        let mut offset = face;
        offset.offset_s = f32::from_bits(face.offset_s.to_bits().wrapping_add(1));
        let mut glowing = face;
        glowing.glow = 0.5;
        let mut shiny = face;
        shiny.bump_shiny_fullbright = 0x40;
        for other in [&retextured, &tinted, &offset, &glowing, &shiny] {
            assert_ne!(key, MaterialKey::new(other, TextureAlpha::Mask));
        }
        assert_ne!(key, MaterialKey::new(&face, TextureAlpha::Blend));
    }

    /// A recorded material revives with a strong handle to the same asset; an
    /// unrecorded key revives to `None`.
    #[test]
    fn revive_shares_while_alive() {
        let mut cache = MaterialCache::default();
        let mut materials = Assets::<FaceMaterial>::default();
        let handle = materials.add(FaceMaterial::default());
        let key = MaterialKey::new(&test_face(), TextureAlpha::Mask);
        cache.record(key, handle.id());
        assert_eq!(
            cache.revive(&key, &mut materials).map(|shared| shared.id()),
            Some(handle.id())
        );
        let other = MaterialKey::new(&test_face(), TextureAlpha::Blend);
        assert!(
            cache.revive(&other, &mut materials).is_none(),
            "an unrecorded key has nothing to revive"
        );
    }

    /// A dead entry stops reviving, is dropped by the prune, and a re-record
    /// over the same key revives the new asset.
    #[test]
    fn dead_entry_misses_and_prunes() {
        let mut cache = MaterialCache::default();
        let mut materials = Assets::<FaceMaterial>::default();
        let dead = materials.add(FaceMaterial::default());
        let key = MaterialKey::new(&test_face(), TextureAlpha::Mask);
        cache.record(key, dead.id());
        materials.remove(dead.id());
        assert!(cache.revive(&key, &mut materials).is_none());
        cache.prune(&materials);
        assert_eq!(cache.stats().entries, 0);
        let alive = materials.add(FaceMaterial::default());
        cache.record(key, alive.id());
        assert_eq!(
            cache.revive(&key, &mut materials).map(|shared| shared.id()),
            Some(alive.id())
        );
    }

    /// The counters saturate upward through the three note paths.
    #[test]
    fn stats_count_the_outcomes() {
        let mut cache = MaterialCache::default();
        cache.note_hit();
        cache.note_hit();
        cache.note_miss();
        cache.note_excluded();
        assert_eq!(
            cache.stats(),
            MaterialCacheStats {
                entries: 0,
                hits: 2,
                misses: 1,
                excluded: 1,
            }
        );
    }

    /// Each exclusion — HUD, legacy material, bump, media, a matching texture
    /// animation, a PBR-covered face — flips a plain face non-internable.
    #[test]
    fn internable_excludes_mutating_faces() {
        let context = MaterialInternContext::default();
        let face_id = PrimFaceId::new(2);
        let face = test_face();
        assert!(context.internable(face_id, &face));

        let hud = MaterialInternContext {
            hud: true,
            ..MaterialInternContext::default()
        };
        assert!(!hud.internable(face_id, &face));

        let mut legacy = face;
        legacy.material_id = Some(Uuid::from_u128(9));
        assert!(!context.internable(face_id, &legacy));
        let mut nil_legacy = face;
        nil_legacy.material_id = Some(Uuid::nil());
        assert!(
            context.internable(face_id, &nil_legacy),
            "a nil material id is 'no legacy material'"
        );

        let mut bumped = face;
        bumped.bump_shiny_fullbright = 0x01;
        assert!(!context.internable(face_id, &bumped));
        let mut shiny = face;
        shiny.bump_shiny_fullbright = 0x40;
        assert!(
            context.internable(face_id, &shiny),
            "shiny without bump is composed at build time, not mutated later"
        );

        let mut media = face;
        media.media_flags = 0x01;
        assert!(!context.internable(face_id, &media));

        let all_faces_anim = MaterialInternContext {
            texture_animation: running_anim(-1),
            ..MaterialInternContext::default()
        };
        assert!(!all_faces_anim.internable(face_id, &face));
        let other_face_anim = MaterialInternContext {
            texture_animation: running_anim(1),
            ..MaterialInternContext::default()
        };
        assert!(other_face_anim.internable(face_id, &face));
        assert!(!other_face_anim.internable(PrimFaceId::new(1), &face));

        let pbr = MaterialInternContext {
            pbr_faces: vec![2],
            ..MaterialInternContext::default()
        };
        assert!(!pbr.internable(face_id, &face));
        assert!(pbr.internable(PrimFaceId::new(0), &face));
    }
}
