//! The selection highlight a **prim, sculpt, tree or grass** face wears: its
//! view-dependent **silhouette edges**, each widened into a ribbon standing off
//! the surface — the port of the reference's `generateSilhouetteVertices` +
//! `LLSelectNode::renderOneSilhouette`.
//!
//! # What this replaces, and why
//!
//! The highlight used to be an **inverted-hull shell**: the face's own mesh drawn
//! a second time with front faces culled and an entity `Transform` scale of
//! 1.035, so the sliver that escaped the real surface read as an edge. That
//! inflate is a fraction of the **whole prim**, not of the surface it outlines,
//! and a prim's drawn surface can be far thinner than its bounding box: a 95 %
//! hollow 0.5 m box has 1.25 cm walls, while 3.5 % of 0.5 m is 1.75 cm. The
//! hollow's inner wall — whose normals point *into* the hollow, so it is back
//! facing and therefore the half the cull keeps — moved further out than the
//! outer wall stands, landing in front of it and painting the whole prim white
//! (`viewer-outline-swallows-thin-hollow-prim`). The same argument holds for any
//! thin-walled shape: a hollow cylinder, a heavily cut torus, a flattened box.
//!
//! A silhouette ribbon cannot do that, because it never moves the surface at all:
//! every vertex it emits sits on the surface or straight out along that surface's
//! own normal, so no part of one face can be displaced across another.
//!
//! # What the reference does
//!
//! `LLSelectMgr::renderSilhouettes` sends every object whose volume is **not**
//! `isMesh()` — prims, sculpts, trees, grass — to `renderOneSilhouette`, and the
//! edges it draws come from `LLVolume::generateSilhouetteVertices`
//! (`indra/llmath/llvolume.cpp`), per selected volume face:
//!
//! - each triangle is classified by the sign of `(camera - v0) · n`, with `n` the
//!   **geometric** (cross-product) normal, so a triangle is *toward* or *away*
//!   from the viewer, or degenerate when that cross product vanishes;
//! - an edge is a silhouette edge when it has no neighbouring triangle in the
//!   face at all (the face grid's boundary — the whole outline of a flat cap or
//!   of a box side), or when its neighbour is classified the other way;
//! - `renderOneSilhouette` then draws each such edge as a **quad**: the edge
//!   itself, plus the same edge pushed out along its two **vertex** normals by
//!   `silhouette_thickness`, with the pushed-out side faded to zero alpha and the
//!   colour doubled at the surface. That is the soft outward glow a selected prim
//!   wears in Second Life, and the thickness is `view_distance × 0.01 × (fov /
//!   60°)`, so it holds a constant width on screen.
//!
//! [`silhouette_mesh`] builds exactly that quad set as one `TriangleList`, in the
//! face's own space, with the fade carried in [`Mesh::ATTRIBUTE_COLOR`] (which
//! `pbr_input_from_standard_material` multiplies into the material's base colour).
//!
//! # Deliberate divergences
//!
//! - **Adjacency is derived, not tabulated.** The reference reads neighbours out
//!   of the profile/path grid it tessellated the volume from
//!   (`LLVolumeFace::generateSilhouetteEdge`); this viewer's faces arrive as
//!   plain indexed triangle lists, so the neighbour of each triangle edge is
//!   found by matching **positions** (vertices duplicated for a UV seam are one
//!   vertex for adjacency, exactly as they are in the reference's grid). That
//!   matching is view-independent, so it is the half of each rebuild that could
//!   be memoized per mesh asset if a selection of many faces ever shows up in a
//!   profile; it is not, because the caller already bounds how many faces are
//!   rebuilt per frame and a prim face is a few hundred triangles.
//! - **Each edge is emitted once.** An interior silhouette edge is seen by both
//!   of its triangles, and the reference draws it twice; into a translucent
//!   overlay a doubled quad reads as a brighter one, so this de-duplicates — the
//!   same choice [`crate::selection_wireframe`] makes for its lines.
//! - **No animated highlight texture.** The reference scrolls a texture along the
//!   ribbon (`sHighlightUAnim` / `sHighlightVAnim`); the UVs here are the ribbon's
//!   own `v` (0 at the surface, 1 at the outer edge) with no texture bound.
//! - **The inner edge is lifted** by [`crate::selection_wireframe::lift_distance`]
//!   rather than sitting exactly on the surface, which would z-fight it.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};

use crate::selection_wireframe::{lift_distance, normal_stretch};

/// The vertex colour at the silhouette edge itself: the material's outline colour
/// **doubled** and fully opaque, the reference's `color * 2` at `v[1]`/`v[3]`.
const EDGE_COLOR: [f32; 4] = [2.0, 2.0, 2.0, 1.0];

/// The vertex colour at the ribbon's outer edge: the material's colour at zero
/// alpha, so the band fades out instead of ending in a hard line — the
/// reference's `alpha = 0` at `v[0]`/`v[2]`.
const OUTER_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.0];

/// How finely positions are quantized when matching two vertices as one for
/// adjacency: 10 µm in the mesh's own units. Vertices duplicated across a UV seam
/// carry bit-identical positions in practice, so this is a guard rather than a
/// tolerance — but a face whose seam did not match exactly would have every
/// triangle edge read as a boundary, drawing the whole triangulation as ribbon.
const WELD_QUANTUM: f32 = 1e5;

/// Which way one triangle faces the viewer, the reference's `AWAY` / `TOWARDS`
/// pair plus the degenerate case it encodes as both bits at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Facing {
    /// The viewer is on the side the geometric normal points to.
    Toward,
    /// The viewer is on the other side.
    Away,
    /// The triangle has no area, so it faces neither way. The reference adopts a
    /// neighbour's facing for such a triangle; this treats it as "never an edge
    /// of its own", which is the same outcome for the grids that produce them (a
    /// collapsed pole triangle sits between two triangles that agree).
    Degenerate,
}

/// The triangles sharing one welded edge.
#[derive(Clone, Copy, Debug)]
enum EdgeUse {
    /// One triangle: a boundary edge of the surface, always a silhouette edge.
    One(usize),
    /// Two triangles: an interior edge, a silhouette edge when they disagree.
    Two(usize, usize),
    /// Three or more: a non-manifold edge no facing test can settle, treated as a
    /// boundary (drawn) so a malformed face is outlined rather than missing.
    Many,
}

/// Build the silhouette-ribbon overlay mesh for `source` as seen from
/// `view_local`, or [`None`] for anything that is not an indexed triangle list
/// carrying positions and normals (the caller then leaves the face unhighlighted
/// this frame; the reconciler retries every frame).
///
/// - `scale` is how many metres one unit of the mesh's own space is worth on each
///   axis — the entity scale the overlay is drawn under. It makes `thickness` a
///   **world** distance: each vertex moves `thickness / |scale · normal|` locally,
///   which the entity scale then stretches back to `thickness` metres.
/// - `view_local` is the camera position **in the mesh's own space**. Facing may
///   be classified there rather than in world space because an affine transform
///   preserves the sign of `(camera - v) · n`: the scale that stretches the
///   positions shrinks the normals by its inverse.
/// - `thickness` is the ribbon's width in metres — the reference's
///   `silhouette_thickness`, which its caller derives from the view distance.
///
/// The returned mesh is a `TriangleList` of two triangles per silhouette edge,
/// carrying [`Mesh::ATTRIBUTE_POSITION`], [`Mesh::ATTRIBUTE_NORMAL`],
/// [`Mesh::ATTRIBUTE_UV_0`] and [`Mesh::ATTRIBUTE_COLOR`]. An unselected-looking
/// result — no silhouette edges at all — comes back as [`None`] rather than an
/// empty mesh, because a zero-vertex mesh is its own problem for the GPU
/// allocator.
pub(crate) fn silhouette_mesh(
    source: &Mesh,
    scale: Vec3,
    view_local: Vec3,
    thickness: f32,
) -> Option<Mesh> {
    if source.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let indices = source.try_indices().ok()?;
    let Ok(Some(VertexAttributeValues::Float32x3(positions))) =
        source.try_attribute_option(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let Ok(Some(VertexAttributeValues::Float32x3(normals))) =
        source.try_attribute_option(Mesh::ATTRIBUTE_NORMAL)
    else {
        return None;
    };
    let triangles = triangles(indices);
    let welded = weld(positions);
    let adjacency = adjacency(&triangles, &welded);
    let facings = facings(&triangles, positions, view_local);
    let lift = lift_distance(positions, scale);

    let mut ribbon = Ribbon::default();
    let mut drawn: HashSet<(u32, u32)> = HashSet::new();
    for (triangle, corners) in triangles.iter().enumerate() {
        let Some(&facing) = facings.get(triangle) else {
            continue;
        };
        if facing == Facing::Degenerate {
            continue;
        }
        for (start, end) in CORNER_PAIRS {
            let (Some(&from), Some(&to)) = (corners.get(start), corners.get(end)) else {
                continue;
            };
            let Some(key) = edge_key(&welded, from, to) else {
                continue;
            };
            if !is_silhouette(&adjacency, &facings, key, triangle, facing) {
                continue;
            }
            if !drawn.insert(key) {
                continue;
            }
            ribbon.push_edge((from, to), positions, normals, scale, lift, thickness);
        }
    }
    ribbon.build()
}

/// A triangle's three edges as index pairs into its corner triple, written out
/// because the modular arithmetic that would produce them trips the workspace
/// `arithmetic_side_effects` lint.
const CORNER_PAIRS: [(usize, usize); 3] = [(0, 1), (1, 2), (2, 0)];

/// Whether the edge `key` (of `triangle`, which faces `facing`) is a silhouette
/// edge: it has no neighbour, its neighbour is non-manifold, or the neighbour
/// faces the other way. A degenerate neighbour is not an edge — the reference
/// hands such a triangle its neighbour's facing, which comes to the same thing.
fn is_silhouette(
    adjacency: &HashMap<(u32, u32), EdgeUse>,
    facings: &[Facing],
    key: (u32, u32),
    triangle: usize,
    facing: Facing,
) -> bool {
    match adjacency.get(&key) {
        None | Some(EdgeUse::One(_) | EdgeUse::Many) => true,
        Some(&EdgeUse::Two(first, second)) => {
            let neighbour = if first == triangle { second } else { first };
            match facings.get(neighbour) {
                None | Some(&Facing::Degenerate) => false,
                Some(&other) => other != facing,
            }
        }
    }
}

/// The mesh's triangles as vertex-index triples, both index widths walked as one
/// iterator so neither is copied into a wider buffer first.
fn triangles(indices: &Indices) -> Vec<[u32; 3]> {
    match indices {
        Indices::U16(values) => values
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[a, b, c]| [u32::from(a), u32::from(b), u32::from(c)])
            .collect(),
        Indices::U32(values) => values.as_chunks::<3>().0.to_vec(),
    }
}

/// Map every vertex index to the index of the first vertex sharing its position,
/// so two vertices duplicated for a UV seam count as one for adjacency — which is
/// what they are in the grid the reference reads its neighbours out of.
fn weld(positions: &[[f32; 3]]) -> Vec<u32> {
    let mut representative: HashMap<[u64; 3], u32> = HashMap::new();
    let mut welded: Vec<u32> = Vec::with_capacity(positions.len());
    for (index, position) in positions.iter().enumerate() {
        let mut key = [0_u64; 3];
        for axis in 0..3_usize {
            let (Some(coordinate), Some(slot)) = (position.get(axis), key.get_mut(axis)) else {
                continue;
            };
            // Quantized in `f64` (a quantized `f32` coordinate leaves the exactly
            // representable integers behind at a fraction of a metre) and keyed by
            // the result's bits. Adding zero is not a no-op on the one value whose
            // bits would otherwise split a coordinate in two: it turns `-0.0` into
            // `0.0`, which is the same point.
            let quantized = (f64::from(*coordinate) * f64::from(WELD_QUANTUM)).round_ties_even();
            *slot = (quantized + 0.0).to_bits();
        }
        let own = u32::try_from(index).unwrap_or(u32::MAX);
        welded.push(*representative.entry(key).or_insert(own));
    }
    welded
}

/// The welded, order-independent key of the edge between two vertex indices, or
/// [`None`] when either index is outside the welding table.
fn edge_key(welded: &[u32], from: u32, to: u32) -> Option<(u32, u32)> {
    let first = *welded.get(usize::try_from(from).ok()?)?;
    let second = *welded.get(usize::try_from(to).ok()?)?;
    if first <= second {
        Some((first, second))
    } else {
        Some((second, first))
    }
}

/// Which triangles share each welded edge.
fn adjacency(triangles: &[[u32; 3]], welded: &[u32]) -> HashMap<(u32, u32), EdgeUse> {
    let mut edges: HashMap<(u32, u32), EdgeUse> = HashMap::new();
    for (triangle, corners) in triangles.iter().enumerate() {
        for corner in 0..3_usize {
            let (Some(&from), Some(&to)) = (
                corners.get(corner),
                corners.get(corner.wrapping_add(1).wrapping_rem(3)),
            ) else {
                continue;
            };
            let Some(key) = edge_key(welded, from, to) else {
                continue;
            };
            edges
                .entry(key)
                .and_modify(|use_| {
                    *use_ = match *use_ {
                        EdgeUse::One(first) => EdgeUse::Two(first, triangle),
                        EdgeUse::Two(_, _) | EdgeUse::Many => EdgeUse::Many,
                    };
                })
                .or_insert(EdgeUse::One(triangle));
        }
    }
    edges
}

/// Classify every triangle against the viewer, the reference's per-triangle
/// `fFacing` pass: the sign of `(camera - v0) · n` with `n` the cross product of
/// the triangle's own edges, and [`Facing::Degenerate`] where that cross product
/// has no length to take a sign from.
fn facings(triangles: &[[u32; 3]], positions: &[[f32; 3]], view_local: Vec3) -> Vec<Facing> {
    triangles
        .iter()
        .map(|corners| {
            let Some([first, second, third]) = corner_positions(corners, positions) else {
                return Facing::Degenerate;
            };
            let mut edge_a = [0.0_f32; 3];
            let mut edge_b = [0.0_f32; 3];
            let mut view = [0.0_f32; 3];
            let camera = view_local.to_array();
            for axis in 0..3_usize {
                let (Some(a), Some(b), Some(c)) =
                    (first.get(axis), second.get(axis), third.get(axis))
                else {
                    continue;
                };
                if let Some(slot) = edge_a.get_mut(axis) {
                    *slot = a - b;
                }
                if let Some(slot) = edge_b.get_mut(axis) {
                    *slot = b - c;
                }
                if let (Some(slot), Some(origin)) = (view.get_mut(axis), camera.get(axis)) {
                    *slot = origin - a;
                }
            }
            let normal = cross(edge_a, edge_b);
            // The reference's own degeneracy threshold, on the squared length.
            if dot(normal, normal) < 1e-8_f32 {
                return Facing::Degenerate;
            }
            if dot(view, normal) > 0.0 {
                Facing::Toward
            } else {
                Facing::Away
            }
        })
        .collect()
}

/// The three positions of one triangle, or [`None`] if any index is out of range.
fn corner_positions(corners: &[u32; 3], positions: &[[f32; 3]]) -> Option<[[f32; 3]; 3]> {
    let mut resolved = [[0.0_f32; 3]; 3];
    for corner in 0..3_usize {
        let index = usize::try_from(*corners.get(corner)?).ok()?;
        *resolved.get_mut(corner)? = *positions.get(index)?;
    }
    Some(resolved)
}

/// The cross product of two three-component vectors, written out because the glam
/// operators trip the workspace `arithmetic_side_effects` lint.
fn cross(first: [f32; 3], second: [f32; 3]) -> [f32; 3] {
    let ([ax, ay, az], [bx, by, bz]) = (first, second);
    [
        ay.mul_add(bz, -(az * by)),
        az.mul_add(bx, -(ax * bz)),
        ax.mul_add(by, -(ay * bx)),
    ]
}

/// The dot product of two three-component vectors.
fn dot(first: [f32; 3], second: [f32; 3]) -> f32 {
    let ([ax, ay, az], [bx, by, bz]) = (first, second);
    ax.mul_add(bx, ay.mul_add(by, az * bz))
}

/// The ribbon being accumulated: four vertices and six indices per silhouette
/// edge (the edge itself, lifted, and the same edge pushed out along its vertex
/// normals).
#[derive(Debug, Default)]
struct Ribbon {
    /// The quad corners, in the mesh's own space.
    positions: Vec<[f32; 3]>,
    /// Each corner's source normal — the direction it was pushed out along.
    normals: Vec<[f32; 3]>,
    /// `v = 0` at the surface, `v = 1` at the outer edge.
    uvs: Vec<[f32; 2]>,
    /// [`EDGE_COLOR`] at the surface, [`OUTER_COLOR`] at the outer edge.
    colors: Vec<[f32; 4]>,
    /// Two triangles per edge.
    indices: Vec<u32>,
}

impl Ribbon {
    /// Add one silhouette edge's quad. A vertex whose normal is degenerate under
    /// `scale` (nothing to push out along) drops the edge rather than emitting a
    /// zero-width sliver.
    fn push_edge(
        &mut self,
        edge: (u32, u32),
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        scale: Vec3,
        lift: f32,
        thickness: f32,
    ) {
        let (from, to) = edge;
        let Ok(base) = u32::try_from(self.positions.len()) else {
            return;
        };
        // Both ends are resolved before either is written, so a vertex this cannot
        // push out leaves no half-quad behind.
        let mut corners = [([0.0_f32; 3], [0.0_f32; 3], [0.0_f32; 3]); 2];
        for (end, vertex) in [from, to].into_iter().enumerate() {
            let at = usize::try_from(vertex).unwrap_or(usize::MAX);
            let (Some(position), Some(normal), Some(slot)) =
                (positions.get(at), normals.get(at), corners.get_mut(end))
            else {
                return;
            };
            let stretch = normal_stretch(*normal, scale);
            if stretch <= f32::EPSILON {
                return;
            }
            *slot = (
                offset(*position, *normal, lift / stretch),
                offset(*position, *normal, (lift + thickness) / stretch),
                *normal,
            );
        }
        for (inner, outer, normal) in corners {
            self.positions.push(inner);
            self.positions.push(outer);
            self.normals.push(normal);
            self.normals.push(normal);
            self.uvs.push([0.0, 0.0]);
            self.uvs.push([0.0, 1.0]);
            self.colors.push(EDGE_COLOR);
            self.colors.push(OUTER_COLOR);
        }
        // `inner(from), outer(from), inner(to), outer(to)` as two triangles. The
        // winding is immaterial: the outline material is drawn double-sided,
        // because a ribbon standing out of a surface is seen from either side.
        for step in [0_u32, 1, 2, 2, 1, 3] {
            self.indices.push(base.wrapping_add(step));
        }
    }

    /// The accumulated ribbon as a mesh, or [`None`] when nothing was emitted.
    fn build(self) -> Option<Mesh> {
        if self.indices.is_empty() {
            return None;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        Some(mesh)
    }
}

/// `position + normal × distance`, component-wise.
fn offset(position: [f32; 3], normal: [f32; 3], distance: f32) -> [f32; 3] {
    let mut moved = position;
    for axis in 0..3_usize {
        let (Some(slot), Some(direction)) = (moved.get_mut(axis), normal.get(axis)) else {
            continue;
        };
        *slot = direction.mul_add(distance, *slot);
    }
    moved
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        clippy::panic,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::silhouette_mesh;
    use bevy::asset::RenderAssetUsages;
    use bevy::math::Vec3;
    use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
    use pretty_assertions::assert_eq;

    /// The ribbon width every fixture asks for, in metres — big enough that a
    /// dropped offset is unmistakable in an assertion.
    const THICKNESS: f32 = 0.25;

    /// Two triangles sharing the `(1,0,0)`–`(0,1,0)` diagonal, normals along `+Z`.
    fn quad() -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0_f32, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0_f32, 0.0, 1.0]; 4]);
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        mesh
    }

    /// A closed unit cube centred on the origin, its eight corners **shared**
    /// between the faces that meet there — the welded topology a prim
    /// tessellation produces, and the one the silhouette walk needs to find a
    /// triangle's neighbours. The normals are the corner directions, which is what
    /// the ribbon is pushed out along.
    fn cube() -> Mesh {
        let corners = [
            [-0.5_f32, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ];
        let normals: Vec<[f32; 3]> = corners
            .iter()
            .map(|corner| Vec3::from_array(*corner).normalize().to_array())
            .collect();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, corners.to_vec());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_indices(Indices::U32(vec![
            // -Z, +Z
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, //
            // -Y, +Y
            0, 1, 5, 0, 5, 4, 3, 6, 2, 3, 7, 6, //
            // -X, +X
            0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5,
        ]));
        mesh
    }

    /// The ribbon's positions.
    fn positions(mesh: &Mesh) -> Vec<[f32; 3]> {
        let Some(VertexAttributeValues::Float32x3(values)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("the ribbon carries positions");
        };
        values.clone()
    }

    /// How many silhouette edges a ribbon drew — four vertices each.
    fn edges(mesh: &Mesh) -> usize {
        positions(mesh).len().wrapping_div(4)
    }

    /// A flat surface's silhouette is its **boundary**: the four edges of the
    /// quad, and not the diagonal the two triangles share, which they see from the
    /// same side. (The reference's `-1` neighbour entry, the face grid's rim.)
    #[test]
    fn a_flat_surface_outlines_its_boundary_and_not_its_seam() {
        let ribbon = silhouette_mesh(&quad(), Vec3::ONE, Vec3::new(0.0, 0.0, 10.0), THICKNESS)
            .expect("a triangle list with normals yields a ribbon");
        assert_eq!(
            ribbon.primitive_topology(),
            PrimitiveTopology::TriangleList,
            "an edge is drawn as a quad, so the ribbon is triangles"
        );
        assert_eq!(edges(&ribbon), 4, "four boundary edges, no shared diagonal");
        assert_eq!(
            ribbon.indices().map(Indices::len),
            Some(24),
            "two triangles per edge"
        );
    }

    /// A closed surface's silhouette is where the facing **flips**: seen from
    /// straight above, a cube's outline is the four edges of its top face, because
    /// each has the top (toward the viewer) on one side and a wall (away) on the
    /// other. Nothing interior to the top and nothing between two walls is drawn.
    #[test]
    fn a_closed_surface_outlines_where_the_facing_flips() {
        let ribbon = silhouette_mesh(&cube(), Vec3::ONE, Vec3::new(0.0, 0.0, 10.0), THICKNESS)
            .expect("a triangle list with normals yields a ribbon");
        assert_eq!(edges(&ribbon), 4, "the four edges of the top face");
        for position in positions(&ribbon) {
            let z = position
                .get(2)
                .copied()
                .expect("a three-component position");
            assert!(
                z > 0.0,
                "every drawn edge belongs to the top of the cube, got {position:?}"
            );
        }
    }

    /// And it follows the camera: from the side, the same cube outlines the face
    /// pointing that way instead. This is the half the reference leaves stale
    /// (it regenerates a silhouette only when the *object* moves), and the reason
    /// the caller rebuilds on a turn of the view.
    #[test]
    fn the_edge_set_follows_the_camera() {
        let cube = cube();
        let from_the_side = silhouette_mesh(&cube, Vec3::ONE, Vec3::new(10.0, 0.0, 0.0), THICKNESS)
            .expect("a triangle list with normals yields a ribbon");
        assert_eq!(edges(&from_the_side), 4, "the four edges of the +X face");
        for position in positions(&from_the_side) {
            let x = position
                .first()
                .copied()
                .expect("a three-component position");
            assert!(
                x > 0.0,
                "every drawn edge belongs to the +X side of the cube, got {position:?}"
            );
        }
    }

    /// The regression this replaces the inverted-hull shell for
    /// (`viewer-outline-swallows-thin-hollow-prim`): the hull scaled the whole
    /// face mesh about the entity origin, so a 3.5 % inflate moved a 95 % hollow
    /// box's inner wall **1.75 cm** — past the 1.25 cm wall it sat behind, in
    /// front of the outer surface, filling the prim white.
    ///
    /// A ribbon cannot do that at any width, because it never displaces a surface
    /// sideways: every vertex it emits is one of the source vertices moved along
    /// that vertex's own normal. Asserted with a ribbon **wider than the cube**,
    /// where a hull's failure would be gross.
    #[test]
    fn a_ribbon_only_ever_moves_along_the_normal() {
        let cube = cube();
        let ribbon = silhouette_mesh(&cube, Vec3::ONE, Vec3::new(0.0, 0.0, 10.0), 2.0)
            .expect("a triangle list with normals yields a ribbon");
        let source = positions(&cube);
        for position in positions(&ribbon) {
            let point = Vec3::from_array(position);
            let sideways = source
                .iter()
                .map(|corner| {
                    let corner = Vec3::from_array(*corner);
                    let normal = corner.normalize();
                    // The part of the displacement that is not along the normal.
                    let step = point.reject_from_normalized(normal);
                    step.distance(corner.reject_from_normalized(normal))
                })
                .fold(f32::INFINITY, f32::min);
            assert!(
                sideways < 1e-5,
                "a ribbon vertex is a source vertex pushed along its own normal, \
                 but {position:?} is {sideways} off every one of them"
            );
        }
    }

    /// The width is a **world** distance: under an entity scale, the local offset
    /// is divided by how far that scale carries a step along the normal, so the
    /// ribbon draws the same width whatever the object's Second Life size. The
    /// quad's normals lie along the axis stretched fourfold, so its outer edge
    /// stands a quarter of the width out in the mesh's own units.
    #[test]
    fn the_width_is_a_world_distance() {
        let scale = Vec3::new(1.0, 1.0, 4.0);
        let ribbon = silhouette_mesh(&quad(), scale, Vec3::new(0.0, 0.0, 10.0), THICKNESS)
            .expect("a triangle list with normals yields a ribbon");
        let heights: Vec<f32> = positions(&ribbon)
            .iter()
            .map(|position| {
                position
                    .get(2)
                    .copied()
                    .expect("a three-component position")
            })
            .collect();
        let inner = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let outer = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            ((outer - inner) - THICKNESS / 4.0).abs() < 1e-6,
            "a fourfold `z` divides the local width by four: expected {}, got {}",
            THICKNESS / 4.0,
            outer - inner
        );
        assert!(
            inner > 0.0,
            "and the inner edge is lifted off the surface rather than z-fighting it"
        );
    }

    /// The fade is carried in the vertex colours the face shader multiplies into
    /// its base colour: the reference's doubled colour at the surface and zero
    /// alpha at the outer edge, which is what makes the outline read as a glow
    /// rather than a band.
    #[test]
    fn the_outer_edge_fades_out() {
        let ribbon = silhouette_mesh(&quad(), Vec3::ONE, Vec3::new(0.0, 0.0, 10.0), THICKNESS)
            .expect("a triangle list with normals yields a ribbon");
        let Some(VertexAttributeValues::Float32x4(colors)) =
            ribbon.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("the ribbon carries vertex colours");
        };
        let alphas: Vec<f32> = colors
            .iter()
            .map(|color| color.get(3).copied().expect("a four-component colour"))
            .collect();
        assert!(
            alphas.iter().any(|alpha| *alpha > 0.0) && alphas.iter().any(|alpha| *alpha <= 0.0),
            "the ribbon runs from opaque at the surface to clear at its outer edge"
        );
    }

    /// Anything the walk cannot read leaves the face unhighlighted rather than
    /// guessing at its geometry: a non-triangle topology, an unindexed mesh, and —
    /// unlike the wireframe, which only needs edges — a mesh with no normals to
    /// push the ribbon out along.
    #[test]
    fn unusable_input_is_refused() {
        let view = Vec3::new(0.0, 0.0, 10.0);

        let mut lines = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default());
        lines.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32, 0.0, 0.0]; 2]);
        lines.insert_indices(Indices::U32(vec![0, 1]));
        assert!(silhouette_mesh(&lines, Vec3::ONE, view, THICKNESS).is_none());

        let mut unindexed = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        unindexed.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32, 0.0, 0.0]; 3]);
        assert!(silhouette_mesh(&unindexed, Vec3::ONE, view, THICKNESS).is_none());

        let mut unlit = quad();
        unlit.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
        assert!(silhouette_mesh(&unlit, Vec3::ONE, view, THICKNESS).is_none());
    }
}
