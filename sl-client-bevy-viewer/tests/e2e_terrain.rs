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
//!
//! And the ground outlives the session: a viewer that relogs into a region
//! whose ground has not streamed yet still knows the height it stands on,
//! from the land it kept on disk ([[viewer-terrain-land-cache-on-disk]]).
//!
//! And the Land tool's brushes say when the land under them is not the
//! agent's to terraform: a press on a stranger's parcel warns
//! `ParcelNoTerraforming` and strokes anyway, a press on the agent's own land
//! says nothing ([[viewer-terraform-parcel-refusal]]) — on both fake flavours,
//! and on Second Life's own land.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use sl_automation_proto::{Locator, Probe, Role};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_fake_grid::world::rect_parcel;
    use sl_fake_grid::{Heightfield, ImitatedGrid, RegionConfig, RegionFixture, TerrainFixture};
    use sl_proto::{
        AgentKey, LandBrushAction, OwnerKey, ParcelKey, RegionLocalParcelId, ServerEvent, Uuid,
    };
    use sl_viewer_driver::{GroundHandle, Viewer};
    use tokio::sync::broadcast;

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

    /// The height of [`KEPT_GROUND`], in metres: none of the stock fixtures'
    /// heights, so only the region's own ground can be where it came from.
    const KEPT_HEIGHT: f32 = 37.25;

    /// A flat region at [`KEPT_HEIGHT`].
    const KEPT_GROUND: Heightfield = Heightfield::Flat {
        height: KEPT_HEIGHT,
    };

    /// How long a pick that must find no ground is given before it counts as
    /// having found none.
    const NO_GROUND_WAIT: Duration = Duration::from_secs(5);

    /// Wait until `alpha`'s ground floor stands it on [`KEPT_HEIGHT`].
    async fn stands_on_the_kept_ground(alpha: &Viewer, when: &str) -> Result<(), BodyError> {
        let mut last = None;
        let polled = tokio::time::timeout(WAIT, async {
            loop {
                let ground = alpha.agent().await?.ground_height;
                if ground.is_some_and(|height| (height - KEPT_HEIGHT).abs() <= HEIGHT_SLOP) {
                    return Ok::<(), BodyError>(());
                }
                last = ground;
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await;
        match polled {
            Ok(result) => result,
            Err(_elapsed) => Err(format!(
                "{when}, the viewer has the ground under its avatar at {last:?}, and the \
                 region's is {KEPT_HEIGHT} m"
            )
            .into()),
        }
    }

    /// **The ground is kept across a relog**: a viewer that has stood in a
    /// region logs out and back in while the region streams it no ground at
    /// all — the login window before the terrain arrives, held open — and its
    /// avatar's ground floor still has the region's height under it, read
    /// from the land the viewer saved as it quit. The pointer finds no ground
    /// to rest on, which is what says none streamed.
    #[test]
    fn the_ground_under_the_avatar_survives_a_relog_before_the_terrain_streams()
    -> Result<(), TestError> {
        StageBuilder::new("terrain_kept")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(RegionConfig {
                terrain: TerrainFixture::default().with_heights(KEPT_GROUND),
                ..RegionConfig::default()
            })
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                arrived(&alpha).await?;
                stands_on_the_kept_ground(&alpha, "once the region's ground streamed").await?;

                stage
                    .grid()?
                    .set_withholds_ground(stage.home_region(), true)?;
                let alpha = stage.relog("Alpha").await?;
                arrived(&alpha).await?;
                stands_on_the_kept_ground(&alpha, "after a relog with no ground streamed").await?;
                if let Ok(hit) = alpha
                    .world()
                    .ground(stage.home_region(), 140.0, 128.0)
                    .timeout(NO_GROUND_WAIT)
                    .hover()
                    .await
                {
                    return Err(format!(
                        "the region was to stream no ground, and the pointer rested on {hit:?}"
                    )
                    .into());
                }
                Ok(())
            })?;
        Ok(())
    }

    /// Low ground behind the agent, below the terrace it stands on: 2.4 m
    /// and 12 m west of the terrace wall at 102.4 m, which stands between
    /// the point and the camera behind an agent that faces east.
    const BELOW_THE_WALL: [(f32, f32); 2] = [(100.0, 120.0), (90.0, 128.0)];

    /// **Low ground at the foot of a rise is reached**: the pointer rests on
    /// a point of the lower terrace close under the wall of the one the agent
    /// stands on. The camera has to frame the point from where the terrace
    /// does not hide it; framed from the side it was on, its eye ended inside
    /// the terrace and no click reached the point
    /// ([[viewer-automation-ground-reveal-eye-under-terrain]]).
    #[test]
    fn low_ground_under_a_terrace_wall_is_reached() -> Result<(), TestError> {
        stage("terrain_below_the_wall", ImitatedGrid::SecondLife).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            arrived(alpha).await?;
            let region = terrain();
            for (east, north) in BELOW_THE_WALL {
                let [hit_east, hit_north, height] = alpha
                    .world()
                    .ground(stage.home_region(), east, north)
                    .timeout(WAIT)
                    .hover()
                    .await?;
                let declared = region.height_at(east, north);
                if (hit_east - east).hypot(hit_north - north) > AIM_SLOP
                    || (height - declared).abs() > HEIGHT_SLOP
                {
                    return Err(format!(
                        "asked for the ground at <{east}, {north}, {declared}>, the pointer \
                         landed on <{hit_east}, {hit_north}, {height}>"
                    )
                    .into());
                }
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

    // ---- The Land tool's parcel refusal -----------------------------------

    /// The Build window's floater id.
    const BUILD_WINDOW: &str = "build-tools";

    /// The template a press on land the agent may not terraform raises.
    const PARCEL_REFUSAL: &str = "ParcelNoTerraforming";

    /// The stranger's parcel: the region's northern half.
    const STRANGERS: RegionLocalParcelId = RegionLocalParcelId(1);

    /// The stranger's parcel's name, which the warning names.
    const STRANGERS_NAME: &str = "Elsewhere";

    /// Alpha's own parcel: the region's southern half.
    const ALPHAS: RegionLocalParcelId = RegionLocalParcelId(2);

    /// Where the brush is pressed on the stranger's land, ahead of an agent
    /// that logs in at the centre facing east.
    const ON_THE_STRANGERS: (f32, f32) = (140.0, 136.0);

    /// Where the brush is pressed on Alpha's own land.
    const ON_ALPHAS: (f32, f32) = (140.0, 120.0);

    /// How many presses a warning is given to come: the first press can beat
    /// the answer to the hover's question about the parcel, and is then
    /// silent, as the reference's is.
    const PRESSES: usize = 5;

    /// How long one press's warning is given.
    const PER_PRESS: Duration = Duration::from_secs(3);

    /// How long after the grid answered the hover request a press on Alpha's
    /// own land is left before it, and how long its silence is watched.
    const SETTLE: Duration = Duration::from_secs(2);

    /// A region split east-west at its middle: the north half a stranger's,
    /// the south half to be handed to Alpha once its id is known.
    fn split_region() -> RegionConfig {
        let stranger = AgentKey::from(Uuid::from_u128(0x7E44_0001));
        let mut fixture = RegionFixture::new();
        fixture.world.add_parcel(
            rect_parcel(
                STRANGERS,
                OwnerKey::Agent(stranger),
                STRANGERS_NAME,
                0.0,
                128.0,
                256.0,
                256.0,
            ),
            ParcelKey::from(Uuid::from_u128(0x7E44_000A)),
            0.0,
        );
        fixture.world.add_parcel(
            rect_parcel(
                ALPHAS,
                OwnerKey::Agent(stranger),
                "Home",
                0.0,
                0.0,
                256.0,
                128.0,
            ),
            ParcelKey::from(Uuid::from_u128(0x7E44_000B)),
            0.0,
        );
        fixture.into_region(RegionConfig::default())
    }

    /// Open the Build window on the Land tool with the Revert brush picked —
    /// the brush that does nothing to land never edited, so a stroke the grid
    /// takes is harmless on any grid.
    async fn pick_the_revert_brush(alpha: &Viewer) -> Result<(), BodyError> {
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window(BUILD_WINDOW);
        let _open = alpha.expect(&build).timeout(WAIT).to_be_visible().await?;
        let _land = build
            .get(Locator::role(Role::Radio).name_key("build-tool-select-land"))
            .click()
            .await?;
        let revert = build.get(Locator::role(Role::Radio).name_key("build-land-revert"));
        let _shown = alpha.expect(&revert).timeout(WAIT).to_be_visible().await?;
        let _picked = revert.click().await?;
        let _checked = alpha.expect(&revert).timeout(WAIT).to_be_checked().await?;
        Ok(())
    }

    /// Press the brush on `ground` until the refusal shows, and return its
    /// text.
    async fn press_until_refused(
        alpha: &Viewer,
        ground: &GroundHandle,
    ) -> Result<String, BodyError> {
        for _press in 0..PRESSES {
            let _hit = ground.click().await?;
            if alpha
                .expect_notification()
                .timeout(PER_PRESS)
                .to_show(PARCEL_REFUSAL)
                .await
                .is_ok()
            {
                let shown = alpha.notifications().await?;
                return shown
                    .into_iter()
                    .find(|notification| notification.template == PARCEL_REFUSAL)
                    .map(|notification| notification.text)
                    .ok_or_else(|| "the refusal went before it could be read".into());
            }
        }
        let shown = alpha.notifications().await?;
        Err(format!(
            "{PRESSES} presses of the brush raised no {PARCEL_REFUSAL}; the notifications are \
             {shown:?}"
        )
        .into())
    }

    /// Wait until the grid's session reports an event `wanted` accepts.
    async fn grid_hears(
        heard: &mut broadcast::Receiver<ServerEvent>,
        what: &str,
        wanted: impl Fn(&ServerEvent) -> bool + Send + Sync,
    ) -> Result<(), BodyError> {
        let seen = tokio::time::timeout(WAIT, async {
            loop {
                match heard.recv().await {
                    Ok(event) if wanted(&event) => return Ok(()),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        })
        .await;
        match seen {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(format!("the grid's event stream: {error}").into()),
            Err(_elapsed) => Err(format!("the grid never heard {what}").into()),
        }
    }

    /// Whether `event` is a revert stroke.
    const fn is_revert_stroke(event: &ServerEvent) -> bool {
        matches!(event, ServerEvent::ModifyLand { edit } if matches!(edit.action, LandBrushAction::Revert))
    }

    /// Whether `event` is the Land tool's hover question about a square of
    /// the half of the region `north_half` names: a request from the hover's
    /// own sequence range, never a snapping one.
    ///
    /// Any square of the half, not the one aimed at: while the camera frames
    /// the point the cursor sweeps the ground, and the question goes out for
    /// the first square past the parcel line — the one under the point is
    /// then in a parcel already answered, and rightly asks nothing.
    fn is_hover_request(event: &ServerEvent, north_half: bool) -> bool {
        matches!(
            event,
            ServerEvent::RequestParcelProperties { south, north, sequence_id, snap_selection: false, .. }
                if *sequence_id <= -50_000
                    && if north_half { *south >= 128.0 } else { *north <= 128.0 }
        )
    }

    /// The fake-grid stage: Alpha is handed the south half, its own land
    /// draws no word from a press, the stranger's north half draws the
    /// warning naming it, and the warned press still strokes.
    async fn the_brush_warns_on_a_strangers_parcel(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let me = stage.agent_id("Alpha")?;
        let session = stage.agent("Alpha").await?;
        session
            .with_world(|world, _sim| {
                if let Some(parcel) = world
                    .parcels
                    .iter_mut()
                    .find(|parcel| parcel.local_id == ALPHAS)
                {
                    parcel.owner = OwnerKey::Agent(me);
                }
            })
            .await;
        let mut heard = session.events();
        pick_the_revert_brush(alpha).await?;

        // Alpha's own land: the hover asks, the answer comes, and a press
        // strokes without a word.
        let home = alpha
            .world()
            .ground(stage.home_region(), ON_ALPHAS.0, ON_ALPHAS.1)
            .timeout(WAIT);
        let _rested = home.hover().await?;
        grid_hears(
            &mut heard,
            "the hover's question about Alpha's land",
            |event| is_hover_request(event, false),
        )
        .await?;
        tokio::time::sleep(SETTLE).await;
        let _hit = home.click().await?;
        grid_hears(&mut heard, "the stroke on Alpha's land", is_revert_stroke).await?;
        tokio::time::sleep(SETTLE).await;
        let shown = alpha.notifications().await?;
        if shown
            .iter()
            .any(|notification| notification.template == PARCEL_REFUSAL)
        {
            return Err(format!("a press on Alpha's own land was refused: {shown:?}").into());
        }

        // The stranger's land: the warning, naming the parcel, and the stroke
        // all the same.
        let elsewhere = alpha
            .world()
            .ground(stage.home_region(), ON_THE_STRANGERS.0, ON_THE_STRANGERS.1)
            .timeout(WAIT);
        let _rested = elsewhere.hover().await?;
        grid_hears(
            &mut heard,
            "the hover's question about the stranger's land",
            |event| is_hover_request(event, true),
        )
        .await?;
        let text = press_until_refused(alpha, &elsewhere).await?;
        if !text.contains(STRANGERS_NAME) {
            return Err(format!("the refusal does not name {STRANGERS_NAME}: {text:?}").into());
        }
        grid_hears(
            &mut heard,
            "the stroke on the stranger's land",
            is_revert_stroke,
        )
        .await?;
        Ok(())
    }

    /// **A brush on a stranger's land, as Second Life's flavour answers**.
    #[test]
    fn a_brush_on_a_strangers_parcel_warns_on_second_life() -> Result<(), TestError> {
        StageBuilder::new("terraform_parcel_second_life")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(split_region())
            .configure_grid(|grid| grid.imitates(ImitatedGrid::SecondLife))
            .needs(Need::GridControl)
            .run(the_brush_warns_on_a_strangers_parcel)?;
        Ok(())
    }

    /// **A brush on a stranger's land, as OpenSim's flavour answers**.
    #[test]
    fn a_brush_on_a_strangers_parcel_warns_on_opensim() -> Result<(), TestError> {
        StageBuilder::new("terraform_parcel_open_sim")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(split_region())
            .configure_grid(|grid| grid.imitates(ImitatedGrid::OpenSim))
            .needs(Need::GridControl)
            .run(the_brush_warns_on_a_strangers_parcel)?;
        Ok(())
    }

    /// **A brush on Second Life's own land**: on aditi the avatar is a
    /// resident standing on land that is neither its own nor its region
    /// owner's, so a revert pressed beside it warns `ParcelNoTerraforming`
    /// naming the parcel there — where the grid itself would say nothing.
    #[test]
    fn a_brush_beside_a_resident_on_second_life_warns() -> Result<(), TestError> {
        StageBuilder::new("terraform_parcel_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::SecondLife("a resident on land not its own"))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                arrived(alpha).await?;
                // A live region is up before the avatar stands in it.
                let _placed = alpha
                    .expect_state(Probe::Agent)
                    .at("/position")
                    .timeout(WAIT)
                    .to_be_present()
                    .await?;
                let readout = alpha.agent().await?;
                let spot = readout.position.ok_or("Alpha has no position")?;
                let eye = readout.camera_eye.ok_or("Alpha has no camera")?;
                let (ahead_x, ahead_y) = (spot[0] - eye[0], spot[1] - eye[1]);
                let length = ahead_x.hypot(ahead_y).max(0.001);
                let (ahead_x, ahead_y) = (ahead_x / length, ahead_y / length);
                // Ahead and to the right, as the ground-beside stage aims: the
                // avatar's own body hides the ground straight ahead of it, and
                // the Build window sits on the left.
                let east = (spot[0] + BESIDE * ahead_x + BESIDE * ahead_y).clamp(1.0, 254.0);
                let north = (spot[1] + BESIDE * ahead_y - BESIDE * ahead_x).clamp(1.0, 254.0);
                pick_the_revert_brush(alpha).await?;
                let ground = alpha
                    .world()
                    .ground(stage.home_region(), east, north)
                    .timeout(WAIT);
                let _rested = ground.hover().await?;
                let text = press_until_refused(alpha, &ground).await?;
                tracing::info!("the brush beside Alpha was refused: {text}");
                Ok(())
            })?;
        Ok(())
    }
}
