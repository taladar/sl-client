//! The **reusable tab widget** (`viewer-ui-tab-widget`): a strip of tab buttons
//! that switches which one of a set of panels is shown, with exactly one active.
//!
//! # The two halves, and why they are separable
//!
//! A tab widget is really two things, and this module exposes them separately
//! because the viewer needs them separately:
//!
//! - a **tab strip** — a single-select strip of buttons ([`spawn_tab_strip`]),
//!   which owns "exactly one is active", the active highlight, and keyboard
//!   selection. This is all the inventory window needs
//!   ([`crate::inventory`]): its Everything / Recent / Worn tabs drive **one**
//!   shared list whose rows are rebuilt from the model, not three separate
//!   panels to reveal.
//! - the **panel switching** on top — a container that also holds one panel per
//!   tab and shows only the active one ([`spawn_tab_container`]). This is the
//!   whole widget, the shape a preferences floater
//!   ([`viewer-preferences-floater`](crate::floater)) wants.
//!
//! Keeping the strip usable on its own is what lets the inventory adopt the
//! widget without inventing three throwaway panels to satisfy it.
//!
//! # Both layouts, named logically
//!
//! The reference viewer puts tabs in three places — `LLTabContainer::TabPosition`
//! is `TOP`, `BOTTOM` and `LEFT` — so horizontal tabs run along the top or bottom
//! edge and vertical tabs run down one side. It has no `RIGHT`: vertical tabs are
//! always on the left, because the reference does not do bidi.
//!
//! We name the placement **logically** instead ([`TabPlacement`]), so the strip's
//! side is chosen independently of the reading direction:
//!
//! - [`TabPlacement::BlockStart`] / [`TabPlacement::BlockEnd`] — a horizontal
//!   strip on the top / bottom edge (the block axis never mirrors, so these are
//!   always top / bottom).
//! - [`TabPlacement::InlineStart`] / [`TabPlacement::InlineEnd`] — a vertical
//!   strip on the leading / trailing edge. Under [`UiDirection::Ltr`](crate::ui::UiDirection::Ltr) the leading
//!   edge is the left one and the trailing edge the right; under
//!   [`UiDirection::Rtl`](crate::ui::UiDirection::Rtl) they swap, with no code here saying so — the container
//!   is a [`crate::ui::row`] and [`crate::ui::apply_ui_direction`] reverses the
//!   flow, exactly as the scaffold's convention 1 promises.
//!
//! `InlineEnd` is therefore not only "the RTL mirror of a left strip". It is a
//! first-class placement a skin or a user setting can choose for an LTR layout
//! too — right-hand vertical tabs the reference cannot express — and it mirrors
//! under RTL like any other logical placement.
//!
//! # Content-sizing, not scroll arrows
//!
//! When tabs outgrow their space the reference grows scroll arrows
//! (`mJumpPrevArrowBtn` … and the `mScrollPos` machinery). We do not: convention
//! 2 says a strip of text sizes to its content and **reflows** rather than
//! clipping, so a horizontal strip wraps to a second line (`FlexWrap::Wrap`) when
//! a larger UI font or a longer translation outgrows the row. A longer label
//! grows its tab; nothing is measured once in English and pinned.
//!
//! # Selection and focus come from the scaffold and upstream
//!
//! The strip is a `bevy_ui_widgets` [`RadioGroup`] and each tab a [`RadioButton`]
//! — single-select, mutually exclusive, and (per the WAI-ARIA tablist pattern)
//! the **group** is the focus stop, not the individual tabs. So `Tab` lands on
//! the strip once and the arrow keys move the selection within it
//! (`radio_group_on_key_input`), which is the reference's `KEY_LEFT` / `KEY_RIGHT`
//! tab navigation in ARIA terms. The one thing upstream deliberately leaves to
//! the app is the [`Checked`] state ("presumed to happen by the app"); we own it,
//! keyed off the single source of truth [`TabStrip::active`], so the arrow
//! handler — which reads [`Checked`] to find the current tab — always agrees with
//! what is drawn.
//!
//! # A resizable divider for vertical tabs
//!
//! Content-sizing is the default, but a vertical strip whose tabs are **data**
//! rather than fixed words — group names, avatar names, which can run long —
//! wants the opposite: a strip narrow enough to leave the panel room, with the
//! long names truncated. So a container spawned with an explicit
//! [`TabSpec::strip_width`] pins the vertical strip to that width, clips each
//! over-long label (declaring [`crate::ui_element::TextMayClip`], the harness's
//! sanctioned exception), and puts a **draggable divider** between the strip and
//! the panel so the split can be moved. The width is a component
//! ([`TabStripWidth`]) so it is the one source of truth: the drag writes it, and
//! [`crate::floater_persist`] saves and restores it per host floater, so a window
//! reopens with the split where the user left it. The drag's sign folds in both
//! the placement and [`UiDirection`](crate::ui::UiDirection) — widening a
//! leading strip and a trailing one, under LTR and RTL, are four different screen
//! gestures resolved by [`resize_strip_width`] — so the handle behaves under a
//! mirrored layout with no per-side code.
//!
//! # Constructible without wiring
//!
//! Per the registry rule ([`crate::ui_element`]): selecting a tab is pure UI
//! state and never reaches a session, so the widget switches panels itself and,
//! for the harness, emits a [`UiAction`] naming that a switch happened. A consumer
//! that must *do* something on a tab change (the inventory rebuilding its list)
//! reacts to `Changed<TabStrip>` and reads [`TabStrip::active`] — it is not wired
//! into the widget. The gallery registers one element per placement
//! ([`spawn_tabs_block_start`] and friends) so every orientation is swept by
//! [`crate::ui_test`].
//!
//! Reference (Firestorm, read-only): `indra/llui/lltabcontainer.{h,cpp}`
//! (`LLTabContainer`).

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::{Checked, UiSystems};
use bevy::ui_widgets::{
    Button, ControlOrientation, RadioButton, RadioGroup, Scrollbar, ScrollbarThumb, ValueChange,
};

use crate::ui::{FocusRevealBounds, UiDirection, column, row};
use crate::ui_element::{ElementCx, TextMayClip, UiAction};
use crate::ui_font::UiFont;

/// The gap between adjacent tab buttons, in logical pixels.
const TAB_GAP: f32 = 4.0;

/// The gap between the tab strip and the panel it fronts, in logical pixels. Zero
/// so the strip abuts its panel, the way a real tabbed container reads.
const STRIP_PANEL_GAP: f32 = 0.0;

/// A panel's widest allowed width, in logical pixels — a bound, never a size, so
/// prose wraps inside it rather than overflowing (convention 2). Narrower than a
/// standalone panel's bound to leave room for a vertical strip beside it. Also
/// bounds a **horizontal** strip, so tabs too wide for it scroll rather than
/// growing the widget.
const PANEL_MAX_WIDTH: f32 = 320.0;

/// The tallest a **vertical** strip may grow, in logical pixels, before its tabs
/// scroll instead of growing the widget. A definite bound is what makes the
/// overflow real: a content-sized container otherwise just grows to fit every
/// tab, and nothing ever scrolls (the reference's tab container is likewise a
/// fixed size, from its floater). About seven tabs at the default size.
const TAB_STRIP_MAX_HEIGHT: f32 = 220.0;

/// An inactive tab's background — recessed and clearly darker than the active
/// one, so the selected tab reads at a glance even without focus.
const TAB_INACTIVE_BACKGROUND: Color = Color::srgb(0.11, 0.13, 0.17);

/// The active tab's background — the same shade as the panel it fronts
/// ([`PANEL_BACKGROUND`]), so the reference-viewer look of the selected tab
/// merging into its content reads.
const TAB_ACTIVE_BACKGROUND: Color = Color::srgb(0.19, 0.23, 0.31);

/// An inactive tab's border.
const TAB_BORDER: Color = Color::srgb(0.28, 0.33, 0.42);

/// The active tab's border — a bright accent, the loudest single "this one is
/// selected" signal, independent of keyboard focus.
const TAB_ACTIVE_BORDER: Color = Color::srgb(0.52, 0.68, 0.95);

/// The radius of a tab's rounded corners, in logical pixels. Applied only to the
/// two corners on the edge **away** from the content ([`tab_corner_radius`]), so
/// a tab reads as a tab rather than a plain button.
const TAB_CORNER_RADIUS: f32 = 8.0;

/// The default truncation glyph for a clipped tab label — a single Latin
/// ellipsis. See [`TabSpec::ellipsis`] for why this is configurable.
pub(crate) const DEFAULT_ELLIPSIS: &str = "…";

/// The leading gap between a truncated label and its ellipsis, in logical pixels,
/// so the marker is not glued to the last visible glyph.
const ELLIPSIS_GAP: f32 = 2.0;

/// A tab label's colour.
const TAB_LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// The panel area's background — the "content" shade the active tab shares.
const PANEL_BACKGROUND: Color = Color::srgb(0.19, 0.23, 0.31);

/// A demo panel's text colour, for the gallery elements.
const PANEL_TEXT_COLOR: Color = Color::WHITE;

/// A gallery panel heading's colour — brighter than the body, so a tab switch
/// (which swaps the heading) is unmistakable.
const PANEL_HEADING_COLOR: Color = Color::srgb(0.70, 0.82, 1.0);

/// The narrowest a resizable vertical strip may be dragged, in logical pixels —
/// enough to keep a tab clickable even when every label is truncated to nothing.
const MIN_STRIP_WIDTH: f32 = 40.0;

/// The widest a resizable vertical strip may be dragged, in logical pixels.
const MAX_STRIP_WIDTH: f32 = 400.0;

/// The draggable divider's thickness, in logical pixels — wide enough to be an
/// obvious grab target.
const DIVIDER_THICKNESS: f32 = 8.0;

/// The draggable divider's grip length, in logical pixels — a short raised nub
/// centred on the bar so it reads as a handle, not just a seam.
const DIVIDER_GRIP_LENGTH: f32 = 28.0;

/// The divider handle's colour.
const DIVIDER_COLOR: Color = Color::srgb(0.34, 0.41, 0.53);

/// The divider grip's colour — brighter than the bar, so the handle stands out.
const DIVIDER_GRIP_COLOR: Color = Color::srgb(0.60, 0.72, 0.92);

/// The scrollbar's thickness, in logical pixels — the width of a vertical strip's
/// bar (and the reserved gutter, so tabs do not jump when it appears).
const SCROLLBAR_THICKNESS: f32 = 10.0;

/// The scrollbar thumb's shortest length, in logical pixels, so it stays grabbable
/// when the content is far taller than the strip.
const SCROLLBAR_MIN_THUMB: f32 = 24.0;

/// The scrollbar track's colour.
const SCROLLBAR_TRACK_COLOR: Color = Color::srgb(0.12, 0.14, 0.18);

/// The scrollbar thumb's colour.
const SCROLLBAR_THUMB_COLOR: Color = Color::srgb(0.40, 0.48, 0.60);

/// How far one click of a horizontal strip's scroll arrow moves the tabs, in
/// logical pixels.
const ARROW_SCROLL_STEP: f32 = 64.0;

/// The scroll arrow that points toward the **inline start** (`◀` under LTR).
const ARROW_TOWARD_START: &str = "\u{25c0}";

/// The scroll arrow that points toward the **inline end** (`▶` under LTR).
const ARROW_TOWARD_END: &str = "\u{25b6}";

/// The action a strip emits when the user switches tabs. A single verb — "a
/// switch happened" — because the *which* is readable directly from
/// [`TabStrip::active`]; the [`UiAction`] exists so the harness can assert the
/// switch occurred without a session behind it.
pub(crate) const TAB_SELECTED_ACTION: &str = "select-tab";

/// Where a tab strip sits relative to the panel it fronts, named logically so the
/// side is chosen independently of the reading direction — see the [module
/// documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabPlacement {
    /// A horizontal strip along the top edge (the block-start edge, never
    /// mirrored).
    BlockStart,
    /// A horizontal strip along the bottom edge (the block-end edge, never
    /// mirrored).
    BlockEnd,
    /// A vertical strip on the leading edge — left under [`UiDirection::Ltr`](crate::ui::UiDirection::Ltr),
    /// right under [`UiDirection::Rtl`](crate::ui::UiDirection::Rtl).
    InlineStart,
    /// A vertical strip on the trailing edge — right under [`UiDirection::Ltr`](crate::ui::UiDirection::Ltr),
    /// left under [`UiDirection::Rtl`](crate::ui::UiDirection::Rtl).
    InlineEnd,
}

impl TabPlacement {
    /// Whether the strip runs vertically (its buttons stack down the block axis).
    const fn is_vertical(self) -> bool {
        matches!(self, Self::InlineStart | Self::InlineEnd)
    }

    /// Whether the strip comes **before** the panel in flow order — the leading /
    /// top placements. Under RTL an inline-axis "before" mirrors to the other
    /// side of the screen for free; the block-axis one does not mirror.
    const fn strip_first(self) -> bool {
        matches!(self, Self::BlockStart | Self::InlineStart)
    }

    /// The container node that holds the strip and the panel area: a
    /// [`crate::ui::column`] when the tabs are horizontal (strip stacked over
    /// panel) and a [`crate::ui::row`] when they are vertical (strip beside
    /// panel).
    ///
    /// `align_items: Stretch` always: it is what bounds the strip to the panel's
    /// size (rather than letting the strip grow to fit every tab), so a strip too
    /// full for the space overflows and its scroll control appears. It also gives
    /// a vertical layout's strip, divider and panel one shared height so the
    /// divider is full-height and grabbable.
    fn container_node(self) -> Node {
        let mut node = if self.is_vertical() {
            row(Val::Px(STRIP_PANEL_GAP))
        } else {
            column(Val::Px(STRIP_PANEL_GAP))
        };
        node.align_items = AlignItems::Stretch;
        node
    }

    /// The strip **wrapper** — the [`RadioGroup`] — a row of `[viewport,
    /// controls]` for both orientations. It carries a definite **max** on the
    /// scroll axis (width for horizontal, height for vertical), which is what
    /// makes a too-full strip overflow-and-scroll rather than grow the widget; a
    /// resizable vertical strip is additionally pinned to `width`. `min` 0 lets
    /// the viewport shrink below its content so it clips.
    fn wrapper_node(self, width: Option<f32>) -> Node {
        let mut node = Node {
            flex_direction: FlexDirection::Row,
            // Controls take the strip's full cross size (a full-height scrollbar,
            // full-height arrows).
            align_items: AlignItems::Stretch,
            ..default()
        };
        if self.is_vertical() {
            node.min_height = Val::Px(0.0);
            node.max_height = Val::Px(TAB_STRIP_MAX_HEIGHT);
            if let Some(width) = width {
                node.width = Val::Px(width);
            }
        } else {
            node.min_width = Val::Px(0.0);
            node.max_width = Val::Px(PANEL_MAX_WIDTH);
        }
        node
    }

    /// The scrolling **viewport** the buttons flow in: a [`crate::ui::column`] for
    /// a vertical strip, a [`crate::ui::row`] for a horizontal one, scrolling on
    /// that axis and shrinkable below its content so it clips rather than growing
    /// the strip. `flex_grow` fills the wrapper beside the controls.
    fn viewport_node(self) -> Node {
        if self.is_vertical() {
            Node {
                overflow: Overflow {
                    x: OverflowAxis::Clip,
                    y: OverflowAxis::Scroll,
                },
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..column(Val::Px(TAB_GAP))
            }
        } else {
            Node {
                overflow: Overflow {
                    x: OverflowAxis::Scroll,
                    y: OverflowAxis::Clip,
                },
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..row(Val::Px(TAB_GAP))
            }
        }
    }
}

/// Everything a tab strip or container is built from — a struct rather than a
/// long argument list, both for legibility and because the widget has more knobs
/// than a positional call should carry.
#[derive(Debug, Clone)]
pub(crate) struct TabSpec<'labels> {
    /// The element id the strip reports in its [`UiAction`], and the prefix of
    /// its nodes' [`Name`]s. Also the stable key [`crate::floater_persist`] saves
    /// a resizable strip's width under.
    pub(crate) element: &'static str,
    /// Where the strip sits relative to its panel.
    pub(crate) placement: TabPlacement,
    /// The tab labels, in order; their count is the number of tabs.
    pub(crate) labels: &'labels [String],
    /// The initially-active tab, clamped into range.
    pub(crate) active: usize,
    /// The strip's single focus stop (the group, not the buttons) — pick it to
    /// slot the strip into the surrounding tab order.
    pub(crate) tab_index: i32,
    /// The tab labels' font size, in logical pixels.
    pub(crate) font_size: f32,
    /// A fixed width for a **vertical** strip, in logical pixels, which turns on
    /// the draggable divider and label truncation. `None` (the default) keeps the
    /// strip content-sized with no divider; ignored for horizontal placements.
    pub(crate) strip_width: Option<f32>,
    /// The glyphs appended to a tab label that had to be truncated (only ever
    /// shown on a clipped, resizable strip). Configurable because the convention
    /// is not universal — Latin uses a single ellipsis `…`, while Chinese and
    /// Japanese use a centred six-dot `……`; a locale layer
    /// (`viewer-i18n-fluent-scaffold`) is where this would eventually come from.
    /// Use [`DEFAULT_ELLIPSIS`] where the caller has no locale of its own.
    pub(crate) ellipsis: &'static str,
    /// Whether [`labels`](Self::labels) are Fluent **keys** to translate
    /// (`crate::i18n::Translated`, re-resolved on locale change / bundle load)
    /// rather than literal display text. A translated strip's labels start empty
    /// and fill once the bundle loads. Use it for real UI; `false` for the
    /// gallery and tests, whose labels are fixed sample text.
    pub(crate) translate_labels: bool,
}

impl TabSpec<'_> {
    /// Whether this spec asks for a resizable divider: a fixed width on a
    /// vertical strip.
    const fn is_resizable(&self) -> bool {
        self.placement.is_vertical() && self.strip_width.is_some()
    }

    /// The text a label node starts with: empty for a translated strip (the key
    /// is not display text, and `crate::i18n::Translated` fills the real text once
    /// the bundle loads), otherwise the literal label.
    fn initial_label(&self, label: &str) -> String {
        if self.translate_labels {
            String::new()
        } else {
            label.to_owned()
        }
    }
}

/// Bind a tab-label node to its Fluent key when the strip is translated, so
/// `crate::i18n::apply_translations` keeps it resolved; a no-op for a literal
/// strip.
fn translate_tab_label(commands: &mut Commands, label_entity: Entity, spec: &TabSpec, label: &str) {
    if spec.translate_labels {
        commands
            .entity(label_entity)
            .insert(crate::i18n::Translated::new(label.to_owned()));
    }
}

/// A tab strip's state: which tab is active. The **single source of truth** — the
/// [`Checked`] flags, the highlight and the panel visibilities are all derived
/// from it, so nothing can drift.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabStrip {
    /// The element id this strip reports in its [`UiAction`], and the prefix of
    /// its nodes' [`Name`]s.
    pub(crate) element: &'static str,
    /// The index of the active tab, into the strip's buttons in spawn order.
    pub(crate) active: usize,
}

/// A resizable vertical strip's width, in logical pixels — the single source of
/// truth for its inline size. The divider drag writes it, [`apply_tab_strip_width`]
/// reflects it onto the node, and [`crate::floater_persist`] saves and restores
/// it. Present only on a strip spawned with [`TabSpec::strip_width`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct TabStripWidth(pub(crate) f32);

/// The draggable divider between a resizable strip and its panel, naming the
/// strip it resizes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabDivider {
    /// The strip whose [`TabStripWidth`] this handle drags.
    pub(crate) strip: Entity,
}

/// A truncatable tab label, naming the ellipsis marker
/// [`apply_tab_ellipsis`] reveals when the label is clipped. Present only on a
/// clipped (resizable) strip's labels.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabLabelClip {
    /// The `…` marker node, shown only while this label overflows its box.
    pub(crate) ellipsis: Entity,
}

/// Marks a tab's truncation-ellipsis marker node, so the i18n scaffold
/// (`crate::i18n::apply_locale_ellipsis`) can rewrite every marker's glyph to
/// the active locale's `ui-ellipsis` — a single Latin `…` for most locales, the
/// centred `……` for CJK. The glyph is [`TabSpec::ellipsis`] (defaulting to
/// [`DEFAULT_ELLIPSIS`]) until the locale bundle resolves it; that is a static
/// fallback, and the locale's convention is the source of truth once loaded.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct TabEllipsisMarker;

/// A tab button: which strip it belongs to and its index within it. Carried so
/// the selection observer can find every button of a strip and place it against
/// the strip's [`active`](TabStrip::active) index.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabButton {
    /// The strip ([`RadioGroup`]) this button is a tab of.
    pub(crate) strip: Entity,
    /// This tab's index within the strip.
    pub(crate) index: usize,
    /// The strip's placement, so [`apply_tab_corner_radius`] can round the two
    /// corners on the edge away from the content.
    pub(crate) placement: TabPlacement,
}

/// A tab panel: which strip switches it and which tab reveals it.
///
/// Hidden panels are toggled with [`Visibility`], **not** the scaffold's
/// `UiPanelShown` / `Display::None`: they must stay laid out so the panel area
/// sizes to the largest of them and the widget does not shrink when a lighter tab
/// is selected. (A consequence is that a *focusable* widget in a hidden panel
/// stays reachable by `Tab` — the scaffold's `UiPanelShown` parks that, and a
/// consumer that puts focusables in tab panels will want the same here; no
/// consumer does yet.)
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabPanel {
    /// The strip that switches this panel.
    pub(crate) strip: Entity,
    /// The tab index that reveals it.
    pub(crate) index: usize,
}

/// The scrolling viewport a strip's buttons live in — bounded to the available
/// space (the panel size) and scrolling when the tabs outgrow it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabViewport {
    /// Whether it scrolls on the block axis (a vertical strip) or the inline axis
    /// (a horizontal strip). Drives which measurement decides overflow and which
    /// controls appear.
    pub(crate) vertical: bool,
}

/// A strip's scroll control — the vertical scrollbar or the horizontal arrow
/// group — shown by [`apply_tab_scroll_controls`] only while its viewport
/// overflows, so it appears from available space, never configuration.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabScrollControl {
    /// The [`TabViewport`] this control scrolls / reflects.
    pub(crate) viewport: Entity,
    /// The measurement axis: block (vertical strip) or inline (horizontal).
    pub(crate) vertical: bool,
}

/// One of a horizontal strip's two scroll-arrow buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabScrollArrow {
    /// The viewport this arrow scrolls.
    pub(crate) viewport: Entity,
    /// Whether it scrolls toward the inline **end** (`▶` under LTR) or the inline
    /// **start** (`◀`). The physical direction and glyph both fold in the live
    /// [`UiDirection`](crate::ui::UiDirection).
    pub(crate) toward_end: bool,
}

/// A scroll arrow's glyph text, so [`apply_tab_arrow_glyphs`] can point it the
/// right physical way for the live direction.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabArrowGlyph {
    /// Matches its [`TabScrollArrow::toward_end`].
    pub(crate) toward_end: bool,
}

/// What [`spawn_tab_container`] hands back: the outer container and the panel
/// slots for the caller to fill.
///
/// Deliberately just these two — the widget owns its own strip, buttons and
/// divider (a consumer finds the strip by its [`TabStrip`] component to react to
/// `Changed<TabStrip>`), so returning them would be surface nobody reads. A
/// consumer that comes to need one adds the field with its reader.
#[derive(Debug, Clone)]
pub(crate) struct TabContainerHandle {
    /// The outer container node.
    pub(crate) container: Entity,
    /// The panel slots, in tab order — spawn each tab's content into these.
    pub(crate) panels: Vec<Entity>,
}

/// The plugin the viewer (and the gallery) adds for the tab widget's runtime
/// half: a resizable strip's [`TabStripWidth`] reaching the layout, and each
/// tab's rounded corners tracking the live [`UiDirection`].
///
/// Both systems are no-ops where they have nothing to act on, so adding the
/// plugin is always safe; a strip spawned with a fixed width already carries it
/// on the node from the start, so only *later* width changes need the first
/// system.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TabWidgetPlugin;

impl Plugin for TabWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, apply_tab_strip_width.before(UiSystems::Layout))
            .add_systems(
                Update,
                (
                    apply_tab_corner_radius,
                    apply_tab_arrow_glyphs,
                    scroll_tabs_with_wheel,
                ),
            )
            // After layout, because they read each node's *measured* size — a
            // label's, a viewport's — to decide truncation / overflow and write
            // next frame, the same shape as the pie's post-layout fit.
            .add_systems(
                PostUpdate,
                (apply_tab_ellipsis, apply_tab_scroll_controls).after(UiSystems::Layout),
            );
    }
}

/// The [`BorderRadius`] a tab carries: its two corners on the edge **away** from
/// the content are rounded, the two adjoining the content square, so it reads as
/// a tab. For an inline (vertical) strip the rounded side is the inline edge
/// opposite the content, resolved against `direction` so it mirrors under RTL;
/// for a block (horizontal) strip it is the top or bottom, which never mirror.
const fn tab_corner_radius(placement: TabPlacement, direction: UiDirection) -> BorderRadius {
    let radius = Val::Px(TAB_CORNER_RADIUS);
    match placement {
        TabPlacement::BlockStart => BorderRadius {
            top_left: radius,
            top_right: radius,
            ..BorderRadius::ZERO
        },
        TabPlacement::BlockEnd => BorderRadius {
            bottom_left: radius,
            bottom_right: radius,
            ..BorderRadius::ZERO
        },
        TabPlacement::InlineStart | TabPlacement::InlineEnd => {
            // The rounded side is the leading edge for an `InlineStart` strip
            // (content trails it) and the trailing edge for `InlineEnd`; RTL flips
            // which physical side that is. The `!=` is the two-way XOR of those.
            let round_left = matches!(placement, TabPlacement::InlineStart) != direction.is_rtl();
            if round_left {
                BorderRadius {
                    top_left: radius,
                    bottom_left: radius,
                    ..BorderRadius::ZERO
                }
            } else {
                BorderRadius {
                    top_right: radius,
                    bottom_right: radius,
                    ..BorderRadius::ZERO
                }
            }
        }
    }
}

/// Keep each tab's rounded corners on the edge away from its content, tracking
/// the live [`UiDirection`] so an inline strip's corners mirror under RTL.
///
/// Guarded per node so an unchanged tab does not re-trigger layout; swept every
/// frame (like the scaffold's `apply_ui_direction`) because a direction flip and
/// a freshly-spawned tab both need it and the two would otherwise be separate
/// `&mut Node` queries.
fn apply_tab_corner_radius(
    direction: Res<UiDirection>,
    mut buttons: Query<(&TabButton, &mut Node)>,
) {
    for (button, mut node) in &mut buttons {
        let wanted = tab_corner_radius(button.placement, *direction);
        if node.border_radius != wanted {
            node.border_radius = wanted;
        }
    }
}

/// Spawn a bare tab strip under `parent`: a single-select strip of buttons with
/// the active highlight, keyboard selection, and a [`UiAction`] on change.
///
/// [`TabSpec::active`] is clamped into range, so a caller cannot spawn a strip
/// with nothing selected. The returned strip carries [`TabStrip`], whose `active`
/// is the source of truth: a consumer that only needs the selection reacts to
/// `Changed<TabStrip>` and reads it.
pub(crate) fn spawn_tab_strip(commands: &mut Commands, parent: Entity, spec: &TabSpec) -> Entity {
    // Clamp rather than trust: an out-of-range active would leave no tab checked,
    // which the arrow handler reads as "start from the end" and the highlight as
    // "none lit". `saturating_sub` keeps an empty strip at 0 without underflow.
    let active = spec.active.min(spec.labels.len().saturating_sub(1));
    let resizable = spec.is_resizable();
    let vertical = spec.placement.is_vertical();

    // The strip is a `RadioGroup` **wrapper** holding a scrolling viewport plus
    // its scroll control, so the buttons can scroll while the control (and the
    // arrow keys, which walk the group's descendants) stay put.
    let strip = commands
        .spawn((
            RadioGroup,
            TabStrip {
                element: spec.element,
                active,
            },
            spec.placement
                .wrapper_node(spec.strip_width.filter(|_| resizable)),
            TabIndex(spec.tab_index),
            Name::new(format!("{}:tab-strip", spec.element)),
            ChildOf(parent),
        ))
        .observe(on_tab_value_change)
        .id();
    if let Some(width) = spec.strip_width.filter(|_| resizable) {
        commands.entity(strip).insert(TabStripWidth(width));
    }

    let viewport = commands
        .spawn((
            spec.placement.viewport_node(),
            ScrollPosition::default(),
            TabViewport { vertical },
            Name::new(format!("{}:tab-viewport", spec.element)),
            ChildOf(strip),
        ))
        .id();

    for (index, label) in spec.labels.iter().enumerate() {
        let is_active = index == active;
        let button = spawn_tab_button(commands, strip, viewport, spec, index, label, is_active);
        if is_active {
            commands.entity(button).insert(Checked);
        }
    }

    // The scroll control sits after the viewport (its trailing inline edge): a
    // scrollbar for a vertical strip, a pair of arrows for a horizontal one. Both
    // are hidden until [`apply_tab_scroll_controls`] finds the viewport
    // overflowing, so they appear from available space, not configuration.
    if vertical {
        spawn_tab_scrollbar(commands, strip, viewport, spec);
    } else {
        spawn_tab_scroll_arrows(commands, strip, viewport, spec);
    }

    strip
}

/// Spawn a vertical strip's scrollbar (a `bevy_ui_widgets` [`Scrollbar`] driving
/// the viewport) at the strip's trailing edge, hidden until it is needed.
fn spawn_tab_scrollbar(commands: &mut Commands, strip: Entity, viewport: Entity, spec: &TabSpec) {
    commands
        .spawn((
            Scrollbar {
                target: viewport,
                orientation: ControlOrientation::Vertical,
                min_thumb_length: SCROLLBAR_MIN_THUMB,
            },
            Node {
                width: Val::Px(SCROLLBAR_THICKNESS),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(SCROLLBAR_TRACK_COLOR),
            // Reserved space (hidden, not removed) so the tabs never jump when the
            // bar appears, which also keeps the overflow measurement stable.
            Visibility::Hidden,
            TabScrollControl {
                viewport,
                vertical: true,
            },
            Name::new(format!("{}:tab-scrollbar", spec.element)),
            ChildOf(strip),
        ))
        .with_child((
            ScrollbarThumb::default(),
            BackgroundColor(SCROLLBAR_THUMB_COLOR),
        ));
}

/// Spawn a horizontal strip's two scroll arrows at its trailing edge, hidden
/// until they are needed.
fn spawn_tab_scroll_arrows(
    commands: &mut Commands,
    strip: Entity,
    viewport: Entity,
    spec: &TabSpec,
) {
    let arrows = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            TabScrollControl {
                viewport,
                vertical: false,
            },
            Name::new(format!("{}:tab-arrows", spec.element)),
            ChildOf(strip),
        ))
        .id();
    for toward_end in [false, true] {
        spawn_tab_scroll_arrow(commands, arrows, viewport, spec, toward_end);
    }
}

/// Spawn one horizontal scroll arrow — a triangle button that nudges the viewport
/// toward the inline start (`toward_end == false`) or end.
fn spawn_tab_scroll_arrow(
    commands: &mut Commands,
    parent: Entity,
    viewport: Entity,
    spec: &TabSpec,
    toward_end: bool,
) {
    commands
        .spawn((
            Button,
            TabScrollArrow {
                viewport,
                toward_end,
            },
            Node {
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::default(),
            Name::new(format!(
                "{}:tab-arrow:{}",
                spec.element,
                usize::from(toward_end)
            )),
            ChildOf(parent),
        ))
        .observe(on_tab_scroll_arrow)
        .with_child((
            // The glyph is set for the live direction by `apply_tab_arrow_glyphs`;
            // start with the LTR default.
            Text::new(if toward_end {
                ARROW_TOWARD_END
            } else {
                ARROW_TOWARD_START
            }),
            UiFont::Sans.at(spec.font_size),
            TextColor(TAB_LABEL_COLOR),
            TabArrowGlyph { toward_end },
        ));
}

/// A scroll arrow's observer: nudge its viewport toward the inline start or end,
/// folding the physical direction in from the live [`UiDirection`].
fn on_tab_scroll_arrow(
    press: On<Pointer<Press>>,
    arrows: Query<&TabScrollArrow>,
    mut positions: Query<&mut ScrollPosition>,
    direction: Res<UiDirection>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(arrow) = arrows.get(press.entity) else {
        return;
    };
    let Ok(mut position) = positions.get_mut(arrow.viewport) else {
        return;
    };
    // `bevy_ui` clamps the far end at layout time, so flooring at zero is all that
    // is needed here.
    position.0.x = (position.0.x + arrow_scroll_delta(arrow.toward_end, *direction)).max(0.0);
}

/// The horizontal scroll delta one click of an arrow applies, folding the
/// physical direction in from the live [`UiDirection`]: inline end is `+x` under
/// LTR and `-x` under RTL.
fn arrow_scroll_delta(toward_end: bool, direction: UiDirection) -> f32 {
    let toward_end_sign = if direction.is_rtl() { -1.0 } else { 1.0 };
    let sign = if toward_end {
        toward_end_sign
    } else {
        -toward_end_sign
    };
    sign * ARROW_SCROLL_STEP
}

/// Spawn the whole tab widget under `parent`: a [`spawn_tab_strip`] strip plus a
/// panel area holding one empty panel slot per tab, only the active one shown —
/// and, for a resizable vertical layout, a draggable divider between the two.
///
/// The panels come back empty in [`TabContainerHandle::panels`]; the caller
/// spawns each tab's content into them. Which panel is visible tracks the strip's
/// selection with no wiring on the caller's part.
pub(crate) fn spawn_tab_container(
    commands: &mut Commands,
    parent: Entity,
    spec: &TabSpec,
) -> TabContainerHandle {
    let resizable = spec.is_resizable();
    let container = commands
        .spawn((
            spec.placement.container_node(),
            Name::new(format!("{}:tab-container", spec.element)),
            ChildOf(parent),
        ))
        .id();

    let strip = spawn_tab_strip(commands, container, spec);
    // The strip is the widget's single focus stop, but tabbing to it should bring
    // the whole widget (strip + panel) into view, not just the header row — so
    // point the scaffold's scroll-into-view at the container
    // (`viewer-ui-focus-scroll-into-view`).
    commands.entity(strip).insert(FocusRevealBounds(container));

    let divider = resizable.then(|| spawn_divider(commands, container, spec, strip));

    let panel_area = commands
        .spawn((
            Node {
                // A one-cell grid every panel is placed into, stacked. The cell —
                // and so the whole widget — sizes to the **largest** panel, so
                // switching to a lighter tab does not shrink the window; only the
                // active panel's `Visibility` changes, never the layout.
                display: Display::Grid,
                grid_template_columns: vec![GridTrack::auto()],
                grid_template_rows: vec![GridTrack::auto()],
                ..default()
            },
            // The "content" backdrop the active tab shares its shade with, so the
            // selected tab reads as merging into its panel.
            BackgroundColor(PANEL_BACKGROUND),
            Name::new(format!("{}:tab-panels", spec.element)),
            ChildOf(container),
        ))
        .id();

    // Flow order is insertion order; set it explicitly so `strip_first` decides
    // which side the strip lands on, with the divider always between the two. For
    // the inline placements RTL then mirrors that order across the screen for
    // free (convention 1); the block placements do not mirror.
    let ordered: Vec<Entity> = match (spec.placement.strip_first(), divider) {
        (true, Some(divider)) => vec![strip, divider, panel_area],
        (true, None) => vec![strip, panel_area],
        (false, Some(divider)) => vec![panel_area, divider, strip],
        (false, None) => vec![panel_area, strip],
    };
    commands.entity(container).add_children(&ordered);

    let mut panels = Vec::with_capacity(spec.labels.len());
    for index in 0..spec.labels.len() {
        let shown = index == handle_active(spec);
        let panel = commands
            .spawn((
                Node {
                    // Every panel is placed into the one grid cell, so all of them
                    // stay laid out (the area holds the max size) and only their
                    // `Visibility` differs — no `Display::None`, which would drop a
                    // panel from the layout and shrink the widget on a switch.
                    grid_column: GridPlacement::start(1),
                    grid_row: GridPlacement::start(1),
                    padding: UiRect::all(Val::Px(12.0)),
                    // A bound, not a size: panel content wraps here.
                    max_width: Val::Px(PANEL_MAX_WIDTH),
                    ..column(Val::Px(8.0))
                },
                // Hidden panels are laid out (so they count toward the max size)
                // but not drawn. `Visibility`, not `Display`, is the whole of the
                // difference from the scaffold's `UiPanelShown`.
                if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                TabPanel { strip, index },
                Name::new(format!("{}:panel:{index}", spec.element)),
                ChildOf(panel_area),
            ))
            .id();
        panels.push(panel);
    }

    // The strip, buttons and divider are intentionally not returned — see
    // [`TabContainerHandle`]; each is reachable by its component and drives
    // itself.
    TabContainerHandle { container, panels }
}

/// The clamped active index a spec resolves to — shared by the strip (for
/// `Checked`) and the container (for which panel starts shown).
fn handle_active(spec: &TabSpec) -> usize {
    spec.active.min(spec.labels.len().saturating_sub(1))
}

/// Spawn one tab button — a [`RadioButton`] styled as a tab. Not focusable
/// itself: per the ARIA tablist pattern the strip is the focus stop and the
/// arrows move the selection within it.
///
/// A tab label never wraps ([`LineBreak::NoWrap`]). On a **content-sized** strip
/// the button grows to fit its label, centred. On a **resizable** strip the
/// button is pinned to the strip width, so the label is a flex child that clips
/// (leading-aligned, so the *start* of a long name shows) with a trailing
/// ellipsis marker ([`spawn_tab_ellipsis`]) that [`apply_tab_ellipsis`] reveals
/// only while the label is actually truncated. The label declares [`TextMayClip`]
/// so the harness's clipping check knows the slice is by design.
fn spawn_tab_button(
    commands: &mut Commands,
    strip: Entity,
    parent: Entity,
    spec: &TabSpec,
    index: usize,
    label: &str,
    active: bool,
) -> Entity {
    // A resizable strip is the one that clips and truncates its labels.
    let clip = spec.is_resizable();
    let button = commands
        .spawn((
            RadioButton,
            TabButton {
                strip,
                index,
                placement: spec.placement,
            },
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                // A resizable tab is pinned to the strip width and clips; a
                // content-sized one grows to its label and centres it.
                justify_content: if clip {
                    JustifyContent::Start
                } else {
                    JustifyContent::Center
                },
                align_items: AlignItems::Center,
                // Always clip — even a content-sized tab whose label fits. This is
                // load-bearing for **picking**, not just drawing: `bevy_ui`'s
                // `clip_check_recursive` stops at the first ancestor with
                // `Overflow::Visible` and declares the node unclipped, so a tab's
                // label (a text node, its own pick target) whose parent button did
                // not clip would be picked even when scrolled out of the viewport —
                // landing on a sibling widget the scrolled-out tab happens to cover.
                // Clipping the button keeps the label's clip chain running up to
                // the scrolling viewport, which correctly rejects it.
                overflow: Overflow::clip(),
                // Rounded corners on the edge away from content are set by
                // `apply_tab_corner_radius` (it needs the live direction); start
                // square.
                ..default()
            },
            BorderColor::all(tab_border(active)),
            BackgroundColor(tab_background(active)),
            Pickable::default(),
            Name::new(format!("{}:tab:{index}", spec.element)),
            ChildOf(parent),
        ))
        .id();

    if clip {
        // A node clips its **descendants**, not its own glyphs — so a text node
        // that clips itself still paints its glyphs past its box, over the
        // ellipsis. The label text therefore sits inside a clipping *container*:
        // the container shrinks below the text (flex-shrink, min-width 0) and
        // clips it, while the text keeps its natural width and is placed at the
        // container's leading edge. Which physical side "leading" is comes from
        // the container's own direction (mirrored by `apply_ui_direction`), so
        // the *start* of the name shows and the *end* clips under RTL as well as
        // LTR, with the ellipsis on the trailing side either way.
        let label_clip = commands
            .spawn((
                Node {
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    align_items: AlignItems::Center,
                    ..default()
                },
                TextMayClip {
                    reason: "a resizable tab strip clips a label longer than its column so the \
                             strip can be narrower than the longest tab name; a trailing ellipsis \
                             marks it",
                },
                Name::new(format!("{}:tab-label:{index}", spec.element)),
                ChildOf(button),
            ))
            .id();
        let label_entity = commands
            .spawn((
                Text::new(spec.initial_label(label)),
                TextLayout::no_wrap(),
                UiFont::Sans.at(spec.font_size),
                TextColor(TAB_LABEL_COLOR),
                // Natural width, so the container — not the text — is what shrinks
                // and clips, and the text overflows the container's trailing edge.
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(label_clip),
            ))
            .id();
        translate_tab_label(commands, label_entity, spec, label);
        let ellipsis = spawn_tab_ellipsis(commands, button, spec, index);
        commands
            .entity(label_clip)
            .insert(TabLabelClip { ellipsis });
    } else {
        let label_entity = commands
            .spawn((
                Text::new(spec.initial_label(label)),
                TextLayout::no_wrap(),
                UiFont::Sans.at(spec.font_size),
                TextColor(TAB_LABEL_COLOR),
                Name::new(format!("{}:tab-label:{index}", spec.element)),
                ChildOf(button),
            ))
            .id();
        translate_tab_label(commands, label_entity, spec, label);
    }

    button
}

/// Spawn a clipped tab's trailing ellipsis marker (`…`, or whatever
/// [`TabSpec::ellipsis`] configured), hidden until [`apply_tab_ellipsis`] finds
/// the label truncated.
fn spawn_tab_ellipsis(
    commands: &mut Commands,
    button: Entity,
    spec: &TabSpec,
    index: usize,
) -> Entity {
    commands
        .spawn((
            Text::new(spec.ellipsis.to_owned()),
            TextLayout::no_wrap(),
            UiFont::Sans.at(spec.font_size),
            TextColor(TAB_LABEL_COLOR),
            Node {
                // Hidden until the label overflows; never shrinks, so it keeps its
                // room once shown; a small leading gap off the last visible glyph.
                display: Display::None,
                flex_shrink: 0.0,
                margin: UiRect::left(Val::Px(ELLIPSIS_GAP)),
                ..default()
            },
            Name::new(format!("{}:tab-ellipsis:{index}", spec.element)),
            TabEllipsisMarker,
            ChildOf(button),
        ))
        .id()
}

/// Reveal a clipped tab's ellipsis exactly when its label overflows its box, and
/// hide it when the label fits.
///
/// Compares the label's natural width (`ComputedNode::content_size`) against its
/// laid-out box (`ComputedNode::size`): wider means the clip bit, so the marker
/// shows. Runs only for labels that declared [`TabLabelClip`] (a resizable
/// strip's), and guards the write so a settled label does not re-toggle layout.
///
/// The label's leading-edge alignment (so the *start* of the name shows and the
/// *end* clips) and the ellipsis's trailing side (right under LTR, left under
/// RTL) both come from the container's flow, which `apply_ui_direction` mirrors —
/// so this system is direction-agnostic.
fn apply_tab_ellipsis(
    labels: Query<(&ComputedNode, &TabLabelClip)>,
    mut ellipses: Query<&mut Node>,
) {
    for (computed, clip) in &labels {
        let truncated = computed.content_size.x > computed.size.x + f32::EPSILON;
        let Ok(mut node) = ellipses.get_mut(clip.ellipsis) else {
            continue;
        };
        let wanted = if truncated {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != wanted {
            node.display = wanted;
        }
    }
}

/// Show a strip's scroll control (scrollbar or arrows) exactly when its viewport
/// overflows on the scroll axis, and hide it when the tabs fit — so the control
/// appears from available space, never configuration. Hidden, not removed, so the
/// tabs never jump and the measurement stays stable.
fn apply_tab_scroll_controls(
    viewports: Query<&ComputedNode, With<TabViewport>>,
    mut controls: Query<(&TabScrollControl, &mut Visibility)>,
) {
    for (control, mut visibility) in &mut controls {
        let Ok(computed) = viewports.get(control.viewport) else {
            continue;
        };
        let overflow = if control.vertical {
            computed.content_size.y > computed.size.y + f32::EPSILON
        } else {
            computed.content_size.x > computed.size.x + f32::EPSILON
        };
        let wanted = if overflow {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

/// Point each horizontal scroll arrow the right physical way for the live
/// direction: toward the inline end is `▶` under LTR and `◀` under RTL.
fn apply_tab_arrow_glyphs(
    direction: Res<UiDirection>,
    mut glyphs: Query<(&TabArrowGlyph, &mut Text)>,
) {
    for (glyph, mut text) in &mut glyphs {
        // toward-end points right under LTR, left under RTL; toward-start the
        // reverse — the same XOR the corner rounding uses.
        let points_right = glyph.toward_end != direction.is_rtl();
        let wanted = if points_right {
            ARROW_TOWARD_END
        } else {
            ARROW_TOWARD_START
        };
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

/// Scroll the vertical tab viewport under the pointer with the mouse wheel — the
/// horizontal strips scroll by their arrows, but a vertical strip wants the wheel
/// like any list. Mirrors [`crate::virtual_list::scroll_virtual_lists`].
fn scroll_tabs_with_wheel(
    wheel: Res<AccumulatedMouseScroll>,
    hover_map: Res<HoverMap>,
    child_of: Query<&ChildOf>,
    viewports: Query<&TabViewport>,
    mut positions: Query<&mut ScrollPosition>,
) {
    if wheel.delta.y.abs() < f32::EPSILON {
        return;
    }
    let delta = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * ARROW_SCROLL_STEP,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    // Scroll the first hovered entity that is (or is inside) a vertical viewport,
    // matching "the strip the wheel is over".
    for hovered in hover_map.values().flat_map(|hits| hits.keys()) {
        let mut node = *hovered;
        loop {
            if viewports.get(node).is_ok_and(|viewport| viewport.vertical) {
                if let Ok(mut position) = positions.get_mut(node) {
                    position.0.y = (position.0.y - delta).max(0.0);
                }
                return;
            }
            let Ok(parent) = child_of.get(node) else {
                break;
            };
            node = parent.parent();
        }
    }
}

/// A tab's background for its active state.
const fn tab_background(active: bool) -> Color {
    if active {
        TAB_ACTIVE_BACKGROUND
    } else {
        TAB_INACTIVE_BACKGROUND
    }
}

/// A tab's border for its active state — the bright accent is the loudest
/// "selected" signal, independent of focus.
const fn tab_border(active: bool) -> Color {
    if active {
        TAB_ACTIVE_BORDER
    } else {
        TAB_BORDER
    }
}

/// Spawn the draggable divider between a resizable strip and its panel, wiring
/// the drag that resizes the strip.
fn spawn_divider(
    commands: &mut Commands,
    container: Entity,
    spec: &TabSpec,
    strip: Entity,
) -> Entity {
    let placement = spec.placement;
    let divider = commands
        .spawn((
            Node {
                width: Val::Px(DIVIDER_THICKNESS),
                // Never shrink below the grab thickness when the container is
                // tight.
                flex_shrink: 0.0,
                // Centre the grip nub in the bar, both axes.
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(DIVIDER_COLOR),
            Pickable::default(),
            TabDivider { strip },
            Name::new(format!("{}:tab-divider", spec.element)),
            ChildOf(container),
        ))
        .id();
    // A short raised nub so the seam reads as a grabbable handle. Ignores the
    // pointer so the drag observer on the bar still receives it.
    commands.spawn((
        Node {
            width: Val::Px(DIVIDER_THICKNESS * 0.5),
            height: Val::Px(DIVIDER_GRIP_LENGTH),
            border_radius: BorderRadius::all(Val::Px(DIVIDER_THICKNESS * 0.25)),
            ..default()
        },
        BackgroundColor(DIVIDER_GRIP_COLOR),
        Pickable::IGNORE,
        Name::new(format!("{}:tab-divider-grip", spec.element)),
        ChildOf(divider),
    ));
    commands
        .entity(divider)
        .observe(
            move |drag: On<Pointer<Drag>>,
                  mut widths: Query<&mut TabStripWidth>,
                  direction: Res<UiDirection>| {
                if drag.button != PointerButton::Primary {
                    return;
                }
                let Ok(mut width) = widths.get_mut(strip) else {
                    return;
                };
                // Assigned unconditionally: a drag event always carries motion, so
                // guarding on an `f32` equality would only trade a real write for a
                // disallowed float comparison.
                width.0 = resize_strip_width(width.0, drag.delta.x, placement, *direction);
            },
        )
        .id()
}

/// The new width a divider drag resolves to, clamped to
/// `[MIN_STRIP_WIDTH, MAX_STRIP_WIDTH]`.
///
/// The sign folds in both the placement and the direction, because widening a
/// leading strip and a trailing one, under LTR and under RTL, are four different
/// screen gestures. A leading strip grows when the divider moves in the inline
/// direction; a trailing strip grows when it moves against it; and RTL flips
/// which way the inline direction points on screen. The product of the two signs
/// is the whole of it — no per-side branch.
fn resize_strip_width(
    current: f32,
    delta_x: f32,
    placement: TabPlacement,
    direction: UiDirection,
) -> f32 {
    let placement_sign = if placement.strip_first() { 1.0 } else { -1.0 };
    let direction_sign = if direction.is_rtl() { -1.0 } else { 1.0 };
    (current + placement_sign * direction_sign * delta_x).clamp(MIN_STRIP_WIDTH, MAX_STRIP_WIDTH)
}

/// Reflect a resizable strip's [`TabStripWidth`] onto its node whenever it
/// changes — from a divider drag or a restore from settings.
///
/// Only the changed strips, and only on a real difference, so an unchanged UI
/// does not re-trigger layout. The initial width is written straight onto the
/// node at spawn, so this handles later changes alone.
fn apply_tab_strip_width(mut strips: Query<(&TabStripWidth, &mut Node), Changed<TabStripWidth>>) {
    for (width, mut node) in &mut strips {
        let wanted = Val::Px(width.0);
        if node.width != wanted {
            node.width = wanted;
        }
    }
}

/// The strip's selection observer: on a [`RadioGroup`] value change — a click or
/// an arrow key — move [`TabStrip::active`] to the picked tab and reconcile
/// everything derived from it (the [`Checked`] flags, the highlight, and the
/// panel visibilities), then emit the [`UiAction`].
///
/// `active` is the one source of truth, so this is the only writer of [`Checked`]
/// and of a tab's [`BackgroundColor`] / a panel's [`Visibility`]. A no-op
/// selection (the active tab re-picked) returns before emitting, so the action
/// means a real change.
fn on_tab_value_change(
    change: On<ValueChange<Entity>>,
    mut commands: Commands,
    mut strips: Query<&mut TabStrip>,
    mut buttons: Query<(Entity, &TabButton, &mut BackgroundColor, &mut BorderColor)>,
    mut panels: Query<(&TabPanel, &mut Visibility)>,
    mut actions: MessageWriter<UiAction>,
) {
    let strip_id = change.source;
    // The event's value is the newly-picked button; its `TabButton` names the
    // index to move to. A value that is not one of this strip's tabs (impossible
    // in practice, but the query is fallible) is ignored.
    let Some(picked) = buttons
        .get(change.value)
        .ok()
        .map(|(_, button, _, _)| button.index)
    else {
        return;
    };
    let Ok(mut strip) = strips.get_mut(strip_id) else {
        return;
    };
    if strip.active == picked {
        return;
    }
    strip.active = picked;
    let element = strip.element;

    for (button, tab, mut background, mut border) in &mut buttons {
        if tab.strip != strip_id {
            continue;
        }
        let is_active = tab.index == picked;
        let wanted_background = tab_background(is_active);
        if background.0 != wanted_background {
            background.0 = wanted_background;
        }
        let wanted_border = BorderColor::all(tab_border(is_active));
        if *border != wanted_border {
            *border = wanted_border;
        }
        if is_active {
            commands.entity(button).insert(Checked);
        } else {
            commands.entity(button).remove::<Checked>();
        }
    }

    for (panel, mut visibility) in &mut panels {
        if panel.strip != strip_id {
            continue;
        }
        let wanted = if panel.index == picked {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }

    actions.write(UiAction {
        element,
        action: TAB_SELECTED_ACTION,
    });
}

// ---------------------------------------------------------------------------
// Gallery elements — one per placement, so `crate::ui_test` sweeps every
// orientation across every script, direction, scale and font size. All are
// content-sized (no divider); the resizable variant is exercised by the unit
// tests and, in the wild, by a host floater.
// ---------------------------------------------------------------------------

/// The tab labels the gallery elements use — short, so a script swap keeps them
/// button-sized. Paired with [`SAMPLE_PANELS`] by index.
const SAMPLE_LABELS: [&str; 3] = ["General", "Graphics", "Sound"];

/// **Distinct** body text per tab, so a switch is unmistakable — the panel's
/// heading (its tab's label) and this line both change. Long enough to wrap and
/// prove the panel reflows.
const SAMPLE_PANELS: [&str; 3] = [
    "General settings: the everyday options a user reaches for first, written long \
     enough that the panel has to wrap it and regrow around whatever language it lands in.",
    "Graphics settings: draw distance, shadows and the quality slider — a different \
     paragraph, so switching tabs visibly swaps the content and not just the heading.",
    "Sound settings: the master volume and the per-source levels, a third distinct body \
     so all three tabs are told apart at a glance when you click between them.",
];

/// Spawn a gallery tab widget at `placement`: three tabs, each fronting a panel
/// with its own heading and body. The shared body of the four registered
/// elements.
fn spawn_tabs_element(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    placement: TabPlacement,
    element: &'static str,
) -> Entity {
    let labels: Vec<String> = SAMPLE_LABELS.iter().map(|label| cx.text(label)).collect();
    let handle = spawn_tab_container(
        commands,
        parent,
        &TabSpec {
            element,
            placement,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: cx.font_size,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );
    fill_sample_panels(commands, &handle.panels, cx);
    handle.container
}

/// Fill each of `panels` with a distinct heading (its tab's label) and body, so a
/// tab switch visibly changes the content.
fn fill_sample_panels(commands: &mut Commands, panels: &[Entity], cx: ElementCx) {
    for (index, &panel) in panels.iter().enumerate() {
        let heading = SAMPLE_LABELS.get(index).copied().unwrap_or("Tab");
        let body = SAMPLE_PANELS.get(index).copied().unwrap_or("");
        commands.spawn((
            Text::new(cx.text(heading)),
            cx.font(UiFont::Sans),
            TextColor(PANEL_HEADING_COLOR),
            ChildOf(panel),
        ));
        commands.spawn((
            Text::new(cx.text(body)),
            cx.font(UiFont::Sans),
            TextColor(PANEL_TEXT_COLOR),
            ChildOf(panel),
        ));
    }
}

/// Gallery element: horizontal tabs on the top edge.
pub(crate) fn spawn_tabs_block_start(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_tabs_element(commands, parent, cx, TabPlacement::BlockStart, "tabs-top")
}

/// Gallery element: horizontal tabs on the bottom edge.
pub(crate) fn spawn_tabs_block_end(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_tabs_element(commands, parent, cx, TabPlacement::BlockEnd, "tabs-bottom")
}

/// Gallery element: vertical tabs on the leading edge (left under LTR).
pub(crate) fn spawn_tabs_inline_start(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_tabs_element(
        commands,
        parent,
        cx,
        TabPlacement::InlineStart,
        "tabs-leading",
    )
}

/// Gallery element: vertical tabs on the trailing edge (right under LTR).
pub(crate) fn spawn_tabs_inline_end(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    spawn_tabs_element(
        commands,
        parent,
        cx,
        TabPlacement::InlineEnd,
        "tabs-trailing",
    )
}

/// Long, data-like tab labels for the resizable demo — the group / avatar names
/// that motivate a movable divider, long enough that a narrow strip must clip
/// them.
const RESIZABLE_LABELS: [&str; 3] = [
    "Sunflower Petrichor Longname",
    "Æther Wintermute-Vandersloot",
    "A Short One",
];

/// Spawn the **resizable vertical tab** demo — a fixed-width strip with a
/// draggable divider and clipped long labels.
///
/// **Not a registered [`crate::ui_element`]**, and deliberately so: a clipped tab
/// label is content wider than its box, which `crate::ui_test::overflow_violations`
/// flags for every node whose overflow is not `Scroll` (clip included), so
/// sweeping it would be a false positive. The gallery hosts it directly instead,
/// as the one place a human can grab the divider and drag it.
pub(crate) fn spawn_tabs_resizable_demo(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let labels: Vec<String> = RESIZABLE_LABELS
        .iter()
        .map(|label| cx.text(label))
        .collect();
    let handle = spawn_tab_container(
        commands,
        parent,
        &TabSpec {
            element: "tabs-resizable",
            placement: TabPlacement::InlineStart,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: cx.font_size,
            strip_width: Some(110.0),
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );
    fill_sample_panels(commands, &handle.panels, cx);
    handle.container
}

/// Spawn a **scroll demo**: `count` numbered tabs at `placement`, so the strip's
/// scroll control (a vertical scrollbar or horizontal arrows) appears when the
/// tabs outgrow the space and stays hidden when they fit. Auto, from available
/// space — the two copies differ only in tab count, never a flag.
///
/// Not registered ([`crate::ui_element`]): a scrolling strip clips its tabs, and
/// the human wants to drive the wheel / arrows here anyway.
pub(crate) fn spawn_tabs_scroll_demo(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    placement: TabPlacement,
    count: usize,
    element: &'static str,
) -> Entity {
    let labels: Vec<String> = (1..=count)
        .map(|number| cx.text(&format!("Tab {number}")))
        .collect();
    let handle = spawn_tab_container(
        commands,
        parent,
        &TabSpec {
            element,
            placement,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: cx.font_size,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: false,
        },
    );
    // Distinct per-widget content (the element id + tab number), so a switch in
    // one demo is unmistakably *its own* panel and not the neighbour's, and every
    // one of the many tabs has something to show.
    for (index, &panel) in handle.panels.iter().enumerate() {
        commands.spawn((
            Text::new(cx.text(&format!("{element} · panel {}", index.saturating_add(1)))),
            cx.font(UiFont::Sans),
            TextColor(PANEL_HEADING_COLOR),
            ChildOf(panel),
        ));
    }
    handle.container
}

#[cfg(test)]
mod tests {
    use super::{
        ARROW_SCROLL_STEP, MAX_STRIP_WIDTH, MIN_STRIP_WIDTH, SAMPLE_LABELS, TAB_ACTIVE_BACKGROUND,
        TAB_INACTIVE_BACKGROUND, TAB_SELECTED_ACTION, TabButton, TabContainerHandle, TabDivider,
        TabLabelClip, TabPanel, TabPlacement, TabScrollControl, TabSpec, TabStrip, TabStripWidth,
        TabViewport, apply_tab_strip_width, arrow_scroll_delta, resize_strip_width,
        spawn_tab_container, spawn_tab_strip,
    };
    use crate::ui::{UiDirection, UiRoot, spawn_ui_root};
    use crate::ui_element::UiAction;
    use bevy::ecs::world::CommandQueue;
    use bevy::input_focus::tab_navigation::TabIndex;
    use bevy::prelude::*;
    use bevy::ui::Checked;
    use bevy::ui_widgets::ValueChange;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The placements paired with whether they are vertical and strip-first — the
    /// whole of the layout contract, checked once here.
    #[test]
    fn placement_geometry() {
        for (placement, vertical, strip_first) in [
            (TabPlacement::BlockStart, false, true),
            (TabPlacement::BlockEnd, false, false),
            (TabPlacement::InlineStart, true, true),
            (TabPlacement::InlineEnd, true, false),
        ] {
            assert_eq!(placement.is_vertical(), vertical, "{placement:?} vertical");
            assert_eq!(
                placement.strip_first(),
                strip_first,
                "{placement:?} strip-first"
            );
            // The scroll viewport stacks a vertical strip's buttons in a column
            // and scrolls the block axis; a horizontal one flows them in a row and
            // scrolls the inline axis.
            let viewport = placement.viewport_node();
            if vertical {
                assert_eq!(viewport.flex_direction, FlexDirection::Column);
                assert_eq!(
                    viewport.overflow.y,
                    OverflowAxis::Scroll,
                    "a vertical strip scrolls"
                );
            } else {
                assert_eq!(viewport.flex_direction, FlexDirection::Row);
                assert_eq!(
                    viewport.overflow.x,
                    OverflowAxis::Scroll,
                    "a horizontal strip scrolls"
                );
            }
            // The wrapper is always a row (viewport + controls) and the container
            // runs across the strip: a row for vertical tabs (strip beside panel),
            // a column for horizontal (strip over panel). The container stretches
            // so the strip is bounded to the panel and overflows rather than grows.
            assert_eq!(
                placement.wrapper_node(None).flex_direction,
                FlexDirection::Row
            );
            let container = placement.container_node();
            assert_eq!(container.align_items, AlignItems::Stretch);
            let want = if vertical {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
            assert_eq!(container.flex_direction, want, "{placement:?} container");
        }
    }

    /// A minimal app with the scaffold root and the `UiAction` message, enough to
    /// spawn a widget and drive its observer by triggering the value change the
    /// `RadioGroup` would.
    fn tab_app() -> App {
        let mut app = App::new();
        app.insert_resource(UiDirection::default())
            .add_message::<UiAction>()
            .add_systems(Startup, spawn_ui_root);
        app.update();
        app
    }

    /// The root the fixtures parent themselves to.
    fn root(app: &App) -> Entity {
        app.world().resource::<UiRoot>().0
    }

    /// The sample labels as owned strings.
    fn sample_labels() -> Vec<String> {
        SAMPLE_LABELS
            .iter()
            .map(|label| (*label).to_owned())
            .collect()
    }

    /// A fixture spec at `placement` with the sample labels; `strip_width` turns
    /// on the resizable divider.
    fn fixture_spec(
        labels: &[String],
        placement: TabPlacement,
        active: usize,
        strip_width: Option<f32>,
    ) -> TabSpec<'_> {
        TabSpec {
            element: "fixture",
            placement,
            labels,
            active,
            tab_index: 1,
            font_size: 15.0,
            strip_width,
            ellipsis: super::DEFAULT_ELLIPSIS,
            translate_labels: false,
        }
    }

    /// Spawn a full tab container into `app` and settle a frame, returning its
    /// handle — the `Commands`-queue dance the constructors need outside a system.
    fn spawn_container(
        app: &mut App,
        parent: Entity,
        placement: TabPlacement,
        active: usize,
        strip_width: Option<f32>,
    ) -> TabContainerHandle {
        let labels = sample_labels();
        let mut queue = CommandQueue::default();
        let handle = {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_tab_container(
                &mut commands,
                parent,
                &fixture_spec(&labels, placement, active, strip_width),
            )
        };
        queue.apply(app.world_mut());
        app.update();
        handle
    }

    /// Spawn a bare strip into `app` and settle a frame, returning its entity.
    fn spawn_strip(app: &mut App, parent: Entity, active: usize) -> Entity {
        let labels = sample_labels();
        let mut queue = CommandQueue::default();
        let strip = {
            let mut commands = Commands::new(&mut queue, app.world());
            spawn_tab_strip(
                &mut commands,
                parent,
                &fixture_spec(&labels, TabPlacement::BlockStart, active, None),
            )
        };
        queue.apply(app.world_mut());
        app.update();
        strip
    }

    /// The one strip in the world — the widget owns it, so a consumer (and a
    /// test) finds it by its [`TabStrip`] component rather than a returned handle.
    fn the_strip(app: &mut App) -> Entity {
        let mut query = app.world_mut().query_filtered::<Entity, With<TabStrip>>();
        query
            .iter(app.world())
            .next()
            .unwrap_or(Entity::PLACEHOLDER)
    }

    /// The tab buttons in the world, ordered by their tab index.
    fn tab_buttons(app: &mut App) -> Vec<Entity> {
        let mut query = app.world_mut().query::<(Entity, &TabButton)>();
        let mut buttons: Vec<(usize, Entity)> = query
            .iter(app.world())
            .map(|(entity, button)| (button.index, entity))
            .collect();
        buttons.sort_by_key(|(index, _)| *index);
        buttons.into_iter().map(|(_, entity)| entity).collect()
    }

    /// The divider handle in the world, if any.
    fn the_divider(app: &mut App) -> Option<Entity> {
        let mut query = app.world_mut().query_filtered::<Entity, With<TabDivider>>();
        query.iter(app.world()).next()
    }

    /// The button of `strip` at `index`.
    fn button_of(app: &mut App, strip: Entity, index: usize) -> Option<Entity> {
        let mut query = app.world_mut().query::<(Entity, &TabButton)>();
        query
            .iter(app.world())
            .find(|(_, button)| button.strip == strip && button.index == index)
            .map(|(entity, _)| entity)
    }

    /// The active index a strip reports, or a sentinel if it lost its component.
    fn strip_active(app: &App, strip: Entity) -> usize {
        app.world()
            .get::<TabStrip>(strip)
            .map_or(usize::MAX, |strip| strip.active)
    }

    /// Whether an entity currently carries [`Checked`].
    fn is_checked(app: &App, entity: Entity) -> bool {
        app.world().get::<Checked>(entity).is_some()
    }

    /// Whether a panel is shown — hidden panels stay laid out (so the widget
    /// keeps the max size), so this reads [`Visibility`], not `Display`.
    fn panel_shown(app: &App, entity: Entity) -> bool {
        app.world()
            .get::<Visibility>(entity)
            .is_some_and(|visibility| *visibility != Visibility::Hidden)
    }

    /// A button's background colour.
    fn background(app: &App, entity: Entity) -> Color {
        app.world()
            .get::<BackgroundColor>(entity)
            .map_or(Color::NONE, |background| background.0)
    }

    /// Pick a tab exactly as a click or an arrow key would — by triggering the
    /// value change the `RadioGroup` emits — and settle a frame.
    fn select(app: &mut App, strip: Entity, button: Entity) {
        app.world_mut().trigger(ValueChange::<Entity> {
            source: strip,
            value: button,
            is_final: true,
        });
        app.update();
    }

    /// Every [`UiAction`] emitted since the last drain.
    fn drained_actions(app: &mut App) -> Vec<UiAction> {
        app.world_mut()
            .resource_mut::<Messages<UiAction>>()
            .drain()
            .collect()
    }

    /// Selecting a tab moves `active`, moves the `Checked` flag and the highlight,
    /// reveals the picked panel and hides the rest, and emits one `UiAction`.
    #[test]
    fn selecting_a_tab_switches_everything() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let handle = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let panels = handle.panels.clone();
        let strip = the_strip(&mut app);
        let buttons = tab_buttons(&mut app);
        let tab0 = *buttons.first().ok_or("no tab 0")?;
        let tab2 = *buttons.get(2).ok_or("no tab 2")?;

        // Resting state: tab 0 active — checked, highlighted (not merely after the
        // first switch), its panel shown; tab 2 inactive.
        assert_eq!(strip_active(&app, strip), 0);
        assert!(is_checked(&app, tab0));
        assert!(!is_checked(&app, *buttons.get(1).ok_or("no tab 1")?));
        assert_eq!(
            background(&app, tab0),
            TAB_ACTIVE_BACKGROUND,
            "the initial active tab is highlighted at rest"
        );
        assert_eq!(background(&app, tab2), TAB_INACTIVE_BACKGROUND);
        assert!(panel_shown(&app, *panels.first().ok_or("no panel 0")?));
        assert!(!panel_shown(&app, *panels.get(2).ok_or("no panel 2")?));

        // Pick tab 2.
        select(&mut app, strip, tab2);

        assert_eq!(strip_active(&app, strip), 2);
        assert!(!is_checked(&app, tab0));
        assert!(is_checked(&app, tab2));
        assert_eq!(background(&app, tab2), TAB_ACTIVE_BACKGROUND);
        assert_eq!(background(&app, tab0), TAB_INACTIVE_BACKGROUND);
        assert!(!panel_shown(&app, *panels.first().ok_or("no panel 0")?));
        assert!(panel_shown(&app, *panels.get(2).ok_or("no panel 2")?));

        let actions = drained_actions(&mut app);
        assert_eq!(actions.len(), 1, "one action for the switch");
        let action = actions.first().ok_or("no action")?;
        assert_eq!(action.action, TAB_SELECTED_ACTION);
        assert_eq!(action.element, "fixture");
        Ok(())
    }

    /// Re-picking the active tab is a no-op: no action, and `active` unmoved.
    #[test]
    fn re_selecting_the_active_tab_is_inert() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let strip = the_strip(&mut app);
        let tab0 = *tab_buttons(&mut app).first().ok_or("no tab 0")?;

        select(&mut app, strip, tab0);

        assert_eq!(strip_active(&app, strip), 0);
        assert!(
            drained_actions(&mut app).is_empty(),
            "no action for a no-op"
        );
        Ok(())
    }

    /// The strip is one focus stop (the group carries the `TabIndex`), and the
    /// tab buttons are not individually focusable — the ARIA tablist shape.
    #[test]
    fn only_the_strip_is_a_focus_stop() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let strip = spawn_strip(&mut app, parent, 0);
        let buttons = tab_buttons(&mut app);

        assert!(
            app.world().get::<TabIndex>(strip).is_some(),
            "the strip is focusable"
        );
        for button in &buttons {
            assert!(
                app.world().get::<TabIndex>(*button).is_none(),
                "a tab button is not individually focusable"
            );
        }
        Ok(())
    }

    /// An out-of-range `active` is clamped, never left with no tab selected.
    #[test]
    fn an_out_of_range_active_is_clamped() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let strip = spawn_strip(&mut app, parent, 99);
        let buttons = tab_buttons(&mut app);
        let last = SAMPLE_LABELS.len() - 1;
        assert_eq!(strip_active(&app, strip), last);
        assert!(is_checked(&app, *buttons.get(last).ok_or("no last tab")?));
        Ok(())
    }

    /// The strip leads or trails the panel area by placement, so RTL then mirrors
    /// an inline placement across the screen for free (the scaffold's job).
    #[test]
    fn the_strip_leads_or_trails_by_placement() -> Result<(), TestError> {
        for (placement, strip_first) in [
            (TabPlacement::InlineStart, true),
            (TabPlacement::InlineEnd, false),
        ] {
            let mut app = tab_app();
            let parent = root(&app);
            let handle = spawn_container(&mut app, parent, placement, 0, None);
            let strip = the_strip(&mut app);
            let children = app
                .world()
                .get::<Children>(handle.container)
                .ok_or("the container has no children")?;
            let strip_index = children
                .iter()
                .position(|child| child == strip)
                .ok_or("the strip is not a child of its container")?;
            let want = if strip_first { 0 } else { 1 };
            assert_eq!(strip_index, want, "{placement:?}: strip child position");
        }
        Ok(())
    }

    /// The divider-drag sign folds in placement and direction: widening a leading
    /// strip and a trailing one, under LTR and RTL, are the four gestures — and
    /// the result clamps.
    #[expect(
        clippy::float_cmp,
        reason = "the resize arithmetic yields exact values, asserted exactly"
    )]
    #[test]
    fn resize_strip_width_sign_and_clamp() {
        // A rightward (+x) drag from a 100 px strip.
        for (placement, direction, want) in [
            (TabPlacement::InlineStart, UiDirection::Ltr, 110.0),
            (TabPlacement::InlineStart, UiDirection::Rtl, 90.0),
            (TabPlacement::InlineEnd, UiDirection::Ltr, 90.0),
            (TabPlacement::InlineEnd, UiDirection::Rtl, 110.0),
        ] {
            assert_eq!(
                resize_strip_width(100.0, 10.0, placement, direction),
                want,
                "{placement:?} {direction:?}"
            );
        }
        // The clamp holds at both ends.
        assert_eq!(
            resize_strip_width(45.0, -100.0, TabPlacement::InlineStart, UiDirection::Ltr),
            MIN_STRIP_WIDTH
        );
        assert_eq!(
            resize_strip_width(395.0, 100.0, TabPlacement::InlineStart, UiDirection::Ltr),
            MAX_STRIP_WIDTH
        );
    }

    /// A resizable vertical container gets a divider, a fixed strip width applied
    /// to the node, a persistable [`TabStripWidth`], clipped labels, and a
    /// stretched container so the divider is full-height.
    #[test]
    fn a_resizable_vertical_container_is_wired() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let handle = spawn_container(&mut app, parent, TabPlacement::InlineStart, 0, Some(120.0));
        let strip = the_strip(&mut app);
        let buttons = tab_buttons(&mut app);

        let divider = the_divider(&mut app).ok_or("a resizable container has a divider")?;
        assert_eq!(
            app.world().get::<TabDivider>(divider).map(|d| d.strip),
            Some(strip),
            "the divider names its strip"
        );
        assert_eq!(
            app.world().get::<TabStripWidth>(strip).map(|w| w.0),
            Some(120.0),
            "the strip carries its persistable width"
        );
        assert_eq!(
            app.world().get::<Node>(strip).map(|node| node.width),
            Some(Val::Px(120.0)),
            "the width is on the node from the start"
        );
        // Each tab has a clippable label: it declares the harness exception and
        // names a live ellipsis marker.
        let mut clips = app.world_mut().query::<(Entity, &TabLabelClip)>();
        let labels: Vec<(Entity, Entity)> = clips
            .iter(app.world())
            .map(|(label, clip)| (label, clip.ellipsis))
            .collect();
        assert_eq!(
            labels.len(),
            buttons.len(),
            "every tab has a clippable label"
        );
        for (label, ellipsis) in labels {
            assert!(
                app.world()
                    .get::<crate::ui_element::TextMayClip>(label)
                    .is_some(),
                "a clipped tab label declares the exception"
            );
            assert!(
                app.world().get::<Text>(ellipsis).is_some(),
                "the label names a live ellipsis marker"
            );
        }
        assert_eq!(
            app.world()
                .get::<Node>(handle.container)
                .map(|node| node.align_items),
            Some(AlignItems::Stretch),
            "a resizable container stretches so the divider is full-height"
        );
        Ok(())
    }

    /// A fixed width on a **horizontal** strip is ignored — no divider, no width
    /// component, content-sized as ever.
    #[test]
    fn a_width_on_horizontal_tabs_is_ignored() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, Some(120.0));
        let strip = the_strip(&mut app);
        assert!(
            the_divider(&mut app).is_none(),
            "no divider for horizontal tabs"
        );
        assert!(
            app.world().get::<TabStripWidth>(strip).is_none(),
            "no width component for horizontal tabs"
        );
        Ok(())
    }

    /// Restoring a width — the persistence-seed path — writes it onto the node via
    /// [`apply_tab_strip_width`].
    #[test]
    fn restoring_a_width_updates_the_node() -> Result<(), TestError> {
        let mut app = tab_app();
        app.add_systems(Update, apply_tab_strip_width);
        let parent = root(&app);
        spawn_container(&mut app, parent, TabPlacement::InlineEnd, 0, Some(120.0));
        let strip = the_strip(&mut app);

        // As `crate::floater_persist` would on restore: write the stored width.
        app.world_mut()
            .get_mut::<TabStripWidth>(strip)
            .ok_or("the strip lost its width")?
            .0 = 200.0;
        app.update();

        assert_eq!(
            app.world().get::<Node>(strip).map(|node| node.width),
            Some(Val::Px(200.0)),
            "the restored width reached the node"
        );
        Ok(())
    }

    /// A strip's buttons live in a scroll viewport, with a scroll control that
    /// starts hidden (it only shows when the tabs overflow) and matches the
    /// orientation.
    #[test]
    fn a_strip_scrolls_its_buttons_with_an_orientation_matched_control() -> Result<(), TestError> {
        for (placement, vertical) in [
            (TabPlacement::BlockStart, false),
            (TabPlacement::InlineStart, true),
        ] {
            let mut app = tab_app();
            let parent = root(&app);
            spawn_container(&mut app, parent, placement, 0, None);

            // A viewport of the right orientation exists, and the tab buttons are
            // inside it (not direct children of the strip).
            let (viewport, viewport_vertical) = {
                let mut query = app.world_mut().query::<(Entity, &TabViewport)>();
                query
                    .iter(app.world())
                    .next()
                    .map(|(entity, viewport)| (entity, viewport.vertical))
                    .ok_or("no scroll viewport")?
            };
            assert_eq!(viewport_vertical, vertical, "{placement:?} viewport axis");
            let buttons = tab_buttons(&mut app);
            for button in &buttons {
                let parent = app
                    .world()
                    .get::<ChildOf>(*button)
                    .map(ChildOf::parent)
                    .ok_or("a tab button has no parent")?;
                assert_eq!(parent, viewport, "a tab button lives in the viewport");
            }

            // A scroll control of the right orientation exists, hidden at rest
            // (few tabs, and no layout in this bare app to measure overflow).
            let mut controls = app.world_mut().query::<(&TabScrollControl, &Visibility)>();
            let control = controls
                .iter(app.world())
                .find(|(control, _)| control.viewport == viewport)
                .ok_or("no scroll control for the viewport")?;
            assert_eq!(control.0.vertical, vertical, "{placement:?} control axis");
            assert_eq!(
                *control.1,
                Visibility::Hidden,
                "the control is hidden until the tabs overflow"
            );
        }
        Ok(())
    }

    /// **Real layout:** a strip with more tabs than the bound holds overflows its
    /// viewport (so its control shows), and a light one does not. Driven through
    /// the layout harness so it is the actual measured sizes, not a guess about
    /// flexbox.
    #[test]
    fn a_full_strip_overflows_its_viewport_and_a_light_one_does_not() -> Result<(), TestError> {
        use crate::ui::{UiRoot, UiScaffoldSystems};
        use crate::ui_test::{LayoutTest, settle};
        for (placement, vertical, few, many) in [
            (TabPlacement::BlockStart, false, 3usize, 16usize),
            (TabPlacement::InlineStart, true, 2usize, 16usize),
        ] {
            for (count, want_overflow) in [(few, false), (many, true)] {
                let mut app = LayoutTest::new().build();
                let labels: Vec<String> =
                    (1..=count).map(|number| format!("Tab {number}")).collect();
                app.add_systems(
                    Startup,
                    (move |mut commands: Commands, root: Res<UiRoot>| {
                        spawn_tab_container(
                            &mut commands,
                            root.0,
                            &super::TabSpec {
                                element: "overflow-fixture",
                                placement,
                                labels: &labels,
                                active: 0,
                                tab_index: 1,
                                font_size: 15.0,
                                strip_width: None,
                                ellipsis: super::DEFAULT_ELLIPSIS,
                                translate_labels: false,
                            },
                        );
                    })
                    .after(UiScaffoldSystems::SpawnRoot),
                );
                settle(&mut app);

                let mut query = app.world_mut().query::<(&ComputedNode, &TabViewport)>();
                let computed = query
                    .iter(app.world())
                    .next()
                    .map(|(computed, _)| *computed)
                    .ok_or("no viewport laid out")?;
                // A logical-pixel slack, as the harness's own overflow check uses.
                let slack = 2.0 * computed.inverse_scale_factor;
                let overflow = if vertical {
                    computed.content_size.y > computed.size.y + slack
                } else {
                    computed.content_size.x > computed.size.x + slack
                };
                assert_eq!(
                    overflow, want_overflow,
                    "{placement:?} with {count} tabs: overflow"
                );
            }
        }
        Ok(())
    }

    /// Two tab containers under one parent are isolated: switching one moves only
    /// its own panels and highlight, never the other's.
    #[test]
    fn two_containers_are_isolated() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let a = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let b = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        // Each panel names its own strip; that is how we tell the two apart.
        let strip_a = app
            .world()
            .get::<TabPanel>(*a.panels.first().ok_or("a has no panel")?)
            .map(|panel| panel.strip)
            .ok_or("a's panel lost its strip")?;

        // Pick tab 2 of container A.
        let a_tab2 = button_of(&mut app, strip_a, 2).ok_or("a has no tab 2")?;
        select(&mut app, strip_a, a_tab2);

        // A switched…
        assert!(
            panel_shown(&app, *a.panels.get(2).ok_or("a panel 2")?),
            "A switched"
        );
        assert!(!panel_shown(&app, *a.panels.first().ok_or("a panel 0")?));
        // …and B did not, in either its content or its header.
        assert!(
            panel_shown(&app, *b.panels.first().ok_or("b panel 0")?),
            "B unmoved"
        );
        assert!(!panel_shown(&app, *b.panels.get(2).ok_or("b panel 2")?));
        Ok(())
    }

    /// Every tab button clips its content. This is the fix for the cross-widget
    /// pick leak: `bevy_ui`'s `clip_check_recursive` stops at the first
    /// `Overflow::Visible` ancestor, so a tab label whose button did not clip
    /// stayed pickable when scrolled out of the viewport and landed on whatever
    /// sibling widget it covered.
    #[test]
    fn tab_buttons_clip_so_scrolled_out_labels_cannot_be_picked() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        for button in tab_buttons(&mut app) {
            let overflow = app
                .world()
                .get::<Node>(button)
                .map(|node| node.overflow)
                .ok_or("a tab button lost its node")?;
            assert_eq!(
                overflow,
                Overflow::clip(),
                "a tab button must clip its label"
            );
        }
        Ok(())
    }

    /// A scroll arrow's step folds in placement intent and direction: toward the
    /// inline end is `+x` under LTR and `-x` under RTL, toward the start the
    /// reverse.
    #[expect(
        clippy::float_cmp,
        reason = "the step is an exact multiple of the constant, asserted exactly"
    )]
    #[test]
    fn arrow_scroll_delta_folds_in_direction() {
        assert_eq!(
            arrow_scroll_delta(true, UiDirection::Ltr),
            ARROW_SCROLL_STEP
        );
        assert_eq!(
            arrow_scroll_delta(true, UiDirection::Rtl),
            -ARROW_SCROLL_STEP
        );
        assert_eq!(
            arrow_scroll_delta(false, UiDirection::Ltr),
            -ARROW_SCROLL_STEP
        );
        assert_eq!(
            arrow_scroll_delta(false, UiDirection::Rtl),
            ARROW_SCROLL_STEP
        );
    }
}
