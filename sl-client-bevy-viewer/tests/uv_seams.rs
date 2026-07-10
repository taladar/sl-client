//! Offline UV-seam smoothness check for a bake-on-mesh avatar part (R22).
//!
//! The mesh-body arm is one submesh with smooth geometry, yet a textured render
//! showed hard tone "seams" at the elbow / wrist. This decodes the actual arm mesh
//! and the `upper` server bake **from the viewer's disk cache** and measures, via
//! [`sl_mesh::analyze_uv_seams`], how much the bake colour jumps across the mesh's
//! UV-island seams — the direct, permanent test of "does the bake map smoothly onto
//! this mesh". It samples both the renderer's `v -> 1 - v` orientation and the
//! un-flipped one, so a wrong orientation shows up as the smoother of the two.
//!
//! It needs the on-disk caches populated by a prior viewer run, so it is `#[ignore]`
//! by default. Run it after visiting the avatar:
//!
//! ```console
//! SL_SEAM_MESH=<mesh-uuid> SL_SEAM_BAKE=<bake-uuid> \
//!   cargo test -p sl-client-bevy-viewer --test uv_seams -- --ignored --nocapture
//! ```
//!
//! The UUIDs default to the mesh body / `upper` bake this was first diagnosed on.

use std::path::PathBuf;

use sl_mesh::{
    CacheLimits as MeshCacheLimits, MeshDiskCache, MeshLod, SeamHit, Submesh, decode_lod,
    parse_header, uv_seam_hits,
};
use sl_proto::{DiscardLevel, Uuid};
use sl_texture::{CacheLimits as TexCacheLimits, DecodedImage, TextureDiskCache, decode_j2c};

/// The mesh-body upper (arm) mesh this was first diagnosed on.
const DEFAULT_MESH: &str = "a2a889c4-0d5a-be3f-61c4-d1def17aafc0";
/// The `upper` server bake that mesh samples via bake-on-mesh.
const DEFAULT_BAKE: &str = "fbefcc98-4a55-1f81-e7bf-be42d19bc5b2";

/// Two vertices count as the same 3D point below this position distance. Seam
/// duplicates are bit-identical, and SL positions are u16-quantised (~`5e-5`
/// steps), so this is kept well under a quantisation step to avoid merging distinct
/// nearby vertices into false "seams".
const POSITION_EPSILON: f32 = 1e-6;
/// A per-channel colour delta (0..1) at or above which a seam reads as visible.
const VISIBLE_DELTA: f32 = 0.06;

/// The viewer's cache base directory (`$SL_CACHE_DIR`, else `~/.cache/sl-client-bevy-viewer`).
fn cache_base() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SL_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".cache/sl-client-bevy-viewer"))
}

/// One RGB texel (sRGB bytes → `0..1`) of an RGBA8 decoded image, or black if out
/// of range.
fn texel(bake: &DecodedImage, x: usize, y: usize) -> [f32; 3] {
    let width = usize::try_from(bake.width).unwrap_or(0);
    let base = y.saturating_mul(width).saturating_add(x).saturating_mul(4);
    let channel = |offset: usize| {
        base.checked_add(offset)
            .and_then(|index| bake.pixels.get(index))
            .map_or(0.0, |&byte| f32::from(byte) / 255.0)
    };
    [channel(0), channel(1), channel(2)]
}

/// Bilinearly sample `bake` at UV `(u, v)` (clamped to the image), returning sRGB
/// `0..1`. The `f32 <-> integer` texel conversions are bounded to the image.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "bounded, clamped bilinear texel indexing in an offline diagnostic test"
)]
fn sample_bilinear(bake: &DecodedImage, u: f32, v: f32) -> [f32; 3] {
    let width = bake.width.max(1);
    let height = bake.height.max(1);
    let max_x = usize::try_from(width.saturating_sub(1)).unwrap_or(usize::MAX);
    let max_y = usize::try_from(height.saturating_sub(1)).unwrap_or(usize::MAX);
    let fx = (u.clamp(0.0, 1.0) * (width as f32 - 1.0)).max(0.0);
    let fy = (v.clamp(0.0, 1.0) * (height as f32 - 1.0)).max(0.0);
    let x0 = (fx as usize).min(max_x);
    let y0 = (fy as usize).min(max_y);
    let x1 = x0.saturating_add(1).min(max_x);
    let y1 = y0.saturating_add(1).min(max_y);
    let tx = fx - fx.floor();
    let ty = fy - fy.floor();
    let lerp = |a: f32, b: f32, t: f32| a * (1.0 - t) + b * t;
    let bilerp =
        |c00: f32, c10: f32, c01: f32, c11: f32| lerp(lerp(c00, c10, tx), lerp(c01, c11, tx), ty);
    let a = texel(bake, x0, y0);
    let b = texel(bake, x1, y0);
    let c = texel(bake, x0, y1);
    let d = texel(bake, x1, y1);
    [
        bilerp(a[0], b[0], c[0], d[0]),
        bilerp(a[1], b[1], c[1], d[1]),
        bilerp(a[2], b[2], c[2], d[2]),
    ]
}

#[test]
#[ignore = "needs the on-disk caches populated by a prior viewer run"]
#[expect(
    clippy::tests_outside_test_module,
    clippy::print_stderr,
    clippy::expect_used,
    clippy::type_complexity,
    reason = "an integration-test binary is a test; it reports on stderr and fails loudly"
)]
fn bake_maps_smoothly_across_mesh_uv_seams() {
    let Some(base) = cache_base() else {
        eprintln!("SKIP: no HOME / SL_CACHE_DIR");
        return;
    };
    let mesh_id: Uuid = std::env::var("SL_SEAM_MESH")
        .unwrap_or_else(|_| DEFAULT_MESH.to_owned())
        .parse()
        .expect("valid mesh uuid");
    let bake_id: Uuid = std::env::var("SL_SEAM_BAKE")
        .unwrap_or_else(|_| DEFAULT_BAKE.to_owned())
        .parse()
        .expect("valid bake uuid");

    let mesh_cache = MeshDiskCache::open(base.join("meshcache"), MeshCacheLimits::default())
        .expect("open mesh cache");
    let tex_cache = TextureDiskCache::open(base.join("texturecache"), TexCacheLimits::default())
        .expect("open texture cache");

    let Some(mesh_bytes) = mesh_cache.read(mesh_id) else {
        eprintln!("SKIP: mesh {mesh_id} not in cache — visit the avatar first");
        return;
    };
    let Some(bake_bytes) = tex_cache.read(bake_id) else {
        eprintln!("SKIP: bake {bake_id} not in cache — visit the avatar first");
        return;
    };

    // `decode_lod` wants the extracted LOD block, not the whole asset — slice it out
    // via the header. Prefer the finest LOD the cached asset actually holds (a
    // partially fetched mesh may lack the high block); the UV layout is the same.
    let asset = mesh_bytes.data();
    let (header, header_size) = parse_header(asset).expect("parse mesh header");
    let mut decoded = None;
    for lod in [
        MeshLod::High,
        MeshLod::Medium,
        MeshLod::Low,
        MeshLod::Lowest,
    ] {
        let Some(block) = header.lod(lod) else {
            continue;
        };
        let (start, end) = block.range(header_size);
        let Some(bytes) = asset.get(start..end) else {
            continue;
        };
        if let Ok(mesh) = decode_lod(bytes, lod) {
            eprintln!("decoded mesh at {lod:?} ({} block bytes)", bytes.len());
            decoded = Some(mesh);
            break;
        }
    }
    let mesh = decoded.expect("no mesh LOD decoded from the cached asset");
    let bake = decode_j2c(&bake_bytes, DiscardLevel::FULL).expect("decode bake");
    eprintln!(
        "mesh {mesh_id}: {} submesh(es); bake {}x{} {}c",
        mesh.submeshes.len(),
        bake.width,
        bake.height,
        bake.components
    );

    // The eight axis-aligned UV orientations. The renderer uses `(u, 1-v)`; if a
    // different one collapses the seams, our UV handling is wrong, otherwise the
    // seams are inherent to how this mesh's UVs sample this bake.
    let orientations: [(&str, fn([f32; 2]) -> [f32; 2]); 8] = [
        ("u, v", |uv| [uv[0], uv[1]]),
        ("u, 1-v  (renderer)", |uv| [uv[0], 1.0 - uv[1]]),
        ("1-u, v", |uv| [1.0 - uv[0], uv[1]]),
        ("1-u, 1-v", |uv| [1.0 - uv[0], 1.0 - uv[1]]),
        ("v, u", |uv| [uv[1], uv[0]]),
        ("1-v, u", |uv| [1.0 - uv[1], uv[0]]),
        ("v, 1-u", |uv| [uv[1], 1.0 - uv[0]]),
        ("1-v, 1-u", |uv| [1.0 - uv[1], 1.0 - uv[0]]),
    ];
    for (index, submesh) in mesh.submeshes.iter().enumerate() {
        if submesh.no_geometry || submesh.uvs.is_empty() {
            continue;
        }
        // Connectivity radius for grouping seam vertices into distinct seam *lines*:
        // a fraction of the mesh's bounding-box diagonal, so consecutive vertices
        // along one ring chain into one cluster.
        let diagonal = bbox_diagonal(submesh);
        let radius = diagonal * 0.03;
        eprintln!(
            "  submesh {index}: {} verts, bbox diagonal {diagonal:.3}, cluster radius {radius:.4}",
            submesh.positions.len(),
        );
        for (name, transform) in orientations {
            let hits = uv_seam_hits(submesh, POSITION_EPSILON, |uv| {
                let t = transform(uv);
                sample_bilinear(&bake, t[0], t[1])
            });
            let visible: Vec<SeamHit> = hits
                .into_iter()
                .filter(|hit| hit.delta >= VISIBLE_DELTA)
                .collect();
            let lines = cluster_lines(&visible, radius);
            // A "prominent" line is a long one (many vertices) — the kind eyeballed
            // as a seam ring, as opposed to a tiny finger/detail island edge.
            let prominent = lines.iter().filter(|line| line.count >= 30).count();
            eprintln!(
                "    {name:<20} {} visible seam vertices -> {} seam line(s), \
                 {prominent} prominent; peak deltas: {}",
                visible.len(),
                lines.len(),
                summarise_peaks(&lines),
            );
        }
    }
}

/// The bounding-box diagonal length of a submesh (mesh-local units).
fn bbox_diagonal(submesh: &Submesh) -> f32 {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for position in &submesh.positions {
        for axis in 0..3 {
            let value = position.get(axis).copied().unwrap_or(0.0);
            let l = lo.get(axis).copied().unwrap_or(0.0);
            let h = hi.get(axis).copied().unwrap_or(0.0);
            if let Some(slot) = lo.get_mut(axis) {
                *slot = l.min(value);
            }
            if let Some(slot) = hi.get_mut(axis) {
                *slot = h.max(value);
            }
        }
    }
    let dx = hi[0] - lo[0];
    let dy = hi[1] - lo[1];
    let dz = hi[2] - lo[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// One clustered seam line: its member count and peak colour delta.
struct SeamLine {
    count: usize,
    peak: f32,
}

/// Group visible seam hits into connected seam *lines* (union-find: two hits join
/// when within `radius`), so a ring of many vertices is one line — matching what a
/// viewer eyeballs as a single seam.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::needless_range_loop,
    reason = "bounded union-find over an in-memory hit list in a diagnostic test"
)]
fn cluster_lines(hits: &[SeamHit], radius: f32) -> Vec<SeamLine> {
    let n = hits.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut Vec<usize>, mut i: usize| -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    };
    let r2 = radius * radius;
    for i in 0..n {
        for j in (i + 1)..n {
            let a = hits[i].position;
            let b = hits[j].position;
            let dx = a[0] - b[0];
            let dy = a[1] - b[1];
            let dz = a[2] - b[2];
            if dx * dx + dy * dy + dz * dz <= r2 {
                let ra = find(&mut parent, i);
                let rb = find(&mut parent, j);
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
    }
    let mut lines: std::collections::HashMap<usize, SeamLine> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        let line = lines.entry(root).or_insert(SeamLine {
            count: 0,
            peak: 0.0,
        });
        line.count += 1;
        line.peak = line.peak.max(hits[i].delta);
    }
    lines.into_values().collect()
}

/// Report the raw UV bounds of each submesh of a cached mesh (R22h): a mesh-body
/// region whose UVs leave `[0, 1]` renders white under a **clamp** sampler but tiles
/// correctly under **repeat** (Firestorm's GL_REPEAT), so this pins whether the
/// white-torso bug is an out-of-range-UV + wrong-sampler issue. Reuses the
/// `SL_SEAM_MESH` cached mesh.
#[test]
#[ignore = "needs the on-disk mesh cache populated by a prior viewer run"]
#[expect(
    clippy::tests_outside_test_module,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "an integration-test binary is a test; it reports on stderr and fails loudly"
)]
fn mesh_uv_bounds() {
    let Some(base) = cache_base() else {
        eprintln!("SKIP: no HOME / SL_CACHE_DIR");
        return;
    };
    let mesh_id: Uuid = std::env::var("SL_SEAM_MESH")
        .unwrap_or_else(|_| DEFAULT_MESH.to_owned())
        .parse()
        .expect("valid mesh uuid");
    let mesh_cache = MeshDiskCache::open(base.join("meshcache"), MeshCacheLimits::default())
        .expect("open mesh cache");
    let Some(mesh_bytes) = mesh_cache.read(mesh_id) else {
        eprintln!("SKIP: mesh {mesh_id} not in cache — visit the avatar first");
        return;
    };
    let asset = mesh_bytes.data();
    let (header, header_size) = parse_header(asset).expect("parse mesh header");
    let mut decoded = None;
    for lod in [
        MeshLod::High,
        MeshLod::Medium,
        MeshLod::Low,
        MeshLod::Lowest,
    ] {
        let Some(block) = header.lod(lod) else {
            continue;
        };
        let (start, end) = block.range(header_size);
        if let Some(bytes) = asset.get(start..end)
            && let Ok(mesh) = decode_lod(bytes, lod)
        {
            decoded = Some(mesh);
            break;
        }
    }
    let mesh = decoded.expect("no mesh LOD decoded");
    eprintln!("mesh {mesh_id}: {} submesh(es)", mesh.submeshes.len());
    for (index, submesh) in mesh.submeshes.iter().enumerate() {
        if submesh.uvs.is_empty() {
            continue;
        }
        let mut u_min = f32::INFINITY;
        let mut u_max = f32::NEG_INFINITY;
        let mut v_min = f32::INFINITY;
        let mut v_max = f32::NEG_INFINITY;
        let mut outside = 0_usize;
        for &[u, v] in &submesh.uvs {
            u_min = u_min.min(u);
            u_max = u_max.max(u);
            v_min = v_min.min(v);
            v_max = v_max.max(v);
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                outside = outside.saturating_add(1);
            }
        }
        eprintln!(
            "  submesh {index}: {} verts, u[{u_min:.3}, {u_max:.3}] v[{v_min:.3}, {v_max:.3}], \
             {outside} vert(s) outside [0,1]",
            submesh.uvs.len(),
        );
    }
}

/// The peak deltas of the largest seam lines, sorted worst-first, as text.
fn summarise_peaks(lines: &[SeamLine]) -> String {
    let mut peaks: Vec<f32> = lines
        .iter()
        .filter(|line| line.count >= 8)
        .map(|line| line.peak)
        .collect();
    peaks.sort_by(|a, b| b.total_cmp(a));
    peaks
        .iter()
        .take(8)
        .map(|peak| format!("{peak:.2}"))
        .collect::<Vec<_>>()
        .join(", ")
}
