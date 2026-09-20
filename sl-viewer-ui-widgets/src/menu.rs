//! The line-based menu widget (`viewer-ui-context-menu`) and the reusable menu
//! bar built on it (`viewer-ui-menu-bar`): the conventional pull-down / pop-up
//! menu — a vertical list of entries, some of which check, disable, separate or
//! open a submenu — and a horizontal strip of buttons that each drop one of
//! those lists down.
//!
//! # The other half of the pie
//!
//! `sl_viewer_ui_pie_menu::pie_menu` is the *radial* presentation of a menu; this
//! is the *line* presentation. The reference viewer makes pie-vs-line a
//! **preference** (`UsePieMenu`), not two feature sets, and the two widgets are
//! two drawings of the same thing: a **tree of entries**, each a label, an
//! action, and the conditions under which it is available and checked. So the
//! entry vocabulary here mirrors the pie's (`pie_menu::PieAction` — a `label`, an
//! `action` string, and a named `when` condition), and both widgets dispatch the
//! same way, by writing a `UiAction` that someone else routes (the registry
//! rule, `ui_element`). What a given domain menu *contains* is
//! per-domain and not here, exactly as it is not in the pie.
//!
//! # Labels are keys, and everything derived from one is derived from the text
//!
//! A [`MenuCommand`] / [`MenuDef`] carries a `label_key`, not a label: a Fluent
//! key the bundle answers, exactly as a notification template's `message_key`
//! is. Three things are derived from a label, and all three are derived from the
//! **resolved** text rather than the key — the drawn line, the keyboard jump key
//! (`assign_jump_keys`), and whether menu search matches it — so a translated
//! menu reads, jumps and searches in the reader's own language. That is why a
//! popup is built with a [`Translator`] in hand (`MenuBuildCtx`) rather than
//! binding each row to its key.
//!
//! The one label that *is* bound, with `i18n::Translated`, is the bar button's:
//! it has no mnemonic to split and no search term to match, so nothing needs it
//! synchronously, and binding keeps [`spawn_menu_bar`] callable from a plain
//! `Commands` — which the element registry's fixed spawn signature requires. A
//! bar label therefore relocalises in place on a language switch, where a popup
//! is rebuilt on its next open. The consequence for the gallery is that a bar
//! label no longer passes through `ElementCx::text`: what varies its length is
//! the bundle now, which is the real version of the thing that transform stood
//! in for.
//!
//! # Self-managed, on `bevy_ui_widgets`' `Popover`
//!
//! The one upstream piece this leans on is [`Popover`] — edge-flipping
//! placement. Everything else is **driven here**, off pointer-**press**
//! observers on the button / entry rows, rather than through
//! `bevy_ui_widgets`' `Button` / `MenuButton` activation. That indirection
//! (`Pointer<Press>` → `Activate` → `MenuEvent`) proved not to fire in this app,
//! whereas a plain press observer on the row is reliable — so a bar button's
//! press toggles its menu (`MenuNav::toggle_host`), an entry's press runs it and
//! closes the stack, and a press that reaches the UI root (i.e. landed on nothing in a
//! menu, because a menu row stops its own press) dismisses everything
//! (`dismiss_menus_on_press`). The highlight is painted by
//! `highlight_menu_hover`, not bevy_flair `:hover`, so it reads identically in
//! the gallery and the viewer.
//!
//! Two consequences worth stating: a child label must be `Pickable::IGNORE`, or
//! it swallows the press and the row never sees it (a child node blocks picking
//! by default); and keyboard traversal of an *open* menu is driven in the same
//! self-managed spirit (`MenuKeyboard` + `menu_keyboard_nav`) — a
//! keyboard-highlighted row index fed from key input, reusing the same
//! `MenuEntryAction` dispatch and submenu open/close the mouse path uses,
//! rather than the upstream focus machinery. The block-axis arrows step the
//! highlight, the inline-axis arrows open / close a submenu (and switch bar
//! menus at the top), `Enter` / `Space` activate, and the reference's
//! underlined **jump keys** (`assign_jump_keys`) jump to an entry once
//! keyboard navigation has begun. The highlight is the same component the hover
//! system paints (`highlight_menu_hover`); keyboard is a second writer of it.
//!
//! # One widget, two containers — why the inventory shares it
//!
//! The reference draws its item rows identically whether they sit in the main
//! menu bar, a right-click context menu, or a gear-button drop-down on a floater.
//! Only the **container** differs, so this exposes the pieces separately:
//!
//! - [`spawn_menu_button`] — one button that drops a [`MenuDef`] beneath itself.
//!   What the inventory window's gear / view buttons want.
//! - [`spawn_menu_bar`] — a horizontal strip of those buttons. The top menu bar.
//! - [`OpenContextMenu`] — pop a [`MenuDef`] at a screen point, with no anchor.
//!
//! All three build the same list (`build_menu_popup`) from the same
//! [`MenuDef`].
//!
//! # Direction-neutral (convention 1)
//!
//! Nothing here says `left` / `right`: a menu drops toward the **block end** and
//! a submenu toward the **inline end**, flipping at the screen edge — mirrored
//! under RTL with no separate code, because [`Popover`]'s candidate placements
//! are built from logical drops folded against the live [`UiDirection`].
//!
//! Reference (Firestorm, read-only): `indra/llui/llmenugl.{h,cpp}`.

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::HoverMap;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide};
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_viewer_ui_core::i18n::{Translated, Translator};
use sl_viewer_ui_core::ui::{
    LogicalMargin, LogicalRect, UiDirection, UiRoot, UiScaffoldSystems, column,
};
use sl_viewer_ui_core::ui_element::{ElementCx, UiAction};
use sl_viewer_ui_core::ui_font::UiFont;

// ---------------------------------------------------------------------------
// The declaration. A menu is a tree of entries, authored as data.
// ---------------------------------------------------------------------------

/// One command a menu can run, at one line.
///
/// The shape mirrors `pie_menu::PieAction` on purpose — a `label`, an
/// `action` string emitted when picked, and named condition keys — so the two
/// presentations of a menu can share a domain's entries rather than drifting.
/// The extra fields are the ones a *line* has room for that a pie slice does
/// not: an accelerator drawn against the entry, and separate enable / check /
/// visible conditions (the reference's `on_enable` / `on_check` / `on_visible`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuCommand {
    /// The Fluent key the entry's text is looked up under, resolved through
    /// [`Translator`] when the line is built and laid out through the ordinary
    /// bidi text stack.
    ///
    /// A **key**, not the text — the same shape
    /// `sl_viewer_notifications::NotificationTemplate::message_key` has, and for
    /// the same reason: a `&'static str` of English is a string a translator
    /// cannot reach. Everything the widget derives from a label — the jump-key
    /// mnemonic, the menu-search match, the drawn line — is derived from the
    /// **resolved** text, so a translated menu navigates and searches in the
    /// reader's own language.
    pub label_key: &'static str,
    /// What this emits when picked — the `action` of the `UiAction` the widget
    /// writes, and the name a test asserts against.
    pub action: &'static str,
    /// The condition under which the entry is **enabled**, or `None` for always.
    /// A failing condition greys the entry and makes it unpickable; it keeps its
    /// line, because the entry belongs to the menu, not to whether it is
    /// available this second.
    pub enabled_when: Option<&'static str>,
    /// The condition under which the entry shows a **check mark**, or `None` for
    /// a plain (uncheckable) entry. A radio group is several entries whose
    /// `checked_when` keys are mutually exclusive.
    pub checked_when: Option<&'static str>,
    /// The condition under which the entry is **shown at all**, or `None` for
    /// always — the reference's `on_visible`, unlike `enabled_when` which greys
    /// the line in place.
    pub visible_when: Option<&'static str>,
    /// The accelerator drawn against the entry (e.g. `"Ctrl+I"`), or `None`.
    ///
    /// Not display-only: [`crate::menu_accel`] parses this very string and
    /// routes the chord to this entry, honouring its `enabled_when` /
    /// `visible_when`. So authoring an accelerator here is the whole of binding
    /// it, and a drawn shortcut cannot disagree with the keyboard.
    pub accelerator: Option<&'static str>,
}

impl MenuCommand {
    /// A plain always-available action: a label key and the action it emits.
    #[must_use]
    pub const fn new(label_key: &'static str, action: &'static str) -> Self {
        Self {
            label_key,
            action,
            enabled_when: None,
            checked_when: None,
            visible_when: None,
            accelerator: None,
        }
    }

    /// The same entry with an accelerator label drawn against it.
    #[must_use]
    pub const fn accel(mut self, accelerator: &'static str) -> Self {
        self.accelerator = Some(accelerator);
        self
    }

    /// The same entry as a check item, checked while `condition` holds.
    #[must_use]
    pub const fn checked_when(mut self, condition: &'static str) -> Self {
        self.checked_when = Some(condition);
        self
    }

    /// The same entry, enabled only while `condition` holds.
    #[must_use]
    pub const fn enabled_when(mut self, condition: &'static str) -> Self {
        self.enabled_when = Some(condition);
        self
    }

    /// The same entry, shown only while `condition` holds.
    #[must_use]
    pub const fn visible_when(mut self, condition: &'static str) -> Self {
        self.visible_when = Some(condition);
        self
    }
}

/// One line in a menu: a command, a submenu, or a rule between groups.
///
/// The variants differ in width on purpose: a `Command` carries its whole
/// declaration inline (label, action, three condition keys, an accelerator)
/// while a `Submenu` is one reference and a `Separator` is empty. The entries
/// live in `static` arrays authored by hand, where by-value commands read far
/// better than a forest of separate `static MenuCommand`s referenced by pointer,
/// and a menu is never large enough for the width to matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItemDef {
    /// A single command. Greyed if its `enabled_when` fails, absent if its
    /// `visible_when` fails.
    Command(MenuCommand),
    /// A named submenu, opened toward the inline end of its line. Recursive: a
    /// submenu is an ordinary [`MenuDef`].
    Submenu(&'static MenuDef),
    /// A [`Submenu`](Self::Submenu) that is present only while a condition
    /// holds — the per-type submenus of a shared context menu (the inventory
    /// item menu's "Attach To" shows only for object rows, the way a
    /// [`MenuCommand`]'s `visible_when` hides a line).
    SubmenuWhen(&'static MenuDef, &'static str),
    /// A submenu whose entries are **not** authored: their labels come from the
    /// named slot of the [`MenuDynamicSlots`] the opener snapshotted, one line
    /// per entry, and a pick reports its *index* in that slot
    /// ([`MenuDynamicPick`]) rather than an action string. The line is absent
    /// while the slot is empty.
    ///
    /// This is the one thing a `&'static` tree cannot spell: a line per avatar
    /// under the cursor, labelled with a name that may only arrive after the
    /// menu is already open ([`SetMenuDynamicLabels`]).
    DynamicSubmenu {
        /// The Fluent key of the submenu line's own (authored) label. Its
        /// *entries* are runtime data and carry no key.
        label_key: &'static str,
        /// The [`MenuDynamicSlots`] slot its entries come from.
        slot: &'static str,
    },
    /// A horizontal rule between two groups of entries.
    Separator,
}

/// A menu: the label it drops from, and its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuDef {
    /// The Fluent key of the button / submenu label this menu drops from.
    pub label_key: &'static str,
    /// The lines, in presentation order (top to bottom *is* the layout).
    pub items: &'static [MenuItemDef],
}

/// A menu bar: an ordered strip of top-level menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuBarDef {
    /// The top-level menus, left-to-right in text order.
    pub menus: &'static [&'static MenuDef],
}

// ---------------------------------------------------------------------------
// Conditions — the same "named key, filled from the world" model as the pie.
// ---------------------------------------------------------------------------

/// The condition key that never holds — the convention for an entry that is
/// present for structure but can never be activated (the top bar's "no entries
/// yet" placeholder). The bar never sets it, so an `enabled_when(NEVER_CONDITION)`
/// entry is always greyed; menu search (`menu_search`) skips it, since a
/// permanently unavailable entry is not a real search target.
pub const NEVER_CONDITION: &str = "never";

/// The conditions that currently hold, by name.
///
/// A component rather than a resource, so two open menus (or a test's fixture
/// and the live bar) do not share one truth. The live viewer fills it from the
/// session (`menu_bar`); the gallery and tests leave it empty, and
/// every conditional entry then reads as unavailable / unchecked — a *true*
/// rendering of "no session", not a stub.
#[derive(Component, Debug, Clone, Default)]
pub struct MenuConditions(pub Vec<&'static str>);

impl MenuConditions {
    /// Whether a named condition holds. A `None` key always holds; a `Some` key
    /// holds iff it is present.
    #[must_use]
    pub fn holds(&self, key: Option<&'static str>) -> bool {
        match key {
            None => true,
            Some(name) => self.0.contains(&name),
        }
    }
}

/// Every command action in a menu tree, depth-first, tagged with the
/// `>`-joined path of menu **label keys** that reaches it — the line-menu
/// analogue of the pie's `pie_menu::addresses`.
///
/// Keys rather than the drawn text, because what this pins is the *walk* and a
/// walk must not move when the reader switches language: `World > Mini-Map` and
/// `Welt > Minikarte` are the same address, and only the keys say so.
///
/// The pie pins *which compass point* an action sits at because a pie is
/// operated by direction; a pull-down is operated by path and order, so what a
/// bar pins is the walk: which menu an action hangs under and where in the list
/// it falls. Same obligation either way — moving an entry must be a deliberate
/// diff against a committed table, not a side effect of tidying, because a user
/// who has learnt *World ▸ Mini-Map* is re-taught for free otherwise.
///
/// A [`MenuItemDef::DynamicSubmenu`]'s lines are data, not declarations — a pick
/// there reports `(slot, index)` and there is no action string to pin — so the
/// walk descends into static submenus only.
#[must_use]
pub fn action_paths(menu: &MenuDef) -> Vec<(String, &'static str)> {
    let mut found = Vec::new();
    collect_action_paths(menu, "", &mut found);
    found
}

/// [`action_paths`]' recursion: walk `menu`, tracking the label-key path taken
/// to reach it.
fn collect_action_paths(menu: &MenuDef, prefix: &str, found: &mut Vec<(String, &'static str)>) {
    let here = if prefix.is_empty() {
        menu.label_key.to_owned()
    } else {
        format!("{prefix} > {}", menu.label_key)
    };
    for item in menu.items {
        match item {
            MenuItemDef::Command(command) => found.push((here.clone(), command.action)),
            MenuItemDef::Submenu(sub) | MenuItemDef::SubmenuWhen(sub, _) => {
                collect_action_paths(sub, &here, found);
            }
            MenuItemDef::DynamicSubmenu { .. } | MenuItemDef::Separator => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic slots — the runtime-labelled entries a static tree cannot spell.
// ---------------------------------------------------------------------------

/// The runtime entries filling each named dynamic slot, by slot name.
///
/// A resource rather than a per-menu component, unlike [`MenuConditions`]: a
/// slot **name** belongs to one domain (`"minimap-profiles"`), so two open menus
/// cannot mean different things by it, and the domain that owns a slot is the
/// only writer — through [`SetMenuDynamicLabels`], the same message that
/// re-labels it when a name arrives late.
///
/// An entry is *only* a label: the identity behind it stays with the domain that
/// filled the slot, and a pick names the entry by `(slot, index)`
/// ([`MenuDynamicPick`]) — the same "the opener keeps the snapshot" model the
/// minimap's right-click already uses for its mark actions. That is what keeps a
/// menu action a `&'static str` while the *lines* are as many as there are
/// avatars under the cursor.
#[derive(Resource, Debug, Clone, Default)]
pub struct MenuDynamicSlots(Vec<(&'static str, Vec<String>)>);

impl MenuDynamicSlots {
    /// The labels filling `slot`, or an empty slice when nothing filled it.
    #[must_use]
    pub fn labels(&self, slot: &'static str) -> &[String] {
        self.0
            .iter()
            .find(|(name, _labels)| *name == slot)
            .map_or(&[], |(_name, labels)| labels.as_slice())
    }

    /// Replace `slot`'s labels (adding the slot if it is new).
    fn set(&mut self, slot: &'static str, labels: Vec<String>) {
        match self.0.iter_mut().find(|(name, _labels)| *name == slot) {
            Some((_name, existing)) => *existing = labels,
            None => self.0.push((slot, labels)),
        }
    }
}

/// A pick of one runtime-filled entry — the dynamic counterpart of `UiAction`.
///
/// It carries no action string because a dynamic entry has none: the domain that
/// filled the slot resolves `index` against the snapshot it kept (which avatar
/// the third line under the cursor was).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuDynamicPick {
    /// The `element` the pick is attributed to, as on `UiAction`.
    pub element: &'static str,
    /// The slot the picked entry came from.
    pub slot: &'static str,
    /// The entry's index in that slot, as the opener filled it.
    pub index: usize,
}

/// Re-label an open menu's dynamic slot **in place** — the asynchronous half.
///
/// A menu popup is built once, at open; a name that arrives a moment later would
/// otherwise leave its line reading "(loading)" for as long as the menu is up.
/// The reference does exactly this (`LLNetMap::setAvatarProfileLabel` writes the
/// item's label when the name cache answers), so a domain that opened a slot
/// with placeholder labels writes this as the real ones land. Entries beyond the
/// labels the menu was built with are left alone: the line count is fixed at
/// open, and only the text is refreshed.
#[derive(Message, Debug, Clone)]
pub struct SetMenuDynamicLabels {
    /// The slot to re-label.
    pub slot: &'static str,
    /// Its labels, in the same order the menu was opened with.
    pub labels: Vec<String>,
}

// ---------------------------------------------------------------------------
// The menu-search filter — the reference's `hightlightAndHide`, applied while a
// popup is built. Set by `menu_search`; read here when a menu opens.
// ---------------------------------------------------------------------------

/// The active menu-search filter.
///
/// While `query` is non-empty, a popup for a menu under `element` is built to
/// show only the entries whose label matches the query (drawn highlighted) — or
/// every entry, under a menu whose own label matched — hiding the rest, the way
/// the reference viewer's `LLStatusBar` filter does (`hightlightAndHide`). An
/// empty `query`, or any menu under a different `element` (the inventory gear, a
/// context menu), builds in full, unfiltered. Set from the search field in
/// `menu_search`; a default (empty) filter changes nothing.
#[derive(Debug, Resource, Default)]
pub struct MenuFilter {
    /// The `element` whose menus this filters — `menu_bar`'s top bar.
    pub element: &'static str,
    /// The lower-cased search term; empty means no active filter.
    pub query: String,
}

impl MenuFilter {
    /// The filter context for building a **top-level** popup of `def` under
    /// `element`, or `None` when no filter applies to it. A top menu whose own
    /// label matches the query shows its whole subtree (`parent_matched`).
    fn context_for(
        &self,
        element: &'static str,
        def: &MenuDef,
        translator: &Translator,
    ) -> Option<MenuFilterCtx<'_>> {
        if self.query.is_empty() || self.element != element {
            return None;
        }
        Some(MenuFilterCtx {
            query: &self.query,
            parent_matched: key_matches_filter(translator, def.label_key, &self.query),
        })
    }

    /// The filter context for a **submenu** popup, whose branch recorded whether
    /// an ancestor (or its own label) already matched (`parent_matched`).
    fn context_for_branch(
        &self,
        element: &'static str,
        parent_matched: bool,
    ) -> Option<MenuFilterCtx<'_>> {
        if self.query.is_empty() || self.element != element {
            return None;
        }
        Some(MenuFilterCtx {
            query: &self.query,
            parent_matched,
        })
    }
}

/// The filter in force while one popup is built: the (non-empty, lower-cased)
/// query, and whether an ancestor menu's label already matched it — in which
/// case this whole level is shown, matching the reference's downward
/// `hide = !bHighlighted` propagation.
#[derive(Clone, Copy)]
struct MenuFilterCtx<'a> {
    /// The lower-cased search term.
    query: &'a str,
    /// Whether an ancestor menu (or this menu's own label) matched, so every
    /// entry at this level is shown regardless of its own match.
    parent_matched: bool,
}

/// Whether `label` contains `query` (a lower-cased, non-empty term),
/// case-insensitively — the reference's substring test.
fn label_matches_filter(label: &str, query: &str) -> bool {
    label.to_lowercase().contains(query)
}

/// [`label_matches_filter`] against the **resolved** text of `key`.
///
/// Search matches what the reader can see. Testing the key would mean a German
/// user searching for `Minikarte` finds nothing while `mini-map` — a string
/// drawn nowhere — finds the entry.
fn key_matches_filter(translator: &Translator, key: &'static str, query: &str) -> bool {
    label_matches_filter(&translator.get(key), query)
}

/// Whether `def`'s subtree carries a match for `query`: one of its commands'
/// labels, or a submenu label or something inside a submenu. A never-enabled
/// placeholder is not counted, so an unpopulated menu does not read as a hit.
fn subtree_matches_filter(def: &MenuDef, query: &str, translator: &Translator) -> bool {
    def.items.iter().any(|item| match item {
        MenuItemDef::Command(command) => {
            command.enabled_when != Some(NEVER_CONDITION)
                && key_matches_filter(translator, command.label_key, query)
        }
        MenuItemDef::Submenu(sub) | MenuItemDef::SubmenuWhen(sub, _) => {
            key_matches_filter(translator, sub.label_key, query)
                || subtree_matches_filter(sub, query, translator)
        }
        // A dynamic submenu's entries do not exist until it is opened, so only
        // its own (authored) label can match a search term.
        MenuItemDef::DynamicSubmenu { label_key, .. } => {
            key_matches_filter(translator, label_key, query)
        }
        MenuItemDef::Separator => false,
    })
}

// ---------------------------------------------------------------------------
// Look and feel. Const paint (so the skinless test / gallery reads right) plus a
// `.sk-menu*` class for a loaded skin's colour / radius; the highlight itself is
// painted by `highlight_menu_hover` so it works with or without a skin.
// ---------------------------------------------------------------------------

/// A menu bar / drop-down surface background. Shared with the status area
/// (`status_bar`) so the two halves of the top row paint the same
/// fallback colour when no skin is loaded.
pub const MENU_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A menu-bar button / menu entry's resting background (transparent).
const ENTRY_BACKGROUND: Color = Color::NONE;

/// A hovered menu button / entry's background — the highlight.
const ENTRY_HIGHLIGHT: Color = Color::srgb(0.24, 0.34, 0.52);

/// The label colour of an entry that **matched** the active menu-search filter —
/// a warm accent, the reference viewer's `hightlightAndHide` highlight. A
/// build-time text colour, not a per-frame background, so it does not fight the
/// hover highlight (`highlight_menu_hover`, which paints backgrounds).
const FILTER_MATCH_COLOR: Color = Color::srgb(0.98, 0.82, 0.40);

/// A drop-down's border.
const MENU_BORDER: Color = Color::srgb(0.30, 0.36, 0.46);

/// An enabled entry's label colour.
const ENTRY_TEXT: Color = Color::srgb(0.92, 0.94, 0.98);

/// A disabled entry's label colour — clearly greyed.
const ENTRY_TEXT_DISABLED: Color = Color::srgb(0.45, 0.49, 0.56);

/// An accelerator / submenu-arrow colour — muted against the label.
const ENTRY_ACCESSORY: Color = Color::srgb(0.62, 0.66, 0.74);

/// A separator rule colour.
const SEPARATOR_COLOR: Color = Color::srgb(0.30, 0.34, 0.42);

/// The inline / block padding around a menu-bar button's label, in logical px.
const BAR_BUTTON_PADDING: Vec2 = Vec2::new(12.0, 6.0);

/// The inline / block padding around a drop-down entry's row, in logical px.
const ENTRY_PADDING: Vec2 = Vec2::new(10.0, 5.0);

/// The width of the leading **check gutter** every entry reserves, in logical
/// px, so labels line up whether or not an entry is checked (the reference's
/// `LEFT_WIDTH`). Fixed because it holds a glyph, not text.
const CHECK_GUTTER_WIDTH: f32 = 16.0;

/// The minimum gap between an entry's label and its trailing accessory, so a
/// long label pushes the accessory out rather than overlapping it.
const ACCESSORY_GAP: f32 = 24.0;

/// A drop-down's inner padding, in logical pixels.
const MENU_PADDING: f32 = 4.0;

/// A drop-down's least width, in logical pixels.
const MENU_MIN_WIDTH: f32 = 140.0;

/// The font size a drop-down entry / bar button sets its text at, in logical px.
const ENTRY_FONT: f32 = 15.0;

/// The check-mark glyph. The reference uses U+2714 HEAVY CHECK MARK; we use the
/// lighter U+2713 CHECK MARK, drawn a couple of points smaller than the label
/// ([`CHECK_FONT`]), which reads as a mark against the entry rather than a
/// competing glyph.
const CHECK_GLYPH: &str = "\u{2713}";

/// The font size the check mark is drawn at, in logical pixels — smaller than
/// the label so the mark sits quietly in its gutter.
const CHECK_FONT: f32 = ENTRY_FONT - 3.0;

/// The gap between the check gutter and the entry's label, in logical pixels —
/// logical, so it stays on the label side of the gutter under RTL.
const GUTTER_LABEL_GAP: f32 = 6.0;

/// The submenu-arrow glyph (U+25B6), the reference's `BRANCH_SUFFIX`. One fixed
/// glyph, not mirrored: it means "there is more, toward the inline end", and the
/// popup it points at is placed there too, so under RTL both move together.
const SUBMENU_ARROW: &str = "\u{25B6}";

/// The z-index a menu popup renders at — above every floater and panel.
const MENU_Z_INDEX: i32 = 10_000;

// ---------------------------------------------------------------------------
// Components tying the widget together.
// ---------------------------------------------------------------------------

/// The host of one menu-bar button (or gear button): the button plus, while
/// open, its drop-down. Owns the def to (re)build and the open popup, if any.
#[derive(Debug, Component)]
pub struct MenuHost {
    /// The menu this host drops down.
    pub(crate) def: &'static MenuDef,
    /// The `element` its actions are attributed to.
    pub(crate) element: &'static str,
    /// The open drop-down popup entity, or `None` while closed.
    pub(crate) open: Option<Entity>,
}

/// A menu-bar (or gear) button, so `highlight_menu_hover` lights it on hover.
#[derive(Component)]
struct MenuBarButton;

/// Marks the one menu-bar row that a lone `Alt` tap opens into keyboard
/// navigation (the reference's tap-`Alt` menu access) — the app's primary top
/// bar. Set by `menu_bar`; a gear-button drop-down or a second bar does
/// not carry it, so `Alt` never targets those.
#[derive(Debug, Component)]
pub struct PrimaryMenuBar;

/// A drop-down command line that emits an action when activated. Read by
/// [`emit_menu_action`].
#[derive(Component, Debug, Clone, Copy)]
struct MenuEntryAction {
    /// The `element` the action is attributed to.
    element: &'static str,
    /// The action string emitted.
    action: &'static str,
}

/// What a popup's lines come from: an authored menu, or one dynamic slot of the
/// [`MenuDynamicSlots`] the menu was opened with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuSource {
    /// An authored menu — every popup that is not a dynamic submenu's.
    Static(&'static MenuDef),
    /// A [`MenuItemDef::DynamicSubmenu`]'s child list: one line per label in the
    /// named slot.
    Dynamic {
        /// The branch line's label key, kept for the popup's `Name`.
        label_key: &'static str,
        /// The slot the lines come from.
        slot: &'static str,
    },
}

impl MenuSource {
    /// The label key this popup drops from — the menu's own, or the dynamic
    /// branch's.
    const fn label_key(self) -> &'static str {
        match self {
            Self::Static(def) => def.label_key,
            Self::Dynamic { label_key, .. } => label_key,
        }
    }
}

/// A submenu line, marking the menu it fronts and holding its open child
/// list, so [`manage_submenus`] can open and close it on hover.
#[derive(Component, Debug, Clone, Copy)]
struct MenuBranch {
    /// The submenu this line opens.
    def: MenuSource,
    /// The `element` its entries' actions are attributed to.
    element: &'static str,
    /// The open child-list popup entity, or `None` while closed.
    open: Option<Entity>,
    /// Whether, when this branch was built under a menu-search filter, an
    /// ancestor (or the submenu's own label) already matched — so the branch's
    /// child popup shows its whole level. Meaningless (and `false`) when no
    /// filter was active; read by [`manage_submenus`] to build the child popup.
    filter_parent_matched: bool,
}

/// A free (anchorless) context menu's cursor anchor — the despawn handle for the
/// whole menu, closed by a pick, an outside press or `Escape`.
#[derive(Component)]
struct FreeContextMenu;

/// A runtime-filled command line: which slot filled it, and where in that slot
/// it sits. Read by [`emit_dynamic_pick`] and by [`apply_dynamic_labels`].
#[derive(Component, Debug, Clone, Copy)]
struct MenuDynamicRow {
    /// The `element` the pick is attributed to.
    element: &'static str,
    /// The slot this line came from.
    slot: &'static str,
    /// Its index in that slot.
    index: usize,
}

/// Marks a dynamic row's label node, so a late-arriving label
/// ([`SetMenuDynamicLabels`]) can be written into the open menu without
/// rebuilding it — the row also carries a check gutter, which is `Text` too.
#[derive(Component)]
struct MenuDynamicLabel;

/// The rows a keyboard pick can step to and commit: an authored command line,
/// or a runtime-filled one. (A submenu branch is navigable too, but it is
/// matched by its own [`MenuBranch`].)
type ActivatableRows<'w, 's> = Query<'w, 's, (), Or<(With<MenuEntryAction>, With<MenuDynamicRow>)>>;

/// The keyboard **jump key** (mnemonic) bound to a command / submenu row — the
/// reference's `LLMenuItemGL::mJumpKey`. Uppercased ASCII, matched against a
/// typed letter once keyboard navigation has begun (`menu_keyboard_nav`).
#[derive(Component, Debug, Clone, Copy)]
struct MenuMnemonic {
    /// The uppercased mnemonic character.
    key: char,
}

/// Marks the one label text span holding a row's mnemonic character, so
/// [`toggle_menu_mnemonic_underline`] can underline it exactly while keyboard
/// navigation is active — the reference's underlined jump key.
#[derive(Component)]
struct MnemonicSpan;

/// The keyboard-navigation state of the open menu stack.
///
/// A single writer of the highlight the hover system already paints
/// (`highlight_menu_hover`). `active` records that keyboard navigation has
/// begun — so the jump-key underlines show, typed letters jump, and the hover
/// systems stand down; `highlighted` is the row the block-axis arrows currently
/// sit on, in the deepest open menu. `pending_first` defers highlighting a
/// just-opened submenu's first row until its (command-spawned) rows exist a
/// frame later — it holds the branch (or host) whose freshly-opened popup should
/// receive the highlight.
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent single-bit flags of one small state machine — \
              whether navigation is active, whether a menu just opened this \
              frame, whether the menu captured focus, and whether an Alt tap is \
              armed; each gates a different edge and folding them into an enum \
              would obscure that they are orthogonal, not mutually exclusive"
)]
#[derive(Resource, Default)]
struct MenuKeyboard {
    /// Whether keyboard navigation has begun.
    active: bool,
    /// The keyboard-highlighted row, or `None` before the first arrow key.
    highlighted: Option<Entity>,
    /// A branch or host whose freshly-opened popup's first row should become the
    /// highlight once its deferred rows have spawned.
    pending_first: Option<Entity>,
    /// Set for the one frame a menu is opened from a `Tab`-focused button, so the
    /// opening key press is not re-read as a command by `menu_keyboard_nav`.
    just_opened: bool,
    /// Whether the menu system itself grabbed keyboard focus to open a menu (a
    /// mouse click on a bar button, a context menu, or a tap-`Alt`), as opposed
    /// to the user's own `Tab`. Only captured focus is handed back to the world
    /// on close ([`menu_focus_release`]); a `Tab`-placed focus is left alone.
    focus_captured: bool,
    /// Whether a lone `Alt` press is in progress and still eligible to open the
    /// menu bar on release — the reference's `mAltKeyTrigger`, cleared by any
    /// other key or by mouse motion (an Alt-drag camera move).
    alt_armed: bool,
}

// ---------------------------------------------------------------------------
// The menu bar and its buttons.
// ---------------------------------------------------------------------------

/// Spawn a horizontal menu bar under `parent`, one drop-down button per
/// top-level menu, and return its row entity.
///
/// The bar sizes to its buttons and wraps rather than clipping (convention 2),
/// so a larger UI font or a longer translation grows and reflows it.
pub fn spawn_menu_bar(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    def: &'static MenuBarDef,
    element: &'static str,
) -> Entity {
    let bar = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(2.0), Val::Px(0.0)),
                column_gap: Val::Px(2.0),
                row_gap: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(MENU_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-bar"]),
            Name::new("menu-bar"),
            ChildOf(parent),
        ))
        .id();
    for menu in def.menus {
        spawn_menu_button(commands, bar, cx, menu, element);
    }
    bar
}

/// Spawn one menu button under `parent` — a labelled button that drops `def`
/// beneath itself when pressed — and return its host entity.
///
/// The reusable unit shared by the top menu bar and the inventory window's gear
/// / view buttons. Open / close is driven by the press observer on the button
/// (`MenuNav::toggle_host`); the `Button` component is kept for keyboard focus,
/// not its activation path.
pub fn spawn_menu_button(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    def: &'static MenuDef,
    element: &'static str,
) -> Entity {
    let host = commands
        .spawn((
            Node::default(),
            MenuHost {
                def,
                element,
                open: None,
            },
            Name::new(format!("menu-host:{}", def.label_key)),
            ChildOf(parent),
        ))
        .id();
    commands
        .spawn((
            Button,
            MenuBarButton,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(BAR_BUTTON_PADDING.x), Val::Px(BAR_BUTTON_PADDING.y)),
                ..default()
            },
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-bar-item"]),
            Name::new(format!("menu-button:{}", def.label_key)),
            ChildOf(host),
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  mut focus: ResMut<InputFocus>,
                  mut keyboard: ResMut<MenuKeyboard>,
                  mut nav: MenuNav| {
                // Consume the press so it does not reach the root dismiss
                // observer (which would close the menu we are about to open).
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                // Give the bar button keyboard focus, so the open menu owns the
                // keyboard (the world's movement keys stand down) and keyboard
                // traversal can pick up where the click left off; this is a
                // menu-captured focus, released back to the world on close.
                focus.set(press.entity, FocusCause::Navigated);
                keyboard.focus_captured = true;
                nav.toggle_host(host);
            },
        )
        .with_child((
            // A **bound** label rather than a resolved one, unlike the rows
            // inside the drop-down. A bar button has no mnemonic to split and no
            // search term to match, so nothing here needs its text
            // synchronously — and binding keeps [`spawn_menu_bar`] /
            // [`spawn_menu_button`] spawnable from a plain `Commands`, which the
            // element registry's fixed spawn signature (`ui_element`) requires.
            // The button then relocalises in place on a language switch, where a
            // popup is simply rebuilt on its next open.
            //
            // It starts empty on purpose: the key is not display text, and
            // `apply_translations` fills it the frame it appears.
            Text::default(),
            Translated::new(def.label_key),
            cx.font(UiFont::Sans),
            TextColor(ENTRY_TEXT),
            // A child node blocks picking by default, so an un-ignored label
            // would swallow the press and the button would never see it.
            Pickable::IGNORE,
        ));
    host
}

// ---------------------------------------------------------------------------
// The menu world, bundled — what every open / close path reaches for.
// ---------------------------------------------------------------------------

/// Everything opening or closing a menu touches, bundled as one [`SystemParam`].
///
/// Every path that drops a popup — a bar button's press, a hover sweep across
/// the bar, the menu-search filter, a submenu hover, and each of the four
/// keyboard paths — needs the *same* world: the ancestry and child order a popup
/// is placed and walked by, the conditions and dynamic slots its lines resolve
/// against, the layout direction and search term it is built for, and the hosts
/// / branches whose `open` it sets. Threading those ten by hand is what made
/// each of those systems a thirteen-parameter signature; as one bundle the
/// signatures say what is actually specific to the system (which keys, which
/// pointer, which request) and the open / close verbs become methods on the
/// bundle rather than free functions taking it apart again.
#[derive(SystemParam)]
struct MenuNav<'w, 's> {
    /// Ancestry: resolving a popup's anchor, and a row's conditions.
    child_of: Query<'w, 's, &'static ChildOf>,
    /// Child order: bar order for a top-menu switch, row order inside a popup.
    children: Query<'w, 's, &'static Children>,
    /// The condition snapshots, read by ancestry ([`conditions_at`]).
    conditions: Query<'w, 's, &'static MenuConditions>,
    /// The runtime entries filling each dynamic slot.
    slots: Res<'w, MenuDynamicSlots>,
    /// The layout direction a popup drops and mirrors against.
    direction: Res<'w, UiDirection>,
    /// The live menu-search term.
    filter: Res<'w, MenuFilter>,
    /// The bundle each line's `label_key` is resolved through.
    translator: Translator<'w>,
    /// The bar (and gear) menus whose drop-down this opens and closes.
    hosts: Query<'w, 's, (Entity, &'static mut MenuHost)>,
    /// The submenu rows whose child popup this opens and closes.
    branches: Query<'w, 's, (Entity, &'static mut MenuBranch)>,
    /// The free (anchorless) context-menu anchors, despawned on dismissal.
    free: Query<'w, 's, Entity, With<FreeContextMenu>>,
    /// What spawns and despawns the popups.
    commands: Commands<'w, 's>,
}

impl MenuNav<'_, '_> {
    /// Toggle `host`'s drop-down: close the whole bar, then (re)open this one
    /// unless it was already the open menu.
    ///
    /// Closing the bar first is what makes clicking straight from one top menu
    /// to the next read as *switching* rather than stacking, and matches the
    /// reference (at most one bar menu is ever down).
    fn toggle_host(&mut self, host: Entity) {
        let was_open = self
            .hosts
            .get(host)
            .is_ok_and(|(_, menu)| menu.open.is_some());
        self.close_all_hosts();
        if !was_open {
            self.open_host(host);
        }
    }

    /// Build and attach `host`'s drop-down.
    fn open_host(&mut self, host: Entity) {
        let Ok((_, menu)) = self.hosts.get(host) else {
            return;
        };
        let (def, element) = (menu.def, menu.element);
        let empty = MenuConditions::default();
        let held = conditions_at(host, &self.child_of, &self.conditions);
        let ctx = MenuBuildCtx {
            element,
            conditions: held.unwrap_or(&empty),
            slots: &self.slots,
            direction: *self.direction,
            filter: self.filter.context_for(element, def, &self.translator),
            translator: &self.translator,
        };
        let popup = build_menu_popup(
            &mut self.commands,
            host,
            MenuSource::Static(def),
            DropDirection::Block,
            ctx,
        );
        if let Ok((_, mut menu)) = self.hosts.get_mut(host) {
            menu.open = Some(popup);
        }
    }

    /// Close every open bar menu.
    fn close_all_hosts(&mut self) {
        close_all_hosts(&mut self.hosts, &mut self.commands);
    }

    /// Close every open menu, bar and free alike.
    fn dismiss_all(&mut self) {
        dismiss_all(&mut self.hosts, &self.free, &mut self.commands);
    }

    /// Build and attach `branch`'s child popup (a no-op if already open) — the
    /// shared submenu-open used by both hover ([`manage_submenus`]) and keyboard
    /// ([`menu_keyboard_nav`]).
    fn open_submenu_popup(&mut self, branch_entity: Entity) {
        let Ok((_, branch)) = self.branches.get(branch_entity) else {
            return;
        };
        if branch.open.is_some() {
            return;
        }
        let (def, element, parent_matched) =
            (branch.def, branch.element, branch.filter_parent_matched);
        let empty = MenuConditions::default();
        let held = conditions_at(branch_entity, &self.child_of, &self.conditions);
        let ctx = MenuBuildCtx {
            element,
            conditions: held.unwrap_or(&empty),
            slots: &self.slots,
            direction: *self.direction,
            filter: self.filter.context_for_branch(element, parent_matched),
            translator: &self.translator,
        };
        let popup = build_menu_popup(
            &mut self.commands,
            branch_entity,
            def,
            DropDirection::Inline,
            ctx,
        );
        if let Ok((_, mut branch)) = self.branches.get_mut(branch_entity) {
            branch.open = Some(popup);
        }
    }

    /// Commit a row the keyboard picked: a submenu opens and the highlight
    /// descends into it (the reference's branch `onCommit`); a command emits its
    /// action and dismisses the whole stack. Shared by `Enter` / `Space`, the
    /// inline-end arrow on a branch, and a jump key.
    fn commit_row(&mut self, row: Entity, keyboard: &mut MenuKeyboard, entries: &ActivatableRows) {
        if self.branches.get(row).is_ok() {
            self.open_submenu_popup(row);
            keyboard.active = true;
            keyboard.highlighted = Some(row);
            keyboard.pending_first = Some(row);
        } else if entries.get(row).is_ok() {
            // Emission and dismissal go through the same points a mouse press
            // uses.
            self.commands.trigger(Activate {
                entity: row,
                button: None,
            });
            self.dismiss_all();
            *keyboard = MenuKeyboard::default();
        }
    }

    /// Switch the open bar menu to the next / previous top menu (inline-axis
    /// arrows at the top level), highlighting the new menu's first entry once it
    /// builds.
    fn switch_bar_menu(&mut self, host: Entity, forward: bool, keyboard: &mut MenuKeyboard) {
        let Ok(bar) = self.child_of.get(host).map(ChildOf::parent) else {
            return;
        };
        let Ok(kids) = self.children.get(bar) else {
            return;
        };
        let siblings: Vec<Entity> = kids
            .iter()
            .filter(|&kid| self.hosts.get(kid).is_ok())
            .collect();
        let last = siblings.len().saturating_sub(1);
        let Some(index) = siblings.iter().position(|&entity| entity == host) else {
            return;
        };
        let target_index = if forward {
            if index >= last {
                0
            } else {
                index.saturating_add(1)
            }
        } else {
            index.checked_sub(1).unwrap_or(last)
        };
        let Some(target) = siblings.get(target_index).copied() else {
            return;
        };
        if target == host {
            return;
        }
        if let Ok((_, mut menu)) = self.hosts.get_mut(host) {
            close_host(&mut menu, &mut self.commands);
        }
        self.open_host(target);
        keyboard.active = true;
        keyboard.highlighted = None;
        keyboard.pending_first = Some(target);
    }
}

/// Once one bar menu is open, hovering a different top-level button opens *that*
/// one — the reference's `LLMenuBarGL::handleHover`, so the bar reads like one
/// strip you sweep across rather than a row you must click each of.
///
/// Gated on a menu already being open: the *first* menu still opens on a click
/// (a bare hover over the bar does nothing), matching the reference.
fn switch_menu_on_hover(
    hover: Res<HoverMap>,
    keyboard: Res<MenuKeyboard>,
    buttons: Query<&ChildOf, With<MenuBarButton>>,
    mut nav: MenuNav,
) {
    // Keyboard navigation owns the open menu while active; sweeping the pointer
    // must not yank it to another top menu.
    if keyboard.active {
        return;
    }
    if !nav.hosts.iter().any(|(_, menu)| menu.open.is_some()) {
        return;
    }
    let mut hovered = HashSet::new();
    for hits in hover.values() {
        for hit in hits.keys() {
            hovered.insert(*hit);
            for ancestor in nav.child_of.iter_ancestors(*hit) {
                hovered.insert(ancestor);
            }
        }
    }
    let Some(host) = hovered
        .iter()
        .find_map(|entity| buttons.get(*entity).ok().map(ChildOf::parent))
    else {
        return;
    };
    // Only switch *to* a closed menu; hovering the already-open one is a no-op
    // (toggling it would close the menu the pointer is on).
    if nav
        .hosts
        .get(host)
        .is_ok_and(|(_, menu)| menu.open.is_none())
    {
        nav.toggle_host(host);
    }
}

/// Close every open bar menu.
fn close_all_hosts(hosts: &mut Query<(Entity, &mut MenuHost)>, commands: &mut Commands) {
    for (_, mut menu) in hosts.iter_mut() {
        close_host(&mut menu, commands);
    }
}

/// Open the first bar menu that carries a match whenever the menu-search filter
/// changes, so typing a term *shows* its result rather than waiting for the user
/// to open a menu by hand.
///
/// "First" is bar order — the child order of the bar row — so the leftmost menu
/// with at least one matching entry opens; the rest stay closed. Each filter
/// change closes every menu under the filtered element and reopens the target
/// against the current term, so refining the term rebuilds the open drop-down;
/// clearing the term closes it. Runs only on a real filter change
/// ([`MenuFilter`]'s change detection), so a menu opened or closed by hand while
/// the term is steady is left alone.
fn open_filtered_menu(mut nav: MenuNav) {
    if !nav.filter.is_changed() {
        return;
    }
    // The bar row holding the filtered element's hosts, walked in child order.
    let bar = nav
        .hosts
        .iter()
        .find(|(_, menu)| menu.element == nav.filter.element)
        .and_then(|(host, _)| nav.child_of.get(host).ok())
        .map(ChildOf::parent);
    // The target: the first host, in bar order, whose subtree carries a match.
    let target = if nav.filter.query.is_empty() {
        None
    } else {
        bar.and_then(|bar| nav.children.get(bar).ok())
            .and_then(|kids| {
                kids.iter().find(|&child| {
                    nav.hosts.get(child).is_ok_and(|(_, menu)| {
                        menu.element == nav.filter.element
                            && subtree_matches_filter(menu.def, &nav.filter.query, &nav.translator)
                    })
                })
            })
    };
    // Close every host under the element, then (re)open the target so its popup
    // reflects the current term. Collected first because reopening one goes
    // back through the whole bundle ([`MenuNav::open_host`]).
    let under_element: Vec<Entity> = nav
        .hosts
        .iter()
        .filter(|(_, menu)| menu.element == nav.filter.element)
        .map(|(host, _)| host)
        .collect();
    for host in under_element {
        if let Ok((_, mut menu)) = nav.hosts.get_mut(host) {
            close_host(&mut menu, &mut nav.commands);
        }
        if Some(host) == target {
            nav.open_host(host);
        }
    }
}

/// Despawn `host`'s drop-down (and any submenus under it), if open.
fn close_host(host_menu: &mut MenuHost, commands: &mut Commands) {
    if let Some(popup) = host_menu.open.take() {
        commands.entity(popup).despawn();
    }
}

/// Which way a popup drops relative to its anchor.
#[derive(Clone, Copy)]
enum DropDirection {
    /// Down, aligned to the anchor's inline start — a top-level or gear menu.
    Block,
    /// Toward the inline end, aligned to the anchor's block start — a submenu.
    Inline,
}

impl DropDirection {
    /// The [`Popover`] candidate placements for this direction, most-preferred
    /// first, each with an edge fallback — built from the logical drop folded
    /// against `direction`, so the whole thing mirrors under RTL.
    fn placements(self, direction: UiDirection) -> Vec<PopoverPlacement> {
        let (inline_start_side, inline_end_side) = match direction {
            UiDirection::Ltr => (PopoverSide::Left, PopoverSide::Right),
            UiDirection::Rtl => (PopoverSide::Right, PopoverSide::Left),
        };
        // For a below-drop, `Start`/`End` are the inline extremes measured
        // left-to-right, so inline-start is `Start` under LTR, `End` under RTL.
        let inline_start_align = match direction {
            UiDirection::Ltr => PopoverAlign::Start,
            UiDirection::Rtl => PopoverAlign::End,
        };
        match self {
            Self::Block => vec![
                PopoverPlacement {
                    side: PopoverSide::Bottom,
                    align: inline_start_align,
                    gap: 0.0,
                },
                PopoverPlacement {
                    side: PopoverSide::Top,
                    align: inline_start_align,
                    gap: 0.0,
                },
            ],
            Self::Inline => vec![
                PopoverPlacement {
                    side: inline_end_side,
                    align: PopoverAlign::Start,
                    gap: 0.0,
                },
                PopoverPlacement {
                    side: inline_start_side,
                    align: PopoverAlign::Start,
                    gap: 0.0,
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Jump keys — the reference's `createJumpKeys`, per menu.
// ---------------------------------------------------------------------------

/// Assign each command / submenu line a keyboard **jump key** — the reference's
/// `LLMenuGL::createJumpKeys`, reduced to "the first free alphanumeric letter of
/// the label". Returned parallel to `labels`: `Some((upper_key, byte_offset))`
/// for a line that got one (the uppercased key and the byte offset of its
/// character in the label, so the mnemonic can be underlined in place), `None`
/// for a separator or a label with no free letter. A key is consumed as it is
/// taken, so one menu never binds one letter to two lines.
///
/// Takes the lines' **resolved** labels (`None` for a separator) rather than the
/// declarations, because a jump key is a promise about the letter the reader can
/// see: in a translated menu the mnemonics must be that language's letters, not
/// the ones English happened to spell. It also leaves this pure — the
/// interesting property, that a menu never binds one letter twice, is checked
/// without standing up a bundle.
fn assign_jump_keys(labels: &[Option<String>]) -> Vec<Option<(char, usize)>> {
    let mut taken: HashSet<char> = HashSet::new();
    let mut out = Vec::with_capacity(labels.len());
    for label in labels {
        let Some(label) = label else {
            out.push(None);
            continue;
        };
        let mut assigned = None;
        for (offset, ch) in label.char_indices() {
            if !ch.is_alphanumeric() {
                continue;
            }
            let key = ch.to_ascii_uppercase();
            // `insert` is true only when the key was not already spoken for.
            if taken.insert(key) {
                assigned = Some((key, offset));
                break;
            }
        }
        out.push(assigned);
    }
    out
}

/// Resolve each line's label for display — `None` for a separator, which has
/// none.
///
/// The one lookup per line the whole build shares: the jump-key pass, the
/// search-filter test and the drawn row all read the same resolved string, so a
/// key reaches the bundle once per built line rather than once per thing derived
/// from it.
fn resolve_item_labels(items: &[MenuItemDef], translator: &Translator) -> Vec<Option<String>> {
    items
        .iter()
        .map(|item| match item {
            MenuItemDef::Command(command) => Some(translator.get(command.label_key)),
            MenuItemDef::Submenu(sub) | MenuItemDef::SubmenuWhen(sub, _) => {
                Some(translator.get(sub.label_key))
            }
            MenuItemDef::DynamicSubmenu { label_key, .. } => Some(translator.get(label_key)),
            MenuItemDef::Separator => None,
        })
        .collect()
}

/// Split `label` at the mnemonic byte `offset` into `(before, mnemonic, after)`,
/// where `mnemonic` is the single character at `offset` — or `None` if `offset`
/// is not a character boundary (a corrupt assignment). Uses `str::get` so the
/// workspace's no-indexing lint is honoured.
fn split_label_at(label: &str, offset: usize) -> Option<(&str, &str, &str)> {
    let before = label.get(..offset)?;
    let rest = label.get(offset..)?;
    let ch = rest.chars().next()?;
    let mnemonic = rest.get(..ch.len_utf8())?;
    let after = rest.get(ch.len_utf8()..)?;
    Some((before, mnemonic, after))
}

// ---------------------------------------------------------------------------
// The drop-down list itself.
// ---------------------------------------------------------------------------

/// What one popup's lines are built against, independent of *which* popup it is.
///
/// One open resolves every line of every level it spawns against the same four
/// facts, so they travel together rather than as four parameters repeated down
/// the builder chain. [`MenuNav`] is the system-side counterpart: it holds the
/// queries these are *read from*, and assembles one of these per open.
#[derive(Clone, Copy)]
struct MenuBuildCtx<'a> {
    /// The `element` a pick from this popup is attributed to.
    element: &'static str,
    /// The condition snapshot every `enabled_when` / `checked_when` /
    /// `visible_when` resolves against.
    conditions: &'a MenuConditions,
    /// The runtime entries filling each dynamic slot.
    slots: &'a MenuDynamicSlots,
    /// The layout direction the popup drops and mirrors against.
    direction: UiDirection,
    /// The menu-search context, `None` when this menu is not the searched one.
    filter: Option<MenuFilterCtx<'a>>,
    /// The bundle every line's `label_key` is resolved through — the whole
    /// reason a translated menu reads, searches and jumps in the reader's
    /// language.
    translator: &'a Translator<'a>,
}

/// Build a drop-down popup for `source` under `anchor`, and return it.
///
/// A column of entry rows positioned against `anchor` by [`Popover`], built
/// fresh on each open so its check / enabled / visible states reflect the
/// conditions that hold *now* — and, for a dynamic slot, the labels it holds
/// now.
fn build_menu_popup(
    commands: &mut Commands,
    anchor: Entity,
    source: MenuSource,
    drop: DropDirection,
    ctx: MenuBuildCtx,
) -> Entity {
    let popup = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(MENU_PADDING)),
                border: UiRect::all(Val::Px(1.0)),
                min_width: Val::Px(MENU_MIN_WIDTH),
                // Align children to the start, not the default stretch. An
                // absolutely-positioned flex column that *stretches* its children
                // on the cross axis is grown on the **main** (block) axis too by
                // taffy — the popup ends up far taller than its rows, leaving dead
                // space below the last entry (starkly visible on a one-line menu
                // like the "(no entries yet)" placeholder). Rows and separators
                // fill the width by an explicit `width: 100%` instead, which does
                // not trip the quirk.
                align_items: AlignItems::Start,
                ..column(Val::Px(0.0))
            },
            Popover {
                positions: drop.placements(ctx.direction),
                window_margin: 4.0,
            },
            BackgroundColor(MENU_BACKGROUND),
            BorderColor::all(MENU_BORDER),
            GlobalZIndex(MENU_Z_INDEX),
            // A drop-down is a floating layer: it must render in full even when it
            // overhangs its anchor's edge. A button-anchored menu (`spawn_menu_button`
            // / `MenuNav::open_host`, and every submenu) is spawned `ChildOf` a button that can
            // live inside a clipping ancestor — the inventory floater's content slot
            // sets `Overflow::clip()` — and a `CalculatedClip` is inherited by all
            // descendants regardless of `position_type: Absolute` or `GlobalZIndex`, so
            // without this the popup is cut off at the floater edge. `OverrideClip`
            // discards any inherited clip (and picking honours it too), so the popup —
            // and its rows, which inherit the now-cleared clip — draw and click in full.
            // The free context-menu path already escapes clipping by parenting at the UI
            // root; this makes every menu popup uniform.
            OverrideClip,
            ClassList::new_with_classes(["sk-menu"]),
            Name::new(format!("menu-popup:{}", source.label_key())),
            ChildOf(anchor),
        ))
        // Consume a press that lands on the popup's own padding / border, so it
        // does not bubble to the root dismiss observer and close the menu.
        .observe(|mut press: On<Pointer<Press>>| press.propagate(false))
        .id();
    match source {
        // Jump keys are assigned per built list, so each row carries its
        // mnemonic.
        MenuSource::Static(def) => {
            let labels = resolve_item_labels(def.items, ctx.translator);
            let jumps = assign_jump_keys(&labels);
            for ((item, label), jump) in def.items.iter().zip(&labels).zip(jumps) {
                spawn_menu_line(commands, popup, *item, label.as_deref(), jump, ctx);
            }
        }
        // A dynamic list is as long as the slot the domain filled, and its
        // labels are data, not authored text: no jump keys (a mnemonic taken
        // from someone's name is noise, and the reference assigns none either).
        MenuSource::Dynamic { slot, .. } => {
            for (index, label) in ctx.slots.labels(slot).iter().enumerate() {
                spawn_dynamic_line(commands, popup, ctx.element, slot, index, label);
            }
        }
    }
    popup
}

/// Spawn one drop-down line — a command, a submenu, or a separator.
///
/// With `filter` set (a menu-search term in force), the reference's
/// `hightlightAndHide` applies: a command is shown only if its label matches (or
/// an ancestor menu already matched), drawn highlighted on its own match; a
/// submenu is shown only if its subtree carries a match; and separators are
/// dropped, since the groups they divide are being filtered anyway.
fn spawn_menu_line(
    commands: &mut Commands,
    popup: Entity,
    item: MenuItemDef,
    label: Option<&str>,
    jump: Option<(char, usize)>,
    ctx: MenuBuildCtx,
) {
    // Every line but a separator was resolved by `resolve_item_labels`; a
    // separator has no label and never reaches a branch that reads one.
    let label = label.unwrap_or_default();
    match item {
        MenuItemDef::Command(command) => {
            if !ctx.conditions.holds(command.visible_when) {
                return;
            }
            match ctx.filter {
                None => {
                    let draw = LineDraw {
                        label,
                        jump,
                        highlight: false,
                    };
                    spawn_command_line(commands, popup, command, draw, ctx);
                }
                Some(filter) => {
                    let own_match = label_matches_filter(label, filter.query);
                    if filter.parent_matched || own_match {
                        let draw = LineDraw {
                            label,
                            jump,
                            highlight: own_match,
                        };
                        spawn_command_line(commands, popup, command, draw, ctx);
                    }
                }
            }
        }
        MenuItemDef::Submenu(sub) => match ctx.filter {
            None => spawn_submenu_line(
                commands,
                popup,
                MenuSource::Static(sub),
                LineDraw {
                    label,
                    jump,
                    highlight: false,
                },
                ctx.element,
                false,
            ),
            Some(filter) => {
                let own_match = label_matches_filter(label, filter.query);
                let child_parent_matched = filter.parent_matched || own_match;
                if child_parent_matched || subtree_matches_filter(sub, filter.query, ctx.translator)
                {
                    spawn_submenu_line(
                        commands,
                        popup,
                        MenuSource::Static(sub),
                        LineDraw {
                            label,
                            jump,
                            highlight: own_match,
                        },
                        ctx.element,
                        child_parent_matched,
                    );
                }
            }
        },
        // A dynamic submenu keeps its line only while something fills its slot:
        // "one line per avatar under the cursor" has nothing to open when there
        // is one avatar, and the reference hides its `View Profiles` branch on
        // exactly that count.
        MenuItemDef::DynamicSubmenu { label_key, slot } => {
            if ctx.slots.labels(slot).is_empty() {
                return;
            }
            let source = MenuSource::Dynamic { label_key, slot };
            match ctx.filter {
                None => {
                    spawn_submenu_line(
                        commands,
                        popup,
                        source,
                        LineDraw {
                            label,
                            jump,
                            highlight: false,
                        },
                        ctx.element,
                        false,
                    );
                }
                Some(filter) => {
                    let own_match = label_matches_filter(label, filter.query);
                    if filter.parent_matched || own_match {
                        spawn_submenu_line(
                            commands,
                            popup,
                            source,
                            LineDraw {
                                label,
                                jump,
                                highlight: own_match,
                            },
                            ctx.element,
                            filter.parent_matched || own_match,
                        );
                    }
                }
            }
        }
        MenuItemDef::SubmenuWhen(sub, when) => {
            if ctx.conditions.holds(Some(when)) {
                spawn_menu_line(
                    commands,
                    popup,
                    MenuItemDef::Submenu(sub),
                    Some(label),
                    jump,
                    ctx,
                );
            }
        }
        MenuItemDef::Separator => {
            if ctx.filter.is_none() {
                spawn_separator_line(commands, popup);
            }
        }
    }
}

/// What one built line draws beyond its own declaration.
///
/// The three facts that are settled *while the popup is built* rather than
/// authored with the entry: what the bundle answered for its `label_key`, which
/// letter the jump-key pass gave it, and whether the menu-search term matched
/// it. They arrive together — the label is resolved first, and the other two are
/// derived from it — so they travel together rather than as three parameters
/// threaded through both line builders.
#[derive(Clone, Copy)]
struct LineDraw<'a> {
    /// The resolved text, as the reader sees it.
    label: &'a str,
    /// The jump key and the byte offset of its character in `label`, or `None`
    /// for a line that got none.
    jump: Option<(char, usize)>,
    /// Whether this line itself matched the active menu-search term, which
    /// draws its label in the accent ([`FILTER_MATCH_COLOR`]). A disabled entry
    /// stays greyed regardless.
    highlight: bool,
}

/// Spawn a command line: [check gutter] [label] [accelerator].
fn spawn_command_line(
    commands: &mut Commands,
    popup: Entity,
    command: MenuCommand,
    draw: LineDraw,
    ctx: MenuBuildCtx,
) {
    let element = ctx.element;
    let enabled = ctx.conditions.holds(command.enabled_when);
    let checked = command.checked_when.is_some() && ctx.conditions.holds(command.checked_when);
    let text_color = if !enabled {
        ENTRY_TEXT_DISABLED
    } else if draw.highlight {
        FILTER_MATCH_COLOR
    } else {
        ENTRY_TEXT
    };
    let action = command.action;
    // A disabled row carries a second class whose skin rule repaints the label
    // in the skin's disabled grey — without it, the skin's `.sk-menu-item`
    // text colour would override the Rust-painted `ENTRY_TEXT_DISABLED` and an
    // unavailable entry would read enabled.
    let classes: &[&str] = if enabled {
        &["sk-menu-item"]
    } else {
        &["sk-menu-item", "sk-menu-item-disabled"]
    };
    let row = commands
        .spawn((
            entry_row_node(),
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(classes.iter().copied()),
            MenuEntryAction { element, action },
            Name::new(format!("menu-item:{}", command.action)),
            ChildOf(popup),
        ))
        .id();
    if !enabled {
        commands.entity(row).insert(InteractionDisabled);
    }
    if checked {
        commands.entity(row).insert(Checked);
    }
    if let Some((key, _)) = draw.jump {
        commands.entity(row).insert(MenuMnemonic { key });
    }
    // Emission is a single point — an `Activate` observer — so a press (mouse)
    // and the harness (`activate`) both dispatch the one way. The press also
    // closes the stack.
    commands.entity(row).observe(emit_menu_action);
    attach_row_press(commands, row);
    spawn_gutter(
        commands,
        row,
        if checked { CHECK_GLYPH } else { "" },
        text_color,
    );
    spawn_entry_label(
        commands,
        row,
        draw.label,
        text_color,
        draw.jump.map(|(_, offset)| offset),
    );
    if let Some(accelerator) = command.accelerator {
        commands.spawn((
            Text::new(accelerator),
            UiFont::Sans.at(ENTRY_FONT),
            TextColor(ENTRY_ACCESSORY),
            Pickable::IGNORE,
            Name::new("menu-item-accel"),
            ChildOf(row),
        ));
    }
}

/// Attach the press half of a command row: run it (through the one `Activate`
/// point) and close the whole stack, unless it is disabled.
///
/// Shared by the authored ([`spawn_command_line`]) and runtime-filled
/// ([`spawn_dynamic_line`]) rows, so a picked line behaves the same either way.
fn attach_row_press(commands: &mut Commands, row: Entity) {
    commands.entity(row).observe(
        move |mut press: On<Pointer<Press>>,
              disabled: Query<Has<InteractionDisabled>>,
              mut hosts: Query<(Entity, &mut MenuHost)>,
              free: Query<Entity, With<FreeContextMenu>>,
              mut commands: Commands| {
            press.propagate(false);
            if press.button != PointerButton::Primary {
                return;
            }
            if disabled.get(row).unwrap_or(false) {
                return;
            }
            commands.trigger(Activate {
                entity: row,
                button: Some(PointerButton::Primary),
            });
            dismiss_all(&mut hosts, &free, &mut commands);
        },
    );
}

/// Spawn one runtime-filled line: [gutter] [label], reporting its `(slot, index)`
/// when picked ([`MenuDynamicPick`]).
///
/// It is a command row in every way the rest of the widget cares about — the
/// hover highlight, the keyboard step and `Enter`, the press-and-dismiss — minus
/// the things an authored entry has and a data line does not: an action string,
/// a check, an accelerator, a jump key.
fn spawn_dynamic_line(
    commands: &mut Commands,
    popup: Entity,
    element: &'static str,
    slot: &'static str,
    index: usize,
    label: &str,
) {
    let row = commands
        .spawn((
            entry_row_node(),
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-item"]),
            MenuDynamicRow {
                element,
                slot,
                index,
            },
            Name::new(format!("menu-item:{slot}#{index}")),
            ChildOf(popup),
        ))
        .observe(emit_dynamic_pick)
        .id();
    attach_row_press(commands, row);
    spawn_gutter(commands, row, "", ENTRY_TEXT);
    let label_entity = spawn_entry_label(commands, row, label, ENTRY_TEXT, None);
    commands.entity(label_entity).insert(MenuDynamicLabel);
}

/// Spawn a submenu line: [gutter] [label] [arrow]. The child list opens lazily
/// on hover ([`manage_submenus`]); its own press is only consumed, so clicking a
/// branch does not dismiss the menu.
fn spawn_submenu_line(
    commands: &mut Commands,
    popup: Entity,
    sub: MenuSource,
    draw: LineDraw,
    element: &'static str,
    filter_parent_matched: bool,
) {
    let label_color = if draw.highlight {
        FILTER_MATCH_COLOR
    } else {
        ENTRY_TEXT
    };
    let row = commands
        .spawn((
            entry_row_node(),
            BackgroundColor(ENTRY_BACKGROUND),
            ClassList::new_with_classes(["sk-menu-item"]),
            MenuBranch {
                def: sub,
                element,
                open: None,
                filter_parent_matched,
            },
            Name::new(format!("menu-submenu:{}", sub.label_key())),
            ChildOf(popup),
        ))
        .observe(|mut press: On<Pointer<Press>>| press.propagate(false))
        .id();
    if let Some((key, _)) = draw.jump {
        commands.entity(row).insert(MenuMnemonic { key });
    }
    spawn_gutter(commands, row, "", label_color);
    spawn_entry_label(
        commands,
        row,
        draw.label,
        label_color,
        draw.jump.map(|(_, off)| off),
    );
    commands.spawn((
        Text::new(SUBMENU_ARROW),
        UiFont::Sans.at(ENTRY_FONT),
        TextColor(ENTRY_ACCESSORY),
        Pickable::IGNORE,
        Name::new("menu-submenu-arrow"),
        ChildOf(row),
    ));
}

/// The shared row node of a command / submenu line.
fn entry_row_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        // Fill the popup width by a percentage, not a cross-axis stretch — the
        // popup aligns its children to the start to avoid a taffy height quirk
        // (see `build_menu_popup`), so every row asks for the full width itself.
        width: Val::Percent(100.0),
        padding: UiRect::axes(Val::Px(ENTRY_PADDING.x), Val::Px(ENTRY_PADDING.y)),
        column_gap: Val::Px(4.0),
        ..default()
    }
}

/// Spawn an entry's leading check gutter, holding `glyph` (empty for none).
///
/// `Pickable::IGNORE`, like every entry child, so the pointer's target is the
/// **row**, not this child.
fn spawn_gutter(commands: &mut Commands, row: Entity, glyph: &str, color: Color) {
    commands.spawn((
        Node {
            width: Val::Px(CHECK_GUTTER_WIDTH),
            flex_shrink: 0.0,
            ..default()
        },
        // A logical gap on the label side of the gutter, so the check sits a
        // little clear of the text (and stays clear of it under RTL).
        LogicalMargin(LogicalRect {
            inline_end: Val::Px(GUTTER_LABEL_GAP),
            ..LogicalRect::ZERO
        }),
        Text::new(glyph),
        UiFont::Sans.at(CHECK_FONT),
        TextColor(color),
        Pickable::IGNORE,
        Name::new("menu-item-check"),
        ChildOf(row),
    ));
}

/// Spawn an entry's growing label, reserving a trailing gap for its accessory.
///
/// With `mnemonic_offset` set (a jump key was assigned), the label is built as
/// three text spans — before / the mnemonic character / after — so
/// [`toggle_menu_mnemonic_underline`] can underline that one character in place
/// while keyboard navigation is active. Without one, it is a single `Text`.
///
/// Returns the label node, so a caller that has to write it again later (a
/// dynamic row, whose name may arrive after the menu is open) can keep hold of
/// it.
fn spawn_entry_label(
    commands: &mut Commands,
    row: Entity,
    label: &str,
    color: Color,
    mnemonic_offset: Option<usize>,
) -> Entity {
    let node = Node {
        flex_grow: 1.0,
        margin: UiRect::right(Val::Px(ACCESSORY_GAP)),
        ..default()
    };
    match mnemonic_offset.and_then(|offset| split_label_at(label, offset)) {
        None => commands
            .spawn((
                node,
                Text::new(label.to_owned()),
                UiFont::Sans.at(ENTRY_FONT),
                TextColor(color),
                Pickable::IGNORE,
                Name::new("menu-item-label"),
                ChildOf(row),
            ))
            .id(),
        Some((before, mnemonic, after)) => {
            let label_entity = commands
                .spawn((
                    node,
                    Text::new(before.to_owned()),
                    UiFont::Sans.at(ENTRY_FONT),
                    TextColor(color),
                    Pickable::IGNORE,
                    Name::new("menu-item-label"),
                    ChildOf(row),
                ))
                .id();
            commands.spawn((
                TextSpan::new(mnemonic.to_owned()),
                UiFont::Sans.at(ENTRY_FONT),
                TextColor(color),
                MnemonicSpan,
                ChildOf(label_entity),
            ));
            commands.spawn((
                TextSpan::new(after.to_owned()),
                UiFont::Sans.at(ENTRY_FONT),
                TextColor(color),
                ChildOf(label_entity),
            ));
            label_entity
        }
    }
}

/// Spawn a separator line — one faint rule, not pickable.
fn spawn_separator_line(commands: &mut Commands, popup: Entity) {
    commands.spawn((
        Node {
            height: Val::Px(1.0),
            // Fill the popup width via a percentage, not a cross-axis stretch:
            // the popup aligns its children to the start to dodge a taffy quirk
            // (see `build_menu_popup`), so a rule that relied on stretch would
            // collapse to zero width. The horizontal inset comes from the popup's
            // own padding rather than a margin that would overflow the 100%.
            width: Val::Percent(100.0),
            margin: UiRect::axes(Val::Px(0.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(SEPARATOR_COLOR),
        ClassList::new_with_classes(["sk-menu-separator"]),
        Pickable::IGNORE,
        Name::new("menu-separator"),
        ChildOf(popup),
    ));
}

/// Observer on a command entry: write its `UiAction` when activated. The whole
/// of an entry's outward wiring — the viewer routes it, the gallery drops it, a
/// test reads it (the registry rule, `ui_element`).
fn emit_menu_action(
    activate: On<Activate>,
    entries: Query<&MenuEntryAction>,
    mut actions: MessageWriter<UiAction>,
) {
    if let Ok(entry) = entries.get(activate.entity) {
        actions.write(UiAction {
            element: entry.element,
            action: entry.action,
        });
    }
}

/// Observer on a runtime-filled entry: report which slot line was picked. The
/// dynamic counterpart of [`emit_menu_action`], and equally the whole of such a
/// row's outward wiring.
fn emit_dynamic_pick(
    activate: On<Activate>,
    rows: Query<&MenuDynamicRow>,
    mut picks: MessageWriter<MenuDynamicPick>,
) {
    if let Ok(row) = rows.get(activate.entity) {
        picks.write(MenuDynamicPick {
            element: row.element,
            slot: row.slot,
            index: row.index,
        });
    }
}

/// Apply [`SetMenuDynamicLabels`]: remember the slot's labels for the next menu
/// that opens, and write them into the lines of one that is already open.
///
/// A line the open menu does not have (the slot grew since) is not added: the
/// list's length is fixed when the popup is built, and a menu that grew a line
/// under the pointer would move what the user is about to click.
fn apply_dynamic_labels(
    mut updates: MessageReader<SetMenuDynamicLabels>,
    mut slots: ResMut<MenuDynamicSlots>,
    rows: Query<(&MenuDynamicRow, &Children)>,
    labels: Query<(), With<MenuDynamicLabel>>,
    mut texts: Query<&mut Text>,
) {
    for update in updates.read() {
        slots.set(update.slot, update.labels.clone());
        for (row, children) in &rows {
            if row.slot != update.slot {
                continue;
            }
            let Some(label) = update.labels.get(row.index) else {
                continue;
            };
            for child in children.iter().filter(|&child| labels.contains(child)) {
                if let Ok(mut text) = texts.get_mut(child)
                    && text.0 != *label
                {
                    label.clone_into(&mut text.0);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Submenus — hover-driven.
// ---------------------------------------------------------------------------

/// Keep each submenu open exactly while its branch is under the pointer.
///
/// "Under the pointer" means the branch **row or anything in its subtree** — and
/// because a branch's open child list is spawned as a *child of the branch row*,
/// the child list is part of that subtree. So the pointer moving from a branch
/// into its submenu keeps the chain open; moving to a sibling drops it.
fn manage_submenus(hover: Res<HoverMap>, keyboard: Res<MenuKeyboard>, mut nav: MenuNav) {
    // While keyboard navigation owns the stack, submenu open / close is driven
    // by the arrow keys (`menu_keyboard_nav`); hover must not fight it (it
    // would close a keyboard-opened submenu the pointer is not over).
    if keyboard.active {
        return;
    }
    let mut hovered = HashSet::new();
    for hits in hover.values() {
        for hit in hits.keys() {
            hovered.insert(*hit);
            for ancestor in nav.child_of.iter_ancestors(*hit) {
                hovered.insert(ancestor);
            }
        }
    }
    // The branches whose state the sweep changes, and the popup each already
    // has. Collected rather than applied in the loop because opening one goes
    // back through the whole bundle ([`MenuNav::open_submenu_popup`]); the list
    // is empty on almost every frame, since a sweep changes nothing until the
    // pointer crosses a branch row.
    let changed: Vec<(Entity, Option<Entity>)> = nav
        .branches
        .iter()
        .filter_map(|(branch_entity, branch)| {
            match (hovered.contains(&branch_entity), branch.open) {
                (true, None) => Some((branch_entity, None)),
                (false, Some(popup)) => Some((branch_entity, Some(popup))),
                (true, Some(_)) | (false, None) => None,
            }
        })
        .collect();
    for (branch_entity, open) in changed {
        match open {
            None => nav.open_submenu_popup(branch_entity),
            Some(popup) => {
                nav.commands.entity(popup).despawn();
                if let Ok((_, mut branch)) = nav.branches.get_mut(branch_entity) {
                    branch.open = None;
                }
            }
        }
    }
}

/// The [`MenuConditions`] on `entity` or the nearest ancestor that carries them.
///
/// The top menu bar puts one [`MenuConditions`] on its bar row and every button
/// under it inherits it by ancestry, while a gear button that wants its own
/// carries them directly (self wins over an ancestor).
pub(crate) fn conditions_at<'q>(
    entity: Entity,
    child_of: &Query<&ChildOf>,
    conditions: &'q Query<&MenuConditions>,
) -> Option<&'q MenuConditions> {
    conditions.get(entity).ok().or_else(|| {
        child_of
            .iter_ancestors(entity)
            .find_map(|ancestor| conditions.get(ancestor).ok())
    })
}

// ---------------------------------------------------------------------------
// The free (anchorless) context menu — a menu at a screen point.
// ---------------------------------------------------------------------------

/// Open a [`MenuDef`] at a screen point, with no anchor button — the shape a
/// right-click context menu uses.
#[derive(Message, Debug, Clone)]
pub struct OpenContextMenu {
    /// The menu to show.
    pub menu: &'static MenuDef,
    /// Where to place its corner, in logical pixels.
    pub at: Vec2,
    /// The `element` its actions are attributed to.
    pub element: &'static str,
    /// The condition names that hold for this open, snapshotted by the opener —
    /// the same open-time model as `pie_menu::OpenPieMenu`. Every
    /// `enabled_when` / `checked_when` / `visible_when` key of the menu resolves
    /// against this set; empty means every conditional entry reads unavailable.
    pub conditions: Vec<&'static str>,
}

/// Spawn a popup for each [`OpenContextMenu`] request, anchored to a zero-size
/// node at the cursor so [`Popover`] positions it against a point. Any previous
/// free menu is cleared first, so a second right-click moves the menu.
fn open_context_menus(
    mut requests: MessageReader<OpenContextMenu>,
    root: Res<UiRoot>,
    mut focus: ResMut<InputFocus>,
    mut keyboard: ResMut<MenuKeyboard>,
    mut nav: MenuNav,
) {
    for request in requests.read() {
        let existing: Vec<Entity> = nav.free.iter().collect();
        for anchor in existing {
            nav.commands.entity(anchor).despawn();
        }
        let anchor = nav
            .commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(request.at.x),
                    top: Val::Px(request.at.y),
                    ..default()
                },
                FreeContextMenu,
                // Carried on the anchor so a submenu opened later resolves the
                // same snapshot by ancestry ([`conditions_at`]).
                MenuConditions(request.conditions.clone()),
                Name::new("context-menu-anchor"),
                ChildOf(root.0),
            ))
            .id();
        // Focus the anchor so the context menu owns the keyboard (the world's
        // movement keys stand down) and keyboard traversal works — a
        // menu-captured focus, released on close (the anchor is also despawned).
        focus.set(anchor, FocusCause::Navigated);
        keyboard.focus_captured = true;
        let held = MenuConditions(request.conditions.clone());
        build_menu_popup(
            &mut nav.commands,
            anchor,
            MenuSource::Static(request.menu),
            DropDirection::Block,
            MenuBuildCtx {
                element: request.element,
                conditions: &held,
                slots: &nav.slots,
                direction: *nav.direction,
                // A context menu is not the searched element, so it is never
                // filtered.
                filter: None,
                translator: &nav.translator,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Dismissal — outside press and Escape.
// ---------------------------------------------------------------------------

/// Attach the outside-press dismiss observer to the UI root, once the root
/// exists.
fn attach_menu_dismiss(root: Res<UiRoot>, mut commands: Commands) {
    commands.entity(root.0).observe(dismiss_menus_on_press);
}

/// Dismiss every open menu when a press reaches the UI root.
///
/// A press that lands on a menu button or entry is consumed there
/// (`propagate(false)`), so any press that bubbles all the way up to the root is
/// outside every menu — the reference's click-away dismissal, with no dependence
/// on the hover map.
fn dismiss_menus_on_press(
    _press: On<Pointer<Press>>,
    mut hosts: Query<(Entity, &mut MenuHost)>,
    free: Query<Entity, With<FreeContextMenu>>,
    mut commands: Commands,
) {
    dismiss_all(&mut hosts, &free, &mut commands);
}

/// Dismiss every open menu on `Escape`.
fn dismiss_menus_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    free: Query<Entity, With<FreeContextMenu>>,
    mut hosts: Query<(Entity, &mut MenuHost)>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::Escape) {
        dismiss_all(&mut hosts, &free, &mut commands);
    }
}

/// Close every open bar menu and despawn every free context menu.
fn dismiss_all(
    hosts: &mut Query<(Entity, &mut MenuHost)>,
    free: &Query<Entity, With<FreeContextMenu>>,
    commands: &mut Commands,
) {
    close_all_hosts(hosts, commands);
    for anchor in free {
        commands.entity(anchor).despawn();
    }
}

// ---------------------------------------------------------------------------
// Highlight.
// ---------------------------------------------------------------------------

/// Highlight the menu row the pointer — or, in keyboard mode, the arrow keys —
/// sit on, and clear the rest.
///
/// The widget's own highlight, because bevy_flair's `:hover` does not read the
/// same in the gallery and the viewer for these rows — so the reference
/// behaviour of the thing under the cursor lighting up is driven here off the
/// hover map. A disabled entry never lights up. The hovered node is usually a
/// child (a label), so each hovered entity is resolved to its owning row.
///
/// While keyboard navigation is active (`MenuKeyboard`) it is the *second*
/// writer of this highlight the module docs describe: the pointer stands down
/// and the lit set becomes the keyboard-highlighted row plus every ancestor
/// submenu row on its open path, so the whole chain from the top menu down reads
/// as lit — the reference's kept-open path.
#[expect(
    clippy::type_complexity,
    reason = "an ordinary Bevy query: the row entity, its background to repaint, \
              and whether it is disabled, filtered to the three row markers — an \
              alias for the tuple would obscure it, not clarify it"
)]
fn highlight_menu_hover(
    hover: Res<HoverMap>,
    keyboard: Res<MenuKeyboard>,
    child_of: Query<&ChildOf>,
    mut rows: Query<
        (Entity, &mut BackgroundColor, Has<InteractionDisabled>),
        Or<(With<MenuEntryAction>, With<MenuBranch>, With<MenuBarButton>)>,
    >,
) {
    let row_entities: HashSet<Entity> = rows.iter().map(|(entity, _, _)| entity).collect();
    let lit: HashSet<Entity> = if keyboard.active {
        let mut set = HashSet::new();
        if let Some(highlight) = keyboard.highlighted {
            if row_entities.contains(&highlight) {
                set.insert(highlight);
            }
            for ancestor in child_of.iter_ancestors(highlight) {
                if row_entities.contains(&ancestor) {
                    set.insert(ancestor);
                }
            }
        }
        set
    } else {
        let mut set = HashSet::new();
        for hits in hover.values() {
            for hit in hits.keys() {
                if row_entities.contains(hit) {
                    set.insert(*hit);
                } else if let Some(row) = child_of
                    .iter_ancestors(*hit)
                    .find(|ancestor| row_entities.contains(ancestor))
                {
                    set.insert(row);
                }
            }
        }
        set
    };
    for (entity, mut background, disabled) in &mut rows {
        let wanted = if lit.contains(&entity) && !disabled {
            ENTRY_HIGHLIGHT
        } else {
            ENTRY_BACKGROUND
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Keyboard traversal of an open menu — the reference's `LLMenuGL::handleKey` /
// `handleJumpKey`, in the widget's self-managed spirit.
// ---------------------------------------------------------------------------

/// The root open popup of the whole stack — the open bar menu's drop-down, or a
/// free context menu's popup — or `None` if nothing is open.
fn root_open_popup(
    hosts: &Query<(Entity, &mut MenuHost)>,
    free: &Query<Entity, With<FreeContextMenu>>,
    children: &Query<&Children>,
) -> Option<Entity> {
    if let Some(popup) = hosts.iter().find_map(|(_, menu)| menu.open) {
        return Some(popup);
    }
    free.iter().find_map(|anchor| {
        children
            .get(anchor)
            .ok()
            .and_then(|kids| kids.iter().next())
    })
}

/// Descend from `popup` through every open submenu to the deepest open popup —
/// the one the arrow keys act on when no row is highlighted yet.
fn deepest_open_popup(
    popup: Entity,
    children: &Query<&Children>,
    branches: &Query<(Entity, &mut MenuBranch)>,
) -> Entity {
    let mut current = popup;
    loop {
        let descend = children.get(current).ok().and_then(|kids| {
            kids.iter()
                .find_map(|kid| branches.get(kid).ok().and_then(|(_, branch)| branch.open))
        });
        match descend {
            Some(child) => current = child,
            None => return current,
        }
    }
}

/// The menu currently receiving keys: the popup holding the highlight, or — before
/// the first arrow key — the deepest open popup.
fn current_nav_popup(
    keyboard: &MenuKeyboard,
    child_of: &Query<&ChildOf>,
    hosts: &Query<(Entity, &mut MenuHost)>,
    free: &Query<Entity, With<FreeContextMenu>>,
    children: &Query<&Children>,
    branches: &Query<(Entity, &mut MenuBranch)>,
) -> Option<Entity> {
    if let Some(highlight) = keyboard.highlighted {
        return child_of.get(highlight).ok().map(ChildOf::parent);
    }
    let root = root_open_popup(hosts, free, children)?;
    Some(deepest_open_popup(root, children, branches))
}

/// The command / submenu rows of `popup`, in layout order, minus disabled ones —
/// the list the arrows step and jump keys search (the reference's
/// `highlightNextItem`/`highlightPrevItem` skip disabled by default).
fn navigable_rows(
    popup: Entity,
    children: &Query<&Children>,
    entries: &ActivatableRows,
    branches: &Query<(Entity, &mut MenuBranch)>,
    disabled: &Query<Has<InteractionDisabled>>,
) -> Vec<Entity> {
    let Ok(kids) = children.get(popup) else {
        return Vec::new();
    };
    kids.iter()
        .filter(|&kid| entries.get(kid).is_ok() || branches.get(kid).is_ok())
        .filter(|&kid| !disabled.get(kid).unwrap_or(false))
        .collect()
}

/// The open child popup of `anchor`, whether it is a bar host or a submenu branch
/// — used to resolve a deferred first-child highlight.
fn open_popup_of(
    anchor: Entity,
    hosts: &Query<(Entity, &mut MenuHost)>,
    branches: &Query<(Entity, &mut MenuBranch)>,
) -> Option<Entity> {
    hosts
        .get(anchor)
        .ok()
        .and_then(|(_, menu)| menu.open)
        .or_else(|| {
            branches
                .get(anchor)
                .ok()
                .and_then(|(_, branch)| branch.open)
        })
}

/// The bar host at the root of `popup`'s open chain, or `None` for a free context
/// menu — the target of a top-level inline-axis bar switch.
fn root_host_of(
    popup: Entity,
    child_of: &Query<&ChildOf>,
    hosts: &Query<(Entity, &mut MenuHost)>,
    branches: &Query<(Entity, &mut MenuBranch)>,
) -> Option<Entity> {
    let mut current = popup;
    loop {
        let anchor = child_of.get(current).ok().map(ChildOf::parent)?;
        if hosts.get(anchor).is_ok() {
            return Some(anchor);
        }
        if branches.get(anchor).is_ok() {
            current = child_of.get(anchor).ok().map(ChildOf::parent)?;
            continue;
        }
        return None;
    }
}

/// The next / previous highlight in `rows`, wrapping, given the current one —
/// starting at the first (forward) or last (backward) when nothing is highlighted.
fn step_highlight(rows: &[Entity], current: Option<Entity>, forward: bool) -> Option<Entity> {
    if rows.is_empty() {
        return None;
    }
    let last = rows.len().saturating_sub(1);
    let next = match current.and_then(|row| rows.iter().position(|&candidate| candidate == row)) {
        None => {
            if forward {
                0
            } else {
                last
            }
        }
        Some(index) => {
            if forward {
                // Wrap past the last row back to the first.
                if index >= last {
                    0
                } else {
                    index.saturating_add(1)
                }
            } else {
                // Wrap before the first row back to the last.
                index.checked_sub(1).unwrap_or(last)
            }
        }
    };
    rows.get(next).copied()
}

/// The block-end / block-start (down / up) list arrows are fixed, but the
/// submenu (inline) arrows follow the writing direction — inline-end is `Right`
/// under LTR, `Left` under RTL.
const fn inline_end_key(direction: UiDirection) -> KeyCode {
    match direction {
        UiDirection::Ltr => KeyCode::ArrowRight,
        UiDirection::Rtl => KeyCode::ArrowLeft,
    }
}

/// The inline-start arrow — `Left` under LTR, `Right` under RTL. See
/// [`inline_end_key`].
const fn inline_start_key(direction: UiDirection) -> KeyCode {
    match direction {
        UiDirection::Ltr => KeyCode::ArrowLeft,
        UiDirection::Rtl => KeyCode::ArrowRight,
    }
}

/// The uppercase letter / digit a jump-key-eligible [`KeyCode`] types, or `None`
/// for any other key — so a typed character can be matched against a row's
/// [`MenuMnemonic`].
const fn keycode_to_letter(key: KeyCode) -> Option<char> {
    let letter = match key {
        KeyCode::KeyA => 'A',
        KeyCode::KeyB => 'B',
        KeyCode::KeyC => 'C',
        KeyCode::KeyD => 'D',
        KeyCode::KeyE => 'E',
        KeyCode::KeyF => 'F',
        KeyCode::KeyG => 'G',
        KeyCode::KeyH => 'H',
        KeyCode::KeyI => 'I',
        KeyCode::KeyJ => 'J',
        KeyCode::KeyK => 'K',
        KeyCode::KeyL => 'L',
        KeyCode::KeyM => 'M',
        KeyCode::KeyN => 'N',
        KeyCode::KeyO => 'O',
        KeyCode::KeyP => 'P',
        KeyCode::KeyQ => 'Q',
        KeyCode::KeyR => 'R',
        KeyCode::KeyS => 'S',
        KeyCode::KeyT => 'T',
        KeyCode::KeyU => 'U',
        KeyCode::KeyV => 'V',
        KeyCode::KeyW => 'W',
        KeyCode::KeyX => 'X',
        KeyCode::KeyY => 'Y',
        KeyCode::KeyZ => 'Z',
        KeyCode::Digit0 => '0',
        KeyCode::Digit1 => '1',
        KeyCode::Digit2 => '2',
        KeyCode::Digit3 => '3',
        KeyCode::Digit4 => '4',
        KeyCode::Digit5 => '5',
        KeyCode::Digit6 => '6',
        KeyCode::Digit7 => '7',
        KeyCode::Digit8 => '8',
        KeyCode::Digit9 => '9',
        _ => return None,
    };
    Some(letter)
}

/// The first jump-key-eligible character pressed this frame, if any.
fn pressed_letter(keys: &ButtonInput<KeyCode>) -> Option<char> {
    keys.get_just_pressed().copied().find_map(keycode_to_letter)
}

/// Leave keyboard navigation the moment the pointer really moves — the reference
/// switches back to mouse mode on any hover, so a keyboard-opened submenu then
/// yields to the hover systems.
fn menu_keyboard_mouse_switch(
    motion: Res<AccumulatedMouseMotion>,
    mut keyboard: ResMut<MenuKeyboard>,
) {
    if keyboard.active && motion.delta != Vec2::ZERO {
        keyboard.active = false;
        keyboard.highlighted = None;
        keyboard.pending_first = None;
    }
}

/// Enter the primary menu bar on a lone `Alt` tap — the reference's tap-`Alt`
/// menu access (`LLMenuBarGL::checkMenuTrigger`).
///
/// `Alt` is *armed* on press and disarmed by any other key or by mouse motion
/// (an Alt-drag camera move), so only a clean tap-and-release with nothing else
/// happening opens the bar. It opens the bar's first menu into keyboard
/// navigation; from there the inline arrows switch top menus, the block arrows
/// step entries, and the jump keys work — the same as opening it with the mouse.
/// (The reference highlights the first *closed* top menu instead; opening its
/// drop-down immediately is the one deliberate simplification.)
fn menu_alt_enter(
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    bars: Query<&Children, With<PrimaryMenuBar>>,
    buttons: Query<(), With<MenuBarButton>>,
    mut keyboard: ResMut<MenuKeyboard>,
    mut focus: ResMut<InputFocus>,
    mut nav: MenuNav,
) {
    let alt_down = keys.just_pressed(KeyCode::AltLeft) || keys.just_pressed(KeyCode::AltRight);
    let alt_up = keys.just_released(KeyCode::AltLeft) || keys.just_released(KeyCode::AltRight);
    if alt_down {
        keyboard.alt_armed = true;
    } else if keys.get_just_pressed().next().is_some() || motion.delta != Vec2::ZERO {
        // Any other key, or a mouse move (an Alt-drag), means this was not a tap.
        keyboard.alt_armed = false;
    }
    if !alt_up {
        return;
    }
    let armed = keyboard.alt_armed;
    keyboard.alt_armed = false;
    if !armed || nav.hosts.iter().any(|(_, menu)| menu.open.is_some()) {
        return;
    }
    // The primary bar's first menu button, and the host it drops.
    let Some(button) = bars
        .iter()
        .flat_map(bevy::ecs::hierarchy::Children::iter)
        .filter_map(|host| nav.children.get(host).ok())
        .flat_map(bevy::ecs::hierarchy::Children::iter)
        .find(|&child| buttons.get(child).is_ok())
    else {
        return;
    };
    let Ok(host) = nav.child_of.get(button).map(ChildOf::parent) else {
        return;
    };
    if nav.hosts.get(host).is_err() {
        return;
    }
    nav.open_host(host);
    keyboard.active = true;
    keyboard.highlighted = None;
    keyboard.pending_first = Some(host);
    keyboard.just_opened = true;
    // Tap-Alt is a menu-captured focus: released back to the world on close.
    focus.set(button, FocusCause::Navigated);
    keyboard.focus_captured = true;
}

/// Open a bar menu from its `Tab`-focused button: with nothing open yet and a
/// menu-bar button holding focus, `Enter` / `Space` / the block-end arrow drop
/// its menu and enter keyboard navigation (its first entry highlights a frame
/// later, once the deferred rows exist — [`MenuKeyboard::pending_first`]).
fn menu_keyboard_open_focused(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    buttons: Query<&ChildOf, With<MenuBarButton>>,
    mut keyboard: ResMut<MenuKeyboard>,
    mut nav: MenuNav,
) {
    if nav.hosts.iter().any(|(_, menu)| menu.open.is_some()) {
        return;
    }
    let opens = keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::Space)
        || keys.just_pressed(KeyCode::ArrowDown);
    if !opens {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Ok(host) = buttons.get(focused).map(ChildOf::parent) else {
        return;
    };
    if nav.hosts.get(host).is_err() {
        return;
    }
    nav.open_host(host);
    keyboard.active = true;
    keyboard.pending_first = Some(host);
    // The same key press must not also activate through `menu_keyboard_nav`
    // this frame (the chained sync point makes the new rows visible to it).
    keyboard.just_opened = true;
}

/// Drive the highlight of an open menu from the keyboard — the heart of the task.
///
/// The block-axis arrows step the highlight (wrapping, skipping disabled); the
/// inline-axis arrows open the highlighted submenu / close the current one (and
/// switch top menus at the bar); `Enter` / `Space` commit the highlight; and,
/// once navigation has begun, a typed letter jumps to its [`MenuMnemonic`]. With
/// nothing open it resets the state.
fn menu_keyboard_nav(
    keys: Res<ButtonInput<KeyCode>>,
    entries: ActivatableRows,
    disabled: Query<Has<InteractionDisabled>>,
    mnemonics: Query<&MenuMnemonic>,
    mut keyboard: ResMut<MenuKeyboard>,
    mut nav: MenuNav,
) {
    // Resolve a submenu's deferred first-child highlight, now its rows may exist.
    if let Some(anchor) = keyboard.pending_first {
        match open_popup_of(anchor, &nav.hosts, &nav.branches) {
            Some(popup) => {
                let rows = navigable_rows(popup, &nav.children, &entries, &nav.branches, &disabled);
                if let Some(first) = rows.first().copied() {
                    keyboard.highlighted = Some(first);
                    keyboard.pending_first = None;
                }
            }
            None => keyboard.pending_first = None,
        }
    }

    // The key that just opened a menu from a focused button (handled by
    // `menu_keyboard_open_focused`) must not be re-processed here as a command.
    if keyboard.just_opened {
        keyboard.just_opened = false;
        return;
    }

    // Nothing open: reset and bail, so the next open starts in mouse mode.
    let open = nav.hosts.iter().any(|(_, menu)| menu.open.is_some()) || !nav.free.is_empty();
    if !open {
        if keyboard.active || keyboard.highlighted.is_some() || keyboard.pending_first.is_some() {
            *keyboard = MenuKeyboard::default();
        }
        return;
    }

    let Some(popup) = current_nav_popup(
        &keyboard,
        &nav.child_of,
        &nav.hosts,
        &nav.free,
        &nav.children,
        &nav.branches,
    ) else {
        return;
    };
    let rows = navigable_rows(popup, &nav.children, &entries, &nav.branches, &disabled);
    let inline_end = inline_end_key(*nav.direction);
    let inline_start = inline_start_key(*nav.direction);

    if keys.just_pressed(KeyCode::ArrowDown) {
        if let Some(next) = step_highlight(&rows, keyboard.highlighted, true) {
            keyboard.active = true;
            keyboard.highlighted = Some(next);
        }
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        if let Some(next) = step_highlight(&rows, keyboard.highlighted, false) {
            keyboard.active = true;
            keyboard.highlighted = Some(next);
        }
    } else if keys.just_pressed(inline_end) {
        // A highlighted submenu opens; otherwise the bar advances a top menu.
        let branch_highlight = keyboard
            .highlighted
            .filter(|&row| nav.branches.get(row).is_ok());
        if let Some(branch) = branch_highlight {
            nav.commit_row(branch, &mut keyboard, &entries);
        } else if let Some(host) = root_host_of(popup, &nav.child_of, &nav.hosts, &nav.branches) {
            nav.switch_bar_menu(host, true, &mut keyboard);
        }
    } else if keys.just_pressed(inline_start) {
        // A submenu closes (back up a level); at the top, the bar steps back.
        if let Some(anchor) = nav.child_of.get(popup).ok().map(ChildOf::parent) {
            if nav.branches.get(anchor).is_ok() {
                if let Ok((_, mut branch)) = nav.branches.get_mut(anchor)
                    && let Some(child_popup) = branch.open.take()
                {
                    nav.commands.entity(child_popup).despawn();
                }
                keyboard.active = true;
                keyboard.highlighted = Some(anchor);
            } else if nav.hosts.get(anchor).is_ok() {
                nav.switch_bar_menu(anchor, false, &mut keyboard);
            }
        }
    } else if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        if let Some(highlight) = keyboard.highlighted {
            nav.commit_row(highlight, &mut keyboard, &entries);
        }
    } else if let Some(letter) = pressed_letter(&keys) {
        // Jump keys act only once keyboard navigation has begun.
        if keyboard.active
            && let Some(row) = rows.iter().copied().find(|&candidate| {
                mnemonics
                    .get(candidate)
                    .is_ok_and(|mnemonic| mnemonic.key == letter)
            })
        {
            keyboard.highlighted = Some(row);
            nav.commit_row(row, &mut keyboard, &entries);
        }
    }
}

/// Hand the keyboard back to the world once every menu the *menu system* grabbed
/// focus for has closed. Only focus the menu captured (a mouse-click open, a
/// context menu, a tap-`Alt`) is released; focus the user placed with `Tab` is
/// left where it is, so `Tab`-then-close does not silently steal keyboard focus.
fn menu_focus_release(
    hosts: Query<&MenuHost>,
    free: Query<(), With<FreeContextMenu>>,
    mut keyboard: ResMut<MenuKeyboard>,
    mut focus: ResMut<InputFocus>,
) {
    if !keyboard.focus_captured {
        return;
    }
    let open = hosts.iter().any(|menu| menu.open.is_some()) || !free.is_empty();
    if open {
        return;
    }
    focus.clear();
    keyboard.focus_captured = false;
}

/// Underline each row's mnemonic character exactly while keyboard navigation is
/// active — the reference draws the jump-key underline only once keyboard mode
/// has begun (`jumpKeysActive() && getKeyboardMode()`).
fn toggle_menu_mnemonic_underline(
    keyboard: Res<MenuKeyboard>,
    spans: Query<(Entity, Has<Underline>), With<MnemonicSpan>>,
    mut commands: Commands,
) {
    for (entity, underlined) in &spans {
        if keyboard.active && !underlined {
            commands.entity(entity).insert(Underline);
        } else if !keyboard.active && underlined {
            commands.entity(entity).remove::<Underline>();
        }
    }
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// The line-menu widget's runtime.
///
/// **Requires the i18n string half.** Every line resolves its `label_key` as it
/// is built, so these systems take a `sl_viewer_ui_core::i18n::Translator` — an
/// app that schedules this plugin without `ViewerI18nPlugin` or
/// `i18n::install_untranslated` panics on its first frame with a
/// `Resource does not exist` validation error, rather than merely drawing
/// nothing.
#[derive(Debug)]
pub struct MenuWidgetPlugin;

impl Plugin for MenuWidgetPlugin {
    fn build(&self, app: &mut App) {
        // `InputFocus` / `AccumulatedMouseMotion` come from `DefaultPlugins` in
        // the viewer; `init_resource` is idempotent, so this only fills them in
        // for the headless test harness (which brings neither).
        app.add_message::<OpenContextMenu>()
            .add_message::<MenuDynamicPick>()
            .add_message::<SetMenuDynamicLabels>()
            .init_resource::<MenuFilter>()
            .init_resource::<MenuKeyboard>()
            .init_resource::<MenuDynamicSlots>()
            .init_resource::<InputFocus>()
            .init_resource::<AccumulatedMouseMotion>()
            .add_systems(
                Startup,
                attach_menu_dismiss.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    // A slot's labels land before a menu that reads them opens,
                    // so a right-click that fills a slot in the same frame gets
                    // the lines it just asked for.
                    apply_dynamic_labels.before(open_context_menus),
                    open_context_menus,
                    open_filtered_menu,
                    dismiss_menus_on_escape,
                    // The chord drawn against an entry runs that entry
                    // (`crate::menu_accel`). Before the keyboard navigation
                    // below, so a `Ctrl` chord is dispatched from the menu it
                    // is drawn on rather than being read as a jump key by an
                    // open drop-down.
                    crate::menu_accel::dispatch_menu_accelerators
                        .before(menu_keyboard_mouse_switch),
                    // Keyboard navigation runs first, then the hover systems
                    // stand down / paint against the state it left.
                    (
                        menu_keyboard_mouse_switch,
                        menu_alt_enter,
                        menu_keyboard_open_focused,
                        menu_keyboard_nav,
                    )
                        .chain(),
                    (
                        switch_menu_on_hover,
                        manage_submenus,
                        menu_focus_release,
                        highlight_menu_hover,
                        toggle_menu_mnemonic_underline,
                    )
                        .after(menu_keyboard_nav),
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// The gallery / test fixture — one bar exercising every entry kind.
// ---------------------------------------------------------------------------

/// A submenu under the fixture's "World" menu, so the fixture exercises nesting.
static FIXTURE_SUBMENU: MenuDef = MenuDef {
    label_key: "menu-fixture-environment",
    items: &[
        MenuItemDef::Command(MenuCommand::new("menu-fixture-sunrise", "env-sunrise")),
        MenuItemDef::Command(MenuCommand::new("menu-fixture-midday", "env-midday")),
        MenuItemDef::Command(MenuCommand::new("menu-fixture-sunset", "env-sunset")),
    ],
};

/// The fixture's "Avatar" menu — a check item, a disabled item, accelerators.
static FIXTURE_AVATAR: MenuDef = MenuDef {
    label_key: "menu-fixture-avatar",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("menu-fixture-inventory", "inventory").accel("Ctrl+I"),
        ),
        MenuItemDef::Command(MenuCommand::new("menu-fixture-appearance", "appearance")),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("menu-fixture-fly", "fly")
                .checked_when("flying")
                .accel("Home"),
        ),
        MenuItemDef::Command(
            MenuCommand::new("menu-fixture-sit-down", "sit").enabled_when("can-sit"),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("menu-fixture-quit", "quit").accel("Ctrl+Q")),
    ],
};

/// The fixture's "World" menu, holding the submenu.
static FIXTURE_WORLD: MenuDef = MenuDef {
    label_key: "menu-fixture-world",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("menu-fixture-mini-map", "mini-map").accel("Ctrl+Shift+M"),
        ),
        MenuItemDef::Submenu(&FIXTURE_SUBMENU),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new(
            "menu-fixture-teleport-home",
            "teleport-home",
        )),
        // Shown only under an "advanced" condition — a demo of `on_visible`,
        // absent in the gallery (no conditions), present in the test that sets it.
        MenuItemDef::Command(
            MenuCommand::new("menu-fixture-region-debug-console", "region-debug")
                .visible_when("advanced"),
        ),
    ],
};

/// The fixture menu bar, referenced by the gallery specimen and the tests.
pub static FIXTURE_MENU_BAR: MenuBarDef = MenuBarDef {
    menus: &[&FIXTURE_AVATAR, &FIXTURE_WORLD],
};

/// The fixture context menu, opened by the gallery's right-click toggle.
pub static FIXTURE_CONTEXT_MENU: MenuDef = FIXTURE_AVATAR;

/// Spawn the gallery's menu-bar specimen — the closed bar, whose menus open when
/// clicked (never a pre-opened menu). Registered in
/// `ui_element::ELEMENTS`.
pub fn spawn_menu_bar_specimen(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    spawn_menu_bar(commands, parent, cx, &FIXTURE_MENU_BAR, "menu-bar-specimen")
}

/// The condition marking a slice whose feature does not exist yet.
///
/// It is **never** pushed into the live condition set, so any entry gated on it
/// always resolves disabled — the "declared in its reference place, but not yet
/// pickable" state. Replacing an entry's `when` with a real condition (or `None`)
/// is how a slice goes live, in a single deliberate edit that leaves its address
/// untouched.
pub const UNIMPLEMENTED: &str = "unimplemented";

/// The `element` the top menu bar attributes its actions to — the tag
/// `handle_top_menu_actions` filters on, so it routes *its* menu's picks and
/// not some other widget's. Menu search (`crate::menu_search`) emits under the
/// same tag, so activating a search hit routes through `handle_top_menu_actions`
/// exactly as opening the menu and clicking the entry would.
pub const TOP_MENU_ELEMENT: &str = "top-menu-bar";

#[cfg(test)]
mod tests {
    use super::{
        CHECK_GLYPH, DropDirection, FIXTURE_AVATAR, FIXTURE_MENU_BAR, FIXTURE_WORLD, MenuBranch,
        MenuCommand, MenuConditions, MenuDef, MenuDynamicPick, MenuDynamicSlots, MenuEntryAction,
        MenuHost, MenuItemDef, MenuKeyboard, MenuMnemonic, MenuSource, MnemonicSpan, SUBMENU_ARROW,
        SetMenuDynamicLabels, Translator, action_paths, assign_jump_keys, build_menu_popup,
        spawn_menu_bar_specimen,
    };
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::picking::hover::HoverMap;
    use bevy::prelude::*;
    use bevy::ui_widgets::Activate;
    use pretty_assertions::{assert_eq, assert_ne};

    use crate::ui_test::{
        LayoutTest, TestError, activate, drain_actions, enable_action_recording, find_by_name,
        settle,
    };
    use sl_viewer_ui_core::i18n::{LocaleChoice, UiLocale};
    use sl_viewer_ui_core::ui::{UiDirection, UiRoot, UiScaffoldSystems};
    use sl_viewer_ui_core::ui_element::{ElementCx, UiAction};

    /// The fixture bar's entire action table, pinned against a hand-written list.
    ///
    /// Spelt in **label keys**, which is what [`action_paths`] walks: an
    /// address must not move when the reader switches language, and
    /// `World > Mini-Map` and `Welt > Minikarte` are the same address only if
    /// the keys say so.
    #[test]
    fn the_fixture_action_table_is_pinned() {
        let mut table = Vec::new();
        for menu in FIXTURE_MENU_BAR.menus {
            table.extend(action_paths(menu));
        }
        let expected = vec![
            ("menu-fixture-avatar".to_owned(), "inventory"),
            ("menu-fixture-avatar".to_owned(), "appearance"),
            ("menu-fixture-avatar".to_owned(), "fly"),
            ("menu-fixture-avatar".to_owned(), "sit"),
            ("menu-fixture-avatar".to_owned(), "quit"),
            ("menu-fixture-world".to_owned(), "mini-map"),
            (
                "menu-fixture-world > menu-fixture-environment".to_owned(),
                "env-sunrise",
            ),
            (
                "menu-fixture-world > menu-fixture-environment".to_owned(),
                "env-midday",
            ),
            (
                "menu-fixture-world > menu-fixture-environment".to_owned(),
                "env-sunset",
            ),
            ("menu-fixture-world".to_owned(), "teleport-home"),
            ("menu-fixture-world".to_owned(), "region-debug"),
        ];
        assert_eq!(table, expected);
    }

    /// No two commands in one menu share an action string.
    #[test]
    fn no_menu_repeats_an_action() {
        for menu in FIXTURE_MENU_BAR.menus {
            let actions: Vec<&str> = action_paths(menu)
                .into_iter()
                .map(|(_, action)| action)
                .collect();
            let mut unique = actions.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(actions.len(), unique.len(), "a menu repeats an action");
        }
    }

    /// **A line's text is the bundle's answer, not its `label_key`.**
    ///
    /// Invisible in the resting harness, where every key resolves to itself and
    /// the two are the same string — so this asks in the one locale that needs
    /// no bundle and still changes the answer. The pseudolocale accents and
    /// fences whatever the translator resolved, so a row built off the
    /// declaration would still read `menu-fixture-quit` and a row built through
    /// the translator cannot.
    ///
    /// That the *mnemonic* is one of the accented letters is the same claim from
    /// the other side: the jump key is taken from the text the reader sees.
    #[test]
    fn a_line_draws_the_bundles_answer_not_its_key() -> Result<(), TestError> {
        let mut app = localised_popup_app(
            &FIXTURE_AVATAR,
            &[],
            MenuDynamicSlots::default(),
            LocaleChoice::Pseudo,
        )?;
        let quit = action_entity(&mut app, "quit").ok_or("the Quit entry is missing")?;
        let drawn = row_label_text(&app, quit).ok_or("the Quit row drew no label")?;
        assert_ne!(
            drawn, "menu-fixture-quit",
            "the line drew its key rather than the bundle's answer"
        );
        assert!(
            drawn.starts_with('\u{27e6}') && drawn.ends_with('\u{27e7}'),
            "the drawn text did not come through the translator: {drawn}"
        );
        let mnemonic = app
            .world()
            .get::<MenuMnemonic>(quit)
            .ok_or("Quit got no jump key")?
            .key;
        assert!(
            drawn.contains(mnemonic),
            "the jump key `{mnemonic}` is not a letter of the drawn label {drawn}"
        );
        Ok(())
    }

    /// The whole text a row's label draws, spans included.
    ///
    /// A label with a mnemonic is three spans — before, the mnemonic character,
    /// after — and only their concatenation is what a reader sees.
    fn row_label_text(app: &App, row: Entity) -> Option<String> {
        let children: Vec<Entity> = app.world().get::<Children>(row)?.iter().collect();
        let label = children.into_iter().find(|child| {
            app.world()
                .get::<Name>(*child)
                .is_some_and(|name| name.as_str() == "menu-item-label")
        })?;
        let mut drawn = app.world().get::<Text>(label)?.0.clone();
        let spans: Vec<Entity> = app
            .world()
            .get::<Children>(label)
            .map(|children| children.iter().collect())
            .unwrap_or_default();
        for span in spans {
            if let Some(text) = app.world().get::<TextSpan>(span) {
                drawn.push_str(&text.0);
            }
        }
        Some(drawn)
    }

    /// `MenuConditions::holds` — `None` always holds, a named key holds iff set.
    #[test]
    fn conditions_gate_named_keys() {
        let held = MenuConditions(vec!["flying"]);
        assert!(held.holds(None));
        assert!(held.holds(Some("flying")));
        assert!(!held.holds(Some("can-sit")));
    }

    /// Spawn a drop-down for `menu` under a fresh root, with `conditions` held,
    /// and settle its layout.
    fn popup_app(menu: &'static MenuDef, conditions: &[&'static str]) -> Result<App, TestError> {
        slotted_popup_app(menu, conditions, MenuDynamicSlots::default())
    }

    /// [`popup_app`], with the dynamic slots a runtime-filled submenu reads.
    fn slotted_popup_app(
        menu: &'static MenuDef,
        conditions: &[&'static str],
        slots: MenuDynamicSlots,
    ) -> Result<App, TestError> {
        localised_popup_app(menu, conditions, slots, LocaleChoice::English)
    }

    /// [`slotted_popup_app`], in a chosen locale.
    ///
    /// Only two of the five are reachable without a bundle folder, and both are
    /// worth having: `English` is the resting harness, where every key resolves
    /// to itself, and `Pseudo` is the one configuration in which *resolved* text
    /// and the key are visibly different strings — which is how a test can tell
    /// a line drawn through the bundle from one drawn off the declaration.
    fn localised_popup_app(
        menu: &'static MenuDef,
        conditions: &[&'static str],
        slots: MenuDynamicSlots,
        locale: LocaleChoice,
    ) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app.insert_resource(UiLocale::new(locale));
        enable_action_recording(&mut app);
        let held = MenuConditions(conditions.to_vec());
        app.add_systems(
            Startup,
            (move |mut commands: Commands, translator: Translator, root: Res<UiRoot>| {
                let anchor = commands.spawn((Node::default(), ChildOf(root.0))).id();
                build_menu_popup(
                    &mut commands,
                    anchor,
                    MenuSource::Static(menu),
                    DropDirection::Block,
                    super::MenuBuildCtx {
                        element: "test",
                        conditions: &held,
                        slots: &slots,
                        direction: UiDirection::Ltr,
                        filter: None,
                        translator: &translator,
                    },
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// Spawn a drop-down for `menu` under a filter `query`, and settle its layout.
    /// The filter's `parent_matched` is seeded from whether `menu`'s own label
    /// matches, exactly as [`MenuNav::open_host`](super::MenuNav::open_host)
    /// does for a top menu.
    fn filtered_popup_app(menu: &'static MenuDef, query: &str) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        enable_action_recording(&mut app);
        let query = query.to_lowercase();
        app.add_systems(
            Startup,
            (move |mut commands: Commands, translator: Translator, root: Res<UiRoot>| {
                let anchor = commands.spawn((Node::default(), ChildOf(root.0))).id();
                let ctx = super::MenuFilterCtx {
                    query: &query,
                    parent_matched: super::label_matches_filter(menu.label_key, &query),
                };
                build_menu_popup(
                    &mut commands,
                    anchor,
                    MenuSource::Static(menu),
                    DropDirection::Block,
                    super::MenuBuildCtx {
                        element: "test",
                        conditions: &MenuConditions::default(),
                        slots: &MenuDynamicSlots::default(),
                        direction: UiDirection::Ltr,
                        filter: Some(ctx),
                        translator: &translator,
                    },
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// The specimen spawns **closed** — a host per top menu, no popup — so the
    /// gallery never shows a pre-opened menu.
    #[test]
    fn the_specimen_spawns_closed() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                spawn_menu_bar_specimen(&mut commands, root.0, ElementCx::new());
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        let hosts = app
            .world_mut()
            .query::<&MenuHost>()
            .iter(app.world())
            .count();
        assert_eq!(hosts, 2, "one host per top-level fixture menu");
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_none(),
            "no menu is open on a freshly spawned bar"
        );
        Ok(())
    }

    /// A button-anchored drop-down escapes an enclosing `Overflow::clip()`
    /// ancestor: the popup gets **no** `CalculatedClip`, so it renders (and picks)
    /// in full even when it overhangs the clipping window — the inventory gear-menu
    /// bug. A control sibling *inside* the same clip does get a clip, proving the
    /// scene really clips and the assertion is not vacuous.
    #[test]
    fn a_menu_popup_escapes_a_clipping_ancestor() -> Result<(), TestError> {
        let mut app = LayoutTest::new().with_viewport(400, 300).build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app.add_systems(
            Startup,
            (|mut commands: Commands, translator: Translator, root: Res<UiRoot>| {
                // A small window that clips its overflow, like the inventory
                // floater's content slot, placed at the top-left corner.
                let window = commands
                    .spawn((
                        Node {
                            width: Val::Px(120.0),
                            height: Val::Px(80.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        Name::new("clip-window"),
                        ChildOf(root.0),
                    ))
                    .id();
                // A plain child inside the clip — the control that *should* inherit
                // the window's clip.
                commands.spawn((
                    Node {
                        width: Val::Px(200.0),
                        height: Val::Px(200.0),
                        ..default()
                    },
                    Name::new("clipped-control"),
                    ChildOf(window),
                ));
                // The gear button lives inside the clipping window; its drop-down is
                // spawned `ChildOf` the button, so without `OverrideClip` it would
                // inherit the window's clip.
                let button = commands
                    .spawn((
                        Node {
                            width: Val::Px(20.0),
                            height: Val::Px(20.0),
                            ..default()
                        },
                        Name::new("gear-button"),
                        ChildOf(window),
                    ))
                    .id();
                build_menu_popup(
                    &mut commands,
                    button,
                    MenuSource::Static(&FIXTURE_AVATAR),
                    DropDirection::Block,
                    super::MenuBuildCtx {
                        element: "test",
                        conditions: &MenuConditions::default(),
                        slots: &MenuDynamicSlots::default(),
                        direction: UiDirection::Ltr,
                        filter: None,
                        translator: &translator,
                    },
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);

        let control = find_by_name(&mut app, "clipped-control")
            .ok_or_else(|| TestError::from("the clipped control was not spawned"))?;
        assert!(
            app.world().get::<CalculatedClip>(control).is_some(),
            "the control child of the clip window must inherit its clip — otherwise the scene does \
             not clip and the popup assertion below proves nothing"
        );

        let popup = find_by_name(&mut app, "menu-popup:menu-fixture-avatar")
            .ok_or_else(|| TestError::from("the menu popup was not spawned"))?;
        assert!(
            app.world().get::<CalculatedClip>(popup).is_none(),
            "a button-anchored menu popup must escape the enclosing floater clip (OverrideClip), so \
             the drop-down is never cut off at the window edge"
        );
        Ok(())
    }

    /// An opened menu lays out every entry kind: the visible commands, the check
    /// on a held item, the greying of a failed enable, the accelerators, and the
    /// separators.
    #[test]
    fn an_opened_menu_lays_out_its_entries() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_AVATAR, &["flying"])?;

        let commands = app
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(app.world())
            .count();
        assert_eq!(
            commands, 5,
            "five commands: two separators are not commands"
        );

        let checks = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .filter(|text| text.0 == CHECK_GLYPH)
            .count();
        assert_eq!(checks, 1, "only the held check item shows a check mark");

        let sit = find_by_name(&mut app, "menu-item:sit").ok_or("the Sit entry did not spawn")?;
        assert!(
            app.world()
                .get::<bevy::ui::InteractionDisabled>(sit)
                .is_some(),
            "an entry whose enable condition fails is disabled"
        );

        let accelerators: Vec<String> = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        for accelerator in ["Ctrl+I", "Home", "Ctrl+Q"] {
            assert!(
                accelerators.iter().any(|text| text == accelerator),
                "the {accelerator} accelerator is drawn against its entry"
            );
        }

        let separators = count_named(&mut app, "menu-separator");
        assert_eq!(separators, 2, "both separators are drawn");
        Ok(())
    }

    /// `visible_when` removes a line when its condition fails and restores it when
    /// it holds — unlike a failed `enabled_when`, which greys the line in place.
    #[test]
    fn visible_when_adds_and_removes_a_line() -> Result<(), TestError> {
        let mut hidden = popup_app(&FIXTURE_WORLD, &[])?;
        assert!(
            action_entity(&mut hidden, "region-debug").is_none(),
            "the advanced entry is absent without its condition"
        );
        let mut shown = popup_app(&FIXTURE_WORLD, &["advanced"])?;
        assert!(
            action_entity(&mut shown, "region-debug").is_some(),
            "the advanced entry appears when its condition holds"
        );
        Ok(())
    }

    /// A submenu row fronts its child menu with a branch arrow, and names it.
    #[test]
    fn a_submenu_row_fronts_its_child() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_WORLD, &[])?;
        let branches: Vec<&'static str> = app
            .world_mut()
            .query::<&MenuBranch>()
            .iter(app.world())
            .map(|branch| branch.def.label_key())
            .collect();
        assert_eq!(
            branches,
            vec!["menu-fixture-environment"],
            "one submenu, named"
        );
        let arrows = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .filter(|text| text.0 == SUBMENU_ARROW)
            .count();
        assert_eq!(arrows, 1, "the submenu row draws one branch arrow");
        Ok(())
    }

    /// Activating an entry writes its `UiAction` and nothing else — the whole of
    /// its outward wiring, and the one point a mouse press and a test share.
    #[test]
    fn activating_an_entry_emits_its_action() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_AVATAR, &[])?;
        let quit =
            find_by_name(&mut app, "menu-item:quit").ok_or("the Quit entry did not spawn")?;
        activate(&mut app, quit);
        let actions = drain_actions(&mut app);
        assert_eq!(
            actions,
            vec![UiAction {
                element: "test",
                action: "quit",
            }],
        );
        Ok(())
    }

    /// The slot the dynamic-entry tests fill.
    const TEST_SLOT: &str = "test-people";

    /// A menu with one runtime-filled submenu beside an authored command — the
    /// minimap's shape (View Profile / View Profiles ▸ …).
    static FIXTURE_DYNAMIC: MenuDef = MenuDef {
        label_key: "menu-fixture-people",
        items: &[
            MenuItemDef::Command(MenuCommand::new("menu-fixture-view-profile", "profile")),
            MenuItemDef::DynamicSubmenu {
                label_key: "menu-fixture-view-profiles",
                slot: TEST_SLOT,
            },
        ],
    };

    /// The slots resource holding `labels` under [`TEST_SLOT`].
    fn test_slots(labels: &[&str]) -> MenuDynamicSlots {
        let mut slots = MenuDynamicSlots::default();
        slots.set(
            TEST_SLOT,
            labels.iter().copied().map(ToOwned::to_owned).collect(),
        );
        slots
    }

    /// A dynamic submenu keeps its line only while its slot holds something:
    /// there is nothing to open when nobody filled it.
    #[test]
    fn an_empty_dynamic_slot_drops_its_line() -> Result<(), TestError> {
        let mut empty = popup_app(&FIXTURE_DYNAMIC, &[])?;
        assert_eq!(
            count_named(&mut empty, "menu-submenu:menu-fixture-view-profiles"),
            0,
            "an unfilled slot shows no submenu line"
        );
        let mut filled = slotted_popup_app(&FIXTURE_DYNAMIC, &[], test_slots(&["Ann", "Bo"]))?;
        assert_eq!(
            count_named(&mut filled, "menu-submenu:menu-fixture-view-profiles"),
            1,
            "a filled slot fronts its list"
        );
        Ok(())
    }

    /// The dynamic list is one line per label, in slot order, and a pick reports
    /// the index the opener filled — the whole of a dynamic row's wiring.
    #[test]
    fn a_dynamic_list_reports_the_picked_index() -> Result<(), TestError> {
        let mut app = dynamic_popup_app(&["Ann", "Bo", "Cy"])?;
        assert_eq!(
            dynamic_labels(&mut app),
            vec!["Ann", "Bo", "Cy"],
            "one line per slot entry"
        );

        let second = find_by_name(&mut app, &format!("menu-item:{TEST_SLOT}#1"))
            .ok_or("the second dynamic row did not spawn")?;
        // Trigger without settling: an observer runs at once, and a settle would
        // age the message out of its double buffer before it can be read.
        app.world_mut().trigger(Activate {
            entity: second,
            button: None,
        });
        let messages = app.world().resource::<Messages<MenuDynamicPick>>();
        let mut cursor = messages.get_cursor();
        let picks: Vec<MenuDynamicPick> = cursor.read(messages).copied().collect();
        assert_eq!(
            picks,
            vec![MenuDynamicPick {
                element: "test",
                slot: TEST_SLOT,
                index: 1,
            }],
        );
        // A dynamic row carries no action string, so nothing rides the ordinary
        // action channel.
        assert!(drain_actions(&mut app).is_empty(), "no UiAction is written");
        Ok(())
    }

    /// A label that arrives after the menu opened is written into the open line —
    /// the reference's `setAvatarProfileLabel`, which is why the placeholder is
    /// not a lie the user has to close the menu to escape.
    #[test]
    fn a_late_label_rewrites_the_open_line() -> Result<(), TestError> {
        let mut app = dynamic_popup_app(&["(loading)", "Bo"])?;
        app.world_mut().write_message(SetMenuDynamicLabels {
            slot: TEST_SLOT,
            labels: vec!["Ann".to_owned(), "Bo".to_owned()],
        });
        settle(&mut app);
        assert_eq!(
            dynamic_labels(&mut app),
            vec!["Ann", "Bo"],
            "the open line took the new name"
        );
        assert_eq!(
            app.world().resource::<MenuDynamicSlots>().labels(TEST_SLOT),
            ["Ann".to_owned(), "Bo".to_owned()],
            "and the slot remembers it for the next open"
        );
        Ok(())
    }

    /// The open dynamic list's drawn labels, in slot order (the ECS iterates
    /// rows in no particular order, so they are put back in the order the lines
    /// carry).
    fn dynamic_labels(app: &mut App) -> Vec<String> {
        let mut rows: Vec<(usize, String)> = app
            .world_mut()
            .query::<(&super::MenuDynamicRow, &Children)>()
            .iter(app.world())
            .filter_map(|(row, children)| {
                let label = children.iter().find_map(|child| {
                    app.world()
                        .get::<super::MenuDynamicLabel>(child)
                        .and_then(|_marker| app.world().get::<Text>(child))
                })?;
                Some((row.index, label.0.clone()))
            })
            .collect();
        rows.sort_by_key(|(index, _label)| *index);
        rows.into_iter().map(|(_index, label)| label).collect()
    }

    /// Spawn a **dynamic** list popup (the child a `DynamicSubmenu` opens) whose
    /// slot holds `labels`, with the widget plugin running so the late-label
    /// message is applied.
    fn dynamic_popup_app(labels: &[&str]) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        enable_action_recording(&mut app);
        // The pieces of `MenuWidgetPlugin` a bare popup needs — the whole plugin
        // would also bring its hover / keyboard systems, whose resources this
        // headless fixture has no window to fill.
        app.add_message::<MenuDynamicPick>()
            .add_message::<SetMenuDynamicLabels>()
            .add_systems(Update, super::apply_dynamic_labels);
        app.insert_resource(test_slots(labels));
        app.add_systems(
            Startup,
            (move |mut commands: Commands,
                   translator: Translator,
                   root: Res<UiRoot>,
                   slots: Res<MenuDynamicSlots>| {
                let anchor = commands.spawn((Node::default(), ChildOf(root.0))).id();
                build_menu_popup(
                    &mut commands,
                    anchor,
                    MenuSource::Dynamic {
                        label_key: "menu-fixture-view-profiles",
                        slot: TEST_SLOT,
                    },
                    DropDirection::Inline,
                    super::MenuBuildCtx {
                        element: "test",
                        conditions: &MenuConditions::default(),
                        slots: &slots,
                        direction: UiDirection::Ltr,
                        filter: None,
                        translator: &translator,
                    },
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// `subtree_matches_filter` sees into submenus and past the never-enabled
    /// placeholder.
    ///
    /// Driven through a one-system app because the walk matches against each
    /// line's **resolved** text: with no bundles behind the translator that is
    /// the key itself, so the terms below are matched against the keys the
    /// fixture declares.
    #[test]
    fn subtree_match_sees_into_submenus() {
        let mut app = App::new();
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        app.add_systems(Update, |translator: Translator| {
            // "sunset" is only inside World's Environment submenu.
            assert!(super::subtree_matches_filter(
                &FIXTURE_WORLD,
                "sunset",
                &translator
            ));
            // "teleport" is a top-level World command.
            assert!(super::subtree_matches_filter(
                &FIXTURE_WORLD,
                "teleport",
                &translator
            ));
            // Nothing in World mentions "inventory".
            assert!(!super::subtree_matches_filter(
                &FIXTURE_WORLD,
                "inventory",
                &translator
            ));
        });
        app.update();
    }

    /// A filter shows only the matching command and hides the rest.
    #[test]
    fn a_filter_hides_non_matching_commands() -> Result<(), TestError> {
        let mut app = filtered_popup_app(&FIXTURE_AVATAR, "fl")?;
        let commands = app
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(app.world())
            .count();
        assert_eq!(commands, 1, "only the matching Fly entry is shown");
        assert!(
            action_entity(&mut app, "fly").is_some(),
            "the matching entry is present",
        );
        assert!(
            action_entity(&mut app, "inventory").is_none(),
            "a non-matching entry is hidden",
        );
        Ok(())
    }

    /// A filter that matches the menu's own label shows the whole menu — the
    /// reference's downward "show everything under a matched menu" propagation.
    #[test]
    fn a_matched_menu_label_shows_every_entry() -> Result<(), TestError> {
        let mut app = filtered_popup_app(&FIXTURE_AVATAR, "avatar")?;
        let commands = app
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(app.world())
            .count();
        assert_eq!(
            commands, 5,
            "every command shows under a matched menu label"
        );
        Ok(())
    }

    /// A submenu is kept when its subtree carries a match, and dropped when it
    /// does not — so a hit nested one level deep is still reachable.
    #[test]
    fn a_filter_keeps_a_submenu_with_a_nested_match() -> Result<(), TestError> {
        let mut with_match = filtered_popup_app(&FIXTURE_WORLD, "sunset")?;
        let branches = with_match
            .world_mut()
            .query::<&MenuBranch>()
            .iter(with_match.world())
            .count();
        assert_eq!(branches, 1, "the Environment submenu is kept for its match");
        let top_commands = with_match
            .world_mut()
            .query::<&MenuEntryAction>()
            .iter(with_match.world())
            .count();
        assert_eq!(top_commands, 0, "no top-level World command matched");

        let mut without_match = filtered_popup_app(&FIXTURE_WORLD, "mini")?;
        let branches = without_match
            .world_mut()
            .query::<&MenuBranch>()
            .iter(without_match.world())
            .count();
        assert_eq!(
            branches, 0,
            "the submenu is dropped when nothing in it matches"
        );
        Ok(())
    }

    /// Spawn a live fixture bar (element `test-bar`) under a full menu-widget
    /// runtime, then apply the search filter `query` and settle. The bar's picks
    /// need the picking / keyboard resources the layout harness omits.
    fn filtered_bar_app(query: &str) -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        enable_action_recording(&mut app);
        app.init_resource::<HoverMap>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(super::MenuWidgetPlugin);
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                super::spawn_menu_bar(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    &FIXTURE_MENU_BAR,
                    "test-bar",
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        app.insert_resource(super::MenuFilter {
            element: "test-bar",
            query: query.to_lowercase(),
        });
        settle(&mut app);
        Ok(app)
    }

    /// A term opens the **first** bar menu (in bar order) that carries a match —
    /// the leftmost, even when a later menu also matches.
    #[test]
    fn a_term_opens_the_first_matching_menu() -> Result<(), TestError> {
        // "quit" is in Avatar (first). Avatar opens, World stays closed.
        let mut app = filtered_bar_app("quit")?;
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_some(),
            "the first matching menu opens",
        );
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-world").is_none(),
            "a non-matching (or later) menu stays closed",
        );
        Ok(())
    }

    /// When only a later menu matches, that one opens — bar order, not always the
    /// first menu.
    #[test]
    fn a_term_skips_to_a_later_matching_menu() -> Result<(), TestError> {
        // "teleport" is only in World (second); Avatar has no match.
        let mut app = filtered_bar_app("teleport")?;
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-world").is_some(),
            "the first *matching* menu opens, though it is not the first menu",
        );
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_none(),
            "the earlier non-matching menu is left closed",
        );
        Ok(())
    }

    /// Clearing the term closes the menu the filter opened.
    #[test]
    fn clearing_the_term_closes_the_menu() -> Result<(), TestError> {
        let mut app = filtered_bar_app("quit")?;
        assert!(find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_some());
        app.insert_resource(super::MenuFilter {
            element: "test-bar",
            query: String::new(),
        });
        settle(&mut app);
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_none(),
            "an empty term closes the filter-opened menu",
        );
        Ok(())
    }

    /// A drop-down hugs its content vertically — no dead space below the last
    /// entry. Guards the taffy quirk `build_menu_popup` sidesteps (an
    /// absolutely-positioned column that stretches its children was grown far
    /// taller than its rows, worst on the one-line "(no entries yet)" menu).
    #[test]
    fn a_popup_hugs_its_content_height() -> Result<(), TestError> {
        static PLACEHOLDER: MenuDef = MenuDef {
            label_key: "menu-fixture-comm",
            items: &[MenuItemDef::Command(
                super::MenuCommand::new("menu-fixture-no-entries-yet", "noop")
                    .enabled_when("never"),
            )],
        };
        let mut app = popup_app(&PLACEHOLDER, &[])?;
        let popup = find_by_name(&mut app, "menu-popup:menu-fixture-comm")
            .ok_or("the placeholder popup did not spawn")?;
        let row =
            find_by_name(&mut app, "menu-item:noop").ok_or("the placeholder row is missing")?;
        let popup_height = app
            .world()
            .entity(popup)
            .get::<bevy::ui::ComputedNode>()
            .ok_or("no computed node on the popup")?
            .size()
            .y;
        let row_height = app
            .world()
            .entity(row)
            .get::<bevy::ui::ComputedNode>()
            .ok_or("no computed node on the row")?
            .size()
            .y;
        // The popup is the row plus its own padding (4 px each side) and border
        // (1 px each side): 10 px of chrome, no dead line below.
        let expected = row_height + 10.0;
        assert!(
            (popup_height - expected).abs() < 2.0,
            "the popup should hug its one row ({expected} px), but is {popup_height} px tall — \
             dead space below the entry has crept back",
        );
        Ok(())
    }

    /// Every command row in a drop-down is the same width, so the hover highlight
    /// reads as a full-width bar rather than shrinking to each label — the width
    /// is filled by an explicit `width: 100%`, since the popup cannot use a
    /// cross-axis stretch (see [`a_popup_hugs_its_content_height`]).
    #[test]
    fn every_entry_row_is_full_width() -> Result<(), TestError> {
        let mut app = popup_app(&FIXTURE_AVATAR, &[])?;
        let widths: Vec<f32> = {
            let popup = find_by_name(&mut app, "menu-popup:menu-fixture-avatar")
                .ok_or("the Avatar popup did not spawn")?;
            let kids: Vec<Entity> = app
                .world()
                .entity(popup)
                .get::<Children>()
                .map(|c| c.iter().collect())
                .unwrap_or_default();
            kids.into_iter()
                .filter_map(|kid| {
                    let entity = app.world().entity(kid);
                    // Command rows only; a separator is a thin rule of its own and
                    // carries no `MenuEntryAction`.
                    entity.get::<MenuEntryAction>()?;
                    entity.get::<bevy::ui::ComputedNode>().map(|cn| cn.size().x)
                })
                .collect()
        };
        assert!(widths.len() >= 2, "expected several command rows");
        let first = widths.first().copied().unwrap_or(0.0);
        for width in &widths {
            assert!(
                (width - first).abs() < 1.0,
                "entry rows differ in width ({widths:?}) — the highlight would be ragged",
            );
        }
        Ok(())
    }

    /// The letter keys a jump key can be typed on — the inverse of
    /// [`keycode_to_letter`](super::keycode_to_letter), for a test that reads an
    /// assigned mnemonic off a row and has to type it.
    const LETTER_KEYS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];

    /// The resolved-label list `assign_jump_keys` reads: one entry per line in
    /// order, `None` for a separator.
    fn lines(labels: &[Option<&str>]) -> Vec<Option<String>> {
        labels
            .iter()
            .map(|label| label.map(ToOwned::to_owned))
            .collect()
    }

    /// Jump keys are the first free letter of each line's label, separators get
    /// none, and the offset points at that character.
    ///
    /// Stated in English labels rather than through a fixture's keys, because
    /// the rule is about the text the reader sees — which in another language is
    /// another set of letters entirely.
    #[test]
    fn jump_keys_are_the_first_free_letter() {
        // The fixture Avatar menu's shape: Inventory, Appearance, ―, Fly, Sit
        // Down, ―, Quit.
        let avatar: Vec<Option<char>> = assign_jump_keys(&lines(&[
            Some("Inventory"),
            Some("Appearance"),
            None,
            Some("Fly"),
            Some("Sit Down"),
            None,
            Some("Quit"),
        ]))
        .iter()
        .map(|assigned| assigned.map(|(key, _)| key))
        .collect();
        assert_eq!(
            avatar,
            vec![
                Some('I'),
                Some('A'),
                None,
                Some('F'),
                Some('S'),
                None,
                Some('Q'),
            ],
        );
    }

    /// A letter already taken by an earlier line is skipped to the next free one,
    /// so one menu never binds a key twice.
    #[test]
    fn jump_keys_avoid_collisions() {
        let keys = assign_jump_keys(&lines(&[Some("Save"), Some("Sit")]));
        // "Save" takes S; "Sit" cannot, so it takes the next free letter, 'I'@1.
        assert_eq!(keys.first().copied().flatten(), Some(('S', 0)));
        assert_eq!(keys.get(1).copied().flatten(), Some(('I', 1)));
    }

    /// A live fixture bar under the full widget runtime, with the keyboard / focus
    /// / mouse-motion resources the harness omits, settled closed.
    fn keyboard_bar_app() -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        // Every menu line resolves its `label_key`; with no bundles behind the
        // translator each one resolves to itself.
        sl_viewer_ui_core::i18n::install_untranslated(&mut app);
        enable_action_recording(&mut app);
        app.init_resource::<HoverMap>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(super::MenuWidgetPlugin);
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<UiRoot>| {
                super::spawn_menu_bar(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    &FIXTURE_MENU_BAR,
                    "test-bar",
                );
            })
            .after(UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// Give `entity` keyboard focus.
    fn focus(app: &mut App, entity: Entity) {
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(entity, FocusCause::Navigated);
    }

    /// Tap a key: press it for one frame, then release and step a few more so any
    /// deferred popup rows spawn and a pending first-child highlight resolves.
    ///
    /// The harness has no input plugin clearing `ButtonInput`, so the key must be
    /// **released** (not merely `clear`ed, which leaves it in the pressed set) or
    /// a second identical tap would not read as `just_pressed`.
    fn tap(app: &mut App, key: KeyCode) {
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(key);
            keys.press(key);
        }
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        }
        for _ in 0..4 {
            app.update();
        }
    }

    /// The keyboard-highlighted row, if any.
    fn highlighted(app: &App) -> Option<Entity> {
        app.world().resource::<MenuKeyboard>().highlighted
    }

    /// `Enter` on a focused bar button opens its menu into keyboard navigation and
    /// highlights the first (enabled) entry.
    #[test]
    fn enter_on_a_focused_button_opens_and_highlights_first() -> Result<(), TestError> {
        let mut app = keyboard_bar_app()?;
        let button = find_by_name(&mut app, "menu-button:menu-fixture-avatar")
            .ok_or("the Avatar button is missing")?;
        focus(&mut app, button);
        tap(&mut app, KeyCode::Enter);
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_some(),
            "the menu opened",
        );
        let inventory =
            find_by_name(&mut app, "menu-item:inventory").ok_or("the Inventory row is missing")?;
        assert_eq!(
            highlighted(&app),
            Some(inventory),
            "the first enabled entry is highlighted",
        );
        assert!(
            app.world().resource::<MenuKeyboard>().active,
            "keyboard navigation is active",
        );
        Ok(())
    }

    /// The block-axis arrows step the highlight, skipping the disabled entry, and
    /// `Enter` emits the highlighted entry's action and closes the menu.
    #[test]
    fn arrows_step_and_enter_activates() -> Result<(), TestError> {
        let mut app = keyboard_bar_app()?;
        let button = find_by_name(&mut app, "menu-button:menu-fixture-avatar")
            .ok_or("the Avatar button is missing")?;
        focus(&mut app, button);
        tap(&mut app, KeyCode::Enter);
        // Inventory → Appearance (Fly's predecessor Sit is disabled, but we stop
        // at Appearance here).
        tap(&mut app, KeyCode::ArrowDown);
        let appearance = find_by_name(&mut app, "menu-item:appearance")
            .ok_or("the Appearance row is missing")?;
        assert_eq!(
            highlighted(&app),
            Some(appearance),
            "Down moved the highlight"
        );
        tap(&mut app, KeyCode::Enter);
        assert_eq!(
            drain_actions(&mut app),
            vec![UiAction {
                element: "test-bar",
                action: "appearance",
            }],
            "Enter activated the highlighted entry",
        );
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-avatar").is_none(),
            "activating an entry closes the menu",
        );
        Ok(())
    }

    /// Navigation skips a disabled entry: from Appearance, Down lands on Fly, not
    /// the disabled Sit between them.
    #[test]
    fn navigation_skips_a_disabled_entry() -> Result<(), TestError> {
        let mut app = keyboard_bar_app()?;
        let button = find_by_name(&mut app, "menu-button:menu-fixture-avatar")
            .ok_or("the Avatar button is missing")?;
        focus(&mut app, button);
        tap(&mut app, KeyCode::Enter);
        tap(&mut app, KeyCode::ArrowDown); // Appearance
        tap(&mut app, KeyCode::ArrowDown); // Fly (Sit is disabled, skipped)
        let fly = find_by_name(&mut app, "menu-item:fly").ok_or("the Fly row is missing")?;
        assert_eq!(
            highlighted(&app),
            Some(fly),
            "the disabled Sit entry is stepped over",
        );
        Ok(())
    }

    /// A jump key jumps straight to its entry and commits it — typing Quit's own
    /// mnemonic activates it without stepping to it.
    ///
    /// The letter is read off the row the widget built rather than written down
    /// here: the mnemonic comes from the label's *resolved* text
    /// ([`assign_jump_keys`](super::assign_jump_keys)), so pinning a letter
    /// would pin one language. What this is about is the dispatch — a typed
    /// letter reaching the row that carries it.
    #[test]
    fn a_jump_key_activates_its_entry() -> Result<(), TestError> {
        let mut app = keyboard_bar_app()?;
        let button = find_by_name(&mut app, "menu-button:menu-fixture-avatar")
            .ok_or("the Avatar button is missing")?;
        focus(&mut app, button);
        tap(&mut app, KeyCode::Enter);
        let quit = action_entity(&mut app, "quit").ok_or("the Quit entry is missing")?;
        let mnemonic = app
            .world()
            .get::<MenuMnemonic>(quit)
            .ok_or("Quit got no jump key")?
            .key;
        let typed = LETTER_KEYS
            .into_iter()
            .find(|code| super::keycode_to_letter(*code) == Some(mnemonic))
            .ok_or("Quit's mnemonic is not a letter key")?;
        tap(&mut app, typed);
        assert_eq!(
            drain_actions(&mut app),
            vec![UiAction {
                element: "test-bar",
                action: "quit",
            }],
            "Quit's own jump key activated it",
        );
        Ok(())
    }

    /// The inline-end arrow opens a highlighted submenu and lands on its first
    /// entry; the inline-start arrow closes it and returns to the branch row.
    #[test]
    fn inline_arrows_open_and_close_a_submenu() -> Result<(), TestError> {
        let mut app = keyboard_bar_app()?;
        let button = find_by_name(&mut app, "menu-button:menu-fixture-world")
            .ok_or("the World button is missing")?;
        focus(&mut app, button);
        tap(&mut app, KeyCode::Enter); // World open, Mini-Map highlighted
        tap(&mut app, KeyCode::ArrowDown); // Environment (the submenu branch)
        let branch = find_by_name(&mut app, "menu-submenu:menu-fixture-environment")
            .ok_or("the Environment branch is missing")?;
        assert_eq!(highlighted(&app), Some(branch), "the branch is highlighted");
        tap(&mut app, KeyCode::ArrowRight); // open the submenu, land on its first entry
        let sunrise = find_by_name(&mut app, "menu-item:env-sunrise")
            .ok_or("the submenu's first entry is missing")?;
        assert_eq!(
            highlighted(&app),
            Some(sunrise),
            "the submenu opened and its first entry is highlighted",
        );
        tap(&mut app, KeyCode::ArrowLeft); // close the submenu, back to the branch
        assert_eq!(
            highlighted(&app),
            Some(branch),
            "closing the submenu returns to the branch row",
        );
        assert!(
            find_by_name(&mut app, "menu-popup:menu-fixture-environment").is_none(),
            "the submenu popup is gone",
        );
        Ok(())
    }

    /// Mnemonic characters are underlined exactly while keyboard navigation is
    /// active, and the underline is cleared once the menu closes.
    #[test]
    fn mnemonics_underline_only_while_navigating() -> Result<(), TestError> {
        let mut app = keyboard_bar_app()?;
        let button = find_by_name(&mut app, "menu-button:menu-fixture-avatar")
            .ok_or("the Avatar button is missing")?;
        focus(&mut app, button);
        tap(&mut app, KeyCode::Enter);
        let underlined = app
            .world_mut()
            .query_filtered::<(), (With<MnemonicSpan>, With<Underline>)>()
            .iter(app.world())
            .count();
        assert!(
            underlined > 0,
            "mnemonic characters underline once keyboard navigation begins",
        );
        Ok(())
    }

    /// The entity of the command line emitting `action`, if present.
    fn action_entity(app: &mut App, action: &str) -> Option<Entity> {
        app.world_mut()
            .query::<(Entity, &MenuEntryAction)>()
            .iter(app.world())
            .find(|(_, entry)| entry.action == action)
            .map(|(entity, _)| entity)
    }

    /// How many entities carry the given `Name` — for counting separators.
    fn count_named(app: &mut App, name: &str) -> usize {
        app.world_mut()
            .query::<&Name>()
            .iter(app.world())
            .filter(|entity_name| entity_name.as_str() == name)
            .count()
    }

    /// **The bar, driven** (`viewer-ui-widget-interaction-suite`): open,
    /// navigate, dismiss — with a pointer that has to hit what it aims at and
    /// keys that arrive as keys.
    ///
    /// Three of this widget's behaviours are *defined* in terms of the pointer
    /// and cannot be reached any other way:
    ///
    /// - `switch_menu_on_hover` reads the live `HoverMap`, so the sweep across
    ///   the bar (one menu closing as the next opens, with no click) simply
    ///   does not happen in a harness that has no hover;
    /// - `manage_submenus` opens and closes a branch's child list from the same
    ///   map, hit-testing through the popup that escaped its clipping ancestor;
    /// - the root dismiss observer only ever fires for a press that reached the
    ///   root, which is a statement about what the popups consume.
    ///
    /// The keyboard half is here for a different reason: live, a menu is opened
    /// with the mouse and then walked with the arrows, and the focus that makes
    /// that work is placed by the *click*. The tests above set focus by hand
    /// and would keep passing if a click had stopped placing it.
    mod scenarios {
        use bevy::input::keyboard::Key;
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;

        use super::{FIXTURE_MENU_BAR, MenuHost, MenuKeyboard, TestError};
        use crate::menu::{MenuWidgetPlugin, spawn_menu_bar};
        use crate::ui_test::interact::{self, InteractionTest};
        use crate::ui_test::{drain_actions, enable_action_recording, find_by_name, settle};
        use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems};
        use sl_viewer_ui_core::ui_element::{ElementCx, UiAction};

        /// The element the fixture bar attributes its actions to.
        const ELEMENT: &str = "test-bar";

        /// The fixture menu bar under the real pointer stack.
        fn bar_app() -> App {
            let mut app = InteractionTest::new().build();
            // Every menu line resolves its `label_key`; with no bundles behind the
            // translator each one resolves to itself.
            sl_viewer_ui_core::i18n::install_untranslated(&mut app);
            app.add_plugins(MenuWidgetPlugin);
            enable_action_recording(&mut app);
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_menu_bar(
                        &mut commands,
                        root.0,
                        ElementCx::new(),
                        &FIXTURE_MENU_BAR,
                        ELEMENT,
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            app
        }

        /// How many of the bar's menus are open.
        fn open_menus(app: &mut App) -> usize {
            app.world_mut()
                .query::<&MenuHost>()
                .iter(app.world())
                .filter(|host| host.open.is_some())
                .count()
        }

        /// Whether the named node exists (a popup is spawned on open and
        /// despawned on close, so its presence *is* its openness).
        fn present(app: &mut App, name: &str) -> bool {
            find_by_name(app, name).is_some()
        }

        /// A click opens a top menu; sweeping the pointer onto the next button
        /// switches to that one without a second click; a click on the open
        /// button closes it again.
        #[test]
        fn a_click_opens_a_menu_and_a_sweep_switches_it() -> Result<(), TestError> {
            let mut app = bar_app();
            assert_eq!(open_menus(&mut app), 0, "the bar rests closed");

            interact::click_node(&mut app, "menu-button:menu-fixture-avatar")?;
            settle(&mut app);
            assert!(
                present(&mut app, "menu-popup:menu-fixture-avatar"),
                "the click dropped it"
            );

            // No click: the bar reads as one strip you sweep across once
            // something on it is open.
            interact::hover_node(&mut app, "menu-button:menu-fixture-world")?;
            settle(&mut app);
            assert!(
                present(&mut app, "menu-popup:menu-fixture-world"),
                "hovering the next button opens that menu"
            );
            assert!(
                !present(&mut app, "menu-popup:menu-fixture-avatar"),
                "and the previous one closes — at most one bar menu is ever down"
            );
            assert_eq!(open_menus(&mut app), 1);

            interact::click_node(&mut app, "menu-button:menu-fixture-world")?;
            settle(&mut app);
            assert_eq!(
                open_menus(&mut app),
                0,
                "clicking the open button closes it"
            );
            Ok(())
        }

        /// Hovering a branch line opens its submenu, and sweeping off it (onto
        /// a sibling line) closes it again.
        #[test]
        fn hovering_a_branch_opens_its_submenu_and_leaving_closes_it() -> Result<(), TestError> {
            let mut app = bar_app();
            interact::click_node(&mut app, "menu-button:menu-fixture-world")?;
            settle(&mut app);

            interact::hover_node(&mut app, "menu-submenu:menu-fixture-environment")?;
            settle(&mut app);
            assert!(
                present(&mut app, "menu-popup:menu-fixture-environment"),
                "the branch's child list opens under the pointer"
            );
            assert!(
                present(&mut app, "menu-item:env-midday"),
                "and it is the child list, with its own entries"
            );

            interact::hover_node(&mut app, "menu-item:teleport-home")?;
            settle(&mut app);
            assert!(
                !present(&mut app, "menu-popup:menu-fixture-environment"),
                "sweeping onto a sibling line closes the submenu again"
            );
            Ok(())
        }

        /// Clicking an entry emits its action and takes the whole bar down.
        #[test]
        fn an_entry_click_emits_its_action_and_closes_the_bar() -> Result<(), TestError> {
            let mut app = bar_app();
            interact::click_node(&mut app, "menu-button:menu-fixture-avatar")?;
            settle(&mut app);
            let _opening = drain_actions(&mut app);

            interact::click_node(&mut app, "menu-item:appearance")?;
            settle(&mut app);

            let actions: Vec<UiAction> = drain_actions(&mut app);
            let action = actions.first().ok_or("the entry emitted nothing")?;
            assert_eq!(action.action, "appearance");
            assert_eq!(action.element, ELEMENT);
            assert_eq!(actions.len(), 1, "one pick, one action: {actions:?}");
            assert_eq!(open_menus(&mut app), 0, "picking closes the menu");
            Ok(())
        }

        /// A press outside every popup dismisses the bar and emits nothing —
        /// the escape route from a menu opened by mistake.
        #[test]
        fn an_outside_press_dismisses_the_bar() -> Result<(), TestError> {
            let mut app = bar_app();
            interact::click_node(&mut app, "menu-button:menu-fixture-avatar")?;
            settle(&mut app);
            let _opening = drain_actions(&mut app);

            interact::click(&mut app, Vec2::new(900.0, 800.0), MouseButton::Left);
            settle(&mut app);

            assert_eq!(open_menus(&mut app), 0, "the press outside closed it");
            let actions = drain_actions(&mut app);
            assert!(actions.is_empty(), "and picked nothing: {actions:?}");
            Ok(())
        }

        /// The mixed gesture a user actually makes: open with the mouse, walk
        /// with the arrows, commit with `Enter`.
        ///
        /// The keyboard only reaches the menu because the click placed focus on
        /// the bar button, so this is as much a test of the click as of the
        /// arrows.
        #[test]
        fn a_clicked_menu_is_walked_and_activated_from_the_keyboard() -> Result<(), TestError> {
            let mut app = bar_app();
            interact::click_node(&mut app, "menu-button:menu-fixture-avatar")?;
            settle(&mut app);
            let _opening = drain_actions(&mut app);
            assert!(
                app.world().resource::<MenuKeyboard>().focus_captured,
                "opening by click hands the keyboard to the menu"
            );

            // Down to the first entry, then down again to the second.
            interact::tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
            settle(&mut app);
            let first = app.world().resource::<MenuKeyboard>().highlighted;
            interact::tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
            settle(&mut app);
            let second = app.world().resource::<MenuKeyboard>().highlighted;
            assert!(
                first.is_some() && first != second,
                "the arrows step the highlight: {first:?} -> {second:?}"
            );

            interact::tap(&mut app, KeyCode::Enter, Key::Enter);
            settle(&mut app);

            let actions: Vec<UiAction> = drain_actions(&mut app);
            let action = actions.first().ok_or("Enter activated nothing")?;
            assert_eq!(
                action.action, "appearance",
                "Enter activates the highlighted entry, the second one"
            );
            assert_eq!(open_menus(&mut app), 0, "and the bar closes behind it");
            Ok(())
        }

        /// `Escape` closes an open menu without picking anything.
        #[test]
        fn escape_closes_the_open_menu() -> Result<(), TestError> {
            let mut app = bar_app();
            interact::click_node(&mut app, "menu-button:menu-fixture-avatar")?;
            settle(&mut app);
            let _opening = drain_actions(&mut app);

            interact::tap(&mut app, KeyCode::Escape, Key::Escape);
            settle(&mut app);

            assert_eq!(open_menus(&mut app), 0, "Escape closes the menu");
            let actions = drain_actions(&mut app);
            assert!(actions.is_empty(), "and picks nothing: {actions:?}");
            Ok(())
        }

        /// **The accelerator a menu draws is the accelerator it answers to**:
        /// pressing `Ctrl+I` emits Inventory's command without the menu ever
        /// being opened.
        ///
        /// This is the positive counterpart the old canary asked for. It used
        /// to assert the opposite — the chords this widget drew were labels
        /// with nothing behind them (`viewer-menu-accelerators-inert`), each
        /// working chord in the viewer having its own bespoke handler
        /// elsewhere — pinned so that the day a generic dispatcher landed the
        /// test would fail and have to be rewritten rather than go on passing
        /// and stop meaning anything. [`crate::menu_accel`] is that dispatcher,
        /// and this is that rewrite.
        ///
        /// The drawn label is still asserted first: without it this would be
        /// testing a chord that happens to work rather than the promise a menu
        /// makes to the user by printing it.
        #[test]
        fn a_drawn_accelerator_runs_its_entry() -> Result<(), TestError> {
            let mut app = bar_app();
            let _resting = drain_actions(&mut app);
            // The label is really drawn — otherwise this would be asserting a
            // chord that works rather than the accelerator's promise kept.
            assert!(
                present(&mut app, "menu-button:menu-fixture-avatar"),
                "the fixture bar is up"
            );

            interact::with_modifier(&mut app, KeyCode::ControlLeft, Key::Control, |app| {
                interact::tap(app, KeyCode::KeyI, Key::Character("i".into()));
            });
            settle(&mut app);

            let actions: Vec<UiAction> = drain_actions(&mut app);
            let action = actions.first().ok_or("the accelerator emitted nothing")?;
            assert_eq!(
                action.action, "inventory",
                "`Ctrl+I` must run the entry it is drawn against"
            );
            assert_eq!(action.element, ELEMENT);
            assert_eq!(actions.len(), 1, "one chord, one action: {actions:?}");
            assert_eq!(
                open_menus(&mut app),
                0,
                "and reaches it without opening the menu"
            );
            Ok(())
        }
    }
}
