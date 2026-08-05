//! World-space avatar name-tag billboards — the reference feature's rendering
//! (`llhudnametag.cpp`, `llvoavatar.cpp::idleUpdateNameTag`), replacing the old
//! screen-space overlay-camera projection this viewer used to draw.
//!
//! Each tag is a small mesh authored in **tag-local physical pixels ÷ 1024**
//! around the bubble centre — an SDF rounded-rect backdrop, drop-shadow glyph
//! copies, then the glyph quads — drawn by the main world camera in the
//! transparent phase ([`AlphaMode::Blend`]: depth-**tested** against world
//! geometry, no depth write), so occlusion, depth sorting and distance fade all
//! come from the world pass rather than special-cased projection code. The
//! vertex shader (`name_tag.wgsl`) expands the mesh into a camera-facing
//! billboard whose **on-screen size is constant at every distance** (the
//! reference's pixel-vector behaviour), pulls it toward the camera by the
//! avatar's radius so the tag is not swallowed by its own avatar's head, and
//! applies the reference's distance fade in-shader.
//!
//! Per-frame state deliberately never touches a material asset (mutating one
//! recreates its whole bind group): the anchor position rides the entity
//! [`Transform`], and the anti-overlap screen offset rides the per-instance
//! [`MeshTag`] (packed by [`pack_overlap_offset`]).
//!
//! Camera queries here must stay qualified `With<ViewerCamera>` — the P33.2
//! reflection-probe cameras make any unqualified `single()` fail every frame
//! (roadmap `viewer-name-tags-lost-to-probe-cameras`). Tag entities are
//! **top-level** (never children of the avatar anchor, whose
//! `Propagate(dynamic_render_layers())` would leak the probe layer onto them)
//! and carry [`RenderLayers`] layer 0 only, so probe cameras never see a tag.

use bevy::asset::{Asset, load_internal_asset, uuid_handle};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::entity::EntityHashSet;
use bevy::mesh::{MeshTag, MeshVertexBufferLayoutRef};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::text::{
    ComputedTextBlock, FontAtlasSet, FontCx, LayoutCx, LetterSpacing, LineHeight, RemSize, ScaleCx,
    TextBounds, TextLayoutInfo, TextPipeline, TextReader, TextSection,
};
use bevy::window::PrimaryWindow;

use sl_client_bevy::{AgentKey, SlIdentity};

use crate::avatars::{AvatarAnchor, AvatarPickTarget, NameTag};
use crate::name_tag_content::TagContent;
use crate::ui_font::UiFont;

/// The internal handle the name-tag shader (`name_tag.wgsl`) is loaded under,
/// so the material can reference it without an on-disk asset path.
const NAME_TAG_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7c2f31d8-9b4a-4e0f-8a67-2d5c90e1b3f4");

/// Tag-local mesh units per physical pixel: positions are authored as pixels ÷
/// 1024 (exact in `f32`) so the mesh AABB stays tiny and the transparent-phase
/// sort key — the transformed AABB centre — stays on the anchor position.
pub(crate) const TAG_UNITS_PER_PIXEL: f32 = 1.0 / 1024.0;

/// The reference's horizontal bubble padding, physical px each side
/// (`llhudnametag.cpp` `HORIZONTAL_PADDING` 16, halved per side there because
/// it pads the summed width; we keep the summed value and split it in the
/// builder).
pub(crate) const HORIZONTAL_PADDING_PX: f32 = 16.0;

/// The reference's vertical bubble padding, physical px total
/// (`llhudnametag.cpp` `VERTICAL_PADDING` 12).
pub(crate) const VERTICAL_PADDING_PX: f32 = 12.0;

/// The reference's leading between text lines, logical px
/// (`llhudnametag.cpp` `LINE_PADDING` 3).
pub(crate) const LINE_PADDING_PX: f32 = 3.0;

/// The reference's maximum tag text width, logical px, before word-wrap
/// (`llhudnametag.cpp` `NAMETAG_MAX_WIDTH` 298 — "fits 31 M's").
pub(crate) const MAX_TAG_WIDTH_PX: f32 = 298.0;

/// The screen-space lift above the anchor, **logical** px (the reference's
/// `NAMETAG_VERTICAL_SCREEN_OFFSET` 25, `llvoavatar.cpp`). Baked into the tag
/// mesh (where the layout scale factor is known), so the whole bubble floats
/// this far above the projected anchor point.
pub(crate) const BASE_LIFT_PX: f32 = 25.0;

/// Default distance at which a tag starts fading, metres (the reference's
/// `CHAT_NORMAL_RADIUS` 20 — `setFadeDistance(CHAT_NORMAL_RADIUS, 5)`).
pub(crate) const DEFAULT_FADE_START_METRES: f32 = 20.0;

/// Default fade range, metres: a tag is fully gone `FadeRange` past the fade
/// start (reference: 5 m, so tags vanish at 25 m).
pub(crate) const DEFAULT_FADE_RANGE_METRES: f32 = 5.0;

/// The distance at which tags start fading, metres (a float setting;
/// default [`DEFAULT_FADE_START_METRES`]).
pub(crate) const SETTING_FADE_START: &str = "FadeStartDistance";

/// The fade range, metres past the fade start at which tags are gone (a
/// float setting; default [`DEFAULT_FADE_RANGE_METRES`]).
pub(crate) const SETTING_FADE_RANGE: &str = "FadeRange";

/// The bubble backdrop opacity (the reference `ChatBubbleOpacity`,
/// default 0.5).
pub(crate) const SETTING_BUBBLE_OPACITY: &str = "BubbleOpacity";

/// The neutral (no anti-overlap offset) [`MeshTag`] value: both packed
/// components at the `+32768` bias.
pub(crate) const NEUTRAL_MESH_TAG: u32 = 0x8000_8000;

/// The bias added to each signed offset component when packing into a
/// [`MeshTag`] half (mirrors `TAG_OFFSET_BIAS` in `name_tag.wgsl`).
const TAG_OFFSET_BIAS: i32 = 0x8000;

/// The render layers a tag entity lives on: the main viewpoint layer **only**.
/// Deliberately not derived from any avatar subtree propagation — probe
/// cameras (layers 4/5/6) and the HUD camera (layer 1) must never see tags.
pub(crate) const fn tag_render_layers() -> RenderLayers {
    RenderLayers::layer(crate::probe_layers::MAIN_LAYER)
}

/// Pack a billboard-local anti-overlap offset (physical px, +y up) into a
/// [`MeshTag`] value: each component rounded to whole pixels, offset-biased by
/// `+32768`, and clamped into `u16` range (x in the high half, y in the low).
pub(crate) fn pack_overlap_offset(offset: Vec2) -> u32 {
    /// Round one component to whole pixels and clamp it into the biased range.
    fn pack_component(value: f32) -> u32 {
        // NaN falls out as the neutral bias (`clamp` keeps ±∞ in range, and a
        // NaN comparison makes both branches of `clamp` false → use 0.0).
        let rounded = if value.is_finite() {
            value.round()
        } else {
            0.0
        };
        let clamped = rounded.clamp(-32_768.0, 32_767.0);
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "clamped to the i16 range just above, and whole after `round`"
        )]
        let signed = clamped as i32;
        u32::try_from(signed.saturating_add(TAG_OFFSET_BIAS)).unwrap_or(0x8000)
    }
    (pack_component(offset.x) << 16) | pack_component(offset.y)
}

/// Unpack a [`MeshTag`] value produced by [`pack_overlap_offset`] back into the
/// billboard-local pixel offset (test-only: the runtime write guard compares
/// packed values directly).
#[cfg(test)]
pub(crate) fn unpack_overlap_offset(packed: u32) -> Vec2 {
    /// Undo the bias on one 16-bit half.
    fn unpack_component(half: u32) -> f32 {
        let biased = i32::try_from(half & 0xFFFF).unwrap_or(TAG_OFFSET_BIAS);
        let unbiased = biased.saturating_sub(TAG_OFFSET_BIAS);
        // The biased half is at most 65535, so the unbiased value fits i16
        // exactly — and i16 → f32 is lossless.
        i16::try_from(unbiased).map_or(0.0, f32::from)
    }
    Vec2::new(unpack_component(packed >> 16), unpack_component(packed))
}

/// The name-tag billboard material: the shared fade/lift params and the glyph
/// atlas page this mesh samples. One material per atlas page, **shared by every
/// tag** using that page and never mutated per frame (per-frame state rides the
/// entity transform and [`MeshTag`] instead — a material write recreates its
/// bind group).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub(crate) struct NameTagMaterial {
    /// `x` = fade-start distance (m), `y` = fade range (m), `z` = base
    /// screen-space lift (px), `w` = unused.
    #[uniform(0)]
    pub(crate) params: Vec4,
    /// The glyph-atlas page (bubble-only meshes bind a 1×1 white fallback).
    #[texture(1)]
    #[sampler(2)]
    pub(crate) atlas: Handle<Image>,
}

impl NameTagMaterial {
    /// Build the material for one atlas page with the given fade settings.
    pub(crate) const fn new(atlas: Handle<Image>, fade_start: f32, fade_range: f32) -> Self {
        Self {
            params: Vec4::new(fade_start, fade_range, 0.0, 0.0),
            atlas,
        }
    }
}

impl Material for NameTagMaterial {
    /// The bundled billboard shader (camera-facing expansion, constant
    /// on-screen size, camera pull, `MeshTag` offset).
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(NAME_TAG_SHADER_HANDLE)
    }

    /// The bundled billboard shader (SDF bubble / atlas glyphs, distance fade).
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(NAME_TAG_SHADER_HANDLE)
    }

    /// Alpha-blended: tags render in the transparent phase — depth-tested
    /// against the world (occluded by geometry in front) without writing depth,
    /// exactly the reference's `LLGLDepthTest(GL_TRUE, GL_FALSE)`.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    /// No depth / normal prepass: the mesh carries position + UVs + colour (no
    /// normals), and a translucent overlay belongs in neither prepass.
    fn enable_prepass() -> bool {
        false
    }

    /// Tags cast no shadows.
    fn enable_shadows() -> bool {
        false
    }

    /// Pin the vertex layout to the shader's `@location`s (position, atlas UV,
    /// SDF UV, colour) and draw both faces — the billboard math in the vertex
    /// shader can wind either way depending on the camera basis.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        // The SL glow pass extracts `scene_rgb * scene.a`, reading the frame's
        // alpha channel as the per-face glow mask (`glow_extract.wgsl`). A
        // blended overlay would otherwise write its text alpha into that channel
        // and bloom — so leave the glow mask untouched, exactly as the other
        // transparent world overlays (parcel borders, particles, prim faces) do.
        sl_client_bevy::preserve_glow_mask_alpha(descriptor);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Text layout: a custom bevy_text root, laid out manually.
// ---------------------------------------------------------------------------

/// Root text component of a name-tag layout block.
///
/// Deliberately **not** [`bevy::sprite::Text2d`]: the stock 2D text systems
/// only lay out entities some 2D camera renders, which is never true for a
/// world-space tag (and would double-compute if it were). [`layout_tag_text`]
/// drives [`TextPipeline`] over these roots instead. The root's own text stays
/// empty — every line lives in a [`TextSpan`] child, so `section_index` `0` is
/// the root and lines are sections `1..`.
#[derive(Component, Debug, Default, Clone)]
#[require(
    TextLayout,
    TextFont,
    TextColor,
    LineHeight,
    LetterSpacing,
    TextBounds,
    FontHinting::Disabled
)]
pub(crate) struct TagText(pub(crate) String);

impl From<String> for TagText {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl TextSection for TagText {
    fn get_text(&self) -> &str {
        &self.0
    }

    fn get_text_mut(&mut self) -> &mut String {
        &mut self.0
    }
}

/// The extra leading, logical px, added to each line's height over its font
/// size — the reference's `LINE_PADDING` between tag lines.
fn tag_line_height(font_size: f32) -> LineHeight {
    LineHeight::Px(font_size + LINE_PADDING_PX)
}

/// Materialise a tag's [`TagContent`] lines as [`TextSpan`] children of its
/// [`TagText`] root: span *i* carries line *i* (with a trailing newline on all
/// but the last), the line tier's font, and the line colour. Existing span
/// entities are updated in place so span count churn only happens when the
/// line count changes; excess spans despawn, missing ones append (appending
/// keeps [`Children`] order = line order).
pub(crate) fn sync_tag_spans(
    mut commands: Commands,
    changed: Query<(Entity, &TagContent, Option<&Children>), Changed<TagContent>>,
    mut spans: Query<(
        &mut TextSpan,
        &mut TextFont,
        &mut TextColor,
        &mut LineHeight,
    )>,
) {
    for (root, content, children) in &changed {
        let last_index = content.lines.len().saturating_sub(1);
        let desired: Vec<(String, f32, Color)> = content
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let mut text = line.text.clone();
                if index < last_index {
                    text.push('\n');
                }
                (text, line.size.font_size_px(), line.color)
            })
            .collect();

        // The root's existing span children, in order (non-span children —
        // none are expected under a tag root — are left alone).
        let existing: Vec<Entity> = children
            .into_iter()
            .flat_map(|c| c.iter())
            .filter(|child| spans.contains(*child))
            .collect();

        let mut desired_lines = desired.into_iter();
        for child in &existing {
            let Some((text, size, color)) = desired_lines.next() else {
                // More spans than lines: despawn the excess.
                commands.entity(*child).try_despawn();
                continue;
            };
            let Ok((mut span, mut font, mut text_color, mut line_height)) = spans.get_mut(*child)
            else {
                continue;
            };
            if span.0 != text {
                span.0 = text;
            }
            let wanted_font = UiFont::Sans.at(size);
            if *font != wanted_font {
                *font = wanted_font;
            }
            if text_color.0 != color {
                text_color.0 = color;
            }
            let wanted_height = tag_line_height(size);
            if *line_height != wanted_height {
                *line_height = wanted_height;
            }
        }
        // More lines than spans: append the remainder (appending keeps
        // `Children` order = line order).
        for (text, size, color) in desired_lines {
            let span = commands
                .spawn((
                    TextSpan(text),
                    UiFont::Sans.at(size),
                    TextColor(color),
                    tag_line_height(size),
                ))
                .id();
            commands.entity(root).add_child(span);
        }
    }
}

/// Lay out every dirty [`TagText`] block through [`TextPipeline`] — a trimmed
/// port of the stock `update_text2d_layout` loop, with the scale factor taken
/// from the **viewer camera** (`With<ViewerCamera>` — the probe cameras make
/// any unqualified camera resolution fail) and no dependence on 2D-camera
/// visibility. Shares the pipeline, font contexts and glyph atlases with
/// bevy_ui, so common glyphs are already rasterised.
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::arithmetic_side_effects,
    reason = "a trimmed port of the stock `update_text2d_layout`: the argument list and \
              query row mirror the original's, and the scale-factor math is finite"
)]
pub(crate) fn layout_tag_text(
    mut last_logical_viewport_size: Local<Vec2>,
    mut reprocess_queue: Local<EntityHashSet>,
    mut textures: ResMut<Assets<Image>>,
    fonts: Res<Assets<bevy::text::Font>>,
    cameras: Query<&Camera, With<crate::camera::ViewerCamera>>,
    mut font_atlas_set: ResMut<FontAtlasSet>,
    mut text_pipeline: ResMut<TextPipeline>,
    mut blocks: Query<(
        Entity,
        Ref<TagText>,
        Ref<TextLayout>,
        Ref<TextBounds>,
        &mut TextLayoutInfo,
        &mut ComputedTextBlock,
        Ref<FontHinting>,
    )>,
    mut text_reader: TextReader<TagText>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    mut scale_cx: ResMut<ScaleCx>,
    rem_size: Res<RemSize>,
    primary_window: Option<Single<&Window, With<PrimaryWindow>>>,
) {
    let logical_viewport_size =
        primary_window.map_or(Vec2::splat(1000.0), |window| window.resolution.size());
    let viewport_size_changed = *last_logical_viewport_size == logical_viewport_size;
    *last_logical_viewport_size = logical_viewport_size;

    let scale_factor = cameras
        .iter()
        .filter_map(Camera::target_scaling_factor)
        .fold(None::<f32>, |best, factor| {
            Some(best.map_or(factor, |b| b.max(factor)))
        })
        .unwrap_or(1.0);

    for (entity, tag_text, block, bounds, mut text_layout_info, mut computed, hinting) in
        &mut blocks
    {
        #[expect(
            clippy::float_cmp,
            reason = "mirrors the stock update_text2d_layout change gate: the stored \
                      scale factor is copied verbatim, so equality is exact"
        )]
        let text_changed = scale_factor != text_layout_info.scale_factor
            || tag_text.is_changed()
            || block.is_changed()
            || computed.needs_rerender(viewport_size_changed, rem_size.is_changed())
            || (!reprocess_queue.is_empty() && reprocess_queue.remove(&entity));

        if !(text_changed || bounds.is_changed() || hinting.is_changed()) {
            continue;
        }

        let text_bounds = TextBounds {
            width: if block.linebreak == LineBreak::NoWrap {
                None
            } else {
                bounds.width.map(|width| width * scale_factor)
            },
            height: bounds.height.map(|height| height * scale_factor),
        };

        if text_changed {
            match text_pipeline.update_buffer(
                &fonts,
                text_reader.iter(entity),
                block.linebreak,
                block.justify,
                text_bounds,
                scale_factor,
                &mut computed,
                &mut font_cx,
                &mut layout_cx,
                logical_viewport_size,
                rem_size.0,
            ) {
                Err(
                    TextError::NoSuchFont
                    | TextError::NoSuchFontFamily(_)
                    | TextError::DegenerateScaleFactor,
                ) => {
                    // The font may simply not have loaded yet — retry next
                    // frame, exactly like the stock system.
                    reprocess_queue.insert(entity);
                    continue;
                }
                Err(error) => {
                    // The stock system panics on the remaining variants; a
                    // name tag is not worth crashing the viewer over.
                    warn!("name-tag text buffer update failed: {error}");
                    text_layout_info.clear();
                    continue;
                }
                Ok(()) => {}
            }
        }

        match text_pipeline.update_text_layout_info(
            &mut text_layout_info,
            &mut font_atlas_set,
            &mut textures,
            &mut computed,
            &mut scale_cx,
            text_bounds,
            block.justify,
            *hinting,
        ) {
            Err(TextError::NoSuchFont | TextError::NoSuchFontFamily(_)) => {
                reprocess_queue.insert(entity);
            }
            Err(error) => {
                warn!("name-tag text layout failed: {error}");
                text_layout_info.clear();
            }
            Ok(()) => {
                text_layout_info.scale_factor = scale_factor;
                // Unlike the stock system (whose consumers want logical px),
                // the mesh builder works in physical px and reads
                // `size * scale_factor`; keep the stock convention anyway so
                // the two systems agree on the stored value's meaning.
                text_layout_info.size *= scale_factor.recip();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mesh building: bubble + drop shadows + glyph quads, grouped by atlas page.
// ---------------------------------------------------------------------------

/// The camera-pull distance, metres, baked into a tag's mesh (`position.z`):
/// the reference pushes a tag toward the camera by the source object's radius
/// (`mSourceObject->getVObjRadius()`) so the avatar's own body cannot occlude
/// its tag. Set at spawn from the avatar representation's rough radius.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTagPullRadius(pub(crate) f32);

impl Default for NameTagPullRadius {
    fn default() -> Self {
        // Half a metre — roughly half a shoulder-to-shoulder box; the exact
        // value only needs to beat the head's radius at the tag's height.
        Self(0.5)
    }
}

/// The tag bubble's current on-screen size in **physical pixels**, recorded by
/// [`build_tag_meshes`]; the anti-overlap solver and the cursor hit test both
/// read it (a tag renders at constant on-screen size, so this is valid at any
/// distance).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct NameTagPixelSize(pub(crate) Vec2);

/// Marker on a spawned extra atlas-page child of a tag (page 0 rides the tag
/// entity itself; a tag whose glyphs span several atlas pages gets one child
/// per further page).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTagPage;

/// The extra camera pull, metres, per atlas page beyond the first — a
/// deterministic transparent-phase tie-break so a multi-page tag's pages
/// cannot flicker against each other (they share one world position).
const PAGE_PULL_STEP_METRES: f32 = 0.001;

/// The reference's glyph drop shadow: one atlas-sampled copy per glyph, offset
/// this many **logical** px down-right and coloured black
/// (`LLFontGL::DROP_SHADOW`).
const SHADOW_OFFSET_PX: Vec2 = Vec2::new(1.0, -1.0);

/// The reference's tag backdrop colour: `NameTagBackground` black; the alpha
/// is the `ChatBubbleOpacity` setting (default 0.5) carried in
/// [`NameTagMaterials::bubble_opacity`].
const BUBBLE_COLOR: Color = Color::BLACK;

/// The shared per-atlas-page tag materials and the current tag appearance
/// parameters. Materials are created once per atlas page and **never mutated
/// per frame** — mutating a material recreates its bind group. The settings
/// system rewrites [`NameTagMaterial::params`] (and bumps layouts for
/// opacity changes) only when a preference actually changes.
#[derive(Resource, Debug)]
pub(crate) struct NameTagMaterials {
    /// One shared material per glyph-atlas page (keyed by the atlas image).
    by_atlas: bevy::platform::collections::HashMap<AssetId<Image>, Handle<NameTagMaterial>>,
    /// Distance at which tags start fading, metres.
    pub(crate) fade_start: f32,
    /// Metres past the fade start at which tags are fully gone.
    pub(crate) fade_range: f32,
    /// The bubble backdrop's alpha (the reference's `ChatBubbleOpacity`).
    pub(crate) bubble_opacity: f32,
}

impl Default for NameTagMaterials {
    fn default() -> Self {
        Self::with_fade(DEFAULT_FADE_START_METRES, DEFAULT_FADE_RANGE_METRES, 0.5)
    }
}

/// The floating object-text (`llSetText`) materials — the same per-atlas
/// billboard materials as the name tags, but a **separate** registry so hover
/// text carries its own (shorter) fade distances (`LLHUDText`'s 8 m / 4 m,
/// against the name-tag 20 m / 5 m). Bare text draws no bubble, so the opacity
/// is unused. See [`crate::hover_text`].
#[derive(Resource, Debug)]
pub(crate) struct HoverTextMaterials(pub(crate) NameTagMaterials);

impl Default for HoverTextMaterials {
    fn default() -> Self {
        Self(NameTagMaterials::with_fade(
            crate::hover_text::DEFAULT_HOVER_FADE_START_METRES,
            crate::hover_text::DEFAULT_HOVER_FADE_RANGE_METRES,
            0.0,
        ))
    }
}

/// How a world-anchored text entity's mesh is built and which fade registry it
/// samples: avatar name tags draw a chat-bubble backdrop lifted 25 px above the
/// anchor and fade at name-tag range; object floating text draws bare text at
/// the anchor and fades at the shorter `LLHUDText` range. A tag entity without
/// this component is treated as a name tag (the historical default).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WorldTextStyle {
    /// Whether to draw the rounded-rect bubble backdrop.
    pub(crate) draw_bubble: bool,
    /// The screen-space lift above the anchor, **logical** px.
    pub(crate) lift_px: f32,
    /// Sample the [`HoverTextMaterials`] registry (shorter fade) instead of the
    /// name-tag one.
    pub(crate) use_hover_registry: bool,
}

impl WorldTextStyle {
    /// The avatar name-tag style: bubble on, lifted 25 px, name-tag fade.
    pub(crate) const NAME_TAG: Self = Self {
        draw_bubble: true,
        lift_px: BASE_LIFT_PX,
        use_hover_registry: false,
    };

    /// The object floating-text style: bare text at the anchor, `LLHUDText` fade.
    pub(crate) const HOVER_TEXT: Self = Self {
        draw_bubble: false,
        lift_px: 0.0,
        use_hover_registry: true,
    };
}

impl NameTagMaterials {
    /// An empty registry with the given fade distances and bubble opacity.
    pub(crate) fn with_fade(fade_start: f32, fade_range: f32, bubble_opacity: f32) -> Self {
        Self {
            by_atlas: bevy::platform::collections::HashMap::default(),
            fade_start,
            fade_range,
            bubble_opacity,
        }
    }

    /// The shared material for one atlas page, created on first use. The
    /// default [`Handle<Image>`] (bevy's built-in 1×1 white) serves the
    /// bubble-only mesh of a tag with no glyphs.
    fn material_for(
        &mut self,
        atlas: AssetId<Image>,
        images: &mut Assets<Image>,
        materials: &mut Assets<NameTagMaterial>,
    ) -> Handle<NameTagMaterial> {
        if let Some(handle) = self.by_atlas.get(&atlas) {
            return handle.clone();
        }
        let atlas_handle = images.get_strong_handle(atlas).unwrap_or_default();
        let handle = materials.add(NameTagMaterial::new(
            atlas_handle,
            self.fade_start,
            self.fade_range,
        ));
        self.by_atlas.insert(atlas, handle.clone());
        handle
    }

    /// Every live per-atlas material (for the settings updater).
    pub(crate) fn handles(&self) -> impl Iterator<Item = &Handle<NameTagMaterial>> {
        self.by_atlas.values()
    }
}

/// One laid-out glyph, pre-resolved for mesh building: everything the pure
/// geometry pass needs, with asset lookups already done.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphQuadInput {
    /// The glyph quad's centre in layout space (physical px, top-left origin,
    /// +y **down** — the bevy_text convention).
    pub(crate) center: Vec2,
    /// The glyph quad's size, physical px.
    pub(crate) size: Vec2,
    /// Atlas UV rectangle, already normalised.
    pub(crate) uv_min: Vec2,
    /// See [`Self::uv_min`].
    pub(crate) uv_max: Vec2,
    /// Which atlas page (index into the tag's page list) the glyph samples.
    pub(crate) page: usize,
    /// The glyph's tint: the line colour for alpha-mask glyphs, white for
    /// colour (emoji) glyphs — atlas pages store white + alpha for masks, so
    /// `sample × tint` is correct for both.
    pub(crate) color: LinearRgba,
}

/// The geometry of one page mesh: parallel vertex arrays + triangle indices,
/// ready to pour into a [`Mesh`].
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct TagPageGeometry {
    /// Vertex positions (tag-local px ÷ 1024; z = camera pull, metres).
    pub(crate) positions: Vec<[f32; 3]>,
    /// Atlas UVs (glyphs) or the negative half-extent sentinel (bubble).
    pub(crate) uvs0: Vec<[f32; 2]>,
    /// Bubble SDF corner offsets (px); zero on glyph vertices.
    pub(crate) uvs1: Vec<[f32; 2]>,
    /// Linear straight-alpha vertex colours.
    pub(crate) colors: Vec<[f32; 4]>,
    /// Triangle list indices.
    pub(crate) indices: Vec<u32>,
}

impl TagPageGeometry {
    /// Append one camera-facing quad.
    ///
    /// `center`/`half` are tag-local px (+y up); `uv0` is per-corner when
    /// sampling the atlas (`[bl, br, tr, tl]`) or a constant sentinel for the
    /// bubble; `uv1` likewise per-corner SDF offsets or zeroes.
    fn push_quad(
        &mut self,
        center: Vec2,
        half: Vec2,
        z: f32,
        uv0: [[f32; 2]; 4],
        uv1: [[f32; 2]; 4],
        color: [f32; 4],
    ) {
        let base = u32::try_from(self.positions.len()).unwrap_or(0);
        let corners = [
            Vec2::new(center.x - half.x, center.y - half.y),
            Vec2::new(center.x + half.x, center.y - half.y),
            Vec2::new(center.x + half.x, center.y + half.y),
            Vec2::new(center.x - half.x, center.y + half.y),
        ];
        for (corner, (uv0_corner, uv1_corner)) in corners.into_iter().zip(uv0.into_iter().zip(uv1))
        {
            self.positions.push([
                corner.x * TAG_UNITS_PER_PIXEL,
                corner.y * TAG_UNITS_PER_PIXEL,
                z,
            ]);
            self.uvs0.push(uv0_corner);
            self.uvs1.push(uv1_corner);
            self.colors.push(color);
        }
        self.indices.extend_from_slice(&[
            base,
            base.saturating_add(1),
            base.saturating_add(2),
            base.saturating_add(2),
            base.saturating_add(3),
            base,
        ]);
    }
}

/// The built geometry of a whole tag: one entry per atlas page (page 0 also
/// carries the bubble), plus the bubble's physical pixel size.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct TagMeshData {
    /// Per-page geometry, page 0 first.
    pub(crate) pages: Vec<TagPageGeometry>,
    /// The bubble size, physical px (the tag's on-screen footprint).
    pub(crate) bubble_size: Vec2,
}

/// Build a tag's mesh geometry from its resolved glyph quads.
///
/// Pure — no ECS, no assets — so the quad accounting is unit-testable. The
/// glyph bounding box (not the layout's reported size) centres the text in the
/// bubble, which makes the result independent of how the text was justified
/// inside its wrapping bounds. Painter's order inside each page (depth write
/// is off): bubble, then every drop shadow, then every glyph.
///
/// The whole tag is lifted `lift_px` **physical** px up from the anchor (the
/// reference's 25 px screen offset), bottom-anchored: local y spans
/// `[lift_px, lift_px + bubble height]`. The resulting off-origin AABB centre
/// shifts the transparent-phase sort point by well under a metre (px ÷ 1024),
/// which is negligible for ordering.
///
/// `draw_bubble` toggles the rounded-rect backdrop quad: avatar name tags draw
/// it, but object floating text ([`crate::hover_text`]) has no background in the
/// reference (`LLHUDText` renders bare text + drop shadow), so it passes `false`.
/// The bubble-sized padding is kept either way, so the text keeps its small
/// breathing room and the bottom-anchored geometry is identical.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "finite pixel-space layout geometry; the glam / float operators are the \
              readable form"
)]
pub(crate) fn build_tag_mesh_data(
    glyphs: &[GlyphQuadInput],
    pull_radius: f32,
    scale_factor: f32,
    bubble_opacity: f32,
    lift_px: f32,
    draw_bubble: bool,
) -> TagMeshData {
    // No glyphs → no tag at all (not even the bubble): an empty page keeps the
    // mesh valid while the composer has nothing to show.
    if glyphs.is_empty() {
        return TagMeshData {
            pages: vec![TagPageGeometry::default()],
            bubble_size: Vec2::ZERO,
        };
    }
    let page_count = glyphs
        .iter()
        .map(|glyph| glyph.page.saturating_add(1))
        .max()
        .unwrap_or(1);

    // The glyph bounding box in layout space (+y down).
    let mut bbox_min = Vec2::splat(f32::INFINITY);
    let mut bbox_max = Vec2::splat(f32::NEG_INFINITY);
    for glyph in glyphs {
        let half = glyph.size * 0.5;
        bbox_min = bbox_min.min(glyph.center - half);
        bbox_max = bbox_max.max(glyph.center + half);
    }
    let bbox_center = (bbox_min + bbox_max) * 0.5;
    let text_size = bbox_max - bbox_min;

    let padding = Vec2::new(HORIZONTAL_PADDING_PX, VERTICAL_PADDING_PX) * scale_factor;
    let bubble_size = text_size + padding;
    let bubble_half = bubble_size * 0.5;

    let mut pages: Vec<TagPageGeometry> = vec![TagPageGeometry::default(); page_count];

    // The bubble's centre in tag-local space: bottom-anchored `lift_px` above
    // the anchor point.
    let bubble_center = Vec2::new(0.0, lift_px + bubble_half.y);

    // Bubble, on page 0: constant negative-half-extent sentinel in UV0, the
    // corner offsets (the SDF sample points) in UV1. Skipped for bare object
    // floating text, which has no backdrop.
    if let Some(page0) = pages.first_mut()
        && draw_bubble
    {
        let sentinel = [-bubble_half.x, -bubble_half.y];
        let corner_offsets = [
            [-bubble_half.x, -bubble_half.y],
            [bubble_half.x, -bubble_half.y],
            [bubble_half.x, bubble_half.y],
            [-bubble_half.x, bubble_half.y],
        ];
        let mut bubble = BUBBLE_COLOR.to_linear();
        bubble.alpha = bubble_opacity;
        page0.push_quad(
            bubble_center,
            bubble_half,
            pull_radius,
            [sentinel; 4],
            corner_offsets,
            bubble.to_f32_array(),
        );
    }

    // Layout space (+y down, top-left origin) → tag-local (+y up, bubble
    // centre at `bubble_center`).
    let to_local = |layout: Vec2| {
        Vec2::new(
            layout.x - bbox_center.x + bubble_center.x,
            bbox_center.y - layout.y + bubble_center.y,
        )
    };

    let shadow_offset = SHADOW_OFFSET_PX * scale_factor;
    for pass in [TagQuadPass::Shadow, TagQuadPass::Glyph] {
        for glyph in glyphs {
            let Some(page) = pages.get_mut(glyph.page) else {
                continue;
            };
            let z = pull_radius
                + PAGE_PULL_STEP_METRES * u16::try_from(glyph.page).map_or(0.0, f32::from);
            let uv0 = [
                [glyph.uv_min.x, glyph.uv_max.y],
                [glyph.uv_max.x, glyph.uv_max.y],
                [glyph.uv_max.x, glyph.uv_min.y],
                [glyph.uv_min.x, glyph.uv_min.y],
            ];
            let (center, color) = match pass {
                TagQuadPass::Shadow => (
                    to_local(glyph.center) + shadow_offset,
                    [0.0, 0.0, 0.0, glyph.color.alpha],
                ),
                TagQuadPass::Glyph => (to_local(glyph.center), glyph.color.to_f32_array()),
            };
            page.push_quad(center, glyph.size * 0.5, z, uv0, [[0.0, 0.0]; 4], color);
        }
    }

    TagMeshData { pages, bubble_size }
}

/// The two glyph passes of [`build_tag_mesh_data`], in paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagQuadPass {
    /// The black offset copy under every glyph.
    Shadow,
    /// The tinted glyph itself.
    Glyph,
}

/// Pour one page's geometry into a mesh asset.
fn write_page_mesh(mesh: &mut Mesh, geometry: TagPageGeometry) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, geometry.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, geometry.uvs0);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, geometry.uvs1);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, geometry.colors);
    mesh.insert_indices(bevy::mesh::Indices::U32(geometry.indices));
}

/// An empty tag mesh (a tag with no content yet).
fn empty_tag_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    write_page_mesh(&mut mesh, TagPageGeometry::default());
    mesh
}

/// Rebuild the mesh(es) of every tag whose text layout changed: resolve each
/// glyph's atlas page, UVs and section colour, run [`build_tag_mesh_data`],
/// and pour the result into the tag's mesh (page 0) and its [`NameTagPage`]
/// children (further pages — rare, e.g. emoji in a name). Also records the
/// bubble's physical size in [`NameTagPixelSize`].
#[expect(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::arithmetic_side_effects,
    reason = "the mesh rebuild is the single fan-in of layout, atlas, material and page \
              state, and its UV / pull math is finite pixel-space geometry"
)]
pub(crate) fn build_tag_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<NameTagMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut registry: ResMut<NameTagMaterials>,
    mut hover_registry: ResMut<HoverTextMaterials>,
    mut changed: Query<
        (
            Entity,
            &TextLayoutInfo,
            &ComputedTextBlock,
            &NameTagPullRadius,
            &mut NameTagPixelSize,
            Option<&Mesh3d>,
            Option<&Children>,
            Option<&WorldTextStyle>,
        ),
        (Changed<TextLayoutInfo>, With<TagText>),
    >,
    colors: Query<&TextColor>,
    pages: Query<(), With<NameTagPage>>,
) {
    for (entity, layout, computed, pull_radius, mut pixel_size, mesh3d, children, style) in
        &mut changed
    {
        // Absent style = name tag (the historical default).
        let style = style.copied().unwrap_or(WorldTextStyle::NAME_TAG);
        // Select the fade registry this entity samples; a bare `&mut` to the
        // chosen `ResMut`'s inner value, so `material_for` / `bubble_opacity`
        // read through one binding below.
        let registry: &mut NameTagMaterials = if style.use_hover_registry {
            &mut hover_registry.0
        } else {
            &mut registry
        };
        let scale_factor = if layout.scale_factor > 0.0 {
            layout.scale_factor
        } else {
            1.0
        };

        // Resolve glyphs → per-page quad inputs (atlas pages keyed in order of
        // first appearance; page 0 always exists for the bubble).
        let mut atlas_pages: Vec<AssetId<Image>> = Vec::new();
        let mut quads: Vec<GlyphQuadInput> = Vec::new();
        for glyph in &layout.glyphs {
            let atlas = glyph.atlas_info.texture;
            let Some(image) = images.get(atlas) else {
                continue;
            };
            let atlas_size = image.size_f32();
            if atlas_size.x <= 0.0 || atlas_size.y <= 0.0 {
                continue;
            }
            let page = atlas_pages
                .iter()
                .position(|known| *known == atlas)
                .unwrap_or_else(|| {
                    atlas_pages.push(atlas);
                    atlas_pages.len().saturating_sub(1)
                });
            let line_color = computed
                .entities()
                .get(glyph.section_index)
                .and_then(|section| colors.get(section.entity).ok())
                .map_or(Color::WHITE, |text_color| text_color.0);
            let color = if glyph.atlas_info.is_alpha_mask {
                line_color.to_linear()
            } else {
                LinearRgba::WHITE
            };
            quads.push(GlyphQuadInput {
                center: glyph.position,
                size: glyph.atlas_info.rect.size(),
                uv_min: glyph.atlas_info.rect.min / atlas_size,
                uv_max: glyph.atlas_info.rect.max / atlas_size,
                page,
                color,
            });
        }

        let data = build_tag_mesh_data(
            &quads,
            pull_radius.0,
            scale_factor,
            registry.bubble_opacity,
            style.lift_px * scale_factor,
            style.draw_bubble,
        );
        pixel_size.set_if_neq(NameTagPixelSize(data.bubble_size));

        // Page 0 rides the tag entity.
        let mut page_geometries = data.pages.into_iter();
        let page0 = page_geometries.next().unwrap_or_default();
        let page0_atlas = atlas_pages.first().copied().unwrap_or_default();
        let page0_material = registry.material_for(page0_atlas, &mut images, &mut materials);
        if let Some(handle) = mesh3d {
            if let Some(mut mesh) = meshes.get_mut(handle) {
                write_page_mesh(&mut mesh, page0);
            }
        } else {
            let mut mesh = empty_tag_mesh();
            write_page_mesh(&mut mesh, page0);
            commands.entity(entity).insert(Mesh3d(meshes.add(mesh)));
        }
        commands
            .entity(entity)
            .insert(MeshMaterial3d(page0_material));

        // Existing page children, in order.
        let existing: Vec<Entity> = children
            .into_iter()
            .flat_map(|c| c.iter())
            .filter(|child| pages.contains(*child))
            .collect();
        let mut extra_pages = page_geometries.enumerate();
        let mut existing_iter = existing.into_iter();
        loop {
            match (existing_iter.next(), extra_pages.next()) {
                (Some(child), Some((index, geometry))) => {
                    let atlas = atlas_pages
                        .get(index.saturating_add(1))
                        .copied()
                        .unwrap_or_default();
                    let material = registry.material_for(atlas, &mut images, &mut materials);
                    let mut mesh = empty_tag_mesh();
                    write_page_mesh(&mut mesh, geometry);
                    commands
                        .entity(child)
                        .insert((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
                }
                (Some(child), None) => {
                    commands.entity(child).try_despawn();
                }
                (None, Some((index, geometry))) => {
                    let atlas = atlas_pages
                        .get(index.saturating_add(1))
                        .copied()
                        .unwrap_or_default();
                    let material = registry.material_for(atlas, &mut images, &mut materials);
                    let mut mesh = empty_tag_mesh();
                    write_page_mesh(&mut mesh, geometry);
                    let page = commands
                        .spawn((
                            NameTagPage,
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(material),
                            Transform::IDENTITY,
                            Visibility::Inherited,
                            // Without a neutral MeshTag the shader would
                            // unpack tag 0 as a −32768 px offset and draw the
                            // page far off-screen. (Different font sizes use
                            // different atlas pages, so multi-page tags are
                            // the NORM for any tag with small + name lines.)
                            MeshTag(NEUTRAL_MESH_TAG),
                            bevy::camera::visibility::NoFrustumCulling,
                            tag_render_layers(),
                        ))
                        .id();
                    commands.entity(entity).add_child(page);
                }
                (None, None) => break,
            }
        }
    }
}

/// Apply the name-tag appearance settings when they change: the fade
/// distances rewrite every live [`NameTagMaterial`]'s params (a bind-group
/// recreation per material — fine for a settings change, forbidden per
/// frame), and a bubble-opacity change forces every tag's mesh to rebuild
/// (the opacity rides the bubble's vertex colour) by re-marking its layout
/// changed.
#[expect(
    clippy::float_cmp,
    reason = "the registry stores verbatim copies of the setting values, so exact \
              equality is the correct change test"
)]
pub(crate) fn apply_name_tag_settings(
    settings: Option<Res<crate::settings::ViewerSettings>>,
    mut registry: ResMut<NameTagMaterials>,
    mut materials: ResMut<Assets<NameTagMaterial>>,
    mut layouts: Query<&mut TextLayoutInfo, With<TagText>>,
) {
    let Some(settings) = settings else {
        return;
    };
    if !settings.is_changed() {
        return;
    }
    let store = settings.store();
    let fade_start = store
        .get_f32(SETTING_FADE_START)
        .unwrap_or(DEFAULT_FADE_START_METRES);
    let fade_range = store
        .get_f32(SETTING_FADE_RANGE)
        .unwrap_or(DEFAULT_FADE_RANGE_METRES);
    let bubble_opacity = store.get_f32(SETTING_BUBBLE_OPACITY).unwrap_or(0.5);

    if fade_start != registry.fade_start || fade_range != registry.fade_range {
        registry.fade_start = fade_start;
        registry.fade_range = fade_range;
        let handles: Vec<Handle<NameTagMaterial>> = registry.handles().cloned().collect();
        for handle in handles {
            if let Some(mut material) = materials.get_mut(&handle) {
                material.params = Vec4::new(fade_start, fade_range, 0.0, 0.0);
            }
        }
    }
    if bubble_opacity != registry.bubble_opacity {
        registry.bubble_opacity = bubble_opacity;
        for mut layout in &mut layouts {
            layout.set_changed();
        }
    }
}

// ---------------------------------------------------------------------------
// Placement: anchor following, smoothing, cutoff, screen rect, hit test.
// ---------------------------------------------------------------------------

/// The dead band, metres, before a tag re-targets to a moved anchor point (the
/// reference's `NAMETAG_UPDATE_THRESHOLD`): small head bobbing does not drag
/// the tag around.
const RETARGET_DEAD_BAND_METRES: f32 = 0.3;

/// The tighter **vertical** dead band, metres: the tag's float height gets
/// corrected (posed-head fitting) in steps smaller than the overall dead
/// band, and those corrections must land — while ordinary head-bob stays
/// below this.
const RETARGET_VERTICAL_DEAD_BAND_METRES: f32 = 0.1;

/// The exponential smoothing time constant, seconds, for a tag easing toward
/// its (dead-banded) target — the reference's `LLSmoothInterpolation` 0.2 s.
const SMOOTHING_TIME_CONSTANT_SECS: f32 = 0.2;

/// Below this remaining distance, metres, the easing snaps to its target so a
/// settled tag stops writing its transform every frame.
const SMOOTHING_SNAP_METRES: f32 = 0.002;

/// Per-tag anchor-follow smoothing state (the reference's smoothed
/// root-to-head offset): the dead-banded target and the eased position last
/// written to the tag's [`Transform`].
#[derive(Component, Debug, Default, Clone, Copy)]
pub(crate) struct NameTagSmooth {
    /// The dead-banded target the tag is easing toward.
    target: Option<Vec3>,
    /// The eased position last written (`None` until first placement, which
    /// snaps).
    current: Option<Vec3>,
}

/// The tag's current on-screen bubble rectangle (logical px, window top-left
/// origin — [`Window::cursor_position`] space) and its camera distance.
/// Filled by [`follow_tag_anchors`]; the anti-overlap solver shifts it and the
/// cursor hit test ([`NameTagHitTest`]) reads it.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct NameTagScreenRect {
    /// The bubble rect, or `None` while the tag projects off-screen.
    pub(crate) rect: Option<Rect>,
    /// Camera→tag distance, metres (front-most-wins ordering for the hit
    /// test).
    pub(crate) camera_distance: f32,
}

/// The renderer-side components of one tag entity, composed by
/// `AvatarState::spawn_label` alongside the avatar-side identity components
/// (`NameTag`, `AvatarPickTarget`, `TagContent`).
pub(crate) fn name_tag_render_bundle(pull_radius: f32) -> impl Bundle {
    (
        TagText::default(),
        TextLayout {
            justify: Justify::Center,
            linebreak: LineBreak::WordOrCharacter,
        },
        // The reference wraps tag text at 298 px (`NAMETAG_MAX_WIDTH`).
        TextBounds {
            width: Some(MAX_TAG_WIDTH_PX),
            height: None,
        },
        NameTagPullRadius(pull_radius),
        NameTagPixelSize::default(),
        NameTagScreenRect::default(),
        NameTagSmooth::default(),
        NameTagOverlapOffset::default(),
        bevy::mesh::MeshTag(NEUTRAL_MESH_TAG),
        Transform::default(),
        // Hidden until the first placement so it never flashes at the origin.
        Visibility::Hidden,
        // Tag-local units are px ÷ 1024, not world geometry — the mesh AABB
        // is meaningless for culling, and the CPU distance cutoff already
        // hides far tags.
        bevy::camera::visibility::NoFrustumCulling,
        tag_render_layers(),
    )
}

/// Follow each tag's avatar anchor: dead-banded, exponentially-smoothed world
/// placement (`Transform`), the distance cutoff and preference gates
/// (`Visibility`), and the projected on-screen bubble rect the overlap solver
/// and hit test consume.
///
/// Runs in `PostUpdate` before transform propagation, reading the anchor
/// root's **`Transform`** (anchor roots are top-level entities, so their local
/// transform *is* their world pose — and unlike `GlobalTransform` it is
/// current this frame).
#[expect(
    clippy::type_complexity,
    clippy::arithmetic_side_effects,
    reason = "the per-tag query row is the placement state in one fetch, and the \
              smoothing / projection math is finite metre- and pixel-space geometry"
)]
pub(crate) fn follow_tag_anchors(
    time: Res<Time>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::ViewerCamera>>,
    anchors: Query<&Transform, (With<AvatarAnchor>, Without<NameTag>)>,
    mut tags: Query<
        (
            &NameTag,
            Option<&AvatarPickTarget>,
            &NameTagPixelSize,
            &mut NameTagSmooth,
            &mut Transform,
            &mut Visibility,
            &mut NameTagScreenRect,
        ),
        Without<AvatarAnchor>,
    >,
    registry: Option<Res<NameTagMaterials>>,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    identity: Option<Res<SlIdentity>>,
) {
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    // The preferences gates (optional resources, so a headless test's bare
    // world runs ungated): a store without the keys means "shown".
    let show_tags = settings
        .as_ref()
        .and_then(|settings| {
            settings
                .store()
                .get_bool(crate::avatars::SETTING_SHOW_NAME_TAGS)
                .ok()
        })
        .unwrap_or(true);
    let show_own = settings
        .as_ref()
        .and_then(|settings| {
            settings
                .store()
                .get_bool(crate::avatars::SETTING_SHOW_OWN_NAME_TAG)
                .ok()
        })
        .unwrap_or(true);
    let own_agent = identity.as_ref().and_then(|identity| identity.agent_id);
    let (fade_start, fade_range) = registry.as_ref().map_or(
        (DEFAULT_FADE_START_METRES, DEFAULT_FADE_RANGE_METRES),
        |r| (r.fade_start, r.fade_range),
    );
    let hide_beyond = fade_start + fade_range;
    let camera_position = camera_transform.translation();
    let scale_factor = camera.target_scaling_factor().unwrap_or(1.0);
    // Frame-rate-independent exponential approach factor for the smoothing
    // time constant (`1 − e^(−dt/tc)`).
    let approach = 1.0 - (-time.delta_secs() / SMOOTHING_TIME_CONSTANT_SECS).exp();

    for (tag, pick, pixel_size, mut smooth, mut transform, mut visibility, mut screen_rect) in
        &mut tags
    {
        let is_own = own_agent.is_some() && pick.map(AvatarPickTarget::agent) == own_agent;
        if !show_tags || (is_own && !show_own) {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let Ok(anchor) = anchors.get(tag.anchor) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let base = anchor.translation;
        // Float the tag above the avatar (per-component add to avoid the
        // `arithmetic_side_effects` lint on the glam `Vec3` operator).
        let head = Vec3::new(base.x, base.y + tag.tag_height, base.z);

        // Dead band, then exponential approach (snap on first placement and
        // when settled, so a stationary avatar writes nothing). The vertical
        // axis uses a tighter band so head-height corrections land.
        let target = match smooth.target {
            Some(previous)
                if previous.distance(head) <= RETARGET_DEAD_BAND_METRES
                    && (previous.y - head.y).abs() <= RETARGET_VERTICAL_DEAD_BAND_METRES =>
            {
                previous
            }
            _ => {
                smooth.target = Some(head);
                head
            }
        };
        let position = match smooth.current {
            None => target,
            Some(current) if current.distance(target) <= SMOOTHING_SNAP_METRES => {
                if current == target { current } else { target }
            }
            Some(current) => current.lerp(target, approach.clamp(0.0, 1.0)),
        };
        if smooth.current != Some(position) {
            smooth.current = Some(position);
        }
        if transform.translation != position {
            transform.translation = position;
        }

        // Distance cutoff: fully faded tags stop rendering at all (the
        // reference culls at fade distance + range).
        let camera_distance = camera_position.distance(position);
        if camera_distance > hide_beyond {
            visibility.set_if_neq(Visibility::Hidden);
            screen_rect.set_if_neq(NameTagScreenRect {
                rect: None,
                camera_distance,
            });
            continue;
        }
        visibility.set_if_neq(Visibility::Inherited);

        // The bubble's on-screen rect (logical px): bottom-centred on the
        // projected anchor, lifted by the baked screen offset.
        let rect = camera
            .world_to_viewport(camera_transform, position)
            .ok()
            .map(|projected| {
                let size = pixel_size.0 / scale_factor.max(f32::EPSILON);
                let bottom = projected.y - BASE_LIFT_PX;
                Rect::new(
                    projected.x - size.x * 0.5,
                    bottom - size.y,
                    projected.x + size.x * 0.5,
                    bottom,
                )
            });
        screen_rect.set_if_neq(NameTagScreenRect {
            rect,
            camera_distance,
        });
    }
}

// ---------------------------------------------------------------------------
// Anti-overlap: bounded screen-space spring separation.
// ---------------------------------------------------------------------------

/// The reference's soft-rect padding, logical px, around each tag when testing
/// overlap (`llhudnametag.cpp` `BUFFER_SIZE`).
const OVERLAP_BUFFER_PX: f32 = 2.0;

/// Spring iterations per frame (`NUM_OVERLAP_ITERATIONS`).
const OVERLAP_ITERATIONS: usize = 10;

/// The fraction of each remaining overlap corrected per iteration
/// (`SPRING_STRENGTH`).
const OVERLAP_SPRING_STRENGTH: f32 = 0.7;

/// The camera speed, m/s, above which the solve freezes (the reference's
/// `MAX_STABLE_CAMERA_VELOCITY`): while the view is flying, rects churn every
/// frame and separating them just makes tags dance.
const OVERLAP_FREEZE_CAMERA_SPEED: f32 = 0.1;

/// The maximum anti-overlap displacement, in tag heights (the **user's**
/// bound, deliberately not in the reference: a separated tag must stay
/// recognisably over its avatar, never drift far away).
const MAX_OVERLAP_DISPLACEMENT_TAG_HEIGHTS: f32 = 1.5;

/// Below this remaining screen distance, logical px, the offset smoothing
/// snaps to its target so settled tags stop writing.
const OVERLAP_SNAP_PX: f32 = 0.5;

/// Per-tag anti-overlap state: the solved target offset and the smoothed
/// offset actually applied (both logical px, viewport axes — +y down).
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct NameTagOverlapOffset {
    /// This frame's solved separation offset.
    target: Vec2,
    /// The smoothed offset last applied (eases toward [`Self::target`]).
    current: Vec2,
}

/// Solve screen-space separation offsets for a set of (already buffered) tag
/// rects: a bounded port of the reference's spring de-overlap. Pure and
/// deterministic — rects are processed in the caller's (sorted) order, from
/// zero offsets each call, so isolated tags always resolve to zero and the
/// result is independent of history.
///
/// Each entry is `(rect, max_offset)`; the result is one offset per entry
/// (viewport axes, +y down), each clamped to its `max_offset` length.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "finite screen-space rect geometry; the float operators are the readable form"
)]
pub(crate) fn solve_overlap_offsets(tags: &[(Rect, f32)]) -> Vec<Vec2> {
    let mut offsets = vec![Vec2::ZERO; tags.len()];
    if tags.len() < 2 {
        return offsets;
    }
    for _ in 0..OVERLAP_ITERATIONS {
        let mut any = false;
        for first in 0..tags.len() {
            for second in first.saturating_add(1)..tags.len() {
                let (Some((rect_a, max_a)), Some((rect_b, max_b))) =
                    (tags.get(first), tags.get(second))
                else {
                    continue;
                };
                let (Some(off_a), Some(off_b)) =
                    (offsets.get(first).copied(), offsets.get(second).copied())
                else {
                    continue;
                };
                let a = Rect {
                    min: rect_a.min + off_a,
                    max: rect_a.max + off_a,
                };
                let b = Rect {
                    min: rect_b.min + off_b,
                    max: rect_b.max + off_b,
                };
                let overlap_x = a.max.x.min(b.max.x) - a.min.x.max(b.min.x);
                let overlap_y = a.max.y.min(b.max.y) - a.min.y.max(b.min.y);
                if overlap_x <= 0.0 || overlap_y <= 0.0 {
                    continue;
                }
                any = true;
                // Separate along the axis of least penetration; bigger tags
                // move less (area-weighted, the reference's mass weighting).
                let area_a = (a.max.x - a.min.x).max(1.0) * (a.max.y - a.min.y).max(1.0);
                let area_b = (b.max.x - b.min.x).max(1.0) * (b.max.y - b.min.y).max(1.0);
                let weight_a = area_b / (area_a + area_b);
                let weight_b = 1.0 - weight_a;
                let correction = OVERLAP_SPRING_STRENGTH
                    * if overlap_x < overlap_y {
                        overlap_x
                    } else {
                        overlap_y
                    };
                let direction = if overlap_x < overlap_y {
                    // Horizontal separation, by centre order (ties: `first`
                    // goes left).
                    let sign = if a.center().x <= b.center().x {
                        -1.0
                    } else {
                        1.0
                    };
                    Vec2::new(sign, 0.0)
                } else {
                    // Vertical separation (ties: `first` goes up — screen
                    // −y).
                    let sign = if a.center().y <= b.center().y {
                        -1.0
                    } else {
                        1.0
                    };
                    Vec2::new(0.0, sign)
                };
                let moved_a = off_a + direction * (correction * weight_a);
                let moved_b = off_b - direction * (correction * weight_b);
                if let Some(slot) = offsets.get_mut(first) {
                    *slot = moved_a.clamp_length_max(*max_a);
                }
                if let Some(slot) = offsets.get_mut(second) {
                    *slot = moved_b.clamp_length_max(*max_b);
                }
            }
        }
        if !any {
            break;
        }
    }
    offsets
}

/// Separate overlapping tags on screen: solve bounded offsets from this
/// frame's projected rects, smooth each tag's applied offset toward its
/// target, and push the result into the per-instance [`MeshTag`] (whole
/// physical px — written only when the packed value changes, so settled
/// scenes write nothing) and into [`NameTagScreenRect`] for the hit test.
///
/// The solve freezes (offsets hold, smoothing continues) while the camera
/// moves faster than [`OVERLAP_FREEZE_CAMERA_SPEED`], like the reference —
/// separating rects that churn every frame just makes tags dance.
#[expect(
    clippy::type_complexity,
    clippy::arithmetic_side_effects,
    reason = "the per-tag query row is the solver state in one fetch, and the offset \
              smoothing / packing math is finite pixel-space geometry"
)]
pub(crate) fn solve_tag_overlap(
    time: Res<Time>,
    mut last_camera: Local<Option<Vec3>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::ViewerCamera>>,
    mut tags: Query<
        (
            &AvatarPickTarget,
            &NameTagPixelSize,
            &mut NameTagScreenRect,
            &mut NameTagOverlapOffset,
            &mut MeshTag,
            &Visibility,
        ),
        With<NameTag>,
    >,
) {
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let camera_position = camera_transform.translation();
    let dt = time.delta_secs();
    let camera_speed = last_camera
        .map(|last| {
            if dt > 0.0 {
                last.distance(camera_position) / dt
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);
    *last_camera = Some(camera_position);
    let scale_factor = camera.target_scaling_factor().unwrap_or(1.0);

    // Gather the visible, on-screen tags in a deterministic order.
    let mut entries: Vec<(AgentKey, Rect, f32)> = tags
        .iter()
        .filter(|(_, _, screen_rect, _, _, visibility)| {
            **visibility != Visibility::Hidden && screen_rect.rect.is_some()
        })
        .filter_map(|(pick, pixel_size, screen_rect, _, _, _)| {
            screen_rect.rect.map(|rect| {
                let buffered = Rect {
                    min: rect.min - Vec2::splat(OVERLAP_BUFFER_PX),
                    max: rect.max + Vec2::splat(OVERLAP_BUFFER_PX),
                };
                let tag_height = (pixel_size.0.y / scale_factor.max(f32::EPSILON)).max(1.0);
                (
                    pick.agent(),
                    buffered,
                    tag_height * MAX_OVERLAP_DISPLACEMENT_TAG_HEIGHTS,
                )
            })
        })
        .collect();
    entries.sort_by_key(|(agent, _, _)| *agent);

    let solved: bevy::platform::collections::HashMap<AgentKey, Vec2> = if camera_speed
        <= OVERLAP_FREEZE_CAMERA_SPEED
    {
        let inputs: Vec<(Rect, f32)> = entries.iter().map(|(_, rect, max)| (*rect, *max)).collect();
        let offsets = solve_overlap_offsets(&inputs);
        entries
            .iter()
            .zip(offsets)
            .map(|((agent, _, _), offset)| (*agent, offset))
            .collect()
    } else {
        bevy::platform::collections::HashMap::default()
    };
    let frozen = camera_speed > OVERLAP_FREEZE_CAMERA_SPEED;

    // Frame-rate-independent approach factor (same time constant as the
    // reference's offset damping).
    let approach = (1.0 - (-dt / SMOOTHING_TIME_CONSTANT_SECS).exp()).clamp(0.0, 1.0);

    for (pick, _, mut screen_rect, mut overlap, mut mesh_tag, visibility) in &mut tags {
        if *visibility == Visibility::Hidden {
            continue;
        }
        let target = if frozen {
            overlap.target
        } else {
            solved.get(&pick.agent()).copied().unwrap_or(Vec2::ZERO)
        };
        let current = if overlap.current.distance(target) <= OVERLAP_SNAP_PX {
            target
        } else {
            overlap.current.lerp(target, approach)
        };
        let next = NameTagOverlapOffset { target, current };
        if *overlap != next {
            *overlap = next;
        }

        // Viewport axes (+y down, logical) → billboard-local (+y up,
        // physical) for the shader; whole-pixel packing is the write guard.
        let packed = pack_overlap_offset(Vec2::new(
            current.x * scale_factor,
            -current.y * scale_factor,
        ));
        if mesh_tag.0 != packed {
            mesh_tag.0 = packed;
        }
        // The hit test sees the tag where it actually renders.
        if current != Vec2::ZERO
            && let Some(rect) = screen_rect.rect
        {
            let shifted = Rect {
                min: rect.min + current,
                max: rect.max + current,
            };
            let updated = NameTagScreenRect {
                rect: Some(shifted),
                camera_distance: screen_rect.camera_distance,
            };
            screen_rect.set_if_neq(updated);
        }
    }
}

/// Mirror each tag's per-instance [`MeshTag`] onto its [`NameTagPage`]
/// children (extra atlas pages), so a multi-page tag separates as one unit.
/// (`Visibility` needs no mirroring — pages inherit it.)
#[expect(
    clippy::type_complexity,
    reason = "the filter pair (marker + change gate) is clearer inline than behind an alias"
)]
pub(crate) fn sync_tag_pages(
    tags: Query<(&MeshTag, &Children), (With<NameTag>, Changed<MeshTag>)>,
    mut pages: Query<&mut MeshTag, (With<NameTagPage>, Without<NameTag>)>,
) {
    for (tag, children) in &tags {
        for child in children.iter() {
            if let Ok(mut page_tag) = pages.get_mut(child)
                && page_tag.0 != tag.0
            {
                page_tag.0 = tag.0;
            }
        }
    }
}

/// Resolve which avatar's name tag (if any) is under the cursor — the tag is
/// a valid avatar pick target (right-click opens the avatar menu, matching
/// the reference), and no picking backend covers our custom tag meshes, so
/// this is a stored-rect test against [`NameTagScreenRect`]. The front-most
/// (nearest-camera) tag wins where tags overlap.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct NameTagHitTest<'w, 's> {
    /// Every live tag's pick identity, screen rect and visibility.
    tags: Query<
        'w,
        's,
        (
            &'static AvatarPickTarget,
            &'static NameTagScreenRect,
            &'static Visibility,
        ),
        With<NameTag>,
    >,
}

impl NameTagHitTest<'_, '_> {
    /// The agent whose tag contains `cursor` (logical px, window top-left
    /// origin), if any.
    pub(crate) fn agent_at(&self, cursor: Vec2) -> Option<AgentKey> {
        self.tags
            .iter()
            .filter(|(_, _, visibility)| **visibility != Visibility::Hidden)
            .filter_map(|(pick, screen_rect, _)| {
                screen_rect
                    .rect
                    .filter(|rect| rect.contains(cursor))
                    .map(|_| (pick.agent(), screen_rect.camera_distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(agent, _)| agent)
    }
}

/// The plugin wiring the world-space name-tag billboards: the embedded shader
/// and the [`NameTagMaterial`] pipeline. The tag systems themselves are
/// registered alongside the avatar systems in `lib.rs`.
#[derive(Debug, Default)]
pub(crate) struct NameTagBillboardPlugin;

impl Plugin for NameTagBillboardPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            NAME_TAG_SHADER_HANDLE,
            "name_tag.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(MaterialPlugin::<NameTagMaterial>::default())
            .init_resource::<NameTagMaterials>();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GlyphQuadInput, NEUTRAL_MESH_TAG, TagText, build_tag_mesh_data, pack_overlap_offset,
        sync_tag_spans, unpack_overlap_offset,
    };
    use crate::name_tag_content::{TagContent, TagLine, TagLineSize};
    use bevy::prelude::*;
    use pretty_assertions::{assert_eq, assert_ne};

    /// Two-line content: a small grey title over a white name line.
    fn two_line_content() -> TagContent {
        TagContent {
            lines: vec![
                TagLine {
                    text: "Tester Title".to_owned(),
                    size: TagLineSize::Small,
                    color: Color::srgb(0.9, 0.9, 0.9),
                },
                TagLine {
                    text: "Avatar Tester".to_owned(),
                    size: TagLineSize::Name,
                    color: Color::WHITE,
                },
            ],
            base_color: Color::WHITE,
        }
    }

    /// The ordered span children of a tag root: `(text, font px, colour)`.
    fn span_rows(app: &mut App, root: Entity) -> Vec<(String, FontSize, Color)> {
        let world = app.world_mut();
        let children: Vec<Entity> = world
            .get::<Children>(root)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        children
            .into_iter()
            .filter_map(|child| {
                let span = world.get::<TextSpan>(child)?;
                let font = world.get::<TextFont>(child)?;
                let color = world.get::<TextColor>(child)?;
                Some((span.0.clone(), font.font_size, color.0))
            })
            .collect()
    }

    /// A minimal app running only the span sync.
    fn span_app() -> App {
        let mut app = App::new();
        app.add_systems(Update, sync_tag_spans);
        app
    }

    /// Lines materialise as ordered spans: newline separators on all but the
    /// last, the tier font sizes, and the line colours.
    #[test]
    fn spans_materialise_in_line_order() {
        let mut app = span_app();
        let root = app
            .world_mut()
            .spawn((TagText::default(), two_line_content()))
            .id();
        app.update();
        let rows = span_rows(&mut app, root);
        assert_eq!(
            rows,
            vec![
                (
                    "Tester Title\n".to_owned(),
                    FontSize::Px(13.0),
                    Color::srgb(0.9, 0.9, 0.9),
                ),
                ("Avatar Tester".to_owned(), FontSize::Px(16.0), Color::WHITE),
            ],
        );
    }

    /// A content change rewrites the existing span entities in place — no
    /// span churn while the line count is stable.
    #[test]
    fn content_change_updates_spans_in_place() {
        let mut app = span_app();
        let root = app
            .world_mut()
            .spawn((TagText::default(), two_line_content()))
            .id();
        app.update();
        let before: Vec<Entity> = app
            .world_mut()
            .get::<Children>(root)
            .map(|c| c.iter().collect())
            .unwrap_or_default();

        if let Some(mut content) = app.world_mut().get_mut::<TagContent>(root)
            && let Some(line) = content.lines.get_mut(1)
        {
            line.text = "Renamed Tester".to_owned();
        }
        app.update();
        let after: Vec<Entity> = app
            .world_mut()
            .get::<Children>(root)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        assert_eq!(before, after);
        let rows = span_rows(&mut app, root);
        assert_eq!(
            rows.into_iter()
                .map(|(text, _, _)| text)
                .collect::<Vec<_>>(),
            vec!["Tester Title\n".to_owned(), "Renamed Tester".to_owned()],
        );
    }

    /// Shrinking the line count despawns the excess spans (and the remaining
    /// line loses its newline separator).
    #[test]
    fn shrinking_content_despawns_excess_spans() {
        let mut app = span_app();
        let root = app
            .world_mut()
            .spawn((TagText::default(), two_line_content()))
            .id();
        app.update();

        if let Some(mut content) = app.world_mut().get_mut::<TagContent>(root) {
            content.lines.truncate(1);
        }
        app.update();
        let rows = span_rows(&mut app, root);
        assert_eq!(
            rows,
            vec![(
                "Tester Title".to_owned(),
                FontSize::Px(13.0),
                Color::srgb(0.9, 0.9, 0.9),
            )],
        );
    }

    /// Zero offset packs to the neutral tag and round-trips.
    #[test]
    fn neutral_offset_round_trips() {
        assert_eq!(pack_overlap_offset(Vec2::ZERO), NEUTRAL_MESH_TAG);
        assert_eq!(unpack_overlap_offset(NEUTRAL_MESH_TAG), Vec2::ZERO);
    }

    /// Positive and negative offsets survive the biased u16 round trip.
    #[test]
    fn signed_offsets_round_trip() {
        for offset in [
            Vec2::new(12.0, -34.0),
            Vec2::new(-1.0, 1.0),
            Vec2::new(500.0, -500.0),
        ] {
            assert_eq!(unpack_overlap_offset(pack_overlap_offset(offset)), offset);
        }
    }

    /// Components round to whole pixels before packing.
    #[test]
    fn offsets_round_to_whole_pixels() {
        assert_eq!(
            unpack_overlap_offset(pack_overlap_offset(Vec2::new(1.4, -1.6))),
            Vec2::new(1.0, -2.0),
        );
    }

    /// Out-of-range and non-finite components clamp instead of wrapping.
    #[test]
    fn extreme_offsets_clamp() {
        let packed = pack_overlap_offset(Vec2::new(1.0e9, -1.0e9));
        assert_eq!(
            unpack_overlap_offset(packed),
            Vec2::new(32_767.0, -32_768.0)
        );
        let nan = pack_overlap_offset(Vec2::new(f32::NAN, f32::NAN));
        assert_eq!(unpack_overlap_offset(nan), Vec2::ZERO);
    }

    /// One glyph centred at (10, 20), 4×6 px, on page 0.
    fn one_glyph() -> GlyphQuadInput {
        GlyphQuadInput {
            center: Vec2::new(10.0, 20.0),
            size: Vec2::new(4.0, 6.0),
            uv_min: Vec2::new(0.25, 0.5),
            uv_max: Vec2::new(0.5, 0.75),
            page: 0,
            color: LinearRgba::new(1.0, 0.0, 0.0, 1.0),
        }
    }

    /// A single glyph yields one page holding bubble + shadow + glyph quads in
    /// painter's order, and the bubble is the glyph box plus the reference
    /// padding.
    #[test]
    fn mesh_data_orders_bubble_shadow_glyph() {
        let data = build_tag_mesh_data(&[one_glyph()], 0.5, 1.0, 0.5, 0.0, true);
        assert_eq!(data.pages.len(), 1);
        let page = data.pages.first().cloned().unwrap_or_default();
        // Three quads: 12 vertices, 18 indices.
        assert_eq!(page.positions.len(), 12);
        assert_eq!(page.indices.len(), 18);
        // Bubble quad first: UV0 carries the negative-half-extent sentinel.
        assert!(page.uvs0.first().is_some_and(|uv| uv[0] < 0.0));
        // Shadow quad second: black, glyph alpha.
        assert_eq!(page.colors.get(4), Some(&[0.0, 0.0, 0.0, 1.0]));
        // Glyph quad last: the line colour.
        assert_eq!(page.colors.get(8), Some(&[1.0, 0.0, 0.0, 1.0]));
        // Bubble size = glyph box (4×6) + padding (16×12).
        assert_eq!(data.bubble_size, Vec2::new(20.0, 18.0));
        // The glyph's bounding box centres on the bubble centre — which, with
        // zero lift, is half the bubble height (9 px) above the anchor
        // (bottom-anchored mesh).
        let glyph_center: Vec2 = page
            .positions
            .iter()
            .skip(8)
            .map(|p| Vec2::new(p[0], p[1]))
            .sum::<Vec2>()
            / 4.0;
        assert_eq!(glyph_center * 1024.0, Vec2::new(0.0, 9.0));
        // Every vertex bakes the camera-pull radius into z.
        assert!(
            page.positions
                .iter()
                .all(|p| (p[2] - 0.5).abs() < f32::EPSILON)
        );
    }

    /// The drop shadow sits one logical pixel down-right of its glyph, scaled
    /// to physical pixels.
    #[test]
    fn shadow_offset_scales_with_scale_factor() {
        let data = build_tag_mesh_data(&[one_glyph()], 0.0, 2.0, 0.5, 0.0, true);
        let page = data.pages.first().cloned().unwrap_or_default();
        let shadow_center: Vec2 = page
            .positions
            .iter()
            .skip(4)
            .take(4)
            .map(|p| Vec2::new(p[0], p[1]))
            .sum::<Vec2>()
            / 4.0;
        // Tag-local units are px / 1024. At scale factor 2 the bubble is the
        // 4×6 glyph box + (32, 24) padding → half height 15 px, which is the
        // bubble-centred glyph position; the shadow offsets (1, -1) px × 2
        // from there.
        assert_eq!(shadow_center * 1024.0, Vec2::new(2.0, 13.0));
    }

    /// Glyphs on a second atlas page land in a second page mesh (shadow +
    /// glyph, no bubble) pulled a step closer to the camera.
    #[test]
    fn second_atlas_page_gets_own_geometry() {
        let mut emoji = one_glyph();
        emoji.page = 1;
        emoji.center = Vec2::new(20.0, 20.0);
        emoji.color = LinearRgba::WHITE;
        let data = build_tag_mesh_data(&[one_glyph(), emoji], 0.5, 1.0, 0.5, 0.0, true);
        assert_eq!(data.pages.len(), 2);
        let page1 = data.pages.get(1).cloned().unwrap_or_default();
        // Shadow + glyph only: 8 vertices.
        assert_eq!(page1.positions.len(), 8);
        // Page 1 pulls a millimetre closer than page 0.
        assert!(
            page1
                .positions
                .iter()
                .all(|p| (p[2] - 0.501).abs() < 1.0e-6)
        );
    }

    /// A minimal placement app: the follow system plus a write counter.
    fn placement_app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<TagWrites>();
        app.add_systems(
            Update,
            (super::follow_tag_anchors, count_tag_writes).chain(),
        );
        // A fake computed camera, the `ui_test` harness idiom: identity pose
        // looking down -Z with a symmetric orthographic clip sized to the
        // window.
        app.world_mut().spawn((
            crate::camera::ViewerCamera,
            Camera {
                computed: bevy::camera::ComputedCameraValues {
                    clip_from_view: Mat4::orthographic_rh(-640.0, 640.0, -360.0, 360.0, 0.1, 100.0),
                    target_info: Some(bevy::camera::RenderTargetInfo {
                        physical_size: UVec2::new(1280, 720),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            GlobalTransform::IDENTITY,
        ));
        app
    }

    /// How many tag `Transform`s were written (change-detected) this frame.
    #[derive(Resource, Default)]
    struct TagWrites(usize);

    /// Record how many tags' `Transform` changed this frame (an `Added`
    /// counts, which is exactly right: the first placement is a write).
    fn count_tag_writes(
        mut writes: ResMut<TagWrites>,
        changed: Query<(), (Changed<Transform>, With<crate::avatars::NameTag>)>,
    ) {
        writes.0 = changed.iter().count();
    }

    /// Spawn an anchor + tag pair; returns the tag entity.
    fn spawn_tag(app: &mut App, anchor_at: Vec3, agent: sl_client_bevy::AgentKey) -> Entity {
        let anchor = app
            .world_mut()
            .spawn((
                crate::avatars::AvatarAnchor,
                Transform::from_translation(anchor_at),
            ))
            .id();
        app.world_mut()
            .spawn((
                super::name_tag_render_bundle(0.5),
                crate::avatars::NameTag {
                    anchor,
                    tag_height: 0.3,
                },
                crate::avatars::AvatarPickTarget::new(agent),
            ))
            .id()
    }

    /// Headless end-to-end check of the placement: an in-range anchor gets its
    /// tag placed at anchor + tag height and made visible — and a second frame
    /// with nothing moved writes **nothing** (the inequality guards), which is
    /// what keeps stationary scenes free of per-frame tag work.
    #[test]
    fn placement_follows_anchor_and_idles_when_stationary() {
        let mut app = placement_app();
        let agent: sl_client_bevy::AgentKey = sl_client_bevy::Uuid::from_u128(3).into();
        let tag = spawn_tag(&mut app, Vec3::new(5.0, 0.0, -10.0), agent);

        app.update();
        assert_eq!(
            app.world().get::<Transform>(tag).map(|t| t.translation),
            Some(Vec3::new(5.0, 0.3, -10.0))
        );
        assert_eq!(
            app.world().get::<Visibility>(tag).copied(),
            Some(Visibility::Inherited)
        );
        // The projected screen rect exists and sits above the projection point.
        let rect = app
            .world()
            .get::<super::NameTagScreenRect>(tag)
            .and_then(|r| r.rect);
        assert!(rect.is_some());
        assert_eq!(app.world().resource::<TagWrites>().0, 1);

        // Nothing moved: the guarded writes must all skip.
        app.update();
        assert_eq!(app.world().resource::<TagWrites>().0, 0);
    }

    /// A tag past the fade cutoff (fade start + range, default 25 m) hides.
    #[test]
    fn placement_hides_beyond_fade_cutoff() {
        let mut app = placement_app();
        let agent: sl_client_bevy::AgentKey = sl_client_bevy::Uuid::from_u128(4).into();
        let tag = spawn_tag(&mut app, Vec3::new(0.0, 0.0, -40.0), agent);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(tag).copied(),
            Some(Visibility::Hidden)
        );
    }

    /// The preferences gates hide tags: the master toggle hides every tag, the
    /// own-tag toggle hides only the logged-in avatar's.
    #[test]
    fn preference_toggles_gate_tags() {
        use crate::avatars::{SETTING_SHOW_NAME_TAGS, SETTING_SHOW_OWN_NAME_TAG};
        use crate::settings::ViewerSettings;
        use sl_client_bevy::{SlIdentity, Uuid};
        use sl_settings::{Scope, SettingValue, SettingsStore};

        let own: sl_client_bevy::AgentKey = Uuid::from_u128(7).into();
        let other: sl_client_bevy::AgentKey = Uuid::from_u128(9).into();
        let mut store = SettingsStore::new();
        store
            .register(SETTING_SHOW_NAME_TAGS, SettingValue::Bool(true), "tags")
            .ok();
        store
            .register(SETTING_SHOW_OWN_NAME_TAG, SettingValue::Bool(true), "own")
            .ok();

        let mut app = placement_app();
        app.insert_resource(ViewerSettings::from_store_for_test(store));
        app.insert_resource(SlIdentity {
            agent_id: Some(own),
            ..default()
        });
        let own_tag = spawn_tag(&mut app, Vec3::new(0.0, 0.0, -5.0), own);
        let other_tag = spawn_tag(&mut app, Vec3::new(2.0, 0.0, -5.0), other);

        app.update();
        assert_eq!(
            app.world().get::<Visibility>(own_tag).copied(),
            Some(Visibility::Inherited)
        );

        // Own-tag toggle off: only the own tag hides.
        if let Some(settings) = app.world_mut().get_resource_mut::<ViewerSettings>() {
            settings.into_inner().set(
                Scope::Global,
                SETTING_SHOW_OWN_NAME_TAG,
                SettingValue::Bool(false),
            );
        }
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(own_tag).copied(),
            Some(Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(other_tag).copied(),
            Some(Visibility::Inherited)
        );

        // Master toggle off: every tag hides.
        if let Some(settings) = app.world_mut().get_resource_mut::<ViewerSettings>() {
            settings.into_inner().set(
                Scope::Global,
                SETTING_SHOW_NAME_TAGS,
                SettingValue::Bool(false),
            );
        }
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(other_tag).copied(),
            Some(Visibility::Hidden)
        );
    }

    /// Two overlapping tag rects separate; the result is deterministic and
    /// the final (offset) rects no longer intersect.
    #[test]
    fn overlap_solver_separates_overlapping_rects() {
        let a = Rect::new(0.0, 0.0, 100.0, 40.0);
        let b = Rect::new(10.0, 10.0, 110.0, 50.0);
        let offsets = super::solve_overlap_offsets(&[(a, 100.0), (b, 100.0)]);
        let (off_a, off_b) = (
            offsets.first().copied().unwrap_or_default(),
            offsets.get(1).copied().unwrap_or_default(),
        );
        assert_ne!((off_a, off_b), (Vec2::ZERO, Vec2::ZERO));
        let moved_a = Rect {
            min: a.min + off_a,
            max: a.max + off_a,
        };
        let moved_b = Rect {
            min: b.min + off_b,
            max: b.max + off_b,
        };
        // The spring converges geometrically (like the reference's), so the
        // residual penetration after ten iterations is sub-pixel, not zero.
        let residual = moved_a.intersect(moved_b);
        assert!(residual.is_empty() || residual.size().min_element() < 0.5);
        // Deterministic: solving again gives the same offsets.
        assert_eq!(
            super::solve_overlap_offsets(&[(a, 100.0), (b, 100.0)]),
            offsets
        );
    }

    /// Disjoint rects stay exactly where they are.
    #[test]
    fn overlap_solver_leaves_disjoint_rects_alone() {
        let a = Rect::new(0.0, 0.0, 100.0, 40.0);
        let b = Rect::new(200.0, 0.0, 300.0, 40.0);
        assert_eq!(
            super::solve_overlap_offsets(&[(a, 100.0), (b, 100.0)]),
            vec![Vec2::ZERO, Vec2::ZERO],
        );
    }

    /// The per-tag displacement bound holds even when full separation would
    /// need more room (the user's no-drift constraint).
    #[test]
    fn overlap_solver_respects_displacement_bound() {
        let a = Rect::new(0.0, 0.0, 100.0, 40.0);
        let b = Rect::new(1.0, 1.0, 101.0, 41.0);
        let offsets = super::solve_overlap_offsets(&[(a, 5.0), (b, 5.0)]);
        assert!(offsets.iter().all(|off| off.length() <= 5.0 + 1.0e-3));
    }

    /// No glyphs → one empty page and a zero footprint (no floating bubble).
    #[test]
    fn empty_content_builds_empty_mesh() {
        let data = build_tag_mesh_data(&[], 0.5, 1.0, 0.5, 0.0, true);
        assert_eq!(data.pages.len(), 1);
        assert_eq!(data.pages.first().map(|p| p.positions.len()), Some(0));
        assert_eq!(data.bubble_size, Vec2::ZERO);
    }
}
