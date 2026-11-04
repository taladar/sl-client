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
//! A click on a material **embedded in a notecard** opens it here too — to look
//! at and play with, not to save: it is not an item of the agent's inventory,
//! so the reference's Save is disabled for it and ours is absent.
//!
//! This is the standalone-floater half of the material editing the viewer
//! already does inline in the Build Tools Texture tab (`edit_material`): here the
//! subject is an inventory asset previewed on a sphere, not a selected in-world
//! face.
//!
//! # One window per material
//!
//! The editor is a **keyed floater** ([`FloaterKey`], `viewer-key-material-
//! editor`): opening a second material opens a second window rather than
//! re-pointing the first, so two variants of one surface can be edited side by
//! side, and opening one can never discard another's unsaved changes. A window
//! is keyed by its inventory item; a material embedded in a notecard by the
//! notecard (and the prim holding it) as well, since its item id may be the id
//! of the resident's own inventory copy it was made from — and that one can be
//! saved, this one cannot. Re-opening a material already up only **raises** its
//! window and never reloads it.
//!
//! Each window's edit (`MatEdit`) is a component on that window, and every
//! system and observer here reaches its edit through the window the control
//! lives in ([`FloaterHost`]): a picker reply names its swatch, a commit its
//! field, a press its button. Closing a window ends it and its edit.
//!
//! - **The preview.** Each window has its own preview sphere: every
//!   [`MaterialPreview`] node is bound to a studio of its own on a render layer
//!   of its own, so two previews never share a sphere or show in each other's
//!   pane. Nothing is arbitrated: the second window's preview is exactly the
//!   first's.
//! - **The decode.** [`MaterialManager`] fetches and decodes per asset, so two
//!   windows loading at once each wait on their own asset id, and two windows
//!   on items sharing one asset are served by one fetch.
//! - **Save.** A save is matched back to the window that sent it by the item it
//!   wrote. The upload reply may not name the item (OpenSim's answers with the
//!   new asset alone), so it is the window that knows which item it saved that
//!   rebinds the inventory model to the new asset — otherwise the next open of
//!   that material would load the asset it had *before* the save. A failed
//!   upload names nothing at all, so it is reported in every window with a
//!   save in flight.
//!
//! Reference (Firestorm, read-only): `llmaterialeditor`,
//! `floater_material_editor.xml` (`LLMaterialEditor` is registered per item).

use crate::skin_palette::SkinPalette;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{Activate, ValueChange};

use crate::ui_checkbox::{CheckboxSpec, spawn_checkbox};
use sl_client_bevy::{
    AssetKey, AssetUpdateLocation, Command, GltfAlphaMode, GltfMaterial, GltfTexture, InventoryKey,
    ItemInfo, SlCommand, SlEvent, SlSessionEvent, TextureKey, UpdatableAssetType, Uuid,
    encode_material_asset,
};

use crate::floater::{
    FloaterCaps, FloaterHandle, FloaterHost, FloaterKey, FloaterSpec, FloaterSystems,
    KeyedFloaterOpen, KeyedFloaters,
};
use crate::intents::{NotecardSource, TexturePicked};
use crate::inventory::{InventoryModel, OpenMaterialEditor};
use crate::material_preview::MaterialPreview;
use crate::materials::MaterialManager;
use crate::ui::row;
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_font::UiFont;
use crate::ui_spawn::{self, ButtonKind, ButtonSpec, LabeledRowSpec, UiLabel};
use crate::ui_spinner::{FieldCommits, SpinStep, SpinnerSpec, format_spin_value, spawn_spinner};
use crate::ui_text::set_node_text;
use crate::ui_text_input::{TextInputKind, TextInputSpec};
use crate::ui_texture_picker::{TextureSwatchValue, spawn_texture_swatch};
use sl_viewer_ui_core::skin::text_role;

/// The chrome font size, in logical pixels.
const FONT: f32 = 13.0;

/// The labelled rows' label-column width at [`FONT`], in logical pixels.
const LABEL_WIDTH: f32 = 110.0;

/// A text column's width at `font_size`: `width` at [`FONT`], and in
/// proportion above it, so a larger font's label stays inside its column.
/// Still a fixed width at a given font size, so successive rows keep their
/// controls aligned.
fn column_width(width: f32, font_size: f32) -> f32 {
    width.max((font_size * width / FONT).ceil())
}

/// The preview sphere pane's side length, in logical pixels.
const PREVIEW_SIZE: f32 = 128.0;

/// The label colour.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// A control's border colour.
const CONTROL_BORDER: Color = Color::srgba(0.34, 0.40, 0.52, 1.0);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

// ---------------------------------------------------------------------------
// Messages, components and resources.
// ---------------------------------------------------------------------------

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

/// A scalar material factor, on the spinner field that edits it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum MatFactor {
    /// The metallic factor.
    Metallic,
    /// The roughness factor.
    Roughness,
    /// The alpha cutoff (Mask mode).
    Cutoff,
}

impl MatFactor {
    /// The spinner's element id — one per factor, so each is its own node.
    const fn element(self) -> &'static str {
        match self {
            Self::Metallic => "material-metallic",
            Self::Roughness => "material-roughness",
            Self::Cutoff => "material-alpha-cutoff",
        }
    }
}

/// What one arrow step of a factor spinner does — the reference
/// `panel_gltf_material.xml` metallic / roughness / alpha-cutoff factor
/// spinners: a hundredth within 0–1, three decimals.
const FACTOR_STEP: SpinStep = SpinStep::new(0.01, 0.0, 1.0, 3);

/// A factor spinner's field width, in `"0"`-glyph advances.
const FACTOR_FIELD_GLYPHS: f32 = 6.0;

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

/// One window's material edit — a **component on the window**, since the
/// editor opens per material ([`FloaterKey`]) and the edit lives exactly as
/// long as its window.
#[derive(Component)]
struct MatEdit {
    /// The window's content column, which the controls are (re)built into.
    content: Entity,
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
    /// Whether there is anything for Save to write back onto: `false` for a
    /// material read out of a notecard, which is not an item of the agent's
    /// inventory (the reference disables Save for one).
    can_save: bool,
    /// The preview sphere node.
    preview: Option<Entity>,
    /// The alpha-mode button's label node.
    alpha_label: Option<Entity>,
    /// The double-sided checkbox (its `Checked` is the state).
    double_check: Option<Entity>,
    /// The status readout node.
    status: Option<Entity>,
}

/// The material-editor plugin.
#[derive(Debug)]
pub struct EditMaterialAssetPlugin;

impl Plugin for EditMaterialAssetPlugin {
    /// Register the open message and the open / per-window systems.
    ///
    /// Nothing spawns at `Startup`: a material window exists only while that
    /// material is open, so `open_material_editor` spawns the instance. The
    /// per-window systems are gated on there being a window.
    fn build(&self, app: &mut App) {
        crate::ui_spinner::ensure_spinner_widget(app);
        app.add_message::<OpenMaterialEditor>().add_systems(
            Update,
            (
                // After the manager's command pass — see `FloaterSystems`: the
                // click that opens a material (an inventory row, a notecard
                // item) also raises the window it landed in, and the later
                // raise wins.
                open_material_editor.after(FloaterSystems::Commands),
                (
                    populate_material_editor,
                    apply_mat_texture_picked,
                    apply_mat_color_picked,
                    drive_material_preview,
                    commit_material_factors,
                    report_material_save,
                )
                    .chain()
                    .run_if(any_with_component::<MatEdit>),
            )
                .chain(),
        );
    }
}

/// The material editor floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn material_editor_floater_spec() -> FloaterSpec {
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
    }
}

// ---------------------------------------------------------------------------
// Open — one window per material; request the asset, build once it decodes.
// ---------------------------------------------------------------------------

/// The [`FloaterKey`] of the window editing `item`.
///
/// A material **embedded in a notecard** is keyed by the notecard (and the prim
/// holding it) too: its item id is the one it had when it was dropped in, which
/// may well be the id of the resident's own inventory material it was made
/// from, and the two are different windows — one saveable, one not.
fn material_key(item: InventoryKey, notecard: Option<NotecardSource>) -> FloaterKey {
    match notecard {
        None => FloaterKey::subject(&item),
        Some(source) => {
            let holder = source
                .object_id()
                .map_or_else(String::new, |object| object.to_string());
            FloaterKey::subject(&format!("{holder}/{}/{item}", source.item_id()))
        }
    }
}

/// Open a material **per material** (`viewer-key-material-editor`): raise its
/// window when it is already up, and otherwise spawn one with a loading note,
/// hang a fresh [`MatEdit`] off it and request the asset. The controls are
/// built by [`populate_material_editor`] once the decode lands.
///
/// Every open of the frame is honoured, not just the last. A window already up
/// is only raised — never reloaded — because reloading it would throw away
/// exactly what a second open must not touch: the resident's unsaved edits.
fn open_material_editor(
    mut opens: MessageReader<OpenMaterialEditor>,
    mut floaters: KeyedFloaters,
    mut materials: ResMut<MaterialManager>,
    mut commands: Commands,
) {
    for open in opens.read() {
        let key = material_key(open.item.item_id, open.notecard);
        let KeyedFloaterOpen::Spawned(handle) = floaters.open(material_editor_floater_spec(), key)
        else {
            continue;
        };
        let item = open.item.clone();
        let asset = AssetKey::from(item.asset_id);
        build_material_window(&mut commands, handle, item, open.notecard.is_none());
        materials.request_material(asset);
    }
}

/// Furnish a freshly spawned material window: its title, a loading line, and
/// the [`MatEdit`] the per-window systems find it by.
fn build_material_window(
    commands: &mut Commands,
    handle: FloaterHandle,
    item: ItemInfo,
    can_save: bool,
) {
    // The title node carries no Fluent key, so a direct text set is not fought
    // by the translator.
    commands
        .entity(handle.title_text)
        .insert(Text::new(format!("Edit: {}", item.name)));
    commands.spawn((
        Text::new("Loading material…"),
        UiFont::Sans.at(FONT),
        text_role(LABEL_COLOR),
        ChildOf(handle.content),
    ));
    let asset = AssetKey::from(item.asset_id);
    commands.entity(handle.root).insert(MatEdit {
        content: handle.content,
        item,
        asset,
        original: GltfMaterial::default(),
        edited: GltfMaterial::default(),
        phase: MatPhase::Loading,
        dirty: false,
        saving: false,
        can_save,
        preview: None,
        alpha_label: None,
        double_check: None,
        status: None,
    });
}

/// Build each window's controls once its material asset has decoded, and
/// rebuild them from the edited material after a Revert.
fn populate_material_editor(
    materials: Res<MaterialManager>,
    mut edits: Query<&mut MatEdit>,
    mut commands: Commands,
) {
    for mut edit in &mut edits {
        // Build on first decode, or rebuild the controls from the edited
        // material after a Revert; otherwise nothing to do.
        let material = match edit.phase {
            MatPhase::Loading => {
                let Some(material) = materials.decoded_material(edit.asset).copied() else {
                    continue;
                };
                edit.original = material;
                edit.edited = material;
                material
            }
            MatPhase::Rebuild => edit.edited,
            MatPhase::Ready => continue,
        };
        edit.phase = MatPhase::Ready;
        edit.dirty = true;

        commands.entity(edit.content).despawn_related::<Children>();
        let controls =
            spawn_material_controls(&mut commands, edit.content, FONT, &material, edit.can_save);
        edit.preview = Some(controls.preview);
        edit.alpha_label = Some(controls.alpha_label);
        edit.double_check = Some(controls.double_check);
        edit.status = Some(controls.status);
    }
}

/// The control entities [`spawn_material_controls`] returns, the ones the
/// edit's systems update afterwards.
#[derive(Debug, Clone, Copy)]
struct MatControls {
    /// The preview sphere node.
    preview: Entity,
    /// The alpha-mode button's label node.
    alpha_label: Entity,
    /// The double-sided checkbox.
    double_check: Entity,
    /// The status readout node.
    status: Entity,
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The material editor's gallery / `ui_test` specimen: the live controls,
/// built by the same `spawn_material_controls` at the cell's font size from
/// a sample material — a tinted, half-metallic, alpha-masked, double-sided
/// surface with no texture maps (a specimen fetches nothing), so every swatch,
/// slider readout and toggle shows a non-default value.
pub fn spawn_material_editor_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let material = GltfMaterial {
        base_color: [0.62, 0.34, 0.18, 1.0],
        metallic_factor: 0.5,
        roughness_factor: 0.35,
        emissive_factor: [0.05, 0.1, 0.3],
        alpha_mode: GltfAlphaMode::Mask,
        alpha_cutoff: 0.4,
        double_sided: true,
        ..GltfMaterial::default()
    };
    spawn_material_controls(commands, parent, cx.font_size, &material, true);
    parent
}

/// Build the editor's controls for `material` into `content` at `font_size`:
/// the Save / Revert row, the status line, the preview sphere and one row per
/// channel. Shared by the live editor and its specimen.
///
/// Without `can_save` — a material read out of a notecard — the row has no
/// Save, and the status line says why: the edits still preview, and Revert
/// still restores, but there is no item of the agent's to write them onto.
fn spawn_material_controls(
    commands: &mut Commands,
    content: Entity,
    font_size: f32,
    material: &GltfMaterial,
    can_save: bool,
) -> MatControls {
    let mut tab = 0_i32;

    // Action buttons + status.
    let button_row = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    for (kind, label) in [(MatButton::Save, "Save"), (MatButton::Revert, "Revert")] {
        if kind == MatButton::Save && !can_save {
            continue;
        }
        spawn_mat_button(commands, button_row, kind, label, &mut tab, font_size);
    }
    let note = if can_save {
        String::new()
    } else {
        "In a notecard: copy it to your inventory to save changes.".to_owned()
    };
    let status = commands
        .spawn((
            Text::new(note),
            UiFont::Sans.at(font_size),
            text_role(LABEL_COLOR),
            Name::new("material-status"),
            Node {
                margin: UiRect::bottom(Val::Px(4.0)),
                ..Default::default()
            },
            ChildOf(content),
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
            MaterialPreview::Material(Box::new(*material)),
            ChildOf(content),
        ))
        .id();

    // Base colour: tint swatch + texture.
    let base_row = spawn_labeled_row(commands, content, "Base Color", font_size);
    let base_color = base_linear_color(material);
    let swatch = spawn_color_swatch(commands, base_row, "material-base", tab, base_color);
    commands.entity(swatch).insert(MatColorSlot::Base);
    tab = tab.saturating_add(1);
    let base_tex = spawn_texture_swatch(
        commands,
        base_row,
        "material-base-tex",
        tab,
        texture_key(material.base_color_texture),
    );
    commands.entity(base_tex).insert(MatTexSlot::Base);
    tab = tab.saturating_add(1);

    // Metallic / roughness: two factor sliders + the packed texture.
    spawn_factor_spinner(
        commands,
        content,
        "Metallic",
        MatFactor::Metallic,
        material.metallic_factor,
        &mut tab,
        font_size,
    );
    spawn_factor_spinner(
        commands,
        content,
        "Roughness",
        MatFactor::Roughness,
        material.roughness_factor,
        &mut tab,
        font_size,
    );
    let mr_row = spawn_labeled_row(commands, content, "Metal/Rough Map", font_size);
    let mr_tex = spawn_texture_swatch(
        commands,
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
    let normal_row = spawn_labeled_row(commands, content, "Normal Map", font_size);
    let normal_tex = spawn_texture_swatch(
        commands,
        normal_row,
        "material-normal-tex",
        tab,
        texture_key(material.normal_texture),
    );
    commands.entity(normal_tex).insert(MatTexSlot::Normal);
    tab = tab.saturating_add(1);

    // Emissive: tint swatch + texture.
    let emissive_row = spawn_labeled_row(commands, content, "Emissive", font_size);
    let emissive_color = Color::linear_rgb(
        material.emissive_factor.first().copied().unwrap_or(0.0),
        material.emissive_factor.get(1).copied().unwrap_or(0.0),
        material.emissive_factor.get(2).copied().unwrap_or(0.0),
    );
    let em_swatch = spawn_color_swatch(
        commands,
        emissive_row,
        "material-emissive",
        tab,
        emissive_color,
    );
    commands.entity(em_swatch).insert(MatColorSlot::Emissive);
    tab = tab.saturating_add(1);
    let em_tex = spawn_texture_swatch(
        commands,
        emissive_row,
        "material-emissive-tex",
        tab,
        texture_key(material.emissive_texture),
    );
    commands.entity(em_tex).insert(MatTexSlot::Emissive);
    tab = tab.saturating_add(1);

    // Alpha mode (cycle) + cutoff.
    let alpha_row = spawn_labeled_row(commands, content, "Alpha Mode", font_size);
    let alpha_label = spawn_text_button(
        commands,
        alpha_row,
        alpha_mode_name(material.alpha_mode),
        MatAlphaButton,
        &mut tab,
        font_size,
    );
    spawn_factor_spinner(
        commands,
        content,
        "Alpha Cutoff",
        MatFactor::Cutoff,
        material.alpha_cutoff,
        &mut tab,
        font_size,
    );

    // Double-sided toggle: the shared checkbox widget, caption-less because this
    // editor's rows carry their label in a leading column (as the alerts table
    // does) rather than beside each control.
    let double_row = spawn_labeled_row(commands, content, "Double Sided", font_size);
    let double_check = spawn_checkbox(
        commands,
        double_row,
        &CheckboxSpec {
            element: "material-double-sided",
            label: String::new(),
            tab_index: tab,
            font_size,
            translate_label: false,
        },
    );
    commands
        .entity(double_check.checkbox)
        .insert(MatDoubleSidedButton)
        .observe(on_mat_double_sided);
    if material.double_sided {
        commands.entity(double_check.checkbox).insert(Checked);
    }

    MatControls {
        preview,
        alpha_label,
        double_check: double_check.checkbox,
        status,
    }
}

// ---------------------------------------------------------------------------
// Control spawn helpers.
// ---------------------------------------------------------------------------

/// Spawn a labelled row at `font_size` and return the row entity to parent the
/// control into.
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
            .label_width(Val::Px(column_width(LABEL_WIDTH, font_size)))
            .margin(UiRect::bottom(Val::Px(3.0))),
    )
    .row
}

/// Spawn a factor spinner row (`0..=1`) at `font_size`, its field tagged with
/// its [`MatFactor`].
fn spawn_factor_spinner(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    kind: MatFactor,
    value: f32,
    tab: &mut i32,
    font_size: f32,
) {
    let row_entity = spawn_labeled_row(commands, parent, label, font_size);
    let field = spawn_spinner(
        commands,
        row_entity,
        &SpinnerSpec {
            input: TextInputSpec {
                initial: format_spin_value(f64::from(value.clamp(0.0, 1.0)), FACTOR_STEP.decimals),
                font_size,
                width_glyphs: FACTOR_FIELD_GLYPHS,
                tab_index: *tab,
                ..TextInputSpec::new(kind.element(), TextInputKind::Float)
            },
            step: FACTOR_STEP,
        },
    )
    .field;
    commands.entity(field).insert(kind);
    *tab = tab.saturating_add(1);
}

/// Spawn a bordered text button at `font_size` carrying `marker`, and return
/// its label node so the caller can update the button's text later.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    marker: impl Component,
    tab: &mut i32,
    font_size: f32,
) -> Entity {
    let spawned = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::literal(label), "material-text-button")
            .kind(ButtonKind::Headless)
            .tab_from(tab)
            .padding(10.0, 3.0)
            .colors(BUTTON_BACKGROUND, CONTROL_BORDER)
            .label_color(LABEL_COLOR)
            .font_size(font_size),
    );
    commands
        .entity(spawned.button)
        .insert(marker)
        .observe(on_mat_toggle);
    spawned.label
}

/// Spawn one chrome action button (Save / Revert) at `font_size`.
fn spawn_mat_button(
    commands: &mut Commands,
    parent: Entity,
    kind: MatButton,
    label: &str,
    tab: &mut i32,
    font_size: f32,
) {
    let button = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::literal(label.to_owned()),
            format!("material-button:{label}"),
        )
        .kind(ButtonKind::Headless)
        .tab_from(tab)
        .padding(10.0, 3.0)
        .colors(BUTTON_BACKGROUND, CONTROL_BORDER)
        .label_color(LABEL_COLOR)
        .font_size(font_size),
    )
    .button;
    commands
        .entity(button)
        .insert(kind)
        .observe(on_mat_action_button);
}

// ---------------------------------------------------------------------------
// Edit handlers.
// ---------------------------------------------------------------------------

/// A factor spinner's commit — `Enter`, focus leaving it, or an arrow step
/// ([`FieldCommits`]): clamp, record the edit, mark dirty. A field holding no
/// number records nothing.
fn commit_material_factors(
    mut commits: FieldCommits,
    fields: Query<(&MatFactor, &EditableText)>,
    host: FloaterHost,
    mut edits: Query<&mut MatEdit>,
) {
    for commit in commits.take(|entity| fields.contains(entity)) {
        let Ok((kind, editor)) = fields.get(commit.field) else {
            continue;
        };
        let Some(mut edit) = host
            .of(commit.field)
            .and_then(|window| edits.get_mut(window).ok())
        else {
            continue;
        };
        let Some(value) = commit
            .text(editor)
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
        else {
            continue;
        };
        let clamped = value.clamp(0.0, 1.0);
        match kind {
            MatFactor::Metallic => edit.edited.metallic_factor = clamped,
            MatFactor::Roughness => edit.edited.roughness_factor = clamped,
            MatFactor::Cutoff => edit.edited.alpha_cutoff = clamped,
        }
        edit.dirty = true;
    }
}

/// A texture pick: record the new map (or clear it on a nil pick) in the edit
/// of the window the swatch lives in, repaint the swatch, mark dirty.
fn apply_mat_texture_picked(
    mut picks: MessageReader<TexturePicked>,
    slots: Query<&MatTexSlot>,
    mut values: Query<&mut TextureSwatchValue>,
    host: FloaterHost,
    mut edits: Query<&mut MatEdit>,
) {
    for pick in picks.read() {
        let Ok(slot) = slots.get(pick.requester) else {
            continue;
        };
        let Some(mut edit) = host
            .of(pick.requester)
            .and_then(|window| edits.get_mut(window).ok())
        else {
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

/// A colour pick: write the base or emissive factor in the edit of the window
/// the swatch lives in, repaint the swatch, mark dirty.
fn apply_mat_color_picked(
    mut picks: MessageReader<ColorPicked>,
    slots: Query<&MatColorSlot>,
    mut values: Query<&mut ColorSwatchValue>,
    host: FloaterHost,
    mut edits: Query<&mut MatEdit>,
) {
    for pick in picks.read() {
        let Ok(slot) = slots.get(pick.requester) else {
            continue;
        };
        let Some(mut edit) = host
            .of(pick.requester)
            .and_then(|window| edits.get_mut(window).ok())
        else {
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

/// A text button was activated — by a primary click, `Enter` or `Space`:
/// cycle the alpha mode.
fn on_mat_toggle(
    activate: On<Activate>,
    alpha: Query<&MatAlphaButton>,
    host: FloaterHost,
    mut edits: Query<&mut MatEdit>,
    mut texts: Query<&mut Text>,
) {
    // No edit only in the gallery, whose specimen has no window of its own.
    let Some(mut edit) = host
        .of(activate.entity)
        .and_then(|window| edits.get_mut(window).ok())
    else {
        return;
    };
    if alpha.get(activate.entity).is_ok() {
        edit.edited.alpha_mode = next_alpha_mode(edit.edited.alpha_mode);
        set_node_text(
            &mut texts,
            edit.alpha_label,
            alpha_mode_name(edit.edited.alpha_mode),
        );
        edit.dirty = true;
    }
}

/// The double-sided checkbox was toggled: the widget has already moved its own
/// tick, so this writes the draft and nothing else.
fn on_mat_double_sided(
    change: On<ValueChange<bool>>,
    double: Query<&MatDoubleSidedButton>,
    host: FloaterHost,
    mut edits: Query<&mut MatEdit>,
    mut commands: Commands,
) {
    if double.get(change.source).is_err() {
        return;
    }
    // No edit only in the gallery, whose specimen has no window of its own.
    let Some(mut edit) = host
        .of(change.source)
        .and_then(|window| edits.get_mut(window).ok())
    else {
        // No material open: the box has no draft to write to, so put its tick
        // back rather than leaving it claiming something.
        if change.value {
            commands.entity(change.source).remove::<Checked>();
        }
        return;
    };
    edit.edited.double_sided = change.value;
    edit.dirty = true;
}

/// Refresh each window's preview sphere when its edited material changed.
fn drive_material_preview(
    mut edits: Query<&mut MatEdit>,
    mut previews: Query<&mut MaterialPreview>,
) {
    for mut edit in &mut edits {
        if !edit.dirty {
            continue;
        }
        if let Some(node) = edit.preview
            && let Ok(mut preview) = previews.get_mut(node)
        {
            *preview = MaterialPreview::Material(Box::new(edit.edited));
        }
        edit.dirty = false;
    }
}

/// Save (write the edited material back onto the item) or Revert, from a
/// primary click or from `Enter` / `Space` on the focused button alike.
fn on_mat_action_button(
    activate: On<Activate>,
    buttons: Query<&MatButton>,
    host: FloaterHost,
    mut edits: Query<&mut MatEdit>,
    commands: Option<MessageWriter<SlCommand>>,
    mut texts: Query<&mut Text>,
) {
    let Ok(kind) = buttons.get(activate.entity).copied() else {
        return;
    };
    // Both are absent only in the gallery, whose specimen has no edit to save.
    let Some(mut commands) = commands else {
        return;
    };
    let Some(mut edit) = host
        .of(activate.entity)
        .and_then(|window| edits.get_mut(window).ok())
    else {
        return;
    };
    match kind {
        // Never spawned without `can_save`; checked again rather than trusted,
        // since a save with nothing to write onto would overwrite whatever
        // inventory item happens to share the embedded item's id.
        MatButton::Save if !edit.can_save => {}
        MatButton::Save => {
            commands.write(SlCommand(Command::UpdateInventoryAsset {
                location: AssetUpdateLocation::AgentInventory {
                    item_id: edit.item.item_id,
                },
                asset_type: UpdatableAssetType::Material,
                data: encode_material_asset(&edit.edited),
            }));
            edit.saving = true;
            set_node_text(&mut texts, edit.status, "Saving…");
        }
        MatButton::Revert => {
            edit.edited = edit.original;
            // Rebuild the swatches / sliders from the restored material.
            edit.phase = MatPhase::Rebuild;
            edit.dirty = true;
            set_node_text(&mut texts, edit.status, "Reverted.");
        }
    }
}

/// Report a Save's outcome from the CAPS uploader reply, in the window that
/// saved.
///
/// The reply is matched by the item it names; one that names none is taken as
/// this window's rather than leaving its status stuck on "Saving…". A landed
/// save rebinds the item to its new asset in the inventory model, driven from
/// the item this window saved — not from the reply, which may carry the asset
/// alone — so the next open of this material loads what was just saved.
fn report_material_save(
    mut events: MessageReader<SlEvent>,
    mut edits: Query<&mut MatEdit>,
    mut inventory: Option<ResMut<InventoryModel>>,
    mut texts: Query<&mut Text>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for mut edit in &mut edits {
        for event in &frame {
            if !edit.saving {
                break;
            }
            match &event.0 {
                SlSessionEvent::AssetUploaded {
                    new_asset,
                    new_inventory_item,
                    // A save replaces the asset of an item that already exists;
                    // only an upload that creates one carries it.
                    created: _,
                } => {
                    if matches!(new_inventory_item, Some(id) if *id != edit.item.item_id.uuid()) {
                        continue;
                    }
                    edit.saving = false;
                    edit.asset = AssetKey::from(*new_asset);
                    edit.item.asset_id = *new_asset;
                    if let Some(model) = inventory.as_mut() {
                        let _folder = model.rebind_asset(edit.item.item_id, *new_asset);
                    }
                    set_node_text(&mut texts, edit.status, "Saved.");
                }
                // A failure names no item, so every window with a save in
                // flight hears it.
                SlSessionEvent::AssetUploadFailed { reason } => {
                    warn!("material save failed: {reason}");
                    edit.saving = false;
                    set_node_text(&mut texts, edit.status, "Save failed.");
                }
                _other => {}
            }
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

#[cfg(test)]
mod tests {
    use super::{
        MatAlphaButton, MatButton, MatEdit, MatPhase, material_editor_floater_spec,
        spawn_mat_button, spawn_text_button,
    };
    use crate::floater::{FloaterPlugin, spawn_floater};
    use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems};
    use bevy::input::keyboard::Key;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, AssetKey, AssetType, GltfAlphaMode, GltfMaterial, InventoryFolderKey,
        InventoryKey, InventoryType, ItemInfo, OwnerKey, Permissions, Permissions5, SlCommand,
        Uuid,
    };
    use sl_viewer_testkit::interact::{self, InteractionTest};
    use sl_viewer_testkit::{find_by_name, settle};

    /// The Revert button's node name, per `spawn_mat_button`.
    const REVERT: &str = "material-button:Revert";

    /// The alpha-mode button's node name, per `spawn_text_button`.
    const ALPHA: &str = "material-text-button";

    /// One way of pressing a button: a click, or a key on the focused button.
    type Gesture = fn(&mut App, Entity) -> Result<(), String>;

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

    /// Every way a button is pressed, named for a failure message.
    const GESTURES: [(&str, Gesture); 3] = [("click", click), ("Enter", enter), ("Space", space)];

    /// The material item being edited.
    fn item() -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(0x11)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(0x22)),
            name: "A material".to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(0x33),
            asset_type: AssetType::Material,
            inv_type: InventoryType::Material,
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

    /// An editor window with an open, edited material, its Revert and
    /// alpha-mode buttons, and a status readout — under the real input and
    /// focus stack. The buttons sit in the window, as they do live, since each
    /// reaches its edit through the window it lives in.
    fn editor_app() -> Result<App, String> {
        let mut app = InteractionTest::new().build();
        app.add_message::<SlCommand>().add_plugins(FloaterPlugin);
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                let window = spawn_floater(&mut commands, root.0, material_editor_floater_spec());
                // A floater spawns hidden and its opener shows it; a hidden
                // window has no box for a pointer to find.
                commands.entity(window.root).insert(UiPanelShown(true));
                let content = window.content;
                let mut tab = 0_i32;
                spawn_mat_button(
                    &mut commands,
                    content,
                    MatButton::Revert,
                    "Revert",
                    &mut tab,
                    13.0,
                );
                let alpha_label = spawn_text_button(
                    &mut commands,
                    content,
                    "Opaque",
                    MatAlphaButton,
                    &mut tab,
                    13.0,
                );
                let status = commands
                    .spawn((Text::new(String::new()), ChildOf(content)))
                    .id();
                let edited = GltfMaterial {
                    double_sided: true,
                    ..GltfMaterial::default()
                };
                commands.entity(window.root).insert(MatEdit {
                    content,
                    item: item(),
                    asset: AssetKey::from(Uuid::from_u128(0x33)),
                    original: GltfMaterial::default(),
                    edited,
                    phase: MatPhase::Ready,
                    dirty: false,
                    saving: false,
                    can_save: true,
                    preview: None,
                    alpha_label: Some(alpha_label),
                    double_check: None,
                    status: Some(status),
                });
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        let open = app
            .world_mut()
            .query::<&MatEdit>()
            .iter(app.world())
            .count();
        if open == 1 {
            Ok(app)
        } else {
            Err(format!("expected one open edit, found {open}"))
        }
    }

    /// The open edit's state after `gesture` on the node named `node`: its
    /// status line, whether it went back to the original material, its phase
    /// and its alpha mode.
    fn after(
        node: &str,
        gesture: Gesture,
    ) -> Result<(String, bool, MatPhase, GltfAlphaMode), String> {
        let mut app = editor_app()?;
        let button = find_by_name(&mut app, node).ok_or_else(|| format!("no `{node}`"))?;
        gesture(&mut app, button)?;
        settle(&mut app);
        let edit = app
            .world_mut()
            .query::<&MatEdit>()
            .iter(app.world())
            .next()
            .ok_or("the edit went away")?;
        let status = edit
            .status
            .and_then(|status| app.world().get::<Text>(status))
            .map(|text| text.0.clone())
            .unwrap_or_default();
        Ok((
            status,
            edit.edited == edit.original,
            edit.phase,
            edit.edited.alpha_mode,
        ))
    }

    /// **Revert acts from the keyboard as from the mouse.**
    ///
    /// The button used to observe `Pointer<Press>`, so `Tab` reached it and
    /// `Enter` / `Space` did nothing (`viewer-floater-buttons-ignore-keyboard-
    /// activate`).
    #[test]
    fn revert_answers_enter_space_and_a_click() -> Result<(), String> {
        for (how, gesture) in GESTURES {
            assert_eq!(
                after(REVERT, gesture)?,
                (
                    "Reverted.".to_owned(),
                    true,
                    MatPhase::Rebuild,
                    GltfAlphaMode::Opaque
                ),
                "Revert by {how}"
            );
        }
        Ok(())
    }

    /// **The alpha-mode button steps from the keyboard as from the mouse** — once
    /// per gesture, and without touching anything else.
    #[test]
    fn the_alpha_mode_button_answers_enter_space_and_a_click() -> Result<(), String> {
        for (how, gesture) in GESTURES {
            assert_eq!(
                after(ALPHA, gesture)?,
                (String::new(), false, MatPhase::Ready, GltfAlphaMode::Mask),
                "the alpha-mode button by {how}"
            );
        }
        Ok(())
    }

    /// **One window per material** (`viewer-key-material-editor`): the open
    /// path, driven by the message an inventory row's Edit writes, and the
    /// per-window passes behind it.
    ///
    /// What the keying buys is not two windows for their own sake — it is that
    /// opening a second material cannot touch the first window's **unsaved
    /// edit**, which the singleton editor replaced on every open.
    mod instances {
        use super::super::{
            MatColorSlot, MatEdit, MatPhase, apply_mat_color_picked, apply_mat_texture_picked,
            material_key, open_material_editor, populate_material_editor, report_material_save,
        };
        use super::item;
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin, host_floater};
        use crate::intents::{NotecardSource, TexturePicked};
        use crate::inventory::OpenMaterialEditor;
        use crate::materials::MaterialManager;
        use crate::ui::UiRoot;
        use crate::ui_color_picker::ColorPicked;
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{InventoryKey, SlCommand, SlEvent, SlSessionEvent, Uuid};

        /// A boxed error so tests can use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// The first material's item id.
        const FIRST: u128 = 0xA1;

        /// The second material's item id.
        const SECOND: u128 = 0xB2;

        /// An app with the floater manager, the open path and the per-window
        /// passes that do not need a live session: the build, the two picker
        /// replies and the save report. The material manager has no fetch
        /// capability, so a request parks rather than going anywhere.
        fn editor_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<OpenMaterialEditor>()
                .add_message::<ColorPicked>()
                .add_message::<TexturePicked>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<MaterialManager>()
                .add_plugins(FloaterPlugin)
                .add_systems(
                    Update,
                    (
                        open_material_editor,
                        populate_material_editor,
                        apply_mat_texture_picked,
                        apply_mat_color_picked,
                        report_material_save,
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

        /// Open material `id` the way every caller does — by writing the
        /// message — from the inventory, or out of `notecard`.
        fn open(app: &mut App, id: u128, notecard: Option<NotecardSource>) {
            let mut material = item();
            material.item_id = key(id);
            material.asset_id = Uuid::from_u128(id << 8);
            app.world_mut().write_message(OpenMaterialEditor {
                item: material,
                notecard,
            });
            app.update();
        }

        /// Every live material window, as (window, item id) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, InventoryKey)> {
            app.world_mut()
                .query::<(Entity, &MatEdit)>()
                .iter(app.world())
                .map(|(window, edit)| (window, edit.item.item_id))
                .collect()
        }

        /// The window editing the inventory material `id`.
        fn window_of(app: &mut App, id: u128) -> Result<Entity, TestError> {
            windows(app)
                .into_iter()
                .find_map(|(window, item)| (item == key(id)).then_some(window))
                .ok_or_else(|| TestError::from(format!("material {id:#x} has no window")))
        }

        /// The edit in `window`.
        fn edit(app: &App, window: Entity) -> Result<&MatEdit, TestError> {
            app.world()
                .get::<MatEdit>(window)
                .ok_or_else(|| TestError::from("the window has no edit"))
        }

        /// Build `window`'s controls from its edited material, the way a Revert
        /// does — standing in for the decode, which needs a live fetch.
        fn build_controls(app: &mut App, window: Entity) -> Result<(), TestError> {
            app.world_mut()
                .get_mut::<MatEdit>(window)
                .ok_or("the window has no edit")?
                .phase = MatPhase::Rebuild;
            app.update();
            Ok(())
        }

        /// The parent links and windows `host_floater` walks up through.
        type HostLookup<'world, 'state> = (
            Query<'world, 'state, &'static ChildOf>,
            Query<'world, 'state, (Entity, &'static Floater)>,
        );

        /// The base-colour swatch built into `window`.
        fn base_swatch(app: &mut App, window: Entity) -> Result<Entity, TestError> {
            let swatches: Vec<Entity> = app
                .world_mut()
                .query::<(Entity, &MatColorSlot)>()
                .iter(app.world())
                .filter(|(_swatch, slot)| **slot == MatColorSlot::Base)
                .map(|(swatch, _slot)| swatch)
                .collect();
            let mut system_state: bevy::ecs::system::SystemState<HostLookup<'_, '_>> =
                bevy::ecs::system::SystemState::new(app.world_mut());
            let (parents, floaters) = system_state.get(app.world())?;
            swatches
                .into_iter()
                .find(|swatch| host_floater(*swatch, &parents, &floaters) == Some(window))
                .ok_or_else(|| TestError::from("the window has no base-colour swatch"))
        }

        /// Two materials are two windows, each on its own item, with its own
        /// content column, under its own key.
        #[test]
        fn two_materials_open_two_windows() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, FIRST, None);
            open(&mut app, SECOND, None);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second material reused the first window"
            );
            let first = window_of(&mut app, FIRST)?;
            let second = window_of(&mut app, SECOND)?;
            assert!(
                edit(&app, first)?.content != edit(&app, second)?.content,
                "both windows build into one content column"
            );
            let key_of = |window: Entity| {
                app.world()
                    .get::<Floater>(window)
                    .and_then(Floater::key)
                    .cloned()
            };
            assert_eq!(key_of(first), Some(material_key(key(FIRST), None)));
            assert_eq!(key_of(second), Some(material_key(key(SECOND), None)));
            Ok(())
        }

        /// **Re-opening a material that is already up keeps its edit.** A
        /// second open raises the window; it must not reload the material over
        /// what the resident changed and has not saved.
        #[test]
        fn reopening_a_material_keeps_its_unsaved_edit() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, FIRST, None);
            let window = window_of(&mut app, FIRST)?;
            {
                let mut edit = app
                    .world_mut()
                    .get_mut::<MatEdit>(window)
                    .ok_or("the window has no edit")?;
                edit.phase = MatPhase::Ready;
                edit.edited.metallic_factor = 0.9;
            }

            open(&mut app, FIRST, None);
            assert_eq!(windows(&mut app).len(), 1, "a re-open spawned a window");
            let edit = edit(&app, window)?;
            assert_eq!(
                (edit.phase, edit.edited.metallic_factor),
                (MatPhase::Ready, 0.9),
                "a re-open reloaded the material over the unsaved edit"
            );
            Ok(())
        }

        /// A material embedded in a notecard is not the inventory copy it may
        /// share an item id with: it is a window of its own, and one with
        /// nothing for Save to write onto.
        #[test]
        fn an_embedded_material_is_its_own_window() -> Result<(), TestError> {
            let mut app = editor_app();
            let notecard = NotecardSource::Agent { item_id: key(0xC3) };
            open(&mut app, FIRST, None);
            open(&mut app, FIRST, Some(notecard));

            let saveable: Vec<bool> = app
                .world_mut()
                .query::<&MatEdit>()
                .iter(app.world())
                .map(|edit| edit.can_save)
                .collect();
            assert_eq!(saveable.len(), 2, "the embedded copy reused the window");
            assert!(saveable.contains(&true) && saveable.contains(&false));
            Ok(())
        }

        /// Closing one material ends that window and its edit, and leaves the
        /// other open.
        #[test]
        fn closing_one_material_leaves_the_other() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, FIRST, None);
            open(&mut app, SECOND, None);
            let first = window_of(&mut app, FIRST)?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: first,
                    op: FloaterOp::Close,
                });
            app.update();

            let left = windows(&mut app);
            assert_eq!(
                left.iter().map(|(_window, item)| *item).collect::<Vec<_>>(),
                vec![key(SECOND)]
            );
            Ok(())
        }

        /// **A colour pick lands in the window whose swatch asked for it.** The
        /// reply names a swatch, and the swatch's window is the edit it writes
        /// — the other window's material is untouched.
        #[test]
        fn a_colour_pick_changes_only_its_own_window() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, FIRST, None);
            open(&mut app, SECOND, None);
            let first = window_of(&mut app, FIRST)?;
            let second = window_of(&mut app, SECOND)?;
            build_controls(&mut app, first)?;
            build_controls(&mut app, second)?;
            let untouched = edit(&app, first)?.edited.base_color;

            let swatch = base_swatch(&mut app, second)?;
            app.world_mut().write_message(ColorPicked {
                requester: swatch,
                color: Color::linear_rgb(1.0, 0.0, 0.0),
                final_pick: true,
            });
            app.update();

            // Bit patterns: the pick is passed through, not computed, so the
            // channels are exact.
            assert_eq!(
                edit(&app, second)?.edited.base_color.map(f32::to_bits),
                [1.0_f32, 0.0, 0.0, 1.0].map(f32::to_bits)
            );
            assert_eq!(
                edit(&app, first)?.edited.base_color.map(f32::to_bits),
                untouched.map(f32::to_bits),
                "a pick in one window changed the other's material"
            );
            Ok(())
        }

        /// **A save is reported in the window that saved.** A reply naming
        /// another item leaves the save in flight; the one naming this window's
        /// item ends it and moves the window onto the asset the save wrote, so
        /// a later save or reload starts from what is stored.
        #[test]
        fn a_save_reply_lands_in_the_window_that_saved() -> Result<(), TestError> {
            let mut app = editor_app();
            open(&mut app, FIRST, None);
            open(&mut app, SECOND, None);
            let first = window_of(&mut app, FIRST)?;
            let second = window_of(&mut app, SECOND)?;
            app.world_mut()
                .get_mut::<MatEdit>(first)
                .ok_or("the window has no edit")?
                .saving = true;

            let saved = Uuid::from_u128(0x5A_7ED);
            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::AssetUploaded {
                    new_asset: saved,
                    new_inventory_item: Some(Uuid::from_u128(SECOND)),
                    created: None,
                }));
            app.update();
            assert!(
                edit(&app, first)?.saving,
                "another item's save reply ended this window's save"
            );

            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::AssetUploaded {
                    new_asset: saved,
                    new_inventory_item: Some(Uuid::from_u128(FIRST)),
                    created: None,
                }));
            app.update();
            let landed = edit(&app, first)?;
            assert_eq!((landed.saving, landed.item.asset_id), (false, saved));
            assert!(
                edit(&app, second)?.item.asset_id != saved,
                "the save rebound a window that did not save"
            );
            Ok(())
        }
    }
}
