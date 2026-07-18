//! The **floater window manager** (`viewer-ui-floater-basic` +
//! `viewer-ui-floater-resize-dock`): a draggable, titled, free-floating window on
//! top of `bevy_ui`, and the chrome every real viewer panel hangs off.
//!
//! Nothing upstream has a floater manager — every Second Life viewer hand-writes
//! one — so this is ours, built on [`crate::ui`]'s scaffold. It is modelled on
//! Firestorm's `LLFloater` (read-only reference: `indra/llui/llfloater.cpp`,
//! `lldraghandle`, `llresizehandle`, `llmultifloater`), reproduced faithfully
//! where the *feel* matters and adapted where our conventions are a strict
//! improvement (see below).
//!
//! # The two tiers
//!
//! - **Basic** (`viewer-ui-floater-basic`): a title bar you **drag** to move, a
//!   **z-order** where any press brings the floater to the front, a
//!   **focus**/active highlight on the front-most one, and a **close** button
//!   (plus `Ctrl+W`, the reference shortcut). Kept on screen: at least
//!   [`MIN_VISIBLE`] logical pixels of the title bar always stay reachable
//!   (`FLOATER_MIN_VISIBLE_PIXELS = 16`).
//! - **Resize / dock** (`viewer-ui-floater-resize-dock`): a **resize** grip at the
//!   trailing-bottom corner, **minimize** to a title-only strip, and
//!   **dock** / **tear-off** — reparenting a floater into a host container and
//!   back out to a free window.
//!
//! # Content-driven, not a pixel rect — and how manual resize composes with that
//!
//! Per the scaffold's convention 2 a floater **sizes to its content**: its width
//! and height are `Val::Auto`, so a longer translated title or a larger UI font
//! grows the window rather than clipping it (the reference pins a fixed
//! `header_height`; we let the title bar size to its own content instead, which is
//! the same idea done right). What a floater *does* own is a **position**, and
//! that is the one thing the scaffold left a hole for: [`crate::ui::LogicalInset`]
//! resting at `Val::Auto`, added by this task, so the remembered leading/top
//! offset mirrors under RTL for free.
//!
//! Manual resize (the second tier) layers over that. A plain panel with no
//! [`FloaterSpec::default_size`] is purely content-driven and grows to its text.
//! A **scroll-list** window (the inventory) is the case the content-sizing
//! convention explicitly carves out for a definite size — a list has no natural
//! width, and the reference gives it a default rect and `can_resize` — so it
//! opens at a `default_size` and the grip adjusts that **content-area** size
//! ([`Floater::content_size`]), floored at the window's own
//! [`min_size`](FloaterSpec::min_size), with the consumer's own content filling
//! it (and the slot **clips**, so nothing renders past the window edge). So the
//! window grows *and* shrinks with the grip, down to a real minimum, rather than
//! being pinned to one measured rect.
//!
//! # Constructible without its wiring
//!
//! Like every element (`crate::ui_element`), a floater's chrome is spawnable with
//! no plugin, no session and no world: [`build_floater_chrome`] lays out the title
//! bar, buttons, content slot and grip as ordinary nodes, and the gallery /
//! headless harness render that **specimen** ([`spawn_floater_specimen`], the
//! registered element) with none of the live behaviour attached. The live
//! [`spawn_floater`] adds the absolute placement, the [`Floater`] state and the
//! observers on top of the same chrome, so what the harness sweeps is the layout
//! the viewer actually ships.
//!
//! # Where we deliberately differ from the reference
//!
//! - **Docking hosts as a vertical stack, not a tabbed `LLMultiFloater`.** The
//!   reparent mechanism is faithful (dock disables drag/resize/minimize and uses
//!   the floater's title as a section header; tear-off restores and re-floats);
//!   the *tabbed* presentation is deferred to [[viewer-ui-tab-widget]], which the
//!   host will adopt.
//! - **Bring-to-front does not steal the keyboard.** A press marks the floater
//!   active (front-most, highlighted) but does **not** move `InputFocus` — that
//!   stays with whatever focusable child (a text field, a list) the click landed
//!   on, so dragging a title bar never quietly takes the keyboard from the world.
//! - **No sibling-snap or minimized-corner tiling yet.** The screen-edge clamp
//!   (keep 16 px visible) is in; the reference's 5 px snap-to-sibling and its
//!   top-left tiling of minimized strips are follow-ons, noted where they'd land.
//!
//! Reference (Firestorm, read-only): `indra/llui/llfloater.{h,cpp}`
//! (`bringToFront`, `setMinimized`, `updateTransparency`, `TitleBarFocusColor` =
//! `White_10`, `FLOATER_MIN_VISIBLE_PIXELS = 16`), `lldraghandle.cpp`,
//! `llresizehandle.cpp` (`Resize_Corner`, `RESIZE_HANDLE_WIDTH = 11`),
//! `llmultifloater.cpp` (docking / tear-off).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::ui::{
    LogicalBorder, LogicalInset, LogicalPadding, LogicalRect, UiDirection, UiPanelShown, UiRoot,
    UiScaffoldSystems, column, row,
};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;

/// The least a floater may move off screen: at least this many **logical** pixels
/// of it always stay reachable, so a window can never be dragged fully out of
/// sight. The reference's `FLOATER_MIN_VISIBLE_PIXELS` (`llfloater.h`).
const MIN_VISIBLE: f32 = 16.0;

/// How far a **docked** floater's title bar must be dragged before it tears off,
/// in logical pixels — the reference's undock `SLOP` (`lldraghandle.cpp`). Below
/// this a stray drag does not detach it.
const TEAROFF_SLOP: f32 = 12.0;

/// The smallest a floater's manual (dragged) size may be set to, in logical
/// pixels. A floor on the resize grip so a window cannot be dragged to nothing;
/// the *content* min-size floors it higher whenever the content needs more.
const RESIZE_FLOOR: Vec2 = Vec2::new(120.0, 48.0);

/// The chrome font size, in logical pixels — the title and the button glyphs in
/// the **live** floater. The specimen uses the harness's swept size instead.
const CHROME_FONT_SIZE: f32 = 14.0;

/// The floater's background — a dark neutral, close to the reference's `DkGray`
/// window fill, at the viewer's usual panel opacity.
const FLOATER_BACKGROUND: Color = Color::srgba(0.11, 0.12, 0.15, 0.95);

/// A hairline border around the whole window, so a floater reads as one object
/// over a busy world behind it.
const FLOATER_BORDER_COLOR: Color = Color::srgb(0.30, 0.34, 0.42);

/// The title band's fill when the floater is **front-most** — the reference's
/// `TitleBarFocusColor` = `White_10`, a faint white wash that composites over the
/// floater background. Inactive floaters show no wash ([`Color::NONE`]).
const TITLE_BAR_ACTIVE: Color = Color::srgba(1.0, 1.0, 1.0, 0.10);

/// The title text when the floater is front-most — bright, as the reference
/// brightens the drag-handle title of the focused floater.
const TITLE_TEXT_ACTIVE: Color = Color::srgb(0.92, 0.94, 0.98);

/// The title text when the floater is not front-most — dimmed, the reference's
/// `setForeground(false)` on a background floater.
const TITLE_TEXT_INACTIVE: Color = Color::srgb(0.55, 0.58, 0.64);

/// A chrome button's background — a barely-there tint, so the glyphs read as
/// buttons without boxing them loudly.
const BUTTON_BACKGROUND: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);

/// A chrome button's glyph colour — the reference's `FloaterButtonImageColor` =
/// `LtGray`.
const BUTTON_GLYPH: Color = Color::srgb(0.90, 0.90, 0.90);

/// The resize grip's colour, a touch brighter than the buttons so the corner
/// affordance is findable.
const RESIZE_GRIP_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// The close-button glyph.
const GLYPH_CLOSE: &str = "\u{2715}";

/// The minimize-button glyph (an expanded floater — click to collapse).
const GLYPH_MINIMIZE: &str = "\u{2014}";

/// The restore-button glyph (a minimized floater — click to expand).
const GLYPH_RESTORE: &str = "\u{25ad}";

/// The dock-button glyph while **free-floating** (click to dock into the host).
const GLYPH_DOCK: &str = "\u{25a4}";

/// The dock-button glyph while **docked** (click to tear off into a free window).
const GLYPH_TEAROFF: &str = "\u{25a5}";

/// The resize-grip glyph — a lower-right corner wedge, standing in for the
/// reference's `Resize_Corner` image.
const GLYPH_RESIZE: &str = "\u{25e2}";

/// The plugin that drives every live [`Floater`]: the chrome commands, the
/// layout that follows a floater's state, the active-floater highlight, and the
/// on-screen clamp.
///
/// It adds no rendering and touches no session, so it is safe in any app that has
/// the scaffold's [`UiRoot`] — the viewer wires it; the gallery renders only the
/// static specimen and does not need it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FloaterPlugin;

impl Plugin for FloaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FloaterCommand>()
            .init_resource::<FloaterZTop>()
            .init_resource::<ActiveFloater>()
            .init_resource::<DefaultDockHost>()
            .add_systems(
                Startup,
                spawn_default_dock_host.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    close_active_floater_shortcut,
                    apply_floater_commands,
                    // Clamp the (drag-updated) position *before* it is written to
                    // the inset, so an overshoot is corrected in the same frame
                    // rather than snapping back a frame later. It reads last frame's
                    // measured size, which is right: the size changes slowly and a
                    // frame-old value never lets the window escape by more than a
                    // drag step.
                    clamp_floaters_on_screen,
                    // The systems that reflect a floater's changed state into its
                    // nodes. After the command + clamp, so a dock / minimize / clamp
                    // this frame is reflected the same frame.
                    apply_floater_inset,
                    apply_floater_content,
                    apply_floater_glyphs,
                    highlight_active_floater,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A live floating window, on its root node. Holds everything a floater
/// remembers that is not derivable from the tree.
#[derive(Component, Debug, Clone)]
pub(crate) struct Floater {
    /// A stable id — lets a consumer (the inventory window) tell its own floater
    /// apart from any other, and keys its remembered geometry in the settings
    /// store ([`crate::floater_persist`]).
    pub(crate) id: &'static str,
    /// The remembered on-screen position while free-floating, in **logical**
    /// pixels: `x` is the inline-start offset, `y` the block-start (top) offset.
    /// Written into [`LogicalInset`], so it mirrors under RTL.
    position: Vec2,
    /// The **content-area** size, in logical pixels, or `None` for a purely
    /// content-driven floater. Seeded from [`FloaterSpec::default_size`] and
    /// adjusted by the resize grip (floored at [`RESIZE_FLOOR`]); applied as the
    /// width / height of the content slot, which the consumer's own content then
    /// fills. A scroll-list window (the inventory) wants a definite, resizable
    /// area like this — the scaffold's content-sizing convention carves scroll
    /// viewports out exactly so — while a plain panel leaves it `None` and sizes
    /// to its text.
    content_size: Option<Vec2>,
    /// Whether the window is collapsed to its title bar.
    minimized: bool,
    /// The host it is currently docked in, or `None` when free-floating.
    docked_in: Option<Entity>,
    /// The last host it was docked in, so tearing off then re-docking returns it
    /// there (the reference's `mLastHostHandle`).
    last_host: Option<Entity>,
    /// The smallest the content area may be resized to, in logical pixels — the
    /// floor the grip stops at. Per floater, because a scroll-list window's real
    /// minimum (the toolbar and search must still fit) is bigger than a bare
    /// [`RESIZE_FLOOR`]; below it the chrome would spill out of the window.
    min_size: Vec2,
    /// Which chrome this floater offers.
    caps: FloaterCaps,
}

/// A floater's **persistable geometry** — everything the settings store
/// remembers per user ([`crate::floater_persist`]): where it sits, how big its
/// content area is, and the two boolean states. The live [`Floater`] holds more
/// (its host entity, min-size, caps) that is either not user data or not stable
/// across sessions; this is the slice that round-trips to disk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloaterGeometry {
    /// The free-floating position, in logical pixels (see [`Floater::position`]).
    pub(crate) position: Vec2,
    /// The content-area size, or `None` for a content-driven floater (see
    /// [`Floater::content_size`]).
    pub(crate) content_size: Option<Vec2>,
    /// Whether it is collapsed to its title bar.
    pub(crate) minimized: bool,
    /// Whether it is docked into a host.
    pub(crate) docked: bool,
}

impl Floater {
    /// The current geometry, snapshotted for persistence.
    pub(crate) const fn geometry(&self) -> FloaterGeometry {
        FloaterGeometry {
            position: self.position,
            content_size: self.content_size,
            minimized: self.minimized,
            docked: self.docked_in.is_some(),
        }
    }

    /// Restore a saved **position / size / minimized** state at seed time.
    ///
    /// Docking is deliberately *not* applied here — reparenting into a host needs
    /// the manager's command path ([`FloaterOp::ToggleDock`]), so
    /// [`FloaterGeometry::docked`] is honoured by the seeding system separately —
    /// and `docked_in` is left untouched.
    pub(crate) const fn restore_geometry(&mut self, geometry: FloaterGeometry) {
        self.position = geometry.position;
        self.content_size = geometry.content_size;
        self.minimized = geometry.minimized;
    }
}

/// The chrome entities of a floater, held on its root so the systems find each
/// part without a marker query. `Option` where a capability may be absent.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FloaterParts {
    /// The title band (the drag handle).
    title_bar: Entity,
    /// The title text node — recoloured active / inactive.
    title_text: Entity,
    /// The content slot a consumer fills; hidden when minimized.
    content: Entity,
    /// The close button box, when closable.
    close_button: Option<Entity>,
    /// The resize grip, when resizable.
    resize_handle: Option<Entity>,
    /// The minimize button box, when minimizable.
    minimize_button: Option<Entity>,
    /// The minimize button's glyph text, swapped minimize ↔ restore.
    minimize_glyph: Option<Entity>,
    /// The dock button box, when dockable.
    dock_button: Option<Entity>,
    /// The dock button's glyph text, swapped dock ↔ tear-off.
    dock_glyph: Option<Entity>,
}

/// The z-order high-water mark: the next `GlobalZIndex` a bring-to-front assigns.
///
/// Monotonic, so raising a floater never has to renumber the others — it simply
/// takes a value above every one seen so far, which is the reference's
/// front-of-list ordering expressed as a paint order. `i32` is far more headroom
/// than any session of clicks could exhaust.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct FloaterZTop(i32);

impl Default for FloaterZTop {
    fn default() -> Self {
        // Start above the ordinary panels (`GlobalZIndex` 0) so any floater floats
        // over them from its first raise.
        Self(1)
    }
}

impl FloaterZTop {
    /// The next z value, advancing the mark. Saturates rather than wrapping, so a
    /// pathological run of raises degrades to "all on top together" instead of
    /// diving behind everything.
    const fn next(&mut self) -> i32 {
        let z = self.0;
        self.0 = self.0.saturating_add(1);
        z
    }
}

/// The front-most / active floater, or `None`. Drives the title-bar highlight;
/// the reference's front-child concept.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct ActiveFloater(Option<Entity>);

/// The container a dock button docks its floater into, when one is set. The
/// plugin spawns one on the trailing edge and publishes it here; without one the
/// dock button is inert.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct DefaultDockHost(pub(crate) Option<Entity>);

/// The dock host's background — a faint panel so a docked floater reads as hosted
/// (the reference hides a hosted floater's own background to avoid double
/// opacity; here the host supplies the surround instead).
const DOCK_HOST_BACKGROUND: Color = Color::srgba(0.06, 0.07, 0.10, 0.85);

/// Startup: spawn the trailing-edge **dock host** — a vertical stack docked
/// floaters flow into — and publish it in [`DefaultDockHost`].
///
/// A stack rather than the reference's tabbed `LLMultiFloater`; the tabbed
/// presentation is deferred to `viewer-ui-tab-widget`. Empty (and so invisible)
/// until a floater docks; its content-sized column then grows to hold it.
fn spawn_default_dock_host(
    mut commands: Commands,
    root: Res<UiRoot>,
    mut host: ResMut<DefaultDockHost>,
) {
    let entity = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..column(Val::Px(4.0))
            },
            LogicalInset(LogicalRect {
                inline_end: Val::Px(12.0),
                block_start: Val::Px(60.0),
                ..LogicalRect::AUTO
            }),
            LogicalPadding(LogicalRect::all(Val::Px(4.0))),
            BackgroundColor(DOCK_HOST_BACKGROUND),
            // Above the ordinary panels, so a docked floater is not hidden behind
            // them, but it does not fight the free floaters' own raised z-order.
            GlobalZIndex(0),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Name::new("floater-dock-host"),
            ChildOf(root.0),
        ))
        .id();
    host.0 = Some(entity);
}

/// A chrome action to apply — written by a button's press observer and carried out
/// by [`apply_floater_commands`].
///
/// The reparent-and-restack operations (close, dock, raise) need `Commands`,
/// several resources and cross-entity edits, which is a lot to give an observer;
/// routing them through one message keeps each observer a one-liner and the heavy
/// lifting in a single, testable system. Drag and resize, which only mutate the
/// floater's own [`Floater`], skip the message and edit it directly.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct FloaterCommand {
    /// The floater root to act on.
    pub(crate) floater: Entity,
    /// What to do to it.
    pub(crate) op: FloaterOp,
}

/// The chrome operations routed through [`FloaterCommand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloaterOp {
    /// Raise to the front and make active (any press).
    BringToFront,
    /// Close (hide) the floater.
    Close,
    /// Toggle minimize / restore.
    ToggleMinimize,
    /// Toggle dock / tear-off.
    ToggleDock,
}

// ---------------------------------------------------------------------------
// Pure helpers — the arithmetic the systems and tests share.
// ---------------------------------------------------------------------------

/// Move a floater's logical position by a drag delta, respecting the writing
/// direction.
///
/// The delta is in **screen** pixels (y-down), while `position.x` is the
/// *inline-start* offset — measured from the leading edge, which is the right one
/// under RTL. So a physical rightward drag *increases* the offset under LTR and
/// *decreases* it under RTL, which keeps the window under the pointer either way
/// and lets a locale flip mirror it for free. The block axis never flips.
fn drag_position(position: Vec2, delta: Vec2, direction: UiDirection) -> Vec2 {
    let dx = if direction.is_rtl() {
        -delta.x
    } else {
        delta.x
    };
    Vec2::new(position.x + dx, position.y + delta.y)
}

/// Grow (or shrink) a floater's manual size by a grip drag, floored at
/// [`RESIZE_FLOOR`], respecting the writing direction.
///
/// The grip is at the **trailing**-bottom corner, so dragging it toward the
/// trailing edge enlarges the width — rightward under LTR, leftward under RTL —
/// the same inline-axis sign flip as [`drag_position`]. Height grows downward in
/// both.
fn resize_size(size: Vec2, delta: Vec2, direction: UiDirection, floor: Vec2) -> Vec2 {
    let dx = if direction.is_rtl() {
        -delta.x
    } else {
        delta.x
    };
    Vec2::new((size.x + dx).max(floor.x), (size.y + delta.y).max(floor.y))
}

/// A node's size in **logical** pixels: its physical [`ComputedNode`] size scaled
/// down per component.
///
/// Per component rather than `size * inverse_scale_factor`, because the whole-
/// `Vec2` `*` is a `glam` operator and the workspace's `arithmetic_side_effects`
/// lint fires on those (but not on plain `f32` arithmetic) — the same reason
/// `crate::pie_menu` and `crate::ik` spell their vector maths out.
fn logical_size(computed: &ComputedNode) -> Vec2 {
    let size = computed.size();
    let inverse = computed.inverse_scale_factor();
    Vec2::new(size.x * inverse, size.y * inverse)
}

/// Clamp a floater's logical position so at least [`MIN_VISIBLE`] pixels of it
/// stay on screen.
///
/// Reasoned entirely in **logical inline/block** terms, which is what makes it
/// direction-independent: `position.x` is the offset of the leading edge from the
/// leading side of the viewport, and under RTL the whole frame mirrors uniformly,
/// so the same bounds hold. The inline offset may go as negative as
/// `MIN_VISIBLE - width` (the trailing sliver still shows) and as positive as
/// `viewport - MIN_VISIBLE` (the leading sliver still shows); the top is kept at
/// or below zero's worth of visibility down to `viewport - MIN_VISIBLE`.
fn clamp_position(position: Vec2, size: Vec2, viewport: Vec2) -> Vec2 {
    let inline = position.x.clamp(
        MIN_VISIBLE - size.x,
        (viewport.x - MIN_VISIBLE).max(MIN_VISIBLE - size.x),
    );
    let block = position.y.clamp(0.0, (viewport.y - MIN_VISIBLE).max(0.0));
    Vec2::new(inline, block)
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// What a floater is created with. Everything the manager cannot derive: an id, a
/// title, where it opens, and which chrome it offers.
#[derive(Debug, Clone)]
pub(crate) struct FloaterSpec {
    /// The stable id (see [`Floater::id`]).
    pub(crate) id: &'static str,
    /// The title text.
    pub(crate) title: String,
    /// The opening position, in logical pixels (inline-start, block-start).
    pub(crate) position: Vec2,
    /// The content slot's starting size, in logical pixels, or `None` to size the
    /// window to its content. A resizable scroll-list window (the inventory) sets
    /// this so the grip can grow *and* shrink it; a plain panel leaves it `None`.
    pub(crate) default_size: Option<Vec2>,
    /// The smallest the grip may shrink the content area to, in logical pixels, or
    /// `None` for the bare [`RESIZE_FLOOR`]. Set it to whatever keeps this window's
    /// own chrome from spilling out (for the inventory, enough for the toolbar and
    /// search plus a few list rows).
    pub(crate) min_size: Option<Vec2>,
    /// Which chrome to offer.
    pub(crate) caps: FloaterCaps,
}

/// The entities a caller needs after spawning a floater.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FloaterHandle {
    /// The floater root — carries [`Floater`] and [`UiPanelShown`]. Open / close
    /// it by setting `UiPanelShown`.
    pub(crate) root: Entity,
    /// The content slot to parent the window's own UI into.
    pub(crate) content: Entity,
}

/// **Spawn a live floater** under `root`, starting hidden.
///
/// Lays out the shared chrome ([`build_floater_chrome`]), then makes it live: an
/// absolute [`LogicalInset`] at the spec's position, the [`Floater`] state, a
/// `GlobalZIndex` for the z-order, `Pickable` so it takes clicks off the world
/// behind it, and the drag / press / button observers. Starts closed
/// (`UiPanelShown(false)`) — the opener (e.g. the inventory toggle) shows it.
pub(crate) fn spawn_floater(
    commands: &mut Commands,
    root: Entity,
    spec: FloaterSpec,
) -> FloaterHandle {
    let font = UiFont::Sans.at(CHROME_FONT_SIZE);
    let floater = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // Content-driven: the window is as wide and tall as its content
                // needs (convention 2). Manual resize adds a `min_*` floor later.
                ..column(Val::ZERO)
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(spec.position.x),
                block_start: Val::Px(spec.position.y),
                ..LogicalRect::AUTO
            }),
            LogicalBorder(LogicalRect::all(Val::Px(1.0))),
            BorderColor::all(FLOATER_BORDER_COLOR),
            BackgroundColor(FLOATER_BACKGROUND),
            GlobalZIndex(0),
            // Opaque to picking, so a click on the floater does not fall through to
            // the world, and hoverable so its own buttons receive events.
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            UiPanelShown(false),
            Floater {
                id: spec.id,
                position: spec.position,
                content_size: spec.default_size,
                minimized: false,
                docked_in: None,
                last_host: None,
                min_size: spec.min_size.unwrap_or(RESIZE_FLOOR),
                caps: spec.caps,
            },
            Name::new(format!("floater:{}", spec.id)),
            ChildOf(root),
        ))
        .id();

    // Any press anywhere on the floater raises it (the reference brings a floater
    // to front on any mouse-down). Child presses bubble up to this root observer;
    // the root entity is captured rather than read off the event, because a
    // bubbled `Pointer` keeps the *original* hit entity in `.entity`, not the
    // ancestor the observer is attached to.
    commands.entity(floater).observe(
        move |press: On<Pointer<Press>>, mut commands: MessageWriter<FloaterCommand>| {
            if press.button == PointerButton::Primary {
                commands.write(FloaterCommand {
                    floater,
                    op: FloaterOp::BringToFront,
                });
            }
        },
    );

    let parts = build_floater_chrome(commands, floater, &spec.title, font.clone(), spec.caps);
    commands.entity(floater).insert(parts);

    // Make the chrome live: the drag handle moves the floater (or tears it off a
    // host), the grip resizes it, and each button raises then acts. Each observer
    // captures the floater root by `move`, so it is correct however deep in the
    // chrome the pointer actually landed.
    commands.entity(parts.title_bar).observe(
        move |drag: On<Pointer<Drag>>,
              mut floaters: Query<&mut Floater>,
              direction: Res<UiDirection>,
              mut commands: MessageWriter<FloaterCommand>| {
            drag_title(floater, &drag, &mut floaters, *direction, &mut commands);
        },
    );
    if let Some(handle) = parts.resize_handle {
        let content = parts.content;
        commands
            .entity(handle)
            .observe(
                move |_drag: On<Pointer<DragStart>>,
                      mut floaters: Query<&mut Floater>,
                      computed: Query<&ComputedNode>| {
                    seed_content_size(floater, content, &mut floaters, &computed);
                },
            )
            .observe(
                move |drag: On<Pointer<Drag>>,
                      mut floaters: Query<&mut Floater>,
                      direction: Res<UiDirection>,
                      mut commands: MessageWriter<FloaterCommand>| {
                    drag_resize(floater, &drag, &mut floaters, *direction, &mut commands);
                },
            );
    }
    for (button, op) in [
        (parts.dock_button, FloaterOp::ToggleDock),
        (parts.minimize_button, FloaterOp::ToggleMinimize),
        (parts.close_button, FloaterOp::Close),
    ] {
        let Some(button) = button else {
            continue;
        };
        commands.entity(button).observe(
            move |press: On<Pointer<Press>>, mut commands: MessageWriter<FloaterCommand>| {
                if press.button != PointerButton::Primary {
                    return;
                }
                // Raise first (any mouse-down brings a floater to front), then act.
                commands.write(FloaterCommand {
                    floater,
                    op: FloaterOp::BringToFront,
                });
                commands.write(FloaterCommand { floater, op });
            },
        );
    }

    FloaterHandle {
        root: floater,
        content: parts.content,
    }
}

/// The title-bar drag body, shared by the observer closure: move the floater with
/// the pointer, or tear it off its host once dragged past the slop.
fn drag_title(
    floater: Entity,
    drag: &Pointer<Drag>,
    floaters: &mut Query<&mut Floater>,
    direction: UiDirection,
    commands: &mut MessageWriter<FloaterCommand>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(mut state) = floaters.get_mut(floater) else {
        return;
    };
    if state.docked_in.is_some() {
        if drag.distance.length() > TEAROFF_SLOP {
            commands.write(FloaterCommand {
                floater,
                op: FloaterOp::ToggleDock,
            });
        }
        return;
    }
    state.position = drag_position(state.position, drag.delta, direction);
}

/// Seed a floater's content size from the content slot's measured size on the
/// first grip press, so a content-driven window starts resizing from where it
/// already is rather than jumping. A window that was given a
/// [`FloaterSpec::default_size`] already has one and is left alone.
fn seed_content_size(
    floater: Entity,
    content: Entity,
    floaters: &mut Query<&mut Floater>,
    computed: &Query<&ComputedNode>,
) {
    let Ok(mut state) = floaters.get_mut(floater) else {
        return;
    };
    if state.content_size.is_none()
        && let Ok(node) = computed.get(content)
    {
        state.content_size = Some(logical_size(node));
    }
}

/// The grip drag body: grow / shrink the manual size, or tear the floater off its
/// host first (the reference undocks on resize).
fn drag_resize(
    floater: Entity,
    drag: &Pointer<Drag>,
    floaters: &mut Query<&mut Floater>,
    direction: UiDirection,
    commands: &mut MessageWriter<FloaterCommand>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok(mut state) = floaters.get_mut(floater) else {
        return;
    };
    if state.docked_in.is_some() {
        commands.write(FloaterCommand {
            floater,
            op: FloaterOp::ToggleDock,
        });
        return;
    }
    let current = state.content_size.unwrap_or(state.min_size);
    let floor = state.min_size;
    state.content_size = Some(resize_size(current, drag.delta, direction, floor));
}

/// Which chrome a floater offers — carried by [`FloaterSpec`] and stored on the
/// [`Floater`], and read by the layout systems (a hidden grip, a guarded close).
#[expect(
    clippy::struct_excessive_bools,
    reason = "these are four independent, orthogonal capability toggles — resize, minimize, close, \
              dock — that a floater genuinely has in any combination; the reference (LLFloater) \
              carries them as the same set of independent flags, and a bitfield or enum would only \
              obscure four plainly-named yes/no capabilities"
)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct FloaterCaps {
    /// Build / offer the resize grip.
    pub(crate) resizable: bool,
    /// Build / offer the minimize button.
    pub(crate) minimizable: bool,
    /// Build / offer the close button (and honour `Ctrl+W`).
    pub(crate) closable: bool,
    /// Build / offer the dock / tear-off button.
    pub(crate) dockable: bool,
}

/// **Build the shared chrome** — the title bar, its buttons, the content slot and
/// the resize grip — as ordinary nodes under `parent`.
///
/// The half that is identical for the live floater and the static specimen, so the
/// harness sweeps the layout the viewer ships. It attaches **no** observers, no
/// absolute placement and nothing that needs the plugin: those are the live
/// [`spawn_floater`]'s job. Every text node is a plain child of a padded box (no
/// padding on the `Text` itself — the measure bug), and the buttons size to their
/// glyph so the font-size sweep grows them rather than clipping.
fn build_floater_chrome(
    commands: &mut Commands,
    parent: Entity,
    title: &str,
    font: TextFont,
    caps: FloaterCaps,
) -> FloaterParts {
    let title_bar = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                // The title takes the leading edge and the button cluster the
                // trailing one, whatever the window's width — so the chrome sits
                // top-trailing (top-right under LTR) as the reference does, not
                // bunched after the title. The title bar is stretched to the
                // window's width by the root column's `align_items: stretch`, which
                // is what gives `SpaceBetween` a width to distribute across.
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(12.0),
                ..row(Val::Px(4.0))
            },
            // The reference insets the title ~14 px from the leading edge; the
            // trailing side leaves room for the buttons. Logical, so it mirrors.
            LogicalPadding(LogicalRect {
                inline_start: Val::Px(14.0),
                inline_end: Val::Px(6.0),
                block_start: Val::Px(4.0),
                block_end: Val::Px(4.0),
            }),
            BackgroundColor(Color::NONE),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("floater-title-bar"),
            ChildOf(parent),
        ))
        .id();
    let title_text = commands
        .spawn((
            Text::new(title.to_owned()),
            font.clone(),
            TextColor(TITLE_TEXT_ACTIVE),
            Name::new("floater-title"),
            ChildOf(title_bar),
        ))
        .id();

    // The button cluster, in text order leading → trailing: dock, minimize, close
    // — so close is the trailing-most (right-most under LTR), the reference's
    // order, with the far-right slot the close box.
    let cluster = commands
        .spawn((
            Node {
                ..row(Val::Px(4.0))
            },
            Name::new("floater-buttons"),
            ChildOf(title_bar),
        ))
        .id();
    let (dock_button, dock_glyph) = if caps.dockable {
        let (button, glyph) = chrome_button(commands, cluster, GLYPH_DOCK, font.clone(), "dock");
        (Some(button), Some(glyph))
    } else {
        (None, None)
    };
    let (minimize_button, minimize_glyph) = if caps.minimizable {
        let (button, glyph) =
            chrome_button(commands, cluster, GLYPH_MINIMIZE, font.clone(), "minimize");
        (Some(button), Some(glyph))
    } else {
        (None, None)
    };
    let close_button = if caps.closable {
        let (button, _glyph) = chrome_button(commands, cluster, GLYPH_CLOSE, font.clone(), "close");
        Some(button)
    } else {
        None
    };

    // The content slot the consumer fills. Its own padding and gap; hidden when the
    // floater is minimized. **Clips its overflow**: a window is a boundary, so a
    // child that ends up outside it (a resized-narrow toolbar, say) must be cut off
    // at the edge, never left rendering out in space beyond the window.
    let content = commands
        .spawn((
            Node {
                overflow: Overflow::clip(),
                ..column(Val::Px(6.0))
            },
            LogicalPadding(LogicalRect::all(Val::Px(8.0))),
            Name::new("floater-content"),
            ChildOf(parent),
        ))
        .id();

    // The resize grip, absolute at the trailing-bottom corner (mirrored via the
    // logical inset), drawn over the content. A small padded glyph, content-sized.
    let resize_handle = if caps.resizable {
        Some(
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        padding: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    LogicalInset(LogicalRect {
                        inline_end: Val::Px(0.0),
                        block_end: Val::Px(0.0),
                        ..LogicalRect::AUTO
                    }),
                    Pickable {
                        should_block_lower: true,
                        is_hoverable: true,
                    },
                    Name::new("floater-resize"),
                    ChildOf(parent),
                ))
                .with_child((
                    Text::new(GLYPH_RESIZE.to_owned()),
                    font,
                    TextColor(RESIZE_GRIP_COLOR),
                ))
                .id(),
        )
    } else {
        None
    };

    FloaterParts {
        title_bar,
        title_text,
        content,
        close_button,
        resize_handle,
        minimize_button,
        minimize_glyph,
        dock_button,
        dock_glyph,
    }
}

/// Spawn one chrome button (a small padded box with a centred glyph) under
/// `parent`, returning the box and its glyph text.
fn chrome_button(
    commands: &mut Commands,
    parent: Entity,
    glyph: &str,
    font: TextFont,
    name: &str,
) -> (Entity, Entity) {
    let button = commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new(format!("floater-button:{name}")),
            ChildOf(parent),
        ))
        .id();
    let glyph = commands
        .spawn((
            Text::new(glyph.to_owned()),
            font,
            TextColor(BUTTON_GLYPH),
            ChildOf(button),
        ))
        .id();
    (button, glyph)
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Carry out the chrome commands: raise / close / minimize / dock.
#[expect(
    clippy::too_many_arguments,
    reason = "the reparent-and-restack commands genuinely touch the floater state, the z-order, \
              the active floater, the dock host, the root and the panel-shown flag; splitting them \
              would only scatter one coherent operation across several systems"
)]
fn apply_floater_commands(
    mut messages: MessageReader<FloaterCommand>,
    mut commands: Commands,
    mut floaters: Query<&mut Floater>,
    mut z_indices: Query<&mut GlobalZIndex>,
    mut z_top: ResMut<FloaterZTop>,
    mut active: ResMut<ActiveFloater>,
    mut panels: Query<&mut UiPanelShown>,
    dock_host: Res<DefaultDockHost>,
    root: Res<UiRoot>,
) {
    for command in messages.read() {
        let Ok(mut floater) = floaters.get_mut(command.floater) else {
            continue;
        };
        match command.op {
            FloaterOp::BringToFront => {
                // A docked floater is in its host's flow and does not restack.
                if floater.docked_in.is_none() {
                    raise(command.floater, &mut z_indices, &mut z_top);
                }
                active.0 = Some(command.floater);
            }
            FloaterOp::Close => {
                if !floater.caps.closable {
                    continue;
                }
                if let Ok(mut shown) = panels.get_mut(command.floater) {
                    shown.0 = false;
                }
                if active.0 == Some(command.floater) {
                    active.0 = None;
                }
                // A consumer that tracks its own open state observes the
                // `UiPanelShown` change above (the inventory window does), so no
                // separate close event is needed.
            }
            FloaterOp::ToggleMinimize => {
                if floater.caps.minimizable {
                    floater.minimized = !floater.minimized;
                }
            }
            FloaterOp::ToggleDock => {
                if !floater.caps.dockable {
                    continue;
                }
                if floater.docked_in.is_some() {
                    tear_off(
                        command.floater,
                        &mut floater,
                        &mut commands,
                        root.0,
                        &mut z_indices,
                        &mut z_top,
                    );
                    active.0 = Some(command.floater);
                } else if let Some(host) = floater.last_host.or(dock_host.0) {
                    // Re-dock into the last host if there is one (the reference's
                    // `mLastHostHandle`), else the default host.
                    dock(
                        command.floater,
                        &mut floater,
                        &mut commands,
                        host,
                        &mut z_indices,
                    );
                    // A just-docked floater is the one the user acted on, so keep
                    // it active rather than leaving the highlight on nothing.
                    active.0 = Some(command.floater);
                }
            }
        }
    }
}

/// Assign `entity` the next z value, raising it above every other floater.
fn raise(entity: Entity, z_indices: &mut Query<&mut GlobalZIndex>, z_top: &mut FloaterZTop) {
    let z = z_top.next();
    if let Ok(mut index) = z_indices.get_mut(entity)
        && index.0 != z
    {
        index.0 = z;
    }
}

/// Dock a free floater into `host`: reparent it into the host's flow, forget its
/// float placement, un-minimize it, and drop it out of the z-order. The reference
/// also disables its drag / resize / minimize while hosted; here the layout
/// systems read `docked_in` for that ([`apply_floater_inset`],
/// [`apply_floater_content`]).
fn dock(
    entity: Entity,
    floater: &mut Floater,
    commands: &mut Commands,
    host: Entity,
    z_indices: &mut Query<&mut GlobalZIndex>,
) {
    floater.docked_in = Some(host);
    floater.last_host = Some(host);
    floater.minimized = false;
    commands.entity(entity).insert(ChildOf(host));
    if let Ok(mut index) = z_indices.get_mut(entity)
        && index.0 != 0
    {
        index.0 = 0;
    }
}

/// Tear a docked floater off its host: reparent it back under the UI root, restore
/// it as a free window and raise it to the front.
fn tear_off(
    entity: Entity,
    floater: &mut Floater,
    commands: &mut Commands,
    root: Entity,
    z_indices: &mut Query<&mut GlobalZIndex>,
    z_top: &mut FloaterZTop,
) {
    floater.docked_in = None;
    commands.entity(entity).insert(ChildOf(root));
    raise(entity, z_indices, z_top);
}

/// Reflect a changed floater into its root node: the placement (absolute + inset
/// while free, in-flow while docked).
fn apply_floater_inset(
    mut floaters: Query<(&Floater, &mut Node, &mut LogicalInset), Changed<Floater>>,
) {
    for (floater, mut node, mut inset) in &mut floaters {
        let docked = floater.docked_in.is_some();
        let position_type = if docked {
            PositionType::Relative
        } else {
            PositionType::Absolute
        };
        if node.position_type != position_type {
            node.position_type = position_type;
        }
        // The float position, or `Auto` on every edge while docked (so a relative
        // node is not nudged off its flow slot by a leftover offset).
        let wanted = if docked {
            LogicalInset(LogicalRect::AUTO)
        } else {
            LogicalInset(LogicalRect {
                inline_start: Val::Px(floater.position.x),
                block_start: Val::Px(floater.position.y),
                ..LogicalRect::AUTO
            })
        };
        if *inset != wanted {
            *inset = wanted;
        }
    }
}

/// Reflect a changed floater into its content slot: its size (which the consumer's
/// content fills), and its visibility — hidden when minimized, and the resize grip
/// hidden when minimized or docked.
fn apply_floater_content(
    floaters: Query<(&Floater, &FloaterParts), Changed<Floater>>,
    mut nodes: Query<&mut Node>,
) {
    for (floater, parts) in &floaters {
        set_display(&mut nodes, parts.content, !floater.minimized);
        // The content-area size, floored at the floater's minimum — `None` leaves
        // the slot content-driven. Applied even while docked (bounding the content
        // so an unbounded child cannot blow the docked width out to the screen).
        let (width, height) = match floater.content_size {
            Some(size) => (
                Val::Px(size.x.max(floater.min_size.x)),
                Val::Px(size.y.max(floater.min_size.y)),
            ),
            None => (Val::Auto, Val::Auto),
        };
        if let Ok(mut node) = nodes.get_mut(parts.content) {
            if node.width != width {
                node.width = width;
            }
            if node.height != height {
                node.height = height;
            }
        }
        // While minimized the content is out of the layout, so the title bar would
        // otherwise shrink to its own text and the buttons would jump. Hold the bar
        // at the window's width (the content width) so the strip — and its restore
        // / close buttons — stays put between minimized and restored.
        let bar_min_width = match (floater.minimized, floater.content_size) {
            (true, Some(size)) => Val::Px(size.x.max(floater.min_size.x)),
            _other => Val::Auto,
        };
        if let Ok(mut node) = nodes.get_mut(parts.title_bar)
            && node.min_width != bar_min_width
        {
            node.min_width = bar_min_width;
        }
        if let Some(handle) = parts.resize_handle {
            let show = floater.caps.resizable && !floater.minimized && floater.docked_in.is_none();
            set_display(&mut nodes, handle, show);
        }
    }
}

/// Set a node's `Display` between `Flex` (shown) and `None` (hidden), writing only
/// on a real change.
fn set_display(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// Reflect a changed floater into its toggling glyphs: minimize ↔ restore, and
/// dock ↔ tear-off.
fn apply_floater_glyphs(
    floaters: Query<(&Floater, &FloaterParts), Changed<Floater>>,
    mut texts: Query<&mut Text>,
) {
    for (floater, parts) in &floaters {
        if let Some(glyph) = parts.minimize_glyph {
            let wanted = if floater.minimized {
                GLYPH_RESTORE
            } else {
                GLYPH_MINIMIZE
            };
            set_glyph(&mut texts, glyph, wanted);
        }
        if let Some(glyph) = parts.dock_glyph {
            let wanted = if floater.docked_in.is_some() {
                GLYPH_TEAROFF
            } else {
                GLYPH_DOCK
            };
            set_glyph(&mut texts, glyph, wanted);
        }
    }
}

/// Set a glyph text node's string, writing only on a real change.
fn set_glyph(texts: &mut Query<&mut Text>, entity: Entity, glyph: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != glyph
    {
        glyph.clone_into(&mut text.0);
    }
}

/// Keep each floater's title bar highlighted when it is the active one, and its
/// title text bright / dimmed accordingly.
fn highlight_active_floater(
    active: Res<ActiveFloater>,
    floaters: Query<(Entity, &FloaterParts)>,
    mut backgrounds: Query<&mut BackgroundColor>,
    mut texts: Query<&mut TextColor>,
) {
    if !active.is_changed() {
        return;
    }
    for (entity, parts) in &floaters {
        let is_active = active.0 == Some(entity);
        let bar_color = if is_active {
            TITLE_BAR_ACTIVE
        } else {
            Color::NONE
        };
        if let Ok(mut background) = backgrounds.get_mut(parts.title_bar)
            && background.0 != bar_color
        {
            background.0 = bar_color;
        }
        let text_color = if is_active {
            TITLE_TEXT_ACTIVE
        } else {
            TITLE_TEXT_INACTIVE
        };
        let wanted = TextColor(text_color);
        if let Ok(mut color) = texts.get_mut(parts.title_text)
            && *color != wanted
        {
            *color = wanted;
        }
    }
}

/// `Ctrl+W` closes the front-most closable floater — the reference's
/// `File.CloseWindow`. Only when a floater is active and shown; the `Ctrl` keeps
/// it from firing while a bare `w` is typed.
fn close_active_floater_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    active: Res<ActiveFloater>,
    floaters: Query<(&Floater, &UiPanelShown)>,
    mut commands: MessageWriter<FloaterCommand>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyW)) {
        return;
    }
    let Some(entity) = active.0 else {
        return;
    };
    if let Ok((floater, shown)) = floaters.get(entity)
        && floater.caps.closable
        && shown.0
    {
        commands.write(FloaterCommand {
            floater: entity,
            op: FloaterOp::Close,
        });
    }
}

/// Keep every free-floating, shown floater at least [`MIN_VISIBLE`] pixels on
/// screen — the reference's "can't drag a window fully off screen".
///
/// Writes back through [`Floater::position`] (which [`apply_floater_inset`], next
/// in the chain, applies the *same* frame — so an overshoot never visibly snaps
/// back). It reads last frame's measured size, which is close enough: a window's
/// size changes slowly, and a frame-stale value can never let it escape by more
/// than one drag step. Only the free ones: a docked floater is in its host's flow.
fn clamp_floaters_on_screen(
    mut floaters: Query<(&mut Floater, &ComputedNode, &UiPanelShown)>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    for (mut floater, computed, shown) in &mut floaters {
        if !shown.0 || floater.docked_in.is_some() {
            continue;
        }
        let clamped = clamp_position(floater.position, logical_size(computed), viewport);
        // Exact inequality is right: `clamp_position` returns the input unchanged
        // when it is already on screen, and a bound otherwise — no epsilon needed,
        // and the whole-`Vec2` subtraction the lint forbids is avoided.
        if clamped != floater.position {
            floater.position = clamped;
        }
    }
}

// ---------------------------------------------------------------------------
// Registry specimen
// ---------------------------------------------------------------------------

/// Spawn a **static** floater specimen for the gallery / harness: the full chrome
/// (title bar, dock / minimize / close buttons, a content slot with a line of
/// prose, the resize grip) laid out in flow, with no live behaviour.
///
/// In flow (not absolute), so the harness's containment / viewport checks measure
/// it like any other card — the live floater's absolute placement contributes
/// nothing to a parent's content box and would sail past a check unmeasured. Its
/// text is the harness's swept sample, so a long translation or a large font grows
/// the window rather than clipping the title or a button glyph.
pub(crate) fn spawn_floater_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let floater = commands
        .spawn((
            Node {
                ..column(Val::ZERO)
            },
            LogicalBorder(LogicalRect::all(Val::Px(1.0))),
            BorderColor::all(FLOATER_BORDER_COLOR),
            BackgroundColor(FLOATER_BACKGROUND),
            Name::new("floater"),
            ChildOf(parent),
        ))
        .id();
    let parts = build_floater_chrome(
        commands,
        floater,
        &cx.text("Object properties"),
        cx.font(UiFont::Sans),
        FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: true,
        },
    );
    // A line of content in the slot, so the specimen shows the window around real
    // text rather than an empty frame.
    commands.spawn((
        Text::new(cx.text("A floating window, sized to its content.")),
        cx.font(UiFont::Sans),
        TextColor(Color::srgb(0.86, 0.89, 0.95)),
        Node {
            max_width: Val::Px(360.0),
            ..default()
        },
        Name::new("floater-content-text"),
        ChildOf(parts.content),
    ));
    floater
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveFloater, DefaultDockHost, FloaterCaps, FloaterCommand, FloaterOp, FloaterParts,
        FloaterSpec, FloaterZTop, MIN_VISIBLE, RESIZE_FLOOR, apply_floater_commands,
        apply_floater_content, apply_floater_glyphs, apply_floater_inset, clamp_position,
        drag_position, highlight_active_floater, resize_size, spawn_floater,
    };
    use crate::ui::{UiDirection, UiPanelShown, UiRoot};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A headless app with the floater command + reflection systems, a `UiRoot`
    /// to parent to, and a dock host — everything a floater's *behaviour* needs,
    /// minus the picking backend the observers would use (which the tests drive by
    /// writing [`FloaterCommand`]s directly instead).
    fn floater_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_message::<FloaterCommand>()
            .init_resource::<FloaterZTop>()
            .init_resource::<ActiveFloater>()
            .insert_resource(UiDirection::Ltr)
            .add_systems(
                Update,
                (
                    apply_floater_commands,
                    apply_floater_inset,
                    apply_floater_content,
                    apply_floater_glyphs,
                    highlight_active_floater,
                )
                    .chain(),
            );
        let root = app.world_mut().spawn(Node::default()).id();
        let host = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        app.insert_resource(DefaultDockHost(Some(host)));
        (app, root, host)
    }

    /// Spawn a live floater into `app` and settle a frame, returning its root.
    ///
    /// The root comes straight from [`spawn_floater`]'s handle — `Commands`
    /// reserves the entity eagerly, so the id is valid before the queue is applied
    /// — rather than by querying, which could not tell two floaters apart.
    fn spawn_one(app: &mut App, root: Entity) -> Entity {
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let floater = {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_floater(
                &mut commands,
                root,
                FloaterSpec {
                    id: "test",
                    title: "Test".to_owned(),
                    position: Vec2::new(30.0, 40.0),
                    default_size: None,
                    min_size: None,
                    caps: FloaterCaps {
                        resizable: true,
                        minimizable: true,
                        closable: true,
                        dockable: true,
                    },
                },
            )
            .root
        };
        queue.apply(app.world_mut());
        app.update();
        floater
    }

    /// Write a command and run a frame so the systems act on it.
    fn command(app: &mut App, floater: Entity, op: FloaterOp) {
        app.world_mut()
            .resource_mut::<Messages<FloaterCommand>>()
            .write(FloaterCommand { floater, op });
        app.update();
    }

    /// A drag moves the floater with the pointer under LTR, and mirrors the inline
    /// axis under RTL (the inline-start offset is measured from the right edge
    /// there), leaving the block axis alone in both.
    #[test]
    fn a_drag_moves_with_the_pointer_and_mirrors_under_rtl() {
        let start = Vec2::new(100.0, 50.0);
        let delta = Vec2::new(10.0, 6.0);
        let ltr = drag_position(start, delta, UiDirection::Ltr);
        assert_eq!(ltr, Vec2::new(110.0, 56.0), "LTR follows the pointer");
        let rtl = drag_position(start, delta, UiDirection::Rtl);
        assert_eq!(
            rtl,
            Vec2::new(90.0, 56.0),
            "RTL mirrors the inline axis (rightward drag reduces the offset from the right edge) \
             and leaves the block axis alone"
        );
    }

    /// The grip grows the window toward the trailing edge, mirrored under RTL, and
    /// never below the floor.
    #[test]
    fn the_grip_resizes_toward_the_trailing_edge_and_floors() {
        let size = Vec2::new(300.0, 200.0);
        assert_eq!(
            resize_size(size, Vec2::new(20.0, 10.0), UiDirection::Ltr, RESIZE_FLOOR),
            Vec2::new(320.0, 210.0),
            "a trailing-bottom drag grows both axes under LTR"
        );
        assert_eq!(
            resize_size(size, Vec2::new(-20.0, 10.0), UiDirection::Rtl, RESIZE_FLOOR),
            Vec2::new(320.0, 210.0),
            "under RTL the trailing edge is the left one, so a leftward drag grows the width"
        );
        // A generous floor stops the grip well before the content spills out.
        let floor = Vec2::new(260.0, 200.0);
        assert_eq!(
            resize_size(
                Vec2::new(280.0, 220.0),
                Vec2::new(-100.0, -100.0),
                UiDirection::Ltr,
                floor
            ),
            floor,
            "the content size never drops below the floor the window was given"
        );
    }

    /// The clamp keeps at least `MIN_VISIBLE` of the window on every side, and
    /// leaves a window already on screen untouched.
    #[expect(
        clippy::float_cmp,
        reason = "the clamp produces exact bound values, asserted exactly"
    )]
    #[test]
    fn the_clamp_keeps_a_sliver_on_screen() {
        let size = Vec2::new(300.0, 200.0);
        let viewport = Vec2::new(1000.0, 800.0);
        // Well inside: unchanged.
        assert_eq!(
            clamp_position(Vec2::new(100.0, 100.0), size, viewport),
            Vec2::new(100.0, 100.0),
        );
        // Dragged far off the trailing / bottom edge: pulled back to the last
        // visible sliver.
        let far = clamp_position(Vec2::new(2000.0, 2000.0), size, viewport);
        assert_eq!(far.x, viewport.x - MIN_VISIBLE);
        assert_eq!(far.y, viewport.y - MIN_VISIBLE);
        // Dragged far off the leading / top edge: the trailing sliver still shows,
        // and the top never goes above zero.
        let near = clamp_position(Vec2::new(-2000.0, -2000.0), size, viewport);
        assert_eq!(near.x, MIN_VISIBLE - size.x);
        assert_eq!(near.y, 0.0);
    }

    /// Bring-to-front hands out strictly increasing z values, so the last-raised
    /// floater is always on top and no raise renumbers the others.
    #[test]
    fn raising_hands_out_increasing_z() {
        let mut top = FloaterZTop::default();
        let first = top.next();
        let second = top.next();
        let third = top.next();
        assert!(
            first < second && second < third,
            "each raise must take a value above the last: {first}, {second}, {third}"
        );
    }

    /// Closing a floater hides it (via its `UiPanelShown`, the flag a consumer
    /// keeps its own open state in step with) and clears it as active.
    #[test]
    fn closing_hides_and_clears_active() -> Result<(), TestError> {
        let (mut app, root, _host) = floater_app();
        let floater = spawn_one(&mut app, root);
        command(&mut app, floater, FloaterOp::BringToFront);
        assert_eq!(
            app.world().resource::<ActiveFloater>().0,
            Some(floater),
            "a raise makes the floater active"
        );

        command(&mut app, floater, FloaterOp::Close);
        let shown = app
            .world()
            .get::<UiPanelShown>(floater)
            .ok_or("the floater lost its `UiPanelShown`")?;
        assert!(!shown.0, "closing must hide the floater");
        assert_eq!(
            app.world().resource::<ActiveFloater>().0,
            None,
            "closing the active floater clears it"
        );
        Ok(())
    }

    /// Minimize collapses the content out of the layout (leaving the title bar),
    /// and restore brings it back — with the button glyph swapping either way.
    #[test]
    fn minimize_hides_the_content_and_restore_returns_it() -> Result<(), TestError> {
        let (mut app, root, _host) = floater_app();
        let floater = spawn_one(&mut app, root);
        let parts = *app
            .world()
            .get::<FloaterParts>(floater)
            .ok_or("the floater has no parts")?;

        command(&mut app, floater, FloaterOp::ToggleMinimize);
        assert_eq!(
            app.world()
                .get::<Node>(parts.content)
                .ok_or("the content lost its `Node`")?
                .display,
            Display::None,
            "minimizing must take the content out of the layout"
        );

        command(&mut app, floater, FloaterOp::ToggleMinimize);
        assert_eq!(
            app.world()
                .get::<Node>(parts.content)
                .ok_or("the content lost its `Node`")?
                .display,
            Display::Flex,
            "restoring must bring the content back"
        );
        Ok(())
    }

    /// Docking reparents the floater into the host and puts it in flow; tearing
    /// off returns it under the UI root as an absolutely-placed free window and
    /// raises it.
    #[test]
    fn docking_reparents_into_the_host_and_tearing_off_restores() -> Result<(), TestError> {
        let (mut app, root, host) = floater_app();
        let floater = spawn_one(&mut app, root);

        command(&mut app, floater, FloaterOp::ToggleDock);
        assert_eq!(
            app.world().get::<ChildOf>(floater).map(ChildOf::parent),
            Some(host),
            "docking must reparent the floater into the host"
        );
        assert_eq!(
            app.world()
                .get::<Node>(floater)
                .ok_or("the floater lost its `Node`")?
                .position_type,
            PositionType::Relative,
            "a docked floater flows in the host, not by absolute inset"
        );

        command(&mut app, floater, FloaterOp::ToggleDock);
        assert_eq!(
            app.world().get::<ChildOf>(floater).map(ChildOf::parent),
            Some(root),
            "tearing off must reparent the floater back under the UI root"
        );
        assert_eq!(
            app.world()
                .get::<Node>(floater)
                .ok_or("the floater lost its `Node`")?
                .position_type,
            PositionType::Absolute,
            "a torn-off floater is placed by absolute inset again"
        );
        Ok(())
    }

    /// Bringing a floater to the front raises it above the others and marks it
    /// active — the front-most concept, driven through the command system.
    #[test]
    fn bringing_to_front_raises_above_the_others() -> Result<(), TestError> {
        let (mut app, root, _host) = floater_app();
        let first = spawn_one(&mut app, root);
        let second = spawn_one(&mut app, root);

        command(&mut app, first, FloaterOp::BringToFront);
        command(&mut app, second, FloaterOp::BringToFront);

        let first_z = app
            .world()
            .get::<GlobalZIndex>(first)
            .ok_or("the first floater lost its z")?
            .0;
        let second_z = app
            .world()
            .get::<GlobalZIndex>(second)
            .ok_or("the second floater lost its z")?
            .0;
        assert!(
            second_z > first_z,
            "the last floater raised must sit above the earlier one: {second_z} vs {first_z}"
        );
        assert_eq!(
            app.world().resource::<ActiveFloater>().0,
            Some(second),
            "the last floater raised is the active one"
        );
        Ok(())
    }
}
