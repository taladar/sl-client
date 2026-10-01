//! The **state probes** over the fixture world: each reader of
//! `sl_viewer_automation` changes when the state it reads does, through the
//! probe sources the viewer's assembly registers
//! ([`crate::automation_sources`]).
//!
//! The readers the fixture world cannot feed — a transcript from a real chat,
//! a toast on screen, a teleport, a screenshot of a rendered window — are the
//! full-stack tier's (`full_stack_test`); the readout functions themselves are
//! tested beside the models they read.

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{
        CameraView, EnvironmentReadout, InventoryEntry, InventoryFolderReadout, InventoryRoot,
        LogStream, NotificationReadout, OfferedButton, RegionReadout, SelectedObject, SkyReadout,
    };
    use sl_client_bevy::{
        AgentKey, AssetType, Command, FolderInfo, FolderState, FolderType, InventoryFolderKey,
        InventoryKey, InventoryType, ItemInfo, LandArea, LindenAmount, MoneyBalance, ObjectKey,
        OwnerKey, Permissions5, RegionHandle, SaleInfo, SkySettings, SlAgentParcel, SlCommand,
        SlCurrentRegion, SlEvent, SlIdentity, SlRegion, SlSessionEvent, Uuid, Vector,
    };
    use sl_viewer_automation::{
        EventLog, ProbeError, ProbeSources, StateProbesPlugin, read_agent, read_environment,
        read_inventory, read_notifications, read_quiescence, read_selection, read_status,
    };
    use sl_viewer_inventory::inventory::InventoryModel;
    use sl_viewer_notifications::{
        NotificationId, NotificationManager, NotificationRecord, ToastButton, template,
    };
    use sl_viewer_ui_core::ui_element::UiAction;
    use sl_viewer_world_api::rlv::RlvEnvironmentSlot;
    use sl_viewer_world_api::{AgentRegionPosition, AvatarControls, CameraMode, SelectionSet};

    use crate::status_bar::AgentBalance;
    use crate::world_test::{entity_of, seed_prim_numbered, settle, world_app};

    /// A failed setup step.
    type TestError = Box<dyn core::error::Error>;

    /// The own agent.
    const OWN: u128 = 0xA;

    /// The fixture world with the state probes installed and the viewer's own
    /// probe sources registered, as the assembly registers them.
    fn probe_world() -> App {
        let mut app = world_app();
        app.add_plugins(StateProbesPlugin)
            .insert_resource(crate::automation_sources::probe_sources());
        settle(&mut app, 1);
        app
    }

    #[test]
    fn the_agent_probe_follows_the_session() -> Result<(), TestError> {
        let mut app = probe_world();
        let before = read_agent(app.world_mut());
        assert_eq!(before.agent_id, None);
        assert_eq!(before.region, None);
        assert_eq!(before.position, None);
        assert_eq!(before.seated_on, None);
        assert_eq!(before.camera, Some(CameraView::ThirdPerson));
        assert_eq!(
            before.teleport, None,
            "no teleport overlay in the fixture world, so nothing to report"
        );

        let own = AgentKey::from(Uuid::from_u128(OWN));
        let seat = ObjectKey::from(Uuid::from_u128(0x5EA7));
        let handle = RegionHandle::new((256_000_u64 << 32) | 256_512);
        app.world_mut().resource_mut::<SlIdentity>().agent_id = Some(own);
        app.world_mut().spawn((
            SlRegion {
                handle,
                sim: "127.0.0.1:9000".parse()?,
            },
            SlCurrentRegion,
        ));
        app.world_mut().insert_resource(AgentRegionPosition {
            position: Some(Vector {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            }),
        });
        app.world_mut().resource_mut::<SlAgentParcel>().seated_on = Some(seat);
        *app.world_mut().resource_mut::<CameraMode>() = CameraMode::Mouselook;
        let after = read_agent(app.world_mut());
        assert_eq!(after.agent_id, Some(own.uuid()));
        assert_eq!(
            after.region,
            Some(RegionReadout {
                name: None,
                handle: handle.get(),
                id: None,
            }),
            "a region before its handshake has a handle and nothing else"
        );
        assert_eq!(after.position, Some([10.0, 20.0, 30.0]));
        assert_eq!(after.seated_on, Some(seat.uuid()));
        assert_eq!(after.camera, Some(CameraView::Mouselook));
        Ok(())
    }

    /// The heading is the viewer's own, and only once it is known: an unseeded
    /// heading is no heading, whatever its placeholder value.
    #[test]
    fn the_agent_probe_reads_the_held_heading() {
        let mut app = probe_world();
        app.world_mut().resource_mut::<AvatarControls>().yaw = 1.25;
        assert_eq!(
            read_agent(app.world_mut()).heading,
            None,
            "a heading not yet seeded from the avatar is not one"
        );
        {
            let mut controls = app.world_mut().resource_mut::<AvatarControls>();
            controls.seeded = true;
            controls.yaw = -0.5;
        }
        assert_eq!(read_agent(app.world_mut()).heading, Some(-0.5));
    }

    /// The environment probe reads the sky the scene publishes and whether
    /// the local layer holds it, and from the scene the water drawn and the
    /// windows previewing through the edit layer.
    #[test]
    fn the_environment_probe_reads_the_published_sky() {
        let mut app = probe_world();
        app.world_mut().remove_resource::<RlvEnvironmentSlot>();
        let scene = app
            .world_mut()
            .remove_resource::<sl_viewer_world_scene::environment::EnvironmentState>();
        assert_eq!(
            read_environment(app.world_mut()),
            EnvironmentReadout {
                sky: None,
                local_sky: false,
                transition: None,
                water: None,
                previewing: Vec::new(),
            },
            "no environment scene, no sky"
        );
        let mut sky = SkySettings::legacy_windlight_default("Default");
        sky.ambient = sl_client_bevy::Color::new(1.0, 0.0, 0.0);
        sky.haze_density = 2.5;
        let (azimuth, elevation) = sl_client_bevy::rotation_to_azimuth_altitude(&sky.sun_rotation);
        app.world_mut().insert_resource(RlvEnvironmentSlot {
            rendered: Some(sky),
            fixed_sky: true,
            ..RlvEnvironmentSlot::default()
        });
        assert_eq!(
            read_environment(app.world_mut()),
            EnvironmentReadout {
                sky: Some(SkyReadout {
                    name: "Default".to_owned(),
                    ambient: [1.0, 0.0, 0.0],
                    haze_density: 2.5,
                    sun: [azimuth, elevation],
                }),
                local_sky: true,
                transition: None,
                water: None,
                previewing: Vec::new(),
            },
            "no scene: the slot's sky alone"
        );

        let mut scene = scene.unwrap_or_default();
        let mut water = sl_client_bevy::WaterSettings::legacy_default("Edited Water");
        water.water_fog_density = 7.5;
        scene.set_edit(
            sl_viewer_world_scene::environment::EditPreviewer("settings-editor-water"),
            sl_client_bevy::EnvironmentAsset::Water(water),
        );
        app.world_mut().insert_resource(scene);
        let readout = read_environment(app.world_mut());
        assert_eq!(
            readout.water,
            Some(sl_automation_proto::WaterReadout {
                name: "Edited Water".to_owned(),
                fog_density: 7.5,
            }),
            "the water drawn is the one the editor previews"
        );
        assert_eq!(readout.previewing, vec!["settings-editor-water".to_owned()]);
    }

    #[test]
    fn the_status_probe_reads_the_balance_the_status_bar_shows() {
        let mut app = probe_world();
        app.init_resource::<AgentBalance>()
            .add_systems(Update, crate::status_bar::track_balance);
        assert_eq!(read_status(app.world_mut()).balance, None);
        assert_eq!(read_status(app.world_mut()).region, None);
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::MoneyBalance(MoneyBalance {
                agent_id: AgentKey::from(Uuid::from_u128(OWN)),
                transaction_id: Uuid::nil(),
                success: true,
                balance: LindenAmount(250),
                square_meters_credit: LandArea(0),
                square_meters_committed: LandArea(0),
                description: String::new(),
                transaction: None,
            })));
        settle(&mut app, 1);
        let status = read_status(app.world_mut());
        assert_eq!(status.balance, Some(250));
        assert!(status.time.hour < 24 && status.time.minute < 60);
    }

    #[test]
    fn the_selection_probe_follows_the_selection() -> Result<(), TestError> {
        let mut app = probe_world();
        let first = seed_prim_numbered(
            &mut app,
            1,
            Vector {
                x: 100.0,
                y: 100.0,
                z: 25.0,
            },
        );
        let second = seed_prim_numbered(
            &mut app,
            2,
            Vector {
                x: 104.0,
                y: 100.0,
                z: 25.0,
            },
        );
        settle(&mut app, 2);
        assert_eq!(read_selection(app.world()), Vec::new());
        let first_entity = entity_of(&mut app, first).ok_or("the first prim never spawned")?;
        let second_entity = entity_of(&mut app, second).ok_or("the second prim never spawned")?;
        let full = |local: u32| ObjectKey::from(Uuid::from_u128(u128::from(local) + 0x1000));
        {
            let mut selection = app.world_mut().resource_mut::<SelectionSet>();
            selection.insert(first, full(first.id.0), first_entity);
            selection.insert(second, full(second.id.0), second_entity);
        }
        assert_eq!(
            read_selection(app.world()),
            vec![
                SelectedObject {
                    full_id: full(first.id.0).uuid(),
                    local_id: first.id.0,
                    primary: false,
                },
                SelectedObject {
                    full_id: full(second.id.0).uuid(),
                    local_id: second.id.0,
                    primary: true,
                },
            ],
            "in selection order, the last one primary"
        );
        app.world_mut()
            .resource_mut::<SelectionSet>()
            .remove(second);
        assert_eq!(
            read_selection(app.world())
                .iter()
                .map(|object| (object.local_id, object.primary))
                .collect::<Vec<_>>(),
            [(first.id.0, true)]
        );
        Ok(())
    }

    /// A skeleton folder `id` under `parent`.
    fn folder(
        id: u128,
        parent: Option<u128>,
        name: &str,
        ty: FolderType,
        loaded: bool,
    ) -> FolderInfo {
        FolderInfo {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(id)),
            parent_id: parent.map(|parent| InventoryFolderKey::from(Uuid::from_u128(parent))),
            name: name.to_owned(),
            folder_type: ty,
            version: 1,
            state: if loaded {
                FolderState::Loaded { version: 1 }
            } else {
                FolderState::Unknown
            },
        }
    }

    /// A notecard item `id` in `folder`.
    fn notecard(id: u128, folder: u128, name: &str) -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(id)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(folder)),
            name: name.to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(0),
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            flags: 0,
            sale: SaleInfo::default(),
            creation_date: 0,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(OWN))),
            last_owner_id: Uuid::from_u128(0),
            creator_id: AgentKey::from(Uuid::from_u128(OWN)),
            group: None,
            permissions: Permissions5::default(),
        }
    }

    #[test]
    fn the_inventory_probe_resolves_a_folder_path() {
        let mut app = probe_world();
        let path = |segments: &[&str]| -> Vec<String> {
            segments
                .iter()
                .map(|segment| (*segment).to_owned())
                .collect()
        };
        assert_eq!(
            read_inventory(app.world(), InventoryRoot::Agent, &[]),
            Err(ProbeError::NoSuchFolder {
                path: Vec::new(),
                index: 0,
                segment: String::new(),
            }),
            "no root before the skeleton"
        );
        app.world_mut()
            .resource_mut::<InventoryModel>()
            .merge_folders(
                &[
                    folder(1, None, "My Inventory", FolderType::RootInventory, true),
                    folder(2, Some(1), "Notecards", FolderType::Notecard, false),
                    folder(3, Some(2), "Recipes", FolderType::None, false),
                ],
                false,
            );
        let unloaded = read_inventory(app.world(), InventoryRoot::Agent, &path(&["Notecards"]));
        assert_eq!(
            unloaded,
            Ok(InventoryFolderReadout {
                id: Uuid::from_u128(2),
                name: "Notecards".to_owned(),
                loaded: false,
                folders: vec![InventoryEntry {
                    id: Uuid::from_u128(3),
                    name: "Recipes".to_owned(),
                    kind: "none".to_owned(),
                }],
                items: Vec::new(),
            })
        );
        {
            let mut model = app.world_mut().resource_mut::<InventoryModel>();
            model.merge_folders(
                &[folder(3, Some(2), "Recipes", FolderType::None, true)],
                false,
            );
            model.set_items(
                InventoryFolderKey::from(Uuid::from_u128(3)),
                &[notecard(10, 3, "Pancakes")],
            );
        }
        let loaded = read_inventory(
            app.world(),
            InventoryRoot::Agent,
            &path(&["Notecards", "Recipes"]),
        );
        assert_eq!(
            loaded.map(|folder| (folder.loaded, folder.items)),
            Ok((
                true,
                vec![InventoryEntry {
                    id: Uuid::from_u128(10),
                    name: "Pancakes".to_owned(),
                    kind: "notecard".to_owned(),
                }]
            ))
        );
        assert_eq!(
            read_inventory(
                app.world(),
                InventoryRoot::Agent,
                &path(&["Notecards", "Nope"])
            ),
            Err(ProbeError::NoSuchFolder {
                path: path(&["Notecards", "Nope"]),
                index: 1,
                segment: "Nope".to_owned(),
            })
        );
        assert_eq!(
            read_inventory(app.world(), InventoryRoot::Library, &[]).map(|folder| folder.id),
            Err(ProbeError::NoSuchFolder {
                path: Vec::new(),
                index: 0,
                segment: String::new(),
            }),
            "no library root"
        );
    }

    /// Every notification in the history counts as on screen, showing one
    /// button of its own — the stand-in for the toast host the fixture world
    /// leaves out.
    fn every_notification_is_live(world: &mut World) -> Vec<(NotificationId, Vec<ToastButton>)> {
        world
            .get_resource::<NotificationManager>()
            .map_or_else(Vec::new, |manager| {
                manager
                    .history()
                    .map(|record| {
                        (
                            record.id,
                            vec![ToastButton::new("Mine", "Mine").default_choice()],
                        )
                    })
                    .collect()
            })
    }

    #[test]
    fn the_notification_probe_reads_text_buttons_and_answers() -> Result<(), TestError> {
        let mut app = probe_world();
        app.init_resource::<NotificationManager>();
        assert_eq!(read_notifications(app.world_mut()), Vec::new());
        let alert = template("GenericAlert").ok_or("no GenericAlert template")?;
        let id = {
            let mut manager = app.world_mut().resource_mut::<NotificationManager>();
            let id = manager.allocate_id();
            manager.push_history(NotificationRecord {
                id,
                template: alert.name,
                kind: alert.kind,
                body: "The region is restarting".to_owned(),
                response: None,
            });
            id
        };
        let offered: Vec<OfferedButton> = alert
            .form
            .iter()
            .map(|button| OfferedButton {
                name: button.name.to_owned(),
                label: crate::i18n_keys::english(button.label_key),
                default: button.is_default,
            })
            .collect();
        assert!(!offered.is_empty(), "GenericAlert offers a button");
        let expected =
            |buttons: Vec<OfferedButton>, live: bool, response: Option<&str>| NotificationReadout {
                id: id.get(),
                template: "GenericAlert".to_owned(),
                text: "The region is restarting".to_owned(),
                buttons,
                live,
                response: response.map(ToOwned::to_owned),
            };
        assert_eq!(
            read_notifications(app.world_mut()),
            vec![expected(offered.clone(), false, None)],
            "no toast host, so nothing is on screen: the template's buttons"
        );
        app.world_mut()
            .resource_mut::<ProbeSources>()
            .live_notifications = Some(every_notification_is_live);
        let shown = vec![OfferedButton {
            name: "Mine".to_owned(),
            label: "Mine".to_owned(),
            default: true,
        }];
        assert_eq!(
            read_notifications(app.world_mut()),
            vec![expected(shown.clone(), true, None)],
            "on screen, the buttons its card shows"
        );
        let answer = alert
            .default_button()
            .ok_or("GenericAlert has no default")?;
        app.world_mut()
            .resource_mut::<NotificationManager>()
            .record_response(id, Some(answer));
        assert_eq!(
            read_notifications(app.world_mut()),
            vec![expected(shown, true, Some(answer))]
        );
        Ok(())
    }

    #[test]
    fn the_quiescence_probe_needs_a_region() -> Result<(), TestError> {
        let mut app = probe_world();
        let before = read_quiescence(app.world_mut());
        assert!(!before.region_up);
        assert!(!before.is_quiet(), "nothing is quiet before a region");
        assert_eq!(
            before.outstanding,
            Some(0),
            "the scene source is registered, and the fixture has nothing in flight"
        );
        assert!(
            before.outstanding_by.is_empty(),
            "no bucket has work: {:?}",
            before.outstanding_by
        );
        assert_eq!(
            before.waiting_pipelines,
            Some(0),
            "the render-settle cell exists, and no render app writes it"
        );
        app.world_mut().spawn((
            SlRegion {
                handle: RegionHandle::new(0),
                sim: "127.0.0.1:9000".parse()?,
            },
            SlCurrentRegion,
        ));
        assert!(read_quiescence(app.world_mut()).is_quiet());
        Ok(())
    }

    #[test]
    fn the_event_log_returns_a_burst_in_order() {
        let mut app = probe_world();
        let cursor = app.world().resource::<EventLog>().cursor();
        // A burst in one frame: more events than any reader of a single frame
        // would think to expect, a command and a UI action among them.
        for index in 0..300_u32 {
            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::TeleportProgress {
                    message: format!("step {index}"),
                    teleport_flags: 0,
                }));
        }
        app.world_mut().write_message(SlCommand(Command::Stand));
        app.world_mut().write_message(UiAction {
            element: "toolbar",
            action: "stand",
        });
        settle(&mut app, 2);
        let page = app
            .world()
            .resource::<EventLog>()
            .read(cursor, &[], usize::MAX);
        let progress: Vec<String> = page
            .entries
            .iter()
            .filter(|entry| entry.kind == "TeleportProgress")
            .map(|entry| entry.detail.clone())
            .collect();
        assert_eq!(progress.len(), 300);
        for (index, detail) in progress.iter().enumerate() {
            assert!(
                detail.contains(&format!("\"step {index}\"")),
                "entry {index} out of order: {detail}"
            );
        }
        let seqs: Vec<u64> = page.entries.iter().map(|entry| entry.seq).collect();
        assert!(
            seqs.windows(2).all(|pair| pair.first() < pair.get(1)),
            "sequence numbers rise"
        );
        let command = page
            .entries
            .iter()
            .find(|entry| entry.stream == LogStream::Command)
            .map(|entry| entry.kind.as_str());
        assert_eq!(command, Some("Stand"));
        let action = page
            .entries
            .iter()
            .find(|entry| entry.stream == LogStream::UiAction)
            .map(|entry| entry.kind.as_str());
        assert_eq!(action, Some("toolbar.stand"));
        assert_eq!(page.dropped, 0);
        let again = app
            .world()
            .resource::<EventLog>()
            .read(page.next, &[], usize::MAX);
        assert!(
            again
                .entries
                .iter()
                .all(|entry| entry.kind != "TeleportProgress"),
            "a read from the returned cursor does not repeat the burst"
        );
    }
}

/// The probes over a **real session**: the whole viewer logged into the fake
/// grid, rendering into an off-screen window, with every source the assembly
/// registers.
#[cfg(test)]
mod full_stack {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{
        Bounds, ChatKind, ConversationRef, Locator, LogStream, SpeakerKind, TeleportState,
    };
    use sl_client_bevy::{
        AgentKey, ChatSource, ChatType, Command, ImDialog, InstantMessage, ObjectKey,
        RegionCoordinates, Uuid, Vector,
    };
    use sl_fake_grid::RegionConfig;
    use sl_viewer_automation::{
        EventLog, OVERLAY_COLOUR, find_all, read_agent, read_conversations, read_notifications,
        read_quiescence, read_status, request_screenshot, snapshot, take_screenshot,
    };

    use crate::full_stack_test::{HarnessOptions, ViewerHarness, stock_fixture};
    use crate::render_test::TestError;

    /// The region the account starts in.
    const HOME: &str = "Fake Region";
    /// A region ten away, so a teleport there is a real one rather than a
    /// hop to a neighbour the login already connected.
    const FAR: &str = "Fake Region Far";
    /// The off-screen window's size.
    const WINDOW: UVec2 = UVec2::new(640, 360);

    /// The other resident who sends an instant message.
    const OTHER: u128 = 0xB0B;
    /// The object that speaks in local chat.
    const DOOR: u128 = 0xD00;

    /// Every probe changes as the session does: the agent and the status bar
    /// on login, the scene settling, a chat line, an instant message and an
    /// alert from the grid reaching the transcripts, the notifications and the
    /// event log in order, a screenshot of the window with a box outlined, and
    /// a teleport walking the teleport readout to success in another region.
    #[test]
    fn the_state_probes_follow_a_real_session() -> Result<(), TestError> {
        let far = RegionConfig {
            name: FAR.to_owned(),
            grid_x: RegionConfig::default().grid_x.saturating_add(10),
            ..RegionConfig::default()
        };
        let mut harness = ViewerHarness::start_in_with(
            vec![
                stock_fixture().into_region(RegionConfig::default()),
                stock_fixture().into_region(far),
            ],
            HarnessOptions::in_offscreen_window(WINDOW),
        )?;
        harness.login()?;
        if !harness.wait_quiet()? {
            tracing::warn!("no GPU adapter: skipping the state probe check");
            return Ok(());
        }

        // Settled, logged in, standing where the login put it.
        let quiet = read_quiescence(harness.app_world_mut());
        assert!(quiet.is_quiet(), "the harness waited for quiet: {quiet:?}");
        let own = harness.agent()?.agent_id();
        let agent = read_agent(harness.app_world_mut());
        assert_eq!(agent.agent_id, Some(own.uuid()));
        let home = agent.region.clone().ok_or("no region after login")?;
        assert_eq!(home.name.as_deref(), Some(HOME));
        assert_eq!(
            Some(home.handle),
            harness.region_handle(HOME).map(|handle| handle.get())
        );
        assert_eq!(
            agent.teleport.map(|teleport| teleport.state),
            Some(TeleportState::Idle)
        );
        let [x, y, _z] = agent.position.ok_or("no position after login")?;
        assert!(
            (x - 128.0).abs() < 2.0 && (y - 128.0).abs() < 2.0,
            "the login placed the agent at the region centre, not {x},{y}"
        );
        assert_eq!(
            read_status(harness.app_world_mut()).region.as_deref(),
            Some(HOME)
        );
        harness.run_until("the status bar's parcel", |harness| {
            (read_status(harness.app_world_mut()).parcel.as_deref()
                == Some(sl_fake_grid::scenario::STOCK_PARCEL_NAME))
            .then_some(())
        })?;

        // The grid speaks: an object's shout, a resident's instant message
        // and a modal alert, then a marker to know all three have arrived.
        let cursor = harness.world().resource::<EventLog>().cursor();
        let grid_side = harness.agent()?;
        let now = grid_side.now();
        let other = AgentKey::from(Uuid::from_u128(OTHER));
        let door = ObjectKey::from(Uuid::from_u128(DOOR));
        harness.grid(grid_side.with_sim(|sim| -> Result<(), sl_proto::Error> {
            sim.send_chat_from_simulator(
                "Door",
                ChatSource::Object(door),
                Uuid::nil(),
                ChatType::Shout,
                1,
                Vector {
                    x: 130.0,
                    y: 128.0,
                    z: 26.0,
                },
                "Locked",
                now,
            )?;
            sim.send_instant_message(
                &InstantMessage {
                    from_agent_id: other,
                    from_agent_name: "Two Resident".to_owned(),
                    to_agent_id: own,
                    dialog: ImDialog::Message,
                    from_group: false,
                    region_id: None,
                    position: RegionCoordinates::new(0.0, 0.0, 0.0),
                    offline: false,
                    timestamp: None,
                    id: Uuid::from_u128(OTHER ^ 0xA),
                    parent_estate_id: 0,
                    message: "psst".to_owned(),
                    binary_bucket: Vec::new(),
                },
                now,
            )?;
            sim.send_agent_alert_message(own, true, "Probe alert", now)
        }))?;
        harness.mark("probes")?;
        harness.wait_marker("probes")?;
        let conversations = harness.run_until("the transcripts", |harness| {
            let conversations = read_conversations(harness.app_world_mut()).ok()?;
            let heard = |conversation: ConversationRef| {
                conversations.iter().any(|readout| {
                    readout.conversation == conversation && !readout.lines.is_empty()
                })
            };
            (heard(ConversationRef::Nearby) && heard(ConversationRef::Direct(other.uuid())))
                .then_some(conversations)
        })?;
        let nearby = conversations
            .iter()
            .find(|readout| readout.conversation == ConversationRef::Nearby)
            .ok_or("no nearby transcript")?;
        let shout = nearby
            .lines
            .iter()
            .find(|line| line.text == "Locked")
            .ok_or("the shout is not in the nearby transcript")?;
        assert_eq!(shout.speaker, "Door");
        assert_eq!(shout.speaker_id, Some(door.uuid()));
        assert_eq!(shout.speaker_kind, SpeakerKind::Object);
        assert_eq!(shout.chat_kind, Some(ChatKind::Shout));
        assert!(!shout.own);
        let direct = conversations
            .iter()
            .find(|readout| readout.conversation == ConversationRef::Direct(other.uuid()))
            .ok_or("no direct transcript")?;
        assert_eq!(
            direct
                .lines
                .iter()
                .map(|line| (line.speaker.as_str(), line.text.as_str(), line.own))
                .collect::<Vec<_>>(),
            [("Two Resident", "psst", false)]
        );
        assert_eq!(
            direct.unread, 1,
            "it arrived while Nearby was the active tab"
        );

        let alert = harness.run_until("the alert toast", |harness| {
            read_notifications(harness.app_world_mut())
                .into_iter()
                .find(|notification| notification.text.contains("Probe alert") && notification.live)
        })?;
        assert_eq!(alert.template, "GenericAlert");
        assert_eq!(alert.response, None);
        let button = alert
            .buttons
            .iter()
            .find(|button| button.default)
            .ok_or("the alert offers no default button")?;
        // …and the button the readout names is a node a locator reaches.
        let roots = snapshot(harness.app_world_mut())?;
        let nodes = find_all(
            &roots,
            &Locator::test_id(format!("toast-button:{}", button.name)),
        )?;
        assert_eq!(nodes.len(), 1, "the offered button is one semantic node");

        let page =
            harness
                .world()
                .resource::<EventLog>()
                .read(cursor, &[LogStream::Event], usize::MAX);
        let kinds: Vec<&str> = page
            .entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .filter(|kind| {
                [
                    "ChatReceived",
                    "InstantMessageReceived",
                    "AgentAlertMessage",
                    "GenericMessage",
                ]
                .contains(kind)
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "ChatReceived",
                "InstantMessageReceived",
                "AgentAlertMessage",
                "GenericMessage"
            ],
            "the event log holds the grid's three messages and the marker, in order"
        );
        assert_eq!(page.dropped, 0);

        // A screenshot of the window, a box outlined on it.
        let outlined = Bounds {
            x: 20.0,
            y: 30.0,
            width: 100.0,
            height: 40.0,
        };
        let ticket = request_screenshot(harness.app_world_mut(), vec![outlined]);
        let frame = harness.run_until("the screenshot", |harness| {
            take_screenshot(harness.app_world_mut(), ticket)
        })??;
        assert_eq!((frame.width, frame.height), (WINDOW.x, WINDOW.y));
        assert_eq!(
            frame.pixel(20, 30),
            Some(OVERLAY_COLOUR),
            "the box's corner"
        );
        assert_eq!(frame.pixel(119, 69), Some(OVERLAY_COLOUR), "its far corner");
        let drawn = (0..WINDOW.y)
            .step_by(8)
            .flat_map(|y| (0..WINDOW.x).step_by(8).map(move |x| (x, y)))
            .filter(|(x, y)| {
                frame
                    .pixel(*x, *y)
                    .is_some_and(|[r, g, b, _a]| r | g | b > 16)
            })
            .count();
        assert!(
            drawn > 100,
            "the window frame is black: {drawn} lit samples"
        );
        let png = frame.to_png()?;
        assert_eq!(png.get(1..4), Some(b"PNG".as_slice()));

        // A teleport, watched a frame at a time.
        let far_handle = harness.region_handle(FAR).ok_or("no far region")?;
        let before_teleport = harness.world().resource::<EventLog>().cursor();
        harness.command(Command::Teleport {
            region_handle: far_handle,
            position: RegionCoordinates::new(128.0, 128.0, 26.0),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        });
        let mut seen: Vec<TeleportState> = Vec::new();
        let mut last_region = None;
        let arrival = harness.run_until("the teleport's success in the far region", |harness| {
            let agent = read_agent(harness.app_world_mut());
            if let Some(state) = agent.teleport.map(|teleport| teleport.state)
                && seen.last() != Some(&state)
            {
                seen.push(state);
            }
            last_region = agent.region;
            let arrived = last_region
                .as_ref()
                .and_then(|region| region.name.as_deref())
                == Some(FAR);
            (arrived && seen.contains(&TeleportState::Succeeded)).then_some(())
        });
        if let Err(error) = arrival {
            let handover: Vec<String> = harness
                .world()
                .resource::<EventLog>()
                .read(before_teleport, &[LogStream::Event], usize::MAX)
                .entries
                .into_iter()
                .filter(|entry| {
                    ["RegionInfoHandshake", "RegionChanged", "TeleportFinished"]
                        .contains(&entry.kind.as_str())
                })
                .map(|entry| {
                    let handle = entry
                        .detail
                        .find("region_handle")
                        .and_then(|at| entry.detail.get(at..at.saturating_add(40)))
                        .unwrap_or_default()
                        .to_owned();
                    format!("{} {handle}", entry.kind)
                })
                .collect();
            return Err(format!(
                "{error}\n  teleport states seen: {seen:?}\n  last region: {last_region:?}\n  \
                 handover events: {handover:#?}"
            )
            .into());
        }
        assert!(
            !seen
                .iter()
                .any(|state| matches!(state, TeleportState::Failed { .. })),
            "the teleport went through a failure: {seen:?}"
        );
        assert!(
            seen.iter().any(|state| matches!(
                state,
                TeleportState::Requested | TeleportState::InProgress | TeleportState::Arriving
            )),
            "the readout jumped straight to the end: {seen:?}"
        );
        assert_eq!(
            read_status(harness.app_world_mut()).region.as_deref(),
            Some(FAR)
        );
        Ok(())
    }
}
