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
//! `LLAgentWearables::saveWearable`. **Save As** mints a fresh item via the CAPS
//! uploader. **Revert** restores the asset the editor opened on.
//!
//! Reference (Firestorm, read-only): `llpaneleditwearable.cpp`,
//! `llfloatercustomize.cpp`, `llwearabletype.cpp`.

use std::collections::BTreeMap;
use std::collections::HashSet;

use crate::skin_palette::SkinPalette;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui_widgets::{Slider, SliderRange, SliderStep, SliderValue, ValueChange};
use sl_client_bevy::{
    AssetType, Command, InventoryType, ItemInfo, JointOverrides, ParamEffect, ParamGroup, ParamSex,
    ResolvedParams, SkeletalDeformations, SlCommand, SlEvent, SlSessionEvent, TextureKey,
    TransactionId, Uuid, WearableAsset, WearablePermissions, WearableSaleType, WearableType,
};
use sl_client_bevy::{SaleType, avatar_texture};

use crate::asset_editor::{SaveEditorWindow, UnsavedWork};
use crate::avatar_assets::AvatarAssetLibrary;
use crate::avatars::OwnLocalBake;
use crate::bake_inputs::{OwnBakeInputs, shape_is_male};
use crate::floater::{FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater};
use crate::intents::TexturePicked;
use crate::intents::{ItemCreationFinished, ItemCreationTicket, PendingItemCreations};
use crate::inventory::OpenWearableEditor;
use crate::inventory_actions::{wearable_param_group, wearable_type_of};
use crate::inventory_properties::to_wire_item;
use crate::textures::{TextureDecoded, TextureManager};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_font::UiFont;
use crate::ui_radio::{RadioLayout, RadioSelection, RadioSpec, spawn_radio_group};
use crate::ui_slider::{SliderStyle, SliderWidgetPlugin, spawn_slider};
use crate::ui_spawn::{self, ButtonSpec, LabeledRowSpec, UiLabel};
use crate::ui_texture_picker::{TextureSwatchValue, spawn_texture_swatch};
use crate::world_api::DecodedTextures;
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

/// The default next-owner permission mask a Save-As grants (modify | copy |
/// transfer), matching the New-wearable creators.
const NEXT_OWNER_DEFAULT: u32 = 0x0008_e000;

// ---------------------------------------------------------------------------
// Messages, components and resources.
// ---------------------------------------------------------------------------

/// A param slider row: the param it drives, whether that param feeds the bake
/// (a colour / alpha layer) rather than the body shape, and its value label.
#[derive(Component, Debug, Clone, Copy)]
struct WearParamSlider {
    /// The visual-param id.
    id: i32,
    /// Whether editing this param re-composites the bake (a colour / alpha
    /// param) rather than only re-shaping the body.
    is_bake: bool,
    /// The value-readout label entity to keep in sync.
    label: Entity,
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

/// The editor floater's entity handles.
#[derive(Resource)]
struct WearEditorUi {
    /// The floater root (open / close by its [`UiPanelShown`]).
    panel: Entity,
    /// The rebuilt-per-open content column.
    content: Entity,
    /// The floater title text.
    title: Entity,
}

/// The live edit in progress, or `None` when the editor is closed.
#[derive(Resource, Default)]
struct WearEditState {
    /// The active edit.
    active: Option<WearEdit>,
}

/// One in-progress wearable edit.
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
    /// This editor's claim on the shared creation queue for a **Save As** in
    /// flight, or `None` when none is — which is also how "a copy is on the
    /// wire" is known, since the two are the same fact.
    ///
    /// The copy is minted by the CAPS uploader, whose reply carries no
    /// correlation id, so the viewer's one ordered creation queue is what says
    /// which creation is whose, and this is the editor's place in it.
    creation: Option<ItemCreationTicket>,
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
    /// Register the open message, state and systems; spawn the hidden floater.
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SliderWidgetPlugin>() {
            app.add_plugins(SliderWidgetPlugin);
        }
        app.init_resource::<WearEditState>()
            .add_message::<OpenWearableEditor>()
            .add_systems(
                Startup,
                spawn_wearable_editor.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_wearable_editor,
                    apply_wear_texture_picked,
                    apply_wear_tint_picked,
                    apply_wear_gender_radio,
                    note_edited_texture_decoded,
                    drive_wearable_preview,
                    sync_wearable_sliders,
                    save_wearable,
                    report_wearable_save,
                    report_wearable_save_as,
                    end_wearable_edit_on_close,
                    track_wearable_unsaved,
                    scroll_wearable_list,
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
        default_size: Some(Vec2::new(320.0, 520.0)),
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

/// Spawn the (hidden) editor floater and stash its handles.
fn spawn_wearable_editor(mut commands: Commands, root: Res<UiRoot>) {
    let FloaterHandle {
        root: panel,
        content,
        title_text,
    } = spawn_floater(&mut commands, root.0, wearable_editor_floater_spec());
    // Subject-bound: it opens on whatever item you clicked, so its geometry is
    // meaningless across sessions.
    commands.entity(panel).insert((
        crate::floater_persist::FloaterPersistExempt,
        // The edit previews on the avatar and is not stored until a Save, so a
        // close with the sliders moved throws real work away — the scaffold's
        // guard asks first ([`track_wearable_unsaved`]).
        UnsavedWork::default(),
    ));
    commands.insert_resource(WearEditorUi {
        panel,
        content,
        title: title_text,
    });
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

/// What opening the editor rebuilds, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the panel it shows, the
/// title it rewrites, and the commands the whole control list is spawned
/// through.
#[derive(bevy::ecs::system::SystemParam)]
struct WearOpenWidgets<'w, 's> {
    /// The floater's panel, shown once the controls are up.
    panels: Query<'w, 's, &'static mut UiPanelShown>,
    /// The title text, rewritten to the opened item.
    texts: Query<'w, 's, &'static mut Text>,
    /// What the control list is spawned through.
    commands: Commands<'w, 's>,
}

/// Handle an [`OpenWearableEditor`]: seed the edit state from the worn wearable
/// and rebuild the floater's controls.
fn open_wearable_editor(
    mut opens: MessageReader<OpenWearableEditor>,
    ui: Option<Res<WearEditorUi>>,
    inputs: Res<OwnBakeInputs>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut state: ResMut<WearEditState>,
    widgets: WearOpenWidgets,
) {
    let WearOpenWidgets {
        mut panels,
        mut texts,
        mut commands,
    } = widgets;
    let (Some(ui), Some(open)) = (ui, opens.read().last()) else {
        return;
    };
    let item = open.item.clone();
    let slot = wearable_type_of(&item);
    if let Ok(mut title) = texts.get_mut(ui.title) {
        title.0 = format!("Edit: {}", item.name);
    }
    // Tear down the previous content.
    commands.entity(ui.content).despawn_related::<Children>();

    // Seed the edited asset from the worn wearable (falling back to a fresh
    // defaults asset if the outfit's bake inputs are not assembled yet).
    let mut edited = inputs
        .worn_asset(slot)
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
    let sex = avatar_sex(library.as_deref(), inputs.worn_asset(WearableType::Shape));

    // Fill in every editable group param at its current (or default) value, so
    // the saved asset is complete and the sliders start at the right place.
    let sliders: Vec<ParamSliderSpec> = library
        .as_deref()
        .map(|library| editable_params(library, slot, sex, tint_params))
        .unwrap_or_default();
    for slider in &sliders {
        let _prev = edited.params.entry(slider.id).or_insert(slider.value);
    }

    // --- Build the controls. ---
    let mut tab = 0_i32;
    // Action buttons row.
    let button_row = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..row(Val::Px(6.0))
            },
            ChildOf(ui.content),
        ))
        .id();
    for (kind, label) in [
        (WearButton::Save, "Save"),
        (WearButton::SaveAs, "Save As"),
        (WearButton::Revert, "Revert"),
    ] {
        spawn_action_button(&mut commands, button_row, kind, label, &mut tab);
    }
    let status = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT),
            text_role(LABEL_COLOR),
            Node {
                margin: UiRect::bottom(Val::Px(4.0)),
                ..Default::default()
            },
            ChildOf(ui.content),
        ))
        .id();

    // Scrollable list of texture pickers, the tint swatch, then the sliders.
    let list = commands
        .spawn((
            Node {
                height: Val::Px(LIST_HEIGHT),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(4.0))
            },
            ScrollPosition::default(),
            WearScrollList,
            ChildOf(ui.content),
        ))
        .id();

    // Shape: a height read-out and a gender toggle head the list (the reference's
    // Body sub-tab). Other slots have neither.
    let gender_param = library
        .as_deref()
        .and_then(|library| library.params().by_name("male"))
        .map(|param| param.id);
    let mut height_label = None;
    if slot == WearableType::Shape {
        let height_row = spawn_labeled_row(&mut commands, list, "Height");
        height_label = Some(
            commands
                .spawn((
                    Text::new("— m"),
                    UiFont::Sans.at(FONT),
                    text_role(LABEL_COLOR),
                    ChildOf(height_row),
                ))
                .id(),
        );
        let male = gender_param
            .and_then(|id| edited.params.get(&id).copied())
            .unwrap_or(0.0);
        let gender_row = spawn_labeled_row(&mut commands, list, "Gender");
        let labels = [String::from("Female"), String::from("Male")];
        let group = spawn_radio_group(
            &mut commands,
            gender_row,
            &RadioSpec {
                element: GENDER_ELEMENT,
                labels: &labels,
                active: usize::from(male > 0.5),
                tab_index: tab,
                font_size: FONT,
                layout: RadioLayout::Row,
                translate_labels: false,
            },
        );
        commands.entity(group).insert(WearGenderRadio);
        tab = tab.saturating_add(1);
    }

    for (te, label) in wearable_texture_slots(slot) {
        let current = edited
            .textures
            .get(&u32::try_from(te).unwrap_or_default())
            .copied()
            .map_or_else(|| TextureKey::from(Uuid::nil()), TextureKey::from);
        let picker_row = spawn_labeled_row(&mut commands, list, &label);
        let swatch = spawn_texture_swatch(&mut commands, picker_row, "wearable", tab, current);
        commands.entity(swatch).insert(WearTextureSwatch(te));
        tab = tab.saturating_add(1);
    }

    let mut tint_swatch = None;
    if let Some([r, g, b]) = tint_params {
        let color = tint_color(&edited, [r, g, b]);
        let tint_row = spawn_labeled_row(&mut commands, list, "Color / Tint");
        let swatch = spawn_color_swatch(&mut commands, tint_row, "wearable-tint", tab, color);
        commands.entity(swatch).insert(WearTintSwatch);
        tint_swatch = Some(swatch);
        tab = tab.saturating_add(1);
    }

    for slider in &sliders {
        spawn_param_slider(&mut commands, list, slider, &mut tab);
    }

    state.active = Some(WearEdit {
        item,
        wearable_type: slot,
        original: edited.clone(),
        edited,
        tint_params,
        tint_swatch,
        gender_param,
        height_label,
        pending_textures: HashSet::new(),
        shape_dirty: true,
        bake_dirty: true,
        saving: None,
        creation: None,
        dirty: false,
        status: Some(status),
    });

    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
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

/// Spawn a labelled row (a left label plus a slot for the control) and return
/// the row entity to parent the control into.
fn spawn_labeled_row(commands: &mut Commands, parent: Entity, label: &str) -> Entity {
    ui_spawn::spawn_labeled_row(
        commands,
        parent,
        LabeledRowSpec::new(UiLabel::literal(label))
            .label_color(LABEL_COLOR)
            .font_size(FONT)
            .label_width(Val::Px(120.0)),
    )
    .row
}

/// Spawn one param slider row: a label, a slider track + thumb (over
/// `[min, max]`), and a value readout, tagged [`WearParamSlider`].
fn spawn_param_slider(
    commands: &mut Commands,
    parent: Entity,
    spec: &ParamSliderSpec,
    tab: &mut i32,
) {
    let &ParamSliderSpec {
        id,
        is_bake,
        min,
        max,
        value,
        ref label,
    } = spec;
    let row_entity = spawn_labeled_row(commands, parent, label);
    let readout = commands
        .spawn((
            Text::new(format!("{value:.2}")),
            UiFont::Sans.at(FONT),
            text_role(LABEL_COLOR),
            Node {
                width: Val::Px(38.0),
                ..Default::default()
            },
            ChildOf(row_entity),
        ))
        .id();
    let range = SliderRange::new(min, max);
    let track = spawn_slider(
        commands,
        row_entity,
        SLIDER,
        *tab,
        0.0,
        (
            Slider::default(),
            SliderValue(value.clamp(min, max)),
            range,
            SliderStep((max - min) / 100.0),
            WearParamSlider {
                id,
                is_bake,
                label: readout,
            },
            Name::new(format!("wearable-slider:{id}")),
        ),
    );
    commands.entity(track).observe(on_wear_slider_change);
    *tab = tab.saturating_add(1);
}

/// Spawn one chrome action button (Save / Save As / Revert).
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    kind: WearButton,
    label: &str,
    tab: &mut i32,
) {
    let button = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::literal(label), format!("wearable-button:{label}"))
            .tab_from(tab)
            .padding(10.0, 3.0)
            .colors(BUTTON_BACKGROUND, CONTROL_BORDER)
            .label_color(LABEL_COLOR)
            .font_size(FONT),
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
    mut state: ResMut<WearEditState>,
    mut commands: Commands,
) {
    let Ok((info, range)) = sliders.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(edit) = state.active.as_mut() {
        let _prev = edit.edited.params.insert(info.id, clamped);
        edit.dirty = true;
        if info.is_bake {
            edit.bake_dirty = true;
        } else {
            edit.shape_dirty = true;
        }
    }
}

/// The Shape gender radio (Female = `0` / Male = `1`): write the `male` param
/// and re-shape the body when the selection changes.
fn apply_wear_gender_radio(
    radios: Query<&RadioSelection, (With<WearGenderRadio>, Changed<RadioSelection>)>,
    mut state: ResMut<WearEditState>,
) {
    let Some(edit) = state.active.as_mut() else {
        return;
    };
    let Some(id) = edit.gender_param else {
        return;
    };
    for selection in &radios {
        let male = if selection.active >= 1 { 1.0 } else { 0.0 };
        if edit.edited.params.get(&id).copied() != Some(male) {
            let _prev = edit.edited.params.insert(id, male);
            edit.shape_dirty = true;
            edit.dirty = true;
        }
    }
}

/// A texture pick: record the new layer texture, repaint the swatch, and mark
/// the bake dirty.
fn apply_wear_texture_picked(
    mut picks: MessageReader<TexturePicked>,
    swatches: Query<&WearTextureSwatch>,
    mut values: Query<&mut TextureSwatchValue>,
    mut state: ResMut<WearEditState>,
) {
    let Some(edit) = state.active.as_mut() else {
        picks.clear();
        return;
    };
    for pick in picks.read() {
        let Ok(slot) = swatches.get(pick.requester) else {
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

/// A tint pick: write the colour into the slot's three colour params, repaint
/// the swatch, and mark the bake dirty.
fn apply_wear_tint_picked(
    mut picks: MessageReader<ColorPicked>,
    mut values: Query<&mut ColorSwatchValue, With<WearTintSwatch>>,
    mut state: ResMut<WearEditState>,
) {
    let Some(edit) = state.active.as_mut() else {
        picks.clear();
        return;
    };
    let (Some([r, g, b]), Some(tint_swatch)) = (edit.tint_params, edit.tint_swatch) else {
        picks.clear();
        return;
    };
    for pick in picks.read() {
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
    mut state: ResMut<WearEditState>,
) {
    let Some(edit) = state.active.as_mut() else {
        decoded.clear();
        return;
    };
    for &TextureDecoded(key) in decoded.read() {
        if edit.pending_textures.remove(&key) {
            edit.bake_dirty = true;
        }
    }
}

/// Push the edited wearable into the bake inputs when something changed, so the
/// shape morph and bake composite re-derive from it (the live preview).
fn drive_wearable_preview(
    mut state: ResMut<WearEditState>,
    mut inputs: ResMut<OwnBakeInputs>,
    mut texture_manager: ResMut<TextureManager>,
    store: Res<DecodedTextures>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut local_bake: ResMut<OwnLocalBake>,
    mut texts: Query<&mut Text>,
) {
    let Some(edit) = state.active.as_mut() else {
        return;
    };
    if !edit.shape_dirty && !edit.bake_dirty {
        return;
    }
    // The shape morph re-derives from the substituted asset; the bake needs an
    // explicit texture request + reassemble + rebuild.
    inputs.set_preview_asset(edit.edited.clone());
    // Update the Shape height read-out from the edited body proportions (the
    // reference's `computeBodySize` `mBodySize.z`).
    if edit.shape_dirty
        && edit.wearable_type == WearableType::Shape
        && let (Some(library), Some(label)) = (library.as_deref(), edit.height_label)
    {
        let bytes = inputs.visual_params(library.params());
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
        inputs.request_asset_textures(&edit.edited, &mut texture_manager, &store);
        // Track edited textures still decoding so the composite re-runs once
        // they land.
        edit.pending_textures = edit
            .edited
            .textures
            .values()
            .copied()
            .filter(|id| !id.is_nil())
            .map(TextureKey::from)
            .filter(|key| store.get(*key).is_none())
            .collect();
        inputs.reassemble(&store, library.as_deref());
        local_bake.invalidate();
    }
    edit.shape_dirty = false;
    edit.bake_dirty = false;
}

/// Keep each slider's value **readout** in sync with its [`SliderValue`]. The
/// thumb is `sl_viewer_ui_widgets::ui_slider`'s to place.
fn sync_wearable_sliders(
    sliders: Query<(&WearParamSlider, &SliderValue)>,
    mut texts: Query<&mut Text>,
) {
    for (info, value) in &sliders {
        if let Ok(mut text) = texts.get_mut(info.label) {
            let want = format!("{:.2}", value.0);
            if text.0 != want {
                text.0 = want;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Save / Save-As / Revert.
// ---------------------------------------------------------------------------

/// A chrome button press: Save (in place), Save As (new item), or Revert.
///
/// Save is not carried out here — it is written as a [`SaveEditorWindow`] for
/// [`save_wearable`], the same message the unsaved-work confirmation's "Save"
/// answer writes, so the button and the confirmation cannot come to save
/// different things.
fn on_wear_button(
    press: On<Pointer<Press>>,
    buttons: Query<&WearButton>,
    ui: Option<Res<WearEditorUi>>,
    mut state: ResMut<WearEditState>,
    mut preview: WearPreview,
    out: WearSaveOut,
    mut texts: Query<&mut Text>,
) {
    let WearSaveOut {
        mut pending,
        mut commands,
        mut saves,
    } = out;
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(kind) = buttons.get(press.entity).copied() else {
        return;
    };
    let Some(edit) = state.active.as_mut() else {
        return;
    };
    match kind {
        WearButton::Save => {
            if let Some(ui) = ui.as_deref() {
                saves.write(SaveEditorWindow { window: ui.panel });
            }
        }
        WearButton::SaveAs => {
            let perms = wearable_permissions(&edit.item);
            let name = format!("{} (copy)", edit.item.name);
            let mut copy = edit.edited.clone();
            copy.name.clone_from(&name);
            let data = copy.to_text(&perms).into_bytes();
            commands.write(SlCommand(Command::UploadAsset {
                folder_id: edit.item.folder_id,
                asset_type: asset_type_of(edit.wearable_type),
                inventory_type: InventoryType::Wearable,
                name,
                description: String::new(),
                next_owner_mask: NEXT_OWNER_DEFAULT,
                group_mask: 0,
                everyone_mask: 0,
                expected_upload_cost: 0,
                data,
            }));
            // The copy has been *asked for*, not made: the bytes are on their way
            // to the CAPS uploader and the grid has yet to say whether it kept
            // them. Announcing the copy here — which is what this did — is a
            // claim about something that has not happened, and a refused upload
            // left it standing. The ticket is this editor's claim on the one
            // ordered creation queue; `report_wearable_save_as` answers on it.
            edit.creation =
                Some(pending.enqueue(u32::from(edit.wearable_type.to_code()), edit.item.folder_id));
            set_status(&mut texts, edit.status, "Saving a copy…");
        }
        WearButton::Revert => {
            edit.edited.clone_from(&edit.original);
            // Re-derive the preview from the restored asset.
            preview.restore(&edit.original);
            edit.shape_dirty = true;
            // Back to what was opened on, so there is nothing unsaved left to
            // ask about on a close.
            edit.dirty = false;
            set_status(&mut texts, edit.status, "Reverted.");
        }
    }
}

/// Where a save leaves, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the ordered creation queue
/// a Save As takes a ticket in, the session the upload goes out on, and the
/// in-place save request the Save button raises.
#[derive(bevy::ecs::system::SystemParam)]
struct WearSaveOut<'w> {
    /// The ordered creation queue a Save As claims a ticket in, so the reply
    /// can be told from somebody else's.
    pending: ResMut<'w, PendingItemCreations>,
    /// The session a Save As's upload goes out on.
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
    /// Re-derive the worn preview from `original`, the asset the editor was
    /// opened on.
    fn restore(&mut self, original: &WearableAsset) {
        self.inputs.set_preview_asset(original.clone());
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
    ui: Option<Res<WearEditorUi>>,
    mut state: ResMut<WearEditState>,
    mut commands: MessageWriter<SlCommand>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui.as_deref() else {
        requests.clear();
        return;
    };
    for request in requests.read() {
        if request.window != ui.panel {
            continue;
        }
        let Some(edit) = state.active.as_mut() else {
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
    mut state: ResMut<WearEditState>,
    mut texts: Query<&mut Text>,
) {
    let Some(edit) = state.active.as_mut() else {
        events.clear();
        return;
    };
    for event in events.read() {
        let SlSessionEvent::InventoryAssetSaved {
            transaction_id,
            success,
            ..
        } = &event.0
        else {
            continue;
        };
        // Ours only if it names the transaction this editor minted. Without the
        // check, any save completing while this one is in flight — another
        // editor's, or an earlier save of this one that timed out — is reported
        // here as this save's result.
        if edit.saving.is_none() || *transaction_id != edit.saving {
            continue;
        }
        edit.saving = None;
        if *success {
            // Stored, so the edit *is* what the item holds now: nothing unsaved
            // is left, a Revert should go back to this rather than to what was
            // opened on, and a close waiting on the save may go ahead.
            edit.original.clone_from(&edit.edited);
            edit.dirty = false;
        }
        let message = if *success { "Saved." } else { "Save failed." };
        set_status(&mut texts, edit.status, message);
    }
}

/// Report a **Save As** landing — or failing — on the ticket this editor took
/// out of the shared creation queue.
fn report_wearable_save_as(
    mut finished: MessageReader<ItemCreationFinished>,
    mut state: ResMut<WearEditState>,
    mut texts: Query<&mut Text>,
) {
    let Some(edit) = state.active.as_mut() else {
        finished.clear();
        return;
    };
    for creation in finished.read() {
        if edit.creation != Some(creation.ticket) {
            continue;
        }
        edit.creation = None;
        let message = if creation.item.is_some() {
            "Saved a copy to inventory."
        } else {
            "Saving a copy failed."
        };
        set_status(&mut texts, edit.status, message);
    }
}

/// End the edit when the window closes: restore the worn wearable the editor
/// opened on and drop the live edit.
///
/// Closing is the one answer the guard does not stop — either the resident
/// saved first (in which case the restored asset *is* what they saved), or they
/// said the work could go. Either way the preview must not outlive the window:
/// [`OwnBakeInputs::set_preview_asset`] substitutes the edit into the worn
/// outfit, so leaving it in place keeps the avatar wearing an unsaved edit with
/// no window left to save, revert or even see it in — and the next open would
/// read that preview back as if it were what is worn.
fn end_wearable_edit_on_close(
    ui: Option<Res<WearEditorUi>>,
    panels: Query<&UiPanelShown, Changed<UiPanelShown>>,
    mut state: ResMut<WearEditState>,
    mut preview: WearPreview,
    mut works: Query<&mut UnsavedWork>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let Ok(shown) = panels.get(ui.panel) else {
        return;
    };
    if shown.0 {
        return;
    }
    let Some(edit) = state.active.take() else {
        return;
    };
    preview.restore(&edit.original);
    if let Ok(mut work) = works.get_mut(ui.panel) {
        *work = UnsavedWork::default();
    }
}

/// Keep the editor floater's close guard in step with the edit: while the
/// wearable on screen differs from what was opened on (or last saved), closing
/// the window would throw that away, and the scaffold asks first.
fn track_wearable_unsaved(
    ui: Option<Res<WearEditorUi>>,
    state: Res<WearEditState>,
    mut windows: Query<&mut UnsavedWork>,
) {
    let Some(ui) = ui.as_deref() else {
        return;
    };
    let Ok(mut work) = windows.get_mut(ui.panel) else {
        return;
    };
    let dirty = state.active.as_ref().is_some_and(|edit| edit.dirty);
    if work.dirty != dirty {
        work.dirty = dirty;
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
    use super::{WearEdit, WearEditState, report_wearable_save, report_wearable_save_as};
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

    /// The shirt the editor is editing.
    fn shirt() -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(0x11)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x22)),
            name: "A shirt".to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(0x33),
            asset_type: AssetType::Clothing,
            inv_type: InventoryType::Wearable,
            flags: 0,
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

    /// An editor with an open, modified edit and a status readout, plus the
    /// systems that report a save's outcome.
    fn editor_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_message::<SlEvent>()
            .add_message::<crate::intents::ItemCreationFinished>()
            .init_resource::<WearEditState>()
            .init_resource::<PendingItemCreations>()
            .add_systems(Update, (report_wearable_save, report_wearable_save_as));
        let status = app.world_mut().spawn(Text::new(String::new())).id();
        let asset = WearableAsset {
            version: super::WEARABLE_VERSION,
            name: "A shirt".to_owned(),
            wearable_type: WearableType::Shirt,
            params: BTreeMap::new(),
            textures: BTreeMap::new(),
        };
        app.world_mut().resource_mut::<WearEditState>().active = Some(WearEdit {
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
        });
        app.update();
        (app, status)
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
        let (mut app, status) = editor_app();
        let ours = TransactionId::from(Uuid::from_u128(0xAA));
        let theirs = TransactionId::from(Uuid::from_u128(0xBB));
        if let Some(edit) = app
            .world_mut()
            .resource_mut::<WearEditState>()
            .active
            .as_mut()
        {
            edit.saving = Some(ours);
        }

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
        assert!(
            app.world()
                .resource::<WearEditState>()
                .active
                .as_ref()
                .is_some_and(|edit| edit.saving == Some(ours)),
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
        let state = app.world().resource::<WearEditState>();
        let edit = state.active.as_ref().ok_or("the edit went away")?;
        assert_eq!(edit.saving, None, "the save is no longer in flight");
        assert!(
            !edit.dirty,
            "a stored edit is no longer unsaved work to ask about"
        );
        Ok(())
    }

    /// A refused save leaves the work unsaved, so a close still asks and a
    /// Revert still has somewhere to go back to.
    #[test]
    fn a_refused_save_leaves_the_work_unsaved() -> Result<(), TestError> {
        let (mut app, status) = editor_app();
        let ours = TransactionId::from(Uuid::from_u128(0xCC));
        if let Some(edit) = app
            .world_mut()
            .resource_mut::<WearEditState>()
            .active
            .as_mut()
        {
            edit.saving = Some(ours);
        }
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::InventoryAssetSaved {
                transaction_id: Some(ours),
                asset_id: Uuid::from_u128(0xC0),
                success: false,
            }));
        app.update();
        assert_eq!(status_of(&app, status), "Save failed.");
        let state = app.world().resource::<WearEditState>();
        let edit = state.active.as_ref().ok_or("the edit went away")?;
        assert!(edit.dirty, "a refused save must not clear the unsaved work");
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
        let (mut app, status) = editor_app();
        // The ticket comes out of the real queue, the way the button takes one.
        let ticket = app
            .world_mut()
            .resource_mut::<PendingItemCreations>()
            .enqueue(
                u32::from(WearableType::Shirt.to_code()),
                InventoryFolderKey::from(Uuid::from_u128(0x22)),
            );
        let other = app
            .world_mut()
            .resource_mut::<PendingItemCreations>()
            .enqueue(
                u32::from(WearableType::Pants.to_code()),
                InventoryFolderKey::from(Uuid::from_u128(0x99)),
            );
        if let Some(edit) = app
            .world_mut()
            .resource_mut::<WearEditState>()
            .active
            .as_mut()
        {
            edit.creation = Some(ticket);
        }

        // Somebody else's creation on the shared queue is not this Save As.
        app.world_mut()
            .write_message(crate::intents::ItemCreationFinished {
                ticket: other,
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
                ticket,
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
        let (mut app, status) = editor_app();
        let ticket = app
            .world_mut()
            .resource_mut::<PendingItemCreations>()
            .enqueue(
                u32::from(WearableType::Shirt.to_code()),
                InventoryFolderKey::from(Uuid::from_u128(0x22)),
            );
        if let Some(edit) = app
            .world_mut()
            .resource_mut::<WearEditState>()
            .active
            .as_mut()
        {
            edit.creation = Some(ticket);
        }
        app.world_mut()
            .write_message(crate::intents::ItemCreationFinished { ticket, item: None });
        app.update();
        assert_eq!(status_of(&app, status), "Saving a copy failed.");
        Ok(())
    }
}
