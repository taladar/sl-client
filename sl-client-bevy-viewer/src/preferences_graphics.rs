//! The Preferences **graphics** tab (`viewer-preferences-graphics-tab`).
//!
//! Surfaces the render knobs as controls bound to the typed settings store
//! through the preferences shell ([`crate::preferences`]): the quality tier,
//! draw distance, the mesh / prim LOD factor, particles, shadows, reflections
//! and mirrors, glow, tone mapping / exposure, and vsync / frame-rate cap.
//!
//! Ownership: this module registers only the settings **it** consumes (the
//! shadow, vsync / FPS-cap and quality-tier keys). The other rows bind
//! settings owned — registered and applied — by their feature modules:
//! [`crate::session`] (draw distance), [`crate::render_priority`] (LOD
//! factor), [`crate::particles`], [`crate::glow`], [`crate::tonemap`],
//! [`crate::exposure`], and [`crate::probes`] (which registers its keys in
//! `Startup` systems — always before the floater's deferred first-open
//! build, so binding those keys here is safe).
//!
//! - **Quality tier** ([`SETTING_RENDER_QUALITY`], the reference
//!   `RenderQualityPerformance`): a *driver* control. A user pick writes the
//!   [`QUALITY_TIERS`] row's values through the store (the reference
//!   `LLFeatureManager::setGraphicsLevel` shape); every written key has a
//!   bound row in this tab, so the shell's open-time snapshot covers a tier
//!   click and Cancel fully reverts it. The applier reacts to
//!   [`ComboChanged`] only — a programmatic [`ComboSelection`] write (the
//!   Cancel revert path) emits none, so a revert can never re-trigger the
//!   tier. Manually editing an individual setting deliberately does **not**
//!   move the stored tier (reference behaviour; no "custom" sentinel).
//! - **Shadows**: detail 0 = none, 1 = sun / moon (no projector shadows
//!   exist; the reference's `2` slots in later), the shadow-map resolution
//!   ([`bevy::light::DirectionalLightShadowMap`] is read by the render world
//!   every frame, so it applies live) and the cascade count. The
//!   screenshot-harness envs (`SL_VIEWER_SUN_SHADOWS`,
//!   `SL_VIEWER_SHADOW_CASCADES`) **win** over the stored values, the
//!   [`crate::tonemap`] / [`crate::glow`] override pattern.
//! - **VSync / FPS cap**: vsync flips the primary window's
//!   [`PresentMode`] (`AutoVsync` / `AutoNoVsync`); the cap sleeps the main
//!   schedule in [`Last`] (the `bevy_framepace` mechanism — with pipelined
//!   rendering it costs up to one frame of latency, fine for a
//!   thermals / battery cap).
//! - **Mirror resolution** is restart-scoped (it sizes GPU targets at
//!   [`crate::probes`]' rig setup); its row label says so — the shell has no
//!   restart-note idiom, and baking it into the label keeps it searchable.
//!
//! [`ComboSelection`]: crate::ui_combo::ComboSelection
//!
//! Reference (Firestorm, read-only): `panel_preferences_graphics1.xml`,
//! `llfloaterpreference.cpp` (`onChangeQuality`), `llfeaturemanager.cpp`
//! (`setGraphicsLevel`, `featuretable.txt`).

use bevy::light::{CascadeShadowConfig, DirectionalLightShadowMap};
use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderStep};
use bevy::window::{PresentMode, PrimaryWindow};
use sl_settings::{Scope, SettingValue};

use crate::preferences::{
    spawn_pref_checkbox, spawn_pref_combo, spawn_pref_combo_with_anchor, spawn_pref_section,
    spawn_pref_slider,
};
use crate::settings::ViewerSettings;
use crate::settings_binding::SettingBinding;
use crate::sky::SceneSun;
use crate::ui_combo::ComboChanged;

/// The stable id of this tab in [`crate::preferences::PREF_TABS`].
pub(crate) const TAB_ID: &str = "graphics";

/// The persisted-file section the tab's own settings live in (`[render]`),
/// matching the reference's `Render*` naming and [`crate::session`]'s section.
const RENDER_SECTION: &[&str] = &["render"];

/// The reference `RenderShadowDetail` setting name: `0` = no shadows, `1` =
/// sun / moon shadows. The reference's `2` (+ projector shadows) is not a
/// level this viewer has — the numbering is kept so it can slot in later.
pub(crate) const SETTING_SHADOW_DETAIL: &str = "RenderShadowDetail";
/// The default shadow detail: sun / moon shadows on (the spawn-time default
/// of [`SceneSun`]).
const DEFAULT_SHADOW_DETAIL: u32 = 1;

/// The directional shadow-map resolution setting (texels per side, a power
/// of two). Applied live to [`DirectionalLightShadowMap`].
pub(crate) const SETTING_SHADOW_MAP_SIZE: &str = "RenderShadowMapSize";
/// The default shadow-map resolution (the value `main` has always set).
const DEFAULT_SHADOW_MAP_SIZE: u32 = 4096;
/// The smallest selectable shadow-map resolution.
const SHADOW_MAP_SIZE_MIN: u32 = 1024;
/// The largest selectable shadow-map resolution.
const SHADOW_MAP_SIZE_MAX: u32 = 8192;

/// The sun shadow cascade-count setting (1–4). The
/// `SL_VIEWER_SHADOW_CASCADES` experiment env, when set, wins over this.
pub(crate) const SETTING_SHADOW_CASCADES: &str = "RenderShadowCascades";
/// The default cascade count ([`crate::sky::shadow_cascades`]' default).
const DEFAULT_SHADOW_CASCADES: u32 = 4;
/// The smallest selectable cascade count.
const SHADOW_CASCADES_MIN: u32 = 1;
/// The largest selectable cascade count (Bevy's cascade maximum).
const SHADOW_CASCADES_MAX: u32 = 4;

/// The reference `RenderVSyncEnable` setting name: sync presentation to the
/// monitor refresh (the window's [`PresentMode`]).
pub(crate) const SETTING_VSYNC: &str = "RenderVSyncEnable";
/// The default vsync state: on (Bevy's default window `PresentMode` is
/// `Fifo`, i.e. vsync).
const DEFAULT_VSYNC: bool = true;

/// The Firestorm `FSLimitFramerate` setting name (kept so a Firestorm value
/// ports across): whether the frame-rate cap is active.
pub(crate) const SETTING_LIMIT_FRAMERATE: &str = "FSLimitFramerate";
/// The default frame-rate-cap state: off.
const DEFAULT_LIMIT_FRAMERATE: bool = false;

/// The Firestorm `FramePerSecondLimit` setting name: the frame-rate cap, in
/// frames per second, applied while [`SETTING_LIMIT_FRAMERATE`] is on.
pub(crate) const SETTING_FPS_LIMIT: &str = "FramePerSecondLimit";
/// The default frame-rate cap.
const DEFAULT_FPS_LIMIT: u32 = 60;
/// The lowest selectable frame-rate cap.
const FPS_LIMIT_MIN: f32 = 15.0;
/// The highest selectable frame-rate cap.
const FPS_LIMIT_MAX: f32 = 240.0;
/// The frame-rate-cap slider step.
const FPS_LIMIT_STEP: f32 = 5.0;

/// The reference `RenderQualityPerformance` setting name: the last-applied
/// quality tier, `0` (low) ..= `6` (ultra), indexing [`QUALITY_TIERS`].
pub(crate) const SETTING_RENDER_QUALITY: &str = "RenderQualityPerformance";
/// The default quality tier: 4 ("high" — this viewer's stock defaults sit
/// between the high and ultra rows).
const DEFAULT_RENDER_QUALITY: u32 = 4;

/// The LOD-factor slider step (an eighth, the reference slider's increment).
const LOD_FACTOR_STEP: f32 = 0.125;

/// The glow-strength slider bounds / step (default 0.325; the additive
/// per-pass strength saturates fast, so the useful range is small).
const GLOW_STRENGTH_MAX: f32 = 1.0;
/// See [`GLOW_STRENGTH_MAX`].
const GLOW_STRENGTH_STEP: f32 = 0.025;
/// The glow-width slider maximum (default 1.3; beyond ~4 the blur smears).
const GLOW_WIDTH_MAX: f32 = 4.0;
/// The glow-width slider step.
const GLOW_WIDTH_STEP: f32 = 0.1;
/// The glow blur-iteration slider bounds (each iteration is two passes; `0`
/// would extract but never blur, so the floor is 1).
const GLOW_ITERATIONS_MIN: f32 = 1.0;
/// See [`GLOW_ITERATIONS_MIN`].
const GLOW_ITERATIONS_MAX: f32 = 4.0;

/// The tone-mix slider step (0–1 blend of the tone curve).
const TONEMAP_MIX_STEP: f32 = 0.05;
/// The exposure slider bounds / step (the reference `RenderExposure` slider).
const EXPOSURE_MIN: f32 = 0.5;
/// See [`EXPOSURE_MIN`].
const EXPOSURE_MAX: f32 = 4.0;
/// See [`EXPOSURE_MIN`].
const EXPOSURE_STEP: f32 = 0.1;

/// Marks a quality-tier combo **anchor** (the entity emitting
/// [`ComboChanged`]), so [`apply_quality_tier`] recognises a user pick on
/// either surface carrying it — this tab and the quick-preferences panel.
#[derive(Component, Debug)]
pub(crate) struct QualityTierControl;

/// One quality tier's target values ([`QUALITY_TIERS`]).
struct QualityTier {
    /// Draw distance, metres ([`crate::session::SETTING_DRAW_DISTANCE`]).
    far_clip: f32,
    /// Mesh / prim LOD factor
    /// ([`crate::render_priority::SETTING_LOD_FACTOR`]).
    lod_factor: f32,
    /// Particle cap ([`crate::particles::SETTING_MAX_PARTICLES`]).
    max_particles: u32,
    /// Shadow detail ([`SETTING_SHADOW_DETAIL`]).
    shadow_detail: u32,
    /// Shadow-map resolution ([`SETTING_SHADOW_MAP_SIZE`]).
    shadow_map_size: u32,
    /// Glow pass on ([`crate::glow::SETTING_ENABLED`]).
    glow: bool,
    /// Avatars in local reflection probes
    /// ([`crate::probes::PROBE_DYNAMIC_SETTING`]).
    probe_dynamic: bool,
    /// Realtime mirrors ([`crate::probes::RENDER_MIRRORS_SETTING`]).
    mirrors: bool,
}

/// The tier table [`apply_quality_tier`] writes through the store, indexed by
/// [`SETTING_RENDER_QUALITY`] (0 = low .. 6 = ultra). The ramps follow the
/// reference `featuretable.txt` reshaped to *this* viewer's stock defaults
/// (draw distance 512, mirrors on): the top tier reaches the defaults, the
/// low tiers shed the costly features first (shadows, dynamic probe content,
/// mirrors, glow). Tiers deliberately do **not** touch tone mapping /
/// exposure (aesthetic, not performance), the mirror resolution
/// (restart-scoped — a tier pick must apply instantly), vsync / the FPS cap
/// (user policy), or the cascade count.
const QUALITY_TIERS: [QualityTier; 7] = [
    QualityTier {
        far_clip: 64.0,
        lod_factor: 1.0,
        max_particles: 1024,
        shadow_detail: 0,
        shadow_map_size: 1024,
        glow: false,
        probe_dynamic: false,
        mirrors: false,
    },
    QualityTier {
        far_clip: 96.0,
        lod_factor: 1.0,
        max_particles: 2048,
        shadow_detail: 0,
        shadow_map_size: 1024,
        glow: true,
        probe_dynamic: false,
        mirrors: false,
    },
    QualityTier {
        far_clip: 128.0,
        lod_factor: 1.25,
        max_particles: 4096,
        shadow_detail: 0,
        shadow_map_size: 2048,
        glow: true,
        probe_dynamic: false,
        mirrors: false,
    },
    QualityTier {
        far_clip: 192.0,
        lod_factor: 1.5,
        max_particles: 4096,
        shadow_detail: 1,
        shadow_map_size: 2048,
        glow: true,
        probe_dynamic: true,
        mirrors: false,
    },
    QualityTier {
        far_clip: 256.0,
        lod_factor: 2.0,
        max_particles: 4096,
        shadow_detail: 1,
        shadow_map_size: 4096,
        glow: true,
        probe_dynamic: true,
        mirrors: false,
    },
    QualityTier {
        far_clip: 384.0,
        lod_factor: 3.0,
        max_particles: 8192,
        shadow_detail: 1,
        shadow_map_size: 4096,
        glow: true,
        probe_dynamic: true,
        mirrors: true,
    },
    QualityTier {
        far_clip: 512.0,
        lod_factor: 4.0,
        max_particles: 8192,
        shadow_detail: 1,
        shadow_map_size: 8192,
        glow: true,
        probe_dynamic: true,
        mirrors: true,
    },
];

/// The quality-tier combo's option keys, in tier order (must stay in step
/// with [`QUALITY_TIERS`] — a unit test pins the lengths equal). Shared with
/// the quick-preferences panel's quality row.
pub(crate) const QUALITY_OPTION_KEYS: [&str; 7] = [
    "preferences-quality-low",
    "preferences-quality-medium-low",
    "preferences-quality-medium",
    "preferences-quality-medium-high",
    "preferences-quality-high",
    "preferences-quality-high-ultra",
    "preferences-quality-ultra",
];

/// The [`SettingValue`]s a quality-tier combo's options write, in
/// [`QUALITY_OPTION_KEYS`] order — the tier indices as `U32`s. Shared by
/// this tab's row and the quick-preferences panel's.
pub(crate) fn quality_option_values() -> Vec<SettingValue> {
    (0..u32::try_from(QUALITY_TIERS.len()).unwrap_or(0))
        .map(SettingValue::U32)
        .collect()
}

/// Register the settings this tab itself consumes (see the module doc for
/// the ownership split). Called from [`ViewerSettings`]'s `load`.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        RENDER_SECTION,
        SETTING_SHADOW_DETAIL,
        SettingValue::U32(DEFAULT_SHADOW_DETAIL),
        "Shadow detail: 0 no shadows, 1 sun / moon shadows",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_SHADOW_MAP_SIZE,
        SettingValue::U32(DEFAULT_SHADOW_MAP_SIZE),
        "Directional shadow-map resolution (texels per side, a power of two)",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_SHADOW_CASCADES,
        SettingValue::U32(DEFAULT_SHADOW_CASCADES),
        "Sun shadow cascade count (1-4): fewer is faster, coarser in the distance",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_VSYNC,
        SettingValue::Bool(DEFAULT_VSYNC),
        "Sync presentation to the monitor refresh (vsync)",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_LIMIT_FRAMERATE,
        SettingValue::Bool(DEFAULT_LIMIT_FRAMERATE),
        "Cap the frame rate at FramePerSecondLimit (thermals / battery)",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_FPS_LIMIT,
        SettingValue::U32(DEFAULT_FPS_LIMIT),
        "The frame-rate cap, frames per second, while FSLimitFramerate is on",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_RENDER_QUALITY,
        SettingValue::U32(DEFAULT_RENDER_QUALITY),
        "The last-applied quality tier (0 low - 6 ultra); picking one writes \
         the tier's values into the individual render settings",
    );
}

/// Build the graphics tab's content into its panel (the
/// [`crate::preferences::PREF_TABS`] `build` hook).
pub(crate) fn build_graphics_tab(commands: &mut Commands, panel: Entity) {
    spawn_pref_section(commands, panel, "preferences-section-render-quality");
    let quality_options: Vec<(&str, SettingValue)> = QUALITY_OPTION_KEYS
        .iter()
        .copied()
        .zip(quality_option_values())
        .collect();
    let (_row, quality_anchor) = spawn_pref_combo_with_anchor(
        commands,
        panel,
        "preferences-row-render-quality",
        SettingBinding::global(SETTING_RENDER_QUALITY),
        &quality_options,
    );
    commands.entity(quality_anchor).insert(QualityTierControl);
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-draw-distance",
        SettingBinding::global(crate::session::SETTING_DRAW_DISTANCE),
        SliderRange::new(32.0, 1024.0),
        SliderStep(8.0),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-lod-factor",
        SettingBinding::global(crate::render_priority::SETTING_LOD_FACTOR),
        SliderRange::new(
            crate::render_priority::LOD_FACTOR_MIN,
            crate::render_priority::LOD_FACTOR_MAX,
        ),
        SliderStep(LOD_FACTOR_STEP),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-max-particles",
        SettingBinding::global(crate::particles::SETTING_MAX_PARTICLES),
        SliderRange::new(0.0, 8192.0),
        SliderStep(256.0),
    );

    spawn_pref_section(commands, panel, "preferences-section-shadows");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-shadow-detail",
        SettingBinding::global(SETTING_SHADOW_DETAIL),
        &[
            ("preferences-shadows-none", SettingValue::U32(0)),
            ("preferences-shadows-sun-moon", SettingValue::U32(1)),
        ],
    );
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-shadow-map-size",
        SettingBinding::global(SETTING_SHADOW_MAP_SIZE),
        &[
            ("preferences-shadow-map-1024", SettingValue::U32(1024)),
            ("preferences-shadow-map-2048", SettingValue::U32(2048)),
            ("preferences-shadow-map-4096", SettingValue::U32(4096)),
            ("preferences-shadow-map-8192", SettingValue::U32(8192)),
        ],
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-shadow-cascades",
        SettingBinding::global(SETTING_SHADOW_CASCADES),
        SliderRange::new(1.0, 4.0),
        SliderStep(1.0),
    );

    spawn_pref_section(commands, panel, "preferences-section-reflections");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-probe-dynamic",
        SettingBinding::global(crate::probes::PROBE_DYNAMIC_SETTING),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-mirrors",
        SettingBinding::global(crate::probes::RENDER_MIRRORS_SETTING),
    );
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-mirror-resolution",
        SettingBinding::global(crate::probes::HERO_RESOLUTION_SETTING),
        &[
            ("preferences-mirror-res-256", SettingValue::U32(256)),
            ("preferences-mirror-res-512", SettingValue::U32(512)),
            ("preferences-mirror-res-1024", SettingValue::U32(1024)),
            ("preferences-mirror-res-2048", SettingValue::U32(2048)),
        ],
    );
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-mirror-update-rate",
        SettingBinding::global(crate::probes::HERO_UPDATE_RATE_SETTING),
        &[
            ("preferences-mirror-rate-1", SettingValue::U32(1)),
            ("preferences-mirror-rate-2", SettingValue::U32(2)),
            ("preferences-mirror-rate-4", SettingValue::U32(4)),
            ("preferences-mirror-rate-8", SettingValue::U32(8)),
        ],
    );

    spawn_pref_section(commands, panel, "preferences-section-glow");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-glow",
        SettingBinding::global(crate::glow::SETTING_ENABLED),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-glow-strength",
        SettingBinding::global(crate::glow::SETTING_STRENGTH),
        SliderRange::new(0.0, GLOW_STRENGTH_MAX),
        SliderStep(GLOW_STRENGTH_STEP),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-glow-width",
        SettingBinding::global(crate::glow::SETTING_WIDTH),
        SliderRange::new(0.0, GLOW_WIDTH_MAX),
        SliderStep(GLOW_WIDTH_STEP),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-glow-iterations",
        SettingBinding::global(crate::glow::SETTING_ITERATIONS),
        SliderRange::new(GLOW_ITERATIONS_MIN, GLOW_ITERATIONS_MAX),
        SliderStep(1.0),
    );

    spawn_pref_section(commands, panel, "preferences-section-tonemap");
    spawn_pref_combo(
        commands,
        panel,
        "preferences-row-tonemap-type",
        SettingBinding::global(crate::tonemap::SETTING_TONEMAP_TYPE),
        &[
            (
                "preferences-tonemap-khronos",
                SettingValue::U32(crate::tonemap::TONEMAP_KHRONOS_NEUTRAL),
            ),
            (
                "preferences-tonemap-aces",
                SettingValue::U32(crate::tonemap::TONEMAP_ACES),
            ),
            (
                "preferences-tonemap-none",
                SettingValue::U32(crate::tonemap::TONEMAP_NONE),
            ),
        ],
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-tonemap-mix",
        SettingBinding::global(crate::tonemap::SETTING_TONEMAP_MIX),
        SliderRange::new(0.0, 1.0),
        SliderStep(TONEMAP_MIX_STEP),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-exposure",
        SettingBinding::global(crate::tonemap::SETTING_EXPOSURE),
        SliderRange::new(EXPOSURE_MIN, EXPOSURE_MAX),
        SliderStep(EXPOSURE_STEP),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-dynamic-exposure",
        SettingBinding::global(crate::exposure::SETTING_ENABLED),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-auto-adjust-legacy",
        SettingBinding::global(crate::exposure::SETTING_AUTO_ADJUST_LEGACY),
    );

    spawn_pref_section(commands, panel, "preferences-section-display");
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-vsync",
        SettingBinding::global(SETTING_VSYNC),
    );
    spawn_pref_checkbox(
        commands,
        panel,
        "preferences-row-limit-framerate",
        SettingBinding::global(SETTING_LIMIT_FRAMERATE),
    );
    spawn_pref_slider(
        commands,
        panel,
        "preferences-row-fps-limit",
        SettingBinding::global(SETTING_FPS_LIMIT),
        SliderRange::new(FPS_LIMIT_MIN, FPS_LIMIT_MAX),
        SliderStep(FPS_LIMIT_STEP),
    );
}

/// Apply [`SETTING_SHADOW_DETAIL`] to the scene sun's
/// `DirectionalLight::shadow_maps_enabled` (idempotent, writes only on
/// disagreement). The `SL_VIEWER_SUN_SHADOWS=0` experiment env is a hard
/// off that wins over the stored value.
fn apply_shadow_detail(
    settings: Option<Res<ViewerSettings>>,
    mut suns: Query<&mut DirectionalLight, With<SceneSun>>,
) {
    let Some(settings) = settings else {
        return;
    };
    let stored = settings
        .store()
        .get_u32(SETTING_SHADOW_DETAIL)
        .unwrap_or(DEFAULT_SHADOW_DETAIL);
    let want = crate::sky::sun_shadows_enabled() && stored > 0;
    for mut light in &mut suns {
        if light.shadow_maps_enabled != want {
            light.shadow_maps_enabled = want;
        }
    }
}

/// Apply [`SETTING_SHADOW_MAP_SIZE`] to the [`DirectionalLightShadowMap`]
/// resource (clamped to a power of two in
/// [`SHADOW_MAP_SIZE_MIN`]..=[`SHADOW_MAP_SIZE_MAX`]; the render world
/// re-extracts the resource each frame, so a change applies live).
///
/// [`crate::sky`]'s `SHADOW_MAP_SIZE` direction-snap step deliberately stays
/// at the 4096 constant: at 8192 the snap is one texel coarse, at 1024 one
/// texel fine — both imperceptible, and the snap tuning keeps its tests.
fn apply_shadow_map_size(
    settings: Option<Res<ViewerSettings>>,
    map: Option<ResMut<DirectionalLightShadowMap>>,
) {
    let (Some(settings), Some(mut map)) = (settings, map) else {
        return;
    };
    let stored = settings
        .store()
        .get_u32(SETTING_SHADOW_MAP_SIZE)
        .unwrap_or(DEFAULT_SHADOW_MAP_SIZE);
    let clamped = shadow_map_size_for(stored);
    let want = usize::try_from(clamped).unwrap_or(4096);
    if map.size != want {
        map.size = want;
    }
}

/// Clamp a stored shadow-map size to the selectable power-of-two range.
fn shadow_map_size_for(stored: u32) -> u32 {
    stored
        .clamp(SHADOW_MAP_SIZE_MIN, SHADOW_MAP_SIZE_MAX)
        .next_power_of_two()
        .min(SHADOW_MAP_SIZE_MAX)
}

/// Apply [`SETTING_SHADOW_CASCADES`] to the scene sun's
/// [`CascadeShadowConfig`], rebuilding it via
/// [`crate::sky::shadow_cascades_for`] when the cascade count disagrees. The
/// `SL_VIEWER_SHADOW_CASCADES` experiment env wins over the stored value.
fn apply_shadow_cascades(
    settings: Option<Res<ViewerSettings>>,
    mut suns: Query<&mut CascadeShadowConfig, With<SceneSun>>,
) {
    let Some(settings) = settings else {
        return;
    };
    let stored = settings
        .store()
        .get_u32(SETTING_SHADOW_CASCADES)
        .unwrap_or(DEFAULT_SHADOW_CASCADES)
        .clamp(SHADOW_CASCADES_MIN, SHADOW_CASCADES_MAX);
    let want =
        crate::sky::shadow_cascade_count().unwrap_or_else(|| usize::try_from(stored).unwrap_or(4));
    for mut config in &mut suns {
        if config.bounds.len() != want {
            *config = crate::sky::shadow_cascades_for(want);
        }
    }
}

/// Apply [`SETTING_VSYNC`] to the primary window's [`PresentMode`]
/// (idempotent). The startup `Fifo` default already *is* vsync, so it is
/// left untouched rather than rewritten to `AutoVsync` — a `Window` write
/// makes Bevy reconfigure the wgpu surface, which the launch frame should
/// not pay for nothing. `AutoNoVsync` falls back `Immediate` → `Mailbox` →
/// `Fifo`, so it degrades gracefully where unsupported (e.g. Wayland has no
/// `Immediate`).
fn apply_vsync(
    settings: Option<Res<ViewerSettings>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Some(settings) = settings else {
        return;
    };
    let want_vsync = settings
        .store()
        .get_bool(SETTING_VSYNC)
        .unwrap_or(DEFAULT_VSYNC);
    for mut window in &mut windows {
        let satisfied = if want_vsync {
            matches!(
                window.present_mode,
                PresentMode::AutoVsync | PresentMode::Fifo | PresentMode::FifoRelaxed
            )
        } else {
            matches!(
                window.present_mode,
                PresentMode::AutoNoVsync | PresentMode::Immediate | PresentMode::Mailbox
            )
        };
        if !satisfied {
            window.present_mode = if want_vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::AutoNoVsync
            };
        }
    }
}

/// How long [`limit_framerate`] should sleep: the frame budget (`1 / limit`
/// seconds) minus the time since the last frame boundary, `None` when the
/// cap is off, no boundary exists yet, or the frame already ran over budget.
fn frame_sleep(
    now: std::time::Instant,
    last: Option<std::time::Instant>,
    enabled: bool,
    limit: u32,
) -> Option<std::time::Duration> {
    if !enabled || limit == 0 {
        return None;
    }
    let budget = std::time::Duration::from_secs_f64(1.0 / f64::from(limit));
    let elapsed = now.duration_since(last?);
    budget
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
}

/// Cap the frame rate ([`SETTING_LIMIT_FRAMERATE`] / [`SETTING_FPS_LIMIT`])
/// by sleeping the tail of the main schedule — the `bevy_framepace`
/// mechanism. Runs in [`Last`]; the tracked frame boundary advances by the
/// budget (not the wake-up time), so the pace stays steady.
fn limit_framerate(
    settings: Option<Res<ViewerSettings>>,
    mut last_boundary: Local<Option<std::time::Instant>>,
) {
    let now = std::time::Instant::now();
    let (enabled, limit) = settings.as_ref().map_or((false, 0), |settings| {
        (
            settings
                .store()
                .get_bool(SETTING_LIMIT_FRAMERATE)
                .unwrap_or(DEFAULT_LIMIT_FRAMERATE),
            settings
                .store()
                .get_u32(SETTING_FPS_LIMIT)
                .unwrap_or(DEFAULT_FPS_LIMIT),
        )
    });
    match frame_sleep(now, *last_boundary, enabled, limit) {
        Some(remaining) => {
            std::thread::sleep(remaining);
            *last_boundary = now.checked_add(remaining);
        }
        None => *last_boundary = Some(now),
    }
}

/// Apply a **user** pick on a quality-tier combo (an entity carrying
/// [`QualityTierControl`]): write the picked [`QUALITY_TIERS`] row through
/// the store. Reacts to [`ComboChanged`] only — programmatic combo writes
/// (the preferences Cancel revert) emit none, so a revert never re-applies
/// a tier (see the module doc).
fn apply_quality_tier(
    mut changes: MessageReader<ComboChanged>,
    tier_combos: Query<(), With<QualityTierControl>>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    let Some(settings) = settings.as_mut() else {
        return;
    };
    for change in changes.read() {
        if tier_combos.get(change.combo).is_err() {
            continue;
        }
        let Some(tier) = QUALITY_TIERS.get(change.active) else {
            continue;
        };
        settings.set(
            Scope::Global,
            crate::session::SETTING_DRAW_DISTANCE,
            SettingValue::F32(tier.far_clip),
        );
        settings.set(
            Scope::Global,
            crate::render_priority::SETTING_LOD_FACTOR,
            SettingValue::F32(tier.lod_factor),
        );
        settings.set(
            Scope::Global,
            crate::particles::SETTING_MAX_PARTICLES,
            SettingValue::U32(tier.max_particles),
        );
        settings.set(
            Scope::Global,
            SETTING_SHADOW_DETAIL,
            SettingValue::U32(tier.shadow_detail),
        );
        settings.set(
            Scope::Global,
            SETTING_SHADOW_MAP_SIZE,
            SettingValue::U32(tier.shadow_map_size),
        );
        settings.set(
            Scope::Global,
            crate::glow::SETTING_ENABLED,
            SettingValue::Bool(tier.glow),
        );
        settings.set(
            Scope::Global,
            crate::probes::PROBE_DYNAMIC_SETTING,
            SettingValue::Bool(tier.probe_dynamic),
        );
        settings.set(
            Scope::Global,
            crate::probes::RENDER_MIRRORS_SETTING,
            SettingValue::Bool(tier.mirrors),
        );
    }
}

/// The graphics tab's runtime appliers (the tab *content* is built by the
/// shell through [`crate::preferences::PREF_TABS`]).
pub(crate) struct PreferencesGraphicsPlugin;

impl Plugin for PreferencesGraphicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                apply_shadow_detail,
                apply_shadow_map_size,
                apply_shadow_cascades,
                apply_vsync,
                apply_quality_tier,
            ),
        )
        .add_systems(Last, limit_framerate);
    }
}

#[cfg(test)]
mod tests {
    use bevy::light::DirectionalLightShadowMap;
    use bevy::prelude::*;
    use bevy::window::{PresentMode, PrimaryWindow};
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use std::time::{Duration, Instant};

    use super::{
        DEFAULT_FPS_LIMIT, DEFAULT_RENDER_QUALITY, DEFAULT_SHADOW_CASCADES, DEFAULT_SHADOW_DETAIL,
        DEFAULT_SHADOW_MAP_SIZE, DEFAULT_VSYNC, QUALITY_OPTION_KEYS, QUALITY_TIERS,
        QualityTierControl, SETTING_FPS_LIMIT, SETTING_LIMIT_FRAMERATE, SETTING_RENDER_QUALITY,
        SETTING_SHADOW_CASCADES, SETTING_SHADOW_DETAIL, SETTING_SHADOW_MAP_SIZE, SETTING_VSYNC,
        apply_quality_tier, apply_shadow_detail, apply_shadow_map_size, apply_vsync, frame_sleep,
        shadow_map_size_for,
    };
    use crate::settings::ViewerSettings;
    use crate::sky::SceneSun;
    use crate::ui_combo::ComboChanged;

    /// A headless app with the graphics (and tier-member) settings
    /// registered; each test adds the applier under test.
    fn graphics_app() -> App {
        let store = SettingsStore::new();
        let mut settings = ViewerSettings::from_store_for_test(store);
        super::register_settings(&mut settings);
        crate::session::register_settings(&mut settings);
        crate::particles::register_settings(&mut settings);
        crate::glow::register_settings(&mut settings);
        crate::render_priority::register_settings(&mut settings);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).insert_resource(settings);
        app
    }

    /// Write a global setting on the app's store.
    fn set(app: &mut App, name: &str, value: SettingValue) {
        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .set(Scope::Global, name, value);
    }

    #[test]
    fn quality_tiers_are_monotone() {
        assert_eq!(QUALITY_TIERS.len(), QUALITY_OPTION_KEYS.len());
        for pair in QUALITY_TIERS.windows(2) {
            let [lower, upper] = pair else {
                unreachable!("windows(2) yields pairs");
            };
            assert!(lower.far_clip <= upper.far_clip);
            assert!(lower.lod_factor <= upper.lod_factor);
            assert!(lower.max_particles <= upper.max_particles);
            assert!(lower.shadow_detail <= upper.shadow_detail);
            assert!(lower.shadow_map_size <= upper.shadow_map_size);
            assert!(!(lower.glow && !upper.glow));
            assert!(!(lower.probe_dynamic && !upper.probe_dynamic));
            assert!(!(lower.mirrors && !upper.mirrors));
        }
    }

    #[test]
    fn registered_defaults_match_consts() {
        let store = SettingsStore::new();
        let mut settings = ViewerSettings::from_store_for_test(store);
        super::register_settings(&mut settings);
        let store = settings.store();
        assert_eq!(
            store.get_u32(SETTING_SHADOW_DETAIL).ok(),
            Some(DEFAULT_SHADOW_DETAIL)
        );
        assert_eq!(
            store.get_u32(SETTING_SHADOW_MAP_SIZE).ok(),
            Some(DEFAULT_SHADOW_MAP_SIZE)
        );
        assert_eq!(
            store.get_u32(SETTING_SHADOW_CASCADES).ok(),
            Some(DEFAULT_SHADOW_CASCADES)
        );
        assert_eq!(store.get_bool(SETTING_VSYNC).ok(), Some(DEFAULT_VSYNC));
        assert_eq!(store.get_bool(SETTING_LIMIT_FRAMERATE).ok(), Some(false));
        assert_eq!(
            store.get_u32(SETTING_FPS_LIMIT).ok(),
            Some(DEFAULT_FPS_LIMIT)
        );
        assert_eq!(
            store.get_u32(SETTING_RENDER_QUALITY).ok(),
            Some(DEFAULT_RENDER_QUALITY)
        );
    }

    #[test]
    fn frame_sleep_cases() {
        let now = Instant::now();
        let budget_60 = Duration::from_secs_f64(1.0 / 60.0);
        // Disabled or zero limit: never sleeps.
        assert_eq!(frame_sleep(now, Some(now), false, 60), None);
        assert_eq!(frame_sleep(now, Some(now), true, 0), None);
        // No boundary yet (first frame): no sleep.
        assert_eq!(frame_sleep(now, None, true, 60), None);
        // Frame ran over budget: no sleep.
        let long_ago = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);
        assert_eq!(frame_sleep(now, Some(long_ago), true, 60), None);
        // Instant frame: sleeps the whole budget.
        assert_eq!(frame_sleep(now, Some(now), true, 60), Some(budget_60));
    }

    #[test]
    fn shadow_map_size_clamps_to_power_of_two() {
        assert_eq!(shadow_map_size_for(4096), 4096);
        assert_eq!(shadow_map_size_for(3000), 4096);
        assert_eq!(shadow_map_size_for(64), 1024);
        assert_eq!(shadow_map_size_for(u32::MAX), 8192);
        assert_eq!(shadow_map_size_for(5000), 8192);
    }

    #[test]
    fn shadow_detail_applier_toggles_scene_sun() {
        let mut app = graphics_app();
        app.add_systems(Update, apply_shadow_detail);
        let sun = app
            .world_mut()
            .spawn((
                DirectionalLight {
                    shadow_maps_enabled: true,
                    ..default()
                },
                SceneSun,
            ))
            .id();
        set(&mut app, SETTING_SHADOW_DETAIL, SettingValue::U32(0));
        app.update();
        let off = app
            .world()
            .entity(sun)
            .get::<DirectionalLight>()
            .map(|light| light.shadow_maps_enabled);
        assert_eq!(off, Some(false));
        set(&mut app, SETTING_SHADOW_DETAIL, SettingValue::U32(1));
        app.update();
        let on = app
            .world()
            .entity(sun)
            .get::<DirectionalLight>()
            .map(|light| light.shadow_maps_enabled);
        assert_eq!(on, Some(true));
    }

    #[test]
    fn shadow_map_size_applier_writes_resource() {
        let mut app = graphics_app();
        app.add_systems(Update, apply_shadow_map_size);
        app.insert_resource(DirectionalLightShadowMap { size: 4096 });
        set(&mut app, SETTING_SHADOW_MAP_SIZE, SettingValue::U32(8192));
        app.update();
        assert_eq!(
            app.world().resource::<DirectionalLightShadowMap>().size,
            8192
        );
        set(&mut app, SETTING_SHADOW_MAP_SIZE, SettingValue::U32(3000));
        app.update();
        assert_eq!(
            app.world().resource::<DirectionalLightShadowMap>().size,
            4096
        );
    }

    #[test]
    fn vsync_applier_switches_present_mode() {
        let mut app = graphics_app();
        app.add_systems(Update, apply_vsync);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        // The default (vsync on, `Fifo`) is already satisfied — no rewrite.
        app.update();
        let mode = app
            .world()
            .entity(window)
            .get::<Window>()
            .map(|w| w.present_mode);
        assert_eq!(mode, Some(PresentMode::Fifo));
        set(&mut app, SETTING_VSYNC, SettingValue::Bool(false));
        app.update();
        let mode = app
            .world()
            .entity(window)
            .get::<Window>()
            .map(|w| w.present_mode);
        assert_eq!(mode, Some(PresentMode::AutoNoVsync));
        set(&mut app, SETTING_VSYNC, SettingValue::Bool(true));
        app.update();
        let mode = app
            .world()
            .entity(window)
            .get::<Window>()
            .map(|w| w.present_mode);
        assert_eq!(mode, Some(PresentMode::AutoVsync));
    }

    #[test]
    fn quality_tier_applies_on_combo_change_only() {
        let mut app = graphics_app();
        app.add_systems(Update, apply_quality_tier);
        app.add_message::<ComboChanged>();
        let combo = app.world_mut().spawn(QualityTierControl).id();
        app.world_mut()
            .resource_mut::<Messages<ComboChanged>>()
            .write(ComboChanged { combo, active: 0 });
        app.update();
        {
            let settings = app.world().resource::<ViewerSettings>();
            let store = settings.store();
            assert_eq!(
                store.get_f32(crate::session::SETTING_DRAW_DISTANCE).ok(),
                Some(64.0)
            );
            assert_eq!(
                store
                    .get_f32(crate::render_priority::SETTING_LOD_FACTOR)
                    .ok(),
                Some(1.0)
            );
            assert_eq!(
                store.get_u32(crate::particles::SETTING_MAX_PARTICLES).ok(),
                Some(1024)
            );
            assert_eq!(store.get_u32(SETTING_SHADOW_DETAIL).ok(), Some(0));
            assert_eq!(
                store.get_bool(crate::glow::SETTING_ENABLED).ok(),
                Some(false)
            );
        }
        // A *store* write of the tier value (the Cancel-revert path) must not
        // re-apply the tier: hand-set a member setting, then write the tier
        // key directly and assert the member stays put.
        set(
            &mut app,
            crate::session::SETTING_DRAW_DISTANCE,
            SettingValue::F32(700.0),
        );
        set(&mut app, SETTING_RENDER_QUALITY, SettingValue::U32(0));
        app.update();
        let settings = app.world().resource::<ViewerSettings>();
        assert_eq!(
            settings
                .store()
                .get_f32(crate::session::SETTING_DRAW_DISTANCE)
                .ok(),
            Some(700.0)
        );
    }
}
