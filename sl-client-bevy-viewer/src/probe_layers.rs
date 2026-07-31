//! Render-layer assignment that keeps reflection-probe captures off the sun's
//! shadow path (viewer-perf-pipeline-specialization-stalls).
//!
//! Bevy builds directional-light shadow cascades for **every active camera**
//! (`build_directional_light_cascades` filters only on `is_active`), gates
//! shadow-*view* creation by *light-layers ∩ camera-layers* (`prepare_lights`),
//! and gates shadow-*casting* by *light-layers ∩ mesh-layers*
//! (`check_dir_light_mesh_visibility`). There is **no per-camera "disable
//! shadows" flag**, so the only way to stop the reflection-probe capture cameras
//! from generating (and, every capture cycle, re-specializing) a full set of sun
//! shadow cascades is to put them on render layers the shadow-casting sun does
//! not share — mirroring the reference viewer, whose probe captures render no sun
//! shadow maps at all (`generateSunShadow` is never called under `gCubeSnapshot`).
//!
//! The scheme keeps the **main view unchanged** — everything world-visible stays
//! on the default [`RenderLayers`] layer `0`, which the main camera and the
//! shadow-casting [`SceneSun`](crate::sky) already use — and *adds* a probe layer
//! to each renderable so the probe capture cameras (which are **not** on layer 0)
//! can see it:
//!
//! | content | layers | seen by |
//! | --- | --- | --- |
//! | environment (sky/water/terrain/clouds/discs/stars) | `0` + [`PROBE_ENV_LAYER`] | main, default probe, local probes |
//! | static world geometry (prims/meshes/sculpts/trees/grass) | `0` + [`PROBE_GEOM_LAYER`] | main, local probes |
//! | dynamic content (avatars, particles) | `0` + [`PROBE_DYNAMIC_LAYER`] | main, local probes *(when the setting includes it)* |
//!
//! The shadow-casting sun stays on layer `0` (so the main view is untouched and
//! world geometry still casts real shadows there); a **shadow-free mirror sun**
//! ([`crate::sky`]) sits on the three probe layers so probe captures are still
//! lit by the sun without any cascade being built for their cameras. The default
//! probe camera renders [`PROBE_ENV_LAYER`] only — the reference's environment-
//! only ambient probe — while local probe cameras also render
//! [`PROBE_GEOM_LAYER`] (and [`PROBE_DYNAMIC_LAYER`] per the runtime setting).
//!
//! Content is tagged with [`bevy::app::Propagate`] on subtree roots (Bevy 0.19
//! has no `RenderLayers` auto-propagation, but the viewer already runs
//! [`HierarchyPropagatePlugin::<RenderLayers>`](bevy::app::HierarchyPropagatePlugin));
//! a descendant with its own `Propagate` overrides from that point, which is how
//! a HUD attachment (routed under the HUD screen's own propagation) stays on the
//! HUD layer rather than a probe layer.

use bevy::camera::visibility::RenderLayers;

/// The default [`RenderLayers`] layer every world-visible entity and the main
/// camera already use. The shadow-casting [`SceneSun`](crate::sky) lives here, so
/// leaving the main view on this layer keeps its real-time shadows unchanged.
pub(crate) const MAIN_LAYER: usize = 0;

/// Render layer for **environment** geometry (sky / WL-sky / water / terrain /
/// clouds / sun-moon discs / stars) — the only content the reference's default
/// (ambient) probe captures.
pub(crate) const PROBE_ENV_LAYER: usize = 4;

/// Render layer for **static world geometry** (prims / meshes / sculpts / trees /
/// grass) — captured by local probes but not the environment-only default probe.
pub(crate) const PROBE_GEOM_LAYER: usize = 5;

/// Render layer for **dynamic content** (avatars, particles) whose per-frame
/// motion makes any probe that includes it re-render constantly. Split out so a
/// runtime setting can keep it out of local probes.
pub(crate) const PROBE_DYNAMIC_LAYER: usize = 6;

/// Render layers for static world geometry: the main layer plus
/// [`PROBE_GEOM_LAYER`].
pub(crate) fn world_geom_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(PROBE_GEOM_LAYER)
}

/// Render layers for environment geometry: the main layer plus
/// [`PROBE_ENV_LAYER`].
pub(crate) fn environment_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(PROBE_ENV_LAYER)
}

/// Render layers for dynamic content: the main layer plus
/// [`PROBE_DYNAMIC_LAYER`].
pub(crate) fn dynamic_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(PROBE_DYNAMIC_LAYER)
}

/// Render layers for the **shadow-free mirror sun**: all three probe layers, and
/// crucially **not** [`MAIN_LAYER`] — so it lights every probe capture camera but
/// never the main view (which the real shadow-casting sun already lights), and no
/// double-lighting results.
pub(crate) fn mirror_sun_render_layers() -> RenderLayers {
    RenderLayers::layer(PROBE_ENV_LAYER)
        .with(PROBE_GEOM_LAYER)
        .with(PROBE_DYNAMIC_LAYER)
}

/// The main layer plus all three probe layers — everything a renderable can be on.
/// Used by the headless render-readback / gallery harnesses, which build a
/// synthetic scene outside the real object / sky pipeline: propagating this onto
/// the scene root makes every mesh **and** the scene's own lights visible to (and
/// lighting) both the main camera and every probe capture camera, replacing the
/// real viewer's `Propagate` tagging + mirror sun that those harnesses do not run.
pub(crate) fn all_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER)
        .with(PROBE_ENV_LAYER)
        .with(PROBE_GEOM_LAYER)
        .with(PROBE_DYNAMIC_LAYER)
}

/// Render layers for the **default (ambient) probe** capture cameras: environment
/// only, and **not** [`MAIN_LAYER`] (so the shadow sun builds no cascades for
/// them). Mirrors the reference default probe, which renders only sky / water /
/// terrain / clouds.
pub(crate) const fn default_probe_camera_render_layers() -> RenderLayers {
    RenderLayers::layer(PROBE_ENV_LAYER)
}

/// Render layers for a **local probe** capture camera: environment plus static
/// world geometry, and — when `include_dynamic` — dynamic content. Never
/// [`MAIN_LAYER`], so the shadow sun builds no cascades for these cameras.
pub(crate) fn local_probe_camera_render_layers(include_dynamic: bool) -> RenderLayers {
    let layers = RenderLayers::layer(PROBE_ENV_LAYER).with(PROBE_GEOM_LAYER);
    if include_dynamic {
        layers.with(PROBE_DYNAMIC_LAYER)
    } else {
        layers
    }
}
