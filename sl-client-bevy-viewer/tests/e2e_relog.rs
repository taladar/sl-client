//! End-to-end tests for what must survive a relog: a stage viewer logs out
//! and back in within one test, keeping its directories
//! ([`Stage::relog`]), on both backends ([[test-e2e-sweep-relog]]).
//!
//! - a moved and resized window comes back where it was left;
//! - a group notice left unanswered comes back, and once answered does not;
//! - a blacklisted object stays derendered, and a temporary entry clears on a
//!   teleport;
//! - a contact set's settings — its checkboxes and its own reply — persist,
//!   and its online-first order holds;
//! - the alerts tab's choices hold: a suppressed confirmation answers itself,
//!   group-notice toasts stay off, and an offer is accepted unasked.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{Bounds, Locator, NodeValue, Probe, Role};
    use sl_e2e::{BodyError, FIRST_NAME, Need, Stage, StageBuilder};
    use sl_fake_grid::{AccountConfig, RegionConfig};
    use sl_proto::{
        AgentKey, AssetType, FriendKey, GroupKey, GroupNoticeReceived, ImDialog, InstantMessage,
        InventoryItemOrFolderKey, InventoryKey, InventoryOffer, ObjectKey, RegionCoordinates,
        RegionLocalObjectId, ServerEvent, Uuid,
    };
    use sl_viewer_driver::{UiLocator, Viewer};
    use tokio::sync::broadcast;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// How long a teleport may take, handover included.
    const TELEPORT: Duration = Duration::from_secs(120);

    /// A stage for `name` with the viewers `labels`.
    fn stage(name: &str, labels: &[&str]) -> StageBuilder {
        labels.iter().fold(
            StageBuilder::new(name).viewer_binary(VIEWER),
            |builder, label| builder.viewer(*label),
        )
    }

    /// Send viewer `label` `im` from the grid.
    async fn deliver(stage: &Stage, label: &str, im: &InstantMessage) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| sim.send_instant_message(im, now))
            .await
            .map_err(|error| format!("delivering the IM: {error}"))?;
        Ok(())
    }

    /// Wait until the grid's session reports an event `wanted` accepts: the
    /// viewer's `what` reaching the grid.
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
            Err(_elapsed) => Err(format!("the grid never heard the {what}").into()),
        }
    }

    /// Whether `viewer` shows a notification of `template` now — its
    /// history, which the readout includes, aside.
    async fn shows(viewer: &Viewer, template: &str) -> Result<bool, BodyError> {
        Ok(viewer
            .notifications()
            .await?
            .iter()
            .any(|notification| notification.live && notification.template == template))
    }

    // ---- Window geometry ----------------------------------------------------

    /// How far apart two box edges may be and still be the same place: the
    /// stored rect is in logical pixels, and layout rounds.
    const EDGE_SLOP: f32 = 1.5;

    /// Whether `left` and `right` are the same box, edge for edge.
    fn same_box(left: &Bounds, right: &Bounds) -> bool {
        (left.x - right.x).abs() <= EDGE_SLOP
            && (left.y - right.y).abs() <= EDGE_SLOP
            && (left.width - right.width).abs() <= EDGE_SLOP
            && (left.height - right.height).abs() <= EDGE_SLOP
    }

    /// The bounds of the one window `window` names, once it shows.
    async fn shown_bounds(viewer: &Viewer, window: &UiLocator) -> Result<Bounds, BodyError> {
        let shown = viewer.expect(window).to_be_visible().await?;
        Ok(shown.first().ok_or("the window showed as no node")?.bounds)
    }

    /// **Window geometry**: the inventory window, dragged by its title bar and
    /// resized by its grip, opens again after a relog where it was left and
    /// as big as it was.
    #[test]
    fn a_moved_and_resized_window_comes_back_where_it_was_left() -> Result<(), TestError> {
        stage("window_geometry", &["Alpha"]).run(async |stage: &Stage| {
            let alpha = stage.viewer("Alpha")?;
            let window = alpha.open_floater("inventory").await?;
            let opened = shown_bounds(&alpha, &window).await?;
            let _moved = window
                .test_id("floater-title-bar")
                .drag_by(140.0, 90.0)
                .await?;
            let _resized = window.test_id("floater-resize").drag_by(60.0, 40.0).await?;
            let left = window.node().await?.bounds;
            assert!(
                (left.x - opened.x - 140.0).abs() <= EDGE_SLOP
                    && (left.y - opened.y - 90.0).abs() <= EDGE_SLOP,
                "the title bar moved the window: {opened:?} → {left:?}"
            );
            assert!(
                (left.width - opened.width - 60.0).abs() <= EDGE_SLOP
                    && (left.height - opened.height - 40.0).abs() <= EDGE_SLOP,
                "the grip resized the window: {opened:?} → {left:?}"
            );

            let alpha = stage.relog("Alpha").await?;
            let back = shown_bounds(&alpha, &alpha.ui().window("inventory")).await?;
            assert!(
                same_box(&back, &left),
                "the window came back as {back:?}, not where it was left: {left:?}"
            );
            Ok(())
        })?;
        Ok(())
    }

    // ---- Notification persistence -------------------------------------------

    /// The template a group notice's card reports as.
    const GROUP_NOTICE: &str = "GroupNotice";

    /// The group the notices are posted to: nobody's, which a notice does
    /// not check.
    const NOTICE_GROUP: u128 = 0x6E0_0000_0000_0000_0000_0000_0000_0001;

    /// A group notice posted to viewer `label`, `subject` its subject and
    /// `id` its notice id.
    fn group_notice(
        stage: &Stage,
        label: &str,
        subject: &str,
        id: u128,
    ) -> Result<InstantMessage, BodyError> {
        let notice = GroupNoticeReceived {
            group_id: GroupKey::from(Uuid::from_u128(NOTICE_GROUP)),
            sender_name: "Notice Poster".to_owned(),
            subject: subject.to_owned(),
            body: "Read me after the relog.".to_owned(),
            timestamp: Some(1_790_000_000),
            attachment: None,
        };
        Ok(notice.instant_message(Uuid::from_u128(id), stage.agent_id(label)?)?)
    }

    /// **Persistent notifications**: a group notice left unanswered shows
    /// again after a relog; answered, it does not come back after the next.
    #[test]
    fn an_unanswered_notice_comes_back_after_a_relog_and_an_answered_one_does_not()
    -> Result<(), TestError> {
        stage("notification_persistence", &["Alpha"])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                deliver(
                    stage,
                    "Alpha",
                    &group_notice(stage, "Alpha", "Meeting", 0x6E01)?,
                )
                .await?;
                let _shown = alpha
                    .expect_notification()
                    .timeout(WAIT)
                    .to_show(GROUP_NOTICE)
                    .await?;

                let alpha = stage.relog("Alpha").await?;
                let _again = alpha
                    .expect_notification()
                    .timeout(WAIT)
                    .to_show(GROUP_NOTICE)
                    .await?;
                let ok = alpha.ui().test_id("group-notice-button:OK");
                let _answered = ok.click().await?;
                let _closed = alpha.expect(&ok).to_be_detached().await?;
                assert!(
                    !shows(&alpha, GROUP_NOTICE).await?,
                    "the answered notice is still shown"
                );

                let alpha = stage.relog("Alpha").await?;
                assert!(
                    !shows(&alpha, GROUP_NOTICE).await?,
                    "the answered notice came back after the next relog"
                );
                Ok(())
            })?;
        Ok(())
    }

    // ---- Derender blacklist -------------------------------------------------

    /// The region east of the derender test's home.
    const NEIGHBOUR: &str = "Neighbour";

    /// The box that stays rendered: the scene arrived once it shows.
    const KEPT_BOX: &str = "Kept Box";

    /// The box derendered for good.
    const BLACKLISTED_BOX: &str = "Blacklisted Box";

    /// The box derendered until the next teleport.
    const PASSING_BOX: &str = "Passing Box";

    /// Put a box named `name` on the grid beside the stock one — region-local
    /// id `local`, `offset` metres east of it — owned by somebody else, and
    /// show it to viewer `label`. The region keeps it for every later session.
    async fn place_box(
        stage: &Stage,
        label: &str,
        name: &str,
        local: u32,
        offset: f32,
    ) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        let mut placed = sl_fake_grid::world::box_prim(
            RegionLocalObjectId(local),
            ObjectKey::from(Uuid::from_u128(0xDE2E_0000 | u128::from(local))),
            AgentKey::from(Uuid::from_u128(0xDE2E_0A11)),
            sl_proto::Vector {
                x: 130.0 + offset,
                y: 124.0,
                z: 25.5,
            },
            sl_proto::Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        let mut properties = sl_fake_grid::world::default_object_properties(&placed);
        properties.name = name.to_owned();
        placed.properties = Some(properties);
        agent
            .with_world(|world, sim| {
                world.objects.push(placed.clone());
                sl_fake_grid::world::send_objects(sim, &[placed], now)
            })
            .await
            .map_err(|error| format!("showing {name}: {error}"))?;
        Ok(())
    }

    /// Derender the object `name` from its pie: More ▸ More ▸ Derender ▸
    /// `how` (`pie-object-blacklist` or `pie-object-temporary`), and wait
    /// until it is gone from the scene.
    async fn derender(viewer: &Viewer, name: &str, how: &str) -> Result<(), BodyError> {
        let object = viewer.world().object_named(name).timeout(WAIT);
        let _pie = object.open_pie().await?;
        for key in [
            "pie-object-more",
            "pie-object-more",
            "pie-object-derender",
            how,
        ] {
            let _slice = viewer
                .pie_slice(Locator::role(Role::MenuItem).name_key(key))
                .await?;
        }
        let _gone = viewer.expect_world(&object).to_be_detached().await?;
        Ok(())
    }

    /// Open the Asset Blacklist window and expect it to count `total`
    /// entries, every one of them shown — each number in the isolation marks
    /// Fluent puts around a placeable.
    async fn expect_blacklisted(viewer: &Viewer, total: usize) -> Result<(), BodyError> {
        let window = viewer.open_floater("asset-blacklist").await?;
        let _counted = viewer
            .expect(&window.test_id("blacklist-count"))
            .to_have_text(&format!(
                "\u{2068}{total}\u{2069} of \u{2068}{total}\u{2069} blacklisted"
            ))
            .await?;
        // Out of the way of the scene the test goes on to click into.
        let _closed = window.test_id("floater-button:close").click().await?;
        let _hidden = viewer.expect(&window).to_be_hidden().await?;
        Ok(())
    }

    /// **Derender**: a blacklisted object stays derendered after a relog,
    /// while one derendered only for now is back; derendered for now again,
    /// its entry clears on the next teleport and the blacklisted one's stays.
    #[test]
    fn a_blacklisted_object_stays_derendered_and_a_temporary_entry_clears_on_a_teleport()
    -> Result<(), TestError> {
        let home = RegionConfig::default();
        let neighbour = RegionConfig {
            name: NEIGHBOUR.to_owned(),
            grid_x: home.grid_x.saturating_add(1),
            ..RegionConfig::default()
        };
        stage("derender_relog", &["Alpha"])
            .region(home)
            .region(neighbour)
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                for (name, local, offset) in [
                    (KEPT_BOX, 0xDE01, 0.0),
                    (BLACKLISTED_BOX, 0xDE02, 2.0),
                    (PASSING_BOX, 0xDE03, 4.0),
                ] {
                    place_box(stage, "Alpha", name, local, offset).await?;
                }
                derender(&alpha, BLACKLISTED_BOX, "pie-object-blacklist").await?;
                derender(&alpha, PASSING_BOX, "pie-object-temporary").await?;
                expect_blacklisted(&alpha, 2).await?;

                let alpha = stage.relog("Alpha").await?;
                let _kept = alpha
                    .world()
                    .object_named(KEPT_BOX)
                    .timeout(WAIT)
                    .node()
                    .await?;
                let _passing = alpha
                    .world()
                    .object_named(PASSING_BOX)
                    .timeout(WAIT)
                    .node()
                    .await?;
                assert_eq!(
                    alpha.world().object_named(BLACKLISTED_BOX).count().await?,
                    0,
                    "the blacklisted box is back after the relog"
                );
                expect_blacklisted(&alpha, 1).await?;

                derender(&alpha, PASSING_BOX, "pie-object-temporary").await?;
                expect_blacklisted(&alpha, 2).await?;
                teleport_by_map(&alpha, NEIGHBOUR).await?;
                expect_blacklisted(&alpha, 1).await?;
                let window = alpha.open_floater("asset-blacklist").await?;
                let _permanent = alpha
                    .expect(
                        &window.get(Locator::role(Role::ListItem).name_containing(BLACKLISTED_BOX)),
                    )
                    .to_be_visible()
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    /// Teleport to `region` through the world map's search, and wait until
    /// the agent is there.
    async fn teleport_by_map(viewer: &Viewer, region: &str) -> Result<(), BodyError> {
        let map = viewer.open_floater("worldmap").await?;
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
        let _arrived = viewer
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(TELEPORT)
            .to_equal(json!(region))
            .await?;
        Ok(())
    }

    // ---- Contact sets ---------------------------------------------------------

    /// The contact set the tests make.
    const SET: &str = "Inner Circle";

    /// The friend whose name sorts first, and who stays offline.
    const FIRST_FRIEND: &str = "Alfa";

    /// The friend whose name sorts last, and who comes online.
    const ONLINE_FRIEND: &str = "Zulu";

    /// The set's own Unavailable reply.
    const SET_REPLY: &str = "Back soon, circle.";

    /// The `First Last` name of the stage account `last`.
    fn account(last: &str) -> String {
        format!("{FIRST_NAME} {last}")
    }

    /// The Conversations window, opened on its People tab's `tab`.
    async fn people_tab(viewer: &Viewer, tab: &str) -> Result<UiLocator, BodyError> {
        let window = viewer.open_floater("conversations").await?;
        let _people = window
            .get(Locator::role(Role::Tab).name_key("people-tab"))
            .click()
            .await?;
        let _tab = window
            .get(Locator::role(Role::Tab).name_key(tab))
            .click()
            .await?;
        Ok(window)
    }

    /// Make the contact set `name` from the Contact Sets tab's New Set…, and
    /// leave the tab showing it.
    async fn new_set(viewer: &Viewer, name: &str) -> Result<UiLocator, BodyError> {
        let window = people_tab(viewer, "people-contact-sets-tab").await?;
        let _asked = window.button_key("contact-sets-action-new").click().await?;
        let _named = viewer.ui().test_id("toast-input:field").fill(name).await?;
        let _created = viewer.ui().test_id("toast-button:Create").click().await?;
        choose_set(viewer, &window, name).await?;
        Ok(window)
    }

    /// Show the set `name` in the Contact Sets tab of `window`.
    async fn choose_set(viewer: &Viewer, window: &UiLocator, name: &str) -> Result<(), BodyError> {
        let chooser = window.test_id("contact-sets-chooser:combo");
        let _picked = chooser
            .select_option(Locator::role(Role::ListItem).named(name))
            .await?;
        let _shown = viewer.expect(&chooser).to_contain_text(name).await?;
        Ok(())
    }

    /// File the friend `last` under the one contact set there is, from their
    /// profile: Friends ▸ the row ▸ Profile ▸ Add to Set… ▸ Add.
    async fn file_friend(viewer: &Viewer, stage: &Stage, last: &str) -> Result<(), BodyError> {
        let window = people_tab(viewer, "people-friends-tab").await?;
        let _row = window
            .get(Locator::role(Role::ListItem).name_containing(account(last)))
            .timeout(WAIT)
            .click()
            .await?;
        let _profile = window.button_key("people-action-profile").click().await?;
        let friend = stage
            .grid()?
            .account_agent_id(FIRST_NAME, last)
            .ok_or("no such friend")?;
        let profile = viewer
            .ui()
            .window(&format!("avatar-profile#{}", friend.uuid()));
        let _add = profile
            .button_key("profile-add-to-contact-set")
            .click()
            .await?;
        let _filed = viewer
            .ui()
            .window("add-to-contact-set")
            .button_key("add-to-contact-set-add")
            .click()
            .await?;
        let _closed = profile.test_id("floater-button:close").click().await?;
        Ok(())
    }

    /// Tell viewer `label` that the friend `last` is online, as a grid's
    /// friends service would.
    async fn announce_online(stage: &Stage, label: &str, last: &str) -> Result<(), BodyError> {
        let friend = stage
            .grid()?
            .account_agent_id(FIRST_NAME, last)
            .ok_or("no such friend")?;
        let agent = stage.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| sim.send_online_notification(&[FriendKey::from(friend.uuid())], now))
            .await
            .map_err(|error| format!("announcing {last} online: {error}"))?;
        Ok(())
    }

    /// Expect the Contact Sets tab of `window` to list the friend `last` on
    /// top.
    async fn expect_on_top(
        viewer: &Viewer,
        window: &UiLocator,
        last: &str,
    ) -> Result<(), BodyError> {
        let top = window
            .test_id("people-contact-sets-content")
            .get(Locator::role(Role::ListItem))
            .nth(0);
        let _top = viewer.expect(&top).to_contain_text(&account(last)).await?;
        Ok(())
    }

    /// A checkbox of the set's settings window, by its caption's key.
    fn config_check(viewer: &Viewer, key: &str) -> UiLocator {
        viewer
            .ui()
            .window("contact-set-config")
            .get(Locator::role(Role::Checkbox).name_key(key))
    }

    /// The set's own Unavailable reply field.
    fn busy_reply(viewer: &Viewer) -> UiLocator {
        viewer
            .ui()
            .window("contact-set-config")
            .test_id("contact-set-config-reply-busy-field:field")
    }

    /// The set's three settings this test turns on.
    const SET_CHECKS: [&str; 3] = [
        "contact-set-config-notify",
        "contact-set-config-sort-online",
        "contact-set-config-reply-busy",
    ];

    /// **Contact sets**: a set's settings window keeps its ticks and its own
    /// reply across a relog, and a set sorted online-first lists the online
    /// friend above the one whose name comes first — before and after.
    #[test]
    fn a_contact_sets_settings_and_online_first_order_survive_a_relog() -> Result<(), TestError> {
        stage("contact_sets_relog", &["Alpha"])
            .needs(Need::GridControl)
            .configure_grid(|grid| {
                [FIRST_FRIEND, ONLINE_FRIEND]
                    .into_iter()
                    .fold(grid, |grid, last| {
                        grid.account(AccountConfig::new(FIRST_NAME, last, "password"))
                            .friends((FIRST_NAME, "Alpha"), (FIRST_NAME, last))
                    })
            })
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                announce_online(stage, "Alpha", ONLINE_FRIEND).await?;
                let _set = new_set(&alpha, SET).await?;
                for last in [FIRST_FRIEND, ONLINE_FRIEND] {
                    file_friend(&alpha, stage, last).await?;
                }
                let window = people_tab(&alpha, "people-contact-sets-tab").await?;
                choose_set(&alpha, &window, SET).await?;
                expect_on_top(&alpha, &window, FIRST_FRIEND).await?;

                let _configure = window
                    .button_key("contact-sets-action-configure")
                    .click()
                    .await?;
                for key in SET_CHECKS {
                    config_check(&alpha, key).check().await?;
                }
                let _typed = busy_reply(&alpha).fill(SET_REPLY).await?;
                // The reply commits when the field loses the focus.
                alpha.press("Tab").await?;
                expect_on_top(&alpha, &window, ONLINE_FRIEND).await?;

                let alpha = stage.relog("Alpha").await?;
                announce_online(stage, "Alpha", ONLINE_FRIEND).await?;
                let window = people_tab(&alpha, "people-contact-sets-tab").await?;
                choose_set(&alpha, &window, SET).await?;
                expect_on_top(&alpha, &window, ONLINE_FRIEND).await?;
                let _configure = window
                    .button_key("contact-sets-action-configure")
                    .click()
                    .await?;
                for key in SET_CHECKS {
                    let _ticked = alpha
                        .expect(&config_check(&alpha, key))
                        .to_be_checked()
                        .await?;
                }
                assert_eq!(
                    busy_reply(&alpha).value().await?,
                    Some(NodeValue::Text(SET_REPLY.to_owned())),
                    "the set's own reply after the relog"
                );
                Ok(())
            })?;
        Ok(())
    }

    // ---- Alerts tab -------------------------------------------------------

    /// The alert row whose confirmation the test suppresses: Delete Set's.
    const REMOVE_SET_ROW: &str = "Confirm before removing a contact set";

    /// The template of that confirmation.
    const REMOVE_SET: &str = "RemoveContactSet";

    /// The set the alerts test makes and deletes.
    const SHORT_SET: &str = "Short Lived";

    /// Who offers the alerts test an item: nobody the grid knows.
    const GIVER: u128 = 0x61F7_0000_0000_0000_0000_0000_0000_0001;

    /// The checkbox of the preferences row `key`.
    fn preference(viewer: &Viewer, key: &str) -> UiLocator {
        viewer
            .ui()
            .window("preferences")
            .test_id(&format!("preferences:row:{key}"))
            .role(Role::Checkbox)
    }

    /// The alerts list's Show checkbox for Delete Set's confirmation, found by
    /// filtering the list down to it with the window's search box.
    async fn remove_set_alert(viewer: &Viewer) -> Result<UiLocator, BodyError> {
        let window = viewer.ui().window("preferences");
        let _filtered = window
            .test_id("preferences-search:search")
            .role(Role::Textbox)
            .fill(REMOVE_SET_ROW)
            .await?;
        Ok(window.get(Locator::role(Role::Checkbox).named(REMOVE_SET_ROW)))
    }

    /// Preferences open on the Alerts tab.
    async fn alerts_tab(viewer: &Viewer) -> Result<UiLocator, BodyError> {
        let window = viewer.open_floater("preferences").await?;
        let _tab = window
            .get(Locator::role(Role::Tab).name_key("preferences-tab-alerts"))
            .click()
            .await?;
        Ok(window)
    }

    /// An inventory offer of a notecard to viewer `label` from the giver.
    fn offer(stage: &Stage, label: &str) -> Result<InstantMessage, BodyError> {
        let giver = AgentKey::from(Uuid::from_u128(GIVER));
        let offer = InventoryOffer {
            asset_type: AssetType::Notecard,
            item_id: InventoryItemOrFolderKey::Item(InventoryKey::from(Uuid::from_u128(
                GIVER ^ 0x17E,
            ))),
            transaction_id: Uuid::from_u128(GIVER ^ 0x7A),
            from_agent_id: giver,
            from_task: false,
        };
        Ok(InstantMessage {
            from_agent_id: giver,
            from_agent_name: "Giver Resident".to_owned(),
            to_agent_id: stage.agent_id(label)?,
            dialog: ImDialog::InventoryOffered,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(128.0, 128.0, 25.0),
            offline: false,
            timestamp: None,
            id: offer.transaction_id,
            parent_estate_id: 1,
            message: "A Gift".to_owned(),
            binary_bucket: offer.binary_bucket()?,
        })
    }

    /// **Alerts tab**: with Delete Set's confirmation unticked, group-notice
    /// toasts off and offers auto-accepted, a relog keeps all three — the
    /// tab shows them so, Delete Set deletes without asking, a group notice
    /// raises no card, and an offer is accepted with nobody asked.
    #[test]
    fn the_alerts_tabs_choices_hold_across_a_relog() -> Result<(), TestError> {
        stage("alerts_relog", &["Alpha"])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                let window = alerts_tab(&alpha).await?;
                preference(&alpha, "preferences-row-group-notice-toasts")
                    .uncheck()
                    .await?;
                preference(&alpha, "preferences-row-auto-accept-inventory")
                    .check()
                    .await?;
                remove_set_alert(&alpha).await?.uncheck().await?;
                let _ok = window
                    .test_id("preferences:button:preferences-ok")
                    .click()
                    .await?;
                let _closed = alpha.expect(&window).to_be_hidden().await?;

                let alpha = stage.relog("Alpha").await?;
                let window = alerts_tab(&alpha).await?;
                let _off = alpha
                    .expect(&preference(&alpha, "preferences-row-group-notice-toasts"))
                    .to_be_unchecked()
                    .await?;
                let _on = alpha
                    .expect(&preference(&alpha, "preferences-row-auto-accept-inventory"))
                    .to_be_checked()
                    .await?;
                let _kept = alpha
                    .expect(&remove_set_alert(&alpha).await?)
                    .to_be_unchecked()
                    .await?;
                let _cancel = window
                    .test_id("preferences:button:preferences-cancel")
                    .click()
                    .await?;

                // The suppressed confirmation answers itself with its default:
                // the set goes, with nothing asked.
                let sets = new_set(&alpha, SHORT_SET).await?;
                let _deleted = sets
                    .button_key("contact-sets-action-delete")
                    .click()
                    .await?;
                let chooser = sets.test_id("contact-sets-chooser:combo");
                let _gone = alpha.expect(&chooser).to_contain_text("All Sets").await?;
                assert!(
                    !shows(&alpha, REMOVE_SET).await?,
                    "Delete Set asked for a confirmation that was suppressed"
                );

                // A group notice raises no card, and an offer is accepted.
                let mut heard = stage.agent("Alpha").await?.events();
                deliver(
                    stage,
                    "Alpha",
                    &group_notice(stage, "Alpha", "Unseen", 0x6E02)?,
                )
                .await?;
                deliver(stage, "Alpha", &offer(stage, "Alpha")?).await?;
                grid_hears(&mut heard, "acceptance of the offer", |event| {
                    matches!(event, ServerEvent::InstantMessage(im)
                        if im.dialog == ImDialog::InventoryAccepted
                            && im.id == Uuid::from_u128(GIVER ^ 0x7A))
                })
                .await?;
                stage.mark("Alpha", "after-the-offer").await?;
                stage.wait_marker("Alpha", "after-the-offer", WAIT).await?;
                assert!(
                    !shows(&alpha, GROUP_NOTICE).await?,
                    "a group notice raised a card with its toasts off"
                );
                Ok(())
            })?;
        Ok(())
    }
}
