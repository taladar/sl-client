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

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{
        Locator, LogStream, Probe, Role, WorldKind, WorldLocator, WorldNode,
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

    /// The name a prim nobody has named has on both grids.
    const UNNAMED: &str = "Object";

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
        alpha.press("Ctrl+B").await?;
        let build = alpha.ui().window("build-tools");
        let _create = build
            .get(Locator::role(Role::Radio).name_key("build-tool-create"))
            .click()
            .await?;
        let spot = if stage.on_grid() == Grid::Fake {
            let _placed = alpha.world().object_named(UNNAMED).place().await?;
            None
        } else {
            // On the ground a few metres to the camera's right of the avatar
            // and a little ahead of it: the Build window takes the left of
            // the screen. Where exactly moves with the run, since a run that
            // failed half-way leaves its prim where the next would aim.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| format!("the clock: {error}"))?
                .as_secs();
            let aside = 3.0 + f32::from(u8::try_from(stamp % 6).unwrap_or(0));
            let readout = alpha.agent().await?;
            let spot = readout.position.ok_or("Alpha has no position")?;
            let eye = readout.camera_eye.ok_or("Alpha has no camera")?;
            let (ahead_x, ahead_y) = (spot[0] - eye[0], spot[1] - eye[1]);
            let length = ahead_x.hypot(ahead_y).max(0.001);
            let (ahead_x, ahead_y) = (ahead_x / length, ahead_y / length);
            let (east, north) = (
                spot[0] + aside * ahead_y + 3.0 * ahead_x,
                spot[1] - aside * ahead_x + 3.0 * ahead_y,
            );
            let _placed = alpha
                .world()
                .ground(stage.home_region(), east, north)
                .timeout(SETTLE)
                .place()
                .await?;
            Some((east, north))
        };
        let _general = build
            .get(Locator::role(Role::Tab).name_key("build-tab-general"))
            .click()
            .await?;
        Ok((build, spot))
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
        // A name of this run's own on a live grid, whose inventory keeps the
        // items of the runs before it.
        let name = if spot.is_some() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| format!("the clock: {error}"))?
                .as_secs();
            format!("{TAKEN} {stamp}")
        } else {
            TAKEN.to_owned()
        };
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
}
