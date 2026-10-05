//! End-to-end tests for what one viewer's windows show and refuse on the fake
//! grid, through the automation driver alone, on both backends
//! ([[test-e2e-sweep-single-viewer-ui]]):
//!
//! - the notecard and script windows' text bodies grow with the window when
//!   it is resized;
//! - the Build window's material controls grey on a prim the agent may not
//!   modify, and stay live on one it may;
//! - About Region's Experiences tab pins the estate's default experience to
//!   the Key list with no Remove, two About Region windows on two regions each
//!   get a picker of their own and only the one that asked takes the pick, and
//!   a picker closes with its window;
//! - a sky editor with unsaved changes asks before its ✕ closes it;
//! - every clothing layer in the inventory shows its own icon;
//! - About Landmark, opened from an inventory landmark, fills every row, shows
//!   the parcel's snapshot, copies its SLURL and commits a title and notes
//!   edit to the grid;
//! - the About window's simulator line names each region's own simulator
//!   across a teleport.

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{Bounds, InventoryRoot, Locator, Probe, Role};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_fake_grid::scenario::{Scenario, class_folder};
    use sl_fake_grid::{ImitatedGrid, RegionConfig};
    use sl_proto::{
        AgentKey, AssetKey, AssetType, GroupKey, InventoryItem, InventoryKey, InventoryType,
        Maturity, ObjectKey, OwnerKey, Permissions, Permissions5, RegionCoordinates,
        RegionLocalObjectId, ServerEvent, TextureKey, Uuid, WearableType, landmark_to_wire,
    };
    use sl_test_assets::inventory::fixture_id;
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

    /// A stage for `name` with the one viewer `Alpha`.
    fn stage(name: &str) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
    }

    // ---- The seeded home region -------------------------------------------

    /// The home region's id, fixed so a landmark can name it and a window
    /// keyed by it can be found.
    const HOME_ID: u128 = 0x57A6_E000_0000_0000_0000_0000_0000_0001;

    /// The neighbour region's id.
    const NEIGHBOUR_ID: u128 = 0x57A6_E000_0000_0000_0000_0000_0000_0002;

    /// The neighbour region's name.
    const NEIGHBOUR: &str = "Neighbour";

    /// The notecard the agent owns, and may edit.
    const NOTECARD: (u128, &str) = (0x57A6_E1E0_0001, "Stage Notecard");

    /// The script the agent owns.
    const SCRIPT: (u128, &str) = (0x57A6_E1E0_0002, "Stage Script");

    /// The landmark the agent owns, onto the home region.
    const LANDMARK: (u128, &str) = (0x57A6_E1E0_0003, "Stage Landmark");

    /// The landmark's asset: the home region at [`LANDMARK_AT`].
    const LANDMARK_ASSET: u128 = 0x57A6_E1A5_0003;

    /// Where the landmark points, region-local.
    const LANDMARK_AT: (f32, f32, f32) = (100.0, 90.0, 25.0);

    /// The clothing layers the inventory holds one item of each of, and the
    /// glyph each shows (`inventory.rs`'s `wearable_icon`).
    const LAYERS: [(WearableType, &str); 13] = [
        (WearableType::Shirt, "\u{1f455}"),
        (WearableType::Pants, "\u{1f456}"),
        (WearableType::Shoes, "\u{1f45f}"),
        (WearableType::Socks, "\u{1f9e6}"),
        (WearableType::Jacket, "\u{1f9e5}"),
        (WearableType::Gloves, "\u{1f9e4}"),
        (WearableType::Undershirt, "\u{1f3bd}"),
        (WearableType::Underpants, "\u{1fa72}"),
        (WearableType::Skirt, "\u{1f457}"),
        (WearableType::Alpha, "\u{1fae5}"),
        (WearableType::Tattoo, "\u{1f58b}\u{fe0f}"),
        (WearableType::Physics, "\u{269b}\u{fe0f}"),
        (WearableType::Universal, "\u{1f310}"),
    ];

    /// The first clothing item's id; the rest follow it.
    const FIRST_LAYER_ITEM: u128 = 0x57A6_E1E0_0100;

    /// The name the inventory shows the clothing item of `layer` by.
    fn layer_item_name(layer: WearableType) -> String {
        format!("Stage {layer:?} Layer")
    }

    /// An item of the agent's own, with every permission, filed where the grid
    /// files its class.
    fn own_item(
        agent: AgentKey,
        id: u128,
        name: &str,
        asset: Uuid,
        class: (AssetType, InventoryType),
        flags: u32,
    ) -> InventoryItem {
        let (asset_type, inv_type) = class;
        let everything = Permissions5 {
            base: Permissions::ALL,
            owner: Permissions::ALL,
            group: Permissions::NONE,
            everyone: Permissions::NONE,
            next_owner: Permissions::ALL,
        };
        InventoryItem {
            item_id: InventoryKey::from(Uuid::from_u128(id)),
            folder_id: class_folder(asset_type),
            name: name.to_owned(),
            description: String::new(),
            asset_id: asset,
            item_type: i8::try_from(asset_type.to_code()).unwrap_or(0),
            inv_type: i8::try_from(inv_type.to_code()).unwrap_or(0),
            flags,
            sale_type: 0,
            sale_price: None,
            creation_date: 0,
            owner: OwnerKey::Agent(agent),
            last_owner_id: Uuid::nil(),
            creator_id: agent,
            group: None,
            permissions: everything,
        }
    }

    /// What the agent owns beyond the stock fixtures: a notecard and a script
    /// it may edit, a landmark onto the home region, and one clothing item per
    /// layer.
    fn own_items(agent: AgentKey) -> Vec<InventoryItem> {
        let mut items = vec![
            own_item(
                agent,
                NOTECARD.0,
                NOTECARD.1,
                fixture_id(AssetType::Notecard),
                (AssetType::Notecard, InventoryType::Notecard),
                0,
            ),
            own_item(
                agent,
                SCRIPT.0,
                SCRIPT.1,
                fixture_id(AssetType::ScriptText),
                (AssetType::ScriptText, InventoryType::Script),
                0,
            ),
            own_item(
                agent,
                LANDMARK.0,
                LANDMARK.1,
                Uuid::from_u128(LANDMARK_ASSET),
                (AssetType::Landmark, InventoryType::Landmark),
                0,
            ),
        ];
        let mut id = FIRST_LAYER_ITEM;
        for (layer, _glyph) in LAYERS {
            // The icon is chosen by the layer in the item's flags; the body
            // is the stock shirt's, which the grid serves.
            items.push(own_item(
                agent,
                id,
                &layer_item_name(layer),
                fixture_id(AssetType::Clothing),
                (AssetType::Clothing, InventoryType::Wearable),
                u32::from(layer.to_code()),
            ));
            id = id.saturating_add(1);
        }
        items
    }

    /// The stock scenario with the agent's own items, the landmark's asset,
    /// and a snapshot on the stock parcel.
    fn seeded() -> Scenario {
        let mut scenario = Scenario::default();
        let (x, y, z) = LANDMARK_AT;
        let _replaced = scenario.assets.insert(
            AssetKey::from(Uuid::from_u128(LANDMARK_ASSET)),
            landmark_to_wire(Uuid::from_u128(HOME_ID), RegionCoordinates::new(x, y, z)),
        );
        if let Some(parcel) = scenario.world.parcels.first_mut() {
            parcel.snapshot_id = Some(TextureKey::from(fixture_id(AssetType::Texture)));
        }
        let stock = scenario.setup_for_agent.clone();
        scenario.setup_for_agent = Some(Arc::new(move |sim, identity, now| {
            if let Some(stock) = &stock {
                stock(sim, identity, now);
            }
            for item in own_items(identity.agent_id) {
                sim.agent_inventory_mut().insert_item(item);
            }
        }));
        scenario
    }

    /// The home region: the stock region under a fixed id, seeded.
    fn home() -> RegionConfig {
        RegionConfig {
            region_id: Some(Uuid::from_u128(HOME_ID)),
            scenario: Some(seeded()),
            ..RegionConfig::default()
        }
    }

    /// The region east of [`home`], under a fixed id.
    fn neighbour() -> RegionConfig {
        let home = RegionConfig::default();
        RegionConfig {
            name: NEIGHBOUR.to_owned(),
            grid_x: home.grid_x.saturating_add(1),
            region_id: Some(Uuid::from_u128(NEIGHBOUR_ID)),
            ..home
        }
    }

    /// The group that owns the parcel of [`adult_group_home`].
    const LANDLORD_GROUP: u128 = 0x57A6_E160_0003;

    /// [`home`] rated adult, its parcel owned by [`LANDLORD_GROUP`]: the two
    /// things a parcel listing's flags byte says that its other fields do not.
    fn adult_group_home() -> RegionConfig {
        let mut region = home();
        region.maturity = Maturity::Adult;
        if let Some(parcel) = region
            .scenario
            .as_mut()
            .and_then(|scenario| scenario.world.parcels.first_mut())
        {
            let group = GroupKey::from(Uuid::from_u128(LANDLORD_GROUP));
            parcel.owner = OwnerKey::Group(group);
            parcel.group = Some(group);
        }
        region
    }

    // ---- Helpers ----------------------------------------------------------

    /// Wait until the inventory window's model holds `name` in the top-level
    /// folder `folder`: the folder's page has arrived, so the rows a search
    /// shows have stopped moving under the pointer.
    async fn item_known(alpha: &Viewer, folder: &str, name: &str) -> Result<(), BodyError> {
        let _listed = alpha
            .expect_state(Probe::Inventory {
                root: InventoryRoot::Agent,
                path: vec![folder.to_owned()],
            })
            .at("/items")
            .timeout(WAIT)
            .to_include(json!([{ "name": name }]))
            .await?;
        Ok(())
    }

    /// Open the inventory, narrow it to `name` — an item of the top-level
    /// folder `folder` — and pick the row's context menu entry whose caption
    /// is `entry`. The search is what fetches the folders nobody has opened.
    async fn inventory_item_menu(
        alpha: &Viewer,
        folder: &str,
        name: &str,
        entry: &str,
    ) -> Result<(), BodyError> {
        let inventory = inventory(alpha, name).await?;
        item_known(alpha, folder, name).await?;
        let _menu = item_row(&inventory, name).right_click().await?;
        let _picked = alpha
            .ui()
            .locator(Locator::role(Role::MenuItem).name_key(entry))
            .click()
            .await?;
        Ok(())
    }

    /// The inventory window, open and searched for `term`.
    async fn inventory(alpha: &Viewer, term: &str) -> Result<UiLocator, BodyError> {
        let inventory = alpha.ui().window("inventory");
        if !inventory.is_visible().await? {
            alpha.press("Ctrl+I").await?;
            let _shown = alpha.expect(&inventory).to_be_visible().await?;
        }
        let _searched = inventory
            .test_id("inventory:search")
            .role(Role::Textbox)
            .fill(term)
            .await?;
        Ok(inventory)
    }

    /// The inventory row naming the item `name`.
    fn item_row(inventory: &UiLocator, name: &str) -> UiLocator {
        inventory
            .get(Locator::role(Role::TreeItem).named(name))
            .timeout(WAIT)
    }

    /// Close `window` with its own close button, and wait for it to go.
    async fn close_window(alpha: &Viewer, window: &UiLocator) -> Result<(), BodyError> {
        let _closed = window.test_id("floater-button:close").click().await?;
        let _gone = alpha.expect(window).to_be_hidden().await?;
        Ok(())
    }

    /// The box of the one node `locator` names, once it shows.
    async fn bounds_of(alpha: &Viewer, locator: &UiLocator) -> Result<Bounds, BodyError> {
        let shown = alpha.expect(locator).timeout(WAIT).to_be_visible().await?;
        shown
            .first()
            .map(|node| node.bounds)
            .ok_or_else(|| "no node to measure".into())
    }

    /// Read `locator`'s box until it is at least `width` × `height`, a layout
    /// pass or two after whatever changed it; the last box read either way.
    async fn bounds_reaching(
        locator: &UiLocator,
        width: f32,
        height: f32,
    ) -> Result<Bounds, BodyError> {
        let mut last = locator.node().await?.bounds;
        for _read in 0..30 {
            if last.width >= width && last.height >= height {
                break;
            }
            last = locator.node().await?.bounds;
        }
        Ok(last)
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

    /// Teleport to `region` through the world map's search, and wait until
    /// the agent is there; the map is closed again afterwards.
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

    // ---- Multi-line fields ------------------------------------------------

    /// How far the editors' windows are dragged bigger.
    const GROW: (f32, f32) = (160.0, 120.0);

    /// **A body that fills its window**: the notecard and the script windows
    /// are dragged bigger by their resize grip, and their text bodies grow by
    /// as much — not a lake of empty window around a field that kept its size.
    #[test]
    fn a_notecard_and_a_script_body_grow_with_their_window() -> Result<(), TestError> {
        stage("multiline_fills_its_window")
            .region(home())
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                for ((id, name), folder, kind, body) in [
                    (
                        NOTECARD,
                        "Notecards",
                        "notecard-editor",
                        "notecard-body:field",
                    ),
                    (SCRIPT, "Scripts", "script-editor", "script-body:field"),
                ] {
                    inventory_item_menu(alpha, folder, name, "menu-inv-open").await?;
                    let window = alpha
                        .ui()
                        .window(&format!("{kind}#{}", Uuid::from_u128(id)));
                    let field = window.test_id(body);
                    let before = bounds_of(alpha, &field).await?;
                    let (wider, taller) = GROW;
                    let _dragged = window
                        .test_id("floater-resize")
                        .drag_by(wider, taller)
                        .await?;
                    // The field takes the window's growth less nothing: allow
                    // a few pixels for rounding at the edges.
                    let want_width = before.width + wider - 4.0;
                    let want_height = before.height + taller - 4.0;
                    let after = bounds_reaching(&field, want_width, want_height).await?;
                    assert!(
                        after.width >= want_width && after.height >= want_height,
                        "the {kind} body grew from {before:?} to {after:?} in a window grown by \
                         {GROW:?}"
                    );
                    close_window(alpha, &window).await?;
                }
                Ok(())
            })?;
        Ok(())
    }

    // ---- The Build window's material gate ---------------------------------

    /// The Build window's floater id.
    const BUILD_WINDOW: &str = "build-tools";

    /// The prim the agent owns, beside the stock box.
    const OWN_PRIM: &str = "Own Prim";

    /// Its region-local id, clear of the stock scene's.
    const OWN_PRIM_LOCAL_ID: u32 = 0x57A6;

    /// The material controls the gate greys, one per material mode, with the
    /// mode tab that shows it.
    const MATERIAL_CONTROLS: [(&str, &str); 2] = [
        ("build-tex-matmedia-material", "build-tex-alpha-mode:combo"),
        (
            "build-tex-matmedia-pbr",
            "build-tex-pbr-material:texture-swatch",
        ),
    ];

    /// Put a prim the agent `label` owns beside the stock box, and show it.
    async fn rez_own_prim(stage: &Stage, label: &str) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        let mut prim = sl_fake_grid::world::box_prim(
            RegionLocalObjectId(OWN_PRIM_LOCAL_ID),
            ObjectKey::from(Uuid::from_u128(0x57A6_0B1E)),
            stage.agent_id(label)?,
            sl_proto::Vector {
                x: 126.0,
                y: 124.0,
                z: 25.5,
            },
            sl_proto::Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        let mut properties = sl_fake_grid::world::default_object_properties(&prim);
        properties.name = OWN_PRIM.to_owned();
        prim.properties = Some(properties);
        agent
            .with_world(|world, sim| {
                world.objects.push(prim.clone());
                sl_fake_grid::world::send_objects(sim, &[prim], now)
            })
            .await
            .map_err(|error| format!("showing the own prim: {error}"))?;
        Ok(())
    }

    /// Select `object` with the Build window's Move tool and show the Texture
    /// tab's mode `mode`.
    async fn material_page(
        alpha: &Viewer,
        object: &str,
        mode: &str,
    ) -> Result<UiLocator, BodyError> {
        let build = alpha.ui().window(BUILD_WINDOW);
        let _moving = build
            .get(Locator::role(Role::Radio).name_key("build-tool-move"))
            .click()
            .await?;
        let _selected = alpha.world().object_named(object).select().await?;
        let _texture = build
            .get(Locator::role(Role::Tab).name_key("build-tab-texture"))
            .click()
            .await?;
        let _mode = build
            .get(Locator::role(Role::Tab).name_key(mode))
            .click()
            .await?;
        Ok(build)
    }

    /// **The material gate**: with the stock box selected — somebody else's,
    /// which the agent may not modify — each material mode's controls are
    /// greyed; with the agent's own prim selected they are live again.
    #[test]
    fn the_material_controls_grey_on_a_prim_the_agent_may_not_modify() -> Result<(), TestError> {
        stage("material_permission_gate")
            .needs(Need::GridControl)
            .needs(Need::Content(
                "the stock scene's box, owned by somebody else",
            ))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                rez_own_prim(stage, "Alpha").await?;
                let _named = alpha
                    .world()
                    .object_named(OWN_PRIM)
                    .timeout(WAIT)
                    .node()
                    .await?;
                alpha.press("Ctrl+B").await?;
                let _open = alpha
                    .expect(&alpha.ui().window(BUILD_WINDOW))
                    .to_be_visible()
                    .await?;
                for (mode, control) in MATERIAL_CONTROLS {
                    let build = material_page(alpha, "Object", mode).await?;
                    let _greyed = alpha
                        .expect(&build.test_id(control))
                        .to_be_disabled()
                        .await?;
                    let build = material_page(alpha, OWN_PRIM, mode).await?;
                    let _live = alpha
                        .expect(&build.test_id(control))
                        .to_be_enabled()
                        .await?;
                }
                Ok(())
            })?;
        Ok(())
    }

    // ---- About Region's experiences and their pickers ---------------------

    /// The Region / Estate window's menu path.
    const REGION_ESTATE: [&str; 2] = ["menu-bar-world", "menu-bar-region-estate"];

    /// The estate's default experience, as the fake grid names it.
    const DEFAULT_EXPERIENCE: &str = "Fake Grid Estate Default";

    /// An experience the fake grid lists as trusted, beside the default.
    const TRUSTED_EXPERIENCE: &str = "Fake Grid Weather";

    /// The narrowest an experience row's name cell may be: room for a name,
    /// not an ellipsis.
    const NAME_CELL_MIN: f32 = 100.0;

    /// The fixture's land-scoped experience beside the default, which the
    /// Allowed picker offers.
    const LAND_EXPERIENCE: &str = "Fake Grid Arena";

    /// The grid-scoped experience the picker test blocks.
    const PICKED_EXPERIENCE: &str = "Fake Grid Tour";

    /// The About Region window of the region `id`.
    fn about_region(alpha: &Viewer, id: u128) -> UiLocator {
        alpha
            .ui()
            .window(&format!("about-region#{}", Uuid::from_u128(id)))
    }

    /// Open the Region / Estate window on the region the agent is in — the
    /// region `id` — on its Experiences tab.
    async fn experiences_tab(alpha: &Viewer, id: u128) -> Result<UiLocator, BodyError> {
        let _opened = alpha.menu_path(&REGION_ESTATE).await?;
        let window = about_region(alpha, id);
        let _shown = alpha.expect(&window).timeout(WAIT).to_be_visible().await?;
        let _tab = window
            .get(Locator::role(Role::Tab).name_key("about-region-tab-experiences"))
            .click()
            .await?;
        Ok(window)
    }

    /// The row of `list` (`trusted`, `allowed`, `blocked`) naming `name`.
    fn experience_row(window: &UiLocator, list: &str, name: &str) -> UiLocator {
        window
            .test_id(&format!("about-region-experiences-{list}:table"))
            .get(Locator::role(Role::ListItem).name_containing(name))
            .timeout(WAIT)
    }

    /// **The estate's default experience**: About Region's Experiences tab
    /// lists it in the Key list beside the region's trusted one, and only the
    /// trusted one's row offers Remove.
    #[test]
    fn the_default_experience_is_a_key_row_that_cannot_be_removed() -> Result<(), TestError> {
        stage("experiences_default")
            .region(home())
            .estate_manager("Alpha")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let window = experiences_tab(alpha, HOME_ID).await?;
                let default = experience_row(&window, "trusted", DEFAULT_EXPERIENCE);
                let trusted = experience_row(&window, "trusted", TRUSTED_EXPERIENCE);
                let _listed = alpha.expect(&default).timeout(WAIT).to_be_visible().await?;
                let _beside = alpha.expect(&trusted).to_be_visible().await?;
                // Named in full, but also *shown*: the name's cell has room
                // beside the rating and the buttons, not an ellipsis.
                let name_cell = default.test_id("about-region-experiences-trusted:table-cell:0");
                let shown = bounds_of(alpha, &name_cell).await?;
                assert!(
                    shown.width >= NAME_CELL_MIN,
                    "the default experience's name has {shown:?} to show in"
                );
                let remove = |row: &UiLocator| row.test_id("about-region-experience-row:Remove");
                let _sticky = alpha.expect(&remove(&default)).to_be_hidden().await?;
                let _removable = alpha.expect(&remove(&trusted)).to_be_visible().await?;
                // The default is not on offer to allow, either.
                let _add = window
                    .test_id("about-region-button:about-region-experiences-add-allowed")
                    .click()
                    .await?;
                let picker = alpha.ui().window(&format!(
                    "experience-picker#about-region/{}/about-region-experience-allowed",
                    Uuid::from_u128(HOME_ID)
                ));
                // Allowed takes land-scoped experiences: the fixture's other
                // one is rated Moderate, so the filter must admit it.
                let _shown = alpha.expect(&picker).timeout(WAIT).to_be_visible().await?;
                let _rating = picker
                    .test_id("experience-picker-rating:combo")
                    .select_option(
                        Locator::role(Role::ListItem).name_key("experience-rating-moderate"),
                    )
                    .await?;
                let results = picker_search(alpha, &picker, "Fake Grid").await?;
                let _others = alpha
                    .expect(
                        &results
                            .get(Locator::role(Role::ListItem).name_containing(LAND_EXPERIENCE)),
                    )
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let _excluded = alpha
                    .expect(
                        &results
                            .get(Locator::role(Role::ListItem).name_containing(DEFAULT_EXPERIENCE)),
                    )
                    .to_be_detached()
                    .await?;
                Ok(())
            })?;
        Ok(())
    }

    /// Search `picker` for `term`, and answer its results table.
    async fn picker_search(
        alpha: &Viewer,
        picker: &UiLocator,
        term: &str,
    ) -> Result<UiLocator, BodyError> {
        let _shown = alpha.expect(picker).timeout(WAIT).to_be_visible().await?;
        let query = picker.test_id("experience-picker-query:field");
        let _typed = query.fill(term).await?;
        query.press("Enter").await?;
        Ok(picker.test_id("experience-picker-results:table"))
    }

    /// **One picker per window**: About Region on the home region and Add on
    /// its Blocked list; a teleport next door, About Region there too — a
    /// second window, keyed by the other region — and Add on its Blocked list
    /// opens a second picker rather than taking over the first. A pick in the
    /// second lands in the second window's list and is posted once, while the
    /// first window's list and picker are untouched. Each picker closes with
    /// the window that opened it.
    #[test]
    fn two_region_windows_each_get_their_own_picker_and_pick() -> Result<(), TestError> {
        stage("experience_picker_per_window")
            .region(home())
            .region(neighbour())
            .estate_manager("Alpha")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let picker_of = |id: u128| {
                    alpha.ui().window(&format!(
                        "experience-picker#about-region/{}/about-region-experience-blocked",
                        Uuid::from_u128(id)
                    ))
                };
                let add = "about-region-button:about-region-experiences-add-blocked";

                let first = experiences_tab(alpha, HOME_ID).await?;
                let _first_add = first.test_id(add).click().await?;
                let first_picker = picker_of(HOME_ID);
                let _first_open = alpha
                    .expect(&first_picker)
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;

                teleport_by_map(alpha, NEIGHBOUR).await?;
                let second = experiences_tab(alpha, NEIGHBOUR_ID).await?;
                let _second_add = second.test_id(add).click().await?;
                let second_picker = picker_of(NEIGHBOUR_ID);
                let _second_open = alpha
                    .expect(&second_picker)
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let _both = alpha.expect(&first_picker).to_be_attached().await?;

                let mut heard = stage.agent("Alpha").await?.events();
                let results = picker_search(alpha, &second_picker, PICKED_EXPERIENCE).await?;
                let _row = results
                    .get(Locator::role(Role::ListItem).name_containing(PICKED_EXPERIENCE))
                    .timeout(WAIT)
                    .click()
                    .await?;
                let _picked = second_picker
                    .get(Locator::role(Role::Button).name_key("experience-picker-select"))
                    .click()
                    .await?;
                let posted = grid_hears(&mut heard, "blocked-list post", |event| match event {
                    ServerEvent::RegionExperiencesSet { blocked, .. } => Some(blocked.clone()),
                    _ => None,
                })
                .await?;
                assert_eq!(
                    posted.len(),
                    2,
                    "the stock blocked experience and the picked one: {posted:?}"
                );
                let _second_lists = alpha
                    .expect(&experience_row(&second, "blocked", PICKED_EXPERIENCE))
                    .timeout(WAIT)
                    .to_be_attached()
                    .await?;
                let _first_does_not = alpha
                    .expect(&experience_row(&first, "blocked", PICKED_EXPERIENCE))
                    .to_be_detached()
                    .await?;
                let _first_waits = alpha.expect(&first_picker).to_be_attached().await?;

                // The second window is on top: close it, then the first.
                close_window(alpha, &second).await?;
                let _second_gone = alpha.expect(&second_picker).to_be_detached().await?;
                let _first_stays = alpha.expect(&first_picker).to_be_attached().await?;
                close_window(alpha, &first).await?;
                let _first_gone = alpha.expect(&first_picker).to_be_detached().await?;
                Ok(())
            })?;
        Ok(())
    }

    // ---- A dirty editor's close -------------------------------------------

    /// The sky editor's floater id.
    const SKY_EDITOR: &str = "settings-editor-sky";

    /// The confirmation an editor with unsaved changes asks before closing.
    const CONFIRM_LOSS: &str = "SettingsConfirmLoss";

    /// **A dirty editor asks before it closes**: a new sky from My
    /// Environments, opened in the sky editor and a knob moved, is asked to
    /// close by its ✕ — it stays, and asks; No keeps it open; asked again,
    /// Yes closes it.
    #[test]
    fn a_sky_editor_with_unsaved_changes_asks_before_it_closes() -> Result<(), TestError> {
        stage("dirty_editor_close").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let _opened = alpha
                .menu_path(&[
                    "menu-bar-world",
                    "menu-bar-environment",
                    "menu-bar-my-environments",
                ])
                .await?;
            let environments = alpha.ui().window("my-environments");
            let _created = environments
                .test_id("my-environments-new-sky:button")
                .click()
                .await?;
            let row = environments
                .get(Locator::role(Role::ListItem).name_containing("New Sky"))
                .timeout(WAIT);
            let _menu = row.right_click().await?;
            let _edit = alpha
                .ui()
                .locator(Locator::role(Role::MenuItem).name_key("menu-my-env-edit"))
                .click()
                .await?;
            let editor = alpha.ui().window(SKY_EDITOR);
            let _shown = alpha.expect(&editor).timeout(WAIT).to_be_visible().await?;
            editor
                .test_id("settings-editor-sky-haze-density:slider")
                .press("End")
                .await?;
            let close = editor.test_id("floater-button:close");

            let _asked = close.click().await?;
            let _confirm = alpha
                .expect_notification()
                .timeout(WAIT)
                .to_show(CONFIRM_LOSS)
                .await?;
            let _kept = alpha.expect(&editor).to_be_visible().await?;
            let no = alpha.ui().test_id("toast-button:Cancel");
            let _no = no.click().await?;
            let _answered = alpha.expect(&no).to_be_detached().await?;
            let _still = alpha.expect(&editor).to_be_visible().await?;

            let _again = close.click().await?;
            let _yes = alpha.ui().test_id("toast-button:OK").click().await?;
            let _closed = alpha.expect(&editor).to_be_hidden().await?;
            Ok(())
        })?;
        Ok(())
    }

    // ---- Inventory icons --------------------------------------------------

    /// **One icon per clothing layer**: each clothing item in the inventory
    /// shows its own layer's glyph beside its name — not every one the shirt.
    #[test]
    fn every_clothing_layer_shows_its_own_icon() -> Result<(), TestError> {
        stage("clothing_icons")
            .region(home())
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let inventory = inventory(alpha, "Layer").await?;
                for (layer, glyph) in LAYERS {
                    let row = item_row(&inventory, &layer_item_name(layer));
                    // Attached, not visible: the list is taller than its
                    // window, and a row below the fold is still bound.
                    let _icon = alpha
                        .expect(&row.get(Locator::role(Role::Text).named(glyph)))
                        .timeout(WAIT)
                        .to_be_attached()
                        .await
                        .map_err(|error| format!("the {layer:?} item's icon: {error}"))?;
                }
                Ok(())
            })?;
        Ok(())
    }

    // ---- About Landmark ---------------------------------------------------
    //
    // The parcel rows of this window (name, rating, owner, traffic, area) come
    // from the parcel's *listing* (`ParcelInfoReply`). These tests need
    // `Need::GridControl`, so they run against the fake grid only, which
    // answers a listing from the live parcel record. If one is ever ported to
    // the live OpenSim (`SL_E2E_GRID=opensim`) and a row shows the parcel as
    // it was before an edit, that is OpenSim's listing cache — an entry lives
    // 30 s past its last read, so reopening the window to look again keeps the
    // stale answer alive. Wait ~45 s without opening anything that asks for
    // the listing. Measured by the `parcel-info-dwell` conformance case
    // (`LISTING_CACHE_WAIT`); written up in `book/src/gridspec/land.md`,
    // § Parcel info.

    /// The About Landmark window of the seeded landmark.
    fn about_landmark(alpha: &Viewer) -> UiLocator {
        alpha
            .ui()
            .window(&format!("about-landmark#{}", Uuid::from_u128(LANDMARK.0)))
    }

    /// The landmark's new title and notes.
    const NEW_TITLE: &str = "Stage Landmark Renamed";

    /// The notes the test writes.
    const NEW_NOTES: &str = "Where the stage starts";

    /// Whether `event` rewrites the seeded landmark's name and description in
    /// a way `wanted` accepts — over the legacy `UpdateInventoryItem` or the
    /// inventory API's item patch, whichever the viewer used.
    fn landmark_written(event: &ServerEvent, wanted: impl Fn(&str, &str) -> bool) -> Option<()> {
        let landmark = InventoryKey::from(Uuid::from_u128(LANDMARK.0));
        let written = match event {
            ServerEvent::UpdateAgentInventoryItems { items, .. } => items.iter().any(|updated| {
                updated.item.item_id == landmark
                    && wanted(&updated.item.name, &updated.item.description)
            }),
            ServerEvent::InventoryItemUpdated {
                item_id,
                name,
                description,
            } => *item_id == landmark && wanted(name, description),
            _ => false,
        };
        written.then_some(())
    }

    /// **About Landmark, the listing's flags**: a landmark onto a group's
    /// parcel in an adult region is rated Adult and owned by the group on
    /// either grid. The two pack an adult rating differently in the listing's
    /// flags byte — Second Life sets the moderate bit beside the adult one,
    /// OpenSim does not — and group ownership is the bit this viewer once took
    /// for "for sale" (`gridspec-parcel-info-dwell`).
    #[test]
    fn about_landmark_reads_the_rating_and_the_group_owner_on_each_grid() -> Result<(), TestError> {
        for (name, flavour) in [
            ("about_landmark_flags_second_life", ImitatedGrid::SecondLife),
            ("about_landmark_flags_opensim", ImitatedGrid::OpenSim),
        ] {
            stage(name)
                .region(adult_group_home())
                .configure_grid(move |grid| grid.imitates(flavour))
                .needs(Need::GridControl)
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    inventory_item_menu(alpha, "Landmarks", LANDMARK.1, "menu-inv-about-landmark")
                        .await?;
                    let window = about_landmark(alpha);
                    let _shown = alpha.expect(&window).timeout(WAIT).to_be_visible().await?;
                    let _rated = alpha
                        .expect(&window.get(Locator::role(Role::Text).named("Adult")))
                        .timeout(WAIT)
                        .to_be_visible()
                        .await?;
                    // No grid here knows the group's name, so the row shows its
                    // id the way an unresolved group is shown — in brackets,
                    // which an agent of that id never is.
                    let group = GroupKey::from(Uuid::from_u128(LANDLORD_GROUP));
                    let _owned = alpha
                        .expect(&window.get(Locator::role(Role::Text).named(format!("({group})"))))
                        .timeout(WAIT)
                        .to_be_visible()
                        .await?;
                    Ok(())
                })?;
        }
        Ok(())
    }

    /// **About Landmark**: opened from the inventory row's About Landmark
    /// entry, it resolves the landmark to its region and coordinates, the
    /// parcel's name and snapshot; Copy SLURL puts the SLURL where a paste
    /// finds it; a title and a notes edit, each committed with Enter, reach
    /// the grid as the item's new name and description.
    #[test]
    fn about_landmark_fills_its_rows_copies_its_slurl_and_commits_edits() -> Result<(), TestError> {
        stage("about_landmark")
            .region(home())
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                inventory_item_menu(alpha, "Landmarks", LANDMARK.1, "menu-inv-about-landmark")
                    .await?;
                let window = about_landmark(alpha);
                let _shown = alpha.expect(&window).timeout(WAIT).to_be_visible().await?;
                let text = |content: &str| window.get(Locator::role(Role::Text).named(content));
                let (x, y, z) = LANDMARK_AT;
                let region_line = format!("{} ({x}, {y}, {z})", stage.home_region());
                let _region = alpha
                    .expect(
                        &window.get(Locator::role(Role::Text).name_containing(stage.home_region())),
                    )
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                let _parcel = alpha
                    .expect(&text("Fake Grid Parcel"))
                    .timeout(WAIT)
                    .to_be_visible()
                    .await?;
                // Every "(loading)" is gone once the rows resolve and the
                // snapshot lands (its placeholder goes with it), and the box
                // never says it has no image.
                let _resolved = alpha
                    .expect(&text("(loading)"))
                    .timeout(WAIT)
                    .to_be_detached()
                    .await?;
                let _pictured = alpha.expect(&text("(no image)")).to_be_detached().await?;

                let _copied = window
                    .test_id("about-landmark:about-landmark-copy-slurl")
                    .click()
                    .await?;
                let bar = alpha.ui().test_id("nearby-chat-bar").role(Role::Textbox);
                let _focused = bar.click().await?;
                bar.press("Ctrl+V").await?;
                let pasted = alpha.expect(&bar).to_contain_text("secondlife").await?;
                let slurl = pasted
                    .first()
                    .and_then(|node| match &node.value {
                        Some(sl_automation_proto::NodeValue::Text(text)) => Some(text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                assert!(
                    slurl.contains(&stage.home_region().replace(' ', "%20"))
                        && slurl.contains("/100/90/25"),
                    "the SLURL names the region and the landmark's place: {slurl:?} (the region \
                     line reads {region_line:?})"
                );
                let _cleared = bar.fill("").await?;

                let mut heard = stage.agent("Alpha").await?.events();
                let title = window.test_id("about-landmark-name:field");
                let _titled = title.fill(NEW_TITLE).await?;
                title.press("Enter").await?;
                grid_hears(&mut heard, "title edit", |event| {
                    landmark_written(event, |name, _notes| name == NEW_TITLE)
                })
                .await?;
                let notes = window.test_id("about-landmark-notes:field");
                let _noted = notes.fill(NEW_NOTES).await?;
                notes.press("Enter").await?;
                grid_hears(&mut heard, "notes edit", |event| {
                    landmark_written(event, |_name, notes| notes == NEW_NOTES)
                })
                .await
            })?;
        Ok(())
    }

    // ---- The About window's simulator line --------------------------------

    /// The neighbour's simulator, unlike the home region's stock one.
    const NEIGHBOUR_SIMULATOR: &str = "Stage Neighbour Simulator 7.1";

    /// **The simulator line follows a teleport**: two regions whose
    /// simulators name themselves differently; the About window's support
    /// block names the home one, and after a teleport the neighbour's.
    #[test]
    fn the_about_simulator_line_names_each_regions_own_simulator() -> Result<(), TestError> {
        let neighbour = RegionConfig {
            simulator_version: Some(NEIGHBOUR_SIMULATOR.to_owned()),
            ..neighbour()
        };
        stage("about_simulator_version")
            .region(RegionConfig::default())
            .region(neighbour)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let about_path = ["menu-bar-help", "menu-bar-about"];
                let _opened = alpha.menu_path(&about_path).await?;
                let block = alpha
                    .ui()
                    .window("about")
                    .test_id("about:info:support-block");
                let _home = alpha
                    .expect(&block)
                    .to_contain_text("Simulator version: sl-fake-grid")
                    .await?;
                let _closed = alpha.menu_path(&about_path).await?;
                teleport_by_map(alpha, NEIGHBOUR).await?;
                let _reopened = alpha.menu_path(&about_path).await?;
                let _there = alpha
                    .expect(&block)
                    .timeout(WAIT)
                    .to_contain_text(&format!("Simulator version: {NEIGHBOUR_SIMULATOR}"))
                    .await?;
                Ok(())
            })?;
        Ok(())
    }
}
