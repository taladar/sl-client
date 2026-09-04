//! A **virtualized (windowed-recycling) list** (`viewer-ui-virtualized-list`).
//!
//! Bevy's `ListBox` — and any plain `column()` under an `Overflow::scroll()`
//! viewport (the gallery's approach) — spawns **one entity per row**, so a
//! 10 000-item inventory (`inventory`) would mean 10 000 taffy nodes
//! laid out every frame. This widget instead keeps a **small pool** of row
//! entities — only enough to cover the viewport plus a little overscan — and
//! **recycles** them as the viewport scrolls: a row that scrolls off the top is
//! re-bound to the item now scrolling in at the bottom — and *only* that row,
//! because the slot↔item mapping is modular rather than by offset (see
//! `slot_index`). The cost is set by the viewport height, not the item count,
//! so a list of any length scrolls cheaply, and one row of scroll costs one
//! consumer rebind rather than a pool's worth.
//!
//! # The split: generic recycling, app-supplied row content
//!
//! This module owns only the part that is the same for every list — the
//! **windowing arithmetic** (`row_window`) and the **pool machinery** that
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
//! `row_window` has no Bevy in it at all) and lets one mechanism back every
//! long-list panel — inventory, radar, the people list, chat history at scale.
//!
//! # Scrolling and the camera
//!
//! The wheel both zooms the world camera and scrolls a hovered list, so the two
//! must not fire at once. They are kept apart by **hover**: a virtual-list
//! viewport is a blocking [`Pickable`] node, so whenever the pointer is over
//! one the camera's wheel zoom stands down (`pointer_over_blocking_ui` in
//! `camera`) and [`scroll_virtual_lists`] scrolls the hovered list —
//! no click-into-the-list first (the old input-context gate left the wheel
//! doing *nothing* over a not-yet-focused list, since the camera already
//! ignored it there too). Away from any list the hover walk finds nothing and
//! the wheel stays the camera's.
//!
//! That routing — a real `AccumulatedMouseScroll` over a real `HoverMap`, and
//! the scrollbar thumb's drag — is covered by
//! `sl_viewer_ui_widgets::ui_table::tests::scenarios`, through the table, which
//! is this widget's main consumer. It is tested *there* rather than here
//! because the pointer harness (`sl-viewer-testkit`) is built on this crate: a
//! dev-dependency back onto it would link **two** copies of this crate into the
//! test binary — the `cfg(test)` one and the harness's — whose `UiRoot` and
//! every other resource are different types, so a fixture would fail parameter
//! validation on a resource it can see sitting in the world. The tests below
//! stay pure: the windowing arithmetic and the pool, with the viewport's
//! `ComputedNode` written by hand.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

use crate::ui::{LogicalInset, LogicalRect};

/// How many extra rows to keep live just past each edge of the viewport, so a
/// fast scroll does not flash blank rows before the pool catches up. Small on
/// purpose — the whole point is a bounded pool.
const OVERSCAN_ROWS: usize = 3;

/// Logical pixels scrolled per wheel notch reported in [`MouseScrollUnit::Line`]
/// units — a few rows, so one notch is a comfortable step rather than a jump.
const LINE_SCROLL_PIXELS: f32 = 48.0;

/// The plugin that drives every [`VirtualList`]: it recycles each list's row
/// pool and routes the wheel to a hovered, focused list.
#[derive(Debug)]
pub struct VirtualListPlugin;

impl Plugin for VirtualListPlugin {
    /// Register the scroll and layout systems. Layout runs after scroll so a
    /// wheel step is reflected the same frame, and both run in `Update` — the
    /// row positions they write are plain [`Node`] fields that the `PostUpdate`
    /// layout pass then resolves.
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                scroll_virtual_lists,
                layout_virtual_lists,
                drive_virtual_scrollbars,
            )
                .chain(),
        );
    }
}

// ---------------------------------------------------------------------------
// The overlay scrollbar.
// ---------------------------------------------------------------------------

/// The scrollbar track's thickness, in logical pixels (the tab strip's value).
const SCROLLBAR_THICKNESS: f32 = 10.0;

/// The thumb's shortest length, in logical pixels, so it stays grabbable on a
/// very long list.
const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// The scrollbar track's colour (the tab-strip scrollbar palette).
const SCROLLBAR_TRACK_COLOR: Color = Color::srgb(0.12, 0.14, 0.18);

/// The scrollbar thumb's colour.
const SCROLLBAR_THUMB_COLOR: Color = Color::srgb(0.40, 0.48, 0.60);

/// A [`VirtualList`] viewport's overlay scrollbar track, naming its viewport.
/// Bevy's `Scrollbar` widget drives the native `ScrollPosition`, which a
/// virtual list does not use (it owns its own clamped offset), so the bar is
/// driven from [`VirtualList`] directly by `drive_virtual_scrollbars`.
#[derive(Component, Debug, Clone, Copy)]
struct VirtualScrollbar {
    /// The [`VirtualList`] viewport this bar reflects and drives.
    viewport: Entity,
}

/// The draggable thumb inside a [`VirtualScrollbar`] track.
#[derive(Component, Debug, Clone, Copy)]
struct VirtualScrollbarThumb;

/// Spawn the overlay scrollbar for a [`VirtualList`] `viewport`: a slim track
/// pinned to the viewport's trailing inline edge (an *overlay*, so it never
/// shifts the header / row layout), holding a thumb whose size and position
/// `drive_virtual_scrollbars` keeps proportional to the scroll state.
/// Hidden while the content fits. Dragging the thumb scrolls the list; the
/// wheel path is untouched (hover on the bar still bubbles to the viewport).
pub fn spawn_virtual_scrollbar(commands: &mut Commands, viewport: Entity) -> Entity {
    let track = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(SCROLLBAR_THICKNESS),
                ..default()
            },
            LogicalInset(LogicalRect {
                inline_end: Val::Px(0.0),
                block_start: Val::Px(0.0),
                block_end: Val::Px(0.0),
                ..LogicalRect::AUTO
            }),
            BackgroundColor(SCROLLBAR_TRACK_COLOR),
            // Above the pooled rows, which are appended later in paint order.
            ZIndex(1),
            Visibility::Hidden,
            VirtualScrollbar { viewport },
            Name::new("virtual-list:scrollbar"),
            ChildOf(viewport),
        ))
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Px(SCROLLBAR_MIN_THUMB),
                top: Val::Px(0.0),
                ..default()
            },
            BackgroundColor(SCROLLBAR_THUMB_COLOR),
            Pickable::default(),
            VirtualScrollbarThumb,
            Name::new("virtual-list:scrollbar-thumb"),
            ChildOf(track),
        ))
        .observe(
            move |mut drag: On<Pointer<Drag>>,
                  mut lists: Query<(&mut VirtualList, &ComputedNode)>| {
                drag.propagate(false);
                if drag.button != PointerButton::Primary {
                    return;
                }
                let Ok((mut list, computed)) = lists.get_mut(viewport) else {
                    return;
                };
                let viewport_height = computed.size().y * computed.inverse_scale_factor();
                let Some(geometry) =
                    scrollbar_geometry(list.item_count, list.row_height, viewport_height)
                else {
                    return;
                };
                // A pointer step maps through the thumb's travel range to the
                // scroll range, so the thumb tracks the pointer exactly.
                let travel = (viewport_height - geometry.thumb_height).max(f32::EPSILON);
                list.scroll_by(drag.delta.y * geometry.max_scroll / travel);
            },
        );
    track
}

/// A scrollbar's derived geometry for one frame: how long the thumb is and how
/// far the list can scroll. `None` while the content fits the viewport (the
/// bar hides).
struct ScrollbarGeometry {
    /// The thumb's length, in logical pixels.
    thumb_height: f32,
    /// The largest legal scroll offset (see [`max_scroll`]).
    max_scroll: f32,
}

/// The [`ScrollbarGeometry`] for a list of `item_count` rows of `row_height`
/// in a `viewport_height` window — the track is the viewport-height overlay.
fn scrollbar_geometry(
    item_count: usize,
    row_height: f32,
    viewport_height: f32,
) -> Option<ScrollbarGeometry> {
    let content = content_height(item_count, row_height);
    if viewport_height <= 0.0 || content <= viewport_height {
        return None;
    }
    let thumb_height = (viewport_height * viewport_height / content)
        .max(SCROLLBAR_MIN_THUMB)
        .min(viewport_height);
    Some(ScrollbarGeometry {
        thumb_height,
        max_scroll: max_scroll(item_count, row_height, viewport_height),
    })
}

/// Keep every [`VirtualScrollbar`] agreeing with its list: hidden while the
/// content fits, otherwise a thumb proportional to the visible fraction,
/// positioned at the scroll fraction. Runs after [`layout_virtual_lists`] so
/// it reads the frame's clamped offset.
fn drive_virtual_scrollbars(
    lists: Query<(&VirtualList, &ComputedNode)>,
    mut tracks: Query<(&VirtualScrollbar, &mut Visibility, &Children)>,
    mut thumbs: Query<&mut Node, With<VirtualScrollbarThumb>>,
) {
    for (bar, mut visibility, children) in &mut tracks {
        let Ok((list, computed)) = lists.get(bar.viewport) else {
            continue;
        };
        let viewport_height = computed.size().y * computed.inverse_scale_factor();
        let geometry = scrollbar_geometry(list.item_count, list.row_height, viewport_height);
        let want = if geometry.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
        let Some(geometry) = geometry else {
            continue;
        };
        let fraction = if geometry.max_scroll > 0.0 {
            (list.scroll / geometry.max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let top = fraction * (viewport_height - geometry.thumb_height).max(0.0);
        for child in children {
            let Ok(mut node) = thumbs.get_mut(*child) else {
                continue;
            };
            let want_height = Val::Px(geometry.thumb_height);
            if node.height != want_height {
                node.height = want_height;
            }
            let want_top = Val::Px(top);
            if node.top != want_top {
                node.top = want_top;
            }
        }
    }
}

/// A virtualized list, placed on the **viewport** node (the clipped container the
/// pooled rows live inside).
///
/// The consumer sets [`row_height`](Self::row_height) and keeps
/// [`item_count`](Self::item_count) current; the scroll offset is owned here and
/// nudged by [`scroll_virtual_lists`] / clamped by [`layout_virtual_lists`].
#[derive(Component, Debug, Clone, Copy)]
pub struct VirtualList {
    /// The uniform height of every row, in logical pixels. Uniform because the
    /// windowing arithmetic maps a scroll offset to a row index by division —
    /// variable heights would need a prefix-sum the inventory does not require.
    pub row_height: f32,
    /// How many items the list is currently presenting. The consumer updates
    /// this whenever its model changes; the pool follows.
    pub item_count: usize,
    /// The current scroll offset from the top, in logical pixels. Private so it
    /// is only ever changed through the systems that clamp it.
    scroll: f32,
}

impl VirtualList {
    /// A new list with the given uniform row height, empty and scrolled to the
    /// top.
    #[must_use]
    pub const fn new(row_height: f32) -> Self {
        Self {
            row_height,
            item_count: 0,
            scroll: 0.0,
        }
    }

    /// Reset the scroll offset to the top — used when the presented content
    /// changes wholesale (a tab switch, a new search) so the old offset does not
    /// leave the new, shorter list scrolled past its end.
    pub const fn scroll_to_top(&mut self) {
        self.scroll = 0.0;
    }

    /// The current scroll offset from the top, in logical pixels — read by a
    /// consumer that maps a pointer position back to a row index (the inventory
    /// drag-and-drop hit test).
    #[must_use]
    pub const fn scroll_offset(&self) -> f32 {
        self.scroll
    }

    /// Nudge the scroll offset by `delta` logical pixels (positive scrolls
    /// toward the end). Clamped at the top here; the layout system clamps the
    /// far end against the live viewport height, exactly as it does for the
    /// wheel path.
    pub const fn scroll_by(&mut self, delta: f32) {
        self.scroll = (self.scroll + delta).max(0.0);
    }

    /// Scroll so the row at `index` sits at the top of the viewport — the jump
    /// that reveals a specific row (the inventory "Show in Main view" action).
    /// The far end is clamped by the layout system against the live viewport
    /// height, exactly as the wheel path is.
    pub fn scroll_to_index(&mut self, index: usize) {
        self.scroll = row_top(index, self.row_height);
    }
}

/// A pooled row entity: a child of a [`VirtualList`] viewport that is repeatedly
/// re-bound to whichever item is currently at its screen position.
///
/// [`slot`](Self::slot) is the row's fixed place in the pool (`0..pool_len`);
/// [`index`](Self::index) is the model item it currently shows, or `None` when
/// the pool has more rows than the window needs and this one is parked.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualRow {
    /// This row's fixed index within its list's pool.
    pub slot: usize,
    /// The model item index this row currently presents, or `None` when parked
    /// (hidden). The consumer reads this to know what to draw.
    pub index: Option<usize>,
}

/// Marks the entity a pooled row's `ChildOf` points at as a virtual-list
/// viewport, so the pool-building system can find the list an
/// [`Added`] row belongs to. Inserted automatically
/// alongside [`VirtualList`] would be ideal, but the consumer spawns the
/// viewport, so the layout system tolerates its absence and treats any
/// [`VirtualList`] entity as the pool parent.
#[derive(Component, Debug, Clone, Copy)]
pub struct VirtualViewport;

/// The contiguous window of item indices that must have a live row entity: the
/// rows on screen, plus `OVERSCAN_ROWS` beyond each edge, clamped to the item
/// count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowWindow {
    /// The first item index in the window.
    pub first: usize,
    /// How many items the window spans.
    pub count: usize,
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

/// Which model item pool slot `slot` presents, out of a pool of `pool_len` rows
/// showing `window` — or `None` when the window is narrower than the pool and
/// this slot is parked.
///
/// **This is what makes the widget a recycler rather than a re-binder.** The
/// mapping is *modular*: item `i` always lives in slot `i % pool_len`. Scroll by
/// one row and exactly **one** slot changes item — the row that just left the
/// top is the one that takes the item arriving at the bottom, which is the
/// promise in the module docs. The obvious alternative, `first + slot`, maps by
/// offset, so a one-row scroll changes the item under *every* slot and wakes
/// every consumer's `Changed<VirtualRow>` bind: N rebinds per row scrolled,
/// where the whole point of pooling is one.
///
/// The pool length is the modulus, so it must be the *settled* length for the
/// frame (already grown to cover the window), not the length before growth. The
/// modulus changing does rebind the whole pool — but the pool only ever grows,
/// and only up to what the viewport needs, so that is a handful of times over a
/// list's life rather than once per scroll step.
fn slot_index(slot: usize, window: RowWindow, pool_len: usize) -> Option<usize> {
    if slot >= pool_len {
        return None;
    }
    // Which slot the window's first item lands in. `slot < pool_len` already
    // implies a non-empty pool, so this remainder is always defined.
    let anchor = window.first.checked_rem(pool_len)?;
    // How far into the window the item congruent to `slot` sits: `slot - anchor`
    // modulo the pool length, adding a whole pool first so the subtraction stays
    // in `usize`. The result is in `0..pool_len`, and the window is never wider
    // than the pool, so distinct slots always name distinct items.
    let offset = slot
        .checked_add(pool_len)?
        .checked_sub(anchor)?
        .checked_rem(pool_len)?;
    (offset < window.count).then(|| window.first.saturating_add(offset))
}

/// The [`Node`] a pooled row carries: a full-width absolutely positioned band at
/// its item's offset within the scrolled content, or hidden when parked.
///
/// Shared by the growth path (which spawns a row already bound) and the bind
/// path (which writes the same three fields in place), so a fresh row and a
/// recycled one cannot end up laid out differently.
fn row_node(index: Option<usize>, row_height: f32, scroll: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        right: Val::Px(0.0),
        height: Val::Px(row_height),
        top: Val::Px(index.map_or(0.0, |index| row_top(index, row_height) - scroll)),
        display: if index.is_some() {
            Display::Flex
        } else {
            Display::None
        },
        ..default()
    }
}

/// Amend a pooled row's [`Node`] in place, leaving the fields this module owns
/// alone — **the supported way for a consumer to dress its rows**.
///
/// A row's `top` and `display` belong to [`layout_virtual_lists`]: where the row
/// sits in the scrolled content, and whether it is parked because the window is
/// narrower than the pool. A consumer wants the *other* fields — its alignment,
/// gap and padding — and reaching for them by inserting a fresh `Node`
/// **replaces** the whole component, placement included, so a parked row comes
/// back at `top: Auto` and visible until the next layout pass takes it away
/// again. Amending says "these fields are mine" and means it.
///
/// The amendment runs against whatever node the row already has; a row with none
/// (a test fixture, say) gets a default one first, exactly as an insert would
/// have given it.
pub fn amend_row_node(
    commands: &mut Commands,
    row: Entity,
    amend: impl FnOnce(&mut Node) + Send + Sync + 'static,
) {
    commands
        .entity(row)
        .entry::<Node>()
        .or_default()
        .and_modify(move |mut node| amend(&mut node));
}

/// Route the wheel to the virtual list under the pointer. The world camera
/// never zooms at the same time: a list viewport is blocking UI, and the
/// camera's wheel zoom ignores a scroll over blocking UI — see the
/// [module docs](self) for the coordination.
pub fn scroll_virtual_lists(
    wheel: Res<AccumulatedMouseScroll>,
    hover_map: Res<HoverMap>,
    child_of: Query<&ChildOf>,
    mut lists: Query<&mut VirtualList>,
) {
    if wheel.delta.y.abs() < f32::EPSILON {
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
///
/// Which slot shows which item is `slot_index`'s modular mapping, so a scroll
/// rebinds only the rows that actually changed item. A row grown this frame is
/// spawned already bound and positioned, so it never renders blank.
pub fn layout_virtual_lists(
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

        // The pool length is the modulus [`slot_index`] turns on, so settle it
        // *before* binding anything: the rows grown below must land on the same
        // mapping as the rows that already exist, or the two halves of one frame
        // would disagree about which slot shows which item.
        let pool_len = pool.len().max(window.count);

        // Grow the pool until it can cover the window. A grown row is spawned
        // already bound to its item and already positioned — the bind loop below
        // could not do it, because `commands.spawn` is deferred and `rows` cannot
        // see an entity that does not exist yet, which used to leave every fresh
        // row blank and `Display::None` for one frame.
        for slot in pool.len()..window.count {
            let index = slot_index(slot, window, pool_len);
            commands.spawn((
                VirtualRow { slot, index },
                row_node(index, list.row_height, list.scroll),
                ChildOf(list_entity),
            ));
        }

        // Bind each pooled row to its window item (or park it) and position it.
        for &(entity, slot) in &pool {
            let index = slot_index(slot, window, pool_len);
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
/// trick `coords::metres_to_f32` uses. Counts far beyond `u32` are not
/// reachable by any real inventory, and saturate rather than wrap.
#[must_use]
pub fn index_to_f32(n: usize) -> f32 {
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
    use bevy::prelude::*;
    use std::collections::BTreeSet;

    use super::{
        OVERSCAN_ROWS, RowWindow, VirtualList, VirtualRow, content_height, floor_to_usize,
        index_to_f32, layout_virtual_lists, max_scroll, row_top, row_window, scrollbar_geometry,
        slot_index,
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

    /// Every slot's item, in slot order — the whole mapping for one frame.
    fn slot_map(window: RowWindow, pool_len: usize) -> Vec<Option<usize>> {
        (0..pool_len)
            .map(|slot| slot_index(slot, window, pool_len))
            .collect()
    }

    /// At the top of the list, with a pool sized exactly to the window, the
    /// modular mapping is the identity — which is why every test and consumer
    /// written against the old offset mapping still reads the same.
    #[test]
    fn a_full_pool_at_the_top_maps_slot_to_item() {
        let window = RowWindow { first: 0, count: 8 };
        assert_eq!(
            slot_map(window, 8),
            (0..8).map(Some).collect::<Vec<Option<usize>>>()
        );
    }

    /// The mapping is a bijection: every item in the window has exactly one
    /// slot, and no two slots claim the same item.
    #[test]
    fn every_window_item_gets_exactly_one_slot() {
        let pool_len = 11;
        for first in 0..40 {
            let window = RowWindow {
                first,
                count: pool_len,
            };
            let items: BTreeSet<usize> = slot_map(window, pool_len).into_iter().flatten().collect();
            assert_eq!(
                items,
                (first..first.saturating_add(pool_len)).collect::<BTreeSet<usize>>(),
                "first={first}: the pool must cover the window exactly once"
            );
        }
    }

    /// **The bug this widget exists to avoid.** Scrolling by one row must move
    /// exactly one slot to a new item — the row that left the top takes the item
    /// arriving at the bottom. The offset mapping (`first + slot`) changed every
    /// slot instead, waking every consumer's `Changed<VirtualRow>` bind.
    #[test]
    fn a_one_row_scroll_rebinds_exactly_one_slot() {
        let pool_len = 11;
        for first in 0..40 {
            let before = slot_map(
                RowWindow {
                    first,
                    count: pool_len,
                },
                pool_len,
            );
            let after = slot_map(
                RowWindow {
                    first: first.saturating_add(1),
                    count: pool_len,
                },
                pool_len,
            );
            let moved = before
                .iter()
                .zip(&after)
                .filter(|(before, after)| before != after)
                .count();
            assert_eq!(moved, 1, "first={first}: one row of scroll, one rebind");
        }
    }

    /// A pool wider than the window parks its surplus slots, and parks the same
    /// number however far down the list the window sits.
    #[test]
    fn a_pool_wider_than_the_window_parks_the_surplus() {
        let window = RowWindow {
            first: 17,
            count: 6,
        };
        let parked = slot_map(window, 10)
            .into_iter()
            .filter(Option::is_none)
            .count();
        assert_eq!(parked, 4);
        // An empty window parks the whole pool.
        let empty = RowWindow { first: 0, count: 0 };
        assert_eq!(slot_map(empty, 10).into_iter().flatten().count(), 0);
        // A slot outside the pool never has an item.
        assert_eq!(slot_index(10, window, 10), None);
        assert_eq!(slot_index(0, window, 0), None);
    }

    /// The scrollbar hides while the content fits, and otherwise reports a thumb
    /// proportional to the visible fraction beside the same `max_scroll` the
    /// layout pass clamps against.
    #[expect(
        clippy::float_cmp,
        reason = "the geometry is exact at these representable inputs, asserted exactly"
    )]
    #[test]
    fn scrollbar_geometry_tracks_the_visible_fraction() -> Result<(), TestError> {
        // Content that fits, and a degenerate viewport: no bar at all.
        assert!(scrollbar_geometry(5, 20.0, 100.0).is_none());
        assert!(scrollbar_geometry(50, 20.0, 0.0).is_none());
        // Two fifths of the content is visible, so the thumb covers two fifths
        // of the track, and the content scrolls by everything below the fold.
        let geometry = scrollbar_geometry(25, 10.0, 100.0).ok_or("a 25-row list must scroll")?;
        assert_eq!(geometry.thumb_height, 40.0);
        assert_eq!(geometry.max_scroll, 150.0);
        // A very long list would compute a sub-pixel thumb; it is floored at the
        // minimum so it stays grabbable.
        let long = scrollbar_geometry(100_000, 20.0, 100.0).ok_or("a huge list must scroll")?;
        assert_eq!(long.thumb_height, super::SCROLLBAR_MIN_THUMB);
        Ok(())
    }

    /// Errors a test reports rather than unwrapping.
    type TestError = Box<dyn core::error::Error>;

    /// How many rows the last [`layout_virtual_lists`] run actually re-bound —
    /// the number a consumer's `Changed<VirtualRow>` bind would have paid for.
    #[derive(Resource, Default)]
    struct Rebinds(usize);

    /// Record the frame's rebind count. Runs straight after the layout pass, so
    /// it sees exactly the writes that pass made.
    fn count_rebinds(mut rebinds: ResMut<Rebinds>, rows: Query<(), Changed<VirtualRow>>) {
        rebinds.0 = rows.iter().count();
    }

    /// An app carrying just the pool machinery and the rebind counter — no
    /// `bevy_ui` layout, so the viewport's [`ComputedNode`] is written by hand.
    fn pool_app(item_count: usize, row_height: f32, viewport_height: f32) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Rebinds>()
            .add_systems(Update, (layout_virtual_lists, count_rebinds).chain());
        let mut list = VirtualList::new(row_height);
        list.item_count = item_count;
        let viewport = app
            .world_mut()
            .spawn((
                list,
                Node::default(),
                ComputedNode {
                    size: Vec2::new(200.0, viewport_height),
                    ..ComputedNode::DEFAULT
                },
            ))
            .id();
        (app, viewport)
    }

    /// Every pooled row's item, in slot order.
    fn pooled_items(app: &mut App, viewport: Entity) -> Vec<Option<usize>> {
        let mut query = app.world_mut().query::<(&VirtualRow, &ChildOf)>();
        let mut rows = query
            .iter(app.world())
            .filter(|&(_, child_of)| child_of.parent() == viewport)
            .map(|(row, _)| (row.slot, row.index))
            .collect::<Vec<(usize, Option<usize>)>>();
        rows.sort_unstable_by_key(|&(slot, _)| slot);
        rows.into_iter().map(|(_, index)| index).collect()
    }

    /// A row grown this frame comes up **already bound and already placed**. The
    /// growth loop spawns through `Commands`, so the bind loop right below it
    /// cannot see the new entity — it used to `continue` past it, leaving the row
    /// blank and `Display::None` until the next frame.
    #[test]
    fn a_grown_row_comes_up_already_bound() -> Result<(), TestError> {
        let (mut app, viewport) = pool_app(1000, 20.0, 100.0);
        app.update();

        let items = pooled_items(&mut app, viewport);
        assert!(!items.is_empty(), "the pool must cover the viewport");
        assert!(
            items.iter().all(Option::is_some),
            "a row spawned to cover the window is bound to its item: {items:?}"
        );
        let mut nodes = app.world_mut().query::<(&VirtualRow, &Node)>();
        for (row, node) in nodes.iter(app.world()) {
            let index = row.index.ok_or("a covering row lost its item")?;
            assert_eq!(node.display, Display::Flex, "slot {} is hidden", row.slot);
            assert_eq!(
                node.top,
                Val::Px(row_top(index, 20.0)),
                "slot {} sits at the wrong offset",
                row.slot
            );
        }
        Ok(())
    }

    /// End to end through the real system: one row of scroll costs one rebind,
    /// and a still list costs none.
    #[test]
    fn scrolling_one_row_wakes_one_bind() -> Result<(), TestError> {
        let (mut app, viewport) = pool_app(1000, 20.0, 100.0);
        // Start in the middle: near the top the overscan pins the window's first
        // index at 0, so scrolling a row would not move the window at all.
        app.world_mut()
            .get_mut::<VirtualList>(viewport)
            .ok_or("the viewport lost its list")?
            .scroll_to_index(20);
        // Settle: the first frame grows the pool, the second is the consumer's
        // `Added` frame, the third is quiet.
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Rebinds>().0,
            0,
            "a list nobody touched must not wake a single bind"
        );
        let before = pooled_items(&mut app, viewport);

        app.world_mut()
            .get_mut::<VirtualList>(viewport)
            .ok_or("the viewport lost its list")?
            .scroll_by(20.0);
        app.update();

        assert_eq!(
            app.world().resource::<Rebinds>().0,
            1,
            "one row of scroll must re-bind one pooled row, not the whole pool"
        );
        let after = pooled_items(&mut app, viewport);
        // And the one that moved took the item arriving at the bottom.
        let moved: Vec<(Option<usize>, Option<usize>)> = before
            .iter()
            .zip(&after)
            .filter(|(before, after)| before != after)
            .map(|(before, after)| (*before, *after))
            .collect();
        assert_eq!(moved.len(), 1);
        let (left, arrived) = *moved.first().ok_or("no row moved")?;
        let left = left.ok_or("the recycled row was parked")?;
        let arrived = arrived.ok_or("the recycled row was parked")?;
        assert!(
            arrived > left,
            "scrolling down recycles the top row to the bottom: {left} -> {arrived}"
        );
        Ok(())
    }
}
