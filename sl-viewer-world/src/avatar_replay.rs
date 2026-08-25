//! Avatar-state **replay** runtime (viewer-avatar-state-dump-replay): render a
//! captured avatar bundle offline, with no grid.
//!
//! Given a bundle directory (`--replay <dir>`), `crate::run` points the asset
//! stores at the bundle's drop-in `cache/` (via
//! [`crate::paths::set_replay_cache_root`]) and runs the *normal* viewer app with
//! [`SlClientPlugin`](sl_client_bevy::SlClientPlugin) in **offline** mode, plus
//! the systems here. [`inject_replay_bundle`] then, once, feeds the session the
//! captured events — a synthetic [`SlCapabilities`] (so the cap-gated asset
//! managers serve from the bundle cache), each avatar object and its attachment
//! tree, each [`AvatarAppearance`](sl_client_bevy::AvatarAppearance), and each
//! avatar's animations — so the viewer's live render pipeline draws the avatar
//! exactly as a login would. That is the whole point: a rendering **fix** can be
//! tested against the same captured inputs, repeatably, after the avatar is gone.
//!
//! A small **test rig** can be added around the avatar to exercise material paths
//! a bare void does not: an orbiting local light (`--replay-orbit-light`, sweeps
//! specular highlights) and a local reflection probe (`--replay-reflection-probe`,
//! feeds image-based lighting to materials that need it).

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_VIEWER_ASSET, ReflectionProbe,
    ReflectionProbeFlags, SlCapabilities, SlEvent, SlIdentity, SlSessionEvent,
};

use crate::probes::ObjectReflectionProbe;
use crate::replay_bundle::ReplayManifest;
use crate::world_api::AvatarState;

/// The replay configuration and injection latch: the loaded avatar manifests, the
/// primary avatar the test rig centres on, and which rig extras to spawn.
#[derive(Debug, Resource)]
pub struct ReplayConfig {
    /// The avatar manifests loaded from the bundle (one per captured avatar).
    manifests: Vec<ReplayManifest>,
    /// The first captured avatar — the one the orbit light / reflection probe
    /// follow, and whose region the world origin is set to.
    primary_agent: Option<AgentKey>,
    /// Whether to spawn the orbiting test light (`--replay-orbit-light`).
    orbit_light: bool,
    /// Whether to spawn the local test reflection probe (`--replay-reflection-probe`).
    reflection_probe: bool,
    /// Set once the events have been injected, so [`inject_replay_bundle`] fires
    /// exactly one time.
    injected: bool,
}

impl ReplayConfig {
    /// Build the config from the loaded manifests and the rig flags, resolving the
    /// primary avatar (the first manifest carrying an avatar object).
    #[must_use]
    pub fn new(manifests: Vec<ReplayManifest>, orbit_light: bool, reflection_probe: bool) -> Self {
        let primary_agent = manifests
            .iter()
            .find_map(|manifest| manifest.avatar_object())
            .map(|object| AgentKey::from(object.full_id.uuid()));
        Self {
            manifests,
            primary_agent,
            orbit_light,
            reflection_probe,
            injected: false,
        }
    }

    /// The Second Life region-local position of the primary avatar's object (Z-up
    /// metres), for framing the camera before the world exists. `None` when no
    /// avatar object was captured.
    pub fn primary_position(&self) -> Option<Vec3> {
        let object = self
            .manifests
            .iter()
            .find_map(ReplayManifest::avatar_object)?;
        Some(crate::coords::sl_to_bevy_vec(&object.motion.position))
    }
}

/// The angular speed of the orbiting test light, in radians per second.
const ORBIT_RATE: f32 = 0.8;
/// The orbit radius of the test light around the avatar, in metres.
const ORBIT_RADIUS: f32 = 3.0;
/// The orbit height of the test light above the avatar's anchor, in metres.
const ORBIT_HEIGHT: f32 = 1.4;
/// The luminous power of the orbiting test light, in lumens — bright enough to
/// throw a clear specular highlight across a mesh surface.
const LIGHT_INTENSITY: f32 = 2_000_000.0;
/// The reach of the orbiting test light, in metres.
const LIGHT_RANGE: f32 = 40.0;
/// The diameter of the local test reflection probe's influence volume, in metres
/// — a sphere comfortably enclosing a standing avatar.
const PROBE_DIAMETER: f32 = 6.0;

/// A marker on the orbiting test light, carrying its current orbit angle.
#[derive(Debug, Component)]
pub struct ReplayOrbitLight {
    /// The current orbit angle, in radians, advanced each frame.
    angle: f32,
}

/// A marker on the local test reflection probe, so it can be re-centred on the
/// avatar each frame.
#[derive(Debug, Component)]
pub struct ReplayProbeFollower;

/// Inject the captured session events once, so the live render systems draw the
/// avatar. Also sets the world identity (region / circuit) from the primary
/// avatar and spawns the optional test rig.
pub fn inject_replay_bundle(
    mut config: ResMut<ReplayConfig>,
    mut identity: ResMut<SlIdentity>,
    mut events: MessageWriter<SlEvent>,
    mut capabilities: MessageWriter<SlCapabilities>,
    mut commands: Commands,
) {
    if config.injected {
        return;
    }
    config.injected = true;

    // Synthetic capabilities: any non-empty URL flips the four asset managers'
    // cap gate open, so each request is served from the bundle's drop-in cache
    // (the placeholder URL is only ever reached on a genuine cache miss).
    let placeholder = "http://127.0.0.1:0/replay".to_owned();
    let mut caps = std::collections::HashMap::new();
    for cap in [
        CAP_GET_TEXTURE,
        CAP_GET_MESH,
        CAP_GET_MESH2,
        CAP_VIEWER_ASSET,
    ] {
        let _previous = caps.insert(cap.to_owned(), placeholder.clone());
    }
    capabilities.write(SlCapabilities(caps));

    // Seed the world origin from the primary avatar's object, so the terrain /
    // coordinate origin resolves (no `RegionHandshake` is sent offline). The
    // agent id is left unset so every injected avatar renders through the faithful
    // "other avatar" path rather than the own-avatar shortcuts.
    if let Some(object) = config
        .manifests
        .iter()
        .find_map(ReplayManifest::avatar_object)
    {
        identity.region_handle = Some(object.region_handle);
        identity.circuit_id = Some(object.circuit);
    }

    // Re-emit the captured events verbatim: the avatar objects + their attachment
    // trees (spawn + track), the appearances (shape / bakes / part visibility),
    // and the animation sets (pose).
    let mut avatars = 0_u32;
    for manifest in &config.manifests {
        for object in &manifest.objects {
            events.write(SlEvent(SlSessionEvent::ObjectAdded(Box::new(
                object.clone(),
            ))));
        }
        if let Some(appearance) = &manifest.appearance {
            events.write(SlEvent(SlSessionEvent::AvatarAppearance(Box::new(
                appearance.clone(),
            ))));
        }
        if !manifest.animations.is_empty() {
            events.write(SlEvent(SlSessionEvent::AvatarAnimation {
                avatar_id: AgentKey::from(manifest.agent),
                animations: manifest.animations.clone(),
                physical_events: Vec::new(),
            }));
        }
        // Legacy `LLMaterial`s cannot be fetched offline (the `RenderMaterials`
        // cap is unreachable), so re-emit the captured entries directly.
        if !manifest.render_materials.is_empty() {
            events.write(SlEvent(SlSessionEvent::RenderMaterials(
                manifest.render_materials.clone(),
            )));
        }
        avatars = avatars.saturating_add(1);
    }
    info!("avatar replay: injected {avatars} avatar(s) from the bundle");

    if config.orbit_light {
        commands.spawn((
            PointLight {
                intensity: LIGHT_INTENSITY,
                range: LIGHT_RANGE,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_translation(Vec3::new(ORBIT_RADIUS, ORBIT_HEIGHT, 0.0)),
            ReplayOrbitLight { angle: 0.0 },
        ));
    }
    if config.reflection_probe {
        // A dynamic sphere probe (renders the avatar into its own capture), lifted
        // onto the same `ObjectReflectionProbe` component the live prim-probe
        // pipeline captures — so replay exercises the real image-based-lighting path.
        commands.spawn((
            Transform::from_translation(Vec3::new(0.0, ORBIT_HEIGHT, 0.0)),
            Visibility::default(),
            ObjectReflectionProbe {
                data: ReflectionProbe {
                    ambiance: 0.0,
                    clip_distance: 0.1,
                    flags: ReflectionProbeFlags::DYNAMIC,
                },
                scale: [PROBE_DIAMETER, PROBE_DIAMETER, PROBE_DIAMETER],
            },
            ReplayProbeFollower,
        ));
    }
}

/// The world-space centre the test rig orbits / follows: the primary avatar's
/// anchor, once it has a propagated world transform.
fn rig_center(
    config: &ReplayConfig,
    state: &AvatarState,
    anchors: &Query<&GlobalTransform>,
) -> Option<Vec3> {
    let agent = config.primary_agent?;
    let anchor = state.anchor_of(agent)?;
    anchors.get(anchor).ok().map(GlobalTransform::translation)
}

/// Advance the orbiting test light around the avatar each frame (a slow specular
/// sweep). A no-op until the avatar has a world transform to orbit.
pub fn drive_replay_orbit_light(
    time: Res<Time>,
    config: Res<ReplayConfig>,
    state: Res<AvatarState>,
    anchors: Query<&GlobalTransform>,
    mut lights: Query<(&mut Transform, &mut ReplayOrbitLight)>,
) {
    let Some(center) = rig_center(&config, &state, &anchors) else {
        return;
    };
    let delta = time.delta_secs();
    for (mut transform, mut light) in &mut lights {
        light.angle = ORBIT_RATE.mul_add(delta, light.angle);
        // Component-wise (glam's vector `+` trips the workspace
        // `arithmetic_side_effects` lint).
        transform.translation = Vec3::new(
            ORBIT_RADIUS.mul_add(light.angle.cos(), center.x),
            center.y + ORBIT_HEIGHT,
            ORBIT_RADIUS.mul_add(light.angle.sin(), center.z),
        );
    }
}

/// Keep the local test reflection probe centred on the avatar each frame.
pub fn follow_replay_probe(
    config: Res<ReplayConfig>,
    state: Res<AvatarState>,
    anchors: Query<&GlobalTransform>,
    mut probes: Query<&mut Transform, With<ReplayProbeFollower>>,
) {
    let Some(center) = rig_center(&config, &state, &anchors) else {
        return;
    };
    for mut transform in &mut probes {
        transform.translation = Vec3::new(center.x, center.y + ORBIT_HEIGHT, center.z);
    }
}

#[cfg(test)]
mod tests {
    use super::ReplayConfig;

    #[test]
    fn empty_bundle_has_no_primary_avatar() {
        let config = ReplayConfig::new(Vec::new(), false, false);
        assert!(config.primary_agent.is_none());
        assert!(config.primary_position().is_none());
        assert!(!config.injected);
    }
}
