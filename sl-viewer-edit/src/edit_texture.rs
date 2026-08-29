//! The build floater's **Texture tab** (`viewer-prim-texture-editing`, the
//! legacy per-face surface half): the per-face colour / transparency / glow /
//! full-bright / bumpiness / shininess / mapping and the texture repeats /
//! offset / rotation, applied to the faces the Select Face tool
//! ([`crate::edit_selection`], `viewer-edit-face-selection`) picked — or to the
//! whole object when no individual face is chosen.
//!
//! # Model
//!
//! - Every widget reads the **primary selection**'s representative face: the
//!   object's decoded [`TextureEntry`]
//!   ([`ObjectState::texture_entry_of`]) indexed by the lowest selected face (or
//!   face 0 for a whole-object selection). Widgets rewrite only when that
//!   snapshot **changes** (`TexShownSnapshot`), so a just-committed edit is
//!   not clobbered back to the pre-echo value while the simulator's confirming
//!   `ObjectUpdate` is in flight.
//! - A **commit** touches exactly one texture-entry attribute (the reference's
//!   `sendColor` / `sendGlow` / `sendBump` … split — never a whole-face
//!   rewrite, so faces with differing other params keep them): it decodes each
//!   selected object's current entry, applies the one changed attribute to the
//!   selected faces (or every face), and sends the whole modified
//!   [`TextureEntry`] as an `ObjectImage` ([`Command::SetObjectImage`]),
//!   preserving the object's media URL. The visible faces re-texture when the
//!   simulator echoes the change (a texture change re-tessellates in
//!   [`crate::objects::update_objects`]) — the same round-trip the shape editors
//!   ([`crate::edit_params`]) rely on.
//! - Deliberate deviations, pending their own tasks: the colour is three numeric
//!   sRGB-byte fields ([[viewer-ui-color-picker]], as the light colour already
//!   is), the diffuse texture image is read-only ([[viewer-ui-texture-picker]],
//!   as the sculpt / projector textures already are), and the normal / specular
//!   (`LLMaterial`) and GLTF / PBR channels are their own material-editor tasks.
//!
//! Reference (Firestorm, read-only): `llpanelface`, `lltoolface`; message
//! `ObjectImage`.

use crate::world_api::{MatChannel, MatMedia, MatModeState, PbrChannel};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    Command, PrimFaceId, ScopedObjectId, SlCommand, TextureEntry, TextureFace, TextureKey, Uuid,
    decode_texture_entry,
};

use crate::edit_params::set_disabled_class;
use crate::edit_tool::{
    BuildTabPages, CHECKED_GLYPH, LABEL_CLASS, TOOL_FONT_SIZE, UNCHECKED_GLYPH, VALUE_CLASS,
    spawn_row_label,
};
use crate::face_material::FaceMaterial;
use crate::i18n::{TransArgs, Translated, Translator};
use crate::objects::{FaceTextureDebug, PrimFaceEntity, TEXTURE_EDIT_LOG_TARGET};
use crate::ui::row;
use crate::ui_color_picker::{ColorPicked, ColorSwatchValue, spawn_color_swatch};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_radio::{RadioLayout, RadioSelection, RadioSpec, spawn_radio_group};
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::ui_text::set_editor_text;
use crate::ui_text_input::{TextInputKind, TextInputSpec, TextInputValue, spawn_text_input};
use crate::ui_texture_picker::{TextureSwatchValue, spawn_texture_swatch};
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::EditToolState;
use crate::world_api::ObjectState;
use crate::world_api::SelectionSet;
use crate::world_api::TexturePicked;

/// The tab index the Texture-tab widgets start their focus order at (well past
/// the Object / Features tabs' fields).
const TEX_TAB_INDEX: i32 = 400;

/// The width, in `"0"`-glyph advances, of a Texture-tab numeric field.
const TEX_FIELD_GLYPHS: f32 = 7.0;

// ---------------------------------------------------------------------------
// Material mode / channel selection (the reference's `combobox matmedia` +
// `radio_material_type` + `radio_pbr_type`, `llpanelface.cpp`).
// ---------------------------------------------------------------------------

/// When a mode-dependent Texture-tab control is shown: a single visibility system
/// ([`apply_material_mode_visibility`]) sets each tagged control's `Node.display`
/// from the current [`MatModeState`], mirroring the reference
/// `LLPanelFace::updateVisibility`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowWhen {
    /// Material mode, diffuse (Texture) channel: the diffuse swatch, colour,
    /// transparency, alpha mode / mask cutoff and the diffuse transforms.
    MaterialDiffuse,
    /// Material mode, normal (Bumpiness) channel: the normal swatch, bumpiness
    /// combo and the normal-map transforms.
    MaterialNormal,
    /// Material mode, specular (Shininess) channel: the specular swatch,
    /// shininess combo, glossiness / environment / specular colour and the
    /// specular-map transforms.
    MaterialSpecular,
    /// Material mode, any channel: the glow / full-bright / mapping / align
    /// controls (`TextureEntry`-level, shared across the map channels).
    MaterialAny,
    /// PBR mode, any channel: the per-channel PBR transforms.
    PbrAny,
    /// PBR mode, the Complete-material channel: the render-material swatch, the
    /// New / Save buttons, and the material-level alpha mode / cutoff /
    /// double-sided controls.
    PbrMaterialId,
    /// PBR mode, the Base-colour channel: its texture swatch + tint.
    PbrBaseColor,
    /// PBR mode, the Metallic-roughness channel: its texture swatch + the
    /// metallic / roughness factors.
    PbrMetallic,
    /// PBR mode, the Emissive channel: its texture swatch + tint.
    PbrEmissive,
    /// PBR mode, the Normal channel: its texture swatch.
    PbrNormal,
}

impl ShowWhen {
    /// Whether a control tagged with this rule is shown in `state`.
    pub(crate) const fn matches(self, state: MatModeState) -> bool {
        match self {
            Self::MaterialDiffuse => {
                state.is_material() && matches!(state.mat_type, MatChannel::Diffuse)
            }
            Self::MaterialNormal => {
                state.is_material() && matches!(state.mat_type, MatChannel::Normal)
            }
            Self::MaterialSpecular => {
                state.is_material() && matches!(state.mat_type, MatChannel::Specular)
            }
            Self::MaterialAny => state.is_material(),
            Self::PbrAny => state.is_pbr(),
            Self::PbrMaterialId => state.is_pbr() && matches!(state.pbr_type, PbrChannel::Material),
            Self::PbrBaseColor => state.is_pbr() && matches!(state.pbr_type, PbrChannel::BaseColor),
            Self::PbrMetallic => {
                state.is_pbr() && matches!(state.pbr_type, PbrChannel::MetallicRoughness)
            }
            Self::PbrEmissive => state.is_pbr() && matches!(state.pbr_type, PbrChannel::Emissive),
            Self::PbrNormal => state.is_pbr() && matches!(state.pbr_type, PbrChannel::Normal),
        }
    }
}

// ---------------------------------------------------------------------------
// The editable per-face attributes.
// ---------------------------------------------------------------------------

/// One numeric Texture-tab field and the texture-entry attribute it edits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum TexField {
    /// Transparency percent, 0..100 (`alpha = (100 - value) / 100`).
    Transparency,
    /// Glow, 0..1.
    Glow,
    /// Horizontal texture repeats (`scale_s`).
    RepeatU,
    /// Vertical texture repeats (`scale_t`).
    RepeatV,
    /// Horizontal texture offset, −1..1 (`offset_s`).
    OffsetU,
    /// Vertical texture offset, −1..1 (`offset_t`).
    OffsetV,
    /// Texture rotation, in degrees (the wire is radians).
    Rotation,
}

impl TexField {
    /// The widget element id, for the skin / harness.
    const fn element(self) -> &'static str {
        match self {
            Self::Transparency => "build-tex-transparency",
            Self::Glow => "build-tex-glow",
            Self::RepeatU => "build-tex-repeat-u",
            Self::RepeatV => "build-tex-repeat-v",
            Self::OffsetU => "build-tex-offset-u",
            Self::OffsetV => "build-tex-offset-v",
            Self::Rotation => "build-tex-rotation",
        }
    }

    /// The field's input kind (colour / transparency are integers, the rest
    /// floats).
    const fn input_kind(self) -> TextInputKind {
        match self {
            Self::Transparency => TextInputKind::Integer,
            _float => TextInputKind::Float,
        }
    }

    /// Read the field's **display** value off a decoded face — the planar-texgen
    /// faces show 2× the stored repeats, the reference's `getScaleS/T` ×2 quirk.
    fn display_value(self, face: &TextureFace) -> f32 {
        let planar = face.is_planar_texgen();
        match self {
            Self::Transparency => {
                let alpha = f32::from(byte_at(face.color, 3));
                ((1.0 - alpha / 255.0) * 100.0).round()
            }
            Self::Glow => face.glow,
            Self::RepeatU => face.scale_s * if planar { 2.0 } else { 1.0 },
            Self::RepeatV => face.scale_t * if planar { 2.0 } else { 1.0 },
            Self::OffsetU => face.offset_s,
            Self::OffsetV => face.offset_t,
            Self::Rotation => face.rotation.to_degrees(),
        }
    }

    /// Apply the field's committed value to a face — the one attribute this field
    /// owns, leaving the rest of the face untouched (the reference's per-attribute
    /// `send*`). Planar-texgen repeats are stored at half the entered value (the
    /// ×2 display quirk's inverse).
    fn apply(self, face: &mut TextureFace, value: f32) {
        let planar = face.is_planar_texgen();
        match self {
            Self::Transparency => {
                let transp = value.clamp(0.0, 100.0);
                set_byte(
                    &mut face.color,
                    3,
                    round_to_byte((1.0 - transp / 100.0) * 255.0),
                );
            }
            Self::Glow => face.glow = value.clamp(0.0, 1.0),
            Self::RepeatU => face.scale_s = value / if planar { 2.0 } else { 1.0 },
            Self::RepeatV => face.scale_t = value / if planar { 2.0 } else { 1.0 },
            Self::OffsetU => face.offset_s = value.clamp(-1.0, 1.0),
            Self::OffsetV => face.offset_t = value.clamp(-1.0, 1.0),
            Self::Rotation => face.rotation = value.to_radians(),
        }
    }
}

/// The diffuse field rows, in the order they appear in the tab; each tuple is a
/// label key, the fields on its row, and the material mode the row shows in.
/// Transparency and the diffuse transforms belong to the diffuse (Texture)
/// channel; glow is a `TextureEntry`-level attribute shown across the Material
/// channels.
const TEX_FIELD_ROWS: &[(&str, &[TexField], ShowWhen)] = &[
    (
        "build-tex-transparency-label",
        &[TexField::Transparency],
        ShowWhen::MaterialDiffuse,
    ),
    (
        "build-tex-glow-label",
        &[TexField::Glow],
        ShowWhen::MaterialAny,
    ),
    (
        "build-tex-repeats-label",
        &[TexField::RepeatU, TexField::RepeatV],
        ShowWhen::MaterialDiffuse,
    ),
    (
        "build-tex-offset-label",
        &[TexField::OffsetU, TexField::OffsetV],
        ShowWhen::MaterialDiffuse,
    ),
    (
        "build-tex-rotation-label",
        &[TexField::Rotation],
        ShowWhen::MaterialDiffuse,
    ),
];

/// A Texture-tab boolean toggle and the packed bit it flips.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum TexToggle {
    /// Full-bright (unlit) — bit 5 of the bump/shiny/fullbright byte.
    Fullbright,
}

impl TexToggle {
    /// Whether the toggle is on for a face.
    const fn get(self, face: &TextureFace) -> bool {
        match self {
            Self::Fullbright => face.fullbright(),
        }
    }

    /// Set the toggle on a face.
    const fn set(self, face: &mut TextureFace, on: bool) {
        match self {
            Self::Fullbright => {
                let bit = 1_u8 << 5;
                if on {
                    face.bump_shiny_fullbright |= bit;
                } else {
                    face.bump_shiny_fullbright &= !bit;
                }
            }
        }
    }
}

/// A Texture-tab cycle button (a combo stand-in, [[viewer-ui-combo-widget]]) and
/// the packed enum it advances.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum TexCycle {
    /// Bumpiness — the low 5 bits, legacy values 0..=17 (the reference's
    /// `combobox bumpiness`, its `Use texture` entry needs a normal map and is
    /// its own material-editor task).
    Bump,
    /// Shininess — the top 2 bits, 0..=3 (None / Low / Medium / High).
    Shininess,
    /// Mapping — texgen Default (0) or Planar (1).
    TexGen,
}

impl TexCycle {
    /// The current value index for a face.
    fn index(self, face: &TextureFace) -> usize {
        match self {
            Self::Bump => usize::from(face.bumpmap()),
            Self::Shininess => usize::from(face.shininess()),
            Self::TexGen => usize::from(face.is_planar_texgen()),
        }
    }

    /// How many values the cycle steps through.
    const fn count(self) -> usize {
        match self {
            Self::Bump => 18,
            Self::Shininess => 4,
            Self::TexGen => 2,
        }
    }

    /// The combo element id, for the widget name / harness.
    const fn element(self) -> &'static str {
        match self {
            Self::Bump => "build-tex-bump",
            Self::Shininess => "build-tex-shiny",
            Self::TexGen => "build-tex-texgen",
        }
    }

    /// The Fluent label key for value `index`.
    fn label_key(self, index: usize) -> &'static str {
        match self {
            Self::Bump => BUMP_LABELS.get(index).copied().unwrap_or("build-bump-none"),
            Self::Shininess => SHINY_LABELS
                .get(index)
                .copied()
                .unwrap_or("build-shiny-none"),
            Self::TexGen => {
                if index == 0 {
                    "build-texgen-default"
                } else {
                    "build-texgen-planar"
                }
            }
        }
    }

    /// Set value `index` on a face, preserving the bits the cycle does not own.
    fn set(self, face: &mut TextureFace, index: usize) {
        let byte = u8::try_from(index).unwrap_or(0);
        match self {
            Self::Bump => {
                let value = byte & 0x1f;
                face.bump_shiny_fullbright = (face.bump_shiny_fullbright & !0x1f) | value;
            }
            Self::Shininess => {
                let value = (byte & 0x03) << 6;
                face.bump_shiny_fullbright = (face.bump_shiny_fullbright & !0xc0) | value;
            }
            Self::TexGen => {
                // Clear the texgen bits (0x06), set planar (0x02) for index 1.
                let planar = if index == 1 { 0x02 } else { 0x00 };
                face.media_flags = (face.media_flags & !0x06) | planar;
            }
        }
    }
}

/// The 18 legacy bumpiness value labels, indexed by wire value.
const BUMP_LABELS: [&str; 18] = [
    "build-bump-none",
    "build-bump-bright",
    "build-bump-dark",
    "build-bump-woodgrain",
    "build-bump-bark",
    "build-bump-bricks",
    "build-bump-checker",
    "build-bump-concrete",
    "build-bump-crustytile",
    "build-bump-cutstone",
    "build-bump-discs",
    "build-bump-gravel",
    "build-bump-petridish",
    "build-bump-siding",
    "build-bump-stonetile",
    "build-bump-stucco",
    "build-bump-suction",
    "build-bump-weave",
];

/// The four shininess value labels, indexed by wire value.
const SHINY_LABELS: [&str; 4] = [
    "build-shiny-none",
    "build-shiny-low",
    "build-shiny-medium",
    "build-shiny-high",
];

/// A read-only info line the sync pass rewrites.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum TexInfo {
    /// Which faces the edits will hit (the selected-face count, or "all faces").
    Faces,
}

/// A toggle row's check-glyph marker.
#[derive(Component, Debug, Clone, Copy)]
struct TexToggleGlyph(TexToggle);

/// Tags a Texture-tab combo with the packed enum it drives, so the combo sync
/// and the [`ComboChanged`] handler map the combo back to its attribute.
#[derive(Component, Debug, Clone, Copy)]
struct TexCombo(TexCycle);

/// The Align-planar-faces action button.
#[derive(Component, Debug, Clone, Copy)]
struct TexAlignButton;

/// Marks an interactive Texture-tab control (a numeric field, the full-bright
/// toggle, a combo anchor, a swatch, the align button) so the sync pass can
/// **gate** it — the reference greys and disables the whole panel while nothing is
/// selected. A gated-off control ignores the pointer (no focus, no popover, no
/// press) and its label / value text greys via [`crate::edit_params::DISABLED_CLASS`].
#[derive(Component, Debug, Clone, Copy)]
struct TexControl;

/// The last-shown Texture-tab snapshot, so widgets rewrite only on a real change.
#[derive(Resource, Debug, Default)]
struct TexShownSnapshot {
    /// The last displayed `(object, representative face, selected-face signature)`,
    /// or `None` when nothing valid is shown.
    shown: Option<(ScopedObjectId, TextureFace, u64)>,
    /// The last applied enabled state, so the panel gates its controls only on a
    /// selected/deselected transition rather than every frame.
    enabled: Option<bool>,
}

/// Which Texture-tab field held focus last frame, to commit on blur.
#[derive(Resource, Debug, Default)]
struct TexFieldFocus {
    /// The field entity focused last frame, if any.
    last: Option<Entity>,
}

/// The plugin wiring the Texture tab into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct EditTexturePlugin;

impl Plugin for EditTexturePlugin {
    /// Spawn the Texture-tab widgets (after the build floater's tab pages exist)
    /// and run the sync + commit systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<TexShownSnapshot>()
            .init_resource::<TexFieldFocus>()
            .init_resource::<TexturePreview>()
            .init_resource::<MatModeState>()
            .init_resource::<MatModeSelected>()
            // Fills the Build floater's Texture page once its lazily-built
            // content publishes the pages (`BuildTabPages` appears on first
            // open); ordered after the general parameter tabs, as before.
            .add_systems(
                Update,
                spawn_texture_tab
                    .run_if(resource_added::<crate::edit_tool::BuildTabPages>)
                    .after(crate::edit_params::spawn_param_tabs),
            )
            // Gated on build mode: the widget sync / commit systems already
            // bailed on `!active`, and the live-preview systems only ever have a
            // preview to drive or revert while (or just after) building. The
            // settling window lets `revert_texture_preview_on_deselect` restore
            // the previewed faces on the close edge, after the selection clears.
            .add_systems(
                Update,
                (
                    auto_select_material_mode,
                    read_material_mode,
                    apply_material_mode_visibility,
                    sync_texture_widgets,
                    commit_texture_fields,
                    apply_tex_combo_changes,
                    apply_color_picked,
                    apply_texture_picked,
                    drive_texture_preview,
                    revert_texture_preview_on_deselect,
                )
                    .chain()
                    .run_if(crate::edit_tool::edit_tool_active_or_settling),
            );
    }
}

/// Spawn the Texture-tab editors into the build floater's Texture page.
fn spawn_texture_tab(mut commands: Commands, pages: Option<Res<BuildTabPages>>) {
    let Some(pages) = pages else {
        return;
    };
    let page = pages.texture;
    let mut tab_index = TEX_TAB_INDEX;

    // The selected-face summary.
    spawn_info_row(&mut commands, page, TexInfo::Faces, "build-tex-faces-all");

    // The material-mode / channel selectors (the reference's `combobox matmedia`
    // + `radio_material_type` + `radio_pbr_type`): which of the legacy diffuse /
    // normal / specular maps or the PBR render material the tab edits.
    let selectors = spawn_mode_selectors(&mut commands, page, &mut tab_index);

    // The diffuse texture swatch (opens the texture picker) and the colour swatch
    // (opens the colour picker) — the Material / Texture channel.
    let texture_row = spawn_row(&mut commands, page, "build-tex-texture-id-label");
    commands
        .entity(texture_row)
        .insert(ShowWhen::MaterialDiffuse);
    let texture_swatch = spawn_texture_swatch(
        &mut commands,
        texture_row,
        "build-tex-texture",
        tab_index,
        TextureKey::from(Uuid::nil()),
    );
    commands.entity(texture_swatch).insert(TexControl);
    tab_index = tab_index.saturating_add(1);

    let color_row = spawn_row(&mut commands, page, "build-tex-color-label");
    commands.entity(color_row).insert(ShowWhen::MaterialDiffuse);
    let color_swatch = spawn_color_swatch(
        &mut commands,
        color_row,
        "build-tex-color",
        tab_index,
        Color::WHITE,
    );
    commands.entity(color_swatch).insert(TexControl);
    tab_index = tab_index.saturating_add(1);

    // Transparency / glow / repeats / offset / rotation numeric rows.
    for (label_key, fields, show_when) in TEX_FIELD_ROWS {
        spawn_tex_field_row(
            &mut commands,
            page,
            label_key,
            fields,
            *show_when,
            &mut tab_index,
        );
    }

    // Full-bright toggle (a `TextureEntry`-level attribute, shown across the
    // Material channels).
    let fullbright = spawn_tex_toggle(
        &mut commands,
        page,
        TexToggle::Fullbright,
        "build-tex-fullbright",
        &mut tab_index,
    );
    commands.entity(fullbright).insert(ShowWhen::MaterialAny);

    // Bumpiness / shininess / mapping combo boxes (the reference's `combobox
    // bumpiness` (normal channel) / `combobox shininess` (specular channel) /
    // `combobox texgen` (shared)).
    for (cycle, label_key, show_when) in [
        (
            TexCycle::Bump,
            "build-tex-bump-label",
            ShowWhen::MaterialNormal,
        ),
        (
            TexCycle::Shininess,
            "build-tex-shiny-label",
            ShowWhen::MaterialSpecular,
        ),
        (
            TexCycle::TexGen,
            "build-tex-mapping-label",
            ShowWhen::MaterialAny,
        ),
    ] {
        let row_entity = spawn_row(&mut commands, page, label_key);
        commands.entity(row_entity).insert(show_when);
        spawn_tex_combo(&mut commands, row_entity, cycle, &mut tab_index);
    }

    // Align planar faces (the reference's `checkbox planar align`, offered as a
    // one-shot action button that aligns the selected faces to the primary face).
    let align = spawn_align_button(&mut commands, page, &mut tab_index);
    commands.entity(align).insert(ShowWhen::MaterialAny);

    // The Blinn-Phong normal / specular channels and the PBR channels
    // ([`crate::edit_material`]) share this page and the mode selectors.
    crate::edit_material::spawn_material_channels(&mut commands, page, &mut tab_index);

    commands.insert_resource(BuildTextureUi {
        page,
        color_swatch,
        texture_swatch,
        matmedia_strip: selectors.matmedia_strip,
        mat_type_radio: selectors.mat_type_radio,
        pbr_type_radio: selectors.pbr_type_radio,
    });
}

/// The three material-mode selector entities the mode systems read / write.
struct ModeSelectors {
    /// The `matmedia` tab strip (Material / PBR).
    matmedia_strip: Entity,
    /// The `radio_material_type` group (Texture / Bumpiness / Shininess).
    mat_type_radio: Entity,
    /// The `radio_pbr_type` group (Material / Base / Metallic / Emissive /
    /// Normal).
    pbr_type_radio: Entity,
}

/// Spawn the matmedia tab strip, the material-type radio and the pbr-type radio,
/// returning their entities. The radios each carry their own `ShowWhen` so the
/// material-type radio hides in PBR mode and the pbr-type radio hides in Material
/// mode, mirroring the reference `updateVisibility`.
fn spawn_mode_selectors(
    commands: &mut Commands,
    page: Entity,
    tab_index: &mut i32,
) -> ModeSelectors {
    // matmedia mode switch (Material / PBR). The reference presents the material
    // type as a select box, but here it reads as a tab strip
    // ([`crate::ui_tab`]) so it matches the build floater's tabbed shell; the
    // strip's [`TabStrip::active`] replaces the combo's selection index the mode
    // systems read. Unlike the per-channel editors it carries no [`TexControl`]:
    // switching mode to *view* an object's Blinn-Phong vs PBR values is
    // navigation (like the aspect tabs above it), so it stays usable even on a
    // non-modifiable object whose editors are greyed.
    let matmedia_labels = [
        "build-tex-matmedia-material".to_owned(),
        "build-tex-matmedia-pbr".to_owned(),
    ];
    let matmedia_strip = spawn_tab_strip(
        commands,
        page,
        &TabSpec {
            element: "build-tex-matmedia",
            placement: TabPlacement::BlockStart,
            labels: &matmedia_labels,
            active: MatMedia::Material.radio_index(),
            tab_index: *tab_index,
            font_size: TOOL_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    *tab_index = tab_index.saturating_add(1);

    // material-type radio (Texture / Bumpiness / Shininess).
    let mat_type_row = spawn_row(commands, page, "build-tex-mattype-label");
    commands.entity(mat_type_row).insert(ShowWhen::MaterialAny);
    let mat_type_labels = [
        "build-tex-mattype-diffuse".to_owned(),
        "build-tex-mattype-normal".to_owned(),
        "build-tex-mattype-specular".to_owned(),
    ];
    let mat_type_radio = spawn_radio_group(
        commands,
        mat_type_row,
        &RadioSpec {
            element: "build-tex-mattype",
            labels: &mat_type_labels,
            active: MatChannel::Diffuse.radio_index(),
            tab_index: *tab_index,
            font_size: TOOL_FONT_SIZE,
            layout: RadioLayout::Row,
            translate_labels: true,
        },
    );
    commands.entity(mat_type_radio).insert(TexControl);
    *tab_index = tab_index.saturating_add(1);

    // pbr-type radio (Material / Base / Metallic / Emissive / Normal).
    let pbr_type_row = spawn_row(commands, page, "build-tex-pbrtype-label");
    commands.entity(pbr_type_row).insert(ShowWhen::PbrAny);
    let pbr_type_labels = [
        "build-tex-pbrtype-material".to_owned(),
        "build-tex-pbrtype-base".to_owned(),
        "build-tex-pbrtype-metallic".to_owned(),
        "build-tex-pbrtype-emissive".to_owned(),
        "build-tex-pbrtype-normal".to_owned(),
    ];
    let pbr_type_radio = spawn_radio_group(
        commands,
        pbr_type_row,
        &RadioSpec {
            element: "build-tex-pbrtype",
            labels: &pbr_type_labels,
            active: PbrChannel::Material.radio_index(),
            tab_index: *tab_index,
            font_size: TOOL_FONT_SIZE,
            layout: RadioLayout::Row,
            translate_labels: true,
        },
    );
    commands.entity(pbr_type_radio).insert(TexControl);
    *tab_index = tab_index.saturating_add(1);

    ModeSelectors {
        matmedia_strip,
        mat_type_radio,
        pbr_type_radio,
    }
}

/// Spawn a labelled numeric-field row tagged with the mode it shows in.
fn spawn_tex_field_row(
    commands: &mut Commands,
    page: Entity,
    label_key: &'static str,
    fields: &[TexField],
    show_when: ShowWhen,
    tab_index: &mut i32,
) {
    let row_entity = spawn_row(commands, page, label_key);
    commands.entity(row_entity).insert(show_when);
    for &field in fields {
        spawn_tex_field(commands, row_entity, field, tab_index);
    }
}

/// Tracks the object the material-mode auto-select last applied to, so the mode
/// is re-derived only when the selected object changes (the reference's
/// `prev_obj_id` guard) — never every frame, which would fight the user's manual
/// matmedia / channel picks.
#[derive(Resource, Debug, Default)]
struct MatModeSelected {
    /// The primary object the auto-select last ran for.
    last_object: Option<ScopedObjectId>,
}

/// Auto-select the material mode when the primary selection changes: choose the
/// PBR matmedia entry for a face that carries a GLTF render material, otherwise
/// the Material (Blinn-Phong) entry — the reference `LLPanelFace::updateUI`
/// behaviour where a PBR'd object opens in PBR mode and everything else in
/// Material mode. Only runs on an object change, so the user's later manual
/// matmedia pick stands.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection state, the UI handles, the per-object guard, the render-material + \
              hierarchy / scene queries the PBR test walks, and the combo the mode writes"
)]
fn auto_select_material_mode(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    ui: Option<Res<BuildTextureUi>>,
    mut selected: ResMut<MatModeSelected>,
    render_materials: Query<&crate::materials::ObjectRenderMaterials>,
    children: Query<&Children>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut strips: Query<&mut TabStrip>,
) {
    if !tool.active {
        selected.last_object = None;
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let Some(primary) = selection.primary() else {
        selected.last_object = None;
        return;
    };
    if selected.last_object == Some(primary.scoped) {
        return;
    }
    selected.last_object = Some(primary.scoped);
    let face_id = primary_face_index(&selection);
    let has_pbr = face_has_render_material(
        primary.entity,
        face_id,
        &render_materials,
        &children,
        &scene,
    );
    let want = if has_pbr {
        MatMedia::Pbr
    } else {
        MatMedia::Material
    };
    // Write the strip's active index directly; the tab widget's
    // `apply_programmatic_tab_selection` reconciles its highlight (`crate::ui_tab`
    // owns the click / arrow path, this is the programmatic one).
    if let Ok(mut strip) = strips.get_mut(ui.matmedia_strip) {
        strip.active = want.radio_index();
    }
}

/// The lowest selected face index of the primary selection (face 0 for a
/// whole-object selection) — the face the tab represents.
pub(crate) fn primary_face_index(selection: &SelectionSet) -> u8 {
    let index = selection
        .primary_faces()
        .and_then(|set| set.iter().map(|face| face.get()).min())
        .unwrap_or(0);
    u8::try_from(index).unwrap_or(0)
}

/// Whether face `face_id` of the object rooted at `root` carries a GLTF render
/// material — walked from the object's [`crate::materials::ObjectRenderMaterials`]
/// holder(s), stopping at any linkset-child scene object.
fn face_has_render_material(
    root: Entity,
    face_id: u8,
    render_materials: &Query<&crate::materials::ObjectRenderMaterials>,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
) -> bool {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if entity != root && scene.get(entity).is_ok() {
            continue;
        }
        if let Ok(holder) = render_materials.get(entity)
            && holder.faces.iter().any(|(face, _id)| *face == face_id)
        {
            return true;
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
    false
}

/// Mirror the three selector widgets' current values into [`MatModeState`] each
/// frame, writing only on a real change so the visibility system reacts once per
/// mode switch.
fn read_material_mode(
    ui: Option<Res<BuildTextureUi>>,
    strips: Query<&TabStrip>,
    radios: Query<&RadioSelection>,
    mut mode: ResMut<MatModeState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let matmedia = strips
        .get(ui.matmedia_strip)
        .map_or(MatMedia::Material, |strip| {
            MatMedia::from_radio_index(strip.active)
        });
    let mat_type = radios
        .get(ui.mat_type_radio)
        .map_or(MatChannel::Diffuse, |radio| {
            MatChannel::from_radio_index(radio.active)
        });
    let pbr_type = radios
        .get(ui.pbr_type_radio)
        .map_or(PbrChannel::Material, |radio| {
            PbrChannel::from_radio_index(radio.active)
        });
    mode.set_if_neq(MatModeState {
        matmedia,
        mat_type,
        pbr_type,
    });
}

/// Show or hide each mode-tagged Texture-tab control by setting its `Node.display`
/// from the current [`MatModeState`] — the reference `updateVisibility`. Runs only
/// when the mode changes.
fn apply_material_mode_visibility(
    mode: Res<MatModeState>,
    mut controls: Query<(&ShowWhen, &mut Node)>,
) {
    if !mode.is_changed() {
        return;
    }
    for (show_when, mut node) in &mut controls {
        let display = if show_when.matches(*mode) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// The Texture-tab widget handles the sync / reply systems address.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct BuildTextureUi {
    /// The Texture page container, walked to grey every label / value when the
    /// panel gates off.
    page: Entity,
    /// The tint-colour swatch (the colour picker's requester).
    color_swatch: Entity,
    /// The diffuse-texture swatch (the texture picker's requester).
    texture_swatch: Entity,
    /// The `matmedia` tab strip (Material / PBR) — its [`TabStrip::active`] is
    /// read for the mode and written by the per-object auto-select.
    pub(crate) matmedia_strip: Entity,
    /// The `radio_material_type` group (Texture / Bumpiness / Shininess).
    pub(crate) mat_type_radio: Entity,
    /// The `radio_pbr_type` group.
    pub(crate) pbr_type_radio: Entity,
}

/// Spawn a labelled row container and return it.
pub(crate) fn spawn_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ChildOf(parent),
        ))
        .id();
    spawn_row_label(commands, row_entity, label_key);
    row_entity
}

/// Spawn one numeric Texture-tab field.
fn spawn_tex_field(commands: &mut Commands, parent: Entity, field: TexField, tab_index: &mut i32) {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let entity = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: TOOL_FONT_SIZE,
            width_glyphs: TEX_FIELD_GLYPHS,
            tab_index: index,
            ..TextInputSpec::new(field.element(), field.input_kind())
        },
    );
    commands.entity(entity).insert((field, TexControl));
}

/// Spawn one Texture-tab toggle row (check glyph + label); returns the toggle
/// row so the caller can tag it (e.g. with a `ShowWhen`).
fn spawn_tex_toggle(
    commands: &mut Commands,
    parent: Entity,
    toggle: TexToggle,
    label_key: &'static str,
    tab_index: &mut i32,
) -> Entity {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let toggle_row = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(index),
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Pickable::default(),
            toggle,
            TexControl,
            Name::new(format!("build-tex:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(UNCHECKED_GLYPH),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        TexToggleGlyph(toggle),
        Pickable::IGNORE,
        ChildOf(toggle_row),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
        ClassList::new_with_classes([LABEL_CLASS]),
        Pickable::IGNORE,
        ChildOf(toggle_row),
    ));
    commands.entity(toggle_row).observe(handle_tex_toggle_press);
    toggle_row
}

/// Spawn one Texture-tab combo box for `cycle`, its options the cycle's value
/// labels, tagged [`TexCombo`] so the sync and change handler map it back.
fn spawn_tex_combo(commands: &mut Commands, parent: Entity, cycle: TexCycle, tab_index: &mut i32) {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let labels: Vec<String> = (0..cycle.count())
        .map(|value| cycle.label_key(value).to_owned())
        .collect();
    let combo = spawn_combo(
        commands,
        parent,
        &ComboSpec {
            element: cycle.element(),
            labels: &labels,
            active: 0,
            tab_index: index,
            font_size: TOOL_FONT_SIZE,
            translate_labels: true,
        },
    );
    commands.entity(combo).insert((TexCombo(cycle), TexControl));
}

/// Spawn the Align-planar-faces action button; returns the button entity so the
/// caller can tag it (e.g. with a `ShowWhen`).
fn spawn_align_button(commands: &mut Commands, parent: Entity, tab_index: &mut i32) -> Entity {
    let index = *tab_index;
    *tab_index = tab_index.saturating_add(1);
    let button = commands
        .spawn((
            bevy::ui_widgets::Button,
            bevy::input_focus::tab_navigation::TabIndex(index),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..row(Val::ZERO)
            },
            BorderColor::all(Color::srgba(0.4, 0.4, 0.45, 1.0)),
            BackgroundColor(Color::srgba(0.18, 0.18, 0.2, 1.0)),
            TexAlignButton,
            TexControl,
            Pickable::default(),
            Name::new("build-tex:align"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("build-tex-align"),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(handle_tex_align_press);
    button
}

/// Spawn one read-only info row.
fn spawn_info_row(commands: &mut Commands, parent: Entity, info: TexInfo, label_key: &'static str) {
    let info_row = spawn_row(commands, parent, label_key);
    commands.spawn((
        Text::default(),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        info,
        ChildOf(info_row),
    ));
}

/// The representative face the tab displays for the primary selection: the
/// object's decoded entry indexed by its lowest selected face (or face 0 for a
/// whole-object selection), plus a signature of the selected-face set so the
/// snapshot changes when the selection does.
pub(crate) fn representative_face(
    selection: &SelectionSet,
    objects: &ObjectState,
) -> Option<(ScopedObjectId, TextureFace, u64)> {
    let primary = selection.primary()?;
    let scoped = primary.scoped;
    let bytes = objects.texture_entry_of(&scoped)?;
    // The lowest selected face id (face 0 when the whole object is selected).
    let face_id = selection
        .primary_faces()
        .and_then(|set| set.iter().map(|face| face.get()).min())
        .unwrap_or(0);
    let entry = decode_texture_entry(bytes, usize::from(face_id).saturating_add(1));
    let face = *entry.face(usize::from(face_id))?;
    let signature = face_set_signature(selection.primary_faces());
    Some((scoped, face, signature))
}

/// A cheap signature of the selected-face set, so the snapshot changes when the
/// face selection does (order-independent — a sum / count of face ids).
fn face_set_signature(faces: Option<&std::collections::HashSet<PrimFaceId>>) -> u64 {
    match faces {
        None => u64::MAX,
        Some(set) => set
            .iter()
            .map(|face| u64::from(face.get()).wrapping_add(1))
            .fold(0_u64, |acc, value| {
                acc.wrapping_add(value.wrapping_mul(value))
            }),
    }
}

/// The widget queries the sync pass rewrites, bundled to stay inside Bevy's
/// system-parameter limit.
#[derive(bevy::ecs::system::SystemParam)]
struct TexWidgets<'w, 's> {
    /// The numeric fields.
    fields: Query<'w, 's, (Entity, &'static TexField, &'static mut EditableText)>,
    /// The toggle glyphs.
    glyphs: Query<'w, 's, (&'static TexToggleGlyph, &'static mut Text), Without<TexInfo>>,
    /// The bump / shiny / mapping combos, whose selection index the sync sets from
    /// the representative face (the combo widget reconciles the visible value).
    combos: Query<'w, 's, (&'static TexCombo, &'static mut ComboSelection)>,
    /// The info-line texts.
    infos: Query<'w, 's, (&'static TexInfo, &'static mut Text), Without<TexToggleGlyph>>,
    /// The tint-colour swatch value (set from the representative face).
    color_swatch: Query<'w, 's, &'static mut ColorSwatchValue>,
    /// The diffuse-texture swatch value (set from the representative face).
    texture_swatch: Query<'w, 's, &'static mut TextureSwatchValue>,
    /// Every interactive control, gated (pointer-disabled) while nothing is
    /// selected.
    controls: Query<'w, 's, Entity, With<TexControl>>,
    /// The child links, walked from the page to grey every label / value.
    children: Query<'w, 's, &'static Children>,
    /// The skin class lists on the tab's texts, greyed while nothing is selected.
    class_lists: Query<'w, 's, &'static mut ClassList>,
    /// Commands, to toggle the controls' `InteractionDisabled` / `Pickable`.
    commands: Commands<'w, 's>,
}

/// Grey (or un-grey) every label / value text under the Texture page by toggling
/// [`crate::edit_params::DISABLED_CLASS`] on each descendant that carries a build
/// label / value class.
/// The reference greys the whole panel while nothing is selected; a single walk
/// from the page covers every control's text without a marker on each.
fn grey_texture_tab(
    page: Entity,
    disabled: bool,
    children: &Query<&Children>,
    class_lists: &mut Query<&mut ClassList>,
) {
    let mut stack = vec![page];
    while let Some(entity) = stack.pop() {
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
        if let Ok(mut class_list) = class_lists.get_mut(entity)
            && (class_list.contains(LABEL_CLASS) || class_list.contains(VALUE_CLASS))
        {
            set_disabled_class(&mut class_list, disabled);
        }
    }
}

/// Populate the Texture-tab widgets from the primary selection's representative
/// face — skipping whichever field the user is editing, and only when the shown
/// snapshot changes (so a just-committed edit is not clobbered before the
/// simulator's confirming update lands).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection / object state, the snapshot guard, the focus, the widget queries, the \
              translator, and the text-layout contexts a programmatic field rewrite needs"
)]
fn sync_texture_widgets(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    ui: Option<Res<BuildTextureUi>>,
    mut snapshot: ResMut<TexShownSnapshot>,
    focus: Res<InputFocus>,
    mut widgets: TexWidgets,
    translator: Translator,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
) {
    if !tool.active {
        return;
    }
    let current = representative_face(&selection, &objects);
    // Gate the whole panel on selection **and** modify permission — a texture /
    // colour edit is a modify, so the reference greys and disables every control
    // while nothing is selected or the primary is not modifiable (values still
    // show). Applied only on the transition so a stable state does not re-touch
    // every control each frame.
    let modify_ok = selection
        .primary()
        .is_some_and(|node| objects.agent_can_modify(&node.scoped));
    let enabled = current.is_some() && modify_ok;
    if snapshot.enabled != Some(enabled) {
        snapshot.enabled = Some(enabled);
        for control in &widgets.controls {
            if enabled {
                widgets
                    .commands
                    .entity(control)
                    .remove::<bevy::ui::InteractionDisabled>()
                    .insert(Pickable::default());
            } else {
                widgets
                    .commands
                    .entity(control)
                    .insert((bevy::ui::InteractionDisabled, Pickable::IGNORE));
            }
        }
        if let Some(ui) = ui.as_ref() {
            grey_texture_tab(
                ui.page,
                !enabled,
                &widgets.children,
                &mut widgets.class_lists,
            );
        }
    }
    // Only rewrite when the snapshot changes; the focused field is always skipped.
    if snapshot.shown == current {
        return;
    }
    snapshot.shown = current;
    let Some((_scoped, face, _signature)) = current else {
        // Deselected: clear the Texture tab to its neutral state so it never
        // shows a stale prim's details.
        for (entity, _field, mut editor) in &mut widgets.fields {
            if focus.get() == Some(entity) {
                continue;
            }
            if !editor.value().to_string().is_empty() {
                set_editor_text(&mut editor, "", &mut font_cx, &mut layout_cx);
            }
        }
        for (_glyph, mut text) in &mut widgets.glyphs {
            if text.0 != UNCHECKED_GLYPH {
                UNCHECKED_GLYPH.clone_into(&mut text.0);
            }
        }
        for (_combo, mut combo_selection) in &mut widgets.combos {
            if combo_selection.active != 0 {
                combo_selection.active = 0;
            }
        }
        let none = translator.get("build-tex-selection-none");
        for (_info, mut text) in &mut widgets.infos {
            if text.0 != none {
                text.0.clone_from(&none);
            }
        }
        if let Some(ui) = ui {
            if let Ok(mut swatch) = widgets.color_swatch.get_mut(ui.color_swatch)
                && swatch.0 != Color::WHITE
            {
                swatch.0 = Color::WHITE;
            }
            let nil = TextureKey::from(Uuid::nil());
            if let Ok(mut swatch) = widgets.texture_swatch.get_mut(ui.texture_swatch)
                && swatch.0 != nil
            {
                swatch.0 = nil;
            }
        }
        return;
    };

    for (entity, field, mut editor) in &mut widgets.fields {
        if focus.get() == Some(entity) {
            continue;
        }
        let want = format_tex_value(*field, field.display_value(&face));
        if editor.value().to_string() != want {
            set_editor_text(&mut editor, &want, &mut font_cx, &mut layout_cx);
        }
    }
    for (glyph, mut text) in &mut widgets.glyphs {
        let want = if glyph.0.get(&face) {
            CHECKED_GLYPH
        } else {
            UNCHECKED_GLYPH
        };
        if text.0 != want {
            want.clone_into(&mut text.0);
        }
    }
    for (combo, mut selection) in &mut widgets.combos {
        let want = combo.0.index(&face);
        if selection.active != want {
            selection.active = want;
        }
    }
    for (info, mut text) in &mut widgets.infos {
        let want = match info {
            TexInfo::Faces => faces_summary(&selection, &translator),
        };
        if text.0 != want {
            text.0 = want;
        }
    }
    // The colour / texture swatches follow the representative face.
    if let Some(ui) = ui {
        if let Ok(mut swatch) = widgets.color_swatch.get_mut(ui.color_swatch) {
            let want = Color::srgb_u8(
                byte_at(face.color, 0),
                byte_at(face.color, 1),
                byte_at(face.color, 2),
            );
            if swatch.0 != want {
                swatch.0 = want;
            }
        }
        if let Ok(mut swatch) = widgets.texture_swatch.get_mut(ui.texture_swatch)
            && swatch.0 != face.texture_id
        {
            swatch.0 = face.texture_id;
        }
    }
}

/// The selected-face summary line: "all faces" for a whole-object selection, else
/// the chosen-face count.
fn faces_summary(selection: &SelectionSet, translator: &Translator) -> String {
    match selection.primary_faces() {
        None => translator.get("build-tex-faces-all"),
        Some(set) => {
            let count = i64::try_from(set.len()).unwrap_or(i64::MAX);
            translator.format(
                "build-tex-faces-count",
                &TransArgs::new().int("count", count),
            )
        }
    }
}

/// Format one field's value the way the tab displays it (integers for
/// colour / transparency, three decimals otherwise).
fn format_tex_value(field: TexField, value: f32) -> String {
    match field.input_kind() {
        TextInputKind::Integer => round_to_i64(value).to_string(),
        _float => format!("{value:.3}"),
    }
}

/// Commit numeric Texture-tab edits: on `Enter` in a focused field or when focus
/// leaves one, apply the one attribute to the selected faces and send the
/// modified entry.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection / object state, the focus and its blur tracker, the field query, the \
              keyboard, and the outgoing command writer"
)]
fn commit_texture_fields(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    focus: Res<InputFocus>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus_track: ResMut<TexFieldFocus>,
    fields: Query<(Entity, &TexField, &EditableText)>,
    prim_faces: PrimFaceLookup,
    mut commands: MessageWriter<SlCommand>,
) {
    if !tool.active {
        focus_track.last = None;
        return;
    }
    let focused = focus.get().filter(|entity| fields.contains(*entity));
    let enter =
        keyboard.just_pressed(KeyCode::Enter) || keyboard.just_pressed(KeyCode::NumpadEnter);
    let commit = if enter {
        focused
    } else if focus_track.last != focused {
        focus_track.last.filter(|entity| fields.contains(*entity))
    } else {
        None
    };
    focus_track.last = focused;
    let Some(entity) = commit else {
        return;
    };
    let Ok((_entity, &field, editor)) = fields.get(entity) else {
        return;
    };
    let Some(value) = parse_tex_value(field.input_kind(), &editor.value().to_string()) else {
        return;
    };
    apply_to_selection(&selection, &objects, &prim_faces, &mut commands, |face| {
        field.apply(face, value);
    });
}

/// Flip a Texture-tab toggle on the selected faces.
fn handle_tex_toggle_press(
    press: On<Pointer<Press>>,
    toggles: Query<&TexToggle>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    prim_faces: PrimFaceLookup,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(&toggle) = toggles.get(press.entity) else {
        return;
    };
    // Read the current value off the primary's representative face, then flip it
    // for every selected face.
    let current = representative_face(&selection, &objects)
        .is_some_and(|(_scoped, face, _sig)| toggle.get(&face));
    apply_to_selection(&selection, &objects, &prim_faces, &mut commands, |face| {
        toggle.set(face, !current);
    });
}

/// Apply a Texture-tab combo's user pick to the selected faces: map the changed
/// combo back to its packed attribute ([`TexCombo`]) and set the chosen value on
/// every selected face. Reads [`ComboChanged`] (a **user** pick only), so the
/// display sync setting a combo's index never loops back into a send.
fn apply_tex_combo_changes(
    mut changes: MessageReader<ComboChanged>,
    combos: Query<&TexCombo>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    prim_faces: PrimFaceLookup,
    mut commands: MessageWriter<SlCommand>,
) {
    for change in changes.read() {
        let Ok(combo) = combos.get(change.combo) else {
            continue;
        };
        let cycle = combo.0;
        let value = change.active;
        apply_to_selection(&selection, &objects, &prim_faces, &mut commands, |face| {
            cycle.set(face, value);
        });
    }
}

/// Apply a colour from the colour picker to the selected faces' tint (RGB only,
/// the reference's `sendColor` — transparency is its own field). A **live**
/// (non-final) update previews the colour on the faces' materials in place — no
/// wire send, so a drag does not flood the simulator — and only the **committed**
/// (OK) colour is sent as an `ObjectImage`; a Cancel's revert preview restores
/// the opened-on colour. Ignores a reply for any other requester.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the picker replies, \
              the UI handles, the selection / object state and the commit's face lookup, plus the \
              hierarchy / face-material queries and the material store the live preview mutates"
)]
fn apply_color_picked(
    mut picks: MessageReader<ColorPicked>,
    ui: Option<Res<BuildTextureUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    prim_faces: PrimFaceLookup,
    children: Query<&Children>,
    face_materials: Query<(&PrimFaceEntity, &MeshMaterial3d<FaceMaterial>)>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.requester != ui.color_swatch {
            continue;
        }
        if pick.final_pick {
            // Commit: send the whole modified entry; the sim echo re-tessellates
            // the faces with the final colour.
            let srgba = pick.color.to_srgba();
            let rgb = [
                round_to_byte(srgba.red * 255.0),
                round_to_byte(srgba.green * 255.0),
                round_to_byte(srgba.blue * 255.0),
            ];
            apply_to_selection(&selection, &objects, &prim_faces, &mut commands, |face| {
                for (index, byte) in rgb.iter().enumerate() {
                    set_byte(&mut face.color, index, *byte);
                }
            });
        } else {
            // Live preview: tint the selected faces' materials in place, keeping
            // each face's current alpha (transparency).
            preview_face_tint(
                &selection,
                pick.color,
                &children,
                &scene,
                &face_materials,
                &mut materials,
            );
        }
    }
}

/// Tint the selected faces' materials to `color` in place (keeping each face's
/// current alpha) — the colour picker's live preview, with no wire send. Walks
/// each selected object's own faces (stopping at linkset-child objects) and, for
/// a per-face selection, only the chosen faces.
fn preview_face_tint(
    selection: &SelectionSet,
    color: Color,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
    face_materials: &Query<(&PrimFaceEntity, &MeshMaterial3d<FaceMaterial>)>,
    materials: &mut Assets<FaceMaterial>,
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
                && let Some(mut standard) = materials.get_mut(&material.0)
            {
                let alpha = standard.base.base_color.alpha();
                standard.base.base_color = color.with_alpha(alpha);
            }
            if let Ok(list) = children.get(entity) {
                for child in list.iter() {
                    stack.push(child);
                }
            }
        }
    }
}

/// The live texture-preview target: the texture the picker last previewed on the
/// selected faces (and whether it has been painted onto their materials yet). A
/// non-final [`TexturePicked`] sets it; the [`drive_texture_preview`] driver
/// paints it once the image decodes; a commit clears it (the sim echo rebuilds).
#[derive(Resource, Debug, Default)]
struct TexturePreview {
    /// The texture to show live, or `None` when not previewing.
    texture: Option<TextureKey>,
    /// Whether the current [`texture`](Self::texture) has been painted on.
    applied: bool,
    /// The object entity being previewed, so the preview can be reverted if the
    /// selection moves off it (a deselect while the picker is open).
    object: Option<Entity>,
}

/// Apply a texture chosen in the texture picker to the selected faces' diffuse.
/// A **live** (non-final) pick previews the texture on the faces' materials in
/// place — no wire send — and only the **committed** (OK) texture is sent as an
/// `ObjectImage` (the reference's `sendTexture` / `selectionSetImage`); a
/// Cancel's revert preview restores the opened-on texture. Ignores a reply for
/// any other requester.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the picker replies, \
              the UI handles, the selection / object state and the commit's face lookup, the \
              live-preview state, the texture store, and the outgoing command writer"
)]
fn apply_texture_picked(
    mut picks: MessageReader<TexturePicked>,
    ui: Option<Res<BuildTextureUi>>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    prim_faces: PrimFaceLookup,
    mut preview: ResMut<TexturePreview>,
    mut textures: ResMut<crate::textures::TextureManager>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.requester != ui.texture_swatch {
            continue;
        }
        if pick.final_pick {
            // Commit: send the whole modified entry; the sim echo re-tessellates
            // the faces with the final texture, so stop previewing.
            let texture = pick.texture;
            apply_to_selection(&selection, &objects, &prim_faces, &mut commands, |face| {
                face.texture_id = texture;
            });
            preview.texture = None;
            preview.object = None;
        } else {
            // Live preview: paint the texture on the selected faces once it
            // decodes ([`drive_texture_preview`]); request the decode now.
            textures.request_boosted(pick.texture, AVATAR_BOOST_PRIORITY);
            preview.texture = Some(pick.texture);
            preview.object = selection.primary().map(|node| node.entity);
        }
        preview.applied = false;
    }
}

/// Paint the live texture preview onto the selected faces' materials once its
/// image decodes: an absent texture clears the material's texture (flat tint), a
/// decoded one swaps it in, and a not-yet-decoded one waits. Runs only while a
/// preview is active and unpainted, so it is a no-op once settled.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the preview state, \
              the selection, the texture decode store and the image assets it uploads into, the \
              hierarchy / face-material queries and the material store the preview paints into"
)]
fn drive_texture_preview(
    mut preview: ResMut<TexturePreview>,
    selection: Res<SelectionSet>,
    store: Res<crate::world_api::DecodedTextures>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    face_materials: Query<(&PrimFaceEntity, &MeshMaterial3d<FaceMaterial>)>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let Some(texture) = preview.texture else {
        return;
    };
    if preview.applied {
        return;
    }
    let image = match store.diffuse_image(texture, &mut images) {
        crate::textures::DiffuseImage::Absent => None,
        crate::textures::DiffuseImage::Ready(handle) => Some(handle),
        // Not decoded yet; try again next frame (the request was already sent).
        crate::textures::DiffuseImage::Pending => return,
    };
    preview_face_texture(
        &selection,
        image,
        &children,
        &scene,
        &face_materials,
        &mut materials,
    );
    preview.applied = true;
}

/// Revert a live texture preview when the selection moves off the previewed
/// object (a deselect, or selecting a different object, while the picker is still
/// open): restore that object's faces' `base_color_texture` to their real
/// (`FaceTextureDebug`) texture, since the entry was never committed. A no-op
/// while the previewed object is still primary, or when nothing is previewing.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the preview state, \
              the selection, the texture decode store and image assets, the hierarchy / face \
              queries, and the material store the revert repaints"
)]
fn revert_texture_preview_on_deselect(
    mut preview: ResMut<TexturePreview>,
    selection: Res<SelectionSet>,
    store: Res<crate::world_api::DecodedTextures>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    faces: Query<(&FaceTextureDebug, &MeshMaterial3d<FaceMaterial>)>,
    scene: Query<(), With<crate::objects::SceneObject>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let Some(object) = preview.object else {
        return;
    };
    if preview.texture.is_none() {
        return;
    }
    if selection.primary().map(|node| node.entity) == Some(object) {
        // Still editing this object — keep previewing.
        return;
    }
    let mut stack = vec![object];
    while let Some(entity) = stack.pop() {
        if entity != object && scene.get(entity).is_ok() {
            continue;
        }
        if let Ok((FaceTextureDebug(face), material)) = faces.get(entity) {
            let image = match store.diffuse_image(face.texture_id, &mut images) {
                crate::textures::DiffuseImage::Absent => None,
                crate::textures::DiffuseImage::Ready(handle) => Some(handle),
                // The real texture is not decoded (unusual — it was rendering);
                // leave it, a later re-tessellation restores it.
                crate::textures::DiffuseImage::Pending => continue,
            };
            if let Some(mut standard) = materials.get_mut(&material.0) {
                standard.base.base_color_texture = image;
            }
        }
        if let Ok(list) = children.get(entity) {
            for child in list.iter() {
                stack.push(child);
            }
        }
    }
    preview.texture = None;
    preview.object = None;
    preview.applied = false;
}

/// Swap the selected faces' materials' `base_color_texture` to `image` (or clear
/// it when `None`) in place — the texture picker's live preview, keeping each
/// face's tint. Walks each selected object's own faces (stopping at linkset-child
/// objects) and, for a per-face selection, only the chosen faces.
fn preview_face_texture(
    selection: &SelectionSet,
    image: Option<Handle<Image>>,
    children: &Query<&Children>,
    scene: &Query<(), With<crate::objects::SceneObject>>,
    face_materials: &Query<(&PrimFaceEntity, &MeshMaterial3d<FaceMaterial>)>,
    materials: &mut Assets<FaceMaterial>,
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
                && let Some(mut standard) = materials.get_mut(&material.0)
            {
                standard.base.base_color_texture.clone_from(&image);
            }
            if let Ok(list) = children.get(entity) {
                for child in list.iter() {
                    stack.push(child);
                }
            }
        }
    }
}

/// The per-face lookup a commit uses to rebuild an object's texture entry from
/// its **rendered** faces.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct PrimFaceLookup<'w, 's> {
    /// Parent → children, to walk an object's own face entities.
    children: Query<'w, 's, &'static Children>,
    /// Each face's Linden index and its current decoded [`TextureFace`] — the
    /// ground truth of what the face renders (correct texture id, tint, repeats,
    /// …), so an edit preserves every attribute it does not touch and every
    /// unedited face keeps its value.
    faces: Query<'w, 's, (&'static PrimFaceEntity, &'static FaceTextureDebug)>,
    /// Scene identities, to stop the walk at a linkset child object.
    scene: Query<'w, 's, (), With<crate::objects::SceneObject>>,
}

impl PrimFaceLookup<'_, '_> {
    /// The object's current per-face [`TextureFace`]s, indexed by Linden face id
    /// (`0..=max`), read from what each face actually renders
    /// ([`FaceTextureDebug`]) rather than by re-decoding the stored blob — so a
    /// re-sent entry carries every face's real value, not a face-count-dependent
    /// approximation. A gap (a face with no rendered geometry) is filled with the
    /// neutral default. Empty when the object has no rendered faces. Stops at any
    /// descendant that is its own scene object (a linkset child).
    pub(crate) fn current_faces(&self, root: Entity) -> Vec<TextureFace> {
        let mut by_id: Vec<(u16, TextureFace)> = Vec::new();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if entity != root && self.scene.get(entity).is_ok() {
                continue;
            }
            if let Ok((marker, FaceTextureDebug(face))) = self.faces.get(entity) {
                by_id.push((marker.face_id.get(), *face));
            }
            if let Ok(list) = self.children.get(entity) {
                for child in list.iter() {
                    stack.push(child);
                }
            }
        }
        let Some(max) = by_id.iter().map(|(id, _)| *id).max() else {
            return Vec::new();
        };
        let count = usize::from(max).saturating_add(1);
        let default = TextureFace::new(TextureKey::from(Uuid::nil()));
        let mut faces = vec![default; count];
        for (id, face) in by_id {
            if let Some(slot) = faces.get_mut(usize::from(id)) {
                *slot = face;
            }
        }
        faces
    }
}

/// Apply `edit` to every selected face of every selected object and send each
/// object's modified `TextureEntry` as an `ObjectImage` — the shared spine of
/// the field / toggle / cycle / colour / texture commits. The entry is rebuilt
/// from the object's **rendered** per-face values ([`PrimFaceLookup::current_faces`]),
/// so `edit` — which mutates one attribute of a face — leaves every other
/// attribute and every unedited face exactly as it was (a colour edit keeps the
/// face's texture, an unedited face keeps its tint).
fn apply_to_selection(
    selection: &SelectionSet,
    objects: &ObjectState,
    prim_faces: &PrimFaceLookup,
    commands: &mut MessageWriter<SlCommand>,
    edit: impl Fn(&mut TextureFace),
) {
    for node in selection.iter() {
        let scoped = node.scoped();
        let faces = prim_faces.current_faces(node.entity);
        if faces.is_empty() {
            continue;
        }
        let mut entry = TextureEntry { faces };
        let face_count = entry.faces.len();
        let indices = node_face_indices(node, face_count);
        log_face_edit(scoped, &indices, &entry, "before");
        let touched = apply_edit_to_faces(&mut entry, indices, &edit);
        if !touched {
            continue;
        }
        log_face_edit(scoped, &[], &entry, "sent");
        commands.write(SlCommand(Command::SetObjectImage {
            local_id: scoped,
            media_url: objects.media_url_of(&scoped),
            texture_entry: entry,
        }));
    }
}

/// Log one commit's entry at [`TEXTURE_EDIT_LOG_TARGET`]: the faces the lookup
/// found, the indices the edit hits (`stage = "before"`) and the per-face tint /
/// texture the commit puts on the wire (`stage = "sent"`).
fn log_face_edit(
    scoped: ScopedObjectId,
    indices: &[usize],
    entry: &TextureEntry,
    stage: &'static str,
) {
    if !bevy::log::tracing::enabled!(target: TEXTURE_EDIT_LOG_TARGET, bevy::log::Level::DEBUG) {
        return;
    }
    let faces: Vec<String> = entry
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| {
            format!(
                "{index}:rgba{:?} tex={} glow={:.2} mat={}",
                face.color,
                face.texture_id.uuid().simple(),
                face.glow,
                face.material_id.is_some_and(|id| !id.is_nil()),
            )
        })
        .collect();
    debug!(
        target: TEXTURE_EDIT_LOG_TARGET,
        "{stage} object {scoped}: {} rendered faces, editing {indices:?} — {}",
        entry.faces.len(),
        faces.join(" | "),
    );
}

/// The Linden face indices a selection node's edit hits: its chosen faces, or
/// every face (0..`face_count`) when the whole object is selected.
pub(crate) fn node_face_indices(
    node: &crate::world_api::SelectedNode,
    face_count: usize,
) -> Vec<usize> {
    match &node.faces {
        Some(set) => set
            .iter()
            .map(|face| usize::from(face.get()))
            .filter(|index| *index < face_count)
            .collect(),
        None => (0..face_count).collect(),
    }
}

/// Apply `edit` to each of `indices` in `entry`; returns whether any face was
/// touched.
fn apply_edit_to_faces(
    entry: &mut TextureEntry,
    indices: Vec<usize>,
    edit: &impl Fn(&mut TextureFace),
) -> bool {
    let mut touched = false;
    for index in indices {
        if let Some(face) = entry.faces.get_mut(index) {
            edit(face);
            touched = true;
        }
    }
    touched
}

/// The Align-planar-faces action (implemented in [`crate::edit_texture_align`]).
fn handle_tex_align_press(
    press: On<Pointer<Press>>,
    _buttons: Query<&TexAlignButton>,
    selection: Res<SelectionSet>,
    objects: Res<ObjectState>,
    prim_faces: PrimFaceLookup,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    crate::edit_texture_align::align_planar_faces(&selection, &objects, &prim_faces, &mut commands);
}

// ---------------------------------------------------------------------------
// Small numeric helpers (kept free of the disallowed `as` / indexing lints).
// ---------------------------------------------------------------------------

/// The byte at `index` of an RGBA quad (0 outside 0..4).
fn byte_at(color: [u8; 4], index: usize) -> u8 {
    color.get(index).copied().unwrap_or(0)
}

/// Set the byte at `index` of an RGBA quad (a no-op outside 0..4).
fn set_byte(color: &mut [u8; 4], index: usize, value: u8) {
    if let Some(slot) = color.get_mut(index) {
        *slot = value;
    }
}

/// Round a display value to a colour byte, clamped to 0..=255.
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

/// Round a display value to the integer a colour / transparency field shows.
const fn round_to_i64(value: f32) -> i64 {
    let rounded = value.round();
    if rounded.is_finite() {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "a bounded colour / transparency entry, finite and small"
        )]
        let int = rounded as i64;
        int
    } else {
        0
    }
}

/// Parse one committed field value.
pub(crate) fn parse_tex_value(kind: TextInputKind, text: &str) -> Option<f32> {
    match kind.parse(text.trim())? {
        TextInputValue::Float(value) => {
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "f64 → f32 at the field boundary; the value is a bounded surface entry"
            )]
            let value = value as f32;
            value.is_finite().then_some(value)
        }
        TextInputValue::Integer(value) => {
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "a small bounded colour / transparency integer widens exactly to f32"
            )]
            let value = value as f32;
            Some(value)
        }
        _other => None,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{TextureFace, TextureKey, Uuid};

    use super::{TexCycle, TexField, TexToggle, round_to_byte};

    /// A neutral face for tests.
    fn face() -> TextureFace {
        TextureFace::new(TextureKey::from(Uuid::nil()))
    }

    /// Transparency round-trips through the alpha byte (0 % ⇒ opaque, 100 % ⇒
    /// clear).
    #[test]
    fn transparency_applies() {
        let mut f = face();
        TexField::Transparency.apply(&mut f, 25.0);
        // 25 % transparent ⇒ alpha 75 % ⇒ round(0.75 * 255) = 191.
        assert_eq!(f.color.get(3).copied(), Some(191));
        assert!((TexField::Transparency.display_value(&f) - 25.0).abs() < 1.0);
    }

    /// Rotation is entered in degrees and stored in radians.
    #[test]
    fn rotation_is_degrees_over_radians() {
        let mut f = face();
        TexField::Rotation.apply(&mut f, 90.0);
        assert!((f.rotation - core::f32::consts::FRAC_PI_2).abs() < 1.0e-4);
        assert!((TexField::Rotation.display_value(&f) - 90.0).abs() < 1.0e-3);
    }

    /// The planar-texgen ×2 display quirk: a planar face shows twice its stored
    /// repeats, and an entered value is halved back on the way in.
    #[test]
    fn planar_repeats_double_on_display() {
        let mut f = face();
        TexCycle::TexGen.set(&mut f, 1);
        assert!(f.is_planar_texgen());
        TexField::RepeatU.apply(&mut f, 4.0);
        assert!((f.scale_s - 2.0).abs() < 1.0e-6);
        assert!((TexField::RepeatU.display_value(&f) - 4.0).abs() < 1.0e-6);
    }

    /// Bump / shiny / full-bright pack into the one byte without disturbing one
    /// another.
    #[test]
    fn packed_byte_fields_are_independent() {
        let mut f = face();
        TexCycle::Bump.set(&mut f, 5);
        TexCycle::Shininess.set(&mut f, 2);
        TexToggle::Fullbright.set(&mut f, true);
        assert_eq!(f.bumpmap(), 5);
        assert_eq!(f.shininess(), 2);
        assert!(f.fullbright());
        // Clearing full-bright leaves bump / shiny intact.
        TexToggle::Fullbright.set(&mut f, false);
        assert!(!f.fullbright());
        assert_eq!(f.bumpmap(), 5);
        assert_eq!(f.shininess(), 2);
    }

    /// The colour byte rounder clamps and rounds.
    #[test]
    fn byte_rounder_clamps() {
        assert_eq!(round_to_byte(-5.0), 0);
        assert_eq!(round_to_byte(300.0), 255);
        assert_eq!(round_to_byte(127.6), 128);
    }

    /// The whole commit path for a whole-object edit, over a box's six rendered
    /// faces: the per-face lookup, the face-index expansion, the edit, and the
    /// wire round-trip.
    ///
    /// The guard is [[viewer-transparency-all-faces-skips-top]] — a report that a
    /// Transparency edit with **no** individual face selected leaves a cube's
    /// **top** face alone. A box's top cap is Linden face **0** (the profile
    /// emits `add_cap(PATH_BEGIN)` first, `sl-prim/src/profile.rs`), which is
    /// exactly the index a range-end off-by-one cannot miss but a
    /// `saturating_sub` / "skip the first" slip can — so it is pinned here.
    #[test]
    fn a_whole_object_edit_reaches_every_face_including_the_top_cap() {
        use bevy::app::App;
        use bevy::ecs::message::Messages;
        use bevy::prelude::{ChildOf, MessageWriter, Res, Update};
        use sl_client_bevy::{
            Command, PrimFaceId, SlCommand, TextureEntry, decode_texture_entry,
            encode_texture_entry,
        };

        use crate::objects::{FaceTextureDebug, PrimFaceEntity};
        use crate::world_api::{ObjectState, SelectionSet};

        use super::{PrimFaceLookup, apply_to_selection};

        /// A box's six faces: the top cap (0), the four sides (1..=4) and the
        /// bottom cap (5) — all opaque, as a freshly rezzed prim's are.
        const FACE_COUNT: u16 = 6;

        /// The commit under test: the Transparency field's whole-object apply.
        fn commit(
            selection: Res<SelectionSet>,
            objects: Res<ObjectState>,
            prim_faces: PrimFaceLookup,
            mut commands: MessageWriter<SlCommand>,
        ) {
            apply_to_selection(&selection, &objects, &prim_faces, &mut commands, |face| {
                TexField::Transparency.apply(face, 50.0);
            });
        }

        let mut app = App::new();
        app.add_message::<SlCommand>()
            .init_resource::<ObjectState>()
            .add_systems(Update, commit);

        // The scene shape the world layer builds: the object entity, its
        // geometry holder child, and one face entity per rendered face.
        let object = app.world_mut().spawn_empty().id();
        let geometry = app.world_mut().spawn(ChildOf(object)).id();
        for face_id in 0..FACE_COUNT {
            let _face = app.world_mut().spawn((
                PrimFaceEntity {
                    face_id: PrimFaceId::new(face_id),
                },
                FaceTextureDebug(face()),
                ChildOf(geometry),
            ));
        }
        let mut selection = SelectionSet::default();
        selection.insert(scoped(), full(), object);
        assert!(
            selection.primary().is_some_and(|node| node.faces.is_none()),
            "an ordinary object selection means every face"
        );
        app.insert_resource(selection);

        app.update();

        let sent: Vec<TextureEntry> = {
            let messages = app.world().resource::<Messages<SlCommand>>();
            let mut cursor = messages.get_cursor();
            cursor
                .read(messages)
                .filter_map(|command| match &command.0 {
                    Command::SetObjectImage { texture_entry, .. } => Some(texture_entry.clone()),
                    _other => None,
                })
                .collect()
        };
        assert_eq!(sent.len(), 1, "one ObjectImage for the one selected object");
        for entry in &sent {
            assert_eq!(
                entry.faces.len(),
                usize::from(FACE_COUNT),
                "the entry covers every rendered face"
            );
            // 50 % transparent ⇒ alpha round(0.5 * 255) = 128, on every face —
            // the top cap (0) and the bottom cap (5) as much as the sides.
            for (index, face) in entry.faces.iter().enumerate() {
                assert_eq!(
                    face.color.get(3).copied(),
                    Some(128),
                    "face {index} took the transparency edit"
                );
            }
            // …and survives the wire packing the simulator decodes: the
            // run-length form writes the *last* face as the field default, so a
            // face silently dropped from the packing would decode as opaque.
            let decoded =
                decode_texture_entry(&encode_texture_entry(entry), usize::from(FACE_COUNT));
            for (index, face) in decoded.faces.iter().enumerate() {
                assert_eq!(
                    face.color.get(3).copied(),
                    Some(128),
                    "face {index} kept its transparency across the wire"
                );
            }
        }
    }

    /// A scoped id for the selection tests.
    fn scoped() -> sl_client_bevy::ScopedObjectId {
        sl_client_bevy::ScopedObjectId {
            circuit: sl_client_bevy::CircuitId::new(1),
            id: sl_client_bevy::RegionLocalObjectId(1),
        }
    }

    /// A full object key for the selection tests.
    fn full() -> sl_client_bevy::ObjectKey {
        sl_client_bevy::ObjectKey::from(Uuid::from_u128(1))
    }
}
