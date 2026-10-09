//! End-to-end tests for the objects a viewer ends up with out of what a grid
//! streams at it, on the fake grid and on a live one, through the automation
//! driver alone ([[gridspec-object-update-stream]]).
//!
//! The two live grids tell a viewer about a region's objects in different
//! messages and different amounts — compressed updates, cache probes or
//! neither depending on what the viewer's handshake reply said, everything
//! or what is in range — and the fake grid in full updates. Whichever it is,
//! the viewer's world has to hold objects once the arrival is over, and go
//! on holding them; it has to have been sent its own appearance; and where
//! the grid takes objects away for being out of range, as Second Life does
//! by naming a linkset's root alone, they have to go whole and come back
//! whole.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{LogStream, Probe, WorldKind, WorldLocator, WorldNode};
    use sl_e2e::{BodyError, Grid, Stage, StageBuilder};
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// How long the objects may take to stop coming or going. Second Life
    /// streamed a region of a thousand for six seconds, and its neighbours'
    /// for longer.
    const SETTLE: Duration = Duration::from_secs(120);

    /// How far apart the world is counted while it settles.
    const COUNT_EVERY: Duration = Duration::from_secs(3);

    /// How many counts in a row have to agree for the world to have settled:
    /// nine seconds without a change.
    const STEADY_COUNTS: usize = 4;

    /// How long the viewer's own avatar may take to be given its baked
    /// appearance. Where the viewer bakes, that is a bake and an upload.
    const BAKED: Duration = Duration::from_secs(120);

    /// The edge of a region, metres: a position is told in the agent's
    /// region's frame, so a neighbour's object reads outside `0..256`.
    const REGION_EDGE: f32 = 256.0;

    /// The viewer's draw distance setting, in metres.
    const DRAW_DISTANCE: &str = "RenderFarClip";

    /// The draw distance the range test starts from and returns to.
    const FAR_M: f64 = 256.0;

    /// The draw distance it takes the viewer down to.
    const NEAR_M: f64 = 32.0;

    /// How long after a change of draw distance the world is left alone
    /// before it is counted. Second Life's first kills came 0.4 s after the
    /// change and its first objects back 0.4 s after the next.
    const CHANGE: Duration = Duration::from_secs(6);

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

    /// Whether `node` is in the agent's own region. A neighbour's objects
    /// come and go with the neighbour, which a draw distance also decides.
    fn in_own_region(node: &WorldNode) -> bool {
        node.position.is_some_and(|[east, north, _up]| {
            (0.0..REGION_EDGE).contains(&east) && (0.0..REGION_EDGE).contains(&north)
        })
    }

    /// The objects of the agent's own region in `alpha`'s world once their
    /// number has held for [`STEADY_COUNTS`] counts — more than none of them.
    async fn settled_objects(alpha: &Viewer) -> Result<Vec<WorldNode>, BodyError> {
        let objects = alpha.world().locator(WorldLocator::kind(WorldKind::Object));
        let mut counts: Vec<usize> = Vec::new();
        let settled = tokio::time::timeout(SETTLE, async {
            loop {
                let mut nodes = objects.nodes().await?;
                nodes.retain(in_own_region);
                counts.push(nodes.len());
                let steady = counts.len() >= STEADY_COUNTS
                    && counts
                        .iter()
                        .rev()
                        .take(STEADY_COUNTS)
                        .all(|count| Some(count) == counts.last() && *count > 0);
                if steady {
                    return Ok::<Vec<WorldNode>, BodyError>(nodes);
                }
                tokio::time::sleep(COUNT_EVERY).await;
            }
        })
        .await;
        match settled {
            Ok(result) => result,
            Err(_elapsed) => Err(format!(
                "the viewer's world never settled on a number of objects above none: it counted \
                 {counts:?}"
            )
            .into()),
        }
    }

    /// How many of `objects` hang off something the viewer's world does not
    /// hold: a child prim whose root is gone.
    async fn rootless(alpha: &Viewer, objects: &[WorldNode]) -> Result<usize, BodyError> {
        let everything = alpha
            .world()
            .locator(WorldLocator::default())
            .nodes()
            .await?;
        let held: std::collections::BTreeSet<u32> =
            everything.iter().filter_map(|node| node.local_id).collect();
        Ok(objects
            .iter()
            .filter(|node| node.parent.is_some_and(|parent| !held.contains(&parent)))
            .count())
    }

    /// The viewer's world holds objects once the arrival is over, and the
    /// viewer was sent an appearance of its own avatar: it tells every grid
    /// it understands one, and Second Life sends none to a viewer that does
    /// not say so.
    async fn the_world_holds_objects(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let _objects = settled_objects(alpha).await?;
        // The avatar's node holds the bakes its latest appearance named: none
        // until an appearance about it has come that names some. OpenSim's
        // names none for an avatar nobody has baked yet; there the viewer
        // bakes and says what it published.
        let me = alpha.world().me();
        let baked = tokio::time::timeout(BAKED, async {
            loop {
                // Where the grid bakes, the appearance it sends names the
                // bakes; where the viewer does, what it published does.
                if !me.node().await?.bakes.is_empty()
                    || !alpha.agent().await?.published_bakes.is_empty()
                {
                    return Ok::<(), BodyError>(());
                }
                tokio::time::sleep(COUNT_EVERY).await;
            }
        })
        .await;
        match baked {
            Ok(result) => result,
            Err(_elapsed) => {
                let log = alpha.read_log(0, &[LogStream::Event]).await?;
                let appearances = log
                    .entries
                    .iter()
                    .filter(|entry| entry.kind == "AvatarAppearance")
                    .count();
                Err(format!(
                    "the viewer's own avatar was given no baked appearance in {BAKED:?}; the \
                     session saw {appearances} AvatarAppearance messages in all"
                )
                .into())
            }
        }
    }

    /// Take the draw distance down and bring it back, through the viewer's
    /// own setting, and hold the world to what the grid does about it.
    async fn range_takes_linksets_whole(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        // The viewer may have arrived at another distance than this: give
        // the grid time to start acting on the change before counting.
        let _far = alpha.set_setting(DRAW_DISTANCE, json!(FAR_M)).await?;
        tokio::time::sleep(CHANGE).await;
        let before = settled_objects(alpha).await?;

        let set = alpha.set_setting(DRAW_DISTANCE, json!(NEAR_M)).await?;
        if set != json!(NEAR_M) {
            return Err(
                format!("the draw distance reads {set} after it was set to {NEAR_M}").into(),
            );
        }
        tokio::time::sleep(CHANGE).await;
        let near = settled_objects(alpha).await?;
        let adrift = rootless(alpha, &near).await?;
        if adrift > 0 {
            return Err(format!(
                "{adrift} of the {} objects left at {NEAR_M} m hang off a root the viewer no \
                 longer holds",
                near.len()
            )
            .into());
        }
        match stage.on_grid() {
            // Second Life takes away what is out of range.
            Grid::Aditi => {
                if near.len() >= before.len() {
                    return Err(format!(
                        "the viewer held {} objects at {FAR_M} m and {} at {NEAR_M} m: nothing \
                         left",
                        before.len(),
                        near.len()
                    )
                    .into());
                }
            }
            // OpenSim, and the fake grid whichever it imitates, send a
            // region's objects whatever the draw distance.
            Grid::OpenSim | Grid::Fake => {
                if near.len() != before.len() {
                    return Err(format!(
                        "the viewer held {} objects at {FAR_M} m and {} at {NEAR_M} m on a grid \
                         that does not cull",
                        before.len(),
                        near.len()
                    )
                    .into());
                }
            }
        }

        let _far = alpha.set_setting(DRAW_DISTANCE, json!(FAR_M)).await?;
        tokio::time::sleep(CHANGE).await;
        let after = settled_objects(alpha).await?;
        let adrift = rootless(alpha, &after).await?;
        // A live region's content moves a little on its own; what went has
        // to be back, to within a handful.
        let floor = before.len().saturating_sub(before.len().div_ceil(20));
        if adrift > 0 || after.len() < floor {
            return Err(format!(
                "the viewer held {} objects before, {} at {NEAR_M} m and {} back at {FAR_M} m, \
                 {adrift} of them without their root",
                before.len(),
                near.len(),
                after.len()
            )
            .into());
        }
        Ok(())
    }

    /// **Out of range and back**: on Second Life the objects beyond a 32 m
    /// draw distance leave the viewer's world and none is left hanging off a
    /// root that went; back at 256 m they are there again. On a grid that does
    /// not cull, the same moves change nothing.
    #[test]
    fn objects_out_of_range_leave_whole_and_come_back() -> Result<(), TestError> {
        StageBuilder::new("objects_range")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .run(range_takes_linksets_whole)?;
        Ok(())
    }

    /// **Objects from whichever grid this is**: on a live grid
    /// (`SL_E2E_GRID=opensim|aditi`) that is the grid's own stream — compressed
    /// updates, sent at once because the viewer says its cache is empty.
    #[test]
    fn the_viewers_world_holds_the_regions_objects() -> Result<(), TestError> {
        StageBuilder::new("objects_arrival")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .run(the_world_holds_objects)?;
        Ok(())
    }

    /// **Objects from each fake flavour**, which differ in whether the agent
    /// is sent its own appearance for the handshake flags it gave and in
    /// nothing about objects yet.
    #[test]
    fn each_fake_flavours_objects_arrive() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "objects_second_life"),
            (ImitatedGrid::OpenSim, "objects_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .region(RegionConfig::default())
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(the_world_holds_objects)?;
        }
        Ok(())
    }
}
