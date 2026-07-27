//! The **material editor** floater (`viewer-pbr-material-editor`): a standalone
//! window that edits a GLTF (PBR) **material inventory asset** — a peer of the
//! wearable editors, combining a few texture maps and tint / factor values with
//! a live **preview sphere**.
//!
//! Opened from the inventory context menu's **Edit** on a material item, it
//! fetches and decodes the [`GltfMaterial`], exposes its base-colour tint +
//! texture, metallic / roughness factors + packed texture, normal map, emissive
//! tint + texture, alpha mode + cutoff, and the double-sided flag, previews the
//! result on a sphere ([`MaterialPreview`]), and **Save**s it back onto the same
//! item over the `UpdateMaterialAgentInventory` capability
//! ([`Command::UpdateInventoryAsset`]).
//!
//! This is the standalone-floater half of the material editing the viewer
//! already does inline in the Build Tools Texture tab (`edit_material`): here the
//! subject is an inventory asset previewed on a sphere, not a selected in-world
//! face.
//!
//! Reference (Firestorm, read-only): `llmaterialeditor`,
//! `floater_material_editor.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Slider, SliderRange, SliderStep, SliderThumb, SliderValue, ValueChange};
use sl_client_bevy::{
    AssetKey, AssetUpdateLocation, Command, GltfAlphaMode, GltfMaterial, GltfTexture, ItemInfo,
    SlCommand, SlEvent, SlSessionEvent, TextureKey, UpdatableAssetType, Uuid,
    encode_material_asset,
};

use crate::floater::{FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater};
use crate::material_preview::MaterialPreview;
use crate::materials::MaterialManager;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, row};
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_font::UiFont;
use crate::ui_texture_picker::{TexturePicked, TextureSwatchValue, spawn_texture_swatch};

/// The chrome font size, in logical pixels.
const FONT: f32 = 13.0;

/// A slider track's width, in logical pixels.
const TRACK_WIDTH: f32 = 140.0;

/// A slider track's height.
const TRACK_HEIGHT: f32 = 12.0;

/// A slider thumb's width.
const THUMB_WIDTH: f32 = 9.0;

/// The preview sphere pane's side length, in logical pixels.
const PREVIEW_SIZE: f32 = 128.0;

/// The label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A control's border colour.
const CONTROL_BORDER: Color = Color::srgba(0.34, 0.40, 0.52, 1.0);

/// A slider track's fill.
const TRACK_FILL: Color = Color::srgba(0.12, 0.13, 0.16, 1.0);

/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.72, 0.76, 0.84);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// The checked / unchecked toggle glyphs.
const CHECKED_GLYPH: &str = "\u{2611}";
/// The unchecked toggle glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

// ---------------------------------------------------------------------------
// Messages, components and resources.
// ---------------------------------------------------------------------------

/// Open the material editor on a material inventory item.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenMaterialEditor {
    /// The material item to edit.
    pub(crate) item: ItemInfo,
}

/// Which texture channel a picker swatch edits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MatTexSlot {
    /// The base-colour (albedo) texture.
    Base,
    /// The packed metallic-roughness texture.
    MetallicRoughness,
    /// The tangent-space normal map.
    Normal,
    /// The emissive texture.
    Emissive,
}

/// Which colour a tint swatch edits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MatColorSlot {
    /// The base-colour factor (its RGB; alpha is left unchanged).
    Base,
    /// The emissive factor.
    Emissive,
}

/// Which scalar factor a slider edits.
#[derive(Component, Debug, Clone, Copy)]
struct MatFactorSlider {
    /// The factor this slider drives.
    kind: MatFactor,
    /// The value-readout label entity.
    label: Entity,
}

/// A scalar material factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatFactor {
    /// The metallic factor.
    Metallic,
    /// The roughness factor.
    Roughness,
    /// The alpha cutoff (Mask mode).
    Cutoff,
}

/// The alpha-mode cycle button.
#[derive(Component, Debug, Clone, Copy)]
struct MatAlphaButton;

/// The double-sided toggle button.
#[derive(Component, Debug, Clone, Copy)]
struct MatDoubleSidedButton;

/// A chrome action button.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MatButton {
    /// Write the edit back onto the item.
    Save,
    /// Restore the material the editor opened on.
    Revert,
}

/// The editor floater's entity handles.
#[derive(Resource)]
struct MatEditorUi {
    /// The floater root.
    panel: Entity,
    /// The rebuilt-per-open content column.
    content: Entity,
    /// The floater title text.
    title: Entity,
}

/// The material edit in progress, or `None` when the editor is closed.
#[derive(Resource, Default)]
struct MatEditState {
    /// The active edit.
    active: Option<MatEdit>,
}

/// Where an edit is in its build lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatPhase {
    /// Awaiting the material asset's decode before building the controls.
    Loading,
    /// The controls are built and live.
    Ready,
    /// Rebuild the controls from the edited material (a Revert reset).
    Rebuild,
}

/// One in-progress material edit.
struct MatEdit {
    /// The item being edited.
    item: ItemInfo,
    /// The material asset id (the fetch / decode key).
    asset: AssetKey,
    /// The material the editor opened on (for Revert).
    original: GltfMaterial,
    /// The live-edited material (Save / preview source of truth).
    edited: GltfMaterial,
    /// The build lifecycle phase.
    phase: MatPhase,
    /// A change is awaiting a preview refresh.
    dirty: bool,
    /// A Save is in flight.
    saving: bool,
    /// The preview sphere node.
    preview: Option<Entity>,
    /// The alpha-mode button's label node.
    alpha_label: Option<Entity>,
    /// The double-sided button's glyph node.
    double_label: Option<Entity>,
    /// The status readout node.
    status: Option<Entity>,
}

/// The material-editor plugin.
pub(crate) struct EditMaterialAssetPlugin;

impl Plugin for EditMaterialAssetPlugin {
    /// Register the open message, state and systems; spawn the hidden floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<MatEditState>()
            .add_message::<OpenMaterialEditor>()
            .add_systems(
                Startup,
                spawn_material_editor.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_material_editor,
                    populate_material_editor,
                    apply_mat_texture_picked,
                    apply_mat_color_picked,
                    drive_material_preview,
                    sync_material_sliders,
                    report_material_save,
                )
                    .chain(),
            );
    }
}

/// Spawn the (hidden) editor floater and stash its handles.
fn spawn_material_editor(mut commands: Commands, root: Res<UiRoot>) {
    let FloaterHandle {
        root: panel,
        content,
        title_text,
    } = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "material-editor",
            title: "Edit Material".to_owned(),
            position: Vec2::new(340.0, 90.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(panel)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands.insert_resource(MatEditorUi {
        panel,
        content,
        title: title_text,
    });
}

// ---------------------------------------------------------------------------
// Open — request the asset, then build once it decodes.
// ---------------------------------------------------------------------------

/// Handle an [`OpenMaterialEditor`]: request the material asset, clear the
/// content to a loading note, and show the floater. The controls are built by
/// [`populate_material_editor`] once the decode lands.
fn open_material_editor(
    mut opens: MessageReader<OpenMaterialEditor>,
    ui: Option<Res<MatEditorUi>>,
    mut materials: ResMut<MaterialManager>,
    mut state: ResMut<MatEditState>,
    mut panels: Query<&mut UiPanelShown>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    let (Some(ui), Some(open)) = (ui, opens.read().last()) else {
        return;
    };
    let item = open.item.clone();
    let asset = AssetKey::from(item.asset_id);
    if let Ok(mut title) = texts.get_mut(ui.title) {
        title.0 = format!("Edit: {}", item.name);
    }
    commands.entity(ui.content).despawn_related::<Children>();
    commands.spawn((
        Text::new("Loading material…"),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        ChildOf(ui.content),
    ));
    materials.request_material(asset);
    state.active = Some(MatEdit {
        item,
        asset,
        original: GltfMaterial::default(),
        edited: GltfMaterial::default(),
        phase: MatPhase::Loading,
        dirty: false,
        saving: false,
        preview: None,
        alpha_label: None,
        double_label: None,
        status: None,
    });
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

/// Build the editor controls once the material asset has decoded.
fn populate_material_editor(
    ui: Option<Res<MatEditorUi>>,
    materials: Res<MaterialManager>,
    mut state: ResMut<MatEditState>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(edit) = state.active.as_mut() else {
        return;
    };
    // Build on first decode, or rebuild the controls from the edited material
    // after a Revert; otherwise nothing to do.
    let material = match edit.phase {
        MatPhase::Loading => {
            let Some(material) = materials.decoded_material(edit.asset).copied() else {
                return;
            };
            edit.original = material;
            edit.edited = material;
            material
        }
        MatPhase::Rebuild => edit.edited,
        MatPhase::Ready => return,
    };
    edit.phase = MatPhase::Ready;
    edit.dirty = true;

    commands.entity(ui.content).despawn_related::<Children>();
    let mut tab = 0_i32;

    // Action buttons + status.
    let button_row = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..row(Val::Px(6.0))
            },
            ChildOf(ui.content),
        ))
        .id();
    for (kind, label) in [(MatButton::Save, "Save"), (MatButton::Revert, "Revert")] {
        spawn_mat_button(&mut commands, button_row, kind, label, &mut tab);
    }
    let status = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT),
            TextColor(LABEL_COLOR),
            Node {
                margin: UiRect::bottom(Val::Px(4.0)),
                ..Default::default()
            },
            ChildOf(ui.content),
        ))
        .id();

    // Preview sphere.
    let preview = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_SIZE),
                height: Val::Px(PREVIEW_SIZE),
                border: UiRect::all(Val::Px(1.0)),
                margin: UiRect::bottom(Val::Px(6.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            MaterialPreview::Material(Box::new(material)),
            ChildOf(ui.content),
        ))
        .id();

    // Base colour: tint swatch + texture.
    let base_row = spawn_labeled_row(&mut commands, ui.content, "Base Color");
    let base_color = base_linear_color(&material);
    let swatch = spawn_color_swatch(&mut commands, base_row, "material-base", tab, base_color);
    commands.entity(swatch).insert(MatColorSlot::Base);
    tab = tab.saturating_add(1);
    let base_tex = spawn_texture_swatch(
        &mut commands,
        base_row,
        "material-base-tex",
        tab,
        texture_key(material.base_color_texture),
    );
    commands.entity(base_tex).insert(MatTexSlot::Base);
    tab = tab.saturating_add(1);

    // Metallic / roughness: two factor sliders + the packed texture.
    spawn_factor_slider(
        &mut commands,
        ui.content,
        "Metallic",
        MatFactor::Metallic,
        material.metallic_factor,
        &mut tab,
    );
    spawn_factor_slider(
        &mut commands,
        ui.content,
        "Roughness",
        MatFactor::Roughness,
        material.roughness_factor,
        &mut tab,
    );
    let mr_row = spawn_labeled_row(&mut commands, ui.content, "Metal/Rough Map");
    let mr_tex = spawn_texture_swatch(
        &mut commands,
        mr_row,
        "material-mr-tex",
        tab,
        texture_key(material.metallic_roughness_texture),
    );
    commands
        .entity(mr_tex)
        .insert(MatTexSlot::MetallicRoughness);
    tab = tab.saturating_add(1);

    // Normal map.
    let normal_row = spawn_labeled_row(&mut commands, ui.content, "Normal Map");
    let normal_tex = spawn_texture_swatch(
        &mut commands,
        normal_row,
        "material-normal-tex",
        tab,
        texture_key(material.normal_texture),
    );
    commands.entity(normal_tex).insert(MatTexSlot::Normal);
    tab = tab.saturating_add(1);

    // Emissive: tint swatch + texture.
    let emissive_row = spawn_labeled_row(&mut commands, ui.content, "Emissive");
    let emissive_color = Color::linear_rgb(
        material.emissive_factor.first().copied().unwrap_or(0.0),
        material.emissive_factor.get(1).copied().unwrap_or(0.0),
        material.emissive_factor.get(2).copied().unwrap_or(0.0),
    );
    let em_swatch = spawn_color_swatch(
        &mut commands,
        emissive_row,
        "material-emissive",
        tab,
        emissive_color,
    );
    commands.entity(em_swatch).insert(MatColorSlot::Emissive);
    tab = tab.saturating_add(1);
    let em_tex = spawn_texture_swatch(
        &mut commands,
        emissive_row,
        "material-emissive-tex",
        tab,
        texture_key(material.emissive_texture),
    );
    commands.entity(em_tex).insert(MatTexSlot::Emissive);
    tab = tab.saturating_add(1);

    // Alpha mode (cycle) + cutoff.
    let alpha_row = spawn_labeled_row(&mut commands, ui.content, "Alpha Mode");
    let alpha_label = spawn_text_button(
        &mut commands,
        alpha_row,
        alpha_mode_name(material.alpha_mode),
        MatAlphaButton,
        &mut tab,
    );
    spawn_factor_slider(
        &mut commands,
        ui.content,
        "Alpha Cutoff",
        MatFactor::Cutoff,
        material.alpha_cutoff,
        &mut tab,
    );

    // Double-sided toggle.
    let double_row = spawn_labeled_row(&mut commands, ui.content, "Double Sided");
    let double_label = spawn_text_button(
        &mut commands,
        double_row,
        toggle_glyph(material.double_sided),
        MatDoubleSidedButton,
        &mut tab,
    );

    edit.preview = Some(preview);
    edit.alpha_label = Some(alpha_label);
    edit.double_label = Some(double_label);
    edit.status = Some(status);
}

// ---------------------------------------------------------------------------
// Control spawn helpers.
// ---------------------------------------------------------------------------

/// Spawn a labelled row and return the row entity to parent the control into.
fn spawn_labeled_row(commands: &mut Commands, parent: Entity, label: &str) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                margin: UiRect::bottom(Val::Px(3.0)),
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(label.to_owned()),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Node {
            width: Val::Px(110.0),
            ..Default::default()
        },
        ChildOf(row_entity),
    ));
    row_entity
}

/// Spawn a factor slider row (`0..=1`) tagged with its [`MatFactor`].
fn spawn_factor_slider(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    kind: MatFactor,
    value: f32,
    tab: &mut i32,
) {
    let row_entity = spawn_labeled_row(commands, parent, label);
    let readout = commands
        .spawn((
            Text::new(format!("{value:.2}")),
            UiFont::Sans.at(FONT),
            TextColor(LABEL_COLOR),
            Node {
                width: Val::Px(34.0),
                ..Default::default()
            },
            ChildOf(row_entity),
        ))
        .id();
    commands
        .spawn((
            Slider::default(),
            SliderValue(value.clamp(0.0, 1.0)),
            SliderRange::new(0.0, 1.0),
            SliderStep(0.01),
            MatFactorSlider {
                kind,
                label: readout,
            },
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(*tab),
            Name::new("material-factor-slider"),
            ChildOf(row_entity),
        ))
        .observe(on_mat_slider_change)
        .with_child((
            SliderThumb,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(THUMB_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                ..Default::default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(0.0),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(THUMB_FILL),
        ));
    *tab = tab.saturating_add(1);
}

/// Spawn a bordered text button carrying `marker`, and return its label node so
/// the caller can update the button's text later.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    marker: impl Component,
    tab: &mut i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(*tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            marker,
            Pickable::default(),
            Name::new("material-text-button"),
            ChildOf(parent),
        ))
        .observe(on_mat_toggle)
        .id();
    let text = commands
        .spawn((
            Text::new(label.to_owned()),
            UiFont::Sans.at(FONT),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    *tab = tab.saturating_add(1);
    text
}

/// Spawn one chrome action button (Save / Revert).
fn spawn_mat_button(
    commands: &mut Commands,
    parent: Entity,
    kind: MatButton,
    label: &str,
    tab: &mut i32,
) {
    let button = commands
        .spawn((
            Button,
            TabIndex(*tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            kind,
            Pickable::default(),
            Name::new(format!("material-button:{label}")),
            ChildOf(parent),
        ))
        .observe(on_mat_action_button)
        .id();
    commands.spawn((
        Text::new(label.to_owned()),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    *tab = tab.saturating_add(1);
}

// ---------------------------------------------------------------------------
// Edit handlers.
// ---------------------------------------------------------------------------

/// A factor slider drag: clamp, write back, record the edit, mark dirty.
fn on_mat_slider_change(
    change: On<ValueChange<f32>>,
    sliders: Query<&MatFactorSlider>,
    mut state: ResMut<MatEditState>,
    mut commands: Commands,
) {
    let Ok(info) = sliders.get(change.source) else {
        return;
    };
    let clamped = change.value.clamp(0.0, 1.0);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(edit) = state.active.as_mut() {
        match info.kind {
            MatFactor::Metallic => edit.edited.metallic_factor = clamped,
            MatFactor::Roughness => edit.edited.roughness_factor = clamped,
            MatFactor::Cutoff => edit.edited.alpha_cutoff = clamped,
        }
        edit.dirty = true;
    }
}

/// A texture pick: record the new map (or clear it on a nil pick), repaint the
/// swatch, mark dirty.
fn apply_mat_texture_picked(
    mut picks: MessageReader<TexturePicked>,
    slots: Query<&MatTexSlot>,
    mut values: Query<&mut TextureSwatchValue>,
    mut state: ResMut<MatEditState>,
) {
    let Some(edit) = state.active.as_mut() else {
        picks.clear();
        return;
    };
    for pick in picks.read() {
        let Ok(slot) = slots.get(pick.requester) else {
            continue;
        };
        let texture = material_texture(pick.texture, slot_texture(&edit.edited, *slot));
        match slot {
            MatTexSlot::Base => edit.edited.base_color_texture = texture,
            MatTexSlot::MetallicRoughness => edit.edited.metallic_roughness_texture = texture,
            MatTexSlot::Normal => edit.edited.normal_texture = texture,
            MatTexSlot::Emissive => edit.edited.emissive_texture = texture,
        }
        edit.dirty = true;
        if let Ok(mut value) = values.get_mut(pick.requester) {
            value.0 = pick.texture;
        }
    }
}

/// A colour pick: write the base or emissive factor, repaint the swatch, mark
/// dirty.
fn apply_mat_color_picked(
    mut picks: MessageReader<ColorPicked>,
    slots: Query<&MatColorSlot>,
    mut values: Query<&mut ColorSwatchValue>,
    mut state: ResMut<MatEditState>,
) {
    let Some(edit) = state.active.as_mut() else {
        picks.clear();
        return;
    };
    for pick in picks.read() {
        let Ok(slot) = slots.get(pick.requester) else {
            continue;
        };
        let linear = pick.color.to_linear();
        match slot {
            MatColorSlot::Base => {
                let alpha = edit.edited.base_color.get(3).copied().unwrap_or(1.0);
                edit.edited.base_color = [linear.red, linear.green, linear.blue, alpha];
            }
            MatColorSlot::Emissive => {
                edit.edited.emissive_factor = [linear.red, linear.green, linear.blue];
            }
        }
        edit.dirty = true;
        if let Ok(mut value) = values.get_mut(pick.requester) {
            value.0 = pick.color;
        }
    }
}

/// A text-button press: cycle the alpha mode, or toggle double-sided.
fn on_mat_toggle(
    press: On<Pointer<Press>>,
    alpha: Query<&MatAlphaButton>,
    double: Query<&MatDoubleSidedButton>,
    mut state: ResMut<MatEditState>,
    mut texts: Query<&mut Text>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Some(edit) = state.active.as_mut() else {
        return;
    };
    if alpha.get(press.entity).is_ok() {
        edit.edited.alpha_mode = next_alpha_mode(edit.edited.alpha_mode);
        set_text(
            &mut texts,
            edit.alpha_label,
            alpha_mode_name(edit.edited.alpha_mode),
        );
        edit.dirty = true;
    } else if double.get(press.entity).is_ok() {
        edit.edited.double_sided = !edit.edited.double_sided;
        set_text(
            &mut texts,
            edit.double_label,
            toggle_glyph(edit.edited.double_sided),
        );
        edit.dirty = true;
    }
}

/// Refresh the preview sphere when the edited material changed.
fn drive_material_preview(
    mut state: ResMut<MatEditState>,
    mut previews: Query<&mut MaterialPreview>,
) {
    let Some(edit) = state.active.as_mut() else {
        return;
    };
    if !edit.dirty {
        return;
    }
    if let Some(node) = edit.preview
        && let Ok(mut preview) = previews.get_mut(node)
    {
        *preview = MaterialPreview::Material(Box::new(edit.edited));
    }
    edit.dirty = false;
}

/// Keep each factor slider's thumb + readout in sync with its [`SliderValue`].
fn sync_material_sliders(
    sliders: Query<(&MatFactorSlider, &SliderValue, &Children)>,
    mut insets: Query<&mut LogicalInset, With<SliderThumb>>,
    mut texts: Query<&mut Text>,
) {
    for (info, value, children) in &sliders {
        let offset = value.0.clamp(0.0, 1.0) * (TRACK_WIDTH - THUMB_WIDTH);
        for child in children.iter() {
            if let Ok(mut inset) = insets.get_mut(child) {
                inset.0.inline_start = Val::Px(offset);
            }
        }
        if let Ok(mut text) = texts.get_mut(info.label) {
            let want = format!("{:.2}", value.0);
            if text.0 != want {
                text.0 = want;
            }
        }
    }
}

/// Save (write the edited material back onto the item) or Revert.
fn on_mat_action_button(
    press: On<Pointer<Press>>,
    buttons: Query<&MatButton>,
    mut state: ResMut<MatEditState>,
    mut commands: MessageWriter<SlCommand>,
    mut texts: Query<&mut Text>,
) {
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
        MatButton::Save => {
            commands.write(SlCommand(Command::UpdateInventoryAsset {
                location: AssetUpdateLocation::AgentInventory {
                    item_id: edit.item.item_id,
                },
                asset_type: UpdatableAssetType::Material,
                data: encode_material_asset(&edit.edited),
            }));
            edit.saving = true;
            set_text(&mut texts, edit.status, "Saving…");
        }
        MatButton::Revert => {
            edit.edited = edit.original;
            // Rebuild the swatches / sliders from the restored material.
            edit.phase = MatPhase::Rebuild;
            edit.dirty = true;
            set_text(&mut texts, edit.status, "Reverted.");
        }
    }
}

/// Report a Save's outcome from the CAPS uploader reply.
fn report_material_save(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<MatEditState>,
    mut texts: Query<&mut Text>,
) {
    let Some(edit) = state.active.as_mut() else {
        events.clear();
        return;
    };
    if !edit.saving {
        events.clear();
        return;
    }
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AssetUploaded { .. } => {
                edit.saving = false;
                set_text(&mut texts, edit.status, "Saved.");
            }
            SlSessionEvent::AssetUploadFailed { .. } => {
                edit.saving = false;
                set_text(&mut texts, edit.status, "Save failed.");
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// The base-colour factor as a Bevy colour (linear RGB; glTF stores factors
/// linear).
fn base_linear_color(material: &GltfMaterial) -> Color {
    Color::linear_rgb(
        material.base_color.first().copied().unwrap_or(1.0),
        material.base_color.get(1).copied().unwrap_or(1.0),
        material.base_color.get(2).copied().unwrap_or(1.0),
    )
}

/// The texture-swatch value for an optional material texture (nil when absent).
fn texture_key(texture: Option<GltfTexture>) -> TextureKey {
    texture.map_or_else(|| TextureKey::from(Uuid::nil()), |texture| texture.id)
}

/// A slot's current texture on the edited material (to preserve its UV transform
/// across a re-pick).
const fn slot_texture(material: &GltfMaterial, slot: MatTexSlot) -> Option<GltfTexture> {
    match slot {
        MatTexSlot::Base => material.base_color_texture,
        MatTexSlot::MetallicRoughness => material.metallic_roughness_texture,
        MatTexSlot::Normal => material.normal_texture,
        MatTexSlot::Emissive => material.emissive_texture,
    }
}

/// Build the material texture reference for a pick: `None` for a nil (cleared)
/// pick, else the picked id keeping any existing UV transform.
fn material_texture(picked: TextureKey, existing: Option<GltfTexture>) -> Option<GltfTexture> {
    if picked.uuid().is_nil() {
        return None;
    }
    Some(GltfTexture {
        id: picked,
        transform: existing
            .map(|texture| texture.transform)
            .unwrap_or_default(),
    })
}

/// The display name of an alpha mode.
const fn alpha_mode_name(mode: GltfAlphaMode) -> &'static str {
    match mode {
        GltfAlphaMode::Opaque => "Opaque",
        GltfAlphaMode::Mask => "Mask",
        GltfAlphaMode::Blend => "Blend",
    }
}

/// The next alpha mode in the Opaque → Mask → Blend cycle.
const fn next_alpha_mode(mode: GltfAlphaMode) -> GltfAlphaMode {
    match mode {
        GltfAlphaMode::Opaque => GltfAlphaMode::Mask,
        GltfAlphaMode::Mask => GltfAlphaMode::Blend,
        GltfAlphaMode::Blend => GltfAlphaMode::Opaque,
    }
}

/// The checkbox glyph for a boolean.
const fn toggle_glyph(on: bool) -> &'static str {
    if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }
}

/// Set a node's text if present.
fn set_text(texts: &mut Query<&mut Text>, node: Option<Entity>, message: &str) {
    if let Some(node) = node
        && let Ok(mut text) = texts.get_mut(node)
    {
        message.clone_into(&mut text.0);
    }
}
