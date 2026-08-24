//! The **texture-picker floater + swatch widget** (`viewer-ui-texture-picker`):
//! a reusable texture swatch any panel can host, and a shared picker floater it
//! opens — the reference's `LLTextureCtrl` + `LLFloaterTexturePicker`.
//!
//! # Model
//!
//! - [`spawn_texture_swatch`] drops a bordered button showing the current
//!   texture's thumbnail ([`TextureSwatchValue`]); clicking it emits
//!   [`OpenTexturePicker`] tagged with the swatch entity as the **requester**. A
//!   consumer keeps the swatch's value current (this module paints the
//!   thumbnail) and reads [`TexturePicked`] filtered to its own swatch.
//! - The floater is the reference's shape: a **lazy inventory folder tree** on
//!   the left (folders fetch their contents only when opened, so it scales to any
//!   inventory size — never a bulk fetch) filtered to texture / snapshot items,
//!   a **single preview pane** of the selected texture, the **None / Blank /
//!   Default** quick choices, and **OK / Cancel**. A name **search** collapses
//!   the tree to a flat match list. The tree keeps its **own** expansion state,
//!   so browsing here does not disturb the main inventory floater.
//! - The picker has a [`PickerKind`] (the reference's `LLTextureCtrl`
//!   `EPickInventoryType`): opened in **material** mode (via a
//!   [`spawn_material_swatch`] swatch) it browses GLTF render-material items
//!   instead of textures, retitles to *Pick: Material*, hides the texture-only
//!   Blank / Default quick choices, and returns the chosen material id in the
//!   same [`TexturePicked`] reply. In material mode the preview pane previews the
//!   *selected* material on a lit sphere ([`crate::material_preview`],
//!   `viewer-material-swatch-sphere-preview`) via a [`MaterialPreview`] component,
//!   the same way it previews a texture.
//! - Selecting a texture emits a **non-final** [`TexturePicked`] so the consumer
//!   can live-preview it on the object; **OK** emits the final choice and
//!   **Cancel** emits the original (revert), mirroring the colour picker.
//!
//! Reference (Firestorm, read-only): `llfloatertexturepicker.cpp`,
//! `lltexturectrl.cpp`, `llinventorypanel.cpp`.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::HoverMap;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Button;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    AssetKey, InventoryFolderKey, InventoryType, TextureKey, Uuid, to_bevy_image,
};
use std::hash::{Hash, Hasher as _};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::inventory::{InventoryModel, item_icon, query_folder_page};
use crate::material_preview::MaterialPreview;
use crate::textures::TextureManager;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::{OpenTexturePicker, PickerKind, TexturePicked};
use sl_client_bevy::SlCommand;

/// The blank / white texture (`IMG_WHITE`) the **Blank** quick choice picks.
const IMG_BLANK: Uuid = Uuid::from_u128(0x5748_decc_f629_461c_9a36_a35a_221f_e21f);

/// The default object texture (`IMG_DEFAULT`, plywood) the **Default** quick
/// choice picks.
const IMG_DEFAULT: Uuid = Uuid::from_u128(0x8955_6747_24cb_43ed_920b_47ca_ed15_465f);

/// A tree row's height, in logical pixels.
const ROW_HEIGHT: f32 = 20.0;

/// The indent added per tree depth, in logical pixels.
const INDENT_PER_DEPTH: f32 = 14.0;

/// The tree viewport width / height.
const TREE_WIDTH: f32 = 300.0;

/// The tree viewport height.
const TREE_HEIGHT: f32 = 280.0;

/// The preview swatch side.
const PREVIEW_SIZE: f32 = 128.0;

/// The reusable swatch's side.
const SWATCH_SIZE: f32 = 40.0;

/// The picker font size.
const PICKER_FONT: f32 = 13.0;

/// Pixels scrolled per wheel line.
const LINE_SCROLL_PIXELS: f32 = 40.0;

/// The most tree rows rendered at once; the search filter narrows past this, and
/// the count read-out shows when it is capping.
const MAX_ROWS: usize = 400;

/// A bordered control's border colour.
const CONTROL_BORDER: Color = Color::srgba(0.4, 0.4, 0.45, 1.0);

/// A [disabled](bevy::ui::InteractionDisabled) swatch's border — dimmed so a
/// swatch the consumer cannot change reads as disabled.
const DISABLED_BORDER: Color = Color::srgba(0.28, 0.28, 0.32, 1.0);

/// A tile / swatch's empty fill.
const EMPTY_FILL: Color = Color::srgba(0.1, 0.1, 0.12, 1.0);

/// A selected row's background.
const SELECTED_FILL: Color = Color::srgba(0.2, 0.3, 0.45, 1.0);

/// A row's background when the pointer is over it.
const ROW_HOVER: Color = Color::srgba(0.22, 0.24, 0.3, 1.0);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgba(0.18, 0.18, 0.2, 1.0);

/// The text colour.
const TEXT_COLOR: Color = Color::srgb(0.9, 0.92, 0.96);

/// A folder row's label colour.
const FOLDER_COLOR: Color = Color::srgb(0.82, 0.86, 0.95);

/// The skin class for value text.
const VALUE_CLASS: &str = "sk-build-value";

/// A reusable texture swatch's current value; this module paints the swatch's
/// thumbnail from it, and a consumer reads / writes it.
#[derive(Component, Debug, Clone, Copy)]
pub struct TextureSwatchValue(pub TextureKey);

/// Marks a swatch as a **material** picker and carries its current material
/// asset id. A swatch with this component opens the picker in
/// [`PickerKind::Material`] seeded with the id (rather than in texture mode from
/// its [`TextureSwatchValue`], which for a material swatch paints the material's
/// base-colour stand-in thumbnail).
#[derive(Component, Debug, Clone, Copy)]
pub struct MaterialSwatchValue(pub Uuid);

/// Spawn a texture swatch under `parent`: a bordered button showing `initial`'s
/// thumbnail that opens the picker on click. The returned entity is the
/// **requester** a [`TexturePicked`] reply is matched by.
pub fn spawn_texture_swatch(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    tab_index: i32,
    initial: TextureKey,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                width: Val::Px(SWATCH_SIZE),
                height: Val::Px(SWATCH_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(EMPTY_FILL),
            TextureSwatchValue(initial),
            Pickable::default(),
            Name::new(format!("{element}:texture-swatch")),
            ChildOf(parent),
        ))
        .observe(open_picker_from_swatch)
        .id()
}

/// Spawn a **material** swatch under `parent`: visually identical to a
/// [`spawn_texture_swatch`] swatch (it paints a texture thumbnail — a material's
/// base-colour stand-in), but clicking it opens the picker in
/// [`PickerKind::Material`] seeded with `initial_material`. Carries both a
/// [`TextureSwatchValue`] (the thumbnail the consumer keeps current) and a
/// [`MaterialSwatchValue`] (the material asset id the pick opens on / returns).
pub fn spawn_material_swatch(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    tab_index: i32,
    initial_texture: TextureKey,
    initial_material: Uuid,
) -> Entity {
    let swatch = spawn_texture_swatch(commands, parent, element, tab_index, initial_texture);
    commands
        .entity(swatch)
        .insert(MaterialSwatchValue(initial_material));
    swatch
}

/// Request the picker for the clicked swatch — in material mode (seeded with the
/// current material id) when the swatch carries a [`MaterialSwatchValue`], else
/// in texture mode from its [`TextureSwatchValue`].
fn open_picker_from_swatch(
    press: On<Pointer<Press>>,
    swatches: Query<(&TextureSwatchValue, Option<&MaterialSwatchValue>)>,
    disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    mut opens: MessageWriter<OpenTexturePicker>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    // A disabled swatch does not open the picker (scroll past it still works).
    if disabled.contains(press.entity) {
        return;
    }
    if let Ok((texture, material)) = swatches.get(press.entity) {
        let (kind, current) = match material {
            Some(material) => (PickerKind::Material, TextureKey::from(material.0)),
            None => (PickerKind::Texture, texture.0),
        };
        opens.write(OpenTexturePicker {
            requester: press.entity,
            current,
            kind,
        });
    }
}

/// The picker's live state while open.
#[derive(Resource, Debug)]
struct TexturePickerState {
    /// The widget that opened it.
    requester: Option<Entity>,
    /// What this open browses (textures or materials).
    kind: PickerKind,
    /// The texture (or material id) it opened on (for Cancel's revert).
    original: TextureKey,
    /// The currently-selected texture (or material id).
    selected: TextureKey,
    /// The active search filter (lower-cased); when non-empty the tree collapses
    /// to a flat match list.
    filter: String,
    /// The folders expanded **in this picker** (kept separate from the main
    /// inventory floater's expansion).
    expanded: HashSet<InventoryFolderKey>,
    /// Folders this picker has already asked the session to fetch.
    requested: HashSet<InventoryFolderKey>,
    /// Set when the tree needs rebuilding (open / expand / filter / inventory
    /// change).
    dirty: bool,
    /// A hash of the last-rendered rows (+ selection), so a rebuild that would
    /// produce the identical tree despawns nothing — the lazy fetch marks the
    /// tree dirty every frame while it lands, and respawning the rows each frame
    /// would leave a just-clicked row un-laid-out (zero size) so the click falls
    /// through to the world and deselects.
    last_rows_sig: u64,
}

impl Default for TexturePickerState {
    /// [`TextureKey`] has no `Default`; start on the null texture.
    fn default() -> Self {
        Self {
            requester: None,
            kind: PickerKind::Texture,
            original: TextureKey::from(Uuid::nil()),
            selected: TextureKey::from(Uuid::nil()),
            filter: String::new(),
            expanded: HashSet::new(),
            requested: HashSet::new(),
            dirty: false,
            last_rows_sig: 0,
        }
    }
}

/// The picker floater's entities.
#[derive(Resource, Debug)]
struct TexturePickerUi {
    /// The floater root.
    panel: Entity,
    /// The floater title text (retitled per [`PickerKind`] on open).
    title_text: Entity,
    /// The search text field.
    search: Entity,
    /// The scrolling tree container.
    tree: Entity,
    /// The selected-texture preview.
    preview: Entity,
    /// The shown / total count read-out.
    count: Entity,
    /// The **Blank** quick choice (a texture UUID; hidden in material mode).
    blank_button: Entity,
    /// The **Default** quick choice (a texture UUID; hidden in material mode).
    default_button: Entity,
}

/// Thumbnail nodes (the preview / swatches) awaiting their texture decode.
#[derive(Resource, Debug, Default)]
struct PendingTexturePreviews {
    /// The nodes waiting on each texture.
    waiting: HashMap<TextureKey, Vec<Entity>>,
}

/// A tree row for a folder (click toggles expansion + lazy-fetches).
#[derive(Component, Debug, Clone, Copy)]
struct TreeFolderRow(InventoryFolderKey);

/// A tree row for a texture item (click selects it).
#[derive(Component, Debug, Clone, Copy)]
struct TreeItemRow(TextureKey);

/// A quick-choice / action button.
#[derive(Component, Debug, Clone, Copy)]
enum PickerButton {
    /// The transparent "None" choice.
    None,
    /// The blank/white texture.
    Blank,
    /// The default (plywood) texture.
    Default,
    /// Accept the selected texture.
    Ok,
    /// Discard and close.
    Cancel,
}

/// One flattened tree row, produced by [`build_tree_rows`]. Hashed (excluding
/// the transient selection highlight) to detect when a rebuild would change the
/// tree's **structure** and so needs to respawn rows.
#[derive(Debug, Clone, Hash)]
enum TreeRow {
    /// A folder at `depth`, with its key, expanded flag, and display name.
    Folder {
        /// The folder key.
        key: InventoryFolderKey,
        /// The nesting depth.
        depth: usize,
        /// Whether this picker has the folder expanded.
        expanded: bool,
        /// The display name.
        name: String,
    },
    /// A texture item at `depth`, with its texture key and display name.
    Item {
        /// The texture asset key.
        key: TextureKey,
        /// The nesting depth.
        depth: usize,
        /// The display name.
        name: String,
    },
}

/// The plugin wiring the texture picker into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TexturePickerPlugin;

impl Plugin for TexturePickerPlugin {
    /// Register the messages, state, floater, and systems.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenTexturePicker>()
            .add_message::<TexturePicked>()
            .init_resource::<TexturePickerState>()
            .init_resource::<PendingTexturePreviews>()
            .add_systems(
                Startup,
                spawn_texture_picker_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    handle_open_texture_picker,
                    revert_on_close,
                    refresh_tree_on_inventory,
                    watch_search_filter,
                    rebuild_tree,
                    paint_tree_selection,
                    apply_texture_swatch_thumbnail,
                    request_preview_texture,
                    sync_material_preview_pane,
                    resolve_texture_previews,
                    scroll_tree,
                )
                    .chain(),
            )
            .add_systems(Update, reflect_texture_swatch_disabled);
    }
}

/// Dim a texture swatch's border while it is
/// [disabled](bevy::ui::InteractionDisabled), restoring it when enabled.
fn reflect_texture_swatch_disabled(
    mut swatches: Query<
        (&mut BorderColor, Has<bevy::ui::InteractionDisabled>),
        With<TextureSwatchValue>,
    >,
) {
    for (mut border, disabled) in &mut swatches {
        let wanted = BorderColor::all(if disabled {
            DISABLED_BORDER
        } else {
            CONTROL_BORDER
        });
        if *border != wanted {
            *border = wanted;
        }
    }
}

/// Build the shared texture-picker floater (hidden until opened).
fn spawn_texture_picker_floater(mut commands: Commands, root: Option<Res<UiRoot>>) {
    let Some(root) = root.map(|root| root.0) else {
        return;
    };
    let handle = spawn_floater(
        &mut commands,
        root,
        FloaterSpec {
            id: "texture-picker",
            title: String::from("Pick: Texture"),
            // Clear of the Build Tools floater (which spans the upper-left).
            position: Vec2::new(520.0, 90.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    // Subject-bound: it opens on whatever swatch requested it, disconnected from
    // saved app state, so it is exempt from floater persistence.
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands
        .entity(handle.title_text)
        .insert(Translated::new("texture-picker-title"));
    let content = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                ..column(Val::Px(8.0))
            },
            ChildOf(handle.content),
        ))
        .id();

    // Search row.
    let search_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("texture-picker-search"),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        ChildOf(search_row),
    ));
    let search = spawn_text_input(
        &mut commands,
        search_row,
        &TextInputSpec {
            font_size: PICKER_FONT,
            width_glyphs: 18.0,
            tab_index: 1,
            ..TextInputSpec::new("texture-picker-search", TextInputKind::Line)
        },
    );
    let count = commands
        .spawn((
            Text::new("0"),
            UiFont::Sans.at(PICKER_FONT),
            TextColor(TEXT_COLOR),
            ClassList::new_with_classes([VALUE_CLASS]),
            Name::new("texture-picker-count"),
            ChildOf(search_row),
        ))
        .id();

    // Tree + preview row.
    let middle = commands
        .spawn((
            Node {
                align_items: AlignItems::Start,
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let tree = commands
        .spawn((
            Node {
                width: Val::Px(TREE_WIDTH),
                height: Val::Px(TREE_HEIGHT),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..Default::default()
            },
            ScrollPosition::default(),
            BackgroundColor(EMPTY_FILL),
            Pickable::default(),
            Name::new("texture-picker-tree"),
            ChildOf(middle),
        ))
        .id();
    let preview = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_SIZE),
                height: Val::Px(PREVIEW_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(EMPTY_FILL),
            Name::new("texture-picker-preview"),
            ChildOf(middle),
        ))
        .id();

    // Quick-choice row (None / Blank / Default).
    let quick = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    spawn_picker_button(
        &mut commands,
        quick,
        PickerButton::None,
        "texture-picker-none",
    );
    let blank_button = spawn_picker_button(
        &mut commands,
        quick,
        PickerButton::Blank,
        "texture-picker-blank",
    );
    let default_button = spawn_picker_button(
        &mut commands,
        quick,
        PickerButton::Default,
        "texture-picker-default",
    );

    // OK / Cancel row.
    let buttons = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    spawn_picker_button(
        &mut commands,
        buttons,
        PickerButton::Ok,
        "texture-picker-ok",
    );
    spawn_picker_button(
        &mut commands,
        buttons,
        PickerButton::Cancel,
        "texture-picker-cancel",
    );

    commands.insert_resource(TexturePickerUi {
        panel: handle.root,
        title_text: handle.title_text,
        search,
        tree,
        preview,
        count,
        blank_button,
        default_button,
    });
}

/// Spawn a picker button, returning its entity.
fn spawn_picker_button(
    commands: &mut Commands,
    parent: Entity,
    which: PickerButton,
    label_key: &'static str,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            which,
            Pickable::default(),
            Name::new(format!("texture-picker-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_picker_button)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(TEXT_COLOR),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Handle an [`OpenTexturePicker`]: seed the state and show the floater, marking
/// the tree for a rebuild, and adapt the floater to the requested [`PickerKind`]
/// — retitle it, hide the texture-only quick choices in material mode, and clear
/// the preview pane (a material is not a texture the preview can decode).
///
/// It deliberately does **not** bulk-fetch the inventory: a real Second Life
/// inventory is hundreds of thousands of items. The tree fetches a folder's
/// contents only when it is opened ([`toggle_folder`]).
fn handle_open_texture_picker(
    mut opens: MessageReader<OpenTexturePicker>,
    ui: Option<Res<TexturePickerUi>>,
    mut state: ResMut<TexturePickerState>,
    mut panels: Query<&mut UiPanelShown>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(open) = opens.read().last() else {
        return;
    };
    state.requester = Some(open.requester);
    state.kind = open.kind;
    state.original = open.current;
    state.selected = open.current;
    state.dirty = true;
    // Retitle for the active kind.
    let title_key = match open.kind {
        PickerKind::Texture => "texture-picker-title",
        PickerKind::Material => "texture-picker-title-material",
    };
    if let Ok(mut title) = commands.get_entity(ui.title_text) {
        title.insert(Translated::new(title_key));
    }
    // Blank / Default are texture UUIDs — meaningless as a material, so hidden in
    // material mode (None still clears).
    let quick_display = match open.kind {
        PickerKind::Texture => Display::Flex,
        PickerKind::Material => Display::None,
    };
    for button in [ui.blank_button, ui.default_button] {
        if let Ok(mut node) = nodes.get_mut(button) {
            node.display = quick_display;
        }
    }
    // Clear whatever the pane last showed — a texture thumbnail *or* a material
    // sphere left from a previous open of the other kind — so it never flashes a
    // stale preview. The correct preview then repaints for this open: in material
    // mode [`sync_material_preview_pane`] binds the pane's [`MaterialPreview`]
    // sphere; in texture mode [`request_preview_texture`] loads the opened-on
    // texture's thumbnail (a nil / None swatch just stays empty). Without this a
    // texture-mode open reopened after a material-mode open showed the leftover
    // sphere until (or unless) a thumbnail decoded over it.
    if let Ok(mut preview) = commands.get_entity(ui.preview) {
        preview.remove::<ImageNode>();
        preview.insert(BackgroundColor(EMPTY_FILL));
    }
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

/// If the floater was closed by its **X** (title-bar close) rather than OK /
/// Cancel — which leaves the requester set and the live preview showing an
/// uncommitted texture — revert the preview to the opened-on texture and clear
/// the requester, so closing never leaves the object wrongly textured.
fn revert_on_close(
    ui: Option<Res<TexturePickerUi>>,
    panels: Query<&UiPanelShown>,
    mut state: ResMut<TexturePickerState>,
    mut picked: MessageWriter<TexturePicked>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(requester) = state.requester else {
        return;
    };
    let hidden = panels.get(ui.panel).is_ok_and(|shown| !shown.0);
    if hidden {
        picked.write(TexturePicked {
            requester,
            texture: state.original,
            final_pick: false,
        });
        state.requester = None;
    }
}

/// While the picker is open, rebuild the tree as inventory folders arrive (a
/// lazy fetch lands over several frames).
fn refresh_tree_on_inventory(
    inventory: Res<InventoryModel>,
    mut state: ResMut<TexturePickerState>,
) {
    if state.requester.is_some() && inventory.is_changed() {
        state.dirty = true;
    }
}

/// Refilter the tree when the search field's text changes.
fn watch_search_filter(
    ui: Option<Res<TexturePickerUi>>,
    editors: Query<&EditableText>,
    mut state: ResMut<TexturePickerState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(editor) = editors.get(ui.search) else {
        return;
    };
    let want = editor.value().to_string().to_lowercase();
    if want != state.filter {
        state.filter = want;
        state.dirty = true;
    }
}

/// Flatten the inventory into the picker's tree rows: the folder hierarchy
/// (expanded folders only) with texture / snapshot items, or — when a search
/// filter is active — a flat list of matching loaded textures.
fn build_tree_rows(inventory: &InventoryModel, state: &TexturePickerState) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    if state.filter.is_empty() {
        for &root in inventory.roots() {
            emit_folder(inventory, state, root, 0, &mut rows);
        }
    } else {
        let mut seen: HashMap<TextureKey, ()> = HashMap::new();
        let mut items: Vec<(String, TextureKey)> = inventory
            .all_loaded_items()
            .filter(|item| item_matches(item.inv_type, state.kind))
            .filter(|item| item.name.to_lowercase().contains(&state.filter))
            .filter_map(|item| {
                let key = TextureKey::from(item.asset_id);
                seen.insert(key, ())
                    .is_none()
                    .then(|| (item.name.clone(), key))
            })
            .collect();
        items.sort_by_key(|(name, _key)| name.to_lowercase());
        for (name, key) in items {
            rows.push(TreeRow::Item {
                key,
                depth: 0,
                name,
            });
        }
    }
    rows
}

/// Emit a folder and (when expanded) its child folders and texture items.
fn emit_folder(
    inventory: &InventoryModel,
    state: &TexturePickerState,
    folder: InventoryFolderKey,
    depth: usize,
    rows: &mut Vec<TreeRow>,
) {
    let name = inventory
        .folder_info(folder)
        .map_or_else(|| String::from("(folder)"), |info| info.name.clone());
    let expanded = state.expanded.contains(&folder);
    rows.push(TreeRow::Folder {
        key: folder,
        depth,
        expanded,
        name,
    });
    if !expanded {
        return;
    }
    // Child folders, by name.
    let mut children: Vec<InventoryFolderKey> = inventory.child_folders_of(folder).to_vec();
    children.sort_by_key(|child| {
        inventory
            .folder_info(*child)
            .map(|info| info.name.to_lowercase())
            .unwrap_or_default()
    });
    for child in children {
        emit_folder(inventory, state, child, depth.saturating_add(1), rows);
    }
    // Matching items (textures or materials, per the picker kind), by name.
    let mut items: Vec<(String, TextureKey)> = inventory
        .loaded_items_of(folder)
        .iter()
        .filter(|item| item_matches(item.inv_type, state.kind))
        .map(|item| (item.name.clone(), TextureKey::from(item.asset_id)))
        .collect();
    items.sort_by_key(|(name, _key)| name.to_lowercase());
    for (name, key) in items {
        rows.push(TreeRow::Item {
            key,
            depth: depth.saturating_add(1),
            name,
        });
    }
}

/// Whether an inventory type is one the picker lists for the active [`PickerKind`]
/// — texture / snapshot items in texture mode, GLTF render materials in material
/// mode.
const fn item_matches(inv_type: InventoryType, kind: PickerKind) -> bool {
    match kind {
        PickerKind::Texture => {
            matches!(inv_type, InventoryType::Texture | InventoryType::Snapshot)
        }
        PickerKind::Material => matches!(inv_type, InventoryType::Material),
    }
}

/// Rebuild the tree rows when marked dirty **and** the tree's structure actually
/// changed (open / expand / collapse / filter / a fetch landing): despawn the old
/// rows and spawn the flattened set. A dirty tick whose rows hash the same — a
/// mere selection change, or an inventory update that did not alter the visible
/// tree — despawns nothing, so a just-clicked row is never left despawned in the
/// pointer hover-map (which would fail the UI-block test and let the click fall
/// through to the world and deselect the object). The selection highlight is
/// painted separately by [`paint_tree_selection`].
fn rebuild_tree(
    mut state: ResMut<TexturePickerState>,
    ui: Option<Res<TexturePickerUi>>,
    inventory: Res<InventoryModel>,
    trees: Query<&Children>,
    mut counts: Query<&mut Text>,
    mut commands: Commands,
) {
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let Some(ui) = ui else {
        return;
    };
    let rows = build_tree_rows(&inventory, &state);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rows.hash(&mut hasher);
    let sig = hasher.finish();
    if sig == state.last_rows_sig {
        // Structure unchanged: keep the existing rows (and their valid hover-map
        // entries); the highlight follows via [`paint_tree_selection`].
        return;
    }
    state.last_rows_sig = sig;
    if let Ok(children) = trees.get(ui.tree) {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    let total = rows.len();
    let shown = total.min(MAX_ROWS);
    if let Some(slice) = rows.get(..shown) {
        for row_data in slice {
            spawn_tree_row(&mut commands, ui.tree, row_data, state.kind);
        }
    }
    if let Ok(mut text) = counts.get_mut(ui.count) {
        text.0 = if total > shown {
            format!("{shown} / {total} — refine search")
        } else {
            format!("{total}")
        };
    }
}

/// Paint the selected texture item's row highlight on the existing rows (so a
/// selection change never respawns rows). Only touches a row whose highlight
/// state actually flips, leaving the hover tint alone.
fn paint_tree_selection(
    state: Res<TexturePickerState>,
    mut rows: Query<(&TreeItemRow, &mut BackgroundColor)>,
) {
    for (row, mut background) in &mut rows {
        let selected = row.0 == state.selected;
        if selected && background.0 != SELECTED_FILL {
            background.0 = SELECTED_FILL;
        } else if !selected && background.0 == SELECTED_FILL {
            background.0 = Color::NONE;
        }
    }
}

/// Spawn one tree row (a folder or an item). The item glyph follows `kind` (a
/// material icon in material mode). The selection highlight is applied by
/// [`paint_tree_selection`], not here, so a selection change never respawns.
fn spawn_tree_row(commands: &mut Commands, tree: Entity, row_data: &TreeRow, kind: PickerKind) {
    let item_icon_type = match kind {
        PickerKind::Texture => InventoryType::Texture,
        PickerKind::Material => InventoryType::Material,
    };
    let (depth, glyph, label, colour) = match row_data {
        TreeRow::Folder {
            depth,
            expanded,
            name,
            ..
        } => {
            let arrow = if *expanded { "\u{25be}" } else { "\u{25b8}" };
            (
                *depth,
                format!("{arrow} \u{1f4c1}"),
                name.clone(),
                FOLDER_COLOR,
            )
        }
        TreeRow::Item { depth, name, .. } => (
            *depth,
            String::from(item_icon(item_icon_type)),
            name.clone(),
            TEXT_COLOR,
        ),
    };
    let row_entity = commands
        .spawn((
            Button,
            Node {
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                padding: UiRect::left(Val::Px(4.0 + depth_indent(depth))),
                column_gap: Val::Px(4.0),
                ..row(Val::ZERO)
            },
            BackgroundColor(Color::NONE),
            // Hoverable and clickable, but does NOT block lower: the stable tree
            // container beneath is what the world-pick's UI-block test sees, so a
            // row that despawns on a rebuild can never leave a hole in the block
            // (which would let the click fall through to the world and deselect).
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("texture-picker-row"),
            ChildOf(tree),
        ))
        .id();
    match row_data {
        TreeRow::Folder { key, .. } => {
            commands.entity(row_entity).insert(TreeFolderRow(*key));
            commands.entity(row_entity).observe(on_folder_row_press);
        }
        TreeRow::Item { key, .. } => {
            commands.entity(row_entity).insert(TreeItemRow(*key));
            commands.entity(row_entity).observe(on_item_row_press);
        }
    }
    commands
        .entity(row_entity)
        .observe(on_row_hover)
        .observe(on_row_unhover);
    commands.spawn((
        Text::new(glyph),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(colour),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    commands.spawn((
        Text::new(label),
        UiFont::Sans.at(PICKER_FONT),
        TextColor(colour),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
}

/// The left indent for a tree row at `depth`.
fn depth_indent(depth: usize) -> f32 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "tree depth is a small non-negative integer, exact as f32"
    )]
    let depth = depth as f32;
    depth * INDENT_PER_DEPTH
}

/// Highlight a row under the pointer (unless it is the selected item).
fn on_row_hover(over: On<Pointer<Over>>, mut rows: Query<&mut BackgroundColor, With<Button>>) {
    if let Ok(mut background) = rows.get_mut(over.entity)
        && background.0 == Color::NONE
    {
        background.0 = ROW_HOVER;
    }
}

/// Clear a row's hover highlight (unless it is the selected item).
fn on_row_unhover(out: On<Pointer<Out>>, mut rows: Query<&mut BackgroundColor, With<Button>>) {
    if let Ok(mut background) = rows.get_mut(out.entity)
        && background.0 == ROW_HOVER
    {
        background.0 = Color::NONE;
    }
}

/// Toggle a folder row's expansion, lazily fetching its contents the first time.
fn on_folder_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&TreeFolderRow>,
    mut state: ResMut<TexturePickerState>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&TreeFolderRow(folder)) = rows.get(press.entity) else {
        return;
    };
    if state.expanded.contains(&folder) {
        state.expanded.remove(&folder);
    } else {
        state.expanded.insert(folder);
        // Fetch the folder's contents the first time it is opened here.
        if state.requested.insert(folder) {
            query_folder_page(folder, &mut commands);
        }
    }
    state.dirty = true;
}

/// Select a texture item row: update the selection and live-preview it.
fn on_item_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&TreeItemRow>,
    mut state: ResMut<TexturePickerState>,
    mut picked: MessageWriter<TexturePicked>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&TreeItemRow(texture)) = rows.get(press.entity) else {
        return;
    };
    select_texture(&mut state, texture, &mut picked);
}

/// Set the picker's selection, mark the tree for a re-paint, and emit a live
/// (non-final) preview for the requester.
fn select_texture(
    state: &mut TexturePickerState,
    texture: TextureKey,
    picked: &mut MessageWriter<TexturePicked>,
) {
    state.selected = texture;
    state.dirty = true;
    if let Some(requester) = state.requester {
        picked.write(TexturePicked {
            requester,
            texture,
            final_pick: false,
        });
    }
}

/// Request the selected texture's decode for the preview pane (once per change).
/// Skipped in material mode: a material id is not a texture, so decoding it would
/// issue a bogus fetch (the preview was cleared on open, pending the sphere task).
fn request_preview_texture(
    ui: Option<Res<TexturePickerUi>>,
    state: Res<TexturePickerState>,
    mut textures: ResMut<TextureManager>,
    mut pending: ResMut<PendingTexturePreviews>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !state.is_changed() || state.kind == PickerKind::Material {
        return;
    }
    textures.request_boosted(state.selected, AVATAR_BOOST_PRIORITY);
    pending
        .waiting
        .entry(state.selected)
        .or_default()
        .push(ui.preview);
}

/// Keep the preview pane's [`MaterialPreview`] in step with the picker's selection
/// while it is open in **material** mode — so the pane previews the selected
/// material on a lit sphere ([`crate::material_preview`]), the reference's
/// `LLTextureCtrl` material preview. In texture mode or once the picker is closed
/// the component is removed, handing the pane back to the texture-preview path.
fn sync_material_preview_pane(
    ui: Option<Res<TexturePickerUi>>,
    state: Res<TexturePickerState>,
    mut previews: Query<&mut MaterialPreview>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    if !state.is_changed() {
        return;
    }
    if state.requester.is_none() || state.kind != PickerKind::Material {
        // Closed, or a texture-mode pick: the pane is not a material preview.
        if let Ok(mut pane) = commands.get_entity(ui.preview) {
            pane.remove::<MaterialPreview>();
        }
        return;
    }
    let want = if state.selected.uuid().is_nil() {
        MaterialPreview::Empty
    } else {
        MaterialPreview::Asset(AssetKey::from(state.selected.uuid()))
    };
    if let Ok(mut preview) = previews.get_mut(ui.preview) {
        if *preview != want {
            *preview = want;
        }
    } else if let Ok(mut pane) = commands.get_entity(ui.preview) {
        pane.insert(want);
    }
}

/// Swap thumbnail nodes for their decoded textures as they land (the preview and
/// the swatches).
fn resolve_texture_previews(
    manager: Res<TextureManager>,
    mut pending: ResMut<PendingTexturePreviews>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    if pending.waiting.is_empty() {
        return;
    }
    let ready: Vec<TextureKey> = pending
        .waiting
        .keys()
        .copied()
        .filter(|key| manager.decoded(*key).is_some())
        .collect();
    for key in ready {
        let Some(decoded) = manager.decoded(key) else {
            continue;
        };
        let handle = images.add(to_bevy_image(decoded));
        if let Some(nodes) = pending.waiting.remove(&key) {
            for node in nodes {
                if let Ok(mut entity) = commands.get_entity(node) {
                    entity.insert(ImageNode::new(handle.clone()));
                }
            }
        }
    }
}

/// Keep each texture swatch showing its [`TextureSwatchValue`] thumbnail — or, for
/// a nil (no-texture) value, clear the thumbnail so a deselected swatch does not
/// keep showing the last prim's texture.
fn apply_texture_swatch_thumbnail(
    swatches: Query<(Entity, &TextureSwatchValue), Changed<TextureSwatchValue>>,
    mut textures: ResMut<TextureManager>,
    mut pending: ResMut<PendingTexturePreviews>,
    mut commands: Commands,
) {
    for (entity, value) in &swatches {
        if value.0.uuid().is_nil() {
            if let Ok(mut swatch) = commands.get_entity(entity) {
                swatch.remove::<ImageNode>();
                swatch.insert(BackgroundColor(EMPTY_FILL));
            }
            continue;
        }
        textures.request_boosted(value.0, AVATAR_BOOST_PRIORITY);
        pending.waiting.entry(value.0).or_default().push(entity);
    }
}

/// Scroll the tree with the wheel while the pointer is over it.
fn scroll_tree(
    wheel: Res<AccumulatedMouseScroll>,
    ui: Option<Res<TexturePickerUi>>,
    hover: Res<HoverMap>,
    parents: Query<&ChildOf>,
    mut positions: Query<&mut ScrollPosition>,
) {
    let Some(ui) = ui else {
        return;
    };
    if wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    let over = hover.values().flat_map(|hits| hits.keys()).any(|hovered| {
        let mut node = *hovered;
        loop {
            if node == ui.tree {
                return true;
            }
            match parents.get(node) {
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
    if let Ok(mut position) = positions.get_mut(ui.tree) {
        position.0.y = (position.0.y - delta).max(0.0);
    }
}

/// A quick-choice / OK / Cancel press.
fn on_picker_button(
    press: On<Pointer<Press>>,
    buttons: Query<&PickerButton>,
    ui: Option<Res<TexturePickerUi>>,
    mut state: ResMut<TexturePickerState>,
    mut picked: MessageWriter<TexturePicked>,
    mut panels: Query<&mut UiPanelShown>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(which) = buttons.get(press.entity) else {
        return;
    };
    match which {
        PickerButton::None => {
            select_texture(&mut state, TextureKey::from(Uuid::nil()), &mut picked);
        }
        PickerButton::Blank => {
            select_texture(&mut state, TextureKey::from(IMG_BLANK), &mut picked);
        }
        PickerButton::Default => {
            select_texture(&mut state, TextureKey::from(IMG_DEFAULT), &mut picked);
        }
        PickerButton::Ok => {
            if let Some(requester) = state.requester {
                picked.write(TexturePicked {
                    requester,
                    texture: state.selected,
                    final_pick: true,
                });
            }
            close_picker(&mut state, ui.as_deref(), &mut panels);
        }
        PickerButton::Cancel => {
            // Revert the live preview to the texture the picker opened on.
            if let Some(requester) = state.requester {
                picked.write(TexturePicked {
                    requester,
                    texture: state.original,
                    final_pick: false,
                });
            }
            close_picker(&mut state, ui.as_deref(), &mut panels);
        }
    }
}

/// Close the picker and clear its requester.
fn close_picker(
    state: &mut TexturePickerState,
    ui: Option<&TexturePickerUi>,
    panels: &mut Query<&mut UiPanelShown>,
) {
    state.requester = None;
    if let Some(ui) = ui
        && let Ok(mut shown) = panels.get_mut(ui.panel)
    {
        shown.0 = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{PickerKind, item_matches};
    use sl_client_bevy::InventoryType;

    /// Texture mode lists texture and snapshot items and nothing else — a
    /// material must not leak into a texture pick.
    #[test]
    fn texture_kind_lists_textures_and_snapshots_only() {
        assert!(item_matches(InventoryType::Texture, PickerKind::Texture));
        assert!(item_matches(InventoryType::Snapshot, PickerKind::Texture));
        assert!(!item_matches(InventoryType::Material, PickerKind::Texture));
        assert!(!item_matches(InventoryType::Object, PickerKind::Texture));
        assert!(!item_matches(InventoryType::Notecard, PickerKind::Texture));
    }

    /// Material mode lists only render-material items — not textures, snapshots,
    /// or anything else.
    #[test]
    fn material_kind_lists_materials_only() {
        assert!(item_matches(InventoryType::Material, PickerKind::Material));
        assert!(!item_matches(InventoryType::Texture, PickerKind::Material));
        assert!(!item_matches(InventoryType::Snapshot, PickerKind::Material));
        assert!(!item_matches(InventoryType::Object, PickerKind::Material));
    }
}
