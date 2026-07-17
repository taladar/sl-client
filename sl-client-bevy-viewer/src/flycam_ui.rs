//! The on-screen **"Stop flycam"** button.
//!
//! The reference viewer shows a small "Flycam" button while the joystick flycam is
//! engaged; clicking it leaves flycam. We show the same affordance, with one
//! deliberate wording change the user asked for: the button reads **"Stop
//! flycam"**, saying what it does rather than which mode you are in. It appears
//! only in [`CameraMode::Flycam`] and, when activated (clicked, or `Enter` /
//! `Space` while focused), returns the camera to third person.
//!
//! Built on the viewer's UI scaffold ([`crate::ui`]): a headless
//! `bevy_ui_widgets` [`Button`] parented to the one `UiRoot`, so it inherits the
//! font stack, tab navigation and focus routing every panel shares.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::camera::{CameraMode, CameraRig, FocusTarget, ViewerCamera};
use crate::ui::{UiRoot, UiScaffoldSystems};
use crate::ui_font::UiFont;

/// The button's label — the requested "Stop flycam" wording.
const LABEL: &str = "Stop flycam";
/// The button label's font size, logical px.
const FONT_SIZE: f32 = 16.0;
/// The bar's inset from the top of the viewport, logical px.
const TOP_INSET: f32 = 12.0;

/// The button's border colour.
const BORDER: Color = Color::srgb(0.8, 0.8, 0.85);
/// The button's background colour (a translucent dark slate).
const BACKGROUND: Color = Color::srgba(0.1, 0.1, 0.13, 0.85);

/// A marker on the top-centred bar that holds the button, whose visibility is
/// toggled with the camera mode (hiding the bar hides the button it parents).
#[derive(Component)]
struct FlycamButtonBar;

/// The flycam-button plugin: spawns the (hidden) button and shows it in flycam.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FlycamButtonPlugin;

impl Plugin for FlycamButtonPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            setup_flycam_button.after(UiScaffoldSystems::SpawnRoot),
        )
        .add_systems(Update, update_flycam_button_visibility);
    }
}

/// Startup: spawn the top-centred bar and its "Stop flycam" button under the
/// `UiRoot`, hidden until flycam is entered.
fn setup_flycam_button(mut commands: Commands, root: Res<UiRoot>) {
    commands.entity(root.0).with_children(|parent| {
        parent
            .spawn((
                FlycamButtonBar,
                // A full-width absolute strip at the top, centring the button — so
                // the button sits top-centre whatever the window width, without any
                // manual half-width offset.
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(TOP_INSET),
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                // Hidden until the camera enters flycam.
                Visibility::Hidden,
            ))
            .with_children(|bar| {
                bar.spawn((
                    Button,
                    TabIndex(0),
                    Node {
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(BORDER),
                    BackgroundColor(BACKGROUND),
                ))
                .with_child((
                    Text::new(LABEL),
                    UiFont::Sans.at(FONT_SIZE),
                    TextColor(Color::WHITE),
                ))
                .observe(stop_flycam);
            });
    });
}

/// Observer: leave flycam for third person when the button is activated, warping
/// (not gliding) to the follow view — the reference snaps when flycam stops.
fn stop_flycam(
    _activate: On<Activate>,
    mut mode: ResMut<CameraMode>,
    mut focus: ResMut<FocusTarget>,
    mut cameras: Query<&mut CameraRig, With<ViewerCamera>>,
) {
    if *mode == CameraMode::Flycam {
        *mode = CameraMode::ThirdPerson;
        *focus = FocusTarget::Avatar;
        if let Ok(mut rig) = cameras.single_mut() {
            rig.resnap();
        }
    }
}

/// Show the button only while in flycam.
fn update_flycam_button_visibility(
    mode: Res<CameraMode>,
    mut bars: Query<&mut Visibility, With<FlycamButtonBar>>,
) {
    if !mode.is_changed() {
        return;
    }
    let next = if *mode == CameraMode::Flycam {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut bars {
        if *visibility != next {
            *visibility = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FlycamButtonBar, update_flycam_button_visibility};
    use crate::camera::CameraMode;
    use bevy::prelude::*;

    /// A boxed error so the test can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The bar is shown in flycam and hidden otherwise, tracking the mode.
    #[test]
    fn the_button_shows_only_in_flycam() -> Result<(), TestError> {
        let mut app = App::new();
        app.insert_resource(CameraMode::ThirdPerson)
            .add_systems(Update, update_flycam_button_visibility);
        let bar = app
            .world_mut()
            .spawn((FlycamButtonBar, Visibility::Hidden))
            .id();

        // Enter flycam → shown.
        *app.world_mut().resource_mut::<CameraMode>() = CameraMode::Flycam;
        app.update();
        let visibility = app
            .world()
            .get::<Visibility>(bar)
            .ok_or("the bar lost its visibility")?;
        assert!(matches!(visibility, Visibility::Visible), "shown in flycam");

        // Back to third person → hidden.
        *app.world_mut().resource_mut::<CameraMode>() = CameraMode::ThirdPerson;
        app.update();
        let visibility = app
            .world()
            .get::<Visibility>(bar)
            .ok_or("the bar lost its visibility")?;
        assert!(
            matches!(visibility, Visibility::Hidden),
            "hidden outside flycam"
        );
        Ok(())
    }
}
