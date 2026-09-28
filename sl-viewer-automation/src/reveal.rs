//! Making a node actionable the way a user would: scrolling it into view,
//! paging a virtual list until the row is bound, and opening a floater.

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sl_automation_proto::{AutomationError, Locator, Role};

use crate::pursuit::PursuitError;
use sl_viewer_ui_core::ui::UiPanelShown;
use sl_viewer_ui_core::virtual_list::VirtualList;
use sl_viewer_ui_widgets::floater::{Floater, floater_panel};

/// How many frames a virtual list is given after each scroll before the rows
/// it shows are judged: one for the pool to rebind, one for the consumer's
/// bind and the layout after it.
const SEARCH_SETTLE_FRAMES: u32 = 2;

/// Open the singleton floater whose stable [`Floater::id`] is `id`, as
/// `SL_VIEWER_OPEN_FLOATER` does, and return the locator of its window — the
/// scope to find its content within.
///
/// # Errors
///
/// [`AutomationError::NotFound`] naming the window's locator when there is no
/// singleton floater with that id, and [`PursuitError::Model`] when the
/// floaters cannot be queried at all.
pub fn open_floater(world: &mut World, id: &str) -> Result<Locator, PursuitError> {
    let window = window_locator(format!("floater:{id}"));
    let mut state = SystemState::<Query<'_, '_, (Entity, &Floater)>>::new(world);
    let panel = floater_panel(&state.get(world)?, id);
    let not_found = || AutomationError::NotFound {
        locator: window.clone(),
    };
    let panel = panel.ok_or_else(not_found)?;
    let mut shown = world.get_mut::<UiPanelShown>(panel).ok_or_else(not_found)?;
    shown.0 = true;
    Ok(match world.get::<Name>(panel) {
        Some(name) => window_locator(name.as_str().to_owned()),
        None => window,
    })
}

/// The window whose test id is `test_id`.
fn window_locator(test_id: String) -> Locator {
    Locator {
        role: Some(Role::Window),
        ..Locator::test_id(test_id)
    }
}

/// Scroll the innermost scroll area or virtual list that holds `entity` out of
/// sight just far enough to bring it in: its near edge to the area's near edge,
/// or its far edge to the far one when it fits. Returns whether anything
/// scrolled.
///
/// One area per call: an outer area's geometry is only right once the inner
/// one has been laid out again, so a node nested in two scroll areas comes into
/// view over two frames.
pub fn scroll_into_view(world: &mut World, entity: Entity) -> bool {
    let Some(node) = logical_rect(world, entity) else {
        return false;
    };
    let mut ancestor = entity;
    while let Some(parent) = world.get::<ChildOf>(ancestor) {
        ancestor = parent.parent();
        let Some(area) = logical_rect(world, ancestor) else {
            continue;
        };
        if let Some(mut list) = world.get_mut::<VirtualList>(ancestor) {
            let delta = scroll_needed(node.min.y, node.max.y, area.min.y, area.max.y);
            if delta != 0.0 {
                list.scroll_by(delta);
                return true;
            }
            continue;
        }
        let Some(overflow) = world.get::<Node>(ancestor).map(|style| style.overflow) else {
            continue;
        };
        let delta = Vec2::new(
            if overflow.x == OverflowAxis::Scroll {
                scroll_needed(node.min.x, node.max.x, area.min.x, area.max.x)
            } else {
                0.0
            },
            if overflow.y == OverflowAxis::Scroll {
                scroll_needed(node.min.y, node.max.y, area.min.y, area.max.y)
            } else {
                0.0
            },
        );
        if delta != Vec2::ZERO
            && let Some(mut position) = world.get_mut::<ScrollPosition>(ancestor)
        {
            position.0 = Vec2::new(position.x + delta.x, position.y + delta.y);
            return true;
        }
    }
    false
}

/// How far to scroll an area spanning `area_min..area_max` so that the span
/// `min..max` comes into it — negative toward the start. Zero when it is
/// already inside; a span larger than the area is aligned at its start.
fn scroll_needed(min: f32, max: f32, area_min: f32, area_max: f32) -> f32 {
    if min < area_min {
        min - area_min
    } else if max > area_max {
        (max - area_max).min(min - area_min)
    } else {
        0.0
    }
}

/// Whether `scope` is an ancestor of `entity`.
fn is_inside(world: &World, entity: Entity, scope: Entity) -> bool {
    let mut node = entity;
    while let Some(parent) = world.get::<ChildOf>(node) {
        node = parent.parent();
        if node == scope {
            return true;
        }
    }
    false
}

/// `entity`'s box in logical pixels, or `None` when it is not a laid-out UI
/// node.
fn logical_rect(world: &World, entity: Entity) -> Option<Rect> {
    let computed = world.get::<ComputedNode>(entity)?;
    let transform = world.get::<UiGlobalTransform>(entity)?;
    let scale = computed.inverse_scale_factor;
    let (centre_x, centre_y) = (
        transform.translation.x * scale,
        transform.translation.y * scale,
    );
    let (half_x, half_y) = (computed.size.x * scale / 2.0, computed.size.y * scale / 2.0);
    Some(Rect::new(
        centre_x - half_x,
        centre_y - half_y,
        centre_x + half_x,
        centre_y + half_y,
    ))
}

/// A virtual list only shows the rows in its window, so a row further down is
/// not in the tree at all until the list is scrolled to it. This pages through
/// the lists in scope — the way a user scrolls a list looking for a row — until
/// the locator matches, one list at a time, each from its top.
///
/// A list searched to its end without a match is put back where it was and not
/// searched again by the same action.
#[derive(Debug, Default)]
pub(crate) struct ListSearch {
    /// The list being paged, if one is.
    current: Option<Paging>,
    /// The lists already searched to their end.
    searched: Vec<Entity>,
}

/// One list being paged.
#[derive(Debug, Clone, Copy)]
struct Paging {
    /// The list's viewport.
    list: Entity,
    /// Its scroll offset before the search, restored if the row is not in it.
    original: f32,
    /// Frames still to wait before judging what the list shows.
    settle: u32,
    /// The offset the last page was turned from; unchanged after a turn means
    /// the list is at its end.
    turned_from: Option<f32>,
}

impl ListSearch {
    /// The locator matched: whatever list was being paged stays where it is,
    /// showing the row.
    pub(crate) const fn found(&mut self) {
        self.current = None;
    }

    /// Advance the search one frame, over the lists at or under `scope` (every
    /// list when `None`). Once every list in scope has been searched to its end,
    /// this does nothing.
    pub(crate) fn step(&mut self, world: &mut World, scope: Option<Entity>) {
        let Some(mut paging) = self.current.take() else {
            self.start(world, scope);
            return;
        };
        if paging.settle > 0 {
            paging.settle = paging.settle.saturating_sub(1);
            self.current = Some(paging);
            return;
        }
        let viewport_height = logical_rect(world, paging.list).map(|rect| rect.height());
        let Some(mut list) = world.get_mut::<VirtualList>(paging.list) else {
            // The list went away under the search; it has nothing left to show.
            self.searched.push(paging.list);
            return;
        };
        let offset = list.scroll_offset();
        let at_end = paging
            .turned_from
            .is_some_and(|from| (offset - from).abs() < 0.5);
        match viewport_height {
            Some(height) if !at_end && height > 0.0 => {
                paging.turned_from = Some(offset);
                list.scroll_by(height);
                paging.settle = SEARCH_SETTLE_FRAMES;
                self.current = Some(paging);
            }
            _ => {
                list.scroll_to_top();
                list.scroll_by(paging.original);
                self.searched.push(paging.list);
            }
        }
    }

    /// Begin paging the next list in scope that is shown and not yet
    /// searched, from its top, if there is one.
    fn start(&mut self, world: &mut World, scope: Option<Entity>) {
        let mut lists =
            world.query::<(Entity, &VirtualList, &ComputedNode, &InheritedVisibility)>();
        let mut candidates: Vec<(Entity, f32)> = lists
            .iter(world)
            .filter(|(list, _, computed, visibility)| {
                visibility.get()
                    && computed.size.y > 0.0
                    && !self.searched.contains(list)
                    && scope.is_none_or(|scope| *list == scope || is_inside(world, *list, scope))
            })
            .map(|(list, virtual_list, _, _)| (list, virtual_list.scroll_offset()))
            .collect();
        candidates.sort_unstable_by_key(|(list, _)| *list);
        let Some(&(list, original)) = candidates.first() else {
            return;
        };
        if let Some(mut virtual_list) = world.get_mut::<VirtualList>(list) {
            virtual_list.scroll_to_top();
        }
        self.current = Some(Paging {
            list,
            original,
            settle: SEARCH_SETTLE_FRAMES,
            turned_from: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::scroll_needed;

    /// Whether two scroll amounts agree to well under a pixel.
    fn same(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn scrolling_brings_a_span_just_into_the_area() {
        for (span, wanted, what) in [
            ((10.0, 20.0), 0.0, "inside"),
            ((-30.0, -10.0), -30.0, "above: its start to the area's"),
            ((150.0, 170.0), 70.0, "below: its end to the area's"),
            (
                (150.0, 400.0),
                150.0,
                "taller than the area: its start to the area's",
            ),
        ] {
            let got = scroll_needed(span.0, span.1, 0.0, 100.0);
            assert!(same(got, wanted), "{what}: {got} where {wanted} was wanted");
        }
    }
}
