//! App-side gating for `bevy_ui`'s unconditional per-frame systems
//! (`viewer-perf-ui-layout-per-frame-relayout`).
//!
//! bevy_ui 0.19 runs its whole PostUpdate stack — layout, stacking, clipping —
//! every frame with change detection only *inside* always-running full-tree
//! traversals, so each system has a cost floor proportional to the live UI
//! node count even on frames where nothing UI-related changed at all. Each
//! gated system is the sole member of a public [`bevy::ui::UiSystems`] set, so
//! the app can attach a run condition to the *set*
//! (`configure_sets(PostUpdate, set.run_if(…))`) without forking bevy_ui;
//! this module holds those conditions.
//!
//! A run condition tracks change ticks relative to its **own** last
//! evaluation, and the gated system's internal change detection is relative to
//! the system's own last *run* — so a change is never lost across skipped
//! frames: the condition sees it once and fires, and the system then sees
//! everything since *its* last run. Removal messages are retained for two
//! frames and the conditions fire on the frame the removal is flushed.
//!
//! What is deliberately **not** gated:
//!
//! - `update_clipping_system` shares `UiSystems::PostLayout` with
//!   `text_system` and the viewport-sizing system, so a set-level condition
//!   would wrongly gate text — and its unconditional run is what keeps a
//!   `Display::None` subtree invisible (empty clip rect) whatever the layout
//!   system does.
//! - `ui_picking` — `UiPickingSettings::require_markers` would silently break
//!   every `Button` spawned without an explicit `Pickable` component, and the
//!   per-node work is already skipped for hidden (zero-size) nodes.

use bevy::prelude::*;
use bevy::text::{EditableText, LineHeight};
use bevy::ui::ContentSize;

/// Opt-in marker: this node's [`ContentSize`] (its measured content) can never
/// change the UI layout, so [`ui_layout_dirty`] ignores its per-frame
/// `Changed<ContentSize>` the same way it ignores an [`EditableText`] field's.
///
/// The invariant the caller asserts by adding it: the node sits inside a
/// **fixed-size** (`Val::Px`, non-shrinking) ancestor that **clips overflow**,
/// so a longer/shorter measure neither resizes that ancestor (clip suppresses
/// the min-content minimum; `flex_shrink: 0` stops flex shrinking it) nor
/// escapes it to the outer tree — and the node is single-line, so its measured
/// height is constant. The only residual is the node re-positioning *within*
/// that fixed box (e.g. a trailing-aligned read-out shifting when its digit
/// count changes); like the closed-floater carve-out that deferral is invisible
/// enough to trade for never running the full-tree layout on its account.
///
/// An automatic "fixed-size ancestor" walk cannot replace this: the nodes that
/// need it (the status-bar read-outs) live under content-derived (`Auto`)
/// heights all the way up, so no ancestor is definite on *both* axes — the
/// fixed-width-plus-clip guarantee is real but only the caller can vouch for it.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FixedSlotContentSize;

/// Run condition for [`bevy::ui::UiSystems::Stack`] (`ui_stack_system`): the
/// stack — a full-tree walk plus per-level z-sorts, rebuilt from scratch every
/// run — only needs rebuilding when one of its actual inputs changed. Those
/// inputs are exactly (bevy_ui `stack.rs`): UI-node membership (`Node`
/// added/removed — the secondary `GlobalZIndex` root query filters on
/// `ComputedStackIndex`, a required component of `Node`, so membership covers
/// it), hierarchy (`ChildOf`/`Children` — the root set and each node's
/// children order), and the `ZIndex`/`GlobalZIndex` values sorted by. No
/// camera or `Display` involvement — hidden nodes stay in the stack.
///
/// Removals need care: `RemovedComponents<ChildOf>` (and the z-index
/// components) also fire for **world** entities — prims despawn constantly
/// during a rez burst — so bare removal messages would defeat the gate exactly
/// when it matters. A removal only dirties the stack if the entity still
/// exists as a UI node (a component removed from a live node, e.g. a floater
/// dropping its `GlobalZIndex`); a *despawned* UI node is caught by
/// `RemovedComponents<Node>` instead, and a despawned world entity by
/// neither.
///
/// Ordering audit (2026-07): `ui_stack_system` has **no** ordering constraints
/// against the rest of PostUpdate upstream, so any same-frame writer ordered
/// after it (bevy_flair *can* write `ZIndex` from a CSS `z-index` property —
/// none of our skins use it) was already racing the unconditional rebuild;
/// under the gate such a write lands on the next evaluation instead, the same
/// one-frame worst case.
#[expect(
    clippy::type_complexity,
    reason = "the Or<> filter IS the documented trigger union; splitting it into named type \
              aliases would only scatter it"
)]
pub(crate) fn ui_stack_dirty(
    changed: Query<
        (),
        (
            With<Node>,
            Or<(
                Added<Node>,
                Changed<ChildOf>,
                Changed<Children>,
                Changed<ZIndex>,
                Changed<GlobalZIndex>,
            )>,
        ),
    >,
    nodes: Query<(), With<Node>>,
    mut removed_nodes: RemovedComponents<Node>,
    mut removed_child_of: RemovedComponents<ChildOf>,
    mut removed_z: RemovedComponents<ZIndex>,
    mut removed_global_z: RemovedComponents<GlobalZIndex>,
) -> bool {
    // Read (and thereby drain) every removal cursor even once dirty is known,
    // so a single burst never re-triggers a second, spurious rebuild on the
    // next evaluation.
    let removed_ui_node = removed_nodes.read().count() > 0;
    let removed_on_live_node = removed_child_of
        .read()
        .chain(removed_z.read())
        .chain(removed_global_z.read())
        .filter(|&entity| nodes.contains(entity))
        .count()
        > 0;
    !changed.is_empty() || removed_ui_node || removed_on_live_node
}

/// Run condition for [`bevy::ui::UiSystems::Layout`] (`ui_layout_system`):
/// skip the full-tree layout walk (node iteration, children sync, taffy
/// round pass, geometry recursion) on frames where none of the system's
/// actual inputs changed **visibly**.
///
/// The trigger union covers everything `ui_layout_system` reads (bevy_ui
/// `layout/mod.rs`): node membership and styles (`Node`), measured content
/// (`ContentSize` — the text remeasure path; the Content set runs before
/// Layout, so a same-frame measure is seen), the render-target info
/// (window resize, scale factor, `UiScale`, camera retarget — all propagated
/// into `ComputedUiRenderTargetInfo` by the earlier-ordered Prepare set),
/// the post-layout transform (`UiTransform`), scrolling, outlines, layout
/// config, and the hierarchy (`Children`/`ChildOf`). Removals **always**
/// fire: removal messages die after two frames, and the system's
/// `ui_surface` cleanup must never miss one.
///
/// "Visibly" is deliberately simple — no occlusion logic: a change is
/// ignored iff some **strict ancestor** currently has `Display::None`
/// *and* that ancestor's own `Node` did not change this frame. So content
/// churn inside a closed floater (the conversations window receiving chat
/// and friends-presence updates) defers its layout cost to the open, while
/// the open itself (the ancestor's `Display` flip *is* a `Node` change)
/// always fires — and the deferred changes are still seen then, because the
/// gated system's internal change detection is relative to its own last
/// run. A hidden subtree stays invisible meanwhile regardless of layout:
/// the ungated `update_clipping_system` gives it an empty clip rect every
/// frame.
/// `ContentSize` is a trigger only on **non-editable** nodes: bevy_ui 0.19's
/// editable-text systems (`update_editable_text_styles` and the input-field
/// layout in `widget/text_input_layout.rs`) take `&mut EditableText` and
/// deref it for every field every frame, so `Changed<EditableText>` is
/// permanently true and `update_editable_text_content_size` re-`set`s every
/// field's `ContentSize` each frame — with an **identical** measure (it
/// derives from `visible_lines` / `visible_width` / font, never from the
/// typed content). Taking that as a trigger would keep this gate firing on
/// every frame that any text-input field exists (the chat bar always does).
/// Instead the editable fields trigger on their measure's *real* inputs —
/// `TextFont` / `LineHeight` / `TextLayout` (their `visible_*` fields live on
/// the perma-changed `EditableText` itself and so cannot be watched; they are
/// set at spawn, which `Added<Node>` covers). The upstream churn is filed in
/// the roadmap as a candidate bevy fix.
///
/// A [`FixedSlotContentSize`]-marked node is likewise not a content-size
/// trigger: the caller has vouched that it lives in a fixed-width, clipping
/// slot where its measure cannot change the layout (the status-bar read-outs —
/// the FPS integer re-measures at up to 10 Hz otherwise).
#[expect(
    clippy::type_complexity,
    reason = "the Or<> filters ARE the documented trigger union; splitting them into named \
              type aliases would only scatter it"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "a run condition's parameters are the gated system's full input surface: the \
              three trigger queries, the ancestor walk, the changed-node probe, the UI-node \
              membership check, and the two removal cursors"
)]
pub(crate) fn ui_layout_dirty(
    changed: Query<
        (Entity, Option<&ChildOf>),
        (
            With<Node>,
            Or<(
                Added<Node>,
                Changed<Node>,
                Changed<ComputedUiRenderTargetInfo>,
                Changed<UiTransform>,
                Changed<ScrollPosition>,
                Changed<Outline>,
                Changed<LayoutConfig>,
                Changed<IgnoreScroll>,
                Changed<Children>,
                Changed<ChildOf>,
            )>,
        ),
    >,
    changed_measure: Query<
        (Entity, Option<&ChildOf>),
        (
            With<Node>,
            Without<EditableText>,
            Without<FixedSlotContentSize>,
            Changed<ContentSize>,
        ),
    >,
    changed_editable: Query<
        (Entity, Option<&ChildOf>),
        (
            With<Node>,
            With<EditableText>,
            Or<(Changed<TextFont>, Changed<LineHeight>, Changed<TextLayout>)>,
        ),
    >,
    ancestors: Query<(&Node, Option<&ChildOf>)>,
    changed_nodes: Query<(), (With<Node>, Changed<Node>)>,
    nodes: Query<(), With<Node>>,
    mut removed_nodes: RemovedComponents<Node>,
    mut removed_children: RemovedComponents<Children>,
) -> bool {
    // Removals first (and drain both cursors — see `ui_stack_dirty` on why):
    // a despawned UI node must reach `ui_surface.remove_entities` while its
    // removal message still exists. `RemovedComponents<Children>` also fires
    // for world entities; only one on a live UI node counts.
    let removed_ui_node = removed_nodes.read().count() > 0;
    let removed_ui_children = removed_children
        .read()
        .filter(|&entity| nodes.contains(entity))
        .count()
        > 0;
    if removed_ui_node || removed_ui_children {
        return true;
    }
    'candidates: for (_entity, child_of) in changed
        .iter()
        .chain(changed_measure.iter())
        .chain(changed_editable.iter())
    {
        // Walk the strict ancestors looking for a stably-hidden one.
        let mut parent = child_of.map(ChildOf::parent);
        while let Some(current) = parent {
            let Ok((node, next)) = ancestors.get(current) else {
                break;
            };
            if node.display == Display::None {
                if changed_nodes.contains(current) {
                    // The hidden ancestor itself changed (e.g. the
                    // `Display` flip of a floater opening): a real trigger.
                    return true;
                }
                // Buried under an unchanged `Display::None` ancestor: the
                // change waits for the subtree to be shown.
                continue 'candidates;
            }
            parent = next.map(ChildOf::parent);
        }
        return true;
    }
    false
}

/// Whether `SL_VIEWER_LOG_UI_DIRTY` is set: log, per frame, which entities
/// tripped each [`ui_layout_dirty`] trigger — the tool for finding what keeps
/// the layout gate from ever skipping.
fn log_ui_dirty_enabled() -> bool {
    std::env::var_os("SL_VIEWER_LOG_UI_DIRTY").is_some()
}

/// Log the entities matching each layout-gate trigger this frame (first three
/// per category, with their `Name`s). Runs in `Update` — a hair earlier than
/// the PostUpdate condition, so late PostUpdate writers can still differ — but
/// every steady per-frame dirtier shows up identically.
#[expect(
    clippy::type_complexity,
    reason = "one query per trigger category IS the diagnostic"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "one query per trigger category IS the diagnostic — including the two carved-out \
              (editable / fixed-slot) content-size categories logged for observability"
)]
fn log_ui_layout_dirty_causes(
    changed_node: Query<(Entity, Option<&Name>), (With<Node>, Or<(Added<Node>, Changed<Node>)>)>,
    changed_content: Query<
        (Entity, Option<&Name>),
        (
            With<Node>,
            Without<EditableText>,
            Without<FixedSlotContentSize>,
            Changed<ContentSize>,
        ),
    >,
    changed_editable_content: Query<
        (Entity, Option<&Name>),
        (With<Node>, With<EditableText>, Changed<ContentSize>),
    >,
    changed_fixed_content: Query<
        (Entity, Option<&Name>),
        (With<Node>, With<FixedSlotContentSize>, Changed<ContentSize>),
    >,
    changed_target: Query<
        (Entity, Option<&Name>),
        (With<Node>, Changed<bevy::ui::ComputedUiRenderTargetInfo>),
    >,
    changed_transform: Query<(Entity, Option<&Name>), (With<Node>, Changed<UiTransform>)>,
    changed_scroll: Query<(Entity, Option<&Name>), (With<Node>, Changed<ScrollPosition>)>,
    changed_children: Query<
        (Entity, Option<&Name>),
        (With<Node>, Or<(Changed<Children>, Changed<ChildOf>)>),
    >,
) {
    /// One category's log line: `label: n (name, name, name, …)`.
    fn report(
        label: &str,
        query: &Query<(Entity, Option<&Name>), impl bevy::ecs::query::QueryFilter>,
    ) {
        let count = query.iter().count();
        if count == 0 {
            return;
        }
        let sample: Vec<String> = query
            .iter()
            .take(3)
            .map(|(entity, name)| {
                name.map_or_else(|| format!("{entity}"), |name| name.as_str().to_owned())
            })
            .collect();
        info!("ui-dirty {label}: {count} ({})", sample.join(", "));
    }
    report("node", &changed_node);
    report("content-size", &changed_content);
    // Editable fields' per-frame `ContentSize` churn (upstream, see
    // `viewer-perf-editable-text-per-frame-churn`) is carved out of the gate;
    // logged separately so its volume stays observable.
    report("content-size(editable, ignored)", &changed_editable_content);
    // Fixed-slot read-outs (the status bar) whose measure cannot affect layout
    // are carved out too (see `FixedSlotContentSize`); logged separately.
    report("content-size(fixed-slot, ignored)", &changed_fixed_content);
    report("render-target", &changed_target);
    report("ui-transform", &changed_transform);
    report("scroll", &changed_scroll);
    report("hierarchy", &changed_children);
}

/// How many times the gated layout set actually ran ([`count_layout_runs`]).
#[derive(Resource, Default)]
struct LayoutRuns(u32);

/// Counts layout-set runs: registered **in** `UiSystems::Layout`, so it
/// inherits the set-level [`ui_layout_dirty`] gate and runs exactly when
/// `ui_layout_system` does.
fn count_layout_runs(mut runs: ResMut<LayoutRuns>) {
    runs.0 = runs.0.wrapping_add(1);
}

/// Log the layout gate's skip rate every ~5 s: layout-set runs vs frames.
/// Window-focus / tracy-free — the ground truth for "does the gate skip".
fn log_layout_skip_rate(
    frames: Res<bevy::diagnostic::FrameCount>,
    runs: Res<LayoutRuns>,
    mut last: Local<Option<(u32, u32)>>,
    time: Res<Time>,
    mut next_at: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now < *next_at {
        return;
    }
    *next_at = now + 5.0;
    if let Some((last_frames, last_runs)) = *last {
        let frame_delta = frames.0.wrapping_sub(last_frames);
        let run_delta = runs.0.wrapping_sub(last_runs);
        info!("ui layout gate: ran {run_delta} of {frame_delta} frames");
    }
    *last = Some((frames.0, runs.0));
}

/// Registers the (env-gated) layout-gate cause logger and skip-rate meter.
pub(crate) struct UiPerfDiagnosticsPlugin;

impl Plugin for UiPerfDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        if log_ui_dirty_enabled() {
            app.init_resource::<LayoutRuns>()
                .add_systems(Update, (log_ui_layout_dirty_causes, log_layout_skip_rate))
                .add_systems(
                    PostUpdate,
                    count_layout_runs.in_set(bevy::ui::UiSystems::Layout),
                );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ui_layout_dirty, ui_stack_dirty};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// How often the gated probe ran — i.e. how often [`ui_stack_dirty`]
    /// returned true.
    #[derive(Resource, Default)]
    struct Fires(usize);

    /// The probe standing in for `ui_stack_system` (which bevy_ui does not
    /// export — the real registration gates the public `UiSystems::Stack` set
    /// instead; these tests pin the *condition's* fire pattern).
    fn probe(mut fires: ResMut<Fires>) {
        fires.0 = fires.0.wrapping_add(1);
    }

    /// A minimal app running a counting probe behind the real condition.
    fn gated_app() -> App {
        let mut app = App::new();
        app.init_resource::<Fires>();
        app.add_systems(Update, probe.run_if(ui_stack_dirty));
        app
    }

    /// How often the probe has run so far.
    fn fires(app: &App) -> usize {
        app.world().resource::<Fires>().0
    }

    /// Node spawns fire the gate once; a clean frame skips; a `ZIndex` change
    /// fires again.
    #[test]
    fn fires_on_spawn_skips_when_clean_fires_on_zindex() {
        let mut app = gated_app();
        let node = app.world_mut().spawn((Node::default(), ZIndex(1))).id();
        app.world_mut().spawn((Node::default(), ZIndex(2)));
        app.update();
        assert_eq!(fires(&app), 1);

        app.update();
        app.update();
        assert_eq!(fires(&app), 1);

        app.world_mut().entity_mut(node).insert(ZIndex(3));
        app.update();
        assert_eq!(fires(&app), 2);
    }

    /// A UI-node despawn fires (via `RemovedComponents<Node>`); reparenting a
    /// live node fires (via `Changed<ChildOf>`).
    #[test]
    fn fires_on_ui_despawn_and_reparent() {
        let mut app = gated_app();
        let root = app.world_mut().spawn(Node::default()).id();
        let child = app.world_mut().spawn(Node::default()).id();
        let doomed = app.world_mut().spawn(Node::default()).id();
        app.update();
        assert_eq!(fires(&app), 1);

        app.world_mut().entity_mut(doomed).despawn();
        app.update();
        assert_eq!(fires(&app), 2);

        app.world_mut().entity_mut(child).insert(ChildOf(root));
        app.update();
        assert_eq!(fires(&app), 3);
    }

    /// A layout-gated app: the probe stands in for `ui_layout_system` (the
    /// real registration gates the `UiSystems::Layout` set).
    fn layout_gated_app() -> App {
        let mut app = App::new();
        app.init_resource::<Fires>();
        app.add_systems(Update, probe.run_if(ui_layout_dirty));
        app
    }

    /// The layout gate's visible-only rule: a change buried under an
    /// unchanged `Display::None` ancestor (a closed floater receiving
    /// content) does not fire; showing that ancestor does; a visible change
    /// does.
    #[test]
    fn layout_gate_ignores_hidden_subtree_churn() {
        let mut app = layout_gated_app();
        let hidden_parent = app
            .world_mut()
            .spawn(Node {
                display: Display::None,
                ..Node::default()
            })
            .id();
        let hidden_child = app
            .world_mut()
            .spawn((Node::default(), ChildOf(hidden_parent)))
            .id();
        let visible = app.world_mut().spawn(Node::default()).id();
        app.update();
        assert_eq!(fires(&app), 1);
        app.update();
        assert_eq!(fires(&app), 1);

        // Content churn inside the hidden subtree: no fire.
        app.world_mut().entity_mut(hidden_child).insert(Node {
            width: Val::Px(120.0),
            ..Node::default()
        });
        app.update();
        assert_eq!(fires(&app), 1);

        // A visible node's change: fires.
        app.world_mut().entity_mut(visible).insert(Node {
            width: Val::Px(64.0),
            ..Node::default()
        });
        app.update();
        assert_eq!(fires(&app), 2);

        // Showing the hidden ancestor (its own `Node` changes): fires — and
        // the gated system would then see the deferred child change too.
        app.world_mut()
            .entity_mut(hidden_parent)
            .insert(Node::default());
        app.update();
        assert_eq!(fires(&app), 3);
    }

    /// A [`FixedSlotContentSize`]-marked node (a fixed-width, clipping
    /// status-bar read-out) does not fire the gate when it re-measures, while an
    /// unmarked node's `ContentSize` change does — the FPS integer re-shaping at
    /// ~10 Hz must not force a full-tree relayout.
    #[test]
    fn layout_gate_ignores_fixed_slot_content_size() {
        use super::FixedSlotContentSize;
        use bevy::ui::ContentSize;
        let mut app = layout_gated_app();
        let fixed = app
            .world_mut()
            .spawn((
                Node::default(),
                ContentSize::default(),
                FixedSlotContentSize,
            ))
            .id();
        let plain = app
            .world_mut()
            .spawn((Node::default(), ContentSize::default()))
            .id();
        app.update();
        assert_eq!(fires(&app), 1);
        app.update();
        assert_eq!(fires(&app), 1);

        // The fixed-slot read-out re-measures (its `ContentSize` changes): the
        // carve-out keeps the gate closed.
        app.world_mut()
            .entity_mut(fixed)
            .insert(ContentSize::default());
        app.update();
        assert_eq!(fires(&app), 1);

        // An unmarked node's re-measure still fires.
        app.world_mut()
            .entity_mut(plain)
            .insert(ContentSize::default());
        app.update();
        assert_eq!(fires(&app), 2);
    }

    /// Removals always fire the layout gate — the surface cleanup must never
    /// outlive the two-frame removal-message window — while world-entity
    /// churn stays ignored.
    #[test]
    fn layout_gate_fires_on_ui_removal_only() {
        let mut app = layout_gated_app();
        let doomed = app.world_mut().spawn(Node::default()).id();
        app.update();
        assert_eq!(fires(&app), 1);

        let world_parent = app.world_mut().spawn_empty().id();
        let world_child = app
            .world_mut()
            .spawn((Transform::default(), ChildOf(world_parent)))
            .id();
        app.update();
        assert_eq!(fires(&app), 1);
        app.world_mut().entity_mut(world_child).despawn();
        app.world_mut().entity_mut(world_parent).despawn();
        app.update();
        assert_eq!(fires(&app), 1);

        app.world_mut().entity_mut(doomed).despawn();
        app.update();
        assert_eq!(fires(&app), 2);
    }

    /// World-entity (non-`Node`) hierarchy churn — the constant state during a
    /// rez burst — must NOT fire the gate: neither spawning a parented world
    /// entity nor despawning it (which flushes `RemovedComponents<ChildOf>`)
    /// counts as UI dirt.
    #[test]
    fn ignores_world_entity_churn() {
        let mut app = gated_app();
        app.world_mut().spawn(Node::default());
        app.update();
        assert_eq!(fires(&app), 1);

        let world_parent = app.world_mut().spawn_empty().id();
        let world_child = app
            .world_mut()
            .spawn((Transform::default(), ChildOf(world_parent)))
            .id();
        app.update();
        assert_eq!(fires(&app), 1);

        app.world_mut().entity_mut(world_child).despawn();
        app.world_mut().entity_mut(world_parent).despawn();
        app.update();
        app.update();
        assert_eq!(fires(&app), 1);
    }
}
