//! Object **floating text** (`llSetText`) — the world-anchored text a script
//! sets over a prim (vendors, rental boxes, scripted signs, HUD readouts).
//!
//! This reuses the world-space text billboard the avatar name tags render
//! through ([`crate::name_tag_billboard`]): the same [`TagText`] layout, glyph
//! atlas and camera-facing constant-on-screen-size mesh, styled with
//! [`WorldTextStyle::HOVER_TEXT`] (no chat-bubble backdrop, no 25 px screen
//! lift, and a shorter `LLHUDText` fade sampled from [`HoverTextMaterials`]).
//!
//! The reference-faithful bits (`LLViewerObject::updateText`,
//! `LLHUDText::renderText`):
//!
//! - **Vertical anchor** — the text hangs off the object *centre* lifted by
//!   `0.6 × (full local Z scale)` in **world up**, never rotated by the prim
//!   (`up_offset.mV[2] = getScale().mV[VZ]*0.6f`). This exact offset matters:
//!   creators place `llSetText` precisely (text framed over a texture-only
//!   backdrop prim), so [`HOVER_ANCHOR_SCALE_FACTOR`] must match the reference.
//! - **Bottom-anchored** — the text block grows upward from the anchor
//!   (`ALIGN_VERT_TOP`); the shared mesh builder is bottom-anchored already, so
//!   passing `lift_px = 0` lands the block bottom on the anchor.
//! - **Colour + inverted alpha** — the wire alpha byte is transmitted inverted
//!   (`coloru.mV[3] = 255 - coloru.mV[3]`); a resulting alpha of 0 is the common
//!   "text set but invisible, revealed later by script" trick, so the entity is
//!   kept (drawn fully transparent) rather than dropped.
//! - **Empty text destroys the text object** — `llSetText("")` clears it, so an
//!   empty string despawns the billboard.
//! - **Fade + hard cap** — fades from [`SETTING_HOVER_FADE_START`] over
//!   [`SETTING_HOVER_FADE_RANGE`] (the reference's 8 m / 4 m), and never draws
//!   past [`SETTING_PRIM_TEXT_MAX_DISTANCE`] (`PrimTextMaxDrawDistance`, 64 m).
//!
//! Billboards are **top-level** entities (never children of the object), so the
//! object subtree's `Propagate(probe layers)` cannot leak a reflection-probe
//! layer onto them — the same rule the name tags follow. Their lifetime is
//! tracked in [`HoverTextLabels`] and reaped when the object's
//! [`ObjectFloatingText`] is removed (cleared text *or* the object despawning).

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::text::TextBounds;

use crate::name_tag_billboard::{
    HoverTextMaterials, NEUTRAL_MESH_TAG, NameTagMaterial, NameTagPixelSize, NameTagPullRadius,
    TagText, WorldTextStyle, tag_render_layers,
};
use crate::name_tag_content::{TagContent, TagLine, TagLineSize};
use crate::objects::{ObjectSlMotion, SceneObject};

/// The world-up lift of the text anchor as a fraction of the prim's full local
/// Z scale (`LLViewerObject::updateText`: `getScale().mV[VZ]*0.6f`). Applied in
/// world up regardless of the prim's rotation — 10 % of the height above the
/// top of an un-rotated bounding box.
pub(crate) const HOVER_ANCHOR_SCALE_FACTOR: f32 = 0.6;

/// The distance at which floating text starts to fade, metres — the reference's
/// `FSHudTextFadeDistance` / stock `LLHUDText::mFadeDistance` (8 m).
pub(crate) const DEFAULT_HOVER_FADE_START_METRES: f32 = 8.0;

/// Metres past the fade start at which floating text is fully gone — the
/// reference's `FSHudTextFadeRange` / stock `LLHUDText::mFadeRange` (4 m), so
/// text vanishes at 12 m.
pub(crate) const DEFAULT_HOVER_FADE_RANGE_METRES: f32 = 4.0;

/// The hard maximum draw distance for prim text, metres — the reference's
/// `PrimTextMaxDrawDistance` (64 m). Unlike name tags, floating text is never
/// drawn past this even when the fade range would keep it visible.
pub(crate) const DEFAULT_PRIM_TEXT_MAX_DISTANCE_METRES: f32 = 64.0;

/// The floating-text word-wrap width, logical px — the reference's
/// `HUD_TEXT_MAX_WIDTH_NO_BUBBLE` (1000 px): effectively "don't wrap unless
/// extremely long", unlike the name tag's 298 px chat-bubble wrap.
const HOVER_MAX_WIDTH_PX: f32 = 1000.0;

/// Master toggle: show floating object text at all (the reference's
/// `RenderHUDText` / hover-text preference; default on).
pub(crate) const SETTING_SHOW_HOVER_TEXT: &str = "ShowHoverText";

/// The floating-text fade-start distance, metres (a float setting).
pub(crate) const SETTING_HOVER_FADE_START: &str = "HoverTextFadeDistance";

/// The floating-text fade range, metres (a float setting).
pub(crate) const SETTING_HOVER_FADE_RANGE: &str = "HoverTextFadeRange";

/// The floating-text hard maximum draw distance, metres (a float setting).
pub(crate) const SETTING_PRIM_TEXT_MAX_DISTANCE: &str = "PrimTextMaxDrawDistance";

/// The settings section floating-text toggles live in.
const HOVER_TEXT_SECTION: &[&str] = &["hovertext"];

/// Register the floating-text settings.
pub(crate) fn register_settings(settings: &mut crate::settings::ViewerSettings) {
    settings.register_in(
        HOVER_TEXT_SECTION,
        SETTING_SHOW_HOVER_TEXT,
        sl_settings::SettingValue::Bool(true),
        "Show floating text (llSetText) over in-world objects",
    );
    settings.register_in(
        HOVER_TEXT_SECTION,
        SETTING_HOVER_FADE_START,
        sl_settings::SettingValue::F32(DEFAULT_HOVER_FADE_START_METRES),
        "Distance in metres at which floating object text starts to fade",
    );
    settings.register_in(
        HOVER_TEXT_SECTION,
        SETTING_HOVER_FADE_RANGE,
        sl_settings::SettingValue::F32(DEFAULT_HOVER_FADE_RANGE_METRES),
        "Metres past the fade start at which floating object text is hidden",
    );
    settings.register_in(
        HOVER_TEXT_SECTION,
        SETTING_PRIM_TEXT_MAX_DISTANCE,
        sl_settings::SettingValue::F32(DEFAULT_PRIM_TEXT_MAX_DISTANCE_METRES),
        "Hard maximum distance in metres at which floating object text is drawn",
    );
}

/// The floating text an object currently carries, mirrored onto its object
/// entity from the decoded `Object` on every full / compressed update
/// ([`crate::objects::apply_object`]). Absent when the object has no text (an
/// `llSetText("")` clears it); a terse motion update never touches it.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectFloatingText {
    /// The text (already trimmed of the trailing NUL by the decode).
    pub(crate) text: String,
    /// The RGBA colour, **as transmitted** — the alpha byte is inverted on the
    /// wire and un-inverted in [`Self::color`].
    pub(crate) raw_color: [u8; 4],
}

impl ObjectFloatingText {
    /// The display colour: the wire RGB with the alpha byte un-inverted
    /// (`255 - a`, `LLViewerObject::processUpdateMessage`).
    const fn color(&self) -> Color {
        let [r, g, b, a] = self.raw_color;
        Color::srgba_u8(r, g, b, 255_u8.saturating_sub(a))
    }

    /// The composed tag content: one line per `\r` / `\n`-separated source line,
    /// all in the object's colour at the name font tier (`getFontSansSerif`).
    fn to_content(&self) -> TagContent {
        let color = self.color();
        let lines: Vec<TagLine> = self
            .text
            .split(['\r', '\n'])
            .map(|line| TagLine {
                text: line.to_owned(),
                size: TagLineSize::Name,
                color,
            })
            .collect();
        TagContent {
            lines,
            base_color: color,
        }
    }
}

/// A world-space floating-text billboard, pointing back at the object entity it
/// floats over so [`follow_hover_text`] can track its world pose and Z scale.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct HoverText {
    /// The object entity this text labels.
    pub(crate) object: Entity,
}

/// Maps an object entity to its floating-text billboard entity, so a cleared
/// (or despawned) object can reap the top-level billboard it owns.
#[derive(Resource, Debug, Default)]
pub(crate) struct HoverTextLabels(HashMap<Entity, Entity>);

/// The renderer-side components of a floating-text billboard (mirrors
/// [`crate::name_tag_billboard::name_tag_render_bundle`], minus the name-tag
/// anti-overlap state — floating text is placed precisely and never nudged).
fn hover_text_render_bundle(pull_radius: f32) -> impl Bundle {
    (
        TagText::default(),
        TextLayout {
            justify: Justify::Center,
            linebreak: LineBreak::WordOrCharacter,
        },
        // The reference wraps floating text only at 1000 px (no chat bubble).
        TextBounds {
            width: Some(HOVER_MAX_WIDTH_PX),
            height: None,
        },
        WorldTextStyle::HOVER_TEXT,
        NameTagPullRadius(pull_radius),
        NameTagPixelSize::default(),
        // No anti-overlap solve runs over floating text, so the shader's
        // per-instance offset stays neutral (else it unpacks tag 0 as a huge
        // negative offset and draws off-screen).
        bevy::mesh::MeshTag(NEUTRAL_MESH_TAG),
        Transform::default(),
        // Hidden until the first placement so it never flashes at the origin.
        Visibility::Hidden,
        bevy::camera::visibility::NoFrustumCulling,
        tag_render_layers(),
    )
}

/// The object's bounding radius, metres, from its Second Life scale — the
/// reference's `getVObjRadius()` camera pull so the prim cannot swallow its own
/// text.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "finite half-extent of a bounded prim scale; the glam operator is the readable form"
)]
fn object_pull_radius(scale: &sl_client_bevy::Vector) -> f32 {
    let half = Vec3::new(scale.x, scale.y, scale.z) * 0.5;
    half.length()
}

/// Spawn / update / drop floating-text billboards from the objects' mirrored
/// [`ObjectFloatingText`]: a changed text (re)composes the billboard's content
/// in place (spawning it on first sight), the compare-then-assign on
/// [`TagContent`] keeping the layout pipeline quiet when nothing shown changed.
pub(crate) fn sync_object_hover_text(
    mut commands: Commands,
    mut labels: ResMut<HoverTextLabels>,
    changed: Query<(Entity, &ObjectFloatingText, &ObjectSlMotion), Changed<ObjectFloatingText>>,
    mut contents: Query<(&mut TagContent, &mut NameTagPullRadius), With<HoverText>>,
) {
    for (object, floating, motion) in &changed {
        let content = floating.to_content();
        let pull_radius = object_pull_radius(&motion.scale);
        if let Some(&label) = labels.0.get(&object) {
            if let Ok((mut existing, mut radius)) = contents.get_mut(label) {
                if *existing != content {
                    *existing = content;
                }
                if (radius.0 - pull_radius).abs() > f32::EPSILON {
                    radius.0 = pull_radius;
                }
            }
        } else {
            let label = commands
                .spawn((
                    hover_text_render_bundle(pull_radius),
                    content,
                    HoverText { object },
                ))
                .id();
            labels.0.insert(object, label);
        }
    }
}

/// Reap floating-text billboards whose object lost its [`ObjectFloatingText`] —
/// fired both when a script clears the text (`llSetText("")`, the component is
/// removed) and when the object despawns entirely (the component goes with it).
pub(crate) fn despawn_removed_hover_text(
    mut commands: Commands,
    mut labels: ResMut<HoverTextLabels>,
    mut removed: RemovedComponents<ObjectFloatingText>,
) {
    for object in removed.read() {
        if let Some(label) = labels.0.remove(&object) {
            commands.entity(label).try_despawn();
        }
    }
}

/// The floating-text placement inputs resolved once per frame from the settings
/// store (or the reference defaults when a store key is missing / absent).
struct HoverTextPlacement {
    /// Whether floating text is shown at all.
    show: bool,
    /// The camera distance, metres, past which text is fully faded (fade start +
    /// range) — text beyond it stops rendering.
    fade_cutoff: f32,
    /// The hard maximum draw distance, metres.
    max_distance: f32,
}

impl HoverTextPlacement {
    /// Resolve the placement inputs from the settings (defaults when absent, so
    /// a bare headless test world runs shown at the reference distances).
    fn resolve(settings: Option<&crate::settings::ViewerSettings>) -> Self {
        let show = settings
            .and_then(|settings| settings.store().get_bool(SETTING_SHOW_HOVER_TEXT).ok())
            .unwrap_or(true);
        let fade_start = settings
            .and_then(|settings| settings.store().get_f32(SETTING_HOVER_FADE_START).ok())
            .unwrap_or(DEFAULT_HOVER_FADE_START_METRES);
        let fade_range = settings
            .and_then(|settings| settings.store().get_f32(SETTING_HOVER_FADE_RANGE).ok())
            .unwrap_or(DEFAULT_HOVER_FADE_RANGE_METRES);
        let max_distance = settings
            .and_then(|settings| {
                settings
                    .store()
                    .get_f32(SETTING_PRIM_TEXT_MAX_DISTANCE)
                    .ok()
            })
            .unwrap_or(DEFAULT_PRIM_TEXT_MAX_DISTANCE_METRES);
        Self {
            show,
            fade_cutoff: fade_start + fade_range,
            max_distance,
        }
    }

    /// The distance, metres, past which a billboard stops rendering: the nearer
    /// of the fade cutoff and the hard cap (the reference culls at either).
    const fn cull_distance(&self) -> f32 {
        self.fade_cutoff.min(self.max_distance)
    }
}

/// The world anchor point of an object's floating text: its centre lifted by
/// [`HOVER_ANCHOR_SCALE_FACTOR`] × the prim's Z scale, in **world up** (Bevy
/// +Y), never rotated by the prim — the reference's `up_offset`.
pub(crate) fn hover_text_anchor(object_world: &GlobalTransform, sl_scale_z: f32) -> Vec3 {
    let base = object_world.translation();
    Vec3::new(
        base.x,
        base.y + HOVER_ANCHOR_SCALE_FACTOR * sl_scale_z,
        base.z,
    )
}

/// Everything [`follow_hover_text`] reads about the object a billboard labels.
#[derive(SystemParam)]
pub(crate) struct HoverTextObjects<'w, 's> {
    /// Object world pose (for a linkset child this is the only correct world
    /// position) and its Second Life Z scale (the lift term).
    objects: Query<'w, 's, (&'static GlobalTransform, &'static ObjectSlMotion), With<SceneObject>>,
}

/// Place each floating-text billboard over its object anchor, cull it by
/// distance, and gate it on the show-hover-text preference. Reads the object's
/// `GlobalTransform` (correct for linkset children), so the billboard trails a
/// moving object by one frame — imperceptible for the stationary vendors /
/// signs floating text lives on.
pub(crate) fn follow_hover_text(
    cameras: Query<&GlobalTransform, With<crate::camera::ViewerCamera>>,
    objects: HoverTextObjects,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    mut billboards: Query<(&HoverText, &mut Transform, &mut Visibility)>,
    mut logged: Local<bevy::ecs::entity::EntityHashSet>,
) {
    let Ok(camera_transform) = cameras.single() else {
        return;
    };
    let camera_position = camera_transform.translation();
    let placement = HoverTextPlacement::resolve(settings.as_deref());
    let cull_distance = placement.cull_distance();
    // A one-shot-per-object diagnostic (`SL_VIEWER_LOG_HOVER_TEXT`) for the
    // anchor height: prints the object centre, its Z scale, and the resulting
    // lift so a "too low" report can be read against the object's real size.
    let log_anchor = std::env::var_os("SL_VIEWER_LOG_HOVER_TEXT").is_some();

    for (hover, mut transform, mut visibility) in &mut billboards {
        let Ok((object_world, motion)) = objects.objects.get(hover.object) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        if !placement.show {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let anchor = hover_text_anchor(object_world, motion.scale.z);
        if log_anchor && logged.insert(hover.object) {
            let center = object_world.translation();
            info!(
                "hover-text anchor: object centre y={:.3}, sl scale=({:.3},{:.3},{:.3}), \
                 lift={:.3}, anchor y={:.3}",
                center.y,
                motion.scale.x,
                motion.scale.y,
                motion.scale.z,
                HOVER_ANCHOR_SCALE_FACTOR * motion.scale.z,
                anchor.y,
            );
        }
        if transform.translation != anchor {
            transform.translation = anchor;
        }
        if camera_position.distance(anchor) > cull_distance {
            visibility.set_if_neq(Visibility::Hidden);
        } else {
            visibility.set_if_neq(Visibility::Inherited);
        }
    }
}

/// Apply the floating-text fade settings when they change: rewrite every live
/// [`HoverTextMaterials`] material's fade params (a bind-group recreation per
/// material — fine for a settings change, forbidden per frame).
#[expect(
    clippy::float_cmp,
    reason = "the registry stores verbatim copies of the setting values, so exact \
              equality is the correct change test"
)]
pub(crate) fn apply_hover_text_settings(
    settings: Option<Res<crate::settings::ViewerSettings>>,
    mut registry: ResMut<HoverTextMaterials>,
    mut materials: ResMut<Assets<NameTagMaterial>>,
) {
    let Some(settings) = settings else {
        return;
    };
    if !settings.is_changed() {
        return;
    }
    let store = settings.store();
    let fade_start = store
        .get_f32(SETTING_HOVER_FADE_START)
        .unwrap_or(DEFAULT_HOVER_FADE_START_METRES);
    let fade_range = store
        .get_f32(SETTING_HOVER_FADE_RANGE)
        .unwrap_or(DEFAULT_HOVER_FADE_RANGE_METRES);
    if fade_start == registry.0.fade_start && fade_range == registry.0.fade_range {
        return;
    }
    registry.0.fade_start = fade_start;
    registry.0.fade_range = fade_range;
    let handles: Vec<Handle<NameTagMaterial>> = registry.0.handles().cloned().collect();
    for handle in handles {
        if let Some(mut material) = materials.get_mut(&handle) {
            material.params = Vec4::new(fade_start, fade_range, 0.0, 0.0);
        }
    }
}

/// The floating object-text plugin: the [`HoverTextMaterials`] registry and the
/// [`HoverTextLabels`] lifetime map. The systems are scheduled alongside the
/// name-tag chain in `lib.rs`.
#[derive(Debug, Default)]
pub(crate) struct HoverTextPlugin;

impl Plugin for HoverTextPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverTextMaterials>()
            .init_resource::<HoverTextLabels>();
    }
}

#[cfg(test)]
mod tests {
    use super::{HOVER_ANCHOR_SCALE_FACTOR, ObjectFloatingText, hover_text_anchor};
    use crate::name_tag_content::TagLineSize;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// The wire alpha byte is inverted: a transmitted 0 means fully opaque, a
    /// transmitted 255 means fully transparent (the `llSetText` invisible trick).
    #[test]
    fn alpha_byte_is_inverted() {
        let opaque = ObjectFloatingText {
            text: "Vendor".to_owned(),
            raw_color: [255, 128, 0, 0],
        };
        assert!((opaque.color().to_srgba().alpha - 1.0).abs() < f32::EPSILON);

        let invisible = ObjectFloatingText {
            text: "Hidden".to_owned(),
            raw_color: [255, 255, 255, 255],
        };
        assert!(invisible.color().to_srgba().alpha.abs() < f32::EPSILON);
    }

    /// Multi-line text splits on both `\r` and `\n`, one line per split, all at
    /// the name tier and the object colour.
    #[test]
    fn multiline_splits_on_cr_and_lf() {
        let floating = ObjectFloatingText {
            text: "Line 1\nLine 2\rLine 3".to_owned(),
            raw_color: [10, 20, 30, 0],
        };
        let content = floating.to_content();
        let texts: Vec<&str> = content
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(texts, vec!["Line 1", "Line 2", "Line 3"]);
        assert!(
            content
                .lines
                .iter()
                .all(|line| line.size == TagLineSize::Name)
        );
    }

    /// The anchor lifts the object centre by 0.6 × Z scale in world up only —
    /// the X/Z of a rotated prim's centre are untouched.
    #[test]
    fn anchor_lifts_center_in_world_up() {
        let world = GlobalTransform::from(Transform {
            translation: Vec3::new(3.0, 5.0, -7.0),
            rotation: Quat::from_rotation_x(1.2),
            scale: Vec3::ONE,
        });
        let anchor = hover_text_anchor(&world, 2.0);
        assert_eq!(
            anchor,
            Vec3::new(3.0, 5.0 + HOVER_ANCHOR_SCALE_FACTOR * 2.0, -7.0)
        );
    }
}
