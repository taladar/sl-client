//! The input **action map** (`viewer-input-action-map`): named actions and a
//! per-mode binding-profile system that replaces the hardcoded keys in
//! [`crate::movement`] and [`crate::camera`].
//!
//! # The two axes, and where this one sits
//!
//! [`crate::input_context`] owns the *focus* axis — who receives input at all
//! (the world, a UI widget, a text field), via [`world_has_keyboard`]. This
//! module owns the *mode* axis, Firestorm's `keys.xml` **modes**
//! (`MODE_THIRD_PERSON` / `MODE_FIRST_PERSON` / …): what a physical key *does*
//! once the world has the keyboard. The two compose — a key only reaches an
//! action when the world owns the keyboard *and* the active mode's profile binds
//! it.
//!
//! # The model
//!
//! - **Named actions.** [`Action`] is the closed set of viewer actions the
//!   camera and avatar-movement systems consume. A system asks "is
//!   [`Action::MoveForward`] held?" rather than "is `W` down?", so the physical
//!   key is a rebindable detail (`viewer-input-rebinding-ui` /
//!   `-persistence` build the editor and the disk layer on top of this).
//! - **Per-mode profiles.** [`InputBindings`] holds one [`BindingProfile`] per
//!   [`InputMode`], mirroring the per-mode blocks of the reference's `keys.xml`.
//!   One physical key means different things in third-person vs. flycam — `W`
//!   walks the avatar in third person and slides the *camera* in flycam.
//! - **Many-to-one bindings.** A profile is a `key -> target` map, so several
//!   keys can drive one action (both `W` and `↑` → [`Action::MoveForward`]); a
//!   key resolves to exactly one action in a given mode.
//! - **Dynamic binding targets.** A binding's target is [`BindingTarget`], an
//!   **open** model — an [`Action`] today, but deliberately not a closed enum,
//!   because a binding may also point at a dynamic entry such as a bound
//!   inventory **gesture** (`viewer-input-gesture-bindings`). Only the
//!   [`BindingTarget::Action`] arm is resolved here; the gesture arm is carried
//!   through so the map shape does not have to change when gestures land.
//!
//! # Resolution
//!
//! [`update_action_input`] rebuilds a `ButtonInput<Action>` resource each frame
//! from the physical `ButtonInput<KeyCode>` and the active mode's profile, so a
//! consumer reads actions with the same `pressed` / `just_pressed` /
//! `just_released` interface it would read keys with. When the world does not own
//! the keyboard (a focused UI), every action reads released — the gate lives here
//! once rather than as a run-condition on every movement / camera system.
//!
//! Reference (Firestorm, read-only): `indra/newview/llviewerinput.cpp/h` (the
//! keybinding table and its `keys.xml`), `indra/llwindow/llkeyboard`.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::Uuid;

use crate::input_context::InputContext;

/// A named viewer action a binding can drive — the stable target the camera and
/// avatar-movement systems consume instead of a raw key.
///
/// The movement actions are interpreted by *whoever owns motion in the active
/// mode*: in third-person / mouselook they advertise avatar intent
/// ([`crate::movement`]); in flycam they translate the free camera
/// ([`crate::camera`]). Same action, different consumer — which is the whole
/// point of the per-[`InputMode`] profiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    /// Walk the avatar forward, or slide the flycam forward.
    MoveForward,
    /// Walk the avatar back, or slide the flycam back.
    MoveBackward,
    /// Turn the avatar left, or slide the flycam left.
    MoveLeft,
    /// Turn the avatar right, or slide the flycam right.
    MoveRight,
    /// Ascend (fly up), or raise the flycam.
    MoveUp,
    /// Descend (fly down), or lower the flycam.
    MoveDown,
    /// Run / move fast (a held modifier, not a toggle).
    Run,
    /// Toggle flight on the avatar.
    ToggleFly,
    /// Enter or leave first-person **mouselook**.
    ToggleMouselook,
    /// Enter or leave the free **flycam**.
    ToggleFlycam,
}

impl Action {
    /// Every action, for the per-frame reconciliation in [`update_action_input`].
    pub(crate) const ALL: [Self; 10] = [
        Self::MoveForward,
        Self::MoveBackward,
        Self::MoveLeft,
        Self::MoveRight,
        Self::MoveUp,
        Self::MoveDown,
        Self::Run,
        Self::ToggleFly,
        Self::ToggleMouselook,
        Self::ToggleFlycam,
    ];
}

/// What a binding points at.
///
/// Deliberately **not** a bare [`Action`]: the reference's binding table can also
/// target a dynamic entry — a bound inventory gesture — so the target is modelled
/// open from the start (`viewer-input-gesture-bindings` fills the
/// [`Gesture`](Self::Gesture) arm in). Only [`Action`](Self::Action) is resolved
/// today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    variant_size_differences,
    reason = "the dynamic gesture target carries a 16-byte asset id vs the small action arm; \
              boxing it to even the sizes would complicate the open binding-target model for a \
              negligible saving"
)]
pub(crate) enum BindingTarget {
    /// A named viewer action.
    Action(Action),
    /// A bound inventory gesture, by asset id — carried through the map but not yet
    /// triggered. Part of the open target model the action-map task requires; the
    /// gesture-binding task constructs and fires it.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the open binding-target model reserves the dynamic gesture arm; \
                      viewer-input-gesture-bindings constructs it"
        )
    )]
    Gesture(Uuid),
}

/// The keys.xml **mode** the active binding profile is chosen by: what a key does
/// once the world owns the keyboard.
///
/// Derived from the camera mode by [`crate::camera`] (never assigned by hand),
/// this is the second axis of `viewer-input-focus-contexts` — the one that task
/// deliberately left with a single inhabitant until mouselook / flycam gave it
/// more. `Sitting` / `EditAvatar` (`viewer-sit-stand-actions`) extend it later.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) enum InputMode {
    /// Orbiting third-person: the movement keys drive the avatar.
    #[default]
    ThirdPerson,
    /// First-person mouselook: the movement keys still drive the avatar; the
    /// mouse aims.
    Mouselook,
    /// Free flycam: the movement keys drive the camera, not the avatar.
    Flycam,
}

/// One mode's `key -> target` bindings.
///
/// A `key -> target` map is many-to-one by construction: several keys may map to
/// the same target (both `W` and `↑` → [`Action::MoveForward`]), while a key
/// resolves to exactly one target in the mode.
#[derive(Clone, Debug, Default)]
pub(crate) struct BindingProfile {
    /// The physical key → binding-target map for this mode.
    bindings: HashMap<KeyCode, BindingTarget>,
}

impl BindingProfile {
    /// Bind `key` to `target`, returning the profile so builders can chain.
    fn bind(mut self, key: KeyCode, target: BindingTarget) -> Self {
        self.bindings.insert(key, target);
        self
    }

    /// Bind `key` to a plain [`Action`].
    fn action(self, key: KeyCode, action: Action) -> Self {
        self.bind(key, BindingTarget::Action(action))
    }

    /// The target bound to `key` in this profile, if any.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the single-key lookup is the accessor the rebinding UI \
                      (viewer-input-rebinding-ui) will read; resolution itself iterates"
        )
    )]
    pub(crate) fn target(&self, key: KeyCode) -> Option<BindingTarget> {
        self.bindings.get(&key).copied()
    }

    /// Every `(key, target)` binding in this profile.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (KeyCode, BindingTarget)> + '_ {
        self.bindings.iter().map(|(key, target)| (*key, *target))
    }
}

/// The per-mode binding profiles. See the [module documentation](self).
#[derive(Resource, Clone, Debug)]
pub(crate) struct InputBindings {
    /// One profile per [`InputMode`].
    profiles: HashMap<InputMode, BindingProfile>,
}

impl InputBindings {
    /// The active mode's profile (an empty profile if a mode somehow has none, so
    /// resolution degrades to "no action bound" rather than panicking).
    #[must_use]
    fn profile(&self, mode: InputMode) -> Option<&BindingProfile> {
        self.profiles.get(&mode)
    }
}

impl Default for InputBindings {
    /// The reference-faithful default bindings.
    ///
    /// Third-person and mouselook share an avatar-driving profile — `W`/`↑`
    /// forward, `A`/`←` turn, `Shift` run, `F` fly, `PageUp`/`E` up while flying —
    /// mirroring the reference's default `keys.xml`. Flycam rebinds the same WASD
    /// cluster to the *camera* (`Space`/`Ctrl` for vertical), the difference the
    /// per-mode split exists to express. Mouselook and flycam are toggled with the
    /// same keys from either side so they read as their own off switch.
    fn default() -> Self {
        // Avatar-driving profile, shared by third-person and mouselook.
        let avatar = BindingProfile::default()
            .action(KeyCode::KeyW, Action::MoveForward)
            .action(KeyCode::ArrowUp, Action::MoveForward)
            .action(KeyCode::KeyS, Action::MoveBackward)
            .action(KeyCode::ArrowDown, Action::MoveBackward)
            .action(KeyCode::KeyA, Action::MoveLeft)
            .action(KeyCode::ArrowLeft, Action::MoveLeft)
            .action(KeyCode::KeyD, Action::MoveRight)
            .action(KeyCode::ArrowRight, Action::MoveRight)
            .action(KeyCode::PageUp, Action::MoveUp)
            .action(KeyCode::KeyE, Action::MoveUp)
            .action(KeyCode::PageDown, Action::MoveDown)
            .action(KeyCode::KeyC, Action::MoveDown)
            .action(KeyCode::ShiftLeft, Action::Run)
            .action(KeyCode::ShiftRight, Action::Run)
            .action(KeyCode::KeyF, Action::ToggleFly)
            .action(KeyCode::KeyM, Action::ToggleMouselook);

        // Flycam: the WASD cluster drives the camera; `E` / `C` (and
        // `PageUp` / `PageDown`) raise and lower it, `Shift` moves fast. `M`
        // still drops to mouselook. Deliberately NOT `Space` / `Ctrl`: a bare
        // modifier as a movement key collides with every chord shortcut —
        // holding `Ctrl` for `Ctrl+B` would sink the camera.
        let flycam = BindingProfile::default()
            .action(KeyCode::KeyW, Action::MoveForward)
            .action(KeyCode::KeyS, Action::MoveBackward)
            .action(KeyCode::KeyA, Action::MoveLeft)
            .action(KeyCode::KeyD, Action::MoveRight)
            .action(KeyCode::KeyE, Action::MoveUp)
            .action(KeyCode::PageUp, Action::MoveUp)
            .action(KeyCode::KeyC, Action::MoveDown)
            .action(KeyCode::PageDown, Action::MoveDown)
            .action(KeyCode::ShiftLeft, Action::Run)
            .action(KeyCode::ShiftRight, Action::Run)
            .action(KeyCode::KeyM, Action::ToggleMouselook);

        let mut profiles = HashMap::new();
        profiles.insert(InputMode::ThirdPerson, avatar.clone());
        profiles.insert(InputMode::Mouselook, avatar);
        profiles.insert(InputMode::Flycam, flycam);
        Self { profiles }
    }
}

/// The input action-map plugin: registers the bindings, the active mode, the
/// derived `ButtonInput<Action>`, and the per-frame resolution.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InputActionPlugin;

impl Plugin for InputActionPlugin {
    /// Wire the action map. Resolution runs in `PreUpdate` after the input
    /// context is computed (so its `world` reading is this frame's) and before any
    /// `Update` consumer reads `ButtonInput<Action>`.
    fn build(&self, app: &mut App) {
        app.init_resource::<InputBindings>()
            .init_resource::<InputMode>()
            .init_resource::<ButtonInput<Action>>()
            .add_systems(
                PreUpdate,
                update_action_input.after(crate::input_context::compute_input_context),
            );
    }
}

/// Rebuild `ButtonInput<Action>` from the physical keyboard and the active mode's
/// profile.
///
/// A binding's action is "down" when any key bound to it is pressed *and* the
/// world owns the keyboard; the previous frame's state is reconciled against that
/// so `just_pressed` / `just_released` come out right. When a UI holds focus every
/// action reads released — the single gate that would otherwise be a
/// [`world_has_keyboard`](crate::input_context::world_has_keyboard) run-condition
/// on every movement / camera system.
pub(crate) fn update_action_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    bindings: Res<InputBindings>,
    mode: Res<InputMode>,
    context: Res<InputContext>,
    mut actions: ResMut<ButtonInput<Action>>,
) {
    // Reset just_pressed / just_released for the new frame; pressed is preserved
    // and reconciled below.
    actions.clear();

    // Which actions are down this frame: any bound key pressed, but only while the
    // world owns the keyboard. A focused UI leaves the set empty, so every action
    // releases.
    let mut down = std::collections::HashSet::new();
    if context.is_world()
        && let Some(profile) = bindings.profile(*mode)
    {
        for (key, target) in profile.iter() {
            if let BindingTarget::Action(action) = target
                && keyboard.pressed(key)
            {
                down.insert(action);
            }
        }
    }

    // Reconcile every action against the previous frame so the edge events fire.
    for action in Action::ALL {
        let is_down = down.contains(&action);
        let was_down = actions.pressed(action);
        if is_down && !was_down {
            actions.press(action);
        } else if !is_down && was_down {
            actions.release(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Action, BindingProfile, BindingTarget, InputBindings, InputMode, update_action_input,
    };
    use crate::input_context::InputContext;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// An app wired with the action-map resources but none of the windowing.
    fn action_app(context: InputContext) -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputBindings>()
            .init_resource::<InputMode>()
            .init_resource::<ButtonInput<Action>>()
            .insert_resource(context)
            .add_systems(Update, update_action_input);
        app
    }

    /// Press a physical key and step the world one frame.
    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
    }

    /// Both `W` and `↑` resolve to the same forward action — the many-to-one
    /// requirement — and releasing the key releases the action.
    #[test]
    fn many_keys_map_to_one_action() {
        for key in [KeyCode::KeyW, KeyCode::ArrowUp] {
            let mut app = action_app(InputContext::World);
            press(&mut app, key);
            let actions = app.world().resource::<ButtonInput<Action>>();
            assert!(actions.pressed(Action::MoveForward), "{key:?} → forward");
            assert!(actions.just_pressed(Action::MoveForward));

            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key);
            app.update();
            let actions = app.world().resource::<ButtonInput<Action>>();
            assert!(!actions.pressed(Action::MoveForward));
            assert!(actions.just_released(Action::MoveForward));
        }
    }

    /// The same physical key means different things in different modes: the
    /// arrow keys drive the avatar in third-person but are unbound in flycam
    /// (which keeps the WASD cluster only), while `E` raises in both.
    #[test]
    fn a_key_resolves_per_mode() {
        // `ArrowUp` is bound only in the avatar profile.
        let mut app = action_app(InputContext::World);
        *app.world_mut().resource_mut::<InputMode>() = InputMode::Flycam;
        press(&mut app, KeyCode::ArrowUp);
        assert!(
            !app.world()
                .resource::<ButtonInput<Action>>()
                .pressed(Action::MoveForward),
            "ArrowUp does nothing in flycam"
        );

        let mut app = action_app(InputContext::World);
        *app.world_mut().resource_mut::<InputMode>() = InputMode::Flycam;
        press(&mut app, KeyCode::KeyE);
        assert!(
            app.world()
                .resource::<ButtonInput<Action>>()
                .pressed(Action::MoveUp),
            "E raises the flycam in flycam mode"
        );
    }

    /// Bare modifiers are never movement keys: holding `Ctrl` (a chord
    /// modifier — `Ctrl+B` opens the build tools) must not sink the flycam,
    /// and `Space` is likewise unbound there.
    #[test]
    fn flycam_has_no_bare_modifier_movement() {
        for key in [KeyCode::ControlLeft, KeyCode::Space] {
            let mut app = action_app(InputContext::World);
            *app.world_mut().resource_mut::<InputMode>() = InputMode::Flycam;
            press(&mut app, key);
            let actions = app.world().resource::<ButtonInput<Action>>();
            assert!(
                !actions.pressed(Action::MoveDown) && !actions.pressed(Action::MoveUp),
                "{key:?} must not move the flycam"
            );
        }
    }

    /// A focused UI takes every action away, so typing never drives the avatar or
    /// camera — the gate the module centralises.
    #[test]
    fn a_focused_ui_suppresses_all_actions() {
        let mut app = action_app(InputContext::TextEntry);
        press(&mut app, KeyCode::KeyW);
        assert!(
            !app.world()
                .resource::<ButtonInput<Action>>()
                .pressed(Action::MoveForward),
            "no action fires while a UI holds focus"
        );
    }

    /// The dynamic-target arm is carried through the map even though it is not yet
    /// resolved to a `ButtonInput<Action>` — the shape is open for gestures.
    #[test]
    fn gesture_targets_are_representable_and_ignored() {
        let profile = BindingProfile::default();
        let profile = super::BindingProfile {
            bindings: [(
                KeyCode::KeyG,
                BindingTarget::Gesture(sl_client_bevy::Uuid::nil()),
            )]
            .into_iter()
            .chain(profile.iter())
            .collect(),
        };
        assert_eq!(
            profile.target(KeyCode::KeyG),
            Some(BindingTarget::Gesture(sl_client_bevy::Uuid::nil()))
        );
    }

    /// Every mode has a profile, so resolution never falls through to "no bindings".
    #[test]
    fn every_mode_has_a_profile() {
        let bindings = InputBindings::default();
        for mode in [
            InputMode::ThirdPerson,
            InputMode::Mouselook,
            InputMode::Flycam,
        ] {
            assert!(bindings.profile(mode).is_some(), "{mode:?} has a profile");
        }
    }
}
