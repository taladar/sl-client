//! Cross-instance **geometry cache**: shared Bevy [`Mesh`] handles for every
//! object instance whose geometry is byte-identical (roadmap
//! `viewer-perf-prim-tessellation-cache`).
//!
//! Second Life regions are full of copy-pasted identical geometry — default
//! boxes, fence posts, a stand of one tree, a vendor wall of identical boxes —
//! but the wire protocol has no instancing, so every object arrives as its own
//! update. Before this cache each instance tessellated its own shape (prims,
//! sculpts) or re-converted the shared decoded mesh asset (meshes) and
//! uploaded its own GPU buffers. The cache keys geometry by *content* — shape
//! parameters + level of detail for a prim, sculpt map + type + decoded size
//! for a sculpt, asset id + level of detail for a mesh — and shares one
//! [`Mesh`] asset per (key, face) across every instance, so N copies cost one
//! tessellation and one GPU upload plus N transforms.
//!
//! **Texture independence:** the key deliberately excludes the texture entry —
//! same-shape / different-texture prims are everywhere (that vendor wall), and
//! per-face texture placement lives in the material's `uv_transform`, not the
//! mesh. The one exception is a **planar-texgen** face, whose UV0 is baked
//! from the object scale
//! ([`apply_planar_texgen`](crate::objects); see the module docs there): those
//! faces are shared per *quantized object scale* instead of unconditionally,
//! so same-scale copies (the common copy-paste case) still share.
//!
//! **Lifetime:** the cache stores weak [`AssetId`]s only — never strong
//! [`Handle`]s, which would pin every mesh forever. A spawn *revives* a shared
//! asset via [`Assets::get_strong_handle`], which succeeds exactly while some
//! live face entity still holds the asset; once the last instance despawns
//! (object removal, region teardown) Bevy frees the asset and the dead cache
//! entry is dropped by the periodic [`prune_geometry_cache`] sweep. No
//! teleport hook is needed.

use bevy::prelude::*;
use sl_client_bevy::{MeshKey, MeshLod, PrimFaceId, PrimLod, PrimShapeParams, TextureKey};
use std::collections::HashMap;
use std::time::Duration;

/// An object scale quantized to millimetres per axis, the key under which a
/// planar-texgen face's scale-dependent UV variant is shared (sub-millimetre
/// scale differences produce imperceptible planar UV differences, the same
/// trade the grass spread fingerprint makes).
pub(crate) type ScaleMm = (i32, i32, i32);

/// Quantize an object scale (metres per axis) to [`ScaleMm`].
pub(crate) fn scale_mm(scale: [f32; 3]) -> ScaleMm {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "object scale in mm is far inside i32 range"
    )]
    let quantize = |metres: f32| (metres * 1000.0).round() as i32;
    let [x, y, z] = scale;
    (quantize(x), quantize(y), quantize(z))
}

/// The identity of one distinct piece of face geometry, shared across every
/// object instance that produces byte-identical vertex data. Texture identity
/// is deliberately excluded (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GeometryKey {
    /// A plain prim: its quantized wire shape parameters at one tessellation
    /// level. `PrimShapeParams` is all-integer, and `sl_prim::tessellate` is a
    /// pure function of the dequantized shape and the level, so equal keys
    /// yield byte-identical geometry.
    Prim {
        /// The quantized path/profile shape parameters.
        shape: PrimShapeParams,
        /// The client-tessellation level of detail.
        lod: PrimLod,
    },
    /// A sculpted prim: its map asset, sculpt-type byte (including the
    /// invert / mirror flags), and the decoded map's pixel size — a re-decode
    /// of the same map at another discard level produces different geometry,
    /// so the dimensions make it a clean different key (the stale-resolution
    /// entry dies by pruning once its instances rebuild).
    Sculpt {
        /// The sculpt map texture asset id.
        map: TextureKey,
        /// The raw sculpt-type byte (type + invert / mirror flags).
        sculpt_type: u8,
        /// The decoded map width in pixels.
        width: u32,
        /// The decoded map height in pixels.
        height: u32,
    },
    /// A mesh asset at one decoded level of detail. The decoded geometry is
    /// already shared per asset (`MeshManager`); this key additionally shares
    /// the *converted Bevy meshes* across instances.
    Mesh {
        /// The mesh asset id.
        mesh: MeshKey,
        /// The level of detail the shared submeshes were decoded from.
        lod: MeshLod,
    },
}

/// One cached non-empty face of a geometry: its Linden face id and the shared
/// mesh asset(s) built for it so far.
#[derive(Debug, Clone, Default)]
pub(crate) struct FaceSlot {
    /// The scale-independent variant (the face as tessellated, UVs from the
    /// sweep / decode) — `None` until some shareable (non-planar-texgen)
    /// instance has built it, or after the asset died and was pruned.
    shared: Option<AssetId<Mesh>>,
    /// The planar-texgen variants, keyed by the quantized object scale their
    /// UV0 was baked from.
    planar: HashMap<ScaleMm, AssetId<Mesh>>,
}

/// The cached face layout of one geometry key.
#[derive(Debug, Clone, Default)]
struct GeometryEntry {
    /// The tessellation's total face-slot count (empty faces included) — what
    /// a reviving instance decodes its texture entry against without any
    /// geometry work.
    face_count: usize,
    /// The non-empty faces in tessellation order, keyed by Linden face id.
    faces: Vec<(PrimFaceId, FaceSlot)>,
}

/// A cumulative snapshot of the cache counters for the pipeline panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GeometryCacheStats {
    /// Distinct geometry keys currently cached.
    pub(crate) entries: usize,
    /// Spawns that revived every face — no tessellation / conversion ran.
    pub(crate) hits: u64,
    /// Spawns that ran the geometry work but revived at least one shared face.
    pub(crate) partial_hits: u64,
    /// Spawns that found nothing to revive.
    pub(crate) misses: u64,
}

/// One face of a [`GeometryCache::revive`] attempt: the Linden face id and the
/// revived shared handle, `None` where the cache had no live asset for this
/// instance's variant (so the caller must build that face itself).
#[derive(Debug, Clone)]
pub(crate) struct RevivedFace {
    /// The Linden face id (the `TextureEntry` slot the face is textured from).
    pub(crate) face_id: PrimFaceId,
    /// The revived strong handle, if the shared asset is still alive.
    pub(crate) mesh: Option<Handle<Mesh>>,
}

/// The result of a [`GeometryCache::revive`] attempt for one geometry key
/// (the total face-slot count for texture-entry decoding comes from
/// [`GeometryCache::cached_face_count`]).
#[derive(Debug, Clone)]
pub(crate) struct RevivedGeometry {
    /// One element per non-empty face, in tessellation order.
    pub(crate) faces: Vec<RevivedFace>,
}

impl RevivedGeometry {
    /// Whether every face revived — the spawn can proceed with zero geometry
    /// work.
    pub(crate) fn complete(&self) -> bool {
        self.faces.iter().all(|face| face.mesh.is_some())
    }
}

/// The viewer-wide cross-instance geometry cache resource. See the module docs
/// for the design; [`crate::objects`] is the only writer.
#[derive(Resource, Debug, Default)]
pub(crate) struct GeometryCache {
    /// The cached geometries by content key.
    entries: HashMap<GeometryKey, GeometryEntry>,
    /// Spawns that revived every face without geometry work.
    hits: u64,
    /// Spawns that revived at least one face but still ran the geometry work.
    partial_hits: u64,
    /// Spawns that revived nothing.
    misses: u64,
}

impl GeometryCache {
    /// Try to revive every cached face of `key` for an instance whose per-face
    /// planar-texgen requests are given by `is_planar` (from its own texture
    /// entry) and whose quantized scale is `scale`: a planar face looks up the
    /// scale's planar variant, every other face the scale-independent one.
    /// Returns `None` when the key has never been recorded (the caller
    /// tessellates and records); otherwise each face carries its revived
    /// strong handle or `None` where the caller must build that face.
    pub(crate) fn revive(
        &self,
        key: &GeometryKey,
        scale: ScaleMm,
        is_planar: impl Fn(PrimFaceId) -> bool,
        meshes: &mut Assets<Mesh>,
    ) -> Option<RevivedGeometry> {
        let entry = self.entries.get(key)?;
        let faces = entry
            .faces
            .iter()
            .map(|(face_id, slot)| {
                let id = if is_planar(*face_id) {
                    slot.planar.get(&scale).copied()
                } else {
                    slot.shared
                };
                RevivedFace {
                    face_id: *face_id,
                    mesh: id.and_then(|id| meshes.get_strong_handle(id)),
                }
            })
            .collect();
        Some(RevivedGeometry { faces })
    }

    /// The recorded total face-slot count of `key`, if the key was ever
    /// recorded — what a reviving instance decodes its texture entry against
    /// before any geometry work.
    pub(crate) fn cached_face_count(&self, key: &GeometryKey) -> Option<usize> {
        self.entries.get(key).map(|entry| entry.face_count)
    }

    /// Ensure `key` has an entry with `face_count` total face slots, so a
    /// geometry with no non-empty faces is still remembered (and a later
    /// identical instance revives to an empty spawn instead of
    /// re-tessellating).
    pub(crate) fn ensure_entry(&mut self, key: GeometryKey, face_count: usize) {
        let entry = self.entries.entry(key).or_default();
        entry.face_count = face_count;
    }

    /// Record the shared asset built for one face of `key`: the planar variant
    /// under `planar_scale`'s quantized scale when the face baked planar UVs,
    /// else the scale-independent variant. Creates the face slot (in call
    /// order, which is tessellation order on a fresh entry) when absent.
    pub(crate) fn record_face(
        &mut self,
        key: GeometryKey,
        face_id: PrimFaceId,
        planar_scale: Option<ScaleMm>,
        id: AssetId<Mesh>,
    ) {
        let entry = self.entries.entry(key).or_default();
        let slot = match entry
            .faces
            .iter_mut()
            .find(|(slot_id, _)| *slot_id == face_id)
        {
            Some((_, slot)) => slot,
            None => {
                entry.faces.push((face_id, FaceSlot::default()));
                let Some((_, slot)) = entry.faces.last_mut() else {
                    // Unreachable: the element was just pushed.
                    return;
                };
                slot
            }
        };
        match planar_scale {
            Some(scale) => {
                slot.planar.insert(scale, id);
            }
            None => slot.shared = Some(id),
        }
    }

    /// Count a spawn that revived every face (no geometry work ran).
    pub(crate) const fn note_hit(&mut self) {
        self.hits = self.hits.saturating_add(1);
    }

    /// Count a spawn that ran the geometry work but revived at least one face.
    pub(crate) const fn note_partial_hit(&mut self) {
        self.partial_hits = self.partial_hits.saturating_add(1);
    }

    /// Count a spawn that found nothing to revive.
    pub(crate) const fn note_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
    }

    /// A snapshot of the counters for the pipeline panel.
    pub(crate) fn stats(&self) -> GeometryCacheStats {
        GeometryCacheStats {
            entries: self.entries.len(),
            hits: self.hits,
            partial_hits: self.partial_hits,
            misses: self.misses,
        }
    }

    /// Drop every dead asset id (its mesh is gone — every instance despawned)
    /// and every entry with no live asset left. An entry whose geometry had no
    /// non-empty faces holds no assets and is dropped too — the degenerate
    /// (fully cut / dimple-closed) shape re-tessellates on a later spawn,
    /// which is rare and cheap.
    pub(crate) fn prune(&mut self, meshes: &Assets<Mesh>) {
        self.entries.retain(|_key, entry| {
            let mut live = false;
            for (_face_id, slot) in &mut entry.faces {
                if let Some(id) = slot.shared
                    && !meshes.contains(id)
                {
                    slot.shared = None;
                }
                slot.planar.retain(|_scale, id| meshes.contains(*id));
                live = live || slot.shared.is_some() || !slot.planar.is_empty();
            }
            live
        });
    }
}

/// How often the periodic [`prune_geometry_cache`] sweep runs (via an
/// `on_timer` run condition in `main`). Prune latency only delays freeing the
/// (small) bookkeeping entries — the mesh assets themselves are freed by Bevy
/// the moment their last face entity despawns — so a lazy cadence is fine.
pub(crate) const PRUNE_INTERVAL: Duration = Duration::from_secs(30);

/// System: periodically drop cache entries whose shared meshes all died (see
/// [`GeometryCache::prune`]).
pub(crate) fn prune_geometry_cache(mut cache: ResMut<GeometryCache>, meshes: Res<Assets<Mesh>>) {
    cache.prune(&meshes);
}

#[cfg(test)]
mod tests {
    use super::{GeometryCache, GeometryCacheStats, GeometryKey, scale_mm};
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{PrimFaceId, PrimLod, PrimShapeParams};

    /// A minimal mesh asset to populate a test `Assets<Mesh>` with.
    fn test_mesh() -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
    }

    /// A prim geometry key with the default shape at the given level.
    fn prim_key(lod: PrimLod) -> GeometryKey {
        GeometryKey::Prim {
            shape: PrimShapeParams::default(),
            lod,
        }
    }

    /// Object scales quantize to millimetres, so sub-millimetre differences
    /// share a planar variant and larger ones do not.
    #[test]
    fn scale_quantizes_to_millimetres() {
        assert_eq!(scale_mm([1.0, 2.0, 0.5]), (1000, 2000, 500));
        assert_eq!(scale_mm([1.0001, 2.0, 0.5]), (1000, 2000, 500));
        assert_eq!(scale_mm([1.002, 2.0, 0.5]), (1002, 2000, 500));
    }

    /// A recorded shared face revives with a fresh strong handle; an
    /// unrecorded key revives to `None`.
    #[test]
    fn shared_face_revives_while_alive() {
        let mut cache = GeometryCache::default();
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(test_mesh());
        let key = prim_key(PrimLod::Low);
        let face = PrimFaceId::new(0);
        cache.ensure_entry(key, 1);
        cache.record_face(key, face, None, handle.id());
        let Some(revived) = cache.revive(&key, scale_mm([1.0; 3]), |_face| false, &mut meshes)
        else {
            unreachable!("a recorded key revives");
        };
        assert_eq!(cache.cached_face_count(&key), Some(1));
        assert!(revived.complete());
        assert_eq!(
            revived
                .faces
                .first()
                .and_then(|face| face.mesh.as_ref())
                .map(Handle::id),
            Some(handle.id())
        );
        assert!(
            cache
                .revive(
                    &prim_key(PrimLod::High),
                    scale_mm([1.0; 3]),
                    |_face| false,
                    &mut meshes
                )
                .is_none(),
            "a different level of detail is a different key"
        );
    }

    /// A planar-texgen face revives only for the same quantized scale; a
    /// different scale (and the scale-independent variant) stay unbuilt.
    #[test]
    fn planar_face_is_shared_per_scale() {
        let mut cache = GeometryCache::default();
        let mut meshes = Assets::<Mesh>::default();
        let handle = meshes.add(test_mesh());
        let key = prim_key(PrimLod::Low);
        let face = PrimFaceId::new(0);
        cache.ensure_entry(key, 1);
        cache.record_face(key, face, Some(scale_mm([2.0; 3])), handle.id());
        let Some(same_scale) = cache.revive(&key, scale_mm([2.0; 3]), |_face| true, &mut meshes)
        else {
            unreachable!("a recorded key revives");
        };
        assert!(same_scale.complete());
        let Some(other_scale) = cache.revive(&key, scale_mm([3.0; 3]), |_face| true, &mut meshes)
        else {
            unreachable!("a recorded key revives");
        };
        assert!(!other_scale.complete());
        assert!(other_scale.faces.iter().all(|face| face.mesh.is_none()));
        let Some(non_planar) = cache.revive(&key, scale_mm([2.0; 3]), |_face| false, &mut meshes)
        else {
            unreachable!("a recorded key revives");
        };
        assert!(
            !non_planar.complete(),
            "a planar variant never stands in for the scale-independent one"
        );
    }

    /// Pruning drops a dead face's id and an entry with nothing left alive,
    /// while an entry with a live asset survives with its dead slot cleared.
    #[test]
    fn prune_drops_dead_entries() {
        let mut cache = GeometryCache::default();
        let mut meshes = Assets::<Mesh>::default();
        let dead = meshes.add(test_mesh());
        let alive = meshes.add(test_mesh());
        let dead_key = prim_key(PrimLod::Low);
        let mixed_key = prim_key(PrimLod::High);
        cache.ensure_entry(dead_key, 1);
        cache.record_face(dead_key, PrimFaceId::new(0), None, dead.id());
        cache.ensure_entry(mixed_key, 2);
        cache.record_face(mixed_key, PrimFaceId::new(0), None, dead.id());
        cache.record_face(mixed_key, PrimFaceId::new(1), None, alive.id());
        meshes.remove(dead.id());
        cache.prune(&meshes);
        assert_eq!(cache.stats().entries, 1);
        assert!(
            cache
                .revive(&dead_key, scale_mm([1.0; 3]), |_face| false, &mut meshes)
                .is_none(),
            "the fully dead entry is gone"
        );
        let Some(mixed) = cache.revive(&mixed_key, scale_mm([1.0; 3]), |_face| false, &mut meshes)
        else {
            unreachable!("the mixed entry survives");
        };
        let live_faces: Vec<bool> = mixed.faces.iter().map(|face| face.mesh.is_some()).collect();
        assert_eq!(live_faces, vec![false, true]);
    }

    /// The counters saturate upward through the three note paths.
    #[test]
    fn stats_count_the_three_outcomes() {
        let mut cache = GeometryCache::default();
        cache.note_hit();
        cache.note_hit();
        cache.note_partial_hit();
        cache.note_miss();
        assert_eq!(
            cache.stats(),
            GeometryCacheStats {
                entries: 0,
                hits: 2,
                partial_hits: 1,
                misses: 1,
            }
        );
    }
}
