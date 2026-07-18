//! A **virtualized (windowed-recycling) list** (`viewer-ui-virtualized-list`).
//!
//! Bevy's `ListBox` — and any plain `column()` under an `Overflow::scroll()`
//! viewport (the gallery's approach) — spawns **one entity per row**, so a
//! 10 000-item inventory ([`crate::inventory`]) would mean 10 000 taffy nodes
//! laid out every frame. This widget instead keeps a **small pool** of row
//! entities — only enough to cover the viewport plus a little overscan — and
//! **recycles** them as the viewport scrolls: a row that scrolls off the top is
//! re-bound to the item now scrolling in at the bottom. The cost is set by the
//! viewport height, not the item count, so a list of any length scrolls cheaply.
//!
//! # The split: generic recycling, app-supplied row content
//!
//! This module owns only the part that is the same for every list — the
//! **windowing arithmetic** ([`row_window`]) and the **pool machinery** that
//! keeps the right rows alive and positioned ([`layout_virtual_lists`]). It knows
//! nothing about what a row *contains*. A consumer:
//!
//! 1. spawns a **viewport** node carrying [`VirtualList`] (its
//!    [`row_height`](VirtualList::row_height) and item count), clipped and
//!    focusable, and
//! 2. reacts to [`VirtualRow`] changing — `Added` to build a row's persistent
//!    inner nodes once, `Changed` to (re)bind them to
//!    [`index`](VirtualRow::index) — writing its own item's icon / label / indent
//!    into the pooled entity.
//!
//! That keeps the recycling logic testable in isolation (the pure
//! [`row_window`] has no Bevy in it at all) and lets one mechanism back every
//! long-list panel — inventory, radar, the people list, chat history at scale.
//!
//! # Scrolling and the camera
//!
//! The wheel both zooms the world camera and scrolls a hovered list, so the two
//! must not fire at once. They are kept apart by the input context
//! ([`crate::input_context`]): the camera zoom runs only in
//! [`InputContext::World`], and [`scroll_virtual_lists`] runs only when the world
//! does *not* own input — i.e. after the list (a focusable widget) has been
//! clicked into. Clicking the list focuses it (see the `Press` observer
//! installed by the consumer), which is what flips the context; the wheel then
//! scrolls the list under the pointer and the camera holds still.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

use crate::input_context::InputContext;

/// How many extra rows to keep live just past each edge of the viewport, so a
/// fast scroll does not flash blank rows before the pool catches up. Small on
/// purpose — the whole point is a bounded pool.
const OVERSCAN_ROWS: usize = 3;

/// Logical pixels scrolled per wheel notch reported in [`MouseScrollUnit::Line`]
/// units — a few rows, so one notch is a comfortable step rather than a jump.
const LINE_SCROLL_PIXELS: f32 = 48.0;

/// The plugin that drives every [`VirtualList`]: it recycles each list's row
/// pool and routes the wheel to a hovered, focused list.
pub(crate) struct VirtualListPlugin;

impl Plugin for VirtualListPlugin {
    /// Register the scroll and layout systems. Layout runs after scroll so a
    /// wheel step is reflected the same frame, and both run in `Update` — the
    /// row positions they write are plain [`Node`] fields that the `PostUpdate`
    /// layout pass then resolves.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (scroll_virtual_lists, layout_virtual_lists).chain());
    }
}

/// A virtualized list, placed on the **viewport** node (the clipped container the
/// pooled rows live inside).
///
/// The consumer sets [`row_height`](Self::row_height) and keeps
/// [`item_count`](Self::item_count) current; the scroll offset is owned here and
/// nudged by [`scroll_virtual_lists`] / clamped by [`layout_virtual_lists`].
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct VirtualList {
    /// The uniform height of every row, in logical pixels. Uniform because the
    /// windowing arithmetic maps a scroll offset to a row index by division —
    /// variable heights would need a prefix-sum the inventory does not require.
    pub(crate) row_height: f32,
    /// How many items the list is currently presenting. The consumer updates
    /// this whenever its model changes; the pool follows.
    pub(crate) item_count: usize,
    /// The current scroll offset from the top, in logical pixels. Private so it
    /// is only ever changed through the systems that clamp it.
    scroll: f32,
}

impl VirtualList {
    /// A new list with the given uniform row height, empty and scrolled to the
    /// top.
    pub(crate) const fn new(row_height: f32) -> Self {
        Self {
            row_height,
            item_count: 0,
            scroll: 0.0,
        }
    }

    /// Reset the scroll offset to the top — used when the presented content
    /// changes wholesale (a tab switch, a new search) so the old offset does not
    /// leave the new, shorter list scrolled past its end.
    pub(crate) const fn scroll_to_top(&mut self) {
        self.scroll = 0.0;
    }
}

/// A pooled row entity: a child of a [`VirtualList`] viewport that is repeatedly
/// re-bound to whichever item is currently at its screen position.
///
/// [`slot`](Self::slot) is the row's fixed place in the pool (`0..pool_len`);
/// [`index`](Self::index) is the model item it currently shows, or `None` when
/// the pool has more rows than the window needs and this one is parked.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VirtualRow {
    /// This row's fixed index within its list's pool.
    pub(crate) slot: usize,
    /// The model item index this row currently presents, or `None` when parked
    /// (hidden). The consumer reads this to know what to draw.
    pub(crate) index: Option<usize>,
}

/// Marks the entity a pooled row's `ChildOf` points at as a virtual-list
/// viewport, so the pool-building system can find the list an
/// [`Added`](bevy::prelude::Added) row belongs to. Inserted automatically
/// alongside [`VirtualList`] would be ideal, but the consumer spawns the
/// viewport, so the layout system tolerates its absence and treats any
/// [`VirtualList`] entity as the pool parent.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct VirtualViewport;

/// The contiguous window of item indices that must have a live row entity: the
/// rows on screen, plus [`OVERSCAN_ROWS`] beyond each edge, clamped to the item
/// count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowWindow {
    /// The first item index in the window.
    pub(crate) first: usize,
    /// How many items the window spans.
    pub(crate) count: usize,
}

/// The total height of all rows, in logical pixels — the scrollable extent.
fn content_height(item_count: usize, row_height: f32) -> f32 {
    index_to_f32(item_count) * row_height.max(0.0)
}

/// The largest legal scroll offset: how far past the top the content can go
/// before its bottom reaches the viewport's bottom. Never negative — a list
/// shorter than its viewport does not scroll.
fn max_scroll(item_count: usize, row_height: f32, viewport_height: f32) -> f32 {
    (content_height(item_count, row_height) - viewport_height.max(0.0)).max(0.0)
}

/// The top of row `index`, in logical pixels from the top of the content.
fn row_top(index: usize, row_height: f32) -> f32 {
    index_to_f32(index) * row_height
}

/// Which rows must be live for a given scroll offset and viewport height.
///
/// The pure heart of the widget: a scroll offset and a viewport height in, the
/// window of item indices that need entities out. Everything Bevy-side is
/// bookkeeping around this one function, which is why it is where the tests are.
fn row_window(
    scroll: f32,
    viewport_height: f32,
    row_height: f32,
    item_count: usize,
    overscan: usize,
) -> RowWindow {
    if row_height <= 0.0 || item_count == 0 || viewport_height <= 0.0 {
        return RowWindow { first: 0, count: 0 };
    }
    let first_visible = floor_to_usize(scroll / row_height);
    // The first row wholly past the bottom edge: ceil of the bottom offset over
    // the row height. `ceil` so a row peeking in at the bottom still counts.
    let bottom = (scroll + viewport_height) / row_height;
    let last_visible = floor_to_usize(bottom.ceil());
    let first = first_visible.saturating_sub(overscan);
    let last = last_visible.saturating_add(overscan).min(item_count);
    RowWindow {
        first,
        count: last.saturating_sub(first),
    }
}

/// Route the wheel to the virtual list under the pointer, but only while a UI
/// widget owns input (the list has been clicked into), so the world camera —
/// which zooms only in [`InputContext::World`] — never zooms at the same time.
///
/// See the [module docs](self) for why the context gate is the whole of the
/// coordination.
pub(crate) fn scroll_virtual_lists(
    context: Res<InputContext>,
    wheel: Res<AccumulatedMouseScroll>,
    hover_map: Res<HoverMap>,
    child_of: Query<&ChildOf>,
    mut lists: Query<&mut VirtualList>,
) {
    if context.is_world() || wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    let delta = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * LINE_SCROLL_PIXELS,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    // Scroll the first hovered entity that is (or is inside) a virtual list; the
    // mouse pointer's hover set is what `Pointer<Scroll>` itself would bubble
    // through, so this matches "the list the wheel is over".
    for hovered in hover_map.values().flat_map(|hits| hits.keys()) {
        let mut node = *hovered;
        loop {
            if let Ok(mut list) = lists.get_mut(node) {
                // Wheel up (positive) scrolls content up, i.e. toward the top.
                list.scroll = (list.scroll - delta).max(0.0);
                return;
            }
            match child_of.get(node) {
                Ok(parent) => node = parent.parent(),
                Err(_) => break,
            }
        }
    }
}

/// Recycle each list's row pool: clamp the scroll, compute the window, grow the
/// pool if the viewport needs more rows than exist, and (re)bind and position
/// every pooled row. Runs every frame but writes a row's [`VirtualRow`] or
/// [`Node`] only when a value actually changes, so a still list costs a compare
/// and nothing more (and does not spuriously wake consumers' `Changed` binds).
pub(crate) fn layout_virtual_lists(
    mut commands: Commands,
    mut lists: Query<(Entity, &mut VirtualList, &ComputedNode)>,
    children: Query<&Children>,
    mut rows: Query<(&mut VirtualRow, &mut Node)>,
) {
    for (list_entity, mut list, computed) in &mut lists {
        let viewport_height = computed.size().y * computed.inverse_scale_factor();
        if viewport_height <= 0.0 {
            continue;
        }
        let clamped = list.scroll.clamp(
            0.0,
            max_scroll(list.item_count, list.row_height, viewport_height),
        );
        if (clamped - list.scroll).abs() > f32::EPSILON {
            list.scroll = clamped;
        }
        let window = row_window(
            list.scroll,
            viewport_height,
            list.row_height,
            list.item_count,
            OVERSCAN_ROWS,
        );

        // Collect the current pool, in slot order, so growth appends the next
        // slot rather than reusing one.
        let mut pool: Vec<(Entity, usize)> = children
            .get(list_entity)
            .into_iter()
            .flat_map(|kids| kids.iter())
            .filter_map(|kid| rows.get(kid).ok().map(|(row, _)| (kid, row.slot)))
            .collect();
        pool.sort_unstable_by_key(|&(_, slot)| slot);

        // Grow the pool until it can cover the window.
        for slot in pool.len()..window.count {
            let row = commands
                .spawn((
                    VirtualRow { slot, index: None },
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(0.0),
                        height: Val::Px(list.row_height),
                        display: Display::None,
                        ..default()
                    },
                    ChildOf(list_entity),
                ))
                .id();
            pool.push((row, slot));
        }

        // Bind each pooled row to its window item (or park it) and position it.
        for &(entity, slot) in &pool {
            let index = if slot < window.count {
                Some(window.first.saturating_add(slot))
            } else {
                None
            };
            let Ok((mut row, mut node)) = rows.get_mut(entity) else {
                continue;
            };
            if row.index != index {
                row.index = index;
            }
            let display = if index.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
            if let Some(index) = index {
                let top = Val::Px(row_top(index, list.row_height) - list.scroll);
                if node.top != top {
                    node.top = top;
                }
                let height = Val::Px(list.row_height);
                if node.height != height {
                    node.height = height;
                }
            }
        }
    }
}

/// Widen a row index or count to `f32` without an `as` cast (the workspace
/// forbids them), by splitting the low 32 bits into two `u16` halves — the same
/// trick [`crate::coords::metres_to_f32`] uses. Counts far beyond `u32` are not
/// reachable by any real inventory, and saturate rather than wrap.
fn index_to_f32(n: usize) -> f32 {
    let clamped = u32::try_from(n).unwrap_or(u32::MAX);
    let high = u16::try_from(clamped >> 16).unwrap_or(u16::MAX);
    let low = u16::try_from(clamped & 0xFFFF).unwrap_or(u16::MAX);
    f32::from(high) * 65_536.0 + f32::from(low)
}

/// Floor a non-negative `f32` to a `usize`, saturating a non-finite or huge
/// value to `0` / a large bound respectively. The one float-to-int conversion in
/// the module, kept behind a guard so the cast is always in range.
fn floor_to_usize(value: f32) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    // Clamp below `u32::MAX` (the largest count `index_to_f32` represents) so the
    // truncation is exact and never wraps.
    let floored = value.floor().min(4_294_967_040.0);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "value is guarded finite and in 0.0..=4_294_967_040.0, so it fits usize exactly"
    )]
    let n = floored as usize;
    n
}

#[cfg(test)]
mod tests {
    use super::{
        OVERSCAN_ROWS, RowWindow, content_height, floor_to_usize, index_to_f32, max_scroll,
        row_top, row_window,
    };
    use pretty_assertions::assert_eq;

    /// A window with no overscan, for reasoning about the visible span alone.
    fn window_no_overscan(scroll: f32, viewport: f32, row_height: f32, count: usize) -> RowWindow {
        row_window(scroll, viewport, row_height, count, 0)
    }

    /// At the top, the window starts at row 0 and spans the visible rows.
    #[test]
    fn window_at_top_starts_at_zero() {
        // A 100 px viewport over 20 px rows shows five whole rows.
        let window = window_no_overscan(0.0, 100.0, 20.0, 1000);
        assert_eq!(window.first, 0);
        assert_eq!(window.count, 5);
    }

    /// A partial scroll pulls in the row peeking at the bottom.
    #[test]
    fn partial_scroll_includes_the_peeking_row() {
        // Scrolled 10 px: rows 0..=5 are all at least partly visible (row 0's
        // bottom 10 px, then 1..4 whole, then row 5 peeking) — six rows.
        let window = window_no_overscan(10.0, 100.0, 20.0, 1000);
        assert_eq!(window.first, 0);
        assert_eq!(window.count, 6);
    }

    /// Scrolling by whole rows advances the window's first index.
    #[test]
    fn whole_row_scroll_advances_first() {
        let window = window_no_overscan(40.0, 100.0, 20.0, 1000);
        assert_eq!(window.first, 2);
        assert_eq!(window.count, 5);
    }

    /// Overscan widens the window on both sides but never past the ends.
    #[test]
    fn overscan_widens_but_clamps_to_ends() {
        // Mid-list: overscan on both sides.
        let middle = row_window(400.0, 100.0, 20.0, 1000, OVERSCAN_ROWS);
        assert_eq!(middle.first, 20usize.saturating_sub(OVERSCAN_ROWS));
        // At the very top there is nothing before row 0 to overscan into.
        let top = row_window(0.0, 100.0, 20.0, 1000, OVERSCAN_ROWS);
        assert_eq!(top.first, 0);
    }

    /// The window never runs past the item count.
    #[test]
    fn window_clamps_to_item_count() {
        // Only three items, a viewport that could show five.
        let window = row_window(0.0, 100.0, 20.0, 3, OVERSCAN_ROWS);
        assert_eq!(window.first, 0);
        assert_eq!(window.count, 3);
    }

    /// Degenerate inputs yield an empty window rather than a panic or a
    /// nonsense span.
    #[test]
    fn degenerate_inputs_are_empty() {
        assert_eq!(window_no_overscan(0.0, 100.0, 20.0, 0).count, 0);
        assert_eq!(window_no_overscan(0.0, 100.0, 0.0, 10).count, 0);
        assert_eq!(window_no_overscan(0.0, 0.0, 20.0, 10).count, 0);
    }

    /// Content height and max scroll agree: a list exactly as tall as its
    /// viewport does not scroll; a taller one scrolls by the difference.
    #[expect(
        clippy::float_cmp,
        reason = "the windowing arithmetic produces exact, representable results, asserted exactly"
    )]
    #[test]
    fn max_scroll_is_content_minus_viewport() {
        assert_eq!(content_height(10, 20.0), 200.0);
        assert_eq!(max_scroll(10, 20.0, 200.0), 0.0);
        assert_eq!(max_scroll(10, 20.0, 100.0), 100.0);
        // A short list never scrolls.
        assert_eq!(max_scroll(2, 20.0, 100.0), 0.0);
    }

    /// A row's top is its index times the row height.
    #[expect(
        clippy::float_cmp,
        reason = "row_top produces exact multiples of the row height, asserted exactly"
    )]
    #[test]
    fn row_top_is_index_times_height() {
        assert_eq!(row_top(0, 20.0), 0.0);
        assert_eq!(row_top(7, 20.0), 140.0);
    }

    /// The integer/float helpers behave at the boundaries the windowing relies
    /// on.
    #[expect(
        clippy::float_cmp,
        reason = "the small integers widen to exact f32 values, asserted exactly"
    )]
    #[test]
    fn conversion_helpers_are_well_behaved() {
        assert_eq!(index_to_f32(0), 0.0);
        assert_eq!(index_to_f32(70_000), 70_000.0);
        assert_eq!(floor_to_usize(-1.0), 0);
        assert_eq!(floor_to_usize(f32::NAN), 0);
        assert_eq!(floor_to_usize(3.9), 3);
    }
}
