//! The viewer UI scaffold (`viewer-ui-widget-scaffold`): the plugin that stands
//! the `bevy_ui` framework up, and the anchor for the two cross-cutting
//! conventions every viewer widget and panel inherits.
//!
//! # What the plugin wires
//!
//! Bevy's `DefaultPlugins` already brings most of the framework, because the
//! `ui` feature is on by default: `bevy_ui`'s `UiPlugin`, the `bevy_ui_widgets`
//! widget observers (`UiWidgetsPlugins` — button, checkbox, slider, list box,
//! menu, scroll area, radio group, and the `EditableText` input), and
//! `bevy_input_focus`'s `InputFocusPlugin` (the [`InputFocus`] /
//! `InputFocusVisible` resources) plus `InputDispatchPlugin` (which routes
//! keyboard input to the focused entity). Three things it does *not* bring, and
//! [`ViewerUiPlugin`] does:
//!
//! - [`TabNavigationPlugin`] — the keyboard half of focus. Without it `Tab` does
//!   nothing, because `DefaultPlugins` wires focus *dispatch* but no navigation.
//! - [`crate::ui_font::register_ui_fonts`] — the bundled font stack from
//!   the `viewer-ui-text-foundation` stack ([`crate::ui_text`]): `Inter` + `DejaVu` under
//!   private families, and the `CBDT` colour-emoji face bound to the `Emoji`
//!   generic. OS font enumeration and per-script fallback come from the
//!   `system_font_discovery` Bevy feature (see this crate's `Cargo.toml`).
//! - [`UiRoot`] — the single node every viewer panel parents itself to, and the
//!   [`TabGroup`] all tab navigation walks.
//!
//! # Convention 1 — direction-neutral, bidi-first
//!
//! **Name every directional API and style hook logically — never `left` /
//! `right`.** We implement the Unicode Bidirectional Algorithm (proven in
//! [`crate::ui_text`]); an RTL locale must mirror the whole layout with no
//! per-side special-casing. The reference viewer does not do this, so there is
//! no prior art to copy and a late retrofit would touch every panel.
//!
//! Most of `bevy_ui` is already logical and needs nothing from us:
//! `FlexDirection::Row` means "along the text direction" (not "left to right"),
//! and the alignment enums (`AlignItems`, `JustifyContent`, …) only offer
//! `Start` / `End` / `FlexStart` / `FlexEnd`. The physical leak is the **box
//! model**: `Node`'s `margin` / `padding` / `border` are a `UiRect` of
//! `left` / `right` / `top` / `bottom`, and its inset is four physical `Val`
//! fields.
//!
//! So this module supplies the missing half — [`LogicalRect`] (the CSS logical
//! properties: `inline_start` / `inline_end` / `block_start` / `block_end`) and
//! the [`LogicalMargin`] / [`LogicalPadding`] / [`LogicalBorder`] components
//! that [`resolve_logical_boxes`] folds into the physical `Node` against the
//! live [`UiDirection`]. Write an asymmetric box in
//! logical terms and it mirrors for free; a *symmetric* one (`UiRect::all`,
//! `UiRect::horizontal`) needs no component, because it mirrors onto itself.
//!
//! `Node`'s fourth physical box — the **inset** (`left` / `right` / `top` /
//! `bottom`) — deliberately has no logical component yet, because nothing here
//! positions itself by inset: convention 2 says a panel places itself by flow.
//! The first real inset is a floater's remembered on-screen position
//! (`viewer-ui-floater-basic`), and that task should add `LogicalInset` in the
//! shape of the three above — resting at `Val::Auto` rather than zero, which
//! for an inset means "wherever flow put me" rather than "pinned to the edge".
//!
//! The one non-obvious mechanic is that `taffy` — the layout engine under
//! `bevy_ui` — has **no style inheritance**: it reads `direction` off each
//! node's own style and defaults it to `Ltr`. Setting the direction on the root
//! alone would leave every descendant left-to-right, so [`apply_ui_direction`]
//! writes it to every `Node` in the tree.
//!
//! # Convention 2 — content-driven auto-layout
//!
//! **Build on `bevy_ui`'s taffy/flexbox with content sizing; no absolute pixel
//! rects.** This is a strict superset of the reference's absolute-`topleft`
//! model, and it dissolves the whole class of breakage where a longer
//! translated label, a larger UI font, or a wider script overflows a
//! fixed-width panel. [`column()`] and [`row()`] are the constructors that carry
//! the rule: they set flow and gap, and leave sizing alone (`Val::Auto`
//! everywhere), so a container is exactly as big as its content needs.
//!
//! Fixed pixel sizes remain right for things whose size is *intrinsic* rather
//! than a guess about text — a texture thumbnail, an icon, a colour swatch.
//! The rule is about **containers of text**, which must never be pinned to a
//! measurement taken in one language at one font size.
//!
//! # Convention 3 — constructible without its wiring
//!
//! **A UI element must be spawnable with no session, no grid and no world, and
//! its actions must be injectable.** Established by `viewer-ui-test-harness`;
//! the mechanism and the full argument live in [`crate::ui_element`].
//!
//! In short: an element never calls into a live `Session`. It emits a
//! `UiAction`, and who listens decides what that means — the viewer routes it to
//! a real handler, the gallery routes it nowhere (so a click is inert *by
//! construction* rather than by stubbing the dangerous parts out), and a test
//! reads the queue to assert what a click meant.
//!
//! This is not overhead, it is the thing that makes a panel testable at all. A
//! button wired straight to a `Session` cannot be exercised without a grid and a
//! human; a button that emits a `UiAction` is exercised by reading a queue. A
//! panel that can only be spawned by reaching for a live session is a panel that
//! can never be tested, and retrofitting the separation later is exactly the kind
//! of rework this scaffold exists to prevent.
//!
//! The obligation on a new panel or widget is one line: **register it in
//! [`crate::ui_element::ELEMENTS`]**. That buys it every check in
//! [`crate::ui_test`] — across every script, direction, UI scale, font size and
//! translation length — including the checks that do not exist yet.
//!
//! Reference (Firestorm, read-only): `indra/llui/` (`llpanel`,
//! `lluictrlfactory`), and the XUI layouts under `newview/skins/` — a feature
//! checklist, **not** something to import: their pixel coordinates *are* the
//! design and cannot carry over.

use std::ffi::OsStr;

use bevy::input_focus::tab_navigation::{TabGroup, TabIndex, TabNavigationPlugin};
use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::ui_font::{UiFont, register_ui_fonts};

/// The environment variable that seeds [`UiDirection`], so the RTL mirroring can
/// be exercised before a locale selector exists (`viewer-i18n-locale-selection`).
const UI_DIRECTION_ENV: &str = "SL_VIEWER_UI_DIRECTION";

/// The value of [`UI_DIRECTION_ENV`] that selects right-to-left.
const UI_DIRECTION_RTL: &str = "rtl";

/// The value of [`UI_DIRECTION_ENV`] that selects left-to-right.
const UI_DIRECTION_LTR: &str = "ltr";

/// The scaffold's startup work, as a set, so a panel spawned by another module
/// can order itself after the root exists (`.after(UiScaffoldSystems::SpawnRoot)`)
/// and read [`UiRoot`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum UiScaffoldSystems {
    /// [`spawn_ui_root`] has run: [`UiRoot`] is inserted and its node exists.
    SpawnRoot,
}

/// The viewer UI plugin: the "the framework is stood up" anchor that every other
/// UI task builds on. See the [module documentation](self) for what it wires and
/// the two conventions it establishes.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ViewerUiPlugin;

impl Plugin for ViewerUiPlugin {
    fn build(&self, app: &mut App) {
        app
            // The keyboard half of focus. `DefaultPlugins` brings `InputFocus`
            // and dispatch, but not navigation, so `Tab` is inert without this.
            .add_plugins(TabNavigationPlugin)
            .insert_resource(UiDirection::from_env())
            .insert_resource(UiDemoVisible::from_env())
            .init_resource::<UiDemoLabelLong>()
            .init_resource::<UiDemoTextSize>()
            .add_systems(
                Startup,
                (
                    // Register the bundled faces under their private families and
                    // re-point the generics, before any text is shaped in
                    // `PostUpdate`.
                    register_ui_fonts,
                    spawn_ui_root.in_set(UiScaffoldSystems::SpawnRoot),
                    setup_ui_demo.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            // `PostStartup`, because that is where `InputFocusPlugin` parks focus
            // on the window; ordering after it is what makes the clear stick.
            .add_systems(
                PostStartup,
                clear_initial_window_focus.after(bevy::input_focus::set_initial_focus),
            )
            .add_systems(
                Update,
                (
                    toggle_ui_demo,
                    apply_ui_demo_visibility.after(toggle_ui_demo),
                    update_ui_demo_text,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    apply_panel_visibility,
                    invalidate_logical_boxes,
                    resolve_logical_boxes,
                    apply_ui_direction,
                )
                    // Chained because the middle two are a dirty-then-resolve
                    // pair, and because three of the four write `Node` — left
                    // unordered, they would be an ambiguous access.
                    .chain()
                    // Ahead of the layout pass, so a node spawned or re-resolved
                    // this frame is laid out with the boxes it asked for rather
                    // than one frame late.
                    .before(bevy::ui::UiSystems::Layout),
            )
            .add_systems(
                PostUpdate,
                // *After* layout, because it reads the freshly computed
                // `ComputedNode` / `UiGlobalTransform` of the focused widget and
                // its scroll container to decide how far to scroll.
                scroll_focus_into_view.after(bevy::ui::UiSystems::Layout),
            );
    }
}

/// The inline-axis direction the whole UI lays out in — the CSS `direction`
/// property, and the resource the [`LogicalRect`] components resolve against.
///
/// This is deliberately one global rather than a per-node style: the reference
/// viewer's model is one UI language at a time, and so is ours. Text *within* a
/// node still runs bidi per-paragraph (parley resolves that from the characters
/// themselves), so a Hebrew name in an LTR UI reads correctly either way. What
/// this flips is the **layout**: which side a panel's leading edge is on, which
/// way a row flows, which side an asymmetric padding lands on.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum UiDirection {
    /// Left-to-right: the inline start edge is the left one (Latin, CJK,
    /// Cyrillic, …).
    #[default]
    Ltr,
    /// Right-to-left: the inline start edge is the right one (Arabic, Hebrew).
    Rtl,
}

impl UiDirection {
    /// The initial direction, seeded from [`UI_DIRECTION_ENV`]: `rtl` selects
    /// right-to-left, anything else (including unset) left-to-right.
    ///
    /// A stopgap until the locale selector (`viewer-i18n-locale-selection`)
    /// owns this, and the way the mirroring is exercised by hand today — the
    /// resource is otherwise write-able at runtime, and everything downstream
    /// re-resolves when it changes.
    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var_os(UI_DIRECTION_ENV).as_deref())
    }

    /// [`Self::from_env`]'s decision, split out from the read so it can be
    /// tested without `set_var` (which races every other thread's `getenv`).
    fn parse(value: Option<&OsStr>) -> Self {
        match value {
            Some(value) if value.eq_ignore_ascii_case(UI_DIRECTION_RTL) => Self::Rtl,
            Some(_) | None => Self::Ltr,
        }
    }

    /// The direction the [`UI_DIRECTION_ENV`] knob *forces*, or `None` when it is
    /// unset so the locale (`crate::i18n`) drives the layout instead.
    ///
    /// Distinct from [`from_env`](Self::from_env), which cannot tell "unset" from
    /// "`ltr`": the i18n scaffold needs "no override" to be a third answer, so a
    /// non-Latin locale can flip the layout while an explicit knob still wins.
    pub(crate) fn rtl_override_from_env() -> Option<Self> {
        Self::parse_override(std::env::var_os(UI_DIRECTION_ENV).as_deref())
    }

    /// [`Self::rtl_override_from_env`]'s decision, split out so it is testable
    /// without touching the process environment.
    fn parse_override(value: Option<&OsStr>) -> Option<Self> {
        match value {
            Some(value) if value.eq_ignore_ascii_case(UI_DIRECTION_RTL) => Some(Self::Rtl),
            Some(value) if value.eq_ignore_ascii_case(UI_DIRECTION_LTR) => Some(Self::Ltr),
            Some(_) | None => None,
        }
    }

    /// Whether the inline axis runs right-to-left.
    pub(crate) const fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }

    /// This direction as the `bevy_ui` style value that `taffy` lays out with.
    const fn inline(self) -> InlineDirection {
        match self {
            Self::Ltr => InlineDirection::Ltr,
            Self::Rtl => InlineDirection::Rtl,
        }
    }
}

/// The viewer's single `bevy_ui` root: the node every panel, floater and overlay
/// parents itself to, and the [`TabGroup`] tab navigation walks.
///
/// Held as a resource (rather than looked up by marker every time) because a
/// spawning system's usual shape is `commands.spawn((…, ChildOf(root.0)))`, and
/// a `Res` is cheaper and less error-prone than a single-item query. Systems that
/// spawn into it must run `.after(UiScaffoldSystems::SpawnRoot)`.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct UiRoot(pub(crate) Entity);

/// A marker on the [`UiRoot`] node itself, for the systems that need to find it
/// in the hierarchy rather than by resource.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct UiRootNode;

/// Drop the focus `InputFocusPlugin` parks on the primary window at startup.
///
/// `bevy_input_focus` seeds focus to the window so that keyboard input has
/// somewhere to go, but the window is not a UI node and so sits under no
/// [`TabGroup`] — which makes the very first `Tab` press log
/// `No tab group found for currently focused entity … Users will not be able to
/// navigate back to this entity`. That is the truth (the window is not
/// tabbable), but it is not a problem: it is the resting state, once per run.
///
/// Clearing focus instead is not a loss. `dispatch_focused_input` explicitly
/// routes to the primary window when focus is `None`, so keyboard input goes
/// exactly where it did — which is *also* how the viewer stays drivable
/// (`WASD`, the overlay toggles) with no UI focused, and where
/// [`apply_panel_visibility`] returns it when a panel closes on its own focus.
fn clear_initial_window_focus(mut focus: ResMut<InputFocus>) {
    focus.clear();
}

/// Startup system: spawn the one [`UiRoot`] node and publish its entity.
pub(crate) fn spawn_ui_root(mut commands: Commands, direction: Res<UiDirection>) {
    let root = commands
        .spawn((
            Node {
                // The full window, so panels position themselves inside a known
                // box rather than against an implicit one.
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                // Panels stack down the block axis and hug their own content on
                // the inline axis (convention 2) — `Stretch`, the flexbox
                // default, would blow every child out to the full window width.
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                direction: direction.inline(),
                ..default()
            },
            // The root covers the whole window, and `bevy_picking` blocks lower
            // entities by default — so without this the root would swallow every
            // pointer hit aimed at the world behind it. The viewer's own object /
            // HUD pick raycasts by hand (`crate::hud_pick`) and so is unaffected
            // today, but `viewer-object-selection-core` is expected to move to a
            // picking backend, which this would silently kill. Still hoverable,
            // so a click on empty UI space routes `AcquireFocus`, finds no
            // focusable ancestor, and hands the keyboard back to the world.
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            // The default (non-modal) tab group: `Tab` cycles every `TabIndex`
            // under the root. A modal floater adds `TabGroup::modal()` of its own
            // to trap the cycle inside itself (`viewer-ui-floater-basic`).
            TabGroup::new(0),
            UiRootNode,
        ))
        .id();
    commands.insert_resource(UiRoot(root));
}

/// A box in **logical** (writing-mode-relative) terms — the CSS logical
/// properties, which `bevy_ui`'s physical [`UiRect`] has no equivalent of.
///
/// The block axis is not flipped by [`UiDirection`]: `direction` is an
/// *inline*-axis property, and vertical writing modes are not something either
/// `taffy` or Second Life has. `block_start` is therefore always the top. It is
/// still named logically so the vocabulary is one thing rather than two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogicalRect {
    /// The leading inline edge: `left` under [`UiDirection::Ltr`], `right` under
    /// [`UiDirection::Rtl`].
    pub(crate) inline_start: Val,
    /// The trailing inline edge: `right` under [`UiDirection::Ltr`], `left`
    /// under [`UiDirection::Rtl`].
    pub(crate) inline_end: Val,
    /// The leading block edge — the top.
    pub(crate) block_start: Val,
    /// The trailing block edge — the bottom.
    pub(crate) block_end: Val,
}

impl LogicalRect {
    /// Every edge zero — the identity for margin / padding / border.
    pub(crate) const ZERO: Self = Self::all(Val::ZERO);

    /// Every edge [`Val::Auto`] — the resting value for an **inset**, where
    /// `Auto` means "wherever flow put me" rather than "pinned to the edge". The
    /// base a [`LogicalInset`] overrides one or two edges of, so a floater that
    /// remembers only its leading / top position leaves the other edges to flow.
    pub(crate) const AUTO: Self = Self::all(Val::Auto);

    /// The same value on all four edges.
    pub(crate) const fn all(value: Val) -> Self {
        Self {
            inline_start: value,
            inline_end: value,
            block_start: value,
            block_end: value,
        }
    }

    /// One value per axis: `inline` on both inline edges, `block` on both block
    /// edges.
    ///
    /// The usual base for an asymmetric rect, overridden per edge with Rust's
    /// struct-update syntax — which is why there are no `with_edge` builders
    /// here, and why the fields are public:
    ///
    /// ```ignore
    /// LogicalRect {
    ///     inline_start: Val::Px(24.0), // a hanging indent on the leading side
    ///     ..LogicalRect::axes(Val::Px(8.0), Val::Px(4.0))
    /// }
    /// ```
    pub(crate) const fn axes(inline: Val, block: Val) -> Self {
        Self {
            inline_start: inline,
            inline_end: inline,
            block_start: block,
            block_end: block,
        }
    }

    /// This rect as the physical [`UiRect`] `bevy_ui` wants, for `direction`:
    /// under [`UiDirection::Rtl`] the two inline edges swap sides, which is the
    /// whole of the mirroring.
    pub(crate) const fn resolve(self, direction: UiDirection) -> UiRect {
        let (left, right) = if direction.is_rtl() {
            (self.inline_end, self.inline_start)
        } else {
            (self.inline_start, self.inline_end)
        };
        UiRect {
            left,
            right,
            top: self.block_start,
            bottom: self.block_end,
        }
    }
}

/// A node's margin, in logical terms. Resolved into `Node::margin` by
/// [`resolve_logical_boxes`]; omit it when the margin is symmetric and write
/// `Node::margin` directly.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogicalMargin(pub(crate) LogicalRect);

/// A node's padding, in logical terms. Resolved into `Node::padding` by
/// [`resolve_logical_boxes`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogicalPadding(pub(crate) LogicalRect);

/// A node's border widths, in logical terms. Resolved into `Node::border` by
/// [`resolve_logical_boxes`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogicalBorder(pub(crate) LogicalRect);

/// A node's **inset** — its `left` / `right` / `top` / `bottom` position — in
/// logical terms. Resolved into those four `Node` fields by
/// [`resolve_logical_boxes`].
///
/// The fourth physical box, and the last one to get a logical component, because
/// nothing positioned itself by inset until a floater needed to remember where it
/// was ([`viewer-ui-floater-basic`](crate::floater)). It differs from the other
/// three in exactly one way, and it is the load-bearing one: its resting value is
/// [`LogicalRect::AUTO`], not [`LogicalRect::ZERO`]. `Val::Auto` on an inset edge
/// means "leave this edge to flow", whereas `Val::ZERO` would **pin** the edge to
/// the container — a floater whose unset edges were zero would be stretched to
/// every side of the window rather than sitting where it was placed.
///
/// A floater carries only the two edges it means (leading + top, say) over an
/// `AUTO` base, so under [`UiDirection::Rtl`] its remembered leading offset
/// mirrors to the right edge for free, exactly as an asymmetric margin does.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogicalInset(pub(crate) LogicalRect);

/// Mark every logical box dirty when [`UiDirection`] flips, so
/// [`resolve_logical_boxes`] — which is otherwise driven by change detection on
/// the components alone — re-resolves the whole tree against the new direction.
///
/// Touching the components rather than widening the resolver's query is what
/// keeps that system to a single `&mut Node` query: several `Query<&mut Node>`
/// in one system are a conflicting access and would need a `ParamSet` to
/// untangle.
pub(crate) fn invalidate_logical_boxes(
    direction: Res<UiDirection>,
    mut margins: Query<&mut LogicalMargin>,
    mut paddings: Query<&mut LogicalPadding>,
    mut borders: Query<&mut LogicalBorder>,
    mut insets: Query<&mut LogicalInset>,
) {
    if !direction.is_changed() {
        return;
    }
    for mut margin in &mut margins {
        margin.set_changed();
    }
    for mut padding in &mut paddings {
        padding.set_changed();
    }
    for mut border in &mut borders {
        border.set_changed();
    }
    for mut inset in &mut insets {
        inset.set_changed();
    }
}

/// The nodes [`resolve_logical_boxes`] has work for: those carrying at least one
/// logical box that changed since it last ran, with each box optional because a
/// node may declare any subset of the three.
type ChangedLogicalBoxes<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static mut Node,
        Option<&'static LogicalMargin>,
        Option<&'static LogicalPadding>,
        Option<&'static LogicalBorder>,
        Option<&'static LogicalInset>,
    ),
    Or<(
        Changed<LogicalMargin>,
        Changed<LogicalPadding>,
        Changed<LogicalBorder>,
        Changed<LogicalInset>,
    )>,
>;

/// Fold each node's logical boxes into the physical `Node` fields `taffy` reads,
/// against the live [`UiDirection`].
///
/// Runs only for nodes whose logical boxes changed (or all of them, the frame
/// the direction flips — see [`invalidate_logical_boxes`]), and writes through
/// `Node`'s change detection only on a real difference, so an unchanged UI does
/// not re-trigger layout every frame.
pub(crate) fn resolve_logical_boxes(direction: Res<UiDirection>, mut nodes: ChangedLogicalBoxes) {
    for (mut node, margin, padding, border, inset) in &mut nodes {
        if let Some(LogicalMargin(rect)) = margin {
            let resolved = rect.resolve(*direction);
            if node.margin != resolved {
                node.margin = resolved;
            }
        }
        if let Some(LogicalPadding(rect)) = padding {
            let resolved = rect.resolve(*direction);
            if node.padding != resolved {
                node.padding = resolved;
            }
        }
        if let Some(LogicalBorder(rect)) = border {
            let resolved = rect.resolve(*direction);
            if node.border != resolved {
                node.border = resolved;
            }
        }
        if let Some(LogicalInset(rect)) = inset {
            // The inset is four separate `Val` fields on `Node` rather than a
            // `UiRect`, but the mirroring is identical — the resolved `left` /
            // `right` swap under RTL — so the same `LogicalRect::resolve` produces
            // it and the four fields are written out individually.
            let resolved = rect.resolve(*direction);
            if node.left != resolved.left {
                node.left = resolved.left;
            }
            if node.right != resolved.right {
                node.right = resolved.right;
            }
            if node.top != resolved.top {
                node.top = resolved.top;
            }
            if node.bottom != resolved.bottom {
                node.bottom = resolved.bottom;
            }
        }
    }
}

/// Write [`UiDirection`] onto every `Node`'s `direction`.
///
/// `taffy` has **no style inheritance** — it reads `direction` off each node's
/// own style, defaulting to `Ltr` — so setting it on [`UiRoot`] alone would
/// leave the entire tree left-to-right. Every node therefore carries it, and
/// this system keeps them all in step, both when the resource flips and when a
/// node is spawned mid-run (a floater opened while the UI is RTL must not come
/// up left-to-right).
///
/// Sweeping every node each frame rather than filtering on `Added` / `Changed`
/// is deliberate: the two would need separate `&mut Node` queries — a
/// conflicting access — and `bevy_ui` already walks every node each frame
/// anyway. The write is guarded, so an unchanged node does not re-trigger
/// layout.
///
/// A widget whose axes denote a **compass or world direction** rather than a
/// reading order — the radial menu, and later the minimap and world map — must not
/// mirror. It gets that not by exempting a node here but by not depending on this
/// at all: `crate::pie_menu` positions its labels by absolute inset from a compass
/// *angle* (`fit_pie_layout`), which this never touches, so the compass stays put
/// while each label's own text still shapes bidi. If a future widget cannot avoid
/// laying itself out through `direction`, a per-node opt-out belongs here.
pub(crate) fn apply_ui_direction(direction: Res<UiDirection>, mut nodes: Query<&mut Node>) {
    let target = direction.inline();
    for mut node in &mut nodes {
        if node.direction != target {
            node.direction = target;
        }
    }
}

/// Whether a panel subtree is currently shown. The scaffold's answer to "close
/// this panel", and what every toggleable surface should carry rather than
/// reaching for `Visibility` itself.
///
/// Hiding a panel correctly is three things, only the first of which is obvious:
///
/// 1. **`Display::None`, not `Visibility::Hidden`.** A hidden-but-displayed node
///    still occupies its slot in the parent's flow, so under [`UiRoot`]'s column
///    a closed panel would leave a panel-shaped hole above the open one.
/// 2. **Park the `TabIndex`.** `bevy_input_focus`'s tab navigation walks the
///    hierarchy and does **not** check visibility or display, so a closed
///    panel's buttons stay reachable by `Tab` — focus lands on something that is
///    not on screen. Nothing upstream does this for us.
/// 3. **Drop focus that is inside it.** Otherwise the closed panel keeps the
///    keyboard: keystrokes go on being typed into an editor nobody can see.
///
/// All three are [`apply_panel_visibility`]'s job.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiPanelShown(pub(crate) bool);

/// Where a hidden panel's `TabIndex` waits while the panel is closed, so
/// [`apply_panel_visibility`] can give each node back the index it had rather
/// than a guessed one.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ParkedTabIndex(TabIndex);

/// Apply a panel's [`UiPanelShown`] to its whole subtree: flow, tab reachability
/// and focus. See [`UiPanelShown`] for why each of the three is needed.
pub(crate) fn apply_panel_visibility(
    mut commands: Commands,
    panels: Query<(Entity, &UiPanelShown), Changed<UiPanelShown>>,
    mut nodes: Query<&mut Node>,
    children: Query<&Children>,
    tab_indices: Query<&TabIndex>,
    parked: Query<&ParkedTabIndex>,
    mut focus: ResMut<InputFocus>,
) {
    for (panel, shown) in &panels {
        if let Ok(mut node) = nodes.get_mut(panel) {
            let display = if shown.0 {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        // The panel itself can be focusable, so walk it as well as its children.
        for node in core::iter::once(panel).chain(children.iter_descendants(panel)) {
            if shown.0 {
                if let Ok(ParkedTabIndex(index)) = parked.get(node) {
                    commands
                        .entity(node)
                        .insert(*index)
                        .remove::<ParkedTabIndex>();
                }
            } else {
                if let Ok(index) = tab_indices.get(node) {
                    commands
                        .entity(node)
                        .insert(ParkedTabIndex(*index))
                        .remove::<TabIndex>();
                }
                if focus.get() == Some(node) {
                    focus.clear();
                }
            }
        }
    }
}

/// On a focusable widget, a wider entity whose bounds [`scroll_focus_into_view`]
/// should bring into view *together with* the focus stop itself — for a composite
/// widget whose focus stop is smaller than its meaningful visual whole.
///
/// A tab widget's focus stop is its header strip, but tabbing to it should reveal
/// the whole widget (strip + panel), so the strip carries this pointing at its
/// container. The reveal is the *union* of the two boxes, so the focus ring on
/// the small stop stays visible even when the whole does not fit, and it works on
/// both axes at once (a vertical tab strip, revealed horizontally, brings its
/// side panel in the same way).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FocusRevealBounds(pub(crate) Entity);

/// The scroll-offset delta, along one axis, that brings an item band fully into
/// a viewport band — the pure heart of [`scroll_focus_into_view`], split out so
/// it can be tested without a laid-out UI.
///
/// All four values are positions on the *same* axis in the *same* units (item and
/// viewport as currently laid out), the axis increasing in the direction a
/// growing [`ScrollPosition`] reveals — down / trailing. The result is the delta
/// to *add* to the scroll offset, or `None` when the item already fits so the
/// wheel is left alone:
///
/// - Item past the far (bottom / trailing) edge → a **positive** delta, scrolling
///   until the item's far edge meets the viewport's.
/// - Item past the near (top / leading) edge → a **negative** delta, scrolling
///   back until the near edges meet. The near edge wins when the item is taller
///   than the viewport (the reference "align the top" rule), so focus never lands
///   with the item's start clipped.
fn reveal_delta(viewport_min: f32, viewport_max: f32, item_min: f32, item_max: f32) -> Option<f32> {
    if item_min < viewport_min {
        // Above / before the viewport: scroll back so the near edges align.
        Some(item_min - viewport_min)
    } else if item_max > viewport_max {
        // Below / after the viewport: scroll forward so the far edges align.
        Some(item_max - viewport_max)
    } else {
        None
    }
}

/// Scroll the keyboard-focused widget into view (`viewer-ui-focus-scroll-into-
/// view`): when focus lands on a widget a scroll container has clipped off
/// screen, nudge that container's [`ScrollPosition`] just enough to reveal it.
///
/// Runs only for *keyboard* focus (`InputFocusVisible` true), matching the focus
/// ring — a mouse click cannot reach an off-screen widget anyway, so the scroll
/// is left alone for the pointer. It walks up from the focused entity to the
/// nearest ancestor that owns a `ScrollPosition` (a scroll container), compares
/// the two boxes as the layout currently has them, and moves the offset by
/// [`reveal_delta`] on each axis — a no-op when the widget already fits, so it
/// never fights the wheel. Like the focus ring, it is one scaffold system that a
/// new focusable widget in any scroll container gets for free.
///
/// `pub(crate)` so the gallery — which stands up the scaffold's systems by hand
/// rather than adding [`ViewerUiPlugin`] — can register it too.
pub(crate) fn scroll_focus_into_view(
    focus: Res<InputFocus>,
    focus_visible: Res<InputFocusVisible>,
    parents: Query<&ChildOf>,
    overflows: Query<&Node>,
    reveal_targets: Query<&FocusRevealBounds>,
    boxes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut containers: Query<&mut ScrollPosition>,
) {
    if !focus.is_changed() && !focus_visible.is_changed() {
        return;
    }
    if !focus_visible.0 {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Ok((focus_node, focus_transform)) = boxes.get(focused) else {
        return;
    };
    // The nearest *scrolling* ancestor. Every UI node carries a `ScrollPosition`
    // (it is a required component of `Node`), so a container is identified by its
    // `Overflow` actually being set to `Scroll` on an axis — not by the mere
    // presence of `ScrollPosition`, which would match the item's own parent and
    // so never scroll anything.
    let mut container = None;
    let mut current = focused;
    while let Ok(child_of) = parents.get(current) {
        let parent = child_of.parent();
        if overflows.get(parent).is_ok_and(scrolls_on_some_axis) {
            container = Some(parent);
            break;
        }
        current = parent;
    }
    let Some(container) = container else {
        return;
    };
    let Ok(container_style) = overflows.get(container) else {
        return;
    };
    let Ok((container_node, container_transform)) = boxes.get(container) else {
        return;
    };

    // The box to reveal: the focus stop, unioned with any wider bounds the widget
    // names (its whole self — a tab strip points at its container), so the ring on
    // the small stop stays visible while as much of the whole as fits is brought
    // in. Each box is centred on its world translation and sized in physical
    // pixels; worked per-component in `f32` on purpose, as `Vec2`'s own `+`/`*`
    // trip the workspace `arithmetic_side_effects` lint and primitive float ops do
    // not.
    let (reveal_node, reveal_transform) = reveal_targets
        .get(focused)
        .ok()
        .and_then(|bounds| boxes.get(bounds.0).ok())
        .unwrap_or((focus_node, focus_transform));
    let item_min_x = f32::min(
        focus_transform.translation.x - focus_node.size.x * 0.5,
        reveal_transform.translation.x - reveal_node.size.x * 0.5,
    );
    let item_max_x = f32::max(
        focus_transform.translation.x + focus_node.size.x * 0.5,
        reveal_transform.translation.x + reveal_node.size.x * 0.5,
    );
    let item_min_y = f32::min(
        focus_transform.translation.y - focus_node.size.y * 0.5,
        reveal_transform.translation.y - reveal_node.size.y * 0.5,
    );
    let item_max_y = f32::max(
        focus_transform.translation.y + focus_node.size.y * 0.5,
        reveal_transform.translation.y + reveal_node.size.y * 0.5,
    );

    let view_centre = container_transform.translation;
    let view_half_x = container_node.size.x * 0.5;
    let view_half_y = container_node.size.y * 0.5;

    // Only the axes the container actually scrolls: a `reveal_delta` on a
    // non-scrolling axis would be a no-op in `bevy_ui` but is clearer skipped.
    let delta_x = (container_style.overflow.x == OverflowAxis::Scroll)
        .then(|| {
            reveal_delta(
                view_centre.x - view_half_x,
                view_centre.x + view_half_x,
                item_min_x,
                item_max_x,
            )
        })
        .flatten();
    let delta_y = (container_style.overflow.y == OverflowAxis::Scroll)
        .then(|| {
            reveal_delta(
                view_centre.y - view_half_y,
                view_centre.y + view_half_y,
                item_min_y,
                item_max_y,
            )
        })
        .flatten();
    if delta_x.is_none() && delta_y.is_none() {
        return;
    }

    let Ok(mut scroll) = containers.get_mut(container) else {
        return;
    };
    // `ScrollPosition` is in logical pixels; the boxes are physical, so scale the
    // delta down. `bevy_ui` clamps the far end at layout; the near end is 0.
    let inverse_scale = container_node.inverse_scale_factor;
    if let Some(delta) = delta_x {
        scroll.0.x = (scroll.0.x + delta * inverse_scale).max(0.0);
    }
    if let Some(delta) = delta_y {
        scroll.0.y = (scroll.0.y + delta * inverse_scale).max(0.0);
    }
}

/// Whether a node's [`Overflow`] scrolls on at least one axis — the test for "is
/// this a scroll container", used by [`scroll_focus_into_view`] because every
/// `Node` has a `ScrollPosition` but only ones with `Overflow::scroll` move.
fn scrolls_on_some_axis(node: &Node) -> bool {
    node.overflow.x == OverflowAxis::Scroll || node.overflow.y == OverflowAxis::Scroll
}

/// A container whose children stack along the **block** axis (top to bottom),
/// separated by `gap`, sized to its content.
///
/// The convention-2 constructor: it sets flow and spacing and touches no size,
/// so the container is exactly as large as its children need. Override
/// `max_width` (to force wrapping) or `min_width` (to stop a container
/// collapsing) on the result when a panel genuinely needs a bound — never
/// `width` and `height` together, which is the fixed rect the convention exists
/// to prevent.
pub(crate) fn column(gap: Val) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        // The gap between rows of a column is the *row* gap; `column_gap` here
        // would space the (single) column against nothing and silently do
        // nothing. Getting this backwards is the classic flexbox slip, so it is
        // made once, here.
        row_gap: gap,
        ..default()
    }
}

/// A container whose children flow along the **inline** axis (in text order,
/// so right-to-left under [`UiDirection::Rtl`]), separated by `gap`, sized to
/// its content.
///
/// The inline-axis counterpart of [`column()`]; the same sizing rule applies.
/// Note that `FlexDirection::Row` is already the logical "along the text
/// direction" and needs no mirroring of its own — `taffy` reverses the flow off
/// the node's `direction`, which [`apply_ui_direction`] keeps current.
pub(crate) fn row(gap: Val) -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        // Mirror of `column`: the gap between items of a row is the *column* gap.
        column_gap: gap,
        ..default()
    }
}

// ---------------------------------------------------------------------------
// The scaffold's proof surface.
//
// A toggleable panel (`F5`, or `SL_VIEWER_UI_DEMO` for the screenshot harness)
// that exercises every part of the scaffold by hand, in the pattern the
// neighbouring foundations already use (`crate::ui_text`'s `F4` text panel,
// `crate::diagnostics`' `F3` pipeline overlay). It is not a widget library —
// the generic primitives come from `bevy_ui_widgets`, and the viewer-domain
// composites are each their own task — it is the thing that makes the
// scaffold's four claims falsifiable:
//
// - **Tab navigation works.** Two `bevy_ui_widgets` buttons carry a `TabIndex`;
//   `Tab` / `Shift+Tab` cycle them (and `crate::ui_text`'s editor, when that
//   panel is open too), `Enter` / `Space` activate the focused one. With one
//   focusable node in the whole viewer this could not be tested at all.
// - **RTL mirroring works, live.** The direction button flips [`UiDirection`] at
//   runtime, so the whole tree re-mirrors — a stronger claim than the
//   start-up-only `SL_VIEWER_UI_DIRECTION`, and the one downstream panels rely
//   on when a locale changes.
// - **Logical boxes mirror.** The sample label's accent bar and hanging indent
//   are asymmetric, so which side they land on is visible at a glance.
// - **Layout is content-driven.** The label button swaps the sample text
//   between short and long; the panel regrows around it with no size to update.
// ---------------------------------------------------------------------------

/// The key that toggles the scaffold demo panel on and off.
const UI_DEMO_TOGGLE_KEY: KeyCode = KeyCode::F5;

/// The environment variable that starts the demo panel shown, for the offline
/// screenshot harness (which cannot press [`UI_DEMO_TOGGLE_KEY`]).
const UI_DEMO_ENV: &str = "SL_VIEWER_UI_DEMO";

/// The demo panel's instruction-line font size, in logical pixels.
const DEMO_TITLE_FONT_SIZE: f32 = 13.0;

/// The demo panel's margin, in logical pixels, from the leading inline edge of
/// [`UiRoot`] — clear of the top-leading pipeline overlay, like the text panel.
const DEMO_PANEL_MARGIN: f32 = 90.0;

/// The width, in logical pixels, of the sample label's leading accent bar.
const ACCENT_BAR_WIDTH: f32 = 4.0;

/// The sample label's hanging indent, in logical pixels: a wide leading padding
/// against a narrow trailing one, so the asymmetry is unmistakable when it
/// mirrors.
const HANGING_INDENT: f32 = 24.0;

/// The one-line instruction shown above the demo's controls.
const UI_DEMO_TITLE: &str = "UI scaffold demo (F5) - Tab / Shift+Tab walk the three buttons in \
     order (and F4's editor, when that panel is open); Enter or Space activates the focused one. \
     Flip the direction and watch the panel, the button order and the accent bar all mirror; grow \
     the text or the label and watch the panel reflow around it.";

/// The sample label's short text.
const SAMPLE_SHORT: &str = "A short label.";

/// The sample label's long text — long enough to wrap inside the panel's
/// `max_width`, so the panel visibly regrows around it.
const SAMPLE_LONG: &str = "A much longer label, of the length a translated string reaches when the \
     original was written in English and measured once, which is exactly the case a fixed pixel \
     rect gets wrong.";

/// Whether the scaffold demo panel is currently shown. Toggled by
/// [`UI_DEMO_TOGGLE_KEY`]; hidden by default so it stays out of the way.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct UiDemoVisible(bool);

impl UiDemoVisible {
    /// The initial visibility, seeded from [`UI_DEMO_ENV`]: set to start shown,
    /// unset to start hidden (the interactive default).
    fn from_env() -> Self {
        Self(std::env::var_os(UI_DEMO_ENV).is_some())
    }
}

/// Whether the demo's sample label is currently showing its long text.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct UiDemoLabelLong(bool);

/// The size the demo's own text is set at, cycled by its third button.
///
/// A step on the same cross-cutting claim as the label-length swap, from the
/// other side: a panel must reflow around a **font-size** change as readily as
/// around a longer string, because the two break a fixed pixel rect the same
/// way. A viewer UI font-size preference (`viewer-preferences-graphics-tab`) is
/// exactly this, applied for real.
///
/// An enum of named steps rather than an index into a table of sizes: the cycle
/// is then total and needs no arithmetic or bounds check to advance.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
enum UiDemoTextSize {
    /// The demo's resting size.
    #[default]
    Medium,
    /// Large enough that the button row outgrows the panel and wraps.
    Large,
    /// Large enough to be obviously wrong for a fixed rect.
    Huge,
    /// Back down the other side of the cycle.
    Small,
}

impl UiDemoTextSize {
    /// This step, in logical pixels.
    const fn px(self) -> f32 {
        match self {
            Self::Small => 11.0,
            Self::Medium => 15.0,
            Self::Large => 22.0,
            Self::Huge => 30.0,
        }
    }

    /// The next step in the cycle.
    const fn next(self) -> Self {
        match self {
            Self::Medium => Self::Large,
            Self::Large => Self::Huge,
            Self::Huge => Self::Small,
            Self::Small => Self::Medium,
        }
    }

    /// This step's button label.
    const fn label(self) -> &'static str {
        match self {
            Self::Small => "Text: 11 px",
            Self::Medium => "Text: 15 px",
            Self::Large => "Text: 22 px",
            Self::Huge => "Text: 30 px",
        }
    }
}

/// A marker on the demo panel's root node.
#[derive(Component, Debug, Clone, Copy)]
struct UiDemoRoot;

/// A marker on both of the demo's buttons, so a test can find them (e.g. to
/// assert each carries a `TabIndex`).
#[derive(Component, Debug, Clone, Copy)]
struct UiDemoButton;

/// Which of the demo's four pieces of live text a `Text` node is.
///
/// One marker carrying a discriminant rather than four marker components, so
/// [`update_ui_demo_text`] is a single query instead of four mutually
/// `Without`-filtered ones over the same `&mut Text`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum UiDemoText {
    /// The direction button's label, reporting the live [`UiDirection`].
    DirectionButton,
    /// The length button's label, reporting the live [`UiDemoLabelLong`].
    LengthButton,
    /// The size button's label, reporting the live [`UiDemoTextSize`].
    SizeButton,
    /// The sample label whose length the length button swaps.
    Sample,
}

/// The demo panel's translucent backdrop, matching the text panel's.
const DEMO_PANEL_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

/// The demo's instruction-line colour.
const DEMO_TITLE_COLOR: Color = Color::srgb(0.80, 0.85, 0.92);

/// A demo button's background.
const DEMO_BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A demo button's border. The keyboard focus ring is now the shared outline
/// the skin draws on any focusable widget (`viewer-ui-focus-ring-visible`), not
/// a recolour of this border.
const DEMO_BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The sample label's leading accent bar.
const DEMO_ACCENT_COLOR: Color = Color::srgb(0.36, 0.72, 0.98);

/// Startup system: spawn the demo panel under [`UiRoot`], so it must run after
/// [`UiScaffoldSystems::SpawnRoot`].
fn setup_ui_demo(mut commands: Commands, visible: Res<UiDemoVisible>, root: Res<UiRoot>) {
    let display = if visible.0 {
        Display::Flex
    } else {
        Display::None
    };
    commands
        .spawn((
            Node {
                display,
                padding: UiRect::all(Val::Px(12.0)),
                // A bound, not a size: the long sample text wraps here, and the
                // panel is narrower than this when the short one is showing.
                max_width: Val::Px(560.0),
                ..column(Val::Px(8.0))
            },
            // Asymmetric, so flipping the direction visibly walks the whole panel
            // across the window.
            LogicalMargin(LogicalRect {
                inline_start: Val::Px(DEMO_PANEL_MARGIN),
                block_start: Val::Px(DEMO_PANEL_MARGIN),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(DEMO_PANEL_BACKGROUND),
            UiPanelShown(visible.0),
            UiDemoRoot,
            ChildOf(root.0),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(UI_DEMO_TITLE),
                UiFont::Sans.at(DEMO_TITLE_FONT_SIZE),
                TextColor(DEMO_TITLE_COLOR),
            ));
            // A `row`, so the buttons flow in text order: under RTL they swap
            // ends with no code here saying so. It wraps rather than overflowing
            // once the text size outgrows the panel — the row-level half of
            // convention 2, and `row_gap` (the space *between* wrapped lines) is
            // the one gap `row`'s own `column_gap` cannot mean.
            panel
                .spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(8.0),
                    ..row(Val::Px(8.0))
                })
                .with_children(|buttons| {
                    buttons
                        .spawn(demo_button(1))
                        .with_child((
                            Text::default(),
                            UiFont::Sans.at(UiDemoTextSize::default().px()),
                            TextColor(Color::WHITE),
                            UiDemoText::DirectionButton,
                        ))
                        .observe(flip_ui_direction);
                    buttons
                        .spawn(demo_button(2))
                        .with_child((
                            Text::default(),
                            UiFont::Sans.at(UiDemoTextSize::default().px()),
                            TextColor(Color::WHITE),
                            UiDemoText::LengthButton,
                        ))
                        .observe(flip_sample_length);
                    buttons
                        .spawn(demo_button(3))
                        .with_child((
                            Text::default(),
                            UiFont::Sans.at(UiDemoTextSize::default().px()),
                            TextColor(Color::WHITE),
                            UiDemoText::SizeButton,
                        ))
                        .observe(cycle_text_size);
                });
            // The decoration goes on a container and the text is a plain child —
            // do not collapse the two back together. Padding or a border applied
            // *directly* to a `Text` node makes `bevy_ui`'s text measure resolve
            // the wrong available width: it came out 12 logical px too wide (the
            // trailing padding plus the border), which cost the wrap one line, so
            // the node was laid out one line shorter than the text it drew and
            // the last line's descenders hung out below the panel. Measured on
            // this very panel: the text node reported `size.y = 94` against
            // `content_size.y = 121` (physical), and wrapping it here made the two
            // agree. Decorating a container rather than a text run is the right
            // structure anyway — a text run is not a box.
            panel
                .spawn((
                    Node::default(),
                    // An accent bar on the leading edge only — a border on one
                    // side, which is exactly the kind of thing a physical
                    // `UiRect::left` would silently strand on the wrong side of
                    // an RTL layout.
                    LogicalBorder(LogicalRect {
                        inline_start: Val::Px(ACCENT_BAR_WIDTH),
                        ..LogicalRect::ZERO
                    }),
                    // A hanging indent: wide on the leading side, narrow elsewhere.
                    LogicalPadding(LogicalRect {
                        inline_start: Val::Px(HANGING_INDENT),
                        ..LogicalRect::axes(Val::Px(8.0), Val::Px(4.0))
                    }),
                    BorderColor::all(DEMO_ACCENT_COLOR),
                ))
                .with_child((
                    Text::default(),
                    UiFont::Sans.at(UiDemoTextSize::default().px()),
                    TextColor(Color::WHITE),
                    UiDemoText::Sample,
                ));
        });
}

/// The bundle shared by both demo buttons: a headless `bevy_ui_widgets` button
/// that is focusable at `tab_index`, sized to its label.
fn demo_button(tab_index: i32) -> impl Bundle {
    (
        Button,
        TabIndex(tab_index),
        UiDemoButton,
        Node {
            padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BorderColor::all(DEMO_BUTTON_BORDER),
        BackgroundColor(DEMO_BUTTON_BACKGROUND),
    )
}

/// Observer: flip [`UiDirection`] when the direction button is activated —
/// clicked, or `Enter` / `Space` while it holds focus.
///
/// The whole tree re-mirrors from this one write, which is the claim the
/// scaffold is making.
fn flip_ui_direction(_activate: On<Activate>, mut direction: ResMut<UiDirection>) {
    *direction = match *direction {
        UiDirection::Ltr => UiDirection::Rtl,
        UiDirection::Rtl => UiDirection::Ltr,
    };
}

/// Observer: swap the sample label between its short and long text, so the panel
/// visibly regrows around content it was never sized for.
fn flip_sample_length(_activate: On<Activate>, mut long: ResMut<UiDemoLabelLong>) {
    long.0 = !long.0;
}

/// Observer: step the demo's text size, so the panel — and, at the larger steps,
/// the button row that wraps — reflows around type it was never measured for.
fn cycle_text_size(_activate: On<Activate>, mut size: ResMut<UiDemoTextSize>) {
    *size = size.next();
}

/// Keep the demo's live text in step with the state it reports: each button says
/// what it currently is, the sample label shows the text the length button
/// selected, and all four are set at the size the size button selected.
///
/// Text and size are one system because they are one query over the same
/// `&mut Text` entities, and both are why the panel reflows.
fn update_ui_demo_text(
    direction: Res<UiDirection>,
    long: Res<UiDemoLabelLong>,
    size: Res<UiDemoTextSize>,
    mut texts: Query<(&mut Text, &mut TextFont, &UiDemoText)>,
) {
    if !direction.is_changed() && !long.is_changed() && !size.is_changed() {
        return;
    }
    let font_size = FontSize::Px(size.px());
    for (mut text, mut font, which) in &mut texts {
        let wanted = match *which {
            UiDemoText::DirectionButton if direction.is_rtl() => "Direction: RTL",
            UiDemoText::DirectionButton => "Direction: LTR",
            UiDemoText::LengthButton if long.0 => "Label: long",
            UiDemoText::LengthButton => "Label: short",
            UiDemoText::SizeButton => size.label(),
            UiDemoText::Sample if long.0 => SAMPLE_LONG,
            UiDemoText::Sample => SAMPLE_SHORT,
        };
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
        if font.font_size != font_size {
            font.font_size = font_size;
        }
    }
}

/// Toggle the demo panel when [`UI_DEMO_TOGGLE_KEY`] is pressed.
fn toggle_ui_demo(keyboard: Res<ButtonInput<KeyCode>>, mut visible: ResMut<UiDemoVisible>) {
    if keyboard.just_pressed(UI_DEMO_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Drive the demo panel's [`UiPanelShown`] from [`UiDemoVisible`], leaving
/// [`apply_panel_visibility`] to do the actual hiding.
fn apply_ui_demo_visibility(
    visible: Res<UiDemoVisible>,
    mut panels: Query<&mut UiPanelShown, With<UiDemoRoot>>,
) {
    if !visible.is_changed() {
        return;
    }
    for mut shown in &mut panels {
        if shown.0 != visible.0 {
            shown.0 = visible.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LogicalBorder, LogicalInset, LogicalMargin, LogicalPadding, LogicalRect, UiDemoButton,
        UiDemoLabelLong, UiDemoRoot, UiDemoTextSize, UiDemoVisible, UiDirection, UiPanelShown,
        UiRoot, UiRootNode, ViewerUiPlugin, apply_panel_visibility, apply_ui_direction, column,
        invalidate_logical_boxes, resolve_logical_boxes, reveal_delta, row, setup_ui_demo,
        spawn_ui_root,
    };
    use bevy::input_focus::tab_navigation::{TabGroup, TabIndex, TabNavigationPlugin};
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsStr;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// An asymmetric rect: a different value on each edge, so a resolve that
    /// swapped the wrong pair, or dropped one, is visible.
    const ASYMMETRIC: LogicalRect = LogicalRect {
        inline_start: Val::Px(1.0),
        inline_end: Val::Px(2.0),
        block_start: Val::Px(3.0),
        block_end: Val::Px(4.0),
    };

    /// An item already wholly inside the viewport — including flush against both
    /// edges — needs no scroll, so the wheel is left alone.
    #[test]
    fn reveal_delta_leaves_a_visible_item_alone() {
        assert_eq!(reveal_delta(0.0, 100.0, 10.0, 90.0), None);
        assert_eq!(reveal_delta(0.0, 100.0, 0.0, 100.0), None);
    }

    /// An item past the far edge scrolls forward by exactly the overshoot, so its
    /// far edge lands on the viewport's.
    #[test]
    fn reveal_delta_scrolls_forward_for_an_item_past_the_far_edge() {
        assert_eq!(reveal_delta(0.0, 100.0, 30.0, 120.0), Some(20.0));
    }

    /// An item before the near edge scrolls back by the (negative) shortfall, so
    /// its near edge lands on the viewport's.
    #[test]
    fn reveal_delta_scrolls_back_for_an_item_before_the_near_edge() {
        assert_eq!(reveal_delta(0.0, 100.0, -15.0, 40.0), Some(-15.0));
    }

    /// When the item is taller than the viewport, the near edge wins (align the
    /// top) rather than the far edge, so focus never lands with its start clipped.
    #[test]
    fn reveal_delta_aligns_the_near_edge_when_the_item_is_taller_than_the_viewport() {
        assert_eq!(reveal_delta(0.0, 100.0, -10.0, 150.0), Some(-10.0));
    }

    /// Left-to-right is the identity mapping: inline start is left, inline end
    /// is right, and the block axis is top / bottom either way.
    #[test]
    fn ltr_resolves_inline_start_to_left() {
        assert_eq!(
            ASYMMETRIC.resolve(UiDirection::Ltr),
            UiRect {
                left: Val::Px(1.0),
                right: Val::Px(2.0),
                top: Val::Px(3.0),
                bottom: Val::Px(4.0),
            }
        );
    }

    /// Right-to-left swaps the two **inline** edges and only those: the block
    /// axis must not flip, because `direction` is an inline-axis property (there
    /// is no vertical writing mode here).
    #[test]
    fn rtl_swaps_only_the_inline_edges() {
        assert_eq!(
            ASYMMETRIC.resolve(UiDirection::Rtl),
            UiRect {
                left: Val::Px(2.0),
                right: Val::Px(1.0),
                top: Val::Px(3.0),
                bottom: Val::Px(4.0),
            }
        );
    }

    /// Resolving is an involution on the inline axis: mirroring twice is the
    /// identity. A rect that survives the round trip cannot have leaked an edge
    /// into the wrong field.
    #[test]
    fn mirroring_twice_is_the_identity() {
        let once = ASYMMETRIC.resolve(UiDirection::Rtl);
        let mirrored_back = LogicalRect {
            inline_start: once.right,
            inline_end: once.left,
            block_start: once.top,
            block_end: once.bottom,
        };
        assert_eq!(mirrored_back, ASYMMETRIC);
    }

    /// A symmetric rect is direction-independent — which is exactly why a
    /// symmetric box needs no logical component at all and can be written
    /// straight onto `Node`.
    #[test]
    fn symmetric_rects_resolve_the_same_either_way() {
        for rect in [
            LogicalRect::ZERO,
            LogicalRect::all(Val::Px(6.0)),
            LogicalRect::all(Val::Auto),
            LogicalRect::axes(Val::Px(8.0), Val::Px(4.0)),
            LogicalRect::axes(Val::Percent(5.0), Val::ZERO),
        ] {
            assert_eq!(
                rect.resolve(UiDirection::Ltr),
                rect.resolve(UiDirection::Rtl),
                "{rect:?} is symmetric, so it must not depend on the direction"
            );
        }
    }

    /// The convention-2 constructors put the gap on the axis children actually
    /// stack along, and leave sizing entirely alone.
    #[test]
    fn layout_constructors_are_content_sized() {
        let gap = Val::Px(8.0);
        let column = column(gap);
        assert_eq!(column.flex_direction, FlexDirection::Column);
        assert_eq!(column.row_gap, gap, "a column is spaced by its row gap");
        assert_eq!(column.column_gap, Val::ZERO);

        let row = row(gap);
        assert_eq!(row.flex_direction, FlexDirection::Row);
        assert_eq!(row.column_gap, gap, "a row is spaced by its column gap");
        assert_eq!(row.row_gap, Val::ZERO);

        for node in [column, row] {
            assert_eq!(node.width, Val::Auto, "a container must size to content");
            assert_eq!(node.height, Val::Auto, "a container must size to content");
            assert_eq!(node.min_width, Val::Auto);
            assert_eq!(node.max_width, Val::Auto);
        }
    }

    /// A logical inset over an `AUTO` base — the floater case: a remembered
    /// leading and top position, the trailing and bottom edges left to flow. The
    /// leading edge must mirror under RTL while the two `Auto` edges stay `Auto`
    /// (a resolved `Val::ZERO` there would pin the node to two sides of the
    /// window rather than leaving it where it was placed).
    #[test]
    fn a_logical_inset_mirrors_its_placed_edges_and_leaves_auto_alone() -> Result<(), TestError> {
        let placed = LogicalRect {
            inline_start: Val::Px(40.0),
            block_start: Val::Px(60.0),
            ..LogicalRect::AUTO
        };
        for (direction, want_left, want_right) in [
            (UiDirection::Ltr, Val::Px(40.0), Val::Auto),
            (UiDirection::Rtl, Val::Auto, Val::Px(40.0)),
        ] {
            let mut app = scaffold_app(direction);
            let node = app
                .world_mut()
                .spawn((Node::default(), LogicalInset(placed)))
                .id();
            app.update();

            let node = app
                .world()
                .get::<Node>(node)
                .ok_or("the spawned node lost its `Node`")?;
            assert_eq!(node.left, want_left, "{direction:?}: leading inset -> left");
            assert_eq!(
                node.right, want_right,
                "{direction:?}: leading inset -> right under RTL"
            );
            assert_eq!(
                node.top,
                Val::Px(60.0),
                "{direction:?}: the top is not flipped"
            );
            assert_eq!(
                node.bottom,
                Val::Auto,
                "{direction:?}: an unset (Auto) inset edge must stay Auto, never become zero"
            );
        }
        Ok(())
    }

    /// A minimal app carrying just the scaffold's own state and systems — the
    /// full [`ViewerUiPlugin`] would drag in `bevy_ui`'s rendering and a window.
    fn scaffold_app(direction: UiDirection) -> App {
        let mut app = App::new();
        app.insert_resource(direction).add_systems(
            Update,
            (
                invalidate_logical_boxes,
                resolve_logical_boxes,
                apply_ui_direction,
            )
                .chain(),
        );
        app
    }

    /// Every logical box lands in the physical field it names, for both
    /// directions — the resolver's whole job, driven through a real `App` rather
    /// than by calling `resolve` directly.
    #[test]
    fn the_resolver_writes_every_logical_box_onto_the_node() -> Result<(), TestError> {
        for (direction, want_start, want_end) in [
            (UiDirection::Ltr, Val::Px(1.0), Val::Px(2.0)),
            (UiDirection::Rtl, Val::Px(2.0), Val::Px(1.0)),
        ] {
            let mut app = scaffold_app(direction);
            let node = app
                .world_mut()
                .spawn((
                    Node::default(),
                    LogicalMargin(ASYMMETRIC),
                    LogicalPadding(ASYMMETRIC),
                    LogicalBorder(ASYMMETRIC),
                ))
                .id();
            app.update();

            let node = app
                .world()
                .get::<Node>(node)
                .ok_or("the spawned node lost its `Node`")?;
            for (name, rect) in [
                ("margin", node.margin),
                ("padding", node.padding),
                ("border", node.border),
            ] {
                assert_eq!(rect.left, want_start, "{direction:?} {name}: left edge");
                assert_eq!(rect.right, want_end, "{direction:?} {name}: right edge");
                assert_eq!(rect.top, Val::Px(3.0), "{direction:?} {name}: top edge");
                assert_eq!(
                    rect.bottom,
                    Val::Px(4.0),
                    "{direction:?} {name}: bottom edge"
                );
            }
        }
        Ok(())
    }

    /// Flipping the direction at runtime re-mirrors a tree that was already
    /// spawned and resolved. This is the case change detection alone would miss
    /// — the components did not change, the resource did — and the reason
    /// `invalidate_logical_boxes` exists.
    #[test]
    fn flipping_the_direction_re_mirrors_an_existing_tree() -> Result<(), TestError> {
        let mut app = scaffold_app(UiDirection::Ltr);
        let node = app
            .world_mut()
            .spawn((Node::default(), LogicalPadding(ASYMMETRIC)))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<Node>(node)
                .ok_or("the spawned node lost its `Node`")?
                .padding
                .left,
            Val::Px(1.0),
            "the leading inline padding starts on the left under LTR"
        );

        app.insert_resource(UiDirection::Rtl);
        app.update();

        let node = app
            .world()
            .get::<Node>(node)
            .ok_or("the spawned node lost its `Node`")?;
        assert_eq!(
            node.padding.left,
            Val::Px(2.0),
            "flipping to RTL must move the leading padding to the right edge"
        );
        assert_eq!(node.padding.right, Val::Px(1.0));
        assert_eq!(
            node.direction,
            InlineDirection::Rtl,
            "flipping the resource must re-point the node's own layout direction"
        );
        Ok(())
    }

    /// `taffy` reads `direction` per node and never inherits it, so a node
    /// spawned *after* the flip — a floater opened while the UI is RTL — must
    /// still come up mirrored rather than laying out left-to-right.
    #[test]
    fn a_node_spawned_later_still_picks_up_an_rtl_direction() -> Result<(), TestError> {
        let mut app = scaffold_app(UiDirection::Rtl);
        app.update();
        let late = app.world_mut().spawn(Node::default()).id();
        app.update();
        assert_eq!(
            app.world()
                .get::<Node>(late)
                .ok_or("the spawned node lost its `Node`")?
                .direction,
            InlineDirection::Rtl,
            "a node spawned after startup must inherit the UI direction; taffy has no \
             style inheritance, so nothing else would give it one"
        );
        Ok(())
    }

    /// The direction seed only accepts `rtl`, case-insensitively; anything else
    /// — including a typo, and including unset — is left-to-right, because a
    /// misspelt debug variable must not silently mirror the whole UI.
    #[test]
    fn the_direction_env_var_only_accepts_rtl() {
        for (value, want) in [
            (Some("rtl"), UiDirection::Rtl),
            (Some("RTL"), UiDirection::Rtl),
            (Some("Rtl"), UiDirection::Rtl),
            (Some("ltr"), UiDirection::Ltr),
            (Some("rtl "), UiDirection::Ltr),
            (Some(""), UiDirection::Ltr),
            (None, UiDirection::Ltr),
        ] {
            assert_eq!(
                UiDirection::parse(value.map(OsStr::new)),
                want,
                "{value:?} should seed {want:?}"
            );
        }
    }

    /// The *override* read (which the i18n scaffold uses) has a third answer the
    /// seed cannot: `None` when unset, so a non-Latin locale drives the layout,
    /// but an explicit `rtl` / `ltr` still forces its side.
    #[test]
    fn the_direction_override_distinguishes_unset_from_a_forced_side() {
        for (value, want) in [
            (Some("rtl"), Some(UiDirection::Rtl)),
            (Some("RTL"), Some(UiDirection::Rtl)),
            (Some("ltr"), Some(UiDirection::Ltr)),
            (Some("LTR"), Some(UiDirection::Ltr)),
            // Unset, or a typo, is "no override" — not a forced left-to-right —
            // so the locale is free to mirror the layout.
            (Some("sideways"), None),
            (None, None),
        ] {
            assert_eq!(
                UiDirection::parse_override(value.map(OsStr::new)),
                want,
                "{value:?} should override to {want:?}"
            );
        }
    }

    /// The root exists, is published as a resource, and carries the pieces every
    /// downstream panel depends on: a tab group to be navigated within, a
    /// `Pickable` that does not swallow the world behind it, and a column flow
    /// that lets children keep their own size.
    #[test]
    fn the_ui_root_is_spawned_with_the_scaffold_contract() -> Result<(), TestError> {
        let mut app = App::new();
        app.insert_resource(UiDirection::Ltr)
            .add_systems(Startup, spawn_ui_root);
        app.update();

        let root = app
            .world()
            .get_resource::<UiRoot>()
            .ok_or("`spawn_ui_root` did not publish the `UiRoot` resource")?
            .0;
        assert!(
            app.world().get::<UiRootNode>(root).is_some(),
            "the published entity must be the marked root node"
        );
        assert!(
            app.world().get::<TabGroup>(root).is_some(),
            "the root must be a tab group, or nothing under it is reachable by `Tab`"
        );
        let pickable = app
            .world()
            .get::<Pickable>(root)
            .ok_or("the root must carry an explicit `Pickable`")?;
        assert!(
            !pickable.should_block_lower,
            "a full-window root that blocks lower entities swallows every world pick"
        );
        let node = app
            .world()
            .get::<Node>(root)
            .ok_or("the root lost its `Node`")?;
        assert_eq!(node.flex_direction, FlexDirection::Column);
        assert_eq!(
            node.align_items,
            AlignItems::Start,
            "the flexbox default (`Stretch`) would blow every panel out to the window width"
        );
        Ok(())
    }

    /// Closing a panel must do all three things [`UiPanelShown`] promises, and
    /// re-opening it must undo all three. The middle one is the load-bearing
    /// surprise: `bevy_input_focus` walks the hierarchy without consulting
    /// `Display` or `Visibility`, so nothing upstream stops `Tab` landing on a
    /// button inside a closed panel.
    #[test]
    fn closing_a_panel_removes_it_from_flow_focus_and_the_tab_cycle() -> Result<(), TestError> {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .add_systems(Update, apply_panel_visibility);
        let panel = app
            .world_mut()
            .spawn((Node::default(), UiPanelShown(true)))
            .id();
        let button = app
            .world_mut()
            .spawn((Node::default(), TabIndex(3), ChildOf(panel)))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(button, FocusCause::Navigated);
        app.update();
        assert!(
            app.world().get::<TabIndex>(button).is_some(),
            "an open panel's button must stay in the tab cycle"
        );

        app.world_mut()
            .get_mut::<UiPanelShown>(panel)
            .ok_or("the panel lost its `UiPanelShown`")?
            .0 = false;
        app.update();

        assert_eq!(
            app.world()
                .get::<Node>(panel)
                .ok_or("the panel lost its `Node`")?
                .display,
            Display::None,
            "a closed panel must leave the flow, or it holds a panel-shaped hole open"
        );
        assert!(
            app.world().get::<TabIndex>(button).is_none(),
            "a closed panel's button must leave the tab cycle: `Tab` does not check display"
        );
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            None,
            "a closed panel must give up the keyboard, or it goes on receiving what is typed"
        );

        app.world_mut()
            .get_mut::<UiPanelShown>(panel)
            .ok_or("the panel lost its `UiPanelShown`")?
            .0 = true;
        app.update();

        assert_eq!(
            app.world().get::<TabIndex>(button).copied(),
            Some(TabIndex(3)),
            "re-opening must give the button back the tab index it had, not a guessed one"
        );
        assert_eq!(
            app.world()
                .get::<Node>(panel)
                .ok_or("the panel lost its `Node`")?
                .display,
            Display::Flex
        );
        Ok(())
    }

    /// The demo's text-size cycle visits every step and closes: four `next`s
    /// return to the start, no step repeats, and no two steps are the same size.
    /// A cycle written as an index into a size table could silently skip or
    /// repeat a step; the enum cannot, and this is what says so.
    #[test]
    fn the_text_size_cycle_visits_every_step_and_closes() {
        let mut size = UiDemoTextSize::default();
        let mut steps = Vec::new();
        for _step in 0..4 {
            steps.push(size);
            size = size.next();
        }
        assert_eq!(
            size,
            UiDemoTextSize::default(),
            "four steps must close the cycle"
        );

        let mut distinct = steps.clone();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            4,
            "the cycle must not repeat a step: {steps:?}"
        );

        // Compared as bits, so distinctness is exact and no float comparison is
        // involved.
        let mut sizes: Vec<u32> = steps.iter().map(|step| step.px().to_bits()).collect();
        sizes.sort_unstable();
        sizes.dedup();
        assert_eq!(
            sizes.len(),
            4,
            "every step must be a distinct size: {steps:?}"
        );
    }

    /// The demo panel offers three tab stops, at consecutive indices after the
    /// text panel's editor (which takes 0).
    ///
    /// Three is the point, not two: with only two focusable nodes a cycle is its
    /// own reverse, so `Tab` and `Shift+Tab` are indistinguishable and neither
    /// order nor direction is observable. The third button is what makes the
    /// scaffold's tab navigation actually testable by hand.
    #[test]
    fn the_demo_panel_offers_three_ordered_tab_stops() -> Result<(), TestError> {
        let mut app = App::new();
        app.insert_resource(UiDirection::Ltr)
            .insert_resource(UiDemoVisible(true))
            .init_resource::<UiDemoLabelLong>()
            .init_resource::<UiDemoTextSize>()
            .add_systems(Startup, (spawn_ui_root, setup_ui_demo).chain());
        app.update();

        let mut state = app
            .world_mut()
            .query_filtered::<&TabIndex, With<UiDemoButton>>();
        let mut indices: Vec<i32> = state.iter(app.world()).map(|index| index.0).collect();
        indices.sort_unstable();
        assert_eq!(
            indices,
            vec![1, 2, 3],
            "the demo's buttons must take 1, 2, 3 — three distinct stops, after the text \
             panel's editor at 0"
        );

        let root = app
            .world()
            .get_resource::<UiRoot>()
            .ok_or("`spawn_ui_root` did not publish the `UiRoot` resource")?
            .0;
        let mut panels = app
            .world_mut()
            .query_filtered::<&ChildOf, With<UiDemoRoot>>();
        assert_eq!(
            panels.single(app.world()).map(ChildOf::parent).ok(),
            Some(root),
            "the demo panel must hang off the `UiRoot`, not float loose in the window"
        );
        Ok(())
    }

    /// The plugin builds, and brings the one focus piece `DefaultPlugins` omits.
    /// Without `TabNavigationPlugin` the `Tab` key is inert, which is the sort of
    /// thing nothing else would catch until a human tried it.
    #[test]
    fn the_plugin_adds_tab_navigation() {
        let mut app = App::new();
        app.add_plugins(ViewerUiPlugin);
        assert!(
            app.is_plugin_added::<TabNavigationPlugin>(),
            "`DefaultPlugins` wires focus dispatch but not navigation; the scaffold must add it"
        );
    }
}
