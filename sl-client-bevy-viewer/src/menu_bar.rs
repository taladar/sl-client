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

use crate::conversations::ConversationsUi;
use crate::inventory::InventoryUi;
use crate::menu::{
    MenuBarDef, MenuCommand, MenuConditions, MenuDef, MenuItemDef, NEVER_CONDITION, PrimaryMenuBar,
    spawn_menu_bar,
};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems};
use crate::ui_element::{ElementCx, UiAction};

/// The `element` the top menu bar attributes its actions to — the tag
/// [`handle_top_menu_actions`] filters on, so it routes *its* menu's picks and
/// not some other widget's. Menu search ([`crate::menu_search`]) emits under the
/// same tag, so activating a search hit routes through [`handle_top_menu_actions`]
/// exactly as opening the menu and clicking the entry would.
pub(crate) const TOP_MENU_ELEMENT: &str = "top-menu-bar";

/// The z-index the bar renders at — above the floaters (so a window never covers
/// the menu bar), below an open menu's popup (`crate::menu`'s `MENU_Z_INDEX`).
const TOP_BAR_Z: i32 = 9_000;

/// The condition key that holds while the inventory window is open — drives the
/// check mark on the Avatar ▸ Inventory entry.
const INVENTORY_OPEN: &str = "inventory-open";

/// The condition key that holds while the Conversations floater is open — drives
/// the check mark on the Comm ▸ Conversations entry.
const CONVERSATIONS_OPEN: &str = "conversations-open";

/// The condition key that holds while the web browser floater is open — drives
/// the check mark on the Content ▸ Web Browser entry.
const WEB_BROWSER_OPEN: &str = "web-browser-open";

/// The placeholder shown in a menu that has no wired entries yet — a single
/// disabled line, so the menu still opens and plainly reads as unpopulated. Its
/// `enabled_when` names a condition the bar never sets, so it is always greyed.
static PLACEHOLDER_ITEMS: &[MenuItemDef] = &[MenuItemDef::Command(
    MenuCommand::new("(no entries yet)", "noop").enabled_when(NEVER_CONDITION),
)];

/// The Avatar (Me) menu — the two entries with a live target today.
static AVATAR_MENU: MenuDef = MenuDef {
    label: "Avatar",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("Inventory", "toggle-inventory")
                .accel("Ctrl+I")
                .checked_when(INVENTORY_OPEN),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Quit", "quit").accel("Ctrl+Q")),
    ],
};

/// The Comm menu — the reference viewer's Communicate menu. Its Conversations
/// entry opens the [`crate::conversations`] floater (the reference's
/// `Comm > Conversations…`); friends / groups and the rest are future entries.
static COMM_MENU: MenuDef = MenuDef {
    label: "Comm",
    items: &[MenuItemDef::Command(
        MenuCommand::new("Conversations", "toggle-conversations")
            .accel("Ctrl+T")
            .checked_when(CONVERSATIONS_OPEN),
    )],
};

/// The World menu — a name for future world entries (mini-map, world map,
/// teleport, environment).
static WORLD_MENU: MenuDef = MenuDef {
    label: "World",
    items: PLACEHOLDER_ITEMS,
};

/// The Build menu — a name for future build / edit entries.
static BUILD_MENU: MenuDef = MenuDef {
    label: "Build",
    items: PLACEHOLDER_ITEMS,
};

/// The Content menu — the in-viewer web browser today; search / marketplace
/// are future entries.
static CONTENT_MENU: MenuDef = MenuDef {
    label: "Content",
    items: &[MenuItemDef::Command(
        MenuCommand::new("Web Browser", "toggle-web-browser").checked_when(WEB_BROWSER_OPEN),
    )],
};

/// The Help menu — a name for future help / about entries.
static HELP_MENU: MenuDef = MenuDef {
    label: "Help",
    items: PLACEHOLDER_ITEMS,
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
fn update_top_menu_conditions(
    inventory: Option<Res<InventoryUi>>,
    conversations: Option<Res<ConversationsUi>>,
    web_browser: Option<Res<crate::web_floater::WebFloaterUi>>,
    panels: Query<&UiPanelShown>,
    mut bars: Query<&mut MenuConditions, With<TopMenuBar>>,
) {
    let inventory_open = inventory
        .and_then(|ui| panels.get(ui.panel()).ok().map(|shown| shown.0))
        .unwrap_or(false);
    let conversations_open = conversations
        .and_then(|ui| panels.get(ui.panel()).ok().map(|shown| shown.0))
        .unwrap_or(false);
    let web_browser_open = web_browser
        .and_then(|ui| panels.get(ui.panel()).ok().map(|shown| shown.0))
        .unwrap_or(false);
    let mut wanted: Vec<&'static str> = Vec::new();
    if inventory_open {
        wanted.push(INVENTORY_OPEN);
    }
    if conversations_open {
        wanted.push(CONVERSATIONS_OPEN);
    }
    if web_browser_open {
        wanted.push(WEB_BROWSER_OPEN);
    }
    for mut conditions in &mut bars {
        if conditions.0 != wanted {
            conditions.0.clone_from(&wanted);
        }
    }
}

/// Route the top menu bar's picks to their live targets.
///
/// Only the actions with something to do today are handled; the rest (the
/// placeholder's `noop`, and any future entry whose handler is not written yet)
/// fall through harmlessly, which is exactly what lets a future task add an
/// entry to a `static` menu above and wire it here in one place.
fn handle_top_menu_actions(
    mut actions: MessageReader<UiAction>,
    inventory: Option<Res<InventoryUi>>,
    conversations: Option<Res<ConversationsUi>>,
    web_browser: Option<Res<crate::web_floater::WebFloaterUi>>,
    mut panels: Query<&mut UiPanelShown>,
    mut exit: MessageWriter<AppExit>,
) {
    for action in actions.read() {
        if action.element != TOP_MENU_ELEMENT {
            continue;
        }
        match action.action {
            "quit" => {
                exit.write(AppExit::Success);
            }
            "toggle-inventory" => {
                if let Some(ui) = &inventory
                    && let Ok(mut shown) = panels.get_mut(ui.panel())
                {
                    shown.0 = !shown.0;
                }
            }
            "toggle-conversations" => {
                if let Some(ui) = &conversations
                    && let Ok(mut shown) = panels.get_mut(ui.panel())
                {
                    shown.0 = !shown.0;
                }
            }
            "toggle-web-browser" => {
                if let Some(ui) = &web_browser
                    && let Ok(mut shown) = panels.get_mut(ui.panel())
                {
                    shown.0 = !shown.0;
                }
            }
            _ => {}
        }
    }
}
