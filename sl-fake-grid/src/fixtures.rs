//! Content fixtures: typed builders for the world a fake region shows, and
//! the named [`catalogue()`] every consumer of the fake grid loads.
//!
//! [`Scenario`] is the raw content model — a setup closure, an asset store, a
//! [`SceneFixtures`] — and says nothing about *what* the content is. A
//! [`RegionFixture`] is one region's content as a value: its objects, the
//! assets those objects reference, the legacy materials and per-face media
//! their faces name, its environment and its ground. [`into_region`] turns it
//! into the [`RegionConfig`] a grid is built from, wiring each piece to the
//! surface that serves it.
//!
//! [`into_region`]: RegionFixture::into_region

pub mod catalogue;
pub mod prims;

use std::sync::Arc;

use sl_proto::{EnvironmentSettings, InMemoryAssetSource, ObjectMediaState, SimSession};
use sl_types::key::ObjectKey;
use sl_wire::LegacyMaterial;

use crate::runtime::RegionConfig;
use crate::scenario::Scenario;
use crate::terrain::TerrainFixture;
use crate::world::SceneFixtures;

pub use catalogue::{CatalogueEntry, catalogue};
pub use prims::{DEFAULT_FACE_COUNT, FaceStyle, PrimFixture, SculptKind, blank_texture, linkset};

/// Everything one fake region shows, as a value.
///
/// Each field is served by a different surface — objects and parcels over UDP,
/// assets over `GetTexture`/`GetMesh2`/`ViewerAsset`, materials over
/// `RenderMaterials`, media over `ObjectMedia`, the environment over
/// `ExtEnvironment`, the ground as `LayerData` and the estate RAW download —
/// and [`into_region`](Self::into_region) is the one place that knows which
/// goes where. A fixture therefore describes content, not plumbing.
#[derive(Debug, Clone, Default)]
pub struct RegionFixture {
    /// The parcels and objects pushed at an arriving agent and replayed on
    /// request.
    pub world: SceneFixtures,
    /// The binary assets the objects reference: textures, sculpt maps, meshes,
    /// GLTF materials.
    pub assets: InMemoryAssetSource,
    /// The legacy (`LLMaterial`) materials the object faces name, by material
    /// id — what the `RenderMaterials` capability answers with.
    pub materials: Vec<(uuid::Uuid, LegacyMaterial)>,
    /// The per-face media (MOAP) of the objects that have any — what the
    /// `ObjectMedia` capability answers with.
    pub media: Vec<(ObjectKey, ObjectMediaState)>,
    /// The region's environment (day cycle, day length, sky altitudes), or
    /// `None` for the session's stock four-hour day.
    pub environment: Option<EnvironmentSettings>,
    /// The region's ground.
    pub terrain: TerrainFixture,
}

impl RegionFixture {
    /// An empty fixture: no parcels, no objects, no assets, the stock ground.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The fixture as a [`Scenario`]: the world and assets as they are, and a
    /// setup hook that installs the region materials, the object media and the
    /// environment on every fresh session.
    ///
    /// The ground is **not** carried here — it belongs to the region, not to
    /// its scenario — so a caller that only wants a scenario keeps whatever
    /// terrain its [`RegionConfig`] already names.
    /// [`into_region`](Self::into_region) sets both.
    #[must_use]
    pub fn into_scenario(self) -> Scenario {
        let materials = self.materials;
        let media = self.media;
        let environment = self.environment;
        Scenario {
            setup: Arc::new(move |sim: &mut SimSession, _now| {
                for (id, material) in &materials {
                    sim.set_region_material(*id, material.clone());
                }
                for (object, state) in &media {
                    sim.set_object_media(*object, state.clone());
                }
                if let Some(environment) = environment.clone() {
                    sim.set_environment(stamped_for(environment, sim.region_id()));
                }
            }),
            assets: self.assets,
            world: self.world,
            ..Scenario::empty()
        }
    }

    /// The fixture as a whole region: `base` with the fixture's ground and its
    /// [`into_scenario`](Self::into_scenario) scenario.
    ///
    /// This is what a test hands [`FakeGridBuilder::region`], and it is the
    /// only call that wires every piece of the fixture to the surface serving
    /// it.
    ///
    /// [`FakeGridBuilder::region`]: crate::FakeGridBuilder::region
    #[must_use]
    pub fn into_region(self, base: RegionConfig) -> RegionConfig {
        let terrain = self.terrain.clone();
        RegionConfig {
            terrain,
            scenario: Some(self.into_scenario()),
            ..base
        }
    }
}

/// An environment record stamped with the session's region id when it carries
/// none of its own — the same fill-in the region config's environment gets.
fn stamped_for(environment: EnvironmentSettings, region_id: uuid::Uuid) -> EnvironmentSettings {
    if environment.region_id.is_nil() {
        EnvironmentSettings {
            region_id,
            ..environment
        }
    } else {
        environment
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{AssetKey, RegionLocalObjectId};

    use super::*;

    /// The pieces a fixture carries reach the surfaces that serve them: the
    /// materials and media stores through the setup hook, the world and assets
    /// straight onto the scenario, and the ground onto the region.
    #[expect(
        clippy::float_cmp,
        reason = "the flat ground returns the exactly-representable height it \
                  was built with, so exact equality is the test"
    )]
    #[test]
    fn a_region_fixture_wires_every_piece_to_its_surface() {
        let material_id = uuid::Uuid::from_u128(0xAA);
        let object = ObjectKey::from(uuid::Uuid::from_u128(0xBB));
        let asset = AssetKey::from(uuid::Uuid::from_u128(0xCC));
        let fixture = RegionFixture {
            assets: InMemoryAssetSource::new().with_asset(asset, vec![1, 2, 3]),
            materials: vec![(material_id, catalogue::legacy_material())],
            media: vec![(
                object,
                ObjectMediaState {
                    version: "x-mv:0000000001/00000000-0000-0000-0000-000000000000".to_owned(),
                    faces: vec![None],
                },
            )],
            terrain: TerrainFixture::flat(33.0),
            ..RegionFixture::new()
        };
        let region = fixture.into_region(crate::RegionConfig::default());
        assert_eq!(region.terrain.height_at(10.0, 10.0), 33.0);
        assert!(
            region
                .scenario
                .is_some_and(|scenario| scenario.assets.contains(asset)),
            "the fixture's assets are not on the scenario"
        );
    }

    /// A fixture's world objects survive the conversion, which is what an
    /// arriving agent is actually shown.
    #[test]
    fn a_region_fixture_keeps_its_objects() {
        let mut world = SceneFixtures::new();
        world.objects.push(
            PrimFixture::boxed(
                RegionLocalObjectId(7),
                ObjectKey::from(uuid::Uuid::from_u128(7)),
                sl_types::key::AgentKey::from(uuid::Uuid::from_u128(1)),
                sl_types::lsl::Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                sl_types::lsl::Vector {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                },
            )
            .build(),
        );
        let scenario = RegionFixture {
            world,
            ..RegionFixture::new()
        }
        .into_scenario();
        assert_eq!(scenario.world.objects.len(), 1);
    }
}
