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
//! The reference's rule (`LLToolPie::teleportToClickedLocation`,
//! `lltoolpie.cpp`, with Firestorm's FIRE-1765 extension), applied to what the
//! double-click's pick names ([`lands`]):
//!
//! - **terrain** always teleports; so does **water**, to the ground under it —
//!   the reference's pick looks through water at the land
//!   ([`ground_under_water`]);
//! - **another avatar** teleports, to the point on it the pick hit; one's
//!   **own** avatar, and anything it wears, never does;
//! - an **in-world object** teleports when it takes no click of its own — no
//!   click action and no touch script ([`FLAGS_HANDLE_TOUCH`] on the prim or its
//!   root) — or, with `FSAllowDoubleClickOnScriptedObjects` (on by default, as
//!   in Firestorm), whenever its own click action is not *sit*. A click action
//!   of *disabled* ("None" in the build floater) counts as no click action;
//! - a **HUD** attachment never teleports (and its pick never reaches here:
//!   UI panels and HUDs occlude the world pick, so a double-click on them never
//!   falls through).
//!
//! The arrival height is the picked point plus the own avatar's pelvis-to-foot
//! height, as the reference adds `getPelvisToFoot` — the position a teleport
//! names is where the agent's pelvis goes.
//!
//! RLVa's `@tplocal` / `@sittp` veto is not applied here yet: the viewer's send
//! paths do not consult `RlvActions` at all, and wiring them — this teleport
//! among them — is [[viewer-rlv-send-side-consumers]].
//!
//! The picked point is converted from Bevy world space back to the containing
//! region's Second Life frame ([`teleport_destination`], shared math with the
//! minimap via [`region_handle_at`](crate::minimap::region_handle_at)), and the
//! teleport is issued through the shared [`issue_teleport`] backend so it drives
//! the same progress overlay as the map surfaces.
//!
//! Reference (Firestorm, read-only): `lltoolpie` (double-click dispatch),
//! `llagent::teleportViaLocationLookAt`, setting `DoubleClickAction`.

use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sl_settings::{Scope, SettingValue};

use sl_client_bevy::{AgentKey, RegionCoordinates, RegionHandle, SlCommand, SlIdentity, Vector};

use crate::avatar_assets::AvatarAssetLibrary;
use crate::coords::bevy_to_sl_vec;
use crate::edit_tool::edit_tool_inactive;
use crate::gpu_pick::{GpuPickResolved, GpuPicker, PickPurpose, PickResolution};
use crate::hud_pick::HudRayCast;
use crate::intents::{BeginTeleportFlow, TeleportTarget, issue_teleport};
use crate::minimap::{narrow, region_handle_at};
use crate::settings::ViewerSettings;
use crate::world_api::pointer_over_blocking_ui;
use crate::world_api::{
    AvatarState, FLAGS_HANDLE_TOUCH, InputContext, ObjectPickSummary, ObjectState,
    SETTING_DOUBLE_CLICK_ACTION, SETTING_DOUBLE_CLICK_SCRIPTED_OBJECTS, TerrainState,
};

/// The persisted setting section — shared with other input-behaviour settings.
const INPUT_SECTION: &[&str] = &["input"];

/// The maximum interval (seconds) between the two clicks of a double-click, and
/// the maximum cursor travel (pixels) between them — matched to the minimap's.
const DOUBLE_CLICK_SECONDS: f64 = 0.4;

/// The maximum cursor travel (pixels) between the two clicks of a double-click.
const DOUBLE_CLICK_SLOP: f32 = 6.0;

/// The largest representable region-local coordinate (just inside 256 m), so an
/// arrival point exactly on a region's far edge stays inside the region.
const REGION_MAX_LOCAL: f32 = 255.99;

/// The `click_action` byte of an object a click sits on (`CLICK_ACTION_SIT`).
const CLICK_ACTION_SIT: u8 = 1;

/// The `click_action` byte the build floater calls "None"
/// (`CLICK_ACTION_DISABLED`): no click action, and it masks the root's.
const CLICK_ACTION_DISABLED: u8 = 8;

/// The step, metres, of the march along the pick ray from a water hit down to
/// the ground under it.
const UNDERWATER_STEP: f32 = 0.25;

/// How far past the water surface the march looks for the ground, metres —
/// the deepest a region's ground can lie under its water.
const UNDERWATER_REACH: f32 = 512.0;

/// The bisection rounds that refine a marched crossing into the ground: each
/// halves the step's uncertainty, so eight leave a millimetre.
const UNDERWATER_BISECTIONS: u32 = 8;

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
        "setting-desc-DoubleClickAction",
    );
    settings.register_in(
        INPUT_SECTION,
        SETTING_DOUBLE_CLICK_SCRIPTED_OBJECTS,
        SettingValue::Bool(true),
        "setting-desc-FSAllowDoubleClickOnScriptedObjects",
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
                resolve_double_click_teleport,
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

/// What gates a double-click teleport, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the mouse and modifier keys,
/// the focus context that says the click is the world's, the setting that
/// switches the gesture on, and the window the cursor is read from.
#[derive(Debug, bevy::ecs::system::SystemParam)]
struct TeleportGate<'w, 's> {
    /// The mouse buttons.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The modifier keys.
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// Where input is going; only a world click teleports.
    context: Res<'w, InputContext>,
    /// The setting that switches the gesture on.
    settings: Res<'w, ViewerSettings>,
    /// The window the cursor position is read from.
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

/// Detect a double-click and request the GPU ID-buffer pick under it (when
/// the double-click action is `Teleport`); [`resolve_double_click_teleport`]
/// issues the teleport if the readback names bare ground.
fn world_double_click_teleport(
    time: Res<Time>,
    gate: TeleportGate,
    occlusion: UiOcclusion,
    hud: HudRayCast,
    mut picker: ResMut<GpuPicker>,
    mut last_click: Local<Option<(f64, Vec2)>>,
) {
    let TeleportGate {
        buttons,
        keyboard,
        context,
        settings,
        windows,
    } = gate;
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
    if hud.over(cursor) {
        return;
    }

    // Request the GPU ID-buffer pick; the resolve system fires the teleport
    // if the readback names bare ground.
    picker.request(cursor, PickPurpose::DoubleClick);
}

/// What a double-click's pick landed on, as far as the teleport rule cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// Bare ground.
    Terrain,
    /// A water surface; the teleport goes to the ground under it.
    Water,
    /// An avatar's body.
    Avatar {
        /// Whether it is the agent's own.
        own: bool,
    },
    /// A prim: an in-world object's, or one an avatar wears (a rigid
    /// attachment, or a rigged one drawn on its wearer's body).
    Object(ObjectClick),
}

/// The facts about a picked prim the double-click rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObjectClick {
    /// The picked prim's own click action.
    picked_action: u8,
    /// Its root's click action (the attachment root's for a worn prim).
    root_action: u8,
    /// Whether a script in the prim or its root handles touches.
    handles_touch: bool,
    /// Whether, and by whom, it is worn.
    worn: Worn,
}

/// Whether a picked prim is worn, and by whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Worn {
    /// Nobody wears it: an in-world object.
    No,
    /// Another avatar wears it.
    ByOther,
    /// The agent itself wears it.
    ByMe,
    /// It is worn on a HUD point.
    Hud,
}

impl ObjectClick {
    /// The facts of `summary`, the pick summary of the picked prim; `own` is
    /// whether the agent wears it, `hud` whether on a HUD point.
    const fn of(summary: &ObjectPickSummary, own: bool, hud: bool) -> Self {
        let worn = match (summary.attachment, hud, own) {
            (false, _, _) => Worn::No,
            (true, true, _) => Worn::Hud,
            (true, false, true) => Worn::ByMe,
            (true, false, false) => Worn::ByOther,
        };
        Self {
            picked_action: summary.picked_click_action,
            root_action: summary.root_click_action,
            handles_touch: summary.flags & FLAGS_HANDLE_TOUCH != 0,
            worn,
        }
    }

    /// The click action a left click on it carries out — the reference's
    /// `final_click_action`: none for an attachment, the prim's own when it
    /// has one or the root disables its own, the root's otherwise.
    const fn final_action(self) -> u8 {
        if !matches!(self.worn, Worn::No) {
            0
        } else if self.root_action == CLICK_ACTION_DISABLED || self.picked_action != 0 {
            self.picked_action
        } else {
            self.root_action
        }
    }

    /// Whether a left click on it does something of its own: a click action
    /// or a touch script, unless the click action is *disabled*.
    const fn takes_a_click(self) -> bool {
        let action = self.final_action();
        (action != 0 || self.handles_touch) && action != CLICK_ACTION_DISABLED
    }
}

/// Whether a double-click on `surface` teleports — the reference's
/// `teleportToClickedLocation` rule, `scripted_objects_too` its
/// `FSAllowDoubleClickOnScriptedObjects`.
const fn lands(surface: Surface, scripted_objects_too: bool) -> bool {
    match surface {
        Surface::Terrain | Surface::Water => true,
        Surface::Avatar { own } => !own,
        Surface::Object(object) => {
            if matches!(object.worn, Worn::ByMe | Worn::Hud) {
                return false;
            }
            !object.takes_a_click()
                || (scripted_objects_too && object.picked_action != CLICK_ACTION_SIT)
        }
    }
}

/// Where the world stands for a double-click's resolution: what was picked,
/// who the agent is, the ground, and the agent's body.
#[derive(SystemParam)]
struct Landing<'w> {
    /// The agent and its region.
    identity: Res<'w, SlIdentity>,
    /// The tracked objects, for a picked prim's click facts.
    objects: Res<'w, ObjectState>,
    /// The avatars, for whose body or attachment was picked.
    avatars: Res<'w, AvatarState>,
    /// The ground heights, for the ground under a water hit.
    terrain: Res<'w, TerrainState>,
    /// The settings: whether scripted objects take a double-click teleport.
    settings: Res<'w, ViewerSettings>,
    /// The skeleton the own avatar's pelvis-to-foot height is measured on;
    /// absent without the avatar assets.
    library: Option<Res<'w, AvatarAssetLibrary>>,
}

impl Landing<'_> {
    /// What the pick `resolution` names, as the rule sees it; `None` for an
    /// object no longer tracked.
    fn surface(&self, resolution: &PickResolution) -> Option<Surface> {
        let own_agent = self.identity.agent_id;
        let worn = |scoped| {
            let summary = self.objects.pick_summary(scoped)?;
            let wearer = summary
                .wearer
                .and_then(|avatar| self.avatars.agent_of(avatar));
            let hud = summary.attachment && self.objects.wearer_of(scoped).is_none();
            let own = wearer.is_some() && wearer == own_agent;
            Some(Surface::Object(ObjectClick::of(&summary, own, hud)))
        };
        match resolution {
            PickResolution::Terrain => Some(Surface::Terrain),
            PickResolution::Water => Some(Surface::Water),
            PickResolution::Avatar { agent, worn: None } => Some(Surface::Avatar {
                own: Some(*agent) == own_agent,
            }),
            PickResolution::Avatar {
                agent,
                worn: Some(scoped),
            } => {
                let surface = worn(*scoped)?;
                // A rigged attachment is drawn on its wearer's body, whose
                // agent the pick names even when the chain does not resolve
                // to one yet.
                Some(match surface {
                    Surface::Object(object) if Some(*agent) == own_agent => {
                        Surface::Object(ObjectClick {
                            worn: Worn::ByMe,
                            ..object
                        })
                    }
                    other => other,
                })
            }
            PickResolution::ObjectFace { scoped, .. } => worn(*scoped),
        }
    }

    /// Whether a double-click teleport may land on an object that takes a
    /// click itself.
    fn scripted_objects_too(&self) -> bool {
        self.settings
            .store()
            .get_bool(SETTING_DOUBLE_CLICK_SCRIPTED_OBJECTS)
            .unwrap_or(true)
    }

    /// The ground height under a point in the current region's frame — which
    /// may lie in a neighbour — or `None` where no ground is known.
    fn ground_at(&self, current: RegionHandle, point: &Vector) -> Option<f32> {
        let (region_east, region_north) = current.global_coordinates();
        let global_e = f64::from(region_east) + f64::from(point.x);
        let global_n = f64::from(region_north) + f64::from(point.y);
        let handle = region_handle_at(global_e, global_n)?;
        let (dest_east, dest_north) = handle.global_coordinates();
        self.terrain.land_height(
            handle,
            narrow(global_e - f64::from(dest_east)),
            narrow(global_n - f64::from(dest_north)),
        )
    }

    /// The own avatar's pelvis-to-foot height under its current shape; the
    /// rest shape's while its appearance has not arrived, and nothing
    /// without the avatar assets.
    fn pelvis_to_foot(&self) -> f32 {
        let Some(library) = self.library.as_ref() else {
            return 0.0;
        };
        let agent: Option<AgentKey> = self.identity.agent_id;
        let deform = agent
            .and_then(|agent| self.avatars.deformations(agent))
            .cloned()
            .unwrap_or_default();
        let overrides = agent
            .and_then(|agent| self.avatars.effective_joint_overrides(agent))
            .unwrap_or_default();
        library
            .skeleton()
            .body_size_metrics(&deform, &overrides)
            .map_or(0.0, |metrics| metrics.pelvis_to_foot)
    }
}

/// Issue the teleport when a double-click's GPU pick names a surface the
/// reference teleports to ([`lands`]): to the picked point, or for water the
/// ground under it, raised by the own avatar's pelvis-to-foot height.
fn resolve_double_click_teleport(
    mut picks: MessageReader<GpuPickResolved>,
    landing: Landing,
    mut commands: MessageWriter<SlCommand>,
    mut begin: MessageWriter<BeginTeleportFlow>,
) {
    for pick in picks.read() {
        if pick.purpose != PickPurpose::DoubleClick {
            continue;
        }
        let Some(hit) = pick.hit.as_ref() else {
            continue;
        };
        let Some(surface) = landing.surface(&hit.resolution) else {
            debug!("double-click teleport: the picked object is not tracked; ignored");
            continue;
        };
        if !lands(surface, landing.scripted_objects_too()) {
            debug!(
                ?surface,
                "double-click teleport: not a surface to land on; ignored"
            );
            continue;
        }
        let Some(handle) = landing.identity.region_handle else {
            debug!("double-click teleport: no region handle yet; ignored");
            continue;
        };
        let forward = bevy_to_sl_vec(Vec3::from(pick.ray.direction));
        let mut point = bevy_to_sl_vec(hit.world_point);
        if surface == Surface::Water {
            let Some(ground) =
                ground_under_water(&point, &forward, |at| landing.ground_at(handle, at))
            else {
                debug!("double-click teleport: no ground known under the water; ignored");
                continue;
            };
            point = ground;
        }
        point.z += landing.pelvis_to_foot();
        let Some((destination, position, look_at)) = teleport_destination(handle, &point, &forward)
        else {
            continue;
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
}

/// Where the ray through a water hit at `surface`, going `forward`, meets the
/// ground under the water — the point the reference's pick finds, since it
/// looks through water at the land. `ground` answers the ground height under a
/// point; `None` when the ray meets no known ground within
/// [`UNDERWATER_REACH`].
fn ground_under_water(
    surface: &Vector,
    forward: &Vector,
    ground: impl Fn(&Vector) -> Option<f32>,
) -> Option<Vector> {
    let length = (forward.x * forward.x + forward.y * forward.y + forward.z * forward.z).sqrt();
    if length <= f32::EPSILON || forward.z >= 0.0 {
        return None;
    }
    let along = |distance: f32| Vector {
        x: surface.x + forward.x / length * distance,
        y: surface.y + forward.y / length * distance,
        z: surface.z + forward.z / length * distance,
    };
    let below = |point: &Vector| ground(point).map(|height| point.z <= height);
    let mut above_at = 0.0_f32;
    let mut distance = UNDERWATER_STEP;
    while distance <= UNDERWATER_REACH {
        if below(&along(distance))? {
            let mut beneath_at = distance;
            for _round in 0..UNDERWATER_BISECTIONS {
                let middle = f32::midpoint(above_at, beneath_at);
                if below(&along(middle))? {
                    beneath_at = middle;
                } else {
                    above_at = middle;
                }
            }
            let landed = along(beneath_at);
            return Some(Vector {
                z: ground(&landed)?,
                ..landed
            });
        }
        above_at = distance;
        distance += UNDERWATER_STEP;
    }
    None
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
    use super::{
        CLICK_ACTION_DISABLED, CLICK_ACTION_SIT, ObjectClick, Surface, WorldDoubleClickAction,
        Worn, ground_under_water, horizontal_look, lands, teleport_destination,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{RegionHandle, Vector};

    /// The click action `ClickAction::Pay`.
    const CLICK_ACTION_PAY: u8 = 3;

    /// An unworn prim with no click action and no touch script.
    const PLAIN: ObjectClick = ObjectClick {
        picked_action: 0,
        root_action: 0,
        handles_touch: false,
        worn: Worn::No,
    };

    /// Whether a double-click on `object` teleports, without and with
    /// `FSAllowDoubleClickOnScriptedObjects`.
    const fn both(object: ObjectClick) -> (bool, bool) {
        (
            lands(Surface::Object(object), false),
            lands(Surface::Object(object), true),
        )
    }

    /// Ground, water and avatars: ground and water always, another avatar
    /// always, one's own never.
    #[test]
    fn ground_water_and_avatars() {
        for scripted in [false, true] {
            assert!(lands(Surface::Terrain, scripted), "terrain");
            assert!(lands(Surface::Water, scripted), "water");
            assert!(
                lands(Surface::Avatar { own: false }, scripted),
                "another avatar"
            );
            assert!(
                !lands(Surface::Avatar { own: true }, scripted),
                "one's own avatar"
            );
        }
    }

    /// An object that takes no click of its own is a landing either way; one
    /// that does (a touch script, a pay action) only with the scripted-objects
    /// setting; a sit object never — and *disabled* is no click action.
    #[test]
    fn objects_by_what_a_click_on_them_does() {
        assert_eq!(both(PLAIN), (true, true), "a plain prim");
        let touch = ObjectClick {
            handles_touch: true,
            ..PLAIN
        };
        assert_eq!(both(touch), (false, true), "a touch script");
        let pay = ObjectClick {
            picked_action: CLICK_ACTION_PAY,
            ..PLAIN
        };
        assert_eq!(both(pay), (false, true), "a pay action");
        let sit = ObjectClick {
            picked_action: CLICK_ACTION_SIT,
            ..PLAIN
        };
        assert_eq!(both(sit), (false, false), "a sit action");
        let disabled = ObjectClick {
            picked_action: CLICK_ACTION_DISABLED,
            handles_touch: true,
            ..PLAIN
        };
        assert_eq!(both(disabled), (true, true), "a disabled click action");
    }

    /// The root's click action is the child's when the child has none, and a
    /// root set to *disabled* lets the child's own stand — the reference's
    /// `final_click_action`. The sit check reads the picked prim's own.
    #[test]
    fn a_child_takes_its_roots_click_action() {
        let under_a_sit_root = ObjectClick {
            root_action: CLICK_ACTION_SIT,
            ..PLAIN
        };
        assert_eq!(
            both(under_a_sit_root),
            (false, true),
            "the root's sit is the child's click, but not its own sit action"
        );
        let under_a_disabled_root = ObjectClick {
            root_action: CLICK_ACTION_DISABLED,
            ..PLAIN
        };
        assert_eq!(both(under_a_disabled_root), (true, true));
    }

    /// Attachments: one's own and a HUD never; another avatar's by its touch
    /// script alone, since an attachment has no click action.
    #[test]
    fn attachments() {
        let worn = ObjectClick {
            worn: Worn::ByOther,
            picked_action: CLICK_ACTION_PAY,
            ..PLAIN
        };
        assert_eq!(
            both(worn),
            (true, true),
            "a worn prim's click action is none"
        );
        let scripted = ObjectClick {
            handles_touch: true,
            ..worn
        };
        assert_eq!(both(scripted), (false, true), "a scripted attachment");
        let own = ObjectClick {
            worn: Worn::ByMe,
            ..worn
        };
        assert_eq!(both(own), (false, false), "one's own attachment");
        let hud = ObjectClick {
            worn: Worn::Hud,
            ..worn
        };
        assert_eq!(both(hud), (false, false), "a HUD");
    }

    /// The ray through a water hit is followed down to the ground: from 20 m
    /// up at 45° over flat ground at 10 m, 10 m on; never upwards, and not
    /// where no ground is known.
    #[test]
    fn the_ground_under_the_water() -> Result<(), String> {
        let surface = Vector {
            x: 100.0,
            y: 50.0,
            z: 20.0,
        };
        let down = Vector {
            x: 1.0,
            y: 0.0,
            z: -1.0,
        };
        let landed = ground_under_water(&surface, &down, |_at| Some(10.0))
            .ok_or("the ray meets the ground")?;
        assert!(
            (landed.x - 110.0).abs() < 0.01 && (landed.y - 50.0).abs() < 0.01,
            "{landed:?}"
        );
        assert!((landed.z - 10.0).abs() < 1.0e-6, "{landed:?}");
        let up = Vector {
            x: 1.0,
            y: 0.0,
            z: 0.5,
        };
        assert_eq!(ground_under_water(&surface, &up, |_at| Some(10.0)), None);
        assert_eq!(ground_under_water(&surface, &down, |_at| None), None);
        Ok(())
    }

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
