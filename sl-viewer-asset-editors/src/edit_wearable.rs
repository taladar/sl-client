//! The **appearance editor** floater (`viewer-appearance-editor-shell` +
//! `-bodyparts` + `-clothing`): edit a **worn** wearable — a body part
//! (Shape / Skin / Hair / Eyes) or a clothing layer (shirt, pants, …) — with
//! live preview and Save / Save-As / Revert.
//!
//! # One data-driven floater for every slot
//!
//! Every wearable editor in the reference viewer (`LLPanelEditWearable`) is the
//! same shape: the slot's `avatar_lad` visual-param sliders, its per-layer
//! **texture pickers**, and — for a clothing layer — a **tint** colour swatch.
//! This module builds that from data: the param list is
//! [`VisualParams`](sl_client_bevy::VisualParams) filtered to the slot's
//! `wearable` group (and the avatar's sex), the texture slots come from
//! [`avatar_texture::LAYER_TEXTURES`], and the tint swatch drives the slot's
//! three colour params. So Shape falls out as ~80 sliders with no textures, Skin
//! as a few tone sliders plus three bodypaint pickers, a shirt as a fabric
//! picker + tint swatch + a couple of fit sliders — all one code path.
//!
//! # Live preview through the existing pipeline
//!
//! Editing does not touch the render systems directly: it substitutes the edited
//! [`WearableAsset`] into [`OwnBakeInputs`] (via
//! [`set_preview_asset`](OwnBakeInputs::set_preview_asset)), so the shape morph
//! (`avatars::apply_own_shape_from_wearables`)
//! re-derives the body from the edited params and the bake composite re-runs from
//! the edited textures / tints — the same path that renders the worn outfit.
//!
//! # Save
//!
//! **Save** authors the edited `.wearable` asset ([`WearableAsset::to_text`]) and
//! writes it back onto the *same* item over the legacy transaction upload
//! ([`Command::SaveInventoryAsset`]) — the reference's
//! `LLAgentWearables::saveWearable`. **Save As** makes a fresh item the way a
//! new wearable is made ([`new_wearable_command`]: the asset over the same
//! transaction upload, and a `CreateInventoryItem` naming it) — the
//! reference's `LLWearableList::createCopy`. **Revert** restores the asset the
//! editor opened on.
//!
//! # One window per wearable
//!
//! The editor is a **keyed floater** ([`FloaterKey`], `viewer-key-wearable-
//! editor`): opening a second wearable opens a second window rather than
//! re-pointing the first, so a shape can be edited beside the skin it sits
//! under, and opening one can never discard another's unsaved edit. A window
//! is keyed by its inventory item — a Current Outfit Folder link opens the item
//! it links to, so a link row and the item's own row are one window, and Save
//! writes onto the wearable rather than onto the link. Re-opening a wearable
//! already up only **raises** its window and never re-seeds it.
//!
//! Each window's edit (`WearEdit`) is a component on that window, and every
//! system and observer here reaches its edit through the window the control
//! lives in ([`FloaterHost`]): a picker reply names its swatch, a drag its
//! slider, a press its button. Closing a window ends it and its edit.
//!
//! - **The preview.** The worn avatar is one, and every open editor previews
//!   on it. That is safe because the bake inputs hold the worn assets **by
//!   item** ([`OwnBakeInputs::set_preview_asset`]): each window substitutes its
//!   own item's asset, so a shape window and a skin window both show, and a
//!   window closing puts back its own item's asset and leaves the other's edit
//!   on the avatar. Two layers of one type preview independently too: the
//!   composite draws every worn wearable of a type, in worn order, each from
//!   its own asset (`sl-bake`'s `WornLayer`).
//! - **Save.** An in-place save and a Save As are each matched back by the
//!   transaction they minted, carried by the window that sent them, so a reply
//!   lands in the window that saved.
//!
//! Reference (Firestorm, read-only): `llpaneleditwearable.cpp`,
//! `llfloatercustomize.cpp`, `llwearabletype.cpp`.

use std::collections::BTreeMap;
use std::collections::HashSet;

use crate::skin_palette::SkinPalette;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Slider, SliderRange, SliderStep, SliderValue, ValueChange};
use sl_client_bevy::{
    ASSET_CODE_LINK, AssetType, Command, InventoryKey, ItemInfo, JointOverrides, ParamEffect,
    ParamGroup, ParamSex, ResolvedParams, SkeletalDeformations, SlCommand, SlEvent, SlSessionEvent,
    TextureKey, TransactionId, Uuid, WearableAsset, WearablePermissions, WearableSaleType,
    WearableType,
};
use sl_client_bevy::{SaleType, avatar_texture};

use crate::asset_editor::{SaveEditorWindow, UnsavedWork};
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::OwnLocalBake;
use crate::bake_inputs::{OwnBakeInputs, shape_is_male};
use crate::floater::{
    FloaterCaps, FloaterHandle, FloaterHost, FloaterKey, FloaterSpec, FloaterSystems,
    KeyedFloaterOpen, KeyedFloaters,
};
use crate::intents::TexturePicked;
use crate::intents::{ItemCreationFinished, PendingItemCreations, new_wearable_command};
use crate::inventory::{InventoryModel, OpenWearableEditor};
use crate::inventory_actions::{wearable_param_group, wearable_type_of};
use crate::inventory_properties::to_wire_item;
use crate::textures::{TextureDecoded, TextureManager};
use crate::ui::{column, row};
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_font::UiFont;
use crate::ui_radio::{RadioLayout, RadioSelection, RadioSpec, spawn_radio_group};
use crate::ui_slider::{SliderFormat, SliderLabels, SliderStyle, SliderWidgetPlugin, spawn_slider};
use crate::ui_spawn::{self, ButtonKind, ButtonSpec, LabeledRowSpec, UiLabel};
use crate::ui_texture_picker::{TextureSwatchValue, spawn_texture_swatch};
use crate::world_api::DecodedTextures;
use sl_viewer_ui_core::scrollbar::{ScrollTarget, spawn_scrollbar};
use sl_viewer_ui_core::skin::text_role;

/// The Shape gender radio group's element id.
const GENDER_ELEMENT: &str = "wearable-gender";

/// The wearable-asset format version the editor authors (`LLWearable version`).
const WEARABLE_VERSION: i32 = 22;

/// The chrome font size, in logical pixels.
const FONT: f32 = 13.0;

/// The scrollable param list's height, in logical pixels.
const LIST_HEIGHT: f32 = 380.0;

/// Logical pixels scrolled per wheel notch (`MouseScrollUnit::Line`).
const LINE_SCROLL_PIXELS: f32 = 40.0;

/// How a wearable parameter's slider is drawn.
const SLIDER: SliderStyle = SliderStyle {
    track_width: 150.0,
    track_height: 12.0,
    border: 1.0,
    border_color: CONTROL_BORDER,
    track_fill: TRACK_FILL,
    thumb_width: 9.0,
    thumb_fill: THUMB_FILL,
    font_size: FONT,
};

/// The label colour.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// A control's border colour.
const CONTROL_BORDER: Color = Color::srgba(0.34, 0.40, 0.52, 1.0);

/// A slider track's fill.
const TRACK_FILL: Color = Color::srgba(0.12, 0.13, 0.16, 1.0);

/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.72, 0.76, 0.84);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

// ---------------------------------------------------------------------------
// Messages, components and resources.
// ---------------------------------------------------------------------------

/// A param slider row: the param it drives, and whether that param feeds the
/// bake (a colour / alpha layer) rather than the body shape. Its value readout
/// is the slider widget's.
#[derive(Component, Debug, Clone, Copy)]
struct WearParamSlider {
    /// The visual-param id.
    id: i32,
    /// Whether editing this param re-composites the bake (a colour / alpha
    /// param) rather than only re-shaping the body.
    is_bake: bool,
}

/// A texture picker swatch row: the avatar `TextureEntry` slot it edits.
#[derive(Component, Debug, Clone, Copy)]
struct WearTextureSwatch(usize);

/// The single tint colour swatch (a clothing layer's colour).
#[derive(Component, Debug, Clone, Copy)]
struct WearTintSwatch;

/// The Shape editor's gender radio group (Female / Male → the `male` param).
#[derive(Component, Debug, Clone, Copy)]
struct WearGenderRadio;

/// The scrollable control list (wheel-scroll target).
#[derive(Component, Debug, Clone, Copy)]
struct WearScrollList;

/// Which action a chrome button performs.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum WearButton {
    /// Write the edit back onto the same item (in-place).
    Save,
    /// Mint a fresh item from the edit.
    SaveAs,
    /// Restore the asset the editor opened on.
    Revert,
}

/// One window's wearable edit — a **component on the window**, since the
/// editor opens per wearable ([`FloaterKey`]) and the edit lives exactly as
/// long as its window.
#[derive(Component)]
struct WearEdit {
    /// The item being edited (its permissions / folder feed Save).
    item: ItemInfo,
    /// The wearable slot.
    wearable_type: WearableType,
    /// The asset the editor opened on (for Revert).
    original: WearableAsset,
    /// The live-edited asset (the Save / preview source of truth).
    edited: WearableAsset,
    /// The slot's tint colour params (`[r, g, b]`), when it has a tint swatch.
    tint_params: Option<[i32; 3]>,
    /// The tint swatch entity, to filter its [`ColorPicked`] replies.
    tint_swatch: Option<Entity>,
    /// The `male` param id (the Shape gender radio drives it), when known.
    gender_param: Option<i32>,
    /// The height read-out label node, for a Shape edit.
    height_label: Option<Entity>,
    /// Edited layer textures still decoding, so the bake preview re-composites
    /// once they arrive.
    pending_textures: HashSet<TextureKey>,
    /// A param change is awaiting a shape re-derive.
    shape_dirty: bool,
    /// A texture / tint change is awaiting a bake re-composite.
    bake_dirty: bool,
    /// The transaction an in-place Save put on the wire, or `None` when none is
    /// in flight.
    ///
    /// The completion (`InventoryAssetSaved`) names the stored asset, not the
    /// item, so this is the only thing that tells *this* editor's result from
    /// another save's — matching on "a save completed while we were saving"
    /// reports whichever one finishes first, whosever it was.
    saving: Option<TransactionId>,
    /// The transaction of this editor's **Save As** in flight, or `None` when
    /// none is — which is also how "a copy is on the wire" is known, since the
    /// two are the same fact.
    ///
    /// The copy's item and its upload's refusal both name the transaction, so
    /// it is what tells *this* editor's copy from another creation.
    creation: Option<TransactionId>,
    /// Whether the edit has moved since it was opened, reverted, or last saved —
    /// what the close guard asks about, and what a Revert has to undo.
    dirty: bool,
    /// The status readout node.
    status: Option<Entity>,
}

/// The appearance-editor plugin.
#[derive(Debug)]
pub struct EditWearablePlugin;

impl Plugin for EditWearablePlugin {
    /// Register the open message and the open / per-window systems.
    ///
    /// Nothing spawns at `Startup`: a wearable window exists only while that
    /// wearable is open, so `open_wearable_editor` spawns the instance. The
    /// per-window systems are gated on there being a window; the preview's
    /// restore runs as the window's edit goes (`restore_preview_on_close`).
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SliderWidgetPlugin>() {
            app.add_plugins(SliderWidgetPlugin);
        }
        app.add_message::<OpenWearableEditor>()
            .add_observer(restore_preview_on_close)
            .add_systems(
                Update,
                (
                    // After the manager's command pass — see `FloaterSystems`:
                    // the inventory row whose Edit opens a wearable also raises
                    // the inventory window, and the later raise wins.
                    open_wearable_editor.after(FloaterSystems::Commands),
                    (
                        apply_wear_texture_picked,
                        apply_wear_tint_picked,
                        apply_wear_gender_radio,
                        note_edited_texture_decoded,
                        drive_wearable_preview,
                        save_wearable,
                        report_wearable_save,
                        report_wearable_save_as,
                        track_wearable_unsaved,
                        scroll_wearable_list,
                    )
                        .chain()
                        .run_if(any_with_component::<WearEdit>),
                )
                    .chain(),
            );
    }
}

/// The wearable editor floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn wearable_editor_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "wearable-editor",
        title: "Edit Wearable".to_owned(),
        position: Vec2::new(300.0, 80.0),
        // A little wider than the reference's 333 px panel: a slider row is a
        // fixed-width track plus a font-sized readout, so the label column is
        // all that gives, and at a large font a long translated label's widest
        // word needs the extra room.
        default_size: Some(Vec2::new(360.0, 520.0)),
        min_size: Some(Vec2::new(280.0, 240.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

// ---------------------------------------------------------------------------
// Slot data tables.
// ---------------------------------------------------------------------------

/// The clothing layer's three tint colour params (`[red, green, blue]`), or
/// `None` for a slot with no single tint swatch (body parts, tattoo's per-region
/// tints, the universal layers). The ids match `avatar_lad.xml` (and the
/// `sl-bake` layer plan).
const fn wearable_tint_params(slot: WearableType) -> Option<[i32; 3]> {
    match slot {
        WearableType::Shirt => Some([803, 804, 805]),
        WearableType::Pants => Some([806, 807, 808]),
        WearableType::Shoes => Some([812, 813, 817]),
        WearableType::Socks => Some([818, 819, 820]),
        WearableType::Jacket => Some([831, 832, 833]),
        WearableType::Gloves => Some([827, 829, 830]),
        WearableType::Undershirt => Some([821, 822, 823]),
        WearableType::Underpants => Some([824, 825, 826]),
        WearableType::Skirt => Some([921, 922, 923]),
        _other => None,
    }
}

/// The texture-picker slots a wearable type exposes: each `TextureEntry` layer
/// the slot supplies, with a display label. Derived from
/// [`avatar_texture::LAYER_TEXTURES`] (skin → three bodypaint layers, hair → one,
/// a clothing layer → its fabric, alpha → its masks, …).
fn wearable_texture_slots(slot: WearableType) -> Vec<(usize, String)> {
    avatar_texture::LAYER_TEXTURES
        .iter()
        .filter(|(_slot, _name, wearable)| *wearable == slot)
        .map(|(te, name, _wearable)| (*te, prettify(name)))
        .collect()
}

/// Turn an `avatar_lad` layer name (`upper_bodypaint`) into a display label
/// (`Upper Bodypaint`).
fn prettify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for word in name.split('_') {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Open — rebuild the content from the worn wearable.
// ---------------------------------------------------------------------------

/// The item a wearable row stands for: the item itself, or — for a Current
/// Outfit Folder **link** — the item it links to, when that is loaded.
///
/// A link is not the wearable: its asset id names its target, and a save
/// written onto it would land on the link. Keying by the target also makes a
/// link row and the item's own row one window rather than two editors of one
/// wearable. A link whose target is not loaded is opened as it is.
fn resolve_wearable(item: &ItemInfo, model: Option<&InventoryModel>) -> ItemInfo {
    if item.asset_type != AssetType::Other(ASSET_CODE_LINK) {
        return item.clone();
    }
    model
        .and_then(|model| model.find_item(InventoryKey::from(item.asset_id)))
        .cloned()
        .unwrap_or_else(|| item.clone())
}

/// The [`FloaterKey`] of the window editing the wearable `item`.
fn wearable_key(item: InventoryKey) -> FloaterKey {
    FloaterKey::subject(&item)
}

/// What a new window's edit is seeded from, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the worn outfit's parsed
/// assets, the avatar-param library its sliders are listed from, and the
/// inventory a link row is resolved through.
#[derive(bevy::ecs::system::SystemParam)]
struct WearSeed<'w> {
    /// The worn assets, by item.
    inputs: Res<'w, OwnBakeInputs>,
    /// The avatar-param library, for the slot's sliders.
    library: Option<Res<'w, AvatarAssetLibrary>>,
    /// The inventory, for a link's target.
    inventory: Option<Res<'w, InventoryModel>>,
}

/// Open a wearable **per wearable** (`viewer-key-wearable-editor`): raise its
/// window when it is already up, and otherwise spawn one, seed a fresh
/// [`WearEdit`] from the worn asset and build the controls into it.
///
/// Every open of the frame is honoured, not just the last. A window already up
/// is only raised — never re-seeded — because re-seeding it would throw away
/// exactly what a second open must not touch: the resident's unsaved edits.
fn open_wearable_editor(
    mut opens: MessageReader<OpenWearableEditor>,
    mut floaters: KeyedFloaters,
    seed: WearSeed,
    mut commands: Commands,
) {
    for open in opens.read() {
        let item = resolve_wearable(&open.item, seed.inventory.as_deref());
        let KeyedFloaterOpen::Spawned(handle) =
            floaters.open(wearable_editor_floater_spec(), wearable_key(item.item_id))
        else {
            continue;
        };
        build_wearable_window(&mut commands, handle, item, &seed);
    }
}

/// Furnish a freshly spawned wearable window: its title, its controls, the
/// unsaved-work guard, and the [`WearEdit`] the per-window systems find it by.
fn build_wearable_window(
    commands: &mut Commands,
    handle: FloaterHandle,
    item: ItemInfo,
    seed: &WearSeed,
) {
    let slot = wearable_type_of(&item);
    // The title node carries no Fluent key, so a direct text set is not fought
    // by the translator.
    commands
        .entity(handle.title_text)
        .insert(Text::new(format!("Edit: {}", item.name)));

    // Seed the edited asset from this item's worn asset (falling back to a
    // fresh defaults asset if the outfit's bake inputs are not assembled yet).
    let mut edited = seed
        .inputs
        .worn_asset_of(item.item_id)
        .cloned()
        .unwrap_or_else(|| WearableAsset {
            version: WEARABLE_VERSION,
            name: item.name.clone(),
            wearable_type: slot,
            params: BTreeMap::new(),
            textures: BTreeMap::new(),
        });
    edited.wearable_type = slot;

    let tint_params = wearable_tint_params(slot);
    let library = seed.library.as_deref();
    let sex = avatar_sex(library, seed.inputs.worn_asset(WearableType::Shape));

    // Fill in every editable group param at its current (or default) value, so
    // the saved asset is complete and the sliders start at the right place.
    let sliders: Vec<ParamSliderSpec> = library
        .map(|library| editable_params(library, slot, sex, tint_params))
        .unwrap_or_default();
    for slider in &sliders {
        let _prev = edited.params.entry(slider.id).or_insert(slider.value);
    }

    let gender_param = library
        .and_then(|library| library.params().by_name("male"))
        .map(|param| param.id);
    let controls = spawn_wearable_controls(
        commands,
        handle.content,
        FONT,
        &edited,
        &sliders,
        gender_param,
    );

    commands.entity(handle.root).insert((
        // The edit previews on the avatar and is not stored until a Save, so a
        // close with the sliders moved throws real work away — the scaffold's
        // guard asks first ([`track_wearable_unsaved`]).
        UnsavedWork::default(),
        WearEdit {
            item,
            wearable_type: slot,
            original: edited.clone(),
            edited,
            tint_params,
            tint_swatch: controls.tint_swatch,
            gender_param,
            height_label: controls.height_label,
            pending_textures: HashSet::new(),
            shape_dirty: true,
            bake_dirty: true,
            saving: None,
            creation: None,
            dirty: false,
            status: Some(controls.status),
        },
    ));
}

/// The control entities [`spawn_wearable_controls`] returns, the ones the
/// edit keeps to update or filter by afterwards.
#[derive(Debug, Clone, Copy)]
struct WearControls {
    /// The status readout node.
    status: Entity,
    /// The tint swatch, for a slot that has one.
    tint_swatch: Option<Entity>,
    /// The Shape height read-out.
    height_label: Option<Entity>,
}

/// Build the editor's controls for `edited` into `content` at `font_size`: the
/// Save / Save As / Revert row, the status line, and the scrolling list of the
/// slot's texture pickers, tint swatch and one slider per `sliders` entry
/// (headed, for a Shape, by the height read-out and the gender toggle driving
/// `gender_param`). Shared by the live editor and its specimen.
fn spawn_wearable_controls(
    commands: &mut Commands,
    content: Entity,
    font_size: f32,
    edited: &WearableAsset,
    sliders: &[ParamSliderSpec],
    gender_param: Option<i32>,
) -> WearControls {
    let mut tab = 0_i32;
    // Action buttons row.
    let button_row = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    for (kind, label) in [
        (WearButton::Save, "Save"),
        (WearButton::SaveAs, "Save As"),
        (WearButton::Revert, "Revert"),
    ] {
        spawn_action_button(commands, button_row, kind, label, &mut tab, font_size);
    }
    let status = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(font_size),
            text_role(LABEL_COLOR),
            Name::new("wearable-status"),
            Node {
                margin: UiRect::bottom(Val::Px(4.0)),
                ..Default::default()
            },
            ChildOf(content),
        ))
        .id();

    // Scrollable list of texture pickers, the tint swatch, then the sliders.
    let list_row = commands
        .spawn((
            Node {
                height: Val::Px(LIST_HEIGHT),
                ..row(Val::ZERO)
            },
            Name::new("wearable-list-row"),
            ChildOf(content),
        ))
        .id();
    let list = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(4.0))
            },
            ScrollPosition::default(),
            WearScrollList,
            ChildOf(list_row),
        ))
        .id();
    spawn_scrollbar(
        commands,
        list_row,
        ScrollTarget::Container(list),
        Node::default(),
        "wearable-list-scrollbar",
    );

    // Shape: a height read-out and a gender toggle head the list (the reference's
    // Body sub-tab). Other slots have neither.
    let mut height_label = None;
    if edited.wearable_type == WearableType::Shape {
        let height_row = spawn_labeled_row(commands, list, "Height", font_size);
        height_label = Some(
            commands
                .spawn((
                    Text::new("— m"),
                    UiFont::Sans.at(font_size),
                    text_role(LABEL_COLOR),
                    ChildOf(height_row),
                ))
                .id(),
        );
        let male = gender_param
            .and_then(|id| edited.params.get(&id).copied())
            .unwrap_or(0.0);
        let gender_row = spawn_labeled_row(commands, list, "Gender", font_size);
        let labels = [String::from("Female"), String::from("Male")];
        let group = spawn_radio_group(
            commands,
            gender_row,
            &RadioSpec {
                element: GENDER_ELEMENT,
                labels: &labels,
                active: usize::from(male > 0.5),
                tab_index: tab,
                font_size,
                layout: RadioLayout::Row,
                translate_labels: false,
            },
        );
        commands.entity(group).insert(WearGenderRadio);
        tab = tab.saturating_add(1);
    }

    for (te, label) in wearable_texture_slots(edited.wearable_type) {
        let current = edited
            .textures
            .get(&u32::try_from(te).unwrap_or_default())
            .copied()
            .map_or_else(|| TextureKey::from(Uuid::nil()), TextureKey::from);
        let picker_row = spawn_labeled_row(commands, list, &label, font_size);
        let swatch = spawn_texture_swatch(commands, picker_row, "wearable", tab, current);
        commands.entity(swatch).insert(WearTextureSwatch(te));
        tab = tab.saturating_add(1);
    }

    let mut tint_swatch = None;
    if let Some([r, g, b]) = wearable_tint_params(edited.wearable_type) {
        let color = tint_color(edited, [r, g, b]);
        let tint_row = spawn_labeled_row(commands, list, "Color / Tint", font_size);
        let swatch = spawn_color_swatch(commands, tint_row, "wearable-tint", tab, color);
        commands.entity(swatch).insert(WearTintSwatch);
        tint_swatch = Some(swatch);
        tab = tab.saturating_add(1);
    }

    for slider in sliders {
        spawn_param_slider(commands, list, slider, &mut tab, font_size);
    }

    WearControls {
        status,
        tint_swatch,
        height_label,
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The wearable editor's gallery / `ui_test` specimen: the live controls,
/// built by the same `spawn_wearable_controls` at the cell's font size, for
/// a sample **shirt** — its fabric picker (the slot list the live editor
/// derives), its tint swatch on a sample colour and a handful of fit sliders.
///
/// The sliders are sample rows rather than the slot's real `avatar_lad`
/// params: those come from the avatar-asset library, which a specimen does not
/// load. The ids and ranges are the shirt's own.
pub fn spawn_wearable_editor_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let sliders = [
        (600, "Sleeve Length", 0.0, 0.85, 0.7, false),
        (601, "Shirt Bottom", 0.0, 1.0, 0.8, false),
        (602, "Collar Front", 0.0, 1.0, 0.35, false),
        (778, "Collar Back", 0.0, 1.0, 0.75, false),
        (828, "Loose Shirt", 0.0, 1.0, 0.2, false),
        (868, "Shirt Wrinkles", 0.0, 1.0, 0.0, true),
    ]
    .into_iter()
    .map(|(id, label, min, max, value, is_bake)| ParamSliderSpec {
        id,
        is_bake,
        min,
        max,
        value,
        label: cx.text(label),
    })
    .collect::<Vec<_>>();
    let mut params = sliders
        .iter()
        .map(|slider| (slider.id, slider.value))
        .collect::<BTreeMap<_, _>>();
    if let Some([r, g, b]) = wearable_tint_params(WearableType::Shirt) {
        params.extend([(r, 0.28), (g, 0.45), (b, 0.62)]);
    }
    let edited = WearableAsset {
        version: WEARABLE_VERSION,
        name: cx.text("Sample Shirt"),
        wearable_type: WearableType::Shirt,
        params,
        textures: BTreeMap::new(),
    };
    spawn_wearable_controls(commands, parent, cx.font_size, &edited, &sliders, None);
    parent
}

/// One editable visual param, and everything its slider row is built from: the
/// param's id and label, whether it feeds the bake, the range the track spans,
/// and the value the thumb starts at.
#[derive(Debug, Clone)]
struct ParamSliderSpec {
    /// The visual-param id the slider writes.
    id: i32,
    /// Whether moving it invalidates the bake (a colour or alpha param) rather
    /// than only the shape.
    is_bake: bool,
    /// The bottom of the track.
    min: f32,
    /// The top of it.
    max: f32,
    /// Where the thumb starts — the worn value, or the param's default.
    value: f32,
    /// The row's label, from `avatar_lad` or prettified from the param name.
    label: String,
}

/// The editor-tweakable params of a slot, filtered by the avatar's sex and
/// excluding those the tint swatch already covers.
fn editable_params(
    library: &AvatarAssetLibrary,
    slot: WearableType,
    sex: ParamSex,
    tint_params: Option<[i32; 3]>,
) -> Vec<ParamSliderSpec> {
    let group = wearable_param_group(slot);
    library
        .params()
        .all()
        .iter()
        .filter(|param| param.wearable.as_deref() == Some(group))
        .filter(|param| {
            matches!(
                param.group,
                ParamGroup::Tweakable | ParamGroup::TweakableNoTransmit
            )
        })
        .filter(|param| param.sex == ParamSex::Both || param.sex == sex)
        .filter(|param| tint_params.is_none_or(|ids| !ids.contains(&param.id)))
        // The `male` param is presented as the dedicated gender toggle, not a
        // slider.
        .filter(|param| param.name != "male")
        .map(|param| {
            let is_bake = matches!(param.effect, ParamEffect::Color(_) | ParamEffect::Alpha);
            let label = param.label.clone().unwrap_or_else(|| prettify(&param.name));
            ParamSliderSpec {
                id: param.id,
                is_bake,
                min: param.min,
                max: param.max,
                value: param.default,
                label,
            }
        })
        .collect()
}

/// The avatar's sex, from the worn Shape's `male` param (`> 0.5` is male, the
/// reference viewer's `getVisualParamWeight("male")` rule); defaults to female
/// (the SL default body) when the shape or the param table is unknown.
///
/// The rule itself is [`shape_is_male`], shared with the RLV surface that
/// answers `@getdebug_avatarsex` — one avatar cannot be two sexes because two
/// call sites read the param differently.
fn avatar_sex(library: Option<&AvatarAssetLibrary>, shape: Option<&WearableAsset>) -> ParamSex {
    if shape_is_male(library, shape).unwrap_or(false) {
        ParamSex::Male
    } else {
        ParamSex::Female
    }
}

/// The current tint colour for a clothing layer, read from the three colour
/// params (each param weight is one linear channel, the reference swatch's
/// mapping).
fn tint_color(asset: &WearableAsset, [r, g, b]: [i32; 3]) -> Color {
    let channel = |id: i32| {
        asset
            .params
            .get(&id)
            .copied()
            .unwrap_or(1.0)
            .clamp(0.0, 1.0)
    };
    Color::srgb(channel(r), channel(g), channel(b))
}

// ---------------------------------------------------------------------------
// Control spawn helpers.
// ---------------------------------------------------------------------------

/// Spawn a labelled row (a left label plus a slot for the control) at
/// `font_size` and return the row entity to parent the control into.
fn spawn_labeled_row(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    font_size: f32,
) -> Entity {
    ui_spawn::spawn_labeled_row(
        commands,
        parent,
        LabeledRowSpec::new(UiLabel::literal(label))
            .label_color(LABEL_COLOR)
            .font_size(font_size)
            .label_width(Val::Px(120.0)),
    )
    .row
}

/// Spawn one param slider row at `font_size`: a label, then a slider over
/// `[min, max]` with its value readout and bounds, tagged [`WearParamSlider`].
fn spawn_param_slider(
    commands: &mut Commands,
    parent: Entity,
    spec: &ParamSliderSpec,
    tab: &mut i32,
    font_size: f32,
) {
    let &ParamSliderSpec {
        id,
        is_bake,
        min,
        max,
        value,
        ref label,
    } = spec;
    let row_entity = spawn_labeled_row(commands, parent, label, font_size);
    let range = SliderRange::new(min, max);
    let track = spawn_slider(
        commands,
        row_entity,
        SliderStyle {
            font_size,
            ..SLIDER
        },
        *tab,
        0.0,
        // The reference's `LLSliderCtrl` value field, read-only here.
        SliderLabels::beside(SliderFormat::Decimal(2)),
        (
            Slider::default(),
            SliderValue(value.clamp(min, max)),
            range,
            SliderStep((max - min) / 100.0),
            WearParamSlider { id, is_bake },
            Name::new(format!("wearable-slider:{id}")),
        ),
    );
    commands.entity(track).observe(on_wear_slider_change);
    *tab = tab.saturating_add(1);
}

/// Spawn one chrome action button (Save / Save As / Revert) at `font_size`.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    kind: WearButton,
    label: &str,
    tab: &mut i32,
    font_size: f32,
) {
    let button = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::literal(label), format!("wearable-button:{label}"))
            .kind(ButtonKind::Headless)
            .tab_from(tab)
            .padding(10.0, 3.0)
            .colors(BUTTON_BACKGROUND, CONTROL_BORDER)
            .label_color(LABEL_COLOR)
            .font_size(font_size),
    )
    .button;
    commands.entity(button).insert(kind).observe(on_wear_button);
}

// ---------------------------------------------------------------------------
// Edit handlers.
// ---------------------------------------------------------------------------

/// A param slider drag: clamp, write back the value, record the edit, and mark
/// the shape (or bake, for a colour / alpha param) dirty for the next preview.
fn on_wear_slider_change(
    change: On<ValueChange<f32>>,
    sliders: Query<(&WearParamSlider, &SliderRange)>,
    host: FloaterHost,
    mut edits: Query<&mut WearEdit>,
    mut commands: Commands,
) {
    let Ok((info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    // No edit only in the gallery, whose specimen has no window of its own.
    if let Some(mut edit) = host
        .of(change.source)
        .and_then(|window| edits.get_mut(window).ok())
    {
        let _prev = edit.edited.params.insert(info.id, clamped);
        edit.dirty = true;
        if info.is_bake {
            edit.bake_dirty = true;
        } else {
            edit.shape_dirty = true;
        }
    }
}

/// The gender radios whose selection changed this frame.
type ChangedGenderRadio = (With<WearGenderRadio>, Changed<RadioSelection>);

/// The Shape gender radio (Female = `0` / Male = `1`): write the `male` param
/// and re-shape the body when the selection changes.
fn apply_wear_gender_radio(
    radios: Query<(Entity, &RadioSelection), ChangedGenderRadio>,
    host: FloaterHost,
    mut edits: Query<&mut WearEdit>,
) {
    for (radio, selection) in &radios {
        let Some(mut edit) = host.of(radio).and_then(|window| edits.get_mut(window).ok()) else {
            continue;
        };
        let Some(id) = edit.gender_param else {
            continue;
        };
        let male = if selection.active >= 1 { 1.0 } else { 0.0 };
        if edit.edited.params.get(&id).copied() != Some(male) {
            let _prev = edit.edited.params.insert(id, male);
            edit.shape_dirty = true;
            edit.dirty = true;
        }
    }
}

/// A texture pick: record the new layer texture in the edit of the window the
/// swatch lives in, repaint the swatch, and mark the bake dirty.
fn apply_wear_texture_picked(
    mut picks: MessageReader<TexturePicked>,
    swatches: Query<&WearTextureSwatch>,
    mut values: Query<&mut TextureSwatchValue>,
    host: FloaterHost,
    mut edits: Query<&mut WearEdit>,
) {
    for pick in picks.read() {
        let Ok(slot) = swatches.get(pick.requester) else {
            continue;
        };
        let Some(mut edit) = host
            .of(pick.requester)
            .and_then(|window| edits.get_mut(window).ok())
        else {
            continue;
        };
        let _prev = edit.edited.textures.insert(
            u32::try_from(slot.0).unwrap_or_default(),
            pick.texture.uuid(),
        );
        edit.bake_dirty = true;
        edit.dirty = true;
        if let Ok(mut value) = values.get_mut(pick.requester) {
            value.0 = pick.texture;
        }
    }
}

/// A tint pick: write the colour into the slot's three colour params of the
/// window whose tint swatch asked, repaint the swatch, and mark the bake dirty.
fn apply_wear_tint_picked(
    mut picks: MessageReader<ColorPicked>,
    mut values: Query<&mut ColorSwatchValue, With<WearTintSwatch>>,
    host: FloaterHost,
    mut edits: Query<&mut WearEdit>,
) {
    for pick in picks.read() {
        let Some(mut edit) = host
            .of(pick.requester)
            .and_then(|window| edits.get_mut(window).ok())
        else {
            continue;
        };
        // The window's own tint swatch, not merely one inside it: a colour
        // picker answers every swatch that opened one.
        let (Some([r, g, b]), Some(tint_swatch)) = (edit.tint_params, edit.tint_swatch) else {
            continue;
        };
        if pick.requester != tint_swatch {
            continue;
        }
        let srgba = pick.color.to_srgba();
        let _prev = edit.edited.params.insert(r, srgba.red.clamp(0.0, 1.0));
        let _prev = edit.edited.params.insert(g, srgba.green.clamp(0.0, 1.0));
        let _prev = edit.edited.params.insert(b, srgba.blue.clamp(0.0, 1.0));
        edit.bake_dirty = true;
        edit.dirty = true;
        if let Ok(mut value) = values.get_mut(pick.requester) {
            value.0 = pick.color;
        }
    }
}

/// When an edited layer texture finishes decoding, re-mark the bake dirty so the
/// preview composite picks it up.
fn note_edited_texture_decoded(
    mut decoded: MessageReader<TextureDecoded>,
    mut edits: Query<&mut WearEdit>,
) {
    let keys: Vec<TextureKey> = decoded.read().map(|&TextureDecoded(key)| key).collect();
    if keys.is_empty() {
        return;
    }
    for mut edit in &mut edits {
        for key in &keys {
            if edit.pending_textures.remove(key) {
                edit.bake_dirty = true;
            }
        }
    }
}

/// Push each window's edited wearable into the bake inputs when something in
/// it changed, so the shape morph and bake composite re-derive from it (the
/// live preview).
fn drive_wearable_preview(
    mut edits: Query<&mut WearEdit>,
    mut preview: WearPreview,
    mut texts: Query<&mut Text>,
) {
    for mut edit in &mut edits {
        preview.show(&mut edit, &mut texts);
    }
}

// ---------------------------------------------------------------------------
// Save / Save-As / Revert.
// ---------------------------------------------------------------------------

/// A chrome button activated — by a primary click, `Enter` or `Space`: Save
/// (in place), Save As (new item), or Revert.
///
/// Save is not carried out here — it is written as a [`SaveEditorWindow`] for
/// [`save_wearable`], the same message the unsaved-work confirmation's "Save"
/// answer writes, so the button and the confirmation cannot come to save
/// different things.
fn on_wear_button(
    activate: On<Activate>,
    buttons: Query<&WearButton>,
    host: FloaterHost,
    mut edits: Query<&mut WearEdit>,
    mut preview: Option<WearPreview>,
    out: Option<WearSaveOut>,
    mut texts: Query<&mut Text>,
) {
    // Absent only in the gallery, whose specimen has no edit. The preview is
    // too, and only Revert reads it, so only Revert asks for it.
    let Some(out) = out else {
        return;
    };
    let WearSaveOut {
        mut pending,
        mut commands,
        mut saves,
    } = out;
    let Ok(kind) = buttons.get(activate.entity).copied() else {
        return;
    };
    let Some(window) = host.of(activate.entity) else {
        return;
    };
    let Ok(mut edit) = edits.get_mut(window) else {
        return;
    };
    let edit = &mut *edit;
    match kind {
        WearButton::Save => {
            saves.write(SaveEditorWindow { window });
        }
        WearButton::SaveAs => {
            let perms = wearable_permissions(&edit.item);
            let name = format!("{} (copy)", edit.item.name);
            let mut copy = edit.edited.clone();
            copy.name.clone_from(&name);
            let data = copy.to_text(&perms).into_bytes();
            let transaction = TransactionId::from(Uuid::new_v4());
            commands.write(SlCommand(new_wearable_command(
                edit.item.folder_id,
                edit.wearable_type,
                &name,
                data,
                transaction,
            )));
            // The copy has been *asked for*, not made: the bytes are on their way
            // and the grid has yet to say whether it kept them. Announcing the
            // copy here — which is what this did — is a claim about something
            // that has not happened, and a refused upload left it standing. The
            // transaction is registered with the viewer's one creation consumer;
            // `report_wearable_save_as` answers on what it publishes.
            pending.enqueue(transaction, &name);
            edit.creation = Some(transaction);
            set_status(&mut texts, edit.status, "Saving a copy…");
        }
        WearButton::Revert => {
            let Some(preview) = preview.as_mut() else {
                return;
            };
            edit.edited.clone_from(&edit.original);
            // Re-derive the preview from the restored asset.
            preview.restore(edit.item.item_id, &edit.original);
            edit.shape_dirty = true;
            // Back to what was opened on, so there is nothing unsaved left to
            // ask about on a close.
            edit.dirty = false;
            set_status(&mut texts, edit.status, "Reverted.");
        }
    }
}

/// Where a save leaves, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the creation registry a
/// Save As's transaction goes into, the session the copy goes out on, and the
/// in-place save request the Save button raises.
#[derive(bevy::ecs::system::SystemParam)]
struct WearSaveOut<'w> {
    /// The creation registry a Save As's transaction goes into, so its answer
    /// is published for this editor to read.
    pending: ResMut<'w, PendingItemCreations>,
    /// The session a Save As's copy goes out on.
    commands: MessageWriter<'w, SlCommand>,
    /// The in-place save request, which is also what the unsaved-work
    /// confirmation's "Save" answer writes.
    saves: MessageWriter<'w, SaveEditorWindow>,
}

/// What the worn preview is re-derived through, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the bake inputs, the
/// texture manager and decoded store the asset's textures are fetched and read
/// from, the avatar-param library the layers are reassembled against, and the
/// local bake the result invalidates.
///
/// Revert and a close-without-saving both put the *original* asset back on the
/// avatar, so both go through [`WearPreview::restore`] rather than repeating
/// the four calls.
#[derive(bevy::ecs::system::SystemParam)]
struct WearPreview<'w> {
    /// The bake inputs the preview asset is set on.
    inputs: ResMut<'w, OwnBakeInputs>,
    /// The texture manager the asset's textures are requested through.
    texture_manager: ResMut<'w, TextureManager>,
    /// The decoded textures the reassembly reads.
    store: Res<'w, DecodedTextures>,
    /// The avatar-param library the layers are reassembled against.
    library: Option<Res<'w, AvatarAssetLibrary>>,
    /// The local bake, invalidated so the next frame re-bakes.
    local_bake: ResMut<'w, OwnLocalBake>,
}

impl WearPreview<'_> {
    /// Show one window's edit on the avatar when something in it changed: the
    /// shape re-derives from the substituted asset, the Shape height read-out
    /// follows, and a texture or tint change re-composites the bake.
    fn show(&mut self, edit: &mut WearEdit, texts: &mut Query<&mut Text>) {
        if !edit.shape_dirty && !edit.bake_dirty {
            return;
        }
        // The shape morph re-derives from the substituted asset; the bake needs
        // an explicit texture request + reassemble + rebuild.
        self.inputs
            .set_preview_asset(edit.item.item_id, edit.edited.clone());
        // Update the Shape height read-out from the edited body proportions
        // (the reference's `computeBodySize` `mBodySize.z`).
        if edit.shape_dirty
            && edit.wearable_type == WearableType::Shape
            && let (Some(library), Some(label)) = (self.library.as_deref(), edit.height_label)
        {
            let bytes = self.inputs.visual_params(library.params());
            let resolved = ResolvedParams::from_appearance(library.params(), &bytes);
            let deform = SkeletalDeformations::from_resolved(library.params(), &resolved);
            if let Some(metrics) = library
                .skeleton()
                .body_size_metrics(&deform, &JointOverrides::default())
                && let Ok(mut text) = texts.get_mut(label)
            {
                text.0 = format!("{:.2} m", metrics.body_size_z);
            }
        }
        if edit.bake_dirty {
            self.inputs.request_asset_textures(
                &edit.edited,
                &mut self.texture_manager,
                &self.store,
            );
            // Track edited textures still decoding so the composite re-runs
            // once they land.
            edit.pending_textures = edit
                .edited
                .textures
                .values()
                .copied()
                .filter(|id| !id.is_nil())
                .map(TextureKey::from)
                .filter(|key| self.store.get(*key).is_none())
                .collect();
            self.inputs.reassemble(&self.store, self.library.as_deref());
            self.local_bake.invalidate();
        }
        edit.shape_dirty = false;
        edit.bake_dirty = false;
    }

    /// Re-derive the worn preview of `item` from `original`, the asset its
    /// editor was opened on (or last saved).
    fn restore(&mut self, item: InventoryKey, original: &WearableAsset) {
        self.inputs.set_preview_asset(item, original.clone());
        self.inputs
            .request_asset_textures(original, &mut self.texture_manager, &self.store);
        self.inputs.reassemble(&self.store, self.library.as_deref());
        self.local_bake.invalidate();
    }
}

/// Write the edit back onto the **same** item over the legacy transaction
/// upload, and remember the transaction so the completion can be told from
/// somebody else's.
fn save_wearable(
    mut requests: MessageReader<SaveEditorWindow>,
    mut edits: Query<&mut WearEdit>,
    mut commands: MessageWriter<SlCommand>,
    mut texts: Query<&mut Text>,
) {
    for request in requests.read() {
        // Another editor's window, or one closed since the request was raised.
        let Ok(mut edit) = edits.get_mut(request.window) else {
            continue;
        };
        let perms = wearable_permissions(&edit.item);
        let data = edit.edited.to_text(&perms).into_bytes();
        let transaction_id = TransactionId::from(Uuid::new_v4());
        commands.write(SlCommand(Command::SaveInventoryAsset {
            item: Box::new(to_wire_item(&edit.item)),
            asset_type: asset_type_of(edit.wearable_type),
            transaction_id,
            data,
        }));
        edit.saving = Some(transaction_id);
        set_status(&mut texts, edit.status, "Saving…");
    }
}

/// Report an in-place Save's outcome from the [`InventoryAssetSaved`] reply that
/// names **this** editor's transaction.
fn report_wearable_save(
    mut events: MessageReader<SlEvent>,
    mut edits: Query<&mut WearEdit>,
    mut inventory: Option<ResMut<InventoryModel>>,
    mut texts: Query<&mut Text>,
) {
    for event in events.read() {
        let SlSessionEvent::InventoryAssetSaved {
            transaction_id,
            asset_id,
            success,
        } = &event.0
        else {
            continue;
        };
        // Ours only if it names the transaction this window minted. Without the
        // check, any save completing while this one is in flight — another
        // window's, or an earlier save of this one that timed out — is reported
        // here as this save's result.
        let Some(mut edit) = edits
            .iter_mut()
            .find(|edit| edit.saving.is_some() && *transaction_id == edit.saving)
        else {
            continue;
        };
        edit.saving = None;
        if *success {
            // Stored, so the edit *is* what the item holds now: nothing unsaved
            // is left, a Revert should go back to this rather than to what was
            // opened on, and a close waiting on the save may go ahead.
            let saved = edit.edited.clone();
            edit.original = saved;
            edit.dirty = false;
            // The save stored a new asset and the grid rebound the item to it;
            // follow it, so what reads the item's asset reads the saved one.
            edit.item.asset_id = *asset_id;
            if let Some(model) = inventory.as_mut() {
                let _folder = model.rebind_asset(edit.item.item_id, *asset_id);
            }
        }
        let message = if *success { "Saved." } else { "Save failed." };
        set_status(&mut texts, edit.status, message);
    }
}

/// Report a **Save As** landing — or failing — on the transaction this editor
/// registered with the shared creation consumer.
fn report_wearable_save_as(
    mut finished: MessageReader<ItemCreationFinished>,
    mut edits: Query<&mut WearEdit>,
    mut texts: Query<&mut Text>,
) {
    for creation in finished.read() {
        let Some(mut edit) = edits
            .iter_mut()
            .find(|edit| edit.creation == Some(creation.transaction))
        else {
            continue;
        };
        edit.creation = None;
        let message = if creation.item.is_some() {
            "Saved a copy to inventory."
        } else {
            "Saving a copy failed."
        };
        set_status(&mut texts, edit.status, message);
    }
}

/// End the edit when its window closes: put back the worn asset the editor
/// opened on (or last saved).
///
/// Closing is the one answer the guard does not stop — either the resident
/// saved first (in which case the restored asset *is* what they saved), or they
/// said the work could go. Either way the preview must not outlive the window:
/// [`OwnBakeInputs::set_preview_asset`] substitutes the edit into the worn
/// outfit, so leaving it in place keeps the avatar wearing an unsaved edit with
/// no window left to save, revert or even see it in — and the next open would
/// read that preview back as if it were what is worn.
///
/// An observer on the edit's removal, since closing a keyed window despawns it:
/// there is no hidden window left for a system to notice. It restores only this
/// window's item, so another window's edit stays on the avatar.
fn restore_preview_on_close(
    removed: On<Remove, WearEdit>,
    edits: Query<&WearEdit>,
    mut preview: WearPreview,
) {
    let Ok(edit) = edits.get(removed.entity) else {
        return;
    };
    preview.restore(edit.item.item_id, &edit.original);
}

/// Keep each window's close guard in step with its edit: while the wearable on
/// screen differs from what was opened on (or last saved), closing the window
/// would throw that away, and the scaffold asks first.
fn track_wearable_unsaved(mut windows: Query<(&WearEdit, &mut UnsavedWork)>) {
    for (edit, mut work) in &mut windows {
        if work.dirty != edit.dirty {
            work.dirty = edit.dirty;
        }
    }
}

/// Scroll the control list with the wheel while the pointer is over it.
fn scroll_wearable_list(
    wheel: Res<AccumulatedMouseScroll>,
    hover: Res<HoverMap>,
    lists: Query<Entity, With<WearScrollList>>,
    parents: Query<&ChildOf>,
    mut positions: Query<&mut ScrollPosition>,
) {
    if wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    let delta = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * LINE_SCROLL_PIXELS,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    for list in &lists {
        let over = hover.values().flat_map(|hits| hits.keys()).any(|hovered| {
            let mut node = *hovered;
            loop {
                if node == list {
                    return true;
                }
                match parents.get(node) {
                    Ok(parent) => node = parent.parent(),
                    Err(_root) => return false,
                }
            }
        });
        if over && let Ok(mut position) = positions.get_mut(list) {
            position.0.y = (position.0.y - delta).max(0.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// The asset class for a wearable slot (body parts save as `Bodypart`,
/// everything else as `Clothing`).
const fn asset_type_of(slot: WearableType) -> AssetType {
    if slot.is_body_part() {
        AssetType::Bodypart
    } else {
        AssetType::Clothing
    }
}

/// Build the `.wearable` header permissions block from the inventory item (the
/// reference authors the export from the item's `LLPermissions`).
fn wearable_permissions(item: &ItemInfo) -> WearablePermissions {
    let (sale_type, sale_price) = (
        map_sale_type(item.sale.sale_type),
        i32::try_from(item.sale.price.0).unwrap_or_default(),
    );
    WearablePermissions {
        base_mask: item.permissions.base.bits(),
        owner_mask: item.permissions.owner.bits(),
        group_mask: item.permissions.group.bits(),
        everyone_mask: item.permissions.everyone.bits(),
        next_owner_mask: item.permissions.next_owner.bits(),
        creator_id: item.creator_id.uuid(),
        owner_id: item.owner.uuid(),
        last_owner_id: item.last_owner_id,
        group_id: item.group.map_or_else(Uuid::nil, |group| group.uuid()),
        sale_type,
        sale_price,
    }
}

/// Map the inventory sale type onto the wearable-asset `sale_type` keyword.
const fn map_sale_type(sale: SaleType) -> WearableSaleType {
    match sale {
        SaleType::Original => WearableSaleType::Original,
        SaleType::Copy => WearableSaleType::Copy,
        SaleType::Contents => WearableSaleType::Contents,
        // `NotForSale` and any future variant author `not`.
        _other => WearableSaleType::Not,
    }
}

/// Write a one-line status message into the editor's readout.
fn set_status(texts: &mut Query<&mut Text>, node: Option<Entity>, message: &str) {
    if let Some(node) = node
        && let Ok(mut text) = texts.get_mut(node)
    {
        message.clone_into(&mut text.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{WearEdit, report_wearable_save, report_wearable_save_as};
    use crate::intents::PendingItemCreations;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, AssetType, InventoryFolderKey, InventoryKey, InventoryType, ItemInfo, OwnerKey,
        Permissions, Permissions5, SlEvent, SlSessionEvent, TransactionId, Uuid, WearableAsset,
        WearableType,
    };
    use std::collections::{BTreeMap, HashSet};

    /// A boxed error so tests can use `?` rather than the disallowed `unwrap` /
    /// `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The worn wearable item `id` of `slot`.
    fn wearable(id: u128, slot: WearableType) -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(id)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x22)),
            name: format!("Wearable {id:#x}"),
            description: String::new(),
            asset_id: Uuid::from_u128(id << 8),
            asset_type: if slot.is_body_part() {
                AssetType::Bodypart
            } else {
                AssetType::Clothing
            },
            inv_type: InventoryType::Wearable,
            flags: u32::from(slot.to_code()),
            sale: sl_client_bevy::SaleInfo::default(),
            creation_date: 0,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0x44))),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::from_u128(0x44)),
            group: None,
            permissions: Permissions5 {
                base: Permissions::from_bits(0x7fff_ffff),
                owner: Permissions::from_bits(0x7fff_ffff),
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::from_bits(0x0008_2000),
            },
        }
    }

    /// The shirt the editor is editing.
    fn shirt() -> ItemInfo {
        let mut shirt = wearable(0x11, WearableType::Shirt);
        "A shirt".clone_into(&mut shirt.name);
        shirt
    }

    /// An empty asset of `slot` named `name`.
    fn asset(name: &str, slot: WearableType) -> WearableAsset {
        WearableAsset {
            version: super::WEARABLE_VERSION,
            name: name.to_owned(),
            wearable_type: slot,
            params: BTreeMap::new(),
            textures: BTreeMap::new(),
        }
    }

    /// An editor window with an open, modified edit and a status readout, plus
    /// the systems that report a save's outcome: the app, the window and the
    /// status node.
    fn editor_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_message::<SlEvent>()
            .add_message::<crate::intents::ItemCreationFinished>()
            .init_resource::<PendingItemCreations>()
            .add_systems(Update, (report_wearable_save, report_wearable_save_as));
        let status = app.world_mut().spawn(Text::new(String::new())).id();
        let window = app.world_mut().spawn(open_edit(status)).id();
        app.update();
        (app, window, status)
    }

    /// An open, modified edit of [`shirt`] reporting to `status`.
    fn open_edit(status: Entity) -> WearEdit {
        let asset = asset("A shirt", WearableType::Shirt);
        WearEdit {
            item: shirt(),
            wearable_type: WearableType::Shirt,
            original: asset.clone(),
            edited: asset,
            tint_params: None,
            tint_swatch: None,
            gender_param: None,
            height_label: None,
            pending_textures: HashSet::new(),
            shape_dirty: false,
            bake_dirty: false,
            saving: None,
            creation: None,
            dirty: true,
            status: Some(status),
        }
    }

    /// The edit in `window`, to change.
    fn edit_mut(app: &mut App, window: Entity) -> Result<Mut<'_, WearEdit>, TestError> {
        app.world_mut()
            .get_mut::<WearEdit>(window)
            .ok_or_else(|| TestError::from("the window has no edit"))
    }

    /// The edit in `window`.
    fn edit_of(app: &App, window: Entity) -> Result<&WearEdit, TestError> {
        app.world()
            .get::<WearEdit>(window)
            .ok_or_else(|| TestError::from("the window has no edit"))
    }

    /// What the status readout says.
    fn status_of(app: &App, status: Entity) -> String {
        app.world()
            .get::<Text>(status)
            .map(|text| text.0.clone())
            .unwrap_or_default()
    }

    /// A completion that names **another** transaction is not this editor's
    /// result.
    ///
    /// This is the defect the correlation exists for: the editor used to take
    /// any `InventoryAssetSaved` arriving while it was saving as its own, so an
    /// unrelated save — an earlier one of its own that timed out, another
    /// surface's — decided what it told the resident about theirs.
    #[test]
    fn a_foreign_completion_is_not_this_save() -> Result<(), TestError> {
        let (mut app, window, status) = editor_app();
        let ours = TransactionId::from(Uuid::from_u128(0xAA));
        let theirs = TransactionId::from(Uuid::from_u128(0xBB));
        edit_mut(&mut app, window)?.saving = Some(ours);

        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::InventoryAssetSaved {
                transaction_id: Some(theirs),
                asset_id: Uuid::from_u128(0xB0),
                success: false,
            }));
        app.update();
        assert_eq!(
            status_of(&app, status),
            "",
            "another save's failure was reported as this editor's"
        );
        assert_eq!(
            edit_of(&app, window)?.saving,
            Some(ours),
            "another save's completion cleared this editor's in-flight save"
        );

        // Ours lands, and it is ours that is reported.
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::InventoryAssetSaved {
                transaction_id: Some(ours),
                asset_id: Uuid::from_u128(0xA0),
                success: true,
            }));
        app.update();
        assert_eq!(status_of(&app, status), "Saved.");
        let edit = edit_of(&app, window)?;
        assert_eq!(edit.saving, None, "the save is no longer in flight");
        assert!(
            !edit.dirty,
            "a stored edit is no longer unsaved work to ask about"
        );
        assert_eq!(
            edit.item.asset_id,
            Uuid::from_u128(0xA0),
            "the window still names the asset the item had before the save"
        );
        Ok(())
    }

    /// A refused save leaves the work unsaved, so a close still asks and a
    /// Revert still has somewhere to go back to.
    #[test]
    fn a_refused_save_leaves_the_work_unsaved() -> Result<(), TestError> {
        let (mut app, window, status) = editor_app();
        let ours = TransactionId::from(Uuid::from_u128(0xCC));
        edit_mut(&mut app, window)?.saving = Some(ours);
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::InventoryAssetSaved {
                transaction_id: Some(ours),
                asset_id: Uuid::from_u128(0xC0),
                success: false,
            }));
        app.update();
        assert_eq!(status_of(&app, status), "Save failed.");
        assert!(
            edit_of(&app, window)?.dirty,
            "a refused save must not clear the unsaved work"
        );
        Ok(())
    }

    /// **Save As** reports the copy when the grid has actually made one — and
    /// says so when it has not.
    ///
    /// The editor used to announce "Saved a copy to inventory." the instant the
    /// bytes were queued, which is a claim about something that had not happened
    /// yet; a refused upload left that claim standing.
    #[test]
    fn save_as_reports_the_copy_it_actually_got() -> Result<(), TestError> {
        let (mut app, window, status) = editor_app();
        let ours = TransactionId::from(Uuid::from_u128(0x22));
        let other = TransactionId::from(Uuid::from_u128(0x99));
        edit_mut(&mut app, window)?.creation = Some(ours);

        // Somebody else's creation is not this Save As.
        app.world_mut()
            .write_message(crate::intents::ItemCreationFinished {
                transaction: other,
                item: Some(InventoryKey::from(Uuid::from_u128(0xEE))),
            });
        app.update();
        assert_eq!(
            status_of(&app, status),
            "",
            "another creation was reported as this editor's copy"
        );

        app.world_mut()
            .write_message(crate::intents::ItemCreationFinished {
                transaction: ours,
                item: Some(InventoryKey::from(Uuid::from_u128(0xFF))),
            });
        app.update();
        assert_eq!(status_of(&app, status), "Saved a copy to inventory.");
        Ok(())
    }

    /// A Save As the grid refuses says so, instead of leaving a success on
    /// screen.
    #[test]
    fn a_refused_save_as_says_so() -> Result<(), TestError> {
        let (mut app, window, status) = editor_app();
        let ours = TransactionId::from(Uuid::from_u128(0x22));
        edit_mut(&mut app, window)?.creation = Some(ours);
        app.world_mut()
            .write_message(crate::intents::ItemCreationFinished {
                transaction: ours,
                item: None,
            });
        app.update();
        assert_eq!(status_of(&app, status), "Saving a copy failed.");
        Ok(())
    }

    /// **Save and Save As act from the keyboard as from the mouse.**
    ///
    /// The chrome buttons used to observe `Pointer<Press>`, so `Tab` reached
    /// them and `Enter` / `Space` did nothing (`viewer-floater-buttons-ignore-
    /// keyboard-activate`). Each gesture, in a fresh app under the real input
    /// and focus stack, must do exactly what the click does. The buttons sit in
    /// an editor window, as they do live, since each reaches its edit through
    /// the window it lives in.
    #[test]
    fn the_save_buttons_answer_enter_space_and_a_click() -> Result<(), TestError> {
        use super::{WearButton, spawn_action_button, wearable_editor_floater_spec};
        use crate::asset_editor::SaveEditorWindow;
        use crate::floater::{FloaterPlugin, spawn_floater};
        use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems};
        use bevy::input::keyboard::Key;
        use sl_client_bevy::{Command, SlCommand};
        use sl_viewer_testkit::interact::{self, InteractionTest};
        use sl_viewer_testkit::{drain, find_by_name, record, settle};

        /// One way of pressing a button: a click, or a key on the focused one.
        type Gesture = fn(&mut App, Entity) -> Result<(), String>;

        /// What one gesture on the button named `node` left behind: the
        /// windows asked to save, the copies sent under the transaction the
        /// edit holds and registered, and the status line.
        type Outcome = (Vec<Entity>, usize, String);

        /// A primary click on the button's centre.
        fn click(app: &mut App, button: Entity) -> Result<(), String> {
            let at = interact::centre_of_entity(app, button).ok_or("the button has no box")?;
            interact::click(app, at, MouseButton::Left);
            Ok(())
        }

        /// `Enter` on the focused button.
        fn enter(app: &mut App, button: Entity) -> Result<(), String> {
            interact::focus(app, button);
            interact::tap(app, KeyCode::Enter, Key::Enter);
            Ok(())
        }

        /// `Space` on the focused button.
        fn space(app: &mut App, button: Entity) -> Result<(), String> {
            interact::focus(app, button);
            interact::tap(app, KeyCode::Space, Key::Space);
            Ok(())
        }

        /// The editor's window, and what `gesture` on the button named `node`
        /// did to its open edit.
        fn after(node: &str, gesture: Gesture) -> Result<(Entity, Outcome), TestError> {
            let mut app = InteractionTest::new().build();
            app.init_resource::<PendingItemCreations>()
                .add_plugins(FloaterPlugin);
            record::<SaveEditorWindow>(&mut app);
            record::<SlCommand>(&mut app);
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    let window =
                        spawn_floater(&mut commands, root.0, wearable_editor_floater_spec());
                    // A floater spawns hidden and its opener shows it; a hidden
                    // window has no box for a pointer to find.
                    commands.entity(window.root).insert(UiPanelShown(true));
                    let mut tab = 0_i32;
                    for (kind, label) in
                        [(WearButton::Save, "Save"), (WearButton::SaveAs, "Save As")]
                    {
                        spawn_action_button(
                            &mut commands,
                            window.content,
                            kind,
                            label,
                            &mut tab,
                            13.0,
                        );
                    }
                    let status = commands
                        .spawn((Text::new(String::new()), ChildOf(window.content)))
                        .id();
                    commands.entity(window.root).insert(open_edit(status));
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            let (window, status) = app
                .world_mut()
                .query::<(Entity, &WearEdit)>()
                .iter(app.world())
                .next()
                .and_then(|(window, edit)| Some((window, edit.status?)))
                .ok_or("the edit was never opened")?;
            let button = find_by_name(&mut app, node).ok_or_else(|| format!("no `{node}`"))?;
            gesture(&mut app, button)?;
            settle(&mut app);
            let saves = drain::<SaveEditorWindow>(&mut app)
                .into_iter()
                .map(|save| save.window)
                .collect();
            // A copy counts only when it names the transaction the edit holds,
            // and that transaction is the one registered for its answer.
            let creation = edit_of(&app, window)?.creation;
            let registered = creation.is_some_and(|transaction| {
                app.world_mut()
                    .resource_mut::<PendingItemCreations>()
                    .take(transaction)
                    .is_some()
            });
            let copies = drain::<SlCommand>(&mut app)
                .into_iter()
                .filter(|command| match &command.0 {
                    Command::CreateInventoryItemWithAsset { item, .. } => {
                        registered
                            && creation == Some(TransactionId::from(item.transaction_id))
                            && item.name.ends_with(" (copy)")
                    }
                    _other => false,
                })
                .count();
            Ok((window, (saves, copies, status_of(&app, status))))
        }

        let gestures: [(&str, Gesture); 3] = [("click", click), ("Enter", enter), ("Space", space)];
        for (how, gesture) in gestures {
            let (window, saved) = after("wearable-button:Save", gesture)?;
            assert_eq!(
                saved,
                (vec![window], 0, String::new()),
                "Save by {how} asks for its window to be saved, once"
            );
            let (_window, copied) = after("wearable-button:Save As", gesture)?;
            assert_eq!(
                copied,
                (Vec::new(), 1, "Saving a copy…".to_owned()),
                "Save As by {how} sends one copy"
            );
        }
        Ok(())
    }

    /// **One window per wearable** (`viewer-key-wearable-editor`): the open
    /// path, driven by the message an inventory row's Edit writes, and the
    /// per-window passes behind it.
    ///
    /// What the keying buys is not two windows for their own sake — it is that
    /// opening a second wearable cannot touch the first window's **unsaved
    /// edit**, which the singleton editor replaced on every open.
    mod instances {
        use super::super::{
            WearEdit, WearTintSwatch, apply_wear_tint_picked, open_wearable_editor,
            report_wearable_save, resolve_wearable, restore_preview_on_close, save_wearable,
            wearable_key,
        };
        use super::{TestError, asset, edit_mut, edit_of, wearable};
        use crate::asset_editor::SaveEditorWindow;
        use crate::avatars::OwnLocalBake;
        use crate::bake_inputs::OwnBakeInputs;
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin, host_floater};
        use crate::intents::TexturePicked;
        use crate::inventory::{InventoryModel, OpenWearableEditor};
        use crate::textures::TextureManager;
        use crate::ui::UiRoot;
        use crate::ui_color_picker::ColorPicked;
        use crate::world_api::DecodedTextures;
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            ASSET_CODE_LINK, AssetType, Command, InventoryFolderKey, InventoryKey, SlCommand,
            SlEvent, SlSessionEvent, Uuid, WearableType,
        };

        /// The shirt's item id.
        const SHIRT: u128 = 0xA1;

        /// The pants' item id.
        const PANTS: u128 = 0xB2;

        /// The upper of two layered shirts' item id ([`SHIRT`] the lower).
        const UPPER: u128 = 0xC3;

        /// An app with the floater manager, the open path, the close restore
        /// and the per-window passes that need no live session: the tint pick,
        /// the save and its report.
        fn editor_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<OpenWearableEditor>()
                .add_message::<ColorPicked>()
                .add_message::<TexturePicked>()
                .add_message::<SaveEditorWindow>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<OwnBakeInputs>()
                .init_resource::<TextureManager>()
                .init_resource::<DecodedTextures>()
                .init_resource::<OwnLocalBake>()
                .add_plugins(FloaterPlugin)
                .add_observer(restore_preview_on_close)
                .add_systems(
                    Update,
                    (
                        open_wearable_editor,
                        apply_wear_tint_picked,
                        save_wearable,
                        report_wearable_save,
                    )
                        .chain(),
                );
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// The item id `id` stands for.
        fn key(id: u128) -> InventoryKey {
            InventoryKey::from(Uuid::from_u128(id))
        }

        /// Open wearable `id` of `slot` the way the inventory's Edit does — by
        /// writing the message.
        fn open(app: &mut App, id: u128, slot: WearableType) {
            app.world_mut().write_message(OpenWearableEditor {
                item: wearable(id, slot),
            });
            app.update();
        }

        /// Every live wearable window, as (window, item id) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, InventoryKey)> {
            app.world_mut()
                .query::<(Entity, &WearEdit)>()
                .iter(app.world())
                .map(|(window, edit)| (window, edit.item.item_id))
                .collect()
        }

        /// The window editing wearable `id`.
        fn window_of(app: &mut App, id: u128) -> Result<Entity, TestError> {
            windows(app)
                .into_iter()
                .find_map(|(window, item)| (item == key(id)).then_some(window))
                .ok_or_else(|| TestError::from(format!("wearable {id:#x} has no window")))
        }

        /// The parent links and windows `host_floater` walks up through.
        type HostLookup<'world, 'state> = (
            Query<'world, 'state, &'static ChildOf>,
            Query<'world, 'state, (Entity, &'static Floater)>,
        );

        /// The tint swatch built into `window`.
        fn tint_swatch(app: &mut App, window: Entity) -> Result<Entity, TestError> {
            let swatches: Vec<Entity> = app
                .world_mut()
                .query_filtered::<Entity, With<WearTintSwatch>>()
                .iter(app.world())
                .collect();
            let mut system_state: bevy::ecs::system::SystemState<HostLookup<'_, '_>> =
                bevy::ecs::system::SystemState::new(app.world_mut());
            let (parents, floaters) = system_state.get(app.world())?;
            swatches
                .into_iter()
                .find(|swatch| host_floater(*swatch, &parents, &floaters) == Some(window))
                .ok_or_else(|| TestError::from("the window has no tint swatch"))
        }

        /// The name of the worn asset the bake inputs hold for `id`.
        fn worn_name(app: &App, id: u128) -> Option<String> {
            app.world()
                .resource::<OwnBakeInputs>()
                .worn_asset_of(key(id))
                .map(|asset| asset.name.clone())
        }

        /// Two wearables are two windows, each on its own item, with its own
        /// title, under its own key.
        #[test]
        fn two_wearables_open_two_windows() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, SHIRT, WearableType::Shirt);
            open(&mut app, PANTS, WearableType::Pants);

            assert_eq!(
                windows(&mut app).len(),
                2,
                "the second wearable reused the first window"
            );
            let shirt = window_of(&mut app, SHIRT)?;
            let pants = window_of(&mut app, PANTS)?;
            let key_of = |window: Entity| {
                app.world()
                    .get::<Floater>(window)
                    .and_then(Floater::key)
                    .cloned()
            };
            assert_eq!(key_of(shirt), Some(wearable_key(key(SHIRT))));
            assert_eq!(key_of(pants), Some(wearable_key(key(PANTS))));
            Ok(())
        }

        /// **Re-opening a wearable that is already up keeps its edit.** A
        /// second open raises the window; it must not re-seed the edit from the
        /// worn asset over what the resident changed and has not saved.
        #[test]
        fn reopening_a_wearable_keeps_its_unsaved_edit() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, SHIRT, WearableType::Shirt);
            let window = window_of(&mut app, SHIRT)?;
            {
                let mut edit = edit_mut(&mut app, window)?;
                let _prev = edit.edited.params.insert(600, 0.25);
                edit.dirty = true;
            }

            open(&mut app, SHIRT, WearableType::Shirt);
            assert_eq!(windows(&mut app).len(), 1, "a re-open spawned a window");
            let edit = edit_of(&app, window)?;
            assert_eq!(
                (edit.edited.params.get(&600).copied(), edit.dirty),
                (Some(0.25), true),
                "a re-open re-seeded the wearable over the unsaved edit"
            );
            Ok(())
        }

        /// A window is seeded from **its own item's** worn asset — not from
        /// whichever wearable of its type the outfit lists first, which is what
        /// a type lookup answered for the second of two layered shirts.
        #[test]
        fn a_window_seeds_from_its_own_item() -> Result<(), TestError> {
            let mut app = editor_app();
            {
                let mut inputs = app.world_mut().resource_mut::<OwnBakeInputs>();
                inputs.set_preview_asset(key(SHIRT), asset("lower shirt", WearableType::Shirt));
                inputs.set_preview_asset(key(UPPER), asset("upper shirt", WearableType::Shirt));
            }
            open(&mut app, UPPER, WearableType::Shirt);
            let window = window_of(&mut app, UPPER)?;
            assert_eq!(edit_of(&app, window)?.edited.name, "upper shirt");
            Ok(())
        }

        /// A Current Outfit Folder link opens the wearable it links to: one
        /// window for the link row and the item's own row, and a Save that
        /// writes onto the wearable rather than the link.
        #[test]
        fn a_link_row_opens_the_item_it_links_to() {
            let target = wearable(SHIRT, WearableType::Shirt);
            let mut link = wearable(0xD4, WearableType::Shirt);
            link.asset_type = AssetType::Other(ASSET_CODE_LINK);
            link.asset_id = target.item_id.uuid();
            let mut model = InventoryModel::default();
            model.set_items(
                InventoryFolderKey::from(Uuid::from_u128(0x22)),
                core::slice::from_ref(&target),
            );
            assert_eq!(
                resolve_wearable(&link, Some(&model)).item_id,
                target.item_id
            );
            assert_eq!(
                resolve_wearable(&target, Some(&model)).item_id,
                target.item_id,
                "a wearable resolves to itself"
            );
            assert_eq!(
                resolve_wearable(&link, None).item_id,
                link.item_id,
                "an unresolvable link opens as it is"
            );
        }

        /// **Closing one window puts back only its own item.** Both edits are
        /// previewing on the avatar; closing the shirt's window restores the
        /// shirt as it was opened and leaves the pants' unsaved edit on.
        #[test]
        fn closing_one_wearable_restores_only_its_own() -> Result<(), TestError> {
            let mut app = editor_app();
            {
                let mut inputs = app.world_mut().resource_mut::<OwnBakeInputs>();
                inputs.set_preview_asset(key(SHIRT), asset("shirt", WearableType::Shirt));
                inputs.set_preview_asset(key(PANTS), asset("pants", WearableType::Pants));
            }
            open(&mut app, SHIRT, WearableType::Shirt);
            open(&mut app, PANTS, WearableType::Pants);
            let shirt = window_of(&mut app, SHIRT)?;
            {
                // Both windows' edits on the avatar, as the preview pass puts
                // them there.
                let mut inputs = app.world_mut().resource_mut::<OwnBakeInputs>();
                inputs.set_preview_asset(key(SHIRT), asset("shirt, edited", WearableType::Shirt));
                inputs.set_preview_asset(key(PANTS), asset("pants, edited", WearableType::Pants));
            }

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: shirt,
                    op: FloaterOp::CloseNow,
                });
            app.update();

            assert_eq!(
                windows(&mut app)
                    .into_iter()
                    .map(|(_window, item)| item)
                    .collect::<Vec<_>>(),
                vec![key(PANTS)],
                "closing one window took the other with it"
            );
            assert_eq!(
                worn_name(&app, SHIRT).as_deref(),
                Some("shirt"),
                "the closed window's edit stayed on the avatar"
            );
            assert_eq!(
                worn_name(&app, PANTS).as_deref(),
                Some("pants, edited"),
                "closing the shirt took the pants' edit off the avatar"
            );
            Ok(())
        }

        /// **A tint pick lands in the window whose swatch asked for it.** The
        /// reply names a swatch; the swatch's window is the edit it writes, and
        /// the other window's colour is untouched.
        #[test]
        fn a_tint_pick_changes_only_its_own_window() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, SHIRT, WearableType::Shirt);
            open(&mut app, PANTS, WearableType::Pants);
            let shirt = window_of(&mut app, SHIRT)?;
            let pants = window_of(&mut app, PANTS)?;
            let untouched = edit_of(&app, shirt)?.edited.params.clone();

            let swatch = tint_swatch(&mut app, pants)?;
            app.world_mut().write_message(ColorPicked {
                requester: swatch,
                color: Color::srgb(1.0, 0.0, 0.0),
                final_pick: true,
            });
            app.update();

            let [red, green, blue] = edit_of(&app, pants)?
                .tint_params
                .ok_or("pants have no tint")?;
            let params = &edit_of(&app, pants)?.edited.params;
            assert_eq!(
                [red, green, blue].map(|id| params.get(&id).copied()),
                [Some(1.0), Some(0.0), Some(0.0)]
            );
            assert_eq!(
                edit_of(&app, shirt)?.edited.params,
                untouched,
                "a pick in one window changed the other's wearable"
            );
            assert!(edit_of(&app, pants)?.dirty && !edit_of(&app, shirt)?.dirty);
            Ok(())
        }

        /// **A save goes out for, and is reported in, the window that
        /// saved.** Save on the shirt writes the shirt; the reply lands in the
        /// shirt's window and the pants' window has no save to hear about.
        #[test]
        fn a_save_lands_in_the_window_that_saved() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, SHIRT, WearableType::Shirt);
            open(&mut app, PANTS, WearableType::Pants);
            let shirt = window_of(&mut app, SHIRT)?;
            let pants = window_of(&mut app, PANTS)?;
            edit_mut(&mut app, shirt)?.dirty = true;
            let _drained = app
                .world_mut()
                .resource_mut::<Messages<SlCommand>>()
                .drain()
                .count();

            app.world_mut()
                .write_message(SaveEditorWindow { window: shirt });
            app.update();
            let saved: Vec<InventoryKey> = app
                .world_mut()
                .resource_mut::<Messages<SlCommand>>()
                .drain()
                .filter_map(|command| match command.0 {
                    Command::SaveInventoryAsset { item, .. } => Some(item.item_id),
                    _other => None,
                })
                .collect();
            assert_eq!(saved, vec![key(SHIRT)], "the save wrote the wrong item");
            let transaction = edit_of(&app, shirt)?.saving;
            assert!(transaction.is_some() && edit_of(&app, pants)?.saving.is_none());

            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::InventoryAssetSaved {
                    transaction_id: transaction,
                    asset_id: Uuid::from_u128(0x5A_7ED),
                    success: true,
                }));
            app.update();
            assert!(
                !edit_of(&app, shirt)?.dirty,
                "the save did not land in its own window"
            );
            Ok(())
        }
    }
}
