//! The **synthetic pointer and keyboard**: real input, headlessly.
//!
//! [`crate::LayoutTest`]'s `activate` triggers a widget's `Activate` directly,
//! which deliberately skips hit-testing — it cannot say whether the control is
//! *where the user's pointer thinks it is*, cannot tell the buttons apart, and
//! cannot hover, scroll or drag. This module adds the missing half: a driver
//! that writes the same messages winit writes live — the typed `bevy_input` /
//! `bevy_window` messages **plus their [`WindowEvent`] wrappers** — and lets
//! Bevy's own picking input plugin derive `PointerInput` from them, exactly as
//! it does under a real window. One source of truth, so `ButtonInput`, the
//! accumulated mouse resources, `Window::cursor_position()` and the picking
//! pointer can never disagree.
//!
//! # The frame protocol
//!
//! Picking hits use the **previous** frame's layout (`ui_picking` runs in
//! `PreUpdate` against the `ComputedNode`s of the last `PostUpdate`), and the
//! widget state machines read `just_pressed` one frame and `pressed` the next.
//! So every pointer step is its own `update()`: a click is move, press,
//! release — three frames — and a drag steps the cursor one `update()` at a
//! time. [`click`] also pins the multi-click interval to zero around its
//! frames, because the click counter is wall-clock and two test clicks in
//! consecutive frames would otherwise read as a double click.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::{ButtonState, InputPlugin};
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
use bevy::picking::PickingSettings;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{UiStack, UiSystems, ui_focus_system, ui_stack_system};
use bevy::window::{PrimaryWindow, WindowEvent, WindowResolution};

use crate::{LayoutTest, record};
use sl_viewer_ui_core::ui_element::UiAction;

/// A [`crate::LayoutTest`] with the input and picking stack on top: a window, a
/// mouse pointer, Bevy's picking core and UI backend, the UI stack, focus
/// dispatch and the widget interaction systems — everything a click needs to
/// travel from a synthetic cursor to a widget observer, with no renderer.
#[derive(Debug, Clone, Copy)]
pub struct InteractionTest {
    /// The layout half.
    layout: LayoutTest,
}

impl Default for InteractionTest {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractionTest {
    /// The default layout, interactive.
    #[must_use]
    pub fn new() -> Self {
        Self::over(LayoutTest::new())
    }

    /// A specific layout, interactive.
    #[must_use]
    pub const fn over(layout: LayoutTest) -> Self {
        Self { layout }
    }

    /// Build the app: the layout harness plus the input stack.
    pub fn build(self) -> App {
        let viewport = self.layout.viewport();
        let scale_factor = self.layout.scale_factor();
        let mut app = self.layout.build();
        install_input_stack(&mut app, viewport, scale_factor);

        // Visibility propagation (pure ECS — `bevy_camera`, not the renderer):
        // `Node` requires `Visibility`, but `InheritedVisibility` stays at its
        // hidden default until this plugin's propagate system runs, and
        // `ui_picking` skips any node not propagated visible. The plugin's
        // mesh-bounds systems read these two asset stores; a UI-only world has
        // no `MeshPlugin`, so empty stores stand in (an absent resource fails
        // system-parameter validation with a panic).
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>();
        app.add_plugins(bevy::camera::visibility::VisibilityPlugin);

        install_ui_interaction(&mut app);
        record::<UiAction>(&mut app);
        app
    }
}

/// The UI half of the interaction stack — what turns a pointer that has
/// already reached the app into a widget interaction — on its own, for a host
/// that brought its own window and picking core (a fixture world composing the
/// UI on top of the world fold).
///
/// Requires the input stack ([`install_input_stack`]), the layout stack
/// ([`crate::LayoutTest::install`]) and visibility propagation to be present
/// already: this adds only the pieces `bevy_ui`'s own `UiPlugin` would.
pub fn install_ui_interaction(app: &mut App) {
    // The UI stack and its writer: `ui_picking` reads the back-to-front
    // node list `UiPlugin` fills, so the harness fills it the same way.
    app.init_resource::<UiStack>();
    app.configure_sets(PostUpdate, UiSystems::Stack.after(UiSystems::Layout));
    app.add_systems(PostUpdate, ui_stack_system.in_set(UiSystems::Stack));
    // The UI picking backend itself, and the `Interaction` component drive
    // (hover / pressed parity with the live viewer).
    app.add_plugins(UiPickingPlugin);
    app.add_systems(PreUpdate, ui_focus_system.in_set(UiSystems::Focus));
    // The headless widget interaction systems: press and click become
    // `Activate` on the widgets that carry them.
    app.add_plugins(bevy::ui_widgets::UiWidgetsPlugins);
    // The keyboard half of focus, which `ViewerUiPlugin` adds live: without it
    // the `Tab` key is inert — the observer that reads it is installed on the
    // primary window at `Startup` by this plugin, and by nothing else.
    app.add_plugins(bevy::input_focus::tab_navigation::TabNavigationPlugin);
}

/// Add the input half on its own — the window, time, input plugins, picking
/// core and focus dispatch — to an app that is not a [`crate::LayoutTest`]
/// (a fixture world). Returns the window entity.
pub fn install_input_stack(app: &mut App, viewport: UVec2, scale_factor: f32) -> Entity {
    // Deterministic frames: the pointer protocol counts updates, so the clock
    // must too.
    app.add_plugins(bevy::time::TimePlugin);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        core::time::Duration::from_millis(16),
    ));
    app.add_plugins(InputPlugin);
    // One primary window, never presented: pickings' hit tests and the
    // viewer's `cursor_position()` readers both go through it. The real
    // `WindowPlugin` (windowing types only — winit is a separate plugin)
    // spawns it AND registers every window message type; a hand-spawned
    // window misses registrations like `Ime` that the text-input widgets
    // read, and an unregistered `Messages<T>` panics param validation the
    // first time such a system runs.
    // `viewport` is physical pixels, which is what the resolution takes.
    let mut window = Window {
        resolution: WindowResolution::new(viewport.x, viewport.y),
        ..Window::default()
    };
    window
        .resolution
        .set_scale_factor_override(Some(scale_factor));
    app.add_plugins(bevy::window::WindowPlugin {
        primary_window: Some(window),
        primary_cursor_options: None,
        exit_condition: bevy::window::ExitCondition::DontExit,
        close_when_requested: false,
    });
    let window_entity = {
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        windows.single(app.world()).unwrap_or(Entity::PLACEHOLDER)
    };
    // The picking core: pointer bookkeeping, hover map, interaction states,
    // and the input plugin that turns `WindowEvent`s into `PointerInput`.
    app.add_plugins(DefaultPickingPlugins);
    // Focus bookkeeping and the dispatch that routes keys to the focused
    // widget (`FocusedInput` — Enter / Space activation, text entry).
    app.add_plugins((InputFocusPlugin, InputDispatchPlugin));
    window_entity
}

/// The primary window entity, for message stamping.
fn window_entity(app: &mut App) -> Entity {
    let mut windows = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>();
    windows.single(app.world()).unwrap_or(Entity::PLACEHOLDER)
}

/// The pointer's current logical position, as the window knows it.
#[must_use]
pub fn cursor(app: &mut App) -> Option<Vec2> {
    let mut windows = app
        .world_mut()
        .query_filtered::<&Window, With<PrimaryWindow>>();
    windows
        .single(app.world())
        .ok()
        .and_then(Window::cursor_position)
}

/// The centre of the named node, in logical pixels — where a user aiming at it
/// would put the pointer. `None` when no such node laid out.
#[must_use]
pub fn centre_of(app: &mut App, name: &str) -> Option<Vec2> {
    let entity = crate::find_by_name(app, name)?;
    let node = app.world().get::<ComputedNode>(entity)?;
    let transform = app.world().get::<UiGlobalTransform>(entity)?;
    // `UiGlobalTransform` is in physical pixels; the driver speaks logical.
    // Component-wise plain `f32`, per the workspace convention: the
    // `arithmetic_side_effects` lint fires on `glam`'s overloaded operators.
    let centre = transform.translation;
    let logical = node.inverse_scale_factor();
    Some(Vec2::new(centre.x * logical, centre.y * logical))
}

/// Move the pointer to `at` (logical pixels) and run one frame: the window's
/// cursor, the typed `CursorMoved` / `MouseMotion` and their wrappers.
pub fn hover(app: &mut App, at: Vec2) {
    let window = window_entity(app);
    let previous = cursor(app);
    let scale_factor = {
        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        let mut win = match windows.single_mut(app.world_mut()) {
            Ok(win) => win,
            Err(_missing) => return,
        };
        let scale_factor = win.scale_factor();
        // Component-wise `f32`: the lint fires on `glam` operators.
        let physical = Vec2::new(at.x * scale_factor, at.y * scale_factor);
        win.set_physical_cursor_position(Some(physical.as_dvec2()));
        scale_factor
    };
    let _unused = scale_factor;
    let delta = previous.map(|was| Vec2::new(at.x - was.x, at.y - was.y));
    let moved = CursorMoved {
        window,
        position: at,
        delta,
    };
    app.world_mut().write_message(moved.clone());
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(moved));
    if let Some(delta) = delta {
        let motion = MouseMotion { delta };
        app.world_mut().write_message(motion);
        app.world_mut()
            .write_message(WindowEvent::MouseMotion(motion));
    }
    app.update();
}

/// Move the pointer to the named node's centre. Errors when it never laid out.
///
/// # Errors
///
/// Returns the node's name when it cannot be found.
pub fn hover_node(app: &mut App, name: &str) -> Result<Vec2, String> {
    let at = centre_of(app, name).ok_or_else(|| format!("no laid-out node named `{name}`"))?;
    hover(app, at);
    Ok(at)
}

/// Press `button` where the pointer is, and run one frame.
pub fn press(app: &mut App, button: MouseButton) {
    let window = window_entity(app);
    let input = MouseButtonInput {
        button,
        state: ButtonState::Pressed,
        window,
    };
    app.world_mut().write_message(input);
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(input));
    app.update();
}

/// Release `button` where the pointer is, and run one frame.
pub fn release(app: &mut App, button: MouseButton) {
    let window = window_entity(app);
    let input = MouseButtonInput {
        button,
        state: ButtonState::Released,
        window,
    };
    app.world_mut().write_message(input);
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(input));
    app.update();
}

/// One single click at `at`: move, press, release — each its own frame — with
/// the multi-click interval pinned to zero so two test clicks in consecutive
/// frames are two singles, not a double. One settling frame at the end lets
/// the widgets' own observers run before the caller drains.
pub fn click(app: &mut App, at: Vec2, button: MouseButton) {
    let interval = app
        .world()
        .get_resource::<PickingSettings>()
        .map(|settings| settings.multi_click_interval);
    if let Some(mut settings) = app.world_mut().get_resource_mut::<PickingSettings>() {
        settings.multi_click_interval = core::time::Duration::ZERO;
    }
    hover(app, at);
    press(app, button);
    release(app, button);
    app.update();
    if let (Some(interval), Some(mut settings)) = (
        interval,
        app.world_mut().get_resource_mut::<PickingSettings>(),
    ) {
        settings.multi_click_interval = interval;
    }
}

/// A single left click on the named node's centre.
///
/// # Errors
///
/// Returns the node's name when it cannot be found.
pub fn click_node(app: &mut App, name: &str) -> Result<(), String> {
    let at = hover_node(app, name)?;
    click(app, at, MouseButton::Left);
    Ok(())
}

/// Two clicks at `at` under the default multi-click interval, so the second
/// carries `count == 2` — the double click the widgets read.
pub fn double_click(app: &mut App, at: Vec2, button: MouseButton) {
    hover(app, at);
    press(app, button);
    release(app, button);
    press(app, button);
    release(app, button);
    app.update();
}

/// Press at `from`, step the pointer to `to` across `steps` frames, release —
/// the shape every drag reader (title bars, gizmos, sliders) consumes.
pub fn drag(app: &mut App, from: Vec2, to: Vec2, steps: u32, button: MouseButton) {
    hover(app, from);
    press(app, button);
    let count = steps.max(1);
    for step in 1..=count {
        let t = f32::from(u16::try_from(step).unwrap_or(u16::MAX))
            / f32::from(u16::try_from(count).unwrap_or(u16::MAX));
        hover(app, from.lerp(to, t));
    }
    release(app, button);
    app.update();
}

/// Scroll `lines` at `at` (vertical positive = away from the user).
pub fn scroll(app: &mut App, at: Vec2, lines: Vec2) {
    hover(app, at);
    let window = window_entity(app);
    let wheel = MouseWheel {
        unit: MouseScrollUnit::Line,
        x: lines.x,
        y: lines.y,
        window,
        phase: bevy::input::touch::TouchPhase::Moved,
    };
    app.world_mut().write_message(wheel);
    app.world_mut()
        .write_message(WindowEvent::MouseWheel(wheel));
    app.update();
}

/// Raw relative mouse motion with **no** cursor move — what mouselook reads.
pub fn hold_mouse_motion(app: &mut App, delta: Vec2) {
    let motion = MouseMotion { delta };
    app.world_mut().write_message(motion);
    app.world_mut()
        .write_message(WindowEvent::MouseMotion(motion));
    app.update();
}

/// Press `key` down (with its logical meaning and optional text), one frame.
pub fn key_down(app: &mut App, key_code: KeyCode, logical: Key, text: Option<&str>) {
    write_key(app, key_code, logical, text, ButtonState::Pressed);
}

/// Release `key`, one frame.
pub fn key_up(app: &mut App, key_code: KeyCode, logical: Key) {
    write_key(app, key_code, logical, None, ButtonState::Released);
}

/// Tap `key`: down, up — two frames.
pub fn tap(app: &mut App, key_code: KeyCode, logical: Key) {
    key_down(app, key_code, logical.clone(), None);
    key_up(app, key_code, logical);
}

/// Type `text`, one character key per frame pair, as an IME-less keyboard
/// delivers it.
pub fn type_str(app: &mut App, text: &str) {
    for character in text.chars() {
        let logical = Key::Character(character.to_string().into());
        key_down(
            app,
            KeyCode::F35,
            logical.clone(),
            Some(&character.to_string()),
        );
        key_up(app, KeyCode::F35, logical);
    }
}

/// The shared keyboard write: the typed message plus its wrapper.
fn write_key(
    app: &mut App,
    key_code: KeyCode,
    logical: Key,
    text: Option<&str>,
    state: ButtonState,
) {
    let window = window_entity(app);
    let input = KeyboardInput {
        key_code,
        logical_key: logical,
        state,
        text: text.map(Into::into),
        repeat: false,
        window,
    };
    app.world_mut().write_message(input.clone());
    app.world_mut()
        .write_message(WindowEvent::KeyboardInput(input));
    app.update();
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{InteractionTest, click, cursor, double_click, hover};
    use crate::settle;

    /// Where clicks land, recorded by an observer on each test node.
    #[derive(Resource, Default)]
    struct Clicks(Vec<(String, u8)>);

    /// Record every `Pointer<Click>` on the observed node, with its count.
    fn observe_clicks(app: &mut App, entity: Entity, name: &str) {
        let name = name.to_owned();
        app.world_mut().entity_mut(entity).observe(
            move |click: On<Pointer<Click>>, mut clicks: ResMut<Clicks>| {
                clicks.0.push((name.clone(), click.count));
            },
        );
    }

    /// A solid absolutely-placed node.
    fn solid_node(left: f32, top: f32, width: f32, height: f32) -> Node {
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(top),
            width: Val::Px(width),
            height: Val::Px(height),
            ..Node::default()
        }
    }

    fn interactive_app() -> App {
        let mut app = InteractionTest::new().build();
        app.init_resource::<Clicks>();
        app
    }

    /// **A click lands on the node under the pointer, and nowhere else.**
    #[test]
    fn a_click_lands_on_the_node_under_the_pointer() {
        let mut app = interactive_app();
        let node = app
            .world_mut()
            .spawn(solid_node(10.0, 10.0, 100.0, 40.0))
            .id();
        observe_clicks(&mut app, node, "target");
        settle(&mut app);

        click(&mut app, Vec2::new(60.0, 30.0), MouseButton::Left);
        assert_eq!(
            app.world().resource::<Clicks>().0,
            vec![("target".to_owned(), 1)]
        );
        // Beside the node: nothing.
        click(&mut app, Vec2::new(300.0, 300.0), MouseButton::Left);
        assert_eq!(app.world().resource::<Clicks>().0.len(), 1);
    }

    /// **An opaque overlay blocks the click** — the coverage `activate` never
    /// had: the control must actually be reachable where the pointer is.
    #[test]
    fn an_overlay_on_top_of_a_node_takes_its_click() {
        let mut app = interactive_app();
        let below = app
            .world_mut()
            .spawn(solid_node(10.0, 10.0, 100.0, 40.0))
            .id();
        observe_clicks(&mut app, below, "below");
        // Spawned later at the same spot: later in the UI stack, so on top.
        let overlay = app
            .world_mut()
            .spawn(solid_node(0.0, 0.0, 200.0, 200.0))
            .id();
        observe_clicks(&mut app, overlay, "overlay");
        settle(&mut app);

        click(&mut app, Vec2::new(60.0, 30.0), MouseButton::Left);
        let clicks = &app.world().resource::<Clicks>().0;
        assert_eq!(
            clicks.first().map(|(name, _count)| name.as_str()),
            Some("overlay"),
            "the overlay stands between the pointer and the node, so it must take the \
             click — a hit through an overlay is exactly the bug this tier exists to catch \
             (got {clicks:?})"
        );
        assert!(
            !clicks.iter().any(|(name, _count)| name == "below"),
            "the covered node must not also be clicked (got {clicks:?})"
        );

        // Remove the overlay: the click reaches the node — so the block above
        // was the overlay's doing, not a dead pointer.
        app.world_mut().entity_mut(overlay).despawn();
        settle(&mut app);
        click(&mut app, Vec2::new(60.0, 30.0), MouseButton::Left);
        assert!(
            app.world()
                .resource::<Clicks>()
                .0
                .iter()
                .any(|(name, _count)| name == "below"),
            "with the overlay gone the node must be clickable"
        );
    }

    /// **Two [`click`]s are two singles; a [`double_click`] carries count 2.**
    #[test]
    fn two_clicks_are_two_singles_and_a_double_click_counts_two() {
        let mut app = interactive_app();
        let node = app
            .world_mut()
            .spawn(solid_node(10.0, 10.0, 100.0, 40.0))
            .id();
        observe_clicks(&mut app, node, "target");
        settle(&mut app);

        let at = Vec2::new(60.0, 30.0);
        click(&mut app, at, MouseButton::Left);
        click(&mut app, at, MouseButton::Left);
        let counts: Vec<u8> = app
            .world()
            .resource::<Clicks>()
            .0
            .iter()
            .map(|(_name, count)| *count)
            .collect();
        assert_eq!(counts, vec![1, 1], "consecutive test clicks must not merge");

        double_click(&mut app, at, MouseButton::Left);
        let counts: Vec<u8> = app
            .world()
            .resource::<Clicks>()
            .0
            .iter()
            .map(|(_name, count)| *count)
            .collect();
        assert!(
            counts.contains(&2),
            "a deliberate double click must reach the widgets as one (counts {counts:?})"
        );
    }

    /// Which node saw which key press, in order.
    #[derive(Resource, Default)]
    struct Keys(Vec<(String, KeyCode)>);

    /// Record every focused key *press* that reaches the observed node.
    fn observe_keys(app: &mut App, entity: Entity, name: &str) {
        let name = name.to_owned();
        app.world_mut().entity_mut(entity).observe(
            move |key: On<bevy::input_focus::FocusedInput<KeyboardInput>>,
                  mut keys: ResMut<Keys>| {
                if key.input.state == ButtonState::Pressed {
                    keys.0.push((name.clone(), key.input.key_code));
                }
            },
        );
    }

    /// **A key reaches the focused node, and nothing else.**
    ///
    /// The keyboard's half of the coverage `trigger(Activate)` never had: an
    /// activation triggered on an entity is delivered there by construction,
    /// so it cannot say whether a *typed* key would have gone to that widget.
    /// Here the key is written as winit writes it and routed by
    /// `InputDispatchPlugin` to whatever [`InputFocus`] names — which is what
    /// decides, in the running viewer, whether typing walks the avatar or
    /// lands in the chat bar.
    #[test]
    fn a_key_reaches_only_the_focused_node() {
        let mut app = interactive_app();
        app.init_resource::<Keys>();
        let first = app
            .world_mut()
            .spawn((solid_node(10.0, 10.0, 100.0, 40.0), TabIndex(0)))
            .id();
        let second = app
            .world_mut()
            .spawn((solid_node(10.0, 60.0, 100.0, 40.0), TabIndex(1)))
            .id();
        observe_keys(&mut app, first, "first");
        observe_keys(&mut app, second, "second");
        settle(&mut app);

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(first, FocusCause::Navigated);
        super::tap(&mut app, KeyCode::KeyA, Key::Character("a".into()));
        assert_eq!(
            app.world().resource::<Keys>().0,
            vec![("first".to_owned(), KeyCode::KeyA)],
            "the focused node takes the key, and the unfocused one hears nothing"
        );

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(second, FocusCause::Navigated);
        super::tap(&mut app, KeyCode::KeyB, Key::Character("b".into()));
        assert_eq!(
            app.world().resource::<Keys>().0,
            vec![
                ("first".to_owned(), KeyCode::KeyA),
                ("second".to_owned(), KeyCode::KeyB)
            ],
            "moving the focus moves where the keys land"
        );
    }

    /// **The `Tab` key itself moves focus** — not `TabNavigation` called by
    /// hand, which is what [`crate::navigate`] does.
    ///
    /// Worth the distinction: the observer that reads `Tab` is installed on the
    /// primary window at `Startup` by `TabNavigationPlugin`, so a harness that
    /// forgot the plugin (or the window) would navigate perfectly through
    /// [`crate::navigate`] while the key did nothing at all — which is exactly
    /// the shape of the bug a keyboard test is for.
    #[test]
    fn the_tab_key_moves_focus() {
        let mut app = interactive_app();
        let first = app
            .world_mut()
            .spawn((solid_node(10.0, 10.0, 100.0, 40.0), TabIndex(0)))
            .id();
        let second = app
            .world_mut()
            .spawn((solid_node(10.0, 60.0, 100.0, 40.0), TabIndex(1)))
            .id();
        app.world_mut()
            .spawn((solid_node(0.0, 0.0, 200.0, 200.0), TabGroup::new(0)))
            .add_children(&[first, second]);
        settle(&mut app);
        // `set_initial_focus` parks the focus on the primary window until
        // something takes it — so "unfocused" here means "no widget has it",
        // not `None`.
        let focused = app.world().resource::<InputFocus>().get();
        assert!(
            focused != Some(first) && focused != Some(second),
            "no tab stop may hold focus before the first Tab (it is on {focused:?})"
        );

        super::tap(&mut app, KeyCode::Tab, Key::Tab);
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(first),
            "the first Tab focuses the first stop in the group"
        );
        super::tap(&mut app, KeyCode::Tab, Key::Tab);
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(second),
            "the next Tab moves one stop on"
        );
    }

    /// **The window tracks the pointer**: `cursor_position()` — what the
    /// viewer's own world systems read — agrees with where the driver moved.
    #[test]
    fn the_window_cursor_follows_the_driver() {
        let mut app = interactive_app();
        settle(&mut app);
        assert_eq!(cursor(&mut app), None);
        hover(&mut app, Vec2::new(42.0, 17.0));
        assert_eq!(cursor(&mut app), Some(Vec2::new(42.0, 17.0)));
    }
}
