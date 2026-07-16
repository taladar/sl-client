//! Input focus / modal context (`viewer-input-focus-contexts`): who receives
//! input each frame — the world, a UI widget, or a text field — and who holds
//! the mouse cursor.
//!
//! Folded in alongside [`crate::ui`]'s widget scaffold rather than left to its
//! own change, because the two are one foundation: the scaffold is what makes
//! focus reachable, and without this a focused text field is unusable — every
//! keystroke typed into it *also* drives the avatar, since the whole viewer
//! reads `ButtonInput<KeyCode>` directly and no one had told it not to.
//!
//! # The rule
//!
//! **While anything in the UI holds focus, the world does not get the
//! keyboard** — except the `F`-key overlay toggles, which no text field could
//! want.
//!
//! This is the reference viewer's rule, and it is worth stating precisely
//! because the obvious weaker version is wrong. Firestorm's
//! `LLViewerWindow::handleKey` gives a keystroke to the focused control when
//! there *is* keyboard focus, the key is not an accelerator (`Ctrl` / `Alt`),
//! and it is "likely to generate a character" (`key < 0x80`) — only then does it
//! fall through to `LLViewerInput`'s binding table and thus to the world. Note
//! what that covers beyond the letters: **the arrow keys**, which in a text
//! field move the caret and in the world turn the avatar. Both of ours did both
//! at once.
//!
//! We apply the rule at the system level, with [`world_has_keyboard`] as a run
//! condition on every system that reads a key the UI could want, rather than by
//! filtering keys — Bevy's `ButtonInput` is a shared resource that a system
//! either consults or does not, so the gate belongs at the system.
//!
//! # Contexts
//!
//! [`InputContext`] is derived from `bevy_input_focus`'s `InputFocus` each
//! frame, never set by hand: focus *is* the state, and a second copy of it would
//! only drift. The three cases follow the reference's `acceptsTextInput()`
//! split, and are what a per-context binding profile
//! (`viewer-input-action-map`) will key off.
//!
//! Firestorm's own `keys.xml` **modes** (`MODE_FIRST_PERSON`,
//! `MODE_THIRD_PERSON`, `MODE_EDIT_AVATAR`, `MODE_SITTING` —
//! `indra/newview/llviewerinput.h`) are the *other* axis of that task: they
//! scope what a key does when the world does have the keyboard. They are
//! deliberately not modelled yet — mouselook (`viewer-camera-mouselook`) and the
//! sit/stand actions (`viewer-sit-stand-actions`) are the tasks that would give
//! them a second value to take, and an enum with one inhabitant is not a state
//! machine. [`InputContext::World`] is the seam they extend at.
//!
//! # The cursor
//!
//! The viewer grabbed the cursor unconditionally, which is why UI mouse
//! interaction was impossible: a locked pointer cannot be moved onto a button.
//! [`drive_cursor_grab`] makes the grab follow the context — the world takes it,
//! the UI releases it — so tabbing into a panel frees the mouse, and `Escape`
//! (never a letter key, which a text field would rightly eat) hands it back.
//!
//! **This is a waypoint, not the destination.** Second Life grabs the cursor in
//! **mouselook and nowhere else**: in third person the pointer is free and you
//! click the world with it, and the camera orbits on a modifier-drag rather than
//! on raw motion. So the grab properly keys off the **camera mode**, not off
//! this context at all. The reason [`InputContext::World`] takes the cursor
//! today is that the only camera we have is the debug fly-camera
//! ([`crate::camera`]), which is permanently mouselook-shaped — it steers from
//! raw mouse motion and so cannot work with a free pointer.
//!
//! When [`InputContext::World`] gains the camera modes it is a seam for
//! (`viewer-camera-mouselook`, `viewer-camera-third-person-orbit`), the
//! condition below becomes "the camera is in mouselook" and third person joins
//! the UI on the free-cursor side. At that point `crate::hud_pick`'s `H`
//! free-cursor toggle should disappear rather than be ported: it exists only to
//! escape a grab that third person will not have.
//!
//! Reference (Firestorm, read-only): `indra/newview/llviewerwindow.cpp`
//! (`handleKey` / `handleKeyUp` focus routing), `indra/newview/llviewerinput.h`
//! (the mode enum), `indra/llwindow/llkeyboard`, `llagentcamera` (the mouselook
//! transition that takes and releases the pointer).

use bevy::input_focus::{InputFocus, InputFocusSystems};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::hud_pick::HudCursorMode;

/// The key that hands the keyboard back to the world from a focused UI.
const RELEASE_FOCUS_KEY: KeyCode = KeyCode::Escape;

/// The input focus / modal context plugin. See the [module documentation](self).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InputContextPlugin;

impl Plugin for InputContextPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InputContext>()
            .add_systems(
                PreUpdate,
                // After focus dispatch, so a `Tab` press this frame is already
                // reflected: every `Update` system's `world_has_keyboard` gate
                // then reads a context that matches the focus it is gating on.
                compute_input_context.after(InputFocusSystems::Dispatch),
            )
            .add_systems(
                Update,
                (release_ui_focus_on_escape, drive_cursor_grab).chain(),
            );
    }
}

/// Whether the world context is allowed to grab the cursor at all.
///
/// `false` in screenshot mode, where the run is unattended and grabbing would
/// hijack the desktop's pointer — the reason `main` set the cursor free there in
/// the first place. Without this the context would helpfully re-grab it.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct CursorGrabAllowed(pub(crate) bool);

/// Who input belongs to this frame.
///
/// Derived from `InputFocus` by [`compute_input_context`]; never assigned by
/// hand.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum InputContext {
    /// Nothing in the UI holds focus: the world has the keyboard and the mouse.
    ///
    /// The seam the camera / movement modes (mouselook, third-person, sitting —
    /// Firestorm's `keys.xml` modes) subdivide when they arrive.
    #[default]
    World,
    /// A focusable UI node that does not take text holds focus — a button, a
    /// checkbox. `Enter` / `Space` activate it, and the world gets no keys.
    UiWidget,
    /// A text-accepting node holds focus. Characters, the arrows and `Backspace`
    /// are all its; the world gets nothing.
    TextEntry,
}

impl InputContext {
    /// Whether the world owns input right now.
    pub(crate) const fn is_world(self) -> bool {
        matches!(self, Self::World)
    }
}

/// Derive [`InputContext`] from what currently holds focus.
///
/// A focused entity that is a UI node with `EditableText` is [`TextEntry`]; any
/// other UI node is [`UiWidget`]; anything else — nothing focused, or the
/// primary window, which is what `bevy_input_focus` falls back to — is
/// [`World`]. That last case is why this needs no "is the UI open" flag: the
/// window is not a `Node`, so it reads as the world for free.
///
/// [`TextEntry`]: InputContext::TextEntry
/// [`UiWidget`]: InputContext::UiWidget
/// [`World`]: InputContext::World
fn compute_input_context(
    focus: Res<InputFocus>,
    ui_nodes: Query<Has<EditableText>, With<Node>>,
    mut context: ResMut<InputContext>,
) {
    let next = match focus.get().map(|entity| ui_nodes.get(entity)) {
        Some(Ok(true)) => InputContext::TextEntry,
        Some(Ok(false)) => InputContext::UiWidget,
        Some(Err(_)) | None => InputContext::World,
    };
    if *context != next {
        *context = next;
    }
}

/// A run condition: true while the world owns the keyboard.
///
/// Put this on every system that reads a key a focused UI could want — which is
/// all of them bar the `F`-key overlay toggles. See the
/// [module documentation](self) for why the arrow keys are in that set.
pub(crate) fn world_has_keyboard(context: Res<InputContext>) -> bool {
    context.is_world()
}

/// [`RELEASE_FOCUS_KEY`] hands the keyboard back to the world.
///
/// Only while the UI holds focus: in [`InputContext::World`] the same key still
/// reaches `crate::session`'s quit, so `Escape` reads as "back out of this" at
/// both levels. The two cannot both fire on one press, because that system is
/// gated on [`world_has_keyboard`] and the context is not recomputed until the
/// next frame's `PreUpdate`.
fn release_ui_focus_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    mut focus: ResMut<InputFocus>,
) {
    if !context.is_world() && keyboard.just_pressed(RELEASE_FOCUS_KEY) {
        focus.clear();
    }
}

/// Drive the window's cursor grab from the context: the world takes the cursor,
/// the UI releases it.
///
/// Two things override "the world takes it": the free-cursor toggle
/// (`crate::hud_pick`'s `H`), which is a deliberate request for a pointer while
/// still in the world, and [`CursorGrabAllowed`], which is false for an
/// unattended screenshot run.
fn drive_cursor_grab(
    context: Res<InputContext>,
    hud_cursor: Res<HudCursorMode>,
    allowed: Res<CursorGrabAllowed>,
    mut cursors: Query<&mut CursorOptions>,
) {
    if !context.is_changed() && !hud_cursor.is_changed() && !allowed.is_changed() {
        return;
    }
    let grab = allowed.0 && context.is_world() && !hud_cursor.active;
    let (grab_mode, visible) = if grab {
        (CursorGrabMode::Locked, false)
    } else {
        (CursorGrabMode::None, true)
    };
    for mut cursor in &mut cursors {
        if cursor.grab_mode != grab_mode {
            cursor.grab_mode = grab_mode;
        }
        if cursor.visible != visible {
            cursor.visible = visible;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CursorGrabAllowed, InputContext, InputContextPlugin, compute_input_context,
        drive_cursor_grab, release_ui_focus_on_escape, world_has_keyboard,
    };
    use crate::hud_pick::HudCursorMode;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::window::{CursorGrabMode, CursorOptions};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// An app with the context derivation wired, but none of the UI or windowing
    /// the full [`InputContextPlugin`] would want.
    fn context_app() -> App {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .init_resource::<InputContext>()
            .add_systems(Update, compute_input_context);
        app
    }

    /// The context follows what holds focus, including back to the world when
    /// focus is dropped. Focus *is* the state; the context is only a reading of
    /// it.
    #[test]
    fn the_context_follows_focus() -> Result<(), TestError> {
        let mut app = context_app();
        let button = app.world_mut().spawn(Node::default()).id();
        let editor = app
            .world_mut()
            .spawn((Node::default(), EditableText::new("hello")))
            .id();
        // Something focusable that is not a UI node at all — the stand-in for the
        // primary window, which is what `bevy_input_focus` parks focus on.
        let not_a_node = app.world_mut().spawn_empty().id();

        app.update();
        assert_eq!(
            *app.world().resource::<InputContext>(),
            InputContext::World,
            "nothing focused must read as the world"
        );

        for (entity, want) in [
            (button, InputContext::UiWidget),
            (editor, InputContext::TextEntry),
            (not_a_node, InputContext::World),
        ] {
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(entity, FocusCause::Navigated);
            app.update();
            assert_eq!(*app.world().resource::<InputContext>(), want);
        }

        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
        assert_eq!(
            *app.world().resource::<InputContext>(),
            InputContext::World,
            "dropping focus must hand the world back its keyboard"
        );
        Ok(())
    }

    /// The gate the whole module exists for: the world reads keys only when
    /// nothing in the UI has focus. If this ever inverts, typing walks the
    /// avatar again.
    #[test]
    fn the_world_only_has_the_keyboard_with_no_ui_focus() {
        for (context, want) in [
            (InputContext::World, true),
            (InputContext::UiWidget, false),
            (InputContext::TextEntry, false),
        ] {
            let mut app = App::new();
            app.insert_resource(context);
            let mut system = IntoSystem::into_system(world_has_keyboard);
            system.initialize(app.world_mut());
            assert_eq!(
                system.run((), app.world_mut()).ok(),
                Some(want),
                "{context:?} must {} give the world the keyboard",
                if want { "" } else { "not" }
            );
        }
    }

    /// `Escape` drops UI focus — and only UI focus. In the world it must pass
    /// through untouched, because there it is `crate::session`'s quit.
    #[test]
    fn escape_releases_ui_focus_but_not_world_focus() -> Result<(), TestError> {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(InputContext::TextEntry)
            .add_systems(Update, release_ui_focus_on_escape);
        let editor = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(editor, FocusCause::Navigated);

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            None,
            "escape must hand the keyboard back to the world"
        );

        // In the world, the same press must leave focus alone for `session` to
        // read as a quit.
        app.insert_resource(InputContext::World);
        let other = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(other, FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(other),
            "in the world, escape belongs to the quit handler, not to focus"
        );
        Ok(())
    }

    /// The grab follows the context — and yields to both of its overrides. The
    /// screenshot case is the one with teeth: a re-grab there hijacks the
    /// desktop pointer of an unattended run.
    #[test]
    fn the_cursor_grab_follows_the_context_and_its_overrides() -> Result<(), TestError> {
        for (context, hud_cursor, allowed, want) in [
            (InputContext::World, false, true, CursorGrabMode::Locked),
            (InputContext::UiWidget, false, true, CursorGrabMode::None),
            (InputContext::TextEntry, false, true, CursorGrabMode::None),
            // The free-cursor toggle: still the world, but the user asked for a
            // pointer.
            (InputContext::World, true, true, CursorGrabMode::None),
            // Screenshot mode: never grab, whatever the context says.
            (InputContext::World, false, false, CursorGrabMode::None),
        ] {
            let mut app = App::new();
            app.insert_resource(context)
                .insert_resource(HudCursorMode { active: hud_cursor })
                .insert_resource(CursorGrabAllowed(allowed))
                .add_systems(Update, drive_cursor_grab);
            let window = app.world_mut().spawn(CursorOptions::default()).id();
            app.update();

            let cursor = app
                .world()
                .get::<CursorOptions>(window)
                .ok_or("the window lost its `CursorOptions`")?;
            assert_eq!(
                cursor.grab_mode, want,
                "{context:?} / hud cursor {hud_cursor} / grab allowed {allowed}"
            );
            assert_eq!(
                cursor.visible,
                want == CursorGrabMode::None,
                "a grabbed cursor is hidden and a free one is shown"
            );
        }
        Ok(())
    }

    /// The plugin builds and registers the context it derives.
    #[test]
    fn the_plugin_registers_the_context() {
        let mut app = App::new();
        app.add_plugins(InputContextPlugin);
        assert!(app.world().get_resource::<InputContext>().is_some());
    }
}
