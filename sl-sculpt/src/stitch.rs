//! The sculpt **stitch type** and its mirror / invert flags, parsed from the
//! wire `sculpt_type` byte (`LLSculptParams`).
//!
//! The low three bits of the byte name the stitch topology
//! (`LL_SCULPT_TYPE_MASK`); the two high bits are the invert
//! (`LL_SCULPT_FLAG_INVERT`) and mirror (`LL_SCULPT_FLAG_MIRROR`) flags. A
//! byte whose stitch bits are not one of the four sculpt topologies (i.e. the
//! `NONE` / `MESH` / `GLTF` values, which are not sculpt-texture shapes) falls
//! back to [`SculptStitch::Plane`]; a sculpt object always carries a real
//! stitch type, so this only guards against a malformed byte.

/// The bit mask selecting the stitch topology from the `sculpt_type` byte
/// (Firestorm `LL_SCULPT_TYPE_MASK`).
const LL_SCULPT_TYPE_MASK: u8 = 0x07;

/// The stitch value meaning a sphere (Firestorm `LL_SCULPT_TYPE_SPHERE`).
const LL_SCULPT_TYPE_SPHERE: u8 = 1;

/// The stitch value meaning a torus (Firestorm `LL_SCULPT_TYPE_TORUS`).
const LL_SCULPT_TYPE_TORUS: u8 = 2;

/// The stitch value meaning a plane (Firestorm `LL_SCULPT_TYPE_PLANE`).
const LL_SCULPT_TYPE_PLANE: u8 = 3;

/// The stitch value meaning a cylinder (Firestorm `LL_SCULPT_TYPE_CYLINDER`).
const LL_SCULPT_TYPE_CYLINDER: u8 = 4;

/// The invert flag bit: the sculpt surface is turned inside out (Firestorm
/// `LL_SCULPT_FLAG_INVERT`).
const LL_SCULPT_FLAG_INVERT: u8 = 64;

/// The mirror flag bit: the sculpt geometry is mirrored across its X axis
/// (Firestorm `LL_SCULPT_FLAG_MIRROR`).
const LL_SCULPT_FLAG_MIRROR: u8 = 128;

/// How a sculpt map's displacement grid is stitched into a closed surface: which
/// edges wrap and which rows collapse to a pole.
///
/// This is the geometric meaning of the low bits of the `sculpt_type` byte,
/// mirroring Firestorm's `LL_SCULPT_TYPE_*` (minus the non-sculpt `NONE` /
/// `MESH` / `GLTF` values, which never reach a sculpt tessellation).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `SculptStitch` reads clearly"
)]
pub enum SculptStitch {
    /// An open grid: neither edge wraps (`LL_SCULPT_TYPE_PLANE`). The default
    /// fallback for an unrecognised stitch value.
    #[default]
    Plane,
    /// The U (around) edge wraps into a tube (`LL_SCULPT_TYPE_CYLINDER`).
    Cylinder,
    /// The U edge wraps and the top / bottom rows collapse to poles
    /// (`LL_SCULPT_TYPE_SPHERE`).
    Sphere,
    /// Both the U and V edges wrap into a ring-of-rings
    /// (`LL_SCULPT_TYPE_TORUS`).
    Torus,
}

impl SculptStitch {
    /// The stitch topology named by the low three bits of a `sculpt_type` byte;
    /// an unrecognised value (`NONE` / `MESH` / `GLTF`) falls back to
    /// [`SculptStitch::Plane`].
    #[must_use]
    pub const fn from_sculpt_type(sculpt_type: u8) -> Self {
        match sculpt_type & LL_SCULPT_TYPE_MASK {
            LL_SCULPT_TYPE_SPHERE => Self::Sphere,
            LL_SCULPT_TYPE_TORUS => Self::Torus,
            LL_SCULPT_TYPE_CYLINDER => Self::Cylinder,
            LL_SCULPT_TYPE_PLANE => Self::Plane,
            _non_sculpt => Self::Plane,
        }
    }

    /// Whether the U (around) edge wraps: true for every stitch except a plane.
    #[must_use]
    pub const fn wraps_u(self) -> bool {
        matches!(self, Self::Cylinder | Self::Sphere | Self::Torus)
    }

    /// Whether the V (along) edge wraps: true only for a torus.
    #[must_use]
    pub const fn wraps_v(self) -> bool {
        matches!(self, Self::Torus)
    }

    /// Whether the top and bottom rows collapse to a single pole vertex each:
    /// true only for a sphere.
    #[must_use]
    pub const fn has_poles(self) -> bool {
        matches!(self, Self::Sphere)
    }
}

/// The fully parsed sculpt parameters: the [`SculptStitch`] topology plus the
/// invert / mirror flags that reflect the surface.
///
/// Following Firestorm's `sculptGenerateMapVertices`, the flags reshape the
/// sampled geometry rather than post-processing it:
///
/// - `reverse_u` (invert **XOR** mirror) reverses the horizontal (U) sampling
///   direction, so each grid column reads the mirror-image map column;
/// - `mirror` additionally negates the sampled position's X component.
///
/// With a fixed triangle winding these two transforms compose to the four
/// intended facings — outward (no flags), mirrored-outward (mirror),
/// inside-out (invert), and inside-out-mirrored (both) — so no separate winding
/// flip is needed; the per-vertex normals, computed from the resulting geometry,
/// follow automatically.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct SculptParams {
    /// The stitch topology.
    pub stitch: SculptStitch,
    /// The invert flag: turn the surface inside out.
    pub invert: bool,
    /// The mirror flag: mirror the geometry across its X axis.
    pub mirror: bool,
}

impl SculptParams {
    /// Parse the wire `sculpt_type` byte into a stitch topology and its flags.
    #[must_use]
    pub const fn from_sculpt_type(sculpt_type: u8) -> Self {
        Self {
            stitch: SculptStitch::from_sculpt_type(sculpt_type),
            invert: sculpt_type & LL_SCULPT_FLAG_INVERT != 0,
            mirror: sculpt_type & LL_SCULPT_FLAG_MIRROR != 0,
        }
    }

    /// Whether the horizontal (U) sampling direction is reversed: the invert and
    /// mirror flags differ (Firestorm's `reverse_horizontal = invert ? !mirror :
    /// mirror`).
    #[must_use]
    pub const fn reverse_u(self) -> bool {
        self.invert != self.mirror
    }
}

#[cfg(test)]
mod tests {
    use super::{SculptParams, SculptStitch};
    use pretty_assertions::assert_eq;

    #[test]
    fn stitch_values_map_to_topologies() {
        assert_eq!(SculptStitch::from_sculpt_type(1), SculptStitch::Sphere);
        assert_eq!(SculptStitch::from_sculpt_type(2), SculptStitch::Torus);
        assert_eq!(SculptStitch::from_sculpt_type(3), SculptStitch::Plane);
        assert_eq!(SculptStitch::from_sculpt_type(4), SculptStitch::Cylinder);
    }

    #[test]
    fn non_sculpt_stitch_values_fall_back_to_plane() {
        // NONE, MESH, GLTF, and the unused 7 are not sculpt-texture shapes.
        for value in [0_u8, 5, 6, 7] {
            assert_eq!(SculptStitch::from_sculpt_type(value), SculptStitch::Plane);
        }
    }

    #[test]
    fn wrap_and_pole_predicates_match_topology() {
        assert!(!SculptStitch::Plane.wraps_u());
        assert!(SculptStitch::Cylinder.wraps_u());
        assert!(SculptStitch::Sphere.wraps_u());
        assert!(SculptStitch::Torus.wraps_u());

        assert!(!SculptStitch::Cylinder.wraps_v());
        assert!(SculptStitch::Torus.wraps_v());

        assert!(SculptStitch::Sphere.has_poles());
        assert!(!SculptStitch::Torus.has_poles());
    }

    #[test]
    fn high_bits_decode_the_flags() {
        // Sphere stitch (1) plus both flag bits (64 | 128).
        let params = SculptParams::from_sculpt_type(1 | 64 | 128);
        assert_eq!(params.stitch, SculptStitch::Sphere);
        assert!(params.invert);
        assert!(params.mirror);
        // Invert XOR mirror is false when both are set.
        assert!(!params.reverse_u());
    }

    #[test]
    fn reverse_u_is_invert_xor_mirror() {
        let plain = SculptParams::from_sculpt_type(3);
        assert!(!plain.reverse_u());
        let invert = SculptParams::from_sculpt_type(3 | 64);
        assert!(invert.reverse_u());
        let mirror = SculptParams::from_sculpt_type(3 | 128);
        assert!(mirror.reverse_u());
    }
}
