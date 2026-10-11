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
        let spot = place_a_prim(stage, alpha, &build, None, 0.0).await?;
        Ok((build, spot))
    }

    /// With the Build window open, rez a prim with the Create tool — on the
    /// object `on` on the fake grid, or on the stock scene's unnamed box;
    /// `further` metres further to the side than [`rez_a_prim`]'s on a live
    /// grid — and leave the window on its General tab.
    async fn place_a_prim(
        stage: &Stage,
        alpha: &Viewer,
        build: &UiLocator,
        on: Option<sl_proto::Uuid>,
        further: f32,
    ) -> Result<Option<(f32, f32)>, BodyError> {
        let _create = build
            .get(Locator::role(Role::Radio).name_key("build-tool-create"))
            .click()
            .await?;
        let spot = if stage.on_grid() == Grid::Fake {
            let on = on.map_or_else(
                || alpha.world().object_named(UNNAMED),
                |id| alpha.world().locator(WorldLocator::full_id(id)),
            );
            let _placed = on.place().await?;
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
            let aside = 3.0 + further + f32::from(u8::try_from(stamp % 6).unwrap_or(0));
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

    /// The name the edit test gives its prim.
    const EDITED: &str = "Edited By The Viewer";

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
        let (build, spot) = rez_a_prim(stage, alpha).await?;
        let name = if spot.is_some() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| format!("the clock: {error}"))?
                .as_secs();
            format!("{EDITED} {stamp}")
        } else {
            EDITED.to_owned()
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
        further: f32,
    ) -> Result<sl_proto::Uuid, BodyError> {
        let spot = place_a_prim(stage, alpha, build, on, further).await?;
        let name = if spot.is_some() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| format!("the clock: {error}"))?
                .as_secs();
            format!("{name} {stamp}")
        } else {
            name.to_owned()
        };
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
        let near = rez_named(stage, alpha, &build, stock, near, 0.0).await?;
        let far = rez_named(stage, alpha, &build, stock, far, 1.5).await?;
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

        // Both are still selected, as two prims or as one linkset: Delete
        // takes what is selected.
        alpha.press("Escape").await?;
        alpha.press("Delete").await?;
        for id in [near, far] {
            let _gone = alpha
                .expect_world(&prim(id))
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
}
