//! The **Stand Up / Stop flycam / Stop Flying** state buttons — a reserved slot at
//! the leading edge of the bottom toolbar holding every "get me out of this mode"
//! affordance the current state calls for.
//!
//! The reference viewer combines two of these into one panel,
//! `LLPanelStandStopFlying` (`llmoveview`): a "Stand Up" button shown while the
//! avatar is sitting and a "Stop Flying" button shown while flying. We host the
//! same pair, plus this viewer's own "leave this camera mode" affordance — the old
//! top-centre "Stop flycam" bar, folded in here at the user's request so the
//! transient buttons share one home:
//!
//! - **Stand Up** — shown while the local avatar is seated (on an object,
//!   [`SlAgentParcel::seated_on`], or on the ground, [`SelfGroundSit`]). Pressing
//!   it sends [`Command::Stand`] (the reference's `AGENT_CONTROL_STAND_UP`).
//! - **Stop flycam** — shown while the camera is in [`CameraMode::Flycam`].
//!   Pressing it returns the camera to third person.
//! - **Stop Flying** — shown while the avatar is flying
//!   ([`AvatarControls::flying`]). Pressing it drops the fly intent, exactly as
//!   the reference's `onStopFlyingButtonClick` calls `gAgent.setFlying(false)`,
//!   and the avatar falls to the ground.
//!
//! # Each stands on its own state
//!
//! There is no precedence between them: they answer *independent* states, and a
//! state that holds is one the user may want out of, whatever else also holds.
//! Flying **and** in the flycam shows both — the flycam deliberately parks the
//! avatar with the fly bit set so a detached camera does not leave the body
//! plummeting, so "stop the camera" and "stop the flying" are two different
//! exits. Seated **and** in the flycam likewise shows both.
//!
//! The one pair that never co-occurs is Stand and Stop Flying: a seated avatar is
//! not flying, so [`wants_button`] does not offer to land one. That caps the slot
//! at two buttons at a time, which is what the toolbar's reserved
//! [state slot](crate::bottom_toolbar) is sized for.
//!
//! # Why here, not a floating panel
//!
//! The reference floats its stand panel in the bottom-centre tray, where it
//! collides with the bottom-left conversation dock. We instead host it in a
//! fixed-width **reserved slot** the bottom toolbar
//! ([`BottomArea::state_slot`](crate::ui::BottomArea::state_slot))
//! carves out at the button group's leading edge, balanced by a trailing spacer so
//! the button's coming and going never reflows the toolbar and never intrudes on
//! the dock.
//!
//! Reference (Firestorm, read-only): `llmoveview` (`LLPanelStandStopFlying`, the
//! stand / stop-flying button pair and its visibility rules).

use bevy::ecs::system::SystemParam;
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use sl_client_bevy::{Command, SlAgentParcel, SlCommand};

use crate::camera::FocusTarget;
use crate::i18n::Translated;
use crate::ui::BottomArea;
use crate::ui_font::UiFont;
use crate::world_api::SelfGroundSit;
use crate::world_api::{AvatarControls, CameraMode, CameraRig, ViewerCamera};

/// The state-button label font size, in logical pixels — matched to the toolbar's.
const FONT_SIZE: f32 = 13.0;

/// The button border colour, matching the toolbar buttons' resting border.
const BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// The button background — the toolbar's lit / active blue, so the transient
/// action reads as a live call to act, not a resting toggle.
const BACKGROUND: Color = Color::srgb(0.22, 0.40, 0.60);

/// The Fluent key for the Stand Up label.
const STAND_LABEL_KEY: &str = "stand-button-stand";

/// The Fluent key for the Stop flycam label.
const STOP_FLYCAM_LABEL_KEY: &str = "stand-button-stop-flycam";

/// The Fluent key for the Stop Flying label.
const STOP_FLYING_LABEL_KEY: &str = "stand-button-stop-flying";

/// Which action a state button performs — carried on the button so its visibility
/// system and its press observer agree on what it is without a marker query.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum StateButtonKind {
    /// Stand the seated avatar up ([`Command::Stand`]).
    Stand,
    /// Leave the joystick flycam for third person.
    StopFlycam,
    /// Drop the fly intent so the avatar falls to the ground.
    StopFlying,
}

/// The state-button plugin: spawns the buttons into the toolbar's reserved slot
/// once it exists, then shows the ones the state calls for (any, all, or none).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StandStopButtonPlugin;

impl Plugin for StandStopButtonPlugin {
    /// Spawn the buttons once the [`BottomArea`] slot is published, and keep their
    /// visibility current each frame.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_state_buttons, update_state_button_visibility),
        );
    }
}

/// Spawn the state buttons into the toolbar's reserved slot, once — hidden until
/// the visibility system reveals the ones the state wants.
///
/// Runs in `Update` guarded by a `Local` done-flag (rather than `Startup`) so it
/// never races the bottom toolbar's own startup spawn: it simply waits for
/// [`BottomArea`] to be published, spawns into its slot, and then never runs its
/// body again.
fn spawn_state_buttons(
    mut done: Local<bool>,
    area: Option<Res<BottomArea>>,
    mut commands: Commands,
) {
    if *done {
        return;
    }
    let Some(area) = area else {
        return;
    };
    for kind in KINDS {
        spawn_button(&mut commands, area.state_slot, kind, label_key(kind));
    }
    *done = true;
}

/// The Fluent key naming a state button's label.
const fn label_key(kind: StateButtonKind) -> &'static str {
    match kind {
        StateButtonKind::Stand => STAND_LABEL_KEY,
        StateButtonKind::StopFlycam => STOP_FLYCAM_LABEL_KEY,
        StateButtonKind::StopFlying => STOP_FLYING_LABEL_KEY,
    }
}

/// Spawn one hidden state button of `kind` into the slot, with its Fluent-bound
/// label and its press observer.
fn spawn_button(
    commands: &mut Commands,
    slot: Entity,
    kind: StateButtonKind,
    label_key: &'static str,
) {
    commands
        .spawn((
            Button,
            TabIndex(0),
            kind,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                // Size to the label and never be compressed below it, so the whole
                // label stays on one line (the slot is wide enough to hold it).
                flex_shrink: 0.0,
                // Removed from layout until the state calls for it — NOT
                // `Visibility::Hidden`, which only stops rendering and leaves the
                // node occupying its full width in the slot's flex row. With every
                // button `Hidden` they all laid out side by side, overflowed the
                // fixed-width slot, and the overflow drew over the neighbouring
                // Chat button (viewer-flycam-stop-button-overlaps-chat). `None`
                // collapses an inactive button so only the shown ones take space —
                // and the slot is sized for the most that can be shown at once.
                display: Display::None,
                ..default()
            },
            BorderColor::all(BORDER),
            BackgroundColor(BACKGROUND),
            Name::new(match kind {
                StateButtonKind::Stand => "state-button:stand",
                StateButtonKind::StopFlycam => "state-button:stop-flycam",
                StateButtonKind::StopFlying => "state-button:stop-flying",
            }),
            ChildOf(slot),
        ))
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(Color::WHITE),
            // Keep the label on a single line — the text measure otherwise
            // under-allocates in a flex slot and wraps a two-word label.
            TextLayout::no_wrap(),
        ))
        .observe(on_state_button);
}

/// Observer: perform the pressed button's action.
///
/// - **Stand** clears the viewer-tracked ground sit (the session keeps no
///   ground-sit state, so the flag must be cleared here just as the avatar pie
///   does) and sends [`Command::Stand`].
/// - **Stop flycam** returns the camera to third person, warping (not gliding) to
///   the follow view, exactly as the old top-centre button did.
/// - **Stop Flying** clears the fly intent the movement driver advertises, which
///   drops [`ControlFlags::FLY`] on the next frame's `SetControls` and lets the
///   avatar fall — the reference's `gAgent.setFlying(false)`. The hold-to-take-off
///   accumulator is cleared with it, so a still-held ascend key starts its half
///   second over rather than re-launching on the next frame.
///
/// Whichever was pressed, the keyboard goes **back to the world** afterwards. A
/// click focuses the button, and a focused UI node makes the input context
/// [`UiWidget`](crate::world_api::InputContext::UiWidget), which gates the
/// movement keys off — so without this, pressing Stop Flying dropped the avatar
/// and then swallowed the keys that would have caught it, until the user clicked
/// the world back into focus. The reference ends both of its handlers the same
/// way: `setFocus(false)`, its `onStopFlyingButtonClick` carrying the comment
/// `EXT-482` for the very same bug.
fn on_state_button(
    activate: On<Activate>,
    buttons: Query<&StateButtonKind>,
    mut ground_sit: ResMut<SelfGroundSit>,
    mut commands: MessageWriter<SlCommand>,
    mut controls: ResMut<AvatarControls>,
    mut focus: ResMut<InputFocus>,
    mut camera: CameraExit,
) {
    let Ok(kind) = buttons.get(activate.entity) else {
        return;
    };
    focus.clear();
    match kind {
        StateButtonKind::Stand => {
            ground_sit.sitting = false;
            commands.write(SlCommand(Command::Stand));
        }
        StateButtonKind::StopFlycam => camera.leave_flycam(),
        StateButtonKind::StopFlying => {
            controls.flying = false;
            controls.ascend_hold_secs = 0.0;
            controls.ascend_hold_frames = 0;
        }
    }
}

/// The camera state Stop flycam rewrites, grouped so the press observer stays
/// within Bevy's per-system parameter budget.
#[derive(SystemParam)]
struct CameraExit<'w, 's> {
    /// The active camera mode — the flycam this leaves.
    mode: ResMut<'w, CameraMode>,
    /// What the third-person camera orbits once it is back.
    focus: ResMut<'w, FocusTarget>,
    /// The one viewer camera's follow rig, re-snapped so the return is a warp.
    cameras: Query<'w, 's, &'static mut CameraRig, With<ViewerCamera>>,
}

impl CameraExit<'_, '_> {
    /// Return the camera to third person, warping (not gliding) to the follow
    /// view. A no-op when the camera is not in the flycam.
    fn leave_flycam(&mut self) {
        if *self.mode != CameraMode::Flycam {
            return;
        }
        *self.mode = CameraMode::ThirdPerson;
        *self.focus = FocusTarget::Avatar;
        if let Ok(mut rig) = self.cameras.single_mut() {
            rig.resnap();
        }
    }
}

/// Whether the local avatar is seated — on an object (the session's
/// [`SlAgentParcel::seated_on`]) or on the ground (the viewer-tracked
/// [`SelfGroundSit`]).
const fn is_seated(parcel: &SlAgentParcel, ground_sit: &SelfGroundSit) -> bool {
    parcel.seated_on.is_some() || ground_sit.sitting
}

/// Whether one state button's own state holds. Each is independent — the caller
/// shows every kind that answers `true`, so a flying avatar in the flycam offers
/// both exits. The single interaction is that Stop Flying stands down while
/// seated: a seated avatar is not flying, so there is nothing to land.
fn wants_button(
    kind: StateButtonKind,
    parcel: &SlAgentParcel,
    ground_sit: &SelfGroundSit,
    mode: CameraMode,
    flying: bool,
) -> bool {
    match kind {
        StateButtonKind::Stand => is_seated(parcel, ground_sit),
        StateButtonKind::StopFlycam => mode == CameraMode::Flycam,
        StateButtonKind::StopFlying => flying && !is_seated(parcel, ground_sit),
    }
}

/// Every state-button kind, in the order they are spawned into the slot.
const KINDS: [StateButtonKind; 3] = [
    StateButtonKind::Stand,
    StateButtonKind::StopFlycam,
    StateButtonKind::StopFlying,
];

/// Show every state button whose own state holds and hide the rest, so the
/// reserved slot carries exactly the exits available right now — none, one, or the
/// two that can co-occur.
///
/// Toggles [`Display`] (not [`Visibility`]): a hidden button must be **removed
/// from the flex layout**, or it keeps its width in the slot's row, the row
/// overflows, and the overflow overlaps the neighbouring Chat button
/// (viewer-flycam-stop-button-overlaps-chat). Only writes on a state change (the
/// value compare), so the layout is not touched every frame.
fn update_state_button_visibility(
    parcel: Res<SlAgentParcel>,
    ground_sit: Res<SelfGroundSit>,
    mode: Res<CameraMode>,
    controls: Res<AvatarControls>,
    mut buttons: Query<(&StateButtonKind, &mut Node)>,
) {
    for (kind, mut node) in &mut buttons {
        let next = if wants_button(*kind, &parcel, &ground_sit, *mode, controls.flying) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != next {
            node.display = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KINDS, StateButtonKind, is_seated, wants_button};
    use crate::world_api::CameraMode;
    use crate::world_api::SelfGroundSit;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ObjectKey, SlAgentParcel, Uuid};

    /// An arbitrary object key, for the "seated on an object" assertions.
    fn a_seat() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(1))
    }

    /// A parcel with the given object-seat, for the seated-state assertions.
    fn parcel(seated_on: Option<ObjectKey>) -> SlAgentParcel {
        SlAgentParcel {
            seated_on,
            ..SlAgentParcel::default()
        }
    }

    /// Sitting on an object or on the ground both count as seated; standing does
    /// not.
    #[test]
    fn seated_covers_object_and_ground_sits() {
        let standing = parcel(None);
        let ground = SelfGroundSit { sitting: true };
        let not_ground = SelfGroundSit { sitting: false };
        assert!(!is_seated(&standing, &not_ground), "standing is not seated");
        assert!(is_seated(&standing, &ground), "a ground sit is seated");
        assert!(
            is_seated(&parcel(Some(a_seat())), &not_ground),
            "an object sit is seated",
        );
    }

    /// Every state button the given state calls for, in slot order.
    fn shown(
        parcel: &SlAgentParcel,
        ground_sit: &SelfGroundSit,
        mode: CameraMode,
        flying: bool,
    ) -> Vec<StateButtonKind> {
        KINDS
            .into_iter()
            .filter(|kind| wants_button(*kind, parcel, ground_sit, mode, flying))
            .collect()
    }

    /// Each button answers its own state, with no precedence between them: seated
    /// or flying in the flycam shows *both* exits, because the camera and the
    /// avatar are two different things to get out of. A regression that made them
    /// mutually exclusive again — hiding Stop Flying behind Stop flycam, or Stand
    /// behind either — would trip here.
    #[test]
    fn the_state_shows_every_exit_it_offers() {
        use StateButtonKind::{Stand, StopFlycam, StopFlying};

        let standing = parcel(None);
        let seated = parcel(Some(a_seat()));
        let no_ground = SelfGroundSit { sitting: false };
        let (flying, walking) = (true, false);

        assert_eq!(
            shown(&standing, &no_ground, CameraMode::ThirdPerson, walking),
            vec![],
            "standing in third person shows no state button",
        );
        assert_eq!(
            shown(&standing, &no_ground, CameraMode::Flycam, walking),
            vec![StopFlycam],
            "flycam alone shows Stop flycam",
        );
        assert_eq!(
            shown(&seated, &no_ground, CameraMode::ThirdPerson, walking),
            vec![Stand],
            "sitting shows Stand",
        );
        assert_eq!(
            shown(&standing, &no_ground, CameraMode::ThirdPerson, flying),
            vec![StopFlying],
            "flying in third person shows Stop Flying",
        );
        assert_eq!(
            shown(&standing, &no_ground, CameraMode::Mouselook, flying),
            vec![StopFlying],
            "mouselook is an ordinary camera — flying still shows Stop Flying",
        );

        // The two pairs that co-occur, both shown at once.
        assert_eq!(
            shown(&seated, &no_ground, CameraMode::Flycam, walking),
            vec![Stand, StopFlycam],
            "seated in the flycam offers both exits",
        );
        assert_eq!(
            shown(&standing, &no_ground, CameraMode::Flycam, flying),
            vec![StopFlycam, StopFlying],
            "flying in the flycam offers both exits",
        );

        // A seated avatar is not flying, so it is never offered a landing — which
        // is what keeps the slot to two buttons.
        assert_eq!(
            shown(&seated, &no_ground, CameraMode::ThirdPerson, flying),
            vec![Stand],
            "a seated avatar is not flying — no Stop Flying",
        );
        assert_eq!(
            shown(&seated, &no_ground, CameraMode::Flycam, flying),
            vec![Stand, StopFlycam],
            "seated beats flying even in the flycam — at most two buttons",
        );
    }
}
