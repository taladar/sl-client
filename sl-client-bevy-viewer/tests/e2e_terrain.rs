//! End-to-end tests for the ground a viewer builds out of what a grid sends
//! it, on the fake grid, through the automation driver alone
//! ([[gridspec-terrain]]).
//!
//! The two live grids stream a region's ground differently — how many
//! patches a message holds, the order they come in, and how a patch with no
//! relief is written — and the fake grid sends each flavour's. The ground the
//! viewer ends up with has to be the same one either way: the height its own
//! pick resolver gives for a point is the height the region declares there.
//!
//! A live grid declares no heights to hold the viewer to, so the one test
//! that runs there (`SL_E2E_GRID=opensim|aditi`) asks less: that the viewer
//! has ground to rest the pointer on beside its avatar at all.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use sl_automation_proto::Probe;
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::{Heightfield, ImitatedGrid, RegionConfig, TerrainFixture};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// How far the viewer's ground may sit from the region's, in metres: the
    /// codec quantizes a patch's heights, and one that holds a terrace edge
    /// rings a little either side of it.
    const HEIGHT_SLOP: f32 = 0.5;

    /// Five terraces stepping up west to east, two metres at a time. Each is
    /// 51.2 m wide, so most patches are flat — which OpenSim writes as a
    /// header alone — and the ones a terrace edge runs through are not.
    const TERRACES: Heightfield = Heightfield::Steps {
        base: 22.0,
        rise: 2.0,
        count: 5,
    };

    /// Where the ground is asked about, in region metres, all ahead of an
    /// agent that logs in at the centre facing east: a flat patch beside it;
    /// either side of the terrace edge at 153.6 m, inside the one patch it
    /// runs through; and a flat patch beyond.
    const POINTS: [(f32, f32); 4] = [
        (140.0, 134.0),
        (150.0, 134.0),
        (158.0, 134.0),
        (172.0, 122.0),
    ];

    /// The region's ground.
    fn terrain() -> TerrainFixture {
        TerrainFixture::default().with_heights(TERRACES)
    }

    /// A stage for `name` with the one viewer `Alpha` in a terraced region of
    /// a grid imitating `flavour`.
    fn stage(name: &str, flavour: ImitatedGrid) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(RegionConfig {
                terrain: terrain(),
                ..RegionConfig::default()
            })
            .configure_grid(move |grid| grid.imitates(flavour))
    }

    /// Wait until `alpha` stands in a region.
    async fn arrived(alpha: &Viewer) -> Result<(), BodyError> {
        let _arrived = alpha
            .expect_state(Probe::Agent)
            .at("/region")
            .timeout(WAIT)
            .to_be_present()
            .await?;
        Ok(())
    }

    /// Rest the pointer on each of [`POINTS`] and hold the height the viewer
    /// has for the ground there to the region's.
    async fn the_ground_is_the_regions(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let region = terrain();
        for (east, north) in POINTS {
            let [hit_east, hit_north, height] = alpha
                .world()
                .ground(stage.home_region(), east, north)
                .timeout(WAIT)
                .hover()
                .await?;
            let declared = region.height_at(east, north);
            if (height - declared).abs() > HEIGHT_SLOP {
                return Err(format!(
                    "the viewer has the ground at <{east}, {north}> at {height} m (the pointer \
                     landed on <{hit_east}, {hit_north}>), and the region declares {declared} m"
                )
                .into());
            }
        }
        Ok(())
    }

    /// How far from its avatar, ahead and to the side, the pointer is rested
    /// on the ground wherever the grid put the avatar, in metres.
    const BESIDE: f32 = 4.0;

    /// How far from the point asked for the pointer may land, in metres.
    const AIM_SLOP: f32 = 1.0;

    /// **Ground from whichever grid this is**: wherever the avatar stands,
    /// the viewer has ground a few metres from it that the pointer can rest
    /// on — the pick resolver says ground there, and says it where it was
    /// asked. On a live grid that is the grid's own `LayerData`, decoded and
    /// built into something a ray can hit.
    #[test]
    fn the_viewer_has_ground_beside_its_avatar() -> Result<(), TestError> {
        StageBuilder::new("terrain_beside")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                arrived(alpha).await?;
                let readout = alpha.agent().await?;
                let spot = readout.position.ok_or("Alpha has no position")?;
                let eye = readout.camera_eye.ok_or("Alpha has no camera")?;
                let (ahead_x, ahead_y) = (spot[0] - eye[0], spot[1] - eye[1]);
                let length = ahead_x.hypot(ahead_y).max(0.001);
                let (ahead_x, ahead_y) = (ahead_x / length, ahead_y / length);
                let east = (spot[0] + BESIDE * ahead_x + BESIDE * ahead_y).clamp(1.0, 254.0);
                let north = (spot[1] + BESIDE * ahead_y - BESIDE * ahead_x).clamp(1.0, 254.0);
                let [hit_east, hit_north, height] = alpha
                    .world()
                    .ground(stage.home_region(), east, north)
                    .timeout(WAIT)
                    .hover()
                    .await?;
                if (hit_east - east).hypot(hit_north - north) > AIM_SLOP || !height.is_finite() {
                    return Err(format!(
                        "asked for the ground at <{east}, {north}>, the pointer landed on \
                         <{hit_east}, {hit_north}, {height}>"
                    )
                    .into());
                }
                Ok(())
            })?;
        Ok(())
    }

    /// **The ground as Second Life sends it**: outwards from where the agent
    /// stands, a message kept within 1,200 bytes, every patch transformed.
    #[test]
    fn a_second_life_flavoured_ground_has_the_regions_heights() -> Result<(), TestError> {
        stage("terrain_second_life", ImitatedGrid::SecondLife).run(the_ground_is_the_regions)?;
        Ok(())
    }

    /// **The ground as OpenSim sends it**: outwards from the agent's patch, a
    /// message closed once past 890 bytes, a flat patch as a header alone.
    #[test]
    fn an_opensim_flavoured_ground_has_the_regions_heights() -> Result<(), TestError> {
        stage("terrain_open_sim", ImitatedGrid::OpenSim).run(the_ground_is_the_regions)?;
        Ok(())
    }
}
