//! The trailing truncation ellipsis: one marker, and one rule for revealing it.
//!
//! Three surfaces clip a value to a box and mark the cut with a trailing `…` —
//! the tab strip's labels, the table's cells and the inventory's rows. Each used
//! to carry its own copy of the same component-and-system pair, and the copies
//! had already drifted (the marker's leading gap was 1 px in one and 2 px in the
//! other two, and one of the three spelled its overflow test as a free
//! function). This module is the single copy: [`spawn_ellipsis_marker`] makes
//! the marker, [`RevealEllipsis`] names it from the node whose overflow decides,
//! and [`apply_reveal_ellipsis`] — registered once, by
//! [`crate::ui::ViewerUiPlugin`] — shows and hides it.
//!
//! # Why the marker's own width is added back
//!
//! The marker is a **sibling** of the clip container rather than a child of it,
//! and it does not shrink, so showing it takes its width away from the clip.
//! Measuring the value's natural width against the *shrunk* clip therefore asks
//! a different question depending on the answer it gave last frame, and the
//! state latches: any value whose natural width falls between
//! `available - marker` and `available` reads as truncated forever once anything
//! first showed the marker — a column narrowed and widened again, or a
//! virtualized row rebinding onto a longer value — and is then drawn clipped
//! although it would have fitted whole (`viewer-audit-ellipsis-reveal-latch`).
//!
//! [`ellipsis_wanted`] measures against the width the value would have **with
//! the marker hidden**: the clip's laid-out width plus whatever the marker
//! occupies right now. That is the same number in both states, so the predicate
//! has no memory of its own answer.
//!
//! It is also why the marker's leading gap is `padding` and not the `margin` the
//! three copies used: padding is inside the border box that layout reports as
//! the node's `size`, so "whatever the marker occupies" is one physical number
//! to read rather than a physical size plus a logical margin to scale and add.
//! As a [`LogicalPadding`] it mirrors under a right-to-left locale as well,
//! which the physical `margin: left` never did.

use bevy::prelude::*;

use crate::i18n::LocaleEllipsisMarker;
use crate::ui::{LogicalPadding, LogicalRect};
use crate::ui_font::UiFont;

/// The gap between the last visible glyph and the `…`, in logical pixels, on the
/// marker's leading side.
const ELLIPSIS_GAP: f32 = 2.0;

/// How far a value may overflow its box before the overflow is worth an
/// ellipsis, in physical pixels.
///
/// Text measurement and flexbox rounding disagree in the last fraction of a
/// pixel, and half a pixel of clipped tail is not visible; without a tolerance a
/// settled label can toggle the marker forever on that disagreement, since
/// toggling it is itself a layout write.
const OVERFLOW_TOLERANCE: f32 = 0.5;

/// On the node whose overflow decides — a clip container, or a label that clips
/// itself — naming the `…` marker [`apply_reveal_ellipsis`] reveals while the
/// value inside it does not fit.
///
/// The marker is **not** this node's child (it would be clipped away with the
/// text it marks); it is a sibling, spawned by [`spawn_ellipsis_marker`] into the
/// same row as the clip.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevealEllipsis {
    /// The `…` node to show while this node's content overflows it.
    pub marker: Entity,
}

/// Whether the trailing `…` belongs on a value `natural` physical pixels wide,
/// laid out in a box `laid_out` physical pixels wide, with `marker` physical
/// pixels currently taken by the marker beside it — zero while it is hidden.
///
/// `laid_out + marker` is the space the value would have *without* the marker,
/// which is the question that has an answer independent of the last one; see the
/// [module documentation](self) for the latch that measuring against `laid_out`
/// alone produces.
#[must_use]
pub fn ellipsis_wanted(natural: f32, laid_out: f32, marker: f32) -> bool {
    natural > laid_out + marker + OVERFLOW_TOLERANCE
}

/// Spawn a hidden trailing `…` marker as a child of `parent`, to be named by a
/// [`RevealEllipsis`] on the clip container beside it.
///
/// `text` is the glyph to start with; the marker carries
/// [`LocaleEllipsisMarker`], so `i18n` replaces it with the active locale's
/// ellipsis (a CJK locale's centred `……`, say) as soon as that is known. It is
/// [`Pickable::IGNORE`] because it is decoration: a click that lands on the `…`
/// means the row or tab it trails, not the marker.
pub fn spawn_ellipsis_marker(
    commands: &mut Commands,
    parent: Entity,
    font_size: f32,
    color: Color,
    text: &str,
) -> Entity {
    commands
        .spawn((
            Text::new(text.to_owned()),
            TextLayout::no_wrap(),
            UiFont::Sans.at(font_size),
            TextColor(color),
            Node {
                // Hidden until the value overflows; never shrinks, so it keeps
                // its room once shown.
                display: Display::None,
                flex_shrink: 0.0,
                ..default()
            },
            // The gap off the last visible glyph, inside the marker's own border
            // box so `apply_reveal_ellipsis` can read it as part of the width the
            // marker occupies.
            LogicalPadding(LogicalRect {
                inline_start: Val::Px(ELLIPSIS_GAP),
                ..LogicalRect::ZERO
            }),
            LocaleEllipsisMarker,
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// Reveal each [`RevealEllipsis`] node's marker exactly while the value inside
/// it overflows, and hide it while the value fits.
///
/// Registered by [`crate::ui::ViewerUiPlugin`] after `UiSystems::Layout`, so it
/// reads the freshly measured boxes and writes next frame's `Display` — the same
/// after-layout shape as the widgets' own fit systems. The write is guarded, so a
/// settled surface costs a compare per marker and nothing more.
///
/// A clipped value's leading-edge alignment (so the *start* of the name shows and
/// the *end* clips) and the marker's trailing side (right under LTR, left under
/// RTL) both come from the container's flow, which `apply_ui_direction` mirrors,
/// so this system is direction-agnostic.
pub fn apply_reveal_ellipsis(
    clipped: Query<(&ComputedNode, &RevealEllipsis)>,
    mut markers: Query<(&ComputedNode, &mut Node), With<LocaleEllipsisMarker>>,
) {
    for (computed, reveal) in &clipped {
        let Ok((marker_computed, mut marker_node)) = markers.get_mut(reveal.marker) else {
            continue;
        };
        // A hidden node has no layout, so its `size` is zero either way; read the
        // `Display` we asked for rather than trusting that.
        let occupied = if marker_node.display == Display::None {
            0.0
        } else {
            marker_computed.size.x
        };
        let wanted = if ellipsis_wanted(computed.content_size.x, computed.size.x, occupied) {
            Display::Flex
        } else {
            Display::None
        };
        if marker_node.display != wanted {
            marker_node.display = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OVERFLOW_TOLERANCE, ellipsis_wanted};

    /// The band the old per-widget test latched in: a value that fits the box
    /// *without* the marker, but not with it. The answer must be the same
    /// whichever state the marker is in — that is the whole of the fix.
    #[test]
    fn a_value_in_the_marker_wide_band_fits_in_both_states() {
        // 100 px of value in a 104 px cell, beside a 6 px marker.
        let (natural, cell, marker) = (100.0, 104.0, 6.0);
        assert!(
            !ellipsis_wanted(natural, cell, 0.0),
            "hidden marker: 100 px of value fits a 104 px cell"
        );
        assert!(
            !ellipsis_wanted(natural, cell - marker, marker),
            "shown marker: the same value still fits the same cell — the old \
             measure against the shrunk 98 px clip said it did not, and never \
             cleared"
        );
    }

    /// A value that genuinely does not fit also answers the same in both states,
    /// so the marker stays put rather than blinking once shown.
    #[test]
    fn an_overlong_value_overflows_in_both_states() {
        let (natural, cell, marker) = (140.0, 104.0, 6.0);
        assert!(
            ellipsis_wanted(natural, cell, 0.0),
            "hidden marker: 140 px of value overflows a 104 px cell"
        );
        assert!(
            ellipsis_wanted(natural, cell - marker, marker),
            "shown marker: it still overflows, so the marker stays"
        );
    }

    /// The boundary: exactly filling the box is not an overflow, and neither is
    /// a sub-pixel tail — but a pixel over is.
    #[test]
    fn the_boundary_is_the_box_plus_a_half_pixel() {
        assert!(
            !ellipsis_wanted(104.0, 104.0, 0.0),
            "a value that exactly fills its box is not truncated"
        );
        assert!(
            !ellipsis_wanted(104.0 + OVERFLOW_TOLERANCE, 104.0, 0.0),
            "half a pixel of tail is rounding, not truncation"
        );
        assert!(
            ellipsis_wanted(105.0, 104.0, 0.0),
            "a whole pixel over is truncation"
        );
    }
}
