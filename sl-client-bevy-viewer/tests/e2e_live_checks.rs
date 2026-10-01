//! End-to-end tests for checks that used to be left to "check it live": what a
//! person verified by logging in and pressing keys, on the fake grid, through
//! the automation driver alone, on both backends
//! ([[test-e2e-live-verify-sweep]]).
//!
//! - every window chord the menu bar draws opens its window and closes it
//!   again, and the flycam chord toggles the flycam;
//! - the Build menu's chords send the selection's undo and redo, link two
//!   prims and unlink them, and Avatar ▸ Quit's chord logs the viewer out;
//! - the About window's tabs switch panes, a license row shows its text, Copy
//!   to Clipboard pastes the support block, the Help menu ticks the window,
//!   and the Region line follows a teleport;
//! - while Unavailable, an IM is answered once and an offer is held until the
//!   mode ends;
//! - the radar sorts on a column click, filters by name, and opens a row's
//!   profile and an IM with it.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::json;
    use sl_automation_proto::{CameraView, Locator, LogStream, Probe, Role};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_fake_grid::RegionConfig;
    use sl_fake_grid::fixtures::scenarios;
    use sl_proto::{AgentKey, ImDialog, InstantMessage, RegionCoordinates, ServerEvent, Uuid};
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

    /// The catalogue scene as the home region: the one with the NPCs.
    fn catalogue() -> Result<RegionConfig, TestError> {
        let scenario =
            scenarios::scenario("catalogue").ok_or("the catalogue scenario is not registered")?;
        Ok(scenario.dress(RegionConfig::default()))
    }

    // ---- Menu accelerators ------------------------------------------------

    /// Each window chord the menu bar draws, and the window it toggles.
    const WINDOW_CHORDS: [(&str, &str); 8] = [
        ("Ctrl+P", "preferences"),
        ("Ctrl+I", "inventory"),
        ("Ctrl+T", "conversations"),
        ("Ctrl+M", "worldmap"),
        ("Ctrl+B", "build-tools"),
        ("Alt+P", "phototools"),
        ("Ctrl+Alt+Shift+S", "debug_settings"),
        ("Ctrl+F", "search"),
    ];

    /// **Window accelerators**: each chord the menu bar draws beside a window
    /// entry opens that window, and pressed again closes it; the flycam chord
    /// switches the camera to the flycam and back.
    #[test]
    fn every_window_chord_on_the_menu_bar_opens_and_closes_its_window() -> Result<(), TestError> {
        stage("window_chords", &["Alpha"]).run(async |stage: &Stage| {
            let alpha = stage.viewer("Alpha")?;
            for (chord, floater) in WINDOW_CHORDS {
                let window = alpha.ui().window(floater);
                assert!(
                    !window.is_visible().await?,
                    "{floater} is open before {chord} opened it"
                );
                alpha.press(chord).await?;
                let _open = alpha.expect(&window).to_be_visible().await?;
            }
            for (chord, floater) in WINDOW_CHORDS {
                alpha.press(chord).await?;
                let _closed = alpha
                    .expect(&alpha.ui().window(floater))
                    .to_be_hidden()
                    .await?;
            }

            let camera = alpha.expect_state(Probe::Agent).at("/camera");
            let resting = alpha.agent().await?.camera;
            assert_ne!(resting, Some(CameraView::Flycam), "the flycam at login");
            alpha.press("Alt+Shift+F").await?;
            let _flying = camera.clone().to_equal(json!(CameraView::Flycam)).await?;
            alpha.press("Alt+Shift+F").await?;
            let _back = camera.to_equal(json!(resting)).await?;
            Ok(())
        })?;
        Ok(())
    }

    /// The Build menu's path to the Build window, by the entries' Fluent keys.
    const BUILD_WINDOW: &str = "build-tools";

    /// The build-tool radio option whose caption is `key`.
    fn tool(key: &str) -> Locator {
        Locator::role(Role::Radio).name_key(key)
    }

    /// The Build menu's entry whose caption is `key`, while the menu is open.
    fn build_entry(alpha: &Viewer, key: &str) -> UiLocator {
        alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key(key))
    }

    /// Open the menu bar's Build menu, wait for its Link entry to be
    /// `enabled` — it wants two selected linksets — and close the menu again.
    async fn expect_link(alpha: &Viewer, enabled: bool) -> Result<(), BodyError> {
        let _menu = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-bar-build"))
            .click()
            .await?;
        let link = alpha.expect(&build_entry(alpha, "menu-bar-link"));
        let _state = if enabled {
            link.to_be_enabled().await?
        } else {
            link.to_be_disabled().await?
        };
        alpha.press("Escape").await?;
        let _closed = alpha
            .expect(&build_entry(alpha, "menu-bar-link"))
            .to_be_detached()
            .await?;
        Ok(())
    }

    /// The names the build-chord test gives its two prims.
    const FIRST_PRIM: &str = "Chord One";

    /// The second prim's name.
    const SECOND_PRIM: &str = "Chord Two";

    /// Rez a prim on the stock box from the Build window's Create tool and
    /// name it `name`; the Build window is left on the Move tool with the new
    /// prim selected, and the focus on a tab rather than in a text field.
    async fn rez_named(alpha: &Viewer, name: &str) -> Result<(), BodyError> {
        let build = alpha.ui().window(BUILD_WINDOW);
        let _create = build.get(tool("build-tool-create")).click().await?;
        // The stock box is the region's one unnamed object.
        let _placed = alpha.world().object_named("Object").place().await?;
        let _moving = alpha
            .expect(&build.get(tool("build-tool-move")))
            .to_be_checked()
            .await?;
        let _general = build
            .get(Locator::role(Role::Tab).name_key("build-tab-general"))
            .click()
            .await?;
        let field = build.test_id("build-name:field");
        let _filled = field.fill(name).await?;
        field.press("Enter").await?;
        let _named = alpha
            .world()
            .object_named(name)
            .timeout(WAIT)
            .node()
            .await?;
        Ok(())
    }

    /// **Build chords**: Ctrl+Z asks the grid to undo the selected prim's last
    /// edit and Ctrl+Y to redo it; with two prims selected Ctrl+L links them, and
    /// Ctrl+Shift+L unlinks them; Ctrl+Q logs the viewer out and quits.
    #[test]
    fn the_build_chords_undo_redo_link_unlink_and_the_quit_chord_logs_out() -> Result<(), TestError>
    {
        stage("build_chords", &["Alpha"])
            .needs(Need::Content("the stock scene's unnamed box, to rez on"))
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                alpha.press("Ctrl+B").await?;
                let build = alpha.ui().window(BUILD_WINDOW);
                let _open = alpha.expect(&build).to_be_visible().await?;
                rez_named(alpha, FIRST_PRIM).await?;

                // Undo / redo: the simulator keeps an object's edit history
                // and does the reverting, so what the chords owe is the
                // `Undo` / `Redo` naming the selection.
                let prim = alpha.world().object_named(FIRST_PRIM).node().await?.full_id;
                let mut heard = stage.agent("Alpha").await?.events();
                // A chord typed into a text field is the field's: take the
                // focus out of the name field first.
                let _tab = build
                    .get(Locator::role(Role::Tab).name_key("build-tab-object"))
                    .click()
                    .await?;
                alpha.press("Ctrl+Z").await?;
                grid_hears(&mut heard, "Undo", |event| {
                    matches!(event, ServerEvent::ObjectsUndone { object_ids }
                        if object_ids.iter().any(|id| id.uuid() == prim))
                })
                .await?;
                alpha.press("Ctrl+Y").await?;
                grid_hears(&mut heard, "Redo", |event| {
                    matches!(event, ServerEvent::ObjectsRedone { object_ids }
                        if object_ids.iter().any(|id| id.uuid() == prim))
                })
                .await?;

                // Link and unlink: the second prim, then the first joined to
                // the selection.
                rez_named(alpha, SECOND_PRIM).await?;
                let _added = alpha
                    .world()
                    .object_named(FIRST_PRIM)
                    .shift_select()
                    .await?;
                let selected = alpha.selection().await?;
                assert_eq!(selected.len(), 2, "both prims are selected: {selected:?}");
                expect_link(alpha, true).await?;
                alpha.press("Ctrl+L").await?;
                // One linkset is selected now, which cannot be linked again.
                expect_link(alpha, false).await?;
                let first = alpha.world().object_named(FIRST_PRIM).node().await?;
                let second = alpha.world().object_named(SECOND_PRIM).node().await?;
                assert!(
                    first.parent.is_some() || second.parent.is_some(),
                    "one prim hangs off the other once linked: {first:?} {second:?}"
                );
                alpha.press("Ctrl+Shift+L").await?;
                expect_link(alpha, true).await?;
                let first = alpha.world().object_named(FIRST_PRIM).node().await?;
                let second = alpha.world().object_named(SECOND_PRIM).node().await?;
                assert!(
                    first.parent.is_none() && second.parent.is_none(),
                    "neither prim hangs off the other once unlinked: {first:?} {second:?}"
                );

                // Quit.
                let mut heard = stage.agent("Alpha").await?.events();
                stage.expect_quit("Alpha")?;
                alpha.press("Ctrl+Q").await?;
                grid_hears(&mut heard, "logout", |event| {
                    matches!(event, ServerEvent::LoggedOut)
                })
                .await
            })?;
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

    // ---- About ------------------------------------------------------------

    /// The About window's floater id.
    const ABOUT: &str = "about";

    /// The Help menu's path to the About window.
    const HELP_ABOUT: [&str; 2] = ["menu-bar-help", "menu-bar-about"];

    /// The license row the About test opens: a crate everything links.
    const LICENSE_ROW: &str = "Apache-2.0: ring";

    /// The region east of the About test's home.
    const NEIGHBOUR: &str = "Neighbour";

    /// **About**: Help ▸ About opens the window and ticks its entry; the tabs
    /// switch panes; a license row shows that license's text; Copy to
    /// Clipboard puts the support block where a paste finds it; and after a
    /// teleport the Region line names the new region.
    #[test]
    fn the_about_window_switches_tabs_copies_its_block_and_follows_a_teleport()
    -> Result<(), TestError> {
        let home = RegionConfig::default();
        let neighbour = RegionConfig {
            name: NEIGHBOUR.to_owned(),
            grid_x: home.grid_x.saturating_add(1),
            ..RegionConfig::default()
        };
        stage("about_window", &["Alpha"])
            .region(home)
            .region(neighbour)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                let _opened = alpha.menu_path(&HELP_ABOUT).await?;
                let about = alpha.ui().window(ABOUT);
                let _shown = alpha.expect(&about).to_be_visible().await?;
                expect_about_ticked(alpha).await?;

                let block = about.test_id("about:info:support-block");
                let region_line = format!("Region: {} (", stage.home_region());
                let _home = alpha.expect(&block).to_contain_text(&region_line).await?;
                let _version = alpha
                    .expect(&block)
                    .to_contain_text("Simulator version: sl-fake-grid")
                    .await?;

                let tab = |key: &str| about.get(Locator::role(Role::Tab).name_key(key));
                let _credits = tab("about-tab-credits").click().await?;
                let _credits_shown = alpha
                    .expect(&about.test_id("about:credits"))
                    .to_be_visible()
                    .await?;
                let _info_hidden = alpha.expect(&block).to_be_hidden().await?;
                let _licenses = tab("about-tab-licenses").click().await?;
                let _row = about
                    .get(Locator::role(Role::Button).name_containing(LICENSE_ROW))
                    .click()
                    .await?;
                let _text = alpha
                    .expect(&about.test_id("about:licenses:text"))
                    .to_contain_text("Used by: ring")
                    .await?;

                let _info = tab("about-tab-info").click().await?;
                let _copied = about.button_key("about-copy").click().await?;
                let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
                let _focused = bar.click().await?;
                bar.press("Ctrl+V").await?;
                let _pasted = alpha.expect(&bar).to_contain_text(&region_line).await?;
                let _cleared = bar.fill("").await?;

                // The world map covers the window; close it for the teleport.
                let _closed = alpha.menu_path(&HELP_ABOUT).await?;
                let _hidden = alpha.expect(&about).to_be_hidden().await?;
                teleport_by_map(alpha, NEIGHBOUR).await?;
                let _reopened = alpha.menu_path(&HELP_ABOUT).await?;
                let _moved = alpha
                    .expect(&block)
                    .to_contain_text(&format!("Region: {NEIGHBOUR} ("))
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    /// Open the Help menu, see its About entry ticked, and close the menu.
    async fn expect_about_ticked(alpha: &Viewer) -> Result<(), BodyError> {
        let _menu = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-bar-help"))
            .click()
            .await?;
        let entry = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key("menu-bar-about"));
        let _ticked = alpha.expect(&entry).to_be_checked().await?;
        alpha.press("Escape").await?;
        let _closed = alpha.expect(&entry).to_be_detached().await?;
        Ok(())
    }

    /// Teleport to `region` through the world map's search, and wait until
    /// the agent is there.
    async fn teleport_by_map(alpha: &Viewer, region: &str) -> Result<(), BodyError> {
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
        let _arrived = alpha
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(TELEPORT)
            .to_equal(json!(region))
            .await?;
        let _closed = alpha
            .menu_path(&["menu-bar-world", "menu-bar-world-map"])
            .await?;
        Ok(())
    }

    // ---- Do Not Disturb ---------------------------------------------------

    /// The Comm menu's path to the Unavailable (do-not-disturb) mode.
    const UNAVAILABLE: [&str; 3] = [
        "menu-bar-comm",
        "menu-bar-online-status",
        "menu-bar-unavailable",
    ];

    /// The resident who IMs and offers in the do-not-disturb test: nobody the
    /// grid knows, which a simulator relaying an IM does not check either.
    const CALLER: u128 = 0xD1D_0000_0000_0000_0000_0000_0000_0CA1;

    /// The caller's name, as an IM carries it.
    const CALLER_NAME: &str = "Caller Resident";

    /// An IM or an offer from the caller to viewer `label`, with `dialog`.
    fn from_caller(
        stage: &Stage,
        label: &str,
        dialog: ImDialog,
        message: &str,
    ) -> Result<InstantMessage, BodyError> {
        let caller = AgentKey::from(Uuid::from_u128(CALLER));
        let to = stage.agent_id(label)?;
        let session = Uuid::from_u128(to.uuid().as_u128() ^ CALLER);
        Ok(InstantMessage {
            from_agent_id: caller,
            from_agent_name: CALLER_NAME.to_owned(),
            to_agent_id: to,
            dialog,
            from_group: false,
            region_id: None,
            position: RegionCoordinates::new(128.0, 128.0, 25.0),
            offline: false,
            timestamp: None,
            id: if dialog == ImDialog::Message {
                session
            } else {
                Uuid::from_u128(CALLER ^ 0x7E1E)
            },
            parent_estate_id: 1,
            message: message.to_owned(),
            binary_bucket: Vec::new(),
        })
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

    /// The first IM, and the second the viewer must not answer again.
    const FIRST_IM: &str = "Are you there?";

    /// The second IM.
    const SECOND_IM: &str = "Hello again";

    /// The template a teleport offer's card reports as.
    const TELEPORT_OFFER: &str = "TeleportOffered";

    /// **Unavailable**: with Comm ▸ Online Status ▸ Unavailable on, an IM is
    /// answered with the busy reply — once, not again for the next line in
    /// the same conversation — and the conversation says so; a teleport offer
    /// shows nothing until the mode is turned off, and then its card appears.
    #[test]
    fn while_unavailable_an_im_is_answered_once_and_an_offer_waits() -> Result<(), TestError> {
        stage("do_not_disturb", &["Alpha"])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                let _on = alpha.menu_path(&UNAVAILABLE).await?;
                let mut heard = stage.agent("Alpha").await?.events();
                let caller = Uuid::from_u128(CALLER);

                deliver(
                    stage,
                    "Alpha",
                    &from_caller(stage, "Alpha", ImDialog::Message, FIRST_IM)?,
                )
                .await?;
                let conversation = json!({ "kind": "direct", "id": caller });
                let _arrived = alpha
                    .expect_state(Probe::Conversations)
                    .timeout(WAIT)
                    .to_include(json!([{
                        "conversation": conversation,
                        "lines": [{ "text": FIRST_IM }],
                    }]))
                    .await?;
                grid_hears(&mut heard, "busy reply", |event| {
                    busy_reply_to(event, caller)
                })
                .await?;
                let _noted = alpha
                    .expect_state(Probe::Conversations)
                    .timeout(WAIT)
                    .to_include(json!([{
                        "conversation": conversation,
                        "lines": [{ "text": "Autoresponse sent:" }],
                    }]))
                    .await?;

                let replies_before = reply_commands(alpha).await?;
                deliver(
                    stage,
                    "Alpha",
                    &from_caller(stage, "Alpha", ImDialog::Message, SECOND_IM)?,
                )
                .await?;
                deliver(
                    stage,
                    "Alpha",
                    &from_caller(stage, "Alpha", ImDialog::LureUser, "Join me")?,
                )
                .await?;
                stage.mark("Alpha", "after-the-second-im").await?;
                stage
                    .wait_marker("Alpha", "after-the-second-im", WAIT)
                    .await?;
                let _second = alpha
                    .expect_state(Probe::Conversations)
                    .timeout(WAIT)
                    .to_include(json!([{
                        "conversation": conversation,
                        "lines": [{ "text": SECOND_IM }],
                    }]))
                    .await?;
                assert_eq!(
                    reply_commands(alpha).await?,
                    replies_before,
                    "the second IM of the conversation is not answered again"
                );
                let held = alpha
                    .notifications()
                    .await?
                    .into_iter()
                    .any(|notification| notification.template == TELEPORT_OFFER);
                assert!(!held, "the teleport offer showed while Unavailable");

                let _off = alpha.menu_path(&UNAVAILABLE).await?;
                let _shown = alpha
                    .expect_notification()
                    .timeout(WAIT)
                    .to_show(TELEPORT_OFFER)
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    /// Whether `event` is the viewer's do-not-disturb reply to `to`.
    fn busy_reply_to(event: &ServerEvent, to: Uuid) -> bool {
        matches!(event, ServerEvent::InstantMessage(im)
            if im.to_agent_id.uuid() == to && im.dialog == ImDialog::DoNotDisturbAutoResponse)
    }

    /// How many busy replies the viewer has sent, by its own command log.
    async fn reply_commands(alpha: &Viewer) -> Result<usize, BodyError> {
        let mut log = alpha.events_from_start();
        Ok(log
            .read(&[LogStream::Command])
            .await?
            .iter()
            .filter(|entry| entry.kind == "AutoResponse")
            .count())
    }

    // ---- Radar ------------------------------------------------------------

    /// The radar's floater id.
    const RADAR: &str = "radar";

    /// The catalogue NPC standing nearer the login point.
    const NEAR_NPC: &str = "Catalogue Resident";

    /// The catalogue NPC sitting farther away.
    const FAR_NPC: &str = "Seated Resident";

    /// The Range column's header cell.
    const RANGE_HEADER: &str = "radar:table-header-cell:7";

    /// **Radar**: the rows sort nearest first; a click on the Range header
    /// turns the order round; typing in the filter leaves only the matching
    /// row; and with a row selected, Profile opens that resident's profile and
    /// IM a conversation with them.
    #[test]
    fn the_radar_sorts_filters_and_opens_a_profile_and_an_im() -> Result<(), TestError> {
        stage("radar", &["Alpha"])
            .region(catalogue()?)
            .run(async |stage: &Stage| {
                let alpha = stage.viewer("Alpha")?;
                let _opened = alpha
                    .menu_path(&["menu-bar-world", "menu-bar-radar"])
                    .await?;
                let radar = alpha.ui().window(RADAR);
                let row =
                    |name: &str| radar.get(Locator::role(Role::ListItem).name_containing(name));
                let top = radar.get(Locator::role(Role::ListItem)).nth(0);
                let _near = alpha
                    .expect(&row(FAR_NPC))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let _nearest_first = alpha.expect(&top).to_contain_text(NEAR_NPC).await?;
                let _sorted = radar.test_id(RANGE_HEADER).click().await?;
                let _farthest_first = alpha.expect(&top).to_contain_text(FAR_NPC).await?;

                let _filtered = radar.test_id("radar-filter:field").fill("seat").await?;
                let _gone = alpha.expect(&row(NEAR_NPC)).to_be_hidden().await?;
                let _kept = alpha.expect(&row(FAR_NPC)).to_be_visible().await?;

                let _selected = row(FAR_NPC).click().await?;
                let resident = alpha.world().avatar(FAR_NPC).node().await?.full_id;
                let _profile = radar.button_key("radar-action-profile").click().await?;
                let _shown = alpha
                    .expect(&alpha.ui().window(&format!("avatar-profile#{resident}")))
                    .to_be_visible()
                    .await?;
                let _im = radar.button_key("radar-action-im").click().await?;
                let _conversation = alpha
                    .expect_state(Probe::Conversations)
                    .to_include(json!([{
                        "conversation": { "kind": "direct", "id": resident },
                    }]))
                    .await?;
                Ok(())
            })?;
        Ok(())
    }
}
