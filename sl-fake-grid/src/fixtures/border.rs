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
use sl_types::key::{AgentKey, Key, ObjectKey, OwnerKey, ParcelKey, TextureKey};
use sl_types::lsl::Vector;

use super::RegionFixture;
use super::npcs::{NpcAppearance, NpcFixture};
use super::prims::PrimFixture;
use crate::world::{AvatarIdentity, SIT_TARGET_OFFSET, SceneFixtures, region_wide_parcel};

/// The border parcel's name.
pub const BORDER_PARCEL_NAME: &str = "Fake Grid Border";

/// The border parcel's region-local id.
pub const BORDER_PARCEL_LOCAL_ID: RegionLocalParcelId = RegionLocalParcelId(1);

/// The agent every border object is owned by (a fixture owner, never a login).
const BORDER_OWNER: u128 = 0x000B_04DE_0000;

/// The grid-wide id of the border scene's parcel: what a `RemoteParcelRequest`
/// resolves a location in it to, and what its dwell and search listing are
/// keyed on.
pub const BORDER_PARCEL_ID: ParcelKey = ParcelKey(Key(uuid::Uuid::from_u128(0x000B_04DE_0003)));

/// The dwell the border parcel reports: none, as a region nobody has visited.
const BORDER_DWELL: f32 = 0.0;

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

/// The **vehicle**: a rideable platform a ridden border crossing hands to the
/// region next door.
///
/// It keeps its grid-wide [`ObjectKey`] across the border and takes the
/// destination's own region-local id, which is the renumbering a rider's seat
/// has to survive — see [`BorderSide`].
pub const VEHICLE_OBJECT: ObjectKey = ObjectKey(Key(uuid::Uuid::from_u128(0x000B_04DE_0010)));

/// How wide and deep the vehicle is, in metres.
pub const VEHICLE_SIZE: f32 = 2.0;

/// How thick the vehicle's deck is, in metres.
pub const VEHICLE_HEIGHT: f32 = 0.4;

/// The rider NPC's agent id.
pub const RIDER_AGENT: uuid::Uuid = uuid::Uuid::from_u128(0x000B_04DE_0011);

/// The colour the rider NPC is baked, so a picture can tell it from the
/// arriving agent's own green body.
pub const RIDER_BAKE_COLOR: [u8; 4] = sl_test_assets::markers::BLUE;

/// How far a vehicle stands from the border it is about to cross, or has just
/// crossed, in metres.
///
/// A few metres, because a crossing is a few metres: the two regions' copies of
/// one vehicle are this far either side of the same line, which is what a
/// handover looks like. (An earlier version of this scene put both copies at
/// the same *region-local* x, which put them 256 m apart and made the
/// "handover" a sideways jump across a whole region.)
pub const VEHICLE_FROM_BORDER: f32 = 4.0;

/// Which side of the shared border a region is on, for a west-to-east
/// crossing.
///
/// The two regions of a border grid are not interchangeable, and pairing the
/// wrong local id with the wrong position is exactly the mistake this type
/// exists to make impossible: a vehicle waiting to leave stands inside its
/// region's **east** edge, and the copy it becomes stands inside the next
/// region's **west** edge, a few metres away across the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "the module is the scene and the type is a side of it; `border::BorderSide` \
              reads as it should at every call site"
)]
pub enum BorderSide {
    /// The region the crossing starts in: its vehicle waits just inside the
    /// **east** edge, against the border.
    Leaving,
    /// The region the crossing ends in: its vehicle stands just inside the
    /// **west** edge, the other side of that same border.
    Arriving,
}

impl BorderSide {
    /// The vehicle's region-local id on this side. Deliberately different
    /// either side, because a handover renumbers.
    #[must_use]
    pub const fn vehicle_local_id(self) -> RegionLocalObjectId {
        match self {
            Self::Leaving => RegionLocalObjectId(0x310),
            Self::Arriving => RegionLocalObjectId(0x340),
        }
    }

    /// The scripted rider's region-local id on this side, renumbered with its
    /// vehicle.
    #[must_use]
    pub const fn rider_local_id(self) -> RegionLocalObjectId {
        match self {
            Self::Leaving => RegionLocalObjectId(0x311),
            Self::Arriving => RegionLocalObjectId(0x341),
        }
    }

    /// The texture this side's ground is painted with — its own id, so the two
    /// regions' grounds are two different textures rather than one shared one.
    #[must_use]
    pub const fn ground_texture(self) -> TextureKey {
        match self {
            Self::Leaving => TextureKey(Key(uuid::Uuid::from_u128(0x000B_04DE_0020))),
            Self::Arriving => TextureKey(Key(uuid::Uuid::from_u128(0x000B_04DE_0021))),
        }
    }

    /// The marker colour this side's ground is painted, so a framing holding
    /// both regions' terrain can say which half of it is which.
    ///
    /// Blue and yellow, not the pillar's red and green: a capture that holds
    /// the ground either side of a border may hold the pillar too, and four
    /// classes that never collide is what makes both readable at once.
    #[must_use]
    pub const fn ground_color(self) -> [u8; 4] {
        match self {
            Self::Leaving => sl_test_assets::markers::BLUE,
            Self::Arriving => sl_test_assets::markers::YELLOW,
        }
    }

    /// Where the vehicle stands in this region, in region metres: hard against
    /// the shared border, on this side of it.
    ///
    /// The two are [`VEHICLE_FROM_BORDER`] × 2 apart in world space — the
    /// short hop a crossing actually is.
    #[must_use]
    pub const fn vehicle_position(self) -> Vector {
        let x = match self {
            Self::Leaving => REGION_SIZE_M - VEHICLE_FROM_BORDER,
            Self::Arriving => VEHICLE_FROM_BORDER,
        };
        Vector {
            x,
            y: MARKER_Y - 4.0,
            z: MARKER_Z,
        }
    }
}

/// A region's width in metres, for placing something against its far edge.
const REGION_SIZE_M: f32 = 256.0;

/// The vehicle as a prim, standing on `side` of the border.
#[must_use]
pub fn vehicle(side: BorderSide) -> PrimFixture {
    PrimFixture::boxed(
        side.vehicle_local_id(),
        VEHICLE_OBJECT,
        AgentKey::from(uuid::Uuid::from_u128(BORDER_OWNER)),
        side.vehicle_position(),
        Vector {
            x: VEHICLE_SIZE,
            y: VEHICLE_SIZE,
            z: VEHICLE_HEIGHT,
        },
    )
    .textured(MARKER_TEXTURE)
}

/// A scripted **other rider** on the vehicle, so a test can tell "my seat
/// retargeted" from "every seat retargeted".
#[must_use]
pub fn rider(side: BorderSide) -> NpcFixture {
    let agent = AgentKey::from(RIDER_AGENT);
    NpcFixture::new(
        side.rider_local_id(),
        AvatarIdentity::new(agent, "Border", "Rider"),
        // Parent-relative, like every seated avatar on the wire.
        SIT_TARGET_OFFSET,
    )
    .seated_on(side.vehicle_local_id())
    .looking(NpcAppearance::solid(agent, RIDER_BAKE_COLOR))
}

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
///
/// Carries no vehicle. A ridden crossing needs the two regions to number the
/// same object differently, which a single fixture cannot express — use
/// [`border_with_vehicle`] and give each region its own ids.
#[must_use]
pub fn border() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(BORDER_OWNER));
    let mut world = SceneFixtures::new();
    world.add_parcel(
        region_wide_parcel(
            BORDER_PARCEL_LOCAL_ID,
            OwnerKey::Agent(owner),
            BORDER_PARCEL_NAME,
        ),
        BORDER_PARCEL_ID,
        BORDER_DWELL,
    );
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

/// [`border`] plus the rideable vehicle, placed and numbered for one side of
/// the shared border.
///
/// Give one region [`BorderSide::Leaving`] and the next [`BorderSide::Arriving`]
/// and the pair is a crossing: one object, the same full id, a few metres and
/// one renumbering apart. `ridden` decides whether the scripted rider is
/// aboard, so the same scene serves both the alone case and the with-others
/// case.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported nowhere; `border::border_with_vehicle` is how a caller reads it"
)]
pub fn border_with_vehicle(side: BorderSide, ridden: bool) -> RegionFixture {
    let mut fixture = border();
    fixture.world.objects.push(vehicle(side).build());
    if ridden {
        fixture.world.npcs.push(rider(side));
    }
    fixture
}

/// [`border`] with its **ground painted** this side's [`ground_color`], so a
/// framing that holds the terrain either side of the shared border can say
/// which half of the picture belongs to which region.
///
/// All four detail slots carry the one solid, so the ground is that colour at
/// every altitude — the height-blend the viewer shades a real region's ground
/// with would otherwise make "what colour is the ground" a question about
/// where on it you looked.
///
/// [`ground_color`]: BorderSide::ground_color
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported nowhere; `border::border_on_painted_ground` is how a caller reads it"
)]
pub fn border_on_painted_ground(side: BorderSide) -> RegionFixture {
    let mut fixture = border();
    fixture.terrain.composition.detail_textures = [side.ground_texture().uuid(); 4];
    fixture
}

/// The side, in pixels, of a painted-ground solid. A solid carries no detail,
/// so a small tile is all the ground needs.
const GROUND_TEXTURE_SIZE: u32 = 128;

/// The marker's checker, plus the two painted-ground solids
/// [`border_on_painted_ground`] shades a region with. An encode failure is
/// logged rather than fatal, as everywhere else in the fixtures: the pillar
/// then renders untextured, which is a visible failure and not a panic.
///
/// Both grounds are registered whichever side this fixture is built for: the
/// grid's asset store is grid-wide, and a viewer standing in one region
/// fetches its neighbour's ground texture over the region it is *rooted* in.
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
    for side in [BorderSide::Leaving, BorderSide::Arriving] {
        let ground =
            sl_test_assets::RgbaImage::solid(GROUND_TEXTURE_SIZE, side.ground_color()).j2c();
        match ground {
            Ok(bytes) => {
                let _previous = assets.insert(AssetKey::from(side.ground_texture().uuid()), bytes);
            }
            Err(error) => {
                tracing::warn!("encoding the {side:?} side's painted ground failed: {error}");
            }
        }
    }
    assets
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};

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

    /// **The two copies of the vehicle stand either side of one line.**
    ///
    /// The property the whole ridden-crossing scene rests on, and the one an
    /// earlier version of this fixture got wrong: both sides used the same
    /// region-local position, which put the copies 256 m apart in world space
    /// and turned a border crossing into a jump across a whole region.
    ///
    /// Stated in world metres for a west-to-east pair, where the eastern
    /// region's own origin is one region width further on.
    #[test]
    fn the_vehicle_sits_against_the_border_on_both_sides() {
        let leaving = BorderSide::Leaving.vehicle_position();
        let arriving = BorderSide::Arriving.vehicle_position();
        // The border is the leaving region's east edge, which is also the
        // arriving region's origin.
        let gap = (arriving.x + REGION_SIZE_M) - leaving.x;
        assert!(
            gap > 0.0 && gap <= VEHICLE_FROM_BORDER * 4.0,
            "the two copies are {gap} m apart across the border, which is not a crossing"
        );
        assert!(
            leaving.x > REGION_SIZE_M / 2.0,
            "the leaving side's vehicle must be against its *east* edge"
        );
        assert!(
            arriving.x < REGION_SIZE_M / 2.0,
            "the arriving side's vehicle must be against its *west* edge"
        );
    }

    /// The two painted grounds are two textures in two marker classes, and
    /// both are served whichever side the fixture was built for — the grid's
    /// asset store is grid-wide, so a viewer looking across the border fetches
    /// the neighbour's ground over its own region's `GetTexture`.
    #[test]
    fn the_painted_grounds_are_two_distinguishable_textures() {
        let (west, east) = (BorderSide::Leaving, BorderSide::Arriving);
        assert_ne!(west.ground_texture(), east.ground_texture());
        assert_ne!(west.ground_color(), east.ground_color());
        // Neither is the pillar's red or green, so a capture holding the pillar
        // and both grounds classifies all three.
        for side in [west, east] {
            assert_ne!(side.ground_color(), sl_test_assets::markers::RED);
            assert_ne!(side.ground_color(), sl_test_assets::markers::GREEN);
        }
        let fixture = border_on_painted_ground(west);
        assert_eq!(
            fixture.terrain.composition.detail_textures,
            [west.ground_texture().uuid(); 4],
            "every altitude band has to be the one colour or the ground is a gradient"
        );
        for side in [west, east] {
            assert!(
                fixture
                    .assets
                    .contains(AssetKey::from(side.ground_texture().uuid())),
                "the {side:?} side's ground texture is not served"
            );
        }
    }

    /// A handover renumbers: the same object is not the same local id either
    /// side, or a viewer that keyed a seat by local id alone would never be
    /// tested.
    #[test]
    fn the_two_sides_number_the_vehicle_differently() {
        assert_ne!(
            BorderSide::Leaving.vehicle_local_id(),
            BorderSide::Arriving.vehicle_local_id()
        );
        assert_ne!(
            BorderSide::Leaving.rider_local_id(),
            BorderSide::Arriving.rider_local_id()
        );
        assert_eq!(
            vehicle(BorderSide::Leaving).build().full_id,
            vehicle(BorderSide::Arriving).build().full_id,
            "and it is the same object, or nothing was handed over"
        );
    }
}
