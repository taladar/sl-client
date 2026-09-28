//! The viewer's clipboard: the one Bevy's `ClipboardPlugin` registers, which
//! the text fields' Ctrl+C / Ctrl+V already use, shared by the **"Copy …"**
//! affordances — Copy SLURL on the world map and the landmark, the avatar /
//! group profiles, About's Copy Info, the debug settings' name copy and RLV's
//! restriction list.
//!
//! There is exactly one: on Linux (X11 / Wayland) the clipboard offer is served
//! by the owning process, so a second live handle in the same process is a
//! second selection owner, and which of the two the compositor asks for the
//! bytes is not the viewer's to decide. The interactive viewer builds Bevy with
//! `system_clipboard`, so that one handle is the OS clipboard, kept alive for
//! the App's lifetime.
//!
//! A viewer that must not touch the desktop — a headless or test App — gets a
//! **private** clipboard instead ([`Clipboard::in_process`]): a copy in it is
//! what a paste in the same App reads back, and the user's clipboard is never
//! read or written. [`use_private_clipboard`] installs it; a plugin that reads
//! the clipboard and may stand alone falls back to one with
//! [`init_private_clipboard`] rather than opening the OS clipboard itself.

use bevy::prelude::*;

/// Copy `text` to the viewer's clipboard. A failing clipboard is logged, not
/// fatal: the copy was a convenience, and nothing else in the action depends
/// on it.
pub fn copy_to_clipboard(clipboard: &mut Clipboard, text: &str) {
    if let Err(error) = clipboard.set_text(text) {
        warn!("could not copy to the clipboard: {error}");
    }
}

/// Give `app` a private, in-process clipboard, replacing any it has.
///
/// Before `DefaultPlugins` (or any plugin bringing Bevy's `ClipboardPlugin`),
/// so the system clipboard is never opened at all: the plugin only
/// initialises a clipboard when none is present.
pub fn use_private_clipboard(app: &mut App) {
    app.insert_resource(Clipboard::in_process());
}

/// Give `world` a private clipboard unless it already has one — the fallback
/// for a plugin that reads the clipboard and may be added to a host without
/// Bevy's `ClipboardPlugin` (a gallery card, a test). A host that has the
/// plugin keeps its clipboard, the OS one included.
pub fn init_private_clipboard(world: &mut World) {
    if !world.contains_resource::<Clipboard>() {
        world.insert_resource(Clipboard::in_process());
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{copy_to_clipboard, init_private_clipboard, use_private_clipboard};

    /// The text a paste reads back, or the error that says why there is none.
    fn paste(clipboard: &mut Clipboard) -> Result<String, String> {
        clipboard
            .fetch_text()
            .poll_result()
            .ok_or_else(|| "the read never completed".to_owned())?
            .map_err(|error| error.to_string())
    }

    /// A private clipboard pastes back what was copied into it — the round
    /// trip a copy/paste test relies on, with no desktop involved.
    #[test]
    fn a_private_clipboard_pastes_what_was_copied() -> Result<(), String> {
        let mut app = App::new();
        use_private_clipboard(&mut app);
        let mut clipboard = app.world_mut().resource_mut::<Clipboard>();
        assert_eq!(paste(&mut clipboard)?, "", "a new clipboard starts empty");
        copy_to_clipboard(&mut clipboard, "secondlife:///app/region/Ahern/128/128/25");
        assert_eq!(
            paste(&mut clipboard)?,
            "secondlife:///app/region/Ahern/128/128/25"
        );
        Ok(())
    }

    /// Two Apps' private clipboards are two clipboards: a copy in one is not a
    /// paste in the other.
    #[test]
    fn two_apps_do_not_share_a_private_clipboard() -> Result<(), String> {
        let mut first = App::new();
        let mut second = App::new();
        use_private_clipboard(&mut first);
        use_private_clipboard(&mut second);
        copy_to_clipboard(&mut first.world_mut().resource_mut::<Clipboard>(), "one");
        copy_to_clipboard(&mut second.world_mut().resource_mut::<Clipboard>(), "two");
        assert_eq!(
            paste(&mut first.world_mut().resource_mut::<Clipboard>())?,
            "one"
        );
        assert_eq!(
            paste(&mut second.world_mut().resource_mut::<Clipboard>())?,
            "two"
        );
        Ok(())
    }

    /// The fallback leaves a host's own clipboard alone, and gives a host with
    /// none a private one.
    #[test]
    fn the_fallback_keeps_an_existing_clipboard() -> Result<(), String> {
        let mut world = World::new();
        init_private_clipboard(&mut world);
        copy_to_clipboard(&mut world.resource_mut::<Clipboard>(), "kept");
        init_private_clipboard(&mut world);
        assert_eq!(paste(&mut world.resource_mut::<Clipboard>())?, "kept");
        Ok(())
    }
}
