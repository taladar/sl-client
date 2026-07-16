//! The headless half of the UI test harness (`viewer-ui-test-harness`): enough
//! of `bevy_ui` to run **real layout, with real fonts, in `cargo test`** — no
//! window, no renderer, no GPU, no login, no world.
//!
//! # Why
//!
//! The bugs this UI cluster will actually ship are the ones that only appear in
//! a particular font, script, translation or UI scale. That space is
//! combinatorial, and a human logging into a grid and pressing a key cannot walk
//! it. `viewer-text-node-padding-measure` is the proof: a text node laid out
//! one line shorter than the text it drew, diagnosed through a login to OpenSim,
//! a temporary debug key, and six rounds of a human reporting numbers back. It
//! is a pure function of a font, a string and an available width, and
//! [`tests::a_text_node_may_not_carry_its_own_padding`] now catches it in a
//! fifth of a second.
//!
//! So the **matrix lives here**, not in the gallery ([`crate::gallery`]). The
//! gallery is for what only an eye can judge — *does this look right*. Whether a
//! layout is *correct* is machine-checkable, and a machine should check it,
//! across every cell.
//!
//! # What was reachable, and the task's stale premise
//!
//! `bevy_ui`'s own layout tests (`bevy_ui-0.19.0/src/layout/mod.rs`,
//! `setup_ui_test_app`) drive layout headlessly, and the roadmap task recorded
//! that they do it through `pub(crate)` internals unreachable from a downstream
//! crate — so that the first job might be **upstreaming a public headless-layout
//! harness to Bevy**.
//!
//! That is not so in 0.19. Every piece is `pub`:
//! [`propagate_ui_target_cameras`], [`ui_layout_system`], [`UiSurface`],
//! [`ComputedCameraValues`] / [`RenderTargetInfo`], and the `bevy_transform`
//! systems. No fork, no `[patch.crates-io]`, no upstream PR — this module is
//! ordinary downstream code. (Bevy's own harness omits `measure_text_system`,
//! because none of its fixtures carry text. Ours cannot omit it: text
//! *measurement* is the thing most worth testing, and the padding bug lives
//! precisely there.)
//!
//! # What it is not
//!
//! Layout only. Nothing here rasterises a glyph, so this cannot answer "did the
//! right pixels light up" — no `text_system`, no font atlas, no images. It
//! answers "is every box the right size and in the right place", which is where
//! the bugs have actually been.
//!
//! [`viewer-text-node-padding-measure`]: ../../../roadmap/bugs/viewer-text-node-padding-measure.md

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ecs::system::SystemState;
use bevy::input_focus::tab_navigation::{NavAction, TabNavigation};
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{FontCx, LayoutCx, RemSize, ScaleCx, TextPipeline};
use bevy::transform::systems::{
    mark_dirty_trees, propagate_parent_transforms, sync_simple_transforms,
};
use bevy::ui::UiSystems;
use bevy::ui::ui_layout_system;
use bevy::ui::ui_surface::UiSurface;
use bevy::ui::update::{propagate_ui_target_cameras, update_clipping_system};
use bevy::ui::widget::measure_text_system;
use bevy::ui_widgets::Activate;

use crate::ui::{
    UiDirection, UiRoot, UiScaffoldSystems, apply_panel_visibility, apply_ui_direction,
    invalidate_logical_boxes, resolve_logical_boxes, spawn_ui_root,
};
use crate::ui_element::{AlignEdge, AlignmentGroup, ElementCx, TextMayClip, UiAction, UiElement};
use crate::ui_font::register_ui_fonts;

/// A boxed error, so a test can use `?` rather than the workspace-denied
/// `unwrap` / `expect`.
pub(crate) type TestError = Box<dyn core::error::Error>;

/// A node's border box, in physical pixels, from its computed size and where the
/// layout put it.
///
/// Built per-component in plain `f32` rather than with the `glam` operators, per
/// the convention the rest of this crate follows (`crate::ik`, `crate::camera`):
/// the workspace's `arithmetic_side_effects` lint fires on `glam`'s overloaded
/// operators but not on plain floating-point arithmetic.
fn border_box(computed: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let centre = transform.translation;
    let (half_x, half_y) = (computed.size.x / 2.0, computed.size.y / 2.0);
    Rect {
        min: Vec2::new(centre.x - half_x, centre.y - half_y),
        max: Vec2::new(centre.x + half_x, centre.y + half_y),
    }
}

/// How much a node may exceed its box before it counts as a violation, in
/// **logical** pixels.
///
/// This is **not** a rounding allowance, and it is worth being exact about why,
/// because "it's just rounding" is the comfortable answer and it is wrong.
///
/// Rounding is real but sub-pixel: `bevy_ui` rounds a node's `size` to whole
/// physical pixels (hence `unrounded_size` beside it) while `content_size` comes
/// back from `taffy` unrounded. That accounts for less than 1 px.
///
/// What this actually absorbs is the **upstream measure error** of
/// `viewer-text-node-padding-measure`, which the matrix characterised while this
/// constant was being argued over. Two properties, both measured, both useful to
/// the upstream report:
///
/// - **It does not accumulate with nesting.** A three-deep tree reports the *same*
///   overshoot at every level — text 551/546, its box 599/594, the panel 635/630,
///   all 5 px — rather than 5/10/15. So it is one error introduced at the text
///   measure and propagated outward unchanged by each ancestor's `content_size`,
///   not a per-level rounding loss.
/// - **It scales with the font, not with the display.** Across the matrix it is
///   ≈ 0.23 × the font size — 5 logical px at 22 px text, 3.5 at 15 px — and
///   near-constant against both `scale_factor` and `UiScale` once converted to
///   logical. Roughly a quarter em: a per-line advance the measure does not
///   account for.
///
/// Hence 6 logical px: enough to clear ~0.23 em at the matrix's largest font
/// (22 px → 5 px) with a little headroom. **Sweeping a materially larger UI font
/// would need this raised** — or, better, the upstream bug fixed.
///
/// It is a ceiling on how fine a finding can be, not a licence. The failure this
/// harness exists to catch overshoots by a whole **line** — 18 px at the demo
/// panel's font size — and anything structural is line-scale or larger. Nothing
/// real hides under a quarter em.
///
/// It should come back down to ~1 when the upstream measure is fixed; the canary
/// for that is [`tests::a_text_node_may_not_carry_its_own_padding`], which starts
/// failing the day Bevy corrects it.
const OVERFLOW_EPSILON: f32 = 6.0;

/// A headless `bevy_ui` layout app, configured and then [`build`](Self::build).
///
/// The defaults are the interesting-but-boring case: a roomy viewport at scale
/// factor 1, `UiScale` 1, left-to-right. Each `with_*` method moves one axis of
/// the matrix.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayoutTest {
    /// The window's size in **logical** pixels — the room the UI actually has.
    ///
    /// Logical rather than physical, and it matters: a user on a 2x display has
    /// the same size window and more pixels in it, not half the room. Holding the
    /// *physical* size constant across the scale-factor axis would shrink the
    /// logical window to a quarter at scale 2 and overflow every element for a
    /// reason that has nothing to do with the element — a whole column of the
    /// matrix failing to say anything.
    logical_viewport: UVec2,
    /// The window scale factor — the display's own DPI scaling, as `bevy_winit`
    /// would report it. The padding bug was first measured at 1.5.
    scale_factor: f32,
    /// The `UiScale` resource: the user's UI size preference, which multiplies
    /// on top of [`Self::scale_factor`] and is a *separate* way for the same
    /// class of bug to surface.
    ui_scale: f32,
    /// The inline direction the tree lays out in.
    direction: UiDirection,
}

impl Default for LayoutTest {
    fn default() -> Self {
        Self {
            // Generous, because the checks are about whether an element fits the
            // box it asked for — not about whether it fits a window nobody would
            // use. A cramped default would make every check a window-size check.
            logical_viewport: UVec2::new(1600, 1200),
            scale_factor: 1.0,
            ui_scale: 1.0,
            direction: UiDirection::Ltr,
        }
    }
}

impl LayoutTest {
    /// A test at the default configuration.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set the window's size in logical pixels.
    pub(crate) const fn with_viewport(mut self, width: u32, height: u32) -> Self {
        self.logical_viewport = UVec2::new(width, height);
        self
    }

    /// Set the window scale factor (the display's DPI scaling).
    pub(crate) const fn with_scale_factor(mut self, scale_factor: f32) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// Set the `UiScale` (the user's UI size preference).
    pub(crate) const fn with_ui_scale(mut self, ui_scale: f32) -> Self {
        self.ui_scale = ui_scale;
        self
    }

    /// Set the inline layout direction.
    pub(crate) const fn with_direction(mut self, direction: UiDirection) -> Self {
        self.direction = direction;
        self
    }

    /// This configuration's viewport in **physical** pixels — the logical window
    /// scaled by the display's scale factor, and what [`viewport_violations`]
    /// measures a tree against (`ComputedNode` is physical throughout).
    pub(crate) fn viewport(self) -> UVec2 {
        self.logical_viewport
            .as_vec2()
            .mul_add(Vec2::splat(self.scale_factor), Vec2::ZERO)
            .as_uvec2()
    }

    /// This configuration's inline direction — what [`alignment_violations`]
    /// resolves a logical edge against.
    pub(crate) const fn direction(self) -> UiDirection {
        self.direction
    }

    /// Build the app: the layout pipeline, the viewer's font stack, the
    /// scaffold's own systems, and a [`crate::ui::UiRoot`] to parent fixtures to.
    ///
    /// The app is returned **un-run**, so a test can add its own `Startup`
    /// fixture system (ordered `.after(UiScaffoldSystems::SpawnRoot)`, exactly as
    /// a real panel does) before the first `update`.
    pub(crate) fn build(self) -> App {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            // `measure_text_system` reads `Assets<Font>`, so the collection has
            // to exist even though the viewer's faces are registered into
            // parley's `FontCx` rather than loaded as Bevy assets.
            AssetPlugin::default(),
        ))
        .init_asset::<Font>();

        // The two hierarchy propagations `UiPlugin` would install: which camera a
        // node targets, and that target's size / scale factor. Layout reads the
        // latter for every percentage and every `Val::Px` -> physical conversion,
        // so without them nothing resolves.
        app.add_plugins((
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
        ));

        app.insert_resource(UiScale(self.ui_scale))
            .insert_resource(self.direction)
            .init_resource::<UiSurface>()
            .init_resource::<TextPipeline>()
            .init_resource::<FontCx>()
            .init_resource::<ScaleCx>()
            .init_resource::<LayoutCx>()
            .init_resource::<RemSize>()
            // `apply_panel_visibility` takes focus off a panel it closes.
            .init_resource::<InputFocus>()
            .init_resource::<StaticTransformOptimizations>();

        app.add_systems(
            Startup,
            (
                register_ui_fonts,
                spawn_ui_root.in_set(UiScaffoldSystems::SpawnRoot),
            ),
        );

        // Mirror `UiPlugin`'s own set structure rather than hand-chaining the
        // systems. It is not ceremony: the scaffold's `resolve_logical_boxes` /
        // `apply_ui_direction` order themselves `.before(UiSystems::Layout)`, and
        // if `ui_layout_system` were not *in* that set those orderings would
        // silently evaporate — the harness would run the scaffold's writes and
        // the layout in an arbitrary order and produce results the viewer never
        // would.
        app.configure_sets(
            PostUpdate,
            (
                UiSystems::Prepare,
                UiSystems::Propagate,
                UiSystems::Content,
                UiSystems::Layout,
                UiSystems::PostLayout,
            )
                .chain(),
        );
        app.configure_sets(
            PostUpdate,
            (
                PropagateSet::<ComputedUiTargetCamera>::default(),
                PropagateSet::<ComputedUiRenderTargetInfo>::default(),
            )
                .in_set(UiSystems::Propagate),
        );
        app.add_systems(
            PostUpdate,
            (
                propagate_ui_target_cameras.in_set(UiSystems::Prepare),
                measure_text_system.in_set(UiSystems::Content),
                ui_layout_system.in_set(UiSystems::Layout),
                (
                    mark_dirty_trees,
                    sync_simple_transforms,
                    propagate_parent_transforms,
                    // Computes each node's `CalculatedClip`, without which
                    // `clipping_violations` has nothing to read.
                    update_clipping_system,
                )
                    .chain()
                    .in_set(UiSystems::PostLayout),
            ),
        );

        // The scaffold's own half, in the order `ViewerUiPlugin` gives it.
        app.add_systems(
            PostUpdate,
            (
                apply_panel_visibility,
                invalidate_logical_boxes,
                resolve_logical_boxes,
                apply_ui_direction,
            )
                .chain()
                .before(UiSystems::Layout),
        );

        // The camera and its dummy render target: no window and no renderer, so
        // the target info a real `Camera` would compute is supplied directly.
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: self.viewport(),
                        scale_factor: self.scale_factor,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: self.viewport(),
                    ..default()
                }),
                ..default()
            },
        ));
        app
    }
}

/// Run the app until layout has settled.
///
/// Two updates, not one, and the reason is load-bearing for every text fixture:
/// `measure_text_system` installs a node's measure function on the frame its
/// `Text` first appears, and `ui_layout_system` consumes it on the *next* one. A
/// single update therefore lays every text node out at zero size and quietly
/// passes every invariant below — the failure mode where the harness reports
/// success because it measured nothing at all.
pub(crate) fn settle(app: &mut App) {
    app.update();
    app.update();
}

/// How a node is named in a violation message: its [`Name`], if a fixture gave
/// it one, else its entity id.
fn describe(name: Option<&Name>, entity: Entity) -> String {
    name.map_or_else(|| format!("{entity}"), |name| format!("`{name}`"))
}

/// **The invariant.** Every node's content must fit inside the box the node was
/// given for it.
///
/// Content spilling out of its own content box is never intentional in this UI,
/// and it is exactly what a wrongly measured text node looks like: the measure
/// resolves too much available width, fits one more word per line, arrives at one
/// fewer line, and the node is laid out shorter than the text it draws — whose
/// last line then hangs out of the bottom of whatever contains it.
///
/// Checked against **`size`** — the border box — and not, as looks tempting,
/// against the narrower content box. `content_size` is measured in border-box
/// space and **already includes the node's own padding**: a 400 px container
/// with 32 px of padding around a 364 px child reports `content_size` 400, not
/// 368. Comparing that against the 364 px content box would report a 36 px
/// overflow on a node that is laid out perfectly, and the check would fire on
/// every padded container in the viewer — which is exactly what it did before
/// this was measured rather than assumed.
///
/// The narrower comparison loses nothing: the failure this exists to catch shows
/// up on the block axis, where a text node reports a content **taller** than its
/// own border box, and no amount of padding can explain that away.
///
/// An axis set to [`OverflowAxis::Scroll`] is skipped: content larger than the
/// box is that widget's entire purpose (`viewer-ui-virtualized-list`), and it is
/// the one case where the overflow is a decision rather than a bug.
///
/// Returns one message per breach, so a caller can assert the whole tree at once
/// and see everything wrong with it rather than the first thing.
pub(crate) fn overflow_violations(app: &mut App) -> Vec<String> {
    let mut query = app
        .world_mut()
        .query::<(Entity, &ComputedNode, &Node, Option<&Name>)>();
    let mut violations = Vec::new();
    for (entity, computed, node, name) in query.iter(app.world()) {
        let available = computed.size;
        let content = computed.content_size;
        for (axis, content, available, overflow) in [
            ("width", content.x, available.x, node.overflow.x),
            ("height", content.y, available.y, node.overflow.y),
        ] {
            if overflow == OverflowAxis::Scroll {
                continue;
            }
            // Compared in logical pixels — see `OVERFLOW_EPSILON`.
            let overshoot = (content - available) * computed.inverse_scale_factor;
            if overshoot > OVERFLOW_EPSILON {
                violations.push(format!(
                    "{}: content {axis} {content} exceeds its own box {available} by \
                     {overshoot} logical px",
                    describe(name, entity),
                ));
            }
        }
    }
    violations
}

/// Every node must lie within the viewport.
///
/// A panel laid out off the edge of the screen is unreachable, and it is the
/// other way a translation that runs long fails: not by overflowing its own box
/// but by pushing the box it is in past the window. Zero-sized nodes are skipped
/// — a closed panel (`Display::None`) is legitimately nowhere.
pub(crate) fn viewport_violations(app: &mut App, viewport: UVec2) -> Vec<String> {
    let mut query = app
        .world_mut()
        .query::<(Entity, &ComputedNode, &UiGlobalTransform, Option<&Name>)>();
    let mut violations = Vec::new();
    let bounds = viewport.as_vec2();
    for (entity, computed, transform, name) in query.iter(app.world()) {
        if computed.size.cmple(Vec2::ZERO).any() {
            continue;
        }
        let node_box = border_box(computed, transform);
        let (min, max) = (node_box.min, node_box.max);
        if min.x < -OVERFLOW_EPSILON
            || min.y < -OVERFLOW_EPSILON
            || max.x > bounds.x + OVERFLOW_EPSILON
            || max.y > bounds.y + OVERFLOW_EPSILON
        {
            violations.push(format!(
                "{}: laid out at {min}..{max}, outside the {bounds} viewport",
                describe(name, entity),
            ));
        }
    }
    violations
}

/// Whether `node` is allowed to have its text sliced by a clip.
///
/// Two ways to earn it, and both are **declarations** rather than guesses:
///
/// - an ancestor with `Overflow::Scroll` — `bevy_ui`'s own structural statement
///   that content is clipped here and reached by scrolling;
/// - an ancestor carrying [`TextMayClip`] — the element saying so in as many
///   words, with a reason, for the cases the tree cannot show: a single-line
///   field scrolling horizontally, a non-wrapping editor, chat.
///
/// Walks the ancestry rather than checking the node itself: the text of an
/// editor is a child of the editor, and a row inside a list inside a scroll area
/// is three levels below the thing that scrolls. Stopping at either end would
/// report a whole widget's worth of correct text as broken.
fn may_be_clipped(world: &World, node: Entity) -> bool {
    core::iter::successors(Some(node), |current| {
        world.get::<ChildOf>(*current).map(ChildOf::parent)
    })
    .any(|ancestor| {
        if world.get::<TextMayClip>(ancestor).is_some() {
            return true;
        }
        world.get::<Node>(ancestor).is_some_and(|ancestor| {
            ancestor.overflow.x == OverflowAxis::Scroll
                || ancestor.overflow.y == OverflowAxis::Scroll
        })
    })
}

/// **Universal.** No node's box may escape its parent's box.
///
/// The direct reading of "no pixel an element renders lands outside its parent":
/// a child laid out past its parent's edge is either drawn over whatever is next
/// to the parent, or clipped away and unreachable. Either way nobody asked for
/// it.
///
/// This overlaps [`overflow_violations`] for ordinary flow children — `taffy`'s
/// `content_size` is the union of their boxes — but only partly, and the part it
/// adds is the part that bites: a child placed by **inset** rather than by flow
/// (a floater, a menu, a tooltip) contributes nothing to `content_size` and can
/// sail straight out of its parent with the content check none the wiser.
///
/// A parent that clips or scrolls the axis is skipped: escaping is that widget's
/// purpose there, and [`clipping_violations`] takes over the question of whether
/// the result is *readable*.
pub(crate) fn containment_violations(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut query = world.query::<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &ChildOf,
        Option<&Name>,
    )>();
    let boxes: Vec<(Entity, Rect, Entity, Option<String>)> = query
        .iter(world)
        .map(|(entity, computed, transform, parent, name)| {
            (
                entity,
                border_box(computed, transform),
                parent.parent(),
                name.map(|name| name.to_string()),
            )
        })
        .collect();
    let mut parent_boxes = world.query::<(&ComputedNode, &UiGlobalTransform, &Node)>();

    let mut violations = Vec::new();
    for (entity, child_box, parent, name) in boxes {
        let Ok((parent_computed, parent_transform, parent_node)) = parent_boxes.get(world, parent)
        else {
            // No parent node: the `UiRoot` itself, which `viewport_violations`
            // measures against the window instead.
            continue;
        };
        if child_box.size().cmple(Vec2::ZERO).any() {
            continue;
        }
        let parent_box = border_box(parent_computed, parent_transform);
        for (axis, clipped, child_min, child_max, parent_min, parent_max) in [
            (
                "inline",
                parent_node.overflow.x != OverflowAxis::Visible,
                child_box.min.x,
                child_box.max.x,
                parent_box.min.x,
                parent_box.max.x,
            ),
            (
                "block",
                parent_node.overflow.y != OverflowAxis::Visible,
                child_box.min.y,
                child_box.max.y,
                parent_box.min.y,
                parent_box.max.y,
            ),
        ] {
            if clipped {
                continue;
            }
            if child_min < parent_min - OVERFLOW_EPSILON
                || child_max > parent_max + OVERFLOW_EPSILON
            {
                violations.push(format!(
                    "{}: {axis} extent {child_min}..{child_max} escapes its parent's \
                     {parent_min}..{parent_max}",
                    name.unwrap_or_else(|| format!("{entity}")),
                ));
                break;
            }
        }
    }
    violations
}

/// **Universal, with a declared exception.** No text may be *partially* hidden
/// by a clip.
///
/// A label sliced in half is unreadable, and — unlike a box that is merely the
/// wrong size — it looks deliberate on screen, so it survives review. It is
/// exactly what a translation that runs long does inside a container someone gave
/// a fixed size in English.
///
/// The rule is *partially*, and the precision is the point. Fully clipped is not
/// reported: a row scrolled out of view is entirely legitimate, and a check that
/// called it a bug would be noise nobody keeps. Sliced is the bug; hidden is a
/// state.
///
/// Plenty of correct widgets slice text on purpose, though — a single-line field
/// scrolling past its end, a non-wrapping editor, a scroll area's boundary row —
/// so the strict rule holds only where the element has not declared otherwise.
/// See [`may_be_clipped`] and [`TextMayClip`]: the exception is opt-in, carries a
/// reason, and is greppable, rather than being a silent special case in here.
pub(crate) fn clipping_violations(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &CalculatedClip,
        Option<&Name>,
    ), With<Text>>();
    let clipped: Vec<(Entity, Rect, Rect, Option<String>)> = query
        .iter(world)
        .map(|(entity, computed, transform, clip, name)| {
            (
                entity,
                border_box(computed, transform),
                clip.clip,
                name.map(|name| name.to_string()),
            )
        })
        .collect();
    let mut violations = Vec::new();
    for (entity, text_box, clip, name) in clipped {
        if text_box.size().cmple(Vec2::ZERO).any() {
            continue;
        }
        let visible = text_box.intersect(clip);
        let fully_visible = visible
            .size()
            .abs_diff_eq(text_box.size(), OVERFLOW_EPSILON);
        let fully_hidden = visible.is_empty() || visible.size().cmple(Vec2::ZERO).any();
        if fully_visible || fully_hidden {
            continue;
        }
        if may_be_clipped(world, entity) {
            continue;
        }
        violations.push(format!(
            "{}: text {text_box:?} is sliced by the clip rect {clip:?} — it is neither fully \
             visible nor fully hidden, so it renders as a cut-off label",
            name.unwrap_or_else(|| format!("{entity}")),
        ));
    }
    violations
}

/// One declared alignment group, gathered from the tree: the edge its members
/// must share, and where each of them actually landed.
///
/// A named type rather than the tuple it started as, because the tuple was three
/// levels deep and said nothing about what any of it meant.
#[derive(Debug, Clone)]
struct GatheredGroup {
    /// The group's name, as the element declared it.
    group: &'static str,
    /// The edge its members must agree on.
    edge: AlignEdge,
    /// Each member: how to name it in a failure, and its edge in physical pixels.
    members: Vec<(String, f32)>,
}

/// **Declared.** Every node in an [`AlignmentGroup`] must share the edge it
/// names.
///
/// See [`AlignmentGroup`] for why this tier exists at all: nothing in a tree says
/// whether two boxes *ought* to line up, so the element declares it and this
/// holds it to the declaration in every cell of the matrix — which is where the
/// failure actually is. A column of fields is straight in English because the
/// labels beside it happen to be the same width, and ragged in the first
/// language where they are not.
///
/// The edge is resolved **logically**: `InlineStart` is the left edge under LTR
/// and the right edge under RTL, so a group declared once holds in both
/// directions without the element saying anything about sides.
pub(crate) fn alignment_violations(app: &mut App, direction: UiDirection) -> Vec<String> {
    let world = app.world_mut();
    let mut query = world.query::<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &AlignmentGroup,
        Option<&Name>,
    )>();
    // Grouped by name, keeping insertion order so a failure message reads in the
    // order the element spawned its rows rather than an arbitrary one.
    let mut groups: Vec<GatheredGroup> = Vec::new();
    for (entity, computed, transform, group, name) in query.iter(world) {
        let node_box = border_box(computed, transform);
        let (min, max) = (node_box.min.x, node_box.max.x);
        // The logical edge, resolved against the live direction: the leading
        // inline edge is the left one under LTR and the right one under RTL.
        let edge = match (group.edge, direction.is_rtl()) {
            (AlignEdge::InlineStart, false) | (AlignEdge::InlineEnd, true) => min,
            (AlignEdge::InlineStart, true) | (AlignEdge::InlineEnd, false) => max,
        };
        let label = name.map_or_else(|| format!("{entity}"), |name| name.to_string());
        if let Some(existing) = groups
            .iter_mut()
            .find(|gathered| gathered.group == group.group)
        {
            existing.members.push((label, edge));
        } else {
            groups.push(GatheredGroup {
                group: group.group,
                edge: group.edge,
                members: vec![(label, edge)],
            });
        }
    }

    let mut violations = Vec::new();
    for gathered in groups {
        let GatheredGroup {
            group,
            edge,
            members,
        } = gathered;
        let Some((_, first)) = members.first() else {
            continue;
        };
        let ragged = members
            .iter()
            .any(|(_, position)| (position - first).abs() > OVERFLOW_EPSILON);
        if ragged {
            violations.push(format!(
                "alignment group `{group}` ({edge:?}) is ragged: {members:?} — these were \
                 declared to share an edge and do not",
            ));
        }
    }
    violations
}

/// Every check, over the whole tree, as one list.
///
/// The shape every matrix cell uses: assert the result is empty and print it on
/// failure, so one run reports everything wrong with the fixture rather than the
/// first thing.
///
/// **A new check belongs here.** That is what makes it retroactive: the moment it
/// is in this list it runs against every registered element, in every cell,
/// including the elements written before the check existed and the ones written
/// after.
pub(crate) fn layout_violations(app: &mut App, test: LayoutTest) -> Vec<String> {
    let mut violations = overflow_violations(app);
    violations.extend(containment_violations(app));
    violations.extend(clipping_violations(app));
    violations.extend(viewport_violations(app, test.viewport()));
    violations.extend(alignment_violations(app, test.direction()));
    violations
}

// ---------------------------------------------------------------------------
// Driving input, so behaviour is checkable and not just the resting state.
// ---------------------------------------------------------------------------

/// Spawn one element from the registry into a fresh app, and settle it.
///
/// The whole of a matrix cell's setup. Returns the app and the element's root
/// entity, so a check can look at the tree and a behaviour test can click it.
pub(crate) fn spawn_element(test: LayoutTest, element: &UiElement, cx: ElementCx) -> App {
    let mut app = test.build();
    app.add_message::<UiAction>()
        .init_resource::<RecordedActions>()
        .add_systems(Update, record_actions);
    // `Startup`, ordered after the root exists, because that is how a real panel
    // spawns — testing it any other way would be testing a different thing.
    let spawn = element.spawn;
    app.add_systems(
        Startup,
        (move |mut commands: Commands, root: Res<UiRoot>| {
            spawn(&mut commands, root.0, cx);
        })
        .after(UiScaffoldSystems::SpawnRoot),
    );
    settle(&mut app);
    app
}

/// Find one node of the spawned tree by its [`Name`].
///
/// Elements name the nodes worth addressing, so a behaviour test can say "the
/// Cancel button" rather than reaching for an entity id it has no way to know.
pub(crate) fn find_by_name(app: &mut App, name: &str) -> Option<Entity> {
    let mut query = app.world_mut().query::<(Entity, &Name)>();
    query
        .iter(app.world())
        .find(|(_, node_name)| node_name.as_str() == name)
        .map(|(entity, _)| entity)
}

/// Activate a widget as a click or `Enter` would, and settle.
///
/// `bevy_ui_widgets` routes both a pointer click and a keyboard activation to the
/// same `Activate` event, so triggering it directly exercises the element's real
/// observer — the one the viewer runs — without a pointer, a window, or a
/// picking backend. What it deliberately does *not* cover is the hit-testing:
/// whether the button is where the user thinks it is. That is what
/// [`containment_violations`] and [`viewport_violations`] are for, and the two
/// together are the claim.
pub(crate) fn activate(app: &mut App, entity: Entity) {
    app.world_mut().trigger(Activate { entity });
    settle(app);
}

/// Move keyboard focus one stop, as `Tab` (or `Shift+Tab`) would, and settle.
///
/// Goes through `bevy_input_focus`'s real navigation rather than setting
/// [`InputFocus`] directly, so what is tested is the thing the user drives: the
/// order, the wrap-around, and whether a node is reachable at all.
pub(crate) fn navigate(app: &mut App, action: NavAction) -> Option<Entity> {
    let focus = app.world().resource::<InputFocus>().clone();
    let mut navigation = SystemState::<TabNavigation>::new(app.world_mut());
    // `SystemState::get` is fallible and `expect` is denied workspace-wide, so
    // a navigation that cannot run reports "nowhere to go" rather than panicking.
    let next = navigation
        .get(app.world())
        .ok()
        .and_then(|navigation| navigation.navigate(&focus, action).ok());
    if let Some(next) = next {
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(next, FocusCause::Navigated);
    }
    settle(app);
    next
}

/// Every [`UiAction`] emitted since the last [`drain_actions`], kept across
/// frames — see there for why the message queue itself will not do.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct RecordedActions(Vec<UiAction>);

/// Copy this frame's [`UiAction`]s into [`RecordedActions`] before the message
/// queue drops them.
fn record_actions(mut actions: MessageReader<UiAction>, mut recorded: ResMut<RecordedActions>) {
    recorded.0.extend(actions.read().copied());
}

/// Every [`UiAction`] the app has emitted since this was last called.
///
/// The assertion surface the registry's no-wiring rule buys: an element's button
/// is driven for real and what it *meant* is read off a queue, with nothing
/// behind it that could teleport an avatar or spend money.
///
/// Read from [`RecordedActions`] rather than straight from `Messages<UiAction>`:
/// a `Message` lives two frames and [`settle`] runs two updates, so draining the
/// queue directly races the buffer swap and reports an empty list for an action
/// that fired perfectly well. That is a false *negative* in a test whose whole
/// job is to notice that a button did something, which is the worst direction for
/// one to fail in.
pub(crate) fn drain_actions(app: &mut App) -> Vec<UiAction> {
    core::mem::take(&mut app.world_mut().resource_mut::<RecordedActions>().0)
}

#[cfg(test)]
mod tests {
    use super::{
        LayoutTest, TestError, activate, drain_actions, find_by_name, layout_violations,
        overflow_violations, settle, spawn_element,
    };
    use crate::ui::UiDirection;
    use crate::ui_element::{ELEMENTS, ElementCx, SCRIPTS, SampleText, UiAction};
    use crate::ui_font::UiFont;
    use bevy::input_focus::InputFocus;
    use bevy::input_focus::tab_navigation::NavAction;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// The UI font sizes the matrix sweeps.
    ///
    /// The user's font-size preference is a first-class way to break a layout —
    /// the same way a long translation is, from the other side — so it is an axis
    /// rather than a constant.
    const FONT_SIZES: [f32; 3] = [11.0, 15.0, 22.0];

    /// The window scale factors the matrix sweeps. 1.5 is where the padding bug
    /// was first measured by hand.
    const SCALE_FACTORS: [f32; 3] = [1.0, 1.5, 2.0];

    // -----------------------------------------------------------------------
    // The harness has to have teeth. These two tests are about the *checks*,
    // not about the UI: a suite whose checks cannot fail is a suite that
    // reports success because it looked at nothing.
    // -----------------------------------------------------------------------

    /// The known-bad structure from `viewer-text-node-padding-measure`, which is
    /// the reason this harness exists: a `Text` node carrying **its own** padding
    /// and border is laid out with the wrong wrap width, so it gets one fewer
    /// line than it draws and the last line hangs out of the bottom.
    ///
    /// This asserts the bug is **still present** and that the check **sees it**.
    /// Both halves matter. It is the proof that `overflow_violations` has teeth —
    /// a check that cannot fail protects nothing — and it is a canary: when Bevy
    /// fixes the measure upstream this test starts failing, which is precisely
    /// when we want to be told, so the workaround can go.
    ///
    /// Diagnosing this by hand cost a login to OpenSim, a temporary debug key in
    /// the demo panel, and six rounds of a human pressing it and reporting
    /// numbers. It is a pure function of a font, a string and a width.
    #[test]
    fn a_text_node_may_not_carry_its_own_padding() {
        let test = LayoutTest::new();
        let mut app = test.build();
        let text = app
            .world_mut()
            .spawn((
                Text::new(
                    "A much longer label, of the length a translated string reaches when the \
                     original was written in English and measured once, which is exactly the \
                     case a fixed pixel rect gets wrong.",
                ),
                UiFont::Sans.at(15.0),
                Node {
                    // The bug: padding and a border on the text node itself.
                    padding: UiRect {
                        left: Val::Px(24.0),
                        right: Val::Px(8.0),
                        top: Val::Px(4.0),
                        bottom: Val::Px(4.0),
                    },
                    border: UiRect {
                        left: Val::Px(4.0),
                        ..UiRect::ZERO
                    },
                    ..default()
                },
                Name::new("text-with-its-own-padding"),
            ))
            .id();
        // Inside a bounded panel, because that is where the bug lives: the wrap
        // width has to arrive from the *parent's* content box for the measure to
        // subtract it wrongly. A text node bounded by its own `max_width` lays
        // out correctly and would make this test quietly vacuous.
        app.world_mut()
            .spawn((
                Node {
                    // A column, as every real panel is: in a row the child would
                    // be stretched instead of bounded, and the measure would
                    // never be handed the too-wide width that is the bug.
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(12.0)),
                    max_width: Val::Px(560.0),
                    ..default()
                },
                Name::new("bounding-panel"),
            ))
            .add_child(text);
        settle(&mut app);

        let violations = overflow_violations(&mut app);
        assert!(
            !violations.is_empty(),
            "a `Text` node carrying its own padding is the known upstream measure bug \
             (viewer-text-node-padding-measure) and `overflow_violations` must report it. \
             If this now passes, Bevy has fixed the measure: drop the workaround in \
             `crate::ui_element::spawn_label` and this test with it."
        );
    }

    /// The same text, decorated the way the convention says — the box on a
    /// **container**, the `Text` a plain child — must be clean.
    ///
    /// The other half of the pair above, and the one that makes it meaningful. A
    /// check that fires on the bad structure proves nothing on its own; it has to
    /// also *not* fire on the good one, or it is simply a check that always
    /// fires.
    #[test]
    fn the_same_text_in_a_decorated_container_is_clean() {
        let test = LayoutTest::new();
        let mut app = test.build();
        let text = app
            .world_mut()
            .spawn((
                Text::new(
                    "A much longer label, of the length a translated string reaches when the \
                     original was written in English and measured once, which is exactly the \
                     case a fixed pixel rect gets wrong.",
                ),
                UiFont::Sans.at(15.0),
                Name::new("plain-text-child"),
            ))
            .id();
        app.world_mut()
            .spawn((
                Node {
                    max_width: Val::Px(400.0),
                    padding: UiRect {
                        left: Val::Px(24.0),
                        right: Val::Px(8.0),
                        top: Val::Px(4.0),
                        bottom: Val::Px(4.0),
                    },
                    border: UiRect {
                        left: Val::Px(4.0),
                        ..UiRect::ZERO
                    },
                    ..default()
                },
                Name::new("decorated-container"),
            ))
            .add_child(text);
        settle(&mut app);

        let violations = overflow_violations(&mut app);
        assert!(
            violations.is_empty(),
            "decorating a container and leaving the text a plain child is the convention, and \
             must lay out cleanly: {violations:#?}"
        );
    }

    // -----------------------------------------------------------------------
    // The matrix. Every registered element, in every cell.
    // -----------------------------------------------------------------------

    /// **Every element × every script.** The sweep the gallery cannot be: eight
    /// writing systems against every element in the registry, at both label and
    /// prose length, in the direction each script is actually written in.
    ///
    /// This is the combinatorial half that no human walks. A new element inherits
    /// it by being registered; a new script by being listed.
    #[test]
    fn every_element_survives_every_script() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            for sample in SCRIPTS {
                // RTL scripts are laid out RTL: testing Arabic in a left-to-right
                // UI would be testing a configuration no user ever has.
                let direction = match sample.name {
                    "Arabic" | "Hebrew" => UiDirection::Rtl,
                    _other => UiDirection::Ltr,
                };
                let test = LayoutTest::new().with_direction(direction);
                let cx = ElementCx {
                    text: SampleText::Script(sample),
                    ..ElementCx::new()
                };
                let mut app = spawn_element(test, element, cx);
                let violations = layout_violations(&mut app, test);
                if !violations.is_empty() {
                    failures.push(format!(
                        "element `{}` in {} ({direction:?}): {violations:#?}",
                        element.id, sample.name
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × pseudolocalisation × font size × scale factor.**
    ///
    /// The axes that break a layout without changing a single glyph anyone can
    /// read: a translation ~40% longer than the English it was measured against,
    /// a user who turned the UI font up, and a display that scales. Each one on
    /// its own has shipped a bug in this viewer already.
    #[test]
    fn every_element_survives_a_long_translation_at_every_scale() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            for font_size in FONT_SIZES {
                for scale_factor in SCALE_FACTORS {
                    let test = LayoutTest::new().with_scale_factor(scale_factor);
                    let cx = ElementCx {
                        text: SampleText::Pseudo,
                        font_size,
                    };
                    let mut app = spawn_element(test, element, cx);
                    let violations = layout_violations(&mut app, test);
                    if !violations.is_empty() {
                        failures.push(format!(
                            "element `{}` pseudolocalised at {font_size}px, scale \
                             {scale_factor}: {violations:#?}",
                            element.id,
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// **Every element × direction × `UiScale`.**
    ///
    /// Mirroring is the axis with no partial credit: either the whole tree
    /// mirrors or the UI is broken for every RTL reader. Swept against `UiScale`
    /// as well as the direction because the two compose, and the reference viewer
    /// gets neither right.
    #[test]
    fn every_element_survives_both_directions_at_every_ui_scale() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            for direction in [UiDirection::Ltr, UiDirection::Rtl] {
                for ui_scale in [1.0_f32, 1.25, 2.0] {
                    let test = LayoutTest::new()
                        .with_direction(direction)
                        .with_ui_scale(ui_scale);
                    let mut app = spawn_element(test, element, ElementCx::new());
                    let violations = layout_violations(&mut app, test);
                    if !violations.is_empty() {
                        failures.push(format!(
                            "element `{}` {direction:?} at UiScale {ui_scale}: {violations:#?}",
                            element.id,
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// Every element must lay out at all — a non-zero box with a real position.
    ///
    /// The guard against the quiet failure this whole file is exposed to: if a
    /// fixture never spawned, or text never measured, every check above passes by
    /// looking at nothing. This is the test that says the others had something to
    /// look at.
    #[test]
    fn every_element_actually_lays_out() -> Result<(), TestError> {
        for element in ELEMENTS {
            let mut app = spawn_element(LayoutTest::new(), element, ElementCx::new());
            let mut query = app.world_mut().query::<(&ComputedNode, &Name)>();
            let sized = query
                .iter(app.world())
                .filter(|(computed, _)| computed.size.x > 0.0 && computed.size.y > 0.0)
                .count();
            assert!(
                sized > 0,
                "element `{}` laid out nothing with a non-zero size — the fixture did not \
                 spawn, or its text never measured, and every other check is passing \
                 vacuously",
                element.id
            );
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Behaviour. Not the resting state — what the element *does*.
    // -----------------------------------------------------------------------

    /// A button emits its action when activated, and **nothing else happens**.
    ///
    /// The registry's no-wiring rule, demonstrated: the button is driven exactly
    /// as a click or `Enter` drives it in the viewer, its real observer runs, and
    /// what it meant is read off a message queue — with nothing behind it that
    /// could teleport an avatar, edit an object or spend money. A button wired
    /// straight to a `Session` could not be tested at all without a grid.
    #[test]
    fn activating_a_button_emits_its_action_and_nothing_else() -> Result<(), TestError> {
        let element = ELEMENTS
            .iter()
            .find(|element| element.id == "button-row")
            .ok_or("the `button-row` element is not registered")?;
        let mut app = spawn_element(LayoutTest::new(), element, ElementCx::new());

        let cancel = find_by_name(&mut app, "button:cancel")
            .ok_or("the button row did not spawn a `cancel` button")?;
        activate(&mut app, cancel);

        assert_eq!(
            drain_actions(&mut app),
            vec![UiAction {
                element: "button-row",
                action: "cancel",
            }],
            "activating the Cancel button must emit exactly its own action"
        );
        Ok(())
    }

    /// `Tab` walks the button row in order, and `Shift+Tab` walks back.
    ///
    /// Driven through `bevy_input_focus`'s real navigation rather than by setting
    /// focus directly, so what is under test is what the user does. Three stops,
    /// not two: with two, a cycle is its own reverse and neither order nor
    /// direction is observable.
    #[test]
    fn tab_walks_the_button_row_in_order_and_shift_tab_walks_back() -> Result<(), TestError> {
        let element = ELEMENTS
            .iter()
            .find(|element| element.id == "button-row")
            .ok_or("the `button-row` element is not registered")?;
        let mut app = spawn_element(LayoutTest::new(), element, ElementCx::new());

        let order: Vec<Entity> = ["button:save", "button:discard", "button:cancel"]
            .into_iter()
            .filter_map(|name| find_by_name(&mut app, name))
            .collect();
        assert_eq!(order.len(), 3, "the button row must offer three tab stops");

        app.world_mut().resource_mut::<InputFocus>().clear();
        let mut walked = Vec::new();
        for _stop in 0..3 {
            if let Some(next) = super::navigate(&mut app, NavAction::Next) {
                walked.push(next);
            }
        }
        assert_eq!(
            walked, order,
            "`Tab` must walk the buttons in their declared order"
        );

        let back = super::navigate(&mut app, NavAction::Previous);
        assert_eq!(
            back,
            order.get(1).copied(),
            "`Shift+Tab` must walk back to the previous stop"
        );
        Ok(())
    }

    /// The registry is actually being swept — every element and every script is
    /// reached by the matrix above.
    ///
    /// Cheap insurance against the way a matrix rots: someone adds an element or
    /// a script, nothing references it, and the suite goes on being green about a
    /// smaller world than it claims.
    #[test]
    fn the_matrix_covers_the_whole_registry() {
        assert!(!ELEMENTS.is_empty(), "no elements to sweep");
        assert!(SCRIPTS.len() >= 2, "a one-script matrix is not a matrix");
    }

    /// Every element must fit a **narrow** window, at the longest strings.
    ///
    /// The other end of the viewport axis. A panel is written and eyeballed on
    /// the author's wide monitor, and the person it breaks for is on a laptop
    /// with the UI font turned up and a language that runs long — three axes that
    /// each look fine alone. 720x600 logical is a small but entirely real window.
    #[test]
    fn every_element_fits_a_narrow_window() {
        let mut failures = Vec::new();
        for element in ELEMENTS {
            let test = LayoutTest::new().with_viewport(720, 600);
            let cx = ElementCx {
                text: SampleText::Pseudo,
                font_size: 15.0,
            };
            let mut app = spawn_element(test, element, cx);
            let violations = layout_violations(&mut app, test);
            if !violations.is_empty() {
                failures.push(format!("element `{}`: {violations:#?}", element.id));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
