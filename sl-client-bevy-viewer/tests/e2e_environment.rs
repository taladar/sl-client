//! End-to-end tests for the environment windows ([[test-e2e-sweep-environment]]):
//! each is opened in the real viewer on the fake grid, edited and saved, and
//! what the grid stored is read back — the item it names, the settings asset
//! under it, the environment a region publish left in its store.
//!
//! - a new sky and a new water item, created in My Environments, edited in
//!   their editors and saved over `UpdateSettingsAgentInventory`; the grid's
//!   items carry their kind in their flags;
//! - a new day cycle, scrubbed, given a keyframe and saved;
//! - the Region / Estate window's Environment tab: Customize Day Cycle edits
//!   the region's inline cycle, Apply publishes it with a day length and
//!   offset, and Use Default Settings resets it;
//! - a WindLight sky preset imported into the sky editor and filed with Save
//!   As, and a folder of them through World ▸ Environment ▸ Bulk Import;
//! - My Environments filters, renames, applies to the viewer alone and
//!   deletes;
//! - a region that serves neither settings capability greys what would store
//!   a settings asset and refuses a bulk import;
//! - Personal Lighting's sliders, colour picker and sun trackball edit the
//!   local sky, a preset picked under a transition time cross-fades, and
//!   Reset hands the sky back.

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::path::PathBuf;

    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::json;
    use sl_automation_proto::{InventoryRoot, Locator, Probe, Role};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_fake_grid::RegionConfig;
    use sl_proto::{
        AssetKey, CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPDATE_SETTINGS_TASK_INVENTORY,
        CapsUploadMetadata, DayCycle, EnvironmentAsset, InventoryItem, InventoryKey, ServerEvent,
        SkySettings, Uuid, WaterSettings, environment_asset_from_bytes,
    };
    use sl_viewer_driver::{UiLocator, Viewer};
    use tokio::sync::broadcast;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// The My Environments window's floater id.
    const MY_ENVIRONMENTS: &str = "my-environments";

    /// The sky editor's floater id.
    const SKY_EDITOR: &str = "settings-editor-sky";

    /// The water editor's floater id.
    const WATER_EDITOR: &str = "settings-editor-water";

    /// The day-cycle editor's floater id.
    const DAY_EDITOR: &str = "day-cycle-editor";

    /// The Personal Lighting window's floater id.
    const PERSONAL_LIGHTING: &str = "personal-lighting";

    /// The settings item every stage account is seeded with: a sky.
    const FIXTURE_SKY: &str = "Fixture Settings";

    /// The menu path to My Environments.
    const OPEN_MY_ENVIRONMENTS: [&str; 3] = [
        "menu-bar-world",
        "menu-bar-environment",
        "menu-bar-my-environments",
    ];

    /// A one-viewer stage for `name`.
    fn stage(name: &str) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
    }

    /// Where the WindLight sky preset fixtures are.
    fn preset_skies() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/assets/windlight/skies")
    }

    /// Wait until the grid's session reports an event `wanted` accepts: the
    /// viewer's `what` reaching the grid.
    async fn grid_hears<T>(
        heard: &mut broadcast::Receiver<ServerEvent>,
        what: &str,
        wanted: impl Fn(&ServerEvent) -> Option<T> + Send + Sync,
    ) -> Result<T, BodyError> {
        let seen = tokio::time::timeout(WAIT, async {
            loop {
                match heard.recv().await {
                    Ok(event) => {
                        if let Some(found) = wanted(&event) {
                            return Ok(found);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        })
        .await;
        match seen {
            Ok(Ok(found)) => Ok(found),
            Ok(Err(error)) => Err(format!("the grid's event stream: {error}").into()),
            Err(_elapsed) => Err(format!("the grid never heard the {what}").into()),
        }
    }

    /// Wait until the grid stores a settings save over `item`.
    async fn grid_stores_save(
        heard: &mut broadcast::Receiver<ServerEvent>,
        item: Uuid,
    ) -> Result<(), BodyError> {
        grid_hears(heard, "settings save", |event| match event {
            ServerEvent::CapsAssetUploaded { metadata, .. } => matches!(
                metadata.as_ref(),
                CapsUploadMetadata::UpdateAgentItem { item_id, .. } if item_id.uuid() == item
            )
            .then_some(()),
            _ => None,
        })
        .await
    }

    /// The grid's own copy of agent item `item`.
    async fn grid_item(stage: &Stage, item: Uuid) -> Result<InventoryItem, BodyError> {
        let agent = stage.agent("Alpha").await?;
        agent
            .with_sim(|sim| {
                sim.agent_inventory()
                    .item(InventoryKey::from(item))
                    .cloned()
            })
            .await
            .ok_or_else(|| format!("the grid holds no item {item}").into())
    }

    /// The settings asset the grid's item `item` names, decoded.
    async fn grid_settings(
        stage: &Stage,
        item: Uuid,
    ) -> Result<(InventoryItem, EnvironmentAsset), BodyError> {
        let held = grid_item(stage, item).await?;
        let agent = stage.agent("Alpha").await?;
        let bytes = agent
            .stored_asset(AssetKey::from(held.asset_id))
            .await
            .ok_or_else(|| format!("the grid stores no asset {}", held.asset_id))?;
        let asset = environment_asset_from_bytes(&held.name, &bytes)
            .ok_or_else(|| format!("{} is not a settings asset", held.asset_id))?;
        Ok((held, asset))
    }

    /// The sky a settings asset holds.
    fn sky_of(asset: EnvironmentAsset) -> Result<SkySettings, BodyError> {
        match asset {
            EnvironmentAsset::Sky(sky) => Ok(*sky),
            other => Err(format!("not a sky: {:?}", other.kind()).into()),
        }
    }

    /// The water a settings asset holds.
    fn water_of(asset: EnvironmentAsset) -> Result<WaterSettings, BodyError> {
        match asset {
            EnvironmentAsset::Water(water) => Ok(water),
            other => Err(format!("not water: {:?}", other.kind()).into()),
        }
    }

    /// The day cycle a settings asset holds.
    fn day_of(asset: EnvironmentAsset) -> Result<DayCycle, BodyError> {
        match asset {
            EnvironmentAsset::DayCycle(cycle) => Ok(*cycle),
            other => Err(format!("not a day cycle: {:?}", other.kind()).into()),
        }
    }

    /// The id of the item named `name` in the Settings folder, once the viewer
    /// lists it.
    async fn settings_item(alpha: &Viewer, name: &str) -> Result<Uuid, BodyError> {
        let _listed = alpha
            .expect_state(Probe::Inventory {
                root: InventoryRoot::Agent,
                path: vec!["Settings".to_owned()],
            })
            .at("/items")
            .timeout(WAIT)
            .to_include(json!([{ "name": name }]))
            .await?;
        let folder = alpha
            .inventory(InventoryRoot::Agent, &["Settings"])
            .await?
            .ok_or("the viewer knows no Settings folder")?;
        folder
            .items
            .iter()
            .find(|item| item.name == name)
            .map(|item| item.id)
            .ok_or_else(|| format!("no settings item named {name}").into())
    }

    /// Open My Environments from the menu bar, and wait for it.
    async fn open_my_environments(alpha: &Viewer) -> Result<UiLocator, BodyError> {
        let _opened = alpha.menu_path(&OPEN_MY_ENVIRONMENTS).await?;
        let window = alpha.ui().window(MY_ENVIRONMENTS);
        let _shown = alpha.expect(&window).to_be_visible().await?;
        Ok(window)
    }

    /// My Environments' row naming `name`.
    fn row(window: &UiLocator, name: &str) -> UiLocator {
        window.get(Locator::role(Role::ListItem).name_containing(name))
    }

    /// Open the row's context menu and pick the entry whose caption is `key`.
    async fn row_menu(alpha: &Viewer, row: &UiLocator, key: &str) -> Result<(), BodyError> {
        let _menu = row.right_click().await?;
        let _picked = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key(key))
            .click()
            .await?;
        Ok(())
    }

    /// Create a settings item with My Environments' `button`, and answer its
    /// id once the viewer lists it under `name`.
    async fn create(
        alpha: &Viewer,
        window: &UiLocator,
        button: &str,
        name: &str,
    ) -> Result<Uuid, BodyError> {
        let _created = window.test_id(button).click().await?;
        let _row = alpha
            .expect(&row(window, name))
            .timeout(WAIT)
            .to_be_visible()
            .await?;
        settings_item(alpha, name).await
    }

    /// Answer the notification `template` with its `button`.
    async fn answer(alpha: &Viewer, template: &str, button: &str) -> Result<(), BodyError> {
        let _shown = alpha
            .expect_notification()
            .timeout(WAIT)
            .to_show(template)
            .await?;
        let _answered = alpha
            .ui()
            .test_id(&format!("toast-button:{button}"))
            .click()
            .await?;
        Ok(())
    }

    /// Close `window` with its own close button, and wait for it to go.
    async fn close_window(alpha: &Viewer, window: &UiLocator) -> Result<(), BodyError> {
        let _closed = window.button_key("floater-chrome-close").click().await?;
        let _hidden = alpha.expect(window).to_be_hidden().await?;
        Ok(())
    }

    /// Wait until the environment readout holds `value` at `pointer`.
    async fn environment_at(
        alpha: &Viewer,
        pointer: &str,
        value: serde_json::Value,
    ) -> Result<(), BodyError> {
        let _held = alpha
            .expect_state(Probe::Environment)
            .at(pointer)
            .timeout(WAIT)
            .to_equal(value)
            .await?;
        Ok(())
    }

    /// Wait until nothing previews through the edit layer.
    async fn nothing_previewing(alpha: &Viewer) -> Result<(), BodyError> {
        let _gone = alpha
            .expect_state(Probe::Environment)
            .at("/previewing")
            .timeout(WAIT)
            .to_be_absent()
            .await?;
        Ok(())
    }

    /// The top of a slider's range, by its keyboard: End.
    async fn slide_to_end(slider: &UiLocator) -> Result<(), BodyError> {
        slider.press("End").await?;
        Ok(())
    }

    // ---- The fixed editors ------------------------------------------------

    /// **Sky and water editors**: New Sky and New Water each make a settings
    /// item the grid stamps with its kind; Edit opens the matching editor on
    /// it; a knob moved and Save pressed stores a new asset over the item,
    /// holding the edit.
    #[test]
    fn a_new_sky_and_water_are_edited_and_saved_to_the_grid() -> Result<(), TestError> {
        stage("environment_fixed_editors")
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = open_my_environments(alpha).await?;
                let mut heard = stage.agent("Alpha").await?.events();

                let sky =
                    create(alpha, &window, "my-environments-new-sky:button", "New Sky").await?;
                let (item, _asset) = grid_settings(stage, sky).await?;
                assert_eq!(item.flags, 0, "a sky item's flags say sky: {item:?}");
                row_menu(alpha, &row(&window, "New Sky"), "menu-my-env-edit").await?;
                let editor = alpha.ui().window(SKY_EDITOR);
                let _open = alpha.expect(&editor).to_be_visible().await?;
                let _named = alpha
                    .expect(&editor.test_id("settings-editor-sky-name:field"))
                    .to_have_text("New Sky")
                    .await?;
                // The frame being edited is drawn the moment it opens, a knob
                // moves it before anything is saved, and Revert moves it back.
                environment_at(alpha, "/previewing", json!([SKY_EDITOR])).await?;
                environment_at(alpha, "/sky/name", json!("New Sky")).await?;
                let opened = drawn_sky(alpha).await?.haze_density;
                let haze = editor.test_id("settings-editor-sky-haze-density:slider");
                slide_to_end(&haze).await?;
                environment_at(alpha, "/sky/haze_density", json!(5.0)).await?;
                let _reverted = editor
                    .test_id("settings-editor-sky-revert:button")
                    .click()
                    .await?;
                environment_at(alpha, "/sky/haze_density", json!(opened)).await?;
                slide_to_end(&haze).await?;
                environment_at(alpha, "/sky/haze_density", json!(5.0)).await?;
                let _saved = editor
                    .test_id("settings-editor-sky-save:button")
                    .click()
                    .await?;
                grid_stores_save(&mut heard, sky).await?;
                close_window(alpha, &editor).await?;
                // Closing takes the preview away: the region's sky again.
                nothing_previewing(alpha).await?;
                let shared = drawn_sky(alpha).await?;
                assert_ne!(shared.name, "New Sky", "the region's sky is drawn again");
                let (_item, stored) = grid_settings(stage, sky).await?;
                let stored = sky_of(stored)?;
                assert!(
                    (stored.haze_density - 5.0).abs() < 1e-3,
                    "the saved sky holds the edited haze: {}",
                    stored.haze_density
                );

                let water = create(
                    alpha,
                    &window,
                    "my-environments-new-water:button",
                    "New Water",
                )
                .await?;
                let (item, _asset) = grid_settings(stage, water).await?;
                assert_eq!(item.flags, 1, "a water item's flags say water: {item:?}");
                row_menu(alpha, &row(&window, "New Water"), "menu-my-env-edit").await?;
                let editor = alpha.ui().window(WATER_EDITOR);
                let _open = alpha.expect(&editor).to_be_visible().await?;
                let region_fog = drawn_water_fog(alpha).await?;
                environment_at(alpha, "/previewing", json!([WATER_EDITOR])).await?;
                environment_at(alpha, "/water/name", json!("New Water")).await?;
                slide_to_end(&editor.test_id("settings-editor-water-water-fog-density:slider"))
                    .await?;
                environment_at(alpha, "/water/fog_density", json!(100.0)).await?;
                let _saved = editor
                    .test_id("settings-editor-water-save:button")
                    .click()
                    .await?;
                grid_stores_save(&mut heard, water).await?;
                close_window(alpha, &editor).await?;
                nothing_previewing(alpha).await?;
                environment_at(alpha, "/water/fog_density", json!(region_fog)).await?;
                let (_item, stored) = grid_settings(stage, water).await?;
                let stored = water_of(stored)?;
                assert!(
                    (stored.water_fog_density - 100.0).abs() < 1e-2,
                    "the saved water holds the edited fog density: {}",
                    stored.water_fog_density
                );
                Ok(())
            })?;
        Ok(())
    }

    // ---- The day-cycle editor ---------------------------------------------

    /// **Day-cycle editor**: New Day Cycle makes a day item; in the editor a
    /// press on the timeline moves the scrubber, Add Frame puts a keyframe
    /// there, a knob edits it, and Save stores the cycle with the new keyframe
    /// holding the edit.
    #[test]
    fn a_new_day_cycle_is_scrubbed_keyframed_and_saved() -> Result<(), TestError> {
        stage("environment_day_cycle_editor")
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = open_my_environments(alpha).await?;
                let mut heard = stage.agent("Alpha").await?.events();
                let day = create(
                    alpha,
                    &window,
                    "my-environments-new-day-cycle:button",
                    "New Day Cycle",
                )
                .await?;
                let (item, asset) = grid_settings(stage, day).await?;
                assert_eq!(item.flags, 2, "a day item's flags say day cycle: {item:?}");
                let before = day_of(asset)?;
                let ground_before = before.sky_tracks.first().map_or(0, Vec::len);

                row_menu(alpha, &row(&window, "New Day Cycle"), "menu-my-env-edit").await?;
                let editor = alpha.ui().window(DAY_EDITOR);
                let _open = alpha.expect(&editor).to_be_visible().await?;
                environment_at(alpha, "/previewing", json!([DAY_EDITOR])).await?;
                let midnight_haze = drawn_sky(alpha).await?.haze_density;
                let time = editor.test_id("day-cycle-editor-time");
                let midnight = time.text().await?;
                let add = editor.test_id("day-cycle-editor-add-frame:button");
                // The scrubber opens on the midnight keyframe, where there is
                // already one.
                let _occupied = alpha.expect(&add).to_be_disabled().await?;
                let _scrubbed = editor
                    .test_id("day-cycle-editor-cursor:strip")
                    .click()
                    .await?;
                let _free = alpha.expect(&add).to_be_enabled().await?;
                assert_ne!(
                    time.text().await?,
                    midnight,
                    "the time readout follows the scrubber"
                );
                let _added = add.click().await?;
                let _taken = alpha.expect(&add).to_be_disabled().await?;
                slide_to_end(&editor.test_id("day-cycle-editor-sky-haze-density:slider")).await?;
                // The preview is the cycle at the scrubber: the edited keyframe
                // here, the untouched one at midnight.
                environment_at(alpha, "/sky/haze_density", json!(5.0)).await?;
                let _back = editor
                    .test_id("day-cycle-editor-skip-back:button")
                    .click()
                    .await?;
                environment_at(alpha, "/sky/haze_density", json!(midnight_haze)).await?;
                let _forward = editor
                    .test_id("day-cycle-editor-skip-forward:button")
                    .click()
                    .await?;
                environment_at(alpha, "/sky/haze_density", json!(5.0)).await?;
                let _saved = editor
                    .test_id("day-cycle-editor-save:button")
                    .click()
                    .await?;
                grid_stores_save(&mut heard, day).await?;
                close_window(alpha, &editor).await?;
                nothing_previewing(alpha).await?;

                let (_item, stored) = grid_settings(stage, day).await?;
                let cycle = day_of(stored)?;
                let ground = cycle
                    .sky_tracks
                    .first()
                    .ok_or("the cycle has a ground track")?;
                assert_eq!(
                    ground.len(),
                    ground_before.saturating_add(1),
                    "one keyframe was added: {ground:?}"
                );
                let added = ground
                    .iter()
                    .find(|keyframe| (0.3..0.7).contains(&keyframe.keyframe))
                    .ok_or("the new keyframe sits where the timeline was pressed")?;
                let frame = cycle
                    .sky_frames
                    .get(&added.name)
                    .ok_or("the new keyframe names a frame the cycle holds")?;
                assert!(
                    (frame.haze_density - 5.0).abs() < 1e-3,
                    "the new keyframe holds the edit: {}",
                    frame.haze_density
                );
                Ok(())
            })?;
        Ok(())
    }

    // ---- The region's environment -----------------------------------------

    /// **The Region / Estate window's Environment tab**: Customize Day Cycle
    /// opens the region's own (inline) cycle in the day-cycle editor, whose
    /// Save hands the edit back; Apply publishes it with the day length and
    /// offset the sliders hold; Use Default Settings, confirmed, resets it.
    #[test]
    fn the_region_environment_is_customized_published_and_reset() -> Result<(), TestError> {
        stage("environment_region_panel")
            .needs(Need::GridControl)
            .configure_grid(|mut grid| {
                let _granted = grid.grant_estate_powers("Stage", "Alpha");
                grid
            })
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let region = stage
                    .grid()?
                    .region_id(stage.home_region())
                    .ok_or("the home region has an id")?;
                let _opened = alpha
                    .menu_path(&["menu-bar-world", "menu-bar-region-estate"])
                    .await?;
                let window = alpha.ui().window(&format!("about-region#{region}"));
                let _shown = alpha.expect(&window).to_be_visible().await?;
                let _tab = window
                    .get(Locator::role(Role::Tab).name_key("about-region-tab-environment"))
                    .click()
                    .await?;
                let apply = window.test_id("land-environment-apply:button");
                let customize = window.test_id("land-environment-edit:button");
                let _loaded = alpha
                    .expect(&customize)
                    .timeout(WAIT)
                    .to_be_enabled()
                    .await?;

                let _customize = customize.click().await?;
                let editor = alpha.ui().window(DAY_EDITOR);
                let _open = alpha.expect(&editor).to_be_visible().await?;
                // It opens on the midnight keyframe of the ground track, and
                // previews it.
                environment_at(alpha, "/previewing", json!([DAY_EDITOR])).await?;
                slide_to_end(&editor.test_id("day-cycle-editor-sky-haze-density:slider")).await?;
                environment_at(alpha, "/sky/haze_density", json!(5.0)).await?;
                let _saved = editor
                    .test_id("day-cycle-editor-save:button")
                    .click()
                    .await?;
                close_window(alpha, &editor).await?;
                nothing_previewing(alpha).await?;

                slide_to_end(&window.test_id("land-environment-day-length:slider")).await?;
                window
                    .test_id("land-environment-day-offset:slider")
                    .press("Home")
                    .await?;
                let mut heard = stage.agent("Alpha").await?.events();
                let _applied = apply.click().await?;
                let update = grid_hears(&mut heard, "region publish", |event| match event {
                    ServerEvent::EnvironmentUpdated {
                        parcel_id: -1,
                        track_no: None,
                        update,
                    } => Some(update.clone()),
                    _ => None,
                })
                .await?;
                assert_eq!(update.day_length, Some(168 * 3600), "a week-long day");
                assert_eq!(
                    update.day_offset,
                    Some(45_000),
                    "−11.5 h, wrapped into the day as the reference sends it"
                );
                let cycle = update
                    .day_cycle
                    .ok_or("the customized cycle is published inline")?;
                let midnight = cycle
                    .sky_tracks
                    .first()
                    .and_then(|ground| ground.first())
                    .ok_or("the cycle keeps its midnight keyframe")?;
                let frame = cycle
                    .sky_frames
                    .get(&midnight.name)
                    .ok_or("the keyframe names a frame the cycle holds")?;
                assert!(
                    (frame.haze_density - 5.0).abs() < 1e-3,
                    "the customized frame was published: {}",
                    frame.haze_density
                );

                let _default = window
                    .test_id("land-environment-use-default:button")
                    .click()
                    .await?;
                answer(alpha, "SettingsConfirmReset", "OK").await?;
                grid_hears(&mut heard, "region reset", |event| {
                    matches!(event, ServerEvent::EnvironmentReset { parcel_id: -1, .. })
                        .then_some(())
                })
                .await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- Importing WindLight presets --------------------------------------

    /// **Legacy import**: the sky editor's Import reads a WindLight preset
    /// picked in the file chooser, names it after the file, and Save As files
    /// it as a new settings item holding the preset's values.
    #[test]
    fn a_windlight_sky_is_imported_and_filed_with_save_as() -> Result<(), TestError> {
        stage("environment_import_preset")
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let editor = alpha.open_floater(SKY_EDITOR).await?;
                let _imported = editor
                    .test_id("settings-editor-sky-import:button")
                    .click()
                    .await?;
                let dialog = alpha
                    .answer_file_dialog(Some(&preset_skies().join("Amber%20Dawn.xml")))
                    .await?;
                assert_eq!(dialog.purpose, "settings-editor-import-sky");
                assert!(!dialog.folder, "an Import picks one file");
                let _named = alpha
                    .expect(&editor.test_id("settings-editor-sky-name:field"))
                    .to_have_text("Amber Dawn")
                    .await?;
                let _status = alpha
                    .expect(&editor.test_id("settings-editor-sky-status"))
                    .to_contain_text("Imported Amber Dawn")
                    .await?;
                let _filed = editor
                    .test_id("settings-editor-sky-save-as:button")
                    .click()
                    .await?;
                let item = settings_item(alpha, "Amber Dawn").await?;
                let mut heard = stage.agent("Alpha").await?.events();
                // The body may already be stored by the time the item is
                // listed; wait for it only if it is not.
                let stored_haze = |sky: &SkySettings| (sky.haze_density - 3.25).abs() < 1e-3;
                let (_item, stored) = grid_settings(stage, item).await?;
                if !sky_of(stored).is_ok_and(|sky| stored_haze(&sky)) {
                    grid_stores_save(&mut heard, item).await?;
                }
                let (_item, stored) = grid_settings(stage, item).await?;
                let sky = sky_of(stored)?;
                assert!(
                    stored_haze(&sky),
                    "the filed sky is the preset's: {}",
                    sky.haze_density
                );
                Ok(())
            })?;
        Ok(())
    }

    /// **Bulk import**: World ▸ Environment ▸ Bulk Import ▸ Skies asks for a
    /// folder and files every preset in it as a settings item of its own,
    /// named after its file and holding its values.
    #[test]
    fn a_folder_of_windlight_skies_is_bulk_imported() -> Result<(), TestError> {
        stage("environment_bulk_import")
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let _started = alpha
                    .menu_path(&[
                        "menu-bar-world",
                        "menu-bar-environment",
                        "menu-bar-bulk-import",
                        "menu-bar-skies",
                    ])
                    .await?;
                let dialog = alpha.answer_file_dialog(Some(&preset_skies())).await?;
                assert_eq!(dialog.purpose, "bulk-import-skies");
                assert!(dialog.folder, "a bulk import picks a folder");
                let _finished = alpha
                    .expect_notification()
                    .timeout(WAIT)
                    .to_show("WindlightBulkImportFinished")
                    .await?;
                for (name, haze) in [("Amber Dawn", 3.25), ("Grey Noon", 1.5)] {
                    let item = settings_item(alpha, name).await?;
                    let holds = |asset: EnvironmentAsset| {
                        sky_of(asset).is_ok_and(|sky| (sky.haze_density - haze).abs() < 1e-3)
                    };
                    let stored = tokio::time::timeout(WAIT, async {
                        loop {
                            if let Ok((_item, asset)) = grid_settings(stage, item).await
                                && holds(asset)
                            {
                                return;
                            }
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    })
                    .await;
                    if stored.is_err() {
                        let (held, asset) = grid_settings(stage, item).await?;
                        return Err(format!(
                            "{name} was not filed holding its preset's haze {haze}: the grid's \
                             item names {} holding {:?}",
                            held.asset_id,
                            sky_of(asset).map(|sky| sky.haze_density)
                        )
                        .into());
                    }
                }
                Ok(())
            })?;
        Ok(())
    }

    // ---- My Environments --------------------------------------------------

    /// **My Environments**: the name filter and a kind filter hide what they
    /// leave out; Rename renames the item on the grid; Apply Only To Myself
    /// puts the sky over the viewer alone; Delete, confirmed, moves the item to
    /// the Trash.
    #[test]
    fn my_environments_filters_renames_applies_and_deletes() -> Result<(), TestError> {
        stage("environment_my_environments")
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = open_my_environments(alpha).await?;
                let sky =
                    create(alpha, &window, "my-environments-new-sky:button", "New Sky").await?;
                let fixture = row(&window, FIXTURE_SKY);
                let _listed = alpha.expect(&fixture).timeout(WAIT).to_be_visible().await?;

                let filter = window.test_id("my-environments-filter:field");
                let _typed = filter.fill("fixture").await?;
                let _hidden = alpha
                    .expect(&row(&window, "New Sky"))
                    .to_be_hidden()
                    .await?;
                let _kept = alpha.expect(&fixture).to_be_visible().await?;
                let _cleared = filter.fill("").await?;
                let skies = window.test_id("my-environments-filter-sky:checkbox");
                skies.uncheck().await?;
                let _no_skies = alpha.expect(&fixture).to_be_hidden().await?;
                skies.check().await?;
                let _skies_back = alpha.expect(&fixture).to_be_visible().await?;

                let _selected = row(&window, "New Sky").click().await?;
                let _named = window
                    .test_id("my-environments-rename:field")
                    .fill("Morning Haze")
                    .await?;
                let _renamed = window
                    .test_id("my-environments-rename:button")
                    .click()
                    .await?;
                let renamed = row(&window, "Morning Haze");
                let _shown = alpha.expect(&renamed).timeout(WAIT).to_be_visible().await?;
                let held = grid_item(stage, sky).await?;
                assert_eq!(held.name, "Morning Haze", "the grid's item is renamed");

                let local_sky = alpha.expect_state(Probe::Environment).at("/local_sky");
                let _shared = local_sky.clone().to_equal(json!(false)).await?;
                row_menu(alpha, &fixture, "menu-my-env-apply-only-to-myself").await?;
                let _applied = alpha
                    .expect(&window.test_id("my-environments-status"))
                    .to_have_text("Applied to you.")
                    .await?;
                let _local = local_sky.timeout(WAIT).to_equal(json!(true)).await?;

                let _picked = renamed.click().await?;
                let _deleted = window
                    .test_id("my-environments-delete:button")
                    .click()
                    .await?;
                answer(alpha, "DeleteItems", "Yes").await?;
                let _gone = alpha.expect(&renamed).timeout(WAIT).to_be_hidden().await?;
                let trash = alpha
                    .inventory(InventoryRoot::Agent, &["Trash"])
                    .await?
                    .ok_or("the viewer knows the Trash")?;
                let moved = tokio::time::timeout(WAIT, async {
                    loop {
                        if grid_item(stage, sky)
                            .await
                            .is_ok_and(|item| item.folder_id.uuid() == trash.id)
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                })
                .await;
                assert!(moved.is_ok(), "the grid's item is in the Trash");
                Ok(())
            })?;
        Ok(())
    }

    // ---- A region that cannot store settings ------------------------------

    /// **No settings capabilities**: on a region that grants neither
    /// `UpdateSettingsAgentInventory` nor `UpdateSettingsTaskInventory`, My
    /// Environments greys every button that would store a settings asset, the
    /// sky editor greys Save and Save As, and a bulk import is refused with
    /// the reference's notification before any chooser opens.
    #[test]
    fn a_region_without_settings_caps_greys_what_would_store_one() -> Result<(), TestError> {
        let region = RegionConfig {
            withheld_caps: vec![
                CAP_UPDATE_SETTINGS_AGENT_INVENTORY.to_owned(),
                CAP_UPDATE_SETTINGS_TASK_INVENTORY.to_owned(),
            ],
            ..RegionConfig::default()
        };
        stage("environment_settings_unsupported")
            .region(region)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = open_my_environments(alpha).await?;
                let fixture = row(&window, FIXTURE_SKY);
                let _listed = alpha.expect(&fixture).timeout(WAIT).to_be_visible().await?;
                let _selected = fixture.click().await?;
                for button in [
                    "my-environments-new-sky:button",
                    "my-environments-new-water:button",
                    "my-environments-new-day-cycle:button",
                    "my-environments-rename:button",
                    "my-environments-delete:button",
                ] {
                    let _greyed = alpha
                        .expect(&window.test_id(button))
                        .to_be_disabled()
                        .await?;
                }

                row_menu(alpha, &fixture, "menu-my-env-edit").await?;
                let editor = alpha.ui().window(SKY_EDITOR);
                let _open = alpha.expect(&editor).to_be_visible().await?;
                for button in [
                    "settings-editor-sky-save:button",
                    "settings-editor-sky-save-as:button",
                ] {
                    let _greyed = alpha
                        .expect(&editor.test_id(button))
                        .to_be_disabled()
                        .await?;
                }
                let _import = alpha
                    .expect(&editor.test_id("settings-editor-sky-import:button"))
                    .to_be_enabled()
                    .await?;

                let _bulk = alpha
                    .menu_path(&[
                        "menu-bar-world",
                        "menu-bar-environment",
                        "menu-bar-bulk-import",
                        "menu-bar-skies",
                    ])
                    .await?;
                let _refused = alpha
                    .expect_notification()
                    .timeout(WAIT)
                    .to_show("SettingsUnsuported")
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- Personal Lighting ------------------------------------------------

    /// The sky the viewer draws.
    async fn drawn_sky(alpha: &Viewer) -> Result<sl_automation_proto::SkyReadout, BodyError> {
        Ok(alpha
            .environment()
            .await?
            .sky
            .ok_or("the viewer draws no sky")?)
    }

    /// The fog density of the water the viewer draws.
    async fn drawn_water_fog(alpha: &Viewer) -> Result<f32, BodyError> {
        Ok(alpha
            .environment()
            .await?
            .water
            .ok_or("the viewer draws no water")?
            .fog_density)
    }

    /// Wait until the drawn sky satisfies `holds`, and answer it.
    async fn sky_until(
        alpha: &Viewer,
        what: &str,
        holds: impl Fn(&sl_automation_proto::SkyReadout) -> bool + Send + Sync,
    ) -> Result<sl_automation_proto::SkyReadout, BodyError> {
        let found = tokio::time::timeout(WAIT, async {
            loop {
                if let Ok(sky) = drawn_sky(alpha).await
                    && holds(&sky)
                {
                    return sky;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        found.map_err(|_elapsed| format!("the drawn sky never came to {what}").into())
    }

    /// **Personal Lighting**: a slider edits the local sky at once, the
    /// ambient swatch's colour picker recolours it, the sun trackball moves
    /// the sun — a click aims it, an arrow key nudges it — and Reset,
    /// confirmed, hands the sky back to the region.
    #[test]
    fn personal_lighting_edits_the_local_sky_and_resets_it() -> Result<(), TestError> {
        stage("environment_personal_lighting").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let local_sky = alpha.expect_state(Probe::Environment).at("/local_sky");
            let _shared = local_sky.clone().to_equal(json!(false)).await?;
            let _opened = alpha
                .menu_path(&[
                    "menu-bar-world",
                    "menu-bar-environment",
                    "menu-bar-personal-lighting",
                ])
                .await?;
            let window = alpha.ui().window(PERSONAL_LIGHTING);
            let _shown = alpha.expect(&window).to_be_visible().await?;

            slide_to_end(&window.test_id("personal-lighting-haze-density:slider")).await?;
            let _local = local_sky
                .clone()
                .timeout(WAIT)
                .to_equal(json!(true))
                .await?;
            let _hazy = sky_until(alpha, "the slider's haze", |sky| {
                (sky.haze_density - 5.0).abs() < 1e-3
            })
            .await?;

            let _picker = window
                .test_id("personal-lighting-ambient:color-swatch")
                .click()
                .await?;
            // The channels are spin buttons, as the reference's are: typed
            // into, and committed with Enter.
            let channel = |key: &str| {
                alpha
                    .ui()
                    .locator(Locator::role(Role::SpinButton).name_key(key))
            };
            for (key, value) in [
                ("color-picker-red-name", "255"),
                ("color-picker-green-name", "0"),
                ("color-picker-blue-name", "0"),
            ] {
                let _filled = channel(key).fill(value).await?;
                alpha.press("Enter").await?;
            }
            let _ok = alpha
                .ui()
                .test_id("color-picker-button:color-picker-ok")
                .click()
                .await?;
            let _red = sky_until(alpha, "a red ambient", |sky| {
                let [red, green, blue] = sky.ambient;
                red > 0.0 && green.abs() < 1e-3 && blue.abs() < 1e-3
            })
            .await?;

            let trackball = window.test_id("personal-lighting-sun:trackball");
            let _aimed = trackball.click().await?;
            // The trackball's centre is the zenith.
            let overhead = sky_until(alpha, "the sun overhead", |sky| {
                sky.sun[1] > 80.0_f32.to_radians()
            })
            .await?;
            alpha.press("ArrowDown").await?;
            let _lowered = sky_until(alpha, "the sun nudged down", |sky| {
                sky.sun[1] < overhead.sun[1] - 0.01
            })
            .await?;

            let _reset = window
                .test_id("personal-lighting-reset:button")
                .click()
                .await?;
            answer(alpha, "PersonalSettingsConfirmReset", "OK").await?;
            let _back = local_sky.timeout(WAIT).to_equal(json!(false)).await?;
            let _closed = alpha.expect(&window).to_be_hidden().await?;
            Ok(())
        })?;
        Ok(())
    }

    /// **The cross-fade**: with a manual transition time set in the
    /// debug-settings editor, a preset picked from World ▸ Environment fades in
    /// — the environment reports a fade under way, then none — rather than
    /// cutting.
    #[test]
    fn a_preset_cross_fades_over_the_manual_transition_time() -> Result<(), TestError> {
        stage("environment_cross_fade").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            alpha.press("Ctrl+Alt+Shift+S").await?;
            let editor = alpha.ui().window("debug_settings");
            let _open = alpha.expect(&editor).to_be_visible().await?;
            let _searched = editor
                .test_id("debug-settings:field")
                .fill("manualtransition")
                .await?;
            let _picked = editor
                .get(Locator::role(Role::ListItem).named("EnvironmentManualTransitionTime"))
                .click()
                .await?;
            let value = editor.test_id("debug-settings-f32:field");
            let _typed = value.fill("6").await?;
            value.press("Enter").await?;
            close_window(alpha, &editor).await?;

            let transition = alpha.expect_state(Probe::Environment).at("/transition");
            let _still = transition.clone().to_be_absent().await?;
            let _sunset = alpha
                .menu_path(&[
                    "menu-bar-world",
                    "menu-bar-environment",
                    "menu-bar-legacy",
                    "menu-bar-sunset",
                ])
                .await?;
            let _fading = transition.clone().to_be_present().await?;
            let _settled = transition
                .timeout(Duration::from_secs(30))
                .to_be_absent()
                .await?;
            Ok(())
        })?;
        Ok(())
    }
}
