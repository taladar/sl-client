//! The **inventory gallery** view (`viewer-inventory-gallery`): a thumbnail
//! grid over one folder's contents — the reference viewer's
//! `LLInventoryGallery` (`panel_inventory_gallery.xml`), a single-folder
//! presentation with back / forward / up navigation, sharing the tree's
//! model and selection.
//!
//! # Same model, different projection
//!
//! The grid draws from [`crate::inventory::InventoryModel`] exactly like the
//! tree does: sub-folder tiles first, then item tiles, name order. A texture
//! or snapshot tile resolves its own asset through the shared texture
//! pipeline as its thumbnail (the wire model does not carry the reference's
//! per-item thumbnail id); every other tile shows its type glyph large.
//! Selection is the tree's `crate::inventory::InventorySelection`; a
//! right-click opens the same context menus; a double-click descends into a
//! folder or opens an item's preview.
//!
//! Reference (Firestorm, read-only): `llinventorygallery.cpp`,
//! `panel_inventory_gallery_item.xml` (130×149 tiles, 128 px thumbnail,
//! min two per row, back / forward navigation).

use crate::skin_palette::SkinPalette;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{FolderType, InventoryFolderKey, InventoryType, SlCommand, TextureKey};
use sl_viewer_ui_core::skin::ACTIVE_CLASS;

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::inventory::{
    InventoryModel, InventorySelection, RowKey, folder_icon, item_icon, query_folder_page,
};
use crate::inventory_properties::{OpenItemPreview, previewable};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::UiAction;
use crate::ui_font::UiFont;
use crate::world_api::ui_texture::{PendingUiTexture, UiTexturePlugin};

/// The gallery font size for tile names, in logical pixels.
const TILE_FONT_SIZE: f32 = 12.0;

/// The chrome font size, in logical pixels.
const CHROME_FONT_SIZE: f32 = 14.0;

/// A tile's width, in logical pixels (the reference's 130).
const TILE_WIDTH: f32 = 130.0;

/// A tile's thumbnail edge, in logical pixels (the reference's 128, sized to
/// the tile width here).
const THUMB_EDGE: f32 = 118.0;

/// The grid viewport's size, in logical pixels.
const GRID_WIDTH: f32 = 430.0;
/// The grid viewport's height, in logical pixels.
const GRID_HEIGHT: f32 = 420.0;

/// One wheel notch's scroll, in logical pixels.
const LINE_SCROLL_PIXELS: f32 = 40.0;

/// The label colour.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// A dimmer label.
const DIM_LABEL_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// A button's background / border.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);
/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The skin class on a gallery tile. Its resting backing is a scrim rather
/// than transparent (`--tile-bg`), so a thumbnail reads against whatever is
/// behind the panel; a selected tile adds [`ACTIVE_CLASS`].
const TILE_CLASS: &str = "sk-gallery-tile";

/// Two clicks on the same tile within this window are a double-click, in
/// seconds.
const DOUBLE_CLICK_SECS: f64 = 0.35;

/// The gallery's live state: the shown folder and the navigation history.
#[derive(Resource, Debug, Default)]
pub(crate) struct GalleryState {
    /// The folder whose contents the grid shows.
    current: Option<InventoryFolderKey>,
    /// The back history (previous folders, most recent last).
    back: Vec<InventoryFolderKey>,
    /// The forward history (folders backed out of, most recent last).
    forward: Vec<InventoryFolderKey>,
    /// The last click, for the double-click detection: time and tile key.
    last_click: Option<(f64, RowKey)>,
}

impl GalleryState {
    /// Navigate to a folder, pushing the current one onto the back history.
    fn navigate(&mut self, folder: InventoryFolderKey) {
        if self.current == Some(folder) {
            return;
        }
        if let Some(current) = self.current {
            self.back.push(current);
        }
        self.forward.clear();
        self.current = Some(folder);
    }

    /// Step back in the history.
    fn go_back(&mut self) {
        if let Some(previous) = self.back.pop() {
            if let Some(current) = self.current {
                self.forward.push(current);
            }
            self.current = Some(previous);
        }
    }

    /// Step forward in the history.
    fn go_forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            if let Some(current) = self.current {
                self.back.push(current);
            }
            self.current = Some(next);
        }
    }
}

/// Entity handles for the gallery floater.
#[derive(Resource)]
pub(crate) struct GalleryUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The current-folder name label.
    title_label: Entity,
    /// The scrollable tile grid.
    grid: Entity,
}

impl GalleryUi {
    /// The floater's panel root, for the gear menu's toggle.
    pub(crate) const fn panel(&self) -> Entity {
        self.panel
    }
}

/// A tile's key, for the click / context observers.
#[derive(Component, Debug, Clone, Copy)]
struct TileKey(RowKey);

/// The plugin owning the gallery view.
#[derive(Debug)]
pub struct InventoryGalleryPlugin;

impl Plugin for InventoryGalleryPlugin {
    /// Register the state and systems and spawn the (hidden) floater.
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<UiTexturePlugin>() {
            app.add_plugins(UiTexturePlugin);
        }
        app.init_resource::<GalleryState>()
            .add_systems(
                Startup,
                spawn_gallery_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (route_gallery_actions, rebuild_gallery, scroll_gallery_grid).chain(),
            );
    }
}

/// The inventory gallery floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn inventory_gallery_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "inventory-gallery",
        title: "Inventory Gallery".to_owned(),
        position: Vec2::new(400.0, 70.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: true,
            closable: true,
            dockable: true,
        },
    }
}

/// Spawn the gallery floater: the navigation row and the scrollable grid.
fn spawn_gallery_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, inventory_gallery_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("inventory-gallery-title"));
    let content = handle.content;

    // Navigation: back / forward / up + the folder name.
    let nav = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let back = spawn_nav_button(&mut commands, nav, "\u{25c0}", 1);
    commands.entity(back).observe(
        |press: On<Pointer<Press>>, mut state: ResMut<GalleryState>| {
            if press.button == PointerButton::Primary {
                state.go_back();
            }
        },
    );
    let forward = spawn_nav_button(&mut commands, nav, "\u{25b6}", 2);
    commands.entity(forward).observe(
        |press: On<Pointer<Press>>, mut state: ResMut<GalleryState>| {
            if press.button == PointerButton::Primary {
                state.go_forward();
            }
        },
    );
    let up = spawn_nav_button(&mut commands, nav, "\u{2b06}", 3);
    commands.entity(up).observe(
        |press: On<Pointer<Press>>, model: Res<InventoryModel>, mut state: ResMut<GalleryState>| {
            if press.button != PointerButton::Primary {
                return;
            }
            if let Some(parent) = state
                .current
                .and_then(|current| model.folder_info(current))
                .and_then(|info| info.parent_id)
            {
                state.navigate(parent);
            }
        },
    );
    let title_label = commands
        .spawn((
            Text::new(""),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(LABEL_COLOR),
            ChildOf(nav),
        ))
        .id();

    // The scrollable tile grid: a wrapping row inside a clipped, scrolled
    // viewport (the shared UI-gallery pattern — `bevy_ui` clips, the wheel
    // system moves the offset).
    let grid = commands
        .spawn((
            Node {
                width: Val::Px(GRID_WIDTH),
                height: Val::Px(GRID_HEIGHT),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(6.0),
                overflow: Overflow::scroll(),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.20)),
            Pickable::default(),
            Name::new("inventory-gallery-grid"),
            ChildOf(content),
        ))
        .id();

    commands.insert_resource(GalleryUi {
        panel: handle.root,
        title_label,
        grid,
    });
}

/// Spawn one square navigation button with a glyph label.
fn spawn_nav_button(
    commands: &mut Commands,
    parent: Entity,
    glyph: &'static str,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("inventory-gallery-nav:{glyph}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(glyph),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

/// Route the gear menu's Gallery View toggle: open on the selected folder
/// (or the agent root) or close.
fn route_gallery_actions(
    mut picks: MessageReader<UiAction>,
    ui: Option<Res<GalleryUi>>,
    model: Res<InventoryModel>,
    selection: Res<InventorySelection>,
    mut state: ResMut<GalleryState>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.element != "inventory-gear" || pick.action != "gallery-view" {
            continue;
        }
        let Ok(mut shown) = panels.get_mut(ui.panel) else {
            continue;
        };
        if shown.0 {
            shown.0 = false;
            continue;
        }
        // Opening: land on the selected folder (an item's parent), else the
        // current folder from last time, else the agent root.
        let target = selection
            .single()
            .and_then(|key| match key {
                RowKey::Folder(folder) => Some(folder),
                RowKey::Item(item) => model.find_item(item).map(|info| info.folder_id),
            })
            .or(state.current)
            .or_else(|| model.agent_root());
        if let Some(target) = target {
            state.navigate(target);
            query_folder_page(target, &mut commands);
        }
        shown.0 = true;
    }
}

/// What the grid is rebuilt from, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the gallery's own state, the
/// inventory model, the selection the tiles highlight, and the window's handles
/// plus its show / hide switch.
#[derive(bevy::ecs::system::SystemParam)]
struct GallerySources<'w, 's> {
    /// The folder the gallery is showing.
    state: Res<'w, GalleryState>,
    /// The inventory model the tiles are folded out of.
    model: Res<'w, InventoryModel>,
    /// The selection the tiles highlight.
    selection: Res<'w, InventorySelection>,
    /// The window's handles, absent before it is built.
    ui: Option<Res<'w, GalleryUi>>,
    /// Whether the window is shown; a hidden gallery rebuilds nothing.
    panels: Query<'w, 's, &'static UiPanelShown>,
}

/// Rebuild the grid whenever the shown folder, the model or the selection
/// changed while the gallery is open.
fn rebuild_gallery(
    sources: GallerySources,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let GallerySources {
        state,
        model,
        selection,
        ui,
        panels,
    } = sources;
    let Some(ui) = ui else {
        return;
    };
    if !state.is_changed() && !model.is_changed() && !selection.is_changed() {
        return;
    }
    let open = panels.get(ui.panel).is_ok_and(|shown| shown.0);
    if !open {
        return;
    }
    let Some(current) = state.current else {
        return;
    };
    // The folder name label.
    if let Ok(mut text) = texts.get_mut(ui.title_label) {
        let name = model
            .folder_info(current)
            .map_or_else(|| "(folder)".to_owned(), |info| info.name.clone());
        if text.0 != name {
            text.0 = name;
        }
    }
    // Tear the old tiles down.
    if let Ok(existing) = children.get(ui.grid) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
    // Sub-folder tiles first, then items — name order from the model.
    for &child in model.child_folders_of(current) {
        let info = model.folder_info(child);
        let name = info.map_or_else(|| "(folder)".to_owned(), |folder| folder.name.clone());
        let folder_type = info.map_or(FolderType::None, |folder| folder.folder_type);
        spawn_tile(
            &mut commands,
            ui.grid,
            RowKey::Folder(child),
            &name,
            folder_icon(folder_type, false),
            selection.contains(RowKey::Folder(child)),
            None,
        );
    }
    for item in model.loaded_items_of(current) {
        let thumbnail = matches!(
            item.inv_type,
            InventoryType::Texture | InventoryType::Snapshot
        )
        .then(|| TextureKey::from(item.asset_id));
        spawn_tile(
            &mut commands,
            ui.grid,
            RowKey::Item(item.item_id),
            &item.name,
            item_icon(item.inv_type),
            selection.contains(RowKey::Item(item.item_id)),
            thumbnail,
        );
    }
    // An unfetched folder fetches on arrival; harmless if already held.
    query_folder_page(current, &mut sl_commands);
}

/// Spawn one gallery tile: a thumbnail area (type glyph, or the texture once
/// decoded) over the wrapped name.
fn spawn_tile(
    commands: &mut Commands,
    grid: Entity,
    key: RowKey,
    name: &str,
    icon: &'static str,
    selected: bool,
    thumbnail: Option<TextureKey>,
) {
    let tile = commands
        .spawn((
            Button,
            Node {
                width: Val::Px(TILE_WIDTH),
                padding: UiRect::all(Val::Px(4.0)),
                ..column(Val::Px(3.0))
            },
            ClassList::new_with_classes(
                core::iter::once(TILE_CLASS).chain(selected.then_some(ACTIVE_CLASS)),
            ),
            Pickable::default(),
            TileKey(key),
            Name::new("inventory-gallery-tile"),
            ChildOf(grid),
        ))
        .observe(on_tile_press)
        .observe(on_tile_context)
        .id();
    let thumb = commands
        .spawn((
            Node {
                width: Val::Px(THUMB_EDGE),
                height: Val::Px(THUMB_EDGE),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            Pickable::IGNORE,
            ChildOf(tile),
        ))
        .with_child((
            Text::new(icon),
            UiFont::Sans.at(44.0),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    if let Some(texture) = thumbnail {
        commands
            .entity(thumb)
            .insert(PendingUiTexture::over_placeholder(texture));
    }
    commands.spawn((
        Text::new(name.to_owned()),
        UiFont::Sans.at(TILE_FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(tile),
    ));
}

/// A tile was clicked: select it; a double-click descends into a folder or
/// opens an item's preview.
fn on_tile_press(
    press: On<Pointer<Press>>,
    tiles: Query<&TileKey>,
    model: Res<InventoryModel>,
    time: Res<Time>,
    mut state: ResMut<GalleryState>,
    mut selection: ResMut<InventorySelection>,
    mut previews: MessageWriter<OpenItemPreview>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(tile) = tiles.get(press.entity) else {
        return;
    };
    let key = tile.0;
    let now = time.elapsed_secs_f64();
    let double = state
        .last_click
        .is_some_and(|(at, last)| last == key && now - at <= DOUBLE_CLICK_SECS);
    state.last_click = Some((now, key));
    selection.select_single(key, 0);
    if !double {
        return;
    }
    match key {
        RowKey::Folder(folder) => state.navigate(folder),
        RowKey::Item(item) => {
            if let Some(info) = model.find_item(item).cloned()
                && previewable(info.inv_type)
            {
                previews.write(OpenItemPreview { item: info });
            }
        }
    }
}

/// A right-click on a tile: select it and open the same context menu the
/// tree rows use, targeting the tile's folder / item.
fn on_tile_context(
    mut press: On<Pointer<Press>>,
    tiles: Query<&TileKey>,
    facts: crate::inventory_actions::InventoryMenuFacts,
    mut pick: crate::inventory_actions::InventoryMenuPick,
    mut menus: MessageWriter<crate::menu::OpenContextMenu>,
) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Ok(tile) = tiles.get(press.entity) else {
        return;
    };
    press.propagate(false);
    if !pick.selection.contains(tile.0) {
        pick.selection.select_single(tile.0, 0);
    }
    let _opened = crate::inventory_actions::open_inventory_context_menu(
        &[tile.0],
        press.pointer_location.position,
        &facts,
        // The gallery is a flat single view with no Worn / Recent membership
        // tabs, so it never offers the "Show in Main view" jump.
        false,
        &mut pick,
        &mut menus,
    );
}

/// Scroll the grid with the wheel while the pointer is over it.
fn scroll_gallery_grid(
    wheel: Res<AccumulatedMouseScroll>,
    ui: Option<Res<GalleryUi>>,
    hover: Res<HoverMap>,
    children: Query<&ChildOf>,
    mut positions: Query<&mut ScrollPosition>,
) {
    let Some(ui) = ui else {
        return;
    };
    if wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    // Only while the pointer is over the grid (or a tile in it).
    let over = hover.values().flat_map(|hits| hits.keys()).any(|hovered| {
        let mut node = *hovered;
        loop {
            if node == ui.grid {
                return true;
            }
            match children.get(node) {
                Ok(parent) => node = parent.parent(),
                Err(_root) => return false,
            }
        }
    });
    if !over {
        return;
    }
    let delta = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * LINE_SCROLL_PIXELS,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    if let Ok(mut position) = positions.get_mut(ui.grid) {
        position.0.y = (position.0.y - delta).max(0.0);
    }
}
