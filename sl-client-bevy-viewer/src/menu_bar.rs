//! The viewer's **live top menu bar** (`viewer-ui-menu-bar`): the actual strip
//! of pull-down menus at the top of the screen, built on the reusable line-menu
//! widget ([`crate::menu`]).
//!
//! # Names now, entries as they land
//!
//! This is the *bar*, not the hundreds of entries the reference viewer's menus
//! hold. It stands up the **top-level menu names** — Avatar, Comm, World, Build,
//! Content, Help — in their reference arrangement, so every future UI task has a
//! home to hang its command in (an inventory toggle under Avatar, a mini-map
//! toggle under World, and so on), and wires only the entries that already have
//! something to do: **Quit**, and the **Inventory** window that already exists.
//! A menu with nothing wired yet shows a single disabled placeholder, so it is a
//! real (openable) menu that visibly reads as "not populated yet" rather than a
//! dead button — exactly the way the pie shipped its mechanism with a fixture
//! and left the per-domain entries to their own tasks.
//!
//! The same shape is why the widget lives in [`crate::menu`] and this module is
//! thin: a future task adds a [`MenuItemDef`] to one of the `static` menus here
//! (or a whole new domain menu) and wires its `action` string in
//! [`handle_top_menu_actions`]; nothing about the bar itself has to change.
//!
//! # Wiring is by action string, testably
//!
//! The bar emits a [`UiAction`] per pick, exactly like every other widget, and
//! [`handle_top_menu_actions`] routes the ones with a live target. So the bar is
//! still constructible with no session (the registry rule), and what a pick
//! *does* is a separate, readable dispatch rather than a callback buried in the
//! menu declaration.
//!
//! Deliberately **not** here yet: the reference's **status area** (the region /
//! parcel name, agent position, L$ balance, time, FPS and the parcel-permission
//! icons that share the menu bar's row) — a substantial, separate concern with
//! its own data sources, split out to its own `viewer-ui-status-bar` task as the
//! menu-bar roadmap note anticipated.

use bevy::prelude::*;

use crate::floater::toggle_floater;
use crate::menu::{
    MenuBarDef, MenuCommand, MenuConditions, MenuDef, MenuItemDef, PrimaryMenuBar,
    TOP_MENU_ELEMENT, spawn_menu_bar,
};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems};
use crate::ui_element::{ElementCx, UiAction};

/// The z-index the bar renders at — above the floaters (so a window never covers
/// the menu bar), below an open menu's popup (`crate::menu`'s `MENU_Z_INDEX`).
const TOP_BAR_Z: i32 = 9_000;

/// The condition key that holds while the inventory window is open — drives the
/// check mark on the Avatar ▸ Inventory entry.
const INVENTORY_OPEN: &str = "inventory-open";

/// Condition: the Preferences floater is open (drives its check mark).
const PREFERENCES_OPEN: &str = "preferences-open";

/// Condition: the debug-settings editor is open (drives its check mark).
const DEBUG_SETTINGS_OPEN: &str = "debug-settings-open";

/// Condition: the About floater is open (drives its check mark).
const ABOUT_OPEN: &str = "about-open";

/// The condition key that holds while the Conversations floater is open — drives
/// the check mark on the Comm ▸ Conversations entry.
const CONVERSATIONS_OPEN: &str = "conversations-open";

/// The condition keys that hold while each presence mode is on — they drive the
/// check marks on Comm ▸ Online Status ([`crate::presence`]).
const PRESENCE_AWAY: &str = "presence-away-on";
/// See [`PRESENCE_AWAY`].
const PRESENCE_DO_NOT_DISTURB: &str = "presence-do-not-disturb-on";
/// See [`PRESENCE_AWAY`].
const PRESENCE_AUTORESPOND: &str = "presence-autorespond-on";
/// See [`PRESENCE_AWAY`].
const PRESENCE_AUTORESPOND_NON_FRIENDS: &str = "presence-autorespond-non-friends-on";

/// The condition keys that hold while each auto-reject mode is on — they drive
/// the check marks on the same submenu ([`crate::auto_reject`]).
const REJECT_TELEPORT_OFFERS: &str = "reject-teleport-offers-on";
/// See [`REJECT_TELEPORT_OFFERS`].
const REJECT_GROUP_INVITES: &str = "reject-group-invites-on";
/// See [`REJECT_TELEPORT_OFFERS`].
const REJECT_FRIENDSHIP_REQUESTS: &str = "reject-friendship-requests-on";

/// The condition key that holds while the Experiences floater is open — drives the
/// check mark on the Avatar ▸ Experiences entry.
const EXPERIENCES_OPEN: &str = "experiences-open";

/// The condition key that holds while the web browser floater is open — drives
/// the check mark on the Content ▸ Web Browser entry.
const WEB_BROWSER_OPEN: &str = "web-browser-open";

/// The condition key that holds while the Search floater is open — drives the
/// check mark on the Content ▸ Search entry.
const SEARCH_OPEN: &str = "search-open";

/// The condition key that holds while the minimap floater is open — drives the
/// check mark on the World ▸ Mini-Map entry.
const MINIMAP_OPEN: &str = "minimap-open";

/// The condition key that holds while the world-map floater is open — drives
/// the check mark on the World ▸ World Map entry.
const WORLD_MAP_OPEN: &str = "world-map-open";

/// The condition key that holds while the avatar radar floater is open —
/// drives the check mark on the World ▸ Radar entry.
const RADAR_OPEN: &str = "radar-open";

/// The condition key that holds while the friends-only render filter is on —
/// drives the check mark on the World ▸ Show Friends Only entry.
const FRIENDS_ONLY_ON: &str = "render-friends-only-on";

/// The condition key that holds while the Asset Blacklist floater is open —
/// drives the check mark on the World ▸ Asset Blacklist entry.
const BLACKLIST_OPEN: &str = "asset-blacklist-open";

/// The condition key that holds while the Avatar Render Settings floater is
/// open — drives the check mark on the World ▸ Avatar Render Settings entry.
const AVATAR_RENDER_SETTINGS_OPEN: &str = "avatar-render-settings-open";

/// The condition key that holds while the in-world property lines are shown —
/// drives the check mark on the World ▸ Property Lines entry.
const PROPERTY_LINES_ON: &str = "property-lines-on";

/// Condition key: protocol-diagnostic collection is on — drives the check mark
/// on the Advanced ▸ Collect Protocol Diagnostics entry.
const COLLECT_DIAGNOSTICS_ON: &str = "collect-diagnostics-on";

/// Condition key: the Build Tools floater (`crate::edit_tool`) is open.
const BUILD_TOOLS_OPEN: &str = "build-tools-open";

/// Condition key: the current selection can be **linked** (`crate::edit_link`).
const CAN_LINK: &str = "can-link";

/// Condition key: the current selection can be **unlinked** (`crate::edit_link`).
const CAN_UNLINK: &str = "can-unlink";

/// Condition key: the current selection has an object whose last edit can be
/// **undone** (`crate::edit_undo`).
const CAN_UNDO: &str = "can-undo";

/// Condition key: the current selection has an object whose last undone edit can
/// be **redone** (`crate::edit_undo`).
const CAN_REDO: &str = "can-redo";

/// The condition keys that hold while the matching World ▸ Environment fixed
/// environment is pinned — one per group × time (Day Cycle / Legacy / Modern ×
/// Sunrise / Midday / Sunset / Midnight), plus the shared-environment default.
/// Each drives the check mark on its entry; exactly one holds at a time.
const ENV_DAYCYCLE_SUNRISE_ACTIVE: &str = "env-daycycle-sunrise-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_DAYCYCLE_MIDDAY_ACTIVE: &str = "env-daycycle-midday-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_DAYCYCLE_SUNSET_ACTIVE: &str = "env-daycycle-sunset-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_DAYCYCLE_MIDNIGHT_ACTIVE: &str = "env-daycycle-midnight-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_LEGACY_SUNRISE_ACTIVE: &str = "env-legacy-sunrise-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_LEGACY_MIDDAY_ACTIVE: &str = "env-legacy-midday-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_LEGACY_SUNSET_ACTIVE: &str = "env-legacy-sunset-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_LEGACY_MIDNIGHT_ACTIVE: &str = "env-legacy-midnight-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_MODERN_SUNRISE_ACTIVE: &str = "env-modern-sunrise-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_MODERN_MIDDAY_ACTIVE: &str = "env-modern-midday-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_MODERN_SUNSET_ACTIVE: &str = "env-modern-sunset-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_MODERN_MIDNIGHT_ACTIVE: &str = "env-modern-midnight-active";
/// See [`ENV_DAYCYCLE_SUNRISE_ACTIVE`].
const ENV_SHARED_ACTIVE: &str = "env-shared-active";

/// The Avatar (Me) menu — the entries with a live target today.
static AVATAR_MENU: MenuDef = MenuDef {
    label: "Avatar",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Preferences\u{2026}", "toggle-preferences")
                .accel("Ctrl+P")
                .checked_when(PREFERENCES_OPEN),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Inventory", "toggle-inventory")
                .accel("Ctrl+I")
                .checked_when(INVENTORY_OPEN),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Experiences", "toggle-experiences").checked_when(EXPERIENCES_OPEN),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Quit", "quit").accel("Ctrl+Q")),
    ],
};

/// The Comm ▸ **Online Status** submenu — the presence modes
/// ([`crate::presence`]), in the reference's order and with its labels (Do Not
/// Disturb shows as *Unavailable*, the name other residents see on the tag),
/// followed by the three standing auto-reject modes ([`crate::auto_reject`]).
static ONLINE_STATUS_MENU: MenuDef = MenuDef {
    label: "Online Status",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Away", "presence-away").checked_when(PRESENCE_AWAY)),
        MenuItemDef::Command(
            MenuCommand::new("Unavailable", "presence-do-not-disturb")
                .checked_when(PRESENCE_DO_NOT_DISTURB),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Autorespond", "presence-autorespond")
                .checked_when(PRESENCE_AUTORESPOND),
        ),
        MenuItemDef::Command(
            MenuCommand::new(
                "Autorespond to non-friends",
                "presence-autorespond-non-friends",
            )
            .checked_when(PRESENCE_AUTORESPOND_NON_FRIENDS),
        ),
        MenuItemDef::Command(
            MenuCommand::new(
                "Reject teleport offers and requests",
                "reject-teleport-offers",
            )
            .checked_when(REJECT_TELEPORT_OFFERS),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Reject all group invites", "reject-group-invites")
                .checked_when(REJECT_GROUP_INVITES),
        ),
        MenuItemDef::Command(
            MenuCommand::new(
                "Reject all friendship requests",
                "reject-friendship-requests",
            )
            .checked_when(REJECT_FRIENDSHIP_REQUESTS),
        ),
    ],
};

/// The Comm menu — the reference viewer's Communicate menu. Its Conversations
/// entry opens the [`crate::conversations`] floater (the reference's
/// `Comm > Conversations…`); friends / groups and the rest are future entries.
static COMM_MENU: MenuDef = MenuDef {
    label: "Comm",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Conversations", "toggle-conversations")
                .accel("Ctrl+T")
                .checked_when(CONVERSATIONS_OPEN),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Submenu(&ONLINE_STATUS_MENU),
        MenuItemDef::Separator,
        // The reference's `Comm > Contacts / Groups / Block List`. All three
        // lists live in sub-tabs of the People pane inside the conversations
        // window, so each entry opens that window and fronts its sub-tab rather
        // than opening a floater of its own.
        MenuItemDef::Command(MenuCommand::new("Friends", "open-friends-list")),
        MenuItemDef::Command(MenuCommand::new("Groups", "open-groups-list")),
        MenuItemDef::Command(MenuCommand::new("Block List", "open-block-list")),
    ],
};

/// The World ▸ Environment ▸ **Day Cycle** submenu — the region's / parcel's own
/// EEP day cycle frozen at each of the four times (fixed sun, the region's
/// palette).
static ENV_DAYCYCLE_MENU: MenuDef = MenuDef {
    label: "Day Cycle",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Sunrise", "env-daycycle-sunrise")
                .checked_when(ENV_DAYCYCLE_SUNRISE_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Midday", "env-daycycle-midday")
                .checked_when(ENV_DAYCYCLE_MIDDAY_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sunset", "env-daycycle-sunset")
                .checked_when(ENV_DAYCYCLE_SUNSET_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Midnight", "env-daycycle-midnight")
                .checked_when(ENV_DAYCYCLE_MIDNIGHT_ACTIVE),
        ),
    ],
};

/// The World ▸ Environment ▸ **Legacy** submenu — the ported Linden `A-*`
/// WindLight presets (`reflection_probe_ambiance = 0`, the classic-mode path).
static ENV_LEGACY_MENU: MenuDef = MenuDef {
    label: "Legacy",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Sunrise", "env-legacy-sunrise")
                .checked_when(ENV_LEGACY_SUNRISE_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Midday", "env-legacy-midday").checked_when(ENV_LEGACY_MIDDAY_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sunset", "env-legacy-sunset").checked_when(ENV_LEGACY_SUNSET_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Midnight", "env-legacy-midnight")
                .checked_when(ENV_LEGACY_MIDNIGHT_ACTIVE),
        ),
    ],
};

/// The World ▸ Environment ▸ **Modern** submenu — the reference viewer's
/// `KNOWN_SKY_*` library EEP skies, fetched by UUID so they render byte-identical
/// input to Firestorm's matching presets.
static ENV_MODERN_MENU: MenuDef = MenuDef {
    label: "Modern",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Sunrise", "env-modern-sunrise")
                .checked_when(ENV_MODERN_SUNRISE_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Midday", "env-modern-midday").checked_when(ENV_MODERN_MIDDAY_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sunset", "env-modern-sunset").checked_when(ENV_MODERN_SUNSET_ACTIVE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Midnight", "env-modern-midnight")
                .checked_when(ENV_MODERN_MIDNIGHT_ACTIVE),
        ),
    ],
};

/// The World ▸ Environment submenu — three groups of the four fixed times of day
/// (**Day Cycle** the region's own EEP frozen per time, **Legacy** the Linden
/// `A-*` presets, **Modern** the fetched `KNOWN_SKY_*` EEP library skies) plus the
/// return to the region's shared environment. The three groups let a legacy sky,
/// a modern EEP sky, and the region's own be compared at the same time of day.
static ENVIRONMENT_MENU: MenuDef = MenuDef {
    label: "Environment",
    items: &[
        MenuItemDef::Submenu(&ENV_DAYCYCLE_MENU),
        MenuItemDef::Submenu(&ENV_LEGACY_MENU),
        MenuItemDef::Submenu(&ENV_MODERN_MENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Use Shared Environment", "env-shared")
                .checked_when(ENV_SHARED_ACTIVE),
        ),
    ],
};

/// The World menu — the minimap, world map, and environment today; teleport is
/// a future entry.
static WORLD_MENU: MenuDef = MenuDef {
    label: "World",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Mini-Map", "toggle-minimap").checked_when(MINIMAP_OPEN),
        ),
        // The nearby-avatar radar (viewer-avatar-radar).
        MenuItemDef::Command(MenuCommand::new("Radar", "toggle-radar").checked_when(RADAR_OPEN)),
        MenuItemDef::Command(
            MenuCommand::new("World Map", "toggle-world-map")
                .accel("Ctrl+M")
                .checked_when(WORLD_MAP_OPEN),
        ),
        // The in-world parcel property lines (viewer-parcel-borders-render),
        // colour-coded by ownership; the reference's World ▸ Property Lines.
        MenuItemDef::Command(
            MenuCommand::new("Property Lines", "toggle-property-lines")
                .checked_when(PROPERTY_LINES_ON),
        ),
        MenuItemDef::Separator,
        // The About Land floater (viewer-parcel-options-general) on the agent's
        // current parcel, and its read-only "About this location" variant.
        MenuItemDef::Command(MenuCommand::new("About Land…", "about-land")),
        MenuItemDef::Command(MenuCommand::new("Place Profile…", "place-profile")),
        // The Region / Estate floater (viewer-region-options-*) on the agent's
        // current region.
        MenuItemDef::Command(MenuCommand::new("Region / Estate…", "about-region")),
        MenuItemDef::Separator,
        // The derender / asset blacklist (viewer-derender-blacklist), where the
        // reference keeps it: World ▸ Asset Blacklist.
        MenuItemDef::Command(
            MenuCommand::new("Asset Blacklist…", "toggle-asset-blacklist")
                .checked_when(BLACKLIST_OPEN),
        ),
        // The standing per-avatar render exceptions
        // (viewer-avatar-render-settings-manager), where the reference keeps
        // them: World ▸ Avatar Render Settings.
        MenuItemDef::Command(
            MenuCommand::new("Avatar Render Settings…", "toggle-avatar-render-settings")
                .checked_when(AVATAR_RENDER_SETTINGS_OPEN),
        ),
        // Draw only friends' avatars (viewer-render-friends-only) — the
        // crowded-event performance escape hatch, at the reference's own World
        // ▸ Show Friends only.
        MenuItemDef::Command(
            MenuCommand::new("Show Friends Only", "toggle-friends-only")
                .checked_when(FRIENDS_ONLY_ON),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Submenu(&ENVIRONMENT_MENU),
    ],
};

/// The Build menu — the build tool (`crate::edit_tool`), object undo / redo
/// (`crate::edit_undo`), and prim linking / unlinking (`crate::edit_link`); the
/// grid options / selection-filter entries are future tasks.
static BUILD_MENU: MenuDef = MenuDef {
    label: "Build",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Build Tools", "toggle-build-tools")
                .accel("Ctrl+B")
                .checked_when(BUILD_TOOLS_OPEN),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Undo", crate::edit_undo::UNDO_ACTION)
                .accel("Ctrl+Z")
                .enabled_when(CAN_UNDO),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Redo", crate::edit_undo::REDO_ACTION)
                .accel("Ctrl+Y")
                .enabled_when(CAN_REDO),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Link", crate::edit_link::LINK_ACTION)
                .accel("Ctrl+L")
                .enabled_when(CAN_LINK),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Unlink", crate::edit_link::UNLINK_ACTION)
                .accel("Ctrl+Shift+L")
                .enabled_when(CAN_UNLINK),
        ),
    ],
};

/// The Content menu — the directory search and the in-viewer web browser today;
/// marketplace is a future entry.
static CONTENT_MENU: MenuDef = MenuDef {
    label: "Content",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Search…", "toggle-search")
                .accel("Ctrl+F")
                .checked_when(SEARCH_OPEN),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Web Browser", "toggle-web-browser").checked_when(WEB_BROWSER_OPEN),
        ),
    ],
};

/// The Help menu — the About window today; future help entries join it.
static HELP_MENU: MenuDef = MenuDef {
    label: "Help",
    items: &[MenuItemDef::Command(
        MenuCommand::new("About\u{2026}", "toggle-about").checked_when(ABOUT_OPEN),
    )],
};

/// The Advanced menu — the reference viewer's power-user menu, after Help as
/// in the reference's bar order. The debug-settings editor today; future
/// developer / diagnostic commands join here. The accel string is display
/// only; the live shortcut is `crate::debug_settings`'s own keyboard system.
static ADVANCED_MENU: MenuDef = MenuDef {
    label: "Advanced",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Debug settings\u{2026}", "toggle-debug-settings")
                .accel("Ctrl+Alt+Shift+S")
                .checked_when(DEBUG_SETTINGS_OPEN),
        ),
        // Protocol-diagnostic collection (`notification_host`): the session
        // records the anomalies it would otherwise silently drop, and the
        // viewer reports them to the log. Reachable from the debug-settings
        // editor too; here because it costs something to leave on.
        MenuItemDef::Command(
            MenuCommand::new("Collect Protocol Diagnostics", "toggle-collect-diagnostics")
                .checked_when(COLLECT_DIAGNOSTICS_ON),
        ),
    ],
};

/// The top menu bar, in the reference viewer's order. Exposed so menu search
/// ([`crate::menu_search`]) can walk the same tree it draws.
pub(crate) static TOP_MENU_BAR: MenuBarDef = MenuBarDef {
    menus: &[
        &AVATAR_MENU,
        &COMM_MENU,
        &WORLD_MENU,
        &BUILD_MENU,
        &CONTENT_MENU,
        &HELP_MENU,
        &ADVANCED_MENU,
    ],
};

/// A marker on the top menu bar's row, so [`update_top_menu_conditions`] writes
/// the live conditions there — every button under it inherits them by ancestry
/// ([`MenuConditions`]).
#[derive(Component)]
struct TopMenuBar;

/// The top menu bar's runtime: spawn the bar, keep its conditions current, and
/// route its picks.
pub(crate) struct TopMenuBarPlugin;

impl Plugin for TopMenuBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            spawn_top_menu_bar.after(UiScaffoldSystems::SpawnRoot),
        )
        .add_systems(
            Update,
            (update_top_menu_conditions, handle_top_menu_actions),
        );
    }
}

/// Spawn the top menu bar under the UI root.
///
/// Spanning the **full window width** and top-aligned: the menu names sit at the
/// leading corner (content-sized) and the status area ([`crate::status_bar`])
/// fills the rest of the row to the trailing edge, so the top row reads as one
/// continuous bar (the reference viewer's arrangement) rather than a
/// content-sized huddle. It reflows on a font-size / locale change.
fn spawn_top_menu_bar(mut commands: Commands, root: Res<UiRoot>, asset_server: Res<AssetServer>) {
    let bar = spawn_menu_bar(
        &mut commands,
        root.0,
        ElementCx::new(),
        &TOP_MENU_BAR,
        TOP_MENU_ELEMENT,
    );
    commands.entity(bar).insert((
        GlobalZIndex(TOP_BAR_Z),
        MenuConditions::default(),
        TopMenuBar,
        // A lone `Alt` tap opens this bar into keyboard navigation (see
        // `crate::menu`'s `menu_alt_enter`).
        PrimaryMenuBar,
    ));
    // Stretch the (otherwise content-sized) menu-bar widget across the window so
    // the status area's trailing read-outs reach the right edge. Patched here
    // rather than in `spawn_menu_bar`, which the content-sized inventory gear /
    // view menus share.
    commands.entity(bar).entry::<Node>().and_modify(|mut node| {
        node.width = Val::Percent(100.0);
    });
    // The menu-search field sits in the bar, immediately after the last menu
    // (viewer-ui-menu-search): its text drives `crate::menu`'s `MenuFilter`, so
    // opening a menu while a term is active shows only the matching entries.
    crate::menu_search::spawn_menu_search_field(&mut commands, bar);
    // The status area (viewer-ui-status-bar) fills the rest of the row after the
    // search field, its parcel-name read-out flexing to push the balance / time /
    // FPS to the trailing edge.
    crate::status_bar::spawn_status_area(&mut commands, &asset_server, bar);
}

/// Recompute the bar's live conditions each frame from the world.
///
/// Cheap — one small `Vec` and only written on a real change — and read only
/// when a menu opens ([`crate::menu`] rebuilds a popup from the conditions that
/// hold at open time), so nothing here needs to run against an open menu.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries, and this one is \
              the fan-in of every condition the bar's check marks and enable gates read: the \
              floaters, the environment, the selection and edit tool, the settings, the \
              presence modes, the panel-shown query, and the bar itself"
)]
fn update_top_menu_conditions(
    floaters: Query<(Entity, &crate::floater::Floater)>,
    environment: Option<Res<crate::environment::EnvironmentState>>,
    selection: Res<crate::world_api::SelectionSet>,
    edit_tool: Res<crate::world_api::EditToolState>,
    settings: Res<crate::settings::ViewerSettings>,
    presence: Option<Res<crate::world_api::PresenceState>>,
    panels: Query<&UiPanelShown>,
    mut bars: Query<&mut MenuConditions, With<TopMenuBar>>,
) {
    // A floater's open state, resolved by stable id — not through its module's
    // `XUi` resource, which a lazily-built floater only gains on first open.
    let open = |id: &str| {
        crate::floater::floater_panel(&floaters, id)
            .and_then(|panel| panels.get(panel).ok())
            .is_some_and(|shown| shown.0)
    };
    let preferences_open = open(crate::preferences::PREFERENCES_FLOATER_ID);
    let debug_settings_open = open(crate::debug_settings::DEBUG_SETTINGS_FLOATER_ID);
    let about_open = open(crate::about_floater::ABOUT_FLOATER_ID);
    let inventory_open = open(crate::inventory::INVENTORY_FLOATER_ID);
    let conversations_open = open(crate::conversations::CONVERSATIONS_FLOATER_ID);
    let web_browser_open = open(crate::web_floater::WEB_FLOATER_ID);
    let minimap_open = open(crate::minimap::MINIMAP_FLOATER_ID);
    let radar_open = open(crate::radar::RADAR_FLOATER_ID);
    let world_map_open = open(crate::world_map::WORLD_MAP_FLOATER_ID);
    let search_open = open(crate::search::SEARCH_FLOATER_ID);
    let build_tools_open = open(crate::edit_tool::BUILD_TOOLS_FLOATER_ID);
    let experiences_open = open(crate::experiences_floater::EXPERIENCES_FLOATER_ID);
    let blacklist_open = open(crate::asset_blacklist::BLACKLIST_FLOATER_ID);
    let render_settings_open = open(crate::avatar_render_floater::RENDER_SETTINGS_FLOATER_ID);
    let mut wanted: Vec<&'static str> = Vec::new();
    if preferences_open {
        wanted.push(PREFERENCES_OPEN);
    }
    if debug_settings_open {
        wanted.push(DEBUG_SETTINGS_OPEN);
    }
    if about_open {
        wanted.push(ABOUT_OPEN);
    }
    if inventory_open {
        wanted.push(INVENTORY_OPEN);
    }
    if conversations_open {
        wanted.push(CONVERSATIONS_OPEN);
    }
    if web_browser_open {
        wanted.push(WEB_BROWSER_OPEN);
    }
    if minimap_open {
        wanted.push(MINIMAP_OPEN);
    }
    if radar_open {
        wanted.push(RADAR_OPEN);
    }
    if world_map_open {
        wanted.push(WORLD_MAP_OPEN);
    }
    if search_open {
        wanted.push(SEARCH_OPEN);
    }
    if build_tools_open {
        wanted.push(BUILD_TOOLS_OPEN);
    }
    if experiences_open {
        wanted.push(EXPERIENCES_OPEN);
    }
    if blacklist_open {
        wanted.push(BLACKLIST_OPEN);
    }
    if render_settings_open {
        wanted.push(AVATAR_RENDER_SETTINGS_OPEN);
    }
    if settings
        .store()
        .get_bool(crate::derender::SETTING_FRIENDS_ONLY)
        .unwrap_or(false)
    {
        wanted.push(FRIENDS_ONLY_ON);
    }
    // The Comm ▸ Online Status check marks: the two session modes from the
    // presence state, the two autorespond modes from their persisted settings.
    if let Some(presence) = &presence {
        if presence.is_away() {
            wanted.push(PRESENCE_AWAY);
        }
        if presence.is_do_not_disturb() {
            wanted.push(PRESENCE_DO_NOT_DISTURB);
        }
    }
    if settings
        .store()
        .get_bool(crate::world_api::SETTING_AUTORESPOND_MODE)
        .unwrap_or(false)
    {
        wanted.push(PRESENCE_AUTORESPOND);
    }
    if settings
        .store()
        .get_bool(crate::world_api::SETTING_AUTORESPOND_NON_FRIENDS_MODE)
        .unwrap_or(false)
    {
        wanted.push(PRESENCE_AUTORESPOND_NON_FRIENDS);
    }
    // The three auto-reject check marks, likewise from their persisted flags.
    for (setting, condition) in [
        (
            crate::auto_reject::SETTING_REJECT_TELEPORT_OFFERS,
            REJECT_TELEPORT_OFFERS,
        ),
        (
            crate::auto_reject::SETTING_REJECT_ALL_GROUP_INVITES,
            REJECT_GROUP_INVITES,
        ),
        (
            crate::auto_reject::SETTING_REJECT_FRIENDSHIP_REQUESTS,
            REJECT_FRIENDSHIP_REQUESTS,
        ),
    ] {
        if settings.store().get_bool(setting).unwrap_or(false) {
            wanted.push(condition);
        }
    }
    // The World ▸ Property Lines check mark, from the in-world property-lines
    // setting (default on).
    if settings
        .store()
        .get_bool(crate::parcel_borders::SETTING_SHOW_PROPERTY_LINES)
        .unwrap_or(true)
    {
        wanted.push(PROPERTY_LINES_ON);
    }
    // The Advanced ▸ Collect Protocol Diagnostics check mark (default on).
    if settings
        .store()
        .get_bool(crate::notification_host::SETTING_COLLECT_DIAGNOSTICS)
        .unwrap_or(true)
    {
        wanted.push(COLLECT_DIAGNOSTICS_ON);
    }
    // The Build ▸ Link / Unlink enable gates, from the current selection.
    if crate::edit_link::can_link(&selection, &edit_tool) {
        wanted.push(CAN_LINK);
    }
    if crate::edit_link::can_unlink(&selection) {
        wanted.push(CAN_UNLINK);
    }
    // The Build ▸ Undo / Redo enable gates, from the current selection's
    // per-object permissions (the reference's `canUndo` / `canRedo`).
    if crate::edit_undo::can_undo(&selection, &edit_tool) {
        wanted.push(CAN_UNDO);
    }
    if crate::edit_undo::can_redo(&selection, &edit_tool) {
        wanted.push(CAN_REDO);
    }
    // The Environment submenu's check marks: exactly one of the four presets or
    // the shared default holds. The gallery has no environment resource, so the
    // submenu simply shows no check there.
    if let Some(environment) = &environment {
        wanted.push(environment_condition(environment.fixed()));
    }
    for mut conditions in &mut bars {
        if conditions.0 != wanted {
            conditions.0.clone_from(&wanted);
        }
    }
}

/// The check-mark condition key for a pinned environment selection (or the shared
/// default) — one per Day Cycle / Legacy / Modern × time of day.
const fn environment_condition(
    fixed: Option<crate::environment::FixedEnvironment>,
) -> &'static str {
    use crate::environment::FixedEnvironment::{DayCycle, Legacy, Modern};
    use crate::sky_presets::FixedSky::{Midday, Midnight, Sunrise, Sunset};
    match fixed {
        None => ENV_SHARED_ACTIVE,
        Some(DayCycle(Sunrise)) => ENV_DAYCYCLE_SUNRISE_ACTIVE,
        Some(DayCycle(Midday)) => ENV_DAYCYCLE_MIDDAY_ACTIVE,
        Some(DayCycle(Sunset)) => ENV_DAYCYCLE_SUNSET_ACTIVE,
        Some(DayCycle(Midnight)) => ENV_DAYCYCLE_MIDNIGHT_ACTIVE,
        Some(Legacy(Sunrise)) => ENV_LEGACY_SUNRISE_ACTIVE,
        Some(Legacy(Midday)) => ENV_LEGACY_MIDDAY_ACTIVE,
        Some(Legacy(Sunset)) => ENV_LEGACY_SUNSET_ACTIVE,
        Some(Legacy(Midnight)) => ENV_LEGACY_MIDNIGHT_ACTIVE,
        Some(Modern(Sunrise)) => ENV_MODERN_SUNRISE_ACTIVE,
        Some(Modern(Midday)) => ENV_MODERN_MIDDAY_ACTIVE,
        Some(Modern(Sunset)) => ENV_MODERN_SUNSET_ACTIVE,
        Some(Modern(Midnight)) => ENV_MODERN_MIDNIGHT_ACTIVE,
    }
}

/// Route the top menu bar's picks to their live targets.
///
/// Only the actions with something to do today are handled; the rest (the
/// placeholder's `noop`, and any future entry whose handler is not written yet)
/// fall through harmlessly, which is exactly what lets a future task add an
/// entry to a `static` menu above and wire it here in one place.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the action \
              stream, the by-id floater lookup, the environment state, the parcel, the two \
              open-request channels, the settings, the panel-shown query, the People sub-tab \
              request, the presence modes and their notification channel, and the \
              quit-request writer"
)]
fn handle_top_menu_actions(
    mut actions: MessageReader<UiAction>,
    floaters: Query<(Entity, &crate::floater::Floater)>,
    mut environment: Option<ResMut<crate::environment::EnvironmentState>>,
    agent_parcel: Res<sl_client_bevy::SlAgentParcel>,
    mut about_land: MessageWriter<crate::about_land::OpenAboutLand>,
    mut about_region: MessageWriter<crate::about_region::OpenAboutRegion>,
    mut settings: ResMut<crate::settings::ViewerSettings>,
    mut panels: Query<&mut UiPanelShown>,
    mut people_tabs: MessageWriter<crate::people::OpenPeopleSubTab>,
    mut presence: Option<ResMut<crate::world_api::PresenceState>>,
    mut notify: MessageWriter<crate::notifications::ShowNotification>,
    mut quit: MessageWriter<crate::session::QuitRequested>,
) {
    use crate::environment::FixedEnvironment;
    use crate::sky_presets::FixedSky;
    // The World ▸ Environment picks share one shape: pin a fixed environment (or
    // restore the shared environment) on the environment state.
    let set_fixed = |environment: &mut Option<ResMut<crate::environment::EnvironmentState>>,
                     fixed: Option<FixedEnvironment>| {
        if let Some(environment) = environment {
            environment.set_fixed(fixed);
        }
    };
    for action in actions.read() {
        if action.element != TOP_MENU_ELEMENT {
            continue;
        }
        // The Comm ▸ Online Status picks all share one handler (they toggle a
        // mode and raise its notification); it reports whether it claimed the
        // action, so the ordinary dispatch below stays untouched.
        if let Some(presence) = presence.as_deref_mut()
            && crate::presence::toggle_presence_mode(
                action.action,
                presence,
                &mut settings,
                &mut notify,
            )
        {
            continue;
        }
        // The three auto-reject picks on the same submenu — pure settings, so
        // they need no session state at all.
        if crate::auto_reject::toggle_reject_mode(action.action, &mut settings, &mut notify) {
            continue;
        }
        match action.action {
            "quit" => {
                // Route through a graceful logout, not an abrupt `AppExit`, so
                // the grid session closes cleanly (see `handle_quit_requests`).
                quit.write(crate::session::QuitRequested);
            }
            "toggle-preferences" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::preferences::PREFERENCES_FLOATER_ID,
                );
            }
            "toggle-debug-settings" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::debug_settings::DEBUG_SETTINGS_FLOATER_ID,
                );
            }
            "toggle-about" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::about_floater::ABOUT_FLOATER_ID,
                );
            }
            "toggle-inventory" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::inventory::INVENTORY_FLOATER_ID,
                );
            }
            "toggle-conversations" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::conversations::CONVERSATIONS_FLOATER_ID,
                );
            }
            "open-friends-list" | "open-groups-list" | "open-block-list" => {
                let sub_tab = match action.action {
                    "open-friends-list" => crate::people::PeopleSubTab::Friends,
                    "open-groups-list" => crate::people::PeopleSubTab::Groups,
                    _blocked => crate::people::PeopleSubTab::Blocked,
                };
                crate::floater::show_floater(
                    &floaters,
                    &mut panels,
                    crate::conversations::CONVERSATIONS_FLOATER_ID,
                );
                people_tabs.write(crate::people::OpenPeopleSubTab(sub_tab));
            }
            "toggle-web-browser" => {
                toggle_floater(&floaters, &mut panels, crate::web_floater::WEB_FLOATER_ID);
            }
            "toggle-minimap" => {
                toggle_floater(&floaters, &mut panels, crate::minimap::MINIMAP_FLOATER_ID);
            }
            "toggle-radar" => {
                toggle_floater(&floaters, &mut panels, crate::radar::RADAR_FLOATER_ID);
            }
            "toggle-world-map" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::world_map::WORLD_MAP_FLOATER_ID,
                );
            }
            "toggle-property-lines" => {
                let name = crate::parcel_borders::SETTING_SHOW_PROPERTY_LINES;
                let current = settings.store().get_bool(name).unwrap_or(true);
                settings.set(
                    sl_settings::Scope::Global,
                    name,
                    sl_settings::SettingValue::Bool(!current),
                );
            }
            "toggle-collect-diagnostics" => {
                let name = crate::notification_host::SETTING_COLLECT_DIAGNOSTICS;
                let current = settings.store().get_bool(name).unwrap_or(true);
                settings.set(
                    sl_settings::Scope::Global,
                    name,
                    sl_settings::SettingValue::Bool(!current),
                );
            }
            "toggle-search" => {
                toggle_floater(&floaters, &mut panels, crate::search::SEARCH_FLOATER_ID);
            }
            "toggle-build-tools" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::edit_tool::BUILD_TOOLS_FLOATER_ID,
                );
            }
            "toggle-friends-only" => {
                let name = crate::derender::SETTING_FRIENDS_ONLY;
                let current = settings.store().get_bool(name).unwrap_or(false);
                settings.set_account(name, sl_settings::SettingValue::Bool(!current));
                settings.save_async();
            }
            "toggle-asset-blacklist" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::asset_blacklist::BLACKLIST_FLOATER_ID,
                );
            }
            "toggle-avatar-render-settings" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::avatar_render_floater::RENDER_SETTINGS_FLOATER_ID,
                );
            }
            "toggle-experiences" => {
                toggle_floater(
                    &floaters,
                    &mut panels,
                    crate::experiences_floater::EXPERIENCES_FLOATER_ID,
                );
            }
            "about-land" | "place-profile" => {
                if let Some(current) = agent_parcel.current.as_ref() {
                    about_land.write(crate::about_land::OpenAboutLand {
                        subject: crate::about_land::AboutLandSubject::CurrentParcel(
                            current.local_id,
                        ),
                        read_only: action.action == "place-profile",
                    });
                }
            }
            "about-region" => {
                about_region.write(crate::about_region::OpenAboutRegion);
            }
            "env-daycycle-sunrise" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::DayCycle(FixedSky::Sunrise)),
                );
            }
            "env-daycycle-midday" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::DayCycle(FixedSky::Midday)),
                );
            }
            "env-daycycle-sunset" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::DayCycle(FixedSky::Sunset)),
                );
            }
            "env-daycycle-midnight" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::DayCycle(FixedSky::Midnight)),
                );
            }
            "env-legacy-sunrise" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Legacy(FixedSky::Sunrise)),
                );
            }
            "env-legacy-midday" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Legacy(FixedSky::Midday)),
                );
            }
            "env-legacy-sunset" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Legacy(FixedSky::Sunset)),
                );
            }
            "env-legacy-midnight" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Legacy(FixedSky::Midnight)),
                );
            }
            "env-modern-sunrise" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Modern(FixedSky::Sunrise)),
                );
            }
            "env-modern-midday" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Modern(FixedSky::Midday)),
                );
            }
            "env-modern-sunset" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Modern(FixedSky::Sunset)),
                );
            }
            "env-modern-midnight" => {
                set_fixed(
                    &mut environment,
                    Some(FixedEnvironment::Modern(FixedSky::Midnight)),
                );
            }
            "env-shared" => set_fixed(&mut environment, None),
            _ => {}
        }
    }
}
