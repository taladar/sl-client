//! The Texture tab's **material channels** (`viewer-face-materials-pbr`): the
//! Blinn-Phong legacy `LLMaterial` (normal / specular maps + glossiness /
//! environment / specular colour + the diffuse alpha mode / mask cutoff) and the
//! PBR (GLTF) render-material channels, alongside the diffuse channel that
//! [`crate::edit_texture`] owns.
//!
//! # Model
//!
//! - The three selector widgets (matmedia combo, material-type radio, pbr-type
//!   radio) and the shared [`MatModeState`] / [`ShowWhen`] visibility live in
//!   [`crate::edit_texture`]; this module spawns and drives the per-channel
//!   editors that appear under them.
//! - **Blinn-Phong (`LLMaterial`)**: the normal / specular map swatches, their
//!   repeats / offset / rotation, glossiness, environment intensity, specular
//!   colour and the diffuse alpha mode / mask cutoff all edit the face's
//!   *legacy material* (not its `TextureEntry`). A commit resolves each selected
//!   face's current material (from [`crate::legacy_materials::LegacyMaterialManager`],
//!   or a default when the face has none), applies the one changed attribute, and
//!   sends the whole material for every selected face over the `RenderMaterials`
//!   capability **PUT** ([`Command::SetRenderMaterials`]); the simulator assigns
//!   the material id and echoes it on the faces, exactly like the reference's
//!   `LLMaterialMgr::put`.
//! - **PBR (GLTF)**: the render-material swatch assigns (or clears) a stored
//!   material asset on the selected faces via the `ModifyMaterialParams`
//!   capability ([`Command::ModifyMaterialParams`]) — the reference's
//!   "assign a saved material to faces". The per-channel base-colour /
//!   metallic-roughness / emissive / normal repeats / offset / rotation are
//!   displayed from the face's effective (base + override) material and **edited**
//!   as a per-face GLTF override: the edit amends the face's current override,
//!   serialises it to the GLTF-JSON the cap carries
//!   ([`encode_override_gltf_json`]), and sends it with `ModifyMaterialParams`
//!   (the reference's `updateGLTFTextureTransform` → `LLGLTFMaterialList::queueModify`).
//!   The render-material channel applies a transform edit to every channel at
//!   once, as the reference does.
//!
//! Reference (Firestorm, read-only): `llpanelface`, `llmaterialmgr`,
//! `llmaterialeditor`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use sl_client_bevy::{
    AssetKey, AssetType, Command, FaceMaterialPut, GltfAlphaMode, GltfMaterial, GltfTexture,
    GltfTextureTransform, InventoryType, LegacyMaterial, MaterialOverride, MaterialOverrideUpdate,
    ObjectKey, SlCommand, TextureFace, TextureKey, TextureOverride, TextureTransformOverride, Uuid,
    encode_override_gltf_json,
};

use crate::chat::LocalChatNotice;
use crate::edit_texture::{
    MatModeState, PbrChannel, PrimFaceLookup, ShowWhen, node_face_indices, parse_tex_value,
    primary_face_index, representative_face, spawn_row,
};
use crate::edit_tool::{
    CHECKED_GLYPH, EditToolState, LABEL_CLASS, TOOL_FONT_SIZE, UNCHECKED_GLYPH, VALUE_CLASS,
};
use crate::face_material::{FaceMaterial, MAP_FLAG_NORMAL, MAP_FLAG_SPEC};
use crate::gizmos::{EditPerm, perm_notice};
use crate::legacy_materials::{
    LegacyMaterialManager, apply_legacy_scalars, build_linear_image, build_srgb_image,
    preview_legacy_material,
};
use crate::material_preview::MaterialPreview;
use crate::materials::{MaterialManager, ObjectRenderMaterials};
use crate::objects::{FaceTextureDebug, ObjectState, PrimFaceEntity, SceneObject};
use crate::render_priority::TERRAIN_BOOST_PRIORITY;
use crate::textures::{PrimTextures, TextureAlpha, TextureManager, compose_face_material};
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::ui_texture_picker::{
    MaterialSwatchValue, TexturePicked, TextureSwatchValue, spawn_material_swatch,
    spawn_texture_swatch,
};
use crate::web_floater::set_editor_text;
use crate::world_api::SelectionSet;

/// The width, in `"0"`-glyph advances, of a material-channel numeric field
/// (matching the diffuse fields' width).
const MAT_FIELD_GLYPHS: f32 = 7.0;

/// The `LLMaterial` diffuse-alpha mode: fully opaque, no alpha handling. The
/// blend (`1`) and mask (`2`) modes sit between this and emissive, in the order
/// [`ALPHA_MODE_LABELS`] lists them.
const ALPHA_MODE_NONE: u8 = 0;
/// The `LLMaterial` diffuse-alpha mode: emissive mask (the highest value).
const ALPHA_MODE_EMISSIVE: u8 = 3;

/// The alpha-mode combo option labels, indexed by the wire value.
const ALPHA_MODE_LABELS: [&str; 4] = [
    "build-tex-alpha-none",
    "build-tex-alpha-blend",
    "build-tex-alpha-mask",
    "build-tex-alpha-emissive",
];

/// The GLTF texture slot indices (matching [`MaterialOverride`]'s slot arrays and
/// `sl_material`'s slot order).
const SLOT_BASE_COLOR: usize = 0;
/// The normal texture slot index.
const SLOT_NORMAL: usize = 1;
/// The metallic-roughness texture slot index.
const SLOT_METALLIC_ROUGHNESS: usize = 2;
/// The emissive texture slot index.
const SLOT_EMISSIVE: usize = 3;

/// The next-owner permission mask a saved material grants: copy + modify +
/// transfer (`PERM_COPY | PERM_MODIFY | PERM_TRANSFER`).
const PERM_COPY_MODIFY_TRANSFER: u32 = 0x0000_8000 | 0x0000_4000 | 0x0000_2000;

/// The reference's blank GLTF material asset (`BLANK_MATERIAL_ASSET_ID`,
/// `indra_constants.cpp`): assigning it to a face gives it a default PBR material
/// to then override — the "New material" action.
const BLANK_MATERIAL_ASSET_ID: Uuid = Uuid::from_u128(0x968c_bad0_4dad_d64e_71b5_72bf_13ad_051a);

/// The reference `LLMaterial`'s default specular exponent
/// (`DEFAULT_SPECULAR_LIGHT_EXPONENT`, `(U8)(0.2 * 255) = 51`).
const DEFAULT_SPECULAR_EXPONENT: u8 = 51;
/// The reference `LLMaterial`'s default alpha-mask cutoff.
const DEFAULT_ALPHA_MASK_CUTOFF: u8 = 128;

/// A neutral [`LLMaterial`](LegacyMaterial) — the reference `LLMaterial()`
/// defaults, used as the base when a face without an existing legacy material
/// first gains one (setting a normal / specular map or a non-default parameter).
fn default_legacy_material() -> LegacyMaterial {
    LegacyMaterial {
        normal_map: TextureKey::from(Uuid::nil()),
        normal_offset: (0.0, 0.0),
        normal_repeat: (1.0, 1.0),
        normal_rotation: 0.0,
        specular_map: TextureKey::from(Uuid::nil()),
        specular_offset: (0.0, 0.0),
        specular_repeat: (1.0, 1.0),
        specular_rotation: 0.0,
        specular_color: [255, 255, 255, 255],
        specular_exponent: DEFAULT_SPECULAR_EXPONENT,
        environment_intensity: 0,
        diffuse_alpha_mode: ALPHA_MODE_NONE,
        alpha_mask_cutoff: DEFAULT_ALPHA_MASK_CUTOFF,
    }
}

// ---------------------------------------------------------------------------
// Legacy (Blinn-Phong) numeric fields.
// ---------------------------------------------------------------------------

/// One numeric field editing a legacy-material attribute (a normal / specular
/// transform component, or a scalar), plus the mode it shows in.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyField {
    /// Normal-map horizontal repeats.
    NormalRepeatU,
    /// Normal-map vertical repeats.
    NormalRepeatV,
    /// Normal-map horizontal offset.
    NormalOffsetU,
    /// Normal-map vertical offset.
    NormalOffsetV,
    /// Normal-map rotation, in degrees (stored radians).
    NormalRotation,
    /// Specular-map horizontal repeats.
    SpecRepeatU,
    /// Specular-map vertical repeats.
    SpecRepeatV,
    /// Specular-map horizontal offset.
    SpecOffsetU,
    /// Specular-map vertical offset.
    SpecOffsetV,
    /// Specular-map rotation, in degrees (stored radians).
    SpecRotation,
    /// Glossiness (the specular exponent), 0..=255.
    Glossiness,
    /// Environment-reflection intensity, 0..=255.
    Environment,
    /// Alpha-mask cutoff, 0..=255.
    MaskCutoff,
}

impl LegacyField {
    /// The widget element id, for the skin / harness.
    const fn element(self) -> &'static str {
        match self {
            Self::NormalRepeatU => "build-mat-normal-repeat-u",
            Self::NormalRepeatV => "build-mat-normal-repeat-v",
            Self::NormalOffsetU => "build-mat-normal-offset-u",
            Self::NormalOffsetV => "build-mat-normal-offset-v",
            Self::NormalRotation => "build-mat-normal-rotation",
            Self::SpecRepeatU => "build-mat-spec-repeat-u",
            Self::SpecRepeatV => "build-mat-spec-repeat-v",
            Self::SpecOffsetU => "build-mat-spec-offset-u",
            Self::SpecOffsetV => "build-mat-spec-offset-v",
            Self::SpecRotation => "build-mat-spec-rotation",
            Self::Glossiness => "build-mat-glossiness",
            Self::Environment => "build-mat-environment",
            Self::MaskCutoff => "build-mat-mask-cutoff",
        }
    }

    /// The field's input kind (the scalars are integers, transforms floats).
    const fn input_kind(self) -> TextInputKind {
        match self {
            Self::Glossiness | Self::Environment | Self::MaskCutoff => TextInputKind::Integer,
            _float => TextInputKind::Float,
        }
    }

    /// Read the field's display value off a resolved legacy material.
    fn display_value(self, material: &LegacyMaterial) -> f32 {
        match self {
            Self::NormalRepeatU => material.normal_repeat.0,
            Self::NormalRepeatV => material.normal_repeat.1,
            Self::NormalOffsetU => material.normal_offset.0,
            Self::NormalOffsetV => material.normal_offset.1,
            Self::NormalRotation => material.normal_rotation.to_degrees(),
            Self::SpecRepeatU => material.specular_repeat.0,
            Self::SpecRepeatV => material.specular_repeat.1,
            Self::SpecOffsetU => material.specular_offset.0,
            Self::SpecOffsetV => material.specular_offset.1,
            Self::SpecRotation => material.specular_rotation.to_degrees(),
            Self::Glossiness => f32::from(material.specular_exponent),
            Self::Environment => f32::from(material.environment_intensity),
            Self::MaskCutoff => f32::from(material.alpha_mask_cutoff),
        }
    }

    /// Apply the field's committed value to a material, touching only its own
    /// attribute (the reference's per-attribute `set*`).
    const fn apply(self, material: &mut LegacyMaterial, value: f32) {
        match self {
            Self::NormalRepeatU => material.normal_repeat.0 = value,
            Self::NormalRepeatV => material.normal_repeat.1 = value,
            Self::NormalOffsetU => material.normal_offset.0 = value.clamp(-1.0, 1.0),
            Self::NormalOffsetV => material.normal_offset.1 = value.clamp(-1.0, 1.0),
            Self::NormalRotation => material.normal_rotation = value.to_radians(),
            Self::SpecRepeatU => material.specular_repeat.0 = value,
            Self::SpecRepeatV => material.specular_repeat.1 = value,
            Self::SpecOffsetU => material.specular_offset.0 = value.clamp(-1.0, 1.0),
            Self::SpecOffsetV => material.specular_offset.1 = value.clamp(-1.0, 1.0),
            Self::SpecRotation => material.specular_rotation = value.to_radians(),
            Self::Glossiness => material.specular_exponent = clamp_to_byte(value),
            Self::Environment => material.environment_intensity = clamp_to_byte(value),
            Self::MaskCutoff => material.alpha_mask_cutoff = clamp_to_byte(value),
        }
    }
}

/// The legacy-material numeric field rows, in tab order: each tuple is a label
/// key, the fields on the row, and the mode the row shows in.
const LEGACY_FIELD_ROWS: &[(&str, &[LegacyField], ShowWhen)] = &[
    (
        "build-tex-repeats-label",
        &[LegacyField::NormalRepeatU, LegacyField::NormalRepeatV],
        ShowWhen::MaterialNormal,
    ),
    (
        "build-tex-offset-label",
        &[LegacyField::NormalOffsetU, LegacyField::NormalOffsetV],
        ShowWhen::MaterialNormal,
    ),
    (
        "build-tex-rotation-label",
        &[LegacyField::NormalRotation],
        ShowWhen::MaterialNormal,
    ),
    (
        "build-tex-glossiness-label",
        &[LegacyField::Glossiness],
        ShowWhen::MaterialSpecular,
    ),
    (
        "build-tex-environment-label",
        &[LegacyField::Environment],
        ShowWhen::MaterialSpecular,
    ),
    (
        "build-tex-repeats-label",
        &[LegacyField::SpecRepeatU, LegacyField::SpecRepeatV],
        ShowWhen::MaterialSpecular,
    ),
    (
        "build-tex-offset-label",
        &[LegacyField::SpecOffsetU, LegacyField::SpecOffsetV],
        ShowWhen::MaterialSpecular,
    ),
    (
        "build-tex-rotation-label",
        &[LegacyField::SpecRotation],
        ShowWhen::MaterialSpecular,
    ),
    (
        "build-tex-mask-cutoff-label",
        &[LegacyField::MaskCutoff],
        ShowWhen::MaterialDiffuse,
    ),
];

// ---------------------------------------------------------------------------
// PBR channel transform display fields.
// ---------------------------------------------------------------------------

/// One numeric field **displaying** a PBR channel's `KHR_texture_transform`
/// component (read-only this pass — authoring PBR overrides is the
/// `viewer-pbr-material-editor` live editor's job).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PbrField {
    /// Horizontal repeats (`scale[0]`).
    RepeatU,
    /// Vertical repeats (`scale[1]`).
    RepeatV,
    /// Horizontal offset (`offset[0]`).
    OffsetU,
    /// Vertical offset (`offset[1]`).
    OffsetV,
    /// Rotation, in degrees (stored radians).
    Rotation,
}

impl PbrField {
    /// The widget element id, for the skin / harness.
    const fn element(self) -> &'static str {
        match self {
            Self::RepeatU => "build-pbr-repeat-u",
            Self::RepeatV => "build-pbr-repeat-v",
            Self::OffsetU => "build-pbr-offset-u",
            Self::OffsetV => "build-pbr-offset-v",
            Self::Rotation => "build-pbr-rotation",
        }
    }

    /// Read the field's display value off a channel transform.
    fn display_value(self, transform: &GltfTextureTransform) -> f32 {
        match self {
            Self::RepeatU => component(transform.scale, 0, 1.0),
            Self::RepeatV => component(transform.scale, 1, 1.0),
            Self::OffsetU => component(transform.offset, 0, 0.0),
            Self::OffsetV => component(transform.offset, 1, 0.0),
            Self::Rotation => transform.rotation.to_degrees(),
        }
    }

    /// Apply the field's committed value to a transform, touching only its own
    /// component (the reference's per-component `mTextureTransform` overwrite; the
    /// rotation field is entered in degrees, stored radians).
    fn apply(self, transform: &mut GltfTextureTransform, value: f32) {
        match self {
            Self::RepeatU => set_component(&mut transform.scale, 0, value),
            Self::RepeatV => set_component(&mut transform.scale, 1, value),
            Self::OffsetU => set_component(&mut transform.offset, 0, value),
            Self::OffsetV => set_component(&mut transform.offset, 1, value),
            Self::Rotation => transform.rotation = value.to_radians(),
        }
    }
}

/// The PBR transform display rows, in tab order.
const PBR_FIELD_ROWS: &[(&str, &[PbrField])] = &[
    (
        "build-tex-repeats-label",
        &[PbrField::RepeatU, PbrField::RepeatV],
    ),
    (
        "build-tex-offset-label",
        &[PbrField::OffsetU, PbrField::OffsetV],
    ),
    ("build-tex-rotation-label", &[PbrField::Rotation]),
];

/// The element of a two-component array, or `fallback` if out of range (kept off
/// the disallowed indexing lint).
fn component(pair: [f32; 2], index: usize, fallback: f32) -> f32 {
    pair.get(index).copied().unwrap_or(fallback)
}

/// Set the element of a two-component array (a no-op out of range).
fn set_component(pair: &mut [f32; 2], index: usize, value: f32) {
    if let Some(slot) = pair.get_mut(index) {
        *slot = value;
    }
}

/// The GLTF texture slots a PBR channel edits: a single slot for a specific
/// channel, or all four for the whole-material channel (the reference applies a
/// render-material transform edit to every channel).
const fn pbr_channel_slots(channel: PbrChannel) -> &'static [usize] {
    match channel {
        PbrChannel::Material => &[0, 1, 2, 3],
        PbrChannel::BaseColor => &[0],
        PbrChannel::Normal => &[1],
        PbrChannel::MetallicRoughness => &[2],
        PbrChannel::Emissive => &[3],
    }
}

// ---------------------------------------------------------------------------
// PBR scalar (factor) fields.
// ---------------------------------------------------------------------------

/// A numeric field editing a PBR material scalar factor.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PbrScalarField {
    /// The metallic factor, 0..=1.
    Metallic,
    /// The roughness factor, 0..=1.
    Roughness,
    /// The alpha-mask cutoff, 0..=1.
    AlphaCutoff,
}

impl PbrScalarField {
    /// The widget element id.
    const fn element(self) -> &'static str {
        match self {
            Self::Metallic => "build-pbr-metallic-factor",
            Self::Roughness => "build-pbr-roughness-factor",
            Self::AlphaCutoff => "build-pbr-alpha-cutoff",
        }
    }

    /// Read the field's display value off an effective material.
    const fn display_value(self, material: &GltfMaterial) -> f32 {
        match self {
            Self::Metallic => material.metallic_factor,
            Self::Roughness => material.roughness_factor,
            Self::AlphaCutoff => material.alpha_cutoff,
        }
    }

    /// Set the field's committed value on a material override (clamped 0..=1).
    const fn apply(self, over: &mut MaterialOverride, value: f32) {
        let value = value.clamp(0.0, 1.0);
        match self {
            Self::Metallic => over.metallic_factor = Some(value),
            Self::Roughness => over.roughness_factor = Some(value),
            Self::AlphaCutoff => over.alpha_cutoff = Some(value),
        }
    }
}

/// The PBR alpha-mode combo option labels, indexed by [`GltfAlphaMode`] order
/// (Opaque / Mask / Blend — the reference `LLGLTFMaterial` order).
const PBR_ALPHA_LABELS: [&str; 3] = [
    "build-pbr-alpha-opaque",
    "build-pbr-alpha-mask",
    "build-pbr-alpha-blend",
];

/// The combo index for a PBR alpha mode.
const fn pbr_alpha_index(mode: GltfAlphaMode) -> usize {
    match mode {
        GltfAlphaMode::Opaque => 0,
        GltfAlphaMode::Mask => 1,
        GltfAlphaMode::Blend => 2,
    }
}

/// The PBR alpha mode for a combo index.
const fn pbr_alpha_mode(index: usize) -> GltfAlphaMode {
    match index {
        1 => GltfAlphaMode::Mask,
        2 => GltfAlphaMode::Blend,
        _opaque => GltfAlphaMode::Opaque,
    }
}

// ---------------------------------------------------------------------------
// Marker components + UI handle resource.
// ---------------------------------------------------------------------------

/// Tags the diffuse (legacy) alpha-mode combo so the sync and change handler
/// find it.
#[derive(Component, Debug, Clone, Copy)]
struct AlphaModeCombo;

/// Tags the PBR alpha-mode combo.
#[derive(Component, Debug, Clone, Copy)]
struct PbrAlphaCombo;

/// The double-sided toggle button (a PBR material-level flag).
#[derive(Component, Debug, Clone, Copy)]
struct DoubleSidedButton;

/// The double-sided toggle's check-glyph text.
#[derive(Component, Debug, Clone, Copy)]
struct DoubleSidedGlyph;

/// The "New material" (apply a blank GLTF material) button.
#[derive(Component, Debug, Clone, Copy)]
struct PbrNewButton;

/// The "Save material to inventory" button.
#[derive(Component, Debug, Clone, Copy)]
struct PbrSaveButton;

/// Tags every interactive material-channel control (the swatches, combos, numeric
/// fields, toggles and action buttons) so [`gate_material_controls`] can
/// pointer-disable them — and grey a disabled text field's font — when the primary
/// selection is not modifiable, the material-tab counterpart of the Texture tab's
/// `TexControl`. The row **labels / values** grey through the shared page walk
/// (`grey_texture_tab`), so this marker drives only the interaction-disable.
#[derive(Component, Debug, Clone, Copy)]
struct MatControl;

/// The material-channel widget handles the sync / reply systems address.
#[derive(Resource, Debug, Clone, Copy)]
struct BuildMaterialUi {
    /// The normal-map texture swatch (the texture picker's requester).
    normal_swatch: Entity,
    /// The specular-map texture swatch.
    specular_swatch: Entity,
    /// The specular-highlight colour swatch (the colour picker's requester).
    spec_color_swatch: Entity,
    /// The PBR render-material swatch (assigns a material asset to the faces).
    pbr_swatch: Entity,
    /// The diffuse (legacy) alpha-mode combo.
    alpha_combo: Entity,
    /// The PBR base-colour texture swatch.
    pbr_base_swatch: Entity,
    /// The PBR base-colour tint swatch.
    pbr_base_tint: Entity,
    /// The PBR metallic-roughness texture swatch.
    pbr_metallic_swatch: Entity,
    /// The PBR emissive texture swatch.
    pbr_emissive_swatch: Entity,
    /// The PBR emissive tint swatch.
    pbr_emissive_tint: Entity,
    /// The PBR normal texture swatch.
    pbr_normal_swatch: Entity,
    /// The PBR alpha-mode combo.
    pbr_alpha_combo: Entity,
    /// The double-sided toggle's glyph text (rewritten by the sync).
    double_sided_glyph: Entity,
}

/// The last-shown material-channel snapshot, so the widgets rewrite only on a
/// real change (the resolved legacy material, the PBR material id / channel
/// transform, and the mode all feed it) — a just-committed edit is not clobbered
/// before the simulator's confirming update lands.
#[derive(Resource, Debug, Default, PartialEq)]
struct MatShownSnapshot {
    /// The last shown `(selected-face signature, mode, resolved legacy material,
    /// PBR material id, PBR channel transform)`, or `None` when nothing valid.
    shown: Option<MatShown>,
}

/// The resolved material state the tab last displayed — the comparison the sync
/// snapshot keys on.
#[derive(Debug, Clone, PartialEq)]
struct MatShown {
    /// A signature of the selected-face set, so a selection change re-syncs.
    signature: u64,
    /// The active mode / channel, so a channel switch re-syncs.
    mode: MatModeState,
    /// The resolved legacy material of the representative face (its own, or the
    /// neutral default when it has none).
    legacy: LegacyMaterial,
    /// The representative face's PBR render-material asset id, if any.
    pbr_material: Option<Uuid>,
    /// The representative face's decoded base PBR material (default until it
    /// decodes / when the face has none).
    pbr_base: GltfMaterial,
    /// The representative face's PBR override, if any.
    pbr_override: Option<MaterialOverride>,
}

/// The plugin wiring the material channels into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditMaterialPlugin;

impl Plugin for EditMaterialPlugin {
    /// Run the material-channel sync + commit systems (the widgets are spawned by
    /// [`spawn_material_channels`], called from the Texture-tab spawn).
    fn build(&self, app: &mut App) {
        app.init_resource::<MatShownSnapshot>()
            .init_resource::<LegacyPreview>()
            .add_systems(
                Update,
                (
                    // Enable / disable every material control on the modify gate
                    // before this frame's sync / edits read them.
                    gate_material_controls,
                    sync_material_widgets,
                    // End a legacy-material live preview (revert to the real appearance)
                    // when its object is deselected or the Material mode / tool is left,
                    // before this frame's edits start a new one.
                    revert_legacy_preview,
                    commit_legacy_fields,
                    apply_alpha_mode_change,
                    apply_normal_specular_picked,
                    apply_spec_color_picked,
                    // Paint the live legacy-material preview onto the selected faces
                    // (after the edit handlers set it this frame).
                    drive_legacy_preview,
                    // Live-preview a browsed material on the prim (non-final picks),
                    // before the OK that `apply_pbr_material_picked` sends for real.
                    preview_pbr_material_picked,
                    apply_pbr_material_picked,
                    commit_pbr_fields,
                    commit_pbr_scalars,
                    apply_pbr_texture_picked,
                    apply_pbr_tint_picked,
                    apply_pbr_alpha_change,
                )
                    .chain(),
            );
    }
}

/// Spawn the material-channel editors under the Texture page (called from
/// [`crate::edit_texture`]'s tab spawn so they share the page and the mode
/// selectors). Advances `tab_index` past the widgets it spawns.
pub(crate) fn spawn_material_channels(commands: &mut Commands, page: Entity, tab_index: &mut i32) {
    // Diffuse alpha mode (a legacy-material attribute shown in the Texture
    // channel, the reference's `combobox alphamode`).
    let alpha_row = spawn_row(commands, page, "build-tex-alpha-mode-label");
    commands.entity(alpha_row).insert(ShowWhen::MaterialDiffuse);
    let alpha_labels: Vec<String> = ALPHA_MODE_LABELS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let alpha_combo = spawn_combo(
        commands,
        alpha_row,
        &ComboSpec {
            element: "build-tex-alpha-mode",
            labels: &alpha_labels,
            active: usize::from(ALPHA_MODE_NONE),
            tab_index: *tab_index,
            font_size: TOOL_FONT_SIZE,
            translate_labels: true,
        },
    );
    commands
        .entity(alpha_combo)
        .insert((AlphaModeCombo, MatControl));
    *tab_index = tab_index.saturating_add(1);

    // Normal-map swatch (the reference's `bumpytexture`).
    let normal_row = spawn_row(commands, page, "build-tex-normal-label");
    commands.entity(normal_row).insert(ShowWhen::MaterialNormal);
    let normal_swatch = spawn_texture_swatch(
        commands,
        normal_row,
        "build-tex-normal",
        *tab_index,
        TextureKey::from(Uuid::nil()),
    );
    commands.entity(normal_swatch).insert(MatControl);
    *tab_index = tab_index.saturating_add(1);

    // Specular-map swatch (the reference's `shinytexture`).
    let specular_row = spawn_row(commands, page, "build-tex-specular-label");
    commands
        .entity(specular_row)
        .insert(ShowWhen::MaterialSpecular);
    let specular_swatch = spawn_texture_swatch(
        commands,
        specular_row,
        "build-tex-specular",
        *tab_index,
        TextureKey::from(Uuid::nil()),
    );
    commands.entity(specular_swatch).insert(MatControl);
    *tab_index = tab_index.saturating_add(1);

    // Specular-highlight colour (the reference's `shinycolorswatch`).
    let spec_color_row = spawn_row(commands, page, "build-tex-shiny-color-label");
    commands
        .entity(spec_color_row)
        .insert(ShowWhen::MaterialSpecular);
    let spec_color_swatch = spawn_color_swatch(
        commands,
        spec_color_row,
        "build-tex-shiny-color",
        *tab_index,
        Color::WHITE,
    );
    commands.entity(spec_color_swatch).insert(MatControl);
    *tab_index = tab_index.saturating_add(1);

    // The legacy normal / specular transform + scalar rows.
    for (label_key, fields, show_when) in LEGACY_FIELD_ROWS {
        spawn_legacy_field_row(commands, page, label_key, fields, *show_when, tab_index);
    }

    // --- PBR (render-material) channel controls ---

    // The render-material swatch (assign / clear a stored material asset) + the
    // New / Save buttons (the reference's material picker + Edit/Save).
    let pbr_row = spawn_row(commands, page, "build-tex-pbr-material-label");
    commands.entity(pbr_row).insert(ShowWhen::PbrMaterialId);
    // A material swatch: it opens the *material* picker (not the texture picker)
    // seeded with the current render-material id, while still painting the
    // material's base-colour texture as its thumbnail stand-in.
    let pbr_swatch = spawn_material_swatch(
        commands,
        pbr_row,
        "build-tex-pbr-material",
        *tab_index,
        TextureKey::from(Uuid::nil()),
        Uuid::nil(),
    );
    // The render-material swatch previews the material on a lit sphere
    // ([`crate::material_preview`]) rather than painting a flat texture thumbnail;
    // it starts empty until a face with a render material is selected.
    commands
        .entity(pbr_swatch)
        .insert((MaterialPreview::Empty, MatControl));
    *tab_index = tab_index.saturating_add(1);
    let new_button = spawn_action_button(commands, pbr_row, "build-pbr-new", *tab_index);
    commands.entity(new_button).insert(PbrNewButton);
    commands.entity(new_button).observe(handle_pbr_new_press);
    *tab_index = tab_index.saturating_add(1);
    let save_button = spawn_action_button(commands, pbr_row, "build-pbr-save", *tab_index);
    commands.entity(save_button).insert(PbrSaveButton);
    commands.entity(save_button).observe(handle_pbr_save_press);
    *tab_index = tab_index.saturating_add(1);

    // Material-level alpha mode + cutoff + double-sided (render-material channel).
    let pbr_alpha_row = spawn_row(commands, page, "build-tex-alpha-mode-label");
    commands
        .entity(pbr_alpha_row)
        .insert(ShowWhen::PbrMaterialId);
    let pbr_alpha_labels: Vec<String> = PBR_ALPHA_LABELS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let pbr_alpha_combo = spawn_combo(
        commands,
        pbr_alpha_row,
        &ComboSpec {
            element: "build-pbr-alpha-mode",
            labels: &pbr_alpha_labels,
            active: 0,
            tab_index: *tab_index,
            font_size: TOOL_FONT_SIZE,
            translate_labels: true,
        },
    );
    commands
        .entity(pbr_alpha_combo)
        .insert((PbrAlphaCombo, MatControl));
    *tab_index = tab_index.saturating_add(1);
    spawn_pbr_scalar_row(
        commands,
        page,
        "build-tex-mask-cutoff-label",
        PbrScalarField::AlphaCutoff,
        ShowWhen::PbrMaterialId,
        tab_index,
    );
    let double_sided_glyph =
        spawn_double_sided_toggle(commands, page, "build-pbr-double-sided", tab_index);

    // --- PBR base-colour channel ---
    let pbr_base_swatch = spawn_pbr_swatch_row(
        commands,
        page,
        "build-pbr-base-texture-label",
        "build-pbr-base-texture",
        ShowWhen::PbrBaseColor,
        tab_index,
    );
    let pbr_base_tint = spawn_pbr_tint_row(
        commands,
        page,
        "build-pbr-base-tint-label",
        "build-pbr-base-tint",
        ShowWhen::PbrBaseColor,
        tab_index,
    );

    // --- PBR metallic-roughness channel ---
    let pbr_metallic_swatch = spawn_pbr_swatch_row(
        commands,
        page,
        "build-pbr-metallic-texture-label",
        "build-pbr-metallic-texture",
        ShowWhen::PbrMetallic,
        tab_index,
    );
    spawn_pbr_scalar_row(
        commands,
        page,
        "build-pbr-metallic-factor-label",
        PbrScalarField::Metallic,
        ShowWhen::PbrMetallic,
        tab_index,
    );
    spawn_pbr_scalar_row(
        commands,
        page,
        "build-pbr-roughness-factor-label",
        PbrScalarField::Roughness,
        ShowWhen::PbrMetallic,
        tab_index,
    );

    // --- PBR emissive channel ---
    let pbr_emissive_swatch = spawn_pbr_swatch_row(
        commands,
        page,
        "build-pbr-emissive-texture-label",
        "build-pbr-emissive-texture",
        ShowWhen::PbrEmissive,
        tab_index,
    );
    let pbr_emissive_tint = spawn_pbr_tint_row(
        commands,
        page,
        "build-pbr-emissive-tint-label",
        "build-pbr-emissive-tint",
        ShowWhen::PbrEmissive,
        tab_index,
    );

    // --- PBR normal channel ---
    let pbr_normal_swatch = spawn_pbr_swatch_row(
        commands,
        page,
        "build-pbr-normal-texture-label",
        "build-pbr-normal-texture",
        ShowWhen::PbrNormal,
        tab_index,
    );

    // The PBR per-channel transform display / edit rows (shown in every PBR
    // channel).
    for (label_key, fields) in PBR_FIELD_ROWS {
        spawn_pbr_field_row(commands, page, label_key, fields, tab_index);
    }

    commands.insert_resource(BuildMaterialUi {
        normal_swatch,
        specular_swatch,
        spec_color_swatch,
        pbr_swatch,
        alpha_combo,
        pbr_base_swatch,
        pbr_base_tint,
        pbr_metallic_swatch,
        pbr_emissive_swatch,
        pbr_emissive_tint,
        pbr_normal_swatch,
        pbr_alpha_combo,
        double_sided_glyph,
    });
}

/// Spawn a PBR channel texture-swatch row, returning the swatch entity.
fn spawn_pbr_swatch_row(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    element: &'static str,
    show_when: ShowWhen,
    tab_index: &mut i32,
) -> Entity {
    let row = spawn_row(commands, page, label_key);
    commands.entity(row).insert(show_when);
    let swatch = spawn_texture_swatch(
        commands,
        row,
        element,
        *tab_index,
        TextureKey::from(Uuid::nil()),
    );
    commands.entity(swatch).insert(MatControl);
    *tab_index = tab_index.saturating_add(1);
    swatch
}

/// Spawn a PBR channel colour-tint row, returning the swatch entity.
fn spawn_pbr_tint_row(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    element: &'static str,
    show_when: ShowWhen,
    tab_index: &mut i32,
) -> Entity {
    let row = spawn_row(commands, page, label_key);
    commands.entity(row).insert(show_when);
    let swatch = spawn_color_swatch(commands, row, element, *tab_index, Color::WHITE);
    commands.entity(swatch).insert(MatControl);
    *tab_index = tab_index.saturating_add(1);
    swatch
}

/// Spawn a labelled PBR scalar-factor field row.
fn spawn_pbr_scalar_row(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    field: PbrScalarField,
    show_when: ShowWhen,
    tab_index: &mut i32,
) {
    let row = spawn_row(commands, page, label_key);
    commands.entity(row).insert(show_when);
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let entity = spawn_text_input(
        commands,
        row,
        &TextInputSpec {
            font_size: TOOL_FONT_SIZE,
            width_glyphs: MAT_FIELD_GLYPHS,
            tab_index: index,
            ..TextInputSpec::new(field.element(), TextInputKind::Float)
        },
    );
    commands.entity(entity).insert((field, MatControl));
}

/// Spawn the double-sided toggle row, returning the check-glyph entity.
fn spawn_double_sided_toggle(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    tab_index: &mut i32,
) -> Entity {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let row = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(index),
            Node {
                align_items: AlignItems::Center,
                ..crate::ui::row(Val::Px(6.0))
            },
            Pickable::default(),
            DoubleSidedButton,
            MatControl,
            ShowWhen::PbrMaterialId,
            Name::new("build-pbr:double-sided"),
            ChildOf(page),
        ))
        .id();
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH),
            crate::ui_font::UiFont::Sans.at(TOOL_FONT_SIZE),
            TextColor(Color::WHITE),
            bevy_flair::style::components::ClassList::new_with_classes([VALUE_CLASS]),
            DoubleSidedGlyph,
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        Text::default(),
        crate::i18n::Translated::new(label_key),
        crate::ui_font::UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        bevy_flair::style::components::ClassList::new_with_classes([LABEL_CLASS]),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    commands.entity(row).observe(handle_double_sided_press);
    glyph
}

/// Spawn a small labelled action button (New / Save), returning its entity.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..crate::ui::row(Val::ZERO)
            },
            BorderColor::all(Color::srgba(0.4, 0.4, 0.45, 1.0)),
            BackgroundColor(Color::srgba(0.18, 0.18, 0.2, 1.0)),
            Pickable::default(),
            MatControl,
            Name::new(format!("build-pbr:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        crate::i18n::Translated::new(label_key),
        crate::ui_font::UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::WHITE),
        bevy_flair::style::components::ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn a labelled legacy-material numeric-field row tagged with its mode.
fn spawn_legacy_field_row(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    fields: &[LegacyField],
    show_when: ShowWhen,
    tab_index: &mut i32,
) {
    let row_entity = spawn_row(commands, page, label_key);
    commands.entity(row_entity).insert(show_when);
    for &field in fields {
        let index = *tab_index;
        *tab_index = tab_index.saturating_add(1);
        let entity = spawn_text_input(
            commands,
            row_entity,
            &TextInputSpec {
                font_size: TOOL_FONT_SIZE,
                width_glyphs: MAT_FIELD_GLYPHS,
                tab_index: index,
                ..TextInputSpec::new(field.element(), field.input_kind())
            },
        );
        commands.entity(entity).insert((field, MatControl));
    }
}

/// Spawn a labelled PBR-transform display row (read-only fields).
fn spawn_pbr_field_row(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    fields: &[PbrField],
    tab_index: &mut i32,
) {
    let row_entity = spawn_row(commands, page, label_key);
    commands.entity(row_entity).insert(ShowWhen::PbrAny);
    for &field in fields {
        let index = *tab_index;
        *tab_index = tab_index.saturating_add(1);
        let entity = spawn_text_input(
            commands,
            row_entity,
            &TextInputSpec {
                font_size: TOOL_FONT_SIZE,
                width_glyphs: MAT_FIELD_GLYPHS,
                tab_index: index,
                ..TextInputSpec::new(field.element(), TextInputKind::Float)
            },
        );
        commands.entity(entity).insert((field, MatControl));
    }
}

// ---------------------------------------------------------------------------
// Reading the resolved material of the representative face.
// ---------------------------------------------------------------------------

/// The legacy material a face displays: its own decoded material (from the
/// manager) when it has one, otherwise the neutral default a new material starts
/// from.
fn legacy_material_of(face: &TextureFace, manager: &LegacyMaterialManager) -> LegacyMaterial {
    face.material_id
        .and_then(|id| manager.decoded_material(&id).cloned())
        .unwrap_or_else(default_legacy_material)
}

/// The representative PBR state of the primary selection: the face's render
/// material asset id (from its object's [`ObjectRenderMaterials`] holder), the
/// decoded base material (default until it decodes), and the face's override —
/// enough to derive every displayed PBR field. `None` id when the face carries no
/// render material.
fn representative_pbr(
    selection: &SelectionSet,
    render_materials: &Query<&ObjectRenderMaterials>,
    material_manager: &MaterialManager,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
) -> (Option<Uuid>, GltfMaterial, Option<MaterialOverride>) {
    let Some(primary) = selection.primary() else {
        return (None, GltfMaterial::default(), None);
    };
    let face_id = primary_face_index(selection);
    let Some(material_id) =
        face_material_id(primary.entity, face_id, render_materials, children, scene)
    else {
        return (None, GltfMaterial::default(), None);
    };
    let base = material_manager
        .decoded_material(AssetKey::from(material_id))
        .copied()
        .unwrap_or_default();
    let over = material_manager.face_override(primary.scoped, face_id);
    (Some(material_id), base, over)
}

/// The effective PBR material a face displays: its base material with the face's
/// override folded on (the reference's render material).
fn effective_pbr_material(base: GltfMaterial, over: Option<&MaterialOverride>) -> GltfMaterial {
    let mut effective = base;
    if let Some(over) = over {
        over.apply_to(&mut effective);
    }
    effective
}

/// The effective transform of `channel` on a face: the base material's channel
/// transform with the face's override (if any) folded on — what the reference's
/// PBR transform fields display (the render-material transform).
fn effective_channel_transform(
    material: &GltfMaterial,
    over: Option<&MaterialOverride>,
    channel: PbrChannel,
) -> Option<GltfTextureTransform> {
    let mut transform = channel_transform(material, channel)?;
    if let Some(over) = over
        && let Some(&slot) = pbr_channel_slots(channel).first()
        && let Some(slot_over) = over.transforms.get(slot)
    {
        if let Some(offset) = slot_over.offset {
            transform.offset = offset;
        }
        if let Some(scale) = slot_over.scale {
            transform.scale = scale;
        }
        if let Some(rotation) = slot_over.rotation {
            transform.rotation = rotation;
        }
    }
    Some(transform)
}

/// The GLTF render-material asset id of face `face_id` on the object rooted at
/// `root`, walked from its [`ObjectRenderMaterials`] holder(s).
fn face_material_id(
    root: Entity,
    face_id: u8,
    render_materials: &Query<&ObjectRenderMaterials>,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
) -> Option<Uuid> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if entity != root && scene.get(entity).is_ok() {
            continue;
        }
        if let Ok(holder) = render_materials.get(entity)
            && let Some((_face, id)) = holder.faces.iter().find(|(face, _id)| *face == face_id)
        {
            return Some(*id);
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
    None
}

/// The `KHR_texture_transform` of a decoded material's channel, if that channel
/// carries a texture (the whole-material channel has no single transform).
fn channel_transform(material: &GltfMaterial, channel: PbrChannel) -> Option<GltfTextureTransform> {
    let texture = match channel {
        PbrChannel::Material | PbrChannel::BaseColor => material.base_color_texture,
        PbrChannel::MetallicRoughness => material.metallic_roughness_texture,
        PbrChannel::Emissive => material.emissive_texture,
        PbrChannel::Normal => material.normal_texture,
    };
    texture.map(|texture| texture.transform)
}

// ---------------------------------------------------------------------------
// Sync: populate the widgets from the representative face.
// ---------------------------------------------------------------------------

/// The material-channel widget queries the sync pass rewrites.
#[derive(bevy::ecs::system::SystemParam)]
struct MatWidgets<'w, 's> {
    /// The legacy numeric fields.
    legacy_fields: Query<'w, 's, (Entity, &'static LegacyField, &'static mut EditableText)>,
    /// The PBR transform display fields.
    pbr_fields:
        Query<'w, 's, (Entity, &'static PbrField, &'static mut EditableText), Without<LegacyField>>,
    /// The PBR scalar fields (metallic / roughness / alpha cutoff).
    #[expect(
        clippy::type_complexity,
        reason = "a Bevy query's type is its filter: the scalar fields, disjoint from the legacy \
                  and transform fields so the three field queries don't alias"
    )]
    pbr_scalars: Query<
        'w,
        's,
        (Entity, &'static PbrScalarField, &'static mut EditableText),
        (Without<LegacyField>, Without<PbrField>),
    >,
    /// The alpha-mode combo selections (legacy + PBR).
    combos: Query<'w, 's, &'static mut ComboSelection>,
    /// The texture-swatch values (normal / specular / PBR channels).
    texture_swatches: Query<'w, 's, &'static mut TextureSwatchValue>,
    /// The render-material swatch's material id (what its picker opens on).
    material_swatches: Query<'w, 's, &'static mut MaterialSwatchValue>,
    /// The render-material swatch's sphere preview (the effective material shown).
    material_previews: Query<'w, 's, &'static mut MaterialPreview>,
    /// The colour swatch values (specular / PBR tints).
    color_swatches: Query<'w, 's, &'static mut ColorSwatchValue>,
    /// The double-sided toggle glyph text.
    double_sided_glyph: Query<'w, 's, &'static mut Text, With<DoubleSidedGlyph>>,
}

/// Enable or disable every [`MatControl`] material-channel control on the same
/// gate the Texture tab uses — a modifiable primary selection — pointer-disabling
/// them (and greying a disabled text field's font, via
/// [`crate::ui_text_input`]'s `reflect_disabled_text_color`) when the primary is
/// not modifiable. The row **labels / values** grey through the shared page walk
/// (`grey_texture_tab`, driven by the Texture tab's own gate on the same
/// condition), so this system owns only the interaction-disable. Applied on the
/// enabled/disabled transition, so a stable state does not re-touch every control
/// each frame; reset when the build tool closes so a reopen re-applies.
fn gate_material_controls(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut last_enabled: Local<Option<bool>>,
    controls: Query<Entity, With<MatControl>>,
    mut commands: Commands,
) {
    if !tool.active {
        *last_enabled = None;
        return;
    }
    // The same gate as [`sync_material_widgets`] / the Texture tab: a texture /
    // colour / material edit is a modify, so every control greys and disables
    // while nothing is selected or the primary is not modifiable (values still
    // show).
    let modify_ok = selection
        .primary()
        .is_some_and(|node| objects.agent_can_modify(&node.scoped));
    let enabled = representative_face(&selection, &objects).is_some() && modify_ok;
    if *last_enabled == Some(enabled) {
        return;
    }
    *last_enabled = Some(enabled);
    for control in &controls {
        if enabled {
            commands
                .entity(control)
                .remove::<bevy::ui::InteractionDisabled>()
                .insert(Pickable::default());
        } else {
            commands
                .entity(control)
                .insert((bevy::ui::InteractionDisabled, Pickable::IGNORE));
        }
    }
}

/// Whether a Material commit may proceed on the current selection, posting the
/// shared no-modify notice (and returning `false`) when the primary selection is
/// present but not modifiable — the belt-and-braces backstop behind
/// [`gate_material_controls`]'s greying, mirroring the transform-field / gizmo
/// gate ([`EditPerm::Modify`] on the agent-relative `update_flags`, the reference's
/// greyed-panel notice). Returns `true` when there is no primary — nothing to
/// commit — so callers still no-op naturally.
fn material_edit_allowed(
    selection: &SelectionSet,
    objects: &ObjectState,
    notices: &mut MessageWriter<LocalChatNotice>,
) -> bool {
    let Some(primary) = selection.primary() else {
        return true;
    };
    if EditPerm::Modify.granted(objects, &primary.scoped) {
        return true;
    }
    let name = primary
        .properties
        .as_ref()
        .map_or_else(String::new, |properties| properties.name.clone());
    notices.write(LocalChatNotice::new(perm_notice(EditPerm::Modify, &name)));
    false
}

/// Populate the material-channel widgets from the primary selection's
/// representative face — skipping the field the user is editing, and only when
/// the shown snapshot changes.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection / object state, the two material managers, the render-material + \
              hierarchy queries, the snapshot guard, the focus, the widget queries, the UI \
              handles, and the text-layout contexts a programmatic rewrite needs"
)]
fn sync_material_widgets(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    legacy_manager: Res<LegacyMaterialManager>,
    material_manager: Res<MaterialManager>,
    mode: Res<MatModeState>,
    ui: Option<Res<BuildMaterialUi>>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut snapshot: ResMut<MatShownSnapshot>,
    focus: Res<InputFocus>,
    mut widgets: MatWidgets,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    if !tool.active {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let current = representative_face(&selection, &objects).map(|(_scoped, face, signature)| {
        let legacy = legacy_material_of(&face, &legacy_manager);
        let (pbr_material, pbr_base, pbr_override) = representative_pbr(
            &selection,
            &render_materials,
            &material_manager,
            &children,
            &scene,
        );
        MatShown {
            signature,
            mode: *mode,
            legacy,
            pbr_material,
            pbr_base,
            pbr_override,
        }
    });
    if snapshot.shown == current {
        return;
    }
    snapshot.shown.clone_from(&current);
    let Some(shown) = current else {
        return;
    };
    let effective = effective_pbr_material(shown.pbr_base, shown.pbr_override.as_ref());

    // Legacy numeric fields.
    for (entity, field, mut editor) in &mut widgets.legacy_fields {
        if focus.get() == Some(entity) {
            continue;
        }
        let want = format_field(field.input_kind(), field.display_value(&shown.legacy));
        if editor.value().to_string() != want {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }
    // PBR transform fields: the active channel's effective transform (base +
    // override), or defaults for a textureless channel.
    let transform = effective_channel_transform(
        &shown.pbr_base,
        shown.pbr_override.as_ref(),
        mode.pbr_channel(),
    )
    .unwrap_or_default();
    for (entity, field, mut editor) in &mut widgets.pbr_fields {
        if focus.get() == Some(entity) {
            continue;
        }
        let want = format_field(TextInputKind::Float, field.display_value(&transform));
        if editor.value().to_string() != want {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }
    // PBR scalar fields (metallic / roughness / alpha cutoff).
    for (entity, field, mut editor) in &mut widgets.pbr_scalars {
        if focus.get() == Some(entity) {
            continue;
        }
        let want = format!("{:.3}", field.display_value(&effective));
        if editor.value().to_string() != want {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }
    // Legacy alpha-mode combo.
    if let Ok(mut combo) = widgets.combos.get_mut(ui.alpha_combo) {
        let want = usize::from(shown.legacy.diffuse_alpha_mode.min(ALPHA_MODE_EMISSIVE));
        if combo.active != want {
            combo.active = want;
        }
    }
    // PBR alpha-mode combo.
    if let Ok(mut combo) = widgets.combos.get_mut(ui.pbr_alpha_combo) {
        let want = pbr_alpha_index(effective.alpha_mode);
        if combo.active != want {
            combo.active = want;
        }
    }
    // Double-sided toggle glyph.
    if let Ok(mut text) = widgets.double_sided_glyph.get_mut(ui.double_sided_glyph) {
        let want = if effective.double_sided {
            CHECKED_GLYPH
        } else {
            UNCHECKED_GLYPH
        };
        if text.0 != want {
            want.clone_into(&mut text.0);
        }
    }
    // Legacy swatches.
    set_texture_swatch(
        &mut widgets.texture_swatches,
        ui.normal_swatch,
        shown.legacy.normal_map,
    );
    set_texture_swatch(
        &mut widgets.texture_swatches,
        ui.specular_swatch,
        shown.legacy.specular_map,
    );
    set_color_swatch(
        &mut widgets.color_swatches,
        ui.spec_color_swatch,
        Color::srgb_u8(
            byte_at(shown.legacy.specular_color, 0),
            byte_at(shown.legacy.specular_color, 1),
            byte_at(shown.legacy.specular_color, 2),
        ),
    );
    // PBR swatches. The render-material swatch previews the assigned material on a
    // lit sphere ([`crate::material_preview`], the reference's `LLTextureCtrl`
    // material preview): the effective material (base + override) when the face
    // carries one, else an empty swatch.
    let preview = match shown.pbr_material {
        Some(_) => MaterialPreview::Material(Box::new(effective)),
        None => MaterialPreview::Empty,
    };
    set_material_preview(&mut widgets.material_previews, ui.pbr_swatch, preview);
    // The render-material swatch's picker opens on the current material id.
    set_material_swatch(
        &mut widgets.material_swatches,
        ui.pbr_swatch,
        shown.pbr_material.unwrap_or_else(Uuid::nil),
    );
    set_texture_swatch(
        &mut widgets.texture_swatches,
        ui.pbr_base_swatch,
        texture_id_of(effective.base_color_texture),
    );
    set_texture_swatch(
        &mut widgets.texture_swatches,
        ui.pbr_metallic_swatch,
        texture_id_of(effective.metallic_roughness_texture),
    );
    set_texture_swatch(
        &mut widgets.texture_swatches,
        ui.pbr_emissive_swatch,
        texture_id_of(effective.emissive_texture),
    );
    set_texture_swatch(
        &mut widgets.texture_swatches,
        ui.pbr_normal_swatch,
        texture_id_of(effective.normal_texture),
    );
    set_color_swatch(
        &mut widgets.color_swatches,
        ui.pbr_base_tint,
        color_from_linear_rgba(effective.base_color),
    );
    set_color_swatch(
        &mut widgets.color_swatches,
        ui.pbr_emissive_tint,
        color_from_linear_rgb(effective.emissive_factor),
    );
}

/// Set a colour swatch's value if it differs.
fn set_color_swatch(swatches: &mut Query<&mut ColorSwatchValue>, entity: Entity, value: Color) {
    if let Ok(mut swatch) = swatches.get_mut(entity)
        && swatch.0 != value
    {
        swatch.0 = value;
    }
}

/// The texture id of an optional GLTF texture slot (nil when empty).
fn texture_id_of(texture: Option<GltfTexture>) -> TextureKey {
    texture.map_or_else(|| TextureKey::from(Uuid::nil()), |texture| texture.id)
}

/// A Bevy colour from a linear-RGBA GLTF factor (dropping alpha into the swatch).
const fn color_from_linear_rgba(rgba: [f32; 4]) -> Color {
    let [red, green, blue, alpha] = rgba;
    Color::linear_rgba(red, green, blue, alpha)
}

/// A Bevy colour from a linear-RGB GLTF emissive factor.
const fn color_from_linear_rgb(rgb: [f32; 3]) -> Color {
    let [red, green, blue] = rgb;
    Color::linear_rgb(red, green, blue)
}

/// The linear-RGBA a picked colour sends as a GLTF base-colour factor (keeping
/// full alpha — transparency is the alpha channel, edited elsewhere).
fn linear_rgba_of(color: Color) -> [f32; 4] {
    let linear = color.to_linear();
    [linear.red, linear.green, linear.blue, 1.0]
}

/// The linear-RGB a picked colour sends as a GLTF emissive factor.
fn linear_rgb_of(color: Color) -> [f32; 3] {
    let linear = color.to_linear();
    [linear.red, linear.green, linear.blue]
}

/// Set a texture swatch's value if it differs.
fn set_texture_swatch(
    swatches: &mut Query<&mut TextureSwatchValue>,
    entity: Entity,
    value: TextureKey,
) {
    if let Ok(mut swatch) = swatches.get_mut(entity)
        && swatch.0 != value
    {
        swatch.0 = value;
    }
}

/// Set a material swatch's material id if it differs (what its picker opens on).
fn set_material_swatch(
    swatches: &mut Query<&mut MaterialSwatchValue>,
    entity: Entity,
    value: Uuid,
) {
    if let Ok(mut swatch) = swatches.get_mut(entity)
        && swatch.0 != value
    {
        swatch.0 = value;
    }
}

/// Set a swatch's sphere-preview target if it differs (the effective material the
/// render-material swatch previews).
fn set_material_preview(
    previews: &mut Query<&mut MaterialPreview>,
    entity: Entity,
    value: MaterialPreview,
) {
    if let Ok(mut preview) = previews.get_mut(entity)
        && *preview != value
    {
        *preview = value;
    }
}

// ---------------------------------------------------------------------------
// Commit: send edits.
// ---------------------------------------------------------------------------

/// Commit numeric legacy-material edits on `Enter` in a focused field or when
/// focus leaves one: apply the one attribute to each selected face's material and
/// send them over the `RenderMaterials` PUT.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection / object state, the legacy manager, the focus + its blur tracker, the \
              field query, the per-face lookup, the keyboard, the no-modify notice writer, and the \
              command writer"
)]
fn commit_legacy_fields(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    legacy_manager: Res<LegacyMaterialManager>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: Local<Option<Entity>>,
    fields: Query<(Entity, &LegacyField, &EditableText)>,
    prim_faces: PrimFaceLookup,
    mut preview: ResMut<LegacyPreview>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !tool.active {
        *focus_track = None;
        return;
    }
    let focused = focus.get().filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused
    } else if *focus_track != focused {
        focus_track.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    *focus_track = focused;
    let Some(entity) = commit else {
        return;
    };
    let Ok((_entity, &field, editor)) = fields.get(entity) else {
        return;
    };
    let Some(value) = parse_tex_value(field.input_kind(), &editor.value().to_string()) else {
        return;
    };
    if !material_edit_allowed(&selection, &objects, &mut notices) {
        return;
    }
    let edit = move |material: &mut LegacyMaterial| field.apply(material, value);
    preview_legacy_edit(&mut preview, &selection, &objects, &legacy_manager, edit);
    apply_legacy_edit(
        &selection,
        &legacy_manager,
        &prim_faces,
        &mut commands,
        edit,
    );
}

/// Apply an alpha-mode combo pick to the selected faces' materials (previewed live
/// and sent over the PUT).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the combo changes + \
              its marker, the UI handle, the selection / object state, the legacy manager, the \
              per-face lookup, the live-preview state, the no-modify notice writer, and the command \
              writer"
)]
fn apply_alpha_mode_change(
    mut changes: MessageReader<ComboChanged>,
    combos: Query<(), With<AlphaModeCombo>>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    legacy_manager: Res<LegacyMaterialManager>,
    prim_faces: PrimFaceLookup,
    mut preview: ResMut<LegacyPreview>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for change in changes.read() {
        if change.combo != ui.alpha_combo || !combos.contains(change.combo) {
            continue;
        }
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        let mode = clamp_to_byte(from_usize(change.active)).min(ALPHA_MODE_EMISSIVE);
        let edit = move |material: &mut LegacyMaterial| material.diffuse_alpha_mode = mode;
        preview_legacy_edit(&mut preview, &selection, &objects, &legacy_manager, edit);
        apply_legacy_edit(
            &selection,
            &legacy_manager,
            &prim_faces,
            &mut commands,
            edit,
        );
    }
}

/// The live in-place preview of a legacy (Blinn-Phong) material edit: the edited
/// [`LegacyMaterial`] shown on the selected faces while the user browses a bump /
/// specular map (or edits a legacy field), mirroring the diffuse texture picker's
/// live preview ([`crate::edit_texture`]). Applied locally the way the reference
/// viewer applies a material edit for instant feedback — independent of the
/// `RenderMaterials` PUT round-trip, so the highlight / bump renders at once rather
/// than only after (and if) the simulator echoes a new material id. The commit still
/// sends the PUT; the preview reverts to the face's real appearance when the object
/// is deselected or the Material mode / build tool is left.
#[derive(Resource, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is one leg of the preview's apply/upload progress (scalars applied, each \
              map requested, all settled); they gate distinct once-only steps of the driver, not a \
              state that would read better as an enum"
)]
struct LegacyPreview {
    /// The edited material to show, or `None` when not previewing.
    material: Option<LegacyMaterial>,
    /// The object being previewed, so the preview reverts if the selection moves
    /// off it.
    object: Option<Entity>,
    /// Whether the matte base + scalar half has been applied — applied once, not
    /// every frame, to avoid a per-frame material re-prepare.
    scalars_done: bool,
    /// The uploaded (linear) normal map, once its texture decodes.
    normal_image: Option<Handle<Image>>,
    /// Whether the normal-map texture has been requested — so it is asked for **once**
    /// (a map that never decodes, e.g. a missing asset, must not be re-fetched every
    /// frame). Reset when a new edit replaces the preview.
    normal_requested: bool,
    /// The uploaded (sRGB) specular map, once its texture decodes.
    spec_image: Option<Handle<Image>>,
    /// Whether the specular-map texture has been requested (see [`normal_requested`](Self::normal_requested)).
    spec_requested: bool,
    /// Whether the scalars and both maps have settled, after which the driver stops
    /// touching the materials.
    settled: bool,
}

impl LegacyPreview {
    /// Start (or replace) the preview with `material` on `object`, resetting the
    /// apply / upload progress so the driver re-applies it.
    fn set(&mut self, material: LegacyMaterial, object: Option<Entity>) {
        self.material = Some(material);
        self.object = object;
        self.scalars_done = false;
        self.normal_image = None;
        self.normal_requested = false;
        self.spec_image = None;
        self.spec_requested = false;
        self.settled = false;
    }

    /// Stop previewing, returning the object that was being previewed (so the caller
    /// can revert its faces).
    fn take_object(&mut self) -> Option<Entity> {
        self.material = None;
        self.scalars_done = false;
        self.normal_image = None;
        self.normal_requested = false;
        self.spec_image = None;
        self.spec_requested = false;
        self.settled = false;
        self.object.take()
    }
}

/// Build the representative face's edited [`LegacyMaterial`] (its current material,
/// or the neutral default, with `edit` applied) and show it live on the selected
/// faces via [`LegacyPreview`] — the same edit the commit sends over the wire, shown
/// at once.
fn preview_legacy_edit(
    preview: &mut LegacyPreview,
    selection: &SelectionSet,
    objects: &ObjectState,
    legacy_manager: &LegacyMaterialManager,
    edit: impl Fn(&mut LegacyMaterial),
) {
    let Some((_scoped, face, _signature)) = representative_face(selection, objects) else {
        return;
    };
    let mut material = legacy_material_of(&face, legacy_manager);
    edit(&mut material);
    preview.set(material, selection.primary().map(|node| node.entity));
}

/// Apply `f` to each selected face's [`FaceMaterial`] handle — the selected faces of
/// each selected object (a per-face selection restricts to its chosen faces),
/// stopping the walk at linkset-child objects. The legacy-preview analog of
/// [`crate::edit_texture`]'s `preview_face_texture` walk.
fn for_selected_face_materials(
    selection: &SelectionSet,
    children: &Query<&Children>,
    scene: &Query<(), With<SceneObject>>,
    face_materials: &Query<(&PrimFaceEntity, &MeshMaterial3d<FaceMaterial>)>,
    mut f: impl FnMut(&Handle<FaceMaterial>),
) {
    for node in selection.iter() {
        let wanted = node.faces.as_ref();
        let mut stack = vec![node.entity];
        while let Some(entity) = stack.pop() {
            if entity != node.entity && scene.get(entity).is_ok() {
                continue;
            }
            if let Ok((marker, material)) = face_materials.get(entity)
                && wanted.is_none_or(|set| set.contains(&marker.face_id))
            {
                f(&material.0);
            }
            if let Ok(list) = children.get(entity) {
                for child in list.iter() {
                    stack.push(child);
                }
            }
        }
    }
}

/// Resolve one preview map: `true` once nothing more is needed (a nil id, or the map
/// already uploaded), `false` while its texture is still decoding. Uploads and caches
/// the image on first decode, in the colour space the slot needs (linear normal /
/// sRGB specular). The texture is requested **once** (`requested` gates it): a map
/// that never decodes — a missing asset — otherwise re-issues the fetch every frame
/// for as long as the picker is open.
fn resolve_preview_map(
    cache: &mut Option<Handle<Image>>,
    requested: &mut bool,
    id: TextureKey,
    srgb: bool,
    textures: &mut TextureManager,
    images: &mut Assets<Image>,
) -> bool {
    if id.uuid().is_nil() || cache.is_some() {
        return true;
    }
    let Some(decoded) = textures.decoded(id).map(std::sync::Arc::clone) else {
        if !*requested {
            textures.request_boosted(id, TERRAIN_BOOST_PRIORITY);
            *requested = true;
        }
        return false;
    };
    let image = if srgb {
        build_srgb_image(&decoded)
    } else {
        build_linear_image(&decoded)
    };
    *cache = Some(images.add(image));
    true
}

/// Paint the live legacy-material preview onto the selected faces: the matte base +
/// scalars (once), then each map as its texture decodes, turning on its re-sample
/// bit. Settles once the scalars and both maps are applied, after which it is a
/// no-op — so a held preview does not re-prepare the material every frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the preview state, \
              the selection, the texture store + image assets it uploads maps into, the hierarchy \
              / face-material queries, and the material store the preview paints into"
)]
fn drive_legacy_preview(
    mut preview: ResMut<LegacyPreview>,
    selection: Res<SelectionSet>,
    mut textures: ResMut<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    face_materials: Query<(&PrimFaceEntity, &MeshMaterial3d<FaceMaterial>)>,
    scene: Query<(), With<SceneObject>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    // Reborrow to a plain `&mut` so the two map resolves below can each take a
    // disjoint pair of fields (`*_image` + `*_requested`) — a borrow the `ResMut`
    // deref would otherwise widen to the whole resource.
    let preview = preview.as_mut();
    if preview.settled {
        return;
    }
    let Some(material) = preview.material.clone() else {
        return;
    };
    if !preview.scalars_done {
        for_selected_face_materials(&selection, &children, &scene, &face_materials, |handle| {
            if let Some(mut face) = materials.get_mut(handle) {
                let _override = apply_legacy_scalars(&mut face, &material);
            }
        });
        preview.scalars_done = true;
    }
    let normal_ready = resolve_preview_map(
        &mut preview.normal_image,
        &mut preview.normal_requested,
        material.normal_map,
        false,
        &mut textures,
        &mut images,
    );
    let spec_ready = resolve_preview_map(
        &mut preview.spec_image,
        &mut preview.spec_requested,
        material.specular_map,
        true,
        &mut textures,
        &mut images,
    );
    if let Some(image) = preview.normal_image.clone() {
        for_selected_face_materials(&selection, &children, &scene, &face_materials, |handle| {
            if let Some(mut face) = materials.get_mut(handle) {
                face.extension.normal_map = image.clone();
                face.extension.params.map_flags |= MAP_FLAG_NORMAL;
            }
        });
    }
    if let Some(image) = preview.spec_image.clone() {
        for_selected_face_materials(&selection, &children, &scene, &face_materials, |handle| {
            if let Some(mut face) = materials.get_mut(handle) {
                face.extension.specular_map = image.clone();
                face.extension.params.map_flags |= MAP_FLAG_SPEC;
            }
        });
    }
    preview.settled = normal_ready && spec_ready;
}

/// Revert the legacy-material preview to the faces' real appearance when it should
/// end — the object is no longer the primary selection, or the build tool / Material
/// mode has been left. Recomposes each previewed face from scratch: its diffuse
/// Blinn-Phong look ([`compose_face_material`]) plus its real cached legacy material,
/// exactly as the face was composed before the preview.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the preview state, \
              the tool / mode / selection driving the end-of-preview test, the legacy / texture / \
              prim-texture stores the revert recomposes through, and the hierarchy / face / \
              material queries"
)]
fn revert_legacy_preview(
    mut preview: ResMut<LegacyPreview>,
    tool: Res<EditToolState>,
    mode: Res<MatModeState>,
    selection: Res<SelectionSet>,
    mut legacy_manager: ResMut<LegacyMaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    children: Query<&Children>,
    faces: Query<(&FaceTextureDebug, &MeshMaterial3d<FaceMaterial>)>,
    scene: Query<(), With<SceneObject>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let Some(object) = preview.object else {
        return;
    };
    let previewing = tool.active
        && mode.is_material()
        && selection.primary().map(|node| node.entity) == Some(object);
    if previewing {
        return;
    }
    let _cleared = preview.take_object();
    // Recompose each of the object's own faces (stopping at linkset children) to its
    // real appearance.
    let mut stack = vec![object];
    while let Some(entity) = stack.pop() {
        if entity != object && scene.get(entity).is_ok() {
            continue;
        }
        if let Ok((FaceTextureDebug(texture_face), material)) = faces.get(entity) {
            compose_face_material(
                &material.0,
                texture_face,
                &mut materials,
                &mut textures,
                &mut prim_textures,
                TERRAIN_BOOST_PRIORITY,
                TextureAlpha::Mask,
            );
            if let Some(material_id) = texture_face.material_id
                && !material_id.is_nil()
            {
                preview_legacy_material(
                    &mut legacy_manager,
                    &mut textures,
                    &mut materials,
                    &material.0,
                    material_id,
                );
            }
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
}

/// Apply a normal / specular map pick to the selected faces' materials. A **live**
/// (non-final) pick previews the edited material on the faces in place (via
/// [`LegacyPreview`]) so the bump / specular renders at once; the **committed** (OK)
/// pick previews it and also sends the `RenderMaterials` PUT.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the picker replies, \
              the UI handle, the selection / object state, the legacy manager, the per-face lookup, \
              the live-preview state, the no-modify notice writer, and the command writer"
)]
fn apply_normal_specular_picked(
    mut picks: MessageReader<TexturePicked>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    legacy_manager: Res<LegacyMaterialManager>,
    prim_faces: PrimFaceLookup,
    mut preview: ResMut<LegacyPreview>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        let texture = pick.texture;
        let edit: Box<dyn Fn(&mut LegacyMaterial)> = if pick.requester == ui.normal_swatch {
            Box::new(move |material: &mut LegacyMaterial| material.normal_map = texture)
        } else if pick.requester == ui.specular_swatch {
            Box::new(move |material: &mut LegacyMaterial| material.specular_map = texture)
        } else {
            continue;
        };
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        preview_legacy_edit(&mut preview, &selection, &objects, &legacy_manager, &edit);
        if pick.final_pick {
            apply_legacy_edit(
                &selection,
                &legacy_manager,
                &prim_faces,
                &mut commands,
                &edit,
            );
        }
    }
}

/// Apply a specular-colour pick to the selected faces' materials (RGB, keeping full
/// alpha). A live (non-final) pick previews the highlight tint in place; the
/// committed pick previews it and sends the PUT.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the colour-picker \
              replies, the UI handle, the selection / object state, the legacy manager, the \
              per-face lookup, the live-preview state, the no-modify notice writer, and the command \
              writer"
)]
fn apply_spec_color_picked(
    mut picks: MessageReader<ColorPicked>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    legacy_manager: Res<LegacyMaterialManager>,
    prim_faces: PrimFaceLookup,
    mut preview: ResMut<LegacyPreview>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.requester != ui.spec_color_swatch {
            continue;
        }
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        let srgba = pick.color.to_srgba();
        let color = [
            round_to_byte(srgba.red * 255.0),
            round_to_byte(srgba.green * 255.0),
            round_to_byte(srgba.blue * 255.0),
            255,
        ];
        let edit = move |material: &mut LegacyMaterial| material.specular_color = color;
        preview_legacy_edit(&mut preview, &selection, &objects, &legacy_manager, edit);
        if pick.final_pick {
            apply_legacy_edit(
                &selection,
                &legacy_manager,
                &prim_faces,
                &mut commands,
                edit,
            );
        }
    }
}

/// Assign (or clear) a PBR render material on the selected faces via
/// `ModifyMaterialParams`: a final texture pick's UUID is the material asset to
/// apply (the nil id clears it). The reference's "assign a saved material to
/// faces".
fn apply_pbr_material_picked(
    mut picks: MessageReader<TexturePicked>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.requester != ui.pbr_swatch || !pick.final_pick {
            continue;
        }
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        let asset_id = pick.texture.uuid();
        let updates: Vec<MaterialOverrideUpdate> = selection
            .iter()
            .flat_map(|node| pbr_updates_for_node(node, asset_id))
            .collect();
        if !updates.is_empty() {
            commands.write(SlCommand(Command::ModifyMaterialParams { updates }));
        }
    }
}

/// **Live-preview** the material being browsed in the *Pick: Material* picker on
/// the selected faces, before OK — the reference viewer previews the highlighted
/// material on the prim as you scroll the list, and reverts on Cancel. The picker
/// emits a **non-final** [`TexturePicked`] on each selection (and the original id
/// on Cancel), so this applies whatever the pick carries as a no-wire preview
/// ([`MaterialManager::preview_face_material`]); the final OK pick, handled by
/// [`apply_pbr_material_picked`], is what actually sends the assignment. The nil
/// id a Cancel carries for a face that had no material reverts it to Blinn-Phong.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters: the picker replies, the UI handles, the selection, the \
              three material resources the preview composes through, and the scene / hierarchy / \
              face queries the per-face walk reads"
)]
fn preview_pbr_material_picked(
    mut picks: MessageReader<TexturePicked>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    mut manager: ResMut<MaterialManager>,
    mut textures: ResMut<TextureManager>,
    mut prim_textures: ResMut<PrimTextures>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    children: Query<&Children>,
    faces: Query<(
        &PrimFaceEntity,
        &MeshMaterial3d<FaceMaterial>,
        &FaceTextureDebug,
    )>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.requester != ui.pbr_swatch {
            continue;
        }
        let id = AssetKey::from(pick.texture.uuid());
        for node in selection.iter() {
            for (face_id, entity) in prim_faces_of_node(node, &scene, &children, &faces) {
                let Ok((_face, material, FaceTextureDebug(texture_face))) = faces.get(entity)
                else {
                    continue;
                };
                let handle = material.0.clone();
                let base_uv = materials
                    .get(&handle)
                    .map_or(bevy::math::Affine2::IDENTITY, |standard| {
                        standard.base.uv_transform
                    });
                let texture_face = *texture_face;
                manager.preview_face_material(
                    (node.scoped, face_id),
                    id,
                    &handle,
                    base_uv,
                    &texture_face,
                    &mut textures,
                    &mut prim_textures,
                    &mut materials,
                );
            }
        }
    }
}

/// Every prim face of a selection node's **own** prim (the walk stops at nested
/// linkset-child objects, matching the render-material assignment's per-prim
/// `side` = −1), as `(face index, entity)`, filtered to the node's selected face
/// set when it has one. Unlike [`pbr_faces_of`] this yields **all** faces, not
/// only those already carrying a render material — a material can be assigned to a
/// face that has none, so the live preview must reach those too.
fn prim_faces_of_node(
    node: &crate::world_api::SelectedNode,
    scene: &Query<(), With<crate::objects::SceneObject>>,
    children: &Query<&Children>,
    faces: &Query<(
        &PrimFaceEntity,
        &MeshMaterial3d<FaceMaterial>,
        &FaceTextureDebug,
    )>,
) -> Vec<(u8, Entity)> {
    let mut out: Vec<(u8, Entity)> = Vec::new();
    let mut stack = vec![node.entity];
    while let Some(entity) = stack.pop() {
        if entity != node.entity && scene.get(entity).is_ok() {
            continue;
        }
        if let Ok((face, _material, _debug)) = faces.get(entity)
            && let Ok(face_id) = u8::try_from(face.face_id.as_usize())
        {
            out.push((face_id, entity));
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
    match &node.faces {
        None => out,
        Some(set) => {
            let chosen: std::collections::HashSet<u16> =
                set.iter().map(|face| face.get()).collect();
            out.into_iter()
                .filter(|(face, _entity)| chosen.contains(&u16::from(*face)))
                .collect()
        }
    }
}

/// Commit a PBR channel transform edit on `Enter` in a focused field or when
/// focus leaves one: amend each selected PBR face's override with the changed
/// transform component and send it over `ModifyMaterialParams`. Only faces that
/// already carry a render material are touched.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection / object state, the active mode, the material manager, the focus + its blur \
              tracker, the field query, the render-material + hierarchy / scene queries the \
              per-face material lookup walks, the no-modify notice writer, and the command writer"
)]
fn commit_pbr_fields(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mode: Res<MatModeState>,
    mut material_manager: ResMut<MaterialManager>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: Local<Option<Entity>>,
    fields: Query<(Entity, &PbrField, &EditableText)>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !tool.active {
        *focus_track = None;
        return;
    }
    let focused = focus.get().filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused
    } else if *focus_track != focused {
        focus_track.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    *focus_track = focused;
    let Some(entity) = commit else {
        return;
    };
    let Ok((_entity, &field, editor)) = fields.get(entity) else {
        return;
    };
    let Some(value) = parse_tex_value(TextInputKind::Float, &editor.value().to_string()) else {
        return;
    };
    if !material_edit_allowed(&selection, &objects, &mut notices) {
        return;
    }
    let slots = pbr_channel_slots(mode.pbr_channel());
    let mut updates: Vec<MaterialOverrideUpdate> = Vec::new();
    for node in selection.iter() {
        let object_id: ObjectKey = node.full;
        for face_id in pbr_faces_of(node, &render_materials, &children, &scene) {
            let Some(material_id) =
                face_material_id(node.entity, face_id, &render_materials, &children, &scene)
            else {
                continue;
            };
            let base = material_manager.decoded_material(AssetKey::from(material_id));
            let existing = material_manager
                .face_override(node.scoped, face_id)
                .unwrap_or_default();
            let mut new_override = existing;
            for &slot in slots {
                let mut transform = base
                    .map(|material| slot_base_transform(material, slot))
                    .unwrap_or_default();
                fold_slot_override(&existing, slot, &mut transform);
                field.apply(&mut transform, value);
                if let Some(slot_ref) = new_override.transforms.get_mut(slot) {
                    *slot_ref = full_transform_override(transform);
                }
            }
            updates.push(MaterialOverrideUpdate {
                object_id,
                side: i32::from(face_id),
                gltf_json: Some(encode_override_gltf_json(&new_override)),
                asset_id: None,
            });
            // Show the transform edit at once, before the sim echoes it.
            material_manager.apply_local_override(node.scoped, face_id, &new_override);
        }
    }
    if !updates.is_empty() {
        commands.write(SlCommand(Command::ModifyMaterialParams { updates }));
    }
}

/// The selected face indices of a node that carry a GLTF render material (the
/// faces a PBR transform edit can touch): the chosen faces filtered to those in
/// the object's [`ObjectRenderMaterials`] holder, or every render-material face
/// for a whole-object selection.
fn pbr_faces_of(
    node: &crate::world_api::SelectedNode,
    render_materials: &Query<&ObjectRenderMaterials>,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
) -> Vec<u8> {
    let mut material_faces: Vec<u8> = Vec::new();
    let mut stack = vec![node.entity];
    while let Some(entity) = stack.pop() {
        if entity != node.entity && scene.get(entity).is_ok() {
            continue;
        }
        if let Ok(holder) = render_materials.get(entity) {
            for (face, _id) in &holder.faces {
                material_faces.push(*face);
            }
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
    match &node.faces {
        None => material_faces,
        Some(set) => {
            let chosen: std::collections::HashSet<u16> =
                set.iter().map(|face| face.get()).collect();
            material_faces
                .into_iter()
                .filter(|face| chosen.contains(&u16::from(*face)))
                .collect()
        }
    }
}

/// The base material's texture transform for slot `slot` (default when the slot
/// has no texture).
fn slot_base_transform(material: &GltfMaterial, slot: usize) -> GltfTextureTransform {
    let texture = match slot {
        0 => material.base_color_texture,
        1 => material.normal_texture,
        2 => material.metallic_roughness_texture,
        _emissive => material.emissive_texture,
    };
    texture.map_or_else(GltfTextureTransform::default, |texture| texture.transform)
}

/// Fold a face override's slot transform onto `transform`.
fn fold_slot_override(over: &MaterialOverride, slot: usize, transform: &mut GltfTextureTransform) {
    let Some(slot_over) = over.transforms.get(slot) else {
        return;
    };
    if let Some(offset) = slot_over.offset {
        transform.offset = offset;
    }
    if let Some(scale) = slot_over.scale {
        transform.scale = scale;
    }
    if let Some(rotation) = slot_over.rotation {
        transform.rotation = rotation;
    }
}

/// A fully-specified transform override (every component `Some`) from a resolved
/// transform — the shape a PBR transform edit writes so the whole transform is
/// re-sent (the reference writes the entire `mTextureTransform`).
const fn full_transform_override(transform: GltfTextureTransform) -> TextureTransformOverride {
    TextureTransformOverride {
        offset: Some(transform.offset),
        scale: Some(transform.scale),
        rotation: Some(transform.rotation),
    }
}

/// The `ModifyMaterialParams` updates that assign material `asset_id` to a
/// selection node's faces (one per chosen face, or `-1` for the whole object).
fn pbr_updates_for_node(
    node: &crate::world_api::SelectedNode,
    asset_id: Uuid,
) -> Vec<MaterialOverrideUpdate> {
    let object_id: ObjectKey = node.full;
    match &node.faces {
        None => vec![MaterialOverrideUpdate {
            object_id,
            side: -1,
            gltf_json: None,
            asset_id: Some(asset_id),
        }],
        Some(set) => set
            .iter()
            .map(|face| i32::from(face.get()))
            .map(|side| MaterialOverrideUpdate {
                object_id,
                side,
                gltf_json: None,
                asset_id: Some(asset_id),
            })
            .collect(),
    }
}

/// Apply `edit` to every selected PBR face's override and send them over
/// `ModifyMaterialParams` — the shared spine of the PBR channel edits (textures /
/// tints / factors / alpha / double-sided). Each face's current override is read,
/// amended, re-serialised ([`encode_override_gltf_json`]) and sent, matching the
/// reference `updateSelectedGLTFMaterials`.
fn apply_pbr_override(
    selection: &SelectionSet,
    material_manager: &mut MaterialManager,
    render_materials: &Query<&ObjectRenderMaterials>,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
    commands: &mut MessageWriter<SlCommand>,
    edit: impl Fn(&mut MaterialOverride),
) {
    let mut updates: Vec<MaterialOverrideUpdate> = Vec::new();
    for node in selection.iter() {
        let object_id: ObjectKey = node.full;
        for face_id in pbr_faces_of(node, render_materials, children, scene) {
            let mut over = material_manager
                .face_override(node.scoped, face_id)
                .unwrap_or_default();
            edit(&mut over);
            updates.push(MaterialOverrideUpdate {
                object_id,
                side: i32::from(face_id),
                gltf_json: Some(encode_override_gltf_json(&over)),
                asset_id: None,
            });
            // Show the edit at once (swatch + prim), before the sim echoes it.
            material_manager.apply_local_override(node.scoped, face_id, &over);
        }
    }
    if !updates.is_empty() {
        commands.write(SlCommand(Command::ModifyMaterialParams { updates }));
    }
}

/// Assign a base / metallic-roughness / emissive / normal texture (final pick) to
/// the selected PBR faces via an override — the reference material editor's
/// per-channel texture pickers. A nil pick clears the slot's texture.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters: the picker replies, the UI handles, the selection / \
              object state + material manager, the render-material / hierarchy / scene queries the \
              per-face lookup walks, the no-modify notice writer, and the command writer"
)]
fn apply_pbr_texture_picked(
    mut picks: MessageReader<TexturePicked>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut material_manager: ResMut<MaterialManager>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if !pick.final_pick {
            continue;
        }
        let slot = if pick.requester == ui.pbr_base_swatch {
            SLOT_BASE_COLOR
        } else if pick.requester == ui.pbr_metallic_swatch {
            SLOT_METALLIC_ROUGHNESS
        } else if pick.requester == ui.pbr_emissive_swatch {
            SLOT_EMISSIVE
        } else if pick.requester == ui.pbr_normal_swatch {
            SLOT_NORMAL
        } else {
            continue;
        };
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        let texture = pick.texture;
        apply_pbr_override(
            &selection,
            &mut material_manager,
            &render_materials,
            &children,
            &scene,
            &mut commands,
            |over| {
                if let Some(slot_ref) = over.textures.get_mut(slot) {
                    *slot_ref = Some(if texture.uuid().is_nil() {
                        TextureOverride::Clear
                    } else {
                        TextureOverride::Set(texture)
                    });
                }
            },
        );
    }
}

/// Assign a base-colour or emissive tint (final pick) to the selected PBR faces
/// via an override — the reference material editor's colour swatches.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters: the picker replies, the UI handles, the selection / \
              object state + material manager, the render-material / hierarchy / scene queries, the \
              no-modify notice writer, and the command writer"
)]
fn apply_pbr_tint_picked(
    mut picks: MessageReader<ColorPicked>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut material_manager: ResMut<MaterialManager>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if !pick.final_pick {
            continue;
        }
        if pick.requester != ui.pbr_base_tint && pick.requester != ui.pbr_emissive_tint {
            continue;
        }
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        if pick.requester == ui.pbr_base_tint {
            let rgba = linear_rgba_of(pick.color);
            apply_pbr_override(
                &selection,
                &mut material_manager,
                &render_materials,
                &children,
                &scene,
                &mut commands,
                |over| over.base_color = Some(rgba),
            );
        } else if pick.requester == ui.pbr_emissive_tint {
            let rgb = linear_rgb_of(pick.color);
            apply_pbr_override(
                &selection,
                &mut material_manager,
                &render_materials,
                &children,
                &scene,
                &mut commands,
                |over| over.emissive_factor = Some(rgb),
            );
        }
    }
}

/// Apply a PBR alpha-mode combo pick to the selected faces via an override.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters: the combo changes, the tagging query, the UI handles, \
              the selection / object state + material manager, the render-material / hierarchy / \
              scene queries, the no-modify notice writer, and the command writer"
)]
fn apply_pbr_alpha_change(
    mut changes: MessageReader<ComboChanged>,
    combos: Query<(), With<PbrAlphaCombo>>,
    ui: Option<Res<BuildMaterialUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut material_manager: ResMut<MaterialManager>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for change in changes.read() {
        if change.combo != ui.pbr_alpha_combo || !combos.contains(change.combo) {
            continue;
        }
        if !material_edit_allowed(&selection, &objects, &mut notices) {
            continue;
        }
        let mode = pbr_alpha_mode(change.active);
        apply_pbr_override(
            &selection,
            &mut material_manager,
            &render_materials,
            &children,
            &scene,
            &mut commands,
            |over| over.alpha_mode = Some(mode),
        );
    }
}

/// Commit a PBR scalar factor edit (metallic / roughness / alpha cutoff) on Enter
/// or blur via an override.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters: the tool / selection / object state, the material \
              manager, the focus + its blur tracker, the field query, the render-material / \
              hierarchy / scene queries, the keyboard, the no-modify notice writer, and the command \
              writer"
)]
fn commit_pbr_scalars(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut material_manager: ResMut<MaterialManager>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: Local<Option<Entity>>,
    fields: Query<(Entity, &PbrScalarField, &EditableText)>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !tool.active {
        *focus_track = None;
        return;
    }
    let focused = focus.get().filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused
    } else if *focus_track != focused {
        focus_track.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    *focus_track = focused;
    let Some(entity) = commit else {
        return;
    };
    let Ok((_entity, &field, editor)) = fields.get(entity) else {
        return;
    };
    let Some(value) = parse_tex_value(TextInputKind::Float, &editor.value().to_string()) else {
        return;
    };
    if !material_edit_allowed(&selection, &objects, &mut notices) {
        return;
    }
    apply_pbr_override(
        &selection,
        &mut material_manager,
        &render_materials,
        &children,
        &scene,
        &mut commands,
        |over| field.apply(over, value),
    );
}

/// Toggle the double-sided flag on the selected PBR faces (reads the primary
/// face's current effective value, flips it).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters: the press event, the button tag, the selection / \
              object state + material manager, the render-material / hierarchy / scene queries, the \
              no-modify notice writer, and the command writer"
)]
fn handle_double_sided_press(
    press: On<Pointer<Press>>,
    buttons: Query<(), With<DoubleSidedButton>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut material_manager: ResMut<MaterialManager>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary || !buttons.contains(press.entity) {
        return;
    }
    if !material_edit_allowed(&selection, &objects, &mut notices) {
        return;
    }
    let (_id, base, over) = representative_pbr(
        &selection,
        &render_materials,
        &material_manager,
        &children,
        &scene,
    );
    let current = effective_pbr_material(base, over.as_ref()).double_sided;
    apply_pbr_override(
        &selection,
        &mut material_manager,
        &render_materials,
        &children,
        &scene,
        &mut commands,
        |over| over.double_sided = Some(!current),
    );
}

/// Assign the blank GLTF material to the selected faces — the "New material"
/// action (the reference's `BLANK_MATERIAL_ASSET_ID`). Applies to every selected
/// face regardless of whether it already has a material.
fn handle_pbr_new_press(
    press: On<Pointer<Press>>,
    buttons: Query<(), With<PbrNewButton>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary || !buttons.contains(press.entity) {
        return;
    }
    if !material_edit_allowed(&selection, &objects, &mut notices) {
        return;
    }
    let updates: Vec<MaterialOverrideUpdate> = selection
        .iter()
        .flat_map(|node| pbr_updates_for_node(node, BLANK_MATERIAL_ASSET_ID))
        .collect();
    if !updates.is_empty() {
        commands.write(SlCommand(Command::ModifyMaterialParams { updates }));
    }
}

/// Save the primary face's effective PBR material to inventory as a new
/// `AT_MATERIAL` asset (the reference material editor's Save): encode it and
/// upload it into the agent's Materials folder over `NewFileAgentInventory`. The
/// new asset lands in inventory; the user assigns it to faces via the
/// render-material swatch (auto-assign-on-save is not wired).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters: the press event, the button tag, the selection + \
              material manager + inventory model, the render-material / hierarchy / scene \
              queries, and the command writer"
)]
fn handle_pbr_save_press(
    press: On<Pointer<Press>>,
    buttons: Query<(), With<PbrSaveButton>>,
    selection: Res<SelectionSet>,
    material_manager: Res<MaterialManager>,
    inventory: Res<crate::inventory::InventoryModel>,
    render_materials: Query<&ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary || !buttons.contains(press.entity) {
        return;
    }
    let (material_id, base, over) = representative_pbr(
        &selection,
        &render_materials,
        &material_manager,
        &children,
        &scene,
    );
    if material_id.is_none() {
        // No PBR material on the face to save.
        return;
    }
    let effective = effective_pbr_material(base, over.as_ref());
    let Some(folder_id) = inventory
        .folder_by_type(sl_client_bevy::FolderType::Material)
        .or_else(|| inventory.agent_root())
    else {
        return;
    };
    let data = sl_client_bevy::encode_material_asset(&effective);
    commands.write(SlCommand(Command::UploadAsset {
        folder_id,
        asset_type: AssetType::Material,
        inventory_type: InventoryType::Material,
        name: "New Material".to_owned(),
        description: String::new(),
        next_owner_mask: PERM_COPY_MODIFY_TRANSFER,
        group_mask: 0,
        everyone_mask: 0,
        expected_upload_cost: 0,
        data,
    }));
}

/// Apply `edit` to every selected face's resolved legacy material and send them
/// over the `RenderMaterials` PUT — the shared spine of the legacy commits.
fn apply_legacy_edit(
    selection: &SelectionSet,
    legacy_manager: &LegacyMaterialManager,
    prim_faces: &PrimFaceLookup,
    commands: &mut MessageWriter<SlCommand>,
    edit: impl Fn(&mut LegacyMaterial),
) {
    let mut updates: Vec<FaceMaterialPut> = Vec::new();
    for node in selection.iter() {
        let local_id = node.scoped().id().0;
        let faces = prim_faces.current_faces(node.entity);
        if faces.is_empty() {
            continue;
        }
        for index in node_face_indices(node, faces.len()) {
            let Some(face) = faces.get(index) else {
                continue;
            };
            let Ok(face_id) = u8::try_from(index) else {
                continue;
            };
            let mut material = legacy_material_of(face, legacy_manager);
            edit(&mut material);
            updates.push(FaceMaterialPut {
                local_id,
                face: face_id,
                material: Some(material),
            });
        }
    }
    if !updates.is_empty() {
        commands.write(SlCommand(Command::SetRenderMaterials { updates }));
    }
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// Format a field value the way its input kind displays it (integer, or three
/// decimals for a float).
fn format_field(kind: TextInputKind, value: f32) -> String {
    match kind {
        TextInputKind::Integer => round_to_i64(value).to_string(),
        _float => format!("{value:.3}"),
    }
}

/// Round and clamp a display value to a `u8` material byte.
const fn clamp_to_byte(value: f32) -> u8 {
    round_to_byte(value)
}

/// Round a display value to a colour / scalar byte, clamped to 0..=255.
const fn round_to_byte(value: f32) -> u8 {
    let clamped = value.clamp(0.0, 255.0).round();
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 and rounded, so the f32 → u8 narrowing is exact"
    )]
    let byte = clamped as u8;
    byte
}

/// Round a display value to the integer a scalar field shows.
const fn round_to_i64(value: f32) -> i64 {
    let rounded = value.round();
    if rounded.is_finite() {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "a bounded 0..=255 scalar entry, finite and small"
        )]
        let int = rounded as i64;
        int
    } else {
        0
    }
}

/// Widen a combo index to the `f32` the byte clamp takes.
const fn from_usize(value: usize) -> f32 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "a small combo index (0..=3) widens exactly to f32"
    )]
    let float = value as f32;
    float
}

/// The byte at `index` of an RGBA quad (0 outside 0..4).
fn byte_at(color: [u8; 4], index: usize) -> u8 {
    color.get(index).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{LegacyField, default_legacy_material, round_to_byte};

    /// A legacy transform field round-trips degrees ⇔ radians.
    #[test]
    fn normal_rotation_is_degrees() {
        let mut material = default_legacy_material();
        LegacyField::NormalRotation.apply(&mut material, 90.0);
        assert!((material.normal_rotation - core::f32::consts::FRAC_PI_2).abs() < 1.0e-4);
        assert!((LegacyField::NormalRotation.display_value(&material) - 90.0).abs() < 1.0e-3);
    }

    /// The glossiness scalar clamps into the byte range.
    #[test]
    fn glossiness_clamps() {
        let mut material = default_legacy_material();
        LegacyField::Glossiness.apply(&mut material, 300.0);
        assert_eq!(material.specular_exponent, 255);
        assert!((LegacyField::Glossiness.display_value(&material) - 255.0).abs() < f32::EPSILON);
    }

    /// The default material carries the reference specular exponent.
    #[test]
    fn default_specular_exponent() {
        assert_eq!(default_legacy_material().specular_exponent, 51);
    }

    /// The byte rounder clamps and rounds.
    #[test]
    fn byte_rounder_clamps() {
        assert_eq!(round_to_byte(-5.0), 0);
        assert_eq!(round_to_byte(300.0), 255);
        assert_eq!(round_to_byte(127.6), 128);
    }
}
