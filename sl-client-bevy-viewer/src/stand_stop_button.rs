//! The **Stand Up / Stop flycam** state button — a single reserved slot at the
//! leading edge of the bottom toolbar that surfaces whichever "get me out of this
//! mode" affordance the current state needs.
//!
//! The reference viewer combines these into one panel, `LLPanelStandStopFlying`
//! (`llmoveview`): a "Stand Up" button shown while the avatar is sitting and a
//! "Stop Flying" button shown while flying, only ever one visible at a time. We
//! mirror that pairing, with our joystick **flycam** standing in for the
//! reference's flying (this viewer's "leave this camera mode" affordance — the old
//! top-centre "Stop flycam" bar, folded in here at the user's request so the two
//! transient buttons share one home):
//!
//! - **Stand Up** — shown while the local avatar is seated (on an object,
//!   [`SlAgentParcel::seated_on`], or on the ground, [`SelfGroundSit`]). Pressing
//!   it sends [`Command::Stand`] (the reference's `AGENT_CONTROL_STAND_UP`).
//! - **Stop flycam** — shown while the camera is in [`CameraMode::Flycam`] and the
//!   avatar is *not* seated. Pressing it returns the camera to third person.
//!
//! Sitting takes precedence over flycam, matching the reference (which sets
//! `SSFM_STAND` whenever the avatar is sitting).
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

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use sl_client_bevy::{Command, SlAgentParcel, SlCommand};

use crate::camera::{CameraMode, CameraRig, FocusTarget, ViewerCamera};
use crate::i18n::Translated;
use crate::ui::BottomArea;
use crate::ui_font::UiFont;
use crate::world_api::SelfGroundSit;

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

/// Which action a state button performs — carried on the button so its visibility
/// system and its press observer agree on what it is without a marker query.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum StateButtonKind {
    /// Stand the seated avatar up ([`Command::Stand`]).
    Stand,
    /// Leave the joystick flycam for third person.
    StopFlycam,
}

/// The Stand / Stop-flycam state-button plugin: spawns the two buttons into the
/// toolbar's reserved slot once it exists, then shows whichever the state calls
/// for (or neither).
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

/// Spawn the two state buttons into the toolbar's reserved slot, once — hidden
/// until the visibility system reveals the one the state wants.
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
    spawn_button(
        &mut commands,
        area.state_slot,
        StateButtonKind::Stand,
        STAND_LABEL_KEY,
    );
    spawn_button(
        &mut commands,
        area.state_slot,
        StateButtonKind::StopFlycam,
        STOP_FLYCAM_LABEL_KEY,
    );
    *done = true;
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
                // node occupying its full width in the slot's flex row. With both
                // buttons `Hidden` they laid out side by side, overflowed the
                // fixed-width slot, and the overflow drew over the neighbouring
                // Chat button (viewer-flycam-stop-button-overlaps-chat). `None`
                // collapses the inactive button so only the shown one takes space.
                display: Display::None,
                ..default()
            },
            BorderColor::all(BORDER),
            BackgroundColor(BACKGROUND),
            Name::new(match kind {
                StateButtonKind::Stand => "state-button:stand",
                StateButtonKind::StopFlycam => "state-button:stop-flycam",
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
fn on_state_button(
    activate: On<Activate>,
    buttons: Query<&StateButtonKind>,
    mut ground_sit: ResMut<SelfGroundSit>,
    mut commands: MessageWriter<SlCommand>,
    mut mode: ResMut<CameraMode>,
    mut focus: ResMut<FocusTarget>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    let Ok(kind) = buttons.get(activate.entity) else {
        return;
    };
    match kind {
        StateButtonKind::Stand => {
            ground_sit.sitting = false;
            commands.write(SlCommand(Command::Stand));
        }
        StateButtonKind::StopFlycam => {
            if *mode == CameraMode::Flycam {
                *mode = CameraMode::ThirdPerson;
                *focus = FocusTarget::Avatar;
                if let Ok(mut rig) = cameras.single_mut() {
                    rig.resnap();
                }
            }
        }
    }
}

/// Whether the local avatar is seated — on an object (the session's
/// [`SlAgentParcel::seated_on`]) or on the ground (the viewer-tracked
/// [`SelfGroundSit`]).
const fn is_seated(parcel: &SlAgentParcel, ground_sit: &SelfGroundSit) -> bool {
    parcel.seated_on.is_some() || ground_sit.sitting
}

/// The state button (if any) the current state wants shown — sitting first
/// (Stand), then flycam (Stop flycam), matching the reference's precedence.
fn wanted_button(
    parcel: &SlAgentParcel,
    ground_sit: &SelfGroundSit,
    mode: CameraMode,
) -> Option<StateButtonKind> {
    if is_seated(parcel, ground_sit) {
        Some(StateButtonKind::Stand)
    } else if mode == CameraMode::Flycam {
        Some(StateButtonKind::StopFlycam)
    } else {
        None
    }
}

/// Show whichever state button the current state calls for and hide the other, so
/// the reserved slot holds at most one — the sitting Stand or the flycam Stop, or
/// nothing when neither state holds.
///
/// Toggles [`Display`] (not [`Visibility`]): the hidden button must be **removed
/// from the flex layout**, or both buttons keep their width in the slot's row,
/// overflow it, and the overflow overlaps the neighbouring Chat button
/// (viewer-flycam-stop-button-overlaps-chat). Only writes on a state change (the
/// value compare), so the layout is not touched every frame.
fn update_state_button_visibility(
    parcel: Res<SlAgentParcel>,
    ground_sit: Res<SelfGroundSit>,
    mode: Res<CameraMode>,
    mut buttons: Query<(&StateButtonKind, &mut Node)>,
) {
    let wanted = wanted_button(&parcel, &ground_sit, *mode);
    for (kind, mut node) in &mut buttons {
        let next = if wanted == Some(*kind) {
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
    use super::{StateButtonKind, is_seated, wanted_button};
    use crate::camera::CameraMode;
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

    /// Sitting shows Stand and takes precedence over flycam; flycam alone shows
    /// Stop flycam; neither state shows nothing. A regression that flipped the
    /// precedence (Stop flycam winning while seated in a vehicle) would trip here.
    #[test]
    fn the_state_picks_at_most_one_button() {
        let standing = parcel(None);
        let seated = parcel(Some(a_seat()));
        let no_ground = SelfGroundSit { sitting: false };

        assert_eq!(
            wanted_button(&standing, &no_ground, CameraMode::ThirdPerson),
            None,
            "standing in third person shows no state button",
        );
        assert_eq!(
            wanted_button(&standing, &no_ground, CameraMode::Flycam),
            Some(StateButtonKind::StopFlycam),
            "flycam alone shows Stop flycam",
        );
        assert_eq!(
            wanted_button(&seated, &no_ground, CameraMode::ThirdPerson),
            Some(StateButtonKind::Stand),
            "sitting shows Stand",
        );
        assert_eq!(
            wanted_button(&seated, &no_ground, CameraMode::Flycam),
            Some(StateButtonKind::Stand),
            "sitting beats flycam — Stand wins",
        );
    }
}
