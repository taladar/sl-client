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
//!
//! And what a viewer shows of an object's *record* — its name, its owner, its
//! permissions — it has from the answer to a select
//! ([[gridspec-object-properties]]). Both grids answer one; Second Life
//! answers a rename as well and OpenSim does not; neither answers a select of
//! something the region does not hold. The Build window has to fill from the
//! answer, keep a name it wrote whether or not the grid said so, and leave
//! the record's fields shut over an answer that never comes.
//!
//! And an object comes and goes by the resident's own hand
//! ([[gridspec-object-rez-derez]]): taken into the inventory, dragged out of
//! it again, deleted. Both grids announce the item a take makes with the
//! legacy message, but say the object is gone differently — Second Life in
//! one kill naming every prim, OpenSim in two naming the root — and stamp
//! the item with different permission masks. Whichever it is, the object has
//! to leave the viewer's world, its item has to show in the inventory window
//! under the object's name, and a rez of that item has to put an object of
//! that name back.
//!
//! And a grid does not take every edit as it was sent
//! ([[gridspec-object-edit]]): a prim's side is kept within limits that are
//! Second Life's or OpenSim's, a move under the ground or off the region's
//! edge is refused by one and not the other, and an object sent out of the
//! world comes back to its owner's inventory on Second Life. The viewer
//! shows a move and a resize the moment they are typed, so what it shows
//! afterwards has to be what the grid kept and not what was typed.
//!
//! And a link is the grid's to refuse ([[gridspec-object-link-delink]]):
//! Second Life links nothing of a selection whose prims stand more than
//! 64 m apart and says so in an alert no viewer has a template for; OpenSim
//! links at any distance. The viewer has to show the one as the grid worded
//! it and leave the prims free, and draw the other as one linkset.
//!
//! And a prim's contents are the grid's to list ([[gridspec-task-inventory]]):
//! over a capability on Second Life and over UDP on OpenSim, under names the
//! grid chooses — a second script is `New Script 1` on both — and with
//! nothing at all said of a write the grid does not take. The Content tab
//! has to show a new script under the grid's name for it, lose a row the
//! grid removed, and stop saying a change is on its way when no listing ever
//! comes back.
//!
//! And a script that faults says so on `DEBUG_CHANNEL`
//! ([[gridspec-lsl-runtime-errors]]), in each grid's own words: on a live
//! grid, a script saved from the editor that divides by zero has to open the
//! Script Errors window on them.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{
        Locator, LogStream, NodeValue, Probe, Role, WorldKind, WorldLocator, WorldNode,
    };
    use sl_e2e::{BodyError, Grid, Need, Stage, StageBuilder};
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_proto::{RegionLocalObjectId, ServerEvent};
    use sl_viewer_driver::UiLocator;
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

    /// How many spots of the ground a live placement tries.
    const GROUND_SPOTS: u64 = 12;

    /// [`GROUND_SPOTS`], as the index of a spot.
    const GROUND_SPOTS_U8: u8 = 12;

    /// How long one spot is given: the camera comes round to it in a few
    /// seconds where a click reaches it at all.
    const GROUND_SPOT_WAIT: Duration = Duration::from_secs(20);

    /// The name a prim nobody has named has on both grids.
    const UNNAMED: &str = "Object";

    /// What the name of every prim these tests name on a live grid ends in,
    /// so that a later run can tell what an earlier one left standing from
    /// anything else its avatar owns there.
    const MARK: &str = "5d0c1b7e9a4f4c62b8e3f1a27c94d6e0";

    /// The longest name a prim keeps, in bytes, on both grids.
    const NAME_BYTES: usize = 63;

    /// How far from the avatar a leftover is looked for, in metres: past
    /// every spot a placement tries.
    const LEFTOVER_RANGE_M: f32 = 12.0;

    /// The most prims a sweep selects to read their names.
    const LEFTOVERS_READ: usize = 16;

    /// The name a test gives its prim: `label` on the fake grid, and on a
    /// live one `label`, the time and [`MARK`] — a name of this run's own,
    /// where an inventory keeps the items of the runs before it.
    fn run_name(stage: &Stage, label: &str) -> Result<String, BodyError> {
        if stage.on_grid() == Grid::Fake {
            return Ok(label.to_owned());
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("the clock: {error}"))?
            .as_secs();
        let name = format!("{label} {stamp} {MARK}");
        if name.len() > NAME_BYTES {
            return Err(format!("{name:?} is longer than a grid keeps a name").into());
        }
        Ok(name)
    }

    /// Whether `node` could be a prim one of these tests left: a root of the
    /// default half-metre size, within [`LEFTOVER_RANGE_M`] of `at`.
    fn could_be_a_leftover(node: &WorldNode, at: [f32; 3]) -> bool {
        let default_size = node
            .scale
            .is_some_and(|scale| scale.iter().all(|side| (side - 0.5).abs() < 0.01));
        let near = node.position.is_some_and(|position| {
            (position[0] - at[0]).hypot(position[1] - at[1]) < LEFTOVER_RANGE_M
        });
        node.parent.is_none() && default_size && near
    }

    /// On a live grid, delete what earlier runs of these tests left standing
    /// round the avatar: a run that fails half-way leaves its prim where it
    /// aimed, and the next to aim there finds a cube on its ground.
    ///
    /// Only a prim whose **name** says so is deleted: one that ends in
    /// [`MARK`], or a default cube still called "Object" — what a run leaves
    /// that died between its rez and its rename. The avatar may be standing
    /// among prims it owns for other reasons, so nothing goes for being
    /// owned, near or small alone; those only decide which prims are
    /// selected to have their names read.
    async fn sweep_leftovers(stage: &Stage, alpha: &Viewer) -> Result<(), BodyError> {
        if stage.on_grid() == Grid::Fake {
            return Ok(());
        }
        let readout = alpha.agent().await?;
        let (Some(me), Some(at)) = (readout.agent_id, readout.position) else {
            return Ok(());
        };
        let mut candidates = alpha
            .world()
            .locator(WorldLocator::kind(WorldKind::Object).owned_by(me))
            .nodes()
            .await?;
        candidates.retain(|node| could_be_a_leftover(node, at));
        candidates.truncate(LEFTOVERS_READ);
        if candidates.is_empty() {
            return Ok(());
        }
        let build = alpha.ui().window("build-tools");
        if !build.is_visible().await? {
            alpha.press("Ctrl+B").await?;
        }
        let _open = alpha.expect(&build).timeout(WAIT).to_be_visible().await?;
        let _move = build
            .get(Locator::role(Role::Radio).name_key("build-tool-move"))
            .click()
            .await?;
        let field = build.test_id("build-name:field");
        for node in candidates {
            let prim = alpha
                .world()
                .locator(WorldLocator::full_id(node.full_id))
                .timeout(GROUND_SPOT_WAIT);
            // One no click reaches is left where it is: it is no more in a
            // placement's way than it is in this one's.
            if prim.select().await.is_err() {
                continue;
            }
            let _general = build
                .get(Locator::role(Role::Tab).name_key("build-tab-general"))
                .click()
                .await?;
            if alpha
                .expect(&field)
                .timeout(GROUND_SPOT_WAIT)
                .to_be_enabled()
                .await
                .is_err()
            {
                continue;
            }
            let Some(NodeValue::Text(name)) = field.value().await? else {
                continue;
            };
            if !(name.ends_with(MARK) || name == UNNAMED) {
                continue;
            }
            // By its pie, which asks nothing of where the keyboard is.
            pie(alpha, &prim, &DELETE).await?;
            let _gone = alpha
                .expect_world(&prim)
                .timeout(WAIT)
                .to_be_detached()
                .await?;
        }
        alpha.press("Escape").await?;
        if build.is_visible().await? {
            alpha.press("Ctrl+B").await?;
        }
        let _closed = alpha.expect(&build).timeout(WAIT).to_be_hidden().await?;
        Ok(())
    }

    /// The name the record test gives its prim.
    const WRITTEN: &str = "Named By The Viewer";

    /// Open the Build window, rez a prim with its Create tool and show the
    /// General tab: on the stock scene's unnamed box on the fake grid, on the
    /// ground beside the avatar on a live one. Returns the window and, on a
    /// live grid, the spot on the ground.
    async fn rez_a_prim(
        stage: &Stage,
        alpha: &Viewer,
    ) -> Result<(UiLocator, Option<(f32, f32)>), BodyError> {
        sweep_leftovers(stage, alpha).await?;
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window("build-tools");
        let spot = place_a_prim(stage, alpha, &build, None).await?;
        Ok((build, spot))
    }

    /// With the Build window open, rez a prim with the Create tool — on the
    /// object `on`; with none, on the stock scene's unnamed box on the fake
    /// grid and on the ground beside the avatar on a live one — and leave
    /// the window on its General tab. Returns the spot on the ground, where
    /// it was the ground.
    async fn place_a_prim(
        stage: &Stage,
        alpha: &Viewer,
        build: &UiLocator,
        on: Option<sl_proto::Uuid>,
    ) -> Result<Option<(f32, f32)>, BodyError> {
        let _create = build
            .get(Locator::role(Role::Radio).name_key("build-tool-create"))
            .click()
            .await?;
        let spot = if let Some(on) = on {
            let _placed = alpha
                .world()
                .locator(WorldLocator::full_id(on))
                .timeout(SETTLE)
                .place()
                .await?;
            None
        } else if stage.on_grid() == Grid::Fake {
            let _placed = alpha.world().object_named(UNNAMED).place().await?;
            None
        } else {
            // On the ground a few metres to the camera's right of the avatar
            // and a little ahead of it: the Build window takes the left of
            // the screen. Which of a dozen spots comes first moves with the
            // run, and a spot no click reaches gives way to the next: a run
            // that failed half-way leaves its prim standing where it aimed,
            // and the run after it that aimed there too used to wait two
            // minutes for ground a cube covered.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| format!("the clock: {error}"))?
                .as_secs();
            let first = u8::try_from(stamp % GROUND_SPOTS).unwrap_or(0);
            let readout = alpha.agent().await?;
            let spot = readout.position.ok_or("Alpha has no position")?;
            let eye = readout.camera_eye.ok_or("Alpha has no camera")?;
            let (ahead_x, ahead_y) = (spot[0] - eye[0], spot[1] - eye[1]);
            let length = ahead_x.hypot(ahead_y).max(0.001);
            let (ahead_x, ahead_y) = (ahead_x / length, ahead_y / length);
            let mut placed = None;
            let mut refused = None;
            for turn in 0..GROUND_SPOTS_U8 {
                let index = first.wrapping_add(turn) % GROUND_SPOTS_U8;
                // Six across, in two rows.
                let aside = 3.0 + f32::from(index % 6);
                let ahead = 3.0 + 2.0 * f32::from(index / 6);
                let (east, north) = (
                    spot[0] + aside * ahead_y + ahead * ahead_x,
                    spot[1] - aside * ahead_x + ahead * ahead_y,
                );
                match alpha
                    .world()
                    .ground(stage.home_region(), east, north)
                    .timeout(GROUND_SPOT_WAIT)
                    .place()
                    .await
                {
                    Ok(_placed) => {
                        placed = Some((east, north));
                        break;
                    }
                    Err(error) => refused = Some(error),
                }
            }
            match (placed, refused) {
                (Some(at), _) => Some(at),
                (None, Some(error)) => return Err(error.into()),
                (None, None) => return Err("no spot of the ground was tried".into()),
            }
        };
        let _general = build
            .get(Locator::role(Role::Tab).name_key("build-tab-general"))
            .click()
            .await?;
        Ok(spot)
    }

    /// Rez a prim, read its record off the Build window's General tab, rename
    /// it and read the name back from a fresh selection. On a live grid the
    /// prim is then deleted; on the fake one it is selected once more after
    /// the grid has lost it without a word.
    async fn the_build_window_follows_the_record(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let (build, _spot) = rez_a_prim(stage, alpha).await?;
        let fake = stage.on_grid() == Grid::Fake;

        // The rez leaves the prim selected, and the select's answer names it.
        let field = build.test_id("build-name:field");
        let _named = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(UNNAMED)
            .await?;
        let _live = alpha.expect(&field).timeout(WAIT).to_be_enabled().await?;
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let (prim_id, local_id) = (prim.full_id, prim.local_id);

        // A rename. Second Life answers it with the record and OpenSim with
        // nothing; the field reads what was written either way.
        let heard = if fake {
            Some(stage.agent("Alpha").await?.events())
        } else {
            None
        };
        let _typed = field.fill(WRITTEN).await?;
        field.press("Enter").await?;
        if let Some(mut heard) = heard {
            let renamed = tokio::time::timeout(WAIT, async {
                loop {
                    match heard.recv().await {
                        Ok(ServerEvent::ObjectNameSet { .. }) => return Ok(()),
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
            })
            .await;
            match renamed {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(format!("the grid's event stream: {error}").into()),
                Err(_elapsed) => return Err("the grid never heard the rename".into()),
            }
        } else {
            // No grid handle to hear it with: give the grid the time an
            // answer to it was measured to take, several times over.
            tokio::time::sleep(UNANSWERED).await;
        }
        let _kept = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(WRITTEN)
            .await?;

        // A fresh selection reads the record again: Escape takes the focus
        // out of the field, a second Escape the selection.
        let this_prim = || alpha.world().locator(WorldLocator::full_id(prim_id));
        let deselect = async || -> Result<(), BodyError> {
            alpha.press("Escape").await?;
            alpha.press("Escape").await?;
            let _cleared = alpha
                .expect_state(Probe::Selection)
                .to_equal(json!([]))
                .await?;
            Ok(())
        };
        deselect().await?;
        let _blank = alpha.expect(&field).timeout(WAIT).to_be_disabled().await?;
        let _reselected = this_prim().select().await?;
        let _reread = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(WRITTEN)
            .await?;
        let _live = alpha.expect(&field).timeout(WAIT).to_be_enabled().await?;

        if !fake {
            // Nothing to take the prim away behind the viewer's back with. The
            // click that selected it left the world the keyboard, and Delete
            // takes what is selected.
            alpha.press("Delete").await?;
            let _gone = alpha
                .expect_world(&this_prim())
                .timeout(SETTLE)
                .to_be_detached()
                .await?;
            return Ok(());
        }

        // The grid loses the prim and tells nobody: the viewer still holds
        // it, selects it, and is not answered. The field stays shut, with no
        // name in it that a commit could write.
        deselect().await?;
        stage
            .agent("Alpha")
            .await?
            .with_world(|world, _sim| {
                world
                    .objects
                    .retain(|object| object.local_id != RegionLocalObjectId(local_id));
            })
            .await;
        let _selected = this_prim().select().await?;
        let _picked = alpha
            .expect_state(Probe::Selection)
            .at("/0/local_id")
            .timeout(WAIT)
            .to_equal(json!(local_id))
            .await?;
        tokio::time::sleep(UNANSWERED).await;
        let _shut = alpha.expect(&field).to_be_disabled().await?;
        let _empty = alpha.expect(&field).to_have_text("").await?;
        Ok(())
    }

    /// How long a select that gets no answer is given before the window is
    /// read: both live grids answered within half a second.
    const UNANSWERED: Duration = Duration::from_secs(3);

    /// **The Build window and the record**, on each fake flavour: the General
    /// tab names a freshly rezzed prim from the select's answer and only then
    /// lets its name be edited; a rename shows whether the grid answers it
    /// (Second Life) or not (OpenSim), and is what a fresh selection reads
    /// back; and a select the grid does not answer leaves the name blank and
    /// shut rather than editable.
    #[test]
    fn the_build_window_follows_the_record_on_each_fake_flavour() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "record_second_life"),
            (ImitatedGrid::OpenSim, "record_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's unnamed box, to rez on"))
                .needs(Need::GridControl)
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(the_build_window_follows_the_record)?;
        }
        Ok(())
    }

    /// **The Build window and the record, live** (`SL_E2E_GRID=opensim|aditi`,
    /// where the avatar stands on land it may build on): the same rez, record,
    /// rename and fresh read against the grid's own answers — Second Life's,
    /// which come a third of a second after the select and after every
    /// rename, and OpenSim's, which answers the select and not the rename.
    #[test]
    fn the_build_window_follows_a_live_grids_record() -> Result<(), TestError> {
        StageBuilder::new("record_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid(
                "a grid's own answer to a select and to a rename",
            ))
            .run(the_build_window_follows_the_record)?;
        Ok(())
    }

    /// **The Texture tab's PBR mode on each fake flavour**: the Second-Life
    /// flavour grants `ModifyMaterialParams` and the tab is live; the OpenSim
    /// flavour grants none, as a stock OpenSim grants none
    /// (`book/src/gridspec/materials.md`), and the tab is greyed rather than
    /// left to send edits nothing takes.
    #[test]
    fn the_pbr_mode_is_offered_where_the_grid_takes_it_on_each_fake_flavour()
    -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "pbr_mode_second_life"),
            (ImitatedGrid::OpenSim, "pbr_mode_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's box, to select"))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async move |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    arrived(alpha).await?;
                    alpha.press("Ctrl+B").await?;
                    let build = alpha.ui().window("build-tools");
                    let _open = alpha.expect(&build).timeout(WAIT).to_be_visible().await?;
                    let _moving = build
                        .get(Locator::role(Role::Radio).name_key("build-tool-move"))
                        .click()
                        .await?;
                    let _selected = alpha
                        .world()
                        .object_named(UNNAMED)
                        .timeout(WAIT)
                        .select()
                        .await?;
                    let _texture = build
                        .get(Locator::role(Role::Tab).name_key("build-tab-texture"))
                        .click()
                        .await?;
                    let pbr =
                        build.get(Locator::role(Role::Tab).name_key("build-tex-matmedia-pbr"));
                    let offered = alpha.expect(&pbr).timeout(WAIT);
                    let _seen = match flavour {
                        ImitatedGrid::SecondLife => offered.to_be_enabled().await?,
                        ImitatedGrid::OpenSim => offered.to_be_disabled().await?,
                    };
                    Ok(())
                })?;
        }
        Ok(())
    }

    /// The name the take test gives its prim.
    const TAKEN: &str = "Taken By The Viewer";

    /// A search no inventory item answers to.
    const NO_SUCH_ITEM: &str = "no item is called this";

    /// The slices of an object's pie that take it: the Take sub-pie, then
    /// Take.
    const TAKE: [&str; 2] = ["pie-object-take", "pie-object-take"];

    /// The slices that delete it: More, then Delete.
    const DELETE: [&str; 2] = ["pie-object-more", "pie-object-delete"];

    /// Open the pie of the one object `object` names and follow `slices`
    /// through it.
    async fn pie(
        alpha: &Viewer,
        object: &sl_viewer_driver::WorldHandle,
        slices: &[&str],
    ) -> Result<(), BodyError> {
        let _pie = object.open_pie().await?;
        for key in slices {
            let _slice = alpha
                .pie_slice(Locator::role(Role::MenuItem).name_key(*key))
                .await?;
        }
        Ok(())
    }

    /// Rez a prim and name it, take it from its pie, find its item in the
    /// inventory window, drag that back into the world and delete what
    /// appears.
    async fn a_prim_is_taken_and_rezzed_again(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let (build, spot) = rez_a_prim(stage, alpha).await?;
        let name = run_name(stage, TAKEN)?;
        let field = build.test_id("build-name:field");
        let _live = alpha.expect(&field).timeout(WAIT).to_be_enabled().await?;
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let prim_id = prim.full_id;
        let _typed = field.fill(&name).await?;
        field.press("Enter").await?;
        let _kept = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(&name)
            .await?;
        // Out of the field, out of the selection, out of the Build window:
        // a pie is a right click outside build mode.
        alpha.press("Escape").await?;
        alpha.press("Escape").await?;
        let _cleared = alpha
            .expect_state(Probe::Selection)
            .to_equal(json!([]))
            .await?;
        alpha.press("Ctrl+B").await?;
        let _closed = alpha.expect(&build).to_be_hidden().await?;

        // Taken: the object goes, however the grid says so, and its item
        // comes, under the object's name.
        let rezzed = alpha
            .world()
            .locator(WorldLocator::full_id(prim_id))
            .timeout(WAIT);
        pie(alpha, &rezzed, &TAKE).await?;
        let _gone = alpha
            .expect_world(&rezzed)
            .timeout(SETTLE)
            .to_be_detached()
            .await?;
        let inventory = alpha.ui().window("inventory");
        if !inventory.is_visible().await? {
            alpha.press("Ctrl+I").await?;
            let _shown = alpha.expect(&inventory).to_be_visible().await?;
        }
        // The list narrows to a search a moment after it is typed, and a row
        // found before that is not where a drag will find it. So the item is
        // searched away first: it can only show again once the list has
        // narrowed to its name, which is where it stays.
        let search = inventory.test_id("inventory:search").role(Role::Textbox);
        let named = inventory.get(Locator::role(Role::TreeItem).named(&name));
        let _searched = search.fill(NO_SUCH_ITEM).await?;
        let _narrowed = alpha.expect(&named).timeout(WAIT).to_be_hidden().await?;
        let _searched = search.fill(&name).await?;
        let row = inventory
            .get(Locator::role(Role::TreeItem).named(&name))
            .timeout(WAIT);
        let _filed = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;

        // Rezzed again: an object of that name stands in the world.
        match spot {
            Some((east, north)) => {
                let _dropped = alpha
                    .world()
                    .ground(stage.home_region(), east, north)
                    .timeout(SETTLE)
                    .drop_from(&row)
                    .await?;
            }
            None => {
                let _dropped = alpha.world().object_named(UNNAMED).drop_from(&row).await?;
            }
        }
        let again = alpha.world().object_named(&name).timeout(SETTLE);
        let _back = alpha.expect_world(&again).to_be_attached().await?;
        // The item is a copy's and stays where it was.
        let _still = alpha.expect(&row).to_be_visible().await?;

        // Deleted: gone again.
        pie(alpha, &again, &DELETE).await?;
        let _deleted = alpha
            .expect_world(&again)
            .timeout(SETTLE)
            .to_be_detached()
            .await?;
        Ok(())
    }

    /// **Take, rez and delete on each fake flavour**: the object leaves the
    /// viewer's world on Second Life's one kill and on OpenSim's two, its
    /// item shows in the inventory window with either grid's masks, a drag
    /// out of the window puts an object of the same name back, and Delete
    /// takes that away.
    #[test]
    fn a_prim_is_taken_and_rezzed_again_on_each_fake_flavour() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "take_second_life"),
            (ImitatedGrid::OpenSim, "take_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's unnamed box, to rez on"))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(a_prim_is_taken_and_rezzed_again)?;
        }
        Ok(())
    }

    /// **Take, rez and delete, live** (`SL_E2E_GRID=opensim|aditi`, where the
    /// avatar stands on land it may build on): the same against the grid's
    /// own kills and its own item. The item stays in the avatar's Objects
    /// folder, and a copy of it in its Trash.
    #[test]
    fn a_prim_is_taken_and_rezzed_again_on_a_live_grid() -> Result<(), TestError> {
        StageBuilder::new("take_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid("a grid's own kill and its own item"))
            .run(a_prim_is_taken_and_rezzed_again)?;
        Ok(())
    }

    /// The name the edit test gives its prim.
    const EDITED: &str = "Edited By A Viewer";

    /// A side no grid lets a prim have, in metres, as it is typed.
    const TOO_LARGE: &str = "1000";

    /// The side a rezzed prim has, as the Build window writes it.
    const REZZED_SIDE: &str = "0.500";

    /// The text of a numeric field of the Build window's Object tab, as a
    /// number.
    async fn reading(field: &UiLocator) -> Result<f32, BodyError> {
        match field.value().await? {
            Some(NodeValue::Number(number)) => Ok(number),
            Some(NodeValue::Text(text)) => text
                .parse::<f32>()
                .map_err(|error| format!("the field reads {text:?}: {error}").into()),
            other => Err(format!("the field holds {other:?}, not a number").into()),
        }
    }

    /// Wait until the numeric field `field` reads `wanted` to the centimetre.
    async fn reads(field: &UiLocator, wanted: f32, what: &str) -> Result<(), BodyError> {
        let waited = tokio::time::timeout(WAIT, async {
            loop {
                if let Ok(read) = reading(field).await
                    && (read - wanted).abs() < 0.01
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await;
        if waited.is_err() {
            let read = field.value().await?;
            return Err(format!("{what}: the field reads {read:?}, not {wanted}").into());
        }
        Ok(())
    }

    /// Wait until the object `prim` names is drawn as `wanted` says.
    async fn drawn(
        prim: &sl_viewer_driver::WorldHandle,
        wanted: impl Fn(&WorldNode) -> bool + Send + Sync,
        what: &str,
    ) -> Result<(), BodyError> {
        let waited = tokio::time::timeout(WAIT, async {
            loop {
                if let Ok(node) = prim.node().await
                    && wanted(&node)
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await;
        if waited.is_err() {
            let node = prim.node().await?;
            return Err(format!(
                "{what}: the prim is drawn at {:?}, {:?} large",
                node.position, node.scale
            )
            .into());
        }
        Ok(())
    }

    /// Rez a prim and name it; type a side no grid takes into the Build
    /// window and read back the grid's largest; type a place the grid refuses
    /// and read back where the prim stands; move it, and step back out of the
    /// move and forward again; and, where the grid sends an object out of the
    /// world back to its owner, type such a place and find the prim gone and
    /// its item in the inventory.
    async fn an_edit_reads_what_the_grid_kept(
        stage: &Stage,
        like: ImitatedGrid,
    ) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let (build, _spot) = rez_a_prim(stage, alpha).await?;
        let name = run_name(stage, EDITED)?;
        let field = build.test_id("build-name:field");
        let _live = alpha.expect(&field).timeout(WAIT).to_be_enabled().await?;
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let prim_id = prim.full_id;
        let _typed = field.fill(&name).await?;
        field.press("Enter").await?;
        let _kept = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(&name)
            .await?;
        let _object = build
            .get(Locator::role(Role::Tab).name_key("build-tab-object"))
            .click()
            .await?;
        let policy = like.edit_policy();
        let this_prim = || alpha.world().locator(WorldLocator::full_id(prim_id));

        // A side of a kilometre: the grid keeps its own largest, and says so
        // in the update it answers a resize with.
        let side = build.test_id("build-size-x:field");
        reads(&side, 0.5, "the side a prim is rezzed with").await?;
        let _typed = side.fill(TOO_LARGE).await?;
        side.press("Enter").await?;
        reads(&side, policy.largest_prim_m, "a side of a kilometre").await?;
        // The window shows a resize the moment it is typed, and OpenSim's
        // largest is the viewer's own: what says the grid kept it is the prim
        // as the grid sent it again.
        drawn(
            &this_prim(),
            |node| {
                node.scale
                    .is_some_and(|scale| (scale[0] - policy.largest_prim_m).abs() < 0.01)
            },
            "a side of a kilometre",
        )
        .await?;
        let _typed = side.fill(REZZED_SIDE).await?;
        side.press("Enter").await?;
        reads(&side, 0.5, "the side it was rezzed with").await?;

        // A place the grid refuses: under the ground on Second Life, off the
        // region's edge on OpenSim. The prim stays, and the grid sends it
        // again — which has to put it back in the window and in the world.
        let (east, up) = (
            build.test_id("build-pos-x:field"),
            build.test_id("build-pos-z:field"),
        );
        let stood = [reading(&east).await?, reading(&up).await?];
        let (refused, back, what) = match like {
            ImitatedGrid::SecondLife => (&up, stood[1], "a move under the ground"),
            ImitatedGrid::OpenSim => (&east, stood[0], "a move off the region's edge"),
        };
        let _typed = refused
            .fill(match like {
                ImitatedGrid::SecondLife => "-50",
                ImitatedGrid::OpenSim => "-10",
            })
            .await?;
        refused.press("Enter").await?;
        reads(refused, back, what).await?;
        drawn(
            &this_prim(),
            |node| {
                node.position.is_some_and(|at| {
                    (at[0] - stood[0]).abs() < 0.05 && (at[2] - stood[1]).abs() < 0.05
                })
            },
            what,
        )
        .await?;

        // A move the grid takes, and a step back out of it: the undo is the
        // grid's, which sends the prim again where it stood before. A chord
        // typed into a text field is the field's, so the focus goes to a tab
        // first.
        let raised = stood[1] + 1.0;
        let _typed = up.fill(&format!("{raised:.3}")).await?;
        up.press("Enter").await?;
        reads(&up, raised, "a move a metre up").await?;
        let _tab = build
            .get(Locator::role(Role::Tab).name_key("build-tab-object"))
            .click()
            .await?;
        alpha.press("Ctrl+Z").await?;
        reads(&up, stood[1], "an undo of the move").await?;
        alpha.press("Ctrl+Y").await?;
        reads(&up, raised, "a redo of it").await?;

        if like == ImitatedGrid::OpenSim {
            // OpenSim keeps an object wherever else it is sent. The click
            // that selected the prim left the world the keyboard, and Delete
            // takes what is selected.
            alpha.press("Escape").await?;
            alpha.press("Delete").await?;
            let _gone = alpha
                .expect_world(&this_prim())
                .timeout(SETTLE)
                .to_be_detached()
                .await?;
            return Ok(());
        }

        // Off the region's edge, where no region is: Second Life sends the
        // prim back to its owner's Lost And Found.
        let _typed = east.fill("-10").await?;
        east.press("Enter").await?;
        let _gone = alpha
            .expect_world(&this_prim())
            .timeout(SETTLE)
            .to_be_detached()
            .await?;
        alpha.press("Escape").await?;
        alpha.press("Escape").await?;
        alpha.press("Ctrl+B").await?;
        let inventory = alpha.ui().window("inventory");
        if !inventory.is_visible().await? {
            alpha.press("Ctrl+I").await?;
            let _shown = alpha.expect(&inventory).to_be_visible().await?;
        }
        let search = inventory.test_id("inventory:search").role(Role::Textbox);
        let _searched = search.fill(&name).await?;
        let row = inventory
            .get(Locator::role(Role::TreeItem).named(&name))
            .timeout(WAIT);
        let _filed = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;
        Ok(())
    }

    /// **An edit a grid does not take as sent, on each fake flavour**: a side
    /// of a kilometre reads Second Life's 64 m or OpenSim's 256 m once the
    /// grid has answered; a move the grid refuses — under the ground on one,
    /// off the edge on the other — is put back in the Build window and in the
    /// world by the update the grid answers it with; Ctrl+Z puts a moved
    /// prim back where it stood and Ctrl+Y where it was moved to; and on
    /// Second Life a prim sent off the region's edge leaves the world and its
    /// item shows in the inventory window.
    #[test]
    fn an_edit_reads_what_the_grid_kept_on_each_fake_flavour() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "edit_second_life"),
            (ImitatedGrid::OpenSim, "edit_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's unnamed box, to rez on"))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async move |stage: &Stage| {
                    an_edit_reads_what_the_grid_kept(stage, flavour).await
                })?;
        }
        Ok(())
    }

    /// **An edit a grid does not take as sent, live**
    /// (`SL_E2E_GRID=opensim|aditi`, where the avatar stands on land it may
    /// build on): the same against the grid's own limits and its own
    /// refusals. On Second Life the prim ends in the avatar's Lost And Found.
    #[test]
    fn an_edit_reads_what_the_grid_kept_on_a_live_grid() -> Result<(), TestError> {
        StageBuilder::new("edit_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid("a grid's own limits and its own refusals"))
            .run(async |stage: &Stage| {
                let like = if stage.on_grid() == Grid::Aditi {
                    ImitatedGrid::SecondLife
                } else {
                    ImitatedGrid::OpenSim
                };
                an_edit_reads_what_the_grid_kept(stage, like).await
            })?;
        Ok(())
    }
    /// The names the link test gives its two prims.
    const LINKED: [&str; 2] = ["Link Root", "Link Far"];

    /// How far above the first the second prim is moved, in metres: further
    /// than Second Life links.
    const TOO_FAR_M: f32 = 100.0;

    /// What Second Life's refusal says, in the grid's own words.
    const TOO_FAR_APART: &str = "pieces are too far apart";

    /// Rez and name a prim with the Build window open, and return its id: a
    /// grid that does not answer a rename leaves the viewer's world without
    /// the name to find it by.
    async fn rez_named(
        stage: &Stage,
        alpha: &Viewer,
        build: &UiLocator,
        on: Option<sl_proto::Uuid>,
        name: &str,
    ) -> Result<sl_proto::Uuid, BodyError> {
        let _spot = place_a_prim(stage, alpha, build, on).await?;
        let name = run_name(stage, name)?;
        let field = build.test_id("build-name:field");
        let _live = alpha.expect(&field).timeout(WAIT).to_be_enabled().await?;
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let _typed = field.fill(&name).await?;
        field.press("Enter").await?;
        let _kept = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(&name)
            .await?;
        Ok(prim.full_id)
    }

    /// Rez two prims, move the second a hundred metres up, select both and
    /// link them with the Build menu's chord. Second Life refuses and the
    /// viewer shows the grid's words with both prims still free; OpenSim
    /// links them and the viewer draws one hanging off the other.
    async fn a_link_too_far_apart(stage: &Stage, like: ImitatedGrid) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        sweep_leftovers(stage, alpha).await?;
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window("build-tools");
        let [near, far] = LINKED;
        // A prim a grid leaves unnamed in the viewer's world is a second
        // "Object": the stock box is found by its id from here on.
        let stock = if stage.on_grid() == Grid::Fake {
            Some(alpha.world().object_named(UNNAMED).node().await?.full_id)
        } else {
            None
        };
        let near = rez_named(stage, alpha, &build, stock, near).await?;
        // The second stands on the first on a live grid: ground that takes
        // one click is not ground that takes two, with whatever grows there.
        let far = rez_named(stage, alpha, &build, stock.or(Some(near)), far).await?;
        let prim = |id: sl_proto::Uuid| alpha.world().locator(WorldLocator::full_id(id));

        // The second prim is the one selected: up it goes.
        let object_tab = build.get(Locator::role(Role::Tab).name_key("build-tab-object"));
        let _object = object_tab.click().await?;
        let up = build.test_id("build-pos-z:field");
        let raised = reading(&up).await? + TOO_FAR_M;
        let _typed = up.fill(&format!("{raised:.3}")).await?;
        up.press("Enter").await?;
        reads(&up, raised, "a move a hundred metres up").await?;

        // The first joins the selection and leads it, so it is the root a
        // link names. A chord typed into a text field is the field's.
        // The camera has to come round to it first, which on a live grid
        // takes longer than an action's own patience.
        let _added = prim(near).timeout(SETTLE).shift_select().await?;
        let selected = alpha.selection().await?;
        if selected.len() != 2 {
            return Err(format!("both prims are to be selected: {selected:?}").into());
        }
        let _tab = object_tab.click().await?;
        alpha.press("Ctrl+L").await?;

        let linked = async || -> Result<bool, BodyError> {
            let near = prim(near).node().await?;
            let far = prim(far).node().await?;
            Ok(near.parent.is_some() || far.parent.is_some())
        };
        match like {
            ImitatedGrid::SecondLife => {
                let told = tokio::time::timeout(WAIT, async {
                    loop {
                        if let Ok(shown) = alpha.notifications().await
                            && shown
                                .iter()
                                .any(|notification| notification.text.contains(TOO_FAR_APART))
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                })
                .await;
                if told.is_err() {
                    let shown = alpha.notifications().await?;
                    return Err(format!(
                        "the grid's refusal was never shown; the notifications are {shown:?}"
                    )
                    .into());
                }
                if linked().await? {
                    return Err("a link the grid refused is drawn as a linkset".into());
                }
            }
            ImitatedGrid::OpenSim => {
                let taken = tokio::time::timeout(WAIT, async {
                    loop {
                        if matches!(linked().await, Ok(true)) {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                })
                .await;
                if taken.is_err() {
                    return Err("a link a hundred metres long was never drawn".into());
                }
            }
        }

        // Each goes by its own pie — the first takes the second with it
        // where they are one linkset now. The Delete key would do for both
        // at once, but only while they are still selected, the world has
        // the keyboard and the inventory has shown the viewer a Trash — and
        // an Escape pressed first to be sure of the second undoes the
        // first.
        for id in [near, far] {
            let this = prim(id).timeout(SETTLE);
            if this.count().await? == 0 {
                continue;
            }
            pie(alpha, &this, &DELETE).await?;
            let _gone = alpha
                .expect_world(&this)
                .timeout(SETTLE)
                .to_be_detached()
                .await?;
        }
        Ok(())
    }

    /// **A link of prims a hundred metres apart, on each fake flavour**: the
    /// Second-Life flavour refuses it with `LinkFailed`, an alert the viewer
    /// has no template for and shows in the grid's own words, and the prims
    /// stay free; the OpenSim flavour links them.
    #[test]
    fn a_link_too_far_apart_on_each_fake_flavour() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "link_second_life"),
            (ImitatedGrid::OpenSim, "link_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's unnamed box, to rez on"))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async move |stage: &Stage| a_link_too_far_apart(stage, flavour).await)?;
        }
        Ok(())
    }

    /// **A link of prims a hundred metres apart, live**
    /// (`SL_E2E_GRID=opensim|aditi`, where the avatar stands on land it may
    /// build on): the same against the grid's own refusal, or its own link.
    #[test]
    fn a_link_too_far_apart_on_a_live_grid() -> Result<(), TestError> {
        StageBuilder::new("link_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid("a grid's own refusal of a link"))
            .run(async |stage: &Stage| {
                let like = if stage.on_grid() == Grid::Aditi {
                    ImitatedGrid::SecondLife
                } else {
                    ImitatedGrid::OpenSim
                };
                a_link_too_far_apart(stage, like).await
            })?;
        Ok(())
    }

    /// The name the contents test gives its prim.
    const HOLDER: &str = "Holds A Script";

    /// What both grids call a script made new in a prim, as the viewer asks.
    const NEW_SCRIPT: &str = "New Script";

    /// What both grids call the second of that name.
    const SECOND_SCRIPT: &str = "New Script 1";

    /// What a row whose item is being added says after its name
    /// (`build-content-state-adding`).
    const ADDING: &str = "adding";

    /// What a row whose item is being removed says after its name
    /// (`build-content-state-deleting`).
    const DELETING: &str = "deleting";

    /// How long a listing nobody answers is waited for before the viewer is
    /// held to having given up: the session drops the request after a
    /// minute.
    const UNANSWERED_LISTING: Duration = Duration::from_secs(100);

    /// Rez a prim, make two scripts in it from the Content tab and remove
    /// the second: the rows are the grid's listing, under the grid's names.
    /// On the fake grid the prim is then lost behind the viewer's back and a
    /// third script made, which nothing answers: the row that says it is
    /// being added goes, and the listing the grid last gave stays.
    async fn a_prims_contents_follow_the_grid(
        stage: &Stage,
        like: ImitatedGrid,
    ) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        sweep_leftovers(stage, alpha).await?;
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window("build-tools");
        let fake = stage.on_grid() == Grid::Fake;
        let stock = if fake {
            Some(alpha.world().object_named(UNNAMED).node().await?.full_id)
        } else {
            None
        };
        let prim_id = rez_named(stage, alpha, &build, stock, HOLDER).await?;
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let local_id = prim.local_id;
        let _content = build
            .get(Locator::role(Role::Tab).name_key("build-tab-content"))
            .click()
            .await?;
        // By what a row's name has in it: a row is called what its cells
        // say, and one whose change is on its way says so after its name.
        let row = |name: &str| build.get(Locator::role(Role::ListItem).name_containing(name));
        let adding = build.get(Locator::role(Role::ListItem).name_containing(ADDING));
        let deleting = build.get(Locator::role(Role::ListItem).name_containing(DELETING));
        let new_script = build.test_id("contents-button:build-content-new-script");

        // A script: its row settles under the name the grid lists it by.
        let _made = new_script.click().await?;
        let _listed = alpha
            .expect(&row(NEW_SCRIPT))
            .timeout(WAIT)
            .to_be_visible()
            .await?;
        // A second of the same name is numbered by the grid, not by the
        // viewer, which asked for `New Script` again.
        let _made = new_script.click().await?;
        let _numbered = alpha
            .expect(&row(SECOND_SCRIPT))
            .timeout(WAIT)
            .to_be_visible()
            .await?;
        let _settled = alpha.expect(&adding).timeout(WAIT).to_be_hidden().await?;

        // Removed: the row goes when the grid's listing no longer has it.
        let _picked = row(SECOND_SCRIPT).click().await?;
        let _removed = build
            .test_id("contents-button:build-content-remove")
            .click()
            .await?;
        let _gone = alpha
            .expect(&row(SECOND_SCRIPT))
            .timeout(WAIT)
            .to_be_hidden()
            .await?;
        let _settled = alpha.expect(&deleting).timeout(WAIT).to_be_hidden().await?;
        let _kept = alpha.expect(&row(NEW_SCRIPT)).to_be_visible().await?;

        let this_prim = alpha
            .world()
            .locator(WorldLocator::full_id(prim_id))
            .timeout(SETTLE);
        if !fake {
            // Nothing to lose the prim behind the viewer's back with.
            alpha.press("Escape").await?;
            alpha.press("Escape").await?;
            alpha.press("Ctrl+B").await?;
            pie(alpha, &this_prim, &DELETE).await?;
            let _deleted = alpha
                .expect_world(&this_prim)
                .timeout(SETTLE)
                .to_be_detached()
                .await?;
            return Ok(());
        }

        if like == ImitatedGrid::SecondLife {
            a_scripts_experience_survives_a_save(stage, alpha, &build, prim_id, local_id).await?;
        }

        // The grid loses the prim and tells nobody. A script made now is
        // answered by nothing — not the write, and not the listing asked
        // for after it, over either road.
        stage
            .agent("Alpha")
            .await?
            .with_world(|world, _sim| {
                world
                    .objects
                    .retain(|object| object.local_id != RegionLocalObjectId(local_id));
            })
            .await;
        let _made = new_script.click().await?;
        let _in_flight = alpha.expect(&adding).timeout(WAIT).to_be_visible().await?;
        let _given_up = alpha
            .expect(&adding)
            .timeout(UNANSWERED_LISTING)
            .to_be_hidden()
            .await?;
        let _still = alpha.expect(&row(NEW_SCRIPT)).to_be_visible().await?;
        let _no_second = alpha.expect(&row(SECOND_SCRIPT)).to_be_hidden().await?;
        Ok(())
    }

    /// The experience the fake grid is told the script is under.
    const EXPERIENCE: u128 = 0xE4BE_71E4_CE00_0000_0000_0000_0000_0001;

    /// With one script in the prim, the grid is told it was compiled under
    /// an experience; the script is opened from the Content tab, the editor
    /// says which experience, and its save names it again — a save that named
    /// none would take the script out of it.
    async fn a_scripts_experience_survives_a_save(
        stage: &Stage,
        alpha: &Viewer,
        build: &UiLocator,
        prim_id: sl_proto::Uuid,
        local_id: u32,
    ) -> Result<(), BodyError> {
        let experience = sl_proto::ExperienceKey::from(sl_proto::Uuid::from_u128(EXPERIENCE));
        let agent = stage.agent("Alpha").await?;
        let script = agent
            .with_world(|world, _sim| {
                let contents = world
                    .task_inventories
                    .get_mut(&RegionLocalObjectId(local_id))?;
                let script = contents.items.first()?.item_id;
                let _previous = contents.experiences.insert(script, experience);
                Some(script)
            })
            .await
            .ok_or("the grid holds no script in the prim")?;

        let row = build.get(Locator::role(Role::ListItem).name_containing(NEW_SCRIPT));
        let _opened = row.double_click().await?;
        let editor = alpha
            .ui()
            .window(&format!("script-editor#{prim_id}/{}", script.uuid()));
        let line = editor.get(Locator::role(Role::Text).name_containing(experience.to_string()));
        let _said = alpha.expect(&line).timeout(WAIT).to_be_visible().await?;

        let mut heard = agent.events();
        let _saved = editor
            .get(Locator::role(Role::Button).name_key("script-save"))
            .click()
            .await?;
        let named = tokio::time::timeout(WAIT, async {
            loop {
                match heard.recv().await {
                    Ok(ServerEvent::CapsAssetUploaded { metadata, .. }) => {
                        if let sl_proto::CapsUploadMetadata::UpdateScriptTask(request) = *metadata {
                            return Ok(request.experience);
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        })
        .await;
        match named {
            Ok(Ok(Some(named))) if named == experience => {}
            Ok(Ok(named)) => {
                return Err(format!("the save named the experience {named:?}").into());
            }
            Ok(Err(error)) => return Err(format!("the grid's event stream: {error}").into()),
            Err(_elapsed) => return Err("the grid never heard the save".into()),
        }
        let _closed = editor.test_id("floater-button:close").click().await?;
        let _gone = alpha.expect(&editor).timeout(WAIT).to_be_hidden().await?;
        Ok(())
    }

    /// **A prim's contents on each fake flavour**: listed over the
    /// capability on the Second-Life flavour and over UDP on the OpenSim one,
    /// a new script shows under the grid's name for it and a second as
    /// `New Script 1`, a removed one goes, and a script made in a prim the
    /// grid no longer has — a write and a listing nothing answers — leaves
    /// the Content tab showing what the grid last listed. On the Second-Life
    /// flavour, which has experiences, the script editor says which one a
    /// script is under and its save names it again.
    #[test]
    fn a_prims_contents_follow_the_grid_on_each_fake_flavour() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "contents_second_life"),
            (ImitatedGrid::OpenSim, "contents_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's unnamed box, to rez on"))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async move |stage: &Stage| {
                    a_prims_contents_follow_the_grid(stage, flavour).await
                })?;
        }
        Ok(())
    }

    /// **A prim's contents, live** (`SL_E2E_GRID=opensim|aditi`, where the
    /// avatar stands on land it may build on): the same rows against the
    /// grid's own listing, without the write nothing answers.
    #[test]
    fn a_prims_contents_follow_the_grid_on_a_live_grid() -> Result<(), TestError> {
        StageBuilder::new("contents_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid("a grid's own listing of a prim's contents"))
            .run(async |stage: &Stage| {
                let like = if stage.on_grid() == Grid::Aditi {
                    ImitatedGrid::SecondLife
                } else {
                    ImitatedGrid::OpenSim
                };
                a_prims_contents_follow_the_grid(stage, like).await
            })?;
        Ok(())
    }

    /// The name the run-time error test gives its prim.
    const FAULTY: &str = "Divides By Zero";

    /// A script that divides by zero as it starts: the zero is the prim's
    /// empty description, so no compiler folds the division away.
    const DIVIDES_BY_ZERO: &str = "default\n{\n    state_entry()\n    {\n        \
                                   integer zero = (integer)llGetObjectDesc();\n        \
                                   llOwnerSay((string)(1 / zero));\n    }\n}\n";

    /// **A run-time error, live** (`SL_E2E_GRID=opensim|aditi`, where the
    /// avatar stands on land it may build on): a prim's new script is
    /// replaced in the script editor with one that divides by zero, and the
    /// grid's report of the fault opens the Script Errors window on the
    /// grid's own words (`book/src/gridspec/lsl.md` § Run-time errors) —
    /// Second Life's `Math Error`, OpenSim's division by zero.
    #[test]
    fn a_scripts_fault_opens_the_script_errors_window_on_a_live_grid() -> Result<(), TestError> {
        StageBuilder::new("script_fault_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid("a grid's own run-time error"))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                arrived(alpha).await?;
                sweep_leftovers(stage, alpha).await?;
                alpha.press("Ctrl+B").await?;
                let build = alpha.ui().window("build-tools");
                let prim_id = rez_named(stage, alpha, &build, None, FAULTY).await?;
                let _content = build
                    .get(Locator::role(Role::Tab).name_key("build-tab-content"))
                    .click()
                    .await?;
                let row = build.get(Locator::role(Role::ListItem).name_containing(NEW_SCRIPT));
                let _made = build
                    .test_id("contents-button:build-content-new-script")
                    .click()
                    .await?;
                let _listed = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;
                // A row whose script is still on its way opens nothing.
                let adding = build.get(Locator::role(Role::ListItem).name_containing(ADDING));
                let _settled = alpha.expect(&adding).timeout(WAIT).to_be_hidden().await?;
                let _opened = row.double_click().await?;
                let body = alpha.ui().test_id("script-body:field");
                let _shown = alpha.expect(&body).timeout(WAIT).to_be_visible().await?;
                let _typed = body.fill(DIVIDES_BY_ZERO).await?;
                let _saved = alpha.ui().button_key("script-save").click().await?;

                let words = if stage.on_grid() == Grid::Aditi {
                    "Math Error"
                } else {
                    "ivision by Zero"
                };
                let window = alpha.ui().window("script-errors");
                let _opened = alpha.expect(&window).timeout(WAIT).to_be_visible().await?;
                let line =
                    window.get(Locator::test_id("script-errors-line").name_containing(words));
                let _reported = alpha.expect(&line).timeout(WAIT).to_be_visible().await?;
                let tab = window.get(Locator::role(Role::Tab).name_containing(FAULTY));
                let _tabbed = alpha.expect(&tab).timeout(WAIT).to_be_visible().await?;

                // Both windows stand between the camera and the prim.
                let _closed = window.button_key("floater-chrome-close").click().await?;
                let editor = alpha
                    .ui()
                    .locator(Locator::role(Role::Window).named(NEW_SCRIPT));
                let _closed = editor.button_key("floater-chrome-close").click().await?;
                let _gone = alpha.expect(&editor).timeout(WAIT).to_be_hidden().await?;
                alpha.press("Escape").await?;
                alpha.press("Escape").await?;
                alpha.press("Ctrl+B").await?;
                let this_prim = alpha
                    .world()
                    .locator(WorldLocator::full_id(prim_id))
                    .timeout(SETTLE);
                pie(alpha, &this_prim, &DELETE).await?;
                let _deleted = alpha
                    .expect_world(&this_prim)
                    .timeout(SETTLE)
                    .to_be_detached()
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    /// The name the compile test gives its prim.
    const COMPILED: &str = "Compiles On Save";

    /// A script missing an expression: the `;` at column 39 is where both
    /// grids place the fault. One line, because the editor types what a
    /// person types — an `Enter` with the completion list open accepts the
    /// completion — and indents each new line itself.
    const MISSING_EXPRESSION: &str = "default { state_entry() { integer x = ; } }";

    /// A script that compiles.
    const COMPILES: &str = "default { state_entry() { } }";

    /// A prim's new script is saved from the editor with a missing
    /// expression: the grid's compile refuses it, the editor says the compile
    /// failed and lists the error at line 1, column 39; saved again fixed, it
    /// says saved and lists nothing (`book/src/gridspec/lsl.md` § Compiling).
    async fn a_failed_compile_is_listed_in_the_editor(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        sweep_leftovers(stage, alpha).await?;
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window("build-tools");
        let stock = Some(alpha.world().object_named(UNNAMED).node().await?.full_id);
        let _prim_id = rez_named(stage, alpha, &build, stock, COMPILED).await?;
        let _content = build
            .get(Locator::role(Role::Tab).name_key("build-tab-content"))
            .click()
            .await?;
        let row = build.get(Locator::role(Role::ListItem).name_containing(NEW_SCRIPT));
        let _made = build
            .test_id("contents-button:build-content-new-script")
            .click()
            .await?;
        let _listed = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;
        let adding = build.get(Locator::role(Role::ListItem).name_containing(ADDING));
        let _settled = alpha.expect(&adding).timeout(WAIT).to_be_hidden().await?;
        let _opened = row.double_click().await?;
        let editor = alpha
            .ui()
            .locator(Locator::role(Role::Window).named(NEW_SCRIPT));
        let body = editor.test_id("script-body:field");
        let _shown = alpha.expect(&body).timeout(WAIT).to_be_visible().await?;

        let _typed = body.fill(MISSING_EXPRESSION).await?;
        let _saved = editor.button_key("script-save").click().await?;
        let failed = editor.get(Locator::role(Role::Text).name_key("script-status-compile-failed"));
        let _failed = alpha.expect(&failed).timeout(WAIT).to_be_visible().await?;
        // The row is the compiler's message after the position.
        let error = editor.get(Locator::role(Role::Text).name_containing("expected an expression"));
        let _listed = alpha.expect(&error).timeout(WAIT).to_be_visible().await?;
        // Without the isolation marks Fluent puts around each placeable.
        let said: String = error
            .text()
            .await?
            .unwrap_or_default()
            .chars()
            .filter(|character| !matches!(character, '\u{2068}' | '\u{2069}'))
            .collect();
        if !said.starts_with("Line 1, column 39: ") {
            return Err(format!("the error is listed as {said:?}").into());
        }

        let _typed = body.fill(COMPILES).await?;
        let _saved = editor.button_key("script-save").click().await?;
        let saved = editor.get(Locator::role(Role::Text).name_key("script-status-saved"));
        let _saved = alpha.expect(&saved).timeout(WAIT).to_be_visible().await?;
        let _cleared = alpha.expect(&error).timeout(WAIT).to_be_hidden().await?;
        let _closed = editor.button_key("floater-chrome-close").click().await?;
        let _gone = alpha.expect(&editor).timeout(WAIT).to_be_hidden().await?;
        Ok(())
    }

    /// **A failed compile, on each fake flavour**: the fake grid compiles a
    /// save as the grid it imitates — Second Life's one error at the token
    /// its parser stood on, column in bytes; YEngine's at the fault, in
    /// characters — and the editor lists it at the same line and column.
    #[test]
    fn a_failed_compile_is_listed_in_the_editor_on_each_fake_flavour() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "compile_second_life"),
            (ImitatedGrid::OpenSim, "compile_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .needs(Need::Content("the stock scene's unnamed box, to rez on"))
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| a_failed_compile_is_listed_in_the_editor(stage).await)?;
        }
        Ok(())
    }

    // ---- Attachments ------------------------------------------------------

    /// The name the attachment test gives its prim.
    const WORN: &str = "Worn By The Viewer";

    /// Where the attachment test goes and comes back from: a region the
    /// world map finds by name. Mauve, where the aditi avatars build, has no
    /// neighbour on the map; Ahern is where the third aditi avatar stands.
    fn elsewhere(stage: &Stage, here: &str) -> &'static str {
        match stage.on_grid() {
            Grid::Aditi => "Ahern",
            Grid::OpenSim | Grid::Fake => {
                if here == "East Region" {
                    "Default Region"
                } else {
                    "East Region"
                }
            }
        }
    }

    /// The region `alpha` stands in, by name.
    async fn region_name(alpha: &Viewer) -> Result<String, BodyError> {
        alpha
            .agent()
            .await?
            .region
            .and_then(|region| region.name)
            .ok_or_else(|| "Alpha stands in no named region".into())
    }

    /// Teleport `alpha` to `region` from the world map and wait until it
    /// stands there.
    async fn teleport_by_map(alpha: &Viewer, region: &str) -> Result<(), BodyError> {
        let map = alpha.open_floater("worldmap").await?;
        let _typed = map
            .test_id("worldmap:search")
            .role(Role::Textbox)
            .fill(region)
            .await?;
        let _picked = map
            .get(Locator::role(Role::ListItem).named(region))
            .timeout(WAIT)
            .click()
            .await?;
        let _asked = map
            .test_id("worldmap-button:teleport-selected")
            .click()
            .await?;
        let _arrived = alpha
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(SETTLE)
            .to_equal(json!(region))
            .await?;
        Ok(())
    }

    /// The attachments `alpha`'s own avatar is drawn wearing: the root of
    /// each, by full id.
    async fn worn_roots(alpha: &Viewer, me: sl_proto::Uuid) -> Result<Vec<WorldNode>, BodyError> {
        let mut nodes = alpha
            .world()
            .locator(WorldLocator::kind(WorldKind::Attachment))
            .nodes()
            .await?;
        nodes.retain(|node| node.worn_by == Some(me) && node.attachment_point.is_some());
        Ok(nodes)
    }

    /// Wait until `alpha`'s avatar is drawn wearing one object that `pick`
    /// chooses out of what it wears, and return it.
    async fn drawn_worn(
        alpha: &Viewer,
        me: sl_proto::Uuid,
        what: &str,
        pick: impl Fn(&WorldNode) -> bool,
    ) -> Result<WorldNode, BodyError> {
        let started = std::time::Instant::now();
        loop {
            let worn = worn_roots(alpha, me).await?;
            let picked: Vec<&WorldNode> = worn.iter().filter(|node| pick(node)).collect();
            if let [only] = picked.as_slice() {
                return Ok((*only).clone());
            }
            if started.elapsed() > SETTLE {
                return Err(format!(
                    "{what}: the avatar is drawn wearing {} matching object(s) of {}",
                    picked.len(),
                    worn.len()
                )
                .into());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Rez a prim, name it, take it, and wear it from the inventory window
    /// with Add; teleport away and back; and take it off again. Each time
    /// the avatar arrives the prim has to be drawn on it once — under the
    /// same full id and a new local id, as both grids re-send it
    /// ([[gridspec-attachments]]) — and the inventory's Worn tab has to list
    /// it; once it is taken off, neither.
    async fn a_worn_prim_follows_the_avatar(stage: &Stage) -> Result<(), BodyError> {
        let alpha = &stage.viewer("Alpha")?;
        arrived(alpha).await?;
        let me = alpha
            .agent()
            .await?
            .agent_id
            .ok_or("Alpha has no agent id")?;
        let (build, _spot) = rez_a_prim(stage, alpha).await?;
        let name = run_name(stage, WORN)?;
        let field = build.test_id("build-name:field");
        let _live = alpha.expect(&field).timeout(WAIT).to_be_enabled().await?;
        let selected = alpha.selection().await?;
        let [prim] = selected.as_slice() else {
            return Err(format!("the rez left {selected:?} selected").into());
        };
        let prim_id = prim.full_id;
        let _typed = field.fill(&name).await?;
        field.press("Enter").await?;
        let _kept = alpha
            .expect(&field)
            .timeout(WAIT)
            .to_have_text(&name)
            .await?;
        alpha.press("Escape").await?;
        alpha.press("Escape").await?;
        let _cleared = alpha
            .expect_state(Probe::Selection)
            .to_equal(json!([]))
            .await?;
        alpha.press("Ctrl+B").await?;
        let _closed = alpha.expect(&build).to_be_hidden().await?;
        let rezzed = alpha
            .world()
            .locator(WorldLocator::full_id(prim_id))
            .timeout(WAIT);
        pie(alpha, &rezzed, &TAKE).await?;
        let _gone = alpha
            .expect_world(&rezzed)
            .timeout(SETTLE)
            .to_be_detached()
            .await?;

        let inventory = alpha.ui().window("inventory");
        if !inventory.is_visible().await? {
            alpha.press("Ctrl+I").await?;
            let _shown = alpha.expect(&inventory).to_be_visible().await?;
        }
        let search = inventory.test_id("inventory:search").role(Role::Textbox);
        let named = inventory.get(Locator::role(Role::TreeItem).named(&name));
        let _searched = search.fill(NO_SUCH_ITEM).await?;
        let _narrowed = alpha.expect(&named).timeout(WAIT).to_be_hidden().await?;
        let _searched = search.fill(&name).await?;
        let row = inventory
            .get(Locator::role(Role::TreeItem).named(&name))
            .timeout(WAIT);
        let _filed = alpha.expect(&row).timeout(WAIT).to_be_visible().await?;

        // Worn with Add, beside whatever the avatar already wears.
        let before: Vec<sl_proto::Uuid> = worn_roots(alpha, me)
            .await?
            .iter()
            .map(|node| node.full_id)
            .collect();
        let _menu = row.right_click().await?;
        let _added = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-inv-add"))
            .timeout(WAIT)
            .click()
            .await?;
        let worn = drawn_worn(alpha, me, "after the wear", |node| {
            !before.contains(&node.full_id)
        })
        .await?;
        let worn_tab = inventory.get(Locator::role(Role::Tab).name_key("inventory-tab-worn"));
        let all_tab = inventory.get(Locator::role(Role::Tab).name_key("inventory-tab-everything"));
        let _tab = worn_tab.click().await?;
        let listed = inventory
            .get(Locator::role(Role::TreeItem).named(&name))
            .timeout(WAIT);
        let _listed = alpha.expect(&listed).timeout(WAIT).to_be_visible().await?;
        let _tab = all_tab.click().await?;

        // Away and back: drawn on the avatar each time, once, the same object.
        let home = region_name(alpha).await?;
        let mut last = worn;
        for region in [elsewhere(stage, &home).to_owned(), home] {
            teleport_by_map(alpha, &region).await?;
            let full_id = last.full_id;
            let arrived_worn = drawn_worn(alpha, me, &format!("in {region}"), |node| {
                node.full_id == full_id
            })
            .await?;
            if arrived_worn.local_id == last.local_id {
                return Err(format!(
                    "in {region} the attachment kept its local id {:?}: the grid's \
                     re-send under a new one was never read",
                    last.local_id
                )
                .into());
            }
            last = arrived_worn;
        }
        let _tab = worn_tab.click().await?;
        let _still = alpha.expect(&listed).timeout(WAIT).to_be_visible().await?;

        // Taken off from its row: gone from the avatar and from the tab.
        let _menu = listed.right_click().await?;
        let _off = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-inv-detach-from-yourself"))
            .timeout(WAIT)
            .click()
            .await?;
        let off = alpha
            .world()
            .locator(WorldLocator::full_id(last.full_id))
            .timeout(SETTLE);
        let _detached = alpha
            .expect_world(&off)
            .timeout(SETTLE)
            .to_be_detached()
            .await?;
        let _unlisted = alpha.expect(&listed).timeout(WAIT).to_be_hidden().await?;
        let _tab = all_tab.click().await?;
        Ok(())
    }

    /// **An attachment follows the avatar, live** (`SL_E2E_GRID=opensim|aditi`):
    /// a prim taken and worn from the inventory stays drawn on the avatar, once
    /// and listed as worn, across a teleport away and back, and goes when it is
    /// taken off. The item stays in the avatar's Objects folder.
    #[test]
    fn a_worn_prim_follows_the_avatar_on_a_live_grid() -> Result<(), TestError> {
        StageBuilder::new("attachment_live")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid(
                "a grid's own attachments, re-sent under new local ids by a teleport",
            ))
            .run(a_worn_prim_follows_the_avatar)?;
        Ok(())
    }
}
