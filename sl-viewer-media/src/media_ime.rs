//! **IME composition** into a focused media surface (`viewer-media-prim-browser`):
//! the platform input method's in-progress text shown inside the page, the way
//! it is in a desktop browser, instead of only its committed result arriving as
//! typed characters.
//!
//! Two routes hold a page's keyboard focus — an in-world media face
//! (`sl_viewer_world_view::media_prim`) and a UI browser view
//! ([`crate::browser_widget`]) — and both hand the window's `Ime` messages to
//! [`MediaIme::forward`] for the surface they target. What they share lives
//! here:
//!
//! - **Whether the window's IME is on.** Bevy's text input turns
//!   `Window::ime_enabled` on for a focused `EditableText` and off otherwise,
//!   and only when the input focus *changes* — so a page, which is not an
//!   `EditableText`, would never get one. A route that holds a page's focus
//!   [`request`](MediaIme::request)s it each frame, and `apply_media_ime`
//!   turns the IME on while any route asks and off again when none does
//!   (never over a text field that has just taken the focus, whose own switch
//!   Bevy has already thrown).
//! - **Whether a composition is in progress.** While one is, the keys belong to
//!   the IME — the same rule Bevy's `EditableText` keeps — so the routes stop
//!   forwarding key events and typed text until it commits or is cancelled;
//!   otherwise a key the IME is composing with would also reach the page as a
//!   keystroke.
//!
//! The candidate window opens at the pointer — where the click that focused
//! the page landed. Placing it at the page's own caret would need CEF's
//! composition-bounds callback mapped through a prim face's projection onto
//! the screen, which is not attempted.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::window::PrimaryWindow;
use sl_cef::MediaSurface;

/// The system set the window's IME switch is applied in; a route's
/// [`MediaIme::request`] runs before it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaImeSystems;

/// The IME state shared by the page-focus routes (see the module docs).
#[derive(Resource, Debug, Default)]
pub struct MediaIme {
    /// Whether an IME composition is in progress in the focused page.
    composing: bool,
    /// Whether a route asked for the IME this frame.
    requested: bool,
    /// Whether this module turned the window's IME on (and so owns turning it
    /// off).
    enabled: bool,
}

impl MediaIme {
    /// Whether a composition is in progress — while it is, a route forwards no
    /// key events or typed text to the page.
    #[must_use]
    pub const fn composing(&self) -> bool {
        self.composing
    }

    /// Ask for the window's IME this frame (a route that holds a page's
    /// keyboard focus calls this every frame it does).
    pub const fn request(&mut self) {
        self.requested = true;
    }

    /// Hand one `Ime` message to `surface`: a preedit updates the page's
    /// composition, a commit ends it with its text, and the IME being switched
    /// off cancels whatever was in progress.
    pub fn forward(&mut self, surface: &dyn MediaSurface, ime: &Ime) {
        match ime {
            Ime::Preedit { value, cursor, .. } => {
                if value.is_empty() {
                    if self.composing {
                        surface.ime_cancel();
                    }
                    self.composing = false;
                } else {
                    surface.ime_set_composition(value, *cursor);
                    self.composing = true;
                }
            }
            Ime::Commit { value, .. } => {
                surface.ime_commit(value);
                self.composing = false;
            }
            Ime::Disabled { .. } => {
                if self.composing {
                    surface.ime_cancel();
                }
                self.composing = false;
            }
            Ime::Enabled { .. } => {}
        }
    }
}

/// The plugin for the shared IME state; added by each page-focus route's own
/// plugin, once.
#[derive(Debug, Clone, Copy, Default)]
pub struct MediaImePlugin;

impl Plugin for MediaImePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MediaIme>()
            .add_systems(Update, apply_media_ime.in_set(MediaImeSystems));
    }
}

/// Turn the window's IME on while a route asks for it, and off when none does
/// any longer — unless the focus has moved to a text field, whose IME Bevy has
/// already switched on for itself.
fn apply_media_ime(
    mut ime: ResMut<MediaIme>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    focus: Res<InputFocus>,
    fields: Query<(), With<EditableText>>,
) {
    let requested = core::mem::take(&mut ime.requested);
    let Ok((window_ime, position)) = windows
        .single()
        .map(|window| (window.ime_enabled, window.cursor_position()))
    else {
        return;
    };
    if requested {
        if !window_ime && let Ok(mut window) = windows.single_mut() {
            window.ime_enabled = true;
            if let Some(position) = position {
                window.ime_position = position;
            }
        }
        ime.enabled = true;
    } else if ime.enabled {
        ime.enabled = false;
        ime.composing = false;
        let field_focused = focus.get().is_some_and(|entity| fields.contains(entity));
        if !field_focused
            && window_ime
            && let Ok(mut window) = windows.single_mut()
        {
            window.ime_enabled = false;
        }
    }
}
