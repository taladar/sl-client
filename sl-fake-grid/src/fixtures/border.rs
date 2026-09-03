//! The border scene: one marker pillar just inside the region's west edge, so
//! a camera standing in the region to the **west** is looking at something that
//! belongs to the region to the **east**.
//!
//! Every other fixture answers a question about one region. This one exists for
//! the questions that need two: is the neighbour across the border drawn at all
//! ([`crate::neighbours`]), and does it stay in exactly the same place on screen
//! when the avatar walks over the border into it ([`crate::crossing`])? Both are
//! decidable in pixels only if there is a subject whose position is stated
//! relative to the border rather than to the middle of a region — which is the
//! whole of what this scene is.
//!
//! The pillar **floats**, five metres clear of the ground, for the same reason
//! the catalogue's row stands on it: a camera framing it from just over the
//! border has nothing behind it but sky, so its own disc is the pillar and not
//! the neighbouring terrain.

use sl_proto::{AssetKey, InMemoryAssetSource, RegionLocalObjectId, RegionLocalParcelId};
use sl_types::key::{AgentKey, Key, ObjectKey, OwnerKey, TextureKey};
use sl_types::lsl::Vector;

use super::RegionFixture;
use super::prims::PrimFixture;
use crate::world::{SceneFixtures, region_wide_parcel};

/// The border parcel's name.
pub const BORDER_PARCEL_NAME: &str = "Fake Grid Border";

/// The border parcel's region-local id.
pub const BORDER_PARCEL_LOCAL_ID: RegionLocalParcelId = RegionLocalParcelId(1);

/// The agent every border object is owned by (a fixture owner, never a login).
const BORDER_OWNER: u128 = 0x000B_04DE_0000;

/// The marker pillar's region-local id.
pub const MARKER_LOCAL_ID: RegionLocalObjectId = RegionLocalObjectId(0x300);

/// The marker pillar's full (asset-space) id.
pub const MARKER_OBJECT: ObjectKey = ObjectKey(Key(uuid::Uuid::from_u128(0x000B_04DE_0001)));

/// The checker the marker pillar wears — red and green marker cells, so a
/// pixel test can say "the pillar is on screen" without naming a shade.
pub const MARKER_TEXTURE: TextureKey = TextureKey(Key(uuid::Uuid::from_u128(0x000B_04DE_0002)));

/// How far east of the region's west edge the marker pillar stands, in metres.
///
/// Close enough that a camera a dozen metres the other side of the border sees
/// it at a useful size, far enough in that it is unambiguously *this* region's
/// object and not a rounding error away from the neighbour's.
pub const MARKER_X: f32 = 4.0;

/// The marker pillar's `y`: the middle of the border, so the framing is the
/// same whichever region is to the west.
pub const MARKER_Y: f32 = 128.0;

/// The marker pillar's `z` — its centre, five metres clear of the stock ground,
/// so a camera framing it sees sky behind it rather than the neighbour's
/// terrain.
pub const MARKER_Z: f32 = 31.0;

/// How big the marker pillar is along each axis, in metres.
pub const MARKER_SIZE: f32 = 3.0;

/// The side, in pixels, of the marker's checker texture, and the size of one
/// of its cells. The same honest size the catalogue uses: a 64² texture reads
/// as a stuck low-LOD blur on a prim this close to a camera.
const TEXTURE_SIZE: u32 = 512;

/// Where the marker pillar stands, in region metres.
#[must_use]
pub const fn marker_position() -> Vector {
    Vector {
        x: MARKER_X,
        y: MARKER_Y,
        z: MARKER_Z,
    }
}

/// The border scene as a [`RegionFixture`]: one region-wide parcel, one
/// checkered marker pillar inside the west edge, and the checker it wears.
#[must_use]
pub fn border() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(BORDER_OWNER));
    let mut world = SceneFixtures::new();
    world.parcels.push(region_wide_parcel(
        BORDER_PARCEL_LOCAL_ID,
        OwnerKey::Agent(owner),
        BORDER_PARCEL_NAME,
    ));
    world.objects.push(
        PrimFixture::boxed(
            MARKER_LOCAL_ID,
            MARKER_OBJECT,
            owner,
            marker_position(),
            Vector {
                x: MARKER_SIZE,
                y: MARKER_SIZE,
                z: MARKER_SIZE,
            },
        )
        .textured(MARKER_TEXTURE)
        .build(),
    );
    RegionFixture {
        world,
        assets: marker_assets(),
        ..RegionFixture::new()
    }
}

/// The marker's checker. An encode failure is logged rather than fatal, as
/// everywhere else in the fixtures: the pillar then renders untextured, which
/// is a visible failure and not a panic.
fn marker_assets() -> InMemoryAssetSource {
    let mut assets = crate::scenario::default_assets();
    let checker = sl_test_assets::RgbaImage::checker(
        TEXTURE_SIZE,
        TEXTURE_SIZE / 4,
        sl_test_assets::markers::RED,
        sl_test_assets::markers::GREEN,
    );
    match checker.j2c() {
        Ok(bytes) => {
            let _previous = assets.insert(AssetKey::from(MARKER_TEXTURE.uuid()), bytes);
        }
        Err(error) => tracing::warn!("encoding the border marker's checker failed: {error}"),
    }
    assets
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The pillar stands inside the west edge and clear of the ground — the two
    /// facts every framing in the crossing tests is built on.
    #[test]
    fn the_marker_stands_inside_the_west_edge_and_off_the_ground()
    -> Result<(), Box<dyn core::error::Error>> {
        let fixture = border();
        let marker = fixture
            .world
            .objects
            .first()
            .ok_or("the border scene rezzes its marker")?;
        assert_eq!(marker.local_id, MARKER_LOCAL_ID);
        assert!(
            marker.motion.position.x < MARKER_SIZE * 2.0,
            "the marker has to be near the west edge for a camera over the border to see it"
        );
        assert!(
            marker.motion.position.z - MARKER_SIZE / 2.0
                > f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M),
            "the marker has to clear the ground for a framing of it to be against the sky"
        );
        Ok(())
    }
}
