//! **Glyph slots**: the decorative marks a skin draws, named rather than
//! written (`viewer-skin-glyphs-from-content`).
//!
//! The skin work used to assume CSS cannot change text. It can, and browsers
//! have done so for twenty years: `::before` / `::after` with `content`.
//! `bevy_flair` supports it — [`PseudoElementsSupport`] spawns a child
//! `TextSpan` under a text entity and `content` writes it — so *which mark* a
//! widget wears is a stylesheet decision wherever the mark is decoration
//! rather than data.
//!
//! # The shape of a slot
//!
//! A glyph host is an **empty** text node carrying [`GLYPH_CLASS`] and one slot
//! class naming what the mark *means* — [`CLOSE`], [`RELOAD`], [`DISCLOSURE`].
//! It names no character; `common.css` writes one per slot:
//!
//! ```css
//! .sk-glyph-close::before { content: "\2715"; }
//! ```
//!
//! and a skin that wants a different close box writes its own. A mark that
//! follows a state is the same slot with a state class beside it — a
//! [`DISCLOSURE`] host under [`EXPANDED`] points down, a [`RELOAD`] host under
//! [`LOADING`] is a stop — so the system that used to *choose a string* now
//! only says which state is true, and a skin decides what that looks like.
//!
//! # Colour is the host's
//!
//! `.sk-glyph::before { color: inherit; }` is the one colour rule every slot
//! shares. `bevy_flair` copies a host's `TextColor` into its pseudo-element
//! once, at spawn; `inherit` is what makes the glyph follow the host as a rule
//! repaints it — a refused button's caption greys, and the glyph that *is* its
//! caption greys with it. So a host always carries a class that colours it
//! (a button's label class does), and a slot never needs a colour rule of its
//! own.
//!
//! # What is not a slot
//!
//! - **Data.** A name, a chat line, a row's value, a truncation ellipsis: a
//!   stylesheet rewriting those would be a skin rewriting the world.
//! - **A kind of thing.** An inventory item's type, a notice's attachment
//!   type, an offer's kind: those are an *icon set*, a family of pictures a
//!   skin swaps wholesale, and `viewer-skin-icon-set` owns them.
//! - The emoji picker's cells, which are the content.

use bevy::prelude::*;
use bevy_flair::style::components::{ClassList, PseudoElementsSupport};

/// The CSS class on every glyph host: an empty text node whose `::before`
/// `content` is the mark, and whose colour the mark inherits.
pub const GLYPH_CLASS: &str = "sk-glyph";

// ---------------------------------------------------------------------------
// Slots — what a mark means. One class each; `common.css` gives each its
// resting `content`.
// ---------------------------------------------------------------------------

/// Close a window or a conversation (`✕`).
pub const CLOSE: &str = "sk-glyph-close";

/// Dismiss a notice, a toast or a dialog (`×`) — lighter than [`CLOSE`],
/// because what it takes away was never a window someone opened.
pub const DISMISS: &str = "sk-glyph-dismiss";

/// A floater's minimize box; under [`MINIMIZED`] it is the restore box.
pub const MINIMIZE: &str = "sk-glyph-minimize";

/// A floater's dock box; under [`DOCKED`] it is the tear-off box.
pub const DOCK: &str = "sk-glyph-dock";

/// A floater's corner resize grip.
pub const RESIZE: &str = "sk-glyph-resize";

/// A drop-down's arrow — a combo box's, and any button that opens a list.
pub const DROP_DOWN: &str = "sk-glyph-drop-down";

/// A search field's magnifier.
pub const SEARCH: &str = "sk-glyph-search";

/// A field's clear box — empties what was typed.
pub const CLEAR: &str = "sk-glyph-clear";

/// A menu entry's check gutter: a mark only under `:checked` on its row.
pub const MENU_CHECK: &str = "sk-glyph-menu-check";

/// A menu entry that opens a sub-menu.
pub const SUBMENU: &str = "sk-glyph-submenu";

/// A sortable column header's direction: nothing unless the host carries
/// [`SORT_ASCENDING`] or [`SORT_DESCENDING`].
pub const SORT: &str = "sk-glyph-sort";

/// A tree row's disclosure triangle; under [`EXPANDED`] it points down.
pub const DISCLOSURE: &str = "sk-glyph-disclosure";

/// Someone's presence: under
/// [`PRESENCE_ONLINE_CLASS`](crate::skin::PRESENCE_ONLINE_CLASS) a filled mark,
/// otherwise an empty one. The state class is the one that already colours the
/// dot, so the mark and its colour cannot disagree.
pub const PRESENCE: &str = "sk-glyph-presence";

/// Where someone is known to be: under [`PRECISE`] a filled mark (seen in a
/// region the agent is connected to), otherwise a hollow one (only a coarse
/// map position). The radar's region column.
pub const POSITION: &str = "sk-glyph-position";

/// The one current entry of a list — the active group. A mark only under
/// [`CURRENT`].
pub const CURRENT_MARK: &str = "sk-glyph-current";

/// A yes in a yes / no column, where "no" is an empty cell.
pub const YES: &str = "sk-glyph-yes";

/// A value that differs from its default — debug settings' changed marker. A
/// mark only under [`CHANGED`].
pub const CHANGED_MARK: &str = "sk-glyph-changed";

/// A list item's bullet.
pub const BULLET: &str = "sk-glyph-bullet";

/// Step back: a browser's history, a pager's previous page. Mirrors under
/// right-to-left, where "back" points the other way.
pub const BACK: &str = "sk-glyph-back";

/// Step forward — [`BACK`]'s partner.
pub const FORWARD: &str = "sk-glyph-forward";

/// Step to the previous entry of a small cycle (a preset, a linked part) —
/// the lighter of the two pairs. Mirrors under right-to-left.
pub const PREVIOUS: &str = "sk-glyph-previous";

/// Step to the next entry of a small cycle — [`PREVIOUS`]'s partner.
pub const NEXT: &str = "sk-glyph-next";

/// Go up one level — a folder's parent.
pub const UP: &str = "sk-glyph-up";

/// Open a panel that unfolds upward from a bar button.
pub const EXPAND_UP: &str = "sk-glyph-expand-up";

/// Cycle to the next of several stacked items (a notification queue).
pub const CYCLE: &str = "sk-glyph-cycle";

/// A browser's home page.
pub const HOME: &str = "sk-glyph-home";

/// Open in the system's own browser.
pub const EXTERNAL: &str = "sk-glyph-external";

/// A page reached over a secure connection — the padlock beside an address.
pub const SECURE: &str = "sk-glyph-secure";

/// Reload; under [`LOADING`] it is the stop box.
pub const RELOAD: &str = "sk-glyph-reload";

/// Play; under [`PLAYING`] it is the pause box.
pub const PLAY_PAUSE: &str = "sk-glyph-play-pause";

/// Play a stream that cannot pause; under [`PLAYING`] it is the stop box.
pub const PLAY_STOP: &str = "sk-glyph-play-stop";

/// A music stream — the parcel audio bar's leading mark.
pub const MUSIC: &str = "sk-glyph-music";

/// Zoom in on media; under [`ZOOMED`] it zooms back out.
pub const ZOOM: &str = "sk-glyph-zoom";

/// A sound's speaker; under [`MUTED`] it is struck through.
pub const SPEAKER: &str = "sk-glyph-speaker";

/// Add someone or something to what is open.
pub const ADD: &str = "sk-glyph-add";

/// Settings for the thing beside it.
pub const SETTINGS: &str = "sk-glyph-settings";

/// Open the emoji picker.
pub const EMOJI: &str = "sk-glyph-emoji";

// ---------------------------------------------------------------------------
// States — what a stateful slot's host (or an ancestor) says is true.
// ---------------------------------------------------------------------------

/// A floater that is minimized.
pub const MINIMIZED: &str = "sk-minimized";

/// A floater that is docked.
pub const DOCKED: &str = "sk-docked";

/// A tree row that is open.
pub const EXPANDED: &str = "sk-expanded";

/// A column sorted ascending.
pub const SORT_ASCENDING: &str = "sk-sort-ascending";

/// A column sorted descending.
pub const SORT_DESCENDING: &str = "sk-sort-descending";

/// The one entry of a list that is current — the active group. Not
/// [`ACTIVE_CLASS`](crate::skin::ACTIVE_CLASS), which means *selected* and
/// paints a selection background.
pub const CURRENT: &str = "sk-current";

/// A value that differs from its default.
pub const CHANGED: &str = "sk-changed";

/// A position known precisely rather than coarsely.
pub const PRECISE: &str = "sk-precise";

/// A page that is loading.
pub const LOADING: &str = "sk-loading";

/// Media that is playing.
pub const PLAYING: &str = "sk-playing";

/// Media the camera is zoomed in on.
pub const ZOOMED: &str = "sk-zoomed";

/// A sound that is muted.
pub const MUTED: &str = "sk-muted";

/// Every slot, for the test that holds `common.css` to giving each one a mark
/// and the scratch skin to overriding each one.
pub const SLOTS: &[&str] = &[
    CLOSE,
    DISMISS,
    MINIMIZE,
    DOCK,
    RESIZE,
    DROP_DOWN,
    SEARCH,
    CLEAR,
    MENU_CHECK,
    SUBMENU,
    SORT,
    DISCLOSURE,
    PRESENCE,
    CURRENT_MARK,
    POSITION,
    YES,
    CHANGED_MARK,
    BULLET,
    BACK,
    FORWARD,
    PREVIOUS,
    NEXT,
    UP,
    EXPAND_UP,
    CYCLE,
    HOME,
    EXTERNAL,
    SECURE,
    RELOAD,
    PLAY_PAUSE,
    PLAY_STOP,
    MUSIC,
    ZOOM,
    SPEAKER,
    ADD,
    SETTINGS,
    EMOJI,
];

/// A glyph host for `slot` in `font`, as a spawn bundle: an empty text node
/// the skin's `content` fills. The pseudo-element inherits the font, so it is
/// the size a skin's mark is drawn at unless a rule sets `font-size`.
///
/// `extra` is the classes worn beside [`GLYPH_CLASS`] and the slot — the one
/// that colours the host (see the module docs) and any state the host starts
/// in. The host is never a pick target: the button or row around it is.
#[must_use]
pub fn glyph_host(
    slot: &'static str,
    font: TextFont,
    extra: impl IntoIterator<Item = &'static str>,
) -> impl Bundle {
    (
        Text::default(),
        PseudoElementsSupport,
        font,
        ClassList::new_with_classes([GLYPH_CLASS, slot].into_iter().chain(extra)),
        Pickable::IGNORE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Every slot is a distinct class under the one prefix, so no two slots can
    /// collide and a skin author finding one in `common.css` can grep for all.
    #[test]
    fn every_slot_is_a_distinct_sk_glyph_class() {
        let mut seen = std::collections::BTreeSet::new();
        for slot in SLOTS {
            assert!(
                slot.starts_with("sk-glyph-"),
                "{slot} is not under the sk-glyph- prefix"
            );
            assert!(seen.insert(*slot), "{slot} is listed twice");
        }
        assert_eq!(seen.len(), SLOTS.len());
    }
}
