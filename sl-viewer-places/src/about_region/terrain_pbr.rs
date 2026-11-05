//! The Terrain tab's **PBR-material variant** and the checks its **Apply**
//! makes first (`viewer-region-options-terrain`).
//!
//! A region's four terrain detail slots hold either four **textures** (the
//! classic splat) or four **PBR materials**; the same `texturedetail` estate
//! message carries either. Nothing on the wire says which, so the reference
//! (`LLTerrainMaterials::getMaterialType`) asks its stores: if any slot decoded
//! as a texture the region is on textures; else if any decoded as a material it
//! is on materials; else it cannot tell, shows **both** sets of swatches seeded
//! with the same ids, and starts on textures. This does the same over the
//! viewer's [`DecodedTextures`] and [`MaterialManager`] — each id is asked for
//! as both, and the wrong kind simply fails to decode.
//!
//! The "PBR Metallic Roughness" checkbox switches which set the swatches and
//! **Apply** use (the other is kept, as the reference keeps `mLastSetTextures`
//! / `mLastSetMaterials`), and shows the **Transforms** sub-tab: a scale,
//! rotation and offset per material, written over the `ModifyRegion`
//! capability after the estate messages and read back from it on open.
//!
//! # What Apply checks
//!
//! Before anything is sent, as `LLPanelRegionTerrainInfo::sendUpdate` does:
//! on textures, every texture must be RGB or opaque RGBA and no larger than
//! [`MAX_TERRAIN_TEXTURE_SIZE`] (`validateTextureSizes`); on materials, every
//! material must have loaded, be single-sided and not alpha-blended
//! (`validateMaterials`). Each failure is the catalogue's own notice, naming the
//! slot, and nothing goes out.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::ValueChange;
use sl_client_bevy::{
    AssetKey, CAP_MODIFY_REGION, Command, Llsd, RegionTerrainComposition, RegionTerrainUpdate,
    SlCapabilities, SlCommand, SlEvent, SlSessionEvent, TerrainMaterialOverrides, TextureKey, Uuid,
};
use sl_material::GltfAlphaMode;
use sl_viewer_pickers::ui_texture_picker::{MaterialSwatchValue, spawn_material_swatch};
use sl_viewer_ui_core::semantic::{Role, Semantic};
use sl_viewer_world_api::{AVATAR_BOOST_PRIORITY, BoostTexture, DecodedTextures};
use sl_viewer_world_objects::material_preview::MaterialPreview;
use sl_viewer_world_objects::materials::{MaterialManager, MaterialStatus};
use sl_viewer_world_objects::textures::TextureManager;
use std::collections::HashMap;

use super::{
    AboutRegionDirty, AboutRegionState, AboutRegionUi, DIM_LABEL_COLOR, EditGate, SpinStep,
    TerrainHandles, set_check_marker, spawn_key_label, spawn_row, spawn_terrain_field,
};
use crate::edit_fields::seed_one_field;
use crate::floater::{Floater, FloaterHost, host_floater};
use crate::intents::TexturePicked;
use crate::notifications::ShowNotification;
use crate::ui_checkbox::{CheckboxSpec, spawn_checkbox};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabButton, TabPanel, TabPlacement, TabSpec, TabStrip, spawn_tab_container,
};

/// The blank material the reference seeds the material swatches with
/// (`BLANK_MATERIAL_ASSET_ID`), and which Apply's material check passes
/// without loading.
pub(super) const BLANK_MATERIAL: Uuid = Uuid::from_u128(0x968c_bad0_4dad_d64e_71b5_72bf_13ad_051a);

/// The largest terrain texture Apply lets through, in either dimension — the
/// reference's `RenderMaxTextureResolution` default it checks against.
pub(super) const MAX_TERRAIN_TEXTURE_SIZE: u32 = 2048;

/// Which set of assets the terrain's four detail slots hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TerrainKind {
    /// Four textures — the classic splat.
    #[default]
    Textures,
    /// Four PBR materials.
    Pbr,
}

/// One terrain material's texture transform, as the Transforms section edits
/// it: the same transform on every texture of the material, as the reference's
/// PBR terrain shader only supports one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TerrainTransform {
    /// The UV scale `(u, v)`.
    pub scale: [f32; 2],
    /// The UV rotation, in degrees.
    pub rotation_degrees: f32,
    /// The UV offset `(u, v)`.
    pub offset: [f32; 2],
}

impl Default for TerrainTransform {
    /// The identity: unit scale, no rotation, no offset.
    fn default() -> Self {
        Self {
            scale: [1.0, 1.0],
            rotation_degrees: 0.0,
            offset: [0.0, 0.0],
        }
    }
}

impl TerrainTransform {
    /// The five values in the Transforms row's column order.
    const fn columns(self) -> [f32; 5] {
        [
            self.scale[0],
            self.scale[1],
            self.rotation_degrees,
            self.offset[0],
            self.offset[1],
        ]
    }

    /// Set the value of column `column` (see [`Self::columns`]).
    const fn set_column(&mut self, column: usize, value: f32) {
        match column {
            0 => self.scale[0] = value,
            1 => self.scale[1] = value,
            2 => self.rotation_degrees = value,
            3 => self.offset[0] = value,
            4 => self.offset[1] = value,
            _ => {}
        }
    }
}

/// One window's PBR-variant state, on its [`AboutRegionState`].
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct TerrainPbr {
    /// Which set the swatches and Apply use.
    pub kind: TerrainKind,
    /// Whether the person picked [`Self::kind`] with the checkbox, after which
    /// what the stores say no longer moves it.
    chosen: bool,
    /// Whether the stores have said which kind the region's assets are.
    resolved: bool,
    /// The four ids the region last stated, which [`Self::materials`] was
    /// seeded from.
    region_ids: Option<[Uuid; 4]>,
    /// The four material swatches' ids.
    pub materials: [Uuid; 4],
    /// The four materials' texture transforms.
    pub transforms: [TerrainTransform; 4],
    /// The region whose overrides have been asked for over `ModifyRegion`.
    overrides_region: Option<Uuid>,
    /// What each transform field was last given, for the edit-preserving reseed.
    shown_transforms: Option<[[String; 5]; 4]>,
    /// Whether the transform fields need writing from [`Self::transforms`].
    reseed_transforms: bool,
}

/// The Terrain tab's PBR-variant handles, on [`TerrainHandles`].
#[derive(Debug, Default)]
pub(super) struct PbrHandles {
    /// The "PBR Metallic Roughness" checkbox.
    check: Option<Entity>,
    /// The "Terrain textures" caption.
    texture_label: Option<Entity>,
    /// The "Terrain materials" caption.
    material_label: Option<Entity>,
    /// The four material swatches (lowest to highest ground).
    materials: [Option<Entity>; 4],
    /// The Elevation / Transforms sub-tab strip's Transforms panel, whose
    /// [`TabPanel`] names the strip and the button to hide.
    transforms_panel: Option<Entity>,
    /// Each material's five transform fields, in [`TerrainTransform::columns`]
    /// order.
    transform_fields: [[Option<Entity>; 5]; 4],
}

/// A marker on a terrain material swatch, carrying the slot it edits.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct TerrainMaterialSwatch {
    /// The detail slot (0–3).
    slot: usize,
}

/// A marker on the "PBR Metallic Roughness" checkbox.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct TerrainPbrCheck;

/// Whether the agent's region grants `ModifyRegion`, the capability the
/// Transforms section reads and writes over: Second Life does, OpenSim does not
/// (`book/src/gridspec/terrain.md`).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ModifyRegionSupport {
    /// Whether the region grants the capability.
    supported: bool,
}

/// The four default terrain textures — what a nil slot is drawn with, and what
/// the texture swatches fall back to when the region turns out to be on
/// materials (the reference's `mLastSetTextures` start).
fn default_terrain_textures() -> [Uuid; 4] {
    RegionTerrainComposition {
        detail_textures: [Uuid::nil(); 4],
        start_heights: [0.0; 4],
        height_ranges: [0.0; 4],
    }
    .detail_textures_or_default()
}

/// Which kind the stores say `ids` are, or `None` while they cannot tell — the
/// reference's `getMaterialType`: any decoded texture means textures, else any
/// decoded material means materials.
pub(super) fn detect_kind(
    ids: &[Uuid; 4],
    is_texture: impl Fn(Uuid) -> bool,
    is_material: impl Fn(Uuid) -> bool,
) -> Option<TerrainKind> {
    if ids.iter().any(|&id| is_texture(id)) {
        Some(TerrainKind::Textures)
    } else if ids.iter().any(|&id| is_material(id)) {
        Some(TerrainKind::Pbr)
    } else {
        None
    }
}

/// What Apply's texture check needs to know about one detail texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TextureFacts {
    /// The source's component count (3 = RGB, 4 = RGBA); `0` when it has not
    /// decoded, as the reference's unloaded texture reports.
    pub components: u16,
    /// Whether any texel is less than fully opaque.
    pub has_transparency: bool,
    /// The full-resolution size, once known.
    pub native: Option<(u32, u32)>,
}

/// The notices Apply's texture check raises for four detail textures — the
/// reference's `validateTextureSizes`. Empty when all four pass.
pub(super) fn texture_notices(facts: &[TextureFacts; 4]) -> Vec<ShowNotification> {
    let mut notices = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        let number = index.saturating_add(1).to_string();
        if fact.components != 3 && fact.components != 4 {
            notices.push(
                ShowNotification::new("InvalidTerrainBitDepth")
                    .arg("TEXTURE_NUM", number)
                    .arg(
                        "TEXTURE_BIT_DEPTH",
                        u32::from(fact.components).saturating_mul(8).to_string(),
                    )
                    .arg("MAX_SIZE", MAX_TERRAIN_TEXTURE_SIZE.to_string()),
            );
            continue;
        }
        if fact.components == 4 && fact.has_transparency {
            notices.push(
                ShowNotification::new("InvalidTerrainAlpha").arg("TEXTURE_NUM", number.clone()),
            );
        }
        if let Some((width, height)) = fact.native
            && (width > MAX_TERRAIN_TEXTURE_SIZE || height > MAX_TERRAIN_TEXTURE_SIZE)
        {
            notices.push(
                ShowNotification::new("InvalidTerrainSize")
                    .arg("TEXTURE_NUM", number)
                    .arg("TEXTURE_SIZE_X", width.to_string())
                    .arg("TEXTURE_SIZE_Y", height.to_string())
                    .arg("MAX_SIZE", MAX_TERRAIN_TEXTURE_SIZE.to_string()),
            );
        }
    }
    notices
}

/// What Apply's material check knows about one detail material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MaterialFacts {
    /// The blank material, which passes unloaded.
    Blank,
    /// Still being fetched.
    Loading,
    /// The fetch or the decode failed.
    Failed,
    /// Loaded.
    Loaded {
        /// Whether it is double-sided.
        double_sided: bool,
        /// Its alpha mode.
        alpha_mode: GltfAlphaMode,
    },
}

/// The notices Apply's material check raises for four detail materials — the
/// reference's `validateMaterials`. Empty when all four pass.
pub(super) fn material_notices(facts: &[MaterialFacts; 4]) -> Vec<ShowNotification> {
    let mut notices = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        let number = index.saturating_add(1).to_string();
        match *fact {
            MaterialFacts::Blank => {}
            MaterialFacts::Loading => notices.push(
                ShowNotification::new("InvalidTerrainMaterialNotLoaded")
                    .arg("MATERIAL_NUM", number),
            ),
            MaterialFacts::Failed => notices.push(
                ShowNotification::new("InvalidTerrainMaterialLoadFailed")
                    .arg("MATERIAL_NUM", number),
            ),
            MaterialFacts::Loaded {
                double_sided,
                alpha_mode,
            } => {
                if double_sided {
                    notices.push(
                        ShowNotification::new("InvalidTerrainMaterialDoubleSided")
                            .arg("MATERIAL_NUM", number.clone()),
                    );
                }
                if alpha_mode == GltfAlphaMode::Blend {
                    notices.push(
                        ShowNotification::new("InvalidTerrainMaterialAlphaMode")
                            .arg("MATERIAL_NUM", number)
                            .arg("MATERIAL_ALPHA_MODE", "BLEND"),
                    );
                }
            }
        }
    }
    notices
}

/// Whether two transform components differ by more than rounding.
fn differs(left: &[f32; 2], right: &[f32; 2]) -> bool {
    left.iter()
        .zip(right)
        .any(|(left, right)| (left - right).abs() > f32::EPSILON)
}

/// A real number for an override's LLSD.
fn real(value: f32) -> Llsd {
    Llsd::Real(f64::from(value))
}

/// The `ModifyRegion` overrides the four transforms make: per material, the
/// transform on all four of its texture infos (`ti`), each component only where
/// it differs from the identity, and an empty map for a material with none — the
/// reference's `getOverrideLLSD` against the default material.
pub(super) fn overrides_from_transforms(
    transforms: &[TerrainTransform; 4],
) -> TerrainMaterialOverrides {
    let identity = TerrainTransform::default();
    let mut overrides = TerrainMaterialOverrides::default();
    for (slot, transform) in overrides.0.iter_mut().zip(transforms.iter()) {
        if *transform == identity {
            continue;
        }
        let mut info = HashMap::new();
        if differs(&transform.offset, &identity.offset) {
            let _previous = info.insert(
                "o".to_owned(),
                Llsd::Array(transform.offset.iter().copied().map(real).collect()),
            );
        }
        if differs(&transform.scale, &identity.scale) {
            let _previous = info.insert(
                "s".to_owned(),
                Llsd::Array(transform.scale.iter().copied().map(real).collect()),
            );
        }
        if (transform.rotation_degrees - identity.rotation_degrees).abs() > f32::EPSILON {
            let _previous = info.insert(
                "r".to_owned(),
                real(transform.rotation_degrees.to_radians()),
            );
        }
        let infos = vec![Llsd::Map(info); 4];
        *slot = Llsd::Map(HashMap::from([("ti".to_owned(), Llsd::Array(infos))]));
    }
    overrides
}

/// The transforms a `ModifyRegion` answer carries: each material's first
/// texture info, the one the reference's terrain shader reads; the identity
/// where a material has none.
pub(super) fn transforms_from_overrides(
    overrides: &TerrainMaterialOverrides,
) -> [TerrainTransform; 4] {
    let pair = |value: Option<&Llsd>, fallback: [f32; 2]| {
        value
            .and_then(Llsd::as_array)
            .and_then(|array| Some([array.first()?.as_f32()?, array.get(1)?.as_f32()?]))
            .unwrap_or(fallback)
    };
    let mut transforms = [TerrainTransform::default(); 4];
    for (transform, slot) in transforms.iter_mut().zip(overrides.0.iter()) {
        let Some(info) = slot
            .get("ti")
            .and_then(Llsd::as_array)
            .and_then(<[Llsd]>::first)
        else {
            continue;
        };
        transform.scale = pair(info.get("s"), transform.scale);
        transform.offset = pair(info.get("o"), transform.offset);
        if let Some(radians) = info.get("r").and_then(Llsd::as_f32) {
            transform.rotation_degrees = radians.to_degrees();
        }
    }
    transforms
}

/// The stores Apply's checks read, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct TerrainChecks<'w> {
    /// The decoded textures.
    decoded: Option<Res<'w, DecodedTextures>>,
    /// The texture manager, for a texture's full-resolution size.
    textures: Option<Res<'w, TextureManager>>,
    /// The material manager.
    materials: Option<Res<'w, MaterialManager>>,
    /// Whether the region grants `ModifyRegion`.
    modify_region: Res<'w, ModifyRegionSupport>,
}

impl TerrainChecks<'_> {
    /// What the check needs to know about texture `id`.
    fn texture_facts(&self, id: Uuid) -> TextureFacts {
        let key = TextureKey::from(id);
        let image = self.decoded.as_ref().and_then(|decoded| decoded.get(key));
        let native = match (self.textures.as_ref(), self.decoded.as_ref()) {
            (Some(textures), Some(decoded)) => textures.native_dimensions(key, decoded),
            _ => None,
        };
        image.map_or(
            TextureFacts {
                components: 0,
                has_transparency: false,
                native: None,
            },
            |image| TextureFacts {
                components: image.components,
                has_transparency: image.min_alpha < u8::MAX,
                native,
            },
        )
    }

    /// What the check knows about material `id`.
    fn material_facts(&self, id: Uuid) -> MaterialFacts {
        if id == BLANK_MATERIAL {
            return MaterialFacts::Blank;
        }
        let key = AssetKey::from(id);
        let Some(materials) = self.materials.as_ref() else {
            return MaterialFacts::Loading;
        };
        match materials.material_status(key) {
            MaterialStatus::Decoded => {
                materials
                    .decoded_material(key)
                    .map_or(MaterialFacts::Failed, |material| MaterialFacts::Loaded {
                        double_sided: material.double_sided,
                        alpha_mode: material.alpha_mode,
                    })
            }
            MaterialStatus::Loading | MaterialStatus::Unrequested => MaterialFacts::Loading,
            MaterialStatus::Unavailable => MaterialFacts::Failed,
        }
    }

    /// The terrain update and overrides an Apply sends, or the notices that stop
    /// it — the checks of the reference's `sendUpdate`, in its order.
    pub(super) fn prepare_apply(
        &self,
        state: &AboutRegionState,
    ) -> Result<(RegionTerrainUpdate, Option<TerrainMaterialOverrides>), Vec<ShowNotification>>
    {
        let pbr = &state.terrain_pbr;
        let mut update = state.terrain_draft.clone();
        match pbr.kind {
            TerrainKind::Textures => {
                let facts = update.detail_textures.map(|id| self.texture_facts(id));
                let notices = texture_notices(&facts);
                if !notices.is_empty() {
                    return Err(notices);
                }
                Ok((update, None))
            }
            TerrainKind::Pbr => {
                let facts = pbr.materials.map(|id| self.material_facts(id));
                let notices = material_notices(&facts);
                if !notices.is_empty() {
                    return Err(notices);
                }
                update.detail_textures = pbr.materials;
                let overrides = self
                    .modify_region
                    .supported
                    .then(|| overrides_from_transforms(&pbr.transforms));
                Ok((update, overrides))
            }
        }
    }
}

/// Build the textures caption row: the two captions (one shown at a time) and
/// the "PBR Metallic Roughness" checkbox.
pub(super) fn build_caption_row(
    commands: &mut Commands,
    panel: Entity,
    handles: &mut TerrainHandles,
    font_size: f32,
) {
    let row = spawn_row(commands, panel);
    let texture_label = commands.spawn((Node::default(), ChildOf(row))).id();
    spawn_key_label(
        commands,
        texture_label,
        "about-region-terrain-textures",
        DIM_LABEL_COLOR,
        font_size,
    );
    let material_label = commands
        .spawn((
            Node {
                display: Display::None,
                ..Default::default()
            },
            ChildOf(row),
        ))
        .id();
    spawn_key_label(
        commands,
        material_label,
        "about-region-terrain-materials",
        DIM_LABEL_COLOR,
        font_size,
    );
    let checkbox = spawn_checkbox(
        commands,
        row,
        &CheckboxSpec {
            element: "about-region-terrain-pbr",
            label: "about-region-terrain-pbr".to_owned(),
            tab_index: 5,
            font_size,
            translate_label: true,
        },
    );
    commands
        .entity(checkbox.checkbox)
        .insert((TerrainPbrCheck, EditGate))
        .observe(on_terrain_pbr_check);
    handles.pbr.check = Some(checkbox.checkbox);
    handles.pbr.texture_label = Some(texture_label);
    handles.pbr.material_label = Some(material_label);
}

/// Put slot `slot`'s material swatch beside its texture swatch in `row`, hidden
/// until the tab is on materials.
pub(super) fn build_material_swatch(
    commands: &mut Commands,
    row: Entity,
    slot: usize,
    handles: &mut TerrainHandles,
) {
    let swatch = spawn_material_swatch(
        commands,
        row,
        "about-region-terrain-material",
        5,
        TextureKey::from(Uuid::nil()),
        BLANK_MATERIAL,
    );
    commands.entity(swatch).insert((
        TerrainMaterialSwatch { slot },
        EditGate,
        MaterialPreview::Empty,
        Node {
            display: Display::None,
            ..Default::default()
        },
    ));
    // Called what its slot's caption says.
    if let Some(key) = TRANSFORM_ROWS.get(slot) {
        commands
            .entity(swatch)
            .insert(Semantic::new(Role::Button).name_key(*key));
    }
    if let Some(entry) = handles.pbr.materials.get_mut(slot) {
        *entry = Some(swatch);
    }
}

/// What one arrow step of a transform column does, and its range — the
/// reference's `panel_settings_terrain_transform.xml` spinners.
const TRANSFORM_STEPS: [SpinStep; 5] = [
    SpinStep::new(0.1, -100.0, 100.0, 3),
    SpinStep::new(0.1, -100.0, 100.0, 3),
    SpinStep::new(1.0, -360.0, 360.0, 2),
    SpinStep::new(0.1, -999.0, 999.0, 3),
    SpinStep::new(0.1, -999.0, 999.0, 3),
];

/// The transform columns' captions, in [`TerrainTransform::columns`] order.
const TRANSFORM_COLUMNS: [&str; 5] = [
    "about-region-terrain-scale-u",
    "about-region-terrain-scale-v",
    "about-region-terrain-rotation",
    "about-region-terrain-offset-u",
    "about-region-terrain-offset-v",
];

/// The four material rows' captions.
const TRANSFORM_ROWS: [&str; 4] = [
    "about-region-terrain-tex-1",
    "about-region-terrain-tex-2",
    "about-region-terrain-tex-3",
    "about-region-terrain-tex-4",
];

/// Build the Elevation / Transforms sub-tab strip under `panel`, returning its
/// two panels. The Transforms tab's button is hidden until the tab is on
/// materials ([`sync_terrain_pbr_controls`]).
pub(super) fn build_terrain_sub_tabs(
    commands: &mut Commands,
    panel: Entity,
    handles: &mut TerrainHandles,
    font_size: f32,
) -> Vec<Entity> {
    let labels: Vec<String> = [
        "about-region-terrain-elevation-tab",
        "about-region-terrain-transforms-tab",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs = spawn_tab_container(
        commands,
        panel,
        &TabSpec {
            element: "about-region-terrain-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 6,
            font_size,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
            names: &[],
        },
    );
    handles.pbr.transforms_panel = tabs.panels.get(1).copied();
    tabs.panels
}

/// Build the Transforms section into its sub-tab panel: a row per material of
/// its five transform fields.
pub(super) fn build_transforms(
    commands: &mut Commands,
    panel: Entity,
    handles: &mut TerrainHandles,
    font_size: f32,
) {
    let section = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..Default::default()
            },
            ChildOf(panel),
        ))
        .id();
    let caption = spawn_row(commands, section);
    spawn_key_label(
        commands,
        caption,
        "about-region-terrain-transforms",
        DIM_LABEL_COLOR,
        font_size,
    );
    for (slot, row_key) in TRANSFORM_ROWS.into_iter().enumerate() {
        let row = spawn_row(commands, section);
        spawn_key_label(commands, row, row_key, DIM_LABEL_COLOR, font_size);
        for (column, (column_key, step)) in TRANSFORM_COLUMNS
            .into_iter()
            .zip(TRANSFORM_STEPS)
            .enumerate()
        {
            spawn_key_label(commands, row, column_key, DIM_LABEL_COLOR, font_size);
            let field = spawn_terrain_field(
                commands,
                row,
                "about-region-terrain-transform",
                6,
                step,
                font_size,
            );
            // Each field is called what the caption before it says.
            commands
                .entity(field)
                .insert(Semantic::new(Role::SpinButton).name_key(column_key));
            if let Some(entry) = handles
                .pbr
                .transform_fields
                .get_mut(slot)
                .and_then(|row| row.get_mut(column))
            {
                *entry = Some(field);
            }
        }
    }
}

/// Read the Transforms section's fields into the window's transforms (on
/// Apply).
pub(super) fn read_transform_fields(
    state: &mut AboutRegionState,
    ui: &AboutRegionUi,
    read: &dyn Fn(Option<Entity>) -> Option<String>,
) {
    for (transform, fields) in state
        .terrain_pbr
        .transforms
        .iter_mut()
        .zip(ui.terrain.pbr.transform_fields.iter())
    {
        for (column, field) in fields.iter().enumerate() {
            if let Some(value) = read(*field).and_then(|text| text.trim().parse::<f32>().ok()) {
                transform.set_column(column, value);
            }
        }
    }
}

/// Fold each window's region ids into its PBR state and ask the stores which
/// kind they are, until they say.
pub(super) fn resolve_terrain_kind(
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    decoded: Option<Res<DecodedTextures>>,
    mut materials: Option<ResMut<MaterialManager>>,
    mut boosts: MessageWriter<BoostTexture>,
) {
    for (mut state, mut dirty) in &mut windows {
        if !state.is_current {
            continue;
        }
        let Some(ids) = state
            .identity
            .as_ref()
            .map(|identity| identity.terrain.detail_textures_or_default())
        else {
            continue;
        };
        if state.terrain_pbr.region_ids != Some(ids) {
            let pbr = &mut state.terrain_pbr;
            pbr.region_ids = Some(ids);
            pbr.materials = ids;
            pbr.resolved = false;
            pbr.chosen = false;
            pbr.kind = TerrainKind::Textures;
            for id in ids {
                boosts.write(BoostTexture {
                    key: TextureKey::from(id),
                    priority: AVATAR_BOOST_PRIORITY,
                });
                if let Some(materials) = materials.as_deref_mut() {
                    materials.request_material(AssetKey::from(id));
                }
            }
            dirty.terrain_pbr = true;
        }
        if state.terrain_pbr.resolved {
            continue;
        }
        let kind = detect_kind(
            &ids,
            |id| {
                decoded
                    .as_ref()
                    .is_some_and(|decoded| decoded.contains(TextureKey::from(id)))
            },
            |id| {
                materials.as_ref().is_some_and(|materials| {
                    materials.material_status(AssetKey::from(id)) == MaterialStatus::Decoded
                })
            },
        );
        let Some(kind) = kind else {
            continue;
        };
        let state = &mut *state;
        state.terrain_pbr.resolved = true;
        if state.terrain_pbr.chosen {
            continue;
        }
        state.terrain_pbr.kind = kind;
        match kind {
            // The other set starts where the reference's does, slot by slot
            // where nobody has picked one yet.
            TerrainKind::Textures => {
                for (material, id) in state.terrain_pbr.materials.iter_mut().zip(ids) {
                    if *material == id {
                        *material = BLANK_MATERIAL;
                    }
                }
            }
            TerrainKind::Pbr => {
                for ((texture, id), default) in state
                    .terrain_draft
                    .detail_textures
                    .iter_mut()
                    .zip(ids)
                    .zip(default_terrain_textures())
                {
                    if *texture == id {
                        *texture = default;
                    }
                }
                dirty.terrain_values = true;
            }
        }
        dirty.terrain_pbr = true;
    }
}

/// The sub-tab strip's parts, for finding and hiding the Transforms tab,
/// bundled as one [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct TerrainSubTabs<'w, 's> {
    /// The tab panels, to find the Transforms panel's strip and index.
    panels: Query<'w, 's, &'static TabPanel>,
    /// The tab buttons, to find the Transforms tab's own.
    buttons: Query<'w, 's, (Entity, &'static TabButton)>,
    /// The strips, to hand a hidden tab's strip back to Elevation.
    strips: Query<'w, 's, &'static mut TabStrip>,
}

/// Show the set of swatches, the caption and the Transforms section the
/// window's kind wants, tick the checkbox to match, and keep the material
/// swatches and transform fields showing the state.
pub(super) fn sync_terrain_pbr_controls(
    mut windows: Query<(&mut AboutRegionDirty, &mut AboutRegionState, &AboutRegionUi)>,
    mut nodes: Query<&mut Node>,
    sub_tabs: TerrainSubTabs,
    mut swatches: Query<(&mut MaterialSwatchValue, &mut MaterialPreview)>,
    checks: Query<Has<Checked>, With<TerrainPbrCheck>>,
    mut fields: Query<&mut EditableText>,
    mut commands: Commands,
) {
    let TerrainSubTabs {
        panels,
        buttons,
        mut strips,
    } = sub_tabs;
    for (mut dirty, mut state, ui) in &mut windows {
        if dirty.terrain_pbr {
            dirty.terrain_pbr = false;
            let pbr_on = state.terrain_pbr.kind == TerrainKind::Pbr;
            let mut show = |entity: Option<Entity>, shown: bool| {
                if let Some(mut node) = entity.and_then(|entity| nodes.get_mut(entity).ok()) {
                    let display = if shown { Display::Flex } else { Display::None };
                    if node.display != display {
                        node.display = display;
                    }
                }
            };
            for swatch in &ui.terrain.textures {
                show(*swatch, !pbr_on);
            }
            for swatch in &ui.terrain.pbr.materials {
                show(*swatch, pbr_on);
            }
            show(ui.terrain.pbr.texture_label, !pbr_on);
            show(ui.terrain.pbr.material_label, pbr_on);
            // The Transforms sub-tab exists only on materials; leaving it for
            // textures hands its strip back to Elevation.
            if let Some(tab) = ui
                .terrain
                .pbr
                .transforms_panel
                .and_then(|panel| panels.get(panel).ok())
            {
                let button = buttons
                    .iter()
                    .find(|(_entity, button)| {
                        button.strip == tab.strip && button.index == tab.index
                    })
                    .map(|(entity, _button)| entity);
                show(button, pbr_on);
                if !pbr_on
                    && let Ok(mut strip) = strips.get_mut(tab.strip)
                    && strip.active == tab.index
                {
                    strip.active = 0;
                }
            }
            if let Some(check) = ui.terrain.pbr.check
                && let Ok(is_ticked) = checks.get(check)
            {
                set_check_marker(&mut commands, check, is_ticked, pbr_on);
            }
            for (swatch, id) in ui
                .terrain
                .pbr
                .materials
                .iter()
                .zip(state.terrain_pbr.materials)
            {
                let Some((mut value, mut preview)) =
                    swatch.and_then(|swatch| swatches.get_mut(swatch).ok())
                else {
                    continue;
                };
                if value.0 != id {
                    value.0 = id;
                }
                let want = if id.is_nil() {
                    MaterialPreview::Empty
                } else {
                    MaterialPreview::Asset(AssetKey::from(id))
                };
                if *preview != want {
                    *preview = want;
                }
            }
        }
        if state.terrain_pbr.reseed_transforms {
            let pbr = &mut state.terrain_pbr;
            pbr.reseed_transforms = false;
            let wanted = pbr
                .transforms
                .map(|transform| transform.columns().map(|value| format!("{value:.3}")));
            let mut left = wanted.clone();
            for (slot, row) in ui.terrain.pbr.transform_fields.iter().enumerate() {
                for (column, field) in row.iter().enumerate() {
                    let previous = pbr
                        .shown_transforms
                        .as_ref()
                        .and_then(|shown| shown.get(slot)?.get(column))
                        .map(String::as_str);
                    let (Some(want), Some(left)) = (
                        wanted.get(slot).and_then(|row| row.get(column)),
                        left.get_mut(slot).and_then(|row| row.get_mut(column)),
                    ) else {
                        continue;
                    };
                    seed_one_field(&mut fields, *field, want, previous, left);
                }
            }
            pbr.shown_transforms = Some(left);
        }
    }
}

/// The checkbox flipped: switch the window to the other set — refused, and
/// the tick put back, where the agent may not manage the estate.
fn on_terrain_pbr_check(
    change: On<ValueChange<bool>>,
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut commands: Commands,
) {
    let entity = change.source;
    let Some(window) = host_floater(entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, mut dirty)) = windows.get_mut(window) else {
        return;
    };
    if !state.can_manage {
        let on = state.terrain_pbr.kind == TerrainKind::Pbr;
        set_check_marker(&mut commands, entity, change.value, on);
        return;
    }
    state.terrain_pbr.kind = if change.value {
        TerrainKind::Pbr
    } else {
        TerrainKind::Textures
    };
    state.terrain_pbr.chosen = true;
    dirty.terrain_pbr = true;
}

/// Fold a material pick into the material slot of the window whose swatch
/// asked, and start loading it so Apply can check it.
pub(super) fn apply_material_picks(
    mut picks: MessageReader<TexturePicked>,
    swatches: Query<&TerrainMaterialSwatch>,
    host: FloaterHost,
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    mut materials: Option<ResMut<MaterialManager>>,
) {
    for pick in picks.read() {
        let Ok(swatch) = swatches.get(pick.requester) else {
            continue;
        };
        let Some(window) = host.of(pick.requester) else {
            continue;
        };
        let Ok((mut state, mut dirty)) = windows.get_mut(window) else {
            continue;
        };
        let id = pick.texture.uuid();
        let Some(slot) = state.terrain_pbr.materials.get_mut(swatch.slot) else {
            continue;
        };
        if *slot == id {
            continue;
        }
        *slot = id;
        if let Some(materials) = materials.as_deref_mut()
            && id != BLANK_MATERIAL
        {
            materials.request_material(AssetKey::from(id));
        }
        dirty.terrain_pbr = true;
    }
}

/// Fold each capability map into [`ModifyRegionSupport`].
pub(super) fn ingest_modify_region_capability(
    mut capabilities: MessageReader<SlCapabilities>,
    mut support: ResMut<ModifyRegionSupport>,
) {
    for SlCapabilities(map) in capabilities.read() {
        let supported = map.contains_key(CAP_MODIFY_REGION);
        if supported != support.supported {
            support.supported = supported;
        }
    }
}

/// Ask for the region's terrain overrides once per region a current window
/// shows, where the region grants `ModifyRegion` — the reference reads them on
/// every region handshake.
pub(super) fn request_terrain_overrides(
    mut windows: Query<&mut AboutRegionState>,
    support: Res<ModifyRegionSupport>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if !support.supported {
        return;
    }
    let mut asked = false;
    for mut state in &mut windows {
        if !state.is_current || state.terrain_pbr.overrides_region == Some(state.region) {
            continue;
        }
        let region = state.region;
        state.terrain_pbr.overrides_region = Some(region);
        asked = true;
    }
    if asked {
        sl_commands.write(SlCommand(Command::RequestTerrainMaterialOverrides));
    }
}

/// Take the region's overrides into each current window's transforms, and ask
/// again after a write (the reference's GET after the write's handshake).
pub(super) fn ingest_terrain_overrides(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut AboutRegionState>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::TerrainMaterialOverrides(answer) => {
                let Some(overrides) = answer.overrides.as_ref() else {
                    continue;
                };
                let transforms = transforms_from_overrides(overrides);
                for mut state in &mut windows {
                    if state.is_current {
                        state.terrain_pbr.transforms = transforms;
                        state.terrain_pbr.reseed_transforms = true;
                    }
                }
            }
            SlSessionEvent::TerrainMaterialOverridesWritten(_answer) => {
                for mut state in &mut windows {
                    if state.is_current {
                        state.terrain_pbr.overrides_region = None;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{Llsd, TerrainMaterialOverrides, Uuid};
    use sl_material::GltfAlphaMode;

    use super::{
        BLANK_MATERIAL, MaterialFacts, TerrainKind, TerrainTransform, TextureFacts, detect_kind,
        material_notices, overrides_from_transforms, texture_notices, transforms_from_overrides,
    };

    /// The templates of `notices`, in order.
    fn templates(notices: &[crate::notifications::ShowNotification]) -> Vec<&'static str> {
        notices.iter().map(|notice| notice.template).collect()
    }

    /// An opaque RGB texture of `size` square.
    const fn rgb(size: u32) -> TextureFacts {
        TextureFacts {
            components: 3,
            has_transparency: false,
            native: Some((size, size)),
        }
    }

    /// Any decoded texture means textures; else any decoded material means
    /// materials; else nobody can tell yet.
    #[test]
    fn the_kind_follows_what_decoded() {
        let ids = [1, 2, 3, 4].map(Uuid::from_u128);
        let texture = Uuid::from_u128(3);
        let material = Uuid::from_u128(2);
        assert_eq!(
            detect_kind(&ids, |id| id == texture, |id| id == material),
            Some(TerrainKind::Textures),
            "a texture wins, as in the reference"
        );
        assert_eq!(
            detect_kind(&ids, |_id| false, |id| id == material),
            Some(TerrainKind::Pbr)
        );
        assert_eq!(detect_kind(&ids, |_id| false, |_id| false), None);
    }

    /// Four opaque RGB textures within the limit pass.
    #[test]
    fn good_textures_pass() {
        assert!(texture_notices(&[rgb(512), rgb(1024), rgb(2048), rgb(256)]).is_empty());
    }

    /// Each texture failure is its own notice, naming the slot.
    #[test]
    fn bad_textures_are_named_by_slot() {
        let notices = texture_notices(&[
            TextureFacts {
                components: 1,
                has_transparency: false,
                native: Some((256, 256)),
            },
            TextureFacts {
                components: 4,
                has_transparency: true,
                native: Some((256, 256)),
            },
            rgb(4096),
            TextureFacts {
                components: 0,
                has_transparency: false,
                native: None,
            },
        ]);
        assert_eq!(
            templates(&notices),
            [
                "InvalidTerrainBitDepth",
                "InvalidTerrainAlpha",
                "InvalidTerrainSize",
                "InvalidTerrainBitDepth",
            ]
        );
        assert_eq!(
            notices.first().map(|notice| notice.args.pairs().to_vec()),
            Some(vec![
                ("TEXTURE_NUM".to_owned(), "1".to_owned()),
                ("TEXTURE_BIT_DEPTH".to_owned(), "8".to_owned()),
                ("MAX_SIZE".to_owned(), "2048".to_owned()),
            ])
        );
    }

    /// An opaque RGBA texture passes: only real transparency is refused.
    #[test]
    fn opaque_rgba_passes() {
        let opaque = TextureFacts {
            components: 4,
            has_transparency: false,
            native: Some((512, 512)),
        };
        assert!(texture_notices(&[opaque; 4]).is_empty());
    }

    /// The blank material and opaque or masked single-sided ones pass; each
    /// failure is its own notice.
    #[test]
    fn materials_are_checked_like_the_reference() {
        let opaque = MaterialFacts::Loaded {
            double_sided: false,
            alpha_mode: GltfAlphaMode::Opaque,
        };
        let masked = MaterialFacts::Loaded {
            double_sided: false,
            alpha_mode: GltfAlphaMode::Mask,
        };
        assert!(material_notices(&[MaterialFacts::Blank, opaque, masked, opaque]).is_empty());
        let notices = material_notices(&[
            MaterialFacts::Loading,
            MaterialFacts::Failed,
            MaterialFacts::Loaded {
                double_sided: true,
                alpha_mode: GltfAlphaMode::Blend,
            },
            opaque,
        ]);
        assert_eq!(
            templates(&notices),
            [
                "InvalidTerrainMaterialNotLoaded",
                "InvalidTerrainMaterialLoadFailed",
                "InvalidTerrainMaterialDoubleSided",
                "InvalidTerrainMaterialAlphaMode",
            ]
        );
        let _blank = BLANK_MATERIAL;
    }

    /// The identity writes an empty override; a transform writes only what
    /// moved, on all four texture infos, and reads back as itself.
    #[test]
    fn transforms_round_trip_through_the_overrides() {
        let mut transforms = [TerrainTransform::default(); 4];
        if let Some(second) = transforms.get_mut(1) {
            *second = TerrainTransform {
                scale: [4.0, 2.0],
                rotation_degrees: 90.0,
                offset: [0.25, 0.0],
            };
        }
        let overrides = overrides_from_transforms(&transforms);
        assert_eq!(
            overrides.0.first(),
            Some(&Llsd::Map(std::collections::HashMap::new())),
            "no override for the identity"
        );
        let infos = overrides
            .0
            .get(1)
            .and_then(|slot| slot.get("ti"))
            .and_then(Llsd::as_array)
            .map(<[Llsd]>::len);
        assert_eq!(infos, Some(4), "the same transform on every texture info");
        let back = transforms_from_overrides(&overrides);
        for (read, written) in back.iter().zip(transforms.iter()) {
            assert!(
                (read.scale[0] - written.scale[0]).abs() < 1e-4,
                "{read:?} vs {written:?}"
            );
            assert!(
                (read.scale[1] - written.scale[1]).abs() < 1e-4,
                "{read:?} vs {written:?}"
            );
            assert!(
                (read.rotation_degrees - written.rotation_degrees).abs() < 1e-3,
                "{read:?} vs {written:?}"
            );
            assert!(
                (read.offset[0] - written.offset[0]).abs() < 1e-4,
                "{read:?} vs {written:?}"
            );
            assert!(
                (read.offset[1] - written.offset[1]).abs() < 1e-4,
                "{read:?} vs {written:?}"
            );
        }
    }

    /// Second Life's answer for a region with none — four `undef`s — reads as
    /// four identities.
    #[test]
    fn undefined_overrides_read_as_the_identity() {
        let none = TerrainMaterialOverrides([Llsd::Undef, Llsd::Undef, Llsd::Undef, Llsd::Undef]);
        assert_eq!(
            transforms_from_overrides(&none),
            [TerrainTransform::default(); 4]
        );
    }
}
