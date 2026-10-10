//! The injector's frame protocol, on a bare app: the input plugin and a window,
//! no UI. What a widget does with the messages is the testkit's to test.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::{ButtonState, InputPlugin};
use bevy::picking::PickingSettings;
use bevy::prelude::*;
use pretty_assertions::assert_eq;

use super::{ActionStatus, InputAction, SyntheticInput, SyntheticInputPlugin};

/// Every key message, as `(key, pressed)`, in the frame it was read.
#[derive(Resource, Default)]
struct KeyLog(Vec<(u64, KeyCode, bool)>);

/// Log this frame's key messages with the injector's frame number.
fn log_keys(
    mut reader: MessageReader<KeyboardInput>,
    input: Res<SyntheticInput>,
    mut log: ResMut<KeyLog>,
) {
    for key in reader.read() {
        log.0.push((
            input.frame(),
            key.key_code,
            key.state == ButtonState::Pressed,
        ));
    }
}

/// A bare app: input, one primary window, the injector, and a key log.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::time::TimePlugin,
        InputPlugin,
        bevy::window::WindowPlugin {
            primary_window: Some(Window::default()),
            primary_cursor_options: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            close_when_requested: false,
        },
        SyntheticInputPlugin,
    ));
    app.init_resource::<KeyLog>().add_systems(Update, log_keys);
    app
}

/// Queue `action`.
fn enqueue(app: &mut App, action: InputAction) -> super::InputActionId {
    app.world_mut()
        .resource_mut::<SyntheticInput>()
        .enqueue(action)
}

/// Where `id` is.
fn status(app: &App, id: super::InputActionId) -> ActionStatus {
    app.world().resource::<SyntheticInput>().status(id)
}

/// **One step per frame, one action after another, and done at the frame of
/// its last step** — the protocol a click's four frames rest on.
#[test]
fn actions_take_one_frame_per_step_and_run_in_order() {
    let mut app = app();
    let click = enqueue(&mut app, InputAction::click(Vec2::ONE, MouseButton::Left));
    let tap = enqueue(
        &mut app,
        InputAction::tap(KeyCode::KeyA, Key::Character("a".into())),
    );
    assert_eq!(status(&app, click), ActionStatus::Queued);
    assert_eq!(status(&app, tap), ActionStatus::Queued);

    app.update();
    assert_eq!(status(&app, click), ActionStatus::Running);
    assert_eq!(status(&app, tap), ActionStatus::Queued);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        status(&app, click),
        ActionStatus::Done { frame: 4 },
        "a click is move, press, release, settle: four frames"
    );
    assert_eq!(status(&app, tap), ActionStatus::Queued);

    app.update();
    app.update();
    assert_eq!(status(&app, tap), ActionStatus::Done { frame: 6 });
    assert!(app.world().resource::<SyntheticInput>().is_idle());
    assert_eq!(
        app.world().resource::<KeyLog>().0,
        vec![(5, KeyCode::KeyA, true), (6, KeyCode::KeyA, false)],
        "the tap's steps land in the frames after the click's"
    );
}

/// **An empty action finishes without taking a frame** from the one behind it.
#[test]
fn an_empty_action_takes_no_frame() {
    let mut app = app();
    let empty = enqueue(&mut app, InputAction::type_text(""));
    let key = enqueue(
        &mut app,
        InputAction::key_down(KeyCode::KeyB, Key::Character("b".into()), None),
    );
    app.update();
    assert_eq!(status(&app, empty), ActionStatus::Done { frame: 1 });
    assert_eq!(status(&app, key), ActionStatus::Done { frame: 1 });
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyB),
        "the key behind the empty action goes down in the same frame"
    );
}

/// **A held key reads as pressed for exactly its frames** — how long a
/// movement key walks.
#[test]
fn a_held_key_is_pressed_for_its_frames() {
    let mut app = app();
    enqueue(
        &mut app,
        InputAction::hold_key(KeyCode::KeyW, Key::Character("w".into()), 3),
    );
    let mut pressed = Vec::new();
    for _ in 0..5 {
        app.update();
        pressed.push(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW),
        );
    }
    assert_eq!(pressed, vec![true, true, true, false, false]);
}

/// **A chord presses its modifiers first and lets them go last**, in reverse.
#[test]
fn a_chord_wraps_the_key_in_its_modifiers() {
    let mut app = app();
    enqueue(
        &mut app,
        InputAction::chord(
            &[
                (KeyCode::ControlLeft, Key::Control),
                (KeyCode::ShiftLeft, Key::Shift),
            ],
            KeyCode::KeyS,
            Key::Character("s".into()),
        ),
    );
    for _ in 0..6 {
        app.update();
    }
    let order: Vec<(KeyCode, bool)> = app
        .world()
        .resource::<KeyLog>()
        .0
        .iter()
        .map(|(_frame, key, down)| (*key, *down))
        .collect();
    assert_eq!(
        order,
        vec![
            (KeyCode::ControlLeft, true),
            (KeyCode::ShiftLeft, true),
            (KeyCode::KeyS, true),
            (KeyCode::KeyS, false),
            (KeyCode::ShiftLeft, false),
            (KeyCode::ControlLeft, false),
        ]
    );
}

/// **A click pins the multi-click interval for its frames and puts it back**
/// the moment it is done — never later, so the app's own double clicks are
/// untouched between synthetic ones.
#[test]
fn a_click_pins_the_multi_click_interval_only_while_it_runs() {
    let mut app = app();
    app.init_resource::<PickingSettings>();
    let original = app
        .world()
        .resource::<PickingSettings>()
        .multi_click_interval;
    assert!(
        original > core::time::Duration::ZERO,
        "the fixture needs a non-zero default to tell the pin apart"
    );
    let click = enqueue(&mut app, InputAction::click(Vec2::ONE, MouseButton::Left));
    for frame in 1..=4 {
        app.update();
        let interval = app
            .world()
            .resource::<PickingSettings>()
            .multi_click_interval;
        if frame < 4 {
            assert_eq!(
                interval,
                core::time::Duration::ZERO,
                "pinned in frame {frame}"
            );
        } else {
            assert_eq!(interval, original, "restored at the end of the last frame");
        }
    }
    assert_eq!(status(&app, click), ActionStatus::Done { frame: 4 });
}

/// **A double click pins the multi-click interval the other way**, to
/// forever, and puts it back: its presses are two frames apart, and at five
/// frames a second that is longer than the app's own interval — the second
/// press would be a first.
#[test]
fn a_double_click_is_one_however_long_its_frames_took() {
    let mut app = app();
    app.init_resource::<PickingSettings>();
    let original = app
        .world()
        .resource::<PickingSettings>()
        .multi_click_interval;
    let action = InputAction::double_click(Vec2::ONE, MouseButton::Left);
    let frames = action.frames();
    let double = enqueue(&mut app, action);
    for frame in 1..=frames {
        app.update();
        let interval = app
            .world()
            .resource::<PickingSettings>()
            .multi_click_interval;
        if frame < frames {
            assert_eq!(interval, super::FOREVER, "pinned in frame {frame}");
        } else {
            assert_eq!(interval, original, "restored at the end of the last frame");
        }
    }
    assert!(matches!(status(&app, double), ActionStatus::Done { .. }));
}

/// **The pointer moves the window's cursor**, which the viewer's world readers
/// (`cursor_position()`) see.
#[test]
fn a_move_moves_the_window_cursor() {
    let mut app = app();
    enqueue(&mut app, InputAction::move_to(Vec2::new(42.0, 17.0)));
    app.update();
    let mut windows = app.world_mut().query::<&Window>();
    let cursor = windows
        .single(app.world())
        .ok()
        .and_then(Window::cursor_position);
    assert_eq!(cursor, Some(Vec2::new(42.0, 17.0)));
}

/// **Mouselook spreads its motion over its frames**, and the whole arrives.
#[test]
fn mouse_look_spreads_its_motion_evenly() {
    let action = InputAction::mouse_look(Vec2::new(40.0, -8.0), 4);
    assert_eq!(action.frames(), 4);
    let total = action
        .steps()
        .iter()
        .fold(Vec2::ZERO, |sum, step| match step {
            super::InputStep::Motion(delta) => Vec2::new(sum.x + delta.x, sum.y + delta.y),
            _other => sum,
        });
    assert_eq!(total, Vec2::new(40.0, -8.0));
}

/// **Old completions are forgotten, not kept forever**, and say so.
#[test]
fn old_completions_expire() {
    let mut app = app();
    let first = enqueue(&mut app, InputAction::default());
    let count = super::REMEMBERED_COMPLETIONS;
    let ids: Vec<_> = core::iter::repeat_with(InputAction::default)
        .take(count)
        .map(|action| enqueue(&mut app, action))
        .collect();
    app.update();
    assert_eq!(status(&app, first), ActionStatus::Expired);
    assert_eq!(
        ids.last().map(|id| status(&app, *id)),
        Some(ActionStatus::Done { frame: 1 })
    );
}
