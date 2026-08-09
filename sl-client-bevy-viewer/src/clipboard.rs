//! Shared OS-clipboard access for the viewer's **"Copy …"** affordances — Copy
//! SLURL on the world map and the avatar / group profiles.
//!
//! A single `arboard` handle is opened lazily and **kept alive** in a resource:
//! on Linux (X11 / Wayland) the clipboard offer is served by the owning process,
//! so dropping the handle can drop the copied selection before the user pastes.
//! The world map keeps its own handle for historical reasons; new "Copy" sites
//! share this one.

use std::sync::Mutex;

use bevy::prelude::*;

/// The kept-alive OS clipboard handle, opened on first use.
#[derive(Resource, Default)]
pub(crate) struct ViewerClipboard(Mutex<Option<arboard::Clipboard>>);

/// Copy `text` to the OS clipboard, lazily opening (and keeping) the handle. A
/// missing / failing clipboard is logged, not fatal.
pub(crate) fn copy_to_clipboard(clipboard: &ViewerClipboard, text: &str) {
    let Ok(mut holder) = clipboard.0.lock() else {
        return;
    };
    if holder.is_none() {
        match arboard::Clipboard::new() {
            Ok(handle) => *holder = Some(handle),
            Err(error) => {
                warn!("clipboard unavailable: {error}");
                return;
            }
        }
    }
    if let Some(handle) = holder.as_mut()
        && let Err(error) = handle.set_text(text.to_owned())
    {
        warn!("could not copy to the clipboard: {error}");
    }
}

/// Registers the shared clipboard resource.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ClipboardPlugin;

impl Plugin for ClipboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewerClipboard>();
    }
}
