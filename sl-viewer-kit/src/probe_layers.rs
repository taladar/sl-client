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
//! shadow-casting `SceneSun` already use — and *adds* a probe layer
//! to each renderable so the probe capture cameras (which are **not** on layer 0)
//! can see it:
//!
//! | content | layers | seen by |
//! | --- | --- | --- |
//! | environment (sky/water/terrain/clouds/discs/stars) | `0` + [`PROBE_ENV_LAYER`] | main, default probe, local probes |
//! | static world geometry (prims/meshes/sculpts/trees/grass) | `0` + [`PROBE_GEOM_LAYER`] | main, local probes |
//! | dynamic content (avatars, particles) | `0` + [`PROBE_DYNAMIC_LAYER`] | main, local probes *(when the setting includes it)* |
//! | the own avatar's head in mouselook | [`SUN_SHADOW_ONLY_LAYER`] + [`PROBE_DYNAMIC_LAYER`] | the sun's shadow, local probes |
//!
//! The shadow-casting sun stays on layer `0` (so the main view is untouched and
//! world geometry still casts real shadows there), plus
//! [`SUN_SHADOW_ONLY_LAYER`], which no camera renders; a **shadow-free mirror sun**
//! (`sky`) sits on the three probe layers so probe captures are still
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
/// camera already use. The shadow-casting `SceneSun` lives here, so
/// leaving the main view on this layer keeps its real-time shadows unchanged.
pub const MAIN_LAYER: usize = 0;

/// Render layer for **environment** geometry (sky / WL-sky / water / terrain /
/// clouds / sun-moon discs / stars) — the only content the reference's default
/// (ambient) probe captures.
pub const PROBE_ENV_LAYER: usize = 4;

/// Render layer for **static world geometry** (prims / meshes / sculpts / trees /
/// grass) — captured by local probes but not the environment-only default probe.
pub const PROBE_GEOM_LAYER: usize = 5;

/// Render layer for **dynamic content** (avatars, particles) whose per-frame
/// motion makes any probe that includes it re-render constantly. Split out so a
/// runtime setting can keep it out of local probes.
pub const PROBE_DYNAMIC_LAYER: usize = 6;

/// Render layer for **water-exclusion surfaces** (the "invisiprim" successor,
/// `water_exclusion`): these faces render **only** here — never on
/// [`MAIN_LAYER`] or any probe layer — so they are invisible in every ordinary
/// view and are seen only by the water-exclusion mask camera, which renders this
/// layer alone into the screen-space mask the water shader samples.
pub const WATER_EXCLUSION_LAYER: usize = 7;

/// Render layer for geometry that **casts a sun shadow but is not drawn** in the
/// main view: the shadow-casting `SceneSun` is on it, the main camera is not.
///
/// It exists for the own avatar's head in mouselook. The reference skips the
/// head, hair, eyelashes and eyeballs when it draws the view from inside them,
/// but draws them in its shadow pass (`LLVOAvatar::renderSkinned` gates on
/// `LLAgent::needsRenderHead() || LLPipeline::sShadowRender`), so the avatar's
/// shadow keeps its head. A `Visibility::Hidden` part would lose the shadow too:
/// the directional caster cull (`shadow_visibility`) drops anything not
/// inherited-visible.
pub const SUN_SHADOW_ONLY_LAYER: usize = 8;

/// Render layers for static world geometry: the main layer plus
/// [`PROBE_GEOM_LAYER`].
#[must_use]
pub fn world_geom_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(PROBE_GEOM_LAYER)
}

/// Render layers for environment geometry: the main layer plus
/// [`PROBE_ENV_LAYER`].
#[must_use]
pub fn environment_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(PROBE_ENV_LAYER)
}

/// Render layers for dynamic content: the main layer plus
/// [`PROBE_DYNAMIC_LAYER`].
#[must_use]
pub fn dynamic_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(PROBE_DYNAMIC_LAYER)
}

/// Render layers for dynamic content that must **not** appear in the main view
/// but still casts a sun shadow and still shows in a probe that captures
/// dynamic content: [`PROBE_DYNAMIC_LAYER`] plus [`SUN_SHADOW_ONLY_LAYER`], and
/// no [`MAIN_LAYER`].
#[must_use]
pub fn dynamic_shadow_only_render_layers() -> RenderLayers {
    RenderLayers::layer(PROBE_DYNAMIC_LAYER).with(SUN_SHADOW_ONLY_LAYER)
}

/// Render layers for dynamic content that must appear in **neither** the main
/// view nor the sun's shadow, only in a probe that captures dynamic content:
/// [`PROBE_DYNAMIC_LAYER`] alone.
#[must_use]
pub const fn dynamic_probe_only_render_layers() -> RenderLayers {
    RenderLayers::layer(PROBE_DYNAMIC_LAYER)
}

/// Render layers for the **shadow-casting sun**: [`MAIN_LAYER`], so it lights
/// and shadows the main view exactly as a light with no layers of its own does,
/// plus [`SUN_SHADOW_ONLY_LAYER`], so what sits on that layer alone still casts
/// its shadow.
#[must_use]
pub fn scene_sun_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER).with(SUN_SHADOW_ONLY_LAYER)
}

/// Render layers for the **shadow-free mirror sun**: all three probe layers, and
/// crucially **not** [`MAIN_LAYER`] — so it lights every probe capture camera but
/// never the main view (which the real shadow-casting sun already lights), and no
/// double-lighting results.
#[must_use]
pub fn mirror_sun_render_layers() -> RenderLayers {
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
#[must_use]
pub fn all_render_layers() -> RenderLayers {
    RenderLayers::layer(MAIN_LAYER)
        .with(PROBE_ENV_LAYER)
        .with(PROBE_GEOM_LAYER)
        .with(PROBE_DYNAMIC_LAYER)
}

/// Render layers for the **default (ambient) probe** capture cameras: environment
/// only, and **not** [`MAIN_LAYER`] (so the shadow sun builds no cascades for
/// them). Mirrors the reference default probe, which renders only sky / water /
/// terrain / clouds.
#[must_use]
pub const fn default_probe_camera_render_layers() -> RenderLayers {
    RenderLayers::layer(PROBE_ENV_LAYER)
}

/// Render layers for a **local probe** capture camera: environment plus static
/// world geometry, and — when `include_dynamic` — dynamic content. Never
/// [`MAIN_LAYER`], so the shadow sun builds no cascades for these cameras.
#[must_use]
pub fn local_probe_camera_render_layers(include_dynamic: bool) -> RenderLayers {
    let layers = RenderLayers::layer(PROBE_ENV_LAYER).with(PROBE_GEOM_LAYER);
    if include_dynamic {
        layers.with(PROBE_DYNAMIC_LAYER)
    } else {
        layers
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::visibility::RenderLayers;
    use pretty_assertions::assert_eq;

    use super::{
        MAIN_LAYER, PROBE_DYNAMIC_LAYER, PROBE_ENV_LAYER, PROBE_GEOM_LAYER, SUN_SHADOW_ONLY_LAYER,
        WATER_EXCLUSION_LAYER, all_render_layers, default_probe_camera_render_layers,
        dynamic_probe_only_render_layers, dynamic_render_layers, dynamic_shadow_only_render_layers,
        environment_render_layers, local_probe_camera_render_layers, mirror_sun_render_layers,
        scene_sun_render_layers, world_geom_render_layers,
    };

    /// The main camera, which renders [`MAIN_LAYER`] and nothing else.
    fn main_camera() -> RenderLayers {
        RenderLayers::layer(MAIN_LAYER)
    }

    /// Every probe capture camera the scheme can build, named for the assertion
    /// messages.
    fn probe_cameras() -> [(&'static str, RenderLayers); 3] {
        [
            ("the default probe", default_probe_camera_render_layers()),
            ("a local probe", local_probe_camera_render_layers(false)),
            (
                "a local probe with dynamic content",
                local_probe_camera_render_layers(true),
            ),
        ]
    }

    /// Each kind of world-visible content and the layers it is tagged with.
    fn content() -> [(&'static str, RenderLayers); 3] {
        [
            ("environment", environment_render_layers()),
            ("static world geometry", world_geom_render_layers()),
            ("dynamic content", dynamic_render_layers()),
        ]
    }

    #[test]
    /// The invariant the whole module exists for: Bevy builds sun shadow
    /// cascades for every active camera whose layers meet the light's, and
    /// re-specializes them on every capture cycle. A probe camera sharing one
    /// layer with the shadow-casting sun is the pipeline stall this scheme was
    /// written to remove, and it would be invisible in a picture.
    fn no_probe_camera_shares_a_layer_with_the_shadow_casting_sun() {
        let sun = scene_sun_render_layers();
        for (name, camera) in probe_cameras() {
            assert!(
                !camera.intersects(&sun),
                "{name} meets the shadow-casting sun, so Bevy builds cascades for it"
            );
        }
    }

    #[test]
    /// The mirror sun is the other half of that trade: it must light every probe
    /// capture (or the captures go black) and must never reach the main view (or
    /// the main view is lit twice).
    fn the_mirror_sun_lights_every_probe_capture_and_never_the_main_view() {
        let mirror = mirror_sun_render_layers();
        for (name, camera) in probe_cameras() {
            assert!(
                camera.intersects(&mirror),
                "{name} is not lit by the shadow-free mirror sun"
            );
        }
        assert!(
            !mirror.intersects(&main_camera()),
            "the mirror sun double-lights the main view"
        );
    }

    #[test]
    /// The main view is the part the scheme promises to leave alone: everything
    /// world-visible still renders there, and the shadow-casting sun still
    /// lights it.
    fn every_kind_of_content_stays_in_the_main_view() {
        let sun = scene_sun_render_layers();
        for (name, layers) in content() {
            assert!(
                layers.intersects(&main_camera()),
                "{name} dropped out of the main view"
            );
            assert!(layers.intersects(&sun), "{name} lost its real-time shadow");
        }
    }

    #[test]
    /// Which probe captures which content: the default (ambient) probe is
    /// environment-only, mirroring the reference; a local probe adds static
    /// geometry, and dynamic content only when the runtime setting asks.
    fn each_probe_captures_exactly_the_content_it_is_meant_to() {
        let default_probe = default_probe_camera_render_layers();
        let [(_, environment), (_, geometry), (_, dynamic)] = content();

        assert!(environment.intersects(&default_probe));
        assert!(!geometry.intersects(&default_probe));
        assert!(!dynamic.intersects(&default_probe));

        let local = local_probe_camera_render_layers(false);
        assert!(environment.intersects(&local));
        assert!(geometry.intersects(&local));
        assert!(!dynamic.intersects(&local));

        assert!(dynamic.intersects(&local_probe_camera_render_layers(true)));
    }

    #[test]
    /// The own avatar's head in mouselook: not drawn, but still casting the
    /// shadow the reference keeps (`renderSkinned` gates on
    /// `needsRenderHead() || sShadowRender`). A `Visibility::Hidden` head would
    /// lose the shadow with the picture, which is why it is a layer and not a
    /// visibility flag.
    fn the_mouselook_head_casts_a_sun_shadow_without_being_drawn() {
        let head = dynamic_shadow_only_render_layers();
        assert!(
            !head.intersects(&main_camera()),
            "the mouselook head is drawn in the view from inside it"
        );
        assert!(
            head.intersects(&scene_sun_render_layers()),
            "the mouselook head casts no sun shadow"
        );
        assert!(
            head.intersects(&local_probe_camera_render_layers(true)),
            "the mouselook head vanishes from a probe that captures dynamic content"
        );
    }

    #[test]
    /// Its counterpart, for content that must show in a probe and nowhere else:
    /// neither drawn nor shadow-casting.
    fn probe_only_dynamic_content_is_neither_drawn_nor_shadow_casting() {
        let probe_only = dynamic_probe_only_render_layers();
        assert!(!probe_only.intersects(&main_camera()));
        assert!(!probe_only.intersects(&scene_sun_render_layers()));
        assert!(probe_only.intersects(&local_probe_camera_render_layers(true)));
    }

    #[test]
    /// Water-exclusion surfaces render **only** to the mask camera: they are
    /// invisible in the main view, in every probe, and in the sun's shadow — a
    /// leak into any of those is a visible black hole in the world.
    fn water_exclusion_surfaces_are_invisible_to_every_ordinary_view() {
        let exclusion = RenderLayers::layer(WATER_EXCLUSION_LAYER);
        assert!(!exclusion.intersects(&main_camera()));
        assert!(!exclusion.intersects(&scene_sun_render_layers()));
        assert!(!exclusion.intersects(&mirror_sun_render_layers()));
        assert!(
            !exclusion.intersects(&all_render_layers()),
            "the harness layer set would draw water-exclusion surfaces"
        );
        for (name, camera) in probe_cameras() {
            assert!(
                !exclusion.intersects(&camera),
                "{name} captures a water-exclusion surface"
            );
        }
    }

    #[test]
    /// The headless harnesses build a synthetic scene outside the real pipeline
    /// and propagate one layer set onto its root, so that set has to reach the
    /// main camera and every probe camera at once.
    fn the_harness_layer_set_reaches_the_main_camera_and_every_probe() {
        let all = all_render_layers();
        assert!(all.intersects(&main_camera()));
        for (name, camera) in probe_cameras() {
            assert!(
                all.intersects(&camera),
                "{name} sees nothing in a harness scene"
            );
        }
    }

    #[test]
    /// Two roles sharing a layer number would silently merge them — the failure
    /// every assertion above is written against, caught at its source.
    fn every_role_has_its_own_layer() {
        let roles = [
            MAIN_LAYER,
            PROBE_ENV_LAYER,
            PROBE_GEOM_LAYER,
            PROBE_DYNAMIC_LAYER,
            WATER_EXCLUSION_LAYER,
            SUN_SHADOW_ONLY_LAYER,
        ];
        let mut distinct = roles.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            roles.len(),
            "two roles share a render layer"
        );
    }
}
