//! The pilot end-to-end tests (tier E): what a person checked by logging in
//! and clicking, on the fake grid, through the automation driver alone, and
//! on both backends.
//!
//! - the status bar names the region, and the menu bar opens the Build
//!   window;
//! - one viewer rezzes a prim from the Build window and names it, and another
//!   finds it by that name, selects it and is refused its edit controls,
//!   because it does not own it;
//! - an object's pie offers its slices, and Touch reaches the grid;
//! - the world map teleports a viewer to the neighbouring region;
//! - a line said in one viewer's chat bar is heard in the other's transcript.
//!
//! The chrome and the chat test need nothing of the grid but its accounts,
//! so they also run on a live one (`SL_E2E_GRID=opensim|aditi`); the others
//! need the fake grid's scene or its control handle, and skip there.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{Locator, Probe, Role, TeleportState};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_fake_grid::fixtures::scenarios;
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_proto::{AgentKey, AnyMessage, ObjectKey, RegionLocalObjectId, ServerEvent, prim_flags};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take: a rez, a name, a
    /// touch reaching the grid.
    const WAIT: Duration = Duration::from_secs(60);

    /// How long a teleport may take, handover included.
    const TELEPORT: Duration = Duration::from_secs(120);

    /// The Build menu's path to the Build window, by the entries' Fluent keys.
    const BUILD_TOOLS: [&str; 2] = ["menu-bar-build", "menu-bar-build-tools"];

    /// The Build window's floater id.
    const BUILD_WINDOW: &str = "build-tools";

    /// The status bar's region read-out.
    const REGION_READOUT: &str = "status-readout:region";

    /// A stage for `name` with the viewers `labels`.
    fn stage(name: &str, labels: &[&str]) -> StageBuilder {
        labels.iter().fold(
            StageBuilder::new(name).viewer_binary(VIEWER),
            |builder, label| builder.viewer(*label),
        )
    }

    /// The build-tool radio option whose caption is `key`.
    fn tool(key: &str) -> Locator {
        Locator::role(Role::Radio).name_key(key)
    }

    /// **Login and chrome**: the status bar names the region the viewer
    /// logged into, and the menu bar's Build ▸ Build Tools opens the Build
    /// window.
    #[test]
    fn the_status_bar_names_the_region_and_the_menu_bar_opens_the_build_window()
    -> Result<(), TestError> {
        stage("login_and_chrome", &["Alpha"]).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let ui = alpha.ui();
            let _named = alpha
                .expect(&ui.test_id(REGION_READOUT))
                .to_have_text(stage.home_region())
                .await?;
            let build = ui.window(BUILD_WINDOW);
            assert!(
                !build.is_visible().await?,
                "the Build window is open before anything opened it"
            );
            let _clicked = alpha.menu_path(&BUILD_TOOLS).await?;
            let _shown = alpha.expect(&build).to_be_visible().await?;
            Ok(())
        })?;
        Ok(())
    }

    /// **A rez on bare ground the camera has to turn to**: a point eight
    /// metres to the camera's right of the avatar and three ahead — about 45°
    /// off the view axis, at the edge of the frame — is revealed, and the
    /// Create tool rezzes there; then, with the selection dropped and the
    /// camera sent back to the avatar, picking the new prim again leaves the
    /// camera outside it. The geometry and the sequence of the live
    /// two-resident test at its widest offset.
    #[test]
    fn a_ground_point_at_the_edge_of_the_view_is_revealed_and_rezzed_on() -> Result<(), TestError> {
        stage("ground_reveal", &["Alpha"]).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let _opened = alpha.menu_path(&BUILD_TOOLS).await?;
            let build = alpha.ui().window(BUILD_WINDOW);
            let _create = alpha
                .expect(&build.get(tool("build-tool-create")))
                .to_be_checked()
                .await?;
            let readout = alpha.agent().await?;
            let spot = readout.position.ok_or("Alpha has no position")?;
            let eye = readout.camera_eye.ok_or("Alpha has no camera")?;
            let (ahead_x, ahead_y) = (spot[0] - eye[0], spot[1] - eye[1]);
            let length = ahead_x.hypot(ahead_y).max(0.001);
            let (ahead_x, ahead_y) = (ahead_x / length, ahead_y / length);
            let _placed = alpha
                .world()
                .ground(
                    stage.home_region(),
                    spot[0] + 8.0 * ahead_y + 3.0 * ahead_x,
                    spot[1] - 8.0 * ahead_x + 3.0 * ahead_y,
                )
                .timeout(WAIT)
                .place()
                .await?;
            let _moving = alpha
                .expect(&build.get(tool("build-tool-move")))
                .to_be_checked()
                .await?;
            // As the live test does next: drop the selection, the camera
            // going back to the avatar, and pick the new prim again.
            let selected = alpha.selection().await?;
            let [prim] = selected.as_slice() else {
                return Err(format!("the rez left {selected:?} selected").into());
            };
            let prim = alpha
                .world()
                .locator(sl_automation_proto::WorldLocator::full_id(prim.full_id))
                .timeout(WAIT);
            alpha.press("Escape").await?;
            alpha.press("Escape").await?;
            let _reselected = prim.select().await?;
            let node = prim.node().await?;
            let (Some(centre), Some(size)) = (node.position, node.scale) else {
                return Err(format!("the prim has no box: {node:?}").into());
            };
            let eye = alpha
                .agent()
                .await?
                .camera_eye
                .ok_or("Alpha has no camera")?;
            let inside = (0..3).all(|axis| {
                (eye.get(axis).copied().unwrap_or(0.0) - centre.get(axis).copied().unwrap_or(0.0))
                    .abs()
                    < size.get(axis).copied().unwrap_or(0.0) / 2.0
            });
            assert!(
                !inside,
                "the camera ended inside the prim it selected: eye {eye:?}, box {centre:?} ± \
                 {size:?}/2"
            );
            Ok(())
        })?;
        Ok(())
    }

    /// The name Alpha gives the prim it rezzes.
    const PRIM: &str = "Pilot Box";

    /// **Two viewers, one object**: Alpha opens the Build window — on the
    /// Create tool, since nothing is selected — rezzes a prim on the stock box
    /// and names it. Beta finds it by that name, selects it, and its transform
    /// controls are disabled, because Beta does not own the prim; Alpha's are
    /// enabled, because Alpha does.
    #[test]
    fn a_prim_rezzed_by_one_viewer_is_selected_but_not_editable_in_another() -> Result<(), TestError>
    {
        stage("one_prim_two_viewers", &["Alpha", "Beta"])
            .needs(Need::Content("the stock scene's unnamed box, to rez on"))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let beta = &stage.viewer("Beta")?;

                let _opened = alpha.menu_path(&BUILD_TOOLS).await?;
                let build = alpha.ui().window(BUILD_WINDOW);
                let _create = alpha
                    .expect(&build.get(tool("build-tool-create")))
                    .to_be_checked()
                    .await?;
                // The stock box is the region's one object, and nobody has named
                // it.
                let _placed = alpha.world().object_named("Object").place().await?;
                // A plain rez drops into edit on the new prim, on the Move tool.
                let _moving = alpha
                    .expect(&build.get(tool("build-tool-move")))
                    .to_be_checked()
                    .await?;
                let _general = build
                    .get(Locator::role(Role::Tab).name_key("build-tab-general"))
                    .click()
                    .await?;
                let name = build.test_id("build-name:field");
                let _filled = name.fill(PRIM).await?;
                name.press("Enter").await?;
                let _editable = alpha
                    .expect(&build.test_id("build-size-x:field"))
                    .to_be_enabled()
                    .await?;

                let prim = beta.world().object_named(PRIM).timeout(WAIT);
                let seen = prim.node().await?;
                let _opened = beta.menu_path(&BUILD_TOOLS).await?;
                let beta_build = beta.ui().window(BUILD_WINDOW);
                // The Build window opens on the Create tool, where a click rezzes.
                let _moving = beta_build.get(tool("build-tool-move")).click().await?;
                let _selected = prim.select().await?;
                let selection = beta.selection().await?;
                assert_eq!(
                    selection
                        .iter()
                        .map(|selected| selected.full_id)
                        .collect::<Vec<_>>(),
                    vec![seen.full_id],
                    "Beta's selection is Alpha's prim"
                );
                for field in ["build-pos-x:field", "build-size-x:field"] {
                    let _refused = beta
                        .expect(&beta_build.test_id(field))
                        .to_be_disabled()
                        .await?;
                }
                Ok(())
            })?;
        Ok(())
    }

    /// The touch-handling box the pie test puts on the grid.
    const TOUCH_BOX: &str = "Touch Box";

    /// Its region-local id, clear of the stock scene's.
    const TOUCH_BOX_LOCAL_ID: u32 = 0x7E57;

    /// Put a box named [`TOUCH_BOX`] beside the stock one, owned by somebody
    /// else and flagged the way a simulator flags a prim whose script has a
    /// touch handler, and show it to the viewer `label`.
    async fn rez_touch_box(stage: &Stage, label: &str) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        let mut touch_box = sl_fake_grid::world::box_prim(
            RegionLocalObjectId(TOUCH_BOX_LOCAL_ID),
            ObjectKey::from(sl_proto::Uuid::from_u128(0x7E57_0B1E)),
            AgentKey::from(sl_proto::Uuid::from_u128(0x7E57_0A11)),
            sl_proto::Vector {
                x: 132.0,
                y: 124.0,
                z: 25.5,
            },
            sl_proto::Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        touch_box.update_flags = prim_flags::SCRIPTED | prim_flags::HANDLE_TOUCH;
        let mut properties = sl_fake_grid::world::default_object_properties(&touch_box);
        properties.name = TOUCH_BOX.to_owned();
        touch_box.properties = Some(properties);
        agent
            .with_world(|world, sim| {
                world.objects.push(touch_box.clone());
                sl_fake_grid::world::send_objects(sim, &[touch_box], now)
            })
            .await
            .map_err(|error| format!("showing the touch box: {error}"))?;
        Ok(())
    }

    /// **Pie menu**: a right-click on a touch-handling prim opens the object
    /// pie with its slices, Touch enabled and Pay not; Touch sends the touch
    /// to the grid.
    #[test]
    fn the_object_pie_offers_its_slices_and_touch_reaches_the_grid() -> Result<(), TestError> {
        stage("object_pie", &["Alpha"])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                rez_touch_box(stage, "Alpha").await?;
                let mut heard = stage.agent("Alpha").await?.events();

                let _pie = alpha
                    .world()
                    .object_named(TOUCH_BOX)
                    .timeout(WAIT)
                    .open_pie()
                    .await?;
                let pie = alpha.ui().role(Role::Menu);
                for enabled in [
                    "pie-object-open",
                    "pie-object-create",
                    "pie-object-touch",
                    "pie-object-sit-here",
                    "pie-object-edit",
                ] {
                    let _slice = alpha
                        .expect(&pie.get(Locator::role(Role::MenuItem).name_key(enabled)))
                        .to_be_enabled()
                        .await?;
                }
                // The viewer has no payment floater yet.
                let _pay = alpha
                    .expect(&pie.get(Locator::role(Role::MenuItem).name_key("pie-object-pay")))
                    .to_be_disabled()
                    .await?;
                let _touched = alpha
                    .pie_slice(Locator::role(Role::MenuItem).name_key("pie-object-touch"))
                    .await?;

                let grabbed = tokio::time::timeout(WAIT, async {
                    loop {
                        match heard.recv().await {
                            Ok(ServerEvent::ClientMessage(message)) => {
                                if let AnyMessage::ObjectGrab(grab) = *message
                                    && grab.object_data.local_id == TOUCH_BOX_LOCAL_ID
                                {
                                    return Ok(());
                                }
                            }
                            Ok(_other) => {}
                            Err(error) => return Err(error),
                        }
                    }
                })
                .await;
                match grabbed {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(error)) => Err(format!("the grid's event stream: {error}").into()),
                    Err(_elapsed) => Err("the touch never reached the grid".into()),
                }
            })?;
        Ok(())
    }

    /// The line Alpha says in the chat test.
    const LINE: &str = "Hello from the pilot suite";

    /// **Two-viewer chat**: Alpha says a line in its nearby-chat bar; Beta's
    /// transcript shows it from Alpha, and so does Alpha's own — the echo a
    /// viewer shows its own line by. On a live grid the two avatars must be
    /// within chat range of each other where they log in.
    #[test]
    fn a_line_said_in_one_viewer_is_heard_in_the_other() -> Result<(), TestError> {
        stage("two_viewer_chat", &["Alpha", "Beta"]).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let beta = &stage.viewer("Beta")?;
            let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
            let _typed = bar.fill(LINE).await?;
            bar.press("Enter").await?;
            let speaker = stage.account_name("Alpha")?;
            for viewer in [alpha, beta] {
                let _heard = viewer
                    .expect_chat()
                    .from(speaker)
                    .timeout(WAIT)
                    .to_contain(LINE)
                    .await?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// The region the teleport test starts in.
    const HOME: &str = "Catalogue";

    /// The region east of it.
    const NEIGHBOUR: &str = "Neighbour";

    /// **Two regions**: from the catalogue region, the world map's search
    /// finds the neighbouring region, and its Teleport button takes the
    /// viewer there; the status bar and the agent probe agree on where it
    /// arrived.
    #[test]
    fn the_world_map_teleports_to_the_neighbouring_region() -> Result<(), TestError> {
        let catalogue =
            scenarios::scenario("catalogue").ok_or("the catalogue scenario is not registered")?;
        let home = catalogue.dress(RegionConfig {
            name: HOME.to_owned(),
            ..RegionConfig::default()
        });
        let neighbour = RegionConfig {
            name: NEIGHBOUR.to_owned(),
            grid_x: home.grid_x.saturating_add(1),
            ..RegionConfig::default()
        };
        stage("world_map_teleport", &["Alpha"])
            .region(home)
            .region(neighbour)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let _opened = alpha
                    .menu_path(&["menu-bar-world", "menu-bar-world-map"])
                    .await?;
                let map = alpha.ui().window("worldmap");
                let _typed = map
                    .test_id("worldmap:search")
                    .role(Role::Textbox)
                    .fill(NEIGHBOUR)
                    .await?;
                let _picked = map
                    .get(Locator::role(Role::ListItem).named(NEIGHBOUR))
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
                    .timeout(TELEPORT)
                    .to_equal(json!(NEIGHBOUR))
                    .await?;
                let _shown = alpha
                    .expect(&alpha.ui().test_id(REGION_READOUT))
                    .to_have_text(NEIGHBOUR)
                    .await?;
                let status = alpha.status().await?;
                assert_eq!(
                    status.region.as_deref(),
                    Some(NEIGHBOUR),
                    "the status probe"
                );
                Ok(())
            })?;
        Ok(())
    }

    /// A region rated Moderate, ten regions east: a distant teleport, and one
    /// a grid that checks refuses to an agent whose preference is General.
    const RATED: &str = "Rated";

    /// Search the world map for `region` and press its Teleport button.
    async fn ask_the_map_for(alpha: &Viewer, region: &str) -> Result<(), BodyError> {
        let _opened = alpha
            .menu_path(&["menu-bar-world", "menu-bar-world-map"])
            .await?;
        let map = alpha.ui().window("worldmap");
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
        Ok(())
    }

    /// **A teleport ends the way the grid it is on ends it**: the same request
    /// — an agent whose maturity preference is General asking the world map
    /// for a region rated Moderate — is refused by a grid imitating Second
    /// Life and carried out by one imitating OpenSim, and the viewer follows
    /// both.
    ///
    /// The refusal is Second Life's whole shape: a start and two progress
    /// lines over UDP, then a `TeleportFailed` over the **event queue** whose
    /// reason is a sentence and whose alert is the key `RegionTPAccessBlocked`.
    /// The progress display ends on the notification catalogue's text for that
    /// key with the key itself beneath it, and the agent is where it was. On
    /// the OpenSim flavour nothing narrates the teleport and nothing checks the
    /// preference: the agent arrives.
    #[test]
    fn a_teleport_ends_as_the_grid_it_is_on_ends_it() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "teleport_refused_second_life"),
            (ImitatedGrid::OpenSim, "teleport_admitted_open_sim"),
        ] {
            let catalogue = scenarios::scenario("catalogue")
                .ok_or("the catalogue scenario is not registered")?;
            let home = catalogue.dress(RegionConfig {
                name: HOME.to_owned(),
                ..RegionConfig::default()
            });
            let rated = RegionConfig {
                name: RATED.to_owned(),
                grid_x: home.grid_x.saturating_add(10),
                maturity: sl_proto::Maturity::Mature,
                ..RegionConfig::default()
            };
            stage(name, &["Alpha"])
                .region(home)
                .region(rated)
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    // The preference the viewer would have set through its
                    // own preferences, stored where the grid keeps it.
                    stage
                        .agent("Alpha")
                        .await?
                        .with_sim(|sim| {
                            sim.merge_agent_preferences(&sl_proto::AgentPreferences {
                                max_access_pref: Some("PG".to_owned()),
                                ..sl_proto::AgentPreferences::default()
                            });
                        })
                        .await;
                    ask_the_map_for(alpha, RATED).await?;
                    match flavour {
                        ImitatedGrid::SecondLife => {
                            let _failed = alpha
                                .expect_state(Probe::Agent)
                                .at("/teleport/state")
                                .timeout(TELEPORT)
                                .to_equal(json!("failed"))
                                .await?;
                            let readout = alpha.agent().await?;
                            let teleport = readout.teleport.ok_or("no teleport readout")?;
                            let TeleportState::Failed { reason, detail } = teleport.state else {
                                return Err(format!("the teleport is {:?}", teleport.state).into());
                            };
                            assert!(
                                reason.contains("maturity rating"),
                                "the refusal reads as the catalogue's text: {reason:?}"
                            );
                            assert!(
                                detail
                                    .as_deref()
                                    .is_some_and(|detail| detail.contains("RegionTPAccessBlocked")),
                                "the key the grid sent stays on the page: {detail:?}"
                            );
                            assert_eq!(
                                readout.region.and_then(|region| region.name).as_deref(),
                                Some(HOME),
                                "a refused teleport leaves the agent where it was"
                            );
                        }
                        ImitatedGrid::OpenSim => {
                            let _arrived = alpha
                                .expect_state(Probe::Agent)
                                .at("/region/name")
                                .timeout(TELEPORT)
                                .to_equal(json!(RATED))
                                .await?;
                        }
                    }
                    Ok(())
                })?;
        }
        Ok(())
    }
}
