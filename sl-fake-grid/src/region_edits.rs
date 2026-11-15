//! What a region's scripts changed of it beyond its prims and parcels
//! (`server-lsl-lib-ossl-land-writers`): the ground once a script moved it,
//! the patches its avatars are still to be told of, the wind models'
//! parameters, the environments set since the region started, and a restart
//! being counted down.
//!
//! Held in the region's world ([`crate::world::SceneFixtures::edits`]), so
//! every reader of the ground — a session streaming it, the avatar controller,
//! physics, a script — sees the same heights.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sl_lsl_runtime::vm::land::WindParamError;
use sl_proto::{AgentKey, EnvironmentSettings};

use crate::terrain::{Heightmap, LiveGround, PATCH_CELLS, TerrainFixture};

/// The region's changes a script made, beside its prims and parcels.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegionEdits {
    /// The ground since a script first moved it; the fixture's until then.
    /// Shared so a session's stream reads it without copying the region.
    pub(crate) ground: Option<Arc<Heightmap>>,
    /// The patches moved since the region last told its avatars, by patch
    /// column and row.
    pub(crate) moved: BTreeSet<(u32, u32)>,
    /// Whether every patch is to be sent again (`osTerrainFlush`).
    pub(crate) flush: bool,
    /// The wind models' parameters.
    pub(crate) wind: WindModels,
    /// The environments set since the region started.
    pub(crate) environments: EnvironmentEdits,
    /// The restart being counted down: the seconds before it at which a
    /// notice is still due. The countdown's later notices and the restart
    /// itself are `server-fake-grid-estate-actions`'.
    pub(crate) restart: Option<Vec<i32>>,
}

impl RegionEdits {
    /// The ground as it stands over `fixture`.
    #[must_use]
    pub(crate) fn ground<'ground>(
        &'ground self,
        fixture: &'ground TerrainFixture,
    ) -> LiveGround<'ground> {
        fixture.live(self.ground.as_deref())
    }

    /// Keeps `settings` as the environment of the parcel `parcel_id`, or of
    /// the region for `-1`; `None` for one dropped.
    pub(crate) fn keep_environment(
        &mut self,
        parcel_id: i32,
        settings: Option<EnvironmentSettings>,
    ) {
        if parcel_id == -1 {
            self.environments.region =
                Some(settings.map_or(EditedEnvironment::Dropped, |settings| {
                    EditedEnvironment::Set(Box::new(settings))
                }));
        } else {
            let _previous = self.environments.parcels.insert(parcel_id, settings);
        }
    }

    /// Serves the environments set since the region started on a session
    /// opening in it, over what its setup installed: `stock` for a region
    /// whose own was dropped, and `agent`'s own where it has one.
    pub(crate) fn serve_environments(
        &self,
        sim: &mut sl_proto::SimSession,
        stock: &EnvironmentSettings,
        agent: AgentKey,
    ) {
        if let Some(region) = &self.environments.region {
            sim.set_environment(region.settings().cloned().unwrap_or_else(|| stock.clone()));
        }
        for (parcel_id, settings) in &self.environments.parcels {
            match settings {
                Some(settings) => sim.set_environment(settings.clone()),
                None => sim.forget_environment(*parcel_id),
            }
        }
        if let Some(own) = self.environments.agents.get(&agent) {
            sim.set_agent_environment(Some(own.clone()));
        }
    }

    /// The patches of the ground moved since the region last told its
    /// avatars — every patch after a flush — stamped with `handle`, now told;
    /// [`None`] when nothing moved.
    pub(crate) fn take_moved(
        &mut self,
        fixture: &TerrainFixture,
        handle: sl_wire::RegionHandle,
    ) -> Option<crate::world::RegionChange> {
        if !self.flush && self.moved.is_empty() {
            return None;
        }
        let ground = fixture.live(self.ground.as_deref());
        let patches = if self.flush {
            ground.to_patches(handle)
        } else {
            self.moved
                .iter()
                .map(|(x, y)| ground.patch(handle, *x, *y))
                .collect()
        };
        self.flush = false;
        self.moved.clear();
        Some(crate::world::RegionChange::TerrainPatched(patches))
    }

    /// Sets the ground at the whole-metre point `(x, y)` to `height`, the
    /// fixture's heights taken over the first time, and notes its patch as
    /// moved when the height changed.
    pub(crate) fn set_height(&mut self, fixture: &TerrainFixture, x: u32, y: u32, height: f32) {
        let ground = self
            .ground
            .get_or_insert_with(|| Arc::new(Heightmap::of(fixture)));
        let (Ok(column), Ok(row)) = (usize::try_from(x), usize::try_from(y)) else {
            return;
        };
        if Arc::make_mut(ground).set(column, row, height) {
            let _new = self.moved.insert((x / PATCH_CELLS, y / PATCH_CELLS));
        }
    }
}

/// The parameters of the two wind models OpenSim ships, both loaded whichever
/// is active, at their stock values — what `osGetWindParam` and
/// `osSetWindParam` read and write. The fake grid's wind is its fixture's,
/// the same everywhere, whatever they say: no wind model is simulated.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WindModels {
    /// `SimpleRandomWind`'s `strength`.
    strength: f32,
    /// `ConfigurableWind`'s `avgStrength`, `avgDirection`, `varStrength`,
    /// `varDirection` and `rateChange`, in that order.
    configurable: [f32; 5],
}

impl Default for WindModels {
    /// OpenSim's stock `[Wind]` values.
    fn default() -> Self {
        Self {
            strength: 1.0,
            configurable: [5.0, 0.0, 5.0, 30.0, 1.0],
        }
    }
}

/// `SimpleRandomWind`, as it names itself.
const SIMPLE_RANDOM_WIND: &str = "SimpleRandomWind";
/// `ConfigurableWind`, as it names itself.
const CONFIGURABLE_WIND: &str = "ConfigurableWind";
/// `ConfigurableWind`'s parameters, in [`WindModels::configurable`]'s order.
const CONFIGURABLE_PARAMS: [&str; 5] = [
    "avgStrength",
    "avgDirection",
    "varStrength",
    "varDirection",
    "rateChange",
];

impl WindModels {
    /// The slot of the model `plugin`'s parameter `param`.
    ///
    /// # Errors
    ///
    /// [`WindParamError`] for a model or a parameter there is none of.
    fn slot(&mut self, plugin: &str, param: &str) -> Result<&mut f32, WindParamError> {
        match plugin {
            SIMPLE_RANDOM_WIND if param == "strength" => Ok(&mut self.strength),
            SIMPLE_RANDOM_WIND => Err(WindParamError::NoParam {
                plugin: SIMPLE_RANDOM_WIND.to_owned(),
            }),
            CONFIGURABLE_WIND => CONFIGURABLE_PARAMS
                .iter()
                .position(|name| *name == param)
                .and_then(|index| self.configurable.get_mut(index))
                .ok_or_else(|| WindParamError::NoParam {
                    plugin: CONFIGURABLE_WIND.to_owned(),
                }),
            _ => Err(WindParamError::NoPlugin),
        }
    }

    /// The model `plugin`'s parameter `param`.
    ///
    /// # Errors
    ///
    /// [`WindParamError`] for a model or a parameter there is none of.
    pub(crate) fn get(&self, plugin: &str, param: &str) -> Result<f32, WindParamError> {
        self.clone().slot(plugin, param).map(|value| *value)
    }

    /// Sets the model `plugin`'s parameter `param`; nothing for a model or a
    /// parameter there is none of, as OpenSim's models ignore one.
    pub(crate) fn set(&mut self, plugin: &str, param: &str, value: f32) {
        if let Ok(slot) = self.slot(plugin, param) {
            *slot = value;
        }
    }
}

/// The environments set in a region since it started, over what its fixture
/// and its sessions' setup installed.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct EnvironmentEdits {
    /// The region's: [`None`] when nobody set it.
    pub(crate) region: Option<EditedEnvironment>,
    /// Each parcel's by region-local id: `None` once dropped.
    pub(crate) parcels: BTreeMap<i32, Option<EnvironmentSettings>>,
    /// Each avatar's own, which overrides every other it would see.
    pub(crate) agents: BTreeMap<AgentKey, EnvironmentSettings>,
}

/// What became of an environment somebody changed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EditedEnvironment {
    /// It was set to this.
    Set(Box<EnvironmentSettings>),
    /// It was dropped, so what it overrode shows: for the region, the grid's
    /// default day.
    Dropped,
}

impl EditedEnvironment {
    /// The environment it was set to, or [`None`] once dropped.
    pub(crate) const fn settings(&self) -> Option<&EnvironmentSettings> {
        match self {
            Self::Set(settings) => Some(settings),
            Self::Dropped => None,
        }
    }
}
