//! The far-floor scene: one long, flat, finely checkered slab to look along.
//!
//! It exists for texture **minification** — the far half of a floor seen at a
//! grazing angle, where each pixel covers many texels. A viewer that samples
//! only a texture's full-resolution level shows that half as shimmering noise
//! (and, from a still camera, as moiré); one that samples a mip chain shows it
//! settling to the checker's average grey. Every other scene is framed from a
//! few metres away, square on to its subject, where the two look the same.
//!
//! The slab is **fullbright** and black-and-white, so a frame pair taken along
//! it differs only in how the texture was sampled: no lighting, sky tint or
//! colour space to explain a difference away.

use sl_proto::{AssetKey, InMemoryAssetSource, RegionLocalObjectId, RegionLocalParcelId};
use sl_types::key::{AgentKey, Key, ObjectKey, OwnerKey, ParcelKey, TextureKey};
use sl_types::lsl::Vector;

use super::RegionFixture;
use super::prims::{FaceStyle, PrimFixture};
use crate::world::{SceneFixtures, region_wide_parcel};

/// The far-floor parcel's name.
pub const FAR_FLOOR_PARCEL_NAME: &str = "Fake Grid Far Floor";

/// The far-floor parcel's region-local id.
pub const FAR_FLOOR_PARCEL_LOCAL_ID: RegionLocalParcelId = RegionLocalParcelId(1);

/// The grid-wide id of the far-floor scene's parcel.
pub const FAR_FLOOR_PARCEL_ID: ParcelKey = ParcelKey(Key(uuid::Uuid::from_u128(0x000F_100F_0003)));

/// The agent the slab is owned by (a fixture owner, never a login).
const FAR_FLOOR_OWNER: u128 = 0x000F_100F_0000;

/// The dwell the parcel reports: none.
const FAR_FLOOR_DWELL: f32 = 0.0;

/// The slab's region-local id.
pub const SLAB_LOCAL_ID: RegionLocalObjectId = RegionLocalObjectId(0x400);

/// The slab's full (asset-space) id.
pub const SLAB_OBJECT: ObjectKey = ObjectKey(Key(uuid::Uuid::from_u128(0x000F_100F_0001)));

/// The checker the slab's top face wears.
pub const SLAB_TEXTURE: TextureKey = TextureKey(Key(uuid::Uuid::from_u128(0x000F_100F_0002)));

/// The slab's side, in metres: the largest a prim may be, so its far edge is as
/// far from a camera at its near edge as one object allows.
pub const SLAB_SIZE: f32 = 64.0;

/// The slab's thickness, in metres.
pub const SLAB_THICKNESS: f32 = 0.5;

/// The height of the slab's top face above the stock ground, in metres — clear
/// of it, so no terrain pokes through and the avatar standing at the region's
/// centre is hidden underneath.
pub const SLAB_CLEARANCE: f32 = 2.0;

/// How many times the checker repeats along each side of the top face: two
/// metres a repeat.
pub const SLAB_REPEATS: f32 = 32.0;

/// The side, in pixels, of the checker texture.
const TEXTURE_SIZE: u32 = 512;

/// The side, in pixels, of one checker cell: eight cells a repeat, so a cell
/// is a quarter of a metre on the slab — fine enough that its far half is
/// many texels to a pixel from a camera standing at the near edge.
const TEXTURE_CELL: u32 = 64;

/// The checker's two colours: black and white, whose average is the grey a
/// mip chain settles to.
const CHECKER_COLORS: [[u8; 4]; 2] = [[0, 0, 0, 255], [255, 255, 255, 255]];

/// The height of the slab's top face, in region metres.
#[must_use]
pub fn slab_top() -> f32 {
    f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M) + SLAB_CLEARANCE
}

/// The slab's centre, in region metres: over the middle of the region.
#[must_use]
pub fn slab_position() -> Vector {
    Vector {
        x: 128.0,
        y: 128.0,
        z: slab_top() - SLAB_THICKNESS / 2.0,
    }
}

/// The middle of the slab's **south** edge, on its top face: where a camera
/// stands to look north along it.
#[must_use]
pub fn near_edge() -> Vector {
    Vector {
        y: slab_position().y - SLAB_SIZE / 2.0,
        z: slab_top(),
        ..slab_position()
    }
}

/// The middle of the slab's **north** edge, on its top face: what that camera
/// looks at.
#[must_use]
pub fn far_edge() -> Vector {
    Vector {
        y: slab_position().y + SLAB_SIZE / 2.0,
        z: slab_top(),
        ..slab_position()
    }
}

/// The far-floor scene as a [`RegionFixture`]: one region-wide parcel, the
/// slab, and its checker.
#[must_use]
pub fn far_floor() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(FAR_FLOOR_OWNER));
    let mut world = SceneFixtures::new();
    world.add_parcel(
        region_wide_parcel(
            FAR_FLOOR_PARCEL_LOCAL_ID,
            OwnerKey::Agent(owner),
            FAR_FLOOR_PARCEL_NAME,
        ),
        FAR_FLOOR_PARCEL_ID,
        FAR_FLOOR_DWELL,
    );
    let checker = FaceStyle {
        texture: Some(SLAB_TEXTURE),
        fullbright: true,
        repeats: [SLAB_REPEATS, SLAB_REPEATS],
        ..FaceStyle::default()
    };
    world.objects.push(
        PrimFixture::boxed(
            SLAB_LOCAL_ID,
            SLAB_OBJECT,
            owner,
            slab_position(),
            Vector {
                x: SLAB_SIZE,
                y: SLAB_SIZE,
                z: SLAB_THICKNESS,
            },
        )
        .faces(&checker)
        .build(),
    );
    RegionFixture {
        world,
        assets: slab_assets(),
        ..RegionFixture::new()
    }
}

/// The slab's checker. An encode failure is logged rather than fatal, as
/// everywhere else in the fixtures: the slab then renders untextured, which is
/// a visible failure and not a panic.
fn slab_assets() -> InMemoryAssetSource {
    let mut assets = crate::scenario::default_assets();
    let [black, white] = CHECKER_COLORS;
    let checker = sl_test_assets::RgbaImage::checker(TEXTURE_SIZE, TEXTURE_CELL, black, white);
    match checker.j2c() {
        Ok(bytes) => {
            let _previous = assets.insert(AssetKey::from(SLAB_TEXTURE.uuid()), bytes);
        }
        Err(error) => tracing::warn!("encoding the far floor's checker failed: {error}"),
    }
    assets
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The slab is the scene's one object, clear of the ground, and its checker
    /// is in the scene's asset store.
    #[test]
    fn the_slab_stands_clear_of_the_ground_with_its_checker()
    -> Result<(), Box<dyn core::error::Error>> {
        let fixture = far_floor();
        assert_eq!(fixture.world.objects.len(), 1);
        let slab = fixture
            .world
            .objects
            .first()
            .ok_or("the far-floor scene rezzes its slab")?;
        assert_eq!(slab.local_id, SLAB_LOCAL_ID);
        assert!(
            slab.motion.position.z - SLAB_THICKNESS / 2.0
                > f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M),
            "the slab has to clear the ground, or terrain shows through it"
        );
        assert!(
            fixture.assets.contains(AssetKey::from(SLAB_TEXTURE.uuid())),
            "the slab's checker is not in the scene's asset store"
        );
        Ok(())
    }

    /// The two edges a camera is posed by are a whole slab apart, level with
    /// its top face.
    #[test]
    fn the_edges_are_a_slab_apart_on_its_top() {
        /// Far below a millimetre, and far above `f32` rounding at these sizes.
        const TOLERANCE: f32 = 1.0e-4;
        assert!((far_edge().y - near_edge().y - SLAB_SIZE).abs() < TOLERANCE);
        assert!((near_edge().z - slab_top()).abs() < TOLERANCE);
        assert!((far_edge().z - slab_top()).abs() < TOLERANCE);
    }
}
