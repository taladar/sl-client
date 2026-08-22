//! The headless half of the UI test harness (`viewer-ui-test-harness`): enough
//! of `bevy_ui` to run **real layout, with real fonts, in `cargo test`** — no
//! window, no renderer, no GPU, no login, no world.
//!
//! Its own crate because the widget crates and the binary both test against it,
//! and a harness living in any one of them could not be reached from the
//! others. It depends only on the UI vocabulary, never on the widgets it is
//! used to test: a widget whose placement taffy cannot express supplies its own
//! layout system through [`LayoutTest::with_widget_layout`].
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
//! `a_text_node_may_not_carry_its_own_padding` now catches it in a
//! fifth of a second.
//!
//! So the **matrix lives here**, not in the gallery (`gallery`). The
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

use sl_viewer_ui_core::ui::{
    UiDirection, UiRoot, UiScaffoldSystems, apply_panel_visibility, apply_ui_direction,
    invalidate_logical_boxes, resolve_logical_boxes, spawn_ui_root,
};
use sl_viewer_ui_core::ui_element::{
    AlignEdge, AlignmentGroup, ElementCx, RadialCentre, RadialPlacement, TextMayClip, UiAction,
    UiElement,
};
use sl_viewer_ui_core::ui_font::register_ui_fonts;

/// A boxed error, so a test can use `?` rather than the workspace-denied
/// `unwrap` / `expect`.
pub type TestError = Box<dyn core::error::Error>;

/// A node's border box, in physical pixels, from its computed size and where the
/// layout put it.
///
/// Built per-component in plain `f32` rather than with the `glam` operators, per
/// the convention the viewer follows (`ik`, `camera`):
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
/// for that is `a_text_node_may_not_carry_its_own_padding` (in the viewer), which starts
/// failing the day Bevy corrects it.
const OVERFLOW_EPSILON: f32 = 6.0;

/// A headless `bevy_ui` layout app, configured and then [`build`](Self::build).
///
/// The defaults are the interesting-but-boring case: a roomy viewport at scale
/// factor 1, `UiScale` 1, left-to-right. Each `with_*` method moves one axis of
/// the matrix.
#[derive(Debug, Clone, Copy)]
pub struct LayoutTest {
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
    /// Extra layout systems to register, for widgets whose placement taffy
    /// cannot express. See the note in [`Self::app`].
    widget_layout: &'static [fn(&mut App)],
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
            widget_layout: &[],
        }
    }
}

impl LayoutTest {
    /// A test at the default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the window's size in logical pixels.
    #[must_use]
    pub const fn with_viewport(mut self, width: u32, height: u32) -> Self {
        self.logical_viewport = UVec2::new(width, height);
        self
    }

    /// Set the window scale factor (the display's DPI scaling).
    #[must_use]
    pub const fn with_scale_factor(mut self, scale_factor: f32) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// Set the `UiScale` (the user's UI size preference).
    #[must_use]
    pub const fn with_ui_scale(mut self, ui_scale: f32) -> Self {
        self.ui_scale = ui_scale;
        self
    }

    /// Register extra layout systems for widgets taffy cannot place — the
    /// harness runs each after `UiSystems::Layout`.
    #[must_use]
    pub const fn with_widget_layout(mut self, widget_layout: &'static [fn(&mut App)]) -> Self {
        self.widget_layout = widget_layout;
        self
    }

    /// Set the inline layout direction.
    #[must_use]
    pub const fn with_direction(mut self, direction: UiDirection) -> Self {
        self.direction = direction;
        self
    }

    /// This configuration's viewport in **physical** pixels — the logical window
    /// scaled by the display's scale factor, and what [`viewport_violations`]
    /// measures a tree against (`ComputedNode` is physical throughout).
    #[must_use]
    pub fn viewport(self) -> UVec2 {
        self.logical_viewport
            .as_vec2()
            .mul_add(Vec2::splat(self.scale_factor), Vec2::ZERO)
            .as_uvec2()
    }

    /// This configuration's inline direction — what [`alignment_violations`]
    /// resolves a logical edge against.
    #[must_use]
    pub const fn direction(self) -> UiDirection {
        self.direction
    }

    /// Build the app: the layout pipeline, the viewer's font stack, the
    /// scaffold's own systems, and a [`sl_viewer_ui_core::ui::UiRoot`] to parent fixtures to.
    ///
    /// The app is returned **un-run**, so a test can add its own `Startup`
    /// fixture system (ordered `.after(UiScaffoldSystems::SpawnRoot)`, exactly as
    /// a real panel does) before the first `update`.
    pub fn build(self) -> App {
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

        // **Widget layout systems**: a registered element whose placement is not
        // pure taffy needs its own system run, or the harness lays it out
        // differently from the viewer and every check reasons about a widget that
        // does not exist. The pie menu is the example — its labels are placed by
        // polar coordinate, which no flexbox can express.
        //
        // The caller supplies them ([`Self::with_widget_layout`]) rather than
        // this harness naming them: it sits below the widgets, so it cannot see
        // them, and a widget that needs one knows it does. They run after layout,
        // because such a system reads each label's *measured* size and writes
        // next frame's placement exactly as it does live; hence [`settle`] taking
        // two frames matters here too.
        for register in self.widget_layout {
            register(&mut app);
        }

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
pub fn settle(app: &mut App) {
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
pub fn overflow_violations(app: &mut App) -> Vec<String> {
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
pub fn viewport_violations(app: &mut App, viewport: UVec2) -> Vec<String> {
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
pub fn containment_violations(app: &mut App) -> Vec<String> {
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
/// See `may_be_clipped` and [`TextMayClip`]: the exception is opt-in, carries a
/// reason, and is greppable, rather than being a silent special case in here.
pub fn clipping_violations(app: &mut App) -> Vec<String> {
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
pub fn alignment_violations(app: &mut App, direction: UiDirection) -> Vec<String> {
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

/// **Declared.** Every node with a [`RadialPlacement`] must actually lie in the
/// direction it names, from its group's [`RadialCentre`].
///
/// The tier that exists because the box vocabulary runs out. See
/// [`RadialPlacement`] for the argument in full: a radial menu's slices are
/// angular sectors drawn by a shader, so there is no node for a harness to
/// measure, and every box in the widget can be legal while the thing lies about
/// what it will do. The element states the direction it means and this holds it
/// to it — in every script, at every scale, in both directions.
///
/// Measured to each node's **box centre**, because that is what a person aims
/// at, and in the y-up frame the angles are declared in.
pub fn radial_violations(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut centres_query = world.query::<(&RadialCentre, &UiGlobalTransform)>();
    let centres: Vec<(&'static str, Vec2)> = centres_query
        .iter(world)
        .map(|(centre, transform)| (centre.group, transform.translation))
        .collect();
    let mut placed = world.query::<(Entity, &RadialPlacement, &UiGlobalTransform, Option<&Name>)>();
    let mut violations = Vec::new();
    for (entity, placement, transform, name) in placed.iter(world) {
        let Some((_, centre)) = centres.iter().find(|(group, _)| *group == placement.group) else {
            violations.push(format!(
                "{}: declares a radial placement in group `{}`, which has no `RadialCentre` — \
                 the claim cannot be checked, so it is not a claim",
                describe(name, entity),
                placement.group,
            ));
            continue;
        };
        // The y-up frame the angles are declared in: `bevy_ui`'s y grows downward.
        let offset = Vec2::new(
            transform.translation.x - centre.x,
            -(transform.translation.y - centre.y),
        );
        if offset.length() < f32::EPSILON {
            continue;
        }
        let actual = offset.to_angle();
        let strayed = angular_difference(actual, placement.angle);
        if strayed > placement.tolerance {
            violations.push(format!(
                "{}: declared at {:.1}° from the centre of `{}` but laid out at {:.1}°, which \
                 is {:.1}° away — more than the {:.1}° it is allowed. A pointer aimed at this \
                 lands in a different slice than the one it names.",
                describe(name, entity),
                placement.angle.to_degrees(),
                placement.group,
                actual.to_degrees(),
                strayed.to_degrees(),
                placement.tolerance.to_degrees(),
            ));
        }
    }
    violations
}

/// **Declared.** No two nodes placed in a radial group may overlap **each
/// other**.
///
/// The radial counterpart of [`clipping_violations`], and it is a *counterpart*
/// rather than a duplicate — the harm is the same, the cause is not.
///
/// [`clipping_violations`] catches text made unreadable by a **clip**. A pie has
/// no clip: its labels are bounded by `max_width`, which *wraps* text rather than
/// hiding it, so no label ever gets a `CalculatedClip` and that check is silent
/// here. Correctly silent — nothing is hidden.
///
/// A pie's labels are made unreadable a different way: by landing **on top of
/// each other**. Two overlapping boxes are each individually legal — inside their
/// parent, content inside themselves, nothing clipped — so every box check in this
/// file passes while two labels are an unreadable pile. That is the same blind
/// spot [`radial_violations`] covers for direction, on the other axis, and this is
/// exactly what found the day the labels wrapped taller than the row they were in
/// and dropped onto their neighbours.
///
/// It is **label against label only**, deliberately. The labels sit *inside* the
/// ring — that is the whole point of the polar layout, they are drawn over the
/// wedges they name — so a label's box overlapping the ring's box is not a defect,
/// it is the design. The [`RadialCentre`] is the frame the directions are measured
/// from ([`radial_violations`]); it is not a thing the labels must avoid.
pub fn radial_overlap_violations(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut members = world.query::<(
        Entity,
        &RadialPlacement,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&Name>,
    )>();
    let boxes: Vec<(&'static str, String, Rect)> = members
        .iter(world)
        .map(|(entity, placement, computed, transform, name)| {
            (
                placement.group,
                describe(name, entity),
                border_box(computed, transform),
            )
        })
        .collect();

    let mut violations = Vec::new();
    for (index, (group, name, rect)) in boxes.iter().enumerate() {
        for (other_group, other_name, other_rect) in boxes.iter().skip(index.saturating_add(1)) {
            if group != other_group {
                continue;
            }
            if rect.size().cmple(Vec2::ZERO).any() || other_rect.size().cmple(Vec2::ZERO).any() {
                continue;
            }
            let shared = rect.intersect(*other_rect);
            // A shared edge is not an overlap; a shared *area* is. The epsilon is
            // the same tolerance every other check here uses.
            if shared.size().x > OVERFLOW_EPSILON && shared.size().y > OVERFLOW_EPSILON {
                violations.push(format!(
                    "{name} and {other_name} overlap by {} px in radial group `{group}` — \
                     they are drawn on top of each other, so at least one is unreadable",
                    shared.size(),
                ));
            }
        }
    }
    violations
}

/// The absolute angular difference between two angles, wrapped into `0..=PI`.
fn angular_difference(left: f32, right: f32) -> f32 {
    let raw = (left - right).rem_euclid(core::f32::consts::TAU);
    if raw > core::f32::consts::PI {
        core::f32::consts::TAU - raw
    } else {
        raw
    }
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
pub fn layout_violations(app: &mut App, test: LayoutTest) -> Vec<String> {
    let mut violations = overflow_violations(app);
    violations.extend(containment_violations(app));
    violations.extend(clipping_violations(app));
    violations.extend(viewport_violations(app, test.viewport()));
    violations.extend(alignment_violations(app, test.direction()));
    violations.extend(radial_violations(app));
    violations.extend(radial_overlap_violations(app));
    violations
}

// ---------------------------------------------------------------------------
// Driving input, so behaviour is checkable and not just the resting state.
// ---------------------------------------------------------------------------

/// Spawn one element from the registry into a fresh app, and settle it.
///
/// The whole of a matrix cell's setup. Returns the app and the element's root
/// entity, so a check can look at the tree and a behaviour test can click it.
/// Wire up [`UiAction`] recording on `app`, so [`activate`] / [`drain_actions`]
/// can read what an element's click meant.
///
/// Shared by [`spawn_element`] and any test that builds its own app rather than
/// going through the registry (a widget checked directly, like the pie), so the
/// action-recording machinery lives in one place.
pub fn enable_action_recording(app: &mut App) {
    app.add_message::<UiAction>()
        .init_resource::<RecordedActions>()
        .add_systems(Update, record_actions);
}

/// Build an app and spawn one registered element into it, ready to settle.
///
/// The element is spawned under the scaffold's UI root, so it lays out exactly
/// where it would live in the viewer.
pub fn spawn_element(test: LayoutTest, element: &UiElement, cx: ElementCx) -> App {
    let mut app = test.build();
    enable_action_recording(&mut app);
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
pub fn find_by_name(app: &mut App, name: &str) -> Option<Entity> {
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
pub fn activate(app: &mut App, entity: Entity) {
    app.world_mut().trigger(Activate { entity });
    settle(app);
}

/// Move keyboard focus one stop, as `Tab` (or `Shift+Tab`) would, and settle.
///
/// Goes through `bevy_input_focus`'s real navigation rather than setting
/// [`InputFocus`] directly, so what is tested is the thing the user drives: the
/// order, the wrap-around, and whether a node is reachable at all.
pub fn navigate(app: &mut App, action: NavAction) -> Option<Entity> {
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
pub struct RecordedActions(Vec<UiAction>);

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
pub fn drain_actions(app: &mut App) -> Vec<UiAction> {
    core::mem::take(&mut app.world_mut().resource_mut::<RecordedActions>().0)
}
