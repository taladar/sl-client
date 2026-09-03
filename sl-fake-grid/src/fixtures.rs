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
//! [`scenarios`] names the scenes: `stock`, `catalogue`, and whatever comes
//! next. A harness selects one by name rather than by rebuilding a region
//! itself, which is what lets two viewers agree on which scene they
//! photographed.
//!
//! [`into_region`]: RegionFixture::into_region

pub mod arrival;
pub mod border;
pub mod catalogue;
pub mod npcs;
pub mod prims;
pub mod scenarios;

use std::sync::Arc;

use sl_proto::{EnvironmentSettings, InMemoryAssetSource, ObjectMediaState, SimSession};
use sl_types::key::ObjectKey;
use sl_wire::LegacyMaterial;

use crate::runtime::RegionConfig;
use crate::scenario::Scenario;
use crate::terrain::TerrainFixture;
use crate::world::SceneFixtures;

pub use arrival::arrival;
pub use border::border;
pub use catalogue::{CatalogueEntry, catalogue};
pub use npcs::{NpcAppearance, NpcBake, NpcFixture};
pub use prims::{DEFAULT_FACE_COUNT, FaceStyle, PrimFixture, SculptKind, blank_texture, linkset};
pub use scenarios::{Landmark, NamedScenario};

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
    /// GLTF materials. Folded into the grid-wide store when the grid starts
    /// (`assets.rs`) — a fixture states what its content needs, and every
    /// region then serves it.
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

    /// The fixture as a [`Scenario`]: the world as it is, the assets plus
    /// every NPC's bakes (which the builder then folds into the grid-wide
    /// store), and a setup hook that installs the region materials,
    /// the object media and the environment on every fresh session.
    ///
    /// An NPC's baked textures are registered here rather than by the caller
    /// because the appearance already names their ids — the fixture describes
    /// the avatar, and this is the one place that knows a bake is fetched over
    /// `GetTexture` like any other texture.
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
        // Start from the stock asset store, not an empty one. It carries the
        // built-in library textures every viewer asks any grid for -- the sun
        // and moon discs, the cloud, water normals, the default prim texture,
        // the terrain details -- which are not fixture content and which a
        // fixture has no business dropping. Without them the ground shades
        // flat, the sky has no sun in it, and each missing id burns its whole
        // retry budget on every arrival, so the scene never goes quiet and a
        // capture harness waiting for quiescence always times out.
        //
        // The fixture's own assets are inserted over the top, so a fixture that
        // deliberately replaces a built-in still wins.
        let mut assets = crate::scenario::default_assets();
        for (key, bytes) in self.assets.iter() {
            let _previous = assets.insert(key, bytes.to_vec());
        }
        for npc in &self.world.npcs {
            for (key, bytes) in npc.bake_assets() {
                let _previous = assets.insert(key, bytes);
            }
        }
        let npc_identities = self
            .world
            .npcs
            .iter()
            .map(|npc| npc.identity.display_name_record())
            .collect::<Vec<_>>();
        Scenario {
            setup: Arc::new(move |sim: &mut SimSession, now| {
                // A fixture describes a region's *objects*, not the account
                // logging in and not the region's civic furniture, so it is
                // layered on top of the stock session seeding rather than
                // replacing it. Skipping this used to leave the agent with an
                // empty inventory, and a login response with no
                // `inventory-root` is one no viewer descended from the Linden
                // client will accept: its success check requires a usable
                // inventory root, so the login is refused after otherwise
                // succeeding. The objects and assets below still replace the
                // stock ones, which is what "replaces region content" means.
                crate::scenario::default_setup(sim, now);

                // Every NPC the fixture rezzes needs a people-service record,
                // or its name tag renders as `(???) (???)` — the id lands in
                // the `GetDisplayNames` reply's `bad_ids` and the viewer caches
                // that for an hour.
                for npc in &npc_identities {
                    sim.set_display_name(npc.clone());
                }

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
            assets,
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
