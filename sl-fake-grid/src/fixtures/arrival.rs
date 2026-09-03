//! The **arrival** scene: one prim standing exactly where the catalogue's
//! checkered box stands, wearing a colour the catalogue does not have.
//!
//! This exists for one question, and it is a question no single-region fixture
//! can ask: after a teleport, is what the camera is looking at the *new*
//! region's content, or the old region's left behind? Both scenes put a prim in
//! the same region-local slot, so one framing photographs whichever of them the
//! viewer actually built — and the two prims are told apart by colour class
//! alone ([`ARRIVAL_COLOR`] against the catalogue's red-and-green checker), so
//! "the old texture is on the new prim" is a thing a capture can say.
//!
//! The prim rests on the row's ground and is a plain solid rather than a
//! checker: a leak is a *colour* arriving where it should not, and one flat
//! colour is the cleanest way to see one.

use sl_proto::{AssetKey, InMemoryAssetSource, RegionLocalObjectId, RegionLocalParcelId};
use sl_types::key::{AgentKey, Key, ObjectKey, OwnerKey, TextureKey};
use sl_types::lsl::Vector;

use super::RegionFixture;
use super::catalogue::{ROW_FIRST_X, ROW_SPACING, ROW_Y, ROW_Z};
use super::prims::PrimFixture;
use crate::world::{SceneFixtures, region_wide_parcel};

/// The arrival parcel's name.
pub const ARRIVAL_PARCEL_NAME: &str = "Fake Grid Arrival";

/// The arrival parcel's region-local id.
pub const ARRIVAL_PARCEL_LOCAL_ID: RegionLocalParcelId = RegionLocalParcelId(1);

/// The agent every arrival object is owned by (a fixture owner, never a login).
const ARRIVAL_OWNER: u128 = 0x00A2_2140_0000;

/// The arrival prim's region-local id. Deliberately clear of the catalogue's
/// row (`0x100`…) and of the border scene's ids (`0x300`…), so a grid can serve
/// both scenes and an id in a capture still names one scene's prim.
pub const ARRIVAL_LOCAL_ID: RegionLocalObjectId = RegionLocalObjectId(0x400);

/// The arrival prim's full (asset-space) id.
pub const ARRIVAL_OBJECT: ObjectKey = ObjectKey(Key(uuid::Uuid::from_u128(0x00A2_2140_0001)));

/// The solid the arrival prim wears.
pub const ARRIVAL_TEXTURE: TextureKey = TextureKey(Key(uuid::Uuid::from_u128(0x00A2_2140_0002)));

/// The colour that solid is painted: blue, which is neither of the catalogue
/// checker's two classes, so a frame carrying red or green where this prim
/// stands is carrying the region the viewer left.
pub const ARRIVAL_COLOR: [u8; 4] = sl_test_assets::markers::BLUE;

/// The side, in pixels, of the arrival solid. A solid carries no detail, so a
/// small tile is enough — unlike the catalogue's checker, whose cells have to
/// survive the viewer's own level-of-detail choice.
const TEXTURE_SIZE: u32 = 128;

/// Where the arrival prim stands, in region metres: the catalogue's
/// `checker-box` slot, so one camera framing photographs either scene.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "the module is the scene and this is where it stands; \
              `arrival::arrival_position()` reads as it should at every call site"
)]
pub const fn arrival_position() -> Vector {
    Vector {
        x: ROW_FIRST_X + ROW_SPACING,
        y: ROW_Y,
        z: ROW_Z,
    }
}

/// The arrival scene as a [`RegionFixture`]: one region-wide parcel, one
/// blue-solid box in the catalogue's checker slot, and the solid it wears.
#[must_use]
pub fn arrival() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(ARRIVAL_OWNER));
    let mut world = SceneFixtures::new();
    world.parcels.push(region_wide_parcel(
        ARRIVAL_PARCEL_LOCAL_ID,
        OwnerKey::Agent(owner),
        ARRIVAL_PARCEL_NAME,
    ));
    world.objects.push(
        PrimFixture::boxed(
            ARRIVAL_LOCAL_ID,
            ARRIVAL_OBJECT,
            owner,
            arrival_position(),
            Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        )
        .textured(ARRIVAL_TEXTURE)
        .build(),
    );
    RegionFixture {
        world,
        assets: arrival_assets(),
        ..RegionFixture::new()
    }
}

/// The arrival prim's solid. An encode failure is logged rather than fatal, as
/// everywhere else in the fixtures: the prim then renders untextured, which is
/// a visible failure and not a panic.
fn arrival_assets() -> InMemoryAssetSource {
    let mut assets = crate::scenario::default_assets();
    match sl_test_assets::RgbaImage::solid(TEXTURE_SIZE, ARRIVAL_COLOR).j2c() {
        Ok(bytes) => {
            let _previous = assets.insert(AssetKey::from(ARRIVAL_TEXTURE.uuid()), bytes);
        }
        Err(error) => tracing::warn!("encoding the arrival solid failed: {error}"),
    }
    assets
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    /// The arrival prim stands in the catalogue's checker slot and wears a
    /// colour the catalogue does not — the two facts the leak check is made of.
    /// Neither is arithmetic a reader can check by eye, so both are pinned here.
    #[test]
    fn the_arrival_prim_stands_in_the_checkers_slot_in_another_colour() {
        let checker = super::super::catalogue::entry("checker-box").map_or(
            Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            |entry| entry.position(),
        );
        assert_eq!(arrival_position(), checker);
        assert_ne!(ARRIVAL_COLOR, sl_test_assets::markers::RED);
        assert_ne!(ARRIVAL_COLOR, sl_test_assets::markers::GREEN);
        assert_ne!(ARRIVAL_TEXTURE, super::super::catalogue::CHECKER_TEXTURE);
    }

    /// The scene rezzes exactly its one prim, wearing the solid it serves.
    #[test]
    fn the_arrival_scene_serves_the_solid_its_prim_wears() {
        let fixture = arrival();
        assert_eq!(fixture.world.objects.len(), 1);
        assert_eq!(
            fixture.world.objects.first().map(|object| object.local_id),
            Some(ARRIVAL_LOCAL_ID)
        );
        assert!(
            fixture
                .assets
                .contains(AssetKey::from(ARRIVAL_TEXTURE.uuid()))
        );
    }
}
