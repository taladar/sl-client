//! **In-world double-click teleport** — the Firestorm-style alternative to
//! click-to-walk: double-clicking bare ground teleports the avatar to the picked
//! point, within the current region or a visible neighbour.
//!
//! # The setting and its hotkey
//!
//! A persisted `DoubleClickAction` setting selects what a double-click on the
//! world does — `0` nothing, `1` teleport (this task), `2` walk (autopilot;
//! [[viewer-autopilot-click-to-walk]], stubbed here). Default is `0` (off), like
//! the reference. The reference's **Ctrl+Shift+D** hotkey (the `menu_viewer.xml`
//! "DoubleClick Teleport" `Advanced.SetDoubleClickAction teleport_to` shortcut)
//! toggles teleport on/off so the gesture can be enabled without opening
//! preferences.
//!
//! # What triggers
//!
//! Only a double-click that lands on **terrain** teleports — a click that
//! resolves to a scene object keeps its existing meaning (touch / edit), and a
//! click on an avatar or the sky is ignored. We cannot see an object's script
//! touch handlers from the client, so restricting the trigger to terrain is the
//! conservative reading of the reference's "bare ground / non-interactive
//! surface only" rule; object-surface teleport is left for a follow-up once
//! interactivity is known. UI panels and HUD attachments occlude the pick, so a
//! double-click on them never falls through to a teleport.
//!
//! The picked point is converted from Bevy world space back to the containing
//! region's Second Life frame ([`teleport_destination`], shared math with the
//! minimap via [`region_handle_at`](crate::minimap::region_handle_at)), and the
//! teleport is issued through the shared [`issue_teleport`] backend so it drives
//! the same progress overlay as the map surfaces.
//!
//! Reference (Firestorm, read-only): `lltoolpie` (double-click dispatch),
//! `llagent::teleportViaLocationLookAt`, setting `DoubleClickAction`.

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_settings::{Scope, SettingValue};
use std::collections::HashSet;

use sl_client_bevy::{RegionCoordinates, RegionHandle, SlCommand, SlIdentity, Vector};

use crate::camera::ViewerCamera;
use crate::coords::bevy_to_sl_vec;
use crate::edit_tool::edit_tool_inactive;
use crate::hud::{HudCamera, on_hud_layer};
use crate::hud_pick::{pointer_over_blocking_ui, pointer_over_hud};
use crate::input_context::InputContext;
use crate::minimap::{narrow, region_handle_at};
use crate::objects::SceneObject;
use crate::settings::ViewerSettings;
use crate::teleport_progress::{BeginTeleportFlow, TeleportTarget, issue_teleport};
use crate::terrain::TerrainSurface;

/// The persisted setting section — shared with other input-behaviour settings.
const INPUT_SECTION: &[&str] = &["input"];

/// The in-world double-click action setting name (mirrors the reference's
/// `DoubleClickAction`).
const SETTING_DOUBLE_CLICK_ACTION: &str = "DoubleClickAction";

/// The maximum interval (seconds) between the two clicks of a double-click, and
/// the maximum cursor travel (pixels) between them — matched to the minimap's.
const DOUBLE_CLICK_SECONDS: f64 = 0.4;

/// The maximum cursor travel (pixels) between the two clicks of a double-click.
const DOUBLE_CLICK_SLOP: f32 = 6.0;

/// The largest representable region-local coordinate (just inside 256 m), so an
/// arrival point exactly on a region's far edge stays inside the region.
const REGION_MAX_LOCAL: f32 = 255.99;

/// What a double-click on the world does, decoded from `DoubleClickAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorldDoubleClickAction {
    /// Do nothing (the default).
    Nothing,
    /// Teleport to the clicked point.
    Teleport,
    /// Walk to the clicked point via autopilot ([[viewer-autopilot-click-to-walk]],
    /// not yet wired — treated as nothing here).
    Walk,
}

impl WorldDoubleClickAction {
    /// Decode the persisted integer (`0` nothing, `1` teleport, `2` walk),
    /// treating anything else as nothing.
    const fn from_setting(value: i32) -> Self {
        match value {
            1 => Self::Teleport,
            2 => Self::Walk,
            _ => Self::Nothing,
        }
    }

    /// The persisted integer for this action.
    const fn to_setting(self) -> i32 {
        match self {
            Self::Nothing => 0,
            Self::Teleport => 1,
            Self::Walk => 2,
        }
    }
}

/// Register the double-click action setting (called from
/// [`crate::settings::ViewerSettings`]'s loader).
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        INPUT_SECTION,
        SETTING_DOUBLE_CLICK_ACTION,
        SettingValue::I32(WorldDoubleClickAction::Nothing.to_setting()),
        "In-world double-click action: 0 nothing, 1 teleport, 2 walk (autopilot)",
    );
}

/// The in-world double-click teleport plugin.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DoubleClickTeleportPlugin;

impl Plugin for DoubleClickTeleportPlugin {
    /// Wire the double-click detector and the enable/disable hotkey.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                toggle_double_click_teleport,
                world_double_click_teleport.run_if(edit_tool_inactive),
            ),
        );
    }
}

/// **Ctrl+Shift+D** (the reference hotkey) toggles the double-click teleport
/// action on/off (between `Teleport` and `Nothing`), so it can be enabled without
/// opening preferences.
fn toggle_double_click_teleport(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    mut settings: ResMut<ViewerSettings>,
) {
    // Never while a text field owns the keyboard (typing a `t`); a non-text
    // widget having focus must not block the global toggle, though.
    if matches!(*context, InputContext::TextEntry | InputContext::Media) {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if !(ctrl && shift && keyboard.just_pressed(KeyCode::KeyD)) {
        return;
    }
    let current = WorldDoubleClickAction::from_setting(
        settings
            .store()
            .get_i32(SETTING_DOUBLE_CLICK_ACTION)
            .unwrap_or(0),
    );
    let next = if current == WorldDoubleClickAction::Teleport {
        WorldDoubleClickAction::Nothing
    } else {
        WorldDoubleClickAction::Teleport
    };
    settings.set(
        Scope::Global,
        SETTING_DOUBLE_CLICK_ACTION,
        SettingValue::I32(next.to_setting()),
    );
    info!(
        "double-click teleport {}",
        if next == WorldDoubleClickAction::Teleport {
            "enabled"
        } else {
            "disabled"
        }
    );
}

/// The world ray-cast + hit-classification queries, grouped so the double-click
/// system stays within Bevy's system-parameter budget.
#[derive(SystemParam)]
struct WorldRay<'w, 's> {
    /// The perspective world camera the pick ray is cast from.
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// The orthographic HUD camera, for HUD-attachment occlusion.
    hud_cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<HudCamera>>,
    /// The ray caster.
    ray_cast: MeshRayCast<'w, 's>,
    /// Every render-layered entity, to split the HUD subtree from the world.
    layers: Query<'w, 's, (Entity, &'static RenderLayers)>,
    /// Terrain surfaces, so a hit can be confirmed as land.
    terrain: Query<'w, 's, (), With<TerrainSurface>>,
    /// Scene objects, so a hit resolving to one is treated as an object, not land.
    scene: Query<'w, 's, &'static SceneObject>,
    /// Parent links, walked up from a hit to its object / terrain root.
    parents: Query<'w, 's, &'static ChildOf>,
}

/// The UI-occlusion queries, grouped for the same reason.
#[derive(SystemParam)]
struct UiOcclusion<'w, 's> {
    /// The hovered `bevy_ui` nodes this frame.
    hover_map: Res<'w, HoverMap>,
    /// Their pickables (to read `should_block_lower`).
    pickables: Query<'w, 's, &'static Pickable>,
    /// Their laid-out sizes (to ignore zero-area hover entries).
    sizes: Query<'w, 's, &'static ComputedNode>,
}

/// Detect a double-click on bare ground and teleport there, when the
/// double-click action is `Teleport`.
#[expect(
    clippy::too_many_arguments,
    reason = "the input state, cursor window, identity, the grouped ray / occlusion \
              params, the click-timing local, and the two command channels"
)]
fn world_double_click_teleport(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    settings: Res<ViewerSettings>,
    windows: Query<&Window>,
    identity: Res<SlIdentity>,
    occlusion: UiOcclusion,
    mut ray: WorldRay,
    mut last_click: Local<Option<(f64, Vec2)>>,
    mut commands: MessageWriter<SlCommand>,
    mut begin: MessageWriter<BeginTeleportFlow>,
) {
    // A mouse gesture is independent of keyboard focus (a click on the world
    // still teleports while a floater holds the keyboard) — occlusion is handled
    // by the UI / HUD guards below, not by the keyboard-focus context. Only skip
    // a media face that has taken the pointer.
    if matches!(*context, InputContext::Media) {
        return;
    }
    let action = WorldDoubleClickAction::from_setting(
        settings
            .store()
            .get_i32(SETTING_DOUBLE_CLICK_ACTION)
            .unwrap_or(0),
    );
    if action != WorldDoubleClickAction::Teleport {
        return;
    }
    // A plain left-press; an Alt-held press is the camera focus gesture.
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    if !buttons.just_pressed(MouseButton::Left) || alt {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window
        .cursor_position()
        .unwrap_or_else(|| Vec2::new(window.width() * 0.5, window.height() * 0.5));

    // Track the double-click: the second qualifying press within the window.
    let now = time.elapsed_secs_f64();
    let double = last_click.is_some_and(|(at, position)| {
        now - at <= DOUBLE_CLICK_SECONDS && position.distance(cursor) <= DOUBLE_CLICK_SLOP
    });
    if !double {
        *last_click = Some((now, cursor));
        return;
    }
    *last_click = None;

    // Respect UI and HUD occlusion (this pick casts its own ray, bypassing
    // bevy_picking, so it must honour occlusion by hand).
    if pointer_over_blocking_ui(&occlusion.hover_map, &occlusion.pickables, &occlusion.sizes) {
        return;
    }
    if pointer_over_hud(cursor, &ray.hud_cameras, &ray.layers, &mut ray.ray_cast) {
        return;
    }

    // Cast the world ray (everything not on the HUD layer).
    let Ok((camera, camera_transform)) = ray.cameras.single() else {
        return;
    };
    let Ok(world_ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let hud_entities: HashSet<Entity> = ray
        .layers
        .iter()
        .filter(|(_entity, layers)| on_hud_layer(Some(layers)))
        .map(|(entity, _layers)| entity)
        .collect();
    let world_filter = |entity: Entity| !hud_entities.contains(&entity);
    let cast_settings = MeshRayCastSettings::default().with_filter(&world_filter);
    let Some((entity, hit)) = ray
        .ray_cast
        .cast_ray(world_ray, &cast_settings)
        .first()
        .cloned()
    else {
        return;
    };

    // Only bare ground teleports; an object / avatar / sky hit is left alone.
    if !is_terrain_hit(entity, &ray.terrain, &ray.scene, &ray.parents) {
        debug!("double-click teleport: hit {entity} is not terrain; ignored");
        return;
    }

    let Some(handle) = identity.region_handle else {
        debug!("double-click teleport: no region handle yet; ignored");
        return;
    };
    let point = bevy_to_sl_vec(hit.point);
    let forward = bevy_to_sl_vec(Vec3::from(world_ray.direction));
    let Some((destination, position, look_at)) = teleport_destination(handle, &point, &forward)
    else {
        return;
    };
    let label = format!(
        "{:.0}, {:.0}, {:.0}",
        position.x(),
        position.y(),
        position.z()
    );
    info!("double-click teleport → {destination:?} at {label}");
    issue_teleport(
        &mut commands,
        &mut begin,
        TeleportTarget {
            region_handle: destination,
            position,
            look_at,
        },
        Some(label),
    );
}

/// Whether a ray hit resolves to **terrain** (bare ground) rather than a scene
/// object, avatar, or the sky. Walks up the parent chain: a [`SceneObject`]
/// ancestor means an object (not terrain); a [`TerrainSurface`] means land.
fn is_terrain_hit(
    entity: Entity,
    terrain: &Query<(), With<TerrainSurface>>,
    scene: &Query<&SceneObject>,
    parents: &Query<&ChildOf>,
) -> bool {
    let mut current = entity;
    loop {
        if scene.get(current).is_ok() {
            return false;
        }
        if terrain.get(current).is_ok() {
            return true;
        }
        match parents.get(current) {
            Ok(child_of) => current = child_of.parent(),
            Err(_) => return false,
        }
    }
}

/// Resolve a picked point (in the current region's Second Life frame, metres
/// from its south-west corner — which may fall in a neighbour when beyond
/// `[0, 256)`) into a teleport destination: the containing region's handle, the
/// arrival position in that region's local frame, and a horizontal arrival
/// look-at from `forward`. `None` when the point is off the representable grid.
fn teleport_destination(
    current: RegionHandle,
    point: &Vector,
    forward: &Vector,
) -> Option<(RegionHandle, RegionCoordinates, Vector)> {
    let (region_east, region_north) = current.global_coordinates();
    let global_e = f64::from(region_east) + f64::from(point.x);
    let global_n = f64::from(region_north) + f64::from(point.y);
    let handle = region_handle_at(global_e, global_n)?;
    let (dest_east, dest_north) = handle.global_coordinates();
    let local_x = narrow(global_e - f64::from(dest_east)).clamp(0.0, REGION_MAX_LOCAL);
    let local_y = narrow(global_n - f64::from(dest_north)).clamp(0.0, REGION_MAX_LOCAL);
    Some((
        handle,
        RegionCoordinates::new(local_x, local_y, point.z),
        horizontal_look(forward),
    ))
}

/// A horizontal (level) unit look-at from a view direction, so the avatar faces
/// where the camera was pointing on arrival. Falls back to east on a degenerate
/// (near-vertical) direction.
fn horizontal_look(forward: &Vector) -> Vector {
    let length = forward.x.hypot(forward.y);
    if length > 1.0e-4 {
        Vector {
            x: forward.x / length,
            y: forward.y / length,
            z: 0.0,
        }
    } else {
        Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WorldDoubleClickAction, horizontal_look, teleport_destination};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{RegionHandle, Vector};

    /// The setting integer round-trips through the action enum, and unknown
    /// values decode to nothing.
    #[test]
    fn action_setting_round_trips() {
        for action in [
            WorldDoubleClickAction::Nothing,
            WorldDoubleClickAction::Teleport,
            WorldDoubleClickAction::Walk,
        ] {
            assert_eq!(
                WorldDoubleClickAction::from_setting(action.to_setting()),
                action
            );
        }
        assert_eq!(
            WorldDoubleClickAction::from_setting(99),
            WorldDoubleClickAction::Nothing,
            "an unknown value is off",
        );
    }

    /// A point inside the current region resolves to that region with the same
    /// local coordinates.
    #[test]
    fn destination_stays_in_the_current_region() -> Result<(), String> {
        // Region at grid (4, 4) → global corner (1024, 1024).
        let current = RegionHandle::from_grid(4, 4);
        let point = Vector {
            x: 128.0,
            y: 64.0,
            z: 30.0,
        };
        let forward = Vector {
            x: 1.0,
            y: 0.0,
            z: -1.0,
        };
        let (handle, position, _look) =
            teleport_destination(current, &point, &forward).ok_or("on-grid point resolves")?;
        assert_eq!(
            handle, current,
            "a point inside stays in the current region"
        );
        assert!((position.x() - 128.0).abs() < 0.01 && (position.y() - 64.0).abs() < 0.01);
        assert!(
            (position.z() - 30.0).abs() < 0.01,
            "the up-height is carried through"
        );
        Ok(())
    }

    /// A point east of the current region resolves to the eastern neighbour, with
    /// the local X wrapped into that region's frame.
    #[test]
    fn destination_crosses_into_the_eastern_neighbour() -> Result<(), String> {
        let current = RegionHandle::from_grid(4, 4);
        // 300 m east is 44 m into the region one east (grid 5, 4).
        let point = Vector {
            x: 300.0,
            y: 100.0,
            z: 25.0,
        };
        let forward = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let (handle, position, _look) =
            teleport_destination(current, &point, &forward).ok_or("neighbour point resolves")?;
        assert_eq!(
            handle,
            RegionHandle::from_grid(5, 4),
            "resolves to the east neighbour"
        );
        assert!(
            (position.x() - 44.0).abs() < 0.01,
            "local X wraps into the neighbour"
        );
        assert!((position.y() - 100.0).abs() < 0.01, "local Y is unchanged");
        Ok(())
    }

    /// The arrival look-at is a horizontal unit vector; a near-vertical view
    /// falls back to east rather than producing a zero look.
    #[test]
    fn look_at_is_horizontal_and_unit() {
        let look = horizontal_look(&Vector {
            x: 3.0,
            y: 4.0,
            z: -9.0,
        });
        assert!((look.z).abs() < 1.0e-6, "the look is levelled");
        assert!(
            (look.x.hypot(look.y) - 1.0).abs() < 1.0e-5,
            "the look is unit-length"
        );

        let degenerate = horizontal_look(&Vector {
            x: 0.0,
            y: 0.0,
            z: -1.0,
        });
        assert!(
            (degenerate.x - 1.0).abs() < 1.0e-6,
            "a vertical view falls back to east",
        );
        assert!(degenerate.y.abs() < 1.0e-6, "with no north component");
    }
}
