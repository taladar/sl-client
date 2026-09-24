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
//!   (`inventory`): its Everything / Recent / Worn tabs drive **one**
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
//!   strip on the leading / trailing edge. Under `UiDirection::Ltr` the leading
//!   edge is the left one and the trailing edge the right; under
//!   `UiDirection::Rtl` they swap, with no code here saying so — the container
//!   is a `ui::row` and `ui::apply_ui_direction` reverses the
//!   flow, exactly as the scaffold's convention 1 promises.
//!
//! `InlineEnd` is therefore not only "the RTL mirror of a left strip". It is a
//! first-class placement a skin or a user setting can choose for an LTR layout
//! too — right-hand vertical tabs the reference cannot express — and it mirrors
//! under RTL like any other logical placement.
//!
//! # When the tabs outgrow the strip
//!
//! A strip sizes to its content where it can, but a floater of fixed width
//! and a long list of tabs (or a long translation) can overflow it. The tabs
//! then scroll inside a clipped viewport, and a control appears beside them —
//! from available space, never from configuration:
//!
//! - a **vertical** strip gets the shared
//!   [`scrollbar`](sl_viewer_ui_core::scrollbar), so it wears whatever a skin
//!   gives every other bar (thickness, arrow ends). The reference gives a
//!   vertical tab container a pair of ▲ / ▼ step buttons instead; a bar is
//!   kept on purpose, because it also shows *where* in a long list you are and
//!   drags, and a skin that wants the step buttons turns on the bar's ends.
//! - a **horizontal** strip gets the reference tab container's four overflow
//!   buttons (`mJumpPrevArrowBtn`, `mPrevArrowBtn`, `mNextArrowBtn`,
//!   `mJumpNextArrowBtn`): jump to the first tab, back one, on one, jump to
//!   the last. A step moves by a **tab**, not by pixels, lining the next tab's
//!   leading edge up with the strip's, and back / on repeat while held. The
//!   glyphs are the skin's, and a skin can drop the two jumps
//!   (`--tab-jump-buttons`).
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
//! over-long label (declaring `ui_element::TextMayClip`, the harness's
//! sanctioned exception), and puts a **draggable divider** between the strip and
//! the panel so the split can be moved. The width is a component
//! ([`TabStripWidth`]) so it is the one source of truth: the drag writes it, and
//! [`crate::floater_persist`] saves and restores it per host floater, so a window
//! reopens with the split where the user left it. The drag's sign folds in both
//! the placement and `UiDirection` — widening a
//! leading strip and a trailing one, under LTR and RTL, are four different screen
//! gestures resolved by [`resize_strip_width`] — so the handle behaves under a
//! mirrored layout with no per-side code.
//!
//! # A disabled tab
//!
//! [`InteractionDisabled`](bevy::ui::InteractionDisabled) on a single tab
//! button refuses the switch *to that tab* and greys only its label — the
//! reference's per-tab `LLTabContainer::enableTabButton`. There is deliberately
//! no container-wide equivalent: a strip whose content is all disabled is still
//! one whose tabs you may read, the reference has no such thing either, and
//! nothing in the viewer ever wanted one. Bevy's marker is advisory (it updates the
//! a11y tree and nothing else), so the widget enforces it in its own
//! value-change and divider-drag observers.
//!
//! Scrolling is not a gesture on the *selection*, so the scroll arrows, the
//! scrollbar and the wheel stay live on a disabled strip: a strip that will not
//! switch is still one whose tabs must be readable. The active tab keeps its
//! highlight for the same reason.
//!
//! # Constructible without wiring
//!
//! Per the registry rule (`ui_element`): selecting a tab is pure UI
//! state and never reaches a session, so the widget switches panels itself and,
//! for the harness, emits a `UiAction` naming that a switch happened. A consumer
//! that must *do* something on a tab change (the inventory rebuilding its list)
//! reacts to `Changed<TabStrip>` and reads [`TabStrip::active`] — it is not wired
//! into the widget. The gallery registers one element per placement
//! ([`spawn_tabs_block_start`] and friends) so every orientation is swept by
//! `ui_test`.
//!
//! Reference (Firestorm, read-only): `indra/llui/lltabcontainer.{h,cpp}`
//! (`LLTabContainer`).

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::{Checked, UiSystems};
use bevy::ui_widgets::{Activate, ActivateOnPress, Button, RadioButton, RadioGroup, ValueChange};

use bevy_flair::style::components::{ClassList, PseudoElementsSupport};
use sl_viewer_ui_core::ui::{
    FocusRevealBounds, HideWith, PanelVisibility, TabStopsFollowVisibility, UiDirection, column,
    row,
};
use sl_viewer_ui_core::ui_element::{ElementCx, TextMayClip, UiAction};

use sl_viewer_ui_core::hold_repeat::{HoldToRepeat, ensure_hold_repeat};
use sl_viewer_ui_core::scrollbar::{ScrollTarget, ensure_scrollbar_widget, spawn_scrollbar};
use sl_viewer_ui_core::skin::{TAB_SCROLL_BUTTON_CLASS, TEXT_CLASS};
use sl_viewer_ui_core::skin_palette::SkinPalette;
use sl_viewer_ui_core::ui_ellipsis::{RevealEllipsis, spawn_ellipsis_marker};
use sl_viewer_ui_core::ui_font::UiFont;

/// The gap between adjacent tab buttons, in logical pixels.
const TAB_GAP: f32 = 4.0;

/// The gap between the tab strip and the panel it fronts, in logical pixels. Zero
/// so the strip abuts its panel, the way a real tabbed container reads.
const STRIP_PANEL_GAP: f32 = 0.0;

/// A panel's widest allowed width, in logical pixels — a bound, never a size, so
/// prose wraps inside it rather than overflowing (convention 2). Narrower than a
/// standalone panel's bound to leave room for a vertical strip beside it.
const PANEL_MAX_WIDTH: f32 = 320.0;

/// The widest a **horizontal** strip may grow, in logical pixels, before its
/// tabs scroll instead of growing the widget — about eight tabs at the default
/// size, and the mirror of [`TAB_STRIP_MAX_HEIGHT`] for the other orientation.
///
/// Deliberately much wider than a panel's bound ([`PANEL_MAX_WIDTH`]), which is
/// about wrapping *prose*: a strip of four short tabs in a wide pane was being
/// measured against the prose bound and grew scroll arrows with room to spare
/// either side of it — a widget arguing with a window nobody has.
const TAB_STRIP_MAX_WIDTH: f32 = 640.0;

/// The tallest a **vertical** strip may grow, in logical pixels, before its tabs
/// scroll instead of growing the widget. A definite bound is what makes the
/// overflow real: a content-sized container otherwise just grows to fit every
/// tab, and nothing ever scrolls (the reference's tab container is likewise a
/// fixed size, from its floater). About seven tabs at the default size.
const TAB_STRIP_MAX_HEIGHT: f32 = 220.0;

/// The radius of a tab's rounded corners, in logical pixels. Applied only to the
/// two corners on the edge **away** from the content ([`tab_corner_radius`]), so
/// a tab reads as a tab rather than a plain button.
const TAB_CORNER_RADIUS: f32 = 8.0;

/// The default truncation glyph for a clipped tab label — a single Latin
/// ellipsis. See [`TabSpec::ellipsis`] for why this is configurable.
pub const DEFAULT_ELLIPSIS: &str = "…";

/// A tab label's colour — the skin's primary text role.
///
/// The live widget takes this from `.sk-tab-label` in the skin, and greys from
/// `.sk-tab:disabled .sk-tab-label`; this remains for the spawn-time value of
/// nodes a free function builds before the cascade reaches them, and for the
/// unit tests, which run with no stylesheet at all.
#[must_use]
pub const fn tab_label_color(palette: &SkinPalette) -> Color {
    palette.text_primary
}

/// The skin class on a tab box. Its selected and refused looks are the skin's
/// `:checked` / `:disabled` rules over the `Checked` / `InteractionDisabled`
/// the widget already maintains, so there is no state class to keep in step.
const TAB_CLASS: &str = "sk-tab";

/// The skin class on a tab's caption and its ellipsis marker. Greyed by
/// `.sk-tab:disabled .sk-tab-label` — an ancestor rule, because `bevy_ui` has
/// no style inheritance and the caption is its own node.
///
/// `pub` because a caption is also what a *filter* dims: the preferences
/// search hangs [`NO_MATCH_CLASS`](sl_viewer_ui_core::skin::NO_MATCH_CLASS) on
/// this node, and its fixture spawns the same pair.
pub const TAB_LABEL_CLASS: &str = "sk-tab-label";

/// The skin class on the panel area — the "content" shade the active tab
/// shares (`--card-bg`).
const PANEL_CLASS: &str = "sk-tab-panel";

/// The skin class on a gallery demo panel's heading — brighter than the body,
/// so a tab switch (which swaps the heading) is unmistakable.
const PANEL_HEADING_CLASS: &str = "sk-heading";

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

/// The skin class on the divider handle (`--divider`).
const DIVIDER_CLASS: &str = "sk-divider";

/// The skin class on the divider's grip nub (`--accent`) — brighter than the
/// bar, so the handle stands out.
const DIVIDER_GRIP_CLASS: &str = "sk-divider-grip";

/// How far one wheel notch moves a vertical strip's tabs, in logical pixels.
const WHEEL_LINE_STEP: f32 = 64.0;

/// The skin class on an overflow button's glyph host, whose `::before` is the
/// arrow (`--tab-scroll-arrow`).
const TAB_SCROLL_GLYPH_CLASS: &str = "sk-tab-scroll-glyph";

/// The action a strip emits when the user switches tabs. A single verb — "a
/// switch happened" — because the *which* is readable directly from
/// [`TabStrip::active`]; the `UiAction` exists so the harness can assert the
/// switch occurred without a session behind it.
pub const TAB_SELECTED_ACTION: &str = "select-tab";

/// Where a tab strip sits relative to the panel it fronts, named logically so the
/// side is chosen independently of the reading direction — see the [module
/// documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabPlacement {
    /// A horizontal strip along the top edge (the block-start edge, never
    /// mirrored).
    BlockStart,
    /// A horizontal strip along the bottom edge (the block-end edge, never
    /// mirrored).
    BlockEnd,
    /// A vertical strip on the leading edge — left under `UiDirection::Ltr`,
    /// right under `UiDirection::Rtl`.
    InlineStart,
    /// A vertical strip on the trailing edge — right under `UiDirection::Ltr`,
    /// left under `UiDirection::Rtl`.
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
    /// `ui::column` when the tabs are horizontal (strip stacked over
    /// panel) and a `ui::row` when they are vertical (strip beside
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
            node.max_width = Val::Px(TAB_STRIP_MAX_WIDTH);
        }
        node
    }

    /// The scrolling **viewport** the buttons flow in: a `ui::column` for
    /// a vertical strip, a `ui::row` for a horizontal one, scrolling on
    /// that axis and shrinkable below its content so it clips rather than growing
    /// the strip. `flex_grow` fills the wrapper beside the controls.
    ///
    /// **Both** axes are unpinned, and the cross one is not decoration. A flex
    /// item's `min-width: auto` resolves to its *min-content* width, and a
    /// no-wrap label has no break opportunity, so its min-content width is the
    /// whole name: without `min_width: 0` a vertical viewport cannot shrink below
    /// the longest tab name, the buttons keep their full width inside a strip
    /// pinned to a narrower one, and the labels spill out of the strip instead of
    /// clipping — which also means their ellipsis marker is never revealed, since
    /// nothing ever overflows. Found by
    /// `a_widened_strip_takes_the_ellipsis_off_a_label_that_fits_again`, which
    /// could not reach the state it was written to test.
    fn viewport_node(self) -> Node {
        if self.is_vertical() {
            Node {
                overflow: Overflow {
                    x: OverflowAxis::Clip,
                    y: OverflowAxis::Scroll,
                },
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                min_width: Val::Px(0.0),
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
pub struct TabSpec<'labels> {
    /// The element id the strip reports in its `UiAction`, and the prefix of
    /// its nodes' [`Name`]s. Also the stable key [`crate::floater_persist`] saves
    /// a resizable strip's width under.
    pub element: &'static str,
    /// Where the strip sits relative to its panel.
    pub placement: TabPlacement,
    /// The tab labels, in order; their count is the number of tabs.
    pub labels: &'labels [String],
    /// The initially-active tab, clamped into range.
    pub active: usize,
    /// The strip's single focus stop (the group, not the buttons) — pick it to
    /// slot the strip into the surrounding tab order.
    pub tab_index: i32,
    /// The tab labels' font size, in logical pixels.
    pub font_size: f32,
    /// A fixed width for a **vertical** strip, in logical pixels, which turns on
    /// the draggable divider and label truncation. `None` (the default) keeps the
    /// strip content-sized with no divider; ignored for horizontal placements.
    pub strip_width: Option<f32>,
    /// The glyphs appended to a tab label that had to be truncated (only ever
    /// shown on a clipped, resizable strip). Configurable because the convention
    /// is not universal — Latin uses a single ellipsis `…`, while Chinese and
    /// Japanese use a centred six-dot `……`; a locale layer
    /// (`viewer-i18n-fluent-scaffold`) is where this would eventually come from.
    /// Use [`DEFAULT_ELLIPSIS`] where the caller has no locale of its own.
    pub ellipsis: &'static str,
    /// Whether [`labels`](Self::labels) are Fluent **keys** to translate
    /// (`i18n::Translated`, re-resolved on locale change / bundle load)
    /// rather than literal display text. A translated strip's labels start empty
    /// and fill once the bundle loads. Use it for real UI; `false` for the
    /// gallery and tests, whose labels are fixed sample text.
    pub translate_labels: bool,
}

impl TabSpec<'_> {
    /// Whether this spec asks for a resizable divider: a fixed width on a
    /// vertical strip.
    const fn is_resizable(&self) -> bool {
        self.placement.is_vertical() && self.strip_width.is_some()
    }

    /// The text a label node starts with: empty for a translated strip (the key
    /// is not display text, and `i18n::Translated` fills the real text once
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
/// `i18n::apply_translations` keeps it resolved; a no-op for a literal
/// strip.
fn translate_tab_label(commands: &mut Commands, label_entity: Entity, spec: &TabSpec, label: &str) {
    if spec.translate_labels {
        commands
            .entity(label_entity)
            .insert(sl_viewer_ui_core::i18n::Translated::new(label.to_owned()));
    }
}

/// A tab strip's state: which tab is active. The **single source of truth** — the
/// [`Checked`] flags, the highlight and the panel visibilities are all derived
/// from it, so nothing can drift.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabStrip {
    /// The element id this strip reports in its `UiAction`, and the prefix of
    /// its nodes' [`Name`]s.
    pub element: &'static str,
    /// The index of the active tab, into the strip's buttons in spawn order.
    pub active: usize,
}

/// A resizable vertical strip's width, in logical pixels — the single source of
/// truth for its inline size. The divider drag writes it, `apply_tab_strip_width`
/// reflects it onto the node, and [`crate::floater_persist`] saves and restores
/// it. Present only on a strip spawned with [`TabSpec::strip_width`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TabStripWidth(pub f32);

/// The draggable divider between a resizable strip and its panel, naming the
/// strip it resizes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabDivider {
    /// The strip whose [`TabStripWidth`] this handle drags.
    pub strip: Entity,
}

/// A tab button: which strip it belongs to and its index within it. Carried so
/// the selection observer can find every button of a strip and place it against
/// the strip's [`active`](TabStrip::active) index.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabButton {
    /// The strip ([`RadioGroup`]) this button is a tab of.
    pub strip: Entity,
    /// This tab's index within the strip.
    pub index: usize,
    /// The strip's placement, so `apply_tab_corner_radius` can round the two
    /// corners on the edge away from the content.
    pub placement: TabPlacement,
}

/// A tab panel: which strip switches it and which tab reveals it.
///
/// Hidden panels are toggled with [`Visibility`], **not** the scaffold's
/// `UiPanelShown` / `Display::None`: they must stay laid out so the panel area
/// sizes to the largest of them and the widget does not shrink when a lighter tab
/// is selected. That is the *only* difference — the tab-stop parking and the
/// focus drop are identical, so both go through the same
/// [`PanelVisibility`] ([`HideWith::Visibility`] here,
/// [`HideWith::Display`] there) and a focusable in a switched-away panel is out
/// of the `Tab` cycle exactly as it is in a closed one.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabPanel {
    /// The strip that switches this panel.
    pub strip: Entity,
    /// The tab index that reveals it.
    pub index: usize,
}

/// The scrolling viewport a strip's buttons live in — bounded to the available
/// space (the panel size) and scrolling when the tabs outgrow it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabViewport {
    /// Whether it scrolls on the block axis (a vertical strip) or the inline axis
    /// (a horizontal strip). Drives which measurement decides overflow and which
    /// controls appear.
    pub vertical: bool,
}

/// A horizontal strip's group of overflow buttons, shown by
/// `apply_tab_scroll_controls` only while its viewport overflows, so it
/// appears from available space, never configuration. (A vertical strip's
/// scrollbar hides itself: the shared widget does that for every bar.)
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabScrollControl {
    /// The [`TabViewport`] these buttons scroll.
    pub viewport: Entity,
}

/// What one of a horizontal strip's overflow buttons does — the reference
/// tab container's four: jump to the first tab, step back one, step on one,
/// jump to the last. Named for the **inline** order, so under RTL "first" is
/// the rightmost tab and its button sits on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabStep {
    /// Scroll the first tab into the leading edge (`jump_left` in the
    /// reference).
    First,
    /// Scroll one tab back toward the first.
    Prev,
    /// Scroll one tab on toward the last.
    Next,
    /// Scroll the last tab into the trailing edge (`jump_right`).
    Last,
}

impl TabStep {
    /// All four, in inline order — the order the buttons are spawned in.
    const ALL: [Self; 4] = [Self::First, Self::Prev, Self::Next, Self::Last];

    /// The name suffix and the skin class that tells the four apart.
    const fn name_and_class(self) -> (&'static str, &'static str) {
        match self {
            Self::First => ("first", "sk-tab-scroll-first"),
            Self::Prev => ("prev", "sk-tab-scroll-prev"),
            Self::Next => ("next", "sk-tab-scroll-next"),
            Self::Last => ("last", "sk-tab-scroll-last"),
        }
    }

    /// Whether it steps one tab at a time — the two that repeat while held.
    const fn is_step(self) -> bool {
        matches!(self, Self::Prev | Self::Next)
    }
}

/// One of a horizontal strip's overflow buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct TabScrollButton {
    /// The viewport it scrolls.
    viewport: Entity,
    /// What it does.
    step: TabStep,
}

/// What [`spawn_tab_container`] hands back: the outer container, the panel
/// area, and the panel slots for the caller to fill.
///
/// Deliberately just these — the widget owns its own strip, buttons and
/// divider (a consumer finds the strip by its [`TabStrip`] component to react to
/// `Changed<TabStrip>`), so returning them would be surface nobody reads. A
/// consumer that comes to need one adds the field with its reader.
#[derive(Debug, Clone)]
pub struct TabContainerHandle {
    /// The outer container node.
    pub container: Entity,
    /// The strip wrapper (the `[viewport, controls]` row;
    /// [`fill_tab_container`] lifts its scroll-axis bound so the bar tracks a
    /// definite parent).
    pub strip: Entity,
    /// The one-cell grid the panels stack in ([`fill_tab_container`] restyles
    /// it to track a definite parent).
    pub panel_area: Entity,
    /// The panel slots, in tab order — spawn each tab's content into these.
    pub panels: Vec<Entity>,
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
pub struct TabWidgetPlugin;

impl Plugin for TabWidgetPlugin {
    fn build(&self, app: &mut App) {
        // The overflow buttons' previous / next repeat while held, and a
        // vertical strip's bar hides itself while the tabs fit.
        ensure_hold_repeat(app);
        ensure_scrollbar_widget(app);
        app.add_systems(PostUpdate, apply_tab_strip_width.before(UiSystems::Layout))
            .add_systems(
                Update,
                (
                    apply_tab_corner_radius,
                    apply_programmatic_tab_selection,
                    scroll_tabs_with_wheel,
                ),
            )
            // After layout, because it reads each viewport's *measured* size to
            // decide overflow and writes next frame, the same shape as the pie's
            // post-layout fit. Truncation is the other half of that shape, and it
            // is `ui_ellipsis::apply_reveal_ellipsis` — registered once by
            // `ViewerUiPlugin`, because the table and the inventory clip too.
            .add_systems(
                PostUpdate,
                apply_tab_scroll_controls.after(UiSystems::Layout),
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
/// the active highlight, keyboard selection, and a `UiAction` on change.
///
/// [`TabSpec::active`] is clamped into range, so a caller cannot spawn a strip
/// with nothing selected. The returned strip carries [`TabStrip`], whose `active`
/// is the source of truth: a consumer that only needs the selection reacts to
/// `Changed<TabStrip>` and reads it.
pub fn spawn_tab_strip(commands: &mut Commands, parent: Entity, spec: &TabSpec) -> Entity {
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
    // are hidden until `apply_tab_scroll_controls` finds the viewport
    // overflowing, so they appear from available space, not configuration.
    if vertical {
        spawn_tab_scrollbar(commands, strip, viewport, spec);
    } else {
        spawn_tab_scroll_arrows(commands, strip, viewport, spec);
    }

    strip
}

/// Spawn a vertical strip's scrollbar (the shared
/// [`scrollbar`](sl_viewer_ui_core::scrollbar) widget, driving the viewport) at
/// the strip's trailing edge, hidden until it is needed.
fn spawn_tab_scrollbar(commands: &mut Commands, strip: Entity, viewport: Entity, spec: &TabSpec) {
    // Shown by the widget itself only while the tabs overflow — hidden, not
    // removed, so the tabs never jump when it appears.
    spawn_scrollbar(
        commands,
        strip,
        ScrollTarget::Container(viewport),
        Node::default(),
        &format!("{}:tab-scrollbar", spec.element),
    );
}

/// Spawn a horizontal strip's overflow buttons at its trailing edge — jump to
/// first, previous, next, jump to last, as the reference's tab container has
/// them — hidden until the tabs overflow.
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
                align_items: AlignItems::Stretch,
                ..default()
            },
            Visibility::Hidden,
            TabScrollControl { viewport },
            Name::new(format!("{}:tab-arrows", spec.element)),
            ChildOf(strip),
        ))
        .id();
    for step in TabStep::ALL {
        spawn_tab_scroll_button(commands, arrows, viewport, spec, step);
    }
}

/// Spawn one overflow button: an empty glyph host whose `::before` the skin
/// fills (and turns round under `dir="rtl"`), on a face the skin paints. The
/// two single steps repeat while held; the two jumps do not, as in the
/// reference.
fn spawn_tab_scroll_button(
    commands: &mut Commands,
    parent: Entity,
    viewport: Entity,
    spec: &TabSpec,
    step: TabStep,
) {
    let (suffix, class) = step.name_and_class();
    let mut button = commands.spawn((
        Button,
        ActivateOnPress,
        TabScrollButton { viewport, step },
        Node {
            padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        ClassList::new_with_classes([TAB_SCROLL_BUTTON_CLASS, class]),
        Pickable::default(),
        Name::new(format!("{}:tab-arrow:{suffix}", spec.element)),
        ChildOf(parent),
    ));
    if step.is_step() {
        button.insert(HoldToRepeat::default());
    }
    let button = button.observe(on_tab_scroll_button).id();
    commands.spawn((
        Text::default(),
        PseudoElementsSupport,
        UiFont::Sans.at(spec.font_size),
        TextColor(tab_label_color(&SkinPalette::default())),
        ClassList::new_with_classes([TAB_SCROLL_GLYPH_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// An overflow button was pressed (or a step button is being held): scroll
/// its viewport to the offset [`tab_scroll_target`] picks.
fn on_tab_scroll_button(
    activate: On<Activate>,
    buttons: Query<&TabScrollButton>,
    mut viewports: Query<(
        &mut ScrollPosition,
        &ComputedNode,
        &UiGlobalTransform,
        &Children,
    )>,
    tabs: Query<(&ComputedNode, &UiGlobalTransform), With<TabButton>>,
    direction: Res<UiDirection>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Ok((mut position, computed, transform, children)) = viewports.get_mut(button.viewport)
    else {
        return;
    };
    let scale = computed.inverse_scale_factor();
    let visible = (computed.size().x - computed.scrollbar_size.x) * scale;
    let range = (computed.content_size().x * scale - visible).max(0.0);
    // Each tab's two edges in content coordinates: where it is on screen,
    // relative to the viewport's own left edge, plus how far the content has
    // already scrolled.
    let viewport_left = (transform.translation.x - computed.size().x / 2.0) * scale;
    let edges: Vec<(f32, f32)> = children
        .iter()
        .filter_map(|child| tabs.get(child).ok())
        .map(|(tab, tab_transform)| {
            let left = (tab_transform.translation.x - tab.size().x / 2.0) * scale - viewport_left
                + position.x;
            (left, left + tab.size().x * scale)
        })
        .collect();
    position.x = tab_scroll_target(
        button.step,
        &edges,
        position.x,
        visible,
        range,
        direction.is_rtl(),
    );
}

/// The horizontal offset an overflow button scrolls a strip to.
///
/// `edges` are the tabs' left and right edges in content coordinates,
/// `offset` the current scroll, `visible` the viewport's width and `range`
/// how far it can scroll. A single step lines the **next tab's leading edge**
/// up with the viewport's leading edge — the reference's `mScrollPos`, which
/// counts tabs rather than pixels — so a click never leaves a tab half
/// scrolled in. Under LTR the leading edge is the left one; under RTL the
/// right one, and "toward the end" is toward a smaller offset.
fn tab_scroll_target(
    step: TabStep,
    edges: &[(f32, f32)],
    offset: f32,
    visible: f32,
    range: f32,
    rtl: bool,
) -> f32 {
    /// How far a candidate must be from the current offset to count as a step,
    /// in logical pixels, so rounding never makes a click a no-op.
    const EPSILON: f32 = 0.5;
    // The offset that lines each tab's leading edge up with the viewport's.
    let mut stops: Vec<f32> = edges
        .iter()
        .map(|&(left, right)| if rtl { right - visible } else { left })
        .map(|stop| stop.clamp(0.0, range))
        .collect();
    stops.sort_by(f32::total_cmp);
    // The inline start is offset 0 under LTR and the far end of the range
    // under RTL; "on" is toward the other one.
    let (start, end) = if rtl { (range, 0.0) } else { (0.0, range) };
    let forward = !rtl;
    let target = match step {
        TabStep::First => start,
        TabStep::Last => end,
        TabStep::Next if forward => stops
            .iter()
            .copied()
            .find(|stop| *stop > offset + EPSILON)
            .unwrap_or(end),
        TabStep::Next => stops
            .iter()
            .rev()
            .copied()
            .find(|stop| *stop < offset - EPSILON)
            .unwrap_or(end),
        TabStep::Prev if forward => stops
            .iter()
            .rev()
            .copied()
            .find(|stop| *stop < offset - EPSILON)
            .unwrap_or(start),
        TabStep::Prev => stops
            .iter()
            .copied()
            .find(|stop| *stop > offset + EPSILON)
            .unwrap_or(start),
    };
    target.clamp(0.0, range)
}

/// Spawn the whole tab widget under `parent`: a [`spawn_tab_strip`] strip plus a
/// panel area holding one empty panel slot per tab, only the active one shown —
/// and, for a resizable vertical layout, a draggable divider between the two.
///
/// The panels come back empty in [`TabContainerHandle::panels`]; the caller
/// spawns each tab's content into them. Which panel is visible tracks the strip's
/// selection with no wiring on the caller's part.
pub fn spawn_tab_container(
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
            BackgroundColor(SkinPalette::default().card_bg),
            ClassList::new_with_classes([PANEL_CLASS]),
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
                // difference from the scaffold's `UiPanelShown` — and the marker
                // beside it is what tells the scaffold that this one *is* a
                // managed subtree, so a panel not on screen is out of the `Tab`
                // cycle as well as unseen.
                if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                TabStopsFollowVisibility,
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
    TabContainerHandle {
        container,
        strip,
        panel_area,
        panels,
    }
}

/// Restyle a just-spawned tab container to **fill a definite-size parent** (a
/// resizable floater's content slot) instead of content-sizing: the container,
/// strip and panel area grow (min 0) to the space the parent gives them, the
/// one grid cell becomes `1fr` so the panels track that size rather than their
/// content, and each panel scrolls its overflow vertically — by wheel (the
/// panels join `scroll_tabs_with_wheel` via [`TabViewport`]) and by a
/// trailing-edge scrollbar that appears only while its panel both overflows
/// and is the visible one.
///
/// A content-driven host (the default) skips this and keeps the widget sized
/// to its largest panel.
pub fn fill_tab_container(
    commands: &mut Commands,
    placement: TabPlacement,
    handle: &TabContainerHandle,
) {
    let mut container_node = placement.container_node();
    container_node.flex_grow = 1.0;
    container_node.min_width = Val::Px(0.0);
    container_node.min_height = Val::Px(0.0);
    commands.entity(handle.container).insert(container_node);
    // The strip's scroll-axis cap becomes the parent's size, so the tab bar
    // widens (or, vertical, lengthens) with the floater instead of stopping at
    // the fixed bound.
    let mut strip_node = placement.wrapper_node(None);
    if placement.is_vertical() {
        strip_node.max_height = Val::Percent(100.0);
    } else {
        strip_node.max_width = Val::Percent(100.0);
    }
    commands.entity(handle.strip).insert(strip_node);
    commands.entity(handle.panel_area).insert(Node {
        display: Display::Grid,
        // `1fr`, not `auto`: the cell takes the area's size, so the panels
        // track the parent instead of the largest panel's content.
        grid_template_columns: vec![GridTrack::flex(1.0)],
        grid_template_rows: vec![GridTrack::flex(1.0)],
        flex_grow: 1.0,
        min_width: Val::Px(0.0),
        min_height: Val::Px(0.0),
        ..default()
    });
    for panel in &handle.panels {
        commands.entity(*panel).insert((
            Node {
                grid_column: GridPlacement::start(1),
                grid_row: GridPlacement::start(1),
                padding: UiRect::all(Val::Px(12.0)),
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                ..column(Val::Px(8.0))
            },
            ScrollPosition::default(),
            // The wheel system scrolls the innermost vertical viewport under
            // the pointer; a nested strip inside the panel still wins because
            // the walk starts at the hovered node.
            TabViewport { vertical: true },
        ));
        // The panel's scrollbar: a later sibling in the same grid cell (so it
        // draws over the panel), hugging the trailing edge. The widget shows it
        // only while the panel overflows and is not itself the hidden one.
        spawn_scrollbar(
            commands,
            handle.panel_area,
            ScrollTarget::Container(*panel),
            Node {
                grid_column: GridPlacement::start(1),
                grid_row: GridPlacement::start(1),
                justify_self: JustifySelf::End,
                ..default()
            },
            "fill-panel-scrollbar",
        );
    }
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
/// ellipsis marker ([`spawn_tab_ellipsis`]) that
/// [`sl_viewer_ui_core::ui_ellipsis::apply_reveal_ellipsis`] reveals only while
/// the label is actually truncated. The label declares [`TextMayClip`]
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
            // Selected / disabled are `:checked` / `:disabled` in the skin,
            // both driven by components the widget already maintains, so the
            // tab needs no state class and nothing paints it from Rust.
            ClassList::new_with_classes([TAB_CLASS]),
            Pickable::default(),
            Name::new(format!("{}:tab:{index}", spec.element)),
            ChildOf(parent),
        ))
        .id();
    if active {
        // The selected tab is `Checked` from its first frame, so the skin's
        // `:checked` rule dresses it before the reconcile runs. The reconcile
        // keeps it in step from then on.
        commands.entity(button).insert(Checked);
    }

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
                ClassList::new_with_classes([TAB_LABEL_CLASS]),
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
        // Greys with its label, from the same `.sk-tab:disabled` ancestor rule.
        commands
            .entity(ellipsis)
            .insert(ClassList::new_with_classes([TAB_LABEL_CLASS]));
        commands
            .entity(label_clip)
            .insert(RevealEllipsis { marker: ellipsis });
    } else {
        let label_entity = commands
            .spawn((
                Text::new(spec.initial_label(label)),
                TextLayout::no_wrap(),
                UiFont::Sans.at(spec.font_size),
                ClassList::new_with_classes([TAB_LABEL_CLASS]),
                Name::new(format!("{}:tab-label:{index}", spec.element)),
                ChildOf(button),
            ))
            .id();
        translate_tab_label(commands, label_entity, spec, label);
    }

    button
}

/// Spawn a clipped tab's trailing ellipsis marker (`…`, or whatever
/// [`TabSpec::ellipsis`] configured), hidden until `apply_reveal_ellipsis` finds
/// the label truncated.
///
/// A thin wrapper over the shared [`spawn_ellipsis_marker`]: the marker, plus the
/// name the layout harness addresses it by.
fn spawn_tab_ellipsis(
    commands: &mut Commands,
    button: Entity,
    spec: &TabSpec,
    index: usize,
) -> Entity {
    let marker = spawn_ellipsis_marker(
        commands,
        button,
        spec.font_size,
        tab_label_color(&SkinPalette::default()),
        spec.ellipsis,
    );
    commands
        .entity(marker)
        .insert(Name::new(format!("{}:tab-ellipsis:{index}", spec.element)));
    marker
}

/// Show a horizontal strip's overflow buttons exactly when its viewport
/// overflows, and hide them when the tabs fit — so they appear from available
/// space, never configuration. Hidden, not removed, so the tabs never jump and
/// the measurement stays stable.
fn apply_tab_scroll_controls(
    viewports: Query<&ComputedNode, With<TabViewport>>,
    mut controls: Query<(&TabScrollControl, &mut Visibility), Without<TabViewport>>,
) {
    for (control, mut visibility) in &mut controls {
        let Ok(computed) = viewports.get(control.viewport) else {
            continue;
        };
        let overflow = computed.content_size.x > computed.size.x + f32::EPSILON;
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

/// Scroll the vertical tab viewport under the pointer with the mouse wheel — the
/// horizontal strips scroll by their arrows, but a vertical strip wants the wheel
/// like any list. Mirrors `virtual_list::scroll_virtual_lists`.
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
        MouseScrollUnit::Line => wheel.delta.y * WHEEL_LINE_STEP,
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
            BackgroundColor(SkinPalette::default().divider),
            ClassList::new_with_classes([DIVIDER_CLASS]),
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
        BackgroundColor(SkinPalette::default().accent),
        ClassList::new_with_classes([DIVIDER_GRIP_CLASS]),
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
#[must_use]
pub fn resize_strip_width(
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
/// panel visibilities), then emit the `UiAction`.
///
/// `active` is the one source of truth, so this is the only writer of [`Checked`]
/// and of a tab's [`BackgroundColor`] / a panel's [`Visibility`]. A no-op
/// selection (the active tab re-picked) returns before emitting, so the action
/// means a real change.
fn on_tab_value_change(
    change: On<ValueChange<Entity>>,
    mut strips: Query<&mut TabStrip>,
    mut paint: TabPaint,
    mut actions: MessageWriter<UiAction>,
) {
    let strip_id = change.source;
    // The event's value is the newly-picked button; its `TabButton` names the
    // index to move to — and its own disabled flag comes along, so a single
    // disabled tab refuses the switch *to it* while its neighbours stay live
    // (the reference's per-tab `enableTabButton`). Upstream's `RadioButton`
    // already blocks a click or key on a disabled button; this also refuses a
    // `ValueChange` written straight into the world. A value that is not one of
    // this strip's tabs (impossible in practice, but the query is fallible) is
    // ignored.
    let Some((picked, tab_disabled)) = paint
        .buttons
        .get(change.value)
        .ok()
        .map(|(_, button, disabled)| (button.index, disabled))
    else {
        return;
    };
    let Ok(mut strip) = strips.get_mut(strip_id) else {
        return;
    };
    // Only a *tab* can refuse a switch, never the strip as a whole: a strip
    // whose content is all disabled is still one whose tabs you may read, and
    // no panel in the viewer has ever wanted otherwise. The reference agrees —
    // `LLTabContainer::enableTabButton` disables individual buttons and has no
    // container-wide equivalent.
    if tab_disabled {
        return;
    }
    if strip.active == picked {
        return;
    }
    strip.active = picked;
    let element = strip.element;

    paint.reconcile(strip_id, picked);

    actions.write(UiAction {
        element,
        action: TAB_SELECTED_ACTION,
    });
}

/// Everything a strip's selection is painted into, as one bundle: the tab
/// boxes, their panels, and the panel-visibility helper that parks their tab
/// stops. Both writers — the click / arrow observer ([`on_tab_value_change`])
/// and the programmatic / skin pass ([`apply_programmatic_tab_selection`]) —
/// take this rather than five parameters each, and [`TabPaint::reconcile`] is
/// the one place any of it is written.
#[derive(SystemParam)]
struct TabPaint<'w, 's> {
    /// Every tab box: its strip and index. No paint components — a tab's
    /// selected look is the skin's `:checked` rule, so the only thing this
    /// writes is the flag that rule selects on.
    buttons: Query<
        'w,
        's,
        (
            Entity,
            &'static TabButton,
            Has<bevy::ui::InteractionDisabled>,
        ),
    >,
    /// The panels those tabs front.
    panels: Query<'w, 's, (Entity, &'static TabPanel)>,
    /// Shows / hides a panel, parking its tab stops with it.
    visibility: PanelVisibility<'w, 's>,
    /// The [`Checked`] flag, which is deferred.
    commands: Commands<'w, 's>,
}

impl TabPaint<'_, '_> {
    /// Reconcile everything derived from a strip's [`TabStrip::active`] — the
    /// [`Checked`] flags and its panels' [`Visibility`] — to the given `active`
    /// index.
    ///
    /// A panel's `Visibility` is written through [`PanelVisibility`] rather
    /// than here, because hiding a panel is not only a `Visibility`:
    /// `bevy_input_focus` consults neither `Visibility` nor `Display`, so a
    /// panel switched away from keeps its tab stops and `Tab` walks into it.
    /// That is the same leak the scaffold's `UiPanelShown` parks, and it is
    /// parked in the same place.
    fn reconcile(&mut self, strip_id: Entity, active: usize) {
        for (button, tab, _disabled) in self.buttons.iter_mut() {
            if tab.strip != strip_id {
                continue;
            }
            // `Checked` is the whole of a tab's selected state now: the skin's
            // `:checked` rule paints it, so marking the button is both the
            // accessibility signal and the repaint.
            if tab.index == active {
                self.commands.entity(button).insert(Checked);
            } else {
                self.commands.entity(button).remove::<Checked>();
            }
        }

        // Collected first: the walk `set_shown` does reads `Children` and
        // writes `Visibility`, which it cannot do while this query is still
        // iterating.
        let switched: Vec<(Entity, bool)> = self
            .panels
            .iter()
            .filter(|(_, panel)| panel.strip == strip_id)
            .map(|(entity, panel)| (entity, panel.index == active))
            .collect();
        for (panel, is_active) in switched {
            self.visibility
                .set_shown(panel, is_active, HideWith::Visibility);
        }
    }
}

/// Reconcile a strip's visuals when its [`TabStrip::active`] is set
/// **programmatically** — a consumer writing `TabStrip::active` directly (the
/// build floater's material-mode auto-select) rather than through a click / arrow,
/// which [`on_tab_value_change`] already reconciles. Runs only for the strips that
/// changed this frame, and every write is guarded, so re-running it right after a
/// user selection (which also marks the strip changed) is a no-op.
fn apply_programmatic_tab_selection(
    changed: Query<(Entity, &TabStrip), Changed<TabStrip>>,
    mut paint: TabPaint,
) {
    // This used to widen to EVERY strip whenever the palette changed, because
    // a Rust-painted highlight does not follow a skin switch and a strip nobody
    // touched would keep the previous skin's colours indefinitely. A tab's look
    // is the cascade's now, so `bevy_flair` repaints every strip on a skin,
    // theme or hot-reload change with nothing here involved — and this is back
    // to what its name says: the strips whose selection actually moved.
    let strips: Vec<(Entity, usize)> = changed
        .iter()
        .map(|(id, strip)| (id, strip.active))
        .collect();
    for (strip_id, active) in strips {
        paint.reconcile(strip_id, active);
    }
}

// ---------------------------------------------------------------------------
// Gallery elements — one per placement, so `ui_test` sweeps every
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
            TextColor(SkinPalette::default().text_heading),
            ClassList::new_with_classes([PANEL_HEADING_CLASS]),
            ChildOf(panel),
        ));
        commands.spawn((
            Text::new(cx.text(body)),
            cx.font(UiFont::Sans),
            TextColor(SkinPalette::default().text_primary),
            ClassList::new_with_classes([TEXT_CLASS]),
            ChildOf(panel),
        ));
    }
}

/// Gallery element: horizontal tabs on the top edge.
pub fn spawn_tabs_block_start(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    spawn_tabs_element(commands, parent, cx, TabPlacement::BlockStart, "tabs-top")
}

/// Gallery element: horizontal tabs on the bottom edge.
pub fn spawn_tabs_block_end(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    spawn_tabs_element(commands, parent, cx, TabPlacement::BlockEnd, "tabs-bottom")
}

/// Gallery element: vertical tabs on the leading edge (left under LTR).
pub fn spawn_tabs_inline_start(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    spawn_tabs_element(
        commands,
        parent,
        cx,
        TabPlacement::InlineStart,
        "tabs-leading",
    )
}

/// Gallery element: vertical tabs on the trailing edge (right under LTR).
pub fn spawn_tabs_inline_end(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
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
/// **Not a registered `ui_element`**, and deliberately so: a clipped tab
/// label is content wider than its box, which `ui_test::overflow_violations`
/// flags for every node whose overflow is not `Scroll` (clip included), so
/// sweeping it would be a false positive. The gallery hosts it directly instead,
/// as the one place a human can grab the divider and drag it.
pub fn spawn_tabs_resizable_demo(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
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
/// Not registered (`ui_element`): a scrolling strip clips its tabs, and
/// the human wants to drive the wheel / arrows here anyway.
pub fn spawn_tabs_scroll_demo(
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
            TextColor(SkinPalette::default().text_heading),
            ClassList::new_with_classes([PANEL_HEADING_CLASS]),
            ChildOf(panel),
        ));
    }
    handle.container
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_STRIP_WIDTH, MIN_STRIP_WIDTH, RevealEllipsis, SAMPLE_LABELS, TAB_SELECTED_ACTION,
        TabButton, TabContainerHandle, TabDivider, TabPanel, TabPlacement, TabScrollControl,
        TabSpec, TabStep, TabStrip, TabStripWidth, TabViewport, apply_tab_strip_width,
        resize_strip_width, spawn_tab_container, spawn_tab_strip, tab_scroll_target,
    };
    use sl_viewer_ui_core::scrollbar::{ScrollTarget, ScrollbarFrame};

    use bevy::ecs::system::SystemState;
    use bevy::ecs::world::CommandQueue;
    use bevy::input_focus::tab_navigation::{NavAction, TabIndex, TabNavigation};
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::prelude::*;
    use bevy::ui::Checked;
    use bevy::ui_widgets::ValueChange;
    use pretty_assertions::assert_eq;
    use sl_viewer_ui_core::ui::{
        UiDirection, UiRoot, park_new_tab_stops_in_hidden_subtrees, spawn_ui_root,
    };
    use sl_viewer_ui_core::ui_element::UiAction;

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
    ///
    /// [`InputFocus`] is the scaffold's, not this widget's — live it comes from
    /// `InputFocusPlugin` — but a tab switch drops focus that the panel it hides
    /// was holding, so a harness without it cannot run the observer at all.
    /// `park_new_tab_stops_in_hidden_subtrees` likewise runs here because it is
    /// what parks the widgets a container is *built* with, in the panels it is
    /// built hidden.
    fn tab_app() -> App {
        let mut app = App::new();
        app.insert_resource(UiDirection::default())
            .init_resource::<InputFocus>()
            .add_message::<UiAction>()
            .add_systems(Startup, spawn_ui_root)
            .add_systems(Update, park_new_tab_stops_in_hidden_subtrees);
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

    /// Every `UiAction` emitted since the last drain.
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
        assert!(panel_shown(&app, *panels.first().ok_or("no panel 0")?));
        assert!(!panel_shown(&app, *panels.get(2).ok_or("no panel 2")?));

        // Pick tab 2.
        select(&mut app, strip, tab2);

        assert_eq!(strip_active(&app, strip), 2);
        assert!(!is_checked(&app, tab0));
        assert!(is_checked(&app, tab2));
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
        let mut clips = app.world_mut().query::<(Entity, &RevealEllipsis)>();
        let labels: Vec<(Entity, Entity)> = clips
            .iter(app.world())
            .map(|(label, clip)| (label, clip.marker))
            .collect();
        assert_eq!(
            labels.len(),
            buttons.len(),
            "every tab has a clippable label"
        );
        for (label, ellipsis) in labels {
            assert!(
                app.world()
                    .get::<sl_viewer_ui_core::ui_element::TextMayClip>(label)
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
    /// `apply_tab_strip_width`.
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
            // (few tabs, and no layout in this bare app to measure overflow):
            // the shared scrollbar for a vertical strip, the overflow buttons
            // for a horizontal one.
            let hidden = if vertical {
                let mut bars = app.world_mut().query::<(&ScrollbarFrame, &Visibility)>();
                bars.iter(app.world())
                    .find(|(frame, _)| frame.target == ScrollTarget::Container(viewport))
                    .map(|(_, visibility)| *visibility)
            } else {
                let mut controls = app.world_mut().query::<(&TabScrollControl, &Visibility)>();
                controls
                    .iter(app.world())
                    .find(|(control, _)| control.viewport == viewport)
                    .map(|(_, visibility)| *visibility)
            };
            assert_eq!(
                hidden.ok_or("no scroll control for the viewport")?,
                Visibility::Hidden,
                "{placement:?}: the control is hidden until the tabs overflow"
            );
        }
        Ok(())
    }

    /// **Real layout:** a [`fill_tab_container`]ed widget tracks a
    /// definite-size parent — the resizable-floater shape that motivated it
    /// (`viewer-social-profiles`): the container takes the slot's size and the
    /// panels grow to the panel area instead of content-sizing, while the
    /// default (unfilled) widget stays content-sized.
    #[test]
    fn a_filled_container_tracks_a_definite_parent() -> Result<(), TestError> {
        use crate::ui_test::{LayoutTest, settle};
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column};
        const SLOT: Vec2 = Vec2::new(400.0, 540.0);
        for fill in [false, true] {
            let mut app = LayoutTest::new().build();
            app.add_systems(
                Startup,
                (move |mut commands: Commands, root: Res<UiRoot>| {
                    // The resizable floater's content slot: definite size,
                    // clipped.
                    let slot = commands
                        .spawn((
                            Node {
                                width: Val::Px(SLOT.x),
                                height: Val::Px(SLOT.y),
                                overflow: Overflow::clip(),
                                ..column(Val::Px(6.0))
                            },
                            Name::new("fill-fixture:slot"),
                            ChildOf(root.0),
                        ))
                        .id();
                    let labels: Vec<String> = ["One", "Two", "Three"].map(str::to_owned).into();
                    let tabs = spawn_tab_container(
                        &mut commands,
                        slot,
                        &super::TabSpec {
                            element: "fill-fixture",
                            placement: TabPlacement::BlockStart,
                            labels: &labels,
                            active: 0,
                            tab_index: 1,
                            font_size: 14.0,
                            strip_width: None,
                            ellipsis: super::DEFAULT_ELLIPSIS,
                            translate_labels: false,
                        },
                    );
                    for panel in &tabs.panels {
                        commands.spawn((Text::new("panel line"), ChildOf(*panel)));
                    }
                    if fill {
                        super::fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);
                    }
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            let measure = |app: &mut App, name: &str| -> Option<Vec2> {
                let mut query = app.world_mut().query::<(&ComputedNode, &Name)>();
                query
                    .iter(app.world())
                    .find(|(_computed, node_name)| node_name.as_str() == name)
                    .map(|(computed, _name)| computed.size * computed.inverse_scale_factor)
            };
            let container =
                measure(&mut app, "fill-fixture:tab-container").ok_or("no container")?;
            let panel = measure(&mut app, "fill-fixture:panel:0").ok_or("no panel")?;
            if fill {
                assert!(
                    (container.y - SLOT.y).abs() < 2.0,
                    "filled: the container tracks the slot height ({container})"
                );
                assert!(
                    panel.y > SLOT.y * 0.8,
                    "filled: the panels grow to the area ({panel})"
                );
            } else {
                assert!(
                    container.y < SLOT.y * 0.5,
                    "unfilled: the widget stays content-sized ({container})"
                );
            }
        }
        Ok(())
    }

    /// **Real layout, regression:** a label wide enough to need the ellipsis at
    /// one strip width must stop needing it when the strip is widened back —
    /// including through the band exactly one marker wide, where the reveal used
    /// to latch (`viewer-audit-ellipsis-reveal-latch`).
    ///
    /// The marker is a sibling of the clip and does not shrink, so showing it
    /// takes its own width off the clip. Measuring the label against that shrunk
    /// clip — which all three copies of this system did — makes the answer depend
    /// on the previous answer, and a value whose natural width lands between
    /// `available - marker` and `available` never gets its marker taken away
    /// again.
    ///
    /// The width is **calibrated from the run itself** rather than guessed: the
    /// strip is widened until the label has its own natural width plus half a
    /// marker, which is inside the band by construction whatever the font
    /// measures. `ui_ellipsis::apply_reveal_ellipsis` must then hide the marker
    /// and hand the whole name back.
    #[test]
    fn a_widened_strip_takes_the_ellipsis_off_a_label_that_fits_again() -> Result<(), TestError> {
        use crate::ui_test::{LayoutTest, settle};
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};
        /// Narrow enough that every sample label is clipped to start with.
        const NARROW: f32 = 70.0;

        let mut app = LayoutTest::new().build();
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                let labels: Vec<String> = ["A really quite long tab label".to_owned()].into();
                spawn_tab_container(
                    &mut commands,
                    root.0,
                    &super::TabSpec {
                        element: "latch-fixture",
                        placement: TabPlacement::InlineStart,
                        labels: &labels,
                        active: 0,
                        tab_index: 1,
                        font_size: 14.0,
                        strip_width: Some(NARROW),
                        ellipsis: super::DEFAULT_ELLIPSIS,
                        translate_labels: false,
                    },
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);

        let strip = the_strip(&mut app);
        let mut clips = app.world_mut().query::<(Entity, &RevealEllipsis)>();
        let (clip, marker) = clips
            .iter(app.world())
            .map(|(clip, reveal)| (clip, reveal.marker))
            .next()
            .ok_or("the resizable strip's label declares a reveal")?;

        let displayed = |app: &App, entity: Entity| -> Option<Display> {
            app.world()
                .entity(entity)
                .get::<Node>()
                .map(|node| node.display)
        };
        assert_eq!(
            displayed(&app, marker),
            Some(Display::Flex),
            "the narrow strip clips the label, so the marker starts shown —              without that this test never enters the latching state"
        );

        // Calibrate off the measured boxes: the label's natural width, the box it
        // was given, and what the marker is taking. All physical pixels.
        let (natural, laid_out, inverse_scale) = app
            .world()
            .entity(clip)
            .get::<ComputedNode>()
            .map(|computed| {
                (
                    computed.content_size.x,
                    computed.size.x,
                    computed.inverse_scale_factor,
                )
            })
            .ok_or("the clip is laid out")?;
        let marker_width = app
            .world()
            .entity(marker)
            .get::<ComputedNode>()
            .map(|computed| computed.size.x)
            .ok_or("the shown marker is laid out")?;
        assert!(
            marker_width > 1.0,
            "the marker must have real width for the band to exist ({marker_width} px)"
        );

        // Widen so the label has its natural width plus *half* a marker: it fits
        // whole, but would not fit beside the marker — the band itself.
        let available = laid_out + marker_width;
        let widen = (natural + marker_width * 0.5 - available) * inverse_scale;
        assert!(
            widen > 0.0,
            "the fixture must start narrower than the band ({widen} logical px)"
        );
        let widened = NARROW + widen;
        if let Some(mut node) = app.world_mut().entity_mut(strip).get_mut::<Node>() {
            node.width = Val::Px(widened);
        }
        settle(&mut app);

        assert_eq!(
            displayed(&app, marker),
            Some(Display::None),
            "at {widened} logical px the label fits without the marker, so the \
             marker must come off — measuring against the shrunk clip leaves it \
             on forever"
        );
        let (natural_now, laid_out_now) = app
            .world()
            .entity(clip)
            .get::<ComputedNode>()
            .map(|computed| (computed.content_size.x, computed.size.x))
            .ok_or("the clip is still laid out")?;
        assert!(
            natural_now <= laid_out_now,
            "with the marker gone the whole label must fit its box \
             ({natural_now} px of label in {laid_out_now} px)"
        );
        Ok(())
    }

    /// **Real layout:** a strip with more tabs than its space holds overflows its
    /// viewport (so its control shows), and one that fits does not. Driven
    /// through the layout harness so it is the actual measured sizes, not a
    /// guess about flexbox.
    ///
    /// The horizontal cases pin the **bound itself**: eight tabs sit inside
    /// [`TAB_STRIP_MAX_WIDTH`] and must not grow arrows (a four-tab People strip
    /// that did is what moved this bound off the prose one), while sixteen do
    /// outgrow it. The vertical strip's own bound is [`TAB_STRIP_MAX_HEIGHT`].
    #[test]
    fn a_full_strip_overflows_its_viewport_and_a_light_one_does_not() -> Result<(), TestError> {
        use crate::ui_test::{LayoutTest, settle};
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};
        for (placement, vertical, host, count, want_overflow) in [
            (TabPlacement::BlockStart, false, None::<f32>, 3_usize, false),
            (TabPlacement::BlockStart, false, None, 8, false),
            (TabPlacement::BlockStart, false, None, 16, true),
            (TabPlacement::InlineStart, true, None, 2, false),
            (TabPlacement::InlineStart, true, None, 16, true),
        ] {
            let mut app = LayoutTest::new().build();
            let labels: Vec<String> = (1..=count).map(|number| format!("Tab {number}")).collect();
            app.add_systems(
                Startup,
                (move |mut commands: Commands, root: Res<UiRoot>| {
                    let parent = match host {
                        Some(width) => commands
                            .spawn((
                                Node {
                                    width: Val::Px(width),
                                    ..default()
                                },
                                Name::new("overflow-fixture-host"),
                                ChildOf(root.0),
                            ))
                            .id(),
                        None => root.0,
                    };
                    spawn_tab_container(
                        &mut commands,
                        parent,
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
                "{placement:?} with {count} tabs in a {host:?} host: overflow"
            );
        }
        Ok(())
    }

    /// A focusable in a panel that is not the selected one is out of the `Tab`
    /// cycle — from the moment the container is built, not merely after the
    /// first switch — and comes back with the index it had when its tab is
    /// picked.
    ///
    /// `bevy_input_focus` consults neither `Visibility` nor `Display`, so
    /// without this every tabbed floater in the viewer — preferences, profiles,
    /// places, search, the pickers, inventory — lets `Tab` walk into panels that
    /// are not on screen.
    #[test]
    fn only_the_selected_panel_keeps_its_tab_stops() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let handle = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let panels = handle.panels.clone();

        // A focusable per panel, spawned *after* the container, which is the
        // order every consumer builds in.
        let mut stops = Vec::new();
        for (index, &panel) in panels.iter().enumerate() {
            let want = i32::try_from(index).unwrap_or(0).saturating_add(1);
            stops.push(
                app.world_mut()
                    .spawn((Node::default(), TabIndex(want), ChildOf(panel)))
                    .id(),
            );
        }
        app.update();

        let stop_index = |app: &App, stop: Entity| app.world().get::<TabIndex>(stop).copied();
        assert_eq!(
            stop_index(&app, *stops.first().ok_or("no stop 0")?),
            Some(TabIndex(1)),
            "the selected panel's widget is reachable"
        );
        for (index, &stop) in stops.iter().enumerate().skip(1) {
            assert!(
                stop_index(&app, stop).is_none(),
                "panel {index} is not selected, so `Tab` must not reach into it"
            );
        }

        let strip = the_strip(&mut app);
        let buttons = tab_buttons(&mut app);
        select(&mut app, strip, *buttons.get(2).ok_or("no tab 2")?);

        assert_eq!(
            stop_index(&app, *stops.get(2).ok_or("no stop 2")?),
            Some(TabIndex(3)),
            "the newly selected panel's widget must come back with the index it had"
        );
        assert!(
            stop_index(&app, *stops.first().ok_or("no stop 0")?).is_none(),
            "and the panel switched away from must give its stop up"
        );
        Ok(())
    }

    /// The same thing said by the keyboard rather than by the components:
    /// pressing `Tab` from the strip lands on the selected panel's widget and
    /// never on a switched-away panel's.
    ///
    /// Driven through `bevy_input_focus`'s real `TabNavigation`, because that —
    /// not the presence of a component — is what the user experiences, and it is
    /// the thing that consults neither `Visibility` nor `Display`.
    #[test]
    fn tab_never_lands_in_an_unselected_panel() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let handle = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let panels = handle.panels.clone();
        let mut stops = Vec::new();
        for &panel in &panels {
            stops.push(
                app.world_mut()
                    .spawn((Node::default(), TabIndex(0), ChildOf(panel)))
                    .id(),
            );
        }
        app.update();

        // Walk the whole cycle from nowhere and collect where focus lands.
        let mut visited = Vec::new();
        app.world_mut().resource_mut::<InputFocus>().clear();
        for _step in 0..panels.len().saturating_add(2) {
            let focus = app.world().resource::<InputFocus>().clone();
            let mut state = SystemState::<TabNavigation>::new(app.world_mut());
            let Some(next) = state
                .get(app.world())
                .ok()
                .and_then(|navigation| navigation.navigate(&focus, NavAction::Next).ok())
            else {
                break;
            };
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(next, FocusCause::Navigated);
            app.update();
            visited.push(next);
        }

        for (index, &stop) in stops.iter().enumerate().skip(1) {
            assert!(
                !visited.contains(&stop),
                "`Tab` reached the widget in unselected panel {index}"
            );
        }
        assert!(
            visited.contains(stops.first().ok_or("no stop 0")?),
            "`Tab` must still reach the selected panel's widget"
        );
        Ok(())
    }

    /// Switching away from the panel that holds keyboard focus takes the
    /// keyboard with it.
    ///
    /// Otherwise the hidden panel goes on receiving what is typed — the same
    /// third promise the scaffold's `UiPanelShown` makes, for the same reason.
    #[test]
    fn switching_away_drops_focus_that_was_inside_the_panel() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let handle = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let panel = *handle.panels.first().ok_or("no panel 0")?;
        let stop = app
            .world_mut()
            .spawn((Node::default(), TabIndex(1), ChildOf(panel)))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(stop, FocusCause::Navigated);

        let strip = the_strip(&mut app);
        let buttons = tab_buttons(&mut app);
        select(&mut app, strip, *buttons.get(1).ok_or("no tab 1")?);

        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            None,
            "a panel switched away from must give up the keyboard"
        );
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

    /// **The overflow buttons count tabs, not pixels.** Four 100-wide tabs
    /// with 10 of gap in a 150-wide viewport (range 280): a step lines the next
    /// tab's leading edge up with the viewport's, so no click leaves a tab
    /// half scrolled in, and the jumps go to either end.
    ///
    /// Under RTL the leading edge is the right one and "on" is toward a
    /// smaller offset — the first tab is the rightmost, at the far end of the
    /// range.
    #[expect(
        clippy::float_cmp,
        reason = "the stops are exact sums of the fixture's widths, asserted exactly"
    )]
    #[test]
    fn the_overflow_buttons_step_a_tab_at_a_time() {
        let edges = [(0.0, 100.0), (110.0, 210.0), (220.0, 320.0), (330.0, 430.0)];
        let (visible, range) = (150.0, 280.0);
        let ltr = |step, offset| tab_scroll_target(step, &edges, offset, visible, range, false);
        assert_eq!(ltr(TabStep::Next, 0.0), 110.0, "on to the second tab");
        assert_eq!(ltr(TabStep::Next, 110.0), 220.0, "and the third");
        assert_eq!(ltr(TabStep::Next, 220.0), 280.0, "the last stop is the end");
        assert_eq!(ltr(TabStep::Next, 280.0), 280.0, "and it stays there");
        assert_eq!(ltr(TabStep::Prev, 280.0), 220.0, "back one tab");
        assert_eq!(
            ltr(TabStep::Prev, 150.0),
            110.0,
            "back to a tab edge, not by a width"
        );
        assert_eq!(ltr(TabStep::Prev, 0.0), 0.0, "and never past the start");
        assert_eq!(ltr(TabStep::First, 200.0), 0.0);
        assert_eq!(ltr(TabStep::Last, 0.0), 280.0);

        let rtl = |step, offset| tab_scroll_target(step, &edges, offset, visible, range, true);
        // The trailing (right) edges less the viewport: -50, 60, 170, 280,
        // clamped to 0, 60, 170, 280.
        assert_eq!(
            rtl(TabStep::First, 0.0),
            280.0,
            "the first tab is the rightmost"
        );
        assert_eq!(rtl(TabStep::Last, 280.0), 0.0);
        assert_eq!(
            rtl(TabStep::Next, 280.0),
            170.0,
            "on is toward a smaller offset"
        );
        assert_eq!(rtl(TabStep::Next, 60.0), 0.0);
        assert_eq!(rtl(TabStep::Prev, 60.0), 170.0);
        assert_eq!(rtl(TabStep::Prev, 280.0), 280.0, "never past the start");
    }

    // -----------------------------------------------------------------------
    // The disabled state.
    // -----------------------------------------------------------------------

    /// A strip is **never** disabled as a whole: marking one switches exactly
    /// as it did before.
    ///
    /// The widget used to refuse every switch on a strip wearing
    /// [`InteractionDisabled`](bevy::ui::InteractionDisabled), which nothing in
    /// the viewer ever set — a strip whose content is all disabled is still one
    /// whose tabs you may read, and the reference has no container-wide
    /// equivalent of its per-tab `enableTabButton`. This pins that the marker on
    /// a strip is inert, so the capability cannot creep back in unnoticed.
    #[test]
    fn a_marked_strip_still_switches() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        let handle = spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let panels = handle.panels.clone();
        let strip = the_strip(&mut app);
        let tab2 = *tab_buttons(&mut app).get(2).ok_or("no tab 2")?;
        app.world_mut()
            .entity_mut(strip)
            .insert(bevy::ui::InteractionDisabled);
        app.update();
        let _settled = drained_actions(&mut app);

        select(&mut app, strip, tab2);

        assert_eq!(strip_active(&app, strip), 2, "the switch went through");
        assert!(is_checked(&app, tab2));
        assert!(panel_shown(&app, *panels.get(2).ok_or("no panel 2")?));
        assert_eq!(
            drained_actions(&mut app).len(),
            1,
            "and it announced itself like any other switch"
        );
        Ok(())
    }

    /// The marker on **one tab button** refuses the switch to that tab and greys
    /// only its label — the reference's per-tab `enableTabButton` — while its
    /// neighbours still switch.
    #[test]
    fn one_disabled_tab_leaves_the_others_live() -> Result<(), TestError> {
        let mut app = tab_app();
        let parent = root(&app);
        spawn_container(&mut app, parent, TabPlacement::BlockStart, 0, None);
        let strip = the_strip(&mut app);
        let buttons = tab_buttons(&mut app);
        let frozen = *buttons.get(2).ok_or("no tab 2")?;
        let live = *buttons.get(1).ok_or("no tab 1")?;
        app.world_mut()
            .entity_mut(frozen)
            .insert(bevy::ui::InteractionDisabled);
        app.update();

        select(&mut app, strip, frozen);
        assert_eq!(
            strip_active(&app, strip),
            0,
            "the disabled tab was not switched to"
        );
        select(&mut app, strip, live);
        assert_eq!(strip_active(&app, strip), 1, "its neighbour still switches");

        // Per-tab greying is `.sk-tab:disabled .sk-tab-label` — an ancestor
        // rule, so the flag sits on the button and reaches its caption. What
        // this test can see without a stylesheet is that the flag is where the
        // selector expects it, and only there.
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(frozen)
                .is_some(),
            "the refused tab carries the flag `:disabled` selects on"
        );
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(live)
                .is_none(),
            "its neighbour does not"
        );
        Ok(())
    }

    /// **The strip, driven** (`viewer-ui-widget-interaction-suite`): a real
    /// click on a real tab, in each of the four edge orientations, and a real
    /// drag on the divider between strip and panel.
    ///
    /// The tests above reach the widget through the [`ValueChange`] its
    /// `RadioGroup` would have produced, and a hand-built `Pointer<Drag>` aimed
    /// at the divider by entity. Both skip hit-testing, so both would keep
    /// passing on a strip whose buttons had been laid out somewhere no pointer
    /// can reach — behind the panel, off the edge of the container, clipped out
    /// of the viewport. The orientation loop is the reason that is worth
    /// paying for: which side the strip lands on is decided by flow order and
    /// `strip_first`, and it is exactly the sort of thing that goes wrong for
    /// one placement and not the other three.
    mod scenarios {
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;

        use super::{
            SAMPLE_LABELS, TAB_SELECTED_ACTION, TabPlacement, TabSpec, TabStrip, TabStripWidth,
            TestError, spawn_tab_container,
        };
        use crate::ui_tab::TabWidgetPlugin;
        use crate::ui_test::interact::{self, InteractionTest, centre_of};
        use crate::ui_test::{
            box_of, drain_actions, enable_action_recording, find_by_name, settle,
        };
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};
        use sl_viewer_ui_core::ui_element::UiAction;

        /// The element id every scenario container carries.
        const ELEMENT: &str = "fixture";

        /// One tab container under the real pointer stack, at `placement`, with
        /// tab 0 active and the widget's own plugin driving it.
        fn tab_app(placement: TabPlacement, strip_width: Option<f32>) -> App {
            let mut app = InteractionTest::new().build();
            app.add_plugins(TabWidgetPlugin);
            enable_action_recording(&mut app);
            app.add_systems(
                Startup,
                (move |mut commands: Commands, root: Res<UiRoot>| {
                    let labels: Vec<String> = SAMPLE_LABELS
                        .iter()
                        .map(|label| (*label).to_owned())
                        .collect();
                    spawn_tab_container(
                        &mut commands,
                        root.0,
                        &TabSpec {
                            element: ELEMENT,
                            placement,
                            labels: &labels,
                            active: 0,
                            tab_index: 1,
                            font_size: 15.0,
                            strip_width,
                            ellipsis: super::super::DEFAULT_ELLIPSIS,
                            translate_labels: false,
                        },
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            app
        }

        /// The strip's active index.
        fn active(app: &mut App) -> Option<usize> {
            let strip = find_by_name(app, "fixture:tab-strip")?;
            app.world().get::<TabStrip>(strip).map(|strip| strip.active)
        }

        /// Whether the named panel is the one on screen.
        fn panel_shown(app: &mut App, index: usize) -> Option<bool> {
            let panel = find_by_name(app, &format!("fixture:panel:{index}"))?;
            app.world()
                .get::<InheritedVisibility>(panel)
                .map(|visible| visible.get())
        }

        /// A click on a tab switches the widget — in every placement.
        ///
        /// One app per placement rather than one app with four containers:
        /// which side the strip sits on is a layout fact, and four widgets
        /// sharing a root would let one that landed on top of another pass by
        /// swallowing its clicks.
        #[test]
        fn a_click_switches_the_tab_in_every_placement() -> Result<(), TestError> {
            for placement in [
                TabPlacement::BlockStart,
                TabPlacement::BlockEnd,
                TabPlacement::InlineStart,
                TabPlacement::InlineEnd,
            ] {
                let mut app = tab_app(placement, None);
                let _resting = drain_actions(&mut app);
                assert_eq!(active(&mut app), Some(0), "{placement:?} starts on tab 0");

                interact::click_node(&mut app, "fixture:tab:2")?;
                settle(&mut app);

                assert_eq!(
                    active(&mut app),
                    Some(2),
                    "{placement:?}: clicking tab 2 selects it"
                );
                assert_eq!(
                    panel_shown(&mut app, 2),
                    Some(true),
                    "{placement:?}: tab 2's panel is the one on screen"
                );
                assert_eq!(
                    panel_shown(&mut app, 0),
                    Some(false),
                    "{placement:?}: tab 0's panel went away"
                );
                let actions: Vec<UiAction> = drain_actions(&mut app);
                let action = actions
                    .first()
                    .ok_or_else(|| format!("{placement:?}: the switch announced nothing"))?;
                assert_eq!(action.action, TAB_SELECTED_ACTION);
                assert_eq!(action.element, ELEMENT);
                assert_eq!(
                    actions.len(),
                    1,
                    "{placement:?}: one click, one action: {actions:?}"
                );
            }
            Ok(())
        }

        /// Clicking the tab already open changes nothing and announces nothing —
        /// so a consumer that rebuilds its panel on the action does not rebuild
        /// it every time the user re-clicks where they already are.
        #[test]
        fn clicking_the_open_tab_announces_nothing() -> Result<(), TestError> {
            let mut app = tab_app(TabPlacement::BlockStart, None);
            let _resting = drain_actions(&mut app);

            interact::click_node(&mut app, "fixture:tab:0")?;
            settle(&mut app);

            assert_eq!(active(&mut app), Some(0));
            let actions = drain_actions(&mut app);
            assert!(
                actions.is_empty(),
                "re-clicking the open tab is not a switch: {actions:?}"
            );
            Ok(())
        }

        /// A drag on the divider resizes the strip, and the width follows the
        /// pointer the whole way rather than jumping to where it was released.
        ///
        /// The step-by-step check is the point: the observer sums `drag.delta`,
        /// so a handler that read `distance` (the total since the press)
        /// instead would land at four times the distance for a four-step drag
        /// and still finish "in the right direction".
        #[test]
        fn a_divider_drag_resizes_the_strip_as_the_pointer_moves() -> Result<(), TestError> {
            const START_WIDTH: f32 = 120.0;
            const TRAVEL: f32 = 60.0;

            let mut app = tab_app(TabPlacement::InlineStart, Some(START_WIDTH));
            let strip = find_by_name(&mut app, "fixture:tab-strip").ok_or("no strip")?;
            let width = |app: &App| -> Option<f32> {
                app.world().get::<TabStripWidth>(strip).map(|width| width.0)
            };
            assert_eq!(width(&app), Some(START_WIDTH));

            let grip =
                centre_of(&mut app, "fixture:tab-divider").ok_or("the divider never laid out")?;
            interact::drag(
                &mut app,
                grip,
                Vec2::new(grip.x + TRAVEL, grip.y),
                4,
                MouseButton::Left,
            );
            settle(&mut app);

            let widened = width(&app).ok_or("the strip lost its width")?;
            let wanted = START_WIDTH + TRAVEL;
            assert!(
                (widened - wanted).abs() < 1.0,
                "an {TRAVEL} px drag widens the strip by {TRAVEL} px, not {}: {widened}",
                widened - START_WIDTH
            );

            // And back the other way, which a handler that only ever added
            // would fail.
            let from =
                centre_of(&mut app, "fixture:tab-divider").ok_or("the divider moved away")?;
            interact::drag(
                &mut app,
                from,
                Vec2::new(from.x - TRAVEL, from.y),
                4,
                MouseButton::Left,
            );
            settle(&mut app);
            let narrowed = width(&app).ok_or("the strip lost its width")?;
            assert!(
                (narrowed - START_WIDTH).abs() < 1.0,
                "dragging back returns the strip to {START_WIDTH}: {narrowed}"
            );
            Ok(())
        }

        /// A horizontal strip of `count` tabs under the real pointer stack —
        /// more than fit, so the overflow buttons show.
        fn overflowing_strip_app(count: usize) -> App {
            strip_app(TabPlacement::BlockStart, count)
        }

        /// A strip of `count` tabs at `placement` under the real pointer stack.
        fn strip_app(placement: TabPlacement, count: usize) -> App {
            let mut app = InteractionTest::new().build();
            app.add_plugins(TabWidgetPlugin);
            enable_action_recording(&mut app);
            app.add_systems(
                Startup,
                (move |mut commands: Commands, root: Res<UiRoot>| {
                    let labels: Vec<String> =
                        (1..=count).map(|number| format!("Tab {number}")).collect();
                    spawn_tab_container(
                        &mut commands,
                        root.0,
                        &TabSpec {
                            element: ELEMENT,
                            placement,
                            labels: &labels,
                            active: 0,
                            tab_index: 1,
                            font_size: 15.0,
                            strip_width: None,
                            ellipsis: super::super::DEFAULT_ELLIPSIS,
                            translate_labels: false,
                        },
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            settle(&mut app);
            app
        }

        /// The strip's horizontal scroll offset.
        fn strip_offset(app: &mut App) -> Option<f32> {
            let viewport = find_by_name(app, "fixture:tab-viewport")?;
            app.world()
                .get::<ScrollPosition>(viewport)
                .map(|position| position.x)
        }

        /// Whether some tab's leading (left) edge sits exactly on the
        /// viewport's — what a step by a *tab* leaves, and a step by pixels
        /// almost never does.
        fn a_tab_is_flush_with_the_leading_edge(app: &mut App, count: usize) -> bool {
            let Some(viewport) = box_of(app, "fixture:tab-viewport") else {
                return false;
            };
            (0..count).any(|index| {
                box_of(app, &format!("fixture:tab:{index}"))
                    .is_some_and(|tab| (tab.min.x - viewport.min.x).abs() < 1.0)
            })
        }

        /// **A horizontal strip that overflows grows the reference's four
        /// buttons, and they move by tabs.** On steps so the next tab is flush
        /// with the strip's leading edge; last goes to the end; first back to
        /// the start; and back, held, keeps stepping until it is let go.
        #[test]
        fn the_overflow_buttons_walk_the_strip_by_tabs() -> Result<(), TestError> {
            use bevy::time::TimeUpdateStrategy;
            use std::time::Duration;

            const COUNT: usize = 24;
            let mut app = overflowing_strip_app(COUNT);
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            let arrows =
                find_by_name(&mut app, "fixture:tab-arrows").ok_or("no overflow buttons")?;
            assert_eq!(
                app.world().get::<Visibility>(arrows),
                Some(&Visibility::Inherited),
                "{COUNT} tabs overflow the strip, so its buttons show"
            );
            assert_eq!(
                strip_offset(&mut app),
                Some(0.0),
                "it starts at the first tab"
            );

            interact::click_node(&mut app, "fixture:tab-arrow:next")?;
            settle(&mut app);
            let stepped = strip_offset(&mut app).ok_or("the viewport lost its scroll")?;
            assert!(stepped > 0.0, "on moves the strip");
            assert!(
                a_tab_is_flush_with_the_leading_edge(&mut app, COUNT),
                "on stops with a tab flush with the leading edge, not part-way through one"
            );
            interact::click_node(&mut app, "fixture:tab-arrow:next")?;
            settle(&mut app);
            let twice = strip_offset(&mut app).ok_or("the viewport lost its scroll")?;
            assert!(twice > stepped, "and on again moves it further");

            interact::click_node(&mut app, "fixture:tab-arrow:last")?;
            settle(&mut app);
            let end = strip_offset(&mut app).ok_or("the viewport lost its scroll")?;
            let viewport = box_of(&mut app, "fixture:tab-viewport").ok_or("no viewport")?;
            let last =
                box_of(&mut app, &format!("fixture:tab:{}", COUNT - 1)).ok_or("no last tab")?;
            assert!(
                (last.max.x - viewport.max.x).abs() < 1.0,
                "last shows the last tab at the trailing edge: {last:?} in {viewport:?}"
            );

            // Hold back for a second of 100 ms frames: several tabs, not one.
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )));
            let prev = centre_of(&mut app, "fixture:tab-arrow:prev").ok_or("no back button")?;
            interact::hover(&mut app, prev);
            interact::press(&mut app, MouseButton::Left);
            settle(&mut app);
            let after_press = strip_offset(&mut app).ok_or("the viewport lost its scroll")?;
            for _frame in 0..10 {
                app.update();
            }
            let held = strip_offset(&mut app).ok_or("the viewport lost its scroll")?;
            assert!(after_press < end, "the press steps back once");
            assert!(held < after_press, "holding it keeps stepping back");
            interact::release(&mut app, MouseButton::Left);
            for _frame in 0..10 {
                app.update();
            }
            assert_eq!(strip_offset(&mut app), Some(held), "letting go stops it");

            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            interact::click_node(&mut app, "fixture:tab-arrow:first")?;
            settle(&mut app);
            assert_eq!(
                strip_offset(&mut app),
                Some(0.0),
                "first goes back to the start"
            );
            Ok(())
        }

        /// **A vertical strip's scrollbar shows only while there is something
        /// to scroll.** A bar whose thumb fills its whole groove says nothing
        /// but "this could scroll", so two tabs show none and twenty-four,
        /// which overflow the strip, show it.
        #[test]
        fn a_vertical_strips_bar_shows_only_while_its_tabs_overflow() -> Result<(), TestError> {
            for (count, want) in [(2_usize, Visibility::Hidden), (24, Visibility::Inherited)] {
                let mut app = strip_app(TabPlacement::InlineStart, count);
                let bar = find_by_name(&mut app, "fixture:tab-scrollbar")
                    .ok_or("the strip has no bar")?;
                assert_eq!(
                    app.world().get::<Visibility>(bar),
                    Some(&want),
                    "{count} tabs: the bar's visibility"
                );
            }
            Ok(())
        }
    }
}
