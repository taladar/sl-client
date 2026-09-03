//! The named scenes a fake grid can be started with, looked up by name.
//!
//! A scenario is a *name in the repository* for one region's content, so a
//! harness that starts the grid, runs two viewers against it and compares what
//! they saw can say which scene it photographed without anyone retyping a
//! command line. `catalogue` is the first one; the point of the registry is
//! that adding the next scene does not change the harness, the launcher script
//! or the binary's argument parsing.
//!
//! A scenario is expressed as *how it dresses a region*, not as a
//! [`RegionFixture`](super::RegionFixture), because the stock scene is not a
//! fixture at all — it is the grid-wide default a [`RegionConfig`] with no
//! scenario of its own inherits, and flattening it into a fixture would drop
//! the arrival greeting and the legacy UDP asset fixtures that come with it.

use sl_types::lsl::Vector;

use crate::runtime::RegionConfig;

/// Something in a scene worth aiming a camera at: a name and where it stands,
/// in region metres.
///
/// This is how a harness says *what* it photographed without knowing which
/// scene it loaded — "the landmark called `mesh-cube`" rather than a
/// hard-coded position — and it is what the binary logs on startup so a person
/// driving a viewer by hand can fly to one.
#[derive(Debug, Clone, PartialEq)]
pub struct Landmark {
    /// The stable name of the thing.
    pub name: String,
    /// Where it stands, in region metres.
    pub position: Vector,
}

/// One named scene: what it is called, what it shows, how it dresses a region,
/// and what stands in it.
#[derive(Debug, Clone, Copy)]
pub struct NamedScenario {
    /// The stable name a harness, a launcher and the binary's `--scenario`
    /// all use.
    pub name: &'static str,
    /// One line saying what the scene contains, shown in the binary's help.
    pub summary: &'static str,
    /// How the scene dresses a region.
    dress: fn(RegionConfig) -> RegionConfig,
    /// What stands in the scene, in the order a camera sweeping it meets them.
    landmarks: fn() -> Vec<Landmark>,
}

impl NamedScenario {
    /// `region` with this scene's content: what a
    /// [`FakeGridBuilder::region`](crate::FakeGridBuilder::region) is handed.
    #[must_use]
    pub fn dress(&self, region: RegionConfig) -> RegionConfig {
        (self.dress)(region)
    }

    /// What stands in this scene, and where.
    #[must_use]
    pub fn landmarks(&self) -> Vec<Landmark> {
        (self.landmarks)()
    }

    /// The landmark called `name`, or `None` if this scene has none.
    #[must_use]
    pub fn landmark(&self, name: &str) -> Option<Landmark> {
        self.landmarks()
            .into_iter()
            .find(|landmark| landmark.name == name)
    }
}

/// The scenario a grid starts with when none is named: the stock content every
/// region had before scenarios were named at all.
pub const DEFAULT: &str = "stock";

/// The border scene: the region's content replaced by [`super::border()`].
fn border(region: RegionConfig) -> RegionConfig {
    super::border().into_region(region)
}

/// The one thing standing in the border scene: its marker pillar.
fn border_landmarks() -> Vec<Landmark> {
    vec![Landmark {
        name: "border-marker".to_owned(),
        position: super::border::marker_position(),
    }]
}

/// Every named scene, in the order the binary's help lists them.
const ALL: [NamedScenario; 3] = [
    NamedScenario {
        name: "stock",
        summary: "the standard region: one region-wide parcel, one scripted box, \
                  an arrival greeting, the stock inventory and library",
        dress: stock,
        landmarks: stock_landmarks,
    },
    NamedScenario {
        name: "catalogue",
        summary: "the named prim catalogue: one prim per rendering feature in a \
                  west-to-east row, an NPC avatar, every asset they reference",
        dress: catalogue,
        landmarks: catalogue_landmarks,
    },
    NamedScenario {
        name: "border",
        summary: "one checkered marker pillar floating just inside the region's \
                  west edge, for looking at (and walking into) the region next \
                  door",
        dress: border,
        landmarks: border_landmarks,
    },
];

/// The stock scene: a region dressed with nothing, so it inherits the
/// grid-wide [`Scenario::default`](crate::Scenario).
const fn stock(region: RegionConfig) -> RegionConfig {
    region
}

/// The one thing standing in the stock scene: its scripted box.
fn stock_landmarks() -> Vec<Landmark> {
    vec![Landmark {
        name: "scripted-box".to_owned(),
        position: crate::scenario::STOCK_SCRIPTED_OBJECT_POSITION,
    }]
}

/// The catalogue scene: the region's content replaced wholesale by
/// [`super::catalogue()`].
fn catalogue(region: RegionConfig) -> RegionConfig {
    super::catalogue().into_region(region)
}

/// The catalogue's row of prims, west to east, its NPC standing at the west end
/// of the row, and the seated NPC on its bench west of that.
///
/// The seated one's landmark is the position its avatar object ends up at
/// (`seated_npc_position`) rather than its own `position`, which is
/// parent-relative — a camera aimed at a landmark is aimed at a place in the
/// region.
fn catalogue_landmarks() -> Vec<Landmark> {
    let npc = super::catalogue::npc();
    let seated = super::catalogue::seated_npc();
    let named = |npc: &super::npcs::NpcFixture| {
        format!(
            "{}-{}",
            npc.identity.first_name.to_lowercase(),
            npc.identity.last_name.to_lowercase()
        )
    };
    [
        Landmark {
            name: named(&seated),
            position: super::catalogue::seated_npc_position(),
        },
        Landmark {
            name: "sit-bench".to_owned(),
            position: super::catalogue::seat().build().motion.position,
        },
        Landmark {
            name: named(&npc),
            position: npc.position,
        },
    ]
    .into_iter()
    .chain(
        super::catalogue::entries()
            .into_iter()
            .map(|entry| Landmark {
                name: entry.name.to_owned(),
                position: entry.position(),
            }),
    )
    .collect()
}

/// Every named scene, in the order the binary's help lists them.
#[must_use]
pub const fn all() -> &'static [NamedScenario] {
    &ALL
}

/// Every scene's name, in the same order — what a command-line parser offers
/// as its possible values.
#[must_use]
pub fn names() -> Vec<&'static str> {
    ALL.iter().map(|scenario| scenario.name).collect()
}

/// The scene called `name`, or `None` if there is no such scene.
#[must_use]
pub fn scenario(name: &str) -> Option<NamedScenario> {
    ALL.iter().copied().find(|scenario| scenario.name == name)
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// What a test returns when a lookup that should succeed does not.
    type TestError = Box<dyn core::error::Error>;

    /// Every name the registry offers resolves back to a scene, and the
    /// default is one of them — the property a `--scenario` value parser built
    /// from [`names`] depends on.
    #[test]
    fn every_offered_name_resolves() {
        for name in names() {
            assert!(
                scenario(name).is_some_and(|found| found.name == name),
                "the offered name {name:?} does not resolve"
            );
        }
        assert!(
            scenario(DEFAULT).is_some(),
            "the default scenario {DEFAULT:?} is not in the registry"
        );
        assert!(
            scenario("no-such-scene").is_none(),
            "an unknown name resolved to a scene"
        );
    }

    /// The stock scene leaves the region's scenario alone, so the region
    /// inherits the grid-wide default rather than a flattened copy of it.
    #[test]
    fn the_stock_scene_dresses_a_region_with_nothing() -> Result<(), TestError> {
        let dressed = scenario("stock")
            .ok_or("the stock scene is not in the registry")?
            .dress(RegionConfig::default());
        assert!(
            dressed.scenario.is_none(),
            "the stock scene gave the region a scenario of its own"
        );
        Ok(())
    }

    /// The catalogue scene rezzes the catalogue: the region carries a scenario
    /// whose world holds every catalogue entry (plus the linkset children,
    /// which are objects without being entries) and the catalogue's NPC.
    #[test]
    fn the_catalogue_scene_rezzes_the_catalogue() -> Result<(), TestError> {
        let dressed = scenario("catalogue")
            .ok_or("the catalogue scene is not in the registry")?
            .dress(RegionConfig::default());
        let world = dressed
            .scenario
            .ok_or("the catalogue scene gave the region no scenario")?
            .world;
        for entry in super::super::catalogue::entries() {
            assert!(
                world
                    .objects
                    .iter()
                    .any(|object| object.local_id == entry.local_id),
                "the dressed region does not hold the catalogue's {:?} prim",
                entry.name
            );
        }
        assert_eq!(
            world.npcs.len(),
            2,
            "the dressed region does not hold both of the catalogue's NPCs"
        );
        let seated = world
            .npcs
            .iter()
            .find(|npc| npc.seat.is_some())
            .ok_or("the dressed region holds no seated NPC")?;
        assert_eq!(
            seated.seat,
            Some(super::super::catalogue::SEAT_LOCAL_ID),
            "the seated NPC does not sit on the catalogue's bench"
        );
        assert!(
            world
                .objects
                .iter()
                .any(|object| object.local_id == super::super::catalogue::SEAT_LOCAL_ID),
            "the dressed region does not hold the bench its NPC sits on"
        );
        Ok(())
    }

    /// Every scene names what stands in it, and a landmark is where the
    /// content that backs it actually is — a camera aimed at a landmark has to
    /// find the prim there.
    #[test]
    fn a_landmark_stands_where_its_content_does() -> Result<(), TestError> {
        for scene in all() {
            assert!(
                !scene.landmarks().is_empty(),
                "the {:?} scene names nothing to look at",
                scene.name
            );
        }
        let catalogue =
            scenario("catalogue").ok_or("the catalogue scene is not in the registry")?;
        for entry in super::super::catalogue::entries() {
            assert_eq!(
                catalogue.landmark(entry.name).map(|found| found.position),
                Some(entry.position()),
                "the catalogue scene's {:?} landmark is not where the prim is",
                entry.name
            );
        }
        let border = scenario("border").ok_or("the border scene is not in the registry")?;
        assert_eq!(
            border.landmark("border-marker").map(|found| found.position),
            Some(super::super::border::marker_position()),
            "the border scene's landmark is not where its marker pillar is"
        );
        let stock = scenario("stock").ok_or("the stock scene is not in the registry")?;
        assert_eq!(
            stock.landmark("scripted-box").map(|found| found.position),
            Some(crate::scenario::STOCK_SCRIPTED_OBJECT_POSITION)
        );
        assert!(
            stock.landmark("mesh-cube").is_none(),
            "the stock scene claims a landmark only the catalogue has"
        );
        Ok(())
    }
}
