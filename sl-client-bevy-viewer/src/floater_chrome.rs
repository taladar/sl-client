//! The **floater chrome sweep**: every window in `crate::floaters::FLOATERS`,
//! dragged, resized, minimized, docked and closed by a real pointer.
//!
//! `ui_test` sweeps the same registry for *layout* — every window in eight
//! scripts, three font sizes, three scale factors, three UI scales — and
//! `ui_contract` sweeps `ELEMENTS` for *behaviour*. This is the third: the
//! behaviour of the window **around** an element, driven through the picking
//! stack rather than through [`FloaterCommand`](crate::floater::FloaterCommand)
//! messages written by hand.
//!
//! # What a pointer reaches that a message does not
//!
//! `floater.rs`'s own tests already write the commands and assert the systems
//! act on them, and its `scenarios` module drives one fixture window through
//! the pointer. What neither can see is whether **this** window's chrome is
//! where a user's cursor would find it. The grip is an absolutely-placed node
//! pinned to the trailing-bottom corner through a logical inset and drawn over
//! the content; the title bar is stretched across a window whose width is its
//! content's; the button cluster takes whatever width the title leaves it. A
//! grip that mirrored to the wrong corner, a title bar that stopped filling the
//! header, or a close button squeezed to zero by a long title would pass every
//! command-level test and be un-grabbable in the viewer — per window, because
//! each one's chrome is sized by its own title and its own content.
//!
//! So a registered floater inherits chrome coverage the same way it inherits
//! the layout matrix: **by being in `FLOATERS`**.
//!
//! # One app per cell
//!
//! Each check builds its own app and spawns one window into it, because these
//! gestures deliberately change state — a closed window has no box for the next
//! gesture to aim at, and a minimized one has no grip. A cell that inherited
//! the previous cell's window would be pinning an order rather than a
//! behaviour.

use bevy::prelude::*;

use crate::floater::{FloaterCommand, FloaterElement, FloaterPlugin};
use crate::ui::{UiRoot, UiScaffoldSystems};
use crate::ui_element::ElementCx;
use crate::ui_test::interact::InteractionTest;
use crate::ui_test::{record, settle};

/// Build an interactive app with one registered floater open in it, settled.
///
/// The interaction counterpart of `ui_test`'s `spawn_registered_floater`, and
/// it differs in what it installs rather than in what it spawns: the **whole**
/// [`FloaterPlugin`] (the layout sweep deliberately takes only the three
/// layout-affecting systems, so that a badly placed window is reported rather
/// than clamped) plus the element hosting a content specimen's observers need,
/// because here a pointer really does travel over that content.
///
/// [`FloaterCommand`]s are recorded, so a check can assert which
/// [`FloaterOp`](crate::floater::FloaterOp) a button emitted rather than only
/// what the state became.
pub(crate) fn floater_app(test: InteractionTest, floater: &FloaterElement) -> App {
    let mut app = test.build();
    app.add_plugins(FloaterPlugin);
    crate::ui_contract::install_element_hosting(&mut app);
    record::<FloaterCommand>(&mut app);
    let spawn = *floater;
    app.add_systems(
        Startup,
        (move |mut commands: Commands, root: Res<UiRoot>| {
            spawn.spawn(&mut commands, root.0, ElementCx::new());
        })
        .after(UiScaffoldSystems::SpawnRoot),
    );
    settle(&mut app);
    app
}

#[cfg(test)]
mod tests {
    use super::floater_app;
    use crate::floater::{Floater, FloaterCommand, FloaterGeometry, FloaterOp, FloaterSpec};
    use crate::floaters::FLOATERS;
    use crate::ui::{UiPanelShown, UiRoot};
    use crate::ui_test::interact::{self, InteractionTest, centre_of_entity};
    use crate::ui_test::{
        TestError, border_box, drain, find_by_name, interaction_violations, settle,
    };
    use bevy::prelude::*;

    /// How many frames a drag is stepped over — four, because a reader that
    /// only looked at the press and the release would pass a one-step drag.
    const DRAG_STEPS: u32 = 4;

    /// Where the move check parks every window, in logical pixels. A definite
    /// destination rather than a delta, so the check is the same for a window
    /// that opens at the top-leading corner and one that opens near the trailing
    /// edge — and so no window is dragged somewhere it would legitimately
    /// overhang the viewport and turn the layout re-check into a placement
    /// complaint.
    const MOVE_TO: Vec2 = Vec2::new(100.0, 100.0);

    /// How far the grip is dragged to grow a window, in logical pixels.
    const GROW_BY: Vec2 = Vec2::new(80.0, 60.0);

    /// How far the grip is dragged to shrink a window: far past any window's
    /// own floor, so what is left is the floor itself.
    const SHRINK_BY: Vec2 = Vec2::new(-2000.0, -2000.0);

    /// How far past the trailing-bottom corner the clamp check throws a window,
    /// in logical pixels.
    const OFF_SCREEN_BY: f32 = 400.0;

    /// How much of a thrown-off window's title bar must still be on screen, in
    /// logical pixels, for it to be grabbable. The manager promises
    /// `FLOATER_MIN_VISIBLE_PIXELS` (16, the reference's constant); this is the
    /// slack-allowing floor the sweep holds it to, so a 1 px border or a
    /// rounding difference is not a failure while a window lost off the edge
    /// still is.
    const MIN_GRABBABLE: f32 = 8.0;

    /// The most of a thrown-off window's title bar that may still be on screen,
    /// in logical pixels, for the window to count as having reached the edge.
    /// Comfortably above the promised 16 and far below any window's width, so
    /// it separates "clamped at the edge" from "never moved".
    const AT_THE_EDGE: f32 = 32.0;

    /// How far a dragged window may land from where the pointer left it, in
    /// logical pixels. A drag is stepped over four frames and each step's delta
    /// is accumulated, so the total is exact bar float error.
    const DRAG_TOLERANCE: f32 = 1.5;

    /// How far a resized content area may land from the size the grip asked
    /// for, in logical pixels. Looser than [`DRAG_TOLERANCE`] because the
    /// starting size is *measured* (a content-driven window seeds its size from
    /// the laid-out slot on the first grip press) rather than declared.
    const RESIZE_TOLERANCE: f32 = 2.5;

    /// The registered floater's live root entity.
    fn root_of(app: &mut App, id: &str) -> Option<Entity> {
        find_by_name(app, &format!("floater:{id}"))
    }

    /// A live floater's persistable geometry.
    fn geometry(app: &App, root: Entity) -> Option<FloaterGeometry> {
        app.world().get::<Floater>(root).map(Floater::geometry)
    }

    /// The named chrome node of **this** window.
    ///
    /// By child search rather than by name lookup, because the chrome names are
    /// shared: `floater-title-bar` addresses a part of every window, and the
    /// sweep must aim at the one belonging to the floater under test even when
    /// (as in the dock check) a second container is in the tree.
    fn chrome(app: &App, root: Entity, name: &str) -> Option<Entity> {
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if app
                .world()
                .get::<Name>(entity)
                .is_some_and(|found| found.as_str() == name)
            {
                return Some(entity);
            }
            if let Some(children) = app.world().get::<Children>(entity) {
                stack.extend(children.iter());
            }
        }
        None
    }

    /// Where a user would aim at that chrome node, in logical pixels.
    fn aim(app: &App, root: Entity, name: &str) -> Option<Vec2> {
        let entity = chrome(app, root, name)?;
        centre_of_entity(app, entity)
    }

    /// Drag from `from` by `delta`, in logical pixels.
    fn drag_by(app: &mut App, from: Vec2, delta: Vec2) {
        // Component-wise `f32`: the workspace's `arithmetic_side_effects` lint
        // fires on `glam`'s overloaded operators.
        let to = Vec2::new(from.x + delta.x, from.y + delta.y);
        interact::drag(app, from, to, DRAG_STEPS, MouseButton::Left);
        settle(app);
    }

    /// The [`FloaterOp`]s emitted since the last drain, for `root` only.
    ///
    /// Every chrome button raises its window before it acts, and the press also
    /// bubbles to the window's own raise observer, so a click emits two or three
    /// commands of which at most one is the button's own. Filtering the raises
    /// out leaves exactly what the button meant.
    fn ops(app: &mut App, root: Entity) -> Vec<FloaterOp> {
        drain::<FloaterCommand>(app)
            .into_iter()
            .filter(|command| command.floater == root && command.op != FloaterOp::BringToFront)
            .map(|command| command.op)
            .collect()
    }

    /// Whether the window has been raised above the resting plane — the paint
    /// order a bring-to-front hands out (`FloaterZTop` starts above 0, and a
    /// window spawns at 0).
    fn raised(app: &App, root: Entity) -> bool {
        app.world()
            .get::<GlobalZIndex>(root)
            .is_some_and(|index| index.0 > 0)
    }

    /// **Every window lays out cleanly with its text measured for editing.**
    ///
    /// The resting-state check, and it is not a duplicate of `ui_test`'s: the
    /// layout matrix has no editable-text stack, so a field sized in *visible
    /// lines* (`EditableText::visible_lines`, whose intrinsic height only exists
    /// once `update_editable_text_content_size` has run) is measured there at
    /// nothing. Under the pointer stack it has its real height, and the window
    /// around it is measured against the rect it declared.
    ///
    /// That gap is exactly where this sweep's first two findings lived: the
    /// notecard and script editors each opened with a body field taller than
    /// their own `default_size`, and a floater's content slot **clips**, so the
    /// Save button below it was cut off the bottom of the window — reachable by
    /// no click, in a window the layout matrix had reported clean in eight
    /// scripts.
    #[test]
    fn every_floater_lays_out_under_the_pointer_stack() {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            let mut app = floater_app(test, floater);
            let violations = interaction_violations(&mut app, test.layout());
            if !violations.is_empty() {
                failures.push(format!("floater `{}`: {violations:#?}", floater.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every window follows its title bar.**
    ///
    /// The drag is aimed at the bar where it is *drawn*, so a title bar that
    /// stopped filling the header — or that a long title pushed out from under
    /// the cluster — fails here rather than in the viewer.
    #[test]
    fn every_floater_moves_with_its_title_bar() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let before = geometry(&app, root).ok_or("the window lost its state")?;
            let Some(bar) = aim(&app, root, "floater-title-bar") else {
                failures.push(format!("floater `{}`: no title bar to grab", floater.id));
                continue;
            };
            let travel = Vec2::new(MOVE_TO.x - before.position.x, MOVE_TO.y - before.position.y);
            drag_by(&mut app, bar, travel);

            let after = geometry(&app, root).ok_or("the window lost its state")?;
            if after.position.distance(MOVE_TO) > DRAG_TOLERANCE {
                failures.push(format!(
                    "floater `{}`: dragged by {travel:?} from {:?}, landed at {:?} rather than \
                     {MOVE_TO:?}",
                    floater.id, before.position, after.position
                ));
            }
            if after.content_size != before.content_size {
                failures.push(format!(
                    "floater `{}`: moving the window resized it, {:?} -> {:?}",
                    floater.id, before.content_size, after.content_size
                ));
            }
            let violations = interaction_violations(&mut app, test.layout());
            if !violations.is_empty() {
                failures.push(format!(
                    "floater `{}` after the move: {violations:#?}",
                    floater.id
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// **No window can be thrown away.**
    ///
    /// Dragged hard past the trailing-bottom corner, every window keeps enough
    /// of its title bar on screen to be dragged back — the one guarantee
    /// `clamp_floaters_on_screen` exists to make, asserted through a real drag
    /// so the clamp is measured against the window a pointer actually moved
    /// rather than against a position written by hand.
    ///
    /// `interaction_violations` is deliberately **not** re-asserted: a window parked
    /// at the edge is outside the viewport and outside its parent on purpose,
    /// and the checks that say so would be reporting the fixture.
    #[test]
    fn every_floater_can_be_dragged_back_from_the_edge() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let viewport = test.layout().viewport().as_vec2();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let Some(bar) = aim(&app, root, "floater-title-bar") else {
                failures.push(format!("floater `{}`: no title bar to grab", floater.id));
                continue;
            };
            drag_by(
                &mut app,
                bar,
                Vec2::new(
                    viewport.x - bar.x + OFF_SCREEN_BY,
                    viewport.y - bar.y + OFF_SCREEN_BY,
                ),
            );

            let title_bar =
                chrome(&app, root, "floater-title-bar").ok_or("the window lost its title bar")?;
            let Some(visible) = on_screen(&app, title_bar, viewport) else {
                failures.push(format!(
                    "floater `{}`: the title bar never laid out",
                    floater.id
                ));
                continue;
            };
            if visible.x < MIN_GRABBABLE || visible.y < MIN_GRABBABLE {
                failures.push(format!(
                    "floater `{}`: thrown at the corner it left only {visible:?} logical pixels of \
                     its title bar on a {viewport:?} screen — there is nothing left to drag it \
                     back by",
                    floater.id
                ));
            }
            // The other side of the same claim, and the one that keeps this
            // check honest: the window has to have *reached* the edge. A drag
            // whose events never arrived (a pointer that stops being delivered
            // once it leaves the window would do it) would leave the window
            // sitting comfortably on screen and pass the bound above without
            // the clamp ever running.
            if visible.x > AT_THE_EDGE || visible.y > AT_THE_EDGE {
                failures.push(format!(
                    "floater `{}`: after a drag past the corner {visible:?} logical pixels of its \
                     title bar are still on screen — the window never reached the edge, so the \
                     clamp was not what stopped it",
                    floater.id
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// How much of `entity`'s box is inside the viewport, in logical pixels per
    /// axis.
    fn on_screen(app: &App, entity: Entity, viewport: Vec2) -> Option<Vec2> {
        let computed = app.world().get::<ComputedNode>(entity)?;
        let transform = app.world().get::<UiGlobalTransform>(entity)?;
        let node_box = border_box(computed, transform);
        let logical = computed.inverse_scale_factor();
        let width = node_box.max.x.min(viewport.x) - node_box.min.x.max(0.0);
        let height = node_box.max.y.min(viewport.y) - node_box.min.y.max(0.0);
        Some(Vec2::new(
            width.max(0.0) * logical,
            height.max(0.0) * logical,
        ))
    }

    /// **Every resizable window grows with its grip**, and the window's own box
    /// grows with the content area.
    #[test]
    fn every_resizable_floater_grows_with_its_grip() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            if !(floater.spec)().caps.resizable {
                continue;
            }
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let content =
                chrome(&app, root, "floater-content").ok_or("the window has no content")?;
            let before = logical_size(&app, content).ok_or("the content never laid out")?;
            let width_before = logical_size(&app, root)
                .ok_or("the window never laid out")?
                .x;
            let Some(grip) = aim(&app, root, "floater-resize") else {
                failures.push(format!(
                    "floater `{}`: resizable, but its grip is nowhere a pointer can reach it",
                    floater.id
                ));
                continue;
            };
            drag_by(&mut app, grip, GROW_BY);

            let wanted = Vec2::new(before.x + GROW_BY.x, before.y + GROW_BY.y);
            let after = geometry(&app, root)
                .and_then(|state| state.content_size)
                .ok_or("the grip gave the window no manual size")?;
            if after.distance(wanted) > RESIZE_TOLERANCE {
                failures.push(format!(
                    "floater `{}`: the grip was dragged {GROW_BY:?} from a {before:?} content \
                     area, which left {after:?} rather than {wanted:?}",
                    floater.id
                ));
            }
            let width_after = logical_size(&app, root).ok_or("the window lost its box")?.x;
            if width_after <= width_before {
                failures.push(format!(
                    "floater `{}`: the content area grew but the window did not, {width_before} \
                     -> {width_after}",
                    floater.id
                ));
            }
            let violations = interaction_violations(&mut app, test.layout());
            if !violations.is_empty() {
                failures.push(format!(
                    "floater `{}` after the resize: {violations:#?}",
                    floater.id
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// **Every resizable window stops at its own floor.**
    ///
    /// Dragged far past any plausible minimum, the content area lands on the
    /// window's declared [`FloaterSpec::min_size`] — and where a window declares
    /// none, on the manager's shared floor, which is asserted as a floor rather
    /// than a number: the grip stops somewhere positive and a second shove does
    /// not move it. A window that shrank to nothing would let its own chrome
    /// spill out of it.
    #[test]
    fn every_resizable_floater_stops_at_its_own_floor() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            let spec: FloaterSpec = (floater.spec)();
            if !spec.caps.resizable {
                continue;
            }
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let Some(grip) = aim(&app, root, "floater-resize") else {
                failures.push(format!("floater `{}`: no grip to drag", floater.id));
                continue;
            };
            drag_by(&mut app, grip, SHRINK_BY);
            let floored = geometry(&app, root)
                .and_then(|state| state.content_size)
                .ok_or("the grip gave the window no manual size")?;

            if floored.x <= 0.0 || floored.y <= 0.0 {
                failures.push(format!(
                    "floater `{}`: the grip shrank the content area to {floored:?}",
                    floater.id
                ));
                continue;
            }
            if let Some(min_size) = spec.min_size
                && floored.distance(min_size) > RESIZE_TOLERANCE
            {
                failures.push(format!(
                    "floater `{}`: declares a {min_size:?} minimum, but the grip stopped at \
                     {floored:?}",
                    floater.id
                ));
            }
            // Shoved again, a real floor does not move. This is what holds a
            // window with no declared minimum to the shared one without
            // restating its value here.
            let Some(grip) = aim(&app, root, "floater-resize") else {
                failures.push(format!(
                    "floater `{}`: the grip left the window once it was at its floor",
                    floater.id
                ));
                continue;
            };
            drag_by(&mut app, grip, SHRINK_BY);
            let again = geometry(&app, root)
                .and_then(|state| state.content_size)
                .ok_or("the window lost its manual size")?;
            if again.distance(floored) > RESIZE_TOLERANCE {
                failures.push(format!(
                    "floater `{}`: shrank past what looked like its floor, {floored:?} -> \
                     {again:?}",
                    floater.id
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// A node's laid-out size in logical pixels.
    fn logical_size(app: &App, entity: Entity) -> Option<Vec2> {
        let computed = app.world().get::<ComputedNode>(entity)?;
        let size = computed.size();
        let logical = computed.inverse_scale_factor();
        Some(Vec2::new(size.x * logical, size.y * logical))
    }

    /// **Every minimizable window collapses and comes back.**
    ///
    /// The click is aimed at the button where the title bar's `SpaceBetween`
    /// put it, so a cluster squeezed out by a long title is caught here.
    #[test]
    fn every_minimizable_floater_collapses_and_restores() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            if !(floater.spec)().caps.minimizable {
                continue;
            }
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let content =
                chrome(&app, root, "floater-content").ok_or("the window has no content")?;
            let _spawn = drain::<FloaterCommand>(&mut app);

            let Some(button) = aim(&app, root, "floater-button:minimize") else {
                failures.push(format!(
                    "floater `{}`: minimizable, but its minimize button is nowhere a pointer can \
                     reach it",
                    floater.id
                ));
                continue;
            };
            interact::click(&mut app, button, MouseButton::Left);
            settle(&mut app);

            let emitted = ops(&mut app, root);
            if emitted != vec![FloaterOp::ToggleMinimize] {
                failures.push(format!(
                    "floater `{}`: the minimize button emitted {emitted:?}",
                    floater.id
                ));
            }
            if geometry(&app, root).is_none_or(|state| !state.minimized) {
                failures.push(format!(
                    "floater `{}`: the click did not minimize it",
                    floater.id
                ));
            }
            if app
                .world()
                .get::<Node>(content)
                .is_none_or(|node| node.display != Display::None)
            {
                failures.push(format!(
                    "floater `{}`: minimized, but its content is still in the layout",
                    floater.id
                ));
            }
            let violations = interaction_violations(&mut app, test.layout());
            if !violations.is_empty() {
                failures.push(format!(
                    "floater `{}` while minimized: {violations:#?}",
                    floater.id
                ));
            }

            // The strip keeps its buttons, so the same click restores it.
            let Some(button) = aim(&app, root, "floater-button:minimize") else {
                failures.push(format!(
                    "floater `{}`: minimized, and the restore button went with the content",
                    floater.id
                ));
                continue;
            };
            interact::click(&mut app, button, MouseButton::Left);
            settle(&mut app);
            if geometry(&app, root).is_none_or(|state| state.minimized) {
                failures.push(format!("floater `{}`: it would not restore", floater.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// **Every dockable window docks into its host and tears off again.**
    #[test]
    fn every_dockable_floater_docks_and_tears_off() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            if !(floater.spec)().caps.dockable {
                continue;
            }
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let host = find_by_name(&mut app, "floater-dock-host").ok_or("no dock host")?;
            let ui_root = app.world().resource::<UiRoot>().0;
            let _spawn = drain::<FloaterCommand>(&mut app);

            let Some(button) = aim(&app, root, "floater-button:dock") else {
                failures.push(format!(
                    "floater `{}`: dockable, but its dock button is nowhere a pointer can reach it",
                    floater.id
                ));
                continue;
            };
            interact::click(&mut app, button, MouseButton::Left);
            settle(&mut app);

            let emitted = ops(&mut app, root);
            if emitted != vec![FloaterOp::ToggleDock] {
                failures.push(format!(
                    "floater `{}`: the dock button emitted {emitted:?}",
                    floater.id
                ));
            }
            if geometry(&app, root).is_none_or(|state| !state.docked) {
                failures.push(format!(
                    "floater `{}`: the click did not dock it",
                    floater.id
                ));
            }
            if parent_of(&app, root) != Some(host) {
                failures.push(format!(
                    "floater `{}`: docked, but it is not in the host's tree",
                    floater.id
                ));
            }
            let violations = interaction_violations(&mut app, test.layout());
            if !violations.is_empty() {
                failures.push(format!(
                    "floater `{}` while docked: {violations:#?}",
                    floater.id
                ));
            }

            let Some(button) = aim(&app, root, "floater-button:dock") else {
                failures.push(format!(
                    "floater `{}`: docked, and the tear-off button went with the chrome",
                    floater.id
                ));
                continue;
            };
            interact::click(&mut app, button, MouseButton::Left);
            settle(&mut app);
            if geometry(&app, root).is_none_or(|state| state.docked) {
                failures.push(format!("floater `{}`: it would not tear off", floater.id));
            }
            if parent_of(&app, root) != Some(ui_root) {
                failures.push(format!(
                    "floater `{}`: torn off, but it did not return to the UI root",
                    floater.id
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// A node's parent.
    fn parent_of(app: &App, entity: Entity) -> Option<Entity> {
        app.world().get::<ChildOf>(entity).map(ChildOf::parent)
    }

    /// **Every closable window closes on its ×.**
    #[test]
    fn every_closable_floater_closes_on_its_button() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            if !(floater.spec)().caps.closable {
                continue;
            }
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let _spawn = drain::<FloaterCommand>(&mut app);
            if app
                .world()
                .get::<UiPanelShown>(root)
                .is_none_or(|shown| !shown.0)
            {
                failures.push(format!(
                    "floater `{}`: the registry spawned it closed, so this check would pass by \
                     closing nothing",
                    floater.id
                ));
                continue;
            }

            let Some(button) = aim(&app, root, "floater-button:close") else {
                failures.push(format!(
                    "floater `{}`: closable, but its × is nowhere a pointer can reach it",
                    floater.id
                ));
                continue;
            };
            interact::click(&mut app, button, MouseButton::Left);
            settle(&mut app);

            let emitted = ops(&mut app, root);
            if emitted != vec![FloaterOp::Close] {
                failures.push(format!(
                    "floater `{}`: the × emitted {emitted:?}",
                    floater.id
                ));
            }
            if app
                .world()
                .get::<UiPanelShown>(root)
                .is_none_or(|shown| shown.0)
            {
                failures.push(format!("floater `{}`: the × did not close it", floater.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// **A press anywhere raises the window and lights its title bar.**
    ///
    /// The press lands on the title bar, which is *not* one of the buttons: the
    /// raise comes from the window's own root observer, reached by the press
    /// bubbling up from whatever piece of chrome was actually hit.
    #[test]
    fn a_press_raises_and_highlights_every_floater() -> Result<(), TestError> {
        let test = InteractionTest::new();
        let mut failures = Vec::new();
        for floater in FLOATERS {
            let mut app = floater_app(test, floater);
            let root = root_of(&mut app, floater.id).ok_or("the window did not spawn")?;
            let title_bar =
                chrome(&app, root, "floater-title-bar").ok_or("the window has no title bar")?;
            let Some(at) = centre_of_entity(&app, title_bar) else {
                failures.push(format!(
                    "floater `{}`: the title bar never laid out",
                    floater.id
                ));
                continue;
            };
            interact::click(&mut app, at, MouseButton::Left);
            settle(&mut app);

            if !raised(&app, root) {
                failures.push(format!(
                    "floater `{}`: a press left it at the bottom of the z-order",
                    floater.id
                ));
            }
            let lit = app
                .world()
                .get::<BackgroundColor>(title_bar)
                .is_some_and(|colour| colour.0.alpha() > 0.0);
            if !lit {
                failures.push(format!(
                    "floater `{}`: pressed and raised, but its title bar is not highlighted as \
                     the active window",
                    floater.id
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// **The sweep has windows to sweep, and each conditional branch has at
    /// least one subject.**
    ///
    /// The anti-vacuity guard. Four of the checks above are skipped for a
    /// window that lacks the capability, so a registry where nothing were
    /// resizable would report a green resize sweep that had resized nothing.
    #[test]
    fn the_chrome_sweep_covers_every_capability() {
        assert!(!FLOATERS.is_empty(), "no floaters to sweep");
        // One assertion per branch rather than a table, so a failure names the
        // capability that lost its last subject.
        assert!(
            FLOATERS
                .iter()
                .any(|floater| (floater.spec)().caps.resizable),
            "no registered floater is resizable, so the two grip checks resize nothing"
        );
        assert!(
            FLOATERS
                .iter()
                .any(|floater| (floater.spec)().caps.minimizable),
            "no registered floater is minimizable, so that check minimizes nothing"
        );
        assert!(
            FLOATERS
                .iter()
                .any(|floater| (floater.spec)().caps.closable),
            "no registered floater is closable, so that check closes nothing"
        );
        assert!(
            FLOATERS
                .iter()
                .any(|floater| (floater.spec)().caps.dockable),
            "no registered floater is dockable, so that check docks nothing"
        );
    }
}
